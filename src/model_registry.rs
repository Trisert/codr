// ============================================================
// Type-Safe Model Registry with Capabilities
// ============================================================

use crate::model::{Model, ModelType};
use crate::model_probe::ModelCapabilities;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Known model identifiers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KnownModel {
    // Anthropic Claude models
    #[serde(rename = "claude-sonnet-4-20250514")]
    ClaudeSonnet4,
    #[serde(rename = "claude-3-5-sonnet")]
    Claude35Sonnet,
    #[serde(rename = "claude-3-opus")]
    Claude3Opus,
    #[serde(rename = "claude-3-haiku")]
    Claude3Haiku,

    // OpenAI models
    #[serde(rename = "gpt-4-turbo")]
    GPT4Turbo,
    #[serde(rename = "gpt-4")]
    GPT4,
    #[serde(rename = "gpt-3.5-turbo")]
    GPT35Turbo,

    // DeepSeek models
    #[serde(rename = "deepseek-r1")]
    DeepSeekR1,
    #[serde(rename = "deepseek-v3")]
    DeepSeekV3,

    // Local LLM models (generic)
    #[serde(rename = "local-llama")]
    LocalLlama,
    #[serde(rename = "local-mistral")]
    LocalMistral,
    #[serde(rename = "local-qwen")]
    LocalQwen,

    // Custom/unknown models
    #[serde(rename = "custom")]
    Custom(String),
}

impl KnownModel {
    /// Parse a model name/identifier into a KnownModel
    pub fn from_name(name: &str) -> Self {
        let name_lower = name.to_lowercase();

        match name_lower.as_str() {
            // Anthropic Claude
            "claude-sonnet-4-20250514" | "claude-sonnet-4" | "claude-sonnet" | "claude" => {
                KnownModel::ClaudeSonnet4
            }
            "claude-3-5-sonnet" | "claude-3.5-sonnet" => KnownModel::Claude35Sonnet,
            "claude-3-opus" | "claude-opus" => KnownModel::Claude3Opus,
            "claude-3-haiku" | "claude-haiku" => KnownModel::Claude3Haiku,

            // OpenAI
            "gpt-4-turbo" | "gpt-4-turbo-preview" => KnownModel::GPT4Turbo,
            "gpt-4" => KnownModel::GPT4,
            "gpt-3.5-turbo" | "gpt-3.5" => KnownModel::GPT35Turbo,

            // DeepSeek
            "deepseek-r1" | "deepseek-r1-distill" | "deepseek-reasoner" => KnownModel::DeepSeekR1,
            "deepseek-v3" | "deepseek" => KnownModel::DeepSeekV3,

            // Local models (by pattern matching)
            n if n.contains("llama") => KnownModel::LocalLlama,
            n if n.contains("mistral") => KnownModel::LocalMistral,
            n if n.contains("qwen") => KnownModel::LocalQwen,

            // Custom/unknown
            _ => KnownModel::Custom(name.to_string()),
        }
    }

    /// Get the canonical name for this model
    pub fn canonical_name(&self) -> String {
        match self {
            KnownModel::ClaudeSonnet4 => "claude-sonnet-4-20250514".to_string(),
            KnownModel::Claude35Sonnet => "claude-3-5-sonnet".to_string(),
            KnownModel::Claude3Opus => "claude-3-opus".to_string(),
            KnownModel::Claude3Haiku => "claude-3-haiku".to_string(),
            KnownModel::GPT4Turbo => "gpt-4-turbo".to_string(),
            KnownModel::GPT4 => "gpt-4".to_string(),
            KnownModel::GPT35Turbo => "gpt-3.5-turbo".to_string(),
            KnownModel::DeepSeekR1 => "deepseek-r1".to_string(),
            KnownModel::DeepSeekV3 => "deepseek-v3".to_string(),
            KnownModel::LocalLlama => "local-llama".to_string(),
            KnownModel::LocalMistral => "local-mistral".to_string(),
            KnownModel::LocalQwen => "local-qwen".to_string(),
            KnownModel::Custom(name) => name.clone(),
        }
    }

    /// Get default capabilities for this model
    pub fn default_capabilities(&self) -> ModelCapabilities {
        match self {
            KnownModel::ClaudeSonnet4 => ModelCapabilities {
                supports_thinking: true,
                supports_tools: true,
                max_tokens: Some(200_000),
                model_name: "claude-sonnet-4-20250514".to_string(),
                provider_type: "anthropic".to_string(),
                detected_at: 0,
            },
            KnownModel::Claude35Sonnet => ModelCapabilities {
                supports_thinking: true,
                supports_tools: true,
                max_tokens: Some(200_000),
                model_name: "claude-3-5-sonnet".to_string(),
                provider_type: "anthropic".to_string(),
                detected_at: 0,
            },
            KnownModel::Claude3Opus => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(200_000),
                model_name: "claude-3-opus".to_string(),
                provider_type: "anthropic".to_string(),
                detected_at: 0,
            },
            KnownModel::Claude3Haiku => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(200_000),
                model_name: "claude-3-haiku".to_string(),
                provider_type: "anthropic".to_string(),
                detected_at: 0,
            },
            KnownModel::GPT4Turbo => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(128_000),
                model_name: "gpt-4-turbo".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::GPT4 => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(8_192),
                model_name: "gpt-4".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::GPT35Turbo => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(16_385),
                model_name: "gpt-3.5-turbo".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::DeepSeekR1 => ModelCapabilities {
                supports_thinking: true,
                supports_tools: true,
                max_tokens: Some(64_000),
                model_name: "deepseek-r1".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::DeepSeekV3 => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(64_000),
                model_name: "deepseek-v3".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::LocalLlama => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(128_000),
                model_name: "local-llama".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::LocalMistral => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(32_000),
                model_name: "local-mistral".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::LocalQwen => ModelCapabilities {
                supports_thinking: false,
                supports_tools: true,
                max_tokens: Some(32_000),
                model_name: "local-qwen".to_string(),
                provider_type: "openai".to_string(),
                detected_at: 0,
            },
            KnownModel::Custom(name) => {
                // Estimate capabilities for custom models
                ModelCapabilities {
                    supports_thinking: ModelCapabilities::estimate_thinking_support(name),
                    supports_tools: ModelCapabilities::estimate_tool_support(name),
                    max_tokens: Some(ModelCapabilities::estimate_max_tokens(name)),
                    model_name: name.clone(),
                    provider_type: "openai".to_string(),
                    detected_at: 0,
                }
            }
        }
    }
}

/// Model registry entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub known_model: KnownModel,
    pub capabilities: ModelCapabilities,
    pub available: bool,
}

/// Type-safe model registry
pub struct ModelRegistry {
    entries: HashMap<String, ModelEntry>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            entries: HashMap::new(),
        };

        // Register known models
        registry.register_defaults();

        registry
    }

    fn register_defaults(&mut self) {
        // Anthropic Claude models
        self.register(KnownModel::ClaudeSonnet4);
        self.register(KnownModel::Claude35Sonnet);
        self.register(KnownModel::Claude3Opus);
        self.register(KnownModel::Claude3Haiku);

        // OpenAI models
        self.register(KnownModel::GPT4Turbo);
        self.register(KnownModel::GPT4);
        self.register(KnownModel::GPT35Turbo);

        // DeepSeek models
        self.register(KnownModel::DeepSeekR1);
        self.register(KnownModel::DeepSeekV3);

        // Local model categories
        self.register(KnownModel::LocalLlama);
        self.register(KnownModel::LocalMistral);
        self.register(KnownModel::LocalQwen);
    }

    /// Register a model in the registry
    pub fn register(&mut self, model: KnownModel) {
        let capabilities = model.default_capabilities();
        let name = model.canonical_name();

        self.entries.insert(
            name.clone(),
            ModelEntry {
                known_model: model,
                capabilities,
                available: true,
            },
        );
    }

    /// Look up a model by name
    pub fn lookup(&self, name: &str) -> Option<&ModelEntry> {
        self.entries.get(name)
    }

    /// Look up or register a model by name (with pattern matching)
    pub fn lookup_or_register(&mut self, name: &str) -> &ModelEntry {
        if !self.entries.contains_key(name) {
            let known_model = KnownModel::from_name(name);
            self.register(known_model);
        }
        self.entries.get(name).unwrap()
    }

    /// Get capabilities for a model by name
    pub fn get_capabilities(&self, name: &str) -> Option<ModelCapabilities> {
        self.lookup(name).map(|entry| entry.capabilities.clone())
    }

    /// List all registered models
    pub fn list_models(&self) -> Vec<String> {
        let mut models: Vec<_> = self.entries.keys().cloned().collect();
        models.sort();
        models
    }

    /// List available models
    pub fn list_available(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.available)
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared model registry
pub type SharedModelRegistry = Arc<RwLock<ModelRegistry>>;

/// Create a new shared model registry
pub fn create_model_registry() -> SharedModelRegistry {
    Arc::new(RwLock::new(ModelRegistry::new()))
}

// ============================================================
// Integration with Model
// ============================================================

impl Model {
    /// Get the KnownModel identifier for this model
    pub fn known_model(&self) -> KnownModel {
        match self.model_type() {
            ModelType::Anthropic => KnownModel::ClaudeSonnet4,
            ModelType::OpenAI { model, .. } => KnownModel::from_name(model),
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_model_from_name() {
        assert_eq!(KnownModel::from_name("claude"), KnownModel::ClaudeSonnet4);
        assert_eq!(KnownModel::from_name("gpt-4"), KnownModel::GPT4);
        assert_eq!(KnownModel::from_name("deepseek-r1"), KnownModel::DeepSeekR1);
        assert_eq!(KnownModel::from_name("llama-3-8b"), KnownModel::LocalLlama);
    }

    #[test]
    fn test_known_model_canonical_name() {
        assert_eq!(
            KnownModel::ClaudeSonnet4.canonical_name(),
            "claude-sonnet-4-20250514"
        );
        assert_eq!(KnownModel::GPT4.canonical_name(), "gpt-4");
        assert_eq!(KnownModel::DeepSeekR1.canonical_name(), "deepseek-r1");
    }

    #[test]
    fn test_model_registry_new() {
        let registry = ModelRegistry::new();
        assert!(registry.lookup("claude-sonnet-4-20250514").is_some());
        assert!(registry.lookup("gpt-4").is_some());
    }

    #[test]
    fn test_model_registry_lookup() {
        let registry = ModelRegistry::new();
        let entry = registry.lookup("claude-sonnet-4-20250514").unwrap();
        assert_eq!(entry.known_model, KnownModel::ClaudeSonnet4);
        assert!(entry.capabilities.supports_thinking);
    }

    #[test]
    fn test_model_registry_lookup_or_register() {
        let mut registry = ModelRegistry::new();
        let unknown_model = "custom-model-xyz";

        // First call should register the model
        let entry1 = registry.lookup_or_register(unknown_model);
        assert_eq!(
            entry1.known_model,
            KnownModel::Custom(unknown_model.to_string())
        );

        // Clone the capabilities before the second call
        let name1 = entry1.capabilities.model_name.clone();

        // Second call should return the same entry
        let entry2 = registry.lookup_or_register(unknown_model);
        assert_eq!(name1, entry2.capabilities.model_name);
    }

    #[test]
    fn test_model_registry_list_models() {
        let registry = ModelRegistry::new();
        let models = registry.list_models();
        assert!(!models.is_empty());
        assert!(models.contains(&"claude-sonnet-4-20250514".to_string()));
        assert!(models.contains(&"gpt-4".to_string()));
    }

    #[test]
    fn test_known_model_custom() {
        let custom = KnownModel::Custom("my-custom-model".to_string());
        assert_eq!(custom.canonical_name(), "my-custom-model");
        assert!(matches!(custom, KnownModel::Custom(_)));
    }
}
