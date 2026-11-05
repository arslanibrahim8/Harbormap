//! Roles from path words (Learn L5a scope decision "Roles from paths only").
//!
//! L1 has no route, UI-framework or database facts yet, so a box's role comes from a
//! small table of folder and file-name words written for Harbormap. Folder words win
//! over file-name words, and the deepest folder word wins over shallower ones, so
//! `src/db/client.ts` is `keep` although `client` is a `see` word.

use serde::{Deserialize, Serialize};

/// What a box is for; it picks the box's colour and its overview lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// What a user sees: pages, components, styles.
    See,
    /// Logic and server endpoints; the default.
    Work,
    /// Data: models, schemas, migrations, stores.
    Keep,
    /// Code that talks to services outside the project.
    Outside,
    /// Helpers used everywhere, and the hub files of the Shared box.
    Shared,
}

/// The role table: each word is matched, lowercased, against the words of a path.
const TABLE: &[(Role, &[&str])] = &[
    (
        Role::See,
        &[
            "app",
            "pages",
            "page",
            "components",
            "component",
            "views",
            "view",
            "ui",
            "screens",
            "screen",
            "layouts",
            "layout",
            "styles",
            "public",
            "frontend",
            "web",
            "client",
        ],
    ),
    (
        Role::Work,
        &[
            "api",
            "routes",
            "route",
            "server",
            "controllers",
            "controller",
            "handlers",
            "handler",
            "actions",
            "middleware",
            "services",
            "service",
            "lib",
            "core",
            "domain",
            "jobs",
            "workers",
        ],
    ),
    (
        Role::Keep,
        &[
            "db",
            "database",
            "models",
            "model",
            "schema",
            "schemas",
            "prisma",
            "migrations",
            "repositories",
            "repository",
            "entities",
            "entity",
            "store",
            "stores",
            "data",
            "dal",
        ],
    ),
    (
        Role::Outside,
        &[
            "integrations",
            "integration",
            "external",
            "vendor",
            "sdk",
            "adapters",
            "gateways",
            "gateway",
            "webhooks",
        ],
    ),
    (
        Role::Shared,
        &[
            "shared",
            "common",
            "types",
            "utils",
            "util",
            "helpers",
            "constants",
            "config",
        ],
    ),
];

/// Names whose `work` role reads "Server endpoints" rather than "Logic".
const ENDPOINT_NAMES: &[&str] = &["api", "routes", "route", "controllers", "handlers"];

/// The role of a repository-relative path: the deepest folder word found in the role
/// table wins; when no folder word matches, the words of the file's stem are tried,
/// last word first; the default is [`Role::Work`].
///
/// The last segment counts as a file when it has an extension (`orders.ts`), and as a
/// folder otherwise (`src/api`), so a group key can be passed as it is.
pub fn role_for(path: &str) -> Role {
    let last = path.rsplit('/').next().unwrap_or("");
    role_of(path, last.rfind('.').is_some_and(|i| i > 0))
}

/// [`role_for`] with the caller saying whether the last segment is a file.
pub(crate) fn role_of(path: &str, last_is_file: bool) -> Role {
    let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let file = if last_is_file { segments.pop() } else { None };
    let folder_role = segments
        .iter()
        .rev()
        .find_map(|segment| words(segment).iter().rev().find_map(|w| lookup(w)));
    folder_role
        .or_else(|| file.and_then(|f| words(stem(f)).iter().rev().find_map(|w| lookup(w))))
        .unwrap_or(Role::Work)
}

fn lookup(word: &str) -> Option<Role> {
    let word = word.to_lowercase();
    TABLE
        .iter()
        .find(|(_, words)| words.contains(&word.as_str()))
        .map(|(role, _)| *role)
}

/// The beginner's word for a box of `role` named `name`. A `work` box reads "Server
/// endpoints" when the last segment of its name (without extension, any case) is one
/// of `api routes route controllers handlers`.
pub fn role_word(role: Role, name: &str) -> &'static str {
    match role {
        Role::See => "Screens",
        Role::Work => {
            let last = name.rsplit('/').next().unwrap_or("");
            let last = last.split('.').next().unwrap_or("").to_lowercase();
            if ENDPOINT_NAMES.contains(&last.as_str()) {
                "Server endpoints"
            } else {
                "Logic"
            }
        }
        Role::Keep => "Data",
        Role::Outside => "Outside services",
        Role::Shared => "Shared helpers",
    }
}

/// A file name without its last extension (`orders.controller.ts` →
/// `orders.controller`). A leading dot is part of the stem (`.env`).
pub(crate) fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// The words of a name: split at `-`, `_`, `.`, spaces and camelCase boundaries
/// (`ordersController` → `orders`, `Controller`; `HTTPServer` → `HTTP`, `Server`).
/// Words keep their spelling; empty words are dropped.
pub(crate) fn words(name: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (k, &(i, c)) in chars.iter().enumerate() {
        if matches!(c, '-' | '_' | '.' | ' ') {
            if let Some(s) = start.take() {
                out.push(&name[s..i]);
            }
            continue;
        }
        if let Some(s) = start
            && c.is_uppercase()
        {
            let prev = chars[k - 1].1;
            let next_lower = chars.get(k + 1).is_some_and(|&(_, n)| n.is_lowercase());
            let boundary =
                prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower);
            if boundary {
                out.push(&name[s..i]);
                start = Some(i);
            }
        }
        if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(&name[s..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_at_separators_and_camel_case() {
        assert_eq!(words("ordersController"), ["orders", "Controller"]);
        assert_eq!(words("HTTPServer"), ["HTTP", "Server"]);
        assert_eq!(
            words("order-service_v2.test"),
            ["order", "service", "v2", "test"]
        );
        assert_eq!(words("OrderList"), ["Order", "List"]);
        assert_eq!(words("--a"), ["a"]);
        assert!(words("").is_empty());
        assert_eq!(stem("orders.controller.ts"), "orders.controller");
        assert_eq!(stem(".env"), ".env");
        assert_eq!(stem("Makefile"), "Makefile");
    }

    #[test]
    fn role_words_match_whole_words_only() {
        assert_eq!(role_for("src/apiary/x.ts"), Role::Work);
        assert_eq!(role_for("src/viewport/x.ts"), Role::Work);
        assert_eq!(role_for("src/userModels/x.ts"), Role::Keep);
        assert_eq!(role_for("src/Components/x.ts"), Role::See);
        assert_eq!(role_for(""), Role::Work);
        assert_eq!(role_word(Role::Work, "features/routes"), "Server endpoints");
        assert_eq!(role_word(Role::Work, "API"), "Server endpoints");
        assert_eq!(role_word(Role::Work, "routes.ts"), "Server endpoints");
        assert_eq!(role_word(Role::Keep, "api"), "Data");
    }
}
