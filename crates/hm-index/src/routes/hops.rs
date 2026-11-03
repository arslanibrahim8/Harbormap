//! Client calls and the edges through data hooks, forms and dependency injection (Learn
//! spec §6.3).
//!
//! Every URL a client reads becomes a `ClientCall`, matched or not, so that routes found
//! later (another language, another app) can still be matched without indexing the client
//! again. A call that matches exactly one most-specific route also becomes an `http_calls`
//! edge. Data hooks (React Query, SWR) and server-action forms add the `calls` edges that a
//! plain call graph cannot see: the framework, not the code, runs the function.
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
use crate::facts::{AliasFact, ArgFact, FileFacts, FormFact, Owner};
use crate::ts::resolve::{Project, resolve_callee};
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
        for c in &facts.web.url_calls {
            calls.insert((
                ClientCall {
                    file: rel.clone(),
                    src_id: source(&mut ids, rel, &c.owner, c.line),
                    line: c.line,
                    method: c.method.clone().unwrap_or_else(|| "ANY".to_string()),
                    url: c.url.clone(),
                    rule: format!("ts.client.{}", c.client),
                },
                Target::ByUrl,
            ));
        }
        for form in &facts.web.forms {
            let method = form.method.clone().unwrap_or_else(|| "GET".to_string());
            let call = |url: String, rule: &str, src_id: String| ClientCall {
                file: rel.clone(),
                src_id,
                line: form.line,
                method: method.clone(),
                url,
                rule: rule.to_string(),
            };
            match &form.action {
                Some(ArgFact::Str(u)) if !u.is_empty() => {
                    let src_id = source(&mut ids, rel, &form.owner, form.line);
                    let url = normalize_client_url(u);
                    calls.insert((call(url, "ts.client.form", src_id), Target::ByUrl));
                }
                // A form that submits to the page it is in: the page's own pattern.
                Some(ArgFact::Str(_)) | None if is_self_form(form) => {
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
                }
                Some(ArgFact::Name(name)) => {
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
                _ => {}
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

    let mut found: HashMap<(&str, &str), Option<&Route>> = HashMap::new();
    for (call, target) in &calls {
        let dst = match target {
            Target::Page(id) => routes
                .iter()
                .find(|r| r.id == *id)
                .filter(|r| methods_agree(&r.method, &call.method)),
            Target::ByUrl => *found
                .entry((call.method.as_str(), call.url.as_str()))
                .or_insert_with(|| match_route(&routes, &call.method, &call.url)),
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
    /// Matched by method and URL against every route.
    ByUrl,
    /// A form without action: the page it is in, by id.
    Page(String),
}

/// The symbol that holds code at `line` inside `owner`, or the file itself.
fn source(ids: &mut Ids, rel: &str, owner: &Owner, line: u32) -> String {
    ids.holder(rel, owner.as_ref(), line)
        .unwrap_or_else(|| file_id(rel))
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

/// The route that a client call to `url` with `method` reaches (Learn spec §6.3), among the
/// `http` and `page` routes of `routes` of every root: a client may call another app of the
/// repository. A route fits when its method is the same (or `ANY` on either side) and its
/// pattern takes the URL; of the routes that fit, the one with the most literal segments
/// wins. `None` for no match and for a tie (no guessing).
pub fn match_route<'r>(routes: &'r [Route], method: &str, url: &str) -> Option<&'r Route> {
    // A path from the root of this site: not `api/x`, not `{}/x` (a base URL held in a
    // variable), not `//host/x` (protocol-relative, another host).
    if !url.starts_with('/') || url.starts_with("//") {
        return None;
    }
    let parts: Vec<&str> = segments(url).collect();
    let mut best: Option<(&Route, usize)> = None;
    let mut tied = false;
    for route in routes
        .iter()
        .filter(|r| r.entry_kind == "http" || r.entry_kind == "page")
        .filter(|r| methods_agree(&r.method, method))
    {
        let segs: Vec<Seg> = segments(&route.pattern).map(seg).collect();
        if !fits(&segs, &parts) {
            continue;
        }
        let literals = segs.iter().filter(|s| matches!(s, Seg::Literal(_))).count();
        match best {
            Some((_, most)) if most > literals => {}
            Some((_, most)) if most == literals => tied = true,
            _ => {
                best = Some((route, literals));
                tied = false;
            }
        }
    }
    best.filter(|_| !tied).map(|(route, _)| route)
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
