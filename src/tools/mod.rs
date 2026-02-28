// ============================================================
// Tool System for codr AI Agent
// ============================================================

pub mod async_handler;
pub mod context;
pub mod r#impl;
pub mod schema;

use schema::{ToolSchema, ValidationError};
use serde_json::Value;
use std::sync::Arc;

// ============================================================
// Role System
// ============================================================

/// Role determines which tools are available and whether they require approval
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Yolo mode: Full access, all tools auto-approved
    Yolo,
    /// Safe mode: All tools available, write/edit/bash require approval
    Safe,
    /// Planning mode: Read + bash only, no write/edit tools available
    Planning,
}

impl Role {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Yolo => "YOLO",
            Self::Safe => "SAFE",
            Self::Planning => "PLAN",
        }
    }

    pub fn color(&self) -> (u8, u8, u8) {
        match self {
            Self::Yolo => (255, 100, 100),   // Red
            Self::Safe => (100, 255, 100),   // Green
            Self::Planning => (100, 200, 255), // Blue
        }
    }

    /// Check if a tool requires approval in this role
    pub fn requires_approval(&self, tool_name: &str) -> bool {
        match self {
            Self::Yolo => false,
            Self::Safe => matches!(tool_name, "write" | "edit" | "bash"),
            Self::Planning => matches!(tool_name, "write" | "edit" | "bash"),
        }
    }

    /// Check if a tool is available in this role
    pub fn tool_available(&self, tool_name: &str) -> bool {
        match self {
            Self::Yolo => true,
            Self::Safe => true,  // All tools available, some require approval
            Self::Planning => matches!(tool_name, "read" | "bash" | "grep" | "find" | "file_info"),
        }
    }
}

impl Default for Role {
    fn default() -> Self {
        Self::Safe
    }
}

// ============================================================
// Tool Categories
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    FileOps,    // read, write, edit, file_info
    Search,     // grep, find
    System,     // bash
}

impl ToolCategory {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FileOps => "File Operations",
            Self::Search => "Search",
            Self::System => "System",
        }
    }
}

// ============================================================
// Tool Trait
// ============================================================

#[allow(dead_code)]
pub trait Tool: Send + Sync {
    /// Name of the tool (used for invocation)
    fn name(&self) -> &str;

    /// Display label for the tool
    fn label(&self) -> &str;

    /// Description of what the tool does
    fn description(&self) -> &str;

    /// JSON schema for parameters
    fn parameters(&self) -> &ToolSchema;

    /// Execute the tool with given parameters
    #[allow(clippy::result_large_err)]
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;

    /// Get the tool category (optional override)
    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
    }

    /// Validate parameters against schema (optional override)
    #[allow(clippy::result_large_err)]
    fn validate_params(&self, params: &Value) -> Result<Value, ValidationError> {
        Ok(params.clone())
    }
}

// ============================================================
// Tool Context
// ============================================================

#[allow(dead_code)]
pub struct ToolContext {
    /// Current working directory
    pub cwd: std::path::PathBuf,
    /// Environment variables
    pub env: Vec<(String, String)>,
    /// Token limit for truncation (default: 500000)
    pub token_limit: usize,
    /// Line limit for truncation (default: 5000)
    pub line_limit: usize,
    /// Max image dimension (default: 2000)
    pub max_image_dimension: u32,
}

impl ToolContext {
    pub fn new(cwd: std::path::PathBuf) -> Self {
        Self {
            cwd,
            env: std::env::vars().collect(),
            token_limit: 500_000,
            line_limit: 5_000,
            max_image_dimension: 2000,
        }
    }

    #[allow(dead_code)]
    pub fn with_limits(mut self, token_limit: usize, line_limit: usize) -> Self {
        self.token_limit = token_limit;
        self.line_limit = line_limit;
        self
    }

    /// Resolve a path relative to cwd
    pub fn resolve_path(&self, path: &str) -> std::path::PathBuf {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.cwd.join(p)
        }
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| ".".into()))
    }
}

// ============================================================
// Tool Output
// ============================================================

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: Arc<String>,  // Shared content
    pub attachments: Vec<Attachment>,
    pub metadata: OutputMetadata,
    /// Alternative content for TUI display (if set, this is shown instead of content)
    pub content_for_display: Option<Arc<String>>,  // Shared display content
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct OutputMetadata {
    pub file_path: Option<String>,
    pub line_count: Option<usize>,
    pub byte_count: Option<usize>,
    pub truncated: bool,
    /// Brief summary for display (used instead of full content when preferred)
    pub display_summary: Option<String>,
}

impl ToolOutput {
    pub fn text(content: String) -> Self {
        Self {
            content: Arc::new(content),
            attachments: Vec::new(),
            metadata: OutputMetadata::default(),
            content_for_display: None,
        }
    }

    pub fn with_attachment(mut self, name: String, content_type: String, data: Vec<u8>) -> Self {
        self.attachments.push(Attachment {
            name,
            content_type,
            data,
        });
        self
    }

    pub fn with_metadata(mut self, metadata: OutputMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_summary_display<S: Into<Arc<String>>>(mut self, summary: S) -> Self {
        self.content_for_display = Some(summary.into());
        self
    }
}

// ============================================================
// Tool Error
// ============================================================

#[allow(dead_code)]
#[allow(clippy::result_large_err)]
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path not found: {0}")]
    PathNotFound(String),

    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Pattern error: {0}")]
    PatternError(#[from] regex::Error),

    #[error("Glob error: {0}")]
    GlobError(#[from] glob::PatternError),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Ignore error: {0}")]
    IgnoreError(#[from] ignore::Error),

    #[error("Tool-specific error: {0}")]
    Custom(String),

    #[error("Validation error: {0}")]
    ValidationError(#[from] ValidationError),
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        ToolError::Custom(s)
    }
}

impl From<&str> for ToolError {
    fn from(s: &str) -> Self {
        ToolError::Custom(s.to_string())
    }
}

// ============================================================
// Tool Registry
// ============================================================

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
    cwd: std::path::PathBuf,
}

impl ToolRegistry {
    pub fn new(cwd: std::path::PathBuf) -> Self {
        Self {
            tools: Vec::new(),
            cwd,
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) -> &mut Self {
        self.tools.push(tool);
        self
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    /// Check if a tool is available for the given role
    pub fn tool_available(&self, name: &str, role: Role) -> bool {
        if let Some(tool) = self.get(name) {
            role.tool_available(tool.name())
        } else {
            false
        }
    }

    /// Check if a tool requires approval in the given role
    pub fn tool_requires_approval(&self, name: &str, role: Role) -> bool {
        if let Some(tool) = self.get(name) {
            role.requires_approval(tool.name())
        } else {
            false
        }
    }

    /// Get tools available for a specific role
    pub fn get_tools_for_role(&self, role: Role) -> Vec<&dyn Tool> {
        self.tools
            .iter()
            .filter(|t| role.tool_available(t.name()))
            .map(|t| t.as_ref())
            .collect()
    }

    #[allow(dead_code)]
    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.iter().map(|t| t.as_ref()).collect()
    }

    /// Validate parameters against tool schema
    #[allow(clippy::result_large_err)]
    pub fn validate(&self, name: &str, params: &Value) -> Result<Value, ValidationError> {
        let tool = self.get(name).ok_or_else(|| {
            ValidationError::new(
                name,
                &format!("Tool '{}' not found", name),
                Value::Null,
                Value::Null,
            )
        })?;

        let schema = tool.parameters();

        // Check required parameters
        for required_param in &schema.required {
            if params.get(required_param).is_none() {
                return Err(ValidationError::new(
                    name,
                    &format!("Missing required parameter: {}", required_param),
                    Value::Null,
                    Value::String("any".to_string()),
                ));
            }
        }

        // Validate each provided parameter
        for (key, value) in params.as_object().unwrap_or(&serde_json::Map::new()) {
            if let Some(prop) = schema.get_property(key) {
                let type_valid = match &prop.property_type {
                    schema::PropertyType::String => value.is_string() || value.is_null(),
                    schema::PropertyType::Number => {
                        value.is_number() || value.is_string() || value.is_null()
                    }
                    schema::PropertyType::Integer => {
                        value.is_number()
                            || value.is_string()
                            || value.as_bool().is_some()
                            || value.is_null()
                    }
                    schema::PropertyType::Boolean => {
                        value.is_boolean()
                            || value.is_string()
                            || value.is_number()
                            || value.is_null()
                    }
                    schema::PropertyType::Array(_) => value.is_array(),
                    schema::PropertyType::Object => value.is_object(),
                    schema::PropertyType::OneOf(types) => types.iter().any(|t| match t {
                        schema::PropertyType::String => value.is_string(),
                        schema::PropertyType::Number => value.is_number(),
                        schema::PropertyType::Integer => value.is_number(),
                        schema::PropertyType::Boolean => value.is_boolean(),
                        _ => false,
                    }),
                };

                if !type_valid {
                    let expected_type = match &prop.property_type {
                        schema::PropertyType::String => "string",
                        schema::PropertyType::Number => "number",
                        schema::PropertyType::Integer => "integer",
                        schema::PropertyType::Boolean => "boolean",
                        schema::PropertyType::Array(_) => "array",
                        schema::PropertyType::Object => "object",
                        schema::PropertyType::OneOf(_) => "string/number/integer/boolean",
                    };

                    return Err(ValidationError::new(
                        name,
                        &format!(
                            "Parameter '{}' must be {}, got: {}",
                            key, expected_type, value
                        ),
                        value.clone(),
                        Value::String(expected_type.to_string()),
                    ));
                }
            }
        }

        // Use tool's own validation (allows custom coercion)
        tool.validate_params(params)
    }

    #[allow(clippy::result_large_err)]
    pub fn execute(&self, name: &str, params: Value) -> Result<ToolOutput, ToolError> {
        // Validate first
        let validated_params = self.validate(name, &params)?;

        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::Custom(format!("Tool '{}' not found", name)))?;
        let ctx = ToolContext::new(self.cwd.clone());
        tool.execute(validated_params, &ctx)
    }

    /// Execute multiple actions - parallel for read-only, sequential for others
    /// Returns results in the same order as the input actions
    #[allow(dead_code)]
    pub fn execute_parallel(
        &self,
        actions: Vec<(String, Value)>,
    ) -> Vec<Result<ToolOutput, ToolError>> {
        use std::thread;

        if actions.is_empty() {
            return vec![];
        }

        // Separate read-only and write actions with their indices
        let mut read_tasks: Vec<(usize, String, Value)> = Vec::new();
        let mut write_tasks: Vec<(usize, String, Value)> = Vec::new();

        for (i, (name, params)) in actions.into_iter().enumerate() {
            if matches!(name.as_str(), "read" | "grep" | "find") {
                read_tasks.push((i, name, params));
            } else {
                write_tasks.push((i, name, params));
            }
        }

        // Execute read-only tools in parallel using threads
        let mut results: Vec<(usize, Result<ToolOutput, ToolError>)> = Vec::new();

        if !read_tasks.is_empty() {
            let handles: Vec<thread::JoinHandle<(usize, Result<ToolOutput, ToolError>)>> =
                read_tasks
                    .into_iter()
                    .map(|(idx, name, params)| {
                        let cwd = self.cwd.clone();

                        thread::spawn(move || {
                            let registry = ToolRegistry::new(cwd);
                            let result = registry.execute(&name, params);
                            (idx, result)
                        })
                    })
                    .collect();

            for handle in handles {
                if let Ok((idx, result)) = handle.join() {
                    results.push((idx, result));
                }
            }
        }

        // Execute write tools sequentially
        for (idx, name, params) in write_tasks {
            results.push((idx, self.execute(&name, params)));
        }

        // Sort by index and extract results
        results.sort_by_key(|(idx, _)| *idx);
        results.into_iter().map(|(_, r)| r).collect()
    }

    /// Get tool descriptions for AI context, organized by category
    pub fn descriptions(&self) -> String {
        let mut by_category: std::collections::HashMap<ToolCategory, Vec<&str>> =
            std::collections::HashMap::new();

        for tool in &self.tools {
            by_category
                .entry(tool.category())
                .or_insert_with(Vec::new)
                .push(tool.name());
        }

        let mut result = String::new();

        // Add File Operations tools
        if let Some(tools) = by_category.get(&ToolCategory::FileOps) {
            result.push_str(&format!("## {}\n", ToolCategory::FileOps.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
            result.push('\n');
        }

        // Add Search tools
        if let Some(tools) = by_category.get(&ToolCategory::Search) {
            result.push_str(&format!("## {}\n", ToolCategory::Search.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
            result.push('\n');
        }

        // Add System tools
        if let Some(tools) = by_category.get(&ToolCategory::System) {
            result.push_str(&format!("## {}\n", ToolCategory::System.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
        }

        result.trim_end().to_string()
    }

    /// Get tool descriptions for AI context, filtered by role
    pub fn descriptions_for_role(&self, role: Role) -> String {
        let mut by_category: std::collections::HashMap<ToolCategory, Vec<&str>> =
            std::collections::HashMap::new();

        for tool in self.get_tools_for_role(role) {
            by_category
                .entry(tool.category())
                .or_insert_with(Vec::new)
                .push(tool.name());
        }

        let mut result = String::new();

        // Add File Operations tools
        if let Some(tools) = by_category.get(&ToolCategory::FileOps) {
            result.push_str(&format!("## {}\n", ToolCategory::FileOps.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
            result.push('\n');
        }

        // Add Search tools
        if let Some(tools) = by_category.get(&ToolCategory::Search) {
            result.push_str(&format!("## {}\n", ToolCategory::Search.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
            result.push('\n');
        }

        // Add System tools
        if let Some(tools) = by_category.get(&ToolCategory::System) {
            result.push_str(&format!("## {}\n", ToolCategory::System.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
        }

        // Add role-specific note
        match role {
            Role::Planning => {
                result.push_str("\nNote: In Planning mode, write and edit tools are not available. Use bash for read-only operations.\n");
            }
            Role::Safe => {
                result.push_str("\nNote: In Safe mode, write, edit, and bash operations require approval.\n");
            }
            Role::Yolo => {
                result.push_str("\nNote: In YOLO mode, all operations are auto-approved.\n");
            }
        }

        result.trim_end().to_string()
    }

    /// Suggest tool based on keywords
    #[allow(dead_code)]
    pub fn suggest_tool(&self, query: &str) -> Option<&dyn Tool> {
        let q = query.to_lowercase();

        // Direct match first
        for tool in &self.tools {
            if tool.name().to_lowercase().contains(&q) || q.contains(tool.name()) {
                return Some(tool.as_ref());
            }
        }

        // Keyword matching
        let keywords: Vec<(&[&str], &str)> = vec![
            (&["read", "file", "view", "show", "content"], "read"),
            (&["search", "grep", "find", "pattern"], "grep"),
            (&["find", "glob", "files", "search"], "find"),
            (&["edit", "modify", "change", "replace"], "edit"),
            (&["write", "create", "new", "save"], "write"),
            (
                &["run", "execute", "command", "bash", "shell", "terminal"],
                "bash",
            ),
        ];

        for (kws, tool_name) in keywords {
            for kw in kws {
                if q.contains(kw)
                    && let Some(tool) = self.get(tool_name) {
                        return Some(tool);
                    }
            }
        }

        None
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| ".".into()))
    }
}

// ============================================================
// Predefined Tool Sets
// ============================================================

pub fn create_coding_tools(cwd: std::path::PathBuf) -> ToolRegistry {
    let mut registry = ToolRegistry::new(cwd);
    registry
        .register(Box::new(r#impl::ReadTool::new()))
        .register(Box::new(r#impl::BashTool::new()))
        .register(Box::new(r#impl::EditTool::new()))
        .register(Box::new(r#impl::WriteTool::new()))
        .register(Box::new(r#impl::GrepTool::new()))
        .register(Box::new(r#impl::FindTool::new()))
        .register(Box::new(r#impl::FileInfoTool::new()));
    registry
}

#[allow(dead_code)]
pub fn create_read_only_tools(cwd: std::path::PathBuf) -> ToolRegistry {
    let mut registry = ToolRegistry::new(cwd);
    registry
        .register(Box::new(r#impl::ReadTool::new()))
        .register(Box::new(r#impl::GrepTool::new()))
        .register(Box::new(r#impl::FindTool::new()));
    registry
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::r#impl::{ReadTool, BashTool};

    // ============================================================
    // ToolContext Tests
    // ============================================================

    #[test]
    fn test_tool_context_default() {
        let ctx = ToolContext::default();
        
        assert!(ctx.cwd.exists() || ctx.cwd == std::path::PathBuf::from("."));
        assert!(!ctx.env.is_empty());
        assert_eq!(ctx.token_limit, 500_000);
        assert_eq!(ctx.line_limit, 5_000);
        assert_eq!(ctx.max_image_dimension, 2000);
    }

    #[test]
    fn test_tool_context_new() {
        let ctx = ToolContext::new(std::path::PathBuf::from("/tmp"));
        
        assert_eq!(ctx.cwd, std::path::PathBuf::from("/tmp"));
        assert!(!ctx.env.is_empty());
    }

    #[test]
    fn test_tool_context_with_limits() {
        let ctx = ToolContext::new(std::path::PathBuf::from("/tmp"))
            .with_limits(1000, 100);
        
        assert_eq!(ctx.token_limit, 1000);
        assert_eq!(ctx.line_limit, 100);
    }

    #[test]
    fn test_tool_context_resolve_path_absolute() {
        let ctx = ToolContext::new(std::path::PathBuf::from("/tmp"));
        
        let resolved = ctx.resolve_path("/etc/passwd");
        assert_eq!(resolved, std::path::PathBuf::from("/etc/passwd"));
    }

    #[test]
    fn test_tool_context_resolve_path_relative() {
        let ctx = ToolContext::new(std::path::PathBuf::from("/tmp"));
        
        let resolved = ctx.resolve_path("test.txt");
        assert_eq!(resolved, std::path::PathBuf::from("/tmp/test.txt"));
    }

    // ============================================================
    // ToolOutput Tests
    // ============================================================

    #[test]
    fn test_tool_output_text() {
        let output = ToolOutput::text("Hello world".to_string());
        
        assert_eq!(&*output.content, "Hello world");
        assert!(output.attachments.is_empty());
        assert!(output.content_for_display.is_none());
    }

    #[test]
    fn test_tool_output_with_attachment() {
        let output = ToolOutput::text("Image content".to_string())
            .with_attachment("test.png".to_string(), "image/png".to_string(), vec![1, 2, 3]);
        
        assert_eq!(output.attachments.len(), 1);
        assert_eq!(output.attachments[0].name, "test.png");
        assert_eq!(output.attachments[0].content_type, "image/png");
        assert_eq!(output.attachments[0].data, vec![1, 2, 3]);
    }

    #[test]
    fn test_tool_output_with_metadata() {
        let output = ToolOutput::text("Content".to_string())
            .with_metadata(OutputMetadata {
                file_path: Some("test.txt".to_string()),
                line_count: Some(10),
                byte_count: Some(100),
                truncated: false,
                display_summary: None,
            });
        
        assert_eq!(output.metadata.file_path, Some("test.txt".to_string()));
        assert_eq!(output.metadata.line_count, Some(10));
    }

    #[test]
    fn test_tool_output_with_summary_display() {
        let output = ToolOutput::text("Full content...".to_string())
            .with_summary_display("Short summary".to_string());
        
        assert!(output.content_for_display.is_some());
        assert_eq!(&*output.content_for_display.unwrap(), "Short summary");
    }

    #[test]
    fn test_tool_output_clone() {
        let output = ToolOutput::text("Hello".to_string())
            .with_metadata(OutputMetadata {
                file_path: Some("test.txt".to_string()),
                ..Default::default()
            });
        
        let cloned = output.clone();
        
        assert_eq!(&*cloned.content, "Hello");
        assert_eq!(cloned.metadata.file_path, Some("test.txt".to_string()));
    }

    // ============================================================
    // OutputMetadata Tests
    // ============================================================

    #[test]
    fn test_output_metadata_default() {
        let meta = OutputMetadata::default();
        
        assert!(meta.file_path.is_none());
        assert!(meta.line_count.is_none());
        assert!(meta.byte_count.is_none());
        assert!(!meta.truncated);
        assert!(meta.display_summary.is_none());
    }

    // ============================================================
    // Attachment Tests
    // ============================================================

    #[test]
    fn test_attachment_creation() {
        let attachment = Attachment {
            name: "test.png".to_string(),
            content_type: "image/png".to_string(),
            data: vec![1, 2, 3],
        };
        
        assert_eq!(attachment.name, "test.png");
        assert_eq!(attachment.content_type, "image/png");
        assert_eq!(attachment.data, vec![1, 2, 3]);
    }

    // ============================================================
    // ToolError Tests
    // ============================================================

    #[test]
    fn test_tool_error_io() {
        let err = ToolError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_tool_error_path_not_found() {
        let err = ToolError::PathNotFound("/missing/file.txt".to_string());
        assert!(err.to_string().contains("Path not found"));
    }

    #[test]
    fn test_tool_error_invalid_parameters() {
        let err = ToolError::InvalidParameters("Missing required parameter".to_string());
        assert!(err.to_string().contains("Invalid parameters"));
    }

    #[test]
    fn test_tool_error_execution_failed() {
        let err = ToolError::ExecutionFailed("Command failed".to_string());
        assert!(err.to_string().contains("Execution failed"));
    }

    // ============================================================
    // ToolRegistry Tests
    // ============================================================

    #[test]
    fn test_tool_registry_new() {
        let registry = ToolRegistry::new(std::path::PathBuf::from("/tmp"));
        
        assert_eq!(registry.tools.len(), 0);
    }

    #[test]
    fn test_tool_registry_register() {
        let mut registry = ToolRegistry::new(std::path::PathBuf::from("/tmp"));
        registry.register(Box::new(ReadTool::new()));
        
        assert_eq!(registry.tools.len(), 1);
    }

    #[test]
    fn test_tool_registry_get() {
        let mut registry = ToolRegistry::new(std::path::PathBuf::from("/tmp"));
        registry.register(Box::new(ReadTool::new()));
        
        let tool = registry.get("read");
        assert!(tool.is_some());
        
        let tool = registry.get("nonexistent");
        assert!(tool.is_none());
    }

    #[test]
    fn test_tool_registry_list() {
        let mut registry = ToolRegistry::new(std::path::PathBuf::from("/tmp"));
        registry.register(Box::new(ReadTool::new()));
        registry.register(Box::new(BashTool::new()));
        
        let tools = registry.list();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_tool_registry_descriptions() {
        let mut registry = ToolRegistry::new(std::path::PathBuf::from("/tmp"));
        registry.register(Box::new(ReadTool::new()));
        
        let descriptions = registry.descriptions();
        assert!(descriptions.contains("read"));
    }

    #[test]
    fn test_tool_registry_suggest_tool_direct_match() {
        let registry = create_coding_tools(std::path::PathBuf::from("/tmp"));
        
        let suggestion = registry.suggest_tool("read");
        assert!(suggestion.is_some());
    }

    #[test]
    fn test_tool_registry_suggest_tool_keyword() {
        let registry = create_coding_tools(std::path::PathBuf::from("/tmp"));
        
        let suggestion = registry.suggest_tool("view a file");
        assert!(suggestion.is_some());
    }

    #[test]
    fn test_tool_registry_suggest_tool_no_match() {
        let registry = create_coding_tools(std::path::PathBuf::from("/tmp"));
        
        let suggestion = registry.suggest_tool("xyznonexistent");
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_tool_registry_execute() {
        let registry = create_coding_tools(std::path::PathBuf::from("/tmp"));
        
        let result = registry.execute("read", serde_json::json!({
            "file_path": "/nonexistent/file.txt"
        }));
        
        // Should fail because file doesn't exist, but should not panic
        assert!(result.is_err());
    }

    // ============================================================
    // Predefined Tool Sets Tests
    // ============================================================

    #[test]
    fn test_create_coding_tools() {
        let registry = create_coding_tools(std::path::PathBuf::from("/tmp"));

        assert_eq!(registry.tools.len(), 7);
        assert!(registry.get("read").is_some());
        assert!(registry.get("bash").is_some());
        assert!(registry.get("edit").is_some());
        assert!(registry.get("write").is_some());
        assert!(registry.get("grep").is_some());
        assert!(registry.get("find").is_some());
        assert!(registry.get("file_info").is_some());
    }

    #[test]
    fn test_create_read_only_tools() {
        let registry = create_read_only_tools(std::path::PathBuf::from("/tmp"));
        
        assert_eq!(registry.tools.len(), 3);
        assert!(registry.get("read").is_some());
        assert!(registry.get("grep").is_some());
        assert!(registry.get("find").is_some());
        assert!(registry.get("bash").is_none());
        assert!(registry.get("edit").is_none());
        assert!(registry.get("write").is_none());
    }
}
