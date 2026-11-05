//! `/api/file`: the text of one indexed file, for the source view of a card.

use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use hm_index::view::indexed_file;
use hm_project::paths::within;
use serde::{Deserialize, Serialize};

use crate::Shared;
use crate::api::{ApiError, Params, check_path, read};

/// The most bytes of a file the panel is sent.
const MAX_BYTES: usize = 512 * 1024;

#[derive(Deserialize)]
pub(crate) struct FileParams {
    path: String,
}

/// A file's text.
#[derive(Serialize)]
pub(crate) struct SourceFile {
    v: u8,
    path: String,
    lang: Option<String>,
    text: String,
    /// Lines of `text`.
    lines: usize,
    /// The file is longer than what `text` holds.
    truncated: bool,
}

/// `GET /api/file?path=P`. A path is served only when the index has a row for it
/// (so never `.git/config` or an ignored file) and when it still leads to a file
/// inside the project (so never a symlink that points out of it).
pub(crate) async fn file(
    State(shared): State<Arc<Shared>>,
    Params(params): Params<FileParams>,
) -> Result<Json<SourceFile>, ApiError> {
    let path = params.path;
    check_path(&path)?;
    let rel = path.clone();
    let row = read(&shared, move |s| indexed_file(s, &rel))
        .await?
        .ok_or_else(ApiError::not_found)?;

    let root = shared.root.clone();
    let (text, truncated) = tokio::task::spawn_blocking(move || load(&root, &path))
        .await
        .map_err(ApiError::internal)??;
    Ok(Json(SourceFile {
        v: 1,
        path: row.path,
        lang: row.lang,
        lines: text.lines().count(),
        text,
        truncated,
    }))
}

/// The text of `rel` under `root`, cut at a line end when it is longer than
/// [`MAX_BYTES`], and whether it was cut. Bytes that are not UTF-8 become U+FFFD.
fn load(root: &Path, rel: &str) -> Result<(String, bool), ApiError> {
    let gone = |e: std::io::Error| match e.kind() {
        ErrorKind::NotFound | ErrorKind::NotADirectory => ApiError::not_found(),
        _ => ApiError::internal(e),
    };
    let canonical = dunce::canonicalize(root.join(rel)).map_err(gone)?;
    if !within(&canonical, root, cfg!(windows)) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "the file is outside the project",
        ));
    }
    // Only a regular file is read: opening a pipe would wait for a writer. The check is
    // made before opening (so a pipe is never opened) and again on the open handle (so
    // a swap in between is caught).
    let not_a_file = || ApiError::new(StatusCode::NOT_FOUND, "not a regular file");
    if !fs::metadata(&canonical).map_err(gone)?.is_file() {
        return Err(not_a_file());
    }
    let file = File::open(&canonical).map_err(gone)?;
    if !file.metadata().map_err(gone)?.is_file() {
        return Err(not_a_file());
    }
    // One byte more than the limit tells whether the file goes on.
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(gone)?;
    let truncated = bytes.len() > MAX_BYTES;
    if truncated {
        bytes.truncate(MAX_BYTES);
        // Keep whole lines. A file with no line end in its first 512 KiB is cut as is.
        if let Some(end) = bytes.iter().rposition(|&b| b == b'\n') {
            bytes.truncate(end + 1);
        }
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}
