//! #1779 (bugbash:tui-pty:linux): "Adding a line that becomes the buffer's
//! new last line is not repainted correctly (line 1 vanishes, content shown
//! one row too high)".
//!
//! # Why a real pty, not `TuiDriver` / `TuiVtDriver`
//!
//! Both in-process drivers were tried first (see
//! `src/tui_main/app_on_tui_tests.rs`'s
//! `opening_a_line_below_the_last_line_paints_every_line_in_order_1779` and
//! `..._via_vt_driver_1779`) and **both stayed green** driving the exact
//! same key sequence against the exact same `App`/`Engine`/render pipeline
//! this bug report names — one reading ratatui's own in-memory `Buffer`
//! directly, the other reading a real `CrosstermBackend`'s ANSI byte
//! stream through a real `vt100::Parser`. Neither reproduces the bug,
//! which means it is not in `Engine::handle_key` or `render.rs`'s layout
//! math (both already covered, by both drivers, and both correct) — it is
//! specifically a real-terminal/real-pty transport-layer defect, the same
//! quadraui#302-shaped blind spot `tests/pty_settings_header_delay.rs` and
//! `tests/conpty_activity_bar_click.rs` already carve real-pty test files
//! out for. This file is that carve-out for #1779: a real `vcd` binary,
//! under a real Unix pty, opened directly on a one-line file (the issue's
//! own repro), typing the exact key sequence the bug report gives and
//! reading the real byte stream back through `vt100`.
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
    send_bytes(&writer, b"o");
    std::thread::sleep(Duration::from_millis(100));
    send_bytes(&writer, b"ZQXW_BETA");
    std::thread::sleep(Duration::from_millis(100));
    send_bytes(&writer, b"\x1b"); // Escape

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
    // Give the final frame time to fully settle before reading rows back.
    std::thread::sleep(Duration::from_millis(300));
    sync_parser(&mut parser, &captured, &mut fed);

    let screen = screen_text(&parser);
    let alpha_row = row_of(&parser, "ZQXW_ALPHA");
    let beta_row = row_of(&parser, "ZQXW_BETA");
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
    send_bytes(&writer, b":w\r");
    std::thread::sleep(Duration::from_millis(300));
    let on_disk = std::fs::read_to_string(&file_path).unwrap_or_default();
    assert_eq!(
        on_disk, "ZQXW_ALPHA\nZQXW_BETA\n",
        "precondition: the saved file content must be correct even if the \
         paint (asserted above) is not"
    );

    // Best-effort teardown.
    send_bytes(&writer, b"\x1b:qa!\r");
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.kill();
    let _ = child.wait();
}
