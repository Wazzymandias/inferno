//! Exercise CLI and environment handling in isolated processes.

use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_infergate"));
    for name in [
        "API_ADDRESS",
        "API_PORT",
        "INFERENCE_ENDPOINT",
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
        .env("API_ADDRESS", "127.0.0.1")
        .env("API_PORT", "0")
        .env("INFERENCE_ENDPOINT", "http://localhost/v1");
    command
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_and_version_need_no_configuration() {
    let help = command().arg("--help").output().unwrap();
    assert!(help.status.success(), "{}", stderr(&help));
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("serve"));
    assert!(text.contains("render"));
    let help = command().args(["serve", "--help"]).output().unwrap();
    assert!(help.status.success(), "{}", stderr(&help));
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--shutdown-timeout-seconds"));
    assert!(text.contains("INFERENCE_ENDPOINT"));
    let version = command().arg("--version").output().unwrap();
    assert!(version.status.success());
    assert!(
        String::from_utf8(version.stdout)
            .unwrap()
            .contains(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rejects_missing_unknown_and_invalid_configuration() {
    assert!(!command().output().unwrap().status.success());
    assert!(!command().arg("serve").output().unwrap().status.success());
    assert!(
        !configured()
            .arg("--unknown")
            .output()
            .unwrap()
            .status
            .success()
    );
    for (name, value) in [
        ("API_ADDRESS", "invalid"),
        ("API_PORT", "65536"),
        ("INFERENCE_ENDPOINT", ""),
        ("INFERENCE_ENDPOINT", "not-a-url"),
        ("INFERENCE_ENDPOINT", "https://localhost/v1?secret=value"),
        ("INFERENCE_ENDPOINT", "https://localhost/v1#fragment"),
        ("INFERENCE_ENDPOINT", "file:///tmp/backend"),
        ("INFERENCE_TIMEOUT_SECONDS", "0"),
        ("MAX_REQUEST_BYTES", "0"),
        ("SHUTDOWN_TIMEOUT_SECONDS", "0"),
        ("SHUTDOWN_TIMEOUT_SECONDS", "invalid"),
    ] {
        let output = configured().env(name, value).output().unwrap();
        assert!(!output.status.success(), "{name}={value} must fail");
    }
}

#[test]
fn flags_override_environment_before_endpoint_validation() {
    // An invalid scheme reaches endpoint validation only if flags override the
    // malformed environment values. No listener or external backend is needed.
    let output = configured()
        .env("API_ADDRESS", "invalid")
        .env("API_PORT", "invalid")
        .env("INFERENCE_TIMEOUT_SECONDS", "invalid")
        .env("MAX_REQUEST_BYTES", "invalid")
        .env("SHUTDOWN_TIMEOUT_SECONDS", "invalid")
        .args([
            "--api-address",
            "127.0.0.1",
            "--api-port",
            "0",
            "--inference-endpoint",
            "file:///tmp/backend",
            "--inference-timeout-seconds",
            "1",
            "--max-request-bytes",
            "1",
            "--shutdown-timeout-seconds",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("inference endpoint must be HTTP(S)"),
        "{}",
        stderr(&output)
    );
}
