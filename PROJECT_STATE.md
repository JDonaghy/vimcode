# VimCode Project State

**Last updated:** Mar 10, 2026 (Session 161 — Terminal install + F1 palette, v0.3.2) | **Tests:** 3995

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 159 are in **SESSION_HISTORY.md**.

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

**Session 161 — Terminal install + F1 palette (3995 tests):**
Extension install scripts now run in a visible terminal pane (TerminalPane::new_command) instead of silently in the background — users see real-time output, errors, and can enter sudo passwords. InstallContext struct tracks extension name/install key for post-install LSP/DAP registration. EngineAction::RunInTerminal bridges engine→UI. F1 opens Command Palette in both Vim and VSCode modes (fixes Ctrl+Shift+P not working in many terminals). 3 new extension install tests.

**Session 160 — Extensions UX + workspace isolation + word wrap (3992 tests):**
Extension sidebar UX overhaul: Enter shows README preview for any extension (installed or available), `i` key installs (was Enter). Double-click in TUI Explorer fixed (last_click_time/pos updated at all click sites). Word-boundary wrapping (`compute_word_wrap_segments()` in render.rs). Workspace session isolation fix (global session `open_files` cleared to prevent cross-workspace bleed). LSP kickstart after extension install (`lsp_did_open` called on active buffer). LSP args fix (`InstallComplete` handler uses manifest args instead of empty vec). Bicep LSP install command rewritten (curl+unzip from Azure/bicep GitHub releases, not NuGet). Removed commentary Lua extension (native `:Comment` replaces it). All 16 extension READMEs rewritten with prerequisites and auto-install info. New `EXTENSIONS.md` extension development guide.

> Sessions 159 and earlier archived in **SESSION_HISTORY.md**.
