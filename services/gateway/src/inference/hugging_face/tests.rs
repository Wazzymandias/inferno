use hf_chat_template::{ChatTemplate, RenderInput, TokenizerConfig};
use serde_json::{Value, json};

use super::{CreateResponseRequest, RenderInputError};
use crate::inference::ModelInput;

fn request(value: Value) -> CreateResponseRequest {
    serde_json::from_value(value).unwrap()
}

fn rendered(value: Value) -> Value {
    serde_json::to_value(RenderInput::try_from(&request(value)).unwrap()).unwrap()
}

fn rejects(value: Value, field: &str) -> RenderInputError {
    let error = RenderInput::try_from(&request(value)).unwrap_err();
    assert_eq!(error.field, field);
    error
}

#[test]
fn owned_borrowed_and_direct_conversions_produce_the_same_input() {
    let request = request(json!({"instructions": "Be brief.", "input": "  Héllo\n世界  "}));
    let original = request.clone();
    let borrowed = ModelInput::from(&request).render_input().unwrap();
    let via_trait: RenderInput = ModelInput::from(&request).try_into().unwrap();
    let direct = RenderInput::try_from(&request).unwrap();
    assert_eq!(request, original);
    let owned = ModelInput::from(request).render_input().unwrap();
    let expected = json!({
        "messages": [
            {"role": "system", "content": "Be brief."},
            {"role": "user", "content": "  Héllo\n世界  "}
        ],
        "add_generation_prompt": true
    });
    for input in [borrowed, via_trait, direct, owned] {
        assert_eq!(serde_json::to_value(input).unwrap(), expected);
    }
}

#[test]
fn sampling_and_transport_fields_do_not_become_template_variables() {
    let plain = rendered(json!({"input": "Hello"}));
    let configured = rendered(json!({
        "input": "Hello", "instructions": "", "model": "model-id",
        "metadata": {"trace": "example"}, "temperature": 0.1, "top_p": 0.9,
        "max_output_tokens": 32, "stream": true, "store": false,
        "prompt_cache_key": "routing-key", "tool_choice": "auto",
        "truncation": "disabled", "reasoning": {}
    }));
    assert_eq!(plain, configured);
}

#[test]
fn preserves_roles_order_and_text_part_boundaries() {
    let result = rendered(json!({"instructions": "Instructions", "input": [
        {"role": "system", "content": "System"},
        {"role": "developer", "content": "Developer"},
        {"type": "message", "role": "user", "status": "completed", "content": [
            {"type": "input_text", "text": "first"},
            {"type": "input_text", "text": ""},
            {"type": "input_text", "text": " second\n"}
        ]},
        {"role": "assistant", "content": [{"type": "input_text", "text": "Reply"}]},
        {"role": "user", "content": "Next"}
    ]}));
    assert_eq!(
        result["messages"],
        json!([
            {"role": "system", "content": "Instructions"},
            {"role": "system", "content": "System"},
            {"role": "developer", "content": "Developer"},
            {"role": "user", "content": "first\n second\n"},
            {"role": "assistant", "content": "Reply"},
            {"role": "user", "content": "Next"}
        ])
    );
}

#[test]
fn groups_parallel_function_calls_and_correlates_results() {
    let result = rendered(json!({"input": [
        {"role": "assistant", "content": "Checking."},
        {"type": "function_call", "call_id": "call_1", "name": "lookup", "arguments": "{\"city\":\"Paris\"}"},
        {"type": "function_call", "call_id": "call_2", "name": "clock", "arguments": ""},
        {"type": "function_call_output", "call_id": "call_1", "output": "Sunny"},
        {"type": "function_call_output", "call_id": "call_2", "output": [
            {"type": "input_text", "text": "10:00"}, {"type": "input_text", "text": "UTC"}
        ]}
    ]}));
    assert_eq!(
        result["messages"],
        json!([
            {"role": "assistant", "content": "Checking.", "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "lookup", "arguments": {"city": "Paris"}}},
                {"id": "call_2", "type": "function", "function": {"name": "clock", "arguments": {}}}
            ]},
            {"role": "tool", "content": "Sunny", "tool_call_id": "call_1"},
            {"role": "tool", "content": "10:00\nUTC", "tool_call_id": "call_2"}
        ])
    );
}

#[test]
fn nests_function_definitions_and_preserves_schema_and_optional_fields() {
    let result = rendered(json!({"input": "Hello", "tools": [
        {"type": "function", "name": "lookup", "description": "Find a city.", "parameters": {
            "type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]
        }, "strict": false, "defer_loading": true},
        {"type": "function", "name": "clock", "parameters": null}
    ]}));
    assert_eq!(
        result["tools"],
        json!([
            {"type": "function", "function": {"name": "lookup", "description": "Find a city.", "parameters": {
                "type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]
            }, "strict": false, "defer_loading": true}},
            {"type": "function", "function": {"name": "clock", "description": null, "parameters": null}}
        ])
    );
}

#[test]
fn keeps_separate_assistant_texts_and_fills_content_after_tool_calls() {
    let result = rendered(json!({"input": [
        {"role": "assistant", "content": "First"},
        {"role": "assistant", "content": "Second"},
        {"role": "user", "content": "Next"},
        {"type": "function_call", "call_id": "call_1", "name": "clock", "arguments": "{}"},
        {"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
         "content": [{"type": "output_text", "text": "Checking.", "annotations": [], "logprobs": []}]}
    ]}));
    let messages = result["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0]["content"], "First");
    assert_eq!(messages[1]["content"], "Second");
    assert_eq!(messages[3]["content"], "Checking.");
    assert_eq!(messages[3]["tool_calls"][0]["id"], "call_1");
}

#[test]
fn groups_reasoning_text_and_calls_without_emitting_response_metadata() {
    let result = rendered(json!({"input": [
        {"type": "reasoning", "id": "rs_1", "summary": [], "content": [{"type": "reasoning_text", "text": "Think."}]},
        {"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
         "content": [{"type": "output_text", "text": "Answer.", "annotations": [], "logprobs": []}]},
        {"type": "function_call", "call_id": "call_1", "name": "clock", "arguments": "{}"}
    ]}));
    assert_eq!(
        result["messages"],
        json!([{
            "role": "assistant", "content": "Answer.", "reasoning": "Think.", "reasoning_content": "Think.",
            "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "clock", "arguments": {}}}]
        }])
    );
    let summary = rendered(json!({"input": [
        {"type": "reasoning", "id": "rs_2", "summary": [{"type": "summary_text", "text": "Summary."}]}
    ]}));
    assert_eq!(summary["messages"][0]["reasoning"], "Summary.");
    assert_eq!(summary["messages"][0]["content"], "");
}

#[test]
fn passes_input_to_the_renderer_with_special_tokens_owned_by_model_config() {
    // A small fixture exercises the real renderer boundary. It is not a
    // production model template or an assertion of backend token parity.
    let config: TokenizerConfig = serde_json::from_value(json!({
        "bos_token": "<bos>",
        "chat_template": "{{ bos_token }}{% for m in messages %}[{{ m.role }}]{{ m.content }}{% endfor %}{% if add_generation_prompt %}[assistant]{% endif %}"
    })).unwrap();
    let template = ChatTemplate::from_tokenizer_config(&config).unwrap();
    let input = ModelInput::from(request(json!({"input": "Hello"})))
        .render_input()
        .unwrap();
    assert_eq!(
        template.render(&input).unwrap(),
        "<bos>[user]Hello[assistant]"
    );
    assert!(input.extra.is_empty());
}

#[test]
fn rejects_unresolved_state_and_model_dependent_transformations() {
    for (field, value) in [
        ("conversation", json!("private-conversation")),
        ("previous_response_id", json!("private-response")),
        ("prompt", json!({"id": "private-prompt"})),
        ("context_management", json!([{"type": "compaction"}])),
        ("truncation", json!("auto")),
        ("reasoning", json!({"effort": "low"})),
        ("tool_choice", json!("required")),
    ] {
        let mut body = json!({"input": "private-input"});
        body[field] = value;
        let error = rejects(body, field);
        assert!(!error.to_string().contains("private-"));
    }
    rejects(
        json!({"input": [{"type": "item_reference", "id": "private-item"}]}),
        "input[0]",
    );
}

#[test]
fn rejects_unsupported_parts_instead_of_losing_prompt_content() {
    for (kind, part) in [
        (
            "image",
            json!({"type": "input_image", "detail": "auto", "image_url": "https://example.invalid/image"}),
        ),
        ("file", json!({"type": "input_file", "file_id": "file_1"})),
    ] {
        let error = rejects(
            json!({"input": [{"role": "user", "content": [
                {"type": "input_text", "text": "Keep this"}, part
            ]}]}),
            "input[0].content[1]",
        );
        assert!(error.reason.contains("model-aware"), "{kind}: {error}");
    }
    rejects(
        json!({"input": [{"role": "assistant", "content": [
            {"type": "input_text", "text": "First"}, {"type": "input_text", "text": "Second"}
        ]}]}),
        "input[0].content",
    );
    rejects(
        json!({"input": [{"type": "message", "id": "msg_1", "role": "assistant", "status": "completed",
            "content": [{"type": "refusal", "refusal": "Refused"}]
        }]}),
        "input[0].content",
    );
    rejects(
        json!({"input": [{"type": "compaction", "encrypted_content": "private-content"}]}),
        "input[0]",
    );
    rejects(
        json!({"input": [{"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "private-content"}]}),
        "input[0].encrypted_content",
    );
}

#[test]
fn rejects_partial_assistant_continuation() {
    for status in ["in_progress", "incomplete"] {
        rejects(
            json!({"input": [{"type": "message", "id": "msg_1", "role": "assistant", "status": status,
                "content": [{"type": "output_text", "text": "Partial", "annotations": [], "logprobs": []}]
            }]}),
            "input[0].status",
        );
        rejects(
            json!({"input": [{"type": "reasoning", "id": "rs_1", "summary": [], "status": status}]}),
            "input[0].status",
        );
    }
}

#[test]
fn rejects_malformed_tool_history_without_exposing_arguments() {
    for arguments in ["not private JSON", "[]", "null", "42"] {
        let error = rejects(
            json!({"input": [{"type": "function_call", "name": "lookup", "call_id": "call_1", "arguments": arguments}]}),
            "input[0].arguments",
        );
        assert!(!error.to_string().contains(arguments));
    }
    rejects(
        json!({"input": [{"type": "function_call_output", "output": "Result"}]}),
        "input[0].call_id",
    );
    rejects(
        json!({"input": [{"type": "function_call", "name": "lookup", "call_id": "call_1", "arguments": "{}", "namespace": "tools"}]}),
        "input[0].namespace",
    );
    rejects(
        json!({"input": "Hello", "tools": [{"type": "web_search"}]}),
        "tools[0]",
    );
    rejects(
        json!({"input": "Hello", "tools": [{"type": "function", "name": "lookup", "parameters": null, "async": true}]}),
        "tools[0]",
    );
}

#[test]
fn unspecified_and_missing_inputs_are_errors_but_empty_inline_input_is_preserved() {
    assert!(ModelInput::Unspecified.render_input().is_err());
    rejects(json!({}), "input");
    assert_eq!(
        rendered(json!({"input": ""}))["messages"],
        json!([{"role": "user", "content": ""}])
    );
    assert_eq!(rendered(json!({"input": []}))["messages"], json!([]));
}
