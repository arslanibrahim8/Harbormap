//! The panel bundle (`ui/learn/dist`), embedded in the binary so `harbormap learn`
//! needs no files next to it.

use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::api::ApiError;

#[derive(RustEmbed)]
#[folder = "../../ui/learn/dist/"]
struct Dist;

/// The content type for a file name, from its extension.
fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The embedded file `name`, or `None`.
fn serve(name: &str) -> Option<Response> {
    let file = Dist::get(name)?;
    Some(
        (
            [(header::CONTENT_TYPE, content_type(name))],
            file.data.into_owned(),
        )
            .into_response(),
    )
}

/// `GET /`: the panel page.
pub(crate) async fn index() -> Response {
    serve("index.html").unwrap_or_else(|| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the panel bundle is missing",
        )
        .into_response()
    })
}

/// `GET /assets/{*file}`: a file of the bundle.
pub(crate) async fn asset(Path(file): Path<String>) -> Response {
    serve(&file)
        .filter(|_| file != "SOURCE_HASH")
        .unwrap_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no such asset").into_response())
}
