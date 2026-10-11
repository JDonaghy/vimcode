//! Real-pty regression test that a real `vcd` process exits
//! promptly (bounded wall-clock time, bounded CPU) once its pty's master
//! side closes, instead of busy-spinning on the dead fd forever.
//!
//! # How this test exercises the fix
//!
//! This test runs a real `vcd` binary under a real Unix pty (similar to
//! `spawn_under_pty` in `tests/pty_settings_header_delay.rs`), closes the
//! pty master from outside, and verifies that the process exits on its own
//! in bounded time without consuming excessive CPU. `TuiBackend::wait_events`
//! calls `wait_for_stdin_ready`, a `poll(2)`-based check that detects
//! `POLLHUP` immediately, rather than delegating a blocking wait to
//! `crossterm::event::poll`.
//!
//! # Every dup of the master fd must close, not just one handle
//!
//! `portable_pty::MasterPty::try_clone_reader`/`take_writer` both `dup(2)`
//! the master fd. The pty's master side stays open — and the slave never
//! sees a hangup — until *every* dup'd fd referencing it is closed, not
//! just the original `MasterPty` handle. This test reads on the main test
//! thread without background threads holding cloned reader/writer fds, so
//! a single explicit `drop` of the reader, writer, and master is a real,
//! complete close.
#![cfg(unix)]

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const PTY_ROWS: u16 = 30;
const PTY_COLS: u16 = 100;

/// How long `vcd`'s startup paint is allowed to take before this test
/// gives up (generous — this is a precondition, not the thing under test).
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Upper bound on how long this test waits, after closing the pty master,
/// for `vcd` to exit on its own. `TuiBackend::wait_events` calls
/// `wait_for_stdin_ready`, which uses `poll(2)` to detect `POLLHUP`
/// immediately, and exits essentially right away; a busy-spinning process
/// never exits. This budget is tight enough that a busy-spin bug is caught
/// in seconds rather than hanging the suite.
const EXIT_BUDGET: Duration = Duration::from_secs(5);

/// Upper bound on how much CPU time (seconds, `utime + stime` as reported
/// by `ps -o time=`) `vcd` is allowed to consume *during* the dead-pty
/// wait window. A correct implementation notices the hangup on its next
/// scheduled wakeup and does ~no work; a busy-spinning process pins close
/// to 100% of one core for the entire window. `EXIT_BUDGET` is 5s; 1s of
/// consumed CPU time during that window is already a generous bar — a
/// reproduction of the bug would consume close to the full 5s.
///
/// Note on measurement floor: the BSD `ps` macOS ships reports `time=` with
/// fractional seconds (`[[dd-]hh:]mm:ss.ff`), but Linux's `procps` `ps`
/// reports whole seconds only (`[DD-]HH:MM:SS`, no `.ff` field at all) —
/// `parse_ps_time` handles both, but on Linux this bar is effectively "a
/// full second of CPU was observed," not "1.0s precisely." That's still
/// well below what a 100%-of-a-core spin would produce over `EXIT_BUDGET`,
/// so it doesn't change what this test catches, but don't tighten this
/// constant below 1.0 expecting sub-second precision on Linux.
const MAX_CPU_SECONDS_WHILE_WAITING: f64 = 1.0;

/// Non-blocking read polling interval while waiting for `vcd`'s startup
/// output (the pty's master fd is set `O_NONBLOCK`, so each `read()` that
/// finds nothing yet returns `WouldBlock` immediately rather than
/// blocking — this is how long this test sleeps between retries).
const READ_RETRY_INTERVAL: Duration = Duration::from_millis(10);

fn isolated_home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "vimcode_pty_1735_{}_{}",
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
        r#"{"lsp_enabled": false, "use_nerd_fonts": false}"#,
    )
    .expect("seed settings.json");
    home
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
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

/// Sets `O_NONBLOCK` on `fd` (shared by every `dup(2)` of the same open
/// file description, including the clones `try_clone_reader`/`take_writer`
/// below hand out — `fcntl(F_SETFL, ...)` flags live on the open file
/// description, not the per-process fd-table entry).
fn set_nonblocking(fd: std::os::fd::RawFd) {
    // SAFETY: `fd` is a valid, open fd for the whole duration of this
    // call (owned by `master` in the caller, which outlives it); both
    // `fcntl` calls' returns are checked.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        assert!(
            flags >= 0,
            "fcntl(F_GETFL) failed: {}",
            std::io::Error::last_os_error()
        );
        let rc = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        assert!(
            rc == 0,
            "fcntl(F_SETFL, O_NONBLOCK) failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Reads whatever is currently available from `reader` (non-blocking —
/// `WouldBlock` means "nothing yet", not an error) into `parser`, and
/// answers the startup `ESC[6n` cursor-position query inline if this
/// chunk contains it. Returns `false` once the pty signals EOF
/// (`Ok(0)`) — not expected to happen before this test explicitly closes
/// the master itself.
fn pump_once(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    parser: &mut vt100::Parser,
    buf: &mut [u8],
) -> bool {
    match reader.read(buf) {
        Ok(0) => false,
        Ok(n) => {
            let chunk = &buf[..n];
            parser.process(chunk);
            if contains_subslice(chunk, b"\x1b[6n") {
                let _ = writer.write_all(b"\x1b[1;1R");
                let _ = writer.flush();
            }
            true
        }
        Err(e) if e.kind() == ErrorKind::WouldBlock => true,
        Err(_) => false,
    }
}

fn wait_for_screen_contains(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    parser: &mut vt100::Parser,
    needle: &str,
    timeout: Duration,
) -> Option<Duration> {
    let mut buf = [0u8; 4096];
    let start = Instant::now();
    loop {
        let still_open = pump_once(reader, writer, parser, &mut buf);
        if screen_text(parser).contains(needle) {
            return Some(start.elapsed());
        }
        if !still_open {
            // EOF before startup ever finished — not expected on this
            // code path (this is only called before the test closes the
            // master itself), so fail fast rather than spinning the
            // retry loop for the full `timeout`.
            return None;
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(READ_RETRY_INTERVAL);
    }
}

/// Cumulative CPU time (`utime + stime`, in seconds) `ps` reports for
/// `pid`, parsed from `ps -o time=`'s `[[dd-]hh:]mm:ss[.ff]` format (the
/// same keyword on both the BSD `ps` macOS ships and Linux's `procps`).
/// `None` once the process is gone (what `ps -p <dead-pid>` reports as a
/// non-zero exit with empty stdout) — callers treat that the same as "no
/// new sample", not an error, since a process that's already exited is
/// exactly the success case this test is checking for.
fn ps_cpu_time_secs(pid: u32) -> Option<f64> {
    let output = Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    parse_ps_time(&text)
}

/// Parses `ps -o time=`'s `[[dd-]hh:]mm:ss[.ff]` into total seconds.
fn parse_ps_time(text: &str) -> Option<f64> {
    let (days, rest) = match text.split_once('-') {
        Some((d, rest)) => (d.parse::<f64>().ok()?, rest),
        None => (0.0, text),
    };
    let fields: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match fields.as_slice() {
        [h, m, s] => (
            h.parse::<f64>().ok()?,
            m.parse::<f64>().ok()?,
            s.parse::<f64>().ok()?,
        ),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [s] => (0.0, 0.0, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(days * 86_400.0 + hours * 3_600.0 + minutes * 60.0 + seconds)
}

#[test]
fn vcd_should_exit_promptly_once_its_pty_master_closes() {
    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

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
    cmd.arg(&main_rs);
    cmd.cwd(&home);
    cmd.env("HOME", &home);
    cmd.env(
        "VIMCODE_TEST_DATA_HOME",
        home.join(".local").join("share").join("vimcode"),
    );
    cmd.env("VIMCODE_TEST_HOME_ISOLATED", "1");
    // Deliberately *not* a controlling terminal (`setsid`/`TIOCSCTTY`,
    // `portable_pty::CommandBuilder`'s default) — a real controlling
    // terminal's hangup kills the child via the default `SIGHUP`
    // disposition, which would make this test pass for the wrong reason.
    // This mirrors the real headless vcd session that `wait_for_stdin_ready`
    // is meant to handle.
    cmd.set_controlling_tty(false);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .expect("spawn vcd under a real Unix pty");
    drop(pair.slave);
    let pid = child.process_id().expect("vcd reports its own pid");

    let master = pair.master;
    let master_fd = master
        .as_raw_fd()
        .expect("master pty exposes a raw fd on unix");
    // Shared by every dup of this open file description — see
    // `set_nonblocking`'s doc.
    set_nonblocking(master_fd);

    let mut reader = master.try_clone_reader().expect("clone pty reader");
    let mut writer = master.take_writer().expect("take pty writer");

    let mut parser = vt100::Parser::new(PTY_ROWS, PTY_COLS, 0);

    assert!(
        wait_for_screen_contains(
            &mut *reader,
            &mut *writer,
            &mut parser,
            "NORMAL",
            STARTUP_TIMEOUT
        )
        .is_some(),
        "vcd never painted its first real frame (status bar's NORMAL mode \
         indicator) within {STARTUP_TIMEOUT:?}; screen:\n{}",
        screen_text(&parser)
    );

    // Baseline CPU time right before the hangup, so the assertion below
    // measures only what vcd spends *after* its pty dies, not startup/paint
    // work. `None` (the baseline `ps` call itself failed) is kept distinct
    // from `Some(0.0)`: a silent zero-fallback would measure vcd's *total*
    // CPU including startup, which could fail the assertion for unrelated
    // reasons. If the baseline is missing, the CPU assertion is skipped.
    let cpu_before = ps_cpu_time_secs(pid);

    // The bugbash's own trigger: the driving terminal/process goes away.
    // Every dup'd fd referencing the master (the original handle, the
    // cloned reader, and the cloned writer — see this file's module doc)
    // must close for the slave to actually hang up; drop all three
    // together.
    drop(reader);
    drop(writer);
    drop(master);

    // One unconditional sample taken right after the close, before the
    // first `try_wait` check below, so a build that exits on its very next
    // scheduler tick (the success path this test exists to protect) still
    // has at least one post-close CPU sample to report — otherwise
    // `peak_cpu_during_wait` could stay at its initial 0.0 the whole time
    // and the CPU assertion below would pass vacuously, never having
    // actually measured anything.
    let mut peak_cpu_during_wait = match (cpu_before, ps_cpu_time_secs(pid)) {
        (Some(before), Some(now)) => (now - before).max(0.0),
        _ => 0.0,
    };

    let wait_start = Instant::now();
    let mut exited = false;
    loop {
        if let Ok(Some(_status)) = child.try_wait() {
            exited = true;
            break;
        }
        if let (Some(before), Some(cpu_now)) = (cpu_before, ps_cpu_time_secs(pid)) {
            let consumed = (cpu_now - before).max(0.0);
            if consumed > peak_cpu_during_wait {
                peak_cpu_during_wait = consumed;
            }
        }
        if wait_start.elapsed() >= EXIT_BUDGET {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let wait_elapsed = wait_start.elapsed();

    // Teardown first, so a failed assertion below doesn't leak a running
    // (or, if the bug is present, busy-spinning) child process.
    if !exited {
        let _ = child.kill();
    }
    let _ = child.wait();

    if cpu_before.is_none() {
        eprintln!(
            "WARNING: the baseline `ps` sample failed (see ps_cpu_time_secs), \
             so the CPU-time assertion below is being skipped entirely rather \
             than measuring against a wrong (zero) baseline. Only the exit-\
             time assertion ran."
        );
    }

    assert!(
        exited,
        "#1735: vcd did not exit on its own within {EXIT_BUDGET:?} of its \
         pty master closing — it is still running (and by this point may \
         be busy-spinning on the dead fd; peak CPU time observed while \
         waiting: {peak_cpu_during_wait:.2}s). A fixed build notices the \
         hangup and exits within roughly one idle-poll tick."
    );
    assert!(
        cpu_before.is_none() || peak_cpu_during_wait < MAX_CPU_SECONDS_WHILE_WAITING,
        "#1735: vcd exited after its pty master closed ({wait_elapsed:?}), \
         but consumed {peak_cpu_during_wait:.2}s of CPU time while doing \
         so — over the {MAX_CPU_SECONDS_WHILE_WAITING}s bar, which is \
         itself generous. This is the reported busy-spin-on-a-dead-pty \
         signature (orphaned vcd processes spinning at ~24% CPU each on a \
         deleted pty), even though it did eventually exit."
    );
}
