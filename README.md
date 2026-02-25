# codr

A minimal AI agent for terminal use, built in Rust. Inspired by [minimal-agent.com](https://minimal-agent.com).

## Features

- Simple ~400 line core implementation
- TUI (Terminal User Interface) for interactive chat
- **Tool System**: 6 integrated tools for codebase interaction
  - `read` - Read files with offset/limit and image detection
  - `bash` - Execute shell commands
  - `edit` - Surgical find/replace in files
  - `write` - Create or overwrite files
  - `grep` - Search file contents with regex (.gitignore-aware)
  - `find` - Find files by glob pattern (.gitignore-aware)
- Support for multiple LLM providers:
  - **llama-server** (default) - Local OpenAI-compatible API from llama.cpp
  - **Anthropic Claude** - Cloud API
- TOML configuration file support

## Quick Start

### TUI Mode (Interactive Chat)

```bash
# Start llama-server in another terminal
llama-server --model /path/to/your/model.gguf

# Launch TUI (default when no task specified)
cargo run -- --chat

# Or with -c shorthand
cargo run -c

# TUI Keybindings:
# - Ctrl+S: Send message
# - Ctrl+Q: Quit
# - Arrow keys: Move cursor
# - Enter: Insert newline
```

### Direct Mode (Single Task)

```bash
# Run a single task non-interactively
cargo run -- "List all rust files in the current directory"

# With explicit flag
cargo run -- -- "Find all TODO comments"
```

## Tool System

The agent uses structured tool calls for operations:

### Tool Call Format

```
```tool-action
<tool_name>
<json_parameters>
```
```

### Available Tools

| Tool | Parameters | Description |
|------|------------|-------------|
| `read` | `file_path`, `offset`, `limit` | Read file contents |
| `bash` | `command`, `cwd` | Execute shell commands |
| `edit` | `file_path`, `old_text`, `new_text` | Find and replace text |
| `write` | `file_path`, `content` | Create/overwrite files |
| `grep` | `pattern`, `path`, `case_insensitive` | Search with regex |
| `find` | `pattern`, `path` | Find files by glob |

### Example Tool Calls

```markdown
Read a file:
```tool-action
read
{"file_path": "src/main.rs", "offset": 0, "limit": 100}
```

Search for TODOs:
```tool-action
grep
{"pattern": "TODO", "path": ".", "case_insensitive": true}
```

Find all Rust files:
```tool-action
find
{"pattern": "*.rs", "path": "."}
```

Execute bash command:
```bash-action
cargo test
```
```

## Configuration

The agent looks for configuration in this order:

1. `./codr.toml` (current directory)
2. `~/.config/codr/config.toml` (XDG config home)
3. Default configuration (llama on localhost:8080)

### Example `codr.toml`

```toml
# Which model to use: "llama" or "anthropic"
model = "llama"

[llama]
server_url = "http://localhost:8080"
model = "default"

[anthropic]
api_key = "sk-ant-..."
```

For Anthropic, you can also set `ANTHROPIC_API_KEY` environment variable instead of putting it in the config file.

## Architecture

The agent follows a simple loop:

```
┌─────────────────────────────────────────────────────────────┐
│  1. Send conversation history to LM                         │
│  2. LM proposes an action (tool call or bash command)       │
│  3. Parse and execute the action                            │
│  4. Feed output back to LM                                  │
│  5. Repeat until exit command                               │
└─────────────────────────────────────────────────────────────┘
```

## TUI Display

```
┌─ Chat ─────────────────────────────────────────────────────┐
│ [19:42] 👤 You                                              │
│     List all rust files                                     │
│                                                             │
│ [19:42] 🤖 codr                                             │
│     I'll find all Rust files in the project.                │
│                                                             │
│     ```tool-action                                          │
│     find                                                    │
│     {"pattern": "*.rs"}                                     │
│     ```                                                     │
│                                                             │
│ [19:42] 🔧 tool: find | {"pattern":"*.rs"}                  │
│ [19:42] 📤 Output                                           │
│     ./src/main.rs                                           │
│     ./src/error.rs                                          │
│     ./src/parser.rs                                         │
└─────────────────────────────────────────────────────────────┘
┌─ Input (Ctrl+S to send, Ctrl+Q to quit) ──────────────────┐
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## License

MIT
