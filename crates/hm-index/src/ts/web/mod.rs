//! Web facts of one TypeScript, TSX or JavaScript file: the routes, client calls and
//! wiring that the L2a rules read (Learn spec §6.3). Next.js, client-side and
//! server-side (Express, NestJS) extraction each live in their own module and fill
//! their part of `WebFacts`; what they share lives here.

use tree_sitter::{Language, Node, Query};

use crate::facts::{Owner, SymbolFact, WebFacts};
use crate::lang::Lang;

mod client;
mod next;
mod server;

/// A symbol with the byte range it occupies, to find the symbol around a node.
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub owner: String,
    pub name: String,
}

/// An inline function that became a symbol of its own, so that calls inside it
/// have it as owner.
pub(crate) struct Callback {
    pub fact: SymbolFact,
    pub start: usize,
    pub end: usize,
}

/// The `(owner, name)` of the innermost span around `pos`; `None` at top level.
pub(crate) fn owner_at(spans: &[Span], pos: usize) -> Owner {
    spans
        .iter()
        .filter(|s| s.start <= pos && pos < s.end)
        .min_by_key(|s| s.end - s.start)
        .map(|s| (s.owner.clone(), s.name.clone()))
}

/// The inline route handlers of the file, as symbols.
pub(crate) fn route_callbacks(root: Node, src: &[u8], lang: Lang) -> Vec<Callback> {
    server::route_callbacks(root, src, lang)
}

/// All web facts of the file. `spans` are its symbols, callbacks included.
pub(crate) fn extract_web(root: Node, src: &[u8], lang: Lang, spans: &[Span]) -> WebFacts {
    let mut out = WebFacts::default();
    next::extract(root, src, lang, spans, &mut out);
    client::extract(root, src, lang, spans, &mut out);
    server::extract(root, src, lang, spans, &mut out);
    out
}

/// Compiles the part of `scm` that applies to `lang`. A query that does not compile is
/// a bug and panics; `None` for a language L1 does not parse. Each area module keeps
/// the result in one `OnceLock` per language.
pub(crate) fn compile(scm: &str, lang: Lang) -> Option<Query> {
    let language: Language = match lang {
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => return None,
    };
    let source = super::extract::query_source(scm, lang);
    Some(
        Query::new(&language, &source)
            .unwrap_or_else(|e| panic!("the bundled {lang:?} web query compiles: {e}")),
    )
}

/// The hosts that mean this machine: a client that calls one of them reaches a route of the
/// project being indexed.
const LOCAL_HOSTS: [&str; 4] = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"];

/// The path a client call asks for. `raw` is the literal's text without quotes, with each
/// `${…}` already replaced by `{}`. Drops `?…` and `#…`, collapses `//` and a trailing `/`,
/// and keeps a leading `{}`.
///
/// An absolute `http://` or `https://` URL (the scheme in any case) is a local path only
/// when its host is `localhost`, `127.0.0.1`, `0.0.0.0` or `[::1]`, with any port; any other
/// host stays in the result, lower-cased and with its port, as `//host/path`. A
/// protocol-relative `//host/x` keeps its leading `//` too. Neither starts with a single `/`,
/// so a route never matches them. A reference with no path (`""`, `#top`, `?page=2`) means
/// the current page and gives `""`.
pub fn normalize_client_url(raw: &str) -> String {
    let end = raw.find(['?', '#']).unwrap_or(raw.len());
    let mut rest = &raw[..end];
    let mut had_scheme = false;
    for scheme in ["http://", "https://"] {
        if rest
            .get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
        {
            let after = &rest[scheme.len()..];
            let (host, path) = after.split_at(after.find('/').unwrap_or(after.len()));
            let host = host.to_ascii_lowercase();
            let name = host.rsplit_once(':').map_or(host.as_str(), |(name, port)| {
                // `[::1]` has colons of its own; a port is digits after the last one.
                if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
                    name
                } else {
                    host.as_str()
                }
            });
            if !LOCAL_HOSTS.contains(&name) {
                let joined = path_parts(path);
                return if joined.is_empty() {
                    format!("//{host}")
                } else {
                    format!("//{host}/{joined}")
                };
            }
            rest = path;
            had_scheme = true;
            break;
        }
    }
    let joined = path_parts(rest);
    if !had_scheme && rest.starts_with("//") {
        return format!("//{joined}");
    }
    // A leading `{}` is a base URL variable, not a path segment: `{}/api` stays as is.
    if rest.starts_with('/') || had_scheme {
        format!("/{joined}")
    } else {
        joined
    }
}

/// The non-empty `/`-separated parts of `path`, joined by `/`.
fn path_parts(path: &str) -> String {
    path.split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize, owner: &str, name: &str) -> Span {
        Span {
            start,
            end,
            owner: owner.into(),
            name: name.into(),
        }
    }

    #[test]
    fn owner_at_is_the_innermost_span() {
        let spans = [span(0, 100, "", "outer"), span(10, 20, "", "inner")];
        assert_eq!(owner_at(&spans, 15), Some((String::new(), "inner".into())));
        assert_eq!(owner_at(&spans, 50), Some((String::new(), "outer".into())));
        assert_eq!(owner_at(&spans, 100), None);
    }
}
