//! #1735: real-pty regression test for "[bugbash:tui-pty] 51 orphaned vcd
//! processes busy-spin at ~24% CPU each on a deleted pty".
//!
//! # Why a hand-rolled pty, not `portable_pty`/`TuiDriver`
//!
//! `quadraui::tui::testing::TuiDriver` dispatches scripted `UiEvent`s
//! in-process against a `TestBackend` — there is no real file descriptor
//! anywhere in that path, so it cannot reproduce a dead *physical* pty at
//! all.
//!
//! `portable_pty`'s Unix backend (used by `tests/pty_settings_header_
//! delay.rs`) calls `setsid()` + `ioctl(TIOCSCTTY)` in the child before
//! exec, making the new pty the child's *controlling terminal*. Dropping
//! that master then delivers `SIGHUP` to the child (a real session
//! hangup), which kills `vcd` immediately via the default disposition —
//! confirmed experimentally while writing this test: that path exits
//! `vcd` in well under a second, and does **not** reach the code this
//! issue is actually about.
//!
//! The bugbash's own repro evidence (fd 0/1/2 all pointing at a `(deleted)`
//! pty, process still `R`/running, **no** indication it ever received or
//! acted on a hangup signal) only makes sense if the real driving harness
//! (`coord`'s `tui-pty` driver / `UnixPtyChild`) connects `vcd`'s stdio to
//! the pty slave *without* establishing it as a controlling terminal —
//! e.g. opening the slave with `O_NOCTTY` and never calling `setsid()` in
//! the child, which is exactly what a `std::process::Command` with its
//! stdio redirected to already-open fds does by default (no `setsid`
//! unless a caller explicitly asks via `pre_exec`). In that shape there is
//! no hangup signal at all — the *only* way `vcd` can ever learn the
//! connection died is by noticing a read/write error on its own, which is
//! precisely the code path quadraui's live TUI runner takes
//! (`quadraui::tui::backend::TuiBackend::wait_events`, reached via
//! `quadraui::tui::shell_runner::run_with_shell` →
//! `quadraui::tui::run::TuiRunner::pump`).
//!
//! This test reproduces that shape directly via `posix_openpt`/`grantpt`/
//! `unlockpt` + `O_NOCTTY` on both ends (see [`RawPty`]), rather than
//! `portable_pty`, specifically to rule the signal-based path out and
//! exercise the no-signal, read-error-only path the bugbash evidence
//! implies.
//!
//! # What this test actually proves
//!
//! It spawns `vcd` with its stdio wired to a real Unix pty slave that was
//! never made its controlling terminal, waits for the first real frame to
//! paint, then closes every master-side fd (simulating "the driving
//! process/terminal goes away without vcd reaching its own teardown
//! path"). On Linux, once all master fds are closed, a `read()` on the
//! slave returns `-EIO` ("I/O error") — not `0`/EOF, and not a signal —
//! forever, for as long as the slave stays open. The *expected* behaviour
//! (this issue's own "Expected behaviour" section) is that `vcd` notices
//! that read error and exits promptly, consuming ~0% CPU. The *reported*
//! bug is a tight failing-syscall loop: `crossterm::event::poll` on a dead
//! fd returns "ready" immediately (an error condition is always "ready" to
//! `poll(2)`/`epoll`), `read` then fails, and quadraui's `wait_events`
//! treats that failure as "no events this iteration" rather than "exit" —
//! so the runner spins calling `poll`+`read` with no sleep at all, exactly
//! the ~24%-CPU/mostly-kernel-time symptom the bugbash evidence shows.
//! This test measures real CPU ticks consumed by the child process
//! (`/proc/<pid>/stat`) over a bounded window after the master closes and
//! fails if the process is still alive *and* burning a large fraction of a
//! core — the live signature of the bug, not a proxy for it.
//!
//! RED-verified (#1735) against the unfixed pinned quadraui rev
//! (`ca7fcc83afad01ec3422f79366566f3a263b22bf`): the child neither exits
//! within [`EXIT_OR_IDLE_TIMEOUT`] nor stays near-idle — it is observed
//! consuming most of a CPU core the whole window, matching the bugbash
//! evidence. The actual fix is quadraui-side (see `docs/PENDING_QUADRAUI_
//! ISSUES.md`'s matching entry); per this repo's Platform-Neutrality Rule,
//! no vimcode-side workaround is added here. This test is expected to
//! start passing once that fix lands and the `Cargo.toml` quadraui pin is
//! bumped past it — no change to this file required.
#![cfg(target_os = "linux")]

use std::ffi::CStr;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PTY_ROWS: u16 = 30;
const PTY_COLS: u16 = 100;

/// How long this test waits for `vcd`'s first real frame to paint before
/// giving up on the precondition.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Upper bound on how long this test gives the child to either exit, or
/// settle into a verifiably-idle CPU profile, after every master-side fd
/// is closed. Generous relative to "promptly" (a correct implementation
/// notices the read error on its very next poll iteration, i.e.
/// sub-millisecond), but bounded so a reproduction of the bug fails this
/// test in a few seconds rather than hanging it forever.
const EXIT_OR_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// The CPU-tick measurement window used once [`EXIT_OR_IDLE_TIMEOUT`] has
/// passed and the process is still alive — see [`cpu_ticks_now`].
const CPU_SAMPLE_WINDOW: Duration = Duration::from_millis(500);

/// A busy-spinning process pins close to 100% of one core; the bugbash
/// evidence measured ~24% sustained (a `poll`+`read` syscall loop is cheap
/// per iteration, but still far above true idle). Any sustained CPU
/// fraction above this bar, this long after the connection died, can only
/// be a spin loop — a correctly-idle-but-still-running process (e.g. one
/// genuinely blocked in a blocking write elsewhere) would show ~0%.
const BUSY_SPIN_CPU_FRACTION: f64 = 0.05;

/// A minimal hand-rolled Unix 98 pty pair, deliberately **not** set up as
/// anyone's controlling terminal (`O_NOCTTY` on both ends, no `setsid()`
/// anywhere) — see this file's module doc for why that distinction is the
/// whole point of this test.
struct RawPty {
    master: OwnedFd,
    slave_path: std::ffi::CString,
}

fn errno_io_error(what: &str) -> io::Error {
    io::Error::other(format!("{what}: {}", io::Error::last_os_error()))
}

impl RawPty {
    fn open() -> io::Result<Self> {
        // SAFETY: standard POSIX pty-allocation sequence; each call is
        // checked for its own documented failure return before the next
        // one runs, and `master_fd` is wrapped in `OwnedFd` immediately
        // after the one call that can produce an invalid fd.
        unsafe {
            let master_fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            if master_fd < 0 {
                return Err(errno_io_error("posix_openpt"));
            }
            let master = OwnedFd::from_raw_fd(master_fd);
            if libc::grantpt(master.as_raw_fd_()) != 0 {
                return Err(errno_io_error("grantpt"));
            }
            if libc::unlockpt(master.as_raw_fd_()) != 0 {
                return Err(errno_io_error("unlockpt"));
            }
            let name_ptr = libc::ptsname(master.as_raw_fd_());
            if name_ptr.is_null() {
                return Err(errno_io_error("ptsname"));
            }
            let slave_path = CStr::from_ptr(name_ptr).to_owned();

            let ws = libc::winsize {
                ws_row: PTY_ROWS,
                ws_col: PTY_COLS,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if libc::ioctl(master.as_raw_fd_(), libc::TIOCSWINSZ, &ws) != 0 {
                return Err(errno_io_error("ioctl(TIOCSWINSZ)"));
            }

            Ok(RawPty { master, slave_path })
        }
    }

    /// Open a fresh fd onto the slave device — `O_NOCTTY` so opening it
    /// (even as a process with no controlling terminal of its own, which
    /// `vcd` won't be here since we never call `setsid()`) never
    /// implicitly acquires it as one.
    fn open_slave(&self) -> io::Result<OwnedFd> {
        // SAFETY: `self.slave_path` is a valid, nul-terminated path
        // obtained from `ptsname` above; `open`'s return is checked before
        // use.
        let fd = unsafe { libc::open(self.slave_path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        if fd < 0 {
            return Err(errno_io_error("open(slave)"));
        }
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    /// Non-blocking read of whatever is currently available on the
    /// master, or `None` on `EAGAIN`/`EWOULDBLOCK` (nothing queued right
    /// now). An `Ok(0)`/other error is also folded into `None` — this test
    /// only uses this to opportunistically drain output, not to detect
    /// the master's own end-of-life (that's driven by the explicit `drop`
    /// below).
    fn try_read(&self, buf: &mut [u8]) -> Option<usize> {
        // SAFETY: `buf` is a valid, correctly-sized mutable slice; `read`
        // writes at most `buf.len()` bytes into it, matching the syscall's
        // contract.
        let n = unsafe { libc::read(self.master.as_raw_fd_(), buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            Some(n as usize)
        } else {
            None
        }
    }

    fn write_all(&self, bytes: &[u8]) {
        // SAFETY: `bytes` is a valid slice for its own length; a short or
        // failed write is tolerated (`let _`) since this is only used for
        // the best-effort cursor-position reply below.
        unsafe {
            let _ = libc::write(self.master.as_raw_fd_(), bytes.as_ptr().cast(), bytes.len());
        }
    }

    fn set_nonblocking(&self) -> io::Result<()> {
        // SAFETY: `fcntl` with `F_GETFL`/`F_SETFL` on a valid, owned fd is
        // the standard way to toggle `O_NONBLOCK`; both calls' results are
        // checked.
        unsafe {
            let flags = libc::fcntl(self.master.as_raw_fd_(), libc::F_GETFL);
            if flags < 0 {
                return Err(errno_io_error("fcntl(F_GETFL)"));
            }
            if libc::fcntl(self.master.as_raw_fd_(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(errno_io_error("fcntl(F_SETFL)"));
            }
        }
        Ok(())
    }
}

// Small local helper so the `unsafe` blocks above read as plain fd
// arithmetic rather than reaching for `std::os::fd::AsRawFd` imports.
trait AsRawFdExt {
    fn as_raw_fd_(&self) -> RawFd;
}
impl AsRawFdExt for OwnedFd {
    fn as_raw_fd_(&self) -> RawFd {
        std::os::fd::AsRawFd::as_raw_fd(self)
    }
}

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

/// Spawn `vcd` with its stdio connected to `pty`'s slave, deliberately
/// *not* establishing it as the child's controlling terminal (no
/// `setsid()`/`TIOCSCTTY` anywhere in this process) — see this file's
/// module doc.
fn spawn_attached(pty: &RawPty, main_rs: &PathBuf, home: &PathBuf) -> std::process::Child {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_vcd"));
    let mut cmd = Command::new("strace");
    cmd.args(["-f", "-tt", "-o", "/tmp/vcd_strace_1735.log", "--"]);
    cmd.arg(&exe);
    cmd.arg(main_rs);
    cmd.current_dir(home);
    cmd.env("HOME", home);
    cmd.env(
        "VIMCODE_TEST_DATA_HOME",
        home.join(".local").join("share").join("vimcode"),
    );
    cmd.stdin(Stdio::from(
        pty.open_slave().expect("open slave fd for stdin"),
    ));
    cmd.stdout(Stdio::from(
        pty.open_slave().expect("open slave fd for stdout"),
    ));
    cmd.stderr(Stdio::from(
        pty.open_slave().expect("open slave fd for stderr"),
    ));
    // Deliberately no `setsid`/`TIOCSCTTY`/`pre_exec` of any kind:
    // `std::process::Command` does none of that by default, so the child
    // stays in this process's own session with no controlling terminal
    // change at all — see this file's module doc for why that's the
    // whole point of this test.
    cmd.spawn().expect("spawn vcd attached to the raw pty")
}

/// Read `utime + stime` (in clock ticks) from `/proc/<pid>/stat` — fields
/// 14 and 15 (1-indexed), the same fields the bugbash evidence's own
/// `ps`-based diagnosis reasoned about (`stime > utime`, "mostly kernel
/// time"). Returns `None` once the process is gone (`/proc/<pid>` no
/// longer exists) — treated by callers as "it exited," which is the
/// success path, not a measurement failure.
fn cpu_ticks_now(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` (field 2) is parenthesised and may itself contain spaces/
    // parens, so split on the *last* ')' and index fields from there
    // rather than naively splitting the whole line on whitespace.
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // Fields 1-2 (pid, comm) are consumed above; `fields[0]` here is
    // field 3 (state). utime is field 14 -> index 14-3=11; stime is field
    // 15 -> index 12.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

fn clock_ticks_per_sec() -> f64 {
    // SAFETY: `sysconf` with a valid, well-known name is always safe to
    // call and returns a plain integer; no pointers cross the FFI
    // boundary.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 {
        ticks as f64
    } else {
        100.0 // standard Linux default; used only if the query itself fails
    }
}

#[test]
fn vcd_exits_or_goes_idle_once_its_pty_master_is_closed() {
    let pty = RawPty::open().expect("allocate a raw Unix 98 pty pair");
    pty.set_nonblocking().expect("set master O_NONBLOCK");

    let home = isolated_home();
    let main_rs = home.join("main.rs");
    std::fs::write(&main_rs, "fn main() {}\n").expect("write main.rs");

    let child = spawn_attached(&pty, &main_rs, &home);
    let pid = child.id();

    // Guard: no matter how this test concludes (pass, fail, or panic), make
    // sure the child is actually killed before the test process exits.
    // Without this, a reproduction of the bug under test would leave
    // behind exactly the kind of orphaned, CPU-spinning process #1735
    // reports — this test must not itself become a source of that bug on
    // the machine running it.
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = KillOnDrop(child);

    // Drain output (and answer the ESC[6n cursor-position query ratatui's
    // `Terminal::new()` sends during startup) until the first real frame
    // paints, or we give up.
    let mut captured: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    let start = Instant::now();
    let mut started = false;
    while start.elapsed() < STARTUP_TIMEOUT {
        if let Some(n) = pty.try_read(&mut buf) {
            let chunk = &buf[..n];
            captured.extend_from_slice(chunk);
            if contains_subslice(chunk, b"\x1b[6n") {
                pty.write_all(b"\x1b[1;1R");
            }
        }
        if String::from_utf8_lossy(&captured).contains("NORMAL") {
            started = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        started,
        "vcd never painted its first real frame (status bar's NORMAL mode \
         indicator) within {STARTUP_TIMEOUT:?}; captured so far:\n{}",
        String::from_utf8_lossy(&captured)
    );

    // Simulate the bugbash's repro: the driving process/terminal goes away
    // without vcd reaching its own teardown path. `pty` is the *only*
    // master-side fd (never duplicated), so dropping it here closes the
    // master end entirely; vcd's slave-side stdio starts seeing `-EIO` on
    // every further read, with no signal involved at all (see module
    // doc).
    drop(pty);

    let close_at = Instant::now();
    let deadline = close_at + EXIT_OR_IDLE_TIMEOUT;
    loop {
        if let Some(status) = child.0.try_wait().ok().flatten() {
            eprintln!(
                "DEBUG: child exited {:?} after master close, status={:?}",
                close_at.elapsed(),
                status
            );
            // Fixed behaviour: vcd noticed the dead connection and exited
            // on its own well within the timeout. Nothing left to check.
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // Still alive after the timeout. That alone isn't necessarily the bug
    // (a slow-but-clean shutdown might still be in flight) — the
    // determining signal is whether it's burning CPU while it does it.
    // TEMP DEBUG: strace the still-alive child for a bit to see what
    // syscall it's actually blocked in.
    if let Ok(mut st) = Command::new("strace")
        .args(["-p", &pid.to_string(), "-f", "-tt"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        std::thread::sleep(Duration::from_millis(1500));
        let _ = st.kill();
        if let Some(mut err) = st.stderr.take() {
            let mut out = String::new();
            use std::io::Read as _;
            let _ = err.read_to_string(&mut out);
            eprintln!("DEBUG strace output:\n{out}");
        }
    }

    let before = cpu_ticks_now(pid);
    let state_before = std::fs::read_to_string(format!("/proc/{pid}/status")).ok();
    std::thread::sleep(CPU_SAMPLE_WINDOW);
    let after = cpu_ticks_now(pid);
    eprintln!(
        "DEBUG: still alive after {:?}; before={:?} after={:?} state_before={:?}",
        EXIT_OR_IDLE_TIMEOUT,
        before,
        after,
        state_before
            .as_deref()
            .and_then(|s| s.lines().find(|l| l.starts_with("State:")))
    );

    match (before, after) {
        (None, _) | (_, None) => {
            // Exited between the deadline check and the sample window —
            // the fixed-behaviour path, just observed a few ms later.
        }
        (Some(before), Some(after)) => {
            let delta_ticks = after.saturating_sub(before);
            let delta_secs = delta_ticks as f64 / clock_ticks_per_sec();
            let cpu_fraction = delta_secs / CPU_SAMPLE_WINDOW.as_secs_f64();
            assert!(
                cpu_fraction < BUSY_SPIN_CPU_FRACTION,
                "#1735: vcd (pid {pid}) is still running {:?} after every \
                 master-side fd of its pty was closed, consuming {:.1}% of \
                 a CPU core over a {CPU_SAMPLE_WINDOW:?} sample — this is \
                 the reported busy-spin-on-a-dead-pty bug (orphaned vcd \
                 processes spinning at ~24% CPU each on a deleted pty). An \
                 idle vcd whose controlling pty has gone away must detect \
                 the dead connection (read/write error or EOF) and exit \
                 promptly, not spin.",
                EXIT_OR_IDLE_TIMEOUT + CPU_SAMPLE_WINDOW,
                cpu_fraction * 100.0,
            );
        }
    }
}
