//! Client call and data-hook facts: `fetch`, axios, request helpers and SWR keys become
//! `UrlCallFact`s; the functions handed to React Query and SWR become `HookFact`s.
//! The query finds every call; the code here decides which ones are of interest.

use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

use super::{Span, compile, normalize_client_url, owner_at};
use crate::facts::{ArgFact, HookFact, UrlCallFact, WebFacts};
use crate::lang::Lang;

const CLIENT_SCM: &str = include_str!("queries/client.scm");

/// The HTTP verbs a client method can be named after.
const VERBS: [&str; 7] = ["get", "post", "put", "patch", "delete", "head", "options"];

/// React Query hooks that take an options object.
const QUERY_HOOKS: [&str; 4] = [
    "useMutation",
    "useQuery",
    "useSuspenseQuery",
    "useInfiniteQuery",
];

/// The options of those hooks that hold a function.
const HOOK_OPTIONS: [&str; 5] = ["mutationFn", "queryFn", "onSuccess", "onSettled", "onError"];

fn query(lang: Lang) -> Option<&'static Query> {
    static TS: OnceLock<Option<Query>> = OnceLock::new();
    static TSX: OnceLock<Option<Query>> = OnceLock::new();
    static JS: OnceLock<Option<Query>> = OnceLock::new();
    let cell = match lang {
        Lang::TypeScript => &TS,
        Lang::Tsx => &TSX,
        Lang::JavaScript => &JS,
        _ => return None,
    };
    cell.get_or_init(|| compile(CLIENT_SCM, lang)).as_ref()
}

pub(crate) fn extract(root: Node, src: &[u8], lang: Lang, spans: &[Span], out: &mut WebFacts) {
    let Some(q) = query(lang) else {
        return;
    };
    let names = q.capture_names();
    let reader = Reader { src, spans };
    let mut found: Vec<(usize, Found)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(q, root, src);
    while let Some(m) = matches.next() {
        let (mut call, mut name, mut receiver, mut args) = (None, None, None, None);
        for cap in m.captures() {
            match names[cap.index as usize] {
                "call" => call = Some(cap.node),
                "name" => name = Some(reader.text(cap.node)),
                "receiver" => receiver = Some(cap.node),
                "args" => args = Some(cap.node),
                _ => {}
            }
        }
        let (Some(call), Some(name), Some(args)) = (call, name, args) else {
            continue;
        };
        let site = Site {
            call,
            name,
            receiver,
            args: named_children(args),
        };
        let mut facts = Found::default();
        reader.read(&site, &mut facts);
        found.push((call.start_byte(), facts));
    }
    found.sort_by_key(|(start, _)| *start);
    for (_, facts) in found {
        out.url_calls.extend(facts.urls);
        out.hooks.extend(facts.hooks);
    }
}

/// One call expression: the callee name, its receiver (for `a.b.name(…)`) and its arguments
/// without comments.
struct Site<'t> {
    call: Node<'t>,
    name: &'t str,
    receiver: Option<Node<'t>>,
    args: Vec<Node<'t>>,
}

#[derive(Default)]
struct Found {
    urls: Vec<UrlCallFact>,
    hooks: Vec<HookFact>,
}

struct Reader<'s> {
    src: &'s [u8],
    spans: &'s [Span],
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .collect()
}

fn line_of(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

fn is_verb(name: &str) -> bool {
    VERBS.contains(&name)
}

/// What an options object says about the HTTP method.
enum Method {
    /// A literal `method:` property, upper-cased.
    Literal(String),
    /// No `method:` property and nothing that could hide one.
    Absent,
    /// A `method:` property or a spread that is not a literal.
    Unknown,
}

impl<'s> Reader<'s> {
    fn text(&self, node: Node) -> &'s str {
        node.utf8_text(self.src).unwrap_or("")
    }

    fn read(&self, site: &Site, out: &mut Found) {
        match site.receiver {
            None => match site.name {
                "fetch" => self.fetch(site, out),
                "axios" => self.axios(site, None, out),
                "useSWR" | "useSWRMutation" => self.swr(site, out),
                n if QUERY_HOOKS.contains(&n) => self.query_hook(site, out),
                _ => self.request_helper(site, out),
            },
            Some(receiver) => {
                let root = self.root_name(receiver);
                if site.name == "fetch" && self.text(receiver) == "window" {
                    return self.fetch(site, out);
                }
                let Some(root) = root else {
                    return self.request_helper(site, out);
                };
                let axios_like = {
                    let lower = root.to_ascii_lowercase();
                    lower == "axios" || ["api", "client", "http"].iter().any(|s| lower.ends_with(s))
                };
                let first_is_object = site.args.first().is_some_and(|a| a.kind() == "object");
                if is_verb(site.name) && axios_like && !first_is_object {
                    self.axios(site, Some(site.name), out);
                } else if root == "axios" && (site.name == "request" || is_verb(site.name)) {
                    self.axios(site, is_verb(site.name).then_some(site.name), out);
                } else {
                    self.request_helper(site, out);
                }
            }
        }
    }

    /// The leftmost identifier of `a.b.c`.
    fn root_name(&self, node: Node) -> Option<&'s str> {
        match node.kind() {
            "identifier" => Some(self.text(node)),
            "member_expression" => self.root_name(node.child_by_field_name("object")?),
            _ => None,
        }
    }

    fn push_url(
        &self,
        site: &Site,
        client: &str,
        method: Option<String>,
        url: &str,
        out: &mut Found,
    ) {
        out.urls.push(UrlCallFact {
            owner: owner_at(self.spans, site.call.start_byte()),
            client: client.into(),
            method,
            url: normalize_client_url(url),
            line: line_of(site.call),
        });
    }

    /// `fetch(url, init?)`; the method is `GET` unless `init` says otherwise.
    fn fetch(&self, site: &Site, out: &mut Found) {
        let Some(url) = site.args.first().and_then(|a| self.url_text(*a)) else {
            return;
        };
        let method = match site.args.get(1) {
            None => Some("GET".into()),
            Some(init) => match self.method_of(*init) {
                Method::Literal(m) => Some(m),
                Method::Absent => Some("GET".into()),
                Method::Unknown => None,
            },
        };
        self.push_url(site, "fetch", method, &url, out);
    }

    /// `axios(url, config?)`, `axios({ url, method })`, `axios.request({…})` and
    /// `<verb>(url, …)` on an axios-like receiver. `verb` is the callee name when it is one.
    /// Without anything said, axios uses `GET`.
    fn axios(&self, site: &Site, verb: Option<&str>, out: &mut Found) {
        let Some(first) = site.args.first() else {
            return;
        };
        let (url, config) = match first.kind() {
            "object" => (self.string_prop(*first, "url"), Some(*first)),
            _ => (self.url_text(*first), site.args.get(1).copied()),
        };
        let Some(url) = url else {
            return;
        };
        let method = match verb {
            Some(v) => Some(v.to_ascii_uppercase()),
            None => match config.map_or(Method::Absent, |c| self.method_of(c)) {
                Method::Literal(m) => Some(m),
                Method::Absent => Some("GET".into()),
                Method::Unknown => None,
            },
        };
        self.push_url(site, "axios", method, &url, out);
    }

    /// Any call with an object argument that has a literal `url:` property.
    fn request_helper(&self, site: &Site, out: &mut Found) {
        let found = site
            .args
            .iter()
            .filter(|a| a.kind() == "object")
            .find_map(|obj| self.string_prop(*obj, "url").map(|u| (*obj, u)));
        let Some((obj, url)) = found else {
            return;
        };
        let method = match self.method_of(obj) {
            Method::Literal(m) => Some(m),
            Method::Absent | Method::Unknown => {
                is_verb(site.name).then(|| site.name.to_ascii_uppercase())
            }
        };
        self.push_url(site, "request_helper", method, &url, out);
    }

    /// `useSWR(key, fetcher?)` and `useSWRMutation(key, fetcher?)`.
    fn swr(&self, site: &Site, out: &mut Found) {
        if let Some(key) = site.args.first().and_then(|a| self.url_text(*a)) {
            // A key like `'user'` names data, not a URL.
            if key.starts_with('/') || key.starts_with("http://") || key.starts_with("https://") {
                let method = (site.name == "useSWR").then(|| "GET".to_string());
                self.push_url(site, "swr", method, &key, out);
            }
        }
        if let Some(fetcher) = site.args.get(1) {
            out.hooks.push(HookFact {
                owner: owner_at(self.spans, site.call.start_byte()),
                hook: site.name.into(),
                key: "fetcher".into(),
                target: self.arg_fact(*fetcher),
                line: line_of(site.call),
            });
        }
    }

    /// `useMutation({ mutationFn, onSuccess, … })` and its siblings.
    fn query_hook(&self, site: &Site, out: &mut Found) {
        let Some(options) = site.args.first().filter(|a| a.kind() == "object") else {
            return;
        };
        for (key, value) in self.properties(*options) {
            if HOOK_OPTIONS.contains(&key.as_str()) {
                out.hooks.push(HookFact {
                    owner: owner_at(self.spans, site.call.start_byte()),
                    hook: site.name.into(),
                    key,
                    target: self.arg_fact(value),
                    line: line_of(value),
                });
            }
        }
    }

    /// The properties of an object literal: `key: value`, `key` (shorthand) and `key() {}`.
    /// The node is the value, the shorthand identifier or the method.
    fn properties<'t>(&self, object: Node<'t>) -> Vec<(String, Node<'t>)> {
        let mut out = Vec::new();
        for child in named_children(object) {
            let (key, value) = match child.kind() {
                "pair" => (
                    child.child_by_field_name("key"),
                    child.child_by_field_name("value"),
                ),
                "shorthand_property_identifier" => (Some(child), Some(child)),
                "method_definition" => (child.child_by_field_name("name"), Some(child)),
                _ => continue,
            };
            let (Some(key), Some(value)) = (key, value) else {
                continue;
            };
            let key = match key.kind() {
                "property_identifier" | "shorthand_property_identifier" => {
                    self.text(key).to_string()
                }
                "string" => match self.string_value(key) {
                    Some(k) => k,
                    None => continue,
                },
                _ => continue,
            };
            out.push((key, value));
        }
        out
    }

    fn property<'t>(&self, object: Node<'t>, name: &str) -> Option<Node<'t>> {
        self.properties(object)
            .into_iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    }

    /// The URL of the string or template literal in property `name`.
    fn string_prop(&self, object: Node, name: &str) -> Option<String> {
        self.property(object, name).and_then(|v| self.url_text(v))
    }

    fn method_of(&self, node: Node) -> Method {
        if node.kind() != "object" {
            return Method::Unknown;
        }
        if let Some(value) = self.property(node, "method") {
            return match self.url_text(value) {
                Some(m) if !m.contains("{}") => Method::Literal(m.to_ascii_uppercase()),
                _ => Method::Unknown,
            };
        }
        if named_children(node)
            .iter()
            .any(|c| c.kind() == "spread_element")
        {
            Method::Unknown
        } else {
            Method::Absent
        }
    }

    /// The text of a string without quotes; `None` for anything else.
    fn string_value(&self, node: Node) -> Option<String> {
        if node.kind() != "string" {
            return None;
        }
        let t = self.text(node);
        t.get(1..t.len().checked_sub(1)?).map(str::to_string)
    }

    /// The text of a string or template literal, each `${…}` as `{}`; `None` for anything
    /// else and for the empty string.
    fn url_text(&self, node: Node) -> Option<String> {
        let text = match node.kind() {
            "string" => self.string_value(node)?,
            "template_string" => {
                let mut cursor = node.walk();
                let mut out = String::new();
                for part in node.named_children(&mut cursor) {
                    match part.kind() {
                        "template_substitution" => out.push_str("{}"),
                        _ => out.push_str(self.text(part)),
                    }
                }
                out
            }
            _ => return None,
        };
        (!text.is_empty()).then_some(text)
    }

    /// `a.b.c` as text; `None` for anything that is not a plain path.
    fn dotted_name(&self, node: Node) -> Option<String> {
        match node.kind() {
            "identifier" | "this" => Some(self.text(node).to_string()),
            "member_expression" => {
                let object = self.dotted_name(node.child_by_field_name("object")?)?;
                let property = node.child_by_field_name("property")?;
                (property.kind() == "property_identifier")
                    .then(|| format!("{object}.{}", self.text(property)))
            }
            _ => None,
        }
    }

    fn arg_fact(&self, node: Node) -> ArgFact {
        match node.kind() {
            "string" | "template_string" => {
                self.url_text(node).map_or(ArgFact::Other, ArgFact::Str)
            }
            "identifier" | "shorthand_property_identifier" => {
                ArgFact::Name(self.text(node).to_string())
            }
            "member_expression" => self.dotted_name(node).map_or(ArgFact::Other, ArgFact::Name),
            "arrow_function"
            | "function_expression"
            | "function"
            | "generator_function"
            | "method_definition" => {
                let name = self
                    .spans
                    .iter()
                    .find(|s| s.start == node.start_byte() && s.end == node.end_byte())
                    .map(|s| s.name.clone());
                ArgFact::Inline(name)
            }
            "object" => ArgFact::Object {
                path: self.string_prop(node, "path"),
            },
            "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression" => node
                .named_child(0)
                .map_or(ArgFact::Other, |inner| self.arg_fact(inner)),
            _ => ArgFact::Other,
        }
    }
}
