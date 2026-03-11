# VimCode Project State

**Last updated:** Mar 11, 2026 (Session 168 — Keybinding Discoverability + VSCode Remapping) | **Tests:** 4088

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 167 are in **SESSION_HISTORY.md**.

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

**Session 168 — Keybinding Discoverability + VSCode Remapping (4088 tests):**
Made keybinding remapping discoverable and enabled it in VSCode mode. Added 7 new ex command aliases (`:hover`, `:LspImpl`, `:LspTypedef`, `:nextdiag`, `:prevdiag`, `:nexthunk`, `:prevhunk`) so every remappable keybinding has a named command. Updated `:Keybindings` reference (both Vim and VSCode) to show command names alongside bindings (e.g., `gd → :def`, `F12 → :def`, `Ctrl+P → :fuzzy`) with a remapping hint. Added 12 commands to `available_commands()` for tab completion. Enabled `:map` remapping in VSCode mode — `handle_vscode_key()` now checks `try_user_keymap()` before built-in handlers; mode `"n"` keymaps apply. Added "Open Keyboard Shortcuts" to command palette so VSCode users can F1 → remap keys. Updated `:Keymaps` help text to mention VSCode mode. Fixed pre-existing test hermiticity bug: `engine_with()` now resets `mode` to Normal and rebuilds `user_keymaps` (was leaking disk settings into tests). 17 new tests in `tests/wincmd.rs` (40 total). README updated with discoverability instructions.

> Sessions 167 and earlier archived in **SESSION_HISTORY.md**.
