# VimCode Project State

**Last updated:** Apr 13, 2026 (Session 275 — Win-GUI h-scrollbar, bundled Nerd Font, Phase 2c verification) | **Tests:** 5495

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 272 are in **SESSION_HISTORY.md**.

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

**Session 275 — Win-GUI horizontal scrollbar, bundled Nerd Font, Phase 2c source verification:**

1. **Win-GUI horizontal scrollbar drag** — Full horizontal scrollbar implementation: drawing (track + thumb at bottom of editor), `h_scrollbar_hit()` hit-testing, click-to-jump, drag-to-scroll via `h_scrollbar_drag` state, mouse-up cleanup. Text rendering now applies `scroll_left` offset (was missing — scrollbar moved but text stayed put). Added text-area clip rect to prevent scrolled text bleeding over gutter. Cursor also offset by `scroll_left`.
2. **Win-GUI bundled Nerd Font via DirectWrite** — `install_bundled_icon_font_windows()` writes embedded 13KB `vimcode-icons.ttf` to `%LOCALAPPDATA%\Microsoft\Windows\Fonts\` (per-user, no admin). `register_user_font()` adds registry entry at `HKCU\...\Fonts`. `WM_FONTCHANGE` broadcast for same-session availability. `icon_text_format` now tries "Symbols Nerd Font" first, falls back to Segoe MDL2/Fluent. Activity bar and ext panel icons render native Nerd Font glyphs (using `icons::` constants matching GTK). Added `Win32_System_Registry` + `Win32_Security` Cargo features.
3. **Phase 2c source-code verification** — `test_wingui_source_contains_required_calls` reads Win-GUI source files and greps for the engine method calls required by each `UiAction` variant (26 checks). Automated bug-finder: fails if a new action's required engine call is missing from source. Uses `CARGO_MANIFEST_DIR` for stable paths. 1 new test.

> Sessions 274 and earlier in **SESSION_HISTORY.md**.
