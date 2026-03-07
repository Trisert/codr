//! Tests for configuration loading and management

use crate::config::Config;
use crate::model::ModelType;

#[test]
fn test_config_default_values() {
    let config = Config::default();

    // Verify default model type is OpenAI
    let model_type = config.to_model_type();
    match model_type {
        ModelType::OpenAI { .. } => {
            // Expected - default is OpenAI
        }
        _ => panic!("Default model should be OpenAI, got {:?}", model_type),
    }
}

#[test]
fn test_config_to_model_type_openai() {
    let config = Config::default();
    let model_type = config.to_model_type();

    match model_type {
        ModelType::OpenAI {
            base_url, model, ..
        } => {
            assert!(!base_url.is_empty());
            assert!(!model.is_empty());
        }
        _ => panic!("Expected OpenAI model type"),
    }
}

#[test]
fn test_config_to_model_type_has_defaults() {
    let config = Config::default();
    let model_type = config.to_model_type();

    // Just verify we can create a model type from config
    match model_type {
        ModelType::OpenAI { .. } | ModelType::Anthropic { .. } => {
            // Success - we got a valid model type
        }
    }
}
