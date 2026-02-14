# AGENTS.md

This document provides detailed instructions, workflows, and guidelines for AI agents (and human developers) working on the **VimCode** repository.

## 1. Global Instructions

- At the start of every session, read `PROJECT_STATE.md` to understand the current progress and roadmap.
- Before finishing a significant task, prompt the user to update `PROJECT_STATE.md`.
- Always check `.opencode/specs/` for detailed feature requirements before starting an Epic.

## 2. Project Overview & Architecture

VimCode is a high-performance, cross-platform code editor built with Rust. It emphasizes a clean separation between the editor logic and the UI layer.

### Core Technologies
-   **Language:** Rust (2021 edition)
-   **UI Framework:** [GTK4](https://gtk-rs.org/) via [Relm4](https://relm4.org/)
-   **Text Engine:** [Ropey](https://github.com/cessen/ropey) (immutable text rope for efficient editing)
-   **Parsing:** Tree-sitter (for robust syntax highlighting)
-   **Rendering:** Pango + Cairo (via `gtk4::DrawingArea` for custom text rendering)

### Architectural Boundaries

1.  **`src/core/` (The Engine):**
    -   Contains strictly platform-agnostic logic.
    -   **Rule:** This directory *must not* depend on `gtk4`, `relm4`, or `pangocairo`. It should be testable in isolation.

2.  **`src/main.rs` (The UI):**
    -   Handles the application lifecycle, window management, and input events.
    -   **Pattern:** Uses `Relm4`'s `SimpleComponent` trait.
    -   **State:** Holds the `Engine` inside an `Rc<RefCell<Engine>>`.
    -   **Rendering:** Custom rendering via `pangocairo` — no GTK text widgets.

### Core Data Model

```
Engine
├── BufferManager                      # Owns all buffers
│   └── HashMap<BufferId, BufferState>
│       └── BufferState
│           ├── buffer: Buffer         # Rope-based text content
│           ├── file_path: Option<PathBuf>
│           ├── dirty: bool
│           ├── syntax: Syntax         # Tree-sitter parser
│           ├── highlights: Vec<(usize, usize, String)>
│           ├── undo_stack: Vec<UndoEntry>    # Undo history
│           ├── redo_stack: Vec<UndoEntry>    # Redo history
│           └── current_undo_group: Option<UndoEntry>
│
├── windows: HashMap<WindowId, Window> # All windows across all tabs
│   └── Window
│       ├── buffer_id: BufferId        # Which buffer this window shows
│       └── view: View                 # Cursor, scroll position
│
├── tabs: Vec<Tab>                     # Tab pages
│   └── Tab
│       ├── layout: WindowLayout       # Binary split tree
│       └── active_window: WindowId
│
├── registers: HashMap<char, (String, bool)>  # Yank/delete storage (content, is_linewise)
├── selected_register: Option<char>           # Set by "x prefix
│
└── Global state
    ├── mode: Mode                     # Normal, Insert, Command, Search
    ├── command_buffer: String         # Current :command or /search
    ├── message: String                # Status message
    ├── search_query: String
    ├── search_matches: Vec<(usize, usize)>
    └── pending_key: Option<char>      # For multi-key sequences (gg, dd, "x)
```

### Key Concepts

-   **Buffer:** In-memory file content. Persists until explicitly deleted with `:bd`.
-   **Window:** A viewport into a buffer. Has its own cursor and scroll position.
-   **Tab:** A layout of windows. Each tab can have multiple split windows.
-   **View:** Per-window state (cursor position, scroll offset).

Multiple windows can show the same buffer with independent cursors.

### File Structure

```
src/
├── main.rs                 # GTK4/Relm4 UI, rendering (~550 lines)
└── core/
    ├── mod.rs              # Module declarations
    ├── engine.rs           # Engine: orchestrates everything (~2950 lines)
    ├── buffer.rs           # Buffer: Rope-based text storage
    ├── buffer_manager.rs   # BufferManager: owns all buffers
    ├── view.rs             # View: per-window cursor/scroll
    ├── window.rs           # Window, WindowLayout (split tree)
    ├── tab.rs              # Tab: window layout container
    ├── cursor.rs           # Cursor position (line, col)
    ├── mode.rs             # Mode enum
    └── syntax.rs           # Tree-sitter parsing
```

## 3. Build, Test, and Lint Commands

Agents should verify changes frequently using these commands.

### Basic Workflow
```bash
cargo build              # Compile
cargo run -- <file>      # Run with a file
```

### Testing Strategy
```bash
cargo test                           # Run all 88 tests
cargo test test_buffer_editing       # Run single test
cargo test core::engine::tests::     # Run all engine tests
```

-   Place unit tests in `#[cfg(test)] mod tests { ... }` at the bottom of each file.
-   Ensure core logic has high test coverage since it's UI-independent.

### Quality Assurance
```bash
cargo fmt                      # Format code
cargo clippy -- -D warnings    # Lint (must pass)
```

## 4. Code Style & Conventions

### General Rust Style
-   **Formatting:** `rustfmt` defaults, 4-space indentation.
-   **Naming:** `PascalCase` for types, `snake_case` for functions/vars.
-   **Ordering:** imports → structs → impl blocks → tests module

### Import Convention
-   Group imports by crate (std, external, internal).
-   In `src/core/`, prefer explicit imports over wildcards.
-   Preludes (`gtk4::prelude::*`) are OK in `main.rs`.

### Error Handling
-   **Core Logic:** Return `Result<T, E>` for I/O. Prefer silent no-ops for bounds checking.
-   **UI Logic:** Use `unwrap()` only when failure is mathematically impossible.

## 5. Common Tasks

### Adding a New Command (`:cmd`)

1.  **engine.rs** → `execute_command()`: Add a match arm for the command.
2.  If the command needs new state, add fields to `Engine` or `BufferManager`.
3.  Add a test in `engine.rs` tests module.

### Adding a New Normal Mode Key

1.  **engine.rs** → `handle_normal_key()`: Add a match arm.
2.  For multi-key sequences (like `gg`), use `pending_key`.
3.  Add a test.

### Adding a Ctrl-W Window Command

1.  **engine.rs** → `handle_pending_key()` under the `'\x17'` (Ctrl-W) case.
2.  Call the appropriate method (`split_window`, `close_window`, etc.).

### Adding a New Buffer/Window Operation

1.  Add method to `Engine` (e.g., `engine.new_operation()`).
2.  Use `self.active_window_id()`, `self.active_buffer_id()` to get current context.
3.  Use `self.buffer()` / `self.buffer_mut()` for buffer access.
4.  Use `self.view()` / `self.view_mut()` for cursor/scroll access.

### Modifying Window Layout

-   `WindowLayout` is a binary tree (see `window.rs`).
-   `split_at()` — insert a split at a window.
-   `remove()` — remove a window, promoting sibling.
-   `calculate_rects()` — get pixel bounds for rendering.

### Adding UI Rendering

1.  **main.rs** → modify `draw_editor()` or add helper functions.
2.  Use `engine.calculate_window_rects()` to get window bounds.
3.  For per-window rendering, iterate over `window_rects`.

## 6. Environment & Constraints

-   **Platform:** Linux / WSLg.
-   **Rendering:** CPU-based (Cairo). Avoid GPU-specific calls.
-   **Performance:**
    -   Rendering is called every frame — keep it optimized.
    -   Syntax re-parsing happens on every buffer change (incremental parsing is TODO).

## 7. Facade Methods on Engine

For backward compatibility and convenience, `Engine` provides facade methods:

```rust
engine.buffer()          // &Buffer for active window
engine.buffer_mut()      // &mut Buffer
engine.view()            // &View (cursor, scroll)
engine.view_mut()        // &mut View
engine.cursor()          // &Cursor (shorthand for view().cursor)
engine.file_path()       // Option<&PathBuf>
engine.dirty()           // bool
engine.set_dirty(bool)
engine.viewport_lines()  // usize
engine.set_viewport_lines(usize)
engine.update_syntax()   // Re-parse active buffer
engine.save()            // Save active buffer to file
```

These all operate on the **active window's buffer**.
