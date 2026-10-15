//! Redaction of command lines before they are stored or shown (v0.1 spec §15).
//! Log lines use `redact_text` from alpha.3 on.

use std::sync::LazyLock;

use regex::Regex;

use crate::names::exe_stem;

pub const MASK: &str = "***";

/// Words that make a name secret when one of its parts ends with them, digits aside
/// (`DB_PASSWORD`, `PGPASSWORD`, `NGROK_AUTHTOKEN`, `client_secret`, `PASSWORD2`).
const SECRET_WORDS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "apikey",
];

/// Short words that make a name secret only as its last part (`API_KEY`, `--db-pass`,
/// `apiKey`), so that `--key-file`, `monkey` and `--pass-with-no-tests` stay readable.
const SECRET_LAST_WORDS: &[&str] = &["key", "pass", "pwd"];

/// Trailing name parts that only qualify a secret (`API_KEY_2`, `STRIPE_KEY_LIVE`).
const QUALIFIERS: &[&str] = &[
    "prod",
    "production",
    "live",
    "test",
    "dev",
    "staging",
    "local",
];

/// Programs that run the next argument as a command (`sudo mysql -p…`).
const WRAPPERS: &[&str] = &["sudo", "doas", "env", "exec", "nohup", "time"];

/// CLIs where `-pVALUE` (attached) is a password rather than a port.
const MYSQL_LIKE: &[&str] = &[
    "mysql",
    "mysqldump",
    "mysqladmin",
    "mysqlsh",
    "mysqlcheck",
    "mysqlimport",
    "mysqlshow",
    "mysqlbinlog",
    "mysqlpump",
    "mariadb",
    "mariadb-dump",
    "mariadb-admin",
    "mariadb-check",
    "mariadb-import",
    "mariadb-show",
    "mariadb-binlog",
];

/// Userinfo runs to the last `@` before the host ends, so `p@ss` is masked whole.
static URL_USERINFO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z][A-Za-z0-9+.\-]*://)[^/\s]*@").unwrap());

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=\-]+").unwrap());

static PREFIXED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(sk_live_|sk_test_|rk_live_|rk_test_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|xox[abprs]-|AKIA|ASIA|AIza)[A-Za-z0-9_\-]{8,}",
    )
    .unwrap()
});

/// A flag and the word after it, inside text such as `sh -c "app --password x"`.
static FLAG_AND_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(^|\s)(--?[A-Za-z][A-Za-z0-9_.\-]*)(\s+)("[^"]*"|'[^']*'|[^\s\-"']\S*)"#).unwrap()
});

/// `mysql … -pVALUE` inside text, where `mysql` is a command: first, after `;`, `&`, `|`,
/// `(` or a backtick, after a wrapper such as `sudo`, or as a path. `--name mysql -p3306`
/// is a port mapping, not a password.
static MYSQL_PASSWORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"((?:^|[;&|(`]\s*|\b(?:{})\s+|[/\\])(?i:{})(?:\.exe)?\s(?:[^;&|\n]*?\s)?-p)('[^']*'|"[^"]*"|[^\s;&|'"]+)"#,
        WRAPPERS.join("|"),
        MYSQL_LIKE.join("|")
    ))
    .unwrap()
});

/// Runs of at least 24 token characters; base64 padding may end them. `=` inside is a
/// separator, so `--max-old-space-size=4096` is two short tokens, not one random one.
static CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_+\-]{24,}={0,2}").unwrap());

/// Redacts a full command line. The program (after wrappers such as `sudo`) decides
/// whether MySQL-style `-pVALUE` applies.
pub fn redact_argv(argv: &[String]) -> Vec<String> {
    let mysql_like = argv
        .iter()
        .map(|arg| exe_stem(arg))
        .find(|stem| !WRAPPERS.contains(&stem.as_str()))
        .is_some_and(|stem| MYSQL_LIKE.contains(&stem.as_str()));
    let mut out = Vec::with_capacity(argv.len());
    let mut mask_next = false;
    for (index, arg) in argv.iter().enumerate() {
        if mask_next {
            mask_next = false;
            // A flag right after a secret flag means that flag was a switch.
            if !arg.starts_with('-') {
                out.push(MASK.to_string());
                continue;
            }
        }
        if index > 0 {
            if mysql_like && arg.len() > 2 && arg.starts_with("-p") {
                out.push(format!("-p{MASK}"));
                continue;
            }
            if arg.starts_with('-') && !arg.contains('=') && is_secret_flag(arg) {
                out.push(arg.clone());
                mask_next = true;
                continue;
            }
            // A bare `name=value` argument is one value: everything after the first `=`
            // is secret. Anything else (a URL, a shell command) is text.
            if let Some((name, _)) = arg.split_once('=')
                && !name.is_empty()
                && name.bytes().all(is_name_byte)
                && is_secret_name(name)
            {
                out.push(format!("{name}={MASK}"));
                continue;
            }
        }
        out.push(redact_text(arg));
    }
    out
}

/// Redacts secrets inside free text: URL userinfo, bearer tokens, known key prefixes,
/// values named by a secret name (`name=value`, `name: value`, `"name":"value"`,
/// `--name value`), MySQL `-pVALUE` and long high-entropy strings.
pub fn redact_text(text: &str) -> String {
    let text = URL_USERINFO.replace_all(text, format!("${{1}}{MASK}@"));
    let text = BEARER.replace_all(&text, format!("${{1}} {MASK}"));
    let text = PREFIXED.replace_all(&text, format!("${{1}}{MASK}"));
    let text = mask_named_values(&text);
    let text = FLAG_AND_VALUE.replace_all(&text, |caps: &regex::Captures<'_>| {
        if is_secret_flag(&caps[2]) {
            format!("{}{}{}{MASK}", &caps[1], &caps[2], &caps[3])
        } else {
            caps[0].to_string()
        }
    });
    let text = MYSQL_PASSWORD.replace_all(&text, format!("${{1}}{MASK}"));
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

/// True when a variable or option name names a secret: one of its parts (split at `-`,
/// `_` and `.`) ends with a secret word, or its last part, qualifiers aside, is a short
/// one (`DB_PASSWORD`, `-Dpassword`, `NGROK_AUTHTOKEN`, `API_KEY_2`, `accessToken`).
fn is_secret_name(name: &str) -> bool {
    let parts = secret_parts(name);
    parts.iter().any(|part| ends_with_secret_word(part))
        || parts.last().is_some_and(|last| is_short_secret_word(last))
}

/// True when the word after a flag is a secret: the flag's last part names it
/// (`--password`, `--client-secret`, `--api-key`). `--password-file` and `--token-limit`
/// keep their values, and `--no-password` and `--ask-pass` are switches.
fn is_secret_flag(flag: &str) -> bool {
    let parts = secret_parts(flag);
    if parts
        .first()
        .is_some_and(|first| first.eq_ignore_ascii_case("no") || first.eq_ignore_ascii_case("ask"))
    {
        return false;
    }
    parts
        .last()
        .is_some_and(|last| ends_with_secret_word(last) || is_short_secret_word(last))
}

/// The parts of a name without leading dashes and trailing qualifiers.
fn secret_parts(name: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = name
        .trim_start_matches('-')
        .split(['-', '_', '.'])
        .filter(|part| !part.is_empty())
        .collect();
    while parts.len() > 1 && parts.last().is_some_and(|part| is_qualifier(part)) {
        parts.pop();
    }
    parts
}

/// `2`, `v2`, `PROD`, `live`.
fn is_qualifier(part: &str) -> bool {
    let version = part
        .strip_prefix(['v', 'V'])
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()));
    part.bytes().all(|byte| byte.is_ascii_digit())
        || version
        || QUALIFIERS
            .iter()
            .any(|word| part.eq_ignore_ascii_case(word))
}

/// `PGPASSWORD`, `AUTHTOKEN`, `accessToken`, `PASSWORD2`; not `tokens` or `tokenizer`.
fn ends_with_secret_word(part: &str) -> bool {
    let lower = part
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .to_ascii_lowercase();
    SECRET_WORDS.iter().any(|word| lower.ends_with(word))
}

/// `KEY`, `pass`, `pwd`, or a camelCase part ending in one (`apiKey`, `dbPass`).
fn is_short_secret_word(part: &str) -> bool {
    let word = last_camel_word(part).to_ascii_lowercase();
    SECRET_LAST_WORDS.contains(&word.as_str())
}

/// The last word of a camelCase part (`apiKey` → `Key`), or the whole part.
fn last_camel_word(part: &str) -> &str {
    match part.rfind(|c: char| c.is_ascii_uppercase()) {
        Some(index)
            if index > 0
                && part[..index]
                    .ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) =>
        {
            &part[index..]
        }
        _ => part,
    }
}

/// Masks the value of every `name=value`, `name: value` and `"name":"value"` pair whose
/// name is a secret name. It looks at every separator in turn, so a pair inside the value
/// of a harmless pair is still found.
fn mask_named_values(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'=' | b':')
            && let Some((start, end)) = secret_value_after(text, index)
        {
            out.push_str(&text[copied..start]);
            out.push_str(MASK);
            copied = end;
            index = end;
            continue;
        }
        index += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// For the separator at `separator`, the byte range of the value to mask when the name
/// before it is a secret name. A `:` separates a pair only before whitespace or a quote,
/// so `cache-key:6379` stays a host and port.
fn secret_value_after(text: &str, separator: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut name_end = separator;
    while name_end > 0 && bytes[name_end - 1].is_ascii_whitespace() {
        name_end -= 1;
    }
    if name_end > 0 && matches!(bytes[name_end - 1], b'"' | b'\'') {
        name_end -= 1;
    }
    let mut name_start = name_end;
    while name_start > 0 && is_name_byte(bytes[name_start - 1]) {
        name_start -= 1;
    }
    if name_start == name_end || !is_secret_name(&text[name_start..name_end]) {
        return None;
    }
    let mut start = separator + 1;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    let quoted = start < bytes.len() && matches!(bytes[start], b'"' | b'\'');
    if bytes[separator] == b':' && start == separator + 1 && !quoted {
        return None;
    }
    let end = if quoted {
        let quote = bytes[start] as char;
        text[start + 1..]
            .find(quote)
            .map_or(bytes.len(), |offset| start + 1 + offset + 1)
    } else {
        text[start..]
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '&' | ';' | ',' | '"' | '\'' | '}' | ']')
            })
            .map_or(bytes.len(), |offset| start + offset)
    };
    (end > start).then_some((start, end))
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
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
            &["node", "--max-old-space-size=4096", "server.js"],
            &["chrome", "--remote-debugging-port=9222", "--headless=new"],
            &[
                "app",
                "--auth-url",
                "http://keycloak:8080/auth",
                "--cache",
                "redis://cache-key-store:6379/0",
            ],
            &["jest", "--pass-with-no-tests", "src/"],
            &["nginx", "--key-file", "/etc/tls/site.pem", "monkey=banana"],
            &["vllm", "serve", "--max-tokens", "4096"],
            &["docker", "login", "--password-stdin", "ghcr.io"],
            &["ansible-playbook", "--ask-pass", "site.yml"],
            &["psql", "--no-password", "-h", "db", "app"],
            &[
                "app",
                "--token-limit",
                "100",
                "--password-file",
                "/etc/app/pw",
            ],
            &["sh", "-c", "docker run --name mysql -p3306:3306 mysql:8"],
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
        assert_eq!(
            redact_argv(&argv(&[
                "app",
                "--client-secret",
                "hunter2",
                "-password",
                "x1"
            ])),
            argv(&["app", "--client-secret", "***", "-password", "***"])
        );
        // One argument is one value, so `;`, `&` and spaces do not end it.
        assert_eq!(
            redact_argv(&argv(&[
                "docker",
                "run",
                "-e",
                "DB_PASSWORD=pa;ss",
                "-e",
                "TOKEN=a b"
            ])),
            argv(&["docker", "run", "-e", "DB_PASSWORD=***", "-e", "TOKEN=***"])
        );
        // A whole command line inside one argument, as `sh -c` runs it.
        assert_eq!(
            redact_argv(&argv(&["sh", "-c", "app --password hunter2 --port 80"])),
            argv(&["sh", "-c", "app --password *** --port 80"])
        );
        // Secret words inside longer names, and qualifiers after them.
        assert_eq!(
            redact_argv(&argv(&[
                "env",
                "PGPASSWORD=a1",
                "NGROK_AUTHTOKEN=a2",
                "API_KEY_2=a3",
                "OPENAI_API_KEY_PROD=a4",
                "PASSWORD2=a5",
                "-Dpassword=a6",
            ])),
            argv(&[
                "env",
                "PGPASSWORD=***",
                "NGROK_AUTHTOKEN=***",
                "API_KEY_2=***",
                "OPENAI_API_KEY_PROD=***",
                "PASSWORD2=***",
                "-Dpassword=***",
            ])
        );
        // An argument that is not a bare `name=value` keeps its text around the secret.
        assert_eq!(
            redact_argv(&argv(&[
                "curl",
                "https://u:hunter2@host.example/x?api_key=abc"
            ])),
            argv(&["curl", "https://***@host.example/x?api_key=***"])
        );
        assert_eq!(
            redact_argv(&argv(&[
                "sh",
                "-c",
                "export API_TOKEN=abc && exec node server.js --port 80",
            ])),
            argv(&[
                "sh",
                "-c",
                "export API_TOKEN=*** && exec node server.js --port 80",
            ])
        );
    }

    #[test]
    fn masks_attached_mysql_passwords_only_for_mysql_like_tools() {
        assert_eq!(
            redact_argv(&argv(&["mysql", "-u", "root", "-phunter2", "shop"])),
            argv(&["mysql", "-u", "root", "-p***", "shop"])
        );
        assert_eq!(
            redact_argv(&argv(&["mariadb-admin", "-phunter2", "status"])),
            argv(&["mariadb-admin", "-p***", "status"])
        );
        assert_eq!(
            redact_argv(&argv(&["psql", "-p5432"])),
            argv(&["psql", "-p5432"])
        );
        assert_eq!(
            redact_argv(&argv(&["bash", "-lc", "mysql -u root -phunter2 shop"])),
            argv(&["bash", "-lc", "mysql -u root -p*** shop"])
        );
        // The password itself may look like a secret name; `-p` still wins.
        assert_eq!(
            redact_argv(&argv(&["mysql", "-uroot", "-pmy_secret", "shop"])),
            argv(&["mysql", "-uroot", "-p***", "shop"])
        );
        assert_eq!(
            redact_argv(&argv(&["sudo", "mysql", "-phunter2"])),
            argv(&["sudo", "mysql", "-p***"])
        );
        assert_eq!(
            redact_text("cd /srv && mysql -u root -p'hunter 2' shop"),
            "cd /srv && mysql -u root -p*** shop"
        );
    }

    #[test]
    fn masks_secrets_inside_text() {
        assert_eq!(
            redact_text("postgres://app:s3cret@db:5432/shop"),
            "postgres://***@db:5432/shop"
        );
        assert_eq!(
            redact_text("postgres://user:p@ss@db:5432/shop"),
            "postgres://***@db:5432/shop"
        );
        assert_eq!(
            redact_text("Authorization: Bearer abc.def.ghi"),
            "Authorization: Bearer ***"
        );
        assert_eq!(redact_text("X-Api-Key: abc123"), "X-Api-Key: ***");
        assert_eq!(
            redact_text("-Dspring.datasource.password=letmein"),
            "-Dspring.datasource.password=***"
        );
        assert_eq!(redact_text(r#"{"password":"x1"}"#), r#"{"password":***}"#);
        assert_eq!(
            redact_text(&redact_text(r#"{"password":"x1"}"#)),
            r#"{"password":***}"#
        );
        assert_eq!(redact_text(r#"{"apiKey": "k1"}"#), r#"{"apiKey": ***}"#);
        assert_eq!(
            redact_text(&redact_text(r#"{"password": "x1"}"#)),
            r#"{"password": ***}"#
        );
        assert_eq!(
            redact_text("url=http://x/?token=abc&page=2"),
            "url=http://x/?token=***&page=2"
        );
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
        fn url_passwords_never_survive(user in "[a-z]{3,8}", pass in "[A-Za-z0-9@]{6,20}") {
            let out = redact_text(&format!("postgres://{user}:{pass}@db:5432/app"));
            prop_assert_eq!(out, "postgres://***@db:5432/app");
        }

        #[test]
        fn secret_named_values_never_survive(
            name in prop::sample::select(vec!["token", "API_KEY", "client_secret", "DB_PASSWORD", "pwd"]),
            value in "[A-Za-z0-9]{6,30}",
        ) {
            prop_assert_eq!(redact_text(&format!("{name}={value}")), format!("{name}={MASK}"));
            prop_assert_eq!(
                redact_text(&format!("app --{name} {value} --port 80")),
                format!("app --{name} {MASK} --port 80")
            );
            prop_assert_eq!(redact_text(&format!("{name}: {value}")), format!("{name}: {MASK}"));
        }

        #[test]
        fn random_strings_never_survive(secret in "[A-Za-z0-9]{24,64}") {
            prop_assume!(looks_random(&secret));
            let out = redact_text(&format!("--x {secret} --y"));
            prop_assert!(!out.contains(&secret));
        }

        #[test]
        fn secret_flags_never_leak(value in "[A-Za-z]{2,10}[0-9][!@#%^&*;,][A-Za-z0-9]{1,12}") {
            let spaced = redact_argv(&argv(&["app", "--password", &value]));
            let joined = redact_argv(&argv(&["app", &format!("--token={value}")]));
            let named = redact_argv(&argv(&["app", &format!("DB_PASSWORD={value}")]));
            let mysql = redact_argv(&argv(&["mysql", &format!("-p{value}")]));
            for out in [spaced, joined, named, mysql] {
                prop_assert!(!out.iter().any(|arg| arg.contains(&value)));
            }
        }
    }
}
