//! HTTP gateway for forwarding inference requests and streaming backend responses.

use std::{error::Error, time::Duration};

use backend::Backend;
use gateway::Gateway;

mod backend;
mod cli;
mod gateway;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = cli::options().run();
    let backend = Backend::new(config.endpoint, Duration::from_secs(config.timeout.get()))?;
    Gateway::new(config.address, config.port)
        .with_backend(backend)
        .with_limits(
            config.body_limit.get(),
            Duration::from_secs(config.shutdown_timeout.get()),
        )
        .serve()
        .await?;
    Ok(())
}
