use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{Router, http::Uri, routing::get};
use serde_json::Value;

use super::{Backend, ModelCache, ModelConfig};

const FIXTURE: &[u8] = include_bytes!("../../testdata/input-string/model-config.json");

async fn deployment() -> (Backend, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let router = Router::new().route(
        "/v1/inferno/model-config",
        get(move |uri: Uri| {
            let observed = Arc::clone(&observed);
            async move {
                assert_eq!(uri.query(), Some("model=fixture"));
                observed.fetch_add(1, Ordering::SeqCst);
                FIXTURE
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let backend = Backend::new(
        format!("http://{address}/v1").parse().unwrap(),
        Duration::from_secs(1),
        None,
    )
    .unwrap();
    (backend, requests, task)
}

#[tokio::test]
async fn discovery_refreshes_policy_and_inspection_works_offline() {
    let directory = tempfile::tempdir().unwrap();
    let (backend, requests, server) = deployment().await;
    let cache = ModelCache::in_directory(directory.path().into(), &backend, "fixture");
    let mut stale: Value = serde_json::from_slice(FIXTURE).unwrap();
    stale["prefix"]["block_size"] = 64.into();
    cache.write(&serde_json::to_vec(&stale).unwrap()).unwrap();

    cache
        .discover(
            backend.client.get(super::config_url(&backend, "fixture")),
            "fixture",
        )
        .await
        .unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());

    let processor = crate::inference::InputProcessor::load(
        &cache.for_inspection(&backend, "fixture").await.unwrap(),
    )
    .unwrap();
    let request =
        serde_json::from_value(serde_json::json!({"model":"fixture", "input":"hello"})).unwrap();
    assert!(
        !processor
            .prepare(&request)
            .unwrap()
            .prefix_hashes()
            .is_empty()
    );
    // Serving must fail if discovery fails, even when an offline copy exists.
    let error = cache
        .discover(
            backend.client.get(super::config_url(&backend, "fixture")),
            "fixture",
        )
        .await
        .unwrap_err();
    assert!(error.source().unwrap().is::<reqwest::Error>());
}

#[tokio::test]
async fn first_inspection_discovers_once_then_uses_the_cache() {
    let directory = tempfile::tempdir().unwrap();
    let (backend, requests, server) = deployment().await;
    let cache = ModelCache::in_directory(directory.path().into(), &backend, "fixture");
    cache.for_inspection(&backend, "fixture").await.unwrap();
    cache.for_inspection(&backend, "fixture").await.unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    server.abort();
}

#[test]
fn rejects_wrong_model_and_version_and_preserves_parse_errors() {
    assert!(ModelConfig::parse(FIXTURE, "unknown").is_err());
    let mut config: Value = serde_json::from_slice(FIXTURE).unwrap();
    config["format"] = "unsupported".into();
    assert!(ModelConfig::parse(&serde_json::to_vec(&config).unwrap(), "fixture").is_err());
    let error = ModelConfig::parse(b"private-invalid-json", "fixture").unwrap_err();
    assert!(error.source().unwrap().is::<serde_json::Error>());
    assert!(!format!("{error:?}").contains("private-invalid-json"));
}
