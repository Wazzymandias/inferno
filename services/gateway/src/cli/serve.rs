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

use super::{inference_endpoint, inference_timeout};
use crate::{backend::Pool, gateway::Gateway};

/// Infergate: an OpenAI-compatible inference gateway. Flags override environment variables.
#[derive(Clone, Debug, Bpaf)]
#[bpaf(generate(serve_command))]
pub(crate) struct ServeCommand {
    /// Listener IP address (required)
    #[bpaf(long("api-address"), env("API_ADDRESS"), argument("IP"))]
    address: IpAddr,

    /// Listener port (required; 0 selects an available port)
    #[bpaf(long("api-port"), env("API_PORT"), argument("PORT"))]
    port: u16,

    /// OpenAI-compatible HTTP(S) API endpoint (required)
    #[bpaf(external(inference_endpoint))]
    endpoint: Url,

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
        let mut pool = Pool::new();
        pool.add(self.endpoint, Duration::from_secs(self.timeout.get()))?;
        Gateway::new(self.address, self.port)
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
