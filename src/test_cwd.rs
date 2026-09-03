//! Process-wide working-directory arbitration for the test run (#785).
//!
//! # Why this exists
//!
//! `cargo test` runs every test of a target in **one process**, on a pool of
//! threads, and the working directory is per-*process*. vimcode has tests on
//! both sides of that:
//!
//! - **Writers.** `Engine::open_folder` / `open_workspace` call
//!   `std::env::set_current_dir` (see `core/engine/buffers.rs`) — production
//!   behaviour, not a bug — so any test that drives them moves the whole
//!   process.
//! - **Readers.** A surprising amount of *paint* depends on the CWD:
//!   `Engine`'s explorer root (`core/engine/mod.rs`), `ext_panel`'s
//!   relative-path display, `buffers.rs`'s fallback base directory. The GTK
//!   harness paints a real file explorer, so with the directory moved the
//!   sidebar renders `VIMCODE_TEST_OPEN_FOLDER` and an empty tree instead of
//!   the repo's name and file list — a *different frame* around whatever the
//!   test was actually probing.
//!
//! [`CwdGuard`] (#785, first round) restored the directory on `Drop`, which
//! fixed the *leak* — every test that started **after** a writer finished.
//! It did nothing for tests running **during** the writer's window, which is
//! the same bug reached by a different scheduling order and is why
//! `tab_hover_tooltip_paints_below_tab_row_not_inside_it` went red on a
//! diff that touches neither tooltips nor the explorer. It reproduces 15/15
//! with:
//!
//! ```text
//! cargo test --lib -- tab_hover_tooltip_paints_below \
//!     test_lsp_flush_clears_diagnostics_by_canonical_path \
//!     test_open_folder_resets_cwd test_open_workspace_parses_json
//! ```
//!
//! The paint side was also confirmed *deterministically*, rather than
//! inferred from that race: a scratch single-threaded test that painted the
//! same engine twice with a `set_current_dir("/tmp")` in between found the
//! top 200 rows across the full 1400px width differing — which is exactly
//! the band `tab_hover_tooltip_paints_below_tab_row_not_inside_it` compares
//! for equality. So any writer overlapping a harness *can* corrupt a
//! frame-vs-frame comparison; the only question was scheduling luck.
//!
//! # The mechanism
//!
//! One process-wide [`RwLock`]. Writers take it exclusively for as long as
//! the directory is moved ([`CwdGuard`], which also restores on `Drop`);
//! readers take it shared for as long as they need a stable directory
//! ([`CwdReadGuard`], held by `gtk::testing::Harness` for its whole
//! lifetime, covering every frame it paints). Concurrent readers still run
//! in parallel — the only serialisation added is reader-vs-writer, and
//! there are four writers in the whole suite.
//!
//! Two properties worth knowing before you use these:
//!
//! - **Reader acquisition is thread-local reentrant.** A test that holds two
//!   harnesses at once (or nests one inside another) takes the underlying
//!   read lock exactly once, at depth 0. Without that, `std::sync::RwLock`'s
//!   writer-preferring behaviour — a queued writer parks *new* readers — can
//!   deadlock a thread on its own second read.
//! - **Never mix the two on one thread.** A writer that builds a GTK harness
//!   (or a harness test that takes a [`CwdGuard`]) self-deadlocks. No test
//!   does this today: writers live in `core::engine::tests`, readers in
//!   `gtk::testing`.
//!
//! Poisoning is deliberately ignored (`into_inner`). A panicking test must
//! not cascade into "every later test panics on a poisoned lock" — the whole
//! point of the guard is that one red test stays one red test.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::{RwLock, RwLockReadGuard};

/// The arbiter. Exclusive = "I am moving the process CWD"; shared = "I need
/// it to hold still".
pub static CWD_LOCK: RwLock<()> = RwLock::new(());

thread_local! {
    /// Reentrancy depth of [`CwdReadGuard`] on this thread.
    static READ_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// The single underlying read guard this thread holds while
    /// `READ_DEPTH > 0`.
    static READ_GUARD: RefCell<Option<RwLockReadGuard<'static, ()>>> =
        const { RefCell::new(None) };
}

/// Shared claim on the process working directory: while one is alive, no
/// [`CwdGuard`] can be constructed, so the directory cannot move underneath
/// the frames being painted.
///
/// Reentrant per thread — see this module's doc for why that matters.
pub struct CwdReadGuard(());

impl CwdReadGuard {
    /// Take (or join) this thread's shared claim.
    pub fn acquire() -> Self {
        READ_DEPTH.with(|depth| {
            if depth.get() == 0 {
                let guard = CWD_LOCK.read().unwrap_or_else(|e| e.into_inner());
                READ_GUARD.with(|slot| *slot.borrow_mut() = Some(guard));
            }
            depth.set(depth.get() + 1);
        });
        Self(())
    }
}

impl Drop for CwdReadGuard {
    fn drop(&mut self) {
        READ_DEPTH.with(|depth| {
            let remaining = depth.get().saturating_sub(1);
            depth.set(remaining);
            if remaining == 0 {
                READ_GUARD.with(|slot| *slot.borrow_mut() = None);
            }
        });
    }
}

/// Exclusive claim on the process working directory, which is **also**
/// restored to where it started when this drops.
///
/// Use it in any test that lets the engine chdir (`open_folder`,
/// `open_workspace`, `:cd`) or that chdirs itself. Restoring on `Drop`
/// rather than at the end of the test body means a panicking assertion can't
/// skip the cleanup and turn one red test into many.
pub struct CwdGuard {
    /// Where the directory was before; put back by `Drop`, which runs before
    /// either field is dropped — so the exclusive claim below is always
    /// released *after* the restore, never before.
    previous: Option<PathBuf>,
    _lock: std::sync::RwLockWriteGuard<'static, ()>,
}

impl CwdGuard {
    /// Block until every [`CwdReadGuard`] has gone, then record the current
    /// directory so `Drop` can put it back.
    pub fn new() -> Self {
        let lock = CWD_LOCK.write().unwrap_or_else(|e| e.into_inner());
        Self {
            previous: std::env::current_dir().ok(),
            _lock: lock,
        }
    }
}

impl Default for CwdGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(dir) = self.previous.take() {
            let _ = std::env::set_current_dir(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing property, asserted directly rather than through a
    /// timing race: a live [`CwdGuard`] excludes readers, so no harness can
    /// paint a frame while the directory is moved.
    ///
    /// RED-first: drop the `_lock` field from `CwdGuard` (i.e. go back to
    /// the restore-only guard) and `try_read` succeeds, failing this.
    #[test]
    fn cwd_guard_excludes_readers_for_its_whole_lifetime() {
        let guard = CwdGuard::new();
        assert!(
            CWD_LOCK.try_read().is_err(),
            "a live CwdGuard must exclude harness paints — restoring the \
             directory on drop only fixes the tests that start *after* the \
             writer, not the ones running during it (#785)"
        );
        drop(guard);

        // No "and now it is released" assertion here or below: the lock is
        // process-wide, so another thread can hold it at any instant and
        // such an assertion would be a flake of its own. A guard that never
        // released would hang the whole suite instead — loud enough without
        // an assertion.
    }

    /// The reverse direction, and the reason `Harness` holds one: a live
    /// reader keeps every writer out.
    #[test]
    fn read_guard_excludes_writers_and_is_reentrant() {
        let outer = CwdReadGuard::acquire();
        assert!(
            CWD_LOCK.try_write().is_err(),
            "a live CwdReadGuard must exclude CwdGuard"
        );

        // Nested acquisition on the same thread must not deadlock and must
        // not release the claim early — the two-harnesses-at-once case.
        let inner = CwdReadGuard::acquire();
        drop(inner);
        assert!(
            CWD_LOCK.try_write().is_err(),
            "dropping a nested read guard must not release the outer claim"
        );
        drop(outer);
    }
}
