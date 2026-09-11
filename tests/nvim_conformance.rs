//! Neovim conformance tests — the repo's only *oracle-backed* Vim-behaviour suite.
//!
//! Each case defines an initial buffer, a cursor position (1-indexed line and
//! col — Neovim convention), an optional per-case `setup` snippet of Lua, and a
//! key sequence.  The same scenario is run through `nvim --headless` and through
//! `Engine`, and the resulting buffer + cursor are compared.  **Nothing here is
//! hand-authored**: Neovim is the oracle, so a case cannot encode the author's
//! misconception about what Vim does the way an expectation-based test can.
//!
//! ## Adding cases
//!
//! Append to the relevant per-area `CASES_*` array via the `c(..)` constructor
//! (or `cs(..)` when the case needs Lua `setup` to pin a Vim-vs-Neovim option
//! default such as `startofline`, `joinspaces`, `nrformats` or `smarttab` —
//! without that you cannot distinguish "vimcode differs from Vim" from "Neovim
//! differs from Vim").  The arrays are per-area purely for editability; the
//! runner flattens them.
//!
//! ## `KNOWN_DEVIATIONS` (#799)
//!
//! VimCode does not yet match Neovim on every case, so the suite ships a list of
//! labels that are *currently* expected to differ.  The gate is **bidirectional**:
//!
//!   * an unlisted label that fails → **regression**, test fails;
//!   * a listed label that starts passing → **fix landed**, test fails until the
//!     entry is deleted.
//!
//! That is what lets the full corpus land green today and shrink monotonically —
//! the list can only ever get shorter, and each Vim-compat fix is forced to prove
//! itself by deleting entries.
//!
//! Regenerate the list after an intentional behaviour change with:
//!
//! ```sh
//! CONFORMANCE_DUMP_DEVIATIONS=/tmp/dev.txt \
//!   cargo test --no-default-features --test nvim_conformance -- --nocapture
//! ```
//!
//! which writes the current failing set (already formatted as Rust string
//! literals) instead of asserting.  Never regenerate to paper over a regression:
//! the point of the list is that it does not grow.
//!
//! ## `HARNESS_LIMITED` (#875)
//!
//! A separate, smaller list next to `KNOWN_DEVIATIONS` for cases this *test*
//! cannot faithfully probe — a harness gap or a broken headless oracle, not a
//! vimcode bug. These are reported but never enter the bidirectional gate: they
//! can fail forever without being a regression, and cannot force an entry
//! deletion by passing. See the array's own doc comment for the two harness
//! gaps it currently covers (`run_in_vimcode` dropping a case's `setup`, and
//! the headless-scroll artifact).
//!
//! ## Debugging a single area
//!
//! `PROBE_FILTER=<label-substring>` restricts the run; `PROBE_VERBOSE=1` prints
//! passes too.  Failures are tagged `BUF`, `CUR` or `BUF+CUR` so you can tell a
//! wrong edit from a wrong final cursor at a glance.
//!
//! ## Harness fidelity — do not "simplify" these away
//!
//! Each of the following was silencing real failures before it was added:
//!
//! | Detail | Why |
//! |---|---|
//! | `vim.o.undolevels = -1` around the fixture write, restored to `1000` | `nvim_buf_set_lines` is itself an undo step, so `u` undid the *fixture* and the buffer became `""` (41 spurious undo failures) |
//! | `feedkeys(.., "ntx")`, not `"nx"` | without `t`, keys count as mapping-sourced: `q` records nothing (every `@a` was a silent no-op on the nvim side) and undo is not synced between commands |
//! | capture `nvim_win_get_height(0)`, mirror via `engine.set_viewport_lines(rows)` | `H`/`M`/`L`/`<C-d>`/`zt` are meaningless with mismatched window heights |
//! | `engine.ensure_cursor_visible()` after placing the start cursor | `nvim_win_set_cursor` scrolls the window; a raw engine cursor write does not (12 spurious scroll failures) |
//! | pump `macro_playback_queue` after every key | the UI normally pumps it, so the harness must too, or `@a` never executes on the VimCode side |
//!
//! ## Requires `nvim` >= 0.12 on PATH — failing is the default (#865)
//!
//! A missing oracle is **not** a pass: it is 1,436 cases that did not run.  The
//! runner therefore *fails* on every lane — CI, a coordinator Test leg, a
//! developer laptop — when `nvim` is absent, unparseable, or older than
//! [`MIN_NVIM_VERSION`].  Skipping is still possible, but must be deliberate and
//! visible on the command line:
//!
//! ```sh
//! NVIM_CONFORMANCE_ALLOW_SKIP=1 cargo test
//! ```
//!
//! This replaced a `CI`-env-var-only guard (#795).  That guard was right about
//! the danger and wrong about the lane: `CI` is set by GitHub Actions but **not**
//! by the coordinator's Test stage, so on a fleet Test leg a host with no `nvim`
//! on its service-unit PATH was indistinguishable from 1,436 passing cases —
//! measured across three agent hosts, one of which could not see its own
//! Homebrew `nvim` at all.  Before #795, CI never installed nvim, so this suite's
//! "SKIP" was reported as `ok` on every PR — a regression in `d}`, `ciw`, `da"`,
//! etc. would have sailed through with a green check.  Do not reintroduce an
//! implicit skip, and do not remove the CI install steps
//! (`.github/workflows/ci.yml`, both jobs).
//!
//! The version floor exists for the same reason the `cs(..)` Lua `setup` hook
//! does: Neovim's own option defaults and headless behaviour move between
//! releases, so a verdict from 0.9.x is not comparable with one from 0.12.x.  The
//! fleet standard — every agent host and both CI jobs — is upstream stable
//! **v0.12.5**, which CI installs from a pinned release tarball rather than apt
//! (`ubuntu-24.04` apt ships 0.9.5, below the floor).  Every run prints the
//! resolved binary path and its version, so "which nvim produced this verdict" is
//! answerable from a log.
//!
//! ## Oracle version skew (#868, #865, #872)
//!
//! `KNOWN_DEVIATIONS` was captured against Neovim [`DEVIATIONS_ORACLE`], which
//! sits next to the list itself precisely because the two are one fact.  A
//! markedly different Neovim can legitimately disagree on a handful of labels;
//! that is oracle-version skew, not a regression, and is **not** a reason to
//! edit the list.
//!
//! That policy used to be advice the runner then contradicted: the "a listed
//! label now passes" direction panicked unconditionally, so a dev on a newer
//! Neovim was *forced* to delete entries that CI would immediately re-report as
//! regressions.  Concretely, going from the 0.9.x baseline to 0.12 thirty-seven
//! entries "pass" — all but one of them a `scroll:` label excused by the
//! headless-topline bug documented in Group A below, which upstream has since
//! fixed.  Measured with the same 60-line/22-row probe as that comment:
//!
//! ```text
//!     keys    0.9.x headless w0    0.12 headless w0    interactive w0
//!     22j            23 (== cursor)              2                  2
//!     G              60 (== cursor)             39                 39
//!     50%            30 (== cursor)              9                  9
//! ```
//!
//! So the runner applies the documented policy itself: the *fixed* direction is
//! enforcing when the running Neovim's major.minor matches [`DEVIATIONS_ORACLE`]
//! (or cannot be determined at all — failing closed), and is otherwise
//! downgraded to a printed advisory (see [`fixes_are_enforced`]).  The
//! **regression** direction and the stale-entry check stay fatal everywhere —
//! they are the ones that catch real bugs.
//!
//! #865 raised the floor to 0.12 and moved CI onto the pinned fleet oracle,
//! which left a gap: no lane ran [`DEVIATIONS_ORACLE`] (still 0.9 at that
//! point), so the fixed direction was advisory everywhere.  #867 then deleted
//! the 37 entries measured to pass under 0.12.5 (114 -> 77), and #872 closed
//! the remaining gap: regenerating the 77-entry list against 0.12.5 changed
//! nothing — same 77 labels, byte-for-byte, confirming #867's manual deletion
//! had already found everything 0.12 fixed — and bumped [`DEVIATIONS_ORACLE`]
//! to `(0, 12)` in that same commit.  The fixed direction is enforcing again on
//! every standard host and both CI jobs.

mod common;

use common::engine_with;
use serde::Deserialize;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use vimcode_core::Engine;

#[derive(Deserialize)]
struct NvimResult {
    buf: Vec<String>,
    line: usize,
    col: usize,
    rows: usize,
}

/// Unique suffix for this probe's temp files, so probes can run concurrently.
fn probe_id() -> String {
    static N: AtomicUsize = AtomicUsize::new(0);
    format!(
        "{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

fn run_in_neovim(
    lines: &[&str],
    cursor_line_1: usize,
    cursor_col_1: usize,
    keys: &str,
    setup: &str,
) -> Option<NvimResult> {
    let id = probe_id();
    let mut lua = String::new();
    lua.push_str("vim.o.compatible = false\n");
    lua.push_str("vim.o.shiftwidth = 4\n");
    lua.push_str("vim.o.expandtab = true\n");
    lua.push_str("vim.o.tabstop = 4\n");
    lua.push_str(setup);
    lua.push('\n');
    // `nvim_buf_set_lines` is itself an undo step, so without this the `undo:`
    // cases undo the *fixture* and compare against an empty buffer.
    lua.push_str("vim.o.undolevels = -1\n");
    lua.push_str("vim.api.nvim_buf_set_lines(0, 0, -1, false, {");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            lua.push_str(", ");
        }
        let escaped = line
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\t', "\\t");
        lua.push('"');
        lua.push_str(&escaped);
        lua.push('"');
    }
    lua.push_str("})\n");
    lua.push_str("vim.o.undolevels = 1000\n");
    lua.push_str(&format!(
        "vim.api.nvim_win_set_cursor(0, {{{}, {}}})\n",
        cursor_line_1,
        cursor_col_1.saturating_sub(1)
    ));
    let escaped_keys = keys.replace('\\', "\\\\").replace('"', "\\\"");
    // Mode "ntx", not "nx": without `t` the keys count as mapping-sourced, so
    // `q` records nothing and undo is not synced between commands.
    lua.push_str(&format!(
        "pcall(function() vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(\"{escaped_keys}\", true, false, true), \"ntx\", false) end)\n"
    ));
    let result_path = std::env::temp_dir().join(format!("vimcode_nvim_probe_{id}.json"));
    let result_path_str = result_path.to_string_lossy().replace('\\', "/");
    lua.push_str(&format!(
        "local buf = vim.api.nvim_buf_get_lines(0, 0, -1, false)\n\
         local pos = vim.api.nvim_win_get_cursor(0)\n\
         local rows = vim.api.nvim_win_get_height(0)\n\
         local result = vim.fn.json_encode({{buf = buf, line = pos[1], col = pos[2] + 1, rows = rows}})\n\
         local f = io.open(\"{result_path_str}\", \"w\")\n\
         f:write(result)\n\
         f:close()\n\
         vim.cmd(\"qa!\")\n"
    ));
    let script_path = std::env::temp_dir().join(format!("vimcode_nvim_probe_{id}.lua"));
    {
        let mut f = std::fs::File::create(&script_path).ok()?;
        f.write_all(lua.as_bytes()).ok()?;
    }
    let _ = std::fs::remove_file(&result_path);
    let output = std::process::Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg("NONE")
        .arg("-i")
        .arg("NONE")
        .arg("-l")
        .arg(script_path.to_string_lossy().as_ref())
        .output()
        .ok();
    // Judge the probe by whether it produced a parseable result, not by nvim's
    // exit status: some keys (`<C-n>` keyword completion, for one) leave nvim
    // exiting non-zero even though `qa!` ran and the result file is complete.
    let parsed = match output {
        Some(o) => {
            let parsed: Option<NvimResult> = std::fs::read_to_string(&result_path)
                .ok()
                .and_then(|json| serde_json::from_str(&json).ok());
            if parsed.is_none() && !o.status.success() {
                eprintln!("nvim stderr: {}", String::from_utf8_lossy(&o.stderr));
            }
            parsed
        }
        None => None,
    };
    let _ = std::fs::remove_file(&script_path);
    let _ = std::fs::remove_file(&result_path);
    parsed
}

// ---------------------------------------------------------------------------
// VimCode runner
// ---------------------------------------------------------------------------

/// Drain the macro playback queue — the UI pumps this every frame, so a harness
/// that doesn't will see `@a` do nothing at all.
fn pump(engine: &mut Engine) {
    let mut n = 0;
    while !engine.macro_playback_queue.is_empty() && n < 100_000 {
        engine.advance_macro_playback();
        n += 1;
    }
}

fn press_char(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), false);
    pump(engine);
}

fn press_special(engine: &mut Engine, name: &str) {
    engine.handle_key(name, None, false);
    pump(engine);
}

fn press_ctrl(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), true);
    pump(engine);
}

/// Parse and send a key sequence to the engine.
/// Supports `<Esc>`, `<CR>`, `<C-x>`, named keys, and literal characters.
fn send_keys(engine: &mut Engine, keys: &str) {
    let mut chars = keys.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '<' {
            let rest: String = chars.clone().collect();
            let has_closing = rest.contains('>');
            let starts_special = chars
                .peek()
                .map(|&c| c.is_ascii_uppercase() || c == 'C')
                .unwrap_or(false);
            if has_closing && starts_special {
                let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
                match name.as_str() {
                    "Esc" => press_special(engine, "Escape"),
                    "CR" | "Enter" => press_special(engine, "Return"),
                    "BS" => press_special(engine, "BackSpace"),
                    "Tab" => press_special(engine, "Tab"),
                    "Del" | "Delete" => press_special(engine, "Delete"),
                    "Up" => press_special(engine, "Up"),
                    "Down" => press_special(engine, "Down"),
                    "Left" => press_special(engine, "Left"),
                    "Right" => press_special(engine, "Right"),
                    "Home" => press_special(engine, "Home"),
                    "End" => press_special(engine, "End"),
                    n if n.starts_with("C-") => {
                        let c = n.chars().nth(2).unwrap();
                        press_ctrl(engine, c);
                    }
                    other => press_special(engine, other),
                }
            } else {
                press_char(engine, '<');
            }
        } else {
            press_char(engine, ch);
        }
    }
}

fn run_in_vimcode(
    lines: &[&str],
    cursor_line_1: usize,
    cursor_col_1: usize,
    keys: &str,
    rows: usize,
) -> (String, usize, usize) {
    let text = lines.join("\n");
    let mut engine = engine_with(&text);
    engine.settings.shift_width = 4;
    engine.settings.expand_tab = true;
    engine.settings.tabstop = 4;
    // Screen-relative motions (H/M/L, <C-d>, zt) are meaningless unless both
    // sides agree on the window height, so mirror nvim's.
    engine.set_viewport_lines(rows);
    engine.view_mut().cursor.line = cursor_line_1.saturating_sub(1);
    engine.view_mut().cursor.col = cursor_col_1.saturating_sub(1);
    // nvim_win_set_cursor scrolls the window to show the cursor; a raw engine
    // cursor write does not. Mirror that too.
    engine.ensure_cursor_visible();
    send_keys(&mut engine, keys);
    let buf = engine.buffer().to_string();
    let line = engine.view().cursor.line + 1;
    let col = engine.view().cursor.col + 1;
    (buf, line, col)
}

// ---------------------------------------------------------------------------
// Test cases — add new conformance checks to the per-area arrays below
// ---------------------------------------------------------------------------

struct Case {
    label: &'static str,
    lines: &'static [&'static str],
    cursor_line: usize,
    cursor_col: usize,
    keys: &'static str,
    setup: &'static str,
}
const fn c(
    label: &'static str,
    lines: &'static [&'static str],
    cursor_line: usize,
    cursor_col: usize,
    keys: &'static str,
) -> Case {
    Case {
        label,
        lines,
        cursor_line,
        cursor_col,
        keys,
        setup: "",
    }
}
const fn cs(
    label: &'static str,
    lines: &'static [&'static str],
    cursor_line: usize,
    cursor_col: usize,
    keys: &'static str,
    setup: &'static str,
) -> Case {
    Case {
        label,
        lines,
        cursor_line,
        cursor_col,
        keys,
        setup,
    }
}

const LONG: &[&str] = &[
    "L01 a", "L02 b", "L03 c", "L04 d", "L05 e", "L06 f", "L07 g", "L08 h", "L09 i", "L10 j",
    "L11 k", "L12 l", "L13 m", "L14 n", "L15 o", "L16 p", "L17 q", "L18 r", "L19 s", "L20 t",
    "L21 u", "L22 v", "L23 w", "L24 x", "L25 y", "L26 z", "L27 a", "L28 b", "L29 c", "L30 d",
    "L31 e", "L32 f", "L33 g", "L34 h", "L35 i", "L36 j", "L37 k", "L38 l", "L39 m", "L40 n",
    "L41 o", "L42 p", "L43 q", "L44 r", "L45 s", "L46 t", "L47 u", "L48 v", "L49 w", "L50 x",
    "L51 y", "L52 z", "L53 a", "L54 b", "L55 c", "L56 d", "L57 e", "L58 f", "L59 g", "L60 h",
];

// ─────────────────────────── A. operators × motions ───────────────────────────
const CASES_OP: &[Case] = &[
    c(
        "op:dw last word of line does not join",
        &["foo bar", "baz"],
        1,
        5,
        "dw",
    ),
    c("op:dw last word of buffer", &["foo bar"], 1, 5, "dw"),
    c("op:dw on whitespace", &["foo   bar"], 1, 4, "dw"),
    c("op:2dw across line end", &["a b", "c d"], 1, 3, "2dw"),
    c(
        "op:3dw crossing lines",
        &["one two", "three four"],
        1,
        5,
        "3dw",
    ),
    c("op:dw on punctuation", &["foo.bar baz"], 1, 4, "dw"),
    c("op:dw on empty line", &["", "abc"], 1, 1, "dw"),
    c("op:dw on blank-only line", &["   ", "abc"], 1, 1, "dw"),
    c(
        "op:dw last word with trailing spaces",
        &["foo bar   ", "baz"],
        1,
        5,
        "dw",
    ),
    c("op:cw on word", &["foo bar"], 1, 1, "cwX<Esc>"),
    c("op:cw on whitespace", &["foo   bar"], 1, 4, "cwX<Esc>"),
    c("op:cw at end of word", &["foo bar"], 1, 3, "cwX<Esc>"),
    c("op:cw on punctuation", &["foo.bar"], 1, 4, "cwX<Esc>"),
    c("op:2cw", &["one two three"], 1, 1, "2cwX<Esc>"),
    c("op:c2w", &["one two three"], 1, 1, "c2wX<Esc>"),
    c("op:cW", &["foo.bar baz"], 1, 1, "cWX<Esc>"),
    c(
        "op:cw last word of line",
        &["foo bar", "baz"],
        1,
        5,
        "cwX<Esc>",
    ),
    c(
        "op:c3w spans lines",
        &["one two", "three four"],
        1,
        5,
        "c3wX<Esc>",
    ),
    c("op:cw on single char word", &["a b"], 1, 1, "cwX<Esc>"),
    c("op:ce", &["foo bar"], 1, 1, "ceX<Esc>"),
    c("op:cb", &["foo bar"], 1, 5, "cbX<Esc>"),
    c("op:c$", &["foo bar"], 1, 4, "c$X<Esc>"),
    c("op:C", &["foo bar"], 1, 4, "CX<Esc>"),
    c("op:2C", &["foo bar", "baz", "qux"], 1, 4, "2CX<Esc>"),
    c("op:c0", &["foo bar"], 1, 5, "c0X<Esc>"),
    c("op:cc keeps indent", &["    foo", "bar"], 1, 6, "ccX<Esc>"),
    c("op:S keeps indent", &["    foo"], 1, 6, "SX<Esc>"),
    c("op:2cc", &["  a", "  b", "c"], 1, 1, "2ccX<Esc>"),
    c("op:cc on blank", &["", "x"], 1, 1, "ccX<Esc>"),
    cs(
        "op:cc noautoindent",
        &["    foo"],
        1,
        6,
        ":set noai<CR>ccX<Esc>",
        "vim.o.autoindent=false",
    ),
    c("op:s", &["hello"], 1, 2, "sX<Esc>"),
    c("op:3s", &["hello"], 1, 2, "3sX<Esc>"),
    c("op:3s beyond eol", &["hi"], 1, 2, "3sX<Esc>"),
    c("op:de at end of word", &["foo bar baz"], 1, 3, "de"),
    c("op:dge", &["foo bar baz"], 1, 6, "dge"),
    c("op:dgE", &["foo.x bar baz"], 1, 8, "dgE"),
    c("op:dE", &["foo.bar baz"], 1, 1, "dE"),
    c("op:dB", &["foo.bar baz"], 1, 9, "dB"),
    c("op:dW", &["foo.bar baz"], 1, 1, "dW"),
    c(
        "op:d) sentence",
        &["Hello world. Goodbye now. End."],
        1,
        1,
        "d)",
    ),
    c(
        "op:d( sentence",
        &["Hello world. Goodbye now. End."],
        1,
        14,
        "d(",
    ),
    c("op:das", &["Hello world. Goodbye now. End."], 1, 15, "das"),
    c("op:dis", &["Hello world. Goodbye now. End."], 1, 15, "dis"),
    c("op:d% on paren", &["foo(a, b) bar"], 1, 4, "d%"),
    c("op:d% before paren", &["foo(a, b) bar"], 1, 1, "d%"),
    c(
        "op:d% multiline braces",
        &["if (x) {", "  a;", "}", "b"],
        1,
        8,
        "d%",
    ),
    c("op:d/pat", &["foo bar baz"], 1, 1, "d/baz<CR>"),
    c("op:d/pat/e", &["foo bar baz"], 1, 1, "d/bar/e<CR>"),
    c("op:d?pat", &["foo bar baz"], 1, 9, "d?foo<CR>"),
    c("op:dn", &["foo bar foo baz"], 1, 1, "/foo<CR>ggdn"),
    c(
        "op:d/pat multiline",
        &["aaa", "bbb", "ccc"],
        1,
        2,
        "d/ccc<CR>",
    ),
    c(
        "op:d/pat/+1 linewise",
        &["aaa", "bbb", "ccc", "ddd"],
        1,
        1,
        "d/bbb/+1<CR>",
    ),
    c(
        "op:d/pat to col1 exclusive rule",
        &["aaa", "bbb", "ccc"],
        1,
        1,
        "d/ccc<CR>",
    ),
    c("op:d2f,", &["a,b,c,d"], 1, 1, "d2f,"),
    c("op:dt;", &["foo; bar; baz"], 1, 1, "dt;"),
    c("op:t; then ; repeat", &["foo; bar; baz"], 1, 1, "t;;"),
    c("op:t; then ; then ;", &["a;b;c;d"], 1, 1, "t;;;"),
    c("op:dt; then ;", &["foo; bar; baz"], 1, 1, "dt;;"),
    c("op:f, then ;", &["a,b,c,d"], 1, 1, "f,;"),
    c("op:f, then 2;", &["a,b,c,d"], 1, 1, "f,2;"),
    c("op:F, F, then ,", &["a,b,c,d"], 1, 7, "F,F,,"),
    c("op:t, ; ,", &["a,b,c,d"], 1, 1, "t,;,"),
    c("op:T, then ;", &["a,b,c,d"], 1, 7, "T,;"),
    c("op:f fails no move", &["abc"], 1, 1, "fz"),
    c("op:df fails no delete", &["abc def"], 1, 1, "dfz"),
    c("op:3f.", &["a.b.c.d"], 1, 1, "3f."),
    c("op:F at col1", &["abc"], 1, 1, "Fa"),
    c("op:x at eol", &["abc"], 1, 3, "x"),
    c("op:10x beyond eol", &["abcdef"], 1, 4, "10x"),
    c("op:x on empty line", &["", "x"], 1, 1, "x"),
    c("op:X at col1", &["abc"], 1, 1, "X"),
    c("op:3X", &["abcdef"], 1, 5, "3X"),
    c("op:5X beyond start", &["abcdef"], 1, 3, "5X"),
    c("op:dh at col1", &["abc", "def"], 2, 1, "dh"),
    c("op:dl at eol", &["abc"], 1, 3, "dl"),
    c("op:d3l beyond eol", &["abc"], 1, 2, "d3l"),
    c("op:d0 at col1", &["abc"], 1, 1, "d0"),
    c("op:d^", &["   abc def"], 1, 8, "d^"),
    c("op:d^ before first nonblank", &["   abc"], 1, 2, "d^"),
    c("op:dG mid", &["a", "b", "c", "d"], 2, 1, "dG"),
    c("op:dgg mid", &["a", "b", "c", "d"], 3, 1, "dgg"),
    c("op:d3G", &["a", "b", "c", "d"], 1, 1, "d3G"),
    c("op:dj last line", &["a", "b"], 2, 1, "dj"),
    c("op:dk first line", &["a", "b"], 1, 1, "dk"),
    c("op:d2j beyond end", &["a", "b", "c"], 2, 1, "d2j"),
    c("op:d5j beyond end", &["a", "b", "c"], 1, 1, "d5j"),
    c("op:d'a", &["a", "b", "c", "d"], 1, 1, "jjmaggd'a"),
    c("op:d`a", &["abc def", "ghi jkl"], 2, 4, "magg0d`a"),
    c("op:y`a cursor", &["abc def", "ghi jkl"], 2, 4, "magg0y`a"),
    c("op:yiw cursor", &["foo bar"], 1, 6, "yiw"),
    c("op:yb cursor", &["foo bar"], 1, 5, "yb"),
    c("op:yk cursor", &["a", "b"], 2, 1, "yk"),
    c("op:yj cursor", &["a", "b"], 1, 1, "yj"),
    c("op:y$ then P", &["foo bar"], 1, 5, "y$P"),
    c("op:yw at eol then p", &["foo bar", "baz"], 1, 5, "ywjp"),
    c("op:Y is linewise", &["foo bar", "baz"], 1, 5, "Yp"),
    c("op:yy 3p", &["a", "b"], 1, 1, "yy3p"),
    c("op:yw 3p", &["ab cd"], 1, 1, "yw3p"),
    c(
        "op:p linewise cursor first nonblank",
        &["  a", "b"],
        1,
        1,
        "yyjp",
    ),
    c("op:P linewise cursor", &["  a", "b"], 1, 1, "yyjP"),
    c(
        "op:p charwise multiline",
        &["abc", "def", "ghi"],
        1,
        2,
        "vjy$p",
    ),
    c(
        "op:P charwise multiline",
        &["abc", "def", "ghi"],
        1,
        2,
        "vjyP",
    ),
    c("op:gp linewise", &["a", "b"], 1, 1, "yygp"),
    c("op:gP linewise", &["a", "b"], 1, 1, "yygP"),
    c("op:gp charwise", &["abc"], 1, 1, "ylgp"),
    c("op:]p", &["    a", "b"], 1, 1, "yyj]p"),
    c("op:p charwise at eol", &["abc"], 1, 3, "ylp"),
    c("op:xp swap", &["abc"], 1, 1, "xp"),
    c("op:xp at eol", &["abc"], 1, 3, "xp"),
    c("op:ddp swap", &["a", "b", "c"], 1, 1, "ddp"),
    c("op:ddp last line", &["a", "b", "c"], 3, 1, "ddp"),
    c("op:dd last line cursor", &["  a", "  b", "  c"], 3, 1, "dd"),
    c("op:dd only line", &["abc"], 1, 1, "dd"),
    c("op:3dd more than lines", &["a", "b"], 1, 1, "3dd"),
    c("op:5dd from last line", &["a", "b", "c"], 3, 1, "5dd"),
    c("op:D on empty", &["", "a"], 1, 1, "D"),
    c("op:3D", &["abc", "def", "ghi", "jkl"], 1, 2, "3D"),
    c("op:J basic", &["a", "  b"], 1, 1, "J"),
    c(
        "op:J after period (nvim nojoinspaces)",
        &["end.", "next"],
        1,
        1,
        "J",
    ),
    cs(
        "op:J after period (vim joinspaces)",
        &["end.", "next"],
        1,
        1,
        "J",
        "vim.o.joinspaces=true",
    ),
    c("op:J next starts with )", &["foo(", "  )"], 1, 1, "J"),
    c("op:J next blank", &["a", "", "b"], 1, 1, "J"),
    c("op:J current ends with space", &["a ", "b"], 1, 1, "J"),
    c("op:3J", &["a", "b", "c", "d"], 1, 1, "3J"),
    c("op:J last line", &["a", "b"], 2, 1, "J"),
    c("op:5J count too big", &["a", "b"], 1, 1, "5J"),
    c("op:gJ", &["a", "  b"], 1, 1, "gJ"),
    c("op:3gJ", &["a", "b", "c"], 1, 1, "3gJ"),
    c("op:J cursor col", &["abc", "def"], 1, 1, "J"),
    c("op:J with tab indent", &["a", "\tb"], 1, 1, "J"),
    c("op:r", &["abc"], 1, 2, "rx"),
    c("op:3r", &["abcdef"], 1, 2, "3rx"),
    c("op:5r beyond eol", &["abc"], 1, 2, "5rx"),
    c("op:r<CR>", &["abc def"], 1, 4, "r<CR>"),
    c("op:3r<CR>", &["abcdef"], 1, 2, "3r<CR>"),
    c("op:R", &["abcdef"], 1, 2, "Rxy<Esc>"),
    c("op:R past eol", &["abc"], 1, 3, "Rxyz<Esc>"),
    c("op:R BS restores", &["abcdef"], 1, 2, "Rxyz<BS><BS><Esc>"),
    c("op:2R", &["abcdef"], 1, 1, "2Rxy<Esc>"),
    c("op:R <CR>", &["abcdef"], 1, 2, "Rx<CR>y<Esc>"),
    c("op:~", &["abc"], 1, 1, "~"),
    c("op:3~", &["abcdef"], 1, 1, "3~"),
    c("op:5~ past eol", &["abc"], 1, 2, "5~"),
    c("op:~ on non-letter", &["1a"], 1, 1, "~"),
    c("op:g~~ cursor", &["aBc dEf"], 1, 5, "g~~"),
    c("op:gUU", &["  abc"], 1, 3, "gUU"),
    c("op:guu", &["ABC"], 1, 3, "guu"),
    c("op:3guu", &["A", "B", "C", "D"], 1, 1, "3guu"),
    c("op:g~iw", &["aBc dEf"], 1, 6, "g~iw"),
    c("op:gUap", &["abc", "def", "", "ghi"], 1, 2, "gUap"),
    c("op:gu$", &["ABC DEF"], 1, 3, "gu$"),
    c("op:gUw punctuation", &["foo.bar"], 1, 1, "gUw"),
    c("op:3gUw", &["a b c d"], 1, 1, "3gUw"),
    c("op:gUe", &["abc def"], 1, 2, "gUe"),
    c("op:gUiw then w .", &["ab cd"], 1, 1, "gUiww."),
    c("op:g?? rot13", &["hello"], 1, 1, "g??"),
    c("op:g?w", &["hello world"], 1, 1, "g?w"),
    c("op:>>", &["a"], 1, 1, ">>"),
    c("op:3>>", &["a", "b", "c", "d"], 1, 1, "3>>"),
    c("op:3>> skips blank", &["a", "", "b"], 1, 1, "3>>"),
    c("op:>2j", &["a", "b", "c", "d"], 1, 1, ">2j"),
    c("op:>ip", &["a", "b", "", "c"], 1, 1, ">ip"),
    c("op:<< partial indent", &["  a"], 1, 1, "<<"),
    c("op:<< no indent", &["a"], 1, 1, "<<"),
    c("op:>> cursor", &["  abc"], 1, 4, ">>"),
    cs(
        "op:>> cursor sol",
        &["  abc"],
        1,
        4,
        ">>",
        "vim.o.startofline=true",
    ),
    c("op:>> then .", &["a"], 1, 1, ">>."),
    c("op:V2>", &["a", "b"], 1, 1, "V2>"),
    c("op:3>> j .", &["a", "b", "c", "d", "e"], 1, 1, "3>>j."),
    c("op:>> noet ts8", &["a"], 1, 1, ":set ts=8 noet<CR>>>"),
    c("op:>> noet ts4", &["a"], 1, 1, ":set ts=4 noet<CR>>>"),
    c("op:>>>> noet ts4", &["a"], 1, 1, ":set ts=4 noet<CR>>>>>"),
    c(
        "op:>> existing tab noet",
        &["\ta"],
        1,
        1,
        ":set ts=4 noet<CR>>>",
    ),
    c(
        "op:<< mixed tab space",
        &["\t  a"],
        1,
        1,
        ":set ts=4 noet<CR><<",
    ),
    c(
        "op:=G braces",
        &["int f() {", "int x;", "if (x) {", "y();", "}", "}"],
        1,
        1,
        "=G",
    ),
    c("op:=ip flat", &["  a", "      b", "c"], 1, 1, "=ip"),
    c("op:== single", &["      b"], 1, 1, "=="),
    c(
        "op:gqq tw20",
        &["one two three four five six seven eight"],
        1,
        1,
        ":set tw=20<CR>gqq",
    ),
    c(
        "op:gqip tw20",
        &[
            "one two three four five six seven eight",
            "nine ten eleven twelve",
        ],
        1,
        1,
        ":set tw=20<CR>gqip",
    ),
    c(
        "op:gqj joins short",
        &["one two", "three"],
        1,
        1,
        ":set tw=30<CR>gqj",
    ),
    c(
        "op:gwip cursor",
        &["one two three four five six seven eight"],
        1,
        5,
        ":set tw=20<CR>gwip",
    ),
    c(
        "op:gqq cursor",
        &["one two three four five six seven eight"],
        1,
        5,
        ":set tw=20<CR>gqq",
    ),
    c(
        "op:gqq tw0 no wrap",
        &["one two three four five six seven eight"],
        1,
        1,
        "gqq",
    ),
    c(
        "op:gqq indented",
        &["    one two three four five six"],
        1,
        1,
        ":set tw=20<CR>gqq",
    ),
    c(
        "op:Vgq",
        &["one two three four five six"],
        1,
        1,
        ":set tw=10<CR>Vgq",
    ),
    c("op:!Gsort", &["b", "a"], 1, 1, "!Gsort<CR>"),
    c("op:!!tr", &["abc"], 1, 1, "!!tr a-z A-Z<CR>"),
    c("op:o autoindent", &["    foo"], 1, 1, "obar<Esc>"),
    c("op:O autoindent", &["    foo"], 1, 1, "Obar<Esc>"),
    c(
        "op:o esc removes indent",
        &["    foo", "bar"],
        1,
        1,
        "o<Esc>",
    ),
    c(
        "op:o x CR esc no trailing ws",
        &["    foo"],
        1,
        1,
        "ox<CR><Esc>",
    ),
    c("op:O first line", &["a"], 1, 1, "Ob<Esc>"),
    c("op:3o", &["a"], 1, 1, "3ox<Esc>"),
    c("op:2O", &["a"], 1, 1, "2Ox<Esc>"),
    c("op:5i", &["a"], 1, 1, "5ix<Esc>"),
    c("op:3a", &["ab"], 1, 1, "3a-<Esc>"),
    c("op:3A", &["ab"], 1, 1, "3Ax<Esc>"),
    c("op:2I", &["  ab"], 1, 4, "2Ix<Esc>"),
    c("op:I indented", &["  ab"], 1, 4, "Ix<Esc>"),
    c("op:gI", &["  ab"], 1, 4, "gIx<Esc>"),
    c("op:A", &["ab"], 1, 1, "Ax<Esc>"),
    c("op:a at eol", &["ab"], 1, 2, "ax<Esc>"),
    c("op:i at eol esc", &["ab"], 1, 2, "i<Esc>"),
    c("op:i col1 esc", &["ab"], 1, 1, "i<Esc>"),
    c("op:A esc cursor", &["ab"], 1, 1, "A<Esc>"),
    c("op:3iab", &["a"], 1, 1, "3iab<Esc>"),
    c("op:2i with CR", &["a"], 1, 1, "2ix<CR><Esc>"),
    c("op:gi", &["abc", "def"], 1, 2, "ix<Esc>jgiy<Esc>"),
    c("op:cw on empty line", &["", "a"], 1, 1, "cwX<Esc>"),
    cs(
        "op:o noai",
        &["    foo"],
        1,
        1,
        ":set noai<CR>ox<Esc>",
        "vim.o.autoindent=false",
    ),
    c("op:x on tab", &["\ta"], 1, 1, "x"),
    c("op:A Tab noet", &["a"], 1, 1, ":set noet<CR>A<Tab>x<Esc>"),
    c("op:dvj charwise force", &["abc", "def"], 1, 2, "dvj"),
    c("op:dVw linewise force", &["abc def", "ghi"], 1, 1, "dVw"),
    c("op:dve exclusive force", &["abc def"], 1, 1, "dve"),
    c("op:dv$", &["abc def"], 1, 2, "dv$"),
    c(
        "op:d<C-v>j blockwise force",
        &["abcde", "fghij"],
        1,
        2,
        "d<C-v>j",
    ),
];

// ─────────────────────────── B. dot repeat ───────────────────────────
const CASES_DOT: &[Case] = &[
    c("dot:dw .", &["a b c d"], 1, 1, "dw."),
    c("dot:cw . next word", &["foo bar baz"], 1, 1, "cwX<Esc>w."),
    c("dot:x...", &["abcdef"], 1, 1, "x..."),
    c("dot:3x .", &["abcdefghij"], 1, 1, "3x."),
    c("dot:3x 2.", &["abcdefghij"], 1, 1, "3x2."),
    c("dot:x 3.", &["abcdefghij"], 1, 1, "x3."),
    c("dot:A; j .", &["a", "b"], 1, 1, "A;<Esc>j."),
    c("dot:ciw w .", &["foo bar"], 1, 1, "ciwX<Esc>w."),
    c("dot:dd .", &["a", "b", "c"], 1, 1, "dd."),
    c("dot:2dd .", &["a", "b", "c", "d", "e"], 1, 1, "2dd."),
    c("dot:2dd 3.", &["a", "b", "c", "d", "e", "f"], 1, 1, "2dd3."),
    c("dot:yyp .", &["a"], 1, 1, "yyp."),
    c("dot:yy3p .", &["a"], 1, 1, "yy3p."),
    c("dot:J .", &["a", "b", "c"], 1, 1, "J."),
    c("dot:~ .", &["abcd"], 1, 1, "~."),
    c("dot:rx l .", &["abcd"], 1, 1, "rxl."),
    c("dot:o .", &["a"], 1, 1, "ob<Esc>."),
    c("dot:O .", &["a"], 1, 1, "Ob<Esc>."),
    c("dot:vlld .", &["abcdefgh"], 1, 1, "vlld."),
    c("dot:Vjd .", &["a", "b", "c", "d", "e"], 1, 1, "Vjd."),
    c("dot:Vj> j .", &["a", "b", "c", "d"], 1, 1, "Vj>j."),
    c("dot:dap .", &["a", "", "b", "", "c"], 1, 1, "dap."),
    c("dot:ifoo .", &["ab"], 1, 1, "ifoo<Esc>."),
    c("dot:3Ax j .", &["a", "b"], 1, 1, "3Ax<Esc>j."),
    c("dot:3Ax j 2.", &["a", "b"], 1, 1, "3Ax<Esc>j2."),
    c("dot:cc j .", &["a", "b"], 1, 1, "ccX<Esc>j."),
    c("dot:s l .", &["abcd"], 1, 1, "sX<Esc>l."),
    c("dot:C j .", &["abc", "def"], 1, 2, "CX<Esc>j."),
    c("dot:ct, .", &["a,b,c"], 1, 1, "ct,X<Esc>ll."),
    c("dot:df. .", &["a.b.c.d"], 1, 1, "df.."),
    c("dot:x u .", &["abc"], 1, 1, "xu."),
    c("dot:R .", &["abcdef"], 1, 1, "Rxy<Esc>ll."),
    c("dot:diw . .", &["foo bar baz"], 1, 1, "diw.."),
    c("dot:& repeat sub", &["a a a", "a a"], 1, 1, ":s/a/b/<CR>j&"),
    c("dot:g&", &["a a", "a a"], 1, 1, ":s/a/b/g<CR>g&"),
    c("dot:@:", &["a", "b", "c"], 1, 1, ":d<CR>@:"),
    c("dot:yank not repeated", &["ab cd"], 1, 1, "xyw."),
    c("dot:\"ayy \"ap .", &["a", "b"], 1, 1, "\"ayyj\"ap."),
    c(
        "dot:\"1p . . increments",
        &["a", "b", "c", "d"],
        1,
        1,
        "dddddd\"1p..",
    ),
    c("dot:>ip .", &["a", "b", "", "c", "d"], 1, 1, ">ip}j."),
    c("dot:I .", &["a", "b"], 1, 1, "Ix<Esc>j."),
    c("dot:i<C-w> .", &["ab cd", "ef gh"], 1, 6, "i<C-w>X<Esc>j$."),
    c("dot:vec .", &["foo bar baz"], 1, 1, "vecX<Esc>w."),
    c(
        "dot:vjd . charwise",
        &["a1", "b2", "c3", "d4", "e5"],
        1,
        1,
        "vjd.",
    ),
    c(
        "dot:<C-v>jIx .",
        &["ab", "ab", "ab", "ab"],
        1,
        1,
        "<C-v>jIx<Esc>jj.",
    ),
    c("dot:cw with count 2.", &["a b c d e"], 1, 1, "cwX<Esc>w2."),
    c("dot:dfx count override", &["a.b.c.d.e"], 1, 1, "df.2."),
    c("dot:gUw .", &["ab cd"], 1, 1, "gUww."),
    c("dot:>> 2.", &["a"], 1, 1, ">>2."),
    c("dot:ofoo<CR>bar .", &["a"], 1, 1, "ofoo<CR>bar<Esc>."),
    c("dot:p charwise .", &["ab"], 1, 1, "ylp."),
    c("dot:xp .", &["abcd"], 1, 1, "xp."),
    c("dot:ciw then . at eol", &["ab cd"], 1, 1, "ciwX<Esc>$."),
];

// ─────────────────────────── C. undo ───────────────────────────
const CASES_UNDO: &[Case] = &[
    c("undo:ifoo u", &["ab"], 1, 1, "ifoo<Esc>u"),
    c("undo:two inserts u", &["ab"], 1, 1, "ifoo<Esc>ibar<Esc>u"),
    c("undo:xxx u", &["abcdef"], 1, 1, "xxxu"),
    c("undo:xxx uu", &["abcdef"], 1, 1, "xxxuu"),
    c("undo:xxx uu C-r", &["abcdef"], 1, 1, "xxxuu<C-r>"),
    c("undo:xxxx 3u", &["abcdef"], 1, 1, "xxxx3u"),
    c("undo:dw u cursor", &["foo bar baz"], 1, 5, "dwu"),
    c("undo:dd u cursor", &["a", "b", "c"], 2, 1, "ddu"),
    c("undo:G dd u cursor", &["a", "b", "c"], 1, 1, "Gddu"),
    c("undo:U", &["abcdef"], 1, 1, "xxxU"),
    c("undo:UU", &["abcdef"], 1, 1, "xxxUU"),
    c(
        "undo:insert with CR is one undo",
        &["ab"],
        1,
        1,
        "ihello<CR>world<Esc>u",
    ),
    c("undo:A xyz u cursor", &["abc"], 1, 1, "A xyz<Esc>u"),
    c("undo:cw u cursor", &["foo bar"], 1, 5, "cwX<Esc>u"),
    c("undo:C-g u splits", &["ab"], 1, 1, "ifoo<C-g>ubar<Esc>u"),
    c(
        "undo:arrow breaks undo",
        &["ab"],
        1,
        1,
        "ifoo<Left>bar<Esc>u",
    ),
    c("undo:u after p", &["a", "b"], 1, 1, "yyjpu"),
    c("undo:u after J", &["a", "b"], 1, 1, "Ju"),
    c("undo:u after >>", &["a"], 1, 1, ">>u"),
    c("undo:u after :s", &["a a"], 1, 1, ":s/a/b/g<CR>u"),
    c(
        "undo:u after :%s cursor",
        &["a", "a", "a"],
        3,
        1,
        ":%s/a/b/<CR>u",
    ),
    c("undo:u after visual d", &["abcdef"], 1, 2, "vlldu"),
    c("undo:u after macro", &["a", "b", "c"], 1, 1, "qaddq@au"),
    c("undo:u after .", &["abcdef"], 1, 1, "x.u"),
    c(
        "undo:C-r after new change noop",
        &["abcdef"],
        1,
        1,
        "xxuux<C-r>",
    ),
    c("undo:u after o cursor", &["a", "b"], 1, 1, "ox<Esc>u"),
    c("undo:u on unchanged", &["a"], 1, 1, "u"),
    c("undo:r u", &["abc"], 1, 2, "rxu"),
    c("undo:3rx u", &["abcdef"], 1, 2, "3rxu"),
    c(
        "undo:u restores cursor after :g",
        &["a", "b", "a"],
        1,
        1,
        ":g/a/d<CR>u",
    ),
    c("undo:cc u cursor", &["  foo", "bar"], 1, 3, "ccX<Esc>u"),
    c("undo:u after R", &["abcdef"], 1, 2, "Rxyz<Esc>u"),
    c("undo:u after ~", &["abc"], 1, 1, "~u"),
    c("undo:u after C-a", &["5"], 1, 1, "<C-a>u"),
    c("undo:u after <C-v>I", &["ab", "ab"], 1, 1, "<C-v>jIx<Esc>u"),
    c(
        "undo:2u after insert ×3",
        &["a"],
        1,
        1,
        "Ax<Esc>Ay<Esc>Az<Esc>2u",
    ),
    c(
        "undo:u after dd on last line cursor",
        &["a", "b", "c"],
        3,
        1,
        "ddu",
    ),
    c("undo:C-r cursor", &["a", "b", "c"], 2, 1, "ddu<C-r>"),
    c(
        "undo:undo insert then cursor col",
        &["hello"],
        1,
        3,
        "ixyz<Esc>u",
    ),
    c("undo:u after :m", &["a", "b", "c"], 1, 1, ":m$<CR>u"),
    c("undo:u after :t", &["a", "b"], 1, 1, ":t$<CR>u"),
    c(
        "undo:u after :normal",
        &["a", "b"],
        1,
        1,
        ":%normal Ax<CR>u",
    ),
];

// ─────────────────────────── D. registers ───────────────────────────
const CASES_REG: &[Case] = &[
    c("reg:\"ayy \"ap", &["a", "b"], 1, 1, "\"ayyj\"ap"),
    c(
        "reg:\"ayw \"Ayw \"ap",
        &["foo bar"],
        1,
        1,
        "\"ayww\"Ayw$\"ap",
    ),
    c(
        "reg:\"Ayy linewise append",
        &["a", "b"],
        1,
        1,
        "\"ayyj\"Ayy\"ap",
    ),
    c(
        "reg:\"Ayy onto charwise becomes linewise",
        &["foo", "bar"],
        1,
        1,
        "\"aywj\"Ayy\"ap",
    ),
    c("reg:dd \"1p", &["a", "b", "c"], 1, 1, "ddj\"1p"),
    c("reg:dd dd \"2p", &["a", "b", "c"], 1, 1, "dddd\"2p"),
    c("reg:dw goes to \"-", &["foo bar"], 1, 1, "dw$\"-p"),
    c(
        "reg:dw does not touch \"1",
        &["foo bar", "x"],
        1,
        1,
        "jddkdw\"1p",
    ),
    c(
        "reg:d/ goes to \"1",
        &["foo bar baz"],
        1,
        1,
        "d/baz<CR>$\"1p",
    ),
    c("reg:d% goes to \"1", &["(ab) cd"], 1, 1, "d%$\"1p"),
    c(
        "reg:dn goes to \"1",
        &["x ab x ab"],
        1,
        1,
        "/ab<CR>ggdn$\"1p",
    ),
    c("reg:yy dd \"0p", &["a", "b", "c"], 1, 1, "yyjdd\"0p"),
    c("reg:\"_dd then p", &["a", "b", "c"], 1, 1, "yyj\"_ddp"),
    c(
        "reg:\"add \"bdd \"ap \"bp",
        &["a", "b", "c"],
        1,
        1,
        "\"add\"bdd\"ap\"bp",
    ),
    c("reg:3\"ap", &["a", "b"], 1, 1, "\"ayy3\"ap"),
    c("reg:\"ayl 3\"ap", &["ab"], 1, 1, "\"ayl3\"ap"),
    c("reg:\". insert register", &["ab"], 1, 1, "ifoo<Esc>\".p"),
    c("reg:\": last cmd", &["a a"], 1, 1, ":s/a/b/<CR>\":p"),
    c("reg:\"/ last search", &["foo bar"], 1, 1, "/bar<CR>\"/P"),
    c("reg:i C-r a", &["foo bar"], 1, 1, "\"aywA<C-r>a<Esc>"),
    c("reg:i C-r \"", &["foo bar"], 1, 1, "ywA<C-r>\"<Esc>"),
    c("reg:i C-r 0", &["foo bar"], 1, 1, "ywA<C-r>0<Esc>"),
    c(
        "reg:i C-r a linewise",
        &["a", "b"],
        1,
        1,
        "\"ayyjA<C-r>a<Esc>",
    ),
    c("reg:\"_x then p", &["abc"], 1, 1, "yl\"_xp"),
    c(
        "reg:dd yy \"1p unchanged by yank",
        &["a", "b", "c"],
        1,
        1,
        "ddyy\"1p",
    ),
    c(
        "reg:\"adw does not set \"-",
        &["foo bar"],
        1,
        1,
        "\"adw\"-p",
    ),
    c("reg:\"ayw x \"ap", &["foo bar"], 1, 1, "\"aywx\"ap"),
    c("reg:\"add then \"1p", &["a", "b", "c"], 1, 1, "\"add\"1p"),
    c(
        "reg:\"Add appends",
        &["a", "b", "c"],
        1,
        1,
        "\"add\"Add\"ap",
    ),
    c(
        "reg:\"ayy \"ap count in visual",
        &["a", "b"],
        1,
        1,
        "\"ayyjV\"ap",
    ),
    c("reg:yiw viwp swaps", &["foo bar"], 1, 1, "yiwwviwp0P"),
    c("reg:viw\"_dP", &["foo bar"], 1, 1, "yiwwviw\"_dP"),
    c("reg:\"0 after visual y", &["a", "b"], 1, 1, "Vyjdd\"0p"),
    c("reg:\"- after x", &["abc"], 1, 1, "x$\"-p"),
    c("reg:\"- after s", &["abc"], 1, 1, "sZ<Esc>$\"-p"),
    c("reg:\"1 after cc", &["a", "b"], 1, 1, "ccX<Esc>j\"1p"),
    c("reg:\"- after cw", &["foo bar"], 1, 1, "cwX<Esc>$\"-p"),
    c(
        "reg:\"1p shifted by dd in visual",
        &["a", "b", "c"],
        1,
        1,
        "Vd\"1p",
    ),
    c(
        "reg:\"a in :normal",
        &["a", "b"],
        1,
        1,
        "\"ayy:normal \"ap<CR>",
    ),
    c("reg:\"= expr", &["a"], 1, 1, "\"=1+1<CR>p"),
    c("reg:C-r = in insert", &["a"], 1, 1, "A<C-r>=2*3<CR><Esc>"),
    c("reg:\"% file name empty", &["a"], 1, 1, "\"%p"),
    c(
        "reg:paste count charwise multiline",
        &["ab", "cd"],
        1,
        1,
        "vjy2p",
    ),
    c(
        "reg:p from \"1 then u then \"2p",
        &["a", "b", "c"],
        1,
        1,
        "dddd\"1pu\"2p",
    ),
];

// ─────────────────────────── E. macros ───────────────────────────
const CASES_MAC: &[Case] = &[
    c("mac:qaxjq @a", &["ab", "cd", "ef"], 1, 1, "qaxjq@a"),
    c(
        "mac:qaA! j q 2@a",
        &["a", "b", "c", "d"],
        1,
        1,
        "qaA!<Esc>jq2@a",
    ),
    c("mac:@a @@", &["a", "b", "c", "d"], 1, 1, "qaA!<Esc>jq@a@@"),
    c(
        "mac:10@a stops at failure",
        &["a,b", "c,d", "e f", "g,h"],
        1,
        1,
        "qa0f,xjq10@a",
    ),
    c("mac:qA append", &["ab", "cd", "ef"], 1, 1, "qaxqqAjq@a"),
    c(
        "mac:macro with insert",
        &["a", "b"],
        1,
        1,
        "qaIfoo <Esc>jq@a",
    ),
    c(
        "mac:macro with :s",
        &["a a", "a a"],
        1,
        1,
        "qa:s/a/b/<CR>jq@a",
    ),
    c(
        "mac:macro with search",
        &["x foo", "y foo", "z foo"],
        1,
        1,
        "qa/foo<CR>rXq@a",
    ),
    c("mac:\"ap shows macro", &["ab"], 1, 1, "qaxq\"ap"),
    c(
        "mac:recursive",
        &["a", "b", "c", "d"],
        1,
        1,
        "qaqqaA!<Esc>j@aq@a",
    ),
    c("mac:@a then .", &["abcd"], 1, 1, "qaxq@a."),
    c("mac:3@a", &["a", "b", "c", "d"], 1, 1, "qaddq3@a"),
    c("mac:count inside", &["a b c d e"], 1, 1, "qa2dwq@a"),
    c(
        "mac:macro with visual",
        &["ab", "cd", "ef"],
        1,
        1,
        "qavlUjq@a",
    ),
    c("mac:macro with dot", &["abc", "def"], 1, 1, "qax.jq@a"),
    c("mac:macro with undo", &["abc"], 1, 1, "qaxuq@a"),
    c(
        "mac:macro ending in insert",
        &["a", "b"],
        1,
        1,
        "qaAx<Esc>q@a",
    ),
    c(
        "mac:@@ after 2@a",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "qaA!<Esc>jq2@a@@",
    ),
    c(
        "mac:macro with ctrl-a",
        &["1", "1", "1"],
        1,
        1,
        "qa<C-a>jq2@a",
    ),
    c("mac:macro yank paste", &["a", "b"], 1, 1, "qayypq@a"),
    c(
        "mac:\"ay then @a executes text",
        &["ix<Esc>", "b"],
        1,
        1,
        "\"ay$j@a",
    ),
    c(
        "mac:macro with ci(",
        &["f(a)", "g(b)"],
        1,
        1,
        "qaci(X<Esc>jq@a",
    ),
    c(
        "mac:q register letter uppercase Q",
        &["ab", "cd"],
        1,
        1,
        "qQxq@Q",
    ),
];

// ─────────────────────────── F. marks & jumps ───────────────────────────
const CASES_MARK: &[Case] = &[
    c("mark:'a first nonblank", &["  a", "b", "c"], 1, 3, "majj'a"),
    c("mark:`a exact", &["abc", "def", "ghi"], 1, 3, "majj`a"),
    c("mark:'' after G", &["a", "b", "c", "d"], 2, 1, "G''"),
    c("mark:`` after gg", &["abc", "def", "ghi"], 2, 2, "gg``"),
    c(
        "mark:'' after search",
        &["a", "b", "c foo"],
        1,
        1,
        "/foo<CR>''",
    ),
    c("mark:'.", &["a", "b", "c"], 2, 1, "xgg'."),
    c("mark:`.", &["abc", "def"], 2, 2, "xgg`."),
    c("mark:`] after yank", &["abc def"], 1, 1, "wyiw0`]"),
    c("mark:`[ after p", &["a", "b"], 1, 1, "yyjp`["),
    c(
        "mark:'> after V",
        &["a", "b", "c", "d"],
        2,
        1,
        "Vj<Esc>gg'>",
    ),
    c("mark:`< after v", &["abc", "def"], 1, 2, "vjl<Esc>gg`<"),
    c(
        "mark:mark shifts after O",
        &["a", "b", "c"],
        2,
        1,
        "maggOx<Esc>'a",
    ),
    c(
        "mark:mark on deleted line",
        &["a", "b", "c"],
        2,
        1,
        "maddgg'a",
    ),
    c("mark:'z unset", &["a", "b"], 2, 1, "'z"),
    c("mark:`^", &["ab", "cd"], 1, 1, "jAx<Esc>gg`^"),
    c("mark:'' toggles", &["a", "b", "c", "d"], 1, 1, "3G''''"),
    c("mark:`` after %", &["(a)", "b"], 1, 1, "%``"),
    c("mark:'' after j only", &["a", "b", "c"], 1, 1, "jj''"),
    c("mark:d'a linewise", &["abc", "def"], 2, 2, "magg0d'a"),
    c("mark:c`a", &["abc def"], 1, 5, "ma0c`aX<Esc>"),
    c("mark:mA global", &["a", "b", "c"], 3, 1, "mAgg'A"),
    c("mark:`a after line join", &["ab", "cd"], 2, 2, "makJ`a"),
    c(
        "mark:'a after text insert above",
        &["a", "b"],
        2,
        1,
        "maggOx<CR>y<Esc>'a",
    ),
    c("mark:`` after ''", &["a", "b", "c"], 1, 1, "G''``"),
    c("mark:y'a cursor", &["a", "b", "c"], 3, 1, "maggy'a"),
    c(
        "mark:'a then '' back",
        &["a", "b", "c", "d"],
        4,
        1,
        "magg'a''",
    ),
    c("mark:`[ `] after :s", &["a a"], 1, 1, ":s/a/xyz/<CR>`]"),
    c("mark:`> after gv", &["abc"], 1, 1, "vl<Esc>0gv<Esc>`>"),
    c("mark:`. after o", &["a", "b"], 1, 1, "ox<Esc>gg`."),
    c("mark:'[ after >>", &["a", "b", "c"], 2, 1, ">jgg'["),
    c("jump:G C-o", &["a", "b", "c", "d"], 2, 1, "G<C-o>"),
    c(
        "jump:gg G C-o C-o C-i",
        &["a", "b", "c", "d"],
        2,
        1,
        "ggG<C-o><C-o><C-i>",
    ),
    c("jump:/foo C-o", &["a", "b", "foo"], 1, 1, "/foo<CR><C-o>"),
    c(
        "jump:3G 5G C-o C-o",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "3G5G<C-o><C-o>",
    ),
    c(
        "jump:C-o after :5",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "3G:5<CR><C-o>",
    ),
    c(
        "jump:C-o then new jump then C-i",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "3G5G<C-o>2G<C-i>",
    ),
    c("jump:g;", &["a", "b", "c"], 1, 1, "xjjxggg;"),
    c("jump:g; g;", &["a", "b", "c"], 1, 1, "xjjxggg;g;"),
    c("jump:g; g; g,", &["a", "b", "c"], 1, 1, "xjjxggg;g;g,"),
    c("jump:n C-o", &["foo", "foo", "foo"], 1, 1, "/foo<CR>n<C-o>"),
    c("jump:* C-o", &["foo bar", "foo"], 1, 1, "*<C-o>"),
    c("jump:% C-o", &["(abc)"], 1, 1, "%<C-o>"),
    c("jump:'a C-o", &["a", "b", "c"], 3, 1, "magg'a<C-o>"),
    c("jump:C-o at start", &["a", "b"], 1, 1, "<C-o>"),
    c("jump:C-i at end", &["a", "b"], 1, 1, "G<C-i>"),
    c("jump:} C-o", &["a", "", "b"], 1, 1, "}<C-o>"),
    c("jump:C-o col", &["abc", "def"], 1, 3, "G<C-o>"),
    c(
        "jump:C-o twice same line dedup",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "3G3G5G<C-o><C-o>",
    ),
    c("jump:C-o after j 20 lines", LONG, 1, 1, "20j<C-o>"),
    c("jump:C-o after 50%", LONG, 1, 1, "50%<C-o>"),
    c("jump:C-o after L", LONG, 1, 1, "L<C-o>"),
    c("jump:C-o after (", &["A b. C d."], 1, 8, "(<C-o>"),
    c(
        "jump:g; after 2 changes same line",
        &["abcdef"],
        1,
        1,
        "x$xgg0g;g;",
    ),
    c(
        "jump:3<C-o>",
        &["1", "2", "3", "4", "5", "6"],
        1,
        1,
        "2G3G4G5G3<C-o>",
    ),
    c(
        "jump:C-o after ''",
        &["a", "b", "c", "d"],
        1,
        1,
        "3G''<C-o>",
    ),
];

// ─────────────────────────── G. search ───────────────────────────
const CASES_SEARCH: &[Case] = &[
    c("search:/ basic", &["foo bar", "baz bar"], 1, 1, "/bar<CR>"),
    c("search:/ n", &["foo bar", "baz bar"], 1, 1, "/bar<CR>n"),
    c(
        "search:/ N wraps",
        &["foo bar", "baz bar"],
        1,
        1,
        "/bar<CR>N",
    ),
    c(
        "search:? then n backward",
        &["bar", "foo", "bar"],
        2,
        1,
        "?bar<CR>n",
    ),
    c(
        "search:? then N forward",
        &["bar", "foo", "bar"],
        2,
        1,
        "?bar<CR>N",
    ),
    c("search:*", &["foo bar foo"], 1, 1, "*"),
    c("search:* whole word", &["foo foobar foo"], 1, 1, "*"),
    c("search:g*", &["foo foobar foo"], 1, 1, "g*"),
    c("search:#", &["foo foobar foo"], 1, 12, "#"),
    c("search:g#", &["foo foobar foo"], 1, 12, "g#"),
    c("search:* cursor not on word", &["  foo bar foo"], 1, 1, "*"),
    c("search:* on punctuation", &["a.b a.b"], 1, 2, "*"),
    c(
        "search:* then n keeps boundaries",
        &["foo foobar foo"],
        1,
        1,
        "*n",
    ),
    c(
        "search:* then :s//",
        &["foo bar foo"],
        1,
        1,
        "*:%s//X/g<CR>",
    ),
    c("search:* with count", &["a foo foo foo"], 1, 3, "2*"),
    c("search:# at start wraps", &["foo bar foo"], 1, 1, "#"),
    c("search:/pat/e", &["foo bar"], 1, 1, "/bar/e<CR>"),
    c("search:/pat/e+1", &["foo bar baz"], 1, 1, "/bar/e+1<CR>"),
    c("search:/pat/e-1", &["foo bar baz"], 1, 1, "/bar/e-1<CR>"),
    c("search:/pat/b+2", &["foo bar baz"], 1, 1, "/bar/b+2<CR>"),
    c("search:/pat/s-1", &["foo bar baz"], 1, 1, "/bar/s-1<CR>"),
    c(
        "search:/pat/+1 linewise",
        &["a", "foo", "b", "c"],
        1,
        1,
        "/foo/+1<CR>",
    ),
    c("search:/pat/-1", &["a", "b", "foo"], 1, 1, "/foo/-1<CR>"),
    c(
        "search:/pat/e then n keeps offset",
        &["foo bar foo bar"],
        1,
        1,
        "/bar/e<CR>n",
    ),
    c("search:/\\v", &["fooo bar"], 1, 3, "/\\vo+<CR>"),
    c("search:/foo\\|bar", &["xx bar foo"], 1, 1, "/foo\\|bar<CR>"),
    c("search:/\\<foo\\>", &["foobar foo"], 1, 1, "/\\<foo\\><CR>"),
    c("search:/^foo", &["a foo", "foo b"], 1, 1, "/^foo<CR>"),
    c("search:/foo$", &["foo a", "a foo"], 1, 1, "/foo$<CR>"),
    c("search:/a\\{2}", &["a aa aaa"], 1, 1, "/a\\{2}<CR>"),
    c("search:/\\d\\+", &["ab 123 cd"], 1, 1, "/\\d\\+<CR>"),
    c("search:/[bc]a", &["xa ba ca"], 1, 1, "/[bc]a<CR>"),
    c("search:ic", &["x FOO foo"], 1, 1, ":set ic<CR>/foo<CR>"),
    c("search:noic", &["x FOO foo"], 1, 1, "/foo<CR>"),
    c(
        "search:scs upper",
        &["x FOO Foo foo"],
        1,
        1,
        ":set ic scs<CR>/Foo<CR>",
    ),
    c(
        "search:scs lower",
        &["x FOO Foo foo"],
        1,
        1,
        ":set ic scs<CR>/foo<CR>",
    ),
    c("search:\\c", &["x FOO foo"], 1, 1, "/\\cfoo<CR>"),
    c(
        "search:\\C with ic",
        &["x foo FOO"],
        1,
        1,
        ":set ic<CR>/\\CFOO<CR>",
    ),
    c(
        "search:* with ic scs",
        &["Foo foo Foo"],
        1,
        1,
        ":set ic scs<CR>*",
    ),
    c(
        "search:// repeat",
        &["foo x foo x foo"],
        1,
        1,
        "/foo<CR>//<CR>",
    ),
    c(
        "search:/<CR> repeat",
        &["foo x foo x foo"],
        1,
        1,
        "/foo<CR>/<CR>",
    ),
    c("search:wrap forward", &["foo", "x"], 1, 1, "/foo<CR>"),
    c("search:wrap backward", &["x", "foo"], 1, 1, "?foo<CR>"),
    c("search:no match", &["abc"], 1, 2, "/zzz<CR>"),
    c("search:n after *", &["foo x foo x foo"], 1, 1, "*n"),
    c("search:3/pat", &["a foo foo foo"], 1, 1, "3/foo<CR>"),
    c("search:2n", &["a foo foo foo"], 1, 1, "/foo<CR>2n"),
    c(
        "search:/pat/;/pat2/",
        &["a foo b bar"],
        1,
        1,
        "/foo/;/bar<CR>",
    ),
    c("search:?pat?e", &["foo bar baz"], 1, 11, "?bar?e<CR>"),
    c("search:c/pat", &["foo bar baz"], 1, 1, "c/baz<CR>X<Esc>"),
    c("search:y/pat cursor", &["foo bar baz"], 1, 5, "y/baz<CR>"),
    c("search:/ from mid-match", &["foofoo"], 1, 2, "/foo<CR>"),
    c("search:? from mid", &["foofoo"], 1, 5, "?foo<CR>"),
    c(
        "search:/o\\nb multiline",
        &["foo", "bar"],
        1,
        1,
        "/o\\nb<CR>",
    ),
    c(
        "search:/ then :s//",
        &["foo bar foo"],
        1,
        1,
        "/foo<CR>:s//X/g<CR>",
    ),
    c("search:\\zs", &["foobar"], 1, 1, "/foo\\zsbar<CR>"),
    c("search:\\ze", &["xbar foobar"], 1, 1, "/foo\\zebar<CR>"),
    c("search:/. literal dot", &["abc a.c"], 1, 1, "/a\\.c<CR>"),
    c(
        "search:/ with * quantifier",
        &["ac abc abbc"],
        1,
        1,
        "/ab*c<CR>",
    ),
    c("search:/ with \\s", &["a\tb c"], 1, 1, "/\\s<CR>"),
    c("search:\\V nomagic", &["a.c abc"], 1, 1, "/\\Va.c<CR>"),
    c("search:/\\%V? skip", &["abc"], 1, 1, "l"),
    c(
        "search:? n then N",
        &["bar", "foo", "bar", "bar"],
        4,
        1,
        "?bar<CR>nN",
    ),
    c(
        "search:/ then ? then n",
        &["bar", "foo", "bar", "bar"],
        1,
        1,
        "/bar<CR>?bar<CR>n",
    ),
    c(
        "search:d/pat/+0? linewise",
        &["a", "foo", "b"],
        1,
        1,
        "d/foo/0<CR>",
    ),
    c(
        "search:/\\(foo\\)\\1",
        &["foo foofoo"],
        1,
        1,
        "/\\(foo\\)\\1<CR>",
    ),
    c(
        "search:/\\w\\+ from col1",
        &["foo bar"],
        1,
        1,
        "/\\w\\+<CR>",
    ),
    c("search:/$ empty match", &["ab", "cd"], 1, 1, "/$<CR>"),
    c("search:/^ empty match", &["ab", "cd"], 1, 1, "/^<CR>"),
    c("search:/\\n at eol", &["ab", "cd"], 1, 1, "/\\n<CR>"),
    c(
        "search:/ upper V with ic",
        &["abc ABC"],
        1,
        1,
        ":set ic<CR>/ABC<CR>",
    ),
    c("search:* on number", &["12 x 12"], 1, 1, "*"),
    c(
        "search:* on word with underscore",
        &["a_b x a_b"],
        1,
        1,
        "*",
    ),
    c("search:gd", &["int x = 1;", "y = x;"], 2, 5, "gd"),
    c(
        "search:gn selects",
        &["foo bar foo"],
        1,
        1,
        "/foo<CR>ggcgnX<Esc>",
    ),
    c(
        "search:cgn .",
        &["foo bar foo baz foo"],
        1,
        1,
        "/foo<CR>ggcgnX<Esc>..",
    ),
    c("search:dgn", &["foo bar foo"], 1, 1, "/foo<CR>ggdgn"),
    c("search:gN", &["foo bar foo"], 1, 11, "/foo<CR>gNd"),
];

// ─────────────────────────── H. :s / :g / ex ───────────────────────────
const CASES_EX: &[Case] = &[
    c("sub:basic", &["a a"], 1, 1, ":s/a/b/<CR>"),
    c("sub:g", &["a a"], 1, 1, ":s/a/b/g<CR>"),
    c("sub:%", &["a", "a", "a"], 1, 1, ":%s/a/b/<CR>"),
    c("sub:%g cursor", &["a a", "b", "a a"], 2, 1, ":%s/a/x/g<CR>"),
    c("sub:2,3", &["a", "a", "a", "a"], 1, 1, ":2,3s/a/b/<CR>"),
    c("sub:.,+1", &["a", "a", "a", "a"], 2, 1, ":.,+1s/a/b/<CR>"),
    c("sub:.,$", &["a", "a", "a", "a"], 3, 1, ":.,$s/a/b/<CR>"),
    c(
        "sub:'a,'b",
        &["a", "a", "a", "a"],
        1,
        1,
        "majjmbgg:'a,'bs/a/b/<CR>",
    ),
    c(
        "sub:'<,'> auto",
        &["a", "a", "a", "a"],
        2,
        1,
        "Vj:s/a/b/<CR>",
    ),
    c(
        "sub:'<,'> explicit",
        &["a", "a", "a", "a"],
        2,
        1,
        "Vj<Esc>:'<,'>s/a/b/<CR>",
    ),
    c("sub:i flag", &["A a"], 1, 1, ":s/a/b/gi<CR>"),
    c(
        "sub:I flag with ic",
        &["A a"],
        1,
        1,
        ":set ic<CR>:s/a/b/gI<CR>",
    ),
    c("sub:ic applies", &["A a"], 1, 1, ":set ic<CR>:s/a/b/g<CR>"),
    c("sub:n flag", &["a a"], 1, 1, ":s/a/b/gn<CR>"),
    c("sub:e flag", &["a"], 1, 1, ":s/z/b/e<CR>"),
    c(
        "sub:& flag",
        &["a a", "a a"],
        1,
        1,
        ":s/a/b/g<CR>j:s/a/c/&<CR>",
    ),
    c("sub:&&", &["a a", "a a"], 1, 1, ":s/a/b/g<CR>j:&&<CR>"),
    c("sub:& cmd", &["a a", "a a"], 1, 1, ":s/a/b/g<CR>j:&<CR>"),
    c(
        "sub:backrefs",
        &["ab"],
        1,
        1,
        ":s/\\(a\\)\\(b\\)/\\2\\1/<CR>",
    ),
    c("sub:\\v groups", &["ab"], 1, 1, ":s/\\v(a)(b)/\\2\\1/<CR>"),
    c("sub:& in replacement", &["foo"], 1, 1, ":s/foo/[&]/<CR>"),
    c("sub:\\0", &["foo"], 1, 1, ":s/foo/[\\0]/<CR>"),
    c("sub:\\U&", &["foo"], 1, 1, ":s/foo/\\U&/<CR>"),
    c("sub:\\u&", &["foo"], 1, 1, ":s/foo/\\u&/<CR>"),
    c("sub:\\L", &["FOO"], 1, 1, ":s/FOO/\\L&/<CR>"),
    c("sub:\\U..\\E", &["foo bar"], 1, 1, ":s/foo/\\U&\\E-x/<CR>"),
    c("sub:\\r newline", &["a,b"], 1, 1, ":s/,/\\r/<CR>"),
    c("sub:\\t", &["a b"], 1, 1, ":s/ /\\t/<CR>"),
    c("sub:alternation", &["a b c"], 1, 1, ":s/a\\|c/x/g<CR>"),
    c("sub:\\zs", &["foobar"], 1, 1, ":s/foo\\zsbar/X/<CR>"),
    c("sub:\\ze", &["foobar"], 1, 1, ":s/foo\\zebar/X/<CR>"),
    c(
        "sub:~ prev replacement",
        &["a b"],
        1,
        1,
        ":s/a/x/<CR>:s/b/~y/<CR>",
    ),
    c("sub:# delimiter", &["a/b"], 1, 1, ":s#/#-#<CR>"),
    c("sub:empty replacement", &["abc"], 1, 1, ":s/b//<CR>"),
    c("sub:no trailing slash", &["abc"], 1, 1, ":s/b/X<CR>"),
    c("sub:no replacement", &["abc"], 1, 1, ":s/b<CR>"),
    c(
        "sub:empty pattern last search",
        &["foo bar foo"],
        1,
        1,
        "/foo<CR>:s//X/g<CR>",
    ),
    c("sub:count", &["a", "a", "a", "a"], 1, 1, ":s/a/b/ 2<CR>"),
    c(
        "sub:range + count",
        &["a", "a", "a", "a"],
        1,
        1,
        ":2s/a/b/ 2<CR>",
    ),
    c(
        "sub:trailing ws",
        &["a  ", "b "],
        1,
        1,
        ":%s/\\s\\+$//e<CR>",
    ),
    c("sub:^ anchor", &["ab", "cd"], 1, 1, ":%s/^/> /<CR>"),
    c("sub:$ anchor", &["ab", "cd"], 1, 1, ":%s/$/;/<CR>"),
    c("sub:.*", &["abc"], 1, 1, ":s/.*/[&]/<CR>"),
    c("sub:literal dot", &["a.b.c"], 1, 1, ":s/\\./,/g<CR>"),
    c("sub:bar chain", &["a b"], 1, 1, ":s/a/x/|s/b/y/<CR>"),
    c("sub:cursor after", &["x", "a b a"], 1, 1, ":2s/a/y/<CR>"),
    c("sub:no match keeps buffer", &["abc"], 1, 1, ":s/z/y/<CR>"),
    c("sub:\\n multiline", &["a", "b"], 1, 1, ":%s/a\\nb/X/<CR>"),
    c("sub:\\{2}", &["aaa"], 1, 1, ":s/a\\{2}/X/<CR>"),
    c("sub:[] class", &["abc"], 1, 1, ":s/[ac]/X/g<CR>"),
    c("sub:\\w\\+", &["foo bar"], 1, 1, ":s/\\w\\+/X/g<CR>"),
    c("sub:\\< \\>", &["foo foobar"], 1, 1, ":s/\\<foo\\>/X/g<CR>"),
    c("sub:\\{-}", &["aaa"], 1, 1, ":s/a\\{-1,}/X/<CR>"),
    c(
        "sub:\\u\\1 swap words",
        &["foo bar"],
        1,
        1,
        ":s/\\(\\w\\+\\) \\(\\w\\+\\)/\\u\\2 \\u\\1/<CR>",
    ),
    c(
        "sub:%s cursor at end",
        &["a", "b", "a"],
        1,
        1,
        ":%s/a/x/<CR>",
    ),
    c("sub:s on empty match ^", &["abc"], 1, 1, ":s/^/x/g<CR>"),
    c("sub:\\= not vimscript skip", &["a"], 1, 1, "l"),
    c("sub:$ anchor g", &["ab"], 1, 1, ":s/$/;/g<CR>"),
    c("sub:x* g on empty", &["abc"], 1, 1, ":s/x*/-/g<CR>"),
    c(
        "sub:& with \\n in pattern",
        &["a", "b", "c"],
        1,
        1,
        ":%s/\\n//<CR>",
    ),
    c(
        "sub:\\r in middle then cursor",
        &["abc"],
        1,
        1,
        ":s/b/\\r/<CR>",
    ),
    c("sub:\\= escaped slash", &["a/b"], 1, 1, ":s/\\//-/<CR>"),
    c("sub:\\/ in replacement", &["a-b"], 1, 1, ":s/-/\\//<CR>"),
    c("sub:& literal via \\&", &["foo"], 1, 1, ":s/foo/\\&/<CR>"),
    c("sub:~ literal via \\~", &["a"], 1, 1, ":s/a/\\~/<CR>"),
    c(
        "sub:whole line",
        &["hello world"],
        1,
        1,
        ":s/\\v(\\w+) (\\w+)/\\2 \\1/<CR>",
    ),
    c("g:d", &["a", "b", "a", "c"], 1, 1, ":g/a/d<CR>"),
    c("g:s", &["a x", "b x", "a x"], 1, 1, ":g/a/s/x/y/<CR>"),
    c("g:!", &["a", "b", "a", "c"], 1, 1, ":g!/a/d<CR>"),
    c("g:v", &["a", "b", "a", "c"], 1, 1, ":v/a/d<CR>"),
    c("g:normal", &["a", "b", "a"], 1, 1, ":g/a/normal Ax<CR>"),
    c("g:m0 reverse", &["1", "2", "3"], 1, 1, ":g/^/m0<CR>"),
    c("g:^$ d", &["a", "", "b", "", ""], 1, 1, ":g/^$/d<CR>"),
    c("g:t$", &["a", "b"], 1, 1, ":g/a/t$<CR>"),
    c(
        "g:cursor after",
        &["a", "b", "a", "c"],
        1,
        1,
        ":g/a/s/a/x/<CR>",
    ),
    c("g:j", &["a", "b", "a", "b"], 1, 1, ":g/a/j<CR>"),
    c("g:range", &["a", "a", "a"], 1, 1, ":2,3g/a/s/a/b/<CR>"),
    c("g:delimiter", &["a", "b"], 1, 1, ":g#a#d<CR>"),
    c(
        "g:normal dd",
        &["a", "b", "a", "c"],
        1,
        1,
        ":g/a/normal dd<CR>",
    ),
    c("g:+1d", &["a", "x", "a", "y"], 1, 1, ":g/a/+1d<CR>"),
    c(
        "g:s// reuse pattern",
        &["a", "b", "a"],
        1,
        1,
        ":g/a/s//x/<CR>",
    ),
    c(
        "g:normal with count",
        &["ab", "cd"],
        1,
        1,
        ":g/./normal 2x<CR>",
    ),
    c(
        "g:copy to end reversed order",
        &["a", "b"],
        1,
        1,
        ":g/./t.<CR>",
    ),
    c(
        "g:d with count",
        &["a", "1", "2", "b", "3", "4"],
        1,
        1,
        ":g/[ab]/d 2<CR>",
    ),
    c(
        "g:normal @a",
        &["a", "b", "a"],
        1,
        1,
        "qaAx<Esc>qu:g/a/normal @a<CR>",
    ),
    c("g:.,+1j", &["a", "b", "c", "d"], 1, 1, ":g/a\\|c/.,+1j<CR>"),
    c("ex:t.", &["a", "b"], 1, 1, ":t.<CR>"),
    c("ex:t0", &["a", "b"], 2, 1, ":t0<CR>"),
    c("ex:t$", &["a", "b"], 1, 1, ":t$<CR>"),
    c("ex:2t0", &["a", "b", "c"], 1, 1, ":2t0<CR>"),
    c("ex:m0", &["a", "b", "c"], 3, 1, ":m0<CR>"),
    c("ex:m$", &["a", "b", "c"], 1, 1, ":m$<CR>"),
    c("ex:m+1", &["a", "b", "c"], 1, 1, ":m+1<CR>"),
    c("ex:m-2", &["a", "b", "c"], 3, 1, ":m-2<CR>"),
    c("ex:2,3m0", &["a", "b", "c", "d"], 1, 1, ":2,3m0<CR>"),
    c("ex:2,3m$", &["a", "b", "c", "d"], 1, 1, ":2,3m$<CR>"),
    c("ex:1,2t$", &["a", "b", "c"], 1, 1, ":1,2t$<CR>"),
    c("ex:1co$", &["a", "b"], 1, 1, ":1co$<CR>"),
    c("ex:d", &["a", "b", "c"], 2, 1, ":d<CR>"),
    c("ex:2d", &["a", "b", "c"], 1, 1, ":2d<CR>"),
    c("ex:2,3d", &["a", "b", "c"], 1, 1, ":2,3d<CR>"),
    c("ex:d a then \"ap", &["a", "b", "c"], 1, 1, ":2d a<CR>\"ap"),
    c("ex:d 2", &["a", "b", "c"], 1, 1, ":d 2<CR>"),
    c("ex:2,3y p", &["a", "b", "c"], 1, 1, ":2,3y<CR>p"),
    c("ex:y a", &["a", "b"], 1, 1, ":y a<CR>j\"ap"),
    c("ex:pu", &["a", "b"], 1, 1, "yy:pu<CR>"),
    c("ex:pu!", &["a", "b"], 1, 1, "yy:pu!<CR>"),
    c("ex:put a", &["a", "b"], 1, 1, "\"ayy:put a<CR>"),
    c("ex:2put", &["a", "b", "c"], 1, 1, "yy:2put<CR>"),
    c("ex:0put", &["a", "b"], 2, 1, "yy:0put<CR>"),
    c("ex:put charwise reg", &["ab", "c"], 1, 1, "yl:put<CR>"),
    c("ex:j", &["a", "b", "c"], 1, 1, ":j<CR>"),
    c("ex:1,3j", &["a", "b", "c"], 1, 1, ":1,3j<CR>"),
    c("ex:j!", &["a", "  b"], 1, 1, ":j!<CR>"),
    c("ex:j 3", &["a", "b", "c", "d"], 1, 1, ":j 3<CR>"),
    c("ex:>", &["a"], 1, 1, ":><CR>"),
    c("ex:>>", &["a"], 1, 1, ":>><CR>"),
    c("ex:2,3>", &["a", "b", "c"], 1, 1, ":2,3><CR>"),
    c("ex:<", &["        a"], 1, 1, ":<<CR>"),
    c("ex:> 2", &["a", "b", "c"], 1, 1, ":> 2<CR>"),
    c("ex:sort", &["c", "a", "b"], 1, 1, ":sort<CR>"),
    c("ex:sort!", &["c", "a", "b"], 1, 1, ":sort!<CR>"),
    c("ex:sort n", &["10", "9", "100"], 1, 1, ":sort n<CR>"),
    c("ex:sort u", &["b", "a", "b"], 1, 1, ":sort u<CR>"),
    c("ex:sort i", &["b", "A", "a"], 1, 1, ":sort i<CR>"),
    c(
        "ex:sort mixed case",
        &["b", "A", "a", "B"],
        1,
        1,
        ":sort<CR>",
    ),
    c("ex:2,3sort", &["c", "b", "a"], 1, 1, ":2,3sort<CR>"),
    c(
        "ex:sort /pat/",
        &["x2 b", "x1 a"],
        1,
        1,
        ":sort /x\\d /<CR>",
    ),
    c(
        "ex:sort /pat/ r",
        &["b 2", "a 1"],
        1,
        1,
        ":sort /\\d/ r<CR>",
    ),
    c(
        "ex:sort n non-numbers first",
        &["b", "2", "a", "1"],
        1,
        1,
        ":sort n<CR>",
    ),
    c("ex:sort cursor", &["c", "a", "b"], 3, 1, ":sort<CR>"),
    c("ex:retab", &["\ta"], 1, 1, ":set ts=4<CR>:retab<CR>"),
    c(
        "ex:retab!",
        &["    a"],
        1,
        1,
        ":set noet ts=4<CR>:retab!<CR>",
    ),
    c("ex:retab 2", &["\ta"], 1, 1, ":set ts=4<CR>:retab 2<CR>"),
    c("ex:undo", &["ab"], 1, 1, "x:undo<CR>"),
    c("ex:undo redo", &["ab"], 1, 1, "x:undo<CR>:redo<CR>"),
    c("ex:5", &["1", "2", "3", "4", "5", "6"], 1, 1, ":5<CR>"),
    c("ex:$", &["1", "2", "3"], 1, 1, ":$<CR>"),
    c("ex:+2", &["1", "2", "3", "4"], 1, 1, ":+2<CR>"),
    c("ex:-1", &["1", "2", "3"], 3, 1, ":-1<CR>"),
    c("ex:/foo/", &["a", "b", "foo"], 1, 1, ":/foo/<CR>"),
    c("ex:?a?", &["a", "b", "c"], 3, 1, ":?a?<CR>"),
    c("ex:/foo/+1", &["a", "foo", "b"], 1, 1, ":/foo/+1<CR>"),
    c("ex:/foo/d", &["a", "foo", "b"], 1, 1, ":/foo/d<CR>"),
    c(
        "ex:/a/,/b/d",
        &["x", "a", "y", "b", "z"],
        1,
        1,
        ":/a/,/b/d<CR>",
    ),
    c(
        "ex:.,/foo/d",
        &["a", "b", "foo", "c"],
        1,
        1,
        ":.,/foo/d<CR>",
    ),
    c("ex:%d", &["a", "b"], 1, 1, ":%d<CR>"),
    c("ex:%j", &["a", "b", "c"], 1, 1, ":%j<CR>"),
    c("ex:2ka 'a", &["a", "b", "c"], 1, 1, ":2ka<CR>'a"),
    c("ex:2mark a", &["a", "b", "c"], 1, 1, ":2mark a<CR>'a"),
    c("ex:le", &["    a"], 1, 1, ":le<CR>"),
    c("ex:le 4", &["a"], 1, 1, ":le 4<CR>"),
    c("ex:ri 10", &["a"], 1, 1, ":ri 10<CR>"),
    c("ex:ce 10", &["a"], 1, 1, ":ce 10<CR>"),
    c("ex:normal Ax", &["a", "b"], 1, 1, ":normal Ax<CR>"),
    c("ex:%normal Ax", &["a", "b"], 1, 1, ":%normal Ax<CR>"),
    c(
        "ex:2,3normal I-",
        &["a", "b", "c"],
        1,
        1,
        ":2,3normal I-<CR>",
    ),
    c("ex:normal 2x", &["abcd"], 1, 1, ":normal 2x<CR>"),
    c("ex:normal! Ax", &["a"], 1, 1, ":normal! Ax<CR>"),
    c("ex:normal incomplete", &["ab"], 1, 1, ":normal d<CR>"),
    c("ex:normal cursor", &["abc", "def"], 1, 1, ":2normal $<CR>"),
    c("ex:r !echo", &["a"], 1, 1, ":r !echo hi<CR>"),
    c("ex:%!sort", &["b", "a"], 1, 1, ":%!sort<CR>"),
    c("ex:2;+1d", &["a", "b", "c", "d"], 1, 1, ":2;+1d<CR>"),
    c("ex:2,+1d", &["a", "b", "c", "d"], 1, 1, ":2,+1d<CR>"),
    c("ex:$-1d", &["a", "b", "c"], 1, 1, ":$-1d<CR>"),
    c("ex:.+2", &["1", "2", "3", "4"], 1, 1, ":.+2<CR>"),
    c("ex:cursor after :t$", &["a", "b"], 1, 1, ":t$<CR>"),
    c("ex:cursor after :m0", &["a", "b", "c"], 3, 1, ":m0<CR>"),
    c("ex:cursor after :2d", &["a", "b", "c"], 1, 1, ":2d<CR>"),
    c("ex:cursor after :>", &["  a"], 1, 2, ":><CR>"),
    c("ex:cursor after :j", &["a", "b", "c"], 1, 1, ":j<CR>"),
    c(
        "ex:cursor after :%normal",
        &["a", "b", "c"],
        1,
        1,
        ":%normal Ax<CR>",
    ),
    c(
        "ex:cursor after :g/d",
        &["a", "b", "a", "c"],
        1,
        1,
        ":g/a/d<CR>",
    ),
    c("ex:1,2co0", &["a", "b", "c"], 1, 1, ":1,2co0<CR>"),
    c("ex:%y then P", &["a", "b"], 2, 1, ":%y<CR>P"),
    c("ex:d _", &["a", "b"], 1, 1, "yyj:d _<CR>p"),
    c("ex:y A append", &["a", "b"], 1, 1, ":y a<CR>j:y A<CR>\"ap"),
    c("ex:.,.+1d", &["a", "b", "c"], 1, 1, ":.,.+1d<CR>"),
    c(
        "ex:'<,'>d after v",
        &["a", "b", "c"],
        1,
        1,
        "vj<Esc>:'<,'>d<CR>",
    ),
    c(
        "ex:*d after visual",
        &["a", "b", "c"],
        1,
        1,
        "Vj<Esc>:*d<CR>",
    ),
    c("ex:g/pat/normal cgn? skip", &["a"], 1, 1, "l"),
    c("ex:s with c flag skipped", &["a"], 1, 1, "l"),
    c(
        "ex:3 goes col firstnonblank",
        &["a", "b", "  c"],
        1,
        1,
        ":3<CR>",
    ),
    c("ex:0", &["a", "b", "c"], 3, 1, ":0<CR>"),
    c("ex:%s then n", &["a", "b", "a"], 1, 1, ":%s/a/x/<CR>ggn"),
    c(
        "ex:s sets last search for n",
        &["a", "b", "a"],
        1,
        1,
        ":s/a/x/<CR>n",
    ),
    c("ex:s cursor col", &["xx a"], 1, 4, ":s/a/b/<CR>"),
    c(
        "ex:%s/x/y/g with \\r cursor",
        &["a,b,c"],
        1,
        1,
        ":s/,/\\r/g<CR>",
    ),
    c("ex:noh no effect", &["a"], 1, 1, "/a<CR>:noh<CR>"),
    c(
        "ex:2>3? shift count",
        &["a", "b", "c", "d"],
        1,
        1,
        ":2> 2<CR>",
    ),
    c("ex:< 2", &["    a", "    b", "    c"], 1, 1, ":< 2<CR>"),
    c("ex:>>> 3 levels", &["a"], 1, 1, ":>>><CR>"),
    c(
        "ex:j with range and count",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        ":2j 3<CR>",
    ),
    c("ex:d x then \"xP", &["a", "b"], 1, 1, ":d x<CR>\"xP"),
    c("ex:m with 0 on first", &["a", "b"], 1, 1, ":m0<CR>"),
    c("ex:m to self", &["a", "b"], 2, 1, ":m2<CR>"),
    c(
        "ex:t with range dest .",
        &["a", "b", "c"],
        3,
        1,
        ":1,2t.<CR>",
    ),
    c(
        "ex:s on visual block ranges",
        &["a a", "a a", "a a"],
        1,
        1,
        "<C-v>j:s/a/b/<CR>",
    ),
];

// ─────────────────────────── I. insert mode keys ───────────────────────────
const CASES_INS: &[Case] = &[
    c("ins:C-w", &["ab"], 1, 1, "Afoo bar<C-w><Esc>"),
    c(
        "ins:C-w at line start joins",
        &["ab", "cd"],
        2,
        1,
        "i<C-w><Esc>",
    ),
    c(
        "ins:C-w over existing text",
        &["ab cd"],
        1,
        6,
        "A<C-w><Esc>",
    ),
    c("ins:C-w punctuation", &["foo.bar"], 1, 8, "A<C-w><Esc>"),
    c(
        "ins:C-w trailing ws then word",
        &["foo bar  "],
        1,
        1,
        "A<C-w><Esc>",
    ),
    c("ins:C-w only whitespace", &["foo   "], 1, 1, "A<C-w><Esc>"),
    c("ins:C-u inserted", &["ab"], 1, 1, "Afoo<C-u><Esc>"),
    c("ins:C-u before start", &["ab"], 1, 1, "A<C-u><Esc>"),
    c("ins:C-u twice", &["ab"], 1, 1, "Afoo<C-u><C-u><Esc>"),
    c("ins:C-u with indent", &["    ab"], 1, 1, "A<C-u><Esc>"),
    c("ins:BS at col1 joins", &["ab", "cd"], 2, 1, "i<BS><Esc>"),
    c(
        "ins:BS over indent (nvim smarttab)",
        &["    a"],
        1,
        5,
        "i<BS><Esc>",
    ),
    cs(
        "ins:BS over indent (nosmarttab)",
        &["    a"],
        1,
        5,
        "i<BS><Esc>",
        "vim.o.smarttab=false",
    ),
    c("ins:BS mid indent", &["      a"], 1, 4, "i<BS><Esc>"),
    c(
        "ins:Tab at start (smarttab)",
        &["x"],
        1,
        1,
        ":set ts=8<CR>i<Tab><Esc>",
    ),
    cs(
        "ins:Tab at start (nosmarttab)",
        &["x"],
        1,
        1,
        ":set ts=8<CR>i<Tab><Esc>",
        "vim.o.smarttab=false",
    ),
    c("ins:Tab mid line ts4", &["a"], 1, 1, "A<Tab>x<Esc>"),
    c(
        "ins:Tab mid line ts8",
        &["a"],
        1,
        1,
        ":set ts=8<CR>A<Tab>x<Esc>",
    ),
    c("ins:Tab after 2 chars ts4", &["ab"], 1, 1, "A<Tab>x<Esc>"),
    c("ins:C-t", &["a"], 1, 1, "i<C-t><Esc>"),
    c("ins:C-t mid line", &["ab"], 1, 2, "i<C-t><Esc>"),
    c("ins:C-d", &["    a"], 1, 5, "i<C-d><Esc>"),
    c("ins:C-d partial", &["  a"], 1, 1, "A<C-d><Esc>"),
    c("ins:0 C-d", &["    a"], 1, 1, "A0<C-d><Esc>"),
    // #804 CI fix: `i_0_CTRL-D` keys off Vim's `lastc` — the previous
    // *keystroke* — not off the buffer text before the cursor. These three
    // pin the distinction the oracle actually makes.
    c("ins:0 C-d after text", &["    afoo"], 1, 1, "A0<C-d><Esc>"),
    c(
        "ins:C-d with untyped 0 before cursor",
        &["    a0"],
        1,
        1,
        "A<C-d><Esc>",
    ),
    c("ins:caret C-d", &["    a"], 1, 1, "A^<C-d><Esc>"),
    c("ins:0 C-d twice", &["        a"], 1, 1, "A0<C-d><C-d><Esc>"),
    c("ins:C-o dw", &["foo bar"], 1, 1, "i<C-o>dw<Esc>"),
    c("ins:C-o $ then type", &["foo"], 1, 1, "i<C-o>$x<Esc>"),
    c("ins:A C-o h", &["foo"], 1, 1, "A<C-o>hx<Esc>"),
    c("ins:C-o with count", &["a b c d"], 1, 1, "i<C-o>2wx<Esc>"),
    c("ins:C-o p", &["ab"], 1, 1, "yli<C-o>p<Esc>"),
    c("ins:C-o :s", &["a a"], 1, 1, "A<C-o>:s/a/b/<CR>x<Esc>"),
    c("ins:C-e", &["a", "cd"], 1, 1, "A<C-e><Esc>"),
    c("ins:C-y", &["ab", "c"], 2, 1, "A<C-y><Esc>"),
    c("ins:C-y nothing above", &["ab", "c"], 1, 1, "A<C-y><Esc>"),
    c("ins:C-a reinsert", &["ab"], 1, 1, "ifoo<Esc>A<C-a><Esc>"),
    c("ins:C-v Tab", &["a"], 1, 1, "i<C-v><Tab><Esc>"),
    c("ins:C-v 065", &["a"], 1, 1, "i<C-v>065<Esc>"),
    c("ins:C-v x41", &["a"], 1, 1, "i<C-v>x41<Esc>"),
    c("ins:CR autoindent", &["    foo"], 1, 8, "A<CR>bar<Esc>"),
    c("ins:CR mid-line", &["foo bar"], 1, 4, "i<CR><Esc>"),
    c("ins:CR after space", &["foo bar"], 1, 5, "i<CR><Esc>"),
    c("ins:CR on indented mid", &["  foo bar"], 1, 6, "i<CR><Esc>"),
    c(
        "ins:CR then Esc removes autoindent",
        &["    foo"],
        1,
        8,
        "A<CR><Esc>",
    ),
    c(
        "ins:CR CR keeps prev line empty",
        &["    foo"],
        1,
        8,
        "A<CR><CR>x<Esc>",
    ),
    c("ins:Right Right X", &["abc"], 1, 1, "i<Right><Right>X<Esc>"),
    c("ins:Left at col1", &["abc"], 1, 1, "i<Left>X<Esc>"),
    c("ins:Right at eol", &["abc"], 1, 3, "i<Right><Right>X<Esc>"),
    c(
        "ins:Down Down col memory",
        &["abcdef", "ab", "abcdef"],
        1,
        5,
        "i<Down><Down>X<Esc>",
    ),
    c("ins:End Home", &["abc"], 1, 2, "i<End>X<Home>Y<Esc>"),
    c("ins:Del", &["abc"], 1, 1, "i<Del><Esc>"),
    c("ins:Del at eol joins", &["ab", "cd"], 1, 2, "a<Del><Esc>"),
    c("ins:Esc cursor left", &["abc"], 1, 3, "a<Esc>"),
    c("ins:C-h as BS", &["abc"], 1, 3, "a<C-h><Esc>"),
    c("ins:C-j newline", &["ab"], 1, 2, "a<C-j><Esc>"),
    c("ins:3ix Left y", &["a"], 1, 1, "3ix<Left>y<Esc>"),
    c("ins:( no autopair", &["a"], 1, 1, "i(<Esc>"),
    c("ins:\" no autopair", &["a"], 1, 1, "i\"<Esc>"),
    c("ins:{ CR no autopair", &["a"], 1, 1, "i{<CR><Esc>"),
    c("ins:[ no autopair", &["a"], 1, 1, "A[<Esc>"),
    // `completeopt=""` is not cosmetic: with the default `menu,preview` and two
    // or more candidates, nvim 0.9.x tries to draw the completion popup and
    // *segfaults* under `--headless -l`, so the oracle can never answer. Pinning
    // it off keeps these cases probeable.
    cs(
        "ins:C-n completion",
        &["foo", "f"],
        2,
        1,
        "A<C-n><Esc>",
        "vim.o.completeopt=\"\"",
    ),
    cs(
        "ins:C-p completion",
        &["foo", "fob", "f"],
        3,
        1,
        "A<C-p><Esc>",
        "vim.o.completeopt=\"\"",
    ),
    c(
        "ins:typing prefix then CR no completion",
        &["foo bar"],
        1,
        1,
        "ofo<CR>x<Esc>",
    ),
    c(
        "ins:typing prefix then Tab",
        &["foo bar"],
        1,
        1,
        "ofo<Tab>x<Esc>",
    ),
    c("ins:typing prefix then Esc", &["foo bar"], 1, 1, "ofo<Esc>"),
    c("ins:C-r C-w? skip", &["a"], 1, 1, "l"),
    c("ins:i then Up", &["abc", "def"], 2, 2, "i<Up>X<Esc>"),
    c("ins:o then Up", &["abc"], 1, 1, "o<Up>X<Esc>"),
    c(
        "ins:BS at start of insert over prev text",
        &["abc"],
        1,
        3,
        "i<BS><BS><Esc>",
    ),
    c(
        "ins:C-w at start of insert",
        &["foo bar"],
        1,
        5,
        "i<C-w><Esc>",
    ),
    c(
        "ins:A then BS past insert start",
        &["ab"],
        1,
        1,
        "A<BS><BS><BS><Esc>",
    ),
    c("ins:C-t then C-d", &["a"], 1, 1, "i<C-t><C-t><C-d><Esc>"),
    c("ins:C-v u00e9", &["a"], 1, 1, "i<C-v>u00e9<Esc>"),
    c(
        "ins:insert Tab then BS (sts)",
        &["a"],
        1,
        1,
        "A<Tab><BS>x<Esc>",
    ),
    c("ins:C-e beyond line", &["abc", "d"], 1, 1, "A<C-e><Esc>"),
    c(
        "ins:i with count and Esc cursor",
        &["abc"],
        1,
        2,
        "2ix<Esc>",
    ),
    c("ins:A with count and CR", &["a"], 1, 1, "2Ax<CR><Esc>"),
    c(
        "ins:C-r register linewise mid line",
        &["a", "bc"],
        1,
        1,
        "yyjli<C-r>\"<Esc>",
    ),
    c(
        "ins:C-r with tab in register",
        &["a\tb", "c"],
        1,
        1,
        "yyjA<C-r>\"<Esc>",
    ),
    c("ins:C-w C-w", &["a b c"], 1, 1, "A<C-w><C-w><Esc>"),
    c(
        "ins:autoindent with tabs noet",
        &["\tfoo"],
        1,
        1,
        ":set noet<CR>obar<Esc>",
    ),
    c("ins:C-t noet", &["a"], 1, 1, ":set noet<CR>i<C-t><Esc>"),
    c("ins:C-k digraph skip", &["a"], 1, 1, "l"),
    c(
        "ins:BS join with autoindent",
        &["a", "    b"],
        2,
        5,
        "i<BS><BS><BS><BS><BS><Esc>",
    ),
    c("ins:i then Esc then . twice", &["a"], 1, 1, "ix<Esc>.."),
];

// ─────────────────────────── J. visual ───────────────────────────
const CASES_VIS: &[Case] = &[
    c("vis:vjd", &["abc", "def", "ghi"], 1, 2, "vjd"),
    c("vis:Vjd", &["abc", "def", "ghi"], 1, 2, "Vjd"),
    c("vis:vjy cursor", &["abc", "def", "ghi"], 2, 2, "vjy"),
    c("vis:vky cursor", &["abc", "def", "ghi"], 2, 2, "vky"),
    c("vis:v$d joins", &["abc", "def"], 1, 2, "v$d"),
    c("vis:v$y p", &["abc", "def"], 1, 1, "v$yjp"),
    c("vis:vec", &["foo bar"], 1, 1, "vecX<Esc>"),
    c("vis:vjJ", &["a", "b", "c"], 1, 1, "vjJ"),
    c("vis:VjjJ", &["a", "b", "c", "d"], 1, 1, "VjjJ"),
    c("vis:vj>", &["a", "b", "c"], 1, 1, "vj>"),
    c("vis:Vj> cursor", &["a", "b", "c"], 1, 1, "Vj>"),
    c("vis:v2jd", &["a", "b", "c", "d"], 1, 1, "v2jd"),
    c("vis:vipd", &["a", "b", "", "c"], 1, 1, "vipd"),
    c("vis:vllohd", &["abcdef"], 1, 3, "vllohd"),
    c("vis:gv", &["abcdef"], 1, 1, "vly<Esc>$gvd"),
    c(
        "vis:gv after Vjd",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "Vjdgvd",
    ),
    c("vis:vGd", &["abc", "def"], 1, 2, "vGd"),
    c("vis:vggd", &["abc", "def"], 2, 2, "vggd"),
    c("vis:v0d", &["abcdef"], 1, 4, "v0d"),
    c("vis:v^d", &["   abc"], 1, 6, "v^d"),
    c("vis:vjr-", &["abc", "def"], 1, 2, "vjr-"),
    c("vis:Vr-", &["abc"], 1, 2, "Vr-"),
    c("vis:vj~", &["abc", "def"], 1, 2, "vj~"),
    c("vis:vjU", &["abc", "def"], 1, 2, "vjU"),
    c("vis:Vju", &["ABC", "DEF"], 1, 1, "Vju"),
    c("vis:vjD", &["abc", "def", "ghi"], 1, 2, "vjD"),
    c("vis:vjX", &["abc", "def", "ghi"], 1, 2, "vjX"),
    c("vis:vjY p", &["abc", "def", "ghi"], 1, 2, "vjYGp"),
    c("vis:vjC", &["abc", "def", "ghi"], 1, 2, "vjCX<Esc>"),
    c("vis:vjS", &["abc", "def", "ghi"], 1, 2, "vjSX<Esc>"),
    c("vis:vjR", &["abc", "def", "ghi"], 1, 2, "vjRX<Esc>"),
    c("vis:v3ld", &["abcdef"], 1, 1, "v3ld"),
    c("vis:vlp linewise reg", &["a", "b", "xyz"], 1, 1, "yyjjvlp"),
    c("vis:Vp charwise reg", &["ab", "cd"], 1, 1, "ylVp"),
    c("vis:vly$P", &["abc"], 1, 1, "vly$P"),
    c("vis:vi(d", &["f(a, b)"], 1, 3, "vi(d"),
    c("vis:va(d", &["f(a, b)"], 1, 3, "va(d"),
    c("vis:vi(i( expands", &["((a))"], 1, 3, "vi(i(d"),
    c("vis:va\"d", &["x \"ab\" y"], 1, 4, "va\"d"),
    c("vis:vi\"d", &["x \"ab\" y"], 1, 4, "vi\"d"),
    c("vis:viwiwiwd", &["a b c d"], 1, 1, "viwiwiwd"),
    c("vis:v3iwd", &["a b c d"], 1, 1, "v3iwd"),
    c("vis:v2awd", &["a b c d"], 1, 1, "v2awd"),
    c("vis:vapd", &["a", "b", "", "c", "d"], 1, 1, "vapd"),
    c("vis:Vj:normal", &["a", "b", "c"], 1, 1, "Vj:normal Ax<CR>"),
    c("vis:Vj=", &["  a", "    b"], 1, 1, "Vj="),
    c("vis:vll Esc cursor", &["abc"], 1, 1, "vll<Esc>"),
    c("vis:vjkd shrink", &["abc", "def"], 1, 2, "vjkd"),
    c("vis:Vd cursor", &["  a", "  b"], 1, 3, "Vd"),
    c("vis:viwd on whitespace", &["a   b"], 1, 2, "viwd"),
    c("vis:vawd at eol", &["foo bar"], 1, 5, "vawd"),
    c("vis:vjy then P", &["abc", "def"], 1, 2, "vjyP"),
    c("vis:vj< ", &["    a", "    b"], 1, 1, "vj<"),
    c("vis:V3>", &["a"], 1, 1, "V3>"),
    c("vis:vjo then d", &["abc", "def"], 1, 2, "vjod"),
    c("vis:Vjo k d", &["a", "b", "c", "d"], 2, 1, "Vjokd"),
    c(
        "vis:vip then ip extends",
        &["a", "", "b", "", "c"],
        1,
        1,
        "vipipd",
    ),
    c("vis:v% d", &["(abc) d"], 1, 1, "v%d"),
    c("vis:vf,d", &["a,b,c"], 1, 1, "vf,d"),
    c("vis:vt,d", &["a,b,c"], 1, 1, "vt,d"),
    c("vis:v/pat d", &["foo bar baz"], 1, 1, "v/baz<CR>d"),
    c("vis:vnd", &["foo x foo y foo"], 1, 1, "/foo<CR>vnd"),
    c("vis:v'a? mark d", &["abc", "def"], 2, 2, "magg0v`ad"),
    c("vis:vjc then u", &["abc", "def"], 1, 2, "vjcX<Esc>u"),
    c("vis:V then count j >", &["a", "b", "c", "d"], 1, 1, "V2j>"),
    c("vis:v$ on last line", &["abc"], 1, 1, "v$d"),
    c("vis:v$h", &["abcd"], 1, 1, "v$hd"),
    c("vis:vgUiw", &["foo bar"], 1, 1, "wviwU"),
    c(
        "vis:vjgq",
        &["one two three four five six", "seven"],
        1,
        1,
        ":set tw=10<CR>Vjgq",
    ),
    c(
        "vis:vj: shows range then s",
        &["a", "a", "a"],
        1,
        1,
        "vj:s/a/b/g<CR>",
    ),
    c("vis:vjy \"0 then p", &["ab", "cd"], 1, 1, "vjy\"0P"),
    c(
        "vis:vjp charwise reg into multi",
        &["abc", "def", "x"],
        1,
        1,
        "ylvjp",
    ),
    c("vis:vjd cursor", &["abc", "def", "ghi"], 1, 2, "vjd"),
    c("vis:V G d cursor", &["a", "b", "c"], 2, 1, "VGd"),
    c("vis:v iw at word end", &["foo bar"], 1, 3, "viwd"),
    c(
        "vis:v aw on last word with leading space",
        &["foo bar"],
        1,
        7,
        "vawd",
    ),
    c("vis:v x", &["abcd"], 1, 2, "vlx"),
    c("vis:v s", &["abcd"], 1, 2, "vlsX<Esc>"),
    c("vis:v with count 3v? skip", &["abcd"], 1, 1, "l"),
    c("vis:vjy count 2p", &["ab", "cd"], 1, 1, "vjy$2p"),
    c("vis:V y cursor col", &["  abc"], 1, 4, "Vy"),
    c("vis:v gv after y", &["abcdef"], 1, 3, "vly0gvd"),
    c("vis:v then gv toggles", &["abcdef"], 1, 1, "vl<Esc>$vgvd"),
    c("vis:v_gJ", &["a", "  b", "c"], 1, 1, "VjgJ"),
    c("vis:v_r CR", &["abc"], 1, 2, "vr<CR>"),
    c("vis:v_J count", &["a", "b", "c", "d"], 1, 1, "V2jJ"),
    c("vis:v ip on blank", &["a", "", "", "b"], 2, 1, "vipd"),
    c("vis:v ap trailing", &["a", "", "b", "c"], 3, 1, "vapd"),
    c("vis:vjd then p", &["abc", "def", "ghi"], 1, 2, "vjdp"),
    c("vis:VjdP", &["a", "b", "c"], 1, 1, "VjdP"),
    c("vis:Vjy then p count", &["a", "b"], 1, 1, "Vjy2p"),
    c("vis:v ip then y cursor", &["a", "b", "", "c"], 2, 1, "vipy"),
    c(
        "vis:vip on last para no trailing",
        &["a", "", "b", "c"],
        4,
        1,
        "vipd",
    ),
    c("vis:v_O charwise same as o", &["abcdef"], 1, 3, "vllOhd"),
    c("vis:v then < count", &["        a"], 1, 1, "V2<"),
    c(
        "vis:v mode Esc then cursor",
        &["abc", "def"],
        1,
        1,
        "vj<Esc>",
    ),
    c(
        "vis:v with $ then j keeps eol",
        &["ab", "abcd", "abc"],
        1,
        1,
        "v$jd",
    ),
    c(
        "vis:v with $ then j then y p",
        &["ab", "abcd", "abc"],
        1,
        1,
        "v$jyGp",
    ),
];

// ─────────────────────────── K. visual block ───────────────────────────
const CASES_VB: &[Case] = &[
    c("vb:jjd", &["abc", "def", "ghi"], 1, 2, "<C-v>jjd"),
    c("vb:jjld", &["abc", "def", "ghi"], 1, 2, "<C-v>jjld"),
    c("vb:jjIx", &["abc", "def", "ghi"], 1, 2, "<C-v>jjIx<Esc>"),
    c("vb:jjAx", &["abc", "def", "ghi"], 1, 2, "<C-v>jjAx<Esc>"),
    c("vb:jj$Ax", &["ab", "abcd", "a"], 1, 1, "<C-v>jj$Ax<Esc>"),
    c("vb:jlrx", &["abc", "def"], 1, 2, "<C-v>jlrx"),
    c("vb:jlcX", &["abc", "def"], 1, 2, "<C-v>jlcX<Esc>"),
    c("vb:j>", &["abc", "def"], 1, 2, "<C-v>j>"),
    c("vb:jy then Gp", &["ab", "cd", "", "xy"], 1, 1, "<C-v>jyGp"),
    c("vb:jy then P", &["ab", "cd"], 1, 1, "<C-v>jy$P"),
    c(
        "vb:ragged d",
        &["abcdef", "ab", "abcdef"],
        1,
        3,
        "<C-v>jjlld",
    ),
    c(
        "vb:I on short line skipped",
        &["abcdef", "ab", "abcdef"],
        1,
        4,
        "<C-v>jjIx<Esc>",
    ),
    c(
        "vb:A on short line padded",
        &["abcdef", "ab", "abcdef"],
        1,
        4,
        "<C-v>jjAx<Esc>",
    ),
    c("vb:jj$d", &["abcdef", "ab", "abcd"], 1, 3, "<C-v>jj$d"),
    c("vb:o", &["abcdef", "abcdef"], 1, 2, "<C-v>jllohd"),
    c("vb:O", &["abcdef", "abcdef"], 1, 2, "<C-v>jllOhd"),
    c("vb:jx", &["abc", "def"], 1, 2, "<C-v>jx"),
    c("vb:jsX", &["abc", "def"], 1, 2, "<C-v>jsX<Esc>"),
    c("vb:jJ", &["abc", "def", "ghi"], 1, 2, "<C-v>jJ"),
    c("vb:jl~", &["abc", "def"], 1, 2, "<C-v>jl~"),
    c("vb:jlU", &["abc", "def"], 1, 2, "<C-v>jlU"),
    c("vb:jCX", &["abcdef", "abcdef"], 1, 3, "<C-v>jCX<Esc>"),
    c("vb:jD", &["abcdef", "abcdef"], 1, 3, "<C-v>jD"),
    c(
        "vb:jIx then .",
        &["ab", "ab", "ab", "ab"],
        1,
        1,
        "<C-v>jIx<Esc>jj.",
    ),
    c(
        "vb:I on empty middle line",
        &["ab", "", "ab"],
        1,
        1,
        "<C-v>jjIx<Esc>",
    ),
    c(
        "vb:A on empty middle line",
        &["ab", "", "ab"],
        1,
        1,
        "<C-v>jjAx<Esc>",
    ),
    c(
        "vb:$A on empty middle line",
        &["ab", "", "ab"],
        1,
        1,
        "<C-v>jj$Ax<Esc>",
    ),
    c("vb:jjy p at eol", &["ab", "cd", "ef"], 1, 1, "<C-v>jjy$p"),
    c(
        "vb:jjy p on shorter",
        &["abc", "abc", "x"],
        1,
        2,
        "<C-v>jjyGp",
    ),
    c("vb:jly then p", &["abc", "def"], 1, 1, "<C-v>jly$p"),
    c("vb:jIx with CR", &["ab", "ab"], 1, 1, "<C-v>jIx<CR><Esc>"),
    c(
        "vb:jc with multi chars",
        &["abcd", "abcd"],
        1,
        2,
        "<C-v>jlcXYZ<Esc>",
    ),
    c("vb:j< ", &["    ab", "    ab"], 1, 5, "<C-v>j<"),
    c("vb:jr<CR>", &["abc", "def"], 1, 2, "<C-v>jr<CR>"),
    c("vb:cursor after d", &["abcdef", "abcdef"], 1, 3, "<C-v>jld"),
    c(
        "vb:cursor after y",
        &["abcdef", "abcdef"],
        2,
        4,
        "<C-v>khhy",
    ),
    c(
        "vb:jjAx then u",
        &["ab", "ab", "ab"],
        1,
        1,
        "<C-v>jjAx<Esc>u",
    ),
    c(
        "vb:2j then o then j",
        &["abc", "abc", "abc", "abc"],
        1,
        1,
        "<C-v>2jlojd",
    ),
    c("vb:I with count? 2I", &["ab", "ab"], 1, 1, "<C-v>j2Ix<Esc>"),
    c(
        "vb:jjp block over block",
        &["ab", "cd", "ef", "gh"],
        1,
        1,
        "<C-v>jy2j<C-v>jp",
    ),
    c(
        "vb:vb yank then p linewise reg? P",
        &["ab", "cd"],
        1,
        1,
        "<C-v>jyP",
    ),
    c("vb:$ then I", &["ab", "abcd"], 1, 1, "<C-v>j$Ix<Esc>"),
    c(
        "vb:d then .",
        &["abcd", "abcd", "abcd", "abcd"],
        1,
        1,
        "<C-v>jdjj.",
    ),
    c(
        "vb:r then .",
        &["abcd", "abcd", "abcd", "abcd"],
        1,
        1,
        "<C-v>jlrxjj.",
    ),
    c(
        "vb:c then .",
        &["abcd", "abcd", "abcd", "abcd"],
        1,
        1,
        "<C-v>jcX<Esc>jj.",
    ),
    c(
        "vb:jjIx on tab lines",
        &["\tab", "\tab"],
        1,
        2,
        "<C-v>jIx<Esc>",
    ),
    c("vb:g C-a", &["1", "1", "1"], 1, 1, "<C-v>jjg<C-a>"),
    c("vb:jjy then gv", &["ab", "cd", "ef"], 1, 1, "<C-v>jjygvd"),
    c("vb:v then C-v switch", &["abc", "def"], 1, 1, "vj<C-v>d"),
    c("vb:V then C-v switch", &["abc", "def"], 1, 2, "Vj<C-v>d"),
    c("vb:C-v then v switch", &["abc", "def"], 1, 2, "<C-v>jvd"),
    c("vb:C-v then V", &["abc", "def"], 1, 2, "<C-v>jVd"),
];

// ─────────────────────────── L. C-a / C-x ───────────────────────────
const CASES_NUM: &[Case] = &[
    c("num:C-a on number", &["x 5 y"], 1, 3, "<C-a>"),
    c("num:C-a before number", &["x 5 y"], 1, 1, "<C-a>"),
    c("num:5C-a", &["x 5 y"], 1, 1, "5<C-a>"),
    c("num:C-x to negative", &["x 0 y"], 1, 3, "<C-x>"),
    c("num:C-a on -5", &["x -5 y"], 1, 3, "<C-a>"),
    c("num:C-a on -1", &["x -1 y"], 1, 3, "<C-a>"),
    c("num:C-x on -1", &["x -1 y"], 1, 3, "<C-x>"),
    c("num:hex 0x0f", &["0x0f"], 1, 1, "<C-a>"),
    c("num:hex 0xff", &["0xff"], 1, 1, "<C-a>"),
    c("num:hex 0xFF", &["0xFF"], 1, 1, "<C-a>"),
    c("num:hex 0xaB", &["0xaB"], 1, 1, "<C-a>"),
    c("num:hex C-x below zero", &["0x0"], 1, 1, "<C-x>"),
    c("num:octal not default 007", &["007"], 1, 1, "<C-a>"),
    cs(
        "num:octal nf=octal 007",
        &["007"],
        1,
        1,
        "<C-a>",
        "vim.o.nrformats='bin,octal,hex'",
    ),
    c("num:binary 0b101", &["0b101"], 1, 1, "<C-a>"),
    c("num:leading zeros 009", &["009"], 1, 1, "<C-a>"),
    c("num:leading zeros 0099 C-x", &["0099"], 1, 1, "<C-x>"),
    c("num:word digits foo9", &["foo9"], 1, 1, "<C-a>"),
    c("num:a-5", &["a-5"], 1, 1, "<C-a>"),
    c("num:1.5 on 1", &["1.5"], 1, 1, "<C-a>"),
    c("num:1.5 on 5", &["1.5"], 1, 3, "<C-a>"),
    c("num:no number after cursor", &["5 x"], 1, 3, "<C-a>"),
    c("num:cursor after C-a", &["x 5 y"], 1, 1, "<C-a>"),
    c("num:cursor on last digit", &["x 100 y"], 1, 1, "<C-a>"),
    c("num:C-a .", &["x 5"], 1, 1, "<C-a>."),
    c("num:3C-a .", &["x 5"], 1, 1, "3<C-a>."),
    c("num:3C-a 2.", &["x 5"], 1, 1, "3<C-a>2."),
    c("num:200C-x", &["100"], 1, 1, "200<C-x>"),
    c("num:V C-a", &["1", "1", "1"], 1, 1, "Vjj<C-a>"),
    c("num:V g C-a", &["1", "1", "1"], 1, 1, "Vjjg<C-a>"),
    c("num:V 2g C-a", &["1", "1", "1"], 1, 1, "Vjj2g<C-a>"),
    c("num:v C-a partial", &["1 1", "1 1"], 1, 1, "vj<C-a>"),
    c("num:C-v block C-a", &["1 1", "1 1"], 1, 3, "<C-v>j<C-a>"),
    c("num:C-a hex mid", &["0x10"], 1, 3, "<C-a>"),
    c("num:5C-a on -3", &["-3"], 1, 1, "5<C-a>"),
    c("num:C-x foo0", &["foo0"], 1, 1, "<C-x>"),
    c("num:1-2 on 1", &["1-2"], 1, 1, "<C-a>"),
    c("num:1-2 on -", &["1-2"], 1, 2, "<C-a>"),
    c("num:1-2 on 2", &["1-2"], 1, 3, "<C-a>"),
    cs(
        "num:alpha",
        &["a"],
        1,
        1,
        "<C-a>",
        "vim.o.nrformats='alpha'",
    ),
    c("num:C-a on 9 width", &["9"], 1, 1, "<C-a>"),
    c(
        "num:C-a on 99999999999999999999 overflow",
        &["99999999999999999999"],
        1,
        1,
        "<C-a>",
    ),
    c(
        "num:V C-a skips lines without numbers",
        &["1", "x", "1"],
        1,
        1,
        "Vjjg<C-a>",
    ),
    c(
        "num:C-a on number after word char",
        &["ab12"],
        1,
        3,
        "<C-a>",
    ),
    c(
        "num:V C-a only first number per line",
        &["1 2", "3 4"],
        1,
        1,
        "Vj<C-a>",
    ),
    c("num:C-a 0x with uppercase X", &["0X0f"], 1, 1, "<C-a>"),
    c("num:C-a on negative hex? -0x1", &["-0x1"], 1, 1, "<C-a>"),
    c("num:C-x on 0 leading zeros 000", &["000"], 1, 1, "<C-x>"),
    c(
        "num:C-a cursor on space before number",
        &["a 1"],
        1,
        2,
        "<C-a>",
    ),
    c("num:C-a on 10 then u", &["10"], 1, 1, "<C-a>u"),
    c("num:V C-a cursor", &["1", "1"], 1, 1, "Vj<C-a>"),
    c(
        "num:v C-a on -5 in visual (no minus)",
        &["x -5"],
        1,
        4,
        "vl<C-a>",
    ),
];

// ─────────────────────────── M. scrolling ───────────────────────────
const CASES_SCROLL: &[Case] = &[
    c("scroll:C-d", LONG, 1, 1, "<C-d>"),
    c("scroll:C-u", LONG, 30, 1, "<C-u>"),
    c("scroll:C-f", LONG, 1, 1, "<C-f>"),
    c("scroll:C-b", LONG, 60, 1, "<C-b>"),
    c("scroll:C-e pushes cursor", LONG, 1, 1, "<C-e>"),
    c("scroll:3C-e", LONG, 1, 1, "3<C-e>"),
    c("scroll:G C-y", LONG, 1, 1, "G<C-y>"),
    c("scroll:H from 30", LONG, 30, 1, "H"),
    c("scroll:M from 30", LONG, 30, 1, "M"),
    c("scroll:L from 30", LONG, 30, 1, "L"),
    c("scroll:3H", LONG, 30, 1, "3H"),
    c("scroll:3L", LONG, 30, 1, "3L"),
    c("scroll:ztL", LONG, 20, 1, "ztL"),
    c("scroll:zzH", LONG, 20, 1, "zzH"),
    c("scroll:zbH", LONG, 30, 1, "zbH"),
    c("scroll:z<CR>L", LONG, 20, 1, "z<CR>L"),
    c("scroll:z.H", LONG, 20, 1, "z.H"),
    c("scroll:z-H", LONG, 30, 1, "z-H"),
    c("scroll:C-d C-d", LONG, 1, 1, "<C-d><C-d>"),
    c("scroll:5C-d C-d", LONG, 1, 1, "5<C-d><C-d>"),
    c("scroll:C-d near end", LONG, 55, 1, "<C-d>"),
    c("scroll:C-f at end", LONG, 60, 1, "<C-f>"),
    c("scroll:C-b at start", LONG, 1, 1, "<C-b>"),
    c("scroll:C-f C-f", LONG, 1, 1, "<C-f><C-f>"),
    c("scroll:C-f C-b", LONG, 1, 1, "<C-f><C-b>"),
    c("scroll:G H", LONG, 1, 1, "GH"),
    c("scroll:G M", LONG, 1, 1, "GM"),
    c("scroll:dL", LONG, 1, 1, "dL"),
    c("scroll:dH", LONG, 10, 1, "dH"),
    c("scroll:dM", LONG, 1, 1, "dM"),
    c("scroll:C-d col kept (nosol)", LONG, 1, 3, "<C-d>"),
    cs(
        "scroll:C-d col sol",
        LONG,
        1,
        3,
        "<C-d>",
        "vim.o.startofline=true",
    ),
    c("scroll:so=5 30G H", LONG, 1, 1, ":set so=5<CR>30GH"),
    c("scroll:so=5 30G L", LONG, 1, 1, ":set so=5<CR>30GL"),
    c("scroll:so=5 C-e", LONG, 1, 1, ":set so=5<CR><C-e>"),
    c("scroll:25j H", LONG, 1, 1, "25jH"),
    c("scroll:25j L", LONG, 1, 1, "25jL"),
    c("scroll:C-u at top", LONG, 1, 1, "<C-u>"),
    c("scroll:C-u C-u from end", LONG, 60, 1, "<C-u><C-u>"),
    c("scroll:C-e C-e H", LONG, 1, 1, "<C-e><C-e>H"),
    c("scroll:G C-y C-y H", LONG, 1, 1, "G<C-y><C-y>H"),
    c("scroll:zt C-e", LONG, 20, 1, "zt<C-e>"),
    c("scroll:zt k", LONG, 20, 1, "ztk"),
    c("scroll:zt k H", LONG, 20, 1, "ztkH"),
    c("scroll:zb j L", LONG, 30, 1, "zbjL"),
    c("scroll:C-d then H L", LONG, 1, 1, "<C-d>H"),
    c("scroll:C-d then L", LONG, 1, 1, "<C-d>L"),
    c("scroll:C-f then H", LONG, 1, 1, "<C-f>H"),
    c("scroll:C-f then L", LONG, 1, 1, "<C-f>L"),
    c("scroll:C-b after G then H", LONG, 60, 1, "<C-b>H"),
    c("scroll:C-b after G then L", LONG, 60, 1, "<C-b>L"),
    c("scroll:10C-e H", LONG, 1, 1, "10<C-e>H"),
    c("scroll:50% H", LONG, 1, 1, "50%H"),
    c("scroll:30G zz H L", LONG, 1, 1, "30GzzHjL"),
    c("scroll:j at bottom scrolls one", LONG, 1, 1, "22jH"),
    c("scroll:k at top", LONG, 30, 1, "ztk"),
    c("scroll:G then k ×5 H", LONG, 1, 1, "GkkkkkH"),
    c("scroll:cursor after zt col", LONG, 20, 3, "zt"),
    c("scroll:C-d twice then C-u", LONG, 1, 1, "<C-d><C-d><C-u>"),
    c(
        "scroll:3C-d sets scroll then C-u",
        LONG,
        30,
        1,
        "3<C-d><C-u>",
    ),
    c("scroll:3<C-f>", LONG, 1, 1, "3<C-f>"),
    c("scroll:2<C-b>", LONG, 60, 1, "2<C-b>"),
    c(
        "scroll:z<CR> col first nonblank",
        &["a", "   b", "c"],
        2,
        1,
        "z<CR>",
    ),
    c("scroll:zt col kept", &["a", "   b", "c"], 2, 1, "zt"),
    c("scroll:H on short buffer", &["a", "b", "c"], 3, 1, "H"),
    c("scroll:L on short buffer", &["a", "b", "c"], 1, 1, "L"),
    c(
        "scroll:M on short buffer",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "M",
    ),
    c(
        "scroll:C-d on short buffer",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "<C-d>",
    ),
    c(
        "scroll:C-f on short buffer",
        &["a", "b", "c", "d", "e"],
        1,
        1,
        "<C-f>",
    ),
    c(
        "scroll:C-e on short buffer",
        &["a", "b", "c"],
        1,
        1,
        "<C-e>",
    ),
    c(
        "scroll:C-u on short buffer",
        &["a", "b", "c", "d", "e"],
        5,
        1,
        "<C-u>",
    ),
    c(
        "scroll:3<C-d> on short",
        &["a", "b", "c", "d", "e", "f", "g", "h"],
        1,
        1,
        "3<C-d>",
    ),
    c("scroll:C-d at last line", LONG, 60, 1, "<C-d>"),
    c("scroll:C-u at line 2", LONG, 2, 1, "<C-u>"),
];

// ─────────────────────────── N. word motions & misc motions ───────────────────────────
const CASES_WORD: &[Case] = &[
    c("word:w punctuation", &["foo.bar baz"], 1, 1, "w"),
    c("word:w on punct", &["foo.bar baz"], 1, 4, "w"),
    c("word:w punct run", &["a..b"], 1, 1, "w"),
    c("word:ww punct run", &["a..b"], 1, 1, "ww"),
    c("word:w to next line", &["foo", "  bar"], 1, 1, "w"),
    c("word:w at last word of buffer", &["foo bar"], 1, 5, "w"),
    c("word:w at end of buffer", &["foo bar"], 1, 7, "w"),
    c("word:w onto blank line", &["foo", "", "bar"], 1, 1, "w"),
    c("word:w from blank line", &["foo", "", "bar"], 2, 1, "w"),
    c("word:w over trailing spaces", &["foo   ", "bar"], 1, 1, "w"),
    c("word:3w", &["a b c d e"], 1, 1, "3w"),
    c("word:w underscore", &["foo_bar baz"], 1, 1, "w"),
    c("word:w digits", &["foo123 bar"], 1, 1, "w"),
    c("word:w a-b", &["a-b c"], 1, 1, "w"),
    c("word:W", &["a.b c"], 1, 1, "W"),
    c("word:e", &["foo bar"], 1, 1, "e"),
    c("word:ee", &["foo bar"], 1, 1, "ee"),
    c("word:e on last char", &["foo bar"], 1, 3, "e"),
    c("word:e punctuation", &["foo.bar"], 1, 1, "e"),
    c("word:ee punctuation", &["foo.bar"], 1, 1, "ee"),
    c("word:eee punctuation", &["foo.bar"], 1, 1, "eee"),
    c("word:e across blank line", &["foo", "", "bar"], 1, 3, "e"),
    c("word:E", &["a.b c.d"], 1, 1, "E"),
    c("word:b at start", &["foo"], 1, 1, "b"),
    c("word:b across line", &["foo", "bar"], 2, 1, "b"),
    c("word:b from mid word", &["foo bar"], 1, 6, "b"),
    c("word:b punctuation", &["foo.bar"], 1, 7, "b"),
    c("word:bb punctuation", &["foo.bar"], 1, 7, "bb"),
    c("word:bbb punctuation", &["foo.bar"], 1, 7, "bbb"),
    c("word:B", &["a.b c.d"], 1, 7, "B"),
    c("word:ge", &["foo bar"], 1, 5, "ge"),
    c("word:ge at start", &["foo bar"], 1, 1, "ge"),
    c("word:ge across lines", &["foo", "bar"], 2, 1, "ge"),
    c("word:ge onto blank", &["foo", "", "bar"], 3, 1, "ge"),
    c("word:ge punctuation", &["foo.bar"], 1, 5, "ge"),
    c("word:gE", &["a.b c.d"], 1, 5, "gE"),
    c("word:2e", &["a b c"], 1, 1, "2e"),
    c("word:2b", &["a b c"], 1, 5, "2b"),
    c("word:10w beyond", &["a b"], 1, 1, "10w"),
    c("word:ww single chars", &["a b c"], 1, 1, "ww"),
    c("word:e single char", &["a b"], 1, 1, "e"),
    c("word:w tabs", &["a\tb"], 1, 1, "w"),
    c("word:w on last char of line", &["ab", "cd"], 1, 2, "w"),
    c("word:b onto blank line", &["foo", "", "bar"], 3, 1, "b"),
    c("word:e at end of buffer", &["ab"], 1, 2, "e"),
    c(
        "word:w over multiple blank lines",
        &["a", "", "", "b"],
        1,
        1,
        "w",
    ),
    c(
        "word:ww over multiple blank lines",
        &["a", "", "", "b"],
        1,
        1,
        "ww",
    ),
    c(
        "word:w from whitespace-only line",
        &["a", "   ", "b"],
        2,
        1,
        "w",
    ),
    c(
        "word:e from whitespace-only line",
        &["a", "   ", "b"],
        2,
        1,
        "e",
    ),
    c(
        "word:) sentences",
        &["Hello world.  Second one.  Third."],
        1,
        1,
        ")",
    ),
    c(
        "word:)) sentences",
        &["Hello world.  Second one.  Third."],
        1,
        1,
        "))",
    ),
    c(
        "word:( sentence",
        &["Hello world.  Second one.  Third."],
        1,
        20,
        "(",
    ),
    c(
        "word:) single space",
        &["Hello world. Second one."],
        1,
        1,
        ")",
    ),
    c("word:) across lines", &["Hello.", "World."], 1, 1, ")"),
    c("word:( para", &["a", "", "b"], 3, 1, "("),
    c("word:) with ! ?", &["A! B? C."], 1, 1, ")"),
    c("word:) with quote", &["A.\" B."], 1, 1, ")"),
    c("word:) at end goes to eol", &["A. B."], 1, 4, ")"),
    c("word:}", &["a", "", "b", "", "c"], 1, 1, "}"),
    c("word:}}", &["a", "", "b", "", "c"], 1, 1, "}}"),
    c("word:}}}", &["a", "", "b", "", "c"], 1, 1, "}}}"),
    c("word:{", &["a", "", "b", "", "c"], 5, 1, "{"),
    c("word:2}", &["a", "", "b", "", "c"], 1, 1, "2}"),
    c("word:} multiple blanks", &["a", "", "", "b"], 1, 1, "}"),
    c("word:}} multiple blanks", &["a", "", "", "b"], 1, 1, "}}"),
    c("word:} from blank", &["a", "", "", "b"], 2, 1, "}"),
    c("word:{ at start", &["a", "b"], 2, 1, "{"),
    c(
        "word:} whitespace-only line not blank",
        &["a", "   ", "b", ""],
        1,
        1,
        "}",
    ),
    c("word:]]", &["{", "a", "}", "{", "b"], 1, 1, "]]"),
    c("word:[[", &["{", "a", "}", "{", "b"], 5, 1, "[["),
    c("word:[{", &["{", "a", "}"], 2, 1, "[{"),
    c("word:]}", &["{", "a", "}"], 2, 1, "]}"),
    c("word:[(", &["(a (b) c)"], 1, 5, "[("),
    c("word:])", &["(a (b) c)"], 1, 5, "])"),
    c("word:% on (", &["(a (b) c)"], 1, 1, "%"),
    c("word:% inside", &["(a (b) c)"], 1, 2, "%"),
    c("word:% on [", &["[a]"], 1, 1, "%"),
    c("word:% on ]", &["[a]"], 1, 3, "%"),
    c("word:% not found", &["abc"], 1, 1, "%"),
    c("word:% multiline", &["{", "a", "}"], 1, 1, "%"),
    c("word:% nested", &["((a))"], 1, 1, "%"),
    c("word:% on closing nested", &["((a))"], 1, 5, "%"),
    c("word:% in quotes", &["\"(\" )"], 1, 2, "%"),
    c("word:% on quote char", &["\"a\" (b)"], 1, 1, "%"),
    c("word:50%", LONG, 1, 1, "50%"),
    c("word:_", &["  ab"], 1, 4, "_"),
    c("word:2_", &["  ab", "  cd"], 1, 4, "2_"),
    c("word:+", &["  a", "  b"], 1, 1, "+"),
    c("word:-", &["  a", "  b"], 2, 1, "-"),
    c("word:<CR>", &["  a", "  b"], 1, 1, "<CR>"),
    c("word:4|", &["abcdef"], 1, 1, "4|"),
    c("word:|", &["abcdef"], 1, 4, "|"),
    c("word:2$", &["ab", "cd", "ef"], 1, 1, "2$"),
    c("word:$jj", &["abcdef", "ab", "abcdef"], 1, 1, "$jj"),
    c(
        "word:jj col memory",
        &["abcdef", "ab", "abcdef"],
        1,
        5,
        "jj",
    ),
    c("word:j col memory short", &["abcdef", "ab"], 1, 5, "j"),
    c("word:5l past end", &["abc"], 1, 1, "5l"),
    c("word:h at start", &["abc"], 1, 1, "h"),
    c("word:0", &["  ab"], 1, 4, "0"),
    c("word:^", &["  ab"], 1, 4, "^"),
    c("word:g_", &["ab  "], 1, 1, "g_"),
    c("word:gg indented (nosol)", &["  a", "b"], 2, 1, "gg"),
    cs(
        "word:gg indented (sol)",
        &["  a", "b"],
        2,
        1,
        "gg",
        "vim.o.startofline=true",
    ),
    c("word:G indented (nosol)", &["a", "  b"], 1, 1, "G"),
    cs(
        "word:G indented (sol)",
        &["a", "  b"],
        1,
        1,
        "G",
        "vim.o.startofline=true",
    ),
    c("word:5G", &["1", "2", "3", "4", "5", "6"], 1, 1, "5G"),
    c("word:5gg", &["1", "2", "3", "4", "5", "6"], 1, 1, "5gg"),
    c("word:10j beyond", &["a", "b", "c"], 1, 1, "10j"),
    c("word:10k beyond", &["a", "b", "c"], 3, 1, "10k"),
    c("word:w on empty buffer", &[""], 1, 1, "w"),
    c("word:x on empty buffer", &[""], 1, 1, "x"),
    c("word:dd on empty buffer", &[""], 1, 1, "dd"),
    c("word:yyp on empty buffer", &[""], 1, 1, "yyp"),
    c("word:$ then j to longer", &["ab", "abcdef"], 1, 1, "$j"),
    c(
        "word:$ then k then j",
        &["abcdef", "ab", "abcdef"],
        2,
        1,
        "$kj",
    ),
    c("word:gj gk nowrap", &["abc", "def", "ghi"], 1, 2, "gjgk"),
    c("word:go", &["ab", "cd"], 1, 1, "5go"),
    c("word:$ with count 1", &["ab", "cd"], 1, 1, "1$"),
    c("word:d$ then j col", &["abcdef", "abcdef"], 1, 3, "d$j"),
    c("word:x at eol then j", &["abc", "abcdef"], 1, 3, "xj"),
    c(
        "word:A esc then j col memory",
        &["ab", "abcdef"],
        1,
        1,
        "A<Esc>j",
    ),
    c("word:i esc then j", &["abcdef", "abcdef"], 1, 4, "i<Esc>j"),
    c("word:$ then h then j", &["abcdef", "abcdef"], 1, 1, "$hj"),
    c("word:w then j col", &["ab cd", "abcdef"], 1, 1, "wj"),
    c("word:e then j", &["abc def", "abcdef"], 1, 1, "ej"),
    c("word:yy then j col", &["abcdef", "abcdef"], 1, 3, "yyj"),
    c("word:p then j col", &["abcdef", "abcdef"], 1, 3, "ylpj"),
    c(
        "word:dd then j col? (nosol)",
        &["abcdef", "abcdef", "abcdef"],
        1,
        3,
        "ddj",
    ),
    c("word:>> then j col", &["abcdef", "abcdef"], 1, 3, ">>j"),
    c("word:u then j col", &["abcdef", "abcdef"], 1, 3, "xuj"),
    c(
        "word:: then j col",
        &["abcdef", "abcdef"],
        1,
        3,
        ":noh<CR>j",
    ),
    c("word:/ then j col", &["abcdef", "abcdef"], 1, 1, "/c<CR>j"),
    c("word:zz then j col", &["abcdef", "abcdef"], 1, 3, "zzj"),
    c(
        "word:5G then j col (nosol)",
        &["abcdef", "abcdef", "abcdef"],
        1,
        3,
        "2Gj",
    ),
    cs(
        "word:5G then j col (sol)",
        &["abcdef", "abcdef", "abcdef"],
        1,
        3,
        "2Gj",
        "vim.o.startofline=true",
    ),
    c(
        "word:H then j col",
        &["abcdef", "abcdef", "abcdef"],
        2,
        3,
        "Hj",
    ),
    c("word:( ) with tab", &["A.\tB."], 1, 1, ")"),
    c("word:w on CJK? skip", &["a"], 1, 1, "l"),
    c("word:e on 2-char word end", &["ab cd"], 1, 2, "e"),
    c("word:cw on last char of word", &["ab cd"], 1, 2, "cwX<Esc>"),
    c(
        "word:w at eol with trailing space",
        &["ab ", "cd"],
        1,
        2,
        "w",
    ),
    c(
        "word:b from col1 of indented line",
        &["ab", "  cd"],
        2,
        3,
        "b",
    ),
    c(
        "word:b from start of indented line",
        &["ab", "  cd"],
        2,
        1,
        "b",
    ),
    c(
        "word:e from indented line start",
        &["ab", "  cd"],
        2,
        1,
        "e",
    ),
    c(
        "word:ge from indented line start",
        &["ab", "  cd"],
        2,
        1,
        "ge",
    ),
    c("word:W across lines", &["a.b", "c.d"], 1, 3, "W"),
    c("word:E across lines", &["a.b", "c.d"], 1, 3, "E"),
    c("word:B across lines", &["a.b", "c.d"], 2, 1, "B"),
    c("word:w over punct then blank", &["a.", "", "b"], 1, 2, "w"),
    c("word:dw over punct at eol", &["a.", "b"], 1, 2, "dw"),
    c("word:cw at eol punct", &["a.", "b"], 1, 2, "cwX<Esc>"),
    c("word:3e beyond", &["a b"], 1, 1, "3e"),
    c("word:w keyword vs nonkeyword @", &["a@b c"], 1, 1, "w"),
    c("word:w with iskeyword dash? -", &["a-b-c d"], 1, 1, "www"),
];

// ─────────────────────────── O. text objects ───────────────────────────
const CASES_TO: &[Case] = &[
    c("to:daw mid", &["foo bar baz"], 1, 5, "daw"),
    c("to:daw start", &["foo bar baz"], 1, 1, "daw"),
    c("to:daw last word", &["foo bar baz"], 1, 9, "daw"),
    c("to:daw on whitespace", &["foo  bar"], 1, 4, "daw"),
    c("to:diw on whitespace", &["foo  bar"], 1, 4, "diw"),
    c("to:diw punctuation", &["foo.bar"], 1, 4, "diw"),
    c("to:daw punctuation", &["foo.bar"], 1, 4, "daw"),
    c("to:d2aw", &["a b c d"], 1, 1, "d2aw"),
    c("to:d3iw", &["a b c d"], 1, 1, "d3iw"),
    c("to:c2aw", &["a b c d"], 1, 1, "c2awX<Esc>"),
    c("to:daw single word line", &["foo"], 1, 1, "daw"),
    c("to:daw leading space only", &["  foo"], 1, 3, "daw"),
    c("to:daW", &["foo.bar baz"], 1, 2, "daW"),
    c("to:diW", &["foo.bar baz"], 1, 2, "diW"),
    c("to:das", &["One two.  Three four.  Five."], 1, 12, "das"),
    c("to:dis", &["One two.  Three four.  Five."], 1, 12, "dis"),
    c(
        "to:das last sentence",
        &["One two.  Three four."],
        1,
        12,
        "das",
    ),
    c(
        "to:das first sentence",
        &["One two.  Three four."],
        1,
        2,
        "das",
    ),
    c(
        "to:dis on whitespace between",
        &["One two.  Three four."],
        1,
        10,
        "dis",
    ),
    c("to:dip", &["a", "b", "", "c"], 1, 1, "dip"),
    c("to:dap", &["a", "b", "", "c"], 1, 1, "dap"),
    c(
        "to:dap trailing no blank",
        &["a", "", "b", "c"],
        3,
        1,
        "dap",
    ),
    c("to:dip on blank lines", &["a", "", "", "b"], 2, 1, "dip"),
    c("to:dap on blank", &["a", "", "", "b"], 2, 1, "dap"),
    c("to:d2ap", &["a", "", "b", "", "c"], 1, 1, "d2ap"),
    c("to:yap cursor", &["a", "", "b", "c"], 3, 1, "yap"),
    c("to:yip cursor", &["a", "", "b", "c"], 4, 1, "yip"),
    c("to:dip at last para", &["a", "", "b"], 3, 1, "dip"),
    c("to:dap only para", &["a", "b"], 1, 1, "dap"),
    c("to:di( inside", &["f(a, b)"], 1, 3, "di("),
    c("to:di( on (", &["f(a, b)"], 1, 2, "di("),
    c("to:di( on )", &["f(a, b)"], 1, 7, "di("),
    c("to:di( nested inner", &["f(a, (b), c)"], 1, 7, "di("),
    c("to:d2i(", &["f(a, (b), c)"], 1, 7, "d2i("),
    c("to:da( nested", &["f(a, (b), c)"], 1, 7, "da("),
    c("to:di( before paren same line", &["x f(a)"], 1, 1, "di("),
    c("to:di( not inside", &["abc"], 1, 1, "di("),
    c("to:dib", &["f(a)"], 1, 3, "dib"),
    c("to:diB", &["f{a}"], 1, 3, "diB"),
    c("to:di{ multiline", &["{", "  a", "  b", "}"], 2, 3, "di{"),
    c("to:da{ multiline", &["{", "  a", "  b", "}"], 2, 3, "da{"),
    c(
        "to:ci{ multiline",
        &["{", "  a", "  b", "}"],
        2,
        3,
        "ci{X<Esc>",
    ),
    c("to:di{ same line", &["f {a}"], 1, 4, "di{"),
    c(
        "to:di{ on line with brace and text",
        &["if (x) {", "  a", "}"],
        2,
        3,
        "di{",
    ),
    c(
        "to:yi{ cursor multiline",
        &["{", "  a", "  b", "}"],
        3,
        3,
        "yi{",
    ),
    c("to:di[", &["a[1]"], 1, 3, "di["),
    c("to:da[", &["a[1]"], 1, 3, "da["),
    c("to:di\" inside", &["x \"ab\" y"], 1, 4, "di\""),
    c("to:di\" on opening", &["x \"ab\" y"], 1, 3, "di\""),
    c("to:di\" on closing", &["x \"ab\" y"], 1, 6, "di\""),
    c("to:di\" before quotes", &["x \"ab\" y"], 1, 1, "di\""),
    c("to:da\" before quotes", &["x \"ab\" y"], 1, 1, "da\""),
    c("to:di\" escaped", &["\"a\\\"b\""], 1, 2, "di\""),
    c(
        "to:di\" between two strings",
        &["\"a\" x \"b\""],
        1,
        5,
        "di\"",
    ),
    c("to:di\" second string", &["\"a\" x \"b\""], 1, 8, "di\""),
    c("to:di'", &["x 'ab' y"], 1, 4, "di'"),
    c("to:di`", &["x `ab` y"], 1, 4, "di`"),
    c("to:ci\"", &["x \"ab\" y"], 1, 4, "ci\"X<Esc>"),
    c("to:yi\" cursor", &["x \"ab\" y"], 1, 5, "yi\""),
    c("to:di\" empty string", &["x \"\" y"], 1, 4, "di\""),
    c("to:di\" after last quote", &["\"ab\" y"], 1, 6, "di\""),
    c(
        "to:da\" with leading and trailing ws",
        &["a  \"b\"  c"],
        1,
        5,
        "da\"",
    ),
    c("to:dit", &["<a><b>x</b></a>"], 1, 7, "dit"),
    c("to:dat", &["<a><b>x</b></a>"], 1, 7, "dat"),
    c("to:d2it", &["<a><b>x</b></a>"], 1, 7, "d2it"),
    c("to:dit on tag", &["<a><b>x</b></a>"], 1, 2, "dit"),
    c("to:dit multiline", &["<div>", "  x", "</div>"], 2, 3, "dit"),
    c("to:cit", &["<a>x</a>"], 1, 4, "citY<Esc>"),
    c("to:dit with attrs", &["<a href=\"x\">y</a>"], 1, 15, "dit"),
    c(
        "to:dat self-closing inside",
        &["<a><br/>x</a>"],
        1,
        9,
        "dat",
    ),
    c("to:d5aw too many", &["a b"], 1, 1, "d5aw"),
    c("to:diw single char", &["a b"], 1, 1, "diw"),
    c("to:daw leading whitespace", &["  foo bar"], 1, 1, "daw"),
    c("to:daw eol trailing space", &["foo bar "], 1, 5, "daw"),
    c("to:ciw on whitespace", &["a   b"], 1, 3, "ciwX<Esc>"),
    c("to:cip", &["a", "b", "", "c"], 1, 1, "cipX<Esc>"),
    c("to:di( across lines", &["f(a,", "  b)"], 1, 3, "di("),
    c("to:yi( cursor", &["f(a, b)"], 1, 5, "yi("),
    c("to:ya( cursor", &["f(a, b)"], 1, 5, "ya("),
    c("to:daw punctuation attached", &["foo, bar"], 1, 1, "daw"),
    c("to:2daw", &["a b c d"], 1, 1, "2daw"),
    c("to:daw at end with count", &["a b c"], 1, 5, "d2aw"),
    c("to:di< nested", &["<a<b>c>"], 1, 5, "di<"),
    c("to:da< outer", &["<a<b>c>"], 1, 2, "da<"),
    c("to:di( empty", &["f()"], 1, 2, "di("),
    c("to:ci( empty", &["f()"], 1, 2, "ci(X<Esc>"),
    c("to:da( empty", &["f()"], 1, 2, "da("),
    c(
        "to:di( with newline after open",
        &["f(", "a", "b)"],
        2,
        1,
        "di(",
    ),
    c("to:da{ with trailing text", &["x {a} y"], 1, 4, "da{"),
    c("to:diw at eol on space", &["foo "], 1, 4, "diw"),
    c("to:daw on multi spaces at start", &["   foo"], 1, 1, "daw"),
    c(
        "to:dis multi-line sentence",
        &["One two", "three.  Four."],
        1,
        1,
        "dis",
    ),
    c(
        "to:das multi-line",
        &["One two", "three.  Four."],
        1,
        1,
        "das",
    ),
    c(
        "to:dip with indented lines",
        &["  a", "  b", "", "c"],
        1,
        1,
        "dip",
    ),
    c("to:vipJ", &["a", "b", "", "c"], 1, 1, "vipJ"),
    c("to:>ap", &["a", "b", "", "c"], 1, 1, ">ap"),
    c("to:=ip", &["  a", "    b", "", "c"], 1, 1, "=ip"),
    c("to:gUip", &["a", "b", "", "c"], 1, 1, "gUip"),
    c("to:yi\" then P", &["x \"ab\" y"], 1, 4, "yi\"P"),
    c("to:ci' then .", &["'a' 'b'"], 1, 2, "ci'X<Esc>4l."),
    c(
        "to:di\" cursor on quote when 3 quotes",
        &["a \"b\" c \" d"],
        1,
        8,
        "di\"",
    ),
    c("to:dit nested same tag", &["<a><a>x</a></a>"], 1, 7, "dit"),
    c("to:dat on closing tag", &["<a>x</a> y"], 1, 6, "dat"),
    c("to:dit no tag", &["abc"], 1, 1, "dit"),
    c("to:di( count 3 too many", &["(a)"], 1, 2, "d3i("),
    c("to:daw on only whitespace line", &["   "], 1, 2, "daw"),
    c("to:diw digits", &["ab 12 cd"], 1, 5, "diw"),
    c(
        "to:dip cursor after",
        &["a", "", "b", "c", "", "d"],
        3,
        1,
        "dip",
    ),
    c(
        "to:dap cursor after",
        &["a", "", "b", "c", "", "d"],
        3,
        1,
        "dap",
    ),
];

// ─────────────────────────── P. misc ───────────────────────────
const CASES_MISC: &[Case] = &[
    c("misc:5p charwise", &["ab"], 1, 1, "yl5p"),
    c("misc:p at last line linewise", &["a", "b"], 2, 1, "yyp"),
    c("misc:P at first", &["a", "b"], 1, 1, "yyP"),
    c("misc:yyP cursor", &["  a", "b"], 1, 3, "yyP"),
    c("misc:ddP", &["a", "b", "c"], 2, 1, "ddP"),
    c("misc:2yy P", &["a", "b", "c"], 2, 1, "2yyP"),
    c(
        "misc:p multi-line charwise cursor",
        &["ab", "cd"],
        1,
        1,
        "vjy$p",
    ),
    c("misc:gp charwise multi", &["ab", "cd"], 1, 1, "vjy$gp"),
    c("misc:gP charwise", &["abc"], 1, 2, "ylgP"),
    c("misc:2gp", &["a", "b"], 1, 1, "yy2gp"),
    c(
        "misc:p with count linewise cursor",
        &["a", "b"],
        1,
        1,
        "yy3p",
    ),
    c("misc:P count", &["ab"], 1, 2, "yl3P"),
    c("misc:xp end", &["ab"], 1, 2, "xp"),
    c("misc:deep count 100x", &["abc"], 1, 1, "100x"),
    c("misc:count on i then esc col", &["abc"], 1, 3, "3ix<Esc>"),
    c("misc:count 0 not a count", &["abc def"], 1, 5, "0"),
    c("misc:10 then 0", &["abcdefghijklmnop"], 1, 1, "10l0"),
    c("misc:d10l", &["abcdefghijklmnop"], 1, 1, "d10l"),
    c("misc:20|", &["abc"], 1, 1, "20|"),
    c("misc:~ count past eol cursor", &["abc"], 1, 1, "10~"),
    c("misc:J with 2 count", &["a", "b", "c"], 1, 1, "2J"),
    c("misc:r on empty line", &["", "a"], 1, 1, "rx"),
    c("misc:r Tab", &["ab"], 1, 1, "r<Tab>"),
    c(
        "misc:R at eol then BS",
        &["ab"],
        1,
        2,
        "Rxyz<BS><BS><BS><BS><Esc>",
    ),
    c("misc:s on empty line", &[""], 1, 1, "sX<Esc>"),
    c("misc:cw on space at eol", &["ab "], 1, 3, "cwX<Esc>"),
    c("misc:D then p", &["abc"], 1, 2, "Dp"),
    c("misc:C then .", &["abc", "def"], 1, 2, "CX<Esc>j0."),
    c("misc:cc then p", &["  a", "b"], 1, 1, "ccX<Esc>jp"),
    c("misc:S then P", &["a", "b"], 1, 1, "SX<Esc>jP"),
    c(
        "misc:& after &&",
        &["a a a", "a a a"],
        1,
        1,
        ":s/a/b/g<CR>j&",
    ),
    c(
        "misc:g& after range",
        &["a", "a", "a"],
        1,
        1,
        ":1s/a/b/<CR>g&",
    ),
    c("misc:: then Esc", &["abc"], 1, 2, ":<Esc>x"),
    c("misc:/ then Esc", &["abc"], 1, 2, "/b<Esc>x"),
    c("misc:d then Esc", &["abc"], 1, 2, "d<Esc>x"),
    c("misc:2d then Esc", &["abc"], 1, 2, "2d<Esc>x"),
    c("misc:3 then Esc then x", &["abcdef"], 1, 1, "3<Esc>x"),
    c("misc:\"a then Esc then x", &["abcdef"], 1, 1, "\"a<Esc>x"),
    c("misc:q then Esc", &["abc"], 1, 1, "q<Esc>x"),
    c("misc:m then Esc", &["abc"], 1, 1, "m<Esc>x"),
    c("misc:' then Esc", &["abc"], 1, 1, "'<Esc>x"),
    c("misc:g then Esc", &["abc"], 1, 1, "g<Esc>x"),
    c("misc:z then Esc", &["abc"], 1, 1, "z<Esc>x"),
    c("misc:f then Esc", &["abc"], 1, 1, "f<Esc>x"),
    c("misc:r then Esc", &["abc"], 1, 1, "r<Esc>x"),
    c("misc:ci then Esc", &["abc"], 1, 1, "ci<Esc>x"),
    c("misc:C-w then Esc", &["abc"], 1, 1, "<C-w><Esc>x"),
    c("misc:Esc in normal no-op", &["abc"], 1, 2, "<Esc>"),
    c("misc:: with count", &["a", "b", "c", "d"], 1, 1, "3:d<CR>"),
    c("misc:3:s", &["a", "a", "a", "a"], 1, 1, "3:s/a/b/<CR>"),
    c("misc:Q skip", &["a"], 1, 1, "l"),
    c("misc:gv after p", &["ab", "cd"], 1, 1, "yyjVpgvd"),
    c("misc:C-l noop", &["abc"], 1, 2, "<C-l>x"),
    c("misc:C-c in insert", &["abc"], 1, 1, "ix<C-c>x"),
    c("misc:C-[ in insert", &["abc"], 1, 1, "ix<C-[>x"),
    c("misc:insert Esc with count 0", &["abc"], 1, 1, "0ix<Esc>"),
    c(
        "misc:count then : then range",
        &["a", "b", "c"],
        1,
        1,
        "2:normal Ax<CR>",
    ),
    c("misc:d3d? invalid", &["a", "b", "c", "d", "e"], 1, 1, "d3d"),
    c("misc:2d2d", &["a", "b", "c", "d", "e", "f"], 1, 1, "2d2d"),
    c("misc:y then y with count", &["a", "b", "c"], 1, 1, "y2yP"),
    c("misc:c3c", &["a", "b", "c", "d"], 1, 1, "c3cX<Esc>"),
    c("misc:>3>", &["a", "b", "c", "d"], 1, 1, ">3>"),
    c("misc:g~3~", &["a", "b", "c", "d"], 1, 1, "g~3~"),
    c("misc:gu2u", &["A", "B", "C"], 1, 1, "gu2u"),
    c("misc:gU2U", &["a", "b", "c"], 1, 1, "gU2U"),
    c("misc:gUgU", &["a b"], 1, 1, "gUgU"),
    c("misc:gugu", &["A B"], 1, 1, "gugu"),
    c("misc:g~g~", &["aB"], 1, 1, "g~g~"),
    c("misc:g?g?", &["ab"], 1, 1, "g?g?"),
    c(
        "misc:gqgq",
        &["one two three four five six"],
        1,
        1,
        ":set tw=10<CR>gqgq",
    ),
    c(
        "misc:gwgw",
        &["one two three four five six"],
        1,
        8,
        ":set tw=10<CR>gwgw",
    ),
    c(
        "misc:d then d with register",
        &["a", "b"],
        1,
        1,
        "\"add\"ap",
    ),
    c("misc:ZZ skip", &["a"], 1, 1, "l"),
    c(
        "misc:. after :normal",
        &["ab", "cd"],
        1,
        1,
        ":normal x<CR>j.",
    ),
    c(
        "misc:. after :g normal",
        &["ab", "cd", "ef"],
        1,
        1,
        ":1,2g/./normal x<CR>G.",
    ),
    c("misc:xp then . ", &["abcd"], 1, 1, "xp."),
    c(
        "misc:dot after @: ",
        &["a", "b", "c", "d"],
        1,
        1,
        ":d<CR>@:.",
    ),
    c("misc:tilde op setting? tildeop skip", &["ab"], 1, 1, "l"),
    c(
        "misc:count with text object c",
        &["a b c d"],
        1,
        1,
        "3ciwX<Esc>",
    ),
    c(
        "misc:count before and after with textobj",
        &["a b c d e f g"],
        1,
        1,
        "2d2aw",
    ),
    c("misc:count on ) motion", &["A. B. C. D."], 1, 1, "2)"),
    c("misc:d2)", &["A. B. C. D."], 1, 1, "d2)"),
    c("misc:d2}", &["a", "", "b", "", "c"], 1, 1, "d2}"),
    c("misc:y2j P", &["a", "b", "c", "d"], 1, 1, "y2jGP"),
    c("misc:2yy 3p", &["a", "b"], 1, 1, "2yy3p"),
    c("misc:3J then u", &["a", "b", "c", "d"], 1, 1, "3Ju"),
    c("misc:count 2 on ~", &["ab"], 1, 1, "2~"),
    c("misc:2r", &["abc"], 1, 1, "2rx"),
    c("misc:5J on 3 lines", &["a", "b", "c"], 1, 1, "5J"),
    c("misc:x on last char then p", &["ab"], 1, 2, "xP"),
    c("misc:dd on last then P", &["a", "b"], 2, 1, "ddP"),
    c("misc:2dd on last", &["a", "b"], 2, 1, "2dd"),
    c(
        "misc:cc on last line indent",
        &["a", "  b"],
        2,
        3,
        "ccX<Esc>",
    ),
    c("misc:cc with count beyond", &["a", "b"], 1, 1, "5ccX<Esc>"),
    c("misc:5>>", &["a", "b"], 1, 1, "5>>"),
    c("misc:5J last", &["a", "b"], 2, 1, "5J"),
    c("misc:d then count then motion 0", &["abc def"], 1, 5, "d0"),
    c("misc:count then i on line start", &["ab"], 1, 1, "2Ix<Esc>"),
    c("misc:count then o with indent", &["  a"], 1, 1, "2ox<Esc>"),
];

// ---------------------------------------------------------------------------
// Categories — the runner flattens these; the split is for editability only.
// ---------------------------------------------------------------------------

const CATEGORIES: &[(&str, &[Case])] = &[
    ("op      operators x motions", CASES_OP),
    ("dot     dot repeat", CASES_DOT),
    ("undo    undo/redo", CASES_UNDO),
    ("reg     registers", CASES_REG),
    ("mac     macros", CASES_MAC),
    ("mark    marks & jumps", CASES_MARK),
    ("search  search", CASES_SEARCH),
    ("ex      :s / :g / ex", CASES_EX),
    ("ins     insert-mode keys", CASES_INS),
    ("vis     visual", CASES_VIS),
    ("vb      visual block", CASES_VB),
    ("num     <C-a> / <C-x>", CASES_NUM),
    ("scroll  scrolling", CASES_SCROLL),
    ("word    word & misc motions", CASES_WORD),
    ("to      text objects", CASES_TO),
    ("misc    misc", CASES_MISC),
];

// ---------------------------------------------------------------------------
// KNOWN_DEVIATIONS — see the module docs. This list may only ever SHRINK.
//
// Deleting an entry is how a Vim-compat fix proves itself: the runner fails if
// a listed label starts passing, and fails if an unlisted label fails.
// ---------------------------------------------------------------------------

const KNOWN_DEVIATIONS: &[&str] = &[
    "op:2cc",
    "op:5dd from last line",
    "op:J after period (vim joinspaces)",
    "op:J next starts with )",
    "op:J next blank",
    "op:J current ends with space",
    "op:>> cursor sol",
    "op:<< mixed tab space",
    "op:o esc removes indent",
    "op:cw on empty line",
    "op:dvj charwise force",
    "op:dve exclusive force",
    "op:dv$",
    // Remaining `dot:` deviations: each fails for a reason outside `.` itself
    // -- linewise-`p` cursor placement (see "op:p linewise cursor first
    // nonblank"), past-eol cursor clamping on entering insert, and `2>>`
    // not aborting when the count exceeds the lines available.
    "dot:i<C-w> .",
    "dot:>> 2.",
    "undo:xxxx 3u",
    "undo:U",
    "undo:UU",
    "undo:A xyz u cursor",
    "undo:u after :%s cursor",
    "undo:u after visual d",
    "undo:u restores cursor after :g",
    "undo:2u after insert ×3",
    "reg:\": last cmd",
    "mac:\"ay then @a executes text",
    "mac:q register letter uppercase Q",
    "mark:`] after yank",
    "jump:g; g; g,",
    "jump:g; after 2 changes same line",
    "search:/\\(foo\\)\\1",
    "search:gd",
    "search:gN",
    "ex:pu!",
    "ex:2put",
    "ex:0put",
    "ex:2,3sort",
    "ex:sort /pat/ r",
    "ex:retab",
    "ex:retab!",
    "ex:retab 2",
    "ex:2ka 'a",
    "ex:2mark a",
    "ex:le",
    "ex:le 4",
    "ex:ri 10",
    "ex:ce 10",
    "ex:r !echo",
    "ex:*d after visual",
    // "ins:BS over indent (nosmarttab)" and "ins:Tab at start (nosmarttab)"
    // moved to HARNESS_LIMITED (#875) — the harness gap they were pinned to,
    // not vimcode's Vim-compat, is the reason they fail. See that array.
    "vis:vip then ip extends",
    "vis:v'a? mark d",
    "vis:vjc then u",
    "vis:v_r CR",
    "vis:v ap trailing",
    "vis:vip on last para no trailing",
    // "num:octal nf=octal 007" and "num:alpha" moved to HARNESS_LIMITED
    // (#875) — same `setup`-is-dropped harness gap as the `nosmarttab` pair
    // above. See that array. (Note "num:octal not default 007" — no
    // `setup`, plain Neovim defaults — is unaffected and still passes as
    // `008`; only the two `setup`-dependent cases are excused.)
    // ── #805: headless-oracle scroll artifacts ──────────────────────────
    //
    // The `scroll:*` entries from here down to "word:gg indented (sol)" are
    // NOT vimcode bugs. They share one proximate cause — `nvim --headless -l
    // script.lua` never attaches a UI, so no redraw ever runs and the
    // window's scroll bookkeeping (`w_topline` / `w_botline` /
    // `w_empty_rows`) is never validated between the keystrokes of a single
    // `nvim_feedkeys()` burst — but it surfaces in **two distinguishable
    // ways**, and they are listed as two separate groups below because the
    // first version of this comment (see #805 review) described the second
    // group wrongly.
    //
    // "Group A" — window-relative *reads* right after any cursor move
    // (`H`/`M`/`L`/`<C-b>`/`zz`/etc.) — was the other half of this and is
    // fully excused; its sole surviving case, "scroll:2<C-b>", moved to
    // HARNESS_LIMITED (#875) along with the measurements backing it. See
    // that array.
    //
    // ── Group B: the 2nd and later scroll command in one burst ──
    //
    // A *single* `<C-d>`/`<C-u>`/`<C-f>` conforms, and that is not luck:
    // these commands move the cursor by exactly as much as they scroll the
    // window, so a wrong topline cancels out of the cursor result. Hence
    // `scroll:C-d`, `scroll:C-u`, `scroll:C-f`, `scroll:3<C-f>`,
    // `scroll:C-d near end`, `scroll:3C-d sets scroll then C-u` etc. all
    // pass and are *not* listed.
    //
    // What does not survive is the **second and subsequent** such command in
    // the same `feedkeys()` burst: nvim's `halfpage()`/`onepage()` advance
    // `w_botline` incrementally from the `w_empty_rows` left over by the
    // previous command, and with no redraw in between nothing ever
    // re-validates that, so the scroll loop terminates against stale state.
    // Measured (60-line buffer, 22-row window, `'scroll'` = 11, start line
    // 1; final cursor line):
    //
    //     keys                headless   interactive   vimcode
    //     <C-d>                     12            12        12   (passes)
    //     <C-d><C-d>                22            23        23
    //     5<C-d><C-d>               10            11        11
    //     <C-d><C-d><C-u>           11            12        12
    //     <C-f>                     21            21        21   (passes)
    //     <C-f><C-f>                19            41        41
    //
    // Note `<C-f><C-f>` lands *above* where a single `<C-f>` lands in the
    // oracle — this is corrupted harness state, not an off-by-one in
    // vimcode. Direct engine coverage pinning vimcode to the interactive
    // column for exactly these four sequences lives in
    // `tests/new_vim_features.rs` (`test_ctrl_d_chain_*`,
    // `test_ctrl_f_chain_*`).
    //
    // RETRACTED: an earlier revision of this comment claimed "`<C-d>`/`<C-u>`
    // are the outliers that DO force a real topline update". That is wrong.
    // It explains why an *isolated* `<C-d>` conforms, but it predicts that a
    // pure `<C-d>` chain would conform too, and the table above shows it
    // does not.
    //
    // `page_up`/`page_down`/`scroll_cursor_center` in
    // `src/core/engine/motions.rs` carry the #805 fixes that *were* real
    // (the 2-line buffer-start/end no-op guards, the post-clamp cursor
    // formula for `<C-b>`, and the `zz`/`z.` centering off-by-one), each
    // with a source comment pointing back here.
    //
    // Group A and Group B are a partial miss against #805's literal
    // acceptance bar ("every label above deleted from KNOWN_DEVIATIONS and
    // passing") — see the #805 PR discussion; the remainder needs either a
    // non-headless oracle for window-relative state or explicit sign-off to
    // close them as a tracked harness limitation.
    //
    // ── Not a harness artifact: 'startofline' ──
    //
    // Separate, genuine root cause: vimcode doesn't implement Vim's
    // 'startofline' option at all, so `<C-d>` never moves the cursor to the
    // first non-blank column the way this case's `vim.o.startofline=true`
    // setup expects — confirmed against real interactive Neovim too. Out of
    // scope for #805; file a follow-up if 'startofline' support is wanted.
    "scroll:C-d col sol",
    "word:gg indented (sol)",
    "word:G indented (sol)",
    "word:5G then j col (sol)",
    "to:das last sentence",
    "to:dis on whitespace between",
    "to:d5aw too many",
    "to:cip",
    "to:daw on only whitespace line",
    "misc:gp charwise multi",
    "misc:cw on space at eol",
    "misc:S then P",
    "misc:: with count",
    "misc:3:s",
    "misc:count then : then range",
    "misc:c3c",
    "misc:2dd on last",
    "misc:cc with count beyond",
];

// ---------------------------------------------------------------------------
// HARNESS_LIMITED (#875) — cases this harness cannot faithfully probe.
//
// Distinct from KNOWN_DEVIATIONS: an entry here is not a claim that vimcode
// differs from Vim. It is a claim that *this test* cannot tell — the failure
// traces to a gap in the harness (`run_in_vimcode` / `run_in_neovim`) or to
// the oracle process itself, not to `Engine`. Counting these against the
// "how far from Neovim is vimcode" number would be dishonest bookkeeping, so
// the runner reports them separately and excludes them from the
// KNOWN_DEVIATIONS bidirectional gate entirely — they can fail forever
// without being a regression, and pass without being "a fix landed" that
// forces an entry deletion.
//
// This array must only ever change for one of two reasons: the harness gap
// it names gets closed (fix the harness, delete the entry, and the case
// rejoins the ordinary pass/regress accounting), or a case gets removed from
// the corpus (delete the stale entry). Like KNOWN_DEVIATIONS, never add an
// entry to paper over a real regression.
// ---------------------------------------------------------------------------

const HARNESS_LIMITED: &[&str] = &[
    // #875: `run_in_vimcode` never reads a case's Lua `setup` — only
    // `run_in_neovim` (the oracle side) does. A `cs(..)` case exists
    // specifically to pin a Vim-vs-Neovim option default that differs from
    // vimcode's hardcoded behaviour, so any such case drives the *same*
    // vimcode keys against a Neovim configured differently than vimcode is.
    // It cannot pass by construction, regardless of vimcode's correctness.
    //
    // "ins:BS over indent (nosmarttab)" / "ins:Tab at start (nosmarttab)":
    // vimcode has no 'smarttab' setting; it always behaves as Vim's actual
    // default (smarttab **on**), matching the "(nvim smarttab)" sibling case
    // and never "(nosmarttab)". Implementing a real 'smarttab' toggle nobody
    // has asked for is not worth doing just to chase this pair to zero — if
    // vimcode ever grows one, wire `setup` through `run_in_vimcode` and move
    // these back to ordinary cases (deleting the entries here).
    "ins:BS over indent (nosmarttab)",
    "ins:Tab at start (nosmarttab)",
    // "num:octal nf=octal 007" / "num:alpha": same shape, for 'nrformats'.
    // VimCode pins Neovim's default, `bin,hex` (see `NrFormats::default()`
    // in `src/core/engine/motions.rs`) — note Vim's own default additionally
    // includes `octal`, which is why "num:octal not default 007" (no
    // `setup`, so plain Neovim defaults) passes as `008` while this one
    // wants the `setup`-pinned octal `010`. Same resolution path as above.
    "num:octal nf=octal 007",
    "num:alpha",
    // #875 (originally #805): "scroll:2<C-b>" is the sole survivor of the
    // #805 headless-scroll-artifact group. `nvim --headless -l script.lua`
    // never attaches a UI, so no redraw ever runs and the window's scroll
    // bookkeeping (`w_topline` / `w_botline` / `w_empty_rows`) is never
    // validated between the keystrokes of one `nvim_feedkeys()` burst.
    // Concretely (60-line buffer, 22-row window, start at line 1) headless
    // nvim's topline silently collapses to *the cursor's own line* after any
    // cursor move, so window-relative reads like `<C-b>` behave as if the
    // window had never scrolled:
    //
    //     keys    headless line('w0')    interactive line('w0')
    //     22j            23  (== cursor)          2
    //     G              60  (== cursor)         39
    //     50%            30  (== cursor)          9
    //
    // Measured, not assumed: `scripts/nvim_headless_vs_interactive_repro.sh`
    // runs the same buffer + cursor + keys through headless nvim (exactly as
    // `run_in_neovim` above does) and through a *real* interactive nvim in a
    // tmux pane with an 80x24 terminal attached, so the window genuinely
    // redraws. Both sides report the same window height (22) and the same
    // `'scroll'` (11), so the comparison is apples-to-apples. vimcode's
    // value matches the **interactive** column and never the headless one —
    // "fixing" this would mean deliberately breaking vimcode's real,
    // correctly-tracked scroll position to imitate a broken oracle. Closing
    // it for real needs a non-headless oracle for window-relative state,
    // which is a much bigger piece of work than this issue.
    "scroll:2<C-b>",
];

// ---------------------------------------------------------------------------
// Oracle version (#865, #872) — deliberately adjacent to KNOWN_DEVIATIONS
// above, because a deviation list is only meaningful against the oracle that
// produced it. If you regenerate the list, move `DEVIATIONS_ORACLE` in the
// same commit.
// ---------------------------------------------------------------------------

/// The `(major, minor)` Neovim that [`KNOWN_DEVIATIONS`] was last regenerated
/// against — the fleet-standard Homebrew/upstream `v0.12.5` that every agent
/// host and both CI jobs run (#872; `ubuntu-24.04` apt's 0.9.5 is below
/// [`MIN_NVIM_VERSION`] and was replaced as CI's oracle by #865). See the
/// "Oracle version skew" section of the module docs: a run against anything
/// else prints a loud banner, and the "a listed label now passes" direction of
/// the gate is downgraded to an advisory (see [`fixes_are_enforced`]).
const DEVIATIONS_ORACLE: (u32, u32) = (0, 12);

/// The minimum `(major, minor)` Neovim this suite will accept as an oracle
/// (#865). The fleet standard — every agent host, and both CI jobs — is
/// Homebrew's / upstream's current stable `v0.12.5`; `0.12` is the floor that
/// pins. A host below it fails loudly rather than quietly producing verdicts
/// from a different Vim.
const MIN_NVIM_VERSION: (u32, u32) = (0, 12);

/// Opt out of the whole suite on a machine that cannot supply a usable oracle
/// (#865). Deliberately an explicit, greppable env var rather than an implicit
/// "`CI` is unset" skip: skipping must be a visible act on the command line,
/// not the default a coordinator Test leg silently inherits.
const ALLOW_SKIP_VAR: &str = "NVIM_CONFORMANCE_ALLOW_SKIP";

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

enum Outcome {
    Pass,
    Fail(String),
    NvimBroke,
}

fn run_case(case: &Case) -> Outcome {
    let nvim = match run_in_neovim(
        case.lines,
        case.cursor_line,
        case.cursor_col,
        case.keys,
        case.setup,
    ) {
        Some(r) => r,
        None => return Outcome::NvimBroke,
    };
    let (vc_buf, vc_line, vc_col) = run_in_vimcode(
        case.lines,
        case.cursor_line,
        case.cursor_col,
        case.keys,
        nvim.rows,
    );
    let nvim_buf = nvim.buf.join("\n");
    let buf_match = vc_buf.trim_end_matches('\n') == nvim_buf.trim_end_matches('\n');
    // Neovim counts a trailing newline as an extra empty line; ropey does not.
    // Allow the VimCode cursor to be one line earlier in exactly that case.
    let cursor_match = (vc_line == nvim.line && vc_col == nvim.col)
        || (vc_buf.ends_with('\n')
            && nvim.buf.last().map(|s| s.is_empty()).unwrap_or(false)
            && vc_line + 1 == nvim.line
            && vc_col == nvim.col);
    if buf_match && cursor_match {
        return Outcome::Pass;
    }
    let what = match (buf_match, cursor_match) {
        (false, false) => "BUF+CUR",
        (false, true) => "BUF",
        _ => "CUR",
    };
    Outcome::Fail(format!(
        "{} [{}] keys={:?} start={:?}@({},{})\n  buffer: nvim={:?} vimcode={:?}\n  cursor: nvim=({},{}) vimcode=({},{})",
        what,
        case.label,
        case.keys,
        case.lines,
        case.cursor_line,
        case.cursor_col,
        nvim_buf,
        vc_buf.trim_end_matches('\n'),
        nvim.line,
        nvim.col,
        vc_line,
        vc_col
    ))
}

// ---------------------------------------------------------------------------
// The bidirectional KNOWN_DEVIATIONS gate, extracted as a pure function so the
// gate itself can be tested without an nvim on PATH — see
// `known_deviation_gate_is_bidirectional` at the bottom of this file. A gate
// that has never been observed to fail is not a gate (#553).
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Eq)]
struct Verdict<'a> {
    /// Failing labels that are *not* in the deviation list — regressions.
    regressions: Vec<&'a str>,
    /// Passing labels that *are* in the deviation list — a fix landed and the
    /// entry must be deleted, so the list can only shrink.
    fixed: Vec<&'a str>,
    /// Deviation-list entries matching no case label at all — stale.
    stale: Vec<&'a str>,
}

impl Verdict<'_> {
    fn is_clean(&self) -> bool {
        self.regressions.is_empty() && self.fixed.is_empty() && self.stale.is_empty()
    }
}

/// `outcomes` is `(label, passed)` for every case that actually ran.
/// `all_labels` is the full corpus, used only for the stale-entry check; pass
/// `None` when the run was filtered, since most labels legitimately didn't run.
fn classify<'a>(
    outcomes: &[(&'a str, bool)],
    known: &[&'a str],
    all_labels: Option<&[&'a str]>,
) -> Verdict<'a> {
    let known_set: std::collections::HashSet<&str> = known.iter().copied().collect();
    let mut verdict = Verdict::default();
    for (label, passed) in outcomes {
        match (known_set.contains(label), passed) {
            (false, false) => verdict.regressions.push(label),
            (true, true) => verdict.fixed.push(label),
            _ => {}
        }
    }
    if let Some(all) = all_labels {
        let all_set: std::collections::HashSet<&str> = all.iter().copied().collect();
        verdict.stale = known
            .iter()
            .copied()
            .filter(|l| !all_set.contains(l))
            .collect();
    }
    verdict
}

/// Parse `(major, minor)` out of `nvim --version`'s first line, which looks like
/// `NVIM v0.12.5` (or `NVIM v0.9.5` / `NVIM v0.11.0-dev+1234-gabcdef`).
/// `None` when the line isn't in that shape — an unknown version is treated as
/// "assume it matches", so a parse failure can only ever make the gate stricter.
fn parse_nvim_version(version_output: &str) -> Option<(u32, u32)> {
    let first = version_output.lines().next()?;
    let v = first.split_whitespace().nth(1)?.strip_prefix('v')?;
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor_field = parts.next()?;
    // `0.11.0-dev+123` → the dash belongs to the patch field, but be tolerant.
    let minor = minor_field.split(['-', '+']).next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

/// Is the "a listed label now PASSES" direction of the gate enforcing?
///
/// Yes when the running Neovim is the one [`KNOWN_DEVIATIONS`] was captured
/// against, or when its version could not be determined (failing closed, so a
/// `nvim --version` format change cannot silently disable the gate). Otherwise
/// the oracle legitimately disagrees on some labels (#868) and deleting them
/// would hand the capture oracle the same count back as false regressions, so
/// the direction is downgraded to a printed advisory.
///
/// #865 removed the former `in_ci ||` short-circuit: it was correct only while
/// CI *was* the capture oracle. CI now runs the pinned fleet oracle (v0.12.5)
/// and, as of #872, so does [`DEVIATIONS_ORACLE`] — the enforcing lane is
/// "whichever lane runs [`DEVIATIONS_ORACLE`]", which is every lane again.
///
/// Note this only ever relaxes the *fixed* direction. Regressions and stale
/// entries stay fatal on every machine, CI included.
fn fixes_are_enforced(nvim: Option<(u32, u32)>) -> bool {
    nvim.is_none_or(|v| v == DEVIATIONS_ORACLE)
}

fn bullet_list(labels: &[&str]) -> String {
    labels
        .iter()
        .map(|l| format!("    {l:?},"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// First line of `nvim --version` (`NVIM v0.12.5`), trimmed — what the runner
/// echoes so a log names the exact oracle it used.
fn version_banner_line(version_output: &str) -> &str {
    version_output
        .lines()
        .next()
        .unwrap_or("(no output)")
        .trim()
}

/// Resolve `nvim` against `PATH` the way the OS would, for *reporting* only —
/// [`std::process::Command`] does its own lookup, but it will not tell us which
/// binary it picked, and "which nvim did this verdict come from" is exactly the
/// question #865 exists to answer. Platform-neutral: no `which`/`where` shell-out.
fn resolve_on_path(exe: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    // Windows carries the extension; trying both on every platform is harmless
    // because the miss simply doesn't exist on disk.
    let candidates = [exe.to_string(), format!("{exe}.exe")];
    std::env::split_paths(&path).find_map(|dir| {
        candidates
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file() && is_executable(p))
    })
}

/// A regular file ahead of the real oracle on `PATH` but lacking the
/// executable bit would never actually be picked by the real `execvp`-style
/// lookup [`std::process::Command`] performs — only `is_file()` would still
/// name it as "resolved", which is misleading in a reporting-only helper
/// whose entire job is "which binary produced this verdict" (review finding
/// on #865). Windows has no executable bit in this sense; `.exe`/no-extension
/// matching in [`resolve_on_path`] is already the whole story there.
#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_p: &std::path::Path) -> bool {
    true
}

/// Write `s` straight to the real OS-level stderr, bypassing libtest's
/// stdout/stderr capture.
///
/// libtest intercepts `print!`/`eprintln!` via a thread-local sink
/// (`io::set_output_capture`) and *discards* it for a passing test unless
/// `--nocapture`/`--show-output` was passed — see the rustc source for
/// `libtest::helpers::concurrency` capture handling. Neither of CI's `Run
/// tests` steps nor the coordinator's plain `cargo test` Test leg passes
/// either flag, and `nvim_conformance` passes on a healthy run — exactly the
/// common case this banner exists to narrate (#865 review finding: a
/// `println!` banner is invisible on precisely the lanes it was built for).
/// Opening `/dev/stderr` writes to fd 2 directly, underneath that capture, so
/// the banner reaches the log on a plain `cargo test` too.
#[cfg(unix)]
fn print_unmissable(s: &str) {
    use std::io::Write as _;
    match std::fs::OpenOptions::new().write(true).open("/dev/stderr") {
        Ok(mut f) => {
            let _ = f.write_all(s.as_bytes());
            let _ = f.flush();
        }
        // No /dev/stderr (sandboxed or unusual environment) — fall back to
        // the macro. Still correct on a failing run; better than nothing on
        // a passing one.
        Err(_) => eprintln!("{s}"),
    }
}

#[cfg(not(unix))]
fn print_unmissable(s: &str) {
    eprintln!("{s}");
}

// ---------------------------------------------------------------------------
// Preflight (#865) — decide what to do about the oracle BEFORE running 1,436
// cases. Pure, so the decision (and the exact wording of every message) is
// testable on a machine with no nvim at all; `nvim_conformance` only probes
// the environment and then does what this says.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Preflight {
    /// Oracle is usable. `banner` names the resolved path and version, and
    /// carries the skew warning when it is not [`DEVIATIONS_ORACLE`].
    Run { banner: String, version: (u32, u32) },
    /// Oracle is unusable but [`ALLOW_SKIP_VAR`] was set — skip deliberately.
    Skip { reason: String },
    /// Oracle is unusable and no opt-out was given — fail the run.
    Refuse { reason: String },
}

/// `probe` is `None` when `nvim` could not be run at all, otherwise the
/// resolved path (for the message) and the raw `nvim --version` output.
fn preflight(probe: Option<(&str, &str)>, allow_skip: bool) -> Preflight {
    let required = format!("{}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1);
    let refuse = |reason: String| {
        if allow_skip {
            Preflight::Skip { reason }
        } else {
            Preflight::Refuse { reason }
        }
    };

    let Some((path, version_output)) = probe else {
        return refuse(format!(
            "nvim not found on PATH (or `nvim --version` failed).\n\n\
             tests/nvim_conformance.rs is this repo's only oracle-backed \
             Vim-behaviour suite — a missing `nvim` is NOT a pass, it is 1,436 \
             cases that did not run. Install Neovim >= {required} (the fleet \
             standard is v0.12.5) and make sure the `nvim` binary is on the PATH \
             this process inherits.\n\n\
             To skip this suite deliberately instead, set {ALLOW_SKIP_VAR}=1."
        ));
    };

    let banner_line = version_banner_line(version_output);
    let Some(version) = parse_nvim_version(version_output) else {
        return refuse(format!(
            "could not parse a version out of `nvim --version` for the oracle at \
             {path}.\n  first line: {banner_line:?}\n  required:   >= {required}\n\n\
             Refusing to run the conformance corpus against an oracle of unknown \
             vintage — KNOWN_DEVIATIONS is only meaningful against a known \
             version. Set {ALLOW_SKIP_VAR}=1 to skip this suite instead."
        ));
    };

    if version < MIN_NVIM_VERSION {
        return refuse(format!(
            "oracle Neovim is too old.\n  path:     {path}\n  found:    {banner_line} \
             (parsed {}.{}.x)\n  required: >= {required}\n\n\
             Neovim's own option defaults and headless behaviour move between \
             versions, so a verdict from {}.{}.x is not comparable with the fleet's \
             (v0.12.5) — see the \"Oracle version skew\" section of the module docs. \
             Upgrade, or set {ALLOW_SKIP_VAR}=1 to skip this suite.",
            version.0, version.1, version.0, version.1
        ));
    }

    let mut banner = format!(
        "\nnvim conformance oracle: {path}\n  version: {banner_line}\n  \
         KNOWN_DEVIATIONS captured against: {}.{}.x\n",
        DEVIATIONS_ORACLE.0, DEVIATIONS_ORACLE.1
    );
    if version != DEVIATIONS_ORACLE {
        banner.push_str(&format!(
            "\n\
             !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n\
             !! ORACLE VERSION SKEW: running {}.{}.x, but KNOWN_DEVIATIONS was\n\
             !! last regenerated against {}.{}.x. Verdicts that move are far more\n\
             !! likely to be the oracle having changed than vimcode. The \"a listed\n\
             !! label now passes\" direction is therefore ADVISORY on this run —\n\
             !! do NOT delete entries to make it green. Regenerate the list and\n\
             !! bump DEVIATIONS_ORACLE in one deliberate commit instead.\n\
             !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n",
            version.0, version.1, DEVIATIONS_ORACLE.0, DEVIATIONS_ORACLE.1
        ));
    }

    Preflight::Run { banner, version }
}

#[test]
fn nvim_conformance() {
    let version_output = std::process::Command::new("nvim")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let resolved = resolve_on_path("nvim");
    let resolved_display = resolved
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "nvim (resolved by PATH lookup)".to_string());
    let probe = version_output
        .as_deref()
        .map(|out| (resolved_display.as_str(), out));

    // #865: a missing/too-old oracle must never read as 1,436 passing cases.
    // Failing is the default on every lane — CI, a coordinator Test leg and a
    // developer laptop alike; skipping requires the explicit opt-out.
    let allow_skip = std::env::var_os(ALLOW_SKIP_VAR).is_some();
    let nvim_version = match preflight(probe, allow_skip) {
        Preflight::Refuse { reason } => panic!("\n\n{reason}\n"),
        Preflight::Skip { reason } => {
            eprintln!("SKIP ({ALLOW_SKIP_VAR} set): {reason}");
            return;
        }
        Preflight::Run { banner, version } => {
            // #865 review: println! is captured-and-discarded by libtest on a
            // passing test unless --nocapture/--show-output is passed, which
            // neither CI nor the coordinator's Test leg does — so this must
            // bypass that capture, not just write through it.
            print_unmissable(&banner);
            Some(version)
        }
    };
    let in_ci = std::env::var_os("CI").is_some();

    // Labels are the identity used by KNOWN_DEVIATIONS, so they must be unique.
    {
        let mut seen = std::collections::HashSet::new();
        let dupes: Vec<&str> = CATEGORIES
            .iter()
            .flat_map(|(_, g)| g.iter())
            .filter(|c| !seen.insert(c.label))
            .map(|c| c.label)
            .collect();
        assert!(
            dupes.is_empty(),
            "duplicate conformance case labels (labels key KNOWN_DEVIATIONS, so they must be unique): {dupes:?}"
        );
    }

    // #875: KNOWN_DEVIATIONS and HARNESS_LIMITED are mutually exclusive
    // buckets — a label wired into both would silently take whichever branch
    // the loop below checks first, masking the other array's accounting.
    {
        let known_set: std::collections::HashSet<&str> = KNOWN_DEVIATIONS.iter().copied().collect();
        let both: Vec<&str> = HARNESS_LIMITED
            .iter()
            .copied()
            .filter(|l| known_set.contains(l))
            .collect();
        assert!(
            both.is_empty(),
            "label(s) listed in both KNOWN_DEVIATIONS and HARNESS_LIMITED: {both:?}"
        );
    }

    let filter = std::env::var("PROBE_FILTER").ok();
    let dump_to = std::env::var("CONFORMANCE_DUMP_DEVIATIONS").ok();
    let verbose = std::env::var_os("PROBE_VERBOSE").is_some();

    let selected: Vec<(usize, &Case)> = CATEGORIES
        .iter()
        .enumerate()
        .flat_map(|(ci, (_, g))| g.iter().map(move |c| (ci, c)))
        .filter(|(_, c)| filter.as_deref().is_none_or(|f| c.label.contains(f)))
        .collect();

    // Each case is an independent `nvim` process, so fan them out; 1,400+
    // serial process spawns is minutes of wall clock for no reason.
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16);
    let chunk = selected.len().div_ceil(workers).max(1);
    let mut results: Vec<(usize, &Case, Outcome)> = std::thread::scope(|scope| {
        let handles: Vec<_> = selected
            .chunks(chunk)
            .map(|slice| {
                scope.spawn(move || {
                    slice
                        .iter()
                        .map(|(ci, case)| (*ci, *case, run_case(case)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("conformance worker panicked"))
            .collect()
    });
    results.sort_by_key(|(ci, _, _)| *ci);

    let known: std::collections::HashSet<&str> = KNOWN_DEVIATIONS.iter().copied().collect();
    let harness_limited: std::collections::HashSet<&str> =
        HARNESS_LIMITED.iter().copied().collect();
    let mut totals = vec![(0usize, 0usize, 0usize); CATEGORIES.len()]; // (pass, known-fail, unexpected-fail)
    let mut outcomes: Vec<(&str, bool)> = Vec::new();
    let mut detail: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    let mut deviating: Vec<&str> = Vec::new();
    let mut nvim_broke: Vec<&str> = Vec::new();
    // #875: harness-limited cases are tracked in a wholly separate bucket —
    // (pass, fail) — and never touch `outcomes`/`totals`/`deviating`, so they
    // cannot become a regression, cannot force a KNOWN_DEVIATIONS deletion,
    // and cannot inflate the deviation count. See the HARNESS_LIMITED doc
    // comment for why.
    let mut harness_totals = (0usize, 0usize);
    let mut harness_seen: Vec<&str> = Vec::new();

    for (ci, case, outcome) in &results {
        if harness_limited.contains(case.label) {
            harness_seen.push(case.label);
            match outcome {
                Outcome::NvimBroke => {
                    nvim_broke.push(case.label);
                    if in_ci {
                        harness_totals.1 += 1;
                    }
                }
                Outcome::Pass => harness_totals.0 += 1,
                Outcome::Fail(msg) => {
                    harness_totals.1 += 1;
                    if verbose {
                        println!("HARNESS-LIMITED-FAIL {msg}");
                    }
                }
            }
            continue;
        }
        let listed = known.contains(case.label);
        match outcome {
            Outcome::NvimBroke => {
                nvim_broke.push(case.label);
                // A broken oracle is not a deviation — it's a broken harness,
                // so it is never excusable via KNOWN_DEVIATIONS under CI.
                if in_ci {
                    totals[*ci].2 += 1;
                    outcomes.push((case.label, false));
                    detail.insert(
                        case.label,
                        format!(
                            "NVIM-FAIL [{}]: nvim execution failed (treated as failure under CI)",
                            case.label
                        ),
                    );
                }
            }
            Outcome::Pass => {
                totals[*ci].0 += 1;
                outcomes.push((case.label, true));
                if verbose {
                    println!("PASS [{}]", case.label);
                }
            }
            Outcome::Fail(msg) => {
                deviating.push(case.label);
                outcomes.push((case.label, false));
                detail.insert(case.label, format!("FAIL {msg}"));
                if listed {
                    totals[*ci].1 += 1;
                    if verbose {
                        // Print the full diff, not just the label: auditing a
                        // KNOWN_DEVIATIONS entry (is it a real bug or a harness
                        // artifact?) needs the expected-vs-actual values, and
                        // they were previously only reachable by temporarily
                        // deleting the entry to turn it into a "regression".
                        println!("KNOWN-FAIL {msg}");
                    }
                } else {
                    totals[*ci].2 += 1;
                }
            }
        }
    }

    println!("\n=== Neovim Conformance Results ===");
    println!(
        "{:<32} {:>6} {:>6} {:>6} {:>6}",
        "category", "cases", "pass", "known", "FAIL"
    );
    for (ci, (name, _)) in CATEGORIES.iter().enumerate() {
        let (p, k, f) = totals[ci];
        if p + k + f == 0 {
            continue;
        }
        println!("{:<32} {:>6} {:>6} {:>6} {:>6}", name, p + k + f, p, k, f);
    }
    let (tp, tk, tf) = totals
        .iter()
        .fold((0, 0, 0), |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2));
    println!(
        "{:<32} {:>6} {:>6} {:>6} {:>6}",
        "TOTAL",
        tp + tk + tf,
        tp,
        tk,
        tf
    );
    if !nvim_broke.is_empty() {
        println!(
            "\nnvim execution failed for {} case(s): {:?}",
            nvim_broke.len(),
            nvim_broke
        );
    }
    if !harness_seen.is_empty() {
        // #875: reported, but deliberately excluded from every column above
        // and from the deviation gate below — see the HARNESS_LIMITED doc
        // comment for why these don't count as "how far from Neovim".
        println!(
            "\n{} case(s) excluded as HARNESS_LIMITED (not counted as deviations — see doc \
             comment): {} pass, {} fail",
            harness_seen.len(),
            harness_totals.0,
            harness_totals.1
        );
    }

    if let Some(path) = dump_to {
        let mut out = String::new();
        for label in &deviating {
            out.push_str(&format!("    {label:?},\n"));
        }
        std::fs::write(&path, &out).expect("failed to write CONFORMANCE_DUMP_DEVIATIONS file");
        println!(
            "\nCONFORMANCE_DUMP_DEVIATIONS: wrote {} failing label(s) to {path}",
            deviating.len()
        );
        return;
    }

    // Stale entries (labels that no longer exist) would silently mask a
    // regression if the case were ever re-added, so treat them as errors too.
    // Only meaningful on an unfiltered run.
    let all_labels: Vec<&str> = CATEGORIES
        .iter()
        .flat_map(|(_, g)| g.iter())
        .map(|c| c.label)
        .collect();
    let mut verdict = classify(
        &outcomes,
        KNOWN_DEVIATIONS,
        filter.is_none().then_some(all_labels.as_slice()),
    );

    // #875: HARNESS_LIMITED gets the same stale-entry hygiene as
    // KNOWN_DEVIATIONS — a label matching no case would silently exclude
    // nothing and nobody would notice. Only meaningful on an unfiltered run;
    // reuse `verdict.stale` so it goes through the existing reporting path.
    if filter.is_none() {
        let all_set: std::collections::HashSet<&str> = all_labels.iter().copied().collect();
        verdict.stale.extend(
            HARNESS_LIMITED
                .iter()
                .copied()
                .filter(|l| !all_set.contains(l)),
        );
    }

    // #868: on a Neovim that is not the one the list was captured against,
    // "this listed label now passes" is ambiguous — it is far more often the
    // oracle having improved than vimcode having. Deleting the entries to
    // satisfy such a run would hand the capture oracle the same count back as
    // false regressions. Report, don't fail. See the module docs.
    if !verdict.fixed.is_empty() && !fixes_are_enforced(nvim_version) {
        println!(
            "\nNOTE: {} KNOWN_DEVIATIONS entr(y/ies) pass against this run's \
             Neovim {} but the list was captured against {}.{}.x, \
             so this is oracle-version skew, not a landed fix — do NOT delete them \
             (see the module docs). Not failing the run:\n{}",
            verdict.fixed.len(),
            nvim_version
                .map(|(a, b)| format!("{a}.{b}.x"))
                .unwrap_or_else(|| "(unknown)".into()),
            DEVIATIONS_ORACLE.0,
            DEVIATIONS_ORACLE.1,
            bullet_list(&verdict.fixed)
        );
        verdict.fixed.clear();
    }

    if verdict.is_clean() {
        return;
    }

    let mut problems: Vec<String> = Vec::new();
    if !verdict.stale.is_empty() {
        problems.push(format!(
            "{} KNOWN_DEVIATIONS entr(y/ies) match no case label — delete them:\n{}",
            verdict.stale.len(),
            bullet_list(&verdict.stale)
        ));
    }
    if !verdict.regressions.is_empty() {
        problems.push(format!(
            "{} conformance REGRESSION(S) — cases not in KNOWN_DEVIATIONS that do not match Neovim:\n\n{}",
            verdict.regressions.len(),
            verdict
                .regressions
                .iter()
                .map(|l| detail.get(l).cloned().unwrap_or_else(|| (*l).to_string()))
                .collect::<Vec<_>>()
                .join("\n\n")
        ));
    }
    if !verdict.fixed.is_empty() {
        problems.push(format!(
            "{} case(s) listed in KNOWN_DEVIATIONS now PASS. \
             Good — delete these entries from KNOWN_DEVIATIONS so the list keeps shrinking:\n{}",
            verdict.fixed.len(),
            bullet_list(&verdict.fixed)
        ));
    }
    panic!("\n\n{}\n", problems.join("\n\n"));
}

/// The gate itself, exercised without needing nvim: both directions must be
/// able to fail, or `KNOWN_DEVIATIONS` is decoration rather than a gate (#553).
#[test]
fn known_deviation_gate_is_bidirectional() {
    let all = ["a:one", "a:two", "b:three"];
    let known = ["a:two"];

    // Steady state: the listed label fails, the unlisted ones pass → clean.
    let steady = classify(
        &[("a:one", true), ("a:two", false), ("b:three", true)],
        &known,
        Some(&all),
    );
    assert!(steady.is_clean(), "steady state should pass: {steady:?}");

    // Direction 1 — an unlisted label starts failing: regression, must fail.
    let regressed = classify(
        &[("a:one", true), ("a:two", false), ("b:three", false)],
        &known,
        Some(&all),
    );
    assert_eq!(regressed.regressions, vec!["b:three"]);
    assert!(!regressed.is_clean());

    // Direction 2 — a listed label starts passing: the fix must delete its
    // entry, so the run must fail until it does.
    let improved = classify(
        &[("a:one", true), ("a:two", true), ("b:three", true)],
        &known,
        Some(&all),
    );
    assert_eq!(improved.fixed, vec!["a:two"]);
    assert!(!improved.is_clean());

    // A deviation entry naming a case that no longer exists is stale — it would
    // silently excuse the case if it were ever re-added.
    let stale = classify(&[("a:one", true)], &["a:gone"], Some(&all));
    assert_eq!(stale.stale, vec!["a:gone"]);
    assert!(!stale.is_clean());

    // Filtered runs cannot judge staleness (most labels didn't run), so the
    // stale check is skipped rather than firing on every `PROBE_FILTER` run.
    let filtered = classify(&[("a:one", true)], &["a:gone"], None);
    assert!(filtered.stale.is_empty());
    assert!(filtered.is_clean());
}

/// #868/#865: the oracle-version escape hatch must relax the *fixed* direction
/// only, and only on a Neovim that is not the one the list was captured
/// against. Every other case stays enforcing.
#[test]
fn fixed_direction_is_advisory_only_on_a_different_nvim() {
    // The relaxed case: a newer Neovim than the capture oracle (0.12.5, as of
    // #872 — the version 0.12.5 that surfaced #868's 37 `scroll:` "passes" is
    // now the oracle itself, so a still-newer minor is used here instead).
    assert!(!fixes_are_enforced(Some((0, 13))));
    // ...and an *older* one skews just as legitimately.
    assert!(!fixes_are_enforced(Some((0, 8))));

    // Running exactly the Neovim the list was captured against: enforcing, so a
    // genuinely landed fix is still forced to delete its entry.
    assert!(fixes_are_enforced(Some(DEVIATIONS_ORACLE)));

    // An unparseable version is treated as "assume it matches" — failing closed,
    // so a `nvim --version` format change cannot silently disable the gate.
    assert!(fixes_are_enforced(None));
}

// ---------------------------------------------------------------------------
// #865 preflight: a missing or too-old oracle must never read as a pass. These
// assert on the exact text the runner emits (the messages are the product here
// — an operator reading a CI log is the consumer), not on some internal flag.
// ---------------------------------------------------------------------------

/// Acceptance #1: nvim absent, no opt-out → FAIL, and the message names both the
/// missing binary and the opt-out variable so the reader knows the way out.
#[test]
fn missing_nvim_fails_by_default_and_names_the_binary_and_the_opt_out() {
    let Preflight::Refuse { reason } = preflight(None, false) else {
        panic!(
            "a missing nvim with no opt-out must REFUSE, not skip: {:?}",
            preflight(None, false)
        );
    };
    assert!(reason.contains("nvim"), "must name the binary: {reason}");
    assert!(
        reason.contains(ALLOW_SKIP_VAR),
        "must name the opt-out variable: {reason}"
    );
    // The floor is part of "what would make this work".
    assert!(
        reason.contains(&format!("{}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1)),
        "must name the required version: {reason}"
    );
}

/// Acceptance #2: with the opt-out set, a missing nvim skips as it used to.
#[test]
fn missing_nvim_skips_when_the_opt_out_is_set() {
    let Preflight::Skip { reason } = preflight(None, true) else {
        panic!("{ALLOW_SKIP_VAR}=1 with no nvim must SKIP");
    };
    assert!(reason.contains(ALLOW_SKIP_VAR));
}

/// Acceptance #3: an nvim below the floor fails naming path, found and required.
#[test]
fn nvim_below_the_declared_minimum_fails_naming_path_found_and_required() {
    let old = "NVIM v0.9.5\nBuild type: Release\nLuaJIT 2.1.0\n";
    let Preflight::Refuse { reason } = preflight(Some(("/usr/bin/nvim", old)), false) else {
        panic!("an oracle below {MIN_NVIM_VERSION:?} must REFUSE");
    };
    assert!(reason.contains("/usr/bin/nvim"), "resolved path: {reason}");
    assert!(reason.contains("NVIM v0.9.5"), "found version: {reason}");
    assert!(
        reason.contains(&format!(">= {}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1)),
        "required version: {reason}"
    );

    // Exactly at the floor is fine — the check is `<`, not `<=`.
    let floor = format!("NVIM v{}.{}.0\n", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1);
    assert!(matches!(
        preflight(Some(("/usr/bin/nvim", &floor)), false),
        Preflight::Run { .. }
    ));

    // And the opt-out covers this case too, for a host that genuinely can't
    // upgrade — but only when it is asked for explicitly.
    assert!(matches!(
        preflight(Some(("/usr/bin/nvim", old)), true),
        Preflight::Skip { .. }
    ));
}

/// An oracle whose version can't be parsed is of unknown vintage, so it is
/// refused rather than trusted — otherwise renaming the banner would silently
/// re-open the hole #865 closed.
#[test]
fn unparseable_nvim_version_is_refused_not_trusted() {
    let Preflight::Refuse { reason } = preflight(Some(("/opt/nvim", "NVIM vX.Y.Z\n")), false)
    else {
        panic!("an unparseable oracle version must REFUSE");
    };
    assert!(reason.contains("/opt/nvim"));
    assert!(reason.contains("NVIM vX.Y.Z"));
    assert!(reason.contains(ALLOW_SKIP_VAR));
}

/// Acceptance #4: a normal run names the binary it used and its version, and a
/// version other than the capture oracle is loudly (but non-fatally) flagged.
#[test]
fn a_usable_oracle_runs_and_the_banner_names_path_version_and_skew() {
    // A minor above the capture oracle ((0, 12) as of #872) — legitimate skew,
    // not the fleet standard itself.
    let fleet = "NVIM v0.13.5\nBuild type: Release\n";
    let Preflight::Run { banner, version } =
        preflight(Some(("/home/x/.local/bin/nvim", fleet)), false)
    else {
        panic!("a Neovim above the floor must be runnable");
    };
    assert_eq!(version, (0, 13));
    assert!(banner.contains("/home/x/.local/bin/nvim"), "path: {banner}");
    assert!(banner.contains("NVIM v0.13.5"), "version: {banner}");
    // 0.13.5 is not the capture oracle, so the skew banner must be there and
    // must name both versions.
    assert!(
        banner.contains("ORACLE VERSION SKEW"),
        "skew banner: {banner}"
    );
    assert!(
        banner.contains("0.13.x") && banner.contains("0.12.x"),
        "{banner}"
    );

    // Now the capture oracle itself. As of #872 it sits exactly at the floor
    // (both are 0.12), so the branch below always takes the "runnable, no
    // skew" arm today — kept as an if/else rather than collapsed so a future
    // DEVIATIONS_ORACLE bump that again falls below MIN_NVIM_VERSION is still
    // caught by this same assertion instead of silently going untested.
    let capture = format!("NVIM v{}.{}.5\n", DEVIATIONS_ORACLE.0, DEVIATIONS_ORACLE.1);
    let verdict = preflight(Some(("/usr/bin/nvim", &capture)), false);
    if DEVIATIONS_ORACLE < MIN_NVIM_VERSION {
        assert!(
            matches!(verdict, Preflight::Refuse { .. }),
            "a capture oracle below MIN_NVIM_VERSION must still be refused: {verdict:?}"
        );
    } else {
        let Preflight::Run { banner, .. } = verdict else {
            panic!("the capture oracle must be runnable once it meets the floor");
        };
        // Running exactly what the list was captured against: named, but no shout.
        assert!(banner.contains("/usr/bin/nvim"));
        assert!(!banner.contains("ORACLE VERSION SKEW"), "{banner}");
    }
}

/// The same three acceptance cases, end-to-end on the *real* `nvim_conformance`
/// test rather than on `preflight` alone: re-invoke this very test binary with a
/// PATH that contains no `nvim` (or a deliberately ancient fake one) and assert
/// on the exit status and the operator-visible output. A gate that has only ever
/// been exercised through a helper is not known to be wired up (#553).
///
/// `cfg(unix)`: the fake-oracle arm writes a `#!/bin/sh` stub. The `preflight`
/// tests above carry the same coverage on every platform.
#[cfg(unix)]
#[test]
fn nvim_conformance_end_to_end_refuses_a_missing_or_ancient_oracle() {
    use std::os::unix::fs::PermissionsExt;

    // A directory that is the entire PATH of the child process.
    let dir = std::env::temp_dir().join(format!(
        "vimcode-nvim-oracle-gate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp PATH dir");

    let run_child = |allow_skip: bool| -> (bool, String) {
        let exe = std::env::current_exe().expect("test binary path");
        let mut cmd = std::process::Command::new(exe);
        cmd.args(["nvim_conformance", "--exact", "--nocapture"])
            .env("PATH", &dir)
            .env_remove("PROBE_FILTER")
            .env_remove("PROBE_VERBOSE")
            .env_remove("CONFORMANCE_DUMP_DEVIATIONS")
            .env_remove(ALLOW_SKIP_VAR);
        if allow_skip {
            cmd.env(ALLOW_SKIP_VAR, "1");
        }
        let out = cmd.output().expect("re-invoke the test binary");
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.success(), text)
    };

    // 1. No nvim anywhere on PATH, no opt-out → the test FAILS (before #865 it
    //    printed "SKIP" and reported `ok`).
    let (ok, output) = run_child(false);
    assert!(
        !ok,
        "a missing nvim must fail the suite, not pass it:\n{output}"
    );
    assert!(output.contains("nvim not found on PATH"), "{output}");
    assert!(output.contains(ALLOW_SKIP_VAR), "{output}");

    // 2. Same, with the opt-out set → skips, and the run is green.
    let (ok, output) = run_child(true);
    assert!(ok, "{ALLOW_SKIP_VAR}=1 must skip cleanly:\n{output}");
    assert!(output.contains("SKIP"), "{output}");

    // 3. An nvim that is present but below the floor → FAILS, naming the
    //    resolved path, the version it found and the one it needs.
    let fake = dir.join("nvim");
    std::fs::write(
        &fake,
        "#!/bin/sh\necho 'NVIM v0.9.5'\necho 'Build type: Release'\n",
    )
    .expect("write fake oracle");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake oracle");

    let (ok, output) = run_child(false);
    std::fs::remove_dir_all(&dir).ok();
    assert!(!ok, "an oracle below the floor must fail:\n{output}");
    assert!(
        output.contains(&fake.display().to_string()),
        "must name the resolved path:\n{output}"
    );
    assert!(
        output.contains("NVIM v0.9.5"),
        "must name what it found:\n{output}"
    );
    assert!(
        output.contains(&format!(">= {}.{}", MIN_NVIM_VERSION.0, MIN_NVIM_VERSION.1)),
        "must name what it requires:\n{output}"
    );
}

/// #865 review finding: every other assertion on the banner (path, version,
/// skew warning) either calls `preflight()` directly — which never goes
/// through libtest at all — or re-invokes the binary with `--nocapture`,
/// which CI and the coordinator's Test leg never pass. Neither proves the
/// banner is visible on the run that actually ships: a PASSING run of a
/// plain `cargo test` (no flags). Prove that here: re-invoke the real
/// `nvim_conformance` test against a fake-but-healthy oracle running a minor
/// above `DEVIATIONS_ORACLE` ((0, 12) as of #872) so the skew banner fires,
/// filtered to match zero cases (fast, and doesn't need a real `nvim
/// --headless`), *without* `--nocapture`, and confirm the banner — including
/// the version-skew warning — reached the child's real stdout/stderr anyway.
#[cfg(unix)]
#[test]
fn nvim_conformance_end_to_end_banner_visible_on_a_passing_uncaptured_run() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!(
        "vimcode-nvim-oracle-banner-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp PATH dir");
    let fake = dir.join("nvim");
    std::fs::write(
        &fake,
        "#!/bin/sh\necho 'NVIM v0.13.5'\necho 'Build type: Release'\n",
    )
    .expect("write fake oracle");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fake oracle");

    let exe = std::env::current_exe().expect("test binary path");
    let out = std::process::Command::new(exe)
        .args(["nvim_conformance", "--exact"]) // deliberately no --nocapture
        .env("PATH", &dir)
        // Matches no case label, so zero cases actually run — the fake
        // oracle only needs to answer `--version`, and the run stays fast.
        .env("PROBE_FILTER", "zzz-no-such-conformance-case-zzz")
        .env_remove("PROBE_VERBOSE")
        .env_remove("CONFORMANCE_DUMP_DEVIATIONS")
        .env_remove(ALLOW_SKIP_VAR)
        .output()
        .expect("re-invoke the test binary");
    std::fs::remove_dir_all(&dir).ok();

    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));

    assert!(
        out.status.success(),
        "a healthy oracle with no matching cases must pass:\n{text}"
    );
    assert!(
        text.contains(&fake.display().to_string()),
        "the resolved path must reach a plain, uncaptured `cargo test` run \
         even though the test PASSED (#865 review finding — libtest discards \
         println! output for passing tests unless --nocapture/--show-output \
         is passed, and neither CI nor the coordinator's Test leg passes \
         them):\n{text}"
    );
    assert!(text.contains("NVIM v0.13.5"), "{text}");
    assert!(
        text.contains("ORACLE VERSION SKEW"),
        "the skew warning must be visible on a passing, uncaptured run too:\n{text}"
    );
}

#[test]
fn nvim_version_parses_release_and_dev_banners() {
    // The two that matter: CI's apt build, and the local build that hit #868.
    assert_eq!(
        parse_nvim_version("NVIM v0.9.5\nBuild type: Release\n"),
        Some((0, 9))
    );
    assert_eq!(
        parse_nvim_version("NVIM v0.12.5\nBuild type: Release\nLuaJIT 2.1\n"),
        Some((0, 12))
    );
    // Nightly/dev banners carry a suffix on the patch field; the minor is still
    // the thing we key on, and `0.11` must not be mistaken for `0.1`.
    assert_eq!(
        parse_nvim_version("NVIM v0.11.0-dev+1234-gabcdef\n"),
        Some((0, 11))
    );
    // Anything not in that shape is "unknown", which `fixes_are_enforced`
    // deliberately treats as enforcing rather than as a free pass.
    assert_eq!(parse_nvim_version(""), None);
    assert_eq!(parse_nvim_version("NVIM\n"), None);
    assert_eq!(parse_nvim_version("some other tool 1.2.3\n"), None);
    assert_eq!(parse_nvim_version("NVIM vX.Y.Z\n"), None);
}
