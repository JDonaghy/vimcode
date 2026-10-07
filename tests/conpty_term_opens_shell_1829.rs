//! #1829: real-ConPTY regression test for "Windows TUI: `:term` opens a
//! blank panel with no PowerShell prompt".
//!
//! # Why this file, and why it only runs on Windows
//!
//! Every vimcode-side call site between the `:term` ex-command and the PTY
//! write is already shared, platform-neutral code with no Windows-specific
//! branch in it: `Engine::execute_command` maps `"terminal"` straight to
//! `EngineAction::OpenTerminal` (`src/core/engine/execute.rs`),
//! `render::handle_action`'s `OpenTerminal` arm calls
//! `Engine::terminal_new_tab` (`src/render.rs`) — the *same* function on
//! every backend — and once `terminal_has_focus` is set, every subsequent
//! keystroke is forwarded to the PTY by `render::route_terminal_key`
//! (`src/render.rs`), also shared. `TerminalSession::spawn`/`poll` is
//! `quadraui::terminal_engine` — not vimcode source at all. So if the panel
//! really does come up blank on Windows while working on Linux/macOS, the
//! fault has to be below this line, in the Windows ConPTY leg of
//! `portable_pty` that `quadraui::terminal_engine::TerminalSession` wraps —
//! confirmed, not assumed: at this repo's pinned quadraui rev,
//! `quadraui/src/terminal_engine.rs`'s own `#[cfg(test)] mod tests` is
//! almost entirely `#[cfg(unix)]`-gated (`grep -c '#\[cfg(unix)\]'` finds
//! dozens of hits, zero Windows-gated spawn/poll tests), so this exact path
//! has essentially no automated coverage on the platform the bug was
//! reported on.
//!
//! Proving anything about that leg needs the real thing: a genuine Win32
//! pseudo console (`CreatePseudoConsole` and friends), wrapped by
//! `portable_pty`'s Windows backend the same way
//! `tests/conpty_idle_flicker.rs` (#1634) and
//! `tests/conpty_activity_bar_click.rs` (#1636) already do — see those
//! files' own module docs for the full "why ConPTY, why `portable_pty`, why
//! this only ever runs on real Windows" rationale, which applies here
//! unchanged. This file borrows their spawn/poll/capture helpers
//! (duplicated locally rather than factored into a shared `tests/` module,
//! matching those two files' own choice to do the same for an unrelated
//! issue, to avoid touching their already-landed, independently-reasoned-
//! about files).
//!
//! # What this test actually proves
//!
//! It spawns `vcd.exe` under a real ConPTY on a small text file (mirroring
//! the issue's own repro: launched as if from a terminal, on a real file),
//! waits for the first real frame, sends the literal keystrokes `:term` +
//! Enter — exactly what a user types — then, after a settle window, sends
//! the issue's own acceptance probe: `echo <marker> > "<path>"` + Enter,
//! straight into the PTY exactly as a real keystroke stream would arrive.
//! It then polls the filesystem (not the screen — the probe's whole point
//! is that it proves the shell actually *ran a command*, not merely that
//! something painted) for that file to appear containing the marker.
//!
//! If `:term` really opens a dead/blank panel on Windows, the nested shell
//! never receives (or never acts on) the echoed command, the probe file
//! never appears, and this test fails with the captured screen attached to
//! the panic message for diagnosis — same shape as
//! `tests/conpty_activity_bar_click.rs`'s own failure messages.
//!
//! # Encoding note
//!
//! `default_shell()` resolves to `"powershell.exe"` (Windows PowerShell
//! 5.1) whenever `$SHELL` is unset, which is the case on a bare Windows
//! host with no WSL/Git-Bash tab involved (the issue's own "What is ruled
//! out" section). Windows PowerShell 5.1's `>` redirection (`Out-File`'s
//! default encoding) writes UTF-16LE with a BOM, not UTF-8 — reading the
//! probe file back as UTF-8 and doing a substring match would silently
//! fail even when the command *did* run. [`contains_marker`] below accepts
//! either encoding so this test cannot produce a false negative purely from
//! that cosmetic difference.
//!
//! # Not yet run on real hardware
//!
//! Unlike `tests/conpty_idle_flicker.rs` and `tests/conpty_activity_bar_click.rs`,
//! this file has **not** been built with `cargo xwin` and executed on
//! `dell64` as part of this change — this session has no attached Windows
//! host. It follows those two files' already-proven spawn/capture pattern
//! closely enough to compile with reasonable confidence, but per this
//! issue's own acceptance criteria, a real run on `dell64` (`cargo xwin
//! test --target x86_64-pc-windows-msvc`) plus a person watching the
//! screen for an actual PowerShell prompt are both still required before
//! #1829 can be considered verified, let alone closed.
#![cfg(windows)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Upper bound on how long this test waits for the startup paint, and
/// separately for the probe file to appear. Generous for the same reason
/// `tests/conpty_activity_bar_click.rs`'s `SETTLE_TIMEOUT` is: a cold
/// real-hardware PowerShell startup (profile scripts, etc.) can legitimately
/// take several seconds.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(20);

/// How long to wait after sending `:term` + Enter before typing the probe
/// command, so the `OpenTerminal` dispatch (synchronous in vimcode, per
/// `src/app.rs`'s own "Create the terminal tab immediately ... so the panel
/// appears on this same draw cycle" comment) has unambiguously already run
/// before the probe keystrokes are sent — not because the dispatch is slow,
/// but so a real-timing race between this test's own two writes is never
/// what's under test here.
const AFTER_OPEN_SETTLE: Duration = Duration::from_millis(1500);

const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 120;

/// Mirrors `tests/conpty_idle_flicker.rs`/`tests/conpty_activity_bar_click.rs`'s
/// identical struct.
struct Captured {
    bytes: Vec<u8>,
}

/// Build an isolated `%APPDATA%`/`%USERPROFILE%` for the spawned `vcd.exe`,
/// seeded with settings that make this test deterministic (no LSP spawn) —
/// mirrors `tests/conpty_activity_bar_click.rs`'s `isolated_home`.
fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_conpty_1829_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let vimcode_dir = home.join("AppData").join("Roaming").join("vimcode");
    std::fs::create_dir_all(&vimcode_dir).expect("create isolated %APPDATA%\\vimcode");
    std::fs::write(
        vimcode_dir.join("settings.json"),
        r#"{"lsp_enabled": false, "use_nerd_fonts": false}"#,
    )
    .expect("seed settings.json");
    home
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `true` if `bytes` contains `marker` encoded as plain ASCII/UTF-8, or as
/// UTF-16LE/UTF-16BE (each ASCII byte padded with a `0x00` on the other
/// side) — see this file's module doc "Encoding note".
fn contains_marker(bytes: &[u8], marker: &str) -> bool {
    if contains_subslice(bytes, marker.as_bytes()) {
        return true;
    }
    let mut utf16le = Vec::with_capacity(marker.len() * 2);
    let mut utf16be = Vec::with_capacity(marker.len() * 2);
    for b in marker.as_bytes() {
        utf16le.push(*b);
        utf16le.push(0u8);
        utf16be.push(0u8);
        utf16be.push(*b);
    }
    contains_subslice(bytes, &utf16le) || contains_subslice(bytes, &utf16be)
}

/// Spawn `vcd.exe` under a real ConPTY, opened on `main_rs`, with `cwd` as
/// its working directory. See `tests/conpty_activity_bar_click.rs`'s
/// `spawn_under_conpty` doc for why the reader thread must also answer
/// `ESC [ 6 n` from an independently spawned thread (a real, reproducible
/// ConPTY deadlock otherwise) — this is the identical responder.
fn spawn_under_conpty(
    main_rs: &PathBuf,
    home: &PathBuf,
) -> (
    Box<dyn portable_pty::Child + Send + Sync>,
    Arc<Mutex<Captured>>,
    Arc<Mutex<Box<dyn Write + Send>>>,
    Box<dyn portable_pty::MasterPty + Send>,
) {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: PTY_ROWS,
            cols: PTY_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty (real Win32 ConPTY)");

    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vcd"));
    let mut cmd = CommandBuilder::new(&exe);
    cmd.arg(main_rs);
    cmd.cwd(home);
    let appdata = home.join("AppData").join("Roaming");
    cmd.env("APPDATA", &appdata);
    cmd.env("USERPROFILE", home);
    cmd.env("LOCALAPPDATA", &appdata);
    cmd.env("VIMCODE_TEST_DATA_HOME", home.join("data"));

    let child = pair
        .slave
        .spawn_command(cmd)
        .expect("spawn vcd.exe under ConPTY");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone ConPTY reader");
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
        pair.master.take_writer().expect("take ConPTY writer"),
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
                    if contains_subslice(chunk, b"\x1b[6n") {
                        let w2 = Arc::clone(&writer_for_thread);
                        std::thread::spawn(move || {
                            let mut w = w2.lock().unwrap();
                            let _ = w.write_all(b"\x1b[1;1R");
                            let _ = w.flush();
                        });
                    }
                }
                Err(_) => break,
            }
        }
    });

    (child, captured, writer, pair.master)
}

/// Poll the captured byte stream until [`screen_text`] (fed through a fresh
/// `vt100::Parser`) contains `needle`, or fail after `timeout`. Mirrors
/// `tests/conpty_activity_bar_click.rs`'s `wait_for_screen_contains`
/// (duplicated rather than imported — see that file's own module doc for
/// why the fixed-quiet-window alternative is flaky on real hardware).
fn wait_for_screen_contains(
    captured: &Arc<Mutex<Captured>>,
    needle: &str,
    timeout: Duration,
) -> bool {
    let start = Instant::now();
    loop {
        let bytes = { captured.lock().unwrap().bytes.clone() };
        let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
        parser.process(&bytes);
        if screen_text(&parser).contains(needle) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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
            out.push_str(if s.is_empty() { " " } else { &s });
        }
        out.push('\n');
    }
    out
}

/// Poll the filesystem for `path` to exist and contain `marker` (in any of
/// the encodings [`contains_marker`] accepts), or fail after `timeout`.
fn wait_for_probe_file(path: &PathBuf, marker: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        if let Ok(bytes) = std::fs::read(path) {
            if contains_marker(&bytes, marker) {
                return true;
            }
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn term_command_opens_a_working_shell_over_real_conpty_1829() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

    let (mut child, captured, writer, _master) = spawn_under_conpty(&main_rs, &home);

    // Wait for the first real frame (status bar's NORMAL mode indicator) —
    // same precondition `tests/conpty_activity_bar_click.rs` waits on.
    assert!(
        wait_for_screen_contains(&captured, "NORMAL", SETTLE_TIMEOUT),
        "vcd.exe never painted its first real frame (status bar's NORMAL \
         mode indicator) within {SETTLE_TIMEOUT:?}"
    );

    // Type `:term` + Enter — the issue's own repro gesture, byte-for-byte
    // what a real keystroke stream delivers.
    {
        let mut w = writer.lock().unwrap();
        w.write_all(b":term\r").expect("write :term<Enter>");
        w.flush().ok();
    }

    std::thread::sleep(AFTER_OPEN_SETTLE);

    // The issue's own acceptance probe, generalised to an isolated,
    // collision-free path under this test's own temp root rather than a
    // fixed `$env:TEMP\vc_term_probe.txt` (so concurrent real-hardware runs
    // of this test, or of the manual repro alongside it, cannot collide).
    let marker = format!("MARKER_1829_{}", std::process::id());
    let probe_path = std::env::temp_dir().join(format!(
        "vc_term_probe_1829_{}_{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    // Best-effort: in case a stale file from an earlier run exists at this
    // exact path (should be impossible given the nanosecond-resolution
    // name, but costs nothing to guard).
    let _ = std::fs::remove_file(&probe_path);

    let command_line = format!("echo {marker} > \"{}\"\r", probe_path.display());
    {
        let mut w = writer.lock().unwrap();
        w.write_all(command_line.as_bytes())
            .expect("write probe echo command");
        w.flush().ok();
    }

    let found = wait_for_probe_file(&probe_path, &marker, SETTLE_TIMEOUT);

    // Best-effort teardown before asserting, so a failed assertion doesn't
    // leave a `vcd.exe`/shell process tree behind.
    {
        let mut w = writer.lock().unwrap();
        w.write_all(b"\x1b\x1b:qa!\r").ok();
        w.flush().ok();
    }
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&probe_path);

    let final_screen = {
        let bytes = captured.lock().unwrap().bytes.clone();
        let mut p = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
        p.process(&bytes);
        screen_text(&p)
    };

    assert!(
        found,
        "typing `:term` then `echo {marker} > \"{}\"` + Enter over a real \
         ConPTY never produced the probe file within {SETTLE_TIMEOUT:?} — \
         this is #1829's exact symptom (the terminal panel opened but the \
         shell inside it never ran the command, i.e. no working PowerShell \
         prompt ever received the keystrokes); last-observed screen:\n{}",
        probe_path.display(),
        final_screen
    );
}
