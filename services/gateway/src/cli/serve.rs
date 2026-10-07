//! Serve command: gateway configuration and process lifecycle.

use std::{
    error::Error,
    net::IpAddr,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use bpaf::Bpaf;
use reqwest::Url;
use tokio_util::sync::CancellationToken;

use super::{inference_api_key, inference_endpoints, inference_model, inference_timeout};
use crate::{backend::Pool, gateway::Gateway, inference::InputProcessor};

/// Inferno: an OpenAI-compatible inference gateway. Flags override environment variables.
#[derive(Clone, Debug, Bpaf)]
#[bpaf(generate(serve_command))]
pub(crate) struct ServeCommand {
    #[bpaf(external(inference_model))]
    model: String,

    /// Listener IP address (required)
    #[bpaf(long("api-address"), env("API_ADDRESS"), argument("IP"))]
    address: IpAddr,

    /// Listener port (required; 0 selects an available port)
    #[bpaf(long("api-port"), env("API_PORT"), argument("PORT"))]
    port: u16,

    /// OpenAI-compatible HTTP(S) API endpoint (required)
    #[bpaf(external(inference_endpoints))]
    endpoints: Vec<Url>,

    /// Cached-token cost of each active request, including streaming
    #[bpaf(long("routing-load-penalty"), env("ROUTING_LOAD_PENALTY"), argument("TOKENS"), fallback(NonZeroUsize::new(256).unwrap()))]
    load_penalty: NonZeroUsize,

    #[bpaf(external(inference_api_key))]
    api_key: Option<reqwest::header::HeaderValue>,

    /// Whole backend response deadline, including streaming
    #[bpaf(external(inference_timeout))]
    timeout: NonZeroU64,

    /// Maximum buffered request body size
    #[bpaf(
        long("max-request-bytes"),
        env("MAX_REQUEST_BYTES"),
        argument("BYTES"),
        fallback(NonZeroUsize::new(1_048_576).unwrap())
    )]
    body_limit: NonZeroUsize,

    /// Maximum time to drain requests after SIGTERM or Ctrl-C
    #[bpaf(
        long("shutdown-timeout-seconds"),
        env("SHUTDOWN_TIMEOUT_SECONDS"),
        argument("SECONDS"),
        fallback(NonZeroU64::new(5).unwrap())
    )]
    shutdown_timeout: NonZeroU64,
}

impl ServeCommand {
    pub(crate) async fn execute(self) -> Result<(), Box<dyn Error>> {
        let (pool, config) = Pool::connect(
            self.endpoints,
            &self.model,
            Duration::from_secs(self.timeout.get()),
            self.api_key,
            self.load_penalty.get(),
        )
        .await?;
        let processor = InputProcessor::load(config)?;
        Gateway::new(self.address, self.port, processor)
            .with_pool(pool)
            .with_limits(
                self.body_limit.get(),
                Duration::from_secs(self.shutdown_timeout.get()),
            )
            .serve(shutdown_signal)
            .await?;
        Ok(())
    }
}

async fn shutdown_signal(shutdown: CancellationToken) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    shutdown.cancel();
    Ok(())
}
