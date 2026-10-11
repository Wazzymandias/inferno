//! Gateway listener, routing, and shutdown lifecycle.

use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use crate::inference;
use tokio_util::sync::CancellationToken;

pub(crate) mod pool;
mod responses;
mod routes;
mod server;

#[cfg(test)]
mod tests;

pub(crate) use pool::{Pool, RequestLease};

/// Serve inference requests with prepared model input and the backend pool
/// attached through [`Gateway::with_pool`]. [`Gateway::serve`] rejects a missing
/// or empty pool before opening its HTTP listener.
#[derive(Debug)]
pub(crate) struct Gateway {
    address: SocketAddr,
    pool: Option<Pool>,
    processor: Arc<inference::InputProcessor>,
    body_limit: usize,
    shutdown_timeout: Duration,
}

impl Gateway {
    /// Configure the HTTP listener with an owned [`inference::InputProcessor`].
    /// Attach the serving backends through [`Gateway::with_pool`] before serving.
    pub(crate) fn new(address: IpAddr, port: u16, processor: inference::InputProcessor) -> Self {
        Self {
            address: SocketAddr::new(address, port),
            pool: None,
            processor: Arc::new(processor),
            body_limit: 1_048_576,
            shutdown_timeout: Duration::from_secs(5),
        }
    }

    /// Attach the [`Pool`] used for request routing and cache-event observation.
    pub(crate) fn with_pool(mut self, pool: Pool) -> Self {
        self.pool = Some(pool);
        self
    }

    /// Set the buffered request limit and the deadline for draining requests
    /// during shutdown. These limits apply to the entire served HTTP application.
    pub(crate) const fn with_limits(
        mut self,
        body_limit: usize,
        shutdown_timeout: Duration,
    ) -> Self {
        self.body_limit = body_limit;
        self.shutdown_timeout = shutdown_timeout;
        self
    }

    /// Serve HTTP and observe cache events until the signal handler cancels
    /// shutdown. A missing or empty [`Pool`] fails before binding; active requests
    /// drain up to the deadline configured through [`Gateway::with_limits`].
    pub(crate) async fn serve<F>(
        self,
        signal_handler: impl FnOnce(CancellationToken) -> F,
    ) -> io::Result<()>
    where
        F: Future<Output = io::Result<()>>,
    {
        let gateway = Arc::new(self);
        let pool = gateway
            .pool
            .as_ref()
            .filter(|pool| !pool.is_empty())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "gateway requires a backend")
            })?;
        let listener = tokio::net::TcpListener::bind(gateway.address).await?;
        println!("inferno listening on {}", listener.local_addr()?);
        let shutdown = CancellationToken::new();
        tokio::try_join!(
            Arc::clone(&gateway).serve_http(listener, shutdown.clone()),
            pool.subscribe_events(shutdown.clone()),
            signal_handler(shutdown),
        )?;
        Ok(())
    }
}
