//! The JSON endpoints: cards, search, and the two lists that are empty until later
//! levels fill them. Every body has `"v": 1`.

use std::sync::{Arc, PoisonError};

use axum::Json;
use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use hm_index::index::Result as IndexResult;
use hm_index::query::{FileCard, SymbolCard, file_card, symbol_card};
use hm_index::view::search;
use hm_store::Store;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::Shared;

/// The most results `/api/search` returns.
const SEARCH_LIMIT: usize = 20;

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

    pub(crate) fn internal() -> ApiError {
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
    .map_err(|_| ApiError::internal())?
    .map_err(|_| ApiError::internal())
}

/// Accepts a repository-relative, `/`-separated path and nothing else: no empty path,
/// no absolute path (`/x`, `C:/x`), no `\`, no NUL, no `.` or `..` segment.
pub(crate) fn check_path(path: &str) -> Result<(), ApiError> {
    let bad = |m| Err(ApiError::bad_request(m));
    if path.is_empty() {
        return bad("empty path");
    }
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
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
    // `view` (the group the card is opened from) only matters once cards carry links.
}

/// The card of a file: what the index knows about it.
#[derive(Serialize)]
struct FileCardPayload {
    v: u8,
    seq: u64,
    kind: &'static str,
    card: FileCard,
    /// The file's arrows in the open view; filled once the map can tell.
    links: Option<Value>,
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
    let seq = shared.seq();
    let CardParams { path, symbol, .. } = params;
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
            let card = read(&shared, move |s| file_card(s, &path)).await?;
            let card = card.ok_or_else(ApiError::not_found)?;
            Ok(Json(FileCardPayload {
                v: 1,
                seq,
                kind: "file",
                card,
                links: None,
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
    let q = params.q.unwrap_or_default();
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
