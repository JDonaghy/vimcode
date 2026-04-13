# Known Bugs

- **(Intermittent) TUI rendering artifacts** — Stale characters from a previous view sometimes linger on screen. Mitigated in Session 244: `terminal.clear()` on resize events and on popup dismiss (picker/folder picker transition to hidden). Root cause: ratatui's incremental diff can miss cells when the physical terminal state diverges from its buffer tracking. Workaround for any remaining cases: Ctrl+L forces a full screen redraw.

- **`x` with count + `.` repeat deletes too many characters** — Using `4x` to delete four characters, then moving to the next line and pressing `.` to repeat deletes more than four characters. The repeat count is not being preserved or restored correctly for the `x` command.

- **Git panel operations have no progress indicator** — Commit, push, and pull operations in the git panel give no visual feedback that an operation is in progress. VSCode shows a spinner for a few seconds to confirm the action was triggered. VimCode should show a spinner (using the existing notification/progress system) while git operations are running.

- **Git branch in status bar not updated on external change** — When the git branch is changed outside of VimCode (e.g. via CLI `git checkout`), the branch name in the status bar is not updated. Needs file watching on `.git/HEAD` or periodic polling to detect external branch switches.


### Win-GUI gaps (vs GTK reference) — found by systematic GTK↔Win-GUI comparison

**Critical (data loss / broken core features):**
- ~~**Win-GUI: tab close skips dirty check**~~ — Fixed: checks `engine.dirty()` before closing; shows engine dialog with Save/Discard/Cancel.
- ~~**Win-GUI: picker (fuzzy finder) not mouse-interactive**~~ — Fixed: click result to select, click outside to dismiss; scroll wheel navigates picker items.
- ~~**Win-GUI: dialog buttons not clickable**~~ — Fixed: button rect hit-testing in `on_mouse_down`, dispatches `dialog_click_button(idx)`, outside-click dismisses.
- ~~**Win-GUI: QuitWithUnsaved action silently ignored**~~ — Fixed: shows engine dialog with Save All & Quit / Quit Without Saving / Cancel. WM_CLOSE also checks for unsaved changes.

**Medium (incorrect behavior):**
- ~~**Win-GUI: scroll doesn't skip folded lines**~~ — Fixed: uses `scroll_down_visible()`/`scroll_up_visible()` instead of raw arithmetic.
- ~~**Win-GUI: picker scroll not intercepted**~~ — Fixed: scroll wheel checks `picker_open` first and navigates picker results.
- ~~**Win-GUI: VSCode selection not cleared on click**~~ — Fixed: calls `vscode_clear_selection()` before `mouse_click` when in VSCode mode.
- ~~**Win-GUI: cursor not kept in viewport after scroll**~~ — Fixed: cursor now clamped into viewport (with scrolloff) after scroll, matching GTK. Also calls `sync_scroll_binds()`.
- ~~**Win-GUI: terminal tab switching by mouse missing**~~ — Fixed: click on numbered tab labels in terminal toolbar switches `terminal_active`. Matches tab label geometry from draw code.

**Medium (incorrect behavior — new):**
- ~~**Win-GUI: tabs disappear when second editor group is created**~~ — Fixed: `draw_group_tab_bar` only subtracted the tab bar height from `bounds.y`, but the reserved space also includes the breadcrumb row. Tab bars were drawn at the breadcrumb position (hidden behind breadcrumbs/editor content). Fixed by accounting for breadcrumb offset in both drawing and click slot caching.

**Medium (incorrect behavior — new):**
- ~~**Win-GUI: explorer single-click opens permanent tab instead of preview**~~ — Investigated: preview system works correctly. `open_file_preview()` is called and reuses the preview tab. Multiple tabs appear only when the preview is promoted (by clicking the tab, editing, or saving) — this is expected VSCode behavior.
- ~~**Win-GUI: terminal steals keyboard focus from editor**~~ — Fixed: added `terminal_has_focus = false` in the editor area click handler, matching GTK/TUI behavior.
- ~~**Win-GUI: no active tab accent line across editor groups**~~ — Fixed: `draw_tabs` now takes `show_accent` parameter; `draw_group_tab_bar` passes `is_active_group` so the 2px accent line only appears on tabs in the focused group.

**Medium (found by systematic focus/draw audit):**
- ~~**Win-GUI: sidebar focus persists after editor click**~~ — Fixed: added `clear_sidebar_focus()` on editor click. Settings/AI/Search/Debug focus flags were never cleared when clicking the editor, causing keyboard events to route to sidebar panels.
- ~~**Win-GUI: sidebar focus persists after terminal click**~~ — Fixed: added `clear_sidebar_focus()` and `sidebar.has_focus = false` when clicking in the terminal panel area.
- ~~**Win-GUI: dialog text and buttons overflow the dialog box**~~ — Fixed: dialog width now auto-sized from content (buttons + body + title) instead of hardcoded 400px. Both draw and click handler use the same calculation.

**Medium (deferred):**
- ~~**Win-GUI: terminal can't regain focus after editor click**~~ — Fixed: terminal content clicks now always set `terminal_has_focus` (was only for split-pane case).

**Medium (new — Win-GUI interaction gaps):**
- ~~**Win-GUI: extension install flashes many windows**~~ — Fixed: added `hidden_command()` helper in `git.rs`; replaced all 6 `Command::new("curl")` in `registry.rs` and `ai.rs` with `hidden_command("curl")` which sets `CREATE_NO_WINDOW` on Windows.
- ~~**Win-GUI: extension panel not appearing after install**~~ — Fixed: full `draw_ext_panel()` renderer with sections, tree items, badges, actions, scrollbar, help popup. Activity bar shows dynamic ext panel icons (Nerd Font glyphs mapped to Segoe MDL2 equivalents). Click/keyboard/scroll handlers, `ext_panel_focus_pending` polling. `WinSidebar.ext_panel_name` field (matching TUI pattern) overrides `active_panel` when set.
- ~~**Win-GUI: activity bar icons non-functional**~~ — Investigated: activity bar clicks were already working (panel switching, focus flags, sc_refresh). The reported issue was likely that the panels themselves didn't respond after switching (see search/AI/git fixes below).
- ~~**Win-GUI: back/forward navigation arrows non-functional**~~ — Fixed: added hit-test code for ◀/▶ arrows in the title bar click handler, calling `tab_nav_back()`/`tab_nav_forward()`. Also added command center search box click → `open_command_center()`.
- ~~**Win-GUI: window resize only works from top/top corners**~~ — Fixed: `on_nchittest()` now handles all 8 resize zones (top/bottom/left/right + 4 corners) instead of only the top edge.
- ~~**Win-GUI: search/replace panel non-functional**~~ — Fixed: added full keyboard routing in `on_key_down()` and `on_char()` — input mode (typing, Tab to toggle search/replace, Enter to execute, Backspace, Ctrl+V paste), results navigation (j/k, Enter to open file), Alt+C/W/R/H toggles for search options. Also set `search_has_focus` on activity bar click.
- ~~**Win-GUI: git panel entries not clickable or navigable**~~ — Fixed: added full keyboard routing for git panel — navigation mode (j/k/s/S/d/D/c/p/P/f/b/B/?/Tab/Enter/Escape/r), commit input mode (text entry, cursor movement, Ctrl+Enter to commit), branch picker, help dialog. All routed through `engine.handle_sc_key()`.
- ~~**Win-GUI: AI panel non-functional**~~ — Fixed: added full keyboard routing — navigation mode (j/k/G/g/i/q), input mode (text entry, Enter to submit, cursor movement, Ctrl+V paste). All routed through `engine.handle_ai_panel_key()`.
- ~~**Win-GUI: settings panel scroll wheel broken**~~ — Fixed: sidebar scroll handler now dispatches by active panel — Settings scrolls `settings_scroll_top`, AI scrolls `ai_scroll_top`, Search scrolls `search_scroll_top`, Explorer scrolls `sidebar.scroll_top`.
- ~~**Win-GUI: settings panel search non-functional**~~ — Settings search was already handled by the existing `handle_settings_key()` routing (the `/` key enters search mode). The scroll fix above makes it usable by allowing scrolling through results.

**Low (missing features):**
- ~~**Win-GUI: breadcrumb clicks not handled**~~ — Fixed: clicking breadcrumb segments opens scoped picker (directory→file picker, symbol→@picker).
- ~~**Win-GUI: group divider drag not implemented**~~ — Fixed: cached dividers from ScreenLayout; full drag-to-resize with cursor change.
- **Win-GUI: horizontal scrollbar drag not implemented** — Horizontal scrollbar renders but is not interactive. GTK has h-scrollbar click and drag. Win-GUI only handles vertical scrollbar drag.
- ~~**Win-GUI: diff toolbar buttons not clickable**~~ — Fixed: ↑/↓/≡ button click handlers dispatch to `jump_prev_hunk()`/`jump_next_hunk()`/`diff_toggle_hide_unchanged()`.
- ~~**Win-GUI: diff peek key routing missing**~~ — Already working: keys route through `handle_key()` → `handle_diff_peek_key()`.
- ~~**Win-GUI: tab tooltip dismiss-on-mouseout missing**~~ — Fixed: mouse hover shows file path, mouseout clears tooltip.

**Systemic fixes found by pattern analysis (Session 270):**
- ~~**Win-GUI: terminal steals keyboard from all non-terminal UI**~~ — Fixed: added blanket `terminal_has_focus = false` at the start of `on_mouse_down()`, `on_mouse_dblclick()`, and `on_right_click()`. Terminal click handlers re-enable it. Previously only sidebar and editor clicks cleared it; tab bar, menu, breadcrumbs, dialogs, scrollbars, and 12+ other click areas were affected.
- ~~**Win-GUI: search panel was a draw stub**~~ — Fixed: `draw_search_panel()` now renders query text, replace text, toggle indicators (Aa/Ab|/.*), status line, and search results with file headers and selection highlight. Was previously just a placeholder with "Search (use :grep)".
- ~~**Win-GUI: terminal panel overlap with status bar**~~ — Fixed: `draw_terminal()` used hardcoded `2.0 * lh` for bottom chrome. Now computes dynamically based on `separated_status_line` and per-window status settings. Also fixed 3 terminal row boundary checks and 2 terminal click handler Y calculations.
- ~~**Win-GUI: DAP debugpy check flashes console**~~ — Fixed: 2 `Command::new(&binary)` calls in `dap_manager.rs` now use `hidden_command()`.
- ~~**Win-GUI: `Event::SoftBreak` renamed to `Event::Break`**~~ — Fixed: stale edit in `markdown.rs` broke all builds. Restored to `Event::SoftBreak`.

**Fixed in Session 272 (git panel rendering + tab scroll):**
- ~~**Win-GUI: git panel is a rendering stub with no interactivity**~~ — Fixed: full `draw_git_panel()` rewrite with commit input box, button row, 4 collapsible sections, selection highlight, scrollbar, branch picker popup, help dialog. Click handling for all zones (items, buttons, commit input, double-click-to-open-diff). Hover dwell for commit log popups. Button hover tracking.
- ~~**Win-GUI: new tabs not visible in tab bar (no scroll-into-view)**~~ — Fixed: Win-GUI now reports available tab bar width via `set_tab_visible_count()` after each paint frame and calls `ensure_all_groups_tabs_visible()`. Previously `tab_bar_width` defaulted to `usize::MAX`, making `ensure_active_tab_visible()` a no-op.

**Fixed in Session 271 (ext panels + Nerd Font + breadcrumb):**
- ~~**Win-GUI: extension panel not appearing after install**~~ — Fixed: full ext panel rendering, activity bar icons, click/keyboard/scroll handlers.
- ~~**Win-GUI/TUI: breadcrumb path starts with `?C:`**~~ — Fixed: `build_breadcrumbs_for_group()` strips UNC prefix from file path and cwd.
- ~~**TUI: tab tooltip path starts with `~/` on Windows**~~ — Fixed: uses `MAIN_SEPARATOR` (`~\` on Windows, `~/` on Linux).
- ~~**TUI: tab tooltip shows UNC prefix**~~ — Fixed: `strip_unc_prefix()` applied in `tab_tooltip_at_col()`.
- ~~**Win-GUI: empty breadcrumb covers tab bar for diff views**~~ — Fixed: `draw_breadcrumb_bar()` returns early when segments are empty.
- ~~**Win-GUI: diff toolbar overlaps last tab text**~~ — Fixed: `draw_tabs()` respects `max_width`, stops rendering tabs before the diff toolbar area.
- ~~**TUI: activity bar shows diamond glyphs on Windows**~~ — Fixed: 9 hardcoded Nerd Font codepoints replaced with `icons::ICON.c()` calls. Auto-detection disables nerd fonts when no Nerd Font is installed. Startup message warns user.

**Fixed in Session 269 (interaction parity audit):**
- ~~**Win-GUI: tab tooltip shows UNC prefix (`\\?\`)**~~ — Fixed: `strip_unc_prefix()` in `paths.rs`, also applied to `copy_relative_path()`.
- ~~**Win-GUI: extension panel clicks/keyboard not working**~~ — Fixed: click geometry matched to draw's fractional Y layout; keyboard routing for `i`/`d`/`u`/`r`/`/`/`j`/`k`/`Return`/`q`; double-click opens README; selection highlight.
- ~~**Win-GUI: clipboard sync missing (yank/paste broken)**~~ — Fixed: register→clipboard sync after yank, clipboard→register load before paste. Bidirectional clipboard=unnamedplus.
- ~~**Win-GUI: context menu items not hoverable**~~ — Fixed: mouse-move tracking highlights items on hover.
- ~~**Win-GUI: tab close button not clickable**~~ — Fixed: tab slot width uses `measure_ui_text_width()` (proportional UI font) matching `draw_tabs()`.
- ~~**Win-GUI: first tab of second group not clickable**~~ — Fixed: tab slots clipped to group bounds.
- ~~**Win-GUI: menu bar click/hover misaligned**~~ — Fixed: all 4 menu bar handlers use proportional font measurement matching draw code.
- ~~**Win-GUI: Ctrl+V doesn't paste in insert mode**~~ — Fixed: intercepts Ctrl+V in Insert/Replace mode, pastes system clipboard.
- ~~**Win-GUI: clipboard_paste() broken on Windows**~~ — Fixed: added `#[cfg(target_os = "windows")]` PowerShell branch. Fixes Ctrl+V in command mode, search mode, picker.
- ~~**Win-GUI: mouse cursor always I-beam**~~ — Fixed: arrow over tabs/sidebar/menus, resize near dividers, I-beam over editor text only.
- ~~**Win-GUI: generic sidebar handler swallows keys for Git/AI panels**~~ — Fixed: guarded with `active_panel == Explorer`.

**Verified fixed this session:**
- ~~Win-GUI: activity bar icon size mismatch~~ — Now uses Segoe MDL2 Assets / Segoe Fluent Icons at 20px in 48×48 cells.
- ~~Win-GUI: no tab drag-and-drop~~ — Full drag with threshold, drop zone computation, visual overlay, ghost label.
- ~~Win-GUI: no terminal split~~ — Split button, pane rendering, focus switching, divider drag.
- ~~Win-GUI: no click/mouse handling for new popups~~ — Editor hover (click/dismiss/scroll), panel hover (dismiss), debug toolbar (button clicks), context menu (full click handling).
- ~~Win-GUI: scrollbar rendering~~ — Fixed to use `theme.scrollbar_thumb`/`scrollbar_track` instead of hardcoded alpha.
- ~~Win-GUI: explorer double-click replaces buffer~~ — Now uses `open_file_preview` (single-click) and `open_file_in_tab` (double-click/Enter).
- ~~Win-GUI: context menu draws under explorer~~ — Context menu, dialog, notifications now draw after sidebar in on_paint.
- ~~Win-GUI: context menu clicks pass through~~ — Full click handler with item selection, action dispatch, outside-click dismiss.
- ~~Win-GUI: :term opens /bin/bash on Windows~~ — `default_shell()` returns `powershell.exe` on Windows.



## Resolved

- **Win-GUI: text rendering truncation** — `draw_styled_line` only rendered syntax spans, leaving gaps invisible (`crate` in `pub(crate)`, variable names). Fixed by drawing text between and after spans in default foreground color.
- **Win-GUI: settings icon clipped and not clickable** — Gear icon positioned at `rt_h - lh` (below status bar). Repositioned to `sidebar_bottom - lh` (above bottom chrome). Click handler updated with matching geometry.
- **Win-GUI: settings panel stub** — Settings panel was a two-line stub. Replaced with full interactive form: categories (expandable), bools, integers, strings, enums, extension settings, search input. Keyboard handling via `handle_settings_key()` in both `on_key_down` and `on_char`.
- **Win-GUI: global status bar drawn over per-window status** — `draw_status_bar` painted full-width blue bar even when per-window status was active (empty `status_left`/`status_right`). Fixed by early return when both are empty. Also fixed `below_terminal_px` to reserve 1 row (not 2) when per-window status is active.
- **Win-GUI: per-window status bar all one color** — Status bar painted entire background with `status_bg`. Now renders per-segment backgrounds (mode name gets its own colored bg, matching TUI).
- **Win-GUI: editor text bleeding past window bounds** — Added `PushAxisAlignedClip` around each editor window draw. Sidebar panel clip rect tightened to `sidebar_bottom`.
- **Win-GUI: command line descenders clipped** — Added bottom margin so characters with descenders (`g`, `y`, `p`, `:`) aren't cut off at the window edge.
- **Win-GUI: sidebar panel/command line background gaps** — Panel background now extends full height; command line background starts at `editor_left`; `panel_h` uses `sidebar_bottom` instead of `rt_h`.
- **Win-GUI: opening a file replaces current buffer** — Explorer single-click now uses `OpenMode::Preview` (transient tab); double-click uses `OpenMode::Permanent`. Previously all opens hardcoded `Permanent`.
- **Win-GUI: no preview tab mode** — Preview tabs now render with dimmer text color in draw.rs. Engine's preview logic now works because explorer single-click creates preview tabs.
- **Win-GUI: missing explorer/tab context menus** — Right-click on explorer items now calls `open_explorer_context_menu(path, is_dir, ...)`. Right-click on tab bar now calls `open_tab_context_menu(group_id, tab_idx, ...)`. Previously only editor right-click worked.
- **Win-GUI: no settings button** — Activity bar now renders a gear icon pinned to the bottom row (matching TUI/VSCode). Click handler routes to `SidebarPanel::Settings`.
- **Win-GUI: status bar not clickable** — Added `win_status_segment_hit_test()` + click handler before editor click. Calls `build_window_status_line()` to get segments, maps click to `StatusAction`, dispatches via `handle_status_action()`. Also fixed `pixel_to_editor_pos()` to exclude per-window status bar area.
- **Win-GUI: tab bar clicks not working** — Tab slot Y coordinates used `lh` (line height) instead of `TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT` (actual title bar height), and slot height was `lh` instead of `lh * TAB_BAR_HEIGHT_MULT`. Clicks at the real tab bar position missed the cached slots. Fixed both single-group and multi-group tab slot calculations.
- **Win-GUI: no terminal resize drag** — Added `terminal_resize_drag` field + header click detection + WM_MOUSEMOVE drag handler + mouse-up finalization with PTY resize and session save.
- **GTK terminal panel toggle requires two clicks** — On first use, the `[P]` status bar button sent an async `Msg::ToggleTerminal` via Relm4's message queue instead of creating the terminal tab synchronously. The async path caused a one-frame delay where the terminal state wasn't set, requiring a second click. Fixed by calling `terminal_new_tab()` immediately in the click handler (matching TUI behavior which already handled `OpenTerminal` synchronously).

- **Terminal panel resize not working via mouse** — Two bugs: (1) mouse events didn't trigger a redraw (`needs_redraw` was not set after `handle_mouse`), so the drag visually did nothing; (2) the available-space formula used hardcoded `2` instead of the computed `bottom_chrome` value, giving wrong row counts with default settings (`window_status_line=true` → `bottom_chrome=1`). Fixed by unconditionally setting `needs_redraw=true` after all mouse events, and using `bottom_chrome` in the formula.

- **TUI terminal paste not working** — Ctrl+V in TUI terminal didn't paste. Three fixes: (1) added `poll_terminal()` after paste writes for immediate feedback, (2) wrapped paste in bracketed paste sequences for multi-line safety, (3) falls back to VimCode `+`/`"` registers when system clipboard is empty. Also added error messages instead of silent failure.
- **GTK terminal Ctrl+C sends newline instead of copying** — `gtk_key_to_pty_bytes()` returned empty for Ctrl+letter keys because GTK's `to_unicode()` filters control chars. Added fallback to derive control byte from `key_name`. Also added Ctrl+Shift+C handler to copy terminal selection.
- **TUI terminal drag-select offset** — Selection used absolute screen column instead of terminal-relative column (`col.saturating_sub(editor_left)`). Row was off by one because mouse handler hardcoded `2` bottom chrome rows (status+cmd) while per-window status lines (default) hide the global status bar. Fixed all 10 instances in mouse.rs to compute `bottom_chrome` dynamically.
- **Editor drag leaks into terminal panel** — Editor text drag that moved outside all editor windows fell through to terminal drag handler. Added early return when `mouse_text_drag` is active and no editor window matches.

- **`o` inserts whitespace instead of creating new line (YAML/bash)** — The `o` handler's `insert_pos` calculation only checked for `\n` line endings. For CRLF files (`\r\n`), it inserted between `\r` and `\n`; for lone `\r` files, the new `\n` was absorbed into a CRLF pair, failing to create a new line. Fixed by checking for `\r\n` (skip both) and `\r` alone (insert before it). 4 new tests.
- **Terminal draws on top of fuzzy finder (TUI + GTK)** — Terminal panel was rendered after the picker popup in both TUI and GTK draw orders, overwriting it. Fixed by moving picker/folder picker/tab switcher/dialog rendering to after the bottom panel in both backends, so popups have higher z-order.
- **GTK visual select highlights wrong line** — `draw_visual_selection()` used `line_idx - scroll_top` to map buffer lines to view rows, which breaks with wrap, diff padding, or any non-1:1 mapping. Rewrote to iterate over `RenderedLine` entries and use `rl.line_idx` for correct view-row lookup. Also skips diff padding rows (`DiffLine::Padding`) which share `line_idx` with the following real line but occupy a separate view row.
- **Right-click in terminal shows editor context menu** — Right-click handler fell through to `open_editor_context_menu()` without checking if the click was on the terminal panel. Added terminal bounds check to suppress the context menu on terminal area right-clicks.
- **Terminal panel steals clicks from explorer tree** — Terminal click handler checked row bounds but not column, so clicks on the sidebar at the same row as the terminal were intercepted. Fixed by adding `col >= editor_left` guard to the TUI terminal panel click handler.
- **Live grep scroll wheel changes file instead of scrolling preview** — Scroll events in the picker didn't distinguish left (results) vs right (preview) pane. Added column-based hit test: scrolling over the preview pane now scrolls the preview content via `picker_preview_scroll`. Also increased preview line limits (30→500 for files, ±5→±50 context for grep matches) and set initial scroll to center on the match line.
- **Pasting clipboard into terminal broken** — TUI only handled Ctrl+Shift+V (uppercase 'V' in crossterm) for terminal paste; plain Ctrl+V was forwarded as a raw control character. Added Ctrl+V (lowercase) handler in TUI. Also added Ctrl+V paste in GTK terminal focus (previously only Ctrl+Shift+V worked).

- **`%` brace match doesn't scroll to matched brace** — `%` jumped to the matching brace but the viewport didn't follow for off-screen matches. Fixed by centering the viewport when the match is more than half a screen away (same approach as search `n`). 1 new test.
- **TUI tab underline extends to tab number prefix** — Active tab underline covered the `N:` prefix. Fixed by splitting the render loop to only apply the underline modifier and accent color to the filename portion (after the `: ` prefix).
- **Preview tab can't be made permanent by clicking its tab** — `goto_tab()` now calls `promote_preview()` on the active buffer if it's in preview mode, matching VSCode behavior. Works for both GTK and TUI since both call `goto_tab()`. 1 new test.
- **Accidental explorer drag triggers move dialog to same location** — `confirm_move_file()` now detects when the source file's parent directory is the same as the destination and silently returns. 2 new tests.
- **GTK tab bar hides tabs despite available space** — The GTK tab bar measured available width using "M" as the reference character, but since GTK uses a proportional sans-serif font, "M" is much wider than average. This made the engine think fewer tabs fit, causing unnecessary scroll offset. Fixed by measuring a representative 15-character sample string for the average char width.


- **TUI spell underlines bleed into fuzzy finder** — `set_cell()` and `set_cell_wide()` only reset character/fg/bg but not `cell.modifier` or `cell.underline_color`, so `Modifier::UNDERLINED` from spell rendering survived into the picker overlay. Fixed by resetting both fields in `set_cell()`, `set_cell_wide()`, and `set_cell_styled()` (which left stale `underline_color` when passed `None`).

- **Marksman LSP status indicator stuck on "initializing"** — `mark_server_responded()` was only called on non-empty hover/definition responses, so servers like `marksman` that don't support semantic tokens (and may return empty hover content for many positions) stayed stuck at "Initializing". Fixed by marking the server as responsive on `Initialized` event (handshake completion is sufficient proof of readiness), and removing the empty-result guards on hover/definition responses.

- **Spell check underline misaligned** — GTK backend called `layout.set_attributes(None)` before computing underline/cursor positions via `index_to_pos`, stripping `font_scale` attributes (1.1–1.4× on markdown headings). Positions were calculated at normal font width while text was rendered scaled, causing underlines to start before the word and end in the middle. Fixed by preserving Pango attributes (`build_pango_attrs(&rl.spans)`) for diagnostics, spell underlines, cursor, ghost text, and extra cursors. Also fixed spell checker not initializing when enabled via Settings sidebar or settings.json reload.
- **Inline rename cursor position tests failing on macOS CI** — `test_inline_rename_start` and `test_inline_rename_typing_and_cursor` expected cursor at full filename length, but `start_explorer_rename()` positions cursor at stem end (before extension). Tests updated to match.
- **Hardcoded colors in rendering code** — Added 4 new Theme fields (`scrollbar_thumb`, `scrollbar_track`, `terminal_bg`, `activity_bar_fg`) with values for all 6 built-in themes + VSCode JSON importer. Replaced hardcoded `RColor::Rgb(128,128,128)` scrollbar thumbs (3 in render_impl.rs, 4 in panels.rs), `RColor::Rgb(90/220,...)` git status colors → `theme.git_added/modified/deleted`, `RColor::Rgb(100,100,110)` activity bar icons → `theme.activity_bar_fg`, `rgb(30,30,30)` terminal bg → `theme.terminal_bg` (GTK + TUI), debug button colors → `theme.git_added`/`theme.diagnostic_error`, terminal find-match colors → `theme.search_match_*`, search result markup → `theme.function`/`theme.foreground`, cursor indicator → `theme.scrollbar_thumb`, tab drag overlay → `theme.cursor`/`theme.background`/`theme.foreground`, ext panel secondary bg → `theme.status_bg.darken(0.15)`. GTK CSS: scrollbar slider, h-editor-scrollbar, find dialog, find-match-count colors now theme-aware via `make_theme_css()` overrides. Remaining STATIC_CSS hex values are either close-button platform convention or dead fallbacks already overridden by `make_theme_css()`.
- **TUI: Settings button in activity bar not clickable** — The status bar click handler (`row + 2 == term_height`) and command line guard (`row + 1 >= term_height`) in `mouse.rs` intercepted ALL clicks on the bottom two terminal rows regardless of column, before the activity bar handler could process them. The settings button is rendered at the bottom of the activity bar, which coincides with the command line row. Fixed by adding `col >= ab_width` guards so those checks only apply to clicks outside the activity bar column.
- **GTK Explorer: first click/Enter on folder required two presses** — `tree_row_expanded()` removed the dummy placeholder child before populating real children, leaving the directory with zero children momentarily. GTK auto-collapsed the row when its last child was removed. Fixed by populating real children first, then removing the dummy. Also fixed Enter after arrow-key navigation to use `ExplorerActivateSelected` (syncs cursor→selection) instead of native `row_activated`.
- **GTK: Inline rename in explorer disappears immediately** — Root cause: periodic `update_tree_indicators` (every 1s) called `set_value` on TreeStore rows, cancelling the active GTK cell editor. Also `RefreshFileTree` could clear the store during editing. Fixed by skipping indicator updates and tree refreshes while `name_cell.is_editing()` is true. Also fixed related SIGSEGV from `__NEW_FILE__`/`__NEW_FOLDER__` marker rows in the indicator walk, context menu popover stealing focus (explicit `popdown()` + 50ms delay), and GTK rename pre-selecting entire filename instead of stem only (`connect_editing_started` + `Entry::select_region()`).
- **LineEnding::detect() crash on multi-byte chars** — `&text[..8192]` panicked when byte 8192 landed inside a multi-byte character (e.g. `─` at bytes 8190..8193). Fixed by backing up to nearest char boundary via `is_char_boundary()` loop.
- **VSCode mode undo granularity** — Every character typed created its own undo entry. Fixed by keeping the undo group open across consecutive character insertions in `handle_vscode_key()`, breaking only on non-character actions (cursor movement, Backspace, Return, Ctrl+* commands) or external cursor moves (mouse clicks). `vscode_undo_group_open` + `vscode_undo_cursor` fields on Engine. 5 new tests.
- **Search `/` results land at viewport bottom** — `jump_to_search_match()` called `ensure_cursor_visible()` which with `scrolloff=0` placed the match at the absolute bottom edge. Fixed by centering the match when it lands in the bottom quarter of the viewport.
- **Tab bar hides tabs when there's room** — `tab_visible_count` feedback loop: TUI renderer returned tab **count** but `set_tab_visible_count()` stored it as `tab_bar_width` (column width). With 5 tabs visible, engine thought it had 5 columns of space, causing a death spiral where each frame hid more tabs. Fixed by returning actual available width in columns (`tab_end_for_content - area.x`), matching the GTK backend. Also fixed `tab_display_width()` off-by-one (+3→+2 for close+separator).
- **Tab bar doesn't update on terminal resize** — After shrinking the terminal, active tab could be off-screen because `tab_bar_width` was stale. Fixed by calling `ensure_all_groups_tabs_visible()` after each render frame reports updated widths.

All bugs below were fixed in Session 225 or earlier. See SESSION_HISTORY.md for details.

- **Search `n` doesn't scroll far enough — match off-screen** — Both TUI and GTK approximate `viewport_lines` missed the tab bar row entirely (GTK) or didn't account for breadcrumbs/hide_single_tab (TUI). The approximate value was set every loop iteration, overwriting the accurate per-window value from the renderer. Fixed by computing correct chrome row count (status + cmd + tab bar + breadcrumbs, minus hidden tab bar) in both backends.
- **Explorer tree doesn't reveal active buffer on folder open** — TUI sidebar had no `reveal_path()` call at startup or after `open_folder()`. Added initial reveal before the event loop and after folder picker confirmation.
- **Visual yank doesn't move cursor to selection start** — `yank_visual_selection()` left cursor at end of selection. Vim moves cursor to start (line-wise: col 0; char-wise: start col). Fixed in `visual.rs` + updated integration test.
- **YAML syntax breaks after editing** — tree-sitter-yaml has an external scanner (like Markdown) that corrupts state during incremental reparsing without `InputEdit`. Fixed by skipping old-tree reuse for YAML in `syntax.rs::reparse()`, same as Markdown.
- **Crash in `completion_prefix_at_cursor` (index out of bounds)** — cursor col could exceed `chars.len()` after edits; clamped col to valid range in `motions.rs`.
- **Swap files don't preserve most recent edits on crash** — swap files were only written every 4 seconds (updatetime); edits in the last 0-4s were lost on panic. Added `emergency_swap_flush()` that writes all dirty buffers immediately, invoked from panic hooks (both GTK and TUI) and from the TUI `catch_unwind` error handler. Global engine pointer registered at startup via `swap::register_emergency_engine()`.
- **Crash in `active_window_mut` (stale WindowId after tab/group close)** — `active_tab().active_window` could point to a `WindowId` no longer in `self.windows` after certain tab/group close sequences. Added `repair_active_window()` self-healing method that finds a valid window from the current tab's layout or creates a scratch window as last resort. Called from `active_window_mut()` and after all close operations (`close_tab`, `close_editor_group`, `close_window`, `close_other_tabs`, `close_tabs_to_right/left`, `close_saved_tabs`).

- **`cargo run -- file.rs` restores entire previous session** — Skip `restore_session_files()` when CLI file/directory argument is provided. Use `open_file_with_mode(Permanent)` to load the file into the initial scratch window's tab (no leftover "[No Name]" tab).
- **TUI: cannot drag tab to create new editor group when only one group exists** — `compute_tui_tab_drop_zone()` single-group branch only handled tab bar reorder. Added content area edge zone detection using terminal size, and visual feedback rendering in `render_tab_drag_overlay()` for `Center`/`Split` zones.
- **GTK: "Don't know color ''" warnings on startup** — Explorer TreeStore rows initialized columns 3 (foreground) and 5 (indicator color) with empty strings `""`. GTK tried to parse these as CSS colors. Replaced with valid hex color defaults (`dir_fg_hex` / `modified_color`).
- **Search highlights wrong text in non-active buffers** — `engine.search_matches` stored char offsets from the active buffer only, but `build_spans()` applied them to all visible buffers (splits). Non-active buffers highlighted text at the same char positions regardless of content. Fixed by computing per-buffer search matches in `build_rendered_window()` via `compute_search_matches_for_buffer()` helper; `build_spans()` now takes `search_matches` + `is_active_buffer` parameters. `search_index` (current match highlight) only applies to the active buffer.
- **Can't paste into search/command/replace inputs** — Ctrl+V paste was missing from `/` search, `:` command, and TUI project search/replace input fields. Added `clipboard_paste()` handler to `handle_search_key()`, `handle_command_key()`, and TUI search panel input mode. GTK project search uses native Entry widgets (already supported paste).
- **Visual mode `x` doesn't delete selection** — `x` in visual mode did nothing because `handle_visual_key()` only matched `'d'`, not `'x'`. Added `'x'` as alias for `'d'` with `pending_key.is_none()` guard (so `rx` still works as replace-with-x). 2 new tests.

- **Explorer tree blue items / no dir color** — Added `explorer_dir_fg`/`explorer_active_bg` Theme fields; directories get distinct color in both TUI and GTK; active buffer row gets subtle background highlight.
- **:Explore opens new tab** — `netrw_activate_entry()` now uses `switch_window_buffer()` instead of `open_file_in_tab()`, opening the file in the current window.
- **Search n not scrolling / ?<enter> not reversing** — Added `ensure_cursor_visible()` after search jump; empty `?<enter>` repeats previous search in reverse direction.
- **Git commit double-line status** — `sc_do_commit()` truncates git output to first line so status bar stays single-line.
- **Double-click word-wise drag** — `mouse_drag_word_mode`/`mouse_drag_word_origin` fields snap to word boundaries during drag; anchor flips when dragging before/after origin word.
- **Can't paste clipboard into fuzzy finder** — `Ctrl+V` in the unified picker was ignored because the `ctrl=true` guard blocked all ctrl key combos. Added `Ctrl+V` handler that pastes first line of clipboard into picker query.
- **Markdown typing color bleed** — Typing near colored sections caused wrong colors due to stale byte offsets in deferred syntax highlights. Fixed with debounced 150ms syntax refresh in both GTK and TUI idle loops via `tick_syntax_debounce()`.
- **TUI hover dismiss consumes click** — Clicking on a symbol with an active hover popup dismissed the hover but didn't move the cursor (required a second click). Fixed by letting the click fall through to the editor click handler after dismissing the hover, matching GTK behavior.
- **TUI selection wrong position with wrap** — Visual selection (mouse drag) rendered on wrong lines when `set wrap` was enabled. `render_selection()` used `scroll_top + row_idx` to compute buffer line, which is incorrect for wrapped text where multiple visual rows share one buffer line. Fixed to use `line.line_idx` and adjust selection columns by `segment_col_offset`.
- **TUI fuzzy finder stale chars** — Cycling through files in the picker or dismissing it left stale characters on screen. `render_picker_popup` and `render_folder_picker` didn't clear their background area before drawing. Added full background clear pass to both functions.
- **GTK scrollbar / tab group divider overlap** — Scrollbar of left group extended into adjacent group's space; divider drag gesture (6px hit zone) intercepted scrollbar clicks. Fixed: scrollbar inset 2px from group edge; divider gesture skips claiming when click is in scrollbar zone (rightmost 10px of any window rect).
- **GTK core dump from draw callback panic** — Panic in extern "C" `set_draw_func` callback aborts the process. Fixed with `catch_unwind` wrapper + replacing all `.unwrap()` with `.ok()` on Cairo fill/stroke/save/restore/paint operations.
- **GStrInteriorNulError from NUL hotkey** — `DialogButton.hotkey: '\0'` produced NUL in button label string via `format_button_label()`. Fixed to return label unchanged when `hotkey == '\0'`. Also sanitized NUL bytes from file content in render.
- **Lightbulb duplication on wrapped lines** — Code action lightbulb icon rendered on every wrapped continuation line. Fixed with `is_wrap_continuation` guard in both GTK and TUI backends.
- **Phantom "Loading..." hover popup** — Mouse hover triggered "Loading..." popup → LSP null response → dismiss → dwell re-fires → infinite flash loop. Fixed: mouse hover no longer shows "Loading..." (popup appears only when LSP returns content); null-position suppression prevents re-request loops; keyboard hover (`gh`) retains "Loading..." with 3s auto-dismiss. Also: only installed extensions can start LSP servers (built-in registry gated).
- **Flatpak build broken** — `cargo-sources.json` had stale vendored crates (tree-sitter 0.24.7) while `Cargo.lock` required 0.26.7. Regenerated with `flatpak-cargo-generator.py`. Going forward, re-run the generator whenever `Cargo.lock` changes.
- **Extension "u" update key does nothing** — `ext_sidebar_selected` is a flat index across installed + available sections, but key handlers compared it directly against `installed.len()` without accounting for collapsed sections. Fixed with `ext_selected_to_section()` helper.
- **Search panel input broken** — TUI click handler for search panel didn't set `sidebar.has_focus = true`, so keystrokes fell through to editor. Fixed.
- **Git insights hover on non-cursor lines** — `clear_annotations()` cleared `line_annotations` but not `editor_hover_content`, so old hover content accumulated for every visited line. Fixed.
- **Semantic tokens disappear after hover popup** — LSP server returning `result: null` for semantic token requests was parsed as empty tokens and stored, wiping existing highlighting. Fixed by only accepting responses with an actual `data` array.
- **Terminal backspace key-hold batching** — `poll_terminal()` only ran during idle time, so held-key output wasn't rendered until release. Fixed by polling immediately after `terminal_write()`.
- **Sidebar scrollbar drag leaks to other handlers** — TUI: explorer/ext panel scrollbar drags had no persistent state, so mouse-move events leaked to editor text selection. Fixed with `dragging_generic_sb` state variable. GTK: ext panel scrollbar `GestureDrag` never claimed the event sequence. Fixed by claiming in `drag_begin`.
- **Tab hover tooltip** — Hovering over a tab now shows the file's full path with `~` shortening for the home directory. Also fixed `tab_close_hit_test` Y-coordinate bug when breadcrumbs are enabled.
