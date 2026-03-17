# VimCode Project State

**Last updated:** Mar 16, 2026 (Session 191 — Subprocess stderr safety audit) | **Tests:** 4511

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 191 are in **SESSION_HISTORY.md**.

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

### Session 191 — Subprocess stderr safety audit (Mar 16, 2026)
- Audited all ~50 `Command::new()` call sites across the codebase
- Only one unsafe site found: `registry.rs` `download_script()` used `.status()` with inherited stderr — could corrupt TUI display during extension downloads
- Fixed by adding `.stdout(Stdio::null()).stderr(Stdio::null())` to the curl call
- All other sites already safe: `.output()` auto-captures both streams; `.spawn()` calls all have explicit `Stdio` redirection
- Breakdown: `git.rs` (~20 calls, all `.output()`), `engine.rs` (~8 calls, all redirected or `.output()`), `ai.rs` (3 curl `.output()`), `dap.rs`/`lsp.rs` (`.spawn()` with explicit piped/null), `dap_manager.rs`/`lsp_manager.rs` (all `.output()`)

### Session 190 — LSP Go-to-Definition Fix + Kitty Keyboard Fix (Mar 16, 2026)
- **Context menu mode-aware shortcuts**: Editor right-click context menu shows Vim shortcuts (`gd`, `gr`, `gD`) in Vim mode and VSCode shortcuts (`F12`, `Shift+F12`, `F2`) in VSCode mode.
- **Kitty terminal ":" fix**: `shift_map_us()` in TUI translates base key + SHIFT modifier to correct shifted character when Kitty's `REPORT_ALL_KEYS_AS_ESCAPE_CODES` keyboard enhancement sends `; + Shift` instead of `:`.
- **LSP `gd` "hanging" fix**: `DefinitionResponse`, `ImplementationResponse`, and `TypeDefinitionResponse` handlers now clear `self.message` after processing. Previously "Jumping to definition..." message persisted after successful jump, appearing as if the operation was still pending.
- **LSP response robustness**: Added `unwrap_or()` fallback for definition/hover responses (empty result instead of silent drop); string ID fallback for response parsing (some servers echo IDs as strings).
- LSP debug logging infrastructure (gated behind `VIMCODE_LSP_DEBUG` env var) retained in `lsp.rs` reader thread.

> Sessions 189 and earlier archived in **SESSION_HISTORY.md**.
