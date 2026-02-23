# VimCode Implementation Plan

## 🔴 Fix Next (Highest Priority — Session 70)

### Terminal: Scrollbar always full height
The scrollbar thumb always fills the full height instead of shrinking as content accumulates.
Root cause: `panel.scrollback_rows` comes from `screen.scrollback()` which returns 0 until lines
actually scroll off the top of the 12-row vt100 screen. When output fits on-screen, no scrollback
is recorded and the fallback `(0, content_rows)` always draws a full bar.
Fix: track the total lines ever written (not just scrollback buffer count) — either maintain a
`lines_written: usize` counter on `TerminalPane` incremented in `poll()` based on cursor row
advancement, or derive a `total_rows = scroll_offset + content_rows` approximation from
`scroll_offset` when scrollback count is zero. The thumb should shrink to
`content_rows / total_rows` once output exceeds the panel height.

### Terminal: `:term` reopens zombie after Ctrl-D exit
When the shell exits (Ctrl-D), `poll_terminal()` correctly sets `exited = true` and calls
`close_terminal()` (sets `terminal_open = false`). However the `TerminalPane` with
`exited = true` is left in `self.terminal`. When the user then types `:term`, `open_terminal()`
sees `terminal_open == false` and `self.terminal.is_some()`, so it reopens the dead pane instead
of spawning a fresh shell.
Fix: in `open_terminal()`, check `self.terminal.as_ref().map(|t| t.exited).unwrap_or(false)`;
if true, drop the old pane (`self.terminal = None`) before creating a new one. Alternatively,
drop the pane inside `close_terminal()` when `exited` is true (since there's nothing to restore).

---

## Recently Completed (Session 69)

### ✅ Terminal Panel Bug Fixes + Scrollbar

- **TUI crash fix** — `build_screen_for_tui` now subtracts `qf_height + term_height` from `content_rows`; same fix in the viewport-sync loop at event-loop top
- **TUI full-width fix** — PTY opens at `terminal.size().ok().map(|s| s.width)`, not editor-column width; `Event::Resize` now passes full `new_w` to `terminal_resize()`
- **Scrollbar** — `TerminalPanel.scrollback_rows: usize` (from `screen.scrollback()`); TUI: rightmost column uses `░`/`█`; GTK: 6px Cairo strip with alpha thumb; thumb tracks `scroll_offset / scrollback_rows`
- **Auto-close on exit** — `poll_terminal()` calls `close_terminal()` when `term.exited` is true; no zombie pane after Ctrl-D / `exit`
- **Click-to-refocus editor** — `Msg::MouseClick` else-branch sets `terminal_has_focus = false` (GTK); `handle_mouse()` sets it false when click lands outside terminal block (TUI)
- **TUI mouse selection** — Down click starts `TermSelection`; Drag arm updates `end_row/end_col`; Scroll arms detect terminal area before editor scroll
- Tests: 638 → 638 (no change)

---

## Recently Completed (Session 68)

### ✅ Integrated Terminal Panel

- **`src/core/terminal.rs`** (new, ~165 lines) — `TerminalPane` backed by `portable-pty` (native PTY creation) + `vt100` (VT100 parser / cell grid); background mpsc reader thread drains PTY output asynchronously; `poll()` feeds parser + checks child exit; `write_input()` sends bytes to shell; `resize()` updates parser dimensions; `selected_text()` extracts selection from vt100 screen; `default_shell()` reads `$SHELL`
- **Engine** — 3 new fields (`terminal`, `terminal_open`, `terminal_has_focus`); 7 new methods (`open_terminal`, `close_terminal`, `toggle_terminal`, `poll_terminal`, `terminal_write`, `terminal_resize`, `terminal_copy_selection`); `EngineAction::OpenTerminal` new variant; `:term`/`:terminal` command dispatch
- **Settings** — `PanelKeys.open_terminal: String` (default `<C-t>`); `pk_open_terminal()` default fn; `Default` impl updated
- **Render** — `TerminalCell`, `TermSelection`, `TerminalPanel` types; `ScreenLayout.terminal: Option<TerminalPanel>`; `build_terminal_panel(engine)` maps vt100 screen cells; `map_vt100_color(color, is_bg)` handles Default/Rgb/Idx(256) variants; `xterm_256_color(n)` 256-color palette; `normalize_term_selection()` helper
- **GTK** — `draw_terminal_panel()` renders toolbar (Nerd Font `󰅖` close / `󰤼` split icons) + cell grid (per-cell bg fill + pango char + cursor rect + selection overlay); `gtk_key_to_pty_bytes()` translates GDK key names to PTY bytes; 6 new Msg variants; key routing checks `open_terminal` panel key first, then PTY routing when `terminal_has_focus`; `term_px` reduces editor content bounds; SearchPollTick polls terminal
- **TUI** — `render_terminal_panel()` toolbar + content rows via ratatui buffer; `translate_key_to_pty()` maps crossterm keycodes; extra `Constraint::Length(terminal_height)` layout slot; key routing; `EngineAction::OpenTerminal` handling; idle polling; resize event calls `terminal_resize()`
- **Future items** — Multiple tabs (tab strip in toolbar, `Vec<TerminalPane>`); draggable panel height; scrollback navigation (ring buffer + scroll_offset); TUI Ctrl+F find dialog; split terminal panes
- Tests: 638 → 638 (PTY requires subprocess; no unit tests in v1)

---

## Recently Completed (Session 67)

### ✅ VSCode Mode: F1 Command Access + Status Bar Hint

- **F1 opens command bar** — `"F1"` arm in `handle_vscode_key()` sets `mode = Command`, clears `command_buffer` and `message`
- **Routing** — top of `handle_vscode_key()` delegates to `handle_command_key()` when `mode == Command` (before undo group start, no undo side-effect)
- **Escape returns to Insert** — `handle_command_key()` Escape arm: `mode = if is_vscode_mode() { Insert } else { Normal }`
- **Return returns to Insert** — after `execute_command()`, if `is_vscode_mode()` → `mode = Insert`; if `:set mode=vim` ran, `is_vscode_mode()` is false so mode stays Normal (correct)
- **Status bar hint** — `mode_str()`: when `is_vscode_mode()`, Insert/Normal → `"EDIT  F1:cmd  Alt-M:vim"`, Command → `"COMMAND"`, Visual → `"SELECT"`
- **Test hermetic fix** — `Settings::load()` returns `Self::default()` under `#[cfg(test)]`; prevents user's `settings.json` from breaking tests
- **3 new tests**: `test_vscode_mode_f1_opens_command`, `test_vscode_mode_command_returns_to_insert`, `test_vscode_mode_f1_escape_returns_to_insert`
- Tests: 635 → 638 (+3)

---

## Recently Completed (Session 66)

### ✅ Edit Mode Toggle (Vim ↔ VSCode)

- **`EditorMode` enum** (`Vim`/`Vscode`) in `settings.rs` with serde `#[serde(rename_all = "lowercase")]`; `editor_mode` field on `Settings`; backward-compat (existing settings.json without field defaults to `Vim`)
- **`:set mode=vim` / `:set mode=vscode`** — `set_value_option()` arm; `query_option()` arm; `display_all()` includes `mode=vim/vscode`
- **`handle_vscode_key(key_name, unicode, ctrl)`** — replaces normal mode dispatch when `is_vscode_mode()`; three branches: ctrl combos, Shift+Arrow selection, regular keys
- **Ctrl combos**: Ctrl-Z undo, Ctrl-Y redo, Ctrl-A select-all, Ctrl-C copy, Ctrl-X cut (line if no selection), Ctrl-V paste, Ctrl+Arrow word jump, Ctrl+Shift+Arrow word select, Ctrl-Delete/Backspace word delete, Ctrl-/ line comment toggle
- **Shift+Arrow**: `vscode_extend_selection()` starts/extends visual selection; exclusive-end semantics (cursor = exclusive end)
- **Regular keys**: Escape clears selection, arrows clear selection + move, BackSpace/Delete/Tab/Return/printable replace selection if active
- **Undo model**: `start_undo_group()` at start of `handle_vscode_key`; `finish_undo_group()` if `changed`; helpers don't manage their own undo groups
- **`vscode_delete_selection()`**: exclusive end (delete `[anchor, cursor)`, not including cursor char); no inner undo group
- **`mode_str()`**: returns "EDIT"/"SELECT"/"NORMAL"/"INSERT"/"COMMAND"/"SEARCH"/"VISUAL"/"VLINE"/"VBLOCK"
- **`toggle_editor_mode()`** — `Alt-M` in both GTK and TUI; saves to settings.json; clears selection; sets mode Insert or Normal
- **GTK**: Shift+Arrow key name transform in vscode mode; Ctrl-V clipboard pre-load; mouse click clears selection
- **TUI**: `translate_key()` Shift+Arrow (ctrl=false), Ctrl+Shift+Arrow (ctrl=true); Alt-M in alt-key block; Ctrl-V clipboard pre-load; mouse click clears
- **render.rs**: `engine.mode_str()` replaces inline mode-string match in status bar
- **15 new tests**: setting, typing (`:` inserts literal colon), undo, redo, shift-arrow selection, ctrl+shift-arrow word select, type-replaces-selection, backspace-clears-selection, ctrl-a, escape, ctrl-x cuts line, ctrl-c copies line, toggle, smart-home, comment-toggle
- Tests: 620 → 635 (+15)

---

## Recently Completed (Session 65)

### ✅ Arrow Key Navigation for Completion Popup + Ctrl-Space Re-trigger Fix

- **`Down`/`Up` in Insert mode navigate popup** — when the completion popup is visible (`completion_display_only && completion_idx.is_some()`), `Down` and `Up` cycle through candidates (same as `Ctrl-N`/`Ctrl-P`) without moving the cursor; intercepted before the clear block so the popup is not dismissed
- **Ctrl-Space re-trigger bug fixed in TUI** — `translate_key()` was emitting `key_name=" "` (literal space) for Ctrl-Space; engine checks `key_name == "space"`; they never matched, so Ctrl-Space had no effect in the TUI backend; fixed to emit `"space"` for ctrl+space (matching GTK/GDK convention)
- **`parse_key_binding` named key support** — `"Space"` (5 chars) failed the single-char guard; added named-key table so `<C-Space>` now parses to `Some((true, false, false, ' '))`; trigger setting round-trips correctly
- File changes: `src/core/engine.rs` (intercept block, updated `test_auto_popup_dismissed_on_navigation`, new `test_auto_popup_arrow_cycles`), `src/core/settings.rs` (named-key table in `parse_key_binding`, new `test_parse_key_binding_named_space`), `src/tui_main.rs` (`translate_key` space fix)
- Tests: 618 → 620 (+2)

---

## Recently Completed (Session 64)

### ✅ VSCode-Style Auto-Popup Completion (replaces ghost text)

- **Removed ghost text** — `ghost_text`, `lsp_pending_ghost_completion`, `ghost_prefix` fields, `update_ghost_text()`, `lsp_request_ghost()`, `ghost_suffix` on `RenderedLine`, `ghost_text_fg` on `Theme`, GTK + TUI ghost rendering blocks; 6 ghost tests removed
- **`completion_display_only: bool`** — new field; `true` when popup triggered by typing or Ctrl-Space (Tab accepts, Ctrl-N/P cycle without inserting); `false` when triggered by explicit Ctrl-N/P (old behavior: inserts immediately)
- **`trigger_auto_completion()`** — new method; called after char insert and BackSpace; uses `word_completions_for_prefix()` sync + `lsp_request_completion()` async; sets `completion_display_only = true`
- **`handle_insert_key()` changes** — configured trigger check (parses `settings.completion_keys.trigger`); Ctrl-N/P: if display-only, just cycles index (no text change); Tab: if display-only, calls `apply_completion_candidate()`; clear block now also resets `completion_display_only`
- **`poll_lsp()` CompletionResponse** — ghost branch removed; popup branch now filters by prefix and sets `completion_display_only = true` (no immediate insert)
- **`CompletionKeys` struct** in `settings.rs` — `trigger` (default `<C-Space>`) + `accept` (default `Tab`); follows `PanelKeys` pattern with serde per-field defaults; added to `Settings` struct
- File changes: `src/core/settings.rs` (+CompletionKeys), `src/core/engine.rs` (−ghost, +display_only, +trigger_auto_completion, rewritten insert_key/poll_lsp, 5 new tests), `src/render.rs` (−ghost_suffix, −ghost_text_fg), `src/main.rs` (−ghost rendering), `src/tui_main.rs` (−ghost rendering)
- Tests: 619 → 618 (−6 ghost tests, +5 auto-popup tests)

---

## Recently Completed (Session 62)

### ✅ Configurable Panel Navigation Keys (`panel_keys`)
- **`PanelKeys` struct** in `settings.rs` — 5 fields (`toggle_sidebar`, `focus_explorer`, `focus_search`, `fuzzy_finder`, `live_grep`) with serde per-field defaults; `parse_key_binding(s) -> Option<(ctrl, shift, alt, char)>` free function parses `<C-b>`, `<A-e>`, `<C-S-x>` notation
- **Defaults** — `toggle_sidebar: <C-b>`, `focus_explorer: <A-e>`, `focus_search: <A-f>`, `fuzzy_finder: <C-p>`, `live_grep: <C-g>`
- **Removed `ExplorerAction::ToggleMode`** — keyboard focus on explorer makes a separate "explorer mode" gate unnecessary; `toggle_mode` field + default fn + test removed from `ExplorerKeys`
- **TUI** — `matches_tui_key(binding, code, mods)` helper; panel_keys dispatch block added in both the editor-focused section (to activate panels) AND the sidebar-focused section (to toggle back to editor or switch panels); all five shortcuts work bidirectionally regardless of where focus is
- **GTK** — `matches_gtk_key(binding, key, state)` helper; `Msg::ToggleFocusExplorer` (toggle between editor and tree view) + new `Msg::ToggleFocusSearch` (show search panel / return to editor without hiding sidebar); tree view `EventControllerKey` now captures `engine` and dispatches panel_keys before the `Stop` catchall — so `Alt+E`, `Alt+F`, `Ctrl+B` all work when the tree has focus
- **Return to editor** — `Escape` works from both explorer and search panels; pressing the same panel shortcut again also returns focus to the editor (toggle); search panel stays visible (no sidebar-hide animation artifact)
- File changes: `src/core/settings.rs` (+55 lines, PanelKeys struct, parse_key_binding, 8 new tests), `src/tui_main.rs` (matches_tui_key helper, panel_keys dispatch ×2, removed explorer_mode), `src/main.rs` (matches_gtk_key helper, ToggleFocusExplorer + ToggleFocusSearch msgs, tree-view key handler update)
- Tests: 606 → 613 (7 net new: +8 PanelKeys, −1 toggle_mode)

---

## Recently Completed (Session 61)

### ✅ Replace arboard with copypasta-ext; fix TUI paste intercept
- **Dependency swap** — removed `arboard = "3"`, added `copypasta-ext = "0.4"`
- **GTK backend** — eliminated background thread + `ClipboardCmd` enum + `clip_tx`; replaced with synchronous `copypasta_ext::x11_bin::ClipboardContext` (xclip/xsel subprocesses, no X11 event-loop conflict); `p`/`P` now read clipboard inline before falling through to `handle_key()`; removed `Msg::ClipboardPasteReady`
- **TUI backend** — replaced ~180 lines of platform-detection helpers (`is_wsl`, `cmd_exists`, `try_setup_*`, etc.) with `build_clipboard_ctx()` (~20 lines) using `copypasta_ext::x11_bin::ClipboardContext` on X11 and `try_context()` elsewhere; `Arc<Mutex<Box<dyn ClipboardProviderExt>>>` wraps the context for the read/write callbacks
- **TUI paste-intercept bug** — `translate_key()` sets `key_name = ""` for regular chars (only ctrl/special keys get a name); paste intercept condition was `key_name == "p"` (always false) so `intercept_paste_key` was never called; fixed to `matches!(unicode, Some('p') | Some('P'))`; also fixed `intercept_paste_key` to pass `key_name = ""` (TUI convention) and to set error message after `handle_key()` (which clears `engine.message`)
- **Why x11_bin explicitly** — `try_context()` picks `x11_fork` first on X11; `x11_fork::get_contents()` delegates to `X11ClipboardContext::get_contents()` (direct X11 socket) which conflicts with GTK's event loop and fails when another app owns the clipboard; `x11_bin` uses xclip/xsel subprocesses (independent X11 connections per call)
- Tests: 606 (no change)

---

## Recently Completed (Session 59)

### ✅ Explorer Polish
- **Prompt delay fix** — early `continue` statements in TUI event loop now set `needs_redraw = true` before continuing, so explorer mode prompts (M, a, A, etc.) appear instantly instead of waiting for the next event
- **Cursor key editing in prompts** — `SidebarPrompt` gained `cursor: usize` field; Left/Right/Home/End/Delete keys work in all sidebar prompts (move, new file, new folder, rename); Backspace and char insert are cursor-position-aware
- **Move path editing** — `engine.move_file()` now accepts either a directory (appends filename) or a full destination path (rename+move); prompt pre-fills with full relative path including filename; `../` paths resolve correctly
- **Auto-refresh** — TUI sidebar rebuilds every 2s when visible and idle (`last_sidebar_refresh` timer); external filesystem changes reflected automatically
- **Root folder entry** — project root shown at top of explorer tree (uppercase name, always expanded) in both GTK (`build_file_tree_with_root()` wrapper) and TUI (`build_rows()` inserts root at depth 0); select root + press `a` to create files at the top level
- **Removed refresh** — `ExplorerAction::Refresh` variant, `refresh` field from `ExplorerKeys`, refresh toolbar icon (GTK + TUI), and `R` key binding all removed; auto-refresh makes manual refresh unnecessary
- **New file/folder prompts** — pre-fill with target directory path relative to root so user can see and edit the destination
- File changes: `tui_main.rs` (+320 lines), `main.rs` (+150 lines), `engine.rs` (move_file API, help text), `settings.rs` (removed refresh)
- Tests: no change (593 total)

---

## Recently Completed (Session 58)

### ✅ Configurable Explorer Keys + Help Hint
- **`ExplorerKeys` struct** in `settings.rs` — 6 configurable fields (`new_file`, `new_folder`, `delete`, `rename`, `move_file`, `toggle_mode`) with serde per-field defaults; `ExplorerAction` enum + `resolve(char)` dispatcher
- **TUI sidebar refactor** — replaced hardcoded `KeyCode::Char` arms with `engine.settings.explorer_keys.resolve(c)` match
- **Explorer mode message** — now reads `Explorer mode ON — a/A/r/M/D  (? to exit, :help explorer for details)`
- **`:help explorer`** — added configurable keys note with JSON example
- Tests: 588 → 593 (5 new: explorer_keys_default, resolve, custom_override, serde_partial, in_settings_serde)

---

## Recently Completed (Session 57)

### ✅ Help System + Move-File Fix
- **`:help [topic]`** / **`:h [topic]`** — opens help text in a read-only vsplit; topics: `explorer` (sidebar keys + explorer mode), `keys` (normal mode reference), `commands` (command mode reference); unknown topic shows error message; no-arg shows topic index
- **Move file selection:** sidebar `M` (move file) now calls `reveal_path(&dest)` instead of `build_rows()`, so the moved file is selected at its new location
- Tests: 584 → 588 (4 new: help_command_explorer, help_command_no_args, help_alias_h, help_unknown_topic)

---

## Recently Completed (Session 56)

### ✅ VSCode-Like Explorer + File Diff
- **Engine:** `rename_file()` / `move_file()` with open-buffer path updates; `DiffLine` enum; `diff_window_pair` + `diff_results` fields; `cmd_diffthis/off/split`; `lcs_diff()` O(N×M) LCS with 3000-line cap; `:diffthis`/`:diffoff`/`:diffsplit` commands
- **render.rs:** `diff_status: Option<DiffLine>` on `RenderedLine`; `diff_added_bg`/`diff_removed_bg` in Theme; populated in `build_rendered_window()`
- **GTK:** F2 inline rename; right-click `GestureClick` → `Popover` context menu; DragSource + DropTarget for file move; diff bg rendering; `SelectForDiff`/`DiffWithSelected` flow; create-in-selected-folder
- **TUI:** `PromptKind::Rename(PathBuf)` + `MoveFile(PathBuf)`; `r`/`M` keys; create-in-selected-folder (`NewFile(PathBuf)` / `NewFolder(PathBuf)`); diff bg via `line_bg` per-row
- **Tests:** 571 → 584 (13 new: rename_file ×3, move_file ×2, lcs_diff ×5, diffthis/off/split ×3)

---

## Recently Completed (Session 55)

### ✅ Quickfix Window
- **`:grep <pattern>`** / **`:vimgrep <pattern>`** — search project via `search_in_project()` (existing engine), populate `engine.quickfix_items: Vec<ProjectMatch>`; open panel automatically (`quickfix_open = true`, `quickfix_has_focus = false`); message shows match count
- **`:copen`/`:cope`** — open panel with keyboard focus (errors if list empty); **`:cclose`/`:ccl`** — close panel, clear focus
- **`:cn`/`:cnext`** / **`:cp`/`:cprev`/`:cN`** — next/prev item; clamps at boundaries; each calls `quickfix_jump()` which opens file + positions cursor
- **`:cc N`** — jump to 1-based index N; uses `strip_prefix("cc ")` + `parse::<usize>()` + `filter(|&n| n > 0)`
- **Key guard:** `handle_key()` checks `self.quickfix_has_focus` after `grep_open` guard; routes to `handle_quickfix_key()` — j/k/Ctrl-N/Ctrl-P navigate, Enter jumps + returns focus to editor, q/Escape closes panel
- **Persistent bottom strip:** 6 rows (1 header + 5 results); not a floating modal
- **GTK:** `qf_px = 6 * line_height` subtracted from editor `content_bounds` height when open; `draw_quickfix_panel()` draws header row (status_bg/fg) + result rows (fuzzy_selected_bg highlight on selected)
- **TUI:** extra `Constraint::Length(qf_height)` slot (`qf_height = 6` or 0) in vertical layout; `render_quickfix_panel()` draws header + items; `quickfix_scroll_top: usize` local var with keep-selection-visible logic
- **render.rs:** `QuickfixPanel { items, selected_idx, total_items, has_focus }`; `quickfix: Option<QuickfixPanel>` on `ScreenLayout`; populated in `build_screen_layout()` from `engine.quickfix_open && !engine.quickfix_items.is_empty()`
- File changes: `src/core/engine.rs` (4 fields, new `impl Engine` block with 8 methods, commands, key guard, 8 tests), `src/render.rs` (QuickfixPanel struct, ScreenLayout field, population), `src/main.rs` (qf_px calc, draw_quickfix_panel fn + call), `src/tui_main.rs` (layout change, render_quickfix_panel fn, quickfix_scroll_top var + tracking, draw_frame param)
- Tests: 563 → 571 total

---

## Recently Completed (Session 54)

### ✅ Live Grep (Telescope-style)
- **`Ctrl-G`** in Normal mode opens a centered two-column floating grep modal
- **Search engine:** reuses `project_search::search_in_project()` + `SearchOptions::default()` (case-insensitive, no regex, no whole-word); capped at 200 matches; fires when query ≥ 2 chars
- **Preview:** `grep_load_preview()` reads ±5 context lines from disk; flags the match line with `is_match=true`
- **Navigation:** `grep_select_next/prev()` (clamped, each calls `grep_load_preview()`); `grep_confirm()` opens file at match line + closes modal; `handle_grep_key()` routes Escape/Enter/Up/Down/Ctrl-N/Ctrl-P/Backspace/printable
- **Key guard:** `handle_key()` checks `self.grep_open` before mode dispatch; Ctrl-G in `handle_normal_key()` calls `open_live_grep()`
- **render.rs:** `LiveGrepPanel { query, results, selected_idx, total_matches, preview_lines }`; `live_grep: Option<LiveGrepPanel>` on `ScreenLayout`; reuses all fuzzy theme colors
- **GTK:** `draw_live_grep_popup()` — 80% × 65% centered; title, query, horizontal separator, vertical separator at 40%; left pane results with ▶ indicator, right pane preview with match line highlighted in `fuzzy_title_fg`; Cairo `save/rectangle/clip/restore` around each pane prevents text spill; stateless `scroll_top = selected_idx + 1 - visible_rows` computed each frame keeps selection visible
- **TUI:** `render_live_grep_popup()` — box-drawing chars with ╭╮╰╯├┤┬┴; left pane 35% width, right pane preview; `grep_scroll_top: usize` local var; sidebar suppressed (`!engine.grep_open`); `draw_frame()` gets `grep_scroll_top` param
- File changes: `src/core/engine.rs` (5 fields, `impl Engine` block with 10 methods, Ctrl-G binding, key guard, 8 tests), `src/render.rs` (LiveGrepPanel struct, ScreenLayout field, populate in build_screen_layout; char-aware snippet truncation to avoid multi-byte UTF-8 panic), `src/main.rs` (draw_live_grep_popup + call; Cairo clipping + stateless scroll fix), `src/tui_main.rs` (render_live_grep_popup + grep_scroll_top + sidebar guard + draw_frame param)
- Tests: 555 → 563 total

---

## Recently Completed (Session 53)

### ✅ Fuzzy File Finder (Telescope-style)
- **`Ctrl-P`** in Normal mode opens a centered floating modal over the editor
- **File walking:** `walk_for_fuzzy()` recursively walks `cwd`; skips hidden dirs/files and `target/`; stores relative `PathBuf`s; sorted alphabetically; built once on open
- **Fuzzy scoring:** `fuzzy_score(path, query)` — subsequence match with gap penalties (`score -= gap`) and word-boundary bonuses (+5 for matches after `/`, `_`, `-`, `.`); returns `None` if not all query chars match
- **Filtering:** `fuzzy_filter()` — empty query shows first 50 files alphabetically; non-empty query scores all files, sorts by score desc, caps at 50
- **Navigation:** `fuzzy_select_next/prev()` (clamped); `fuzzy_confirm()` opens file + closes modal; `handle_fuzzy_key()` routes Escape/Enter/Up/Down/Ctrl-N/Ctrl-P/Backspace/printable
- **Key guard:** `handle_key()` checks `self.fuzzy_open` before mode dispatch; Ctrl-P in `handle_normal_key()` calls `open_fuzzy_finder()`
- **render.rs:** `FuzzyPanel { query, results, selected_idx, total_files }`; `fuzzy: Option<FuzzyPanel>` on `ScreenLayout`; 6 new theme colors (`fuzzy_bg`, `fuzzy_selected_bg`, `fuzzy_fg`, `fuzzy_query_fg`, `fuzzy_border`, `fuzzy_title_fg`)
- **GTK:** `draw_fuzzy_popup()` — 60% × 55% centered rectangle; title row, query row ("> query▌"), separator line, result rows with ▶ selection indicator
- **TUI:** `render_fuzzy_popup()` — box-drawing chars (╭╮╰╯├┤─│); `fuzzy_scroll_top: usize` local var; scroll adjusts after each key in editor section; sidebar suppressed (`!engine.fuzzy_open`) while modal is open; `draw_frame()` gets `fuzzy_scroll_top` param
- File changes: `src/core/engine.rs` (6 fields, `impl Engine` block with 9 methods, Ctrl-P binding, key guard, 11 tests), `src/render.rs` (FuzzyPanel struct, ScreenLayout field, 6 theme colors), `src/main.rs` (draw_fuzzy_popup + call in draw_editor), `src/tui_main.rs` (render_fuzzy_popup + fuzzy_scroll_top + sidebar guard + draw_frame param)
- Tests: 544 → 555 total

---

## Recently Completed (Session 52)

### ✅ :norm Command
- **`:norm[al][!] {keys}`** — execute normal-mode keystrokes on a line range; `!` accepted and treated identically
- **Ranges:** no range (current line), `%` (all lines), `'<,'>` (visual selection), `N,M` (1-based numeric)
- **Key decoding:** local decode loop (does not touch `macro_playback_queue`); supports `<CR>`, `<BS>`, `<C-x>`, `<Left>`/`<Right>`/etc.
- **Single undo:** all changes from `:norm` collapsed into one undo entry (undo with single `u`); achieved by recording undo-stack depth before execution and merging new entries after
- **Trim fix:** norm check runs before `cmd.trim()` so trailing spaces in keys (e.g. `I// `) are preserved
- **Free helpers:** `try_parse_norm()` and `norm_numeric_range_end()` (module-level)
- File changes: `src/core/engine.rs` (`execute_norm_command` method, dispatch in `execute_command`, 2 free helpers, 9 new tests; `UndoEntry` added to imports)
- Tests: 535 → 544 total

---

## Recently Completed (Session 51)

### ✅ it/at Tag Text Objects
- **`it` (inner tag)** — selects content between nearest enclosing HTML/XML open+close tag pair; works with all operators (`d`, `c`, `y`) and visual mode (`v`)
- **`at` (around tag)** — selects the full element including opening and closing tags
- **Algorithm:** backward scan for nearest `<tagname>` open tag, forward scan to matching `</tagname>` with nesting depth tracking; cursor must be within element extent
- **Case-insensitive:** `<DIV>text</div>` treated as a valid pair
- **Nested tags:** `<div><div>inner</div>outer</div>` — cursor in inner selects only inner content
- **Attributes:** `<div class="foo">content</div>` — attribute values with `"` or `'` handled correctly
- **Self-closing / comments skipped:** `<br/>`, `<!--...-->`, `<!DOCTYPE>`, `<?...?>` not treated as enclosing tags
- File changes: `src/core/engine.rs` (`find_tag_text_object` method, `'t'` arm in `find_text_object_range`, 9 new tests)
- Tests: 526 → 535 total

---

## Recently Completed (Session 50)

### ✅ CPU Performance Fixes
- **Cached `max_col`:** `BufferState` now stores `max_col: usize`; initialized in both constructors; computed once in `update_syntax()` instead of O(N_lines) scan per render frame in `render.rs`
- **60fps frame rate cap:** TUI event loop limits renders to ~60fps via `min_frame = Duration::from_millis(16)` and `last_draw: Instant`; eliminates uncapped rendering from rapid LSP/search events
- File changes: `src/core/buffer_manager.rs` (max_col field + compute in update_syntax), `src/render.rs` (use cached max_col), `src/tui_main.rs` (frame rate gate + Instant import)
- Tests: no change (526 total)

---

## Recently Completed (Session 49)

### ✅ 6 High-Priority Vim Features
- **Toggle case:** `~` toggles case of char(s) under cursor; count support (5~); dot-repeatable; visual `~` for selections
- **Scroll cursor:** `zz` (center), `zt` (top), `zb` (bottom) — adjusts `scroll_top` without moving cursor
- **Join lines:** `J` joins next line, collapses leading whitespace to one space (no space before `)`, `]`, `}`); count; dot-repeatable
- **Search word under cursor:** `*` (forward) / `#` (backward) with whole-word boundaries; `n`/`N` continue bounded search; clears on new `/`/`?`
- **Jump list:** `Ctrl-O` (back) / `Ctrl-I` (forward); max 100 entries; cross-file; push on G, gg, /, n, N, %, {, }, gd, *, #
- **Indent/dedent:** `>>` / `<<` indent/dedent count lines by `shiftwidth`; visual `>`/`<`; dot-repeatable; respects `expandtab`
- File changes: `engine.rs` (+600 lines, 6 new ChangeOp variants, 6 new helper sets, 31 new tests), `README.md`, `PROJECT_STATE.md`, `PLAN.md`
- Tests: 495 → 526 total

---

## Recently Completed (Session 48)

### ✅ LSP Bug Fixes + TUI Performance Optimizations
- **Protocol compliance:** `notify_did_open` returns `Result<(), String>` with descriptive errors; initialization guards on all notification methods prevent premature `didOpen`/`didChange`/`didSave`/`didClose`
- **Deterministic response routing:** `pending_requests: Arc<Mutex<HashMap<i64, String>>>` maps request ID → method name; reader thread uses this for correct routing instead of fragile content-based guessing
- **Server request handling:** reader thread now responds to server-initiated requests (e.g. `window/workDoneProgress/create`) with `{"result": null}`; error responses generate proper events with empty data
- **Diagnostic flood optimization:** events capped at 50 per `poll_lsp()` call; pre-computed visible buffer paths (computed once, not per-event); only trigger redraw for diagnostics affecting visible buffers
- **Path mismatch fix:** LSP diagnostics keyed by absolute URI-derived paths; buffer `file_path` may be relative; added `canonicalize()` at lookup points in `render.rs`, `diagnostic_counts()`, `jump_next_diagnostic()`, `jump_prev_diagnostic()`
- **TUI on-demand rendering:** `needs_redraw` flag eliminates unconditional 50 FPS rendering; adaptive poll timeout (1ms when redraw pending, 50ms when idle)
- **Idle-only background work:** `lsp_flush_changes()`, `poll_lsp()`, `poll_project_search()`, `poll_project_replace()` moved to only run when no input is pending — prevents blocking pipe writes during typing
- **stderr fix:** reverted `Stdio::inherit()` to `Stdio::null()` for child process stderr (rust-analyzer stderr was corrupting TUI display)
- File changes: `lsp.rs` (750→1186 lines), `lsp_manager.rs` (340→394 lines), `engine.rs` (+400 lines), `tui_main.rs` (+80 lines), `render.rs` (+100 lines)
- Tests: no change (495 total)

---

## Recently Completed (Session 47)

### ✅ LSP Support (Language Server Protocol)
- **New files:** `src/core/lsp.rs` (~750 lines), `src/core/lsp_manager.rs` (~340 lines)
- **Dependency:** `lsp-types = "0.97"` (protocol type definitions, no runtime)
- **Architecture:** lightweight custom LSP client using `std::thread` + `std::sync::mpsc` (same pattern as project search); no tokio/async runtime
- **Built-in server registry:** rust-analyzer, pyright-langserver, typescript-language-server, gopls, clangd — auto-detected on `PATH`
- **Protocol transport:** `LspServer::start()` spawns subprocess, reader thread parses JSON-RPC frames, dispatches `LspEvent`s via channel; full document sync
- **Multi-server coordinator:** `LspManager` routes notifications/requests by language ID, lazy-starts servers on first use
- **Engine integration:** 4 hook points (open/change/save/close); `poll_lsp()` processes diagnostics/completions/definition/hover events; debounced `didChange` via dirty buffer tracking
- **Key bindings:** `]d`/`[d` (diagnostic nav), `gd` (go-to-definition), `K` (hover), `Ctrl-Space` (LSP completions)
- **Commands:** `:LspInfo`, `:LspRestart`, `:LspStop`
- **Settings:** `lsp_enabled` bool + `lsp_servers` array; `:set lsp`/`:set nolsp` toggle
- **Render layer:** `DiagnosticMark` + `HoverPopup` types; `diagnostic_gutter` map; diagnostic/hover theme colours
- **GTK backend:** wavy underlines via Cairo curves, colored gutter dots, hover popup, LSP poll in SearchPollTick, shutdown on quit
- **TUI backend:** colored underlines + E/W/I/H gutter chars, hover popup, LSP poll in event loop, shutdown on quit
- **Status bar:** `E:N W:N` diagnostic counts in right section
- Tests: 37 new (27 lsp-protocol + 10 lsp-engine); 458→495 total

---

## Recently Completed (Session 46)

### ✅ TUI Scrollbar Drag Fix
- **Immediate h-scroll**: Removed deferred `pending_h_scroll` mechanism — h-scrollbar drag now calls `set_scroll_left_for_window` immediately during drag (matching v-scrollbar behaviour)
- **Drag event coalescing**: After processing a `MouseEventKind::Drag` event, drains all additional queued drag events via `ct_event::poll(Duration::ZERO)`, keeping only the final mouse position; benefits all drag operations (h-scrollbar, v-scrollbar, sidebar resize)
- **Unified scrollbar colour**: V-scrollbar thumb changed from `theme.status_fg` (`#e5e5e5`) to `Rgb(128, 128, 128)` grey to match h-scrollbar
- **Cleanup**: Removed `pending_h_scroll` parameter from `handle_mouse`, `draw_frame`, `render_all_windows`, `render_window`, and `render_h_scrollbar` signatures
- Tests: no change (458 total)

---

## Recently Completed (Session 45)

### ✅ Replace Across Files
- **`replace_in_project()`** in `project_search.rs`: walks files with `ignore` crate, applies `regex::replace_all`, writes back only changed files; `NoExpand` wrapper prevents `$` interpretation in literal mode; files in `skip_paths` are skipped and reported
- **`ReplaceResult` struct**: `replacement_count`, `file_count`, `skipped_files`, `modified_files`
- **`build_search_regex()` refactor**: extracted shared regex builder from `search_in_project` for reuse by both search and replace
- **Engine integration**: `project_replace_text` field; `start_project_replace` (async) / `poll_project_replace` / `run_project_replace` (sync); `apply_replace_result` reloads open buffers, clears undo stacks, refreshes git diff, builds status message with skip info
- **GTK**: Replace `Entry` + "Replace All" button between toggle row and status label; `ProjectReplaceTextChanged` / `ProjectReplaceAll` messages; replace poll piggybacked on `SearchPollTick`
- **TUI**: `replace_input_focused` field; `Tab` switches between search/replace inputs; `Enter` in replace box triggers replace; `Alt+H` shortcut; new `[Replace…]` input row (row 2); all layout offsets shifted +1; mouse click routing updated
- **Tests**: 14 new (9 project_search replace tests + 5 engine replace tests); 444→458 total

---

## Recently Completed (Session 44)

### ✅ Enhanced Project Search (Regex, Whole Word, Case Toggle + Performance)
- **`ignore` crate walker**: Replaced hand-rolled `walk_dir` with `ignore::WalkBuilder` (same as ripgrep) — respects `.gitignore`, skips `target/`, binary detection via UTF-8 decode
- **`regex` crate matching**: `SearchOptions` struct with `case_sensitive`, `whole_word`, `use_regex` toggles; builds `regex::Regex` from query + options; invalid regex returns `SearchError`
- **Result cap**: Max 10,000 matches to prevent memory issues; status message shows "(capped at 10000)" when hit
- **Engine integration**: `project_search_options` field; async channel changed to `Result<Vec<ProjectMatch>, SearchError>`; 3 toggle methods
- **GTK**: 3 `ToggleButton` widgets (`Aa`, `Ab|`, `.*`) with CSS styling; 3 new `Msg` variants
- **TUI**: `Alt+C`/`Alt+W`/`Alt+R` toggles in both input and results mode; toggle indicator row with active/inactive coloring
- **Tests**: 6 new (case-sensitive, whole-word, regex, invalid-regex, whole-word+regex combo, gitignore-respected); 438→444 total

---

## Recently Completed (Session 43)

### ✅ Search Panel Bug Fixes
- **GTK CSS fix**: Changed CSS selectors from `listbox` / `listbox row` to `.search-results-list` / `.search-results-list > row` — GTK4 uses `list` as the CSS node name for `GtkListBox`, so the old selectors never matched; replaced `.search-results-scroll > viewport` with `.search-results-scroll` on the ScrolledWindow itself
- **GTK startup crash fix**: `sync_scrollbar` called during initial `connect_resize` with near-zero dimensions caused `(rect.height - 10.0) as i32` to be negative, rejected by GTK; added early return guard (`da_width < 20.0 || da_height < 20.0`) and clamped `.max(0)`
- **TUI search scrollbar drag**: New `SidebarScrollDrag` struct for drag state; `Down` click on search scrollbar column arms drag; `Drag` event proportionally scrolls `search_scroll_top`; `Up` clears drag
- **TUI j/k scroll-into-view**: `j`/`k` in search results now call `ensure_search_selection_visible` to keep the selected result in the viewport

---

## Recently Completed (Session 42)

### ✅ Search Panel Polish + CI Fix
- **TUI scroll redesign**: `search_scroll_top` is now an independent viewport offset driven by scroll wheel/scrollbar clicks; selection only adjusts scroll when it leaves the viewport (mirrors how Explorer and Editor scrolling work)
- **TUI scrollbar interactivity**: Explorer scrollbar column click → jump-scroll (`sidebar.scroll_top`); Search scrollbar column click → jump-scroll (`sidebar.search_scroll_top`); scroll wheel in sidebar area scrolls Explorer or Search content
- **GTK dark background**: `.search-results-scroll > viewport` CSS targets the internal GTK viewport widget; `.search-results-list label { color: #cccccc; }` fixes grey text; `set_overlay_scrolling: false` makes scrollbar always visible
- **Threaded search**: `engine.start_project_search(PathBuf)` spawns a thread and stores `Receiver`; `engine.poll_project_search() -> bool` checks for results non-blocking; GTK polls via `glib::timeout_add_local(50ms)`; TUI polls each frame
- **CI clippy fix**: Two `map_or(false, ...)` → `is_some_and(...)` in `engine.rs` (lint added in Rust 1.84+)
- **Tests**: 4 new engine-level project search tests (434 → 438 tests)

---

## Recently Completed (Session 41)

### ✅ VSCode-Style Project Search Panel
- Ctrl-Shift-F (GTK + TUI) opens Search panel in sidebar
- `src/core/project_search.rs`: `ProjectMatch` struct + `search_in_project(root, query)`
  - Recursive walk, skips hidden (`.`) dirs/files, skips binary (non-UTF-8) files
  - Case-insensitive literal match; sorted by file path then line number
- Engine: 3 new fields (`project_search_query/results/selected`) + 3 new methods
- GTK: Search button in activity bar enabled; `gtk4::Entry` + `gtk4::ListBox`; file-header rows + result rows; click opens file at matched line
- TUI: `TuiPanel::Search`; `search_input_mode` bool; `render_search_panel()`; input/results keyboard modes; j/k navigation; Enter opens file
- Activity bar row order: Explorer (0) → Search (1) → Settings (2)
- Tests: 5 new (429→434)

---

## Recently Completed (Session 40)

### ✅ Paragraph and Sentence Text Objects
- `ip` / `ap` — inner/around paragraph (contiguous non-blank lines); `ap` includes trailing blank lines
- `is` / `as` — inner/around sentence (`.`/`!`/`?`-terminated); `as` includes trailing whitespace
- Both work with all operators: `d`, `c`, `y`, `v` (visual selection)
- `ip` on a blank line selects the contiguous blank-line block
- Paragraph boundary (blank line) also terminates a sentence
- Tests: 9 new (420→429)

---

## Recently Completed (Session 39)

### ✅ Stage Hunks
- `]c` / `[c` — jump to next/previous `@@` hunk header in diff buffer
- `gs` (via `g` + `s` pending key) — stage hunk under cursor using `git apply --cached`
- `:Ghs` / `:Ghunk` — command-line aliases for stage hunk
- `Hunk` struct + `parse_diff_hunks()` in `git.rs` — pure string parsing, no subprocess
- `run_git_stdin()` — pipes patch text to git subprocess stdin
- `stage_hunk()` — builds minimal patch and feeds it to `git apply --cached -`
- `BufferState.source_file: Option<PathBuf>` — set by `:Gdiff` so hunk staging knows which file to patch
- After staging: refreshes gutter markers on the source buffer if it's open
- Tests: 10 new (410→420)

---

## Recently Completed (Session 38)

### ✅ :set Command
- Vim-compatible `:set option`, `:set nooption`, `:set option=N`, `:set option?`, `:set` (show all)
- Write-through: every change immediately saved to `settings.json`
- Boolean options: `number`/`nu`, `relativenumber`/`rnu`, `expandtab`/`et`, `autoindent`/`ai`, `incsearch`/`is`
- Numeric options: `tabstop`/`ts`, `shiftwidth`/`sw`
- `number` + `relativenumber` → Hybrid line number mode (vim-accurate)
- New settings fields: `expand_tab` (default true), `tabstop` (default 4), `shift_width` (default 4)
- Tab key uses `expand_tab`/`tabstop` instead of hardcoded 4 spaces
- Tests: 22 new (388→410)

---

## Recently Completed (Session 37)

### ✅ Auto-Indent
- Enter/`o`/`O` in insert mode copies leading whitespace of current line to new line
- Controlled by `auto_indent` setting (default: true)
- Tests: 5 new (369→374)

### ✅ Completion Menu (Ctrl-N / Ctrl-P)
- In insert mode: scans buffer for words matching prefix at cursor
- Floating popup (max 10 candidates), cycles on repeated Ctrl-N/P
- Any other key dismisses and accepts current candidate
- GTK: Cairo/Pango popup; TUI: ratatui buffer cells with border
- New engine fields: `completion_candidates`, `completion_idx`, `completion_start_col`
- New render types: `CompletionMenu` + four completion colours in `Theme`
- Tests: 4 new (374→378)

### ✅ Quit / Save Commands
- `:q` — closes current tab; quits if it's the last one (blocked if dirty)
- `:q!` — force-closes current tab; force-quits if last
- `:qa` — quit all (blocked if any dirty buffer)
- `:qa!` — force-quit all
- `Ctrl-S` — save current buffer in any mode without changing mode
- Tests: 9 new (378→387)

### ✅ Session Restore Fix
- Each file in `open_files` restored into its own tab on startup
- Previously-active file's tab is focused
- `open_file_paths()` filters to window-visible buffers so files closed via
  `:q` are not re-opened next session
- Tests: 1 new (387→388)

---

## Recently Completed (Sessions 29–36)

### ✅ TUI Backend (Sessions 29–30)
- Full ratatui/crossterm terminal UI with sidebar, mouse, scrollbars, resize

### ✅ Code Folding (Session 31)
- `za`/`zo`/`zc`/`zR`; gutter indicators; clickable gutter column

### ✅ Session File Restore (Session 32)
- Open file list saved on quit and restored on next launch

### ✅ Git Integration (Sessions 33–35)
- Gutter markers, branch in status bar, `:Gdiff`, `:Gstatus`, `:Gadd`, `:Gcommit`, `:Gpush`, `:Gblame`

### ✅ Explorer Preview (Session 35)
- Single-click → preview tab (italic); double-click → permanent

### ✅ Scrollbar Polish (Session 36)
- Per-window vertical + horizontal scrollbars in TUI; drag support; scroll sync

---

## Roadmap

### Git
- [x] **Stage hunks** — `]c`/`[c` navigation, `gs`/`:Ghs` to stage hunk under cursor

### Editor Features
- [x] **`:set` command** — runtime setting changes; write-through to settings.json; number/rnu/expandtab/tabstop/shiftwidth/autoindent/incsearch; query with `?`
- [x] **`ip`/`ap` paragraph text objects** — inner/around paragraph (contiguous non-blank lines)
- [x] **`is`/`as` sentence text objects** — inner/around sentence (`.`/`!`/`?`-delimited)
- [x] **Enhanced project search** — regex/case/whole-word toggles; `.gitignore`-aware via `ignore` crate; 10k result cap; GTK toggle buttons + TUI Alt+C/W/R
- [x] **VSCode-style replace across files** — replace all matches in project; skip dirty buffers; reload open buffers; regex capture group backreferences
- [x] **`:grep` / `:vimgrep`** — project-wide search, populate quickfix list
- [x] **Quickfix window** — `:copen`, `:cn`, `:cp` navigation
- [x] **`it`/`at` tag text objects** — inner/around HTML/XML tag

### Big Features
- [x] **LSP support** — completions, go-to-definition, hover, diagnostics (session 47 + 48 bug fixes)
- [x] **`gd` / `gD`** — go-to-definition via LSP
- [x] **`:norm`** — execute normal command on a range of lines
- [x] **Fuzzy finder / Telescope-style** — Ctrl-P opens centered file-picker modal with subsequence scoring (session 53)
- [ ] **Multiple cursors** — `Ctrl-D` adds cursor at next match of word under cursor; all cursors receive identical keystrokes; Escape collapses to one
- [ ] **Themes / plugin system** — named color themes selectable via `:colorscheme`; theme file format TBD

### Enhanced Editor
- [x] **Autosuggestions (inline ghost text)** — as-you-type completions shown as dimmed ghost text inline after the cursor; sources: buffer word scan (sync) + LSP `textDocument/completion` (async); Tab accepts, any other key dismisses; coexists with Ctrl-N/Ctrl-P popup (ghost hidden when popup active)
- [x] **Edit mode toggle** — `editor_mode` setting (`"vim"` default | `"vscode"`); `:set mode=vscode`; `Alt-M` runtime toggle; Shift+Arrow selection, Ctrl+Arrow word nav, Ctrl-C/X/V/Z/Y/A shortcuts, Ctrl+/ comment toggle, smart Home; status bar shows EDIT/SELECT; session 66

### Terminal & Debugger
- [x] **Integrated terminal** — VSCode-style 13-row bottom panel; `portable-pty` + `vt100`; Ctrl-T toggle + `:term` command; full 256-color cell rendering; mouse selection; Nerd Font toolbar; shell session persists on close (session 68)
- [ ] **Terminal: multiple tabs** — tab strip in toolbar; `Vec<TerminalPane>`; Ctrl-T N to switch
- [ ] **Terminal: draggable panel height** — same GestureDrag pattern as sidebar resize
- [ ] **Terminal: scrollback navigation** — ring buffer of completed rows + scroll_offset; PgUp/PgDn while focused
- [ ] **Terminal: TUI Ctrl+F find** — general-purpose find dialog in TUI (currently GTK-only)
- [ ] **Debugger (DAP)** — Debug Adapter Protocol client (analogous to LSP but for debugging); auto-detect `codelldb`, `debugpy`, `js-debug`; breakpoints set with `<F9>` or `:Breakpoint`; step over/in/out (`<F10>`/`<F11>`/`<F12>`); call stack + variables panel in sidebar; inline variable values shown as virtual text; status bar shows current debug state

### UI & Menus
- [ ] **VSCode-style menus** — application menu bar (File / Edit / View / Go / Run / Terminal / Help) in GTK; command palette (`Ctrl-Shift-P`) lists all commands + key bindings; fuzzy-searchable; both GTK native menus and TUI pop-up menu overlay
- [ ] **Command palette** — `Ctrl-Shift-P` floating modal (like Telescope but for commands); lists named commands with descriptions and current keybindings; typing filters; Enter executes; shared between GTK and TUI

### Extension System
- [ ] **Extension mechanism** — WASM or Lua plugin sandbox (TBD); plugins can: register commands (`:MyCmd`), add key bindings, hook into editor events (on-save, on-open, on-key), read/write buffer text, show messages; `~/.config/vimcode/extensions/` directory auto-loaded; `:ExtInstall <url>`, `:ExtList`, `:ExtDisable`

### AI Integration
- [ ] **AI assistant panel** — VSCode Copilot-style sidebar chat panel; configurable provider (Anthropic Claude API, OpenAI, Ollama local); `api_key` in settings; `Alt-A` opens panel; multi-turn conversation with editor context (current file, selection, diagnostics); "Insert at cursor" / "Replace selection" actions on responses
- [ ] **AI inline completions** — ghost-text completions from AI provider interleaved with LSP ghost text; separate `ai_completions` setting (default false to avoid unexpected API costs); debounced after 500ms idle in insert mode; Tab accepts whole suggestion, `Alt-]`/`Alt-[` cycle through alternatives
