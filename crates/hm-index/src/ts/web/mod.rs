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

/// The path a client call asks for. `raw` is the literal's text without quotes, with each
/// `${…}` already replaced by `{}`. Drops `?…` and `#…`, drops a leading `http://host` or
/// `https://host`, collapses `//` and a trailing `/`, and keeps a leading `{}`.
pub fn normalize_client_url(raw: &str) -> String {
    let end = raw.find(['?', '#']).unwrap_or(raw.len());
    let mut rest = &raw[..end];
    for scheme in ["http://", "https://"] {
        if let Some(after) = rest.strip_prefix(scheme) {
            rest = after.find('/').map_or("", |i| &after[i..]);
            break;
        }
    }
    let joined = rest
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    // A leading `{}` is a base URL variable, not a path segment: `{}/api` stays as is.
    if rest.starts_with('/') || joined.is_empty() {
        format!("/{joined}")
    } else {
        joined
    }
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
