//! The JSON endpoints, the source view and the embedded panel.

mod common;

use std::io::Read;

use common::{Auth, encode, get_json, request, shop, start, write};
use flate2::read::GzDecoder;
use hm_index::index::Indexer;
use hm_store::Store;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    shop(dir.path());
    dir
}

fn names(card: &serde_json::Value, key: &str) -> Vec<String> {
    card[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is a list"))
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_file_card_lists_jobs() {
    let dir = project();
    let live = start(dir.path()).await;
    let (status, body) = get_json(&live, "/api/card?path=src/orders/service.ts");
    assert_eq!(status, 200);
    assert_eq!(body["kind"], "file");
    assert_eq!(body["seq"], 0);
    assert!(body["links"].is_object(), "service.ts is on the map");
    let card = &body["card"];
    assert_eq!(card["path"], "src/orders/service.ts");
    assert_eq!(card["lang"], "typescript");
    let symbols: Vec<&str> = card["symbols"]
        .as_array()
        .expect("symbols")
        .iter()
        .map(|s| s["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(symbols, ["placeOrder", "history"]);
    assert!(names(card, "uses").contains(&"src/orders/repo.ts".to_string()));
    assert!(names(card, "used_by").contains(&"src/app/page.tsx".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_symbol_card_names_callers() {
    let dir = project();
    let live = start(dir.path()).await;
    let (status, body) = get_json(&live, "/api/card?path=src/orders/repo.ts&symbol=saveOrder");
    assert_eq!(status, 200);
    assert_eq!(body["kind"], "symbol");
    assert_eq!(body["card"]["name"], "saveOrder");
    let callers = names(&body["card"], "callers");
    assert!(
        callers.iter().any(|c| c.contains("placeOrder")),
        "{callers:?}"
    );
    let (status, _) = get_json(&live, "/api/card?path=src/orders/repo.ts&symbol=nope");
    assert_eq!(status, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_file_card_is_404() {
    let dir = project();
    let live = start(dir.path()).await;
    assert_eq!(get_json(&live, "/api/card?path=src/db/client.ts").0, 200);

    std::fs::remove_file(dir.path().join("src/db/client.ts")).expect("remove a file");
    let store = Store::open(&live.db).expect("open the store");
    Indexer::open(dir.path(), store)
        .expect("open the indexer")
        .full()
        .expect("index again");

    assert_eq!(get_json(&live, "/api/card?path=src/db/client.ts").0, 404);
    assert_eq!(get_json(&live, "/api/file?path=src/db/client.ts").0, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dot_dot_and_absolute_paths_are_400() {
    let dir = project();
    let live = start(dir.path()).await;
    let bad = [
        "",
        "/etc/passwd",
        "..",
        "../outside.ts",
        "src/../src/orders/repo.ts",
        "./src/orders/repo.ts",
        "src/./orders/repo.ts",
        "src%5Corders%5Crepo.ts",
        "src%00",
        "C:/Windows/win.ini",
    ];
    for path in bad {
        for endpoint in ["/api/card", "/api/file"] {
            let (status, body) = get_json(&live, &format!("{endpoint}?path={path}"));
            assert_eq!(status, 400, "{endpoint} {path:?}");
            assert_eq!(body["v"], 1);
        }
    }
    // A missing `path` is a bad request too.
    assert_eq!(get_json(&live, "/api/file").0, 400);
    assert_eq!(get_json(&live, "/api/card").0, 400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_outside_the_index_is_404() {
    let dir = project();
    write(dir.path(), ".gitignore", "secret.ts\n");
    write(dir.path(), "secret.ts", "export const key = 1;\n");
    write(dir.path(), ".git/config", "[core]\n");
    let live = start(dir.path()).await;
    for path in [".git/config", "secret.ts", "src/missing.ts", "src"] {
        let (status, _) = get_json(&live, &format!("/api/file?path={path}"));
        assert_eq!(status, 404, "{path}");
        let (status, _) = get_json(&live, &format!("/api/card?path={path}"));
        assert_eq!(status, 404, "{path}");
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_symlink_leaving_the_project_is_refused() {
    let dir = project();
    let outside = tempfile::tempdir().expect("create a temporary directory");
    write(outside.path(), "secret.txt", "do not serve\n");
    let live = start(dir.path()).await;

    // The file is indexed; then it is swapped for a link that leaves the project.
    let target = dir.path().join("src/db/client.ts");
    std::fs::remove_file(&target).expect("remove the file");
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), &target).expect("link");

    let (status, body) = get_json(&live, "/api/file?path=src/db/client.ts");
    assert_eq!(status, 403);
    assert!(!body.to_string().contains("do not serve"));

    // A link that stays inside the project is fine.
    std::fs::remove_file(&target).expect("remove the link");
    std::os::unix::fs::symlink(dir.path().join("src/orders/repo.ts"), &target).expect("link");
    let (status, body) = get_json(&live, "/api/file?path=src/db/client.ts");
    assert_eq!(status, 200);
    assert!(body["text"].as_str().expect("text").contains("saveOrder"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_file_is_cut_at_a_line_end() {
    let dir = project();
    // 600 KiB of 63-byte lines, so the 512 KiB mark falls inside a line.
    let line = format!("// {}\n", "x".repeat(59));
    assert_eq!(line.len(), 63);
    write(dir.path(), "src/big.ts", &line.repeat(600 * 1024 / 63));
    let live = start(dir.path()).await;

    let (status, body) = get_json(&live, "/api/file?path=src/big.ts");
    assert_eq!(status, 200);
    assert_eq!(body["truncated"], true);
    let text = body["text"].as_str().expect("text");
    assert!(text.len() <= 512 * 1024);
    assert!(text.ends_with('\n'));
    assert_eq!(body["lines"].as_u64(), Some(text.lines().count() as u64));
    assert_eq!(body["lines"], 512 * 1024 / 63);

    // A file that fits is whole and not marked.
    let (_, small) = get_json(&live, "/api/file?path=src/orders/repo.ts");
    assert_eq!(small["truncated"], false);
    assert_eq!(small["lines"], 3);
    assert_eq!(small["lang"], "typescript");
    assert_eq!(small["path"], "src/orders/repo.ts");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_utf8_is_replaced_not_refused() {
    let dir = project();
    let mut bytes = b"export const name = '".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe, b'a']);
    bytes.extend_from_slice(b"';\n");
    std::fs::write(dir.path().join("src/bad.ts"), bytes).expect("write a file");
    let live = start(dir.path()).await;

    let (status, body) = get_json(&live, "/api/file?path=src/bad.ts");
    assert_eq!(status, 200);
    let text = body["text"].as_str().expect("text");
    assert!(text.contains("\u{fffd}\u{fffd}a"), "{text:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_path_with_spaces_hash_and_unicode_round_trips() {
    let dir = project();
    let rel = "src/çay #2.ts";
    write(dir.path(), rel, "export function demle() { return 1; }\n");
    let live = start(dir.path()).await;

    let (status, body) = get_json(&live, &format!("/api/file?path={}", encode(rel)));
    assert_eq!(status, 200);
    assert_eq!(body["path"], rel);
    assert!(body["text"].as_str().expect("text").contains("demle"));
    let (status, body) = get_json(&live, &format!("/api/card?path={}", encode(rel)));
    assert_eq!(status, 200);
    assert_eq!(body["card"]["path"], rel);
    let (_, found) = get_json(&live, &format!("/api/search?q={}", encode("çay")));
    assert_eq!(found["results"][0]["path"], rel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_returns_at_most_20() {
    let dir = project();
    for i in 0..30 {
        write(
            dir.path(),
            &format!("src/items/item{i:02}.ts"),
            &format!("export const item{i} = {i};\n"),
        );
    }
    let live = start(dir.path()).await;

    let (status, body) = get_json(&live, "/api/search?q=item");
    assert_eq!(status, 200);
    assert_eq!(body["v"], 1);
    let results = body["results"].as_array().expect("results");
    assert_eq!(results.len(), 20);
    assert!(results.iter().all(|r| r["kind"] == "file"));

    let (_, body) = get_json(&live, "/api/search?q=saveOrder");
    let hit = &body["results"][0];
    assert_eq!(hit["kind"], "symbol");
    assert_eq!(hit["path"], "src/orders/repo.ts");
    assert_eq!(hit["symbol"], "saveOrder");
    assert_eq!(hit["label"], "src/orders/repo.ts#saveOrder");

    // No query, no results.
    let (status, body) = get_json(&live, "/api/search");
    assert_eq!(status, 200);
    assert_eq!(body["results"], serde_json::json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn journeys_and_tech_are_empty_and_versioned() {
    let dir = project();
    let live = start(dir.path()).await;
    let (_, journeys) = get_json(&live, "/api/journeys");
    assert_eq!(journeys, serde_json::json!({ "v": 1, "journeys": [] }));
    let (_, tech) = get_json(&live, "/api/tech");
    assert_eq!(tech, serde_json::json!({ "v": 1, "categories": [] }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_api_response_has_v_1_and_no_store() {
    let dir = project();
    let live = start(dir.path()).await;
    let paths = [
        "/api/card?path=src/orders/service.ts",
        "/api/card?path=src/orders/repo.ts&symbol=saveOrder",
        "/api/file?path=src/orders/service.ts",
        "/api/search?q=order",
        "/api/journeys",
        "/api/tech",
        // Errors are JSON with the version too.
        "/api/file?path=..",
        "/api/card?path=nothing.ts",
    ];
    for path in paths {
        let reply = request(live.port, &live.token, path, Auth::Cookie, &[]);
        assert_eq!(reply.header("cache-control"), Some("no-store"), "{path}");
        assert_eq!(reply.json()["v"], 1, "{path}");
        assert_eq!(
            reply.header("content-type"),
            Some("application/json"),
            "{path}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn index_html_and_the_bundle_are_served_with_csp() {
    let dir = project();
    let live = start(dir.path()).await;
    let cases = [
        ("/", "text/html; charset=utf-8", "/assets/app.js"),
        ("/assets/app.js", "text/javascript; charset=utf-8", ""),
        ("/assets/app.css", "text/css; charset=utf-8", ""),
    ];
    for (path, content_type, needle) in cases {
        let reply = request(live.port, &live.token, path, Auth::Cookie, &[]);
        assert_eq!(reply.status, 200, "{path}");
        assert_eq!(reply.header("content-type"), Some(content_type), "{path}");
        assert_eq!(
            reply.header("content-security-policy"),
            Some("default-src 'self'; img-src 'self' data:"),
            "{path}"
        );
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
        assert_eq!(reply.header("cache-control"), Some("no-store"));
        assert!(reply.text().contains(needle), "{path}");
        assert!(!reply.body.is_empty(), "{path}");
    }
    let missing = request(live.port, &live.token, "/assets/nope.js", Auth::Cookie, &[]);
    assert_eq!(missing.status, 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_and_assets_are_gzipped_when_asked() {
    let dir = project();
    let live = start(dir.path()).await;
    let plain = request(live.port, &live.token, "/assets/app.js", Auth::Cookie, &[]);
    let gzip = request(
        live.port,
        &live.token,
        "/assets/app.js",
        Auth::Cookie,
        &[("Accept-Encoding", "gzip")],
    );
    assert_eq!(gzip.header("content-encoding"), Some("gzip"));
    assert!(gzip.body.len() < plain.body.len());
    let mut unzipped = Vec::new();
    GzDecoder::new(&gzip.body[..])
        .read_to_end(&mut unzipped)
        .expect("valid gzip");
    assert_eq!(unzipped, plain.body);

    let json = request(
        live.port,
        &live.token,
        "/api/card?path=src/orders/service.ts",
        Auth::Cookie,
        &[("Accept-Encoding", "gzip")],
    );
    assert_eq!(json.header("content-encoding"), Some("gzip"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn odd_query_strings_are_answered_and_the_server_keeps_serving() {
    let dir = project();
    let live = start(dir.path()).await;
    // Bytes that are not UTF-8, in a path and in a search.
    assert_eq!(get_json(&live, "/api/file?path=src/%ff.ts").0, 404);
    assert_eq!(get_json(&live, "/api/card?path=%ff").0, 404);
    assert_eq!(get_json(&live, "/api/search?q=%ff%fe").0, 200);
    // A search far longer than any file name matches nothing but is not an error.
    let long = "a".repeat(60_000);
    let (status, body) = get_json(&live, &format!("/api/search?q={long}"));
    assert_eq!(status, 200);
    assert_eq!(body["results"], serde_json::json!([]));
    let (status, _) = get_json(&live, &format!("/api/file?path={long}"));
    assert_eq!(status, 404);
    // Still serving.
    assert_eq!(get_json(&live, "/api/tech").0, 200);
}
