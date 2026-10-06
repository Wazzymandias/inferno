//! Request data available before backend selection.

use std::io;

use hf_chat_template::RenderInput;

use super::hugging_face::RenderInputError;
use super::request::CreateResponseRequest;

#[derive(Debug)]
pub(crate) enum ModelInput<'a> {
    /// Routes that do not yet parse model input.
    Unspecified,
    /// A borrowed Responses request, retained by its caller for forwarding.
    Responses(&'a CreateResponseRequest),
    /// An owned Responses request. Boxing keeps the routing input small.
    OwnedResponses(Box<CreateResponseRequest>),
}

impl<'a> From<&'a CreateResponseRequest> for ModelInput<'a> {
    fn from(request: &'a CreateResponseRequest) -> Self {
        Self::Responses(request)
    }
}

impl From<CreateResponseRequest> for ModelInput<'_> {
    fn from(request: CreateResponseRequest) -> Self {
        Self::OwnedResponses(Box::new(request))
    }
}

impl TryFrom<ModelInput<'_>> for RenderInput {
    type Error = RenderInputError;

    fn try_from(input: ModelInput<'_>) -> Result<Self, Self::Error> {
        input.render_input()
    }
}

impl ModelInput<'_> {
    /// Build input for a text-only Hugging Face chat template with string content.
    ///
    /// `ModelInput::from(&request).render_input()?` keeps the request available for
    /// forwarding. Conversion is fallible because backend-owned history and
    /// unsupported prompt content cannot be silently omitted. The model's
    /// template and special tokens belong to `hf_chat_template::ChatTemplate`,
    /// not to the API request.
    pub(crate) fn render_input(&self) -> Result<RenderInput, RenderInputError> {
        let request = self
            .request()
            .ok_or_else(|| RenderInputError::new("input", "no parsed model input"))?;
        RenderInput::try_from(request)
    }

    const fn request(&self) -> Option<&CreateResponseRequest> {
        match self {
            Self::Unspecified => None,
            Self::Responses(request) => Some(request),
            Self::OwnedResponses(request) => Some(request),
        }
    }

    /// Check API requirements before a request reaches the backend pool.
    pub(crate) fn validate(&self) -> io::Result<()> {
        let Some(request) = self.request() else {
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
