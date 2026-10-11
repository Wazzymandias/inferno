use std::{path::PathBuf, sync::Arc};

use serde_json::{Value, json};

use super::InputProcessor;
use crate::inference::{CreateResponseRequest, ModelConfig};

fn directory(format: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("src/testdata/input-{format}"))
}

fn request(value: Value) -> CreateResponseRequest {
    serde_json::from_value(value).unwrap()
}

#[test]
fn matches_native_tokens_and_chained_hashes_for_both_content_formats() {
    for format in ["string", "openai"] {
        let dir = directory(format);
        let processor = processor(format);
        let cases: Vec<Value> =
            serde_json::from_slice(&std::fs::read(dir.join("parity.json")).unwrap()).unwrap();
        for case in cases {
            let original = request(case["request"].clone());
            let before = serde_json::to_value(&original).unwrap();
            let result = processor.prepare(&original);
            if case["expected"].is_null() {
                assert!(result.is_err(), "{format}: {}", case["name"]);
                continue;
            }
            let input = result.unwrap_or_else(|e| panic!("{format}: {}: {e}", case["name"]));
            assert_eq!(
                json!(input.tokens()),
                case["expected"]["token_ids"],
                "{format}: {}",
                case["name"]
            );
            let hashes: Vec<String> = input
                .prefix_hashes()
                .iter()
                .map(|hash| hash.iter().map(|b| format!("{b:02x}")).collect())
                .collect();
            assert_eq!(
                json!(hashes),
                case["expected"]["prefix_hashes"],
                "{format}: {}",
                case["name"]
            );
            assert_eq!(serde_json::to_value(&original).unwrap(), before);
        }
    }
}

#[test]
fn shared_processor_keeps_requests_and_hash_chains_independent() {
    let processor = Arc::new(processor("string"));
    let request = request(json!({"input":"a prompt with enough tokens for several blocks"}));
    let expected = processor.prepare(&request).unwrap();
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..8 {
            workers.push(scope.spawn(|| processor.prepare(&request).unwrap()));
        }
        for worker in workers {
            let result = worker.join().unwrap();
            assert_eq!(result.tokens(), expected.tokens());
            assert_eq!(result.prefix_hashes(), expected.prefix_hashes());
        }
    });
    let salted = processor
        .prepare(&CreateResponseRequest {
            cache_salt: Some("isolated".into()),
            ..request.clone()
        })
        .unwrap();
    assert_eq!(salted.tokens(), expected.tokens());
    assert!(
        salted
            .prefix_hashes()
            .iter()
            .zip(expected.prefix_hashes())
            .all(|(a, b)| a != b)
    );
    let again = processor.prepare(&request).unwrap();
    assert_eq!(again.prefix_hashes(), expected.prefix_hashes());
}

#[test]
fn rejects_unresolved_or_unimplemented_inputs_without_returning_approximate_tokens() {
    let processor = processor("string");
    for value in [
        json!({"model":"other", "input":"secret text"}),
        json!({"input":"secret text", "previous_response_id":"private-id"}),
        json!({"input":"secret text", "conversation":"private-id"}),
        json!({"input":"secret text", "truncation":"auto"}),
        json!({"input":"secret text", "max_output_tokens":9000}),
        json!({"input":[{"role":"user", "content":[{"type":"input_image", "detail":"auto", "image_url":"https://private.invalid/image"}]}]}),
        json!({"input":[{"type":"item_reference", "id":"private-id"}]}),
        json!({"input":"secret text", "stream_options":{"include_obfuscation":true}}),
    ] {
        let error = processor.prepare(&request(value)).unwrap_err().to_string();
        assert!(!error.contains("secret text") && !error.contains("private"));
    }
}

fn processor(format: &str) -> InputProcessor {
    let bytes = std::fs::read(directory(format).join("model-config.json")).unwrap();
    InputProcessor::load(&ModelConfig::parse(&bytes, "fixture").unwrap()).unwrap()
}

#[test]
fn rejects_modified_tokenizer_assets() {
    let bytes = std::fs::read(directory("string").join("model-config.json")).unwrap();
    let mut config = ModelConfig::parse(&bytes, "fixture").unwrap();
    config.tokenizer_json.push(' ');
    assert!(InputProcessor::load(&config).is_err());
}
