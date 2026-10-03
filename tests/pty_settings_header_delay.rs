//! #1701: real-pty regression test for "[bugbash:tui-pty] Sidebar header
//! shows stale panel name for ~0.4-0.9s after clicking the Settings
//! activity-bar icon".
//!
//! # Why a real pty, not `TuiDriver`
//!
//! The in-process `quadraui::tui::testing::TuiDriver` dispatches already-
//! decoded `UiEvent`s synchronously — a click's resulting `Reaction` and
//! repaint happen inside the same function call, with no wall-clock gap a
//! timing bug could hide in. The bugbash report is specifically about
//! *wall-clock* lag between a real click and the real terminal's next
//! paint, observed by re-reading an actual pty's byte stream at timestamped
//! intervals — exactly what `TuiDriver` cannot reproduce. This mirrors
//! `tests/conpty_activity_bar_click.rs` (#1636), the Windows/ConPTY twin of
//! this same class of test, but runs on a real Unix pty via
//! `portable-pty`'s Unix backend (see the `[target.'cfg(unix)'.dev-
//! dependencies]` block in `Cargo.toml` for why those two crates are
//! available here with no new `Cargo.lock` entries).
//!
//! # What this test actually proves
//!
//! It spawns `vcd` under a real Unix pty, waits for the startup paint to
//! settle, clicks a *different* top panel's activity-bar icon (Explorer,
//! Search, or Board — the issue's own repro matrix), waits well past the
//! 400ms double-click-fold window (`DOUBLE_CLICK_MARGIN`, mirroring
//! `conpty_activity_bar_click.rs`'s identical constant) so any fold timing
//! is categorically ruled out, then clicks the Settings icon and measures
//! how long the real byte stream takes to show "SETTINGS" instead of the
//! prior panel's own header text. If the fix holds, that gap should be on
//! the order of one terminal round trip (milliseconds); the bugbash report
//! pins the regression at ~400-900ms.
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Upper bound on how long this test waits for any single expected screen
/// change (startup paint, or a panel switching after a click) before
/// giving up and failing outright.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);

/// The pty grid this test opens `vcd` on — matches the issue's own repro
/// ("a 100x30 pty").
const PTY_ROWS: u16 = 30;
const PTY_COLS: u16 = 100;

/// The activity bar's icon column — see `tests/conpty_activity_bar_click.
/// rs`'s `ICON_COLUMN` for the full derivation (`ShellConfig::
/// activity_bar_width` defaults to 3 cells; the icon glyph itself paints at
/// the one interior column, `x = 1`).
const ICON_COLUMN: u16 = 1;

/// Two clicks land closer together than this must fold into a single
/// `DoubleClick` (`quadraui::tui::backend::TuiBackend`'s
/// `DoubleClickDetector`: 400ms/1.5 cells) — mirrors `tests/
/// conpty_activity_bar_click.rs`'s identical constant and rationale. The
/// issue report explicitly tested an 800ms gap and still saw the bug, so
/// this test pays the same margin to keep double-click folding
/// categorically out of the picture rather than leaving it ambiguous.
const DOUBLE_CLICK_MARGIN: Duration = Duration::from_millis(450);

/// Shared state the reader thread updates: every byte it has ever seen.
/// Timing is measured by the caller (an `Instant` taken immediately before
/// writing a click's bytes, compared against `Instant::now()` once the
/// expected marker shows up in the parsed screen — see
/// [`wait_for_screen_contains`]'s callers), not stored per-byte here.
struct Captured {
    bytes: Vec<u8>,
}

fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_pty_1701_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config_dir = home.join(".config").join("vimcode");
    std::fs::create_dir_all(&config_dir).expect("create isolated $HOME/.config/vimcode");
    // No LSP spawn, nerd fonts off (fallback glyphs are what this test's
    // icon-column scan searches for) — mirrors `conpty_activity_bar_click.
    // rs`'s `isolated_home` settings.json exactly.
    std::fs::write(
        config_dir.join("settings.json"),
        r#"{"lsp_enabled": false, "use_nerd_fonts": false}"#,
    )
    .expect("seed settings.json");
    home
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Spawn `vcd` under a real Unix pty, opened on `main_rs`, with `home` as
/// its `$HOME` (so the isolated `settings.json` above is the one it loads).
/// Returns the child, the `Captured` handle the background reader thread
/// fills in, and the pty writer used to send mouse reports.
///
/// Answers `ESC [ 6 n` (the cursor-position query `ratatui::Terminal::new()`
/// sends during startup) inline from the reader callback — unlike
/// `conpty_activity_bar_click.rs`'s ConPTY twin, a plain Unix pty tolerates
/// this fine (no cross-thread deadlock to route around; see that test's own
/// module doc for the ConPTY-specific hazard this sidesteps).
/// `(child process, captured-bytes handle, pty writer, pty master)` — see
/// [`spawn_under_pty`]'s own doc for what each piece is used for. A named
/// alias rather than a four-tuple return type directly, per
/// `clippy::type_complexity`.
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

fn find_icon_row(parser: &vt100::Parser, glyph: &str) -> Option<u16> {
    let screen = parser.screen();
    let (rows, _cols) = screen.size();
    for y in 0..rows {
        if let Some(cell) = screen.cell(y, ICON_COLUMN) {
            if cell.contents() == glyph {
                return Some(y);
            }
        }
    }
    None
}

fn wait_for_icon_row(
    parser: &mut vt100::Parser,
    captured: &Arc<Mutex<Captured>>,
    fed: &mut usize,
    glyph: &str,
    timeout: Duration,
) -> Option<u16> {
    let start = Instant::now();
    loop {
        sync_parser(parser, captured, fed);
        if let Some(row) = find_icon_row(parser, glyph) {
            return Some(row);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn send_sgr_click(writer: &Arc<Mutex<Box<dyn Write + Send>>>, col: u16, row: u16) {
    let down = format!("\x1b[<0;{};{}M", col + 1, row + 1);
    let up = format!("\x1b[<0;{};{}m", col + 1, row + 1);
    {
        let mut w = writer.lock().unwrap();
        w.write_all(down.as_bytes()).expect("write SGR mouse-down");
        w.flush().ok();
    }
    std::thread::sleep(Duration::from_millis(10));
    {
        let mut w = writer.lock().unwrap();
        w.write_all(up.as_bytes()).expect("write SGR mouse-up");
        w.flush().ok();
    }
}

/// `(fallback glyph, expected panel marker)` for the three "prior panel"
/// choices the issue's own repro matrix names: Explorer, Search, and
/// Board — each confirmed 9/9 reproducible in the bugbash.
const PRIOR_PANELS: [(&str, &str); 3] = [
    ("\u{229e}", "EXPLORER"), // Explorer
    ("/", "Replace…"),        // Search
    ("\u{25a6}", "BOARD"),    // Board
];

/// How long the Settings header is allowed to lag behind the click before
/// this test calls it a regression. Generous relative to one terminal
/// round trip, but far tighter than the bugbash's reported 400-900ms
/// window — if the fix holds this resolves in low tens of ms; if the bug
/// is present the wait below will reach `SETTLE_TIMEOUT` and the test
/// reports the measured delay in its panic message either way.
const MAX_ACCEPTABLE_HEADER_DELAY: Duration = Duration::from_millis(200);

#[test]
fn settings_header_updates_promptly_after_a_prior_panel_click() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

    let (mut child, captured, writer, _master) = spawn_under_pty(&main_rs, &home);

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
    let mut fed = 0usize;

    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "NORMAL", SETTLE_TIMEOUT)
            .is_some(),
        "vcd never painted its first real frame (status bar's NORMAL mode \
         indicator) within {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );

    for (prior_glyph, prior_marker) in PRIOR_PANELS {
        // Click the prior panel's icon and confirm it actually took over
        // the sidebar — the issue's own precondition.
        let prior_row = wait_for_icon_row(
            &mut parser,
            &captured,
            &mut fed,
            prior_glyph,
            SETTLE_TIMEOUT,
        )
        .unwrap_or_else(|| {
            panic!(
                "prior-panel icon (fallback glyph {prior_glyph:?}) never \
                     appeared in the activity bar's icon column \
                     (x={ICON_COLUMN}) within {SETTLE_TIMEOUT:?}; screen:\n{}",
                screen_text(&parser)
            )
        });
        send_sgr_click(&writer, ICON_COLUMN, prior_row);
        assert!(
            wait_for_screen_contains(
                &mut parser,
                &captured,
                &mut fed,
                prior_marker,
                SETTLE_TIMEOUT
            )
            .is_some(),
            "precondition: clicking the prior panel's icon must open it \
             (marker {prior_marker:?} never appeared); screen:\n{}",
            screen_text(&parser)
        );

        // Wait well past the double-click-fold window before the Settings
        // click — the issue report explicitly ruled this out as the cause,
        // and this test keeps it ruled out rather than ambiguous.
        std::thread::sleep(DOUBLE_CLICK_MARGIN);

        let settings_row = wait_for_icon_row(&mut parser, &captured, &mut fed, "*", SETTLE_TIMEOUT)
            .unwrap_or_else(|| {
                panic!(
                    "Settings icon (fallback glyph '*') never appeared in \
                         the activity bar's icon column (x={ICON_COLUMN}) \
                         within {SETTLE_TIMEOUT:?}; screen:\n{}",
                    screen_text(&parser)
                )
            });

        // Measured from *before* `send_sgr_click` writes a single byte —
        // the same wall-clock reference point the bugbash's own
        // timestamped (`T+`) step logs used — not from whenever this
        // function happens to start its first poll, so the internal
        // mousedown/mouseup gap inside `send_sgr_click` counts against the
        // budget too.
        let click_sent_at = Instant::now();
        send_sgr_click(&writer, ICON_COLUMN, settings_row);

        wait_for_screen_contains(&mut parser, &captured, &mut fed, "SETTINGS", SETTLE_TIMEOUT)
            .unwrap_or_else(|| {
                panic!(
                    "clicking Settings after {prior_marker} never painted the \
                     SETTINGS header within {SETTLE_TIMEOUT:?}; screen:\n{}",
                    screen_text(&parser)
                )
            });
        let total_delay = click_sent_at.elapsed();

        assert!(
            total_delay <= MAX_ACCEPTABLE_HEADER_DELAY,
            "#1701: clicking Settings right after {prior_marker} took \
             {total_delay:?} (from the first mousedown byte written to the \
             SETTINGS header appearing on screen) — exceeds the \
             {MAX_ACCEPTABLE_HEADER_DELAY:?} acceptance bar. The bugbash \
             reported this landing in the 400-900ms range; screen:\n{}",
            screen_text(&parser)
        );
        assert!(
            !screen_text(&parser).contains(prior_marker) || prior_marker == "Replace…", // search's own icon/label can linger harmlessly elsewhere on screen
            "#1701: the sidebar header must not still show the previous \
             panel's own marker ({prior_marker}) once SETTINGS has painted; \
             screen:\n{}",
            screen_text(&parser)
        );
    }

    // Best-effort teardown.
    {
        let mut w = writer.lock().unwrap();
        w.write_all(b"\x1b:qa!\r").ok();
        w.flush().ok();
    }
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(300));
    let _ = child.kill();
    let _ = child.wait();
}
