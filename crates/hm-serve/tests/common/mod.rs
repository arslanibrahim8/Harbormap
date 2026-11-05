//! Helpers shared by the integration tests: a synthetic shop project, a running
//! server over it, and a blocking HTTP client.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use hm_index::index::Indexer;
use hm_serve::{Handle, Model, Repo, ServeConfig, Server};
use hm_store::Store;

/// Writes `text` to `rel` under `dir`, creating folders.
pub fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("a parent folder")).expect("create folders");
    fs::write(path, text).expect("write a file");
}

/// A small shop app: a page, an orders service and repository, a db client, a test
/// and a README.
pub fn shop(dir: &Path) {
    write(
        dir,
        "src/db/client.ts",
        "export function insert(table: string, row: object) { return table; }\n\
         export function find(table: string) { return []; }\n",
    );
    write(
        dir,
        "src/orders/repo.ts",
        "import { insert, find } from '../db/client';\n\
         export function saveOrder(row: object) { return insert('orders', row); }\n\
         export function listOrders() { return find('orders'); }\n",
    );
    write(
        dir,
        "src/orders/service.ts",
        "import { saveOrder, listOrders } from './repo';\n\
         export function placeOrder(cart: object) { return saveOrder(cart); }\n\
         export function history() { return listOrders(); }\n",
    );
    write(
        dir,
        "src/orders/service.test.ts",
        "import { placeOrder } from './service';\nplaceOrder({});\n",
    );
    write(
        dir,
        "src/app/page.tsx",
        "import { placeOrder } from '../orders/service';\n\
         export default function Page() { return placeOrder({}); }\n",
    );
    write(dir, "README.md", "# Shop\n");
}

/// Indexes the project at `dir` into the store at `db`.
pub fn index(dir: &Path, db: &Path) {
    let store = Store::open(db).expect("open the store");
    let mut ix = Indexer::open(dir, store).expect("open the indexer");
    ix.full().expect("index the project");
}

/// A server running over a project. It stops when dropped.
pub struct Live {
    pub port: u16,
    pub token: String,
    pub handle: Handle,
    /// The facts store the server reads.
    pub db: PathBuf,
    task: tokio::task::JoinHandle<()>,
    _db_dir: tempfile::TempDir,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Indexes `dir`, binds a server over it and runs it on a task. The store lives in
/// its own temporary folder, never inside the project.
pub async fn start(dir: &Path) -> Live {
    let db_dir = tempfile::tempdir().expect("create a temporary directory");
    let db = db_dir.path().join("facts.db");
    index(dir, &db);
    let store = Store::open(&db).expect("open the store");
    let model = Model::load(&store, "shop", None).expect("load the model");
    drop(store);
    let config = ServeConfig {
        root: dir.to_path_buf(),
        db: db.clone(),
        repo: Repo {
            name: "shop".into(),
            branch: Some("main".into()),
            commit: Some("abc1234".into()),
        },
        token: None,
    };
    let server = Server::bind(config, model).await.expect("bind the server");
    let (port, token, handle) = (server.port(), server.token().to_string(), server.handle());
    let task = tokio::spawn(async move {
        let _ = server.run(std::future::pending()).await;
    });
    Live {
        port,
        token,
        handle,
        db,
        task,
        _db_dir: db_dir,
    }
}

/// How a request proves who it is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    None,
    Cookie,
    Bearer,
    Query,
}

/// An answer, with its header names lowercased.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }
}

/// Percent-encodes `text` for a query value.
pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A GET with `auth` and extra request headers. 4xx and 5xx are answers, and a
/// redirect is returned as it is.
pub fn request(port: u16, token: &str, path: &str, auth: Auth, extra: &[(&str, &str)]) -> Reply {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut url = format!("http://127.0.0.1:{port}{path}");
    if auth == Auth::Query {
        url.push(if path.contains('?') { '&' } else { '?' });
        url.push_str(&format!("t={token}"));
    }
    let mut req = agent.get(&url);
    match auth {
        Auth::Cookie => req = req.header("Cookie", format!("hm_learn_{port}={token}")),
        Auth::Bearer => req = req.header("Authorization", format!("Bearer {token}")),
        Auth::None | Auth::Query => {}
    }
    for (name, value) in extra {
        req = req.header(*name, *value);
    }
    let mut res = req.call().expect("the request completes");
    let headers = res
        .headers()
        .iter()
        .map(|(n, v)| {
            (
                n.as_str().to_ascii_lowercase(),
                v.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let status = res.status().as_u16();
    let body = res
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .expect("read the body");
    Reply {
        status,
        headers,
        body,
    }
}

/// A GET as `(status, body text)`.
pub fn get(live: &Live, path: &str, auth: Auth) -> (u16, String) {
    let reply = request(live.port, &live.token, path, auth, &[]);
    (reply.status, reply.text())
}

/// A GET with the cookie, whose body is JSON.
pub fn get_json(live: &Live, path: &str) -> (u16, serde_json::Value) {
    let reply = request(live.port, &live.token, path, Auth::Cookie, &[]);
    (reply.status, reply.json())
}
