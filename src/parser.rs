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

impl Action {
    pub fn is_read_only(&self) -> bool {
        match self {
            Action::Tool { name, .. } => matches!(name.as_str(), "read" | "grep" | "find"),
            Action::Bash { .. } => false,
            Action::Response(_) => false,
        }
    }
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

    match serde_json::from_str::<Value>(&params_json) {
        Ok(params) => Some(Action::Tool {
            name: tool_name.to_string(),
            params,
        }),
        Err(_) => Some(Action::Tool {
            name: tool_name.to_string(),
            params: serde_json::json!({ "input": params_json }),
        }),
    }
}

/// Parse OpenAI function calling format: {"name": "read", "arguments": {...}}
fn parse_openai_tool_call(content: &str) -> Option<Action> {
    let value: Value = serde_json::from_str(content).ok()?;

    let name = value.get("name")?.as_str()?.to_string();
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

    let name = value.get("name")?.as_str()?.to_string();
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
            name: tool_name.to_string(),
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
            name: tool_name.to_string(),
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
            command: params["command"].as_str().unwrap_or("").to_string(),
            workdir: params
                .get("workdir")
                .and_then(|v| v.as_str())
                .map(String::from),
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
            ),
            workdir: None,
            timeout_ms: None,
            env: None,
        };
    }

    // Fall back to simple string format
    Action::Bash {
        command: trimmed.to_string(),
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
                    name: potential_tool.to_string(),
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
                        name: tool.to_string(),
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
                        name: tool.to_string(),
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
                        name: tool.to_string(),
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

    // 1. Standard tool-action blocks: ```tool-action\n...\n```
    let tool_re = Regex::new(r"(?s)```\s*tool-action\s*\n(.*?)\n```").unwrap();
    for cap in tool_re.captures_iter(content) {
        if let Some(action) = parse_single_tool_call(cap.get(1).unwrap().as_str()) {
            if !actions.iter().any(|a| action_equals(a, &action)) {
                actions.push(action);
            }
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
            actions: vec![Action::Response(lm_output.to_string())],
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
// Format Error Messages
// ============================================================

#[allow(dead_code)]
pub fn format_available_tools(tools: &str) -> String {
    format!(
        "Available tools:\n{}\n\
        Usage: ```tool-action\n<tool_name>\n<json_params>\n```\n\
        Example: ```tool-action\nread\n{{\"file_path\": \"src/main.rs\"}}\n```\n\
        Also supports OpenAI format: {{\"name\": \"read\", \"arguments\": {{\"file_path\": \"src/main.rs\"}}}}",
        tools
    )
}
