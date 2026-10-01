//! Command-line and environment configuration for the gateway.

use std::{
    net::IpAddr,
    num::{NonZeroU64, NonZeroUsize},
};

use bpaf::Bpaf;
use reqwest::Url;

/// Infergate: an OpenAI-compatible inference gateway. Flags override environment variables.
#[derive(Clone, Debug, Bpaf)]
#[bpaf(options, generate(options), version)]
pub(crate) struct Config {
    /// Listener IP address (required)
    #[bpaf(long("api-address"), env("API_ADDRESS"), argument("IP"))]
    pub(crate) address: IpAddr,

    /// Listener port (required; 0 selects an available port)
    #[bpaf(long("api-port"), env("API_PORT"), argument("PORT"))]
    pub(crate) port: u16,

    /// OpenAI-compatible HTTP(S) API endpoint (required)
    #[bpaf(
        long("inference-endpoint"),
        env("INFERENCE_ENDPOINT"),
        argument("URL"),
        guard(
            |url| matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.query().is_none()
                && url.fragment().is_none(),
            "inference endpoint must be HTTP(S), without a query or fragment"
        )
    )]
    pub(crate) endpoint: Url,

    /// Whole backend response deadline, including streaming
    #[bpaf(
        long("inference-timeout-seconds"),
        env("INFERENCE_TIMEOUT_SECONDS"),
        argument("SECONDS"),
        fallback(NonZeroU64::new(300).unwrap())
    )]
    pub(crate) timeout: NonZeroU64,

    /// Maximum buffered request body size
    #[bpaf(
        long("max-request-bytes"),
        env("MAX_REQUEST_BYTES"),
        argument("BYTES"),
        fallback(NonZeroUsize::new(1_048_576).unwrap())
    )]
    pub(crate) body_limit: NonZeroUsize,

    /// Maximum time to drain requests after SIGTERM or Ctrl-C
    #[bpaf(
        long("shutdown-timeout-seconds"),
        env("SHUTDOWN_TIMEOUT_SECONDS"),
        argument("SECONDS"),
        fallback(NonZeroU64::new(5).unwrap())
    )]
    pub(crate) shutdown_timeout: NonZeroU64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_invariants() {
        options().check_invariants(false);
    }
}
