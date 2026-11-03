//! The token and Host checks in front of every request.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;

use common::{Auth, get, request, shop, start};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    shop(dir.path());
    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_token_is_401() {
    let dir = project();
    let live = start(dir.path()).await;
    for path in ["/", "/api/search?q=a", "/api/events", "/assets/app.js"] {
        assert_eq!(get(&live, path, Auth::None).0, 401, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_token_is_401() {
    let dir = project();
    let live = start(dir.path()).await;
    let wrong = "0".repeat(32);
    let cookie = request(
        live.port,
        &live.token,
        "/api/tech",
        Auth::None,
        &[("Cookie", &format!("hm_learn_{}={wrong}", live.port))],
    );
    assert_eq!(cookie.status, 401);
    let bearer = request(
        live.port,
        &live.token,
        "/api/tech",
        Auth::None,
        &[("Authorization", &format!("Bearer {wrong}"))],
    );
    assert_eq!(bearer.status, 401);
    let query = request(live.port, &wrong, "/", Auth::Query, &[]);
    assert_eq!(query.status, 401);
    // A prefix of the token is not the token.
    let short = request(
        live.port,
        &live.token,
        "/api/tech",
        Auth::None,
        &[(
            "Cookie",
            &format!("hm_learn_{}={}", live.port, &live.token[..31]),
        )],
    );
    assert_eq!(short.status, 401);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_query_token_sets_a_strict_http_only_cookie_and_redirects() {
    let dir = project();
    let live = start(dir.path()).await;
    let reply = request(live.port, &live.token, "/", Auth::Query, &[]);
    assert_eq!(reply.status, 303);
    assert_eq!(reply.header("location"), Some("/"));
    assert_eq!(
        reply.header("set-cookie"),
        Some(
            format!(
                "hm_learn_{}={}; HttpOnly; SameSite=Strict; Path=/",
                live.port, live.token
            )
            .as_str()
        )
    );
    // The token in the query is only taken on `/`.
    let api = request(live.port, &live.token, "/api/tech", Auth::Query, &[]);
    assert_eq!(api.status, 401);
}

/// Cookies ignore the port, so two panels on 127.0.0.1 share one jar. Each keeps its
/// own cookie name; the second panel's link must not log the first one out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_servers_cookies_coexist_in_one_jar() {
    let dir = project();
    let a = start(dir.path()).await;
    let b = start(dir.path()).await;
    assert_ne!(a.port, b.port);
    // A browser jar keeps one value per cookie name, whatever the port: a later
    // `Set-Cookie` with the same name replaces the earlier one.
    let mut jar = std::collections::BTreeMap::new();
    for live in [&a, &b] {
        let reply = request(live.port, &live.token, "/", Auth::Query, &[]);
        let set = reply.header("set-cookie").expect("a cookie");
        let (name, value) = set
            .split(';')
            .next()
            .and_then(|pair| pair.split_once('='))
            .expect("a name=value pair");
        jar.insert(name.to_string(), value.to_string());
    }
    let jar = jar
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    for live in [&a, &b] {
        let reply = request(
            live.port,
            &live.token,
            "/api/tech",
            Auth::None,
            &[("Cookie", &jar)],
        );
        assert_eq!(reply.status, 200, "port {}", live.port);
    }
    // A cookie named for the other server does not open this one.
    let swapped = request(
        a.port,
        &a.token,
        "/api/tech",
        Auth::None,
        &[("Cookie", &format!("hm_learn_{}={}", b.port, a.token))],
    );
    assert_eq!(swapped.status, 401);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cookie_and_the_bearer_token_both_work() {
    let dir = project();
    let live = start(dir.path()).await;
    for auth in [Auth::Cookie, Auth::Bearer] {
        assert_eq!(get(&live, "/api/tech", auth).0, 200);
        assert_eq!(get(&live, "/", auth).0, 200);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_foreign_host_header_is_403() {
    let dir = project();
    let live = start(dir.path()).await;
    // A raw request with a chosen Host; returns the status line and the whole answer.
    let raw = |path: &str, host: &str, auth: Option<&str>| {
        let mut stream = TcpStream::connect(("127.0.0.1", live.port)).expect("connect");
        let auth = auth
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{auth}Connection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("send");
        let mut answer = String::new();
        // An event stream would never end; its answer is cut after the headers.
        let mut buf = [0u8; 4096];
        while !answer.contains("\r\n\r\n") {
            let n = stream.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            answer.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        let status = answer.lines().next().unwrap_or("").to_string();
        (status, answer)
    };
    let token = live.token.clone();
    let foreign = [
        "evil.example",
        &format!("evil.example:{}", live.port),
        "127.0.0.1:1",
    ];
    let paths = [
        "/".to_string(),
        "/assets/app.js".to_string(),
        "/api/events".to_string(),
        "/api/tech".to_string(),
        // The token in the address is not a way around the Host check: no cookie, no 303.
        format!("/?t={token}"),
    ];
    for host in foreign {
        for path in &paths {
            // With the right token, and without a token at all: 403 either way.
            for auth in [Some(token.as_str()), None] {
                let (status, answer) = raw(path, host, auth);
                assert!(status.contains(" 403 "), "{host} {path} {auth:?}: {status}");
                assert!(!answer.to_ascii_lowercase().contains("set-cookie"));
            }
        }
    }
    // A request without any Host header is not served either.
    let mut stream = TcpStream::connect(("127.0.0.1", live.port)).expect("connect");
    stream
        .write_all(b"GET /api/tech HTTP/1.0\r\n\r\n")
        .expect("send");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("read");
    assert!(answer.contains(" 403 "), "{answer}");

    // The real hosts work.
    for host in [
        format!("localhost:{}", live.port),
        format!("127.0.0.1:{}", live.port),
    ] {
        assert!(raw("/api/tech", &host, Some(&token)).0.contains(" 200 "));
        assert!(
            raw(&format!("/?t={token}"), &host, None)
                .0
                .contains(" 303 ")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_server_listens_on_loopback_only() {
    let dir = project();
    let live = start(dir.path()).await;
    assert!(live.handle.addr().ip().is_loopback());

    let db_dir = tempfile::tempdir().expect("create a temporary directory");
    let model = hm_serve::Model::load(
        &hm_store::Store::open_in_memory().expect("open a store"),
        "shop",
        None,
    )
    .expect("load a model");
    let config = hm_serve::ServeConfig {
        root: dir.path().to_path_buf(),
        db: db_dir.path().join("facts.db"),
        repo: hm_serve::Repo {
            name: "shop".into(),
            branch: None,
            commit: None,
        },
        token: None,
    };
    let server = hm_serve::Server::bind(config, model).await.expect("bind");
    assert!(server.url().starts_with("http://127.0.0.1:"));
    assert_eq!(
        server.url(),
        format!("http://127.0.0.1:{}/?t={}", server.port(), server.token())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tokens_are_32_hex_and_differ_per_run() {
    let dir = project();
    let a = start(dir.path()).await;
    let b = start(dir.path()).await;
    for token in [&a.token, &b.token] {
        assert_eq!(token.len(), 32);
        assert!(
            token
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        );
    }
    assert_ne!(a.token, b.token);
}
