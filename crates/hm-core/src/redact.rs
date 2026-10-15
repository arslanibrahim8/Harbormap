//! Redaction of command lines before they are stored or shown (v0.1 spec §15).
//! Log lines use `redact_text` from alpha.3 on.

use std::sync::LazyLock;

use regex::Regex;

use crate::names::exe_stem;

pub const MASK: &str = "***";

/// Flags whose value is the next argument (or follows `=`).
const SECRET_FLAGS: &[&str] = &[
    "--password",
    "--passwd",
    "--pass",
    "--token",
    "--auth-token",
    "--secret",
    "--api-key",
    "--apikey",
    "--access-key",
    "--secret-key",
];

/// CLIs where `-pVALUE` (attached) is a password rather than a port.
const MYSQL_LIKE: &[&str] = &[
    "mysql",
    "mysqldump",
    "mysqladmin",
    "mysqlsh",
    "mariadb",
    "mariadb-dump",
];

static URL_USERINFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z][A-Za-z0-9+.\-]*://)[^/@\s]+@").unwrap());

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=\-]+").unwrap());

static PREFIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(sk_live_|sk_test_|rk_live_|rk_test_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|xox[abprs]-|AKIA|ASIA|AIza)[A-Za-z0-9_\-]{8,}",
    )
    .unwrap()
});

static KEY_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)([A-Za-z0-9_.\-]*(?:token|secret|password|passwd|pwd|key)[A-Za-z0-9_.\-]*"?\s*[=:]\s*)("[^"]*"|'[^']*'|[^\s&;,"']+)"#,
    )
    .unwrap()
});

static CANDIDATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_+=\-]{24,}").unwrap());

/// Redacts a full command line. `argv[0]` decides whether MySQL-style `-pVALUE` applies.
pub fn redact_argv(argv: &[String]) -> Vec<String> {
    let mysql_like = argv
        .first()
        .is_some_and(|first| MYSQL_LIKE.contains(&exe_stem(first).as_str()));
    let mut out = Vec::with_capacity(argv.len());
    let mut mask_next = false;
    for (index, arg) in argv.iter().enumerate() {
        if mask_next {
            out.push(MASK.to_string());
            mask_next = false;
            continue;
        }
        if index > 0 {
            let lower = arg.to_ascii_lowercase();
            if SECRET_FLAGS.contains(&lower.as_str()) {
                out.push(arg.clone());
                mask_next = true;
                continue;
            }
            if let Some((flag, _)) = arg.split_once('=')
                && SECRET_FLAGS.contains(&flag.to_ascii_lowercase().as_str())
            {
                out.push(format!("{flag}={MASK}"));
                continue;
            }
            if mysql_like && arg.len() > 2 && arg.starts_with("-p") {
                out.push(format!("-p{MASK}"));
                continue;
            }
        }
        out.push(redact_text(arg));
    }
    out
}

/// Redacts secrets inside free text: URL userinfo, bearer tokens, `key=value` pairs
/// whose key names a secret, known key prefixes and long high-entropy strings.
pub fn redact_text(text: &str) -> String {
    let text = URL_USERINFO.replace_all(text, format!("${{1}}{MASK}@"));
    let text = BEARER.replace_all(&text, format!("${{1}} {MASK}"));
    let text = PREFIXED.replace_all(&text, format!("${{1}}{MASK}"));
    let text = KEY_VALUE.replace_all(&text, format!("${{1}}{MASK}"));
    CANDIDATE
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            let candidate = &caps[0];
            if looks_random(candidate) {
                MASK.to_string()
            } else {
                candidate.to_string()
            }
        })
        .into_owned()
}

/// At least 24 characters (guaranteed by `CANDIDATE`), letters and digits mixed,
/// and at least 3.5 bits of Shannon entropy per character.
fn looks_random(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && shannon_entropy(s) >= 3.5
}

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for byte in s.bytes() {
        counts[byte as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn leaves_ordinary_command_lines_alone() {
        for parts in [
            &[
                "node",
                "/Users/me/shop/apps/web/node_modules/.bin/vite",
                "--port",
                "5173",
            ][..],
            &[
                "java",
                "-cp",
                "app.jar",
                "org.springframework.boot.loader.launch.JarLauncher",
            ],
            &[
                "python3", "-m", "uvicorn", "main:app", "--reload", "--port", "8000",
            ],
            &["psql", "-h", "localhost", "-p", "5432", "-U", "app"],
            &["/var/folders/zz/T/go-build3456789012/b001/exe/main"],
        ] {
            let input = argv(parts);
            assert_eq!(redact_argv(&input), input);
        }
    }

    #[test]
    fn masks_secret_flags_and_their_values() {
        assert_eq!(
            redact_argv(&argv(&["app", "--password", "hunter2", "--port", "80"])),
            argv(&["app", "--password", "***", "--port", "80"])
        );
        assert_eq!(
            redact_argv(&argv(&["app", "--api-key=abc123"])),
            argv(&["app", "--api-key=***"])
        );
    }

    #[test]
    fn masks_attached_mysql_passwords_only_for_mysql_like_tools() {
        assert_eq!(
            redact_argv(&argv(&["mysql", "-u", "root", "-phunter2", "shop"])),
            argv(&["mysql", "-u", "root", "-p***", "shop"])
        );
        assert_eq!(
            redact_argv(&argv(&["psql", "-p5432"])),
            argv(&["psql", "-p5432"])
        );
    }

    #[test]
    fn masks_secrets_inside_text() {
        assert_eq!(
            redact_text("postgres://app:s3cret@db:5432/shop"),
            "postgres://***@db:5432/shop"
        );
        assert_eq!(
            redact_text("Authorization: Bearer abc.def.ghi"),
            "Authorization: Bearer ***"
        );
        assert_eq!(
            redact_text("-Dspring.datasource.password=letmein"),
            "-Dspring.datasource.password=***"
        );
        assert_eq!(redact_text(r#"{"password":"x1"}"#), r#"{"password":***}"#);
        assert_eq!(redact_text("key ghp_aBcDeFgHiJkLmNoPqRsTuV"), "key ghp_***");
        assert_eq!(
            redact_text("--session Xk2pQ9vR7tLm3nB8cD4fG6hJ1sA5"),
            "--session ***"
        );
    }

    proptest! {
        #[test]
        fn prefixed_keys_never_survive(
            prefix in prop::sample::select(vec!["sk_live_", "sk_test_", "ghp_", "gho_", "xoxb-", "xoxp-", "AKIA", "AIza"]),
            suffix in "[A-Za-z0-9]{16,40}",
        ) {
            let out = redact_text(&format!("run --flag {prefix}{suffix} now"));
            prop_assert!(!out.contains(&suffix));
        }

        #[test]
        fn url_passwords_never_survive(user in "[a-z]{3,8}", pass in "[A-Za-z0-9]{6,20}") {
            let out = redact_text(&format!("postgres://{user}:{pass}@db:5432/app"));
            let needle = format!(":{pass}@");
            prop_assert!(!out.contains(&needle));
        }

        #[test]
        fn secret_named_values_never_survive(
            name in prop::sample::select(vec!["token", "API_KEY", "client_secret", "DB_PASSWORD", "pwd"]),
            value in "[A-Za-z0-9]{6,30}",
        ) {
            let out = redact_text(&format!("{name}={value}"));
            let needle = format!("={value}");
            prop_assert!(!out.contains(&needle));
        }

        #[test]
        fn random_strings_never_survive(secret in "[A-Za-z0-9]{24,64}") {
            prop_assume!(looks_random(&secret));
            let out = redact_text(&format!("--x {secret} --y"));
            prop_assert!(!out.contains(&secret));
        }

        #[test]
        fn secret_flags_never_leak(value in "[A-Za-z]{2,10}[0-9][!@#%^&*][A-Za-z0-9]{1,12}") {
            let spaced = redact_argv(&argv(&["app", "--password", &value]));
            let joined = redact_argv(&argv(&["app", &format!("--token={value}")]));
            let mysql = redact_argv(&argv(&["mysql", &format!("-p{value}")]));
            for out in [spaced, joined, mysql] {
                prop_assert!(!out.iter().any(|arg| arg.contains(&value)));
            }
        }
    }
}
