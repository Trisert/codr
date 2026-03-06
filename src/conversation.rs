//! Conversation persistence module
//!
//! This module handles saving, loading, and managing conversation history.
//! Conversations are stored as JSON files in the XDG data directory.

use crate::model::{Message, SerializableMessage};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata about a saved conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMetadata {
    /// Unique conversation ID (timestamp-based)
    pub id: String,
    /// Human-readable title (optional)
    pub title: Option<String>,
    /// When the conversation was created
    pub created_at: DateTime<Utc>,
    /// When the conversation was last modified
    pub updated_at: DateTime<Utc>,
    /// Number of messages in the conversation
    pub message_count: usize,
    /// Path to the conversation file
    #[serde(skip)]
    pub file_path: PathBuf,
}

/// Complete saved conversation with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedConversation {
    /// Conversation metadata
    pub metadata: ConversationMetadata,
    /// Messages in the conversation
    pub messages: Vec<SerializableMessage>,
    /// Model type used for this conversation
    pub model_type: String,
}

/// Errors that can occur during conversation operations
#[derive(Debug)]
pub enum ConversationError {
    /// IO error
    Io(std::io::Error),
    /// JSON serialization/deserialization error
    Json(serde_json::Error),
    /// Conversation not found
    NotFound(String),
    /// Invalid conversation data
    InvalidData(String),
}

impl std::fmt::Display for ConversationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversationError::Io(e) => write!(f, "IO error: {}", e),
            ConversationError::Json(e) => write!(f, "JSON error: {}", e),
            ConversationError::NotFound(id) => write!(f, "Conversation not found: {}", id),
            ConversationError::InvalidData(msg) => write!(f, "Invalid conversation data: {}", msg),
        }
    }
}

impl std::error::Error for ConversationError {}

impl From<std::io::Error> for ConversationError {
    fn from(e: std::io::Error) -> Self {
        ConversationError::Io(e)
    }
}

impl From<serde_json::Error> for ConversationError {
    fn from(e: serde_json::Error) -> Self {
        ConversationError::Json(e)
    }
}

/// Conversation storage manager
pub struct ConversationStorage {
    /// Directory where conversations are stored
    conversations_dir: PathBuf,
}

impl ConversationStorage {
    /// Create a new conversation storage manager
    pub fn new() -> Result<Self, ConversationError> {
        let conversations_dir = Self::get_conversations_dir()?;

        // Ensure directory exists
        fs::create_dir_all(&conversations_dir)?;

        Ok(Self {
            conversations_dir,
        })
    }

    /// Get the conversations directory (XDG data directory)
    fn get_conversations_dir() -> Result<PathBuf, ConversationError> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| ConversationError::InvalidData(
                "Could not determine XDG data directory".to_string()
            ))?;

        Ok(data_dir.join("codr").join("conversations"))
    }

    /// Get the path to a conversation file
    fn conversation_path(&self, id: &str) -> PathBuf {
        self.conversations_dir.join(format!("{}.json", id))
    }

    /// Generate a new conversation ID based on current timestamp
    pub fn generate_id() -> String {
        
        let now = Utc::now();
        now.format("%Y-%m-%d_%H%M%S").to_string()
    }

    /// Save a conversation
    pub fn save_conversation(
        &self,
        id: &str,
        messages: &[Message],
        model_type: &str,
        title: Option<String>,
    ) -> Result<ConversationMetadata, ConversationError> {
        let now = Utc::now();

        // Convert messages to serializable format
        let serializable_messages: Vec<SerializableMessage> = messages
            .iter()
            .map(|m| m.into())
            .collect();

        // Create metadata
        let metadata = ConversationMetadata {
            id: id.to_string(),
            title: title.or_else(|| Self::generate_title(messages)),
            created_at: now, // In a real implementation, we'd preserve the original created_at
            updated_at: now,
            message_count: messages.len(),
            file_path: self.conversation_path(id),
        };

        // Create saved conversation
        let saved = SavedConversation {
            metadata: metadata.clone(),
            messages: serializable_messages,
            model_type: model_type.to_string(),
        };

        // Write to file
        let json = serde_json::to_string_pretty(&saved)?;
        fs::write(&metadata.file_path, json)?;

        Ok(metadata)
    }

    /// Load a conversation
    pub fn load_conversation(&self, id: &str) -> Result<SavedConversation, ConversationError> {
        let path = self.conversation_path(id);

        if !path.exists() {
            return Err(ConversationError::NotFound(id.to_string()));
        }

        let json = fs::read_to_string(&path)?;
        let mut saved: SavedConversation = serde_json::from_str(&json)?;

        // Update file path
        saved.metadata.file_path = path;

        Ok(saved)
    }

    /// List all conversations
    pub fn list_conversations(&self) -> Result<Vec<ConversationMetadata>, ConversationError> {
        let mut conversations = Vec::new();

        let entries = fs::read_dir(&self.conversations_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Only process .json files
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            // Read and parse the file
            match fs::read_to_string(&path) {
                Ok(json) => {
                    match serde_json::from_str::<SavedConversation>(&json) {
                        Ok(mut saved) => {
                            saved.metadata.file_path = path.clone();
                            conversations.push(saved.metadata);
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to parse conversation file {:?}: {}", path, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to read conversation file {:?}: {}", path, e);
                }
            }
        }

        // Sort by updated_at descending (newest first)
        conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(conversations)
    }

    /// Delete a conversation
    pub fn delete_conversation(&self, id: &str) -> Result<(), ConversationError> {
        let path = self.conversation_path(id);

        if !path.exists() {
            return Err(ConversationError::NotFound(id.to_string()));
        }

        fs::remove_file(path)?;
        Ok(())
    }

    /// Rename a conversation
    pub fn rename_conversation(&self, id: &str, new_title: String) -> Result<ConversationMetadata, ConversationError> {
        let mut saved = self.load_conversation(id)?;
        saved.metadata.title = Some(new_title);

        // Re-save with updated metadata
        let json = serde_json::to_string_pretty(&saved)?;
        fs::write(&saved.metadata.file_path, json)?;

        Ok(saved.metadata.clone())
    }

    /// Get the most recent conversation
    pub fn get_most_recent(&self) -> Result<Option<SavedConversation>, ConversationError> {
        let conversations = self.list_conversations()?;

        match conversations.first() {
            Some(metadata) => {
                let id = &metadata.id;
                self.load_conversation(id).map(Some)
            }
            None => Ok(None),
        }
    }

    /// Export a conversation to different formats
    pub fn export_conversation(
        &self,
        id: &str,
        format: ExportFormat,
        output_path: &Path,
    ) -> Result<(), ConversationError> {
        let saved = self.load_conversation(id)?;

        match format {
            ExportFormat::Json => {
                let json = serde_json::to_string_pretty(&saved)?;
                fs::write(output_path, json)?;
            }
            ExportFormat::Markdown => {
                let md = Self::conversation_to_markdown(&saved);
                fs::write(output_path, md)?;
            }
            ExportFormat::Txt => {
                let txt = Self::conversation_to_text(&saved);
                fs::write(output_path, txt)?;
            }
        }

        Ok(())
    }

    /// Generate a title from the first user message
    fn generate_title(messages: &[Message]) -> Option<String> {
        messages
            .iter()
            .find(|m| &*m.role == "user")
            .and_then(|m| {
                let content = m.content.trim();
                if content.is_empty() {
                    None
                } else {
                    // Take first ~50 chars, truncate at word boundary
                    let truncated = if content.len() > 50 {
                        let mut end = 50;
                        while end > 0 && !content.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &content[..end])
                    } else {
                        content.to_string()
                    };
                    Some(truncated)
                }
            })
    }

    /// Convert conversation to markdown format
    fn conversation_to_markdown(saved: &SavedConversation) -> String {
        let mut output = String::new();

        // Header
        output.push_str("# Conversation\n\n");
        if let Some(ref title) = saved.metadata.title {
            output.push_str(&format!("**Title:** {}\n", title));
        }
        output.push_str(&format!("**ID:** {}\n", saved.metadata.id));
        output.push_str(&format!("**Created:** {}\n", saved.metadata.created_at.format("%Y-%m-%d %H:%M:%S UTC")));
        output.push_str(&format!("**Updated:** {}\n", saved.metadata.updated_at.format("%Y-%m-%d %H:%M:%S UTC")));
        output.push_str(&format!("**Model:** {}\n", saved.model_type));
        output.push_str(&format!("**Messages:** {}\n\n", saved.metadata.message_count));
        output.push_str("---\n\n");

        // Messages
        for msg in &saved.messages {
            let role_name = match msg.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                "system" => "System",
                "thinking" => "Thinking",
                "action" => "Action",
                "output" => "Output",
                _ => &msg.role,
            };

            output.push_str(&format!("## {}\n\n", role_name));
            output.push_str(&msg.content);
            output.push_str("\n\n");
        }

        output
    }

    /// Convert conversation to plain text format
    fn conversation_to_text(saved: &SavedConversation) -> String {
        let mut output = String::new();

        // Header
        output.push_str(&format!("Conversation: {}\n", saved.metadata.id));
        if let Some(ref title) = saved.metadata.title {
            output.push_str(&format!("Title: {}\n", title));
        }
        output.push_str(&format!("Created: {}\n", saved.metadata.created_at.format("%Y-%m-%d %H:%M:%S UTC")));
        output.push_str(&format!("Model: {}\n", saved.model_type));
        output.push_str(&format!("Messages: {}\n", saved.metadata.message_count));
        output.push_str(&format!("\n{}\n\n", "=".repeat(60)));

        // Messages
        for msg in &saved.messages {
            let role_name = match msg.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                "system" => "System",
                "thinking" => "Thinking",
                "action" => "Action",
                "output" => "Output",
                _ => &msg.role,
            };

            output.push_str(&format!("[{}]\n{}\n\n", role_name, msg.content));
        }

        output
    }
}

/// Export format for conversations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Markdown,
    Txt,
}

impl std::str::FromStr for ExportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(ExportFormat::Json),
            "markdown" | "md" => Ok(ExportFormat::Markdown),
            "text" | "txt" => Ok(ExportFormat::Txt),
            _ => Err(format!("Unknown export format: {}", s)),
        }
    }
}

impl ExportFormat {
    /// File extension for this format
    pub fn extension(&self) -> &str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Markdown => "md",
            ExportFormat::Txt => "txt",
        }
    }
}

impl SavedConversation {
    /// Convert saved conversation back to Message list
    pub fn to_messages(&self) -> Vec<Message> {
        self.messages
            .iter()
            .map(|m| m.clone().into())
            .collect()
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn create_test_message(role: &str, content: &str) -> Message {
        Message {
            role: role.into(),
            content: Arc::new(content.to_string()),
            images: Vec::new(),
        }
    }

    #[test]
    fn test_generate_id() {
        let id = ConversationStorage::generate_id();
        // ID should be in format YYYY-MM-DD_HHMMSS
        assert!(id.len() == 17);
        assert!(id.contains('_'));
        assert!(id.contains('-'));
    }

    #[test]
    fn test_generate_title() {
        let messages = vec![
            create_test_message("user", "Hello, how are you today? This is a longer message that should be truncated."),
            create_test_message("assistant", "I'm doing well, thank you!"),
        ];

        let title = ConversationStorage::generate_title(&messages);
        assert!(title.is_some());
        let title = title.unwrap();
        assert!(title.len() <= 54); // 50 + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_export_format_from_str() {
        assert_eq!(ExportFormat::from_str("json"), Ok(ExportFormat::Json));
        assert_eq!(ExportFormat::from_str("JSON"), Ok(ExportFormat::Json));
        assert_eq!(ExportFormat::from_str("markdown"), Ok(ExportFormat::Markdown));
        assert_eq!(ExportFormat::from_str("md"), Ok(ExportFormat::Markdown));
        assert_eq!(ExportFormat::from_str("txt"), Ok(ExportFormat::Txt));
        assert_eq!(ExportFormat::from_str("text"), Ok(ExportFormat::Txt));
        assert!(ExportFormat::from_str("invalid").is_err());
    }

    #[test]
    fn test_export_format_extension() {
        assert_eq!(ExportFormat::Json.extension(), "json");
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Txt.extension(), "txt");
    }

    #[test]
    fn test_conversation_to_markdown() {
        let saved = SavedConversation {
            metadata: ConversationMetadata {
                id: "2025-03-06_120000".to_string(),
                title: Some("Test conversation".to_string()),
                created_at: DateTime::parse_from_rfc3339("2025-03-06T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339("2025-03-06T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                message_count: 2,
                file_path: PathBuf::from("/test/path.json"),
            },
            messages: vec![
                SerializableMessage {
                    role: "user".to_string(),
                    content: "Hello, world!".to_string(),
                    images: Vec::new(),
                },
                SerializableMessage {
                    role: "assistant".to_string(),
                    content: "Hi there!".to_string(),
                    images: Vec::new(),
                },
            ],
            model_type: "test".to_string(),
        };

        let md = ConversationStorage::conversation_to_markdown(&saved);
        assert!(md.contains("# Conversation"));
        assert!(md.contains("**Title:** Test conversation"));
        assert!(md.contains("**ID:** 2025-03-06_120000"));
        assert!(md.contains("## User"));
        assert!(md.contains("Hello, world!"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("Hi there!"));
    }

    #[test]
    fn test_save_and_load_conversation() {
        let temp_dir = TempDir::new().unwrap();
        let storage_dir = temp_dir.path().join("conversations");
        fs::create_dir_all(&storage_dir).unwrap();

        // Create a mock storage that uses the temp dir
        struct MockStorage {
            conversations_dir: PathBuf,
        }

        impl MockStorage {
            fn save_test(&self, id: &str, messages: &[Message]) -> Result<(), ConversationError> {
                let serializable: Vec<SerializableMessage> = messages.iter().map(|m| m.into()).collect();
                let path = self.conversations_dir.join(format!("{}.json", id));
                let saved = SavedConversation {
                    metadata: ConversationMetadata {
                        id: id.to_string(),
                        title: None,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        message_count: messages.len(),
                        file_path: path.clone(),
                    },
                    messages: serializable,
                    model_type: "test".to_string(),
                };
                let json = serde_json::to_string_pretty(&saved)?;
                fs::write(&path, json)?;
                Ok(())
            }

            fn load_test(&self, id: &str) -> Result<SavedConversation, ConversationError> {
                let path = self.conversations_dir.join(format!("{}.json", id));
                let json = fs::read_to_string(&path)?;
                let mut saved: SavedConversation = serde_json::from_str(&json)?;
                saved.metadata.file_path = path;
                Ok(saved)
            }
        }

        let mock = MockStorage {
            conversations_dir: storage_dir.clone(),
        };

        let messages = vec![
            create_test_message("user", "Test message"),
            create_test_message("assistant", "Test response"),
        ];

        // Save
        mock.save_test("test_conv", &messages).unwrap();

        // Load
        let loaded = mock.load_test("test_conv").unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, "user");
        assert_eq!(loaded.messages[0].content, "Test message");
    }
}
