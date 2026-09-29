# Platform Conformance Suite (#926)

One entrypoint that answers "do the native builds behave the same on every
platform?" across the four driver-tier black-box harnesses:

| Backend | Driver | Where | `#[test]` count (at #926) |
|---|---|---|---|
| TUI | quadraui `TuiDriver` via `driver_with_shell` | `src/tui_main/shell_app.rs` | 195 |
| GTK | `GtkDriver`, in-memory Cairo `ImageSurface` | `src/gtk/testing.rs` | 134 |
| macOS | `quadraui::macos::testing::driver_with_shell`, `CGBitmapContext` | `src/macos/mod.rs::mac_driver_tests` | 4 |
| Win-GUI | — | `src/win/` | 0 |

`scripts/platform-conformance.sh` probes the host it's run on, runs whichever
lanes that host can support, and prints a lane matrix. It exits non-zero if a
lane the probe says should run didn't, or if a lane ran but executed **zero**
tests — see the script's own header comment for the full rationale (#645).

**#1092 made it gateable.** Before that it was an on-demand runner whose only
output was a human-readable matrix on a terminal, so a release could roll with
a platform-specific regression nobody had run it against (#1086–#1089 were four
such regressions, all found by an operator looking at a screenshot). It now has:

- a **documented exit contract** — four codes, below, everything but `0` meaning
  *do not roll*;
- a **machine-readable summary** — `--summary <path>` writes a JSON document
  covering all four lanes including the ones this host never probed, plus a
  single greppable `PLATFORM_CONFORMANCE_SUMMARY …` line on stdout;
- a **lane-to-machine map** (below) so the release procedure is written against
  real fleet capability rather than a wish.

`tests/platform_conformance.rs` pins all of it through the script's stub hooks,
so the contract is enforced on any host — no gtk4, no Darwin, no cargo-xwin
required.

## Exit contract

Anything other than `0` means **do not roll**.

| Code | Verdict | Meaning |
|---|---|---|
| `0` | `pass` | Every in-scope lane reached a green terminal state (`passed`, or `check-only` for the win cross-compile tier). |
| `1` | `fail` | At least one lane is `failed` or `error`: a real test failure, a non-zero lane exit, a check-only build that did not compile, a `--lane X` forced onto a host whose probe says X is unsupported, or — the load-bearing case — **a lane that ran but executed zero tests** (#645). An unexpected termination (bash abort, signal, #933) is also forced to `1` by the EXIT trap. |
| `2` | — | **Usage error**: bad invocation (unknown lane, missing argument, unwritable `--summary` path). **No lane ran and no summary file was written.** A release caller must treat a missing summary file exactly like `1`: there is no evidence, so do not roll. |
| `3` | `coverage-gap` | Every lane that ran is green, but at least one lane was **skipped on a host whose probe says it is capable** (`skipped-capable` — e.g. GTK on Darwin, capable but opt-in). Nothing is known to be broken; the run simply does not cover what it could have. Do not roll on this host's evidence alone: re-run with `--lane <name>` to force it, or point at another machine's summary that covers it. |

The `--summary` destination is validated (and truncated) **before any lane
runs**, so a bad path costs a second rather than a full `cargo test`.

## Machine-readable summary

```bash
scripts/platform-conformance.sh --summary /tmp/conformance-$(uname -n).json
scripts/platform-conformance.sh --summary -     # same JSON on stdout, after a marker line
```

Every run — `--summary` or not — ends with one greppable line:

```
PLATFORM_CONFORMANCE_SUMMARY schema=1 verdict=coverage-gap exit=3 mode=run host=Darwin lanes=tui:passed,gtk:skipped-capable,macos:passed,win:not-in-scope tests_passed=2837 tests_failed=0
```

That line alone is enough to gate on: it names all four lanes and their
statuses, whether or not this host probed them. Split the `lanes=` field on
commas and each pair on its **first** colon — under `--print-plan` a status is
itself `plan:run` / `plan:check-only`, so a naive split on every colon
mis-parses a plan run (which is never a gate result anyway: `verdict=plan`).

`--summary <path>` writes the full document (schema
`vimcode.platform-conformance/1`):

```json
{
  "schema": "vimcode.platform-conformance/1",
  "generated_at": "2026-09-29T14:24:01Z",
  "mode": "run",
  "host": {"uname": "Darwin", "node": "macmini"},
  "verdict": "coverage-gap",
  "exit_code": 3,
  "scope": ["tui", "gtk", "macos", "win"],
  "forced": [],
  "totals": {"tests_passed": 2837, "tests_failed": 0, "binaries": 49},
  "lanes": [
    {"lane": "tui", "status": "passed", "in_scope": true, "forced": false,
     "capable": true, "auto_selected": true, "tier": "full",
     "command": "cargo test --no-default-features",
     "tests_passed": 2837, "tests_failed": 0, "binaries": 49,
     "detail": "2837 passed across 49 binaries"},
    {"lane": "gtk", "status": "skipped-capable", "...": "..."}
  ]
}
```

Fields that did not apply are `null` (a skipped lane has no test counts; an
unprobed lane has no `capable`), never `0` — a zero test count is a *failure*
signal in this tool and must not be confused with "did not run".

With `--summary -` the same JSON goes to stdout after the marker line
`===PLATFORM-CONFORMANCE-JSON===`, so a caller can
`sed -n '/===PLATFORM-CONFORMANCE-JSON===/,$p' | tail -n +2 | jq .`.

## Lane-to-machine map

**No single fleet host covers all four lanes.** A roll needs the union of at
least two machines' summaries (three to cover Windows).

| Lane | Machine(s) | Typical status there | Why only that machine |
|---|---|---|---|
| `tui` | any (`precision`, `dellserver`, `macmini`, `dell64`) | `passed` | No prereqs — `cargo test --no-default-features`. |
| `gtk` | `precision` or `dellserver` (Linux) | `passed` | Needs gtk4 dev libs *and* the FreeType rasteriser the pixel probes were baselined against. On `macmini` gtk4 is installed but the lane is `skipped-capable` (exit 3) — see below. |
| `macos` | `macmini` **only** | `passed` (degraded, see below) | `uname = Darwin`; nothing else in the fleet is a Mac. |
| `win` | `dell64` **only** | `passed` (full tier, via WSL interop) | Only host that can both cross-compile with `cargo-xwin` and execute the `.exe` on an attached Windows 11 host. Elsewhere: `skipped` (no cargo-xwin) or `check-only` (cargo-xwin, no interop). |

## What the release side calls

The release procedure runs, **on each machine in the map above**:

```bash
scripts/platform-conformance.sh --summary "/tmp/conformance-$(uname -n).json"
```

plus, on `macmini` only, a second forced run for the opt-in GTK lane if that
roll wants Darwin GTK evidence:

```bash
scripts/platform-conformance.sh --lane gtk --summary "/tmp/conformance-$(uname -n)-gtk.json"
```

Then it applies these rules to the collected summaries:

1. **Any summary with `exit_code` `1` or `2` ⇒ do not roll.** Exit `2` includes
   the "summary file missing entirely" case — no evidence is the same as bad
   evidence.
2. **Union the lanes across every summary.** Each of `tui`, `gtk`, `macos`,
   `win` must appear with status `passed` or `check-only` in *at least one*
   summary. A lane that is only ever `skipped`, `skipped-capable` or
   `not-in-scope` across the whole fleet is **uncovered** ⇒ do not roll (or
   roll with that gap recorded deliberately in the release notes; the point is
   that it is now a decision rather than an oversight).
3. **`exit_code` `3` on a single host is not automatically fatal** — it means
   that host left a capable lane uncovered. It is fatal only if rule 2 then
   finds no other summary covering that lane.
4. **Archive the summaries with the release artifacts.** "Was the GTK lane
   green for this build?" must be answerable from a file six months later, not
   from a terminal scrollback.

> **Outstanding (not in this repo):** nothing in the fleet's release lane
> (propagate / publish, which lives in the `claude-coordinator` repo) calls the
> script yet. This document is the contract that change will consume; wiring it
> up is a separate, operator-coordinated change (#1092 scope explicitly stops
> at this repo's boundary).

## Non-goal — read this before reaching for it in CI

**This is not a per-story gate.** It does not run in the per-PR CI jobs and is
not wired into the coordinator's Test stage. It is an *on-demand* runner the
operator invokes by hand, and a *release-time* gate before cutting a version.
Day-to-day issue work is unaffected; nothing here changes the per-PR
`cargo test` / `cargo clippy` workflow in `CLAUDE.md`.

## Quick reference

```bash
scripts/platform-conformance.sh                  # probe this host, run every supported lane
scripts/platform-conformance.sh --lane tui --lane gtk   # force a subset (repeatable)
scripts/platform-conformance.sh --summary c.json # also write the machine-readable matrix
scripts/platform-conformance.sh --summary -      # ... to stdout instead
scripts/platform-conformance.sh --print-plan     # resolve + print the matrix, run nothing
scripts/platform-conformance.sh --help           # usage + the exit contract
```

Forcing a lane the host cannot support (`--lane <name>` whose probe fails) is
an **error**, not a silent skip — it exits non-zero and prints the probe's
reason. This matters because it's the only way to *insist* a lane run when you
know better than the default policy (e.g. forcing `gtk` on Darwin — see below).

## The three fleet routes (all verified)

### Linux + GTK — `precision` or `dellserver`

The reference lane. Either machine carries the `gtk` capability
(`CAPABILITY_PREREQS` probes `pkg-config --modversion gtk4`), so a plain

```bash
scripts/platform-conformance.sh
```

auto-selects `tui` and `gtk` (and `win` at whichever tier `cargo-xwin`/WSL
interop supports on that box — usually `skipped: cargo-xwin not found on
PATH`, since these are the GTK reference machines, not the Windows one).
`macos` is reported `skipped (host is not Darwin)`.

### macOS — `macmini`

`macmini` carries the `macos` capability. Two things to know before reading a
green run there as "the native builds are fine everywhere":

1. **The GTK lane is red on Darwin and is opt-in there, not auto-selected.**
   gtk4 4.22.4 *is* installed via Homebrew, so the probe reports `capable`,
   but a gui-on `cargo test` there measured **2833 passed, 3 FAILED** at
   `64316c8`:
   `chrome_paint_tests::window_control_buttons_are_visible_against_their_background_in_every_theme`,
   `gtk::testing::minimap::minimap_click_at_the_middle_scrolls_to_half_the_file`,
   `gtk::testing::tests::window_split_divider_drag_repaints_the_line_at_the_new_position`.
   All three are pixel/paint probes on in-memory Cairo surfaces; on Quartz,
   Pangocairo rasterises via Core Text rather than FreeType, so glyph ink and
   colour compositing differ from the Linux baseline those tests were written
   against. The script therefore reports the `gtk` lane
   `skipped-capable (opt-in on Darwin...)` by default on a Darwin host — and
   since #1092 a `skipped-capable` lane makes the whole run **exit `3`
   (coverage-gap)**, not `0`. That is deliberate: `macmini` *could* have
   covered GTK and didn't, so its summary alone must not read as a clean
   bill of health. Force it anyway with `--lane gtk` when you specifically
   want to see those three fail again (e.g. checking whether a Pangocairo
   bump changed anything) — forcing does **not** fix them, it just runs the
   lane instead of skipping it. In the normal release flow the GTK lane's
   green record comes from `precision`/`dellserver` instead (see the
   lane-to-machine map above).
2. **The macOS driver lane passes but is degraded — do not read 4/4 green as
   "the native menu bar works".** Measured on a Darwin host (arm64, gtk4
   4.22.4 present) at `develop`:

   ```
   cargo test --lib --no-default-features --features macos mac_driver_tests
   → 4 passed; 0 failed; finished in 0.19s
   ```

   All four emit a swallowed panic from the pinned quadraui:
   `MacBackend::install_menu_bar must be called from the main thread`
   (`quadraui/src/macos/backend.rs:652`, tracked as **vimcode#901**). The
   wrapper catches the panic, so the tests pass, but the native menu bar never
   actually installs — `native_menu_backend_suppresses_the_drawn_menu_row` and
   `menu_activated_reaches_the_same_action_as_the_drawn_menu` are asserting
   against a degraded frame. **Do not "fix" this from the vimcode side** — the
   bug is in the pinned quadraui rev; see `GOALS.md` / the Platform-Neutrality
   Rule in `CLAUDE.md`.

   `scripts/platform-conformance.sh` reports this lane `passed` (it genuinely
   is, by the script's own vacuous-pass rule — 4 tests ran and 4 tests
   passed) but cannot detect "passed while degraded". That gap is real and is
   filed as follow-up work (see below), not silently patched over here.

### Windows — `dell64`

`dell64` is the **only** fleet machine that can target
`x86_64-pc-windows-msvc`. It runs the script from its WSL side; `cargo-xwin`
cross-compiles there, and the resulting `.exe` is executed **on `dell64`'s own
Windows 11 host**, reached through WSL interop
(`/proc/sys/fs/binfmt_misc/WSLInterop`, and `cargo-xwin` on `PATH`) — the
probe the script uses to decide the win lane is at "full" tier there.

```bash
scripts/platform-conformance.sh --lane win
```

runs:

```bash
RUSTFLAGS="-C target-feature=+crt-static" cargo xwin test \
  --target x86_64-pc-windows-msvc --no-default-features --features win
```

**The `crt-static` flag is not optional and the script sets it itself** — it
never relies on ambient `RUSTFLAGS`. `dell64`'s Windows 11 host has no
`vcruntime140.dll` installed; a dynamically-linked `.exe` built without the
static CRT dies before `main()` runs, with **no output printed at all**. That
reads as a passing no-op (`0 passed; 0 failed`) rather than a build/runtime
problem — which is exactly the kind of vacuous green the script's zero-tests
guard exists to catch, so the two protections are complementary: crt-static
keeps the binary from silently failing to start, and the zero-tests guard
catches it if some *other* silent-failure mode ever gets past that.

### Any other machine

A machine with none of `gtk4`, Darwin, or `cargo-xwin`+WSL-interop runs only
the `tui` lane by default; the others report `skipped (<reason>)` and do not
fail the run.

## Reading the matrix

```
LANE     STATUS           DETAIL
tui      passed           195 passed across 1 binaries
gtk      passed           134 passed across 1 binaries
macos    skipped          host is not Darwin (uname: Linux)
win      check-only       cargo-xwin present but no WSL interop detected ...; compiled ok; not executed
```

Six states, each meaning something different (the same strings appear in the
machine-readable summary's `status` field):

- **`passed`** — the lane ran and every test in it passed. The script also
  guards against this being reported when zero tests actually executed (the
  #645 vacuous-green failure mode) — a "passed" line always has a nonzero
  test count in its detail column.
- **`failed`** — the lane ran and something is wrong: a real test failure, a
  nonzero exit code, or (this is the part worth remembering) **zero tests
  executed**. All three read as `failed`, not `passed` or `skipped`.
- **`skipped (<reason>)`** — the host's probe says this lane **isn't supported
  here** (missing toolchain, wrong OS). Does not fail the run: there is
  nothing this machine could have covered and didn't.
- **`skipped-capable (<reason>)`** — the probe says the host **is** capable but
  the lane was not auto-selected (the deliberate policy opt-out: GTK on
  Darwin). This is the coverage gap, and it exits `3` — distinct from both a
  green run and a failure, because nothing is broken but nothing is proven
  either. Force it with `--lane <name>` to turn it into a real result.
- **`error`** — a lane forced with `--lane <name>` whose probe says the host
  cannot support it. Exits `1`: insisting on a lane the machine can't run is a
  mistake worth failing over, not a silent skip.
- **`not-in-scope`** — appears in the machine-readable summary only: the run
  used a forced `--lane` subset and this lane was never probed at all. The
  release caller's job is to find another machine's summary that covers it.
- **`check-only`** — win-lane-specific: `cargo-xwin` is on `PATH` so the
  win-feature code was cross-compiled and type-checked, but there was no WSL
  interop to a Windows host to actually execute the result. Real signal
  (it compiles), but explicitly not "the tests passed" — a machine with
  `cargo-xwin` installed as a bare cross tool (no attached Windows host)
  reports this instead of silently claiming either `passed` or `skipped`.

## Bash compatibility — target bash 3.2, not just "bash" (#933)

The script's shebang is `#!/usr/bin/env bash`, but on macOS that resolves to
**bash 3.2.57** — Apple has shipped that exact build since 2007 (frozen at the
last GPLv2 release) and there is no newer bash on a stock Mac. Any bash-4+
construct (`declare -A`, `mapfile`/`readarray`, namerefs, `;;&` case
fallthrough, `**` globstar) silently breaks the very lane this script exists to
run on macOS. #933 was exactly this: `declare -A FORCED=()` failed on 3.2,
`set -u` then tripped on the never-populated array, and the script died
mid-run while still reporting **exit 0** — zero lanes run, vacuously green,
inside the tool built to prevent vacuous green.

Two guards now cover this, and both are worth preserving in any future edit:

- **No bash-4 syntax.** The forced-lane set is tracked as a space-delimited
  string matched with a `case` glob (`is_forced()`), not an associative array.
  If you need a new lookup table, reach for a `case` or a second parallel
  indexed array before reaching for `declare -A`.
- **An EXIT trap that cannot itself report a false green.** `finish()` is the
  only way the script exits intentionally; it sets `SCRIPT_DONE=1` right
  before calling `exit`. The `on_exit` trap fires on *every* termination and
  forces a non-zero status if `SCRIPT_DONE` was never set — i.e. bash died out
  from under the script (a parse error, a `set -u` abort, a signal) rather
  than reaching a real exit point. This is deliberately independent of the
  bash-3.2 fix: it's the general "this script's own exit code must never lie"
  contract, so the *next* unanticipated bash bug is loud instead of silently
  green. `PLATCONF_TEST_FORCE_UNHANDLED_EXIT=1` is a test-only hook that
  exercises this deterministically without needing a real crash.

No CI job here runs the script on a Mac (see #933's provenance note — there is
no `capability_rules` entry routing `scripts/` to a `macos`-capable machine;
that's a `coordinator.yml` change filed separately, not in this repo). Anyone
editing this script should sanity-check unfamiliar bash constructs against
3.2 by hand, since nothing will catch a regression here automatically.

## Out of scope — follow-ups, not attempted here

- **Growing the shared scenario table.** A green conformance run today proves
  "the native backends don't abort on first paint", not behavioural parity —
  macOS has 4 tests, Windows has 0. The real answer is a scenario table all
  four drivers execute, the way `render::frame_sequence_fixture()` already
  forces GTK and TUI to agree on frame order
  (`frame_sequence_matches_across_backends_via_gtk_driver` in
  `src/gtk/testing.rs` and its `_via_shell_app` twin). File as a separate
  issue against the #7 Platform-Neutral milestone.
- Fixing vimcode#901 (macOS menu bar install panic) or the three Darwin GTK
  paint failures — reported honestly above, not patched here.
- Detecting "passed but degraded" (the macOS driver lane's swallowed-panic
  case) — the script's vacuous-pass guard catches zero-tests-executed, not
  a test that passes despite an internally-caught error. Also follow-up work.
- Any GitHub Actions workflow — the operator chose the fleet route; CI wiring
  can reuse the same script later if wanted.
- **Calling the script from the fleet's release lane** (#1092). This repo now
  publishes the contract — exit codes, summary schema, lane-to-machine map,
  and the rules the caller applies — but the propagate/publish lane lives in
  the `claude-coordinator` repo and still does not invoke it. That wiring is
  an operator-coordinated change consuming this document, deliberately kept
  outside this repo's diff.
