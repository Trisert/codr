use crate::model::ModelType;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ============================================================
// Configuration file structure
// ============================================================

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
enum ModelTypeConfig {
    #[default]
    OpenAI,
    Anthropic,
    Nim,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
struct OpenAIConfig {
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            model: "default".to_string(),
            api_key: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
#[derive(Default)]
struct AnthropicConfig {
    api_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
struct NimConfig {
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl Default for NimConfig {
    fn default() -> Self {
        Self {
            base_url: "https://integrate.api.nvidia.com".to_string(),
            model: "meta/llama-3.1-70b-instruct".to_string(),
            api_key: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    model: ModelTypeConfig,
    openai: OpenAIConfig,
    anthropic: AnthropicConfig,
    nim: NimConfig,
}

impl Config {
    /// Get the config file path
    fn config_path() -> Option<PathBuf> {
        // Check for config in current directory first
        if let Ok(cwd) = std::env::current_dir() {
            let local_config = cwd.join("codr.toml");
            if local_config.exists() {
                return Some(local_config);
            }
        }

        // Fall back to XDG config home
        if let Some(config_home) = dirs::config_dir() {
            let config_dir = config_home.join("codr");
            let config_file = config_dir.join("config.toml");
            if config_file.exists() {
                return Some(config_file);
            }
        }

        None
    }

    /// Load config from file, or return default if not found
    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            match fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => return config,
                    Err(e) => {
                        eprintln!("Warning: Failed to parse config file {:?}: {}", path, e);
                        eprintln!("Using default configuration.");
                    }
                },
                Err(e) => {
                    eprintln!("Warning: Failed to read config file {:?}: {}", path, e);
                    eprintln!("Using default configuration.");
                }
            }
        }

        Self::default()
    }

    /// Convert to ModelType
    pub fn to_model_type(&self) -> ModelType {
        match &self.model {
            ModelTypeConfig::OpenAI => {
                ModelType::OpenAI {
                    base_url: self.openai.base_url.clone(),
                    model: self.openai.model.clone(),
                    api_key: self.openai.api_key.clone(),
                }
            }
            ModelTypeConfig::Anthropic => {
                let api_key = self
                    .anthropic
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());

                if api_key.is_none() {
                    eprintln!("Warning: ANTHROPIC_API_KEY not set in config or environment");
                }

                ModelType::Anthropic
            }
            ModelTypeConfig::Nim => {
                let api_key = self
                    .nim
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("NVIDIA_API_KEY").ok())
                    .unwrap_or_default();

                if api_key.is_empty() {
                    eprintln!("Warning: NVIDIA_API_KEY not set in config or environment");
                }

                ModelType::Nim {
                    base_url: self.nim.base_url.clone(),
                    model: self.nim.model.clone(),
                    api_key,
                }
            }
        }
    }
}
