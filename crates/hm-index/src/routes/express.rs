//! Express routes and `use` middleware (Learn spec §6.3).
//!
//! A receiver is a name the same file binds to `express()`, `express.Router()` or `Router()`
//! imported from `express`. `X.use('/pre', Y)` mounts router `Y` (a local binding, or one
//! the imported file exports under the imported name) on `X`; prefixes join through at
//! most five mounts, and a router mounted twice gives both sets of routes. Conventions,
//! checked against the official docs:
//! - <https://expressjs.com/en/guide/routing.html> (`app.METHOD`, `app.all`, several
//!   callbacks per route, `express.Router`, `:name` parameters, Express 5 `*name` and
//!   `{*name}` wildcards and `{…}` optional parts);
//! - <https://expressjs.com/en/5x/api.html#router> (`use([path,] fn…)`: the path defaults
//!   to `/` and matches its sub-paths; middleware runs in the order it is declared).
//!
//! A path written as a template with `${…}` cannot be known: such a mount is not joined
//! (with a note), such a `use` gives no edge and such a route call is not a route.
//!
//! Documented limits:
//! - `app.route('/x').get(…)` chains, regular-expression and array paths;
//! - routers exported with `module.exports` or through a re-export, mounted through a
//!   namespace import (`app.use('/x', ns.router)`), and receivers bound in another file;
//! - no scope tracking: a parameter that shadows a module-level receiver
//!   (`function mount(app) { app.get(…) }`) is taken for that receiver;
//! - Express 5 optional literal parts are approximated by dropping their braces
//!   (`/user{s}` → `/users`), and a wildcard inside a segment (Nest's `ab*cd`) becomes a
//!   catch-all there (`ab{...}cd`).

use std::collections::{BTreeMap, BTreeSet};

use super::{Derived, Ids, InferredEdge, Inputs, Route, route};
use crate::facts::{ArgFact, FileFacts, ImportKind, Owner, SymbolKind};
use crate::ts::resolve::{ModuleTarget, Project, binding_package, resolve_callee};

/// The route methods and their verb: the HTTP verbs, and `ANY` for `all`.
const VERBS: [(&str, &str); 8] = [
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
    ("all", "ANY"),
];

/// How many mounts a prefix chain may have.
const MAX_MOUNTS: usize = 5;

/// A name bound to an Express app or router: `(file, enclosing symbol, name)`.
type Node = (String, Owner, String);

/// The order of a call in its file: `(line, argument index)`.
type Key = (u32, usize);

/// A route call on a receiver.
struct Declared {
    node: usize,
    verb: &'static str,
    path: String,
    file: String,
    line: u32,
    handler_id: Option<String>,
    /// The functions before the handler, which run first.
    before: Vec<String>,
}

/// `parent.use(prefix, child)`.
struct Mount {
    parent: usize,
    prefix: String,
    key: Key,
    child: usize,
}

/// One function of `node.use([path,] fn…)` that is not a router, by its symbol id.
struct Use {
    node: usize,
    path: Option<String>,
    key: Key,
    file: String,
    line: u32,
    function: String,
}

/// One step of the way from an app down to a route: at `node`, the call at `key` adds
/// `path` (a mount prefix, or the route's own path for the last step).
#[derive(Clone)]
struct Hop {
    node: usize,
    key: Key,
    path: String,
}

/// A declared route reached through one chain of mounts.
struct Instance {
    declared: usize,
    hops: Vec<Hop>,
}

/// Every receiver, route call, mount and `use` of the tree.
struct Graph {
    nodes: Vec<Node>,
    /// The line of each node's binding, and whether it is an app (`express()`).
    bound: Vec<(u32, bool)>,
    declared: Vec<Declared>,
    mounts: Vec<Mount>,
    uses: Vec<Use>,
    /// Mounts whose path could not be read.
    notes: Vec<String>,
}

/// An emitted route with the instance it came from.
struct Emitted {
    route: Route,
    instance: Instance,
}

/// The routes of `express()` apps and routers.
pub(crate) fn routes(inputs: &Inputs, out: &mut Derived) {
    let mut ids = Ids::new(inputs.project);
    let graph = graph(inputs.project, &mut ids);
    let (emitted, notes) = emit(inputs, &graph);
    out.routes.extend(emitted.into_iter().map(|e| e.route));
    out.notes.extend(notes);
}

/// The `ts.express.use` edges from each route to the `use` functions before it.
pub(crate) fn before_edges(inputs: &Inputs, out: &mut Derived) {
    let mut ids = Ids::new(inputs.project);
    let graph = graph(inputs.project, &mut ids);
    let (emitted, _) = emit(inputs, &graph);
    // A route dropped as a duplicate shares its id with the kept one; only the kept
    // declaration (same file and line) lends its edges.
    let kept: BTreeSet<(&str, &str, u32)> = out
        .routes
        .iter()
        .map(|r| (r.id.as_str(), r.file.as_str(), r.line))
        .collect();
    let mut edges = BTreeSet::new();
    for e in emitted.iter().filter(|e| {
        let r = &e.route;
        kept.contains(&(r.id.as_str(), r.file.as_str(), r.line))
    }) {
        let declared = &graph.declared[e.instance.declared];
        for f in &declared.before {
            edges.insert(edge(&e.route.id, f, &declared.file, declared.line));
        }
        for u in &graph.uses {
            let Some(at) = e.instance.hops.iter().position(|h| h.node == u.node) else {
                continue;
            };
            if e.instance.hops[at].key <= u.key {
                continue;
            }
            let below: Vec<&str> = e.instance.hops[at..]
                .iter()
                .map(|h| h.path.as_str())
                .collect();
            if u.path.as_deref().is_some_and(|p| !covers(p, &below)) {
                continue;
            }
            edges.insert(edge(&e.route.id, &u.function, &u.file, u.line));
        }
    }
    out.edges.extend(edges);
}

fn edge(src: &str, dst: &str, file: &str, line: u32) -> InferredEdge {
    InferredEdge {
        kind: "calls".to_string(),
        src_id: src.to_string(),
        dst_id: dst.to_string(),
        file: file.to_string(),
        line,
        rule: "ts.express.use".to_string(),
    }
}

/// Whether `init` (`express`, `express.Router`, `Router`) makes an Express app or router
/// in `rel`: its root name must be bound to the `express` package.
fn is_express(project: &Project, rel: &str, init: &str) -> bool {
    let (root, wanted): (&str, &[&str]) = match init {
        "express" | "express.Router" => ("express", &["default", "*"]),
        "Router" => ("Router", &["Router"]),
        _ => return false,
    };
    matches!(
        binding_package(project, rel, root),
        Some((package, imported)) if package == "express" && wanted.contains(&imported.as_str())
    )
}

/// The node `name` names in `rel` from inside `owner`: a binding of that symbol, else one
/// at module level.
fn local_node(
    index: &BTreeMap<Node, usize>,
    rel: &str,
    owner: &Owner,
    name: &str,
) -> Option<usize> {
    let key = |owner: &Owner| (rel.to_string(), owner.clone(), name.to_string());
    index
        .get(&key(owner))
        .or_else(|| index.get(&key(&None)))
        .copied()
}

/// The router `name` names in `rel` from inside `owner`: a local one, or the module-level
/// binding that the imported file exports under the imported name (`export default r`,
/// `export { r }`, `export const r = Router()`).
fn router_node(
    project: &Project,
    index: &BTreeMap<Node, usize>,
    facts: &FileFacts,
    rel: &str,
    owner: &Owner,
    name: &str,
) -> Option<usize> {
    if let Some(node) = local_node(index, rel, owner, name) {
        return Some(node);
    }
    let (specifier, imported) = facts
        .imports
        .iter()
        .filter(|i| i.kind != ImportKind::ReExport && !i.type_only)
        .find_map(|i| {
            let n = i.names.iter().find(|n| n.local == name)?;
            Some((i.specifier.as_str(), n.imported.as_str()))
        })?;
    let ModuleTarget::File(target) = project.modules.resolve(rel, specifier) else {
        return None;
    };
    let export = project
        .facts
        .get(&target)?
        .exports
        .iter()
        .find(|e| !e.star && e.from.is_none() && e.name == imported)?;
    let local = export.local.as_deref().unwrap_or(imported);
    index
        .get(&(target.clone(), None, local.to_string()))
        .copied()
}

/// The symbol a `Name` argument names, if it names exactly one.
fn named_symbol(project: &Project, ids: &mut Ids, rel: &str, name: &str) -> Option<String> {
    let (receiver, callee) = match name.rsplit_once('.') {
        Some((receiver, callee)) => (Some(receiver), callee),
        None => (None, name),
    };
    let (file, index) = resolve_callee(project, rel, receiver, callee)?;
    ids.symbol(&file, index)
}

/// The handler of a route call: its last argument.
fn handler(
    project: &Project,
    ids: &mut Ids,
    facts: &FileFacts,
    rel: &str,
    last: &ArgFact,
    line: u32,
) -> Option<String> {
    match last {
        ArgFact::Name(name) => named_symbol(project, ids, rel, name),
        // The callback symbol of this call: of the top-level functions with that name,
        // the first one that starts at or after the call.
        ArgFact::Inline(Some(name)) => {
            let index = facts
                .symbols
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.owner.is_empty()
                        && s.name == *name
                        && s.kind == SymbolKind::Function
                        && s.l0 >= line
                })
                .min_by_key(|(_, s)| s.l0)
                .map(|(i, _)| i)?;
            ids.symbol(rel, index)
        }
        _ => None,
    }
}

fn graph(project: &Project, ids: &mut Ids) -> Graph {
    let mut index: BTreeMap<Node, usize> = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut bound = Vec::new();
    for (rel, facts) in project.facts {
        for b in &facts.web.bindings {
            // An exported binding is a variable symbol, and the owner of its own
            // initialiser: it is bound at module level all the same.
            let own = (String::new(), b.name.clone());
            let owner = b.owner.clone().filter(|o| *o != own);
            let node = (rel.clone(), owner, b.name.clone());
            if index.contains_key(&node) || !is_express(project, rel, &b.init) {
                continue;
            }
            index.insert(node.clone(), nodes.len());
            nodes.push(node);
            bound.push((b.line, b.init == "express"));
        }
    }

    let mut graph = Graph {
        nodes,
        bound,
        declared: Vec::new(),
        mounts: Vec::new(),
        uses: Vec::new(),
        notes: Vec::new(),
    };
    for (rel, facts) in project.facts {
        for c in &facts.web.route_calls {
            let Some(node) = local_node(&index, rel, &c.owner, &c.receiver) else {
                continue;
            };
            if c.method == "use" {
                let (path, first) = match c.args.first() {
                    Some(ArgFact::Str(path)) => (Some(path.clone()), 1),
                    _ => (None, 0),
                };
                let readable = path.as_deref().is_none_or(is_literal);
                for (i, arg) in c.args.iter().enumerate().skip(first) {
                    let ArgFact::Name(name) = arg else {
                        continue;
                    };
                    let key = (c.line, i);
                    let router = router_node(project, &index, facts, rel, &c.owner, name);
                    if !readable {
                        if router.is_some() {
                            graph.notes.push(format!(
                                "{rel}:{}: the mount path of `{name}` is not a literal; it is \
                                 not joined",
                                c.line
                            ));
                        }
                        continue;
                    }
                    if let Some(child) = router {
                        graph.mounts.push(Mount {
                            parent: node,
                            prefix: path.clone().unwrap_or_default(),
                            key,
                            child,
                        });
                    } else if let Some(function) = named_symbol(project, ids, rel, name) {
                        graph.uses.push(Use {
                            node,
                            path: path.clone(),
                            key,
                            file: rel.clone(),
                            line: c.line,
                            function,
                        });
                    }
                }
                continue;
            }
            let Some(&(_, verb)) = VERBS.iter().find(|(m, _)| *m == c.method) else {
                continue;
            };
            let (Some(ArgFact::Str(path)), Some(last)) = (c.args.first(), c.args.last()) else {
                continue;
            };
            if c.args.len() < 2 || !is_literal(path) {
                continue;
            }
            let before = c.args[1..c.args.len() - 1]
                .iter()
                .filter_map(|a| match a {
                    ArgFact::Name(name) => named_symbol(project, ids, rel, name),
                    _ => None,
                })
                .collect();
            graph.declared.push(Declared {
                node,
                verb,
                path: path.clone(),
                file: rel.clone(),
                line: c.line,
                handler_id: handler(project, ids, facts, rel, last, c.line),
                before,
            });
        }
    }
    graph
}

/// Whether a `Str` path is a literal: a template with `${…}` (spelt `{}`) is not.
pub(super) fn is_literal(path: &str) -> bool {
    !path.contains("{}")
}

/// Every declared route through every chain of mounts from an app, and the notes.
fn emit(inputs: &Inputs, graph: &Graph) -> (Vec<Emitted>, Vec<String>) {
    let mounted: BTreeSet<usize> = graph.mounts.iter().map(|m| m.child).collect();
    let mut instances = Vec::new();
    let mut reached = BTreeSet::new();
    for root in (0..graph.nodes.len()).filter(|n| !mounted.contains(n)) {
        let app = graph.bound[root].1;
        let mut visiting = vec![root];
        descend(
            graph,
            root,
            app,
            &mut Vec::new(),
            &mut visiting,
            &mut instances,
            &mut reached,
        );
    }
    // What no app reaches is mounted only through a cycle or more than five mounts deep.
    // Each group of the unreached part that nothing else unreached mounts (a cycle, or one
    // router) is walked from every one of its routers, never through the group again, so
    // the result does not depend on the order of the files.
    let mut notes = graph.notes.clone();
    loop {
        let unreached: BTreeSet<usize> = (0..graph.nodes.len())
            .filter(|n| !reached.contains(n))
            .collect();
        if unreached.is_empty() {
            break;
        }
        for group in source_groups(graph, &unreached) {
            let before = instances.len();
            for &start in &group {
                let mut visiting = group.clone();
                descend(
                    graph,
                    start,
                    false,
                    &mut Vec::new(),
                    &mut visiting,
                    &mut instances,
                    &mut reached,
                );
            }
            if instances.len() > before {
                notes.push(fallback_note(graph, &group));
            }
        }
    }

    let emitted = instances
        .into_iter()
        .map(|(mounted, instance)| {
            let d = &graph.declared[instance.declared];
            let paths: Vec<&str> = instance.hops.iter().map(|h| h.path.as_str()).collect();
            let pattern = path_pattern(&paths);
            let app_root = inputs
                .manifests
                .root_of(&d.file)
                .map_or("", |m| m.root.as_str());
            let detail = (!mounted).then(|| r#"{"mounted":false}"#.to_string());
            let route = Route {
                handler_id: d.handler_id.clone(),
                conf: "probably".to_string(),
                detail,
                ..route(
                    app_root,
                    "http",
                    d.verb,
                    &pattern,
                    &d.file,
                    d.line,
                    "ts.express.route",
                )
            };
            Emitted { route, instance }
        })
        .collect();
    (emitted, notes)
}

/// Emits the routes of `node` and of the routers mounted on it, `chain` being the mounts
/// from the root down to `node`, never entering a router of `visiting`. `mounted` says
/// whether the root is an app.
fn descend(
    graph: &Graph,
    node: usize,
    mounted: bool,
    chain: &mut Vec<Hop>,
    visiting: &mut Vec<usize>,
    out: &mut Vec<(bool, Instance)>,
    reached: &mut BTreeSet<usize>,
) {
    reached.insert(node);
    for (i, d) in graph
        .declared
        .iter()
        .enumerate()
        .filter(|(_, d)| d.node == node)
    {
        let mut hops = chain.clone();
        hops.push(Hop {
            node,
            key: (d.line, 0),
            path: d.path.clone(),
        });
        out.push((mounted, Instance { declared: i, hops }));
    }
    if chain.len() >= MAX_MOUNTS {
        return;
    }
    let mut mounts: Vec<&Mount> = graph.mounts.iter().filter(|m| m.parent == node).collect();
    mounts.sort_by_key(|m| m.key);
    for m in mounts {
        if visiting.contains(&m.child) {
            continue;
        }
        chain.push(Hop {
            node,
            key: m.key,
            path: m.prefix.clone(),
        });
        visiting.push(m.child);
        descend(graph, m.child, mounted, chain, visiting, out, reached);
        visiting.pop();
        chain.pop();
    }
}

/// The groups of `unreached` that no other unreached router mounts: each is a cycle (its
/// routers mount one another) or one router. In node order.
fn source_groups(graph: &Graph, unreached: &BTreeSet<usize>) -> Vec<Vec<usize>> {
    let inner: Vec<&Mount> = graph
        .mounts
        .iter()
        .filter(|m| unreached.contains(&m.parent) && unreached.contains(&m.child))
        .collect();
    // What each unreached router reaches through unreached routers, itself included.
    let reach: BTreeMap<usize, BTreeSet<usize>> = unreached
        .iter()
        .map(|&from| {
            let mut seen = BTreeSet::from([from]);
            let mut todo = vec![from];
            while let Some(n) = todo.pop() {
                for m in inner.iter().filter(|m| m.parent == n) {
                    if seen.insert(m.child) {
                        todo.push(m.child);
                    }
                }
            }
            (from, seen)
        })
        .collect();
    let mut groups: BTreeSet<Vec<usize>> = BTreeSet::new();
    for &n in unreached {
        let group: Vec<usize> = reach[&n]
            .iter()
            .copied()
            .filter(|m| reach[m].contains(&n))
            .collect();
        let entered = inner
            .iter()
            .any(|m| group.contains(&m.child) && !group.contains(&m.parent));
        if !entered {
            groups.insert(group);
        }
    }
    groups.into_iter().collect()
}

/// The note for a group walked without an app: a cycle names every router in it.
fn fallback_note(graph: &Graph, group: &[usize]) -> String {
    let at = |n: usize| format!("{}:{}", graph.nodes[n].0, graph.bound[n].0);
    let cycle = group.len() > 1
        || graph
            .mounts
            .iter()
            .any(|m| m.parent == group[0] && m.child == group[0]);
    if cycle {
        let list: Vec<String> = group
            .iter()
            .map(|&n| format!("`{}` ({})", graph.nodes[n].2, at(n)))
            .collect();
        format!(
            "{}: a mount cycle through {}; their paths are kept without the prefixes of the \
             cycle",
            at(group[0]),
            list.join(", ")
        )
    } else {
        format!(
            "{}: router `{}` is mounted more than {MAX_MOUNTS} mounts deep; its paths are \
             kept without a prefix",
            at(group[0]),
            graph.nodes[group[0]].2
        )
    }
}

/// Whether `use` path `prefix` covers a route whose path below the `use` receiver is
/// `below`: segment by segment, a parameter of the prefix matching any one segment and a
/// wildcard the rest.
fn covers(prefix: &str, below: &[&str]) -> bool {
    let wanted = path_pattern(&[prefix]);
    let route = path_pattern(below);
    let route: Vec<&str> = route.split('/').filter(|s| !s.is_empty()).collect();
    for (i, segment) in wanted.split('/').filter(|s| !s.is_empty()).enumerate() {
        if segment.starts_with("{...") {
            return true;
        }
        match route.get(i) {
            Some(r) if *r == segment || segment.starts_with('{') => {}
            _ => return false,
        }
    }
    true
}

/// The route pattern of path parts joined in order, in the spelling of the Learn spec:
/// `/`-separated, starting with `/`, `:id` → `{id}`, `*name` and `{*name}` → `{...name}`, a
/// bare `*` → `{...}`, and the braces of an optional part dropped (`/order{/:id}` →
/// `/order/{id}`). Express (path-to-regexp) and NestJS paths share this syntax.
pub(super) fn path_pattern(parts: &[&str]) -> String {
    let joined = parts.join("/");
    let mut converted = String::new();
    let mut chars = joined.chars().peekable();
    let name = |chars: &mut std::iter::Peekable<std::str::Chars>| {
        let mut name = String::new();
        while let Some(&c) = chars.peek() {
            if !(c.is_alphanumeric() || c == '_' || c == '$') {
                break;
            }
            name.push(c);
            chars.next();
        }
        name
    };
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'*') => {
                chars.next();
                converted.push_str(&format!("{{...{}}}", name(&mut chars)));
                if chars.peek() == Some(&'}') {
                    chars.next();
                }
            }
            '{' | '}' => {}
            ':' => {
                let n = name(&mut chars);
                if n.is_empty() {
                    converted.push(':');
                } else {
                    converted.push_str(&format!("{{{n}}}"));
                    // Express 4's optional parameter `:id?`.
                    if chars.peek() == Some(&'?') {
                        chars.next();
                    }
                }
            }
            '*' => converted.push_str(&format!("{{...{}}}", name(&mut chars))),
            '\\' => converted.extend(chars.next()),
            c => converted.push(c),
        }
    }
    let segments: Vec<&str> = converted.split('/').filter(|s| !s.is_empty()).collect();
    format!("/{}", segments.join("/"))
}
