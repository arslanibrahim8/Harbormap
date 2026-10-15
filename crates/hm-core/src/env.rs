//! The secrets rule for environment variables (v0.1 spec §15).
//!
//! Harbormap keeps variable *names*. It reads exactly two kinds of values:
//! - keys matching `^(PORT|.*_PORT)$`, parsed as an integer;
//! - keys matching `^.*_(URL|URI|HOST|DSN)$`, parsed in memory to scheme, host and port.
//!
//! Userinfo, path, query and fragment are dropped before anything leaves this module.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvFacts {
    /// Every variable name, sorted and deduplicated. Never values.
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub ports: Vec<EnvPort>,
    #[serde(default)]
    pub urls: Vec<EnvUrl>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvPort {
    pub name: String,
    pub port: u16,
}

/// A URL-like value reduced to where it points.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvUrl {
    pub name: String,
    /// Empty when the value had no scheme (for example `DB_HOST=db:5432`).
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

pub fn is_port_key(name: &str) -> bool {
    name == "PORT" || name.ends_with("_PORT")
}

pub fn is_url_key(name: &str) -> bool {
    ["_URL", "_URI", "_HOST", "_DSN"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// Reduces `NAME=value` pairs to names plus the allowed facts.
pub fn extract<I, K, V>(vars: I) -> EnvFacts
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut facts = EnvFacts::default();
    for (name, value) in vars {
        let (name, value) = (name.as_ref(), value.as_ref());
        if name.is_empty() {
            continue;
        }
        facts.names.push(name.to_string());
        if is_port_key(name) {
            if let Ok(port) = value.trim().parse::<u16>() {
                facts.ports.push(EnvPort {
                    name: name.to_string(),
                    port,
                });
            }
        } else if is_url_key(name)
            && let Some((scheme, host, port)) = parse_target(value.trim())
        {
            facts.urls.push(EnvUrl {
                name: name.to_string(),
                scheme,
                host,
                port,
            });
        }
    }
    facts.names.sort();
    facts.names.dedup();
    // Sorted by name first, then by value, so repeated names cannot change the order.
    facts.ports.sort();
    facts.ports.dedup();
    facts.urls.sort();
    facts.urls.dedup();
    facts
}

/// Splits `KEY=value` entries as found in a process environment block.
pub fn extract_from_pairs<I, S>(entries: I) -> EnvFacts
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let pairs: Vec<(String, String)> = entries
        .into_iter()
        .filter_map(|entry| {
            let entry = entry.as_ref();
            // Windows keeps hidden per-drive variables such as "=C:=C:\\". Their name is
            // empty, so `extract` skips them.
            let (name, value) = entry.split_once('=')?;
            Some((name.to_string(), value.to_string()))
        })
        .collect();
    extract(pairs)
}

fn parse_target(value: &str) -> Option<(String, String, Option<u16>)> {
    if value.is_empty() {
        return None;
    }
    if value.contains("://") {
        let url = url::Url::parse(value).ok()?;
        let host = url
            .host_str()?
            .trim_start_matches('[')
            .trim_end_matches(']');
        if host.is_empty() {
            return None;
        }
        return Some((
            url.scheme().to_string(),
            host.to_string(),
            url.port_or_known_default(),
        ));
    }
    // host, host:port or [v6]:port without a scheme. The authority ends at the first
    // `/`, `?` or `#`, so an `@` in a path or query is never taken for userinfo.
    let authority = value.split(['/', '?', '#']).next()?;
    let hostport = authority
        .rsplit_once('@')
        .map_or(authority, |(_, rest)| rest);
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        host.parse::<std::net::Ipv6Addr>().ok()?;
        let port = after.strip_prefix(':').and_then(|p| p.parse().ok());
        return Some((String::new(), host.to_string(), port));
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            let port = port.parse::<u16>().ok()?;
            valid_host(host).then(|| (String::new(), host.to_string(), Some(port)))
        }
        Some(_) => None,
        None => valid_host(hostport).then(|| (String::new(), hostport.to_string(), None)),
    }
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn keeps_names_and_drops_other_values() {
        let facts = extract([
            ("SECRET_KEY", "hunter2"),
            ("PATH", "/usr/bin"),
            ("PATH", "/usr/bin"),
        ]);
        assert_eq!(facts.names, vec!["PATH", "SECRET_KEY"]);
        assert!(facts.ports.is_empty());
        assert!(facts.urls.is_empty());
    }

    #[test]
    fn reads_port_like_keys_as_integers() {
        let facts = extract([("PORT", "3000"), ("API_PORT", " 8000 "), ("DB_PORT", "x")]);
        assert_eq!(
            facts.ports,
            vec![
                EnvPort {
                    name: "API_PORT".into(),
                    port: 8000
                },
                EnvPort {
                    name: "PORT".into(),
                    port: 3000
                },
            ]
        );
    }

    #[test]
    fn reduces_url_like_values_to_scheme_host_and_port() {
        let facts = extract([
            (
                "DATABASE_URL",
                "postgres://app:s3cret@localhost:5432/shop?sslmode=disable",
            ),
            ("REDIS_URL", "redis://:pw@cache/0"),
            ("API_HOST", "api.internal:8080"),
            ("DB_HOST", "db"),
            ("BROKEN_URL", "not a url"),
            ("QUERY_URL", "localhost:5432/db?password=p@ss"),
            ("V6_HOST", "[::1]:5432"),
            ("BRACKET_HOST", "[hunter2 and more]"),
        ]);
        assert_eq!(
            facts.urls,
            vec![
                EnvUrl {
                    name: "API_HOST".into(),
                    scheme: String::new(),
                    host: "api.internal".into(),
                    port: Some(8080)
                },
                EnvUrl {
                    name: "DATABASE_URL".into(),
                    scheme: "postgres".into(),
                    host: "localhost".into(),
                    port: Some(5432)
                },
                EnvUrl {
                    name: "DB_HOST".into(),
                    scheme: String::new(),
                    host: "db".into(),
                    port: None
                },
                EnvUrl {
                    name: "QUERY_URL".into(),
                    scheme: String::new(),
                    host: "localhost".into(),
                    port: Some(5432)
                },
                EnvUrl {
                    name: "REDIS_URL".into(),
                    scheme: "redis".into(),
                    host: "cache".into(),
                    port: None
                },
                EnvUrl {
                    name: "V6_HOST".into(),
                    scheme: String::new(),
                    host: "::1".into(),
                    port: Some(5432)
                },
            ]
        );
        let json = serde_json::to_string(&facts).unwrap();
        assert!(!json.contains("s3cret"));
        assert!(!json.contains("pw"));
        assert!(!json.contains("sslmode"));
        assert!(!json.contains("p@ss"));
        assert!(!json.contains("hunter2"));
    }

    #[test]
    fn splits_environment_block_entries() {
        let facts = extract_from_pairs(["PORT=5173", "=C:=C:\\work", "EMPTY="]);
        assert_eq!(facts.names, vec!["EMPTY", "PORT"]);
        assert_eq!(facts.ports[0].port, 5173);
    }

    proptest! {
        #[test]
        fn no_secret_value_survives(
            user in "[a-z]{3,10}",
            // The digit keeps generated values from colliding with fixed JSON text.
            pass in "[A-Za-z]{3,10}[0-9][A-Za-z0-9]{4,12}",
            secret in "[A-Za-z]{3,10}[0-9][A-Za-z0-9]{4,20}",
        ) {
            let url = format!("postgres://{user}:{pass}@db.local:5432/app?token={secret}");
            // Without a scheme, and with an `@` in the query.
            let bare = format!("{user}:{pass}@db.local:5432/app?next=x@{secret}");
            let facts = extract([
                ("DATABASE_URL", url.as_str()),
                ("BARE_DSN", bare.as_str()),
                ("API_TOKEN", secret.as_str()),
                ("PASSWORD", pass.as_str()),
            ]);
            let json = serde_json::to_string(&facts).unwrap();
            prop_assert!(!json.contains(&pass));
            prop_assert!(!json.contains(&secret));
        }
    }
}
