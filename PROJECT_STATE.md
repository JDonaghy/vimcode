# VimCode Project State

**Last updated:** Mar 10, 2026 (Session 166 — Extension Registry Decoupling) | **Tests:** 4053

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 161 are in **SESSION_HISTORY.md**.

---

## Testing Policy

**Every new Vim feature and every bug fix MUST have comprehensive integration tests before the work is considered done.** Subtle bugs (register content, cursor position, newline handling, linewise vs. char-mode paste) are only reliably caught by tests. The process is:

1. Write failing tests that document the expected Vim behavior
2. Implement/fix the feature until all tests pass
3. Run the full suite (`cargo test`) — no regressions allowed

When implementing a new key/command, add tests covering:
- Basic happy path
- Edge cases: start/middle/end of line, start/end of file, empty buffer, count prefix
- Register content (text and `is_linewise` flag)
- Cursor position after the operation
- Interaction with paste (`p`/`P`) to verify the yanked/deleted content behaves correctly

---

## Recent Work

**Session 166 — Extension Registry Decoupling (4053 tests):**
Fully decoupled extensions from compiled-in data. Removed `BUNDLED` static array and all `include_str!()` from `extensions.rs`. Extensions now fetched from remote GitHub registry ([vimcode-ext](https://github.com/JDonaghy/vimcode-ext)) and cached locally at `~/.config/vimcode/registry_cache.json`. `ext_available_manifests()` merges registry + local extensions from `~/.config/vimcode/extensions/*/manifest.toml`. `LspManager` stores manifests via `set_ext_manifests()`, `DapManager` functions accept manifests as parameters. New extensions can be added without updating VimCode code. Local extension development supported: create `manifest.toml` + scripts in `extensions/<name>/`, they appear in the sidebar automatically. Removed `extensions/` directory from repo. Generated `registry.json` with all 17 extension manifests. Updated EXTENSIONS.md with local dev workflow and registry submission guide.

**Session 165 — Extension Panel API + Git Log Panel (4053 tests):**
Extension panel infrastructure for custom sidebar panels from Lua plugins. New types: `PanelRegistration`, `ExtPanelItem`, `ExtPanelStyle` in plugin.rs. Lua API: `vimcode.panel.register/set_items/parse_event`, `vimcode.git.branches()`. Engine state: `ext_panels`, `ext_panel_items`, `ext_panel_active`, `ext_panel_has_focus`, `ext_panel_selected`, `ext_panel_scroll_top`, `ext_panel_sections_expanded`. `handle_ext_panel_key()` with j/k nav, Tab expand/collapse, Enter `panel_select` event, q/Esc unfocus, other keys `panel_action` event. Render: `ExtPanelData`/`ExtPanelSectionData` in render.rs, `build_ext_panel_data()`. TUI: `render_ext_panel()`, dynamic activity bar icons, keyboard routing, click handling. GTK: `SidebarPanel::ExtPanel(String)` variant. Git Log Panel: `git::list_branches()`/`BranchEntry` in git.rs, new `git_log_panel.lua` script with Branches/Log/Stash sections, manifest updated (8 scripts total). 17 integration tests in `tests/ext_panel.rs`.

**Session 163 — Git Insights enhancement (4036 tests):**
Full git-insights extension overhaul. **Part 1**: Extended Lua plugin API with 12 new `vimcode.git.*` bindings (show, blame_file, line_log, diff_ref, file_log_detailed, repo_root, branch, stash_list/push/pop/show, log). Added 9 new git.rs functions + 2 structs (`DetailedLogEntry`, `StashEntry`). **Part 2**: Scratch buffer API — `ScratchBufferRequest` struct, `vimcode.buf.open_scratch()` Lua binding, engine handler in `apply_plugin_ctx()` (creates buffer, sets content/name/readonly/syntax, opens in splits). `BufferState.scratch_name` for `[Name]` tab display. 6 new Lua scripts for git-insights (`:GitFileHistory`, `:GitShow`, `:GitLineHistory`, `:GitDiff`, `:GitStash*`, `:GitRepoLog`). BUNDLED array updated from 1→7 scripts. 36 new tests total (6 unit, 12+6+12 integration). Also fixed Flatpak build (cargo-sources.json regen).

**Session 162 — Bulk paste performance fix (4003 tests):**
Fixed critical performance bug: pasting large text in insert mode caused UI freeze / 100% CPU. Root cause was `Event::Paste` (TUI) and `ClipboardPasteToInput` (GTK) feeding each character individually through `handle_key()`, triggering ~N tree-sitter reparses, bracket match scans, auto-completion scans, etc. for an N-character paste. New `Engine::paste_in_insert_mode(text)` method does a single bulk `insert_with_undo()` and runs all expensive post-processing once. Also added safety guard in `compute_word_wrap_segments()` (`pos = break_at.max(pos + 1)`) to prevent potential infinite loops. 8 new tests in `tests/paste_insert.rs`.

> Sessions 161 and earlier archived in **SESSION_HISTORY.md**.
