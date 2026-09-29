#!/usr/bin/env bash
#
# #926: one entrypoint that answers "do the native builds behave the same on
# every platform?" across the four driver-tier black-box harnesses (TUI, GTK,
# macOS, Win-GUI). #1092 made it gateable: a documented exit contract, a
# machine-readable per-lane summary, and a lane-to-machine map in
# docs/PLATFORM_CONFORMANCE.md, so a release procedure can call this and
# refuse the roll on the exit code alone.
#
# The one rule that makes this worth building: a SKIPPED lane is not a pass.
# The script exits non-zero when:
#
#   1. a lane the host's probe says is supported did not run, or
#   2. a lane ran but executed ZERO tests.
#
# That rule exists because this repo already paid for its absence once (#645):
# `[[bin]] vimcode` is `required-features = ["gui"]`, so
# `cargo test --no-default-features` silently omits the whole bin -- all of
# `src/gtk/**`, including the GtkDriver harness, lives there. The Test stage
# compiled zero GTK assertions and every GTK issue earned a vacuously green
# verdict, with no error and no warning. A conformance runner that can report
# vacuous green is worse than no runner.
#
# ===================== EXIT CONTRACT (#1092) =========================
#
# A caller that sees nothing but the exit code and the one-line summary can
# decide whether to roll. Every code below except 0 means DO NOT ROLL.
#
#   0  PASS         every in-scope lane reached a green terminal state
#                   (`passed`, or `check-only` for the win cross-compile
#                   tier). Lanes outside a forced `--lane` subset are
#                   reported `not-in-scope` and do NOT make the run green
#                   for those lanes -- the release caller must union the
#                   summaries from every fleet machine and refuse to roll
#                   while any lane has no green record anywhere.
#   1  FAIL         at least one lane is `failed` or `error`. That includes
#                   a real test failure, a non-zero lane exit, a check-only
#                   build that did not compile, a `--lane X` forced onto a
#                   host whose probe says X is unsupported, and -- the
#                   load-bearing case -- a lane that RAN BUT EXECUTED ZERO
#                   TESTS (the #645 vacuous-green trap). DO NOT ROLL.
#   2  USAGE        the invocation itself was wrong (unknown lane, missing
#                   argument, unwritable `--summary` path). NO LANE RAN and
#                   NO SUMMARY FILE IS WRITTEN. A release caller must treat
#                   a missing summary file exactly like exit 1: there is no
#                   evidence, so DO NOT ROLL.
#   3  COVERAGE-GAP every lane that ran is green, but at least one lane was
#                   SKIPPED ON A HOST WHOSE PROBE SAYS IT IS CAPABLE (status
#                   `skipped-capable`) -- e.g. the GTK lane on Darwin, which
#                   is capable but opt-in. Nothing is known to be broken;
#                   the run simply does not cover what it could have. DO NOT
#                   ROLL on this host's evidence alone: either re-run with
#                   `--lane <name>` to force the lane, or point at another
#                   machine's summary that covers it.
#   1  (trap)       any termination that did not reach a real exit point --
#                   a bash parse error, a `set -u` abort, a signal -- is
#                   forced to 1 by the EXIT trap (#933), never 0.
#
# Machine-readable summary (#1092): `--summary <path>` writes a JSON document
# (schema `vimcode.platform-conformance/1`) describing every one of the four
# lanes, including the ones this host did not run, so "which lane was not
# covered on this roll?" is answerable from a file rather than scrollback.
# `--summary -` writes the same JSON to stdout after the human matrix,
# preceded by the marker line `===PLATFORM-CONFORMANCE-JSON===`. The
# `--summary` path is validated (and truncated) BEFORE any lane runs, so a
# bad path fails in a second rather than after a full `cargo test`.
#
# Regardless of `--summary`, the last line of stdout is always a single
# greppable summary line:
#
#   PLATFORM_CONFORMANCE_SUMMARY schema=1 verdict=<pass|fail|coverage-gap|plan>
#     exit=<n> mode=<run|plan> host=<uname> lanes=<lane>:<status>,... \
#     tests_passed=<n> tests_failed=<n>
#
# (printed as one physical line). Lane statuses are exactly:
# `passed`, `failed`, `check-only`, `skipped`, `skipped-capable`, `error`,
# `not-in-scope`, and in `--print-plan` mode `plan:run` / `plan:check-only`.
#
# Lanes:
#
#   tui    always capable                    cargo test --no-default-features
#   gtk    `pkg-config --modversion gtk4`     cargo test
#          (opt-in, not auto-selected, on a Darwin host -- quartz pangocairo
#          renders 3 known pixel-probe tests differently than freetype; force
#          with `--lane gtk` to run it anyway. See docs/PLATFORM_CONFORMANCE.md.)
#   macos  `uname` = Darwin                   cargo test --lib --no-default-features --features macos
#   win    cargo-xwin on PATH                 cargo xwin test --target x86_64-pc-windows-msvc \
#          + WSL interop (full tier)            --no-default-features --features win
#          cargo-xwin on PATH, no interop     cargo xwin build --target x86_64-pc-windows-msvc \
#          (check-only tier)                    --no-default-features --features win
#
# The win lane's two tiers are why the matrix has a `check-only` state
# distinct from `passed`/`failed`/`skipped`: cross-compiling with cargo-xwin
# needs no Windows host, but *running* the resulting .exe does. A machine with
# cargo-xwin installed but no live Windows host reachable via WSL interop can
# still prove the win-feature code type-checks and links -- that is real
# signal, just not "the tests passed". Reporting it as `skipped` would throw
# that signal away; reporting it as `passed` would be exactly the vacuous
# green this script exists to prevent. Both win tiers set
# RUSTFLAGS="-C target-feature=+crt-static" themselves (never relying on
# ambient env) because the target Windows host has no vcruntime140.dll --
# without the static CRT the .exe dies before `main()` with no output at all,
# which reads as a passing no-op rather than a build/link problem.
#
# Usage:
#   scripts/platform-conformance.sh                 # probe host, run every supported lane
#   scripts/platform-conformance.sh --lane tui --lane gtk   # force a subset (repeatable)
#   scripts/platform-conformance.sh --summary conf.json     # machine-readable matrix
#   scripts/platform-conformance.sh --summary -             # ... on stdout instead
#   scripts/platform-conformance.sh --print-plan    # resolve + print the matrix, run nothing
#   scripts/platform-conformance.sh -h|--help
#
# Forcing a lane the host cannot support (`--lane <name>` where that lane's
# probe fails) is an ERROR, not a silent skip -- exits 1 with the probe's
# reason.
#
# Testing hooks (used by tests/platform_conformance.rs; not part of the
# documented interface, no compatibility promise beyond that suite):
#   PLATCONF_OVERRIDE_<LANE>="capable=0|1;auto=0|1;tier=full|checkonly;reason=<text>"
#       replaces that lane's real probe outright.
#   PLATCONF_CMD_<LANE>="<shell command>"        replaces the command run for
#       that lane's full tier (TUI, GTK, MACOS, WIN).
#   PLATCONF_CMD_WIN_CHECKONLY="<shell command>" replaces the win check-only
#       tier's command specifically.
#   PLATCONF_TEST_FORCE_UNHANDLED_EXIT=1         exits 0 immediately, bypassing
#       `finish`, to prove the EXIT trap forces failure on any termination
#       that didn't go through a real exit point (#933).
# <LANE> is the upper-cased lane name (TUI, GTK, MACOS, WIN).

set -uo pipefail

# #933: this script's entire contract is "a lane that didn't run is a
# failure, not a silent pass" -- so it must never itself terminate with
# status 0 by accident. Route every intentional exit through `finish` so
# `SCRIPT_DONE` is set right before it; the EXIT trap below then knows the
# difference between "we reached a real exit point" and "bash died out from
# under us" (a parse error, an unbound-variable abort under `set -u`, a
# killing signal, ...) and forces the latter non-zero even if the aborting
# command's own status happened to be 0.
SCRIPT_DONE=0
finish() {
    SCRIPT_DONE=1
    exit "${1:-0}"
}
on_exit() {
    local status=$?
    if [ "$SCRIPT_DONE" -ne 1 ]; then
        echo "error: ${BASH_SOURCE[0]}: terminated unexpectedly before completing (status $status) -- forcing failure (#933)" >&2
        exit 1
    fi
}
trap on_exit EXIT

if [ -n "${PLATCONF_TEST_FORCE_UNHANDLED_EXIT:-}" ]; then
    # Test-only hook (tests/platform_conformance.rs): simulate a bash abort
    # or an accidental bare `exit 0` that terminates the script without
    # going through `finish`, so the EXIT trap's guard (#933) can be
    # exercised deterministically on any host's bash, without needing to
    # reproduce an actual bash-3.2 crash in every CI environment.
    exit 0
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Exit codes, named so the contract above is greppable in the code too.
EXIT_PASS=0
EXIT_FAIL=1
EXIT_USAGE=2
EXIT_COVERAGE_GAP=3

SUMMARY_SCHEMA="vimcode.platform-conformance/1"

usage() {
    cat <<'USAGE'
scripts/platform-conformance.sh -- the four-lane driver-tier conformance runner
(#926, gateable per #1092). Full documentation: docs/PLATFORM_CONFORMANCE.md

Usage:
  scripts/platform-conformance.sh                        probe host, run every supported lane
  scripts/platform-conformance.sh --lane tui --lane gtk  force a subset (repeatable)
  scripts/platform-conformance.sh --summary conf.json    write the machine-readable matrix
  scripts/platform-conformance.sh --summary -            ... to stdout instead
  scripts/platform-conformance.sh --print-plan           resolve + print the matrix, run nothing
  scripts/platform-conformance.sh -h|--help

Exit contract (anything but 0 means DO NOT ROLL):
  0  pass          every in-scope lane green (`passed` / `check-only`)
  1  fail          a lane failed, errored, or ran ZERO tests (#645 vacuous green);
                   also any unexpected termination (#933)
  2  usage         bad invocation -- no lane ran, no summary file written
  3  coverage-gap  all green, but a lane the probe calls capable was skipped
                   (`skipped-capable`) -- this host's run does not cover it

Lane statuses: passed, failed, check-only, skipped, skipped-capable, error,
not-in-scope (and plan:run / plan:check-only under --print-plan).
USAGE
}

LANE_ORDER=(tui gtk macos win)
# Bash 3.2 (stock macOS, frozen at the last GPLv2 release) has no associative
# arrays -- `declare -A` needs bash 4.0. Track the forced-lane set as a
# space-delimited string instead, matched with a `case` glob (#933).
FORCED_LANES=""
PRINT_PLAN=0
SUMMARY_PATH=""

is_forced() {
    case " $FORCED_LANES " in
        *" $1 "*) return 0 ;;
        *) return 1 ;;
    esac
}

lane_index() {
    case "$1" in
        tui) echo 0 ;;
        gtk) echo 1 ;;
        macos) echo 2 ;;
        win) echo 3 ;;
    esac
}

while [ $# -gt 0 ]; do
    case "$1" in
        --lane)
            name="${2:-}"
            if [ -z "$name" ]; then
                echo "error: --lane requires an argument" >&2
                finish "$EXIT_USAGE"
            fi
            valid=0
            for l in "${LANE_ORDER[@]}"; do
                [ "$l" = "$name" ] && valid=1
            done
            if [ "$valid" -eq 0 ]; then
                echo "error: unknown lane '$name' (known lanes: ${LANE_ORDER[*]})" >&2
                finish "$EXIT_USAGE"
            fi
            FORCED_LANES="$FORCED_LANES $name"
            shift 2
            ;;
        --summary)
            SUMMARY_PATH="${2:-}"
            if [ -z "$SUMMARY_PATH" ]; then
                echo "error: --summary requires a path (or '-' for stdout)" >&2
                finish "$EXIT_USAGE"
            fi
            shift 2
            ;;
        --print-plan) PRINT_PLAN=1; shift ;;
        -h|--help) usage; finish "$EXIT_PASS" ;;
        *) echo "error: unknown argument: $1" >&2; usage >&2; finish "$EXIT_USAGE" ;;
    esac
done

# #1092: validate the summary destination BEFORE running a single lane. A
# release caller passing an unwritable path should find out in a second, not
# after a full `cargo test` has already burned ten minutes and then lost its
# only machine-readable record.
if [ -n "$SUMMARY_PATH" ] && [ "$SUMMARY_PATH" != "-" ]; then
    # The subshell keeps bash's own redirection diagnostic off stderr, so the
    # caller sees exactly one error line (ours, naming the contract).
    if ! ( : >"$SUMMARY_PATH" ) 2>/dev/null; then
        echo "error: --summary path is not writable: $SUMMARY_PATH" >&2
        finish "$EXIT_USAGE"
    fi
fi

SCOPE=()
if [ -n "$FORCED_LANES" ]; then
    for l in "${LANE_ORDER[@]}"; do
        is_forced "$l" && SCOPE+=("$l")
    done
else
    SCOPE=("${LANE_ORDER[@]}")
fi
FORCE_MODE=$([ -n "$FORCED_LANES" ] && echo 1 || echo 0)

# --- probing -----------------------------------------------------------

# Globals a probe_* function fills in.
CAPABLE=0
AUTO=0
REASON=""
TIER="full"

apply_override() {
    local lane_upper="$1"
    local var="PLATCONF_OVERRIDE_${lane_upper}"
    local spec="${!var:-}"
    [ -z "$spec" ] && return 1

    CAPABLE=1
    AUTO=1
    REASON=""
    TIER="full"
    local IFS=';'
    local part key val
    for part in $spec; do
        key="${part%%=*}"
        val="${part#*=}"
        case "$key" in
            capable) CAPABLE="$val" ;;
            auto) AUTO="$val" ;;
            reason) REASON="$val" ;;
            tier) TIER="$val" ;;
        esac
    done
    return 0
}

probe_tui() {
    apply_override TUI && return
    CAPABLE=1; AUTO=1; REASON=""; TIER="full"
}

probe_gtk() {
    apply_override GTK && return
    if command -v pkg-config >/dev/null 2>&1 && pkg-config --modversion gtk4 >/dev/null 2>&1; then
        CAPABLE=1
        TIER="full"
        if [ "$(uname)" = "Darwin" ]; then
            AUTO=0
            REASON="opt-in on Darwin: quartz pangocairo rasterises glyphs differently than freetype, so 3 pixel/paint probes are known-red there (vimcode#926) -- force with --lane gtk"
        else
            AUTO=1
            REASON=""
        fi
    else
        CAPABLE=0
        AUTO=0
        TIER="full"
        REASON="pkg-config --modversion gtk4 failed: gtk4 dev libs not found"
    fi
}

probe_macos() {
    apply_override MACOS && return
    TIER="full"
    if [ "$(uname)" = "Darwin" ]; then
        CAPABLE=1; AUTO=1; REASON=""
    else
        CAPABLE=0; AUTO=0
        REASON="host is not Darwin (uname: $(uname))"
    fi
}

probe_win() {
    apply_override WIN && return
    if ! command -v cargo-xwin >/dev/null 2>&1; then
        CAPABLE=0; AUTO=0; TIER="full"
        REASON="cargo-xwin not found on PATH"
        return
    fi
    CAPABLE=1
    AUTO=1
    if [ -r /proc/version ] && grep -qi microsoft /proc/version 2>/dev/null; then
        TIER="full"
        REASON=""
    else
        TIER="checkonly"
        REASON="cargo-xwin present but no WSL interop detected -- cross-compiling only, not executing on a Windows host"
    fi
}

probe_lane() {
    case "$1" in
        tui) probe_tui ;;
        gtk) probe_gtk ;;
        macos) probe_macos ;;
        win) probe_win ;;
    esac
}

# --- commands ------------------------------------------------------------

command_for() {
    local lane="$1" tier="$2"
    local lane_upper
    lane_upper="$(printf '%s' "$lane" | tr '[:lower:]' '[:upper:]')"

    if [ "$lane" = "win" ] && [ "$tier" = "checkonly" ]; then
        local override="${PLATCONF_CMD_WIN_CHECKONLY:-}"
        if [ -n "$override" ]; then
            printf '%s' "$override"
            return
        fi
        printf '%s' "RUSTFLAGS=\"-C target-feature=+crt-static\" cargo xwin build --target x86_64-pc-windows-msvc --no-default-features --features win"
        return
    fi

    local override_var="PLATCONF_CMD_${lane_upper}"
    local override="${!override_var:-}"
    if [ -n "$override" ]; then
        printf '%s' "$override"
        return
    fi

    case "$lane" in
        tui) printf '%s' "cargo test --no-default-features" ;;
        gtk) printf '%s' "cargo test" ;;
        macos) printf '%s' "cargo test --lib --no-default-features --features macos" ;;
        win) printf '%s' "RUSTFLAGS=\"-C target-feature=+crt-static\" cargo xwin test --target x86_64-pc-windows-msvc --no-default-features --features win" ;;
    esac
}

# Sum every `test result: <ok|FAILED>. N passed; M failed` line cargo prints
# (one per test binary -- lib, bins, each integration-test crate). Echoes
# "<passed> <failed> <lines>".
parse_results() {
    local output="$1"
    local passed=0 failed=0 lines=0
    local line
    while IFS= read -r line; do
        if [[ "$line" =~ test\ result:\ [A-Za-z]+\.\ ([0-9]+)\ passed\;\ ([0-9]+)\ failed ]]; then
            passed=$((passed + BASH_REMATCH[1]))
            failed=$((failed + BASH_REMATCH[2]))
            lines=$((lines + 1))
        fi
    done <<<"$output"
    echo "$passed $failed $lines"
}

# --- run -------------------------------------------------------------

# Parallel arrays indexed by position in LANE_ORDER (bash 3.2: no `declare
# -A`). Every one of the four lanes has an entry even when a forced `--lane`
# subset means it was never probed -- that is the whole point of the
# machine-readable summary: "which lane was not covered on this roll?" must
# be answerable from the record, not from remembering what was typed.
LANE_STATUS=(not-in-scope not-in-scope not-in-scope not-in-scope)
LANE_DETAIL=("not probed: outside the forced --lane subset" \
             "not probed: outside the forced --lane subset" \
             "not probed: outside the forced --lane subset" \
             "not probed: outside the forced --lane subset")
LANE_CAPABLE=(null null null null)
LANE_AUTO=(null null null null)
LANE_TIER=("" "" "" "")
LANE_CMD=("" "" "" "")
LANE_PASSED=(null null null null)
LANE_FAILED=(null null null null)
LANE_BINARIES=(null null null null)

OVERALL_FAILURE=0
COVERAGE_GAP=0

record() {
    # record <index> <status> <detail>
    LANE_STATUS[$1]="$2"
    LANE_DETAIL[$1]="$3"
}

run_lane() {
    local lane="$1"
    local idx
    idx="$(lane_index "$lane")"
    probe_lane "$lane"
    local capable="$CAPABLE" auto="$AUTO" reason="$REASON" tier="$TIER"
    local forced=0
    is_forced "$lane" && forced=1

    LANE_CAPABLE[$idx]=$([ "$capable" -eq 1 ] && echo true || echo false)
    LANE_AUTO[$idx]=$([ "$auto" -eq 1 ] && echo true || echo false)
    LANE_TIER[$idx]="$tier"

    if [ "$capable" -ne 1 ]; then
        if [ "$forced" -eq 1 ]; then
            record "$idx" "error" "forced but unsupported: $reason"
            OVERALL_FAILURE=1
        else
            record "$idx" "skipped" "$reason"
        fi
        return
    fi

    if [ "$forced" -ne 1 ] && [ "$auto" -ne 1 ]; then
        # #1092: capable, but not auto-selected (a deliberate policy opt-out
        # such as GTK on Darwin). This is NOT the same as `skipped` -- the
        # host could have covered the lane and didn't, which is a coverage
        # gap a release caller must refuse to roll through (exit 3), rather
        # than an honest "this machine cannot run that lane at all".
        record "$idx" "skipped-capable" "$reason"
        COVERAGE_GAP=1
        return
    fi

    local cmd
    cmd="$(command_for "$lane" "$tier")"
    LANE_CMD[$idx]="$cmd"

    if [ "$PRINT_PLAN" -eq 1 ]; then
        if [ "$tier" = "checkonly" ]; then
            record "$idx" "plan:check-only" "$cmd"
        else
            record "$idx" "plan:run" "$cmd"
        fi
        return
    fi

    local output exit_code
    output="$(eval "$cmd" 2>&1)"
    exit_code=$?
    echo "----- $lane: $cmd -----"
    echo "$output"
    echo "----- end $lane (exit $exit_code) -----"

    if [ "$tier" = "checkonly" ]; then
        if [ "$exit_code" -eq 0 ]; then
            local detail="compiled ok; not executed (no WSL interop to a Windows host)"
            [ -n "$reason" ] && detail="$reason; $detail"
            record "$idx" "check-only" "$detail"
        else
            record "$idx" "failed" "build failed (exit $exit_code)"
            OVERALL_FAILURE=1
        fi
        return
    fi

    read -r passed failed lines <<<"$(parse_results "$output")"
    LANE_PASSED[$idx]="$passed"
    LANE_FAILED[$idx]="$failed"
    LANE_BINARIES[$idx]="$lines"
    if [ "$exit_code" -ne 0 ]; then
        record "$idx" "failed" "exit code $exit_code ($passed passed, $failed failed)"
        OVERALL_FAILURE=1
    elif [ "$lines" -eq 0 ]; then
        record "$idx" "failed" "0 'test result:' lines parsed from output -- vacuous pass guard (#926)"
        OVERALL_FAILURE=1
    elif [ "$failed" -gt 0 ]; then
        record "$idx" "failed" "$failed failed ($passed passed across $lines binaries)"
        OVERALL_FAILURE=1
    elif [ "$passed" -eq 0 ]; then
        record "$idx" "failed" "0 tests executed across $lines binaries -- vacuous pass guard (#926)"
        OVERALL_FAILURE=1
    else
        record "$idx" "passed" "$passed passed across $lines binaries"
    fi
}

for lane in "${SCOPE[@]}"; do
    run_lane "$lane"
done

# --- verdict ------------------------------------------------------------

MODE=$([ "$PRINT_PLAN" -eq 1 ] && echo plan || echo run)

if [ "$OVERALL_FAILURE" -eq 1 ]; then
    EXIT_CODE="$EXIT_FAIL"
    VERDICT="fail"
elif [ "$PRINT_PLAN" -eq 1 ]; then
    # Nothing ran, so nothing is proven either way; a plan is never a gate
    # result. The verdict says so explicitly rather than borrowing "pass".
    EXIT_CODE="$EXIT_PASS"
    VERDICT="plan"
elif [ "$COVERAGE_GAP" -eq 1 ]; then
    EXIT_CODE="$EXIT_COVERAGE_GAP"
    VERDICT="coverage-gap"
else
    EXIT_CODE="$EXIT_PASS"
    VERDICT="pass"
fi

# --- report ------------------------------------------------------------

echo
printf '%-8s %-16s %s\n' "LANE" "STATUS" "DETAIL"
for lane in "${SCOPE[@]}"; do
    i="$(lane_index "$lane")"
    printf '%-8s %-16s %s\n' "$lane" "${LANE_STATUS[$i]}" "${LANE_DETAIL[$i]}"
done

if [ "$FORCE_MODE" -eq 1 ]; then
    echo
    echo "(forced lane subset: ${SCOPE[*]} -- lanes outside this set were not probed)"
fi

# --- machine-readable summary (#1092) ------------------------------------

json_escape() {
    printf '%s' "$1" | LC_ALL=C tr -d '[:cntrl:]' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

json_str_or_null() {
    if [ -z "$1" ]; then
        printf 'null'
    else
        printf '"%s"' "$(json_escape "$1")"
    fi
}

json_list() {
    local first=1 item
    for item in "$@"; do
        [ "$first" -eq 0 ] && printf ', '
        printf '"%s"' "$(json_escape "$item")"
        first=0
    done
}

TOTAL_PASSED=0
TOTAL_FAILED=0
TOTAL_BINARIES=0
LANE_PAIRS=""
idx=0
for lane in "${LANE_ORDER[@]}"; do
    [ -n "$LANE_PAIRS" ] && LANE_PAIRS="$LANE_PAIRS,"
    LANE_PAIRS="$LANE_PAIRS$lane:${LANE_STATUS[$idx]}"
    case "${LANE_PASSED[$idx]}" in
        null) ;;
        *)
            TOTAL_PASSED=$((TOTAL_PASSED + LANE_PASSED[idx]))
            TOTAL_FAILED=$((TOTAL_FAILED + LANE_FAILED[idx]))
            TOTAL_BINARIES=$((TOTAL_BINARIES + LANE_BINARIES[idx]))
            ;;
    esac
    idx=$((idx + 1))
done

emit_summary_json() {
    local i=0 lane
    printf '{\n'
    printf '  "schema": "%s",\n' "$SUMMARY_SCHEMA"
    printf '  "generated_at": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '  "mode": "%s",\n' "$MODE"
    printf '  "host": {"uname": "%s", "node": "%s"},\n' \
        "$(json_escape "$(uname)")" "$(json_escape "$(uname -n)")"
    printf '  "verdict": "%s",\n' "$VERDICT"
    printf '  "exit_code": %s,\n' "$EXIT_CODE"
    printf '  "scope": [%s],\n' "$(json_list "${SCOPE[@]}")"
    printf '  "forced": [%s],\n' "$(json_list $FORCED_LANES)"
    printf '  "totals": {"tests_passed": %s, "tests_failed": %s, "binaries": %s},\n' \
        "$TOTAL_PASSED" "$TOTAL_FAILED" "$TOTAL_BINARIES"
    printf '  "lanes": [\n'
    for lane in "${LANE_ORDER[@]}"; do
        local in_scope=false
        case " ${SCOPE[*]} " in
            *" $lane "*) in_scope=true ;;
        esac
        local forced=false
        is_forced "$lane" && forced=true
        [ "$i" -gt 0 ] && printf ',\n'
        printf '    {"lane": "%s", "status": "%s", "in_scope": %s, "forced": %s, "capable": %s, "auto_selected": %s, "tier": %s, "command": %s, "tests_passed": %s, "tests_failed": %s, "binaries": %s, "detail": %s}' \
            "$lane" "${LANE_STATUS[$i]}" "$in_scope" "$forced" \
            "${LANE_CAPABLE[$i]}" "${LANE_AUTO[$i]}" \
            "$(json_str_or_null "${LANE_TIER[$i]}")" \
            "$(json_str_or_null "${LANE_CMD[$i]}")" \
            "${LANE_PASSED[$i]}" "${LANE_FAILED[$i]}" "${LANE_BINARIES[$i]}" \
            "$(json_str_or_null "${LANE_DETAIL[$i]}")"
        i=$((i + 1))
    done
    printf '\n  ]\n}\n'
}

if [ -n "$SUMMARY_PATH" ] && [ "$SUMMARY_PATH" != "-" ]; then
    emit_summary_json >"$SUMMARY_PATH"
fi

echo
printf 'PLATFORM_CONFORMANCE_SUMMARY schema=1 verdict=%s exit=%s mode=%s host=%s lanes=%s tests_passed=%s tests_failed=%s\n' \
    "$VERDICT" "$EXIT_CODE" "$MODE" "$(uname)" "$LANE_PAIRS" "$TOTAL_PASSED" "$TOTAL_FAILED"

if [ "$SUMMARY_PATH" = "-" ]; then
    echo "===PLATFORM-CONFORMANCE-JSON==="
    emit_summary_json
fi

finish "$EXIT_CODE"
