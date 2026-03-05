// ============================================================
// Async Wrapper for Sync Tools
// ============================================================
//
// This module provides async wrapper implementations that bridge
// the sync Tool trait with the async AsyncToolHandler trait.
// This allows existing tools to work with the async system without
// modification.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use super::async_handler::{AsyncToolHandler, ToolInvocation, ToolKind};
use super::{Tool, ToolContext, ToolError, ToolOutput};
// Import tools from the impl module (r#impl is a raw identifier for the 'impl' keyword)
use super::r#impl::{ReadTool, BashTool, EditTool, WriteTool, GrepTool, FindTool};

// ============================================================
// Async Tool Wrapper
// ============================================================

/// Wraps a sync Tool to implement AsyncToolHandler
pub struct AsyncToolWrapper {
    inner: Box<dyn Tool>,
    kind: ToolKind,
}

impl AsyncToolWrapper {
    pub fn new(tool: Box<dyn Tool>) -> Self {
        let kind = match tool.category() {
            super::ToolCategory::FileOps => {
                if matches!(tool.name(), "read" | "file_info") {
                    ToolKind::Read
                } else {
                    ToolKind::Write
                }
            }
            super::ToolCategory::Search => ToolKind::Read,
            super::ToolCategory::System => ToolKind::Execute,
        };

        Self { inner: tool, kind }
    }
}

#[async_trait]
impl AsyncToolHandler for AsyncToolWrapper {
    fn kind(&self) -> ToolKind {
        self.kind
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters_schema(&self) -> Value {
        self.inner.parameters_schema()
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        matches!(self.kind, ToolKind::Write | ToolKind::Execute | ToolKind::System)
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<ToolOutput, ToolError> {
        // Execute the sync tool directly
        // Note: This will block the async task briefly, which is acceptable for most tools
        // For long-running operations, the tool itself should be async
        let params = invocation.params;
        let ctx = ToolContext::new(invocation.cwd);

        // We need to call the tool's execute_json method
        // Since we can't move self.inner into an async block, we use a different approach:
        // Just execute the tool synchronously - the blocking time is typically short
        // (file reads, parameter validation, etc.)
        let tool_ref = &self.inner;

        // Use tokio::task::block_in_place to bridge sync/async
        // This is safe because we're in an async context and the operation is short-lived
        let result = tokio::task::block_in_place(|| {
            tool_ref.execute_json(params, &ctx)
        });

        result
    }
}

// ============================================================
// Async Tool Registry (Integrated)
// ============================================================

/// Integrated async tool registry that wraps sync tools
pub struct IntegratedAsyncToolRegistry {
    tools: Vec<Arc<dyn AsyncToolHandler>>,
    cwd: PathBuf,
    descriptions_cache: std::sync::RwLock<Option<String>>,
}

impl IntegratedAsyncToolRegistry {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            tools: Vec::new(),
            cwd,
            descriptions_cache: std::sync::RwLock::new(None),
        }
    }

    /// Register a sync tool (wraps it automatically)
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) -> &mut Self {
        let wrapper = Arc::new(AsyncToolWrapper::new(tool)) as Arc<dyn AsyncToolHandler>;
        self.tools.push(wrapper);
        self.invalidate_cache();
        self
    }

    /// Register an async tool directly
    pub fn register_async(&mut self, tool: Arc<dyn AsyncToolHandler>) -> &mut Self {
        self.tools.push(tool);
        self.invalidate_cache();
        self
    }

    fn invalidate_cache(&mut self) {
        *self.descriptions_cache.write().unwrap() = None;
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn AsyncToolHandler>> {
        self.tools.iter().find(|t| t.name() == name)
    }

    pub fn list(&self) -> Vec<&Arc<dyn AsyncToolHandler>> {
        self.tools.iter().collect()
    }

    /// Execute a single tool asynchronously
    pub async fn execute(&self, name: &str, params: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let handler = self
            .get(name)
            .ok_or_else(|| ToolError::Custom(format!("Tool '{}' not found", name)))?;

        let invocation = ToolInvocation::new(name.to_string(), params, self.cwd.clone());
        handler.handle(invocation).await
    }

    /// Execute multiple tools - parallel for read-only, sequential for others
    pub async fn execute_parallel(
        &self,
        actions: Vec<(String, serde_json::Value)>,
    ) -> Vec<Result<ToolOutput, ToolError>> {
        if actions.is_empty() {
            return vec![];
        }

        // Separate read-only and mutating actions with their indices
        let mut read_tasks: Vec<(usize, String, serde_json::Value)> = Vec::new();
        let mut write_tasks: Vec<(usize, String, serde_json::Value)> = Vec::new();

        for (i, (name, params)) in actions.into_iter().enumerate() {
            if let Some(handler) = self.get(&name) {
                let invocation = ToolInvocation::new(name.clone(), params, self.cwd.clone());

                // Check if mutating
                let is_mutating = matches!(
                    handler.kind(),
                    ToolKind::Write | ToolKind::Execute | ToolKind::System
                );

                if is_mutating {
                    write_tasks.push((i, name, invocation.params));
                } else {
                    read_tasks.push((i, name, invocation.params));
                }
            } else {
                write_tasks.push((i, name, params));
            }
        }

        let mut results: Vec<(usize, Result<ToolOutput, ToolError>)> = Vec::new();

        // Execute read-only tools in parallel using futures
        if !read_tasks.is_empty() {
            let read_futures: Vec<_> = read_tasks
                .into_iter()
                .map(|(idx, name, params)| {
                    let registry_cwd = self.cwd.clone();
                    // Clone the handler Arc for each task
                    let handler_opt = self.get(&name).map(|h| std::sync::Arc::clone(h));
                    async move {
                        if let Some(handler) = handler_opt {
                            let invocation = ToolInvocation::new(name, params, registry_cwd);
                            let result = handler.handle(invocation).await;
                            (idx, result)
                        } else {
                            (idx, Err(ToolError::Custom("Tool not found".to_string())))
                        }
                    }
                })
                .collect();

            let read_results = futures::future::join_all(read_futures).await;
            results.extend(read_results);
        }

        // Execute mutating tools sequentially
        for (idx, name, params) in write_tasks {
            let result = self.execute(&name, params).await;
            results.push((idx, result));
        }

        // Sort by index and extract results
        results.sort_by_key(|(idx, _)| *idx);
        results.into_iter().map(|(_, r)| r).collect()
    }

    /// Get tool descriptions for AI context (cached)
    pub fn descriptions(&self) -> String {
        if let Ok(cache) = self.descriptions_cache.read()
            && let Some(cached) = cache.as_ref()
        {
            return cached.clone();
        }

        let result = self
            .tools
            .iter()
            .map(|h| format!("- {}: {}", h.name(), h.description()))
            .collect::<Vec<_>>()
            .join("\n");

        if let Ok(mut cache) = self.descriptions_cache.write() {
            *cache = Some(result.clone());
        }

        result
    }

    /// Get tools available for a specific role
    pub fn get_tools_for_role(&self, role: super::Role) -> Vec<Arc<dyn AsyncToolHandler>> {
        self.tools
            .iter()
            .filter(|t| role.tool_available(t.name()))
            .cloned()
            .collect()
    }

    /// Check if a tool requires approval in the given role
    pub fn tool_requires_approval(&self, name: &str, role: super::Role) -> bool {
        role.requires_approval(name)
    }

    /// Stream bash command output in real-time
    pub async fn stream_bash<F>(
        &self,
        command: &str,
        cwd: &PathBuf,
        mut on_output: F,
    ) -> Result<ToolOutput, ToolError>
    where
        F: FnMut(String) + Send + 'static,
    {
        use tokio::process::Command;
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(command)
            .current_dir(cwd)
            .env("PAGER", "cat")
            .env("MANPAGER", "cat")
            .env("LESS", "-R")
            .env("PIP_PROGRESS_BAR", "off")
            .env("TQDM_DISABLE", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn bash: {}", e)))?;

        let stdout = child.stdout.take()
            .ok_or_else(|| ToolError::ExecutionFailed("Failed to capture stdout".to_string()))?;
        let stderr = child.stderr.take()
            .ok_or_else(|| ToolError::ExecutionFailed("Failed to capture stderr".to_string()))?;

        let mut stdout_lines = BufReader::new(stdout).lines();
        let mut stderr_lines = BufReader::new(stderr).lines();

        let mut output = String::new();

        loop {
            tokio::select! {
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let line_str = format!("{}\n", l);
                            on_output(line_str.clone());
                            output.push_str(&line_str);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            output.push_str(&format!("\n[stdout read error: {}]\n", e));
                        }
                    }
                }
                line = stderr_lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            let line_str = format!("{}\n", l);
                            on_output(line_str.clone());
                            output.push_str(&line_str);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            output.push_str(&format!("\n[stderr read error: {}]\n", e));
                        }
                    }
                }
                status = child.wait() => {
                    match status {
                        Ok(exit_status) => {
                            let code = exit_status.code().unwrap_or(-1);
                            if !output.is_empty() && !output.ends_with('\n') {
                                output.push('\n');
                            }
                            if code != 0 {
                                output.push_str(&format!("[exit code: {}]\n", code));
                            }
                        }
                        Err(e) => {
                            output.push_str(&format!("\n[process error: {}]\n", e));
                        }
                    }
                    break;
                }
            }
        }

        let line_count = output.lines().count();
        let byte_count = output.len();

        Ok(ToolOutput::text(output)
            .with_metadata(super::OutputMetadata {
                file_path: None,
                line_count: Some(line_count),
                byte_count: Some(byte_count),
                truncated: false,
                display_summary: None,
            }))
    }
}

impl Default for IntegratedAsyncToolRegistry {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| ".".into()))
    }
}

// ============================================================
// Helper: Create async tool registry with coding tools
// ============================================================

pub fn create_async_coding_tools(cwd: PathBuf) -> IntegratedAsyncToolRegistry {
    let mut registry = IntegratedAsyncToolRegistry::new(cwd);

    // Use the existing tool implementations, wrapped as async
    registry
        .register_tool(Box::new(ReadTool::new()))
        .register_tool(Box::new(BashTool::new()))
        .register_tool(Box::new(EditTool::new()))
        .register_tool(Box::new(WriteTool::new()))
        .register_tool(Box::new(GrepTool::new()))
        .register_tool(Box::new(FindTool::new()))
        .register_tool(Box::new(super::r#impl::FileInfoTool::new()));

    registry
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_async_wrapper_creation() {
        let tool = Box::new(ReadTool::new()) as Box<dyn Tool>;
        let wrapper = AsyncToolWrapper::new(tool);

        assert_eq!(wrapper.name(), "read");
        assert_eq!(wrapper.kind(), ToolKind::Read);
    }

    #[test]
    fn test_async_registry_creation() {
        let registry = create_async_coding_tools(PathBuf::from("/tmp"));

        assert_eq!(registry.tools.len(), 7);
        assert!(registry.get("read").is_some());
        assert!(registry.get("bash").is_some());
    }

    #[test]
    fn test_async_registry_descriptions() {
        let registry = create_async_coding_tools(PathBuf::from("/tmp"));
        let descriptions = registry.descriptions();

        assert!(descriptions.contains("read"));
        assert!(descriptions.contains("bash"));
        assert!(descriptions.contains("edit"));
    }
}
