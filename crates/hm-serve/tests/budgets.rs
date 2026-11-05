//! The payload budgets of Learn spec §10.5, measured on a synthetic project of 5 000
//! files: the first screen is at most 100 KB gzipped, a group view and a card at most
//! 50 KB each, and no response lists the whole project.

mod common;

use std::io::Write;

use common::{Auth, Reply, request, start};
use flate2::Compression;
use flate2::write::GzEncoder;
use hm_serve::{Model, Repo, ServeConfig, Server};
use hm_store::Store;
use serde_json::Value;

const FIRST_SCREEN_LIMIT: usize = 100 * 1024;
const VIEW_LIMIT: usize = 50 * 1024;
const FILES: u32 = 5_000;

/// The size of `body` gzipped at the level a browser would be sent.
fn gzipped(body: &[u8]) -> usize {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(body).expect("gzip into memory");
    gz.finish().expect("gzip finishes").len()
}

/// A server over the synthetic map of 5 000 files, with an empty store behind it.
struct Synthetic {
    port: u16,
    token: String,
    _db: tempfile::TempDir,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Synthetic {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Synthetic {
    async fn start() -> Synthetic {
        let db = tempfile::tempdir().expect("create a temporary directory");
        let path = db.path().join("facts.db");
        drop(Store::open(&path).expect("create the store"));
        let input = hm_layout::synth::input(7, FILES);
        let excluded = vec![("doc".to_string(), 12), ("test".to_string(), 340)];
        let model = Model::from_input(&input, excluded);
        let config = ServeConfig {
            root: db.path().to_path_buf(),
            db: path,
            repo: Repo {
                name: "shop".into(),
                branch: Some("main".into()),
                commit: Some("abc1234".into()),
            },
            token: None,
        };
        let server = Server::bind(config, model).await.expect("bind the server");
        let (port, token) = (server.port(), server.token().to_string());
        let task = tokio::spawn(async move {
            let _ = server.run(std::future::pending()).await;
        });
        Synthetic {
            port,
            token,
            _db: db,
            task,
        }
    }

    fn get(&self, path: &str) -> Reply {
        let reply = request(self.port, &self.token, path, Auth::Cookie, &[]);
        assert_eq!(reply.status, 200, "{path}");
        reply
    }
}

/// Every group of the map, by walking the overview and then each group's children,
/// with the group's reply.
fn every_group(server: &Synthetic) -> Vec<(String, Reply)> {
    let mut todo: Vec<Value> = server.get("/api/overview").json()["boxes"]
        .as_array()
        .expect("boxes")
        .clone();
    let mut groups = Vec::new();
    while let Some(b) = todo.pop() {
        if b["expandable"] != true {
            continue;
        }
        let id = b["id"].as_str().expect("an id").to_string();
        let reply = server.get(&format!("/api/group/{id}"));
        todo.extend(
            reply.json()["children"]
                .as_array()
                .expect("children")
                .clone(),
        );
        groups.push((id, reply));
    }
    groups
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_screen_is_under_100_kb_gzipped() {
    let server = Synthetic::start().await;
    let parts = [
        "/",
        "/assets/app.js",
        "/assets/app.css",
        "/api/overview",
        "/api/journeys",
        "/api/tech",
    ];
    let mut total = 0;
    for path in parts {
        let size = gzipped(&server.get(path).body);
        eprintln!("first screen: {path} {size} bytes gzipped");
        total += size;
    }
    eprintln!("first screen: {total} bytes gzipped in all");
    assert!(
        total <= FIRST_SCREEN_LIMIT,
        "the first screen is {total} bytes gzipped, the limit is {FIRST_SCREEN_LIMIT}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_group_view_is_under_50_kb_gzipped() {
    let server = Synthetic::start().await;
    let groups = every_group(&server);
    assert!(groups.len() > 20, "the project has {} groups", groups.len());
    let mut biggest = (0, String::new());
    for (id, reply) in &groups {
        let size = gzipped(&reply.body);
        if size > biggest.0 {
            biggest = (size, id.clone());
        }
        assert!(
            size <= VIEW_LIMIT,
            "group {id} is {size} bytes gzipped, the limit is {VIEW_LIMIT}"
        );
    }
    eprintln!(
        "{} groups; the biggest is {} at {} bytes gzipped",
        groups.len(),
        biggest.1,
        biggest.0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_card_of_the_busiest_file_is_under_50_kb_gzipped() {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    hm_index::synth::project(dir.path(), 7, 400);
    let live = start(dir.path()).await;
    // The file most files import, from the reverse-import table.
    let store = Store::open(&live.db).expect("open the store");
    let (path, importers): (String, i64) = store
        .conn()
        .query_row(
            "SELECT f.path, COUNT(*) AS n FROM reverse_import r
               JOIN file f ON f.id = r.dst_file_id
              GROUP BY f.path ORDER BY n DESC, f.path LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the project has imports");
    assert!(importers >= 5, "{path} has {importers} importers");

    let reply = request(
        live.port,
        &live.token,
        &format!("/api/card?path={}", common::encode(&path)),
        Auth::Cookie,
        &[],
    );
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json()["card"]["used_by"].as_array().map(Vec::len),
        Some(importers as usize)
    );
    let size = gzipped(&reply.body);
    eprintln!("card of {path} ({importers} importers): {size} bytes gzipped");
    assert!(
        size <= VIEW_LIMIT,
        "the card is {size} bytes gzipped, the limit is {VIEW_LIMIT}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_response_lists_every_file() {
    let server = Synthetic::start().await;
    let overview = server.get("/api/overview");
    let total = overview.json()["files_total"].as_u64().expect("a count");
    assert_eq!(total, u64::from(FILES));
    let paths = |reply: &Reply| {
        let json = reply.json();
        let boxes = json["boxes"]
            .as_array()
            .or_else(|| json["children"].as_array());
        let boxes = boxes.expect("boxes or children");
        boxes.iter().filter(|b| b["path"].is_string()).count() as u64
    };
    assert!(paths(&overview) < total);
    let mut widest = 0;
    for (id, reply) in every_group(&server) {
        let n = paths(&reply);
        widest = widest.max(n);
        assert!(n < total, "group {id} lists {n} of {total} files");
        assert!(
            n <= 25,
            "group {id} lists {n} files, a view holds at most 25 boxes"
        );
    }
    eprintln!("the widest group lists {widest} of {total} files");
}
