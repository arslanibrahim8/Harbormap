//! The facts one source file yields: symbols, imports, exports, calls and class
//! heritage (Learn spec §5.2). Every list is in source order.

use serde::{Deserialize, Serialize};

/// What a symbol is. `Interface` covers `type` aliases as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Variable,
}

impl SymbolKind {
    /// The name used in the store, in hashes and in printed output.
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Variable => "variable",
        }
    }
}

/// Whether the parser found the whole file well-formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    Parsed,
    ParsedWithErrors,
}

/// A function, method, class, interface or exported variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolFact {
    pub kind: SymbolKind,
    pub name: String,
    /// The class name for a method, `""` at top level.
    pub owner: String,
    /// The declaration text up to its body, whitespace collapsed, comments removed.
    pub sig: String,
    pub sig_hash: String,
    pub body_hash: String,
    /// First line, 1-based.
    pub l0: u32,
    /// Last line, 1-based and inclusive.
    pub l1: u32,
    pub exported: bool,
    pub default_export: bool,
    pub lines: u32,
    pub branches: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    Static,
    Dynamic,
    Require,
    ReExport,
}

/// One name an import binds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedName {
    /// `"default"`, `"*"` or the exported name.
    pub imported: String,
    pub local: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportFact {
    pub specifier: String,
    pub kind: ImportKind,
    pub names: Vec<ImportedName>,
    pub type_only: bool,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportFact {
    /// The exported name; `"*"` for `export * from`.
    pub name: String,
    pub local: Option<String>,
    pub from: Option<String>,
    pub star: bool,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallFact {
    /// `(owner, name)` of the enclosing symbol; `None` at top level.
    pub owner: Option<(String, String)>,
    pub callee: String,
    pub receiver: Option<String>,
    pub args: u32,
    pub jsx: bool,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeritageFact {
    pub class: String,
    pub extends: String,
    pub line: u32,
}

/// A named nested function or closure inside a symbol (spec §7.2 item 12). It is a
/// label for the lines it spans, not a symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelFact {
    pub owner: Option<(String, String)>,
    pub name: String,
    pub l0: u32,
    pub l1: u32,
}

/// The type a class states for one of its attributes, as written (`Repo`, `models.Repo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttrFact {
    pub class: String,
    pub attr: String,
    pub type_name: String,
    pub line: u32,
}

/// Everything extracted from one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileFacts {
    pub status: ParseStatus,
    pub symbols: Vec<SymbolFact>,
    pub imports: Vec<ImportFact>,
    pub exports: Vec<ExportFact>,
    pub calls: Vec<CallFact>,
    pub heritage: Vec<HeritageFact>,
    /// The Go package clause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<AttrFact>,
}

impl FileFacts {
    /// No facts, with `status`.
    pub fn empty(status: ParseStatus) -> FileFacts {
        FileFacts {
            status,
            symbols: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            calls: Vec::new(),
            heritage: Vec::new(),
            package: None,
            labels: Vec::new(),
            attrs: Vec::new(),
        }
    }

    /// The index of the symbol that holds a call at `line` whose fact names `(owner, name)`:
    /// of the functions, methods and classes with that pair that span the line, the one
    /// with the smallest span; `None` when there is none or the smallest span is shared.
    pub fn holder(&self, owner: &str, name: &str, line: u32) -> Option<usize> {
        let holders: Vec<(usize, u32)> = self
            .symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.owner == owner
                    && s.name == name
                    && matches!(
                        s.kind,
                        SymbolKind::Function | SymbolKind::Method | SymbolKind::Class
                    )
                    && s.l0 <= line
                    && line <= s.l1
            })
            .map(|(i, s)| (i, s.l1 - s.l0))
            .collect();
        let smallest = holders.iter().map(|&(_, span)| span).min();
        let mut tightest = holders.iter().filter(|&&(_, span)| Some(span) == smallest);
        match (tightest.next(), tightest.next()) {
            (Some(&(i, _)), None) => Some(i),
            _ => None,
        }
    }
}
