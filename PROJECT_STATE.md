# VimCode Project State

**Last updated:** Mar 25, 2026 (Session 217 — Engine Split Refactor) | **Tests:** 4721

> Feature documentation lives in **README.md**.
> Per-session implementation notes through Session 216 are in **SESSION_HISTORY.md**.

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

> Session 216 and earlier archived in **SESSION_HISTORY.md**.

- **Session 217** (Mar 25, 2026): **Engine Split Refactor** — Split monolithic `src/core/engine.rs` (51,825 lines) into `src/core/engine/` directory with 20 submodule files. `engine/mod.rs` (3,334 lines) contains types, structs, enums, Engine struct, `new()`, and free functions. Largest submodules: `tests.rs` (14,334), `keys.rs` (7,056), `motions.rs` (4,628). All 4,721 tests pass with zero changes to public API. No changes to `main.rs`, `tui_main.rs`, `render.rs`, `lib.rs`, or any non-engine files.

