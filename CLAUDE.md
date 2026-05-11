## Platform-Neutrality Rule (MANDATORY — overrides all other guidance)

**NEVER add per-backend code to vimcode to fix a problem.** If a feature requires new code in `src/gtk/` or `src/tui_main/` beyond thin event-to-engine wiring, STOP. Do not attempt the fix. Instead:

1. Identify what quadraui infrastructure is missing.
2. File a quadraui issue describing the gap.
3. Build the infrastructure in quadraui first.
4. Only then implement the vimcode side through the shared API.

**Push back actively.** If the user asks to implement something that would require per-backend code, say so upfront and propose the quadraui-first alternative.

**How to verify:** Before writing any code in a backend file, compare against the relevant quadraui example (`~/src/quadraui/quadraui/examples/`). If the example achieves the same feature with zero backend-specific code, your approach is wrong. Build the shared function in `render.rs` or the engine, have each backend call it in 1-3 lines of wiring.

**Negative example (#319, Session 353):** Menu dropdown keyboard nav was implemented with a GTK overlay DA + Msg dispatch + separate click/motion handlers (~100 lines GTK-specific) vs TUI inline handling (~50 lines TUI-specific). The quadraui `menu_bar_app` example does the same thing with ZERO backend-specific code — one `dropdown_layout()` function called by both backends, one `handle()` method for keyboard/mouse. Three attempts were made and reverted before recognising the architectural mistake.

**Never edit the quadraui repo directly.** The vimcode agent must not modify files under `~/src/quadraui/`. File a GitHub issue on `JDonaghy/quadraui` describing the gap, then wait for the user to confirm the quadraui change has landed.

## Session Start Protocol
1. Read `PROJECT_STATE.md` for current progress
2. Read `PLAN.md` if present — pickup doc for in-flight multi-stage features
3. **If the work touches `quadraui/`** — read `docs/QUADRAUI_GUIDE.md` and quadraui repo's `DECISIONS.md` + `BACKEND_TRAIT_PROPOSAL.md` §9
4. **If navigating unfamiliar code** — read `docs/ARCHITECTURE.md` for directory layout, engine submodule map, and data model
5. Check `.opencode/specs/` for detailed feature specs before starting
6. Run `gh issue list --state open` to see active work and priorities
7. Prompt user to update `PROJECT_STATE.md` and `PLAN.md` after significant tasks

## Conditional Reference Files

| File | Load when |
|------|-----------|
| `docs/ARCHITECTURE.md` | Working on code structure, adding files, navigating unfamiliar modules |
| `docs/QUADRAUI_GUIDE.md` | Quadraui migrations, cross-backend rendering, paint↔click integration |
| `docs/PATTERNS.md` | Adding new keys, commands, settings, theme colors, or clickable UI |
| `docs/DOC_MAINTENANCE.md` | After completing any feature — lists all files to update |

## Development Workflow

All non-trivial work should be tracked via GitHub Issues.

**Documentation-only changes** (pure `.md` edits) may be committed directly to `develop` and pushed. No branch, no smoke test. If any code changes accompany the doc edit, use the full branch workflow.

**For all other changes:**

1. **Always work on a local branch off `develop`.** Never commit code directly to `develop`. Branch naming: `issue-{number}-{short-description}` or `{kind}-{short-description}`.
2. Do the work on that branch, committing as you go.
3. **Do NOT push the branch yet.** Keep it local until the user has run smoke tests or explicitly agreed testing is not needed.
4. **Once approved, ask the user which landing path:**
   - **Path A — merge locally + push.** For small/trivial changes: `git merge --ff-only <branch>`, push `develop`, delete the branch.
   - **Path B — push branch + open PR.** For normal feature/bugfix work: push the branch, open a PR to `develop`. Reference "Closes #{number}" if it closes an issue.
5. **When the user confirms a merge that closes an issue**, immediately `gh issue close <number>`.

**Creating issues:** Include full design context in the body — file paths, API details, expected behavior. Issues should be self-contained so a new session can pick one up.

## Architecture

**VimCode**: Vim-like code editor in Rust. Clean separation: `src/core/` (platform-agnostic logic) vs `src/gtk/` (GTK UI) vs `src/tui_main/` (TUI). `src/main.rs` is a thin CLI dispatcher. A native Windows backend will be re-added as a thin wrapper when the quadraui Win backend ships (quadraui#19–#31).

**Tech Stack:** Rust 2021, GTK4+Relm4, Ropey, Tree-sitter, Pango+Cairo, ratatui+crossterm

**Critical Rule:** `src/core/` must NEVER depend on `gtk4`, `relm4`, or `pangocairo`. Must be testable in isolation.

**Multi-backend rule:** TWO UI backends (GTK, TUI). When fixing bugs or adding features that touch mouse handling, drag, layout, click detection, or rendering — check and update BOTH backends. See `docs/ARCHITECTURE.md` for directory layout and engine submodule map.

## Commands & Quality Checks

```bash
cargo build                       # Compile
cargo test --no-default-features  # Run all tests
cargo clippy -- -D warnings       # Lint (must pass)
cargo fmt                         # Format
```

**MANDATORY before commits:** Run all four commands above. If any fails, fix and re-run. `cargo test --no-default-features --lib` is faster for dev loops but the full suite is the pre-commit gate.

## Code Style
- `rustfmt` defaults (4-space indent)
- `PascalCase` types, `snake_case` functions/vars
- Core: Return `Result<T, E>` for I/O, silent no-ops for bounds
- Tests in `#[cfg(test)] mod tests` at file bottom

## Testing (CRITICAL)
- **Full test suite:** `cargo test --no-default-features` — lib + integration tests
- **Fast dev iteration:** `cargo test --no-default-features --lib` — lib tests only

## Branching & Releases
- All work happens on `develop`; `main` is the release branch
- Merge `develop` → `main` via GitHub PR (CI runs on the PR before release)
- Before creating the PR: bump version in `Cargo.toml`
- If `Cargo.lock` changed: regenerate `flatpak/cargo-sources.json` with `python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json`
- Merging the PR to `main` triggers `release.yml` which creates a GitHub Release tagged `v$VERSION`
- Never push directly to `main`
