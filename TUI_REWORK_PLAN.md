# TUI Rework Plan

## Overview

Complete redesign and refactoring of TUI to:
- Integrate shared agent loop (eliminate deferred work)
- Improve visual style with new theming system
- Reduce code complexity through modularization
- Unify update systems (AgentUpdate + TuiUpdate)

## Current Issues

1. **Code Duplication**: ~3300 lines in `tui.rs`, ~58000 lines in `tui_components.rs`
2. **Incompatible Update Systems**:
   - TUI's `AgentUpdate` enum (10+ variants)
   - Shared's `TuiUpdate` enum (6 variants)
   - Requires mapping/conversion between the two
3. **Monolithic Structure**: Everything in one file makes navigation difficult
4. **Limited Theming**: Hardcoded colors, no easy customization
5. **Deferred Integration**: TUI still uses old `agent_loop()` (~400 lines)

## Proposed Architecture

```
src/tui/
├── mod.rs                  # Main entry point, App struct, state management
├── events.rs               # Keyboard/mouse event handling
├── renderer.rs             # Drawing logic, theme application
├── updates.rs              # Unified update types (AgentUpdate + TuiUpdate)
├── agent.rs                # TUI agent loop using shared components
├── widgets.rs              # Reusable UI components
│   ├── mod.rs
│   ├── conversation.rs      # Message rendering, markdown
│   ├── input.rs            # Command input, history
│   ├── banner.rs           # Logo, model info, status
│   └── status.rs           # Toasts, progress indicators
├── theme.rs                # Color schemes, style definitions
└── utils.rs                # Helper functions (fuzzy, selection, etc.)
```

## Phase 1: Architecture & Types

### 1.1 Create Unified Update System

```rust
// src/tui/updates.rs
#[derive(Debug, Clone)]
pub enum TuiUpdate {
    // Core updates (from shared agent)
    ActionMessage(Arc<str>),
    ToolProgress(Arc<str>),
    OutputMessage(Arc<String>),
    ErrorMessage(Arc<str>),
    NeedsApproval { action_type: Arc<str>, content: Arc<String>, is_tool: bool },

    // TUI-specific updates (from UI events)
    StreamingChunk(Arc<str>),
    StreamingThinkingChunk(Arc<str>),
    SystemMessage(Arc<str>),
    UsageUpdate { input_tokens: u32, output_tokens: u32, cost: f64 },
    ParallelToolCount(usize),
    Done,
}
```

### 1.2 Split tui.rs into modules

```
tui/mod.rs          → App struct, main loop (500 lines)
tui/events.rs       → Event handling (200 lines)
tui/renderer.rs     → Drawing logic (400 lines)
tui/updates.rs      → Update types (100 lines)
tui/agent.rs        → Agent integration (300 lines)
tui/theme.rs        → Theme definitions (200 lines)
tui/widgets/
  mod.rs            → Widget exports (50 lines)
  conversation.rs    → Messages, markdown (1000 lines)
  input.rs          → Command input (500 lines)
  banner.rs         → Logo, status (300 lines)
  status.rs         → Toasts, progress (200 lines)
```

## Phase 2: Theme System

### 2.1 New Theme Structure

```rust
// src/tui/theme.rs
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    // Base colors
    pub background: Color,
    pub foreground: Color,
    pub dimmed: Color,

    // Syntax highlighting
    pub code_keyword: Color,
    pub code_string: Color,
    pub code_comment: Color,
    pub code_function: Color,
    pub code_number: Color,

    // UI elements
    pub primary: Color,
    pub secondary: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // Message types
    pub user_message: Color,
    pub assistant_message: Color,
    pub system_message: Color,
    pub thinking_message: Color,
    pub action_message: Color,
    pub output_message: Color,
}

pub const THEME_DARK: Theme = Theme { /* ... */ };
pub const THEME_LIGHT: Theme = Theme { /* ... */ };
pub const THEME_DRACULA: Theme = Theme { /* ... */ };
pub const THEME_CATPPUCCIN: Theme = Theme { /* ... */ };
```

### 2.2 Theme Application

```rust
// Apply theme throughout UI
impl Theme {
    pub fn style_for_message_type(&self, role: &str) -> Style {
        match role {
            "user" => Style::default().fg(self.user_message),
            "assistant" => Style::default().fg(self.assistant_message),
            "system" => Style::default().fg(self.system_message),
            "thinking" => Style::default().fg(self.thinking_message).add_modifier(Modifier::ITALIC),
            _ => Style::default().fg(self.dimmed),
        }
    }

    pub fn syntax_style(&self, token_type: SyntaxToken) -> Style {
        match token_type {
            SyntaxToken::Keyword => Style::default().fg(self.code_keyword),
            SyntaxToken::String => Style::default().fg(self.code_string),
            // ... etc
        }
    }
}
```

## Phase 3: Agent Integration

### 3.1 Use Shared Agent Loop

```rust
// src/tui/agent.rs
use crate::agent::{run_agent_loop_streaming, StreamingCallback, ThinkingCallback};

pub async fn run_tui_agent(
    model: Model,
    tool_registry: Arc<ToolRegistry>,
    conversation: Vec<Message>,
    tx: mpsc::UnboundedSender<TuiUpdate>,
    cancel_token: CancellationToken,
    role: Role,
) {
    // Create TUI executor
    let executor = TUIExecutor::new(tool_registry, tx, role);

    // Streaming callbacks
    let tx_streaming = tx.clone();
    let tx_thinking = tx.clone();

    let on_streaming: StreamingCallback = Arc::new(move |chunk| {
        let _ = tx_streaming.send(TuiUpdate::StreamingChunk(chunk));
    });

    let on_thinking: ThinkingCallback = Arc::new(move |thinking| {
        let _ = tx_thinking.send(TuiUpdate::StreamingThinkingChunk(thinking));
    });

    // Run shared loop
    let result = run_agent_loop_streaming(
        &model,
        conversation,
        &tool_registry,
        executor,
        &role,
        on_streaming,
        on_thinking,
    ).await;

    // Handle result
    match result {
        Ok(r) => {
            // Send final response if needed
            if let Some(response) = r.final_response {
                let _ = tx.send(TuiUpdate::StreamingChunk(Arc::new(response)));
            }
        }
        Err(e) => {
            let _ = tx.send(TuiUpdate::ErrorMessage(Arc::new(e)));
        }
    }

    let _ = tx.send(TuiUpdate::Done);
}
```

### 3.2 Update agent::TUIExecutor to use unified TuiUpdate

```rust
// src/agent/tui_executor.rs
// Change TuiUpdate to use tui::updates::TuiUpdate instead
use crate::tui::updates::TuiUpdate;

impl TUIExecutor {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
        tx: mpsc::UnboundedSender<TuiUpdate>,  // Now unified type
        role: crate::tools::Role,
    ) -> Self { /* ... */ }
}
```

## Phase 4: Modular Widgets

### 4.1 Conversation Widget

```rust
// src/tui/widgets/conversation.rs
pub struct ConversationWidget<'a> {
    messages: &'a [ChatMessage],
    theme: &'a Theme,
    scroll_state: ScrollState,
}

impl<'a> ConversationWidget<'a> {
    pub fn new(messages: &'a [ChatMessage], theme: &'a Theme) -> Self {
        Self {
            messages,
            theme,
            scroll_state: ScrollState::default(),
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Render messages with theme-based styling
        // Support markdown, code blocks, thinking blocks
    }
}
```

### 4.2 Input Widget

```rust
// src/tui/widgets/input.rs
pub struct InputWidget {
    input: String,
    cursor_position: usize,
    theme: Theme,
    history: Vec<String>,
    history_index: Option<usize>,
}

impl InputWidget {
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Styled input box with theme
        // Cursor rendering
        // File/command picker overlay
    }
}
```

## Phase 5: Visual Redesign

### 5.1 New Color Palette

Based on modern terminal themes:

**Dracula** (dark):
```
Background: #282a36
Foreground: #f8f8f2
Primary: #bd93f9 (purple)
Secondary: #8be9fd (cyan)
Success: #50fa7b (green)
Warning: #ffb86c (orange)
Error: #ff5555 (red)
```

**Catppuccin Mocha** (dark):
```
Background: #1e1e2e
Foreground: #cdd6f4
Primary: #cba6f7 (mauve)
Secondary: #89b4fa (blue)
Success: #a6e3a1 (green)
Warning: #ef9f76 (peach)
Error: #f38ba8 (red)
```

**Tokyo Night** (dark):
```
Background: #1a1b26
Foreground: #a9b1d6
Primary: #7aa2f7 (blue)
Secondary: #c0caf5 (cyan)
Success: #9ece6a (green)
Warning: #e0af68 (yellow)
Error: #f7768e (red)
```

### 5.2 UI Improvements

1. **Better Message Separation**:
   - Distinct borders or spacing between messages
   - User vs assistant vs system message differentiation

2. **Enhanced Code Blocks**:
   - Syntax highlighting with theme colors
   - Line numbers
   - Copy button indicator

3. **Improved Progress Indicators**:
   - Animated spinners for tool execution
   - Clear status bars
   - Better visual feedback

4. **Modern Borders**:
   - Rounded corners
   - Double lines for emphasis
   - Gradient borders (if supported)

5. **Better Notifications**:
   - Toast messages with animations
   - Clear visual hierarchy (error > warning > info)
   - Auto-dismiss timing

## Implementation Order

1. ✅ **Phase 1**: Create modular structure (tui/mod.rs, tui/updates.rs)
2. **Phase 2**: Implement theme system (tui/theme.rs)
3. **Phase 3**: Update shared agent executor (agent/tui_executor.rs)
4. **Phase 4**: Create base widgets (tui/widgets/mod.rs)
5. **Phase 5**: Implement conversation widget
6. **Phase 6**: Implement input widget
7. **Phase 7**: Create agent integration (tui/agent.rs)
8. **Phase 8**: Update main App struct to use new modules
9. **Phase 9**: Apply new theme and style
10. **Phase 10**: Test and remove old code

## Benefits

1. **Code Organization**:
   - Easier navigation (modular files)
   - Clear separation of concerns
   - Reduced file sizes (3300 → ~500 lines in mod.rs)

2. **Maintenance**:
   - Easier to find and fix bugs
   - Simpler to add new features
   - Clear dependency flow

3. **Integration**:
   - Unified update system eliminates compatibility issues
   - Shared agent loop removes ~400 lines of duplication
   - Consistent behavior across all modes

4. **Visual Quality**:
   - Modern, attractive appearance
   - Easy theme customization
   - Better user experience

5. **Testing**:
   - Smaller modules are easier to test
   - Clear boundaries for unit tests
   - Better test coverage

## Migration Strategy

1. **Gradual Migration**: Keep old `tui.rs` while building new structure
2. **Feature Flags**: Use cfg to switch between old/new implementations
3. **Parallel Testing**: Run both versions to compare behavior
4. **Clean Removal**: Once stable, remove old code in final commit

## Estimated Timeline

- Phase 1 (Architecture): 2-3 hours
- Phase 2 (Themes): 1-2 hours
- Phase 3 (Agent): 2-3 hours
- Phase 4 (Base Widgets): 3-4 hours
- Phase 5-6 (Complex Widgets): 4-6 hours
- Phase 7 (Integration): 2-3 hours
- Phase 8 (App Update): 2-3 hours
- Phase 9 (Styling): 3-4 hours
- Phase 10 (Testing): 2-3 hours

**Total**: 21-31 hours (~3-4 days of focused work)
