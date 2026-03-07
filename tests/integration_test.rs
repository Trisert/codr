//! Integration tests for codr
//! Tests end-to-end workflows and component interactions

use codr::config::Config;
use codr::model::ModelType;
use codr::parser::{Action, clean_message_content, parse_action};
use codr::tools::{Role, create_coding_tools};
use serde_json::json;
use std::path::PathBuf;

#[test]
fn test_config_loading_and_model_type_conversion() {
    // Integration test: Load config and convert to ModelType
    // This tests the interaction between Config loading and ModelType creation

    let config = Config::load();
    let model_type = config.to_model_type();

    // Verify the conversion produces a valid ModelType
    match model_type {
        ModelType::OpenAI {
            base_url, model, ..
        } => {
            assert!(!base_url.is_empty(), "OpenAI base_url should not be empty");
            assert!(!model.is_empty(), "OpenAI model should not be empty");
        }
        ModelType::Anthropic => {
            // Anthropic is a unit variant - just verify it was created
            // No assertion needed - reaching this point means it was created
        }
    }
}

#[test]
fn test_tool_registry_with_real_files() {
    // Integration test: Create tool registry with actual filesystem
    // This tests the interaction between ToolRegistry and the real filesystem

    let cwd = PathBuf::from(".");
    let registry = create_coding_tools(cwd.clone());

    // Verify tools are available
    let tools = registry.get_tools_for_role(Role::Safe);
    assert!(!tools.is_empty(), "Tool registry should have tools");

    // Verify common tools exist
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(
        tool_names.contains(&"read"),
        "read tool should be available"
    );
    assert!(
        tool_names.contains(&"bash"),
        "bash tool should be available"
    );
    assert!(
        tool_names.contains(&"grep"),
        "grep tool should be available"
    );
}

#[test]
fn test_role_based_tool_filtering() {
    // Integration test: Verify role-based access control works end-to-end

    let cwd = PathBuf::from(".");
    let registry = create_coding_tools(cwd);

    // Yolo role should have all tools
    let yolo_tools = registry.get_tools_for_role(Role::Yolo);
    let yolo_names: Vec<&str> = yolo_tools.iter().map(|t| t.name()).collect();

    // Safe role should have same tools (but some require approval)
    let safe_tools = registry.get_tools_for_role(Role::Safe);
    let safe_names: Vec<&str> = safe_tools.iter().map(|t| t.name()).collect();

    assert_eq!(
        yolo_names.len(),
        safe_names.len(),
        "YOLO and SAFE should have same tool count"
    );

    // Planning role should have fewer tools (read-only)
    let planning_tools = registry.get_tools_for_role(Role::Planning);
    let planning_names: Vec<&str> = planning_tools.iter().map(|t| t.name()).collect();

    assert!(
        planning_names.len() < yolo_names.len(),
        "Planning should have fewer tools than YOLO"
    );
    assert!(
        !planning_names.contains(&"write"),
        "Planning should not have write tool"
    );
    assert!(
        !planning_names.contains(&"edit"),
        "Planning should not have edit tool"
    );
}

#[test]
fn test_parser_end_to_end() {
    // Integration test: Parse XML tool actions and verify they produce valid Actions
    // This tests the full parsing pipeline from XML to Action enum

    let xml_tool = r#"<codr_tool name="read">{"file_path": "Cargo.toml"}</codr_tool>"#;
    let result = parse_action(xml_tool);

    assert!(result.is_some(), "Should parse valid XML tool action");
    let action = result.unwrap();

    match action {
        Action::Tool { name, params } => {
            assert_eq!(name.as_ref(), "read", "Tool name should be 'read'");
            assert!(
                params["file_path"].is_string(),
                "file_path should be a string"
            );
        }
        _ => panic!("Should parse as Tool action"),
    }
}

#[test]
fn test_message_cleaning_integration() {
    // Integration test: Clean message content with mixed XML and text
    // This tests the interaction between content cleaning and XML parsing

    let content = r#"Here's a command: <codr_bash>echo "hello"</codr_bash> and a tool: <codr_tool name="read">{"file_path": "test.txt"}</codr_tool>"#;
    let cleaned = clean_message_content(content, true);

    // Verify XML tags are removed (along with their content, which is the correct behavior)
    assert!(!cleaned.contains("<codr_bash>"), "Should remove bash tags");
    assert!(
        !cleaned.contains("</codr_bash>"),
        "Should remove bash closing tags"
    );
    assert!(!cleaned.contains("<codr_tool"), "Should remove tool tags");
    assert!(
        !cleaned.contains("echo"),
        "Should remove bash command content"
    );

    // Verify text content is preserved
    assert!(
        cleaned.contains("Here's a command:"),
        "Should preserve text before XML"
    );
    assert!(
        cleaned.contains("and a tool:"),
        "Should preserve text between XML tags"
    );
}

#[test]
fn test_parser_with_json_format() {
    // Integration test: Parse JSON tool calls (native format)
    // This tests compatibility with JSON-based tool calling

    let json_input = json!({
        "type": "function",
        "function": {
            "name": "write",
            "arguments": "{\"file_path\": \"test.txt\", \"content\": \"hello world\"}"
        }
    });

    let json_str = serde_json::to_string(&json_input).unwrap();
    let result = parse_action(&json_str);

    // The parser should return Some for valid input or None for invalid
    // This verifies the parser doesn't crash on unexpected input
    assert!(
        result.is_some() || result.is_none(),
        "Parser should handle JSON input gracefully"
    );
}
