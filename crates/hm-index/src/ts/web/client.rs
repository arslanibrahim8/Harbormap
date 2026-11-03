use tree_sitter::Node;

use super::Span;
use crate::facts::WebFacts;
use crate::lang::Lang;

pub(crate) fn extract(_root: Node, _src: &[u8], _lang: Lang, _spans: &[Span], _out: &mut WebFacts) {
}
