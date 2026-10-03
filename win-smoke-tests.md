# Win-GUI Smoke Tests

Run these after pulling `develop` and building with `cargo build --features win-gui`.

## Session 265 — New Renderers

- [ ] **Editor hover popup** — Open a Rust file, position cursor on a symbol, press `gh`. Should show a rich markdown popup with syntax-highlighted code blocks, headings, and links.
- [ ] **Diff peek popup** — Edit a git-tracked file, save, then press `gD` on a changed line. Should show an inline diff popup with green (+) and red (-) lines, plus an action bar at the bottom: `[s] Stage  [r] Revert  [q] Close`.
- [ ] **Debug toolbar** — Set a breakpoint (F9) and start debugging (F5). A toolbar strip should appear with Continue/Pause/Step Over/Step In/Step Out/Restart/Stop buttons and key hints.
- [ ] **Diff toolbar in tab bar** — Open a diff view (`:Gdiff` or click a changed file in the Source Control panel). The tab bar should show ↑↓≡ buttons at the right edge with a change counter (e.g. "2 of 5").
- [ ] **Tab tooltip** — Hover the mouse over any tab. A tooltip should appear just below the tab bar showing the full file path (with `~/` shortening).
- [ ] **Panel hover popup** — Open the Source Control panel (click the branch icon in the activity bar). Hover over a log entry or changed file. Should show a markdown hover card with commit info or diff stats.

## Positioning & Clipping Checks

- [ ] **Hover near right edge** — Trigger `gh` hover on a symbol near the right edge of the window. Popup should clamp to stay within the window bounds.
- [ ] **Hover near top edge** — Trigger `gh` hover on the first visible line. Popup should appear below the cursor instead of above.
- [ ] **Diff peek on last visible line** — Press `gD` on the last visible line. Popup should not overflow below the window.
- [ ] **Tab tooltip with long path** — Open a deeply nested file. Tooltip text should not overflow past the window edge.

## Previously Fixed (Session 264) — Regression Check

- [ ] **Settings button** — Gear icon visible at bottom of activity bar. Click opens Settings panel.
- [ ] **Tab bar clicks** — Clicking tabs switches between them correctly.
- [ ] **Status bar clicks** — Click Ln:Col → go-to-line, click filetype → language picker, click branch → branch picker.
- [ ] **Context menus** — Right-click in explorer shows context menu. Right-click on tab bar shows tab context menu.
- [ ] **Preview tabs** — Single-click in explorer opens dimmed preview tab. Double-click opens permanent tab.
- [ ] **Terminal resize** — Drag the terminal panel header to resize. Height persists.

## Open real-hardware questions (dell64) — #1691

**[#1691](https://github.com/JDonaghy/vimcode/issues/1691) — Win-GUI editor paints no
line-number gutter, although #1543 made `number` the default.** This issue is **open and
unfixed**: the reported symptom (buffer text flush against the editor pane's left edge, no
line numbers, no inset) has never been reproduced from source, and no production code change
has been made for it. Do **not** treat it as resolved.

What *is* now proven, by tests that execute on every host (`cargo test --no-default-features
--features win --lib win_gutter_contract_1691`, in `src/win/mod.rs`):

- `render::calculate_gutter_cols` + `render::build_rendered_window` give a default-settings
  engine a multi-cell gutter whose first line carries the digit `1`.
- `render::to_q_editor` + `quadraui::Editor::layout` — the exact pair
  `quadraui::win::editor::draw_editor` calls — then inset `text_bounds.x` past it by exactly
  that many cells, and hand back a non-empty `gutter_bounds`.

Both were RED-verified by injecting the reported defect at the two sites the issue names. So
if the bug reproduces on real hardware, the cause is **not** in either of those rungs.

That leaves exactly two things to check on dell64, in this order:

- [ ] **Confirm the live `number` option first.** In the reproducing session run `:set
      number?` and `:verbose set number?`, and print `~/.config/vimcode/settings.json` (or
      the Windows equivalent) looking for a `"line_numbers"` key. Also run `vimcode.exe
      --version` and confirm the build postdates #1543. A stale binary or a persisted
      `nonumber` override reproduces the report exactly — see
      `nonumber_collapses_the_gutter_and_leaves_text_nearly_flush_control_1691`, which pins
      that geometry (1-cell gutter, no digits) for comparison. **If this is the cause, #1691
      is not a paint bug at all** and should be re-scoped.
- [ ] **Only if `number` is confirmed on and the build is current:** run the gated pixel
      probe `win_driver_tests::line_number_gutter_paints_and_insets_text_by_default_1691`
      (`cargo xwin test --no-default-features --features win --lib`, recipe in
      `src/win/mod.rs`'s #1558 section). A failure there localises the bug to
      `quadraui::win::editor::draw_editor`'s Direct2D/DirectWrite draw calls — the one rung
      no non-Windows host can execute — which is a **quadraui** gap to be drafted into
      `docs/PENDING_QUADRAUI_ISSUES.md` and filed, per CLAUDE.md's Platform-Neutrality Rule.
      Capture a screenshot of the editor pane either way.

## Open real-hardware questions (dell64) — #1695

**[#1695](https://github.com/JDonaghy/vimcode/issues/1695) — Explorer sidebar scrollbar is
a wide, always-visible bar with a doubled thumb, instead of VS Code's thin auto-hiding
overlay.** This issue is **open and unfixed**: no vimcode-side production code change has
been made for it, and none is available — the root cause is entirely inside quadraui's
`TreeController`/`primitives::tree`/`primitives::scrollbar` (see `docs/
PENDING_QUADRAUI_ISSUES.md`'s new entry and `src/win/mod.rs`'s `#1695` doc section for the
full write-up). Do **not** treat it as resolved, and the stale "Scrollbar visibility" bullet
under "Known Gaps" below should be read as superseded by this section, not as a separate,
still-open question.

What *is* now proven, by tests that execute on this host today (`cargo test
--features gui --lib scrollbar_paint::explorer_sidebar_scrollbar`, in
`src/gtk/testing.rs`) — on GTK, which shares every line of the implicated quadraui code with
Win-GUI:

- The scrollbar paints unconditionally, with no hover event synthesized — there is no
  hidden-at-rest state anywhere in the implicated code (`explorer_sidebar_scrollbar_paints_
  unconditionally_at_rest_1695`).
- A second, phantom scrollbar-shaped band paints immediately left of the real one, at the
  width the root-cause write-up predicts — a genuine double-paint, not intended thumb-over-
  track compositing (`explorer_sidebar_scrollbar_double_paints_an_adjacent_phantom_band_
  1695`).

Both are characterizations of *today's* upstream behaviour (expected to start failing once
quadraui's fix lands and the pin bumps), not a fix. What they rule out on dell64: a Win-GUI-
specific divergence from GTK. What's left to check once quadraui ships a fix and the pin is
bumped:

- [ ] **Re-run the two GTK tests above** to confirm they now fail (the tripwire firing), then
      delete them per their own doc comments.
- [ ] **Capture a fresh screenshot of the Explorer sidebar on dell64**, scrollable but
      unhovered, and confirm no scrollbar-coloured pixels appear at the panel's right edge —
      then hover/scroll and confirm a single, thin (~10px) overlay fades in with one thumb,
      not two adjacent bands.

## Known Gaps (Not Expected to Work Yet)

- **Mouse handlers for new popups** — The 6 new renderers draw correctly but clicking/scrolling/dismissing them with the mouse won't work yet. Keyboard dismiss (Escape, `q`) should work where the engine handles it.
- **Tab drag-and-drop** — Tabs cannot be reordered or moved between groups by dragging.
- **Terminal split** — No horizontal terminal split button or drag handler.
- **Scrollbar visibility** — Superseded by "Open real-hardware questions (dell64) — #1695"
  above: the sidebar scrollbar is not invisible, it is the opposite (always-visible, doubled)
  — see that section for the real root cause.
