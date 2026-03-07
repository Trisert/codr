// Context Manager for message pruning and token estimation
use crate::model::Message;
use std::collections::VecDeque;

#[derive(Clone)]
pub struct ContextManager {
    max_tokens: usize,
    current_tokens: usize,
    messages: VecDeque<Message>,
    system_prompt_tokens: usize,
}

impl ContextManager {
    pub fn new(max_tokens: usize, system_prompt: &str) -> Self {
        let system_prompt_tokens = estimate_tokens(system_prompt);
        Self {
            max_tokens,
            current_tokens: system_prompt_tokens,
            messages: VecDeque::new(),
            system_prompt_tokens,
        }
    }

    /// Add message and update token count
    pub fn add_message(&mut self, msg: Message) {
        let tokens = estimate_tokens(&msg.content);
        self.current_tokens += tokens;
        self.messages.push_back(msg);
    }

    /// Get all messages
    pub fn get_messages(&self) -> Vec<Message> {
        self.messages.iter().cloned().collect()
    }

    /// Get current token usage as a percentage
    pub fn token_usage(&self) -> f64 {
        if self.max_tokens == 0 {
            return 0.0;
        }
        (self.current_tokens as f64) / (self.max_tokens as f64)
    }

    /// Prune old messages to fit within token limit
    /// Keeps system prompt and last N messages
    pub fn prune_to_fit(&mut self, reserve: usize) {
        let target = self.max_tokens.saturating_sub(reserve);
        if self.current_tokens <= target {
            return;
        }

        // Always keep last 5 messages for continuity
        let keep_recent = 5;
        let mut tokens_to_remove = self.current_tokens.saturating_sub(target);

        // Remove from front (oldest messages first)
        while !self.messages.is_empty() && self.messages.len() > keep_recent && tokens_to_remove > 0
        {
            if let Some(msg) = self.messages.front() {
                let msg_tokens = estimate_tokens(&msg.content);
                if msg_tokens <= tokens_to_remove {
                    tokens_to_remove -= msg_tokens;
                } else {
                    break;
                }
            }
            self.messages.pop_front();
        }

        // Recalculate token count
        self.current_tokens = self.system_prompt_tokens
            + self
                .messages
                .iter()
                .map(|m| estimate_tokens(&m.content))
                .sum::<usize>();
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
        self.current_tokens = self.system_prompt_tokens;
    }
}

/// Simple token estimation (roughly 4 chars per token)
/// This is a rough estimate - for production, use tiktoken or similar
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_context_manager_new() {
        let manager = ContextManager::new(1000, "System prompt");
        assert_eq!(manager.max_tokens, 1000);
        assert_eq!(manager.message_count(), 0);
    }

    #[test]
    fn test_add_message() {
        let mut manager = ContextManager::new(1000, "System prompt");
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello world".to_string()),
            images: vec![],
            metadata: None,
        };
        manager.add_message(msg);
        assert_eq!(manager.message_count(), 1);
    }

    #[test]
    fn test_token_usage() {
        let mut manager = ContextManager::new(1000, "System prompt");
        assert!(manager.token_usage() < 0.5);

        for i in 0..100 {
            let msg = Message {
                role: "user".into(),
                content: Arc::new(format!("Message {}", i)),
                images: vec![],
                metadata: None,
            };
            manager.add_message(msg);
        }

        // Token usage should increase
        assert!(manager.token_usage() > 0.0);
    }

    #[test]
    fn test_prune_to_fit() {
        let mut manager = ContextManager::new(1000, "System prompt");

        // Add many messages
        for _i in 0..50 {
            let msg = Message {
                role: "user".into(),
                content: Arc::new("A".repeat(100)), // ~25 tokens each
                images: vec![],
                metadata: None,
            };
            manager.add_message(msg);
        }

        let initial_count = manager.message_count();
        manager.prune_to_fit(200);
        let final_count = manager.message_count();

        // Should have pruned some messages
        assert!(final_count < initial_count);
        // Should keep at least 5 messages
        assert!(final_count >= 5);
    }

    #[test]
    fn test_clear() {
        let mut manager = ContextManager::new(1000, "System prompt");
        let msg = Message {
            role: "user".into(),
            content: Arc::new("Hello".to_string()),
            images: vec![],
            metadata: None,
        };
        manager.add_message(msg);
        assert_eq!(manager.message_count(), 1);

        manager.clear();
        assert_eq!(manager.message_count(), 0);
    }
}
