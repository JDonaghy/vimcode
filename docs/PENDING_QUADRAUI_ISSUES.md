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
