//! Source languages the indexer recognises, decided by file extension.

use std::path::Path;

/// A source language. Only TypeScript, Tsx and JavaScript are parsed in L1; the
/// others are recognised so that later tasks can count them by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    Java,
    CSharp,
    Rust,
}

impl Lang {
    /// The language of `path`, from its extension. `.d.ts` is TypeScript through its
    /// `ts` extension.
    pub fn from_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "py" | "pyi" => Lang::Python,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "cs" => Lang::CSharp,
            "rs" => Lang::Rust,
            _ => return None,
        })
    }

    /// The name used in the store and in printed output.
    pub fn as_str(&self) -> &'static str {
        match self {
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::JavaScript => "javascript",
            Lang::Python => "python",
            Lang::Go => "go",
            Lang::Java => "java",
            Lang::CSharp => "csharp",
            Lang::Rust => "rust",
        }
    }

    /// Whether L1 parses files of this language with tree-sitter.
    pub fn parsed_in_l1(&self) -> bool {
        matches!(self, Lang::TypeScript | Lang::Tsx | Lang::JavaScript)
    }

    /// Whether the indexer parses files of this language with tree-sitter.
    pub fn parsed(&self) -> bool {
        matches!(
            self,
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript | Lang::Python | Lang::Go
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_comes_from_the_extension() {
        assert_eq!(Lang::from_path(Path::new("a.tsx")), Some(Lang::Tsx));
        assert_eq!(Lang::from_path(Path::new("a.d.ts")), Some(Lang::TypeScript));
        assert_eq!(Lang::from_path(Path::new("a.cjs")), Some(Lang::JavaScript));
        assert_eq!(Lang::from_path(Path::new("a.cs")), Some(Lang::CSharp));
        assert_eq!(Lang::from_path(Path::new("a.pyi")), Some(Lang::Python));
        assert_eq!(Lang::from_path(Path::new("Makefile")), None);

        let parsed: Vec<bool> = [
            Lang::TypeScript,
            Lang::Tsx,
            Lang::JavaScript,
            Lang::Python,
            Lang::Go,
            Lang::Java,
            Lang::CSharp,
            Lang::Rust,
        ]
        .iter()
        .map(Lang::parsed_in_l1)
        .collect();
        assert_eq!(
            parsed,
            [true, true, true, false, false, false, false, false]
        );
    }

    #[test]
    fn parsed_marks_ts_js_python_and_go() {
        let parsed: Vec<bool> = [
            Lang::TypeScript,
            Lang::Tsx,
            Lang::JavaScript,
            Lang::Python,
            Lang::Go,
            Lang::Java,
            Lang::CSharp,
            Lang::Rust,
        ]
        .iter()
        .map(Lang::parsed)
        .collect();
        assert_eq!(parsed, [true, true, true, true, true, false, false, false]);
    }

    #[test]
    fn as_str_gives_the_store_names() {
        let names: Vec<&str> = [
            Lang::TypeScript,
            Lang::Tsx,
            Lang::JavaScript,
            Lang::Python,
            Lang::Go,
            Lang::Java,
            Lang::CSharp,
            Lang::Rust,
        ]
        .iter()
        .map(Lang::as_str)
        .collect();
        assert_eq!(
            names,
            [
                "typescript",
                "tsx",
                "javascript",
                "python",
                "go",
                "java",
                "csharp",
                "rust"
            ]
        );
    }
}
