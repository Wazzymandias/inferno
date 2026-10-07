//! Inspect local preparation using the selected model's cached configuration.

use std::{error::Error, io, num::NonZeroU64, time::Duration};

use bpaf::Bpaf;

use super::{inference_api_key, inference_endpoint, inference_model, inference_timeout};
use crate::{backend::Backend, inference};
use reqwest::Url;

#[derive(Debug, Bpaf)]
#[bpaf(generate(render_command))]
pub(crate) struct RenderCommand {
    #[bpaf(external(inference_model))]
    model: String,
    #[bpaf(external(inference_endpoint))]
    endpoint: Url,
    #[bpaf(external(inference_api_key))]
    api_key: Option<reqwest::header::HeaderValue>,

    #[bpaf(external(inference_timeout))]
    timeout: NonZeroU64,
}

impl RenderCommand {
    pub(crate) async fn execute(self) -> Result<(), Box<dyn Error>> {
        let backend = Backend::new(
            self.endpoint,
            Duration::from_secs(self.timeout.get()),
            self.api_key,
        )?;
        let config = inference::ModelConfig::for_inspection(&backend, &self.model).await?;
        let processor = inference::InputProcessor::load(config)?;
        let request: inference::CreateResponseRequest = serde_json::from_reader(io::stdin().lock())
            .map_err(|error| {
                inference::InputError::with_source(
                    "request",
                    "invalid Responses request on stdin",
                    Box::new(error),
                )
            })?;
        let input = processor.prepare(&request)?;
        // Explicit inspection output, never part of server runtime logging.
        let hashes: Vec<String> = input
            .prefix_hashes()
            .iter()
            .map(|hash| {
                use std::fmt::Write;
                hash.iter()
                    .fold(String::with_capacity(64), |mut text, byte| {
                        write!(&mut text, "{byte:02x}").expect("writing to a String cannot fail");
                        text
                    })
            })
            .collect();
        serde_json::to_writer(
            io::stdout().lock(),
            &serde_json::json!({
                "token_ids": input.tokens(), "prefix_hashes": hashes,
            }),
        )?;
        Ok(())
    }
}
