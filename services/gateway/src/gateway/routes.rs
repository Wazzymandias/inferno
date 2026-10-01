//! HTTP routing and streaming proxy behavior.

use crate::backend::Backend;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{any, get},
};

pub(super) fn app(backend: Backend, body_limit: usize) -> Router {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/readyz", get(ready))
        .route("/v1/{*path}", any(forward))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(backend)
}

async fn ready(State(backend): State<Backend>) -> StatusCode {
    match backend.client.get(backend.url("models", None)).send().await {
        Ok(response) if response.status().is_success() => StatusCode::OK,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn forward(
    State(backend): State<Backend>,
    method: Method,
    uri: Uri,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri
        .path()
        .strip_prefix("/v1/")
        .expect("route has /v1/ prefix");
    strip_transport_headers(&mut headers);
    let response = match backend
        .client
        .request(method, backend.url(path, uri.query()))
        .headers(headers)
        .body(body)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let error = error.without_url();
            println!("inference request failed: {error}");
            let status = if error.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            };
            return (
                status,
                [(header::CONTENT_TYPE, "application/json")],
                r#"{"error":{"message":"inference backend unavailable"}}"#,
            )
                .into_response();
        }
    };

    let status = response.status();
    let mut headers = response.headers().clone();
    strip_transport_headers(&mut headers);
    // Forward chunks as they arrive so token streaming remains incremental.
    let mut outgoing = Response::new(Body::new(reqwest::Body::from(response)));
    *outgoing.status_mut() = status;
    *outgoing.headers_mut() = headers;
    outgoing
}

pub(super) fn strip_transport_headers(headers: &mut HeaderMap) {
    let nominated: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_owned())
        .collect();
    for name in nominated {
        headers.remove(name);
    }
    // Each HTTP connection has its own framing and authority.
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        "content-length",
    ] {
        headers.remove(name);
    }
}
