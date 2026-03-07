# VimCode Project State

**Last updated:** Mar 6, 2026 (Session 137 — Operator+motion completeness) | **Tests:** 2461

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 134 are in **SESSION_HISTORY.md**.

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

**Session 137 — Operator+motion completeness (56 new tests, 2461 total):**
Full operator+motion support for all standard Vim motions:
- **`pending_find_operator`**: New Engine field for `df`/`dt`/`dF`/`dT` (3-keystroke sequences requiring deferred char input)
- **`apply_charwise_operator()`**: Generic helper handling `d`/`c`/`y`/`~`/`u`/`U`/`>`/`<`/`=` on char ranges
- **`apply_linewise_operator()`**: Generic helper for linewise operations on line ranges
- **New motion arms in `handle_operator_motion()`**: `h`/`l` (charwise), `j`/`k` (linewise), `G` (to line), `{`/`}` (paragraph), `(`/`)` (sentence), `W`/`B`/`E` (WORD), `^` (first non-blank), `H`/`M`/`L` (screen), `;`/`,` (repeat find), `f`/`t`/`F`/`T` (deferred find)
- **Operator-aware `gg`/`ge`/`gE`** in `handle_pending_key`: `dgg`, `ygg`, `cgg`, `dge`, `dgE` now work
- **Case/indent/auto-indent operators** extended to support all motions: `g~j`, `guw`, `gUG`, `>j`, `<{`, `=G`, `gufx`, etc.
- **Refactored `apply_operator_with_motion()`** to use `apply_charwise_operator()` — all operators (including case/indent) now work with word motions
- **56 integration tests** in `tests/operator_motions.rs` covering all new combos
- Files changed: `src/core/engine.rs`; updated: `tests/operator_motions.rs`

**Session 136 — Vim-style ex command abbreviations + ~20 new commands (71 new tests, 2405 total):**
Comprehensive ex command normalization system and new Vim commands:
- **`normalize_ex_command()` system**: New `EX_ABBREVS` table with 57 entries mapping abbreviated ex commands to their canonical forms (e.g., `j` -> `join`, `y` -> `yank`, `ve` -> `version`). All `execute_command()` match arms now use canonical forms; the normalizer handles abbreviation resolution before dispatch
- **`:copy` conflict resolution**: The clipboard palette action previously bound to `:copy` was renamed to `clipboard_copy`. Bare `:copy` now shows usage message (`:copy` is Vim's line-copy command, aliased as `:t`)
- **~20 new ex commands**: `:j[oin]`, `:y[ank]`, `:pu[t]`, `:>`, `:<`, `:=`, `:#`, `:ma[rk]`/`:k`, `:pw[d]`, `:f[ile]`, `:ene[w]`, `:up[date]`, `:ve[rsion]`, `:p[rint]`, `:nu[mber]`, `:new`, `:vne[w]`, `:ret[ab]`, `:cq[uit]`, `:sav[eas]`, `:windo`/`:bufdo`/`:tabdo`, `:di[splay]`
- **`QuitWithError` EngineAction**: New variant for `:cquit`, handled in both GTK (`std::process::exit(1)`) and TUI backends
- **71 integration tests** in new `tests/ex_commands.rs` covering all new commands and abbreviation normalization
- Files changed: `src/core/engine.rs`, `src/render.rs`, `src/main.rs`, `src/tui_main.rs`; new: `tests/ex_commands.rs`

**Session 135 — show_hidden_files setting + LSP format undo fix (no new tests, 2346 total):**
New `show_hidden_files` setting and two bug fixes for LSP format-on-save:
- **`show_hidden_files` setting**: New boolean setting (default false) to show/hide dotfiles in the file explorer, fuzzy finder, and folder picker. Accessible via `settings.json`, `:set showhiddenfiles`/`:set shf`, and the Settings sidebar (Workspace category). GTK: `RefreshFileTree` fired on setting change for instant effect. TUI: `sidebar.show_hidden_files` synced from engine settings on periodic refresh and sidebar re-creation.
- **LSP format undo fix**: `apply_lsp_edits()` called `start_undo_group()`/`finish_undo_group()` but never recorded delete/insert operations, so the undo group was always empty. Undo after format-on-save said "already at oldest change". Fixed to call `record_delete()`/`record_insert()` for each edit, and to operate on the target `buffer_id` directly rather than the active buffer.
- **Stale highlighting after format**: After applying LSP formatting edits, the server was never notified via `didChange` and semantic tokens were never re-requested. Fixed by marking the buffer in `lsp_dirty_buffers` after `apply_lsp_edits()` so `lsp_flush_changes()` sends the notification and refreshes tokens.
- Files changed: `settings.rs` (setting field + `:set` support), `render.rs` (SETTING_DEFS entry), `engine.rs` (fuzzy finder filter + `apply_lsp_edits` undo/LSP fix), `main.rs` (GTK explorer filter + tree refresh on setting change), `tui_main.rs` (TUI explorer + folder picker filter)

**Session 134 — search highlight + viewport bug fixes (13 new tests, 2346 total):**
Five bug fixes across search highlighting, rendering, and viewport layout:
- **Search highlights refresh after edits**: `search_matches` (char offsets) were never recalculated when the buffer changed in insert or normal mode. Edits caused stale highlights on wrong characters. Fixed by calling `run_search()` after buffer modifications when `search_matches` is non-empty, in both normal-mode and insert-mode `if changed` blocks
- **Escape clears search highlights**: Pressing Escape in normal mode now clears `search_matches` (like `:noh`), preserving `search_query` so `n`/`N` re-run the search. `search_next()`/`search_prev()` updated to re-run search when matches were cleared but query exists
- **Extra line number in gutter**: `render.rs` used raw `buffer.content.len_lines()` (Ropey) which counts a trailing `\n` as an extra empty line. Changed to `buffer.len_lines()` which already has the correction
- **Markdown preview always wraps**: Preview buffers now force `wrap_on = true` regardless of `:set wrap`/`:set nowrap` setting, and suppress the horizontal scrollbar
- **TUI viewport layout fix**: Tab bar row was double-counted — `draw_frame` split editor column into `tab_area(1)` + `editor_area`, but `calculate_group_window_rects` also reserved 1 row for tab bar via `tab_bar_height=1`. Removed the `ec_chunks` split; `editor_area` is now the full editor column. Window rects' built-in `y=1` offset naturally places code below the tab bar. Changed `content_rows` from `-3` to `-2` (only status+cmd). Also fixed rough `viewport_lines` estimate (-1 for tab bar)
- **GTK per-window viewport sync**: Added per-window viewport sync in SearchPollTick handler (20Hz) from actual window rects, matching what TUI already does. Fixes `G` not scrolling far enough when panels reduce editor height
- **13 integration tests** in new `tests/search_highlight.rs`

**Session 133 — bracket matching: visual mode + y% fix + tests (30 new tests, 2333 total):**
Complete `%` bracket matching implementation:
- **Visual mode `%`**: Added `Some('%')` arm in `handle_visual_key()` so `v%`/`V%` extend selection to matching bracket
- **`y%` bug fix**: `apply_operator_bracket_motion()` always deleted text regardless of operator. Fixed to yank-only for `y` operator (register set, yank highlight shown, no buffer modification). `d%` and `c%` continue to delete/change as before
- **30 integration tests** in new `tests/bracket_matching.rs`: normal mode `%` (forward/backward for all 3 bracket types, nested, cross-line, mixed, forward search, unmatched, empty, edge cases), operator motions (`d%`/`y%`/`c%` with opening/closing brackets, cross-line, nested, no-bracket no-op), visual mode (`v%` select/yank/delete, cross-line, forward search, `V%` linewise)

**Session 132 — LSP session restore + semantic tokens bug fixes (1 new test, 2303 total):**
Three targeted bug fixes for LSP integration:
- **Tree-format session restore missing `lsp_did_open()`**: The newer tree-format session restore path (`restore_session_group_layout`) opened files via `buffer_manager.open_file()` directly but never called `lsp_did_open()`, so the LSP manager was never created when session files were restored. Fixed by adding `lsp_did_open()` calls for all restored buffers after the tree layout is installed in `restore_session_from_tree`.
- **Single pending semantic tokens request**: `lsp_pending_semantic_tokens` was `Option<i64>` (single slot). When the LSP initialized with multiple open files, only the last request's response was accepted. Changed to `HashMap<i64, PathBuf>` so all in-flight requests are tracked. New test: `engine_semantic_tokens_pending_multiple_requests`.
- **Color fix**: `semantic_parameter` in OneDark theme was same color as `variable` (#e06c75). Changed to #c8ae9d for visual distinction.

**Session 131 — LSP semantic tokens + develop branch workflow (17 new tests, 2302 total):**
LSP semantic token highlighting and branching workflow:
- **LSP semantic tokens**: Full `textDocument/semanticTokens/full` implementation. New types `SemanticToken`/`SemanticTokensLegend` in `lsp.rs`; delta-encoded u32 decoder (`decode_semantic_tokens`); `request_semantic_tokens_full()` method; `SemanticTokensResponse` event variant; client capability declaration in init_params; legend caching in `LspManager.semantic_legends`; `BufferState.semantic_tokens` storage; tokens requested on `didOpen`/`didChange`/`Initialized`; decoded via cached legend in `poll_lsp()`
- **8 new theme colors**: `semantic_parameter`, `semantic_property`, `semantic_namespace`, `semantic_enum_member`, `semantic_interface`, `semantic_type_parameter`, `semantic_decorator`, `semantic_macro` — set in all 4 themes (onedark/gruvbox/tokyo-night/solarized)
- **Render overlay**: `Theme::semantic_token_style()` maps LSP token types to styles (with bold for declaration/definition, italic for readonly/static/deprecated); `build_spans()` overlays semantic tokens after tree-sitter spans using binary search + UTF-16→byte conversion
- **Develop branch workflow**: `release.yml` reads version from Cargo.toml, creates `v$VERSION` tagged releases (not rolling `latest`); deleted redundant `rust.yml`; bumped version to 0.2.0; added `## Branching & Releases` section to CLAUDE.md
- **17 new tests**: 5 unit tests in `lsp.rs` (decode_semantic_tokens), 12 integration tests in `tests/semantic_tokens.rs`

> Sessions 130 and earlier archived in **SESSION_HISTORY.md**.
