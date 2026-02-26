use super::{Tool, ToolContext, ToolOutput, ToolError};
use super::schema::ToolSchema;
use super::schema::ExtractParams;
use super::context::{truncate_file, build_walker, find_project_root, is_binary_file, is_image_file};
use std::path::Path;
use std::fs;
use std::process::Command;

// ============================================================
// Read Tool
// ============================================================

#[allow(dead_code)]
pub struct ReadTool {
    schema: ToolSchema,
}

impl ReadTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("file_path", "Path to the file to read", true)
            .integer("offset", "Starting line number (0-indexed)", false)
            .integer("limit", "Maximum number of lines to read", false);

        Self { schema }
    }
}

impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn label(&self) -> &str {
        "Read File"
    }

    fn description(&self) -> &str {
        "Read file contents from the filesystem. Supports offset/limit for large files. \
        Detects images and can return them as attachments. Automatically truncates to ~5000 lines or 500KB."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        let offset = params.get_str("offset")?
            .and_then(|s| s.parse::<usize>().ok());
        let limit = params.get_str("limit")?
            .and_then(|s| s.parse::<usize>().ok());

        let path = ctx.resolve_path(&file_path);

        if !path.exists() {
            return Err(ToolError::PathNotFound(file_path));
        }

        // Check if it's an image file
        if is_image_file(&path) {
            let data = fs::read(&path)?;
            let data_len = data.len();
            return Ok(ToolOutput::text(format!(
                "[Image file: {} - {} bytes]",
                file_path,
                data_len
            ))
            .with_attachment(
                file_path.clone(),
                mime_type(&path),
                data,
            )
            .with_metadata(super::OutputMetadata {
                file_path: Some(file_path),
                byte_count: Some(data_len),
                ..Default::default()
            }));
        }

        // Check if it's a binary file
        if is_binary_file(&path) {
            return Ok(ToolOutput::text(format!(
                "[Binary file: {} - cannot display contents]",
                file_path
            )));
        }

        // Read text file
        let max_lines = limit.unwrap_or(ctx.line_limit);
        let result = truncate_file(
            &path,
            ctx.line_limit,
            ctx.token_limit,
            offset,
            Some(max_lines),
        )?;

        let mut output = ToolOutput::text(result.content)
            .with_metadata(super::OutputMetadata {
                file_path: Some(file_path.clone()),
                line_count: Some(result.line_count),
                byte_count: Some(result.byte_count),
                truncated: result.truncated,
            });

        if result.truncated {
            output.content = format!(
                "{}\n\n[File truncated: showing {} of {} lines]",
                output.content,
                output.content.lines().count(),
                result.line_count
            );
        }

        Ok(output)
    }
}

// ============================================================
// Bash Tool
// ============================================================

#[allow(dead_code)]
pub struct BashTool {
    schema: ToolSchema,
}

impl BashTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("command", "Shell command to execute", true)
            .string("cwd", "Working directory for the command (defaults to project root)", false);

        Self { schema }
    }
}

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn label(&self) -> &str {
        "Execute Bash"
    }

    fn description(&self) -> &str {
        "Execute shell commands. Streams output for long-running commands. \
        Returns combined stdout/stderr. Working directory defaults to project root."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let command = params.get_required_str("command")?;
        let cwd_str = params.get_str("cwd")?;
        let cwd = cwd_str
            .map(|p| ctx.resolve_path(&p))
            .unwrap_or_else(|| find_project_root(&ctx.cwd));

        let output = Command::new("bash")
            .arg("-c")
            .arg(&command)
            .current_dir(&cwd)
            .env("PAGER", "cat")
            .env("MANPAGER", "cat")
            .env("LESS", "-R")
            .env("PIP_PROGRESS_BAR", "off")
            .env("TQDM_DISABLE", "1")
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
        let line_count = combined.lines().count();
        let byte_count = combined.len();

        Ok(ToolOutput::text(combined).with_metadata(super::OutputMetadata {
            file_path: None,
            line_count: Some(line_count),
            byte_count: Some(byte_count),
            truncated: false,
        }))
    }
}

// ============================================================
// Edit Tool
// ============================================================

#[allow(dead_code)]
pub struct EditTool {
    schema: ToolSchema,
}

impl EditTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("file_path", "Path to the file to edit", true)
            .string("old_text", "Exact text to find and replace", true)
            .string("new_text", "Replacement text", true);

        Self { schema }
    }
}

impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn label(&self) -> &str {
        "Edit File"
    }

    fn description(&self) -> &str {
        "Make surgical edits to files by finding exact text and replacing it. \
        The old_text must match exactly. Use the read tool first to see file contents."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        let old_text = params.get_required_str("old_text")?;
        let new_text = params.get_required_str("new_text")?;

        let path = ctx.resolve_path(&file_path);

        if !path.exists() {
            return Err(ToolError::PathNotFound(file_path));
        }

        let content = fs::read_to_string(&path)?;

        if !content.contains(&old_text) {
            return Ok(ToolOutput::text("Edit failed: old_text not found in file.\n\
                The specified text does not exist in the file. \
                Use the read tool to check the current file contents.".to_string()));
        }

        let new_content = content.replace(&old_text, &new_text);
        fs::write(&path, new_content)?;

        Ok(ToolOutput::text(format!(
            "Successfully edited {}",
            file_path
        )))
    }
}

// ============================================================
// Write Tool
// ============================================================

#[allow(dead_code)]
pub struct WriteTool {
    schema: ToolSchema,
}

impl WriteTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("file_path", "Path to the file to write", true)
            .string("content", "Content to write to the file", true);

        Self { schema }
    }
}

impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn label(&self) -> &str {
        "Write File"
    }

    fn description(&self) -> &str {
        "Create new files or overwrite existing ones. \
        Creates parent directories if they don't exist. \
        Use read before edit to preserve existing content."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        let content = params.get_required_str("content")?;
        let content_len = content.len();

        let path = ctx.resolve_path(&file_path);

        // Create parent directories if needed
        if let Some(parent) = path.parent()
            && !parent.exists() {
                fs::create_dir_all(parent)?;
            }

        fs::write(&path, &content)?;

        Ok(ToolOutput::text(format!(
            "Successfully wrote {} ({} bytes)",
            file_path,
            content_len
        )))
    }
}

// ============================================================
// Grep Tool
// ============================================================

#[allow(dead_code)]
pub struct GrepTool {
    schema: ToolSchema,
}

impl GrepTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("pattern", "Regular expression pattern to search for", true)
            .string("path", "Path to search (defaults to current directory)", false)
            .boolean("case_insensitive", "Perform case-insensitive search", false);

        Self { schema }
    }
}

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn label(&self) -> &str {
        "Search Contents"
    }

    fn description(&self) -> &str {
        "Search file contents using regex. Respects .gitignore. \
        Returns matching lines with file paths and line numbers."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = params.get_required_str("pattern")?;
        let path = params.get_str("path")?.unwrap_or_else(|| ".".to_string());
        let case_insensitive = params.get_bool("case_insensitive")?.unwrap_or(false);

        let regex = if case_insensitive {
            regex::RegexBuilder::new(&pattern).case_insensitive(true).build()?
        } else {
            regex::Regex::new(&pattern)?
        };

        let mut matches = Vec::new();

        for entry in build_walker(&ctx.cwd, &path) {
            let entry = entry?;
            let entry_path = entry.path();

            if entry_path.is_file() && !is_binary_file(entry_path) {
                let content = fs::read_to_string(entry_path)?;
                let file_name = entry_path
                    .strip_prefix(&ctx.cwd)
                    .unwrap_or(entry_path)
                    .to_string_lossy();

                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        matches.push(format!("{}:{}:{}", file_name, line_num + 1, line));
                    }
                }
            }
        }

        Ok(ToolOutput::text(if matches.is_empty() {
            "No matches found".to_string()
        } else {
            matches.join("\n")
        }))
    }
}

// ============================================================
// Find Tool
// ============================================================

#[allow(dead_code)]
pub struct FindTool {
    schema: ToolSchema,
}

impl FindTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("pattern", "Glob pattern to match files (e.g., '*.rs', 'src/**/*.ts')", true)
            .string("path", "Path to search (defaults to current directory)", false);

        Self { schema }
    }
}

impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn label(&self) -> &str {
        "Find Files"
    }

    fn description(&self) -> &str {
        "Find files by glob pattern. Respects .gitignore. \
        Supports patterns like '*.rs', 'src/**/*.ts', '**/Cargo.toml'"
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(&self, params: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = params.get_required_str("pattern")?;
        let path = params.get_str("path")?.unwrap_or_else(|| ".".to_string());

        let search_path = ctx.resolve_path(&path);

        // Try fd first (preferred), then find with gitignore, then glob fallback
        let result = try_fd(&search_path, &pattern)
            .or_else(|| try_find(&search_path, &pattern))
            .or_else(|| try_glob(&search_path, &pattern, ctx));

        match result {
            Some(matches) if !matches.is_empty() => {
                let mut sorted = matches;
                sorted.sort();
                Ok(ToolOutput::text(sorted.join("\n")))
            }
            _ => Ok(ToolOutput::text("No files found".to_string())),
        }
    }
}

fn try_fd(search_path: &Path, pattern: &str) -> Option<Vec<String>> {
    let output = Command::new("fd")
        .args(["--type", "f", "--", pattern])
        .current_dir(search_path)
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
        if !files.is_empty() {
            return Some(files);
        }
    }
    None
}

fn try_find(search_path: &Path, pattern: &str) -> Option<Vec<String>> {
    // Convert glob pattern to find-compatible pattern
    let find_pattern = pattern
        .replace("**/", "")
        .replace("**", "*")
        .replace("?", "[^/]");

    let output = Command::new("find")
        .args([
            ".",
            "-type", "f",
            "-path", "*/.*",
            "-prune",
            "-o",
            "-name", &find_pattern,
            "-print"
        ])
        .current_dir(search_path)
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = stdout
            .lines()
            .filter(|s| !s.starts_with("./"))
            .map(|s| s.trim_start_matches("./").to_string())
            .collect();
        if !files.is_empty() {
            return Some(files);
        }
    }
    None
}

fn try_glob(search_path: &Path, pattern: &str, ctx: &ToolContext) -> Option<Vec<String>> {
    let full_pattern = search_path.join(pattern).to_string_lossy().to_string();
    let mut matches = Vec::new();

    for entry in glob::glob(&full_pattern).ok()? {
        match entry {
            Ok(path) if path.is_file() => {
                if let Ok(rel_path) = path.strip_prefix(&ctx.cwd) {
                    matches.push(rel_path.to_string_lossy().to_string());
                }
            }
            Err(e) => {
                eprintln!("Glob error: {:?}", e);
            }
            _ => {}
        }
    }

    if matches.is_empty() {
        None
    } else {
        Some(matches)
    }
}

// ============================================================
// Helpers
// ============================================================

fn mime_type(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("webp") => "image/webp".to_string(),
        Some("bmp") => "image/bmp".to_string(),
        Some("ico") => "image/x-icon".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}
