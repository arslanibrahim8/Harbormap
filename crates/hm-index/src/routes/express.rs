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
//! `app.route('/x').get(…)` chains, regular-expression and array paths, routers exported
//! with `module.exports` or through a re-export, and receivers bound in another file are
//! documented limits.

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
    let kept: BTreeSet<&str> = out.routes.iter().map(|r| r.id.as_str()).collect();
    let mut edges = BTreeSet::new();
    for e in emitted
        .iter()
        .filter(|e| kept.contains(e.route.id.as_str()))
    {
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
                for (i, arg) in c.args.iter().enumerate().skip(first) {
                    let ArgFact::Name(name) = arg else {
                        continue;
                    };
                    let key = (c.line, i);
                    if let Some(child) = router_node(project, &index, facts, rel, &c.owner, name) {
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
            if c.args.len() < 2 {
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

/// Every declared route through every chain of mounts from an app, and the notes.
fn emit(inputs: &Inputs, graph: &Graph) -> (Vec<Emitted>, Vec<String>) {
    let mounted: BTreeSet<usize> = graph.mounts.iter().map(|m| m.child).collect();
    let mut instances = Vec::new();
    let mut reached = BTreeSet::new();
    for root in (0..graph.nodes.len()).filter(|n| !mounted.contains(n)) {
        descend(
            graph,
            root,
            &mut Vec::new(),
            &mut vec![root],
            &mut instances,
            &mut reached,
        );
    }
    // A router reached only through a cycle or through more than five mounts keeps its
    // own paths, as if it were not mounted.
    let mut notes = Vec::new();
    for node in 0..graph.nodes.len() {
        if reached.contains(&node) || !graph.declared.iter().any(|d| d.node == node) {
            continue;
        }
        let (file, _, name) = &graph.nodes[node];
        notes.push(format!(
            "{file}:{}: router `{name}` is mounted only through a cycle or more than \
             {MAX_MOUNTS} mounts deep; its paths are kept without a prefix",
            graph.bound[node].0
        ));
        descend(
            graph,
            node,
            &mut Vec::new(),
            &mut vec![node],
            &mut instances,
            &mut reached,
        );
    }

    let emitted = instances
        .into_iter()
        .map(|(root, instance)| {
            let d = &graph.declared[instance.declared];
            let paths: Vec<&str> = instance.hops.iter().map(|h| h.path.as_str()).collect();
            let pattern = path_pattern(&paths);
            let app_root = inputs
                .manifests
                .root_of(&d.file)
                .map_or("", |m| m.root.as_str());
            let detail = (!graph.bound[root].1).then(|| r#"{"mounted":false}"#.to_string());
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
/// from the root down to `node`.
fn descend(
    graph: &Graph,
    node: usize,
    chain: &mut Vec<Hop>,
    visiting: &mut Vec<usize>,
    out: &mut Vec<(usize, Instance)>,
    reached: &mut BTreeSet<usize>,
) {
    reached.insert(node);
    let root = visiting[0];
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
        out.push((root, Instance { declared: i, hops }));
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
        descend(graph, m.child, chain, visiting, out, reached);
        visiting.pop();
        chain.pop();
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
