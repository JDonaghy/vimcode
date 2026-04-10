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

## Known Gaps (Not Expected to Work Yet)

- **Mouse handlers for new popups** — The 6 new renderers draw correctly but clicking/scrolling/dismissing them with the mouse won't work yet. Keyboard dismiss (Escape, `q`) should work where the engine handles it.
- **Tab drag-and-drop** — Tabs cannot be reordered or moved between groups by dragging.
- **Terminal split** — No horizontal terminal split button or drag handler.
- **Scrollbar visibility** — May have color/contrast issues. Code exists but rendering may be invisible.
