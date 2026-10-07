//! Native Responses normalization, private to the Hugging Face encoder.

use super::super::InputError;
use hf_chat_template::{Content, Message, RenderInput};
use serde_json::{Map, Value, json};

use super::super::request::{
    CreateResponseRequest, EasyInputMessage, EasyInputMessageContent, EasyInputMessageRole,
    FunctionCallCaller, FunctionCallOutputOutput, FunctionOutputContent, ItemStatus, MessageRole,
    ReasoningContent, ReasoningSummary, ResponseInput, ResponseInputContent, ResponseInputItem,
    ResponseItem, ResponseOutputMessageContent, Tool, ToolChoice, ToolChoiceOptions,
};

impl TryFrom<&CreateResponseRequest> for RenderInput {
    type Error = InputError;

    fn try_from(request: &CreateResponseRequest) -> Result<Self, Self::Error> {
        check_local_input(request)?;
        let mut rendered = Self {
            // Responses starts a new assistant turn. Partial assistant
            // continuation is rejected below; merely disabling this flag would
            // leave the template's end-of-message tokens in the prompt.
            add_generation_prompt: true,
            ..Self::default()
        };
        if let Some(instructions) = request
            .instructions
            .as_ref()
            .filter(|text| !text.is_empty())
        {
            rendered.messages.push(Message::system(instructions));
        }
        match request.input.as_ref() {
            Some(ResponseInput::TextInput(text)) => rendered.messages.push(Message::user(text)),
            Some(ResponseInput::ResponseInputItem(items)) => {
                for (index, item) in items.iter().enumerate() {
                    append_item(&mut rendered.messages, item, &format!("input[{index}]"))?;
                }
            }
            None => {
                return Err(Self::Error::new(
                    "input",
                    "supply inline input before rendering",
                ));
            }
        }
        if let Some(tools) = &request.tools {
            for (index, tool) in tools.iter().enumerate() {
                rendered
                    .tools
                    .push(function_tool(tool, &format!("tools[{index}]"))?);
            }
        }
        Ok(rendered)
    }
}

fn check_local_input(request: &CreateResponseRequest) -> Result<(), InputError> {
    for (present, field, reason) in [
        (
            request.conversation.is_some(),
            "conversation",
            "resolve conversation history before rendering",
        ),
        (
            request.previous_response_id.is_some(),
            "previous_response_id",
            "resolve previous response history before rendering",
        ),
        (
            request.prompt.is_some(),
            "prompt",
            "resolve the stored prompt before rendering",
        ),
        (
            request
                .context_management
                .as_ref()
                .is_some_and(|items| !items.is_empty()),
            "context_management",
            "apply context management before rendering",
        ),
    ] {
        if present {
            return Err(InputError::new(field, reason));
        }
    }
    if request.tool_choice.as_ref().is_some_and(|choice| {
        !matches!(
            choice,
            ToolChoice::ToolChoiceOptions(ToolChoiceOptions::Auto | ToolChoiceOptions::None)
        )
    }) {
        return Err(InputError::new(
            "tool_choice",
            "forced tool selection requires the model's tool parser",
        ));
    }
    if let Some(ResponseInput::ResponseInputItem(items)) = &request.input
        && let Some(last) = items.last()
    {
        let status = match last {
            ResponseInputItem::Message(message) => message.status,
            ResponseInputItem::Item(item) => match item.as_ref() {
                ResponseItem::Message { status, .. } => Some(*status),
                ResponseItem::Reasoning { status, .. } => *status,
                _ => None,
            },
            _ => None,
        };
        if matches!(
            status,
            Some(ItemStatus::InProgress | ItemStatus::Incomplete)
        ) {
            return Err(InputError::new(
                format!("input[{}].status", items.len() - 1),
                "partial message continuation requires a continuation-aware renderer",
            ));
        }
    }
    Ok(())
}

fn append_item(
    messages: &mut Vec<Message>,
    item: &ResponseInputItem,
    path: &str,
) -> Result<(), InputError> {
    match item {
        ResponseInputItem::EasyInputMessage(message) => {
            append_easy_message(messages, message, path)
        }
        ResponseInputItem::Message(message) => {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::System => "system",
                MessageRole::Developer => "developer",
            };
            messages.push(Message {
                role: role.into(),
                content: Some(input_content(&message.content, path)?),
                ..Message::default()
            });
            Ok(())
        }
        ResponseInputItem::Item(item) => append_response_item(messages, item, path),
        ResponseInputItem::ItemReference(_) => Err(InputError::new(
            path,
            "resolve the referenced item before rendering",
        )),
    }
}

fn append_easy_message(
    messages: &mut Vec<Message>,
    message: &EasyInputMessage,
    path: &str,
) -> Result<(), InputError> {
    if message.role == EasyInputMessageRole::Assistant {
        let text = match &message.content {
            EasyInputMessageContent::TextInput(text) => text.clone(),
            EasyInputMessageContent::ResponseInputContent(parts) => {
                // Native Responses takes the first assistant text part. Reject
                // additional parts instead of silently discarding user content.
                let [ResponseInputContent::Text { text, .. }] = parts.as_slice() else {
                    return Err(InputError::new(
                        format!("{path}.content"),
                        "assistant input must contain one text part",
                    ));
                };
                text.clone()
            }
        };
        append_assistant_text(messages, text);
        return Ok(());
    }
    let role = match message.role {
        EasyInputMessageRole::User => "user",
        EasyInputMessageRole::System => "system",
        EasyInputMessageRole::Developer => "developer",
        EasyInputMessageRole::Assistant => unreachable!("assistant was handled above"),
    };
    let content = match &message.content {
        EasyInputMessageContent::TextInput(text) => Content::Text(text.clone()),
        EasyInputMessageContent::ResponseInputContent(parts) => input_content(parts, path)?,
    };
    messages.push(Message {
        role: role.into(),
        content: Some(content),
        ..Message::default()
    });
    Ok(())
}

fn input_content(parts: &[ResponseInputContent], path: &str) -> Result<Content, InputError> {
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| match part {
            // Native Responses normalization retains text, not cache-breakpoint
            // extensions, in the template content parts (covered by parity).
            ResponseInputContent::Text { text, .. } => Ok(json!({"type":"text", "text":text})),
            ResponseInputContent::Image { .. } | ResponseInputContent::File { .. } => {
                Err(InputError::new(
                    format!("{path}.content[{index}]"),
                    "images and files require model-aware preprocessing",
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Content::Parts)
}

fn append_response_item(
    messages: &mut Vec<Message>,
    item: &ResponseItem,
    path: &str,
) -> Result<(), InputError> {
    match item {
        ResponseItem::Message { content, .. } => {
            let [ResponseOutputMessageContent::OutputText { text, .. }] = content.as_slice() else {
                return Err(InputError::new(
                    format!("{path}.content"),
                    "assistant output must contain one output_text part",
                ));
            };
            append_assistant_text(messages, text.clone());
        }
        ResponseItem::FunctionCall {
            arguments,
            call_id,
            name,
            namespace,
            caller,
            ..
        } => {
            check_function_context(namespace, caller, path)?;
            let arguments = if arguments.is_empty() {
                Map::new()
            } else {
                serde_json::from_str::<Map<String, Value>>(arguments).map_err(|error| {
                    InputError::with_source(
                        format!("{path}.arguments"),
                        "function arguments must be a JSON object",
                        Box::new(error),
                    )
                })?
            };
            let call = json!({"id": call_id, "function": {"name": name, "arguments": arguments}, "type": "function"});
            if let Some(message) = messages
                .last_mut()
                .filter(|message| message.role == "assistant")
            {
                message.tool_calls.push(call);
            } else {
                messages.push(Message {
                    role: "assistant".into(),
                    tool_calls: vec![call],
                    ..Message::default()
                });
            }
        }
        ResponseItem::FunctionCallOutput {
            output,
            call_id,
            namespace,
            caller,
            ..
        } => {
            check_function_context(namespace, caller, path)?;
            let call_id = call_id.as_ref().ok_or_else(|| {
                InputError::new(
                    format!("{path}.call_id"),
                    "supply the function call ID to correlate its result",
                )
            })?;
            let mut message = Message {
                role: "tool".into(),
                content: Some(function_output(output, path)?),
                ..Message::default()
            };
            message
                .extra
                .insert("tool_call_id".into(), Value::String(call_id.clone()));
            messages.push(message);
        }
        ResponseItem::Reasoning {
            content,
            summary,
            encrypted_content,
            ..
        } => {
            if encrypted_content.is_some() {
                return Err(InputError::new(
                    format!("{path}.encrypted_content"),
                    "encrypted reasoning cannot be rendered locally",
                ));
            }
            let reasoning = reasoning_text(content.as_deref(), summary, path)?;
            append_reasoning(messages, reasoning);
        }
        _ => {
            return Err(InputError::new(
                path,
                "this response item has no supported text-chat representation",
            ));
        }
    }
    Ok(())
}

fn reasoning_text(
    content: Option<&[ReasoningContent]>,
    summary: &[ReasoningSummary],
    path: &str,
) -> Result<String, InputError> {
    if let Some(content) = content.filter(|parts| !parts.is_empty()) {
        let [part] = content else {
            return Err(InputError::new(
                format!("{path}.content"),
                "reasoning input must contain one text part",
            ));
        };
        Ok(part.text.clone())
    } else {
        if summary.len() > 1 {
            return Err(InputError::new(
                format!("{path}.summary"),
                "reasoning input must contain at most one summary part",
            ));
        }
        Ok(summary
            .first()
            .map_or_else(String::new, |part| part.text.clone()))
    }
}

fn append_assistant_text(messages: &mut Vec<Message>, text: String) {
    if let Some(message) = messages
        .last_mut()
        .filter(|message| message.role == "assistant" && message.content.is_none())
    {
        message.content = Some(Content::Text(text));
    } else {
        messages.push(Message::assistant(text));
    }
}

fn append_reasoning(messages: &mut Vec<Message>, reasoning: String) {
    let mut extra = Map::new();
    // Both keys come from vLLM's chat parser; templates use either spelling.
    extra.insert("reasoning".into(), Value::String(reasoning.clone()));
    extra.insert("reasoning_content".into(), Value::String(reasoning));
    if let Some(message) = messages
        .last_mut()
        .filter(|message| message.role == "assistant" && !message.extra.contains_key("reasoning"))
    {
        message.extra.extend(extra);
    } else {
        messages.push(Message {
            role: "assistant".into(),
            extra,
            ..Message::default()
        });
    }
}

fn check_function_context(
    namespace: &Option<String>,
    caller: &Option<FunctionCallCaller>,
    path: &str,
) -> Result<(), InputError> {
    if namespace.is_some() {
        return Err(InputError::new(
            format!("{path}.namespace"),
            "namespaced functions require namespace-aware tool definitions",
        ));
    }
    if matches!(caller, Some(FunctionCallCaller::Program { .. })) {
        return Err(InputError::new(
            format!("{path}.caller"),
            "programmatic calls require tool-execution context",
        ));
    }
    Ok(())
}

fn function_output(output: &FunctionCallOutputOutput, path: &str) -> Result<Content, InputError> {
    match output {
        FunctionCallOutputOutput::Text(text) => Ok(Content::Text(text.clone())),
        FunctionCallOutputOutput::Content(parts) => parts
            .iter()
            .enumerate()
            .map(|(index, part)| match part {
                FunctionOutputContent::Text { text, .. } => Ok(json!({"type":"text", "text":text})),
                FunctionOutputContent::Image { .. } | FunctionOutputContent::File { .. } => {
                    Err(InputError::new(
                        format!("{path}.output[{index}]"),
                        "images and files require model-aware preprocessing",
                    ))
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Content::Parts),
    }
}

fn function_tool(tool: &Tool, path: &str) -> Result<Value, InputError> {
    let Tool::Function {
        name,
        parameters,
        strict,
        description,
        defer_loading,
        allowed_callers,
        r#async,
        output_schema,
    } = tool
    else {
        return Err(InputError::new(
            path,
            "only function tools have a supported text-chat representation",
        ));
    };
    if allowed_callers.is_some() || r#async.is_some() || output_schema.is_some() {
        return Err(InputError::new(
            path,
            "function execution extensions require a tool-execution adapter",
        ));
    }
    // Responses flattens the function fields; HF templates read tools[].function.
    // Preserve the schema from its request owner, including nullable parameters.
    let mut function = Map::new();
    function.insert("name".into(), Value::String(name.clone()));
    function.insert(
        "description".into(),
        description
            .as_ref()
            .map_or(Value::Null, |text| Value::String(text.clone())),
    );
    function.insert(
        "parameters".into(),
        parameters
            .as_ref()
            .map_or(Value::Null, |schema| json!(schema)),
    );
    if let Some(strict) = strict {
        function.insert("strict".into(), Value::Bool(*strict));
    }
    if let Some(defer_loading) = defer_loading {
        function.insert("defer_loading".into(), Value::Bool(*defer_loading));
    }
    Ok(json!({"type": "function", "function": function}))
}
