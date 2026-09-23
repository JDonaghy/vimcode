//! Process-wide arbitration of **Pango/Cairo text work** in the test run.
//!
//! # Why this exists
//!
//! `cargo test` runs a target's tests in one process on a pool of threads
//! (20 on this repo's reference machine). The GTK-side tests paint for real:
//! `gtk::testing`'s [`Harness`](crate::gtk::testing::Harness) drives
//! `quadraui::gtk`'s rasterisers into an in-memory Cairo `ImageSurface`, and
//! a handful of smaller test modules (`gtk::click`'s
//! `emoji_click_column_tests`, `gtk::mod`'s `chrome_paint_tests`) build their
//! own headless `pango::Layout` directly. Production code reached from a
//! harness click does the same — `gtk::click::build_editor_click_context`
//! measures a probe glyph through Pango.
//!
//! Two of those running at once is not safe. libpango/libcairo hand glyph
//! measurement and rasterisation down to a **shared, process-global**
//! FreeType layer (cairo's unscaled-font map and the `FT_Face` cache behind
//! it), and concurrent use from several threads segfaults inside
//! `FT_Load_Glyph`. Observed as an intermittent `SIGSEGV` (~10% of full-suite
//! runs of the `vimcode_core` lib test binary, 6/60 in a measured loop),
//! never reproducible when the GTK tests are run on their own — because with
//! only ~123 of them the scheduler rarely lands two in the same instant.
//! Coredumps confirm it directly; the two stacks seen were
//!
//! ```text
//! FT_Load_Glyph → cairo_scaled_font_glyph_extents → pango_glyph_string_extents_range
//!   → pango_layout_get_size → pango_layout_get_pixel_size
//! __memmove_avx_unaligned_erms → libfreetype → … → cairo_show_glyphs
//!   → pango_cairo_show_layout
//! ```
//!
//! with a second thread inside Pango at the same moment each time. This is a
//! libcairo/libfreetype limitation, not a vimcode bug — the live app paints
//! on one thread and never hits it — so the fix belongs in the test
//! scaffolding, not in `src/gtk/`.
//!
//! # The mechanism
//!
//! One process-wide [`Mutex`], taken by [`PaintGuard::acquire`] for as long
//! as the holder may touch Pango. `gtk::testing::harness` takes one for the
//! whole lifetime of every [`Harness`](crate::gtk::testing::Harness) (so
//! *every* test built on the GTK driver is covered without naming this
//! module), and the few tests that build a `pango::Layout` by hand take one
//! at the top of the test body.
//!
//! **If you add a test that touches Pango or Cairo text, take a
//! [`PaintGuard`] first.** A missing one does not fail loudly — it re-arms a
//! ~10%-per-run segfault somewhere else in the suite.
//!
//! Two properties worth knowing:
//!
//! - **Acquisition is thread-local reentrant**, exactly as
//!   [`crate::test_cwd::CwdReadGuard`] is and for the same reason: a test
//!   that holds two harnesses at once (or takes a guard and then builds a
//!   harness) must not deadlock on its own second acquisition.
//! - **Lock ordering is safe against `test_cwd`.** A harness takes this lock
//!   and then `CWD_LOCK` (read); the only `CwdGuard` writers live in
//!   `core::engine::tests` and never take this lock, so there is no cycle.
//!
//! Poisoning is deliberately ignored (`into_inner`), same as `test_cwd`: one
//! red test must stay one red test rather than cascading into "every later
//! painting test panics on a poisoned lock".

use std::cell::{Cell, RefCell};
use std::sync::{Mutex, MutexGuard};

/// The arbiter. Held = "I may be inside Pango/Cairo text code right now".
pub static PAINT_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    /// Reentrancy depth of [`PaintGuard`] on this thread.
    static PAINT_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// The single underlying guard this thread holds while
    /// `PAINT_DEPTH > 0`.
    static PAINT_GUARD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) };
}

/// Exclusive claim on the process's Pango/Cairo text machinery: while one is
/// alive, no other thread can be painting or measuring text.
///
/// Reentrant per thread — see this module's doc for why that matters.
pub struct PaintGuard(());

impl PaintGuard {
    /// Take (or join) this thread's claim. Blocks while another thread holds
    /// one.
    pub fn acquire() -> Self {
        PAINT_DEPTH.with(|depth| {
            if depth.get() == 0 {
                let guard = PAINT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                PAINT_GUARD.with(|slot| *slot.borrow_mut() = Some(guard));
            }
            depth.set(depth.get() + 1);
        });
        Self(())
    }
}

impl Drop for PaintGuard {
    fn drop(&mut self) {
        PAINT_DEPTH.with(|depth| {
            let remaining = depth.get().saturating_sub(1);
            depth.set(remaining);
            if remaining == 0 {
                PAINT_GUARD.with(|slot| *slot.borrow_mut() = None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing property, asserted directly rather than through the
    /// segfault it prevents: a live [`PaintGuard`] excludes every other
    /// thread, so two threads can never be inside Pango at once.
    ///
    /// RED-first: make `acquire` a no-op that returns `Self(())` without
    /// touching `PAINT_LOCK` and `try_lock` succeeds, failing this.
    #[test]
    fn paint_guard_excludes_other_threads_for_its_whole_lifetime() {
        let guard = PaintGuard::acquire();
        assert!(
            PAINT_LOCK.try_lock().is_err(),
            "a live PaintGuard must exclude other painting threads — \
             concurrent Pango/Cairo text work segfaults inside FT_Load_Glyph"
        );
        drop(guard);

        // No "and now it is released" assertion: the lock is process-wide, so
        // another thread can hold it at any instant and such an assertion
        // would be a flake of its own. A guard that never released would hang
        // the whole suite instead — loud enough without an assertion.
    }

    /// A test that holds two harnesses at once takes this guard twice on one
    /// thread. Without reentrancy that is an instant self-deadlock, so the
    /// nesting is asserted here rather than discovered as a hung suite.
    #[test]
    fn paint_guard_is_reentrant_on_one_thread() {
        let outer = PaintGuard::acquire();
        let inner = PaintGuard::acquire();
        drop(inner);
        assert!(
            PAINT_LOCK.try_lock().is_err(),
            "dropping a nested PaintGuard must not release the outer claim"
        );
        drop(outer);
    }
}
