# Releasing VimCode

Cutting a release has two halves: **prove every architecture still works** (§1 — the
gate), then **ship** (§2 — the mechanics). The gate is a prerequisite, not a
formality: the shipped artifacts cover four backends across three operating systems,
and the per-PR CI covers exactly one of them.

> **Scope.** This is the operator's runbook. Workers and reviewers don't need it —
> `CLAUDE.md`'s *Branching & Releases* section is the summary, and it points here.

---

## 0. What a release actually ships

`.github/workflows/release.yml` produces, on a push to `main`:

| Artifact | Built on | Backend | Ships in v0.13.0? |
|---|---|---|---|
| `vimcode-linux-x86_64`, `vimcode_*.deb` | ubuntu-24.04 | GTK4 (glibc) | **yes** |
| `vcd-linux-x86_64` | ubuntu-24.04 | TUI (musl, static) | **yes** |
| `vimcode-macos-arm64.tar.gz`, `vcd-macos-arm64.tar.gz` | macos-latest | GTK4 via Homebrew / TUI | **yes** — `RELEASE_MACOS=true` |
| `vcd-windows-x86_64.exe` | windows-latest | TUI | **yes** — `RELEASE_WINDOWS=true` |
| `vimcode.flatpak` | ubuntu-24.04 | GTK4 | no — `RELEASE_FLATPAK` unset, and **broken**, see §2.2 |

Each non-Linux job carries `if: ${{ vars.<NAME> == 'true' }}`, so an unset repo
variable skips it. Turning a platform back on is a repo-variable change, not a
workflow edit — but re-run its §1 lane first, and restore its install section in
the release-notes body (the jobs were gated, the notes were trimmed).

**Two things that surprise people, both true as of v0.10.0:**

1. **The macOS GUI artifact is the GTK build, not the native one.** `--features macos`
   (AppKit / Core Graphics / Core Text, `src/macos/mod.rs`) ships in no artifact yet.
2. **No Windows GUI artifact exists.** `README.md`'s platform table advertises
   "Native Win32 + Direct2D + DirectWrite (**alpha**)" — that is the in-repo backend
   behind `--features win` (`src/win/mod.rs`), not something the release produces.
   Windows users get `vcd.exe`, the TUI, only.

Test the two native backends anyway (§1.3, §1.4). They are the next artifacts, and
the gate is where you find out they regressed — not after you've promised them.

---

## 1. The pre-release architecture gate

**Run every lane. Record every result.** Four backends, four lanes, three machines.

### 1.0 Run it with `scripts/platform-conformance.sh` — and with a clean `HOME`

[#926](https://github.com/JDonaghy/vimcode/issues/926) shipped the runner this
section used to describe by hand. Prefer it: it probes the host, runs every lane
that host supports, prints a matrix, and **exits non-zero if a supported lane did
not run or ran zero tests** — the #645 vacuous-green trap, enforced rather than
remembered.

```bash
scripts/platform-conformance.sh --print-plan   # resolve the matrix, run nothing
scripts/platform-conformance.sh                # run every supported lane
```

**Run it under a throwaway `HOME` anyway — belt and suspenders.**
[#976](https://github.com/JDonaghy/vimcode/issues/976) root-caused and fixed the
`shell_app` driver tests that read the real `~/.config/vimcode`: ten fixtures
(nine named in the issue, plus `ctrl_o_activates_original_tab_via_shell_app`,
independently measured red on `dellserver` and attributed to the same cause)
built their `TuiShellApp` with the production `TuiShellApp::new(None)`
constructor, which runs `Engine::new()` (reads the developer's real
`settings.json`/global `session.json`) *and* `restore_session_files()` (reopens
whatever windows/tabs/scroll-positions the developer's real **per-workspace**
session holds for this exact checkout path — keyed on `current_dir()`, so it is
the same on every run from the same clone). All ten now build through
[`TuiShellApp::new_for_test`], which substitutes in-memory defaults and skips
the restore entirely, so their starting state no longer depends on what a real
`vimcode` session left behind. See `src/tui_main/shell_app.rs`'s doc comments on
`TuiShellApp::new_for_test`, `app_with_sidebar_open`, and each fixed test for
the mechanism.

The throwaway-`HOME` habit below is still worth keeping — the ordinary
production constructor is still ambient by design (that is the whole point of
`new(None)` vs `new_for_test`), so any *future* test that reaches for it
inherits the same risk, and a manual smoke (§1.5) writes real config either
way:

```bash
env HOME=$(mktemp -d) RUSTUP_HOME=~/.rustup CARGO_HOME=~/.cargo PATH=~/.cargo/bin:$PATH \
  scripts/platform-conformance.sh
```

Measured at d6e0ed3 on `dellserver`, pre-fix: with the real `HOME`, one failure
(`ctrl_o_activates_original_tab_via_shell_app`); with a clean one, **tui 4129
passed / gtk 4294 passed across 49 binaries, exit 0**. Re-measure on a used
`HOME` at the #976 fix commit to confirm it now matches the clean-`HOME` count
before dropping this section's warning tone.

**The contamination runs both ways — give the manual smoke (§1.5) its own
throwaway `HOME` too.** The rule above is usually read as "tests can be poisoned
by a machine that *has* run the editor". The other direction bites harder: a
smoke run *is* that poisoning event, so smoking first and testing afterwards on
the same host makes the gate lie, and smoking under a dirty `HOME` makes the
*smoke* lie.

```bash
env HOME=$(mktemp -d) ./target/release/vcd /path/to/fixture   # smoke, isolated
```

Measured at `ea894db` on `dellserver`, both failure modes in one session:

- **Tests poisoned by a smoke.** A `vcd` smoke ran first, writing
  `~/.config/vimcode`. The next `cargo test` reported **5 failures** — including
  `hamburger_relocated_click_after_reveal_hides_menu_bar`, the scenario that
  release's headline fix had just ungated. Re-run under a clean `HOME`: **4477
  passed, 0 failed.** CI was green throughout. Read literally, that run said a
  just-shipped fix was still broken.
- **Smoke poisoned by a smoke.** Restored session state left a *different*
  sidebar panel active, which moves every activity-bar row. The hamburger
  toggle therefore appeared dead at its relocated position — a clean-`HOME`
  re-run of the identical click sequence toggled it correctly. Read literally,
  that run said a fixed bug was only half fixed.

Both readings were wrong, in opposite directions, from the same ambient state.
If a lane disagrees with CI, or a smoke disagrees with a merged fix's own
black-box test, **suspect `HOME` before you suspect the code** — and say in §1.6
which `HOME` each result came from.

**The oracle is load-bearing.** `tests/nvim_conformance.rs` hard-fails when `nvim`
is missing rather than skipping — 1,436 Vim-behaviour cases that did not run are
not a pass. The fleet standard is the version `NVIM_ORACLE_VERSION` pins in
`.github/workflows/ci.yml` (v0.12.5 today). A machine without it is not a gate
machine:

```bash
curl -fsSL -o /tmp/nvim.tar.gz \
  https://github.com/neovim/neovim/releases/download/v0.12.5/nvim-linux-x86_64.tar.gz
tar -C ~/.local -xzf /tmp/nvim.tar.gz && ln -sf ~/.local/nvim-linux-x86_64/bin/nvim ~/.local/bin/nvim
```

The manual per-lane commands below remain correct and are what the runner
invokes; keep them for reading a single lane in isolation, or when the runner
itself is what you doubt.

> **Scoping the gate to a platform-limited release.** v0.11.0 ships Linux only
> (§0), so the macOS and Windows lanes are *out of scope*, not *skipped* — the
> "a lane you skipped is a lane that failed" rule below governs platforms you are
> shipping. Write **"out of scope — not shipping this platform"** against those
> rows in §1.6 rather than leaving them blank. The Linux lane (§1.1) and the
> TUI-only lane (§1.2) are both mandatory for a Linux-only release, and §1.1 must
> be fully green.

| Lane | Machine | Command |
|---|---|---|
| Linux GTK + TUI | `precision` / `dellserver` | `cargo test` |
| TUI-only (no-GTK build hygiene) | any | `cargo test --no-default-features` |
| macOS native (AppKit) | **CI** (`test-macos` job, #1042) — re-run by hand only to double-check | `cargo test --lib --no-default-features --features macos` |
| macOS GTK | `macmini` | `cargo test` — **known-red, see §1.3b** |
| Windows GUI | `dell64` **only** | see §1.4 |

### The rule that makes the gate worth running

**A lane you skipped is a lane that failed.** Do not cut a release with a lane
unrun and unexplained — and do not accept a lane that ran but executed *zero
tests*. That second failure mode is not hypothetical: #645 is the story of
`[[bin]] vimcode` being `required-features = ["gui"]`, so `--no-default-features`
made cargo **silently omit the whole bin**. Every GTK assertion lives in that bin,
so the Test stage compiled zero of them and reported green — no error, no warning.
Check the test counts in the output, not just the exit code.

Rough expected magnitudes at 648f2dd (2026-09-14): `cargo test` ≈ 2,855 lib tests
plus 30+ integration targets; `--no-default-features` ≈ 2,700 lib tests plus the
same integration targets; the macOS-specific slice within the macOS lane's own
suite (`macos::mac_driver_tests::*`), 18 tests (was 4 at 648f2dd; grown since via
#928's `conformance_proof_slice` and later additions). #1042's `test-macos` CI job
asserts *that* count specifically is non-zero on every run — not just the lane's
much larger aggregate total, which would stay positive even if the 18 macOS-
specific tests silently failed to run at all (see §1.3) — instead of a human
eyeballing it once per release, so treat the job's own output as authoritative
over this number.

### 1.1 Linux GTK + TUI — the reference lane

```bash
# on precision or dellserver (both carry libgtk-4-dev; probed by coord doctor)
cd ~/src/vimcode && git checkout develop && git pull
cargo test
```

This is the only lane the per-PR CI also runs, and it is a strict superset of the
TUI lane — `gui` is the default feature and no `cfg(feature = "gui")` exists outside
the `vimcode` bin target. Runs headless: no `DISPLAY`, no `WAYLAND_DISPLAY`, nothing
calls `gtk::init`, GTK tests paint into in-memory Cairo `ImageSurface`s.

Must be **fully green**. Everything else in this section is about lanes that are not.

### 1.2 TUI-only — build hygiene

```bash
cargo test --no-default-features
```

Its unique value is proving vimcode still builds on a host with no GTK dev libs —
which is what the Windows and (future) native-macOS artifacts are. A green run here
says **nothing** about GTK code; see §1's zero-test warning.

### 1.3 macOS native (AppKit)

**As of #1042, CI runs this lane on every push/PR** — the `test-macos` job in
`.github/workflows/ci.yml`, on a `macos-latest` GitHub-hosted runner. Before #1042
this was a `macmini`-only manual step and `src/macos/` was never compiled anywhere
in CI; check the job's own run for the actual count rather than trusting a stale
number here. It also runs `cargo fmt -- --check` and
`cargo clippy --no-default-features --features macos -- -D warnings`, so a
macOS-only lint or format issue is now caught without anyone owning a Mac.

You can still run it by hand, e.g. to reproduce a CI failure locally on `macmini`
or any other Darwin host:

```bash
cargo test --lib --no-default-features --features macos
```

**Correction to this line's old claim ("expect 4 passed, sub-second"): that was
never accurate for this command.** `--no-default-features --features macos` still
compiles and runs the *entire portable lib suite* — on Linux, the equivalent
`--no-default-features` flags produce ~2,845 tests, multiple seconds, not four,
not sub-second. On a macOS runner that same portable suite runs, **plus** the 18
macOS-specific cases in `src/macos/mod.rs::mac_driver_tests` (including its
`conformance_proof_slice` submodule), double-gated on `feature = "macos"` **and**
`target_os = "macos"` — on Linux those 18 are not merely skipped, they do not
exist, so only a Mach-O host (or runner) can run them. Because the overall count
is dominated by the portable suite, "the total went up" is not proof the 18
macOS-specific tests ran at all — see the CI job's own `test-macos` step, which
greps the output for `macos::mac_driver_tests::` specifically rather than trusting
the aggregate `test result:` line, for exactly this reason. Read that job's output
for the current, authoritative macOS-specific count.

The CI job is scoped to `--lib` rather than the full `cargo test`, because
`tests/nvim_conformance.rs` hard-fails without an installed `nvim` oracle and the
job installs none — `--lib` runs the unit tests (where `mac_driver_tests` lives)
without pulling in that integration target.

**What CI still does not cover**, unchanged by #1042: the real `NSMenu` bar (the
`install_menu_bar` main-thread panic noted just below) and any real
windowed/interactive smoke. Those stay a `macmini` / Darwin-host manual step —
see §1.5.

**Known, expected, not a failure:** every one of the 18 `mac_driver_tests` cases
(`ShellApp::setup` tries to install the native menu bar on every driver
construction it builds, so this fires once per test in that module — not on the
thousands of unrelated portable tests the same command also runs) prints

```
thread '...' panicked at quadraui/src/macos/backend.rs:652:
MacBackend::install_menu_bar must be called from the main thread
vimcode: Backend::install_menu_bar panicked (quadraui main-thread assertion, see vimcode#901)
```

This is caught deliberately in `src/app.rs:7106` and documented there. Rust's test
runner runs every `#[test]` on a spawned thread, and quadraui's `install_menu_bar`
asserts the real AppKit main thread — so the harness *cannot* install a native menu
bar, by construction. Real invocations go through
`quadraui::macos::shell_runner`, which is always on the main thread.

**The coverage consequence is the part that matters for a release:** the native
`NSMenu` bar is **not exercised by any automated test on any lane**. If a release
claims macOS GUI behaviour, smoke the menu bar by hand (§1.5).

### 1.3b macOS GTK — opt-in, known-red

`macmini` carries gtk4 4.22.4, so `cargo test` runs there — but it is **not clean**.
Measured at 648f2dd (2026-09-14): **2855 passed, 12 failed** — three GTK pixel
probes plus nine `tui_main::shell_app` driver tests. The nine were **not**
GTK-specific or Darwin-specific — they were ambient-config leakage (#976, fixed;
see §1.0) triggered by `macmini` being a machine that has actually run
`vimcode` from this checkout, and were expected to reproduce on the TUI lane
and on any other used machine too, independent of platform. #976 landed a fix
switching the affected fixtures to `TuiShellApp::new_for_test`; this section's
2855/12 count is stale as of that fix and needs a fresh `macmini` measurement
at the fix commit — expect **2855 passed, 2 failed** (the two remaining GTK
pixel probes below) if both that fix and #1375's below still hold. The two
still-expected-red ones are:

- `gtk::chrome_paint_tests::window_control_buttons_are_visible_against_their_background_in_every_theme`
- `gtk::testing::tests::window_split_divider_drag_repaints_the_line_at_the_new_position`

Both are pixel/paint probes on in-memory Cairo surfaces (#934). On quartz, pangocairo
rasterises via Core Text rather than freetype, so glyph ink and colour compositing
differ from the Linux reference. **Treat these two as expected-red on Darwin and
green on Linux — but confirm they are green on the Linux lane at the same SHA before
waving them through.** If they are red on Linux too, they are ordinary bugs and this
section is wrong.

`gtk::testing::minimap::minimap_click_at_the_middle_scrolls_to_half_the_file` used to
be the third member of this list (#934), and #1375 confirmed on a real Darwin host
that #934's widened tolerance still wasn't enough — Core Text's gamma-correct AA can
blend a syntax-colored stroke past any pixel-chroma threshold chosen without access to
the actual rasteriser. #1375 rewrote the probe to read `GtkDriver::painted_texts()`
(the recorded Pango-layout text, identical on every platform regardless of how it was
rasterised) instead of pixel colour, removing the Core-Text-vs-FreeType dependency at
the root rather than re-tuning it a third time. Not yet re-measured on `macmini` —
confirm green there before trusting this line over the fix commit's own reasoning.

This is also why `coordinator.yml` guards vimcode's `test_command` on `uname`: the
Darwin branch runs the GTK-less lane.

### 1.4 Windows GUI

**`dell64` is the only machine that can do this.** It is WSL2 on a Windows 11 host:
it cross-compiles from the WSL side with cargo-xwin and runs the resulting `.exe`
on its own Windows host through WSL interop.

```bash
# on dell64, from the WSL side
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo xwin build --release --target x86_64-pc-windows-msvc \
  --no-default-features --features win
```

**`crt-static` is mandatory.** The Windows host has no `vcruntime140.dll`; without
it the `.exe` dies before `main()` **with no output at all**, which reads as a
passing no-op rather than a failure. If a Windows run produces empty output, assume
this before assuming success.

There is currently **no automated Win-GUI test suite** — `src/win/` contains zero
`#[test]`s. What you can do today:

- `cargo check --no-default-features --features win` — type-checks on an ordinary
  Linux host, because quadraui's `win` module stubs every WinAPI call behind
  `cfg(target_os = "windows")`. Cheap, and worth running on any machine.
- Run the built `.exe` and work through `win-smoke-tests.md` by hand.

### 1.5 Manual smoke — what no lane covers

Everything above is headless. Before a release, open the real app on each platform
you are shipping and confirm it starts, paints a first frame, and takes input:

- **Linux GTK** — `./target/release/vimcode`
- **macOS** — the artifact you are shipping (currently the *GTK* build), plus the
  native build if you are exercising it, with **the native menu bar** specifically
  (§1.3 — nothing automated covers it)
- **Windows** — `vcd.exe`, plus `win-smoke-tests.md` if shipping any GUI build

**Run every one of these under a throwaway `HOME`** (`env HOME=$(mktemp -d) ...`),
for both reasons in §1.0: a smoke inherits whatever session state the last run
left — restored panels move the activity bar, so a control can look dead when it
is fine — and the smoke then leaves that state behind for the next `cargo test`
on the same host. If you smoke a TUI build, `tmux` is the harness: `script -qec`
gives a pty for output but leaves stdin a pipe, so the TUI's `^[[c` / `^[[6n`
terminal queries go unanswered and it exits on EOF after a couple of hundred
bytes. Mouse behaviour can be driven for real by injecting SGR sequences as pane
input (`tmux send-keys -H`, `\033[<0;COL;ROWM` press / `m` release, `<2;` for the
right button), which is how the v0.12.0 post-release smoke exercised the explorer
and the editor scrollbar.

### 1.6 Record the results

Paste into the `develop` → `main` PR body:

```markdown
## Architecture gate (docs/RELEASING.md §1)
- [ ] Linux GTK + TUI — `cargo test` on <machine> @ <sha> — <N> passed
- [ ] TUI-only — `cargo test --no-default-features` @ <sha> — <N> passed
- [ ] macOS native — CI's `test-macos` job @ <sha> (see the run) — <N> passed; confirm the native menu bar by hand (§1.3)
- [ ] macOS GTK — opt-in; 3 known-red (§1.3b) / not run, because: <reason>
- [ ] Windows — `cargo check --features win` / built + smoked: <result>
- [ ] Manual smoke — Linux / macOS / Windows: <what you opened, what you saw>
```

For a Linux-only release the same block collapses to:

```markdown
## Architecture gate (docs/RELEASING.md §1)
- [ ] Linux GTK + TUI — `cargo test` on <machine> @ <sha> — <N> passed
- [ ] TUI-only — `cargo test --no-default-features` @ <sha> — <N> passed
- [ ] Manual smoke — `./target/release/vimcode` and `vcd` on <machine>: <what you saw>
- [ ] macOS / Windows / Flatpak — out of scope, not shipping (§0, §2.2)
```

> **#926 has shipped** — `scripts/platform-conformance.sh` is the entrypoint, see
> §1.0. The per-lane commands above are what it runs; read them when you want one
> lane in isolation.

---

## 2. Shipping

### 2.0 The release runs in GitHub Actions, not locally

`release.yml` was disabled in 3abd5ec — *"disable GitHub Actions workflows until
quadraui sibling-checkout resolved"*. That premise died with #691, and the file was
re-enabled for v0.11.0. **Merging the `develop` → `main` PR is the release**: the
push to `main` triggers the workflow, which reads the version out of `Cargo.toml`,
builds, and publishes the GitHub Release tagged `v$VERSION`. Nothing is built on
your laptop. The §1 gate is the only part you run by hand.

### 2.1 The quadraui pin needs nothing — it is a public git dep

The recurring worry is that a `rev`-pinned git dependency can't be resolved by a
hosted runner. It can, and already is:

- `JDonaghy/quadraui` is a **public** repo — `git ls-remote` over anonymous HTTPS
  succeeds, so cargo needs no token, no secret, no submodule, no deploy key.
- The pinned rev is an **ancestor of quadraui's `develop`** (check with
  `gh api repos/JDonaghy/quadraui/compare/develop...<rev> --jq .status` — `behind`
  or `identical` is good, `diverged` is not). It is permanently reachable, so a
  fresh clone can fetch it even after the feature branch that carried it is deleted.
- Per-PR CI on `develop` is green today on GitHub-hosted runners, which *is* the
  proof that a clean checkout resolves the pin.

`[patch.crates-io] vt100` is gone — quadraui#795 removed the vendored shim upstream.
`CLAUDE.md` still mentions keeping the two pins in sync; there is only one pin now.

**Bumping the pin before a release is optional, not required.** If you do bump it,
it is a code change like any other: branch, edit `rev`, `cargo test` (snapshots
re-run against the new rev), land through the normal workflow — not something to
slip into the release PR.

### 2.2 Flatpak is broken and is not shipping ([#975](https://github.com/JDonaghy/vimcode/issues/975))

`flatpak/cargo-sources.json` predates #691: 635 crates-io entries, **zero
quadraui**. Worse, the manifest's inline cargo config only replaces
`[source.crates-io]`, so even a regenerated file would leave the rev-pinned
quadraui git dep reaching for the network inside flatpak-builder's offline build.
Fixing it needs two things, not one:

1. Regenerate with a generator that emits git sources —
   `python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json`
   (the script is **not in this repo**; fetch it from flathub/flatpak-builder-tools).
2. Add the generator's `[source."git+https://github.com/JDonaghy/quadraui.git?rev=…"]`
   replacement stanza to the inline `config` block in
   `flatpak/io.github.jdonaghy.VimCode.yml`.

Until both land, leave `RELEASE_FLATPAK` unset. Do not regenerate the JSON and
assume it works — it needs a real flatpak-builder run on a Linux host to prove it.

### 2.3 Steps

1. **Run the §1 gate.** All lanes, results recorded.
2. **Bump the version** in `Cargo.toml`.
3. **Flatpak:** skip — see §2.2. (`Cargo.lock` has moved a long way since v0.10.0,
   but regenerating `cargo-sources.json` alone does not make the bundle buildable.)
4. **Open the `develop` → `main` PR**, with the §1.6 checklist in the body. CI runs
   on the PR.
5. **Merge.** The push to `main` triggers `release.yml`, which builds the Linux
   artifacts and creates the GitHub Release tagged `v$VERSION`. Watch the run —
   this is the first release since the workflow was re-enabled, so treat a red
   run as expected-possible rather than alarming, and fix forward on `develop`.
6. **Never push directly to `main`.**
7. **Verify the release**: download `vimcode-linux-x86_64` and `vcd-linux-x86_64`
   from the Release page and run `--version` on a Linux box. The banner prints the
   resolved quadraui rev, so it also confirms the pin baked in as expected.

---

## 3. Known-red inventory

Keep this current — a release gate is only useful if "expected red" is a short,
dated list rather than a habit.

| Symptom | Lane | Status |
|---|---|---|
| 3 pixel/paint probes fail (§1.3b) | macOS GTK | Expected — #934, Core Text vs freetype rasterisation |
| `shell_app` driver tests fail on a used machine | any host with a real `~/.config/vimcode` | Fixed — #976; ten fixtures built `TuiShellApp` through the ambient production constructor, which reopens the real per-workspace session; switched to `TuiShellApp::new_for_test` (see §1.0). Needs a fresh `macmini`/`dellserver` measurement at the fix commit to close out. Was **not** platform-specific and **not** the quadraui pin |
| `install_menu_bar` main-thread panic, caught (§1.3) | macOS native | Expected — test-runner threading; vimcode#901 closed, native menu bar untested |
| No Win-GUI test suite | Windows | Gap — `src/win/` has zero `#[test]`s |
| Flatpak bundle unbuildable (§2.2) | Linux | Gap — #975; `cargo-sources.json` predates the #691 git dep; not shipping in v0.11.0 |
