//! Local Hugging Face encoding. Renderer-specific messages and `RenderInput` never
//! escape this module. Templates and tokenizer come from the serving process.

mod responses;

use std::num::NonZeroUsize;

use hf_chat_template::{ChatTemplate, Content, Message, RenderInput};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokenizers::Tokenizer;

use super::{
    CreateResponseRequest, InputError,
    request::{ToolChoice, ToolChoiceOptions, Truncation},
};

/// Transient output of encoding, consumed immediately by preparation.
/// Media, prompt embeddings and adapters are rejected until their cache-key
/// inputs can be produced faithfully. They must never enter the text-only path.
pub(super) struct EncodedInput {
    pub(super) token_ids: Vec<u32>,
    pub(super) cache_salt: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct EncoderConfig {
    pub(super) models: Vec<String>,
    max_model_len: NonZeroUsize,
    add_special_tokens: bool,
    without_tools: TemplateConfig,
    with_tools: TemplateConfig,
    special_tokens: Map<String, Value>,
    template_kwargs: Map<String, Value>,
    exclude_tools_when_tool_choice_none: bool,
    tokenizer_sha256: [u8; 32],
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TemplateConfig {
    source: String,
    content_format: ContentFormat,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ContentFormat {
    String,
    Openai,
}

#[derive(Debug)]
struct Template {
    compiled: ChatTemplate,
    content_format: ContentFormat,
    supports_developer: bool,
}

impl Template {
    fn compile(config: &TemplateConfig) -> Result<Self, InputError> {
        // A wall-clock template cannot promise the same prompt in independent
        // processes. Reject it rather than introduce an unowned time setting.
        if config.source.contains("strftime_now") {
            return Err(InputError::new(
                "chat_template",
                "time-dependent templates are not supported locally",
            ));
        }
        Ok(Self {
            compiled: ChatTemplate::from_str(&config.source).map_err(|error| {
                InputError::with_source(
                    "chat_template",
                    "cannot compile the model template",
                    Box::new(error),
                )
            })?,
            content_format: config.content_format,
            supports_developer: config.source.contains("\"developer\"")
                || config.source.contains("'developer'"),
        })
    }
}

pub(super) struct HuggingFaceEncoder {
    tokenizer: Tokenizer,
    models: Vec<String>,
    max_model_len: NonZeroUsize,
    add_special_tokens: bool,
    template_context: Map<String, Value>,
    exclude_tools_when_tool_choice_none: bool,
    without_tools: Template,
    with_tools: Template,
}

impl std::fmt::Debug for HuggingFaceEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HuggingFaceEncoder").finish_non_exhaustive()
    }
}

impl HuggingFaceEncoder {
    pub(super) fn load(bytes: &[u8], config: EncoderConfig) -> Result<Self, InputError> {
        use sha2::{Digest, Sha256};
        if config.models.is_empty() || config.models.iter().any(String::is_empty) {
            return Err(InputError::new(
                "encoder.models",
                "supply the native served model names",
            ));
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if digest != config.tokenizer_sha256 {
            return Err(InputError::new(
                "tokenizer",
                "tokenizer checksum does not match the model configuration",
            ));
        }
        let mut tokenizer = Tokenizer::from_bytes(bytes).map_err(|error| {
            InputError::with_source("tokenizer", "cannot load the model tokenizer", error)
        })?;
        tokenizer.with_padding(None);
        tokenizer.with_truncation(None).map_err(|error| {
            InputError::with_source("tokenizer", "cannot disable implicit truncation", error)
        })?;
        let mut template_context = config.special_tokens;
        template_context.extend(config.template_kwargs);
        Ok(Self {
            tokenizer,
            models: config.models,
            max_model_len: config.max_model_len,
            add_special_tokens: config.add_special_tokens,
            template_context,
            exclude_tools_when_tool_choice_none: config.exclude_tools_when_tool_choice_none,
            without_tools: Template::compile(&config.without_tools)?,
            with_tools: Template::compile(&config.with_tools)?,
        })
    }

    pub(super) fn encode(
        &self,
        request: &CreateResponseRequest,
    ) -> Result<EncodedInput, InputError> {
        if request
            .model
            .as_ref()
            .is_some_and(|model| !self.models.contains(model))
        {
            return Err(InputError::new(
                "model",
                "model is not served by this input processor",
            ));
        }
        let mut input = RenderInput::try_from(request)?;
        if input.messages.is_empty() {
            return Err(InputError::new("input", "supply at least one message"));
        }
        if self.exclude_tools_when_tool_choice_none
            && request.tool_choice == Some(ToolChoice::ToolChoiceOptions(ToolChoiceOptions::None))
        {
            input.tools.clear();
        }
        let template = if input.tools.is_empty() {
            &self.without_tools
        } else {
            &self.with_tools
        };
        normalize_messages(&mut input.messages, template);
        let context = self.context(input, request)?;
        let prompt = template
            .compiled
            .render_context(&context)
            .map_err(|error| {
                InputError::with_source(
                    "input",
                    "chat template rejected the request",
                    Box::new(error),
                )
            })?;
        // vLLM Responses supplies its own TokenizeParams, including this flag.
        // Do not replace it with the Transformers chat-tokenization default.
        let encoding = self
            .tokenizer
            .encode(prompt, self.add_special_tokens)
            .map_err(|error| InputError::with_source("input", "tokenization failed", error))?;
        let token_ids = encoding.get_ids().to_vec();
        self.check_length(request, token_ids.len())?;
        Ok(EncodedInput {
            token_ids,
            cache_salt: request.cache_salt.clone(),
        })
    }

    fn context(
        &self,
        input: RenderInput,
        request: &CreateResponseRequest,
    ) -> Result<Value, InputError> {
        let mut context = self.template_context.clone();
        // Python apply_chat_template exposes tools=None even when no tools exist.
        context.insert("tools".into(), Value::Null);
        let Value::Object(values) = serde_json::to_value(input).map_err(|error| {
            InputError::with_source(
                "input",
                "cannot construct template context",
                Box::new(error),
            )
        })?
        else {
            unreachable!()
        };
        context.extend(values);
        context.insert("continue_final_message".into(), Value::Bool(false));
        if let Some(effort) = request.reasoning.as_ref().and_then(|r| r.effort) {
            let effort = json!(effort);
            context.insert("enable_thinking".into(), Value::Bool(effort != "none"));
            context.insert("reasoning_effort".into(), effort);
        }
        Ok(Value::Object(context))
    }

    fn check_length(
        &self,
        request: &CreateResponseRequest,
        count: usize,
    ) -> Result<(), InputError> {
        // Truncation changes the prompt. Until the native policy is implemented,
        // reject it explicitly; never hash an untruncated approximation.
        if request.truncation == Some(Truncation::Auto) {
            return Err(InputError::new(
                "truncation",
                "automatic truncation is not supported locally",
            ));
        }
        let output = request
            .max_output_tokens
            .as_ref()
            .map_or(Some(0), |n| n.as_u64())
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| {
                InputError::new("max_output_tokens", "expected a nonnegative integer")
            })?;
        if count == 0
            || count
                .checked_add(output)
                .is_none_or(|n| n > self.max_model_len.get())
        {
            return Err(InputError::new(
                "input",
                "input and requested output must fit the model context",
            ));
        }
        Ok(())
    }
}

fn normalize_messages(messages: &mut Vec<Message>, template: &Template) {
    for message in messages.iter_mut() {
        let parts = match message.content.take() {
            Some(Content::Text(text)) => vec![json!({"type": "text", "text": text})],
            Some(Content::Parts(parts)) => parts,
            None => Vec::new(),
        };
        message.content = Some(
            if template.content_format == ContentFormat::Openai && message.role != "tool" {
                Content::Parts(parts)
            } else {
                Content::Text(
                    parts
                        .iter()
                        .filter_map(|part| part["text"].as_str())
                        .filter(|s| {
                            template.content_format == ContentFormat::Openai || !s.is_empty()
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            },
        );
        if message.role == "developer" {
            message.extra.insert("tools".into(), Value::Null);
        }
    }
    if template.supports_developer || !messages.iter().any(|m| m.role == "developer") {
        return;
    }
    for message in messages.iter_mut().filter(|m| m.role == "developer") {
        message.role = "system".into();
        message.extra.remove("tools");
    }
    if !messages
        .iter()
        .enumerate()
        .any(|(i, m)| i > 0 && m.role == "system")
    {
        return;
    }
    let mut system = Vec::new();
    messages.retain(|message| {
        if message.role != "system" {
            return true;
        }
        let text = match &message.content {
            Some(Content::Text(text)) => text.clone(),
            Some(Content::Parts(parts)) => parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        };
        if !text.is_empty() {
            system.push(text);
        }
        false
    });
    if !system.is_empty() {
        messages.insert(0, Message::system(system.join("\n\n")));
    }
}
