# VimCode Project State

**Last updated:** Mar 22, 2026 (Session 206 — Git Log Panel Bug Fixes + Release v0.4.0) | **Tests:** 4664

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 205 are in **SESSION_HISTORY.md**.

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

### Session 206 — Git Log Panel Bug Fixes + Release v0.4.0 (Mar 22, 2026)
- **GTK hover popup link clicking**: Links in editor hover popups now activate on click (previously only focused); Pango `index_to_pos` computes link pixel rects; `editor_hover_link_rects` field on App struct
- **Panel reveal fixes**: GTK no longer toggles sidebar on reveal (sets `active_panel` directly); TUI clears expanded tree state before reveal to prevent wrong-commit selection; reveal scrolls to center item
- **Ext panel scroll/scrollbar**: Added `EventControllerScroll` + scrollbar click/drag for GTK ext panels; moved TUI ext panel scroll check to top of sidebar chain; TUI scrollbar click + drag support
- **Git hash consistency**: `git log` now uses `--format=%H %s` for full hashes (was `--oneline`); added `git_log_commit()` for single-commit lookup; `refresh_all()` appends missing commits for reveal of older hashes
- **Lua reveal target timing**: `_git_log_reveal_target` cleared only by `refresh_all()` after use, not before `panel.reveal()` (async processing in `apply_plugin_ctx`)
- Version bumped to 0.4.0; 4664 tests

### Session 205 — Enhanced Git Log Panel + Blame-to-Panel Navigation (Mar 22, 2026)
- **Expandable commits in Git Log panel**: Commits are tree nodes that expand to show changed files as children; `Tab` toggles expand/collapse; hover content on commits shows author/date/message/stat
- **Side-by-side diff from log**: Clicking a file in an expanded commit (`o` key) opens a side-by-side diff reusing existing diff infrastructure; `open_commit_file_diff()` engine method
- **Blame-to-panel navigation**: `GitShow`/`:Gshow` command now navigates to the Git Log panel instead of opening a scratch buffer; `panel.reveal()` API for programmatic panel navigation
- **Git log action keys**: `o`=open diff, `y`=copy hash/path, `b`=open in browser, `r`=refresh, `d`=pop stash, `p`=push stash, `/`=search/filter
- **New Rust APIs**: `commit_files()`, `diff_file_at_commit()`, `show_commit_file()` in git.rs
- **New Lua bindings**: `vimcode.git.commit_files(hash)`, `vimcode.git.diff_file(hash, path)`, `vimcode.git.show_file(hash, path)`, `vimcode.git.commit_detail(hash)`, `vimcode.git.open_diff(hash, path)`, `vimcode.panel.reveal(panel, section, item_id)`

### Session 204 — Command URI Dispatch for Extensions (Mar 22, 2026)
- **Command URI dispatch**: `command:Name?args` links in hover popup markdown now dispatch to plugin-registered commands via `execute_command_uri()`; `percent_decode()` helper for URL-encoded arguments; `execute_hover_goto()` fallback routes unknown commands to plugins; GTK `PanelHoverClick` and TUI panel hover click handlers check for `command:` before URL open/copy
- **git-insights blame.lua**: "Open Commit" (`command:GitShow?hash`) and "Copy Hash" (`command:GitCopyHash?hash`) action links in blame hover popup; `GitShow` command runs `:Gshow`; `GitCopyHash` copies hash to `+` register
- 5 new tests (percent_decode, execute_command_uri edge cases); 4654 total tests

### Session 203 — VSCode Mode Git Insights + Hover Popup Fixes (Mar 21, 2026)
- **VSCode edit mode git insights**: `fire_cursor_move_hook()` added to all exit paths in `handle_vscode_key()` so Lua plugins (blame.lua) receive `cursor_move` events; annotation rendering gate in `render.rs` updated to allow VSCode mode (Insert + VSCode); hover dwell gates in both GTK and TUI backends updated to include VSCode mode
- **GTK hover popup word wrapping**: Pango word wrapping (`WrapMode::WordChar`) replaces fixed-width overflow; pixel-based height cap; Cairo clip for bounds
- **Stale LSP hover fix**: `lsp_hover_text` cleared on dismiss and mouse-off, preventing cached hover from following clicks
- **GTK hover popup click-to-focus**: `editor_hover_popup_rect` caches popup bounds from draw; clicks on popup set focus (blue border, keyboard control); clicks outside dismiss
- **20Hz SearchPollTick dismiss race fix**: Skip `editor_hover_mouse_move()` when mouse is within popup bounds, preventing continuous dismiss cycle that made popups unclickable
- 4649 total tests (13 new from test count delta)

> Sessions 202 and earlier archived in **SESSION_HISTORY.md**.
