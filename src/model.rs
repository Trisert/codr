use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub cost_in_currency: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum ModelType {
    Anthropic,
    OpenAI {
        base_url: String,
        model: String,
        api_key: Option<String>,
        /// Manual override for native tool support. If Some(true), force enable.
        /// If Some(false), force disable (use prompt-based calling).
        /// If None, run the automatic probe.
        native_tools_override: Option<bool>,
        extra: std::collections::HashMap<String, serde_json::Value>,
    },
}

#[derive(Clone)]
pub struct Model {
    client: Client,
    config: ModelConfig,
    usage: Arc<Mutex<Usage>>,
    base_url: Option<String>,  // Store base_url for interrupt requests
    /// Cached result of runtime tool support probe (None = not yet probed)
    tool_support_probed: Arc<Mutex<Option<bool>>>,
}

#[derive(Clone)]
struct ModelConfig {
    model_type: ModelType,
}

/// Parameters for OpenAI-compatible streaming queries
struct OpenAICompatParams<'a> {
    base_url: &'a str,
    model: &'a str,
    api_key: Option<&'a str>,
    extra: &'a std::collections::HashMap<String, serde_json::Value>,
}

/// Parameters for OpenAI-compatible streaming queries with tools
struct OpenAICompatParamsWithTools<'a> {
    base_url: &'a str,
    model: &'a str,
    api_key: Option<&'a str>,
    extra: &'a std::collections::HashMap<String, serde_json::Value>,
    tools: Option<&'a [OpenAIToolDefinition]>,
}

// ============================================================
// Message types - Generic representation
// ============================================================

#[derive(Debug, Clone)]
pub struct ImageAttachment {
    /// Base64 encoded image data (without data URI prefix)
    pub data: Arc<String>,
    /// MIME type (e.g., "image/png", "image/jpeg")
    pub media_type: Arc<str>,
}

// Custom Serialize for ImageAttachment to handle Arc types
impl Serialize for ImageAttachment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ImageAttachment", 2)?;
        state.serialize_field("data", &self.data.as_str())?;
        state.serialize_field("media_type", &self.media_type.as_ref())?;
        state.end()
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Arc<str>,  // Shared, immutable
    pub content: Arc<String>,  // Shared, potentially large
    /// Optional image attachments for vision support
    pub images: Vec<ImageAttachment>,
}

// Custom Serialize for Message that converts Arc to owned strings
impl Serialize for Message {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Message", 3)?;
        state.serialize_field("role", &self.role.to_string())?;
        state.serialize_field("content", &self.content.as_str())?;
        state.serialize_field("images", &self.images)?;
        state.end()
    }
}

// ============================================================
// Cross-Provider Context Handoff
// ============================================================

/// Serializable version of Message for cross-provider handoff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableMessage {
    pub role: String,
    pub content: String,
    pub images: Vec<SerializableImageAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableImageAttachment {
    pub data: String,
    pub media_type: String,
}

impl From<&Message> for SerializableMessage {
    fn from(msg: &Message) -> Self {
        Self {
            role: msg.role.to_string(),
            content: msg.content.to_string(),
            images: msg.images.iter().map(|img| SerializableImageAttachment {
                data: img.data.to_string(),
                media_type: img.media_type.to_string(),
            }).collect(),
        }
    }
}

impl From<SerializableMessage> for Message {
    fn from(msg: SerializableMessage) -> Self {
        Self {
            role: msg.role.into(),
            content: Arc::new(msg.content),
            images: msg.images.into_iter().map(|img| ImageAttachment {
                data: Arc::new(img.data),
                media_type: img.media_type.into(),
            }).collect(),
        }
    }
}

/// Serializable conversation state for cross-provider handoff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationState {
    pub messages: Vec<SerializableMessage>,
    pub system_prompt: Option<String>,
    pub provider_type: String,
    pub export_timestamp: i64,
}

impl Model {
    /// Export conversation state to JSON for cross-provider handoff
    pub fn export_conversation_state(
        &self,
        messages: &[Message],
        system_prompt: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let provider_type = match &self.config.model_type {
            ModelType::Anthropic => "anthropic".to_string(),
            ModelType::OpenAI { .. } => "openai".to_string(),
        };

        let serializable_messages: Vec<SerializableMessage> =
            messages.iter().map(|m| m.into()).collect();

        let state = ConversationState {
            messages: serializable_messages,
            system_prompt: system_prompt.map(|s| s.to_string()),
            provider_type,
            export_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64,
        };

        Ok(serde_json::to_string_pretty(&state)?)
    }

    /// Import conversation state from JSON for cross-provider handoff
    pub fn import_conversation_state(
        &self,
        json: &str,
    ) -> Result<(Vec<Message>, Option<String>), Box<dyn std::error::Error>> {
        let state: ConversationState = serde_json::from_str(json)?;

        let messages: Vec<Message> =
            state.messages.into_iter().map(|m| m.into()).collect();

        Ok((messages, state.system_prompt))
    }
}

// ============================================================
// Native Tool Calling Types
// ============================================================

/// Tool definition for Anthropic API
#[derive(Debug, Clone, Serialize)]
pub struct AnthropicToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Tool definition for OpenAI API
#[derive(Debug, Clone, Serialize)]
pub struct OpenAIToolDefinition {
    pub r#type: String,
    pub function: OpenAIFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAIFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ============================================================
// Anthropic API types
// ============================================================

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<AnthropicMessage>,
    pub system: Option<String>,
    pub thinking: AnthropicThinking,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicToolDefinition>>,
}

#[derive(Debug, Serialize)]
pub struct AnthropicThinking {
    #[serde(rename = "type")]
    pub thinking_type: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    pub content: Vec<ContentBlock>,
    pub usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[allow(dead_code)]
        id: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

// ============================================================
// OpenAI-Compatible API types (for llama-server)
// ============================================================

#[derive(Debug, Serialize, Default)]
pub struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAIToolDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIResponse {
    pub choices: Vec<Choice>,
    pub usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    message: OpenAIMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessageResponse {
    #[serde(default)]
    content: Option<String>,
    // Support for thinking/reasoning content from various APIs
    // DeepSeek: reasoning_content
    // Qwen/Ollama: thinking_content or thinking
    #[serde(default, alias = "reasoning_content")]
    reasoning_content: Option<String>,
    #[serde(default, alias = "thinking_content", alias = "thinking")]
    reasoning: Option<String>,
}

// ============================================================
// Model Implementation
// ============================================================

impl Model {
    pub fn new(model_type: ModelType) -> Self {
        // Extract base_url if available for interrupt requests
        let base_url = match &model_type {
            ModelType::OpenAI { base_url, .. } => Some(base_url.clone()),
            ModelType::Anthropic => None,
        };

        Self {
            client: Client::new(),
            config: ModelConfig { model_type },
            usage: Arc::new(Mutex::new(Usage {
                prompt_tokens: None,
                completion_tokens: None,
                cost_in_currency: None,
            })),
            base_url,
            tool_support_probed: Arc::new(Mutex::new(None)),
        }
    }

    /// Interrupt the current generation on the server (for llama.cpp and compatible servers)
    pub async fn interrupt_generation(&self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(base_url) = &self.base_url else {
            return Ok(()); // No base_url available, nothing to interrupt
        };

        // Try llama.cpp's /interrupt endpoint
        let interrupt_url = format!("{}/interrupt", base_url.trim_end_matches('/'));

        // Send interrupt request with a short timeout (fire and forget)
        let _ = self
            .client
            .post(&interrupt_url)
            .timeout(std::time::Duration::from_millis(500))
            .send()
            .await;

        Ok(())
    }

    pub fn get_usage(&self) -> Result<Usage, Box<dyn std::error::Error>> {
        let usage = self.usage.lock().unwrap();
        Ok(usage.clone())
    }

    pub fn create_messages(&self, items: Vec<(&str, &str)>) -> Vec<Message> {
        items
            .into_iter()
            .map(|(role, content)| Message {
                role: role.into(),
                content: Arc::new(content.to_string()),
                images: Vec::new(),
            })
            .collect()
    }

    pub fn add_user_message(&self, mut messages: Vec<Message>, content: &str) -> Vec<Message> {
        messages.push(Message {
            role: "user".into(),
            content: Arc::new(content.to_string()),
            images: Vec::new(),
        });
        messages
    }

    pub fn add_assistant_message(&self, mut messages: Vec<Message>, content: &str) -> Vec<Message> {
        messages.push(Message {
            role: "assistant".into(),
            content: Arc::new(content.to_string()),
            images: Vec::new(),
        });
        messages
    }

    /// Check if this model supports native tool calling.
    /// For OpenAI-compatible endpoints:
    /// 1. Checks manual override (if set in config)
    /// 2. Falls back to cached probe result
    /// 3. Defaults to false if not yet probed (prompt-based tool calling is safe fallback)
    pub fn supports_native_tools(&self) -> bool {
        match &self.config.model_type {
            ModelType::Anthropic => true,
            ModelType::OpenAI { native_tools_override, .. } => {
                // Use manual override if specified
                if let Some(override_val) = native_tools_override {
                    return *override_val;
                }
                // Otherwise check cached probe result; default to false if not yet probed
                self.tool_support_probed
                    .lock()
                    .unwrap()
                    .unwrap_or(false)
            }
        }
    }

    /// Probe the OpenAI-compatible server to check if it supports native tool calling.
    /// Sends a minimal request with a simple tool definition and checks whether
    /// the response contains structured `tool_calls`.
    /// Returns `true` if the server supports native tool calling, `false` otherwise.
    pub async fn probe_tool_support(&self) -> bool {
        let ModelType::OpenAI { base_url, model, api_key, .. } = &self.config.model_type else {
            return true; // Anthropic always supports tools
        };

        let url = format!("{}/v1/chat/completions", base_url);

        let probe_request = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "user", "content": "Use the ping tool now."}
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "ping",
                    "description": "Respond with pong",
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            }],
            "max_tokens": 64
        });

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .timeout(std::time::Duration::from_secs(15));

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = match req.json(&probe_request).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Tool probe: request failed ({}), assuming no native tool support", e);
                return false;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            eprintln!("Tool probe: server returned {} , assuming no native tool support", status);
            return false;
        }

        // Parse the response to check for tool_calls
        #[derive(Deserialize)]
        struct ProbeToolCall {
            #[serde(default)]
            function: Option<ProbeFunction>,
        }

        #[derive(Deserialize)]
        struct ProbeFunction {
            #[serde(default)]
            name: Option<String>,
        }

        #[derive(Deserialize)]
        struct ProbeMessage {
            #[serde(default)]
            tool_calls: Option<Vec<ProbeToolCall>>,
        }

        #[derive(Deserialize)]
        struct ProbeChoice {
            message: ProbeMessage,
        }

        #[derive(Deserialize)]
        struct ProbeResponse {
            choices: Vec<ProbeChoice>,
        }

        match response.json::<ProbeResponse>().await {
            Ok(probe_resp) => {
                if let Some(choice) = probe_resp.choices.first()
                    && let Some(tool_calls) = &choice.message.tool_calls {
                        let has_valid_tool_call = tool_calls.iter().any(|tc| {
                            tc.function
                                .as_ref()
                                .and_then(|f| f.name.as_ref())
                                .map(|n| !n.is_empty())
                                .unwrap_or(false)
                        });
                        if has_valid_tool_call {
                            eprintln!("Tool probe: server supports native tool calling ✓");
                            return true;
                        }
                    }
                eprintln!("Tool probe: no tool_calls in response, using prompt-based tool calling");
                false
            }
            Err(e) => {
                eprintln!("Tool probe: failed to parse response ({}), assuming no native tool support", e);
                false
            }
        }
    }

    /// Probe the server for tool support and cache the result.
    /// Should be called once at startup.
    /// Skips probing if native_tools_override is set in config.
    pub async fn probe_and_cache_tool_support(&self) {
        if matches!(&self.config.model_type, ModelType::Anthropic) {
            // Anthropic always supports tools, no need to probe
            *self.tool_support_probed.lock().unwrap() = Some(true);
            return;
        }

        // Check if manual override is set
        if let ModelType::OpenAI { native_tools_override: Some(_), .. } = &self.config.model_type {
            // Override is set, no need to probe - supports_native_tools() will use the override
            *self.tool_support_probed.lock().unwrap() = None;
            return;
        }

        let result = self.probe_tool_support().await;
        *self.tool_support_probed.lock().unwrap() = Some(result);
    }

    /// Get a reference to the model type (for capability detection)
    pub fn model_type(&self) -> &ModelType {
        &self.config.model_type
    }

    /// Get the model name (for capability detection)
    pub fn model_name(&self) -> String {
        match &self.config.model_type {
            ModelType::Anthropic => "claude".to_string(),
            ModelType::OpenAI { model, .. } => model.clone(),
        }
    }

    /// Create Anthropic tool definitions from a tool registry
    pub fn create_anthropic_tools(
        &self,
        tools: &[&dyn crate::tools::Tool],
    ) -> Vec<AnthropicToolDefinition> {
        tools
            .iter()
            .map(|t| AnthropicToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.parameters_schema(),
            })
            .collect()
    }

    /// Create OpenAI tool definitions from a tool registry
    pub fn create_openai_tools(
        &self,
        tools: &[&dyn crate::tools::Tool],
    ) -> Vec<OpenAIToolDefinition> {
        tools
            .iter()
            .map(|t| OpenAIToolDefinition {
                r#type: "function".to_string(),
                function: OpenAIFunctionDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters_schema(),
                },
            })
            .collect()
    }

    pub async fn query(&self, messages: &[Message]) -> Result<String, Box<dyn std::error::Error>> {
        let messages_with_reminder = self.append_tool_reminder(messages);

        match &self.config.model_type {
            ModelType::Anthropic => self.query_anthropic(&messages_with_reminder, None).await,
            ModelType::OpenAI { base_url, model, api_key, extra, .. } => {
                self.query_openai_compat(&messages_with_reminder, base_url, model, api_key.as_deref(), extra)
                    .await
            }
        }
    }

    /// Query with native tool calling support
    pub async fn query_with_tools(
        &self,
        messages: &[Message],
        tools: &[&dyn crate::tools::Tool],
    ) -> Result<String, Box<dyn std::error::Error>> {
        match &self.config.model_type {
            ModelType::Anthropic => {
                let tool_defs = self.create_anthropic_tools(tools);
                self.query_anthropic(messages, Some(&tool_defs)).await
            }
            ModelType::OpenAI { base_url, model, api_key, extra, .. } => {
                let tool_defs = self.create_openai_tools(tools);
                self.query_openai_compat_with_tools(messages, base_url, model, api_key.as_deref(), extra, Some(&tool_defs))
                    .await
            }
        }
    }

    pub async fn query_streaming<F, G>(
        &self,
        messages: &[Message],
        on_text: F,
        on_thinking: G,
        cancel_token: &CancellationToken,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let messages_with_reminder = self.append_tool_reminder(messages);

        match &self.config.model_type {
            ModelType::Anthropic => {
                self.query_anthropic_streaming(&messages_with_reminder, None, on_text, on_thinking, cancel_token)
                    .await
            }
            ModelType::OpenAI { base_url, model, api_key, extra, .. } => {
                self.query_openai_compat_streaming(
                    &messages_with_reminder,
                    OpenAICompatParams {
                        base_url,
                        model,
                        api_key: api_key.as_deref(),
                        extra,
                    },
                    on_text,
                    on_thinking,
                    cancel_token,
                )
                .await
            }
        }
    }

    /// Query with native tool calling support (streaming)
    pub async fn query_streaming_with_tools<F, G>(
        &self,
        messages: &[Message],
        tools: &[&dyn crate::tools::Tool],
        on_text: F,
        on_thinking: G,
        cancel_token: &CancellationToken,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        match &self.config.model_type {
            ModelType::Anthropic => {
                let tool_defs = self.create_anthropic_tools(tools);
                self.query_anthropic_streaming(messages, Some(&tool_defs), on_text, on_thinking, cancel_token)
                    .await
            }
            ModelType::OpenAI { base_url, model, api_key, extra, .. } => {
                let tool_defs = self.create_openai_tools(tools);
                self.query_openai_compat_streaming_with_tools(
                    messages,
                    OpenAICompatParamsWithTools {
                        base_url,
                        model,
                        api_key: api_key.as_deref(),
                        extra,
                        tools: Some(&tool_defs),
                    },
                    on_text,
                    on_thinking,
                    cancel_token,
                )
                .await
            }
        }
    }

    /// Appends a formatting reminder to user messages based on model type
    fn append_tool_reminder(&self, messages: &[Message]) -> Vec<Message> {
        let mut result = Vec::new();
        let mut reminder_added = false;

        // Get model-specific reminder
        let model_type_id = match &self.config.model_type {
            ModelType::Anthropic => "anthropic",
            ModelType::OpenAI { .. } => "openai-compatible",
        };
        let reminder = crate::prompt::get_tool_reminder(model_type_id);

        for msg in messages.iter() {
            if &*msg.role == "user" && !reminder_added {
                if let Some(ref rem) = reminder {
                    let new_content = format!("{}\n\n{}", msg.content, rem);
                    result.push(Message {
                        role: msg.role.clone(),
                        content: Arc::new(new_content),
                        images: Vec::new(),
                    });
                    reminder_added = true;
                } else {
                    result.push(msg.clone());
                }
            } else {
                result.push(msg.clone());
            }
        }
        result
    }

    // ============================================================
    // Anthropic API
    // ============================================================

    async fn query_anthropic(
        &self,
        messages: &[Message],
        tools: Option<&[AnthropicToolDefinition]>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY environment variable not set");

        // Extract system prompt and convert messages
        let mut system_prompt = None;
        let anthropic_messages: Vec<AnthropicMessage> = messages
            .iter()
            .filter_map(|msg| {
                if &*msg.role == "system" {
                    system_prompt = Some(msg.content.to_string());
                    None
                } else {
                    Some(AnthropicMessage {
                        role: msg.role.to_string(),
                        content: msg.content.to_string(),
                    })
                }
            })
            .collect();

        let request_body = AnthropicRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: anthropic_messages,
            system: system_prompt,
            thinking: AnthropicThinking {
                thinking_type: "enabled".to_string(),
            },
            tools: tools.map(|t| t.to_vec()),
        };

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("Anthropic API error: {} - {}", status, error_text).into());
        }

        let anthropic_response: AnthropicResponse = response.json().await?;

        if let Some(usage_data) = anthropic_response.usage {
            let mut usage = self.usage.lock().unwrap();
            usage.prompt_tokens = Some(usage_data.input_tokens);
            usage.completion_tokens = Some(usage_data.output_tokens);
            usage.cost_in_currency = Some(
                (usage_data.input_tokens as f64 * 0.000003)
                    + (usage_data.output_tokens as f64 * 0.000015),
            );
        }

        Ok(anthropic_response
            .content
            .iter()
            .map(|c| match c {
                ContentBlock::Text { text } => text.clone(),
                ContentBlock::Thinking { thinking, .. } => {
                    format!("<thinking>{}</thinking>", thinking)
                }
                ContentBlock::ToolUse { id: _, name, input } => {
                    // Convert native tool use to XML format for parser compatibility
                    let input_json = serde_json::to_string(input).unwrap_or_default();
                    format!("<codr_tool name=\"{}\">{}</codr_tool>", name, input_json)
                }
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    // ============================================================
    // OpenAI-compatible API (llama-server, etc.)
    // ============================================================

    async fn query_openai_compat(
        &self,
        messages: &[Message],
        base_url: &str,
        model: &str,
        api_key: Option<&str>,
        extra: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/v1/chat/completions", base_url);

        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.to_string(),
                content: msg.content.to_string(),
            })
            .collect();

        let request_body = OpenAIRequest {
            model: model.to_string(),
            messages: openai_messages,
            max_tokens: Some(4096),
            extra: if extra.is_empty() {
                None
            } else {
                Some(extra.clone())
            },
            tools: None,
            tool_choice: None,
        };

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.json(&request_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("API error: {} - {}", status, error_text).into());
        }

        let openai_response: OpenAIResponse = response.json().await?;

        if let Some(usage_data) = openai_response.usage {
            let mut usage = self.usage.lock().unwrap();
            usage.prompt_tokens = Some(usage_data.prompt_tokens);
            usage.completion_tokens = Some(usage_data.completion_tokens);
            usage.cost_in_currency = Some(usage_data.total_tokens as f64 * 0.000001);
        }

        // Combine reasoning + content if reasoning is present
        if let Some(choice) = openai_response.choices.first() {
            let message = &choice.message;
            let reasoning = message.reasoning.clone().or_else(|| message.reasoning_content.clone());
            let content = message.content.as_deref().unwrap_or("");
            if let Some(ref reasoning) = reasoning {
                // Wrap reasoning in <thinking> tags for consistent extraction
                Ok(format!(
                    "<thinking>{}</thinking>\n\n{}",
                    reasoning, content
                ))
            } else {
                Ok(content.to_string())
            }
        } else {
            Ok(String::new())
        }
    }

    async fn query_openai_compat_with_tools(
        &self,
        messages: &[Message],
        base_url: &str,
        model: &str,
        api_key: Option<&str>,
        extra: &std::collections::HashMap<String, serde_json::Value>,
        tools: Option<&[OpenAIToolDefinition]>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/v1/chat/completions", base_url);

        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.to_string(),
                content: msg.content.to_string(),
            })
            .collect();

        let request_body = OpenAIRequest {
            model: model.to_string(),
            messages: openai_messages,
            max_tokens: Some(4096),
            extra: if extra.is_empty() { None } else { Some(extra.clone()) },
            tools: tools.map(|t| t.to_vec()),
            tool_choice: None,
        };

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.json(&request_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("API error: {} - {}", status, error_text).into());
        }

        let openai_response: OpenAIResponse = response.json().await?;

        if let Some(usage_data) = openai_response.usage {
            let mut usage = self.usage.lock().unwrap();
            usage.prompt_tokens = Some(usage_data.prompt_tokens);
            usage.completion_tokens = Some(usage_data.completion_tokens);
            usage.cost_in_currency = Some(usage_data.total_tokens as f64 * 0.000001);
        }

        // Handle tool calls in response
        if let Some(choice) = openai_response.choices.first() {
            let message = &choice.message;
            let reasoning = message.reasoning.clone().or_else(|| message.reasoning_content.clone());
            let content = message.content.as_deref().unwrap_or("");

            // TODO: Parse tool_calls from response when available
            // For now, just return content + reasoning
            if let Some(ref reasoning) = reasoning {
                Ok(format!(
                    "<thinking>{}</thinking>\n\n{}",
                    reasoning, content
                ))
            } else {
                Ok(content.to_string())
            }
        } else {
            Ok(String::new())
        }
    }

    // ============================================================
    // Anthropic Streaming API
    // ============================================================

    async fn query_anthropic_streaming<F, G>(
        &self,
        messages: &[Message],
        tools: Option<&[AnthropicToolDefinition]>,
        mut on_text: F,
        mut on_thinking: G,
        cancel_token: &CancellationToken,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY environment variable not set");

        let mut system_prompt = None;
        let anthropic_messages: Vec<AnthropicMessage> = messages
            .iter()
            .filter_map(|msg| {
                if &*msg.role == "system" {
                    system_prompt = Some(msg.content.to_string());
                    None
                } else {
                    Some(AnthropicMessage {
                        role: msg.role.to_string(),
                        content: msg.content.to_string(),
                    })
                }
            })
            .collect();

        let request_body = AnthropicRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            messages: anthropic_messages,
            system: system_prompt,
            thinking: AnthropicThinking {
                thinking_type: "enabled".to_string(),
            },
            tools: tools.map(|t| t.to_vec()),
        };

        // Wrap the response in an Option so we can explicitly drop it on cancellation
        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(60))
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("Anthropic API error: {} - {}", status, error_text).into());
        }

        let mut stream = response.bytes_stream();

        let mut full_content = String::new();
        let mut thinking_content = String::new();
        let mut buffer = String::new();

        loop {
            // Check for cancellation before each chunk
            if cancel_token.is_cancelled() {
                return Err("Request cancelled by user".into());
            }

            let chunk = tokio::select! {
                chunk = stream.next() => chunk,
                _ = cancel_token.cancelled() => return Err("Request cancelled by user".into()),
            };

            let Some(bytes) = chunk else {
                break;
            };
            let bytes = bytes?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end().to_string();
                buffer.drain(..=pos);

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Ok(response) = serde_json::from_str::<AnthropicResponse>(data) {
                        for block in response.content {
                            match block {
                                ContentBlock::Text { text } => {
                                    full_content.push_str(&text);
                                    on_text(text);
                                }
                                ContentBlock::Thinking { thinking, .. } => {
                                    thinking_content.push_str(&thinking);
                                    on_thinking(thinking);
                                }
                                ContentBlock::ToolUse { id: _, name, input } => {
                                    // Convert native tool use to XML format for parser compatibility
                                    let input_json = serde_json::to_string(&input).unwrap_or_default();
                                    let tool_xml = format!("<codr_tool name=\"{}\">{}</codr_tool>", name, input_json);
                                    full_content.push_str(&tool_xml);
                                    on_text(tool_xml);
                                }
                            }
                        }
                        if let Some(usage_data) = response.usage {
                            let mut usage = self.usage.lock().unwrap();
                            usage.prompt_tokens = Some(usage_data.input_tokens);
                            usage.completion_tokens = Some(usage_data.output_tokens);
                            usage.cost_in_currency = Some(
                                (usage_data.input_tokens as f64 * 0.000003)
                                    + (usage_data.output_tokens as f64 * 0.000015),
                            );
                        }
                    }
                }
            }
        }

        if !thinking_content.is_empty() {
            Ok(format!(
                "<thinking>{}</thinking>\n\n{}",
                thinking_content, full_content
            ))
        } else {
            Ok(full_content)
        }
    }

    // ============================================================
    // OpenAI-compatible Streaming API (llama-server, NVIDIA NIM, etc.)
    // ============================================================

    async fn query_openai_compat_streaming<F, G>(
        &self,
        messages: &[Message],
        params: OpenAICompatParams<'_>,
        mut on_text: F,
        mut on_thinking: G,
        cancel_token: &CancellationToken,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let url = format!("{}/v1/chat/completions", params.base_url);

        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.to_string(),
                content: msg.content.to_string(),
            })
            .collect();

        #[derive(Serialize)]
        struct StreamingRequest {
            model: String,
            messages: Vec<OpenAIMessage>,
            max_tokens: Option<u32>,
            stream: bool,
            #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
            extra: std::collections::HashMap<String, serde_json::Value>,
        }

        let request_body = StreamingRequest {
            model: params.model.to_string(),
            messages: openai_messages,
            max_tokens: Some(4096),
            stream: true,
            extra: params.extra.clone(),
        };

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(key) = params.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("API error: {} - {}", status, error_text).into());
        }

        let mut stream = response.bytes_stream();

        let mut full_content = String::new();
        let mut thinking_content = String::new();
        let mut buffer = String::new();
        // Accumulate tool call chunks: index -> (name, arguments_so_far)
        let mut tool_call_accumulators: std::collections::HashMap<usize, (String, String, usize)> = std::collections::HashMap::new();

        loop {
            // Check for cancellation before each chunk
            if cancel_token.is_cancelled() {
                return Err("Request cancelled by user".into());
            }

            let chunk = tokio::select! {
                chunk = stream.next() => chunk,
                _ = cancel_token.cancelled() => return Err("Request cancelled by user".into()),
            };

            let Some(bytes) = chunk else {
                break;
            };
            let bytes = bytes?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end().to_string();
                buffer.drain(..=pos);

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    #[derive(Deserialize)]
                    #[allow(dead_code)]
                    struct StreamingChoice {
                        delta: StreamingDelta,
                        #[serde(default)]
                        finish_reason: Option<String>,
                    }

                    #[derive(Deserialize)]
                    struct StreamingDelta {
                        #[serde(default)]
                        content: Option<String>,
                        #[serde(default, alias = "reasoning_content")]
                        reasoning_content: Option<String>,
                        #[serde(default, alias = "thinking_content", alias = "thinking")]
                        reasoning: Option<String>,
                        #[serde(default)]
                        tool_calls: Option<Vec<StreamingToolCall>>,
                    }

                    #[derive(Deserialize)]
                    struct StreamingToolCall {
                        #[serde(default)]
                        index: Option<usize>,
                        #[serde(default)]
                        _id: Option<String>,
                        #[serde(default)]
                        _type: Option<String>,
                        function: Option<StreamingFunction>,
                    }

                    #[derive(Deserialize)]
                    struct StreamingFunction {
                        #[serde(default)]
                        name: Option<String>,
                        #[serde(default)]
                        arguments: Option<String>,
                    }

                    #[derive(Deserialize)]
                    struct StreamResponse {
                        choices: Vec<StreamingChoice>,
                        #[serde(default)]
                        usage: Option<StreamUsage>,
                    }

                    #[derive(Deserialize)]
                    struct StreamUsage {
                        #[serde(default)]
                        prompt_tokens: Option<u32>,
                        #[serde(default)]
                        completion_tokens: Option<u32>,
                        #[serde(default)]
                        total_tokens: Option<u32>,
                    }

                    if let Ok(response) = serde_json::from_str::<StreamResponse>(data) {
                        for choice in response.choices {
                            let reasoning = choice.delta.reasoning.clone().or_else(|| choice.delta.reasoning_content.clone());
                            if let Some(reasoning) = reasoning {
                                thinking_content.push_str(&reasoning);
                                on_thinking(reasoning);
                            }
                            if let Some(content) = choice.delta.content {
                                full_content.push_str(&content);
                                on_text(content);
                            }
                            // Handle tool calls: accumulate name and arguments across chunks
                            if let Some(tool_calls) = choice.delta.tool_calls {
                                for tool_call in tool_calls {
                                    let idx = tool_call.index.unwrap_or(0);
                                    if let Some(function) = tool_call.function {
                                        let entry = tool_call_accumulators.entry(idx)
                                            .or_insert_with(|| (String::new(), String::new(), 0));
                                        if let Some(name) = function.name
                                            && !name.is_empty() && entry.0.is_empty() {
                                                entry.0 = name.clone();
                                                // Stream tool name when first detected
                                                on_text(format!("\n⚙ Calling {}...", name));
                                            }
                                        if let Some(arguments) = function.arguments {
                                            entry.1.push_str(&arguments);
                                            let new_len = entry.1.len();
                                            // Stream progress every ~1KB of new argument data
                                            let last_reported = entry.2;
                                            if new_len >= last_reported + 1024 {
                                                let size_kb = new_len as f64 / 1024.0;
                                                on_text(format!("⚙ {}: generating ({:.1}KB)...", entry.0, size_kb));
                                                entry.2 = new_len;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(usage) = response.usage {
                            let total = usage.total_tokens.unwrap_or(0);
                            let mut usage_lock = self.usage.lock().unwrap();
                            usage_lock.prompt_tokens = usage.prompt_tokens;
                            usage_lock.completion_tokens = usage.completion_tokens;
                            usage_lock.cost_in_currency = Some(total as f64 * 0.000001);
                        }
                    } else if !data.is_empty() {
                        // Sometimes the event is just an empty string or generic message we don't care about,
                        // but if we fail to parse a non-empty payload, we just ignore it.
                    }
                }
            }
        }

        // Emit accumulated tool calls as complete XML
        let mut sorted_indices: Vec<usize> = tool_call_accumulators.keys().copied().collect();
        sorted_indices.sort();
        for idx in sorted_indices {
            if let Some((name, arguments, _line_count)) = tool_call_accumulators.get(&idx)
                && !name.is_empty() {
                    // Try to parse the accumulated arguments as JSON for validation
                    let args_str = if let Ok(args_json) = serde_json::from_str::<serde_json::Value>(arguments) {
                        serde_json::to_string(&args_json).unwrap_or_else(|_| arguments.clone())
                    } else {
                        // Use raw arguments even if not valid JSON - parser can handle it
                        arguments.clone()
                    };
                    let tool_xml = format!("<codr_tool name=\"{}\">{}</codr_tool>", name, args_str);
                    full_content.push_str(&tool_xml);
                    on_text(tool_xml);
                }
        }

        if !thinking_content.is_empty() {
            Ok(format!(
                "<thinking>{}</thinking>\n\n{}",
                thinking_content, full_content
            ))
        } else {
            Ok(full_content)
        }
    }

    async fn query_openai_compat_streaming_with_tools<F, G>(
        &self,
        messages: &[Message],
        params: OpenAICompatParamsWithTools<'_>,
        mut on_text: F,
        mut on_thinking: G,
        cancel_token: &CancellationToken,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let url = format!("{}/v1/chat/completions", params.base_url);

        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.to_string(),
                content: msg.content.to_string(),
            })
            .collect();

        #[derive(Serialize)]
        struct StreamingRequest {
            model: String,
            messages: Vec<OpenAIMessage>,
            max_tokens: Option<u32>,
            stream: bool,
            #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
            extra: std::collections::HashMap<String, serde_json::Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            tools: Option<Vec<OpenAIToolDefinition>>,
        }

        let request_body = StreamingRequest {
            model: params.model.to_string(),
            messages: openai_messages,
            max_tokens: Some(4096),
            stream: true,
            extra: params.extra.clone(),
            tools: params.tools.map(|t| t.to_vec()),
        };

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(key) = params.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req
            .json(&request_body)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("API error: {} - {}", status, error_text).into());
        }

        let mut stream = response.bytes_stream();

        let mut full_content = String::new();
        let mut thinking_content = String::new();
        let mut buffer = String::new();
        // Accumulate tool call chunks: index -> (name, arguments_so_far)
        let mut tool_call_accumulators: std::collections::HashMap<usize, (String, String, usize)> = std::collections::HashMap::new();

        loop {
            if cancel_token.is_cancelled() {
                return Err("Request cancelled by user".into());
            }

            let chunk = tokio::select! {
                chunk = stream.next() => chunk,
                _ = cancel_token.cancelled() => return Err("Request cancelled by user".into()),
            };

            let Some(bytes) = chunk else {
                break;
            };
            let bytes = bytes?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end().to_string();
                buffer.drain(..=pos);

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    #[derive(Deserialize)]
                    #[allow(dead_code)]
                    struct StreamingChoice {
                        delta: StreamingDelta,
                        #[serde(default)]
                        finish_reason: Option<String>,
                    }

                    #[derive(Deserialize)]
                    struct StreamingDelta {
                        #[serde(default)]
                        content: Option<String>,
                        #[serde(default, alias = "reasoning_content")]
                        reasoning_content: Option<String>,
                        #[serde(default, alias = "thinking_content", alias = "thinking")]
                        reasoning: Option<String>,
                        #[serde(default)]
                        tool_calls: Option<Vec<StreamingToolCall>>,
                    }

                    #[derive(Deserialize)]
                    struct StreamingToolCall {
                        #[serde(default)]
                        index: Option<usize>,
                        #[serde(default)]
                        _id: Option<String>,
                        #[serde(default)]
                        _type: Option<String>,
                        function: Option<StreamingFunction>,
                    }

                    #[derive(Deserialize)]
                    struct StreamingFunction {
                        #[serde(default)]
                        name: Option<String>,
                        #[serde(default)]
                        arguments: Option<String>,
                    }

                    #[derive(Deserialize)]
                    struct StreamResponse {
                        choices: Vec<StreamingChoice>,
                        #[serde(default)]
                        usage: Option<StreamUsage>,
                    }

                    #[derive(Deserialize)]
                    struct StreamUsage {
                        #[serde(default)]
                        prompt_tokens: Option<u32>,
                        #[serde(default)]
                        completion_tokens: Option<u32>,
                        #[serde(default)]
                        total_tokens: Option<u32>,
                    }

                    if let Ok(response) = serde_json::from_str::<StreamResponse>(data) {
                        for choice in response.choices {
                            let reasoning = choice.delta.reasoning.clone().or_else(|| choice.delta.reasoning_content.clone());
                            if let Some(reasoning) = reasoning {
                                thinking_content.push_str(&reasoning);
                                on_thinking(reasoning);
                            }
                            if let Some(content) = choice.delta.content {
                                full_content.push_str(&content);
                                on_text(content);
                            }
                            // Handle tool calls: accumulate name and arguments across chunks
                            if let Some(tool_calls) = choice.delta.tool_calls {
                                for tool_call in tool_calls {
                                    let idx = tool_call.index.unwrap_or(0);
                                    if let Some(function) = tool_call.function {
                                        let entry = tool_call_accumulators.entry(idx)
                                            .or_insert_with(|| (String::new(), String::new(), 0));
                                        if let Some(name) = function.name
                                            && !name.is_empty() && entry.0.is_empty() {
                                                entry.0 = name.clone();
                                                // Stream tool name when first detected
                                                on_text(format!("\n⚙ Calling {}...\n", name));
                                            }
                                        if let Some(arguments) = function.arguments {
                                            entry.1.push_str(&arguments);
                                            let new_len = entry.1.len();
                                            // Stream progress every ~1KB of new argument data
                                            let last_reported = entry.2;
                                            if new_len >= last_reported + 1024 {
                                                let size_kb = new_len as f64 / 1024.0;
                                                on_text(format!("⚙ {}: generating ({:.1}KB)...\n", entry.0, size_kb));
                                                entry.2 = new_len;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(usage) = response.usage {
                            let total = usage.total_tokens.unwrap_or(0);
                            let mut usage_lock = self.usage.lock().unwrap();
                            usage_lock.prompt_tokens = usage.prompt_tokens;
                            usage_lock.completion_tokens = usage.completion_tokens;
                            usage_lock.cost_in_currency = Some(total as f64 * 0.000001);
                        }
                    } else if !data.is_empty() {
                        // Sometimes the event is just an empty string or generic message we don't care about,
                        // but if we fail to parse a non-empty payload, we just ignore it.
                    }
                }
            }
        }

        // Emit accumulated tool calls as complete XML
        let mut sorted_indices: Vec<usize> = tool_call_accumulators.keys().copied().collect();
        sorted_indices.sort();
        for idx in sorted_indices {
            if let Some((name, arguments, _line_count)) = tool_call_accumulators.get(&idx)
                && !name.is_empty() {
                    let args_str = if let Ok(args_json) = serde_json::from_str::<serde_json::Value>(arguments) {
                        serde_json::to_string(&args_json).unwrap_or_else(|_| arguments.clone())
                    } else {
                        arguments.clone()
                    };
                    let tool_xml = format!("<codr_tool name=\"{}\">{}</codr_tool>", name, args_str);
                    full_content.push_str(&tool_xml);
                    on_text(tool_xml);
                }
        }

        if !thinking_content.is_empty() {
            Ok(format!(
                "<thinking>{}</thinking>\n\n{}",
                thinking_content, full_content
            ))
        } else {
            Ok(full_content)
        }
    }
}

// ============================================================
// Cache Key Trait (for model_probe capability cache)
// ============================================================

/// Trait for generating cache keys for models
pub trait CacheKey {
    fn get_cache_key(&self) -> String;
}

impl CacheKey for Model {
    fn get_cache_key(&self) -> String {
        match &self.config.model_type {
            ModelType::Anthropic => "anthropic:claude".to_string(),
            ModelType::OpenAI { base_url, model, .. } => {
                format!("openai:{}:{}", base_url, model)
            }
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Usage Tests
    // ============================================================

    #[test]
    fn test_usage_creation() {
        let usage = Usage {
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            cost_in_currency: Some(0.001),
        };
        
        assert_eq!(usage.prompt_tokens, Some(100));
        assert_eq!(usage.completion_tokens, Some(50));
        assert_eq!(usage.cost_in_currency, Some(0.001));
    }

    #[test]
    fn test_usage_clone() {
        let usage = Usage {
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            cost_in_currency: Some(0.001),
        };
        
        let cloned = usage.clone();
        
        assert_eq!(cloned.prompt_tokens, Some(100));
        assert_eq!(cloned.completion_tokens, Some(50));
        assert_eq!(cloned.cost_in_currency, Some(0.001));
    }

    // ============================================================
    // ModelType Tests
    // ============================================================

    #[test]
    fn test_model_type_anthropic() {
        let mt = ModelType::Anthropic;
        
        match mt {
            ModelType::Anthropic => {},
            _ => panic!("Expected Anthropic"),
        }
    }

    #[test]
    fn test_model_type_openai() {
        let mt = ModelType::OpenAI {
            base_url: "http://localhost:8080".to_string(),
            model: "test-model".to_string(),
            api_key: Some("test-key".to_string()),
            native_tools_override: None,
            extra: std::collections::HashMap::new(),
        };

        match mt {
            ModelType::OpenAI { base_url, model, api_key, extra, .. } => {
                assert_eq!(base_url, "http://localhost:8080");
                assert_eq!(model, "test-model");
                assert_eq!(api_key, Some("test-key".to_string()));
            }
            _ => panic!("Expected OpenAI"),
        }
    }

    #[test]
    fn test_model_type_clone() {
        let mt1 = ModelType::OpenAI {
            base_url: "http://localhost:8080".to_string(),
            model: "test-model".to_string(),
            api_key: Some("test-key".to_string()),
            extra: std::collections::HashMap::new(),
        };

        let mt2 = mt1.clone();

        match mt2 {
            ModelType::OpenAI { base_url, model, api_key, extra, .. } => {
                assert_eq!(base_url, "http://localhost:8080");
                assert_eq!(model, "test-model");
                assert_eq!(api_key, Some("test-key".to_string()));
            }
            _ => panic!("Expected OpenAI"),
        }
    }

    // ============================================================
    // Message Tests
    // ============================================================

    #[test]
    fn test_message_creation() {
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello".to_string()),
            images: Vec::new(),
        };

        assert_eq!(&*msg.role, "user");
        assert_eq!(&*msg.content, "Hello");
    }

    #[test]
    fn test_message_clone() {
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello".to_string()),
            images: Vec::new(),
        };

        let cloned = msg.clone();

        assert_eq!(&*cloned.role, "user");
        assert_eq!(&*cloned.content, "Hello");

        // Verify they share the same Arc (cheap clone)
        assert!(Arc::ptr_eq(&msg.content, &cloned.content));

        // Verify they are independent messages
        let msg2 = Message {
            role: "assistant".into(),
            content: Arc::new("Hi there".to_string()),
            images: Vec::new(),
        };

        assert_eq!(&*msg.role, "user");
        assert_eq!(&*msg2.role, "assistant");
    }

    #[test]
    fn test_message_debug() {
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello".to_string()),
            images: Vec::new(),
        };

        let debug_str = format!("{:?}", msg);
        assert!(debug_str.contains("user"));
        assert!(debug_str.contains("Hello"));
    }

    // ============================================================
    // AnthropicMessage Tests
    // ============================================================

    #[test]
    fn test_anthropic_message_creation() {
        let msg = AnthropicMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_anthropic_message_clone() {
        let msg = AnthropicMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        
        let cloned = msg.clone();
        assert_eq!(cloned.role, "user");
        assert_eq!(cloned.content, "Hello");
    }

    // ============================================================
    // AnthropicThinking Tests
    // ============================================================

    #[test]
    fn test_anthropic_thinking_default() {
        let thinking = AnthropicThinking {
            thinking_type: "enabled".to_string(),
        };
        
        assert_eq!(thinking.thinking_type, "enabled");
    }

    // ============================================================
    // AnthropicConfig / Request Tests
    // ============================================================

    #[test]
    fn test_anthropic_request_serialization() {
        let request = AnthropicRequest {
            model: "claude-3".to_string(),
            max_tokens: 1024,
            messages: vec![
                AnthropicMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                }
            ],
            system: Some("You are helpful".to_string()),
            thinking: AnthropicThinking {
                thinking_type: "enabled".to_string(),
            },
            tools: None,
        };
        
        let json = serde_json::to_string(&request).unwrap();
        
        assert!(json.contains("claude-3"));
        assert!(json.contains("user"));
        assert!(json.contains("Hello"));
        assert!(json.contains("You are helpful"));
        assert!(json.contains("enabled"));
    }

    // ============================================================
    // ContentBlock Tests
    // ============================================================

    #[test]
    fn test_content_block_text_deserialization() {
        let json = r#"{"type": "text", "text": "Hello world"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        
        match block {
            ContentBlock::Text { text } => {
                assert_eq!(text, "Hello world");
            }
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_thinking_deserialization() {
        let json = r#"{"type": "thinking", "thinking": "Let me think...", "id": "abc123"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        
        match block {
            ContentBlock::Thinking { thinking, id } => {
                assert_eq!(thinking, "Let me think...");
                assert_eq!(id, "abc123");
            }
            _ => panic!("Expected Thinking block"),
        }
    }

    // ============================================================
    // AnthropicResponse Tests
    // ============================================================

    #[test]
    fn test_anthropic_response_deserialization() {
        let json = r#"
        {
            "content": [
                {"type": "text", "text": "Hello"}
            ],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50
            }
        }
        "#;
        
        let response: AnthropicResponse = serde_json::from_str(json).unwrap();
        
        assert_eq!(response.content.len(), 1);
        assert!(response.usage.is_some());
        
        let usage = response.usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    // ============================================================
    // AnthropicUsage Tests
    // ============================================================

    #[test]
    fn test_anthropic_usage_deserialization() {
        let json = r#"{"input_tokens": 100, "output_tokens": 50}"#;
        let usage: AnthropicUsage = serde_json::from_str(json).unwrap();
        
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
    }

    // ============================================================
    // OpenAI Request/Response Tests
    // ============================================================

    #[test]
    fn test_openai_request_serialization() {
        let request = OpenAIRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                OpenAIMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                }
            ],
            max_tokens: Some(2048),
            extra: None,
            tools: None,
            tool_choice: None,
        };
        
        let json = serde_json::to_string(&request).unwrap();
        
        assert!(json.contains("gpt-4"));
        assert!(json.contains("user"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_openai_response_deserialization() {
        let json = r#"
        {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello!"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        }
        "#;
        
        let response: OpenAIResponse = serde_json::from_str(json).unwrap();
        
        assert_eq!(response.choices.len(), 1);
        assert!(response.usage.is_some());
    }

    // ============================================================
    // OpenAI Stream Response Tests
    // ============================================================

    #[test]
    fn test_openai_stream_response_deserialization() {
        let json = r#"
        {
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [
                {
                    "index": 0,
                    "delta": {
                        "content": "Hello"
                    },
                    "finish_reason": null
                }
            ]
        }
        "#;
        
        #[derive(Deserialize)]
        struct StreamResponse {
            choices: Vec<StreamChoice>,
        }
        
        #[derive(Deserialize)]
        struct StreamChoice {
            delta: Delta,
        }
        
        #[derive(Deserialize)]
        struct Delta {
            content: Option<String>,
        }
        
        let response: StreamResponse = serde_json::from_str(json).unwrap();
        
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].delta.content, Some("Hello".to_string()));
    }

    // ============================================================
    // OpenAI Message Tests
    // ============================================================

    #[test]
    fn test_openai_message_creation() {
        let msg = OpenAIMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    // ============================================================
    // ModelConfig Tests
    // ============================================================

    #[test]
    fn test_model_config_clone() {
        let config = ModelConfig {
            model_type: ModelType::Anthropic,
        };
        
        let cloned = config.clone();
        
        match cloned.model_type {
            ModelType::Anthropic => {},
            _ => panic!("Expected Anthropic"),
        }
    }

    // ============================================================
    // API URL Constants
    // ============================================================

    #[test]
    fn test_anthropic_api_url() {
        assert_eq!(ANTHROPIC_API_URL, "https://api.anthropic.com/v1/messages");
    }

    // ============================================================
    // Tool Support Probe Tests
    // ============================================================

    #[test]
    fn test_anthropic_supports_native_tools() {
        let model = Model::new(ModelType::Anthropic);
        // Anthropic always supports native tools (no probe needed)
        assert!(model.supports_native_tools());
    }

    #[test]
    fn test_openai_defaults_to_no_native_tools_when_unprobed() {
        let model = Model::new(ModelType::OpenAI {
            base_url: "http://localhost:8080".to_string(),
            model: "test-model".to_string(),
            api_key: None,
            extra: std::collections::HashMap::new(),
        });
        // Before probing, OpenAI should default to false (safe fallback)
        assert!(!model.supports_native_tools());
    }

    #[test]
    fn test_openai_respects_cached_probe_true() {
        let model = Model::new(ModelType::OpenAI {
            base_url: "http://localhost:8080".to_string(),
            model: "test-model".to_string(),
            api_key: None,
            extra: std::collections::HashMap::new(),
        });
        // Simulate a successful probe
        *model.tool_support_probed.lock().unwrap() = Some(true);
        assert!(model.supports_native_tools());
    }

    #[test]
    fn test_openai_respects_cached_probe_false() {
        let model = Model::new(ModelType::OpenAI {
            base_url: "http://localhost:8080".to_string(),
            model: "test-model".to_string(),
            api_key: None,
            extra: std::collections::HashMap::new(),
        });
        // Simulate a failed probe
        *model.tool_support_probed.lock().unwrap() = Some(false);
        assert!(!model.supports_native_tools());
    }

    #[test]
    fn test_anthropic_probe_caches_true() {
        let model = Model::new(ModelType::Anthropic);
        // Manually call the synchronous cache-setting path
        // (probe_and_cache_tool_support is async but for Anthropic it just sets true)
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(model.probe_and_cache_tool_support());
        assert_eq!(*model.tool_support_probed.lock().unwrap(), Some(true));
    }
}
