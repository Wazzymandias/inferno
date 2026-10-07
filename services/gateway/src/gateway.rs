//! Gateway listener, routing, and shutdown lifecycle.

use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use crate::backend;
use crate::backend::{Backend, RequestLease};
use crate::inference;
use tokio_util::sync::CancellationToken;

mod responses;
mod routes;
mod server;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) struct Gateway {
    address: SocketAddr,
    pool: backend::Pool,
    processor: Arc<inference::InputProcessor>,
    body_limit: usize,
    shutdown_timeout: Duration,
}

impl Gateway {
    pub(crate) fn new(address: IpAddr, port: u16, processor: inference::InputProcessor) -> Self {
        Self {
            address: SocketAddr::new(address, port),
            pool: backend::Pool::new(),
            processor: Arc::new(processor),
            body_limit: 1_048_576,
            shutdown_timeout: Duration::from_secs(5),
        }
    }

    pub(crate) fn with_pool(mut self, pool: backend::Pool) -> Self {
        self.pool = pool;
        self
    }

    async fn subscribe_events(&self, shutdown: CancellationToken) -> io::Result<()> {
        futures_util::future::try_join_all(
            self.pool
                .subscriptions()
                .iter()
                .enumerate()
                .map(|(id, subscription)| subscription.run(&self.pool, id, shutdown.clone())),
        )
        .await?;
        Ok(())
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

    pub(crate) fn select(&self, input: &inference::ModelInput) -> Option<(&Backend, RequestLease)> {
        self.pool.rank(Some(input))
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
        println!("inferno listening on {}", listener.local_addr()?);
        let gateway = Arc::new(self);
        let shutdown = CancellationToken::new();
        tokio::try_join!(
            Arc::clone(&gateway).serve_http(listener, shutdown.clone()),
            gateway.subscribe_events(shutdown.clone()),
            signal_handler(shutdown),
        )?;
        Ok(())
    }
}
