# VimCode Implementation Plan

## Recently Completed

**Session 98 — Lua Extension Mechanism (Phase D complete):** `Cargo.toml`: `mlua = { version = "0.9", features = ["lua54", "vendored"] }`. `src/core/mod.rs`: `pub mod plugin;`. `src/core/settings.rs`: `plugins_enabled: bool` + `disabled_plugins: Vec<String>` fields. `src/core/plugin.rs` (new, ~430 lines): `PluginManager`; `LoadedPlugin`; `#[derive(Default)] PluginCallContext`; `PluginRegistrations`; `setup_vimcode_api()` (`vimcode.on/command/keymap/message/cwd/command_run/buf.*`); `load_plugins_dir(dir, disabled)` / `load_one_plugin()`; `call_command/call_event/call_keymap` dispatch; 6 unit tests. `src/core/engine.rs`: `plugin_manager: Option<plugin::PluginManager>` field; `plugin_init()` (loads `~/.config/vimcode/plugins/`); `make_plugin_ctx()` / `apply_plugin_ctx()`; `plugin_event/plugin_run_command/plugin_run_keymap` methods (take/put-back borrow pattern); hook points in `save()`, `lsp_did_open()`, `execute_command()` `_ =>` arm, `handle_normal_key()` final return, `handle_insert_key()` `_ =>` arm; `:Plugin list/reload/enable/disable` commands; 3 engine-integration tests. 801 tests (+9).

**Session 95 — C# Non-Public Members grouping + Debug Output scrollbar + scrollbar height fix:** `dap.rs`: `is_nonpublic: bool` on `DapVariable`. `engine.rs`: `SYNTHETIC_NON_PUBLIC_MASK` constant; variables response partitions C# private fields (`_name` prefix, `<Name>k__BackingField`) into a synthetic "Non-Public Members" group node using high-bit ref masking; `dap_toggle_expand_var` handles synthetic refs (no server fetch, persists cache for re-expansion, cleans up on real-var collapse). `render.rs`: `build_var_tree` omits ` = ` for empty-value entries (group headers). `tui_main.rs`: `debug_output_scroll`/`dragging_debug_output_sb` state; `handle_mouse` updated with two new params; drag/scroll-wheel/click handlers for debug output scrollbar; `render_debug_output` renders `█`/`░` scrollbar; fixed both pre-draw and pre-keyboard height-computation sites to include `debug_out_open` in `bp_h` so `ensure_visible` uses correct sidebar height when debug output panel is open. 784 tests (no change).

**Session 94 — Per-Section Scrollbars in Debug Sidebar:** `engine.rs`: 2 new fields (`dap_sidebar_scroll: [usize; 4]`, `dap_sidebar_section_heights: [u16; 4]`); `dap_sidebar_section_index()` maps enum→index; `dap_sidebar_ensure_visible()` auto-adjusts scroll on j/k/x/d; `dap_sidebar_resize_section()` trades rows between adjacent sections; `dap_stop()` resets scroll. `render.rs`: `DebugSidebarData` gains `scroll_offsets`/`section_heights`. `tui_main.rs`: `render_debug_sidebar` rewritten with fixed-height section allocation + per-section scrollbar (`█`/`░`); click handler uses scroll offset. `main.rs`: `draw_debug_sidebar` rewritten with Cairo clip regions + scrollbar thumbs; section heights computed in click/key handlers. 10 new tests. 784 tests (+10).

**Session 93 — Scope-Grouped Variables in Debug Sidebar:** `engine.rs`: `dap_scope_groups: Vec<(String, u64)>` field; `poll_dap` scopes handler parses all non-expensive scopes (first→`dap_variables`, rest→scope groups); flat index helpers updated for scope group headers + expanded children; cleared on stop/frame-select; 5 new tests. `render.rs`: scope groups rendered as expandable `▶`/`▼` headers below primary variables with children via `build_var_tree()`. 774 tests (+5).

**Session 92 — VSCode tasks.json + preLaunchTask Execution:** `dap_manager.rs`: `TaskDefinition` struct; `parse_tasks_json()` + `task_to_shell_command()` with 8 tests. `engine.rs`: `dap_pre_launch_done`/`dap_deferred_lang` fields; `dap_start_debug()` migrates `.vscode/tasks.json` → `.vimcode/tasks.json`, extracts `preLaunchTask` from config, runs matching task via background thread, resumes debug on completion; `poll_lsp` `InstallComplete` handles `"dap_task:"` prefix; `dap_stop()` resets new fields. 769 tests (+8).

**Session 91 — Debug Sidebar Mouse + Keyboard Interactivity:** Wired debug sidebar keyboard and mouse input in both GTK and TUI backends. Engine: `dap_sidebar_has_focus` field + key guard in `handle_key()`; q/Escape unfocus; `dap_sidebar_section_item_count()` for per-section counts; `DebugSidebarSection` gains `Copy`; removed dead_code annotations. TUI: keyboard block for Debug panel (j/k/Tab/Enter/x/d/q/Escape); click handler walks sections to map row→action. GTK: `EventControllerKey` on `debug_sidebar_da` with `set_focusable(true)`; `Msg::DebugSidebarKey` variant; expanded click handler with section/item mapping; focus management via `grab_focus()`. 758 tests.

**Session 90 — Interactive Debug Sidebar:** Wired expand/collapse for variables (Enter/Space toggles `dap_toggle_expand_var`), call stack frame navigation (Enter selects frame via `dap_select_frame` + opens source file at stopped line), breakpoint jumping (Enter navigates to file/line). Added conditional breakpoints: `BreakpointInfo` struct replaces raw `u64` line numbers; `condition`, `hit_condition`, `log_message` fields sent via DAP `setBreakpoints`; `:DapCondition`/`:DapHitCondition`/`:DapLogMessage` commands. Sidebar navigation: j/k clamped to section length, Tab cycles sections, x/d deletes watch expressions or breakpoints. Gutter: `◆` for conditional breakpoints. Recursive `build_var_tree()` in render.rs for nested variable display. 12 new tests. 758 tests.

**Session 89 — DAP debugger polish + codelldb compatibility:** Fixed codelldb adapter launching (missing `lldb-server` in install script). Added `pending_commands: HashMap<u64, String>` to `DapServer` for seq→command tracking (codelldb omits command field from responses). Deferred launch via `dap_seq_initialize`. Three-state debug sidebar button (Start/Stop/Continue). Breakpoint gutter: `▶` current line, `◉` current+BP, `●` BP only. Navigate-to-line: opens source file and centers stopped line via `scroll_cursor_center()`. ANSI stripping for DAP output. Default `"stdio": null` for codelldb. Auto-switch to Debug sidebar on session start (`dap_wants_sidebar` flag). `DebugSidebarData.stopped: bool`. 746 tests.

**Session 88 — VSCode-like Debugger UI:** `dap_manager.rs`: `LaunchConfig` struct; `parse_launch_json(content, workspace_folder)`; `type_to_adapter()`; `generate_launch_json(lang, workspace_folder)`; `detect_rust_package_name()`; 10 new tests. `engine.rs`: `DebugSidebarSection`/`BottomPanelKind` enums; removed `dap_panel_has_focus`; 8 new DAP fields; `debug_toolbar_visible` defaults false; `dap_start_debug()` reads/generates `.vscode/launch.json`; `dap_add_watch`/`dap_remove_watch`; `handle_debug_sidebar_key()`; `:DapWatch`/`:DapBottomPanel` commands; watch expr evaluation after stop; 5 new tests. `render.rs`: replaced `DapPanel` with `DebugSidebarData`/`BottomPanelTabs`; `build_screen_layout` builds all 4 sections. `main.rs`: `SidebarPanel::Debug`; debug activity bar button; `debug_sidebar_da` DrawingArea; `draw_debug_sidebar()`/`draw_bottom_panel_tabs()`/`draw_debug_output()`; removed dap panel strip. `tui_main.rs`: `TuiPanel::Debug`; debug activity bar row 3; `render_debug_sidebar()`/`render_bottom_panel_tabs()`/`render_debug_output()`; removed old `render_dap_panel()`; bottom panel now has tab bar. 743 tests (+12).

**Session 85 — DAP Variables Panel + Call Stack + Output Console:** `engine.rs`: import `StackFrame`+`DapVariable`; 3 new fields: `dap_stack_frames: Vec<StackFrame>`, `dap_variables: Vec<DapVariable>`, `dap_output_lines: Vec<String>`; `poll_dap` RequestComplete now chains: stackTrace → parses all frames, derives `dap_current_line`, sends `scopes(frame_id)`; scopes → reads `variablesReference`, sends `variables(var_ref)`; variables → stores in `dap_variables`; Output → appends formatted lines to `dap_output_lines` (capped 1000, drains from front); Continued/Exited → clears frames+vars; 4 new tests. `render.rs`: `DapPanel { frames, active_frame, variables, output_lines }` struct; `ScreenLayout.dap_panel`; populated in `build_screen_layout` when `dap_session_active`. `main.rs`: `DAP_PANEL_ROWS=8`; `dap_px` reservation; subtracted from `content_bounds`; `draw_dap_panel()` (header + col-headers + 3 content rows + 3 output rows); positioned above quickfix. `tui_main.rs`: `dap_panel_height=8` slot in vertical layout; `render_dap_panel()` function. 720 tests (+4).

**Session 84 — DAP Event Loop + Breakpoint Gutter + Stopped-Line Highlight:** `engine.rs`: `dap_current_line: Option<(String, u64)>` field; `rust_debug_binary(cwd)` free function (walks up dir tree to find `Cargo.toml`, reads `[package] name`, returns `target/debug/{name}` or descriptive error); `dap_start_debug` uses it for Rust with `sourceLanguages: ["rust"]`; `poll_dap` overhauled: removed `#[allow(dead_code)]`; on `Stopped` → sends `stack_trace(thread_id)`; on `RequestComplete{command:"stackTrace",success:true}` → parses `body.stackFrames[0].{line, source.path}` → stores in `dap_current_line`; on `Continued`/`Exited` → clears `dap_current_line`; 4 new tests. `render.rs`: `RenderedLine` gains `is_breakpoint: bool` + `is_dap_current: bool`; `RenderedWindow` gains `has_breakpoints: bool`; `Theme` gains `dap_stopped_bg: Color` (`#3a3000`); `calculate_gutter_cols` gains `has_breakpoints: bool` param; `build_rendered_window` computes `has_bp` (file has BPs or `dap_session_active`), per-line `is_breakpoint` + `is_dap_current`, prepends `◉`/`●`/` ` before git char in gutter_text. `main.rs`: `poll_dap()` in `SearchPollTick`; both `calculate_gutter_cols` call sites updated; bg block handles `dap_stopped_bg`; gutter renders bp char at col 0, git at col 1 when both present. `tui_main.rs`: `poll_dap()` in idle loop; `line_bg` checks `is_dap_current` first; gutter bp col colored at `i==0`, git at `i==bp_offset`. 716 tests (+4).

**Session 83 — DAP Protocol Transport + :DapInstall:** `src/core/dap.rs` (new): Content-Length framing, `DapEvent` enum, `DapServer` with `spawn/send_request/poll` + full request-helper suite, `dap_reader_thread`; 8 tests. `src/core/dap_manager.rs` (new): `AdapterInfo`, `ADAPTER_REGISTRY` (5 adapters: codelldb/rust+c+cpp, debugpy/python, delve/go, js-debug/js+ts, java-debug/java), `resolve_binary`, `DapManager::adapter_for_language/start_adapter/stop`, `install_cmd_for_adapter` (platform-specific: codelldb via curl+unzip from GitHub, debugpy via pip3, delve via go install); 11 tests. `src/core/engine.rs`: 4 new fields (`dap_manager`, `dap_stopped_thread`, `dap_breakpoints`, `dap_seq_launch`); 9 new methods (dap_toggle_breakpoint/start_debug/continue/pause/stop/step_over/step_into/step_out/poll_dap); real DAP dispatch replacing 9 stubs; `:DapInstall <lang>` uses `"dap:{name}"` key so `InstallComplete` shows DAP-specific message without triggering LSP server start; `lsp_manager.rs`: `run_install_command` now combines stdout+stderr on failure. 712 tests (+28).

**Session 82 — Wire Up Menus & Debug Toolbar (UI Interactions):** `engine.rs`: 2 new fields (`dap_session_active`, `menu_highlighted_item`); `open_menu()` resets highlight; `menu_move_selection(delta, is_separator)` / `menu_activate_highlighted() -> Option<(usize,usize)>` methods; `execute_command` made `pub`; 9 debug stub commands ("debug"/"continue"/"pause"/"stop"/"restart"/"stepover"/"stepin"/"stepout"/"brkpt") show "Debugger not yet available" message; F5/F6/F9/F10/F11 dispatch from `handle_normal_key`. `render.rs`: `DebugButton` gains `action: &'static str`; `MenuBarData` gains `highlighted_item_idx`; `build_screen_layout` wires both from engine. `main.rs`: Shift+F5→stop / Shift+F11→stepout routing; debug toolbar pixel hit-test in `Msg::MouseClick`. `tui_main.rs`: Up/Down/Enter dropdown navigation; `render_menu_dropdown` highlights selected row; Shift+F5/F11 routing; menu bar row click / dropdown item click / debug toolbar click in `handle_mouse`. 684 tests (+4 new).

**Session 87 — `:set wrap` / Soft Line-Wrap Rendering:** `settings.rs`: `wrap: bool` field (default false); `"wrap"`/`"nowrap"` in `set_bool_option`, `query_option`, `display_all`; 4 new tests. `render.rs`: `RenderedLine` gains `is_wrap_continuation: bool` + `segment_col_offset: usize`; free functions `visual_rows_for_line`, `slice_spans_for_segment`, `char_to_byte_offset`; `build_rendered_window` splits lines at `viewport_cols` into per-segment `RenderedLine` entries when `settings.wrap=true`; continuation rows have blank gutter, no git/BP markers; cursor `CursorPos.col` adjusted by `segment_col_offset`; `max_col=0` when wrap. `engine.rs`: `ensure_cursor_visible` dispatches to `ensure_cursor_visible_wrap` (counts visual rows from scroll_top); `move_visual_down/up` helpers; `gj`/`gk` in 'g' pending key arm; `engine_visual_rows_for_line` free function. No changes to GTK or TUI backends. 731 tests (+7).

**Session 86 — DAP Panel Interactivity + Expression Evaluation:** `dap.rs`: `evaluate(expression, frame_id)` request helper. `engine.rs`: 6 new fields (`dap_panel_has_focus`, `dap_active_frame`, `dap_expanded_vars: HashSet<u64>`, `dap_child_variables: HashMap<u64,Vec<DapVariable>>`, `dap_eval_result`, `dap_pending_vars_ref`); `dap_stop()` clears all new fields; 5 new methods: `dap_focus_panel()`/`dap_blur_panel()`, `dap_select_frame(idx)` (clamps, clears vars/children/expanded, chains scopes for new frame), `dap_toggle_expand_var(var_ref)` (toggles HashSet, sends variables with dap_pending_vars_ref=var_ref), `dap_eval(expr)` (evaluate request with current frameId), `handle_dap_panel_key()` (Down/j→frame+1, Up/k→frame-1, Esc/q→blur); `handle_key()` guard routes to panel when focused; `poll_dap` scopes arm sets `dap_pending_vars_ref=0`; variables arm routes top-level vs child; evaluate response arm stores in `dap_eval_result` + status; Continued/Exited clear all new fields; `:DapPanel`/`:DapEval`/`:DapExpand` commands. `render.rs`: `DapPanel` gains `has_focus: bool` + `eval_result: Option<String>`; variables built as flat tree with `▶`/`▼` prefixes and 4-space child indent. `main.rs`/`tui_main.rs`: header shows `[FOCUS]` + eval suffix; variables rendered directly. 724 tests (+4).

---

## Recently Completed (Session 81 + polish)

### ✅ Menu Bar + Debug Toolbar Foundation + Mason DAP Detection + Bug Fixes

**Core implementation:**
- **`lsp.rs`** — `MasonPackageInfo.categories: Vec<String>`; `parse_mason_package_yaml()` extracts `categories:` YAML block; `is_lsp()`/`is_dap()`/`is_linter()`/`is_formatter()` helpers; 4 new tests
- **`engine.rs`** — 3 new fields: `menu_bar_visible: bool`, `menu_open_idx: Option<usize>`, `debug_toolbar_visible: bool` (true by default); `toggle_menu_bar()`/`open_menu(idx)`/`close_menu()`/`menu_activate_item(menu_idx, item_idx, action)` methods; `:DapInfo` command; 3 new tests
- **`render.rs`** — `MenuItemData`, `MenuBarData`, `DebugButton`, `DebugToolbarData` types; `MENU_STRUCTURE` static (7 menus: File/Edit/View/Go/Run/Terminal/Help); `DEBUG_BUTTONS` static; `menu_bar`/`debug_toolbar` on `ScreenLayout`
- **`tui_main.rs`** — hamburger `󰍜` at row 0 in activity bar (explorer→row 1, search→row 2); 7-slot vertical layout; `render_menu_bar()`/`render_menu_dropdown()`/`render_debug_toolbar()`; Alt+letter routing
- **`main.rs` (GTK)** — hamburger button at top of activity bar; 4 new `Msg` variants; `draw_menu_bar()`/`draw_menu_dropdown()`/`draw_debug_toolbar()` Cairo functions; Alt+letter routing

**Bug fixes (same session, polish round):**
- **TUI activity bar clicks** — row 0 now correctly triggers `toggle_menu_bar()`; rows 1/2 map to Explorer/Search; `settings_row` calculation corrected for menu+debug offsets
- **TUI render_menu_bar** — removed duplicate hamburger glyph (hamburger lives only in the activity bar)
- **GTK draw_menu_bar** — removed hamburger glyph from the strip (same reason)
- **GTK drawing order** — menu bar moved to `y=0` (above tab bar); `draw_tab_bar` gained `y_offset: f64` parameter
- **GTK full-width menu bar** — `menu_bar_da: gtk4::DrawingArea` added to App struct; GTK layout restructured: outer `gtk4::Box { Vertical }` wraps `menu_bar_da` (full window width) above `main_hbox`; `menu_bar_da` has its own draw func (clones engine) and `GestureClick` handler; height synced via `CacheFontMetrics`; menu bar strip removed from `draw_editor()`; `draw_menu_dropdown` anchor moved to `y=0` in drawing_area; `pixel_to_click_target` and `sync_scrollbar` content_bounds updated accordingly
- **GTK dropdown click detection** — replaced bar-strip detection in `Msg::MouseClick` with dropdown-only logic at `popup_y=0`
- Tests: 673 → 680 (+7 new)

---

## Recently Completed (Session 80)

### ✅ Fix LSP Not Starting for Files Opened via Sidebar / Fuzzy / Split

- **Root cause** — `lsp_did_open(buffer_id)` was only called in `Engine::open()` (CLI startup) and `open_file_with_mode()` (`:e`, quickfix, grep confirm). Four other file-opening functions never triggered LSP initialization, so `:LspInfo` reported `manager=not started` for any file opened via the sidebar, Ctrl-P fuzzy finder, live grep confirm, `:split`/`:vsplit`, or `:tabnew`.
- **`open_file_in_tab()`** — added `self.lsp_did_open(buffer_id)` on all 3 exit paths: preview-promotion early return, existing-tab switch early return, and fall-through (new tab created). Used by sidebar double-click, fuzzy finder, and project search confirm.
- **`open_file_preview()`** — added `self.lsp_did_open(buffer_id)` on both paths: existing-tab switch early return and at the end of the preview-slot if/else block. Used by sidebar single-click.
- **`new_tab()`** — added `self.lsp_did_open(buffer_id)` after `self.active_tab` is set, guarded by `if file_path.is_some()` (no-op for scratch buffers). Used by `:tabnew <file>`.
- **`split_window()`** — added `self.lsp_did_open(new_buffer_id)` before `self.message`, guarded by `if file_path.is_some()` (no-op for same-buffer splits). Used by `:split <file>` and `:vsplit <file>`.
- **`engine.rs` only** — no changes to `render.rs`, `main.rs`, `tui_main.rs`, or any other file.
- Tests: 673 → 673 (no new tests; behavior verified manually)

---

## Recently Completed (Session 79)

### ✅ Leader Key + Extended Syntax Highlighting + Full LSP Feature Set

- **`settings.rs`** — `leader: char` field (default `' '`); `default_leader()` fn; `Default` impl updated
- **`syntax.rs`** — 10 new `SyntaxLanguage` variants (C, TypeScript, TypeScriptReact, Css, Json, Bash, Ruby, CSharp, Java, Toml); full `from_path()`, `language()`, `query_source()` for each; 19 new tests; HTML skipped (tree-sitter-html 0.20.4 depends on tree-sitter 0.22, incompatible with our 0.20.x stack)
- **`Cargo.toml`** — 9 new tree-sitter grammar crates at 0.20 (`tree-sitter-c`, `-typescript`, `-css`, `-json`, `-bash`, `-ruby`, `-c-sharp`, `-java`, `-toml`)
- **`lsp_manager.rs`** — Python fallback chain (pyright → basedpyright → pylsp → jedi); `server_and_uri()` helper; 6 new request methods (references, implementation, type_definition, signature_help, formatting, rename)
- **`lsp.rs`** — 6 new `LspEvent` variants; `FormattingEdit`, `FileEdit`, `WorkspaceEdit`, `SignatureHelpData` types; 7 new `LspServer` request methods; `parse_locations_response`, `try_parse_signature_help_response`, `parse_text_edits`, `try_parse_workspace_edit` parsers; reader_thread routing for all new methods
- **`render.rs`** — `SignatureHelp` struct; `signature_help: Option<SignatureHelp>` on `ScreenLayout`; populated from `engine.lsp_signature_help` in `build_screen_layout`
- **`engine.rs`** — `leader_partial: Option<String>` field; `handle_leader_key()`; leader detection (only when `pending_key.is_none()`); `gr`/`gi`/`gy` bindings in `handle_pending_key`; 8 new LSP pending fields; `lsp_request_references/implementation/type_definition/signature_help`; `lsp_format_current()`; `lsp_request_rename()`; `apply_lsp_edits()`; `apply_workspace_edit()`; `:Lformat`/`:Rename` commands; signature help trigger after `(` or `,` in insert mode; 6 new `poll_lsp` arms
- **`main.rs` (GTK)** — `draw_signature_popup()` — positioned above cursor, active parameter in `theme.keyword` color via Pango AttrList
- **`tui_main.rs`** — `render_signature_popup()` — same layout; active parameter in `theme.keyword` color cell-by-cell
- Tests: 654 → 673 (+19 new syntax tests)

---

## Recently Completed (Session 77)

### ✅ Terminal Split Drag-to-Resize

- **`engine.rs`** — `terminal_split_left_cols: u16` field (0 = use PTY cols); `terminal_split_set_drag_cols(left_cols)` updates visual column during drag without resizing PTY; `terminal_split_finalize_drag(left_cols, right_cols, rows)` commits the new sizes and resizes both PTY panes; `terminal_close_split` resets `terminal_split_left_cols = 0`
- **`render.rs`** — split branch in `build_terminal_panel` now sets `split_left_cols = engine.terminal_split_left_cols` when > 0 (drag in progress), else `left_pane.cols` (PTY authoritative)
- **`main.rs` (GTK)** — `terminal_split_dragging: bool` on App struct; content-area click within 4px of divider (`left_cols * char_width`) starts drag; `Msg::MouseDrag` calls `terminal_split_set_drag_cols(col)` clamped ≥5/≤total-5; `Msg::MouseUp` finalizes via `terminal_split_finalize_drag`; split rendering now uses `panel.split_left_cols as f64 * char_width` instead of hardcoded `(w-SB_W)/2`
- **`tui_main.rs`** — `dragging_terminal_split: &mut bool` added to `handle_mouse` params and both call sites; content click on divider column starts drag; `Drag` event calls `terminal_split_set_drag_cols`; `Up` event finalizes; split rendering uses `panel.split_left_cols` instead of `area.width/2`
- Tests: 638 → 638

---

## Recently Completed (Session 76)

### ✅ Terminal Horizontal Split View

- **`engine.rs`** — `terminal_split: bool` field (default false); `terminal_open_split(half_cols, rows)` creates 2nd pane if needed, resizes both to half-width, sets `terminal_active = 1`; `terminal_close_split(full_cols, rows)` clears flag + resizes active pane; `terminal_toggle_split(full_cols, rows)` delegates; `terminal_split_switch_focus()` toggles `terminal_active` between 0 and 1; `terminal_close_active_tab` resets `terminal_split = false` before removing; `poll_terminal` resets split when panes drop below 2
- **`render.rs`** — `build_pane_rows(term, cursor_active, find)` helper (with `#[allow(clippy::type_complexity)]` on the `find` tuple); `TerminalPanel` gains `split_left_rows: Option<Vec<Vec<TerminalCell>>>`, `split_left_cols: u16`, `split_focus: u8`; `build_terminal_panel` has early-return split branch that builds both pane[0] and pane[1] grids
- **`main.rs` (GTK)** — `NF_SPLIT = "󰤼"` constant; `draw_terminal_cells()` helper extracted; toolbar now shows `"+ ⊞ ×"`; split rendering: fill + left cells + 1px divider + right cells; header click: `split_x = width - 4×char_width` triggers `TerminalToggleSplit`; content click in split mode sets `terminal_active = 0/1` based on `x < width/2`; Ctrl-W intercepted before PTY when `terminal_split`; `Msg::TerminalToggleSplit` + `Msg::TerminalSplitFocus(usize)` handlers
- **`tui_main.rs`** — `NF_TERMINAL_SPLIT = "󰤼"`; `render_terminal_pane_cells()` helper; toolbar shows `"+ ⊞ ×"`; split rendering with `│` divider; header click detects split button at `term_width - 4`; content click sets focus pane; Ctrl-W intercepted when `terminal_split`
- Tests: 638 → 638 (PTY features not unit-testable in isolation)

---

## Recently Completed (Session 75)

### ✅ Terminal History, Resize, and CWD

- **`terminal.rs`** — `HistCell { ch, fg: vt100::Color, bg: vt100::Color, bold, italic, underline }` + `history: VecDeque<Vec<HistCell>>` (5000-row ring buffer) added to `TerminalPane`; `lines_written` removed; `new()` gains `cwd: &Path` param + `cmd.cwd(cwd)`; `master: Box<dyn MasterPty + Send>` stored for resize; `poll()` calls `process_with_capture()` which splits data into ≤rows-newline chunks and calls `capture_scrolled_rows()` between chunks; `capture_scrolled_rows()` safely reads scrolled-off rows via `set_scrollback(N ≤ rows)` → copies into `history` → restores `set_scrollback(0)`; `resize()` now calls `self.master.resize(PtySize{…})` for real SIGWINCH; `set_scroll_offset()` caps at `history.len()`
- **`engine.rs`** — `terminal_new_tab()` passes `self.cwd.clone()` to `TerminalPane::new()`; `terminal_find_update_matches()` rewritten to search `history` VecDeque directly (no `set_scrollback` abuse): history matches get `required_offset = hist_len - hist_idx`, live matches get `required_offset = 0`
- **`render.rs`** — `build_terminal_panel()` rewritten: at `scroll_offset = N`, rows 0..N come from `history[hist_len-N+i]` (HistCell → TerminalCell via `map_vt100_color`), rows N..rows come from `screen.cell(i-N, c)` at scrollback=0; `scrollback_rows = hist_len` (accurate deep history count)
- **`main.rs` (GTK)** — `Msg::Resize` now calls `terminal_resize(cols, rows)` on open panes; `terminal_sb_dragging` code updated from `lines_written` to `history.len()`
- **`tui_main.rs`** — `Event::Resize` uses `engine.session.terminal_panel_rows` instead of hardcoded `12`; scrollbar drag `total` updated from `lines_written` formula to `history.len()`
- Tests: 638 → 638 (no change — PTY features not unit-testable in isolation)

---

## Recently Completed (Session 73)

### ✅ Terminal Find (Ctrl+F in terminal panel)

- **`engine.rs`** — 4 new fields: `terminal_find_active`, `terminal_find_query`, `terminal_find_selected`, `terminal_find_matches: Vec<(u16, u16)>`; 7 new methods: `terminal_find_open()`, `terminal_find_close()`, `terminal_find_char(ch)`, `terminal_find_backspace()`, `terminal_find_next()`, `terminal_find_prev()`, `terminal_find_update_matches()` (private); `poll_terminal()` re-runs `terminal_find_update_matches()` when find is active so matches stay fresh as new output arrives
- **`render.rs`** — `TerminalCell` gains `is_find_match: bool` + `is_find_active: bool`; `TerminalPanel` gains `find_active`, `find_query`, `find_match_count`, `find_selected_idx`; `build_terminal_panel()` applies find highlighting after building cell grid (case-insensitive char-by-char scan of each screen row)
- **`main.rs` (GTK)** — 6 new `Msg` variants (`TerminalFindOpen/Close/Char/Backspace/Next/Prev`); Ctrl+F check reordered so terminal gets priority over editor find dialog when `terminal_has_focus`; find-active key routing in `terminal_has_focus` block; `draw_terminal_panel()` renders inline find bar (query + match count) in toolbar when `find_active`; active-match cells orange bg/black fg; other-match cells dark-amber bg
- **`tui_main.rs`** — find key routing in `terminal_has_focus` block; `render_terminal_panel()` renders find bar when `find_active`; cell highlights match GTK

---

## Recently Completed (Session 72)

### ✅ Terminal Multiple Tabs

- **`engine.rs`** — `terminal: Option<TerminalPane>` → `terminal_panes: Vec<TerminalPane>` + `terminal_active: usize`; helpers `active_terminal()` / `active_terminal_mut()`; new methods `terminal_new_tab()`, `terminal_close_active_tab()`, `terminal_switch_tab()`; `open_terminal()` creates first tab if empty; `poll_terminal()` polls all panes and auto-removes exited ones (panel closes when last pane exits); `terminal_resize()` resizes all panes
- **`render.rs`** — `TerminalPanel` gains `tab_count`, `active_tab`; `build_terminal_panel()` uses `active_terminal()` and populates new fields; `exited`/`tabs_exited` removed (panes never linger in exited state)
- **`main.rs` (GTK)** — new `Msg` variants: `NewTerminalTab`, `TerminalSwitchTab(usize)`, `TerminalCloseActiveTab`; `EngineAction::OpenTerminal` → `NewTerminalTab`; Ctrl-T creates first tab when panel empty; Alt-1–9 in terminal-focus block; header-row click: tab zone → switch, close icon → close, else → resize drag; tab strip `[N] ` in toolbar (active tab inverted); all `terminal.as_mut()/as_ref()` → `active_terminal_mut()/active_terminal()`
- **`tui_main.rs`** — same `EngineAction::OpenTerminal` → `terminal_new_tab()`; Ctrl-T creates first tab if empty; Alt-1–9 in terminal-focus block; header-row click: tab zone → switch, close icon → close; tab strip `[N] ` with `TERMINAL_TAB_COLS = 4`; all `engine.terminal` → `active_terminal()`/`active_terminal_mut()`

---

## Recently Completed (Session 71)

### ✅ Terminal Panel Draggable Resize

- **`session.terminal_panel_rows: u16`** — new field (serde default 12) in `SessionState`; saved on drag end in both backends
- **GTK drag** — `terminal_resize_dragging: bool` on `App`; header-row click starts drag; `Msg::MouseDrag` recalculates rows from y-position clamped [5, 30]; `Msg::MouseUp` calls `terminal_resize(cols, rows)` + `session.save()`; all hardcoded `13.0 *` / `12` terminal row constants replaced with session-dynamic values
- **TUI drag** — `dragging_terminal_resize: bool` local var; `handle_mouse()` gains new parameter + both call sites updated; Drag handler computes `available = term_height - row - 2 - qf_h` then new rows; Up handler resizes PTY and saves session; all `strip_rows` calculations replaced with dynamic values
- **No core changes** — `render.rs` and `engine.rs` unchanged; `open_terminal(cols, rows)` already parameterized
- Tests: 638 → 638 (no change — pure UI drag handling)

---

## Recently Completed (Session 70)

### ✅ Terminal Scrollback, Copy/Paste, and Scrollbar Polish

- **Scrollback works** — `parser.set_scrollback(offset)` wired up in `set_scroll_offset()`; `scroll_up/down/reset` all sync the vt100 parser so `screen.cell()` returns history rows; `poll()` re-clamps after vt100 auto-increments; capped to one screenful (`rows`) per vt100 API constraint (`visible_rows()` panics if offset > rows_len)
- **TUI scrollbar drag fixed** — `total` in drag state capped to `rows as usize`; ratio formula uses `row.saturating_sub(track_start).min(track_len)` so dragging to the bottom reaches `ratio = 1.0` → `offset = 0` (live view)
- **TUI scrollbar color** — now matches editor scrollbar: thumb `Rgb(128,128,128)` / track `theme.separator` / bg `theme.background` (was using light status-bar fg for both)
- **Copy (Ctrl+Y)** — copies terminal mouse-selection to system clipboard in both GTK and TUI; mouse-release auto-copies in TUI (via `clipboard_write` callback)
- **Paste (Ctrl+Shift+V / bracketed paste)** — GTK: new `Msg::TerminalPasteClipboard` reads clipboard and writes to PTY; Ctrl+Shift+V intercepted before PTY routing; TUI: `Event::Paste(text)` (Alacritty/kitty bracketed paste) now routed to PTY when terminal has focus
- **TUI terminal scrollbar drag** — `dragging_terminal_sb: Option<(track_start, track_len, total)>`; click on rightmost column starts drag; drag computes `scroll_offset = (1-ratio) * total`; MouseUp clears state
- **GTK terminal scrollbar drag** — `terminal_sb_dragging: bool` on App; click in 6px right strip starts drag; MouseUp clears state
- **GTK terminal full-width** — `ToggleTerminal` uses actual DA width / `cached_char_width` (not hardcoded `200.0`)
- **GTK editor scrollbar overlapping terminal** — `sync_scrollbar()` subtracts `qf_px + term_px` from `content_bounds`
- **`:term` spawns fresh shell after Ctrl-D** — `open_terminal()` drops dead (exited) pane before `is_none()` guard
- Tests: 638 → 638 (no change — PTY requires subprocess)

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
- [x] **Terminal: multiple tabs** — tab strip in toolbar; `Vec<TerminalPane>`; Alt-1–9 / click `[N]` to switch; auto-close on shell exit (session 72)
- [x] **Terminal: draggable panel height** — drag header row to resize; `session.terminal_panel_rows` persisted; clamped [5, 30] (session 71)
- [x] **Terminal: scrollback navigation** — `scroll_offset` + vt100 `set_scrollback()`; PgUp/PgDn while focused; scrollbar thumb tracks position (session 70)
- [x] **Terminal: find in panel** — Ctrl+F while terminal focused opens an inline find bar in the toolbar row; live match highlighting (orange active, amber others); Enter/Shift+Enter navigate matches; Escape closes
- [x] **Terminal: button bar (Add / Close)** — `+` creates a new tab; `×`/`󰅖` closes the active tab; click detection in both GTK and TUI backends
- [x] **Terminal: horizontal split view** — `⊞`/`󰤼` toolbar button toggles two panes side-by-side; independent PTY sessions; mouse click or Ctrl-W switches active pane; `│` divider
- [x] **Debugger (DAP)** — Transport + adapter registry + `:DapInstall` (S83); poll loop + breakpoint gutter + stopped-line highlight (S84); variables/call-stack/output panel (S85-86); VSCode-like UI with launch.json (S88); codelldb compat (S89); interactive sidebar + conditional breakpoints (S90)

### UI & Menus
- [ ] **VSCode-style menus** — application menu bar (File / Edit / View / Go / Run / Terminal / Help) in GTK; command palette (`Ctrl-Shift-P`) lists all commands + key bindings; fuzzy-searchable; both GTK native menus and TUI pop-up menu overlay
- [ ] **Command palette** — `Ctrl-Shift-P` floating modal (like Telescope but for commands); lists named commands with descriptions and current keybindings; typing filters; Enter executes; shared between GTK and TUI

### Extension System
- [ ] **Extension mechanism** — WASM or Lua plugin sandbox (TBD); plugins can: register commands (`:MyCmd`), add key bindings, hook into editor events (on-save, on-open, on-key), read/write buffer text, show messages; `~/.config/vimcode/extensions/` directory auto-loaded; `:ExtInstall <url>`, `:ExtList`, `:ExtDisable`

### AI Integration
- [ ] **AI assistant panel** — VSCode Copilot-style sidebar chat panel; configurable provider (Anthropic Claude API, OpenAI, Ollama local); `api_key` in settings; `Alt-A` opens panel; multi-turn conversation with editor context (current file, selection, diagnostics); "Insert at cursor" / "Replace selection" actions on responses
- [ ] **AI inline completions** — ghost-text completions from AI provider interleaved with LSP ghost text; separate `ai_completions` setting (default false to avoid unexpected API costs); debounced after 500ms idle in insert mode; Tab accepts whole suggestion, `Alt-]`/`Alt-[` cycle through alternatives
