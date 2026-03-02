# codr

```
                          $$\           
                          $$ |          
 $$$$$$$\  $$$$$$\   $$$$$$$ | $$$$$$\  
$$  _____|$$  __$$\ $$  __$$ |$$  __$$\ 
$$ /      $$ /  $$ |$$ /  $$ |$$ |  \__|
$$ |      $$ |  $$ |$$ |  $$ |$$ |      
\$$$$$$$\ \$$$$$$  |\$$$$$$$ |$$ |      
 \_______| \______/  \_______|\__|      
```

A minimal, terminal-based AI coding agent built in Rust. It gives large language models the ability to interact with your local codebase directly through a set of built-in tools.

## Features

- **Interactive TUI**: Real-time streaming with Ratatui, separate thinking blocks, full message history
- **Tool System**: XML-formatted tool calls for file operations (read, write, edit), search (grep, find), and bash execution
- **Multi-Provider**: OpenAI-compatible endpoints (local llama.cpp) or Anthropic API
- **Safety Roles**: 
  - `PLAN` (read-only)
  - `SAFE` (default - approvals required)
  - `YOLO` (full autonomy)

## Installation

```bash
git clone https://github.com/Trisert/codr.git
cd codr
cargo build --release
```

The binary is at `target/release/codr`.

## Configuration

Config priority: `./codr.toml` > `~/.config/codr/config.toml` > defaults to `http://localhost:8080` (llama.cpp).

Example `codr.toml`:
```toml
model = "openai"

[openai]
base_url = "http://localhost:8080"
model = "default"

[anthropic]
api_key = "sk-ant-..."
```

## Usage

```bash
codr              # Interactive mode
codr -d "task"    # Direct mode
codr --yolo       # Auto-approve all actions
```

### Keybindings

- `Ctrl+S` - Send message
- `Shift+Tab` - Cycle roles
- `Up/Down` - Input history
- `a` / `r` - Approve/reject (SAFE mode)
- `Ctrl+C` - Cancel / Quit
- `Ctrl+Q` - Quit
- `Ctrl+O` - Copy to clipboard

## License

MIT
