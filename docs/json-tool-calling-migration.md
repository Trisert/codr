# JSON-Based Tool Calling Migration Plan

This document outlines the migration from XML-based tool calling to JSON-based tool calling, following the design principles from pi (Mario Zechner's minimal coding agent).

## Motivation

**Current Problems:**
1. XML-based tool calling is verbose and error-prone
2. Custom `ToolSchema` types require manual maintenance
3. Parser complexity: multiple formats to handle (XML, OpenAI JSON, Anthropic JSON, shorthand, legacy)
4. Native tool calls get converted to XML, losing the benefits of native tool calling

**Benefits of JSON-Based Calling (pi-style):**
1. Native tool calling works end-to-end without conversion
2. Automatic schema generation via schemars (like TypeBox in pi)
3. Simpler parser with single primary format
4. Better validation through serde + schemars
5. Tools can return structured data for UI (separate from LLM content)

## Architecture Changes

### 1. Tool Parameter Structs

**Before:**
```rust
// No defined parameter types, just Value
fn execute(&self, params: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>
```

**After:**
```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadParams {
    /// Path to the file to read (relative or absolute)
    pub file_path: String,
    /// Line number to start reading from (1-indexed)
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum number of lines to read
    #[serde(default)]
    pub limit: Option<usize>,
}

// Tools have an associated Params type
trait Tool {
    type Params: Serialize + DeserializeOwned + JsonSchema;

    fn execute(&self, params: Self::Params, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
}
```

### 2. JSON Schema Generation

**Before:**
```rust
// Custom schema types in schema.rs
pub struct ToolSchema {
    pub schema_type: String,
    pub properties: Vec<Property>,
    pub required: Vec<String>,
}

impl ToolSchema {
    pub fn string(mut self, name: &str, description: &str, required: bool) -> Self { ... }
    pub fn to_json_schema(&self) -> serde_json::Value { ... }
}
```

**After:**
```rust
// Automatic via schemars
use schemars::gen::SchemaGenerator;

fn get_tool_schema<T: JsonSchema>() -> serde_json::Value {
    let mut gen = SchemaGenerator::default();
    gen.generate_root_schema::<T>().into()
}

// Example output for ReadParams:
// {
//   "type": "object",
//   "properties": {
//     "file_path": { "type": "string", "description": "Path to the file..." },
//     "offset": { "type": "integer", "description": "Line number..." }
//   },
//   "required": ["file_path"]
// }
```

### 3. Tool Output with UI Display Content

**Before:**
```rust
pub struct ToolOutput {
    pub content: Arc<String>,
    pub attachments: Vec<Attachment>,
    pub metadata: OutputMetadata,
}
```

**After:**
```rust
// Following pi's design - separate content for LLM vs UI
#[derive(Debug, Clone, Serialize)]
pub struct ToolOutput {
    /// Text content for the LLM (what the model "sees")
    pub content: Arc<String>,
    /// Structured data for UI display (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// Binary attachments (images, etc.)
    pub attachments: Vec<Attachment>,
    /// Metadata about the output
    pub metadata: OutputMetadata,
}

impl ToolOutput {
    // Example: read tool can provide line count for UI
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}
```

### 4. Simplified Parser

**Before:**
- XML: `<codr_tool name="read">{"file_path": "..."}</codr_tool>`
- OpenAI JSON: `{"name": "read", "arguments": {...}}`
- Anthropic JSON: `{"type": "tool_use", "name": "read", "input": {...}}`
- Shorthand: `read file.txt`
- Legacy format
- Multiple regex patterns and fallback logic

**After:**
```rust
// Primary format: JSON
fn parse_action(content: &str) -> Option<Action> {
    let trimmed = content.trim();

    // 1. Try JSON array format (multiple tools)
    if let Ok(arr) = serde_json::from_str::<Vec<ToolCallJson>>(trimmed) {
        return Some(Action::MultipleTools(arr));
    }

    // 2. Try single JSON object
    if let Ok(call) = serde_json::from_str::<ToolCallJson>(trimmed) {
        return Some(Action::Tool(call));
    }

    // 3. Fallback to XML for backward compatibility
    parse_xml_tool_call(trimmed)
}

#[derive(Deserialize)]
struct ToolCallJson {
    name: String,
    #[serde(alias = "input", alias = "parameters")]
    arguments: serde_json::Value,
}
```

### 5. Model Integration - No XML Conversion

**Before:**
```rust
// In model.rs streaming handlers - native tool converted to XML
ContentBlock::ToolUse { id: _, name, input } => {
    let input_json = serde_json::to_string(&input).unwrap_or_default();
    let tool_xml = format!("<codr_tool name=\"{}\">{}</codr_tool>", name, input_json);
    full_content.push_str(&tool_xml);
    on_text(tool_xml);
}
```

**After:**
```rust
// Native tool calls preserved as JSON
ContentBlock::ToolUse { id: _, name, input } => {
    // Keep as JSON - create ToolCallJson
    let call = ToolCallJson {
        id: id.clone(),
        name: name.clone(),
        arguments: input.clone(),
    };
    let tool_json = serde_json::to_string(&call).unwrap_or_default();
    full_content.push_str(&tool_json);
    on_text(tool_json);
}

// Parser handles JSON directly
```

### 6. System Prompt Changes

**Before:**
```
## Tool Call Format

Use this XML format for tools:
- Tools: <codr_tool name="TOOL_NAME">{"param": "value"}</codr_tool>
- Bash: <codr_bash>command here</codr_bash>
```

**After:**
```
## Tool Call Format

When you need to use a tool, output a JSON object with this format:
{"name": "tool_name", "arguments": {"param": "value"}}

For bash commands, use the bash tool:
{"name": "bash", "arguments": {"command": "your command here"}}

You can output multiple tool calls as a JSON array:
[
  {"name": "read", "arguments": {"file_path": "src/main.rs"}},
  {"name": "grep", "arguments": {"pattern": "fn main"}}
]
```

## Implementation Steps

### Step 1: Add Dependencies
```toml
[dependencies]
schemars = "0.8"
```

### Step 2: Create Parameter Types
Create `src/tools/params.rs` with parameter structs for each tool:
- `ReadParams`
- `BashParams`
- `EditParams`
- `WriteParams`
- `GrepParams`
- `FindParams`
- `FileInfoParams`

### Step 3: Update Tool Trait
Modify `src/tools/mod.rs`:
- Add `type Params` associated type
- Change `execute()` signature
- Add default implementation for schema generation

### Step 4: Update Tool Implementations
Modify `src/tools/impl.rs`:
- Implement `Params` type for each tool
- Update `execute()` to use typed parameters
- Remove manual parameter extraction

### Step 5: Update Parser
Modify `src/parser.rs`:
- Prioritize JSON format
- Simplify with serde-based parsing
- Keep XML as fallback

### Step 6: Update Model Integration
Modify `src/model.rs`:
- Remove XML conversion
- Let native tool calls flow as JSON

### Step 7: Update System Prompt
Modify `src/prompt.rs`:
- Change format instructions to JSON
- Update examples

### Step 8: Update Tool Registry
Modify `src/tools/mod.rs`:
- Update schema generation to use schemars
- Update validation to use serde deserialization

## Migration Compatibility

To maintain backward compatibility during transition:

1. **Dual-mode parser**: Handle both XML and JSON formats
2. **Gradual migration**: Migrate tools one at a time
3. **Feature flag**: Optional `NATIVE_TOOLS_ONLY` mode to disable XML
4. **Testing**: Ensure existing test cases pass with both formats

## Benefits Summary

| Aspect | Before (XML) | After (JSON) |
|--------|-------------|--------------|
| Schema definition | Manual builder pattern | Automatic via derive macro |
| Validation | Custom type checking | serde + schemars |
| Native tool calling | Converted to XML | Preserved as JSON |
| Parser complexity | ~1500 lines, many formats | ~200 lines, JSON-first |
| Tool output | Text only | Text + structured UI data |
| Token usage | Verbose XML tags | Compact JSON |

## References

- pi blog post: Mario Zechner's "What I learned building an opinionated and minimal coding agent"
- schemars: https://github.com/GREsau/schemars
- serde: https://serde.rs/
