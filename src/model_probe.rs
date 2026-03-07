// ============================================================
// Model Capability Detection via Probe Queries
// ============================================================

use crate::model::{Model, ModelType};
use serde::{Deserialize, Serialize};

/// Detected capabilities of a model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Model supports extended thinking (like Claude's thinking blocks)
    pub supports_thinking: bool,
    /// Model supports native tool calling
    pub supports_tools: bool,
    /// Maximum context window in tokens (detected or estimated)
    pub max_tokens: Option<usize>,
    /// Model name/identifier
    pub model_name: String,
    /// Provider type (anthropic, openai, etc.)
    pub provider_type: String,
    /// Timestamp when capabilities were detected
    pub detected_at: i64,
}

impl ModelCapabilities {
    /// Create default capabilities for a model type
    pub fn defaults_for(model_type: &ModelType, model_name: &str) -> Self {
        let (provider_type, supports_thinking, supports_tools, max_tokens) = match model_type {
            ModelType::Anthropic => (
                "anthropic".to_string(),
                true,          // Anthropic Claude supports extended thinking
                true,          // Anthropic Claude supports native tools
                Some(200_000), // Claude 3.5 Sonnet has 200k context
            ),
            ModelType::OpenAI { model, .. } => {
                // For OpenAI-compatible, we make educated guesses based on model name
                let model_lower = model.to_lowercase();
                let supports_thinking = model_lower.contains("deepseek")
                    || model_lower.contains("r1")
                    || model_lower.contains("qwq")
                    || model_lower.contains("thinking");

                let supports_tools = true; // Most modern OpenAI-compatible servers support tools
                let max_tokens = if model_lower.contains("32k") {
                    Some(32_000)
                } else if model_lower.contains("8k") {
                    Some(8_192)
                } else {
                    Some(128_000) // Default to 128k for modern models
                };

                (
                    "openai".to_string(),
                    supports_thinking,
                    supports_tools,
                    max_tokens,
                )
            }
        };

        Self {
            supports_thinking,
            supports_tools,
            max_tokens,
            model_name: model_name.to_string(),
            provider_type,
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        }
    }

    /// Estimate if thinking is supported based on model name
    pub fn estimate_thinking_support(model_name: &str) -> bool {
        let name_lower = model_name.to_lowercase();
        // Explicit thinking models
        name_lower.contains("thinking")
            || name_lower.contains("r1")
            || name_lower.contains("qwq")
            || name_lower.contains("deepseek")
            // Anthropic Claude
            || name_lower.contains("claude")
            // New reasoning models
            || name_lower.contains("reason")
    }

    /// Estimate if tool calling is supported based on model name
    pub fn estimate_tool_support(model_name: &str) -> bool {
        let name_lower = model_name.to_lowercase();
        // Most modern models support tools
        // Exclude very small/old models that typically don't
        !name_lower.contains("-tiny")
            && !name_lower.contains("-mini")
            && !name_lower.contains("gpt-3.5")
    }

    /// Get max tokens for a model (estimated)
    pub fn estimate_max_tokens(model_name: &str) -> usize {
        let name_lower = model_name.to_lowercase();
        if name_lower.contains("claude") {
            200_000 // Claude 3.5 Sonnet
        } else if name_lower.contains("gpt-4") {
            128_000 // GPT-4 Turbo
        } else if name_lower.contains("32k") {
            32_000
        } else if name_lower.contains("16k") {
            16_384
        } else if name_lower.contains("8k") {
            8_192
        } else {
            128_000 // Default for modern models
        }
    }
}

impl Model {
    /// Get capabilities for this model (with cached detection)
    pub fn get_capabilities(&self) -> ModelCapabilities {
        let model_name = self.model_name();
        ModelCapabilities::defaults_for(self.model_type(), &model_name)
    }

    /// Check if model supports extended thinking
    pub fn supports_thinking(&self) -> bool {
        match self.model_type() {
            ModelType::Anthropic => true,
            ModelType::OpenAI { model, .. } => ModelCapabilities::estimate_thinking_support(model),
        }
    }

    /// Get the maximum context window size
    pub fn max_context_tokens(&self) -> usize {
        match self.model_type() {
            ModelType::Anthropic => 200_000,
            ModelType::OpenAI { model, .. } => ModelCapabilities::estimate_max_tokens(model),
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
    fn test_estimate_thinking_support() {
        assert!(ModelCapabilities::estimate_thinking_support("claude-3"));
        assert!(ModelCapabilities::estimate_thinking_support("deepseek-r1"));
        assert!(ModelCapabilities::estimate_thinking_support("qwq-32b"));
        assert!(!ModelCapabilities::estimate_thinking_support("gpt-3.5"));
    }

    #[test]
    fn test_estimate_tool_support() {
        assert!(ModelCapabilities::estimate_tool_support("claude-3"));
        assert!(ModelCapabilities::estimate_tool_support("gpt-4"));
        assert!(!ModelCapabilities::estimate_tool_support("gpt-3.5-tiny"));
    }

    #[test]
    fn test_estimate_max_tokens() {
        assert_eq!(ModelCapabilities::estimate_max_tokens("claude-3"), 200_000);
        assert_eq!(ModelCapabilities::estimate_max_tokens("gpt-4"), 128_000);
        assert_eq!(ModelCapabilities::estimate_max_tokens("model-32k"), 32_000);
    }
}
