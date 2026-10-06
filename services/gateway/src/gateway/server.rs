//! HTTP serving and bounded request draining.

use std::{future::IntoFuture, sync::Arc};

use tokio_util::sync::CancellationToken;

use super::{Gateway, routes};

impl Gateway {
    pub(super) async fn serve_http(
        self: Arc<Self>,
        listener: tokio::net::TcpListener,
        shutdown: CancellationToken,
    ) -> std::io::Result<()> {
        let router = routes::app(Arc::clone(&self), self.body_limit);
        let server = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.clone().cancelled_owned());
        tokio::select! {
            result = server.into_future() => result,
            _ = self.wait_for_shutdown_deadline(&shutdown) => {
                println!("shutdown deadline reached; terminating remaining requests");
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "shutdown deadline reached",
                ))
            }
        }
    }

    async fn wait_for_shutdown_deadline(&self, shutdown: &CancellationToken) {
        shutdown.cancelled().await;
        println!("shutdown requested; draining requests");
        tokio::time::sleep(self.shutdown_timeout).await;
    }
}
