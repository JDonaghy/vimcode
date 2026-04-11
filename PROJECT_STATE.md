# VimCode Project State

**Last updated:** Apr 11, 2026 (Session 269 — Win-GUI interaction parity: 19 fixes — features, bugs, systematic audit) | **Tests:** 5478

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 268 are in **SESSION_HISTORY.md**.

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

**Session 269 — Win-GUI interaction parity (19 fixes — features, bugs, systematic audit):**

**New features implemented:**
1. **Terminal regains focus** — Terminal content clicks now always set `terminal_has_focus` (was only for split-pane case).
2. **Breadcrumb clicks** — Clicking breadcrumb segments opens scoped picker (directory→file picker, symbol→@picker).
3. **Group divider drag** — Cached dividers from ScreenLayout; mouse-down starts drag, mouse-move resizes via `set_ratio_at_index()`. Cursor changes to resize arrow on hover.
4. **Diff toolbar button clicks** — ↑/↓/≡ buttons in tab bar call `jump_prev_hunk()`/`jump_next_hunk()`/`diff_toggle_hide_unchanged()`.
5. **Tab tooltip show/dismiss** — Mouse hover shows file path under the hovered tab; mouseout dismisses. Positioned under the hovered tab (not top-left corner).
6. **Terminal text selection** — Mouse drag in terminal creates/updates `TermSelection`; auto-copies to clipboard on release.
7. **Terminal paste (Ctrl+V)** — Reads clipboard (falls back to `+`/`"` registers), writes to PTY with bracketed paste.
8. **Terminal copy (Ctrl+Y / Ctrl+Shift+C)** — Copies terminal selection to clipboard.
9. **Extension panel keyboard routing** — `i` (install), `d` (delete), `u` (update), `r` (refresh), `/` (search), `j`/`k` (navigate), `Return` (readme), `q` (quit).
10. **Extension panel double-click** — Opens extension README markdown preview.
11. **Extension panel selection highlight** — Selected item gets background highlight.

**Bugs fixed:**
12. **Tab tooltip UNC prefix** — `strip_unc_prefix()` helper strips `\\?\` from Windows paths. Also applied to `copy_relative_path()`.
13. **Extension click geometry** — Click handler used integer rows but draw uses fractional Y (1.5×lh header, 0.3×lh gap). Rewrote to match draw math.
14. **Clipboard sync** — Added register→clipboard sync after yank and clipboard→register load before paste (`p`/`P`). Bidirectional clipboard=unnamedplus semantics.
15. **Generic sidebar handler swallowing keys** — Guarded with `active_panel == Explorer` so Git/AI/Search/Debug panels reach `handle_key()`.
16. **Tab slot bounds overflow** — Multi-group tab slots clipped to group bounds; prevents first group's overflow stealing second group's clicks.
17. **Context menu hover** — Added mouse-move tracking to highlight items on hover (was click-only).
18. **Tab close button / tab click geometry** — Tab slot width now uses `measure_ui_text_width()` (proportional UI font) matching `draw_tabs()`, instead of monospace `chars().count() * cw`.
19. **Menu bar hit-test font mismatch** — All 4 menu bar click/hover handlers now use proportional `measure_ui_text_width() + pad * 2.0` matching the draw code.

**Cross-platform fixes:**
- **`clipboard_paste()` on Windows** — Added `#[cfg(target_os = "windows")]` branch using PowerShell `Get-Clipboard`. Fixes Ctrl+V in command mode, search mode, and picker.
- **Ctrl+V in insert mode** — Pastes system clipboard instead of inserting literal character.
- **C++ extension** — Added `install_windows: "winget install LLVM.LLVM"`.
- **Mouse cursor** — I-beam over editor, arrow over UI chrome, resize near dividers.

**Documentation:**
- Updated `docs/NATIVE_GUI_LESSONS.md` with 3 new sections (10-12) and 15 new checklist items from this session's bug patterns. Sections cover interaction parity audit methodology, extension panel fractional layout, and terminal integration layers.

> All sessions through 268 archived in **SESSION_HISTORY.md**.
