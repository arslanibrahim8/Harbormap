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

/// Everything extracted from one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileFacts {
    pub status: ParseStatus,
    pub symbols: Vec<SymbolFact>,
    pub imports: Vec<ImportFact>,
    pub exports: Vec<ExportFact>,
    pub calls: Vec<CallFact>,
    pub heritage: Vec<HeritageFact>,
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
        }
    }
}
