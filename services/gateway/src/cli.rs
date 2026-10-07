//! Command-line interface for the gateway.

mod render;
mod serve;

use std::{error::Error, num::NonZeroU64};

use bpaf::{Bpaf, Parser};
use reqwest::{Url, header::HeaderValue};

pub(crate) use render::RenderCommand;
use render::render_command;
pub(crate) use serve::ServeCommand;
use serve::serve_command;

/// Inferno: forward Responses requests, or inspect locally prepared tokens and prefix hashes.
#[derive(Debug, Bpaf)]
#[bpaf(options, generate(options), version)]
pub(crate) enum GatewayCommand {
    /// Run the inference gateway
    #[bpaf(command("serve"))]
    Serve(#[bpaf(external(serve_command))] ServeCommand),
    /// Render a Responses request from stdin without generating a response
    #[bpaf(command("render"))]
    Render(#[bpaf(external(render_command))] RenderCommand),
}

impl GatewayCommand {
    pub(crate) fn new() -> Self {
        options().run()
    }

    pub(crate) async fn execute(self) -> Result<(), Box<dyn Error>> {
        match self {
            Self::Serve(command) => command.execute().await,
            Self::Render(command) => command.execute().await,
        }
    }
}

fn inference_model() -> impl Parser<String> {
    bpaf::long("model")
        .env("INFERENCE_MODEL")
        .help("Model served by the configured inference backend (required)")
        .argument::<String>("MODEL")
        .guard(
            |name| !name.trim().is_empty(),
            "inference model must not be empty",
        )
}

fn inference_endpoint() -> impl Parser<Url> {
    bpaf::long("inference-endpoint")
        .env("INFERENCE_ENDPOINT")
        .help("OpenAI-compatible HTTP(S) API endpoint (required)")
        .argument::<Url>("URL")
        .guard(
            |url| {
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.query().is_none()
                    && url.fragment().is_none()
            },
            "inference endpoint must be HTTP(S), without a query or fragment",
        )
}

// Environment-only credentials stay out of process arguments and Debug output.
fn inference_api_key() -> impl Parser<Option<HeaderValue>> {
    bpaf::env("INFERENCE_API_KEY")
        .help("Optional backend bearer token for startup discovery and forwarding")
        .argument::<String>("TOKEN")
        .optional()
        .parse(|key| -> Result<_, reqwest::header::InvalidHeaderValue> {
            key.filter(|key| !key.is_empty())
                .map(|key| {
                    let mut value = HeaderValue::from_str(&format!("Bearer {key}"))?;
                    value.set_sensitive(true);
                    Ok(value)
                })
                .transpose()
        })
}

fn inference_timeout() -> impl Parser<NonZeroU64> {
    bpaf::long("inference-timeout-seconds")
        .env("INFERENCE_TIMEOUT_SECONDS")
        .help("Whole backend response deadline, including streaming")
        .argument::<NonZeroU64>("SECONDS")
        .fallback(NonZeroU64::new(300).unwrap())
}

#[cfg(test)]
mod tests {
    use super::options;

    #[test]
    fn parser_invariants() {
        options().check_invariants(false);
    }
}
