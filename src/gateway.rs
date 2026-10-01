//! Gateway listener, routing, and shutdown lifecycle.

use std::{
    io,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use crate::backend::Backend;

mod routes;
mod shutdown;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) struct Gateway {
    address: SocketAddr,
    backend: Option<Backend>,
    body_limit: usize,
    shutdown_timeout: Duration,
}

impl Gateway {
    pub(crate) const fn new(address: IpAddr, port: u16) -> Self {
        Self {
            address: SocketAddr::new(address, port),
            backend: None,
            body_limit: 1_048_576,
            shutdown_timeout: Duration::from_secs(5),
        }
    }

    pub(crate) fn with_backend(mut self, backend: Backend) -> Self {
        self.backend = Some(backend);
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

    pub(crate) async fn serve(self) -> io::Result<()> {
        let backend = self.backend.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "gateway requires a backend")
        })?;
        let listener = tokio::net::TcpListener::bind(self.address).await?;
        println!("infergate listening on {}", listener.local_addr()?);
        shutdown::serve(
            listener,
            routes::app(backend, self.body_limit),
            shutdown::shutdown(),
            self.shutdown_timeout,
        )
        .await
    }
}
