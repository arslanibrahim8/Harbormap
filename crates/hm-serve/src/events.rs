//! `/api/events`: a server-sent event stream. The first event is `hello` with the
//! model's `seq`; after it comes one `facts` event per published delta.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
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

/// The `hello` event for model version `seq`.
fn hello(seq: u64) -> Event {
    Event::default()
        .event("hello")
        .data(format!("{{\"seq\":{seq}}}"))
}

/// `GET /api/events`.
pub(crate) async fn events(State(shared): State<Arc<Shared>>) -> impl IntoResponse {
    let (mut rx, seq) = shared.subscribe();
    let (out, stream) = mpsc::channel::<Result<Event, Infallible>>(16);
    tokio::spawn(async move {
        let mut closing = shared.closing.subscribe();
        if out.send(Ok(hello(seq))).await.is_err() {
            return;
        }
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(delta) => {
                        let Ok(event) = Event::default().event("facts").json_data(&*delta) else {
                            continue;
                        };
                        if out.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                    // Too slow to follow: skip what was missed and tell the panel
                    // where the model is now, so it can fetch what it needs.
                    Err(RecvError::Lagged(_)) => {
                        let (fresh, seq) = shared.subscribe();
                        rx = fresh;
                        if out.send(Ok(hello(seq))).await.is_err() {
                            return;
                        }
                    }
                    Err(RecvError::Closed) => return,
                },
                // The client went away.
                () = out.closed() => return,
                // The server is shutting down; open streams must not hold it up.
                _ = async { let _ = closing.wait_for(|closing| *closing).await; } => return,
            }
        }
    });
    Sse::new(ReceiverStream::new(stream))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
