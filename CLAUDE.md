## Current Goal — read first

**[`GOALS.md`](GOALS.md) holds the current north-star objective:** *eliminate all
platform-specific code from vimcode and lift it into quadraui.* It is meta-level
(above any single issue or session) and sequences the work — read it first, plan
against it, and keep it current. The **Platform-Neutrality Rule** below is the
operational rule that stops *new* per-backend code; `GOALS.md` tracks *deleting the
existing* per-backend code via milestone **#7 Platform-Neutral** (the vimcode-side
adoption of shipped quadraui APIs; #5 is the quadraui-build supply side).

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

## Codebase navigation — query the graph first

This repo ships a **graphify** knowledge graph in `graphify-out/` (`graph.json`,
`GRAPH_REPORT.md`), kept current automatically by `post-commit` / `post-checkout`
git hooks. For any architecture / "where is this handled" / "what calls this" /
file-relationship question, **query the graph first** (the `graphify` skill, or the
graphify CLI) before reaching for grep/Read. Grep/Read are for exact-string or
line-level confirmation — not the first move.

## Session Start Protocol
1. Read `PROJECT_STATE.md` for current progress
2. Read `PLAN.md` if present — pickup doc for in-flight multi-stage features
3. **If the work touches `quadraui/`** — read `docs/QUADRAUI_GUIDE.md` and quadraui repo's `DECISIONS.md` + `BACKEND_TRAIT_PROPOSAL.md` §9
4. **If navigating unfamiliar code** — read `docs/ARCHITECTURE.md` for directory layout, engine submodule map, and data model
5. Check `.opencode/specs/` for detailed feature specs before starting
6. Run `gh issue list --state open` to see active work and priorities
7. Prompt user to update `PROJECT_STATE.md` and `PLAN.md` after significant tasks

### Stale quadraui checkout — first thing to suspect on a build break

Vimcode depends on `quadraui` via a **path dependency** to the sibling `~/src/quadraui` checkout (see `Cargo.toml` line ~48), not a crates.io version. There is **no version pinning** — whatever is checked out at `~/src/quadraui` is what gets compiled.

If `cargo build` on a fresh `develop` (or any branch) fails with errors like "no variant named X found for enum Y", "no method named Z found", or "expected N args, found M" on a `quadraui::*` type, **the most likely cause is that your local quadraui checkout lags behind the API vimcode was written against** — not a vimcode bug.

Before debugging further:

```bash
cd ~/src/quadraui && git pull && cd -
cargo build
```

Only investigate the vimcode side if the error persists after pulling quadraui. Do not "fix" vimcode to match a stale quadraui — you'll just undo work that already shipped on the quadraui side.

## Conditional Reference Files

| File | Load when |
|------|-----------|
| `docs/ARCHITECTURE.md` | Working on code structure, adding files, navigating unfamiliar modules |
| `docs/QUADRAUI_GUIDE.md` | Quadraui migrations, cross-backend rendering, paint↔click integration |
| `docs/PATTERNS.md` | Adding new keys, commands, settings, theme colors, or clickable UI |
| `docs/DOC_MAINTENANCE.md` | After completing any feature — lists all files to update |
| `docs/COORDINATOR.md` | Designated as coordinator for multi-machine parallel work |

## Agent Roles

The default role is **developer** — read issues, write code, run tests, open PRs.

If the user designates you as **coordinator**, switch to planning mode: read `docs/COORDINATOR.md` and follow that protocol. Coordinators don't write code — they track work across machines, prevent file conflicts, and assign the next issue when an agent finishes.

## Development Workflow

All non-trivial work should be tracked via GitHub Issues.

**Documentation-only changes** (pure `.md` edits) may be committed directly to `develop` and pushed. No branch, no smoke test. If any code changes accompany the doc edit, use the full branch workflow.

**For all other changes:**

1. **Claim the issue before starting work.** Multiple agents may be active concurrently — claim publicly so nobody picks up the same issue. Run `gh issue edit <N> --add-assignee @me`, create the feature branch from `develop` (`issue-{number}-{short-description}`), and push it empty so it appears on the remote as the claim signal. Pushing an empty branch is NOT opening a PR.
2. **Work on that branch**, committing as you go. Never commit code directly to `develop`. For non-issue work, use `{kind}-{short-description}` naming and you may skip the claim step.
3. **Do NOT open a PR yet.** Keep the branch in "commits pushed, no PR" state until the user has run smoke tests or explicitly agreed testing is not needed. Subsequent pushes to the claim branch are fine.
4. **Once approved, ask the user which landing path:**
   - **Path A — merge locally + push.** For small/trivial changes: `git merge --ff-only <branch>`, push `develop`, delete the branch.
   - **Path B — open PR.** For normal feature/bugfix work: open a PR to `develop` against the already-pushed branch. Reference "Closes #{number}" if it closes an issue.
5. **When the user confirms a merge that closes an issue**, immediately `gh issue close <number>` and unassign yourself.

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
