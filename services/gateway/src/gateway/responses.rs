//! Responses API request processing and backend dispatch.

use std::io;

use axum::{extract::rejection::JsonRejection, http::HeaderMap};

use super::{Gateway, SelectionError};
use crate::inference::{CreateResponseRequest, ModelInput};

#[derive(Debug)]
pub(super) enum CreateResponseError {
    InvalidJson(JsonRejection),
    InvalidRequest(io::Error),
    NoBackend,
    Backend(reqwest::Error),
}

impl From<JsonRejection> for CreateResponseError {
    fn from(error: JsonRejection) -> Self {
        Self::InvalidJson(error)
    }
}

impl From<SelectionError> for CreateResponseError {
    fn from(error: SelectionError) -> Self {
        match error {
            SelectionError::InvalidRequest(error) => Self::InvalidRequest(error),
            SelectionError::NoBackend => Self::NoBackend,
        }
    }
}

impl Gateway {
    pub(super) async fn create_response(
        &self,
        query: Option<&str>,
        headers: HeaderMap,
        request: CreateResponseRequest,
    ) -> Result<reqwest::Response, CreateResponseError> {
        let input = ModelInput::from(&request);
        let backend = self.select(input, &self.pool).await?;

        backend
            .client
            .post(backend.url("responses", query))
            .headers(headers)
            .json(&request)
            .send()
            .await
            .map_err(CreateResponseError::Backend)
    }
}
