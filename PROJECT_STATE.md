# VimCode Project State

**Last updated:** Apr 20, 2026 (Session 303 — Phase A.2b-2: GTK explorer atomic switchover shipped and smoke-tested; two rounds of smoke-test fix follow-ups landed as `c57f594` + `26ed4e9`) | **Tests:** 5223 total (full `cargo test --no-default-features`); 1943 lib + 414 nvim conformance + ~2866 integration

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 279 are in **SESSION_HISTORY.md**.
> **Active multi-stage wave:** `quadraui` cross-platform UI crate extraction — see **PLAN.md** for pickup-on-another-machine instructions.

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

**Session 303 — Phase A.2b-2: GTK explorer atomic switchover (native `gtk4::TreeView` → `DrawingArea`):**

1. **The `view!` macro block** for `explorer_panel` now holds a single
   `#[name = "explorer_da"]` `gtk4::DrawingArea` instead of a
   `ScrolledWindow` + `TreeView` with its inline 60-line
   `EventControllerKey`. Mirrors the A.3c-2 settings-panel shape.
2. **App struct** — removed `tree_store`, `tree_has_focus`,
   `file_tree_view`, `name_cell`; added `explorer_sidebar_da_ref` and
   `explorer_state: Rc<RefCell<ExplorerState>>`.
3. **Imperative DA setup** (after the settings-DA block in `init`):
   draw callback calling `draw_explorer_panel` with the adapter
   `explorer::explorer_to_tree_view`, `GestureClick` left (single →
   preview, double → open / toggle dir), `GestureClick` right (opens
   context menu at click coords), `EventControllerKey` (routes to
   `Msg::ExplorerKey`), and `EventControllerScroll` (wheel scroll).
4. **All 14 right-click context-menu actions** preserved — new_file,
   new_folder, rename, delete, copy_path, copy_relative_path, reveal,
   select_for_diff, diff_with_selected, open_side, open_side_vsplit,
   open_terminal, find_in_folder. Extracted into a dedicated
   `show_explorer_context_menu()` helper on `App`.
5. **Rename / new-file / new-folder inline editing deferred** — those
   three actions now route through `Msg::PromptRenameFile`,
   `Msg::PromptNewFile`, `Msg::PromptNewFolder`, each opening a simple
   modal `gtk4::Dialog` with a `gtk4::Entry` (pre-selected stem for
   rename, empty entry for new-file/folder). Inline text-cursor
   rendering inside `draw_tree` rows is left for a follow-up session
   once the `Form`/`TextInput` primitive ergonomics for row-embedded
   input are proven.
6. **Drag-and-drop** — the 100-line `DragSource` / `DropTarget` block
   was removed; tracked as follow-up [#149](https://github.com/JDonaghy/vimcode/issues/149).
7. **`SidebarPanel::Explorer` added to the `Msg::SwitchPanel`
   `grab_focus` block** — per the A.3c-2 lesson, the activity-bar
   `gtk4::Button` keeps focus after click; without this, keyboard
   input silently goes nowhere.
8. **App methods added**: `reveal_path_in_explorer` (replaces
   `highlight_file_in_tree` callsites), `refresh_explorer`,
   `explorer_viewport_rows`, `explorer_row_at`, `explorer_move_selection`,
   `queue_explorer_draw`, `handle_explorer_da_key`,
   `handle_explorer_da_click`, `handle_explorer_da_right_click`,
   `show_explorer_context_menu`, `prompt_for_name`.
9. **Deleted `src/gtk/tree.rs`** (503 lines). `validate_name` moved to
   `src/gtk/util.rs`; all other helpers (`build_file_tree_*`,
   `tree_row_expanded`, `update_tree_indicators`,
   `selected_parent_dir_from_app`, `selected_file_path_from_app`,
   `highlight_file_in_tree`, `find_tree_iter_for_path`,
   `remove_new_entry_rows`, `TREE_DUMMY_PATH`) removed.
10. **`start_inline_new_entry` replaced by `prompt_for_name`** — a
    generic modal-dialog helper reused by all three rename/new-entry
    prompts.
11. **`update_tree_indicators` periodic refresh removed** — the DA
    pulls indicators from `engine.explorer_indicators()` via the
    adapter on every draw, so the 1 Hz tick just calls `queue_draw()`.
12. **Quality gates all pass** — `cargo fmt`, `cargo clippy -- -D warnings`
    (default + `--no-default-features`), full
    `cargo test --no-default-features` (5223 / 0 / 19, same as
    baseline), `cargo build` (default + `--no-default-features`).
    **Net diff:** +936 / –1513 lines across 5 files (-577 net).
    `src/gtk/mod.rs` shrinks from 10,161 → 10,070; `src/gtk/tree.rs`
    (503 lines) deleted.
13. **Known scope gaps (deferred):**
    - Inline rename / new-entry editing inside `draw_tree` rows
      (follow-up issue to file after smoke-test).
    - Drag-and-drop (#149).
    - Context-menu as a `quadraui` primitive (not yet specced).
14. **Smoke-tested and shipped.** Two rounds of smoke-test fixes
    landed as `c57f594` (click row offset, `j`/`k`/`h`/`l` nav,
    trackpad scroll) and `26ed4e9` (scrollbar drag, folder
    single-click toggle, sidebar-resize hit zone, Ctrl-W h
    highlight). File-preview-on-single-click retested and working.
    Issue #152 closed.

---

**Session 303 — Phase A.2b-1: GTK explorer scaffolding landed (inert):**

1. **Scope decision.** The full A.2b migration (native `gtk4::TreeView`
   → `DrawingArea` + `quadraui_gtk::draw_tree`) is a ~1500-line diff
   across the `view!` macro, the App struct, ~50 scattered `Msg`
   handlers that reference `file_tree_view` / `tree_store` / `name_cell`,
   plus a 310-line right-click context-menu rewrite. Rather than land
   that atomically, the work was split into two sub-phases (recorded
   in `PLAN.md`) so the new draw pipeline can be validated before the
   destructive switchover.
2. **New `src/gtk/explorer.rs`** — module with:
   - `ExplorerRow { depth, name, path, is_dir, is_expanded }`
   - `ExplorerState { rows, expanded, selected, scroll_top }` with
     `new`, `rebuild`, `toggle_dir`, `ensure_visible`, `reveal_path`.
   - `build_explorer_rows(root, expanded, show_hidden, case_insensitive)`
     + `explorer_to_tree_view(state, has_focus, engine)` adapter.
   - Intentionally duplicates the TUI's `ExplorerRow` / `collect_rows`
     shape; unifying the two into `render.rs` is a future session.
3. **New `draw_explorer_panel` in `src/gtk/draw.rs`** — calls
   `quadraui_gtk::draw_tree` and overlays a Cairo scrollbar using the
   same 8px-wide pattern as `draw_settings_panel` (A.3c-2). Row height
   (`line_height * 1.4`) is kept in sync with `draw_tree` so the
   visible-row count used by the scrollbar matches the rendered layout.
4. **Sub-phase 1 is inert** — both additions are `#[allow(dead_code)]`;
   the file tree still renders via the native `gtk4::TreeView`.
   Sub-phase 2 flips the wiring and deletes the dead widget code.
5. **All quality gates pass** — `cargo fmt`, `cargo clippy` (default +
   `--no-default-features`), full `cargo test --no-default-features`
   (5223 / 0 / 19, same as baseline), `cargo build` (default + `--no-default-features`).
   **Net diff:** +319 / –0 lines (explorer.rs new, plus draw.rs +
   mod.rs additions). Zero behavioural change.

---

**Session 302 — Phase A.3c-2 shipped (GTK settings panel → DrawingArea + draw_form):**

1. **GTK settings sidebar migrated** from a native widget tree
   (`Switch`/`SpinButton`/`Entry`/`DropDown`/`Button` rows inside a
   `ScrolledWindow`) to a single `DrawingArea` that calls
   `quadraui_gtk::draw_form` (which has existed since A.3c). The panel
   is now visually consistent with the TUI A.3b version and gains
   in-place rendering of inline-edit cursor + bracketed value overlay.
2. **New `draw_settings_panel` in `src/gtk/draw.rs`** — header bar +
   search row (with cursor when active) + form body (via the
   primitive) + scrollbar column + `Open settings.json` footer row.
   Geometry contract documented in the doc comment so the click
   handler in `App::handle_settings_msg` mirrors row positions.
3. **New `App::handle_settings_msg` in `src/gtk/mod.rs`** — handles
   `Msg::SettingsKey` / `SettingsClick` / `SettingsScroll`. Click
   geometry: header → no-op, search → activate input, scrollbar track
   → jump-scroll, body row → select (double-click toggles bools or
   opens inline-edit for Integer/StringVal), footer → open
   `settings.json` in a new tab. Mouse wheel scrolls 3 rows per notch.
4. **Focus routing (the bug that surfaced during smoke test)** — the
   activity-bar `gtk4::Button` keeps focus after click, so neither the
   editor DA's key controller (capture phase, attached to the editor
   DA) nor the new settings DA's controller fired for `j`/`k`/`/`.
   Fixed by adding `SidebarPanel::Settings` to the per-panel
   `grab_focus` block in `Msg::SwitchPanel` *and* calling
   `da.grab_focus()` inside the click handler. The same pattern
   already existed for SC / Extensions / Debug / AI — captured in
   PLAN.md "Lessons learned" so future panels don't miss it.
5. **Removed the dead `build_settings_form` / `build_setting_row`
   from `src/gtk/util.rs`** (–206 lines). Removed the
   `settings_list_box` / `settings_sections` `App` fields and the
   panel-rebuild block on `SwitchPanel(Settings)` (DrawingArea is
   stateless). Re-exports of `SettingDef`/`SettingType`/`SETTING_DEFS`
   from `src/render.rs` removed (no longer needed).
6. **`SettingType::Integer { min, max }` annotated `#[allow(dead_code)]`**
   with a comment explaining the values are kept for future
   range-aware Form widgets (Slider per #143). Currently no backend
   reads them now that GTK no longer renders a `SpinButton`.
7. **All quality gates pass** — `cargo fmt`, `cargo clippy`
   (default and `--no-default-features`), full
   `cargo test --no-default-features` (5223 / 0 / 19, same as
   baseline), `cargo build`. **Net diff:** +513 / –302 lines across
   5 files; `src/gtk/util.rs` shrinks from 482 → 276 lines.
8. **Next up:** Phase A.2b (GTK explorer native `gtk4::TreeView` →
   `DrawingArea` + `quadraui_gtk::draw_tree`) is the only remaining
   large GTK migration on Linux. A.1c / A.2c need Windows.

---

**Session 301 — Fix #151: TUI palette scrollbar now draggable:**

1. **Mouse-drag on the TUI palette scrollbar thumb (and track-click jump)
   now scrolls the result list** — surfaced while smoke-testing A.4b
   (c8f2d91). The scrollbar drawn by both `quadraui_tui::draw_palette`
   (flat palettes) and the legacy preview-pane renderer
   (`render_picker_popup`) was render-only.
2. **`dragging_picker_sb: Option<SidebarScrollDrag>`** added to the TUI
   event loop and threaded into `handle_mouse`. Hit-test on mouse-down
   (col == popup_x + popup_w - 2, within the results-row band) both
   jump-scrolls to that row *and* starts a drag; mouse-drag updates
   `engine.picker_scroll_top` via the standard ratio; mouse-up clears
   the drag. Matches the existing `dragging_settings_sb` pattern.
3. **Only the TUI scope of #151 is addressed here.** The GTK palette has
   the same unwired scrollbar but is explicitly called out in the issue
   as a follow-up (GTK `draw_palette` lacks hit regions).
4. **No new tests** — scrollbar drag is pure TUI interaction that would
   need a ratatui/crossterm harness that doesn't exist in this repo.
   All existing 5223 tests pass; fmt + clippy (default + no-default-features)
   + cargo build all clean.

---

**Session 300 — Phase A.4b shipped + quickfix fixes around A.5b:**

1. **`quadraui_gtk::draw_palette`** added as the Cairo/Pango counterpart to
   A.4's TUI renderer. Bordered popup (Cairo stroke instead of box
   chars), title `Title  N/M`, query row with cursor block, separator,
   scrollable item rows with Pango per-character fuzzy-match
   highlighting, right-aligned detail, optional scrollbar.
2. **GTK picker migrated** — `draw_picker_popup` early-branches through
   `render::picker_panel_to_palette()` + `draw_palette` for flat pickers
   (command palette, buffer switcher, mark jumper, git-branch picker,
   diagnostic list). Preview / tree pickers (open-file, symbols) keep
   the legacy Cairo renderer, matching the TUI fall-through.
3. **A.5b smoke-test fallout handled on the way:** clearing quickfix
   focus on editor click (GTK + TUI), focusing the quickfix panel on
   `:grep` and `gr` (closes #150), and fixing TUI j/k/q key dispatch
   (engine intercept now normalises `key_name=""` + `unicode=Some(c)`
   to a single char string so handler `match` arms work across both
   backends). 2 new lib tests.
4. **Test count 5219 → 5223** from focus/normalisation tests added in
   A.5b follow-ups. All quality gates pass (fmt, clippy on both
   builds, full test suite, cargo build).
5. **Next up:** A.3c-2 (settings native→DrawingArea) and A.2b
   (explorer native→DrawingArea) remain as the larger architectural
   migrations on Linux.

---

**Session 299 — Phase A.5b shipped (GTK `draw_list` + quickfix migration):**

1. **`quadraui_gtk::draw_list`** added as the Cairo/Pango counterpart to
   A.5's TUI renderer. Optional title header in status-bar styling,
   flat rows at `line_height`-per-row, `▶ ` selection prefix, optional
   icon + detail, decoration-aware fg colour (Error/Warning/Muted/Header).
2. **GTK quickfix migrated** (`draw_quickfix_panel` in `src/gtk/draw.rs`)
   from an inline Cairo loop to a thin wrapper around
   `render::quickfix_to_list_view()` (the adapter already existed from
   A.5) + `draw_list`. Keeps scroll-to-selection behaviour. Net delta
   -38 lines of GTK rendering code.
3. **`docs/DECISIONS_quadraui_primitives.md`** — new running decision log
   for primitive-distinctness calls. D-001 records the retroactive
   rationale for `ListView` being separate from `TreeView`; D-002
   recommends the same call for `DataTable` #140. Establishes the
   principle *"one primitive per UX concept, not per algebraic
   reduction"*.
4. **Test count unchanged at 5219** — pure refactor, no new tests. All
   quality gates pass (fmt, clippy on both no-default-features and
   default GTK build, full test suite, cargo build).
5. **Next up:** A.4b (GTK `draw_palette`) remains the smallest unblocked
   GTK stage. A.3c-2 (settings native→DrawingArea) and A.2b (explorer
   native→DrawingArea) are the bigger architectural migrations still
   queued on Linux.

---

**Session 297 (continued) — Phase A.0 + A.1a shipped after the release:**

1. **Codified branch-first workflow in CLAUDE.md** — all changes go through a local branch off `develop`; no direct commits to `develop`. After local commit, user chooses either Path A (fast-forward merge + push) or Path B (push branch + open PR).
2. **Tightened the test-gate in CLAUDE.md** — pre-release must run full `cargo test --no-default-features`, not just `--lib`. The `--lib` shortcut missed the 2 integration test regressions that broke CI on the v0.10.0 merge (#114 follow-up, landed as `83d93b3`).
3. **Phase A.0 shipped** (`36ccad3`) — added `quadraui/` workspace member + `vimcode` path dep. Placeholder lib.rs, no primitives. Zero functional change.
4. **Phase A.1a shipped** (`bac137e`) — first real primitive migration:
   - Defined `quadraui::types` (Color, Icon, StyledText, WidgetId, Modifiers, TreePath, SelectionMode, Decoration, Badge, TreeStyle) and `quadraui::primitives::tree` (TreeView, TreeRow, TreeEvent). All owned + serde-compatible per plugin invariants.
   - Added `render::source_control_to_tree_view()` adapter (vimcode → quadraui).
   - New `src/tui_main/quadraui_tui` module with `draw_tree()` rendering TreeView into a ratatui Buffer.
   - TUI source-control panel's ~230-line section-rendering loop replaced with one `draw_tree()` call — no visible regression, smoke-tested.
   - Full-gate passes: 5219 tests, 0 failed.
5. **Added `PLAN.md`** — session-level coordination doc for in-flight multi-stage features. Captures current stage map, branch patterns, pickup instructions for A.1b (GTK) and A.1c (Win-GUI) on another machine, and the design invariants that must be preserved across all stages.

**Next up:** Phase A.1b (GTK `draw_tree`) and A.1c (Win-GUI `draw_tree`) are independent and can be done in either order. See PLAN.md for branch names and per-platform setup.

---

**Session 297 — `quadraui` cross-platform UI crate design + v0.10.0 release:**

1. **Design doc `docs/UI_CRATE_DESIGN.md` finalised** — captures the full plan for extracting a `quadraui` crate supporting Windows (Direct2D), Linux (GTK4), macOS (Core Graphics, v1.x), and TUI (ratatui) backends. vimcode becomes the first test app; other keyboard-driven apps (SQL client, k8s dashboard) are the second-wave consumers that prove the abstraction.
2. **13 design decisions resolved** in §7: retained-tree + events model, one `Backend` trait, `BufferView` adapter for `TextEditor` (text engine stays separate), a11y-ready data fields in v1 with platform wiring in v1.1, Option B workspace layout (`quadraui/` as workspace member from day 1), `quadraui` as working crate name (crates.io available), stage-by-stage PRs to develop instead of long-lived refactor branch, macOS ships in v1.x not blocking 1.0.
3. **Plugin-friendly design invariants** documented in §10 — 6 properties (`WidgetId` owned not `&'static`, events dispatched as data not closures, serde-compatible structs, no global event handlers, ownership model) that must hold so Lua plugins can later declare quadraui primitive UIs without breaking API changes.
4. **9 new GitHub issues filed** under new **"Cross-Platform UI Crate"** milestone: #139 TreeTable primitive, #140 DataTable (decide: standalone or TreeTable-with-depth-0), #141 Toast primitive, #142 Spinner+ProgressBar, #143 Form fields (Slider/ColorPicker/Dropdown), #144 live-append TextDisplay streaming, #145 k8s dashboard as Phase D validation app, #146 Lua plugin API extension for quadraui primitives, #147 bundled Postman-like HTTP client extension (depends on #146).
5. **Release 0.10.0** cut from develop as a stable baseline before Phase A work begins — bumped `Cargo.toml` 0.9.0 → 0.10.0, regenerated `flatpak/cargo-sources.json` (635 crate entries). All quality gates pass: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test --no-default-features --lib` (1939 passed / 0 failed / 9 pre-existing ignored), `cargo build`.
6. **Next step — Phase A.0 workspace scaffold**: single PR adding empty `quadraui/` workspace member + vimcode path dep. Then Phase A stages migrate panels one at a time (TreeView → SC panel, then explorer, then Form → settings, etc.).

**Session 295 — Phase 5 (#26) begins: `g`-prefix coverage audit:**

1. **Started Phase 5 `:help` coverage audit** — a new kind of conformance work that catches missing features rather than behavioural bugs. Scope: walk Vim's documentation section by section.
2. **First slice: `g`-prefix normal-mode commands** — 58 commands total. ✅ 36 implemented, 🟡 2 partial, ❌ 14 not implemented, ⏭️ 6 intentionally skipped (Ex mode, Select mode, mouse tag nav, debug features, redundant with VimCode's status bar).
3. **4 gap issues filed** for actionable missing features:
   - **#120** `gF` — edit file + jump to line number (the \`:N\` suffix case)
   - **#121** `g@` — user-definable operator via \`operatorfunc\` (enables plugin-defined operators)
   - **#122** `g<Tab>` — jump to last-accessed tab
   - **#123** Screen-line motions: \`g\$\`, \`g0\`, \`g^\`, \`g<End>\`, \`g<Home>\`
4. **New `COVERAGE_PHASE5.md`** — living document tracking the Phase 5 audit by slice.

**Session 294 — Fix #114: ex-command numeric line addresses are now 1-based:**

1. **Fix #114 — `parse_line_address` now 1-based for bare numbers** — Matches Vim's convention throughout. `":3"` → index 2, `":0"` → index 0 (used by copy/move as "before line 1"). Relative addresses (`+N`, `-N`, `.`, `$`) unchanged — they were already correct.
2. **Added `dest_is_zero` special case** to `execute_copy_command` (the single-line form) so `:copy 0` inserts at top, matching the existing range-version behaviour.
3. **6 existing tests updated** to encode 1-based semantics (`:1,2co3`, `:1m3`, `:t3`, `:m3`, `:co2`, `:copy 2`); 4 new tests added covering 1-based specifically and the `:N m 0` / `:copy 0` special case.

**Session 293 — Fix #116 visual block virtual-end append:**

1. **Fix #116: `<C-v>jj$A<text>` now appends at each line's actual end** — Extended `visual_block_insert_info` tuple with a `virtual_end: bool` flag. When `$` is pressed in visual block mode (sets `visual_dollar = true`), `A` captures that flag and the Esc handler appends `<text>` at each selected line's own end instead of the captured column. Correctly clarified the ignored test's keystroke sequence — the virtual-end trigger is `<C-v>...$A`, not `$<C-v>...A`.

**Session 292 — Phase 4 batch 16 (#25), 18 new conformance tests, #116 filed:**

1. **Phase 4 batch 16: 18 new Neovim-verified tests** — Covering visual block `I` (insert prefix), visual block `A` (append suffix), `:noh` clears search highlights, `:r` on nonexistent file, `:retab` tab-to-spaces, lowercase marks (`ma` / `'a` / `` `a ``), `n` with no prior search, word motions at EOF/BOF, visual indent/dedent (`V>`, `V<`), `5rX` count-prefix replace, `dap` delete-around-paragraph, `.` repeat last change, `u`/`<C-r>` undo-redo, `:set tabstop?` query.
2. **#116 filed** — Visual block started with `$<C-v>jjA` should virtual-append at each line's actual end (Vim behavior). VimCode uses the starting cursor column, so appending on a longer line inserts mid-word instead of at the end.

**Session 291 — Fix #112: ranged/concat/bang ex-command forms, #114 filed:**

1. **Fix #112 — Ranged `:copy`/`:move`, concat `:tN`/`:mN`/`:coN`, `:sort!`** — Added `execute_copy_range()` and `execute_move_range()` helpers with 1-based range semantics matching Vim. Extended `try_execute_ranged_command()` to dispatch `m`/`move`/`t`/`co`/`copy` keywords. Added `split_cmd_and_arg()` helper that matches a command name followed by a valid separator (digit/space/sign/`./$`). Accepted `:sort!` bang as reverse synonym.
2. **#114 filed** — VimCode's `parse_line_address` treats numeric dest as 0-based, but Vim uses 1-based throughout. Fix requires auditing existing callers; scoped as a separate issue so this PR stays focused.
3. **12 new unit tests** covering all new forms + regressions (`:0` still goes to line 0, `:sort r` still works).

**Session 290 — Phase 4 batch 15 (#25), 23 new conformance tests, #112 filed:**

1. **Phase 4 batch 15: 23 new Neovim-verified tests** — Covering `:copy`/`:move` (simple form), `:sort` (basic and reverse via `r` flag), `:sort u` unique, `gi` restart insert, `gv` reselect last visual, jump list (`<C-o>`/`<C-i>`), change list (`g;`), `:enew`, window move (`<C-w>H`), case operators (`gUw`, `guiw`, `g~w`), count+operator (`3dw`, `2cwXYZ`), text object edges (`daw` at word boundary, `das`), `:set number`/`nonumber`, `:pwd`.
2. **#112 filed** — Collected deviations discovered during mining: ranged `:copy`/`:move` forms don't accept range prefixes; `:t<N>` / `:m<N>` / `:co<N>` concatenated forms not recognized; `:sort!` bang not parsed (users must use `:sort r` for reverse).

**Session 289 — Phase 4 batch 14 (#25) + fixes for #109 and #110:**

1. **Phase 4 batch 14: 25 new Neovim-verified tests** — Covering areas still uncovered: named registers (`"ayy`/`"ap`/`"Ayy`/`"add`), folding (`zf`/`zR`/`zd`), window splits (`<C-w>s/v/w/q/o`, `:split`, `:vsplit`), `:echo`, `:w` error case, word-end motions (`e`, `ge`), increment/decrement edge cases, search history, numeric `:N` and `:N,M` ranges.
2. **Fix #109: Ctrl-A/Ctrl-X now parse hex (`0x..`) numbers correctly** — Added hex-prefix detection in `increment_number_at_cursor()` so cursor landing on or before the leading `0` of `0x09` now increments as hex → `0x0a` instead of decimal `1x09`. Also covers `-0x..`. 2 extra tests added (cursor-inside-hex, decrement).
3. **Fix #110: Yank to named register no longer overwrites register 0** — Updated `set_yank_register()` to only update `"0` when the target is the unnamed register (`"`). Matches Vim's `:help registers` semantics.
4. **Closed #60** housekeeping (PR #106 was already merged but issue wasn't auto-closed).

**Session 288 — #107 git_branch_changed plugin event (follow-up to #60):**

1. **Fire `git_branch_changed` plugin event** from `tick_git_branch()` when an external branch change is detected. Plugins (e.g. git-insights panel) can now subscribe via `vimcode.on("git_branch_changed", fn)` and refresh their UI instead of going stale.
2. **No new Lua API surface** — plugins already have `vimcode.git.branch()` to re-query state on the event.
3. **2 new unit tests**: plugin event fires on change, does NOT fire when branch unchanged (1815 → 1817 lib tests).
4. **EXTENSIONS.md updated** with the new event.

**Session 287 — Fix #60 Git branch status bar refresh:**

1. **Fix #60: Status bar now detects external branch changes** — Added `tick_git_branch()` method on Engine that polls `git::current_branch()` at most once per 2 seconds and returns `true` if the branch changed. Wired into all three backends (GTK, TUI, Win-GUI) via their existing tick loops; a detected change triggers a redraw.
2. **2 new unit tests** (rate-limit + change detection) — 1813 → 1815 lib tests.

**Session 286 — Fix #101 Replace mode Esc cursor position:**

1. **Fix #101: Replace mode cursor stepback on Esc** — `handle_replace_key` Esc handler in `src/core/engine/motions.rs` was missing the cursor-step-back that Insert mode already had. Added the same `col > 0 → col -= 1` logic. Also covers `gR` virtual replace.
2. **2 previously-ignored tests now passing** (1811 → 1813 lib, 11 → 9 ignored).

**Session 285 — Phase 4 batch 13 (#25), 25 new conformance tests, 0 new deviations:**

1. **Phase 4 batch 13: 25 new Neovim-verified tests** — Covering substitute (`:s/`, `:%s/`, flags `g`/`i`, empty replacement, no-match), global (`:g/pat/d`, `:v/pat/d`), tab navigation (`:tabnew`, `gt` cycle), G-motions (`dG`, `dgg`, `yG`), bigword motions (`W`, `B`, `E`, `gE`), f/F with count, comma-reverse, `%` bracket matching, register `"1` (last delete), linewise paste (`yyp`, `yyP`).
2. **All 25 tests pass on first run** — no new deviations discovered in these areas.

**Session 284 — Phase 4 batch 12 (#25), 27 new conformance tests, 1 new deviation (#101):**

1. **Phase 4 batch 12: 27 new Neovim-verified tests** — Covering areas previously under-tested: search (`/`, `?`, `n`, `N`, count prefix, wrap-around), scroll commands (`zz`, `<C-d>`, `<C-u>`, `<C-b>`), number increment/decrement (`<C-a>`, `<C-x>` with count and negatives), replace mode (`R`, `r<CR>`, `3rX`), case change (`gUU`, `guw`, `gUw`), and count+motion combos (`5l`, `3j`, `3dd`, `3yy+p`).
2. **1 new deviation documented (#101)**: Replace mode cursor lands at col+1 after `<Esc>` instead of on the last replaced char (Vim behavior). Documented as 2 ignored tests.

**Session 283 — Fix 3 Vim deviations (#97, #98, #99):**

1. **Fix #97: Visual line J now joins selected lines** — Added `J` handler in visual mode operator dispatch. `VjjJ` correctly joins all selected lines.
2. **Fix #98: :%join range now supported** — Added `%` range prefix handling in `execute_command()`. Also supports `%d` and `%y`.
3. **Fix #99: Ctrl-U in insert mode respects insert point** — Added `insert_enter_col` field to track where insert mode was entered. Ctrl-U now deletes only back to that boundary instead of line start.
4. **Closed #65** (already fixed in session 282, issue left open).
5. **4 previously-ignored tests now passing** (1757 → 1761 lib tests).

**Session 282 — Insert paste fix (#65), Phase 4 batches 10-11 (#25), 8 deviations fixed:**

1. **Fix #65: Ctrl-V paste in insert mode added cumulative indentation** — `paste_in_insert_mode()` was applying auto-indent to each pasted line, causing a staircase effect. Fixed by suppressing auto-indent during paste (pasted text already has its own whitespace).
2. **Phase 4 batches 10-11 (#25): 58 new Neovim-mined tests** — Mined from test_undo.vim, test_change.vim, test_put.vim, test_marks.vim, test_registers.vim, test_join.vim. Covering: undo/redo (5), put/paste (7), change operations (11), text objects (9), marks (4), registers (6), macros (2), join edge cases (5), insert mode keys (3), changelist navigation (2).
3. **Fixed 8 Vim deviations**: Vc/Vjc visual line change ate trailing newline; r\<CR\> was a no-op; S didn't preserve indent; daw at end of line didn't consume leading whitespace; tick mark jump ('a) went to col 0 instead of first non-blank; feed_keys didn't drain macro playback queue; updated test_visual_line_change to correct Vim expectation.
4. **3 new deviations documented** (ignored tests): visual J in line mode, :%join range not supported, Ctrl-U in insert deletes to line start instead of insert start.

> Session 281 and earlier in **SESSION_HISTORY.md**.
