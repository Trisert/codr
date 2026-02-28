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
    #[serde(rename = "openai")]
    OpenAI,
    Anthropic,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(default)]
struct OpenAIConfig {
    base_url: String,
    model: String,
    api_key: Option<String>,
    #[serde(default)]
    extra: std::collections::HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
#[derive(Default)]
struct AnthropicConfig {
    api_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
#[derive(Default)]
pub struct Config {
    model: ModelTypeConfig,
    openai: OpenAIConfig,
    anthropic: AnthropicConfig,
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
        fn toml_to_json(v: &toml::Value) -> serde_json::Value {
            match v {
                toml::Value::String(s) => serde_json::Value::String(s.clone()),
                toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
                toml::Value::Float(f) => serde_json::Number::from_f64(*f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
                toml::Value::Array(arr) => {
                    serde_json::Value::Array(arr.iter().map(toml_to_json).collect())
                }
                toml::Value::Table(t) => serde_json::Value::Object(
                    t.iter()
                        .map(|(k, v)| (k.clone(), toml_to_json(v)))
                        .collect(),
                ),
                toml::Value::Datetime(_) => serde_json::Value::Null,
            }
        }

        let extra: std::collections::HashMap<String, serde_json::Value> = self
            .openai
            .extra
            .iter()
            .map(|(k, v)| (k.clone(), toml_to_json(v)))
            .collect();

        match &self.model {
            ModelTypeConfig::OpenAI => ModelType::OpenAI {
                base_url: self.openai.base_url.clone(),
                model: self.openai.model.clone(),
                api_key: self.openai.api_key.clone(),
                extra,
            },
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
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ============================================================
    // Default Config Tests
    // ============================================================

    #[test]
    fn test_default_config() {
        let config = Config::default();

        // Check default model type
        assert!(matches!(config.model, ModelTypeConfig::OpenAI));

        // Check default OpenAI config (derived Default gives empty strings)
        assert_eq!(config.openai.base_url, "");
        assert_eq!(config.openai.model, "");
        assert!(config.openai.api_key.is_none());

        // Check default Anthropic config
        assert!(config.anthropic.api_key.is_none());
    }

    #[test]
    fn test_default_config_to_model_type() {
        let config = Config::default();
        let model_type = config.to_model_type();

        match model_type {
            ModelType::OpenAI {
                base_url,
                model,
                api_key,
                extra: _,
            } => {
                // derived Default gives empty strings
                assert_eq!(base_url, "");
                assert_eq!(model, "");
                assert!(api_key.is_none());
            }
            _ => panic!("Expected OpenAI model type"),
        }
    }

    // ============================================================
    // Config File Loading Tests
    // ============================================================

    #[test]
    fn test_load_from_file_openai() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("codr.toml");

        let config_content = r#"
model = "openai"

[openai]
base_url = "http://custom:8080"
model = "custom-model"
api_key = "test-key"
"#;

        std::fs::write(&config_path, config_content).unwrap();

        // Change to temp dir so config is found
        let original_dir = std::env::current_dir().unwrap();
        let _ = std::env::set_current_dir(temp_dir.path());

        let config = Config::load();

        let _ = std::env::set_current_dir(&original_dir);
        drop(temp_dir);

        assert!(matches!(config.model, ModelTypeConfig::OpenAI));
        assert_eq!(config.openai.base_url, "http://custom:8080");
        assert_eq!(config.openai.model, "custom-model");
        assert_eq!(config.openai.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_load_from_file_anthropic() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("codr.toml");

        let config_content = r#"
model = "anthropic"

[anthropic]
api_key = "sk-ant-test-key"
"#;

        std::fs::write(&config_path, config_content).unwrap();

        // Change to temp dir so config is found
        let original_dir = std::env::current_dir().unwrap();
        let _ = std::env::set_current_dir(temp_dir.path());

        let config = Config::load();

        let _ = std::env::set_current_dir(&original_dir);
        drop(temp_dir);

        assert!(matches!(config.model, ModelTypeConfig::Anthropic));
        assert_eq!(
            config.anthropic.api_key,
            Some("sk-ant-test-key".to_string())
        );
    }

    #[test]
    fn test_load_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("codr.toml");

        let config_content = r#"
this is not valid toml [[[
"#;

        std::fs::write(&config_path, config_content).unwrap();

        // Change to temp dir so config is found
        let original_dir = std::env::current_dir().unwrap();
        let _ = std::env::set_current_dir(temp_dir.path());

        // Should fall back to default
        let config = Config::load();

        let _ = std::env::set_current_dir(&original_dir);
        drop(temp_dir);

        // Should still be valid default config
        assert!(matches!(config.model, ModelTypeConfig::OpenAI));
    }

    // ============================================================
    // Config Serialization Tests
    // ============================================================

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let serialized = toml::to_string(&config).unwrap();

        assert!(serialized.contains("[openai]"));
        assert!(serialized.contains("base_url"));
        assert!(serialized.contains("model"));
    }

    #[test]
    fn test_config_deserialization() {
        let toml_content = r#"
model = "anthropic"

[anthropic]
api_key = "test-key"
"#;

        let config: Config = toml::from_str(toml_content).unwrap();

        assert!(matches!(config.model, ModelTypeConfig::Anthropic));
        assert_eq!(config.anthropic.api_key, Some("test-key".to_string()));
    }

    // ============================================================
    // ModelTypeConfig Tests
    // ============================================================

    #[test]
    fn test_model_type_config_serialization() {
        let openai = ModelTypeConfig::OpenAI;
        let serialized = serde_json::to_string(&openai).unwrap();
        assert!(serialized.contains("openai"));

        let anthropic = ModelTypeConfig::Anthropic;
        let serialized = serde_json::to_string(&anthropic).unwrap();
        assert!(serialized.contains("anthropic"));
    }

    #[test]
    fn test_model_type_config_deserialization() {
        let openai: ModelTypeConfig = serde_json::from_str(r#""openai""#).unwrap();
        assert!(matches!(openai, ModelTypeConfig::OpenAI));

        let anthropic: ModelTypeConfig = serde_json::from_str(r#""anthropic""#).unwrap();
        assert!(matches!(anthropic, ModelTypeConfig::Anthropic));
    }

    // ============================================================
    // OpenAIConfig Tests
    // ============================================================

    #[test]
    fn test_openai_config_default() {
        let config = OpenAIConfig::default();

        // derived Default gives empty strings
        assert_eq!(config.base_url, "");
        assert_eq!(config.model, "");
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_openai_config_serialization() {
        let config = OpenAIConfig::default();
        let serialized = toml::to_string(&config).unwrap();

        assert!(serialized.contains("base_url"));
        assert!(serialized.contains("model"));
    }

    // ============================================================
    // AnthropicConfig Tests
    // ============================================================

    #[test]
    fn test_anthropic_config_default() {
        let config = AnthropicConfig::default();

        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_anthropic_config_with_key() {
        let toml_content = r#"
api_key = "sk-ant-test"
"#;

        let config: AnthropicConfig = toml::from_str(toml_content).unwrap();

        assert_eq!(config.api_key, Some("sk-ant-test".to_string()));
    }
}
