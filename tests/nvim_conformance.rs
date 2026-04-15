//! Neovim conformance tests — automatically verify VimCode behavior against Neovim.
//!
//! Each test case defines:
//!   - initial buffer content (lines)
//!   - cursor position (1-indexed line, 1-indexed col — Neovim convention)
//!   - key sequence to execute (Vim normal-mode keys)
//!
//! The test runs the same scenario through both Neovim (headless) and VimCode,
//! then asserts that the resulting buffer content and cursor position match.
//!
//! To add a new conformance test, just add an entry to the `CASES` array.
//! No manual testing needed — Neovim is the oracle.
//!
//! Requires `nvim` on PATH. Tests are skipped (not failed) if nvim is missing.

mod common;

use common::engine_with;
use serde::Deserialize;
use std::io::Write;
use vimcode_core::Engine;

// ---------------------------------------------------------------------------
// Neovim oracle
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct NvimResult {
    buf: Vec<String>,
    line: usize,
    col: usize,
}

/// Run a key sequence in Neovim headless and return (buffer_lines, cursor_line_1, cursor_col_1).
fn run_in_neovim(
    lines: &[&str],
    cursor_line_1: usize,
    cursor_col_1: usize,
    keys: &str,
) -> Option<NvimResult> {
    // Build a Lua script that sets up buffer, runs keys, writes result
    let mut lua = String::new();
    lua.push_str("vim.o.compatible = false\n");
    lua.push_str("vim.o.shiftwidth = 4\n");
    lua.push_str("vim.o.expandtab = true\n");

    // Set buffer lines
    lua.push_str("vim.api.nvim_buf_set_lines(0, 0, -1, false, {");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            lua.push_str(", ");
        }
        // Escape backslashes and quotes for Lua string
        let escaped = line.replace('\\', "\\\\").replace('"', "\\\"");
        lua.push('"');
        lua.push_str(&escaped);
        lua.push('"');
    }
    lua.push_str("})\n");

    // Set cursor position (1-indexed)
    lua.push_str(&format!(
        "vim.api.nvim_win_set_cursor(0, {{{}, {}}})\n",
        cursor_line_1,
        cursor_col_1.saturating_sub(1) // nvim_win_set_cursor col is 0-indexed
    ));

    // Execute keys via feedkeys with 'nx' flags (noremap, execute immediately)
    // Escape special characters for Lua
    let escaped_keys = keys.replace('\\', "\\\\").replace('"', "\\\"");
    lua.push_str(&format!(
        "vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(\"{}\", true, false, true), \"nx\", false)\n",
        escaped_keys
    ));

    // Capture result
    let result_path = std::env::temp_dir().join("vimcode_nvim_conformance.json");
    let result_path_str = result_path.to_string_lossy().replace('\\', "/");
    lua.push_str(&format!(
        "local buf = vim.api.nvim_buf_get_lines(0, 0, -1, false)\n\
         local pos = vim.api.nvim_win_get_cursor(0)\n\
         local result = vim.fn.json_encode({{buf = buf, line = pos[1], col = pos[2] + 1}})\n\
         local f = io.open(\"{}\", \"w\")\n\
         f:write(result)\n\
         f:close()\n\
         vim.cmd(\"qa!\")\n",
        result_path_str
    ));

    // Write lua script to temp file
    let script_path = std::env::temp_dir().join("vimcode_nvim_conformance.lua");
    {
        let mut f = std::fs::File::create(&script_path).ok()?;
        f.write_all(lua.as_bytes()).ok()?;
    }

    // Remove old result file
    let _ = std::fs::remove_file(&result_path);

    // Run nvim
    let output = std::process::Command::new("nvim")
        .arg("--headless")
        .arg("-u")
        .arg("NONE")
        .arg("-l")
        .arg(script_path.to_string_lossy().as_ref())
        .output()
        .ok()?;

    if !output.status.success() {
        eprintln!("nvim stderr: {}", String::from_utf8_lossy(&output.stderr));
        return None;
    }

    // Read result
    let json = std::fs::read_to_string(&result_path).ok()?;
    serde_json::from_str(&json).ok()
}

// ---------------------------------------------------------------------------
// VimCode runner
// ---------------------------------------------------------------------------

fn press_char(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), false);
}

fn press_special(engine: &mut Engine, name: &str) {
    engine.handle_key(name, None, false);
}

fn press_ctrl(engine: &mut Engine, ch: char) {
    engine.handle_key(&ch.to_string(), Some(ch), true);
}

/// Parse and send a key sequence to the engine.
/// Supports <Esc>, <CR>, <C-x>, and literal characters.
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
                    n if n.starts_with("C-") => {
                        let ctrl_char = n.chars().nth(2).unwrap();
                        press_ctrl(engine, ctrl_char);
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
) -> (String, usize, usize) {
    let text = lines.join("\n");
    let mut engine = engine_with(&text);
    engine.settings.shift_width = 4;
    engine.settings.expand_tab = true;
    // Convert from 1-indexed to 0-indexed
    engine.view_mut().cursor.line = cursor_line_1.saturating_sub(1);
    engine.view_mut().cursor.col = cursor_col_1.saturating_sub(1);

    send_keys(&mut engine, keys);

    let buf = engine.buffer().to_string();
    let line = engine.view().cursor.line + 1; // back to 1-indexed
    let col = engine.view().cursor.col + 1;
    (buf, line, col)
}

// ---------------------------------------------------------------------------
// Test cases — add new conformance checks here
// ---------------------------------------------------------------------------

struct Case {
    /// Short label for the test
    label: &'static str,
    /// Initial buffer lines
    lines: &'static [&'static str],
    /// Cursor line (1-indexed)
    cursor_line: usize,
    /// Cursor col (1-indexed)
    cursor_col: usize,
    /// Key sequence to execute
    keys: &'static str,
}

const CASES: &[Case] = &[
    // --- Paragraph motions with operators ---
    Case {
        label: "d} basic",
        lines: &["line one", "", "line three"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "d}",
    },
    Case {
        label: "d{ basic",
        lines: &["line one", "", "line three"],
        cursor_line: 3,
        cursor_col: 1,
        keys: "d{",
    },
    Case {
        label: "d{ multi-line",
        lines: &["aaa", "", "bbb", "ccc", "ddd"],
        cursor_line: 5,
        cursor_col: 1,
        keys: "d{",
    },
    Case {
        label: "d{ from first line of para",
        lines: &["aaa", "", "bbb", "ccc"],
        cursor_line: 3,
        cursor_col: 1,
        keys: "d{",
    },
    Case {
        label: "d} from blank line",
        lines: &["aaa", "", "bbb", "ccc", "", "ddd"],
        cursor_line: 2,
        cursor_col: 1,
        keys: "d}",
    },
    Case {
        label: "d} to EOF (no trailing blank)",
        lines: &["aaa", "", "bbb", "ccc"],
        cursor_line: 3,
        cursor_col: 1,
        keys: "d}",
    },
    // --- Count multiplication ---
    Case {
        label: "2d2w",
        lines: &["one two three four five"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "2d2w",
    },
    Case {
        label: "3d2w",
        lines: &["a b c d e f g h"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "3d2w",
    },
    Case {
        label: "d3w",
        lines: &["one two three four five"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "d3w",
    },
    Case {
        label: "3dw",
        lines: &["one two three four five"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "3dw",
    },
    // --- Angle bracket text objects ---
    Case {
        label: "di<",
        lines: &["tag<x + y>end"],
        cursor_line: 1,
        cursor_col: 5,
        keys: "di<",
    },
    Case {
        label: "da<",
        lines: &["tag<x + y>end"],
        cursor_line: 1,
        cursor_col: 5,
        keys: "da<",
    },
    // --- Quote text objects with whitespace ---
    Case {
        label: "da\" trailing space",
        lines: &["say \"hello world\" now"],
        cursor_line: 1,
        cursor_col: 6,
        keys: "da\"",
    },
    Case {
        label: "da' trailing space",
        lines: &["say 'hello world' now"],
        cursor_line: 1,
        cursor_col: 6,
        keys: "da'",
    },
    Case {
        label: "da\" no trailing (leading consumed)",
        lines: &["say \"hello\""],
        cursor_line: 1,
        cursor_col: 6,
        keys: "da\"",
    },
    // --- Change + Esc cursor position ---
    Case {
        label: "cw<Esc>",
        lines: &["hello world foo"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "cw<Esc>",
    },
    Case {
        label: "ciw<Esc>",
        lines: &["hello world foo"],
        cursor_line: 1,
        cursor_col: 7,
        keys: "ciw<Esc>",
    },
    Case {
        label: "ci\"<Esc>",
        lines: &["say \"hello\" now"],
        cursor_line: 1,
        cursor_col: 6,
        keys: "ci\"<Esc>",
    },
    // --- Indent/outdent with motions ---
    Case {
        label: "<G outdent",
        lines: &["    one", "    two", "    three"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "<G",
    },
    Case {
        label: ">j indent",
        lines: &["one", "two", "three"],
        cursor_line: 1,
        cursor_col: 1,
        keys: ">j",
    },
    // --- Basic motions with operators ---
    Case {
        label: "dd",
        lines: &["one", "two", "three"],
        cursor_line: 2,
        cursor_col: 1,
        keys: "dd",
    },
    Case {
        label: "3dd",
        lines: &["a", "b", "c", "d", "e"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "3dd",
    },
    Case {
        label: "dj",
        lines: &["aaa", "bbb", "ccc"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "dj",
    },
    Case {
        label: "dk",
        lines: &["aaa", "bbb", "ccc"],
        cursor_line: 2,
        cursor_col: 1,
        keys: "dk",
    },
    Case {
        label: "dw",
        lines: &["hello world foo"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "dw",
    },
    Case {
        label: "de",
        lines: &["hello world foo"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "de",
    },
    Case {
        label: "db",
        lines: &["hello world foo"],
        cursor_line: 1,
        cursor_col: 7,
        keys: "db",
    },
    Case {
        label: "d$",
        lines: &["hello world"],
        cursor_line: 1,
        cursor_col: 6,
        keys: "d$",
    },
    Case {
        label: "d0",
        lines: &["hello world"],
        cursor_line: 1,
        cursor_col: 6,
        keys: "d0",
    },
    Case {
        label: "yy then p",
        lines: &["aaa", "bbb"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "yyp",
    },
    // --- Visual + delete ---
    Case {
        label: "vwd",
        lines: &["hello world foo"],
        cursor_line: 1,
        cursor_col: 1,
        keys: "vwd",
    },
];

// ---------------------------------------------------------------------------
// Test runner
// ---------------------------------------------------------------------------

#[test]
fn nvim_conformance() {
    // Check if nvim is available
    let nvim_ok = std::process::Command::new("nvim")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !nvim_ok {
        eprintln!("SKIP: nvim not found on PATH");
        return;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();

    for case in CASES {
        let nvim = run_in_neovim(case.lines, case.cursor_line, case.cursor_col, case.keys);
        let nvim = match nvim {
            Some(r) => r,
            None => {
                eprintln!("  SKIP [{}]: nvim execution failed", case.label);
                continue;
            }
        };

        let (vc_buf, vc_line, vc_col) =
            run_in_vimcode(case.lines, case.cursor_line, case.cursor_col, case.keys);

        let nvim_buf = nvim.buf.join("\n");
        let buf_match = vc_buf.trim_end_matches('\n') == nvim_buf.trim_end_matches('\n');
        // Neovim counts a trailing newline as an extra empty line;
        // ropey does not.  Allow VimCode cursor to be one line earlier
        // when the buffer ends with '\n' and Neovim's last line is empty.
        let cursor_match = if vc_line == nvim.line && vc_col == nvim.col {
            true
        } else if vc_buf.ends_with('\n')
            && nvim.buf.last().map(|s| s.is_empty()).unwrap_or(false)
            && vc_line + 1 == nvim.line
            && vc_col == nvim.col
        {
            true // trailing-newline line-count difference — acceptable
        } else {
            false
        };

        if buf_match && cursor_match {
            passed += 1;
        } else {
            failed += 1;
            let mut msg = format!("FAIL [{}] keys={:?}\n", case.label, case.keys);
            msg.push_str(&format!(
                "  buffer: nvim={:?} vimcode={:?}\n",
                nvim_buf, vc_buf
            ));
            msg.push_str(&format!(
                "  cursor: nvim=({},{}) vimcode=({},{})\n",
                nvim.line, nvim.col, vc_line, vc_col
            ));
            failures.push(msg);
        }
    }

    println!("\n=== Neovim Conformance Results ===");
    println!("Passed: {passed}");
    println!("Failed: {failed}");
    println!("Total:  {}\n", passed + failed);

    if !failures.is_empty() {
        println!("Failures:");
        for f in &failures {
            println!("{f}");
        }
        panic!("{failed} conformance test(s) failed");
    }
}
