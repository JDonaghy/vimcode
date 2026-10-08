//! #1735: deterministic, vcd-free regression test for "[bugbash:tui-pty] 51
//! orphaned vcd processes busy-spin at ~24% CPU each on a deleted pty".
//!
//! # Why this isn't a full end-to-end `vcd`-over-a-real-pty test
//!
//! The obvious black-box shape — spawn the real `vcd` binary with its
//! stdio wired to a real Unix pty slave that was never made its
//! controlling terminal (so there's no `SIGHUP` to short-circuit the
//! repro — see quadraui's `coord` `tui-pty` driver / `UnixPtyChild`,
//! which uses exactly this shape), then close the master and watch `vcd`
//! spin — was tried first and abandoned: it is **racy** for this specific
//! bug, for a reason specific to `vcd`'s own startup, not to the bug
//! itself. `vcd`'s terminal-capability negotiation (kitty-keyboard probe,
//! SGR-Pixels-mouse DECRQM probe — `quadraui::tui::run::setup_terminal`,
//! `quadraui::tui::caps`) sends several queries before the editor's first
//! frame ever paints, and a test harness must answer the one
//! cursor-position query (`ESC[6n`) among them just to get `vcd` past
//! startup at all (a bare repro with no reply times out after 15s without
//! ever reaching the main loop — confirmed experimentally). That reply,
//! it turns out, is *not* reliably consumed before the main loop's event
//! reader (`ratatui::crossterm`'s lazily-initialised, process-global
//! `InternalEventReader`) first touches the fd — direct instrumentation
//! of a local patched copy of `crossterm` 0.29.0 (`src/event/source/unix/
//! mio.rs`, not committed, used only to trace this during investigation)
//! showed the *same* `ESC[1;1R` reply still sitting unread in the
//! kernel's input queue, byte-for-byte, right up until the pty's master
//! closed — and because it finally gets read exactly then, it gives the
//! very first post-hangup wakeup a real, parseable 6-byte chunk to return
//! on, letting `try_read` return normally instead of ever reaching a bare
//! `Ok(0)` read. That one coincidence reliably masked the actual bug in
//! that end-to-end shape, on this kernel, regardless of how long the test
//! waited before closing the master (tried up to +3.5s of extra settle
//! time with no change) — a confound of `vcd`'s own startup sequence, not
//! evidence the bug was fixed.
//!
//! This file sidesteps that confound entirely by never spawning `vcd` (or
//! any capability-negotiating app) at all: it drives `ratatui::crossterm`
//! — the exact, pinned dependency `quadraui::tui::backend::TuiBackend::
//! wait_events` itself calls — directly, in-process, against a real dead
//! pty, with **zero** startup negotiation to race against. That makes the
//! `Ok(0)`-first condition fully deterministic instead of a coin flip.
//!
//! # What this proves, and how
//!
//! `crossterm` 0.29.0's Linux event source
//! (`crossterm::event::source::unix::mio::UnixInternalEventSource::
//! try_read`, confirmed by reading the pinned dependency's actual source
//! at `~/.cargo/registry/src/.../crossterm-0.29.0/src/event/source/unix/
//! mio.rs`) contains, inside its `TTY_TOKEN` readiness-event handler:
//!
//! ```text
//! loop {
//!     match self.tty_fd.read(&mut self.tty_buffer) {
//!         Ok(read_count) => {
//!             if read_count > 0 { self.parser.advance(..); }
//!             // read_count == 0 (EOF): falls through, no break.
//!         }
//!         Err(e) => {
//!             if e.kind() == WouldBlock { break; }      // <- only exit
//!             else if e.kind() == Interrupted { continue; }
//!         }
//!     };
//!     if let Some(event) = self.parser.next() { return Ok(Some(event)); }
//! }
//! ```
//!
//! Once a watched fd's *other* end (the pty master, here) is fully
//! closed, Linux `read(2)` on the slave never blocks and never errors —
//! it returns `Ok(0)` (EOF) forever, on every single call. There is no
//! break arm for that case, so once this loop ever observes `Ok(0)` with
//! no bytes left for `self.parser.next()` to resolve into an event, it
//! never returns at all: an unconditional, un-rate-limited busy loop,
//! confirmed directly (outside crossterm, via a minimal standalone `mio`
//! + `libc` reproduction built during this investigation, not committed:
//! >10,000,000 `read()` calls in under 2 seconds, no sleep, no break) to
//! be exactly the ~100%-of-one-core, mostly-kernel-time (mostly `read`
//! syscalls) signature the bugbash's own `ps`/`/proc` evidence shows
//! (`stime` far exceeding `utime`). `poll(2)`/`epoll` report a fully
//! hung-up fd as immediately "ready" regardless of the requested timeout,
//! so every call into this path from `TuiBackend::wait_events` returns
//! (or, per this bug, never returns) instantly rather than after a real
//! wait — the "busy", not "idle", half of the signature.
//!
//! This test forces exactly that: a real pty slave with **no** trailing
//! bytes of any kind queued (nothing was ever written to the master in
//! this file, let alone left unconsumed), dup'd onto this test process's
//! own `STDIN_FILENO` (crossterm's Linux `tty_fd()` always resolves to
//! `STDIN_FILENO` when `isatty` says yes, which a live pty slave does).
//! One warm-up `ratatui::crossterm::event::poll` call (confirmed `Ok(false)`
//! — truly nothing pending) registers the lazy global reader against it
//! while the pty is still alive, matching the live runner's own steady
//! idle-polling state before a hangup. The master then closes, and a
//! second `poll` call — the real, pinned, public API `TuiBackend::
//! wait_events` itself calls, not a hand-rolled imitation of it — runs on
//! a background thread (since the bug, if present, means it never
//! returns) while the main thread samples this process's own CPU ticks.
//!
//! RED-verified (#1735) against the pinned quadraui rev's own transitive
//! `crossterm` 0.29.0: the background `poll` call never returns and this
//! process's CPU-tick delta over the sample window matches a tight loop
//! (measured at 100% of one core over the 300ms sample, consistently
//! across repeated runs), not idle blocking.
//!
//! # `#[ignore]`d, not deleted or left to red-wall `cargo test`
//!
//! The fix belongs upstream — in `crossterm` itself, or worked around in
//! `quadraui::tui::backend::TuiBackend::wait_events` — not in vimcode; per
//! this repo's Platform-Neutrality Rule, no vimcode-side workaround is
//! added here. Mirrors the precedent in `src/gtk/testing.rs`'s
//! `tab_close_center`-click test (#659/#679): a test that exposes a real
//! upstream defect gets `#[ignore]`d with a doc comment, not deleted and
//! not left to fail every `cargo test` run until the fix lands. Un-ignore
//! this test (no other change should be needed) once `Cargo.toml`'s
//! quadraui pin moves past a `crossterm` version — or a quadraui-side
//! `wait_events` workaround — that breaks on `Ok(0)` instead of looping.
//!
//! **#1765 update: still RED, and expected to stay RED here regardless of
//! which quadraui rev is pinned.** #1765 bumped the pin to `8425673`,
//! which includes quadraui#1295's dead-pty guard (`TuiBackend::
//! wait_events`/`poll_events` now check their own `poll(2)` on stdin
//! before ever delegating to crossterm). That guard cannot affect *this*
//! test's outcome by construction: this file calls
//! `ratatui::crossterm::event::poll` directly, the exact bypass the
//! guard's own doc comment in quadraui spells out — "crossterm is never
//! entered at all once this check has run," which only holds for a
//! caller that runs the check, and this file deliberately isn't one (see
//! this file's own "why this isn't a full end-to-end" section above).
//! `crossterm` 0.29.0 itself is unpatched by #1765 — only vimcode's
//! transitive quadraui pin moved, not crossterm's own pinned version —
//! so this direct repro remains exactly as RED as it was before. See
//! `tests/pty_dead_master_exit.rs::vcd_should_exit_promptly_once_its_pty_master_closes`,
//! added by #1765, for the test that actually exercises the quadraui-side
//! guard end-to-end through a real `vcd` process.
//!
//! **#1775 update: that other test is now GREEN; this one is not, and
//! stays `#[ignore]`d.** #1775 bumped the pin to quadraui `a536053`,
//! which carries quadraui#1301 — a real structural fix (not just a
//! narrower periodic re-check) that stops `TuiBackend::wait_events` from
//! ever delegating a blocking wait straight into crossterm's broken
//! reader. `pty_dead_master_exit.rs` is un-`#[ignore]`d and passing as of
//! #1775 because it drives a real `vcd` through `TuiBackend`, so it can
//! observe that fix. This file still cannot: it calls `ratatui::crossterm
//! ::event::poll` directly, bypassing `TuiBackend` (and therefore
//! quadraui#1301's fix) entirely by construction, so it still only
//! observes whether `crossterm` 0.29.0 itself was patched — it was not.
//! Both tests, and the drafted follow-up quadraui-vs-crossterm-upstream
//! split, are tracked in `docs/PENDING_QUADRAUI_ISSUES.md`'s "crossterm
//! 0.29.0 ... busy-spins" entry — see its "#1775 update" section for what
//! changed.
#![cfg(target_os = "linux")]

use std::ffi::CStr;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Upper bound on how long this test waits for the background `poll` call
/// to either return on its own, or for this process to prove it's busy-
/// spinning instead. Generous relative to "promptly" (a correct
/// implementation notices the dead fd on its very next `read`, i.e.
/// sub-millisecond), but bounded so a reproduction of the bug fails this
/// test in well under a second rather than hanging it.
const OVERALL_BUDGET: Duration = Duration::from_secs(2);

/// The CPU-tick measurement window.
const CPU_SAMPLE_WINDOW: Duration = Duration::from_millis(300);

/// A busy-spinning process pins close to 100% of one core; the bugbash
/// evidence measured ~24% sustained (a `poll`+`read` syscall loop is cheap
/// per iteration, but still far above true idle). Any sustained CPU
/// fraction above this bar, this long after the only fd crossterm is
/// watching died, can only be a spin loop.
const BUSY_SPIN_CPU_FRACTION: f64 = 0.05;

fn errno_io_error(what: &str) -> io::Error {
    io::Error::other(format!("{what}: {}", io::Error::last_os_error()))
}

/// A minimal hand-rolled Unix 98 pty pair, deliberately **not** anyone's
/// controlling terminal (`O_NOCTTY` on both ends, no `setsid()` anywhere)
/// — irrelevant to *this* file's repro (no signal is involved either way;
/// this test never even has a separate child process to receive one), but
/// matching the shape the bugbash's real driving harness (`coord`'s
/// `tui-pty` driver / `UnixPtyChild`) actually produces.
struct RawPty {
    master: OwnedFd,
    slave_path: std::ffi::CString,
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
            if libc::grantpt(master.as_raw_fd()) != 0 {
                return Err(errno_io_error("grantpt"));
            }
            if libc::unlockpt(master.as_raw_fd()) != 0 {
                return Err(errno_io_error("unlockpt"));
            }
            let name_ptr = libc::ptsname(master.as_raw_fd());
            if name_ptr.is_null() {
                return Err(errno_io_error("ptsname"));
            }
            let slave_path = CStr::from_ptr(name_ptr).to_owned();
            Ok(RawPty { master, slave_path })
        }
    }

    /// Open a fresh fd onto the slave device — `O_NOCTTY` so opening it
    /// never implicitly acquires it as this (non-session-leader) test
    /// process's controlling terminal.
    fn open_slave(&self) -> io::Result<OwnedFd> {
        // SAFETY: `self.slave_path` is a valid, nul-terminated path
        // obtained from `ptsname` above; `open`'s return is checked
        // before use.
        let fd = unsafe { libc::open(self.slave_path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        if fd < 0 {
            return Err(errno_io_error("open(slave)"));
        }
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

/// Read `utime + stime` (in clock ticks) from `/proc/self/stat` — the same
/// fields the bugbash evidence's own `ps`-based diagnosis reasoned about
/// (`stime > utime`, "mostly kernel time"), here for this test process
/// itself (the background thread that calls the real, possibly-hanging
/// `poll` is a thread in *this* process, not a separate one, so
/// process-wide ticks already cover it).
fn cpu_ticks_now() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("read /proc/self/stat");
    let after_comm = stat.rsplit_once(')').expect("/proc/self/stat has a ')'").1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime: u64 = fields[11].parse().expect("utime field is numeric");
    let stime: u64 = fields[12].parse().expect("stime field is numeric");
    utime + stime
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

/// Redirect this *test process's own* `STDIN_FILENO` to `fd`, matching
/// what `std::process::Command`'s stdio redirection does for a spawned
/// child — `dup2` replaces fd 0 in place, so every subsequent call that
/// resolves "the terminal" via `isatty(STDIN_FILENO)` (exactly what
/// `crossterm::terminal::sys::file_descriptor::tty_fd` does) sees `fd`,
/// not whatever cargo's test harness originally wired fd 0 to. Safe to do
/// unconditionally here because each file under `tests/` is its own
/// separate process (cargo integration test convention) with exactly one
/// `#[test]` in it — there is no sibling test in this binary whose own fd
/// 0 this could clobber.
fn redirect_stdin_to(fd: RawFd) {
    // SAFETY: `fd` is a valid, open fd for this process's whole lifetime
    // (owned by `RawPty`, which outlives every use of it below);
    // `dup2`'s return is checked.
    let rc = unsafe { libc::dup2(fd, libc::STDIN_FILENO) };
    assert!(
        rc >= 0,
        "dup2(slave, STDIN_FILENO) failed: {}",
        io::Error::last_os_error()
    );
}

#[test]
#[ignore = "#1735/#1765: RED against the pinned quadraui rev's transitive \
            crossterm 0.29.0 — a confirmed upstream busy-loop bug \
            (crossterm's UnixInternalEventSource::try_read never breaks on \
            a bare Ok(0) read), not a vimcode-side regression, and not \
            something quadraui#1295's TuiBackend-level guard can fix for a \
            test that bypasses TuiBackend entirely (see this file's module \
            doc, '#1765 update', and `docs/PENDING_QUADRAUI_ISSUES.md`'s \
            'crossterm 0.29.0 ... busy-spins' entry for the drafted \
            quadraui issue). Run explicitly with `cargo test --release \
            --test crossterm_dead_pty_busy_loop -- --ignored` to see it \
            fail today. Un-ignore once crossterm itself ships a fix this \
            crate's pin can pick up."]
fn crossterm_poll_busy_spins_once_its_only_watched_fd_is_permanently_hung_up() {
    let pty = RawPty::open().expect("allocate a raw Unix 98 pty pair");
    let slave = pty.open_slave().expect("open a slave fd");
    redirect_stdin_to(slave.as_raw_fd());
    // `slave` can close now — `dup2` gave `STDIN_FILENO` its own
    // independent reference to the same underlying tty; crossterm will
    // read via fd 0, not this handle.
    drop(slave);

    // Warm-up: register crossterm's lazy global reader against fd 0
    // *while the pty is still alive*, matching `TuiBackend::wait_events`'s
    // own steady idle-polling state before any hangup — and confirm there
    // is genuinely nothing pending (no leftover bytes of any kind; nothing
    // was ever written to this pty's master at all).
    let warm_up = ratatui::crossterm::event::poll(Duration::from_millis(50));
    assert!(
        matches!(warm_up, Ok(false)),
        "expected the warm-up poll to see nothing pending on a freshly \
         opened, never-written-to pty; got {warm_up:?} — if this fails, \
         the repro below no longer starts from a clean, data-free fd"
    );

    // The bugbash's repro: the driving process/terminal goes away.
    // `pty.master` is the only master-side fd (never duplicated), so
    // dropping it here closes the master end entirely — the slave is now
    // permanently hung up, with *zero* trailing bytes queued either
    // direction (unlike a full `vcd`-over-a-real-pty repro, whose own
    // startup capability negotiation leaves a confounding trailing byte —
    // see this file's module doc).
    eprintln!(
        "DEBUG: closing the only master-side fd ({})",
        pty.master.as_raw_fd()
    );
    drop(pty);

    // The second `poll` call is the one under test — the exact public API
    // `TuiBackend::wait_events` itself calls. If the bug is present, this
    // never returns, so it runs on a background thread while the main
    // thread watches this process's own CPU usage. The background thread
    // is deliberately never joined: if it's genuinely stuck in the bug's
    // infinite loop, this test's `main` returning (or `assert!` panicking)
    // ends the whole process immediately regardless, per `std::thread`'s
    // own contract — there is nothing left over for this test to clean up.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = ratatui::crossterm::event::poll(Duration::from_secs(30));
        // The channel's receiver may already be gone (test already
        // concluded the spin was real and returned) — a dropped-receiver
        // send error is expected and fine to ignore.
        let _ = tx.send(result);
    });

    let start = Instant::now();
    loop {
        if let Ok(result) = rx.try_recv() {
            // Fixed behaviour: the call returned on its own. Whatever it
            // returned (a real "hung up" error, or `Ok(false)`/timeout),
            // it did not busy-spin to get there. Nothing left to check.
            eprintln!(
                "DEBUG: poll() returned {result:?} after {:?} — not a busy spin",
                start.elapsed()
            );
            return;
        }
        if start.elapsed() >= OVERALL_BUDGET {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Still running after the budget. Determine whether it's busy by
    // sampling this process's own CPU ticks across a short window — the
    // background thread, if genuinely stuck in the bug's loop, is the
    // only thing that could be consuming them (the main thread is asleep
    // in the `thread::sleep` below the whole time).
    let before = cpu_ticks_now();
    std::thread::sleep(CPU_SAMPLE_WINDOW);
    let after = cpu_ticks_now();

    if let Ok(result) = rx.try_recv() {
        eprintln!(
            "DEBUG: poll() returned {result:?} just after the budget elapsed \
             ({:?} total) — not a busy spin, just slow",
            start.elapsed()
        );
        return;
    }

    let delta_ticks = after.saturating_sub(before);
    let delta_secs = delta_ticks as f64 / clock_ticks_per_sec();
    let cpu_fraction = delta_secs / CPU_SAMPLE_WINDOW.as_secs_f64();
    assert!(
        cpu_fraction < BUSY_SPIN_CPU_FRACTION,
        "#1735: ratatui::crossterm::event::poll() has not returned {:?} \
         after its only watched fd (a pty slave) was permanently hung up \
         with zero trailing bytes, and this process consumed {:.1}% of a \
         CPU core over a {CPU_SAMPLE_WINDOW:?} sample while waiting — this \
         is the reported busy-spin-on-a-dead-pty bug (orphaned vcd \
         processes spinning at ~24% CPU each on a deleted pty), reproduced \
         directly against crossterm's own event source rather than via a \
         full vcd process. crossterm's `UnixInternalEventSource::try_read` \
         must break out of its TTY read loop on `Ok(0)` (EOF), the same \
         way it already does on a `WouldBlock` error, instead of looping \
         forever.",
        OVERALL_BUDGET + CPU_SAMPLE_WINDOW,
        cpu_fraction * 100.0,
    );
}
