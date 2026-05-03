# VimCode Session History

Detailed per-session implementation notes archived from PROJECT_STATE.md.
All sessions through 339 archived here. Recent work summary (Sessions 341+342) in PROJECT_STATE.md.

> **Format note:** Sessions 282–339 below were archived in their
> original verbose multi-paragraph format (as maintained in
> PROJECT_STATE during the A.x / Phase 4 / Phase B / Phase C waves).
> Sessions 280 and earlier use the one-paragraph compact format.

---
**Session 339 — #259 Phase B.5c stages 1–7 shipped:**

Closed the trait-coverage gap that B.5b surfaced. Six trait-method
redesigns + two rasteriser lifts + new trait methods for the
trait-less primitives, landed Path-A in seven discrete commits.

| Stage | Commit | What |
|-------|--------|------|
| B5c.1 | `985b087` | `Backend::draw_status_bar(rect, bar) -> Vec<StatusBarHitRegion>`. Drops `&StatusBarLayout` (each backend computes layout internally). TUI rasteriser returns hit regions for trait parity. |
| B5c.2 | `b3eeadf` … `e32cc8a` | `Backend::draw_tab_bar(rect, bar, hovered_close) -> TabBarHits`. `TabBarHits` lifted to primitives so the trait can name it without feature gates. Adds `close_bounds` to hits so GTK close-hover hit-test consumes rasteriser positions instead of `chars × char_width`. TUI mouse single-group + right-click migrated to `bar.layout(...).hit_test()`. Icon glyphs in `build_tab_bar_primitive` now route through `Icon::c()` (was hardcoded). |
| B5c.3 | `92722cc` | `Backend::draw_text_display(rect, td)`. Drops `&TextDisplayLayout` (rasterisers manage line layout internally). |
| B5c.4 | `57f3d21` | TUI explorer / settings / source-control panel render helpers route tree/form draws through `Backend::draw_tree` / `draw_form` via `enter_frame_scope`. In-tree `quadraui_tui::draw_tree` / `draw_form` shims removed. |
| B5c.5 | `7558220` | Lifted `quadraui_gtk::draw_activity_bar` + `draw_terminal_cells` into `quadraui::gtk::*` (with `quadraui::Theme`). New `ActivityBarRowHit` primitive. New theme fields `inactive_fg` + `selection_bg`. New `Color::lighten`. `GtkBackend::draw_activity_bar` + `draw_terminal` no longer `unimplemented!()`. |
| B5c.6 | `a4e6c9f` | `Backend::draw_tooltip` + `Backend::draw_context_menu`. The other 4 of the original 6 trait-less primitives deferred (see below). |
| B5c.7 | this stage | Parity sweep — full test/clippy run across feature combos. `quadraui` 231/231; vimcode `--no-default-features` clean; kubeui builds clean. |

**Trait coverage state (post-B.5c):**

| Primitive | TUI | GTK | Notes |
|---|---|---|---|
| `tree`, `list`, `form`, `palette` | ✅ | ✅ | from B.5b |
| `status_bar` | ✅ | ✅ | hit regions returned (B5c.1) |
| `tab_bar` | ✅ | ✅ | `TabBarHits` returned, includes close_bounds (B5c.2) |
| `text_display` | ✅ | ✅ | layout-internal (B5c.3) |
| `activity_bar` | ⚠️ stub | ✅ | TUI inline; #266 covers TUI lift |
| `terminal` | ⚠️ stub | ✅ | TUI inline; GTK only consumer today |
| `tooltip` | ✅ | ✅ | (B5c.6) |
| `context_menu` | ✅ | ✅ | hit regions returned (B5c.6) |
| `dialog` | ❌ | ❌ | dual-Pango-layout blocker — #267 |
| `rich_text_popup`, `completions`, `find_replace` | ❌ | ❌ | rasterisers still in vimcode shims — #266 |

**Smoke-test followups filed during B5c:**
- #262 — breadcrumb dropdown: parent symbols expandable but not jumpable.
- #263 — TUI breakpoint red dot missing in gutter (pre-existing).
- #264 — Settings panel renders broken when sidebar narrow.
- #265 — TUI nerd-font wide-glyph predicate disagrees with terminal — clicks land off-target.
- #266 — Lift `rich_text_popup` / `completions` / `find_replace` into `quadraui::{tui,gtk}::*`.
- #267 — `Backend::draw_dialog` needs dual-Pango-layout handling.

**What's next:** B.5d (#260 — TUI vs GTK setup-code audit), then B.5e
(#261 — runner crates), then B.6 Win-GUI rebuild on quadraui.

---

**Session 332 (cont.) — #223 TextDisplay rasteriser pilot + kubeui YAML pane adoption:**

Tenth lift, sequenced after the per-primitive arc wrapped. This one
proves out the **kubeui-core view-builder pattern** end-to-end: the
YAML-key/value parsing logic moves into `kubeui_core::build_yaml_view`,
both kubeui binaries shrink to a bespoke title row + delegated body,
and the next external app gets the same machinery for free.

**What shipped:**

- `quadraui/src/tui/text_display.rs` —
  `pub fn quadraui::tui::draw_text_display(buf, area, display, theme)`.
  Generic per-line styled-text rasteriser; respects per-line
  `decoration` (`Error`/`Warning`/`Muted`) and per-span `fg`/`bg`
  overrides; optional `timestamp` prefix in `muted_fg`. 4 unit tests
  cover top-to-bottom paint, span fg override, auto-scroll pinning
  to bottom, zero-size guard.
- `quadraui/src/gtk/text_display.rs` —
  `pub fn quadraui::gtk::draw_text_display(cr, layout, x, y, w, h, display, theme, line_height)`.
  Mirrors the visual contract.
- `kubeui-core::build_yaml_view(state) -> TextDisplay` —
  YAML-key/value parsing extracted from both kubeui binaries into
  the shared core. Each line becomes a `TextDisplayLine` with two
  styled spans (`key:` in blue + value in default fg) or a single
  plain span when no `:` is present.
- No new `quadraui::Theme` fields needed.

**Adoption:**

- `kubeui/src/main.rs::draw_yaml` collapses to ~25 lines: bespoke
  title row (focus-dependent " YAML" / " YAML  ◀ j/k") + 1-line
  delegate to `quadraui::tui::draw_text_display` for the body.
  YAML-pane-specific `bg = (16, 18, 24)` preserved via a one-off
  `quadraui::Theme { background: bg, ..theme() }` override.
- `kubeui-gtk/src/main.rs::draw_yaml` collapses similarly. ~95 lines
  removed from kubeui binaries combined; +56 lines in kubeui-core.
  Net: shared logic moves where it belongs, binaries shrink to
  paint glue.

**Quality:** `cargo test -p quadraui --features tui --features gtk`
223/223 (4 new text_display tests on top of 219); kubeui (TUI + GTK)
build clean; clippy clean across the workspace.

**What's next:** kubeui's picker → `Palette` adoption is the
remaining same-shape lift (the picker has its own ListView shape
today; restructuring to use `Palette` is a kubeui-core view-builder
change). After that the kubeui binaries are at the irreducible
event-loop floor (~150-200 lines each) — further reduction needs
the Phase B `Backend` trait.

---

**Session 332 (cont.) — #223 ContextMenu rasteriser pilot — ARC COMPLETE:**

Ninth and final primitive in the per-primitive arc for #223. Vimcode
uses `ContextMenu` for right-click menus (file explorer, tab action
menu) and the menu-bar dropdowns (File / Edit / View / etc.).

**What shipped:**

- `quadraui/src/tui/context_menu.rs` —
  `pub fn quadraui::tui::draw_context_menu(buf, menu, layout, theme)`.
  Box-bordered popup, selected item rendered inverted (fg/bg swap),
  separator as horizontal `─` dashes, disabled items dimmed,
  shortcut text right-aligned. 4 unit tests.
- `quadraui/src/gtk/context_menu.rs` —
  `pub fn quadraui::gtk::draw_context_menu(cr, layout, menu, menu_layout, line_height, theme) -> Vec<(f64, f64, f64, f64, WidgetId)>`.
  Returns the per-clickable-item hit-rectangles + their `WidgetId`s
  so the caller's click handler can resolve mouse events without
  re-running layout. No new theme fields.

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_context_menu` collapses to a
  delegation. ~115 lines removed.
- `src/gtk/quadraui_gtk.rs::draw_context_menu` collapses to a
  delegation. ~110 lines removed.

**Quality:** `cargo test -p quadraui --features tui --features gtk`
219/219 (4 new context_menu tests on top of 215); clippy clean.

---

## #223 ARC COMPLETE — what landed today (Session 332)

**9 pilots in one session**, plus 1 pre-existing layout-regression fix:

| # | Primitive | Net lines | Theme fields added |
|---|-----------|-----------|---------------------|
| 1 | StatusBar | -169 | `background`, `foreground` (initial set) |
| 2 | TabBar    | -371 | 7 tab fields + `separator` |
| 2½ | (TabBar layout fix) | n/a | — (regression fix in primitive) |
| 3 | ListView  | -249 | 10 modal/list fields |
| 4 | TreeView  | -279 | (no new fields) |
| 5 | Palette   | -467 | `query_fg`, `match_fg` |
| 6 | Form      | -506 | `accent_fg` |
| 7 | Tooltip   | -52  | `hover_bg`, `hover_fg`, `hover_border` |
| 8 | Dialog    | -182 | `input_bg` |
| 9 | ContextMenu | -188 | (no new fields) |

**Total: 24 fields on `quadraui::Theme`. ~2,400 net lines removed
from vimcode + kubeui.** Public rasterisers in
`quadraui::{tui,gtk}::draw_*` behind `tui` / `gtk` feature gates.

**Cross-app payoff:** kubeui adopted ListView; remaining primitives
are vimcode-only consumers today but the rasterisers are ready for
external apps (Postman / SQL client / k8s dashboard / etc.) the
moment they need them.

**Issues filed during smoke-testing the arc** (all pre-existing or
out-of-scope, none introduced by the pilots):

- #225 — GTK tab switcher: rounded corners + bordered ListView support
- #226 — Right-click "Open to the Side" v-splits current tab
- #227 — GTK palette font flicker on first open
- #228 — GTK editor hover: heading bg incomplete
- #229 — GTK editor hover: scrollbar leak (right-edge specific)
- #230 — LSP "rust-analyzer..." indicator stuck
- #231 — TUI rename: tree row stale tinting after dialog closes
- #232 — Tab-click no longer highlights tree row (TUI + GTK)
- #233 — GTK Dialog square corners (cross-backend visual divergence)

**What's next** — the per-primitive arc for #223 is done. Focus
shifts to:
- Cleanup of `quadraui::Theme` field names (some are still vimcode-flavoured: `tab_*`, `hover_*`).
- File the GTK font flicker fix (#227) by setting editor monospace explicitly in vimcode wrappers.
- The smoke-test follow-up issues above.
- Optionally: kubeui adoption of more primitives (Palette / Tooltip / Dialog) if those use cases appear.

---

**Session 332 (cont.) — #223 Dialog rasteriser pilot:**

Eighth primitive lifted. Vimcode uses `Dialog` for the quit-confirm,
close-tab-confirm, and rename-input prompts.

**What shipped:**

- `quadraui/src/tui/dialog.rs` —
  `pub fn quadraui::tui::draw_dialog(buf, dialog, layout, theme)`.
  Rounded `╭─╮ ╰─╯` border + title overlay + body text + optional
  input field + button row. 4 unit tests cover: corner glyphs,
  default button uses `selected_bg`, input field gets `input_bg`
  when present, zero-size guard.
- `quadraui/src/gtk/dialog.rs` —
  `pub fn quadraui::gtk::draw_dialog(cr, body_layout, ui_layout, dialog, dialog_layout, line_height, theme) -> Vec<(f64, f64, f64, f64)>`.
  Returns the per-button hit-rectangles so vimcode's click handler
  keeps working unchanged.
- `quadraui::Theme` adds 1 field: `input_bg` (background of the
  embedded text input — distinct from `surface_bg` so the input
  reads as a separate sub-region).

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_dialog` collapses to a
  delegation. Vimcode-private `title_as_plain` helper deleted with
  it. ~125 lines removed.
- `src/gtk/quadraui_gtk.rs::draw_dialog` collapses to a delegation.
  Vimcode-private `styled_text_plain` helper deleted with it (was
  shared with the now-lifted dialog). ~120 lines removed.

**Quality:** `cargo test -p quadraui --features tui --features gtk`
215/215 (4 new dialog tests on top of 211); clippy clean across all
crate × feature combinations.

**What's next:** **ContextMenu** — last primitive in the per-primitive
arc for #223. Vimcode uses it for right-click menus and the menu-bar
dropdowns.

---

**Session 332 (cont.) — #223 Tooltip rasteriser pilot:**

Seventh primitive lifted. Vimcode uses `Tooltip` for **three** popup
surfaces: LSP hover popup, signature help, and inline diff peek
(the last two via the `styled_lines: Some(...)` multi-line styled
path).

**What shipped:**

- `quadraui/src/tui/tooltip.rs` —
  `pub fn quadraui::tui::draw_tooltip(buf, tooltip, layout, theme)`.
  Side-bar borders only (no top/bottom) — matches the visual style
  of all three vimcode tooltip consumers. 4 unit tests.
- `quadraui/src/gtk/tooltip.rs` —
  `pub fn quadraui::gtk::draw_tooltip(cr, layout, tooltip, tooltip_layout, line_height, padding_x, theme)`.
  Cairo rectangle (background fill + 1 px stroke border) + Pango
  text rendering with per-span `fg` overrides on the styled path.
- `quadraui::Theme` adds 3 fields: `hover_bg`, `hover_fg`,
  `hover_border`. Distinct from the modal-surface fields
  (`surface_bg` / `surface_fg`) so apps can tint tooltip popups
  differently from modal lists.

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_tooltip` collapses to a
  delegation. ~70 lines of vimcode-private rasterisation removed.
- `src/gtk/quadraui_gtk.rs::draw_tooltip` collapses to a
  delegation. ~85 lines removed.

**Quality:** `cargo test -p quadraui --features tui --features gtk`
211/211 (4 new tooltip tests on top of 207); full
`cargo test --no-default-features` green; clippy clean across all
crate × feature combinations.

**What's next:** **Dialog**. Vimcode uses it for confirmation popups
(quit / close-tab / generic). Then **ContextMenu** wraps the
primitive arc for #223.

---

**Session 332 (cont.) — #223 Form rasteriser pilot:**

Sixth primitive lifted. Vimcode uses `Form` for the TUI settings
panel (the GTK settings panel still uses native widgets — its
`draw_form` was already `#[allow(dead_code)]` pre-pilot, lifted
anyway because the next GTK refresh will need it).

**What shipped:**

- `quadraui/src/tui/form.rs` —
  `pub fn quadraui::tui::draw_form(buf, area, form, theme)`. Uniform
  1-cell-per-row measurer. Handles all 7 `FieldKind` variants:
  Label / Toggle / TextInput / Button / ReadOnly / Slider /
  ColorPicker / Dropdown.
- `quadraui/src/gtk/form.rs` —
  `pub fn quadraui::gtk::draw_form(cr, layout, x, y, w, h, form, theme, line_height)`.
  Per-row height `(line_height * 1.4).round()`. Slider /
  ColorPicker / Dropdown not yet rendered (matching pre-lift
  parity — GTK consumer doesn't exist).
- `quadraui::Theme` adds 1 field: `accent_fg` (active-state visual
  cue: toggle "[x]" when on, slider filled cells, button frame when
  focused). Mapped from vimcode's `theme.cursor`.

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_form` collapses to a
  delegation. ~290 lines of vimcode-private rasterisation removed.
  The vimcode-private `draw_styled_text` helper is now also dead
  code (form was its last consumer); deleted.
- `src/gtk/quadraui_gtk.rs::draw_form` collapses to a delegation.
  ~240 lines removed.

**Quality:** `cargo test -p quadraui --features tui --features gtk`
207/207 (4 new form tests on top of 203); full
`cargo test --no-default-features` green; clippy clean across
all crate × feature combinations.

**What's next:** **Tooltip**. Vimcode uses it for LSP hover popups,
signature help, and the diff peek popup. Then Dialog → ContextMenu.

---

**Session 332 (cont.) — #223 Palette rasteriser pilot:**

Fifth primitive lifted. Palette is the vimcode command palette + folder
picker (TUI + GTK). Most visually rich of the lifts so far: bordered
modal with title bar, query-input row with cursor block, separator,
scrollable item list with per-character fuzzy-match highlighting, and
a thumb scrollbar.

**What shipped:**

- `quadraui/src/tui/palette.rs` —
  `pub fn quadraui::tui::draw_palette(buf, area, palette, theme, nerd_fonts_enabled)`.
  4 unit tests cover: corner glyphs, query+prompt row layout,
  match_positions painting in `match_fg`, too-small-area no-op.
- `quadraui/src/gtk/palette.rs` —
  `pub fn quadraui::gtk::draw_palette(cr, layout, x, y, w, h, palette, theme, line_height, nerd_fonts_enabled)`.
  Cairo + Pango with per-character `AttrColor` foreground spans for
  match highlighting.
- `quadraui::Theme` adds 2 fields: `query_fg` (query text + cursor
  block fg, distinct from `surface_fg`) and `match_fg` (highlight
  colour for fuzzy-match positions).

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_palette` collapses to a
  delegation. ~250 lines of vimcode-private rasterisation removed.
- `src/gtk/quadraui_gtk.rs::draw_palette` collapses to a delegation.
  ~280 lines removed.

**Kubeui not adopted yet.** kubeui has its own picker but it builds
a `ListView` (not a `Palette`); migrating kubeui to Palette would
require restructuring the kubeui-core picker view-builder to emit
the richer `Palette` shape (with query + total_count + match
positions). Reasonable follow-up; the rasterisers are ready when
kubeui wants them.

**Quality:** `cargo test -p quadraui --features tui --features gtk`
203/203 (4 new palette tests on top of 199); full
`cargo test --no-default-features` green; clippy clean across all
combinations.

**What's next:** **Form**. Vimcode's settings panel (TUI; GTK still
on native widgets but the rasteriser exists for the eventual GTK
DrawingArea migration). Then Tooltip → Dialog → ContextMenu.

---

**Session 332 (cont.) — #223 TreeView rasteriser pilot:**

Fourth primitive lifted following the StatusBar / TabBar / ListView
template. TreeView is the most complex of the four because GTK has
**variable per-row heights** (header rows 1× line_height, leaves
1.4×) that the rasteriser supplies via the primitive's measurement
closure — the primitive itself doesn't know about font metrics.

**What shipped:**

- `quadraui/src/tui/tree.rs` —
  `pub fn quadraui::tui::draw_tree(buf, area, tree, theme, nerd_fonts_enabled)`.
  Uniform 1-cell-per-row measurer (TUI rows are always 1 cell tall).
  Reuses `draw_styled_text` lifted in pilot 3. 4 unit tests cover
  paint output with branch chevron + indented leaves, selected row
  uses `selected_bg`, header row uses `header_bg`, zero-size guard.
- `quadraui/src/gtk/tree.rs` —
  `pub fn quadraui::gtk::draw_tree(cr, layout, x, y, w, h, tree, theme, line_height, nerd_fonts_enabled)`.
  Per-row heights split at the measurer: `Decoration::Header` rows
  get `line_height`, others get `(line_height * 1.4).round()`.
  Vimcode's GTK click handlers walk the layout's per-row bounds
  (already correct from `TreeViewLayout.visible_rows`) — no click
  drift expected from the lift.
- No new `quadraui::Theme` fields needed. The TreeView rasterisers
  consume the same set ListView added (`header_bg`, `selected_bg`,
  `muted_fg`).

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_tree` collapses to a 1-line
  delegation. ~135 lines of vimcode-private rasterisation removed.
- `src/gtk/quadraui_gtk.rs::draw_tree` collapses to a 12-line
  delegation. ~180 lines of GTK-private rasterisation removed.
- Kubeui doesn't have a tree today; its `theme()` adapters
  populated the relevant Theme fields back in the ListView pilot,
  so adding a kubeui tree later means data + handlers, no
  rasteriser code.

**Quality:** `cargo test -p quadraui --features tui` 199/199 (4 new
tui::tree tests); full `cargo test --no-default-features` green;
clippy clean across vimcode (default + no-default-features) and
quadraui (`tui` + `gtk`).

**What's next:** **Palette**. Vimcode uses it for the command
palette and folder picker; kubeui has its own picker that's a
likely adoption candidate (it's a bordered modal with query +
items, exactly Palette's shape). After Palette: Form → Tooltip →
Dialog → ContextMenu.

---

**Session 332 (cont.) — #223 ListView rasteriser pilot:**

Third primitive lifted following the StatusBar + TabBar pattern. This
one is the first that hits **kubeui head-on**: kubeui (TUI) and
kubeui-gtk both have their own `draw_list` for the resource list,
and both adopt `quadraui::{tui,gtk}::draw_list` in this commit —
proving the cross-app reuse story end-to-end.

**What shipped:**

- `quadraui/src/tui/list.rs` —
  `pub fn quadraui::tui::draw_list(buf, area, list, theme, nerd_fonts_enabled)`.
  5 unit tests cover: paint output with selection marker, selection
  marker suppressed when unfocused, bordered corner glyphs, error
  decoration → `error_fg`, zero-size guard.
- `quadraui/src/gtk/list.rs` —
  `pub fn quadraui::gtk::draw_list(cr, layout, x, y, w, h, list, theme, line_height, nerd_fonts_enabled)`.
  Mirrors the TUI rasteriser's visual contract; bordered mode is
  not yet honoured (no GTK consumer needs it today, deferred as a
  follow-up).
- `quadraui::Theme` extends with 10 list-relevant fields:
  `surface_bg / surface_fg / selected_bg / border_fg / title_fg /
  header_bg / header_fg / muted_fg / error_fg / warning_fg`. Each
  has a sensible dark-palette default; vimcode populates from
  `render::Theme` (mapping `fuzzy_*`, `status_*`, `line_number_fg`,
  `diagnostic_*`); kubeui populates the subset it cares about and
  spreads `..Theme::default()` for the rest.
- `quadraui::tui::draw_styled_text` — generic helper for painting a
  `StyledText` with optional decoration override. Lifted from
  vimcode's TUI rasteriser; will be reused by future migrations
  (form / palette / tooltip).

**Adoption (vimcode + kubeui at the same time):**

- `src/tui_main/quadraui_tui.rs::draw_list` collapses to a
  delegation. ~200 lines of vimcode-private rasterisation removed.
- `src/gtk/quadraui_gtk.rs::draw_list` collapses to a delegation.
  ~170 lines removed.
- `kubeui/src/main.rs::draw_list` collapses to a 1-liner. ~60
  lines of duplicate rasterisation removed; private `put_styled`
  helper deleted.
- `kubeui-gtk/src/main.rs::draw_list` collapses to a 1-liner.
  ~50 lines of duplicate rasterisation removed; private
  `draw_styled_text` + `measure_styled` helpers deleted.
- Both kubeui binaries now have a richer `theme()` helper that
  populates the relevant new Theme fields (selected_bg,
  surface_bg, etc.).

**Net diff impact:** kubeui crates lose more lines than vimcode
because they had less of the rasteriser code already factored —
exactly the ~25% sharing-delta the kubeui spike measured. Each
primitive lift moves the percentage closer to the 90% target.

**Quality:** `cargo test -p quadraui --features tui` 195/195 (5
new tui::list tests on top of 190); full
`cargo test --no-default-features` green; clippy clean across all
four crates × both feature combinations.

**What's next:** **TreeView**. vimcode uses it for the file
explorer + git source-control panel; kubeui doesn't have one
today but plausibly grows one (e.g. resource-by-namespace
hierarchy). Tree migration is the most complex of the remaining
lifts because per-row heights vary on GTK (header rows 1× line
height, leaves 1.4×). After TreeView: Palette → Form → Tooltip →
Dialog → ContextMenu.

---

**Session 332 (cont.) — #223 TabBar rasteriser pilot + a pre-existing
layout-regression fix:**

After the StatusBar pilot landed (`develop` `1bed461`), TabBar followed
the same shape on a second branch. Smoke-testing the branch surfaced
a pre-existing `quadraui::TabBar::layout` regression — captured as
the third bullet below.

**What shipped:**

- `quadraui/src/tui/tab_bar.rs` — `pub fn quadraui::tui::draw_tab_bar(buf, area, bar, layout, theme) -> usize`
  with `pub const TAB_CLOSE_CHAR: char = '×'`. Self-contained: lifted
  `set_cell_wide` + `set_cell_styled` + `is_nerd_wide` from vimcode's
  `tui_main/mod.rs`. 4 unit tests cover paint output, return-value
  contract, segment reservation, zero-size guard.
- `quadraui/src/gtk/tab_bar.rs` — `pub fn quadraui::gtk::draw_tab_bar(...) -> TabBarHits`.
  Returns a generic per-tab + per-right-segment bounds list; vimcode's
  wrapper reshapes those into its app-specific `TabBarHitInfo`
  (diff-toolbar / split / action-menu groupings keyed by WidgetId
  strings — those stay vimcode-side, not in quadraui).
- `quadraui::Theme` extended with 7 tab fields:
  `tab_bar_bg / tab_active_bg / tab_active_fg / tab_inactive_fg /
  tab_preview_active_fg / tab_preview_inactive_fg / separator`. Each
  has a sane dark-palette default; vimcode populates from
  `render::Theme`, kubeui appends `..Theme::default()` (no tab bar
  there yet).
- **Layout regression fix in `quadraui::TabBar::layout`:** when
  `scroll_arrow_width <= 0.0` (TUI's contract — TUI doesn't paint
  scroll arrows because the engine already drives scroll via
  `Engine::ensure_active_tab_visible`), the layout used to collapse
  `resolved_scroll_offset` to `0` and clip from index 0 — silently
  dropping newly-opened tabs at the right edge. Now honours
  `bar.scroll_offset` (clamped to a valid index). Two regression tests
  added in `quadraui/src/lib.rs`. The bug pre-dated this session
  (introduced during Phase B.4 D6 migration) and would have continued
  hurting users — surfaced only because TabBar smoke-test exercised
  multi-file open in a narrow terminal.

**Adoption:**

- `src/tui_main/quadraui_tui.rs::draw_tab_bar` collapses to a 1-line
  delegation. The vimcode-private `set_cell_wide` and `TAB_CLOSE_CHAR`
  constant are deleted (helpers moved into quadraui).
- `src/gtk/quadraui_gtk.rs::draw_tab_bar` collapses to ~50 lines
  (mostly the WidgetId-based reshape from generic `TabBarHits` →
  vimcode's `TabBarHitInfo`).
- Kubeui binaries unchanged for TabBar (no tab bar in kubeui yet);
  their `theme()` helpers gained `..Default::default()` to cover the
  new Theme fields.

**Diff:** -371 net lines on adoption sites; ~470 lines added in
quadraui (the new public rasteriser modules + tests). Quality:
`cargo test -p quadraui --features tui` 190/190 (4 new TUI tab_bar
+ 2 regression for the layout fix); full `cargo test --no-default-features`
green; clippy clean across vimcode (default + no-default), quadraui
(`tui` + `gtk`), kubeui, kubeui-gtk.

**What's next:** **ListView** is the natural follow-up. Both backends
already consume `ListViewLayout` per Phase B.4. After ListView:
TreeView → Palette → Form → Tooltip → Dialog → ContextMenu. Same
per-primitive shape established by these two pilots.

---

**Session 332 — #223 StatusBar rasteriser pilot landed (lift TUI + GTK rasterisers into quadraui):**

The kubeui validation spike (Session 331-end) measured 65% code sharing between vimcode-the-editor and kubeui-the-app, with ~90% achievable if rasterisers move from vimcode-private into the public quadraui crate. This session landed the StatusBar pilot — the smallest possible end-to-end proof of the lift.

**What shipped:**

- `quadraui/src/theme.rs` — minimal backend-agnostic `Theme { background, foreground }`. Apps with rich theme systems (vimcode's `render::Theme`, kubeui's hardcoded palette) build one at the rasteriser call site. Field set grows as more primitives migrate.
- `quadraui/src/tui/{mod,status_bar}.rs` — `pub fn quadraui::tui::draw_status_bar(buf, area, bar, layout, theme)`. Self-contained: includes `set_cell` + `ratatui_color` helpers (lifted from vimcode's `src/tui_main/mod.rs`). 4 unit tests cover paint order, empty-bar fallback, bold modifier, zero-size guard.
- `quadraui/src/gtk/{mod,status_bar}.rs` — `pub fn quadraui::gtk::draw_status_bar(cr, layout, x, y, w, line_height, bar, theme) -> Vec<StatusBarHitRegion>`. Self-contained: includes `cairo_rgb` + `set_source` helpers; computes `StatusBarLayout` internally with Pango pixel measurement (16 px min gap).
- `quadraui/Cargo.toml` — new feature gates: `tui = ["dep:ratatui"]`, `gtk = ["dep:gtk4", "dep:pangocairo"]`. The legacy `gtk-example` now depends on `gtk`. Feature gates keep the data layer dep-free (apps that consume `Theme` / `StatusBar` etc. without painting don't pay for ratatui or gtk4).

**Adoption (vimcode + kubeui in the same diff):**

- `src/tui_main/quadraui_tui.rs::draw_status_bar` and `src/gtk/quadraui_gtk.rs::draw_status_bar` collapse to ~10-line wrappers that build a `quadraui::Theme` from `render::Theme` (via `to_quadraui_color`) and delegate. Caller signatures unchanged — three call sites in `src/tui_main/render_impl.rs` and three in `src/gtk/draw.rs` continue to work.
- `kubeui/src/main.rs` and `kubeui-gtk/src/main.rs` drop their private `draw_status_bar` (~25 + ~50 lines respectively) and call `quadraui::tui::draw_status_bar` / `quadraui::gtk::draw_status_bar` directly. Theme adapter is a tiny `fn theme() -> quadraui::Theme` returning the kubeui palette.

**Behavioural delta:** kubeui's old hardcoded gray-fill becomes "first segment's bg" (per the public rasteriser's contract). Visual is identical because `kubeui-core::build_status_bar` sets every segment's bg to the same gray. Vimcode behaviour is unchanged — its private rasteriser already used the same fill rule.

**Diff:** -169 net lines (108 added, 266 removed from vimcode + kubeui; 4 new files in quadraui). Kept `src/gtk/quadraui_gtk.rs::vc_to_cairo` / `qc_to_cairo` because 60+ other GTK draw functions still use them — those helpers move into quadraui as more primitives migrate.

**Quality checks:** `cargo test --no-default-features` passes; `cargo test -p quadraui --features tui` 184/184 (4 new); `cargo clippy -- -D warnings` clean across vimcode (default + no-default-features), quadraui (`tui` + `gtk`), kubeui, kubeui-gtk.

**What's next** — same per-primitive arc, with TabBar as the natural follow-up: both backends already consume `TabBarLayout` per Phase B.4, both already use the same right-segment width-fit logic, lift is mostly mechanical. After TabBar: ListView → TreeView → Palette → Form → Tooltip → Dialog → ContextMenu. Each migration is a per-primitive commit; vimcode + kubeui both adopt at the same time.

**Friction surfaced for #224 (companion follow-up):** the `gtk` feature gate adds a 3-minute first build (gtk4 / pango / cairo deps) for anyone who didn't have them cached. Not a blocker, but worth noting — apps that consume only the data layer should default to `default-features = false` once we expose one.

---

**Session 331 — Phase B.5 GTK chrome catch-up (umbrella #205 closed, 7/8 slices landed, 8 issues filed, 1 crash fixed):**

Moved GTK chrome from ~65% → ~85% on quadraui primitives. Each
slice was its own branch, smoke-tested in GTK, then Path-A landed.
The wave's defining feature: every smoke test surfaced one or more
pre-existing or freshly-uncovered bugs that the migration had to
either fix in scope or file for follow-up — making the migration's
end state honest about what's left.

**Slices landed (chronological):**

1. `e1e76cd` — slice 1 — \`draw_hover_popup\` → \`Tooltip\`. New
   \`quadraui_gtk::draw_tooltip\` rasteriser (reused by slices 2 + 3).
2. `aaa9a3c` — slice 2 — \`draw_signature_popup\` →
   \`Tooltip{styled_lines}\` (highlighted active param via per-span fg).
   Live-test gated on **#180 fix** because signature help didn't
   render server data.
3. `ead8b56` — fix(lsp) **#180**: flush dirty buffers in
   \`lsp_request_signature_help\` so server has the post-\`(\`-keystroke
   buffer state when computing the cursor position. Same #189-style
   pattern.
4. `e6650fa` — slice 3 — \`draw_diff_peek_popup\` →
   \`Tooltip{styled_lines}\` (per-line +/- colouring + action bar).
5. `7768a25` — slice 5 — \`draw_dialog_popup\` → \`Dialog\`. New
   \`quadraui_gtk::draw_dialog\` rasteriser. Returns the same
   button-rect Vec the legacy click handler consumed.
6. `7ce0f5d` — slice 6 — \`draw_context_menu_popup\` +
   \`draw_menu_dropdown\` → \`ContextMenu\`. Closes **#181**. Also fixes
   shared \`menu_dropdown_to_quadraui_context_menu\` adapter to use
   \`usize::MAX\` sentinel instead of \`unwrap_or(0)\` — affected both
   backends.
7. `caf62a8` — slice 8 — \`draw_breadcrumb_bar\` +
   \`draw_debug_toolbar\` → \`StatusBar\`. GTK debug toolbar buttons
   are clickable for the first time (legacy code only painted, no hit
   zones).
8. `6bd2039` — fix **#213**: \`Tooltip::layout\` clamped with
   \`viewport.width - vw\` as max — panicked when \`vw > viewport.width\`
   (long LSP hover content in narrow editor). Pin to viewport edge
   instead. Two regression tests in \`quadraui/src/lib.rs\`.

**Slice 4 (editor hover popup) deferred** to **#214** — needs a new
\`RichTextPopup\` primitive (markdown + tree-sitter syntax highlighting
in fenced code blocks + text selection + scroll + clickable links).
Building it correctly is its own focused wave; piecemeal-migrating
the existing renderer would split responsibility. Honest scope call.

**Issues filed during the wave** (all independent of #205, all live
for follow-up):

- #181 — closed (slice 6 fix)
- #200 — TUI ext-panel scrollbar not drawn
- #207 — GTK Shift-key in dialog activates default button
- #208 — Stale gutter diagnostics after \`git checkout\` revert
- #209 — Native-look styling for Dialog primitive
- **#210 — Motion handlers should use primitive's hit_test, not
  hand-rolled row math.** This is the structural class that caused
  multiple slice-6/8 bugs and the #211 debug-sidebar bug. Worth
  prioritising — it eliminates a whole class of "row positions
  drift between rasteriser and click handler" bugs.
- #211 — GTK debug variable tree click off-by-2
- #212 — TUI debug variables non-expandable after step
- #213 — closed (\`6bd2039\`)
- #214 — RichTextPopup primitive (deferred slice 4)

**Cross-backend wins worth noting:**

- The shared adapter sentinel fix in slice 6 affects both backends
  from one diff in \`render.rs\`.
- The Tooltip clamp fix in #213 lives entirely in
  \`quadraui/src/primitives/tooltip.rs\` — both backends benefit
  immediately.
- Five of the seven landed slices use the **same**
  \`quadraui_gtk::draw_tooltip\` / \`draw_dialog\` /
  \`draw_context_menu\` / \`draw_status_bar\` rasterisers as their
  TUI counterparts use \`quadraui_tui::draw_*\`. Bug fixes in those
  rasterisers go to one place; both backends pick them up.

---

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

**Session 325 — D7 focus model resolved + §5 migration strategy inverted:**

1. **`quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` §6.4 marked
   RESOLVED**, pointing to §9 D7 for the five sub-decisions.
2. **D7 marked RESOLVED** with all five recommendations accepted
   as-drafted:
   - D7a (transitions): **A** — click + Tab + programmatic, all
     funnelled through `set_focus(id)`.
   - D7b (destruction fallback): **B+C hybrid** — app-designated
     default, else null; next input re-establishes.
   - D7c (focusable declaration): **C** — property of the
     primitive type (`TextInput`/`ListView`/`TreeView` always
     focusable; `Toast`/`Tooltip`/`StatusBar` never; `Dialog`
     when modal).
   - D7d (modal interaction): **A** — backend-owned focus stack;
     push on modal open, pop on close.
   - D7e (native focus bridging): **C** — native focus at the
     top-level surface; simulate widget focus within.
   User explicitly noted iteration is expected on edge cases —
   the top-level shape is what's being committed to, not the
   fine-grained semantics. Marked authoritative with iteration
   allowance.
3. **§5 "Migration" entirely rewritten** for the backend-by-
   backend rewrite strategy (user flagged the "Non-negotiable:
   this must not break vimcode" language as obsolete). New
   structure: B.1 ✅ + B.2 ✅ + B.3 (ready-state quadraui) + B.4
   (TUI rewrite) + B.5 (GTK rewrite) + B.6 (Win-GUI rewrite) +
   B.7 (macOS native) + B.8 (Postman validation). Phase B.3
   builds every primitive with `layout()`; nothing in vimcode
   gets rewritten until B.3 ends. During B.4, GTK and Win-GUI
   are broken; no external users to worry about.
4. **"All decisions resolved" footer updated** — D1–D7 all
   green; next code work enumerated (TabBar::layout first, then
   Backend trait reshape, then focus-model surface, then layout
   primitives, then remaining primitives).
5. **PLAN.md updated** — D7 added to resolved list; §6.4 removed
   from open questions; readiness gate marks all design axes ✅.
6. **What this unblocks.** Phase B.3 code work starts next
   session. First move: `quadraui/src/tab_bar.rs` grows a
   `layout(viewport, measure) -> TabBarLayout` method + a
   `TabBarLayout::hit_test()` — reference implementation for
   the D6 Layout-returning shape, also closes #179.

---

**Session 324 — north-star goal + quadraui readiness gate:**

Documented the strategic inversion: coexistence rule dropped,
backend-by-backend rewrite adopted, TUI → GTK → Win-GUI → macOS
order. PLAN.md gains the north-star goal statement, backend-
state-going-in summary (TUI best, GTK/Win-GUI bug-ridden from
the coexistence-era band-aid cycle), readiness-gate checklist,
and backend rewrite order. Commit `47ab97d`.

---

**Session 323 — §6.2 resolved (Decision D6) + onboarding hooks:**

1. **`quadraui/docs/BACKEND_TRAIT_PROPOSAL.md` §6.2 marked
   RESOLVED A** (primitives return fully-resolved `Layout`;
   backends rasterise verbatim). Earlier proposal (single global
   `quadraui::layout::compute` pass) is superseded — layout is
   per-primitive.
2. **§9 gains Decision D6** with the three options considered
   (A: per-primitive `layout()`; B: behavioural primitives like
   `ScrollableTabBar`; C: required-field augmentation), why A
   won (D-001 cuts against B; C doesn't actually force backends;
   A makes wrong rendering loud, not silently wrong on platform N),
   and what lands (`TabBar::layout()` + `TabBarLayout::hit_test()`
   closes #179; `StatusBar::layout()` migration; trait reshape
   to `draw_*(&Layout)`; ~−100 LOC per backend on existing
   primitives).
3. **`PLAN.md` gains an "Architectural focus" header** at the
   top — names the active wave, lists resolved decisions
   (D1–D6), open questions (§6.3 / §6.4 / §6.5 / §6.6), and the
   ordered reading list for new sessions touching `quadraui/`.
   Solves the gap that produced today's "are you aware of the
   morning's decisions?" question.
4. **`CLAUDE.md` session-start protocol gains a quadraui step**
   — when the work touches `quadraui/`, also read
   `quadraui/docs/DECISIONS.md` and `BACKEND_TRAIT_PROPOSAL.md`
   §9. Justification: "ignoring them produces bandaid fixes that
   recur in the next backend."
5. **Doc-only commit, straight to develop** per CLAUDE.md
   workflow. No code changes; no smoke test required.
6. **What this enables.** Phase B.3 (layout primitives — `Panel`,
   `Split`, `Tabs`, `Stack`, `MenuBar`, `Modal`, `Dialog`) is
   unblocked. #178 / #179 land as part of B.3 — `TabBar::layout()`
   becomes the reference impl for the new shape. Existing
   primitives gain `layout()` incrementally as their backends
   are touched.

---

**Session 322 (cont.) — A.6d-win v2a attempted, reverted, blocked on #178:**

1. **Migration attempt** (branch `quadraui-phase-a6d-tab-bar-win-v2`,
   commit `1c052c0`): routed Win-GUI's diff toolbar through
   `quadraui::TabBar.right_segments`, deleted the bespoke
   `draw_diff_toolbar_in_tab_bar`, replaced the cached-button-positions
   pipeline with `quadraui_win::compute_diff_toolbar_positions`.
   Originally meant to close #177 (1-cell hit zones).
2. **Regression discovered during smoke test**: the prev/next arrow
   glyphs render as `?` in Win-GUI. Root cause: `build_tab_bar_primitive`
   uses Material Design Icon PUA codepoints (`U+F0143` ↑, `U+F0140` ↓,
   `U+F0233` ≡); pre-migration code hardcoded BMP fallbacks (`U+2191`,
   `U+2193`, `U+2261`) which Consolas can render. Win-GUI's mono
   DirectWrite path has no font fallback configured — GTK only "works"
   because Pango falls back to Symbols Nerd Font automatically.
3. **Reverted, branch discarded** — never merged to develop. #177 stays
   open. Filed #178 for the Nerd Font cross-backend design issue and
   #179 for the related tab-bar-overflow parity gap (both backends drop
   tabs silently when bar is too narrow).
4. **Lesson** (broader than this stage): the quadraui crate's promise
   is "test in one backend, ship everywhere" — but primitives that
   emit codepoints implicitly assume a Nerd-Font-capable rendering
   layer that not every backend provides. Future right-segment
   migrations should wait for #178's design conversation before
   reattempting v2.

**Session 322 (cont.) — Phase A.6d-win v1 Win-GUI tab bar (tabs only):**

1. **`quadraui_win::draw_tab_bar` rasteriser** (`src/win_gui/quadraui_win.rs:+110`).
   Renders the tabs portion of a `quadraui::TabBar` primitive — per-tab
   background fill, dirty (●) / close (×) glyph, top accent strip on the
   active tab in focused groups, vertical separator. Italic preview tabs
   not supported (Win-GUI text path is single-format); preview tabs render
   with a dimmer foreground colour, matching pre-migration behaviour.
2. **Two wrappers refactored.** `draw_tab_bar` (single-group) and
   `draw_group_tab_bar` (per-group split) in `src/win_gui/draw.rs` now
   build the primitive via `render::build_tab_bar_primitive` (with no
   diff toolbar / no split buttons / scroll_offset = 0 for v1) and call
   the rasteriser. The legacy `draw_tabs` method (~80 lines) deleted.
3. **`draw_ui_text` and `measure_ui_text` exposed as `pub(super)`** so
   the rasteriser can render the proportional UI font (Segoe UI) for
   tab labels, matching the pre-migration look.
4. **Tab dimensions match exactly** (`TAB_PAD_PX = 12`) so the existing
   `state.tab_slots` click-cache populated in `cache_layout` stays valid
   bit-for-bit. Tab clicks and dirty-or-close glyph clicks unchanged.
5. **v2 scope deferred** (separate PR): right-segments stream
   (diff toolbar / split buttons / action menu unified into
   `bar.right_segments`), `fit_active_scroll_offset` for proper tab
   scrolling, close-button hover state, `TabBarHitInfo` return value to
   replace the parallel cache pipeline. ~250 LOC.
6. **Three smoke-test paper-cuts surfaced, filed as separate issues:**
   #176 (cmd window flashes briefly on file open — full audit attached;
   no obvious missing `CREATE_NO_WINDOW`), #177 (diff toolbar prev/next/
   fold buttons have a 1-cell hit zone but render with 3-cell visual
   stride — one-line fix specified; will be obviated by v2's
   right-segments unification), and a vague z-order issue noted but not
   filed pending specifics from user.

**Session 322 — Phase A.6b-win Win-GUI status bar via quadraui:**

1. **`quadraui_win::draw_status_bar` rasteriser** (`src/win_gui/quadraui_win.rs:+95`).
   Direct2D / DirectWrite counterpart to `quadraui_gtk::draw_status_bar`:
   per-segment background fills, left-segments-from-left + right-segments-
   right-aligned layout, `quadraui::StatusBar::fit_right_start_chars` policy
   so low-priority right segments drop cleanly when the bar is too narrow
   (#159 fix; previously they overflowed past the right edge).
2. **Two call sites refactored.** Per-window status bar inside
   `draw_editor_window` (`src/win_gui/draw.rs:696`, ~40 LOC of bespoke
   segment-fill code deleted) and `draw_separated_status_line`
   (`src/win_gui/draw.rs:920`, ~30 LOC deleted) both build the primitive
   via `render::window_status_line_to_status_bar` and call the new
   rasteriser. The global bottom bar (`draw_status_bar`, plain text only,
   only fires when per-window mode is OFF) was left as-is — no parity gap.
3. **Hit-test stays in lockstep.** `win_status_segment_hit_test`
   (`src/win_gui/mod.rs:6597`) now applies the same
   `fit_right_start_chars(width, MIN_GAP_CHARS=2)` so clicks on a dropped
   right segment cannot fire. Same primitive + same policy on both sides
   (rasteriser + hit-test) keeps them aligned without a cached zone map.
4. **Bold attribute deferred.** Win-GUI's text path uses a single non-bold
   `IDWriteTextFormat`; supporting bold needs a second format and would
   make the hit-test diverge from the rasteriser (proportional vs char-
   count widths). Pre-A.6b code didn't honour bold either. Documented in
   the rasteriser doc comment.
5. **Pre-existing clippy unblock** (separate commit `ce1f500`): six
   `collapsible_match` warnings introduced by the Rust 1.95.0 toolchain
   bump in `dap_ops.rs`, `ext_panel.rs`, `keys.rs`, `motions.rs`,
   `vscode.rs`, `lsp.rs`. Mechanical `cargo clippy --fix`. Was blocking
   the A.6b-win quality gate.
6. **Two settings paper-cuts surfaced during smoke test, filed as
   separate issues:** #173 (`status_line_above_terminal` label is
   inverted from its behaviour — `true` keeps bars *inside* windows, not
   above the terminal) and #174 (`:set window_status_line` rejected
   because the ex command only accepts vim-style `:set wsl`). Both
   pre-existing, neither blocks A.6b-win.

**Session 321 — Phase B.2 terminal-maximize accelerator migration:**

1. **Engine-owned accelerator registry.** New `src/core/engine/mod.rs`
   types + methods: `RegisteredAccelerator { acc, parsed }`,
   `UiEventContext { terminal_cols, terminal_max_rows }`, and on
   `Engine`: `accelerators: Vec<RegisteredAccelerator>` field;
   `register_accelerator`, `unregister_accelerator`,
   `match_accelerator`, `handle_ui_event`,
   `register_default_accelerators`. Re-exports
   `quadraui::{Accelerator, AcceleratorId, AcceleratorScope,
   KeyBinding, UiEvent}`. `Engine::new()` registers
   `"terminal.toggle_maximize"` from
   `settings.panel_keys.toggle_terminal_maximize`.
2. **Departs from §11 Q3.** The "backend owns the event loop" shape
   was more invasive than one accelerator justified (~400 LOC of
   back-translation for keys not yet migrated). Final shape:
   engine owns the registry; backends call
   `engine.match_accelerator(...)` synchronously from existing
   key handlers. Same B.1 types exercised; zero event-loop
   disruption. Backend-owned events can land in B.4 when
   accelerator count grows.
3. **Six sites migrated.** `src/tui_main/mod.rs:2888` (terminal-panel
   early-intercept) and `:3586` (EngineAction arm). `src/gtk/mod.rs:
   1386` (EventControllerKey closure) and `:7219`
   (`Msg::ToggleTerminalMaximize` handler). `src/win_gui/mod.rs:1832`
   (WndProc cascade) and `:4586` + `:6178` (EngineAction handlers).
   Each `matches_*_key(&pk.toggle_terminal_maximize, ...)` + per-
   backend flip+resize sequence collapses to
   `engine.match_accelerator(...)` + `engine.handle_ui_event(...)`.
4. **`EngineAction::ToggleTerminalMaximize` kept.** The ex command
   `:TerminalMaximize` and toolbar click still return this action;
   their handlers just route through `engine.handle_ui_event` now.
   Full collapse is B.4 work.
5. **10 new integration tests** in `tests/accelerator_registry.rs`:
   default registration, match positive/negative, case
   insensitivity, toggle + idempotent re-toggle,
   unknown-accelerator no-op, re-register-same-id-replaces,
   unregister-removes, non-Global-scope filtering. Workspace total
   5295 → 5305.
6. **`src/lib.rs` re-exports `quadraui`** so integration tests and
   future downstream consumers pin to the version vimcode is built
   against.
7. **§11 updated** with "B.2 implementation notes" subsection
   documenting the engine-owned vs backend-owned choice + rationale.
   PLAN.md stage table marks B.2 Done.
8. **Quality gates all pass.** `cargo fmt`, `cargo clippy
   --no-default-features -- -D warnings`, `cargo clippy -- -D
   warnings` (GTK), full `cargo test --workspace
   --no-default-features` 5305/0/19. Win-GUI syntax manually
   reviewed (cargo check --features win-gui fails on Linux due to
   pre-existing `windows-future-0.2.1` incompat; user must verify
   Windows build).
9. **Net diff:** +339 / –74 across 6 files. Payoff materialises at
   accelerator #2: each new binding adds ~1 line per backend.
10. **Awaiting smoke test.** Verify: Ctrl+Shift+T still toggles
    terminal maximize in TUI (kitty / modern alacritty without tmux,
    since tmux strips Shift bit per §11 spike findings) and GTK.
    `:TerminalMaximize` ex command still works. Toolbar maximize/
    unmaximize button still works.
11. **Path B landing.** Branch `quadraui-phase-b2-maximize-pilot`
    off develop; PR expected after smoke test.

---

**Session 319 — Phase B.1 Backend trait scaffolding (#169 blocker):**

1. **Pure additive quadraui types.** Three new files — `quadraui/src/event.rs`, `quadraui/src/accelerator.rs`, `quadraui/src/backend.rs` — plus re-exports in `lib.rs`. No vimcode runtime changes; no migration. The abstractions coexist with existing per-backend dispatch as designed in `BACKEND_TRAIT_PROPOSAL.md` §5 Phase B.1.
2. **`UiEvent` enum (~60 variants/fields).** Backend-neutral event data covering input (`Accelerator`, `KeyPressed`, `CharTyped`), mouse (`MouseDown/Up/Moved/Entered/Left/DoubleClick/Scroll`), window (`WindowResized/Close/Focused/DpiChanged`), files (`FilesDropped/ClipboardPaste`), primitive-specific events re-wrapped (`Tree`, `List`, `Form`, `Palette`, `TabBar`, `StatusBar`, `ActivityBar`, `Terminal`, `TextDisplay`), and `BackendNative` escape hatch. All `Debug + Clone + PartialEq + Serialize + Deserialize` per §2 invariants. Supporting types: `Key`/`NamedKey`, `MouseButton`, `ButtonMask`, `Point`, `Rect`, `ScrollDelta`, `Viewport`, `BackendNativeEvent`.
3. **`Accelerator` + `KeyBinding` + dual-format parser.** 13 universal bindings (`Save`, `Copy`, `Undo`, `Find`, ...) that render platform-appropriately, plus `KeyBinding::Literal(String)` that accepts **both** vim-style (`<C-S-t>`) and plus-style (`Ctrl+Shift+T`) input — first character dispatches. `AcceleratorScope` variants for `Global`/`Widget`/`Mode`. `Platform` enum and `render_accelerator`/`render_binding` helpers produce `⌘⇧T` on macOS, `Ctrl+Shift+T` elsewhere.
4. **`Backend` trait + `PlatformServices` trait.** Frame lifecycle (`begin_frame`/`end_frame`), event polling (`poll_events`/`wait_events`), accelerator registration (`register_accelerator`/`unregister_accelerator`), services access, and **9 per-primitive draw methods** (`draw_tree`/`draw_list`/`draw_form`/.../`draw_text_display`) per Decision 2 (B). No `AnyPrimitive` enum. `Clipboard` sub-trait, `FileDialogOptions`, `Notification` support types.
5. **22 new lib tests in `accelerator.rs`** covering both parser formats, modifier aliases (`Cmd`/`Command`/`Super`/`Win`/`Meta`), case-insensitivity, rejection cases, render platform-parity, serde round-trip. Quadraui test count: 24 → 46. Workspace total: 5273 → 5295.
6. **Documentation updates.** `PLAN.md` stage table gains Phase B.1 (Done) + B.2/B.3/B.4/B.5 rows; `PROJECT_STATE.md` session note.
7. **Quality gates all pass.** `cargo fmt` clean; `cargo clippy --workspace --no-default-features -- -D warnings` clean; `cargo test --workspace --no-default-features` 5295/0/19.
8. **What this PR does NOT do.** Zero vimcode runtime change. No existing code migrated. No behaviour change for users. Pure additive — the new types sit unused until Phase B.2 (terminal maximize pilot migration) lands.
9. **Path B landing.** Branch `quadraui-phase-b1-backend-trait` off develop; merged via PR #170 at `06dec4a` on 2026-04-22.
10. **Next session — sketch before code.** Phase B.2 (terminal-maximize pilot migration) needs 5 design questions answered in `BACKEND_TRAIT_PROPOSAL.md` §11 **before** touching code. See `PLAN.md` §"Phase B.2 starting notes" for the full list (TuiBackend struct shape, event translation algorithm, main-loop integration, GTK event ownership, Win-GUI message-pump hookup), realistic scope (~+250/-75 LOC, not the aspirational -60), and workflow reminders.

---

**Session 318 — Closing the "where does app logic go?" gap:**

1. **User feedback after #34:** the terminal maximize wave landed
   logic across 10 files with 61 references to the maximize helpers —
   most of it duplicated plumbing across three backends (target-rows
   math, keybinding intercept, resize handler, hit-test). User asked
   whether it's too ambitious to expect a cross-platform UI crate to
   abstract more of this away. Answer: it's the stated vision, but a
   Phase A / Phase B roadmap gap. Addressed via docs + one shared
   helper; larger abstractions (layout primitives, `Backend` trait)
   stay parked for Phase B.
2. **New doc `quadraui/docs/APP_ARCHITECTURE.md`** (~220 lines) —
   sibling to `UI_CRATE_DESIGN.md` (vision) and
   `docs/NATIVE_GUI_LESSONS.md` (backend implementer). Audience is
   the app developer. Covers: the layer cake
   (Engine → render adapter → primitive → backend draw), a
   "where does each kind of thing go?" table, a full worked example
   tracing the 11-commit maximize ship through every layer, six
   rules-of-thumb distilled from maximize + earlier lessons, and an
   11-question checklist for new features. Links back to `PLAN.md`
   lessons and the reference commits (`5bcb1bd`, `1d7141a`, `507d63a`).
3. **New shared helper `PanelChromeDesc`** in `src/core/engine/mod.rs`
   (near `EngineAction`): row-unit struct with fields for
   `viewport_rows`, `menu_rows`, `quickfix_rows`, `debug_toolbar_rows`,
   `wildmenu_rows`, `tab_bar_rows`, `separated_status_rows`,
   `status_cmd_rows`, `panel_chrome_rows`, `min_content_rows`, and a
   single method `max_panel_content_rows()` that does the
   saturating-subtract + clamp. Backends fill the struct in their
   own native units (TUI cell count; GTK `da_height / line_height`;
   Win-GUI `client_height_px / line_height`). Five lib-tests cover
   typical TUI, full chrome, min-floor clamp, zero-min clamp, and
   default construction (`test_panel_chrome_*`).
4. **Backend rewiring:**
   - `tui_main::terminal_target_maximize_rows_tui` shrinks to a
     `PanelChromeDesc { … }.max_panel_content_rows()` call.
   - `gtk::gtk_terminal_target_maximize_rows` does the same; the
     `1.6 * line_height` tab row rounds up to 2 row-units (≤0.4 lh
     slack, absorbed by the subsequent clamp).
   - `win_gui::win_gui_terminal_target_maximize_rows` is new (extracted
     from three inline `total_rows.saturating_sub(3).max(5)` copies at
     the keyboard, action-dispatch, and toolbar-click sites). ~20 lines
     of duplicated arithmetic deleted across the three backends.
5. **PLAN.md "Lessons learned"** gains three entries:
   - "Render-time effective values beat mutation-at-toggle-time" (rule
     + `5bcb1bd` commit reference).
   - "Mouse hit-tests mirror draw-time geometry" (rule + `1d7141a` +
     `507d63a` commit references).
   - "Chrome arithmetic belongs in the engine, not in each backend"
     (rule + `PanelChromeDesc` reference).
6. **Quality gates all pass.** `cargo fmt` clean; `cargo clippy
   --no-default-features -- -D warnings` clean; `cargo clippy
   -- -D warnings` (GTK) clean; full workspace test 5273/0/19. All
   six `tests/terminal_maximize.rs` integration tests continue to
   pass unchanged — the refactor is strictly internal.
7. **Net diff:** ~+340 / –85 across 6 files. Biggest add:
   `APP_ARCHITECTURE.md` (new). Struct + method: ~100 lines in
   `engine/mod.rs`. Tests: ~75 lines. Backend rewiring is net
   negative (deletes arithmetic; adds struct-literal + call).
8. **Path B landing.** Branch `followup-chrome-helper` off develop;
   quality-gated and awaiting smoke test — no runtime behaviour
   changes but the maximize path is on the refactored code now.

---

**Session 317 — Terminal maximize (closes #34):**

1. **New Engine state:** `terminal_maximized: bool` + `terminal_saved_rows: u16` on Engine (transient — not persisted in `SessionState`). Initialised to false/0 in `Engine::new()`.
2. **New method `Engine::toggle_terminal_maximize(target_rows: u16)`** in `src/core/engine/terminal_ops.rs`: on maximize saves `session.terminal_panel_rows` into `terminal_saved_rows`, grows the panel to `target_rows` (floor 5, only grows — never shrinks below current), opens the terminal if closed, grabs focus. On un-maximize restores the saved rows. The backend is responsible for computing `target_rows` from its own viewport geometry.
3. **Auto-restore on close:** `close_terminal()` now clears `terminal_maximized` and restores `terminal_saved_rows` if maximize was active. Prevents a "stuck maximized" state after reopening the panel.
4. **New ex command** `:TerminalMaximize` / `:TerminalMax` returns new `EngineAction::ToggleTerminalMaximize`. Backends compute viewport rows + forward the action to `toggle_terminal_maximize`.
5. **New panel keybinding** `panel_keys.toggle_terminal_maximize` (default `<C-S-t>`, VSCode parity with "Maximize Panel Size"). Added to `Settings::PanelKeys` with `pk_toggle_terminal_maximize()` default. Three backends all bind it: TUI (main event loop, `matches_tui_key`), GTK (`Msg::ToggleTerminalMaximize` routed via `matches_gtk_key`), Win-GUI (inline `ctrl && shift && key.key_name == "t"` check in the Win32 keyboard dispatcher).
6. **Viewport computation:** each backend computes target rows as `total_rows.saturating_sub(chrome).max(5)` where chrome reserves space for status + cmd + panel tab-bar + header. TUI uses the crossterm `terminal.size()`; GTK uses the DrawingArea height + `cached_line_height` via new `App::terminal_target_maximize_rows()`; Win-GUI uses `GetClientRect` + `state.line_height`.
7. **PTY resize:** every maximize/unmaximize path calls `engine.terminal_resize(cols, new_rows)` (or `terminal_new_tab` if no pane exists) so the shell receives SIGWINCH and reflows.
8. **6 new integration tests** in `tests/terminal_maximize.rs` cover: flag set + rows grown; unmaximize restores saved rows; target below saved keeps saved (monotone grow); close while maximized restores; ex command returns the correct `EngineAction`; minimum floor of 5 rows. Total workspace tests 5263 (vimcode 5239 + quadraui 24); baseline at branch-off was 5257.
9. **Docs:** README "Integrated Terminal" section gained Ctrl-Shift-T + `:TerminalMaximize` bullets, plus a command-mode table row. Win-GUI caveat noted (binding works; the standalone fallback action handler may paint one frame behind the key-path handler because it lacks direct `GetClientRect` access).
10. **Quality gates all pass:** `cargo fmt`, `cargo clippy --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK), full `cargo test --no-default-features`. Win-GUI code paths follow existing backend patterns; `cargo build --features win-gui` isn't buildable on Linux (`windows-future` crate incompat, pre-existing) so smoke-test on a Windows machine.
11. **Toolbar button added (follow-up commit).** `TerminalPanel.maximized: bool` on render.rs; new `󰊗` (maximize) / `󰊓` (unmaximize) Nerd Font icons drawn between the split and close buttons in all three backends. Click handlers: TUI `src/tui_main/mouse.rs` header-row hit test, GTK `src/gtk/mod.rs` terminal panel click branch (new `max_x` rect), Win-GUI `src/win_gui/mod.rs` toolbar click routing. ASCII fallback icons: `]` / `[` for Win-GUI when Nerd Fonts disabled. Split/add/close button positions all shifted 2 cols leftward to make room. New `UiAction::TerminalMaximizeButton` variant registered in `all_required_ui_actions()` + TUI + Win-GUI collect lists; parity tests still pass.
12. **Path B landing.** Branch `issue-34-terminal-maximize` off develop; PR expected after smoke test.

---

**Session 316 — Documentation: status-bar notifications (closes #156); diff-view alignment bug filed (#166):**

1. **README.md — new "Status-bar notifications" subsection** under the status-line area (just before the "Font" bullet). Documents the spinner-vs-bell indicator: animated Braille spinner (`⠋⠙⠹…`) in function color while in-progress, `󰂞` (Nerd Font) / `*` (ASCII) in string-literal color when done. Covers the three real triggers currently wired up — LSP/DAP server install, project-wide search, project-wide replace — with their actual in-progress and done messages. Calls out the 5-second auto-dismiss and click-to-dismiss behaviour. Notes that the spinner is not clickable (informational only).
2. **Accuracy check:** the issue body listed `GitOperation`, `LspIndexing`, `ExtensionInstall` as triggers, but `grep` shows those `NotificationKind` variants are defined in the enum yet **never passed to `notify()`** in live code (only `LspInstall`, `ProjectSearch`, `ProjectReplace` are). Doc only describes what's real.
3. **Issue #166 filed — side-by-side diff pane drift past the first hunk.** Root-cause analysis: `Engine::sync_scroll_binds()` (`src/core/engine/search.rs:277`) maps active→partner through `diff_aligned`, but then stores the partner's scroll as a **buffer line** rather than an **aligned-row index**. When the partner's aligned entry at the mapped index is a padding row, the fallback walks forward to the next real `source_line`, so the partner skips past padding the active keeps emitting. Every hunk compounds the drift. Fix sketches included: (1) treat `scroll_top` as an aligned-row index for diff-pair windows, (2) back `target_idx` up to the start of a padding run before translating to a buffer line, (3) render-side fallback that lands on the first aligned entry (padding) when multiple match a given `source_line`.
4. **Path A (docs-only)** — README.md + PROJECT_STATE.md committed directly to `develop` per the CLAUDE.md documentation-only-change rule.
5. **#34 (terminal maximize) is next** — Path B (branch + PR). Will land separately.

---

**Session 315 — Phase A.2c: Win-GUI explorer migration:**

1. **Last required Win-GUI quadraui stage done.** A.2c completes the
   originally-required platform-specific stages (A.1b/A.1c/A.2b/A.2c).
   Optional A.6*/A.7 Win-GUI parity stages remain queued under
   "Win-GUI parity scope" in PLAN.md but are not blockers for the
   wave.
2. **Shared adapter promoted to `render.rs`.** `ExplorerRow` and
   `explorer_to_tree_view()` move out of `src/gtk/explorer.rs` into
   `src/render.rs` so both GTK and Win-GUI can call the same builder.
   The adapter takes `(rows: &[ExplorerRow], scroll_top, selected,
   has_focus, engine)` — backend-neutral. GTK's `ExplorerState` now
   wraps the shared `render::ExplorerRow`; `src/gtk/explorer.rs`
   retains the state type + tree-walk helpers but re-exports the
   adapter's row shape.
3. **Win-GUI bespoke explorer render deleted.** `src/win_gui/draw.rs::
   draw_explorer_panel` shrinks from ~75 lines of hand-rolled per-row
   Cairo-style drawing (indent math, expand-arrow glyphs, file-color
   fg pick) down to a ~20-line wrapper: draw the "EXPLORER" header,
   call `render::explorer_to_tree_view`, delegate to
   `quadraui_win::draw_tree` (introduced in A.1c).
4. **Parity boost as a side-effect.** The shared adapter pulls in git
   status letters + LSP diagnostic badges per row (via
   `engine.explorer_indicators()`) — things the old Win-GUI explorer
   didn't display. The primitive's uniform `line_height` rows keep
   the flat-row click-hit math `(row - 1)` intact; no changes needed
   in `src/win_gui/mod.rs`.
5. **EXPLORER header kept outside the primitive.** Same pattern as
   A.1c's SC panel: the panel header is a bespoke row above the tree,
   and the tree starts at `top + line_height` with full remaining
   height. Lets the click-hit math "subtract 1 for the header" stay
   unchanged from pre-migration.
6. **`WinSidebar.rows` type changes.** Local `ExplorerRow` deleted in
   `src/win_gui/mod.rs`; replaced with `use crate::render::
   ExplorerRow;`. `build_rows()` + `collect_explorer_rows()` still
   live locally (they construct the shared type).
7. **Quality gates all pass.** `cargo fmt` clean; `cargo clippy
   --no-default-features -- -D warnings` clean; `cargo clippy
   --features win-gui --no-default-features` shows only the 10
   pre-existing `collapsible_match` warnings that Session 314 already
   noted. Full `cargo test --workspace --no-default-features`:
   5235/0/19 (identical to Session 314 baseline). `cargo build --bin
   vimcode-win --features win-gui --no-default-features` succeeds.
8. **Net diff:** ~+145 / –115 across 5 files. `src/render.rs` gains
   ~110 lines (the new `ExplorerRow` type + adapter). `src/gtk/
   explorer.rs` shrinks ~90 lines (local adapter removed, replaced
   by a re-export + pointer comment). `src/gtk/mod.rs` call-site
   updates to the new signature. `src/win_gui/mod.rs` swaps the
   struct def for a `use`. `src/win_gui/draw.rs` shrinks on
   `draw_explorer_panel`.
9. **Awaiting smoke test on Windows.** Verify: EXPLORER header still
   renders at the top of the panel with foreground text. Tree rows
   show folder/file icons per extension (Nerd Font when available,
   fallback "." otherwise). Git-modified files show an `M`/`A`/`D`/
   `?` badge on the right edge; files with LSP errors show a red
   count badge; warnings show a yellow count badge. Selected row
   gets the `fuzzy_selected_bg` fill — same as the SC panel rows.
   j/k/space/Enter navigation + click + double-click behaviour
   unchanged (click math in `src/win_gui/mod.rs` is untouched).
   Right-click context menu still fires on file / folder rows.

---

**Session 314 — Phase A.1c: Win-GUI `draw_tree` + SC panel migration (commit `25e94f8`):**

1. **First primitive-driven rendering in the Win-GUI backend.** A.1c
   was the last required Windows stage tracked in PLAN; A.2c (Win-GUI
   explorer) is the only Win-GUI required stage still open. Optional
   A.6/A.7 Win-GUI parity stages remain queued under "Win-GUI parity
   scope" in PLAN.md.
2. **New `src/win_gui/quadraui_win.rs`** — Direct2D/DirectWrite
   counterpart to `quadraui_tui::draw_tree` (TUI) and
   `quadraui_gtk::draw_tree` (GTK). ~195 lines. Renders tree bg,
   per-row bg (header / muted / selection / default), indent +
   chevron for branches, optional icon, text spans with per-span fg,
   right-aligned badge with reserve width, span truncation to badge
   edge.
3. **Win-GUI SC panel sections loop deleted** — `draw.rs::draw_git_panel`
   shrank by ~160 lines (the 4-section staged/unstaged/worktrees/log
   render loop). Replaced with
   `render::source_control_to_tree_view(sc, self.theme)` +
   `quadraui_win::draw_tree(self, panel_x, top+ry, panel_w,
   sections_h, &sc_tree)`. `add_color`/`del_color`/`mod_color` theme
   bindings dropped from `draw_git_panel` — they're now encoded in
   the adapter via `theme.git_added/deleted/modified`.
4. **Row-height decision: uniform `line_height` (Win-GUI)**, not the
   `line_height` / `1.4 * line_height` split GTK uses. Preserves the
   pre-migration Win-GUI monospace cadence so the click-hit math in
   `src/win_gui/mod.rs` (mouse-y / lh → flat row index) works
   without modification. Recorded as a lesson in PLAN.md: different
   backends can make different pixel-level decisions; the primitive
   only constrains data, not layout.
5. **`DrawContext` helper visibility.** Three private methods
   (`draw_text`, `mono_text_width`, `solid_brush`) promoted to
   `pub(super)` so the sibling `quadraui_win` module can reach them.
   Matches how GTK's `quadraui_gtk` accesses `super::*`.
6. **Scrollbar kept.** Total content height computed from
   `sc_tree.rows.len() * lh` + commit rows + button row, so the
   existing "thumb-without-offset" scrollbar indicator still draws
   at the right size.
7. **Quality gates all pass** — `cargo fmt` clean, `cargo clippy
   --no-default-features -- -D warnings` clean (no warnings from
   A.1c code), `cargo test --workspace --no-default-features`
   5235/0/19 (identical to develop baseline), `cargo build --bin
   vimcode-win --features win-gui --no-default-features` succeeds.
8. **Known pre-existing issue not touched.** `cargo clippy --features
   win-gui --no-default-features -- -D warnings` has 10 pre-existing
   `collapsible_match` errors on develop (in `core/engine/vscode.rs`,
   `core/lsp.rs`, `win_gui/mod.rs:2170` and others). Unrelated to
   A.1c; should be filed as a separate housekeeping issue if desired.
9. **Net diff:** +213 / –178 lines across 3 files (`win_gui/mod.rs`
   adds the `pub mod quadraui_win` line; `win_gui/draw.rs` loses the
   section render loop + the 3 unused theme bindings; new
   `win_gui/quadraui_win.rs` at 196 lines).
10. **Awaiting smoke test on Windows.** Verify the four sections
    (STAGED CHANGES / CHANGES / WORKTREES if >1 / RECENT COMMITS)
    render with expand chevrons, item-count badges, status-bg
    styling on header rows. j/k moves selection across flat rows
    with inverted bg highlight. Enter/Tab/s on files still
    stages/unstages — click-hit math is unchanged because row height
    stays at lh. Muted decoration on log entries renders in dim_fg.
    Branch picker popup overlay unaffected.

---

**Session 312 — Phase A.8: `TextDisplay` primitive scaffolding (A.9 deferred):**

1. **Strategic decision: A.9 deferred indefinitely.** A.9 (`TextEditor` + `BufferView` adapter) would be a mechanical refactor of vimcode's editor surface through quadraui — zero functional benefit to vimcode users, ~thousands of lines, highest regression risk. The primitive only matters for downstream apps that want to embed a code-editor widget (SQL client #46, k8s dashboard #145, Postman clone #147). Until those apps materialise, A.9 is technical debt with no near-term payoff. PLAN.md stage table marks it Deferred.
2. **A.8 scope kept small.** No vimcode consumer of `TextDisplay` exists yet — AI chat streaming, project search results, find-replace results, etc. all work fine on existing scratch-buffer / List-primitive plumbing. A.8 ships scaffolding-only: primitive types + serde + 3 lib tests, no backend draw functions and no migration. First consumer (k8s pod-log viewer per #144, or LSP trace tail) drives the backend work as A.8b/A.8c.
3. **New primitive `quadraui::primitives::text_display`** — `TextDisplay { id, lines, scroll_offset, auto_scroll, max_lines, has_focus }`, `TextDisplayLine { spans, decoration, timestamp }`, `TextDisplayEvent` (`Scrolled` / `AutoScrollToggled` / `Copied` / `KeyPressed`).
4. **Streaming-friendly API on the primitive itself:** `TextDisplay::new(id)` constructor + `append_line(line)` + `clear()` + `set_max_lines(n)` helpers. `max_lines = 0` means unbounded; positive cap evicts oldest lines via `Vec::drain(..)` when exceeded, with `scroll_offset` adjusted so the visible region stays anchored. Supports the 10k-lines/sec target from #144 — the primitive's append is `Vec::push` + bounded eviction; cost is amortised `O(1)`. Whether the backends can render that fast is the actual benchmark question; deferred to when a backend exists.
5. **3 new quadraui lib tests** — `append_line` + cap eviction + clear + scroll-offset adjustment, serde round-trip on the full primitive (with mixed decorations + timestamps + spans), `TextDisplayEvent` variants.
6. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK), full `cargo test --workspace --no-default-features` (5249/0/19, vimcode 5225 unchanged, quadraui 21→24), `cargo build` both configurations.
7. **Net diff:** +250 / –5 lines across 4 files. New `quadraui/src/primitives/text_display.rs` is ~115 lines including doc comments. Vimcode source untouched.
8. **Quadraui v1 status.** With A.8 scaffolded, the crate now has 9 primitives: Tree, Form, Palette, List, StatusBar, TabBar, ActivityBar, Terminal, TextDisplay. Only `TextEditor` (A.9, deferred) is missing from the design doc's v1 list. The crate is "extracted enough" for vimcode; further work waits for downstream-app demand.

---

**Session 311 — Phase A.7: `Terminal` primitive + TUI + GTK cell migration:**

1. **New primitive `quadraui::primitives::terminal`** — `Terminal { id, cells: Vec<Vec<TerminalCell>> }`, `TerminalCell { ch, fg, bg, bold, italic, underline, selected, is_cursor, is_find_match, is_find_active }`, `TerminalEvent` (`KeyPressed` / `SelectStart` / `SelectExtend` / `SelectEnd` / `Scroll`). The cell layout mirrors `render::TerminalCell` 1:1 so the adapter is a tight inner loop, not a structural transform.
2. **Scope kept narrow.** Only the **per-cell rendering** (the meat of terminal output) goes through the primitive in this stage. Terminal tabs (`TERMINAL` / `DEBUG CONSOLE`), the close/split/new-tab toolbar buttons, and scrollbar drawing remain on bespoke per-backend code. Migrating those is queued as A.7b if useful — they're more about backend-specific chrome (tooltips, hover) than reusable UI primitives.
3. **Adapter `render::terminal_cells_to_quadraui`** in `src/render.rs` — converts `&[Vec<render::TerminalCell>]` → `quadraui::Terminal` once per terminal per frame. Used by both backends.
4. **TUI: build-once dispatch.** `render_terminal_panel` now constructs the `quadraui::Terminal` primitive once before the row loop (separately for the split-pane left/right cases), then `render_terminal_pane_cells` becomes a thin per-row dispatcher into `quadraui_tui::draw_terminal_row(buf, &cells_row, …)`. Avoids N allocations per frame for an N-row terminal.
5. **GTK: thin wrapper.** `src/gtk/draw.rs::draw_terminal_cells` reduces to ~25 lines that build the primitive and delegate to `quadraui_gtk::draw_terminal_cells`. Cell paint (per-cell bg + fg + Pango attrs) lives in the quadraui module.
6. **Performance characteristic.** A typical terminal pane is ~30 rows × ~120 cols = ~3,600 cells. Per-frame `quadraui::Terminal` allocation copies ~150 KB of `TerminalCell` data — well within a single 16ms frame budget on modern CPUs (memcpy clocks at GB/s). If profiling later shows this is hot, the adapter can be reworked to construct lazily / cache between frames; for now the simple owned-data path matches the plugin invariants without measurable overhead.
7. **Wide-glyph behaviour preserved.** Both backends call the same per-cell loop they did before; nothing in this migration changes how `set_cell_wide` / Pango width measurement is invoked. The primitive layer doesn't introduce its own width logic.
8. **2 new quadraui lib tests** — serde round-trip on `Terminal` (with cursor + selection + find overlays) and `TerminalEvent` variants.
9. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK; clippy::duplicated_attributes flagged a stray `#[allow]` left over during scaffolding — removed), full `cargo test --workspace --no-default-features` (5246/0/19, vimcode 5225 unchanged, quadraui 19→21), `cargo build` both configurations.
10. **Net diff:** +320 / –180 lines across 6 files. `src/gtk/draw.rs::draw_terminal_cells` shrinks ~80 lines; `src/tui_main/panels.rs::render_terminal_pane_cells` shrinks ~40 lines plus a small refactor at the call site to build the primitive once. Quadraui gains a 75-line primitive module + a ~95-line GTK draw function + a ~50-line TUI draw function.
11. **Awaiting smoke test.** TUI + GTK terminal output should render identically — characters, fg/bg, bold/italic/underline attributes, cursor (inverted bg/fg), mouse selection (theme selection bg), find matches (orange / amber). Test with `:terminal` then run a colourful command (e.g. `ls --color=auto`, `htop`, `bat src/render.rs`) and verify rendering matches pre-migration. Selection drag, scrollback (Ctrl+PageUp / Ctrl+PageDown), find-in-terminal (Ctrl+F) all should work since they don't go through this code path.

---

**Session 310 — Phase A.6f: GTK ActivityBar native→DrawingArea migration (A.6 complete):**

1. **Atomic switchover.** The view! macro's `activity_bar` `gtk4::Box` with 7 fixed `gtk4::Button` widgets + inline `Separator` spacer is now a single `gtk4::DrawingArea { set_width_request: 48, set_has_tooltip: true, set_can_focus: true }`. Follows the A.2b-2 + A.3c-2 pattern.
2. **New `quadraui_gtk::draw_activity_bar`** — Cairo + Pango renderer that consumes a `quadraui::ActivityBar` + extra `hovered_idx` param. Returns `Vec<ActivityBarHit>` in DA-local pixel coordinates for the caller's click + hover + tooltip pipeline. Geometry: 48 px per row (matches the pre-migration `set_height_request: 48`), icons centred in each row at 20 px Nerd Font size (matches the `.activity-button` CSS), 2 px left-edge accent bar for active rows, subtle hover-bg tint.
3. **Dynamic extension button injection block deleted** (~35 lines). Extension panel icons now flow through the same primitive + draw path as the fixed panels; adding an extension panel just appears as a new `ActivityItem` the next time the DA redraws. Simpler than the old `insert_child_after` bookkeeping.
4. **GTK-specific adapter `build_gtk_activity_bar_primitive`** in `src/gtk/mod.rs`. Builds the primitive from `Engine.ext_panels` + the current `SidebarPanel`. Tooltips populated (unlike TUI which leaves them empty): "Explorer (Ctrl+Shift+E)", "Search (Ctrl+Shift+F)", etc. GTK has no keyboard-focused highlight (native widgets manage tab nav), so all `is_keyboard_selected` are false.
5. **`activity_id_to_panel` decoder** — `WidgetId::as_str()` → `SidebarPanel`, including `"activity:ext:<name>"` → `SidebarPanel::ExtPanel(name)`. Click handler dispatches via this decoder to `Msg::SwitchPanel`, keeping the engine-side dispatch path unchanged.
6. **Interaction wiring**: `GestureClick` resolves rows via the stored `activity_bar_hits` vec. `EventControllerMotion` updates a hover `Rc<Cell<Option<usize>>>` and calls `queue_draw` when the hovered row changes (also on `leave`). `connect_query_tooltip` fires the native GTK tooltip popover using per-row `tooltip` strings. `Msg::SwitchPanel` handler now mirrors the active panel into a shared `Rc<RefCell<SidebarPanel>>` the draw callback reads without borrowing `&self`, and queues a redraw.
7. **Lessons learned (added to PLAN.md):** for atomic switchover of native widget chains to DrawingArea, the pattern is (a) shared interaction state lives in `Rc<RefCell<_>>` / `Rc<Cell<_>>` so draw callbacks don't borrow `&self`, (b) per-frame interaction state (hover row, active selection mirror) gets written synchronously from the interaction handler that changes it, paired with a `queue_draw`, (c) deferred poll-tick dispatch should be avoided for anything affecting visual state (cf. #158's pre-existing tab-scroll lag).
8. **All A.6 stages now complete.** Quadraui primitives shipped: Tree (A.1b), Form (A.3c/A.3c-2), Palette (A.4b), List (A.5b), StatusBar (A.6b), TabBar (A.6d), ActivityBar (A.6f). All Linux GTK migrations done. Windows (A.1c / A.2c) remain for a machine with that toolchain.
9. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK; `clippy::explicit_counter_loop` flagged a manual row-index counter I'd written — simplified to `.enumerate()`), full `cargo test --workspace --no-default-features` (5244/0/19, unchanged from A.6e), `cargo build` both configurations.
10. **Net diff:** +380 / –160 lines across 4 files. `src/gtk/mod.rs` shrinks by ~100 lines (native button chain + dynamic injection deleted; imperative DA setup added); `src/gtk/quadraui_gtk.rs` grows by ~160 (`draw_activity_bar` + `ActivityBarHit`).
11. **Awaiting smoke test.** GTK activity bar should render identically: 7 fixed icons (hamburger was TUI-only — the GTK version had no hamburger row, stays that way), dynamic extension panel icons in the middle, settings pinned bottom. Hover a row → subtle tint + native tooltip popover after dwell. Click any icon → panel switches + active-accent bar appears on the left edge of that row. Opening a new extension panel → its icon appears on the next redraw.

---

**Session 309 — Phase A.6e: `ActivityBar` primitive + TUI migration:**

1. **Scope decision.** A.6 was originally planned with A.6e as a combined TUI + GTK slice. Looking at the GTK activity bar (a `gtk4::Box` with 7 native `gtk4::Button` widgets plus dynamic extension panels added via `insert_child_after`), migrating to a primitive-backed DrawingArea would be another A.2b-2-scale atomic rewrite (click + hover + tooltip + focus + dynamic rebuild). Extended the split pattern: A.6e is TUI-only; GTK lands as A.6f.
2. **New primitive `quadraui::primitives::activity_bar`** — `ActivityBar { id, top_items, bottom_items, active_accent, selection_bg }`; `ActivityItem { id, icon, tooltip, is_active, is_keyboard_selected }`; `ActivityBarEvent` with `ItemClicked` + `KeyPressed`. Top items render from the top downward; bottom items pin to the bottom and win if the area is too small to fit both.
3. **TUI `quadraui_tui::draw_activity_bar`** — one row per item, icon at `area.x + 1` (leaving the left column for the `▎` accent bar when active). Active-without-keyboard-selection gets the accent; keyboard-selected gets a full-row selection-bg fill that takes precedence over the accent. Matches the previous bespoke renderer exactly.
4. **Tooltip field added but unused by TUI.** TUI has no hover UI at the character-cell level; the field is carried for the A.6f GTK migration where `set_tooltip_text` on each row will consume it.
5. **`build_activity_bar_primitive` in `src/tui_main/panels.rs`** — builds the declarative state from `TuiSidebar` + `Engine` + theme. Preserves the existing keyboard-selection index mapping (0 = hamburger, 1-6 = fixed panels, 7 = settings, 8+ = dynamically-registered extension panels) so `toolbar_selected` bookkeeping in `mod.rs` is unchanged. Click resolution stays on the existing row-arithmetic path.
6. **`TuiPanel` enum branch list made explicit.** The adapter's `match panel` has arms for all 6 real panels plus a `_` fallback. Rust's exhaustiveness check will flag new TuiPanel variants that need an icon + tooltip in the adapter.
7. **2 new quadraui lib tests** — serde round-trip on `ActivityBar` (top + bottom items, accent + selection bg), `ActivityBarEvent` variants.
8. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK; clippy::needless_borrows flagged a `&format!(…)` → `format!(…)` simplification), full `cargo test --workspace --no-default-features` (5244/0/19, vimcode 5225 unchanged, quadraui 17→19), `cargo build` both configurations.
9. **Net diff:** +260 / –100 lines across 5 files. `src/tui_main/panels.rs::render_activity_bar` shrinks from ~110 lines to ~10 plus a ~90-line adapter helper (most of the bulk is the `ActivityItem` construction for each of the 8 fixed + N dynamic rows).
10. **Awaiting smoke test.** TUI activity bar should render identically: hamburger at top, Explorer/Search/Debug/Git/Extensions/AI rows, extension panel icons below, settings pinned at bottom, `▎` accent on active item, selection-bg fill on keyboard-focused item, no hover affordance (TUI doesn't track per-cell mouse hover).

---

**Session 308 — Phase A.6d: GTK `draw_tab_bar` migration:**

1. **Shared adapter promoted to `render.rs`** — `build_tab_bar_primitive` (previously TUI-local) moved to `src/render.rs` so both backends use the same primitive construction. Accepts `Option<quadraui::Color>` for the accent; each backend converts its own colour type up-front. TUI's ratatui→quadraui conversion happens in `render_tab_bar`; GTK's uses the new `render::to_quadraui_color` helper on its `render::Color` theme field.
2. **New `quadraui_gtk::draw_tab_bar`** — Cairo+Pango renderer that consumes a `quadraui::TabBar` + an extra GTK-only per-frame `hovered_close_tab: Option<usize>`. Preserves all GTK visual details: 1.6× line_height tab row, sans-serif UI font (separate from editor monospace), italic-on-preview font, 2px top accent bar on active tab, rounded hover background behind close button, ● vs × close glyph.
3. **New `TabBarHitInfo` struct** replacing the bespoke 5-tuple return type. Same data (per-tab slots, diff button rects, split info, action menu rect, available char columns) but named fields. GTK `draw.rs::draw_tab_bar` wrapper flattens it back to the legacy `TabBarDrawResult` tuple so the click-dispatch path (`src/gtk/click.rs`, `src/gtk/mod.rs`) stays untouched.
4. **Rendering-vs-interaction split pattern.** The primitive is pure declarative state (tabs + their visual flags + right segments + accent). GTK's `draw_tab_bar` accepts per-frame interaction state (`hovered_close_tab`) as an extra parameter alongside the primitive. Captured in PLAN.md "Lessons learned" for future primitives that need hover / drag / focus overlays.
5. **Right-segment dispatch by `WidgetId`.** The GTK renderer walks `bar.right_segments` and classifies clickable segments by their `id.as_str()` (`"tab:diff_prev"`, `"tab:split_right"`, etc.) so it can populate the legacy per-button hit-region tuple without re-duplicating the layout logic. Same WidgetId→enum mapping pattern as A.6a's `status_action_from_id`.
6. **GTK `draw_tab_bar` wrapper reduced to ~40 lines.** The 350-line Cairo+Pango routine (tab measurement, per-tab paint, hover affordance, right-button layout) is gone. The wrapper adapts colour, builds the primitive, delegates, unpacks `TabBarHitInfo` into the legacy tuple.
7. **All Linux GTK tab-bar / status-bar / primitive migrations are now done.** Primitives shipped: Tree (A.1b), Form (A.3c/A.3c-2), Palette (A.4b), List (A.5b), StatusBar (A.6b), TabBar (A.6d). Remaining quadraui work on Linux: ActivityBar (A.6e), Terminal (A.7), TextDisplay (A.8), TextEditor (A.9).
8. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK; `clippy::if_same_then_else` flagged a pre-existing `if A { foreground } else if B { foreground }` branch I'd preserved from the old code — simplified to `if A || B`), full `cargo test --workspace --no-default-features` (5242/0/19, unchanged), `cargo build` both configurations.
9. **Net diff:** +480 / –430 lines across 4 files. `src/gtk/draw.rs` shrinks ~300 lines (the tab-bar renderer extracted out); `src/gtk/quadraui_gtk.rs` grows by ~340. `src/render.rs` gains the shared ~85-line adapter; `src/tui_main/render_impl.rs` shrinks by ~85 lines (now just calls the shared adapter).
10. **Awaiting smoke test.** GTK tab bar should render identically — tab padding (14px outer + 10px inner gap), 2px top accent on active tab, italic preview, hover rounded bg on close, dirty dot ● / close ×, split+diff+action right buttons, correct hit regions so clicks still dispatch through the unchanged `src/gtk/click.rs` path.

---

**Session 307 — Phase A.6c: `TabBar` primitive + TUI migration:**

1. **New primitive `quadraui::primitives::tab_bar`** — declarative `TabBar { id, tabs, scroll_offset, right_segments, active_accent }`; `TabItem { label, is_active, is_dirty, is_preview }`; `TabBarSegment { text, width_cells, id: Option<WidgetId>, is_active }`; `TabBarEvent` with `TabActivated`, `TabClosed`, `ButtonClicked`, `KeyPressed`.
2. **Rendering-only migration.** Unlike `StatusBar` (A.6a), which routed clicks through the primitive via `WidgetId` encoding, `TabBar` keeps the click path on vimcode's engine-side `TabBarClickTarget` enum because it has parameterised actions (`Tab(usize)`, `CloseTab(usize)`). The primitive's events + IDs exist for future plugin-declared tab bars (§10 invariants) but vimcode's click resolution still goes through the cached `GroupTabBar.hit_regions`.
3. **TUI `quadraui_tui::draw_tab_bar`** — renders a `TabBar` into a ratatui `Buffer`, returns available tab-content width. Preserves every visual detail of the old `render_tab_bar`: dirty dot `●` vs close `×`, underline-accent on the filename portion only (chars after the last `": "`), italic for preview tabs, bold+underline for active. Right segments support mixed labels (diff "2 of 5") and clickable icon buttons. Nerd Font wide glyphs (`F0932`/`F0143`/`F0140`/`F0233`) use `set_cell_wide`; other PUA glyphs (`F0D7` for split-down) use regular `set_cell` to match pre-migration per-cell output.
4. **TUI `render_tab_bar` reduced to a 12-line wrapper.** Builds the primitive via a new `build_tab_bar_primitive` helper (local to `render_impl.rs`), delegates to `draw_tab_bar`. External signature unchanged — all callers and the tab-bar scroll-offset bookkeeping are untouched.
5. **Wide-glyph heuristic lesson.** First draft used a broad "PUA = wide" check that failed 6 snapshot tests: `SPLIT_DOWN` at `\u{f0d7}` is PUA but renders as 1 cell in practice. Narrowed the heuristic to an explicit allowlist of the 4 wide glyphs vimcode actually uses. Future wide-glyph additions must be added to `is_nerd_wide`. Recorded the lesson in PLAN.md.
6. **2 new quadraui lib tests** — serde round-trip on `TabBar` (with tabs, right segments of mixed clickable/label, active accent), and `TabBarEvent` variants.
7. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK), full `cargo test --workspace --no-default-features` (5242/0/19; vimcode 5225 unchanged, quadraui 15 → 17), `cargo build` (both default + `--no-default-features`).
8. **Net diff:** +300 / –170 lines across 5 files.
9. **Awaiting smoke test.** TUI tab bar should render identically — close/dirty indicators, prefix-vs-filename underline split, italic-on-preview, split/diff/action right-side buttons with correct wide-glyph handling. All tab/close/split/diff/action clicks still dispatch via the unchanged engine-level path.

---

**Session 306 — Phase A.6b: GTK `draw_status_bar` migration:**

1. **New `quadraui_gtk::draw_status_bar`** in `src/gtk/quadraui_gtk.rs` — Cairo + Pango counterpart to the TUI renderer shipped in A.6a. Same contract: background fill from first segment's `bg`, left segments accumulate from `x`, right segments right-aligned inside `width`, per-segment bold via `pango::Weight::Bold`. Returns `Vec<StatusBarHitRegion>` in bar-local coordinates so the caller can populate the per-window click map.
2. **GTK `draw_window_status_bar` reduced to ~20 lines.** The 100-line Cairo+Pango routine (segment measure / fill / stroke / weight-attrs, duplicated for left and right halves) is gone. The wrapper now adapts `WindowStatusLine → quadraui::StatusBar` via the A.6a adapter, calls `quadraui_gtk::draw_status_bar`, and decodes the returned `WidgetId`s back to `StatusAction` for the existing per-window `status_segment_map`. Click dispatch in `src/gtk/click.rs` is unchanged.
3. **Hit-region float→u16 conversion** in the GTK backend uses a saturating clamp — GTK bars render in pixels (typically up to ~2500 px wide at 4K), well within `u16::MAX = 65535`. The TUI variant uses character cells; both fit. A later stage may widen `StatusBarHitRegion::col` / `width` to `u32` if HiDPI bars ever approach the limit.
4. **All Linux GTK primitives are now migrated through quadraui** — Tree (A.1b), Form (A.3c/A.3c-2), Palette (A.4b), List (A.5b), StatusBar (A.6b). The GTK backend's primitive-independent rendering surface shrinks another ~80 lines; what's left in `src/gtk/draw.rs` is editor / popup / sidebar-chrome rendering.
5. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK), full `cargo test --workspace --no-default-features` (5240/0/19, unchanged), `cargo build` (both default + `--no-default-features`).
6. **Net diff:** +140 / –99 lines across 4 files. `src/gtk/quadraui_gtk.rs` grows with `draw_status_bar`; `src/gtk/draw.rs` shrinks where the inline routine used to live.
7. **Awaiting smoke test.** GTK per-window status line should render identically to before: same colours, widths, bold weights, right-alignment of the right half, clickable segments, separated-status-line row above the terminal panel. Focus specifically on the narrow-window edge case flagged during A.6a smoke testing (#157 if filed) — behaviour there is unchanged by this migration but worth confirming the primitive doesn't make it worse.

---

**Session 305 — Phase A.6a: `StatusBar` primitive + TUI per-window status line migration:**

1. **Scope decision.** PLAN.md had A.6 as a single "StatusBar / TabBar / ActivityBar finish" stage. Split into five sub-phases (A.6a–A.6e) matching the A.4/A.4b and A.5/A.5b cadence, so each primitive + backend migration is an independent smoke-testable slice rather than a large three-primitive diff.
2. **New primitive `quadraui::primitives::status_bar`** — `StatusBar { id, left_segments, right_segments }`, `StatusBarSegment { text, fg, bg, bold, action_id: Option<WidgetId> }`, `StatusBarHitRegion`, `StatusBarEvent { SegmentClicked, KeyPressed }`. `StatusBar::hit_regions(bar_width)` + `resolve_click(col, bar_width)` live on the primitive for backend-neutral click resolution.
3. **Engine-agnostic action encoding.** Per plugin invariant §10 ("primitives don't borrow app state"), quadraui can't see vimcode's `StatusAction` enum. Added `render::status_action_id(&StatusAction) -> &'static str` + `status_action_from_id(&str) -> Option<StatusAction>` so the adapter encodes engine enum → opaque `WidgetId` string (`"status:goto_line"`, etc.) and the click handler decodes back. Similar pattern will apply to TabBar and ActivityBar.
4. **New adapter `render::window_status_line_to_status_bar()`** — converts the existing `WindowStatusLine` (built by `build_window_status_line`, Session 241–243) into a `quadraui::StatusBar`, flattening `StatusAction` to `WidgetId` via the encoder.
5. **TUI `quadraui_tui::draw_status_bar`** — renders `StatusBar` into a ratatui `Buffer` with the same pixel-for-pixel behaviour as the old `render_window_status_line`: background fill from first segment's `bg`, left segments from left edge, right segments right-aligned, per-segment bold attribute. Both the old global status bar and per-window status bars now flow through this function.
6. **TUI `render_window_status_line` reduced to 12 lines** — builds the primitive via the adapter and delegates to `draw_status_bar`. The previous ~60-line Cairo-like closure-based implementation is gone.
7. **TUI click path `status_segment_hit_test` migrated** — now builds the primitive, calls `StatusBar::resolve_click`, decodes the returned `WidgetId` via `status_action_from_id`. External signature preserved (`WindowStatusLine` in, `Option<StatusAction>` out) so the ~20 callsites in `mouse.rs` are untouched.
8. **GTK not yet migrated.** GTK still uses its own Pango-based `draw_window_status_bar` with `WindowStatusLine` directly. Tracked as A.6b.
9. **2 new quadraui lib tests** — serde round-trip on `StatusBar` with interactive segments, and `hit_regions` + `resolve_click` on a mixed-side bar. Existing vimcode test count unchanged (5225) — the migration is a refactor with no behavioural change in any integration test.
10. **Quality gates all pass** — `cargo fmt`, `cargo clippy --workspace --no-default-features -- -D warnings`, `cargo clippy -- -D warnings` (GTK), `cargo test --workspace --no-default-features` (5240/0/19, vimcode 5225 unchanged + quadraui 15 including 2 new), `cargo build` (both default + `--no-default-features`).
11. **Net diff:** +296 / –52 lines across 8 files. `src/tui_main/render_impl.rs` shrinks by ~47 lines as the inline renderer becomes a delegation.
12. **Awaiting smoke test.** TUI per-window status line should render identically and all clickable segments (mode / filename / cursor / language / indent / line ending / encoding / branch / LSP / toggles / notifications) must still open their existing pickers.

---

**Session 304 — Fix #154: startup tree-sitter parse on large files:**

1. **Root cause.** `BufferState::update_syntax` unconditionally called
   `Syntax::parse(&text)` at file-open time. On 100k+-line generated
   files (Cargo.lock, logs) this blocked the main thread for 5–10s
   during session restore. Discovered while investigating #153 (idle
   CPU); the startup spike was independent of the idle loops.
2. **New setting `syntax_max_lines` (default 20_000)** — matches
   VSCode's tokenization cutoff. Buffers over the threshold render
   as plain text. `BufferState::update_syntax` now short-circuits
   before `syn.parse()` when `buffer.content.len_lines() > limit`;
   `self.syntax` stays installed so raising the limit and calling
   `update_syntax` again re-enables highlighting without reopening.
3. **Threshold source** — module-level `AtomicUsize SYNTAX_MAX_LINES`
   in `buffer_manager.rs` (mirrors `session::suppress_disk_saves`
   `AtomicBool` pattern). Seeded by `Engine::new` from settings and
   resynced by `Settings::set_value_str` whenever the value changes
   via `:set` or the settings-panel form. Avoids threading `max_lines`
   through 20+ `update_syntax` callsites.
4. **Re-parse on setting change.** `execute_command` (for `:set`) and
   `ext_panel.rs` settings-form `Return` handler both call
   `update_syntax()` after writing the new value, so toggling the
   limit takes effect immediately on the active buffer.
5. **Testability.** `update_syntax` split into a pure
   `update_syntax_with_limit(max_lines)` plus a facade that reads the
   atomic. The gate-logic test uses `_with_limit` directly to avoid
   racing on the process-wide atomic with parallel `Engine::new`
   calls in other tests. Atomic-sync path is covered implicitly by
   every test that opens a file via `Engine`.
6. **Tests.** 1 new lib test in `buffer_manager::tests` covering:
   small buffer → highlights populate, huge buffer at low threshold
   → parse skipped + highlights empty + syntax still installed,
   raising threshold → re-parse re-enables highlighting.
7. **Quality gates all pass** — `cargo fmt` (my files clean; pre-existing
   `spell.rs` diff on develop unchanged), `cargo clippy -- -D warnings`
   (default + `--no-default-features`), full `cargo test --no-default-features`
   (5225 / 0 / 19 vs. baseline 5223), `cargo build` (default +
   `--no-default-features`). **Net diff:** +136 / –1 lines across 5
   files (buffer_manager, execute, ext_panel, engine/mod, settings).
8. **Awaiting smoke test.** Repro from #154: open vimcode on a saved
   session that includes a 100k+-line file (e.g. a big workspace's
   Cargo.lock). Expect instant startup. `:set syntax_max_lines?`
   confirms the threshold; `:set syntax_max_lines=500000` re-enables
   highlighting on the same buffer.

---

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
3. **`quadraui/docs/DECISIONS.md`** — new running decision log
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

1. **Design doc `quadraui/docs/UI_CRATE_DESIGN.md` finalised** — captures the full plan for extracting a `quadraui` crate supporting Windows (Direct2D), Linux (GTK4), macOS (Core Graphics, v1.x), and TUI (ratatui) backends. vimcode becomes the first test app; other keyboard-driven apps (SQL client, k8s dashboard) are the second-wave consumers that prove the abstraction.
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

---



**Session 280 — Fix 6 Vim deviations (#28-#33), Neovim conformance harness:**
Fix #31 (2d2w count multiplication), #32 (<G send_keys parser), #30 (di</da< angle brackets), #29 (da"/da' trailing whitespace), #28 (d}/d{ paragraph boundary), #33 (c+Esc cursor). Neovim conformance test harness: 31 automated tests comparing VimCode vs Neovim headless.

**Session 279 — Vim conformance matrix tests, `:set` option audit:**
Operator × motion matrix tests (Phase 1): 29 new parametric tests with 93 total test cases covering d/c/y/>>/<</ gU/gu/g~ × motions + text objects, count variations, dot-repeat. Test infrastructure: `send_keys(engine, "d2w")` helper. Bug fixes: ge/gE motion (rewrote backward word-end), leader key intercepting df<space>, dw/de/db dot-repeat, send_keys `<<` parsing. `:set` option audit (Phase 2): 18 tests for round-trip, behavior, and ex-command handling. 6 Vim deviations documented as GitHub issues (#28-#33). 47 new tests (total: 1463).

**Session 278 — Find/replace hit regions, colorcolumn, `x`+`.` fix, CLAUDE.md rules:**
Centralized find/replace hit-test geometry — `FindReplaceClickTarget` enum (13 variants), `FrHitRegion` struct, `compute_find_replace_hit_regions()` in `engine/mod.rs`. All 3 backends use shared hit regions. Shared click dispatch via `Engine::handle_find_replace_click()`. TUI mouse rewrite with hit-region-based dispatch, drag-to-select, double-click word select. GTK click handler fixed (3 geometry mismatches). Win-GUI migrated to shared dispatch. Visual selection preserved during Ctrl+F. Dynamic panel width. `:set colorcolumn` implemented with parsing, derived theme color, 3-backend rendering. `x` count+`.` repeat fixed. CLAUDE.md rules elevated. Crate extraction + Vim conformance roadmap items. 27 new tests.

**Session 277 — Visual-mode `:` with `'<,'>` range prefix:**
Pressing `:` in Visual, VisualLine, or VisualBlock mode enters Command mode with `'<,'>` pre-populated in the command buffer. `command_from_visual: Option<Mode>` on Engine tracks originating visual mode; `build_selection()` renders highlight during command input; selection cleared on Escape/Enter. Status line shows COMMAND. 6 new tests.

**Session 276 — Unified find/replace overlay (Ctrl+F):**
Engine-level `FindReplacePanel` in `ScreenLayout`, rendered identically by all 3 backends (GTK Cairo, TUI ratatui, Win-GUI Direct2D). Replaces GTK-only Revealer find dialog. VSCode-style layout: find row with toggles (Aa/ab/.*), match count, nav buttons, close button; replace row with replace/replace-all buttons. Features: incremental search, case/whole-word/regex/preserve-case/in-selection toggles, chevron expand/collapse, `ctrl_f_action` setting, Ctrl+Z undo passthrough, Ctrl+A select-all in inputs, visual selection pre-fill, regex multiline mode, Edit menu integration. Win-GUI mouse interactions (drag-select, double-click word, cached `FindReplaceRect`). Nerd Font icons with ASCII fallbacks. 13 new tests.

**Session 275 — Win-GUI horizontal scrollbar, bundled Nerd Font, Phase 2c verification:**
Win-GUI horizontal scrollbar drag (draw + click + drag + `scroll_left` text offset + text-area clip rect). Bundled Nerd Font via DirectWrite (per-user font install + registry + `icon_text_format`). Phase 2c source-code verification test (grep Win-GUI source for 26 required engine method calls). 1 new test.

**Session 274 — Phase 2d behavioral parity tests, clippy CI fix:**
Phase 2d behavioral backend parity tests — 16 new end-to-end tests in `render.rs` simulating user interaction sequences (tab click/close, context menus, double-click, hover lifecycle, sidebar focus, terminal ops, tab drag-drop, preview promotion). Clippy CI fix (`needless_return` in `lsp_manager.rs`). Updated `/complete-push` command to require clippy on all feature configs.

**Session 273 — Windows LSP fix, extension install fixes, Win-GUI hover:**
Critical LSP fix: `path_to_uri` produced backslash URIs (`file://C:\path`) instead of RFC 3986 (`file:///C:/path`), breaking all LSP on Windows. Win-GUI hover: added `editor_hover_mouse_move()` + `poll_editor_hover()`. Extension install fixes: Win-GUI `pending_terminal_command`, PowerShell terminal wrapper, `&&`→`;`, rustup proxy detection (`cargo_bin_probe_ok`), rust-analyzer install via `rustup component add`, install spinner clear, `lsp_did_open` skip when install pending.

## Session 272 — Win-GUI git panel rendering parity + tab scroll-into-view
Win-GUI git panel full renderer rewrite (~300 lines): themed header, commit input box, button row (Commit/Push/Pull/Sync with hover), 4 collapsible sections, selection highlight, file status coloring, scrollbar, branch picker popup, help dialog. Click interactivity: section items, commit input, buttons, double-click-to-open-diff. Button hover tracking. Panel hover dwell for commit log popups. Tab bar scroll-into-view fix (`set_tab_visible_count` reporting).

## Session 271 — Win-GUI extension panels + Nerd Font auto-detect + breadcrumb/tooltip fixes
Win-GUI extension panel support: full `draw_ext_panel()` renderer (header, search input, flat rows with sections/items/badges/actions, scrollbar, help popup), activity bar ext panel icons (Segoe MDL2 mappings), click/keyboard/scroll handlers, `ext_panel_focus_pending` polling.
Rendering fixes: breadcrumb UNC prefix (`?C:`), tab tooltip UNC prefix, empty breadcrumb covering tab bar, diff toolbar tab overlap.
Nerd Font auto-detect: `detect_nerd_font_windows()`, default `use_nerd_fonts=false` on Windows, TUI activity bar `icons::` fallback-aware calls, startup warning message.

## Session 270 — Win-GUI bug blitz (9 fixes — panel routing, resize, subprocess, nav)
1. `hidden_command()` helper — CREATE_NO_WINDOW for curl calls (6 sites in registry.rs + ai.rs)
2. Full NCHITTEST for all 8 resize zones
3. Search panel keyboard routing (on_key_down + on_char)
4. AI panel keyboard routing via handle_ai_panel_key()
5. Git panel keyboard routing via handle_sc_key()
6. Nav arrow clicks (tab_nav_back/forward) + command center click
7. Panel-specific scroll wheel routing
8. Search/Debug focus on activity bar click
9. Search panel draw stub replaced with full renderer

## Session 269 — Win-GUI interaction parity (19 fixes)
New features: terminal regains focus, breadcrumb clicks, group divider drag, diff toolbar buttons, tab tooltip, terminal selection/paste/copy, extension panel keyboard + double-click.
Bugs: UNC path prefix, clipboard sync, tab close/click geometry, context menu hover, tab slot bounds overflow, menu bar hit-test, insert mode paste, generic sidebar key swallowing. Systematic audit found 4 additional bug classes. Updated NATIVE_GUI_LESSONS.md.

## Session 268 — Win-GUI bug fixes (16 items — systematic audit + user-reported bugs)
1. Tab close dirty check — shows engine dialog for unsaved buffers
2. Picker mouse interaction — click result to select, click outside to dismiss, scroll wheel navigates
3. Dialog button clicks — full button rect hit-testing, outside-click dismisses
4. QuitWithUnsaved handling — shows confirmation dialog, WM_CLOSE checks for unsaved changes
5. Fold-aware scrolling — uses scroll_down_visible()/scroll_up_visible()
6. Picker scroll interception — scroll wheel checks picker_open first
7. VSCode selection clear on click — calls vscode_clear_selection() before mouse_click
8. Cursor kept in viewport after scroll — clamped with scrolloff, calls sync_scroll_binds()
9. Terminal tab switching by mouse — matches tab label geometry from draw code
10. Tabs disappear with second editor group — breadcrumb offset in draw + click + drag overlay
11. Terminal steals keyboard focus — terminal_has_focus cleared on editor click
12. Tab accent only on active group — show_accent parameter, is_active_group check
13. Explorer preview investigated — working correctly (expected VSCode behavior)
14. Sidebar focus persists after editor click — clear_sidebar_focus() added
15. Sidebar focus persists after terminal click — clear_sidebar_focus() + sidebar.has_focus = false
16. Dialog text/buttons overflow — auto-sized width from content
- Created docs/NATIVE_GUI_LESSONS.md with 9 sections of lessons for future backends

---

**Session 267 — Win-GUI bug blitz + parity tests (9 fixes, 6 new tests, 12 bugs found):**

1. **Activity bar icons** — Replaced broken Nerd Font approach with Segoe MDL2 Assets / Segoe Fluent Icons (native Windows icon fonts). 48×48 centered icon cells with dedicated DirectWrite format.
2. **Tab drag-and-drop** — Full implementation: threshold-based drag start, `compute_win_tab_drop_zone()` for reorder/split/merge, visual overlay (blue zone highlight + insertion bar + ghost label), calls engine's `tab_drag_begin()`/`tab_drag_drop()`.
3. **Terminal split** — Split button + add/close buttons in toolbar, split pane rendering with divider, pane focus switching, divider drag resize.
4. **Popup mouse handlers** — `CachedPopupRects` infrastructure. Editor hover (click/dismiss/scroll), panel hover (dismiss), debug toolbar (button clicks via `execute_command`).
5. **Scrollbar theme colors** — Fixed editor scrollbar to use `theme.scrollbar_thumb`/`scrollbar_track` instead of hardcoded alpha values.
6. **Explorer file open** — Single-click now uses `open_file_preview()` (preview tab). Double-click/Enter uses `open_file_in_tab()` (new permanent tab). Was using `switch_window_buffer` which replaced the current buffer.
7. **Context menu z-order + clicks** — Context menu, dialog, notifications now draw after sidebar in `on_paint`. Full click handler: item selection, action dispatch via `handle_context_action()`, outside-click dismiss.
8. **Default shell** — `default_shell()` returns `powershell.exe` on Windows instead of `/bin/bash`.
9. **Phase 2c action parity harness** — `UiAction` enum (26 variants), `all_required_ui_actions()` source of truth, per-backend collectors. 3 parity tests + 3 behavioral contract tests. Systematic GTK↔Win-GUI comparison found 12 additional bugs (4 critical, 5 medium, 3 low — see BUGS.md).

---

**Session 266 — Win-GUI parity fixes (10 fixes, 5471 tests):**

1. **Text rendering truncation** — `draw_styled_line` gap-filling for text between syntax spans
2. **Settings icon clipped/not clickable** — repositioned above bottom chrome, click handler fixed
3. **Settings panel interactive** — full form rendering + keyboard handling (j/k/Enter/Tab/q, editing, paste)
4. **Global status bar over per-window status** — skip when empty; reserve 1 row not 2 for bottom chrome
5. **Per-window status bar segments** — per-segment background colors matching TUI
6. **Editor window clipping** — `PushAxisAlignedClip` prevents text bleeding
7. **Sidebar panel clipping** — clip rect and panel_h use `sidebar_bottom`
8. **Command line descenders clipped** — bottom margin for below-baseline characters
9. **Sidebar/command line background gaps** — panel bg full height; cmd line starts at `editor_left`
10. **Clippy fix** — identical if/else branches in diff toolbar

---

**Session 265 — Backend parity harness + Win-GUI rendering fixes (5471 tests):**

1. **Backend parity harness** — `UiElement` enum (27 variants) + `collect_expected_ui_elements()` source of truth + per-backend collectors (`collect_ui_elements_tui`, `collect_ui_elements_wingui`). 7 parity tests assert every `ScreenLayout` field has a corresponding draw call in each backend.
2. **6 Win-GUI rendering fixes** — Added `draw_editor_hover`, `draw_diff_peek`, `draw_debug_toolbar`, `draw_diff_toolbar_in_tab_bar`, `draw_tab_tooltip`, `draw_panel_hover` to Win-GUI backend. Zero known rendering gaps remaining.
3. **Win-GUI smoke test checklist** — Documented visual verification steps for Session 265 changes.

---

**Session 264 — Context-aware dedent + terminal bug fixes (5440 tests):**

1. **Context-aware dedent** — `dedent_lines()` in `motions.rs` rewritten with two-pass approach: first pass finds minimum indent across all non-blank lines in the selection; second pass removes `min(shift_width, min_indent)` columns from every line uniformly. Preserves relative nesting structure. Blank lines skipped for min calculation. 6 new tests.
2. **Terminal panel resize fix** — Two bugs: (a) mouse drag events didn't set `needs_redraw=true` in `tui_main/mod.rs`, so drag had no visual effect; (b) available-space formula in `mouse.rs` used hardcoded `2` instead of computed `bottom_chrome`. Also raised max terminal height from fixed `30` to dynamic (leaves 4 editor lines visible). Both TUI and GTK backends updated.
3. **GTK terminal toggle fix** — `[P]` status bar button required two clicks on first use because it sent an async `Msg::ToggleTerminal` via Relm4 message queue. Fixed by calling `terminal_new_tab()` synchronously in the click handler (matching TUI which already did this).
4. **CLAUDE.md updates** — Added Win-GUI directory section, multi-backend rule ("check all THREE backends when touching mouse/layout/rendering code").
5. **Release v0.9.0** — Version bumped, PR #21 created and merged.
6. **Rendering test infrastructure** — 9 ScreenLayout tests in `render.rs` (tests shared rendering data all backends consume); 10 TUI assertion tests + 6 insta golden-file snapshot tests in `tui_main/render_impl.rs` using ratatui `TestBackend`; `insta` added as dev-dependency.
7. **Win-GUI bug audit** — Systematic comparison of Win-GUI vs TUI across 10 areas (tab clicks, file open, scrollbar, preview tabs, terminal, status bar, explorer, keys, mouse drag, context menus). Identified 9 gaps, corrected stale bugs.
8. **Win-GUI fixes (7 total)**: (a) Explorer single-click → `OpenMode::Preview`, double-click → `Permanent`; (b) preview tab dimmer color in draw.rs; (c) Settings gear icon pinned to activity bar bottom + click handler; (d) explorer + tab bar right-click context menus; (e) status bar clickable via `win_status_segment_hit_test()` + `build_window_status_line()` on demand + `pixel_to_editor_pos()` now excludes status bar area; (f) tab bar clicks — fixed Y coordinate (`TITLE_BAR_TOP_INSET + lh * TITLE_BAR_HEIGHT_MULT`) and height (`lh * TAB_BAR_HEIGHT_MULT`) in both single-group and multi-group cache; (g) terminal resize drag — `terminal_resize_drag` field + header click + WM_MOUSEMOVE handler + mouse-up PTY resize.

**Session 263 — Status line positioning + Windows alpha note (5422 tests):**

1. **`status_line_above_terminal` setting** (default `true`, abbreviation `slat`): controls where the status line and command line appear relative to the terminal panel.
   - `slat` ON (default): per-window status bars stay inside each editor split window as before — they're naturally above the terminal by being part of the editor area. Command line at screen bottom.
   - `noslat`: when terminal is open, per-window status bars are removed from windows; a single status bar for the active file renders below the terminal panel, with the command line directly below it.
2. **New setting infrastructure**: field on `Settings` struct with `#[serde(default)]`, `parse_set_option` for `:set slat`/`:set noslat`, `get_value_str`/`set_value_str` for JSON, `SettingDef` for Settings UI.
3. **render.rs**: `separated_status_line: Option<WindowStatusLine>` on `ScreenLayout`; `build_screen_layout()` conditionally extracts active window status; `separated_status_height_px()` helper; updated `editor_bottom_px()`.
4. **TUI backend**: 8-slot vertical layout with conditional separated status row between debug toolbar and wildmenu; `render_window_status_line()` for separated bar; mouse click detection via `status_segment_hit_test()`.
5. **GTK backend**: `draw_window_status_bar()` for separated status below terminal; `gtk_editor_bottom()` helper consolidated 6+ inline editor_bottom calculations; click zones auto-populated via `status_segment_map`.
6. **Win-GUI backend**: `draw_separated_status_line()` method; `draw_command_line()` repositioned when separated; layout `bottom_chrome` adjusted.
7. **README**: Windows native GUI marked as **alpha** in Platforms table; added warning note in Windows install section recommending TUI build.
8. **3 new tests**: setting toggle, `editor_bottom_px` with separated status, `separated_status_height_px`.

---

**Session 262 — 7 bug fixes: terminal paste, GTK Ctrl+C, mouse selection, CI (5415 tests):**

1. **TUI terminal paste**: Added `poll_terminal()` for immediate feedback, wrapped paste in bracketed paste escape sequences for multi-line safety, added register fallback (system clipboard → `+` register → `"` register) so yanked text is available in terminal Ctrl+V. Added error messages instead of silent failure.
2. **GTK terminal Ctrl+C**: `gtk_key_to_pty_bytes()` returned empty for Ctrl+letter keys because GTK's `to_unicode()` filters control characters. Added fallback to derive control byte from `key_name` (single letter → `& 0x1f`). Plain Ctrl+C now sends `\x03` (SIGINT) correctly. Added Ctrl+Shift+C handler to copy terminal selection.
3. **TUI terminal selection column offset**: Selection start/drag used absolute screen `col` instead of terminal-relative `col.saturating_sub(editor_left)`. Fixed both click and drag handlers.
4. **TUI terminal selection row offset**: Mouse handler hardcoded `2` bottom chrome rows (status + command line), but per-window status lines (default) hide the global status bar — only 1 bottom chrome row. Replaced all 10 hardcoded instances with dynamic `bottom_chrome` variable.
5. **Editor drag leaking into terminal**: Editor text drag that moved outside all editor windows fell through to terminal drag handler, creating phantom selections. Added early return when `mouse_text_drag` is active.
6. **Terminal drag guard**: Added `col >= editor_left` check and existing-selection requirement to terminal drag handler to prevent sidebar clicks from activating terminal selection.
7. **CI coverage job failure**: `--all-features` on Linux pulled in `win-gui` → `windows-rs` crates with `windows-future 0.2.1` API incompatibility (`IMarshal`/`marshaler` not found). Changed to `--features gui`.

Files changed: `src/tui_main/mod.rs`, `src/tui_main/mouse.rs`, `src/gtk/mod.rs`, `src/gtk/util.rs`, `.github/workflows/ci.yml`, `BUGS.md`.

---

**Session 261 — Fix `o` CRLF/CR line ending bug (5415 tests):**

Bug fix: `o` command failed to create a new line in files with CRLF (`\r\n`) or lone CR (`\r`) line endings. The `insert_pos` calculation in `keys.rs` only checked for `\n`, so for CRLF it inserted between `\r` and `\n` (Ropey re-paired them but created mixed endings), and for lone `\r` the new `\n` was absorbed into a CRLF pair — no new line appeared. Fixed by using `RopeSlice::char()` indexed access to detect `\r\n` (skip 2 chars) and `\r` alone (insert before it). 4 new tests covering CRLF, lone CR, content preservation, and indented YAML scenarios.

---

**Session 260 — 12 bug fixes (5396 tests):**

Massive bug fix session fixing 12 bugs across GTK, TUI, and core engine. Also filed 10 new bugs, 2 new features (status line above terminal, terminal maximize), and 1 new feature request (full keyboard navigation in picker).

Bug fixes:
1. `%` brace match doesn't scroll — center viewport when match is >½ screen away (`keys.rs`). 1 test.
2. TUI tab underline extends to number prefix — split render loop, underline only on filename portion after `: ` (`render_impl.rs`).
3. Preview tab can't be made permanent by clicking tab — `goto_tab()` calls `promote_preview()` on active buffer (`windows.rs`). 1 test.
4. Accidental explorer drag triggers move to same location — `confirm_move_file()` returns early when `src.parent() == dest` (`buffers.rs`). 2 tests.
5. GTK tab bar hides tabs despite available space — char width measurement used "M" (widest Latin char) with proportional font; changed to 15-char representative sample (`draw.rs`).
6. Terminal panel steals clicks from explorer tree — added `col >= editor_left` guard to TUI terminal click handler (`mouse.rs`).
7. Live grep scroll wheel changes file instead of scrolling preview — added column-based pane detection; `picker_preview_scroll` field on Engine; increased preview context (30→500 lines, ±5→±50 grep context); initial scroll centers on match (`mouse.rs`, `picker.rs`, `render.rs`, `mod.rs`).
8. Terminal Ctrl+V paste broken — TUI: added lowercase 'v' match; GTK: added Ctrl+V handler in terminal focus block (`tui_main/mod.rs`, `gtk/mod.rs`).
9. TUI terminal draws on top of fuzzy finder — moved picker/folder-picker/tab-switcher rendering after bottom panel in draw order (`render_impl.rs`).
10. GTK terminal draws on top of fuzzy finder — moved picker/tab-switcher/dialog to absolute end of `draw_editor()` after all persistent UI (`draw.rs`).
11. GTK visual select highlights wrong line — rewrote `draw_visual_selection()`: built `line_to_view` HashMap mapping buffer line→last non-skippable view row; line-mode iterates all visual rows (including wrap continuations); skips `DiffLine::Padding` and `is_ghost_continuation` (`draw.rs`).
12. Right-click in terminal shows editor context menu — added terminal bounds check before `open_editor_context_menu()` fallthrough (`mouse.rs`).

Also fixed: pre-existing clippy warning in `git.rs` (`#[allow(unused_mut)]` for Windows-only mutation).

Files changed: `src/core/engine/keys.rs`, `src/core/engine/windows.rs`, `src/core/engine/buffers.rs`, `src/core/engine/mod.rs`, `src/core/engine/picker.rs`, `src/core/engine/tests.rs`, `src/core/git.rs`, `src/render.rs`, `src/gtk/draw.rs`, `src/gtk/mod.rs`, `src/tui_main/render_impl.rs`, `src/tui_main/mouse.rs`, `src/tui_main/mod.rs`, `BUGS.md`, `PLAN.md`, `PROJECT_STATE.md`.

**Session 259 — README revamp (5391 tests):**

Full review and rewrite of README.md for multi-platform maturity. Replaced "alpha ware" / "vibe-coded" status with "Beta" label and backup disclaimer. Added Platforms table (Linux GTK4, macOS GTK4 via Homebrew, Windows native Win32+Direct2D+DirectWrite, TUI everywhere). Added Windows native GUI and TUI download instructions. Added Windows build commands (`--features win-gui --bin vimcode-win`). Updated Architecture tree with `win_gui/` directory (~5,322 lines) and all current line counts (~128K total, core/ ~81,824, engine/ ~59,947, render.rs ~7,815). Updated Tech Stack with windows-rs/Direct2D/DirectWrite and notify crate. Added LaTeX to syntax highlighting list, mentioned semantic token overlay. Updated test count to 5,391. Removed 7 duplicate command table entries. Referenced vimcode.org for screenshots. Clarified `vcd` as recommended TUI binary. Mentioned Extensions panel for discovering extensions. Documented keymaps editor access in VSCode mode (F1 → Keymaps). Noted F1 command palette works in both Vim and VSCode modes. Updated Acknowledgements. Files changed: `README.md`, `PROJECT_STATE.md`, `PLAN.md`.

---

**Session 258 — Multi-backend shared hit-testing & key-binding extraction (5320 tests):**

Merged `windows` branch into `develop`. Extracted shared hit-testing and key-binding code from GTK/TUI/Win-GUI backends into platform-agnostic `render.rs`. Moved `ClickTarget` enum from `gtk/click.rs` to `render.rs` (now `pub`). Added 8 shared geometry/hit-testing helper functions: `tab_row_height_px()`, `tab_bar_height_px()`, `status_bar_height_px()`, `editor_bottom_px()` (layout dimensions), `scrollbar_click_to_scroll_top()` (ratio-based scroll), `display_col_to_buffer_col()` (tab-aware column mapping), `is_tab_close_click()` (close button zone), `matches_key_binding()` (Vim-style key notation matcher). GTK: `pixel_to_click_target()` uses `tab_bar_height_px()` + `editor_bottom_px()` instead of inline math; `matches_gtk_key()` extracts GDK modifiers and delegates to `matches_key_binding()`. TUI: `matches_tui_key()` extracts crossterm modifiers and delegates; scrollbar click uses `scrollbar_click_to_scroll_top()`. Win-GUI: `scrollbar_hit()` uses shared scrollbar helper. All functions are pure (no platform deps), take basic types (f64, usize, bool, &str). 7 comprehensive tests. Filed 4 pre-existing win-gui bugs: scrollbar not drawn (hit area exists but no paint code), tab bar clicks not working, file open replaces buffer instead of new tab, no preview mode. Files changed: `render.rs` (+352), `gtk/click.rs` (-52), `gtk/util.rs` (-17), `tui_main/mod.rs` (-17), `tui_main/mouse.rs` (-4), `win_gui/mod.rs` (-4), `BUGS.md` (+8).

---

**Session 257 — Win-GUI Phase 4: custom title bar, native file dialogs, IME, file watching (5313 tests):**

Custom frameless title bar: WM_NCCALCSIZE removes Windows chrome, DwmExtendFrameIntoClientArea preserves native shadow and Win11 rounded corners, WM_NCHITTEST provides drag (HTCAPTION), resize borders (HTTOP/HTTOPLEFT/HTTOPRIGHT), caption button zones (HTCLIENT). Min/max/close buttons with hover states (red for close, lighter for min/max). Full title bar returns HTCLIENT when menu dropdown is open to enable hover switching. Taller title bar (1.8× line_height) with 6px top inset for DWM shadow zone. Native file dialogs via COM: IFileOpenDialog for File > Open File and File > Open Folder (FOS_PICKFOLDERS), IFileSaveDialog for Save Workspace As. CoInitializeEx at startup. Menu action strings intercepted before execute_command (same pattern as GTK backend). IME composition: WM_IME_STARTCOMPOSITION positions candidate window at cursor via ImmSetCompositionWindow with CFS_POINT. Cursor pixel position computed from cached window rects + scroll offset with DPI scaling. Cross-platform file watching: notify crate v7, RecommendedWatcher initialized in Engine::new(), files watched on open, tick_file_watcher() polled from win-gui and TUI backends, auto-reload non-dirty buffers, reload/keep dialog for dirty buffers via engine dialog system. UI polish: Segoe UI 13px proportional font for menu bar and tab labels (matching VSCode), separate IDWriteTextFormat + draw_ui_text()/measure_ui_text() helpers. Taller tab bar (1.5× line_height) with proper padding and vertical centering. Menu bar uses tab_bar_bg (dark) instead of status_bg (blue). Dark dropdown background (background.lighten(0.10)). Dynamic window title: "filename — VimCode" with dirty indicator. Bug fix: double RefCell borrow panic in on_mouse_move — caption_button_at() called APP.with(borrow()) inside APP.with(borrow_mut()), silently caught by catch_unwind, preventing all menu hover code from executing. Fixed by inlining the check. Files changed: Cargo.toml (+7 features), Cargo.lock (+126), src/win_gui/mod.rs (+546), src/win_gui/draw.rs (+197), src/core/engine/mod.rs (+13 fields), src/core/engine/buffers.rs (+168 file watcher), src/core/engine/panels.rs (+4 dialog handler), src/tui_main/mod.rs (+2 tick call).

---

**Session 256 — Win-GUI Phase 3: menu bar, terminal, DPI, sidebar clicks, breadcrumbs (5313 tests):**

Menu bar with dropdowns (rendering, keyboard nav with arrow keys/Enter/Escape, mouse hover switching between menus, item highlight), terminal panel (D2D cell-grid rendering, PTY keyboard input, Ctrl-T toggle, toolbar with tabs, find bar), per-monitor DPI awareness (WM_DPICHANGED, physical-to-DIP mouse coords, render target recreation), sidebar panel click handling (Git section toggle/selection, Extensions expand/select, Settings row select, AI/Search/Debug focus), scrollbar click-to-jump + drag, breadcrumb bar rendering, tab bar sidebar/menu offset, D2D axis-aligned clip, periodic git status refresh, Win32_UI_HiDpi feature. 15 iterative bug fixes during testing.

---

**Session 255 — Multi-backend prep for native Windows/macOS GUIs (5313 tests):**

`Color::to_f32_rgba()` for D2D/CoreGraphics; extracted `view_row_to_buf_line()`/`view_row_to_buf_pos_wrap()` from GTK to shared `render.rs`; consolidated `open_url_in_browser()` in core engine (was duplicated in GTK and TUI with platform-specific logic); added Native Platform GUIs roadmap items to PLAN.md.

---

**Session 254 — Windows TUI builds + bug fixes (5313 tests):**

Release v0.8.0 prep and Windows TUI support. Added `CREATE_NEW_PROCESS_GROUP` to LSP/DAP process spawning on Windows (equivalent of Unix setsid). Windows clipboard via powershell `Get-Clipboard`/`Set-Clipboard` and `clip.exe`. Windows URL opener via `cmd /c start`. Windows swap PID check via `tasklist`. Guard DISPLAY env var to non-Windows. Added Windows TUI job to CI (`windows-latest`, `--no-default-features`) and release workflow producing `vcd-windows-x86_64.exe`. Fixed tree-sitter-latex link error on Windows (`kind = "static"` on FFI `#[link]`). Skipped/fixed 8 tests that fail on Windows paths (`#[cfg(not(target_os = "windows"))]` for git diff tests, cross-platform assertions for debugpy venv and session paths). Regenerated `flatpak/cargo-sources.json` for ratatui 0.29. Bug fixes: picker preview stale chars when cycling files (explicit per-row clear + tab sanitization); insert mode click past EOL (allow cursor one past last char in insert/replace mode via `set_cursor_for_window`); scrollbar drag moving cursor (replaced `set_cursor_for_window` with `set_scroll_top_for_window`); git panel discard confirm dialog (`pending_sc_discard` + `confirm_sc_discard` dialog tag). Files changed: `lsp.rs`, `dap.rs`, `swap.rs`, `syntax.rs`, `session.rs`, `dap_manager.rs`, `engine/mod.rs`, `engine/windows.rs`, `engine/source_control.rs`, `engine/panels.rs`, `engine/tests.rs`, `tui_main/mod.rs`, `tui_main/mouse.rs`, `tui_main/render_impl.rs`, `gtk/click.rs`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `flatpak/cargo-sources.json`.

---

**Session 253 — Notification / progress indicator (5313 tests):**

New feature: background operation progress indicator in the per-window status bar. `Notification` struct + `NotificationKind` enum (LspInstall, LspIndexing, ExtensionInstall, GitOperation, ProjectSearch, ProjectReplace) on Engine. Lifecycle methods: `notify()` (push in-progress, returns ID), `notify_done(id, msg)` (mark complete by ID), `notify_done_by_kind(kind, msg)` (mark all of a kind complete), `dismiss_notification(id)` (remove by ID), `dismiss_done_notifications()` (remove all completed), `tick_notifications()` (auto-dismiss after 5s timeout). Rendered as `StatusSegment` in `build_window_status_line()` between Ln:Col and layout toggle buttons — spinner animation (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ braille frames at ~10fps) for in-progress ops using `theme.function` color, bell icon (󰂞 nerd / `*` ASCII) for completed ops using `theme.string_lit` color. `StatusAction::DismissNotifications` click-to-clear all done notifications. TUI: 100ms poll timeout when active notifications for smooth spinner; `needs_redraw = true` in idle loop when notifications present. GTK: `draw_needed.set(true)` on active notifications in poll tick handler. Wired up: LSP install start (lsp_ops.rs), LSP install complete (panels.rs via `notify_done_by_kind`), project search start/complete (search.rs), project replace start/complete (search.rs). Message truncated to 30 chars in status bar. 9 new tests covering lifecycle, auto-dismiss, click-to-dismiss, ID incrementing. Files changed: `engine/mod.rs` (+120), `engine/execute.rs` (+3), `engine/lsp_ops.rs` (+1), `engine/panels.rs` (+6), `engine/search.rs` (+4), `engine/tests.rs` (+100), `render.rs` (+49), `gtk/mod.rs` (+12), `tui_main/mod.rs` (+9).

---

**Session 252 — TUI spell underline bleed fix (5304 tests):**

Bug fix: TUI spell check underlines bled into fuzzy finder (picker) popup overlays. Root cause: `set_cell()` (346 call sites across TUI rendering) only reset character, fg, and bg colors but never cleared `cell.modifier` or `cell.underline_color`. When spell checking added `Modifier::UNDERLINED` + `underline_color` to editor cells, the picker overlay's clear pass via `set_cell()` left those attributes intact, causing horizontal underlines at the same screen positions in the popup. Fixed by resetting `modifier = Modifier::empty()` and `underline_color = RColor::Reset` in `set_cell()`, `set_cell_wide()` (both main and continuation cells), and `set_cell_styled()` (which left stale `underline_color` when passed `None`). Files changed: `src/tui_main/mod.rs`. Also added "Remote editing over SSH" research item to PLAN.md.

---

**Session 251 — Layout toggle buttons (5304 tests):**

Clickable nerd-font icon segments (󰘖 sidebar, 󰆍 panel, 󰍜 menu) with `[S]`/`[P]`/`[M]` ASCII fallbacks in per-window status bar (right side, after Ln:Col). Sidebar and terminal panel toggles in both GTK and TUI; menu bar toggle only in TUI (`menu_bar_toggleable` engine field — GTK menu bar is the window title bar and can't be hidden). Icons dim via `theme.status_inactive_fg` when panel is inactive. `StatusAction::ToggleSidebar` returns `EngineAction::ToggleSidebar` (backends manage sidebar visibility); `StatusAction::TogglePanel` returns `EngineAction::OpenTerminal` when no PTY panes exist, otherwise calls `toggle_terminal()` directly. GTK status bar click detection overhauled: `draw_window_status_bar` now populates a `StatusSegmentMap` cache with Pango-measured `(start_x, end_x, action)` zones per window; `pixel_to_click_target` uses cached zones instead of the old `gtk_status_segment_hit_test` (which used `chars().count() * char_width` and broke on variable-width nerd font glyphs). `handle_status_action` return type changed from `()` to `Option<EngineAction>`; `handle_mouse_click` return type changed to `(Option<bool>, Option<EngineAction>)`. Files changed: `engine/mod.rs`, `engine/execute.rs`, `engine/tests.rs`, `render.rs`, `gtk/mod.rs`, `gtk/draw.rs`, `gtk/click.rs`, `tui_main/mod.rs`, `tui_main/mouse.rs`. Known bug: GTK terminal panel toggle requires two clicks on first use.

---

**Session 250 — Marksman LSP status indicator fix (5300 tests):**

Bug fix: LSP status bar indicator stuck on "Initializing" for Marksman (Markdown LSP) and potentially other servers that don't support semantic tokens. Root cause: `mark_server_responded()` was only called on non-empty hover/definition responses. Marksman doesn't return semantic tokens and often returns empty hover content, so the readiness flag never got set. Fixed by: (1) calling `mark_server_responded()` on `LspEvent::Initialized` — the initialization handshake completing is sufficient proof of readiness; (2) removing `if !locations.is_empty()` guard on `DefinitionResponse` handler; (3) removing `if contents.is_some()` guard on `HoverResponse` handler. Files changed: `src/core/engine/panels.rs`, `src/core/lsp_manager.rs`.

---

**Session 249 — Spell underline fix, spell checker init, CI rename test fix (5300 tests):**

Bug fix: GTK spell check underline misaligned. Root cause: `draw_editor` in `draw.rs` called `layout.set_attributes(None)` before computing positions via `index_to_pos()` for diagnostic underlines, spell underlines, cursor, ghost text, and extra cursors. This stripped `font_scale` attributes (markdown headings use 1.1–1.4×), causing character positions to be computed at normal font width while text was rendered at scaled width. Underlines started before the word and ended in the middle. Fixed by restoring correct Pango attributes via `build_pango_attrs(&rl.spans)` before all `index_to_pos` calls. 5 sites fixed in `draw.rs`: line-level restore (diagnostics + spell), cursor, ghost text, extra cursors.

Bug fix: Spell checker not initializing when `spell` setting enabled via Settings sidebar (Bool toggle or text entry in `ext_panel.rs`) or via `settings.json` file reload (both GTK `mod.rs` and TUI `mod.rs`). Added `ensure_spell_checker()` calls after `set_value_str` for the `spell` key and after settings reload from disk.

Bug fix: CI failure — 2 inline rename integration tests (`test_inline_rename_start`, `test_inline_rename_typing_and_cursor`) in `tests/context_menu.rs` expected cursor at full filename length (e.g., 7 for "old.txt"), but `start_explorer_rename()` positions cursor at stem end (3 for "old"). Tests updated to match. Failure was on macOS ARM64 CI runner, also reproducible locally.

Files changed: `src/gtk/draw.rs` (5960 lines, +3), `src/core/engine/ext_panel.rs`, `src/gtk/mod.rs`, `src/tui_main/mod.rs`, `src/core/engine/tests.rs` (+1 test), `tests/context_menu.rs` (2 test fixes).

---

**Session 248 — TUI settings button fix, hardcoded colors cleanup, wide-char rendering fix (5299 tests):**

Bug fix: TUI Settings button in activity bar not clickable. Four early-return click handlers in `mouse.rs` (command line input, message line selection, status bar branch click, bottom-row guard) intercepted ALL clicks on the bottom two terminal rows regardless of column, before the activity bar handler at line 1562 could process them. The settings button is rendered at the bottom of the activity bar, which coincides with the command line row. Fixed by adding `col >= ab_width` guards to all four checks.

Hardcoded colors cleanup: Added 4 new Theme fields (`scrollbar_thumb`, `scrollbar_track`, `terminal_bg`, `activity_bar_fg`) with values for all 6 built-in themes (onedark, gruvbox_dark, tokyo_night, solarized_dark, vscode_dark, vscode_light) + VSCode JSON theme importer (`scrollbarSlider.background`, `terminal.background`, `activityBar.foreground`). Replaced ~50 hardcoded color values across 5 files:
- `render_impl.rs`: 3× `RColor::Rgb(128,128,128)` scrollbar thumbs → `theme.scrollbar_thumb`
- `panels.rs`: activity bar icons → `theme.activity_bar_fg`; git status colors → `theme.git_added/modified/deleted`; debug buttons → `theme.git_added`/`theme.diagnostic_error`; scrollbar thumbs → `theme.scrollbar_thumb`; terminal bg → `theme.terminal_bg`; terminal find-match → `theme.search_match_*`; ext panel secondary bg → `theme.status_bg.darken(0.15)`
- `mod.rs` (GTK): search result markup → `theme.function`/`theme.foreground`; cursor indicator → `theme.scrollbar_thumb`
- `draw.rs` (GTK): h-scrollbar track/thumb → `theme.scrollbar_track`/`theme.scrollbar_thumb`; tab drag overlay → `theme.cursor`/`theme.background`/`theme.foreground`; picker scrollbar track → `theme.scrollbar_track`; terminal bg → `theme.terminal_bg`
- `css.rs` (GTK): scrollbar slider, h-editor-scrollbar, find dialog, find-match-count colors now theme-aware via `make_theme_css()` overrides

Wide-char rendering fix: `set_cell_wide()` in `tui_main/mod.rs` used `reset()` + `set_skip(true)` on the continuation cell of double-width Nerd Font glyphs. `set_skip(true)` prevented ratatui from emitting anything for that cell, leaving the terminal's default black background visible as a black rectangle next to wide glyphs. Fixed by using `set_symbol("")` + `set_fg(fg)` + `set_bg(bg)` instead — ratatui's convention for wide-char continuation cells that correctly emits the background color.

Theme consistency fixes found during smoke testing:
- Git commit bar buttons used `theme.foreground` (dark on light themes) instead of `theme.status_fg` — illegible against blue `status_bg`. Fixed to use `hdr_fg` (status_fg).
- Debug sidebar active section header used `theme.tab_active_fg` (#333333 on light themes) — black on blue status_bg. Fixed to use `theme.status_fg.lighten(0.2)`.
- Debug "Start Debugging" button: full label was green (`theme.git_added`) which is hard to read on blue status_bg. Fixed: icon char gets semantic green/red, label text uses `hdr_fg` (status_fg) for readability.

Files changed: `src/render.rs` (4 new Theme fields + 6 themes + VSCode importer), `src/tui_main/mouse.rs` (activity bar click fix), `src/tui_main/render_impl.rs` (scrollbar thumbs), `src/tui_main/panels.rs` (activity bar, git, debug, terminal, scrollbar colors), `src/tui_main/mod.rs` (set_cell_wide fix), `src/gtk/mod.rs` (search result markup, cursor indicator), `src/gtk/draw.rs` (scrollbar, tab drag, terminal bg), `src/gtk/css.rs` (theme-aware CSS overrides), `BUGS.md`.

**Session 247 — GTK explorer first-click bug fix, picker search history (5299 tests):**

Bug fix: GTK TreeView folders required two clicks/Enter presses to expand the first time. Root cause: `tree_row_expanded()` in `tree.rs` removed the dummy placeholder child (`__vimcode_loading__`) before calling `build_file_tree_shallow()` to populate real children. Between remove and populate, the directory had zero children, causing GTK to auto-collapse the row. Fix: populate real children first, then remove the dummy. Also fixed Enter after arrow-key navigation — intercept Return/KP_Enter in the TreeView key handler and send `Msg::ExplorerActivateSelected` (which syncs cursor→selection via `select_path()`) instead of relying on native `row_activated` (which uses the stale selection, not the arrow-key cursor position).

Picker search history: `picker_history: HashMap<PickerSource, Vec<String>>` on Engine (session-scoped, not persisted). `picker_push_history()` saves non-empty trimmed query on confirm; consecutive duplicates deduplicated; capped at 100 entries. `picker_history_index: Option<usize>` + `picker_history_typing_buffer: String` for browsing state. Up at `picker_selected == 0` enters history mode (saves current query, recalls most recent entry); subsequent Ups go to older entries; `saturating_sub(1)` at oldest. Down in history mode navigates newer; past newest restores original typed query and exits history mode. Typing, backspace, paste all call `picker_exit_history()` to reset. History state reset on `open_picker()`. Added `Eq + Hash` derives to `PickerSource` for HashMap key usage. 7 new tests.

New bug filed: Marksman (Markdown LSP) status bar indicator stuck on "initializing" — greyed out with `…` suffix persists. Likely because marksman doesn't support `textDocument/semanticTokens`, and the render-side readiness heuristic downgrades `Running` to `Initializing` when `BufferState.semantic_tokens` is empty.

Files changed: `src/gtk/tree.rs` (lazy-load ordering), `src/gtk/mod.rs` (Enter key handler), `src/core/engine/mod.rs` (PickerSource derives + history fields), `src/core/engine/picker.rs` (history helpers + key handler modifications), `src/core/engine/tests.rs` (7 new tests), `BUGS.md`, `PLAN.md`.

**Session 246 — Explorer overhaul, diagnostic filtering, tree UX (5292 tests):**

Removed explorer toolbar (New File/New Folder/Delete buttons) from both TUI (`EXPLORER_TOOLBAR_LEN` constant, toolbar rendering in `panels.rs`, click handling in `mouse.rs`) and GTK (`explorer-toolbar` Box widget + CSS). Removed TUI "EXPLORER" header row — tree rows now start at `area.y` instead of `area.y + 1`; `tree_height` uses full `area.height`; mouse click/scrollbar calculations adjusted (no header offset).

Right-click in empty explorer space opens root folder context menu. TUI: when `tree_row >= sidebar.rows.len()`, uses `sidebar.root` as directory target. GTK: when `path_at_pos()` returns None and no selection, falls back to `engine.cwd`.

Inline rename improvements: `ExplorerRenameState.selection_anchor: Option<usize>` field for text selection. `start_explorer_rename()` pre-selects filename stem (rfind `.` excluding position 0 for dotfiles). `handle_explorer_rename_key()` rewritten: selection-aware Backspace/Delete/typing (delete selection first); Ctrl-A select all, Ctrl-C/X copy/cut via `clipboard_write`, Ctrl-V paste via `clipboard_read`; arrow keys clear selection; single Escape cancels (no two-press). TUI rendering: `sel_bg` (fuzzy_selected_bg) for selected text; horizontal scroll offset when cursor exceeds available width. New-entry input also gets Ctrl-V paste and scroll. GTK: `connect_editing_started` handler on `CellRendererText` — downcasts editable to `Entry`, calls `select_region(0, stem_end)` via idle callback.

Bug fixes: (1) GTK inline rename/new-file disappears immediately — `update_tree_indicators` (every 1s) called `set_value` on TreeStore rows, cancelling active GTK cell editor; `RefreshFileTree` could `store.clear()` during editing. Fix: `cell_editing` guard via `name_cell.is_editing()` skips both operations. (2) GTK SIGSEGV in `gtk_tree_store_set_value` — `__NEW_FILE__`/`__NEW_FOLDER__` marker rows in TreeStore caused crash; fix: skip marker rows in `update_tree_indicators::walk`. (3) GTK context menu new file/folder popover steals focus — explicit `popover.popdown()` before sending messages; `timeout_add_local_once(50ms)` instead of `idle_add_local_once` for all inline edit start operations. (4) TUI context menu "New File" from empty space did nothing — `handle_explorer_context_action` got path from `sidebar.rows[sidebar.selected]` instead of context menu target; fix: new `Engine::context_menu_target_path()` method; callers extract target before `context_menu_confirm()` consumes menu.

Diagnostic source filtering: `ignore_error_sources` from extension manifests now filters error-severity diagnostics at storage time in `poll_lsp()` (not just explorer counts). `refilter_diagnostics()` method retroactively filters when registry updates. `ext_refresh()` called at startup in both GTK and TUI to fetch fresh registry. `initialization_options: Option<serde_json::Value>` field added to `LspConfig` (extensions.rs) and `LspServerConfig` (lsp.rs); merged into LSP `initialize` request's `initializationOptions`. Rust extension in registry declares `ignore_error_sources: ["rust-analyzer"]` to suppress rust-analyzer's native type-check false positives (real errors come from `rustc` via cargo check).

Explorer tree UX: `explorer_file_fg` theme field on all 6 built-in themes (muted grey for file names, distinct from bright `foreground`). VSCode JSON importer reads `sideBar.foreground`. TUI indent guide lines: `│` drawn at each indent level > 0 using `line_number_fg` (dim grey). TUI explorer layout restructured: `[chevron (2 cols)] [icon] [space] [name]` — both dirs and files align icons at the same column. GTK name column: `ellipsize: End` on `CellRendererText` + `Fixed` column sizing prevents long filenames from pushing indicators off-screen.

Case-insensitive explorer sort: `explorer_sort_case_insensitive` setting (default true); `:set noesci` to disable. Applied to TUI `collect_rows`, GTK `build_file_tree_shallow` and `tree_row_expanded`. `TuiSidebar.sort_case_insensitive` mirrors engine setting.

Fix: `LineEnding::detect()` byte-boundary crash — slicing at byte 8192 could land inside a multi-byte character (e.g. `─` at bytes 8190..8193). Now backs up to nearest char boundary via `is_char_boundary()` loop. 10 new tests.

---

**Session 245 — Editor action menu, richer syntax highlighting, explorer colors (5282 tests):**

Editor action menu (`⋯`) button at right edge of each tab bar group. 8-item dropdown: Close All, Close Others, Close Saved, Close to Right/Left, Toggle Word Wrap, Change Language Mode, Reveal in File Explorer. `ContextMenuTarget::EditorActionMenu` + `open_editor_action_menu()` + `close_all_tabs()`. TUI: `TAB_ACTION_BTN_COLS` constant, click handling in multi/single-group paths. GTK: `ActionBtnMap` type, `ClickTarget::ActionMenuButton`, `show_action_menu_popover()` with `PopoverMenu`.

Richer tree-sitter highlight queries: 12 new Theme fields (`control_flow`, `operator`, `punctuation`, `macro_call`, `attribute`, `lifetime`, `constant`, `escape`, `boolean`, `property`, `parameter`, `module`) with colors for all 6 built-in themes + VSCode JSON importer (`keyword.control` scope). `scope_color()` expanded from 8→23 capture names. All 20 language queries expanded: keywords split into storage (`@keyword`) vs control flow (`@keyword.control`), plus operators, punctuation, numbers, booleans, method calls, field access, parameters, escape sequences, macros, attributes, lifetimes. `semantic_token_style()` now checks `controlFlow` modifier on keyword tokens, plus handles `operator`, `boolean`, `lifetime`, `attribute`, `builtinType`. Fixed tree-sitter `reparse()`: always full parse (passing old tree without `tree.edit()` caused stale byte offsets → garbled partial-word coloring). Insert mode now does immediate `update_syntax()` instead of 150ms debounce.

Explorer color overhaul: removed `explorer_dir_fg` distinction — folders/files same base color. Git status propagated recursively to parent dirs (priority M>D>R>A>?). Diagnostic counts propagated recursively to parent dirs. Name fg color priority: error > warning > git > default. GTK indicator column split into own `TreeViewColumn` (no longer clipped by filename column).

Bug fixes: split-down icon changed from pushpin `\u{F0931}` to caret-down `\u{f0d7}`; midline ellipsis `⋯` (`\u{22EF}`); GTK tab bar clip height (`line_height`→`tab_row_height`); split/diff buttons shifted left by action button width; `gtk_editor_bottom()` shared helper eliminates coordinate mismatches between draw, click, and divider handlers; capture-phase GestureDrag and click-handler divider hit-tests both exclude tab bar regions; GTK menu dropdown padding (removed blank header row, added 4px symmetric padding); LSP status no longer downgrades Running→Initializing when semantic tokens temporarily empty; TUI settings button bug added to BUGS.md. Removed Pinned Tabs from roadmap.

---

**Session 244 — TUI rendering artifact fix (5275 tests):**
Mitigated intermittent TUI stale character artifacts. Two fixes in `src/tui_main/mod.rs`: (1) `terminal.clear()` on `Event::Resize` — terminal emulators reflow screen content on resize, desynchronizing the physical display from ratatui's previous-frame buffer; clearing resets both internal buffers so the next draw emits every cell. (2) Popup dismiss detection — track `had_popup_overlay` flag; when picker or folder picker transitions from visible to hidden, call `terminal.clear()` to force full redraw instead of relying on ratatui's incremental diff (which can miss cells where the popup was drawn over the editor). Also removed the "no way to close debug output tab" bug from BUGS.md (already fixed in a prior session).

**Session 243 — LSP status indicator (5275 tests):**
Persistent LSP server status in per-window status bar. `LspStatus` enum in `lsp_manager.rs`: `None` (no LSP for filetype), `Installing` (binary being installed), `Initializing(server_name)` (server started but not ready), `Running(server_name)` (fully indexed), `Crashed`. Server name extracted from command path (`/usr/bin/rust-analyzer` → `rust-analyzer`). `server_has_responded: HashMap<LspServerId, bool>` on `LspManager` tracks first meaningful response; `mark_server_responded(server_id)` called on non-empty hover, definition, and completion responses in `panels.rs`. `lsp_status_for_language()` on `LspManager` checks `initialized` + `server_has_responded` maps. `lsp_status_for_buffer(buffer_id)` on Engine combines `lsp_installing` check + manager query. Render-side readiness override in `build_window_status_line()`: `Running` downgraded to `Initializing` when `BufferState.semantic_tokens` is empty — semantic tokens arrive after full workspace indexing, aligning with hover/go-to-definition readiness (~20s on large Rust projects). Status bar display: `rust-analyzer` (ready, normal color), `rust-analyzer…` (indexing, dimmed), `LSP↓` (installing, dimmed), `LSP✗` (crashed, red), hidden (no LSP). `StatusAction::LspInfo` click runs `:LspInfo` command. 1 new test.

**Session 242 — Clickable status bar segments + line endings (5273 tests):**
Made all per-window status bar segments interactive in both GTK and TUI backends. `StatusAction` enum (`GoToLine`, `ChangeLanguage`, `ChangeIndentation`, `ChangeLineEnding`, `ChangeEncoding`, `SwitchBranch`) added to `StatusSegment.action: Option<StatusAction>` field. `Engine::handle_status_action()` in execute.rs routes each action to the appropriate picker or message. New picker sources: `PickerSource::Languages` (37 languages from `all_known_language_ids()` in lsp.rs, confirm sets filetype via `SyntaxLanguage::from_language_id()` + `Syntax::new_from_language_id()`), `PickerSource::Indentation` (6 presets: Spaces 2/4/8, Tabs 2/4/8, confirm applies `expand_tab`/`tabstop`/`shift_width`), `PickerSource::LineEndings` (LF/CRLF picker, confirm calls `set_line_ending()`). Line ending infrastructure: `LineEnding` enum (`LF`/`Crlf`) in buffer_manager.rs; `LineEnding::detect()` scans first 8KB on file open; `BufferState.line_ending` field; `set_line_ending()` converts all `\r\n` ↔ `\n` in rope and marks dirty; detection re-runs on `reload_from_disk()`. New status bar segments: indentation (`Spaces: 4` / `Tab Size: 4`), line ending (`LF` / `CRLF`), git branch now clickable (opens branch picker). TUI: `status_segment_hit_test()` walks segments by char width at click time; fixed global status bar guard at `row + 2 == term_height` consuming per-window status clicks (added `!engine.settings.window_status_line` guard). GTK: `gtk_status_segment_hit_test()` walks segments by pixel width; `ClickTarget::StatusBarAction` variant; fixed `pixel_to_click_target` second `editor_bottom` calculation not accounting for per-window status or bottom panels (was always using `line_height * 2.0`). `PickerAction::SetLanguage(String)`, `SetIndentation(bool, u8)`, `SetLineEnding(bool)` variants with confirm handlers in picker.rs. 4 new tests.

**Session 241 — Per-window status lines (5265 tests):**
Replaced the single global status bar with per-window status lines (Vim/Neovim behavior). Each window gets a status bar at its bottom edge. Active window shows: bold mode name (text tinted green/purple/red for Insert/Visual/Replace), bold filename, dirty `[+]` flag, macro recording indicator, git branch with ahead/behind, filetype, `utf-8`, cursor `Ln N, Col N`. Inactive windows show: dimmed filename + dirty + cursor position. Colors fully derived from theme — active bar bg = `theme.background.lighten(0.10)` (or `.darken(0.10)` for light themes), fg = `theme.foreground`; inactive uses `theme.status_inactive_bg/fg`. No hardcoded hex colors in rendering code. Global status bar removed when per-window is active; command line remains. New `window_status_line` setting (default `true`); `:set windowstatusline`/`:set nowindowstatusline` (abbreviation `wsl`); `SettingDef` entry in Settings sidebar. New types: `StatusSegment { text, fg, bg, bold }`, `WindowStatusLine { left_segments, right_segments }` in render.rs; `RenderedWindow.status_line: Option<WindowStatusLine>`. `build_window_status_line()` queries per-window buffer state. `build_screen_layout()` reduces `visible_lines` by 1 and skips `build_status_line()` when per-window active. TUI: `render_window_status_line()` draws segments in bottom row of window rect; `render_window()` shadows `area` to shrink editor content; horizontal separator suppressed when upper window has status bar; global status bar layout constraint set to 0; per-window status bar click consumed in mouse handler. GTK: `draw_window_status_bar()` renders Cairo segments with per-segment Pango bold; drawn after scrollbars; global status bar height reduced. 6 new theme fields: `status_mode_normal/insert/visual/replace_bg` (mode text tints), `status_inactive_bg/fg` (inactive bars). All 6 built-in themes updated. VSCode JSON importer inherits from base theme. 6 new tests.

**Session 240 — Cursorline highlight, GTK tab bar polish, breadcrumb picker pre-selection (5199 tests):**
**Cursorline highlight:** `cursorline` setting default changed from `false` to `true`; `cursorline_bg` theme color derived from background via new `Color::cursorline_tint()` method (dark themes lighten 6%, light themes darken 4%); rendered in both GTK (`draw.rs`) and TUI (`render_impl.rs`) as full-width line background behind the cursor line (active window only); priority: DAP stopped > diff > cursorline > normal; `RenderedWindow.cursorline` bool propagated from settings; VSCode theme importer maps `editor.lineHighlightBackground`; all 6 built-in themes derive color from background. Updated existing `test_set_cursorline` test (default now true, abbreviation `cul`/`nocul`).
**Hardcoded colors audit:** Scanned all rendering files, found 59 hardcoded color instances across 5 files (`css.rs` 23, `draw.rs` 12, `panels.rs` 15, `render_impl.rs` 3, `mod.rs` 3). Filed as low-priority bug in BUGS.md with per-file breakdown. Saved feedback memory: always use Theme struct fields, never hardcode colors.
**Breadcrumb picker pre-selection:** `picker_populate_document_symbols()` now pre-selects the symbol whose start line is closest to (and at or before) the cursor position, matching VSCode's behavior of highlighting the current function in the `@` symbol picker. Uses `self.view().cursor.line` to find the best match.
**GTK tab bar padding:** Tab row height increased from `line_height` to `(line_height * 1.6).ceil()` for vertical breathing room; text vertically centered via `text_y_offset`; horizontal padding increased to 14px each side (`tab_pad`); inner gap (name to close button) increased to 10px; outer gap between tabs reduced to 1px. All click hit-test functions updated (`tab_close_hit_test`, `tab_tooltip_hit_test`, click handler in `click.rs`). Command Center search bar minimum width set to 280px.
**Tree-sitter roadmap item:** Added "Richer tree-sitter highlight queries" to PLAN.md — expand all 20 language grammars with comprehensive captures (punctuation, operators, ~25 additional Rust keywords, macro invocations, method calls, attributes, lifetimes) plus new Theme fields.
Files: `render.rs`, `settings.rs`, `engine/picker.rs`, `gtk/draw.rs`, `gtk/click.rs`, `gtk/mod.rs`, `tui_main/render_impl.rs`, `tests/new_vim_features.rs`, `BUGS.md`, `PLAN.md`, `README.md`.

**Session 239 — Tree-style symbol drill-down in breadcrumb picker (5080 tests):**
The `@` symbol picker now shows an expandable tree view instead of a flat list, matching VSCode's Outline behavior.
**Root cause fix: `hierarchicalDocumentSymbolSupport`** — VimCode's LSP init_params was missing `"documentSymbol": { "hierarchicalDocumentSymbolSupport": true }`, causing LSP servers to return flat `SymbolInformation[]` (552 items) instead of hierarchical `DocumentSymbol[]` (56 items with children). Added the capability to `init_params`.
**Hierarchical LSP parsing:** `parse_document_symbols_hierarchical()` + `parse_document_symbol_tree()` in `lsp.rs` preserve `DocumentSymbol` children recursively; `SymbolInfo` gains `children: Vec<SymbolInfo>` field; old flat `parse_document_symbols`/`flatten_document_symbol` removed. `SymbolKind::sort_order()` for consistent kind-based ordering.
**Flat-to-tree reconstruction:** `rebuild_tree_from_containers()` groups flat `SymbolInformation` by `container` field, creating parent nodes with children. Synthetic parent nodes created for containers not found in the symbol list. Skipped when breadcrumb scoped filter is active.
**PickerItem tree fields:** `depth: usize`, `expandable: bool`, `expanded: bool` on `PickerItem`; `PickerPanelItem` mirrors these for rendering.
**Tree building:** `build_symbol_tree_items()` recursively builds depth-first picker items sorted by `SymbolKind::sort_order()` (structs→functions→variables) then alphabetically; top-level containers start expanded.
**Expand/collapse:** `picker_toggle_expand()` toggles state in `picker_all_items` and rebuilds visible tree; `picker_rebuild_visible_tree()` walks depth-first skipping collapsed children. Enter on expandable items toggles expand; Right expands, Left collapses; Enter on leaf confirms. TUI double-click and GTK double-click toggle expand for expandable items in tree mode.
**Flat filter fallback:** typing a query after `@` flattens all items to depth 0 for fuzzy matching.
**Rendering:** both GTK and TUI show `▼`/`▷` expand arrows with indentation; `has_tree` flag adds alignment spacers to non-expandable items when any tree items exist.
**Picker jump centering:** `GotoSymbol`, `GotoLine`, and `OpenFileAtLine` picker actions now call `scroll_cursor_center()` instead of `ensure_cursor_visible()` so the target line appears in the middle of the viewport.
**Bug fixes:** `breadcrumb_scoped_parent` cleared in `open_picker()` to prevent stale scoped filters; `open_picker` was missing this reset.
14 new tests (11 tree + 2 container reconstruction + 1 LSP hierarchical parse).
Files: `lsp.rs`, `engine/mod.rs`, `engine/picker.rs`, `engine/keys.rs`, `engine/tests.rs`, `engine/panels.rs`, `render.rs`, `gtk/mod.rs`, `gtk/draw.rs`, `tui_main/mouse.rs`, `tui_main/render_impl.rs`.

**Session 238 — Breadcrumb navigation, chat prefix, picker UX fixes, scoped symbol filtering (5055 tests):**
Continuation of session 237 with 8 features + multiple bug fixes, 17 new tests. **Command Center `chat` prefix:** `chat` opens AI panel, `chat <question>` sends directly to provider; unconfigured state shows setup guidance; listed in `?` help + empty-query hints (5 tests). **Breadcrumb clickable navigation:** clicking directory segments opens file picker for that dir; clicking symbol segments opens `@` picker scoped to siblings (filtered by LSP `container` field matching `parent_scope`); `BreadcrumbSegment` gains `index`/`path_prefix`/`symbol_line` fields; `build_breadcrumbs_for_active_group()` public API; both GTK + TUI backends (3 tests). **Breadcrumb focus mode (`<leader>b`):** enters keyboard-driven mode highlighting the last segment; h/l navigate segments; Enter opens scoped picker; Escape exits; `BreadcrumbSegmentInfo` engine-side struct with `parent_scope`; `rebuild_breadcrumb_segments()`/`breadcrumb_open_scoped()` methods; `breadcrumb_scoped_parent: Option<Option<String>>` filter consumed by `picker_populate_document_symbols`; visual highlight in both TUI + GTK renderers (7 tests). **`<leader>so` document outline:** opens `@` symbol picker directly; palette entry "Go to Symbol in Editor (Outline)" (1 test). **Tree-sitter child scope methods:** `children_of_scope()`/`top_level_scopes()`/`collect_child_scopes()` in `syntax.rs` for future tree-sitter fallback when LSP unavailable. **Picker click-through fix (both backends):** TUI unified picker now intercepts all mouse events (click to select, double-click to confirm, scroll to navigate); GTK picker guard at top of `handle_mouse_click_msg` + `CtrlMouseClick`/`MouseDoubleClick` guards. **GTK picker scroll wheel:** added picker guard to `MouseScroll` handler; scroll moves selection by 3 items per step. **TUI picker scroll speed:** changed from 1-item to 3-item steps. **GTK picker click off-by-one fix:** results_top changed from `popup_y + 3*lh` to `popup_y + 2*lh + 1.0` matching draw code. **GTK breadcrumb click scoped:** changed from flat `breadcrumb_click()` to `rebuild_breadcrumb_segments()` + `breadcrumb_open_scoped()`. **GTK redraw after handle_key:** added `draw_needed.set(true)` after every `handle_key` call so breadcrumb focus highlight is visible immediately. **Jump list in picker:** added `push_jump_location()` before `OpenFile`/`OpenFileAtLine`/`GotoSymbol`/`GotoLine` actions in `picker_confirm()` so Ctrl-O works after picker navigation. **Status-line confirmations:** audited — already fully migrated to `show_dialog()`, `PromptKind` removed in prior session; marked complete in PLAN.md.
Files: `engine/mod.rs`, `engine/keys.rs`, `engine/picker.rs`, `engine/tests.rs`, `engine/execute.rs`, `syntax.rs`, `render.rs`, `gtk/mod.rs`, `gtk/draw.rs`, `gtk/click.rs`, `tui_main/mouse.rs`, `tui_main/render_impl.rs`, `README.md`, `PLAN.md`.

---

**Session 237 — VSCode undo coalescing, smart indent, buffer picker, keybindings picker, crash logging, bicep comments, :$ EOF (5038 tests):**
7 features + 1 bug fix, 36 new tests. **Bug fix — VSCode undo granularity:** `handle_vscode_key()` called `start_undo_group()`/`finish_undo_group()` per keystroke; now keeps group open across consecutive character insertions via `vscode_undo_group_open` + `vscode_undo_cursor` fields, breaking on non-char actions or cursor jumps (5 tests). **Smart indent (language-aware):** `smart_indent_for_newline()` + `line_triggers_indent()` + `auto_outdent_for_closing()` in `motions.rs`; Enter/`o` add extra indent after `{`/`(`/`[` (universal), `:` (Python), `do`/`then` (Lua/Ruby/Shell); typing `}`/`)`/`]` as first non-blank auto-outdents; `==` also language-aware (9 tests). **Auto-detect indentation:** `BufferState.detected_indent` + `detect_indent()` analyzes indent deltas on file open; `effective_shift_width()` prefers detected over `settings.shift_width`; all indent ops use it (6 tests). **Buffer picker:** `PickerSource::Buffers` via `<leader>sb` / `:Buffers`; lists open buffers with icons, dirty/active flags (4 tests). **Keybindings picker:** `PickerSource::Keybindings` via `<leader>sk`; parses reference text into items by category; shows configurable panel_keys with actual values + user remaps marked; Help > Key Bindings menu wired to `:Keybindings` (8 tests). **Crash logging:** `crash_log_path()` + `write_crash_log()` in `swap.rs` using `std::env::temp_dir()` (cross-platform); GTK panic hook prints log path + GitHub issues URL to stderr; fixed URL to `JDonaghy/vimcode` (1 test). **Bicep comments:** Added `"bicep"` to `//`-family in `comment.rs` (1 test). **`:$` EOF + line addresses:** `:$`, `:+N`, `:-N`, `:.`, `:0` as standalone ex commands (5 tests).
Files: `engine/mod.rs`, `engine/vscode.rs`, `engine/keys.rs`, `engine/motions.rs`, `engine/execute.rs`, `engine/picker.rs`, `engine/ext_panel.rs`, `engine/tests.rs`, `buffer_manager.rs`, `comment.rs`, `swap.rs`, `render.rs`, `gtk/mod.rs`, `tui_main/mod.rs`.

**Session 236 — ratatui 0.29 upgrade + colored underlines + tab bar scroll fix (4987 tests):**
Upgraded ratatui 0.27→0.29 (crossterm 0.27→0.28). Unlocks `cell.underline_color` for per-cell colored underlines in TUI. **Colored underlines:** Tab accent uses `tab_active_accent` theme color via `underline_color` on `set_cell_styled()`; diagnostic underlines colored by severity (`diagnostic_error`/`warning`/`info`/`hint`); spell error underlines use `spell_error` theme color. Requires terminal SGR 58 support (kitty, WezTerm, iTerm2, foot, recent Alacritty; older terminals fall back to white underline). **API migrations:** All `buf.get_mut(x,y)` → `buf[(x,y)]` index syntax (~40 occurrences across 4 files); `frame.size()` → `frame.area()` (6); `frame.set_cursor()` → `frame.set_cursor_position()` (1); `terminal.size()` returns `Size` instead of `Rect` — changed `handle_mouse()`, `handle_explorer_context_action()`, `compute_tui_tab_drop_zone()` params from `Rect` to `Size`. `set_cell_styled()` gains `underline_color: Option<RColor>` parameter. `render_tab_bar()` gains `focused_accent: Option<ratatui::style::Color>` parameter. `#![allow(dead_code)]` on `icons.rs` for unused icon constants. **Bug fix — TUI tab bar scroll death spiral:** `render_tab_bar()` returned tab COUNT but `set_tab_visible_count()` stored it as `tab_bar_width` (column width). With 5 tabs visible, engine thought 5 columns available → showed fewer tabs → reported smaller count → death spiral. Fixed to return `(tab_end_for_content - area.x) as usize` (available width in columns), matching GTK backend's `available_cols`. Also fixed `tab_display_width()` off-by-one: `name_len + 3` → `name_len + 2` (close button + separator = 2, not 3).
Files: `Cargo.toml`, `Cargo.lock`, `icons.rs`, `tui_main/mod.rs`, `tui_main/render_impl.rs`, `tui_main/panels.rs`, `tui_main/mouse.rs`, `core/engine/windows.rs`.

**Session 235 — Active tab accent indicator across editor groups (4987 tests):**
Added `tab_active_accent: Color` field to `Theme` struct — a thin colored line at the top of the active tab in the focused editor group, matching VSCode's `tab.activeBorderTop` behavior. Only the truly active tab in the focused group gets the accent; unfocused groups show normal active tab styling. GTK: 2px accent bar drawn inside `draw_tab_bar()` immediately after the active tab's background fill; `accent_color: Option<render::Color>` parameter added to `draw_tab_bar()`. TUI: `focused_accent: Option<ratatui::style::Color>` parameter on `render_tab_bar()`; active tab in focused group gets `Modifier::UNDERLINED` (white underline — colored underlines require ratatui 0.28+). VSCode JSON theme importer reads `tab.activeBorderTop`. Accent colors per theme: OneDark `#61afef`, Gruvbox `#d65d0e`, Tokyo Night `#7aa2f7`, Solarized `#268bd2`, VSCode Dark `#007acc`, VSCode Light `#005fb8`.
Files: `render.rs`, `gtk/draw.rs`, `tui_main/render_impl.rs`.

**Session 234 — Nerd Font icon handling + fallback + bundling + drag fix (4987 tests):**
**Phase 1 — Icon registry centralization:** Expanded `src/icons.rs` from 30 to ~160 lines. Added `Icon` struct with `nerd` and `fallback` fields, ~45 named constants covering activity bar, file explorer, debug toolbar, source control, terminal, and editor features. `AtomicBool` toggle via `set_nerd_fonts(bool)` / `nerd_fonts_enabled()`. `file_icon()` routed through constants. Replaced ~90+ hardcoded `\u{...}` escapes across 11 files: `gtk/mod.rs` (activity bar buttons, explorer toolbar, file tree), `gtk/draw.rs` (lightbulb, debug, SC, extensions, AI panels, split/diff buttons), `gtk/tree.rs`, `tui_main/panels.rs` (activity bar, explorer, SC, search, extensions, AI, debug), `tui_main/render_impl.rs` (split/diff buttons, lightbulb), `render.rs` (DEBUG_BUTTONS, expand/collapse, breakpoints), `core/engine/ext_panel.rs`, `core/plugin.rs`. Added `pub mod icons` to `lib.rs` so core modules can access icons.
**Phase 2 — `use_nerd_fonts` setting:** Added `use_nerd_fonts: bool` field to `Settings` (default `true`). Wired into `set_bool_option` (`:set nerdfonts`/`:set nonerdfonts`/`:set nf`), `query_option`, `get_value_str`, `set_value_str`, `display_all`. `SettingDef` entry in Appearance category. Both TUI and GTK call `icons::set_nerd_fonts()` at startup from settings.
**Phase 3 — Bundled Nerd Font subset for GTK:** Created 13KB subset of `SymbolsNerdFont-Regular.ttf` via `pyftsubset` with only ~60 needed glyphs (`data/fonts/vimcode-icons.ttf`). MIT + OFL licensed (`data/fonts/LICENSE-NerdFonts`). `install_bundled_icon_font()` in `gtk/util.rs` writes font to `~/.local/share/fonts/` at startup (skips if correct size), triggers `fc-cache`. CSS `.activity-button` uses `font-family: 'Symbols Nerd Font', monospace`. File tree icon cell renderer prefers `"Symbols Nerd Font, {user_font}"`.
**Extension fallback icons:** Added `fallback_icon: Option<char>` to `PanelRegistration`. Lua API: `vimcode.panel.register({ fallback_icon = "G" })`. `resolved_icon()` method returns nerd or fallback based on global flag; falls back to first letter of title if no explicit fallback. Both TUI and GTK activity bars use `panel.resolved_icon()`.
**Bug fix — drag-to-select leaking across editor groups:** Added `mouse_drag_origin_window: Option<WindowId>` to Engine. `mouse_drag()` locks to origin window on first drag; subsequent calls to different windows ignored. Cleared on `mouse_click()`, `mouse_double_click()`, and mouse-up in both backends. 1 new test.
Files: `icons.rs`, `lib.rs`, `render.rs`, `core/settings.rs`, `core/engine/mod.rs`, `core/engine/keys.rs`, `core/engine/ext_panel.rs`, `core/engine/tests.rs`, `core/plugin.rs`, `gtk/mod.rs`, `gtk/draw.rs`, `gtk/css.rs`, `gtk/tree.rs`, `gtk/util.rs`, `tui_main/mod.rs`, `tui_main/panels.rs`, `tui_main/render_impl.rs`, `tui_main/mouse.rs`, `data/fonts/vimcode-icons.ttf`, `data/fonts/LICENSE-NerdFonts`, `tests/ext_panel.rs`.

**Session 233 — Explorer focus UX polish + GTK fixes (4986 tests):**
Explorer focus visibility improvements: stronger `sidebar_sel_bg` colors across all 6 themes (OneDark `#373d4a`, Gruvbox `#504945`, Tokyo Night `#33395a`, Solarized `#0a4a5a`, Dark+ `#04395e`, Light+ `#b4d9ff`); brighter `explorer_active_bg` for current-file highlight when explorer unfocused. TUI: suppress current-editor-file highlight (`is_active`) when `explorer_has_focus` is true; clicking explorer tree sets `explorer_has_focus`. Ctrl-W h now focuses explorer: GTK handles `window_nav_overflow` (left overflow → `Msg::FocusExplorer`); TUI adds `Explorer` case to overflow match. GTK: `OpenFileFromSidebar` clears `explorer_has_focus`/`tree_has_focus` (fixes 100% CPU + stuck focus); `row_activated` handles directory expand/collapse; j/k/arrow keys pass through to TreeView; `ExplorerActivateSelected` message for programmatic activation. Swap recovery: skip dialog when swap content matches disk file. Known bug filed: GTK Enter on folder after arrow-key nav requires two presses.
Files: `render.rs`, `tui_main/panels.rs`, `tui_main/mouse.rs`, `tui_main/mod.rs`, `gtk/mod.rs`, `gtk/css.rs`, `core/engine/ext_panel.rs`, `BUGS.md`.

**Session 232 — Inline new file/folder in explorer tree (4985 tests):**
`ExplorerNewEntryState` struct with inline editing in explorer tree. Replaced status-line prompt (TUI) and modal dialog (GTK) with inline editable row inserted under target directory. GTK: bordered text field via CSS `treeview entry` styling; `CellRendererText` editable mode; `Msg::StartInlineNewFile/Folder`; `ExplorerAction` dispatch via `idle_add_local_once` to avoid RefCell panics. TUI: inverted-cursor rendering with virtual row interleaving in `render_new_entry_row()`; key routing guard for `explorer_new_entry`. Engine: `start_explorer_new_file/folder()`, `handle_explorer_new_entry_key()` (Escape/Return/BackSpace/Delete/Left/Right/Home/End/printable). Removed `PromptKind::NewFile/NewFolder` and `show_name_prompt_dialog()`. Tree helpers: `find_tree_iter_for_path()`, `remove_new_entry_rows()`. Swap recovery: compare swap content with disk, silently delete if identical. 10 new tests + 1 swap recovery test.
Files: `core/engine/mod.rs`, `core/engine/buffers.rs`, `core/engine/ext_panel.rs`, `core/engine/tests.rs`, `gtk/mod.rs`, `gtk/tree.rs`, `gtk/css.rs`, `tui_main/mod.rs`, `tui_main/panels.rs`, `tui_main/mouse.rs`, `tui_main/render_impl.rs`, `tests/swap_recovery.rs`.

**Session 231 — Git branch switcher in status bar (4955 tests):**
Clickable branch name in status bar opens `PickerSource::GitBranches` unified picker. Status bar shows ahead/behind counts (`↑N ↓N`). `status_branch_range: Option<(usize, usize)>` on `ScreenLayout` for click detection. TUI: status bar row click handler in `mouse.rs`. GTK: click handler in `handle_mouse_click_msg()` reconstructs column range from engine state. `:Gbranches` command opens branch picker. Fixed picker confirm using `Gcheckout` (nonexistent) → `Gswitch`. Updated "Git: Switch Branch" palette entry to use `Gbranches`. 6 new tests.
Files: `render.rs`, `engine/picker.rs`, `engine/execute.rs`, `engine/mod.rs`, `engine/tests.rs`, `tui_main/mouse.rs`, `gtk/mod.rs`.

**Session 230 — Command Center enhancements + `<leader>sw` (4937 tests):**
Added Command Center prefix routing: `%` for live grep (extracted `picker_cc_grep_search()`), `debug` keyword prefix (reads `.vimcode/launch.json` / `.vscode/launch.json` via `picker_populate_debug_configs()`), `task` keyword prefix (reads tasks.json via `picker_populate_tasks()`, `EngineAction::RunInTerminal`). Placeholder hints dropdown: 9 mode items shown when CC opens with empty query (Go to File, Commands, Symbols, Line, Grep, Debug, Task, Help). `hint_item()` helper. `<leader>sw` greps word under cursor via `word_under_cursor()` + opens Grep picker pre-filled. `:GrepWord` command + "Search: Word Under Cursor" palette entry. 30 new tests.
Files: `engine/picker.rs`, `engine/keys.rs`, `engine/execute.rs`, `engine/mod.rs`, `engine/tests.rs`.

**Session 229 — Command Center (4847 tests):**
Clickable search box in menu bar opens unified picker with prefix-based mode switching: no prefix = fuzzy files, `>` = command palette, `@` = document symbols (LSP `textDocument/documentSymbol`), `#` = workspace symbols (LSP `workspace/symbol`), `:` = go to line, `?` = help. Added `PickerSource::CommandCenter`, `PickerAction::GotoLine`/`GotoSymbol`. LSP types: `SymbolInfo`, `SymbolKind` (with icons/labels), hierarchical + flat response parsing. `picker_filter_command_center()` prefix routing, `fuzzy_filter_items()` shared helper. `:CommandCenter` ex command + palette entry. GTK `Msg::OpenCommandCenter` + search box click. TUI search box click. 11 new tests. Also added 14 new roadmap items (Command Center enhancements, breadcrumbs, status bar, tab bar, layout, minimap).
Files: `engine/mod.rs`, `engine/picker.rs`, `engine/execute.rs`, `engine/panels.rs`, `engine/tests.rs`, `lsp.rs`, `lsp_manager.rs`, `gtk/mod.rs`, `tui_main/mouse.rs`.

**Session 228 — Menu bar MRU history arrows (4814 tests):**
Moved `◀ ▶` nav arrows from per-group tab bars to the menu bar row, centered alongside a VSCode Command Center-style search box showing the workspace directory name. Nav arrows navigate a global MRU tab history across all editor groups (like VSCode). Removed per-group tab bar nav arrows and overflow indicators. History starts empty on startup (arrows greyed out); seeded with initial tab so first switch records origin. Forward history truncated when navigating to a new tab (undo/redo style). `MenuBarData.title` changed from active filename to workspace dir name. GTK: `nav_arrow_rects: Rc<RefCell<>>` caches exact draw positions for click hit-testing; `EventSequenceState::Claimed` prevents WindowHandle double-click-to-maximize in nav+search area; `menu_bar_da.queue_draw()` for proper redraws. TUI: arrows + search box centered as one unit in menu bar row. `tab_nav_push()` called from explicit navigation methods only; `tab_mru_touch()` no longer pushes to nav history. 7 tab nav tests rewritten.
Files: `engine/mod.rs`, `engine/windows.rs`, `engine/tests.rs`, `render.rs`, `gtk/draw.rs`, `gtk/mod.rs`, `tui_main/render_impl.rs`, `tui_main/mouse.rs`.

**Session 227 — Back/Forward navigation arrows + bug fixes (4811 tests):**
Tab access history with clickable nav arrows; `:tabmove` 1-based; `Ngt` to tab N; TUI split button dedup; Explorer preview scroll; multi-swap recovery; `:q` diff tab fix; SC panel single/double-click; `C` to commit; `p`/`P` swap. Engine fields: `tab_nav_history`, `tab_nav_index`, `tab_nav_navigating`. Methods: `tab_nav_push/back/forward/switch_to/can_go_back/can_go_forward`. PanelKeys: `nav_back` (`<C-A-Left>`), `nav_forward` (`<C-A-Right>`). `:navback`, `:navforward` commands + palette entries. GTK: `NavBtnMap`, nav arrows in `draw_tab_bar`, click targets `NavBack`/`NavForward`. TUI: `◀ ▶` at left of tab bar. 5 new tab nav tests.
Files: `engine/mod.rs`, `engine/windows.rs`, `engine/keys.rs`, `engine/execute.rs`, `engine/source_control.rs`, `engine/panels.rs`, `engine/ext_panel.rs`, `engine/tests.rs`, `settings.rs`, `render.rs`, `gtk/mod.rs`, `gtk/draw.rs`, `gtk/click.rs`, `tui_main/mod.rs`, `tui_main/render_impl.rs`, `tui_main/mouse.rs`.

**Session 226 — Tab scroll-into-view (4784 tests):**
Added per-group tab bar scrolling so the active tab is always visible. `EditorGroup.tab_scroll_offset: usize` — index of first visible tab. `ensure_active_tab_visible()` called from `goto_tab`, `next_tab`, `prev_tab`, `new_tab`, `close_tab`, `open_file_in_tab`, `tab_switcher_confirm`, and `:tabmove`. `◀`/`▶` overflow indicators when tabs hidden on either side. Both GTK and TUI: rendering, click handling, tooltips, drag-drop adjusted for scroll offset. 6 new tests.
Files: `engine/mod.rs`, `engine/windows.rs`, `engine/execute.rs`, `render.rs`, `gtk/draw.rs`, `tui_main/render_impl.rs`, `tui_main/mouse.rs`, `engine/tests.rs`.

**Session 225 — Bug fixes & crash hardening (4778 tests):**
Fixed 7 bugs: explorer reveal on folder open, visual yank cursor position, YAML syntax corruption, completion_prefix crash, swap flush on panic (emergency_swap_flush + global engine pointer), search viewport_lines accounting, stale WindowId crash (repair_active_window self-healing). Added "Tab Navigation & Command Center" plan items.
Files: `accessors.rs`, `motions.rs`, `panels.rs`, `visual.rs`, `windows.rs`, `tests.rs`, `swap.rs`, `syntax.rs`, `gtk/mod.rs`, `tui_main/mod.rs`, `tests/visual_mode.rs`.

**Session 224 — Release prep v0.5.1 (4769 tests):**
Bump version to 0.5.1 for patch release. Update test counts across docs (4736→4769). Tick off macOS CI/Homebrew roadmap item in PLAN.md (implemented in Session 218 CI commit, but release.yml changes hadn't reached `main` yet — this release will be the first to produce macOS binaries and update the Homebrew tap). Investigated why macOS build job wasn't running in release workflow: the `build-macos` job definition only existed on `develop`, not `main`.

**Session 223 — Sidebar focus consolidation (4736 tests):**
Consolidated sidebar focus state into the engine so key routing correctness is testable. Added `explorer_has_focus` and `search_has_focus` fields to Engine struct (previously only tracked in TUI-local `sidebar.has_focus`). Added `sidebar_has_focus()` aggregator (checks all 8 panel focus flags) and `clear_sidebar_focus()` helper in accessors.rs. Added guards in `handle_key()` — explorer/search focus blocks normal-mode key processing. TUI: `sync_sidebar_focus()` helper derives engine focus from TUI sidebar state; called after mouse events and before render; `clear_sidebar_focus()` replaces manual 6-field clears in Ctrl-W navigation. GTK: syncs `explorer_has_focus` in FocusExplorer/ToggleFocusExplorer/FocusEditor handlers. 8 new tests.
Files: `src/core/engine/mod.rs`, `src/core/engine/accessors.rs`, `src/core/engine/keys.rs`, `src/core/engine/tests.rs`, `src/tui_main/mod.rs`, `src/gtk/mod.rs`.

**Session 222 — Hide single tab (4728 tests):**
Added `hide_single_tab` setting (default false). When enabled, hides the tab bar row for editor groups with only one tab, reclaiming the row for editor content. Breadcrumbs row preserved when breadcrumbs is enabled. Works in both GTK and TUI backends with proper click/hover/drag handling. `:set hidesingletab`/`:set hst` toggle. Settings sidebar entry in Appearance category. Multi-group guard: tab bars always shown when ≥2 editor groups exist (prevents confusion when session restores multiple groups each with 1 tab). 7 new tests.
Files: `src/core/settings.rs`, `src/core/engine/accessors.rs`, `src/core/engine/windows.rs`, `src/core/engine/tests.rs`, `src/gtk/draw.rs`, `src/gtk/click.rs`, `src/gtk/mod.rs`, `src/tui_main/render_impl.rs`, `src/tui_main/mouse.rs`, `src/render.rs`.

**Session 221 — Refactor App::update() (4721 tests):**
Extracted the monolithic `update()` method in `src/gtk/mod.rs` from ~4,495 lines to ~430 lines. Created 19 helper methods on `impl App` that group related `Msg` variants: `handle_key_press()`, `handle_poll_tick()`, `handle_mouse_click_msg()`, `handle_mouse_drag_msg()`, `handle_mouse_up_msg()`, `handle_tab_right_click()`, `handle_editor_right_click()`, `handle_terminal_msg()`, `handle_menu_msg()`, `handle_debug_sidebar_msg()`, `handle_sc_sidebar_msg()`, `handle_ext_sidebar_msg()`, `handle_ext_panel_msg()`, `handle_ai_sidebar_msg()`, `handle_sidebar_panel_msg()`, `handle_explorer_msg()`, `handle_find_replace_msg()`, `handle_file_ops_msg()`, `handle_dialog_msg()`. Added `terminal_cols()` utility replacing 4 duplicated terminal column computations.
Follow-up /simplify review fixed 5 deduplication issues: extracted `map_gtk_key_name()`/`map_gtk_key_with_unicode()` (7 duplicated key mapping blocks → 2 free functions), `focus_editor_if_needed()` (13 grab_focus patterns → 1 method), `dispatch_engine_action()` (~160 lines duplicated between main key handling and macro playback → 1 method), cached `cached_ui_line_height` field (4 inline Pango font metric computations → 1 cached value), quit arms now call existing `save_session_and_exit()` (also fixed missing window dimension save). File reduced from 9,448 to 9,258 lines (190 lines net removal).
Files: `src/gtk/mod.rs`.

**Session 220 — Bug Fix Sweep (4721 tests):**
Fixed all 3 open BUGS.md issues:
1. **CLI file arg restores session**: Both GTK and TUI backends unconditionally called `restore_session_files()` then opened the CLI file on top. Fix: skip session restore when CLI arg provided; use `open_file_with_mode(path, OpenMode::Permanent)` to load file into the initial scratch window's tab (no leftover "[No Name]" tab). Files: `src/gtk/mod.rs`, `src/tui_main/mod.rs`.
2. **TUI single-group tab drag**: `compute_tui_tab_drop_zone()` single-group branch only handled `TabReorder`, returning `DropZone::None` for content area. Added edge zone detection using `terminal_size` parameter. `render_tab_drag_overlay()` single-group branch only rendered `TabReorder` highlight — added `Center`/`Split` zone rendering using `editor_area` bounds. Files: `src/tui_main/render_impl.rs`, `src/tui_main/mouse.rs`.
3. **GTK "Don't know color ''" warnings**: Explorer TreeStore rows initialized columns 3 (foreground) and 5 (indicator color) with `""`. GTK CSS parser warned on empty color strings. Replaced with valid hex defaults (`dir_fg_hex` in initial rows, `modified_color` in `update_tree_indicators()` clear path). File: `src/gtk/tree.rs`.
Also added `hide_single_tab` setting to PLAN.md roadmap (default false, hides tab bar when single tab for traditional Vim feel).

**Session 219 — Code Summaries System (4721 tests):**
Created `SUMMARIES/` directory with 16 summary files covering all 45 major source files (~106K total lines). Each summary contains: purpose, line count, key types, key public methods. Added maintenance instructions to CLAUDE.md ("Code Summaries" section) requiring updates when source files are modified. Saves significant tokens in future sessions by providing scannable overviews instead of reading full source files.
Files: `SUMMARIES/gtk_mod.md`, `gtk_draw.md`, `gtk_helpers.md`, `engine_mod.md`, `engine_keys.md`, `engine_motions.md`, `engine_execute.md`, `engine_visual.md`, `engine_buffers.md`, `engine_windows.md`, `engine_small_submodules.md`, `engine_tests.md`, `render.md`, `tui_modules.md`, `core_modules.md`. Also: `CLAUDE.md` (added summaries section).

**Session 218 — GTK + TUI Split Refactor (4721 tests):**
Split `src/main.rs` (16,826 lines) → `src/gtk/` directory with 6 submodules: `mod.rs` (9,267 — App, Msg, SimpleComponent), `draw.rs` (5,519 — all draw_* functions), `click.rs` (575), `css.rs` (525), `util.rs` (468), `tree.rs` (432). Split `src/tui_main.rs` (14,190 lines) → `src/tui_main/` directory with 4 submodules: `mod.rs` (4,166 — structs, event_loop), `panels.rs` (3,931), `render_impl.rs` (3,736), `mouse.rs` (2,379). Thin `main.rs` (55 lines) dispatches to `gtk::run()` or `tui_main::run()`. All 4,721 tests pass; zero API changes.
Files: `src/main.rs`, `src/gtk/mod.rs`, `src/gtk/draw.rs`, `src/gtk/click.rs`, `src/gtk/css.rs`, `src/gtk/util.rs`, `src/gtk/tree.rs`, `src/tui_main/mod.rs`, `src/tui_main/panels.rs`, `src/tui_main/render_impl.rs`, `src/tui_main/mouse.rs`.

**Session 217 — Engine Split Refactor (4721 tests):**
Split monolithic `src/core/engine.rs` (51,825 lines) into `src/core/engine/` directory with 20 submodule files. `engine/mod.rs` (3,334 lines) contains types, structs, enums, Engine struct, `new()`, and free functions. Largest submodules: `tests.rs` (14,334), `keys.rs` (7,056), `motions.rs` (4,628). All 4,721 tests pass with zero changes to public API. No changes to `main.rs`, `tui_main.rs`, `render.rs`, `lib.rs`, or any non-engine files.
Files: `src/core/engine/mod.rs` and 19 submodule files in `src/core/engine/`.

**Session 216 — Explorer Tree Indicators (4721 tests):**
Added right-aligned indicators on explorer tree rows (like VSCode) showing git status and LSP diagnostic counts. Both GTK and TUI backends. Extensive iteration on diagnostic count accuracy to match VSCode behavior.
- **Engine**: `explorer_indicators()` returns `(HashMap<PathBuf, char>, HashMap<PathBuf, (usize, usize)>)` — git status map (canonical path → `M`/`A`/`?`/`D`/`R`) + per-file deduplicated (error, warning) counts. Git status from `sc_file_statuses` (not dirty-buffer tracking). Diagnostics deduplicated by `(code, message)` pairs. Error-severity diagnostics from sources listed in extension `ignore_error_sources` are excluded (e.g. rust-analyzer's internal analysis produces false positives; real errors come from `rustc` via cargo check).
- **TUI** (`tui_main.rs`): Indicators computed once before row loop; rendered right-aligned: diagnostic errors (red) → warnings (yellow) → git status letter (colored by type). Counts capped at `9+`. `sc_refresh()` also runs when Explorer panel is active.
- **GTK** (`main.rs`): TreeStore expanded from 4→6 columns (col 4=indicator text, col 5=indicator color hex). `CellRendererText` with `xalign=1.0` for right-alignment. `update_tree_indicators()` recursive walker. Called on `RefreshFileTree` and periodically (~1s via `SearchPollTick`). Ordering: diagnostics first, then git status (matching VSCode).
- **LSP** (`lsp.rs`): `Diagnostic` struct gains `code: Option<String>` field parsed from LSP `code` (string or numeric). `relatedInformation` capability set to `true` (was `false`, causing rust-analyzer to flatten related diagnostics into separate entries). Default severity for missing field changed from `Error` to `Hint`.
- **Extensions** (`extensions.rs`): `LspConfig` gains `ignore_error_sources: Vec<String>` — per-extension list of diagnostic sources whose errors should be excluded from explorer counts. Rust extension sets `ignore_error_sources = ["rust-analyzer"]`. No hardcoded LSP names in core.
- **Bug logged**: `cargo run -- file.rs` restores entire previous session (added to BUGS.md).
- 3 tests: `test_explorer_indicators_empty`, `test_explorer_indicators_git_status`, `test_explorer_indicators_diagnostics`.
Files: `src/core/engine.rs`, `src/core/lsp.rs`, `src/core/extensions.rs`, `src/main.rs`, `src/tui_main.rs`, `vimcode-ext/rust/manifest.toml`.

**Session 215 — Bug Fix Sweep (4712 tests):**
Fixed 3 remaining BUGS.md items:
- **Visual mode `x`**: Added `'x'` as alias for `'d'` in `handle_visual_key()` with `pending_key.is_none()` guard (preserves `rx` replace). 2 new tests.
- **Ctrl+V paste in search/command/replace inputs**: Added `clipboard_paste()` handler to `handle_search_key()` (/ and ? search), `handle_command_key()` (: command), and TUI project search/replace input mode. Made `clipboard_paste()` public for TUI access. GTK project search uses native Entry widgets (already handled paste).
- **Search highlights wrong text in splits**: `engine.search_matches` stored char offsets from active buffer only, but `build_spans()` applied them to all visible windows. Fixed: `build_rendered_window()` now computes per-buffer matches via `compute_search_matches_for_buffer()` in render.rs. `build_spans()` takes `search_matches` + `is_active_buffer` params. `search_index` (current match highlight) only applies to active buffer.
Files: `src/core/engine.rs`, `src/render.rs`, `src/tui_main.rs`. No open bugs.

**Session 214 — Bug Sweep Continued (4706 tests):**
Continued working through BUGS.md items. Five rendering/UI fixes across TUI and GTK backends:
- **TUI hover dismiss consumes click** (`tui_main.rs:4819`): Removed early `return` after `dismiss_editor_hover()` — click now falls through to editor handler, matching GTK behavior. This also fixed the "off by 2 lines" symptom where double-click selection appeared to select wrong lines.
- **TUI selection wrong position with wrap** (`tui_main.rs:render_selection`): `render_selection()` used `scroll_top + row_idx` to compute buffer line, which is incorrect for wrapped text where multiple visual rows share one buffer line. Fixed to use `line.line_idx` and adjust selection columns by `segment_col_offset`.
- **Markdown typing color bleed**: Added `tick_syntax_debounce()` engine method — 150ms debounced syntax refresh during insert mode. Called from both GTK and TUI idle loops. Prevents stale byte offsets in deferred syntax highlights from causing wrong colors near colored sections.
- **GTK scrollbar / tab group divider overlap**: Scrollbar inset 2px from group edge (`margin_start - 2`); divider gesture `drag_begin` skips claiming when click is in scrollbar zone (rightmost 10px of any window rect).
- **TUI fuzzy finder stale chars**: `render_picker_popup` and `render_folder_picker` didn't clear their background area before drawing. Added full background clear pass (fill with spaces) to both functions.
All BUGS.md items now resolved. No open bugs.

**Session 213 — Bug Sweep: All BUGS.md Items Fixed (4710 tests):**
Worked through every bug in BUGS.md. Fixes across both TUI and GTK backends:
- **Explorer tree colors** (TUI+GTK): Added `explorer_dir_fg`/`explorer_active_bg` Theme fields; directories get distinct warm color, active buffer row gets subtle background; GTK TreeStore expanded to 4 columns for per-row foreground.
- **:Explore opens in same window**: `netrw_activate_entry()` uses `switch_window_buffer()` instead of `open_file_in_tab()`.
- **Search n/N not scrolling**: Added `ensure_cursor_visible()` after `jump_to_search_match()`; empty `?<enter>` repeats previous search.
- **Git commit double-line status**: `sc_do_commit()` truncates output to first line.
- **Double-click word-wise drag**: `mouse_drag_word_mode`/`mouse_drag_word_origin` fields; word boundary snapping in `mouse_drag()`.
- **Ctrl+V paste in fuzzy finder**: Added `"v" if ctrl` arm in `handle_picker_key()`.
- **TUI hover dismiss consumes click** (`tui_main.rs:4819`): Removed early `return` after `dismiss_editor_hover()` — click now falls through to editor handler, matching GTK.
- **TUI selection wrong position with wrap** (`tui_main.rs:render_selection`): Replaced `scroll_top + row_idx` with `line.line_idx`; adjusted selection columns by `segment_col_offset`.
- **Markdown typing color bleed**: Added `tick_syntax_debounce()` — 150ms debounced syntax refresh in both GTK and TUI idle loops.
- **GTK scrollbar/divider overlap**: Scrollbar inset 2px from group edge; divider gesture skips claiming when click is in scrollbar zone.
- **TUI fuzzy finder stale chars**: Added full background clear pass to `render_picker_popup()` and `render_folder_picker()`.

**Session 212 — Selectable/Copyable Hover Popup Text (4710 tests):**
`HoverSelection` struct with anchor/active positions; `EditorHoverPopup.selection` field; `extract_text()` extracts selected range from rendered lines. Mouse drag selection: click on focused popup starts selection, drag extends; TUI: `hover_selecting` state, cell-to-content coordinate conversion; GTK: drag handler extends selection using char_width/line_height. Keyboard copy: `y`/`Y` or Ctrl-C copies selected text (or all popup text if no selection) to system clipboard; `handle_editor_hover_key()` now accepts `ctrl` parameter. Selection highlight rendering: TUI swaps fg/bg for selected chars; GTK uses Pango `AttrColor::new_background()` with theme selection color. Focus indicator updated: GTK shows "y:copy  Tab:links  Esc:close" when popup focused. Bug fix — GTK modifier key dismiss: bare modifier keys (`Control_L`, `Shift_L`, etc.) no longer dismiss focused hover popup. Bug fix — GTK clipboard copy: `hover_selection_text()` accessor + GTK-side intercept using `copypasta` directly. 7 new tests.

**Session 211 — Code Action Apply + Semantic Token Fix (4703 tests):**
Selectable code action dialog: replaced flat OK dialog with vertical button list; j/k/Up/Down navigate, Enter applies; `vertical_buttons` flag on `DialogPanel` renders vertical layout in both GTK and TUI. Code action workspace edit: `CodeAction.edit: Option<WorkspaceEdit>` parsed from LSP response; `process_dialog_result("code_actions")` calls `apply_workspace_edit()` to apply the selected action. Proactive code action request: `flush_cursor_move_hook()` fires `lsp_request_code_actions_for_line()` after 150ms debounce; lightbulb appears when cursor settles; cache cleared per-line before each request. Always-fresh requests: `show_code_actions_popup()` sends fresh LSP request at exact cursor position. Semantic token fix after edits: `apply_lsp_edits()` clears `semantic_tokens` immediately and marks buffer LSP-dirty.

**Session 210 — Code Action Gutter Indicator (4703 tests):**
LSP `textDocument/codeAction` protocol: `CodeAction` struct, `request_code_action()` with zero-width range at cursor position, `CodeActionResponse` event, `codeAction` capability in init_params. Engine: `lsp_code_actions` cache (HashMap<PathBuf, HashMap<usize, Vec<CodeAction>>>), cache cleared on didChange, `has_code_actions_on_line()`, `show_code_actions_popup()`. Keybindings: `<leader>ca` / `:CodeAction` / palette entry "LSP: Code Action". Gutter: lightbulb icon (`\u{f0eb}`) in GTK and TUI; yellow `lightbulb` theme color on all 6 themes; diagnostics take priority. Gutter click triggers popup. Debounced cursor_move hook: `cursor_move_pending: Option<Instant>` with 150ms delay, `flush_cursor_move_hook()` called from backend idle loops (fixes CPU 100% from synchronous git blame on every keystroke). `"ca"` added to leader key SEQUENCES. 6 new tests.

**Session 209 — TUI Tab Drag-and-Drop (4671 tests):**
TUI tab drag between editor groups: mouse-drag tabs from one group's tab bar to another to move buffers; drag to editor content area edge zones creates new splits (vertical/horizontal); drag within same group's tab bar reorders tabs. `compute_tui_tab_drop_zone()` — cell-based hit-testing for tab bar (midpoint-based insertion index), content area (20% edge zones → split, center → merge). `render_tab_drag_overlay()` — blue highlight on target zone, `▎` insertion bar for reorder, ghost label near cursor. Reuses existing `TabDragState`/`DropZone`/`tab_drag_begin()`/`tab_drag_drop()` API. 7 new tests.

**Session 208 — Bug Fixes: Extension Update Key + Flatpak Build (4664 tests):**
Extension "u" update key fix: `ext_sidebar_selected` flat index was compared directly against `installed.len()` without accounting for collapsed sections; added `ext_selected_to_section()` helper. Flatpak CI build fix: regenerated `cargo-sources.json` (stale tree-sitter 0.24→0.26). Added cargo-sources regeneration to CLAUDE.md release checklist.

**Session 207 — Bug Fixes + VS Code Light Theme (4664 tests):**
TUI mouse drag capture fix: `dragging_generic_sb` cleared on MouseUp. GTK ext panel scrollbar drag fix: `connect_drag_begin` claims event sequence with `set_state(Claimed)`. Tab hover tooltip: hovering over a tab shows full path with `~` shortening (GTK Cairo popup below tab bar, TUI overlay on breadcrumbs row); fixed `tab_close_hit_test` Y-coordinate bug with breadcrumbs. Double hover popup fix: mutual exclusion between panel hover and editor hover via `dismiss_panel_hover_now()`/`dismiss_editor_hover()`. VS Code Light theme: new `vscode-light` (`light+`) built-in colorscheme; `Theme::is_light()` perceptual luminance check; fixed `Color::from_hex("#ddd")` crash → `#dddddd`; theme-aware rendering across all GTK UI elements (activity bar, menus, title bar, window controls, all 5 sidebar panels, settings widgets); `prefer_dark_theme` GTK4 setting toggled on theme change; TUI activity bar dark gray icons for light themes.

**Session 206 — Git Log Panel Bug Fixes + Release v0.4.0 (4664 tests):**
GTK hover popup link clicking: Pango `index_to_pos` computes link pixel rects; `editor_hover_link_rects` field. Panel reveal fixes: GTK sets `active_panel` directly (no sidebar toggle); TUI clears expanded tree state before reveal; reveal scrolls to center. Ext panel scroll/scrollbar: `EventControllerScroll` + scrollbar click/drag for GTK; TUI scroll + scrollbar support. Git hash consistency: `--format=%H %s` for full hashes; `git_log_commit()` for single-commit lookup; `refresh_all()` appends missing commits for reveal. Lua reveal target timing: `_git_log_reveal_target` cleared only after use by `refresh_all()`. Version 0.4.0.

**Session 205 — Enhanced Git Log Panel + Blame-to-Panel Navigation (4654 tests):**
Expandable commits in Git Log panel: tree nodes with file children on Tab expand/collapse; hover content (author/date/message/stat). Side-by-side diff from log: `open_commit_file_diff()` engine method. Blame-to-panel navigation: `GitShow`/`:Gshow` uses `panel.reveal()` instead of scratch buffer. Git log action keys: `o`=diff, `y`=copy, `b`=browser, `r`=refresh, `d`=pop stash, `p`=push stash, `/`=search. New git.rs functions: `commit_files()`, `diff_file_at_commit()`, `show_commit_file()`. New Lua bindings: `commit_files`, `diff_file`, `show_file`, `commit_detail`, `open_diff`, `panel.reveal`.

**Session 204 — Command URI Dispatch for Extensions (4654 tests):**
`command:Name?args` links in hover popup markdown dispatch to plugin commands via `execute_command_uri()`; `percent_decode()` helper; `execute_hover_goto()` fallback routes to plugins; GTK/TUI panel hover click `command:` routing. git-insights blame.lua: "Open Commit" and "Copy Hash" action links. 5 new tests.

**Session 203 — VSCode Mode Git Insights + Hover Popup Fixes (4649 tests):**
VSCode edit mode git insights: `fire_cursor_move_hook()` added to all exit paths in `handle_vscode_key()` (main return, Ctrl+K chord, Alt-key, user keymap) so blame.lua receives `cursor_move` events; `render.rs` annotation rendering gate changed from `Mode::Insert` suppression to `Mode::Insert && !is_vscode_mode()` (two locations in `build_rendered_window`); hover dwell mode gates in both GTK (`main.rs` ~line 6479) and TUI (`tui_main.rs`) updated to include `|| engine.is_vscode_mode()`. GTK hover popup word wrapping: replaced fixed-width text overflow with Pango word wrapping — `layout.set_width(pango_text_w)` + `WrapMode::WordChar`; pre-computed wrapped heights per line; pixel-based height cap; Cairo save/clip/restore for bounds; `draw_editor_hover_popup` returns `Option<(f64,f64,f64,f64)>` popup rect. Stale LSP hover fix: `lsp_hover_text` cleared in `dismiss_editor_hover()` and inline dismiss path in `editor_hover_mouse_move()` — prevents cached hover from re-appearing on every click. GTK hover popup click-to-focus: `editor_hover_popup_rect: Rc<Cell<Option<(f64,f64,f64,f64)>>>` field caches popup bounds from draw; click handler checks bounds — clicks on popup set `editor_hover_has_focus=true` and are consumed; clicks outside dismiss. Fixed 20Hz `SearchPollTick` dismiss race: skip `editor_hover_mouse_move()` call when mouse position is within popup bounds, preventing continuous dismiss/redraw cycle that made popups unclickable. Hover dismiss on mouse-off: skipping the motion call (rather than passing a flag) correctly allows popups to dismiss when mouse leaves popup area while keeping them alive when mouse is over them. No new tests (behavioral fixes in GTK/TUI backends).

**Session 202 — Panel Event Enhancements (4636 tests):**
`panel_double_click` event: fires on double-click of extension panel items in both GTK (button-1 n_press>=2) and TUI (400ms double-click detection) backends; `handle_ext_panel_double_click()` resolves flat index to section/item/id, fires event before `panel_select`. `panel_context_menu` event: fires on right-click; GTK: new `ExtPanelRightClick(f64, f64)` message + button-3 `GestureClick` on `ext_dyn_panel_da`; TUI: `MouseButton::Right` guard in ext panel click handler; `open_ext_panel_context_menu(x, y)` resolves item and fires event; `ContextMenuTarget::ExtPanel { panel_name, item_id }` variant added; `context_menu_confirm()` fires `panel_context_menu` event for ExtPanel targets. `panel_input` event + per-panel input field: `/` key in `handle_ext_panel_key()` activates input; `ext_panel_input_active: bool` + `ext_panel_input_text: HashMap<String, String>` engine fields; `handle_ext_panel_input_key()` handles typing/backspace/escape/return; fires `panel_input` event on every keystroke (live filtering) and on Return (confirm); input interception in `handle_key()` before normal panel key handling. Lua API: `vimcode.panel.get_input(name)` reads from `panel_input_snapshot` (engine state snapshot), `vimcode.panel.set_input(name, text)` writes via `panel_input_values` output on `PluginCallContext`; `apply_plugin_ctx()` applies input values to `ext_panel_input_text`. Render layer: `ExtPanelData.input_text/input_active` fields for backends. 10 new tests in `tests/ext_panel.rs`.

**Session 201 — Hover Popup Enhancements (4626 tests):**
Tree-sitter syntax highlighting for fenced code blocks in hover popups and markdown previews: `MdCodeHighlight` struct with byte offsets and scope names; `SyntaxLanguage::from_name()` maps markdown fence language tags to tree-sitter languages; raw code text accumulated during markdown parsing, tree-sitter run at code block end, highlights mapped back to per-line spans with 4-space indent offset. TUI editor hover click-to-copy: `render_editor_hover_popup` returns link rects plumbed through `draw_frame` and `handle_mouse`; click on `command:` URLs calls `execute_hover_goto`, regular URLs call `tui_copy_to_clipboard`. VSCode-style "Go to" navigation links at bottom of editor hover popups: "Go to" prefix in default fg, labels (Definition/Type Definition/Implementations/References) in link color and clickable; keybinds shown as `(:gd)` etc.; gated on LSP server capabilities via `lsp_manager.server_supports()`; `command:` URL scheme for internal navigation; `execute_hover_goto()` saves hover anchor position, dismisses popup, moves cursor, fires LSP request; only shown after LSP content received (not during "Loading..."); vim mode only (hidden in VSCode mode). LSP semantic token reliability fixes: don't send `SemanticTokensResponse` on error/null results (prevents wiping existing tokens); don't overwrite tokens when legend cache misses (changed `unwrap_or_default()` to `if let Some`); removed aggressive `semantic_tokens.clear()` in `lsp_flush_changes()` (old tokens remain until new ones arrive). 12 new tests.

**Session 200 — Rich Panel Layout API + Editor Hover Popups (4614 tests):**
Rich Panel Layout API: Extended `ExtPanelItem` with tree items (expandable/collapsed via `parent_id`), action buttons (clickable badges on items), badges/tags, and separator items for extension panels. Editor Hover Popups: `gh` key triggers hover popup aggregating diagnostics, plugin content, annotations, and LSP hover at cursor position; keyboard navigation with Tab (cycle links), Enter (open), j/k (scroll), Escape (dismiss). GTK + TUI backends fully wired up for rich panel rendering and editor hover popups. 14 new tests in `tests/ext_panel.rs`.

**Session 199 — TUI clipboard fix + hover popup UX polish (4580 tests):**
Fixed TUI clipboard regression: replaced copypasta_ext's `x11_bin::ClipboardContext` with direct xclip/xsel/wl-copy/wl-paste subprocess calls. Root cause: copypasta_ext's `sys_cmd_set` doesn't close the `ChildStdin` pipe before calling `wait()`, so xclip never receives EOF — exits with status 1 under crossterm raw mode. Fix: `child.stdin.take()` drops the handle (sends EOF) before `child.wait()`. Also added `DISPLAY=:0` fallback in TUI `setup_tui_clipboard()` matching GTK backend — fixes "Can't open display: (null)" in tmux/SSH sessions. Hover popup delayed dismiss system: engine field `panel_hover_dismiss_at: Option<Instant>` with 350ms grace period; `dismiss_panel_hover()` schedules delayed dismiss, `dismiss_panel_hover_now()` for immediate dismiss (keypresses/clicks), `cancel_panel_hover_dismiss()` when mouse returns to popup area. GTK popup stability: overlay DA stays `can_target(false)` always — previous approach of toggling `can_target(true)` when popup visible caused leave/enter cycles on the SC panel DrawingArea (blinking). Instead: capture-phase `GestureClick` on `window_overlay` intercepts clicks within popup bounds; capture-phase `EventControllerMotion` on `window_overlay` cancels dismiss when mouse is within popup rect. `panel_hover_popup_rect: Rc<Cell<Option<(f64,f64,f64,f64)>>>` stores popup bounds from draw func. `draw_panel_hover_popup` return type changed to `(Vec<link_rects>, Option<popup_bounds>)`. TUI popup: `hover_popup_rect` and `hover_link_rects` params on `handle_mouse()`; 1-col buffer zone on popup hit area; `mouse_on_hover_popup` guard on all dismiss calls. TUI link click copies URL via `tui_copy_to_clipboard` → engine clipboard callback. Status bar text selection via mouse drag + Ctrl+C in any mode. `panel_hover_mouse_move` updated: same-item dwell cancels dismiss only if it owns the current popup; different-item schedules delayed dismiss instead of immediate clear.

**Session 198 — Extension panel system + hover popups (4580 tests):**
GTK ext panel rendering (`draw_ext_dyn_panel`), hover popups with rendered markdown (both GTK + TUI), native SC panel hovers (branch info, file diff stats, commit details), Lua `vimcode.panel.set_hover()` API, `commit_detail()`/`diff_stat_file()`/`tracking_branch()` git helpers.

**Session 197 — SC panel click targeting fix + async diff open (4556 tests):**
Fixed GTK Source Control panel click targeting: replaced uniform-division row calculation with accumulator walk matching draw code's mixed heights (section headers at `line_height`, items at `item_height = 1.4×`). Diff tabs now open in new tabs instead of replacing current buffer. Added `diff_label: Option<String>` field to `BufferState` for bracket-free tab titles — working copy shows `filename (Working Tree)`, HEAD shows `[filename (HEAD)]`; `clear_diff_labels()` helper cleans up on all diff teardown paths. Async diff flow: SC click sets `sc_selected` and returns immediately (sidebar repaints highlight); `sc_open_selected_async()` opens the tab with the file (tab appears instantly), then spawns `git show HEAD:file` on a background thread via `mpsc::channel`; `poll_sc_diff()` picks up the result and calls `sc_apply_diff_split()` to add the HEAD pane + compute diff. GTK uses `idle_add_local_once` to defer `sc_open_selected_async` after the sidebar repaint. TUI replicates the same async pattern: keyboard Enter and mouse click both call `sc_open_selected_async()`, `poll_sc_diff()` added to TUI polling loop. Engine fields: `sc_diff_rx`, `sc_diff_pending_win`.

**Session 196 — Git commit input editing + SC panel button polish (4556 tests):**
Cursor-based commit message editing: `sc_commit_cursor: usize` field tracks byte position; Left/Right/Up/Down/Home/End/Delete arrow keys; Ctrl+V clipboard paste (xclip/xsel/wl-paste/pbpaste); Ctrl+A/E jump to start/end; cursor-position-aware insert/delete/backspace. Fixed indentation mismatch on continuation lines (was 6 spaces, now 4 to match icon prefix). GTK beam cursor (1.5px vertical line) instead of `|` character; TUI block cursor via fg/bg color inversion. Fixed Left/Right/Home/End/Delete keys not reaching commit input handler — all three key routing locations (TUI + two GTK handlers) were missing explicit mappings for these keys. SC panel button styling: padding above/below button row (TUI: 1 empty row each side; GTK: 0.3× line_height gaps); buttons use contrasting `status_bg` background with horizontal margins; text uses regular foreground color (was dim grey/header blue). Mouse hover on buttons: GTK `EventControllerMotion` + `ScSidebarMotion` message; TUI `MouseEventKind::Moved` handler; both lighten button bg on hover (+0.08 RGB in GTK, +20 RGB in TUI). Fixed SC click handler row offsets broken by padding changes — TUI commit input range was too wide (included padding row), section offset miscalculated; GTK switched from integer row_idx to pixel-based hit testing matching actual draw geometry. Fixed clicking files inserting newline into commit message — `sc_commit_input_active` was not deactivated when clicking outside commit area, so `handle_sc_key("Return")` was intercepted by commit handler. 4 new cursor tests.

**Session 195 — OS-specific install commands + Git panel polish (4544 tests):**
Backfilled `install_linux`/`install_macos`/`install_windows` on 5 extensions: cpp, bicep, terraform, xml, latex. Multi-line commit messages (Enter→newline, Ctrl+Enter→commit, box grows). GTK SC panel 1.4× item spacing. SSH passphrase dialog: `run_git_remote()` uses `SSH_ASKPASS`+`SSH_ASKPASS_REQUIRE=force`+`Stdio::null()` to prevent TTY prompt leaks; auth failures show modal dialog with text input. `DialogInput` struct + `DialogInputPanel` for text input in dialogs. 5 new tests.

**Session 194 — Extension panel UX fixes (4529 tests):**
Extension removal confirmation dialog; dialog wrap-around navigation; terminal focus bug fix; GTK sidebar keyboard routing fix (keys routed via engine focus flags in `Msg::KeyPress`); extension search fix; double-click opens README; tab name shows display_name; GTK extension panel spacing; LaTeX README. 14 updated tests, 3 new.

**Session 193 — Tree-sitter upgrade + Lua/Markdown syntax (4515 tests):**
Tree-sitter 0.24→0.26: Bumped core + 8 grammar crates; `node.child()` now takes `u32`. Added `tree-sitter-lua` 0.5 (functions, strings, comments, numbers, keywords) and `tree-sitter-md` 0.5 (headings, code blocks, thematic breaks). Markdown spell-check treats `.md` files as prose (check all words, not just comments/strings). 20 tree-sitter languages total (was 18). 4 new tests.

**Session 192 — Bug fixes (4511 tests):**
Save message shows relative path (`:w` displays relative instead of absolute). Status line shows filename only. Unrecognized file types no longer get Rust highlighting (`BufferState.syntax` changed from `Syntax` to `Option<Syntax>`). Deleted leftover `homebrew-vimcode/` directory.

**Session 191 — Subprocess stderr safety audit (4511 tests):**
Audited all ~50 `Command::new()` call sites across the codebase. Only one unsafe site found: `registry.rs` `download_script()` used `.status()` with inherited stderr — could corrupt TUI display during extension downloads. Fixed by adding `.stdout(Stdio::null()).stderr(Stdio::null())` to the curl call. All other sites already safe: `.output()` auto-captures both streams; `.spawn()` calls all have explicit `Stdio` redirection. Breakdown: `git.rs` (~20 calls, all `.output()`), `engine.rs` (~8 calls, all redirected or `.output()`), `ai.rs` (3 curl `.output()`), `dap.rs`/`lsp.rs` (`.spawn()` with explicit piped/null), `dap_manager.rs`/`lsp_manager.rs` (all `.output()`).

**Session 190 — LSP Go-to-Definition Fix + Kitty Keyboard Fix (4511 tests):**
Editor right-click context menu expanded to 9 items with mode-aware shortcuts (Vim vs VSCode keybindings displayed per `editor_mode` setting). Kitty terminal ":" not working fix: `shift_map_us()` function in TUI translates base key + SHIFT modifier to correct shifted character when `REPORT_ALL_KEYS_AS_ESCAPE_CODES` keyboard enhancement is active. **LSP `gd` "hanging" fix**: Definition response was arriving and being processed correctly, but `self.message` ("Jumping to definition...") was never cleared after a successful jump — making it appear stuck. Added `self.message.clear()` to `DefinitionResponse`, `ImplementationResponse`, and `TypeDefinitionResponse` handlers. LSP debug logging infrastructure added to `send_request()` and reader thread (gated behind `VIMCODE_LSP_DEBUG` env var); also added `unwrap_or()` fallback for definition/hover responses and string ID fallback for response parsing (some servers echo IDs as strings). Debug logging removed after diagnosis.

**Session 189 — VSCode-style Editor Context Menu (4510 tests):**
Full 9-item editor right-click context menu: Go to Definition, Go to References, Rename Symbol, Open Changes, Cut, Copy, Paste, Open to the Side, Command Palette. LSP items disabled without active server; Cut/Copy disabled without visual selection. Engine-driven for both GTK and TUI. Made 5 engine methods `pub`: `yank_visual_selection`, `delete_visual_selection`, `paste_after`, `lsp_request_definition`, `lsp_request_references`. 8 new tests.

**Session 188 — Centralize Context Menus + Editor Right-Click (4501 tests):**
GTK context menus now driven by engine items: `build_gio_menu_from_engine_items()` helper builds `gio::Menu` with sections from `ContextMenuItem.separator_after` boundaries. Both explorer and tab context menus call `engine.open_*_context_menu()`, clone items, clear engine state, build `gio::Menu`. Tab action enabled/disabled state from `ContextMenuItem.enabled` via `enabled_map` HashMap (replaces hardcoded `tab_count`/`is_last`/`has_file` conditions). Explorer actions use `add_action()` helper for engine-driven enabled state. Fixed `copy_rel` → `copy_relative_path` action name mismatch. New `ContextMenuTarget::Editor` variant + `open_editor_context_menu()` with "Open to the Side (vsplit)" item (disabled when no file). `context_menu_confirm()` handles `open_side_vsplit` via `split_window()` + `open_file_with_mode()`. GTK: `Msg::EditorRightClick` + `ClickTarget::BufferPos`/`Gutter` in right-click handler → PopoverMenu. TUI: right-click on editor area calls `open_editor_context_menu()`. Explorer file menu also gets "Open to the Side (vsplit)" (Vim-style split, not editor group). 4 new tests.

**Session 187 — Tab Context Menu Splits Fix (4498 tests):**
Fixed GTK/TUI split inconsistency: GTK tab context menu "Split Right"/"Split Down" was calling `open_editor_group()` (new editor groups) while engine's `context_menu_confirm()` called `split_window()` (Vim window splits). Fixed GTK to call `split_window()` matching the engine. Added 4 split options to tab context menu: "Split Right" and "Split Down" (Vim window splits within current tab); "Split Right to New Group" and "Split Down to New Group" (new editor groups, VSCode-style). README 3-layer explainer (Windows/Tabs/Editor Groups). 4 new tests.

**Session 186 — Drag-and-Drop File Move + Context Menu Clamping (4468 tests):**
Drag-and-drop file/folder move in both TUI (mouse Down→Drag→Up flow with `explorer_drag_src`/`explorer_drag_active`/`explorer_drop_target` state) and GTK (`DragSource`/`DropTarget` API with `Msg::MoveFile`). Engine-driven Yes/No confirmation dialog (`confirm_move_file()`/`pending_move`/`process_dialog_result`). Clickable dialog buttons: TUI computes positions from rendered layout; GTK uses `DialogBtnRects` (`Rc<RefCell<...>>`) shared between draw closure and click handler with actual Pango-measured button rects. Fixed GTK dialog "No" button activating "Yes" (proportional vs monospace font mismatch). Fixed GTK crash on drag (removed legacy GTK3 DnD handlers). Subtree move prevention + same-directory no-op. Context menu popup clamping: moved popup rendering after status/command line in TUI draw order. 6 new tests.

**Session 185 — Context Menu Action Polish + Bug Fixes (4468 tests):**
Select for Compare / Compare with Selected two-step file comparison flow (engine `diff_selected_file`). Fixed GTK `copy_relative_path` (was sending absolute path — added `Msg::CopyRelativePath`). Fixed GTK `open_side` (was opening in current group — added `Msg::OpenSide`). Fixed "Open to Side" creating 2 tab groups (`open_editor_group()` clone + `open_file_in_tab()` double-add — fixed to use `:e` replacement). Fixed swap file "Abort" not deleting swap (added `delete_swap()` to abort path). Fixed xdg-open stderr corrupting TUI (redirect stdout/stderr to `/dev/null`). Fixed "Open in Integrated Terminal" (TUI: `terminal_new_tab_at()`; GTK: added `Msg::OpenTerminalAt(dir)` + `ctx.open_terminal` GIO action). Deduplicated TUI action handlers (engine-only for `copy_path`, `copy_relative_path`, `reveal`, `open_side`). 8 new tests.

**Session 184 — Right-Click Context Menus (4460 tests):**
Explorer right-click context menu with different menus for files vs folders (matching VSCode). Tab bar right-click context menu (Close, Close Others, Close to Right, Close Saved, Copy Path, Copy Relative Path, Reveal, Split Right/Down). `ContextMenuState`/`ContextMenuTarget` data model in engine. TUI: `render_context_menu_popup()` with box-drawing borders, mouse hover highlighting. GTK: `PopoverMenu` with `gio::Menu` sections + `SimpleActionGroup` actions; `swap_ctx_popover()` lifecycle; GLib log handler for non-fatal GTK4 assertion. `render.rs`: `ContextMenuPanel`/`ContextMenuRenderItem`. `tests/context_menu.rs` integration test file. 38 new tests.

**Session 183 — Vim Compatibility Gap Closure (4422 tests):**
`[#`/`]#` preprocessor navigation (C/C++ `#if`/`#endif` bracket matching), `gR` virtual replace mode (fixed-width character replacement preserving column alignment), `g+`/`g-` timeline undo (chronological undo tree traversal), `q:`/`q?` command-line history window (scrollable popup with Enter to execute, editable entries). VIM_COMPATIBILITY 412→414/417 (99%). 31 new tests.

**Session 182 — LaTeX Extension: Parts A + C (4391 tests):**
LaTeX text objects (`ie`/`ae` environment, `ic`/`ac` command, `i$`/`a$` math) and motions (`]]`/`[[` section jumps, `]m`/`[m`/`]M`/`[M` environment jumps, `][`/`[]` for `\end{}`). Registry extension in `vimcode-ext` repo — `latex/manifest.toml` with texlab LSP, `latex/latex.lua` with vimtex-inspired keymaps. Bug fixes: `vcd <dir>` opens folder as workspace, TUI folder picker mouse clicks. 22 new tests.

**Session 181 — LaTeX Extension: Tree-sitter Syntax + Spell Checking (4331 tests):**
Tree-sitter LaTeX support (18th language); vendored `tree-sitter-latex` v0.3.0 grammar; LaTeX-aware spell checking (`check_line()` API updated from `bool` to `Option<SyntaxLanguage>`). 15 new tests.

**Session 180b — Spell Checker Bug Fixes + UI Polish (4316 tests):**
z= suggestions: numbered list UI with single-key selection (1-9, a-z); `spell_suggestions` state intercepts keys at top of `handle_key()`. Markdown spell checking: fixed `has_syntax` detection to use `SyntaxLanguage::from_path()` instead of `!highlights.is_empty()`. Undo/dirty tracking for spell replacements. GTK scrollbar width halved (10→5px). Text overflow behind scrollbar fixed. Group divider grab bounds fixed.

**Session 180 — Spell Checker (4314 tests):**
New `src/core/spell.rs` (~200 lines): spell checking via `spellbook` 0.4 (pure-Rust Hunspell parser). Bundled `dictionaries/en_US.aff` + `en_US.dic` compiled into binary via `include_bytes!`. User dictionary at `~/.config/vimcode/user.dic`. Tree-sitter aware: only checks comments/strings in code files; all text in plain-text/Markdown files. New settings: `spell` (bool, default false), `spelllang` (string, default "en_US"). `:set spell` / `:set nospell` to toggle; "Toggle Spell Check" in command palette. Vim keybindings: `]s`/`[s` (next/prev error), `z=` (suggestions), `zg` (add to dict), `zw` (mark wrong). Visual: cyan dotted underline in GTK (wavy dots), colored underline in TUI. `SpellError` struct with char-based columns. `check_line()` with syntax-aware filtering. 60 new tests: 11 in spell.rs + 6 in engine.rs.

**Session 179 — Resize Tab Groups (4266 tests):**
Added keyboard and mouse resize for editor group splits. GTK: Alt+,/Alt+. keyboard shortcuts (shrink/expand active group by 0.05 ratio). TUI: mouse drag on group dividers — `dragging_group_divider: Option<usize>` state, hit-testing on `GroupDivider` positions from `ScreenLayout`, drag handler computes new ratio from mouse position via `set_ratio_at_index()`. Alt+,/Alt+. already existed in TUI. GTK drag already existed. Both backends now have full keyboard + mouse group resize.

**Session 178 — Version Querying (4266 tests):**
Added `--version` / `-V` CLI flag to both `vimcode` and `vcd` binaries — prints `VimCode <version>` and exits before UI initialization. Updated Help > About menu action to show version in a modal dialog (using the dialog system instead of message bar). Version sourced from `Cargo.toml` via `env!("CARGO_PKG_VERSION")`.

**Session 177 — Fix Wrap Mode Mouse Click (4266 tests):**
Fixed mouse click targeting on wrapped lines in both GTK and TUI backends. GTK: added `view_row_to_buf_pos_wrap()` in `main.rs` that walks buffer lines from scroll_top using `compute_word_wrap_segments()` (same word-wrap algorithm as renderer) to correctly map visual rows to `(buffer_line, segment_col_offset)`; `pixel_to_click_target()` now calls this when `settings.wrap` is true instead of the wrap-unaware `view_row_to_buf_line()`; column calculation walks from `segment_col_offset` instead of 0. TUI: click handler and drag handler in `tui_main.rs` now read `segment_col_offset` from the rendered line and add it to `col_in_text`. Previously, clicking on a wrap-continuation row mapped to the wrong buffer line (GTK used 1:1 visual-row-to-buffer-line mapping ignoring wraps) or wrong column (TUI didn't account for segment offset). Cleared BUGS.md.

**Session 176 — GTK Performance: Lazy Tree + Open Folder Fix (4266 tests):**
GTK explorer tree lazy loading: replaced eager recursive `build_file_tree()` with `build_file_tree_shallow()` that populates one directory level at a time with dummy placeholder children (`TREE_DUMMY_PATH`); `tree_row_expanded()` replaces dummies with real children on demand via `row-expanded` signal. Fixes multi-second startup when opening in large directories (e.g., home directory). Open Folder fix: `open_folder()` now calls `std::env::set_current_dir(&canonical)` to update process working directory alongside `engine.cwd`; `RefreshFileTree` handler uses `engine.cwd` instead of `std::env::current_dir()`, so tree repopulates with the new folder as root. `highlight_file_in_tree` rewritten to walk path components, expanding ancestors lazily. Removed `find_tree_path_for_file` (no longer needed).

**Session 175 — Diff View Improvements: Click Handling, Fold-Aware Scrolling, Aligned Folds (4263 tests):**
Per-group diff toolbar click handling: GTK `DiffBtnMap`/`SplitBtnMap` HashMap types replacing single shared `DiffBtnPositions` cache; `draw_tab_bar` returns `TabBarDrawResult` tuple; `draw_editor` clears maps per frame; `pixel_to_click_target` checks diff toolbar FIRST then split buttons. TUI `was_active` tracking in multi-group click handler; `had_split = was_active || engine.is_in_diff_view()`. Split buttons shown on all groups in diff mode (`show_split = is_active || engine.is_in_diff_view()`). Fold-aware scrolling: `View::next_visible_line(from, count, max_line)` / `View::prev_visible_line(from, count)` skip fold bodies; Ctrl-D/U/F/B (normal + visual), Ctrl-E/Y, and scroll wheel (TUI `scroll_down_visible_for_window` + GTK `scroll_down_visible`) all fold-aware. `Engine::scroll_down_visible(count)` / `scroll_up_visible(count)` + per-window variants. Aligned-sequence fold computation: `diff_apply_folds()` rewritten to use `diff_aligned` (visual row → buffer line mapping via `AlignedDiffEntry`) instead of raw `diff_results`; builds per-visual-row `changed` flag from both sides, marks context, translates back to per-buffer `buf_visible` array, creates independent fold regions per window. Fixes trailing unchanged lines showing on shorter buffer side. `sc_has_focus` cleared in `cmd_git_diff()` / `cmd_git_diff_split()`. TUI diff toolbar glyphs reverted to Nerd Font (`\u{F0143}`/`\u{F0140}`/`\u{F0233}` via `set_cell_wide`) with `DIFF_BTN_COLS = 3`. 3 new tests (fold-aware Ctrl-D, Ctrl-U, scroll_down_visible).

**Session 174 — Bug Fixes: Dialog System, Completion, Diff, Find Panel (4254 tests):**
Dismissable modal dialog system: `Dialog`/`DialogButton` structs with `show_dialog()`/`show_error_dialog()`/`handle_dialog_key()`/`process_dialog_result()` in engine.rs; `DialogPanel`/`format_button_label()` in render.rs; `render_dialog_popup()` in TUI; `draw_dialog_popup()` in GTK. Swap recovery migrated from status-bar messages to modal dialog (`pending_swap_recovery` field, `process_swap_dialog_action()`). Stderr suppression: RAII `StderrGuard` wrapping `build_clipboard_ctx()` in TUI to prevent "Can't open display" noise. Removed 6 `eprintln!` calls from `swap.rs`/`settings.rs`/`lsp_manager.rs`. Fixed sticky completion popup: `dismiss_completion()` helper clears candidates + cancels `lsp_pending_completion`; CompletionResponse handler checks Insert mode; safety dismiss in `handle_key()` for non-Insert modes. Fixed diff view padding: skip padding entries when `diff_unchanged_hidden` in `build_rendered_window()`. Fixed diff view on large files: removed `MAX_LINES: 5000` guard in `lcs_diff()` that prevented Myers diff on files >5000 lines. Fixed GTK Find Panel: capture-phase key handler detects `Entry`/`Text` widget focus and returns `Propagation::Proceed`. Fixed Visual Mode Ctrl-D/U: added `!ctrl` guards on `'d'`/`'u'` match arms. Fixed undo/redo not notifying LSP: `undo()`/`redo()` now insert into `lsp_dirty_buffers`. Verified diff toolbar populates on both group tab bars. 40 new tests.

**Session 173 — Diff View Fixes: Aligned Scroll Sync + Auto-Filter (4214 tests):**
Aligned-position-aware scroll sync for diff windows: `sync_scroll_binds()` now maps scroll positions through `diff_aligned` sequences instead of copying raw buffer line numbers, so both sides stay in visual lockstep even when one side has large padding blocks. Auto-enable `diff_unchanged_hidden` + `diff_apply_folds()` in `cmd_diffthis()`, `cmd_diffsplit()`, and `cmd_git_diff_split()` so the filter is active by default when entering diff mode. `is_in_diff_view()` now checks all editor groups (not just the active one) for diff window presence. Render fix: `build_rendered_window` advances `aligned_idx` past hidden (folded) lines and their adjacent padding entries. Known remaining issues logged in BUGS.md: large padding blocks still not fully suppressed in filtered view, toolbar not always appearing on both group tab bars. 1 new test.

**Session 172 — VSCode-Style Diff Toolbar + Unified Hunk Navigation (4193 tests):**
Diff toolbar in tab bar with prev/next change buttons, toggle hide-unchanged button, and "N of M" change label. `]c`/`[c` now use `diff_results` when in two-window diff mode (falls back to git_diff markers or `@@` headers otherwise). Fold-based hiding of unchanged sections with 3-line context around changes. `:DiffNext`/`:DiffPrev`/`:DiffToggleContext` ex commands + palette entries. `zR` auto-disables hidden mode. Cleanup in `cmd_diffoff()` and `close_window()`. Both GTK and TUI backends render toolbar and handle clicks. 6 new tests.

**Session 171 — VSCode-Style Side-by-Side Diff Editor (4160 tests):**
`:Gdiffsplit` / `:Gds` opens HEAD (read-only) on left and working copy (editable) on right with LCS diff coloring (green=added, red=removed), scroll-bound. SC panel Enter now opens diff split for tracked changed files (untracked/new files open normally). Diff recomputes on save and after hunk stage/revert via `gD`. Diff state cleaned up on window/tab close. `git::show_file_at_ref()` retrieves file content at any git revision. "Git: Diff Split" in command palette. 7 new tests.

**Session 170 — Inline Diff Peek + Enhanced Hunk Nav (4105 tests):**
Inline diff preview (VSCode parity): `gD` / `:DiffPeek` / click gutter marker opens floating popup showing hunk diff lines (red=removed, green=added) with `[r] Revert` / `[s] Stage` actions. Deleted-line gutter indicator (`▾` in red) for pure deletions. `]c`/`[c` now navigate changed regions on real source files using `git_diff` markers (previously only worked in diff buffers). New `DiffPeekState`/`DiffPeekPopup` structs, `DiffHunkInfo` with line-range mapping, `compute_file_diff_hunks()`, `hunk_for_line()`, `revert_hunk()` in git.rs. `git_deleted` color added to all 4 themes. Both GTK and TUI backends render popup + detect git gutter clicks. "Git: Peek Change" in command palette. 17 new tests.

**Session 169 — GitHub Wiki (4088 tests):**
Created the VimCode GitHub Wiki with 9 pages: Home, Getting Started, Key Remapping, Settings Reference, Extension Development, Lua Plugin API, Theme Customization, DAP Debugger Setup, LSP Configuration. Added Documentation section with wiki links to README.md. Updated extension guide link to point to wiki.

**Session 168 — Keybinding Discoverability + VSCode Remapping (4088 tests):**
Made keybinding remapping discoverable and enabled it in VSCode mode. Added 7 new ex command aliases (`:hover`, `:LspImpl`, `:LspTypedef`, `:nextdiag`, `:prevdiag`, `:nexthunk`, `:prevhunk`) so every remappable keybinding has a named command. Updated `:Keybindings` reference (both Vim and VSCode) to show command names alongside bindings (e.g., `gd → :def`, `F12 → :def`, `Ctrl+P → :fuzzy`) with a remapping hint. Added 12 commands to `available_commands()` for tab completion. Enabled `:map` remapping in VSCode mode — `handle_vscode_key()` now checks `try_user_keymap()` before built-in handlers; mode `"n"` keymaps apply. Added "Open Keyboard Shortcuts" to command palette so VSCode users can F1 → remap keys. Updated `:Keymaps` help text to mention VSCode mode. Fixed pre-existing test hermiticity bug: `engine_with()` now resets `mode` to Normal and rebuilds `user_keymaps` (was leaking disk settings into tests). 17 new tests in `tests/wincmd.rs` (40 total). README updated with discoverability instructions.

**Session 167 — :wincmd Ex Command (4071 tests):**
Added `:wincmd {char} [count]` ex command (abbreviation `:winc`) that executes any window command programmatically (e.g., `:wincmd h` is equivalent to `Ctrl-W h`). Refactored the `Ctrl-W` handler to delegate to a shared `execute_wincmd()` method, eliminating code duplication between the key handler and the new ex command. Updated `:Keybindings` reference to show command names alongside keybindings. Added `:close`, `:only`, `:new`, `:wincmd` to tab completion. 23 integration tests in `tests/wincmd.rs`.

**Session 166 — Extension Registry Decoupling (4053 tests):**
Fully decoupled extensions from compiled-in data. Removed `BUNDLED` static array and all `include_str!()` from `extensions.rs`. Extensions now fetched from remote GitHub registry ([vimcode-ext](https://github.com/JDonaghy/vimcode-ext)) and cached locally at `~/.config/vimcode/registry_cache.json`. `ext_available_manifests()` merges registry + local extensions from `~/.config/vimcode/extensions/*/manifest.toml`. `LspManager` stores manifests via `set_ext_manifests()`, `DapManager` functions accept manifests as parameters. New extensions can be added without updating VimCode code. Local extension development supported: create `manifest.toml` + scripts in `extensions/<name>/`, they appear in the sidebar automatically. Removed `extensions/` directory from repo. Generated `registry.json` with all 17 extension manifests. Updated EXTENSIONS.md with local dev workflow and registry submission guide.

**Session 165 — Extension Panel API + Git Log Panel (4053 tests):**
Extension panel infrastructure for custom sidebar panels from Lua plugins. New types: `PanelRegistration`, `ExtPanelItem`, `ExtPanelStyle` in plugin.rs. Lua API: `vimcode.panel.register/set_items/parse_event`, `vimcode.git.branches()`. Engine state: `ext_panels`, `ext_panel_items`, `ext_panel_active`, `ext_panel_has_focus`, `ext_panel_selected`, `ext_panel_scroll_top`, `ext_panel_sections_expanded`. `handle_ext_panel_key()` with j/k nav, Tab expand/collapse, Enter `panel_select` event, q/Esc unfocus, other keys `panel_action` event. Render: `ExtPanelData`/`ExtPanelSectionData` in render.rs, `build_ext_panel_data()`. TUI: `render_ext_panel()`, dynamic activity bar icons, keyboard routing, click handling. GTK: `SidebarPanel::ExtPanel(String)` variant. Git Log Panel: `git::list_branches()`/`BranchEntry` in git.rs, new `git_log_panel.lua` script with Branches/Log/Stash sections, manifest updated (8 scripts total). 17 integration tests in `tests/ext_panel.rs`.

**Session 163 — Git Insights enhancement (4036 tests):**
Full git-insights extension overhaul. Part 1: Extended Lua plugin API with 12 new `vimcode.git.*` bindings. Added 9 new git.rs functions + 2 structs (`DetailedLogEntry`, `StashEntry`). Part 2: Scratch buffer API — `ScratchBufferRequest` struct, `vimcode.buf.open_scratch()` Lua binding, engine handler in `apply_plugin_ctx()`. `BufferState.scratch_name` for `[Name]` tab display. 6 new Lua scripts for git-insights. BUNDLED array updated from 1→7 scripts. 36 new tests total. Also fixed Flatpak build (cargo-sources.json regen).

**Session 162 — Bulk paste performance fix (4003 tests):**
Fixed critical performance bug: pasting large text in insert mode caused UI freeze / 100% CPU. Root cause was `Event::Paste` (TUI) and `ClipboardPasteToInput` (GTK) feeding each character individually through `handle_key()`. New `Engine::paste_in_insert_mode(text)` method does a single bulk `insert_with_undo()`. Also added safety guard in `compute_word_wrap_segments()`. 8 new tests in `tests/paste_insert.rs`.

**Session 161 — Terminal install + F1 palette (3995 tests):**
Extension install scripts now run in a visible terminal pane (TerminalPane::new_command) instead of silently in the background — users see real-time output, errors, and can enter sudo passwords. InstallContext struct tracks extension name/install key for post-install LSP/DAP registration. EngineAction::RunInTerminal bridges engine→UI. F1 opens Command Palette in both Vim and VSCode modes (fixes Ctrl+Shift+P not working in many terminals). 3 new extension install tests.

**Session 160 — Extensions UX + workspace isolation + word wrap (3992 tests):**
Extension sidebar UX overhaul: Enter shows README preview for any extension (installed or available), `i` key installs (was Enter). Double-click in TUI Explorer fixed (last_click_time/pos updated at all click sites). Word-boundary wrapping (`compute_word_wrap_segments()` in render.rs). Workspace session isolation fix (global session `open_files` cleared to prevent cross-workspace bleed). LSP kickstart after extension install (`lsp_did_open` called on active buffer). LSP args fix (`InstallComplete` handler uses manifest args instead of empty vec). Bicep LSP install command rewritten (curl+unzip from Azure/bicep GitHub releases, not NuGet). Removed commentary Lua extension (native `:Comment` replaces it). All 16 extension READMEs rewritten with prerequisites and auto-install info. New `EXTENSIONS.md` extension development guide.

**Session 159 — Tree-sitter 0.24 + YAML/HTML highlighting (3989 tests):**
Tree-sitter upgrade (0.20→0.24) + YAML/HTML syntax highlighting (17 languages), TUI tab expansion fix, TUI activity bar icon readability, YAML key/value color fix, C# query fixes, v0.3.2.

**Session 158 — VSCode Mode Gap Closure Phases 1–3 (3934 tests):**
Alt key routing (TUI+GTK encode Alt+key→`"Alt_Up"` etc.), line operations (move/duplicate/delete/insert line), multi-cursor (Ctrl+D progressive select + `vscode_select_all_occurrences()`, extra selections rendering, same-line char-index descending sort), indentation (Ctrl+]/[ multi-cursor aware), panel toggles (Ctrl+J/Ctrl+`→`EngineAction::OpenTerminal`, Ctrl+B sidebar, Ctrl+, settings), quick nav (Ctrl+G with `ensure_cursor_visible()`, Ctrl+P/Shift+P), Ctrl+K chord prefix, GTK terminal mouse off-by-one fix (`term_px` +1→+2 for tab bar row, 9 locations), bottom panel sans-serif UI font, 55 tests.

**Session 157 — VSCode Mode Fixes + Build Portability (2941 tests):**
Fixed auto-pairs, bracket matching, and `update_bracket_match()` not running in VSCode mode (early return in `handle_key()` bypassed all three). Added auto-pair insert/skip-over/backspace-delete logic to `handle_vscode_key()`. Added `update_bracket_match()` call at end of `handle_vscode_key()`. 4 new VSCode-mode auto-pair tests. **Build portability**: `vcd` TUI binary now statically linked with musl (`--target x86_64-unknown-linux-musl`). Fixed Flatpak build: replaced `floor_char_boundary` with `is_char_boundary` loop, replaced `is_none_or` with `map_or(true, ...)` for GNOME SDK 47 Rust ~1.80 compat. Released v0.3.1.

**Session 156 — IDE Polish: Indent Guides, Bracket Matching, Auto-Pairs (2937 tests):**
Three visual/editing polish features: (1) Indent guides — vertical `│` lines at each tabstop, `indent_guides` setting. (2) Bracket pair highlighting — `bracket_match_bg` theme color, `match_brackets` setting. (3) Auto-close brackets/quotes — insert/skip-over/backspace-delete, smart quote context, `auto_pairs` setting. All three with `:set` toggle, settings UI entries, theme colors. 29 tests in `tests/ide_polish.rs`.

**Session 155 — Core Commentary Feature (2908 tests):**
Unified 3 comment implementations into `src/core/comment.rs`. `CommentStyle`/`CommentStyleOwned` types, `comment_style_for_language()` 46+ lang table, `compute_toggle_edits()` two-pass algorithm, `resolve_comment_style()` override chain, `CommentConfig` on `ExtensionManifest`, engine `comment_overrides: HashMap`, `toggle_comment()` replaces old methods, `:Comment`/`:Commentary` commands, `vimcode.set_comment_style()` plugin API, Ctrl+/ fix (GTK `"slash"`, TUI `'7'` for byte 0x1F), VSCode Ctrl+Q quit, F10 menu toggle. 19+31 tests.

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

**Session 87 — :set wrap /  line-wrap rendering (7 new tests, 731 total):**
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

**Session 155:** Core Commentary Feature — unified 3 comment implementations into `src/core/comment.rs`; `CommentStyle`/`CommentStyleOwned` types; `comment_style_for_language()` 46+ lang table; `compute_toggle_edits()` two-pass algorithm; `resolve_comment_style()` override chain (plugin→manifest→built-in→fallback `#`); `CommentConfig` on `ExtensionManifest`; engine `toggle_comment()` replaces `toggle_comment_range()`+`vscode_toggle_line_comment()`; `:Comment`/`:Commentary` commands; `vimcode.set_comment_style()` plugin API; Ctrl+/ fix (GTK `"slash"`, TUI `'7'`); VSCode Ctrl+Q quit, F10 menu toggle; 19+31 tests. 2908 total.

**Session 156:** IDE Polish — indent guides (vertical `│` lines at tabstops, active guide highlight, blank line bridging, TUI+GTK), bracket pair highlighting (`bracket_match_bg` theme color, `match_brackets` setting), auto-close brackets/quotes (skip-over, pair backspace, smart quote context, `auto_pairs` setting); 3 new settings + theme colors across all 4 themes; 29 tests in `tests/ide_polish.rs`. 2937 total.

**Session 157:** VSCode mode fixes + build portability — auto-pairs/bracket matching/`update_bracket_match()` added to `handle_vscode_key()` (was bypassed by early return); vcd musl static linking for Linux portability; Flatpak compat (`floor_char_boundary`→`is_char_boundary` loop, `is_none_or`→`map_or`); 4 new VSCode auto-pair tests; v0.3.1 release. 2941 total.

**Session 158:** VSCode Mode Gap Closure Phases 1–3 — Alt key routing (TUI+GTK encode Alt+key→`"Alt_Up"` etc.), line operations (move/duplicate/delete/insert line), multi-cursor (Ctrl+D progressive select + `vscode_select_all_occurrences()`, extra selections rendering, same-line char-index descending sort), indentation (Ctrl+]/[ multi-cursor aware), panel toggles (Ctrl+J/Ctrl+`→`EngineAction::OpenTerminal`, Ctrl+B sidebar, Ctrl+, settings), quick nav (Ctrl+G with `ensure_cursor_visible()`, Ctrl+P/Shift+P), Ctrl+K chord prefix, GTK terminal mouse off-by-one fix, bottom panel sans-serif UI font; 55 tests in `tests/vscode_mode.rs`. 2985 total.

**Session 159:** Tree-sitter upgrade + TUI fixes, v0.3.2 — Upgraded tree-sitter from 0.20→0.24 with all grammar crates. Added YAML and HTML syntax highlighting (17 languages total). Fixed YAML key/value color distinction (query overlap). TUI tab rendering fix (expand literal tabs to spaces, visual-column positioning for cursor/ghost text/selections/brackets). TUI activity bar icons: off-white color + `▎` accent bar for active panel. C# query fixes for updated grammar. 2985 total.

**Session 347 (May 1–2, 2026):** #166 diff-pane alignment + #296 Debug sidebar MSV migration + GTK rename catch-up. Three items landed via Path A on develop: (1) `b29a218` — diff panes no longer drift past the first hunk; `view.aligned_top: Option<usize>` pins both panes to one shared aligned-row index; `clear_all_diff_alignment()` helper; 2 regression tests. (2) `6d70dba` — GTK rename catch-up (`multi_section_view_layout` → `gtk_msv_layout`). (3) `285916b` — Debug sidebar migrated to `quadraui::MultiSectionView`; `render::debug_sidebar_to_multi_section_view` adapter (4 EqualShare sections, PerSection scroll); both TUI + GTK paint through `draw_multi_section_view`; click/scroll read cached `MultiSectionViewLayout` verbatim (never re-derive); fixed pre-existing TUI bug where debug-output panel click handler missing `col >= editor_left` intercepted sidebar clicks; net −139 LOC. Key lesson codified in CLAUDE.md "Paint↔click integration pattern": click never re-derives layout; paint caches it, click reads verbatim.

**Sessions 340–344 (Apr 29–30):** Phase C completion + MSV primitive + quadraui extraction. Session 340: #266/#267/#270/#271 shipped (RichTextPopup, Dialog, GtkBackend runner, FindReplacePanel). Session 341: Phase C stages 2–4 (#277 Scrollbar, #278 settings chrome, #279 MessageList). Session 342: Phase C Stage 1 — `quadraui::Editor` primitive + dual rasterisers, net −1456 LOC vimcode-private paint. Session 343: TUI/GTK paint duplication arc closed (#283/#285/#286/#280/#281), Phase C umbrella #275 closed. Session 344: `MultiSectionView` primitive shipped (#293), first MSV consumer (Extensions sidebar).

**Session 346 (Apr 30):** Harness-first course correction. Pivoted from failed #296 attempts (4 sessions / 8 commits on abandoned branch) to structural fix: #297 cell_quantum integer-snap, #298 TUI MSV round-trip harness, #299 TUI TreeView harness, #300 quadraui extracted to own repo at github.com/JDonaghy/quadraui. Migration prerequisites rule added to CLAUDE.md. 4 vimcode issues gained `blocked` label with cross-repo prereq links.
