pub mod agent;
pub mod commands;
pub mod config;
pub mod context_manager;
pub mod conversation;
pub mod error;
pub mod fuzzy;
pub mod logo;
pub mod model;
pub mod model_probe;
pub mod model_registry;
pub mod parser;
pub mod prompt;
pub mod tools;
pub mod tui;

// ============================================================
// Test modules
// ============================================================

#[cfg(test)]
mod tests;
