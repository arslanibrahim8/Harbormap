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
        &[("Cookie", &format!("hm_learn={wrong}"))],
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
        &[("Cookie", &format!("hm_learn={}", &live.token[..31]))],
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
        Some(format!("hm_learn={}; HttpOnly; SameSite=Strict; Path=/", live.token).as_str())
    );
    // The token in the query is only taken on `/`.
    let api = request(live.port, &live.token, "/api/tech", Auth::Query, &[]);
    assert_eq!(api.status, 401);
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
    let raw = |host: &str| {
        let mut stream = TcpStream::connect(("127.0.0.1", live.port)).expect("connect");
        let request = format!(
            "GET /api/tech HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\n\
             Connection: close\r\n\r\n",
            live.token
        );
        stream.write_all(request.as_bytes()).expect("send");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("read");
        answer.lines().next().unwrap_or("").to_string()
    };
    assert!(raw("evil.example").contains(" 403 "));
    assert!(raw(&format!("evil.example:{}", live.port)).contains(" 403 "));
    // The right token does not excuse a foreign Host, and the wrong port is foreign.
    assert!(raw("127.0.0.1:1").contains(" 403 "));
    assert!(raw(&format!("localhost:{}", live.port)).contains(" 200 "));
    assert!(raw(&format!("127.0.0.1:{}", live.port)).contains(" 200 "));
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
