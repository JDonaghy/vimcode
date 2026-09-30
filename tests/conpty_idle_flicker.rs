//! #1634: raw-output ConPTY idle regression test for the Windows TUI ~1Hz
//! foreground-colour flicker that survived #1583.
//!
//! # Why this file, and why it only runs on Windows
//!
//! #1583 added an *in-process* idle-stability test
//! (`src/tui_main/app_on_tui_tests.rs`'s `idle_stability_1583` module) that
//! drives `vcd`'s `App`/`Engine` through a `quadraui::tui::TuiDriver` and
//! asserts the `TestBackend`/vt100 cell buffer stays byte-identical across
//! idle ticks. That test still passes on unfixed `develop` — the operator
//! kept seeing the flicker anyway (#1634) — because it can only see bytes
//! that flow through the `ratatui::Terminal`'s own `Write` sink. Anything a
//! backend writes to `std::io::stdout()` directly, bypassing that sink
//! entirely, is invisible to it: `quadraui::tui::TuiBackend::set_title`
//! (`WindowControl::set_title`) and `TuiBackend::set_caret_shape` both write
//! straight to real stdout with no `Terminal`/`Buffer` involved at all (see
//! their own doc comments in `quadraui/src/tui/backend.rs`). Proving
//! anything about what actually reaches a downstream consumer needs the
//! real thing: a *real* Win32 pseudo console (`CreatePseudoConsole` and
//! friends), wrapped here by `portable_pty`'s Windows backend
//! (`portable_pty::win::conpty`) rather than hand-rolled FFI (see
//! `Cargo.toml`'s `[target.'cfg(windows)'.dev-dependencies]` comment for why
//! that crate is already a safe, zero-new-dependency choice). That only
//! means anything when *this test binary itself* runs as a real Windows
//! process — cross-compiled with `cargo xwin test --target
//! x86_64-pc-windows-msvc` and executed on real Windows (`dell64`'s WSL2
//! interop reaches the attached Windows 11 host directly; CI's
//! `windows-latest` runners execute natively) — so the whole file is
//! `#[cfg(windows)]`-gated (reading the *target* triple, the same as every
//! other `cfg(target_os = "windows")` gate in this repo — see
//! `tests/windows_manifest.rs`'s note on target-vs-host `cfg`) and the dev-
//! dependency above is target-gated the same way, so a plain Linux `cargo
//! test` never resolves or builds any of this.
//!
//! # What running this on real hardware (dell64) actually found
//!
//! `App::tick_dispatch`/`render::run_shared_tick_chores` called both
//! `set_title` and `set_caret_shape` **unconditionally on every idle tick**
//! (`quadraui::runtime::IDLE_POLL_CEILING`, ~250ms), with no "did the value
//! actually change" guard — unlike, say, the `last_colorscheme` check right
//! next to the title write. That is real, confirmed waste (a syscall this
//! app no longer needs to make 4×/second while idle), and this PR adds the
//! missing guard (`App::last_window_title`/`last_caret_shape`).
//!
//! **It is not, however, confirmed to be the flicker's cause.** With the
//! guards reverted (title/caret-shape writes unconditional again, matching
//! pre-#1634 `develop`) *and* with the guards in place, this test's actual
//! ConPTY-output capture was **silent** in both configurations — no bytes
//! arrived during the post-settle observation window either way, on real
//! ConPTY, on real Windows 11 hardware. Per the issue's own diagnostic
//! framework ("if the stream is silent, it's (b)"), that rules out
//! hypothesis (a) — a vimcode-originated redraw/write that fires during
//! this reproduction — and points at (b): whatever produces the visible
//! flicker is not accompanied by any new byte reaching a downstream ConPTY
//! reader, so it cannot be something vimcode's own output stream causes.
//! The most likely explanation is a ConPTY- or terminal-emulator-side
//! repaint of *already-delivered* content (e.g. a periodic buffer resync or
//! compositor repaint), which is invisible to any test that only inspects
//! the byte stream, this one included — closing that half needs a live
//! Windows Terminal window and either a human watching it or a
//! screen-capture harness, neither of which exists in this repo. This test
//! stays in the repo as the permanent regression guard the issue asked for
//! (and it does catch a real class of future bug: a genuine vimcode-side
//! redraw storm reaching the wire, the (a) case), and as the evidence trail
//! for why (a) was ruled out here. `git log`/the issue thread for #1634
//! should be treated as still open on the (b) half.
//!
//! RED/GREEN note for reviewers: this file's two tests were run against
//! `App::last_window_title`/`last_caret_shape` both reverted and restored,
//! on real ConPTY/dell64, and passed identically in both configurations —
//! see the paragraph above for what that does and does not prove. This is
//! *not* the usual "RED before, GREEN after" shape most fixes in this repo
//! ship (CLAUDE.md's black-box coverage rule); it is called out explicitly
//! here rather than silently claimed, per that rule's own "state it, don't
//! assume it" requirement.
#![cfg(windows)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// How long to wait, with no new bytes arriving, before considering the
/// session "settled" (past startup paint, initial syntax highlight, and the
/// splits this test drives).
const QUIET_FOR: Duration = Duration::from_millis(1000);

/// Upper bound on how long settling itself may take before this test gives
/// up and fails outright (a session that never goes quiet is itself a bug,
/// just not the one #1634 is about).
const SETTLE_TIMEOUT: Duration = Duration::from_secs(15);

/// The idle observation window — #1634 asks for "≥5s"; this uses a bit more
/// margin so a scheduler hiccup on a loaded CI runner can't produce a false
/// green.
const OBSERVE_WINDOW: Duration = Duration::from_secs(7);

/// Shared state the reader thread updates: every byte it has ever seen, and
/// the instant of the most recent read.
struct Captured {
    bytes: Vec<u8>,
    last_read_at: Instant,
}

/// Comment-heavy fixture content — mirrors #1583's own repro file (a real
/// server/session is more likely to be idle-stable on a file with plenty of
/// syntax-highlighted comment spans, which is exactly what the operator's
/// original report described flickering).
fn comment_heavy_rust(marker: &str) -> String {
    let mut text = String::new();
    for i in 0..40 {
        text.push_str(&format!(
            "// {marker} this is a documentation comment line {i}\n"
        ));
    }
    text.push_str("fn main() {\n    println!(\"hello\");\n}\n");
    text
}

/// Build an isolated `%APPDATA%`/`%USERPROFILE%` for the spawned `vcd.exe`
/// so it never reads (or writes) the real operator's `settings.json`/
/// session state, and seed `lsp_enabled: false` — this test's whole point
/// is watching the *idle* byte stream, and a real language server spawn
/// (rust-analyzer, if it happens to be on this Windows host's `PATH`) would
/// be exactly the kind of "extra redraw trigger" #1634's own hypothesis (a)
/// describes, and isn't what this test is isolating.
fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_conpty_1634_{}_{}",
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
        r#"{"lsp_enabled": false}"#,
    )
    .expect("seed settings.json");
    home
}

/// Spawn `vcd.exe` under a real ConPTY, opened on `main.rs`. Returns the
/// child, the `Captured` handle the background reader thread fills in, and
/// the pty writer used to send keystrokes.
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
            rows: 40,
            cols: 120,
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
    // Dropping the slave handle (not the pty pair's master) is what lets a
    // real terminal ever see EOF; keeping it alive here matches how a real
    // terminal emulator hosts a ConPTY child.
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
                Ok(0) => break, // EOF: child exited / pty closed
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
/// `false`) after `timeout` total.
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

/// Owned copy of the last `n` captured bytes, for a failure message that
/// outlives the `MutexGuard` it was read under.
fn tail_snapshot(captured: &Arc<Mutex<Captured>>, n: usize) -> Vec<u8> {
    let c = captured.lock().unwrap();
    let take = c.bytes.len().min(n);
    c.bytes[c.bytes.len() - take..].to_vec()
}

/// Owned copy of everything captured from byte offset `start` onward.
fn snapshot_from(captured: &Arc<Mutex<Captured>>, start: usize) -> Vec<u8> {
    let c = captured.lock().unwrap();
    let start = start.min(c.bytes.len());
    c.bytes[start..].to_vec()
}

/// Render the tail of `bytes` as a debuggable string — printable ASCII
/// as-is, everything else as `\xHH` — so a red run's failure message shows
/// exactly which escape sequence kept arriving (#1634's own "decode which
/// rows or cells are rewritten" ask), without dragging in a VT/ANSI parser
/// dependency just for a test's diagnostic output.
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

#[test]
fn idle_after_settle_produces_no_further_output() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    let right_rs = home.join("right.rs");
    std::fs::write(&main_rs, comment_heavy_rust("LEFT")).expect("write main.rs");
    std::fs::write(&right_rs, comment_heavy_rust("RIGHT")).expect("write right.rs");

    let (mut child, captured, mut writer, _master) = spawn_under_conpty(&main_rs, &home);

    // Let the initial paint (startup, first syntax highlight pass) settle
    // before driving the layout.
    assert!(
        wait_for_quiescence(&captured, QUIET_FOR, SETTLE_TIMEOUT),
        "vcd.exe never went quiet after startup — output kept arriving \
         continuously for {SETTLE_TIMEOUT:?}; last bytes:\n{}",
        escape_for_display(&tail_snapshot(&captured, 400))
    );

    // Reproduce the operator's report (#1634): a tab group on the right,
    // split horizontally into two groups stacked one above the other.
    // `:vsplit` focuses the new (right) window; `:split` then stacks that
    // window into top/bottom.
    writer
        .write_all(format!(":vsplit {}\r", right_rs.display()).as_bytes())
        .expect("send :vsplit");
    writer.flush().ok();
    std::thread::sleep(Duration::from_millis(300));
    writer.write_all(b":split\r").expect("send :split");
    writer.flush().ok();

    assert!(
        wait_for_quiescence(&captured, QUIET_FOR, SETTLE_TIMEOUT),
        "vcd.exe never went quiet after building the stacked-group layout \
         — output kept arriving continuously for {SETTLE_TIMEOUT:?}; last \
         bytes:\n{}",
        escape_for_display(&tail_snapshot(&captured, 400))
    );

    // The actual #1634 assertion: once settled, with no further input, a
    // real ConPTY must forward *nothing* for a sustained window. Record
    // where we are, sleep through the whole observation window, then check
    // whether anything at all arrived during it.
    let (byte_count_before, last_read_before) = {
        let c = captured.lock().unwrap();
        (c.bytes.len(), c.last_read_at)
    };
    std::thread::sleep(OBSERVE_WINDOW);
    let (byte_count_after, last_read_after) = {
        let c = captured.lock().unwrap();
        (c.bytes.len(), c.last_read_at)
    };

    // Best-effort teardown regardless of the assertion outcome below.
    writer.write_all(b"\x1b:qa!\r").ok(); // Escape (cancel any pending op) + force-quit-all
    writer.flush().ok();
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        last_read_before,
        last_read_after,
        "raw ConPTY output kept arriving during a {OBSERVE_WINDOW:?} idle \
         window with no user input (#1634): {} new byte(s) after settle. \
         Trailing bytes received in the window:\n{}",
        byte_count_after.saturating_sub(byte_count_before),
        escape_for_display(&snapshot_from(&captured, byte_count_before))
    );
}

/// Sanity check on the harness itself: a freshly spawned session, before
/// any settling, must actually produce *some* output (the startup paint).
/// Without this, a broken spawn (wrong binary path, ConPTY never wired up,
/// reader thread never started) would make
/// [`idle_after_settle_produces_no_further_output`] pass vacuously — the
/// #553/#2192 trap this whole test class exists to avoid.
#[test]
fn harness_actually_observes_startup_output() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, comment_heavy_rust("STARTUP")).expect("write main.rs");

    let (mut child, captured, mut writer, _master) = spawn_under_conpty(&main_rs, &home);

    let start = Instant::now();
    let mut saw_bytes = false;
    while start.elapsed() < Duration::from_secs(10) {
        if !captured.lock().unwrap().bytes.is_empty() {
            saw_bytes = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    writer.write_all(b"\x1b:qa!\r").ok();
    writer.flush().ok();
    let _ = child.try_wait();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        saw_bytes,
        "the ConPTY reader thread never observed any bytes within 10s of \
         spawning vcd.exe — the harness itself is broken (wrong exe path, \
         ConPTY not actually wired to the child, or the reader thread \
         never started), which would make the idle-silence test above pass \
         vacuously"
    );
}
