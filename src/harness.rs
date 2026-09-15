//! Backend-neutral conformance-test harness (#928).
//!
//! Every GUI-driving black-box test in this repo used to be a per-backend
//! copy: `crate::gtk::testing::Harness` (134 `#[test]`s) and
//! `src/macos/mod.rs::mac_driver_tests` (4) each hand-wrote their own
//! wiring from a bare [`Engine`] to a headless driver, and a shared
//! scenario had to be transcribed twice (or, for Win-GUI, not at all — 0
//! tests). quadraui already ships the cross-backend answer —
//! [`quadraui::testing::ConformanceDriver`] — and has since #708/#799; this
//! module is vimcode's first adoption of it (see `GOALS.md` milestone #7).
//!
//! # What lives here vs. in each backend module
//!
//! [`ConformanceHarness`] and the shared scenario bodies below are the
//! **backend-neutral** half: they only ever call
//! [`quadraui::testing::ConformanceDriver`] methods
//! (`type_char`/`type_text`/`press_named`/`click_text`/`screen_has`), never
//! a concrete driver type. The **per-backend** half — actually constructing
//! a `GtkDriver<impl AppLogic>` / `MacDriver<impl AppLogic>` /
//! `WinDriver<impl AppLogic>` around the shared [`crate::app::App`] — is
//! thin wiring that has to live next to each backend's own
//! `driver_with_shell` import: `crate::gtk::testing::conformance_harness`,
//! `crate::macos`'s own test module, and (Windows-only) `crate::win`'s.
//! That split is exactly what the Platform-Neutrality Rule asks for: no
//! decision lives in a backend directory, only the unavoidable "which
//! driver type" plumbing.
//!
//! # Why `App`, not a fresh mock
//!
//! [`crate::app::App`] is already the one backend-neutral `ShellApp` every
//! GUI entry point (`crate::gtk::run`, `crate::macos::run`, `crate::win::run`)
//! hands to `run_with_shell` (#862/#896/#866) — [`App::new_headless_with_backend`]
//! is the test-only constructor that skips its display-dependent prologue.
//! Wrapping *that* in each backend's `driver_with_shell` is what makes a
//! scenario written once here a genuine conformance check: it drives the
//! same dispatch/paint code a real user's backend does, not a shadow copy.
//!
//! # Scope (#928 — the harness, plus a proof slice, not the port)
//!
//! Per CLAUDE.md's "build the harness once, add tests incrementally" rule,
//! this module ships exactly the surface the proof slice below needs — not
//! a restatement of every `Rc`-cloned field `crate::gtk::testing::Harness`
//! exposes. Follow-up issues that port more of the 84 portable GTK
//! scenarios can grow [`ConformanceHarness`] as they need more.

#![cfg(any(test, feature = "test-support"))]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use quadraui::testing::{ConformanceDriver, DriverInput};
use quadraui::NamedKey;

use crate::app::{App, TextMetricsBackend};
use crate::core::Engine;

/// A backend-neutral [`ConformanceDriver`] plus the `Rc` handle to the
/// [`Engine`] it drives, and the two process-wide guards every headless
/// paint-time harness in this repo needs (mirrors
/// `crate::gtk::testing::Harness` / `src/macos/mod.rs::mac_driver_tests`'s
/// own guard pair).
///
/// `engine` is exposed for the same reason `crate::gtk::testing::Harness`
/// exposes it: a driver only ever hands back the opaque `ShellAdapter` it
/// wraps (`GtkDriver::app()`/`MacDriver::app()` reach no further), so a
/// scenario that needs to read past painted text — e.g. to distinguish
/// "closed because nothing happened" from "closed because it timed out" —
/// has nowhere else to look. The proof slice below deliberately does not
/// need it (see its own doc: asserting on rendered output, not populated
/// state, is the CLAUDE.md rule this repo learned the expensive way via
/// #587/#592), but a harness with no way to reach engine state at all would
/// be a strictly worse starting point for whatever ports next.
pub struct ConformanceHarness<D> {
    pub driver: D,
    pub engine: Rc<RefCell<Engine>>,
    /// Held for the harness's whole lifetime — see
    /// `crate::test_paint::PaintGuard`'s own doc for why a headless paint
    /// harness must never run concurrently with another one on a second
    /// thread. Private and deliberately unnamed by any test: construct a
    /// harness through a per-backend `conformance_harness*` function and
    /// the protection comes with it.
    _paint: crate::test_paint::PaintGuard,
    /// Held for the harness's whole lifetime — see
    /// `crate::test_cwd::CwdReadGuard`'s own doc for why a paint harness
    /// must never race a `chdir`-ing test on another thread. Same
    /// "private, comes for free" note as [`Self::_paint`].
    _cwd: crate::test_cwd::CwdReadGuard,
}

impl<D> ConformanceHarness<D> {
    /// Assemble a harness from an already-constructed driver. `pub(crate)`:
    /// only the per-backend `conformance_harness*` functions (one per
    /// backend module) call this — a test never builds one directly, so it
    /// can never forget to acquire the two guards above.
    pub(crate) fn new(
        driver: D,
        engine: Rc<RefCell<Engine>>,
        paint: crate::test_paint::PaintGuard,
        cwd: crate::test_cwd::CwdReadGuard,
    ) -> Self {
        Self {
            driver,
            engine,
            _paint: paint,
            _cwd: cwd,
        }
    }
}

/// Build the backend-neutral [`App`] + [`quadraui::ShellConfig`] pair every
/// per-backend `conformance_harness*` function wraps in its own
/// `driver_with_shell` — the shared half of what
/// `crate::gtk::testing::conformance_harness`,
/// `src/macos/mod.rs`'s conformance constructor, and (Windows-only)
/// `crate::win`'s each do per-backend. Never call `App::new_headless_with_backend`
/// or `App::shell_config` directly from a backend module — this is the one
/// place that pairs them, so the two can never drift out of the order
/// [`App::shell_config`] expects (built from the *same* `App` it derives
/// the config from).
pub(crate) fn build_app_and_config(
    engine: Rc<RefCell<Engine>>,
    backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
) -> (App, quadraui::ShellConfig) {
    let app = App::new_headless_with_backend(engine, backend);
    let config = app.shell_config();
    (app, config)
}

/// Open `dir`'s shared folder/workspace picker (#815) on `app` — the same
/// [`quadraui::FolderPickerController`] a live `:OpenFolder` ex-command
/// builds via `App::open_folder_dialog`, seeded directly here so a
/// scenario can start from "the picker is already open" without depending
/// on the command-line/ex-command path itself. [`command_palette_filters_and_escape_dismisses`]
/// below is the proof-slice scenario that instead exercises an ex-command
/// live (`:CommandPalette`), so between the two, both entry points —
/// pre-seeded field and ex-command — get covered.
///
/// Call before handing `app` to a `driver_with_shell` — `App::folder_picker`
/// is a `RefCell`, so this only needs `&App`, but the picker has to exist
/// before the first frame paints or that frame won't show it.
pub(crate) fn install_folder_picker(app: &App, dir: PathBuf) {
    *app.folder_picker.borrow_mut() =
        Some(quadraui::FolderPickerController::new(dir, vec![], false));
}

// ── Proof slice (#928) ──────────────────────────────────────────────────
//
// Three scenarios, each written once against `ConformanceDriver` and run
// unmodified by every per-backend instantiation. Picked from the 84 GTK
// scenarios the issue found need no pixel probe — interaction flows, not
// coordinate literals (see `quadraui::testing`'s "Rules for shared bodies").
//
// RED-verification (#928's acceptance bar): see the per-backend `#[test]`s'
// own doc comments for the exact mutation each scenario was observed red
// against (e.g. scenario 1 against a disabled `App::apply_folder_picker_event`).

/// Scenario 1: opening the shared folder/workspace picker (#815) paints
/// its entries, typing filters them via the live
/// `FolderPickerController::handle` path, and Esc dismisses it — the exact
/// three things `src/tui_main/shell_app.rs`'s
/// `folder_picker_paints_and_filters_via_shell_app` protects on the TUI
/// side. TUI drives a different `ShellApp` impl (`TuiShellApp`, out of
/// this issue's scope — see `GOALS.md`), so this is new coverage for the
/// GUI backends, not a duplicate of that test.
///
/// `distinctive`/`other` must be two sibling directory names such that a
/// fuzzy-subsequence match of `query` (the full `distinctive` string)
/// against `other` cannot succeed. **Not** "disjoint character sets" — the
/// names the callers below use (`kkxxqq_distinctive_928` /
/// `another_unrelated_dir_928`) share several characters (`d`, `i`, `t`,
/// `n`, `e`, `_`, digits), and that's fine. What actually rules `other` out
/// is that `distinctive` contains characters absent from `other` entirely
/// (`k`, `x`, `q`): a subsequence match needs *every* character of `query`
/// to appear, in order, somewhere in the candidate, so one query character
/// with no match anywhere in `other` is enough to guarantee the miss. A
/// query that happened to also fuzzy-match `other` would make the
/// "filtered out" assertion pass whether or not filtering actually ran.
pub fn folder_picker_filters_and_escape_dismisses<D: ConformanceDriver>(
    driver: &mut D,
    distinctive: &str,
    other: &str,
    query: &str,
) {
    assert!(
        driver.screen_has(distinctive) && driver.screen_has(other),
        "precondition: the picker must paint both directories before any input"
    );

    driver.type_text(query);

    assert!(
        driver.screen_has(distinctive),
        "typing {query:?} must keep the matching entry {distinctive:?} visible"
    );
    assert!(
        !driver.screen_has(other),
        "typing {query:?} must filter out the non-matching entry {other:?}"
    );

    driver.press_named(NamedKey::Escape);

    assert!(
        !driver.screen_has(distinctive),
        "Esc must dismiss the picker"
    );
}

/// Scenario 2: the command palette (`:CommandPalette`,
/// `PickerSource::Commands`) opens via the ordinary ex-command path — no
/// pre-seeded field, unlike scenario 1 — types a filter, filters, and Esc
/// dismisses. Together with scenario 1 this covers both of vimcode's two
/// fuzzy-picker implementations (`FolderPickerController` and the
/// engine-native `PickerSource` machinery) on every GUI backend.
///
/// Filters on `"Sidebar"` against the always-present "View: Toggle
/// Sidebar" / "View: Toggle Terminal" entries (`MENU_STRUCTURE` in
/// `render.rs`, the same source `PickerSource::Commands` and the drawn
/// menu system both read) — present regardless of engine settings, unlike
/// a keybinding-derived entry.
pub fn command_palette_filters_and_escape_dismisses<D: ConformanceDriver>(driver: &mut D) {
    assert!(
        !driver.screen_has("Toggle Sidebar"),
        "precondition: the palette starts closed"
    );

    driver.type_char(':');
    driver.type_text("CommandPalette");
    driver.press_named(NamedKey::Enter);

    assert!(
        driver.screen_has("Toggle Sidebar") && driver.screen_has("Toggle Terminal"),
        "':CommandPalette<CR>' must open the palette and list the app's \
         commands"
    );

    driver.type_text("Sidebar");

    assert!(
        driver.screen_has("Toggle Sidebar"),
        "typing 'Sidebar' must keep the matching entry visible"
    );
    assert!(
        !driver.screen_has("Toggle Terminal"),
        "typing 'Sidebar' must filter out entries that don't match"
    );

    driver.press_named(NamedKey::Escape);

    assert!(
        !driver.screen_has("Toggle Sidebar"),
        "Esc must dismiss the command palette"
    );
}

/// Scenario 3: a **click** — not a keystroke — outside the open folder
/// picker's popup must dismiss it, the same "modal swallows every input,
/// including the click that lands outside it" contract
/// `src/tui_main/shell_app.rs`'s
/// `click_outside_picker_popup_dismisses_it_via_shell_app` protects for
/// the engine-native picker.
///
/// This is the one proof-slice scenario that could not be written against
/// keyboard-only `ConformanceDriver` methods: it exercises
/// `DriverInput::click`'s raw pixel/cell dispatch (every
/// `ConformanceDriver` is also `DriverInput` — see that trait's doc),
/// which is the seam `render::route_folder_picker_click`'s `Dismiss` arm
/// (#815) is meant to prove works identically on every backend.
///
/// Deliberately does **not** assert on *which* row a click over the
/// popup's own rows would select — `route_folder_picker_click`'s row
/// arithmetic divides by `App::cached_line_height`, which the headless
/// harness never refreshes past `ShellApp::setup`'s one-time read (real
/// runs re-sync it every frame via `tick()`, which a driver never pumps —
/// see `crate::gtk::testing`'s own module doc, "No main loop"). On a
/// backend whose reported `Backend::line_height()` at `setup()` time
/// hasn't converged to what it paints with by the first frame, that stale
/// cache makes a row-precise click land on the wrong row — a real gap,
/// filed rather than routed around here (see this issue's PR notes) since
/// fixing it is outside #928's scope. Clicking *outside* the popup needs
/// only the popup's own outer rect, not the per-row math, so it is not
/// exposed to that gap.
///
/// `outside_x`/`outside_y` must land inside the shell's main content area
/// (below/right of the title bar, activity bar and any sidebar chrome) —
/// those regions are dispatched before `crate::app::App::handle` ever
/// sees the click, same as any other window chrome, so a point *within*
/// them would never reach the folder-picker's own click routing at all
/// and this scenario would fail for the wrong reason. Each per-backend
/// caller picks a point near its own surface's bottom-right corner, well
/// clear of the centered popup.
/// Sweep several points inside one painted text run's vertical band and
/// assert every one resolves to the same interactive target (#967).
///
/// #967's bug was exactly this: `App::explorer_ui_event`'s #540 drift guard
/// re-applies the metrics the tree was *painted* with before hit-testing,
/// but macOS's `TextMetricsBackend` impl stubbed both setters, so the
/// hit-test silently ran against `MacBackend::new()`'s default line height
/// instead. `tree_layout`'s row pitch (`(line_height * 1.4).round()`) then
/// disagreed with the painted pitch by a growing, row-index-dependent
/// amount, so a click in the *lower* part of a row's own glyphs resolved to
/// the row below. Every existing click-driven test in this repo (and in
/// quadraui's own conformance scenarios) aims at a run's centre or an
/// `Anchor` edge, which sits inside the slack this bug never ate into — so
/// none of them could have caught it (same family as quadraui#552/#515).
///
/// A run is emitted by exactly one widget for exactly one row, so no point
/// inside its own painted bounds can legitimately resolve to a different
/// target. That makes this check need no per-row expected-value table
/// (unlike a coordinate-literal test) — it can be pointed at any new
/// toggleable row cheaply.
///
/// `needle` locates the run via `inventory().text_runs()` — never a literal
/// coordinate, same rule as every other body in this module. `samples`
/// (>=2) evenly spans the run's full painted vertical bounds, always
/// including the very top and the very bottom edge — a point away from
/// centre is exactly what #967's bug needed to reproduce.
///
/// `fingerprint` reads back whatever painted, binary signal identifies
/// *which* row a click landed on — e.g. "is this directory row's child
/// still painted". Each sample point is clicked once (to observe
/// `fingerprint`) and then clicked again at the identical `(x, y)` to
/// restore state before the next sample, so the caller's toggle must be
/// idempotent under two clicks at the same point; that is true of the
/// explorer's directory-row expand/collapse toggle this issue's own test
/// uses; a scenario whose click isn't a self-restoring toggle needs a
/// different tool than this one.
pub fn sweep_hit_band_integrity<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    needle: &str,
    samples: usize,
    mut fingerprint: impl FnMut(&mut D) -> bool,
) {
    assert!(
        samples >= 2,
        "sweep_hit_band_integrity: need at least 2 samples to compare, got {samples}"
    );
    let bounds = driver
        .inventory()
        .text_runs()
        .iter()
        .find(|r| r.text.contains(needle))
        .map(|r| r.bounds)
        .unwrap_or_else(|| panic!("sweep_hit_band_integrity: {needle:?} not painted"));

    let x = bounds.x + bounds.width / 2.0;
    let top = bounds.y + 0.5;
    let bottom = (bounds.y + bounds.height - 0.5).max(top);

    let mut outcomes = Vec::with_capacity(samples);
    let mut ys = Vec::with_capacity(samples);
    for i in 0..samples {
        let t = i as f32 / (samples - 1) as f32;
        let y = top + (bottom - top) * t;
        driver.click(x, y);
        outcomes.push(fingerprint(driver));
        driver.click(x, y); // restore — see doc above
        ys.push(y);
    }

    let baseline = outcomes[0];
    for (i, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            *outcome,
            baseline,
            "sweep_hit_band_integrity: point {i}/{} inside {needle:?}'s painted \
             band (x={x:.1}, y={:.1}) resolved to a different target than the \
             top of the row (#967 hit-band drift) — outcomes were {outcomes:?} \
             at y-offsets {ys:?}",
            samples - 1,
            ys[i],
        );
    }
}

pub fn folder_picker_click_outside_dismisses_it<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    distinctive: &str,
    other: &str,
    outside_x: f32,
    outside_y: f32,
) {
    assert!(
        driver.screen_has(distinctive) && driver.screen_has(other),
        "precondition: both directories must be painted by the picker"
    );

    driver.click(outside_x, outside_y);

    assert!(
        !driver.screen_has(distinctive) && !driver.screen_has(other),
        "a click outside the popup must dismiss the picker \
         (route_folder_picker_click's Dismiss arm)"
    );
}
