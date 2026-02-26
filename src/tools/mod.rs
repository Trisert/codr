// ============================================================
// Tool System for codr AI Agent
// ============================================================

pub mod r#impl;
pub mod schema;
pub mod context;

use serde_json::Value;
use schema::ToolSchema;

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
    fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
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
}

impl ToolOutput {
    pub fn text(content: String) -> Self {
        Self {
            content,
            attachments: Vec::new(),
            metadata: OutputMetadata::default(),
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
}

// ============================================================
// Tool Error
// ============================================================

#[allow(dead_code)]
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
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    #[allow(dead_code)]
    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.iter().map(|t| t.as_ref()).collect()
    }

    pub fn execute(&self, name: &str, params: Value) -> Result<ToolOutput, ToolError> {
        let tool = self.get(name).ok_or_else(|| ToolError::Custom(format!("Tool '{}' not found", name)))?;
        let ctx = ToolContext::new(self.cwd.clone());
        tool.execute(params, &ctx)
    }

    /// Get tool descriptions for AI context
    pub fn descriptions(&self) -> String {
        self.tools
            .iter()
            .map(|t| {
                format!(
                    "- {}: {}",
                    t.name(),
                    t.description()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
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

