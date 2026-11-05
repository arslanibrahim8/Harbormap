//! Client calls and the edges through data hooks, forms and dependency injection (Learn
//! spec §6.3).
//!
//! Every URL a client reads becomes a `ClientCall`, matched or not, so that routes found
//! later (another language, another app) can still be matched without indexing the client
//! again. A call that matches exactly one most-specific route also becomes an `http_calls`
//! edge. Data hooks (React Query, SWR) and server-action forms add the `calls` edges that a
//! plain call graph cannot see: the framework, not the code, runs the function.
//!
//! Matching is segment by segment. Of the routes that fit, the one with the most literal
//! segments wins (not Next.js's left-to-right precedence of static over dynamic over
//! catch-all), and two routes with the same count give no edge: no guessing. A call looks
//! first among the routes of its own app (the manifest root that governs its file); only
//! when none of them fits does it look among the routes of every root. Programmatic
//! clients (`fetch`, axios, request helpers, SWR) reach `http` routes only; a form reaches
//! pages and `http` routes. An axios call on an instance from `axios.create({ baseURL })`,
//! bound at module level in the same file or imported from the file that binds and exports
//! it, has the `baseURL` joined before a relative URL.
//!
//! Rules checked against the official docs while building:
//! - `useMutation` and `useQuery` take `mutationFn` / `queryFn` and the callbacks
//!   `onSuccess`, `onError`, `onSettled`; `useSWR(key, fetcher)` takes the fetcher second.
//!   https://tanstack.com/query/latest/docs/framework/react/reference/useMutation
//!   https://swr.vercel.app/docs/api
//! - A function given as `<form action>` (or `formAction` on a button or input) is called
//!   by React on submit; `useActionState(fn, initial)` returns `[state, formAction]` and
//!   `fn.bind(null, arg)` makes a function with `arg` fixed. A server action is a function
//!   marked `'use server'`.
//!   https://react.dev/reference/react/useActionState
//!   https://react.dev/reference/rsc/use-server

use std::collections::{BTreeSet, HashMap, HashSet};

use hm_core::ids::file_id;

use super::{ClientCall, Derived, Ids, InferredEdge, Inputs, Route};
use crate::facts::{AliasFact, ArgFact, FileFacts, FormFact, ImportKind, Owner, UrlCallFact};
use crate::ts::resolve::{ModuleTarget, Project, resolve_callee};
use crate::ts::web::normalize_client_url;

/// How many aliases a name is followed through (`bind`, `useActionState`, `bind`, …).
const MAX_ALIAS_DEPTH: usize = 5;

/// Client calls, `http_calls` matches and mediated `calls` edges.
pub(crate) fn derive(inputs: &Inputs, out: &mut Derived) {
    let project = inputs.project;
    let routes = std::mem::take(&mut out.routes);
    let mut ids = Ids::new(project);
    let handlers: HashSet<&str> = routes
        .iter()
        .filter(|r| r.entry_kind == "server_action")
        .filter_map(|r| r.handler_id.as_deref())
        .collect();

    // The calls and where each one looks for its route.
    let mut calls: BTreeSet<(ClientCall, Target)> = BTreeSet::new();
    let mut edges: BTreeSet<InferredEdge> = BTreeSet::new();
    for (rel, facts) in project.facts {
        for c in facts.web.url_calls.iter().filter(|c| !c.url.is_empty()) {
            calls.insert((
                ClientCall {
                    file: rel.clone(),
                    src_id: source(&mut ids, rel, &c.owner, c.line),
                    line: c.line,
                    method: c.method.clone().unwrap_or_else(|| "ANY".to_string()),
                    url: with_base_url(project, rel, facts, c),
                    rule: format!("ts.client.{}", c.client),
                },
                Target::ByUrl { pages: false },
            ));
        }
        for form in &facts.web.forms {
            // A `<form>` without `method` is `GET`; a button or input without `formMethod`
            // takes the method of the form around it, which is not known here.
            let method = match (&form.method, form.element.as_str()) {
                (Some(m), _) => m.clone(),
                (None, "form") => "GET".to_string(),
                (None, _) => "ANY".to_string(),
            };
            let call = |url: String, rule: &str, src_id: String| ClientCall {
                file: rel.clone(),
                src_id,
                line: form.line,
                method: method.clone(),
                url,
                rule: rule.to_string(),
            };
            // `action=""`, `action="#"` or `action="?page=2"` has no path: like no action,
            // the current page, never `/`.
            let url = match &form.action {
                Some(ArgFact::Str(u)) => Some(normalize_client_url(u)),
                _ => None,
            };
            let to_self = url.as_ref().map_or(form.action.is_none(), String::is_empty);
            if let Some(url) = url.filter(|u| !u.is_empty()) {
                let src_id = source(&mut ids, rel, &form.owner, form.line);
                calls.insert((
                    call(url, "ts.client.form", src_id),
                    Target::ByUrl { pages: true },
                ));
            } else if to_self && is_self_form(form) {
                // A form that submits to the page it is in: the page's own pattern.
                let mut pages = routes
                    .iter()
                    .filter(|r| r.entry_kind == "page" && r.file == *rel);
                if let (Some(page), None) = (pages.next(), pages.next()) {
                    let src_id = source(&mut ids, rel, &form.owner, form.line);
                    calls.insert((
                        call(page.pattern.clone(), "ts.client.form_self", src_id),
                        Target::Page(page.id.clone()),
                    ));
                }
            } else if let Some(ArgFact::Name(name)) = &form.action {
                let found =
                    action_target(project, &mut ids, &handlers, rel, facts, &form.owner, name);
                if let Some(dst) = found {
                    let src_id = source(&mut ids, rel, &form.owner, form.line);
                    edges.insert(calls_edge(
                        rel,
                        src_id,
                        dst,
                        form.line,
                        "ts.next.action.form",
                    ));
                }
            }
        }
        for h in &facts.web.hooks {
            let ArgFact::Name(name) = &h.target else {
                continue;
            };
            let rule = match h.key.as_str() {
                "mutationFn" | "queryFn" => "ts.react_query.fn",
                "onSuccess" | "onSettled" | "onError" => "ts.react_query.back",
                "fetcher" => "ts.swr.fetcher",
                _ => continue,
            };
            if let Some(dst) = symbol_named(project, &mut ids, rel, name) {
                let src_id = source(&mut ids, rel, &h.owner, h.line);
                edges.insert(calls_edge(rel, src_id, dst, h.line, rule));
            }
        }
        // `useActionState(action, …)` runs the action; the form that uses its
        // `formAction` is a separate edge.
        for a in facts
            .web
            .aliases
            .iter()
            .filter(|a| a.via == "useActionState")
        {
            let found = action_target(
                project, &mut ids, &handlers, rel, facts, &a.owner, &a.target,
            );
            if let Some(dst) = found {
                let src_id = source(&mut ids, rel, &a.owner, a.line);
                edges.insert(calls_edge(rel, src_id, dst, a.line, "ts.next.action.state"));
            }
        }
    }

    let matcher = Matcher::new(&routes);
    let mut found: HashMap<(&str, &str, &str, bool), Option<&Route>> = HashMap::new();
    for (call, target) in &calls {
        let dst = match target {
            Target::Page(id) => routes
                .iter()
                .find(|r| r.id == *id)
                .filter(|r| methods_agree(&r.method, &call.method)),
            Target::ByUrl { pages } => {
                let root = inputs
                    .manifests
                    .root_of(&call.file)
                    .map_or("", |m| m.root.as_str());
                *found
                    .entry((root, call.method.as_str(), call.url.as_str(), *pages))
                    .or_insert_with(|| matcher.find(root, &call.method, &call.url, *pages))
            }
        };
        if let Some(route) = dst {
            edges.insert(InferredEdge {
                kind: "http_calls".to_string(),
                src_id: call.src_id.clone(),
                dst_id: route.id.clone(),
                file: call.file.clone(),
                line: call.line,
                rule: call.rule.clone(),
            });
        }
    }

    out.client_calls.extend(calls.into_iter().map(|(c, _)| c));
    out.edges.extend(edges);
    out.routes = routes;
}

/// Where a client call looks for its route.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    /// Matched by method and URL against the `http` routes, and the pages too when `pages`
    /// (a form).
    ByUrl { pages: bool },
    /// A form without action: the page it is in, by id.
    Page(String),
}

/// The symbol that holds code at `line` inside `owner`, or the file itself.
fn source(ids: &mut Ids, rel: &str, owner: &Owner, line: u32) -> String {
    ids.holder(rel, owner.as_ref(), line)
        .unwrap_or_else(|| file_id(rel))
}

/// The URL of an axios call with the `baseURL` of its instance joined before it. A URL
/// that starts with `{}` may be absolute at run time and stays as it is.
fn with_base_url(project: &Project, rel: &str, facts: &FileFacts, call: &UrlCallFact) -> String {
    let base = call
        .receiver
        .as_deref()
        .filter(|_| !call.url.starts_with("{}"))
        .and_then(|name| axios_base(project, rel, facts, name));
    match base {
        Some(base) if !base.is_empty() => {
            let head = base.trim_end_matches('/');
            match call.url.trim_start_matches('/') {
                "" if head.is_empty() => "/".to_string(),
                "" => head.to_string(),
                tail => format!("{head}/{tail}"),
            }
        }
        _ => call.url.clone(),
    }
}

/// The `baseURL` of the axios instance `name` stands for at the top level of `rel`: one
/// bound there, or one imported from the file that binds and exports it.
fn axios_base<'p>(
    project: &'p Project,
    rel: &str,
    facts: &'p FileFacts,
    name: &str,
) -> Option<&'p str> {
    let local = |facts: &'p FileFacts, name: &str| {
        facts
            .web
            .axios_instances
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.base_url.as_str())
    };
    if let Some(base) = local(facts, name) {
        return Some(base);
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
    let target_facts = project.facts.get(&target)?;
    let export = target_facts
        .exports
        .iter()
        .find(|e| !e.star && e.from.is_none() && e.name == imported)?;
    local(target_facts, export.local.as_deref().unwrap_or(imported))
}

/// A `<form>` with no `action` and no `onSubmit`.
fn is_self_form(form: &FormFact) -> bool {
    form.element == "form" && !form.on_submit
}

fn calls_edge(file: &str, src_id: String, dst_id: String, line: u32, rule: &str) -> InferredEdge {
    InferredEdge {
        kind: "calls".to_string(),
        src_id,
        dst_id,
        file: file.to_string(),
        line,
        rule: rule.to_string(),
    }
}

/// The id of the one symbol `name` (`f` or `Class.method`) stands for as seen from the top
/// level of `rel`. A nested local name is no symbol and gives `None`.
fn symbol_named(project: &Project, ids: &mut Ids, rel: &str, name: &str) -> Option<String> {
    let (receiver, callee) = match name.rsplit_once('.') {
        Some((receiver, callee)) => (Some(receiver), callee),
        None => (None, name),
    };
    let (file, index) = resolve_callee(project, rel, receiver, callee)?;
    ids.symbol(&file, index)
}

/// The id of the server action that `name` leads to inside `owner`: through the aliases
/// declared there (`bind`, `useActionState`), then by resolving the last name. `None`
/// unless it is exactly one symbol that is the handler of a server action.
fn action_target(
    project: &Project,
    ids: &mut Ids,
    handlers: &HashSet<&str>,
    rel: &str,
    facts: &FileFacts,
    owner: &Owner,
    name: &str,
) -> Option<String> {
    let mut current = name;
    let mut seen: Vec<&str> = Vec::new();
    for _ in 0..=MAX_ALIAS_DEPTH {
        let aliases: Vec<&AliasFact> = facts
            .web
            .aliases
            .iter()
            .filter(|a| a.owner == *owner && a.local == current)
            .collect();
        match aliases[..] {
            [] => {
                let dst = symbol_named(project, ids, rel, current)?;
                return handlers.contains(dst.as_str()).then_some(dst);
            }
            [alias] if !seen.contains(&current) => {
                seen.push(current);
                current = &alias.target;
            }
            // Declared twice, or a cycle: no way to tell which one is meant.
            _ => return None,
        }
    }
    None
}

/// Whether a call with `call` as method can reach a route with `route` as method.
fn methods_agree(route: &str, call: &str) -> bool {
    route == call || route == "ANY" || call == "ANY"
}

/// One part of a route pattern.
enum Seg {
    Literal(String),
    /// `{id}`: exactly one URL segment.
    Param,
    /// `{...rest}` (at least one segment) or `{...rest?}` (any number): the rest of the URL.
    Rest {
        min: usize,
    },
}

/// A route with its pattern read once.
struct Candidate<'r> {
    route: &'r Route,
    segs: Vec<Seg>,
    literals: usize,
}

/// The routes a client call can reach, with each pattern read once per derivation.
struct Matcher<'r> {
    candidates: Vec<Candidate<'r>>,
}

impl<'r> Matcher<'r> {
    /// Over the `http` and `page` routes of `routes` of every root: a client may call another
    /// app of the repository.
    fn new(routes: &'r [Route]) -> Matcher<'r> {
        let candidates = routes
            .iter()
            .filter(|r| r.entry_kind == "http" || r.entry_kind == "page")
            .map(|route| {
                let segs: Vec<Seg> = segments(&route.pattern).map(seg).collect();
                let literals = segs.iter().filter(|s| matches!(s, Seg::Literal(_))).count();
                Candidate {
                    route,
                    segs,
                    literals,
                }
            })
            .collect();
        Matcher { candidates }
    }

    /// The route a call from a file under manifest root `root` reaches: the one with the
    /// most literal segments among the routes of `root` that fit, else among those of every
    /// root; `None` for no match and for a tie (no guessing). Pages count only when `pages`.
    fn find(&self, root: &str, method: &str, url: &str, pages: bool) -> Option<&'r Route> {
        // A path from the root of this site: not `api/x`, not `{}/x` (a base URL held in a
        // variable), not `//host/x` (another host, whatever the scheme).
        if !url.starts_with('/') || url.starts_with("//") {
            return None;
        }
        let parts: Vec<&str> = segments(url).collect();
        let fitting: Vec<&Candidate> = self
            .candidates
            .iter()
            .filter(|c| pages || c.route.entry_kind == "http")
            .filter(|c| methods_agree(&c.route.method, method) && fits(&c.segs, &parts))
            .collect();
        let own: Vec<&Candidate> = fitting
            .iter()
            .copied()
            .filter(|c| c.route.root == root)
            .collect();
        most_specific(if own.is_empty() { &fitting } else { &own })
    }
}

/// The candidate with the most literal segments; `None` when there is none or a tie.
fn most_specific<'r>(fitting: &[&Candidate<'r>]) -> Option<&'r Route> {
    let most = fitting.iter().map(|c| c.literals).max()?;
    let mut top = fitting.iter().filter(|c| c.literals == most);
    match (top.next(), top.next()) {
        (Some(c), None) => Some(c.route),
        _ => None,
    }
}

/// The non-empty `/`-separated parts of a path or pattern.
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty())
}

fn seg(part: &str) -> Seg {
    if part.starts_with("{...") && part.ends_with('}') {
        Seg::Rest {
            min: usize::from(!part.ends_with("?}")),
        }
    } else if part.starts_with('{') && part.ends_with('}') {
        Seg::Param
    } else {
        Seg::Literal(part.to_string())
    }
}

/// Whether the URL parts `url` are what the route `segs` takes. A client part with a
/// `{}` in it is built at run time and fits only a parameter, never a literal.
fn fits(segs: &[Seg], url: &[&str]) -> bool {
    match segs.split_first() {
        None => url.is_empty(),
        Some((Seg::Rest { min }, tail)) => {
            (*min..=url.len()).any(|taken| fits(tail, &url[taken..]))
        }
        Some((one, tail)) => match url.split_first() {
            None => false,
            Some((part, rest)) => {
                let ok = match one {
                    Seg::Literal(l) => !part.contains("{}") && l == part,
                    _ => true,
                };
                ok && fits(tail, rest)
            }
        },
    }
}
