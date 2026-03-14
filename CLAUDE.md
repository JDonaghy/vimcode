## Session Start Protocol
1. Read `PROJECT_STATE.md` for current progress
2. Check `.opencode/specs/` for detailed feature specs before starting
3. Prompt user to update `PROJECT_STATE.md` after significant tasks

## Documentation Maintenance (MANDATORY)
After completing any feature or significant change, update ALL of these files:
- **`README.md`** — the primary user-facing reference; keep the feature tables, key reference, and command list accurate and complete; update the test count in the intro line
- **`PROJECT_STATE.md`** — internal progress tracker; update session date, test counts, file sizes, recent work entry, and roadmap checkboxes
- **`PLAN.md`** — update recently completed section at top; tick off roadmap items
- **`EXTENSIONS.md`** — extension development guide; update if any Lua API functions, events, manifest fields, or plugin loading behavior change

**README.md update rules:**
- Add new keys/commands to the appropriate Key Reference table
- Add new `:` commands to the Command Mode table
- Add new git commands to the git commands table
- Add new settings to the settings table
- Update architecture section if new files are added or line counts change significantly
- Do NOT add speculative/planned features — only document what is implemented

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

## Branching & Releases
- All work happens on `develop`; `main` is the release branch
- Merge `develop` → `main` via GitHub PR (CI runs on the PR before release)
- Before creating the PR: bump version in `Cargo.toml` (minor for features, patch for fixes)
- Merging the PR to `main` triggers `release.yml` which creates a GitHub Release tagged `v$VERSION`
- Never push directly to `main` — always merge from `develop` via PR

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

**Show User-Facing Info (About, errors, confirmations):** Use the modal dialog system (`show_dialog()` / `show_error_dialog()`) rather than `self.message`. Dialogs are preferred for anything that deserves user attention — the message bar is for transient status only.

**Add New Setting:** When adding a new user-configurable setting, update ALL FOUR of these:
1. Add field to `Settings` struct in `settings.rs` with `#[serde(default = "default_fn_name")]`
2. Create default function returning sensible default value
3. Update `Default` impl to include the field
4. Add to `get_value_str()` and `set_value_str()` in `settings.rs`
5. Add a `SettingDef` entry to `SETTING_DEFS` in `render.rs` (controls the Settings sidebar UI)
6. Settings are automatically merged: new fields are added to existing settings files without overwriting user values
7. Document the setting name and purpose in comments
