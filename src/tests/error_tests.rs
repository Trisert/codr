//! Tests for error types

use crate::error::AgentError;

#[test]
fn test_error_timeout_display() {
    let error = AgentError::Timeout("Operation took too long".to_string());
    assert_eq!(error.to_string(), "TIMEOUT_ERROR: Operation took too long");
}

#[test]
fn test_error_terminating_display() {
    let error = AgentError::Terminating("Fatal error occurred".to_string());
    assert_eq!(error.to_string(), "TERMINATING: Fatal error occurred");
}

#[test]
fn test_error_timeout_creation() {
    let error = AgentError::Timeout("test timeout".to_string());
    match error {
        AgentError::Timeout(msg) => assert_eq!(msg, "test timeout"),
        _ => panic!("Expected Timeout error"),
    }
}

#[test]
fn test_error_terminating_creation() {
    let error = AgentError::Terminating("test terminating".to_string());
    match error {
        AgentError::Terminating(msg) => assert_eq!(msg, "test terminating"),
        _ => panic!("Expected Terminating error"),
    }
}
