use reqwest::Client;
use serde::{Deserialize, Serialize};

// ============================================================
// Model Configuration
// ============================================================

pub enum ModelType {
    Anthropic,
    LlamaServer { base_url: String, model: String },
}

pub struct Model {
    client: Client,
    config: ModelConfig,
}

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
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: String,
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
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: OpenAIMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessageResponse {
    content: String,
}

// ============================================================
// Model Implementation
// ============================================================

impl Model {
    pub fn new(model_type: ModelType) -> Self {
        Self {
            client: Client::new(),
            config: ModelConfig { model_type },
        }
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
        match &self.config.model_type {
            ModelType::Anthropic => self.query_anthropic(messages).await,
            ModelType::LlamaServer { base_url, model } => {
                self.query_llama_server(messages, base_url, model).await
            }
        }
    }

    // ============================================================
    // Anthropic API
    // ============================================================

    async fn query_anthropic(&self, messages: &[Message]) -> Result<String, Box<dyn std::error::Error>> {
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
        Ok(anthropic_response
            .content
            .get(0)
            .map(|c| c.text.clone())
            .unwrap_or_default())
    }

    // ============================================================
    // llama-server (OpenAI-compatible API)
    // ============================================================

    async fn query_llama_server(
        &self,
        messages: &[Message],
        base_url: &str,
        model: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/v1/chat/completions", base_url);

        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .filter_map(|msg| {
                // llama-server (OpenAI-compatible) doesn't have a separate system field
                // We include system messages as role="system"
                Some(OpenAIMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                })
            })
            .collect();

        let request_body = OpenAIRequest {
            model: model.to_string(),
            messages: openai_messages,
            max_tokens: Some(4096),
        };

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(format!("llama-server API error: {} - {}", status, error_text).into());
        }

        let openai_response: OpenAIResponse = response.json().await?;
        Ok(openai_response
            .choices
            .get(0)
            .map(|c| c.message.content.clone())
            .unwrap_or_default())
    }
}
