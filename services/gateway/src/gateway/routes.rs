//! HTTP routing and streaming proxy behavior.

use crate::backend::RequestLease;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use super::Gateway;
use crate::inference::{CreateResponseRequest, InputError};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};

pub(super) fn app(gateway: Arc<Gateway>, body_limit: usize) -> Router {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/readyz", get(ready))
        .route(
            "/v1/responses",
            post(create_response).fallback(|| async {
                responses_error(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "invalid_request_error",
                    "Use POST to create a response.",
                )
            }),
        )
        .route("/v1/{*path}", any(forward))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(gateway)
}

async fn ready(State(gateway): State<Arc<Gateway>>) -> StatusCode {
    let Some((backend, _lease)) = gateway.pool.rank(None) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match backend.client.get(backend.url("models", None)).send().await {
        Ok(response) if response.status().is_success() => StatusCode::OK,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn forward(
    State(gateway): State<Arc<Gateway>>,
    method: Method,
    uri: Uri,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some((backend, lease)) = gateway.pool.rank(None) else {
        return backend_unavailable(StatusCode::BAD_GATEWAY);
    };
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
            return backend_unavailable(backend_error_status(error));
        }
    };

    upstream_response(response, lease)
}

async fn create_response(
    State(gateway): State<Arc<Gateway>>,
    uri: Uri,
    mut headers: HeaderMap,
    request: Result<Json<CreateResponseRequest>, JsonRejection>,
) -> Result<Response, InputError> {
    let Json(request) = request?;
    strip_transport_headers(&mut headers);
    gateway.create_response(uri.query(), headers, request).await
}

impl IntoResponse for InputError {
    fn into_response(self) -> Response {
        if matches!(&self, Self::PreparationFailed(_)) {
            // Error::source retains the typed cause. Its diagnostics may contain
            // request data, so events use the operation's safe Display text.
            println!(
                "{}",
                serde_json::json!({
                    "event": "input_preparation.failed", "message": self.to_string(),
                })
            );
        }
        let status = match self {
            Self::InvalidJson(error) => {
                let status = match &error {
                    JsonRejection::JsonDataError(_) => StatusCode::BAD_REQUEST,
                    _ => error.status(),
                };
                return responses_error(status, "invalid_request_error", &error.body_text());
            }
            Self::InvalidRequest { .. } => {
                return responses_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &self.to_string(),
                );
            }
            Self::PreparationFailed(_) => {
                return responses_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    &self.to_string(),
                );
            }
            Self::NoBackend => StatusCode::SERVICE_UNAVAILABLE,
            Self::Backend(error) if error.is_builder() => {
                return responses_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "The gateway could not build the backend request.",
                );
            }
            Self::Backend(error) => backend_error_status(error),
        };
        responses_error(
            status,
            "server_error",
            "The inference backend is not available.",
        )
    }
}

fn responses_error(status: StatusCode, error_type: &str, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::json!({
            "error": {
                "message": message,
                "type": error_type,
                "param": null,
                "code": null,
            }
        })
        .to_string(),
    )
        .into_response()
}

fn backend_error_status(error: reqwest::Error) -> StatusCode {
    let error = error.without_url();
    println!("inference request failed: {error}");
    if error.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

pub(super) fn upstream_response(response: reqwest::Response, lease: RequestLease) -> Response {
    let status = response.status();
    let mut headers = response.headers().clone();
    strip_transport_headers(&mut headers);
    // Forward chunks as they arrive so token streaming remains incremental.
    let mut outgoing = Response::new(Body::new(UpstreamBody {
        body: reqwest::Body::from(response),
        lease: Some(lease),
    }));
    *outgoing.status_mut() = status;
    *outgoing.headers_mut() = headers;
    outgoing
}

struct UpstreamBody {
    body: reqwest::Body,
    lease: Option<RequestLease>,
}

impl http_body::Body for UpstreamBody {
    type Data = Bytes;
    type Error = reqwest::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(cx);
        if matches!(frame, Poll::Ready(None | Some(Err(_)))) || self.body.is_end_stream() {
            self.lease.take();
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }
}

fn backend_unavailable(status: StatusCode) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":{"message":"inference backend unavailable"}}"#,
    )
        .into_response()
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
