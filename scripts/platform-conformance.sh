#!/usr/bin/env bash
#
# #926: one entrypoint that answers "do the native builds behave the same on
# every platform?" across the four driver-tier black-box harnesses (TUI, GTK,
# macOS, Win-GUI). This is NOT a per-PR CI gate and is NOT wired into the
# coordinator's Test stage -- it is an on-demand / release-time runner the
# operator invokes by hand on whichever fleet machine can cover which lane.
#
# The one rule that makes this worth building: a SKIPPED lane is a failure,
# not a pass. The script exits non-zero when:
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
#   scripts/platform-conformance.sh --print-plan    # resolve + print the matrix, run nothing
#   scripts/platform-conformance.sh -h|--help
#
# Forcing a lane the host cannot support (`--lane <name>` where that lane's
# probe fails) is an ERROR, not a silent skip -- exits non-zero with the
# probe's reason.
#
# Testing hooks (used by tests/platform_conformance.rs; not part of the
# documented interface, no compatibility promise beyond that suite):
#   PLATCONF_OVERRIDE_<LANE>="capable=0|1;auto=0|1;tier=full|checkonly;reason=<text>"
#       replaces that lane's real probe outright.
#   PLATCONF_CMD_<LANE>="<shell command>"        replaces the command run for
#       that lane's full tier (TUI, GTK, MACOS, WIN).
#   PLATCONF_CMD_WIN_CHECKONLY="<shell command>" replaces the win check-only
#       tier's command specifically.
# <LANE> is the upper-cased lane name (TUI, GTK, MACOS, WIN).

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

usage() {
    sed -n '3,49p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

LANE_ORDER=(tui gtk macos win)
declare -A FORCED=()
PRINT_PLAN=0

while [ $# -gt 0 ]; do
    case "$1" in
        --lane)
            name="${2:-}"
            if [ -z "$name" ]; then
                echo "error: --lane requires an argument" >&2
                exit 2
            fi
            valid=0
            for l in "${LANE_ORDER[@]}"; do
                [ "$l" = "$name" ] && valid=1
            done
            if [ "$valid" -eq 0 ]; then
                echo "error: unknown lane '$name' (known lanes: ${LANE_ORDER[*]})" >&2
                exit 2
            fi
            FORCED["$name"]=1
            shift 2
            ;;
        --print-plan) PRINT_PLAN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

SCOPE=()
if [ "${#FORCED[@]}" -gt 0 ]; then
    for l in "${LANE_ORDER[@]}"; do
        [ -n "${FORCED[$l]:-}" ] && SCOPE+=("$l")
    done
else
    SCOPE=("${LANE_ORDER[@]}")
fi
FORCE_MODE=$([ "${#FORCED[@]}" -gt 0 ] && echo 1 || echo 0)

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

LANE_STATUS=()
LANE_DETAIL=()
OVERALL_FAILURE=0

run_lane() {
    local lane="$1"
    probe_lane "$lane"
    local capable="$CAPABLE" auto="$AUTO" reason="$REASON" tier="$TIER"
    local forced=0
    [ -n "${FORCED[$lane]:-}" ] && forced=1

    if [ "$capable" -ne 1 ]; then
        if [ "$forced" -eq 1 ]; then
            LANE_STATUS+=("error")
            LANE_DETAIL+=("forced but unsupported: $reason")
            OVERALL_FAILURE=1
        else
            LANE_STATUS+=("skipped")
            LANE_DETAIL+=("$reason")
        fi
        return
    fi

    if [ "$forced" -ne 1 ] && [ "$auto" -ne 1 ]; then
        LANE_STATUS+=("skipped")
        LANE_DETAIL+=("$reason")
        return
    fi

    local cmd
    cmd="$(command_for "$lane" "$tier")"

    if [ "$PRINT_PLAN" -eq 1 ]; then
        if [ "$tier" = "checkonly" ]; then
            LANE_STATUS+=("plan:check-only")
        else
            LANE_STATUS+=("plan:run")
        fi
        LANE_DETAIL+=("$cmd")
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
            LANE_STATUS+=("check-only")
            local detail="compiled ok; not executed (no WSL interop to a Windows host)"
            [ -n "$reason" ] && detail="$reason; $detail"
            LANE_DETAIL+=("$detail")
        else
            LANE_STATUS+=("failed")
            LANE_DETAIL+=("build failed (exit $exit_code)")
            OVERALL_FAILURE=1
        fi
        return
    fi

    read -r passed failed lines <<<"$(parse_results "$output")"
    if [ "$exit_code" -ne 0 ]; then
        LANE_STATUS+=("failed")
        LANE_DETAIL+=("exit code $exit_code ($passed passed, $failed failed)")
        OVERALL_FAILURE=1
    elif [ "$lines" -eq 0 ]; then
        LANE_STATUS+=("failed")
        LANE_DETAIL+=("0 'test result:' lines parsed from output -- vacuous pass guard (#926)")
        OVERALL_FAILURE=1
    elif [ "$failed" -gt 0 ]; then
        LANE_STATUS+=("failed")
        LANE_DETAIL+=("$failed failed ($passed passed across $lines binaries)")
        OVERALL_FAILURE=1
    elif [ "$passed" -eq 0 ]; then
        LANE_STATUS+=("failed")
        LANE_DETAIL+=("0 tests executed across $lines binaries -- vacuous pass guard (#926)")
        OVERALL_FAILURE=1
    else
        LANE_STATUS+=("passed")
        LANE_DETAIL+=("$passed passed across $lines binaries")
    fi
}

for lane in "${SCOPE[@]}"; do
    run_lane "$lane"
done

# --- report ------------------------------------------------------------

echo
printf '%-8s %-14s %s\n' "LANE" "STATUS" "DETAIL"
i=0
for lane in "${SCOPE[@]}"; do
    printf '%-8s %-14s %s\n' "$lane" "${LANE_STATUS[$i]}" "${LANE_DETAIL[$i]}"
    i=$((i + 1))
done

if [ "$FORCE_MODE" -eq 1 ]; then
    echo
    echo "(forced lane subset: ${SCOPE[*]} -- lanes outside this set were not probed)"
fi

exit "$OVERALL_FAILURE"
