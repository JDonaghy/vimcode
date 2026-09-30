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
//! input translation, and `App`'s dispatch — with nothing stubbed out.
//!
//! # RED/GREEN, actually confirmed on real Windows hardware (`dell64`)
//!
//! Unlike this file's first version, this one **was** built with `cargo
//! xwin` and run to completion — RED and then GREEN — against a real
//! Win32 ConPTY on `dell64`'s attached Windows 11 host (reached directly;
//! no interop layer sits between this test binary and the `vcd.exe` child
//! it spawns — both are ordinary native Windows processes talking over a
//! native `CreatePseudoConsole` pty, confirmed via `tasklist`/`Get-Process`
//! while a run was in flight). Four real, reproducible findings came out
//! of getting it there, three of them purely in *this test's own harness
//! code* rather than anything `vcd.exe` does:
//!
//! 1. **RED, unfixed test:** with no responder for the cursor-position
//!    query (`ESC [ 6 n`) `ratatui::Terminal::new()` sends and blocks on
//!    during startup, `vcd.exe` never proceeds past it — the very first
//!    precondition check failed against a blank screen. This is a gap in
//!    the test (see [`spawn_under_conpty`]'s doc), not evidence of
//!    anything wrong in `vcd.exe` or `quadraui`.
//! 2. **A genuine ConPTY-specific deadlock**, found while fixing (1):
//!    answering the query *synchronously, inline, from within the reader
//!    thread's read callback* (the same shape quadraui's own
//!    `tui_pty_smoke.rs` uses for a plain Unix pty, with no issue there)
//!    wedges the whole ConPTY session solid on real Windows hardware —
//!    confirmed via `Get-Process ... | Select Threads`: every thread in
//!    both processes sits in `Wait` state, zero CPU, indefinitely. Moving
//!    the reply to an independently spawned thread (see
//!    [`spawn_under_conpty`]'s doc) removes the deadlock with no other
//!    behaviour change. This is a real Windows-ConPTY input/output-racing
//!    hazard worth quadraui knowing about if it ever ships its own
//!    Windows real-pty test tier (the quadraui#302 gap this file's first
//!    version pointed at) — filing that as a quadraui issue is follow-up
//!    work, not blocking this one.
//! 3. **The sidebar is not open by default** when `vcd.exe` is opened
//!    directly on a *file* (as this test does) — confirmed by hand, the
//!    first real frame paints the editor and activity bar with an empty
//!    sidebar column. This test's first version assumed Explorer was
//!    already the active panel (mirroring the in-process
//!    `driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app`
//!    fixture's *shadow-engine-only* default, which that fixture's own
//!    comment already flags as not shared by the real runner chrome) and
//!    failed on that false precondition. Fixed by leading the click
//!    sequence with Explorer itself, exercising and asserting on its own
//!    click exactly like every other icon, rather than assuming it.
//! 4. **Back-to-back synthetic clicks fold into a `DoubleClick`** on real
//!    hardware, exactly the class `quadraui::tui::backend::TuiBackend`'s
//!    `DoubleClickDetector` (400ms/1.5-cell window) already causes for the
//!    in-process mirror fixture (that fixture works around it with a
//!    test-only `set_double_click_folding(false)` hook with no real-
//!    terminal equivalent — quadraui#1432). Two of this test's clicks
//!    landing inside that window silently failed the second one's
//!    assertion — a real, on-real-ConPTY reproduction of the #1432 class,
//!    not a new bug. Fixed with an explicit [`DOUBLE_CLICK_MARGIN`] before
//!    every synthetic click.
//!
//! None of the above required a change to `src/` or to `quadraui`. With
//! all four fixed, this test passed **three consecutive real-hardware
//! runs** (`dell64`, real ConPTY) — clicking Explorer, Search, Source
//! Control, Extensions, and Settings each correctly switched the active
//! panel, decoded from genuine SGR mouse bytes through crossterm's real
//! Windows console-mode path, ConPTY's VT-to-native translation, and
//! `App::try_route_sidebar_mouse_event` dispatch, with nothing stubbed.
//!
//! # What this means for #1636 itself
//!
//! **This is evidence the reported symptom does not reproduce when clicks
//! are spaced apart**, not a confirmed root cause for what the operator
//! saw on 2026-09-29. The most plausible innocent explanation this
//! investigation turned up is finding (4) above: a real user clicking
//! through several activity-bar icons in quick succession — exactly what
//! "just trying each icon to see what happens" looks like — can hit the
//! same double-click fold this test had to route around, and quadraui's
//! `DoubleClickDetector` has no "activate panel" handler for a
//! double-click on a plain activity-bar zone (quadraui#1432). That is an
//! *already-tracked* class, not a new one this PR discovered from
//! scratch — but this is the first confirmation it reproduces on a real
//! terminal (ConPTY) rather than only the in-process `TestBackend`
//! harness quadraui#1432 was filed against.
//!
//! **#1636 must stay open.** No production code changed in this PR — the
//! click-dispatch path (input decoding through `App` dispatch) is
//! confirmed working correctly end-to-end on real Windows hardware, which
//! rules out a broken-dispatch explanation, but does not confirm what the
//! operator actually experienced. Closing #1636 off the back of this PR
//! would repeat the exact "issue closed, bug still there" failure mode
//! this repo's own `CLAUDE.md` testing section calls out from the
//! v0.11.0 `KNOWN_BUGS` incident. The concrete next step is operator
//! re-confirmation — specifically, whether the icons that "did nothing"
//! were clicked in quick succession — and, if so, treating #1636 as
//! resolved by whatever fixes quadraui#1432 (or linking the two) rather
//! than by anything in this PR.
#![cfg(windows)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Upper bound on how long this test will wait for any single expected
/// screen change (startup paint, or a panel switching after a click)
/// before giving up and failing outright. Generous on purpose: unlike
/// `tests/conpty_idle_flicker.rs`'s fixed "quiet for 500ms" settle window
/// (which this file used to mirror — see the module doc's RED/GREEN
/// section for why that shape doesn't fit here), a cold real-hardware
/// startup can legitimately *pause* mid-negotiation for longer than any
/// fixed quiet window before its first real content paint arrives (walking
/// the isolated home directory for Explorer's listing, loading tree-sitter
/// grammars, …) — a fixed "no bytes for 500ms ⇒ done" heuristic reads that
/// pause as completion and checks the screen before the real paint has
/// happened, RED-verified by hand against this exact file (see the module
/// doc).
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

/// `true` if `haystack` contains `needle` anywhere as a contiguous run of
/// bytes. Mirrors quadraui's own `tui_pty_smoke.rs` helper of the same
/// name/shape (that file is not `pub`, and lives in a different crate, so
/// this is a small, deliberate duplication rather than a new dependency).
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Spawn `vcd.exe` under a real ConPTY, opened on `main_rs`, with `cwd` as
/// its working directory. Returns the child, the `Captured` handle the
/// background reader thread fills in, and the pty writer used to send
/// keystrokes/mouse reports.
///
/// # Why the reader thread also answers `ESC [ 6 n`
///
/// The PTY *master* side plays the role a real terminal emulator plays for
/// a normal interactive session — including answering the escape-sequence
/// queries a real terminal answers. `ratatui`'s `Terminal::new()` (via
/// `quadraui::tui::run::setup_terminal`, which every vimcode TUI entry
/// point goes through) queries the cursor position (`ESC [ 6 n`, expects
/// `ESC [ row ; col R` back) during startup and blocks on the reply; a
/// dumb byte-in/byte-out pty with nothing on the master side to answer
/// that query leaves `vcd.exe` hung before it ever paints a frame — this
/// was RED-verified by hand against this exact file before this responder
/// existed (see the module doc's "RED/GREEN" section). This mirrors
/// quadraui's own `quadraui/tests/tui_pty_smoke.rs`'s `PtyExample::spawn`
/// (its struct doc names the identical Ratatui/crossterm behaviour) —
/// that file cannot be imported (private to quadraui's own crate), so the
/// same minimal responder is reproduced here rather than pulled in as a
/// new dependency.
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

    let captured = Arc::new(Mutex::new(Captured {
        bytes: Vec::new(),
        last_read_at: Instant::now(),
    }));
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
                    c.last_read_at = Instant::now();
                    drop(c);
                    // Answer `ESC [ 6 n` (cursor position report) the way a
                    // real terminal would — see this function's own doc.
                    // The exact row/col this test claims back is never
                    // observed by anything downstream (`vcd.exe` always
                    // enters the alternate screen and repositions its
                    // cursor with absolute moves for every subsequent
                    // paint), so a fixed `1;1` is sufficient — quadraui's
                    // own `tui_pty_smoke.rs` computes a live answer off its
                    // parsed screen instead, but only because some of its
                    // scenarios use the *inline* (non-alternate-screen)
                    // viewport, where the initial answer does matter.
                    if contains_subslice(chunk, b"\x1b[6n") {
                        // Reply from a freshly spawned thread, never inline
                        // in this read loop: a real, reproducible ConPTY
                        // deadlock (RED-verified by hand — see this
                        // function's doc) — writing the reply synchronously
                        // from *within* the callback that just read the
                        // query wedges the whole ConPTY session on real
                        // Windows hardware (`dell64`). Unlike a plain Unix
                        // pty (where quadraui's own `tui_pty_smoke.rs`
                        // answers inline with no issue), ConPTY appears not
                        // to tolerate an input write raced against its own
                        // in-flight output delivery on the same handle
                        // pair. Handing the write to an independent thread
                        // removes that race with no other behaviour change.
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

/// Poll the captured byte stream, re-parsing it into `parser` each time,
/// until [`screen_text`] contains `needle` — or fail (return `false`)
/// after `timeout` total.
///
/// This is the direct-content-polling replacement for the
/// `conpty_idle_flicker.rs`-style "wait until no bytes arrive for a fixed
/// quiet window, then check once" pattern this file used before: RED-
/// verified by hand, on real `dell64` hardware, that the fixed-quiet-window
/// version fails the very first (startup) assertion even on an otherwise
/// working build — the negotiation burst (raw-mode/mouse-capture/title)
/// goes quiet for well over 500ms *before* Explorer's real directory
/// listing paints, so "quiet ⇒ done" checks the screen too early and never
/// retries. Polling for the actual expected content sidesteps the whole
/// "how long is quiet enough" question — it succeeds the moment the real
/// paint lands, however long that takes, and still fails within a bounded
/// `timeout` if it never does.
fn wait_for_screen_contains(
    parser: &mut vt100::Parser,
    captured: &Arc<Mutex<Captured>>,
    fed: &mut usize,
    needle: &str,
    timeout: Duration,
) -> bool {
    let start = Instant::now();
    loop {
        sync_parser(parser, captured, fed);
        if screen_text(parser).contains(needle) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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

/// [`find_icon_row`], polled: re-parses the captured stream and retries
/// until the glyph is found, or `timeout` elapses. A single one-shot
/// [`find_icon_row`] call can transiently miss a real activity bar that is
/// still mid-slide-in (the sidebar toggling open shifts the icon column's
/// neighbouring content but not, confirmed by hand, the icon column itself
/// — see [`ICON_COLUMN`]'s own derivation comment for why the icon column
/// is fixed) for the handful of frames right after `vcd.exe`'s first paint
/// — RED-verified by hand against this exact file (see the module doc).
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
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Two clicks land closer together than this must fold into a single
/// `DoubleClick` — `quadraui::tui::backend::TuiBackend`'s
/// `DoubleClickDetector` window is documented as 400ms/1.5 cells
/// (`quadraui/src/tui/testing.rs`'s doc comments on the same detector), and
/// a double-click on a plain activity-bar icon zone has no "activate
/// panel" handler (quadraui#1432 — see the in-process mirror fixture's own
/// `set_double_click_folding(false)` comment for the identical failure
/// mode there). A real terminal session has no such test-only override, so
/// [`send_sgr_click`] pays this margin unconditionally instead — RED-
/// verified by hand against this exact file: back-to-back clicks with no
/// gap silently failed the very next icon's assertion, on real ConPTY,
/// `dell64` hardware (see the module doc).
const DOUBLE_CLICK_MARGIN: Duration = Duration::from_millis(450);

/// Write a real SGR mouse-click sequence (`CSI < Cb ; Cx ; Cy M` for the
/// press, lowercase `m` for the release) straight into the ConPTY's input
/// side, at 1-indexed `(col, row)` — the xterm SGR mouse-tracking protocol
/// (button 0 = left, no modifiers, no drag), exactly what a real terminal
/// forwards for a plain left-click. `Cb=0` is `MouseEventKind::Down(Left)`;
/// the same triple with a lowercase terminator is `Up(Left)`. Waits
/// [`DOUBLE_CLICK_MARGIN`] *before* sending — see that constant's doc.
fn send_sgr_click(writer: &Arc<Mutex<Box<dyn Write + Send>>>, col: u16, row: u16) {
    std::thread::sleep(DOUBLE_CLICK_MARGIN);
    let down = format!("\x1b[<0;{};{}M", col + 1, row + 1);
    let up = format!("\x1b[<0;{};{}m", col + 1, row + 1);
    {
        let mut w = writer.lock().unwrap();
        w.write_all(down.as_bytes()).expect("write SGR mouse-down");
        w.flush().ok();
    }
    std::thread::sleep(Duration::from_millis(30));
    {
        let mut w = writer.lock().unwrap();
        w.write_all(up.as_bytes()).expect("write SGR mouse-up");
        w.flush().ok();
    }
}

#[test]
fn click_on_each_activity_bar_icon_over_real_conpty_opens_its_panel() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

    let (mut child, captured, writer, _master) = spawn_under_conpty(&main_rs, &home);

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
    let mut fed = 0usize;

    // Wait for the first real frame — the status bar's mode indicator,
    // painted only once the whole chrome (activity bar, editor, status
    // line) has rendered at least once. `vcd.exe` starts with the sidebar
    // *collapsed* when opened directly on a file (confirmed by hand,
    // real ConPTY, `dell64`: the very first frame paints the editor and
    // activity bar but an empty sidebar column — this test used to assume
    // Explorer was the default active panel, mirroring the in-process
    // `driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app`
    // fixture's own *shadow-engine* default; that fixture's own comment
    // already flags this as a shadow-engine-only default that the real
    // runner chrome does not share — RED-verified by hand here), so the
    // Explorer click right below is this test's first real assertion, not
    // a precondition check.
    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "NORMAL", SETTLE_TIMEOUT),
        "vcd.exe never painted its first real frame (status bar's NORMAL \
         mode indicator) within {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );

    // `(fallback glyph, expected panel marker, human label)` — fallback
    // glyphs match `src/icons.rs`'s `Icon::fallback` for each activity-bar
    // item (nerd fonts forced off via `isolated_home`'s settings.json), and
    // panel markers mirror the in-process shell-app test's own
    // `click_icon_and_expect` table
    // (`app_on_tui_tests.rs::activity_bar::driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app`).
    // Explorer leads the sequence (rather than being assumed as an
    // already-showing default, per the comment above) so clicking it is
    // itself exercised and asserted on, exactly like every other icon.
    let sequence: [(&str, &str, &str); 5] = [
        ("\u{229e}", "EXPLORER", "Explorer"),
        ("/", "Replace…", "Search"),
        ("Y", "SOURCE CONTROL", "Source Control"),
        ("#", "EXTENSIONS", "Extensions"),
        ("*", "SETTINGS", "Settings"),
    ];

    for (glyph, marker, label) in sequence {
        let row = wait_for_icon_row(&mut parser, &captured, &mut fed, glyph, SETTLE_TIMEOUT)
            .unwrap_or_else(|| {
                panic!(
                    "{label} icon (fallback glyph {glyph:?}) never appeared in \
                     the activity bar's icon column (x={ICON_COLUMN}) within \
                     {SETTLE_TIMEOUT:?}; screen:\n{}",
                    screen_text(&parser)
                )
            });

        send_sgr_click(&writer, ICON_COLUMN, row);

        assert!(
            wait_for_screen_contains(&mut parser, &captured, &mut fed, marker, SETTLE_TIMEOUT),
            "clicking the {label} icon (real SGR mouse click over ConPTY at \
             column {ICON_COLUMN}, row {row}) must open its panel — marker \
             {marker:?} never appeared within {SETTLE_TIMEOUT:?}; this is \
             #1636's exact symptom (\"harness test passes; real-terminal \
             path broken\"); screen:\n{}",
            screen_text(&parser)
        );
    }

    // Click Explorer once more at the end — it was clicked first (above)
    // to open it in the first place, so this both leaves the session in a
    // known state for teardown and re-confirms Explorer's own click still
    // works after every other panel has taken a turn as the active one.
    let explorer_row =
        wait_for_icon_row(&mut parser, &captured, &mut fed, "\u{229e}", SETTLE_TIMEOUT)
            .unwrap_or_else(|| {
                panic!(
                    "Explorer icon (fallback glyph U+229E) never appeared in \
                     the activity bar's icon column (x={ICON_COLUMN}) within \
                     {SETTLE_TIMEOUT:?}; screen:\n{}",
                    screen_text(&parser)
                )
            });
    send_sgr_click(&writer, ICON_COLUMN, explorer_row);
    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "EXPLORER", SETTLE_TIMEOUT),
        "clicking the Explorer icon (real SGR mouse click over ConPTY) must \
         re-open the file tree; the \"EXPLORER\" panel header never \
         reappeared within {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );

    // Best-effort teardown.
    {
        let mut w = writer.lock().unwrap();
        w.write_all(b"\x1b:qa!\r").ok();
        w.flush().ok();
    }
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();
}
