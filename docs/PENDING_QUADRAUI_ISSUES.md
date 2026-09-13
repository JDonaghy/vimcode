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
