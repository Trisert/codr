use crate::model::ModelType;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ============================================================
// Configuration file structure
// ============================================================

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ModelTypeConfig {
    Llama,
    Anthropic,
}

impl Default for ModelTypeConfig {
    fn default() -> Self {
        ModelTypeConfig::Llama
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
struct LlamaConfig {
    server_url: String,
    model: String,
}

impl Default for LlamaConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:8080".to_string(),
            model: "default".to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
struct AnthropicConfig {
    api_key: Option<String>,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self { api_key: None }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    model: ModelTypeConfig,
    llama: LlamaConfig,
    anthropic: AnthropicConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: ModelTypeConfig::default(),
            llama: LlamaConfig::default(),
            anthropic: AnthropicConfig::default(),
        }
    }
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
                Ok(content) => {
                    match toml::from_str(&content) {
                        Ok(config) => return config,
                        Err(e) => {
                            eprintln!("Warning: Failed to parse config file {:?}: {}", path, e);
                            eprintln!("Using default configuration.");
                        }
                    }
                }
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
            ModelTypeConfig::Llama => ModelType::LlamaServer {
                base_url: self.llama.server_url.clone(),
                model: self.llama.model.clone(),
            },
            ModelTypeConfig::Anthropic => {
                // Check for API key in config first, then environment
                let api_key = self.anthropic.api_key
                    .clone()
                    .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());

                if api_key.is_none() {
                    eprintln!("Warning: ANTHROPIC_API_KEY not set in config or environment");
                }

                ModelType::Anthropic
            }
        }
    }
}
