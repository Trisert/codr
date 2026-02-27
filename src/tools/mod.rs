// ============================================================
// Tool System for codr AI Agent
// ============================================================

pub mod async_handler;
pub mod context;
pub mod r#impl;
pub mod schema;

use schema::{ToolSchema, ValidationError};
use serde_json::Value;

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
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub metadata: OutputMetadata,
    /// Alternative content for TUI display (if set, this is shown instead of content)
    pub content_for_display: Option<String>,
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
            content,
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

    pub fn with_summary_display(mut self, summary: String) -> Self {
        self.content_for_display = Some(summary);
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

    /// Get tool descriptions for AI context
    pub fn descriptions(&self) -> String {
        self.tools
            .iter()
            .map(|t| format!("- {}: {}", t.name(), t.description()))
            .collect::<Vec<_>>()
            .join("\n")
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
        .register(Box::new(r#impl::FindTool::new()));
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
