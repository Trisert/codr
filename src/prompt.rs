// ============================================================
// Model-Agnostic System Prompt Generation
// ============================================================

use once_cell::sync::Lazy;

/// Cached tool reminders for each model type
static TOOL_REMINDERS: Lazy<std::collections::HashMap<&'static str, String>> = Lazy::new(|| {
    let mut m = std::collections::HashMap::new();
    m.insert(
        "anthropic",
        "Use <codr_tool name=\"tool\">{params}</codr_tool> for tools, <codr_bash>cmd</codr_bash> for bash.".to_string(),
    );
    m.insert(
        "claude",
        "Use <codr_tool name=\"tool\">{params}</codr_tool> for tools, <codr_bash>cmd</codr_bash> for bash.".to_string(),
    );
    m.insert(
        "openai",
        "IMPORTANT: Use XML format for tool calls:\n\
            - Tools: <codr_tool name=\"tool_name\">{\"param\": \"value\"}</codr_tool>\n\
            - Bash: <codr_bash>command</codr_bash>\n\
            Examples: <codr_tool name=\"read\">{\"file_path\": \"src/main.rs\"}</codr_tool>"
            .to_string(),
    );
    m.insert(
        "openai-compatible",
        "IMPORTANT: Use XML format for tool calls:\n\
            - Tools: <codr_tool name=\"tool_name\">{\"param\": \"value\"}</codr_tool>\n\
            - Bash: <codr_bash>command</codr_bash>\n\
            Examples: <codr_tool name=\"read\">{\"file_path\": \"src/main.rs\"}</codr_tool>"
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptStyle {
    /// Standard prompt with clear XML format instructions
    Standard,
    /// Concise prompt for models that prefer brevity
    Concise,
    /// Detailed prompt with extensive examples
    Detailed,
    /// Minimal prompt - assumes model understands tool calling
    Minimal,
}

impl Default for PromptStyle {
    fn default() -> Self {
        Self::Standard
    }
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
        ## How to Respond\n\n\
        When the user asks you to perform a coding task, call the appropriate tool.\n\
        When the user asks a question or greets you, respond naturally in plain text.\n\n\
        ## Tool Call Format\n\n\
        Use this XML format for tools:\n\
        - Tools: <codr_tool name=\"TOOL_NAME\">{{\"param\": \"value\"}}</codr_tool>\n\
        - Bash: <codr_bash>command here</codr_bash>\n\n\
        ## Key Guidelines\n\n\
        - Call tools directly, without explanations\n\
        - You can make multiple tool calls in one response\n\
        - For exploration, read all relevant files before summarizing\n\
        - Use plain text only for questions, greetings, or final summaries\n\n\
        ## Examples\n\n\
        User: List Rust files\n\
        Assistant: <codr_tool name=\"find\">{{\"pattern\": \"*.rs\"}}</codr_tool>\n\n\
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
        Call tools using XML: <codr_tool name=\"NAME\">{{...}}</codr_tool> or <codr_bash>cmd</codr_bash>\n\
        Use plain text for questions. No explanations before tool calls.",
        tools_section, context_section
    )
}

/// Detailed prompt - with extensive examples
fn build_detailed_prompt(tools_section: &str, context_section: &str) -> String {
    format!(
        "You are codr, an AI coding assistant designed to help users explore, understand, \
        and modify codebases efficiently.\n\n\
        {}\n\
        {}\n\
        ## Response Format\n\n\
        You must respond in one of two ways:\n\n\
        1. **Tool Call**: When performing a coding task, use the XML format below\n\
        2. **Plain Text**: When answering questions, greeting, or providing a summary\n\n\
        ## Tool Call Format\n\n\
        Tools are called using XML tags:\n\n\
        **For read/write/search tools:**\n\
        <codr_tool name=\"tool_name\">{{\"param_name\": \"value\"}}</codr_tool>\n\n\
        **For bash commands:**\n\
        <codr_bash>your command here</codr_bash>\n\n\
        ## Important Rules\n\n\
        1. **No explanations before tool calls** - Output the tool call immediately\n\
        2. **Multiple calls allowed** - You can output several tool calls in one response\n\
        3. **Complete the task** - Keep making tool calls until the user's request is fulfilled\n\
        4. **Read before summarizing** - When exploring code, read all relevant files first\n\n\
        ## Examples\n\n\
        User: Find all Rust files\n\
        Assistant: <codr_tool name=\"find\">{{\"pattern\": \"*.rs\"}}</codr_tool>\n\n\
        User: Read main.rs and parser.rs\n\
        Assistant: <codr_tool name=\"read\">{{\"file_path\": \"src/main.rs\"}}</codr_tool>\n\
        <codr_tool name=\"read\">{{\"file_path\": \"src/parser.rs\"}}</codr_tool>\n\n\
        User: Run the tests\n\
        Assistant: <codr_bash>cargo test</codr_bash>\n\n\
        User: Hello!\n\
        Assistant: Hello! How can I help you with your code today?\n\n\
        User: What does the parse function do?\n\
        Assistant: Let me read the parser module to understand what the parse function does.\n\
        <codr_tool name=\"read\">{{\"file_path\": \"src/parser.rs\"}}</codr_tool>\n\
        (After reading) The parse function handles tool call extraction from LLM responses...",
        tools_section, context_section
    )
}

/// Minimal prompt - assumes model understands tool calling
fn build_minimal_prompt(tools_section: &str, context_section: &str) -> String {
    format!(
        "You are codr, a coding assistant.\n\n\
        {}\
        {}\
        Tools: <codr_tool name=\"n\">{{...}}</codr_tool> | <codr_bash>cmd</codr_bash>",
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
}
