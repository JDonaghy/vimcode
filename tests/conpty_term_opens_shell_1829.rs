//! #1829: real-ConPTY regression test for "Windows TUI: `:term` opens a
//! blank panel with no PowerShell prompt".
//!
//! # Why this file, and why it only runs on Windows
//!
//! Every vimcode-side call site between the `:term` ex-command and the PTY
//! write is already shared, platform-neutral code with no Windows-specific
//! branch in it: `Engine::execute_command` maps `"terminal"` straight to
//! `EngineAction::OpenTerminal` (`src/core/engine/execute.rs`),
//! `render::handle_action`'s `OpenTerminal` arm (`src/render.rs`) calls
//! `Engine::terminal_new_tab` (`src/core/engine/terminal_ops.rs`) — the
//! *same* function on every backend — and once `terminal_has_focus` is
//! set, every subsequent keystroke is forwarded to the PTY by
//! `render::route_terminal_key` (`src/render.rs`), also shared.
//! `TerminalSession::spawn`/`poll` is `quadraui::terminal_engine` — not
//! vimcode source at all. So if the panel really does come up blank on
//! Windows while working on Linux/macOS, the fault has to be below this
//! line, in the Windows ConPTY leg of `portable_pty` that
//! `quadraui::terminal_engine::TerminalSession` wraps — confirmed, not
//! assumed: at this repo's pinned quadraui rev,
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
//! Enter — exactly what a user types — waits for the terminal toolbar's own
//! `[1]` tab label to actually paint (not a fixed sleep — see
//! [`wait_for_screen_contains`]'s doc for why, and [`PANEL_OPEN_NEEDLE`]
//! for why *that* string and not the panel's other chrome), then sends the issue's own
//! acceptance probe: `echo <marker> > '<path>'` + Enter, straight into the
//! PTY exactly as a real keystroke stream would arrive. It then polls the
//! filesystem (not the screen — the probe's whole point is that it proves
//! the shell actually *ran* a command, not merely that something painted)
//! for that file to appear containing the marker.
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
//! plain UTF-8/ASCII or UTF-16LE (the PowerShell 5.1 case above); it also
//! accepts UTF-16BE defensively (neither PowerShell 5.1 nor pwsh 7, the
//! only two shells `default_shell()` can select, ever emits big-endian
//! UTF-16, so that branch is not expected to ever match in practice — it
//! costs nothing to keep and avoids this test silently depending on which
//! endianness happens to be in fashion).
//!
//! # CI wiring
//!
//! `.github/workflows/ci.yml`'s `build-windows-tui` job runs this test
//! explicitly (as a `continue-on-error: true` step — see that job's own
//! comment for why, and for the promotion condition) so it actually
//! executes somewhere: `#![cfg(windows)]` alone makes this file compile to
//! zero tests on every Linux/macOS lane, and CI's own `windows-latest`
//! runner hosts a real native ConPTY, so no attached physical Windows host
//! is required to run it.
//!
//! What a result from that step means depends on *when* it was produced:
//!
//! * **Today**, at the quadraui pin this test landed against, a RED result
//!   is the *expected* outcome and carries no new information — the
//!   investigation that added this file attributed #1829 to quadraui's own
//!   untested Windows ConPTY leg (`docs/PENDING_QUADRAUI_ISSUES.md`), which
//!   has not been fixed yet. The useful signal from a red run today is the
//!   *shape* of the failure: which of the two assertions below fired, and
//!   the captured screen attached to it — enough to confirm this test
//!   behaves as designed (reaches the probe, then times out waiting on the
//!   dead shell) rather than failing for a reason of its own.
//! * **After** the upstream fix lands and this repo's pin is bumped past
//!   it, a GREEN result is what promotes this step off
//!   `continue-on-error`, and a RED one becomes a real regression signal.
//!
//! The precondition needle this test waits on ([`PANEL_OPEN_NEEDLE`]) is
//! separately pinned down on Linux, with no Windows host at all, by
//! `src/tui_main/app_on_tui_tests.rs`'s
//! `term_ex_command_paints_bracketed_tab_label_not_uppercase_terminal`, so
//! "the needle is wrong" cannot masquerade as "ConPTY is broken" here.
#![cfg(windows)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

/// Upper bound on how long this test waits for the startup paint, the
/// terminal panel's own header, and separately for the probe file to
/// appear. Generous for the same reason `tests/conpty_activity_bar_click.rs`'s
/// `SETTLE_TIMEOUT` is: a cold real-hardware PowerShell startup (profile
/// scripts, etc.) can legitimately take several seconds.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(20);

const PTY_ROWS: u16 = 40;
const PTY_COLS: u16 = 120;

/// The painted string this test uses as its "`:term` actually opened the
/// terminal panel" precondition: the terminal toolbar's first per-tab
/// label, `format!("[{}]", i + 1)` in `render::build_terminal_toolbar`.
///
/// Why this string and not the panel's other chrome:
///
/// * **`"TERMINAL"` (uppercase) is unreachable.** It is the toolbar tab
///   strip's `if tabs.is_empty()` fallback label, so it paints only when
///   `TerminalPanel::tab_count == 0`. `tab_count` is
///   `engine.terminal_panes.len()`, and `render::terminal_panel_desc`
///   early-returns `None` — painting no panel at all — when there are no
///   panes, so `tab_count >= 1` whenever the panel paints. On the
///   `TerminalSession::spawn` *failure* path, `Engine::terminal_new_tab_at`
///   never sets `terminal_open`, so the panel is not painted either. The
///   needle is therefore unreachable in every state this test can reach:
///   waiting on it would burn `SETTLE_TIMEOUT` and fail identically on a
///   working build and a broken one, never reaching the probe below.
///   (The uppercase sidebar needles in
///   `tests/conpty_activity_bar_click.rs` — `"EXPLORER"` et al — come from
///   `Engine::fixed_panel_title_tooltip`, which covers *sidebar* panels
///   only and has no terminal entry; the analogy does not carry over.)
/// * **`"Terminal"` (title case) is vacuous.** It is the bottom-panel tab
///   bar's label, but it is *also* a permanent top-level menu-bar title, so
///   it is already on screen before `:term` is ever typed.
/// * `"[1]"` is present exactly when a terminal pane exists, and absent
///   before. `src/tui_main/app_on_tui_tests.rs`'s
///   `term_ex_command_paints_bracketed_tab_label_not_uppercase_terminal`
///   asserts both halves of that on Linux, so this constant is verifiable
///   with no Windows host attached.
const PANEL_OPEN_NEEDLE: &str = "[1]";

/// Byte sink the reader thread appends to; read back (cloned) by the
/// polling helpers below. A bare `Arc<Mutex<Vec<u8>>>` would say the same
/// thing — kept as a one-field struct only so call sites read
/// `captured.lock().unwrap().bytes` rather than a doubly-wrapped `Vec`.
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

/// The pieces [`spawn_under_conpty`] hands back: the spawned child, the
/// growing captured-bytes buffer, a shared writer for sending keystrokes,
/// and the PTY's master handle (kept alive for the duration of the test;
/// dropping it would close the PTY).
type SpawnedConpty = (
    Box<dyn portable_pty::Child + Send + Sync>,
    Arc<Mutex<Captured>>,
    Arc<Mutex<Box<dyn Write + Send>>>,
    Box<dyn portable_pty::MasterPty + Send>,
);

/// Spawn `vcd.exe` under a real ConPTY, opened on `main_rs`, with `home` as
/// its working directory. See `tests/conpty_activity_bar_click.rs`'s
/// `spawn_under_conpty` doc for why the reader thread must also answer
/// `ESC [ 6 n` from an independently spawned thread (a real, reproducible
/// ConPTY deadlock otherwise) — this is the identical responder.
fn spawn_under_conpty(main_rs: &Path, home: &Path) -> SpawnedConpty {
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

/// Feed every byte captured so far (from `*fed` onward) into `parser`,
/// advancing `*fed` to the new total. Mirrors
/// `tests/conpty_activity_bar_click.rs`'s `sync_parser`: keeping one
/// persistent `vt100::Parser` across the whole test and feeding it
/// incrementally, rather than re-parsing the whole captured stream from
/// byte 0 on every poll, is just an efficiency choice — `vt100::Parser::
/// process` is a pure state transition, so the two converge on the same
/// screen either way, but incremental feeding is O(n) over the run instead
/// of O(n^2).
fn sync_parser(parser: &mut vt100::Parser, captured: &Arc<Mutex<Captured>>, fed: &mut usize) {
    let new_bytes = {
        let c = captured.lock().unwrap();
        c.bytes[*fed..].to_vec()
    };
    *fed += new_bytes.len();
    parser.process(&new_bytes);
}

/// Poll the captured byte stream, syncing `parser` via [`sync_parser`] each
/// time, until [`screen_text`] contains `needle` — or fail (return `false`)
/// after `timeout` total. Polling actual painted content (rather than a
/// fixed "settle" sleep) is deliberate: `tests/conpty_activity_bar_click.rs`'s
/// own module doc records this RED-verified by hand on real hardware — a
/// fixed quiet/settle window fails the very first assertion even on an
/// otherwise-working build, because cold real-hardware startup (profile
/// scripts, etc.) can legitimately exceed any fixed bound.
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

/// Poll the filesystem for `path` to exist and contain `marker` (in any of
/// the encodings [`contains_marker`] accepts), or fail after `timeout`.
fn wait_for_probe_file(path: &Path, marker: &str, timeout: Duration) -> bool {
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

/// RAII guard ensuring the spawned `vcd.exe` child, this test's isolated
/// `%APPDATA%`/`%USERPROFILE%` tree, and the probe file are all cleaned up
/// no matter how the test function exits — including unwinding past a
/// failed `assert!`, which Rust still runs `Drop` for. Mirrors
/// `tests/extensions.rs`'s `OrphanChildGuard` (#1822's "make the collision
/// test leak-proof on any exit path") for the identical property: teardown
/// must not depend on reaching the function's own final assertion.
struct TermTestGuard {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    home: PathBuf,
    probe_path: Mutex<Option<PathBuf>>,
}

impl TermTestGuard {
    /// Record the probe file's path so `Drop` can remove it too. Called
    /// once the path is actually computed, which happens after this guard
    /// is already constructed (the guard must exist *before* the first
    /// assertion, i.e. before the probe path is even known).
    fn set_probe_path(&self, path: PathBuf) {
        *self.probe_path.lock().unwrap() = Some(path);
    }
}

impl Drop for TermTestGuard {
    fn drop(&mut self) {
        // Best-effort `:qa!`, matching the sibling ConPTY files' teardown.
        // Note this only lands on vimcode's own ex line *before* `:term`
        // succeeds: once `terminal_has_focus` is set,
        // `render::route_terminal_key` forwards these bytes to the nested
        // shell instead, so in the (common) healthy case the `kill()` below
        // is what actually ends the session. Kept anyway because it does
        // cover the early-failure paths — a panic from the startup-frame or
        // panel-open assertion, where vimcode is still in Normal mode.
        //
        // Every `lock()` here uses `unwrap_or_else(|e| e.into_inner())`
        // rather than `unwrap()`: `Drop` runs while unwinding past a failed
        // assertion, and a panic *inside* `Drop` during an unwind aborts the
        // process, destroying the captured-screen diagnostics the assertion
        // message exists to deliver. A poisoned mutex must not cost us that.
        {
            let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            w.write_all(b"\x1b\x1b:qa!\r").ok();
            w.flush().ok();
        }
        std::thread::sleep(Duration::from_millis(500));
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(p) = self
            .probe_path
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = std::fs::remove_file(&p);
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[test]
fn term_command_opens_a_working_shell_over_real_conpty_1829() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

    let (child, captured, writer, _master) = spawn_under_conpty(&main_rs, &home);

    // Constructed immediately after spawn, before any assertion can fail —
    // see `TermTestGuard`'s own doc for why this ordering matters.
    let guard = TermTestGuard {
        child,
        writer: Arc::clone(&writer),
        home: home.clone(),
        probe_path: Mutex::new(None),
    };

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);
    let mut fed = 0usize;

    // Wait for the first real frame (status bar's NORMAL mode indicator) —
    // same precondition `tests/conpty_activity_bar_click.rs` waits on.
    assert!(
        wait_for_screen_contains(&mut parser, &captured, &mut fed, "NORMAL", SETTLE_TIMEOUT),
        "vcd.exe never painted its first real frame (status bar's NORMAL \
         mode indicator) within {SETTLE_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );

    // Type `:term` + Enter — the issue's own repro gesture, byte-for-byte
    // what a real keystroke stream delivers.
    {
        let mut w = writer.lock().unwrap();
        w.write_all(b":term\r").expect("write :term<Enter>");
        w.flush().ok();
    }

    // Wait for the terminal toolbar's own first tab label to actually
    // paint, rather than sleeping a fixed duration — see
    // [`wait_for_screen_contains`]'s doc for why a fixed settle window is
    // flaky on real hardware, and [`PANEL_OPEN_NEEDLE`] for why this
    // particular string. The label is drawn by vimcode's own shared panel
    // chrome from `terminal_panes.len()`, not by anything the nested shell
    // writes, so it paints even if the shell inside the panel is dead —
    // which is exactly the distinction #1829 needs. This is a precondition
    // check on `OpenTerminal` having dispatched and `TerminalSession::
    // spawn` having returned `Ok`, not part of what #1829 is actually
    // testing.
    assert!(
        wait_for_screen_contains(
            &mut parser,
            &captured,
            &mut fed,
            PANEL_OPEN_NEEDLE,
            SETTLE_TIMEOUT
        ),
        "`:term` never painted the terminal toolbar's \"{PANEL_OPEN_NEEDLE}\" \
         tab label within {SETTLE_TIMEOUT:?} — the panel itself never \
         opened, so this is *not* #1829's symptom (which is a panel that \
         opens but has no working shell in it); suspect `:term` dispatch or \
         a `TerminalSession::spawn` error instead, and check the status line \
         for \"terminal: failed to open PTY\". Screen:\n{}",
        screen_text(&parser)
    );

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
    guard.set_probe_path(probe_path.clone());

    // Single-quoted PowerShell string literal: the probe path comes from
    // `std::env::temp_dir()`, which this test does not control the
    // contents of, and single quotes are inert in PowerShell (no `$`/
    // backtick interpolation), unlike the double quotes a straight
    // `Path::display()` interpolation would otherwise sit inside.
    let command_line = format!("echo {marker} > '{}'\r", probe_path.display());
    {
        let mut w = writer.lock().unwrap();
        w.write_all(command_line.as_bytes())
            .expect("write probe echo command");
        w.flush().ok();
    }

    let found = wait_for_probe_file(&probe_path, &marker, SETTLE_TIMEOUT);

    let final_screen = {
        sync_parser(&mut parser, &captured, &mut fed);
        screen_text(&parser)
    };

    // `guard` drops at the end of this function on every path (including
    // unwinding past the assertion below), tearing down the child, the
    // isolated home, and the probe file — see `TermTestGuard`'s doc.
    assert!(
        found,
        "typing `:term` then `echo {marker} > '{}'` + Enter over a real \
         ConPTY never produced the probe file within {SETTLE_TIMEOUT:?} — \
         this is #1829's exact symptom (the terminal panel opened but the \
         shell inside it never ran the command, i.e. no working PowerShell \
         prompt ever received the keystrokes); last-observed screen:\n{}",
        probe_path.display(),
        final_screen
    );
}
