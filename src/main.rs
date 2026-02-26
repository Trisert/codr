mod model;
mod error;
mod parser;
mod config;
mod tui;
mod tui_components;
mod tools;

use error::AgentError;
use model::{Model, ModelType};
use parser::{parse_action, Action, format_available_tools};
use config::Config;
use tools::{ToolRegistry, create_coding_tools};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // Check for --chat or -c flag for TUI mode
    let use_tui = args.iter().any(|a| a == "--chat" || a == "-c");

    // Load configuration
    let config = Config::load();
    let model_type = config.to_model_type();
    let model = Model::new(model_type.clone());

    // Get model name for display
    let model_name = match &model_type {
        ModelType::LlamaServer { model, .. } => model.clone(),
        ModelType::Anthropic => "claude".to_string(),
        ModelType::Nim { model, .. } => model.clone(),
    };

    // Create tool registry
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let tool_registry = create_coding_tools(cwd);
    let tools_description = tool_registry.descriptions();

    if use_tui || args.len() == 1 {
        // TUI mode (interactive)
        let mut app = tui::App::new(model, tool_registry, model_name);
        app.set_system_prompt(&get_system_prompt(&tools_description));

        // If task provided as argument after flags, use it as initial message
        if let Some(pos) = args.iter().position(|a| a == "--") {
            if pos + 1 < args.len() {
                let initial_task = args[pos + 1..].join(" ");
                app.messages.push(tui_components::ChatMessage::user(&initial_task));
                app.process_message().await?;
            }
        }

        tui::run_tui(app).await?;
    } else {
        // Direct mode (non-interactive, single task)
        let initial_task = if let Some(pos) = args.iter().position(|a| a == "--") {
            args[pos + 1..].join(" ")
        } else {
            args[1..].join(" ")
        };

        run_direct(model, &tool_registry, &tools_description, &initial_task).await?;
    }

    Ok(())
}

// ============================================================
// Direct execution mode (non-interactive)
// ============================================================

async fn run_direct(
    model: Model,
    tool_registry: &ToolRegistry,
    tools_description: &str,
    initial_task: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize messages with system prompt and user task
    let mut messages = model.create_messages(vec![
        ("system", &get_system_prompt(tools_description)),
        ("user", initial_task),
    ]);

    // Main agent loop
    loop {
        // Query the LM
        let lm_output = model.query(&messages).await?;
        println!("LM output:\n{}", lm_output);
        println!("\n{}", "─".repeat(60));

        // Remember what the LM said
        messages = model.add_assistant_message(messages, &lm_output);

        // Parse the action
        let action = match parse_action(&lm_output) {
            Ok(a) => a,
            Err(AgentError::FormatError(msg)) => {
                println!("Format error, telling LM to correct...\n");
                let enhanced_msg = format!("{}\n\n{}", msg, format_available_tools(&tools_description));
                messages = model.add_user_message(messages, &enhanced_msg);
                continue;
            }
            Err(AgentError::TerminatingError(msg)) => {
                println!("{}", msg);
                break;
            }
            Err(AgentError::TimeoutError(msg)) => {
                println!("Timeout: {}", msg);
                messages = model.add_user_message(messages, &msg);
                continue;
            }
        };

        // Execute the action
        let output = match execute_action(&action, tool_registry) {
            Ok(o) => o,
            Err(AgentError::TerminatingError(msg)) => {
                println!("{}", msg);
                break;
            }
            Err(AgentError::TimeoutError(msg)) => msg,
            Err(AgentError::FormatError(msg)) => msg,
        };

        println!("Output:\n{}", output);
        println!("\n{}", "═".repeat(60));
        println!();

        // Send command output back to LM
        messages = model.add_user_message(messages, &output);
    }

    Ok(())
}

// ============================================================
// Execute Action
// ============================================================

fn execute_action(action: &Action, tool_registry: &ToolRegistry) -> Result<String, AgentError> {
    use std::process::Command;

    match action {
        Action::Bash(command) => {
            // Handle exit command
            if command.trim() == "exit" {
                return Err(AgentError::TerminatingError("Agent requested to exit".to_string()));
            }

            // Set environment variables to disable interactive elements
            let output = Command::new("bash")
                .arg("-c")
                .arg(command)
                .env("PAGER", "cat")
                .env("MANPAGER", "cat")
                .env("LESS", "-R")
                .env("PIP_PROGRESS_BAR", "off")
                .env("TQDM_DISABLE", "1")
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
                    Ok(format!("{}\n{}", stdout, stderr).trim().to_string())
                }
                Err(e) => Err(AgentError::TimeoutError(format!("Command execution failed: {}", e))),
            }
        }
        Action::Tool { name, params } => {
            match tool_registry.execute(name, params.clone()) {
                Ok(output) => {
                    let mut result = output.content;
                    if !output.attachments.is_empty() {
                        result.push_str(&format!("\n[{} attachment(s)]", output.attachments.len()));
                    }
                    if let Some(line_count) = output.metadata.line_count {
                        result.push_str(&format!("\n[Lines: {}]", line_count));
                    }
                    if output.metadata.truncated {
                        result.push_str(" [truncated]");
                    }
                    Ok(result)
                }
                Err(e) => Ok(format!("Tool error: {}", e)),
            }
        }
    }
}

// ============================================================
// System Prompt
// ============================================================

fn get_system_prompt(tools_description: &str) -> String {
    format!(
        "You are an expert coding assistant operating inside codr, a coding agent harness. \
        You help users by reading files, executing commands, editing code, and writing new files.\n\n\
        Available tools:\n{}\n\n\
        Guidelines:\n\
        - Use read to examine files before editing\n\
        - Use edit for precise changes (find exact text and replace)\n\
        - Use write only for new files or complete rewrites\n\
        - Be concise in your responses\n\
        - Show file paths clearly when working with files\n\
        - Use bash for commands not covered by tools\n\
        \n\
        Action format:\n\
        - For tools: ```tool-action\n<tool_name>\n<json_params>\n```\n\
        - For bash: ```bash-action\n<command>\n```\n\
        \n\
        Example tool calls:\n\
        ```tool-action\nread\n{{\"file_path\": \"src/main.rs\"}}\n```\n\
        ```tool-action\nfind\n{{\"pattern\": \"*.rs\"}}\n```\n\
        ```tool-action\ngrep\n{{\"pattern\": \"TODO\", \"path\": \".\"}}\n```\n\
        \n\
        When you've completed the user's request, run the bash exit command.",
        tools_description
    )
}
