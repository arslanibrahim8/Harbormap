//! The event stream: `hello`, then one `facts` event per published delta.

mod common;

use std::io::{BufRead, BufReader};
use std::time::Duration;

use common::{Live, shop, start};
use hm_serve::{FactsDelta, Model};
use hm_store::Store;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create a temporary directory");
    shop(dir.path());
    dir
}

/// A reader of `/api/events` for the server `live`.
fn subscribe(live: &Live) -> BufReader<Box<dyn std::io::Read + Send + Sync>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_recv_body(Some(Duration::from_secs(10)))
        .build()
        .into();
    let res = agent
        .get(format!("http://127.0.0.1:{}/api/events", live.port))
        .header("Authorization", format!("Bearer {}", live.token))
        .call()
        .expect("connect to the event stream");
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(
        res.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    assert!(res.headers().get("content-encoding").is_none());
    let (_, body) = res.into_parts();
    BufReader::new(Box::new(body.into_reader()))
}

/// The next event as (name, data), skipping comments.
fn next_event(reader: &mut BufReader<Box<dyn std::io::Read + Send + Sync>>) -> (String, String) {
    let (mut name, mut data) = (String::new(), String::new());
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).expect("read the stream");
        assert!(n > 0, "the stream ended");
        let line = line.trim_end();
        if line.is_empty() {
            if !name.is_empty() {
                return (name, data);
            }
        } else if let Some(v) = line.strip_prefix("event:") {
            name = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            data = v.trim().to_string();
        }
    }
}

fn model(live: &Live) -> Model {
    let store = Store::open(&live.db).expect("open the store");
    Model::load(&store, "shop", None).expect("load the model")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hello_carries_the_current_seq() {
    let dir = project();
    let live = start(dir.path()).await;
    assert_eq!(live.handle.seq(), 0);
    assert_eq!(live.handle.publish(model(&live), FactsDelta::default()), 1);
    assert_eq!(live.handle.publish(model(&live), FactsDelta::default()), 2);

    let handle = live.handle.clone();
    let seen = tokio::task::spawn_blocking(move || {
        let mut reader = subscribe(&live);
        let first = next_event(&mut reader);
        drop(reader);
        (first, live)
    })
    .await
    .expect("the reader finishes");
    assert_eq!(seen.0.0, "hello");
    assert_eq!(seen.0.1, r#"{"seq":2}"#);
    assert_eq!(handle.seq(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_published_delta_arrives_as_a_facts_event() {
    let dir = project();
    let live = start(dir.path()).await;
    let handle = live.handle.clone();
    let m = model(&live);

    let result = tokio::task::spawn_blocking(move || {
        let mut reader = subscribe(&live);
        let hello = next_event(&mut reader);
        let delta = FactsDelta {
            seq: 999, // replaced by the server
            at_ms: 1_700_000_000_000,
            took_ms: 12,
            files_changed: vec!["src/orders/repo.ts".into()],
            files_removed: vec!["src/old.ts".into()],
            regrouped: true,
            groups_changed: vec!["g_0123456789ab".into()],
        };
        let seq = handle.publish(m, delta);
        let facts = next_event(&mut reader);
        (hello, seq, facts, live)
    })
    .await
    .expect("the reader finishes");

    let (hello, seq, facts, _live) = result;
    assert_eq!(hello, ("hello".to_string(), r#"{"seq":0}"#.to_string()));
    assert_eq!(seq, 1);
    assert_eq!(facts.0, "facts");
    let data: serde_json::Value = serde_json::from_str(&facts.1).expect("JSON data");
    assert_eq!(
        data,
        serde_json::json!({
            "seq": 1,
            "at_ms": 1_700_000_000_000u64,
            "took_ms": 12,
            "files_changed": ["src/orders/repo.ts"],
            "files_removed": ["src/old.ts"],
            "regrouped": true,
            "groups_changed": ["g_0123456789ab"],
        })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_reader_gets_a_fresh_hello_instead_of_the_backlog() {
    let dir = project();
    let live = start(dir.path()).await;
    let handle = live.handle.clone();
    let m = model(&live);

    let result = tokio::task::spawn_blocking(move || {
        let mut reader = subscribe(&live);
        assert_eq!(next_event(&mut reader).0, "hello");
        // Far more deltas than the channel and the socket buffers hold, while the
        // reader reads nothing.
        for _ in 0..3000 {
            handle.publish(
                m.clone(),
                FactsDelta {
                    files_changed: vec!["x".repeat(4096)],
                    ..FactsDelta::default()
                },
            );
        }
        let mut names = Vec::new();
        let last_seq = loop {
            let (name, data) = next_event(&mut reader);
            let v: serde_json::Value = serde_json::from_str(&data).expect("JSON data");
            let seq = v["seq"].as_u64().expect("a seq");
            names.push(name);
            if seq == 3000 {
                break seq;
            }
        };
        (names, last_seq, live)
    })
    .await
    .expect("the reader finishes");
    assert!(result.0.iter().any(|n| n == "hello"), "no hello after lag");
    assert_eq!(result.1, 3000);
}
