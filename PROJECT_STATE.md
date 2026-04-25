# VimCode Project State

**Last updated:** Apr 24, 2026 (Session 330 — Smoke-test sweep + GTK explorer scrollbar migration: 7 fixes landed via Path A across both backends, 7 issues closed, 3 follow-up issues filed)

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 326 are in **SESSION_HISTORY.md**.
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

## Cross-backend coverage (chrome only — editor viewport excluded)

Snapshot of where each chrome surface stands on its quadraui primitive.
TUI is the reference implementation; GTK has been catching up. Numbers
update with each Path-A landing — read this to find the next slice.

**Status:** TUI chrome is **~95%** on quadraui; GTK chrome is **~65%**.
Closing the gap is **Phase B.5** — tracked as umbrella issue **#205**.

| Surface | Primitive | TUI | GTK | Notes |
|---|---|---|---|---|
| Status bar (per-window + global) | `StatusBar` | ✅ | ✅ | layout via `StatusBarLayout` |
| Tab bar | `TabBar` | ✅ | ✅ | |
| Activity bar | `ActivityBar` | ✅ | ✅ | |
| Tree view (explorer + SC) | `TreeView` | ✅ | ✅ | layout via `TreeViewLayout` |
| List view (quickfix + tab switcher) | `ListView` | ✅ | ✅ | layout via `ListViewLayout` |
| Form (settings) | `Form` | ✅ | ✅ | hint field exists but unrendered (#202) |
| Palette (cmd palette + folder picker) | `Palette` | ✅ | ✅ | layout via `PaletteLayout` |
| Find/replace overlay | shared hit-regions | ✅ | ✅ | engine-side `compute_find_replace_hit_regions` |
| Terminal cells | `Terminal` | ✅ | ✅ | |
| Tooltip (hover, signature, diff peek, panel hover) | `Tooltip` | ✅ | ❌ bespoke | 4 GTK popups → 1 primitive (#205) |
| Dialog (quit/close confirm) | `Dialog` | ✅ | ❌ bespoke | `draw_dialog_popup` (#205) |
| Context menu | `ContextMenu` | ✅ | ❌ bespoke | `draw_context_menu_popup` (#205) |
| Menu dropdown (top menu bar) | `ContextMenu` | ✅ | ❌ bespoke | `draw_menu_dropdown` (#205, also closes #181) |
| Completion popup | `Completions` | ✅ | ❌ bespoke | `draw_completion_popup` (#205) |
| Debug toolbar | `StatusBar` | ✅ | ❌ bespoke | `draw_debug_toolbar` (#205) |
| Breadcrumb bar | `StatusBar` | ✅ | ❌ bespoke | `draw_breadcrumb_bar` (#205) |
| Ext-panel scrollbar (drag + render) | shared dispatch | ✅ drag | ❌ neither | #205 + #200 |

**Cross-backend logic-sharing** (where one implementation drives both backends):

- All primitive `Layout` algorithms (`StatusBarLayout`, `PaletteLayout`, etc.) — single implementation, both backends consume.
- `quadraui::dispatch_mouse_down/drag/up` + `ModalStack` + `DragState` — drives palette drag, picker drag, TUI sidebar scrollbar drag, and GTK explorer scrollbar drag (as of `3e5d7d3`).
- Engine-side hit-region builders (`compute_find_replace_hit_regions`) and cell-unit fit algorithms (`StatusBar::fit_right_start`, `TabBar::fit_active_scroll_offset`) — parameterised over a measurement closure so each backend supplies its native unit.
- `core::settings::SAVE_REVISION` — one source of truth both file watchers consult (#201).
- All `*_to_form` / `*_to_tree_view` / `lsp_status_for_buffer` adapters in `render.rs` and `core/engine/`.

**North-star ("developer doesn't need to know the backend") status:**

- ✅ True for picker-shaped, status-bar-shaped, tree-shaped surfaces — adding a new instance means writing data + handlers, never touching Pango/cells.
- ❌ Not yet true for dialog / tooltip / completion-shaped surfaces — GTK still bespoke. Phase B.5 / #205 closes this.
- ❌ No `Backend::watch_file(path) -> Stream<FileEvent>` trait method — every backend rolls its own watcher (TUI poll, GTK GIO, future Win-GUI `ReadDirectoryChangesW`). Suppress decision is shared (#201) but not the watcher invocation. **Filed as gap to revisit; not blocking B.5.**
- ⏭️ Editor viewport explicitly out of scope (deferred A.9 / B.4-editor) — vimcode-the-editor still hand-renders text per-backend, which is fine for vimcode but means the vim-motion-suite vision (PLAN.md) needs A.9 before it can launch.
- ⏭️ Win-GUI has TreeView / Explorer / StatusBar / TabBar but most of B.3+ hasn't reached Windows. "Cross-platform" currently means ~1.5 platforms.

---

## Recent Work

**Session 330 (cont.) — GTK explorer scrollbar migrated to `dispatch_mouse_drag` (closes #204 + #199):**

- `3e5d7d3` — fix(gtk): explorer scrollbar uses
  `quadraui::dispatch_mouse_drag`. Replaces the hand-rolled `dy / sb_h
  * max_scroll` math (which had the same off-by-thumb-length bug `cb84f82`
  fixed in TUI) with the shared dispatcher. Also resolves the
  click-on-track jump-scroll gap (#199) by doing the jump inline in
  `connect_drag_begin` rather than relying on the click handler — GTK
  was suppressing the click handler once the drag gesture claimed the
  sequence, so the jump never fired. Drops the now-unused
  `explorer_scrollbar_drag_from: Rc<Cell<Option<usize>>>` cell.
- Picker / sidebar / explorer scrollbars now share one math
  implementation in `quadraui::dispatch.rs`. Adding a new scrollbar
  surface (debug sidebar, Win-GUI any of the above) is "hold a
  `DragState`, call `dispatch_mouse_drag`, match
  `UiEvent::ScrollOffsetChanged` on widget id" — no new geometry math.
- **Deferred**: GTK ext-panel scrollbar drag has the same shape but is
  doubly dead — the scrollbar isn't actually drawn (#200) and scroll
  state isn't applied to rendering. Migrating the dead handler in
  isolation would be a no-op for users; bundled with #200's render fix
  in a follow-up.

---

**Session 330 — Smoke-test sweep of Session 329 backlog (6 commits landed via Path A, 3 issues filed):**

Branch-per-issue workflow: each fix on its own local branch, full
`fmt + clippy -D warnings + test --no-default-features` gate before
commit, smoke-tested in TUI + GTK as applicable, then ff-merged to
develop and pushed. Five Session-329-filed issues closed; one new
issue (#201) filed and fixed in the same session because it blocked
verification of #174.

**Landed fixes (chronological order on develop):**

1. `cb84f82` — **TUI sidebar scrollbar pilot** (no issue#).
   Migrates explorer + ext-panel scrollbar drag onto
   `quadraui::DragState` + `dispatch_mouse_drag`. Adds `thumb_length`
   adjustment to the dispatcher's pixel-to-rows math so the thumb
   tracks 1:1 with mouse motion. Adds new generic
   `UiEvent::ScrollOffsetChanged { widget, new_offset }` (replaces
   per-primitive `PaletteEvent::ScrollOffsetChanged`).
2. `9334686` — fix(gtk): settings panel **section headers toggle on
   single-click** (#188). Setting rows still need double-click to
   avoid surprise edits.
3. `4142561` — fix(lsp): **`…` pending indicator restored** until
   semantic tokens arrive (#195). Entirely shared-code fix:
   `LspManager::language_supports_semantic_tokens` + downgrade in
   `Engine::lsp_status_for_buffer`. Both backends pick it up via the
   already-shared status-bar adapter — zero per-backend changes.
4. `9a2271a` — fix(settings): **"Status Line Above Terminal" relabel**
   to "Status Line Inside Window" (#173). Cheapest of the four options
   on the issue: no key change, no settings.json migration, just label
   + description text on the SettingDef.
5. `a8cb6ee` — fix(settings): **`:set` accepts snake_case names**
   shown in the Settings panel (#174). Snake_case→packed-name
   fallback in `set_bool_option`, `set_value_option`, `query_option`
   — backwards-compatible. Includes new unit test.
6. `7f9af22` — fix(gtk,tui): **`:set` output no longer overwritten
   by "Settings reloaded"** (#201). Process-global `SAVE_REVISION`
   atomic in `Settings`; both watchers (TUI mtime poll, GTK GIO
   `FileMonitor`) consult the revision counter to tell self-saves
   from external edits. Replaced GTK's per-instance
   `settings_self_save: bool` flag (which only caught the Settings
   panel path, not `:set` from the command line). Also skips disk
   save on query-form `:set foo?`.

**Issues filed during the sweep:**

- #201 — `:set` message clobber (filed + closed in-session).
- #202 — Settings descriptions never rendered. The `description`
  field on `SettingDef` exists but no backend's `draw_form` reads
  `FormField.hint`. Real fix needs adapter (1 line) + Form layout
  reservation + per-backend hint-row rendering. Out of scope for #173.
- #203 — TUI crash on terminal resize: indent guide bounds check
  uses window-area instead of frame-area (`render_impl.rs:2097`).
  Pre-existing — surfaced during #cb84f82 smoke test, not caused by
  it. Crash is recoverable (panic hook flushes swap files).
- #204 — GTK explorer scrollbar should migrate to
  `dispatch_mouse_drag` (cross-references #199, #200). User noticed
  the same drag-feel bug after the TUI pilot fixed it. The math
  fix exists in `dispatch_mouse_drag` already; the migration is
  re-plumbing the GTK gesture handler to use shared `DragState`
  instead of its own private `explorer_scrollbar_drag_from` cell.

**Cross-backend wins worth noting:**

- The #195 LSP indicator fix is the cleanest example of B.4's payoff
  yet — one core change in `lsp_ops.rs`, both backends correct
  immediately, *zero* per-backend code touched. The status-bar
  adapter that renders `name…` in dim colour was already shared.
- The #201 watcher-clobber fix uses the shared `SAVE_REVISION` atomic
  so both the TUI poll and the GTK GIO monitor consult one source of
  truth. The watching *mechanism* stays per-backend (necessarily —
  GIO vs poll vs Win-GUI's future `ReadDirectoryChangesW`), but the
  *suppress decision* is now shared.

---

**Session 329 — Phase B.4 arc extends into event routing (8 substantive commits + 1 fix + 1 doc + 5 issues filed):**

Session 328 landed every major TUI overlay on quadraui *rendering*
primitives. Session 329 opens the second half of B.4 — the *event*
half — and proves it works. Eight substantive commits land on
develop. Order intentional: GTK catches up on D6 rendering first so
two backends share primitive consumption; then event routing builds
on top of shared contracts.

**GTK rendering — 4 D6 migrations (proves primitive set works across
coordinate systems — char cells on TUI, pixels + Pango on GTK):**

1. `31ebdc4` — `draw_status_bar` consumes `StatusBarLayout`. Pilot
   commit for the GTK D6 migration wave; replaces hand-rolled
   left-accumulate + right-fit loop with a single `bar.layout()` call.
2. `b0215e2` — `draw_list` consumes `ListViewLayout`. Scroll
   clamping + title-row handling now live in the primitive.
3. `89d54ae` — `draw_tree` consumes `TreeViewLayout`. Per-row
   heights (header `line_height` vs items `line_height * 1.4`)
   supplied via the measurement closure.
4. `8ccea7e` — `draw_palette` consumes `PaletteLayout`. Shallow-
   clones the palette locally to inject the effective scroll offset
   without mutating caller state.

**Cross-backend event routing — 4 commits (the infrastructure that
actually earns "cross-platform without knowing GTK"):**

5. `a02eff9` — **B.4 event-routing pilot**. New `quadraui::ModalStack`
   + `dispatch_mouse_down` free function. Fixes #192 (GTK palette
   click-drag leaked to editor). Infrastructure: `Backend::modal_stack_mut()`
   trait method (additive); `ModalEntry { id, bounds }`; `push / pop /
   top / hit_test / iter_top_down`. 7 unit tests in `modal_stack.rs`
   + 5 in `dispatch.rs`.
6. `0f3e0d0` — `DragState` + `dispatch_mouse_drag` + `dispatch_mouse_up`.
   `DragTarget::ScrollbarY` carries track geometry + visible/total
   row counts; dispatcher does linear-interpolation math. Fixes
   #190 (GTK palette scrollbar was painted but not draggable). 6
   new unit tests. New `PaletteEvent::ScrollOffsetChanged { new_offset }`
   variant.
7. `b169ca4` — **TUI palette scrollbar drag migrated** onto the
   same `DragState` + `dispatch_mouse_drag` code GTK uses. This is
   the payoff commit — one quadraui code path now drives both
   backends' scroll math, not two parallel implementations. Legacy
   `dragging_picker_sb: Option<SidebarScrollDrag>` removed.
8. `bad14f0` — **TUI picker modal dismiss migrated** onto
   `ModalStack` + `dispatch_mouse_down`. Both GTK and TUI now
   arbitrate click-inside-modal vs click-outside-to-close through
   the same dispatcher. Completes the picker-surface
   cross-backend story.

**Pre-existing fixes surfaced during smoke testing:**

- `6f26ec7` — TUI source control panel click off-by-one (#184, closed).
  After the SC TreeView migration the renderer stopped emitting a
  "(no changes)" placeholder row for empty expanded sections; the
  click handler was still passing `empty_section_hint: true` and
  every row after an expanded-but-empty section was off by +1.

**Docs:**

- `729c988` — `quadraui/docs/TUI_CONSUMER_TOUR.md` (Session 328
  wrap-up). Reading guide walking through five progressive examples
  from simplest D6 primitive (Tooltip + hover popup) through the
  `hit_regions` escape-hatch pattern (find/replace). Intended as
  orientation for Phase B.5+ (GTK / Win-GUI / macOS rewrites).

**Issues filed during smoke testing (pre-existing bugs, not
regressions):**

- #185 — Quickfix jump scrolls cursor under the quickfix panel
  (engine `ensure_cursor_visible` doesn't subtract qf_height).
- #186 — Explorer diagnostic-count badges show count but lack red
  coloring (severity fg not set in adapter).
- #187 — GTK git panel text-clipping / chevron-overlap on Recent
  Commits (likely `source_control_to_tree_view` indent / leaf
  math).
- #188 — Settings panel needs double-click to expand sections
  (pre-existing; handler guards on n_press>=2).
- #189 — Git panel discard leaves editor view showing stale diff
  state (buffer not reloaded after `git checkout`).
- #191 — GTK palette scrollwheel scrolls 1 row per event (GTK event
  controller flags, no quadraui change needed).
- #192 — **Closed** by `a02eff9`.
- #193 — Palette entries like "Find and Replace" show status-message
  placeholders instead of invoking the action.
- #194 — Status-bar messages aren't mouse-selectable (GTK) / have
  offset-by-sidebar-width selection bug (TUI).

**Quadraui API additions during the arc:**

- `ModalStack`, `ModalEntry` (new module `modal_stack.rs`)
- `DragState`, `DragTarget` (additions to `dispatch.rs`)
- `dispatch_mouse_down`, `dispatch_mouse_drag`, `dispatch_mouse_up`
- `PaletteEvent::ScrollOffsetChanged { new_offset: usize }`
- `Backend::modal_stack_mut()` trait method (additive)

**Architectural framing:** the user's north-star question driving
this session — "can a developer write a cross-platform app on
quadraui without knowing GTK / Cocoa / crossterm?" — is now
genuinely answered "yes, for the picker surface." The proof: the
four event-routing commits ship a loop where identical `quadraui::
dispatch_*` calls service two coordinate systems (char cells on TUI,
pixels on GTK) with zero backend-specific logic in the dispatcher
itself. Adding a third backend (Win-GUI, macOS) means holding a
`ModalStack` + `DragState`, routing raw events through the
dispatcher, and matching on the returned `UiEvent`s. No new math,
no new precedence logic.

**What's next:** generalize the pattern off of the picker. Each of
these is a per-surface commit that reuses the same infrastructure
with zero quadraui changes:

- Tab switcher modal (TUI + GTK) — new modal shape (centered list,
  no scrollbar), proves `ModalStack` extends beyond the picker.
- Sidebar scrollbars (explorer, SC, debug, settings) — each migrates
  from its own `SidebarScrollDrag` Option to a `DragState::ScrollbarY`.
- Dialogs (quit confirm, close-tab confirm, etc.) — backdrop dismiss
  via a new `ModalDismissed(WidgetId)` variant (currently
  `PaletteEvent::Closed` is the universal dismiss event; generalize
  when the second modal type arrives).

The alternative sequence is to close out issue backlog (#185–#194)
before expanding the event-routing surface. Either order works;
depends on whether daily-driver quality or architectural completion
is the higher priority.

---

**Session 328 — B.4 chrome migration substantially complete (22 commits on develop):**

Every major TUI chrome popup / strip now renders through a quadraui
primitive or — for find/replace specifically — through shared
cross-backend hit-region data. Each migration shipped as a focused
commit, smoke-tested in TUI, and Path-A landed (ff-merge + push
develop) after `cargo fmt` + `cargo clippy --no-default-features
-- -D warnings` + full test suite green. No test regressions
across the arc.

**Migrations landed (in chronological order):**

1. **Dialog** rendering migrated to `DialogLayout` (`83974fe`).
   Includes optional `DialogInput` extension (commit `9f24313`).
2. **Completion popup** consumes `CompletionsLayout` (`afa14d9`).
3. **Close-tab confirm overlay** → Dialog primitive
   (`71a0d02` + `19b08ca` click intercept + `fade4e7` `:quit`/`:quit!`
   semantics + `34e4a24` Tab/arrow nav).
4. **Context menu** (tab action menu) → `ContextMenuLayout` (`9a52fd7`).
5. **Quit-confirm overlay** → Dialog (`93fbd4b`).
6. **LSP hover popup** → Tooltip (`0dbbf70`).
7. **Signature help popup** → Tooltip with `styled_lines` extension
   (`e6048d8` + `38a79fc` adapter unit tests + `signatureHelp`
   client-capability fix in LSP init params).
8. **Tab switcher popup** → bordered `ListView` (new `bordered: bool`
   field on ListView; `85841d2`).
9. **Folder picker modal** → `Palette` (`4e470f8` + `eae455c`
   `:OpenRecent` ex-command dispatch fix).
10. **Menu dropdown** → `ContextMenu` (`c6c0718`).
11. **Debug toolbar** → `StatusBar` with `bar.resolve_click` for hit
    testing (`f84c3c2` + `408a326` local-col fix + `de8d7e2`
    toolbar-row math fix).
12. **Breadcrumb bar** → `StatusBar` with `bar.resolve_click` for
    hit testing (`553b207`).
13. **Diff peek popup** → multi-line styled Tooltip (rename Tooltip
    `styled` → `styled_lines: Option<Vec<StyledText>>`; `e4ae90e` +
    `1c6af39` `revert_hunk` `--index` fix so reverting a staged hunk
    also unstages it).
14. **Find/replace overlay** → consolidated TUI rasteriser that walks
    `panel.hit_regions` instead of re-deriving column math
    (`4eacaa0`). No new quadraui primitive (find/replace doesn't fit
    Form / Dialog / StatusBar / Palette cleanly, and a speculative
    `Toolbar` primitive isn't justified yet — no second consumer).

**Quadraui API extensions during the arc:**
- `ListView.bordered: bool` (#[serde(default)]) for modal-style
  bordered lists; layout insets items by 1 cell on each side and
  reserves rows 0 + N-1 for ╭─╮ ╰─╯ borders. Title (when present)
  overlays the top border.
- `Tooltip.styled` (`Option<StyledText>`, single-line) renamed to
  `Tooltip.styled_lines` (`Option<Vec<StyledText>>`, multi-line)
  for consumers that need per-row styled spans (signature help,
  diff peek). Single-line consumers wrap their styled line in a
  1-element vec.
- New `Tooltip` field `styled_lines` documented as multi-line styled
  override; rasteriser dispatches text → styled_lines → plain text.

**Engine bugs surfaced during smoke testing (fixed in same branch):**
- `revert_hunk` ran `git apply --reverse` against the working tree
  only, leaving any staged copy of the hunk in the index. Fixed
  with `--index` flag.
- `:OpenRecent` ex command silently fell through — handler was
  present in menu-click path but missing from command-execution
  dispatch in `mod.rs:3741`.
- Debug toolbar `toolbar_row` math wrongly subtracted `qf_rows +
  strip_rows` (rows above the toolbar, not below); never matched
  when terminal/debug panel was open. Recomputed from below using
  actual layout chunks.
- Debug toolbar click hit-test passed absolute screen col + full
  terminal width to `bar.resolve_click()`; the bar starts at
  `editor_left` so absolute clicks resolved past the last segment.
  Fixed by converting to bar-local space.

**Follow-up issues filed (out-of-scope for the migration arc):**
- #180 — LSP signature help popup never shows data (engine-side
  data pipeline bug; render path is unit-tested correct).
- #181 — Menu dropdown items don't highlight on mouse hover
  (pre-existing TUI mouse-event-handling gap).
- #182 — Debug toolbar icons render as wrong/missing glyphs in
  some terminals (suggested fallback char improvements in
  `src/icons.rs`).
- #183 — Debug toolbar visibility tied to active DAP session;
  proposes a "always show" setting + menu entry.
- #184 — Source control panel: clicking a row highlights the row
  ABOVE the clicked row (off-by-one in TUI mouse handler;
  GTK already uses accumulator walk per Session 197).

**Out of scope for B.4 chrome (deferred):**
- **Tab drag overlay** — three-piece visual (drop-zone highlight
  + insertion bar + ghost label). Doesn't fit any primitive
  cleanly and a future backend will paint each piece its own way
  (different drag conventions per platform). Migration would gain
  nothing real and constrain future backends.
- **Menu bar row** (labels strip + nav arrows + search box) —
  composite chrome. MenuBar primitive only covers the labels
  strip; the rest is bespoke. Revisit when a fuller composite
  primitive lands or when the GTK rewrite forces the issue.
- **Picker popup with preview pane / tree-indented variants** —
  flat-list pickers already migrated to Palette; the preview
  variant needs preview-pane support added to Palette first.

**Net result:** Phase B.4 chrome arc landed 22 commits on develop
covering ~10 substantive migrations + ~6 fixes. Tests still green
end-to-end. Click resolution for the toolbar / breadcrumb /
find-replace overlays now derives from the same data structure as
paint, eliminating the entire "paint and hit-test drift apart" bug
class on those surfaces. Pattern is established for future GTK /
Win-GUI / macOS rewrites: each primitive's rasteriser lives in
`{backend}_quadraui.rs`; engine-side adapter functions in
`render.rs` build the primitive; backend-specific call site
threads the area + theme. No engine logic changed except for the
4 fixes listed above.

**What's next:** Phase B.4 chrome can be considered substantially
done; the remaining TUI work is the editor viewport itself (which
the chrome-only scope explicitly defers — see Session 327 for the
scope decision). Phase B.5 (GTK rewrite) is the natural next wave;
or revisit the deferred picker preview pane / menu bar / tab drag
items first if their lack is felt during day-to-day use.

---

**Session 327 — B.3 readiness gate CLEAR (all primitives on D6):**

Huge session. Starting point: D7 focus model had just been resolved;
readiness gate still needed all 9 existing primitives to gain
`layout()` + `hit_test()`, plus ~14 new primitives for B.3.

**Existing primitives — all 9 gained `layout()` + `hit_test()`:**

1. `TabBar::layout()` + `TabBarLayout::hit_test()` (`0517e54`).
   Reference implementation for the D6 shape. Closed #179
   structurally. 14 unit tests including fractional pixel-unit
   layout (proves TUI/native unit agnosticism).
2. `compute_tab_bar_hit_regions` delegates to `TabBar::layout()`
   (`ebe0eec`). First real-world consumer of D6.
3. `quadraui_tui::draw_tab_bar` consumes `TabBarLayout` (`713f071`).
4. `StatusBar::layout()` (`d9cfa34`).
5. `quadraui_tui::draw_status_bar` + hit-test consume layout
   (`f263765`).
6. `TreeView::layout()` (`7613316`).
7. `ListView::layout()` (`7a09749`).
8. `ActivityBar::layout()` (`914c6f9`).
9. `Palette::layout()` (`65622fb`).
10. `Form::layout()` (`130285e`).
11. `TextDisplay::layout()` with auto-scroll support (`c10dad0`).
12. `Terminal::layout()` (cell grid, unique shape) (`fe7870f`).

**New B.3 primitives (shipped):**

- `Toast` + `ToastStack` (#141) (`ccb515f`). Corner-stacked
  notifications with severity + optional action + dismiss.
- `Spinner` + `ProgressBar` (#142) (`7ac858b`). Activity indicators
  with indeterminate/determinate modes + optional cancel.
- `Tooltip` (`0e9f817`). Anchor-relative with auto-flip placement.
- `ContextMenu` (`bd29340`). Right-click popup with separators + disabled items.
- `Completions` (`01d4fd8`). LSP-style autocomplete popup with 24-variant
  CompletionKind enum, below/above cursor placement.
- `Dialog` (`5e49853`). Title + body + buttons (horizontal or vertical).
- `Panel` (`53ed010`). Chrome (title + actions) + app-drawn content_bounds.
- `Split` (`2a86305`). Two-pane draggable divider with min-size clamping.
- `Modal` (`9cd4eb4`). Backdrop + centered content (Dialog is specialised).
- `MenuBar` (`67668d1`). Top-level menu strip with `&`-marker
  Alt-activation.
- **Form field extensions** (#143) (`da58baa`): `Slider`, `ColorPicker`,
  `Dropdown` as new `FieldKind` variants with TUI rendering.

**Intentionally skipped:** `Tabs` (redundant with `TabBar` + app
content switching), `Stack` (redundant with app render order),
`Palette` TUI consumer migration (custom chrome doesn't map cleanly;
design-first required rather than mechanical).

**TUI consumer migrations completed (6 of 9):**
- ✅ TabBar, StatusBar, TreeView, ListView, ActivityBar, Form
- ⏸ Palette (chrome-heavy, design pass needed)
- ➖ TextDisplay, Terminal (no TUI consumer yet)

**Design doc updates:**
- Closed #141, #142, #143 via `gh issue close`.
- PLAN.md readiness gate marked "CLEAR for Phase B.4 (TUI rewrite)."
- §5 migration strategy updated with B.4–B.8 sequencing.

**Aggregate:** 25+ commits landed today. Tests: 5291 → 5406 (+115).
Zero test regressions throughout. Every commit was Path-A landed
(ff-merge + push develop) after clippy-clean + full-suite-green.

**What's next:** Phase B.4 **chrome-only** (user-picked scope on
2026-04-23 end of session). Editor viewport rendering stays on the
existing `render::build_rendered_window` path. Chrome gets migrated
primitive-by-primitive:

**Dialog migration** — primitive extended with input field
(`9f24313`), ready for adapter work. Still needed: write
`quadraui_tui::draw_dialog` + replace `render_dialog_popup` in
`src/tui_main/render_impl.rs:2352` + replace the parallel
hit-test logic in `src/tui_main/mouse.rs:252`. Per-feature mapping:
vimcode's engine-side `DialogButton { label, hotkey, action }` →
quadraui's `DialogButton { id, label, is_default, is_cancel, tint }`;
`hotkey` maps to Accelerator or is handled by backend.

**Remaining substantive migrations** (each ~3–5 commits):
- Dialog (in flight)
- Context menu (vimcode's `open_editor_action_menu` → quadraui
  `ContextMenu`)
- Menu bar dropdown (`MENU_STRUCTURE` → quadraui `MenuBar` +
  `ContextMenu` composition)
- Completions popup (vimcode's `completion_display_only` flow →
  quadraui `Completions`)
- Palette (custom chrome, warrants design discussion first)

**New TUI chrome not yet in vimcode** (additions rather than
rewrites): Toast, Tooltip, Spinner, ProgressBar — wire up when a
consumer needs them (e.g. git panel progress indicator #59 → Spinner).

**Backend trait impl deferred.** Scaffolding attempted early this
session but backed out due to API-friction; the trait currently
takes `(rect, &primitive)` which doesn't quite match the practical
`(primitive, layout, frame)` pattern the existing draw functions use.
Resolving this lands alongside the event-loop rewrite; chrome-only
B.4 doesn't require it.

---

> Session 326 and earlier in **SESSION_HISTORY.md**.
