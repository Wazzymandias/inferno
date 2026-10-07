//! Exercise CLI and environment handling in isolated processes.

use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_infergate"));
    for name in [
        "API_ADDRESS",
        "API_PORT",
        "INFERENCE_ENDPOINT",
        "INFERENCE_MODEL",
        "INFERENCE_API_KEY",
        "INFERENCE_TIMEOUT_SECONDS",
        "MAX_REQUEST_BYTES",
        "SHUTDOWN_TIMEOUT_SECONDS",
    ] {
        command.env_remove(name);
    }
    command
}

fn configured() -> Command {
    let mut command = command();
    command
        .arg("serve")
        .env("INFERENCE_MODEL", "fixture")
        .env("API_ADDRESS", "127.0.0.1")
        .env("API_PORT", "0")
        .env("INFERENCE_ENDPOINT", "http://localhost/v1");
    command
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn invalid_backend_credentials_fail_without_exposing_the_secret() {
    let output = configured()
        .env("INFERENCE_API_KEY", "private-token\ninvalid-header")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!stderr(&output).contains("private-token"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-token"));
}
