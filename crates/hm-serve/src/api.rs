//! The JSON endpoints: the map (overview and groups), cards, search, and the two lists
//! that are empty until later levels fill them. Every body has `"v": 1`.

use std::sync::{Arc, PoisonError};

use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use hm_index::index::Result as IndexResult;
use hm_index::query::{FileCard, SymbolCard, file_card, symbol_card};
use hm_index::view::search;
use hm_layout::{FileLinks, GroupView, OverviewView};
use hm_store::Store;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{Repo, Shared};

/// The most results `/api/search` returns.
const SEARCH_LIMIT: usize = 20;
/// The most characters of a search that are used. No file name is longer, and SQLite
/// refuses a `LIKE` pattern of about 50 000 bytes.
const SEARCH_CHARS: usize = 256;

/// An error answer: a status and a short English message, as `{"v":1,"error":...}`.
#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: &'static str,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: &'static str) -> ApiError {
        ApiError { status, message }
    }

    pub(crate) fn bad_request(message: &'static str) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, message)
    }

    pub(crate) fn not_found() -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not in the index")
    }

    /// A 500 with a generic body; the cause goes to stderr, not to the client.
    pub(crate) fn internal(cause: impl std::fmt::Display) -> ApiError {
        eprintln!("harbormap: serve: {cause}");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            [(header::CONTENT_TYPE, "application/json")],
            json!({ "v": 1, "error": self.message }).to_string(),
        )
            .into_response()
    }
}

/// Query parameters; a missing or malformed one is a JSON 400.
pub(crate) struct Params<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for Params<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Params(value))
            .map_err(|_| ApiError::bad_request("bad query string"))
    }
}

/// Runs `f` on the read connection, off the async threads. The connection is shared
/// behind a mutex: reads are short and the panel makes few of them at once.
pub(crate) async fn read<T, F>(shared: &Arc<Shared>, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Store) -> IndexResult<T> + Send + 'static,
{
    let shared = Arc::clone(shared);
    tokio::task::spawn_blocking(move || {
        let store = shared.store.lock().unwrap_or_else(PoisonError::into_inner);
        f(&store)
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(ApiError::internal)
}

/// Accepts a repository-relative, `/`-separated path and nothing else: no empty path,
/// no absolute path (`/x`, `C:/x`), no `\`, no NUL, no `.` or `..` segment.
pub(crate) fn check_path(path: &str) -> Result<(), ApiError> {
    let bad = |m| Err(ApiError::bad_request(m));
    if path.is_empty() {
        return bad("empty path");
    }
    // `/x`, or a Windows drive (`C:x`, `C:/x`): an ASCII letter and a colon.
    let drive = matches!(path.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
    if path.starts_with('/') || drive {
        return bad("absolute path");
    }
    if path.contains('\\') || path.contains('\0') {
        return bad("bad character in path");
    }
    if path.split('/').any(|s| s == ".." || s == ".") {
        return bad("`.` and `..` are not allowed in a path");
    }
    Ok(())
}

#[derive(serde::Deserialize)]
pub(crate) struct CardParams {
    path: String,
    symbol: Option<String>,
    /// The view the card is opened from: `overview` (the default) or a group id.
    view: Option<String>,
}

/// `GET /api/overview`: the top boxes and the arrows between them.
#[derive(Serialize)]
pub(crate) struct Overview {
    v: u8,
    seq: u64,
    repo: Repo,
    updated_ms: u64,
    #[serde(flatten)]
    view: OverviewView,
    /// Files left off the map, by class.
    excluded: Vec<Excluded>,
    files_total: u32,
}

#[derive(Serialize)]
struct Excluded {
    class: String,
    files: u32,
}

/// `GET /api/group/{id}`.
#[derive(Serialize)]
pub(crate) struct Group {
    v: u8,
    seq: u64,
    #[serde(flatten)]
    view: GroupView,
}

/// `GET /api/overview`.
pub(crate) async fn overview(State(shared): State<Arc<Shared>>) -> Json<Overview> {
    let now = shared.current();
    let model = &now.model;
    Json(Overview {
        v: 1,
        seq: now.seq,
        repo: now.repo.clone(),
        updated_ms: now.updated_ms,
        view: model.map.overview(),
        // `Model::excluded` is already sorted by class.
        excluded: (model.excluded.iter())
            .map(|(class, files)| Excluded {
                class: class.clone(),
                files: *files,
            })
            .collect(),
        files_total: model.files_total,
    })
}

/// `GET /api/group/{id}`: 404 for an id that is no group of the current map.
pub(crate) async fn group(
    State(shared): State<Arc<Shared>>,
    Path(id): Path<String>,
) -> Result<Json<Group>, ApiError> {
    let now = shared.current();
    let view = now.model.map.group(&id).ok_or_else(ApiError::not_found)?;
    Ok(Json(Group {
        v: 1,
        seq: now.seq,
        view,
    }))
}

/// The card of a file: what the index knows about it.
#[derive(Serialize)]
struct FileCardPayload {
    v: u8,
    seq: u64,
    kind: &'static str,
    card: FileCard,
    /// The file's arrows lifted to the boxes of the open view; `null` when the file is
    /// not on the map (a test, a doc) or the view is unknown.
    links: Option<FileLinks>,
    /// The groups holding the file, from its top box down to its parent group; `null`
    /// when the file is not on the map.
    ancestors: Option<Vec<String>>,
}

/// The card of a symbol.
#[derive(Serialize)]
struct SymbolCardPayload {
    v: u8,
    seq: u64,
    kind: &'static str,
    card: SymbolCard,
}

/// `GET /api/card?path=P[&symbol=S]`.
pub(crate) async fn card(
    State(shared): State<Arc<Shared>>,
    Params(params): Params<CardParams>,
) -> Result<Response, ApiError> {
    check_path(&params.path)?;
    let now = shared.current();
    let seq = now.seq;
    let CardParams { path, symbol, view } = params;
    match symbol {
        Some(symbol) => {
            let card = read(&shared, move |s| symbol_card(s, &path, &symbol)).await?;
            let card = card.ok_or_else(ApiError::not_found)?;
            Ok(Json(SymbolCardPayload {
                v: 1,
                seq,
                kind: "symbol",
                card,
            })
            .into_response())
        }
        None => {
            let map_path = path.clone();
            let card = read(&shared, move |s| file_card(s, &path)).await?;
            let card = card.ok_or_else(ApiError::not_found)?;
            let map = &now.model.map;
            let links = map.file_links(&map_path, view.as_deref().unwrap_or("overview"));
            let ancestors = map.ancestors(&map_path);
            Ok(Json(FileCardPayload {
                v: 1,
                seq,
                kind: "file",
                card,
                links,
                ancestors,
            })
            .into_response())
        }
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct SearchParams {
    q: Option<String>,
}

/// `GET /api/search?q=Q`: at most 20 files and symbols, files first.
pub(crate) async fn find(
    State(shared): State<Arc<Shared>>,
    Params(params): Params<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    let q: String = params
        .q
        .unwrap_or_default()
        .chars()
        .take(SEARCH_CHARS)
        .collect();
    let hits = read(&shared, move |s| search(s, &q, SEARCH_LIMIT)).await?;
    let results: Vec<Value> = hits
        .into_iter()
        .map(|hit| {
            let label = match &hit.symbol {
                Some(symbol) => format!("{}#{symbol}", hit.path),
                None => hit.path.clone(),
            };
            json!({ "kind": hit.kind, "path": hit.path, "symbol": hit.symbol, "label": label })
        })
        .collect();
    Ok(Json(json!({ "v": 1, "results": results })))
}

/// `GET /api/journeys`: none until a later level finds them.
pub(crate) async fn journeys() -> Json<Value> {
    Json(json!({ "v": 1, "journeys": [] }))
}

/// `GET /api/tech`: none until a later level finds them.
pub(crate) async fn tech() -> Json<Value> {
    Json(json!({ "v": 1, "categories": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_path_accepts_plain_relative_paths() {
        for ok in [
            "a.ts",
            "src/orders/service.ts",
            "src/çay #2.ts",
            "a/.hidden",
            "a..b",
        ] {
            assert!(check_path(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn check_path_refuses_everything_else() {
        for bad in [
            "",
            "/etc/passwd",
            "C:/x",
            "a\\b",
            "a\0b",
            "..",
            "a/../b",
            "./a",
            "a/./b",
            "a/..",
        ] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
    }
}
