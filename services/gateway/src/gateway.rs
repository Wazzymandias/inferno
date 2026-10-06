//! Gateway listener, routing, and shutdown lifecycle.

use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use crate::backend;
use crate::backend::Backend;
use crate::inference;
use tokio_util::sync::CancellationToken;

mod responses;
mod routes;
mod server;

use responses::CreateResponseError;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) enum SelectionError {
    InvalidRequest(io::Error),
    NoBackend,
}

#[derive(Debug)]
pub(crate) struct Gateway {
    address: SocketAddr,
    pool: backend::Pool,
    body_limit: usize,
    shutdown_timeout: Duration,
}

impl Gateway {
    pub(crate) const fn new(address: IpAddr, port: u16) -> Self {
        Self {
            address: SocketAddr::new(address, port),
            pool: backend::Pool::new(),
            body_limit: 1_048_576,
            shutdown_timeout: Duration::from_secs(5),
        }
    }

    pub(crate) fn with_pool(mut self, pool: backend::Pool) -> Self {
        self.pool = pool;
        self
    }

    pub(crate) const fn with_limits(
        mut self,
        body_limit: usize,
        shutdown_timeout: Duration,
    ) -> Self {
        self.body_limit = body_limit;
        self.shutdown_timeout = shutdown_timeout;
        self
    }

    /// Selects the first backend without consuming the pool.
    /// Validate the input first. Backend selection currently uses pool order.
    pub(crate) async fn select<'a>(
        &self,
        input: inference::ModelInput<'_>,
        pool: &'a backend::Pool,
    ) -> Result<&'a Backend, SelectionError> {
        input.validate().map_err(SelectionError::InvalidRequest)?;
        pool.first().ok_or(SelectionError::NoBackend)
    }

    pub(crate) async fn serve<F>(
        self,
        signal_handler: impl FnOnce(CancellationToken) -> F,
    ) -> io::Result<()>
    where
        F: Future<Output = io::Result<()>>,
    {
        if self.pool.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "gateway requires a backend",
            ));
        }
        let listener = tokio::net::TcpListener::bind(self.address).await?;
        println!("infergate listening on {}", listener.local_addr()?);
        let gateway = Arc::new(self);
        let shutdown = CancellationToken::new();
        tokio::try_join!(
            gateway.serve_http(listener, shutdown.clone()),
            signal_handler(shutdown),
        )?;
        Ok(())
    }
}
