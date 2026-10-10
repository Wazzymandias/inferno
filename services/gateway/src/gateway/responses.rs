//! Responses API request processing and backend dispatch.

use std::sync::Arc;

use axum::{
    http::{HeaderMap, HeaderValue, header},
    response::Response,
};

use super::{Gateway, routes::upstream_response};
use crate::inference::{CreateResponseRequest, InputError, ModelInput};

impl Gateway {
    /// Forward a validated request using [`Gateway::select`] for backend choice.
    /// Retain the serialized body while preparation owns the parsed request;
    /// preparation failures prevent dispatch even if serialization also failed.
    pub(super) async fn create_response(
        &self,
        query: Option<&str>,
        mut headers: HeaderMap,
        request: CreateResponseRequest,
    ) -> Result<Response, InputError> {
        let body = serde_json::to_vec(&request);
        let input = self.prepare_input(request).await?;
        let (backend, lease) = self.select(&input).ok_or(InputError::NoBackend)?;
        headers
            .entry(header::CONTENT_TYPE)
            .or_insert(HeaderValue::from_static("application/json"));

        let response = backend
            .client
            .post(backend.url("responses", query))
            .headers(headers)
            .body(body?)
            .send()
            .await?;
        Ok(upstream_response(response, lease))
    }

    /// Prepare [`inference::ModelInput`](crate::inference::ModelInput) without
    /// blocking the async executor. The worker consumes the parsed request and
    /// can finish after its awaiting handler is cancelled.
    async fn prepare_input(
        &self,
        request: CreateResponseRequest,
    ) -> Result<ModelInput, InputError> {
        let processor = Arc::clone(&self.processor);
        tokio::task::spawn_blocking(move || processor.prepare(&request)).await?
    }
}
