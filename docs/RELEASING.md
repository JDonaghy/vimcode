# Releasing VimCode

Cutting a release has two halves: **prove every architecture still works** (§1 — the
gate), then **ship** (§2 — the mechanics). The gate is a prerequisite, not a
formality: the shipped artifacts cover four backends across three operating systems,
and the per-PR CI covers exactly one of them.

> **Scope.** This is the operator's runbook. Workers and reviewers don't need it —
> `CLAUDE.md`'s *Branching & Releases* section is the summary, and it points here.

---

## 0. What a release actually ships

`.github/workflows/release.yml` (see §2.0 — it is currently `.disabled`) produces:

| Artifact | Built on | Backend | Build command |
|---|---|---|---|
| `vimcode-linux-x86_64`, `vimcode_*.deb`, `vimcode.flatpak` | ubuntu-24.04 | GTK4 (glibc) | `cargo build --release --bin vimcode` |
| `vcd-linux-x86_64` | ubuntu-24.04 | TUI (musl, static) | `cargo build --release --bin vcd --no-default-features --target x86_64-unknown-linux-musl` |
| `vimcode-macos-arm64.tar.gz` | macos-latest | **GTK4 via Homebrew** | `cargo build --release --bin vimcode` |
| `vcd-macos-arm64.tar.gz` | macos-latest | TUI | `cargo build --release --bin vcd --no-default-features` |
| `vcd-windows-x86_64.exe` | windows-latest | TUI | `cargo build --release --bin vcd --no-default-features` |

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

| Lane | Machine | Command |
|---|---|---|
| Linux GTK + TUI | `precision` / `dellserver` | `cargo test` |
| TUI-only (no-GTK build hygiene) | any | `cargo test --no-default-features` |
| macOS native (AppKit) | `macmini` (or any Darwin host) | `cargo test --lib --no-default-features --features macos` |
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

Rough expected magnitudes at v0.10.0: `cargo test` ≈ 2,830 lib tests plus 30+
integration targets; `--no-default-features` ≈ 2,554 lib tests plus the same
integration targets; the macOS lane, 4 tests.

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

```bash
# on macmini, or any Darwin host
cargo test --lib --no-default-features --features macos
```

Expect **4 passed**, sub-second once compiled. The suite is
`src/macos/mod.rs::mac_driver_tests`, double-gated on `feature = "macos"` **and**
`target_os = "macos"` — on Linux it is not merely skipped, it does not exist, so
only a Mach-O host can run it.

**Known, expected, not a failure:** all four tests print

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
Measured at 64316c8: **2833 passed, 3 failed**.

- `gtk::chrome_paint_tests::window_control_buttons_are_visible_against_their_background_in_every_theme`
- `gtk::testing::minimap::minimap_click_at_the_middle_scrolls_to_half_the_file`
- `gtk::testing::tests::window_split_divider_drag_repaints_the_line_at_the_new_position`

All three are pixel/paint probes on in-memory Cairo surfaces. On quartz, pangocairo
rasterises via Core Text rather than freetype, so glyph ink and colour compositing
differ from the Linux reference. **Treat these three as expected-red on Darwin and
green on Linux — but confirm they are green on the Linux lane at the same SHA before
waving them through.** If they are red on Linux too, they are ordinary bugs and this
section is wrong.

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

### 1.6 Record the results

Paste into the `develop` → `main` PR body:

```markdown
## Architecture gate (docs/RELEASING.md §1)
- [ ] Linux GTK + TUI — `cargo test` on <machine> @ <sha> — <N> passed
- [ ] TUI-only — `cargo test --no-default-features` @ <sha> — <N> passed
- [ ] macOS native — `--features macos` on <machine> @ <sha> — 4 passed
- [ ] macOS GTK — opt-in; 3 known-red (§1.3b) / not run, because: <reason>
- [ ] Windows — `cargo check --features win` / built + smoked: <result>
- [ ] Manual smoke — Linux / macOS / Windows: <what you opened, what you saw>
```

> **This section collapses to one command when [vimcode#926](https://github.com/JDonaghy/vimcode/issues/926)
> lands.** That issue builds `scripts/platform-conformance.sh`: one entrypoint that
> probes the host, runs the lanes it supports, prints a matrix, and exits non-zero
> on a skipped or zero-test lane. Update this section to call it when it ships.

---

## 2. Shipping

### 2.0 The release workflow is currently disabled

`.github/workflows/release.yml.disabled` was renamed in 3abd5ec —
*"disable GitHub Actions workflows until quadraui sibling-checkout resolved"*.
**That premise is stale.** #691 moved quadraui to a `rev`-pinned git dependency;
cargo clones the pinned rev into `~/.cargo/git/` and a plain build does not consult
`~/src/quadraui` at all. Re-enabling it is a decision someone should make
deliberately — not a blocker. Until then, a release means building the §0 matrix by
hand and attaching the artifacts.

### 2.1 Steps

1. **Run the §1 gate.** All lanes, results recorded.
2. **Bump the version** in `Cargo.toml`.
3. **If `Cargo.lock` changed**, regenerate the flatpak manifest:
   ```bash
   python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json
   ```
4. **Open the `develop` → `main` PR**, with the §1.6 checklist in the body. CI runs
   on the PR.
5. **Merge.** With `release.yml` enabled this triggers the build and creates a
   GitHub Release tagged `v$VERSION`. Disabled, build and upload the §0 matrix
   yourself.
6. **Never push directly to `main`.**

---

## 3. Known-red inventory

Keep this current — a release gate is only useful if "expected red" is a short,
dated list rather than a habit.

| Symptom | Lane | Status |
|---|---|---|
| 3 pixel/paint probes fail (§1.3b) | macOS GTK | Expected — Core Text vs freetype rasterisation |
| `install_menu_bar` main-thread panic, caught (§1.3) | macOS native | Expected — test-runner threading; vimcode#901 closed, native menu bar untested |
| No Win-GUI test suite | Windows | Gap — `src/win/` has zero `#[test]`s |
