//! Responses API request processing and backend dispatch.

use std::sync::Arc;

use axum::{http::HeaderMap, response::Response};

use super::{Gateway, routes::upstream_response};
use crate::inference::{CreateResponseRequest, InputError, ModelInput};

impl Gateway {
    pub(super) async fn create_response(
        &self,
        query: Option<&str>,
        headers: HeaderMap,
        request: CreateResponseRequest,
    ) -> Result<Response, InputError> {
        let (request, input) = self.prepare_input(request).await?;
        let (backend, lease) = self.select(&input).ok_or(InputError::NoBackend)?;

        let response = backend
            .client
            .post(backend.url("responses", query))
            .headers(headers)
            .json(&request)
            .send()
            .await?;
        Ok(upstream_response(response, lease))
    }

    /// Run CPU preparation outside the async executor, moving the request
    /// through the worker and returning it intact for backend dispatch.
    /// The worker owns its captures because it can outlive a cancelled handler.
    async fn prepare_input(
        &self,
        request: CreateResponseRequest,
    ) -> Result<(CreateResponseRequest, ModelInput), InputError> {
        let processor = Arc::clone(&self.processor);
        tokio::task::spawn_blocking(move || {
            let input = processor.prepare(&request)?;
            Ok((request, input))
        })
        .await?
    }
}
