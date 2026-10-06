//! Request data available before backend selection.

use std::io;

use super::request::CreateResponseRequest;

#[derive(Debug)]
pub(crate) enum ModelInput<'a> {
    /// Routes that do not yet parse model input.
    Unspecified,
    Responses(&'a CreateResponseRequest),
}

impl<'a> ModelInput<'a> {
    pub(crate) const fn from_responses(request: &'a CreateResponseRequest) -> Self {
        Self::Responses(request)
    }

    /// Check API requirements before a request reaches the backend pool.
    pub(crate) fn validate(&self) -> io::Result<()> {
        let Self::Responses(request) = self else {
            return Ok(());
        };
        if request.conversation.is_some() && request.previous_response_id.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Do not use conversation and previous_response_id in the same request.",
            ));
        }
        if request.stream_options.is_some() && request.stream != Some(true) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Set stream to true when you use stream_options.",
            ));
        }
        Ok(())
    }
}
