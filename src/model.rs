use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize, Serializer};
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
    },
}

#[derive(Clone)]
pub struct Model {
    client: Client,
    config: ModelConfig,
    usage: Arc<Mutex<Usage>>,
}

#[derive(Clone)]
struct ModelConfig {
    model_type: ModelType,
}

// ============================================================
// Message types - Generic representation
// ============================================================

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Arc<str>,  // Shared, immutable
    pub content: Arc<String>,  // Shared, potentially large
}

// Custom Serialize for Message that converts Arc to owned strings
impl Serialize for Message {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Message", 2)?;
        state.serialize_field("role", &self.role.to_string())?;
        state.serialize_field("content", &self.content.as_str())?;
        state.end()
    }
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
}

// ============================================================
// OpenAI-Compatible API types (for llama-server)
// ============================================================

#[derive(Debug, Serialize)]
pub struct OpenAIRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    pub max_tokens: Option<u32>,
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
    content: String,
    // Support for thinking/reasoning content from various APIs
    // DeepSeek: reasoning_content
    // Qwen/Ollama: thinking_content or thinking
    #[serde(
        alias = "reasoning_content",
        alias = "thinking_content",
        alias = "thinking"
    )]
    reasoning: Option<String>,
}

// ============================================================
// Model Implementation
// ============================================================

impl Model {
    pub fn new(model_type: ModelType) -> Self {
        Self {
            client: Client::new(),
            config: ModelConfig { model_type },
            usage: Arc::new(Mutex::new(Usage {
                prompt_tokens: None,
                completion_tokens: None,
                cost_in_currency: None,
            })),
        }
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
            })
            .collect()
    }

    pub fn add_user_message(&self, mut messages: Vec<Message>, content: &str) -> Vec<Message> {
        messages.push(Message {
            role: "user".into(),
            content: Arc::new(content.to_string()),
        });
        messages
    }

    pub fn add_assistant_message(&self, mut messages: Vec<Message>, content: &str) -> Vec<Message> {
        messages.push(Message {
            role: "assistant".into(),
            content: Arc::new(content.to_string()),
        });
        messages
    }

    pub async fn query(&self, messages: &[Message]) -> Result<String, Box<dyn std::error::Error>> {
        let messages_with_reminder = Self::append_tool_reminder(messages);
        
        match &self.config.model_type {
            ModelType::Anthropic => self.query_anthropic(&messages_with_reminder).await,
            ModelType::OpenAI { base_url, model, api_key } => {
                self.query_openai_compat(&messages_with_reminder, base_url, model, api_key.as_deref())
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
        let messages_with_reminder = Self::append_tool_reminder(messages);

        match &self.config.model_type {
            ModelType::Anthropic => {
                self.query_anthropic_streaming(&messages_with_reminder, on_text, on_thinking, cancel_token)
                    .await
            }
            ModelType::OpenAI { base_url, model, api_key } => {
                self.query_openai_compat_streaming(
                    &messages_with_reminder,
                    base_url,
                    model,
                    api_key.as_deref(),
                    on_text,
                    on_thinking,
                    cancel_token,
                )
                .await
            }
        }
    }

    /// Appends a strict formatting reminder to the final user message to ensure generic models comply.
    fn append_tool_reminder(messages: &[Message]) -> Vec<Message> {
        let mut result = Vec::new();
        let mut reminder_added = false;

        for msg in messages.iter() {
            if &*msg.role == "user" && !reminder_added {
                let new_content = format!("{}{}", msg.content, "\n\n\
                    IMPORTANT: Your response must use XML format for tool calls:\n\
                    Use <codr_tool name=\"tool_name\">{\"param\": \"value\"}</codr_tool> for tools\n\
                    Use <codr_bash>command</codr_bash> for bash commands\n\
                    Examples:\n\
                    <codr_tool name=\"read\">{\"file_path\": \"src/main.rs\"}</codr_tool>\n\
                    <codr_tool name=\"find\">{\"pattern\": \"*.rs\"}</codr_tool>\n\
                    <codr_bash>ls -la</codr_bash>");
                result.push(Message {
                    role: msg.role.clone(),
                    content: Arc::new(new_content),
                });
                reminder_added = true;
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
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    // ============================================================
    // OpenAI-compatible API (llama-server, NVIDIA NIM, etc.)
    // ============================================================

    async fn query_openai_compat(
        &self,
        messages: &[Message],
        base_url: &str,
        model: &str,
        api_key: Option<&str>,
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
            if let Some(ref reasoning) = message.reasoning {
                // Wrap reasoning in <thinking> tags for consistent extraction
                Ok(format!(
                    "<thinking>{}</thinking>\n\n{}",
                    reasoning, message.content
                ))
            } else {
                Ok(message.content.clone())
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
        };

        let mut stream = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&request_body)
            .send()
            .await?
            .bytes_stream();

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
        base_url: &str,
        model: &str,
        api_key: Option<&str>,
        mut on_text: F,
        mut on_thinking: G,
        cancel_token: &CancellationToken,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let url = format!("{}/v1/chat/completions", base_url);

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
        }

        let request_body = StreamingRequest {
            model: model.to_string(),
            messages: openai_messages,
            max_tokens: Some(4096),
            stream: true,
        };

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let mut stream = req.json(&request_body).send().await?.bytes_stream();

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
                        #[serde(
                            default,
                            alias = "reasoning_content",
                            alias = "thinking_content",
                            alias = "thinking"
                        )]
                        reasoning: Option<String>,
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
                            if let Some(reasoning) = choice.delta.reasoning {
                                thinking_content.push_str(&reasoning);
                                on_thinking(reasoning);
                            }
                            if let Some(content) = choice.delta.content {
                                full_content.push_str(&content);
                                on_text(content);
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
        };
        
        match mt {
            ModelType::OpenAI { base_url, model, api_key } => {
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
        };
        
        let mt2 = mt1.clone();
        
        match mt2 {
            ModelType::OpenAI { base_url, model, api_key } => {
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
        };

        assert_eq!(&*msg.role, "user");
        assert_eq!(&*msg.content, "Hello");
    }

    #[test]
    fn test_message_clone() {
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello".to_string()),
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
        };

        assert_eq!(&*msg.role, "user");
        assert_eq!(&*msg2.role, "assistant");
    }

    #[test]
    fn test_message_debug() {
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello".to_string()),
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
}
