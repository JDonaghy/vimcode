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

## Quality Checks (MANDATORY Before Commits)
**CRITICAL:** After making ANY code changes and before creating commits, ALWAYS run:
1. `cargo fmt` - Format code
2. `cargo clippy -- -D warnings` - Check linting (must have zero warnings)
3. `cargo test` - Verify all tests pass
4. `cargo build` - Ensure compilation succeeds

If any check fails, fix immediately and re-run. Only commit when ALL checks pass.
This prevents CI failures and maintains code quality.

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

**Add New Setting:** When adding a new user-configurable setting:
1. Add field to `Settings` struct in `settings.rs` with `#[serde(default = "default_fn_name")]`
2. Create default function returning sensible default value
3. Update `Default` impl to include the field
4. Settings are automatically merged: new fields are added to existing settings files without overwriting user values
5. Document the setting name and purpose in comments
