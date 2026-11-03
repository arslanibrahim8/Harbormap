//! Next.js routes, server actions and middleware (Learn spec §6.3). They apply only under
//! a manifest root whose `package.json` declares `next`; a file belongs to the root of its
//! nearest `package.json`.
//!
//! Conventions, checked against the official docs:
//! - <https://nextjs.org/docs/app/getting-started/project-structure> (page and route files,
//!   groups, private folders, slots, intercepting routes);
//! - <https://nextjs.org/docs/app/api-reference/file-conventions/route> (the seven verbs);
//! - <https://nextjs.org/docs/app/api-reference/file-conventions/dynamic-routes>;
//! - <https://nextjs.org/docs/app/api-reference/file-conventions/src-folder> (`src/app` and
//!   `src/pages` are used only when `app` and `pages` are not at the root);
//! - <https://nextjs.org/docs/pages/building-your-application/routing/api-routes>;
//! - <https://nextjs.org/docs/app/api-reference/file-conventions/proxy> (`proxy`, formerly
//!   `middleware`, and its `matcher`).

use std::collections::{BTreeMap, BTreeSet};

use hm_core::ids::file_id;

use super::{Derived, Ids, InferredEdge, Inputs, Route, has_known_under, route};
use crate::facts::SymbolKind;
use crate::ts::resolve::{Project, resolve_export};

/// The extensions of an app-router `page` (the default `pageExtensions`) and of a
/// pages-router file.
const PAGE_EXTS: &[&str] = &["js", "jsx", "ts", "tsx"];
/// The extensions of an app-router `route`.
const ROUTE_EXTS: &[&str] = &["js", "ts"];
/// The verbs a `route` file may export.
const VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
/// Pages-router files at the top of the tree that are not pages.
const PAGES_SPECIAL: &[&str] = &["_app", "_document", "_error"];
/// The middleware file names, newest first, and the function names each may export.
const MIDDLEWARE_FILES: &[&str] = &["proxy", "middleware"];
const MIDDLEWARE_EXTS: &[&str] = &["ts", "js"];

/// One Next.js app: a manifest root that declares `next`, and the parsed files it governs.
struct App<'a> {
    root: &'a str,
    files: BTreeSet<&'a str>,
}

impl App<'_> {
    /// The parsed files under folder `tree`, with the path below it.
    fn under<'s>(&'s self, tree: &str) -> impl Iterator<Item = (&'s str, &'s str)> + 's {
        let prefix = format!("{tree}/");
        self.files
            .iter()
            .filter_map(move |rel| rel.strip_prefix(prefix.as_str()).map(|rest| (*rel, rest)))
    }
}

/// The `app` and `pages` trees of the app at `root`: `R/app` and `R/pages` when either
/// holds a known file, else `R/src/app` and `R/src/pages`. "`src/app` or `src/pages` will
/// be ignored if `app` or `pages` are present in the root directory":
/// <https://nextjs.org/docs/app/api-reference/file-conventions/src-folder>.
fn trees(project: &Project, root: &str) -> (Option<String>, Option<String>) {
    let pick = |base: &str| {
        let present = |dir: &str| {
            let tree = join(base, dir);
            has_known_under(project, &tree).then_some(tree)
        };
        (present("app"), present("pages"))
    };
    match pick(root) {
        (None, None) => pick(&join(root, "src")),
        found => found,
    }
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() {
        rel.to_string()
    } else {
        format!("{dir}/{rel}")
    }
}

/// Every Next.js app of the tree, by root.
fn apps<'a>(inputs: &Inputs<'a>) -> Vec<App<'a>> {
    let mut by_root: BTreeMap<&str, App> = BTreeMap::new();
    for m in &inputs.manifests.npm {
        if m.deps.iter().any(|d| d.name == "next") {
            by_root.insert(
                m.root.as_str(),
                App {
                    root: m.root.as_str(),
                    files: BTreeSet::new(),
                },
            );
        }
    }
    for rel in inputs.project.facts.keys() {
        let Some(m) = inputs.manifests.root_of(rel) else {
            continue;
        };
        if let Some(app) = by_root.get_mut(m.root.as_str()) {
            app.files.insert(rel.as_str());
        }
    }
    by_root.into_values().collect()
}

/// Pages, route handlers, server actions and the middleware of every Next.js app.
pub(crate) fn routes(inputs: &Inputs, out: &mut Derived) {
    let mut ids = Ids::new(inputs.project);
    for app in apps(inputs) {
        let (app_tree, pages_tree) = trees(inputs.project, app.root);
        let mut slot_routes = Vec::new();
        if let Some(tree) = &app_tree {
            for (rel, rest) in app.under(tree) {
                let (in_slot, found) = app_routes(inputs.project, &mut ids, app.root, rel, rest);
                if in_slot {
                    slot_routes.extend(found);
                } else {
                    out.routes.extend(found);
                }
            }
        }
        if let Some(tree) = &pages_tree {
            for (rel, rest) in app.under(tree) {
                if let Some(r) = pages_route(inputs.project, &mut ids, app.root, rel, rest) {
                    out.routes.push(r);
                }
            }
        }
        // A slot's page renders beside the real page of its URL, not instead of it
        // (<https://nextjs.org/docs/app/api-reference/file-conventions/parallel-routes>):
        // it is a route only where no other page gives that URL, and that is no conflict.
        let mut taken: BTreeSet<String> = out
            .routes
            .iter()
            .filter(|r| r.root == app.root)
            .map(|r| r.id.clone())
            .collect();
        slot_routes.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
        for r in slot_routes {
            if taken.insert(r.id.clone()) {
                out.routes.push(r);
            }
        }
        for rel in &app.files {
            server_actions(inputs.project, &mut ids, app.root, rel, out);
        }
        let in_src = [&app_tree, &pages_tree]
            .into_iter()
            .flatten()
            .any(|tree| tree.starts_with(&join(app.root, "src/")));
        if let Some(r) = middleware(inputs.project, &mut ids, &app, in_src) {
            out.routes.push(r);
        }
    }
}

/// The handler symbol that `rel` exports as `name` and the line of the route: the line of
/// the export (`export { remove as DELETE }`), else the symbol's first line when `rel`
/// declares it (a CommonJS export), else 1.
fn exported(project: &Project, ids: &mut Ids, rel: &str, name: &str) -> (Option<String>, u32) {
    let export_line = project
        .facts
        .get(rel)
        .and_then(|f| f.exports.iter().find(|e| !e.star && e.name == name))
        .map(|e| e.line);
    let Some((file, index)) = resolve_export(project, rel, name) else {
        return (None, export_line.unwrap_or(1));
    };
    let line = match (export_line, project.facts.get(&file)) {
        (Some(line), _) => line,
        (None, Some(f)) if file == rel => f.symbols[index].l0,
        _ => 1,
    };
    (ids.symbol(&file, index), line)
}

/// What one app-router folder name does to the URL.
enum Segment {
    /// Adds this segment.
    Push(String),
    /// Leaves the URL as it is: a `(group)` or an `@slot`.
    Drop,
    /// Takes the whole subtree out of routing: a `_private` folder or an intercepting
    /// `(.)x`, `(..)x`, `(...)x` folder (a limit: the route it intercepts is declared
    /// elsewhere).
    Skip,
}

fn app_segment(folder: &str) -> Segment {
    if let Some(rest) = folder.strip_prefix("%5F") {
        // The URL-encoded underscore makes a segment that starts with `_`.
        return Segment::Push(format!("_{rest}"));
    }
    if folder.starts_with('_') || folder.starts_with("(.") {
        return Segment::Skip;
    }
    if (folder.starts_with('(') && folder.ends_with(')')) || folder.starts_with('@') {
        return Segment::Drop;
    }
    Segment::Push(dynamic(folder))
}

/// `[id]` → `{id}`, `[...s]` → `{...s}`, `[[...s]]` → `{...s?}`; anything else as is.
fn dynamic(name: &str) -> String {
    if let Some(inner) = name
        .strip_prefix("[[...")
        .and_then(|n| n.strip_suffix("]]"))
    {
        return format!("{{...{inner}?}}");
    }
    if let Some(inner) = name.strip_prefix("[...").and_then(|n| n.strip_suffix(']')) {
        return format!("{{...{inner}}}");
    }
    if let Some(inner) = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')) {
        return format!("{{{inner}}}");
    }
    name.to_string()
}

fn pattern_of(segments: &[String]) -> String {
    format!("/{}", segments.join("/"))
}

/// `(stem, extension)` of a file name; `None` for a declaration file.
fn split_name(name: &str) -> Option<(&str, &str)> {
    if name.ends_with(".d.ts") {
        return None;
    }
    name.rsplit_once('.')
}

/// The routes of app-router file `rel`, `rest` being its path below the `app` folder: a
/// `page` route for `page.*`, one `http` route per verb for `route.*`. The flag is true
/// when the file sits in an `@slot` folder.
fn app_routes(
    project: &Project,
    ids: &mut Ids,
    root: &str,
    rel: &str,
    rest: &str,
) -> (bool, Vec<Route>) {
    let mut routes = Vec::new();
    let (folders, name) = rest.rsplit_once('/').unwrap_or(("", rest));
    let Some((stem, ext)) = split_name(name) else {
        return (false, routes);
    };
    let is_page = stem == "page" && PAGE_EXTS.contains(&ext);
    let is_route = stem == "route" && ROUTE_EXTS.contains(&ext);
    if !is_page && !is_route {
        return (false, routes);
    }
    let mut segments = Vec::new();
    let mut in_slot = false;
    for folder in folders.split('/').filter(|f| !f.is_empty()) {
        match app_segment(folder) {
            Segment::Push(s) => segments.push(s),
            Segment::Drop => in_slot |= folder.starts_with('@'),
            Segment::Skip => return (false, routes),
        }
    }
    let pattern = pattern_of(&segments);

    if is_page {
        let (handler_id, line) = exported(project, ids, rel, "default");
        routes.push(Route {
            handler_id,
            ..route(root, "page", "GET", &pattern, rel, line, "ts.next.app.page")
        });
        return (in_slot, routes);
    }
    let Some(facts) = project.facts.get(rel) else {
        return (in_slot, routes);
    };
    for verb in VERBS {
        let declared = facts.exports.iter().any(|e| !e.star && e.name == *verb);
        let (handler_id, line) = exported(project, ids, rel, verb);
        if !declared && handler_id.is_none() {
            continue;
        }
        routes.push(Route {
            handler_id,
            ..route(root, "http", verb, &pattern, rel, line, "ts.next.app.route")
        });
    }
    (in_slot, routes)
}

/// The route of pages-router file `rel`, `rest` being its path below the `pages` folder:
/// an `http` route that takes every method under `api/`, a page elsewhere.
fn pages_route(
    project: &Project,
    ids: &mut Ids,
    root: &str,
    rel: &str,
    rest: &str,
) -> Option<Route> {
    let (folders, name) = rest.rsplit_once('/').unwrap_or(("", rest));
    let (stem, ext) = split_name(name)?;
    if !PAGE_EXTS.contains(&ext) || (folders.is_empty() && PAGES_SPECIAL.contains(&stem)) {
        return None;
    }
    let mut segments: Vec<String> = folders
        .split('/')
        .filter(|f| !f.is_empty())
        .map(dynamic)
        .collect();
    let api = segments.first().is_some_and(|s| s == "api");
    if stem != "index" {
        segments.push(dynamic(stem));
    }
    let pattern = pattern_of(&segments);
    let (handler_id, line) = exported(project, ids, rel, "default");
    let (kind, method, rule) = if api {
        ("http", "ANY", "ts.next.pages.api")
    } else {
        ("page", "GET", "ts.next.pages.page")
    };
    Some(Route {
        handler_id,
        ..route(root, kind, method, &pattern, rel, line, rule)
    })
}

/// The server actions of `rel`: with a file-level `use server`, every exported function;
/// with a function-level one, that function, named after the symbol that holds it. The
/// second anonymous one in the same symbol is `Owner›anonymous#2`, and so on, in source
/// order.
/// See <https://nextjs.org/docs/app/api-reference/directives/use-server>.
fn server_actions(project: &Project, ids: &mut Ids, root: &str, rel: &str, out: &mut Derived) {
    let Some(facts) = project.facts.get(rel) else {
        return;
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let use_server = |text: &str| text == "use server";

    if facts
        .web
        .directives
        .iter()
        .any(|d| d.function.is_none() && use_server(&d.text))
    {
        for e in facts.exports.iter().filter(|e| !e.star && e.from.is_none()) {
            let Some((file, index)) = resolve_export(project, rel, &e.name) else {
                continue;
            };
            let is_function = project
                .facts
                .get(&file)
                .is_some_and(|f| f.symbols[index].kind == SymbolKind::Function);
            let pattern = format!("{rel}#{}", e.name);
            if !is_function || !seen.insert(pattern.clone()) {
                continue;
            }
            out.routes.push(Route {
                handler_id: ids.symbol(&file, index),
                ..route(
                    root,
                    "server_action",
                    "ACTION",
                    &pattern,
                    rel,
                    e.line,
                    "ts.next.action.file",
                )
            });
        }
    }

    for d in &facts.web.directives {
        let Some(function) = d.function.as_deref().filter(|_| use_server(&d.text)) else {
            continue;
        };
        let anonymous = function.is_empty();
        let function = if anonymous { "anonymous" } else { function };
        let mut pattern = match &d.owner {
            None => format!("{rel}#{function}"),
            Some((class, name)) => {
                let owner = if class.is_empty() {
                    name.clone()
                } else {
                    format!("{class}.{name}")
                };
                if *name == function {
                    format!("{rel}#{owner}")
                } else {
                    format!("{rel}#{owner}›{function}")
                }
            }
        };
        if anonymous && seen.contains(&pattern) {
            let base = pattern.clone();
            pattern = (2..)
                .map(|n| format!("{base}#{n}"))
                .find(|p| !seen.contains(p))
                .unwrap_or(base);
        }
        if !seen.insert(pattern.clone()) {
            continue;
        }
        out.routes.push(Route {
            handler_id: ids.holder(rel, d.owner.as_ref(), d.line),
            ..route(
                root,
                "server_action",
                "ACTION",
                &pattern,
                rel,
                d.line,
                "ts.next.action.fn",
            )
        });
    }
}

/// The middleware (`proxy`) of `app`: at its root or in `src/`, the one beside the
/// `app`/`pages` tree first. Its pattern is the first matcher, or `/*` without one.
fn middleware(project: &Project, ids: &mut Ids, app: &App, in_src: bool) -> Option<Route> {
    let src = join(app.root, "src");
    let dirs = if in_src {
        [src.as_str(), app.root]
    } else {
        [app.root, src.as_str()]
    };
    let rel = dirs
        .iter()
        .flat_map(|dir| {
            MIDDLEWARE_FILES.iter().flat_map(move |name| {
                MIDDLEWARE_EXTS
                    .iter()
                    .map(move |ext| join(dir, &format!("{name}.{ext}")))
            })
        })
        .find(|rel| app.files.contains(rel.as_str()))?;
    let facts = project.facts.get(&rel)?;

    let values: Vec<String> = facts
        .web
        .matcher
        .as_ref()
        .map(|m| m.values.clone())
        .unwrap_or_default();
    // Without a matcher the middleware runs for every request; a matcher whose values
    // could not be read (a variable) is as unknown as a regex.
    let evaluated = match &facts.web.matcher {
        None => true,
        Some(_) => !values.is_empty() && values.iter().all(|v| parse_matcher(v).is_some()),
    };

    let mut handler = None;
    for name in MIDDLEWARE_FILES.iter().chain(&["default"]) {
        let (id, line) = exported(project, ids, &rel, name);
        if id.is_some() {
            handler = Some((id, line));
            break;
        }
    }
    let (handler_id, line) = match handler {
        Some((id, line)) => (id, line),
        None => {
            let default_line = facts
                .exports
                .iter()
                .find(|e| !e.star && e.name == "default")
                .map(|e| e.line);
            (Some(file_id(&rel)), default_line.unwrap_or(1))
        }
    };

    let pattern = values.first().map_or("/*", String::as_str).to_string();
    let detail = serde_json::json!({ "matcher": values, "evaluated": evaluated });
    Some(Route {
        handler_id,
        conf: if evaluated { "sure" } else { "probably" }.to_string(),
        detail: Some(detail.to_string()),
        ..route(
            app.root,
            "middleware",
            "ANY",
            &pattern,
            &rel,
            line,
            "ts.next.middleware",
        )
    })
}

/// A `calls` edge from every page and route handler that a middleware's matcher covers
/// to the middleware's handler: what runs on the way in.
pub(crate) fn middleware_edges(_inputs: &Inputs, out: &mut Derived) {
    let mut edges = Vec::new();
    for mw in out
        .routes
        .iter()
        .filter(|r| r.entry_kind == "middleware" && r.rule == "ts.next.middleware")
    {
        let Some(handler) = &mw.handler_id else {
            continue;
        };
        let detail: serde_json::Value = mw
            .detail
            .as_deref()
            .and_then(|d| serde_json::from_str(d).ok())
            .unwrap_or_default();
        let evaluated = detail["evaluated"].as_bool().unwrap_or(false);
        let matchers: Vec<Vec<Part>> = detail["matcher"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().and_then(parse_matcher))
            .collect();
        for r in out.routes.iter().filter(|r| {
            r.root == mw.root
                && r.rule.starts_with("ts.next.")
                && matches!(r.entry_kind.as_str(), "page" | "http")
        }) {
            let segments = route_segments(&r.pattern);
            let covered = !evaluated
                || matchers.is_empty()
                || matchers.iter().any(|m| overlaps(m, &segments));
            if covered {
                edges.push(InferredEdge {
                    kind: "calls".to_string(),
                    src_id: r.id.clone(),
                    dst_id: handler.clone(),
                    file: mw.file.clone(),
                    line: mw.line,
                    rule: "ts.next.middleware".to_string(),
                });
            }
        }
    }
    out.edges.extend(edges);
}

/// How many path segments a matcher parameter takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    One,
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
}

/// One segment of a matcher that can be evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Param(Repeat),
}

/// The segments of a matcher made only of literal segments and `:name`, `:name?`,
/// `:name*`, `:name+` parameters; `None` for anything else (a regex group, a lookahead),
/// which is not evaluated.
fn parse_matcher(source: &str) -> Option<Vec<Part>> {
    let path = source.strip_prefix('/')?;
    let mut parts = Vec::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if let Some(param) = segment.strip_prefix(':') {
            let (name, repeat) = match param.chars().last() {
                Some('?') => (&param[..param.len() - 1], Repeat::ZeroOrOne),
                Some('*') => (&param[..param.len() - 1], Repeat::ZeroOrMore),
                Some('+') => (&param[..param.len() - 1], Repeat::OneOrMore),
                _ => (param, Repeat::One),
            };
            let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
            if name.is_empty() || !name.chars().all(word) {
                return None;
            }
            parts.push(Part::Param(repeat));
        } else if segment.contains(|c: char| "()[]{}*+?:\\|^$".contains(c)) {
            return None;
        } else {
            parts.push(Part::Literal(segment.to_string()));
        }
    }
    Some(parts)
}

/// One segment of a route pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Literal(String),
    /// `{name}`: any one segment.
    Param,
    /// `{...name}` (one or more segments) or `{...name?}` (any number).
    CatchAll {
        optional: bool,
    },
}

fn route_segments(pattern: &str) -> Vec<Seg> {
    pattern
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with("{...") {
                Seg::CatchAll {
                    optional: s.ends_with("?}"),
                }
            } else if s.starts_with('{') {
                Seg::Param
            } else {
                Seg::Literal(s.to_string())
            }
        })
        .collect()
}

/// One position of a matcher or a route pattern, as a path language: one segment equal to
/// a text, any one segment, at most one segment, or any number of segments.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Literal(String),
    One,
    Optional,
    Many,
}

fn matcher_steps(matcher: &[Part]) -> Vec<Step> {
    let mut out = Vec::new();
    for part in matcher {
        match part {
            Part::Literal(text) => out.push(Step::Literal(text.clone())),
            Part::Param(Repeat::One) => out.push(Step::One),
            Part::Param(Repeat::ZeroOrOne) => out.push(Step::Optional),
            Part::Param(Repeat::ZeroOrMore) => out.push(Step::Many),
            Part::Param(Repeat::OneOrMore) => out.extend([Step::One, Step::Many]),
        }
    }
    out
}

fn route_steps(route: &[Seg]) -> Vec<Step> {
    let mut out = Vec::new();
    for seg in route {
        match seg {
            Seg::Literal(text) => out.push(Step::Literal(text.clone())),
            Seg::Param => out.push(Step::One),
            Seg::CatchAll { optional: true } => out.push(Step::Many),
            Seg::CatchAll { optional: false } => out.extend([Step::One, Step::Many]),
        }
    }
    out
}

/// True when some URL of the route matches the matcher. A matcher matches the whole path,
/// as the docs say: `/about/:path` matches `/about/a` but not `/about/a/c`, so `/about`
/// does not cover `/about/team` and `/` covers only `/`. A literal or `:p` takes exactly
/// one segment, `:p?` zero or one, `:p*` zero or more, `:p+` one or more; route segments
/// left over are covered only by a trailing `:p*`/`:p+`, and a route's catch-all takes any
/// segments the matcher still has.
fn overlaps(matcher: &[Part], route: &[Seg]) -> bool {
    intersect(&matcher_steps(matcher), &route_steps(route))
}

/// True when the two path languages share a path. Each branch shortens one side, so it
/// ends.
fn intersect(a: &[Step], b: &[Step]) -> bool {
    match (a.split_first(), b.split_first()) {
        (None, None) => true,
        // `Many` takes no more segments, or the first segment of the other side.
        (Some((Step::Many, rest)), _) => {
            intersect(rest, b) || (!b.is_empty() && intersect(a, &b[1..]))
        }
        (_, Some((Step::Many, rest))) => {
            intersect(a, rest) || (!a.is_empty() && intersect(&a[1..], b))
        }
        (Some((Step::Optional, rest)), _) => {
            intersect(rest, b) || intersect(&[&[Step::One], rest].concat(), b)
        }
        (_, Some((Step::Optional, rest))) => {
            intersect(a, rest) || intersect(a, &[&[Step::One], rest].concat())
        }
        (Some((x, a_rest)), Some((y, b_rest))) => {
            let agree = match (x, y) {
                (Step::Literal(l), Step::Literal(r)) => l == r,
                _ => true,
            };
            agree && intersect(a_rest, b_rest)
        }
        // One side ends while the other still needs a segment.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(matcher: &str, pattern: &str) -> bool {
        let m = parse_matcher(matcher).expect("an evaluable matcher");
        overlaps(&m, &route_segments(pattern))
    }

    #[test]
    fn folder_names_become_segments() {
        assert_eq!(dynamic("[id]"), "{id}");
        assert_eq!(dynamic("[...slug]"), "{...slug}");
        assert_eq!(dynamic("[[...slug]]"), "{...slug?}");
        assert_eq!(dynamic("orders"), "orders");
        assert!(matches!(app_segment("(shop)"), Segment::Drop));
        assert!(matches!(app_segment("@modal"), Segment::Drop));
        assert!(matches!(app_segment("_lib"), Segment::Skip));
        assert!(matches!(app_segment("(..)(..)photo"), Segment::Skip));
        assert!(matches!(app_segment("(...)photo"), Segment::Skip));
        assert!(matches!(app_segment("%5Fdocs"), Segment::Push(s) if s == "_docs"));
    }

    #[test]
    fn literal_and_param_matchers_are_evaluated() {
        // https://nextjs.org/docs/app/api-reference/file-conventions/proxy (matcher)
        assert!(parse_matcher("/about/:path*").is_some());
        assert!(parse_matcher("/").is_some_and(|m| m.is_empty()));
        assert!(parse_matcher("/((?!api|_next).*)").is_none());
        assert!(parse_matcher("/about/(.*)").is_none());
        assert!(parse_matcher("about").is_none());

        // The matcher matches the whole path: `/about/:path` matches `/about/a` but not
        // `/about/a/c`.
        assert!(covers("/about", "/about"));
        assert!(!covers("/about", "/about/team"));
        assert!(!covers("/about", "/blog/about"));
        assert!(!covers("/about", "/"));
        assert!(covers("/about/:path", "/about/{id}"));
        assert!(covers("/about/:path", "/about/a"));
        assert!(!covers("/about/:path", "/about/a/c"));
        assert!(!covers("/about/:path", "/about"));
        assert!(covers("/about/:path*", "/about"));
        assert!(covers("/about/:path*", "/about/a/c"));
        assert!(!covers("/about/:path+", "/about"));
        assert!(covers("/about/:path+", "/about/{id}"));
        assert!(covers("/about/:path+", "/about/a/c"));
        assert!(covers("/about/:path?", "/about"));
        assert!(covers("/about/:path?", "/about/a"));
        assert!(!covers("/about/:path?", "/about/a/c"));
        assert!(covers("/:lang/docs", "/{lang}/docs"));
        assert!(covers("/:lang/docs", "/en/docs"));
        assert!(!covers("/:lang/docs", "/en/docs/intro"));
        assert!(!covers("/:lang/docs", "/en/blog"));
        // A route's catch-all takes whatever segments the matcher still has.
        assert!(covers("/docs/intro", "/docs/{...slug}"));
        assert!(covers("/docs/a/b", "/docs/{...slug}"));
        assert!(covers("/docs/:p", "/docs/{...slug}"));
        assert!(!covers("/docs", "/docs/{...slug}"));
        assert!(covers("/docs", "/docs/{...slug?}"));
        assert!(covers("/docs/intro", "/docs/{id}"));
        assert!(!covers("/docs/intro/more", "/docs/{id}"));
        assert!(covers("/", "/"));
        assert!(!covers("/", "/anything"));
        assert!(covers("/", "/{...all?}"));
    }
}
