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

### quadraui is a pinned git dependency, not a sibling checkout (#691)

Vimcode depends on `quadraui` via a **git dependency pinned to a `rev`** in `Cargo.toml` — `quadraui = { git = "https://github.com/JDonaghy/quadraui.git", rev = "<sha>", ... }`, and `[patch.crates-io] vt100` is pinned the same way. Cargo clones the pinned rev into `~/.cargo/git/` itself and locks the resolved SHA in `Cargo.lock`. **`~/src/quadraui` is not consulted by a normal build at all** — a plain `cargo build` is reproducible regardless of what's checked out there, including on a machine running several agents concurrently.

```bash
# Build against the pin (the normal case, and the only case for a plain checkout):
cargo build

# Bump the pin: edit `rev = "..."` in Cargo.toml (the quadraui dependency AND
# the [patch.crates-io] vt100 entry — they must match), then:
cargo test    # updates Cargo.lock and re-runs snapshots against the new rev

# Co-developing quadraui on a local branch? Opt in per-checkout, not per-env-var:
cp cargo-config-local-quadraui.toml.example .cargo/config.toml   # git-ignored
# ... edit ~/src/quadraui, rebuild ...
rm .cargo/config.toml   # back to the pinned rev
```

`cargo update -p quadraui` alone will **not** move a rev-pinned git dep — the `Cargo.toml` edit is the bump, and it is deliberately a reviewable one-line diff for the same reason `quadraui-pin.txt` used to be one (see `docs/QUADRAUI_GUIDE.md` for the #625/#638/#659 history this replaced).

`vimcode --version` / `vcd --version` print the resolved quadraui rev, so "which quadraui?" is answerable from any binary.

Do not "fix" vimcode to match a stale local quadraui checkout — with the git dep, "stale local checkout" can no longer affect a normal build in the first place; if you see it, check for a stray `.cargo/config.toml`.

## Conditional Reference Files

| File | Load when |
|------|-----------|
| `docs/ARCHITECTURE.md` | Working on code structure, adding files, navigating unfamiliar modules |
| `docs/QUADRAUI_GUIDE.md` | Quadraui migrations, cross-backend rendering, paint↔click integration |
| `docs/PATTERNS.md` | Adding new keys, commands, settings, theme colors, or clickable UI |
| `docs/IRREDUCIBLE_SURFACE.md` | Planning platform-neutrality work — what genuinely stays per-backend, and why the rest is duplication not porting |
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
   - **Path A — merge locally + push.** For small/trivial changes: `git merge --ff-only <branch>`, push `develop`, delete the branch. Still available, but see **Branch protection** below — it is now an admin-only escape hatch that skips the CI gate, not the default.
   - **Path B — open PR.** For normal feature/bugfix work: open a PR to `develop` against the already-pushed branch. Reference "Closes #{number}" if it closes an issue.
5. **When the user confirms a merge that closes an issue**, immediately `gh issue close <number>` and unassign yourself.

**Creating issues:** Include full design context in the body — file paths, API details, expected behavior. Issues should be self-contained so a new session can pick one up.

## Architecture

**VimCode**: Vim-like code editor in Rust. Clean separation: `src/core/` (platform-agnostic logic) vs `src/gtk/` (GTK UI) vs `src/tui_main/` (TUI). `src/main.rs` is a thin CLI dispatcher. A native Windows backend will be re-added as a thin wrapper when the quadraui Win backend ships (quadraui#19–#31).

**Tech Stack:** Rust 2021, GTK4 (Relm4 removed in #540), quadraui, Ropey, Tree-sitter, Pango+Cairo, ratatui+crossterm

**Critical Rule:** `src/core/` must NEVER depend on `gtk4`, `relm4`, or `pangocairo`. Must be testable in isolation.

**Multi-backend rule:** TWO UI backends (GTK, TUI). When fixing bugs or adding features that touch mouse handling, drag, layout, click detection, or rendering — check and update BOTH backends. See `docs/ARCHITECTURE.md` for directory layout and engine submodule map.

## Commands & Quality Checks

```bash
cargo build                       # Compile (GUI on — needs GTK4 dev libs)
cargo test                        # Run all tests, BOTH backends (see Testing)
cargo clippy -- -D warnings       # Lint (must pass)
cargo fmt                         # Format
```

**MANDATORY before commits:** Run all four commands above. If any fails, fix and re-run. `cargo test --no-default-features --lib` is faster for dev loops, but plain `cargo test` (GUI-on) is the pre-commit gate — the `--no-default-features` variant never compiles `src/gtk/` and cannot catch a GTK regression (#645).

### "CI's `Test (Linux, headless)` is red but everything passes locally"

That job is the **only** one that runs `cargo fmt -- --check` and
`cargo clippy --no-default-features -- -D warnings` (the GUI job runs `cargo test`
alone), so a *lint or formatting* failure shows up as exactly one red check and
zero red tests. Before hunting for a phantom test regression, check the
toolchain: CI uses `dtolnay/rust-toolchain@stable`, i.e. **whatever stable is
newest on the day the job runs**, while your machine is on whatever you last
installed. Every six weeks a new clippy adds lints that turn pre-existing,
previously-clean code into `-D warnings` errors.

```bash
rustup check                                    # is CI's stable newer than yours?
rustup toolchain install <newer> --component clippy,rustfmt --profile minimal
cargo +<newer> fmt -- --check
cargo +<newer> clippy --no-default-features -- -D warnings
```

Fix the lints (they are real, just newly reported) — do **not** pin the workflow
to an old toolchain to make the check go green. Verify the fix still compiles on
the older stable too, so you don't accidentally raise the MSRV.

## Code Style
- `rustfmt` defaults (4-space indent)
- `PascalCase` types, `snake_case` functions/vars
- Core: Return `Result<T, E>` for I/O, silent no-ops for bounds
- Tests in `#[cfg(test)] mod tests` at file bottom

## Testing (CRITICAL)

### Black-box coverage is the acceptance bar (MANDATORY)

**Every PR that changes user-visible behaviour must ship a black-box test that drives the
running app and asserts on its rendered output.** Both backends have a driver — there is no
"no harness here" excuse:

| Backend | Driver | Where the test goes |
|---|---|---|
| TUI | quadraui `TuiDriver` via `quadraui::tui::testing::driver_with_shell(TuiShellApp, ...)` | **in-crate** in `src/tui_main/shell_app.rs`, `#[cfg(test)]` — reuse the local fixtures there (`app_with_sidebar_open`, `app_with_ext_panel`, …) and follow the existing `render_content_paints_*_via_shell_app` tests |
| GTK | `GtkDriver` (`src/gtk/testing.rs`, harness from #646) | in-crate; paints into in-memory Cairo `ImageSurface`s, headless |

Pure refactors and internal-only changes are exempt — **say so in the PR** if that applies.
The adversarial reviewer reads this file and **rejects** behaviour-changing PRs that lack one.

**Two rules that exist because they were learned the expensive way:**

1. **Assert on rendered output — never on state being populated.** `ScreenLayout.picker` was
   populated on GTK for months while nothing painted it; the symptom read as an input bug and
   burned ~5 sessions before #587 found it was paint, and #592 then found 13 more fields in the
   same state. A test asserting the field is `Some` passes against the bug. Locate targets with
   `find` / `screen_contains`, or probe pixels when the content is icon glyphs (#555) — never
   hardcode coordinates.
2. **State in the PR that the new test fails against unfixed `develop`.** #553 shipped
   black-box tests that stayed green with the bug reinstated. A test that cannot fail is not
   coverage. Remove the fix, re-run, confirm red, restore — then say so in the PR.

If the change touches a surface both backends render, the multi-backend rule above applies to
the tests too: cover both.

- **Full test suite:** `cargo test` (default features, GUI on) — lib + integration tests + the `vimcode` bin's GTK/render unit tests
- **Fast dev iteration:** `cargo test --no-default-features --lib` — lib tests only

### Test lanes: which command covers which backend (#645)

| Command | Compiles | Covers |
|---------|----------|--------|
| `cargo test` (default = `gui` on) | everything: lib, `vcd`, all integration tests, **plus** the `vimcode` bin (`src/gtk/`, bin-side `render`) | **both backends** — strict superset of the TUI lane |
| `cargo test --no-default-features` | lib, `vcd`, integration tests only — `src/gtk/` is **never compiled** | TUI/core only |

- The two lanes compile *identical* code for every shared target — no
  `cfg(feature = "gui")` exists outside the `vimcode` bin target — so the GUI
  lane is a strict superset of the TUI lane's test coverage. The TUI lane's
  only unique value is compile-hygiene: proving vimcode still builds on a
  machine without GTK dev libs (CI keeps a `--no-default-features` job for
  exactly that).
- **A green `--no-default-features` run says NOTHING about GTK code.** Reading
  it as cross-backend coverage is the misread #645 exists to prevent: the Test
  stage reported `passed` on GTK bug fixes whose GTK code it never compiled.
- The GUI lane runs **headlessly** — no `DISPLAY` or `WAYLAND_DISPLAY` needed.
  The GTK tests paint into in-memory Cairo `ImageSurface`s (the quadraui#301
  `GtkDriver` pattern); nothing calls `gtk::init`.
- **Display policy:** every test must pass with no `DISPLAY` set. Any future
  test that genuinely needs a live display must be `#[ignore]`-gated with a
  comment saying why. As of #645 there are none.
- Coordinator Test-stage recommendation (measured on a 20-core machine, warm
  shared dependency cache): fresh-worktree `cargo test` ≈ 50s vs 34s for the
  TUI lane; incremental after a core edit ≈ 19s vs 15s. The GUI lane's extra
  cost is small and it subsumes the TUI lane's tests, so the recommended
  `test_command` is **`cargo test`** (one lane; CI covers no-GTK build
  hygiene).

### Coordinator pipeline: the **Test stage** (read this if you are a smoke / test-stage agent)
The coordinator drives issues through `Work → Test → Review → Merge`. The **Test stage is a separate step from the work that built the branch — do NOT redo the worker's job:**
- **ALWAYS pull the prebuilt artifact** with `coord pull-artifact <work_aid>`. Do **NOT** run `cargo build` / `cargo test` yourself — the work-stage worker already compiled the binary and ran the full suite before finishing. Rebuilding or re-testing here **pins the CPU for zero new signal**.
- **Do NOT run the full test suite** (`cargo test`) at the Test stage. It already ran at the Work stage. The Test stage is **black-box behavior validation + user smoke**: drive the *pulled* binary, exercise the changed behavior end-to-end, and confirm it does what the issue asks.
- The "**MANDATORY before commits: run all four commands**" rule above is for the **work-stage worker authoring the change**, NOT for the test-stage agent.
- Record the verdict with `coord test --passed <work_aid>` or `coord test --fail <work_aid> --reason "<full repro: expected vs actual, steps, suspected files>"`.

## Branching & Releases
- All work happens on `develop`; `main` is the release branch
- Merge `develop` → `main` via GitHub PR (CI runs on the PR before release)
- Before creating the PR: bump version in `Cargo.toml`
- If `Cargo.lock` changed: regenerate `flatpak/cargo-sources.json` with `python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json`
- Merging the PR to `main` triggers `release.yml` which creates a GitHub Release tagged `v$VERSION`
- Never push directly to `main`

### Branch protection — CI is a gate, not a suggestion (#796)

Both CI jobs are **required status checks** on `develop` and `main`. Before #796
they were advisory: `develop` (the default branch, where every agent branch
lands) had no protection object at all and `main` had one with no
`required_status_checks` key, so a PR with red checks was mergeable by clicking
the button. That mattered here more than usual — multiple concurrent agents
land branches, and a stale or partial local run (`--no-default-features`, see
#645) is exactly what the gate is meant to catch.

The settings are versioned in **`.github/branch-protection.json`** and applied
by **`scripts/apply-branch-protection.sh`** rather than living only in the repo
web UI, so they are reviewable in a diff and testable:

```bash
scripts/apply-branch-protection.sh --dry-run   # print the exact API payloads (offline)
scripts/apply-branch-protection.sh --check     # audit live settings, report drift, no writes
scripts/apply-branch-protection.sh             # apply, then read back and verify
```

Applying needs an authenticated `gh` with **admin** rights on the repo — an
agent cannot do it; the owner runs it.

- `main` uses `"strict": true` — must be up to date with base before merging.
  It only ever receives the `develop` → `main` release PR, so that costs nothing.
- `develop` uses `"strict": false` — with strict on, every open branch would need
  a rebase and a full re-run each time another one landed, serialising the queue.
- `enforce_admins: false` (#796 decision (c)) — the gate binds pull requests and
  non-admin pushes; the owner keeps a deliberate escape hatch. **Path A (merge
  locally + push `develop`) therefore still works for the owner, but it bypasses
  the gate** — CI only fires after the code has landed. Prefer Path B; use Path A
  only when you mean to.

**If you rename a CI job, update `.github/branch-protection.json` in the same
commit.** GitHub matches required checks by name: a required context naming a
job that no longer reports stays forever-pending and blocks *every* PR.
`tests/branch_protection.rs` asserts the two lists are equal, so a rename fails
the suite instead of wedging the queue. Any new CI job that should gate merges
goes in that file too.
