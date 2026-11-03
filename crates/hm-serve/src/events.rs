//! `/api/events`: a server-sent event stream. The first event is `hello` with the
//! model's `seq`; after it comes one `facts` event per published delta.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Serialize;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::{mpsc, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::Shared;

/// What changed in one update of the facts (Learn spec §10.6). `seq` is set by
/// [`crate::Handle::publish`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FactsDelta {
    pub seq: u64,
    /// When the change was seen, in milliseconds since the Unix epoch.
    pub at_ms: u64,
    /// How long indexing took.
    pub took_ms: u64,
    pub files_changed: Vec<String>,
    pub files_removed: Vec<String>,
    /// The groups were rebuilt, so box positions may have moved.
    pub regrouped: bool,
    pub groups_changed: Vec<String>,
}

/// What the forwarder tells one client.
#[derive(Debug, PartialEq, Eq)]
enum Msg {
    /// The model is at this `seq`.
    Hello(u64),
    Facts(Arc<FactsDelta>),
}

impl Msg {
    fn into_event(self) -> Event {
        match self {
            Msg::Hello(seq) => Event::default()
                .event("hello")
                .data(format!("{{\"seq\":{seq}}}")),
            Msg::Facts(delta) => Event::default()
                .event("facts")
                .json_data(&*delta)
                .unwrap_or_else(|_| Event::default().comment("unserializable delta")),
        }
    }
}

/// Sends `msg` to the client. `false` when the client is gone or the server is
/// shutting down; both are noticed while the client is not reading, too.
async fn deliver(out: &mpsc::Sender<Msg>, closing: &mut watch::Receiver<bool>, msg: Msg) -> bool {
    tokio::select! {
        sent = out.send(msg) => sent.is_ok(),
        _ = async { let _ = closing.wait_for(|closing| *closing).await; } => false,
    }
}

/// Feeds `out` for one client: `hello(seq)`, then every delta that comes through `rx`.
/// A receiver that falls behind the broadcast skips what it missed: it is resubscribed
/// and the client gets a fresh `hello`, so it can fetch what it needs. Ends when the
/// client or the server goes away.
async fn forward(
    shared: Arc<Shared>,
    out: mpsc::Sender<Msg>,
    mut rx: broadcast::Receiver<Arc<FactsDelta>>,
    seq: u64,
) {
    let mut closing = shared.closing.subscribe();
    if !deliver(&out, &mut closing, Msg::Hello(seq)).await {
        return;
    }
    loop {
        let msg = tokio::select! {
            msg = rx.recv() => match msg {
                Ok(delta) => Msg::Facts(delta),
                Err(RecvError::Lagged(_)) => {
                    let (fresh, seq) = shared.subscribe();
                    rx = fresh;
                    Msg::Hello(seq)
                }
                Err(RecvError::Closed) => return,
            },
            () = out.closed() => return,
            _ = async { let _ = closing.wait_for(|closing| *closing).await; } => return,
        };
        if !deliver(&out, &mut closing, msg).await {
            return;
        }
    }
}

/// `GET /api/events`.
pub(crate) async fn events(State(shared): State<Arc<Shared>>) -> impl IntoResponse {
    let (rx, seq) = shared.subscribe();
    let (out, stream) = mpsc::channel(16);
    tokio::spawn(forward(shared, out, rx, seq));
    let events = ReceiverStream::new(stream).map(|msg| Ok::<_, Infallible>(msg.into_event()));
    Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Handle, Model, Repo, ServeConfig, Server};

    /// A server over an empty store, never run.
    async fn server() -> (Server, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create a temporary directory");
        let store = hm_store::Store::open_in_memory().expect("open a store");
        let model = Model::load(&store, "shop", None).expect("load a model");
        let config = ServeConfig {
            root: dir.path().to_path_buf(),
            db: dir.path().join("facts.db"),
            repo: Repo {
                name: "shop".into(),
                branch: None,
                commit: None,
            },
            token: None,
        };
        (Server::bind(config, model).await.expect("bind"), dir)
    }

    fn publish(handle: &Handle) -> u64 {
        let model = (*handle.model()).clone();
        handle.publish(model, FactsDelta::default())
    }

    #[tokio::test]
    async fn a_client_that_falls_behind_skips_the_backlog_and_gets_a_fresh_hello() {
        let (server, _dir) = server().await;
        let handle = server.handle();
        let shared = Arc::clone(&server.shared);
        let (rx, seq) = shared.subscribe();
        // Room for one message, and nobody reads: the forwarder blocks on the second.
        let (out, mut client) = mpsc::channel(1);
        tokio::spawn(forward(Arc::clone(&shared), out, rx, seq));

        // Far more deltas than the broadcast holds (32), published without a yield, so
        // the forwarder cannot keep up whatever the scheduler does.
        for _ in 0..200 {
            publish(&handle);
        }
        assert_eq!(handle.seq(), 200);

        let mut facts = 0;
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), client.recv())
                .await
                .expect("the forwarder keeps going")
                .expect("the stream is open");
            match msg {
                Msg::Hello(200) => break,
                Msg::Hello(_) => {}
                Msg::Facts(_) => facts += 1,
            }
        }
        assert!(facts < 200, "the backlog was replayed ({facts} deltas)");
        // And it follows from there.
        let next = publish(&handle);
        let msg = tokio::time::timeout(Duration::from_secs(5), client.recv())
            .await
            .expect("a delta arrives")
            .expect("the stream is open");
        assert!(matches!(msg, Msg::Facts(d) if d.seq == next));
    }

    #[tokio::test]
    async fn a_stalled_client_does_not_hold_up_shutdown() {
        let (server, _dir) = server().await;
        let handle = server.handle();
        let shared = Arc::clone(&server.shared);
        let (rx, seq) = shared.subscribe();
        let (out, _client) = mpsc::channel(1);
        let task = tokio::spawn(forward(Arc::clone(&shared), out, rx, seq));
        for _ in 0..5 {
            publish(&handle);
        }
        // The client is not reading, so the forwarder is stuck in a send.
        tokio::task::yield_now().await;
        shared.closing.send_replace(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the forwarder stops")
            .expect("no panic");
    }

    #[tokio::test]
    async fn a_dropped_client_ends_the_forwarder() {
        let (server, _dir) = server().await;
        let shared = Arc::clone(&server.shared);
        let (rx, seq) = shared.subscribe();
        let (out, client) = mpsc::channel(1);
        let task = tokio::spawn(forward(Arc::clone(&shared), out, rx, seq));
        drop(client);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the forwarder stops")
            .expect("no panic");
    }
}
