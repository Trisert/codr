use crate::error::AgentError;
use regex::Regex;
use serde_json::Value;

// ============================================================
// Action Types
// ============================================================

#[derive(Debug, Clone)]
pub enum Action {
    Bash(String),
    Tool { name: String, params: Value },
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

/// Parse bash-action format: ```bash-action\ncommand\n```
fn parse_bash_action(content: &str) -> Action {
    Action::Bash(content.trim().to_string())
}

pub fn parse_action(lm_output: &str) -> Result<Action, AgentError> {
    // Try tool-action first
    let tool_re = Regex::new(r"```tool-action\s*\n(.*?)\n```").unwrap();
    if let Some(cap) = tool_re.captures(lm_output) {
        let content = cap.get(1).unwrap().as_str();
        if let Some(action) = parse_tool_action(content) {
            return Ok(action);
        }
    }

    // Fall back to bash-action
    let bash_re = Regex::new(r"```bash-action\s*\n(.*?)\n```").unwrap();
    if let Some(cap) = bash_re.captures(lm_output) {
        let content = cap.get(1).unwrap().as_str();
        return Ok(parse_bash_action(content));
    }

    Err(AgentError::FormatError(
        "No action found. Please use one of these formats:\n\
        ```tool-action\n<tool_name>\n<json_params>\n```\n\
        ```bash-action\n<command>\n```".to_string()
    ))
}

// ============================================================
// Format Error Messages
// ============================================================

pub fn format_available_tools(tools: &str) -> String {
    format!(
        "Available tools:\n{}\n\
        Usage: ```tool-action\n<tool_name>\n<json_params>\n```\n\
        Example: ```tool-action\nread\n{{\"file_path\": \"src/main.rs\"}}\n```",
        tools
    )
}
