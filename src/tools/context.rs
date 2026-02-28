use crate::tools::ToolError;
use ignore::WalkBuilder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

// ============================================================
// Truncation Utilities
// ============================================================

pub struct TruncatedFile {
    pub content: String,
    pub line_count: usize,
    pub byte_count: usize,
    pub truncated: bool,
}

#[allow(clippy::result_large_err)]
pub fn truncate_file(
    path: &Path,
    line_limit: usize,
    byte_limit: usize,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<TruncatedFile, ToolError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut total_lines = 0;
    let mut total_bytes = 0;
    let mut truncated = false;

    let start_line = offset.unwrap_or(0);
    let max_lines = limit.unwrap_or(line_limit);

    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        total_lines += 1;

        if i < start_line {
            continue;
        }

        if lines.len() >= max_lines || total_bytes >= byte_limit {
            truncated = true;
            break;
        }

        total_bytes += line.len();
        lines.push(line);
    }

    Ok(TruncatedFile {
        content: lines.join("\n"),
        line_count: total_lines,
        byte_count: total_bytes,
        truncated,
    })
}

// ============================================================
// Path Resolution Utilities
// ============================================================

pub fn resolve_path(cwd: &Path, path: &str) -> std::path::PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

// ============================================================
// .gitignore Handling
// ============================================================

pub fn build_walker(cwd: &Path, path: &str) -> ignore::Walk {
    let full_path = resolve_path(cwd, path);

    // Check if it's a file or directory
    if full_path.is_file() {
        // Single file walk
        WalkBuilder::new(&full_path)
            .hidden(false)
            .parents(false)
            .build()
    } else {
        WalkBuilder::new(&full_path)
            .hidden(false)
            .parents(true) // Check parent directories for .gitignore
            .git_ignore(true)
            .git_global(true)
            .build()
    }
}

// ============================================================
// Binary File Detection
// ============================================================

pub fn is_binary_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(
            ext.as_str(),
            "exe"
                | "dll"
                | "so"
                | "dylib"
                | "bin"
                | "o"
                | "a"
                | "lib"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "ico"
                | "bmp"
                | "mp3"
                | "mp4"
                | "wav"
                | "ogg"
                | "flac"
                | "zip"
                | "tar"
                | "gz"
                | "rar"
                | "7z"
                | "pdf"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
        )
    } else {
        false
    }
}

pub fn is_image_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp"
        )
    } else {
        false
    }
}

// ============================================================
// Working Directory Detection
// ============================================================

pub fn find_project_root(start: &Path) -> std::path::PathBuf {
    let mut current = start;

    loop {
        // Check for common project indicators
        let has_git = current.join(".git").exists();
        let has_cargo_toml = current.join("Cargo.toml").exists();
        let has_package_json = current.join("package.json").exists();
        let has_gitignore = current.join(".gitignore").exists();

        if has_git || has_cargo_toml || has_package_json || has_gitignore {
            return current.to_path_buf();
        }

        // Move to parent directory
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent;
            }
            _ => {
                // Reached filesystem root, return original path
                return start.to_path_buf();
            }
        }
    }
}
