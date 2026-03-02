use super::context::{
    build_walker, find_project_root, is_binary_file, is_image_file, truncate_file,
};
use super::schema::ExtractParams;
use super::schema::ToolSchema;
use super::{Tool, ToolContext, ToolError, ToolOutput};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

// ============================================================
// Read Tool
// ============================================================

#[allow(dead_code)]
pub struct ReadTool {
    schema: ToolSchema,
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
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

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::FileOps
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        
        // Support both offset/limit and line_start/line_end (aliases from edit tool)
        // line_start/line_end are treated as 1-indexed (user-friendly), convert to 0-indexed
        let line_start = params.get_str("line_start")?.and_then(|s| s.parse::<usize>().ok()).map(|n| n.saturating_sub(1));
        let line_end = params.get_str("line_end")?.and_then(|s| s.parse::<usize>().ok()).map(|n| n.saturating_sub(1));
        
        let offset = params
            .get_str("offset")?
            .or_else(|| line_start.map(|s| s.to_string()))
            .and_then(|s| s.parse::<usize>().ok());
            
        // If line_end is provided with line_start, compute limit from range
        // Otherwise use limit directly
        let limit = if let (Some(start), Some(end)) = (line_start, line_end) {
            Some(end.saturating_sub(start) + 1)
        } else {
            params.get_str("limit")?.and_then(|s| s.parse::<usize>().ok())
        };

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
                file_path, data_len
            ))
            .with_attachment(file_path.clone(), mime_type(&path), data)
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

        // Create display summary for TUI (minimal, clean)
        let display_summary = if let (Some(off), Some(lim)) = (offset, limit) {
            format!("Reading {}:{}-{}", file_path, off + 1, off + lim)
        } else if let Some(off) = offset {
            format!("Reading {}:{}-", file_path, off + 1)
        } else {
            format!("Reading {}", file_path)
        };

        // Full content goes to LLM, but we mark it with metadata for clean TUI display
        let mut output = ToolOutput::text(result.content).with_metadata(super::OutputMetadata {
            file_path: Some(file_path.clone()),
            line_count: Some(result.line_count),
            byte_count: Some(result.byte_count),
            truncated: result.truncated,
            display_summary: Some(display_summary.clone()),
        });

        // For display purposes, override content with just the summary
        output.content_for_display = Some(Arc::new(display_summary));

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

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

impl BashTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("command", "Shell command to execute", true)
            .string(
                "cwd",
                "Working directory for the command (defaults to project root)",
                false,
            );

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

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::System
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let command = params.get_required_str("command")?;

        // Validate command - reject obviously malformed commands
        let trimmed = command.trim();

        // Check for template syntax that shouldn't be executed
        if trimmed.contains("{pattern}")
            || trimmed.contains("{file}")
            || trimmed.contains("{path}")
            || trimmed.contains("{::")
        {
            return Ok(ToolOutput::text(format!(
                "Invalid command: contains template placeholders. The command looks like:\n{}",
                trimmed
            )));
        }

        // Check for incomplete brace expansion
        if trimmed.contains('{') && !trimmed.contains('}') {
            return Ok(ToolOutput::text(format!(
                "Invalid command: contains unmatched braces. The command looks like:\n{}",
                trimmed
            )));
        }

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
            display_summary: None,
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

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("file_path", "Path to the file to edit", true)
            .string("old_text", "Exact text to find and replace (use with new_text)", false)
            .string("new_text", "Replacement text (use with old_text)", false)
            .integer("line_start", "Starting line number for line-based edit (0-indexed)", false)
            .integer("line_end", "Ending line number for line-based edit (0-indexed)", false)
            .string("new_content", "New content for line range (use with line_start/line_end)", false);

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
        "Edit files using two modes: (1) String replacement: use old_text and new_text to replace exact text. \
        (2) Line-based editing: use line_start, line_end, and new_content to replace a line range. \
        Always read the file first to see its contents."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::FileOps
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        let path = ctx.resolve_path(&file_path);

        if !path.exists() {
            return Err(ToolError::PathNotFound(file_path));
        }

        let content = fs::read_to_string(&path)?;

        // Determine which mode to use: line-based or string replacement
        let line_start = params.get_integer("line_start")?;
        let line_end = params.get_integer("line_end")?;
        let new_content = params.get_str("new_content")?;

        // Line-based editing mode
        if line_start.is_some() || line_end.is_some() || new_content.is_some() {
            let start = line_start.unwrap_or(0) as usize;
            let end = line_end.unwrap_or(start as i64) as usize;
            let replacement = new_content.unwrap_or_default();

            let lines: Vec<&str> = content.lines().collect();

            if start >= lines.len() || end >= lines.len() {
                return Ok(ToolOutput::text(format!(
                    "Edit failed: line range {}-{} out of bounds (file has {} lines).",
                    start, end, lines.len()
                )));
            }

            if start > end {
                return Ok(ToolOutput::text(format!(
                    "Edit failed: line_start ({}) must be <= line_end ({})",
                    start, end
                )));
            }

            // Build new content with line range replaced
            let mut new_lines: Vec<String> = lines[..start].iter().map(|s| s.to_string()).collect();
            new_lines.push(replacement.clone());
            new_lines.extend(lines[end + 1..].iter().map(|s| s.to_string()));

            let new_content = new_lines.join("\n");
            fs::write(&path, new_content)?;

            let summary = format!("Edited {} (lines {}-{})", file_path, start, end);
            return Ok(ToolOutput::text(format!(
                "Successfully edited {} (replaced lines {}-{} with {} lines)",
                file_path, start, end, replacement.lines().count()
            ))
            .with_summary_display(summary));
        }

        // String replacement mode (original)
        let old_text = params.get_required_str("old_text")?;
        let new_text = params.get_required_str("new_text")?;

        if !content.contains(&old_text) {
            return Ok(ToolOutput::text(
                "Edit failed: old_text not found in file.\n\
                The specified text does not exist in the file. \
                Use the read tool to check the current file contents."
                    .to_string(),
            ));
        }

        let new_content = content.replace(&old_text, &new_text);
        fs::write(&path, new_content)?;

        let summary = format!("Edited {}", file_path);
        Ok(ToolOutput::text(format!(
            "Successfully edited {}",
            file_path
        ))
        .with_summary_display(summary))
    }
}

// ============================================================
// File Info Tool
// ============================================================

#[allow(dead_code)]
pub struct FileInfoTool {
    schema: ToolSchema,
}

impl Default for FileInfoTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FileInfoTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("file_path", "Path to the file to get metadata for", true);

        Self { schema }
    }
}

impl Tool for FileInfoTool {
    fn name(&self) -> &str {
        "file_info"
    }

    fn label(&self) -> &str {
        "File Metadata"
    }

    fn description(&self) -> &str {
        "Get detailed file metadata including size, permissions, modification time, and type. \
        Useful for understanding file properties without reading the entire content."
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        let path = ctx.resolve_path(&file_path);

        if !path.exists() {
            return Err(ToolError::PathNotFound(file_path));
        }

        let metadata = std::fs::metadata(&path)?;
        let size = metadata.len();
        let _modified = metadata.modified().ok();
        let _perms = metadata.permissions();
        let is_dir = path.is_dir();
        let is_symlink = path.is_symlink();

        // Format output as structured text
        let mut output = format!("File: {}\n", file_path);
        output.push_str(&format!("Type: {}\n",
            if is_dir { "Directory" }
            else if is_symlink { "Symlink" }
            else { "Regular File" }
        ));
        output.push_str(&format!("Size: {} bytes\n", size));
        output.push_str(&format!("Size (human): {}\n",
            if size < 1024 { format!("{} B", size) }
            else if size < 1024 * 1024 { format!("{:.1} KB", size as f64 / 1024.0) }
            else if size < 1024 * 1024 * 1024 { format!("{:.1} MB", size as f64 / (1024.0 * 1024.0)) }
            else { format!("{:.1} GB", size as f64 / (1024.0 * 1024.0 * 1024.0)) }
        ));

        // Format modification time in a simple format
        if let Ok(modified) = path.metadata().and_then(|m| m.modified()) {
            use std::time::UNIX_EPOCH;
            if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                let secs = duration.as_secs();
                // Simple date formatting
                let days_since_epoch = secs / 86400;
                let year = 1970 + (days_since_epoch / 365) as i32;
                let day_of_year = (days_since_epoch % 365) as u32;
                let month = (day_of_year / 30) + 1;
                let day = (day_of_year % 30) + 1;
                let hours = ((secs % 86400) / 3600) as u32;
                let minutes = ((secs % 3600) / 60) as u32;
                output.push_str(&format!("Modified: {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC\n",
                    year, month, day, hours, minutes, secs % 60));
            }
        }

        // Get file extension
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("none");
        output.push_str(&format!("Extension: {}\n", extension));

        // MIME type hint
        let mime_hint = match extension {
            "rs" => "text/rust",
            "py" => "text/python",
            "js" => "text/javascript",
            "ts" => "text/typescript",
            "json" => "application/json",
            "toml" => "text/toml",
            "yaml" | "yml" => "text/yaml",
            "md" => "text/markdown",
            "txt" => "text/plain",
            "html" => "text/html",
            "css" => "text/css",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream"
        };
        output.push_str(&format!("MIME type: {}\n", mime_hint));

        let summary = format!("Info: {} ({} bytes)", file_path, size);
        Ok(ToolOutput::text(output).with_summary_display(summary))
    }

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::FileOps
    }
}

// ============================================================
// Write Tool
// ============================================================

#[allow(dead_code)]
pub struct WriteTool {
    schema: ToolSchema,
}

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
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

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::FileOps
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let file_path = params.get_required_str("file_path")?;
        let content = params.get_required_str("content")?;
        let content_len = content.len();

        let path = ctx.resolve_path(&file_path);

        // Create parent directories if needed
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        fs::write(&path, &content)?;

        let summary = format!("Wrote {} ({} bytes)", file_path, content_len);
        Ok(ToolOutput::text(format!(
            "Successfully wrote {} ({} bytes)",
            file_path, content_len
        ))
        .with_summary_display(summary))
    }
}

// ============================================================
// Grep Tool
// ============================================================

#[allow(dead_code)]
pub struct GrepTool {
    schema: ToolSchema,
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GrepTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string("pattern", "Regular expression pattern to search for", true)
            .string(
                "path",
                "Path to search (defaults to current directory)",
                false,
            )
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

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Search
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let pattern = params.get_required_str("pattern")?;

        // Validate pattern - reject template syntax
        if pattern.contains('{') || pattern.contains('}') || pattern.contains("$(") {
            return Ok(ToolOutput::text(format!(
                "Invalid pattern '{}': contains shell syntax or template placeholders",
                pattern
            )));
        }

        let path = params.get_str("path")?.unwrap_or_else(|| ".".to_string());
        let case_insensitive = params.get_bool("case_insensitive")?.unwrap_or(false);

        let regex = if case_insensitive {
            regex::RegexBuilder::new(&pattern)
                .case_insensitive(true)
                .build()?
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
        })
        .with_metadata(super::OutputMetadata {
            display_summary: Some(format!(
                "Search found {} matches for '{}'",
                matches.len(),
                pattern
            )),
            ..Default::default()
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

impl Default for FindTool {
    fn default() -> Self {
        Self::new()
    }
}

impl FindTool {
    pub fn new() -> Self {
        let schema = ToolSchema::new()
            .string(
                "pattern",
                "Glob pattern to match files (e.g., '*.rs', 'src/**/*.ts')",
                true,
            )
            .string(
                "path",
                "Path to search (defaults to current directory)",
                false,
            );

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

    fn category(&self) -> super::ToolCategory {
        super::ToolCategory::Search
    }

    fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let pattern = params.get_required_str("pattern")?;

        // Validate pattern - reject template syntax or obviously malformed patterns
        if pattern.contains('{') || pattern.contains('}') || pattern.contains("$(") {
            return Ok(ToolOutput::text(format!(
                "Invalid pattern '{}': contains shell syntax or template placeholders",
                pattern
            )));
        }

        // Also reject patterns that look like JSON or incomplete
        if pattern.starts_with('{') || pattern.ends_with('}') {
            return Ok(ToolOutput::text(format!(
                "Invalid pattern '{}': looks like JSON or incomplete syntax",
                pattern
            )));
        }

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
                let content = sorted.join("\n");
                let summary = format!("Glob '{}' ({} found)", pattern, sorted.len());
                Ok(ToolOutput::text(content)
                    .with_metadata(super::OutputMetadata {
                        display_summary: Some(summary.clone()),
                        ..Default::default()
                    })
                    .with_summary_display(summary))
            }
            _ => {
                let summary = format!("glob '{}' (0 found)", pattern);
                Ok(ToolOutput::text("No files found".to_string())
                    .with_metadata(super::OutputMetadata {
                        display_summary: Some(summary.clone()),
                        ..Default::default()
                    })
                    .with_summary_display(summary))
            }
        }
    }
}

fn try_fd(search_path: &Path, pattern: &str) -> Option<Vec<String>> {
    // Convert glob pattern to fd-compatible pattern
    // **/* -> empty (match all)
    // *.rs -> --glob *.rs
    let (use_glob, fd_pattern) = if pattern == "**/*" || pattern == "*" || pattern.is_empty() {
        (false, "")
    } else if pattern.contains('*') {
        (true, pattern)
    } else {
        (false, pattern)
    };
    
    let mut cmd = Command::new("fd");
    cmd.arg("--type").arg("f");
    
    if use_glob {
        cmd.arg("--glob");
    }
    
    if !fd_pattern.is_empty() {
        cmd.arg("--").arg(fd_pattern);
    }
    
    cmd.current_dir(search_path);
    
    let output = cmd.output().ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = stdout
            .lines()
            .filter(|s| !s.is_empty() && *s != ".")
            .map(|s| s.to_string())
            .collect();
        if !files.is_empty() {
            return Some(files);
        }
    }
    None
}

fn try_find(search_path: &Path, pattern: &str) -> Option<Vec<String>> {
    // Convert glob pattern to find-compatible pattern
    let find_pattern = pattern.replace("**/", "").replace("**", "*");

    let output = Command::new("find")
        .args([
            ".",
            "-type",
            "f",
            "!",
            "-path",
            "*/.*",
            "-name",
            &find_pattern,
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

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ============================================================
    // ReadTool Tests
    // ============================================================

    #[test]
    fn test_read_tool_new() {
        let tool = ReadTool::new();
        
        assert_eq!(tool.name(), "read");
        assert_eq!(tool.label(), "Read File");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_read_tool_schema() {
        let tool = ReadTool::new();
        let schema = tool.parameters();
        
        assert!(schema.properties.iter().any(|p| p.name == "file_path"));
    }

    #[test]
    fn test_read_tool_execute_valid_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello, World!").unwrap();
        
        let tool = ReadTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "file_path": "test.txt" }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Hello, World!"));
    }

    #[test]
    fn test_read_tool_execute_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = ReadTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "file_path": "nonexistent.txt" }),
            &ctx,
        );
        
        assert!(result.is_err());
    }

    #[test]
    fn test_read_tool_execute_with_offset() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Line 1\nLine 2\nLine 3").unwrap();
        
        let tool = ReadTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "file_path": "test.txt", "offset": 1 }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Line 2"));
    }

    #[test]
    fn test_read_tool_execute_with_limit() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Line 1\nLine 2\nLine 3").unwrap();
        
        let tool = ReadTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "file_path": "test.txt", "limit": 1 }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Line 1"));
    }

    // ============================================================
    // BashTool Tests
    // ============================================================

    #[test]
    fn test_bash_tool_new() {
        let tool = BashTool::new();
        
        assert_eq!(tool.name(), "bash");
        assert_eq!(tool.label(), "Execute Bash");
    }

    #[test]
    fn test_bash_tool_execute_simple() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = BashTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "command": "echo hello" }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("hello"));
    }

    #[test]
    fn test_bash_tool_execute_invalid_command() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = BashTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "command": "false" }),
            &ctx,
        );
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_bash_tool_missing_command_param() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = BashTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({}),
            &ctx,
        );
        
        assert!(result.is_err());
    }

    // ============================================================
    // WriteTool Tests
    // ============================================================

    #[test]
    fn test_write_tool_new() {
        let tool = WriteTool::new();
        
        assert_eq!(tool.name(), "write");
        assert_eq!(tool.label(), "Write File");
    }

    #[test]
    fn test_write_tool_execute() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = WriteTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({
                "file_path": "newfile.txt",
                "content": "Hello, World!"
            }),
            &ctx,
        );
        
        assert!(result.is_ok());
        
        // Verify file was created
        let file_path = temp_dir.path().join("newfile.txt");
        assert!(file_path.exists());
        
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Hello, World!");
    }

    #[test]
    fn test_write_tool_execute_missing_path() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = WriteTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "content": "Hello" }),
            &ctx,
        );
        
        assert!(result.is_err());
    }

    // ============================================================
    // GrepTool Tests
    // ============================================================

    #[test]
    fn test_grep_tool_new() {
        let tool = GrepTool::new();
        
        assert_eq!(tool.name(), "grep");
        assert_eq!(tool.label(), "Search Contents");
    }

    #[test]
    fn test_grep_tool_execute() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World\nTest Line").unwrap();
        
        let tool = GrepTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "pattern": "Hello", "file_path": "test.txt" }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Hello"));
    }

    #[test]
    fn test_grep_tool_no_matches() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = GrepTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "pattern": "nonexistent", "file_path": "test.txt" }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("No matches found") || output.content.is_empty());
    }

    #[test]
    fn test_grep_tool_missing_params() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = GrepTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({}),
            &ctx,
        );
        
        assert!(result.is_err());
    }

    // ============================================================
    // FindTool Tests
    // ============================================================

    #[test]
    fn test_find_tool_new() {
        let tool = FindTool::new();
        
        assert_eq!(tool.name(), "find");
        assert_eq!(tool.label(), "Find Files");
    }

    #[test]
    fn test_find_tool_execute() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("test1.txt"), "content").unwrap();
        std::fs::write(temp_dir.path().join("test2.txt"), "content").unwrap();
        
        let tool = FindTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "pattern": "*.txt" }),
            &ctx,
        );
        
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("test1.txt"));
        assert!(output.content.contains("test2.txt"));
    }

    #[test]
    fn test_find_tool_no_matches() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = FindTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({ "pattern": "*.nonexistent" }),
            &ctx,
        );
        
        assert!(result.is_ok());
    }

    // ============================================================
    // EditTool Tests
    // ============================================================

    #[test]
    fn test_edit_tool_new() {
        let tool = EditTool::new();
        
        assert_eq!(tool.name(), "edit");
        assert_eq!(tool.label(), "Edit File");
    }

    #[test]
    fn test_edit_tool_execute() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "Hello World").unwrap();
        
        let tool = EditTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({
                "file_path": "test.txt",
                "old_text": "World",
                "new_text": "Rust"
            }),
            &ctx,
        );
        
        assert!(result.is_ok());
        
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Rust"));
    }

    #[test]
    fn test_edit_tool_missing_params() {
        let temp_dir = TempDir::new().unwrap();
        
        let tool = EditTool::new();
        let ctx = ToolContext::new(temp_dir.path().to_path_buf());
        
        let result = tool.execute(
            serde_json::json!({}),
            &ctx,
        );
        
        assert!(result.is_err());
    }

    // ============================================================
    // Helper Function Tests
    // ============================================================

    #[test]
    fn test_mime_type_png() {
        let path = Path::new("test.png");
        assert_eq!(mime_type(path), "image/png");
    }

    #[test]
    fn test_mime_type_jpg() {
        let path = Path::new("test.jpg");
        assert_eq!(mime_type(path), "image/jpeg");
    }

    #[test]
    fn test_mime_type_unknown() {
        let path = Path::new("test.xyz");
        assert_eq!(mime_type(path), "application/octet-stream");
    }

    #[test]
    fn test_mime_type_no_extension() {
        let path = Path::new("testfile");
        assert_eq!(mime_type(path), "application/octet-stream");
    }
}
