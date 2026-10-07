//! Request types for the `POST /v1/responses` endpoint.
//!
//! Sources (retrieved 2026-10-05):
//! <https://developers.openai.com/api/reference/resources/responses/methods/create>
//! and the shared domain types at
//! <https://developers.openai.com/api/reference/resources/responses>.
//!
//! Optional nullable fields use `Option<T>`: omission and explicit null both
//! deserialize to `None` and are omitted when serialized.
//! Optional non-nullable fields reject explicit null. Required nullable fields
//! must be present. API defaults are left to the server.
//!
//! Function `strict` may be omitted, as documented in the function-calling guide.
//! Deserialization does not enforce model-specific constraints, numeric ranges,
//! or cross-field requirements. `CreateResponseRequest::validate` checks the
//! supported cross-field requirements separately.
//! Fields use untyped JSON only where the reference permits arbitrary JSON.
//! Unknown object fields are rejected so untagged unions cannot silently discard
//! fields while choosing an overlapping variant.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};

use super::InputError;

// Restrict enum fields to JSON strings and API objects to JSON maps. Serde's
// generic data model also accepts enum maps, numeric tags, and struct arrays.
// The typed derives below check field names, required fields, and enum values.
fn string<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    T::deserialize(Value::String(String::deserialize(deserializer)?))
        .map_err(serde::de::Error::custom)
}

fn nullable_string<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = Option::<String>::deserialize(deserializer)?;
    T::deserialize(value.map_or(Value::Null, Value::String)).map_err(serde::de::Error::custom)
}

fn strings<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    let values = Vec::<String>::deserialize(deserializer)?;
    T::deserialize(Value::Array(
        values.into_iter().map(Value::String).collect(),
    ))
    .map_err(serde::de::Error::custom)
}

fn nullable_strings<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    let values = Option::<Vec<String>>::deserialize(deserializer)?;
    let value = values.map_or(Value::Null, |values| {
        Value::Array(values.into_iter().map(Value::String).collect())
    });
    T::deserialize(value).map_err(serde::de::Error::custom)
}

fn check_type<E: serde::de::Error>(object: &Map<String, Value>) -> Result<(), E> {
    // ItemReference permits null; the field's own derive checks nullability.
    if object
        .get("type")
        .is_some_and(|value| !value.is_string() && !value.is_null())
    {
        return Err(E::custom("type must be a string"));
    }
    Ok(())
}

fn object<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    let value = Map::<String, Value>::deserialize(deserializer)?;
    check_type::<D::Error>(&value)?;
    T::deserialize(Value::Object(value)).map_err(serde::de::Error::custom)
}

fn nullable_object<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = Option::<Map<String, Value>>::deserialize(deserializer)?;
    if let Some(ref object) = value {
        check_type::<D::Error>(object)?;
    }
    T::deserialize(value.map_or(Value::Null, Value::Object)).map_err(serde::de::Error::custom)
}

fn objects<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    let values = Vec::<Map<String, Value>>::deserialize(deserializer)?;
    for object in &values {
        check_type::<D::Error>(object)?;
    }
    T::deserialize(Value::Array(
        values.into_iter().map(Value::Object).collect(),
    ))
    .map_err(serde::de::Error::custom)
}

fn nullable_objects<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    let values = Option::<Vec<Map<String, Value>>>::deserialize(deserializer)?;
    if let Some(ref objects) = values {
        for object in objects {
            check_type::<D::Error>(object)?;
        }
    }
    let value = values.map_or(Value::Null, |values| {
        Value::Array(values.into_iter().map(Value::Object).collect())
    });
    T::deserialize(value).map_err(serde::de::Error::custom)
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateResponseRequest {
    #[serde(default, deserialize_with = "object")]
    pub(crate) access_programs: Option<AccessPrograms>,
    pub(crate) background: Option<bool>,
    /// Native vLLM cache isolation; distinct from the `OpenAI` `prompt_cache_key`.
    pub(crate) cache_salt: Option<String>,
    #[serde(default, deserialize_with = "nullable_objects")]
    pub(crate) context_management: Option<Vec<ContextManagement>>,
    pub(crate) conversation: Option<Conversation>,
    #[serde(default, deserialize_with = "nullable_strings")]
    pub(crate) include: Option<Vec<ResponseIncludable>>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) input: Option<ResponseInput>,
    pub(crate) instructions: Option<String>,
    pub(crate) max_output_tokens: Option<Number>,
    pub(crate) max_tool_calls: Option<Number>,
    pub(crate) metadata: Option<BTreeMap<String, String>>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) model: Option<String>,
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) moderation: Option<Moderation>,
    pub(crate) parallel_tool_calls: Option<bool>,
    pub(crate) previous_response_id: Option<String>,
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) prompt: Option<ResponsePrompt>,
    pub(crate) prompt_cache_key: Option<String>,
    #[serde(default, deserialize_with = "object")]
    pub(crate) prompt_cache_options: Option<PromptCacheOptions>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) prompt_cache_retention: Option<PromptCacheRetention>,
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) reasoning: Option<ReasoningConfig>,
    pub(crate) safety_identifier: Option<String>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) service_tier: Option<ServiceTier>,
    pub(crate) store: Option<bool>,
    pub(crate) stream: Option<bool>,
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) stream_options: Option<StreamOptions>,
    pub(crate) temperature: Option<Number>,
    #[serde(default, deserialize_with = "object")]
    pub(crate) text: Option<ResponseTextConfig>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) tool_choice: Option<ToolChoice>,
    #[serde(default, deserialize_with = "objects")]
    pub(crate) tools: Option<Vec<Tool>>,
    pub(crate) top_logprobs: Option<Number>,
    pub(crate) top_p: Option<Number>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) truncation: Option<Truncation>,
    pub(crate) user: Option<String>,
}

impl CreateResponseRequest {
    /// Check request-wide field relationships before model-specific preparation.
    pub(crate) fn validate(&self) -> Result<(), InputError> {
        if self.conversation.is_some() && self.previous_response_id.is_some() {
            return Err(InputError::new(
                "conversation",
                "do not combine conversation and previous_response_id",
            ));
        }
        if self.stream_options.is_some() && self.stream != Some(true) {
            return Err(InputError::new(
                "stream_options",
                "set stream to true when supplying stream_options",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CyberAccessProgram {
    Standard,
    DaybreakBlue,
    DaybreakRed,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct AccessPrograms {
    #[serde(default, deserialize_with = "string")]
    pub(crate) cyber: Option<CyberAccessProgram>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextManagement {
    pub(crate) r#type: String,
    pub(crate) compact_threshold: Option<Number>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(crate) enum Conversation {
    Id(String),
    Object { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ResponseIncludable {
    #[serde(rename = "file_search_call.results")]
    FileSearchCallResults,
    #[serde(rename = "web_search_call.results")]
    WebSearchCallResults,
    #[serde(rename = "web_search_call.action.sources")]
    WebSearchCallActionSources,
    #[serde(rename = "message.input_image.image_url")]
    MessageInputImageImageUrl,
    #[serde(rename = "computer_call_output.output.image_url")]
    ComputerCallOutputOutputImageUrl,
    #[serde(rename = "code_interpreter_call.outputs")]
    CodeInterpreterCallOutputs,
    #[serde(rename = "reasoning.encrypted_content")]
    ReasoningEncryptedContent,
    #[serde(rename = "message.output_text.logprobs")]
    MessageOutputTextLogprobs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptCacheBreakpointMode {
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromptCacheBreakpoint {
    #[serde(deserialize_with = "string")]
    pub(crate) mode: PromptCacheBreakpointMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseInputImageDetail {
    Low,
    High,
    Auto,
    Original,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseInputFileDetail {
    Auto,
    Low,
    High,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(crate) enum ResponseInputContent {
    #[serde(rename = "input_text")]
    Text {
        text: String,
        #[serde(default, deserialize_with = "object")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
    #[serde(rename = "input_image")]
    Image {
        #[serde(deserialize_with = "string")]
        detail: ResponseInputImageDetail,
        file_id: Option<String>,
        image_url: Option<String>,
        #[serde(default, deserialize_with = "object")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
    #[serde(rename = "input_file")]
    File {
        #[serde(default, deserialize_with = "string")]
        detail: Option<ResponseInputFileDetail>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        file_data: Option<String>,
        file_id: Option<String>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        file_url: Option<String>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        filename: Option<String>,
        #[serde(default, deserialize_with = "object")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum EasyInputMessageContent {
    TextInput(String),
    ResponseInputContent(#[serde(deserialize_with = "objects")] Vec<ResponseInputContent>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EasyInputMessageRole {
    User,
    Assistant,
    System,
    Developer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EasyInputMessagePhase {
    Commentary,
    FinalAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EasyInputMessageType {
    Message,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EasyInputMessage {
    pub(crate) content: EasyInputMessageContent,
    #[serde(deserialize_with = "string")]
    pub(crate) role: EasyInputMessageRole,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) phase: Option<EasyInputMessagePhase>,
    #[serde(default, deserialize_with = "string")]
    pub(crate) r#type: Option<EasyInputMessageType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageRole {
    User,
    System,
    Developer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemStatus {
    InProgress,
    Completed,
    Incomplete,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Message {
    #[serde(deserialize_with = "objects")]
    pub(crate) content: Vec<ResponseInputContent>,
    #[serde(deserialize_with = "string")]
    pub(crate) role: MessageRole,
    #[serde(default, deserialize_with = "string")]
    pub(crate) status: Option<ItemStatus>,
    #[serde(default, deserialize_with = "string")]
    pub(crate) r#type: Option<EasyInputMessageType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseOutputTextAnnotations {
    FileCitation {
        file_id: String,
        filename: String,
        index: Number,
    },
    UrlCitation {
        end_index: Number,
        start_index: Number,
        title: String,
        url: String,
    },
    ContainerFileCitation {
        container_id: String,
        end_index: Number,
        file_id: String,
        filename: String,
        start_index: Number,
    },
    FilePath {
        file_id: String,
        index: Number,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponseOutputTextLogprobsTopLogprobs {
    pub(crate) token: String,
    pub(crate) bytes: Vec<Number>,
    pub(crate) logprob: Number,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponseOutputTextLogprobs {
    pub(crate) token: String,
    pub(crate) bytes: Vec<Number>,
    pub(crate) logprob: Number,
    #[serde(deserialize_with = "objects")]
    pub(crate) top_logprobs: Vec<ResponseOutputTextLogprobsTopLogprobs>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseOutputMessageContent {
    OutputText {
        #[serde(deserialize_with = "objects")]
        annotations: Vec<ResponseOutputTextAnnotations>,
        #[serde(deserialize_with = "objects")]
        logprobs: Vec<ResponseOutputTextLogprobs>,
        text: String,
    },
    Refusal {
        refusal: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseOutputMessageRole {
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchCallStatus {
    InProgress,
    Searching,
    Completed,
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum FileSearchCallResultsAttributesValue {
    Text(String),
    Number(Number),
    Boolean(bool),
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileSearchCallResults {
    pub(crate) attributes: Option<BTreeMap<String, FileSearchCallResultsAttributesValue>>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) file_id: Option<String>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) filename: Option<String>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) score: Option<Number>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) text: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComputerCallPendingSafetyChecks {
    pub(crate) id: String,
    pub(crate) code: Option<String>,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClickButton {
    Left,
    Right,
    Wheel,
    Back,
    Forward,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DragPath {
    pub(crate) x: Number,
    pub(crate) y: Number,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComputerCallAction {
    Click {
        #[serde(deserialize_with = "string")]
        button: ClickButton,
        x: Number,
        y: Number,
        keys: Option<Vec<String>>,
    },
    DoubleClick {
        #[serialize_always]
        #[serde(deserialize_with = "Deserialize::deserialize")]
        keys: Option<Vec<String>>,
        x: Number,
        y: Number,
    },
    Drag {
        #[serde(deserialize_with = "objects")]
        path: Vec<DragPath>,
        keys: Option<Vec<String>>,
    },
    Keypress {
        keys: Vec<String>,
    },
    Move {
        x: Number,
        y: Number,
        keys: Option<Vec<String>>,
    },
    Screenshot {},
    Scroll {
        scroll_x: Number,
        scroll_y: Number,
        x: Number,
        y: Number,
        keys: Option<Vec<String>>,
    },
    Type {
        text: String,
    },
    Wait {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComputerCallOutputOutputType {
    ComputerScreenshot,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComputerCallOutputOutput {
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: ComputerCallOutputOutputType,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) file_id: Option<String>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) image_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchSourcesType {
    Url,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchSources {
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: SearchSourcesType,
    pub(crate) url: String,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebSearchCallAction {
    Search {
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        queries: Option<Vec<String>>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        query: Option<String>,
        #[serde(default, deserialize_with = "objects")]
        sources: Option<Vec<SearchSources>>,
    },
    OpenPage {
        url: Option<String>,
    },
    FindInPage {
        pattern: String,
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FunctionCallCaller {
    Direct {},
    Program { caller_id: String },
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(crate) enum FunctionOutputContent {
    #[serde(rename = "input_text")]
    Text {
        text: String,
        #[serde(default, deserialize_with = "nullable_object")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
    #[serde(rename = "input_image")]
    Image {
        #[serde(default, deserialize_with = "nullable_string")]
        detail: Option<ResponseInputImageDetail>,
        file_id: Option<String>,
        image_url: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
    #[serde(rename = "input_file")]
    File {
        #[serde(default, deserialize_with = "string")]
        detail: Option<ResponseInputFileDetail>,
        file_data: Option<String>,
        file_id: Option<String>,
        file_url: Option<String>,
        filename: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        prompt_cache_breakpoint: Option<PromptCacheBreakpoint>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum FunctionCallOutputOutput {
    Text(String),
    Content(#[serde(deserialize_with = "objects")] Vec<FunctionOutputContent>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolSearchCallExecution {
    Server,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FunctionAllowedCallers {
    Direct,
    Programmatic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ComparisonFilterArrayValue {
    Text(String),
    Number(Number),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ComparisonFilterValue {
    Text(String),
    Number(Number),
    Boolean(bool),
    Array(Vec<ComparisonFilterArrayValue>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompoundFilterFilters {
    Eq {
        key: String,
        value: ComparisonFilterValue,
    },
    Ne {
        key: String,
        value: ComparisonFilterValue,
    },
    Gt {
        key: String,
        value: ComparisonFilterValue,
    },
    Gte {
        key: String,
        value: ComparisonFilterValue,
    },
    Lt {
        key: String,
        value: ComparisonFilterValue,
    },
    Lte {
        key: String,
        value: ComparisonFilterValue,
    },
    In {
        key: String,
        value: ComparisonFilterValue,
    },
    Nin {
        key: String,
        value: ComparisonFilterValue,
    },
    And {
        #[serde(deserialize_with = "objects")]
        filters: Vec<Self>,
    },
    Or {
        #[serde(deserialize_with = "objects")]
        filters: Vec<Self>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileSearchRankingOptionsHybridSearch {
    pub(crate) embedding_weight: Number,
    pub(crate) text_weight: Number,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileSearchRankingOptionsRanker {
    Auto,
    #[serde(rename = "default-2024-11-15")]
    Default20241115,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileSearchRankingOptions {
    #[serde(default, deserialize_with = "object")]
    pub(crate) hybrid_search: Option<FileSearchRankingOptionsHybridSearch>,
    #[serde(default, deserialize_with = "string")]
    pub(crate) ranker: Option<FileSearchRankingOptionsRanker>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) score_threshold: Option<Number>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComputerUsePreviewEnvironment {
    Windows,
    Mac,
    Linux,
    Ubuntu,
    Browser,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebSearchFilters {
    pub(crate) allowed_domains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebSearchSearchContextSize {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebSearchUserLocationType {
    Approximate,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebSearchUserLocation {
    pub(crate) city: Option<String>,
    pub(crate) country: Option<String>,
    pub(crate) region: Option<String>,
    pub(crate) timezone: Option<String>,
    #[serde(default, deserialize_with = "string")]
    pub(crate) r#type: Option<WebSearchUserLocationType>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpToolFilter {
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) read_only: Option<bool>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) tool_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum McpAllowedTools {
    McpAllowedTools(Vec<String>),
    McpToolFilter(#[serde(deserialize_with = "object")] Box<McpToolFilter>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum McpConnectorId {
    #[serde(rename = "connector_dropbox")]
    Dropbox,
    #[serde(rename = "connector_gmail")]
    Gmail,
    #[serde(rename = "connector_googlecalendar")]
    GoogleCalendar,
    #[serde(rename = "connector_googledrive")]
    GoogleDrive,
    #[serde(rename = "connector_microsoftteams")]
    MicrosoftTeams,
    #[serde(rename = "connector_outlookcalendar")]
    OutlookCalendar,
    #[serde(rename = "connector_outlookemail")]
    OutlookEmail,
    #[serde(rename = "connector_sharepoint")]
    SharePoint,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpToolApprovalFilter {
    #[serde(default, deserialize_with = "object")]
    pub(crate) always: Option<McpToolFilter>,
    #[serde(default, deserialize_with = "object")]
    pub(crate) never: Option<McpToolFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpToolApprovalSetting {
    Always,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum McpRequireApproval {
    McpToolApprovalFilter(#[serde(deserialize_with = "object")] Box<McpToolApprovalFilter>),
    McpToolApprovalSetting(#[serde(deserialize_with = "string")] Box<McpToolApprovalSetting>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeInterpreterToolAutoType {
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CodeInterpreterToolAutoMemoryLimit {
    #[serde(rename = "1g")]
    Value1g,
    #[serde(rename = "4g")]
    Value4g,
    #[serde(rename = "16g")]
    Value16g,
    #[serde(rename = "64g")]
    Value64g,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContainerNetworkPolicyAllowlistDomainSecrets {
    pub(crate) domain: String,
    pub(crate) name: String,
    pub(crate) value: String,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeInterpreterToolAutoNetworkPolicy {
    Disabled {},
    Allowlist {
        allowed_domains: Vec<String>,
        #[serde(default, deserialize_with = "objects")]
        domain_secrets: Option<Vec<ContainerNetworkPolicyAllowlistDomainSecrets>>,
    },
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeInterpreterToolAuto {
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: CodeInterpreterToolAutoType,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) file_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) memory_limit: Option<CodeInterpreterToolAutoMemoryLimit>,
    #[serde(default, deserialize_with = "object")]
    pub(crate) network_policy: Option<CodeInterpreterToolAutoNetworkPolicy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum CodeInterpreterContainer {
    Text(String),
    CodeInterpreterToolAuto(#[serde(deserialize_with = "object")] Box<CodeInterpreterToolAuto>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationAction {
    Generate,
    Edit,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationBackground {
    Transparent,
    Opaque,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationInputFidelity {
    High,
    Low,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageGenerationInputImageMask {
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) file_id: Option<String>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) image_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationModeration {
    Auto,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationOutputFormat {
    Png,
    Webp,
    Jpeg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationQuality {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum InlineSkillSourceMediaType {
    #[serde(rename = "application/zip")]
    ApplicationZip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InlineSkillSourceType {
    Base64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InlineSkillSource {
    pub(crate) data: String,
    #[serde(deserialize_with = "string")]
    pub(crate) media_type: InlineSkillSourceMediaType,
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: InlineSkillSourceType,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContainerAutoSkills {
    SkillReference {
        skill_id: String,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        version: Option<String>,
    },
    Inline {
        description: String,
        name: String,
        #[serde(deserialize_with = "object")]
        source: InlineSkillSource,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalEnvironmentSkills {
    pub(crate) description: String,
    pub(crate) name: String,
    pub(crate) path: String,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShellEnvironment {
    ContainerAuto {
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        file_ids: Option<Vec<String>>,
        #[serde(default, deserialize_with = "nullable_string")]
        memory_limit: Option<CodeInterpreterToolAutoMemoryLimit>,
        #[serde(default, deserialize_with = "object")]
        network_policy: Option<CodeInterpreterToolAutoNetworkPolicy>,
        #[serde(default, deserialize_with = "objects")]
        skills: Option<Vec<ContainerAutoSkills>>,
    },
    Local {
        #[serde(default, deserialize_with = "objects")]
        skills: Option<Vec<LocalEnvironmentSkills>>,
    },
    ContainerReference {
        container_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GrammarSyntax {
    Lark,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CustomFormat {
    Text {},
    Grammar {
        definition: String,
        #[serde(deserialize_with = "string")]
        syntax: GrammarSyntax,
    },
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NamespaceTools {
    Function {
        name: String,
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        r#async: Option<bool>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        defer_loading: Option<bool>,
        description: Option<String>,
        output_schema: Option<BTreeMap<String, Value>>,
        parameters: Option<Value>,
        strict: Option<bool>,
    },
    Custom {
        name: String,
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        r#async: Option<bool>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        defer_loading: Option<bool>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        description: Option<String>,
        #[serde(default, deserialize_with = "object")]
        format: Option<CustomFormat>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebSearchPreviewSearchContentTypes {
    Text,
    Image,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebSearchPreviewUserLocation {
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: WebSearchUserLocationType,
    pub(crate) city: Option<String>,
    pub(crate) country: Option<String>,
    pub(crate) region: Option<String>,
    pub(crate) timezone: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tool {
    Function {
        name: String,
        #[serialize_always]
        #[serde(deserialize_with = "Deserialize::deserialize")]
        parameters: Option<BTreeMap<String, Value>>,
        strict: Option<bool>,
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        r#async: Option<bool>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        defer_loading: Option<bool>,
        description: Option<String>,
        output_schema: Option<BTreeMap<String, Value>>,
    },
    FileSearch {
        vector_store_ids: Vec<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        filters: Option<CompoundFilterFilters>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        max_num_results: Option<Number>,
        #[serde(default, deserialize_with = "object")]
        ranking_options: Option<FileSearchRankingOptions>,
    },
    Computer {},
    ComputerUsePreview {
        display_height: Number,
        display_width: Number,
        #[serde(deserialize_with = "string")]
        environment: ComputerUsePreviewEnvironment,
    },
    WebSearch {
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        external_web_access: Option<bool>,
        #[serde(default, deserialize_with = "nullable_object")]
        filters: Option<WebSearchFilters>,
        #[serde(default, deserialize_with = "string")]
        search_context_size: Option<WebSearchSearchContextSize>,
        #[serde(default, deserialize_with = "nullable_object")]
        user_location: Option<WebSearchUserLocation>,
    },
    #[serde(rename = "web_search_2025_08_26")]
    WebSearch20250826 {
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        external_web_access: Option<bool>,
        #[serde(default, deserialize_with = "nullable_object")]
        filters: Option<WebSearchFilters>,
        #[serde(default, deserialize_with = "string")]
        search_context_size: Option<WebSearchSearchContextSize>,
        #[serde(default, deserialize_with = "nullable_object")]
        user_location: Option<WebSearchUserLocation>,
    },
    Mcp {
        server_label: String,
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
        allowed_tools: Option<Vec<McpAllowedTools>>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        authorization: Option<String>,
        #[serde(default, deserialize_with = "string")]
        connector_id: Option<McpConnectorId>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        defer_loading: Option<bool>,
        headers: Option<BTreeMap<String, String>>,
        require_approval: Option<McpRequireApproval>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        server_description: Option<String>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        server_url: Option<String>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        tunnel_id: Option<String>,
    },
    CodeInterpreter {
        container: CodeInterpreterContainer,
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
    },
    ProgrammaticToolCalling {},
    ImageGeneration {
        #[serde(default, deserialize_with = "string")]
        action: Option<ImageGenerationAction>,
        #[serde(default, deserialize_with = "string")]
        background: Option<ImageGenerationBackground>,
        #[serde(default, deserialize_with = "nullable_string")]
        input_fidelity: Option<ImageGenerationInputFidelity>,
        #[serde(default, deserialize_with = "object")]
        input_image_mask: Option<ImageGenerationInputImageMask>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        model: Option<String>,
        #[serde(default, deserialize_with = "string")]
        moderation: Option<ImageGenerationModeration>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        output_compression: Option<Number>,
        #[serde(default, deserialize_with = "string")]
        output_format: Option<ImageGenerationOutputFormat>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        partial_images: Option<Number>,
        #[serde(default, deserialize_with = "string")]
        quality: Option<ImageGenerationQuality>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        size: Option<String>,
    },
    LocalShell {},
    Shell {
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
        #[serde(default, deserialize_with = "nullable_object")]
        environment: Option<ShellEnvironment>,
    },
    Custom {
        name: String,
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        r#async: Option<bool>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        defer_loading: Option<bool>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        description: Option<String>,
        #[serde(default, deserialize_with = "object")]
        format: Option<CustomFormat>,
    },
    Namespace {
        description: String,
        name: String,
        #[serde(deserialize_with = "objects")]
        tools: Vec<NamespaceTools>,
    },
    #[serde(rename = "tool_search")]
    Search {
        description: Option<String>,
        #[serde(default, deserialize_with = "string")]
        execution: Option<ToolSearchCallExecution>,
        parameters: Option<Value>,
    },
    WebSearchPreview {
        #[serde(default, deserialize_with = "strings")]
        search_content_types: Option<Vec<WebSearchPreviewSearchContentTypes>>,
        #[serde(default, deserialize_with = "string")]
        search_context_size: Option<WebSearchSearchContextSize>,
        #[serde(default, deserialize_with = "nullable_object")]
        user_location: Option<WebSearchPreviewUserLocation>,
    },
    #[serde(rename = "web_search_preview_2025_03_11")]
    WebSearchPreview20250311 {
        #[serde(default, deserialize_with = "strings")]
        search_content_types: Option<Vec<WebSearchPreviewSearchContentTypes>>,
        #[serde(default, deserialize_with = "string")]
        search_context_size: Option<WebSearchSearchContextSize>,
        #[serde(default, deserialize_with = "nullable_object")]
        user_location: Option<WebSearchPreviewUserLocation>,
    },
    ApplyPatch {
        #[serde(default, deserialize_with = "nullable_strings")]
        allowed_callers: Option<Vec<FunctionAllowedCallers>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdditionalToolsRole {
    Developer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigurationUpdateReasoning {
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningSummaryType {
    SummaryText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReasoningSummary {
    pub(crate) text: String,
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: ReasoningSummaryType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningContentType {
    ReasoningText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReasoningContent {
    pub(crate) text: String,
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: ReasoningContentType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageGenerationCallStatus {
    InProgress,
    Completed,
    Generating,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeInterpreterCallOutputs {
    Logs { logs: String },
    Image { url: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodeInterpreterCallStatus {
    InProgress,
    Completed,
    Incomplete,
    Interpreting,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalShellCallActionType {
    Exec,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocalShellCallAction {
    pub(crate) command: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    #[serde(deserialize_with = "string")]
    pub(crate) r#type: LocalShellCallActionType,
    pub(crate) timeout_ms: Option<Number>,
    pub(crate) user: Option<String>,
    pub(crate) working_directory: Option<String>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShellCallAction {
    pub(crate) commands: Vec<String>,
    pub(crate) max_output_length: Option<Number>,
    pub(crate) timeout_ms: Option<Number>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShellCallEnvironment {
    Local {
        #[serde(default, deserialize_with = "objects")]
        skills: Option<Vec<LocalEnvironmentSkills>>,
    },
    ContainerReference {
        container_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShellCallOutputOutputOutcome {
    Timeout {},
    Exit { exit_code: Number },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShellCallOutputOutput {
    #[serde(deserialize_with = "object")]
    pub(crate) outcome: ShellCallOutputOutputOutcome,
    pub(crate) stderr: String,
    pub(crate) stdout: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(crate) enum ApplyPatchCallOperation {
    #[serde(rename = "create_file")]
    Create { diff: String, path: String },
    #[serde(rename = "delete_file")]
    Delete { path: String },
    #[serde(rename = "update_file")]
    Update { diff: String, path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApplyPatchCallStatus {
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApplyPatchCallOutputStatus {
    Completed,
    Failed,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpListToolsTools {
    pub(crate) input_schema: Value,
    pub(crate) name: String,
    pub(crate) annotations: Option<Value>,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(crate) enum McpCallError {
    #[serde(rename = "mcp_protocol_error")]
    Protocol { code: Number, message: String },
    #[serde(rename = "mcp_tool_execution_error")]
    ToolExecution { content: Value },
    #[serde(rename = "http_error")]
    Http { code: Number, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum McpCallStatus {
    InProgress,
    Completed,
    Incomplete,
    Calling,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum CustomToolCallOutputOutput {
    StringOutput(String),
    ResponseInputContent(#[serde(deserialize_with = "objects")] Vec<ResponseInputContent>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemReferenceType {
    ItemReference,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemReference {
    pub(crate) id: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) r#type: Option<ItemReferenceType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProgramOutputStatus {
    Completed,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ResponseInputItem {
    EasyInputMessage(#[serde(deserialize_with = "object")] Box<EasyInputMessage>),
    Message(#[serde(deserialize_with = "object")] Box<Message>),
    Item(#[serde(deserialize_with = "object")] Box<ResponseItem>),
    ItemReference(#[serde(deserialize_with = "object")] Box<ItemReference>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ResponseInput {
    TextInput(String),
    ResponseInputItem(Vec<ResponseInputItem>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModerationMode {
    Score,
    Block,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModerationAction {
    #[serde(deserialize_with = "string")]
    pub(crate) mode: ModerationMode,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModerationPolicy {
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) input: Option<ModerationAction>,
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) output: Option<ModerationAction>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Moderation {
    pub(crate) model: String,
    #[serde(default, deserialize_with = "nullable_object")]
    pub(crate) policy: Option<ModerationPolicy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum PromptVariable {
    Text(String),
    Content(#[serde(deserialize_with = "object")] ResponseInputContent),
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponsePrompt {
    pub(crate) id: String,
    pub(crate) variables: Option<BTreeMap<String, PromptVariable>>,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptCacheMode {
    Implicit,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum PromptCacheTtl {
    #[serde(rename = "30m")]
    Value30m,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromptCacheOptions {
    pub(crate) comparison_response_id: Option<String>,
    #[serde(default, deserialize_with = "string")]
    pub(crate) mode: Option<PromptCacheMode>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) prewarm: Option<bool>,
    #[serde(default, deserialize_with = "string")]
    pub(crate) ttl: Option<PromptCacheTtl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptCacheRetention {
    InMemory,
    #[serde(rename = "24h")]
    Value24h,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningContext {
    Auto,
    CurrentTurn,
    AllTurns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningSummaryMode {
    Auto,
    Concise,
    Detailed,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReasoningConfig {
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) context: Option<ReasoningContext>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) effort: Option<ReasoningEffort>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) generate_summary: Option<ReasoningSummaryMode>,
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) mode: Option<String>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) summary: Option<ReasoningSummaryMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServiceTier {
    Auto,
    Default,
    Flex,
    Scale,
    Priority,
    Fast,
    Ultrafast,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamOptions {
    #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
    pub(crate) include_obfuscation: Option<bool>,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseTextFormat {
    Text {},
    JsonSchema {
        name: String,
        schema: BTreeMap<String, Value>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        description: Option<String>,
        strict: Option<bool>,
    },
    JsonObject {},
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponseTextConfig {
    #[serde(default, deserialize_with = "object")]
    pub(crate) format: Option<ResponseTextFormat>,
    #[serde(default, deserialize_with = "nullable_string")]
    pub(crate) verbosity: Option<WebSearchSearchContextSize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolChoiceOptions {
    None,
    Auto,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolChoiceAllowedMode {
    Auto,
    Required,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolChoice {
    ToolChoiceOptions(#[serde(deserialize_with = "string")] ToolChoiceOptions),
    Item(#[serde(deserialize_with = "object")] Box<ToolChoiceObject>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Truncation {
    Auto,
    Disabled,
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseItem {
    Message {
        id: String,
        #[serde(deserialize_with = "objects")]
        content: Vec<ResponseOutputMessageContent>,
        #[serde(deserialize_with = "string")]
        role: ResponseOutputMessageRole,
        #[serde(deserialize_with = "string")]
        status: ItemStatus,
        #[serde(default, deserialize_with = "nullable_string")]
        phase: Option<EasyInputMessagePhase>,
    },
    FileSearchCall {
        id: String,
        queries: Vec<String>,
        #[serde(deserialize_with = "string")]
        status: SearchCallStatus,
        #[serde(default, deserialize_with = "nullable_objects")]
        results: Option<Vec<FileSearchCallResults>>,
    },
    ComputerCall {
        id: String,
        call_id: String,
        #[serde(deserialize_with = "objects")]
        pending_safety_checks: Vec<ComputerCallPendingSafetyChecks>,
        #[serde(deserialize_with = "string")]
        status: ItemStatus,
        #[serde(default, deserialize_with = "object")]
        action: Option<ComputerCallAction>,
        #[serde(default, deserialize_with = "objects")]
        actions: Option<Vec<ComputerCallAction>>,
    },
    ComputerCallOutput {
        call_id: String,
        #[serde(deserialize_with = "object")]
        output: ComputerCallOutputOutput,
        id: Option<String>,
        #[serde(default, deserialize_with = "nullable_objects")]
        acknowledged_safety_checks: Option<Vec<ComputerCallPendingSafetyChecks>>,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    WebSearchCall {
        id: String,
        #[serde(deserialize_with = "string")]
        status: SearchCallStatus,
        #[serde(default, deserialize_with = "object")]
        action: Option<WebSearchCallAction>,
    },
    FunctionCall {
        arguments: String,
        call_id: String,
        name: String,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        id: Option<String>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        r#async: Option<bool>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        namespace: Option<String>,
        #[serde(default, deserialize_with = "string")]
        status: Option<ItemStatus>,
    },
    FunctionCallOutput {
        output: FunctionCallOutputOutput,
        id: Option<String>,
        call_id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
        name: Option<String>,
        namespace: Option<String>,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    ToolSearchCall {
        arguments: Value,
        id: Option<String>,
        call_id: Option<String>,
        #[serde(default, deserialize_with = "string")]
        execution: Option<ToolSearchCallExecution>,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    ToolSearchOutput {
        #[serde(deserialize_with = "objects")]
        tools: Vec<Tool>,
        id: Option<String>,
        call_id: Option<String>,
        #[serde(default, deserialize_with = "string")]
        execution: Option<ToolSearchCallExecution>,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    AdditionalTools {
        #[serde(deserialize_with = "string")]
        role: AdditionalToolsRole,
        #[serde(deserialize_with = "objects")]
        tools: Vec<Tool>,
        id: Option<String>,
    },
    ConfigurationUpdate {
        id: Option<String>,
        #[serde(default, deserialize_with = "object")]
        reasoning: Option<ConfigurationUpdateReasoning>,
    },
    Reasoning {
        id: String,
        #[serde(deserialize_with = "objects")]
        summary: Vec<ReasoningSummary>,
        #[serde(default, deserialize_with = "objects")]
        content: Option<Vec<ReasoningContent>>,
        encrypted_content: Option<String>,
        #[serde(default, deserialize_with = "string")]
        status: Option<ItemStatus>,
    },
    Compaction {
        encrypted_content: String,
        id: Option<String>,
    },
    ImageGenerationCall {
        id: String,
        #[serialize_always]
        #[serde(deserialize_with = "Deserialize::deserialize")]
        result: Option<String>,
        #[serde(deserialize_with = "string")]
        status: ImageGenerationCallStatus,
        #[serde(default, deserialize_with = "nullable_string")]
        action: Option<ImageGenerationAction>,
        #[serde(default, deserialize_with = "nullable_string")]
        background: Option<ImageGenerationBackground>,
        #[serde(default, deserialize_with = "nullable_string")]
        output_format: Option<ImageGenerationOutputFormat>,
        #[serde(default, deserialize_with = "nullable_string")]
        quality: Option<ImageGenerationQuality>,
        revised_prompt: Option<String>,
        size: Option<String>,
    },
    CodeInterpreterCall {
        id: String,
        #[serialize_always]
        #[serde(deserialize_with = "Deserialize::deserialize")]
        code: Option<String>,
        container_id: String,
        #[serialize_always]
        #[serde(deserialize_with = "nullable_objects")]
        outputs: Option<Vec<CodeInterpreterCallOutputs>>,
        #[serde(deserialize_with = "string")]
        status: CodeInterpreterCallStatus,
    },
    LocalShellCall {
        id: String,
        #[serde(deserialize_with = "object")]
        action: LocalShellCallAction,
        call_id: String,
        #[serde(deserialize_with = "string")]
        status: ItemStatus,
    },
    LocalShellCallOutput {
        id: String,
        output: String,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    ShellCall {
        #[serde(deserialize_with = "object")]
        action: ShellCallAction,
        call_id: String,
        id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
        #[serde(default, deserialize_with = "nullable_object")]
        environment: Option<ShellCallEnvironment>,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    ShellCallOutput {
        call_id: String,
        #[serde(deserialize_with = "objects")]
        output: Vec<ShellCallOutputOutput>,
        id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
        max_output_length: Option<Number>,
        #[serde(default, deserialize_with = "nullable_string")]
        status: Option<ItemStatus>,
    },
    ApplyPatchCall {
        call_id: String,
        #[serde(deserialize_with = "object")]
        operation: ApplyPatchCallOperation,
        #[serde(deserialize_with = "string")]
        status: ApplyPatchCallStatus,
        id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
    },
    ApplyPatchCallOutput {
        call_id: String,
        #[serde(deserialize_with = "string")]
        status: ApplyPatchCallOutputStatus,
        id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
        output: Option<String>,
    },
    McpListTools {
        id: String,
        server_label: String,
        #[serde(deserialize_with = "objects")]
        tools: Vec<McpListToolsTools>,
        error: Option<String>,
    },
    McpApprovalRequest {
        id: String,
        arguments: String,
        name: String,
        server_label: String,
    },
    McpApprovalResponse {
        approval_request_id: String,
        approve: bool,
        id: Option<String>,
        reason: Option<String>,
    },
    McpCall {
        id: String,
        arguments: String,
        name: String,
        server_label: String,
        approval_request_id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        error: Option<McpCallError>,
        output: Option<String>,
        #[serde(default, deserialize_with = "string")]
        status: Option<McpCallStatus>,
    },
    CustomToolCallOutput {
        call_id: String,
        output: CustomToolCallOutputOutput,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        id: Option<String>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
    },
    CustomToolCall {
        call_id: String,
        input: String,
        name: String,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        id: Option<String>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        r#async: Option<bool>,
        #[serde(default, deserialize_with = "nullable_object")]
        caller: Option<FunctionCallCaller>,
        #[serde(default, with = "serde_with::rust::unwrap_or_skip")]
        namespace: Option<String>,
    },
    CompactionTrigger {
        id: Option<String>,
    },
    Program {
        id: String,
        call_id: String,
        code: String,
        fingerprint: String,
    },
    ProgramOutput {
        id: String,
        call_id: String,
        result: String,
        #[serde(deserialize_with = "string")]
        status: ProgramOutputStatus,
    },
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolChoiceObject {
    AllowedTools {
        #[serde(deserialize_with = "string")]
        mode: ToolChoiceAllowedMode,
        tools: Vec<BTreeMap<String, Value>>,
    },
    FileSearch {},
    WebSearchPreview {},
    Computer {},
    ComputerUsePreview {},
    ComputerUse {},
    #[serde(rename = "web_search_preview_2025_03_11")]
    WebSearchPreview20250311 {},
    ImageGeneration {},
    CodeInterpreter {},
    Function {
        name: String,
    },
    Mcp {
        server_label: String,
        name: Option<String>,
    },
    Custom {
        name: String,
    },
    ProgrammaticToolCalling {},
    ApplyPatch {},
    Shell {},
}

#[cfg(test)]
mod tests {
    use super::CreateResponseRequest;
    use serde_json::{Value, json};

    fn round_trip(value: Value) -> CreateResponseRequest {
        let request: CreateResponseRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&request).unwrap(), value);
        request
    }

    // These tests cover JSON field representations, not whether every fixture
    // is a complete request that a particular model can execute.
    // Source: https://developers.openai.com/api/reference/resources/responses/methods/create
    #[test]
    fn accepts_optional_nullable_fields() {
        for value in [
            json!({}),
            json!({"store": null, "metadata": null, "instructions": null}),
            json!({"input": [{"id": "msg_previous", "type": null}]}),
        ] {
            assert!(serde_json::from_value::<CreateResponseRequest>(value).is_ok());
        }
        round_trip(json!({
            "model": "gpt-4.1", "input": "Hello", "store": false,
            "temperature": 0, "tools": [], "metadata": {}
        }));
    }

    #[test]
    fn preserves_arbitrary_json_in_function_parameters() {
        round_trip(json!({
            "tools": [{
                "type": "function", "name": "lookup", "strict": false,
                "parameters": {"type": "object", "properties": {"value": {"default": null}}}
            }]
        }));
    }

    #[test]
    fn preserves_multimodal_messages_and_tool_history() {
        round_trip(json!({
            "model": "gpt-4.1", "store": false,
            "include": ["reasoning.encrypted_content"],
            "input": [
                {"role": "system", "content": "Describe the supplied files."},
                {"role": "user", "type": "message", "content": [
                    {"type": "input_text", "text": "Read these", "prompt_cache_breakpoint": {"mode": "explicit"}},
                    {"type": "input_image", "detail": "auto", "image_url": "https://example.com/image.png"},
                    {"type": "input_file", "file_id": "file_example"}
                ]},
                {"type": "message", "id": "msg_example", "role": "assistant", "status": "completed",
                 "phase": "commentary", "content": [
                    {"type": "output_text", "text": "Checking", "annotations": [], "logprobs": []}
                 ]},
                {"type": "reasoning", "id": "rs_example", "summary": [], "encrypted_content": "opaque"},
                {"type": "function_call", "call_id": "call_example", "name": "lookup", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_example", "output": [
                    {"type": "input_text", "text": "Found"},
                    {"type": "input_image", "image_url": "https://example.com/result.png"}
                ]},
                {"type": "item_reference", "id": "msg_previous"}
            ]
        }));
    }

    #[test]
    fn preserves_custom_tools_filters_and_structured_output() {
        round_trip(json!({
            "model": "gpt-4.1", "input": "Look up an order",
            "tools": [
                {"type": "function", "name": "lookup", "parameters": {
                    "type": "object", "properties": {"id": {"type": "string"}},
                    "required": ["id"], "additionalProperties": false
                }, "strict": true},
                {"type": "custom", "name": "calculator", "format": {
                    "type": "grammar", "syntax": "regex", "definition": "[0-9]+"
                }},
                {"type": "file_search", "vector_store_ids": ["vs_example"], "filters": {
                    "type": "and", "filters": [
                        {"type": "eq", "key": "category", "value": "orders"},
                        {"type": "or", "filters": [
                            {"type": "gt", "key": "year", "value": 2020},
                            {"type": "eq", "key": "active", "value": true}
                        ]}
                    ]
                }},
                {"type": "mcp", "server_label": "orders", "server_url": "https://example.com/mcp",
                 "require_approval": {"always": {"tool_names": ["lookup"]}}}
            ],
            "tool_choice": {"type": "function", "name": "lookup"},
            "text": {"format": {
                "type": "json_schema", "name": "answer", "strict": true,
                "schema": {"type": "object", "properties": {"found": {"type": "boolean"}},
                           "required": ["found"], "additionalProperties": false}
            }}
        }));
    }

    #[test]
    fn preserves_batched_computer_actions_and_mcp_errors() {
        round_trip(json!({"input": [
            {"type": "computer_call", "id": "cu_example", "call_id": "call_example",
             "status": "completed", "pending_safety_checks": [], "actions": [
                {"type": "click", "button": "left", "x": 10, "y": 20},
                {"type": "type", "text": "Hello"}
             ]},
            {"type": "mcp_call", "id": "mcp_example", "server_label": "orders", "name": "lookup",
             "arguments": "{}", "error": {"type": "mcp_protocol_error", "code": -32602, "message": "Invalid arguments"}}
        ]}));
    }

    #[test]
    fn preserves_request_configuration_and_prompt_variables() {
        round_trip(json!({
            "access_programs": {"cyber": "standard"}, "background": false,
            "context_management": [{"type": "compaction", "compact_threshold": 10000}],
            "conversation": {"id": "conv_example"},
            "instructions": "Be concise", "max_output_tokens": 256, "max_tool_calls": 2,
            "metadata": {"test": "configuration"}, "model": "gpt-4.1",
            "moderation": {"model": "omni-moderation-latest", "policy": {"input": {"mode": "score"}}},
            "parallel_tool_calls": false,
            "prompt": {"id": "pmpt_example", "version": "1", "variables": {
                "name": "Ada", "file": {"type": "input_file", "file_id": "file_example"}
            }},
            "prompt_cache_key": "example", "prompt_cache_options": {"mode": "explicit", "ttl": "30m", "prewarm": false},
            "reasoning": {"effort": "low", "summary": "auto", "context": "auto"},
            "safety_identifier": "example", "service_tier": "default", "store": false,
            "stream": true, "stream_options": {"include_obfuscation": false},
            "temperature": 0.5, "text": {"format": {"type": "text"}, "verbosity": "low"},
            "tool_choice": "auto", "tools": [], "top_logprobs": 2, "top_p": 0.9,
            "truncation": "disabled"
        }));
        round_trip(json!({"conversation": "conv_example"}));
        round_trip(json!({"previous_response_id": "resp_example"}));
    }

    // https://developers.openai.com/api/docs/guides/function-calling#strict-mode
    #[test]
    fn accepts_function_tools_without_strict() {
        round_trip(json!({
            "tools": [{"type": "function", "name": "lookup", "parameters": {"type": "object", "properties": {}}}]
        }));
    }

    #[test]
    fn rejects_non_string_enum_values() {
        for (valid, pointer) in [
            (json!({"tools": [{"type": "local_shell"}]}), "/tools/0/type"),
            (
                json!({"input": [{"id": "call_test", "output": "ok", "type": "local_shell_call_output"}]}),
                "/input/0/type",
            ),
            (
                json!({"input": [{"id": "call_test", "output": "ok", "type": "local_shell_call_output", "status": "completed"}]}),
                "/input/0/status",
            ),
            (
                json!({"input": [{"role": "user", "content": "Hello", "type": "message"}]}),
                "/input/0/type",
            ),
            (
                json!({"input": [{"role": "user", "content": "Hello"}]}),
                "/input/0/role",
            ),
            (
                json!({"input": [{"role": "user", "content": [{"type": "input_text", "text": "Hello"}]}]}),
                "/input/0/content/0/type",
            ),
            (json!({"tool_choice": "auto"}), "/tool_choice"),
            (json!({"service_tier": "default"}), "/service_tier"),
            (
                json!({"tools": [{"type": "mcp", "server_label": "example", "require_approval": "always"}]}),
                "/tools/0/require_approval",
            ),
        ] {
            round_trip(valid.clone());
            let literal = valid.pointer(pointer).unwrap().as_str().unwrap();
            for invalid in [
                json!(0),
                json!(true),
                json!([]),
                json!([literal]),
                json!({literal: null}),
            ] {
                let mut request = valid.clone();
                *request.pointer_mut(pointer).unwrap() = invalid;
                assert!(
                    serde_json::from_value::<CreateResponseRequest>(request.clone()).is_err(),
                    "accepted {request}"
                );
                assert!(
                    serde_json::from_str::<CreateResponseRequest>(&request.to_string()).is_err(),
                    "accepted {request}"
                );
            }
        }
    }

    #[test]
    fn keeps_image_and_file_detail_values_distinct() {
        round_trip(
            json!({"input": [{"role": "user", "content": [{"type": "input_image", "detail": "original", "file_id": "file_test"}]}]}),
        );
        let file = json!({"input": [{"role": "user", "content": [{"type": "input_file", "detail": "original", "file_id": "file_test"}]}]});
        assert!(serde_json::from_value::<CreateResponseRequest>(file).is_err());
    }

    #[test]
    fn rejects_undocumented_field_types_and_values() {
        for value in [
            json!({"model": null}),
            json!({"input": null}),
            json!({"input": [{"role": "unknown", "content": "Hello"}]}),
            json!({"input": [{"type": "function_call", "call_id": "call_example", "name": "lookup"}]}),
            json!({"input": [{"type": "input_image", "image_url": "https://example.com/image.png"}]}),
            json!({"tools": [{"type": "unknown"}]}),
            json!({"stream_options": {"include_obfuscation": null}}),
        ] {
            assert!(
                serde_json::from_value::<CreateResponseRequest>(value.clone()).is_err(),
                "accepted {value}"
            );
        }
        assert!(serde_json::from_value::<CreateResponseRequest>(
            json!({"tools": [{"type": "function", "name": "lookup", "parameters": null, "strict": null}]})
        ).is_ok());
    }
}
