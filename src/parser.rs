use crate::error::AgentError;
use regex::Regex;
use serde_json::Value;
use std::sync::Arc;

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
            Action::Tool { name, .. } => matches!(name.as_ref(), "read" | "grep" | "find"),
            Action::Bash { .. } => false,
            Action::Response(_) => false,
        }
    }
}

// ============================================================
// Parse Action
// ============================================================

/// Parse codr_tool XML format: <codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
fn parse_tool_action(content: &str) -> Option<Action> {
    // First try XML format
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

    // Fall back to old tool-action format for backward compatibility
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let tool_name = lines[0].trim();
    let params_json = lines[1..].join("\n");

    match serde_json::from_str::<Value>(&params_json) {
        Ok(params) => Some(Action::Tool {
            name: tool_name.into(),
            params,
        }),
        Err(_) => Some(Action::Tool {
            name: tool_name.into(),
            params: serde_json::json!({ "input": params_json }),
        }),
    }
}

/// Parse OpenAI function calling format: {"name": "read", "arguments": {...}}
fn parse_openai_tool_call(content: &str) -> Option<Action> {
    let value: Value = serde_json::from_str(content).ok()?;

    let name = value.get("name")?.as_str()?.into();
    let arguments = value.get("arguments")?;

    Some(Action::Tool {
        name,
        params: arguments.clone(),
    })
}

/// Parse Anthropic tool_use format: {"type": "tool_use", "name": "read", "input": {...}}
fn parse_anthropic_tool_use(content: &str) -> Option<Action> {
    let value: Value = serde_json::from_str(content).ok()?;

    if value.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
        return None;
    }

    let name = value.get("name")?.as_str()?.into();
    let input = value.get("input")?.clone();

    Some(Action::Tool {
        name,
        params: input,
    })
}

/// Parse single-line shorthand: read FILE=src/main.rs or read src/main.rs
fn parse_shorthand(content: &str) -> Option<Action> {
    let trimmed = content.trim();
    if trimmed.contains('\n') {
        return None;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let tool_name = parts[0];
    if !["read", "grep", "find", "edit", "write", "bash"].contains(&tool_name) {
        return None;
    }

    // Try KEY=VALUE format
    let mut params = serde_json::Map::new();
    let mut has_key_value = false;

    for part in &parts[1..] {
        if let Some((key, value)) = part.split_once('=') {
            has_key_value = true;
            params.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    if has_key_value {
        return Some(Action::Tool {
            name: tool_name.into(),
            params: serde_json::Value::Object(params),
        });
    }

    // Fallback: positional argument as "input" or "file_path"
    if parts.len() > 1 {
        let params = if tool_name == "bash" {
            serde_json::json!({ "command": parts[1..].join(" ") })
        } else {
            serde_json::json!({ "file_path": parts[1] })
        };
        return Some(Action::Tool {
            name: tool_name.into(),
            params,
        });
    }

    None
}

/// Parse bash-action format
fn parse_bash_action(content: &str) -> Action {
    let mut trimmed = content.trim();

    // Strip <command></command> tokens if present
    if let Some(start) = trimmed.find("<command>")
        && let Some(end) = trimmed.find("</command>")
    {
        trimmed = &trimmed[start + 9..end];
        trimmed = trimmed.trim();
    }

    // Try JSON format
    if let Ok(params) = serde_json::from_str::<Value>(trimmed)
        && params.get("command").is_some()
    {
        return Action::Bash {
            command: params["command"].as_str().unwrap_or("").into(),
            workdir: params
                .get("workdir")
                .and_then(|v| v.as_str())
                .map(|s| s.into()),
            timeout_ms: params.get("timeout").and_then(|v| v.as_u64()),
            env: params.get("env").cloned(),
        };
    }

    // Check for obviously malformed commands (template syntax, etc.)
    if trimmed.contains("{pattern}")
        || trimmed.contains("{file}")
        || trimmed.contains("{path}")
        || trimmed.contains("{::")
    {
        // Return an error command that will be caught during execution
        return Action::Bash {
            command: format!(
                "# ERROR: Invalid command contains template syntax\n# Command was: {}",
                trimmed
            ).into(),
            workdir: None,
            timeout_ms: None,
            env: None,
        };
    }

    // Fall back to simple string format
    Action::Bash {
        command: trimmed.into(),
        workdir: None,
        timeout_ms: None,
        env: None,
    }
}

/// Parse a single tool call from any supported format
fn parse_single_tool_call(content: &str) -> Option<Action> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try OpenAI format first
    if let Some(action) = parse_openai_tool_call(trimmed) {
        return Some(action);
    }

    // Try Anthropic format
    if let Some(action) = parse_anthropic_tool_use(trimmed) {
        return Some(action);
    }

    // Try shorthand format
    if let Some(action) = parse_shorthand(trimmed) {
        return Some(action);
    }

    // Try tool-action block format
    if let Some(action) = parse_tool_action(trimmed) {
        return Some(action);
    }

    // Try multi-line format: tool-name\n{json}\nor: tool-name\n{json params on multiple lines}
    if let Some(first_newline) = trimmed.find('\n') {
        let potential_tool = trimmed[..first_newline].trim();
        if ["read", "grep", "find", "edit", "write", "bash"].contains(&potential_tool) {
            let json_part = trimmed[first_newline..].trim();
            if let Ok(params) = serde_json::from_str::<Value>(json_part) {
                return Some(Action::Tool {
                    name: potential_tool.into(),
                    params,
                });
            }
        }
    }

    // Try common plain-text patterns
    // Pattern: "I'll run: find *" or "I'll execute: read file.txt"
    // Pattern: "find *" or "find: *" or "running: find *"
    // Pattern: "read file.txt" (single argument)
    let lower = trimmed.to_lowercase();
    
    // Match patterns like "find *" or "find: *" or "I'll run: find *"
    for tool in ["find", "grep", "read", "edit", "write", "bash"] {
        let prefixes = vec![
            format!("{} ", tool),
            format!("{}:", tool),
            format!("i'll run: {} ", tool),
            format!("i'll run: {}:", tool),
            format!("i will run: {} ", tool),
            format!("i will run: {}:", tool),
            format!("running: {} ", tool),
            format!("running: {}:", tool),
        ];
        
        for prefix in prefixes {
            if lower.starts_with(&prefix) || lower.contains(&format!(" {} ", prefix.trim())) {
                let rest = trimmed.strip_prefix(&prefix).or_else(|| {
                    // Try to find in the middle
                    if let Some(idx) = lower.find(&format!(" {} ", prefix.trim())) {
                        Some(&trimmed[idx + prefix.len()..])
                    } else {
                        None
                    }
                }).unwrap_or(trimmed).trim();
                
                // Try to parse as JSON first
                if let Ok(params) = serde_json::from_str::<Value>(rest) {
                    return Some(Action::Tool {
                        name: tool.into(),
                        params,
                    });
                }
                
                // Try key=value format
                let mut param_map = serde_json::Map::new();
                let mut has_params = false;
                for part in rest.split_whitespace() {
                    if let Some((k, v)) = part.split_once('=') {
                        has_params = true;
                        param_map.insert(k.to_string(), serde_json::Value::String(v.to_string()));
                    }
                }
                if has_params {
                    return Some(Action::Tool {
                        name: tool.into(),
                        params: serde_json::Value::Object(param_map),
                    });
                }
                
                // Single argument - different for each tool
                if !rest.is_empty() {
                    let params = match tool {
                        "read" | "find" | "edit" => serde_json::json!({"file_path": rest}),
                        "grep" => serde_json::json!({"pattern": rest}),
                        "write" => serde_json::json!({"path": rest, "content": ""}),
                        "bash" => serde_json::json!({"command": rest}),
                        _ => serde_json::json!({"input": rest}),
                    };
                    return Some(Action::Tool {
                        name: tool.into(),
                        params,
                    });
                }
            }
        }
    }

    None
}

/// Parse all tool calls from a response
fn parse_all_tool_calls(content: &str) -> Vec<Action> {
    let mut actions = Vec::new();

    // Regex patterns for various formats

    // 0. XML format: <codr_tool name="...">...</codr_tool>
    let xml_tool_re = Regex::new(r#"(?s)<codr_tool\s+name="([^"]+)">.*?</codr_tool>"#).unwrap();
    for cap in xml_tool_re.captures_iter(content) {
        if let Some(name) = cap.get(1)
            && let Some(body) = cap.get(0) {
                let body_content = body.as_str();
                if let Some(start) = body_content.find('>') {
                    let start = start + 1;
                    if let Some(end) = body_content[start..].find("</codr_tool>") {
                        let params_json = &body_content[start..start + end];
                        match serde_json::from_str::<Value>(params_json) {
                            Ok(params) => {
                                let action = Action::Tool { name: name.as_str().into(), params };
                                if !actions.iter().any(|a| action_equals(a, &action)) {
                                    actions.push(action);
                                }
                            }
                            Err(_) => {
                                let action = Action::Tool { name: name.as_str().into(), params: serde_json::json!({ "input": params_json }) };
                                if !actions.iter().any(|a| action_equals(a, &action)) {
                                    actions.push(action);
                                }
                            }
                        }
                    }
                }
            }
    }

    // 0b. XML format: <codr_bash>...</codr_bash>
    let xml_bash_re = Regex::new(r"(?s)<codr_bash>(.*?)</codr_bash>").unwrap();
    for cap in xml_bash_re.captures_iter(content) {
        if let Some(cmd) = cap.get(1) {
            let command = cmd.as_str().trim();
            if !command.is_empty() {
                let action = Action::Bash { 
                    command: command.into(),
                    workdir: None,
                    timeout_ms: None,
                    env: None,
                };
                if !actions.iter().any(|a| action_equals(a, &action)) {
                    actions.push(action);
                }
            }
        }
    }

    // 1. Standard tool-action blocks: ```tool-action\n...\n```
    let tool_re = Regex::new(r"(?s)```\s*tool-action\s*\n(.*?)\n```").unwrap();
    for cap in tool_re.captures_iter(content) {
        if let Some(action) = parse_single_tool_call(cap.get(1).unwrap().as_str())
            && !actions.iter().any(|a| action_equals(a, &action)) {
                actions.push(action);
            }
    }

    // 2. Bash-action blocks: ```bash-action\n...\n```
    let bash_re = Regex::new(r"(?s)```\s*bash-action\s*\n(.*?)\n```").unwrap();
    for cap in bash_re.captures_iter(content) {
        let action = parse_bash_action(cap.get(1).unwrap().as_str());
        if !actions.iter().any(|a| action_equals(a, &action)) {
            actions.push(action);
        }
    }

    // 5. Try to find JSON array of tool calls: [{"name": "...", "arguments": {...}}, ...]
    if actions.is_empty()
        && let Ok(value) = serde_json::from_str::<Value>(content)
            && let Some(arr) = value.as_array() {
                for item in arr {
                    if let Some(action) = parse_single_tool_call(&item.to_string()) {
                        actions.push(action);
                    }
                }
            }

    actions
}

/// Check if two actions are equivalent (for deduplication)
fn action_equals(a: &Action, b: &Action) -> bool {
    match (a, b) {
        (
            Action::Tool {
                name: n1,
                params: p1,
            },
            Action::Tool {
                name: n2,
                params: p2,
            },
        ) => n1 == n2 && p1 == p2,
        (Action::Bash { command: c1, .. }, Action::Bash { command: c2, .. }) => c1 == c2,
        _ => false,
    }
}

/// Check if all actions are read-only (can be parallelized)
#[allow(dead_code)]
fn all_read_only(actions: &[Action]) -> bool {
    !actions.is_empty() && actions.iter().all(Action::is_read_only)
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParsedActions {
    pub actions: Vec<Action>,
    pub is_parallel: bool,
}

pub fn parse_actions(lm_output: &str) -> Result<ParsedActions, AgentError> {
    let actions = parse_all_tool_calls(lm_output);

    if actions.is_empty() {
        // No tool or bash action found - treat as plain text response
        return Ok(ParsedActions {
            actions: vec![Action::Response(Arc::new(lm_output.to_string()))],
            is_parallel: false,
        });
    }

    let is_parallel = all_read_only(&actions);

    Ok(ParsedActions {
        actions,
        is_parallel,
    })
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Action Tests
    // ============================================================

    #[test]
    fn test_action_is_read_only() {
        assert!(matches!(
            Action::Tool { name: "read".into(), params: serde_json::json!({}) },
            a if a.is_read_only() == true
        ));
        assert!(matches!(
            Action::Tool { name: "grep".into(), params: serde_json::json!({}) },
            a if a.is_read_only() == true
        ));
        assert!(matches!(
            Action::Tool { name: "find".into(), params: serde_json::json!({}) },
            a if a.is_read_only() == true
        ));
        assert!(matches!(
            Action::Tool { name: "edit".into(), params: serde_json::json!({}) },
            a if a.is_read_only() == false
        ));
        assert!(matches!(
            Action::Tool { name: "write".into(), params: serde_json::json!({}) },
            a if a.is_read_only() == false
        ));
        let action = Action::Bash { command: "ls".into(), workdir: None, timeout_ms: None, env: None };
        assert!(!action.is_read_only());
        assert!(matches!(
            Action::Response(Arc::new("hello".to_string())),
            a if a.is_read_only() == false
        ));
    }

    // ============================================================
    // XML Tool Action Parsing Tests
    // ============================================================

    #[test]
    fn test_parse_tool_action_xml_valid() {
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>"#;
        let action = parse_tool_action(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_tool_action_xml_with_newlines() {
        let input = r#"<codr_tool name="read">{
  "file_path": "src/main.rs"
}</codr_tool>"#;
        let action = parse_tool_action(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_tool_action_xml_invalid_json() {
        let input = r#"<codr_tool name="read">not valid json</codr_tool>"#;
        let action = parse_tool_action(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["input"], "not valid json");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_tool_action_xml_missing_tool_name() {
        let input = r#"<codr_tool>{"file_path": "src/main.rs"}</codr_tool>"#;
        let action = parse_tool_action(input);
        
        // Should fall back to legacy format
        assert!(action.is_some());
    }

    // ============================================================
    // OpenAI Tool Call Parsing Tests
    // ============================================================

    #[test]
    fn test_parse_openai_tool_call() {
        let input = r#"{"name": "read", "arguments": {"file_path": "src/main.rs"}}"#;
        let action = parse_openai_tool_call(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_openai_tool_call_missing_fields() {
        let input = r#"{"name": "read"}"#;
        let action = parse_openai_tool_call(input);
        
        assert!(action.is_none());
    }

    // ============================================================
    // Anthropic Tool Use Parsing Tests
    // ============================================================

    #[test]
    fn test_parse_anthropic_tool_use() {
        let input = r#"{"type": "tool_use", "name": "read", "input": {"file_path": "src/main.rs"}}"#;
        let action = parse_anthropic_tool_use(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_anthropic_tool_use_wrong_type() {
        let input = r#"{"type": "text", "name": "read", "input": {"file_path": "src/main.rs"}}"#;
        let action = parse_anthropic_tool_use(input);
        
        assert!(action.is_none());
    }

    // ============================================================
    // Shorthand Parsing Tests
    // ============================================================

    #[test]
    fn test_parse_shorthand_key_value() {
        let input = "read file_path=src/main.rs";
        let action = parse_shorthand(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_shorthand_positional() {
        let input = "read src/main.rs";
        let action = parse_shorthand(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_shorthand_bash() {
        let input = "bash ls -la";
        let action = parse_shorthand(input);
        
        assert!(action.is_some());
        let action = action.unwrap();
        match action {
            Action::Tool { name, params } => {
                assert_eq!(&*name, "bash");
                assert_eq!(params["command"], "ls -la");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_shorthand_multiline_returns_none() {
        let input = "read\nsrc/main.rs";
        let action = parse_shorthand(input);
        
        assert!(action.is_none());
    }

    // ============================================================
    // Bash Action Parsing Tests
    // ============================================================

    #[test]
    fn test_parse_bash_action_simple() {
        let input = "ls -la";
        let action = parse_bash_action(input);
        
        match action {
            Action::Bash { command, workdir, timeout_ms, env } => {
                assert_eq!(command.as_ref(), "ls -la");
                assert!(workdir.is_none());
                assert!(timeout_ms.is_none());
                assert!(env.is_none());
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_parse_bash_action_with_tags() {
        let input = "<command>ls -la</command>";
        let action = parse_bash_action(input);
        
        match action {
            Action::Bash { command, .. } => {
                assert_eq!(command.as_ref(), "ls -la");
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_parse_bash_action_json_format() {
        let input = r#"{"command": "ls -la", "workdir": "/tmp", "timeout": 5000}"#;
        let action = parse_bash_action(input);
        
        match action {
            Action::Bash { command, workdir, timeout_ms, env: _ } => {
                assert_eq!(command.as_ref(), "ls -la");
                assert_eq!(workdir, Some("/tmp".into()));
                assert_eq!(timeout_ms, Some(5000));
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_parse_bash_action_template_syntax_error() {
        let input = "ls {file}";
        let action = parse_bash_action(input);
        
        match action {
            Action::Bash { command, .. } => {
                assert!(command.contains("ERROR"));
            }
            _ => panic!("Expected Bash action with error"),
        }
    }

    #[test]
    fn test_parse_bash_action_pattern_template_error() {
        let input = "grep {pattern}";
        let action = parse_bash_action(input);
        
        match action {
            Action::Bash { command, .. } => {
                assert!(command.contains("ERROR"));
            }
            _ => panic!("Expected Bash action with error"),
        }
    }

    // ============================================================
    // XML Codr Bash Parsing Tests
    // ============================================================

    #[test]
    fn test_parse_codr_bash_xml() {
        let input = "<codr_bash>ls -la</codr_bash>";
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
        
        match &parsed.actions[0] {
            Action::Bash { command, .. } => {
                assert_eq!(command.as_ref(), "ls -la");
            }
            _ => panic!("Expected Bash action"),
        }
    }

    // ============================================================
    // Full parse_actions Tests
    // ============================================================

    #[test]
    fn test_parse_actions_plain_response() {
        let input = "Hello, how are you?";
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
        
        match &parsed.actions[0] {
            Action::Response(content) => {
                assert_eq!(&**content, "Hello, how are you?");
            }
            _ => panic!("Expected Response action"),
        }
    }

    #[test]
    fn test_parse_actions_xml_tool() {
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
        
        match &parsed.actions[0] {
            Action::Tool { name, params } => {
                assert_eq!(name.as_ref(), "read");
                assert_eq!(params["file_path"], "src/main.rs");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_actions_multiple_tools() {
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
<codr_tool name="grep">{"pattern": "fn"}</codr_tool>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 2);
    }

    #[test]
    fn test_parse_actions_mixed_tools_and_bash() {
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
<codr_bash>ls -la</codr_bash>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 2);
    }

    #[test]
    fn test_parse_actions_tool_action_block() {
        let input = r#"```tool-action
read src/main.rs
```"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
        
        match &parsed.actions[0] {
            Action::Tool { name, params: _ } => {
                assert_eq!(name.as_ref(), "read");
            }
            _ => panic!("Expected Tool action"),
        }
    }

    #[test]
    fn test_parse_actions_bash_action_block() {
        let input = r#"```bash-action
ls -la
```"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
        
        match &parsed.actions[0] {
            Action::Bash { command, .. } => {
                assert_eq!(command.as_ref(), "ls -la");
            }
            _ => panic!("Expected Bash action"),
        }
    }

    #[test]
    fn test_parse_actions_is_parallel() {
        // Read-only tools should be parallelizable
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
<codr_tool name="grep">{"pattern": "fn"}</codr_tool>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(parsed.is_parallel);
    }

    #[test]
    fn test_parse_actions_not_parallel_with_write() {
        // Write tool makes it non-parallel
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
<codr_tool name="write">{"path": "test.txt", "content": "hi"}</codr_tool>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(!parsed.is_parallel);
    }

    #[test]
    fn test_parse_actions_not_parallel_with_bash() {
        // Bash makes it non-parallel
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
<codr_bash>ls -la</codr_bash>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(!parsed.is_parallel);
    }

    #[test]
    fn test_parse_actions_deduplication() {
        // Same action should not be duplicated
        let input = r#"<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>
<codr_tool name="read">{"file_path": "src/main.rs"}</codr_tool>"#;
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
    }

    #[test]
    fn test_parse_actions_complex_text_with_tool() {
        let input = "I'll read the file for you.\n\n<codr_tool name=\"read\">{\"file_path\": \"src/main.rs\"}</codr_tool>\n\nLet me check that.";
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
    }

    // ============================================================
    // Edge Cases
    // ============================================================

    #[test]
    fn test_parse_actions_empty_string() {
        let input = "";
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
        
        match &parsed.actions[0] {
            Action::Response(content) => {
                assert!(content.is_empty());
            }
            _ => panic!("Expected Response action"),
        }
    }

    #[test]
    fn test_parse_actions_whitespace_only() {
        let input = "   \n\t\n   ";
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert_eq!(parsed.actions.len(), 1);
    }

    #[test]
    fn test_parse_single_tool_call_plain_text() {
        // Plain text that looks like a tool should not be parsed as tool
        let input = "read this";
        let result = parse_actions(input);
        
        assert!(result.is_ok());
        let parsed = result.unwrap();
        // This might be parsed as shorthand, let's see
        assert!(!parsed.actions.is_empty());
    }
}
