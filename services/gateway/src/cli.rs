//! Command-line interface for the gateway.

mod render;
mod serve;

use std::{error::Error, num::NonZeroU64};

use bpaf::{Bpaf, Parser};
use reqwest::Url;

pub(crate) use render::RenderCommand;
use render::render_command;
pub(crate) use serve::ServeCommand;
use serve::serve_command;

/// Infergate: forward Responses requests, or inspect their input token IDs.
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

fn inference_timeout() -> impl Parser<NonZeroU64> {
    bpaf::long("inference-timeout-seconds")
        .env("INFERENCE_TIMEOUT_SECONDS")
        .help("Whole backend response deadline, including streaming")
        .argument::<NonZeroU64>("SECONDS")
        .fallback(NonZeroU64::new(300).unwrap())
}

#[cfg(test)]
mod tests {
    use super::{GatewayCommand, options};

    #[test]
    fn parser_invariants() {
        options().check_invariants(false);
    }

    #[test]
    fn serve_requires_an_explicit_subcommand() {
        let arguments = [
            "serve",
            "--api-address",
            "127.0.0.1",
            "--api-port",
            "0",
            "--inference-endpoint",
            "http://127.0.0.1:8001/v1",
        ];
        let command = options().run_inner(arguments.as_slice()).unwrap();
        assert!(matches!(command, GatewayCommand::Serve(_)));
        assert!(options().run_inner(&arguments[1..]).is_err());
    }

    #[test]
    fn render_requires_no_listener_configuration_or_local_model() {
        let command = options()
            .run_inner(&["render", "--inference-endpoint", "http://127.0.0.1:8001/v1"])
            .unwrap();
        assert!(matches!(command, GatewayCommand::Render(_)));
    }
}
