//! Render command: inspect backend input token IDs without generating a reply.

use std::{error::Error, io, num::NonZeroU64, path::PathBuf, time::Duration};

use bpaf::Bpaf;
use reqwest::Url;

use super::{inference_endpoint, inference_timeout};
use crate::{backend::Backend, inference};

#[derive(Debug, Bpaf)]
#[bpaf(generate(render_command))]
pub(crate) struct RenderCommand {
    /// Exact backend model snapshot; required only without a native render API
    #[bpaf(long("model-directory"), argument("PATH"))]
    model_directory: Option<PathBuf>,
    #[bpaf(external(inference_endpoint))]
    endpoint: Url,
    #[bpaf(external(inference_timeout))]
    timeout: NonZeroU64,
}

impl RenderCommand {
    pub(crate) async fn execute(self) -> Result<(), Box<dyn Error>> {
        let request: inference::CreateResponseRequest = serde_json::from_reader(io::stdin().lock())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid Responses request on stdin",
                )
            })?;
        let backend = Backend::new(self.endpoint, Duration::from_secs(self.timeout.get()))?;
        let input = inference::ModelInput::from(&request);
        let token_ids = backend
            .tokenize_response(&input, self.model_directory.as_deref())
            .await?;
        // Explicit command output, never part of server runtime logging.
        serde_json::to_writer(
            io::stdout().lock(),
            &serde_json::json!({ "token_ids": token_ids }),
        )?;
        Ok(())
    }
}
