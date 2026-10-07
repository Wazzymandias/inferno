//! Errors shared by local input preparation and Responses request dispatch.

use std::{error::Error, fmt};

use axum::extract::rejection::JsonRejection;

/// Display and Debug omit source diagnostics, which can contain request data.
/// The original typed cause remains available through `Error::source`.
pub(crate) enum InputError {
    InvalidJson(JsonRejection),
    InvalidRequest {
        field: String,
        reason: &'static str,
        source: Option<Box<dyn Error + Send + Sync>>,
    },
    PreparationFailed(Box<dyn Error + Send + Sync>),
    NoBackend,
    Backend(reqwest::Error),
}

impl InputError {
    pub(crate) fn new(field: impl Into<String>, reason: &'static str) -> Self {
        Self::InvalidRequest {
            field: field.into(),
            reason,
            source: None,
        }
    }

    pub(crate) fn with_source(
        field: impl Into<String>,
        reason: &'static str,
        source: Box<dyn Error + Send + Sync>,
    ) -> Self {
        Self::InvalidRequest {
            field: field.into(),
            reason,
            source: Some(source),
        }
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(_) => f.write_str("Invalid Responses request JSON."),
            Self::InvalidRequest { field, reason, .. } => write!(f, "{field}: {reason}"),
            Self::PreparationFailed(_) => f.write_str("Local input preparation failed."),
            Self::NoBackend => f.write_str("No inference backend is available."),
            Self::Backend(_) => f.write_str("The inference backend request failed."),
        }
    }
}

impl Error for InputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
            Self::InvalidRequest { source, .. } => source
                .as_deref()
                .map(|source| source as &(dyn Error + 'static)),
            Self::PreparationFailed(error) => Some(error.as_ref()),
            Self::Backend(error) => Some(error),
            Self::NoBackend => None,
        }
    }
}

impl fmt::Debug for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputError")
            .field("message", &format_args!("{self}"))
            .field("has_source", &self.source().is_some())
            .finish()
    }
}

impl From<JsonRejection> for InputError {
    fn from(error: JsonRejection) -> Self {
        Self::InvalidJson(error)
    }
}

impl From<tokio::task::JoinError> for InputError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::PreparationFailed(Box::new(error))
    }
}

impl From<reqwest::Error> for InputError {
    fn from(error: reqwest::Error) -> Self {
        Self::Backend(error)
    }
}
