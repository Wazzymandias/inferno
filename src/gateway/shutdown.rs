//! Signal handling and bounded request draining.

use std::time::Duration;

use axum::Router;

pub(super) async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    signal: impl Future<Output = ()> + Send + 'static,
    grace: Duration,
) -> std::io::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        signal.await;
        println!("shutdown requested; draining requests");
        let _ = tx.send(());
    });
    let server = std::future::IntoFuture::into_future(server);
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => return result,
        _ = rx => {}
    }
    tokio::time::timeout(grace, server)
        .await
        .unwrap_or_else(|_| {
            println!("shutdown deadline reached; terminating remaining requests");
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "shutdown deadline reached",
            ))
        })
}

pub(super) async fn shutdown() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler")
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install termination handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
