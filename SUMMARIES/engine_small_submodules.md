# Engine Small Submodules

## accessors.rs — 494 lines
Convenience facade methods for accessing the active group/buffer/window.
- `buffer()` / `buffer_mut()` — active window's buffer
- `view()` / `view_mut()` — active window's viewport
- `cursor()` — active cursor position
- `active_group()` / `active_group_mut()` — active editor group
- `active_tab()` / `active_tab_mut()` — active tab in active group
- `repair_active_window()` — self-heal stale active_window ID (find valid window or create scratch)
- `is_tab_bar_hidden(group_id)` — true if tab bar should be hidden (single group, ≤1 tab, setting on)
- `adjust_group_rects_for_hidden_tabs(rects, height)` — expand content area when tab bar hidden
- `sidebar_has_focus()` — true if any sidebar panel has keyboard focus
- `clear_sidebar_focus()` — clear all sidebar panel focus flags
- `explorer_indicators()` — git statuses + diagnostic counts for explorer tree; propagates both recursively to parent dirs

## search.rs — 642 lines
Cursor visibility, scroll synchronization, project search/replace, word search.
- `ensure_cursor_visible()` / `ensure_cursor_visible_wrap()` — scroll to keep cursor in view
- `clamp_cursor_col()` — keep cursor within line bounds
- `sync_scroll_binds()` — synchronize scroll-bound window pairs (e.g. :Gblame)
- `run_project_search(root)` / `start_project_search(root)` — async project-wide search
- `run_project_replace(root)` / `start_project_replace(root)` — async project-wide replace
- `poll_project_search()` / `poll_project_replace()` — check async results
- `search_word_under_cursor(forward)` — * and # motions
- `move_visual_down()` / `move_visual_up()` — gj/gk for wrapped lines

## source_control.rs — 952 lines
Git source control panel operations and key handling.
- `sc_refresh()` — reload git status
- `sc_stage_selected()` / `sc_stage_all()` / `sc_unstage_all()` — staging operations
- `sc_discard_selected()` / `sc_discard_all_unstaged()` — discard changes
- `sc_push()` / `sc_pull()` / `sc_fetch()` / `sc_sync()` — remote operations
- `sc_do_commit()` — commit with message
- `handle_sc_key(key, ctrl, unicode)` — source control panel key routing
- `handle_sc_commit_input_key(key, ctrl, unicode)` — commit message input
- `sc_open_branch_picker()` / `sc_branch_picker_confirm()` — branch switching
- `sc_flat_len()` / `sc_flat_to_section_idx(flat)` — flat-index navigation helpers

## lsp_ops.rs — 711 lines
LSP lifecycle management and extension registry operations.
- `ensure_lsp_manager()` — lazy-init LSP manager
- `lsp_ensure_active_buffer()` — open LSP connection for current buffer
- `lsp_did_open(buffer_id)` — notify LSP of file open
- `ext_installed_manifests()` / `ext_available_manifests()` — extension listing
- `ext_refresh()` / `poll_ext_registry()` — async extension registry fetch
- `ext_install_from_registry(name)` — download and install extension
- `ext_open_selected_readme()` — show extension README
- `ext_show_remove_dialog(name)` / `ext_remove(name, remove_tools)` — extension removal
- `ext_update_one(name)` / `ext_update_all()` — extension updates

## ext_panel.rs — 3,138 lines
Extension panel system (Lua panels), hover popups (panel + editor), and source control hover content.
- `handle_ext_panel_key(key, ctrl, unicode)` — Lua extension panel key handler
- `handle_ext_panel_double_click()` — double-click action dispatch
- `handle_ext_panel_input_key(key, unicode)` — panel input field handler
- `ext_panel_flat_len()` / `ext_panel_flat_to_section()` — flat-index navigation
- `show_panel_hover(panel, item_id, markdown, rect)` — show panel item hover popup
- `dismiss_panel_hover()` / `poll_panel_hover()` — hover lifecycle
- `trigger_editor_hover_at_cursor()` — keyboard-triggered editor hover (K/gh)
- `show_editor_hover(content, source)` — display editor hover popup
- `dismiss_editor_hover()` — close editor hover
- `sc_hover_markdown(flat_idx)` — generate hover content for SC panel items
- `sc_hover_file()` / `sc_hover_log_entry()` / `sc_hover_branch_info()` — SC hover helpers

## panels.rs — 1,726 lines
AI chat panel, dialog system, and swap file crash recovery.
- `ai_send_message(msg)` — send message to AI provider
- `ai_poll()` — poll for AI streaming response
- `handle_ai_key(key, ctrl, unicode)` — AI panel key handler
- `show_dialog(dialog)` / `show_error_dialog(msg)` — modal dialog display
- `process_dialog_result(id)` — handle dialog button press
- `tick_swap_files()` — periodic swap file writes
- `check_swap_recovery(path)` — detect and offer crash recovery
- `emergency_swap_flush()` — write swap files for ALL dirty buffers immediately (called from panic hooks)

## plugins.rs — 653 lines
Lua plugin lifecycle and dispatch.
- `plugin_init()` — load plugins from `~/.config/vimcode/plugins/` and extension dirs
- `plugin_fire_event(event)` — dispatch hook to all plugins
- `plugin_run_command(name, args)` — try plugin command handler
- `plugin_run_keymap(mode, key)` — try plugin keymap handler
- `apply_plugin_ctx(ctx)` — apply plugin side-effects to engine state

## dap_ops.rs — 1,541 lines
DAP debugger operations: polling, breakpoints, sidebar navigation, stepping.
- `poll_dap()` — process DAP events (stopped, output, terminated, etc.)
- `dap_toggle_breakpoint()` — F9 toggle breakpoint
- `dap_start()` — F5 launch/continue debug session
- `dap_step_over()` / `dap_step_in()` / `dap_step_out()` — stepping
- `dap_stop()` — Shift+F5 terminate debug session
- `handle_dap_sidebar_key(key)` — debug sidebar navigation
- `dap_expand_variable(idx)` — expand variable tree node

## vscode.rs — 1,614 lines
VSCode edit mode and menu bar handling.
- `handle_vscode_key(key, ctrl, shift, unicode)` — VSCode mode key handler
- `handle_vscode_select_key(key, ctrl, shift)` — selection mode in VSCode mode
- `menu_open(idx)` / `menu_close()` — menu bar toggle
- `menu_activate_item(menu, item)` — execute menu action
- `handle_menu_key(key)` — menu navigation

## picker.rs — 1,275 lines
Unified fuzzy picker (Telescope-style), command center, quickfix, and branch picker.
- `fuzzy_score(query, candidate)` — subsequence match scoring
- `open_picker(source)` — open picker with file/grep/command/buffer source
- `handle_picker_key(key, ctrl, unicode)` — picker input and navigation
- `picker_confirm()` — execute selected picker item (includes branch switching via `Gswitch`)
- `quickfix_jump(idx)` — jump to quickfix entry
- `open_command_center()` — opens picker in CommandCenter mode
- `picker_filter_command_center()` — prefix-aware routing (>, @, #, :, ?)
- `picker_populate_document_symbols()` — populate picker from LSP document symbol response
- `picker_populate_workspace_symbols()` — populate picker from LSP workspace symbol response
- `picker_populate_branches()` — populate picker from git branch list
- `picker_request_document_symbols()` — send LSP documentSymbol request
- `picker_request_workspace_symbols()` — send LSP workspace/symbol request
- `fuzzy_filter_items()` — shared fuzzy filter helper

## terminal_ops.rs — 525 lines
Integrated terminal management.
- `terminal_toggle()` — Ctrl-T show/hide terminal panel
- `toggle_terminal_maximize(target_rows)` — Ctrl-Shift-T toggle maximize; saves/restores session.terminal_panel_rows via terminal_saved_rows
- `close_terminal()` — hide panel; auto-restores saved rows if maximized
- `terminal_new()` — create new terminal tab
- `terminal_close(idx)` — close terminal tab
- `terminal_write(data)` — send input to active terminal
- `terminal_resize(cols, rows)` — resize PTY
- `poll_terminal()` — read terminal output

## spell_ops.rs — 282 lines
Spell checking operations.
- `spell_next()` / `spell_prev()` — ]s/[s jump to next/prev misspelling
- `spell_suggest()` — z= show spelling suggestions
- `spell_add_word()` — zg add word to user dictionary
- `spell_remove_word()` — zw remove word from user dictionary
