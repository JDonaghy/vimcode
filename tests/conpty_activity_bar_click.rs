//! #1636: real-ConPTY regression test for "Windows TUI: clicking
//! activity-bar icons does nothing".
//!
//! # Why this file, and why it only runs on Windows
//!
//! The existing in-process coverage,
//! `src/tui_main/app_on_tui_tests.rs`'s
//! `driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app`,
//! drives `App`/`Engine` through a `quadraui::tui::testing::TuiDriver` —
//! which injects already-decoded `UiEvent::MouseDown`/`Up` values directly
//! into the dispatch pipeline. That test passes on unfixed `develop` and
//! still passed while #1636 was reported live on `dell64`, because it can
//! only ever exercise the *dispatch* half of the pipeline (chrome-zone hit
//! test, `App::try_route_sidebar_mouse_event`) — never the *input-decoding*
//! half (raw bytes off a real terminal -> `crossterm::event::MouseEvent` ->
//! `UiEvent`), which is exactly where #1636's own issue body points
//! ("input coordinates or glyph width, not panel dispatch").
//!
//! `quadraui::tui::vt_testing::TuiVtDriver` (issue #555/#1060) is the
//! closest existing quadraui driver to a byte-level observer, but its own
//! module doc says outright it is "scoped to output fidelity only" and
//! explicitly defers real terminal-*input* decoding (raw-mode, SGR mouse)
//! to a real-pty tier quadraui does not yet ship for a downstream consumer
//! to reuse (quadraui#302 — see this repo's `CLAUDE.md` SMOKE_TESTS section,
//! which names that exact gap). Proving anything about how a real terminal's
//! raw mouse-click bytes decode on Windows needs the real thing: a genuine
//! Win32 pseudo console (`CreatePseudoConsole` and friends), wrapped by
//! `portable_pty`'s Windows backend the same way
//! `tests/conpty_idle_flicker.rs` (#1634) already does — see that file's own
//! module doc for the full "why ConPTY, why `portable_pty`, why this only
//! ever runs on real Windows" rationale, which applies here unchanged. This
//! file borrows that file's spawn/capture/quiescence helpers (duplicated
//! locally rather than factored into a shared `tests/` module, to avoid
//! touching #1634's already-landed, independently-reasoned-about file for
//! an unrelated issue).
//!
//! # What this test actually proves
//!
//! It spawns `vcd.exe` under a real ConPTY, waits for the startup paint to
//! settle, then for each activity-bar icon in turn:
//!
//! 1. Parses the *real* ANSI byte stream captured so far with the `vt100`
//!    crate (the same crate `quadraui::tui::vt_testing::TuiVtDriver` uses)
//!    to build a screen model, and locates that icon's fallback glyph at
//!    the activity bar's icon column (`x = 1` — see the column-derivation
//!    comment on [`ICON_COLUMN`] below) — never a hardcoded row, so this
//!    does not encode this issue's own "glyph width" suspicion into the
//!    test and self-invalidate it.
//! 2. Writes a **real SGR mouse-click escape sequence**
//!    (`ESC [ < 0 ; col ; row M` / `...m`) straight into the ConPTY's
//!    input side — exactly the bytes a real terminal emulator (Windows
//!    Terminal included; ConPTY's whole purpose is bidirectional VT
//!    translation, so this is not a Linux-only protocol) sends for a
//!    left-click, at the exact column/row the glyph actually rendered at.
//! 3. Waits for the byte stream to settle again, then re-parses it and
//!    asserts the expected panel's own content marker is now visible.
//!
//! This exercises the *entire* real path #1636 names as suspect —
//! crossterm's Windows console-mode mouse decoding, ConPTY's VT-to-native
//! input translation, and `App`'s dispatch — with nothing stubbed out. It
//! is the regression guard the issue's acceptance criteria asks for; actual
//! Windows Terminal confirmation on `dell64` remains a separate, explicit
//! sign-off this file cannot substitute for (see this repo's `CLAUDE.md`
//! "Testing (CRITICAL)" section on real-hardware verification for `tests/
//! conpty_idle_flicker.rs`, which states the same limitation for its own
//! ConPTY coverage).
#![cfg(windows)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// How long to wait, with no new bytes arriving, before considering the
/// session "settled" — mirrors `tests/conpty_idle_flicker.rs`'s constant of
/// the same name/value.
const QUIET_FOR: Duration = Duration::from_millis(500);

/// Upper bound on how long settling itself may take before this test gives
/// up and fails outright.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);

/// The pty grid this test opens `vcd.exe` on. Tall/wide enough to fit the
/// whole activity bar plus a status row with margin.
const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 120;

/// The activity bar's icon column, 0-indexed in cell units.
///
/// Derived, not guessed: `quadraui::shell::ShellConfig::activity_bar_width`
/// defaults to `3.0` (a line-height multiple), and vimcode's own
/// `render::UnitProfile::cell()` (what the TUI backend uses — see
/// `src/render.rs`'s `activity_bar_width_px: None` doc, "`None` on TUI,
/// which leaves [...] own default [...] in charge") never overrides it, so
/// the TUI activity bar is always exactly 3 cells wide: an accent column
/// (`x = 0`, only painted for the active item), a separator column
/// (`x = area.width - 1 = 2`), and everything else — here, exactly one
/// column — for the icon glyph itself
/// (`quadraui::tui::activity_bar::draw_activity_bar_with_style`: `content_start
/// = area.x + 1`, `content_w = content_end - content_start = 1`, which takes
/// the single-cell `set_cell` branch, never the wide-glyph `set_cell_wide`
/// one — the bar is never wide enough for that branch to fire in this
/// app's own configuration, independent of whether any given icon glyph is
/// itself wide). The activity bar is also always the screen's leftmost
/// element (`area.x == 0`), so this column is an absolute screen
/// coordinate, not just bar-relative.
const ICON_COLUMN: u16 = 1;

/// Shared state the reader thread updates: every byte it has ever seen, and
/// the instant of the most recent read. Mirrors `tests/conpty_idle_flicker.
/// rs`'s identical struct.
struct Captured {
    bytes: Vec<u8>,
    last_read_at: Instant,
}

/// Build an isolated `%APPDATA%`/`%USERPROFILE%` for the spawned `vcd.exe`,
/// seeded with settings that make this test deterministic: no LSP spawn
/// (same reasoning as `conpty_idle_flicker.rs`'s `isolated_home`), and nerd
/// fonts explicitly off so the activity-bar icons paint their plain-ASCII
/// fallback glyphs (`Icon::fallback`) — the value this test's column scan
/// searches for — regardless of whatever `default_use_nerd_fonts` resolves
/// to on whatever Windows host actually runs this.
fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_conpty_1636_{}_{}",
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

/// Spawn `vcd.exe` under a real ConPTY, opened on `main_rs`, with `cwd` as
/// its working directory. Returns the child, the `Captured` handle the
/// background reader thread fills in, and the pty writer used to send
/// keystrokes/mouse reports.
fn spawn_under_conpty(
    main_rs: &PathBuf,
    home: &PathBuf,
) -> (
    Box<dyn portable_pty::Child + Send + Sync>,
    Arc<Mutex<Captured>>,
    Box<dyn Write + Send>,
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
    let writer = pair.master.take_writer().expect("take ConPTY writer");

    let captured = Arc::new(Mutex::new(Captured {
        bytes: Vec::new(),
        last_read_at: Instant::now(),
    }));
    let captured_for_thread = Arc::clone(&captured);
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut c = captured_for_thread.lock().unwrap();
                    c.bytes.extend_from_slice(&buf[..n]);
                    c.last_read_at = Instant::now();
                }
                Err(_) => break,
            }
        }
    });

    (child, captured, writer, pair.master)
}

/// Block until no new bytes have arrived for `quiet_for`, or fail (return
/// `false`) after `timeout` total. Mirrors `conpty_idle_flicker.rs`.
fn wait_for_quiescence(
    captured: &Arc<Mutex<Captured>>,
    quiet_for: Duration,
    timeout: Duration,
) -> bool {
    let start = Instant::now();
    loop {
        let quiet_elapsed = captured.lock().unwrap().last_read_at.elapsed();
        if quiet_elapsed >= quiet_for {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Render the tail of `bytes` as a debuggable string — printable ASCII
/// as-is, everything else as `\xHH`. Mirrors `conpty_idle_flicker.rs`.
fn escape_for_display(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            0x20..=0x7e => out.push(b as char),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

fn tail_snapshot(captured: &Arc<Mutex<Captured>>, n: usize) -> Vec<u8> {
    let c = captured.lock().unwrap();
    let take = c.bytes.len().min(n);
    c.bytes[c.bytes.len() - take..].to_vec()
}

/// Feed every byte captured so far (from `*fed` onward) into `parser`,
/// advancing `*fed` to the new total. Keeping one persistent `vt100::Parser`
/// across the whole test (rather than reparsing from byte 0 each time) is
/// just an efficiency choice — `vt100::Parser::process` is a pure state
/// transition over whatever bytes it is handed, so feeding it incrementally
/// and feeding it the same bytes in one shot converge on the same screen.
fn sync_parser(parser: &mut vt100::Parser, captured: &Arc<Mutex<Captured>>, fed: &mut usize) {
    let new_bytes = {
        let c = captured.lock().unwrap();
        c.bytes[*fed..].to_vec()
    };
    *fed += new_bytes.len();
    parser.process(&new_bytes);
}

/// Plain-text dump of the current vt100-observed screen, one line per row —
/// what a real terminal emulator would show right now. Used only for
/// assertion failure messages and for the "does the expected panel marker
/// appear anywhere" checks (unlike the icon-column scan below, panel
/// content can land at any column).
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

/// Scan the activity bar's icon column (`ICON_COLUMN`) top to bottom for a
/// cell whose contents exactly match `glyph`, returning its row. This is
/// the "locate the icon exactly where it actually rendered" step — no row
/// is ever hardcoded, so a real layout shift (e.g. the menu bar toggling
/// on) cannot make this test pass vacuously against the wrong row.
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

/// Write a real SGR mouse-click sequence (`CSI < Cb ; Cx ; Cy M` for the
/// press, lowercase `m` for the release) straight into the ConPTY's input
/// side, at 1-indexed `(col, row)` — the xterm SGR mouse-tracking protocol
/// (button 0 = left, no modifiers, no drag), exactly what a real terminal
/// forwards for a plain left-click. `Cb=0` is `MouseEventKind::Down(Left)`;
/// the same triple with a lowercase terminator is `Up(Left)`.
fn send_sgr_click(writer: &mut dyn Write, col: u16, row: u16) {
    let down = format!("\x1b[<0;{};{}M", col + 1, row + 1);
    let up = format!("\x1b[<0;{};{}m", col + 1, row + 1);
    writer
        .write_all(down.as_bytes())
        .expect("write SGR mouse-down");
    writer.flush().ok();
    std::thread::sleep(Duration::from_millis(30));
    writer.write_all(up.as_bytes()).expect("write SGR mouse-up");
    writer.flush().ok();
}

#[test]
fn click_on_each_activity_bar_icon_over_real_conpty_opens_its_panel() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");
    // Top-level sibling of `main.rs` — Explorer's default root listing shows
    // it with no expand/reveal action needed, unlike a nested path.
    let marker_name = "zqxw1636_marker.txt";
    std::fs::write(home.join(marker_name), "marker").expect("write marker file");

    let (mut child, captured, mut writer, _master) = spawn_under_conpty(&main_rs, &home);

    assert!(
        wait_for_quiescence(&captured, QUIET_FOR, SETTLE_TIMEOUT),
        "vcd.exe never went quiet after startup — output kept arriving \
         continuously for {SETTLE_TIMEOUT:?}; last bytes:\n{}",
        escape_for_display(&tail_snapshot(&captured, 400))
    );

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
    let mut fed = 0usize;
    sync_parser(&mut parser, &captured, &mut fed);

    assert!(
        screen_text(&parser).contains(marker_name),
        "precondition: Explorer is the default active panel, so the marker \
         file should already be listed at startup; screen:\n{}",
        screen_text(&parser)
    );

    // `(fallback glyph, expected panel marker, human label)` — fallback
    // glyphs match `src/icons.rs`'s `Icon::fallback` for each activity-bar
    // item (nerd fonts forced off via `isolated_home`'s settings.json), and
    // panel markers mirror the in-process shell-app test's own
    // `click_icon_and_expect` table
    // (`app_on_tui_tests.rs::activity_bar::driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app`).
    let sequence: [(&str, &str, &str); 4] = [
        ("/", "Replace…", "Search"),
        ("Y", "SOURCE CONTROL", "Source Control"),
        ("#", "EXTENSIONS", "Extensions"),
        ("*", "SETTINGS", "Settings"),
    ];

    for (glyph, marker, label) in sequence {
        sync_parser(&mut parser, &captured, &mut fed);
        let row = find_icon_row(&parser, glyph).unwrap_or_else(|| {
            panic!(
                "{label} icon (fallback glyph {glyph:?}) not found in the \
                 activity bar's icon column (x={ICON_COLUMN}); screen:\n{}",
                screen_text(&parser)
            )
        });

        send_sgr_click(&mut *writer, ICON_COLUMN, row);

        assert!(
            wait_for_quiescence(&captured, QUIET_FOR, SETTLE_TIMEOUT),
            "vcd.exe never went quiet after clicking {label}; last bytes:\n{}",
            escape_for_display(&tail_snapshot(&captured, 400))
        );
        sync_parser(&mut parser, &captured, &mut fed);

        assert!(
            screen_text(&parser).contains(marker),
            "clicking the {label} icon (real SGR mouse click over ConPTY at \
             column {ICON_COLUMN}, row {row}) must open its panel — marker \
             {marker:?} missing; this is #1636's exact symptom (\"harness \
             test passes; real-terminal path broken\"); screen:\n{}",
            screen_text(&parser)
        );
    }

    // Explorer last, same reasoning as the in-process mirror test: nothing
    // else needs clicking afterward.
    sync_parser(&mut parser, &captured, &mut fed);
    let explorer_row = find_icon_row(&parser, "\u{229e}").unwrap_or_else(|| {
        panic!(
            "Explorer icon (fallback glyph U+229E) not found in the activity \
             bar's icon column (x={ICON_COLUMN}); screen:\n{}",
            screen_text(&parser)
        )
    });
    send_sgr_click(&mut *writer, ICON_COLUMN, explorer_row);
    assert!(
        wait_for_quiescence(&captured, QUIET_FOR, SETTLE_TIMEOUT),
        "vcd.exe never went quiet after clicking Explorer; last bytes:\n{}",
        escape_for_display(&tail_snapshot(&captured, 400))
    );
    sync_parser(&mut parser, &captured, &mut fed);
    assert!(
        screen_text(&parser).contains(marker_name),
        "clicking the Explorer icon (real SGR mouse click over ConPTY) must \
         re-open the file tree; marker {marker_name:?} missing; screen:\n{}",
        screen_text(&parser)
    );

    // Best-effort teardown.
    writer.write_all(b"\x1b:qa!\r").ok();
    writer.flush().ok();
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();
}
