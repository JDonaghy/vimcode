# Agent reference — vimcode

Reference material moved out of the repo-root `CLAUDE.md` so it is not re-read on
every agent turn. `CLAUDE.md` keeps the rules; this file keeps the rationale, the
history behind them and the longer procedures. **Read a section only when your
task touches it.**

## Background reading (on demand only)

The old "Session Start Protocol" chain (`PROJECT_STATE.md` ~260 KB, `PLAN.md`
~90 KB, `GOALS.md` ~55 KB) cost a 60k–100k-token detour whenever it was followed.
None of it is required to start a task. Open one only when the task needs it:

- `GOALS.md` — the north-star objective (eliminate platform-specific code from
  vimcode, lift it into quadraui); milestone **#7 Platform-Neutral** is the
  vimcode-side adoption of shipped quadraui APIs, #5 the quadraui-build supply
  side. Read when planning or triaging, not for a single issue.
- `PROJECT_STATE.md` — current progress log. `PLAN.md` — pickup doc for in-flight
  multi-stage features.
- `.opencode/specs/` — detailed feature specs, when your issue names one.
- quadraui's `quadraui/docs/decisions/DECISIONS.md` + `BACKEND_TRAIT_PROPOSAL.md` §9 — when
  designing a quadraui gap.


### Conditional reference files

| File | Load when |
|------|-----------|
| `docs/ARCHITECTURE.md` | Working on code structure, adding files, navigating unfamiliar modules |
| `docs/QUADRAUI_GUIDE.md` | Quadraui migrations, cross-backend rendering, paint↔click integration |
| `docs/PATTERNS.md` | Adding new keys, commands, settings, theme colors, or clickable UI |
| `docs/IRREDUCIBLE_SURFACE.md` | Planning platform-neutrality work — what genuinely stays per-backend, and why the rest is duplication not porting |
| `docs/DOC_MAINTENANCE.md` | After completing any feature — lists all files to update |
| `docs/COORDINATOR.md` | Designated as coordinator for multi-machine parallel work |

## Agent roles

The default role is **developer** — read issues, write code, run tests, open PRs.

If the user designates you as **coordinator**, switch to planning mode: read `docs/COORDINATOR.md` and follow that protocol. Coordinators don't write code — they track work across machines, prevent file conflicts, and assign the next issue when an agent finishes.

## Development workflow — interactive (non-fleet) detail

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

## "CI's `Test (Linux, headless)` is red but everything passes locally"

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

## Testing — why the rules exist

Rule 1 (assert on rendered output): `ScreenLayout.picker` was populated on GTK for
months while nothing painted it; the symptom read as an input bug and burned ~5
sessions before #587 found it was paint, and #592 then found 13 more fields in the
same state. A test asserting the field is `Some` passes against the bug.

Rule 2 (prove the test fails on unfixed `develop`): #553 shipped black-box tests
that stayed green with the bug reinstated.

Rule 3 (a `KNOWN_BUGS` entry is not a closed bug): the bidirectional gate makes a
still-broken bug report a *green* CI run. The v0.11.0 suite (#983, #984, #986,
#987, #988, #990) shipped 15 gated labels in v0.12.0, all six issues closed, zero
fix issues filed, and the bugs went out to a user who believed they were fixed.

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

## Coordinator Test stage — full procedure

The coordinator drives issues through `Work → Test → Review → Merge`. The **Test stage is a separate step from the work that built the branch — do NOT redo the worker's job:**
- **ALWAYS pull the prebuilt artifact** with `coord pull-artifact <work_aid>`. Do **NOT** run `cargo build` / `cargo test` yourself — the work-stage worker already compiled the binary and ran the full suite before finishing. Rebuilding or re-testing here **pins the CPU for zero new signal**.
- **Do NOT run the full test suite** (`cargo test`) at the Test stage. It already ran at the Work stage. The Test stage is **black-box behavior validation + user smoke**: drive the *pulled* binary, exercise the changed behavior end-to-end, and confirm it does what the issue asks.
- The "**MANDATORY before commits: run all four commands**" rule above is for the **work-stage worker authoring the change**, NOT for the test-stage agent.
- **For a bug-fix issue, the branch does not pass the Test stage without a named black-box test and a RED verification.** Check that the PR/branch names the test that covers the reported bug, and that it states the test was observed failing against unfixed `develop` (rule 2 above). If the branch ships a `KNOWN_BUGS`-gated scenario instead of a green one, the bug is **reproduced, not fixed** — `coord test --fail` it unless the issue was explicitly scoped test-only, and in that case confirm its follow-up fix issue exists before passing (rule 3 above).
- Record the verdict with `coord test --passed <work_aid>` or `coord test --fail <work_aid> --reason "<full repro: expected vs actual, steps, suspected files>"`.

## Releases (operator-facing)

Full runbook: `docs/RELEASING.md` (per-backend test lanes, which machine runs each,
expected-red failures, artifacts).

- All work happens on `develop`; `main` is the release branch
- Merge `develop` → `main` via GitHub PR (CI runs on the PR before release)
- Before creating the PR: bump version in `Cargo.toml`
- If `Cargo.lock` changed: regenerate `flatpak/cargo-sources.json` with `python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json`
- Merging the PR to `main` triggers `release.yml` which creates a GitHub Release tagged `v$VERSION`
- Never push directly to `main`
