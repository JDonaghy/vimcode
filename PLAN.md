# VimCode Implementation Plan

- ~~Improve unindenting and indenting behavior to make it context aware~~ **Done** — `dedent_lines()` now computes the minimum indent across all non-blank lines in the selection and removes that amount uniformly (capped at `shift_width`), preserving relative nesting structure. Blank lines are skipped for the minimum calculation. Undo restores in a single step. 6 new tests.

- ~~investigate bundling the nerd font glyphs~~ **Done** — centralized icon registry (`icons.rs`), `use_nerd_fonts` setting with ASCII fallback, bundled 13KB Nerd Font subset for GTK, extension `fallback_icon` API

- ~~add a way to clearly indicate the current tab that works across editor groups~~ **Done** — `tab_active_accent` theme color; GTK draws 2px colored top border on active tab in focused group; TUI uses colored underline (ratatui 0.29); 6 built-in themes + VSCode JSON importer (`tab.activeBorderTop`)

- ~~upgrade ratatui to 0.28+ to unlock colored underlines for TUI tab accent~~ **Done** — upgraded ratatui 0.27→0.29; colored underlines for tab accent, diagnostics, and spell errors; migrated deprecated `buf.get_mut()`→index syntax, `frame.size()`→`frame.area()`; fixed TUI tab bar scroll feedback loop (was returning count instead of width in columns)

- ~~add a smart indent and outdent feature that is language aware~~ **Done** — `smart_indent_for_newline()` + `line_triggers_indent()` + `auto_outdent_for_closing()` in `motions.rs`; Enter in insert mode and `o` in normal mode now add extra indent after `{`/`(`/`[` (universal), `:` (Python), `do`/`then` (Lua/Ruby/Shell); typing `}`/`)`/`]` as first non-blank auto-outdents; `==` operator also language-aware. **Auto-detect indentation**: `BufferState.detected_indent` + `detect_indent()` analyzes indent deltas on file open; `effective_shift_width()` on Engine prefers detected value over `settings.shift_width`; all indent operations (>>, <<, Ctrl+T/D, smart indent, ==) use it. 15 new tests

- ~~update the bicep extension to indicate that comments are "//" not "#"~~ **Done** — added `"bicep"` to the `//`-family in the built-in comment style table (`comment.rs`)

- ~~implement ":$" to go to EOF and check if any related commands remain unimplemented~~ **Done** — `:$` goes to last line; also `:+N`, `:-N`, `:.` line address commands in `execute.rs`

- ~~implement a fuzzy find to search open buffers with the default key combo being `<leader>sb`~~ **Done** — `PickerSource::Buffers` with `picker_populate_buffers()`; `<leader>sb` binding; `:Buffers` command; "Search: Open Buffers" palette entry; shows file icons, dirty/active flags

- ~~**Help > Key Bindings overhaul**~~ **Done** — Help > Key Bindings now opens the `:Keybindings` scratch buffer reference. Added `PickerSource::Keybindings` for fuzzy-searchable key bindings via `<leader>sk`; parses reference text into picker items by category with user remaps marked; "Help: Search Key Bindings" palette entry; `"nop"` picker action for view-only items

- ~~ensure a crash report is always logged to a tmp file and make a best effort to notify the user of its location so they can submit a bug report~~ **Done** — `crash_log_path()` + `write_crash_log()` helpers in `swap.rs` using `std::env::temp_dir()` (cross-platform); GTK panic hook now prints crash log path + issue URL to stderr; fixed issues URL to `github.com/JDonaghy/vimcode/issues`

> Session history archived in **SESSION_HISTORY.md**. Recent work summary in **PROJECT_STATE.md**.

---

## Recently Completed
- **Session 269**: Win-GUI interaction parity (19 fixes). New features: breadcrumb clicks, group divider drag, diff toolbar buttons, tab tooltip, terminal selection/paste/copy, extension panel keyboard + double-click. Bugs: UNC path prefix, clipboard sync, tab close/click geometry (proportional font mismatch), context menu hover, tab slot bounds overflow, menu bar hit-test, insert mode paste, generic sidebar key swallowing. Systematic audit found 4 additional bug classes. Updated `docs/NATIVE_GUI_LESSONS.md` with 3 new sections (10-12) and 15 new checklist items.
- **Session 268**: Win-GUI bug fixes (16 items). Original 9 critical+medium fixes, plus: tabs disappear with 2+ groups, terminal steals focus, tab accent only on active group, sidebar focus persistence, dialog overflow. Created `docs/NATIVE_GUI_LESSONS.md`.
- **Session 267**: Win-GUI bug blitz + parity test suite. Fixes: activity bar icons (Segoe MDL2 Assets), tab drag-and-drop, terminal split (rendering + buttons + divider drag), popup mouse handlers (editor hover, panel hover, debug toolbar, context menu), scrollbar theme colors, explorer file open (open_file_preview / open_file_in_tab), context menu z-order + click dispatch, default shell (powershell.exe on Windows). Phase 2c action parity harness (`UiAction` enum, 26 variants, 6 new tests). Systematic GTK↔Win-GUI comparison found 12 additional bugs (see BUGS.md).
- **Session 266**: 10 Win-GUI parity fixes — text rendering gap-filling, interactive settings panel (rendering + keyboard), per-segment status bar colors, editor window clipping, sidebar/command line layout fixes, command line descender margin.
- **Session 265**: Backend parity harness (`UiElement` enum, 27 variants, 3 collectors, 7 parity tests); 6 Win-GUI rendering fixes (editor hover, diff peek, debug toolbar, diff toolbar, tab tooltip, panel hover) — zero known rendering gaps remaining.
- **Session 264**: Context-aware dedent; TUI terminal resize fix; GTK terminal toggle fix; 7 Win-GUI fixes (preview tabs, settings button, context menus, status bar clicks, tab bar clicks, terminal resize drag); rendering test infrastructure (9 ScreenLayout + 10 TUI assertion + 6 insta snapshot tests); Win-GUI bug audit; **v0.9.0 release**.
> Sessions 263 and earlier in **SESSION_HISTORY.md**.

### Bug Fixes
- [x] Picker preview stale chars when cycling files — explicit per-row clear + tab sanitization
- [x] Insert mode click past EOL — `set_cursor_for_window` allows cursor one past last char in insert/replace mode
- [x] Scrollbar drag moves cursor — replaced `set_cursor_for_window` with `set_scroll_top_for_window`
- [x] Git panel discard needs confirm dialog — `pending_sc_discard` + `confirm_sc_discard` dialog
- [x] Tree-sitter-latex link error on Windows — `kind = "static"` on `#[link]`
- [x] 8 tests fail on Windows paths — `#[cfg(not(target_os = "windows"))]` + cross-platform assertions
- [x] TUI spell underlines bleed into fuzzy finder — `set_cell()`/`set_cell_wide()`/`set_cell_styled()` now reset `modifier` + `underline_color`
- [x] GTK core dump from panic in extern "C" draw callback — `catch_unwind` + `.ok()` on Cairo operations
- [x] GStrInteriorNulError crash from NUL byte in dialog button hotkey
- [x] Lightbulb code action icon duplicated on wrapped lines
- [x] Phantom "Loading..." hover popup when no LSP / LSP returns null — mouse hover deferred, null-position suppression, auto-dismiss timeout
- [x] Save message shows relative path instead of full absolute path
- [x] Status line shows filename only instead of full path
- [x] Unrecognized file types (.md, .txt, etc.) no longer default to Rust syntax highlighting
- [x] Tree-sitter upgrade to 0.26 + Lua and Markdown syntax highlighting (20 languages)
- [x] TUI clipboard broken (copypasta_ext xclip stdin pipe not closed, DISPLAY env missing)
- [x] VSCode edit mode: git insights ghost text + hovers now work (cursor_move hook, annotation/dwell gates)
- [x] GTK hover popup text overflow — Pango word wrapping instead of clipping
- [x] Stale LSP hover following clicks — clear `lsp_hover_text` on dismiss
- [x] GTK hover popup click-to-focus — cached popup rect, SearchPollTick race fix
- [x] TUI mouse drag capture — `dragging_generic_sb` cleared on MouseUp
- [x] GTK ext panel scrollbar drag leak — `set_state(Claimed)` in `drag_begin`
- [x] Tab hover tooltip — full file path with `~` shortening (GTK Cairo + TUI overlay)
- [x] Double hover popup — mutual exclusion between panel hover and editor hover
- [x] VS Code Light theme — `vscode-light` / `light+` built-in colorscheme
- [x] Extension "u" update key does nothing — `ext_selected_to_section()` helper for collapsed-section-aware flat index mapping
- [x] Flatpak CI build broken — regenerated `cargo-sources.json` (stale tree-sitter vendored crate)
- [x] TUI hover dismiss consumes click — removed early return, click falls through to editor
- [x] TUI selection wrong position with wrap — `line.line_idx` + `segment_col_offset` in `render_selection()`
- [x] Markdown typing color bleed — 150ms debounced `tick_syntax_debounce()` in both backends
- [x] GTK scrollbar / tab group divider overlap — scrollbar inset 2px, divider gesture scrollbar zone check
- [x] TUI fuzzy finder stale chars — background clear pass in `render_picker_popup`/`render_folder_picker`
- [x] Explorer tree blue items / no dir color — `explorer_dir_fg`/`explorer_active_bg` Theme fields
- [x] :Explore opens new tab — `netrw_activate_entry()` uses `switch_window_buffer()`
- [x] Search n not scrolling / ?<enter> not reversing — `ensure_cursor_visible()` after search jump
- [x] Git commit double-line status — `sc_do_commit()` truncates to first line
- [x] Double-click word-wise drag — word boundary snapping with `mouse_drag_word_mode`/`mouse_drag_word_origin`
- [x] Ctrl+V paste in fuzzy finder — added handler in `handle_picker_key()`
- [x] Search panel input broken — TUI click handler sets `sidebar.has_focus = true`
- [x] Search `n` doesn't scroll far enough — viewport_lines missed tab bar/breadcrumbs/hide_single_tab chrome rows in both GTK and TUI
- [x] Explorer tree doesn't reveal active buffer on folder open — added `reveal_path()` at TUI startup and after `open_folder()`
- [x] Visual yank doesn't move cursor to selection start — `yank_visual_selection()` moves cursor to start (Vim behavior)
- [x] YAML syntax breaks after editing — added YAML to tree-sitter reparse exclusion (external scanner corruption)
- [x] Crash in `completion_prefix_at_cursor` (index out of bounds) — clamped cursor col to `chars.len()`
- [x] Swap files don't preserve most recent edits on crash — `emergency_swap_flush()` + global engine pointer + panic hooks
- [x] Crash in `active_window_mut` (stale WindowId after tab/group close) — `repair_active_window()` self-healing + called after all close operations
- [x] Git insights hover on non-cursor lines — clear `editor_hover_content` in `clear_annotations()`
- [x] Semantic tokens disappear after hover — only accept responses with actual `data` array
- [x] Terminal backspace key-hold batching — poll immediately after `terminal_write()`
- [x] Sidebar scrollbar drag leaks — `dragging_generic_sb` state + GTK gesture `set_state(Claimed)`
- [x] CLI file arg restores entire previous session — skip `restore_session_files` when CLI arg given; use `open_file_with_mode(Permanent)` to reuse scratch tab
- [x] TUI: cannot drag tab to create new editor group with one group — added edge zone detection + visual feedback in `compute_tui_tab_drop_zone` / `render_tab_drag_overlay`
- [x] GTK "Don't know color ''" warnings — empty TreeStore color columns (3, 5) replaced with valid hex defaults
- [x] Swap recovery dialog shown for unmodified buffers after crash — compare swap content with disk file, silently delete if identical
- [x] GTK explorer focus not returning to editor after file open — clear `explorer_has_focus`/`tree_has_focus` in `OpenFileFromSidebar`
- [x] GTK 100% CPU after opening file from explorer — caused by stuck `explorer_has_focus` state (same fix as above)
- [x] Drag-to-select text leaks across editor groups — `mouse_drag_origin_window` field locks drag to originating window until mouse-up; cross-window drag events ignored
- [x] `%` brace match doesn't scroll to matched brace — center viewport on far jumps
- [x] TUI tab underline extends to tab number prefix — filename-only underline
- [x] Preview tab can't be made permanent by clicking tab — `goto_tab()` promotes preview
- [x] Accidental explorer drag triggers move to same location — source==dest guard
- [x] GTK tab bar hides tabs despite available space — proportional font char width measurement
- [x] Terminal panel steals clicks from explorer tree — column guard on terminal click handler
- [x] Live grep scroll wheel changes file instead of scrolling preview — column-based pane routing + `picker_preview_scroll`
- [x] Terminal Ctrl+V paste broken — added lowercase 'v' match (TUI) + Ctrl+V handler (GTK)
- [x] Terminal draws on top of fuzzy finder (TUI+GTK) — moved popups to end of draw order
- [x] GTK visual select highlights wrong line — `line_to_view` HashMap + wrap-aware line-mode highlight
- [x] Right-click in terminal shows editor context menu — terminal bounds check before fallthrough

## Roadmap
- [x] **Spell checker** — Vim-compatible `]s`/`[s`/`z=`/`zg`/`zw`; spellbook Hunspell parser; bundled en_US dictionary; tree-sitter-aware; `spell`/`spelllang` settings; user dictionary at `~/.config/vimcode/user.dic`
- [x] Add the ability to resize tabgroups: i.e. the leftgroup gets bigger at the right's expense and vice-versa. Also should work for vertically stacked tab groups, as well as with both the mouse and via key combos that are remappable in both tui and gui mode.
- [x] Implement version querying so that the semantic version and build number can be displaced via Help:About and by typing "vcd --version" which shows the version but doesn't start the editor.
### Git
- [x] **Stage hunks** — `]c`/`[c` navigation, `gs`/`:Ghs` to stage hunk under cursor
- [x] **Inline diff peek** — `gD`/`:DiffPeek`/gutter click shows hunk popup with Revert/Stage, deleted-line `▾` gutter marker, `]c`/`[c` on real files
- [x] **Git side panel polish** — Multi-line commit messages (Enter inserts newline, commit input box grows in height like VSCode); GTK panel spacing (1.4× line_height like extensions panel); SSH passphrase dialog (git pull/push/fetch currently leak SSH passphrase prompt to parent terminal in GTK or corrupt TUI display — pipe stdin, detect passphrase prompt, show modal dialog; pass `GIT_SSH_COMMAND="ssh -o BatchMode=yes"` or use `SSH_ASKPASS` with a helper that communicates back to the editor)

### Editor Features
- [x] **`:set` command** — runtime setting changes; write-through to settings.json; number/rnu/expandtab/tabstop/shiftwidth/autoindent/incsearch; query with `?`
- [x] **`ip`/`ap` paragraph text objects** — inner/around paragraph (contiguous non-blank lines)
- [x] **`is`/`as` sentence text objects** — inner/around sentence (`.`/`!`/`?`-delimited)
- [x] **Enhanced project search** — regex/case/whole-word toggles; `.gitignore`-aware via `ignore` crate; 10k result cap; GTK toggle buttons + TUI Alt+C/W/R
- [x] **VSCode-style replace across files** — replace all matches in project; skip dirty buffers; reload open buffers; regex capture group backreferences
- [x] **`:grep` / `:vimgrep`** — project-wide search, populate quickfix list
- [x] **Quickfix window** — `:copen`, `:cn`, `:cp` navigation
- [x] **`it`/`at` tag text objects** — inner/around HTML/XML tag
- [x] **Vim-style ex command abbreviations** — `normalize_ex_command()` with 57-entry abbreviation table; ~20 new ex commands (`:join`, `:yank`, `:put`, `:mark`, `:retab`, `:cquit`, `:saveas`, `:windo`/`:bufdo`/`:tabdo`, etc.)

### Big Features
- [x] **LSP support** — completions, go-to-definition, hover, diagnostics (session 47 + 48 bug fixes)
- [x] **`gd` / `gD`** — go-to-definition via LSP
- [x] **`:norm`** — execute normal command on a range of lines
- [x] **Fuzzy finder / Telescope-style** — Ctrl-P opens centered file-picker modal with subsequence scoring (session 53)
- [x] **Multiple cursors** — `Alt-D` (configurable) adds cursor at next match of word under cursor; all cursors receive identical keystrokes; Escape collapses to one
- [x] **Themes / plugin system** — named color themes selectable via `:colorscheme`; 4 built-in themes + VSCode `.json` theme import from `~/.config/vimcode/themes/` (sessions 116, 145)
- [x] **LSP semantic tokens** — `textDocument/semanticTokens/full` overlay on tree-sitter; 8 semantic theme colors; binary-search span overlay; legend caching (sessions 131–132)

### Extensions (vimcode-ext)
- [x] **Lua extension** — `lua-language-server` with Linux (GitHub releases) + macOS (Homebrew) install commands; comment style config
- [x] **Markdown extension** — `marksman` with Linux (GitHub releases) + macOS (Homebrew) install commands
- [x] **OS-specific install commands for all extensions** — backfill `install_linux`/`install_macos`/`install_windows` on existing extensions (cpp, bicep, terraform, xml, latex); cross-platform tools (npm, cargo, go, gem, dotnet) use generic `install` field

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
- [x] **Status line + command line above terminal pane** — `status_line_above_terminal` setting (default `true`). When enabled and the terminal panel is open, the active window's status line and the command line (`:`, `/`, `?`) render as dedicated rows above the terminal panel. When disabled, status and command line sit below the terminal at the screen bottom. The command line always sits directly below the status line. GTK, TUI, and Win-GUI backends.
- [ ] **Terminal maximize (full editor coverage)** — Toggle the terminal panel to cover the entire editor area (like VSCode's "Maximize Panel Size" / `Ctrl+`\` behavior). When maximized, the terminal fills the space normally occupied by editor panes; sidebar remains visible. A second toggle restores the previous layout. Keyboard shortcut + toolbar button. Both GTK and TUI backends.

### Editor Groups
- [x] **Drag tab between editor groups** — drag a tab from one group's tab bar and drop it onto another group's tab bar to move the buffer; visual drop indicator (highlight bar between tabs or at group edge); dropping on the editor area creates a new split; GTK `DragSource`/`DropTarget` + TUI mouse drag tracking

### Extensions (Planned)
- [x] **Unified Picker (Telescope-style)** — core Rust-native unified picker replacing separate fuzzy/grep/palette modals; `PickerSource`/`PickerItem`/`PickerAction` types; `.gitignore`-aware file walking; fuzzy match highlighting; `<leader>sf`/`sg`/`sp` bindings; remappable via `panel_keys`; Phases 1-2 complete (files, grep, commands). Remaining: Phase 3 (buffers, marks, registers, branches), Phase 4 (Lua `vimcode.picker.show()` API)
- [x] **Fuzzy grep word under cursor (`<leader>sw`)** — `<leader>sw` opens the unified picker in live grep mode pre-filled with the word under the cursor (like Telescope's `grep_string`). Useful for quickly finding all usages of an identifier without manually typing it. Remappable via `panel_keys`. Also available as `:GrepWord` ex command and "Search: Word Under Cursor" palette entry.

### UI & Menus
- [x] **VSCode-style menus** — application menu bar (File / Edit / View / Go / Run / Terminal / Help) in GTK; command palette (`Ctrl-Shift-P`) lists all commands + key bindings; fuzzy-searchable; both GTK native menus and TUI pop-up menu overlay (sessions 81–82, 100–101)
- [x] **Command palette** — `Ctrl-Shift-P` floating modal; lists named commands with descriptions and current keybindings; typing filters; Enter executes; shared GTK + TUI (session 101)
- [x] **Settings editor** — `:Settings` opens `settings.json` in an editor tab; Settings sidebar panel shows live values; auto-reload on save in both backends (session 117)
- [x] **Settings sidebar (GTK + TUI)** — interactive form with 32 settings in 8 categories, search, live controls; GTK native widgets (session 117b/117c), TUI interactive form with keyboard nav + inline editing (session 147)

### Extension System
- [x] **Extension mechanism** — Lua 5.4 plugin sandbox; plugins register commands/keymaps/hooks, read/write buffer text, show messages; `~/.config/vimcode/plugins/` auto-loaded; bundled language-pack extensions + GitHub registry; `:ExtInstall/:ExtList/:ExtEnable/:ExtDisable/:ExtRemove` (sessions 98, 113–114)
- [x] **Keymap editor in settings panel** — "User Keymaps" row in the Settings sidebar opens a scratch buffer (one keymap per line, format `mode keys :command`). `:w` validates, updates `settings.keymaps`, calls `rebuild_user_keymaps()`. Also accessible via `:Keymaps` command. Tab shows `[Keymaps]`. GTK button + TUI "N defined ▸". 11 tests. (session 154)
- [x] **Extension Panel API** — `vimcode.panel.register/set_items/parse_event` Lua API for custom sidebar panels; `PanelRegistration`/`ExtPanelItem`/`ExtPanelStyle` types; `panel_focus/panel_select/panel_action` events; dynamic activity bar icons; GTK + TUI rendering; git-insights Git Log panel (Branches/Log/Stash) as first consumer; `vimcode.git.branches()`; 17 tests (session 165)

### AI Integration
- [x] **AI assistant panel** — sidebar chat panel; configurable provider (Anthropic Claude, OpenAI, Ollama local); `ai_provider`/`ai_api_key`/`ai_model`/`ai_base_url` in settings; activity bar chat icon opens panel; multi-turn conversation; `:AI <msg>` and `:AiClear` commands (session 118)
- [x] **AI inline completions** — ghost-text completions from AI provider interleaved with LSP ghost text; separate `ai_completions` setting (default false to avoid unexpected API costs); debounced after 500ms idle in insert mode; Tab accepts whole suggestion, `Alt-]`/`Alt-[` cycle through alternatives
- [x] **Swap file crash recovery** — Vim-like swap files (`~/.config/vimcode/swap/`); FNV-1a path hashing; atomic writes (`.tmp` + rename); PID-based stale detection; `[R]ecover/[D]elete/[A]bort` recovery dialog; `:set swapfile`/`:set updatetime=N`; periodic writes via `tick_swap_files()`; cleanup on shutdown

### Context Menus
- [x] **Context menu action polish** — Two-step "Select for Compare" → "Compare with 'file'" diff flow; fixed GTK copy_relative_path and open_side bugs; engine-driven action handling; 8 new tests

### Explorer
- [x] **Drag-and-drop file/folder move** — Drag files and folders in the explorer tree to move them to a new location. Should work in both TUI (mouse drag) and GTK (native DnD) backends. Visual feedback during drag (insertion indicator, highlight target folder). Confirmation dialog with clickable buttons.
- [x] **Inline rename in explorer** — Rename files/folders directly in the explorer tree (as close to in-place editing as possible), rather than via a separate prompt. In GTK this can be a native editable cell; in TUI, render an input field overlaid on the tree row.
- [x] **Copy/paste files in explorer** — "Copy" and "Paste" items in the right-click context menu. Copy stores the source path; Paste into a different folder duplicates with the same name, Paste into the same folder prompts for a new name (inline in the tree). Support both single files and folders (recursive copy).
- [x] **Context menu popup clamping** — TUI context menu popup rendering moved after status/command line in draw order so popups are no longer painted over by lower UI elements. Position clamping ensures popup stays within terminal bounds.

### Editor
- [x] **VSCode-style editor right-click context menu** — Full 9-item editor right-click: Go to Definition (`gd`), Go to References (`gr`), Rename Symbol (`<leader>rn`), Open Changes (`gD`), Cut, Copy, Paste, Open to the Side (vsplit), Command Palette (`Ctrl+Shift+P`). LSP items disabled without active server; Cut/Copy disabled without visual selection. Both GTK and TUI backends.

### Robustness
- [x] **Centralize context menu definitions** — GTK backend hardcodes its own `gio::Menu` items separately from the engine's `open_explorer_context_menu()` / `open_tab_context_menu()`. This causes drift (e.g. "Open in Integrated Terminal" was missing from GTK). GTK should read from the engine's `ContextMenuState.items` to build its native menus, so new items only need to be added once in the engine.
- [x] **Subprocess stderr safety audit** — Audit all `Command::spawn()` calls across the codebase to ensure stdout/stderr are redirected (null or piped). Unguarded spawns let child process output corrupt the TUI display. Fixed `xdg-open`/`open` calls; need to verify LSP, DAP, git, terminal, and any other subprocess spawns are safe.

### Extension Panel System
- [x] **GTK extension panel rendering** — Implement `draw_ext_panel()` in GTK backend matching TUI's `render_ext_panel()`; wire `SidebarPanel::ExtPanel(String)` into panel switching logic; add dynamic activity bar icons for registered panels; keyboard routing via engine focus flags; mouse click handling with accumulator walk. **This is the critical fix for git-insights "GIT LOG" panel not appearing in GUI mode.**
- [x] **Rich panel layout API** — Extend `ExtPanelItem` and Lua `vimcode.panel` API with richer layout primitives:
  - Tree items: `children`/`expandable`/`expanded` fields for nested expand/collapse nodes
  - Action buttons: `actions: Vec<{label, key}>` rendered as clickable badges on items
  - Input fields: `vimcode.panel.add_input(panel, section, opts)` for inline search/filter
  - Separator/divider items and badge/tag styling (colored pills for branch status, etc.)
- [x] **Panel event enhancements** — Richer event callbacks beyond `panel_select`/`panel_action`: `panel_expand`/`panel_collapse` for tree nodes, `panel_input` for text field changes, `panel_double_click`, `panel_context_menu`
- [x] **Hover popups with rendered markdown** — Mouse-hover popups on panel items (and available to core engine code) that display markdown content rendered with proper formatting (headings, bold/italic, code blocks, lists) and clickable links. Both GTK and TUI backends. Reuse existing markdown rendering infrastructure (already used for editor tab markdown preview). Core API: engine-level `HoverPopup { target_rect, markdown_content, links }` state + render structs. Lua API: `vimcode.panel.set_hover(panel, item_id, markdown_string)` to define hover content per item. Hover triggers on mouse dwell (~300ms), dismisses on mouse-out. Links clickable in GTK (native URI handler) and TUI (`xdg-open`/`open`).
- [x] **Native git panel hover popups** — Use the hover popup system in the built-in Source Control panel to show rich commit info on log items (author, date, full message, changed files summary) — similar to VSCode's SCM hover cards. Also useful for showing diff stats on changed files, branch details (ahead/behind, tracking remote), and stash contents preview.
- [x] **Keyboard-driven hover popups** — Open hover popup for the selected panel item via Enter (or a dedicated key like `K`); popup takes focus until Escape is pressed; Tab/Shift-Tab cycles through links in the popup; Enter on a focused link opens it; arrow keys still scroll the popup content if it overflows. Both GTK and TUI backends.

### Hover Popups
- [x] **Selectable/copyable popup text** — Allow selecting and copying text in hover popups (editor hover + panel hover) via mouse drag or keyboard. Currently popup text is read-only with no selection support.

### LSP
- [x] **Code action gutter indicator** — Lightbulb gutter indicator on lines where LSP code actions are available (like VSCode). Proactive request on cursor settle (150ms debounce). `<leader>ca` / `:CodeAction` / gutter click opens vertical selection dialog; j/k navigate, Enter applies the selected action's workspace edit.

### Explorer
- [x] **Explorer tree indicators** — Right-aligned git status (`M`/`A`/`?`/`D`/`R`) and deduplicated LSP diagnostic counts (errors/warnings) on explorer tree rows (like VSCode); per-extension `ignore_error_sources` config; `9+` cap. Both GTK and TUI backends.
- [x] **Inline new file/folder in explorer tree** — New File and New Folder should create an empty inline editable entry in the explorer tree (inserted under the selected/target directory) rather than prompting for the name in the status line (TUI) or a modal dialog (GTK). The entry uses the same inline editing pattern as rename (`ExplorerRenameState`-style). In GTK mode, both the new entry input and rename input should display with a visible bordered box around the text field. In TUI mode, the existing inverted-cursor inline style is sufficient. On Enter, create the file/folder; on Escape, cancel. File icon should show a generic new-file/new-folder icon during input.
- [x] **Replace status-line confirmations with modal dialogs** — Already complete: all y/n confirmations (file move/delete, extension removal, swap recovery, SSH passphrase, code actions) use the engine's `show_dialog()` / `show_error_dialog()` system. `PromptKind` was removed in a prior session. No status-line prompts remain.

### Refactoring
- [x] **Split main.rs into gtk/ directory** — `src/main.rs` (16,826 lines) → `src/gtk/` directory with 6 submodules: `mod.rs` (9,267 — App, Msg, SimpleComponent impl), `draw.rs` (5,519 — all 32 draw_* functions), `click.rs` (575 — mouse click/drag), `css.rs` (525 — theme CSS), `util.rs` (468 — GTK utilities), `tree.rs` (432 — file tree). Thin `main.rs` (55 lines) dispatches to `gtk::run()` or `tui_main::run()`. Zero API changes, all 4,721 tests pass.
- [x] **Split tui_main.rs into tui_main/ directory** — `src/tui_main.rs` (14,190 lines) → `src/tui_main/` directory with 4 submodules: `mod.rs` (4,166 — structs, event_loop, setup), `panels.rs` (3,931 — sidebar panel rendering), `render_impl.rs` (3,736 — draw_frame, editor rendering, popups), `mouse.rs` (2,379 — handle_mouse). All files under 5K lines.
- [x] **Refactor App::update() message handler** — Extracted the ~4,495-line monolithic `update()` match into a ~430-line dispatcher calling 19 helper methods on `impl App` (`handle_key_press`, `handle_poll_tick`, `handle_mouse_*`, `handle_terminal_msg`, `handle_menu_msg`, `handle_*_sidebar_msg`, `handle_explorer_msg`, `handle_find_replace_msg`, `handle_file_ops_msg`, `handle_dialog_msg`). Added `terminal_cols()` utility. All 4,721 tests pass.

### UI & Menus
- [x] **Hide tab bar when single tab** — `hide_single_tab` setting (default `false`); when enabled, the tab bar row is hidden if the active editor group has only one tab, reclaiming the row for editor content. Applies to both GTK and TUI backends. Gives a more traditional Vim feel by removing chrome when there's nothing to switch between. Tab bar reappears automatically when a second tab is opened.

### Testing & Backend Parity
- [x] **Phase 2b: Backend rendering parity harness** — `UiElement` enum (27 variants) + `collect_expected_ui_elements()` source of truth + per-backend collectors (`collect_ui_elements_tui`, `collect_ui_elements_wingui`). 7 parity tests assert every `ScreenLayout` field has a corresponding draw call in each backend. Caught and drove fixes for 6 Win-GUI rendering gaps (editor hover, diff peek, debug toolbar, diff toolbar, tab tooltip, panel hover).
- [x] **Phase 2c: Click/mouse handling parity harness** — `UiAction` enum (26 variants) + `all_required_ui_actions()` source of truth + per-backend collectors (`collect_ui_actions_tui`, `collect_ui_actions_wingui`). 3 parity tests assert every required action is handled by both backends. Behavioral contract tests verify `open_file_preview` creates preview tabs (not replacing permanent), `open_file_in_tab` creates new tabs, and `default_shell()` returns platform-appropriate shell. Caught and drove fixes for: wrong explorer open method (switch_window_buffer vs open_file_preview/open_file_in_tab), missing context menu click handler, wrong draw order (context menu behind sidebar), /bin/bash on Windows.
- [x] **Win-GUI: mouse handlers for new popups** — Added click/dismiss/scroll handlers for editor hover (click-to-focus, dismiss, scroll), panel hover (dismiss), debug toolbar (button click → execute_command). Context menu click-inside/click-outside with item selection and action dispatch. Cached popup bounding rectangles (`CachedPopupRects`) populated each frame for hit-testing.
- [ ] **Phase 2d: Behavioral backend parity tests (Option B)** — End-to-end tests that simulate user interaction sequences and assert engine state is identical regardless of which backend drives it. For each interaction (explorer single-click, double-click, context menu action, tab drag-drop, terminal split, etc.), set up engine state → call the same engine methods the backend would call → assert final state (tabs, buffers, focus, modes). Catches method-call sequence bugs that static parity can't (e.g., missing `sidebar.dirty = true` after tree mutation, missing `InvalidateRect` after state change). Build on the `UiAction` contract from Phase 2c.
- [x] **Win-GUI critical bug fixes (from GTK comparison)** — All 4 critical + all 5 medium bugs fixed: (1) tab close dirty check via engine dialog, (2) picker mouse interaction (click-to-select, click-outside-dismiss, scroll navigation), (3) dialog button click handling with rect hit-testing, (4) QuitWithUnsaved shows confirmation dialog + WM_CLOSE handler, (5) fold-aware scrolling via `scroll_down_visible`/`scroll_up_visible`, (6) picker scroll interception, (7) VSCode selection clear on click, (8) cursor kept in viewport after scroll with scrolloff, (9) terminal tab switching by mouse.
- [ ] **Win-GUI missing features (from GTK comparison)** — 1 low-priority feature remaining: horizontal scrollbar drag. (Breadcrumb clicks, group divider drag, diff toolbar buttons, diff peek keys, tab tooltip dismiss all fixed in Session 269.)
- [ ] **Phase 2c source-code verification** — Extend `collect_ui_actions_wingui()` to grep Win-GUI source for the required engine method calls (`open_file_preview`, `open_file_in_tab`, `context_menu_confirm`, `close_context_menu`, etc.) instead of relying on a hand-curated list. Fail the test if the source doesn't contain the expected call. Turns the parity check from a checklist into an automated bug-finder.

### Robustness (Low Priority)
- [x] **Consolidate sidebar focus state into engine** — `explorer_has_focus`/`search_has_focus` on Engine struct; `sidebar_has_focus()` aggregator + `clear_sidebar_focus()` helper; `handle_key()` guards; TUI `sync_sidebar_focus()` keeps state consistent; GTK sync on focus toggle/editor focus; 8 tests verify key routing correctness

### Tab Navigation & Command Center
- [x] **Tab scroll-into-view** — When a tab is opened, switched to via explorer click, or navigated to via history arrows, scroll the tab bar so the active tab appears in the center of the visible tab strip (or as close to center as possible given the tab count). Currently new tabs appear at the end and may be off-screen in the tab bar when many tabs are open. Applies to both GTK and TUI backends, per editor group.
- [x] **Back/Forward navigation arrows** — Add `←` `→` arrow buttons in the menu bar area (between the menu items and the Command Center, matching VSCode's layout). Maintain a per-editor-group **tab access history** stack (`Vec<(GroupId, TabId)>` on Engine) that records every tab focus change. `←` navigates to the previously accessed tab; `→` moves forward through the history after going back. Keyboard shortcuts: `Ctrl-Alt-Left` / `Ctrl-Alt-Right` (remappable via `panel_keys`). The arrows should be clickable in both GTK (drawn in the menu bar row) and TUI (rendered as `◀ ▶` buttons in the menu/tab bar row). History should be bounded (e.g. 100 entries) and deduplicated (consecutive duplicates collapsed). This is distinct from the existing Vim jump list (`Ctrl-O`/`Ctrl-I`), which tracks cursor positions within files rather than tab switches.
- [x] **Menu bar MRU history arrows** — Add `◀ ▶` arrow buttons in the **menu bar** row (to the left of the Command Center, matching VSCode's layout). These are distinct from the existing per-group tab bar arrows (which cycle L/R within the group). The menu bar arrows navigate a **global MRU tab history** across all editor groups — clicking `◀` jumps back to the previously visited tab (which may be in a different editor group), and `▶` moves forward. This enables quickly jumping between tabs you were working on minutes ago, even across splits. Keyboard shortcuts: configurable via `panel_keys` (e.g. `Ctrl-Alt-Left`/`Ctrl-Alt-Right` or similar, distinct from the per-group tab bar arrow bindings). History: bounded (100 entries), deduplicated (consecutive duplicates collapsed), forward entries truncated on new navigation. Rendered in both GTK (drawn in the menu bar row) and TUI (rendered as `◀ ▶` in the menu bar row). Distinct from the Vim jump list (`Ctrl-O`/`Ctrl-I`), which tracks cursor positions within files.
- [x] **Command Center** — Clickable search box in the menu bar opens the unified picker with prefix-based mode switching: _(none)_ fuzzy files, `>` command palette, `@` document symbols (LSP), `#` workspace symbols (LSP), `:` go to line, `?` help. Both GTK and TUI backends. `:CommandCenter` ex command. 11 tests.

### Command Center Enhancements
- [x] **Search for Text prefix (`%`)** — Add `%` prefix to Command Center that opens live grep mode (same as `Ctrl+G` / `PickerSource::Grep`). When the user types `%` as the first character, the picker switches to live project search — matching VSCode's "Search for Text" Command Center entry. The `?` help menu should list this prefix alongside the others.
- [x] **Start Debugging prefix (`debug`)** — Add `debug` keyword prefix to Command Center. When the user types `debug`, show available launch configurations from `.vimcode/launch.json` (or offer to generate one). Selecting a configuration starts the DAP session (same as F5). If no launch.json exists, show "Create launch.json..." option.
- [x] **Run Task prefix (`task`)** — Add `task` keyword prefix to Command Center. When the user types `task`, list available tasks from `.vimcode/tasks.json` (build, test, lint, etc.). Selecting a task runs it in the integrated terminal. If no tasks.json exists, show "Configure Tasks..." option.
- [x] **Open Quick Chat prefix** — `chat` prefix in Command Center: `chat` alone shows "Open AI Panel" + prompt; `chat <question>` shows "Ask AI: ..." and sends to configured provider on confirm; unconfigured state shows setup guidance and opens Settings. Listed in `?` help and empty-query hints. 5 tests.
- [x] **Command Center placeholder hints** — When the Command Center search box is empty and first opened, show a list of available modes as selectable items (matching VSCode's initial dropdown): "Go to File", "Show and Run Commands >", "Search for Text %", "Go to Symbol in Editor @", "Start Debugging debug", "Run Task task", "More ?". Each item should have its keyboard shortcut shown on the right. Selecting an item sets the corresponding prefix.

### Breadcrumbs & Navigation
- [x] **Breadcrumb symbol navigation** — Breadcrumb segments are now clickable in both GTK and TUI backends. Clicking a directory segment opens the file picker filtered to that directory; clicking a symbol segment opens the `@` picker scoped to siblings at the same level (filtered by LSP `container` field). `<leader>b` enters breadcrumb focus mode (h/l navigate, Enter opens scoped picker, Escape exits). `<leader>so` opens document outline. `BreadcrumbSegment` gains `index`, `path_prefix`, `symbol_line` fields; `BreadcrumbSegmentInfo` engine-side struct with `parent_scope`; `breadcrumb_open_scoped()` sets `breadcrumb_scoped_parent` for LSP filtering. Picker click-through + scroll wheel interception in both backends. 12 tests.
- [x] **Tree-style symbol drill-down in breadcrumb picker** — The `@` symbol picker shows an expandable tree matching VSCode's Outline view. Added `hierarchicalDocumentSymbolSupport` LSP capability; `parse_document_symbols_hierarchical` preserves `DocumentSymbol` children; `rebuild_tree_from_containers` reconstructs hierarchy from flat `SymbolInformation` `container` fields. `PickerItem` gains `depth`/`expandable`/`expanded` fields; `build_symbol_tree_items()` sorts by `SymbolKind::sort_order()` then alphabetically. Top-level containers start expanded; Enter/Right expands, Enter/Left collapses; click-to-toggle-expand in GTK+TUI; typing flattens to fuzzy. `▼`/`▷` arrows + indentation in both backends. Picker jumps center the viewport. 14 new tests.

### Status Bar Enhancements
- [x] **Git branch switcher in status bar** — Make the git branch name in the status bar clickable. Clicking opens the unified picker in `PickerSource::GitBranches` mode to switch branches. Show ahead/behind counts next to the branch name. Both GTK (click handler on status bar DA) and TUI (mouse click detection on status bar row).
- [x] **Per-window status lines (Vim-style)** — Each window gets its own status line at its bottom edge, replacing the global status bar (Vim behavior). Active window: bold mode name (colored text tint for Insert/Visual/Replace), filename, dirty flag, git branch, filetype, encoding, cursor position. Inactive windows: dimmed filename + cursor. Colors derived from `theme.background.lighten(0.10)` — no hardcoded hex. `window_status_line` setting (default on); `:set nowindowstatusline` reverts to global bar. `StatusSegment`/`WindowStatusLine` structs in render.rs; `build_window_status_line()` builder; both GTK + TUI backends; horizontal separator suppression; per-window click handling. 6 new tests.
- [x] **Clickable status bar segments** — All status bar segments are interactive in both GTK and TUI. Click Ln:Col → Command Center go-to-line; click filetype → Language Mode picker (37 languages); click indent → Indentation picker (Spaces: 2/4/8, Tabs: 2/4/8); click LF/CRLF → Line Ending picker; click utf-8 → info message; click git branch → Branch picker. `StatusAction` enum on `StatusSegment`; `handle_status_action()` on Engine; `PickerSource::Languages/Indentation/LineEndings`; `LineEnding` enum with detection/conversion on `BufferState`; `SyntaxLanguage::from_language_id()`; segment hit-testing in both backends. 4 new tests.
- [x] **LSP status indicator** — Persistent LSP status in per-window status bar: server binary name shown (e.g. `rust-analyzer`); `…` suffix while indexing (initializing/no semantic tokens yet); `✗` on crash; hidden when no LSP for filetype. Readiness based on `BufferState.semantic_tokens` presence (aligns with hover/definition availability). `LspStatus` enum on `LspManager` with `server_has_responded` tracking; `lsp_status_for_buffer()` on Engine; `StatusAction::LspInfo` click opens `:LspInfo`. Both GTK and TUI.

### Tab Bar Enhancements
- [x] **Editor action menu (`...`) button** — `…` button at right edge of each tab bar group; dropdown with Close All, Close Others, Close Saved, Close Tabs to Right/Left, Toggle Word Wrap, Change Language Mode, Reveal in File Explorer. `ContextMenuTarget::EditorActionMenu` + `open_editor_action_menu()` + `close_all_tabs()`; TUI `TAB_ACTION_BTN_COLS` reserved + click handling; GTK Pango-measured button + `ActionBtnMap` + `PopoverMenu` popover. 5 new tests.

### Layout & Chrome
- [x] **Layout toggle buttons** — Clickable nerd-font icon segments (󰘖/󰆍/󰍜 with `[S]`/`[P]`/`[M]` ASCII fallbacks) in per-window status bar (right side, after Ln:Col) toggle sidebar and terminal panel visibility; dim when inactive. Menu bar toggle only shown in TUI (`menu_bar_toggleable` field). GTK click detection uses Pango-measured `StatusSegmentMap` cache instead of `char_width` approximation. 4 new tests.
- [x] **Notification / progress indicator** — Spinner (⠋⠙⠹…) in per-window status bar during background operations (LSP install, project search/replace); bell icon (󰂞/*) for completed; auto-dismiss after 5s; click bell to clear; `NotificationKind` enum + `Notification` struct on Engine; `notify()`/`notify_done_by_kind()`/`tick_notifications()` lifecycle; both GTK + TUI backends (animated via poll tick). 9 tests.

### Editor Features
- [x] **Richer tree-sitter highlight queries** — Expanded all 20 language grammars with comprehensive highlight queries. 11 new Theme fields (`operator`, `punctuation`, `macro_call`, `attribute`, `lifetime`, `constant`, `escape`, `boolean`, `property`, `parameter`, `module`) with colors for all 6 built-in themes + VSCode JSON importer. `scope_color()` now handles 22 capture names (was 8). Rust: macros, attributes, lifetimes, field access, method calls, numbers, booleans, operators, punctuation, escape sequences, 30+ keywords. Python: decorators, method calls, parameters, operators, booleans, numbers. JS/TS: method calls, properties, parameters, regex, operators, punctuation. Go: method calls, field access, package names, parameters, operators. C/C++: function calls, preprocessor macros, field access, operators, punctuation. Java: annotations, method calls, field access, operators. Ruby: method calls, symbols, operators. C#: operators, punctuation, booleans→boolean, nulls→constant. JSON/TOML/YAML: keys→property, booleans→boolean, nulls→constant, punctuation. Bash: command calls, variables, operators. Lua: method calls, properties, operators. HTML: attributes, punctuation. CSS: property names, color/number values, punctuation.
- [x] **Externalize highlight queries into extensions** — `highlights: Option<String>` field on `ExtensionManifest`; `highlight_overrides: HashMap<String, String>` on Engine populated from installed extensions at init; `Syntax::new_for_language_with_query()` / `new_from_path_with_overrides()` / `new_from_language_id_with_overrides()` accept override queries with fallback to built-in; `populate_highlight_overrides()` re-applies to open buffers; `language_id()` method on `SyntaxLanguage` for reverse lookup. Built-in queries remain as compile-time fallbacks. 4 new tests.
- [ ] **Minimap** — Code overview minimap on the right edge of each editor pane, showing a scaled-down rendering of the entire file with the viewport highlighted. Click/drag to scroll. Syntax-highlighted. Toggleable via `:set minimap` / settings. Both GTK (Cairo scaled rendering) and TUI (braille/block character approximation).

### Debugger
- [ ] **Debug target selection on startup** — When the configured debug target binary is not found at the default location, prompt the user to select the correct binary instead of silently failing. Explore what VSCode does: it shows a file picker or lets the user edit `launch.json` `program` field. Could show a file picker filtered to executables, or a dialog with the failed path and an option to browse. Should also handle missing `launch.json` gracefully.

### Explorer
- [x] **Remove explorer toolbar** — Remove the New File / New Folder / Refresh / Collapse All toolbar buttons at the top of the explorer panel. These are redundant now that right-click context menus provide the same actions. Reclaims a row of vertical space.
- [x] **Right-click in empty explorer space** — Right-clicking in the blank area below the last tree entry should open the context menu as if the root folder was clicked (New File, New Folder, etc.). Currently only tree rows are clickable.
- [x] **Inline rename improvements** — When renaming a file in the explorer: (1) the filename portion (without extension) should be pre-selected so Backspace removes just the name, leaving the extension; (2) if the name is longer than the available width, the input should scroll horizontally (like VSCode); (3) Ctrl-C/Ctrl-V should work in the rename input field.
- [x] **Copy filename during inline rename** — Ctrl-C and Ctrl-V should work in the inline rename text input in the explorer panel, allowing the user to copy the current filename before editing it. Currently these keys may not be handled in the rename input field.
- [ ] **GTK explorer indent guide lines** — TUI has vertical `│` indent guides; GTK TreeView doesn't support vertical-only guides (built-in `enable_tree_lines` draws horizontal connectors too). Needs custom rendering — either a cell data function with guide characters or Cairo custom drawing in a separate column.

### Picker / Fuzzy Finder
- [x] **Search history in picker dialogs** — Up arrow at top of results recalls previous searches from the current session. Per-source history stack (`picker_history: HashMap<PickerSource, Vec<String>>`); Down navigates forward or restores the in-progress query; typing/backspace/paste exits history mode; consecutive duplicates deduplicated; capped at 100 entries; session-scoped (not persisted). 7 new tests.
- [ ] **Full keyboard navigation in picker** — Drive the fuzzy finder entirely by keyboard: Ctrl+J/K or arrow keys to scroll results, Ctrl+U/D for page up/down in results, Ctrl+F/B or arrow keys in preview pane to scroll the preview, Tab to toggle focus between results and preview panes. Currently the picker only supports Up/Down for single-step selection; there is no way to page through results or scroll the preview without a mouse.

### CI & Distribution
- [x] **macOS builds via GitHub Actions + Homebrew tap** — Add a macOS build target to the GitHub Actions CI/release workflow (build on `macos-latest` with `cargo build --release`). Produce a universal or arch-specific binary artifact. Create a Homebrew tap repository (e.g. `homebrew-vimcode`) with a formula that installs the release binary. Ensure the release workflow updates the tap formula (SHA256 + version) on each release. Test the full `brew install` → launch cycle in CI.
- [x] **Windows TUI builds** — Windows CI job (`windows-latest`) builds `vcd.exe` with `--no-default-features` (no GTK). Release workflow produces `vcd-windows-x86_64.exe` artifact. Windows-specific: `CREATE_NEW_PROCESS_GROUP` for LSP/DAP, powershell clipboard, `cmd /c start` URL opener, `tasklist` PID check, tree-sitter-latex static link, 8 test fixes. GTK GUI build not feasible on Windows (no static linking support).
- [ ] **Windows code signing** — Investigate Authenticode signing for `vcd-windows-x86_64.exe` to avoid SmartScreen warnings on corporate machines.

### Website & SEO (vimcode.org)
- [x] **Project website** — Static landing page on GitHub Pages (`JDonaghy/vimcode-website`), OneDark-derived dark theme, responsive, SEO meta tags + JSON-LD structured data + sitemap. Custom domain `vimcode.org` with HTTPS.
- [ ] **Screenshots & OG image** — Add real editor screenshots to the landing page (replace placeholder). Create a 1200x630px Open Graph image for social media previews; add `og:image` and `twitter:image` meta tags.
- [ ] **Google Search Console** — Add `https://vimcode.org/` as a property, verify with HTML meta tag, submit `sitemap.xml`. Use URL Inspection to request initial indexing.
- [ ] **Bing Webmaster Tools** — Add site at https://www.bing.com/webmasters/, verify, submit sitemap (or import from Google Search Console).
- [ ] **Submit to awesome-rust** — PR to https://github.com/rust-unofficial/awesome-rust adding VimCode under "Text editors".
- [ ] **Submit to alternativeto.com** — List VimCode as an alternative to VS Code, Neovim, Helix, Zed.
- [ ] **Announce on Reddit & Hacker News** — "Show HN: VimCode — Vim+VSCode hybrid editor in Rust"; post to r/rust, r/vim, r/neovim. Wait until the editor feels stable enough for first impressions.
- [ ] **Publish on Flathub** — Submit the Flatpak to Flathub for discoverability and a trusted backlink.
- [ ] **Additional website pages** — Getting Started guide, screenshots gallery, extension registry browser. Add each to sitemap.

### Native Platform GUIs
- [x] **Native Windows GUI (Direct2D + DirectWrite)** — Native Win32 backend using `windows-rs` + Direct2D + DirectWrite. Produces `vimcode-win.exe` via `win-gui` Cargo feature. `src/win_gui/` directory (~4.5K lines) consuming `ScreenLayout` from `render.rs` — zero changes to core engine. Phase 1: HWND + D2D + DWrite + keyboard → editor pane with syntax. Phase 2: Tab bar, mouse, popups, clipboard. Phase 3: Sidebar, terminal, menu bar, scrollbar, breadcrumbs, DPI awareness. Phase 4: Custom frameless title bar (DWM + NCCALCSIZE + NCHITTEST, caption buttons), native file dialogs (IFileOpenDialog/IFileSaveDialog via COM), IME composition, file watching (notify crate), Segoe UI proportional font for menus/tabs, dynamic window title, accessibility (MSAA via window title).
- [ ] **Native macOS GUI (Core Graphics + Core Text)** — Future: native macOS backend using `objc2` + Core Graphics rendering + Core Text. Would produce a `.app` bundle. Same `ScreenLayout` consumption pattern as Windows backend. Core Text provides native font rendering with proper Retina support. Depends on Windows backend proving the multi-backend pattern works.
- [x] **Multi-backend prep work** — Extracted shared hit-testing geometry, key binding matching, and scrollbar helpers from GTK/TUI/Win-GUI into `render.rs`. `ClickTarget` enum now public in render.rs. 8 shared helper functions. `Color::to_f32_rgba()` for D2D/CoreGraphics. `view_row_to_buf_line()`/`view_row_to_buf_pos_wrap()` in render.rs. `open_url_in_browser()` in core/engine. `notify` crate for cross-platform file watching (replaces GTK's gio::FileMonitor).

### Remote Editing
- [ ] **Remote editing over SSH** — Research and design a remote editing story for VimCode. Key questions: (1) **Neovim's approach** — Neovim supports `--remote`, `--server`, and `--headless` modes with a msgpack RPC API; clients connect over stdio/TCP/Unix socket; how much of this is worth emulating? (2) **SSH tunneling** — can VimCode run headless on a remote host with the TUI/GTK frontend on the local machine, forwarding over SSH (à la VS Code Remote SSH)? What's the protocol between frontend and engine? (3) **Headless VimCode** — a `vcd --headless` mode that exposes the engine over a socket/pipe for scripting, testing, or remote frontends; what API surface is needed? (4) **sshfs / FUSE alternative** — simpler approach: open remote files via sshfs mount; what are the LSP/git/terminal implications? (5) **Latency** — how to handle input latency, optimistic rendering, reconnection. Study Neovim's `--headless` + `nvim --listen`, VS Code Remote SSH extension architecture, and Mosh's approach to latency compensation.

### Documentation
- [x] **GitHub Wiki** — 9 pages: Home, Getting Started, Key Remapping, Settings Reference, Extension Development, Lua Plugin API, Theme Customization, DAP Debugger Setup, LSP Configuration; README Documentation section links to wiki
- [x] **README revamp** — Full review and rewrite: updated intro/status (removed alpha hedging), added Platforms table (Linux/macOS/Windows), added Windows native GUI + TUI download instructions, added Windows build commands, updated Architecture tree with win_gui/ directory and all current line counts (~128K total), updated Tech Stack with windows-rs/Direct2D/DirectWrite/notify, added LaTeX to syntax list + semantic token overlay mention, updated test count to 5,391, removed duplicate command table entries, updated Acknowledgements.
