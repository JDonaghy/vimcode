#!/usr/bin/env bash
#
# #805: `tests/nvim_conformance.rs` uses a *headless* `nvim --headless -l
# script.lua` process as its conformance oracle (see `run_in_neovim` in that
# file). For most commands that's a faithful stand-in for real Vim. But with no
# UI ever attached, no redraw ever runs, so the window's scroll bookkeeping
# (`w_topline` / `w_botline` / `w_empty_rows`) is never validated between the
# keystrokes of a single `nvim_feedkeys()` burst. That shows up in two
# distinguishable ways, and this script demonstrates both:
#
#   Group A — window-relative *reads*. `H`/`M`/`L`, `<C-b>`, `<C-f>`,
#     `zz`/`zt`/`zb`/`z.`/`z-` all answer "where is the top/bottom/middle of
#     the window?". Headless nvim's topline silently collapses to the cursor's
#     own line, so these behave as if the window had never scrolled.
#
#   Group B — the *second and later* scroll command of one burst. A single
#     `<C-d>`/`<C-u>`/`<C-f>` is immune, because it moves the cursor by
#     exactly as much as it scrolls the window, so a wrong topline cancels out
#     of the cursor result. The next one in the same burst inherits the
#     un-revalidated `w_botline`/`w_empty_rows` the previous one left behind
#     and stops against stale state.
#
# For each case below it runs the exact same buffer + starting cursor +
# keystrokes through:
#   (a) headless nvim, via the same feedkeys-and-dump-JSON approach the
#       conformance oracle uses, and
#   (b) real interactive nvim, driven inside a tmux pane with a genuine
#       80x24 terminal attached, so the window is actually redrawn.
# Both sides report window height 22 and `'scroll'` 11 on the fixture below, so
# the comparison is apples-to-apples. vimcode's own output for the DIVERGE
# cases (from `page_up`/`page_down`/`scroll_cursor_center` in
# `src/core/engine/motions.rs`, cross-checked via `PROBE_FILTER=... cargo test
# --test nvim_conformance -- --nocapture` with `PROBE_VERBOSE=1`, which prints
# the expected-vs-actual for KNOWN_DEVIATIONS entries too) matches (b) in every
# case, never (a). The group-B numbers are additionally pinned by direct engine
# tests in `tests/new_vim_features.rs` (`test_ctrl_d_chain_*`,
# `test_ctrl_f_chain_*`).
#
# The AGREE cases are controls, not padding: a single `<C-d>`/`<C-f>` and a
# plain `j` must come out *identical* on both sides. If a control diverges the
# theory above is wrong (the oracle would be broken far more broadly than
# claimed) and the script fails loudly rather than reporting a happy result.
#
# Usage: scripts/nvim_headless_vs_interactive_repro.sh
# Requires: nvim, tmux, python3. Exits non-zero if any is missing, if an
# expected divergence has disappeared, or if a control case diverges.

set -euo pipefail

for bin in nvim tmux python3; do
    if ! command -v "$bin" >/dev/null 2>&1; then
        echo "SKIP: $bin not found on PATH" >&2
        exit 1
    fi
done

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

buf="$workdir/buf60.txt"
python3 - "$buf" <<'PY'
import sys
path = sys.argv[1]
lines = [f"L{i:02d} {chr(ord('a') + ((i - 1) % 26))}" for i in range(1, 61)]
open(path, "w").write("\n".join(lines) + "\n")
PY

# expectation | name | start_line | start_col | keys (nvim_replace_termcodes-compatible)
#
# "DIVERGE" = headless and interactive must disagree (the artifact).
# "AGREE"   = control; they must agree, or the theory is wrong.
cases=(
    # Group A — window-relative reads.
    "DIVERGE|scroll:C-b|60|1|<C-b>"
    "DIVERGE|scroll:2<C-b>|60|1|2<C-b>"
    "DIVERGE|scroll:G M|1|1|GM"
    "DIVERGE|scroll:50% H|1|1|50%H"
    # Group B — 2nd and later scroll command in one burst.
    "DIVERGE|scroll:C-d C-d|1|1|<C-d><C-d>"
    "DIVERGE|scroll:5C-d C-d|1|1|5<C-d><C-d>"
    "DIVERGE|scroll:C-d twice then C-u|1|1|<C-d><C-d><C-u>"
    "DIVERGE|scroll:C-f C-f|1|1|<C-f><C-f>"
    # Controls — a single scroll command, and a non-scrolling motion.
    "AGREE|control: single <C-d>|1|1|<C-d>"
    "AGREE|control: single <C-f>|1|1|<C-f>"
    "AGREE|control: 5<C-d>|1|1|5<C-d>"
    "AGREE|control: 22j|1|1|22j"
)

run_headless() {
    local start_line="$1" start_col="$2" keys="$3"
    python3 - "$buf" "$start_line" "$start_col" "$keys" "$workdir" <<'PY'
import subprocess, sys, os

buf, start_line, start_col, keys, workdir = sys.argv[1:6]

def lua_str(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"') + '"'

script_path = os.path.join(workdir, "probe.lua")
result_path = os.path.join(workdir, "result_headless.txt")
if os.path.exists(result_path):
    os.remove(result_path)

lua = f"""
vim.o.compatible = false
vim.o.shiftwidth = 4
vim.o.expandtab = true
vim.o.tabstop = 4
vim.o.undolevels = -1
vim.api.nvim_buf_set_lines(0, 0, -1, false, vim.fn.readfile({lua_str(buf)}))
vim.o.undolevels = 1000
vim.api.nvim_win_set_cursor(0, {{{start_line}, {int(start_col) - 1}}})
pcall(function() vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes({lua_str(keys)}, true, false, true), "ntx", false) end)
local pos = vim.api.nvim_win_get_cursor(0)
local f = io.open({lua_str(result_path)}, "w")
f:write(pos[1] .. "," .. (pos[2] + 1))
f:close()
vim.cmd("qa!")
"""
with open(script_path, "w") as f:
    f.write(lua)

subprocess.run(
    ["nvim", "--headless", "-u", "NONE", "-i", "NONE", "-l", script_path],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
try:
    with open(result_path) as f:
        print(f.read().strip())
except FileNotFoundError:
    print("???")
PY
}

# Translate the same `<C-x>` notation the case table uses into raw bytes, so
# it can be injected into the terminal verbatim (tmux `send-keys -l` sends
# its argument as *literal characters typed*, not key names -- `<C-b>` typed
# literally is five separate keystrokes, not one Ctrl-B; only a real 0x02
# byte is one Ctrl-B).
keys_to_bytes() {
    python3 - "$1" <<'PY'
import re, sys
keys = sys.argv[1]
out = []
i = 0
while i < len(keys):
    m = re.match(r"<C-(.)>", keys[i:])
    if m:
        out.append(chr(ord(m.group(1).lower()) - 96))
        i += m.end()
    else:
        out.append(keys[i])
        i += 1
sys.stdout.write("".join(out))
PY
}

# Poll for a file to appear (and be non-empty) rather than sleeping a fixed
# amount: a fixed sleep is both slower than it needs to be and liable to flake
# under load. $1 = path, $2 = timeout in seconds.
wait_for_file() {
    local path="$1" limit="${2:-15}" waited=0
    while [ ! -s "$path" ]; do
        if [ "$waited" -ge $((limit * 20)) ]; then
            return 1
        fi
        sleep 0.05
        waited=$((waited + 1))
    done
    return 0
}

run_interactive() {
    local start_line="$1" start_col="$2" keys="$3"
    local session="repro805_$$_$RANDOM" tmpf="$workdir/case.txt" out="$workdir/result_interactive.txt"
    local ready="$workdir/ready.txt" done_marker="$workdir/done.txt"
    local keyfile="$workdir/keys.bin"
    rm -f "$out" "$ready" "$done_marker"
    cp "$buf" "$tmpf"
    keys_to_bytes "$keys" >"$keyfile"
    tmux kill-session -t "$session" 2>/dev/null || true
    tmux new-session -d -s "$session" -x 80 -y 24
    tmux resize-window -t "$session" -x 80 -y 24
    tmux send-keys -t "$session" "nvim -u NONE -i NONE -n '$tmpf'" Enter
    # Do the whole setup in one command line and have nvim itself signal that
    # it is up and positioned, so the next step waits on a real ready signal
    # instead of a guessed sleep.
    tmux send-keys -t "$session" \
        ":set shiftwidth=4 expandtab tabstop=4 noswapfile | call cursor($start_line,$start_col) | call writefile(['ok'], '$ready')" Enter
    if ! wait_for_file "$ready"; then
        tmux kill-session -t "$session" 2>/dev/null || true
        echo "???"
        return
    fi
    tmux load-buffer -b "repro805keys" "$keyfile"
    tmux paste-buffer -b "repro805keys" -t "$session"
    tmux delete-buffer -b "repro805keys" 2>/dev/null || true
    # The pasted keys are consumed from the same input stream as this command,
    # so nvim cannot reach the `:call writefile(...)` until every one of them
    # has been processed -- the file appearing IS the "keys are done" signal.
    tmux send-keys -t "$session" \
        ":call writefile([line('.') . ',' . col('.')], '$out')" Enter
    local rc=0
    wait_for_file "$out" || rc=1
    tmux kill-session -t "$session" 2>/dev/null || true
    if [ "$rc" -eq 0 ]; then
        cat "$out"
    else
        echo "???"
    fi
}

printf '%-28s | %-10s | %-18s | %s\n' "case" "expected" "headless (oracle)" "interactive (real Vim)"
printf -- '-----------------------------|------------|--------------------|------------------------\n'
fail=0
for entry in "${cases[@]}"; do
    IFS='|' read -r expectation name line col keys <<<"$entry"
    headless=$(run_headless "$line" "$col" "$keys")
    interactive=$(run_interactive "$line" "$col" "$keys")
    marker=""
    if [ "$headless" != "$interactive" ]; then
        if [ "$expectation" = "DIVERGE" ]; then
            marker="  <-- DIVERGES (as expected)"
        else
            marker="  <-- CONTROL DIVERGED (theory broken!)"
            fail=1
        fi
    elif [ "$expectation" = "DIVERGE" ]; then
        marker="  <-- expected a divergence, got agreement"
        fail=1
    fi
    printf '%-28s | %-10s | %-18s | %-22s%s\n' "$name" "$expectation" "$headless" "$interactive" "$marker"
done

echo
if [ "$fail" -eq 0 ]; then
    echo "As expected: headless nvim (the conformance oracle) disagrees with real"
    echo "interactive nvim on every DIVERGE case and agrees on every control."
    echo "vimcode's own values for the DIVERGE cases match the interactive column,"
    echo "not the headless one -- see KNOWN_DEVIATIONS in tests/nvim_conformance.rs."
else
    echo "At least one case did not behave as the headless-oracle theory in"
    echo "tests/nvim_conformance.rs's KNOWN_DEVIATIONS comment predicts. Either the"
    echo "theory no longer holds or the local nvim/tmux differs from the one it was"
    echo "captured against (nvim 0.9.x, 80x24 tmux pane, window height 22)."
    exit 1
fi
