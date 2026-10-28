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
/// Generous on purpose: an idle `vcd` whose pty hangs up exits within one
/// poll tick (milliseconds), so every second of this budget is headroom for
/// a loaded machine's scheduler, not slack the bug could hide in. A
/// busy-spinning build never exits at all, so widening this costs only the
/// time a genuine failure takes to be reported. The whole suite running in
/// parallel on a saturated CI box routinely delays a wakeup by seconds.
const EXIT_BUDGET: Duration = Duration::from_secs(20);

/// Upper bound on how much CPU time (seconds, `utime + stime`) `vcd` is
/// allowed to consume *during* the dead-pty wait window. A correct
/// implementation notices the hangup on its next scheduled wakeup and does
/// ~no work; a busy-spinning process pins close to 100% of one core for the
/// entire window. This — not `EXIT_BUDGET` — is the assertion that
/// discriminates the bug: it is independent of how contended the machine
/// is, because a spin burns CPU proportional to the window while an idle
/// exit burns none regardless of load.
///
/// Note on measurement floor: `cpu_time_secs` reads `/proc/<pid>/stat` on
/// Linux (clock-tick resolution, typically 10ms) and falls back to
/// `ps -o time=` elsewhere. The BSD `ps` macOS ships reports fractional
/// seconds (`[[dd-]hh:]mm:ss.ff`); Linux's `procps` `ps` reports whole
/// seconds only, so on a Linux box without `/proc` this bar degrades to "a
/// full second of CPU was observed."
const MAX_CPU_SECONDS_WHILE_WAITING: f64 = 1.0;

/// How long `vcd` is given to settle into its idle event wait after
/// painting its first frame, before this test closes the pty master.
///
/// The thing under test is "an *idle* vcd notices its pty hung up", so
/// "vcd is idle" is a precondition, not an assumption. A freshly painted
/// first frame does not mean startup is finished: syntax parsing, plugin
/// discovery and theme loading can still be running, and on a loaded
/// machine that tail takes seconds. Closing the master mid-startup
/// measures startup's CPU and startup's latency instead of the hangup
/// path's, which is a false failure.
const QUIESCE_TIMEOUT: Duration = Duration::from_secs(20);

/// Sampling window for the idle check: `vcd` counts as idle once it
/// consumes less than `IDLE_CPU_DELTA_SECONDS` of CPU across this span.
const IDLE_SAMPLE_WINDOW: Duration = Duration::from_millis(250);

/// Number of consecutive idle windows required before the master is closed.
const IDLE_WINDOWS_REQUIRED: u32 = 2;

/// CPU-time growth (seconds) over `IDLE_SAMPLE_WINDOW` that still counts as
/// idle. One clock tick of noise (10ms on Linux) plus margin; a process
/// doing real work consumes far more over a 250ms window.
const IDLE_CPU_DELTA_SECONDS: f64 = 0.03;

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

/// Keeps draining the pty for `span`, so `vcd` never blocks writing into a
/// full pty buffer while this test is only watching its CPU time.
fn pump_for(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    parser: &mut vt100::Parser,
    span: Duration,
) {
    let mut buf = [0u8; 4096];
    let start = Instant::now();
    while start.elapsed() < span {
        if !pump_once(reader, writer, parser, &mut buf) {
            return;
        }
        std::thread::sleep(READ_RETRY_INTERVAL);
    }
}

/// Waits until `pid` stops consuming CPU — `IDLE_WINDOWS_REQUIRED`
/// consecutive `IDLE_SAMPLE_WINDOW`s each growing its CPU time by less
/// than `IDLE_CPU_DELTA_SECONDS` — draining the pty throughout. Returns
/// the CPU time observed once idle, or `None` if `pid` never settled
/// within `timeout` (or its CPU time couldn't be sampled at all).
fn wait_for_cpu_idle(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    parser: &mut vt100::Parser,
    pid: u32,
    timeout: Duration,
) -> Option<f64> {
    let start = Instant::now();
    let mut idle_windows = 0;
    let mut previous = cpu_time_secs(pid)?;
    while start.elapsed() < timeout {
        pump_for(reader, writer, parser, IDLE_SAMPLE_WINDOW);
        let current = cpu_time_secs(pid)?;
        if (current - previous) < IDLE_CPU_DELTA_SECONDS {
            idle_windows += 1;
            if idle_windows >= IDLE_WINDOWS_REQUIRED {
                return Some(current);
            }
        } else {
            idle_windows = 0;
        }
        previous = current;
    }
    None
}

/// Cumulative CPU time (`utime + stime`, in seconds) for `pid`.
///
/// Prefers `/proc/<pid>/stat` (clock-tick resolution, typically 10ms) and
/// falls back to `ps -o time=` where `/proc` isn't available, e.g. macOS.
/// The precision matters twice over: the idle precondition below needs to
/// tell "consuming nothing" from "consuming a little", and the CPU
/// assertion is the test's real discriminator between an idle exit and a
/// busy spin.
///
/// `None` once the process is gone — callers treat that the same as "no
/// new sample", not an error, since a process that's already exited is
/// exactly the success case this test is checking for.
fn cpu_time_secs(pid: u32) -> Option<f64> {
    if let Some(secs) = proc_stat_cpu_time_secs(pid) {
        return Some(secs);
    }
    ps_cpu_time_secs(pid)
}

/// Clock ticks per second as reported by `sysconf(_SC_CLK_TCK)`, the unit
/// `/proc/<pid>/stat`'s `utime`/`stime` fields are counted in.
fn clock_ticks_per_second() -> f64 {
    // SAFETY: `sysconf` takes an int name and returns a long; no pointers,
    // no allocation, no global state touched.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 {
        ticks as f64
    } else {
        100.0
    }
}

/// Cumulative CPU time from `/proc/<pid>/stat`, or `None` where that file
/// doesn't exist (non-Linux) or the process is gone.
fn proc_stat_cpu_time_secs(pid: u32) -> Option<f64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ticks = parse_proc_stat_cpu_ticks(&text)?;
    Some(ticks / clock_ticks_per_second())
}

/// Sums `/proc/<pid>/stat`'s `utime` (field 14) and `stime` (field 15),
/// returning clock ticks.
///
/// Field 2 is the executable name in parentheses and may itself contain
/// spaces and parentheses, so the field split starts after the *last* `)`,
/// which is how the kernel's own documentation says to parse this file.
fn parse_proc_stat_cpu_ticks(text: &str) -> Option<f64> {
    let after_comm = &text[text.rfind(')')? + 1..];
    // `after_comm` starts at field 3 (`state`), so `utime`/`stime` are the
    // 12th and 13th whitespace-separated entries of what remains.
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime = fields.get(11)?.parse::<f64>().ok()?;
    let stime = fields.get(12)?.parse::<f64>().ok()?;
    Some(utime + stime)
}

/// Cumulative CPU time (`utime + stime`, in seconds) `ps` reports for
/// `pid`, parsed from `ps -o time=`'s `[[dd-]hh:]mm:ss[.ff]` format (the
/// same keyword on both the BSD `ps` macOS ships and Linux's `procps`).
/// `None` once the process is gone (what `ps -p <dead-pid>` reports as a
/// non-zero exit with empty stdout).
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
fn proc_stat_cpu_ticks_sums_utime_and_stime() {
    // Fields: pid, (comm), state, ppid, pgrp, session, tty_nr, tpgid,
    // flags, minflt, cminflt, majflt, cmajflt, utime, stime, ...
    let line = "4242 (vcd) S 1 4242 4242 0 -1 4194304 \
                900 0 0 0 137 42 0 0 20 0 1 0 100 0 0";
    assert_eq!(parse_proc_stat_cpu_ticks(line), Some(179.0));
}

#[test]
fn proc_stat_cpu_ticks_tolerates_spaces_and_parens_in_comm() {
    let line = "7 (we i)rd name) S 1 7 7 0 -1 0 \
                0 0 0 0 10 5 0 0 20 0 1 0 0 0 0";
    assert_eq!(parse_proc_stat_cpu_ticks(line), Some(15.0));
}

#[test]
fn own_cpu_time_is_sampleable_and_nonnegative() {
    let secs =
        cpu_time_secs(std::process::id()).expect("this very process's own CPU time is sampleable");
    assert!(
        secs >= 0.0,
        "cumulative CPU time should never be negative, got {secs}"
    );
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

    // The thing under test is how an *idle* vcd reacts to its pty hanging
    // up, so settle into the idle event wait first: a painted first frame
    // doesn't mean startup finished. The CPU time measured once idle
    // doubles as the baseline for the assertion below, so that assertion
    // measures only what vcd spends *after* its pty dies.
    //
    // `None` (no sample could be taken at all) is kept distinct from
    // `Some(0.0)`: a silent zero-fallback would measure vcd's *total* CPU
    // including startup, which could fail the assertion for unrelated
    // reasons. If the baseline is missing, the CPU assertion is skipped.
    let quiesce_start = Instant::now();
    let cpu_before = wait_for_cpu_idle(
        &mut *reader,
        &mut *writer,
        &mut parser,
        pid,
        QUIESCE_TIMEOUT,
    );
    match cpu_before {
        Some(cpu) => eprintln!(
            "vcd settled into its idle event wait after {:?} \
             (CPU time consumed by startup: {cpu:.2}s)",
            quiesce_start.elapsed()
        ),
        None => eprintln!(
            "WARNING: vcd never went idle within {QUIESCE_TIMEOUT:?} of its \
             first painted frame (or its CPU time could not be sampled — see \
             cpu_time_secs). Closing the pty master anyway; the CPU-time \
             assertion is skipped because there is no trustworthy baseline, \
             so only the exit-time assertion runs."
        ),
    }

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
    let mut peak_cpu_during_wait = match (cpu_before, cpu_time_secs(pid)) {
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
        if let (Some(before), Some(cpu_now)) = (cpu_before, cpu_time_secs(pid)) {
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

    // On stderr so a failing run in a CI/smoke log that captures only
    // stderr still says what was measured, not just which line panicked.
    eprintln!(
        "after closing the pty master: exited={exited} after {wait_elapsed:?} \
         (budget {EXIT_BUDGET:?}), peak CPU consumed while waiting \
         {peak_cpu_during_wait:.2}s (bar {MAX_CPU_SECONDS_WHILE_WAITING}s)"
    );

    assert!(
        exited,
        "#1735: vcd did not exit on its own within {EXIT_BUDGET:?} of its \
         pty master closing — it is still running (and by this point may \
         be busy-spinning on the dead fd; peak CPU time observed while \
         waiting: {peak_cpu_during_wait:.2}s). A fixed build notices the \
         hangup and exits within roughly one idle-poll tick. Unless the \
         warning above says otherwise, vcd was confirmed idle before the \
         master was closed, so a loaded machine alone doesn't explain this."
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
