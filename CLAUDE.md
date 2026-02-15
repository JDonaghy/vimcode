## Session Start Protocol
1. Read `PROJECT_STATE.md` for current progress
2. Check `.opencode/specs/` for detailed feature specs before starting
3. Prompt user to update `PROJECT_STATE.md` after significant tasks

## Architecture

**VimCode**: Vim-like code editor in Rust with GTK4/Relm4. Clean separation: `src/core/` (platform-agnostic logic) vs `src/main.rs` (UI).

**Tech Stack:** Rust 2021, GTK4+Relm4, Ropey (text rope), Tree-sitter (parsing), Pango+Cairo (rendering)

**Critical Rule:** `src/core/` must NEVER depend on `gtk4`, `relm4`, or `pangocairo`. Must be testable in isolation.

## Data Model
```
Engine
├── BufferManager { HashMap<BufferId, BufferState> }
│   └── BufferState { buffer: Buffer, file_path, dirty, syntax, undo/redo }
├── windows: HashMap<WindowId, Window { buffer_id, view }>
├── tabs: Vec<Tab { layout: WindowLayout (binary tree), active_window }>
├── registers: HashMap<char, (String, bool)>  # (content, is_linewise)
└── State: mode, command_buffer, message, search_*, pending_key, pending_operator
```

**Concepts:** Buffer (in-memory file) | Window (viewport+cursor) | Tab (window layout) | Multiple windows can show same buffer.

## Commands
```bash
cargo build               # Compile
cargo test                # Run all tests
cargo clippy -- -D warnings  # Lint (must pass)
cargo fmt                 # Format
```

## Code Style
- `rustfmt` defaults (4-space indent)
- `PascalCase` types, `snake_case` functions/vars
- Core: Return `Result<T, E>` for I/O, silent no-ops for bounds
- Tests in `#[cfg(test)] mod tests` at file bottom

## Common Patterns

**Add Normal Mode Key:** `engine.rs` → `handle_normal_key()` → add match arm → test

**Add Command:** `engine.rs` → `execute_command()` → add match arm → test

**Add Operator+Motion:** Set `pending_operator` → implement in `handle_operator_motion()` → test

**Ctrl-W Command:** `handle_pending_key()` under `'\x17'` case

**Engine Facade Methods:** `buffer()`, `buffer_mut()`, `view()`, `view_mut()`, `cursor()` — all operate on active window's buffer
