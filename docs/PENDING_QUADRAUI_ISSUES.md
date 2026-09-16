# Pending quadraui issues — drafted, not yet filed

Vimcode worker sessions in this harness are `git`-only (no `gh` access); filing
GitHub issues, including on `JDonaghy/quadraui`, is a coordinator/human action.
This file holds issue text that a worker has fully drafted and verified but
could not file itself, so the finding survives past the session that found it
(per `GOALS.md`'s milestone-discipline rule: "a comment naming a missing
upstream API is an unfiled issue, and grep will not find it for you" — this
file exists so the comment *is* findable, and the filing doesn't get lost a
third time).

**Coordinator/human action:** file each entry below verbatim on
`JDonaghy/quadraui`, into milestone **#9 "vimcode Platform-Neutral
blockers"**, then delete its entry here and update the citing vimcode issue
(link the filed issue number, leave the vimcode issue **open** behind it per
`GOALS.md`'s rule, do not close on investigation alone).

---

## Multi-band bottom chrome (blocks vimcode#820)

**Title:** `ShellConfig`/`BottomPanelController` models one drawer; vimcode
needs N independently-gated stacked bottom bands

**Body:**

vimcode#820 asked vimcode to adopt `compose::BottomPanelController` on GTK
(then TUI) in place of ~500 lines of local bottom-chrome composition
(`src/render.rs`'s `paint_bottom_panel_rung`, `build_terminal_panel`,
`build_terminal_toolbar`, `build_bottom_panel_tab_bar`, `BottomPanelUnits`).

Investigating found this was already tried and rejected in earlier passes
(#608/#735/#763/#765), for a reason that still holds and blocks pure
adoption: `AppShell` positions `BottomPanelController` as **the single last
band** before `main_content_bounds`'s bottom edge — one resizable drawer.
vimcode stacks up to **five** bands below the editor content area, each
independently gated on its own boolean/state (not just open/closed height,
but presence): the terminal/debug-output panel, the terminal toolbar, the
debug toolbar, the quickfix list, the wildmenu, and (when
`window_status_line` is set without `status_line_above_terminal`) a
separated status row — see
`src/tui_main/render_impl.rs::bottom_chrome_rects_for_shell_content` and its
GTK counterpart `src/app.rs`'s `compose_bottom_band_rungs` for the exact
stacking order and gating conditions. A single generic drawer can't model
that; every rung needs independent presence *and* height, stacked bottom-up
with the others.

**Ask:** extend `ShellConfig`/`AppShellLayout` (or `BottomPanelController`
itself) to support **N** independently-gated bottom bands — each with its own
visibility flag and height — stacked bottom-up above the shell's bottom edge,
generalizing the current single-drawer model. vimcode's
`compose_bottom_band_rungs` (duplicated today between `src/app.rs:3176-3356`
and `src/tui_main/shell_app.rs:594-743`, five near-identical arms) is the
concrete shape of what a shared implementation would need to replace.

**Also verify once this lands** (vimcode#820's original suspected gaps,
narrower than the one above, not yet independently confirmed either way):
terminal split panes, and the terminal scrollbar content — check both have
`BottomPanelController`-side support before assuming pure GTK+TUI adoption is
otherwise complete.

**Blocks:** `JDonaghy/vimcode#820` — leave that issue open behind this one,
per `GOALS.md`'s milestone-discipline rule.

---

## `TabGroupController` has no external-model adoption path (blocks vimcode#822)

**Title:** `TabGroupController` requires owning its own pane/tab model; no
adoption path exists for a host that already has a source of truth

**Body:**

vimcode#822 asked vimcode to adopt `compose::TabGroupController` for tab
drag-and-drop and drop-zone computation, replacing vimcode's local
`TabDragState` (~140 lines, `src/render.rs`'s `TabDragMove`/`TabDragState`
impl block) and the local drop-zone adapter (~322 lines centered on
`compute_tab_drop_zone`/`build_tab_drop_groups`, `src/render.rs`).

Investigating (issue #822's first fix pass, PR for #822) found this is not a
like-for-like swap. The drop-zone *geometry math* is already shared —
`compute_tab_drop_zone` calls straight through to
`quadraui::compute_drop_zone`, and has since #515. What's left on the
vimcode side is the adapter: `TabGroupController` owns its own
`Vec<Pane>`/`GroupLayout` model and tab-bar rendering internally, and
translates gestures into string-keyed `TabGroupEvent`s that mutate *that*
internal model. vimcode already owns the authoritative editor-group/tab
model in `Engine` (`engine.editor_groups`, keyed by `GroupId`, tabs by
index) — adopting `TabGroupController` as specified would mean mirroring
that entire model into a second `Vec<Pane>` copy just to drive drag/drop,
then translating `TabGroupEvent`s back into `Engine` mutations. That's a
second source of truth, not a shim deletion.

There is a partial escape hatch already: a `PaneDragRect`-only adoption
path exists (drag-rectangle geometry without full state ownership), but
`handle_tab_drop` still requires the pane/tab mirror to resolve which pane
a drop lands on and how tabs reorder — so even the narrow path can't avoid
the mirror.

**Ask:** give `TabGroupController` (or a sibling type) an adoption path for
host apps that already own their tab/pane model — e.g. accept a
borrowed slice/trait describing the current tabs + layout for hit-testing
and drag/drop-zone computation, and emit position-based drop instructions
(source index, target index, split direction) that the host translates
into its own mutations, rather than requiring `TabGroupController` to own
and mutate `Vec<Pane>` itself.

**Blocks:** `JDonaghy/vimcode#822` — item 1 of that issue (delete the
`TabBarLayout` downconversion shim) is done; item 2 (`TabGroupController`
adoption, replacing `TabDragState` and the drop-zone code) is the
remaining scope and depends on this. Leave #822 open behind this one, per
`GOALS.md`'s milestone-discipline rule — do not close #822 on the first
fix pass alone.

---

## `quadraui::win::testing` is hard `target_os = "windows"`-gated, not WinAPI-stubbed like `win::backend`/`run`/`shell_runner` (blocks vimcode#928 AC2)

**Title:** `win::testing` needs the same `cfg(target_os = "windows")`-per-call
stubbing as `win::backend`/`run`/`shell_runner`, not a module-level
`target_os` gate

**Body:**

vimcode#928 adopts `quadraui::testing::ConformanceDriver` as a single
backend-neutral black-box test harness and requires (acceptance criterion
#2) that `cargo check --no-default-features --features win` type-check the
`WinDriver` instantiation of that harness on an ordinary Linux host — the
same posture `quadraui::win::backend`/`run`/`shell_runner` already have:
gated on `feature = "win"` alone, with every real WinAPI call individually
`cfg(target_os = "windows")`-gated internally and falling back to a stub
everywhere else, specifically so a Linux CI runner can type-check
`WinBackend` (see quadraui's own `ci.yml` "Compile check (win feature)"
step, and `docs/RELEASING.md` §1.4 on the vimcode side, which documents
this as the existing, working pattern).

At the pinned rev (`dbb3023`), `quadraui/src/win/mod.rs:190-192` declares
`pub mod testing;` — the module defining `WinDriver`/`driver_with_shell` —
`#[cfg(target_os = "windows")]`-gated at the module level, with no
internal WinAPI stubbing inside it the way `win::backend`/`run` have. That
means the module (and everything in it) simply does not exist to the
compiler off Windows; there is no `cargo check`/`cargo test --no-run`
invocation on Linux that can even *see* `WinDriver`, let alone type-check
code that constructs one.

vimcode's own `src/win/mod.rs::win_driver_tests` module (added by #928) has
to compound that with its own `#[cfg(target_os = "windows")]` (on top of
`#[cfg(test)]`), so its `ConformanceHarness<WinDriver<...>>` instantiation
is verified only on a real Windows host — never on the Linux fleet this
project develops on day to day. That directly blocks #928's acceptance
criterion #2, which is currently **unmet** and will stay unmet until this
lands.

**Ask:** gate `quadraui::win::testing` the same way `win::backend`/`run`/
`shell_runner` are gated — `feature = "win"` alone, with `WinDriver`'s
internals individually `cfg(target_os = "windows")`-stubbing their WinAPI
calls (window creation, message loop, hit-testing surface) — so
`cargo check --no-default-features --features win` (and
`cargo check --tests` / `cargo test --no-run` with the same flags) type-checks
`WinDriver`/`driver_with_shell`/`ConformanceHarness<WinDriver<...>>` on an
ordinary Linux host, exactly as it already does for `WinBackend` itself.

**Blocks:** `JDonaghy/vimcode#928` — acceptance criterion #2
("`cargo check --no-default-features --features win` type-checks the Win
instantiation on an ordinary Linux host") is unmet until this lands.
`src/win/mod.rs::win_driver_tests` stays double-gated
(`#[cfg(target_os = "windows")]` + `#[cfg(test)]`) in the interim — inert,
and known to be inert, on every host but real Windows. Leave #928 open
behind this one per `GOALS.md`'s milestone-discipline rule; do not treat
the double gate as a workaround that closes the gap.

---

## TUI minimap has no horizontal downsampling (blocks vimcode#1030 deliverable 2)

**Title:** `tui::minimap` hardcodes one source column per braille dot column
(`COLS_PER_CELL = 2`), so a VS-Code-proportioned TUI strip can only ever
represent ~22 source columns

**Body:**

vimcode#1030 (the fix issue for vimcode#990) asked for the TUI minimap to
keep painting syntax colour for deeply-indented code. Its written
deliverable 2 is verbatim: *"Colour must survive at indent 40 and 80, not
only near column 0."*

quadraui#993 (landed at `8abca3a`, the rev vimcode#1030 bumps to) fixed the
real defect behind that report: the TUI dot rasteriser used to normalise
each line by its own `chars().len()`, so a deeply-indented line's content
was stretched across the whole strip and every cell painted *some* dot —
while `cell_color`'s lookup stayed literal, found no span that far out, and
fell back to `theme.foreground`. The symptom vimcode#990 reported ("no
colour past column ~24") was therefore 100 fully-set braille cells painted
in one fallback colour, with nothing real behind them. `8abca3a` makes
`braille_char_for_cell` literal too, so dots and colour agree and a line
wider than the strip clips instead of squeezing — VS Code's own behaviour,
and the right fix. Confirmed on the vimcode side by
`src/tui_main/shell_app.rs`'s
`minimap_paints_syntax_colour_for_indented_code`.

What remains is the **scale**, and it is not reachable from a host.
`quadraui/src/tui/minimap.rs` hardcodes it:

```rust
pub const COLS_PER_CELL: usize = 2;

// braille_char_for_cell:
let dot_col = col * 2 + dc;
let cols_per_dot = (COLS_PER_CELL / 2).max(1);   // == 1, always
let c0 = dot_col * cols_per_dot;

// cell_color: same literal grid
let col_lo = col * COLS_PER_CELL;
let col_hi = col_lo + COLS_PER_CELL;
```

One braille dot column is exactly one source character column, so an
`N`-cell strip represents `2N` source columns and nothing a host passes in
can widen that — including the `MinimapGrid` handed to `aggregate_spans`,
which vimcode#1030 right-sized to the painted strip precisely to rule that
out.

Measured in vimcode at a 100x24 terminal with a VS-Code-proportioned strip
(`MinimapSizing::VsCodeParity { target_cols: 12, fraction: 0.15, min: 6,
max: 30 }`), fixture = 100 lines of `let value_N = 1;` at a given indent,
400 tree-sitter highlights present throughout. The strip paints **11 cells
= 22 source columns**; distinct painted dot colours by indent:

```text
indent   0   4   8  12  16  18  20 | 22  24  28  40  80
colours  5   5   3   2   2   1   1 |  0   0   0   0   0
dot rows 20  20  20  20  20  20  20|  0   0   0   0   0
```

Colour is correct and per-token everywhere inside `0..22` — and everything
from column 22 on is simply not representable. Code at three indent levels
(12 spaces) is already half-clipped and a four-level-indented block paints
nothing at all, which is a real usability gap in its own right, not just a
blocked acceptance criterion.

**This is TUI-only, and the asymmetry is measured, not assumed.** GTK's
rasteriser paints one 1px block per character column out to
`primitives::minimap::COLUMN_CAPACITY` (120), and VS Code's own minimap
reaches ~120 columns; the TUI reaches 22. The same fixture through vimcode's
GTK driver (1400x900, 120px strip) paints **7 distinct colours at indents 0,
20, 40 and 80** — see
`src/gtk/testing.rs`'s
`minimap_paints_distinct_syntax_colors_at_indentation_via_gtk_driver`. So
vimcode#1030's deliverable 2 is satisfied literally on GTK and is
unreachable only on TUI, and only because of the constant above.

Both vimcode-side alternatives are non-options, which is why this is filed
upstream rather than worked around (Platform-Neutrality Rule): raising
`render::MINIMAP_TARGET_COLS_TUI` from 12 cells to the 60 needed to cover
120 columns would hand 60 of an 80-column terminal to the minimap, and
per-line re-normalisation is exactly the quadraui#993 bug.

**Ask:** make the horizontal scale a **parameter** instead of a constant —
e.g. a `cols_per_cell` (or `source_cols_per_dot`) carried on
`MinimapSizing` / accepted by `tui_minimap_layout`, honoured by both
`braille_char_for_cell` and `cell_color`, defaulting to today's `2`/`1` so
existing hosts are unchanged. The dot side is nearly free: the code already
derives `cols_per_dot` from `COLS_PER_CELL`, so with an effective
`COLS_PER_CELL = 10` an 11-cell strip would cover 110 source columns at 5
source columns per dot. Note this is a **shared** scale — the same for every
line in the file — so quadraui#993's property is preserved, not undone: a
4-space indent still lands at the same dot column on a short line and a long
one. Colour follows automatically, since hosts already declare their grid's
`cols_per_cell` to `aggregate_spans`; the host just needs to be able to read
(or set) the effective value so its `MinimapGrid` matches what the
rasteriser will read.

**Blocks:** `JDonaghy/vimcode#1030` — deliverable 2 ("Colour must survive at
indent 40 and 80, not only near column 0") holds on GTK (7 distinct colours
at both indents, asserted) but is **unmet on TUI** and cannot be met there
until this lands. Everything else in #1030 is done and verified: colour
survives at every depth the strip can actually represent (out to its last
cell), the aggregation grid is derived from the painted strip on both
backends, and the clip boundary is asserted from both sides in
`minimap_paints_syntax_colour_for_indented_code`. **Coordinator/human action
beyond filing:** deliverable 2 needs an explicit decision on the vimcode
issue — either amend #1030's acceptance text to the clip-not-stretch
behaviour verified in that test, or waive the deliverable — and per
`GOALS.md`'s milestone-discipline rule leave it open behind this issue
rather than closing #1030 as if the criterion had been met. A worker session
cannot make that call or edit the issue (`git`-only, no `gh`), which is why
it is recorded here.

---

## `TuiBackend` lets a `None` cursor_position clobber a `Some` within one frame (blocks vimcode#1039)

**Title:** `TuiBackend::last_cursor_position` is overwritten unconditionally
by every `Backend::draw_editor` call, so an inactive window's `None` can
clobber the active window's `Some` painted earlier in the same frame

**Body:**

vimcode#1039 reported that with two tab groups (a split) open, entering
insert mode in the **left** group painted no visible caret — the caret is a
`Bar`/`Underline` shape, which is not painted into the cell buffer like
`Block`; it is placed once per frame via `ratatui::Frame::set_cursor_position`.

Investigating (this issue's fix) found the clobber is one layer deeper than
the issue's own hypothesis. vimcode already gates `RenderedWindow.cursor` to
`Some` only for the active window (`render::build_rendered_window`'s
existing `is_active` check), so `quadraui::tui::draw_editor`
(`quadraui/src/tui/editor.rs:460`, pinned rev `8abca3a`) never actually
reports a `Bar`/`Underline` `cursor_position` for an inactive window today —
inactive windows call `draw_editor` with a cursor-less editor and get
`cursor_position: None` back correctly.

The clobber instead happens in `quadraui::tui::TuiBackend`
(`quadraui/src/tui/backend.rs:2534`, pinned rev `8abca3a`):

```rust
// backend.rs:2534, inside the fn that turns an EditorPaintResult into a
// Backend::draw_editor return value:
self.last_cursor_position = tui_result.cursor_position;
```

This assignment is **unconditional** — every `draw_editor` call overwrites
`last_cursor_position`, including calls for inactive windows that report
`None`. `last_cursor_position` is later drained once per frame via
`TuiBackend::take_last_cursor_position` (`backend.rs:649`) and applied to
the real `Frame::set_cursor_position` (`quadraui/src/tui/run.rs:427-428`).
So whichever window's `draw_editor` call happens to run **last** in a given
frame decides the whole frame's caret — regardless of which window is
actually active/focused. vimcode's `render_all_windows` painted windows in
a fixed layout order (left, then right), so a left-active split had its
correct `Some` position from the left window's `draw_editor` call
overwritten by `None` from the right window's later, cursor-less call —
exactly the "left group breaks, right/single group works" symptom the issue
reported.

**vimcode-side workaround already shipped** (this issue, `750d5e7`):
`render_all_windows` (`src/tui_main/render_impl.rs`) now partitions
`windows` by `is_active` and paints the active window **last**, so its
`Some` position is always the one still standing when `take_last_cursor_
position` drains at end of frame. This is a reorder against data
(`RenderedWindow.is_active`) vimcode already had — no new per-backend
state — but it is a workaround coupled to `TuiBackend`'s specific
last-write-wins behaviour: `render_all_windows` is currently the only TUI
call site of `draw_editor` and exactly one window is ever `is_active`
(`render.rs:13159`), so it holds today, but it would silently stop
mattering (harmlessly) or silently regress (if quadraui's cache semantics
change the other direction) with no compile-time signal, if quadraui's
internals change — e.g. parallelized per-window painting, or multiple
non-window `draw_editor` callers appearing.

**Ask:** make `TuiBackend`'s cache never let a `None` `cursor_position`
clobber a `Some` already recorded earlier in the same frame — e.g. only
overwrite `last_cursor_position` when the new value is `Some`, or reset to
`None` explicitly at frame-start (`backend.rs:1245` already does a
frame-start reset — the question is `2534`'s per-call overwrite happening
unconditionally *within* a frame after that reset) rather than on every
individual `draw_editor` call regardless of its own result. Once that
lands, vimcode's `render_all_windows` partition-and-reorder becomes
redundant scaffolding (order no longer matters) and can be deleted, or kept
harmlessly — either is fine, but should be a deliberate follow-up on the
vimcode side once this fix ships, not silently forgotten.

**Blocks:** `JDonaghy/vimcode#1039` — the vimcode-side reorder in
`750d5e7` is a correct workaround for the caret's visible symptom today,
but the underlying bug is upstream. Leave #1039 open behind this one per
`GOALS.md`'s milestone-discipline rule; do not treat the reorder as the
permanent fix.
