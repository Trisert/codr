// ============================================================
// Tool Parameter Types
// ============================================================
//
// This module defines strongly-typed parameter structs for each tool.
// These derive serde::Serialize/Deserialize for JSON conversion and
// schemars::JsonSchema for automatic JSON Schema generation.
//
// Following pi's design philosophy: minimal but type-safe tool parameters.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============================================================
// Read Tool Parameters
// ============================================================

/// Parameters for the read tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadParams {
    /// Path to the file to read (relative or absolute)
    pub file_path: String,

    /// Line number to start reading from (0-indexed)
    #[serde(default)]
    pub offset: Option<usize>,

    /// Maximum number of lines to read
    #[serde(default)]
    pub limit: Option<usize>,

    /// Alternative: starting line number (1-indexed, user-friendly)
    #[serde(default)]
    pub line_start: Option<usize>,

    /// Alternative: ending line number (1-indexed, user-friendly)
    #[serde(default)]
    pub line_end: Option<usize>,
}

// ============================================================
// Bash Tool Parameters
// ============================================================

/// Parameters for the bash tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BashParams {
    /// Shell command to execute
    pub command: String,

    /// Working directory for the command (defaults to project root)
    #[serde(default)]
    pub cwd: Option<String>,

    /// Timeout in seconds for the command (prevents hanging)
    #[serde(default)]
    pub timeout: Option<u64>,

    /// Additional environment variables (key=value pairs)
    #[serde(default)]
    pub env: Option<Vec<String>>,
}

// ============================================================
// Edit Tool Parameters
// ============================================================

/// Parameters for the edit tool
///
/// Supports two editing modes:
/// 1. String replacement: use old_text and new_text
/// 2. Line-based editing: use line_start, line_end, and new_content
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EditParams {
    /// Path to the file to edit
    pub file_path: String,

    /// String replacement mode: exact text to find and replace
    #[serde(default)]
    pub old_text: Option<String>,

    /// String replacement mode: replacement text
    #[serde(default)]
    pub new_text: Option<String>,

    /// Line-based mode: starting line number (0-indexed)
    #[serde(default)]
    pub line_start: Option<i64>,

    /// Line-based mode: ending line number (0-indexed)
    #[serde(default)]
    pub line_end: Option<i64>,

    /// Line-based mode: new content for the line range
    #[serde(default)]
    pub new_content: Option<String>,
}

// ============================================================
// Write Tool Parameters
// ============================================================

/// Parameters for the write tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WriteParams {
    /// Path to the file to write
    pub file_path: String,

    /// Content to write to the file
    pub content: String,

    /// If true, append to file instead of overwriting
    #[serde(default)]
    pub append: Option<bool>,
}

// ============================================================
// Grep Tool Parameters
// ============================================================

/// Parameters for the grep tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GrepParams {
    /// Regular expression pattern to search for
    pub pattern: String,

    /// Path to search (defaults to current directory)
    #[serde(default)]
    pub path: Option<String>,

    /// Perform case-insensitive search
    #[serde(default)]
    pub case_insensitive: Option<bool>,

    /// File patterns to include (e.g., "*.rs", "*.{js,ts}")
    #[serde(default)]
    pub include: Option<String>,

    /// File patterns to exclude (e.g., "node_modules", "*.min.js")
    #[serde(default)]
    pub exclude: Option<String>,

    /// Number of lines to show before and after each match
    #[serde(default)]
    pub context: Option<usize>,

    /// If true, only return the count of matches instead of lines
    #[serde(default)]
    pub count: Option<bool>,
}

// ============================================================
// Find Tool Parameters
// ============================================================

/// Parameters for the find tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindParams {
    /// Glob pattern to match files (e.g., '*.rs', 'src/**/*.ts')
    pub pattern: String,

    /// Path to search (defaults to current directory)
    #[serde(default)]
    pub path: Option<String>,

    /// Maximum directory depth to search
    #[serde(default)]
    pub depth: Option<usize>,

    /// File patterns to exclude (e.g., "node_modules", "target")
    #[serde(default)]
    pub exclude: Option<String>,
}

// ============================================================
// File Info Tool Parameters
// ============================================================

/// Parameters for the file_info tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileInfoParams {
    /// Path to the file to get metadata for
    pub file_path: String,

    /// Compute and return MD5 hash of file contents
    #[serde(default)]
    pub hash: Option<bool>,

    /// Include permissions in octal format (e.g., "755")
    #[serde(default)]
    pub permissions_octal: Option<bool>,
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test that parameter structs can be serialized/deserialized
    #[test]
    fn test_read_params_serialization() {
        let params = ReadParams {
            file_path: "src/main.rs".to_string(),
            offset: Some(10),
            limit: Some(100),
            line_start: None,
            line_end: None,
        };

        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("file_path"));
        assert!(json.contains("src/main.rs"));

        let deserialized: ReadParams = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.file_path, "src/main.rs");
        assert_eq!(deserialized.offset, Some(10));
    }

    #[test]
    fn test_read_params_default_fields() {
        let json = r#"{"file_path": "test.txt"}"#;
        let params: ReadParams = serde_json::from_str(json).unwrap();

        assert_eq!(params.file_path, "test.txt");
        assert_eq!(params.offset, None);
        assert_eq!(params.limit, None);
    }

    #[test]
    fn test_bash_params_serialization() {
        let params = BashParams {
            command: "ls -la".to_string(),
            cwd: Some("/tmp".to_string()),
            timeout: Some(30),
            env: Some(vec!["FOO=bar".to_string()]),
        };

        let json = serde_json::to_string(&params).unwrap();
        let deserialized: BashParams = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.command, "ls -la");
        assert_eq!(deserialized.cwd, Some("/tmp".to_string()));
        assert_eq!(deserialized.timeout, Some(30));
        assert_eq!(deserialized.env, Some(vec!["FOO=bar".to_string()]));
    }

    #[test]
    fn test_edit_params_string_replacement_mode() {
        let params = EditParams {
            file_path: "test.txt".to_string(),
            old_text: Some("hello".to_string()),
            new_text: Some("world".to_string()),
            line_start: None,
            line_end: None,
            new_content: None,
        };

        let json = serde_json::to_string(&params).unwrap();
        let deserialized: EditParams = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.old_text, Some("hello".to_string()));
        assert_eq!(deserialized.new_text, Some("world".to_string()));
    }

    #[test]
    fn test_edit_params_line_based_mode() {
        let params = EditParams {
            file_path: "test.txt".to_string(),
            old_text: None,
            new_text: None,
            line_start: Some(10),
            line_end: Some(20),
            new_content: Some("new content".to_string()),
        };

        let json = serde_json::to_string(&params).unwrap();
        let deserialized: EditParams = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.line_start, Some(10));
        assert_eq!(deserialized.line_end, Some(20));
        assert_eq!(deserialized.new_content, Some("new content".to_string()));
    }

    #[test]
    fn test_grep_params_optional_fields() {
        let json = r#"{"pattern": "fn main"}"#;
        let params: GrepParams = serde_json::from_str(json).unwrap();

        assert_eq!(params.pattern, "fn main");
        assert_eq!(params.path, None);
        assert_eq!(params.case_insensitive, None);
    }

    #[test]
    fn test_json_schema_generation() {
        // Test that JsonSchema derive works
        let schema = schemars::schema_for!(ReadParams);
        assert!(schema.schema.object.is_some());

        let schema_obj = schema.schema.object.unwrap();
        assert!(schema_obj.properties.contains_key("file_path"));
        assert!(schema_obj.properties.contains_key("offset"));
        assert!(schema_obj.properties.contains_key("limit"));

        // file_path should be required
        assert!(schema_obj.required.contains(&"file_path".to_string()));

        // offset and limit should be optional
        assert!(!schema_obj.required.contains(&"offset".to_string()));
        assert!(!schema_obj.required.contains(&"limit".to_string()));
    }
}
