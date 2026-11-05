//! The facts one source file yields: symbols, imports, exports, calls, class
//! heritage and web facts (Learn spec §5.2). Every list is in source order.

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

/// `(owner, name)` of the enclosing symbol; `None` at top level.
pub type Owner = Option<(String, String)>;

/// One argument of a call, as far as a rule needs to read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum ArgFact {
    /// A string literal; a template literal with each `${…}` as `{}`.
    Str(String),
    /// An identifier or member path: `createInvoice`, `UsersService.readUserMe`.
    Name(String),
    /// An inline function; `Some(name)` when it became a synthetic symbol.
    Inline(Option<String>),
    /// An object literal with its `path:` string property.
    Object {
        path: Option<String>,
    },
    Other,
}

/// A `"use client"` / `"use server"` style directive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DirectiveFact {
    pub owner: Owner,
    /// `None` for the file prologue; `Some(name)` of the function whose body starts with
    /// it (`""` if anonymous).
    pub function: Option<String>,
    pub text: String,
    pub line: u32,
}

/// A client call to a URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlCallFact {
    pub owner: Owner,
    /// `fetch`, `axios`, `request_helper` or `swr`.
    pub client: String,
    /// Upper-case.
    pub method: Option<String>,
    /// As `normalize_client_url` spells it.
    pub url: String,
    pub line: u32,
    /// The plain name an axios call is made on (`api` in `api.get('/x')`), when it is not
    /// `axios` itself and the URL is relative, so that the `baseURL` of an
    /// `axios.create({…})` bound to that name applies. Absent from the JSON otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
}

/// A module-level `name = axios.create({…})`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxiosFact {
    pub name: String,
    /// The `baseURL` as `normalize_client_url` spells it: `""` without one, `{}` when it is
    /// not a literal (or may be hidden by a spread or a config held in a variable).
    pub base_url: String,
    pub line: u32,
}

/// A call on a receiver that may be a router or an app, with its arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCallFact {
    pub owner: Owner,
    pub receiver: String,
    pub method: String,
    pub args: Vec<ArgFact>,
    pub line: u32,
}

/// A module-level `name = init` binding, with the initialiser text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingFact {
    pub owner: Owner,
    pub name: String,
    pub init: String,
    pub line: u32,
}

/// A decorator on a class or method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecoratorFact {
    /// `(class, member)`; the member is `""` for a class decorator.
    pub target: (String, String),
    pub name: String,
    pub args: Vec<ArgFact>,
    pub line: u32,
}

/// A constructor parameter property, for dependency injection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CtorFieldFact {
    pub class: String,
    pub field: String,
    pub type_name: String,
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

/// A `<form>` (or form component) with its action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormFact {
    pub owner: Owner,
    pub element: String,
    pub action: Option<ArgFact>,
    /// Upper-case; `ANY` when the attribute is there but not a string literal; `None`
    /// without the attribute.
    pub method: Option<String>,
    pub on_submit: bool,
    pub line: u32,
}

/// A local name that stands for another one (`const f = useActionState(a)`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasFact {
    pub owner: Owner,
    pub local: String,
    pub target: String,
    pub via: String,
    pub line: u32,
}

/// A data hook (`useMutation`, `useSWR`, …) with the function it is given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookFact {
    pub owner: Owner,
    pub hook: String,
    pub key: String,
    pub target: ArgFact,
    pub line: u32,
}

/// The `matcher` of a Next.js middleware config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatcherFact {
    pub values: Vec<String>,
    pub line: u32,
}

/// A place where a rule could not read what it needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapFact {
    pub owner: Owner,
    pub kind: String,
    pub line: u32,
}

/// A read of an environment variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvFact {
    pub owner: Owner,
    pub name: String,
    pub line: u32,
}

/// What the web rules read from one file (Learn spec §6.3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebFacts {
    pub directives: Vec<DirectiveFact>,
    pub url_calls: Vec<UrlCallFact>,
    pub route_calls: Vec<RouteCallFact>,
    pub bindings: Vec<BindingFact>,
    pub decorators: Vec<DecoratorFact>,
    pub ctor_fields: Vec<CtorFieldFact>,
    pub forms: Vec<FormFact>,
    pub aliases: Vec<AliasFact>,
    pub hooks: Vec<HookFact>,
    pub matcher: Option<MatcherFact>,
    pub gaps: Vec<GapFact>,
    pub env: Vec<EnvFact>,
    /// Absent from the JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub axios_instances: Vec<AxiosFact>,
}

impl WebFacts {
    pub fn is_empty(&self) -> bool {
        *self == WebFacts::default()
    }
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
    /// Every name a top-level statement binds that is not a top-level symbol: `const`,
    /// `let` and `var` names (every name of a destructuring pattern), `enum`,
    /// `namespace`, `import x = require()` and `declare` names. Sorted, without
    /// duplicates; absent from the JSON when empty, as in L1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_names: Vec<String>,
    /// Absent from the JSON of a file without web facts, as in L1.
    #[serde(default, skip_serializing_if = "WebFacts::is_empty")]
    pub web: WebFacts,
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
            module_names: Vec::new(),
            web: WebFacts::default(),
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
