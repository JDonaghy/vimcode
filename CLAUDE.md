# CLAUDE.md — vimcode

Agent-facing rules for **vimcode**. Re-read on every turn, so it holds only what a
diff can violate. Rationale, history and longer procedures live in
[`docs/AGENT_REFERENCE.md`](docs/AGENT_REFERENCE.md) — read a section only when your
task touches it.

**North star:** eliminate platform-specific code from vimcode and lift it into
quadraui ([`GOALS.md`](GOALS.md) — background for planning/triage; a single-issue
worker does not need to read it). The rule below stops *new* per-backend code.

## Platform-Neutrality Rule (MANDATORY — overrides all other guidance)

**NEVER add per-backend code to vimcode to fix a problem.** If a feature requires new code in `src/gtk/`, `src/macos/`, `src/win/` or `src/tui_main/` beyond thin event-to-engine wiring, STOP. Do not attempt the fix. Instead:

1. Identify what quadraui infrastructure is missing.
2. File a quadraui issue describing the gap.
3. Build the infrastructure in quadraui first.
4. Only then implement the vimcode side through the shared API.

**Issues and acceptance are platform-neutral too.** If quadraui works as advertised, every vimcode behaviour is the same on every backend, so:

- A vimcode issue's acceptance is a backend-neutral test (engine/`render.rs` unit test, or the TUI/headless driver), never "on macOS", "on Windows" or "verify on real hardware". Per-platform rendering and input are proven by quadraui's own conformance tests.
- **Test on one platform: `vcd` (TUI) on Linux.** Assume a change that passes there works on every backend. Your verify loop and the Test stage run `cargo test --no-default-features` (plus a narrow filter while iterating); the GTK lane is GitHub CI's job, not yours.
- A bug that shows up on only one backend is a quadraui gap or bug: the fix is a quadraui issue, and the vimcode side is at most a version bump. If your issue asks you to fix a single-backend symptom inside vimcode, say so in your final message and stop rather than adding backend code.
- Allowed per-platform differences are data, not code: e.g. default font per platform as a table the engine reads, and packaging/release plumbing.

**Push back actively.** If the user asks to implement something that would require per-backend code, say so upfront and propose the quadraui-first alternative.

**How to verify:** Before writing any code in a backend file, compare against the relevant quadraui example (`~/src/quadraui/quadraui/examples/`). If the example achieves the same feature with zero backend-specific code, your approach is wrong. Build the shared function in `render.rs` or the engine, have each backend call it in 1-3 lines of wiring.

**Negative example (#319, Session 353):** Menu dropdown keyboard nav was implemented with a GTK overlay DA + Msg dispatch + separate click/motion handlers (~100 lines GTK-specific) vs TUI inline handling (~50 lines TUI-specific). The quadraui `menu_bar_app` example does the same thing with ZERO backend-specific code — one `dropdown_layout()` function called by both backends, one `handle()` method for keyboard/mouse. Three attempts were made and reverted before recognising the architectural mistake.

**Never edit the quadraui repo directly.** The vimcode agent must not modify files under `~/src/quadraui/`. File a GitHub issue on `JDonaghy/quadraui` describing the gap, then wait for the user to confirm the quadraui change has landed.

## Codebase navigation — query the graph first

`graphify-out/` holds a knowledge graph of this repo. For "where is this handled /
what calls this" questions, query it (the `graphify` skill or CLI) before grep/Read.

## Background docs — on demand only

No reading chain is required at session start. `PROJECT_STATE.md`, `PLAN.md` and
`GOALS.md` are large (≈400 KB together) — do not read them unless the task needs
them. Load a reference doc only when its trigger applies: `docs/ARCHITECTURE.md`
(layout, engine submodule map), `docs/PATTERNS.md` (adding keys, commands,
settings, theme colors, clickable UI), `docs/QUADRAUI_GUIDE.md` (quadraui
migrations, paint↔click), `docs/IRREDUCIBLE_SURFACE.md` (platform-neutrality
planning). Full index: `docs/AGENT_REFERENCE.md`.

## quadraui is a crates.io dependency, not a sibling checkout (#1848)

vimcode depends on the **published `quadraui` crate** (`quadraui = { version =
"0.1.x", … }` in `Cargo.toml`, exact version locked in `Cargo.lock`).
`~/src/quadraui` is not consulted by a normal build, and a worktree contains no
quadraui source. **To read quadraui's code, use the registry copy of the locked
version: `~/.cargo/registry/src/*/quadraui-<version>/` (version from `Cargo.lock`)
— read-only; don't `find /` for it.**

- Patch release (0.1.x): `cargo update -p quadraui && cargo test`. Breaking release
  (0.2): a `version = "0.2"` edit in `Cargo.toml`.
- A quadraui fix vimcode needs must be **published** before vimcode depends on it.
  Co-develop with the git-ignored local override
  (`cp cargo-config-local-quadraui.toml.example .cargo/config.toml`, remove after) —
  **never a git or path dep in a committed `Cargo.toml`**.
- Don't "fix" vimcode to match a stale local quadraui checkout; if a build sees one,
  look for a stray `.cargo/config.toml`. `vimcode --version` prints the resolved
  quadraui version.

## Development workflow

- Work on a branch off `develop` (`issue-{number}-{short-description}`, or
  `{kind}-{short-description}` for non-issue work). Never commit code directly to
  `develop`; never push to `main`. PRs target `develop` ("Closes #N").
- Interactive (non-fleet) sessions: claim the issue, don't open a PR until the user
  has smoke-tested or waived it, then ask Path A (ff-merge + push) vs Path B (PR) —
  details in `docs/AGENT_REFERENCE.md`.
- Issues you file must be self-contained: file paths, API details, expected
  behaviour — and platform-neutral (see the rule above).

## Architecture

Vim-like code editor in Rust (GTK4, quadraui, Ropey, Tree-sitter, Pango+Cairo,
ratatui+crossterm). `src/core/` is platform-agnostic logic; `src/gtk/`,
`src/tui_main/`, `src/macos/`, `src/win/` are backends (thin wiring only — see the
Platform-Neutrality Rule); `src/main.rs` is a thin CLI dispatcher.

- **`src/core/` must NEVER depend on `gtk4`, `relm4` or `pangocairo`** — it must be
  testable in isolation.
- A change to a surface the backends render (mouse, drag, layout, click detection,
  rendering) must keep **every** backend working, and its tests cover them.

## Commands & quality checks

**Workers: run narrow, relevant tests — not the whole suite.** Before committing:
`cargo fmt`, `cargo clippy -- -D warnings`, and the tests for what you touched
(`cargo test <filter>` or `cargo test --test <file>`). The full suite is CI's and
the Test stage's job.

- Plain `cargo` (default features = `gui` on) compiles `src/gtk/`;
  `--no-default-features` never does, so **a green `--no-default-features` run says
  nothing about GTK code** — use the default features if you touched GTK/`render`.
- CI also runs `cargo fmt -- --check` and `cargo clippy --no-default-features --
  -D warnings` on the newest stable; a red lint-only CI check is usually a newer
  clippy — fix the lints, never pin the workflow to an older toolchain.
- Every test must pass with no `DISPLAY` set (GTK tests paint into in-memory Cairo
  surfaces). A test that genuinely needs a live display is `#[ignore]`d with a
  comment saying why.

## Code Style

- `rustfmt` defaults; `PascalCase` types, `snake_case` functions/vars.
- Core: return `Result<T, E>` for I/O, silent no-ops for bounds.
- Tests in `#[cfg(test)] mod tests` at file bottom.

## Testing — black-box coverage is the acceptance bar (MANDATORY)

**Every PR that changes user-visible behaviour must ship a black-box test that drives
the running app and asserts on its rendered output.** Pure refactors and
internal-only changes are exempt — **say so in the PR**. The adversarial reviewer
**rejects** behaviour-changing PRs without one.

| Backend | Driver | Where the test goes |
|---|---|---|
| TUI | quadraui `TuiDriver` via `quadraui::tui::testing::driver_with_shell(TuiShellApp, ...)` | in-crate in `src/tui_main/shell_app.rs`, `#[cfg(test)]` — reuse its fixtures (`app_with_sidebar_open`, …), follow the `render_content_paints_*_via_shell_app` tests |
| GTK | `GtkDriver` (`src/gtk/testing.rs`) | in-crate; paints into in-memory Cairo `ImageSurface`s, headless |

1. **Assert on rendered output — never on state being populated.** Locate targets
   with `find` / `screen_contains`, or probe pixels for icon glyphs — never hardcode
   coordinates.
2. **State in the PR that the new test fails against unfixed `develop`.** Remove the
   fix, re-run, confirm red, restore — a test that cannot fail is not coverage.
3. **A reproduction is not a fix.** A test-only issue (a red, `KNOWN_BUGS`-gated
   scenario, no code change) may not be closed until its follow-up fix issue
   exists, and that issue's number goes in the `KNOWN_BUGS` comment beside the label
   it gates.
4. **Never describe a `KNOWN_BUGS`-gated issue as fixed** — not in a PR body,
   release notes or status report. Release notes list every still-gated issue under
   **"Reproduced, not yet fixed"**.

**Sealed acceptance suite:** slices under `tests/acceptance/**` (driven via
`tests/acceptance.rs`) are authored by the test-author agent. Workers may *run*
them (`coord acceptance run --issue N`) but must never create, edit or delete them.

### Test-stage agents

The Test stage is black-box validation, not a redo of the Work stage: run the
routed command / pulled artifact (`coord pull-artifact <work_aid>`) you are given,
don't rebuild or run the full suite. A bug-fix branch fails Test unless it names
the black-box test covering the bug and states it was seen red on unfixed
`develop`; a `KNOWN_BUGS`-gated scenario is reproduced, not fixed. Record the verdict
with `coord test --passed <work_aid>` or `coord test --fail <work_aid> --reason
"<expected vs actual, steps, suspected files>"`. Full procedure:
`docs/AGENT_REFERENCE.md`.

## Branching & releases

`develop` is the integration branch; `main` is the release branch, updated only by
a `develop` → `main` PR. Never push directly to `main`. Release procedure
(version bump, flatpak `cargo-sources.json`, tagging): `docs/RELEASING.md` —
operator-facing; workers don't need it.
