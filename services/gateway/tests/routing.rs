//! Exercise replica discovery and routing through the compiled gateway.

use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};
use zeromq::{PubSocket, RouterSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

struct Replica {
    endpoint: String,
    http: JoinHandle<()>,
    replay: JoinHandle<()>,
    publisher: Option<PubSocket>,
}

impl Drop for Replica {
    fn drop(&mut self) {
        self.http.abort();
        self.replay.abort();
    }
}

fn model_config() -> Value {
    serde_json::from_slice(include_bytes!(
        "../src/testdata/input-string/model-config.json"
    ))
    .unwrap()
}

async fn replica(identity: &'static str, config: Value, cached: bool) -> Replica {
    let mut publisher = PubSocket::new();
    let endpoint = publisher
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let mut replay = RouterSocket::new();
    let replay_endpoint = replay.bind("tcp://127.0.0.1:0").await.unwrap().to_string();
    let discovery = json!({"instance_id":identity, "cache_groups":[8], "sources":[{
        "data_parallel_rank":0, "enable_kv_cache_events":true, "publisher":"zmq",
        "endpoint":endpoint, "replay_endpoint":replay_endpoint, "topic":"cache"
    }]});
    let router = Router::new()
        .route(
            "/v1/inferno/model-config",
            get(move || {
                let config = config.clone();
                async { Json(config) }
            }),
        )
        .route(
            "/v1/inferno/kv-events",
            get(move || {
                let discovery = discovery.clone();
                async { Json(discovery) }
            }),
        )
        .route("/v1/responses", post(move || async move { identity }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let http = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let replay = tokio::spawn(async move {
        loop {
            let request = replay.recv().await.unwrap();
            let id = request.get(0).unwrap().clone();
            if cached {
                let response: ZmqMessage = vec![
                    id.clone(),
                    Vec::new().into(),
                    "cache".into(),
                    0_u64.to_be_bytes().to_vec().into(),
                    include_bytes!("../src/testdata/kv-events/stored.msgpack")
                        .to_vec()
                        .into(),
                ]
                .try_into()
                .unwrap();
                replay.send(response).await.unwrap();
            }
            let end: ZmqMessage = vec![
                id,
                Vec::new().into(),
                Vec::new().into(),
                u64::MAX.to_be_bytes().to_vec().into(),
                Vec::new().into(),
            ]
            .try_into()
            .unwrap();
            replay.send(end).await.unwrap();
        }
    });
    Replica {
        endpoint,
        http,
        replay,
        publisher: Some(publisher),
    }
}

fn command(replicas: &[&Replica], cache: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_inferno"));
    command
        .env_clear()
        .arg("serve")
        .env("API_ADDRESS", "127.0.0.1")
        .env("API_PORT", "0")
        .env("INFERENCE_MODEL", "fixture")
        .env(
            "INFERENCE_ENDPOINT",
            replicas
                .iter()
                .map(|replica| replica.endpoint.as_str())
                .collect::<Vec<_>>()
                .join(","),
        )
        .env("INFERENCE_TIMEOUT_SECONDS", "2")
        .env("XDG_CACHE_HOME", cache);
    command
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn line(lines: &mut mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(5), lines.recv())
        .await
        .unwrap()
        .expect("gateway exited before readiness")
}

#[tokio::test]
async fn discovers_replicas_routes_to_cached_prefix_and_invalidates_on_disconnect() {
    let _ = rustls_graviola::default_provider().install_default();
    let first = replica("replica-a", model_config(), false).await;
    let mut second = replica("replica-b", model_config(), true).await;
    let cache = tempfile::tempdir().unwrap();
    let mut process = Process(
        command(&[&first, &second], cache.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = process.0.stdout.take().unwrap();
    let (sender, mut lines) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut address = String::new();
    let mut subscribed = 0;
    while subscribed < 2 {
        let event = line(&mut lines).await;
        if let Some(listener) = event.strip_prefix("inferno listening on ") {
            address = format!("http://{listener}/v1/responses");
        }
        if event.contains("kv_events.subscribed") {
            subscribed += 1;
        }
    }
    let cases: Vec<Value> =
        serde_json::from_slice(include_bytes!("../src/testdata/input-string/parity.json")).unwrap();
    let request = &cases
        .iter()
        .find(|case| case["name"] == "unicode-whitespace")
        .unwrap()["request"];
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(&address)
            .json(request)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "replica-b"
    );
    second.publisher.take();
    loop {
        let event = line(&mut lines).await;
        if event.contains("kv_events.disconnected") {
            break;
        }
    }
    assert_eq!(
        client
            .post(&address)
            .json(request)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "replica-a"
    );
}

#[tokio::test]
async fn incompatible_models_and_duplicate_replicas_fail_before_listening() {
    for duplicate in [false, true] {
        let first = replica("replica-a", model_config(), false).await;
        let mut config = model_config();
        if !duplicate {
            config["prefix"]["block_size"] = json!(16);
        }
        let second = replica(
            if duplicate { "replica-a" } else { "replica-b" },
            config,
            false,
        )
        .await;
        let cache = tempfile::tempdir().unwrap();
        let mut command = command(&[&first, &second], cache.path());
        let output = tokio::task::spawn_blocking(move || command.output().unwrap())
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("inferno listening"));
        let message = if duplicate {
            "distinct replicas"
        } else {
            "same model preparation"
        };
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    }
}
