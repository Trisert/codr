// ============================================================
// Custom exceptions
// ============================================================

#[derive(Debug)]
pub enum AgentError {
    FormatError(String),
    TimeoutError(String),
    TerminatingError(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::FormatError(msg) => write!(f, "FORMAT_ERROR: {}", msg),
            AgentError::TimeoutError(msg) => write!(f, "TIMEOUT_ERROR: {}", msg),
            AgentError::TerminatingError(msg) => write!(f, "TERMINATING: {}", msg),
        }
    }
}

impl std::error::Error for AgentError {}
