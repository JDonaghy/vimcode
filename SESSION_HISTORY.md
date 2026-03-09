# VimCode Session History

Detailed per-session implementation notes archived from PROJECT_STATE.md.
All sessions through 154 archived here. Recent work summary in PROJECT_STATE.md.

---

**Session 154 — Keymaps Editor in Settings Panel + toggle_comment_range undo fix (2822 tests):**
"User Keymaps" row in the Settings sidebar panel (new `BufferEditor` setting type) — pressing Enter (or `:Keymaps` command) opens a scratch buffer pre-filled with current keymaps (one per line, `mode keys :command` format). `:w` validates each line, rejects invalid entries with line-specific errors, updates `settings.keymaps`, calls `rebuild_user_keymaps()`, and saves. Tab title shows `[Keymaps]`. Buffer reuse on re-open. GTK "Edit…" button + count label; TUI "N defined ▸" display. 11 integration tests in `tests/keymaps_editor.rs`. **Bug fix:** `toggle_comment_range()` (used by visual `gc`) was mutating the buffer directly (`buffer_mut().delete_range()`/`insert()`) without recording undo operations — replaced with `delete_with_undo()`/`insert_with_undo()`. 2 new undo tests in `tests/extensions.rs`.

**Session 153 — Richer Lua Plugin API + VimCode Commentary + User Keymaps (2809 tests):**
Plugin API expansion: Extended `PluginCallContext` with new input/output fields. New Lua APIs: `vimcode.buf.set_cursor(line,col)`, `vimcode.buf.insert_line(n,text)`, `vimcode.buf.delete_line(n)`, `vimcode.opt.get(key)`/`vimcode.opt.set(key,value)`, `vimcode.state.mode()`/`register(char)`/`set_register(char,content,linewise)`/`mark(char)`/`filetype()`. New autocmd events: `BufWrite`, `BufNew`, `BufEnter`, `InsertEnter`, `InsertLeave`, `ModeChanged`, `VimEnter`. Centralized `set_mode()` method fires mode-change events. Visual/command mode keymap fallbacks. Plugin `set_lines` now records undo operations. VimCode Commentary: bundled extension (`extensions/commentary/`) inspired by tpope's vim-commentary — `gcc` toggles comment (count-aware), `gc` in visual mode toggles selection, `:Commentary [N]` command, 40+ language comment strings, engine-level `toggle_comment_range()` with undo group. User-configurable keymaps: `keymaps: Vec<String>` in settings.json, `UserKeymap` struct, multi-key sequence support with replay, `{count}` substitution, `:map`/`:unmap` commands. 22 + 17 + 13 = 52 new tests.

**Session 152 — Visual paste + TUI bug fixes (2768 tests):**
Visual paste: `p`/`P` in Visual/VisualLine/VisualBlock mode replaces selection with register content via `paste_visual_selection()` in engine.rs; `"x` register selection in visual mode via `pending_key`; `p`/`P` in `handle_visual_key()` guarded by `pending_key.is_none()`. `Ctrl+Shift+V` system clipboard paste extended to Normal/Visual modes (TUI+GTK). TUI tab bar fix: multi-group tab bar y-coordinate uses `bounds.y - tab_bar_height` instead of `bounds.y - 1` to account for breadcrumbs offset. Multi-group `Ctrl-W h/l` navigation: `focus_window_direction()` now navigates between adjacent editor groups before setting `window_nav_overflow` to reach sidebar. Pre-existing test fix: `test_restore_session_files` — `swap_scan_stale()` opened stale swaps as extra tabs, fixed with `settings.swap_file = false`. 8 integration tests in `tests/visual_mode.rs`.

**Session 151 — Tab drag-to-split + tab bar draw fix + new logo (2760 tests):**
VSCode-style tab drag-and-drop: drag a tab to the edge of a group to create a new editor group split; drag to center to move tab between groups; drag within tab bar to reorder. New core types: `DropZone` enum (Center/Split/TabReorder/None) in `window.rs`, `TabDragState` struct in `engine.rs`. 7 new engine methods: `tab_drag_begin`, `tab_drag_cancel`, `tab_drag_drop`, `move_tab_to_target_group`, `move_tab_to_new_split`, `reorder_tab_in_group`, `close_group_by_id`. GTK: 8px dead-zone drag detection from tab clicks, `compute_tab_drop_zone()` with 20% edge margins for split zones, `draw_tab_drag_overlay()` with blue highlight + ghost label. Tab bar draw order fix: moved tab bar + breadcrumb drawing AFTER window drawing so tab bars are never overwritten by window backgrounds in multi-group layouts; dividers draw before tab bars so vertical dividers don't bleed through tab bar backgrounds. New logo: `vim-code.svg` gradient VC logo replaces old icon files; removed `vimcode-color.png`, `vimcode-color.svg`, `vimcode.png`, `vimcode.svg`, `asset-pack.jpg`; updated Flatpak icon. 15 integration tests in `tests/tab_drag.rs`.

**Session 150 — Tab switcher polish + tab click fix (2728 tests):**
Alt+t as universal tab switcher binding (works in both TUI and GTK where Ctrl+Tab is often intercepted). GTK modifier-release detection via 100ms polling of `keyboard.modifier_state()` — releasing Ctrl/Alt auto-confirms selection. TUI uses 500ms timeout after last cycle. Sans-serif UI font (`UI_FONT`) applied to tab bar and tab switcher popup in GTK (matching VSCode style). **Tab click fix**: clicking tabs in GUI mode now works correctly — fixed three bugs: (1) breadcrumbs offset caused click y-region to hit breadcrumb row instead of tab row (`grect.y - line_height` → `grect.y - tab_bar_height`); (2) monospace `char_width` tab measurement replaced with Pango-measured slot positions cached during draw; (3) `editor_bottom` calculation now matches draw layout (accounts for quickfix/terminal/debug toolbar). Tab bar clicks skip expensive `fire_cursor_move_hook()` (git blame subprocess) and defer `highlight_file_in_tree` DFS via 50ms timeout for instant visual response.

**Session 149 — Ctrl+Tab MRU tab switcher + autohide panels (2728 tests):**
VSCode-style MRU tab switcher: Ctrl+Tab opens a popup showing recently accessed tabs in most-recently-used order; Ctrl+Tab cycles forward, Ctrl+Shift+Tab cycles backward, Enter or any non-modifier key confirms selection, Escape cancels. New `autohide_panels` boolean setting (default false, TUI only): when enabled, hides sidebar and activity bar at startup; Ctrl-W h reveals them, and they auto-hide when focus returns to the editor. 11 integration tests in `tests/tab_switcher.rs`.

**Session 148 — Netrw in-buffer file browser (2693 tests):**
Vim-style netrw directory browser. `:Explore [dir]` / `:Ex` opens directory listing in buffer; `:Sexplore` / `:Sex` horizontal split; `:Vexplore` / `:Vex` vertical split. Header line shows current directory. Enter on directory navigates; Enter on file opens. `-` key navigates to parent. Respects `show_hidden_files` setting. `netrw_dir` field on `BufferState`. 16 integration tests in `tests/netrw.rs`.

**Session 147 — TUI interactive settings panel (2677 tests):**
Replaced read-only TUI settings panel with full interactive form. Moved `SettingType`/`SettingDef`/`SETTING_DEFS` from `render.rs` to `settings.rs`. New `DynamicEnum` variant for runtime-computed options. Engine fields: `settings_has_focus`, `settings_selected`, `settings_scroll_top`, `settings_query`, `settings_input_active`, `settings_editing`, `settings_edit_buf`, `settings_collapsed`. `handle_settings_key()`: search filter, inline string/int edit, j/k nav, Space/Enter toggle, Enter/l/h enum cycle. `settings_paste()` for Ctrl+V. TUI renders: header, `/` search bar, scrollable categorized form, inline editing, scrollbar. 10 integration tests in `tests/settings_panel.rs`.

**Session 146 — Breadcrumbs bar (14 new tests, 2667 total):**
VSCode-like breadcrumbs bar showing file path segments + tree-sitter symbol hierarchy (e.g. `src › core › engine.rs › Engine › handle_key`) below the tab bar. `BreadcrumbSymbol` struct + `Syntax::enclosing_scopes()` walks parent chain for 10 languages (Rust/Python/JS/TS/Go/C/C++/Java/C#/Ruby). `BreadcrumbSegment`/`BreadcrumbBar` render structs. `breadcrumb_bg/fg/active_fg` theme colors in all 4 built-in themes + VSCode theme loader. `Settings.breadcrumbs: bool` (default true, `:set breadcrumbs`/`:set nobreadcrumbs`). Each editor group gets its own breadcrumb bar. Space reserved via doubled `tab_bar_height` when enabled. GTK `draw_breadcrumb_bar()` + TUI `render_breadcrumb_bar()`. 14 new tests (11 integration + 3 unit).

**Session 145 — VSCode theme loader, TUI crash fix, sidebar navigation (8 new tests, 2650 total):**
VSCode theme support: drop `.json` theme files into `~/.config/vimcode/themes/`, apply with `:colorscheme <name>`. `Theme::from_vscode_json(path)` parses VSCode `colors` (~25 UI keys) + `tokenColors` (~15 TextMate scopes), maps to our 55-field Theme struct. `Color::try_from_hex()` (non-panicking, supports #rrggbb/#rrggbbaa/#rgb), `Color::lighten()`/`darken()` for deriving missing colors, `strip_json_comments()` for JSONC. `Theme::available_names()` now returns built-in + custom themes from disk. `:colorscheme` command updated to accept/list custom themes. 4 unit tests for theme loader. Crash fix: `byte_to_char_idx` in TUI panicked on multi-byte UTF-8 chars; now uses `floor_char_boundary()`. Swap recovery fix: R/D/A keys in TUI. TUI sidebar navigation: `Ctrl-W h/l` toolbar↔sidebar↔editor.

**Session 144 — Vim compatibility batch 4: 10 commands (21 new tests, 2642 total):**
Implemented 10 more missing Vim commands, raising VIM_COMPATIBILITY.md from 400/414 (97%) to 406/414 (98%). `Ctrl-G` show file info (filename, line, col, percentage), `gi` insert at last insert position (LSP go-to-implementation remapped to `<leader>gi`, `last_insert_pos` field tracked on Insert→Normal transition), `Ctrl-W r`/`R` rotate windows (forward/backward buffer+view rotation within tab), `[*`/`]*` and `[/`/`]/` C-style comment block navigation (`/*`/`*/` search), `do`/`dp` diff obtain/put (pull/push lines between diff windows), `o_CTRL-V` force blockwise operator motion (intercepts Ctrl-V with pending operator). Also fixed doc inconsistencies: `g'`/`` g` `` mark without jumplist was already implemented, `[z`/`]z` fold navigation was already implemented. Marked `CTRL-X ...` and `:map` as N/A. 21 integration tests in `tests/vim_compat_batch4.rs`. Sections now at 100%: Search & Marks (26/26), Window (33/33), Operator-Pending (21/21), Ex Commands (67/67).

**Session 143 — File management bug fixes + :e! (9 new tests, 2621 total):**
Fixed 3 bugs found during Neovim comparison testing + added `:e!` command: (1) `:q` dirty guard now checks if the buffer is visible in another window before blocking — `execute_command("quit")` queries `self.windows` for other views of the same `buffer_id`, (2) File auto-reload system — `BufferState.file_mtime: Option<SystemTime>` captured in `with_file()` and `save()`, `BufferState.file_change_warned: bool` for one-shot warnings, `BufferState.reload_from_disk()` method (re-reads file, clears undo/redo, resets dirty), `Settings.autoread: bool` (default true, alias `ar`), `Engine.check_file_changes()` iterates all buffers and stats files (silently reloads clean, shows W12 warning for dirty), `BufferManager.iter()` public iterator, wired into both GTK (`main.rs`: `last_file_check` field, 2s interval) and TUI (`tui_main.rs`: `last_file_check` local, 2s interval), (3) `split_window()` now uses `settings.splitbelow`/`settings.splitright` to compute `new_first` instead of hardcoding `false`, (4) `:e!` (`edit!`) command reloads current file from disk discarding all changes. New `SettingDef` for `autoread` in `render.rs`. 9 integration tests in `tests/vim_compat_batch3.rs`: `:q` dirty split allows close, `:q` dirty last window blocks, `check_file_changes` reload/warn, `:new`/`:vnew` with default/custom `splitbelow`/`splitright`, `:e!` reload.

**Session 142 — Vim compatibility batch 3: 15 new commands (29 new tests, 2612 total):**
Implemented 15 more missing Vim commands, raising VIM_COMPATIBILITY.md from 380/403 (94%) to 400/414 (97%). `g?{motion}` ROT13 encode (with text objects, all motions via `apply_rot13_range()`), `CTRL-@` insert previous text and exit insert, `CTRL-V {char}` insert next character literally (handles Tab/Return too), `CTRL-O` auto-return to Insert after one Normal command, `!{motion}{filter}` filter lines through external command (opens command mode with range pre-filled, `try_execute_filter_command()` pipes through shell), `CTRL-W H/J/K/L` move window to far edge (`move_window_to_edge()`), `CTRL-W T` move window to new group (`move_window_to_new_group()`), `CTRL-W x` exchange windows (`exchange_windows()`), visual block `I`/`A` (insert/append text applied to all block lines on Escape via `visual_block_insert_info`), `o_v`/`o_V` force charwise/linewise motion mode (`force_motion_mode` field, checked in `apply_charwise_operator`/`apply_linewise_operator`). Enhanced `apply_operator_text_object()` with case/ROT13/indent/filter support. Added `insert_ctrl_o_active`, `insert_ctrl_v_pending`, `visual_block_insert_info`, `force_motion_mode` Engine fields. 29 integration tests in `tests/vim_compat_batch3.rs`. Sections now at 100%: Window commands (31/31), Visual mode (26/26), Editing (51/51).

**Session 141 — Vim compatibility batch 2: 27 new commands (38 new tests, 2583 total):**
Implemented 27 more missing Vim commands, raising VIM_COMPATIBILITY.md from 348/403 (85%) to 380/403 (94%). **Tier 1 (quick wins):** `ga` ASCII value, `g8` UTF-8 bytes, `go` byte offset, `gm`/`gM` middle of screen/text, `gI` insert at column 1, `gx` open URL, `g'`/`` g` `` mark without jumplist, `g&` repeat `:s` globally, `CTRL-^` alternate buffer, `CTRL-L` redraw/clear message, `N%` go to N% of file, `zs`/`ze` scroll cursor to left/right edge, `:b {name}` buffer by partial name, `:make`. **Tier 2 (medium effort):** `gq{motion}`/`gw{motion}` format operators (reflow to textwidth, with text object support), `CTRL-W p`/`t`/`b` previous/top/bottom editor group, `CTRL-W f` split+open file, `CTRL-W d` split+go to definition, insert `CTRL-A` repeat last insertion, insert `CTRL-G u`/`j`/`k` break undo/move in insert, visual `gq` format selection, visual `g CTRL-A`/`g CTRL-X` sequential increment/decrement. Added `prev_active_group`/`insert_ctrl_g_pending` Engine fields, `format_lines()` method, gq/gw handling in `apply_operator_text_object`, 38 integration tests in `tests/vim_compat_batch2.rs`. Sections now at 100%: Movement (48/48), Editing (50/50), z-commands (23/23).

**Session 140 — Vim compatibility batch: 29 new commands (45 new tests, 2545 total):**
Implemented 29 missing Vim commands in two tiers, raising VIM_COMPATIBILITY.md from 319/411 (78%) to 348/411 (85%). **Tier 1:** `+`/`-`/`_` line motions, `|` column motion, `gp`/`gP` paste with cursor after, `@:` repeat last ex command, backtick text objects (`` i` ``/`` a` ``), insert `CTRL-E`/`CTRL-Y` (char below/above), visual `r{char}`, `&` repeat last `:s`, `CTRL-W q`/`n`. **Tier 2:** `CTRL-W +`/`-`/`<`/`>`/`=`/`_`/`|` resize/equalize/maximize, `[{`/`]}`/`[(`/`])` unmatched bracket jumps, `[m`/`]m`/`[M`/`]M` method navigation, `[[`/`]]`/`[]`/`][` section navigation. Added `last_ex_command`/`last_substitute` fields to Engine, `set_all_ratios()` to GroupLayout, 45 integration tests in `tests/vim_compat_batch.rs`. Text Objects now 100%.

**Session 139 — Comprehensive z-commands (33 new tests, 2494 total):**
Implemented 15 missing z-commands to bring z-command coverage from 7/22 (32%) to 22/23 (96%). New fold commands: `zM` (close all), `zA`/`zO`/`zC` (recursive toggle/open/close), `zd`/`zD` (delete fold/recursive), `zf{motion}` (fold-create operator with j/k/G/gg/{/} motions), `zF` (fold N lines), `zv` (open to show cursor), `zx` (recompute). Scroll+first-non-blank: `z<CR>`/`z.`/`z-`. Horizontal scroll: `zh`/`zl` (with count), `zH`/`zL` (half-screen). Added 3 View helper methods (`delete_fold_at`, `delete_folds_in_range`, `open_folds_in_range`), 33 integration tests in `tests/z_commands.rs`.

**Session 138 — Vim compatibility inventory (documentation only, 2461 tests):**
Created `VIM_COMPATIBILITY.md` — systematic Vim command inventory with 12 categories, 411 commands tracked, 304 implemented (74%). Added VimScript scope note + link in README.md Vision section. Memory files updated for cross-session awareness.

**Session 137 — Operator+motion completeness (56 new tests, 2461 total):**
Full operator+motion support: `pending_find_operator` for `df`/`dt`/`dF`/`dT`, generic `apply_charwise_operator()`/`apply_linewise_operator()` helpers, all motions in `handle_operator_motion()` (h/l/j/k/G/{/}/(/)/ W/B/E/^/H/M/L/;/,/f/t/F/T), operator-aware gg/ge/gE in pending_key, case/indent operators extended to all motions. 56 tests in `tests/operator_motions.rs`.

**Session 136 — Vim-style ex command abbreviations + ~20 new commands (71 new tests, 2405 total):**
`normalize_ex_command()` system (57-entry abbreviation table), ~20 new ex commands (`:join`, `:yank`, `:put`, `:>/<`, `:=`, `:#`, `:mark`/`:k`, `:pwd`, `:file`, `:enew`, `:update`, `:version`, `:print`, `:number`, `:new`, `:vnew`, `:retab`, `:cquit`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, `:display`), `:copy` conflict fix, `QuitWithError` action. 71 tests in `tests/ex_commands.rs`.

**Session 135 — show_hidden_files setting + LSP format undo fix (no new tests, 2346 total):**
`show_hidden_files` setting (explorer/fuzzy/folder picker), LSP format undo fix (`record_delete`/`record_insert` in `apply_lsp_edits`), stale highlighting after format fix (mark buffer in `lsp_dirty_buffers`).

**Session 134 — search highlight + viewport bug fixes (13 new tests, 2346 total):**
Five bug fixes: search highlights refresh after edits (`run_search()` after buffer changes), Escape clears highlights, extra gutter line number fix (`buffer.len_lines()` vs raw Ropey), markdown preview always wraps, TUI viewport layout fix (double-counted tab bar row), GTK per-window viewport sync in SearchPollTick handler. 13 tests in `tests/search_highlight.rs`.

**Session 133 — bracket matching: visual mode + y% fix + tests (30 new tests, 2333 total):**
`%` bracket matching: visual mode `v%`/`V%`, `y%` yank-only bug fix (was always deleting), 30 integration tests in `tests/bracket_matching.rs`.

**Session 132 — LSP session restore + semantic tokens bug fixes (1 new test, 2303 total):**
Three bug fixes: (1) Tree-format session restore (`restore_session_group_layout`) never called `lsp_did_open()`, so LSP servers weren't started for restored files — fixed by adding calls after tree layout install. (2) `lsp_pending_semantic_tokens` was `Option<i64>` (single slot); changed to `HashMap<i64, PathBuf>` for multi-file init. (3) `semantic_parameter` color in OneDark changed from #e06c75 (same as variable) to #c8ae9d. 1 new test.

**Session 131 — LSP semantic tokens + develop branch workflow (17 new tests, 2302 total):**
Full `textDocument/semanticTokens/full` implementation: `SemanticToken`/`SemanticTokensLegend` types, delta-decoder, `SemanticTokensResponse` event, legend caching in LspManager, `BufferState.semantic_tokens` storage, request triggers on didOpen/didChange/Initialized. 8 new theme colors (parameter/property/namespace/enumMember/interface/typeParameter/decorator/macro). `Theme::semantic_token_style()` with binary-search overlay in `build_spans()`. Branching: version-tagged releases in `release.yml`, deleted `rust.yml`, bumped to 0.2.0. 5 unit + 12 integration tests.

**Session 130 — LSP formatting enhancements (12 new tests, 2268 total):**
Format-on-save (`format_on_save` setting, off by default), LSP capability checking (`documentFormattingProvider`), Shift+Alt+F keybinding (GTK+TUI). `save_with_format()` defers save when format-on-save enabled; FormattingResponse applies edits then saves; `format_save_quit_ready` for deferred `:wq`/`:x`. LSP binary resolution fix (checks `~/.dotnet/tools`, `~/.cargo/bin`, etc.). On-demand server startup from LSP commands. GTK CSS/focus fixes. 12 integration tests.

**Session 129 — GUI polish + sidebar/scrollbar fixes (no new tests, 2256 total):**
Fixed sidebar layout (hexpand propagation), scrollbar ghosts from inactive tabs, visual mode click jitter (4px dead zone), redo dirty flag (`saved_undo_depth` tracking), status line overlap (Pango ellipsis + TUI clamping), search icon, menu dropdown hover highlight, menu actions close_menu centralization, logo embedding + taskbar icon, sidebar background CSS.

**Session 128 — GUI mode polish + data format extensions (no new tests, 2256 total):**
GTK menu hover switching, dialog menu-close fix, removed "Close Tab" from File menu. 4 new bundled extensions: JSON, XML, YAML, Markdown with LSP configs. Added `number` color to Theme (all 4 themes) + `scope_color()`. Expanded C# tree-sitter query with ~30 more keywords.

**Session 127 — Swap file crash recovery (13 new tests, 2256 total):**
Vim-like swap file system: `src/core/swap.rs` (~240 lines) with atomic I/O (FNV-1a hash, PID-based stale detection). Engine: swap created on file open, deleted on save/close, periodic writes via `tick_swap_files()`. Recovery dialog (`[R]ecover/[D]elete/[A]bort`). Settings: `:set swapfile`/`:set updatetime=N`. `swap_scan_stale()` for orphaned swaps. Both backends tick and clean up. 13 tests.

**Session 126 — Markdown preview polish (3 new tests, 1289 total):**
Undo/redo refreshes live preview; extension READMEs open in own tab; scroll sync via `scroll_bind_pairs`; GTK heading font scale (H1=1.4x, H2=1.2x, H3=1.1x via Pango); no line numbers in preview; `color_headings` param (GTK=false/TUI=true); tab close button hover + widened hit area; free mouse scroll.

**Session 125 — Markdown preview (26 new tests, 1286 total):**
`:MarkdownPreview`/`:MdPreview` for live side-by-side preview using `pulldown-cmark`. Read-only preview buffers with styled headings, bold, italic, code, links, lists. Live refresh on source edits. `src/core/markdown.rs` module. Bold/italic in GTK (Pango) and TUI (ratatui). 15 unit + 11 integration tests.

**Session 124 — Generic async plugin shell execution (3 new tests, 1260 total):**
`vimcode.async_shell(command, callback_event, options)` Lua API for non-blocking shell from plugins. Background threads via `std::process::Command`; results as plugin events on next poll. Last-writer-wins per callback_event. `blame.lua` rewritten to use `async_shell` — git blame no longer blocks UI. 3 new tests.

**Session 123 — Performance: cursor movement lag + extension loading fix (no new tests, 1257 total):**
Fixed sluggish arrow-key nav on large files: `plugin_init()` now only loads scripts from installed extensions (was loading all subdirs); `make_plugin_ctx(skip_buf_lines)` skips O(N) allocation for cursor_move; `has_event_hooks()` early-exit. `canonical_path` cached on `BufferState`. Incremental tree-sitter via `last_tree`. `:ExtDisable`/`:ExtEnable` now update `settings.disabled_plugins` + reload plugin manager.

**Session 122 — Extension install UX + sidebar navigation fixes (2 new tests, 1257 total):**
Sidebar navigation: after install, selected resets to installed item; after last delete, available section expands. `ext_install_from_registry()` rewritten with `binary_on_path()` PATH checks — idempotent, shows status. Install diagnostics to `/tmp/vimcode-install.log`. 2 regression tests.

**Session 121 — Manifest-driven LSP/DAP config (24 new tests, 1255 total):**
Extension manifests as single source of truth: `LspConfig` gains `fallback_binaries` + `args`; `DapConfig` gains `binary/install/transport/args`; `ExtensionManifest` gains `workspace_markers`. All 11 bundled manifests updated. `lsp_manager.rs`: manifest candidates tried before registry. `dap_manager.rs`: manifest-first adapter lookup + install. 24 new tests.

**Session 120 — AI ghost text improvements + settings persistence fix (1239 total):**
Multi-line ghost text shown as virtual continuation rows (both GTK + TUI); `is_ghost_continuation` on `RenderedLine`. Settings write-through bug fixed: `saves_suppressed()` runtime guard in `Settings::save()`. GTK settings panel rebuilt from engine.settings each open. AI debounce 500ms → 250ms.

**Session 119b — git-insights blame fixes + TUI mouse crash (1231 total):**
`cursor_move` suppressed in Insert mode; annotations hidden during Insert; `BlameInfo.not_committed`; `blame_line(buf_contents)` uses `--contents -` stdin pipe; `buf_lines.join("")`; TUI drag crash: `saturating_sub(gutter)`.

**Session 119 — AI inline completions / ghost text (19 new tests, 1231 total):**
Opt-in ghost text from AI in insert mode. `ai.rs`: `complete()` fill-in-the-middle. Engine: `ai_ghost_text/alternatives/alt_idx/completion_ticks/completion_rx` fields; Tab accepts, Alt+]/[ cycle alternatives; `ai_completions: bool` setting (default false). `ghost_suffix` on `RenderedLine`; `ghost_text_fg` on Theme. 19 tests.

**Session 118 — AI assistant panel (1212 total):**
Sidebar chat panel. `src/core/ai.rs`: `send_chat()` dispatcher; Anthropic/OpenAI/Ollama via curl. Engine: `ai_messages/ai_input/ai_has_focus/ai_streaming/ai_rx/ai_scroll_top`; `ai_send_message()`, `poll_ai()`, `ai_clear()`, `handle_ai_panel_key()`; `:AI <msg>`/`:AiClear`. Settings: `ai_provider/ai_api_key/ai_model/ai_base_url`. GTK: `SidebarPanel::Ai`, `draw_ai_sidebar()`. TUI: `TuiPanel::Ai`, `render_ai_sidebar()`. 16 integration tests.

**Session 117c — Settings panel bug fixes (no new tests, 1199 total):**
Fixed two visual issues in the GTK settings sidebar: (1) settings panel not collapsing when clicking the Settings activity bar button a second time — removed `#[watch]` from the settings panel's `set_visible` so Relm4 no longer overrides the imperative hide; (2) Toggle switch widgets clipped — removed CSS `min-height`/`min-width` constraints on `.sidebar switch` and added 4px margin on all four sides of each Switch widget so Adwaita's rendering has room; also fixed overlay scrollbar floating over settings widgets via `set_overlay_scrolling(false)`.

**Session 117b — GTK settings sidebar form (no new tests, 1199 total):**
VSCode-style settings sidebar with native GTK widgets. `render.rs`: `SettingType`/`SettingDef`/`SETTING_DEFS` (~30 settings, 7 categories: Appearance/Editor/Search/Workspace/LSP/Terminal/Plugins). `settings.rs`: `get_value_str(key) -> String` and `set_value_str(key, value) -> Result<()>` reflection methods. `main.rs`: `Msg::SettingChanged`; `build_setting_row()` (Switch/SpinButton/DropDown/Entry per type) and `build_settings_form()` (category headers + rows) free functions; imperative panel with search bar (category-aware show/hide), scrolled list, "Open settings.json" button; CSS for `.settings-category-header`, transparent scrolledwindow, dark spinbutton/dropdown/entry; `gtk4::Settings::default().set_gtk_application_prefer_dark_theme(true)` in `init()`.

**Session 117 — Settings editor / :Settings command (3 new tests, 1199 total):**
`:Settings` / `:settings` opens `~/.config/vimcode/settings.json` in a new editor tab. `settings_path()` renamed to `pub fn settings_file_path()`. Engine: `:Settings` command arm + palette entry "Preferences: Open Settings (JSON)". TUI: gear icon click opens the file; `render_settings_panel` shows live current values right-aligned; mtime-based auto-reload on every event-loop iteration. 3 new tests.

**Session 116 — Named colour themes / :colorscheme (10 new tests, 1196 total):**
Four built-in themes: OneDark (default), Gruvbox Dark, Tokyo Night, Solarized Dark. `render.rs`: `Theme::gruvbox_dark/tokyo_night/solarized_dark()` constructors; `Theme::from_name(name)` with alias normalisation; `Theme::available_names()`; `Color::to_hex()`. `settings.rs`: `colorscheme: String` field. Engine: `:colorscheme` lists / `:colorscheme <name>` sets+saves theme. GTK: `make_theme_css(theme)` + `STATIC_CSS` const + hot-reload in `SearchPollTick`. TUI: theme refreshed each event-loop iteration; `render_sidebar` fills full background. 10 new tests.

**Session 115 — DAP SIGTTIN fix + ANSI carry buffer (3 new tests, 1128 total):**
Fixed TUI suspension when DAP breakpoints hit: `setsid()` via `pre_exec` on all DAP and LSP child spawns (`dap.rs`, `lsp.rs`). Added `dap_ansi_carry: String` to buffer incomplete ANSI escape sequences split across DAP output events. Added `libc = "0.2"` dependency. 3 new tests.

**Session 114 — Extensions Sidebar Panel + GitHub Registry (16 new tests, 1125 total):**
VSCode-style Extensions sidebar + GitHub-hosted first-party registry replacing Mason. `src/core/registry.rs`: `fetch_registry()`, `download_script()`, URL constants. Engine: 9 new fields; `ext_available_manifests()` (registry overrides bundled), `ext_refresh()` (background thread), `poll_ext_registry()`, `handle_ext_sidebar_key()`, `ext_install_from_registry()`, `ext_remove()`; `:ExtRemove`/`:ExtRefresh`; Mason registry removed from `lsp_manager.rs`. GTK: `SidebarPanel::Extensions`, `draw_ext_sidebar()`. TUI: `TuiPanel::Extensions`, `render_ext_sidebar()`. 16 new tests.

**Session 113 — Extension/Language Pack System (31 new tests, 1109 total):**
Full VSCode-style extension system: 11 bundled language packs (csharp/python/rust/javascript/go/java/cpp/php/ruby/bash/git-insights) compiled in via `include_str!()`. `extensions.rs`: `BundledExtension`, `ExtensionManifest`, lookup helpers. `ExtensionState` persistence in `session.rs`. Engine: `:ExtInstall/:ExtList/:ExtEnable/:ExtDisable`; `line_annotations: HashMap<usize,String>` for virtual text; auto-detect hint on file open. `git.rs`: `blame_line()`, `epoch_to_relative()`, `log_file()`. `plugin.rs`: `vimcode.buf.cursor/annotate_line/clear_annotations`, `vimcode.git.blame_line/log_file`, `cursor_move` hook. `extensions/git-insights/blame.lua`: inline blame annotation. 31 new tests.

**Session 112 — :set wrap fix + release pipeline (4 new tests, 1078 total):**
Fixed `:set wrap` rendering accuracy (uses `rect.width / char_width` instead of stored approximate value). Fixed GTK resize callback to use measured `char_width`. TUI always redraws after keypress. Added `:set option!` toggle syntax. Release pipeline: `release.yml` publishes public GitHub Release with `.deb` + raw binary on `main` push; `[package.metadata.deb]` in `Cargo.toml`. 4 new tests.

**Session 111 — Missing Vim Commands Batches 1–3 (55 new tests, 1023 total):**
Implemented `^`, `g_`, `W`/`B`/`E`/`gE`, `H`/`M`/`L`, `(`/`)`, `Ctrl+E`/`Ctrl+Y`, `g*`/`g#`, `gJ`, `gf`, `R` (Replace mode), `Ctrl+A`/`Ctrl+X`, `=` operator, `]p`/`[p`, `iW`/`aW`, `Ctrl+R`/`Ctrl+U`/`Ctrl+O` in insert. Ex: `:noh`, `:wa`, `:wqa`, `:reg`, `:marks`, `:jumps`, `:changes`, `:history`, `:echo`, `:tabmove`, `:!cmd`, `:r file`. Settings: `hlsearch`, `ignorecase`, `smartcase`, `scrolloff`, `cursorline`, `colorcolumn`, `textwidth`, `splitbelow`, `splitright`. New `tests/new_vim_features.rs` (55 tests).

**Session 110c — Last-word-of-file yank bug fix (4 new tests, 990 total):**
Fixed off-by-one in `apply_operator_with_motion` when `w` motion lands at EOF with no trailing newline. `move_word_forward()` clamps to `total_chars - 1`; exclusive range `[start, end_pos)` then missed the final char. Fix: detect `end_pos + 1 == total_chars && char != '\n'` and extend `delete_end` to `total_chars`. 4 new tests.

**Session 110b — Yank highlight flash (986 total, no new tests):**
Neovim-style green flash on yanked region (~200ms). Engine: `yank_highlight: Option<(Cursor,Cursor,bool)>` field; set at all yank sites. Render: `Theme.yank_highlight_bg` (`#57d45e`) + `yank_highlight_alpha` (0.35). GTK: `Msg::ClearYankHighlight` + 200ms timeout. TUI: `yank_hl_deadline: Option<Instant>` + deadline check in event loop.

**Session 110 — Operator-Motion Coverage (31 new integration tests + 3 bug fixes, 982 total):**
Created `tests/operator_motions.rs` (31 tests). Fixed 3 bugs: `y` routed through `pending_operator` (not `pending_key`); `yw`/`dw` clamp at line boundary (no newline crossing); `y$`/`d$`/`c$`/`y0`/`d0` added to `handle_operator_motion`.

**Session 109 — Vim Feature Completeness (43 new tests, 955 total):**
Implemented 20+ missing features in `tests/vim_features.rs`: `X`, `g~`/`gu`/`gU`, `gn`/`gN`/`cgn`, `g;`/`g,` (change list); visual `o`/`O`/`gv`; registers `"0`/`"1-9`/`"-`/`"%`/`"/`/`".`; uppercase/special marks; insert Ctrl+W/T/D; `:g`/`:v`/`:d`/`:m`/`:t`/`:sort` global commands.

**Session 108 — Integration Test Suite (64 new tests, 912 total):**
Added `[lib]` crate target (`vimcode_core`) + `[[bin]]` in `Cargo.toml`; `src/lib.rs` re-exports. `tests/common/mod.rs` with hermetic `engine_with()` and `drain_macro_queue()`. 64 integration tests across `normal_mode.rs` (25), `search.rs` (16), `visual_mode.rs` (10), `command_mode.rs` (13).

**Session 107c — Linewise paste fix + Ctrl+Shift+L TUI fix (3 new tests, 848 total):**
`load_clipboard_for_paste()` preserves `is_linewise` on `'"'` register when clipboard matches, fixing `yyp` pasting inline. TUI: push `REPORT_ALL_KEYS_AS_ESCAPE_CODES | DISAMBIGUATE_ESCAPE_CODES` so Ctrl+Shift combos arrive correctly. 3 new tests.

**Session 107b — Multi-Cursor Enhancements (10 new tests, 845 total):**
`select_all_word_occurrences` (Ctrl+Shift+L); `add_cursor_at_pos` for Ctrl+Click; Normal-mode buffer changes clear stale extra_cursors; Escape clears extra_cursors. 10 new tests.

**Session 107 — Multiple Cursors (8 new tests, 835 total):**
`extra_cursors: Vec<Cursor>` on `View`; `add_cursor_at_next_match` (Alt-D); multi-cursor insert/backspace/delete/return helpers; secondary cursor rendering in both GTK and TUI backends. 8 new tests.

**Session 106 — Per-Workspace Session Isolation (2 new tests, 827 total):**
Removed global-session fallback in `restore_session_files()` — editor starts clean when no workspace session exists. `Settings::save()` is no-op under `#[cfg(test)]`. 2 new tests.

**Session 105b — Debug Logging + TUI Crash Fixes (1 new test, 854 total):**
`--debug <logfile>` flag + `debug_log!` macro + panic hook. Fixed TUI u16 subtract overflow in `render_separators`. Fixed right-group tab bar positioning (`bounds.y <= 1.0` instead of `idx == 0`). 1 new test.

**Session 105 — Recursive Editor Group Splits (16 new tests, 853 total):**
`GroupLayout` recursive binary tree in `window.rs` — no cap on group count. `engine.rs`: `HashMap<GroupId, EditorGroup>` + `GroupLayout` tree; Ctrl+1–9 focus by tree position. `render.rs`: `GroupTabBar`. `session.rs`: `SessionGroupLayout` serde enum (backward-compat). GTK/TUI: multi-divider drag/draw, per-group tab bars. 16 new tests.

**Session 104 — Three TUI/GTK Bug Fixes (827 total, no new tests):**
TUI: drag handler off-by-one fixed; tab close confirmation overlay (S=save/D=discard/Esc=cancel); command-line mouse selection (Ctrl-C copies). GTK: `Msg::ShowCloseTabConfirm` + `Msg::CloseTabConfirmed` dialog. Buffer leak fix in `close_tab()` forces deletion of unreferenced buffers. `engine.escape_to_normal()` pub method.

**Session 103 — Command Line Cursor Editing + History Separation (9 new tests, 836 total):**
`command_cursor: usize` + `cmd_char_to_byte()` + `command_insert_str()`; full cursor-aware command editing (Left/Right/Home/End/Delete/BackSpace/Ctrl-A/E/K). `HistoryState` moved from `session.rs` to `history.json`. 9 new tests.

**Session 102 — VSCode-Style Editor Groups (827 total, no new tests):**
`EditorGroup { tabs, active_tab }` replaces flat tabs. `open_editor_group/close/focus/move_tab/resize`; `calculate_group_window_rects`. `render.rs`: `EditorGroupSplitData`. Ctrl+\ split right, Ctrl+1/2 focus, Ctrl-W e/E split.

**Session 101 — Command Palette (10 new tests, 827 total):**
`PALETTE_COMMANDS` static (~65 entries); `palette_open/query/results/selected/scroll_top` engine fields; `open/close/update_filter/confirm/handle_palette_key`. GTK: `draw_command_palette_popup()`. TUI: `render_command_palette_popup()` + keyboard enhancement (`PushKeyboardEnhancementFlags`). Ctrl+Shift+P opens palette. 10 new tests.

**Session 100 — Menus + Workspace Parity + GTK overlay dropdown (817 total):**
GTK dropdown drawing order fixed (overlay DrawingArea above all panels). Dialog action routing fixed (drop engine borrow before routing). TUI menu actions fully wired. "Open Recent…" menu item + picker in both backends. Workspace session saved on quit + restored at startup. `base_settings` restores settings on folder switch. New commands: copy/cut/paste/termkill/about/openrecent.

**Session 99 — SC Panel VSCode Parity + Recent Commits + Bug Fixes (12 new tests, 813 total):**
Commit input row (`c`/Enter/Esc); push/pull/fetch from panel (`p`/`P`/`f`); bulk stage/unstage/discard-all on section headers; `:Gpull`/`:Gfetch`. Recent Commits section (last 20, collapsible). Fixed path resolution via `git::find_repo_root()`. 12 new tests.

**Session 98 — Lua Extension Mechanism (9 new tests, 801 total):**
mlua 5.4 vendored; `src/core/plugin.rs` (~430 lines); `vimcode.*` Lua API: `on/command/keymap/message/cwd/command_run/buf.*`; `~/.config/vimcode/plugins/` auto-loaded; hook points: save/open/normal-key/insert-key/command; `:Plugin list/reload/enable/disable`. 9 new tests.

**Session 97 — Source Control Panel (3 new tests, 792 total):**
`git.rs`: `status_detailed()`, `stage/unstage/discard_path()`, `worktree_list/add/remove()`, `ahead_behind()`. Engine: 7 SC fields; `sc_refresh/stage/discard/switch_worktree/handle_sc_key`; `:GWorktreeAdd/Remove`. GTK: `draw_source_control_panel()`. TUI: `TuiPanel::Git`, `render_source_control()`. 3 new tests.

**Session 96 — UI Polish + Workspaces (5 new tests, 789 total):**
GTK: `set_decorated(false)` + `WindowHandle` drag + window-control buttons [─][☐][✕] in menu bar; terminal title sync. Workspaces: `.vimcode-workspace` JSON; `open_folder/workspace/save_workspace_as`; per-project session (FNV-1a hash); GTK `FileDialog`; TUI fuzzy directory picker modal; `:cd/:OpenFolder/:OpenWorkspace/:SaveWorkspaceAs`. 5 new tests.

**Session 95 — C# Non-Public Members + Debug Output scrollbar (784 total, no new tests):**
`DapVariable.is_nonpublic: bool`; synthetic "Non-Public Members" group node in variables panel. `render.rs`: `build_var_tree` omits ` = ` for empty values. TUI: `debug_output_scroll` + draggable scrollbar; fixed height-computation for `bp_h` when debug output panel is open.

**Session 94 — Per-section scrollbars in debug sidebar (10 new tests, 784 total):**
`dap_sidebar_scroll: [usize;4]` + `dap_sidebar_section_heights: [u16;4]`; `dap_sidebar_ensure_visible()` + `dap_sidebar_resize_section()`; `DebugSidebarData` gains scroll_offsets/section_heights; fixed-height section allocation with per-section scrollbar in both GTK and TUI. 10 new tests.

**Session 93 — Scope-grouped variables in debug sidebar (5 new tests, 774 total):**
`dap_scope_groups: Vec<(String, u64)>` for additional DAP scopes beyond "Locals"; `poll_dap` parses ALL non-expensive scopes; expandable scope group headers appended after primary variables in both backends. 5 new tests.

**Session 92 — VSCode tasks.json + preLaunchTask (8 new tests, 769 total):**
`TaskDefinition` struct; `parse_tasks_json()`; `task_to_shell_command()`. Engine: `dap_pre_launch_done/dap_deferred_lang` fields; `dap_start_debug` migrates `.vscode/tasks.json` → `.vimcode/tasks.json`; `preLaunchTask` executed via `lsp_manager.run_install_command()`; `InstallComplete` with `"dap_task:"` prefix resumes/aborts debug. 8 new tests.

**Session 91 — Debug sidebar interactivity + C# DAP adapter (761 total):**
`dap_sidebar_has_focus` field; key guard in `handle_key()`; `dap_sidebar_section_item_count()` method. TUI+GTK: j/k/Tab/Enter/Space/x/d/q keyboard + click handler walks sections. `netcoredbg` adapter added (`dap_manager.rs`); `find_workspace_root` checks `.sln`/`.csproj`; `substitute_vars` handles `${workspaceFolderBasename}`. 3 new tests.

**Session 90 — Interactive debug sidebar + conditional breakpoints (12 new tests, 758 total):**
`BreakpointInfo` struct (line/condition/hit_condition/log_message) replaces `u64` in `dap_breakpoints`; `set_breakpoints` sends conditions. Sidebar: `handle_debug_sidebar_key` fully wired (j/k/Tab/Enter/x/d/q); helpers `dap_sidebar_section_len`, `dap_var_flat_count`, `dap_bp_at_flat_index`; recursive `build_var_tree()` in render.rs; `is_conditional_bp`/`◆` gutter; `:DapCondition/:DapHitCondition/:DapLogMessage`. 12 new tests.

**Session 89 — DAP polish + codelldb compatibility (746 total):**
`DapServer.pending_commands: HashMap<u64,String>` + `resolve_command()` — codelldb omits `command` from responses. `dap_seq_initialize` for deferred launch. Three-state debug button (Start/Stop/Continue). Navigate to stopped file/line via `scroll_cursor_center()`. ANSI/control stripping. `dap_wants_sidebar` one-shot flag auto-opens debug panel. `DebugSidebarData.stopped: bool`.

**Session 88b — Debugger bug fixes (743 total):**
`set_breakpoints` includes `source.name`; `stopOnEntry: false` in launch args; `Initialized` handler skips empty BP lists; `debug_sidebar_da_ref` for explicit `queue_draw()` on DAP events.

**Session 88 — VSCode-like debugger UI (12 new tests, 743 total):**
`LaunchConfig` struct + `parse_launch_json/type_to_adapter/generate_launch_json` in `dap_manager.rs`. Engine: `DebugSidebarSection`/`BottomPanelKind` enums; 8 new fields; `dap_add/remove_watch()`; `handle_debug_sidebar_key()`; `debug_toolbar_visible` default false. GTK: `SidebarPanel::Debug`, `draw_debug_sidebar()`. TUI: `TuiPanel::Debug`, `render_debug_sidebar()`. 12 new tests.

**Session 87 — :set wrap / soft line-wrap rendering (7 new tests, 731 total):**
`Settings.wrap: bool` (default false). `render.rs`: `RenderedLine.is_wrap_continuation` + `segment_col_offset`; `build_rendered_window` splits lines at `viewport_cols`; `max_col=0` disables h-scroll. Engine: `ensure_cursor_visible_wrap`; `move_visual_down/up` helpers; `gj`/`gk` bindings. 7 new tests.

**Session 86 — DAP panel interactivity + expression evaluation (4 new tests, 724 total):**
`dap.rs`: `evaluate()` request helper. Engine: `dap_panel_has_focus`, `dap_active_frame`, `dap_expanded_vars: HashSet<u64>`, `dap_child_variables: HashMap<u64,Vec<DapVariable>>`, `dap_eval_result`; `dap_select_frame()`, `dap_toggle_expand_var()`, `dap_eval()`; variable tree shows `▶`/`▼` + indented children. `:DapPanel/:DapEval/:DapExpand`. 4 new tests.

**Session 85 — DAP variables panel + call stack + output console (4 new tests, 720 total):**
`dap_stack_frames`, `dap_variables`, `dap_output_lines` engine fields; `poll_dap` chains stackTrace→scopes→variables; Output appends to `dap_output_lines` (capped at 1000). `render.rs`: `DapPanel` struct; GTK `draw_dap_panel()`; TUI `render_dap_panel()`. 4 new tests.

**Session 84 — DAP event loop + breakpoint gutter + stopped-line highlight (4 new tests, 716 total):**
`dap_current_line: Option<(String,u64)>`; `poll_dap` wired; `RenderedLine.is_breakpoint/is_dap_current`; `Theme.dap_stopped_bg` (#3a3000 amber); breakpoint gutter `●`/`◉`/`▶`/`◉`; stopped-line background in GTK+TUI. 4 new tests.

**Session 83 — DAP transport + engine + :DapInstall (23 new tests, 712 total):**
`src/core/dap.rs` (new): Content-Length framing; `DapEvent` enum; request helpers; 8 unit tests. `src/core/dap_manager.rs` (new): 5 adapters (codelldb/debugpy/delve/js-debug/java-debug); Mason resolution; real install commands. Engine: 4 new fields + 9 methods; replaced 9 stub commands; `:DapInstall <lang>`. 23 new tests.

**Session 82 — Menus + debug toolbar UI wiring (4 new tests, 684 total):**
Engine: `menu_move_selection()`/`menu_activate_highlighted()`; `execute_command` made `pub`; F5/F6/F9-F11 dispatch; 9 debug stub commands. GTK: Shift+F5/F11; toolbar pixel hit-test. TUI: Up/Down/Enter dropdown keyboard nav; highlighted row inversion; menu/toolbar click. 4 new tests.

**Session 81 — Menu bar + debug toolbar + Mason DAP detection (7 new tests, 680 total):**
`lsp.rs`: `MasonPackageInfo.categories`; `is_dap()/is_linter()/is_formatter()` helpers. Engine: `menu_bar_visible`, `menu_open_idx`, `debug_toolbar_visible`; `toggle_menu_bar/open_menu/close_menu/menu_activate_item()`; `:DapInfo`. `render.rs`: `MENU_STRUCTURE` (7 menus) + `DEBUG_BUTTONS` statics. GTK+TUI: `draw/render_menu_bar`, `draw/render_menu_dropdown`, `draw/render_debug_toolbar`. 7 new tests.

**Session 80 — Bug fix: LSP not starting for sidebar/fuzzy/split file opens (673 total):**
`lsp_did_open()` was only called from `Engine::open()`. Fixed by adding `self.lsp_did_open(buffer_id)` in `open_file_in_tab()` (3 paths), `open_file_preview()` (2 paths), `new_tab()`, `split_window()`. No new tests.

**Session 79 — Leader key + extended syntax highlighting + LSP features (19 new tests, 673 total):**
`settings.rs`: `leader: char` (default `' '`). `syntax.rs`: 10 new languages (C/TS/TSX/CSS/JSON/Bash/Ruby/C#/Java/TOML); 19 new tests. `lsp_manager.rs`: 6 new request methods (references, implementation, type_definition, signature_help, formatting, rename). `lsp.rs`: 6 new event variants + response parsers. Engine: `leader_partial`; `handle_leader_key()`; `gr`/`gi`/`gy`; `<leader>gf`/`<leader>rn`; `:Lformat`/`:Rename`; signature help on `(`/`,`.

**Session 78 — LSP expansion: Mason registry, auto-detect, :LspInstall (16 new tests, 654 total):**
`language_id_from_path()` gains 12 new extensions. `lsp.rs`: `MasonPackageInfo` + `parse_mason_package_yaml()` + `RegistryLookup/InstallComplete` events. `lsp_manager.rs`: `mason_bin_dir()`, `resolve_command()`, `registry_cache`, `fetch_mason_registry_for_language()`, `run_install_command()`. Engine: `:LspInstall <lang>`. 16 new tests.

**Session 77 — Terminal split drag-to-resize (638 total, no new tests):**
`terminal_split_left_cols: u16` engine field; `terminal_split_set_drag_cols()` + `terminal_split_finalize_drag()`; GTK: drag 4px near divider; TUI: `dragging_terminal_split` state. No new tests.

**Session 76 — Terminal horizontal split view (638 total, no new tests):**
`terminal_split: bool` field; `terminal_open/close/toggle_split()`; `terminal_split_switch_focus()` (Ctrl-W). `render.rs`: `TerminalPanel.split_left_rows/cols/focus`; `build_pane_rows()` helper. GTK+TUI: left/`│`/right split rendering; `⊞` toolbar button. No new tests.

**Session 75 — Terminal deep history + real PTY resize + CWD (638 total, no new tests):**
`TerminalPane.history: VecDeque<Vec<HistCell>>` (configurable scrollback, default 5000); `process_with_capture()`/`capture_scrolled_rows()`. `resize()` calls `master.resize(PtySize)`. `terminal_new_tab()` passes `self.cwd`. No new tests.

**Session 74 — Terminal find bug fixes (638 total, no new tests):**
Find now scans scrollback history (`Vec<(required_offset, row, col)>`); `terminal_find_update_matches()` scans at both offsets; `build_terminal_panel()` uses required_offset. GTK full-width background fill + auto-resize on `CacheFontMetrics`. No new tests.

**Session 73 — Terminal find bar (638 total, no new tests):**
Ctrl+F while terminal has focus opens inline find bar replacing tab strip; case-insensitive; active match orange, others amber; Enter/Shift+Enter navigate; Escape/Ctrl+F close. Engine: 4 fields + 7 methods. `render.rs`: `TerminalCell` +2 bools; `TerminalPanel` +4 find fields. GTK+TUI: routing, toolbar, cell colors. No new tests.

**Session 72:** Terminal multiple tabs + auto-close fix — `terminal_panes: Vec<TerminalPane>` + `terminal_active: usize` replace the single `terminal: Option<TerminalPane>` field. `terminal_new_tab()` always spawns a fresh shell; `terminal_close_active_tab()` removes current pane (closes panel if last); `terminal_switch_tab(idx)` switches active pane. `:term` always creates a new tab (via `EngineAction::OpenTerminal → NewTerminalTab`). Ctrl-T toggles panel (creates first tab if none). Alt-1–9 switches tabs (both GTK and TUI). Click on `[N]` tab label in toolbar switches tab; click on close icon closes active tab. `poll_terminal()` auto-removes exited panes immediately (all tabs, not just single-pane); panel closes when last pane exits. `terminal_resize()` resizes ALL panes. 638 tests (no change — PTY features are UI-only).

**Session 71:** Terminal panel draggable resize — `session.terminal_panel_rows: u16` (serde default 12) added to `SessionState`. GTK: `terminal_resize_dragging: bool` on `App`; header-row click starts drag; `Msg::MouseDrag` recalculates rows from y-position (clamped [5, 30]); `Msg::MouseUp` calls `terminal_resize(cols, rows)` + `session.save()`. TUI: `dragging_terminal_resize: bool` local var + new param in `handle_mouse()`; Up handler saves + resizes PTY. All hardcoded `13`/`12` row constants replaced dynamically. 638 tests (no change).

**Session 70:** Terminal polish — scrollbar draggable in both GTK + TUI; copy (Ctrl+Y) and paste (Ctrl+Shift+V / bracketed paste) wired up in both backends; TUI scrollbar colored to match editor; GTK full-width terminal; GTK editor scrollbar no longer overlaps terminal. 638 tests.

**Session 69:** Terminal panel bug fixes + scrollbar — fixed TUI crash (build_screen_for_tui didn't subtract quickfix/terminal rows from content_rows, causing OOB line number panic). Fixed TUI not-full-width (PTY opened with editor-column width; changed to terminal.size().ok().map(|s| s.width)). Added scroll_offset + scroll_up/down/reset() on TerminalPane; PageUp/PageDown changes offset. Added scrollbar: scrollback_rows on TerminalPanel; TUI rightmost column (░/█); GTK 6px Cairo strip. Fixed mouse click-to-focus; fixed TUI mouse selection; auto-close on shell exit. 638 tests.

**Session 68:** Integrated terminal panel — new `src/core/terminal.rs` (TerminalPane backed by portable-pty + vt100; background mpsc reader thread; poll(), write_input(), resize(), selected_text()). Engine: terminal: Option<TerminalPane>, terminal_open, terminal_has_focus; open_terminal(), close_terminal(), toggle_terminal(), poll_terminal(), terminal_write(), terminal_resize(), terminal_copy_selection(); EngineAction::OpenTerminal; :term/:terminal command. Settings: PanelKeys.open_terminal (default <C-t>). Render: TerminalCell, TermSelection, TerminalPanel, build_terminal_panel(), map_vt100_color(), xterm_256_color(); terminal: Option<TerminalPanel> on ScreenLayout. GTK: draw_terminal_panel(), gtk_key_to_pty_bytes(), terminal Msg variants, key routing. TUI: render_terminal_panel(), translate_key_to_pty(), extra Constraint::Length slot, idle poll, resize handler. 638 tests.

**Session 67:** VSCode mode F1 command access — F1 in handle_vscode_key() sets mode = Command; routing: top of handle_vscode_key() delegates to handle_command_key() when mode == Command; Escape returns to Insert (not Normal); after execute_command(), is_vscode_mode() guard returns to Insert; mode_str() shows `EDIT  F1:cmd  Alt-M:vim` and `COMMAND` during command bar; Settings::load() returns Self::default() under #[cfg(test)] so tests are hermetic regardless of user's settings.json. 3 new tests. 635 → 638 tests.

**Session 66:** VSCode edit mode toggle — EditorMode enum (Vim/Vscode) in settings.rs with serde; full handle_vscode_key() dispatcher with Shift+Arrow selection, Ctrl-C/X/V/Z/Y/A/S shortcuts, Ctrl+Arrow word nav, Ctrl+Shift+Arrow word select, smart Home, Ctrl+/ line comment toggle, Escape clears selection, typing replaces selection; toggle_editor_mode() (Alt-M) persists mode to settings.json; mode_str() returns "EDIT"/"SELECT"; undo model: each keypress is one undo group. 620 → 635 tests (+15).

**Session 65:** Completion popup arrow key navigation + Ctrl-Space re-trigger fix — Down/Up in Insert mode cycle completion candidates when popup visible; Ctrl-Space re-trigger fixed in TUI (translate_key() emitted key_name=" " but engine checks "space"; fixed by normalizing space to "space" in ctrl path); parse_key_binding fixed to accept named keys ("Space") so <C-Space> in settings.json parses correctly. 618 → 620 tests.

**Session 64:** Auto-popup completion — replaces ghost text; popup triggered by typing or Ctrl-Space; completion_display_only: bool determines Tab-accepts vs immediate-insert behavior; trigger_auto_completion() called after BackSpace and char-insert; poll_lsp() CompletionResponse sets display_only=true. Ghost text fields fully removed.

**Session 63:** Inline ghost text autosuggestions (later replaced by auto-popup in session 64) — dimmed suffix after cursor in Insert mode; buffer-word scan + async LSP; ghost_text/ghost_prefix/lsp_pending_ghost_completion fields; Tab accepts; Theme.ghost_text_fg (#636363). 613 → 619 tests (6 new).

**Session 62:** Configurable panel navigation keys (panel_keys) — new PanelKeys struct with 5 fields; parse_key_binding() for Vim-style notation. Removed ExplorerAction::ToggleMode (focus on explorer is sufficient). TUI: matches_tui_key() helper; Alt+E/Alt+F work from both editor and sidebar. GTK: matches_gtk_key(); Msg::ToggleFocusExplorer + new Msg::ToggleFocusSearch. 613 tests (7 net new).

**Session 61:** Replaced arboard with copypasta-ext 0.4. GTK: removed background clipboard thread; synchronous reads/writes via x11_bin::ClipboardContext. TUI: replaced ~180 lines of platform-detection with ~20 lines. Fixed TUI paste-intercept bug (key_name="" for regular chars; fixed to check unicode instead). 606 tests, no change.

**Session 59:** Explorer polish — (1) prompt delay fix: early continue in TUI event loop now sets needs_redraw=true. (2) move path editing with cursor key support in all sidebar prompts via SidebarPrompt.cursor field. (3) Auto-refresh every 2s. (4) Root folder entry at top of tree. (5) Removed ExplorerAction::Refresh. (6) New file/folder at root via pre-filled paths.

**Session 56:** VSCode-Like Explorer + File Diff — rename_file/move_file in engine; DiffLine enum (Same/Added/Removed); diff_window_pair/diff_results; cmd_diffthis/cmd_diffoff/cmd_diffsplit; LCS diff O(N×M), 3000-line cap; :diffthis/:diffoff/:diffsplit dispatch. Render: diff_status on RenderedLine; diff_added_bg/diff_removed_bg in Theme. GTK: RenameFile/MoveFile/CopyPath/SelectForDiff/DiffWithSelected msgs; F2 inline rename; right-click Popover; drag-and-drop. TUI: PromptKind::Rename + PromptKind::MoveFile; r/M keys; diff bg via per-row line_bg. 571 → 584 tests (13 new).

**Session 55:** Quickfix window — :grep/:vimgrep populates quickfix_items; :copen/:cclose toggle panel; :cn/:cp/:cc N navigate/jump. Persistent 6-row bottom strip. TUI: extra Constraint::Length slot + render_quickfix_panel(). GTK: content_bounds reduced by qf_px + draw_quickfix_panel(). Key routing via handle_quickfix_key(). 563 → 571 tests (8 new).

**Session 54:** Telescope-style live grep modal — grep_* fields + open_live_grep/handle_grep_key/grep_run_search/grep_load_preview/grep_confirm in engine. render.rs: LiveGrepPanel. GTK: draw_live_grep_popup(). TUI: render_live_grep_popup() + grep_scroll_top. Ctrl-G opens; two-column split (35% results, 65% preview); ±5 context lines.

**Session 53:** Fuzzy file finder — fuzzy_open/query/all_files/results/selected; open_fuzzy_finder() + walk_for_fuzzy() + fuzzy_filter(); fuzzy_score() with gap penalty + word-boundary bonus. GTK: draw_fuzzy_popup() centered modal. TUI: render_fuzzy_popup() with box-drawing chars + fuzzy_scroll_top. Ctrl-P opens.

**Session 52:** :norm command — :norm[al][!] {keys} on range. Ranges: current line, %, N,M, '<,'>. Key notation: literal + <CR>/<BS>/<Del>/<Left>/<Right>/<Up>/<Down>/<C-x>. Undo entries merged into one step. Fixed trim() ordering bug. 535 → 544 tests (9 new).

**Session 51:** it/at tag text objects — find_tag_text_object(); backward scan for enclosing <tagname>; forward scan for matching </tagname> with nesting depth; case-insensitive; handles attributes, self-closing, comments. 526 → 535 tests (9 new).

**Session 50:** CPU performance fixes — max_col cached in BufferState (not re-scanned every frame); TUI 60fps frame rate cap (min_frame = 16ms). 526 tests, no change.

**Session 49:** 6 vim features — toggle case (~), scroll-to-cursor (zz/zt/zb), join lines (J), search word under cursor (*/#), jump list (Ctrl-O/Ctrl-I, cross-file, max 100), indent/dedent (>>/<<, visual, dot-repeatable). 495 → 526 tests (31 new).

**Session 48:** LSP bug fixes + TUI performance — pending_requests map for deterministic routing; initialization guards on all notification methods; reader thread handles server-initiated requests; diagnostic flood optimization (50/poll cap, visible-only redraw); path canonicalization at lookup points; TUI needs_redraw flag + idle-only background work + adaptive poll timeout. 495 tests, no change.

**Session 47:** LSP support — lsp.rs (~750 lines) + lsp_manager.rs (~340 lines). Engine: LSP lifecycle hooks, poll_lsp(), diagnostic nav (]d/[d), go-to-definition (gd), hover (K), LSP completion (Ctrl-Space). Render: DiagnosticMark + HoverPopup. GTK: wavy underlines, colored gutter dots, hover popup. TUI: colored underlines + E/W/I/H gutter chars, hover popup. Settings: lsp_enabled + lsp_servers. 458 → 495 tests (37 new).

**Session 46:** TUI scrollbar drag fix — removed deferred pending_h_scroll; drag event coalescing (consecutive Drag events → only final rendered); unified scrollbar color Rgb(128,128,128). 458 tests, no change.

**Session 45:** Replace across files — replace_in_project() in project_search.rs; ReplaceResult struct; engine: project_replace_text/start_project_replace/poll_project_replace/apply_replace_result. GTK: Replace Entry + "Replace All" button. TUI: replace_input_focused; Tab switches inputs; Alt+H shortcut. 444 → 458 tests (14 new).

**Session 44:** Enhanced project search — ignore crate for .gitignore support; regex crate for pattern matching; SearchOptions with 3 toggles (case/word/regex); results capped at 10,000; GTK toggle buttons; TUI Alt+C/Alt+W/Alt+R. 438 → 444 tests (6 new).

**Session 43:** Search panel bug fixes — GTK CSS fix (listbox → .search-results-list); startup crash fix in sync_scrollbar. TUI: scrollbar drag for search results; j/k ensures selection visible. 438 tests, no change.

**Session 42:** Search panel polish + CI fix — TUI viewport-independent scroll; scrollbar column jump for both panels; removed unused DisplayRow.result. GTK: dark background CSS fix; always-visible scrollbar. Both: async search thread (start_project_search + poll_project_search). CI: two map_or(false,...) → is_some_and(...). 434 → 438 tests (4 new).

**Session 41:** VSCode-style project search — project_search.rs (ProjectMatch + search_in_project()). Engine: 3 new fields + 3 methods. GTK: Search panel with Entry + ListBox. TUI: TuiPanel::Search; search_input_mode; render_search_panel(). 429 → 434 tests (5 new).

**Session 40:** Paragraph and sentence text objects — ip/ap (inner/around paragraph) + is/as (inner/around sentence) via find_text_object_range(). 420 → 429 tests (9 new).

**Session 39:** Stage hunks — Hunk struct + parse_diff_hunks() in git.rs; run_git_stdin() + stage_hunk(); BufferState.source_file; jump_next/prev_hunk(); cmd_git_stage_hunk(); ]c/[c navigation; gs/`:Ghs`/:Ghunk staging. 410 → 420 tests (10 new).

**Session 38:** :set command — expand_tab/tabstop/shift_width settings; boolean/numeric/query syntax; line number options interact vim-style; Tab respects expand_tab/tabstop. 388 → 410 tests (22 new).

**Session 37 (cont):** Session restore + quit fixes — :q closes tab/quits; :q! force-close; :qa/:qa!; Ctrl-S saves; open_file_paths() filters to visible buffers only. 387 → 388 tests (1 new).

**Session 37:** Auto-indent + Completion menu + Quit/Save — auto_indent copies leading whitespace on Enter/o/O; Ctrl-N/Ctrl-P word completion with floating popup; CompletionMenu in render; 4 completion theme colors. 369 → 388 tests.

**Session 36:** TUI scrollbar overhaul + GTK h-scroll fix — vsplit separator as left-pane scrollbar; h-scrollbar row with thumb/track; corner ┘ when both axes; unified ScrollDragState with is_horizontal; scroll wheel targets pane under cursor; sync_scroll_binds() after all mouse scroll/drag; per-window viewport. GTK: set_scroll_left_for_window for non-active pane h-scrollbar. max_col on RenderedWindow. 369 tests, no change.

**Session 35:** :Gblame + explorer preview fix + scrollbar fixes — :Gblame/:Gb runs git blame --porcelain in scroll-synced vsplit. Fixed :Gdiff/:Gstatus/:Gblame deleting original buffer after split. Explorer single-click → open_file_preview (preview tab, replaced by next click); double-click → permanent. H-scrollbar page_size fixed per-window using cached Pango char_width. V-scroll sync now fires on scrollbar drag (VerticalScrollbarChanged). 365 → 369 tests (4 new).

**Session 34:** Explorer tab bug fix + extended git — open_file_in_tab() switches to existing tab or creates new one. :Gstatus/:Gs, :Gadd/:Gadd!, :Gcommit <msg>, :Gpush. 360 tests, no change.

**Session 33:** Git integration — git.rs with subprocess diff parsing; ▌ gutter markers (green=added, yellow=modified); branch name in status bar; :Gdiff/:Gd; has_git_diff flag. TUI fold-click detection fixed. 357 → 360 tests (3 new).

**Session 32:** Session file restore + fold click polish — open file list + active buffer saved/restored on launch; full gutter width clickable for fold toggle; GTK gutter 3px left padding. 357 tests, no change.

**Session 31:** Code Folding — za/zo/zc/zR; indentation-based; fold state in View (per-window); +/- gutter indicators; clickable gutter; fold-aware rendering (GTK + TUI). 346 → 357 tests (11 new).

**Session 30:** Nerd Font Icons + TUI Sidebar + Mouse + Resize — icons.rs shared module; GTK activity bar + toolbar + file tree icons; TUI sidebar with full explorer (j/k/l/h/Enter, CRUD, Ctrl-B, Ctrl-Shift-E); TUI activity bar; drag-to-resize sidebar in GTK + TUI; full TUI mouse: click, scroll, scrollbar; per-window scrollbars. 346 tests, no change.

**Session 29:** TUI backend (Stage 2) + rendering abstraction — render.rs ScreenLayout bridge; ratatui/crossterm TUI entry point; cursor shapes; Ctrl key combos; viewport sync. 346 tests, no change.

**Session 28:** Ctrl-R Command History Search — reverse incremental search through command history; Ctrl-R activates; Ctrl-R again cycles older; Escape/Ctrl-G cancels. 340 → 346 tests (6 new).

**Session 27:** Cursor + Scroll Position Persistence — reopening restores exact cursor line/col and scroll; positions saved on buffer switch and at quit. Also fixed settings file watcher feedback loop freeze and r+digit bug. 336 → 340 tests (3 new).

**Session 26:** Multi-Language Syntax Highlighting — Python, JavaScript, Go, C++ via Tree-sitter; auto-detected from extension; SyntaxLanguage enum; Syntax::new_from_path(). 324 → 336 tests (12 new).

**Session 25:** Marks + Incremental Search + Visual Case Change — m{a-z} marks; ' and ` jumps; real-time incremental search with Escape cancel; u/U in visual mode. 305 → 324 tests.

**Session 24:** Reverse Search + Replace Character + Undo Line — ? backward search; direction-aware n/N; r replaces char(s) with count/repeat; U restores current line. 284 → 300 tests.

**Session 23:** Session Persistence — CRITICAL line numbers bug fixed; command/search history with Up/Down (max 100, persisted); Tab auto-completion; window geometry persistence; explorer visibility state. 279 → 284 tests.

**Session 22:** Find/Replace — :s command (line/%/visual, g/i flags); Ctrl-F dialog (live search, replace, replace all); proper undo/redo. 269 → 279 tests (9 new).

**Session 21:** Macros — full keystroke recording (nav, Ctrl, special, arrows); Vim-style encoding; playback with count prefix; @@ repeat; recursion protection. 256 → 269 tests (14 new).

**Sessions 15–20:** GTK UI foundations — activity bar, sidebar, file tree CRUD, preview mode, focus+highlighting, scrollbars, explorer button, settings auto-init, visual block mode (Ctrl-V). 232 → 256 tests.

**Sessions 11–12:** High-priority motions + line numbers + config. 146 → 214 tests.
