//! Tests for XML and JSON action parsing

use crate::parser::{Action, clean_message_content};
use serde_json::json;
use std::sync::Arc;

#[test]
fn test_clean_message_content_removes_tool_tags() {
    let content =
        r#"Some text <codr_tool name="read">{"file_path": "test.txt"}</codr_tool> more text"#;
    let cleaned = clean_message_content(content, true);
    assert!(!cleaned.contains("<codr_tool"));
    assert!(!cleaned.contains("</codr_tool>"));
    assert!(cleaned.contains("Some text"));
    assert!(cleaned.contains("more text"));
}

#[test]
fn test_clean_message_content_removes_bash_tags() {
    let content = r#"Text before <codr_bash>ls -la</codr_bash> text after"#;
    let cleaned = clean_message_content(content, true);
    assert!(!cleaned.contains("<codr_bash>"));
    assert!(!cleaned.contains("</codr_bash>"));
    assert!(cleaned.contains("Text before"));
    assert!(cleaned.contains("text after"));
}

#[test]
fn test_clean_message_content_removes_thinking_tags() {
    let content = r#"Thinking... <thinking>Let me consider</thinking> Done"#;
    let cleaned = clean_message_content(content, true);
    assert!(!cleaned.contains("<thinking>"));
    assert!(!cleaned.contains("</thinking>"));
    assert!(cleaned.contains("Thinking..."));
    assert!(cleaned.contains("Done"));
}

#[test]
fn test_clean_message_content_preserves_whitespace_when_requested() {
    let content = "  Text with spaces  ";
    let cleaned = clean_message_content(content, false);
    assert_eq!(cleaned, "  Text with spaces  ");
}

#[test]
fn test_clean_message_content_trims_whitespace_by_default() {
    let content = "  Text with spaces  ";
    let cleaned = clean_message_content(content, true);
    assert_eq!(cleaned, "Text with spaces");
}

#[test]
fn test_clean_message_content_removes_multiple_newlines() {
    let content = "Line 1\n\n\n\n\nLine 2";
    let cleaned = clean_message_content(content, true);
    assert!(!cleaned.contains("\n\n\n"));
    assert!(cleaned.contains("Line 1\n\nLine 2"));
}

#[test]
fn test_action_response_creation() {
    let response = Action::Response(Arc::new("Test response".to_string()));
    match response {
        Action::Response(content) => assert_eq!(*content, "Test response"),
        _ => panic!("Expected Response action"),
    }
}

#[test]
fn test_action_bash_creation() {
    let bash = Action::Bash {
        command: Arc::from("ls -la"),
        workdir: None,
        timeout_ms: Some(5000),
        env: None,
    };

    match bash {
        Action::Bash { command, .. } => assert_eq!(command.as_ref(), "ls -la"),
        _ => panic!("Expected Bash action"),
    }
}

#[test]
fn test_action_tool_creation() {
    let params = json!({"file_path": "test.txt", "content": "test"});
    let tool = Action::Tool {
        name: Arc::from("write"),
        params,
    };

    match tool {
        Action::Tool { name, .. } => assert_eq!(name.as_ref(), "write"),
        _ => panic!("Expected Tool action"),
    }
}

#[test]
fn test_action_is_read_only_for_read_tool() {
    let read_action = Action::Tool {
        name: Arc::from("read"),
        params: json!({"file_path": "test.txt"}),
    };
    assert!(read_action.is_read_only());
}

#[test]
fn test_action_is_read_only_for_grep_tool() {
    let grep_action = Action::Tool {
        name: Arc::from("grep"),
        params: json!({"pattern": "test", "path": "."}),
    };
    assert!(grep_action.is_read_only());
}

#[test]
fn test_action_is_not_read_only_for_write_tool() {
    let write_action = Action::Tool {
        name: Arc::from("write"),
        params: json!({"file_path": "test.txt", "content": "test"}),
    };
    assert!(!write_action.is_read_only());
}

#[test]
fn test_action_bash_is_not_read_only() {
    let bash_action = Action::Bash {
        command: Arc::from("rm -rf /"),
        workdir: None,
        timeout_ms: None,
        env: None,
    };
    assert!(!bash_action.is_read_only());
}

#[test]
fn test_action_response_is_not_read_only() {
    let response_action = Action::Response(Arc::new("Done".to_string()));
    assert!(!response_action.is_read_only());
}

#[test]
fn test_action_find_tool_is_read_only() {
    let find_action = Action::Tool {
        name: Arc::from("find"),
        params: json!({"path": ".", "pattern": "*.rs"}),
    };
    assert!(find_action.is_read_only());
}

#[test]
fn test_action_file_info_tool_is_read_only() {
    let file_info_action = Action::Tool {
        name: Arc::from("file_info"),
        params: json!({"file_path": "test.rs"}),
    };
    assert!(file_info_action.is_read_only());
}
