# VimCode Project State

**Last updated:** Apr 13, 2026 (Session 273 — Windows LSP fix, extension install fixes, Win-GUI hover) | **Tests:** 5478

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

**Session 273 — Windows LSP fix, extension install fixes, Win-GUI hover:**

**Critical LSP fix (Windows):**
1. **`path_to_uri` broken on Windows** — Was producing `file://C:\path` (backslashes, two slashes) instead of RFC 3986 `file:///C:/path` (forward slashes, three slashes). rust-analyzer couldn't match hover/definition/diagnostic requests to known files, making all LSP features non-functional on Windows. Fixed URI generation and `uri_to_path` parsing.

**Win-GUI hover popups:**
2. **`editor_hover_mouse_move()` never called** — Added in `on_mouse_move` for editor area dwell tracking.
3. **`poll_editor_hover()` never called** — Added in `on_tick` to fire the dwell timer and send LSP hover requests.

**Extension install on Windows:**
4. **Win-GUI never consumed `pending_terminal_command`** — Extension install terminal never opened. Added check in Win-GUI tick loop.
5. **`terminal_run_command` used bash syntax on PowerShell** — Added PowerShell wrapper (`-Command`, `$LASTEXITCODE`, `Read-Host`) alongside existing bash wrapper.
6. **`&&` invalid in PowerShell 5.x** — Changed install command separator to `;` (works in both bash and PowerShell).
7. **Broken rustup proxy detection** — `binary_on_path` found proxy exe but it didn't work. Added `cargo_bin_probe_ok()` that runs `--version` to validate binaries in `~/.cargo/bin/`.
8. **Rust extension install command** — Overridden on Windows from `cargo install rust-analyzer` to `rustup component add rust-analyzer`. Updated remote registry (`vimcode-ext`).
9. **Install spinner never cleared** — `finalize_install_from_terminal` now calls `notify_done_by_kind`.
10. **`lsp_did_open` called before install completes** — Skipped when install is pending.

> Sessions 272 and earlier in **SESSION_HISTORY.md**.
