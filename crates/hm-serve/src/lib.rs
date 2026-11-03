//! The Learn panel server: serves the embedded panel and its JSON views on 127.0.0.1
//! with a per-run token, and pushes facts deltas as server-sent events (Learn spec §10.6).

mod api;
mod assets;
mod auth;
mod events;
mod model;
mod source;

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use hm_store::Store;
use tokio::sync::{broadcast, watch};
use tower_http::compression::CompressionLayer;

pub use events::FactsDelta;
pub use model::Model;

/// The repository the panel shows in its header.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Repo {
    pub name: String,
    pub branch: Option<String>,
    /// The first 7 characters of the commit id.
    pub commit: Option<String>,
}

/// What the server needs to run.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// The worktree root; `/api/file` never serves anything outside it.
    pub root: PathBuf,
    /// The facts store the views read.
    pub db: PathBuf,
    pub repo: Repo,
    /// The token to accept; `None` makes a random one for this run.
    pub token: Option<String>,
}

/// The model and its version, changed together under one lock.
struct Live {
    model: Arc<Model>,
    seq: u64,
}

/// State shared by every request and by the [`Handle`]s.
pub(crate) struct Shared {
    /// The canonical worktree root.
    pub(crate) root: PathBuf,
    pub(crate) token: String,
    pub(crate) port: u16,
    /// The read connection; see [`api::read`].
    pub(crate) store: Mutex<Store>,
    live: Mutex<Live>,
    deltas: broadcast::Sender<Arc<FactsDelta>>,
    /// Set to `true` when the server shuts down, so event streams end.
    pub(crate) closing: watch::Sender<bool>,
}

impl Shared {
    fn live(&self) -> std::sync::MutexGuard<'_, Live> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn seq(&self) -> u64 {
        self.live().seq
    }

    /// A receiver of the deltas published from now on, and the `seq` it starts after.
    /// Both are read under one lock, so no delta falls between them.
    pub(crate) fn subscribe(&self) -> (broadcast::Receiver<Arc<FactsDelta>>, u64) {
        let live = self.live();
        (self.deltas.subscribe(), live.seq)
    }
}

/// A bound server that has not started serving yet.
pub struct Server {
    shared: Arc<Shared>,
    listener: tokio::net::TcpListener,
}

/// Publishes model updates to the running server. Cheap to clone.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
}

impl Server {
    /// Binds `127.0.0.1` on a port the OS picks and prepares to serve `model`.
    pub async fn bind(config: ServeConfig, model: Model) -> io::Result<Server> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let token = match &config.token {
            Some(token) => token.clone(),
            None => auth::new_token()?,
        };
        let store = Store::open(&config.db).map_err(io::Error::other)?;
        let root = dunce::canonicalize(&config.root)?;
        let (deltas, _) = broadcast::channel(32);
        let (closing, _) = watch::channel(false);
        let shared = Arc::new(Shared {
            root,
            token,
            port,
            store: Mutex::new(store),
            live: Mutex::new(Live {
                model: Arc::new(model),
                seq: 0,
            }),
            deltas,
            closing,
        });
        Ok(Server { shared, listener })
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    pub fn token(&self) -> &str {
        &self.shared.token
    }

    /// Where to open the panel: the address with the token, which the first visit
    /// trades for a cookie.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/?t={}", self.port(), self.token())
    }

    pub fn handle(&self) -> Handle {
        Handle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Serves until `shutdown` completes, then stops; open event streams end with it.
    pub async fn run(self, shutdown: impl Future<Output = ()> + Send + 'static) -> io::Result<()> {
        let shared = Arc::clone(&self.shared);
        let app = router(Arc::clone(&shared));
        axum::serve(self.listener, app)
            .with_graceful_shutdown(async move {
                shutdown.await;
                shared.closing.send_replace(true);
            })
            .await
    }
}

impl Handle {
    /// Swaps in the new model, sets `delta.seq` to the next seq, and sends the `facts`
    /// event. Returns the new seq.
    pub fn publish(&self, model: Model, mut delta: FactsDelta) -> u64 {
        let mut live = self.shared.live();
        live.seq += 1;
        delta.seq = live.seq;
        live.model = Arc::new(model);
        // No receiver is not an error: nobody has the panel open.
        let _ = self.shared.deltas.send(Arc::new(delta));
        live.seq
    }

    /// The version of the current model: 0, plus 1 per published delta.
    pub fn seq(&self) -> u64 {
        self.shared.seq()
    }

    /// The current model.
    pub fn model(&self) -> Arc<Model> {
        Arc::clone(&self.shared.live().model)
    }

    /// The address the server listens on.
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.shared.port))
    }
}

/// The headers every response carries.
async fn headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; img-src 'self' data:"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    res
}

fn router(shared: Arc<Shared>) -> Router {
    // The event stream is not compressed: a gzip stream would hold events back.
    let compressed = Router::new()
        .route("/", get(assets::index))
        .route("/assets/{*file}", get(assets::asset))
        .route("/api/card", get(api::card))
        .route("/api/file", get(source::file))
        .route("/api/search", get(api::find))
        .route("/api/journeys", get(api::journeys))
        .route("/api/tech", get(api::tech))
        .layer(CompressionLayer::new());
    compressed
        .route("/api/events", get(events::events))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&shared),
            auth::guard,
        ))
        .layer(middleware::from_fn(headers))
        .with_state(shared)
}
