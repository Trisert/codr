// ============================================================
// Dynamic Tool Registration System
// ============================================================
//
// This module allows runtime registration of tools with proper
// schema handling and validation.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use super::schema::ToolSchema;
use super::{Tool, ToolContext, ToolCategory, ToolError, ToolOutput};

// ============================================================
// Dynamic Tool Specification
// ============================================================

/// Specification for registering a tool dynamically
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolSpec {
    /// Unique name for the tool
    pub name: String,
    /// Human-readable label
    pub label: String,
    /// Description of what the tool does
    pub description: String,
    /// JSON schema for parameters
    pub parameters: Value,
    /// Tool category
    pub category: String,  // "FileOps", "Search", or "System"
    /// Optional: Command to execute (for simple tools)
    pub command: Option<String>,
    /// Optional: Script content (for script-based tools)
    pub script: Option<String>,
}

impl DynamicToolSpec {
    /// Create a new dynamic tool spec
    pub fn new(
        name: String,
        label: String,
        description: String,
        parameters: Value,
    ) -> Self {
        Self {
            name,
            label,
            description,
            parameters,
            category: "System".to_string(),
            command: None,
            script: None,
        }
    }

    /// Set the category
    pub fn with_category(mut self, category: String) -> Self {
        self.category = category;
        self
    }

    /// Set the command to execute
    pub fn with_command(mut self, command: String) -> Self {
        self.command = Some(command);
        self
    }

    /// Set the script content
    pub fn with_script(mut self, script: String) -> Self {
        self.script = Some(script);
        self
    }

    /// Parse category string to ToolCategory
    pub fn parse_category(&self) -> ToolCategory {
        match self.category.to_lowercase().as_str() {
            "fileops" | "file" => ToolCategory::FileOps,
            "search" => ToolCategory::Search,
            "system" | "shell" | "bash" => ToolCategory::System,
            _ => ToolCategory::System,
        }
    }

    /// Validate the spec
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("Tool name cannot be empty".to_string());
        }

        if self.description.is_empty() {
            return Err("Tool description cannot be empty".to_string());
        }

        // Validate parameters is a valid JSON object
        if !self.parameters.is_object() {
            return Err("Parameters must be a JSON object".to_string());
        }

        // Validate category
        self.parse_category();

        Ok(())
    }
}

// ============================================================
// Dynamic Tool Implementation
// ============================================================

/// A dynamically registered tool
#[derive(Clone)]
pub struct DynamicTool {
    spec: DynamicToolSpec,
    schema: ToolSchema,
}

impl DynamicTool {
    pub fn from_spec(spec: DynamicToolSpec) -> Result<Self, String> {
        spec.validate()?;

        // Convert JSON schema to ToolSchema
        let schema = ToolSchema::from_json(&spec.parameters)
            .map_err(|e| format!("Invalid parameter schema: {}", e))?;

        Ok(Self { spec, schema })
    }
}

impl Tool for DynamicTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn label(&self) -> &str {
        &self.spec.label
    }

    fn description(&self) -> &str {
        &self.spec.description
    }

    fn parameters(&self) -> &ToolSchema {
        &self.schema
    }

    fn category(&self) -> ToolCategory {
        self.spec.parse_category()
    }

    fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        // If there's a command template, execute it
        if let Some(ref command) = self.spec.command {
            return self.execute_command(command, params, ctx);
        }

        // If there's a script, execute it
        if let Some(ref script) = self.spec.script {
            return self.execute_script(script, params, ctx);
        }

        // Default: return an error
        Err(ToolError::Custom(
            "Dynamic tool has no command or script configured".to_string()
        ))
    }
}

impl DynamicTool {
    fn execute_command(
        &self,
        command_template: &str,
        params: Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        // Substitute parameters into the command template
        let mut command = command_template.to_string();

        if let Some(obj) = params.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{}}}", key);
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => value.to_string(),
                };
                command = command.replace(&placeholder, &replacement);
            }
        }

        // Execute the command
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&command)
            .env("PAGER", "cat")
            .env("MANPAGER", "cat")
            .env("LESS", "-R")
            .output();

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout).to_string();
                let stderr = String::from_utf8_lossy(&result.stderr).to_string();
                let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
                Ok(ToolOutput::text(combined))
            }
            Err(e) => Err(ToolError::Custom(format!("Command failed: {}", e))),
        }
    }

    fn execute_script(
        &self,
        script: &str,
        params: Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        // For script execution, we can pass parameters as environment variables
        let mut cmd = std::process::Command::new("bash");
        cmd.arg("-c");

        if let Some(obj) = params.as_object() {
            for (key, value) in obj {
                let env_value = match value {
                    Value::String(s) => s.clone(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => value.to_string(),
                };
                cmd.env(format!("PARAM_{}", key.to_uppercase()), env_value);
            }
        }

        cmd.arg(script);

        let output = cmd.output();

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout).to_string();
                let stderr = String::from_utf8_lossy(&result.stderr).to_string();
                let combined = format!("{}\n{}", stdout, stderr).trim().to_string();
                Ok(ToolOutput::text(combined))
            }
            Err(e) => Err(ToolError::Custom(format!("Script failed: {}", e))),
        }
    }
}

// ============================================================
// Dynamic Tool Registry Extension
// ============================================================

/// Extension to ToolRegistry for dynamic tool registration
pub struct DynamicToolRegistry {
    /// Base tool registry
    base_registry: super::ToolRegistry,
    /// Dynamically registered tools
    dynamic_tools: RwLock<HashMap<String, DynamicTool>>,
    /// Config file path for persistence
    config_path: Option<PathBuf>,
}

impl DynamicToolRegistry {
    pub fn new(cwd: PathBuf) -> Self {
        let mut registry = Self {
            base_registry: super::ToolRegistry::new(cwd),
            dynamic_tools: RwLock::new(HashMap::new()),
            config_path: None,
        };

        // Load dynamic tools from config
        if let Some(config_dir) = dirs::config_dir() {
            let config_path = config_dir.join("codr").join("dynamic_tools.toml");
            registry.load_tools(&config_path);
            registry.config_path = Some(config_path);
        }

        registry
    }

    /// Register a dynamic tool
    pub fn register_tool(&self, spec: DynamicToolSpec) -> Result<(), String> {
        // Create the tool
        let tool = DynamicTool::from_spec(spec.clone())?;

        // Add to dynamic tools map
        let mut tools = self.dynamic_tools.write().unwrap();
        tools.insert(spec.name.clone(), tool);

        // Save to config
        self.save_tools();

        Ok(())
    }

    /// Unregister a dynamic tool
    pub fn unregister_tool(&self, name: &str) -> Result<(), String> {
        let mut tools = self.dynamic_tools.write().unwrap();

        if tools.remove(name).is_none() {
            return Err(format!("Tool '{}' not found", name));
        }

        self.save_tools();
        Ok(())
    }

    /// List all dynamic tools
    pub fn list_dynamic_tools(&self) -> Vec<String> {
        let tools = self.dynamic_tools.read().unwrap();
        tools.keys().cloned().collect()
    }

    /// Get a dynamic tool by name
    pub fn get_tool(&self, name: &str) -> Option<ToolRef> {
        let dynamic_tools = self.dynamic_tools.read().unwrap();
        if let Some(tool) = dynamic_tools.get(name) {
            return Some(ToolRef::Dynamic(std::sync::Arc::new(tool.clone())));
        }

        // For built-in tools, use base_registry.get() or base_registry.execute() directly
        None
    }

    /// Check if a tool exists (dynamic or built-in)
    pub fn has_tool(&self, name: &str) -> bool {
        let dynamic_tools = self.dynamic_tools.read().unwrap();
        dynamic_tools.contains_key(name) || self.base_registry.get(name).is_some()
    }

    /// Execute a tool (dynamic or built-in)
    pub fn execute_tool(&self, name: &str, params: Value) -> Result<ToolOutput, ToolError> {
        // Try dynamic tools first
        {
            let dynamic_tools = self.dynamic_tools.read().unwrap();
            if let Some(tool) = dynamic_tools.get(name) {
                let ctx = super::ToolContext::new(self.base_registry.cwd.clone());
                return tool.execute(params, &ctx);
            }
        }

        // Try built-in tools
        if self.base_registry.get(name).is_some() {
            return self.base_registry.execute(name, params);
        }

        Err(ToolError::Custom(format!("Tool '{}' not found", name)))
    }

    /// Get tool descriptions including dynamic tools
    pub fn descriptions(&self) -> String {
        let mut result = self.base_registry.descriptions();

        let dynamic_tools = self.dynamic_tools.read().unwrap();
        if !dynamic_tools.is_empty() {
            result.push_str("\n\n## Dynamic Tools\n");
            for tool in dynamic_tools.values() {
                result.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
            }
        }

        result
    }

    fn load_tools(&self, path: &PathBuf) {
        if !path.exists() {
            return;
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(config) = toml::from_str::<DynamicToolsConfig>(&content) {
                let mut tools = self.dynamic_tools.write().unwrap();
                for spec in config.tools {
                    if let Ok(tool) = DynamicTool::from_spec(spec.clone()) {
                        tools.insert(spec.name.clone(), tool);
                    }
                }
            }
        }
    }

    fn save_tools(&self) {
        if let Some(path) = &self.config_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            let tools = self.dynamic_tools.read().unwrap();
            let specs: Vec<DynamicToolSpec> = tools.values()
                .map(|t| DynamicToolSpec {
                    name: t.name().to_string(),
                    label: t.label().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters().to_json_value(),
                    category: format!("{:?}", t.category()),
                    command: t.spec.command.clone(),
                    script: t.spec.script.clone(),
                })
                .collect();

            let config = DynamicToolsConfig { tools: specs };

            if let Ok(toml) = toml::to_string(&config) {
                let _ = std::fs::write(path, toml);
            }
        }
    }
}

/// Reference to a dynamic tool
/// Uses Arc to allow safe return from function
pub enum ToolRef {
    Dynamic(std::sync::Arc<DynamicTool>),
}

/// Serializable config for dynamic tools
#[derive(Debug, Serialize, Deserialize)]
struct DynamicToolsConfig {
    tools: Vec<DynamicToolSpec>,
}

// ============================================================
// Tool Discovery
// ============================================================

/// Discover tools from various sources
pub struct ToolDiscovery {
    config_dirs: Vec<PathBuf>,
}

impl ToolDiscovery {
    pub fn new() -> Self {
        let mut config_dirs = Vec::new();

        // Add XDG config directory
        if let Some(config_dir) = dirs::config_dir() {
            config_dirs.push(config_dir.join("codr").join("tools"));
        }

        // Add local config directory
        config_dirs.push(PathBuf::from("./.codr/tools"));

        Self { config_dirs }
    }

    /// Discover tools from all configured directories
    pub fn discover(&self) -> Vec<DynamicToolSpec> {
        let mut specs = Vec::new();

        for dir in &self.config_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            if let Ok(spec) = toml::from_str::<DynamicToolSpec>(&content) {
                                if spec.validate().is_ok() {
                                    specs.push(spec);
                                }
                            }
                        }
                    }
                }
            }
        }

        specs
    }

    /// Watch for tool changes (returns a channel that receives new specs)
    #[allow(dead_code)]
    pub fn watch(&self) -> std::sync::mpsc::Receiver<DynamicToolSpec> {
        use std::sync::mpsc;
        use std::thread;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        let config_dirs = self.config_dirs.clone();

        thread::spawn(move || {
            let mut last_modifs = HashMap::new();

            loop {
                for dir in &config_dirs {
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        for entry in entries.filter_map(|e| e.ok()) {
                            let path = entry.path();
                            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                                if let Ok(meta) = entry.metadata() {
                                    if let Ok(modif) = meta.modified() {
                                        let last_modif = last_modifs.get(&path).copied();

                                        if last_modif != Some(modif) {
                                            if let Ok(content) = std::fs::read_to_string(&path) {
                                                if let Ok(spec) = toml::from_str::<DynamicToolSpec>(&content) {
                                                    if spec.validate().is_ok() {
                                                        let _ = tx.send(spec);
                                                    }
                                                }
                                            }
                                            last_modifs.insert(path, modif);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                thread::sleep(Duration::from_secs(1));
            }
        });

        rx
    }
}

impl Default for ToolDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_tool_spec_creation() {
        let spec = DynamicToolSpec::new(
            "test_tool".to_string(),
            "Test Tool".to_string(),
            "A test tool".to_string(),
            serde_json::json!({"type": "object"}),
        );

        assert_eq!(spec.name, "test_tool");
        assert_eq!(spec.label, "Test Tool");
    }

    #[test]
    fn test_dynamic_tool_spec_validation() {
        // Valid spec
        let spec = DynamicToolSpec::new(
            "test".to_string(),
            "Test".to_string(),
            "Description".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert!(spec.validate().is_ok());

        // Empty name
        let spec = DynamicToolSpec::new(
            "".to_string(),
            "Test".to_string(),
            "Description".to_string(),
            serde_json::json!({"type": "object"}),
        );
        assert!(spec.validate().is_err());

        // Non-object parameters
        let spec = DynamicToolSpec::new(
            "test".to_string(),
            "Test".to_string(),
            "Description".to_string(),
            serde_json::json!("not an object"),
        );
        assert!(spec.validate().is_err());
    }

    #[test]
    fn test_dynamic_tool_from_spec() {
        let spec = DynamicToolSpec::new(
            "echo_tool".to_string(),
            "Echo".to_string(),
            "Echo input".to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string"}
                }
            }),
        ).with_command("echo {message}".to_string());

        let tool = DynamicTool::from_spec(spec);
        assert!(tool.is_ok());
    }

    #[test]
    fn test_parse_category() {
        let spec = DynamicToolSpec::new(
            "test".to_string(),
            "Test".to_string(),
            "Description".to_string(),
            serde_json::json!({}),
        ).with_category("FileOps".to_string());

        assert_eq!(spec.parse_category(), ToolCategory::FileOps);

        let spec = spec.with_category("Search".to_string());
        assert_eq!(spec.parse_category(), ToolCategory::Search);
    }

    #[test]
    fn test_tool_discovery() {
        let discovery = ToolDiscovery::new();
        assert!(!discovery.config_dirs.is_empty());
    }

    #[test]
    fn test_dynamic_tool_registry() {
        let registry = DynamicToolRegistry::new(PathBuf::from("/tmp"));

        // Register a tool
        let spec = DynamicToolSpec::new(
            "test_tool".to_string(),
            "Test".to_string(),
            "A test tool".to_string(),
            serde_json::json!({"type": "object"}),
        ).with_command("echo hello".to_string());

        let result = registry.register_tool(spec);
        assert!(result.is_ok());

        // List tools
        let tools = registry.list_dynamic_tools();
        assert!(tools.contains(&"test_tool".to_string()));
    }
}
