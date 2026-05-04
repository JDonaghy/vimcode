## Platform-Neutrality Rule (MANDATORY — overrides all other guidance)

**NEVER add per-backend code to vimcode to fix a problem.** If a feature requires new code in `src/gtk/`, `src/tui_main/`, or `src/win_gui/` beyond thin event-to-engine wiring, STOP. Do not attempt the fix. Instead:

1. Identify what quadraui infrastructure is missing.
2. File a quadraui issue describing the gap.
3. Build the infrastructure in quadraui first.
4. Only then implement the vimcode side through the shared API.

This applies to scroll routing, hit-testing, event dispatch, layout math, coordinate transforms — everything. The goal is zero platform-specific logic in vimcode. Both TUI and GTK must behave identically from the same shared code path.

**Push back actively.** If the user asks to implement something that would require per-backend code, say so upfront and propose the quadraui-first alternative. Do not attempt a per-backend fix and fail — refuse the approach and explain what quadraui needs first.

**Why this rule exists.** Multiple sessions of effort were wasted adding GTK-specific scroll handlers, hit-test math, and coordinate conversions that never converged. Per-backend fixes are band-aids that create the exact duplication quadraui exists to eliminate. The lesson: if it can't be done through quadraui's backend-independent API, it can't be done yet — and the right action is to build the API.

## Session Start Protocol
1. Read `PROJECT_STATE.md` for current progress
2. Read `PLAN.md` if present — it is the pickup doc for any in-flight multi-stage feature (e.g. the current `quadraui` wave). The "Architectural focus" header at the top names the active design axis, lists resolved/open architectural questions, and points at the deeper docs; the stage map below tracks shipped/next work. Read both.
3. **If the work touches `quadraui/`** — read [`DECISIONS.md`](https://github.com/JDonaghy/quadraui/blob/main/quadraui/docs/DECISIONS.md) (primitive-distinctness principles) and [`BACKEND_TRAIT_PROPOSAL.md`](https://github.com/JDonaghy/quadraui/blob/main/quadraui/docs/BACKEND_TRAIT_PROPOSAL.md) §9 (resolved decisions log) in the quadraui repo. These define the architectural contract; ignoring them produces bandaid fixes that recur in the next backend.
4. Check `.opencode/specs/` for detailed feature specs before starting
5. Run `gh issue list --state open` and `gh issue list --state open --milestone` to see active work, milestones, and priorities
6. Prompt user to update `PROJECT_STATE.md` and `PLAN.md` after significant tasks

## Development Workflow

All non-trivial work should be tracked via GitHub Issues. Issues are the source of truth for what needs doing, why, and what the design is.

**Documentation-only changes** (pure `.md` edits, or comment-only edits in source files) may be committed directly to `develop` and pushed. No branch, no smoke test, no path decision. This includes `PLAN.md`, `PROJECT_STATE.md`, `README.md`, `docs/*.md`, and `CLAUDE.md` itself. If any code changes accompany the doc edit — even a one-line code change — use the full branch workflow below.

**For all other changes (issue work, bug fixes, code changes, release prep):**

1. **Always work on a local branch off `develop`.** Never commit code directly to `develop`. Branch naming:
   - Issue work: `issue-{number}-{short-description}`
   - Other work: `{kind}-{short-description}` (e.g. `fix-integration-tests-1based`, `docs-workflow-branch-required`, `release-0.11.0`)
2. Do the work on that branch, committing as you go.
3. **Do NOT push the branch yet.** Keep it local until one of the following applies:
   - **(a)** The user has run smoke tests and confirmed the changes work, OR
   - **(b)** The user has explicitly agreed that smoke testing is not needed (e.g. test-only, doc-only, or config-only changes where the change itself is self-verifying).
   Offer smoke tests explicitly and wait for approval.
4. **Once approved, ask the user which landing path they want:**
   - **Path A — merge locally + push.** For small / trivial changes (test fixes, typo fixes, doc updates): fast-forward-merge the branch into `develop` locally with `git merge --ff-only <branch>`, push `develop`, delete the branch. No PR, no separate review.
   - **Path B — push branch + open PR.** For normal feature or bugfix work, and anything closing an issue: push the branch, open a PR to `develop` with `gh pr create`. If the work closes an issue, reference it with "Closes #{number}" in the PR body. User reviews and merges.
5. **When the user confirms a merge that closes an issue**, immediately close the issue with `gh issue close <number> -c "Implemented in PR #N"` — do not rely on GitHub auto-close.

**Why both paths exist:** Path A is lower ceremony for changes so small that a PR review adds no information (a 2-line test fix where the fix *is* the verification). Path B is the default for work that warrants a review artifact, closes an issue, or is large enough that someone might want to see the diff separately from the merge commit. **When in doubt, default to Path B.**

**Creating issues:**
- At session end, create issues for any planned but unstarted work discussed during the session
- Include full design context in the issue body — file paths, API details, expected behavior, Neovim reference values
- Use milestones to group related work (e.g., "Vim Conformance")
- Use labels for categorization (`conformance`, `testing`, `bug:vim-deviation`, `lua-api`, etc.)
- Issues should be self-contained — a new session should be able to pick one up and implement it from the issue body alone

**Bug fixes found during other work:**
- If a bug is found while working on something else, create a separate issue for it
- Fix it on the current branch if it's small and directly related, or leave it for a separate branch if it's independent

## Documentation Maintenance (MANDATORY)
After completing any feature or significant change, update ALL of these files:
- **`README.md`** — the primary user-facing reference; keep the feature tables, key reference, and command list accurate and complete; update the test count in the intro line
- **`PROJECT_STATE.md`** — internal progress tracker; update session date, test counts, file sizes, recent work entry, and roadmap checkboxes
- **GitHub Issues** — close completed issues, create new ones for planned work; update milestones as needed. Issues are the source of truth for individual tasks.
- **`PLAN.md`** — session-level coordination doc for in-flight multi-stage features (extractions, refactors, waves that span several sub-stages). Update after every session that advances a stage: mark completed stages ✅ with their commit SHA, adjust scope notes, bump the date. When the active wave finishes, move the section to a completed list or delete (git history retains it).
- **`EXTENSIONS.md`** — extension development guide; update if any Lua API functions, events, manifest fields, or plugin loading behavior change
- **`SUMMARIES/`** — update any summary file whose source file was modified (new methods, changed types, significant line count changes); see below

## Code Summaries (`SUMMARIES/`)
The `SUMMARIES/` directory contains concise summaries of every major source file. These save tokens by letting you understand file contents without reading thousands of lines.

**When to read:** At session start or before working on a file you haven't read yet — check the summary first to understand structure and find the right methods.

**When to update:** After modifying any source file that has a summary, update the corresponding summary to reflect:
- New or removed public methods/functions
- New or removed structs/enums/types
- Changed line count (update the number)
- Changed file purpose or responsibilities

**Format:** Each summary file covers one source file and contains: purpose, line count, key types, and key public methods. Keep entries to one line each — no implementation details.

**Naming:** `SUMMARIES/gtk_mod.md`, `SUMMARIES/engine_keys.md`, `SUMMARIES/render.md`, etc. (path segments joined with `_`, no extension in name).

**README.md update rules:**
- Add new keys/commands to the appropriate Key Reference table
- Add new `:` commands to the Command Mode table
- Add new git commands to the git commands table
- Add new settings to the settings table
- Update architecture section if new files are added or line counts change significantly
- Do NOT add speculative/planned features — only document what is implemented


## Architecture

**VimCode**: Vim-like code editor in Rust with GTK4/Relm4. Clean separation: `src/core/` (platform-agnostic logic) vs `src/gtk/` (GTK UI) vs `src/tui_main/` (TUI) vs `src/win_gui/` (native Windows). `src/main.rs` is a thin CLI dispatcher.

**Tech Stack:** Rust 2021, GTK4+Relm4, Ropey (text rope), Tree-sitter (parsing), Pango+Cairo (rendering), ratatui+crossterm (TUI), windows-rs+Direct2D+DirectWrite (Win-GUI)

**Critical Rule:** `src/core/` must NEVER depend on `gtk4`, `relm4`, or `pangocairo`. Must be testable in isolation.

**Multi-backend rule:** There are THREE UI backends (GTK, TUI, Win-GUI). When fixing bugs or adding features that touch mouse handling, drag behavior, layout calculations, click detection, rendering, or panel interactions — check and update ALL THREE backends. The Win-GUI backend (`src/win_gui/`) is the newest and may lag behind on features; at minimum verify whether the change applies there. When building a new native GUI backend (e.g. macOS), read **[`quadraui/docs/NATIVE_GUI_LESSONS.md`](https://github.com/JDonaghy/quadraui/blob/main/quadraui/docs/NATIVE_GUI_LESSONS.md)** first — it documents pitfalls from Win-GUI (breadcrumb offset bugs, multi-group layout issues, click/draw parity, backend checklist).

**Cross-backend rendering algorithms:** any "fit X within Y" / "where does Z scroll to" / "which slice fits in N units" logic that's shared across backends MUST be parameterised over a measurement closure — never hardcode a unit (chars vs pixels). Put the algorithm in `quadraui` as `fn ...<F: Fn(...) -> usize>(..., measure: F)`. Each backend supplies its native measurer (TUI: `chars().count()`, GTK: Pango pixel widths, Win-GUI: DirectWrite, macOS: Core Text). Established examples: `quadraui::StatusBar::fit_right_start` and `quadraui::TabBar::fit_active_scroll_offset`. **When debugging a layout bug present in one backend but not another, suspect units before timing** — see [`quadraui/docs/NATIVE_GUI_LESSONS.md`](https://github.com/JDonaghy/quadraui/blob/main/quadraui/docs/NATIVE_GUI_LESSONS.md) §12, §13, §14.

### GTK directory (`src/gtk/`)

| File | What goes here |
|------|---------------|
| `mod.rs` | App struct, Msg enum, `SimpleComponent` impl (view/init/update), `impl App`, geometry helpers |
| `draw.rs` | All `draw_*` free functions (editor, panels, popups, sidebars) |
| `click.rs` | `ClickTarget` enum, `pixel_to_click_target()`, mouse click/drag/double-click handlers |
| `css.rs` | `make_theme_css()`, `STATIC_CSS`, `load_css()` |
| `util.rs` | `matches_gtk_key()`, settings form builders, GTK utilities, icon install |
| `tree.rs` | File tree building/expansion/indicators, name prompt/validation |

### TUI directory (`src/tui_main/`)

| File | What goes here |
|------|---------------|
| `mod.rs` | Structs, `run()`, `event_loop()`, clipboard, key translation, cell helpers |
| `render_impl.rs` | `draw_frame()`, `build_screen_for_tui()`, tab bar, editor/popup rendering |
| `panels.rs` | Sidebar panel rendering (activity bar, explorer, git, debug, extensions, AI, search, terminal) |
| `mouse.rs` | `handle_mouse()` — all click/drag/scroll interactions |

### Win-GUI directory (`src/win_gui/`)

Native Windows backend using `windows-rs` + Direct2D + DirectWrite. Behind `win-gui` Cargo feature. Consumes `ScreenLayout` from `render.rs` — same pattern as GTK/TUI. Some features are still missing (see BUGS.md for known Win-GUI gaps).

| File | What goes here |
|------|---------------|
| `mod.rs` | Win32 window creation, D2D/DWrite setup, event loop, keyboard/mouse handling, rendering |

### Engine directory (`src/core/engine/`)

The Engine is split into focused submodules. Each file adds `impl Engine` blocks — Rust resolves methods across files transparently.

| File | What goes here |
|------|---------------|
| `mod.rs` | Types, enums, `Engine` struct def, `new()`, free functions, `mod` declarations |
| `keys.rs` | `handle_key`, `handle_normal_key`, `handle_pending_key`, operator motions, macros, repeat, user keymaps |
| `insert.rs` | *(future)* `handle_insert_key`, `handle_replace_key` — currently in keys.rs |
| `command.rs` | *(future)* `handle_command_key`, `handle_search_key` — currently in keys.rs |
| `visual.rs` | `handle_visual_key`, visual helpers, multi-cursor |
| `execute.rs` | `execute_command()` — the ex-command dispatcher |
| `motions.rs` | Cursor movement, word/paragraph/scroll, bracket nav, join, indent, jump list |
| `buffers.rs` | File I/O, syntax update, undo/redo, git diff, markdown preview, netrw, workspace |
| `windows.rs` | Window/tab/group splits, focus, resize, session restore |
| `accessors.rs` | Group/buffer/window facades |
| `search.rs` | Project search/replace, search highlighting |
| `source_control.rs` | All `sc_*` methods, `handle_sc_*` key handlers |
| `lsp_ops.rs` | All `lsp_*` methods, code actions, diagnostics, hover, completion |
| `ext_panel.rs` | `ext_*` methods, `handle_ext_*`, extension + settings panel |
| `panels.rs` | AI (`ai_*`), dialog system, swap files |
| `plugins.rs` | Plugin init, event dispatch, command/keymap hooks |
| `dap_ops.rs` | DAP/debug: poll_dap, breakpoints, sidebar, stepping |
| `vscode.rs` | VSCode mode, menu bar methods |
| `picker.rs` | Fuzzy score, unified picker, quickfix |
| `terminal_ops.rs` | All `terminal_*` methods |
| `spell_ops.rs` | Spell checking methods |
| `tests.rs` | All test functions + helpers |

**File size rule:** No single file should exceed ~5,000 lines. If a submodule grows past that, split it further (e.g. `keys.rs` → `keys.rs` + `insert.rs` + `command.rs`). Place new `impl Engine` methods in the submodule matching their responsibility — never dump unrelated methods into `mod.rs`.

**Submodule conventions:**
- Each submodule starts with `use super::*;` to import all engine types
- Methods that other submodules call must be `pub(crate) fn`, not `fn`
- References to sibling core modules use `crate::core::module::` (not `super::module::`, which would look inside engine/)
- Free functions used across submodules stay in `mod.rs` and are accessed via `super::function_name`

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
cargo build                       # Compile
cargo test --no-default-features  # Run all tests (NEVER use --features win-gui — see Testing)
cargo clippy -- -D warnings       # Lint (must pass)
cargo fmt                         # Format
```

## Quality Checks (MANDATORY Before Commits)
**CRITICAL:** After making ANY code changes and before creating commits, ALWAYS run:
1. `cargo fmt` - Format code
2. `cargo clippy -- -D warnings` - Check linting (must have zero warnings)
3. `cargo test --no-default-features` - Verify all tests pass (lib + integration; NEVER use `--features win-gui`)
4. `cargo build` - Ensure compilation succeeds

If any check fails, fix immediately and re-run. Only commit when ALL checks pass.
This prevents CI failures and maintains code quality.

**Important:** `cargo test --no-default-features --lib` is faster for fast-iteration dev loops (lib tests only), but the pre-commit and pre-release gate **must** run the full `cargo test --no-default-features` — otherwise integration-test failures (e.g. the #114 regression that landed in v0.10.0) slip through.

## Branching & Releases
- All work happens on `develop`; `main` is the release branch
- Merge `develop` → `main` via GitHub PR (CI runs on the PR before release)
- Before creating the PR: bump version in `Cargo.toml` (minor for features, patch for fixes)
- If `Cargo.lock` changed since the last release: regenerate `flatpak/cargo-sources.json` with `python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json` (script from `flatpak/flatpak-builder-tools` repo)
- Merging the PR to `main` triggers `release.yml` which creates a GitHub Release tagged `v$VERSION`
- Never push directly to `main` — always merge from `develop` via PR

## Working with the quadraui dep
vimcode's `quadraui` dep is a **path dep** to `../quadraui/quadraui` — a sibling checkout of [JDonaghy/quadraui](https://github.com/JDonaghy/quadraui). Local builds always use whatever branch the sibling is currently on; no publish step.

- **Update to latest integrated quadraui:** `cd ~/src/quadraui && git checkout develop && git pull`. Then `cd ~/src/vimcode && cargo build` picks it up automatically.
- **Test a quadraui PR before merge:** `cd ~/src/quadraui && git fetch origin && git checkout <pr-branch>`. Switch back to `develop` when done.
- **Quadraui follows the same branching model as vimcode:** `develop` is the integration branch, `main` is release-only. Track `develop` for in-flight work; once quadraui ships releases we'll pin a `main` rev or a published version.
- **If a quadraui change breaks vimcode's build,** fix vimcode side on a normal vimcode branch (per the workflow above). The path dep doesn't enforce a version contract — it's caller-beware.
- **Vimcode CI is currently disabled** (`.github/workflows/*.yml.disabled`) because it can't resolve a sibling-clone path dep in a fresh `actions/checkout`. Re-enable strategy is open: workflow step that clones quadraui first, or switch to a git-dep with pinned rev.

## Migration prerequisites (vimcode → quadraui adoption)
Before migrating a vimcode panel/widget to a quadraui primitive, **ALL** of these must be true:

1. **TUI rasteriser exists in quadraui** — `quadraui::tui::draw_<primitive>` plus the layout helper `tui_<primitive>_layout`.
2. **GTK rasteriser exists in quadraui** — `quadraui::gtk::draw_<primitive>` plus the layout helper `gtk_<primitive>_layout`.
3. **Both backends have paint↔click round-trip harnesses passing** — the gate from quadraui's CLAUDE.md "Lessons captured" section. Without these, the migration ships the bug class the harnesses exist to catch (cf. the vimcode #296 smoke wave that triggered the Session 346 course correction).
4. **Consumer pattern validated** — the way vimcode WILL use the primitive (per-section drag/scroll for MSV; input-aux + collapsible sections for SC; etc.) has been exercised in a quadraui example or kubeui demo. The consumer-state round-trip test catches integration bugs that primitive-level harness alone can't see.

The principle: **a vimcode primitive migration must collapse paint code on every vimcode-supported backend, not just one.** Partial adoption is the opposite of consolidation — it leaves bespoke paint code alive in the un-migrated backends, so vimcode now has TWO sources of truth (the primitive AND the bespoke). Don't ship a migration as "TUI-only for now."

**If a primitive is missing from a backend,** file a quadraui issue to add it before starting the vimcode-side migration.

**Once Win-GUI gets rebuilt on quadraui,** the rule grows to include `quadraui::win_gui::*` rasterisers and harnesses. Same for macOS later.

## Paint↔click integration pattern (CRITICAL)

Quadraui's demos use `AppLogic` + `Backend` trait so paint and click share one function context — one `backend.msv_layout(area, &view)` call, same inputs, same result. **Vimcode can't do that**: paint runs inside ratatui's `terminal.draw()` / GTK's `set_draw_func` closure (which owns the `Frame`/`Context`), click runs in the event loop without one.

**The rule: click NEVER re-derives layout. Paint caches it; click reads the cache.**

```rust
// Engine field:
pub foo_layout: RefCell<Option<quadraui::FooLayout>>,

// Paint (inside draw closure — has the real area rect):
let view = render::foo_to_primitive(&data);
backend.draw_foo(area, &view);  // internally computes layout
// Cache the layout for click:
let layout = backend.foo_layout(area, &view);
engine.foo_layout.replace(Some(layout));

// Click (in event handler — does NOT recompute):
if let Some(ref layout) = *engine.foo_layout.borrow() {
    match layout.hit_test(rel_x, rel_y) { ... }
}
```

**Why not call `tui_foo_layout` / `gtk_foo_layout` in click?** Because reconstructing the area rect requires reproducing the chrome math that ratatui's `Layout::split` or GTK's `DrawingArea` sizing computed. That formula drifts ±1 cell/pixel when wildmenu / per-window status / terminal-panel states change between the formula's assumptions and the layout engine's actual output. `EqualShare` amplifies any input mismatch into section-boundary shifts. This caused 4 rounds of smoke failures on #296 (Session 347) before the lesson was captured.

**Corollary**: the same applies to inner body layouts (tree, list, form). If click needs to drill into a section's body (e.g., `TreeViewLayout::hit_test` for which row was clicked), cache that inner layout at paint time too — don't call `tui_tree_layout` / `gtk_tree_layout` in click.

**This rule supersedes any suggestion to "compute layout fresh in click for one source of truth."** In vimcode's architecture, the one source of truth IS the layout paint produced. Click consuming a stale-by-one-frame layout is strictly better than click consuming a fresh layout computed from drifted inputs.

## Code Style
- `rustfmt` defaults (4-space indent)
- `PascalCase` types, `snake_case` functions/vars
- Core: Return `Result<T, E>` for I/O, silent no-ops for bounds
- Tests in `#[cfg(test)] mod tests` at file bottom

## Theme Colors (CRITICAL)
**NEVER introduce new hex color literals for derived theme fields.** Every new color added to the `Theme` struct must be derived from an existing foundational theme field (`background`, `foreground`, etc.) using `lighten()`/`darken()`/`cursorline_tint()`/`colorcolumn_tint()` or similar. Use a local variable to avoid repeating hex strings:
```rust
pub fn onedark() -> Self {
    let bg = Color::from_hex("#1a1a1a");
    Self {
        background: bg,
        new_derived_color: bg.some_tint(),  // GOOD: derived from variable
        // bad_color: Color::from_hex("#2c313a"),  // BAD: hardcoded hex
    }
}
```
**Why:** Hardcoded hex values don't adapt to custom themes or VSCode theme imports. Only foundational colors (background, foreground, keyword, string, etc.) should have hex literals. VSCode theme imports (`from_vscode_json`) can override derived values with user-specified exact colors.

## Testing (CRITICAL)
**NEVER run `cargo test` with the `win-gui` feature enabled.** This spawns hundreds of real Win32 windows and locks up the machine. Use these commands instead:
- **Full test suite (pre-commit, pre-release gate):** `cargo test --no-default-features` — lib + integration tests; no GTK, no win-gui. Required before any commit that might land in a release.
- **Fast dev iteration:** `cargo test --no-default-features --lib` — lib tests only, much faster. Use for inner-loop edit/test cycles, NOT as a release gate.
- **Build win-gui:** `cargo build --bin vimcode-win --features win-gui --no-default-features`
- **Clippy win-gui:** `cargo clippy --features win-gui --no-default-features`
- NEVER combine `cargo test` with `--features win-gui`

## Common Patterns

**Hit regions for clickable UI elements:** When adding clickable elements to the find/replace overlay (or future UI panels), define hit regions in `engine/mod.rs` using `FrHitRegion` + `FindReplaceClickTarget` types. Compute regions once in `build_screen_layout()`, then backends walk the region list to resolve clicks. Dispatch through a shared `Engine::handle_*_click()` method. This avoids per-backend geometry duplication and is the established pattern for crate extraction. See `compute_find_replace_hit_regions()` as the reference implementation.

**Add Normal Mode Key:** `engine/keys.rs` → `handle_normal_key()` → add match arm → test

**Add Command:** `engine/execute.rs` → `execute_command()` → add match arm → test

**Add Operator+Motion:** `engine/keys.rs` → set `pending_operator` → implement in `handle_operator_motion()` → test

**Ctrl-W Command:** `engine/keys.rs` → `handle_pending_key()` under `'\x17'` case

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
