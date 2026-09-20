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

## TUI runner has no host-facing "force full repaint" hook (blocks vimcode#58)

**Title:** `tui::run`/`run_with_shell` internalised the `Terminal`, silently
dropping the only mitigation vimcode#58 (stale-character rendering artifacts)
ever had — no `Reaction`/`Backend` hook replaces it

**Body:**

vimcode#58 tracks intermittent stale characters left on screen: ratatui's
incremental diff can miss cells when the physical terminal's real state
diverges from its internal `Buffer` tracking (typical triggers: PTY writes
into the embedded terminal pane, certain resize sequences, popup
dismissal). vimcode's Session-244 mitigation was to call
`ratatui::Terminal::clear()` — which resets the diff cache so the *next*
frame repaints every cell unconditionally — on resize events and on
popup-dismiss transitions, from its own hand-rolled event loop
(`src/tui_main/mod.rs`, pre-#634).

That loop no longer exists. #634 moved vimcode's TUI onto
`quadraui::tui::shell_runner::run_with_shell` (this crate's `tui::run`/
`run_with` family, `quadraui/src/tui/run.rs`), which now owns the
`ratatui::Terminal` internally and calls `terminal.clear()` exactly once,
at startup (`run_with`, `quadraui/src/tui/run.rs:203`) — never again for
the life of the process. Confirmed by reading the pinned rev
(`7a77602`): `Reaction` (`quadraui/src/runner.rs`) has only
`Continue`/`Redraw`/`RedrawAfter(Duration)`/`Exit` — no variant that maps
to "clear before the next draw" — and neither `Backend` nor `AppLogic`
exposes a `request_full_repaint`-shaped method the runner's frame loop
would consult. So there is currently no way for a quadraui-hosted TUI app
to ask for what `Terminal::clear()` gives a raw ratatui app.

vimcode's own code already documents this as a known, currently-inert
gap rather than working around it: `render::is_force_redraw_key`'s doc
comment (Ctrl+L, `src/render.rs`) and `TuiShellApp::render_content`'s
`had_popup_overlay` tracking (`src/tui_main/shell_app.rs`) both say so —
Ctrl+L today only returns `Reaction::Redraw`, which re-runs the same
incremental diff that missed the cells in the first place, so it does not
actually fix anything a user hits it for. `had_popup_overlay` is computed
and stored every frame but has no reader left — the call site it used to
drive (`terminal.clear()`) was deleted along with the legacy loop.

**Ask:** give a TUI-hosted `AppLogic` a way to force the next frame to
paint as if the terminal were blank. Two shapes, either resolves this:

1. A new `Reaction::FullRedraw` variant — `tui::run`'s frame loop calls
   `terminal.borrow_mut().clear()?` before the next `render_frame` when an
   event handler returns it, otherwise identical to `Reaction::Redraw`.
2. A `Backend::request_full_repaint()` method (default no-op) that
   `TuiBackend` implements by setting a flag the runner checks each loop
   iteration before drawing — mirroring how `request_frame_in`/
   `Reaction::RedrawAfter` already thread a scheduling request through the
   same seam, so it needs no new event/dispatch plumbing.

GTK does not need this: Cairo repaints its `DrawingArea` in full every
frame (no incremental diff to desync), which the existing
`gtk::backend` tests documenting "full repaint after a skipped frame /
modal closed / theme change" already rely on. So this is a TUI-only gap
today, but the hook itself should stay on the backend-neutral trait
surface (`Backend`, not a TUI-only escape hatch) so a future diff-based
renderer (a terminal-multiplexer-aware Win-GUI console mode, say) isn't
left with the identical hole.

**Consumers waiting on this, already commented in place:**
`render::is_force_redraw_key` (Ctrl+L) and
`TuiShellApp::render_content`'s `had_popup_overlay` field
(`src/tui_main/shell_app.rs`) both name the exact call site that would
call the new hook the moment it exists.

**Blocks:** `JDonaghy/vimcode#58` — leave that issue open behind this one,
per `GOALS.md`'s milestone-discipline rule.

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

## ~~`quadraui::CommandLine` has no `selection` field to paint~~ — **SHIPPED, do not file (struck 2026-09-19, #1168)**

> **This draft is retired. The API exists and is already pinned.** quadraui#1001
> landed `Backend::draw_command_line_selection` alongside
> `CommandLineLayout::selection_bounds`, and vimcode's current pin
> `d907a06` (bumped by #1133) carries it — verified by `git grep` against that
> rev, not inferred from the issue being closed. The shape differs from the
> "Ask" below: upstream chose a **dedicated draw call** rather than a
> `selection` field on `CommandLine`, so do not go looking for the field.
>
> **What is actually left is host-side, and belongs to vimcode#1169, not here:**
> vimcode adopts `selection_bounds` but does not yet call
> `draw_command_line_selection`, so `render::command_line_selection_rect` still
> hand-computes the rect and **its doc comment still claims the upstream API does
> not exist** — that comment is stale and should be deleted along with the helper
> when the paint call is adopted. Fixing it is a code change, deliberately out of
> scope for #1168's documentation pass.
>
> vimcode#194's visual-highlight half is therefore **no longer supply-blocked**.
>
> The original draft is kept below, struck, so the history of the verdict is
> readable — **do not file it.**

### ~~Original draft (superseded)~~

**Title:** `CommandLine`/`draw_command_line` cannot paint a selection highlight —
`CommandLineLayout::hit_test`/`selection_bounds` compute the geometry but nothing
carries it to either backend's paint call

**Body:**

vimcode#1044 (the ShellApp/mouse.rs decomposition audit) re-verified
`docs/IRREDUCIBLE_SURFACE.md` §2a's existing verdict on command-line text
selection and found it **stale, not wrong in direction**: that section says
`CommandLineLayout::hit_test` "does not exist anywhere in quadraui" — true when
written (2026-09-03), but quadraui#705 shipped
`CommandLineLayout::hit_test`/`selection_bounds` (`quadraui/src/primitives/command_line.rs:98,134`,
present at the currently-pinned rev `8abca3a`) and vimcode already adopted both,
unconditionally shared by both backends: `render::command_line_click_char_idx`
and `render::command_line_selection_rect` (`src/render.rs:19933,19979`) call
straight through to them. So the **hit-test** half of the old verdict is now
`already-shared`, not a gap — no issue needed there.

What's left, and is real and current: `render::command_line_selection_rect`'s
own doc comment already names it — `quadraui::CommandLine` carries no
`selection` field, and neither the GTK nor TUI `draw_command_line` in quadraui
paints one. `command_line_selection_rect` computes the paintable highlight rect
and has never been wired into either paint path (its doc: "Not wired into
either backend's paint path yet (#816 review)"). TUI works around this by
painting the command line cell-by-cell with the selection baked into the
foreground/background inversion (`tui_main::panels::render_command_line`) —
so TUI *has* a visible selection highlight today, just via a hand-rolled paint
path instead of the shared primitive. GTK has **no visible highlight at all**:
a user who drags a selection over the GTK command line gets `cmd_sel`/Ctrl+C
behavior with zero visual feedback, because there is nowhere in
`quadraui::CommandLine` to put the selection so GTK's `draw_command_line` could
paint it.

**Ask:** add a `selection: Option<(usize, usize)>` (or similar) field to
`quadraui::CommandLine`, and have both backends' `draw_command_line`
(GTK/Cairo, TUI/ratatui) paint the corresponding highlight rect/cell-inversion
when it's set — the geometry math for GTK is already done and waiting
(`render::command_line_selection_rect`); the ratatui side would let TUI stop
hand-painting the highlight itself and instead pass `selection` through like
every other `CommandLine` field.

**Blocks:** `JDonaghy/vimcode#194` ("Status-bar / command-line messages aren't
mouse-selectable — GTK can't; TUI has offset bug") — the hit-test half of #194
is unblocked (already-shared, as above); the visual-highlight half stays
blocked on this. Leave #194 open behind this one per `GOALS.md`'s
milestone-discipline rule. Also update `docs/IRREDUCIBLE_SURFACE.md` §2a once
this is filed — that section's "does not exist anywhere in quadraui" claim
needs correcting to point at this narrower, still-open gap instead (done in
this same PR, see that file's new §2c).

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

---

## `draw_editor`'s decoration overlays index against the caller's `area`, not the real `buf` extent — panics on terminal resize (blocks vimcode#203)

**Title:** Indent guide / color column / diagnostic / spell / bracket-match
paint in `quadraui::tui::draw_editor` bounds-check against the *caller-supplied*
`area` rect instead of `buf.area` (the live `Buffer`'s real extent), so a
resize that shrinks the buffer between layout and paint panics with `index
outside of buffer`

**Body:**

vimcode#203 reported a TUI crash on terminal resize with the Extensions
panel (or any overflowing sidebar/panel) visible:

```
VimCode internal error: index outside of buffer: the area is Rect { x: 0, y: 0, width: 161, height: 32 } but index is (41, 32)
```

Investigating found the panicking code no longer lives in vimcode — #276
Stage 1C (`c985d58`) lifted vimcode's `render_impl::render_window` body
verbatim into `quadraui::tui::draw_editor`
(`quadraui/src/tui/editor.rs`, pinned rev `7a77602`, confirmed still current
at upstream HEAD `4253432` — no commits touch this file or `tui/run.rs`
between the pin and HEAD). vimcode's own call site
(`src/tui_main/render_impl.rs::render_window`) is the ~25-line delegator
`c985d58`'s commit message describes: it converts `RenderedWindow` to
`quadraui::Editor` and calls `backend.draw_editor(rect, &editor)` — no
buffer indexing, no bounds logic, nothing left to fix on the vimcode side.

**Root cause, `quadraui/src/tui/editor.rs`:** five decoration-overlay blocks
in `draw_editor` guard their `buf[(cx, screen_y)]` write with:

```rust
if cx < area.x + area.width && screen_y < area.y + area.height {
    let cell = &mut buf[(cx, screen_y)];
    ...
}
```

— checking the *painted-into* cell against `area`, the `Rect` the caller
passed in for this call, not against `buf.area` (the `Buffer`'s actual
allocated extent). The five sites, all identical in shape:

- indent guides — line 221
- color columns — line 245
- diagnostic underlines — line 285
- spell-error underlines — line 308
- bracket-match highlight — line 330

Three **other** overlay sites in the same function already guard correctly,
against `buf.area` rather than `area` — proving the fix pattern already
exists in-file and these five are simply inconsistent with it:

- cursor `Block` paint — lines 449-454 (`let buf_area = buf.area;` then
  `cursor_screen_x < buf_area.x + buf_area.width && cursor_screen_y <
  buf_area.y + buf_area.height`)
- secondary-cursor paint — lines 503-505
- selection-highlight paint — lines 718-719

`area` and `buf.area` are normally identical — `area` is derived from the
same terminal size `buf` was allocated for. They diverge when the terminal
resizes in the narrow window between when the host (vimcode, via the
quadraui `AppShell`/`run_with_shell` runner) computed window layout rects
from one size and when `ratatui::Terminal::draw` actually resized/reallocated
its buffer for the *next* size: `quadraui::tui::run::render_frame`
(`quadraui/src/tui/run.rs:443-456`) queries `terminal.size()` once, calls
`backend.begin_frame(Viewport::new(size...))` (which the host's layout pass
uses to size windows), and only *then* calls `terminal.draw(...)` — whose
internal `autoresize()` re-queries the backend's real size and can observe a
smaller value if the terminal shrank in between. The result: `draw_editor`
is called with a stale, too-large `area` against a buffer that's already
been shrunk to the new, smaller size — exactly the crash's `Rect { width:
161, height: 32 }` (stale layout) vs. index `(41, 32)` (`y == 32`, one past
the real, already-resized buffer's last row).

**Ask:** two independent, complementary fixes:

1. In `draw_editor`, change the five inconsistent sites (lines 221, 245,
   285, 308, 330) to bounds-check against `buf.area` the way the three
   already-correct sites do — a mechanical, four-line-per-site fix that
   makes the function internally consistent and turns this class of bug
   into a defensive no-op regardless of what `area` the caller supplies.
2. In `quadraui/src/tui/run.rs`, close the TOCTOU gap itself: derive the
   layout-sizing `Viewport` passed to `begin_frame` from the *same* size
   `Terminal::draw`'s closure actually paints into (e.g. move the
   `begin_frame` call, or the size query feeding it, inside the
   `terminal.draw(|frame| ...)` closure and use `frame.area()`), so a
   host's window layout is never computed against a size other than the
   one the buffer it paints into was just resized for.

Fix 1 alone stops the panic (the guard becomes correct); fix 2 removes the
underlying stale-layout condition that produces visibly wrong (if
non-crashing) paint in the resize frame even after fix 1 — a truncated
window silently painting nothing in its last row/column rather than
panicking. Recommend shipping both.

**Blocks:** `JDonaghy/vimcode#203` — no vimcode-side code change is possible
here per this repo's Platform-Neutrality Rule (the panicking code, and its
three correctly-guarded siblings proving the intended pattern, are entirely
inside `quadraui::tui::draw_editor`/`run.rs`). Leave #203 open behind this
one per `GOALS.md`'s milestone-discipline rule; do not close on
investigation alone. vimcode's panic hook already flushes swap files before
the crash unwinds (`src/core/swap.rs:62`), so no data loss occurs today —
this is a crash/robustness fix, not a recovery-path fix.
