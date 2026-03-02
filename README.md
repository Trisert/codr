# codr

A minimal, terminal-based AI coding agent built in Rust. It gives large language models the ability to interact with your local codebase directly through a set of built-in tools. Designed to be lightweight and stay out of your way.

## Overview

codr runs directly in your terminal and provides an interactive interface for collaborating with AI models. Instead of manually copying and pasting code back and forth, you can ask the agent to search your files, read code, run shell commands, and make edits on its own.

### Key Features

- Interactive Terminal UI: A clean, responsive interface using Ratatui. It supports real-time streaming, separates "thinking" blocks from actual responses, and provides full message history.
- Tool System: The agent uses XML-formatted tool calls to interact with your system.
  - File operations: read, write, edit, and file_info
  - Searching: grep and find (both respect .gitignore)
  - System: execute bash commands
- Multi-Provider Support: Works out of the box with:
  - OpenAI-compatible endpoints, which makes it perfect for local models running on llama.cpp.
  - Anthropic API for models like Claude.
- Safety Roles: You control how much freedom the agent has. You can cycle between these roles using Shift+Tab:
  - SAFE (Default): The agent can read and search freely, but needs your explicit approval before modifying files or running shell commands.
  - PLAN: Strictly read-only mode. Perfect for exploring large codebases or planning architectures without risking accidental changes.
  - YOLO: Full autonomy. The agent executes edits and bash commands automatically without waiting for your approval.

## Getting Started

### Prerequisites

You will need the Rust toolchain installed. On Linux, depending on your environment, you might also need X11 dependencies for clipboard support (e.g., libxcb-render0-dev, libxcb-shape0-dev, libxcb-xfixes0-dev).

### Building

Clone the repository and build the release binary:

```bash
git clone https://github.com/Trisert/codr.git
cd codr
cargo build --release
```

The compiled binary will be located in `target/release/codr`.

### Configuration

codr looks for its configuration file in the following order:
1. A `codr.toml` file in the current working directory.
2. A global config at `~/.config/codr/config.toml`.
3. If no config is found, it defaults to looking for a local OpenAI-compatible server at `http://localhost:8080` (the default for llama-server).

Here is an example `codr.toml` configuration:

```toml
# Choose your active provider: "openai" or "anthropic"
model = "openai"

[openai]
base_url = "http://localhost:8080"
model = "default"
# api_key = "sk-..." # Only needed if using the actual OpenAI API or a remote service

[anthropic]
api_key = "sk-ant-..." # Alternatively, set the ANTHROPIC_API_KEY environment variable
```

## Usage

### Interactive TUI Mode

To start the chat interface, run:

```bash
codr
# or run in direct mode for a single task
codr -d "Your task here"
```

Basic keybindings in the TUI:
- Ctrl+S: Send your message
- Shift+Tab: Cycle through agent roles (PLAN -> SAFE -> YOLO)
- Up/Down Arrows: Navigate through your input history
- a / r: Approve or reject pending bash commands or file edits (when in SAFE mode)
- Ctrl+C: Cancel the current agent operation (press twice to quit)
- Ctrl+Q: Quit the application
- Ctrl+O: Copy the current selection to your clipboard

### Direct Mode

You can also run codr for a single, non-interactive task directly from your shell:

```bash
codr "Find all TODO comments in the src directory and list them"
```

## How It Works

The architecture relies on a simple background agent loop:
1. codr sends the conversation history to the model along with a dynamically generated system prompt describing available tools.
2. The model decides whether to provide a plain text response or output an XML block representing a tool call (e.g., `<codr_bash>cargo check</codr_bash>`).
3. codr parses the response. If it is a tool call, it executes the tool locally.
4. The output of the tool is appended to the conversation history, and the loop repeats until the model finishes its task.

## License

This project is licensed under the MIT License.