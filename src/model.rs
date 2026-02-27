use futures::stream::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

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
    Nim {
        base_url: String,
        model: String,
        api_key: String,
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
    pub role: String,
    pub content: String,
}

// ============================================================
// Anthropic API types
// ============================================================

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    system: Option<String>,
    thinking: AnthropicThinking,
}

#[derive(Debug, Serialize)]
struct AnthropicThinking {
    #[serde(rename = "type")]
    thinking_type: String,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
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
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<Choice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct Choice {
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
                role: role.to_string(),
                content: content.to_string(),
            })
            .collect()
    }

    pub fn add_user_message(&self, mut messages: Vec<Message>, content: &str) -> Vec<Message> {
        messages.push(Message {
            role: "user".to_string(),
            content: content.to_string(),
        });
        messages
    }

    pub fn add_assistant_message(&self, mut messages: Vec<Message>, content: &str) -> Vec<Message> {
        messages.push(Message {
            role: "assistant".to_string(),
            content: content.to_string(),
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
            ModelType::Nim {
                base_url,
                model,
                api_key,
            } => {
                self.query_openai_compat(&messages_with_reminder, base_url, model, Some(api_key))
                    .await
            }
        }
    }

    pub async fn query_streaming<F, G>(
        &self,
        messages: &[Message],
        on_text: F,
        on_thinking: G,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let messages_with_reminder = Self::append_tool_reminder(messages);

        match &self.config.model_type {
            ModelType::Anthropic => {
                self.query_anthropic_streaming(&messages_with_reminder, on_text, on_thinking)
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
                )
                .await
            }
            ModelType::Nim {
                base_url,
                model,
                api_key,
            } => {
                self.query_openai_compat_streaming(
                    &messages_with_reminder,
                    base_url,
                    model,
                    Some(api_key),
                    on_text,
                    on_thinking,
                )
                .await
            }
        }
    }

    /// Appends a strict formatting reminder to the final user message to ensure generic models comply.
    fn append_tool_reminder(messages: &[Message]) -> Vec<Message> {
        let mut messages = messages.to_vec();
        for msg in messages.iter_mut().rev() {
            if msg.role == "user" {
                msg.content.push_str(
                    "\n\n\
                    IMPORTANT: Your response must be a tool call block:\n\
                    ```tool-action\n<tool_name>\n<json_params>\n```\n\
                    Examples:\n\
                    ```tool-action\nread\n{\"file_path\": \"src/main.rs\"}\n```\n\
                    ```tool-action\nfind\n{\"pattern\": \"*.rs\"}\n```"
                );
                break;
            }
        }
        messages
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
                if msg.role == "system" {
                    system_prompt = Some(msg.content.clone());
                    None
                } else {
                    Some(AnthropicMessage {
                        role: msg.role.clone(),
                        content: msg.content.clone(),
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
                role: msg.role.clone(),
                content: msg.content.clone(),
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
                if msg.role == "system" {
                    system_prompt = Some(msg.content.clone());
                    None
                } else {
                    Some(AnthropicMessage {
                        role: msg.role.clone(),
                        content: msg.content.clone(),
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

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
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
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(String) + Send,
        G: FnMut(String) + Send,
    {
        let url = format!("{}/v1/chat/completions", base_url);

        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
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

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
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
