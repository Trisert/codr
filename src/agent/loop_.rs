//! Shared agent loop implementation
//!
//! This module provides the core agent loop logic that can be used
//! across different modes (direct, TUI, etc.).

use crate::agent::executor::{ActionExecutor, ExecutionError, MAX_RETRIES};
use crate::error::AgentError;
use crate::model::{Message, Model};
use crate::parser::{Action, clean_message_content, parse_actions};
use crate::tools::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of running the agent loop
#[derive(Debug)]
pub struct LoopResult {
    /// The final response from the agent (if any)
    pub final_response: Option<String>,
    /// The updated conversation history
    pub conversation: Vec<Message>,
    /// Number of tool/bash actions executed
    pub actions_executed: usize,
}

/// Streaming callback type for text content
pub type StreamingCallback = Arc<dyn Fn(Arc<str>) + Send + Sync>;

/// Streaming callback type for thinking content
pub type ThinkingCallback = Arc<dyn Fn(Arc<str>) + Send + Sync>;

/// Configuration for the agent loop
#[derive(Default)]
pub struct LoopConfig {
    /// Enable streaming mode
    pub streaming: bool,
    /// Optional callback for streaming text content
    pub on_streaming: Option<StreamingCallback>,
    /// Optional callback for streaming thinking content
    pub on_thinking: Option<ThinkingCallback>,
    /// Optional cancellation token for interrupting streaming
    pub cancel_token: Option<tokio_util::sync::CancellationToken>,
}

impl LoopConfig {
    /// Create a new non-streaming config
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new streaming config
    pub fn streaming(on_streaming: StreamingCallback, on_thinking: ThinkingCallback) -> Self {
        Self {
            streaming: true,
            on_streaming: Some(on_streaming),
            on_thinking: Some(on_thinking),
            cancel_token: None,
        }
    }

    /// Set the cancellation token
    pub fn with_cancel_token(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }
}

/// Run the agent loop with the given executor
///
/// This is the core agent loop that:
/// 1. Queries the LLM (with optional streaming)
/// 2. Parses the response into actions
/// 3. Executes the actions (with retry logic)
/// 4. Feeds results back to the LLM
/// 5. Repeats until a plain text response is received
///
/// The executor parameter determines how actions are executed and
/// where output is sent (stdout, UI channel, etc.).
///
/// The config parameter controls whether streaming is enabled and
/// provides optional callbacks for real-time updates.
pub async fn run_agent_loop<E: ActionExecutor>(
    model: &Model,
    initial_conversation: Vec<Message>,
    tool_registry: &ToolRegistry,
    mut executor: E,
    role: &crate::tools::Role,
    config: LoopConfig,
) -> Result<LoopResult, String> {
    let mut conversation = initial_conversation;
    let mut actions_executed = 0;

    // Prepare streaming callbacks if enabled
    let on_streaming = config.on_streaming;
    let on_thinking = config.on_thinking;

    loop {
        // Query the LLM (streaming or non-streaming based on config)
        let lm_output = if config.streaming {
            // Streaming mode
            if model.supports_native_tools() {
                let tools_for_role = tool_registry.get_tools_for_role(*role);
                let tools_refs: Vec<&dyn crate::tools::Tool> = tools_for_role;
                let cancel_token = config.cancel_token.clone().unwrap_or_default();
                let on_streaming_cb = on_streaming.clone().unwrap();
                let on_thinking_cb = on_thinking.clone().unwrap();

                model
                    .query_streaming_with_tools(
                        &conversation,
                        &tools_refs,
                        move |chunk| on_streaming_cb(Arc::from(chunk)),
                        move |thinking| on_thinking_cb(Arc::from(thinking)),
                        &cancel_token,
                    )
                    .await
                    .map_err(|e| format!("Query error: {}", e))?
            } else {
                let cancel_token = config.cancel_token.clone().unwrap_or_default();
                let on_streaming_cb = on_streaming.clone().unwrap();
                let on_thinking_cb = on_thinking.clone().unwrap();

                model
                    .query_streaming(
                        &conversation,
                        move |chunk| on_streaming_cb(Arc::from(chunk)),
                        move |thinking| on_thinking_cb(Arc::from(thinking)),
                        &cancel_token,
                    )
                    .await
                    .map_err(|e| format!("Query error: {}", e))?
            }
        } else {
            // Non-streaming mode
            if model.supports_native_tools() {
                let tools_for_role = tool_registry.get_tools_for_role(*role);
                let tools_refs: Vec<&dyn crate::tools::Tool> = tools_for_role;
                model
                    .query_with_tools(&conversation, &tools_refs)
                    .await
                    .map_err(|e| format!("Query error: {}", e))?
            } else {
                model
                    .query(&conversation)
                    .await
                    .map_err(|e| format!("Query error: {}", e))?
            }
        };

        // Parse the response into actions
        let parsed = parse_actions(&lm_output);

        let actions = match parsed {
            Ok(actions) => actions,
            Err(AgentError::Terminating(msg)) => {
                // Handle terminating errors
                let _ = executor.execute_action(&Action::Response(Arc::new((*msg).to_string())));
                return Ok(LoopResult {
                    final_response: None,
                    conversation,
                    actions_executed,
                });
            }
            Err(AgentError::Timeout(msg)) => {
                // Handle timeout errors (continue the loop)
                conversation = model.add_user_message(conversation, &msg);
                continue;
            }
        };

        // Check for plain text response (loop exit condition)
        if let Some(Action::Response(response)) =
            actions.iter().find(|a| matches!(a, Action::Response(_)))
        {
            return Ok(LoopResult {
                final_response: Some((*response).to_string()),
                conversation,
                actions_executed,
            });
        }

        // Execute all tool/bash actions with retry logic
        let result = execute_actions_with_retry(
            &actions,
            &mut executor,
            &lm_output,
            &mut conversation,
            model,
            role,
        );

        match result {
            Ok(count) => {
                actions_executed += count;
            }
            Err(ExecutionError { message, is_fatal }) => {
                if is_fatal {
                    // Fatal error - terminate the loop
                    let _ = executor.execute_action(&Action::Response(Arc::new(message.clone())));
                    return Ok(LoopResult {
                        final_response: None,
                        conversation,
                        actions_executed,
                    });
                } else {
                    // Non-fatal error - feed back to LLM and continue
                    conversation = model.add_user_message(conversation, &message);
                }
            }
        }
    }
}

/// Execute a list of actions with retry logic
///
/// Returns the number of successfully executed actions
fn execute_actions_with_retry<E: ActionExecutor>(
    actions: &[Action],
    executor: &mut E,
    lm_output: &str,
    conversation: &mut Vec<Message>,
    model: &Model,
    role: &crate::tools::Role,
) -> Result<usize, ExecutionError> {
    let mut retry_counts: HashMap<String, usize> = HashMap::new();
    let mut executed_count = 0;

    for action in actions {
        // Skip Response actions (they're handled in the main loop)
        if matches!(action, Action::Response(_)) {
            continue;
        }

        // Check if tool is available in current role
        if let Action::Tool { name, .. } = action
            && !role.tool_available(name.as_ref())
        {
            let error_msg = format!(
                "Tool '{}' is not available in {} mode. Use Shift+Tab to change roles.",
                name,
                role.name()
            );
            let cleaned_output = clean_message_content(lm_output, true);
            *conversation =
                model.add_assistant_message(conversation.clone(), cleaned_output.as_str());
            *conversation = model.add_user_message(conversation.clone(), &error_msg);
            return Err(ExecutionError::retryable(error_msg));
        }

        // Get the action key for tracking retries
        let action_key = get_action_key(action);
        let retry_count = retry_counts.get(&action_key).copied().unwrap_or(0);

        // Execute the action
        match executor.execute_action(action) {
            Ok(output) => {
                // Show output if needed
                if output.show_to_user {
                    let _ = executor.execute_action(&Action::Response(output.llm_content.clone()));
                }

                // Add messages to conversation
                let cleaned_output = clean_message_content(lm_output, true);
                *conversation =
                    model.add_assistant_message(conversation.clone(), cleaned_output.as_str());
                *conversation = model.add_user_message(
                    conversation.clone(),
                    &format!(
                        "Tool result:\n{}",
                        truncate_tool_result(&output.llm_content)
                    ),
                );

                // Reset retry count on success
                retry_counts.insert(action_key, 0);
                executed_count += 1;
            }
            Err(ExecutionError { message, is_fatal }) => {
                if is_fatal {
                    return Err(ExecutionError::fatal(message));
                }

                if retry_count < MAX_RETRIES {
                    // Retry with error feedback
                    retry_counts.insert(action_key, retry_count + 1);

                    let error_json = serde_json::json!({
                        "error": "TOOL_ERROR",
                        "message": message,
                        "retry_count": retry_count + 1,
                        "max_retries": MAX_RETRIES
                    });

                    let feedback = format!(
                        "Error: {}\n\n{}",
                        error_json, "Please fix the parameters and try again."
                    );

                    let cleaned_output = clean_message_content(lm_output, true);
                    *conversation =
                        model.add_assistant_message(conversation.clone(), &cleaned_output);
                    *conversation = model
                        .add_user_message(conversation.clone(), &truncate_tool_result(&feedback));
                } else {
                    // Max retries exceeded
                    let error_msg = format!(
                        "Tool execution failed after {} retries: {}",
                        MAX_RETRIES, message
                    );
                    return Err(ExecutionError::retryable(error_msg));
                }
            }
        }
    }

    Ok(executed_count)
}

/// Run the agent loop with streaming support
///
/// This variant provides real-time streaming of LLM responses through callbacks,
/// useful for TUI mode where progressive display is important.
///
/// This is now a thin wrapper around the unified `run_agent_loop` function.
pub async fn run_agent_loop_streaming<E: ActionExecutor>(
    model: &Model,
    initial_conversation: Vec<Message>,
    tool_registry: &ToolRegistry,
    executor: E,
    role: &crate::tools::Role,
    on_streaming: StreamingCallback,
    on_thinking: ThinkingCallback,
) -> Result<LoopResult, String> {
    let config = LoopConfig::streaming(on_streaming, on_thinking);
    run_agent_loop(
        model,
        initial_conversation,
        tool_registry,
        executor,
        role,
        config,
    )
    .await
}

/// Get a unique key for an action (for tracking retries)
fn get_action_key(action: &Action) -> String {
    match action {
        Action::Bash { command, .. } => (*command).to_string(),
        Action::Tool { name, params } => format!("{}:{}", name, params),
        Action::Response(_) => "response".to_string(),
    }
}

/// Truncate tool output to fit within reasonable context bounds
const MAX_TOOL_RESULT_BYTES: usize = 32_768;

fn truncate_tool_result(content: &str) -> String {
    if content.len() <= MAX_TOOL_RESULT_BYTES {
        return content.to_string();
    }
    // Truncate at a line boundary if possible
    let truncated = &content[..MAX_TOOL_RESULT_BYTES];
    if let Some(last_newline) = truncated.rfind('\n') {
        format!(
            "{}\n\n[...truncated, showing {}/{} bytes]",
            &truncated[..last_newline],
            last_newline,
            content.len()
        )
    } else {
        format!(
            "{}\n\n[...truncated, showing {}/{} bytes]",
            truncated,
            MAX_TOOL_RESULT_BYTES,
            content.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_action_key_bash() {
        let action = Action::Bash {
            command: "ls -la".into(),
            workdir: None,
            timeout_ms: None,
            env: None,
        };
        assert_eq!(get_action_key(&action), "ls -la");
    }

    #[test]
    fn test_get_action_key_tool() {
        let action = Action::Tool {
            name: "read".into(),
            params: serde_json::json!({"file_path": "test.rs"}),
        };
        assert_eq!(get_action_key(&action), "read:{\"file_path\":\"test.rs\"}");
    }

    #[test]
    fn test_get_action_key_response() {
        let action = Action::Response(Arc::new("hello".to_string()));
        assert_eq!(get_action_key(&action), "response");
    }

    #[test]
    fn test_truncate_tool_result_small() {
        let content = "small content";
        assert_eq!(truncate_tool_result(content), content);
    }

    #[test]
    fn test_truncate_tool_result_large() {
        let large_content = "a".repeat(100_000);
        let truncated = truncate_tool_result(&large_content);
        assert!(truncated.len() < large_content.len());
        assert!(truncated.contains("truncated"));
        assert!(truncated.contains(&format!("{}/{} bytes", MAX_TOOL_RESULT_BYTES, 100_000)));
    }

    #[test]
    fn test_truncate_tool_result_at_newline() {
        let content = format!("{}\n{}", "a".repeat(MAX_TOOL_RESULT_BYTES), "extra");
        let truncated = truncate_tool_result(&content);
        assert!(truncated.contains("\n\n[...truncated"));
    }
}
