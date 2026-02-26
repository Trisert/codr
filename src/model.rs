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
    LlamaServer { base_url: String, model: String },
    Nim { base_url: String, model: String, api_key: String },
}

pub struct Model {
    client: Client,
    config: ModelConfig,
    usage: Arc<Mutex<Usage>>,
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
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
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
        match &self.config.model_type {
            ModelType::Anthropic => self.query_anthropic(messages).await,
            ModelType::LlamaServer { base_url, model } => {
                self.query_openai_compat(messages, base_url, model, None).await
            }
            ModelType::Nim { base_url, model, api_key } => {
                self.query_openai_compat(messages, base_url, model, Some(api_key)).await
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
        
        if let Some(usage_data) = anthropic_response.usage {
            let mut usage = self.usage.lock().unwrap();
            usage.prompt_tokens = Some(usage_data.input_tokens);
            usage.completion_tokens = Some(usage_data.output_tokens);
            usage.cost_in_currency = Some(
                (usage_data.input_tokens as f64 * 0.000003) + 
                (usage_data.output_tokens as f64 * 0.000015)
            );
        }
        
        Ok(anthropic_response
            .content
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default())
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

        Ok(openai_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default())
    }
}
