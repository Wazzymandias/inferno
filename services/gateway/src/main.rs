//! HTTP gateway for forwarding inference requests and streaming backend responses.

use std::{error::Error, time::Duration};

use backend::Pool;
use gateway::Gateway;
use tokio_util::sync::CancellationToken;

mod backend;
mod cli;
mod gateway;
mod inference;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = cli::options().run();
    let mut pool = Pool::new();
    pool.add(config.endpoint, Duration::from_secs(config.timeout.get()))?;
    Gateway::new(config.address, config.port)
        .with_pool(pool)
        .with_limits(
            config.body_limit.get(),
            Duration::from_secs(config.shutdown_timeout.get()),
        )
        .serve(shutdown_signal)
        .await?;
    Ok(())
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
