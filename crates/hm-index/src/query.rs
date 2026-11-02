//! Reading the facts back: readable names for files and symbols. The cards of
//! Learn spec §7 are added here later.

use crate::facts::SymbolFact;

/// The name shown for a file or a symbol: `path`, `path#name` or `path#Owner.name`.
pub fn display_name(rel: &str, sym: Option<&SymbolFact>) -> String {
    match sym {
        None => rel.to_string(),
        Some(s) if s.owner.is_empty() => format!("{rel}#{}", s.name),
        Some(s) => format!("{rel}#{}.{}", s.owner, s.name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    #[test]
    fn names_a_file_a_symbol_and_a_method() {
        let facts = crate::ts::extract("function f() {}\nclass C { m() {} }", Lang::TypeScript);
        let name = |i: usize| display_name("src/a.ts", Some(&facts.symbols[i]));
        assert_eq!(display_name("src/a.ts", None), "src/a.ts");
        assert_eq!(name(0), "src/a.ts#f");
        assert_eq!(name(2), "src/a.ts#C.m");
    }
}
