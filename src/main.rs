pub mod commands;
pub mod config;
pub mod error;
pub mod fuzzy;
pub mod logo;
pub mod model;
pub mod parser;
pub mod prompt;
pub mod tools;
pub mod tui;
pub mod tui_components;

use clap::Parser;
use config::Config;
use error::AgentError;
use model::{Model, ModelType};
use parser::{Action, parse_actions};
use prompt::{build_system_prompt, get_model_type_identifier, get_recommended_style, PromptStyle};
use tools::{ToolRegistry, create_coding_tools, Role};

/// codr - AI coding agent harness
#[derive(Parser, Debug)]
#[command(name = "codr")]
#[command(about = "AI coding agent harness", long_about = None)]
struct Cli {
    /// Run in direct mode (non-interactive, single task execution)
    #[arg(short = 'd', long = "direct")]
    direct: bool,

    /// Enable YOLO mode (auto-approve bash commands)
    #[arg(long = "yolo")]
    yolo: bool,

    /// Task to execute (in direct/non-interactive mode, or as initial message in TUI mode)
    #[arg(trailing_var_arg = true)]
    task: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Load configuration
    let config = Config::load();
    let model_type = config.to_model_type();
    let model = Model::new(model_type.clone());

    // Get model name for display
    let model_name = match &model_type {
        ModelType::OpenAI { model, .. } => model.clone(),
        ModelType::Anthropic => "claude".to_string(),
    };

    // Create tool registry
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let tool_registry = create_coding_tools(cwd);
    let tools_description = tool_registry.descriptions();

    // Direct mode is only used when explicitly requested with --direct/-d
    // Otherwise, TUI (chat) mode is the default
    let use_tui = !cli.direct;

    if use_tui {
        // TUI mode (interactive chat - default)
        let model_type_id = get_model_type_identifier(&model_type);
        let prompt_style = get_recommended_style(model_type_id);
        let system_prompt = build_system_prompt(
            &tools_description,
            &load_project_context().unwrap_or_default(),
            prompt_style,
        );
        let mut app = tui::App::new(model, tool_registry, model_name, cli.yolo);
        app.set_system_prompt(&system_prompt);

        // If task provided, use it as initial message
        if !cli.task.is_empty() {
            let initial_task = cli.task.join(" ");
            app.messages
                .push(tui_components::ChatMessage::user(&initial_task));
            app.start_processing();
        }

        tui::run_tui(app).await?;
    } else {
        // Direct mode (non-interactive, single task execution)
        let initial_task = cli.task.join(" ");
        if initial_task.is_empty() {
            eprintln!("Error: --direct mode requires a task to execute");
            std::process::exit(1);
        }
        let model_type_id = get_model_type_identifier(&model_type);
        let prompt_style = get_recommended_style(model_type_id);
        let project_context = load_project_context().unwrap_or_default();
        run_direct(
            model,
            &tool_registry,
            &tools_description,
            &project_context,
            prompt_style,
            &initial_task,
        )
        .await?;
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
    project_context: &str,
    prompt_style: PromptStyle,
    initial_task: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize messages with system prompt and user task
    let system_prompt = build_system_prompt(tools_description, project_context, prompt_style);
    let mut messages = model.create_messages(vec![
        ("system", &system_prompt),
        ("user", initial_task),
    ]);

    // Main agent loop - exits when a plain text response is received
    'agent_loop: loop {
        // Use native tool calling if the model supports it (direct mode uses Yolo role)
        let lm_output = if model.supports_native_tools() {
            let tools_for_role = tool_registry.get_tools_for_role(Role::Yolo);
            let tools_refs: Vec<&dyn crate::tools::Tool> = tools_for_role;
            model.query_with_tools(&messages, &tools_refs).await?
        } else {
            model.query(&messages).await?
        };
        println!("LM output:\n{}", lm_output);
        println!("\n{}", "─".repeat(60));

        // Remember what the LM said
        messages = model.add_assistant_message(messages, &lm_output);

        // Parse the actions
        let parsed = match parse_actions(&lm_output) {
            Ok(p) => p,
            Err(AgentError::Terminating(msg)) => {
                println!("{}", msg);
                break;
            }
            Err(AgentError::Timeout(msg)) => {
                println!("Timeout: {}", msg);
                messages = model.add_user_message(messages, &msg);
                continue;
            }
        };

        // Handle each action
        let mut all_outputs = Vec::new();

        for action in &parsed.actions {
            // Handle plain text response (no tools needed)
            if let Action::Response(response) = action {
                println!("{}", response);
                println!("\n{}", "═".repeat(60));
                println!();
                break 'agent_loop; // Exit the agent loop after a plain text response
            }

            // Execute tool/bash actions
            let output = match execute_action(action, tool_registry) {
                Ok(o) => o,
                Err(AgentError::Terminating(msg)) => {
                    println!("{}", msg);
                    break 'agent_loop;
                }
                Err(AgentError::Timeout(msg)) => msg,
            };

            println!("Output:\n{}", output);
            println!("\n{}", "═".repeat(60));
            println!();

            all_outputs.push(output);
        }

        // Send command outputs back to LM
        let combined_output = all_outputs.join("\n---\n\n");
        messages = model.add_user_message(messages, &combined_output);
    }

    Ok(())
}

// ============================================================
// Execute Action
// ============================================================

fn execute_action(action: &Action, tool_registry: &ToolRegistry) -> Result<String, AgentError> {
    use std::process::Command;
    use std::sync::mpsc;
    use std::thread;

    match action {
        Action::Bash {
            command,
            workdir,
            timeout_ms,
            env,
        } => {
            if command.trim() == "exit" {
                return Err(AgentError::Terminating(
                    "Agent requested to exit".to_string(),
                ));
            }

            let mut cmd = Command::new("bash");
            cmd.arg("-c")
                .arg(&**command)
                .env("PAGER", "cat")
                .env("MANPAGER", "cat")
                .env("LESS", "-R")
                .env("PIP_PROGRESS_BAR", "off")
                .env("TQDM_DISABLE", "1");

            if let Some(dir) = workdir {
                cmd.current_dir(&**dir);
            }

            if let Some(env_vars) = env
                && let Some(obj) = env_vars.as_object()
            {
                for (key, value) in obj {
                    if let Some(v) = value.as_str() {
                        cmd.env(key, v);
                    }
                }
            }

            let result = if let Some(timeout) = timeout_ms {
                let (tx, rx) = mpsc::channel();
                let cmd_str = command.clone();
                let workdir_clone = workdir.clone();
                let env_clone = env.clone();

                thread::spawn(move || {
                    let mut cmd = Command::new("bash");
                    cmd.arg("-c")
                        .arg(&*cmd_str)
                        .env("PAGER", "cat")
                        .env("MANPAGER", "cat")
                        .env("LESS", "-R")
                        .env("PIP_PROGRESS_BAR", "off")
                        .env("TQDM_DISABLE", "1");

                    if let Some(dir) = &workdir_clone {
                        cmd.current_dir(&**dir);
                    }
                    if let Some(env_vars) = &env_clone
                        && let Some(obj) = env_vars.as_object()
                    {
                        for (key, value) in obj {
                            if let Some(v) = value.as_str() {
                                cmd.env(key, v);
                            }
                        }
                    }

                    let output = cmd.output();
                    tx.send(output).ok();
                });

                match rx.recv_timeout(std::time::Duration::from_millis(*timeout)) {
                    Ok(Ok(output)) => Ok((
                        output.status,
                        output.stdout.to_vec(),
                        output.stderr.to_vec(),
                    )),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "command timed out",
                    )),
                }
            } else {
                cmd.output()
                    .map(|o| (o.status, o.stdout.to_vec(), o.stderr.to_vec()))
            };

            match result {
                Ok((status, stdout, stderr)) => {
                    if !status.success() {
                        return Err(AgentError::Timeout(format!(
                            "Command exited with code: {:?}",
                            status.code()
                        )));
                    }
                    let stdout_str = String::from_utf8_lossy(&stdout).to_string();
                    let stderr_str = String::from_utf8_lossy(&stderr).to_string();
                    Ok(format!("{}\n{}", stdout_str, stderr_str).trim().to_string())
                }
                Err(e) => Err(AgentError::Timeout(format!(
                    "Command execution failed: {}",
                    e
                ))),
            }
        }
        Action::Tool { name, params } => match tool_registry.execute(name, params.clone()) {
            Ok(output) => {
                let mut result = (*output.content).clone();
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
        },
        Action::Response(response) => Ok(response.to_string()),
    }
}

// ============================================================
// System Prompt
// ============================================================

/// Load project-specific context from CLAUDE.md or AGENT.md if present
/// Strips out the title and initial description to keep only relevant content
fn load_project_context() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;

    // Try CLAUDE.md first, then AGENT.md
    let claude_path = cwd.join("CLAUDE.md");
    let agent_path = cwd.join("AGENT.md");

    let content = if claude_path.exists() {
        std::fs::read_to_string(&claude_path).ok()
    } else if agent_path.exists() {
        std::fs::read_to_string(&agent_path).ok()
    } else {
        None
    }?;

    // Clean up the content: remove title line and initial description
    let cleaned = content
        .lines()
        .skip_while(|line| {
            let trimmed = line.trim();
            // Skip until we find a real content section (after title/description)
            trimmed.starts_with("#") ||
            trimmed.starts_with("This file provides") ||
            trimmed.is_empty() ||
            trimmed.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(cleaned)
}
