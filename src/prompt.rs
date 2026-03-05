// ============================================================
// Model-Agnostic System Prompt Generation
// ============================================================

use once_cell::sync::Lazy;

/// Cached tool reminders for each model type
static TOOL_REMINDERS: Lazy<std::collections::HashMap<&'static str, String>> = Lazy::new(|| {
    let mut m = std::collections::HashMap::new();
    m.insert(
        "anthropic",
        "Use JSON for tool calls: {\"name\": \"tool\", \"arguments\": {...}}. Bash: {\"name\": \"bash\", \"arguments\": {\"command\": \"cmd\"}}.".to_string(),
    );
    m.insert(
        "claude",
        "Use JSON for tool calls: {\"name\": \"tool\", \"arguments\": {...}}. Bash: {\"name\": \"bash\", \"arguments\": {\"command\": \"cmd\"}}.".to_string(),
    );
    m.insert(
        "openai",
        "IMPORTANT: Use JSON format for tool calls:\n\
            - Tools: {\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n\
            - Bash: {\"name\": \"bash\", \"arguments\": {\"command\": \"command\"}}\n\
            Example: {\"name\": \"read\", \"arguments\": {\"file_path\": \"src/main.rs\"}}"
            .to_string(),
    );
    m.insert(
        "openai-compatible",
        "IMPORTANT: Use JSON format for tool calls:\n\
            - Tools: {\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n\
            - Bash: {\"name\": \"bash\", \"arguments\": {\"command\": \"command\"}}\n\
            Example: {\"name\": \"read\", \"arguments\": {\"file_path\": \"src/main.rs\"}}"
            .to_string(),
    );
    m
});

/// Cached prompt styles for each model type
static PROMPT_STYLES: Lazy<std::collections::HashMap<&'static str, PromptStyle>> =
    Lazy::new(|| {
        let mut m = std::collections::HashMap::new();
        m.insert("anthropic", PromptStyle::Standard);
        m.insert("claude", PromptStyle::Standard);
        m.insert("openai", PromptStyle::Detailed);
        m.insert("openai-compatible", PromptStyle::Detailed);
        m
    });

/// System prompt style - different models respond better to different styles
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PromptStyle {
    /// Standard prompt with clear JSON format instructions
    #[default]
    Standard,
    /// Concise prompt for models that prefer brevity
    Concise,
    /// Detailed prompt with extensive examples
    Detailed,
    /// Minimal prompt - assumes model understands tool calling
    Minimal,
}

/// Build a system prompt that works across different LLM families
pub fn build_system_prompt(
    tools_description: &str,
    project_context: &str,
    style: PromptStyle,
) -> String {
    let tools_section = format_tools(tools_description);
    let context_section = if project_context.is_empty() {
        String::new()
    } else {
        format!("## Project Context\n\n{project_context}\n\n")
    };

    match style {
        PromptStyle::Standard => build_standard_prompt(&tools_section, &context_section),
        PromptStyle::Concise => build_concise_prompt(&tools_section, &context_section),
        PromptStyle::Detailed => build_detailed_prompt(&tools_section, &context_section),
        PromptStyle::Minimal => build_minimal_prompt(&tools_section, &context_section),
    }
}

/// Format tool descriptions in a clear, model-agnostic way
fn format_tools(tools: &str) -> String {
    format!("## Available Tools\n\n{tools}\n")
}

/// Standard prompt - works well for most models
fn build_standard_prompt(tools_section: &str, context_section: &str) -> String {
    format!(
        "You are codr, a coding assistant that helps users explore and modify codebases.\n\n\
        {}\n\
        {}\n\
        ## How This Works\n\n\
        You operate in an agent loop. When you call tools, the system automatically executes them \
        and sends results back as \"Tool result: ...\" messages. These are NOT from the user. \
        Continue working until the task is complete, then respond with plain text.\n\n\
        ## How to Respond\n\n\
        When the user asks you to perform a coding task, call the appropriate tool.\n\
        When the user asks a question or greets you, respond naturally in plain text.\n\n\
        ## Tool Call Format\n\n\
        Use this JSON format for tools:\n\
        - Tools: {{\"name\": \"TOOL_NAME\", \"arguments\": {{\"param\": \"value\"}}}}\n\
        - Bash: {{\"name\": \"bash\", \"arguments\": {{\"command\": \"command here\"}}}}\n\n\
        ## Tool Usage Guide\n\n\
        - **find**: Search for files by NAME (glob patterns like `*.rs`). Does NOT search contents.\n\
        - **grep**: Search INSIDE files for text patterns.\n\
        - **read**: Read file contents.\n\n\
        ## Key Guidelines\n\n\
        - Call tools directly, without explanations\n\
        - You can make multiple tool calls in one response (as a JSON array)\n\
        - Do NOT re-read files you already have\n\
        - After reading 3-5 files, STOP and provide a summary\n\
        - Use plain text only for questions, greetings, or final summaries\n\n\
        ## Examples\n\n\
        User: List Rust files\n\
        Assistant: {{\"name\": \"find\", \"arguments\": {{\"pattern\": \"*.rs\"}}}}\n\n\
        User: Search for parse functions\n\
        Assistant: {{\"name\": \"grep\", \"arguments\": {{\"pattern\": \"fn parse\"}}}}\n\n\
        User: What does this code do?\n\
        Assistant: I'd be happy to help! Could you please specify which file or code section you're referring to?",
        tools_section, context_section
    )
}

/// Concise prompt - for models that prefer brevity
fn build_concise_prompt(tools_section: &str, context_section: &str) -> String {
    format!(
        "You are codr, a coding assistant.\n\n\
        {}\n\
        {}\n\
        Call tools using JSON: {{\"name\": \"NAME\", \"arguments\": {{...}}}}.\n\
        Use plain text for questions. No explanations before tool calls.",
        tools_section, context_section
    )
}

/// Detailed prompt - with extensive examples and agent loop explanation
fn build_detailed_prompt(tools_section: &str, context_section: &str) -> String {
    format!(
        "You are codr, an AI coding assistant that helps users explore, understand, \
        and modify codebases.\n\n\
        {}\n\
        {}\n\
        ## How This Works (Agent Loop)\n\n\
        You operate in an automatic agent loop:\n\
        1. The user sends a message\n\
        2. You respond with tool calls OR plain text\n\
        3. If you made tool calls, the system AUTOMATICALLY executes them and sends the results back to you as \"Tool result: ...\" messages\n\
        4. You then see those results and can make MORE tool calls or respond with plain text\n\
        5. When you respond with ONLY plain text (no tool calls), the loop ends and the user sees your response\n\n\
        IMPORTANT: \"Tool result\" messages are NOT from the user. They are automatic system responses to your tool calls. \
        Do NOT treat them as user messages. Continue working on the original task.\n\n\
        ## CRITICAL OUTPUT RULES\n\n\
        When calling tools, you MUST output ONLY the JSON:\n\
        - NO thinking blocks or reasoning text\n\
        - NO \"I will...\" or \"Let me...\" explanations\n\
        - NO markdown formatting (no ```json...``` wrappers)\n\
        - Output ONLY the JSON object(s) - nothing before or after\n\n\
        ## Response Format\n\n\
        You must respond in one of two ways:\n\n\
        1. **Tool Calls**: Use JSON format to call tools (system will execute and return results)\n\
        2. **Plain Text**: Provide your answer, summary, or explanation (ends the loop)\n\n\
        ## Tool Call Format\n\n\
        **For read/write/search tools:**\n\
        {{\"name\": \"tool_name\", \"arguments\": {{\"param_name\": \"value\"}}}}\n\n\
        **CRITICAL: ALWAYS include both \"name\" AND \"arguments\" fields**\n\
        - WRONG: {{\"content\": \"...\"}} or {{\"file_path\": \"...\", \"content\": \"...\"}}\n\
        - RIGHT: {{\"name\": \"write\", \"arguments\": {{\"file_path\": \"test.txt\", \"content\": \"...\"}}}}\n\n\
        **For bash commands:**\n\
        {{\"name\": \"bash\", \"arguments\": {{\"command\": \"your command here\"}}}}\n\n\
        **Multiple tool calls (JSON array):**\n\
        [\n\
          {{\"name\": \"read\", \"arguments\": {{\"file_path\": \"src/main.rs\"}}}},\n\
          {{\"name\": \"grep\", \"arguments\": {{\"pattern\": \"fn main\"}}}}\n\
        ]\n\n\
        ## Tool Usage Guide\n\n\
        - **read**: Read a file's contents. Use `file_path` parameter.\n\
        - **find**: Search for files BY NAME using glob patterns (e.g. `*.rs`, `*.toml`). Does NOT search file contents.\n\
        - **grep**: Search INSIDE files for text/regex patterns. Use this to find code patterns like functions or imports.\n\
        - **bash**: Run shell commands. Use for builds, tests, git operations, etc.\n\
        - **edit**: Modify files using old_text/new_text replacement.\n\
        - **write**: Create or overwrite files. YOU MUST USE THE write TOOL - never output file content as text.\n\n\
        ## Important Rules\n\n\
        1. **NO file content as plain text** - When asked to create/modify files, ALWAYS use the write/edit tools. Never output file content as plain text in your response.\n\
        2. **No explanations before tool calls** - Output tool calls directly\n\
        3. **Multiple calls allowed** - You can output several tool calls in one response (as a JSON array)\n\
        4. **Do NOT re-read files** - Once you have seen a file's contents, do not read it again\n\
        5. **Summarize when done** - After gathering enough information, respond with a plain text summary\n\
        6. **find searches filenames, grep searches contents** - Do not confuse them\n\
        7. **Stay focused** - Work toward completing the user's original request\n\n\
        ## Workflow for \"Explore the codebase\"\n\n\
        1. First, find all source files: {{\"name\": \"find\", \"arguments\": {{\"pattern\": \"*.rs\"}}}}\n\
        2. Read the key files (main entry point, important modules)\n\
        3. After reading 3-5 key files, STOP and write a plain text summary of the architecture\n\n\
        ## Examples\n\n\
        User: Find all Rust files\n\
        Assistant: {{\"name\": \"find\", \"arguments\": {{\"pattern\": \"*.rs\"}}}}\n\n\
        User: Search for the parse function\n\
        Assistant: {{\"name\": \"grep\", \"arguments\": {{\"pattern\": \"fn parse\"}}}}\n\n\
        User: Read main.rs and parser.rs\n\
        Assistant: [\n\
          {{\"name\": \"read\", \"arguments\": {{\"file_path\": \"src/main.rs\"}}}},\n\
          {{\"name\": \"read\", \"arguments\": {{\"file_path\": \"src/parser.rs\"}}}}\n\
        ]\n\n\
        User: Create a file called test.rs with a hello world function\n\
        Assistant: {{\"name\": \"write\", \"arguments\": {{\"file_path\": \"test.rs\", \"content\": \"pub fn hello() {{ println!(\\\"Hello, world!\\\"); }}\"}}}}\n\n\
        User: Run the tests\n\
        Assistant: {{\"name\": \"bash\", \"arguments\": {{\"command\": \"cargo test\"}}}}\n\n\
        User: Hello!\n\
        Assistant: Hello! How can I help you with your code today?",
        tools_section, context_section
    )
}

/// Minimal prompt - assumes model understands tool calling
fn build_minimal_prompt(tools_section: &str, context_section: &str) -> String {
    format!(
        "You are codr, a coding assistant.\n\n\
        {}\
        {}\
        Tools: {{\"name\": \"n\", \"arguments\": {{...}}}} | Bash: {{\"name\": \"bash\", \"arguments\": {{\"command\": \"cmd\"}}}}",
        tools_section, context_section
    )
}

/// Get the tool reminder that's appended to user messages
/// This can be customized based on model type (cached)
pub fn get_tool_reminder(model_type: &str) -> Option<String> {
    TOOL_REMINDERS.get(model_type).cloned()
}

/// Get recommended prompt style for a model type (cached)
pub fn get_recommended_style(model_type: &str) -> PromptStyle {
    *PROMPT_STYLES
        .get(model_type)
        .unwrap_or(&PromptStyle::Standard)
}

/// Get model type identifier from ModelType (for prompt selection)
pub fn get_model_type_identifier(model_type: &crate::model::ModelType) -> &'static str {
    match model_type {
        crate::model::ModelType::Anthropic => "anthropic",
        crate::model::ModelType::OpenAI { .. } => "openai-compatible",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_prompt_contains_tools() {
        let prompt = build_system_prompt("## Tools\n- read: Read files", "", PromptStyle::Standard);
        assert!(prompt.contains("Available Tools"));
        assert!(prompt.contains("read: Read files"));
    }

    #[test]
    fn test_concise_prompt_is_shorter() {
        let standard = build_system_prompt("## Tools\n- read", "", PromptStyle::Standard);
        let concise = build_system_prompt("## Tools\n- read", "", PromptStyle::Concise);
        assert!(concise.len() < standard.len());
    }

    #[test]
    fn test_minimal_prompt_is_shortest() {
        let minimal = build_system_prompt("## Tools\n- read", "", PromptStyle::Minimal);
        assert!(minimal.len() < 500);
    }

    #[test]
    fn test_tool_reminder_anthropic() {
        let reminder = get_tool_reminder("anthropic");
        assert!(reminder.is_some());
        assert!(reminder.unwrap().len() < 200);
    }

    #[test]
    fn test_tool_reminder_openai() {
        let reminder = get_tool_reminder("openai");
        assert!(reminder.is_some());
        assert!(reminder.unwrap().contains("IMPORTANT"));
    }

    #[test]
    fn test_prompt_uses_json_format() {
        let prompt = build_system_prompt("## Tools\n- read: Read files", "", PromptStyle::Standard);
        // Should contain JSON format examples, not XML
        assert!(prompt.contains("{\"name\":"));
        assert!(prompt.contains("\"arguments\":"));
        // Should NOT contain XML format
        assert!(!prompt.contains("<codr_tool"));
        assert!(!prompt.contains("</codr_tool>"));
    }
}
