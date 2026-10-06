//! #1779 (bugbash:tui-pty:linux): "Adding a line that becomes the buffer's
//! new last line is not repainted correctly (line 1 vanishes, content shown
//! one row too high)".
//!
//! # Root cause, and why this file exists anyway
//!
//! The bug *is* in `src/render.rs`: `run_shared_tick_chores` fed
//! `Engine::set_viewport_for_window` the previous frame's *painted* line
//! count (`RenderedWindow::lines.len()`) instead of the window's real row
//! *capacity* (the new `RenderedWindow::visible_line_capacity` field this
//! same fix adds). It is reachable in-process, too:
//! `src/tui_main/app_on_tui_tests.rs`'s
//! `opening_a_line_below_the_last_line_paints_every_line_in_order_1779`
//! reproduces it with a plain `TuiDriver` by calling the driver's own
//! public `tick()` between the initial render and the `o` keystroke — no
//! pty required. An earlier version of this comment claimed both
//! in-process drivers stayed green and concluded the defect must be a
//! real-pty transport-layer issue; that was wrong on both counts; see that
//! test's own doc for the corrected explanation and the RED-verification
//! steps.
//!
//! This file exists as additional, real-pty, end-to-end coverage for the
//! same bug — not because the in-process drivers can't reach it, but
//! because the original bugbash report came in over a real pty and that
//! exact path is worth pinning directly, the same way
//! `tests/pty_settings_header_delay.rs` and
//! `tests/conpty_activity_bar_click.rs` pin other real-pty scenarios. A
//! real `vcd` binary, under a real Unix pty, opened directly on a one-line
//! file (the issue's own repro), typing the exact key sequence the bug
//! report gives and reading the real byte stream back through `vt100`.
//! (`TuiVtDriver`, the other in-process driver, has no public `tick()` at
//! this repo's pinned quadraui rev, so it genuinely can't be fixed the same
//! way as the `TuiDriver` test — see that test's doc for the detail.)
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);
const PTY_ROWS: u16 = 24;
const PTY_COLS: u16 = 80;

struct Captured {
    bytes: Vec<u8>,
}

fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_pty_1779_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config_dir = home.join(".config").join("vimcode");
    std::fs::create_dir_all(&config_dir).expect("create isolated $HOME/.config/vimcode");
    // Mirrors the issue's own repro settings exactly: LSP off (no spawn
    // noise), nerd fonts off (plain-ASCII screen, nothing to do with this
    // bug but keeps the capture simple), vim mode explicit.
    std::fs::write(
        config_dir.join("settings.json"),
        r#"{"lsp_enabled": false, "use_nerd_fonts": false, "editor_mode": "vim"}"#,
    )
    .expect("seed settings.json");
    home
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

type SpawnedPty = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Arc<Mutex<Captured>>,
    Arc<Mutex<Box<dyn Write + Send>>>,
    Box<dyn portable_pty::MasterPty + Send>,
);

/// Spawn `vcd` under a real Unix pty, opened directly on `file_path`, with
/// `home` as its `$HOME`. Mirrors `tests/pty_settings_header_delay.rs`'s
/// `spawn_under_pty` shape (including answering the startup `ESC [ 6 n`
/// cursor-position query inline from the reader thread).
fn spawn_under_pty(file_path: &PathBuf, home: &PathBuf) -> SpawnedPty {
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
    cmd.arg(file_path);
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

fn wait_for_screen_contains(
    parser: &mut vt100::Parser,
    captured: &Arc<Mutex<Captured>>,
    fed: &mut usize,
    needle: &str,
    timeout: Duration,
) -> Option<Duration> {
    let start = Instant::now();
    loop {
        sync_parser(parser, captured, fed);
        if screen_text(parser).contains(needle) {
            return Some(start.elapsed());
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Poll `path` until it reads back as `expected`, or until `timeout`
/// elapses. Returns whatever the final read saw, so the caller can assert
/// on it and get a useful diff when it never converged.
fn wait_for_file_contents(path: &PathBuf, expected: &str, timeout: Duration) -> String {
    let start = Instant::now();
    loop {
        let contents = std::fs::read_to_string(path).unwrap_or_default();
        if contents == expected || start.elapsed() >= timeout {
            return contents;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Row index (0-based) of the first row whose text contains `needle`, or
/// `None` if it isn't painted anywhere.
fn row_of(parser: &vt100::Parser, needle: &str) -> Option<usize> {
    screen_text(parser)
        .lines()
        .position(|line| line.contains(needle))
}

fn send_bytes(writer: &Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) {
    let mut w = writer.lock().unwrap();
    w.write_all(bytes).expect("write to pty");
    w.flush().ok();
}

/// Same as [`send_bytes`] but never panics — for teardown writes, where the
/// child may already have exited (an EIO on the pty write would otherwise
/// panic and mask whatever verdict the test already reached).
fn send_bytes_best_effort(writer: &Arc<Mutex<Box<dyn Write + Send>>>, bytes: &[u8]) {
    let mut w = writer.lock().unwrap();
    let _ = w.write_all(bytes);
    let _ = w.flush();
}

/// #1779's exact single-line repro: `foo.txt` containing just `foo\n`,
/// press `o`, type `bar`, press Escape. Both `foo` and `bar` must still be
/// painted, each on its own row, in order — `foo` must not vanish, and
/// `bar` must not land on `foo`'s row with a blank row below it (the
/// reported symptom).
#[test]
fn opening_a_line_below_the_only_line_paints_both_lines_1779() {
    let home = isolated_home();
    // Deliberately named so its on-screen path (tab title, breadcrumb bar)
    // never collides with the two buffer-content markers searched for
    // below — otherwise `row_of` can match the breadcrumb bar's path
    // segment instead of the editor's actual content row.
    let file_path = home.join("zqxw_1779.txt");
    std::fs::write(&file_path, "ZQXW_ALPHA\n").expect("seed zqxw_1779.txt");

    let (mut child, captured, writer, _master) = spawn_under_pty(&file_path, &home);

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
    let mut fed = 0usize;

    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "NORMAL", SETTLE_TIMEOUT)
            .is_some(),
        "vcd never reached NORMAL mode within {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );
    assert!(
        wait_for_screen_contains(
            &mut parser,
            &captured,
            &mut fed,
            "ZQXW_ALPHA",
            SETTLE_TIMEOUT
        )
        .is_some(),
        "precondition: 'ZQXW_ALPHA' must be painted before any edit; screen:\n{}",
        screen_text(&parser)
    );
    // 'o', type "ZQXW_BETA", Escape — exactly the issue's own repro steps.
    //
    // Each step waits for the *screen* to confirm the previous keystroke
    // landed rather than sleeping a fixed interval: on a loaded runner a
    // fixed sleep can elapse while `o` is still in flight, and every
    // subsequent keystroke then lands in the wrong mode. See the
    // `wait_for_screen_contains("NORMAL")` below for the specific failure
    // this cost us.
    send_bytes(&writer, b"o");
    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "INSERT", SETTLE_TIMEOUT)
            .is_some(),
        "'o' never put the editor into INSERT mode within {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );
    send_bytes(&writer, b"ZQXW_BETA");
    assert!(
        wait_for_screen_contains(
            &mut parser,
            &captured,
            &mut fed,
            "ZQXW_BETA",
            SETTLE_TIMEOUT
        )
        .is_some(),
        "typing \"ZQXW_BETA\" in INSERT mode never painted it within \
         {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );
    send_bytes(&writer, b"\x1b"); // Escape

    // Wait for the status bar to flip back out of INSERT before doing
    // anything else. This is a correctness requirement, not just tidiness:
    // a bare `\x1b` arriving in the *same* `read()` as the bytes that
    // follow it is parsed by crossterm as `Alt+<next char>`, not as Escape
    // — so writing `:w\r` microseconds after the Escape byte can be read as
    // `Alt+:` plus a stray `w`, the editor never leaves INSERT mode, and
    // the `:w` below silently never runs. That is exactly how this test
    // failed intermittently: the on-disk assertion reported the untouched
    // seed content `"ZQXW_ALPHA\n"`, because no write command was ever
    // executed. Confirming the mode transition on screen guarantees the
    // Escape byte was consumed on its own.
    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "NORMAL", SETTLE_TIMEOUT)
            .is_some(),
        "Escape never returned the editor to NORMAL mode within \
         {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );
    assert!(
        wait_for_screen_contains(
            &mut parser,
            &captured,
            &mut fed,
            "ZQXW_BETA",
            SETTLE_TIMEOUT
        )
        .is_some(),
        "'o' + \"ZQXW_BETA\" + Escape never painted 'ZQXW_BETA' at all; screen:\n{}",
        screen_text(&parser)
    );
    // Bounded poll for both markers to be present, rather than a fixed
    // sleep: every other wait in this file already polls with a 15s
    // budget, and a fixed sample here can be taken before a later repaint
    // lands on a loaded CI runner, losing the row-adjacency check's RED
    // behaviour to timing noise rather than the bug itself.
    let settle_start = Instant::now();
    let (alpha_row, beta_row) = loop {
        sync_parser(&mut parser, &captured, &mut fed);
        let alpha_row = row_of(&parser, "ZQXW_ALPHA");
        let beta_row = row_of(&parser, "ZQXW_BETA");
        if (alpha_row.is_some() && beta_row.is_some()) || settle_start.elapsed() >= SETTLE_TIMEOUT {
            break (alpha_row, beta_row);
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let screen = screen_text(&parser);
    assert!(
        alpha_row.is_some() && beta_row.is_some(),
        "#1779: both 'ZQXW_ALPHA' (original line) and 'ZQXW_BETA' (opened \
         with 'o') must be visible on the real terminal after opening a \
         new line below the buffer's only line; alpha_row={alpha_row:?} \
         beta_row={beta_row:?} screen:\n{screen}"
    );
    assert_eq!(
        beta_row,
        alpha_row.map(|r| r + 1),
        "#1779: 'ZQXW_BETA' (the line opened with 'o') must paint \
         immediately below 'ZQXW_ALPHA' (the original line), with no \
         blank row between them and no row vanishing; \
         alpha_row={alpha_row:?} beta_row={beta_row:?} screen:\n{screen}"
    );

    // Confirm the file itself was always correct (per the issue report) —
    // isolates this test to the *paint*, not a buffer-content regression.
    //
    // Bounded poll rather than a fixed sleep, for the same reason as the
    // screen waits above: `:w` has to round-trip through the pty, the
    // editor's event loop and a real filesystem write, and a fixed 300ms
    // sample can land before that completes on a loaded runner.
    send_bytes(&writer, b":w\r");
    let on_disk = wait_for_file_contents(&file_path, "ZQXW_ALPHA\nZQXW_BETA\n", SETTLE_TIMEOUT);
    assert_eq!(
        on_disk,
        "ZQXW_ALPHA\nZQXW_BETA\n",
        "precondition: the saved file content must be correct even if the \
         paint (asserted above) is not; screen at the time of the check:\n{}",
        screen_text(&parser)
    );

    // Best-effort teardown — the child may already have exited by now, so
    // these writes must not panic on an EIO and mask the real verdict above.
    send_bytes_best_effort(&writer, b"\x1b:qa!\r");
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&home);
}
