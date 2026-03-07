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

use clap::Parser;
use config::Config;
use context_manager::ContextManager;
use model::{Model, ModelType};
use prompt::{PromptStyle, build_system_prompt, get_model_type_identifier, get_recommended_style};
use tools::{Role, ToolRegistry, create_coding_tools};

/// codr - AI coding agent harness
#[derive(Parser, Debug)]
#[command(name = "codr")]
#[command(about = "AI coding agent harness", long_about = None)]
struct Cli {
    /// Run in direct mode (non-interactive, single task execution)
    #[arg(short = 'd', long = "direct")]
    direct: bool,

    /// Resume the most recent conversation
    #[arg(short = 'r', long = "resume")]
    resume: bool,

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

    // Probe the server for native tool calling support (runs once at startup)
    model.probe_and_cache_tool_support().await;

    // Get model name for display
    let _model_name = match &model_type {
        ModelType::OpenAI { model, .. } => model.clone(),
        ModelType::Anthropic => "claude".to_string(),
    };

    // Create tool registry
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let tool_registry = std::sync::Arc::new(create_coding_tools(cwd.clone()));
    let tools_description = tool_registry.descriptions();

    // Create async tool registry for parallel execution and streaming
    let _async_tool_registry =
        std::sync::Arc::new(crate::tools::async_wrapper::create_async_coding_tools(cwd));

    // Direct mode is only used when explicitly requested with --direct/-d
    // Otherwise, TUI (chat) mode is the default
    let use_tui = !cli.direct;

    if use_tui {
        // TUI mode (interactive chat - default)
        let initial_messages = Vec::new(); // Will be loaded if --resume is set

        // Create role from cli flags
        let role = if cli.yolo { Role::Yolo } else { Role::Safe };

        // Run TUI with integrated agent
        tui::run_tui_agent(model, tool_registry, initial_messages, role, cli.resume).await?;
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
    use agent::{DirectExecutor, run_agent_loop};
    use std::sync::Arc;

    // Initialize messages with system prompt and user task
    let system_prompt = build_system_prompt(tools_description, project_context, prompt_style);
    let initial_messages =
        model.create_messages(vec![("system", &system_prompt), ("user", initial_task)]);

    // Create context manager for token-aware message pruning (128k token limit)
    let mut context_manager = ContextManager::new(128_000, &system_prompt);

    // Add initial messages to context manager
    for msg in &initial_messages {
        if &*msg.role != "system" {
            context_manager.add_message(msg.clone());
        }
    }

    // Prune messages to fit (reserve 8k tokens for response)
    context_manager.prune_to_fit(8192);

    // Build initial messages from pruned context
    let mut messages = vec![initial_messages[0].clone()]; // Keep system prompt
    for msg in context_manager.get_messages() {
        messages.push(msg);
    }

    // Create tool registry for the executor (we create a new instance since ToolRegistry is not cloneable)
    // This is safe because create_coding_tools creates the same tools each time
    let tool_registry_owned = create_coding_tools(std::env::current_dir()?);
    let tool_registry_arc = Arc::new(tool_registry_owned);
    let executor = DirectExecutor::new(tool_registry_arc);

    // Run the shared agent loop with the same registry reference
    // Direct mode doesn't need streaming, so use default config
    let loop_result = run_agent_loop(
        &model,
        messages,
        tool_registry,
        executor,
        &Role::Yolo,
        agent::LoopConfig::new(),
    )
    .await
    .map_err(|e| format!("Agent loop error: {}", e))?;

    // Print final response if any
    if let Some(response) = loop_result.final_response {
        println!("{}", response);
        println!("\n{}", "═".repeat(60));
        println!();
    }

    println!("Executed {} actions", loop_result.actions_executed);
    Ok(())
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
            trimmed.starts_with("#")
                || trimmed.starts_with("This file provides")
                || trimmed.is_empty()
                || trimmed.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(cleaned)
}
