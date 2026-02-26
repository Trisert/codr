use crate::error::AgentError;
use regex::Regex;
use serde_json::Value;

// ============================================================
// Action Types
// ============================================================

#[derive(Debug, Clone)]
pub enum Action {
    Response(String),
    Bash {
        command: String,
        workdir: Option<String>,
        timeout_ms: Option<u64>,
        env: Option<Value>,
    },
    Tool {
        name: String,
        params: Value,
    },
}

// ============================================================
// Parse Action
// ============================================================

/// Parse tool-action format: ```tool-action\ntool_name\njson_params\n```
fn parse_tool_action(content: &str) -> Option<Action> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }

    let tool_name = lines[0].trim();
    let params_json = lines[1..].join("\n");

    // Try to parse as JSON
    match serde_json::from_str::<Value>(&params_json) {
        Ok(params) => Some(Action::Tool {
            name: tool_name.to_string(),
            params,
        }),
        Err(_) => {
            // If JSON parsing fails, treat entire content as params
            Some(Action::Tool {
                name: tool_name.to_string(),
                params: serde_json::json!({ "input": params_json }),
            })
        }
    }
}

/// Parse bash-action format:
/// Simple: ```bash-action\n<command>\n```
/// JSON: ```bash-action\n{"command": "...", "workdir": "...", "timeout": 30000, "env": {...}}\n```
fn parse_bash_action(content: &str) -> Action {
    let mut trimmed = content.trim();

    // Strip <command></command> tokens if present
    if let Some(start) = trimmed.find("<command>")
        && let Some(end) = trimmed.find("</command>") {
            trimmed = trimmed[start + 9..end].trim();
        }

    // Try to parse as JSON first
    if let Ok(params) = serde_json::from_str::<Value>(trimmed)
        && params.get("command").is_some() {
            return Action::Bash {
                command: params["command"].as_str().unwrap_or("").to_string(),
                workdir: params
                    .get("workdir")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                timeout_ms: params.get("timeout").and_then(|v| v.as_u64()),
                env: params.get("env").cloned(),
            };
        }

    // Fall back to simple string format (backward compatibility)
    Action::Bash {
        command: trimmed.to_string(),
        workdir: None,
        timeout_ms: None,
        env: None,
    }
}

pub fn parse_action(lm_output: &str) -> Result<Action, AgentError> {
    // Use (?s) dotall flag so . matches newlines
    // Try tool-action first
    let tool_re = Regex::new(r"(?s)```tool-action\s*\n(.*?)\n```").unwrap();
    if let Some(cap) = tool_re.captures(lm_output) {
        let content = cap.get(1).unwrap().as_str();
        if let Some(action) = parse_tool_action(content) {
            return Ok(action);
        }
    }

    // Fall back to bash-action
    let bash_re = Regex::new(r"(?s)```bash-action\s*\n(.*?)\n```").unwrap();
    if let Some(cap) = bash_re.captures(lm_output) {
        let content = cap.get(1).unwrap().as_str();
        return Ok(parse_bash_action(content));
    }

    // Also try generic fenced blocks with tool-action/bash-action as first line
    // (some models wrap differently)
    let generic_re = Regex::new(r"(?s)```\s*\n\s*tool-action\s*\n(.*?)\n```").unwrap();
    if let Some(cap) = generic_re.captures(lm_output) {
        let content = cap.get(1).unwrap().as_str();
        if let Some(action) = parse_tool_action(content) {
            return Ok(action);
        }
    }

    let generic_bash_re = Regex::new(r"(?s)```\s*\n\s*bash-action\s*\n(.*?)\n```").unwrap();
    if let Some(cap) = generic_bash_re.captures(lm_output) {
        let content = cap.get(1).unwrap().as_str();
        return Ok(parse_bash_action(content));
    }

    // No tool or bash action found - treat as plain text response
    Ok(Action::Response(lm_output.to_string()))
}

// ============================================================
// Format Error Messages
// ============================================================

#[allow(dead_code)]
pub fn format_available_tools(tools: &str) -> String {
    format!(
        "Available tools:\n{}\n\
        Usage: ```tool-action\n<tool_name>\n<json_params>\n```\n\
        Example: ```tool-action\nread\n{{\"file_path\": \"src/main.rs\"}}\n```",
        tools
    )
}
