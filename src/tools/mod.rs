// ============================================================
// Tool System for codr AI Agent
// ============================================================

pub mod async_handler;
pub mod async_wrapper;
pub mod context;
pub mod r#impl;
pub mod params;
pub mod schema;

pub use params::*;
use schema::ValidationError;
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;

// ============================================================
// Role System
// ============================================================

/// Role determines which tools are available and whether they require approval
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Role {
    /// Yolo mode: Full access, all tools auto-approved
    Yolo,
    /// Safe mode: All tools available, write/edit/bash require approval
    #[default]
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
            Self::Yolo => (255, 100, 100),     // Red
            Self::Safe => (100, 255, 100),     // Green
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
            Self::Safe => true, // All tools available, some require approval
            Self::Planning => matches!(tool_name, "read" | "bash" | "grep" | "find" | "file_info"),
        }
    }
}

// ============================================================
// Tool Categories
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    FileOps, // read, write, edit, file_info
    Search,  // grep, find
    System,  // bash
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
// Tool Trait (Object-Safe for Registry)
// ============================================================

/// Associated type for tool parameters
/// All parameter structs must derive Serialize, Deserialize, and JsonSchema
pub trait ToolParams: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static {}

// Blanket implementation for all types that satisfy the bounds
impl<T> ToolParams for T where T: Serialize + DeserializeOwned + JsonSchema + Send + Sync + 'static {}

/// Core tool trait (object-safe, uses Value for compatibility)
#[allow(dead_code)]
pub trait Tool: Send + Sync {
    /// Name of the tool (used for invocation)
    fn name(&self) -> &str;

    /// Display label for the tool
    fn label(&self) -> &str;

    /// Description of what the tool does
    fn description(&self) -> &str;

    /// Get JSON schema for parameters (returns JSON Schema format)
    /// Must be implemented by each tool
    #[allow(clippy::result_large_err)]
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with JSON parameters (validated and converted internally)
    /// Must be implemented by each tool
    #[allow(clippy::result_large_err)]
    fn execute_json(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;

    /// Get the tool category (optional override)
    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
    }
}

/// Helper trait for tools with typed parameters (not object-safe, used internally)
pub trait TypedTool: Tool {
    type Params: ToolParams;

    /// Execute with typed parameters
    #[allow(clippy::result_large_err)]
    fn execute(&self, params: Self::Params, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;

    /// Default implementation bridges JSON to typed
    fn execute_json(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        // Deserialize Value to typed params with detailed error reporting
        let typed_params: Self::Params = serde_json::from_value(params.clone()).map_err(|e| {
            ToolError::InvalidParameters(format!(
                "Invalid parameters for tool '{}': {}\nReceived: {}",
                self.name(),
                e,
                params
            ))
        })?;

        self.execute(typed_params, ctx)
    }

    /// Default implementation generates schema from Params type
    fn parameters_schema(&self) -> Value {
        schema_for::<Self::Params>()
    }
}

/// Helper function to generate JSON schema for a type
fn schema_for<T: JsonSchema>() -> Value {
    let schema = schemars::schema_for!(T);
    serde_json::to_value(schema).unwrap_or_else(|_| {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    })
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
    /// Text content for the LLM (what the model "sees")
    pub content: Arc<String>,

    /// Structured data for UI display (optional)
    /// This follows pi's design - separate content for LLM vs structured data for UI
    pub details: Option<Value>,

    /// Binary attachments (images, etc.)
    pub attachments: Vec<Attachment>,

    /// Metadata about the output
    pub metadata: OutputMetadata,

    /// Alternative content for TUI display (if set, this is shown instead of content)
    pub content_for_display: Option<Arc<String>>, // Shared display content

    /// Tool category for UI styling (FileOps, Search, System)
    pub tool_category: Option<ToolCategory>,
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
            details: None,
            attachments: Vec::new(),
            metadata: OutputMetadata::default(),
            content_for_display: None,
            tool_category: None,
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

    /// Add structured details for UI display (following pi's design)
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Set tool category for UI styling
    pub fn with_tool_category(mut self, category: ToolCategory) -> Self {
        self.tool_category = Some(category);
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
    tool_map: std::collections::HashMap<String, usize>,
    cwd: std::path::PathBuf,
    descriptions_cache: std::sync::RwLock<Option<String>>,
    descriptions_for_role_cache: std::sync::RwLock<std::collections::HashMap<Role, String>>,
}

impl ToolRegistry {
    pub fn new(cwd: std::path::PathBuf) -> Self {
        Self {
            tools: Vec::new(),
            tool_map: std::collections::HashMap::new(),
            cwd,
            descriptions_cache: std::sync::RwLock::new(None),
            descriptions_for_role_cache: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) -> &mut Self {
        let idx = self.tools.len();
        self.tool_map.insert(tool.name().to_string(), idx);
        self.tools.push(tool);
        self.invalidate_cache();
        self
    }

    fn invalidate_cache(&mut self) {
        *self.descriptions_cache.write().unwrap() = None;
        self.descriptions_for_role_cache.write().unwrap().clear();
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tool_map
            .get(name)
            .and_then(|&idx| self.tools.get(idx))
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

    /// Validate and prepare parameters for execution
    /// Returns validated JSON value or detailed validation error
    #[allow(clippy::result_large_err)]
    pub fn validate(&self, name: &str, params: &Value) -> Result<Value, ValidationError> {
        // Check tool exists
        if self.get(name).is_none() {
            return Err(ValidationError::new(
                name,
                &format!("Tool '{}' not found", name),
                Value::Null,
                Value::Null,
            ));
        }

        // Validation happens during execution via serde deserialization
        // This provides detailed error messages automatically
        Ok(params.clone())
    }

    /// Execute a tool by name with JSON parameters
    #[allow(clippy::result_large_err)]
    pub fn execute(&self, name: &str, params: Value) -> Result<ToolOutput, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::Custom(format!("Tool '{}' not found", name)))?;
        let ctx = ToolContext::new(self.cwd.clone());
        tool.execute_json(params, &ctx)
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

    /// Get tool descriptions for AI context, organized by category (cached)
    pub fn descriptions(&self) -> String {
        if let Ok(cache) = self.descriptions_cache.read()
            && let Some(cached) = cache.as_ref()
        {
            return cached.clone();
        }

        let result = self.build_descriptions(None);

        if let Ok(mut cache) = self.descriptions_cache.write() {
            *cache = Some(result.clone());
        }

        result
    }

    /// Get tool descriptions for AI context, filtered by role (cached)
    pub fn descriptions_for_role(&self, role: Role) -> String {
        if let Ok(cache) = self.descriptions_for_role_cache.read()
            && let Some(cached) = cache.get(&role)
        {
            return cached.clone();
        }

        let result = self.build_descriptions(Some(role));

        if let Ok(mut cache) = self.descriptions_for_role_cache.write() {
            cache.insert(role, result.clone());
        }

        result
    }

    fn build_descriptions(&self, role: Option<Role>) -> String {
        let tools: Vec<&dyn Tool> = if let Some(r) = role {
            self.get_tools_for_role(r)
        } else {
            self.list()
        };

        let mut by_category: std::collections::HashMap<ToolCategory, Vec<&str>> =
            std::collections::HashMap::new();

        for tool in &tools {
            by_category
                .entry(tool.category())
                .or_default()
                .push(tool.name());
        }

        let mut result = String::new();

        if let Some(tools) = by_category.get(&ToolCategory::FileOps) {
            result.push_str(&format!("## {}\n", ToolCategory::FileOps.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
            result.push('\n');
        }

        if let Some(tools) = by_category.get(&ToolCategory::Search) {
            result.push_str(&format!("## {}\n", ToolCategory::Search.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
            result.push('\n');
        }

        if let Some(tools) = by_category.get(&ToolCategory::System) {
            result.push_str(&format!("## {}\n", ToolCategory::System.name()));
            for &name in tools {
                if let Some(tool) = self.get(name) {
                    result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
                }
            }
        }

        if let Some(r) = role {
            match r {
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
        }

        result.trim_end().to_string()
    }

    /// Suggest tool based on keywords
    #[allow(dead_code)]
    pub fn suggest_tool(&self, query: &str) -> Option<&dyn Tool> {
        let query_lower = query.to_lowercase();

        // Direct match first
        for tool in &self.tools {
            if tool.name().to_lowercase().contains(&query_lower)
                || query_lower.contains(tool.name())
            {
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
                if query_lower.contains(kw)
                    && let Some(tool) = self.get(tool_name)
                {
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
    use crate::tools::r#impl::{BashTool, ReadTool};

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
        let ctx = ToolContext::new(std::path::PathBuf::from("/tmp")).with_limits(1000, 100);

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
        let output = ToolOutput::text("Image content".to_string()).with_attachment(
            "test.png".to_string(),
            "image/png".to_string(),
            vec![1, 2, 3],
        );

        assert_eq!(output.attachments.len(), 1);
        assert_eq!(output.attachments[0].name, "test.png");
        assert_eq!(output.attachments[0].content_type, "image/png");
        assert_eq!(output.attachments[0].data, vec![1, 2, 3]);
    }

    #[test]
    fn test_tool_output_with_metadata() {
        let output = ToolOutput::text("Content".to_string()).with_metadata(OutputMetadata {
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
        let output = ToolOutput::text("Hello".to_string()).with_metadata(OutputMetadata {
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
        let err = ToolError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
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

        let result = registry.execute(
            "read",
            serde_json::json!({
                "file_path": "/nonexistent/file.txt"
            }),
        );

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
