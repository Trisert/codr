// ============================================================
// Custom exceptions
// ============================================================

#[derive(Debug)]
pub enum AgentError {
    Timeout(String),
    Terminating(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Timeout(msg) => write!(f, "TIMEOUT_ERROR: {}", msg),
            AgentError::Terminating(msg) => write!(f, "TERMINATING: {}", msg),
        }
    }
}

impl std::error::Error for AgentError {}
