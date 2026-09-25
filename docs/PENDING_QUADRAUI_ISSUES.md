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

**Before filing, re-check the gap still exists at the pinned rev.** A draft
below is a snapshot from whatever quadraui rev the drafting session had
checked out; the *pinned* rev (`Cargo.toml`'s `quadraui = { ... rev = "..." }`)
can move — and the gap can close upstream — between when a draft is written
and when it is actually filed. Confirm the gap against the current pin
immediately before filing, not against a memory of when the draft was
written. #1259's audit found four entries whose gap had already closed
(struck below); one of the four (`TuiBackend`'s cursor-position clobber) was
filed as quadraui#1039 on 2026-09-20 for a gap whose fix had already landed
upstream three days earlier (quadraui#1002, `e4ef921`, 2026-09-17) — from a
checkout that predated it — and cost three worker dispatches, each exiting
with zero commits, before quadraui#1039 was closed as a duplicate. A
re-check against the pin immediately before filing would have caught that at
zero cost instead.

---

## ~~TUI test drivers can't observe `Backend::request_full_repaint`'s effect from a downstream `ShellApp` (blocks vimcode#1243's black-box test)~~ — **FILED as quadraui#1060, do not file (struck 2026-09-24)**

> **This draft is retired: it is now a real issue.** Filed 2026-09-24 as
> quadraui#1060 (milestone #9), after re-checking the gap at vimcode's pin
> `3020d9e` and quadraui `develop`: there is still no `vt_testing::driver_with_shell`,
> and `build_shell_adapter` and `TuiBackend::take_full_repaint_requested` are
> still `pub(crate)`. The full draft text lives in that issue now. #1243 was
> closed before this was filed, so the black-box test it asked for is tracked
> as **vimcode#1393**, queued behind quadraui#1060 and the pin bump
> vimcode#1388.

---

## ~~TUI runner has no host-facing "force full repaint" hook (blocks vimcode#58)~~ — **SHIPPED, do not file (struck 2026-09-23, #1243)**

> **This draft is retired. The API exists, is pinned, and is now adopted.**
> quadraui#1037 shipped exactly the "Ask" shape 2 below —
> `Backend::request_full_repaint()`, default no-op, implemented on
> `TuiBackend` as a flag `tui::run::run_inner` consumes via
> `take_full_repaint_requested` and answers with `Terminal::clear()` before
> the next `render_frame`. Verified present at vimcode's pin `215e9e4` by
> reading `quadraui/src/backend.rs:1368`, `quadraui/src/tui/backend.rs:1543`
> and `quadraui/src/tui/run.rs:338`, not inferred from the issue being closed.
>
> vimcode#1243 consumed it: `render::is_force_redraw_key`'s Ctrl+L rung and
> `TuiShellApp::render_content`'s `had_popup_overlay` transition — the two
> "consumers waiting on this" named in the draft below — both call the hook
> now, so `had_popup_overlay` has a reader again.
>
> **What is left is test infrastructure, not the hook**, and it is filed as its
> own entry directly above ("TUI test drivers can't observe
> `Backend::request_full_repaint`'s effect…"): the shipped hook has no
> downstream-observable effect under any public driver, so vimcode#1243 cannot
> yet ship the black-box test its own acceptance criteria require.
>
> The original draft is kept below, struck, so the history of the verdict is
> readable — **do not file it.**

### ~~Original draft (superseded by quadraui#1037)~~

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

## ~~Multi-band bottom chrome (blocks vimcode#820)~~ — **SHIPPED, do not file (struck 2026-09-22, #1259)**

> quadraui#997 (`d1b1931`) shipped N independently-gated stacked bottom
> bands, and vimcode's pinned rev carries it. This draft is retired — do
> not file it.

---

## ~~`TabGroupController` has no external-model adoption path (blocks vimcode#822)~~ — **SHIPPED, do not file (struck 2026-09-22, #1259)**

> quadraui#998 (`ca6f40a`) shipped an external-model adoption path for
> `TabGroupController`, and vimcode's pinned rev carries it. This draft is
> retired — do not file it.

---

## ~~`quadraui::win::testing` is hard `target_os = "windows"`-gated, not WinAPI-stubbed like `win::backend`/`run`/`shell_runner` (blocks vimcode#928 AC2)~~ — **SHIPPED, do not file (struck 2026-09-23, #1244)**

> quadraui#1038 (landed in the rev this crate is pinned to, `Cargo.toml`)
> gated `win::testing` on `feature = "win"` alone with every real
> Direct2D/GDI call individually `cfg(target_os = "windows")`-stubbed,
> exactly the ask below. vimcode#1244 consumed it: `src/win/mod.rs`'s
> `win_driver_tests` module dropped its `#[cfg(target_os = "windows")]`
> double-gate down to `#[cfg(test)]` alone (each `#[test]` attribute is now
> individually `cfg_attr(target_os = "windows", test)`-gated instead, so the
> bodies type-check everywhere but only actually run on real Windows),
> closing vimcode#928's acceptance criterion #2. This draft is retired — do
> not file it.

### ~~Original draft (superseded by quadraui#1038)~~

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

## ~~TUI minimap has no horizontal downsampling (blocks vimcode#1030 deliverable 2)~~ — **SHIPPED, do not file (struck 2026-09-22, #1259)**

> Shipped at `55acc70` and consumed on the vimcode side by #1175. This draft
> is retired — do not file it.
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

## ~~`TuiBackend` lets a `None` cursor_position clobber a `Some` within one frame (blocks vimcode#1039)~~ — **CLOSED AS DUPLICATE, do not file (struck 2026-09-22, #1259)**

> This draft named a gap that had already closed upstream, unnoticed: the
> fix landed as quadraui#1002 (`e4ef921`) on 2026-09-17, three days before
> this draft was filed (from a checkout that predated the fix) as
> quadraui#1039 on 2026-09-20. quadraui#1039 was closed as a duplicate on
> 2026-09-21, after three worker dispatches each exited with zero commits
> because there was nothing left to change. This draft is retired — do not
> file it, and do not link #1039 as a live issue anywhere citing
> vimcode#1039. The other issues filed from this same batch —
> quadraui#1037, #1038, #1040 — are unaffected and remain live; see this
> file's other entries.

---

## ~~`draw_editor`'s decoration overlays index against the caller's `area`, not the real `buf` extent — panics on terminal resize (blocks vimcode#203)~~ — **LANDED UPSTREAM, consumed by the pin (struck 2026-09-23, #1246)**

> Filed as quadraui#1040 (title: "`draw_editor` bounds-checks overlays against
> `buf.area`, not stale area") and fixed there in `a0961f8` (2026-09-20),
> which also closed the TOCTOU gap in `quadraui/src/tui/run.rs` that produced
> the stale `area` (fix 2 of the two-part ask below — `render_frame`'s
> `Viewport` is now derived from `frame.area()` inside the
> `terminal.draw(...)` closure instead of a pre-draw `terminal.size()`
> query). Confirmed via `git merge-base --is-ancestor a0961f8 <pin>` against
> vimcode's `Cargo.toml` pin (`215e9e4...`, unchanged by this check) that the
> fix commit is already an ancestor of the pinned rev — it landed via an
> earlier, unrelated pin bump (the rev was already this far ahead when #1246
> picked this up), so no `Cargo.toml` edit was needed here.
>
> vimcode#203/#1246's premise — "vimcode carries a host-side guard" — did not
> hold: the #203 investigation (`51776a1`) found no such guard was ever
> added, and none exists in `src/tui_main/render_impl.rs::render_window`
> today (confirmed by inspection — it is still the ~25-line delegator the
> investigation described, no bounds logic to remove). There is also no
> `#203` regression test in vimcode to "stay green" — the crash lived
> entirely inside `quadraui::tui::draw_editor`/`run.rs`'s TOCTOU gap between
> a pre-draw size query and `Terminal::draw`'s internal autoresize, which
> `ratatui::backend::TestBackend`'s fixed-size construction (what
> `quadraui::tui::testing::TuiDriver` drives) cannot reproduce without new
> quadraui-side test infrastructure to resize a `TestBackend` mid-`Terminal`
> lifetime — out of scope for a vimcode-only issue. #203 is safe to close as
> fixed upstream and consumed by the pin.

---

## ~~`quadraui::tui::testing::TuiDriver` has no way to resize its `TestBackend` mid-test — a TOCTOU/resize regression (e.g. quadraui#1040's class of bug) can't be driver-tested from a downstream crate~~ — **FILED as quadraui#1063, do not file (struck 2026-09-24)**

> **This draft is retired: it is now a real issue.** Filed 2026-09-24 as
> quadraui#1063 (milestone #9), after re-checking the gap at vimcode's pin
> `3020d9e` and quadraui `develop`: `TuiDriver` still keeps its
> `Terminal<TestBackend>` private, sizes it once in `new`, and has no
> `resize`/`terminal()` accessor. The full draft text lives in that issue
> now. It blocks no open vimcode issue; it exists so a future resize/TOCTOU
> regression has a driver-tier repro path.
