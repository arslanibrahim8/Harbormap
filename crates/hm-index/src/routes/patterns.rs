//! Patterns (Learn spec §7.3) and gaps. The TS library rules are the embedded pack
//! `packs/learn/patterns-ts.toml`; its header says what each field of a row means.
//!
//! A call is matched by name through an index, then checked against the rule's scope
//! (the file imports the package, or its manifest declares it) and receiver. Whether a
//! name is bound to the package by an import is read from the file's imports alone: no
//! types, no flow. A name that an import binds to another package is never a match.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use hm_core::ids::file_id;
use serde::Deserialize;

use super::{Derived, Gap, Ids, Inputs, Pattern};
use crate::facts::{FileFacts, ImportKind, SymbolKind};
use crate::manifest::Manifests;
use crate::ts::resolve::binding_package;

const PACK: &str = include_str!("../../../../packs/learn/patterns-ts.toml");

/// Receivers that are JS globals; a call on one is never a library's, unless an import
/// says so.
const GLOBAL_RECEIVERS: &[&str] = &[
    "Array",
    "Buffer",
    "Date",
    "FormData",
    "Headers",
    "Intl",
    "JSON",
    "Map",
    "Math",
    "Number",
    "Object",
    "Promise",
    "Reflect",
    "Set",
    "String",
    "URL",
    "URLSearchParams",
    "WeakMap",
    "WeakSet",
    "console",
    "crypto",
    "document",
    "globalThis",
    "localStorage",
    "process",
    "sessionStorage",
    "window",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Scope {
    Import,
    Manifest,
    Global,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Receiver {
    #[default]
    Any,
    Package,
    None,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    id: String,
    kind: String,
    #[serde(default)]
    package: String,
    scope: Scope,
    #[serde(default)]
    callees: Vec<String>,
    #[serde(default)]
    decorators: Vec<String>,
    #[serde(default)]
    receiver: Receiver,
    #[serde(default)]
    receiver_depth: usize,
    #[serde(default)]
    receiver_names: Vec<String>,
    term: String,
}

#[derive(Debug, Deserialize)]
struct PackFile {
    rule: Vec<Rule>,
}

/// The rules with an index from callee and decorator names to the rules that list them.
struct Pack {
    rules: Vec<Rule>,
    by_callee: HashMap<String, Vec<usize>>,
    by_decorator: HashMap<String, Vec<usize>>,
}

/// The embedded pack. A pack that does not parse (a test catches it) gives no rules.
fn pack() -> &'static Pack {
    static PACK_ONCE: OnceLock<Pack> = OnceLock::new();
    PACK_ONCE.get_or_init(|| {
        let rules = toml::from_str::<PackFile>(PACK).map_or_else(|_| Vec::new(), |p| p.rule);
        let mut by_callee: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_decorator: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, rule) in rules.iter().enumerate() {
            for name in &rule.callees {
                by_callee.entry(name.clone()).or_default().push(i);
            }
            for name in &rule.decorators {
                by_decorator.entry(name.clone()).or_default().push(i);
            }
        }
        Pack {
            rules,
            by_callee,
            by_decorator,
        }
    })
}

/// What an import binds a local name to.
struct Binding {
    package: String,
    imported: String,
    specifier: String,
}

/// How a name stands to a rule's package.
#[derive(PartialEq)]
enum Bound {
    /// Bound by an import of the package (or of the subpath the rule names).
    Yes,
    /// Bound by an import of something else.
    Other,
    /// Not bound by an import.
    No,
}

/// Everything the rules read about one file.
struct FileCtx<'a> {
    rel: &'a str,
    manifests: &'a Manifests,
    facts: &'a FileFacts,
    bindings: HashMap<&'a str, Binding>,
}

impl<'a> FileCtx<'a> {
    fn new(inputs: &'a Inputs, rel: &'a str, facts: &'a FileFacts) -> FileCtx<'a> {
        let mut bindings = HashMap::new();
        for import in facts
            .imports
            .iter()
            .filter(|i| i.kind != ImportKind::ReExport && !i.type_only)
        {
            for name in &import.names {
                if bindings.contains_key(name.local.as_str()) {
                    continue;
                }
                if let Some((package, imported)) = binding_package(inputs.project, rel, &name.local)
                {
                    bindings.insert(
                        name.local.as_str(),
                        Binding {
                            package,
                            imported,
                            specifier: import.specifier.clone(),
                        },
                    );
                }
            }
        }
        FileCtx {
            rel,
            manifests: inputs.manifests,
            facts,
            bindings,
        }
    }

    fn bound(&self, root: &str, package: &str) -> Bound {
        let Some(b) = self.bindings.get(root) else {
            return Bound::No;
        };
        let same_package = within(package, &b.package);
        if same_package && within(&b.specifier, package) {
            Bound::Yes
        } else {
            Bound::Other
        }
    }

    /// True when the file imports `package` or a subpath of it, in any way.
    fn imports(&self, package: &str) -> bool {
        self.facts.imports.iter().any(|i| {
            i.kind != ImportKind::ReExport && !i.type_only && within(&i.specifier, package)
        })
    }

    /// The rules that may match a call or decorator named `name` on `receiver`: those
    /// that list the name, and, for a bare name an import bound under another local
    /// name, those that list the imported name.
    fn candidates(
        &self,
        index: &HashMap<String, Vec<usize>>,
        receiver: Option<&str>,
        name: &str,
    ) -> Vec<usize> {
        let mut found: Vec<usize> = index.get(name).cloned().unwrap_or_default();
        if receiver.is_none()
            && let Some(b) = self.bindings.get(name)
            && let Some(more) = index.get(&b.imported)
        {
            found.extend(more);
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The confidence of `rule` on a call or decorator, or `None` when it does not apply.
    fn confidence(&self, rule: &Rule, receiver: Option<&str>, name: &str) -> Option<&'static str> {
        if let Scope::Global = rule.scope {
            let on = receiver.is_some_and(|r| rule.receiver_names.iter().any(|n| n == r));
            return on.then_some("sure");
        }
        let depth = receiver.map_or(0, |r| r.split('.').count());
        if depth < rule.receiver_depth {
            return None;
        }
        let in_scope = match rule.scope {
            Scope::Import => self.imports(&rule.package),
            Scope::Manifest => self
                .manifests
                .declares(self.rel, base_package(&rule.package)),
            Scope::Global => false,
        };
        if !in_scope {
            return None;
        }
        let root = root_ident(receiver.unwrap_or(name));
        let bound = self.bound(root, &rule.package);
        match (&rule.receiver, bound) {
            (_, Bound::Other) => None,
            (Receiver::Package, Bound::Yes) => Some("sure"),
            (Receiver::Package, _) => None,
            (Receiver::None, _) if receiver.is_some() => None,
            (Receiver::Any, Bound::No)
                if receiver.is_some() && GLOBAL_RECEIVERS.contains(&root) =>
            {
                None
            }
            (_, Bound::Yes) => Some("sure"),
            (_, Bound::No) => Some("probably"),
        }
    }

    /// The symbol a decorator is written on, or the file when no one symbol is.
    fn decorated(&self, ids: &mut Ids, (owner, name): &(String, String)) -> String {
        let mut named = self.facts.symbols.iter().enumerate().filter(|(_, s)| {
            s.owner == *owner
                && s.name == *name
                && matches!(s.kind, SymbolKind::Class | SymbolKind::Method)
        });
        let symbol = match (named.next(), named.next()) {
            (Some((i, _)), None) => ids.symbol(self.rel, i),
            _ => None,
        };
        symbol.unwrap_or_else(|| file_id(self.rel))
    }
}

/// True when `specifier` is `package` or a path below it.
fn within(specifier: &str, package: &str) -> bool {
    specifier == package
        || specifier
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The npm package of a rule's `package`: `next` for `next/cache`.
fn base_package(package: &str) -> &str {
    let mut parts = package.match_indices('/');
    let cut = if package.starts_with('@') {
        parts.nth(1)
    } else {
        parts.next()
    };
    cut.map_or(package, |(i, _)| &package[..i])
}

/// The leading identifier of an expression.
fn root_ident(expr: &str) -> &str {
    let end = expr
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(expr.len());
    &expr[..end]
}

/// The patterns and gaps of every file.
pub(crate) fn derive(inputs: &Inputs, out: &mut Derived) {
    let pack = pack();
    let mut ids = Ids::new(inputs.project);
    // One row per primary key; of two rules that differ only in confidence, the surer.
    let mut found: BTreeMap<(String, String, String, u32, String), Pattern> = BTreeMap::new();
    let mut add = |p: Pattern| {
        let key = (
            p.file.clone(),
            p.subject_id.clone(),
            p.kind.clone(),
            p.line,
            p.rule.clone(),
        );
        match found.get(&key) {
            Some(old) if old.conf >= p.conf => {}
            _ => {
                found.insert(key, p);
            }
        }
    };

    for (rel, facts) in inputs.project.facts {
        let ctx = FileCtx::new(inputs, rel, facts);
        let pattern = |rule: &Rule, subject_id: String, line: u32, conf: &str| Pattern {
            file: rel.clone(),
            subject_id,
            kind: rule.kind.clone(),
            line,
            rule: format!("ts.pattern.{}", rule.id),
            conf: conf.to_string(),
            term: Some(rule.term.clone()),
            detail: None,
        };

        for call in facts.calls.iter().filter(|c| !c.jsx) {
            let receiver = call.receiver.as_deref();
            for i in ctx.candidates(&pack.by_callee, receiver, &call.callee) {
                let rule = &pack.rules[i];
                if let Some(conf) = ctx.confidence(rule, receiver, &call.callee) {
                    let subject = ids
                        .holder(rel, call.owner.as_ref(), call.line)
                        .unwrap_or_else(|| file_id(rel));
                    add(pattern(rule, subject, call.line, conf));
                }
            }
        }

        for dec in &facts.web.decorators {
            for i in ctx.candidates(&pack.by_decorator, None, &dec.name) {
                let rule = &pack.rules[i];
                if let Some(conf) = ctx.confidence(rule, None, &dec.name) {
                    let subject = ctx.decorated(&mut ids, &dec.target);
                    add(pattern(rule, subject, dec.line, conf));
                }
            }
        }

        for read in &facts.web.env {
            let subject = ids
                .holder(rel, read.owner.as_ref(), read.line)
                .unwrap_or_else(|| file_id(rel));
            add(Pattern {
                file: rel.clone(),
                subject_id: subject,
                kind: "env_read".to_string(),
                line: read.line,
                rule: "ts.pattern.env".to_string(),
                conf: "sure".to_string(),
                term: Some("env_var".to_string()),
                detail: Some(read.name.clone()),
            });
        }

        for gap in &facts.web.gaps {
            let subject = ids
                .holder(rel, gap.owner.as_ref(), gap.line)
                .unwrap_or_else(|| file_id(rel));
            out.gaps.push(Gap {
                file: rel.clone(),
                subject_id: subject,
                kind: gap.kind.clone(),
                line: gap.line,
            });
        }
    }
    out.patterns.extend(found.into_values());
}
