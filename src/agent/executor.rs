//! Action execution with retry logic
//!
//! This module provides the `ActionExecutor` trait and implementations
//! for executing actions (tool calls and bash commands) in different contexts.

use crate::error::AgentError;
use crate::parser::Action;
use crate::tools::ToolRegistry;
use std::sync::Arc;

/// Maximum number of retries for failed tool executions
pub const MAX_RETRIES: usize = 3;

/// Result of executing an action
#[derive(Debug, Clone)]
pub struct ActionOutput {
    /// The content result (for LLM consumption)
    pub llm_content: Arc<String>,
    /// Optional UI-friendly summary (for TUI display)
    pub ui_summary: Option<Arc<String>>,
    /// Whether to show this output to the user
    pub show_to_user: bool,
}

impl ActionOutput {
    /// Create a new action output with the given content
    pub fn new(content: String) -> Self {
        Self {
            llm_content: Arc::new(content),
            ui_summary: None,
            show_to_user: false,
        }
    }

    /// Create a new action output that should be shown to the user
    pub fn visible(content: String) -> Self {
        Self {
            llm_content: Arc::new(content.clone()),
            ui_summary: Some(Arc::new(content)),
            show_to_user: true,
        }
    }

    /// Create a new action output with separate LLM and UI content
    pub fn with_summary(llm_content: String, ui_summary: Option<String>) -> Self {
        Self {
            llm_content: Arc::new(llm_content),
            ui_summary: ui_summary.map(Arc::new),
            show_to_user: false,
        }
    }
}

/// Error from action execution
#[derive(Debug, Clone)]
pub struct ExecutionError {
    /// The error message
    pub message: String,
    /// Whether this is a fatal error that should terminate the loop
    pub is_fatal: bool,
}

impl ExecutionError {
    /// Create a non-fatal execution error (can be retried)
    pub fn retryable(message: String) -> Self {
        Self {
            message,
            is_fatal: false,
        }
    }

    /// Create a fatal execution error (will terminate the loop)
    pub fn fatal(message: String) -> Self {
        Self {
            message,
            is_fatal: true,
        }
    }

    /// Convert from AgentError
    pub fn from_agent_error(err: AgentError) -> Self {
        match err {
            AgentError::Terminating(msg) => Self::fatal(msg),
            AgentError::Timeout(msg) => Self::retryable((*msg).to_string()),
        }
    }
}

/// Trait for executing actions in different contexts
///
/// Implementations can vary how they handle output (stdout, channels, etc.)
/// while maintaining consistent execution and retry logic.
pub trait ActionExecutor {
    /// Execute a single action and return the result
    fn execute_action(&self, action: &Action) -> Result<ActionOutput, ExecutionError>;
}

/// Direct executor for command-line mode (writes to stdout)
pub struct DirectExecutor {
    tool_registry: Arc<ToolRegistry>,
}

impl DirectExecutor {
    pub fn new(tool_registry: Arc<ToolRegistry>) -> Self {
        Self { tool_registry }
    }
}

impl ActionExecutor for DirectExecutor {
    fn execute_action(&self, action: &Action) -> Result<ActionOutput, ExecutionError> {
        match action {
            Action::Bash { command, .. } => {
                execute_bash(command.as_ref()).map(ActionOutput::visible)
                    .map_err(|e| ExecutionError::from_agent_error(e))
            }
            Action::Tool { name, params } => {
                self.tool_registry
                    .execute(name.as_ref(), params.clone())
                    .map(|o| {
                        let content = (*o.content).to_string();
                        let mut result = ActionOutput::new(content);
                        if !o.attachments.is_empty() {
                            result = ActionOutput::visible(format!(
                                "{}\n[{} attachment(s)]",
                                result.llm_content,
                                o.attachments.len()
                            ));
                        }
                        if let Some(line_count) = o.metadata.line_count {
                            result.llm_content = Arc::new(format!(
                                "{}\n[Lines: {}]",
                                result.llm_content,
                                line_count
                            ));
                        }
                        if o.metadata.truncated {
                            result.llm_content = Arc::new(format!(
                                "{} [truncated]",
                                result.llm_content
                            ));
                        }
                        result
                    })
                    .map_err(|e| ExecutionError::retryable(format!("Tool error: {}", e)))
            }
            Action::Response(_) => {
                // Response actions shouldn't be executed, they're handled separately
                Err(ExecutionError::fatal(
                    "Response action should not be executed".to_string(),
                ))
            }
        }
    }
}

/// Execute a bash command synchronously
fn execute_bash(command: &str) -> Result<String, AgentError> {
    use std::process::Command;

    if command.trim() == "exit" {
        return Err(AgentError::Terminating(
            "Agent requested to exit".to_string(),
        ));
    }

    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("LESS", "-R")
        .env("PIP_PROGRESS_BAR", "off")
        .env("TQDM_DISABLE", "1")
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout).to_string();
            let stderr = String::from_utf8_lossy(&result.stderr).to_string();
            Ok(format!("{}\n{}", stdout, stderr).trim().to_string())
        }
        Err(e) => Err(AgentError::Timeout(format!(
            "Command execution failed: {}",
            e
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_output_new() {
        let output = ActionOutput::new("test content".to_string());
        assert_eq!(*output.llm_content, "test content");
        assert!(output.ui_summary.is_none());
        assert!(!output.show_to_user);
    }

    #[test]
    fn test_action_output_visible() {
        let output = ActionOutput::visible("visible content".to_string());
        assert_eq!(*output.llm_content, "visible content");
        assert!(output.ui_summary.is_some());
        assert!(output.show_to_user);
    }

    #[test]
    fn test_action_output_with_summary() {
        let output = ActionOutput::with_summary(
            "llm content".to_string(),
            Some("ui summary".to_string()),
        );
        assert_eq!(*output.llm_content, "llm content");
        assert_eq!(*output.ui_summary.unwrap(), "ui summary");
        assert!(!output.show_to_user);
    }

    #[test]
    fn test_execution_error_retryable() {
        let err = ExecutionError::retryable("temporary error".to_string());
        assert_eq!(err.message, "temporary error");
        assert!(!err.is_fatal);
    }

    #[test]
    fn test_execution_error_fatal() {
        let err = ExecutionError::fatal("fatal error".to_string());
        assert_eq!(err.message, "fatal error");
        assert!(err.is_fatal);
    }
}
