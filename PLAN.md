# VimCode Implementation Plan

> Session history archived in **SESSION_HISTORY.md**. Recent work summary in **PROJECT_STATE.md**.

---

## Recently Completed
- **Session 217**: Engine Split Refactor — split monolithic `engine.rs` (51,825 lines) into `src/core/engine/` directory with 20 submodule files; `mod.rs` (3,334 lines) + largest submodules: `tests.rs` (14,334), `keys.rs` (7,056), `motions.rs` (4,628); all 4,721 tests pass; zero public API changes.
- **Session 216**: Explorer Tree Indicators — git status + deduplicated LSP diagnostic counts on explorer rows; `explorer_indicators()` engine method with per-extension `ignore_error_sources` config; `Diagnostic.code` field; `relatedInformation: true`; GTK TreeStore 6-column + `update_tree_indicators()`; TUI render-time indicator drawing; 9+ cap; 3 tests. 4721 tests.
- **Session 215**: Bug Fix Sweep — visual mode `x` delete (alias for `d` with `pending_key.is_none()` guard); Ctrl+V paste in search/command/replace inputs (`handle_search_key`, `handle_command_key`, TUI project search); search highlights in non-active buffers (per-buffer match computation in `build_rendered_window()` via `compute_search_matches_for_buffer()`). No open bugs. 4712 tests.
- **Session 214**: Bug Sweep — TUI hover dismiss click fall-through, TUI selection with wrap, markdown typing color bleed (debounced syntax refresh), GTK scrollbar/divider overlap, TUI fuzzy finder stale chars. 4706 tests.
- **Session 213**: Unified Picker Phase 1+2 + Bug Fixes — unified picker system, GTK crash prevention, markdown highlighting, hover popup fixes. 4706 tests.
- **Session 212**: Selectable/Copyable Hover Popup Text — mouse drag selection, keyboard copy, GTK/TUI highlight rendering. 4710 tests.
- **Session 211**: Code Action Apply + Semantic Token Fix — vertical dialog, workspace edit, proactive requests, semantic token clearing. 4703 tests.

> Sessions 209 and earlier in **SESSION_HISTORY.md**.

### Bug Fixes
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
- [x] Git insights hover on non-cursor lines — clear `editor_hover_content` in `clear_annotations()`
- [x] Semantic tokens disappear after hover — only accept responses with actual `data` array
- [x] Terminal backspace key-hold batching — poll immediately after `terminal_write()`
- [x] Sidebar scrollbar drag leaks — `dragging_generic_sb` state + GTK gesture `set_state(Claimed)`

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

### Editor Groups
- [x] **Drag tab between editor groups** — drag a tab from one group's tab bar and drop it onto another group's tab bar to move the buffer; visual drop indicator (highlight bar between tabs or at group edge); dropping on the editor area creates a new split; GTK `DragSource`/`DropTarget` + TUI mouse drag tracking

### Extensions (Planned)
- [x] **Unified Picker (Telescope-style)** — core Rust-native unified picker replacing separate fuzzy/grep/palette modals; `PickerSource`/`PickerItem`/`PickerAction` types; `.gitignore`-aware file walking; fuzzy match highlighting; `<leader>sf`/`sg`/`sp` bindings; remappable via `panel_keys`; Phases 1-2 complete (files, grep, commands). Remaining: Phase 3 (buffers, marks, registers, branches), Phase 4 (Lua `vimcode.picker.show()` API)

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

### Robustness (Low Priority)
- [ ] **Consolidate sidebar focus state into engine** — TUI's `sidebar.has_focus` is a local variable not accessible to engine tests, making sidebar focus bugs (like the search panel input regression) impossible to catch with unit/integration tests. Move sidebar focus tracking into the engine so key routing correctness can be tested.

### Documentation
- [x] **GitHub Wiki** — 9 pages: Home, Getting Started, Key Remapping, Settings Reference, Extension Development, Lua Plugin API, Theme Customization, DAP Debugger Setup, LSP Configuration; README Documentation section links to wiki
