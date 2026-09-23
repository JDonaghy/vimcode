#!/usr/bin/env bash
#
# What this measures: does a *headless* `nvim --headless -l script.lua` answer
# window-relative questions the same way a *real interactive* nvim in an 80x24
# terminal does? For each case below it runs the same buffer + starting cursor
# + keystrokes through both and prints the two answers side by side.
#
# ## Why it exists, and why every case now says AGREE (#1008)
#
# It was written for #805, when `tests/nvim_conformance.rs` used the headless
# `-l` form as its oracle and a long list of `scroll:` labels sat in
# KNOWN_DEVIATIONS / HARNESS_LIMITED excused as artifacts of it. On nvim 0.9.x
# the two columns really did disagree, in two distinguishable ways:
#
#   Group A -- window-relative *reads* (`H`/`M`/`L`, `<C-b>`, `zz`/`zt`/`zb`):
#     headless nvim's topline silently collapsed to the cursor's own line, so
#     these behaved as if the window had never scrolled.
#
#   Group B -- the *second and later* scroll command of one `feedkeys` burst:
#     with no redraw in between, `<C-d>`/`<C-f>` inherited the previous
#     command's un-revalidated `w_botline`/`w_empty_rows`.
#
# Both are fixed upstream by 0.12.5 (the pinned fleet oracle), so on a current
# nvim **every case agrees** -- which is why they are all marked AGREE below
# and a divergence now fails the script. That is the useful direction to
# assert: #1008 replaced the conformance oracle with an attached-UI RPC
# session precisely so this class of artifact cannot come back silently, and
# if a future nvim reintroduces one, this script says so.
#
# The numbers in the right-hand column are also the ground truth several
# source comments cite -- `page_up` in `src/core/engine/motions.rs` and the
# `HARNESS_LIMITED` doc comment in `tests/nvim_conformance.rs` -- so keep it
# runnable even though it is not part of any test lane (it needs tmux).
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
# "AGREE"   = the two must agree. Every case is AGREE as of #1008 / nvim 0.12:
#             the headless artifacts this script was written to demonstrate are
#             fixed upstream. A "DIVERGE" marker is still understood, so a case
#             can be flipped back if a future nvim regresses one.
cases=(
    # Former Group A — window-relative reads.
    "AGREE|scroll:C-b|60|1|<C-b>"
    "AGREE|scroll:2<C-b>|60|1|2<C-b>"
    "AGREE|scroll:G M|1|1|GM"
    "AGREE|scroll:50% H|1|1|50%H"
    # Former Group B — 2nd and later scroll command in one burst.
    "AGREE|scroll:C-d C-d|1|1|<C-d><C-d>"
    "AGREE|scroll:5C-d C-d|1|1|5<C-d><C-d>"
    "AGREE|scroll:C-d twice then C-u|1|1|<C-d><C-d><C-u>"
    "AGREE|scroll:C-f C-f|1|1|<C-f><C-f>"
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
            marker="  <-- DIVERGED (headless artifact is back)"
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
    echo "Headless and interactive nvim agree on every case, including the eight"
    echo "that diverged on 0.9.x. The right-hand column is the ground truth cited"
    echo "by page_up() in src/core/engine/motions.rs and by the HARNESS_LIMITED"
    echo "doc comment in tests/nvim_conformance.rs."
else
    echo "At least one case behaved unexpectedly. If a former Group A/B case"
    echo "diverged again, a headless-oracle artifact has come back upstream --"
    echo "tests/nvim_conformance.rs drives an attached-UI RPC session (#1008) and"
    echo "is insulated from it, but the numbers quoted in the source comments"
    echo "would need re-measuring. Otherwise the local nvim/tmux differs from the"
    echo "geometry this assumes (80x24 pane, window height 22)."
    exit 1
fi
