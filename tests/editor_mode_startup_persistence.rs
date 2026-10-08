//! #1795: `editor_mode: "vscode"` in `settings.json` must be honoured at
//! **cold startup** (`Engine::new()`'s own `Settings::load()`), not only on
//! the hot-reload path `check_settings_reload` already covers.
//!
//! # Why a real pty driving the real binary, not an in-process `Engine::new()`
//!
//! #1795's reported symptom is specifically that a real, running vimcode's
//! **painted status bar** read `NORMAL` at launch despite an on-disk
//! `settings.json` saying `"editor_mode": "vscode"`
//! (`/tmp/j_appdata_test2.png`). `CLAUDE.md`'s "Testing (CRITICAL)" rule 1
//! is explicit that a test must assert on rendered output, never on state
//! being populated — a prior version of this file called `Engine::new()`
//! in-process and asserted `engine.is_vscode_mode()`/`engine.mode_str()`,
//! which are core getters, not painted bytes, and a review caught that gap.
//!
//! This file instead mirrors the two real-process pty harnesses this repo
//! already ships for exactly this class of problem —
//! `tests/pty_settings_header_delay.rs` (#1701, Unix pty + `vt100`) and
//! `tests/conpty_idle_flicker.rs` (#1634, Windows ConPTY) — spawning the
//! real `vcd` binary (`CARGO_BIN_EXE_vcd`) under a real Unix pty, with a
//! throwaway `$HOME` set **only on the spawned child's own environment**
//! (`CommandBuilder::env`, never `std::env::set_var` on this test process —
//! see those two files' own module docs for why process-global env
//! mutation in a test is a standing hazard in this crate,
//! `src/core/paths.rs`/`src/core/settings.rs`), and asserts on the actual
//! VT100-parsed screen content a real terminal would show. `#![cfg(unix)]`
//! because `portable-pty`'s Unix backend (a plain pty, not ConPTY) is what
//! this dev-dependency set provides outside Windows — see `Cargo.toml`'s
//! `[target.'cfg(unix)'.dev-dependencies]` comment.
//!
//! `Settings::load()`'s `#[cfg(test)]` short-circuit (see
//! `src/core/settings.rs`, "Tests must be hermetic — never read the user's
//! settings.json") only ever applies to the `--lib`/unit-test build; the
//! separately-compiled `vcd` binary this test spawns has no such gate, so
//! this is the one place able to exercise the real cold-start
//! `Engine::new()` -> `Settings::load()` -> `Settings::load_with_validation()`
//! path against a literal on-disk `settings.json`, end to end through the
//! real process launch and real paint.
//!
//! # Investigation result (see PR for the full writeup; durable record now
//! also lives in `tests/smoke-spec/win-terminal.yaml`'s and
//! `tests/smoke-spec/win-gui.yaml`'s own header comments)
//!
//! Reproduced on real Windows hardware (`cargo xwin build --features win`,
//! run both directly and via a PowerShell session with `$env:APPDATA`
//! pointed at a throwaway settings.json containing exactly #1795's
//! reported content): `Engine::new()`/`Engine::startup()` correctly loaded
//! `editor_mode: Vscode` and `Engine::mode_str()` read
//! `"EDIT  F1:palette  Alt-M:vim"`, never `"NORMAL"`, in every case tried.
//! The one case that *did* reproduce "settings edited but a different file
//! got read" was launching the exe from a WSL shell with `APPDATA=<custom>`
//! set only on the *parent* shell's command line (`env APPDATA=... ./a.exe`)
//! — a WSL-interop process-creation quirk that silently drops that
//! override, so the exe reads the real `%APPDATA%\vimcode\settings.json`
//! instead of the one just edited for the test. That is a test-harness
//! environment-propagation gap, not a vimcode-code bug — this test instead
//! pins the one thing that *is* a vimcode-code question, so it cannot
//! silently regress: does the real `vcd` binary honour `editor_mode` from a
//! real on-disk `settings.json` at cold startup, as observed on its actual
//! painted screen. It is green on unfixed `develop`, by design (there is no
//! code bug here) — see the PR body for why this is coverage, not a
//! RED-then-GREEN regression fix.
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Upper bound on how long this test waits for the startup paint to show
/// the expected mode indicator before giving up and failing outright.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);

const PTY_ROWS: u16 = 30;
const PTY_COLS: u16 = 100;

/// Shared state the reader thread updates: every byte it has ever seen.
struct Captured {
    bytes: Vec<u8>,
}

/// Build an isolated `$HOME` whose `.config/vimcode/settings.json` carries
/// the exact content #1795 reported (`lsp_enabled`/`use_nerd_fonts` are
/// along for the ride, unused by this test's assertions, but kept to match
/// the report's reproduction verbatim) — never the real operator's
/// settings.json.
fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_test_1795_home_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config_dir = home.join(".config").join("vimcode");
    std::fs::create_dir_all(&config_dir).expect("create isolated $HOME/.config/vimcode");
    std::fs::write(
        config_dir.join("settings.json"),
        r#"{"lsp_enabled": false, "use_nerd_fonts": false, "editor_mode": "vscode"}"#,
    )
    .expect("seed settings.json");
    home
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Spawn `vcd` under a real Unix pty, opened on `main_rs`, with `home` as
/// its `$HOME` — set only on the **spawned child's** environment
/// (`CommandBuilder::env`), never this test process's own `std::env`, so
/// this test binary stays safe to run alongside any other `#[test]` in the
/// same process without a shared-env race. Returns the child, the
/// `Captured` handle the background reader thread fills in, and the pty
/// writer — shared (`MasterPty::take_writer` may only be called once per
/// pty) between the reader thread's cursor-position-query reply and this
/// test's own teardown write.
type SpawnedPty = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Arc<Mutex<Captured>>,
    Arc<Mutex<Box<dyn Write + Send>>>,
    Box<dyn portable_pty::MasterPty + Send>,
);

fn spawn_under_pty(main_rs: &PathBuf, home: &PathBuf) -> SpawnedPty {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty (real Unix pty)");

    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vcd"));
    let mut cmd = CommandBuilder::new(&exe);
    cmd.arg(main_rs);
    cmd.cwd(home);
    cmd.env("HOME", home);
    cmd.env(
        "VIMCODE_TEST_DATA_HOME",
        home.join(".local").join("share").join("vimcode"),
    );

    let child = pair
        .slave
        .spawn_command(cmd)
        .expect("spawn vcd under a real Unix pty");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
        pair.master.take_writer().expect("take pty writer"),
    ));

    let captured = Arc::new(Mutex::new(Captured { bytes: Vec::new() }));
    let captured_for_thread = Arc::clone(&captured);
    let writer_for_thread = Arc::clone(&writer);
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    let mut c = captured_for_thread.lock().unwrap();
                    c.bytes.extend_from_slice(chunk);
                    drop(c);
                    // Answer `ESC [ 6 n` (the cursor-position query
                    // `ratatui::Terminal::new()` sends during startup) —
                    // mirrors `tests/pty_settings_header_delay.rs`'s own
                    // reader thread; a plain Unix pty tolerates answering
                    // inline with no cross-thread deadlock.
                    if contains_subslice(chunk, b"\x1b[6n") {
                        let mut w = writer_for_thread.lock().unwrap();
                        let _ = w.write_all(b"\x1b[1;1R");
                        let _ = w.flush();
                    }
                }
                Err(_) => break,
            }
        }
    });

    (child, captured, writer, pair.master)
}

fn sync_parser(parser: &mut vt100::Parser, captured: &Arc<Mutex<Captured>>, fed: &mut usize) {
    let new_bytes = {
        let c = captured.lock().unwrap();
        c.bytes[*fed..].to_vec()
    };
    *fed += new_bytes.len();
    parser.process(&new_bytes);
}

fn screen_text(parser: &vt100::Parser) -> String {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let mut out = String::with_capacity((rows as usize) * (cols as usize + 1));
    for y in 0..rows {
        for x in 0..cols {
            let Some(cell) = screen.cell(y, x) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let s = cell.contents();
            out.push_str(if s.is_empty() { " " } else { s });
        }
        out.push('\n');
    }
    out
}

/// Poll the real pty's parsed screen until it contains `needle`, checking
/// on every poll that it does NOT (yet) contain `forbidden` — #1795's
/// reported symptom verbatim is the status bar reading `NORMAL`, so this
/// must never be true at *any* point along the way to the first `EDIT`
/// paint, not just absent from the final frame (a flash of `NORMAL` before
/// correcting itself would be just as real a regression).
fn wait_for_screen_contains_never_seeing(
    parser: &mut vt100::Parser,
    captured: &Arc<Mutex<Captured>>,
    fed: &mut usize,
    needle: &str,
    forbidden: &str,
    timeout: Duration,
) -> Result<Duration, String> {
    let start = Instant::now();
    loop {
        sync_parser(parser, captured, fed);
        let text = screen_text(parser);
        if text.contains(forbidden) {
            return Err(format!(
                "status bar read {forbidden:?} at some point before {needle:?} \
                 ever appeared — #1795's reported symptom verbatim; screen:\n{text}"
            ));
        }
        if text.contains(needle) {
            return Ok(start.elapsed());
        }
        if start.elapsed() >= timeout {
            return Err(format!(
                "vcd never painted {needle:?} within {timeout:?}; screen:\n{text}"
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn vcd_honours_editor_mode_vscode_from_disk_settings_json_at_cold_startup() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

    let (mut child, captured, writer, _master) = spawn_under_pty(&main_rs, &home);

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
    let mut fed = 0usize;

    let result = wait_for_screen_contains_never_seeing(
        &mut parser,
        &captured,
        &mut fed,
        "EDIT",
        "NORMAL",
        SETTLE_TIMEOUT,
    );

    // Best-effort teardown regardless of the assertion outcome below.
    {
        let mut w = writer.lock().unwrap();
        let _ = w.write_all(b"\x1b:qa!\r"); // Escape (cancel pending op) + force-quit-all
        let _ = w.flush();
    }
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&home);

    match result {
        Ok(_elapsed) => {}
        Err(msg) => panic!(
            "vcd must resolve a cold-started on-disk settings.json's \
             \"editor_mode\": \"vscode\" to a painted EDIT status bar, never \
             NORMAL: {msg}"
        ),
    }
}
