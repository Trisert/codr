# codr

A minimal AI agent for terminal use, built in Rust. Inspired by [minimal-agent.com](https://minimal-agent.com).

## Features

- ~400 line core implementation
- TUI (Terminal User Interface) for interactive chat
- **Tool System**: 7 integrated tools for codebase interaction
  - `read` - Read files with offset/limit and image detection
  - `bash` - Execute shell commands
  - `edit` - Surgical find/replace in files
  - `write` - Create or overwrite files
  - `grep` - Search file contents with regex (.gitignore-aware)
  - `find` - Find files by glob pattern (.gitignore-aware)
  - `file_info` - Get file metadata
- Support for multiple LLM providers:
  - **OpenAI** (default) - Local llama.cpp or any OpenAI-compatible API
  - **Anthropic Claude** - Cloud API
  - **NVIDIA NIM** - NVIDIA endpoints
- Role system: PLAN (read-only), SAFE (default), YOLO (full access)

## Quick Start

### 1. Start a model server

```bash
# Option A: Local llama.cpp
llama-server --model /path/to/your/model.gguf

# Option B: Use remote OpenAI-compatible API
```

### 2. Run codr

```bash
# Interactive chat (TUI mode)
cargo run --chat

# Direct mode (single task)
cargo run -- "List all rust files"
```

### TUI Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+S` | Send message |
| `Ctrl+Q` | Quit |
| `Shift+Tab` | Cycle roles (PLAN → SAFE → YOLO) |
| `Ctrl+C` | Cancel agent (press twice to quit) |
| `Ctrl+C` | Cancel agent (press twice to quit) |
| Arrow keys | Navigate prompt history / move cursor |
| `Enter` | Insert newline |
| `Ctrl+O` | Copy selection to clipboard |
| `Ctrl+Y` | Paste from clipboard |

## Configuration

The agent looks for configuration in this order:

1. `./codr.toml` (current directory)
2. `~/.config/codr/config.toml` (XDG config home)
3. Default configuration (OpenAI on localhost:8080)

### Example `codr.toml`

```toml
model = "openai"  # or "anthropic" or "nim"

[openai]
base_url = "http://localhost:8080"
model = "default"
api_key = "..."   # optional, for remote APIs

[anthropic]
api_key = "sk-ant-..."  # or set ANTHROPIC_API_KEY env var

[nim]
base_url = "https://integrate.api.nvidia.com"
model = "meta/llama-3.1-70b-instruct"
api_key = "..."  # or set NVIDIA_API_KEY env var
```

## Tool System

Tools are called using XML-style blocks:

```
<codr_tool name="tool_name">{"param": "value"}</codr_tool>
<codr_bash>command</codr_bash>
```

| Tool | Parameters | Description |
|------|------------|-------------|
| `read` | `file_path`, `offset`, `limit` | Read file contents |
| `bash` | `command`, `cwd`, `timeout`, `env` | Execute shell commands |
| `edit` | `file_path`, `old_text`, `new_text` | Find and replace text |
| `write` | `file_path`, `content` | Create/overwrite files |
| `grep` | `pattern`, `path`, `include` | Search with regex |
| `find` | `pattern`, `path` | Find files by glob |
| `file_info` | `file_path` | Get file metadata |

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

## License

MIT
