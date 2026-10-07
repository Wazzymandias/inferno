//! HTTP gateway for forwarding inference requests and streaming backend responses.

use std::error::Error;

mod backend;
mod cli;
mod events;
mod gateway;
mod inference;
mod telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let command = cli::GatewayCommand::new();
    command.execute().await.inspect_err(|_| {
        println!("{}", serde_json::json!({ "event": "command.failed" }));
    })
}
