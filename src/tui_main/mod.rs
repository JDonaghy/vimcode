//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.
// #937's quadraui pin bump deprecated `Backend::draw_status_bar` (quadraui#819)
// and `TabBarHits`'s tuple fields (quadraui#823) that this module tree (incl.
// `panels`, `render_impl`, `shell_app`) still uses; migrating to the
// `_interactive`/`TabBarLayout` replacements is an unrelated refactor
// deferred to a follow-up, so it's silenced here rather than left as a stray
// warning under `-D warnings`.
#![allow(
    unused_assignments,
    deprecated,
    clippy::collapsible_match,
    clippy::explicit_counter_loop
)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod app_on_tui_tests;
mod backend;
mod events;
mod mouse;
mod panels;
mod quadraui_tui;
mod render_impl;
mod services;
mod shell_app;

/// #657 test-support seam — the TUI half of what the sealed acceptance suite
/// (`tests/acceptance.rs`, a *separate* crate) needs.
///
/// `shell_app` stays a private module: the only thing published here is
/// [`TuiShellApp`] itself, which is exactly what
/// `quadraui::tui::testing::driver_with_shell` takes. An acceptance slice
/// therefore drives the same `event → handle → render_content` path the
/// in-crate `#[cfg(test)]` suite in `shell_app.rs` does, with no privileged
/// access to internals beyond the public `engine` field.
///
/// Compiled under `cfg(test)` too so the in-crate suite and the sealed suite
/// cannot drift onto different seams.
///
/// # Two TUI arms: `tui` vs `tui_prod` (#1043)
///
/// This module offers **two** `crate::harness::ConformanceHarness`
/// constructors for TUI, and `crate::backend_conformance!` correspondingly
/// has two TUI arms — `tui` and `tui_prod` — not one:
///
/// - [`conformance_harness`] wraps [`crate::app::App`] (the
///   *cross-backend-shared* shell) on `quadraui::tui::TuiBackend`. This is
///   the **control**: it isolates "the two rasterisers disagree" from "the
///   two implementations disagree" by proving the one shared shell paints
///   the same thing on GTK's Cairo surface and on a ratatui `TestBackend`.
///   A scenario failing here has nothing to do with the TUI binary users
///   run — it would fail identically on any other `App`-hosting backend.
/// - [`conformance_harness_prod`] wraps [`TuiShellApp`] — the *actual*
///   production TUI shell `tui_main::run` hands to `driver_with_shell` for
///   real, independently hand-written from `App` (its own mouse routing in
///   `tui_main::mouse`, its own render path in `tui_main::render_impl`,
///   etc.). A scenario green on `gtk` **and** `tui` but red on `tui_prod`
///   is, by construction, the shipped TUI diverging from the shared shell
///   both other arms agree on — not a rasteriser artifact, and not
///   ambiguous about which of the two implementations is at fault.
///
/// Before #1043 only the first existed, so a `crate::harness` scenario
/// could never see the second kind of bug (#1025's right-click-selects-the-
/// row-below regression lived entirely in `tui_main::mouse` and was
/// invisible to every `tui`-arm scenario) — a user had to hit it first.
/// `tui_prod`'s `KNOWN_BUGS`-gated entries in `src/harness.rs` are exactly
/// that inventory, generated mechanically instead of by hand.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    pub use super::shell_app::TuiShellApp;

    // ── #982: TUI wiring for `crate::harness::ConformanceHarness` ──────────
    //
    // GTK, macOS and Win-GUI each have their own `conformance_harness`
    // (`crate::gtk::testing::conformance_harness`, `src/macos/mod.rs`,
    // `src/win/mod.rs`) that wraps the shared `crate::app::App` in that
    // backend's own `driver_with_shell`. Nothing did the same for TUI, which
    // is why no `crate::harness` scenario has ever run against it — this is
    // that missing wiring, thin on purpose (mirrors the other three).
    //
    // Note this is deliberately *not* `TuiShellApp` (the production TUI
    // `ShellApp`, used by `tui_main::run` and this module's own in-crate
    // `#[cfg(test)]` suite in `shell_app.rs`). `crate::app::App`'s `impl
    // quadraui::ShellApp for App` (`src/app.rs`) is unconditionally
    // compiled — no `feature = "gui"` gate on the impl block itself, only on
    // the display-dependent innards it reaches through `dyn
    // quadraui::Backend` — which is exactly what makes it, and not
    // `TuiShellApp`, the *cross-backend-shared* shell a `crate::harness`
    // scenario needs: the same `App` a GTK/macOS/Win conformance test drives
    // is what gets wrapped here, so a scenario written once genuinely
    // exercises the same dispatch/paint code on every backend, rather than
    // running against a second, TUI-only reimplementation.
    //
    // #1043 adds the missing second half — [`conformance_harness_prod`],
    // below — that wraps *that* second, TUI-only reimplementation, so a
    // scenario can finally tell the two apart instead of only ever seeing
    // the first.
    use std::cell::RefCell;
    use std::rc::Rc;

    use quadraui::tui::testing::{driver_with_shell, TuiDriver};
    use quadraui::tui::TuiBackend;

    use crate::app::TextMetricsBackend;
    use crate::core::Engine;
    use crate::harness::ConformanceHarness;

    /// [`TextMetricsBackend`] for quadraui's `TuiBackend` (#982) — the TUI
    /// sibling of `impl TextMetricsBackend for GtkBackend`/`WinBackend`
    /// (`src/app.rs`) and `MacBackend` (`src/macos/mod.rs`).
    ///
    /// Both setters are genuine no-ops, not stubs of the #967/#969 kind:
    /// `TuiBackend::line_height`/`char_width` (`quadraui::Backend` impl)
    /// are hardcoded to `1.0` — one ratatui cell is one row/column by
    /// construction, so there is no pixel metric here for the #540/#819
    /// drift guard to ever disagree with. That is also why TUI is exempt
    /// from `crate::harness::assert_text_metrics_backend_applies_metrics`
    /// (only GTK/macOS/Win call it): that assertion round-trips a probe
    /// value through the setter and back through the getter, which would
    /// necessarily fail here even though nothing is broken — TUI's getters
    /// never vary.
    impl TextMetricsBackend for TuiBackend {
        fn set_current_line_height(&mut self, _line_height: f64) {}
        fn set_current_char_width(&mut self, _char_width: f64) {}
    }

    /// The `TuiDriver` instantiation of `crate::harness::ConformanceHarness`
    /// (#982) — mirrors `crate::gtk::testing::conformance_harness`
    /// (`src/gtk/testing.rs:529`) exactly, modulo the backend-specific
    /// pieces: `TuiBackend` instead of `GtkBackend`, and `width`/`height` in
    /// terminal cells (`u16`) rather than pixels (`i32`), matching
    /// `quadraui::tui::testing::driver_with_shell`'s own signature.
    ///
    /// `TuiDriver` does not implement `quadraui::testing::PixelClickConformance`
    /// (only `GtkDriver`/`MacDriver`/`WinDriver` do — pixel-precise native
    /// click delivery has no ratatui equivalent). A scenario bounded by
    /// `ConformanceDriver + DriverInput` (e.g.
    /// `crate::harness::sweep_hit_band_integrity`, which only needs
    /// `DriverInput::click`) still runs on both; one that needs
    /// `PixelClickConformance` specifically stays GTK-only. See
    /// `crate::harness`'s own module doc for this boundary spelled out once,
    /// rather than re-explained at every call site.
    pub fn conformance_harness(
        engine: Engine,
        width: u16,
        height: u16,
    ) -> ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn TextMetricsBackend>>> =
            Rc::new(RefCell::new(Box::new(TuiBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(Rc::clone(&engine), backend);
        let screen_layout = Rc::clone(&app.cached_screen_layout);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new_with_screen_layout(driver, engine, screen_layout, paint, cwd)
    }

    // ── #1043: TUI wiring for `crate::harness::ConformanceHarness`, wrapping
    // the *production* TUI shell rather than the cross-backend-shared `App`
    // wrapped above ──────────────────────────────────────────────────────
    /// The `TuiDriver<TuiShellApp>` instantiation of
    /// `crate::harness::ConformanceHarness` (#1043) — this module's own
    /// "Two TUI arms" doc (top of file) explains why this exists alongside
    /// [`conformance_harness`] rather than replacing it.
    ///
    /// Built via [`TuiShellApp::from_engine`] called directly on the
    /// caller's fixture `engine` — **not** [`TuiShellApp::new_for_test`]
    /// followed by swapping `app.engine` afterwards, which is what this
    /// function did before #1043's review caught it. That
    /// build-then-swap shape ran `from_engine`'s one-time setup (sidebar
    /// `set_backend_info`, `setup_tui_clipboard`, nerd-font resolution —
    /// see [`TuiShellApp::from_engine`]'s own doc) against a disposable
    /// `Engine::new_for_test()` and then discarded that engine in favour of
    /// the caller's, so none of that setup ever touched the engine the
    /// scenario actually drives. Calling `from_engine` on the caller's
    /// `engine` directly — passing `file_path: None, restore_session:
    /// false`, the same arguments [`TuiShellApp::new_for_test`] uses —
    /// mirrors the same requirement [`conformance_harness`] gets for free
    /// from `App::new_headless_with_backend`, which operates on the
    /// caller's actual `Engine` rather than a throwaway one: a conformance
    /// scenario needs to start from a known fixture, not the machine's real
    /// `~/.config/vimcode`, *and* needs that fixture to be the engine that's
    /// actually wired up. See [`TuiShellApp::new_for_test`]'s own doc for
    /// exactly which two ambient reads `restore_session: false` substitutes
    /// and why [`TuiShellApp::new`] cannot be used here instead.
    ///
    /// `#[cfg(test)]`, unlike [`conformance_harness`] above (reachable under
    /// `feature = "test-support"` alone): every call site this issue adds
    /// lives inside a `#[cfg(test)]`-gated scenario module in
    /// `src/harness.rs`, so this needs no wider reach, and
    /// [`TuiShellApp::new_for_test`] is itself `#[cfg(test)]`-only — widening
    /// *that* constructor's own gate to also serve the sealed acceptance
    /// suite (`tests/acceptance.rs`) is a separate, larger change this
    /// harness-wiring issue does not make.
    ///
    /// # `ConformanceHarness::engine` / `::screen_layout` are not live here
    ///
    /// `App` stores its `Engine` and painted `render::ScreenLayout` cache
    /// behind `Rc<RefCell<_>>` *specifically* so a conformance harness can
    /// keep a handle to either after the app itself is moved into
    /// `driver_with_shell` — see [`ConformanceHarness::screen_layout`]'s own
    /// doc. `TuiShellApp` owns its `Engine` directly (a bare `Engine`, not
    /// `Rc<RefCell<Engine>>`) and caches its own layout in a private,
    /// non-`Rc` `RefCell` — neither is retrievable once `self` is consumed
    /// by `driver_with_shell` below, and changing either field's type to
    /// match `App`'s would be a `shell_app.rs`-wide change (hundreds of
    /// `self.engine.*` call sites) well outside this harness-wiring issue's
    /// scope.
    ///
    /// Concretely: this harness's `engine` field is a disconnected,
    /// freshly-constructed `Engine::new_for_test()` placeholder — the same
    /// "nothing live to hand back" shape [`ConformanceHarness::new`]'s own
    /// doc already documents for every pre-#987 caller — and its
    /// `screen_layout` is `None`. Every scenario registered on the
    /// `tui_prod` arm in `src/harness.rs` is therefore one bounded by
    /// `ConformanceDriver`/`DriverInput` alone (reads painted output via the
    /// `driver`, never `ConformanceHarness::engine`/`::screen_layout`) — a
    /// scenario that needs either (the #987 scrollbar-drag family, #983's
    /// `_resetting` sweep) cannot run against this arm yet; see
    /// `issue_987_group_scrollbar_inert_and_click_resizes`'s and
    /// `issue_983_row_click_selects_the_row_below`'s own module docs in
    /// `src/harness.rs` for that explicit, filed gap.
    #[cfg(test)]
    pub fn conformance_harness_prod(
        engine: Engine,
        width: u16,
        height: u16,
    ) -> ConformanceHarness<TuiDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        // #1043 review: run `from_engine`'s one-time setup (sidebar
        // `set_backend_info`, `setup_tui_clipboard`, nerd-font resolution)
        // directly against the caller's fixture `engine`, rather than
        // against a throwaway `Engine::new_for_test()` that then gets
        // discarded in favour of `engine` — see `from_engine`'s own doc for
        // why that used to leave the scenario's real engine's sidebar
        // systems without `set_backend_info` and its clipboard unset.
        let app = TuiShellApp::from_engine(engine, None, false);
        let config = TuiShellApp::build_shell_config(false);
        let driver = driver_with_shell(app, config, width, height);
        let placeholder_engine = Rc::new(RefCell::new(Engine::new_for_test()));
        ConformanceHarness::new(driver, placeholder_engine, paint, cwd)
    }
}

#[allow(unused_imports)]
use mouse::*;
#[allow(unused_imports)]
use panels::*;
#[allow(unused_imports)]
use quadraui::Backend;
#[allow(unused_imports)]
use render_impl::*;

// ─── Debug logging ────────────────────────────────────────────────────────────

/// Global debug log file handle, set once at startup via `--debug <path>`.
static DEBUG_LOG: std::sync::OnceLock<Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Initialise the debug log.  Call once before the shell runner starts.
fn init_debug_log(path: &str) {
    match std::fs::File::create(path) {
        Ok(f) => {
            let _ = DEBUG_LOG.set(Mutex::new(f));
            // Also enable LSP debug logging (read by the reader thread in lsp.rs).
            std::env::set_var("VIMCODE_LSP_DEBUG", "1");
        }
        Err(e) => {
            eprintln!("Warning: cannot open debug log {path}: {e}");
        }
    }
}

/// Write a formatted message to the debug log (if enabled).  No-op when
/// `--debug` was not passed.
#[allow(unused_macros)]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Some(mtx) = $crate::tui_main::DEBUG_LOG.get() {
            if let Ok(mut f) = mtx.lock() {
                let _ = writeln!(f, $($arg)*);
                let _ = f.flush();
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use debug_log;

use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
// `RColor`/`Modifier` are only referenced by the `#[cfg(test)]` legacy paint
// helpers (`set_cell`, `rc`) now that `event_loop` is gone.
#[cfg(test)]
use ratatui::style::{Color as RColor, Modifier};
// The legacy full-frame paint path (`render_impl::draw_frame` and friends) was
// `#[cfg(test)]` from #634 (which deleted `event_loop`, its only production
// caller) until #766 deleted `draw_frame` itself; `with_frame_scope` below is
// what remains of that scaffolding, still used by `render_impl`'s test
// module, which reaches `Terminal` through this module's `use super::*`.
//
// (#657) The `CrosstermBackend` import that used to sit alongside it is gone:
// nothing has referenced it since #634, and promoting `tui_main` into
// `vimcode_core` moved these tests into the lib test target, which — unlike
// the old `vcd` bin — carries no crate-wide `allow(unused_imports)` to hide
// the dead import.
#[cfg(test)]
use ratatui::Terminal;

use crate::core::engine::{EngineAction, PendingPlatformAction};
use crate::core::window::{GroupDivider, GroupId, SplitDirection};
use crate::core::{Engine, Mode, OpenMode, WindowRect};
use crate::icons;
use crate::render::{self, build_screen_layout, Color, ColorExt, RenderedWindow, Theme};

// ─── Key binding helpers ──────────────────────────────────────────────────────

/// Returns true if the given crossterm key event matches a panel_keys binding string.
/// Binding strings use Vim notation: `<C-b>`, `<C-S-e>`, `<A-x>`.
/// Return the effective content-row count for the terminal panel in the TUI.
pub(super) fn effective_terminal_panel_rows_tui(engine: &Engine, screen_h: u16) -> u16 {
    render::compute_editor_layout(engine, screen_h as f64, 1.0, true).terminal_content_rows
}

/// Max target rows for terminal maximize — delegates to shared layout.
pub(super) fn terminal_target_maximize_rows_tui(engine: &Engine, screen_h: u16) -> u16 {
    render::compute_editor_layout(engine, screen_h as f64, 1.0, true).terminal_max_target_rows
}

/// Terminal panel column count (editor column width, excluding sidebar +
/// activity bar). Matches GTK's `terminal_cols()` which divides the drawing
/// area pixel width by char advance.
pub(super) fn terminal_panel_cols(engine: &Engine, screen_w: u16, sidebar_width: u16) -> u16 {
    let sv = engine.app_shell.sidebar_visible();
    let ab = if engine.settings.autohide_panels && !sv {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let sb = if sv { sidebar_width + 1 } else { 0 };
    screen_w.saturating_sub(ab + sb)
}

// ─── Phase B.4 Stage 6: panel-key accelerator registry ──────────────────────
//
// The 14-entry `PanelAccelerator` id table (`render::ACC_*`) and the
// dispatcher itself (`render::dispatch_panel_accelerator`) are shared with
// GTK (#761 / #734 slice 6) — see the rung's header comment in `render.rs`.
// `TuiAccelHost` (in `shell_app.rs`, next to its call sites) is the five-hook
// impl for the actions that need TUI-local state.

// `register_panel_accelerators` (the 14-entry id table + registration loop)
// moved to `render::register_panel_accelerators` in #823 item 1 — it was
// byte-identical to `app.rs`'s copy and had no backend-specific step.

// ─── Sidebar constants ────────────────────────────────────────────────────────

const SIDEBAR_WIDTH: u16 = 30;
const ACTIVITY_BAR_WIDTH: u16 = 3;

// ─── Activity bar panels ──────────────────────────────────────────────────────

use crate::core::engine::sidebar::*;

// ─── Sidebar data structures ──────────────────────────────────────────────────

struct TuiSidebar {
    has_focus: bool,
    /// When set, sidebar renders an extension panel instead of the fixed panels.
    ext_panel_name: Option<String>,
}

impl TuiSidebar {
    fn new() -> Self {
        TuiSidebar {
            has_focus: false,
            ext_panel_name: None,
        }
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

// `ScrollDragState`, `SidebarScrollDrag`, and `DebugSidebarScrollDrag` were retired
// across Phase B.4 Stages 5c (sidebar / settings / debug-sidebar /
// terminal / debug-output) and 5d (editor v/h scrollbars). Every TUI
// scrollbar drag now flows through the shared `quadraui::DragState`.
// Widget ids route the dispatched offset to the right scroll-state
// field: `tui:search_results`, `tui:settings`, `tui:debug_sidebar:N`,
// `tui:terminal_scrollback`, `tui:debug_output`, and
// `tui:editor:<window_id>:vsb` / `:hsb`.

// The TUI-local `FolderPickerState`/`FolderPickerMode` and their
// `collect_dir_entries`/`filter_dir_entries`/`dir_fuzzy_score` helpers were
// removed in #815: `quadraui::FolderPickerController` (shipped 2026-05-25,
// quadraui#166) is the extracted-verbatim replacement, adopted by both
// backends now — see `shell_app.rs`'s `folder_picker` field and
// `render::folder_picker_popup_rect` / `render::folder_picker_visible_rows`.

// =============================================================================
// Clipboard setup helpers
// =============================================================================

/// Set up system clipboard callbacks on the engine, delegating entirely to
/// `quadraui::tui::TuiPlatformServices` (#508 — quadraui#269/#283).
///
/// The old TUI clipboard spawned xclip/xsel/wl-copy/wl-paste directly (with a
/// stderr-suppression + `DISPLAY=:0` hack and a manual stdin-EOF dance to
/// route around a copypasta_ext bug). All of that lived only to reach the
/// clipboard over SSH/tmux where a local desktop clipboard tool isn't always
/// reachable. `TuiPlatformServices` now covers the same ground upstream in
/// quadraui: arboard for the local desktop clipboard, OSC 52 (written to
/// both stdout and `/dev/tty`, with tmux DCS-passthrough) for SSH/tmux, and
/// a native-tool fallback leg for local-X11-inside-tmux — see
/// `quadraui::tui::services` for the full writeup. Reads stay arboard-only
/// (OSC 52 read is disabled in most terminals for security reasons).
///
/// Compiled only for real builds — see the `cfg(test)` twin below for why the
/// in-crate suite gets a hermetic stand-in instead.
#[cfg(not(test))]
fn setup_tui_clipboard(engine: &mut Engine) {
    use quadraui::PlatformServices;

    let services = std::rc::Rc::new(quadraui::tui::TuiPlatformServices::new());

    let read_services = services.clone();
    engine.clipboard_read = Some(Box::new(move || {
        read_services
            .clipboard()
            .read_text()
            .ok_or_else(|| "clipboard empty or unavailable".to_string())
    }));

    engine.clipboard_write = Some(Box::new(move |text: &str| {
        services.clipboard().write_text(text);
        Ok(())
    }));
}

/// Hermetic per-test stand-in for [`setup_tui_clipboard`].
///
/// `TuiShellApp::new_for_test` funnels through the same `from_engine` body as
/// the production constructor, so until this twin existed every driver-tier
/// test installed the **real** `TuiPlatformServices` clipboard — arboard
/// talking to the live X11/Wayland selection of whatever desktop `cargo test`
/// happens to run on. Two consequences, both bugs:
///
/// * Tests *wrote* the developer's actual clipboard: `sync_tui_clipboard`
///   pushes the unnamed register out after every keypress that yanked.
/// * Tests *read* it back: `p`/`P` in Normal or Visual mode go through
///   `render::preload_paste_clipboard` → `Engine::needs_clipboard_for_paste`,
///   which overwrites the `"` register with whatever the desktop selection
///   holds before the paste runs.
///
/// Together those make every paste test a race against every yank test (and
/// against the human at the keyboard). That is what made
/// `gp_charwise_multiline_lands_cursor_on_rendered_last_pasted_char_via_shell_app`
/// intermittent: it yanks `ab\nc`, but a sibling test's yank reached the X11
/// selection in the window between the `y` and the `p`, so the `gp` preload
/// replaced the register with that sibling's text and pasted a stray
/// character instead.
///
/// The replacement keeps the same round-trip shape — write-then-read returns
/// what was written, so `clipboard=unnamedplus` behaviour is still genuinely
/// exercised — but backs it with a `thread_local!` cell. Rust's test harness
/// gives each test its own thread, so the store is per-test: deterministic
/// under `--test-threads` of any size, and invisible to the host desktop.
///
/// See `clipboard_hermeticity_tests` at the bottom of this file.
#[cfg(test)]
fn setup_tui_clipboard(engine: &mut Engine) {
    thread_local! {
        static TEST_CLIPBOARD: std::cell::RefCell<Option<String>> =
            const { std::cell::RefCell::new(None) };
    }

    engine.clipboard_read = Some(Box::new(|| {
        TEST_CLIPBOARD
            .with(|slot| slot.borrow().clone())
            .ok_or_else(|| "clipboard empty or unavailable".to_string())
    }));

    engine.clipboard_write = Some(Box::new(|text: &str| {
        TEST_CLIPBOARD.with(|slot| *slot.borrow_mut() = Some(text.to_string()));
        Ok(())
    }));
}

/// Copy text to the system clipboard and show a status message.
fn tui_copy_to_clipboard(text: &str, engine: &mut Engine) {
    if let Some(ref cb) = engine.clipboard_write {
        if cb(text).is_ok() {
            engine.message = format!("Copied: {}", text);
            return;
        }
    }
    engine.message = format!("Link: {} (clipboard unavailable)", text);
}

/// Sync the `+` register, falling back to the unnamed `"` register, to the
/// system clipboard if the mirrored content changed. Must be called after
/// every keypress that might have yanked/cut text.
///
/// Thin wrapper — see [`crate::render::sync_register_to_clipboard`] (#1239)
/// for the shared implementation GTK's `App::sync_plus_register_to_clipboard`
/// also delegates to. TUI used to mirror `"` only, so a plain yank/delete
/// after an explicit `+` write (e.g. `"+yy` then a bare `dd` elsewhere) could
/// clobber the clipboard's mirror of the `+` write instead of leaving it in
/// place.
fn sync_tui_clipboard(engine: &mut Engine, last: &mut Option<String>) {
    crate::render::sync_register_to_clipboard(engine, last);
}

/// The TUI entry point: initialise the engine and drive it through
/// `quadraui::tui::shell_runner::run_with_shell`.
///
/// #634 (Stage 6, vimcode#595): this *is* the live path now. It started life
/// in #635 (Stage 6b item F) as `run_via_shell`, a dormant sibling of the
/// hand-rolled `run()`/`event_loop()` pair, precisely so that flipping
/// `main.rs`/`tui_bin.rs` over would be a rename plus a deletion rather than
/// a re-architecture. The old `run()`, `event_loop()` (~2,130 lines) and
/// `restore_terminal()` are gone; `git show 509b8fe:src/tui_main/mod.rs`
/// reads them at their final revision, which is what the `mod.rs:NNNN` line
/// references scattered through `shell_app.rs` point at.
///
/// Keeps the non-loop responsibilities the old `run()` owned — the panic
/// hook, emergency-engine registration, the emergency swap flush, and the
/// custom crash message — around `run_with_shell`.
///
/// Unlike the old `run()`, this does **not** do its own raw-mode / alternate-screen
/// / mouse-capture / keyboard-enhancement terminal setup or teardown:
/// `run_with_shell` → `quadraui::tui::run::run` (`quadraui/src/tui/run.rs`)
/// already does all of that internally (`enable_raw_mode`,
/// `EnterAlternateScreen`, `EnableMouseCapture`, `EnableBracketedPaste`, the
/// kitty keyboard-enhancement push/pop), and always restores the terminal
/// — even on panic, via its own inner `catch_unwind` — before propagating
/// via `resume_unwind`. That's exactly what makes wrapping it in a second,
/// outer `catch_unwind` here safe and sufficient: this closure's
/// `catch_unwind` still observes the same panic payload, with the terminal
/// already back to normal, the same guarantee the old `run()`'s own outer
/// `catch_unwind` relied on around `event_loop`.
///
/// `keyboard_enhanced` (threaded into `render::engine_key_from_ui` for
/// Ctrl-combo disambiguation, #826) and the emergency-engine pointer
/// registration both move
/// into `TuiShellApp::setup` instead of living here — see
/// [`shell_app::TuiShellApp::prepare_for_live_run`] and that `setup`
/// override's doc comments for why: `run_with_shell` takes `app` *by
/// value* and moves it through several stack frames
/// (`build_shell_adapter` → `ShellAdapter`'s own field →
/// `tui::run::run`'s `mut app: A` local) before it settles, so a raw
/// pointer captured here, before that call, would already be stale by the
/// time anything could read it — `setup()` runs only after all of those
/// moves are done.
pub fn run(file_path: Option<PathBuf>, debug_log_path: Option<String>) {
    if let Some(ref path) = debug_log_path {
        init_debug_log(path);
        debug_log!("=== VimCode TUI debug log started ===");
    }

    let mut app = shell_app::TuiShellApp::new(file_path);
    app.prepare_for_live_run();

    // Always install a panic hook that writes crash info to
    // /tmp/vimcode-crash.log AND to the debug log (if --debug is active) —
    // verbatim copy of the deleted `run()`'s own hook.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Emergency: flush swap files for all dirty buffers before
            // anything else, via the pointer `TuiShellApp::setup` registers
            // once `app` reaches its stable live-run address.
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                debug_log!("Crash log written to {}", path.display());
            }
            prev_hook(info);
        }));
    }

    // #557: `live_shell_config`, not the static `shell_config` — plugins have
    // already registered their sidebar panels by the time `App::new` returns,
    // so frame zero can paint their activity-bar icons rather than waiting for
    // the first dispatch's `sync_ext_activity_panels` to add them.
    let config = shell_app::TuiShellApp::live_shell_config(&app.engine);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        quadraui::tui::shell_runner::run_with_shell(app, config);
    }));

    if let Err(e) = result {
        // Unlike the deleted `run()`, there is no locally-owned `engine` to call
        // `emergency_swap_flush()` on directly here — `app` (and its
        // `engine`) moved into `run_with_shell` above and is gone by the
        // time a panic unwinds back to this frame. The panic hook already
        // ran `run_emergency_flush()` via the registered emergency-engine
        // pointer *before* unwinding started (while `engine` was still
        // fully valid), so the flush already happened; this block only
        // reproduces `run()`'s user-facing crash message.
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            format!("VimCode internal error: {s}")
        } else if let Some(s) = e.downcast_ref::<String>() {
            format!("VimCode internal error: {s}")
        } else {
            "VimCode internal error (unknown panic payload)".to_string()
        };
        let crash_path = crate::core::swap::crash_log_path();
        eprintln!("{msg}");
        eprintln!("Unsaved buffers written to swap files for recovery.");
        eprintln!("Crash details written to {}", crash_path.display());
        eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
        std::process::exit(1);
    }
}

// ─── Event loop ───────────────────────────────────────────────────────────────

/// Enter `backend`'s frame scope exactly once for the whole test-harness
/// paint call, while still handing the closure a genuine
/// `&mut ratatui::Frame` for the handful of raw buffer writes (separators,
/// cursor placement, ...) that have no `Backend::draw_*` trait equivalent
/// and are interleaved with trait calls in a z-order-sensitive sequence
/// (#600 Stage 1 — collapsing the ~30 `enter_frame_scope` sites the
/// now-deleted `draw_frame`/`panels.rs` used to open individually down to
/// the one this function makes). #766 deleted `draw_frame`; this helper's
/// one remaining caller is `render_impl::tests::render_tui_buffer_impl`.
///
/// Rust's borrow checker won't let a single closure passed to
/// `TuiBackend::enter_frame_scope(frame, |b| ...)` also capture the
/// outer `frame` binding — `frame` is already consumed as
/// `enter_frame_scope`'s own argument, so referencing it again inside
/// the closure is E0382 (use of moved value). Relaying it through a raw
/// pointer sidesteps that: it's the same type-erasure technique
/// `TuiBackend::enter_frame_scope` already uses internally to smuggle
/// `&mut Frame<'_>` past its own `Cell<*mut ()>` field, just applied one
/// layer higher so `f` can reach both `backend` and `frame` at once.
// #634: legacy full-frame paint scaffolding. `event_loop()` was its only
// production caller; with that gone this is reachable *only* from the
// `#[cfg(test)]` snapshot/assertion suite in `render_impl.rs`, so it is
// compiled out of shipping binaries rather than muted with
// `#[allow(dead_code)]` — the failure mode `src/gtk/draw.rs::draw_editor`
// demonstrated after the #540 GTK cutover (a zero-caller painter kept alive
// behind an `allow`, silently dropping every overlay it drew).
//
// #766 did the first half of #634's hand-off note: `draw_frame` itself —
// the raw-`ratatui::Frame` rasteriser this function used to scope for — is
// deleted, and the test suite that drove it now paints through
// `render_impl::tests::render_tui_buffer_impl`, a thinner walk over the
// same `render::compose_editor_band` / `render::compose_bottom_band`
// artefacts both live `render_content`s run. `with_frame_scope` itself
// survives because that walk still needs *some* `&mut ratatui::Frame` to
// bind `TuiBackend` to (the handful of raw writes noted above have no
// `Backend::draw_*` route either way) — retargeting it at
// `TuiShellApp::render_content` proper (an owned `TuiShellApp` +
// `driver_with_shell`, matching `shell_app.rs`'s own test style) is the
// remaining half, deferred because several of the tests that call
// `render_tui_buffer_impl` mutate `&Engine` again immediately after
// rendering and a `driver_with_shell`-based caller cannot get the engine
// back out to do that (see `render_tui_buffer_impl`'s own doc comment).
#[cfg(test)]
fn with_frame_scope<R>(
    backend: &mut backend::TuiBackend,
    frame: &mut ratatui::Frame<'_>,
    f: impl FnOnce(&mut backend::TuiBackend, &mut ratatui::Frame<'_>) -> R,
) -> R {
    // Reborrow (not move) so `frame` is still available to pass into
    // `enter_frame_scope` below; the raw pointer itself carries no
    // borrow-checker-tracked lifetime.
    let frame_ptr: *mut ratatui::Frame<'_> = &mut *frame as *mut ratatui::Frame<'_>;
    backend.enter_frame_scope(frame, |b| {
        // SAFETY: `frame_ptr` aliases the exact `Frame` `frame` refers
        // to. The outer `frame` binding above is not read again until
        // this closure returns (it was moved into the `enter_frame_scope`
        // call and `enter_frame_scope` itself only touches it through
        // its own type-erased pointer, never dereferencing it while `f`
        // runs — see that function's doc comment), so this is the only
        // live `&mut Frame` in play for the duration of `f`.
        let frame: &mut ratatui::Frame<'_> = unsafe { &mut *frame_ptr };
        f(b, frame)
    })
}

// ─── Explorer context menu action handler ────────────────────────────────────

/// TUI's [`render::ExplorerContextHost`] — the one action
/// [`render::apply_explorer_context_action`] needs backend plumbing for
/// (#1418). `terminal_size` is captured by the caller (the live viewport at
/// the moment the context menu was confirmed) since `Engine` doesn't carry
/// it.
struct TuiExplorerCtxHost {
    terminal_size: Option<Size>,
}

impl render::ExplorerContextHost for TuiExplorerCtxHost {
    fn open_terminal_at(&mut self, engine: &mut Engine, dir: PathBuf) {
        let cols = self.terminal_size.map(|s| s.width).unwrap_or(80);
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_new_tab_at(cols, rows, Some(&dir));
    }
}

#[cfg(test)]
fn set_cell(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, ch: char, fg: RColor, bg: RColor) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        let cell = &mut buf[(x, y)];
        cell.set_char(ch).set_fg(fg).set_bg(bg);
        cell.modifier = Modifier::empty();
        cell.underline_color = RColor::Reset;
    }
}

// ─── Tab bar ──────────────────────────────────────────────────────────────────
// Tab/diff constants are defined in render_impl.rs and re-exported via `use render_impl::*;`.

// #826: `shift_map_us`, `tui_key_to_engine_name` and `translate_key` used to
// live here — a TUI-only re-decode of a crossterm `KeyEvent` synthesised back
// out of the `quadraui::Key` the runner had already decoded (a pure round
// trip). All three are now one function, [`render::engine_key_from_ui`],
// typed against `quadraui::Key`/`Modifiers` directly; both backends call it.

// ─── Engine action handling ───────────────────────────────────────────────────

fn handle_action(engine: &mut Engine, action: EngineAction) -> bool {
    match action {
        EngineAction::Quit | EngineAction::SaveQuit => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            true
        }
        EngineAction::OpenFile(path) => {
            if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                engine.message = e;
            }
            false
        }
        EngineAction::OpenTerminal | EngineAction::RunInTerminal(_) => false, // TUI handles terminal open in main event loop
        EngineAction::ToggleTerminalMaximize => false, // TUI handles in main event loop (needs viewport rows)
        EngineAction::OpenFolderDialog
        | EngineAction::OpenWorkspaceDialog
        | EngineAction::SaveWorkspaceAsDialog
        | EngineAction::OpenRecentDialog => false, // handled by caller
        EngineAction::QuitWithUnsaved => false, // handled by caller (shows quit confirm overlay)
        EngineAction::ToggleSidebar => false,   // engine handles internally; no-op here
        EngineAction::QuitWithError => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            std::process::exit(1);
        }
        EngineAction::OpenUrl(url) => {
            // #1134: queue rather than shell out here — this fn has no
            // `backend` handle. `TuiShellApp::tick` drains
            // `pending_platform_actions` through `PlatformServices`
            // (`is_safe_url` was already applied by whichever engine path
            // produced this `EngineAction`, e.g. `panels.rs`'s
            // `open_ext_url:` handler).
            engine
                .pending_platform_actions
                .push(PendingPlatformAction::OpenUrl(url));
            false
        }
        EngineAction::None | EngineAction::Error => false,
    }
}

/// Thin wrapper kept so this file's several call sites don't all need
/// rewriting to `engine.save_session_state()` — the actual body moved to
/// [`crate::core::engine::Engine::save_session_state`] in #823 item 5 (it
/// was the same ~20 lines as `app.rs`'s `save_session_and_exit`, modulo
/// GTK's window-size capture and shutdown epilogue).
fn save_session(engine: &mut Engine) {
    engine.save_session_state();
}

// ─── Color / index helpers ───────────────────────────────────────────────────

#[cfg(test)]
fn rc(c: Color) -> RColor {
    RColor::Rgb(c.r, c.g, c.b)
}

// #826: the `translate_key_tests` module that used to live here moved to
// `render::engine_key_from_ui_tests` alongside the function it now tests.

// ─── Clipboard hermeticity (#197 follow-up) ─────────────────────────────────

/// Coverage for the `cfg(test)` [`setup_tui_clipboard`] twin.
///
/// These live here rather than in `shell_app.rs`'s suite because the unit
/// under test is this file's clipboard wiring — the thing `from_engine`
/// installs on *every* `TuiShellApp::new_for_test`.
#[cfg(test)]
mod clipboard_hermeticity_tests {
    use crate::tui_main::shell_app::TuiShellApp;
    use quadraui::tui::testing::driver_with_shell;

    /// How long to let a *shared* clipboard propagate before concluding the
    /// one under test is not shared. `write_text` is asynchronous on X11 — it
    /// hands off to arboard's selection-owner thread and returns before the
    /// selection has actually changed hands — so a single read straight after
    /// a sibling's write races it and can miss contamination that is about to
    /// arrive. Polling for this long makes "the sibling's text never shows up
    /// here" a real assertion rather than a won race.
    const PROPAGATION_WINDOW: std::time::Duration = std::time::Duration::from_millis(600);
    const POLL_STEP: std::time::Duration = std::time::Duration::from_millis(10);

    /// Poll `read` for `PROPAGATION_WINDOW`, returning `true` as soon as it
    /// yields `wanted`.
    fn clipboard_shows(read: &dyn Fn() -> Result<String, String>, wanted: &str) -> bool {
        let deadline = std::time::Instant::now() + PROPAGATION_WINDOW;
        loop {
            if read().ok().as_deref() == Some(wanted) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL_STEP);
        }
    }

    /// Minimal shell config — mirrors `shell_app.rs`'s test-local `config()`
    /// (1-row title bar, one panel) so geometry matches the live config.
    fn config() -> quadraui::ShellConfig {
        let mut cfg = quadraui::ShellConfig::new(
            "VimCode",
            vec![quadraui::PanelDefinition {
                id: quadraui::WidgetId::new("panel:explorer"),
                title: "Explorer".to_string(),
                icon: String::new(),
                tooltip: String::new(),
            }],
        );
        cfg.title_bar_height_lh = 1.0;
        cfg
    }

    /// A sibling test app that has yanked `text` and is **still alive**.
    ///
    /// Staying alive matters. The pre-fix clipboard was arboard, whose X11
    /// backend owns the selection from a helper thread tied to the live
    /// `TuiPlatformServices` object; let the sibling drop first and the
    /// selection evaporates with it, so the contamination these tests are
    /// about vanishes before they can see it. (A first draft of this helper
    /// joined the thread immediately and consequently passed against the very
    /// bug it exists to catch.) Holding the sibling open is also the honest
    /// shape of the bug: `cargo test` runs ~2.6k tests across threads, so the
    /// yanking test is *concurrent* with the pasting one, not finished first.
    ///
    /// Dropping the handle releases the sibling and joins it.
    struct LiveSiblingYank {
        release: Option<std::sync::mpsc::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl LiveSiblingYank {
        fn new(text: &'static str) -> Self {
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
            let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
            let thread = std::thread::spawn(move || {
                let mut sibling = TuiShellApp::new_for_test();
                // Exactly what `sync_tui_clipboard` does after a yank.
                let write = sibling
                    .engine
                    .clipboard_write
                    .take()
                    .expect("new_for_test must install a clipboard_write hook");
                write(text).expect("clipboard write must succeed");
                ready_tx.send(()).ok();
                // Keep `write` — and with it the services object that owns the
                // selection — alive until the test says otherwise.
                release_rx.recv().ok();
                drop(write);
                drop(sibling);
            });
            ready_rx
                .recv()
                .expect("sibling clipboard thread panicked before yanking");
            Self {
                release: Some(release_tx),
                thread: Some(thread),
            }
        }
    }

    impl Drop for LiveSiblingYank {
        fn drop(&mut self) {
            drop(self.release.take());
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// The clipboard a test app gets must round-trip its *own* writes and stay
    /// invisible to a concurrently-running test app — no shared desktop
    /// selection, no process-global store.
    ///
    /// **Verified RED against unfixed `develop`, on any machine:**
    /// * With a desktop session (`DISPLAY`/`WAYLAND_DISPLAY` set) both apps
    ///   shared the one real X11/Wayland selection, so the live sibling's
    ///   marker showed up here inside the propagation window and the
    ///   isolation assertion tripped.
    /// * Headless, the real `TuiPlatformServices` write leg has nowhere to
    ///   land (OSC 52 goes to a stdout nobody reads) and the arboard read
    ///   fails, so the round-trip assertion tripped instead.
    #[test]
    fn test_app_clipboard_round_trips_locally_and_is_isolated_per_thread() {
        const SIBLING: &str = "ZQXW197_SIBLING_YANK";
        const MINE: &str = "ZQXW197_MY_YANK";

        let mut app = TuiShellApp::new_for_test();
        let write = app
            .engine
            .clipboard_write
            .take()
            .expect("new_for_test must install a clipboard_write hook");
        let read = app
            .engine
            .clipboard_read
            .take()
            .expect("new_for_test must install a clipboard_read hook");

        // Round-trip: `clipboard=unnamedplus` behaviour is still genuinely
        // exercised, so what follows is isolation and not a dead no-op hook.
        write(MINE).expect("clipboard write must succeed");
        assert_eq!(
            read().ok().as_deref(),
            Some(MINE),
            "a test app must read back its own clipboard write"
        );

        // Isolation: a concurrently-running test app's yank must never become
        // visible here, however long we give it to propagate.
        let _sibling = LiveSiblingYank::new(SIBLING);
        assert!(
            !clipboard_shows(&|| read(), SIBLING),
            "a test app must not see a concurrently-running test app's clipboard \
             write — that shared selection is what made paste tests race yank tests"
        );
        assert_eq!(
            read().ok().as_deref(),
            Some(MINE),
            "and our own write must still be what we read back"
        );
    }

    /// End-to-end proof through the rendered screen: `gp` pastes the text
    /// *this* app yanked, never a concurrently-running app's.
    ///
    /// `p`/`P` in Normal mode run `render::preload_paste_clipboard`, which
    /// overwrites the `"` register from the clipboard *before* the paste
    /// happens. With the pre-fix real-desktop clipboard, any other test
    /// yanking in that window refilled the register — the intermittent
    /// failure of
    /// `gp_charwise_multiline_lands_cursor_on_rendered_last_pasted_char_via_shell_app`,
    /// which pasted a stray character a sibling test had left in the X11
    /// selection instead of the `ab\nc` it had just yanked itself.
    ///
    /// Note the ordering below: the sibling has to poison the clipboard
    /// *between* this app's `y` and its `p`. Yank first and the poison is
    /// simply overwritten by our own `sync_tui_clipboard` push, which is why
    /// an earlier draft of this test passed against the bug.
    ///
    /// **Verified RED against unfixed `develop`** on a machine with a desktop
    /// clipboard: `ZQXW197POISON` reached the buffer and painted on screen.
    /// (Headless the bug cannot manifest at all — which is why CI never caught
    /// it and only developer machines saw the flake.)
    #[test]
    fn gp_pastes_own_yank_not_a_concurrent_apps_clipboard_via_shell_app() {
        const POISON: &str = "ZQXW197POISON";

        // A same-thread probe onto whatever clipboard `new_for_test` installs.
        // `driver.app()` reaches the shell adapter, not `TuiShellApp`, so this
        // is how the test observes what the driven app's own paste hook will
        // see a moment later.
        let probe = TuiShellApp::new_for_test()
            .engine
            .clipboard_read
            .take()
            .expect("new_for_test must install a clipboard_read hook");

        let mut app = TuiShellApp::new_for_test();
        app.engine.buffer_mut().insert(0, "ab\ncd");

        let mut driver = driver_with_shell(app, config(), 100, 24);
        driver.press_named(quadraui::NamedKey::Escape);
        driver.render();

        // v j y: charwise-yank "ab\nc" into the unnamed register.
        for c in ['v', 'j', 'y'] {
            driver.type_char(c);
        }

        // ...now a concurrent test app yanks something else, and we wait for
        // that to become visible on a shared clipboard (it never does on a
        // hermetic one).
        let _sibling = LiveSiblingYank::new(POISON);
        clipboard_shows(&probe, POISON);

        // $ gp: paste our own yank back after the cursor.
        for c in ['$', 'g', 'p'] {
            driver.type_char(c);
        }
        driver.render();

        let screen = driver.screen();
        assert!(
            !screen.contains(POISON),
            "gp must paste this app's own yank — a concurrent app's clipboard \
             text must never reach the buffer; screen:\n{screen}"
        );
        assert!(
            screen.contains("abab"),
            "gp should have pasted the charwise-yanked \"ab\\nc\" after the \
             cursor, splicing \"ab\" onto line 0; screen:\n{screen}"
        );
    }
}
