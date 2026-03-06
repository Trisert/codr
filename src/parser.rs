use crate::error::AgentError;
use serde_json::Value;
use std::sync::Arc;

// ============================================================
// Message Cleaning
// ============================================================

/// Clean content for display by removing XML tool tags
///
/// This function removes <codr_tool>...</codr_tool> and <codr_bash>...</codr_bash> tags
/// from content while preserving the semantic meaning for display purposes.
///
/// # Arguments
/// * `content` - The content to clean
/// * `trim_whitespace` - If true, trims leading/trailing whitespace. If false, preserves
///   whitespace (useful for streaming where incremental content shouldn't be trimmed).
///
/// # Returns
/// The cleaned content with XML tags removed
pub fn clean_message_content(content: &str, trim_whitespace: bool) -> String {
    let mut result = content.to_string();

    // Remove <codr_tool name="XXX">params</codr_tool> tags entirely
    while let Some(start) = result.find("<codr_tool") {
        if let Some(end_tag) = result[start..].find("</codr_tool>") {
            let end = start + end_tag + "</codr_tool>".len();
            result.replace_range(start..end, "");
        } else {
            result.truncate(start);
            break;
        }
    }

    // Remove <codr_bash>command</codr_bash> tags entirely
    while let Some(start) = result.find("<codr_bash>") {
        if let Some(end_tag) = result[start..].find("</codr_bash>") {
            let end = start + end_tag + "</codr_bash>".len();
            result.replace_range(start..end, "");
        } else {
            result.truncate(start);
            break;
        }
    }

    // Remove thinking tags entirely
    let thinking_tags = [("<thinking>", "</thinking>"), ("<thinking>", "</thinking>")];
    for (start_tag, end_tag) in thinking_tags {
        while let Some(start) = result.find(start_tag) {
            if let Some(end_offset) = result[start..].find(end_tag) {
                let end = start + end_offset + end_tag.len();
                result.replace_range(start..end, "");
            } else {
                result.truncate(start);
                break;
            }
        }
    }

    // Clean up extra whitespace that might result from tag removal
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }

    if trim_whitespace {
        result.trim().to_string()
    } else {
        result
    }
}

// ============================================================
// Action Types
// ============================================================

#[derive(Debug, Clone)]
pub enum Action {
    Response(Arc<String>),  // Shared response content
    Bash {
        command: Arc<str>,
        workdir: Option<Arc<str>>,  // Shared workdir
        timeout_ms: Option<u64>,
        env: Option<Value>,
    },
    Tool {
        name: Arc<str>,
        params: Value,
    },
}

impl Action {
    pub fn is_read_only(&self) -> bool {
        match self {
            Action::Tool { name, .. } => matches!(name.as_ref(), "read" | "grep" | "find" | "file_info"),
            Action::Bash { .. } => false,
            Action::Response(_) => false,
        }
    }
}

// ============================================================
// Parse Action
// ============================================================

/// Parse XML tool format: <codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
/// (Temporary compatibility during migration)
fn parse_xml_tool_call(content: &str) -> Option<Action> {
    // Strip thinking content if present (models may output <thinking>...</thinking> before tool calls)
    let content = if let Some(thinking_end) = content.find("</thinking>") {
        content[thinking_end + "</thinking>".len()..].trim_start()
    } else {
        content
    };

    if let Some(start) = content.find("<codr_tool")
        && let Some(name_start) = content[start..].find("name=\"") {
            let name_start = start + name_start + 6;
            if let Some(name_end) = content[name_start..].find('"') {
                let name = &content[name_start..name_start + name_end];
                if let Some(body_start) = content.find('>') {
                    let body_start = body_start + 1;
                    if let Some(body_end) = content[body_start..].find("</codr_tool>") {
                        let params_json = &content[body_start..body_start + body_end];
                        match serde_json::from_str::<Value>(params_json) {
                            Ok(params) => return Some(Action::Tool { name: name.into(), params }),
                            Err(_) => return Some(Action::Tool { name: name.into(), params: serde_json::json!({ "input": params_json }) }),
                        }
                    }
                }
            }
        }
    None
}

/// Parse JSON tool call format: {"name": "read", "arguments": {...}}
/// This is the standard format for OpenAI-compatible APIs
fn parse_json_tool_call(content: &str) -> Option<Action> {
    let value: Value = serde_json::from_str(content.trim()).ok()?;

    // Handle array format for multiple tool calls
    if let Some(arr) = value.as_array() {
        if let Some(first) = arr.first() {
            return extract_from_json_value(first);
        }
        return None;
    }

    extract_from_json_value(&value)
}

/// Extract tool call from a JSON value (handles both standard and Qwen formats)
fn extract_from_json_value(value: &Value) -> Option<Action> {
    // Qwen format: {"input": "..."} - extract XML from input field
    if let Some(input_str) = value.get("input").and_then(|v| v.as_str())
        && let Some(extracted) = extract_xml_tool_from_string(input_str) {
            // Return the extracted tool call
            return Some(Action::Tool {
                name: extracted.get("name")?.as_str()?.into(),
                params: extracted.get("arguments")?.clone(),
            });
        }

    // Standard format: {"name": "tool", "arguments": {...}}
    let name = value.get("name")?.as_str()?.into();
    let arguments = value.get("arguments")
        .or_else(|| value.get("parameters"))
        .cloned()
        .unwrap_or_default();

    Some(Action::Tool { name, params: arguments })
}

/// Extract XML tool call from a string (for models that embed tool calls in text)
fn extract_xml_tool_from_string(content: &str) -> Option<Value> {
    // Strip thinking content if present (models may output <thinking>...</thinking> before tool calls)
    let content_clean = if let Some(thinking_end) = content.find("</thinking>") {
        // Skip past the thinking tag and any whitespace/newlines
        content[thinking_end + "</thinking>".len()..].trim_start()
    } else {
        content
    };

    // Look for <codr_tool name="...">{...}</codr_tool>
    if let Some(start) = content_clean.find("<codr_tool") {
        let content_after = &content_clean[start..];
        // Find name="..." attribute
        if let Some(name_start) = content_after.find("name=\"") {
            let name_start_abs = start + name_start + 6;
            if let Some(name_end) = content_clean[name_start_abs..].find('"') {
                let name = &content_clean[name_start_abs..name_start_abs + name_end];
                // Trim the quotes that Qwen adds: name=\"find\" -> find
                let name = name.trim_matches('"');
                // Find the closing >
                if let Some(gt_pos) = content_after.find('>') {
                    let body_start = start + gt_pos + 1;
                    // Find closing tag
                    if let Some(end_tag) = content_clean[body_start..].find("</codr_tool>") {
                        let body = &content_clean[body_start..body_start + end_tag];
                        // The body should be JSON, possibly with escaped quotes
                        match serde_json::from_str::<Value>(body) {
                            Ok(params) => {
                                // Successfully parsed JSON
                                return Some(serde_json::json!({ "name": name, "arguments": params }));
                            }
                            Err(_e) => {
                                // JSON parse failed - might be malformed or escaped
                                // Try to handle common cases
                                let body_clean = body.trim();
                                if body_clean.starts_with('{') && body_clean.ends_with('}') {
                                    // It looks like JSON but has escape issues
                                    // Try to use it as-is
                                    return Some(serde_json::json!({ "name": name, "arguments": body_clean }));
                                }
                                // Last resort: use as input field
                                return Some(serde_json::json!({ "name": name, "arguments": serde_json::json!({ "input": body_clean }) }));
                            }
                        }
                    }
                }
            }
        }
    }

    // Also try to find bash commands
    if let Some(start) = content_clean.find("<codr_bash>") {
        let content_after = &content_clean[start..];
        if let Some(end) = content_after.find("</codr_bash>") {
            let command = &content_clean[start + 11..start + end];
            return Some(serde_json::json!({
                "name": "bash",
                "arguments": { "command": command.trim() }
            }));
        }
    }

    None
}

/// Parse JSON bash format: {"command": "ls -la"} or just the command as string
fn parse_json_bash(content: &str) -> Option<Action> {
    let trimmed = content.trim();

    // Try JSON format first
    if let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && let Some(command) = value.get("command").and_then(|v| v.as_str()) {
            return Some(Action::Bash {
                command: command.into(),
                workdir: value.get("cwd")
                    .or_else(|| value.get("workdir"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.into()),
                timeout_ms: value.get("timeout_ms")
                    .or_else(|| value.get("timeout"))
                    .and_then(|v| v.as_u64()),
                env: value.get("env").cloned(),
            });
        }

    // If it's just a string (simple command)
    if trimmed.starts_with('"') && trimmed.ends_with('"')
        && let Ok(s) = serde_json::from_str::<String>(trimmed) {
            return Some(Action::Bash {
                command: s.into(),
                workdir: None,
                timeout_ms: None,
                env: None,
            });
        }

    None
}

/// Parse bash action from <codr_bash> XML format (legacy support)
fn parse_xml_bash(content: &str) -> Option<Action> {
    if let Some(start) = content.find("<codr_bash>")
        && let Some(end) = content.find("</codr_bash>") {
        let command = &content[start + 11..end];
        return Some(Action::Bash {
            command: command.trim().into(),
            workdir: None,
            timeout_ms: None,
            env: None,
        });
    }
    None
}

/// Parse bash command (supports JSON and XML formats)
fn parse_bash_action(content: &str) -> Action {
    // Try XML format first (for backward compatibility during migration)
    if let Some(action) = parse_xml_bash(content) {
        return action;
    }

    // Try JSON format
    if let Some(action) = parse_json_bash(content) {
        return action;
    }

    // Fallback: treat entire content as command
    Action::Bash {
        command: content.trim().into(),
        workdir: None,
        timeout_ms: None,
        env: None,
    }
}

/// Main parse function - detects format and parses accordingly
pub fn parse_action(content: &str) -> Option<Action> {
    let trimmed = content.trim();

    // Empty content means plain response
    if trimmed.is_empty() {
        return Some(Action::Response(Arc::new(String::new())));
    }

    // Try XML tool format first (backward compatibility)
    if let Some(action) = parse_xml_tool_call(trimmed) {
        return Some(action);
    }

    // Try JSON tool call format
    if let Some(action) = parse_json_tool_call(trimmed) {
        return Some(action);
    }

    // Check if it's a bash command (starts with <codr_bash>)
    if trimmed.contains("<codr_bash>") {
        return Some(parse_bash_action(trimmed));
    }

    // If it looks like JSON but we couldn't parse it, treat as error
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return None;
    }

    // Otherwise, it's a plain text response
    Some(Action::Response(Arc::new(trimmed.to_string())))
}

/// Extract JSON object(s) from mixed text/JSON content
/// Handles cases where models output thinking text followed by JSON
fn extract_json_from_content(content: &str) -> Option<String> {
    let trimmed = content.trim();

    // If it starts with { or [, it's already pure JSON
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Some(trimmed.to_string());
    }

    // Strip markdown-style thinking blocks like "**Thinking:** ... \n\n"
    let content_to_search = if let Some(thinking_start) = trimmed.find("**Thinking:**") {
        let after_thinking = &trimmed[thinking_start + "**Thinking:**".len()..];
        // Look for the end of the thinking (double newline or the JSON start)
        if let Some(json_start) = after_thinking.find("\n\n{\"") {
            after_thinking[json_start + 2..].trim()
        } else if let Some(json_start) = after_thinking.find("\n{\"") {
            after_thinking[json_start + 1..].trim()
        } else if let Some(json_start) = after_thinking.find("{\"") {
            after_thinking[json_start..].trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    // Look for JSON object in the content
    // Find the first { that starts a JSON object
    if let Some(start) = content_to_search.find('{') {
        // Find the matching closing brace
        let mut brace_count = 0;
        let mut in_string = false;
        let mut escape_next = false;

        for (i, c) in content_to_search[start..].chars().enumerate() {
            match c {
                '\\' if in_string => escape_next = true,
                '"' if !escape_next => in_string = !in_string,
                '{' if !in_string => brace_count += 1,
                '}' if !in_string => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        // Found the complete JSON object
                        return Some(trimmed[start..start + i + 1].trim().to_string());
                    }
                }
                _ => escape_next = false,
            }
        }
    }

    // Look for JSON array
    if let Some(start) = content_to_search.find('[') {
        let mut bracket_count = 0;
        let mut in_string = false;
        let mut escape_next = false;

        for (i, c) in content_to_search[start..].chars().enumerate() {
            match c {
                '\\' if in_string => escape_next = true,
                '"' if !escape_next => in_string = !in_string,
                '[' if !in_string => bracket_count += 1,
                ']' if !in_string => {
                    bracket_count -= 1;
                    if bracket_count == 0 {
                        return Some(trimmed[start..start + i + 1].trim().to_string());
                    }
                }
                _ => escape_next = false,
            }
        }
    }

    None
}

/// Parse multiple actions (for handling multiple tool calls in one response)
pub fn parse_actions(content: &str) -> Result<Vec<Action>, AgentError> {
    let mut actions = Vec::new();
    let remaining = content.trim();

    if remaining.is_empty() {
        return Ok(vec![Action::Response(Arc::new(String::new()))]);
    }

    // First, try to extract JSON from mixed content (handles models that output thinking + JSON)
    if let Some(json_str) = extract_json_from_content(remaining) {
        // Try to parse as JSON array of tool calls
        if let Ok(value) = serde_json::from_str::<Value>(&json_str) {
            if let Some(arr) = value.as_array() {
                for item in arr {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        actions.push(Action::Tool {
                            name: name.into(),
                            params: item.get("arguments")
                                .or_else(|| item.get("parameters"))
                                .or_else(|| item.get("input"))
                                .cloned()
                                .unwrap_or_default(),
                        });
                    }
                }
                if !actions.is_empty() {
                    return Ok(actions);
                }
            }

            // Try single tool call
            if let Some(name) = value.get("name").and_then(|v| v.as_str()) {
                actions.push(Action::Tool {
                    name: name.into(),
                    params: value.get("arguments")
                        .or_else(|| value.get("parameters"))
                        .or_else(|| value.get("input"))
                        .cloned()
                        .unwrap_or_default(),
                });
                return Ok(actions);
            } else {
                // Fallback: Detect common malformed patterns
                // If JSON has "content" field but no "name", it might be a malformed write call
                if value.get("content").is_some() {
                    // Check if there's also a "file_path" field
                    let params = if value.get("file_path").is_some() {
                        // Has both file_path and content - reconstruct proper write call
                        value.clone()
                    } else {
                        // Has only content - create default file path
                        serde_json::json!({
                            "file_path": "output.txt",
                            "content": value.get("content").unwrap()
                        })
                    };

                    actions.push(Action::Tool {
                        name: "write".into(),
                        params,
                    });
                    return Ok(actions);
                }
            }
        }
    }

    // Try parsing the whole thing as JSON (for pure JSON without thinking)
    if let Ok(value) = serde_json::from_str::<Value>(remaining)
        && let Some(arr) = value.as_array() {
            for item in arr {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    actions.push(Action::Tool {
                        name: name.into(),
                        params: item.get("arguments")
                            .or_else(|| item.get("parameters"))
                            .or_else(|| item.get("input"))
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }
            if !actions.is_empty() {
                return Ok(actions);
            }
        }

    // Single action
    match parse_action(remaining) {
        Some(action) => Ok(vec![action]),
        None => Err(AgentError::Terminating(format!("Failed to parse action: {}", remaining))),
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_tool_call() {
        let json = r#"{"name": "read", "arguments": {"file_path": "test.txt"}}"#;
        let action = parse_action(json);
        assert!(action.is_some());
        match action.unwrap() {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "test.txt");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_json_tool_call_with_parameters() {
        let json = r#"{"name": "grep", "parameters": {"pattern": "test"}}"#;
        let action = parse_action(json);
        assert!(action.is_some());
        match action.unwrap() {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "grep");
                assert_eq!(params["pattern"], "test");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_json_bash() {
        let json = r#"{"command": "ls -la"}"#;
        let action = parse_json_bash(json);
        assert!(action.is_some());
        match action.unwrap() {
            Action::Bash { command, .. } => {
                assert_eq!(command.as_ref(), "ls -la");
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_parse_xml_bash() {
        let xml = r#"<codr_bash>ls -la</codr_bash>"#;
        let action = parse_xml_bash(xml);
        assert!(action.is_some());
        match action.unwrap() {
            Action::Bash { command, .. } => {
                assert_eq!(command.as_ref(), "ls -la");
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_parse_plain_response() {
        let text = "Hello, world!";
        let action = parse_action(text);
        assert!(action.is_some());
        match action.unwrap() {
            Action::Response(content) => {
                assert_eq!(content.as_ref(), "Hello, world!");
            }
            _ => panic!("Expected Response action"),
        }
    }

    #[test]
    fn test_parse_empty_content() {
        let action = parse_action("");
        assert!(action.is_some());
        match action.unwrap() {
            Action::Response(content) => {
                assert!(content.is_empty());
            }
            _ => panic!("Expected Response action"),
        }
    }

    #[test]
    fn test_is_read_only() {
        let read_action = Action::Tool {
            name: Arc::from("read"),
            params: serde_json::json!({"file_path": "test.txt"}),
        };
        assert!(read_action.is_read_only());

        let bash_action = Action::Bash {
            command: Arc::from("ls"),
            workdir: None,
            timeout_ms: None,
            env: None,
        };
        assert!(!bash_action.is_read_only());
    }
}
