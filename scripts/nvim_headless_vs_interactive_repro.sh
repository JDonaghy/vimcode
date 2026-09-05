#!/usr/bin/env bash
#
# #805: `tests/nvim_conformance.rs` uses a *headless* `nvim --headless -l
# script.lua` process as its conformance oracle (see `run_in_neovim` in that
# file). For most commands that's a faithful stand-in for real Vim. But for a
# specific family of window-relative commands (`H`/`M`/`L`, `G`, `gg`, `%`,
# `<C-b>`, `<C-f>`, `zz`/`zt`/`zb`/`z.`/`z-`, and anything chained after one of
# these), headless nvim silently disagrees with *interactive* Neovim: with no
# UI ever attached, it never validates `w_topline`/`w_botline`, so a command
# that reads "the window's top/bottom line" gets a stale value that defaults
# to the cursor's own current line — as if the window had never scrolled at
# all — instead of the real, already-computed scroll position.
#
# This script proves the divergence directly: for each of a handful of
# representative cases pulled straight out of `KNOWN_DEVIATIONS` in
# `tests/nvim_conformance.rs`, it runs the exact same buffer + starting
# cursor + keystrokes through:
#   (a) headless nvim, via the same feedkeys-and-dump-JSON approach the
#       conformance oracle uses, and
#   (b) real interactive nvim, driven inside a tmux pane with a genuine
#       80x24 terminal attached, so the window is actually redrawn.
# vimcode's own output for the same cases (from `page_up`/`page_down`/
# `scroll_cursor_center` in `src/core/engine/motions.rs`, cross-checked via
# `PROBE_FILTER=... cargo test --test nvim_conformance -- --nocapture` with
# the relevant label temporarily deleted from `KNOWN_DEVIATIONS`) matches (b)
# in every case below, never (a).
#
# Usage: scripts/nvim_headless_vs_interactive_repro.sh
# Requires: nvim, tmux, python3. Exits non-zero if any is missing, or if a
# comparison shows no divergence (meaning the theory above no longer holds).

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

# name | start_line | start_col | keys (nvim_replace_termcodes-compatible)
cases=(
    "scroll:C-b|60|1|<C-b>"
    "scroll:2<C-b>|60|1|2<C-b>"
    "scroll:5C-d C-d|1|1|5<C-d><C-d>"
    "scroll:G M|1|1|GM"
    "scroll:50% H|1|1|50%H"
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

run_interactive() {
    local start_line="$1" start_col="$2" keys="$3"
    local session="repro805_$$_$RANDOM" tmpf="$workdir/case.txt" out="$workdir/result_interactive.txt"
    local keyfile="$workdir/keys.bin"
    rm -f "$out"
    cp "$buf" "$tmpf"
    keys_to_bytes "$keys" >"$keyfile"
    tmux kill-session -t "$session" 2>/dev/null || true
    tmux new-session -d -s "$session" -x 80 -y 24
    tmux resize-window -t "$session" -x 80 -y 24
    tmux send-keys -t "$session" "nvim -u NONE -i NONE -n '$tmpf'" Enter
    sleep 0.6
    tmux send-keys -t "$session" ":set shiftwidth=4 expandtab tabstop=4 noswapfile" Enter
    sleep 0.2
    tmux send-keys -t "$session" ":call cursor($start_line,$start_col)" Enter
    sleep 0.2
    tmux load-buffer -b "repro805keys" "$keyfile"
    tmux paste-buffer -b "repro805keys" -t "$session"
    tmux delete-buffer -b "repro805keys" 2>/dev/null || true
    sleep 0.6
    tmux send-keys -t "$session" ":call writefile([line('.') . ',' . col('.')], '$out')" Enter
    sleep 0.4
    tmux send-keys -t "$session" Escape
    tmux send-keys -t "$session" ":q!" Enter
    sleep 0.3
    tmux kill-session -t "$session" 2>/dev/null || true
    if [ -f "$out" ]; then
        cat "$out"
    else
        echo "???"
    fi
}

echo "case | headless (oracle) | interactive (real Vim)"
echo "-----|--------------------|------------------------"
fail=0
for entry in "${cases[@]}"; do
    IFS='|' read -r name line col keys <<<"$entry"
    headless=$(run_headless "$line" "$col" "$keys")
    interactive=$(run_interactive "$line" "$col" "$keys")
    marker=""
    if [ "$headless" != "$interactive" ]; then
        marker="  <-- DIVERGES"
        fail=1
    fi
    printf '%-28s | %-18s | %-22s%s\n' "$name" "$headless" "$interactive" "$marker"
done

if [ "$fail" -eq 0 ]; then
    echo
    echo "No divergence found -- headless and interactive nvim agreed on every case."
    echo "(If you expected a divergence here, the headless-oracle theory in"
    echo "tests/nvim_conformance.rs's KNOWN_DEVIATIONS comment may no longer hold.)"
    exit 1
else
    echo
    echo "Headless nvim (the conformance oracle) disagrees with real interactive"
    echo "nvim on the cases marked above. vimcode's own values for these cases"
    echo "match the interactive column, not the headless one."
fi
