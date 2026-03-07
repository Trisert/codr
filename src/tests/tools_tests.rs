//! Tests for tool implementations

use crate::tools::{Role, ToolRegistry};
use std::path::PathBuf;

#[test]
fn test_tool_registry_creation() {
    let cwd = PathBuf::from(".");
    let registry = ToolRegistry::new(cwd);
    // Registry should be created with tools
    assert!(registry.get_tools_for_role(Role::Yolo).len() > 0);
}

#[test]
fn test_tool_registry_get_tools_for_role() {
    let cwd = PathBuf::from(".");
    let registry = ToolRegistry::new(cwd);

    // Yolo role should allow all tools
    let tools = registry.get_tools_for_role(Role::Yolo);
    assert!(!tools.is_empty());
}

#[test]
fn test_role_planning_read_only() {
    let cwd = PathBuf::from(".");
    let registry = ToolRegistry::new(cwd);

    // Planning role should only allow read-only tools
    let tools = registry.get_tools_for_role(Role::Planning);
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

    // Write and edit should NOT be available in Planning mode
    assert!(!tool_names.contains(&"write"));
    assert!(!tool_names.contains(&"edit"));
}

#[test]
fn test_role_safe_allows_all_tools() {
    let cwd = PathBuf::from(".");
    let registry = ToolRegistry::new(cwd);

    // Safe role should have all tools (approval required for some)
    let safe_tools = registry.get_tools_for_role(Role::Safe);
    let yolo_tools = registry.get_tools_for_role(Role::Yolo);

    assert_eq!(safe_tools.len(), yolo_tools.len());
}

#[test]
fn test_role_yolo_allows_all_tools() {
    let cwd = PathBuf::from(".");
    let registry = ToolRegistry::new(cwd);

    // Yolo role should allow all tools without approval
    let tools = registry.get_tools_for_role(Role::Yolo);
    assert!(!tools.is_empty());
}
