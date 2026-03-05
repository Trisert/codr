//! TUI-specific action executor
//!
//! This module provides a TUI executor that sends updates through
//! a channel to the UI instead of writing to stdout.
//!
//! Uses shared update types from agent::updates to avoid
//! circular dependencies.

use crate::agent::executor::{ActionExecutor, ActionOutput, ExecutionError};
use crate::agent::updates::TuiUpdate; // Shared update type
use crate::error::AgentError;
use crate::parser::Action;
use crate::tools::ToolRegistry;
use std::sync::Arc;
use tokio::sync::mpsc;

/// TUI executor that sends updates through a channel
pub struct TUIExecutor {
    tool_registry: Arc<ToolRegistry>,
    tx: mpsc::UnboundedSender<TuiUpdate>,
    role: crate::tools::Role,
}

impl TUIExecutor {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
        tx: mpsc::UnboundedSender<TuiUpdate>,
        role: crate::tools::Role,
    ) -> Self {
        Self {
            tool_registry,
            tx,
            role,
        }
    }

    fn send_update(&self, update: TuiUpdate) {
        let _ = self.tx.send(update);
    }
}

impl ActionExecutor for TUIExecutor {
    fn execute_action(&self, action: &Action) -> Result<ActionOutput, ExecutionError> {
        match action {
            Action::Bash { command, .. } => {
                // Check if bash needs approval
                if self.role.requires_approval("bash") {
                    self.send_update(TuiUpdate::ActionMessage(format!("bash: {}", command).into()));
                    self.send_update(TuiUpdate::NeedsApproval {
                        action_type: "bash".into(),
                        content: Arc::new(command.to_string()),
                        is_tool: false,
                    });
                    return Err(ExecutionError::fatal("Approval required".to_string()));
                }

                self.send_update(TuiUpdate::ActionMessage(format!("bash: {}", command).into()));
                self.send_update(TuiUpdate::ToolProgress(
                    format!(
                        "⚙ Running bash: {}...",
                        command.chars().take(60).collect::<String>()
                    )
                    .into(),
                ));

                execute_bash(command.as_ref())
                    .map(|output| {
                        self.send_update(TuiUpdate::OutputMessage(Arc::new(output.clone())));
                        ActionOutput::visible(output)
                    })
                    .map_err(|e| ExecutionError::from_agent_error(e))
            }
            Action::Tool { name, params } => {
                // Check if tool is available in current role
                if !self.role.tool_available(name.as_ref()) {
                    let error_msg = format!(
                        "Tool '{}' is not available in {} mode. Use Shift+Tab to change roles.",
                        name,
                        self.role.name()
                    );
                    self.send_update(TuiUpdate::ErrorMessage(error_msg.clone().into()));
                    return Err(ExecutionError::retryable(error_msg));
                }

                // Check if tool needs approval
                if self.role.requires_approval(name.as_ref()) {
                    let action_msg = match name.as_ref() {
                        "bash" => {
                            if let Some(command) = params.get("command").and_then(|v| v.as_str()) {
                                format!("bash: {}", command)
                            } else {
                                format!("{}: {}", name, params)
                            }
                        }
                        "write" => {
                            if let Some(file_path) = params.get("file_path").and_then(|v| v.as_str()) {
                                let display_path = file_path.strip_prefix('/').unwrap_or(file_path);
                                format!("Writing {}", display_path)
                            } else {
                                format!("{}: {}", name, params)
                            }
                        }
                        "edit" => {
                            if let Some(file_path) = params.get("file_path").and_then(|v| v.as_str()) {
                                let display_path = file_path.strip_prefix('/').unwrap_or(file_path);
                                format!("Editing {}", display_path)
                            } else {
                                format!("{}: {}", name, params)
                            }
                        }
                        _ => format!("{}: {}", name, params),
                    };

                    self.send_update(TuiUpdate::ActionMessage(action_msg.into()));
                    self.send_update(TuiUpdate::NeedsApproval {
                        action_type: name.clone(),
                        content: Arc::new(params.to_string()),
                        is_tool: true,
                    });
                    return Err(ExecutionError::fatal("Approval required".to_string()));
                }

                // Send action message
                let action_msg = match name.as_ref() {
                    "bash" => {
                        if let Some(command) = params.get("command").and_then(|v| v.as_str()) {
                            format!("bash: {}", command)
                        } else {
                            format!("{}: {}", name, params)
                        }
                    }
                    "write" => {
                        if let Some(file_path) = params.get("file_path").and_then(|v| v.as_str()) {
                            let display_path = file_path.strip_prefix('/').unwrap_or(file_path);
                            format!("Writing {}", display_path)
                        } else {
                            format!("{}: {}", name, params)
                        }
                    }
                    "edit" => {
                        if let Some(file_path) = params.get("file_path").and_then(|v| v.as_str()) {
                            let display_path = file_path.strip_prefix('/').unwrap_or(file_path);
                            format!("Editing {}", display_path)
                        } else {
                            format!("{}: {}", name, params)
                        }
                    }
                    _ => format!("{}: {}", name, params),
                };

                self.send_update(TuiUpdate::ActionMessage(action_msg.into()));

                // Send progress message
                let progress = match name.as_ref() {
                    "bash" => params
                        .get("command")
                        .and_then(|v| v.as_str())
                        .map(|c| format!("⚙ Running bash: {}...", c.chars().take(40).collect::<String>()))
                        .unwrap_or_else(|| format!("⚙ Running {}...", name)),
                    "read" => params
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("⚙ Reading {}...", p))
                        .unwrap_or_else(|| format!("⚙ Running {}...", name)),
                    "write" => params
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("⚙ Writing {}...", p))
                        .unwrap_or_else(|| format!("⚙ Running {}...", name)),
                    "edit" => params
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("⚙ Editing {}...", p))
                        .unwrap_or_else(|| format!("⚙ Running {}...", name)),
                    _ => format!("⚙ Running {}...", name),
                };
                self.send_update(TuiUpdate::ToolProgress(progress.into()));

                // Execute tool
                let tool = self
                    .tool_registry
                    .get(name.as_ref())
                    .ok_or_else(|| ExecutionError::retryable(format!("Tool {} not found", name)))?;

                let ctx = crate::tools::ToolContext {
                    cwd: std::env::current_dir().unwrap_or_default(),
                    env: std::env::vars().collect(),
                    token_limit: 500000,
                    line_limit: 5000,
                    max_image_dimension: 2000,
                };

                tool.execute_json(params.clone(), &ctx)
                    .map_err(|e| ExecutionError::retryable(e.to_string()))
                    .map(|output| {
                        // Use cleaner display for read tool (file path instead of full content)
                        if name.as_ref() == "read" {
                            // Use display_summary from metadata if available (e.g., "Reading src/main.rs:10-20")
                            let summary = output.metadata.display_summary.clone().unwrap_or_else(|| {
                                // Fallback to file path from params
                                params.get("file_path")
                                    .and_then(|v| v.as_str())
                                    .map(|p| format!("Reading {}", p))
                                    .unwrap_or_else(|| "Reading file...".to_string())
                            });

                            // Show as progress message (no border box)
                            self.send_update(TuiUpdate::ToolProgress(summary.into()));

                            // Empty output (don't show file content in terminal)
                            self.send_update(TuiUpdate::OutputMessage(Arc::new(String::new())));
                        } else {
                            // For other tools, show full output
                            self.send_update(TuiUpdate::OutputMessage(Arc::new(output.content.to_string())));
                        }

                        // Convert ToolOutput to ActionOutput
                        ActionOutput {
                            llm_content: output.content,
                            ui_summary: output.content_for_display,
                            show_to_user: true,
                        }
                    })
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

/// Execute bash command
fn execute_bash(command: &str) -> Result<String, AgentError> {
    if command.trim() == "exit" {
        return Err(AgentError::Terminating(
            "Agent requested to exit".to_string(),
        ));
    }

    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("LESS", "-R")
        .env("PIP_PROGRESS_BAR", "off")
        .env("TQDM_DISABLE", "1")
        .output()
        .map_err(|e| AgentError::Terminating(format!("Bash execution failed: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let combined = if stderr.is_empty() {
        stdout
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    Ok(combined)
}
