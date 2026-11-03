//! How package, pattern and image names are compared.

use crate::model::Ecosystem;

const NODE_BUILTINS: &[&str] = &[
    "assert",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

/// The comparable form of a package name or pattern (a trailing `*` is kept).
pub fn normalize(ecosystem: Ecosystem, name: &str) -> String {
    match ecosystem {
        Ecosystem::Npm | Ecosystem::Nuget => name.to_lowercase(),
        Ecosystem::Pypi => {
            // PEP 503: every run of `-`, `_` and `.` is one `-`.
            let mut out = String::with_capacity(name.len());
            let mut in_separator = false;
            for c in name.to_lowercase().chars() {
                if matches!(c, '-' | '_' | '.') {
                    if !in_separator {
                        out.push('-');
                    }
                    in_separator = true;
                } else {
                    out.push(c);
                    in_separator = false;
                }
            }
            out
        }
        Ecosystem::Go | Ecosystem::Maven => name.to_string(),
        Ecosystem::Crates => name.to_lowercase().replace('_', "-"),
    }
}

/// The comparable form of a Docker image reference: no registry default, tag or digest.
pub fn normalize_image(image: &str) -> String {
    let mut name = image.to_lowercase();
    if let Some(at) = name.find('@') {
        name.truncate(at);
    }
    if let Some(colon) = name.rfind(':')
        && name.rfind('/').is_none_or(|slash| colon > slash)
    {
        name.truncate(colon);
    }
    let name = ["index.docker.io/", "docker.io/"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(&name);
    name.strip_prefix("library/").unwrap_or(name).to_string()
}

/// Whether an import comes with the runtime or standard library, not from a package.
pub fn is_builtin(ecosystem: Ecosystem, name: &str) -> bool {
    match ecosystem {
        Ecosystem::Npm => {
            name.starts_with("node:") || name.starts_with("bun:") || NODE_BUILTINS.contains(&name)
        }
        Ecosystem::Go => {
            !name.is_empty()
                && !name
                    .split('/')
                    .next()
                    .is_some_and(|first| first.contains('.'))
        }
        _ => false,
    }
}

/// Whether an import string can name a registry package at all (not a relative path,
/// a path alias or a protocol import).
pub fn is_package_specifier(ecosystem: Ecosystem, name: &str) -> bool {
    match ecosystem {
        Ecosystem::Npm => {
            !(name.is_empty()
                || name.starts_with(['.', '/', '~', '#'])
                || name.starts_with("@/")
                || (name.starts_with('@') && !name.contains('/'))
                || name.contains(':'))
        }
        _ => !name.is_empty(),
    }
}
