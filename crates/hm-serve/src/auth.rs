//! The two checks in front of every request: the `Host` header (so a web page on
//! another origin cannot reach the server through a rebinding trick) and the per-run
//! token (so another program on this machine cannot read the project).

use std::io;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, COOKIE, HOST, LOCATION, SET_COOKIE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::Shared;
use crate::api::ApiError;

/// The cookie that carries the token after the first visit. Cookies ignore the port,
/// so two panels on 127.0.0.1 would overwrite each other's cookie under one shared
/// name; each server therefore names its cookie after its own port.
fn cookie_name(port: u16) -> String {
    format!("hm_learn_{port}")
}

/// A new token: 16 random bytes as 32 lowercase hex characters.
pub(crate) fn new_token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Equality that takes the same time wherever the two differ.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

/// The value of the `t` parameter in a query string, as written (a token is hex).
fn query_token(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|pair| pair.strip_prefix("t="))
}

/// Every value of the cookie called `name` in the request.
fn cookie_tokens<'a>(req: &'a Request, name: &'a str) -> impl Iterator<Item = &'a str> {
    req.headers()
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(move |(n, _)| *n == name)
        .map(|(_, value)| value)
}

/// The bearer tokens of the request.
fn bearer_tokens(req: &Request) -> impl Iterator<Item = &str> {
    req.headers()
        .get_all(AUTHORIZATION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| {
            let (scheme, token) = v.split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
        })
}

/// Middleware: 403 for a foreign `Host`, then 401 without the token. `/?t=<token>`
/// sets the cookie and redirects to `/`, so the token leaves the address bar.
pub(crate) async fn guard(State(shared): State<Arc<Shared>>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|host| {
            [
                format!("127.0.0.1:{}", shared.port),
                format!("localhost:{}", shared.port),
            ]
            .iter()
            .any(|ok| host.eq_ignore_ascii_case(ok))
        });
    if !host_ok {
        return ApiError::new(StatusCode::FORBIDDEN, "unexpected Host header").into_response();
    }

    if req.uri().path() == "/"
        && query_token(req.uri().query()).is_some_and(|t| same(t, &shared.token))
    {
        let cookie = format!(
            "{}={}; HttpOnly; SameSite=Strict; Path=/",
            cookie_name(shared.port),
            shared.token
        );
        let mut res = StatusCode::SEE_OTHER.into_response();
        res.headers_mut()
            .insert(LOCATION, HeaderValue::from_static("/"));
        if let Ok(cookie) = HeaderValue::from_str(&cookie) {
            res.headers_mut().insert(SET_COOKIE, cookie);
        }
        return res;
    }

    let name = cookie_name(shared.port);
    let allowed = cookie_tokens(&req, &name)
        .chain(bearer_tokens(&req))
        .fold(false, |ok, t| same(t, &shared.token) | ok);
    if !allowed {
        return ApiError::new(StatusCode::UNAUTHORIZED, "missing or wrong token").into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_compares_whole_strings() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
        assert!(!same("", "a"));
    }

    #[test]
    fn query_token_finds_t_among_other_parameters() {
        assert_eq!(query_token(Some("a=1&t=xyz&b=2")), Some("xyz"));
        assert_eq!(query_token(Some("tt=1")), None);
        assert_eq!(query_token(None), None);
    }
}
