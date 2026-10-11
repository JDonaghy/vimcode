//! Backend-neutral conformance-test harness.
//!
//! This module consolidates shared scenario bodies that every GUI-driving
//! black-box test runs through [`ConformanceHarness`], eliminating the
//! per-backend duplication that used to split test logic across
//! `crate::gtk::testing::Harness`, `src/macos/mod.rs::mac_driver_tests`,
//! and others. Each scenario is written once against
//! [`quadraui::testing::ConformanceDriver`] methods, never backend-specific
//! code, then registered on every backend via [`backend_conformance!`].
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
//! `crate::macos`'s own test module, and `crate::win`'s
//! (`src/win/mod.rs::win_driver_tests`). That split is exactly what the
//! Platform-Neutrality Rule asks for: no decision lives in a backend
//! directory, only the unavoidable "which driver type" plumbing.
//!
//! `crate::win`'s arm compiles off Windows under `feature = "win"` alone:
//! quadraui's `win::testing` module is not gated on `target_os = "windows"`
//! but stubbed per WinAPI call the way `win::backend`/`run`/`shell_runner`
//! are. Each backend's `ConformanceHarness` instantiation is verified on
//! every host via `cargo check --tests`, only *run* on its native platform.
//!
//! # Why `App`, not a fresh mock
//!
//! [`crate::app::App`] is already the one backend-neutral `ShellApp` every
//! GUI entry point (`crate::gtk::run`, `crate::macos::run`, `crate::win::run`)
//! hands to `run_with_shell` — [`App::new_headless_with_backend`]
//! is the test-only constructor that skips its display-dependent prologue.
//! Wrapping *that* in each backend's `driver_with_shell` is what makes a
//! scenario written once here a genuine conformance check: it drives the
//! same dispatch/paint code a real user's backend does, not a shadow copy.
//!
//! # Scope — the harness plus scenarios, not a full port
//!
//! This module ships the surface each added scenario needs — not
//! a restatement of every `Rc`-cloned field `crate::gtk::testing::Harness`
//! exposes. Follow-up work that ports more scenarios can grow
//! [`ConformanceHarness`] as needed.
//!
//! # Which trait bound a scenario needs
//!
//! Every backend's `conformance_harness*` constructor hands back a
//! [`quadraui::testing::ConformanceDriver`] (keyboard-only: `type_char`/
//! `type_text`/`press_named`/`screen_has`/`inventory`). GTK's, macOS's and
//! Win-GUI's drivers *additionally* implement
//! [`quadraui::testing::DriverInput`] (raw pixel `click`) and
//! `quadraui::testing::PixelClickConformance` (native, coordinate-precise
//! click delivery down to the OS widget). **TUI's `TuiDriver` implements
//! `DriverInput` but not `PixelClickConformance`** — ratatui has no OS
//! widget layer for the latter to mean anything.
//!
//! That makes the bound on a scenario's own generic parameter the boundary
//! between "runs on every backend" and "GTK/macOS/Win-only", not a
//! per-backend `#[cfg]`:
//!
//! - `fn scenario<D: ConformanceDriver>(driver: &mut D)` — keyboard-driven
//!   (e.g. [`folder_picker_filters_and_escape_dismisses`],
//!   [`command_palette_filters_and_escape_dismisses`]) — runs on **every**
//!   backend, TUI included.
//! - `fn scenario<D: ConformanceDriver + DriverInput>(driver: &mut D)` —
//!   needs a raw click ([`sweep_hit_band_integrity`],
//!   [`folder_picker_click_outside_dismisses_it`]) — still runs on
//!   **every** backend, TUI included: `DriverInput` is the click bound both
//!   sides share.
//! - A scenario that needs `PixelClickConformance` specifically (native
//!   widget-precise click delivery) is GTK/macOS/Win-only by construction —
//!   write it with that bound and the [`backend_conformance!`] macro simply
//!   has no `tui` arm to offer it.
//!
//! Reach for the narrowest bound the scenario actually needs, not the
//! widest available on the backend you happen to be testing against first
//! — a `DriverInput`-bounded body written against `GtkDriver` costs nothing
//! extra to also run on `TuiDriver` via [`crate::tui_main::testing::conformance_harness`].
//!
//! # `tui` vs `tui_prod` — two arms, one shell since #1434
//!
//! [`backend_conformance!`] still registers two TUI arms:
//!
//! - `tui` → [`crate::tui_main::testing::conformance_harness`], wrapping
//!   [`App`] (the cross-backend-shared shell every other arm also wraps) on
//!   `quadraui::tui::TuiBackend`.
//! - `tui_prod` → [`crate::tui_main::testing::conformance_harness_prod`], a
//!   thin alias of the same [`crate::tui_main::testing::conformance_harness`].
//!
//! Both arms now build the identical `App`. Both names are kept (rather
//! than collapsing back to one arm) so existing `tui_prod`-suffixed
//! scenarios and `KNOWN_BUGS` labels keep resolving unchanged. `tui_prod`
//! still cannot accept every scenario — see
//! [`crate::tui_main::testing::conformance_harness_prod`]'s own doc for
//! which trait bounds it satisfies.

#![cfg(any(test, feature = "test-support"))]

/// The shared plugin-panel fixture and the six painted-geometry
/// scenarios built on it. A submodule rather than more of this file
/// because it owns a fixture (a genuine `PanelRegistration`, not the
/// marketplace) as well as scenario bodies, and because its per-lane
/// registration is hand-written rather than `backend_conformance!`-driven —
/// see its own module doc.
///
/// `#[cfg(test)]`, narrower than this module's own
/// `any(test, feature = "test-support")` gate, for the same reason
/// `crate::tui_main::testing::conformance_harness_prod` is: the fixture is
/// built on `Engine::new_for_test`, which is itself `#[cfg(test)]`-only,
/// and the `tui_prod` arm — the one lane these scenarios are green on —
/// can only be constructed through that `#[cfg(test)]` constructor anyway.
/// Widening either gate to also serve the sealed acceptance suite
/// (`tests/*.rs`, which compiles against `feature = "test-support"`) is a
/// separate change.
#[cfg(test)]
pub mod plugin_panel;

/// The plugin-declared **view** (`vimcode.ui.register_view`) fixture +
/// scenarios. Sibling of [`plugin_panel`], which covers the older
/// `vimcode.panel.register` tree panels.
#[cfg(test)]
pub mod plugin_view;

/// Driver-tier black-box tests for the bundled
/// `contrib/extensions/rest-client` extension — loads the real on-disk
/// Lua (not a synthetic fixture) via the same `PluginManager` harvest
/// [`plugin_view`] uses. See that module's own doc for why `#[cfg(test)]`
/// is the right (narrower) gate here too.
#[cfg(test)]
pub mod rest_client;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use quadraui::testing::{ConformanceDriver, DriverInput};
use quadraui::NamedKey;

use crate::app::App;
use crate::core::Engine;

/// True if `needle` appears in the painted text of any single screen **row**,
/// that row's runs joined left-to-right first.
///
/// The row-aware twin of [`ConformanceDriver::screen_has`], which matches
/// within *one painted run*. A run is one Pango run / one styled span, not
/// one logical row — quadraui's own `GtkDriver::painted_texts` doc spells
/// this out ("for multi-span content … one entry *per span*") — so a phrase
/// that straddles a style change is invisible to `screen_has` even though
/// it is plainly on screen. That is not hypothetical: the fuzzy pickers
/// paint each row's *matched* substring as its own span, so the moment a
/// scenario types a filter, `"View: Toggle Sidebar"` becomes the two runs
/// `"View: Toggle "` + `"Sidebar"` and `"File: Open Folder…"` becomes
/// `"File: "` + `"Open Folder"` + `"…"`.
///
/// Still an assertion on *rendered output*, not on state — it reads the
/// same painted runs `screen_has` does, only grouped the way the user sees
/// them. Rows are keyed on exact painted `y`, the same equality the tab-bar
/// row locators in this file use.
pub fn screen_row_has<D: ConformanceDriver>(driver: &D, needle: &str) -> bool {
    let inventory = driver.inventory();
    let mut rows: std::collections::HashMap<u32, Vec<(f32, String)>> = Default::default();
    for run in inventory.text_runs() {
        rows.entry(run.bounds.y.to_bits())
            .or_default()
            .push((run.bounds.x, run.text.clone()));
    }
    rows.values_mut().any(|row| {
        row.sort_by(|a, b| a.0.total_cmp(&b.0));
        row.iter()
            .map(|(_, t)| t.as_str())
            .collect::<String>()
            .contains(needle)
    })
}

/// Paint one frame *without* dispatching an input event — the harness
/// stand-in for the runner's "the idle tick reported a redraw, so paint".
///
/// Every `ConformanceDriver` in quadraui repaints only when the app's own
/// dispatch returns [`quadraui::Reaction::Redraw`], and none of them expose
/// a tick on the shared trait (`TuiDriver::tick` exists; `GtkDriver` /
/// `MacDriver` have no equivalent). So a backend-neutral scenario that
/// advances *time-driven* engine state by hand — calling
/// `Engine::poll_panel_hover` instead of sleeping out a real hover dwell —
/// has no way to get that state onto the screen.
///
/// Such scenarios ask for the frame explicitly, which is also what the real
/// runner does — `Engine::poll_idle` returns `true` and the runner paints.
///
/// Implemented here rather than upstream because the trait is local; each
/// driver already has a public, inherent `render()`.
pub trait DriverRepaint {
    /// Re-render the current app state into this driver's frame buffer.
    fn repaint(&mut self);
}

impl<A: quadraui::AppLogic> DriverRepaint for quadraui::tui::testing::TuiDriver<A> {
    fn repaint(&mut self) {
        self.render();
    }
}

#[cfg(feature = "gui")]
impl<A: quadraui::AppLogic> DriverRepaint for quadraui::gtk::testing::GtkDriver<A> {
    fn repaint(&mut self) {
        self.render();
    }
}

#[cfg(all(feature = "macos", target_os = "macos"))]
impl<A: quadraui::AppLogic> DriverRepaint for quadraui::macos::testing::MacDriver<A> {
    fn repaint(&mut self) {
        self.render();
    }
}

/// A backend-neutral [`ConformanceDriver`] plus the `Rc` handle to the
/// [`Engine`] it drives, and the two process-wide guards every headless
/// paint-time harness needs.
///
/// `engine` is exposed for scenarios that need to read past painted text —
/// e.g. to distinguish "closed because nothing happened" from "closed
/// because it timed out". A driver only ever hands back the opaque
/// `ShellAdapter` it wraps, so engine state is the only way to reach
/// further than the painted output.
pub struct ConformanceHarness<D> {
    pub driver: D,
    pub engine: Rc<RefCell<Engine>>,
    /// The `App`'s own painted `render::ScreenLayout` cache, or
    /// `None` if the harness was built via [`Self::new`] rather than
    /// [`Self::new_with_screen_layout`] (every existing caller predating
    /// The field defaults to empty for them rather than making this
    /// a breaking change to [`Self::new`]'s signature).
    ///
    /// `crate::gtk::testing::Harness::screen_layout` is the GTK-only
    /// precedent this mirrors (`RenderedWindow.rect` per window, `f32`
    /// pixels there vs `f32` cells here on TUI) — the difference is this
    /// field is on the backend-neutral [`ConformanceHarness`] itself, so a
    /// scenario that needs a window's own painted rect (not just a text
    /// run's) can do so on *either* backend, not just GTK. Needed because
    /// a scrollbar thumb has no text to hand to
    /// `inventory().text_runs()` — locating it means locating the *window*
    /// it belongs to instead, then deriving the thumb's own band from that
    /// window's `RenderedWindow` fields (`total_lines`, `scroll_top`, …)
    /// the same arithmetic `quadraui::Editor::layout`/`fit_thumb` uses.
    pub screen_layout: Rc<RefCell<Option<crate::render::ScreenLayout>>>,
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
    ///
    /// [`Self::screen_layout`] is left empty (`None`, and never populated —
    /// there is no live `App` handle left to clone it from after
    /// construction) — callers that need it use
    /// [`Self::new_with_screen_layout`] instead.
    pub(crate) fn new(
        driver: D,
        engine: Rc<RefCell<Engine>>,
        paint: crate::test_paint::PaintGuard,
        cwd: crate::test_cwd::CwdReadGuard,
    ) -> Self {
        Self {
            driver,
            engine,
            screen_layout: Rc::new(RefCell::new(None)),
            _paint: paint,
            _cwd: cwd,
        }
    }

    /// [`Self::new`], plus a live [`Self::screen_layout`] handle —
    /// `screen_layout` must be `Rc::clone`d from the `App`'s own
    /// `cached_screen_layout` field *before* that `App` is moved into
    /// `driver_with_shell` (see `crate::gtk::testing::conformance_harness`
    /// for the call-site shape this expects).
    pub(crate) fn new_with_screen_layout(
        driver: D,
        engine: Rc<RefCell<Engine>>,
        screen_layout: Rc<RefCell<Option<crate::render::ScreenLayout>>>,
        paint: crate::test_paint::PaintGuard,
        cwd: crate::test_cwd::CwdReadGuard,
    ) -> Self {
        Self {
            driver,
            engine,
            screen_layout,
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
///
/// `units` is the one place this shared builder is *not*
/// backend-neutral: the caller already knows which `quadraui::Backend` it's
/// wrapping (`GtkBackend`, `MacBackend`, `TuiBackend`, ...), so it passes the
/// matching [`crate::render::UnitProfile`] — `px()` from
/// `crate::gtk::testing::conformance_harness`/`src/macos/mod.rs`/
/// `src/win/mod.rs`, `cell()` from
/// `crate::tui_main::testing::conformance_harness`.
pub(crate) fn build_app_and_config(
    engine: Rc<RefCell<Engine>>,
    backend: Rc<RefCell<Box<dyn quadraui::Backend>>>,
    units: crate::render::UnitProfile,
) -> (App, quadraui::ShellConfig) {
    let app = App::new_headless_with_backend(engine, backend, units);
    let config = app.shell_config();
    (app, config)
}

/// Open `dir`'s shared folder/workspace picker on `app` — the same
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

/// Name of the one conflicted file [`make_conflicted_repo`] leaves behind.
/// Short on purpose — the SC sidebar is 30 cells wide on TUI, and a row
/// that gets truncated is a row a `find_bounds`/`screen_has` assertion
/// can't see.
pub const CONFLICT_FIXTURE_FILE: &str = "zqxw991conf.txt";

/// Build a throwaway git repo in a temp dir with **one real merge
/// conflict** (`UU` — both modified, the common case), created with plain
/// `git` exactly as a user would hit it: two branches editing the same
/// line, then a failing `git merge`.
///
/// `tag` disambiguates concurrently-running callers; the returned path
/// additionally carries the pid and a per-process counter, so two
/// backends' arms of the same scenario never collide on disk.
///
/// Panics if `git` is unavailable or the merge does *not* conflict — a
/// silently-clean fixture would make every assertion built on it pass
/// vacuously.
pub fn make_conflicted_repo(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);

    let dir = std::env::temp_dir().join(format!(
        "vimcode_991_{tag}_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp fixture dir");

    let git = |args: &[&str]| -> String {
        let out = std::process::Command::new("git")
            .args([
                "-c",
                "user.email=vimcode991@example.com",
                "-c",
                "user.name=VimCode 991",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(&dir)
            .output()
            .expect("git must be runnable for the #991 conflict fixture");
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let file = dir.join(CONFLICT_FIXTURE_FILE);
    git(&["init"]);
    std::fs::write(&file, "base\n").expect("write fixture file");
    git(&["add", "."]);
    git(&["commit", "-m", "base"]);
    // Read the default branch name back rather than assuming `main` —
    // `init.defaultBranch` is ignored by git < 2.28, where it is `master`.
    let base_branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();

    git(&["checkout", "-b", "zqxw991-other"]);
    std::fs::write(&file, "theirs\n").expect("write fixture file");
    git(&["commit", "-am", "theirs"]);

    git(&["checkout", &base_branch]);
    std::fs::write(&file, "ours\n").expect("write fixture file");
    git(&["commit", "-am", "ours"]);

    // Expected to fail — that failure *is* the fixture.
    git(&["merge", "zqxw991-other"]);

    let porcelain = git(&["status", "--porcelain"]);
    assert!(
        porcelain.contains("UU"),
        "the #991 fixture must leave a real UU merge conflict behind, but \
         `git status --porcelain` in {} said {porcelain:?}",
        dir.display()
    );
    dir
}

pub(crate) fn install_folder_picker(app: &App, dir: PathBuf) {
    *app.folder_picker.borrow_mut() =
        Some(quadraui::FolderPickerController::new(dir, vec![], false));
}

/// Scenario 1: opening the shared folder/workspace picker paints
/// its entries, typing filters them via the live
/// `FolderPickerController::handle` path, and Esc dismisses it.
///
/// `distinctive`/`other` must be two sibling directory names such that a
/// fuzzy-subsequence match of `query` (the full `distinctive` string)
/// against `other` cannot succeed. What rules `other` out is that
/// `distinctive` contains characters absent from `other` entirely: a
/// subsequence match needs *every* character of `query` to appear, in
/// order, somewhere in the candidate, so one query character with no match
/// anywhere in `other` is enough to guarantee the miss.
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
/// `PickerSource::Commands`) opens via the ordinary ex-command path and
/// types a filter, filters, and Esc dismisses. Together with scenario 1
/// this covers both of vimcode's two fuzzy-picker implementations
/// (`FolderPickerController` and the engine-native `PickerSource` machinery)
/// on every GUI backend.
///
/// Filters on `"Sidebar"` against the always-present "View: Toggle
/// Sidebar" / "View: Toggle Terminal" entries (`MENU_STRUCTURE` in
/// `render.rs`, the same source `PickerSource::Commands` and the drawn
/// menu system both read). Asserted with [`screen_row_has`] rather than
/// [`ConformanceDriver::screen_has`] from the filter step on: once a query
/// narrows the list, the picker paints each row's *matched* substring as
/// its own styled span, so a single-run substring match can no longer see
/// the phrase.
pub fn command_palette_filters_and_escape_dismisses<D: ConformanceDriver>(driver: &mut D) {
    assert!(
        !screen_row_has(driver, "Toggle Sidebar"),
        "precondition: the palette starts closed"
    );

    driver.type_char(':');
    driver.type_text("CommandPalette");
    driver.press_named(NamedKey::Enter);

    assert!(
        screen_row_has(driver, "Toggle Sidebar") && screen_row_has(driver, "Toggle Terminal"),
        "':CommandPalette<CR>' must open the palette and list the app's \
         commands"
    );

    driver.type_text("Sidebar");

    assert!(
        screen_row_has(driver, "Toggle Sidebar"),
        "typing 'Sidebar' must keep the matching entry visible"
    );
    assert!(
        !screen_row_has(driver, "Toggle Terminal"),
        "typing 'Sidebar' must filter out entries that don't match"
    );

    driver.press_named(NamedKey::Escape);

    assert!(
        !screen_row_has(driver, "Toggle Sidebar"),
        "Esc must dismiss the command palette"
    );
}

/// Scenario 3: a **click** — not a keystroke — outside the open folder
/// picker's popup must dismiss it.
///
/// This is the one proof-slice scenario that could not be written against
/// keyboard-only `ConformanceDriver` methods: it exercises
/// `DriverInput::click`'s raw pixel/cell dispatch, which is the seam
/// `render::route_folder_picker_click`'s `Dismiss` arm is meant to prove
/// works identically on every backend.
///
/// Deliberately does **not** assert on *which* row a click over the
/// popup's own rows would select — a real gap exists when the headless
/// harness's cached line height hasn't converged to what it paints with.
/// Clicking *outside* the popup needs only the popup's own outer rect, not
/// the per-row math, so it is not exposed to that gap.
///
/// `outside_x`/`outside_y` must land inside the shell's main content area
/// (below/right of the title bar, activity bar and any sidebar chrome) —
/// those regions are dispatched before `crate::app::App::handle` ever
/// sees the click. Each per-backend caller picks a point near its own
/// surface's bottom-right corner, well clear of the centered popup.
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

/// Sweep several points inside one painted text run's vertical band and
/// assert every one resolves to the same interactive target.
///
/// A run is emitted by exactly one widget for exactly one row, so no point
/// inside its own painted bounds can legitimately resolve to a different
/// target. That makes this check need no per-row expected-value table —
/// it can be pointed at any new toggleable row cheaply.
///
/// `needle` locates the run via `inventory().text_runs()` — never a
/// literal coordinate. `samples` (>=2) evenly spans the run's full painted
/// vertical bounds, always including the very top and the very bottom
/// edge.
///
/// `fingerprint` reads back whatever painted, binary signal identifies
/// *which* row a click landed on — e.g. "is this directory row's child
/// still painted". Each sample point is clicked once (to observe
/// `fingerprint`) and then clicked again at the identical `(x, y)` to
/// restore state before the next sample, so the caller's toggle must be
/// idempotent under two clicks at the same point.
pub fn sweep_hit_band_integrity_resetting<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    needle: &str,
    samples: usize,
    mut setup: impl FnMut(&mut D),
    mut fingerprint: impl FnMut(&mut D) -> bool,
) {
    assert!(
        samples >= 2,
        "sweep_hit_band_integrity_resetting: need at least 2 samples to compare, got {samples}"
    );

    let mut outcomes = Vec::with_capacity(samples);
    let mut ys = Vec::with_capacity(samples);
    for i in 0..samples {
        setup(driver);
        let bounds = driver
            .inventory()
            .text_runs()
            .iter()
            .find(|r| r.text.contains(needle))
            .map(|r| r.bounds)
            .unwrap_or_else(|| {
                panic!("sweep_hit_band_integrity_resetting: {needle:?} not painted")
            });
        let x = bounds.x + bounds.width / 2.0;
        let top = bounds.y + 0.5;
        let bottom = (bounds.y + bounds.height - 0.5).max(top);

        let t = i as f32 / (samples - 1) as f32;
        let y = top + (bottom - top) * t;
        driver.click(x, y);
        outcomes.push(fingerprint(driver));
        ys.push(y);
    }

    let baseline = outcomes[0];
    for (i, outcome) in outcomes.iter().enumerate() {
        assert_eq!(
            *outcome,
            baseline,
            "sweep_hit_band_integrity_resetting: point {i}/{} inside {needle:?}'s \
             painted band resolved to a different target than the top of the row \
             (#971 hit-band drift) — outcomes were {outcomes:?} at y-offsets {ys:?}",
            samples - 1,
        );
    }
}

/// Like [`sweep_hit_band_integrity`], for a probe whose click is not
/// self-restoring.
///
/// `setup` rebuilds a fresh, comparable starting state before **every**
/// sample (not just once) instead of relying on a self-cancelling second
/// click. Unlike [`sweep_hit_band_integrity`], `needle`'s bounds are
/// re-located after every `setup()` call rather than once up front — a
/// modal popup that gets torn down and rebuilt this many times may repaint
/// at slightly different pixel positions, and re-locating costs nothing
/// `setup` was not already going to pay for with its own repaint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowBandEdge {
    /// The topmost pixel of the band, above the row's own text glyph.
    AboveGlyph,
    /// The bottommost pixel of the band, below the row's own text glyph.
    BelowGlyph,
}

/// Which side of a row's own text glyph a probe point sits on, within
/// that row's **painted background band**.
///
/// Every panel in this repo paints a row taller than the text glyph it
/// centres inside it, so each row owns a strip of background *above*
/// its glyph and another *below* it. Fixing one edge and not the other
/// only moves a hit-band bug.
pub fn painted_row_band(
    mut pixel: impl FnMut(i32) -> (u8, u8, u8),
    seed_y: i32,
    limit: i32,
) -> (f32, f32) {
    let seed = pixel(seed_y);
    let mut top = seed_y;
    while top > 0 && pixel(top - 1) == seed {
        top -= 1;
    }
    let mut bottom = seed_y;
    while bottom + 1 < limit && pixel(bottom + 1) == seed {
        bottom += 1;
    }
    assert!(
        top > 0 && bottom + 1 < limit,
        "painted_row_band: the probe column is a single flat colour {seed:?} from \
         y={top} to y={bottom} — it never crossed this row's own background band \
         edge, so it is sampling a column with no per-row fill (wrong x?) and any \
         band derived from it would prove nothing"
    );
    (top as f32, (bottom + 1) as f32)
}

/// Measure one painted row's background band by walking a single column
/// of **real painted pixels** outward from `seed_y` while the colour is
/// unchanged, returning the half-open band `[top, bottom)`.
///
/// This exists because `FrameInventory::text_runs()` only exposes *glyph*
/// bounds, and a row's glyph is strictly smaller than the band the panel
/// paints for it — so glyph bounds cannot answer "which row does this
/// point belong to". Reading the band off the pixels the frame actually
/// painted is the only way to state "inside this row" without asserting
/// a hardcoded coordinate.
///
/// `pixel(y)` must sample a column that is background at `seed_y` —
/// i.e. clear of the row's own glyph, its chevron and any scrollbar
/// gutter. Panics if the band cannot be resolved, rather than silently
/// returning a band that proves nothing.
///
/// The row being measured must also be painted in a colour its immediate
/// neighbours do *not* share — a section header or the focused row —
/// or the walk runs straight through the boundary into the next
/// identically-filled row.
pub fn row_click_in_its_painted_band_hits_its_own_row<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    needle: &str,
    next_row_needle: &str,
    band: (f32, f32),
    edge: RowBandEdge,
    mut effect_after_click: impl FnMut(&mut D) -> bool,
) {
    let locate = |d: &mut D, text: &str| -> quadraui::Rect {
        d.inventory()
            .text_runs()
            .iter()
            .find(|r| r.text.contains(text))
            .unwrap_or_else(|| {
                panic!("row_click_in_its_painted_band_hits_its_own_row: {text:?} not painted")
            })
            .bounds
    };

    let bounds = locate(driver, needle);
    let next_bounds = locate(driver, next_row_needle);
    assert!(
        next_bounds.y > bounds.y,
        "sanity: {next_row_needle:?} (y={}) must paint below {needle:?} (y={}) \
         — picked the wrong pair of rows",
        next_bounds.y,
        bounds.y
    );

    let (top, bottom) = band;
    let glyph_bottom = bounds.y + bounds.height;
    assert!(
        top <= bounds.y && bottom >= glyph_bottom,
        "sanity: the measured band [{top}, {bottom}) must contain {needle:?}'s own \
         glyph ([{}, {}]) — otherwise `painted_row_band` was seeded off the wrong \
         row and this probe would test some other row's padding",
        bounds.y,
        glyph_bottom,
    );
    assert!(
        bottom <= next_bounds.y,
        "sanity: {needle:?}'s band must end (y={bottom}) at or above \
         {next_row_needle:?}'s own glyph top (y={}) — they would otherwise \
         overlap, and no probe point could be attributed to one row",
        next_bounds.y,
    );

    let x = bounds.x + bounds.width / 2.0;
    // Self-measured from the frame's own pixels, never a literal
    // coordinate: the extreme pixel of the row's own painted band, on the
    // requested side of its glyph.
    let y = match edge {
        RowBandEdge::AboveGlyph => top + 0.5,
        RowBandEdge::BelowGlyph => bottom - 0.5,
    };
    match edge {
        RowBandEdge::AboveGlyph => assert!(
            y < bounds.y,
            "sanity: the probe point (y={y:.1}) must fall above {needle:?}'s own \
             text glyph (top={:.1}) — otherwise this only re-tests the glyph's own \
             centre, which every pre-existing click test in this panel already \
             covers, not the row's padding strip this issue is about. A backend \
             whose row pitch already equals its glyph height (TUI, by \
             construction) has no such strip and will fail here — that is the \
             point, not a bug in the probe: this scenario should not be \
             registered for that backend.",
            bounds.y,
        ),
        RowBandEdge::BelowGlyph => assert!(
            y > glyph_bottom,
            "sanity: the probe point (y={y:.1}) must fall below {needle:?}'s own \
             text glyph (bottom={glyph_bottom:.1}) — see the `AboveGlyph` arm's \
             message for why this is a structural claim, not a tuning knob."
        ),
    }

    driver.click(x, y);

    assert!(
        effect_after_click(driver),
        "a click inside {needle:?}'s own painted background band (x={x:.1}, \
         y={y:.1}, band [{top}, {bottom})) — {} its text glyph, and strictly \
         above where {next_row_needle:?} begins painting — must act on \
         {needle:?}'s own row, not the row painted next to it (#983/#1028)",
        match edge {
            RowBandEdge::AboveGlyph => "above",
            RowBandEdge::BelowGlyph => "below",
        },
    );
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

/// A click anywhere inside `needle`'s own **painted background band** —
/// the full vertical slot the panel fills for that row, on either side of
/// its text glyph — must act on `needle`'s own row, never on the row
/// painted above or below it.
///
/// `band` is `needle`'s painted band as measured by [`painted_row_band`]
/// from the pixels of the current frame; `edge` picks which extreme of it
/// to probe. Both edges are exercised: fixing only one edge would just
/// move the bug to the other one.
///
/// `effect_after_click` reads back, from **painted** output only,
/// whether the click acted on `needle`'s own row: `true` only when the
/// correct-row outcome is observed.
pub fn activity_bar_click_focuses_search_panel<D: ConformanceDriver + DriverInput>(driver: &mut D) {
    let search_zone = driver
        .inventory()
        .zones()
        .iter()
        .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_SEARCH)
        .map(|z| z.bounds)
        .expect("the Search activity-bar icon must register a chrome zone");

    assert!(
        driver.screen_has("Ln 1,"),
        "precondition: a fresh buffer starts with the cursor on line 1"
    );

    driver.click(
        search_zone.x + search_zone.width / 2.0,
        search_zone.y + search_zone.height / 2.0,
    );
    driver.type_text("jj");

    assert!(
        driver.screen_has("Ln 1,"),
        "clicking the Search icon then typing 'jj' must not move the \
         editor cursor -- the click must move keyboard focus into the \
         search panel, not leave it on the editor"
    );
    assert!(
        driver.screen_has("jj"),
        "'jj' must land in the search panel's query box"
    );

    ConformanceDriver::press_named(driver, NamedKey::Escape);
    ConformanceDriver::press_named(driver, NamedKey::Escape);
    driver.type_text("j");

    assert!(
        driver.screen_has("Ln 2,"),
        "Esc, Esc must return keyboard focus to the editor -- a motion key \
         typed afterwards must move the cursor"
    );
}

/// Clicking a panel's activity-bar icon must move keyboard focus
/// into that panel.
///
/// Locates the Search activity-bar icon via `FrameInventory::zones()` —
/// [`crate::core::engine::sidebar::PANEL_SEARCH`] is literally the
/// `"panel:search"` `WidgetId` `compose::app_shell::AppShell::render`'s
/// `register_chrome_zones` keys every activity-bar item's zone by —
/// rather than `click_text`, since the icon's fallback glyph collides
/// with ordinary path separators the explorer tree paints.
///
/// Asserts on the painted global status bar's "Ln N, Col N" cursor-position
/// segment, painted whenever `'ruler'` is on (the engine default), rather
/// than reading `engine.search_has_focus` directly: this is the exact
/// rendered signal a user watches.
///
/// Two Escapes, not one, are needed to fully return focus to the editor:
/// the query-input Escape arm only demotes the query field to the panel's
/// own results list (a second, in-panel focus move).
pub fn sidebar_ctrl_w_l_returns_focus_to_the_editor<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
) {
    use crate::core::engine::sidebar::PANEL_SEARCH;

    let (x, y) = activity_bar_zone_center(driver, PANEL_SEARCH);
    driver.click(x, y);
    assert!(
        driver.screen_has("Ln 1,"),
        "precondition: a fresh buffer starts with the cursor on line 1"
    );
    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'l');
    driver.type_text("j");
    assert!(
        driver.screen_has("Ln 2,"),
        "Ctrl-W l must return keyboard focus to the editor -- a motion key \
         typed right after must move the cursor"
    );
}

/// `Ctrl-W` from a focused sidebar panel arms a one-keystroke chord;
/// `l`/`Right` moves keyboard focus to the editor.
/// `render::route_sidebar_chord_key` is the shared rung both backends
/// now call.
///
/// A motion key typed right after must move the cursor, which only
/// happens once the editor holds keyboard focus — asserted on painted
/// output (the status line cursor position).
pub fn sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar<
    D: ConformanceDriver + DriverInput,
>(
    driver: &mut D,
) {
    use crate::core::engine::sidebar::PANEL_SEARCH;

    let screen_has_ci = |driver: &D, needle: &str| {
        let needle = needle.to_ascii_lowercase();
        driver
            .inventory()
            .text_runs()
            .iter()
            .any(|r| r.text.to_ascii_lowercase().contains(&needle))
    };

    let (x, y) = activity_bar_zone_center(driver, PANEL_SEARCH);
    driver.click(x, y);
    assert!(
        screen_has_ci(driver, "search"),
        "precondition: clicking the Search icon must open the panel"
    );
    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'h');
    ConformanceDriver::type_char(driver, 'j');
    ConformanceDriver::type_char(driver, 'l');
    // "control", not "source control": the header paints as two separate
    // text runs ("SOURCE", "CONTROL" — one per styled span, same reason
    // `GtkDriver::find`'s own doc warns against a needle that straddles a
    // style change), so a two-word needle never matches a single run even
    // though both words are on screen. "control" alone is still unique to
    // this panel's header on this fixture.
    assert!(
        screen_has_ci(driver, "control"),
        "Ctrl-W h must move keyboard focus to the activity bar toolbar -- \
         'j' then 'l' from there must move the toolbar cursor onto Source \
         Control (the next fixed panel after Search) and activate it, \
         which a still-panel-focused or still-editor-focused 'j'/'l' would \
         not do"
    );
}

/// Center of `panel_id`'s activity-bar chrome zone, shared by
/// [`sidebar_ctrl_w_l_returns_focus_to_the_editor`] and
/// [`sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar`] (and mirrors
/// [`activity_bar_click_focuses_search_panel`]'s own inline lookup, above).
fn activity_bar_zone_center<D: ConformanceDriver>(driver: &D, panel_id: &str) -> (f32, f32) {
    driver
        .inventory()
        .zones()
        .iter()
        .find(|z| z.id.as_str() == panel_id)
        .map(|z| {
            (
                z.bounds.x + z.bounds.width / 2.0,
                z.bounds.y + z.bounds.height / 2.0,
            )
        })
        .unwrap_or_else(|| panic!("the {panel_id} activity-bar icon must register a chrome zone"))
}

/// A full round trip through the toolbar leg of the Ctrl-W chord — panel to
/// toolbar (`Ctrl-W h`) and straight back (`Ctrl-W l`) — must land back in
/// the *same* panel it started from, not switch panels or toggle the
/// sidebar.
///
/// Types `"zz"` into the Search panel's query box, crosses to the toolbar
/// and back, then types `"y"`: only landing back in the Search query box
/// (caret still at the end of `"zz"`) paints `"zzy"` — landing in the
/// editor, a different panel, or a reset query box would not.
///
/// **Not independently red against unfixed `develop`:** before the fix,
/// `Ctrl-W` was never armed on the toolbar's own key rung, so the chord's
/// second keystroke (`h` / `l`, sent via `type_char` with no Ctrl held) fell
/// straight to the toolbar's *plain* key table, where unmodified `h` already
/// means "focus out to the panel" and unmodified `l` already means
/// "activate the toolbar's current row". The toolbar cursor never moves off
/// the Search row in this scenario, so that old plain-key path happens to
/// activate the same panel and this assertion still holds pre-fix. The
/// sibling scenario below,
/// [`panel_toolbar_round_trip_uses_the_active_panel_not_the_toolbar_cursor`],
/// moves the toolbar cursor first and is the one that actually goes red —
/// this scenario is kept as a baseline that the simple, cursor-unmoved round
/// trip still works post-fix.
pub fn panel_toolbar_round_trip_returns_to_the_same_panel<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
) {
    use crate::core::engine::sidebar::PANEL_SEARCH;

    let (x, y) = activity_bar_zone_center(driver, PANEL_SEARCH);
    driver.click(x, y);
    driver.type_text("zz");
    assert!(
        driver.screen_has("zz"),
        "precondition: clicking the Search icon then typing must land in \
         its query box"
    );

    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'h');
    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'l');
    driver.type_text("y");

    assert!(
        driver.screen_has("zzy"),
        "Ctrl-W h then Ctrl-W l must return keyboard focus to the Search \
         panel's own query box, not the editor or a different panel"
    );
}

/// Same round trip as
/// [`panel_toolbar_round_trip_returns_to_the_same_panel`], but with a plain
/// `j` (no Ctrl) moving the toolbar's own selection cursor off the active
/// row before crossing back — the toolbar->panel leg of the chord must still
/// land on the panel that was active when the toolbar was entered, never on
/// whatever row the cursor happens to sit on.
///
/// **Verified RED against unfixed `develop`:** reverting
/// `App::handle_key_press`'s dispatch of `render::route_sidebar_chord_key`
/// ahead of `handle_activity_bar_key` for `FocusKeyRoute::ActivityBar`
/// (restoring the old unconditional `handle_activity_bar_key` call) makes
/// this fail: with the cursor moved to the Source Control row by the plain
/// `j`, the old plain-key table's unmodified `l` activates *that* row
/// instead of returning to the panel that was active when the toolbar was
/// entered, so the final `screen_has("zzy")` assertion sees no `"y"` typed
/// into the Search query box.
pub fn panel_toolbar_round_trip_uses_the_active_panel_not_the_toolbar_cursor<
    D: ConformanceDriver + DriverInput,
>(
    driver: &mut D,
) {
    use crate::core::engine::sidebar::PANEL_SEARCH;

    let (x, y) = activity_bar_zone_center(driver, PANEL_SEARCH);
    driver.click(x, y);
    driver.type_text("zz");
    assert!(
        driver.screen_has("zz"),
        "precondition: clicking the Search icon then typing must land in \
         its query box"
    );

    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'h');
    // Move the toolbar's own cursor off the Search row -- the chord must
    // not use it.
    ConformanceDriver::type_char(driver, 'j');
    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'l');
    driver.type_text("y");

    assert!(
        driver.screen_has("zzy"),
        "Ctrl-W l from the toolbar must return focus to the panel that was \
         active when the toolbar was entered, not the row the toolbar's own \
         cursor was moved to with the plain 'j'"
    );
}

/// `Ctrl-W h`/`Left` from the activity bar toolbar is a no-op (the toolbar
/// is the leftmost stop in the window-navigation chain) -- it must be
/// consumed without changing focus or the toolbar cursor's position.
///
/// Mirrors [`sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar`]
/// exactly, with one extra, inert `Ctrl-W h` inserted while already on the
/// toolbar: the outcome (landing on Source Control via `j`, `l`) is
/// unchanged, which only holds if the extra chord had no effect.
///
/// **Verified RED against unfixed `develop` (runs on `gtk`/`tui_prod` only —
/// see its `backend_conformance!` registration):** before the fix, the
/// toolbar's key rung never checked for an armed `Ctrl-W` chord, so the
/// second `Ctrl-W h`'s `h` keystroke fell to the toolbar's own plain key
/// table, where unmodified `h` means "focus out to the panel". That extra,
/// supposedly inert chord actually knocks focus back out to the Search
/// panel, so the following `j`/`l` type into the Search query box instead
/// of moving the toolbar's own row selector, and the final
/// `screen_has("control")` assertion fails.
pub fn toolbar_ctrl_w_left_is_a_noop<D: ConformanceDriver + DriverInput>(driver: &mut D) {
    use crate::core::engine::sidebar::PANEL_SEARCH;

    let screen_has_ci = |driver: &D, needle: &str| {
        let needle = needle.to_ascii_lowercase();
        driver
            .inventory()
            .text_runs()
            .iter()
            .any(|r| r.text.to_ascii_lowercase().contains(&needle))
    };

    let (x, y) = activity_bar_zone_center(driver, PANEL_SEARCH);
    driver.click(x, y);
    assert!(
        screen_has_ci(driver, "search"),
        "precondition: clicking the Search icon must open the panel"
    );
    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'h');
    // Already on the toolbar -- this extra Ctrl-W h must be a no-op.
    ConformanceDriver::ctrl_char(driver, 'w');
    ConformanceDriver::type_char(driver, 'h');
    ConformanceDriver::type_char(driver, 'j');
    ConformanceDriver::type_char(driver, 'l');
    assert!(
        screen_has_ci(driver, "control"),
        "an inert Ctrl-W h while already on the toolbar must not change \
         focus or the toolbar cursor -- 'j' then 'l' must still land on \
         Source Control exactly as without the extra chord"
    );
}

/// Walks the *whole* window-navigation chain both ways -- editor -> panel
/// -> toolbar -> panel -> editor -- driven entirely through a user keymap
/// that rebinds the chord onto single keystrokes (`nnoremap <C-h> <C-w>h`,
/// `nnoremap <C-l> <C-w>l`); the literal two-keystroke `Ctrl-W` chord is
/// never pressed. Proves every leg -- not just the toolbar/panel one -- is
/// reached through the same `Engine::resolve_wincmd_keymap_direction` rung
/// any other rebound window command uses, not a second, toolbar-only
/// keymap table.
///
/// **Verified RED against unfixed `develop`:** reverting
/// `App::handle_key_press`, `Engine::resolve_wincmd_keymap_direction` and
/// `route_sidebar_chord_key` to their pre-fix state makes this test fail —
/// but not on the editor -> panel leg: that hop is served by the
/// pre-existing window-navigation-overflow rung, which the rebound-keymap
/// lookup already reaches unchanged, so the `"abcd"` assertion still
/// passes. The failure is on the panel <-> toolbar leg instead, for the
/// same reason [`panel_toolbar_round_trip_uses_the_active_panel_not_the_toolbar_cursor`]
/// goes red: pre-fix, the toolbar's own plain key table (not the chord
/// rung) handles the rebound `<C-h>`/`<C-l>`'s underlying unmodified
/// `h`/`l`, so the panel <-> toolbar round trip never reaches the panel
/// that was active when the toolbar was entered and the `"abcdef"`
/// assertion below fails.
pub fn rebound_ctrl_h_ctrl_l_walk_the_same_chain<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
) {
    use crate::core::engine::sidebar::PANEL_SEARCH;

    let (x, y) = activity_bar_zone_center(driver, PANEL_SEARCH);
    driver.click(x, y);
    driver.type_text("ab");
    assert!(
        driver.screen_has("ab"),
        "precondition: clicking the Search icon then typing must land in \
         its query box"
    );

    ConformanceDriver::press_named(driver, NamedKey::Escape);
    ConformanceDriver::press_named(driver, NamedKey::Escape);
    driver.type_text("j");
    assert!(
        driver.screen_has("Ln 2,"),
        "precondition: Esc, Esc must return keyboard focus to the editor, \
         leaving Search as the sidebar's active panel"
    );

    // Editor -> panel: with only one editor window there is nowhere further
    // left to go, so the window-navigation-overflow rung hands focus to the
    // sidebar's active panel (Search) instead -- through the rebound
    // `<C-h>`, never the literal `Ctrl-W` chord.
    ConformanceDriver::ctrl_char(driver, 'h');
    driver.type_text("cd");
    assert!(
        driver.screen_has("abcd"),
        "the rebound <C-h> must move keyboard focus from the editor back \
         into the Search panel's own query box (still holding \"ab\" from \
         before), not reset it or land on a different panel"
    );

    // Panel -> toolbar -> panel, exactly as
    // `panel_toolbar_round_trip_returns_to_the_same_panel`, but through the
    // rebound keys only.
    ConformanceDriver::ctrl_char(driver, 'h');
    ConformanceDriver::ctrl_char(driver, 'l');
    driver.type_text("ef");
    assert!(
        driver.screen_has("abcdef"),
        "the rebound <C-h>/<C-l> keystrokes must walk the identical panel \
         <-> toolbar chain the literal Ctrl-W chord does, landing back in \
         the Search panel's own query box"
    );

    // Panel -> editor, closing the loop.
    ConformanceDriver::ctrl_char(driver, 'l');
    driver.type_text("k");
    assert!(
        driver.screen_has("Ln 1,"),
        "the rebound <C-l> must return keyboard focus to the editor -- a \
         motion key typed right after must move the cursor"
    );
}

/// [`rebound_ctrl_h_ctrl_l_walk_the_same_chain`]'s fixture: [`engine_fixture`]
/// plus the two user keymaps `nnoremap <C-h> <C-w>h` / `nnoremap <C-l>
/// <C-w>l` -- same rhs encoding
/// `Engine::resolve_wincmd_keymap_direction` matches on, mirroring
/// `keyboard_routing`'s own
/// `a_user_keymap_bound_to_the_chord_resolves_identically_to_the_literal_chord`
/// unit test fixture.
fn engine_fixture_with_rebound_wincmd_keys() -> crate::core::Engine {
    use crate::core::engine::{UserKeymap, UserKeymapAction};

    let mut engine = crate::core::Engine::new_for_test();
    engine.settings.use_nerd_fonts = Some(false);
    let text = (1..=30)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    engine.buffer_mut().insert(0, &text);
    engine.user_keymaps.push(UserKeymap {
        mode: "n".to_string(),
        noremap: true,
        keys: vec!["<C-h>".to_string()],
        action: UserKeymapAction::Keys(vec!["<C-w>".to_string(), "h".to_string()]),
        buffer: None,
        desc: None,
    });
    engine.user_keymaps.push(UserKeymap {
        mode: "n".to_string(),
        noremap: true,
        keys: vec!["<C-l>".to_string()],
        action: UserKeymapAction::Keys(vec!["<C-w>".to_string(), "l".to_string()]),
        buffer: None,
        desc: None,
    });
    engine
}

/// The `h`/`Left` half of the `Ctrl-W` sidebar chord.
///
/// `j` then `l` only move the toolbar's own selection cursor and activate
/// whatever it lands on when the activity bar toolbar itself holds keyboard
/// focus. A fresh driver/single click is used deliberately, not a second
/// phase appended to [`sidebar_ctrl_w_l_returns_focus_to_the_editor`].
pub fn explorer_chevron_click_toggles_dir_with_same_arity_as_label_click<
    D: ConformanceDriver + DriverInput,
>(
    driver: &mut D,
    dir_name: &str,
    child_name: &str,
) {
    let locate = |d: &mut D, text: &str| -> quadraui::Rect {
        d.inventory()
            .text_runs()
            .iter()
            .find(|r| r.text == text)
            .unwrap_or_else(|| {
                panic!(
                    "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click: \
                     {text:?} not painted; painted: {:?}",
                    d.inventory().text_runs()
                )
            })
            .bounds
    };

    assert!(
        driver.screen_has(dir_name) && !driver.screen_has(child_name),
        "precondition: {dir_name:?} must be painted collapsed — {child_name:?} \
         (one of its children) must not be painted yet"
    );

    // GTK's shared tree paint path substitutes the sharper
    // codicon `CHEVRON_RIGHT` (U+EAB6) glyph for the literal `▸` whenever
    // `TreeStyle::chevron_collapsed` is left at its `TreeStyle::default()`
    // value (which vimcode does not override), so the collapsed chevron's
    // painted text is backend/nerd-fonts-dependent now, not always
    // literally "▸". Accept either so this scenario still finds the same
    // row's chevron on pre-#1377 and current quadraui alike.
    let chevron_bounds = driver
        .inventory()
        .text_runs()
        .iter()
        .find(|r| {
            r.text == crate::icons::TREE_CHEVRON_RIGHT.fallback
                || r.text == crate::icons::TREE_CHEVRON_RIGHT.nerd
        })
        .unwrap_or_else(|| {
            panic!(
                "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click: \
                 collapsed chevron (\"▸\" or codicon CHEVRON_RIGHT) not painted; \
                 painted: {:?}",
                driver.inventory().text_runs()
            )
        })
        .bounds;
    let (cx, cy) = (
        chevron_bounds.x + chevron_bounds.width / 2.0,
        chevron_bounds.y + chevron_bounds.height / 2.0,
    );
    driver.click(cx, cy);

    assert!(
        driver.screen_has(child_name),
        "a single click on {dir_name:?}'s chevron (x={cx:.1}, y={cy:.1}) must \
         expand it exactly as a single click on its label does — {child_name:?} \
         is still not painted after one chevron click (#984)"
    );

    // Twin assertion (same row, re-located after the repaint above): a
    // single click on the label must collapse it back with the same
    // arity the chevron just needed.
    let label_bounds = locate(driver, dir_name);
    let (lx, ly) = (
        label_bounds.x + label_bounds.width / 2.0,
        label_bounds.y + label_bounds.height / 2.0,
    );
    driver.click(lx, ly);

    assert!(
        !driver.screen_has(child_name),
        "a single click on {dir_name:?}'s own label (x={lx:.1}, y={ly:.1}) must \
         collapse it back with the same single-click arity the chevron click \
         above needed to expand it (#984 parity requirement) — {child_name:?} \
         is still painted after one label click"
    );
}

/// A window's own painted rect from a [`ScreenLayout`](crate::render::ScreenLayout),
/// or a panic naming what *was* painted — the #987 scrollbar scenarios'
/// equivalent of `inventory().text_runs()`'s "locate via what was painted,
/// never a literal coordinate" rule, applied to a window that has no text
/// of its own to search for (a scrollbar thumb isn't a text run).
fn window_rect(
    layout: &crate::render::ScreenLayout,
    id: crate::core::WindowId,
) -> crate::core::window::WindowRect {
    layout
        .windows
        .iter()
        .find(|w| w.window_id == id)
        .unwrap_or_else(|| {
            panic!("window_rect: {id:?} not painted; painted: {:?}", {
                layout
                    .windows
                    .iter()
                    .map(|w| w.window_id)
                    .collect::<Vec<_>>()
            })
        })
        .rect
}

/// A window's own painted rect from a [`ScreenLayout`](crate::render::ScreenLayout),
/// or a panic naming what *was* painted — the equivalent of
/// `inventory().text_runs()`'s "locate via what was painted, never a
/// literal coordinate" rule, applied to a window that has no text of its
/// own to search for.
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarDragOutcome {
    /// `true` if `target`'s own first visible line is *still* painted after
    /// the drag — i.e. the drag did **not** scroll `target`. A working
    /// scrollbar drag must make this `false`.
    pub top_line_still_painted: bool,
    /// `true` if `target`'s own painted rect (position or width) changed —
    /// i.e. the drag resized the split `target` sits in. A correct
    /// scrollbar drag must leave this `false`.
    pub target_resized: bool,
    /// The same resize check for the *other* window in the split — a
    /// vertical-group resize always moves both sides' rects together, so
    /// this is redundant with [`Self::target_resized`] in practice, but
    /// checking it independently means a future split-geometry change that
    /// somehow decoupled the two would still be caught by at least one of
    /// them.
    pub other_resized: bool,
}

fn rect_moved(a: crate::core::window::WindowRect, b: crate::core::window::WindowRect) -> bool {
    (a.x - b.x).abs() > 0.01 || (a.width - b.width).abs() > 0.01
}

/// What [`drag_group_scrollbar_column`] observed, in terms a caller can
/// assert directly — never a raw `view.scroll_top`/ratio read.
/// [`Self::top_line_still_painted`] comes from
/// [`quadraui::testing::ConformanceDriver::screen_has`], and the two
/// `_resized` fields come from the same painted
/// [`crate::render::ScreenLayout`] rects every other scenario in this
/// module locates windows through.
pub fn drag_group_scrollbar_column<D: ConformanceDriver + DriverInput>(
    h: &mut ConformanceHarness<D>,
    target: crate::core::WindowId,
    other: crate::core::WindowId,
    top_line_needle: &str,
) -> ScrollbarDragOutcome {
    let (target_rect, other_rect) = {
        let layout = h.screen_layout.borrow();
        let l = layout
            .as_ref()
            .expect("drag_group_scrollbar_column: no frame painted yet");
        (window_rect(l, target), window_rect(l, other))
    };
    assert!(
        ConformanceDriver::screen_has(&h.driver, top_line_needle),
        "precondition: {top_line_needle:?} (target's own top line) must be \
         painted before the drag"
    );

    let x = (target_rect.x + target_rect.width - 1.0) as f32;
    let y0 = (target_rect.y + 1.0) as f32;
    let y1 = (target_rect.y + target_rect.height * 0.6) as f32;
    h.driver.drag(x, y0, x, y1);

    let (target_rect2, other_rect2) = {
        let layout = h.screen_layout.borrow();
        let l = layout
            .as_ref()
            .expect("drag_group_scrollbar_column: no frame painted after the drag");
        (window_rect(l, target), window_rect(l, other))
    };

    ScrollbarDragOutcome {
        top_line_still_painted: ConformanceDriver::screen_has(&h.driver, top_line_needle),
        target_resized: rect_moved(target_rect, target_rect2),
        other_resized: rect_moved(other_rect, other_rect2),
    }
}

/// V0.11.0 bug report — "the scrollbar in the left editor tab group
/// doesn't work, and clicking it instead triggers a resize between tab
/// groups". Drags `target`'s own painted vertical-scrollbar column
/// downward, and reports what actually happened so the caller can assert
/// the two independent halves of the report: did `target` scroll, and did
/// the drag also resize the split `target`/`other` share.
///
/// # Locating the scrollbar without a hardcoded coordinate
///
/// There is no `inventory().text_runs()` entry for a scrollbar thumb, so
/// this locates the *window* instead, via [`window_rect`] against the
/// harness's own painted `ConformanceHarness::screen_layout`. The click
/// point is then derived purely from that rect: `x = target.rect.x +
/// target.rect.width - 1.0` (one unit inside the window's own right
/// edge, where the scrollbar sits) and `y = target.rect.y + 1.0` down to
/// `target.rect.y + height * 0.6` (well within the track and thumb).
///
/// # Why a *vertical-only* drag, not a horizontal one
///
/// A real user reaching for a vertical scrollbar drags **down**, not
/// sideways. A vertical-only gesture that starts inside the divider's
/// grab band still perturbs the ratio by however far off-center the press
/// happened to land, reproducing the "click resizes" half of the report
/// faithfully rather than avoiding it.
///
/// `top_line_needle` must be the exact text of `target`'s own first
/// visible line — unique enough that it cannot also match `other`'s
/// content (give the two windows distinct buffers), so the outcome
/// unambiguously reads `target`'s own scroll position, not `other`'s.
pub fn drag_group_divider_resizes<D: ConformanceDriver + DriverInput>(
    h: &mut ConformanceHarness<D>,
    left: crate::core::WindowId,
) -> bool {
    let (divider_x, y, left_before) = {
        let layout = h.screen_layout.borrow();
        let l = layout
            .as_ref()
            .expect("drag_group_divider_resizes: no frame painted yet");
        let lrect = window_rect(l, left);
        (l.group_dividers[0].position, lrect.y + 1.0, lrect)
    };

    h.driver.drag(
        divider_x as f32,
        y as f32,
        (divider_x + 20.0) as f32,
        y as f32,
    );

    let left_after = {
        let layout = h.screen_layout.borrow();
        window_rect(layout.as_ref().unwrap(), left)
    };
    rect_moved(left_before, left_after)
}

/// #987 negative-space case: a drag on the group divider **itself** (not
/// near a scrollbar) must still resize the split. This checks that a
/// "fix" doesn't make the scrollbar test pass by deleting the divider's
/// hit zone outright rather than by reordering hit tests. Unlike
/// [`drag_group_scrollbar_column`], this drag moves `x` by a real amount
/// (`+20.0`).
pub(crate) fn assert_text_metrics_backend_applies_metrics<B: quadraui::Backend>(backend: &mut B) {
    const LINE_HEIGHT: f32 = 971.25;
    const CHAR_WIDTH: f32 = 483.5;

    quadraui::Backend::set_current_line_height(backend, LINE_HEIGHT);
    quadraui::Backend::set_current_char_width(backend, CHAR_WIDTH);

    assert_eq!(
        backend.line_height(),
        LINE_HEIGHT,
        "Backend::set_current_line_height did not reach \
         Backend::line_height() — a stubbed setter silently disables the \
         #540/#819 click drift guard (#967); see #969"
    );
    assert_eq!(
        backend.char_width(),
        CHAR_WIDTH,
        "Backend::set_current_char_width did not reach \
         Backend::char_width() — a stubbed setter silently disables the \
         #540/#819 click drift guard (#967); see #969"
    );
}

// ── The `KNOWN_BUGS` bidirectional gate ─────────────────────────────────
//
// Mirrors `tests/nvim_conformance.rs`'s `KNOWN_DEVIATIONS` idiom for
// GUI-driving scenarios written against `ConformanceHarness` instead of
// nvim-comparison output. Six user-reported v0.11.0 bugs (this issue's own
// chained follow-ups) each add a scenario that encodes *correct* behaviour
// while their bug is still unfixed — without red-walling `cargo test` in
// the meantime — by wrapping that scenario's body in [`known_bug_gate`] and
// listing its label here.
//
// # THIS LIST MAY ONLY EVER SHRINK
//
// Never add an entry to silence a regression a fix introduced — that is
// exactly the failure mode this mechanism exists to catch (see the table on
// [`known_bug_gate_outcome`]'s doc). An entry is removed in the same PR that
// fixes the bug it names; [`known_bug_gate`] itself fails the build if a
// listed body starts passing and the entry is left behind, so "delete the
// entry" is not optional cleanup, it is enforced.
/// Conformance assertion for [`quadraui::Backend`]'s two load-bearing
/// setters — `set_current_line_height`/`set_current_char_width`. Both are
/// `&mut self` methods with no return value, so an empty ("stub") body
/// type-checks identically to a correct forwarding one; nothing short of
/// setting a value through the trait object and reading it back through
/// the `quadraui::Backend` getter can tell the two apart.
///
/// Call this once per concrete backend, in whatever driver-tier lane that
/// backend already has. The probe values are distinctive and deliberately
/// unlike any backend's `::new()` default, so a stubbed setter that
/// silently no-ops leaves the getter reporting its construction-time
/// default instead of the probe value — which fails the assertions below.
///
/// Empty as of #1343: #1089 fixed the shared `crate::app::App` paint/click
/// path (`src/app.rs`'s `ext:` arm + `try_route_sidebar_mouse_event`'s
/// `ExtPanel` arm) for all four backends, but its three `::macos`-suffixed
/// plugin-panel scenarios were left gated here because there was no macOS
/// runner available to confirm them at the time. The macOS native/AppKit CI
/// lane has since confirmed all three pass, so `known_bug_gate` fired
/// exactly as designed (see its doc comment) and those entries were
/// deleted.
///
/// The `issue_1256_sidebar_chrome::{settings_filter_row_is_painted,
/// extensions_search_row_is_painted}::{gtk,tui}` quartet that used to live
/// here (#1258's ruling: one shared `render::paint_sidebar_search_row`,
/// called by both GTK's `App::paint_sidebar_panel_rung` and the shipped
/// TUI's `tui_main::panels::render_settings_panel`/`render_ext_sidebar`)
/// was fixed by #1343 and deleted here — `known_bug_gate` reported
/// `FixLanded` once the fix landed, exactly as designed. Every scenario in
/// this suite now takes the plain `Pass`/`Regression` arms of
/// [`GateOutcome`] — there is currently no bug this table needs to track.
///
/// #1425 (App-on-TUI gap inventory) adds the first entries back: `tui`
/// arms on the two tab-bar close-button scenarios (diagnosed pre-existing
/// gaps between `crate::app::App` painted through `quadraui::tui::TuiBackend`
/// and the shipped TUI), plus the `crate::tui_main::app_on_tui_tests`
/// driver-test inventory that issue adds. A third scenario,
/// `activity_bar_click_focuses_search_panel_proof`, was suspected to need a
/// gated `tui` entry too but turned out to already pass — see that
/// scenario's own doc for why it was registered plain instead. Each entry's
/// category (unit/caps/feature/quadraui/product) and target child are
/// recorded next to it, not just its issue number, per that issue's own
/// gate-labelling requirement.
pub(crate) const KNOWN_BUGS: &[&str] = &[];

/// Empty as of #1432: every entry the App-on-TUI gap inventory added
/// has been closed — either by a real fix or by correcting a scenario
/// whose own fixture/assertion turned out to be wrong.
#[cfg(test)]
mod known_bugs_is_empty {
    #[test]
    fn known_bugs_is_empty() {
        assert!(
            super::KNOWN_BUGS.is_empty(),
            "KNOWN_BUGS must be empty per #1432's acceptance bar — found: \
             {:?}",
            super::KNOWN_BUGS
        );
    }
}

/// A saved `std::panic::set_hook`/`take_hook` closure — named so
/// `known_bug_gate_outcome`'s suppress/restore `RestoreHook` doesn't need
/// clippy's `type_complexity`-triggering type spelled out inline twice
/// (production fn + test-only twin).
type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send>;

/// What [`known_bug_gate_outcome`] found, before it decides whether that's a
/// passing test or a panic. Split out from [`known_bug_gate`] itself (which
/// turns this into an actual pass/fail) so the self-test below can assert on
/// the *verdict* directly instead of by making the suite actually fail —
/// see this issue's acceptance bar for why that distinction matters: a test
/// that can only observe "did the process abort" can't tell a correct gate
/// from a gate that always passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateOutcome {
    /// Body ran to completion and its label is not listed — the ordinary
    /// case for every scenario that isn't chasing a known bug.
    Pass,
    /// Body panicked and its label *is* listed — the bug is still open,
    /// exactly as `KNOWN_BUGS` says. Reported as a pass.
    ExpectedFail,
    /// Body panicked and its label is *not* listed — either a regression in
    /// already-working behaviour, or a brand-new bug that needs its own
    /// `KNOWN_BUGS` entry. Reported as a failure either way: an unlisted
    /// panic must never pass silently.
    Regression,
    /// Body ran to completion but its label *is* listed — the fix landed
    /// and nobody deleted the `KNOWN_BUGS` entry. Reported as a failure.
    FixLanded,
}

/// Run `body`, gated on whether `label` is listed in [`KNOWN_BUGS`]:
///
/// | body outcome | label listed | [`GateOutcome`] |
/// |---|---|---|
/// | panics | yes | [`ExpectedFail`](GateOutcome::ExpectedFail) |
/// | panics | no  | [`Regression`](GateOutcome::Regression) |
/// | passes | yes | [`FixLanded`](GateOutcome::FixLanded) |
/// | passes | no  | [`Pass`](GateOutcome::Pass) |
///
/// Uses `catch_unwind` wrapped in [`std::panic::AssertUnwindSafe`]: a
/// `ConformanceHarness` closes over `Rc<RefCell<_>>`/interior-mutable
/// state throughout, none of which is `UnwindSafe` by the auto-trait's
/// strict definition, but that strictness is about *reading possibly-torn
/// state after recovering from a panic* — this function never does that:
/// on either outcome the whole `body` (harness included) has already been
/// dropped by the time `catch_unwind` returns.
///
/// While `label` is listed, the process's panic hook is replaced with a
/// no-op for the duration of the call (restored before returning either
/// way, via a guard so a panicking `body` can't skip the restore) — a
/// listed, still-open bug's expected panic should not spam a green `cargo
/// test` run with a backtrace every time it runs.
///
/// # The guard-poisoning trap
///
/// A `ConformanceHarness` holds a [`crate::test_paint::PaintGuard`] and a
/// [`crate::test_cwd::CwdReadGuard`] — process-wide `Mutex`/`RwLock`s. If
/// `body` panics while one is alive, that lock is poisoned by the unwind.
/// Both guards already handle this at the primitive level by recovering
/// via `.unwrap_or_else(|e| e.into_inner())`, which is the reason:
/// "one red test must stay one red test rather than cascading into every
/// later painting test panicking on a poisoned lock".
pub(crate) fn known_bug_gate_outcome<F>(label: &str, body: F) -> GateOutcome
where
    F: FnOnce(),
{
    static HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let listed = KNOWN_BUGS.contains(&label);

    if !listed {
        // Unlisted: never touch the hook, so an unexpected panic stays loud.
        return if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_ok() {
            GateOutcome::Pass
        } else {
            GateOutcome::Regression
        };
    }

    let _hook_lock = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    /// RAII restore so a panicking `body` (the expected shape here) can't
    /// skip putting the real hook back.
    struct RestoreHook(Option<PanicHook>);
    impl Drop for RestoreHook {
        fn drop(&mut self) {
            if let Some(hook) = self.0.take() {
                std::panic::set_hook(hook);
            }
        }
    }

    let _restore = RestoreHook(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_info| {}));

    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_ok() {
        GateOutcome::FixLanded
    } else {
        GateOutcome::ExpectedFail
    }
}

/// The pass/fail wrapper around [`known_bug_gate_outcome`] a real gated
/// scenario `#[test]` calls: turns [`GateOutcome::Regression`] and
/// [`GateOutcome::FixLanded`] into a panic (test failure) with a message
/// naming which of the two happened and what to do about it;
/// [`GateOutcome::Pass`]/[`GateOutcome::ExpectedFail`] return normally.
pub(crate) fn known_bug_gate<F>(label: &'static str, body: F)
where
    F: FnOnce(),
{
    match known_bug_gate_outcome(label, body) {
        GateOutcome::Pass | GateOutcome::ExpectedFail => {}
        GateOutcome::Regression => panic!(
            "backend_conformance: scenario {label:?} panicked and is NOT listed \
             in KNOWN_BUGS — this is either a regression in previously-working \
             behaviour, or a brand-new bug that needs its own KNOWN_BUGS entry \
             (see src/harness.rs)"
        ),
        GateOutcome::FixLanded => panic!(
            "backend_conformance: scenario {label:?} PASSED but is still listed \
             in KNOWN_BUGS — the fix landed; delete the KNOWN_BUGS entry for \
             {label:?} in src/harness.rs"
        ),
    }
}

// ── The cross-backend runner ────────────────────────────────────────────
//
// See this module's "Which trait bound a scenario needs" doc for the
// `ConformanceDriver` vs `ConformanceDriver + DriverInput` boundary this
// macro's `backends` list rides on.
/// Expand one scenario into one `#[test]` per listed backend, each built
/// through that backend's own `conformance_harness` constructor — a
/// different concrete `ConformanceDriver` type per backend (`GtkDriver` vs
/// `TuiDriver`), so this has to be a macro, not a generic fn over a runtime
/// list of backends.
///
/// ```ignore
/// crate::backend_conformance! {
///     label: my_scenario,
///     backends: [gtk, tui, tui_prod],
///     engine: my_engine_fixture(),
///     size: (800, 480),
///     body: |driver| {
///         crate::harness::command_palette_filters_and_escape_dismisses(driver);
///     },
/// }
/// ```
///
/// expands to a `mod my_scenario { fn gtk() { .. } fn tui() { .. } fn
/// tui_prod() { .. } }` with one `#[test]` per backend arm — `cargo test`'s
/// own `mod_path::backend` test-name nesting keeps a single-backend
/// failure self-locating. The `gtk` arm is gated on `feature = "gui"`,
/// the same gate `vimcode`'s own `required-features` puts on the GTK bin;
/// the `tui`/`tui_prod` arms have no gate.
///
/// # `tui` vs `tui_prod`
///
/// `tui` wraps [`crate::app::App`] (the cross-backend-shared shell, also
/// what `gtk` wraps) on `quadraui::tui::TuiBackend`. `tui_prod` wraps the
/// same `App` via a thin alias ([`crate::tui_main::testing::
/// conformance_harness_prod`]). Both names are kept so existing
/// `tui_prod`-suffixed scenarios and `KNOWN_BUGS` labels keep resolving
/// unchanged. Not every scenario can run on `tui_prod` yet — see
/// `crate::tui_main::testing::conformance_harness_prod`'s own doc for
/// which trait bounds it satisfies.
///
/// `engine`/`size`/`body` are each re-evaluated once per backend arm —
/// the intended shape, since every fixture in this repo that touches the
/// filesystem builds its scratch path with [`scratch_dir`], which
/// disambiguates by `std::thread::current().id()` *and*
/// `std::process::id()`, and `cargo test` runs each generated `#[test]`
/// fn on its own thread, so two backends' arms never collide.
#[macro_export]
macro_rules! backend_conformance {
    (
        label: $label:ident,
        backends: [$($backend:ident),+ $(,)?],
        engine: $engine:expr,
        size: ($w:expr, $h:expr),
        body: |$driver:ident| $body:block $(,)?
    ) => {
        mod $label {
            #[allow(unused_imports)]
            use super::*;

            $(
                $crate::backend_conformance!(
                    @arm $backend, $engine, $w, $h, |$driver| $body
                );
            )+
        }
    };

    (@arm gtk, $engine:expr, $w:expr, $h:expr, |$driver:ident| $body:block) => {
        #[cfg(feature = "gui")]
        #[test]
        fn gtk() {
            let mut __h = $crate::gtk::testing::conformance_harness(
                $engine, $w as i32, $h as i32,
            );
            let $driver = &mut __h.driver;
            $body
        }
    };

    (@arm tui, $engine:expr, $w:expr, $h:expr, |$driver:ident| $body:block) => {
        #[test]
        fn tui() {
            let mut __h = $crate::tui_main::testing::conformance_harness(
                $engine, $w as u16, $h as u16,
            );
            let $driver = &mut __h.driver;
            $body
        }
    };

    (@arm tui_prod, $engine:expr, $w:expr, $h:expr, |$driver:ident| $body:block) => {
        #[test]
        fn tui_prod() {
            let mut __h = $crate::tui_main::testing::conformance_harness_prod(
                $engine, $w as u16, $h as u16,
            );
            let $driver = &mut __h.driver;
            $body
        }
    };
}

/// Test-only helper for [`tests::known_bug_gate_panic_does_not_poison_guards_for_a_later_harness`]:
/// build whichever conformance harness this build has compiled — TUI is
/// unconditional, so prefer it (no `feature = "gui"` dependency for a check
/// that has nothing to do with GTK specifically).
#[cfg(test)]
fn gtk_or_tui_probe_harness(
) -> ConformanceHarness<quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>> {
    let mut engine = crate::core::Engine::new_for_test();
    engine.settings.use_nerd_fonts = Some(false);
    crate::tui_main::testing::conformance_harness(engine, 80, 24)
}

/// Scratch directory for a filesystem-touching test fixture, keyed on
/// **both** the process id and the calling thread's id.
///
/// The thread id alone only disambiguates *within* one test binary.
/// `cargo test` hands every `#[test]` its own thread, but it is **not**
/// enough when two `cargo test` runs execute concurrently on one machine.
/// Both processes then derive the *same* `/tmp` path, and each fixture
/// opens with `remove_dir_all` — so one run deletes the other's fixture
/// out from under it, mid-test. Including `std::process::id()` makes the
/// path per-process and the collision impossible.
#[cfg(test)]
pub(crate) fn scratch_dir(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the invariant [`scratch_dir`]'s doc exists for: the path must
    /// be keyed on the *process* as well as the thread, so two concurrent
    /// `cargo test` runs on one machine cannot derive the same `/tmp`
    /// directory and `remove_dir_all` each other's fixture mid-test.
    ///
    /// RED against the pre-fix fixtures: they interpolated only
    /// `std::thread::current().id()`, so the pid assertion below fails.
    #[test]
    fn scratch_dir_is_unique_per_process_and_thread() {
        let mine = scratch_dir("vimcode_scratch_probe");
        let name = mine.file_name().unwrap().to_string_lossy().into_owned();

        assert!(
            name.contains(&std::process::id().to_string()),
            "scratch path must carry the pid so two concurrent test \
             processes get different dirs, got {name}"
        );
        assert!(
            name.contains(&format!("{:?}", std::thread::current().id())),
            "scratch path must still carry the thread id so two backend \
             arms in one process get different dirs, got {name}"
        );

        // A second thread in *this* process must also get its own path —
        // the property `backend_conformance!`'s per-arm fixtures rely on.
        let other = std::thread::spawn(|| scratch_dir("vimcode_scratch_probe"))
            .join()
            .unwrap();
        assert_ne!(
            mine, other,
            "two threads in one process must not share a scratch dir"
        );
    }

    /// A minimal engine fixture for the cross-backend proof slice: an
    /// explorer rooted at a scratch dir, with the root and `src`
    /// expanded so `src`'s child `core` paints directly beneath it.
    ///
    /// `tag` must be distinct per caller — combined with [`scratch_dir`]'s
    /// process id + thread id, per `backend_conformance!`'s own doc on why
    /// that's enough to avoid collisions.
    fn engine_with_expanded_explorer(tag: &str) -> crate::core::Engine {
        let dir = scratch_dir(&format!("vimcode_test_982_harness_proof_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src").join("core")).unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir.clone();
        engine.explorer_expanded.insert(dir.clone());
        engine.explorer_expanded.insert(dir.join("src"));
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        engine
    }

    // ── Deliverable 4, item 1: one existing scenario, both backends ────
    //
    // `sweep_hit_band_integrity` is the scenario bounded by
    // `ConformanceDriver + DriverInput`, the one this proof slice needs
    // to demonstrate the macro expands identically on both GTK and TUI.
    crate::backend_conformance! {
        label: sweep_hit_band_integrity_proof,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_expanded_explorer("sweep"),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("src") && driver.screen_has("core"),
                "precondition: the explorer must paint both `src` and its \
                 expanded child `core`"
            );
            crate::harness::sweep_hit_band_integrity(driver, "src", 5, |d| {
                quadraui::testing::ConformanceDriver::inventory(d).screen_has("core")
            });
        },
    }

    // ── #1361: Source Control focused-hint row, both backends ───────────
    //
    // The SC panel's bottom row reads "Press '?' for help" on both
    // backends while the panel has keyboard focus, and the row isn't
    // reserved when it doesn't. Fixed by moving the row reservation into
    // the shared `render::sc_sidebar_bands` and painting it through one
    // shared `render::sc_hint_status_bar`.
    fn engine_with_sc_panel(tag: &str, focused: bool) -> crate::core::Engine {
        let dir = scratch_dir(&format!("vimcode_test_1361_sc_hint_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir;
        engine.git_branch = Some("main".to_string());
        engine.sc_has_focus = focused;
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_GIT,
        ));
        engine
    }

    crate::backend_conformance! {
        label: sc_hint_row_shows_only_while_focused,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_sc_panel("focused", true),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("SOURCE CONTROL"),
                "precondition: the SC panel must be showing"
            );
            assert!(
                driver.screen_has("Press '?' for help"),
                "the SC panel has keyboard focus, so the hint row must be painted"
            );

            // Move focus back to the editor. `Escape` is
            // `Engine::handle_sc_key`'s own focus-release binding.
            driver.press_named(quadraui::NamedKey::Escape);
            assert!(
                !driver.screen_has("Press '?' for help"),
                "the SC panel lost keyboard focus, so the hint row must no \
                 longer be reserved/painted"
            );
        },
    }

    // ── #1797 review (non-blocking finding): GTK twin for the CHANGES
    // badges scenario ──
    //
    // The fix (`Engine::adopt_cwd_for_startup_file`) is shared,
    // platform-neutral code that both backends read identically.
    fn engine_outside_cwd_with_sc_panel_1797(tag: &str) -> crate::core::Engine {
        let workspace = scratch_dir(&format!("vimcode_test_1797_sc_panel_ws_{tag}"));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).unwrap();
        let run_git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&workspace)
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {args:?} failed");
        };
        run_git(&["init"]);
        run_git(&["config", "user.email", "t@t.com"]);
        run_git(&["config", "user.name", "T"]);

        let tracked = workspace.join("main.rs");
        std::fs::write(&tracked, "fn main() {}\n").unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "init"]);
        std::fs::write(&tracked, "fn main() { /* changed */ }\n").unwrap();

        let untracked = workspace.join("extra.rs");
        std::fs::write(&untracked, "// new file\n").unwrap();

        // Deliberately not `workspace`: an unrelated, repo-less directory
        // standing in for "wherever the process's cwd happened to be at
        // launch" -- only the fixed `adopt_cwd_for_startup_file` can make
        // `cwd` land on the file's real repo from here.
        let launch_cwd = scratch_dir(&format!("vimcode_test_1797_sc_panel_launch_{tag}"));
        let _ = std::fs::remove_dir_all(&launch_cwd);
        std::fs::create_dir_all(&launch_cwd).unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.swap_file = false;
        engine.cwd = launch_cwd;
        engine.startup_without_session_restore(Some(&tracked));
        engine.git_branch = Some("main".to_string());
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_GIT,
        ));
        // The manual refresh the report's repro step triggers via the
        // sync/refresh icon -- see the `TuiDriver`-level scenario for the
        // click-driven version of this same gesture. Calling `sc_refresh`
        // directly here keeps this fixture backend-neutral (an icon-only
        // toolbar button's painted bounds differ enough between GTK's
        // pixel layout and TUI's cell layout that locating it generically
        // across both backends is its own, separate concern from what
        // this scenario is pinning: that the fix makes `sc_refresh`
        // itself resolve the right repo, and that both backends paint the
        // result identically once it does).
        engine.sc_refresh();
        engine
    }

    /// Whether a painted text run reading exactly `badge` sits on the
    /// same row as, and strictly left of, the first painted run
    /// containing `filename` — i.e. the status badge is painted
    /// immediately before the file's name.
    ///
    /// Not just `inv.screen_has("M main.rs")`: GTK paints the status
    /// char and the filename as two *separate* coloured Pango labels,
    /// so `screen_has`'s per-label `contains` check never sees the
    /// combined substring on that backend. This instead uses
    /// [`quadraui::testing::FrameInventory`]'s own relational vocabulary
    /// (`left_of`/`same_row`, built for "two painted things, however
    /// many labels either backend split them into"), with an *exact*
    /// match on `badge` rather than `contains`.
    fn badge_precedes_file(
        inv: &quadraui::testing::FrameInventory,
        badge: char,
        filename: &str,
    ) -> bool {
        let badge = badge.to_string();
        // `filename` can legitimately paint more than once a frame (the
        // tab bar's own title, the breadcrumb, *and* its CHANGES-section
        // row all read "main.rs") -- checking only the first match in
        // paint order found the tab title instead of the CHANGES row and
        // never saw a badge next to it. Check every occurrence instead;
        // only one needs a badge immediately to its left.
        inv.text_runs()
            .iter()
            .filter(|r| r.text.contains(filename))
            .any(|file_run| {
                inv.text_runs().iter().any(|r| {
                    r.text == badge
                        && r.bounds.x + r.bounds.width <= file_run.bounds.x
                        && r.bounds.y < file_run.bounds.y + file_run.bounds.height
                        && file_run.bounds.y < r.bounds.y + r.bounds.height
                })
            })
    }

    crate::backend_conformance! {
        label: sc_panel_changes_section_shows_status_badges_after_startup_cwd_fix_1797,
        backends: [gtk, tui, tui_prod],
        engine: engine_outside_cwd_with_sc_panel_1797("badges"),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("SOURCE CONTROL"),
                "precondition: the SC panel must be showing"
            );
            assert!(
                driver.screen_has("CHANGES"),
                "precondition: the CHANGES section header must be painted"
            );
            assert!(
                driver.screen_has("main.rs"),
                "the modified tracked file's name must be painted in the \
                 CHANGES list"
            );
            assert!(
                driver.screen_has("extra.rs"),
                "the untracked file's name must be painted in the CHANGES \
                 list"
            );
            let inv = driver.inventory();
            assert!(
                badge_precedes_file(&inv, 'M', "main.rs"),
                "the modified tracked file must carry an 'M' status badge"
            );
            assert!(
                badge_precedes_file(&inv, 'U', "extra.rs"),
                "the untracked file must carry a 'U' status badge (VS \
                 Code's untracked glyph -- see `StatusKind::label`'s own \
                 doc for why this is 'U', not git's raw '?')"
            );
        },
    }

    // ── #1361 review: hit-testing must account for the reserved hint row ──
    //
    // The acceptance bar names `sweep_hit_band_integrity`. With the hint
    // row reserved, a double-click on a real file's own row must still
    // open *that* file — proving the click router resolves against the
    // exact shrunk geometry the painter used. A double-click landing
    // inside the hint row's own painted band must have no effect on any
    // content row beneath it.

    /// Like [`engine_with_sc_panel`], but with one real (untracked) file
    /// on disk so a double-click on its row has a genuine, painted side
    /// effect (`sc_activate_row`'s open path) to assert on — the same
    /// technique `src/tui_main/shell_app.rs`'s
    /// `tui_sc_sidebar_double_click_on_a_changed_file_opens_it`
    /// uses for this exact click path, run here with `sc_has_focus: true`
    /// from the first frame so the hint row is already reserved (slab one
    /// row shorter) when the click below lands.
    fn engine_with_sc_panel_and_file(
        tag: &str,
        file_name: &str,
        marker: &str,
    ) -> crate::core::Engine {
        let dir = scratch_dir(&format!("vimcode_test_1361_sc_row_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output();
        std::fs::write(dir.join(file_name), format!("{marker}\n")).unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir;
        engine.git_branch = Some("main".to_string());
        engine.sc_has_focus = true;
        // Untracked, like other fixtures: `sc_activate_row` takes the
        // plain-open path rather than the diff-split path.
        engine.sc_file_statuses = vec![crate::core::git::FileStatus {
            path: file_name.to_string(),
            staged: None,
            unstaged: Some(crate::core::git::StatusKind::Untracked),
            unmerged: None,
        }];
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_GIT,
        ));
        engine
    }

    #[cfg(feature = "gui")]
    #[test]
    fn sc_content_row_click_still_opens_it_when_hint_reserved_gtk() {
        use quadraui::testing::{Anchor, ConformanceDriver};

        let mut h = crate::gtk::testing::conformance_harness(
            engine_with_sc_panel_and_file("content_row", "zqxw1361c.txt", "SC1361CONTENTMARKER"),
            800,
            480,
        );
        assert!(
            h.driver.screen_has("Press '?' for help"),
            "precondition: the panel starts focused, so the hint row must \
             already be reserved/painted before the click below — this is \
             the exact geometry (slab one row shorter) the click router \
             must account for"
        );
        assert!(
            !h.driver.screen_has("SC1361CONTENTMARKER"),
            "precondition: the file must not be open yet"
        );

        // Two clicks on the same spot fold into one `UiEvent::DoubleClick`
        // (the double-click detector, same folding mechanism as in the hit-band
        // sweep module's own doc explains) — `sc_activate_row`'s open path uses
        // the same mechanism.
        h.driver.click_text_at("zqxw1361c.txt", Anchor::Center);
        h.driver.click_text_at("zqxw1361c.txt", Anchor::Center);

        assert!(
            h.driver.screen_has("SC1361CONTENTMARKER"),
            "a double-click on the file's own row, painted while the SC \
             panel has focus (hint row reserved, slab one row shorter), \
             must still open exactly that file — the click router must \
             resolve against the same shrunk geometry the painter used"
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn sc_hint_row_click_does_not_open_a_content_row_gtk() {
        use quadraui::testing::{Anchor, ConformanceDriver};

        let mut h = crate::gtk::testing::conformance_harness(
            engine_with_sc_panel_and_file("hint_row", "zqxw1361h.txt", "SC1361HINTMARKER"),
            800,
            480,
        );
        assert!(
            h.driver.screen_has("Press '?' for help"),
            "precondition: the hint row must be painted (panel focused)"
        );

        // A click landing inside the hint row's own band must not leak
        // through to `handle_sc_sidebar_ui_event` and activate whatever
        // content row the un-reserved geometry would have put there — the
        // hint row is reserved.
        h.driver.click_text_at("Press '?' for help", Anchor::Center);
        h.driver.click_text_at("Press '?' for help", Anchor::Center);

        assert!(
            !h.driver.screen_has("SC1361HINTMARKER"),
            "a double-click on the reserved hint row must not open any \
             content row beneath it — the reservation the painter applied \
             must also be excluded from the click router's own geometry"
        );
    }

    // ── Deliverable 4, item 2: both gate directions ─────────────────────
    //
    // The load-bearing half of this issue, per its own acceptance bar:
    // "Direction 2 is the half that rots silently if untested". Both
    // directions assert on the returned `GateOutcome` itself rather than
    // on the test process's own pass/fail, exactly as the issue asks —
    // inspecting the verdict is what lets this test itself stay green
    // while proving both a green-suite direction *and* a
    // should-have-failed direction of the mechanism it's testing.
    #[test]
    fn known_bug_gate_reports_expected_fail_for_a_listed_panicking_body() {
        const LABEL: &str = "harness_self_test::always_panics";

        // Deliberately not in `KNOWN_BUGS` — the gate takes the listing as
        // a parameter here (`known_bug_gate_outcome` doesn't consult the
        // real const, `KNOWN_BUGS.contains` does — so this test proves the
        // *mechanism*, independent of what's currently listed for real,
        // by exercising both a body that panics and one that doesn't
        // against a hand-picked "is this listed" question). See the second
        // test below for the "genuinely not listed" half.
        let outcome = known_bug_gate_outcome_for_test(LABEL, true, || {
            panic!("synthetic always-failing body (#982 self-test)");
        });
        assert_eq!(
            outcome,
            GateOutcome::ExpectedFail,
            "a listed body that panics must report ExpectedFail (suite stays green)"
        );
    }

    #[test]
    fn known_bug_gate_reports_fix_landed_for_a_listed_passing_body() {
        const LABEL: &str = "harness_self_test::always_passes";

        let outcome = known_bug_gate_outcome_for_test(LABEL, true, || {
            // Deliberately does nothing — a body that "passes".
        });
        assert_eq!(
            outcome,
            GateOutcome::FixLanded,
            "a listed body that passes must report FixLanded — 'fix landed, \
             delete the KNOWN_BUGS entry' — this is the direction that rots \
             silently if untested"
        );
    }

    #[test]
    fn known_bug_gate_reports_regression_for_an_unlisted_panicking_body() {
        let outcome = known_bug_gate_outcome_for_test("harness_self_test::unlisted", false, || {
            panic!("synthetic unlisted panic (#982 self-test)");
        });
        assert_eq!(
            outcome,
            GateOutcome::Regression,
            "an unlisted body that panics must report Regression, never a pass"
        );
    }

    #[test]
    fn known_bug_gate_reports_pass_for_an_unlisted_passing_body() {
        let outcome =
            known_bug_gate_outcome_for_test("harness_self_test::unlisted_ok", false, || {});
        assert_eq!(outcome, GateOutcome::Pass);
    }

    // Coverage for the real public wrapper, not just the outcome function.
    // The four tests above exercise `known_bug_gate_outcome_for_test` (a
    // test-only twin that can pretend any label is listed). These two
    // exercise `known_bug_gate` itself — the panic-or-not wrapper every
    // real per-bug scenario will actually call — against its two reachable
    // arms: unlisted-passing (must not panic) and unlisted-panicking (must
    // panic, i.e. fail the calling test).
    #[test]
    fn known_bug_gate_does_not_panic_for_an_unlisted_passing_body() {
        known_bug_gate("harness_self_test::wrapper_unlisted_ok", || {});
    }

    #[test]
    fn known_bug_gate_panics_for_an_unlisted_panicking_body() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            known_bug_gate("harness_self_test::wrapper_unlisted_panics", || {
                panic!("synthetic unlisted panic (#982 self-test)");
            });
        }));
        assert!(
            result.is_err(),
            "known_bug_gate must itself panic (failing the calling test) for \
             an unlisted body that panics — an unlisted panic must never be \
             swallowed"
        );
    }

    /// Test-only twin of [`known_bug_gate_outcome`] that takes "is this
    /// listed" as an explicit parameter instead of consulting the real
    /// [`KNOWN_BUGS`] — so this suite can exercise all four table rows
    /// without needing a real (and therefore permanent, until some other
    /// issue's fix deletes it) entry in that list just to test the
    /// mechanism. Duplicates `known_bug_gate_outcome`'s body rather than
    /// refactoring it to take the listing as a parameter, because
    /// `known_bug_gate_outcome`'s own public signature — "label, consult
    /// the real list" — is the contract every real call site (a future
    /// per-bug scenario) needs; threading a test-only bool through it would
    /// leave a footgun parameter in the production API for one test's
    /// convenience.
    fn known_bug_gate_outcome_for_test<F>(label: &str, listed: bool, body: F) -> GateOutcome
    where
        F: FnOnce(),
    {
        static HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        if !listed {
            return if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_ok() {
                GateOutcome::Pass
            } else {
                GateOutcome::Regression
            };
        }

        let _hook_lock = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        struct RestoreHook(Option<PanicHook>);
        impl Drop for RestoreHook {
            fn drop(&mut self) {
                if let Some(hook) = self.0.take() {
                    std::panic::set_hook(hook);
                }
            }
        }

        let _restore = RestoreHook(Some(std::panic::take_hook()));
        std::panic::set_hook(Box::new(|_info| {}));

        let _ = label;
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_ok() {
            GateOutcome::FixLanded
        } else {
            GateOutcome::ExpectedFail
        }
    }

    /// The guard-poisoning trap, verified explicitly rather than trusted
    /// from `PaintGuard`/`CwdReadGuard`'s own doc comments (per this
    /// issue's own instruction: "Verify this explicitly"). Runs an
    /// `ExpectedFail`-shaped gate — a body that builds a real
    /// `conformance_harness` (acquiring both process-wide guards) and then
    /// panics while it's still alive — and then builds and drives a
    /// *second*, independent harness afterwards, on the same thread. If
    /// either guard's `Mutex`/`RwLock` poisoning cascaded past the first
    /// harness's teardown, the second `conformance_harness` call below (or
    /// the `screen_has` it depends on) would deadlock or panic on a poison
    /// error instead of completing normally.
    #[test]
    fn known_bug_gate_panic_does_not_poison_guards_for_a_later_harness() {
        let outcome = known_bug_gate_outcome("harness_self_test::poison_probe_unused", {
            let h = super::gtk_or_tui_probe_harness();
            move || {
                // Touch the harness so it's genuinely alive across the
                // panic, then panic while it's still holding both guards.
                let _ = h.driver.screen_contains("anything");
                panic!("synthetic panic while holding a live ConformanceHarness (#982)");
            }
        });
        // Not asserted against KNOWN_BUGS (the label above is deliberately
        // never listed) — this call goes through the *unlisted* path,
        // which still constructs+panics inside `body` and still must not
        // poison anything for the harness built immediately below.
        assert_eq!(outcome, GateOutcome::Regression);

        // The actual assertion: a second, independent harness must build
        // and paint successfully right after, on this same thread.
        let second = super::gtk_or_tui_probe_harness();
        assert!(
            !second.driver.screen_contains("__never_painted_982__"),
            "a fresh harness built after a guard-holding panic must still \
             paint normally, not deadlock/panic on a poisoned lock"
        );
    }
}

// A click anywhere in a row's painted band — the padding strip above
// its glyph and the one below it — acts on that row. TUI is excluded:
// its row pitch always equals its glyph height by construction, so it
// has no padding strip to probe and the helper's sanity assert fails
// loudly there rather than reporting a false pass.
#[cfg(test)]
mod issue_983_row_click_selects_the_row_below {
    use super::*;
    use crate::core::engine::sidebar::PANEL_SETTINGS;

    /// The Settings panel, scrolled so the "LSP" category — far below row 0,
    /// so as not to reuse `settings_panel_click_toggles_the_clicked_category`'s
    /// row-0 target — sits near the top of a normal 1400x900 viewport with its
    /// settings painted below it. The scroll offset is derived from LSP's real
    /// flat-list index, so adding or removing settings above it can't push its
    /// children off-screen. LSP's own first two settings ("Enable LSP",
    /// "Format on Save") are both `Bool`, chosen deliberately: collapsing
    /// LSP hides both their labels outright, giving a clean, unambiguous
    /// painted signal that the *category* row (not the row below it) was
    /// hit — mirrors `settings_panel_click_toggles_the_clicked_category`'s
    /// own "does the child label vanish" technique, just pointed lower.
    fn engine_settings_scrolled_to_lsp() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_SETTINGS));
        let lsp_cat = crate::core::settings::setting_categories()
            .iter()
            .position(|&c| c == "LSP")
            .expect("an \"LSP\" settings category");
        let lsp_row = engine
            .settings_flat_list()
            .iter()
            .position(
                |r| matches!(r, crate::core::engine::SettingsRow::CoreCategory(i) if *i == lsp_cat),
            )
            .expect("the LSP category header in the settings flat list");
        engine.settings_scroll_top = lsp_row.saturating_sub(2);
        engine
    }

    // The Settings panel, scrolled so the "LSP" category paints inside a
    // normal 1400x900 viewport. LSP's own first two settings ("Enable LSP",
    // "Format on Save") are both `Bool`, chosen deliberately: collapsing
    // LSP hides both their labels outright, giving a clean, unambiguous

    /// `needle`'s painted background band, measured off the pixels of the
    /// frame currently on screen.
    ///
    /// The probe column sits just past the right edge of `needle`'s own
    /// glyph — inside the panel, clear of the glyph itself, its chevron and
    /// the scrollbar gutter — so the colour it reads is the row's own
    /// background fill. Never a hardcoded coordinate: both the column and
    /// the seed row come from `needle`'s real painted bounds.
    #[cfg(feature = "gui")]
    fn probe_band<A: quadraui::AppLogic>(
        driver: &mut quadraui::gtk::testing::GtkDriver<A>,
        needle: &str,
        viewport_h: i32,
    ) -> (f32, f32) {
        let glyph = driver
            .find_bounds(needle)
            .unwrap_or_else(|| panic!("probe_band: {needle:?} not painted"));
        let probe_x = (glyph.x + glyph.width + 8.0) as i32;
        let seed_y = (glyph.y + glyph.height / 2.0) as i32;
        crate::harness::painted_row_band(|y| driver.pixel(probe_x, y), seed_y, viewport_h)
    }

    #[cfg(feature = "gui")]
    #[test]
    fn settings_row_click_below_its_glyph_hits_its_own_row_gtk() {
        let mut h =
            crate::gtk::testing::conformance_harness(engine_settings_scrolled_to_lsp(), 1400, 900);
        assert!(
            h.driver.screen_has("Enable LSP") && h.driver.screen_has("Format on Save"),
            "precondition: scrolling to the LSP category must paint both of its \
             settings; painted: {:?}",
            h.driver.inventory().text_runs()
        );
        let band = probe_band(&mut h.driver, "▼ LSP", 900);
        crate::harness::row_click_in_its_painted_band_hits_its_own_row(
            &mut h.driver,
            "▼ LSP",
            "Enable LSP",
            band,
            crate::harness::RowBandEdge::BelowGlyph,
            |d| !(d.screen_has("Enable LSP") || d.screen_has("Format on Save")),
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn settings_row_click_above_its_glyph_hits_its_own_row_gtk() {
        let mut h =
            crate::gtk::testing::conformance_harness(engine_settings_scrolled_to_lsp(), 1400, 900);
        assert!(
            h.driver.screen_has("Enable LSP") && h.driver.screen_has("Format on Save"),
            "precondition: scrolling to the LSP category must paint both of its \
             settings; painted: {:?}",
            h.driver.inventory().text_runs()
        );
        let band = probe_band(&mut h.driver, "▼ LSP", 900);
        crate::harness::row_click_in_its_painted_band_hits_its_own_row(
            &mut h.driver,
            "▼ LSP",
            "Enable LSP",
            band,
            crate::harness::RowBandEdge::AboveGlyph,
            |d| !(d.screen_has("Enable LSP") || d.screen_has("Format on Save")),
        );
    }

    // Sweep hit-band integrity over a settings row, sampling strictly inside
    // the needle's own painted glyph bounds. This passes today on every backend.
    // It protects against a #967-style regression in the row-pitch formula
    crate::backend_conformance! {
        label: settings_row_sweep_hit_band_integrity,
        backends: [gtk, tui, tui_prod],
        engine: engine_settings_scrolled_to_lsp(),
        size: (1400, 900),
        body: |driver| {
            assert!(
                driver.screen_has("Enable"),
                "precondition: LSP's first setting must be painted"
            );
            crate::harness::sweep_hit_band_integrity(driver, "LSP", 5, |d| {
                // "Enable", not "Enable LSP" -- TUI paints multi-word labels
                // as one text run per word (`Enable`/`LSP` separately), so a
                // needle spanning both never matches there; GTK's single
                // combined-string run still contains "Enable" too.
                d.screen_has("Enable")
            });
        },
    }
}

// The file explorer's expand/collapse chevron needs a double click, while
// its row's text label needs one. The root cause: a missing
// `TreeControllerEvent::RowToggleExpand` match arm in
// `Engine::dispatch_explorer_tree_event`. This bug reproduces on both backends.
#[cfg(test)]
mod issue_984_explorer_chevron_needs_a_double_click {
    use super::*;

    /// An explorer rooted at a scratch dir with one collapsed directory,
    /// `kkxxqq_dir984`, holding one child, `child984mk`. The root itself
    /// is expanded (so `kkxxqq_dir984`'s own row paints) but `kkxxqq_dir984`
    /// is deliberately left out of `explorer_expanded`, so it starts collapsed.
    ///
    /// `child984mk` is short on purpose — at this fixture's depth-2 nesting,
    /// the shared `App::shell_config()` conformance harness's sidebar leaves
    /// only ~12 columns for a TUI row label, and a name past that column
    /// budget paints truncated. `screen_has` needs the whole name painted.
    ///
    /// `tag` must be distinct per caller — combined with
    /// [`crate::harness::scratch_dir`]'s process id + thread id, this
    /// provides the same disambiguation rule `backend_conformance!`'s own doc
    /// spells out for every other filesystem-touching fixture in this module.
    fn engine_with_collapsed_explorer_dir(tag: &str) -> crate::core::Engine {
        let dir = crate::harness::scratch_dir(&format!("vimcode_test_984_explorer_chevron_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("kkxxqq_dir984")).unwrap();
        std::fs::write(dir.join("kkxxqq_dir984").join("child984mk"), b"").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir.clone();
        engine.explorer_expanded.insert(dir.clone());
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        // `session.explorer_visible` alone only reaches the shadow
        // `engine.app_shell`'s `sidebar_visible()` through
        // `Engine::sync_app_shell_sidebar_visibility`, which already ran
        // (inside `Engine::new_for_test()`, above) *before* this line set
        // the field. The `tui` arm's `App::new_headless_with_backend` does
        // not re-run that sync, so the shadow's `sidebar_visible()` would
        // otherwise stay stale-`false` — which `render::sync_runner_sidebar_visibility`
        // (called unconditionally on every dispatch) then mirrors onto the
        // *runner's* own `AppShell`, collapsing the sidebar column entirely
        // after the very first click and erasing the chevron-expand this
        // scenario asserts on. `show_panel` sets the shadow's `sidebar_visible`
        // directly, sidestepping the staleness.
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine
    }

    // Chevron and label click-arity parity on the same row (horizontal axis).
    // This bug reproduces on both backends. Each arm needs its own `KNOWN_BUGS`
    // label suffix (`::gtk` / `::tui`) — hand-written here to allow divergence
    // per-arm, since the macro expands one body verbatim into every backend arm
    #[cfg(feature = "gui")]
    #[test]
    fn explorer_chevron_click_toggles_dir_with_same_arity_as_label_click_gtk() {
        let mut __h = crate::gtk::testing::conformance_harness(
            engine_with_collapsed_explorer_dir("chevron_gtk"),
            800,
            480,
        );
        let driver = &mut __h.driver;
        crate::harness::known_bug_gate(
            "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click::gtk",
            || {
                crate::harness::explorer_chevron_click_toggles_dir_with_same_arity_as_label_click(
                    driver,
                    "kkxxqq_dir984",
                    "child984mk",
                );
            },
        );
    }

    #[test]
    fn explorer_chevron_click_toggles_dir_with_same_arity_as_label_click_tui() {
        let mut __h = crate::tui_main::testing::conformance_harness(
            engine_with_collapsed_explorer_dir("chevron_tui"),
            800,
            480,
        );
        let driver = &mut __h.driver;
        crate::harness::known_bug_gate(
            "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click::tui",
            || {
                crate::harness::explorer_chevron_click_toggles_dir_with_same_arity_as_label_click(
                    driver,
                    "kkxxqq_dir984",
                    "child984mk",
                );
            },
        );
    }

    // The `tui_prod` twin — same scenario, driven through the actual
    // production TUI shell instead of the shared `App`. Its own `::tui_prod`-suffixed
    // `KNOWN_BUGS` label, per the disambiguation rule: each backend arm needs
    // its own suffix once a body can diverge per-arm.
    #[test]
    fn explorer_chevron_click_toggles_dir_with_same_arity_as_label_click_tui_prod() {
        let mut __h = crate::tui_main::testing::conformance_harness_prod(
            engine_with_collapsed_explorer_dir("chevron_tui_prod"),
            800,
            480,
        );
        let driver = &mut __h.driver;
        crate::harness::known_bug_gate(
            "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click::tui_prod",
            || {
                crate::harness::explorer_chevron_click_toggles_dir_with_same_arity_as_label_click(
                    driver,
                    "kkxxqq_dir984",
                    "child984mk",
                );
            },
        );
    }

    // Sweep hit-band integrity across the directory row's label (vertical axis
    // of the same row). Ungated — passes on both backends today. This is not a
    // duplicate of the horizontal parity check above: it samples multiple y-offsets
    // strictly inside the label's own painted glyph bounds, proving no #967-style
    // vertical hit-band drift on the explorer's expand/collapse toggle.
    // Also registered on `tui_prod` — the explorer tree is the surface where
    // production-only regressions can occur, making this worth testing on the shipped TUI.
    crate::backend_conformance! {
        label: explorer_row_sweep_hit_band_integrity,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_collapsed_explorer_dir("sweep"),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("kkxxqq_dir984"),
                "precondition: the collapsed directory must be painted"
            );
            crate::harness::sweep_hit_band_integrity(driver, "kkxxqq_dir984", 5, |d| {
                d.screen_has("child984mk")
            });
        },
    }
}

// The scrollbar in the left editor tab group doesn't work, and clicking it
// instead triggers a resize between tab groups. `SCROLLBAR_IMPLEMENTATION.md`'s
// own "Known Limitations" section already named the first half ("non-active
// window scrollbars are visual-only") — this issue is test-only (no fix),
// pinning both halves as one conformance case.
//
// The root cause: `crate::app::App` has **no vertical scrollbar hit-test at all**
// in `handle_mouse_click_msg`. It hit-tests the horizontal scrollbar, then the
// group/window dividers, then falls through to ordinary editor click handling.
// That gap makes the scrollbar inert on every window. Whether that inert click
// also resizes depends on geometry: a window sitting immediately left of a group
// divider has its scrollbar column within the divider's grab band, causing resize.
// A window with no divider on its scrollbar-adjacent side is simply inert.
#[cfg(test)]
mod issue_987_group_scrollbar_inert_and_click_resizes {
    use super::*;
    use crate::core::window::SplitDirection;
    use crate::core::{Engine, WindowId};

    /// A two-group vertical split, each group showing a distinct 2000-line
    /// buffer (`{left,right}line{0..2000}_{tag}`) so [`drag_group_scrollbar_column`]
    /// can never ambiguously match the other window's content, and so each window
    /// genuinely needs a vertical scrollbar. `focus_left` picks which group ends
    /// up active — deliverable 3 needs the *other* group focused from whichever
    /// one its own scrollbar drag targets, so both directions share this one
    /// fixture rather than two near-identical copies.
    ///
    /// `engine.settings.minimap = false` reproduces the adjacency — the minimap
    /// being on would put ~50 columns of space between a window's scrollbar and
    /// the group divider, hiding the "click resizes" half entirely.
    fn engine_two_groups(tag: &str, focus_left: bool) -> (Engine, WindowId, WindowId) {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.minimap = false;
        // Pin the shadow `engine.app_shell`'s sidebar open (on
        // whichever panel — this scenario never looks at sidebar content,
        // only column geometry) so `render::sync_runner_sidebar_visibility`
        // (called unconditionally on every dispatch) doesn't collapse the
        // runner's sidebar column mid-drag, shifting every column this
        // scenario measures. See `engine_with_collapsed_explorer_dir`'s
        // identical fix, above, for the full mechanics.
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));

        let buf_left = engine.active_buffer_id();
        let left_content: String = (0..2000).map(|i| format!("leftline{i}_{tag}\n")).collect();
        if let Some(st) = engine.buffer_manager.get_mut(buf_left) {
            st.buffer.content = ropey::Rope::from_str(&left_content);
        }
        let left_window = engine.active_window_id();
        let left_group = engine.active_group;

        engine.open_editor_group(SplitDirection::Vertical);
        let right_window = engine.active_window_id();
        let buf_right = engine.buffer_manager.create();
        let right_content: String = (0..2000).map(|i| format!("rightline{i}_{tag}\n")).collect();
        if let Some(st) = engine.buffer_manager.get_mut(buf_right) {
            st.buffer.content = ropey::Rope::from_str(&right_content);
        }
        if let Some(w) = engine.windows.get_mut(&right_window) {
            w.buffer_id = buf_right;
        }
        // `open_editor_group` already focused the new (right) group --
        // matches the reported scenario ("click the left group's scrollbar
        // while the right group has focus") without any extra step.
        if focus_left {
            engine.active_group = left_group;
        }
        (engine, left_window, right_window)
    }

    // ── Deliverable 1: the left group's own scrollbar, right focused ────
    //
    // RED-verification: both assertions below were observed to fail
    // against unfixed `develop` -- `top_line_still_painted` was `true`
    // (the drag never scrolled `left_window` at all: "leftline0_left_gtk"/
    // "leftline0_left_tui" stayed painted) *and* `target_resized` was
    // `true` (the drag silently changed the group split ratio by the
    // fraction of a unit between the press's `x` and the split's exact
    // centre) -- i.e. **both** halves of the report reproduce, not just
    // one. That distinction (both fail, vs. only one) is exactly what this
    // issue's acceptance bar asks a RED run to state.
    #[cfg(feature = "gui")]
    #[test]
    fn left_group_scrollbar_drag_scrolls_without_resizing_gtk() {
        let (engine, left, right) = engine_two_groups("left_gtk", false);
        let mut h = crate::gtk::testing::conformance_harness(engine, 800, 480);
        assert_eq!(
            h.engine.borrow().active_window_id(),
            right,
            "precondition: the right group must hold focus"
        );
        crate::harness::known_bug_gate(
            "left_group_scrollbar_drag_scrolls_without_resizing::gtk",
            || {
                let outcome =
                    drag_group_scrollbar_column(&mut h, left, right, "leftline0_left_gtk");
                assert!(
                    !outcome.top_line_still_painted,
                    "dragging the left group's own scrollbar must scroll it \
                     (#987) -- \"leftline0_left_gtk\" is still painted"
                );
                assert!(
                    !outcome.target_resized && !outcome.other_resized,
                    "dragging the left group's own scrollbar must never \
                     change the group split ratio (#987) -- target_resized=\
                     {} other_resized={}",
                    outcome.target_resized,
                    outcome.other_resized
                );
            },
        );
    }

    #[test]
    fn left_group_scrollbar_drag_scrolls_without_resizing_tui() {
        let (engine, left, right) = engine_two_groups("left_tui", false);
        let mut h = crate::tui_main::testing::conformance_harness(engine, 800, 480);
        assert_eq!(
            h.engine.borrow().active_window_id(),
            right,
            "precondition: the right group must hold focus"
        );
        crate::harness::known_bug_gate(
            "left_group_scrollbar_drag_scrolls_without_resizing::tui",
            || {
                let outcome =
                    drag_group_scrollbar_column(&mut h, left, right, "leftline0_left_tui");
                assert!(
                    !outcome.top_line_still_painted,
                    "dragging the left group's own scrollbar must scroll it \
                     (#987) -- \"leftline0_left_tui\" is still painted"
                );
                assert!(
                    !outcome.target_resized && !outcome.other_resized,
                    "dragging the left group's own scrollbar must never \
                     change the group split ratio (#987) -- target_resized=\
                     {} other_resized={}",
                    outcome.target_resized,
                    outcome.other_resized
                );
            },
        );
    }

    // The right group's own scrollbar, left focused. The right group has no
    // divider on its scrollbar-adjacent side, so only the "inert" half of #987
    // is expected to reproduce here. Dragging it changes neither window's rect.
    // Still wrapped in one `known_bug_gate` call (the missing scroll panics),
    // but the resize assertion is ordered first so it is genuinely exercised.
    #[cfg(feature = "gui")]
    #[test]
    fn right_group_scrollbar_drag_scrolls_without_resizing_gtk() {
        let (engine, left, right) = engine_two_groups("right_gtk", true);
        let mut h = crate::gtk::testing::conformance_harness(engine, 800, 480);
        assert_eq!(
            h.engine.borrow().active_window_id(),
            left,
            "precondition: the left group must hold focus"
        );
        crate::harness::known_bug_gate(
            "right_group_scrollbar_drag_scrolls_without_resizing::gtk",
            || {
                let outcome =
                    drag_group_scrollbar_column(&mut h, right, left, "rightline0_right_gtk");
                assert!(
                    !outcome.target_resized && !outcome.other_resized,
                    "the right group has no divider on its scrollbar side; \
                     dragging its scrollbar must not resize anything -- \
                     target_resized={} other_resized={}",
                    outcome.target_resized,
                    outcome.other_resized
                );
                assert!(
                    !outcome.top_line_still_painted,
                    "dragging the right group's own scrollbar must scroll \
                     it (#987) -- \"rightline0_right_gtk\" is still painted"
                );
            },
        );
    }

    #[test]
    fn right_group_scrollbar_drag_scrolls_without_resizing_tui() {
        let (engine, left, right) = engine_two_groups("right_tui", true);
        let mut h = crate::tui_main::testing::conformance_harness(engine, 800, 480);
        assert_eq!(
            h.engine.borrow().active_window_id(),
            left,
            "precondition: the left group must hold focus"
        );
        crate::harness::known_bug_gate(
            "right_group_scrollbar_drag_scrolls_without_resizing::tui",
            || {
                let outcome =
                    drag_group_scrollbar_column(&mut h, right, left, "rightline0_right_tui");
                assert!(
                    !outcome.target_resized && !outcome.other_resized,
                    "the right group has no divider on its scrollbar side; \
                     dragging its scrollbar must not resize anything -- \
                     target_resized={} other_resized={}",
                    outcome.target_resized,
                    outcome.other_resized
                );
                assert!(
                    !outcome.top_line_still_painted,
                    "dragging the right group's own scrollbar must scroll \
                     it (#987) -- \"rightline0_right_tui\" is still painted"
                );
            },
        );
    }

    // ── Deliverable 2: negative-space case for the divider itself ───────
    //
    // Ungated -- passes today on both backends, and must keep passing
    // after any real fix: without this, a "fix" that made the two
    // deliverables above pass by deleting the group divider's own hit zone
    // (rather than adding the missing scrollbar hit-test, or reordering
    // the two) would sail through them undetected. Hand-written rather
    // than via `backend_conformance!`, same reason as the four tests
    // above: [`drag_group_divider_resizes`] needs the whole
    // `ConformanceHarness` (for `screen_layout`), not just the bare
    // `driver` the macro's generated body binds.
    #[cfg(feature = "gui")]
    #[test]
    fn group_divider_click_still_resizes_gtk() {
        let (engine, left, _right) = engine_two_groups("divider_gtk", false);
        let mut h = crate::gtk::testing::conformance_harness(engine, 800, 480);
        assert!(
            drag_group_divider_resizes(&mut h, left),
            "a drag on the group divider itself must still resize the \
             split (#987 negative-space case)"
        );
    }

    #[test]
    fn group_divider_click_still_resizes_tui() {
        let (engine, left, _right) = engine_two_groups("divider_tui", false);
        let mut h = crate::tui_main::testing::conformance_harness(engine, 800, 480);
        assert!(
            drag_group_divider_resizes(&mut h, left),
            "a drag on the group divider itself must still resize the \
             split (#987 negative-space case)"
        );
    }
}

/// The editor scrollbar's click-vs-drag decision was hand-rolled twice — once
/// in `tui_main/mouse.rs`, once in `app.rs` — the exact shape #987 was before
/// it reproduced a user-visible bug. Both now call `render::resolve_editor_scrollbar_click`
/// instead of re-deriving the three-way page-back/page-forward/begin-drag decision.
///
/// This is a structural convergence fix, not a bug fix: neither hand-rolled copy
/// had a known bug, so this scenario is not expected to go red against pre-#1061
/// `develop` — what it proves is that the architecture converged. The `tui_prod`
/// arm is the only harness lens that actually drives the production TUI shell.
///
/// This scenario uses a single, unsplit window filling the whole terminal (no
/// sidebar, no minimap) so the window's own right edge — where the vertical
/// scrollbar's one-cell column sits — coincides with the terminal's right edge,
/// a structurally known quantity rather than a magic number.
#[cfg(test)]
mod issue_1061_scrollbar_click_resolution_shared {
    use super::*;

    /// A single, unsplit window showing a buffer far taller than any
    /// viewport this scenario paints (500 lines against a 30-row terminal),
    /// so a vertical scrollbar is guaranteed and its thumb starts at the
    /// very top of the track while `scroll_top == 0` — exactly where this
    /// scenario's drag begins.
    fn tall_buffer_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.minimap = false;
        let buf = engine.active_buffer_id();
        let content: String = (0..500).map(|i| format!("scr1061line{i}\n")).collect();
        if let Some(st) = engine.buffer_manager.get_mut(buf) {
            st.buffer.content = ropey::Rope::from_str(&content);
        }
        engine
    }

    #[test]
    fn editor_v_scrollbar_thumb_drag_scrolls_via_shared_resolve_fn_tui_prod() {
        let mut h =
            crate::tui_main::testing::conformance_harness_prod(tall_buffer_fixture(), 60, 30);
        let driver = &mut h.driver;
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();

        assert!(
            driver.screen_has("scr1061line0"),
            "precondition: the buffer's own first line must be painted; \
             screen:\n{}",
            driver.screen()
        );
        let (_, y0) = driver.find("scr1061line0").unwrap_or_else(|| {
            panic!(
                "the first line must be painted to locate the scrollbar's \
                 own row; screen:\n{}",
                driver.screen()
            )
        });
        // One cell inside the window's own right edge, which here is also
        // the terminal's (no sidebar, no minimap to narrow it) — see this
        // module's own doc for why that's a structurally derived column,
        // not a magic number.
        let x = 59.0_f32;
        let y1 = y0 + 15.0;

        driver.drag(x, y0, x, y1);

        assert!(
            !driver.screen_has("scr1061line0"),
            "dragging the editor's own vertical-scrollbar thumb must scroll \
             the window, via the same `render::resolve_editor_scrollbar_click` \
             decision `app.rs`'s own scrollbar handler uses (#1061); \
             screen:\n{}",
            driver.screen()
        );
    }
}

// All three TUI-only mouse features (explorer drag-and-drop, hover-popup
// press-and-drag selection, tree empty-space right-click) used to live only
// in `src/tui_main/mouse.rs` and `shell_app.rs`. Now they live in shared
// `render::apply_explorer_drag_move`/`apply_explorer_drop`/
// `route_tree_empty_space_context_menu` which both backends call. Each scenario
// is registered on `gtk`/`tui` (proving `App` gained the behaviour) and `tui_prod`
// (proving the shipped TUI shell still has it, unregressed).
#[cfg(test)]
mod issue_1429_shared_mouse_routes {
    use super::*;

    /// A single-level explorer: one file (`dndfile1429`) and one empty
    /// folder (`dnddir1429`), both direct children of the root so
    /// there's no depth-2 indentation eating into the ~12-column TUI
    /// sidebar-label budget `engine_with_collapsed_explorer_dir`'s own
    /// doc (`issue_984_...`, above) found tight — see that fixture's doc
    /// for the budget this one deliberately stays clear of.
    ///
    /// `tag` disambiguates concurrently-running callers onto distinct
    /// temp dirs, like every other filesystem-touching fixture in this
    /// module.
    fn engine_with_dnd_explorer_fixture(tag: &str) -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1429_dnd_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dnddir1429")).unwrap();
        std::fs::write(dir.join("dndfile1429"), b"hi").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir.clone();
        engine.explorer_expanded.insert(dir.clone());
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine
    }

    // ── Explorer drag-and-drop (mouse.rs:83-112/:1003-1022, GOALS.md §7)

    // `tui_prod`-only: `confirm_move_file`'s Yes/No dialog resolves to a
    // native message dialog where `App` is the shell (GTK and TUI both wrap `App`).
    // Only `tui_prod` (the production TUI shell's independent paint path) actually
    // composes the dialog in-canvas, so only that arm gets `screen_has("Confirm Move")`.
    // GTK gets a hand-written twin below that checks `native_dialog_shown`.
    crate::backend_conformance! {
        label: explorer_drag_and_drop_opens_confirm_move_dialog,
        backends: [tui_prod],
        engine: engine_with_dnd_explorer_fixture("dnd"),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("dndfile1429") && driver.screen_has("dnddir1429"),
                "precondition: both the file and folder rows must be \
                 painted; painted: {:?}",
                driver.inventory().text_runs()
            );
            driver.drag_text("dndfile1429", "dnddir1429");
            assert!(
                driver.screen_has("Confirm Move"),
                "dragging the file row onto the folder row and releasing \
                 must open the move-confirmation dialog, via the shared \
                 `render::apply_explorer_drag_move`/`apply_explorer_drop` \
                 (#1429); painted: {:?}",
                driver.inventory().text_runs()
            );
        },
    }

    /// GTK twin of the scenario above, via the bespoke `crate::gtk::
    /// testing::harness` (not `conformance_harness`) — the same
    /// `native_dialog_shown`/`pending_native_dialog` idiom
    /// `explorer_context_menu_delete_opens_native_confirm_dialog`
    /// (`src/gtk/testing.rs`) uses, for the same reason: those two fields
    /// are `Rc` handles cloned out of `App` *before* it moves into the
    /// opaque `driver_with_shell(...)` return, which `ConformanceHarness`
    /// has no equivalent for (see this module's own doc, just above).
    #[cfg(feature = "gui")]
    #[test]
    fn explorer_drag_and_drop_opens_confirm_move_dialog_gtk() {
        let mut h =
            crate::gtk::testing::harness(engine_with_dnd_explorer_fixture("dnd_gtk"), 800, 480);
        assert!(
            h.driver.screen_has("dndfile1429") && h.driver.screen_has("dnddir1429"),
            "precondition: both the file and folder rows must be painted"
        );
        h.driver.drag_text("dndfile1429", "dnddir1429");
        assert!(
            h.native_dialog_shown.get(),
            "dragging the file row onto the folder row and releasing must \
             open the move-confirmation dialog -- `App` presents any \
             table/input-free dialog as a native message dialog \
             (quadraui#666's `native_dialog_options`), so the edge-trigger \
             flag is this arm's proof instead of `screen_has` (#1429)"
        );
        let opts = h.pending_native_dialog.take();
        assert!(
            opts.as_ref()
                .is_some_and(|o| o.body.contains("dndfile1429") && o.body.contains("dnddir1429")),
            "the queued native dialog must name both the moved file and \
             its destination folder; got {opts:?}"
        );
    }

    // ── Hover-popup press-and-drag text selection (mouse.rs:242,1661) ───

    /// A one-character buffer (`z`) so a post-copy `P` (paste-before) has an
    /// unambiguous single-row target, plus a focused hover popup over six short,
    /// distinct words — short enough that the popup's content-driven width never
    /// has to wrap in either backend's 800-wide fixture.
    fn engine_with_hover_popup_drag_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        let buf = engine.active_buffer_id();
        if let Some(st) = engine.buffer_manager.get_mut(buf) {
            st.buffer.content = ropey::Rope::from_str("z\n");
        }
        // Hermetic in-memory clipboard — both the `tui` and `tui_prod` arms
        // build through the same `App::new_headless_with_backend`, so neither
        // arm overrides these hooks. The write-then-read round trip stays in-process.
        let clip: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let clip_w = clip.clone();
        engine.clipboard_write = Some(Box::new(move |text: &str| {
            *clip_w.borrow_mut() = Some(text.to_string());
            Ok(())
        }));
        engine.clipboard_read = Some(Box::new(move || {
            clip.borrow().clone().ok_or_else(|| "empty".to_string())
        }));
        // Two markdown *paragraphs* (a blank line between them), not one
        // line with an inline space — `hover_markdown_structure`'s real
        // markdown parser folds a single embedded `\n` into the same
        // rendered line (CommonMark soft-break), which would paint
        // "alpha"/"zulu" as substrings of the *same* text run. GTK paints
        // one combined run per rendered line (see
        // `sweep_hit_band_integrity`'s own "TUI paints multi-word labels
        // as one run per word" doc, just above, for the converse on TUI);
        // `find`/`drag_text` locate a run's *center*, so two needles
        // sharing one run would resolve to the same point and the drag
        // below would be a same-point no-op. Two rows makes "alpha" and
        // "zulu" land in two distinct, independently-locatable runs.
        engine.show_editor_hover(
            0,
            0,
            "alpha beta gamma\n\ndelta epsilon zulu",
            crate::core::engine::EditorHoverSource::Lsp,
            true,
            false,
        );
        engine
    }

    // Registered on `gtk`/`tui` (proving `App` carries the drag follow-through)
    // and `tui_prod` (the original owner). Drives the drag itself, not just the
    // press: a zero-length selection from the press alone would still paste, but
    // never the words strictly between the drag's start and end.
    crate::backend_conformance! {
        label: hover_popup_press_and_drag_extends_selection,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_hover_popup_drag_fixture(),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("gamma") && driver.screen_has("delta"),
                "precondition: the focused hover popup's content must be \
                 painted; painted: {:?}",
                driver.inventory().text_runs()
            );
            driver.drag_text("alpha", "zulu");
            driver.ctrl_char('c');
            driver.press_named(NamedKey::Escape);
            driver.type_char('P');
            assert!(
                driver.screen_has("gamma") && driver.screen_has("delta"),
                "dragging from \"alpha\" to \"zulu\" inside the hover \
                 popup must extend the selection past the press point -- \
                 after Ctrl+C then a normal-mode `P`, the pasted text must \
                 contain the words strictly between the drag's two rows \
                 (#1429); painted: {:?}",
                driver.inventory().text_runs()
            );
        },
    }

    // ── Right-click on the explorer tree's empty space (mouse.rs:1352-1368)

    // Registered on all three arms: `route_tree_empty_space_context_menu` is the
    // shared stopgap for the still-open quadraui#1045 item 4.
    crate::backend_conformance! {
        label: explorer_right_click_below_last_row_opens_root_context_menu,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_dnd_explorer_fixture("emptyspace"),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("dndfile1429"),
                "precondition: the explorer must have at least one \
                 painted row"
            );
            assert!(
                !driver.screen_has("New File"),
                "precondition: no context menu is open yet"
            );
            // `app-shell:sidebar-content` (the quadraui `AppShell` chrome zone) is
            // only registered by the shared `App` — `tui_prod`'s production TUI shell
            // doesn't compose through `AppShell`. The empty-space point is derived from
            // the last painted row's bounds, which every arm paints identically.
            let last_row = driver
                .inventory()
                .text_runs()
                .iter()
                .find(|r| r.text.contains("dndfile1429"))
                .unwrap_or_else(|| {
                    panic!(
                        "\"dndfile1429\"'s row must be painted; painted: {:?}",
                        driver.inventory().text_runs()
                    )
                })
                .bounds;
            let x = last_row.x;
            let y = last_row.y + last_row.height * 5.0;
            driver.dispatch(quadraui::UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Right,
                position: quadraui::Point::new(x, y),
                modifiers: quadraui::Modifiers::default(),
            });
            assert!(
                driver.screen_has("New File"),
                "right-clicking empty tree space below the last row must \
                 open the root folder's context menu, via the shared \
                 `render::route_tree_empty_space_context_menu` (#1429); \
                 painted: {:?}",
                driver.inventory().text_runs()
            );
        },
    }
}

// The `:s///c` confirm-prompt spec was deliberately never built in #801. #1031
// replaced that error with a real confirm loop (`Engine::confirm_sub` /
// `handle_confirm_sub_key`). This gap checks two properties: is the confirm
// prompt actually *painted* on the command line, and does the post-substitute
// report line count only *actual* replacements, excluding matches answered 'n'?
#[cfg(test)]
mod issue_986_confirm_prompt_never_built {
    use super::*;

    /// The exact prompt Neovim v0.12.5 paints for `:%s/zqxw986abc/zqxw986def/gc`
    /// — captured verbatim from a real `nvim --headless` run, via `nvim_buf_set_lines`/
    /// `nvim_feedkeys` driving a real `:%s` and reading nvim's own stdout.
    /// Full captured confirm contract: `y` replaces-and-continues, `n` skips-and-continues,
    /// `a` replaces-this-and-all-remaining, `q`/`<Esc>` quit without replacing the pending
    /// match, `l` replaces-the-pending-match-then-quits. `^E`/`^Y` (scroll the window
    /// while the prompt is up) are left uncovered since they produce no buffer/cursor
    /// difference to observe.
    const NVIM_CONFIRM_PROMPT: &str =
        "replace with zqxw986def? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)";

    /// Three matches across three lines, all sharing one grep-safe token
    /// (`zqxw986abc`) so `screen_has`/`find_bounds` can never accidentally match
    /// unrelated painted chrome.
    fn engine_with_multi_match_buffer() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
            .buffer_mut()
            .insert(0, "zqxw986abc zqxw986abc\nzqxw986abc\nxyz zqxw986abc\n");
        engine
    }

    /// Four matches, one per line -- the fixture the report-line scenario
    /// below needs: answering `n` to exactly one of the four must make the
    /// post-substitute report read "3 substitutions on 3 lines", not "4".
    fn engine_with_four_single_match_lines() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
            .buffer_mut()
            .insert(0, "zqxw986abc\nzqxw986abc\nzqxw986abc\nzqxw986abc\n");
        engine
    }

    // The prompt itself is painted — both backends, via the shared macro.
    crate::backend_conformance! {
        label: confirm_prompt_text_is_painted,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_multi_match_buffer(),
        size: (1400, 900),
        body: |driver| {
            crate::harness::known_bug_gate("confirm_prompt_text_is_painted", || {
                assert!(
                    driver.screen_has("zqxw986abc"),
                    "precondition: the fixture buffer must paint"
                );
                driver.type_char(':');
                driver.type_text("%s/zqxw986abc/zqxw986def/gc");
                driver.press_named(quadraui::NamedKey::Enter);
                assert!(
                    driver.screen_has(NVIM_CONFIRM_PROMPT),
                    "the real ':s///c' confirm prompt (captured verbatim from \
                     Neovim) must be painted on the command line after \
                     ':%s/zqxw986abc/zqxw986def/gc<CR>'; painted: {:?}",
                    driver.inventory().text_runs()
                );
            });
        },
    }

    // Skipped ('n') matches must not count toward the post-substitute "N substitutions
    // on M lines" report — both backends, shared macro.
    crate::backend_conformance! {
        label: confirm_report_line_excludes_skipped_matches,
        backends: [gtk, tui, tui_prod],
        engine: engine_with_four_single_match_lines(),
        size: (1400, 900),
        body: |driver| {
            crate::harness::known_bug_gate(
                "confirm_report_line_excludes_skipped_matches",
                || {
                    assert!(
                        driver.screen_has("zqxw986abc"),
                        "precondition: the fixture buffer must paint"
                    );
                    driver.type_char(':');
                    driver.type_text("%s/zqxw986abc/zqxw986def/gc");
                    driver.press_named(quadraui::NamedKey::Enter);
                    // y, n, y, y -- one of the four matches skipped.
                    driver.type_char('y');
                    driver.type_char('n');
                    driver.type_char('y');
                    driver.type_char('y');
                    assert!(
                        driver.screen_has("3 substitutions on 3 lines"),
                        "answering y/n/y/y (one skip) must report exactly 3 \
                         substitutions on 3 lines -- the skipped match must not \
                         count -- captured verbatim from Neovim; painted: {:?}",
                        driver.inventory().text_runs()
                    );
                },
            );
        },
    }

    // The pending match is visually highlighted — an honest gap, NOT covered by
    // an automated test. Two attempts were made and both failed: any measurement
    // approach produced confounds unrelated to the feature, and building a confound-free
    // probe would require knowing how a real implementation paints the highlight
    // (cursor-line color? a dedicated match style?) — knowledge that does not exist yet.
}

/// A dead ~55-line hand-rolled `ActivityBarTarget` dispatch block was deleted
/// from `mouse.rs`. The block was confirmed-unreachable: `AppShell` consumes
/// the click into a semantic `AppShellEvent` upstream of the production TUI shell's
/// `handle` -> `mouse::handle_mouse` entirely. This is the systematic convergence-audit
/// registration the issue asks for, sampling two targets through the independent
/// `conformance_harness_prod` lens: an ordinary fixed panel (Search) and the
/// specific arm the issue named (the hamburger).
#[cfg(test)]
mod issue_1053_dead_activity_bar_block {
    /// A bare `Engine::new_for_test()` fixture with Nerd Fonts off, so the
    /// hamburger/search icons this scenario looks for are the ASCII
    /// fallback glyphs `crate::icons::Icon::s()` resolves consistently
    /// against (same thread, same flag -- see `icons.rs`'s module doc on
    /// why that's safe across a `cargo test` process without cross-test
    /// interference).
    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    #[test]
    fn activity_bar_click_routes_via_shell_app_not_dead_mouse_block_tui_prod() {
        let mut __h = crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 80, 24);
        let driver = &mut __h.driver;
        crate::icons::set_nerd_fonts(false);
        driver.set_double_click_folding(false);
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();

        assert!(
            !driver.screen_contains("Replace…"),
            "precondition: Search is not the default active panel; \
             screen:\n{}",
            driver.screen()
        );
        let search = crate::icons::SEARCH.s();
        let (sx, sy) = driver.find(search).unwrap_or_else(|| {
            panic!(
                "Search icon must paint on the activity bar; screen:\n{}",
                driver.screen()
            )
        });
        driver.click(sx, sy);
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        assert!(
            driver.screen_contains("Replace…"),
            "clicking the Search icon must open the Search panel via the \
             real ShellApp path, with the dead mouse.rs activity-bar block \
             gone; screen:\n{}",
            driver.screen()
        );

        assert!(
            !driver.screen_contains("File"),
            "precondition: menu bar starts hidden; screen:\n{}",
            driver.screen()
        );
        let hamburger = crate::icons::HAMBURGER.s();
        let (hx, hy) = driver
            .find(hamburger)
            .expect("hamburger icon must paint on the activity bar");
        driver.click(hx, hy);
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        assert!(
            driver.screen_contains("File"),
            "clicking the hamburger -- the specific arm #988's own \
             \"likely shape\" guess pointed at -- must reveal the menu bar \
             via the real ShellApp path, with the dead \
             `ActivityBarTarget::MenuToggle` arm gone; screen:\n{}",
            driver.screen()
        );
    }
}

/// A bottom activity-bar item (`shell_config`'s only one today, "bottom:settings")
/// drifted between backends on a second click: the production TUI shell's
/// `on_shell_event`'s `BottomItemClicked` arm ran `Engine::toggle_sidebar_panel`,
/// collapsing the sidebar. `App::on_shell_event`'s arm just called
/// `app_shell.show_panel(id)` unconditionally, re-showing instead of collapsing.
/// Both backends now call one shared function, `render::apply_activity_panel_switch`.
#[cfg(test)]
mod issue_1057_bottom_item_click_toggles_sidebar {
    use super::*;

    /// Nerd Fonts off so `crate::icons::SETTINGS.s()` resolves to the
    /// stable ASCII fallback `"*"` on every backend/thread, rather than a
    /// glyph whose resolution could vary with the ambient nerd-fonts
    /// default (`Settings::use_nerd_fonts`'s per-backend/per-OS guess) --
    /// same rationale as `issue_1053_dead_activity_bar_block`'s own
    /// `engine_fixture`.
    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    // Click-to-show, then click-again-to-hide, on all three arms -- the
    // issue's "Driver test on both backends covering click-to-show and
    // click-again" deliverable, plus the `tui_prod` convergence proof.
    //
    // Marker is "Appearance" (the Settings form's first group header, from
    // the shared `render::populate_settings_form_controller` +
    // `FormController` -- identical on every backend), not the literal
    // "SETTINGS" caption: that caption is painted by
    // `tui_main::panels::render_settings_panel`'s own
    // `draw_settings_chrome(.., " SETTINGS", ..)` call, which is genuine
    // *TUI-only* chrome -- `App::render_content`'s `PANEL_SETTINGS` arm
    // (`src/app.rs`) renders the shared `FormController` directly, with no
    // such caption. That's a real, separate divergence (a candidate for its
    // own future convergence item), not something this issue's fix touches
    // -- so this test doesn't lean on it.
    crate::backend_conformance! {
        label: bottom_item_second_click_collapses_sidebar,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (800, 480),
        body: |driver| {
            // Every click below is a genuine, independent, fully-released
            // press -- `drag_text(icon, icon)` (down -> move -> up, all at
            // the same point) rather than the bare `click_text`/`click`
            // (down only, no release). A bottom item is not a
            // `SidebarHidden`-reporting top panel (`AppShell` never runs
            // its own toggle for one -- see the arms this scenario
            // exercises, in `src/app.rs` and `src/tui_main/shell_app.rs`),
            // so nothing here depends on that difference *semantically* --
            // but a second bare `mouse_down` at the same point with no
            // intervening `mouse_up` left the simulated button latched
            // "still pressed" and the second click was silently swallowed
            // before ever reaching `on_shell_event` (observed directly:
            // `TuiDriver`'s own translate/dispatch layer, independent of
            // this issue's fix). `drag_text` releases between presses, so
            // each of the two clicks below is a real, independent one, the
            // same as a user's two separate mouse clicks would be.
            //
            // Also disables double-click folding: two of these close
            // together in simulated time would otherwise fold into a
            // `DoubleClick`, which bypasses `ShellAdapter`'s semantic
            // dispatch (and so this arm) entirely (see
            // `driver_click_on_every_activity_bar_icon_opens_its_panel_via_
            // shell_app`'s own doc for the same rationale).
            driver.set_double_click_folding(false);

            // `driver.screen()` is not used in diagnostics here: its return
            // type diverges per backend (`GtkDriver::screen` -> `Vec<u8>`,
            // `TuiDriver::screen` -> `String`), so a shared
            // `backend_conformance!` body can only rely on
            // `ConformanceDriver::screen_has`, which is uniformly `bool`
            // everywhere.
            assert!(
                !driver.screen_has("Appearance"),
                "precondition: the sidebar starts closed (session.explorer_\
                 visible defaults to false), so the Settings panel's form \
                 must not be painted yet"
            );

            driver.drag_text(crate::icons::SETTINGS.s(), crate::icons::SETTINGS.s());
            assert!(
                driver.screen_has("Appearance"),
                "clicking the Settings bottom item once must open its \
                 panel, on every backend"
            );

            driver.drag_text(crate::icons::SETTINGS.s(), crate::icons::SETTINGS.s());
            assert!(
                !driver.screen_has("Appearance"),
                "#1057: clicking the Settings bottom item again, while its \
                 own panel is already open, must collapse the sidebar -- VS \
                 Code collapses an active bottom-panel tab on a second \
                 click (checked before defaulting to this), and this is the \
                 behaviour TUI already had; GTK used to unconditionally \
                 re-show the panel instead of collapsing it"
            );
        },
    }
}

/// The third of three `AppShellEvent` shadow-`engine.app_shell` sync arms
/// (`PanelChanged`/`SidebarHidden`/`SidebarResized`) converged onto one shared
/// function, [`render::sync_shell_event_shadow`]. #988 was one of these forgetting
/// the sync entirely: `PanelChanged { hamburger }` returned early before sync.
/// This covers the *other* two arms, `PanelChanged`/`SidebarHidden` for an ordinary
/// panel, on all three backends — a structural-convergence scenario, not a bug
/// reproduction. The third, `SidebarResized`, has no rendered proxy to assert on.
#[cfg(test)]
mod issue_1062_shell_event_shadow_sync {
    use super::*;

    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    crate::backend_conformance! {
        label: activity_bar_panel_click_open_then_close_via_converged_shadow_sync,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (800, 480),
        body: |driver| {
            // Two genuine, independent, fully-released clicks — `drag_text`, not
            // `click_text`, and double-click folding disabled. A bare down-only click
            // leaves the simulated button latched for the second press, and two real
            // clicks close together would otherwise fold into a `DoubleClick`, which
            // bypasses `ShellAdapter`'s semantic `AppShellEvent` dispatch.
            driver.set_double_click_folding(false);

            assert!(
                !driver.screen_has("Replace…"),
                "precondition: Search is not the default active panel, so \
                 its form must not be painted yet"
            );

            let search = crate::icons::SEARCH.s();
            driver.drag_text(search, search);
            assert!(
                driver.screen_has("Replace…"),
                "clicking the Search icon once must open the Search panel \
                 -- AppShellEvent::PanelChanged synced onto the shadow \
                 engine.app_shell via render::sync_shell_event_shadow, on \
                 every backend"
            );

            driver.drag_text(search, search);
            assert!(
                !driver.screen_has("Replace…"),
                "clicking the Search icon again, while its own panel is \
                 already open, must close the sidebar -- \
                 AppShellEvent::SidebarHidden synced onto the shadow via \
                 the same shared function, on every backend"
            );
        },
    }
}

/// GTK's `App::handle_menu_action` used to hand-code five `EngineAction`
/// variants behind a catch-all instead of calling the general-purpose applier.
/// TUI's menu arm already called its general-purpose applier, but it was a
/// *different* function. Both now call one shared function, `render::apply_engine_action`.
/// This is a structural convergence fix, not a bug fix — every menu item that could
/// reach GTK's catch-all already had an explicit arm there. The catch-all was a
/// latent trap for a future menu item, not a live bug, so this is not expected to go
/// red against pre-#1063 `develop`. TUI-only (`tui` + `tui_prod`, no `gtk` arm):
/// requires Alt+T to reveal the menu, which is a modifier-carrying `UiEvent` specific
/// to `TuiDriver`'s own `dispatch`/`press_named`, not part of `ConformanceDriver`.
fn menu_terminal_activation_opens_terminal_pane<T: quadraui::AppLogic>(
    driver: &mut quadraui::tui::testing::TuiDriver<T>,
) {
    // 't' is `MENU_STRUCTURE`'s alt-letter for the "Terminal" menu
    // (`render.rs`: `("Terminal", 't', &[...])`) -- Alt+T reveals + opens
    // its dropdown in one dispatch.
    driver.dispatch(quadraui::UiEvent::KeyPressed {
        key: quadraui::Key::Char('t'),
        modifiers: quadraui::Modifiers {
            alt: true,
            ..quadraui::Modifiers::default()
        },
        repeat: false,
    });
    assert!(
        driver.screen().contains("New Terminal"),
        "Alt+T should reveal the menu bar and open the Terminal dropdown; \
         screen:\n{}",
        driver.screen()
    );

    // Enter activates the first (already-selected) item, "New Terminal"
    // (action id "terminal" -> `EngineAction::OpenTerminal`).
    // `MenuSystem::handle` closes the dropdown itself before returning
    // `Activated`, so nothing from the dropdown box can paint on the rows
    // checked below.
    driver.press_named(quadraui::NamedKey::Enter);

    // The bottom-panel tab bar's "Terminal" label (`render::
    // build_bottom_panel_tab_bar`) is the deterministic proof a terminal
    // pane actually opened. Skip row 0: the menu bar's own "Terminal"
    // top-level label lives there too (and stays painted regardless of
    // whether the terminal opened), so a whole-screen search would pass
    // even against a no-op.
    let screen = driver.screen();
    assert!(
        screen.lines().skip(1).any(|l| l.contains("Terminal")),
        "Terminal \u{25b8} New Terminal must open a terminal pane \
         (bottom-panel tab bar showing \"Terminal\" outside the menu-bar \
         row), via the shared render::apply_engine_action (#1063); \
         screen:\n{screen}"
    );
}

#[cfg(test)]
mod issue_1063_menu_action_engine_action_applier {
    use super::*;

    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    // #1043's `tui`/`tui_prod` pair: `tui` is the control (the shared `App`),
    // `tui_prod` is the shipped production TUI shell. Structural-convergence proof.
    #[test]
    fn menu_terminal_activation_opens_terminal_pane_tui() {
        let mut h = crate::tui_main::testing::conformance_harness(engine_fixture(), 80, 24);
        menu_terminal_activation_opens_terminal_pane(&mut h.driver);
    }

    #[test]
    fn menu_terminal_activation_opens_terminal_pane_tui_prod() {
        let mut h = crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 80, 24);
        menu_terminal_activation_opens_terminal_pane(&mut h.driver);
    }
}

/// The production TUI shell's `mouse.rs` hand-rolled the tab-bar-row dispatch
/// match twice (once for a split group, once for a single group) instead of
/// calling the shared `click::dispatch_tab_bar_target` GTK has called since #814.
/// Both hand-rolled copies are now deleted: both tab-bar-row arms call
/// `click::dispatch_tab_bar_target` directly and only handle its two deliberately-
/// deferred exceptions (`ActionMenuButton`'s popup placement, `CloseTab`'s confirm-
/// or-close decision) themselves. This is a structural convergence fix, not a bug
/// fix: both copies already produced the same tab-switch/tab-close behaviour.
/// Registered on `gtk`, `tui` **and** `tui_prod` for Tab-switch; CloseTab drops
/// the `tui` arm per an unrelated pre-existing gap that scenario's development surfaced.
#[cfg(test)]
mod issue_1059_tab_bar_dispatch_routes_through_shared_click_fn {
    use super::*;

    /// Two file-backed tabs with distinct, greppable label *and* content
    /// text, so a click can be aimed by name and its effect confirmed by
    /// what's actually painted in the editor pane -- never a hardcoded
    /// coordinate, never a populated-but-unpainted state field (engine
    /// state isn't even reachable on the `tui_prod` arm -- see
    /// `conformance_harness_prod`'s own doc on why `ConformanceHarness::
    /// engine` is a disconnected placeholder there). `new_for_test`'s own
    /// seeded scratch tab is closed immediately so exactly two tabs remain:
    /// `a1059.txt` at index 0 (inactive), `b1059.txt` at index 1 (active).
    fn two_tab_fixture() -> crate::core::Engine {
        let dir = crate::harness::scratch_dir("vimcode_test_1059_tab_bar_dispatch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a1059.txt");
        let b = dir.join("b1059.txt");
        std::fs::write(&a, "AAAA_1059_CONTENT\n").unwrap();
        std::fs::write(&b, "BBBB_1059_CONTENT\n").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.new_tab(Some(&a));
        engine.new_tab(Some(&b));
        engine.goto_tab(0); // the seeded scratch tab
        engine.close_tab();
        engine.goto_tab(1); // b1059.txt active; a1059.txt (index 0) is not
        engine
    }

    /// Locate the `×` sharing a row with *a* run containing `label_needle`
    /// and sitting to that run's right -- not simply "the first `×`" or
    /// "the first `label_needle`" on screen, and not a hardcoded coordinate
    /// either. Both single-needle shortcuts were tried while developing this
    /// scenario and each broke on at least one backend (confirmed by
    /// dumping `driver.inventory()`): a bare `×` needle resolves to `App`'s
    /// own window-close control on the `gtk`/`tui` arms (not `tui_prod`) --
    /// `App`'s shell chrome paints a bare `×` in its title bar, above the
    /// tab row -- and a bare filename needle can resolve to the status
    /// bar's/breadcrumb's copy of the same name on `gtk` specifically,
    /// whose paint order (native Pango/Cairo widget tree, not TUI's cell
    /// grid) puts it before the tab bar. Requiring *both* "a `label_needle`
    /// run" *and* "a `×` run to its right on the same row" only matches the
    /// tab itself -- neither the title bar (no filename text) nor the
    /// status bar (no `×` to its right) can satisfy both at once.
    fn tab_close_button_center<D: ConformanceDriver>(driver: &D, label_needle: &str) -> (f32, f32) {
        let close = tab_close_button_bounds(driver, label_needle);
        (close.x + close.width / 2.0, close.y + close.height / 2.0)
    }

    /// The full painted bounds of `label_needle`'s tab close (`×`) glyph —
    /// see [`tab_close_button_center`]'s doc for why both a label run and a
    /// `×` run to its right, sharing a row, are required to disambiguate it
    /// from unrelated `×` glyphs (the title-bar close control) or unrelated
    /// filename runs (the status bar/breadcrumb).
    fn tab_close_button_bounds<D: ConformanceDriver>(
        driver: &D,
        label_needle: &str,
    ) -> quadraui::Rect {
        tab_label_and_close_bounds(driver, label_needle).1
    }

    /// `(label_bounds, close_bounds)` for `label_needle`'s tab — the two painted
    /// runs both returned so a scenario can probe the dead gap between them without
    /// re-deriving either run's geometry itself.
    fn tab_label_and_close_bounds<D: ConformanceDriver>(
        driver: &D,
        label_needle: &str,
    ) -> (quadraui::Rect, quadraui::Rect) {
        let inventory = driver.inventory();
        let runs = inventory.text_runs();
        runs.iter()
            .filter(|r| r.text.contains(label_needle))
            .find_map(|label| {
                runs.iter()
                    .find(|r| {
                        // GTK now paints the tab close mark
                        // as the codicon `CLOSE` glyph (U+EA76) instead of
                        // the plain `×` (U+00D7) character; accept either
                        // so this disambiguation (same row, to the
                        // label's right) still finds it on both.
                        (r.text.contains(crate::icons::TAB_CLOSE.fallback)
                            || r.text.contains(crate::icons::TAB_CLOSE.nerd))
                            && r.bounds.y == label.bounds.y
                            && r.bounds.x > label.bounds.x
                    })
                    .map(|close| (label.bounds, close.bounds))
            })
            .unwrap_or_else(|| {
                panic!(
                    "{label_needle:?}'s tab label and its close button must both be \
                     painted, on the same row, close button to the right"
                )
            })
    }

    /// Center point of the tab bar's action-menu button (the `⋯` segment
    /// `build_tab_bar_primitive` always appends last, regardless of overflow
    /// or which other right segments are showing) — disambiguated from any
    /// other `⋯` on screen the same way [`tab_close_button_bounds`]
    /// disambiguates `×`: it must share a row with, and sit to the right of,
    /// a run containing `label_needle`.
    fn tab_action_menu_button_center<D: ConformanceDriver>(
        driver: &D,
        label_needle: &str,
    ) -> (f32, f32) {
        let inventory = driver.inventory();
        let runs = inventory.text_runs();
        let btn = runs
            .iter()
            .filter(|r| r.text.contains(label_needle))
            .find_map(|label| {
                runs.iter().find(|r| {
                    r.text.contains('\u{22ef}')
                        && r.bounds.y == label.bounds.y
                        && r.bounds.x > label.bounds.x
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "{label_needle:?}'s tab row must paint the action-menu (\u{22ef}) \
                     button to the label's right"
                )
            })
            .bounds;
        (btn.x + btn.width / 2.0, btn.y + btn.height / 2.0)
    }

    // Click the inactive tab's label. Resolves to `TabBarClickTarget::Tab`,
    // applied via `Engine::handle_tab_bar_click` inside `click::dispatch_tab_bar_target`.
    crate::backend_conformance! {
        label: tab_bar_click_switches_via_shared_dispatch,
        backends: [gtk, tui, tui_prod],
        engine: two_tab_fixture(),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("BBBB_1059_CONTENT") && !driver.screen_has("AAAA_1059_CONTENT"),
                "precondition: b1059.txt is the active tab, a1059.txt is not"
            );

            // A genuine, fully-released press (`drag_text(x, x)`: down ->
            // move -> up, all at the same point) rather than the bare
            // `click_text`/`click` (down only, no release) -- same
            // rationale as `issue_1057_bottom_item_click_toggles_sidebar`'s
            // own doc.
            driver.drag_text("a1059", "a1059");
            assert!(
                driver.screen_has("AAAA_1059_CONTENT") && !driver.screen_has("BBBB_1059_CONTENT"),
                "clicking tab a1059's label must switch to it, on every backend"
            );
        },
    }

    // Click the inactive tab's close button directly. Resolves to `ClickTarget::CloseTab`.
    // #1432 found that `crate::click::tighten_close_bounds` trims every close zone
    // by GTK's proportional-font padding constants unconditionally, in the shared
    // `App` code path; a cell-based backend's close zone is already tight, so the
    // trim always underflowed it and returned `None`. Fixed by falling back to the
    // raw zone instead of `None` when trimming would eat it. Runs ungated now.
    crate::backend_conformance! {
        label: tab_bar_click_closes_via_shared_dispatch,
        backends: [gtk, tui, tui_prod],
        engine: two_tab_fixture(),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("b1059") && driver.screen_has("a1059"),
                "precondition: both tabs are painted"
            );

            let (cx, cy) = tab_close_button_center(driver, "a1059");
            // A real down -> up at the same point, not the bare press-only
            // `click` default.
            driver.drag(cx, cy, cx, cy);
            assert!(
                !driver.screen_has("a1059") && driver.screen_has("BBBB_1059_CONTENT"),
                "clicking a1059.txt's close button must close it and fall \
                 back to the only remaining tab, b1059.txt, on every backend"
            );
        },
    }

    // The close region is the *whole* reserved close gutter. A click in the
    // dead gap immediately left of the painted ×, between the end of the label
    // and the glyph, must still close the tab — the clickable box equals the
    // box the rasteriser highlights on hover.
    //
    // This scenario runs `gtk`-only, deliberately — unlike every other scenario
    // in this suite, which all run on all three arms. The property under test is
    // a *proportional-layout* geometry fact, and quadraui's two layouts genuinely
    // disagree about who owns the inter-glyph gap: `pixel_tab_bar_layout` folds
    // `tab_inner_gap` into the close region, while the cell layout's one-column
    // gap stays with the tab body. That divergence is upstream's to reconcile,
    // not something vimcode may paper over per the Platform-Neutrality Rule, so
    // this asserts the pixel layout's contract where it holds instead of encoding
    // a fork.
    crate::backend_conformance! {
        label: tab_bar_click_in_close_gutter_closes_not_selects,
        backends: [gtk],
        engine: two_tab_fixture(),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("b1059") && driver.screen_has("a1059"),
                "precondition: both tabs are painted"
            );

            let (label, close) = tab_label_and_close_bounds(driver, "a1059");
            let label_end = label.x + label.width;
            assert!(
                close.x > label_end,
                "precondition: the rasteriser leaves a dead `tab_inner_gap` \
                 between a tab's label and its × glyph (label ends at \
                 {label_end}, × starts at {})",
                close.x
            );
            // Midpoint of that gap: outside the painted glyph, inside the
            // reserved close region.
            let x = (label_end + close.x) / 2.0;
            let y = close.y + close.height / 2.0;
            driver.drag(x, y, x, y);

            assert!(
                !driver.screen_has("a1059"),
                "a click in the reserved close gutter (between the label and the \
                 painted ×) must close a1059.txt, not select it"
            );
            assert!(
                driver.screen_has("BBBB_1059_CONTENT"),
                "closing a1059.txt must fall back to the only remaining tab, \
                 b1059.txt, on every backend"
            );
        },
    }

    // Overflow + right-segment click: enough tabs that the strip overflows and
    // scrolls to keep the active tab visible, then a click on the always-present
    // action-menu (`⋯`) right segment — disjoint from the scrolled tab strip,
    // kept together as one all-or-nothing block per `TabBar::layout`'s overflow
    // policy. Proves `TabBarLayout::hit_test`'s `visible_segments`/`hit_regions`
    // stay correct once tabs no longer all fit, not just in the two-tab case.
    //
    // Sized `(800, 480)`, not narrower: on `gtk` those units are real pixels, and
    // a narrower window leaves the editor group less space after the activity bar
    // and sidebar, potentially dropping the whole strip. The overflow itself comes
    // from `overflow_tab_fixture`'s primed scroll state.
    crate::backend_conformance! {
        label: tab_bar_overflow_right_segment_click_opens_action_menu,
        backends: [gtk, tui, tui_prod],
        engine: overflow_tab_fixture(),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("OVERFLOW_LAST_CONTENT"),
                "precondition: the last (active) tab's content is visible"
            );
            assert!(
                !driver.screen_has("overflow_tab_1491_00"),
                "precondition: the strip really has overflowed -- the first tab \
                 must have scrolled out of view to keep the active last tab visible"
            );

            let (cx, cy) = tab_action_menu_button_center(driver, "last1491");
            driver.drag(cx, cy, cx, cy);

            assert!(
                driver.screen_has("Close All"),
                "clicking the tab bar's action-menu button must open the tab \
                 action menu, even while the strip is scrolled by overflow"
            );
        },
    }

    /// Enough same-editor-group tabs that the strip cannot show them all at
    /// once, *already scrolled* so the active last tab (`last1491`, opened
    /// last) is the only one on screen.
    ///
    /// The scroll is primed here, in the fixture, rather than left to the
    /// first paint: `EditorGroup::tab_bar_width` starts at `usize::MAX`
    /// ("before the first render, assume all tabs fit"), so frame 1 always
    /// paints an *unscrolled* strip no matter how many tabs exist or how
    /// narrow the window is — the paint→`post_draw_apply_widths`→scroll
    /// feedback loop only converges from frame 2 on. Sizing the window down
    /// instead is what this scenario first tried, and it cannot work: it
    /// starved `gtk`'s strip of room for even one tab (so nothing painted a
    /// tab label at all) while never overflowing the `tui` arms, whose units
    /// are columns rather than pixels. `set_tab_visible_count` +
    /// `ensure_all_groups_tabs_visible` is exactly the pair
    /// `post_draw_apply_widths` itself runs, so this is the post-first-frame
    /// state reproduced faithfully, in one backend-agnostic unit (columns),
    /// rather than a hand-poked `tab_scroll_offset`.
    ///
    /// The tab *count* is sized for the other end of that same loop: once the
    /// real paint reports its own width back, a `tui` arm at the scenario's
    /// 800 **columns** re-widens `tab_bar_width` to ~766 and would scroll
    /// back to offset 0 if the strip fit — 40 tabs of ~26 columns each
    /// (~1070 total) keeps it overflowed after convergence too, so the
    /// scenario's "the first tab is off screen" precondition holds on every
    /// arm rather than only the pixel-metric one.
    fn overflow_tab_fixture() -> crate::core::Engine {
        let dir = crate::harness::scratch_dir("vimcode_test_1491_tab_bar_overflow");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        for i in 0..40 {
            let path = dir.join(format!("overflow_tab_1491_{i:02}.txt"));
            std::fs::write(&path, format!("OVERFLOW_{i:02}_CONTENT\n")).unwrap();
            engine.new_tab(Some(&path));
        }
        let last = dir.join("last1491.txt");
        std::fs::write(&last, "OVERFLOW_LAST_CONTENT\n").unwrap();
        engine.new_tab(Some(&last));
        let group = engine.active_group;
        engine.goto_tab(0); // the seeded scratch tab
        engine.close_tab();
        // Closing the (now-active) scratch tab re-picks some neighboring tab
        // as active -- re-select `last1491.txt` explicitly (mirrors
        // `two_tab_fixture`'s own `goto_tab` after `close_tab`) rather than
        // assume which one that re-pick landed on.
        let last_idx = engine.editor_groups[&group].tabs.len() - 1;
        engine.goto_tab(last_idx);
        // 30 columns is narrower than two `overflow_tab_1491_NN.txt` tabs
        // (~26 columns each) but wide enough for one, so
        // `TabBar::fit_active_scroll_offset` lands on "the active tab only" —
        // the strip is unambiguously overflowed *and* the tab the scenario
        // has to locate is unambiguously painted, on either backend.
        engine.set_tab_visible_count(group, 30);
        engine.ensure_all_groups_tabs_visible();
        engine
    }

    // Split-group coverage: every scenario above builds its fixture from
    // `two_tab_fixture()`, which has exactly one editor group. `render::build_screen_layout`
    // only produces `Some(editor_group_split)` once `n >= 2` groups exist, so with
    // a single group the split-group tab-bar arm is never entered. `split_two_group_fixture`
    // below gives `editor_group_split` a genuine `Some` by opening a second editor
    // group, so a click aimed at the *non-active* (left) group's tab bar row is
    // hit-tested and dispatched by the split branch specifically.
    fn split_two_group_fixture() -> crate::core::Engine {
        let dir = crate::harness::scratch_dir("vimcode_test_1059_split_tab_bar_dispatch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let left_a = dir.join("left_a1059.txt");
        let left_b = dir.join("left_b1059.txt");
        let right = dir.join("right1059.txt");
        std::fs::write(&left_a, "AAAA_LEFT_1059_CONTENT\n").unwrap();
        std::fs::write(&left_b, "BBBB_LEFT_1059_CONTENT\n").unwrap();
        std::fs::write(&right, "RIGHT_1059_CONTENT\n").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.minimap = false;

        // Left group: two file-backed tabs, mirroring `two_tab_fixture`'s
        // shape -- `left_a1059.txt` ends up inactive (index 0),
        // `left_b1059.txt` active (index 1).
        engine.new_tab(Some(&left_a));
        engine.new_tab(Some(&left_b));
        engine.goto_tab(0); // the seeded scratch tab
        engine.close_tab();
        engine.goto_tab(1); // left_b1059.txt active; left_a1059.txt is not

        // Split: `open_editor_group` makes the new group active and seeds it
        // with a duplicate window onto the buffer that was active in the
        // left group (`left_b1059.txt`'s buffer). Add the real
        // `right1059.txt` tab, then close that duplicate so the right group
        // ends up with exactly one, distinctly-content-tagged tab -- keeping
        // "which content belongs to which group" unambiguous for
        // `screen_has` assertions below.
        engine.open_editor_group(crate::core::window::SplitDirection::Vertical);
        engine.new_tab(Some(&right));
        engine.goto_tab(0);
        engine.close_tab();

        engine
    }

    // ── Split-group Tab arm ──────────────────────────────────────────────
    // Click the *inactive* tab (`left_a1059.txt`) in the *non-active* (left)
    // group's tab bar, while the right group holds global focus. Resolves to
    // `TabBarClickTarget::Tab`, dispatched by the *split*-group arm in
    // `mouse.rs` (the `if let Some(ref split) = layout.editor_group_split`
    // branch) rather than the single-group arm every scenario above
    // exercises.
    //
    // `size: (1600, 480)`, twice every other scenario in this module: at
    // 800 wide, halving the editor area across two groups (minus the
    // sidebar) left too little room for a full two-tab strip in the left
    // group on `gtk` -- `left_a1059.txt`'s label never painted at all (only
    // its breadcrumb copy did), confirmed by dumping `driver.inventory()`
    // before settling on this width. 1600 gives each group's tab bar the
    // same effective room `two_tab_fixture`'s single, unsplit group gets at
    // 800.
    crate::backend_conformance! {
        label: split_tab_bar_click_switches_via_shared_dispatch,
        backends: [gtk, tui, tui_prod],
        engine: split_two_group_fixture(),
        size: (1600, 480),
        body: |driver| {
            assert!(
                driver.screen_has("BBBB_LEFT_1059_CONTENT")
                    && !driver.screen_has("AAAA_LEFT_1059_CONTENT")
                    && driver.screen_has("RIGHT_1059_CONTENT"),
                "precondition: left_b1059.txt is active in the left group, \
                 left_a1059.txt is not, and the right group's own tab is \
                 unaffected"
            );

            driver.drag_text("left_a1059", "left_a1059");
            assert!(
                driver.screen_has("AAAA_LEFT_1059_CONTENT")
                    && !driver.screen_has("BBBB_LEFT_1059_CONTENT"),
                "clicking left_a1059.txt's label in the split (non-active) \
                 group's tab bar must switch to it, on every backend"
            );
            assert!(
                driver.screen_has("RIGHT_1059_CONTENT"),
                "switching tabs in the left group must never disturb the \
                 right group's own content"
            );
        },
    }

    // Click the *inactive* tab's (`left_a1059.txt`) close button directly, in the
    // non-active (left) group's tab bar. Resolves to `ClickTarget::CloseTab`,
    // which `dispatch_tab_bar_target` deliberately leaves unapplied for the caller;
    // `mouse.rs`'s split-group arm makes the `Engine::handle_tab_bar_click` call
    // that decides confirm vs. close.
    crate::backend_conformance! {
        label: split_tab_bar_click_closes_via_shared_dispatch,
        backends: [gtk, tui, tui_prod],
        engine: split_two_group_fixture(),
        // Same `(1600, 480)` widening as `split_tab_bar_click_switches_via_shared_dispatch`
        // above, for the same reason: full width for the left group's
        // two-tab strip to actually paint on `gtk`.
        size: (1600, 480),
        body: |driver| {
            assert!(
                driver.screen_has("left_a1059") && driver.screen_has("left_b1059"),
                "precondition: both left-group tabs are painted"
            );

            let (cx, cy) = tab_close_button_center(driver, "left_a1059");
            driver.drag(cx, cy, cx, cy);
            assert!(
                !driver.screen_has("left_a1059") && driver.screen_has("BBBB_LEFT_1059_CONTENT"),
                "clicking left_a1059.txt's close button in the split \
                 (non-active) group's tab bar must close it and fall back to \
                 the only remaining tab in that group, left_b1059.txt"
            );
            assert!(
                driver.screen_has("RIGHT_1059_CONTENT"),
                "closing a tab in the left group must never disturb the \
                 right group's own content"
            );
        },
    }
}

/// `quadraui::ShellApp::take_requested_panel` was unoverridden on `App` — the
/// trait default always returns `None` — so an app-initiated panel switch
/// (engine call with no runner click involved) never reached the runner's
/// `AppShell`, leaving the sidebar-header title stuck on the previous panel.
/// The sidebar's *content* paints from `engine.app_shell`/`engine.ext_panel_active`
/// directly, so that part always painted correctly. But the runner's own chrome
/// — the sidebar-header title `quadraui::AppShell::render` paints from its own
/// `active_panel()` — has no channel to learn about the switch other than this
/// poll, so it silently kept showing the previous panel's title forever.
/// `screen_has("EXPLORER")` is a safe proxy for that title: that literal
/// all-caps string appears nowhere in either panel's own painted *content*,
/// only in the `PanelDefinition::title` fields the shadow `app_shell` is seeded
/// with, which `quadraui::AppShell::render` echoes into the header.
///
/// Both `gtk` and `tui` below drive the shared `crate::app::App` — on GTK and
/// `quadraui::tui::TuiBackend` respectively — so both arms exercise the
/// identical override and both are expected to flip together.
#[cfg(test)]
mod issue_1064_take_requested_panel {
    use super::*;
    use crate::core::engine::sidebar::PANEL_SEARCH;

    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    /// Shared by the `gtk` and `tui` arms below — both wrap the shared
    /// `App` this issue's fix touches, and both hand back a *live*
    /// `ConformanceHarness::engine` (an `Rc<RefCell<Engine>>` shared with
    /// the running app), which is exactly what this scenario needs to
    /// simulate an app-initiated switch: reaching in and moving the shadow
    /// `engine.app_shell` directly, with no runner click at all.
    fn app_initiated_switch_reconciles_runner_chrome<D>(
        driver: &mut D,
        engine: &std::rc::Rc<std::cell::RefCell<crate::core::Engine>>,
    ) where
        D: ConformanceDriver + DriverInput,
    {
        // `Engine::new_for_test()`'s `AppShell` (and the runner's own,
        // built from the same panel list in `App::shell_config`) both
        // start with the sidebar already open on Explorer — the default
        // active panel (index 0) with `sidebar_visible() == true` — so
        // shadow and runner already agree before anything below runs; the
        // switch below is the *only* variable under test. (Clicking the
        // Explorer icon here, as the other panel-open scenarios in this
        // file do for their own non-default target panel, would instead
        // *close* it — `AppShell::handle_activity_click`'s own "click on
        // the already-active, already-visible panel" branch.)
        assert!(
            driver.screen_has("EXPLORER"),
            "precondition: a fresh engine must start with the sidebar \
             open on Explorer"
        );

        // App-initiated switch: touches only the shadow `engine.app_shell`,
        // the same shape `Engine::process_pending_sidebar`'s DAP
        // `dap_wants_sidebar` reveal and `App::toggle_focus_search`'s
        // keyboard accelerator both take — no runner click, so no chance
        // for `AppShell::handle`'s own hit-testing to update the runner's
        // chrome on its own.
        engine.borrow_mut().focus_sidebar_panel(PANEL_SEARCH);

        // Poke the runner so `ShellAdapter::handle`/`apply_requested_panel` polls
        // `take_requested_panel` again — direct engine mutation, then a dispatch to
        // force the poll. `dispatch` repaints on its own whenever the reaction is
        // `Redraw`, so no separate `render()` call is needed here.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));

        assert!(
            driver.screen_has("SEARCH") && !driver.screen_has("EXPLORER"),
            "#1064: an app-initiated panel switch (no runner click) must \
             steer the runner's own chrome to the new panel — without \
             `take_requested_panel`, the sidebar-header title stays on the \
             previous panel forever even though the content pane (reading \
             the shadow `engine.app_shell` directly) already switched"
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn gtk() {
        let mut h = crate::gtk::testing::conformance_harness(engine_fixture(), 800, 480);
        app_initiated_switch_reconciles_runner_chrome(&mut h.driver, &h.engine);
    }

    #[test]
    fn tui() {
        // `App::shell_config`'s `cell`-profile hamburger
        // `PanelDefinition` now occupies index 0 in the *runner's* fresh
        // `AppShell` (`quadraui::AppShell::new` always activates index 0),
        // so a bare `engine_fixture()` — whose shadow `engine.app_shell`
        // starts hidden, matching `Engine::new_for_test()`'s deterministic
        // default — paints "Menu" (the hamburger's own header) at frame
        // zero instead of "EXPLORER". Pin the shadow's sidebar open on
        // Explorer up front (matching this scenario's own doc, "both start
        // with the sidebar open on Explorer") and land one reconciling
        // dispatch — the same `WindowFocused` "poke" pattern
        // `render::sync_runner_sidebar_visibility`'s own doc and every
        // hamburger scenario in `issue_1427_menu_bar_reveal_shared` uses —
        // so `ShellApp::take_requested_panel` steers the runner's chrome
        // onto Explorer before this scenario's own precondition reads it.
        // `gtk` needs neither: GTK never registers the hamburger panel, so
        // its runner already defaults to Explorer at index 0.
        let mut engine = engine_fixture();
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine.session.explorer_visible = true;
        let mut h = crate::tui_main::testing::conformance_harness(engine, 800, 480);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        app_initiated_switch_reconciles_runner_chrome(&mut h.driver, &h.engine);
    }

    /// This arm is built identically to `tui` above (the independently hand-written
    /// production TUI shell this arm used to exercise is gone). Kept as a separate,
    /// named arm anyway rather than deleted, so the many existing `tui_prod`-suffixed
    /// scenarios and `KNOWN_BUGS` labels across this file keep resolving.
    #[test]
    fn tui_prod() {
        let mut engine = engine_fixture();
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine.session.explorer_visible = true;
        let mut h = crate::tui_main::testing::conformance_harness_prod(engine, 800, 480);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        app_initiated_switch_reconciles_runner_chrome(&mut h.driver, &h.engine);
    }
}

/// Does GTK compose the Settings/Extensions sidebar header and search chrome
/// somewhere other than `App::paint_sidebar_panel_rung`, or is it missing?
/// Originally answered by driving all three shells, finding that the shared
/// `App` now paints the shell's own `AppShell` header (title from
/// `PanelDefinition.title`) and both panels paint a search/filter row through
/// `SidebarPanelBody::render_with` with `SidebarPanelChrome::Search` chrome.
#[cfg(test)]
mod issue_1256_sidebar_chrome {
    use super::*;

    /// A query no settings label contains, so it can only reach the screen
    /// through a painted filter row.
    const SETTINGS_QUERY: &str = "qqzz1256";

    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    fn engine_with_settings_query() -> crate::core::Engine {
        let mut engine = engine_fixture();
        engine.settings_query = SETTINGS_QUERY.to_string();
        engine
    }

    fn open_extensions<D: ConformanceDriver>(driver: &mut D) {
        driver.click_text(crate::icons::EXTENSIONS.s());
        assert!(
            driver.screen_has("AVAILABLE"),
            "precondition: clicking the Extensions icon must paint the \
             extensions sidebar body"
        );
    }

    fn open_settings<D: ConformanceDriver>(driver: &mut D) {
        driver.click_text(crate::icons::SETTINGS.s());
    }

    fn extensions_header_is_painted<D: ConformanceDriver>(driver: &mut D) {
        open_extensions(driver);
        // Case differs by arm: the shared `App`'s `PanelDefinition` titles are
        // all-caps ("EXTENSIONS"), the pre-#1434 TUI shell's `shell_config`'s own are
        // title-case ("Extensions") — both are the shell's *one* header, not a second
        // hand-painted row.
        assert!(
            driver.screen_has("EXTENSIONS") || driver.screen_has("Extensions"),
            "#1256: the Extensions sidebar must paint a header naming it: {:?}",
            driver.inventory()
        );
    }

    fn extensions_search_row_is_painted<D: ConformanceDriver>(driver: &mut D) {
        open_extensions(driver);
        assert!(
            driver.screen_has("Search extensions"),
            "#1256: the Extensions sidebar must paint its search row"
        );
    }

    fn settings_header_names_settings<D: ConformanceDriver>(driver: &mut D) {
        open_settings(driver);
        assert!(
            driver.screen_has("Appearance"),
            "precondition: clicking the Settings icon must paint the \
             settings form body"
        );
        // Case differs by arm: the shared `App`'s `PanelDefinition` titles are
        // all-caps ("SETTINGS"), the pre-#1434 TUI shell's `shell_config`'s own
        // bottom-item title is title-case ("Settings").
        assert!(
            (driver.screen_has("SETTINGS") || driver.screen_has("Settings"))
                && !driver.screen_has("EXPLORER")
                && !driver.screen_has("Explorer"),
            "#1256: the Settings sidebar must be headed SETTINGS, not the \
             previous panel's title: {:?}",
            driver.inventory()
        );
    }

    fn settings_filter_row_is_painted<D: ConformanceDriver>(driver: &mut D) {
        open_settings(driver);
        assert!(
            driver.screen_has(SETTINGS_QUERY),
            "#1256: the Settings sidebar must paint its filter row showing \
             the active query"
        );
    }

    /// Once Settings — a bottom item — owns the sidebar header, clicking a *top*
    /// panel's icon must take the header back rather than leaving it stuck on
    /// Settings. Before the quadraui bump for quadraui#1055/#1056,
    /// `AppShell::handle_activity_click`'s top-panel branch never cleared
    /// `sidebar_bottom_owner`, so the header would have stayed on Settings even
    /// though Explorer's icon was clicked.
    fn settings_then_explorer_reclaims_header<D: ConformanceDriver>(driver: &mut D) {
        open_settings(driver);
        assert!(
            driver.screen_has("Appearance"),
            "precondition: clicking the Settings icon must paint the \
             settings form body"
        );

        driver.click_text(crate::icons::EXPLORER.s());

        // Case differs by arm: the shared `App`'s `PanelDefinition` titles are
        // all-caps ("EXPLORER"), the pre-#1434 TUI shell's `shell_config`'s own are
        // title-case ("Explorer").
        assert!(
            driver.screen_has("EXPLORER") || driver.screen_has("Explorer"),
            "#1356: clicking the Explorer icon while Settings owns the \
             sidebar must reclaim the header for Explorer, not leave it \
             stuck on Settings: {:?}",
            driver.inventory()
        );
        assert!(
            !driver.screen_has("SETTINGS"),
            "#1356: clicking the Explorer icon while Settings owns the \
             sidebar must reclaim the header for Explorer, not leave it \
             stuck on Settings: {:?}",
            driver.inventory()
        );
        assert!(
            !driver.screen_has("Appearance"),
            "#1356: the sidebar body must switch away from the Settings \
             form once Explorer reclaims the sidebar"
        );
    }

    /// The shipped TUI must paint exactly *one* header naming the Extensions
    /// panel — not the shell's own header row plus a second, hand-painted duplicate.
    /// This counts painted text runs instead of just checking presence, since
    /// presence alone can't distinguish "painted once" from "painted twice".
    /// Scoped to `tui_prod` only: GTK and the `tui` control arm never had a
    /// hand-painted second header.
    #[test]
    fn tui_prod_paints_exactly_one_extensions_header() {
        let mut h = crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 100, 30);
        open_extensions(&mut h.driver);
        let inv = h.driver.inventory();
        // Case differs by arm: the pre-#1434 TUI shell's `shell_config`'s title is
        // "Extensions", not "EXTENSIONS"; sum both so a regression that reintroduces
        // the all-caps duplicate still trips this either way.
        let count = inv.count("EXTENSIONS") + inv.count("Extensions");
        assert_eq!(
            count, 1,
            "#1343: the shipped TUI must paint exactly one Extensions \
             header, not the shell's plus a second hand-painted \
             duplicate: {inv:?}"
        );
    }

    /// Settings twin of [`tui_prod_paints_exactly_one_extensions_header`] — the
    /// shipped TUI must paint exactly one Settings header.
    #[test]
    fn tui_prod_paints_exactly_one_settings_header() {
        let mut h = crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 100, 30);
        open_settings(&mut h.driver);
        let inv = h.driver.inventory();
        let count = inv.count("SETTINGS") + inv.count("Settings");
        assert_eq!(
            count, 1,
            "#1343: the shipped TUI must paint exactly one Settings \
             header, not the shell's plus a second hand-painted \
             duplicate: {inv:?}"
        );
    }

    macro_rules! arms {
        ($name:ident, $engine:expr) => {
            mod $name {
                use super::*;

                #[cfg(feature = "gui")]
                #[test]
                fn gtk() {
                    let mut h = crate::gtk::testing::conformance_harness($engine, 800, 480);
                    known_bug_gate(
                        concat!("issue_1256_sidebar_chrome::", stringify!($name), "::gtk"),
                        || super::$name(&mut h.driver),
                    );
                }

                #[test]
                fn tui() {
                    let mut h = crate::tui_main::testing::conformance_harness($engine, 100, 30);
                    known_bug_gate(
                        concat!("issue_1256_sidebar_chrome::", stringify!($name), "::tui"),
                        || super::$name(&mut h.driver),
                    );
                }

                #[test]
                fn tui_prod() {
                    let mut h =
                        crate::tui_main::testing::conformance_harness_prod($engine, 100, 30);
                    known_bug_gate(
                        concat!(
                            "issue_1256_sidebar_chrome::",
                            stringify!($name),
                            "::tui_prod"
                        ),
                        || super::$name(&mut h.driver),
                    );
                }
            }
        };
    }

    arms!(extensions_header_is_painted, engine_fixture());
    arms!(extensions_search_row_is_painted, engine_fixture());
    arms!(settings_header_names_settings, engine_fixture());
    arms!(settings_then_explorer_reclaims_header, engine_fixture());
    arms!(settings_filter_row_is_painted, engine_with_settings_query());
}

/// Cross-backend wiring for [`activity_bar_click_focuses_search_panel`] — see
/// that scenario's own doc for the repro and the assertions' rationale.
///
/// Width bumped `800` -> `1600`: the ruler (`Ln N, Col N`) this scenario's
/// precondition and assertions all read moved to the front of the right-side
/// priority-drop order (VS Code parity — it is now the *leftmost* segment of
/// the right group, not the protected right-most one), so the `gtk` arm's real
/// pixel geometry — sidebar + activity bar default-visible chrome subtracted
/// from `800`px — no longer leaves it room. `1600` is shared with `tui`/
/// `tui_prod` too (one `size` for all three arms).
#[cfg(test)]
mod issue_1360_activity_bar_click_focuses_panel {
    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        let text = (1..=30)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        engine.buffer_mut().insert(0, &text);
        engine
    }

    crate::backend_conformance! {
        label: activity_bar_click_focuses_search_panel_proof,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (1600, 480),
        body: |driver| {
            crate::harness::activity_bar_click_focuses_search_panel(driver);
        },
    }
}

/// At a realistic `(80, 24)` terminal, the shared `App` (driven by
/// `quadraui::tui::TuiBackend`) must paint the buffer's first line directly
/// below a 1-row tab bar, with the status bar on row 22 and the command line
/// on row 23 — proof that `render::UnitProfile::cell()` reserves the tab bar
/// as one cell row (not `TAB_ROW_HEIGHT_PX`'s pixel height) rather than
/// collapsing the whole editor content band.
#[cfg(test)]
mod issue_1426_app_on_tui_rows_match_shipped_tui {
    use quadraui::testing::ConformanceDriver;

    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.buffer_mut().insert(0, "ZQXW1426_ROWLINE\n");
        engine.message = "ZQXW1426_CMDLINE".to_string();
        engine
    }

    /// Click the Explorer activity-bar icon twice, collapsing the sidebar's
    /// screen-space reservation — mirrors the collapse behavior where the *first*
    /// click on Explorer's icon activates it rather than collapsing an
    /// already-active panel; a single click here left the sidebar open, painting
    /// the explorer tree instead of the tab bar. `set_double_click_folding(false)`
    /// prevents two same-position clicks with no simulated time between them from
    /// folding into one `UiEvent::DoubleClick`, which would only ever activate
    /// Explorer once.
    fn collapse_sidebar(driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>) {
        driver.set_double_click_folding(false);
        let explorer_bounds = |driver: &mut quadraui::tui::testing::TuiDriver<_>| {
            driver
                .inventory()
                .zones()
                .iter()
                .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
                .map(|z| z.bounds)
                .expect("the Explorer activity-bar icon must register a chrome zone")
        };
        for _ in 0..2 {
            let zone = explorer_bounds(driver);
            driver.click(zone.x + zone.width / 2.0, zone.y + zone.height / 2.0);
        }
    }

    #[test]
    fn render_content_paints_buffer_status_and_command_line_on_shipped_tui_rows_tui() {
        let mut h = crate::tui_main::testing::conformance_harness(engine_fixture(), 80, 24);
        let driver = &mut h.driver;
        collapse_sidebar(driver);
        let screen = driver.screen();
        let lines: Vec<&str> = screen.lines().collect();
        assert_eq!(
            lines.len(),
            24,
            "an 80x24 terminal must paint exactly 24 rows; screen:\n{screen}"
        );
        assert!(
            lines[22].contains("Ln 1,"),
            "row 22 (second-to-last) must paint the status bar's cursor \
             position; row:\n{}",
            lines[22]
        );
        assert!(
            lines[23].contains("ZQXW1426_CMDLINE"),
            "row 23 (last) must paint the command line; row:\n{}",
            lines[23]
        );

        // Was gated as `render_content_paints_buffer_status_and_
        // command_line_on_shipped_tui_rows::tui` — see this module's own
        // doc for why that gate was a stale test fixture, not a real `App`
        // gap. Ungated. Row 1 is the (empty, since `[No Name]` has no path)
        // breadcrumb row — `tui_prod` reserves the identical row here too.
        assert!(
            lines[0].contains("[No Name]"),
            "row 0 must paint the tab bar; row:\n{}",
            lines[0]
        );
        assert!(
            lines[2].contains("ZQXW1426_ROWLINE"),
            "row 2, directly below the 1-row tab bar and the (empty) \
             breadcrumb row, must paint the buffer's first line; row:\n{}",
            lines[2]
        );
    }
}

/// Cross-backend wiring for [`sidebar_ctrl_w_l_returns_focus_to_the_editor`]
/// and [`sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar`] — see
/// those scenarios' own docs for the repro and the assertions' rationale.
///
/// The `l` proof's width is bumped `800` -> `1600`: the ruler (`Ln N, Col N`)
/// moved to the front of the right-side priority-drop order, so the `gtk`
/// arm's real pixel geometry no longer leaves it room.
#[cfg(test)]
mod issue_406_sidebar_ctrl_w_navigates {
    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        let text = (1..=30)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        engine.buffer_mut().insert(0, &text);
        engine
    }

    crate::backend_conformance! {
        label: sidebar_ctrl_w_l_returns_focus_to_the_editor_proof,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (1600, 480),
        body: |driver| {
            crate::harness::sidebar_ctrl_w_l_returns_focus_to_the_editor(driver);
        },
    }

    crate::backend_conformance! {
        label: sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar_proof,
        backends: [gtk, tui_prod],
        engine: engine_fixture(),
        size: (800, 480),
        body: |driver| {
            crate::harness::sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar(driver);
        },
    }

    crate::backend_conformance! {
        label: panel_toolbar_round_trip_returns_to_the_same_panel_proof,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (1600, 480),
        body: |driver| {
            crate::harness::panel_toolbar_round_trip_returns_to_the_same_panel(driver);
        },
    }

    crate::backend_conformance! {
        label: panel_toolbar_round_trip_uses_the_active_panel_not_the_toolbar_cursor_proof,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (1600, 480),
        body: |driver| {
            crate::harness::panel_toolbar_round_trip_uses_the_active_panel_not_the_toolbar_cursor(driver);
        },
    }

    crate::backend_conformance! {
        label: toolbar_ctrl_w_left_is_a_noop_proof,
        backends: [gtk, tui_prod],
        engine: engine_fixture(),
        size: (800, 480),
        body: |driver| {
            crate::harness::toolbar_ctrl_w_left_is_a_noop(driver);
        },
    }

    crate::backend_conformance! {
        label: rebound_ctrl_h_ctrl_l_walk_the_same_chain_proof,
        backends: [gtk, tui, tui_prod],
        engine: crate::harness::engine_fixture_with_rebound_wincmd_keys(),
        size: (1600, 480),
        body: |driver| {
            crate::harness::rebound_ctrl_h_ctrl_l_walk_the_same_chain(driver);
        },
    }
}

/// `render::apply_explorer_context_action` converges what used to be two
/// hand-rolled, drifted appliers — TUI's `handle_explorer_context_action` and
/// GTK's `App::dispatch_explorer_ctx_action` — into one function both backends
/// now call from `render::ModalKeyRoute::ContextMenu`'s key rung. One
/// conformance scenario per action below, each pre-building the explorer
/// context menu with the target item already highlighted so the scenario body
/// only has to press `Enter` and read the painted result.
///
/// `backends: [gtk, tui_prod]` only (no `tui`): `App::dispatch_context_menu_key`
/// is shared code between the `gtk` arm and the `tui` "control" arm (`App` on
/// `quadraui::tui::TuiBackend`), so `tui` would just be a second copy of the
/// same result — `tui_prod` (the pre-#1434 TUI shell, the shell `tui_main::run`
/// actually ships) is the one genuinely independent implementation.
#[cfg(test)]
mod issue_1418_explorer_context_menu {
    use super::*;

    /// Build an engine with one folder (`root`, containing `marker.txt`)
    /// already shown in an open, populated explorer sidebar, with its
    /// context menu already open and `selected_idx` already highlighted —
    /// one of the 9 items `Engine::open_explorer_context_menu`'s `is_dir`
    /// branch builds, in that fixed order (0 new_file, 1 new_folder,
    /// 2 reveal, 3 open_terminal, 4 find_in_folder, 5 copy_path,
    /// 6 copy_relative_path, 7 rename, 8 delete).
    fn engine_with_folder_ctx_menu(tag: &str, selected_idx: usize) -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1418_ctxmenu_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker.txt"), "hello").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir.clone();
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        // `session.explorer_visible` alone leaves the shadow `engine.app_shell`'s
        // `sidebar_visible()` stale. This scenario runs on `App` on a `TuiBackend`,
        // which needs this reconciliation (see `render::sync_runner_sidebar_visibility`,
        // called unconditionally on every dispatch, which would otherwise collapse the
        // sidebar the moment this scenario's own `Enter` confirm below dispatches
        // anything).
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine.open_explorer_context_menu(dir, true, 5, 5);
        engine.context_menu.as_mut().unwrap().selected = selected_idx;
        engine
    }

    // "New File..." (idx 0): confirming must start the tree's inline-edit
    // placeholder (`Engine::dispatch_explorer_crud`'s `NewFile` arm),
    // painted on both backends (`quadraui::tui::tree`'s `set_cell` loop /
    // `quadraui::gtk::tree`'s `show_layout` call, both gated on
    // `edit.placeholder`).
    //
    // RED-verified: with `apply_explorer_context_action`'s
    // `"new_file"`/`"new_folder"`/`"rename"` arm changed to a bare
    // `_ => {}` (never calling `dispatch_explorer_crud`), "New file
    // name..." goes unpainted on both arms below.
    crate::backend_conformance! {
        label: context_menu_new_file_starts_inline_edit,
        backends: [gtk, tui_prod],
        engine: engine_with_folder_ctx_menu("new_file", 0),
        size: (800, 480),
        body: |driver| {
            // "file name" rather than the full "New file name..." — TUI's
            // `paint_edit_input` (quadraui `tui/tree.rs`) always overwrites the
            // placeholder's first painted cell with an inverted-space cursor block, so the
            // leading "N" never survives into a painted text run there (GTK draws the
            // caret as a separate overlay rectangle instead) — a backend rasterization
            // difference, not part of what this scenario is proving. Also dropping the
            // trailing "..." — the shared `App`'s fixed-width TUI sidebar truncates the
            // placeholder's ellipsis before the row's right edge.
            assert!(
                !driver.screen_has("file name"),
                "precondition: nothing is being edited yet"
            );
            driver.press_named(NamedKey::Enter);
            assert!(
                driver.screen_has("file name"),
                "confirming 'New File...' must start the tree's inline-edit \
                 placeholder"
            );
        },
    }

    // "Delete" (idx 8): confirming must open the delete-confirmation
    // dialog (`Engine::confirm_delete_file`, titled "Confirm Delete") —
    // using the context menu's own explicit target, not whichever row the
    // explorer tree happens to have selected (`ExplorerAction::from_action_str`'s
    // own doc comment on why those two can disagree).
    //
    // `tui_prod` only (no `gtk`): `confirm_delete_file`'s dialog (no
    // table, no text input) is natively-expressible on GTK
    // — it never paints in-canvas there, so `screen_has("Confirm Delete")`
    // cannot be this scenario's GTK proof. GTK's own coverage
    // (`native_dialog_shown`/`pending_native_dialog` bookkeeping, the
    // established idiom this file's `native_dialog_presented_exactly_once_
    // across_repeated_frames` etc. already use) lives in
    // `src/gtk/testing.rs`'s `explorer_context_menu_delete_opens_native_
    // confirm_dialog` instead — see that test's own doc for why it's a
    // separate assertion vocabulary, not weaker coverage.
    //
    // RED-verified: with the `"delete"` arm changed to a bare `_ => {}`,
    // "Confirm Delete" goes unpainted below.
    crate::backend_conformance! {
        label: context_menu_delete_opens_confirm_dialog,
        backends: [tui_prod],
        engine: engine_with_folder_ctx_menu("delete", 8),
        size: (800, 480),
        body: |driver| {
            assert!(
                !driver.screen_has("Confirm Delete"),
                "precondition: no dialog is open yet"
            );
            driver.press_named(NamedKey::Enter);
            assert!(
                driver.screen_has("Confirm Delete"),
                "confirming 'Delete' must open the delete-confirmation dialog"
            );
        },
    }

    // "Open in Integrated Terminal" (idx 3): confirming must open a real
    // terminal tab in the bottom panel (`Engine::terminal_new_tab_at`),
    // same painted "Terminal" tab-strip label
    // `bottom_panel_tab_strip_click_switches_the_painted_panel` (GTK-only,
    // above) already asserts on.
    //
    // `tui_prod` only (no `gtk`): GTK's menu bar has its own permanent
    // "Terminal" top-level menu item (`File Edit View Go Run Terminal
    // Help`), so a bare `screen_has("Terminal")` is true on GTK whether or
    // not a terminal tab ever opens — confirmed empirically (`count
    // ("Terminal")` reads 2 both with and without this scenario's fix
    // applied: the still-open context menu's own "Open in Integrated
    // Terminal" item contributes one occurrence pre-confirm, the new tab
    // strip's "Terminal" label contributes one post-confirm, so the *count*
    // doesn't move even though what produced it did). GTK's own coverage —
    // `src/gtk/testing.rs`'s `explorer_context_menu_open_terminal_opens_
    // terminal_tab`, which measures the count *before the context menu
    // ever opens* to get a real baseline — is what closes that gap; see
    // its own doc for why a plain `screen_has`/`count` inside this shared
    // body can't.
    //
    // RED-verified: with the `"open_terminal"` arm changed to a bare
    // `_ => {}`, no "Terminal" tab is ever painted below.
    crate::backend_conformance! {
        label: context_menu_open_terminal_opens_terminal_tab,
        backends: [tui_prod],
        engine: engine_with_folder_ctx_menu("open_terminal", 3),
        size: (1400, 900),
        body: |driver| {
            // No "no terminal tab is open yet" precondition here — the
            // still-open context menu's own "Open in Integrated Terminal"
            // label already contains the substring "Terminal", so a
            // `!screen_has("Terminal")` check would be asserting against
            // the menu, not the (yet-to-open) terminal panel.
            driver.press_named(NamedKey::Enter);
            assert!(
                driver.screen_has("Terminal"),
                "confirming 'Open in Integrated Terminal' must open a \
                 terminal tab"
            );
        },
    }

    /// Marker text shared by both fixture files
    /// [`engine_with_scoped_grep_ctx_menu`] writes — present in both, so a
    /// query matching it alone cannot tell scoped from unscoped; the
    /// per-directory suffix below is what a scoped search must filter on.
    const SCOPED_GREP_COMMON: &str = "zqxw1418ctxmenugrep";

    /// Build an engine with two sibling folders under one cwd —
    /// `scoped_dir` (containing a file matching `SCOPED_GREP_COMMON` +
    /// `"scoped"`) and `other_dir` (containing a file matching
    /// `SCOPED_GREP_COMMON` + `"other"`) — with `scoped_dir`'s own context
    /// menu open and "Find in Folder..." (idx 4) already highlighted.
    fn engine_with_scoped_grep_ctx_menu(tag: &str) -> crate::core::Engine {
        let root = std::env::temp_dir().join(format!(
            "vimcode_test_1418_grep_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let scoped_dir = root.join("scoped_dir");
        let other_dir = root.join("other_dir");
        std::fs::create_dir_all(&scoped_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        std::fs::write(
            scoped_dir.join("in_scope.txt"),
            format!("{SCOPED_GREP_COMMON}scoped"),
        )
        .unwrap();
        std::fs::write(
            other_dir.join("out_of_scope.txt"),
            format!("{SCOPED_GREP_COMMON}other"),
        )
        .unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = root.clone();
        engine.explorer_expanded.insert(root.clone());
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        engine.open_explorer_context_menu(scoped_dir, true, 5, 5);
        engine.context_menu.as_mut().unwrap().selected = 4;
        engine
    }

    // "Find in Folder..." (idx 4) must now open the Grep picker, titled
    // `"Grep in scoped_dir/"`, scoped to the clicked folder: typing the marker
    // query both fixture files share must surface only `scoped_dir`'s own match.
    crate::backend_conformance! {
        label: context_menu_find_in_folder_scopes_grep_to_the_folder,
        backends: [gtk, tui_prod],
        engine: engine_with_scoped_grep_ctx_menu("find_in_folder"),
        size: (800, 480),
        body: |driver| {
            assert!(
                !driver.screen_has("Grep in scoped_dir"),
                "precondition: the scoped Grep picker is not open yet"
            );
            driver.press_named(NamedKey::Enter);
            assert!(
                driver.screen_has("Grep in scoped_dir"),
                "confirming 'Find in Folder...' must open the Grep picker \
                 titled with the scoped folder (GTK used to just focus \
                 the Search panel instead — a different feature, not a \
                 scoped version of this one)"
            );
            driver.type_text(SCOPED_GREP_COMMON);
            assert!(
                driver.screen_has("in_scope.txt"),
                "the scoped folder's own match must appear"
            );
            assert!(
                !driver.screen_has("out_of_scope.txt"),
                "a sibling folder's match must be filtered out by the \
                 folder scope — TUI used to search the whole workspace \
                 unscoped"
            );
        },
    }
}

/// The TUI menu-bar reveal/hide rung (the Alt+<letter> shim, the hamburger-corner-click
/// guard, and the hamburger panel's own reveal/hide) lifted out of the pre-#1434 TUI
/// shell into shared `render.rs` functions so `App` — the cross-backend-shared
/// shell every `gtk`/`tui`/`macos`/`win` conformance arm already drives — shares
/// them too, gated on `BackendCaps::window_chrome` in `App::setup` rather than
/// reimplemented per backend.
///
/// These are `tui`-arm ports of scenarios that, before #1427, only existed as
/// the pre-#1434 TUI shell-only unit tests in `src/tui_main/shell_app.rs`.
/// Driving `App` on `quadraui::tui::TuiBackend` (via
/// `crate::tui_main::testing::conformance_harness`) rather than the pre-#1434
/// TUI shell proves the shared implementation, not a second TUI-only reimplementation
/// of it.
#[cfg(test)]
mod issue_1427_menu_bar_reveal_shared {
    use super::*;
    use crate::core::engine::sidebar::PANEL_EXPLORER;

    fn engine_fixture() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    /// [`engine_fixture`], plus the sidebar pinned open on Explorer — needed for the
    /// same two reasons: (1) `Engine::new_for_test()`'s sidebar is hidden by default,
    /// which would leave the File dropdown painting flush against the left edge and
    /// erasing an editor-offset-0 marker; and (2), specific to the hamburger tests
    /// below: `App::shell_config`'s `cell`-profile hamburger `PanelDefinition`
    /// occupies index 0 in the *runner's* fresh `AppShell`, so the very first
    /// hamburger click only produces the reveal needed — rather than a toggle-hide
    /// — once construction's initial `take_requested_panel` poll has somewhere real
    /// (Explorer) to reconcile the runner onto first.
    fn engine_with_sidebar_open() -> Engine {
        let mut engine = engine_fixture();
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        engine.session.explorer_visible = true;
        engine
    }

    /// Alt+F must reveal the (hidden by default) menu bar *and* hand the same
    /// keystroke to the `MenuSystem` intercept, which activates the File menu — both
    /// of which redraw, and the reveal reserves one more row above the editor
    /// content, shifting a marker down by exactly one line.
    ///
    /// A marker at buffer offset 0 would sit inside the File dropdown's footprint
    /// and get column-clipped instead of testing the row shift this scenario is
    /// about, so the marker lives 15 blank lines down — below the File dropdown's
    /// current height, comfortably inside this 24-row viewport either side of the
    /// Alt-reveal.
    #[test]
    fn alt_letter_reveals_menu_bar_via_app_on_tui() {
        let mut engine = engine_with_sidebar_open();
        engine
            .buffer_mut()
            .insert(0, &format!("{}ZQXW_ALT_MARKER", "\n".repeat(15)));
        let mut h = crate::tui_main::testing::conformance_harness(engine, 80, 24);

        let before = h.driver.screen();
        let before_row = before
            .lines()
            .position(|l| l.contains("ZQXW_ALT_MARKER"))
            .expect("marker should paint before the Alt-reveal keypress");

        let reaction = h.driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('f'),
            modifiers: quadraui::Modifiers {
                alt: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });
        assert_eq!(
            reaction,
            quadraui::Reaction::Redraw,
            "Alt+F should reveal + activate the File menu, both of which redraw"
        );

        let after = h.driver.screen();
        let after_row = after
            .lines()
            .position(|l| l.contains("ZQXW_ALT_MARKER"))
            .unwrap_or_else(|| {
                panic!("marker should still paint after the Alt-reveal keypress; after:\n{after}")
            });
        assert_eq!(
            after_row,
            before_row + 1,
            "revealing the menu bar should reserve one more row above the \
             editor content, shifting the marker down by exactly one line; \
             before:\n{before}\nafter:\n{after}"
        );
    }

    /// Port of `driver_click_on_settings_toggle_with_menu_bar_visible_
    /// flips_its_own_row_via_shell_app`: with the menu bar already visible
    /// (occupying its own reserved row above the sidebar/activity bar,
    /// `render::sync_menu_bar_title_row`'s seeded-at-construction half —
    /// `App::shell_config`'s `cell`-profile `has_title_bar` branch), a click
    /// on a settings toggle row still resolves to the *painted* row, not a
    /// stale one-row-too-high guess.
    #[test]
    fn driver_click_on_settings_toggle_with_menu_bar_visible_flips_its_own_row_via_app_on_tui() {
        use crate::core::engine::sidebar::PANEL_SETTINGS;

        let mut engine = engine_fixture();
        engine.menu_bar_visible = true;
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_SETTINGS));
        assert!(
            engine.settings.cursorline,
            "fixture assumes cursorline defaults to true"
        );

        let mut h = crate::tui_main::testing::conformance_harness(engine, 80, 24);
        let before = h
            .driver
            .find_bounds("Cursor Line")
            .expect("the \"Cursor Line\" toggle row must paint");

        // Click mid-row (not on the label's own first cell) — the whole
        // row is the control's hit target, so this still proves the click
        // resolved to *this* row's control, not a fluke hit on the label
        // glyph itself.
        h.driver.click(before.x + 2.0, before.y);

        let screen = h.driver.screen();
        let clicked_row = screen.lines().nth(before.y as usize).unwrap_or_default();
        assert!(
            clicked_row.contains("[ ]"),
            "clicking the \"Cursor Line\" row should flip its own toggle off (was \"[x]\"); \
             row:\n{clicked_row}\nfull screen:\n{screen}"
        );
    }

    /// Port of `hamburger_relocated_click_after_reveal_hides_menu_bar`: a
    /// full reveal → correctly-relocated-hide → reveal cycle through the
    /// hamburger `PanelDefinition`'s own click path
    /// (`render::route_hamburger_panel_changed` /
    /// `render::route_hamburger_sidebar_hidden`), each click re-located from
    /// the frame the *previous* click actually painted — never a stored
    /// coordinate, so a real row-shift bug can't hide behind a stale
    /// assertion.
    #[test]
    fn hamburger_relocated_click_after_reveal_hides_menu_bar_via_app_on_tui() {
        let mut h = crate::tui_main::testing::conformance_harness(engine_fixture(), 80, 24);
        h.driver.set_double_click_folding(false);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();

        assert!(
            !h.driver.screen_contains("File"),
            "precondition: menu bar starts hidden; screen:\n{}",
            h.driver.screen()
        );

        let hamburger = crate::icons::HAMBURGER.s();
        let (hx1, hy1) = h
            .driver
            .find(hamburger)
            .expect("hamburger icon must paint on the activity bar");
        h.driver.click(hx1, hy1);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();
        assert!(
            h.driver.screen_contains("File"),
            "first hamburger click must reveal the menu row; screen:\n{}",
            h.driver.screen()
        );

        let (hx2, hy2) = h
            .driver
            .find(hamburger)
            .expect("hamburger icon must still paint with the menu bar open");
        assert_ne!(
            (hx1, hy1),
            (hx2, hy2),
            "sanity: revealing the menu bar must shift the hamburger's own \
             row, or this test isn't exercising the row-shift this issue is \
             about"
        );
        h.driver.click(hx2, hy2);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();
        assert!(
            !h.driver.screen_contains("File"),
            "second hamburger click, correctly re-located, must hide the \
             menu row again; screen:\n{}",
            h.driver.screen()
        );

        let (hx3, hy3) = h
            .driver
            .find(hamburger)
            .expect("hamburger icon must still paint with the menu bar hidden again");
        assert_eq!(
            (hx1, hy1),
            (hx3, hy3),
            "sanity: hiding the menu bar must shift the hamburger back to \
             its original row"
        );
        h.driver.click(hx3, hy3);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();
        assert!(
            h.driver.screen_contains("File"),
            "a third hamburger click must reveal the menu row again — the \
             toggle must survive a second on/off cycle, not just the \
             first; screen:\n{}",
            h.driver.screen()
        );
    }

    /// Port of `hamburger_stale_click_position_after_reveal_still_hides_
    /// menu_bar`: a second click at the *same physical
    /// position* the first (reveal) click used — the stale-position muscle
    /// memory a hamburger reveal leaves behind — must still hide the menu
    /// bar via `render::route_menu_bar_reveal`'s stale-corner-click guard,
    /// rather than falling into the `MenuSystem` intercept and opening
    /// whichever menu now paints at that column.
    #[test]
    fn hamburger_stale_click_position_after_reveal_still_hides_menu_bar_via_app_on_tui() {
        let mut h = crate::tui_main::testing::conformance_harness(engine_fixture(), 80, 24);
        h.driver.set_double_click_folding(false);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();

        assert!(
            !h.driver.screen_contains("File"),
            "precondition: menu bar starts hidden; screen:\n{}",
            h.driver.screen()
        );

        let hamburger = crate::icons::HAMBURGER.s();
        let (hx1, hy1) = h
            .driver
            .find(hamburger)
            .expect("hamburger icon must paint on the activity bar");
        h.driver.click(hx1, hy1);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();
        assert!(
            h.driver.screen_contains("File"),
            "first hamburger click must reveal the menu row; screen:\n{}",
            h.driver.screen()
        );

        let (hx2, hy2) = h
            .driver
            .find(hamburger)
            .expect("hamburger icon must still paint with the menu bar open");
        assert_ne!(
            (hx1, hy1),
            (hx2, hy2),
            "sanity: revealing the menu bar must shift the hamburger's own \
             row, or this test isn't exercising the row-shift this issue is \
             about"
        );

        // Deliberately the SAME `(hx1, hy1)` as the first click.
        h.driver.click(hx1, hy1);
        h.driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        h.driver.render();
        assert!(
            !h.driver.screen_contains("File"),
            "second click at the SAME physical position the first click \
             used must hide the menu row again (#988); screen:\n{}",
            h.driver.screen()
        );
        assert!(
            !h.driver.screen_contains("New Tab"),
            "the stale second click must not open the File dropdown \
             (the #988 symptom); screen:\n{}",
            h.driver.screen()
        );
    }
}

// The Command Palette's "File: New Tab" row advertises `Ctrl+T` as its shortcut
// hint, but the chord's real, live binding is `panel_keys.open_terminal` (it opens
// the terminal panel, never creates a second tab — exactly the mismatch vimcode#1789
// reports). The bug is the palette's own stale label, not the chord's behaviour.
// This scenario pins the label.
#[cfg(test)]
mod issue_1789_command_palette_new_tab_shortcut_lies {
    use super::*;

    fn engine_vscode_fixture() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        engine
    }

    /// The Command Palette's own displayed shortcut hint for "File: New
    /// Tab" must not claim `Ctrl+T` — that chord is live-bound to the
    /// integrated terminal toggle (`panel_keys.open_terminal`), never to
    /// `tabnew`.
    ///
    /// RED against unfixed `develop`: `PALETTE_COMMANDS`'s `"File: New
    /// Tab"` entry hard-codes `shortcut: "Ctrl+T"` with an empty
    /// `vscode_shortcut` (falls back to `shortcut`), so the palette row
    /// paints "Ctrl+T" right next to "File: New Tab" in every mode.
    crate::backend_conformance! {
        label: command_palette_new_tab_shortcut_matches_live_binding,
        backends: [gtk, tui, tui_prod],
        engine: engine_vscode_fixture(),
        size: (800, 480),
        body: |driver| {
            // Opened via F1 (`PALETTE_COMMANDS`'s own "View: Command
            // Palette" entry, `vscode_shortcut: "F1"`, handled directly in
            // both modes by `Engine::handle_key`/`vscode.rs`) rather than
            // `:CommandPalette` — VSCode mode has no Normal-mode ':'
            // gesture, so typing ':' there inserts a literal colon into
            // the buffer instead of opening the command line.
            driver.press_named(NamedKey::F(1));
            driver.type_text("New Tab");
            // "File:" alone, not the full "File: New Tab" phrase — TUI
            // paints each word of a palette row as its own text run with
            // no joining space (`command_palette_filters_and_escape_
            // dismisses`'s own doc spells this out), so a needle spanning
            // the "New"/"Tab" word boundary never matches there even
            // though the phrase is plainly on screen.
            assert!(
                screen_row_has(driver, "File:"),
                "typing 'New Tab' must keep the 'File: New Tab' entry visible"
            );
            assert!(
                !screen_row_has(driver, "Ctrl+T"),
                "#1789: the palette must not advertise Ctrl+T as 'File: \
                 New Tab's shortcut — that chord is live-bound to the \
                 integrated terminal toggle (panel_keys.open_terminal), \
                 never to tabnew"
            );
        },
    }
}

/// The scroll-routing cascade used to be a TUI-private hand-rolled duplicate of
/// the GTK-only equivalent, both now replaced by the *one* shared
/// `App::handle_dispatch`'s `UiEvent::Scroll` arm in `render.rs`, which resolves
/// the target surface once via `quadraui::dispatch_scroll` against
/// `Engine::scroll_surfaces` (registered by the shared `render.rs` paint path, not
/// per backend) before falling back to the generic active/hovered-window viewport
/// scroll. This module is the `::tui_prod` proof that it behaves identically (both
/// direction and magnitude) on every backend.
///
/// #554's polarity convention: `UiEvent::Scroll.delta.y` is quadraui's convention
/// (positive y = up, toward the top of the content) — the opposite of GTK's raw
/// `GdkEventScroll` polarity (positive dy = wheel down). `App::handle_dispatch`
/// negates `delta.y` back to GTK-raw once, at the single shared call site, so
/// every surface-specific consumer downstream is written against GTK-raw polarity.
/// `ConformanceDriver::scroll_at(needle, lines)` dispatches in quadraui's
/// convention directly, so a *negative* `lines` here is a wheel-down notch.
///
/// Magnitude is asserted relatively (farther scroll ⇒ a strictly larger topmost
/// visible line number), not against a hardcoded row count — `scroll_at`'s `lines`
/// are scaled by each backend's own `conformance_line_height()`, so one notch moves
/// the GTK viewport roughly 20x as far as one TUI "line", but both are converted
/// through the identical `handle_mouse_scroll_msg` notch-to-row formula.
#[cfg(test)]
mod issue_1266_scroll_router_convergence {
    use quadraui::testing::ConformanceDriver;

    /// A long, single-token-per-line buffer (`L0000`, `L0001`, …) — single
    /// tokens rather than `"line 0"`-style phrases because `TuiDriver`'s
    /// `inventory()` splits painted text into runs at every space
    /// character (see its own doc), so a multi-word phrase can straddle
    /// two runs on TUI while GTK paints it as one Pango run. A single
    /// token is exactly one run on every backend, so `screen_has` reads
    /// identically everywhere.
    fn engine_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        let text: String = (0..500).map(|i| format!("L{i:04}\n")).collect();
        engine.buffer_mut().insert(0, &text);
        engine
    }

    /// The smallest `L%04d` line number currently painted anywhere on
    /// screen — a backend-neutral, rendered-output measure of "how far
    /// down the viewport has scrolled". Reads `FrameInventory` text runs
    /// (the same inventory `screen_has`/`screen_row_has` read), never
    /// engine state.
    fn topmost_visible_line<D: ConformanceDriver>(driver: &D) -> u32 {
        driver
            .inventory()
            .text_runs()
            .iter()
            .filter_map(|run| run.text.strip_prefix('L'))
            // GTK paints a text-display run as the raw buffer slice,
            // trailing `\n` included; TUI's cell grid has no such
            // character. `take_while(ascii_digit)` reads "the digits at
            // the front" on both, ignoring whichever backend's trailing
            // byte this run happens to carry.
            .map(|digits| {
                digits
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .filter_map(|digits| digits.parse::<u32>().ok())
            .min()
            .expect(
                "at least one L%04d marker line must be painted — the \
                 fixture's 500-line buffer should always leave some in \
                 view",
            )
    }

    crate::backend_conformance! {
        label: wheel_scroll_direction_and_magnitude,
        backends: [gtk, tui, tui_prod],
        engine: engine_fixture(),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("L0000"),
                "precondition: a fresh buffer must open scrolled to the top"
            );

            // Direction: a wheel-down notch (negative `lines`, quadraui's convention) must
            // move the viewport DOWN — later lines come into view, `L0000` scrolls out.
            // Each step re-locates its needle off the *previous* step's own topmost_visible_line
            // reading rather than reusing `"L0000"` — once the viewport has scrolled, `L0000`
            // itself is no longer painted, and `scroll_at` panics if its needle isn't on screen.
            driver.scroll_at("L0000", -2);
            let after_small_down = topmost_visible_line(driver);
            assert!(
                after_small_down > 0,
                "wheel down must move the viewport down (topmost visible \
                 line 0 -> >0), got {after_small_down} — direction is \
                 inverted (#554)"
            );

            // Magnitude: a bigger notch count must move the viewport
            // farther, not just further-than-zero — a consumer that
            // clamps every nonzero delta to the same single-row step
            // would pass the direction check above but fail this one.
            driver.scroll_at(&format!("L{after_small_down:04}"), -10);
            let after_bigger_down = topmost_visible_line(driver);
            assert!(
                after_bigger_down > after_small_down,
                "a larger wheel-down notch (-10 vs -2) must scroll \
                 farther (topmost visible line {after_small_down} -> \
                 strictly more), got {after_bigger_down} — magnitude is \
                 not respected"
            );

            // Direction, the other way: a wheel-up notch (positive
            // `lines`) must walk the viewport back up — this is the half
            // that catches a consumer which ignores the sign entirely
            // and always scrolls the same direction regardless of it.
            driver.scroll_at(&format!("L{after_bigger_down:04}"), 3);
            let after_up = topmost_visible_line(driver);
            assert!(
                after_up < after_bigger_down,
                "wheel up must move the viewport back up ({after_bigger_down} \
                 -> {after_up})"
            );
        },
    }

    /// A second surface in the same shared cascade, chosen for being fully
    /// deterministic without a real git repo or a live PTY (unlike the
    /// sidebar's git/search panels or the terminal's scrollback, both of
    /// which depend on ambient process state this suite doesn't control) —
    /// the debug-output bottom panel's content is seeded directly onto
    /// `Engine::dap_output_lines`.
    ///
    /// This is the `"debug_output"` arm of `handle_mouse_scroll_msg`'s
    /// `quadraui::dispatch_scroll` match (`app.rs`) — the exact other
    /// branch, besides the generic window-viewport fallback the scenario
    /// above exercises, that the old per-backend `mouse.rs` cascade used
    /// to hand-roll a second time (`"debug_output" =>
    /// engine.handle_debug_output_scroll(delta.y)`, identical body, see
    /// this module's git-blame history). Proving *this* arm converges too
    /// is what makes "one shared router" true of the cascade as a whole,
    /// not just of the one arm the first scenario happens to hit.
    ///
    /// Direction here is `auto_scroll` (pinned-to-tail vs not), not a line
    /// number — `Engine::handle_debug_output_scroll`'s `delta_y > 0.0`
    /// branch only *sets* `auto_scroll = true` (never clears it), so
    /// scrolling down from an already-pinned view is a no-op by design;
    /// the observable, direction-sensitive half is scrolling *up* (which
    /// unconditionally clears `auto_scroll`, unpinning the view) and then
    /// *back down enough rows* to re-pin it — both direction and magnitude
    /// have to be right for that round trip to land exactly on repinning.
    mod issue_1266_debug_output_scroll_router_convergence {
        use quadraui::testing::ConformanceDriver;

        /// 200 single-token lines so each one is exactly one painted text
        /// run on every backend — same rationale as the editor scenario's
        /// `engine_fixture`.
        fn engine_fixture() -> crate::core::Engine {
            let mut engine = crate::core::Engine::new_for_test();
            engine.settings.use_nerd_fonts = Some(false);
            engine.bottom_panel_open = true;
            engine.bottom_panel_kind = crate::core::engine::BottomPanelKind::DebugOutput;
            engine.dap_output_lines = (0..200).map(|i| format!("D{i:04}")).collect();
            engine
        }

        crate::backend_conformance! {
            label: debug_output_wheel_scroll_direction_and_magnitude,
            backends: [gtk, tui, tui_prod],
            engine: engine_fixture(),
            size: (800, 480),
            body: |driver| {
                assert!(
                    !driver.exited(),
                    "precondition: the app must still be running"
                );
                assert!(
                    driver.screen_has("D0199"),
                    "precondition: a fresh `auto_scroll: true` panel must open \
                     pinned to the tail (D0199, the last seeded line)"
                );

                // Wheel up (positive `lines`, quadraui's convention) over the
                // panel: must unpin it from the tail. A pinned-to-tail panel
                // keeps painting the same last lines no matter how far
                // `debug_output_scroll` moves internally, so "the tail is
                // still visible" would pass even with the router firing the
                // wrong arm (or no arm at all) — checking the now-*visible*
                // top-of-buffer line is the rendered-output signal that the
                // view really moved.
                driver.scroll_at("D0199", 30);
                assert!(
                    driver.screen_has("D0000"),
                    "a big-enough wheel-up notch over the debug-output panel \
                     must unpin it from the tail and scroll far enough to \
                     bring the first seeded line (D0000) into view — the \
                     view is still pinned, or the router's \"debug_output\" \
                     arm never fired (#1266)"
                );

                // Magnitude, the other direction: scroll back down by much
                // less than a full re-pin's worth and the tail must still be
                // absent — a consumer that always jumps straight back to the
                // tail regardless of the notch size would pass the direction
                // check above but fail this one.
                driver.scroll_at("D0000", -2);
                assert!(
                    !driver.screen_has("D0199"),
                    "a small wheel-down notch must not snap all the way back \
                     to the tail — magnitude is not respected"
                );
            },
        }
    }

    /// A third surface: the sidebar. Chosen because the Git panel's
    /// `build_source_control_data` builds straight off `Engine::sc_file_statuses`,
    /// with no dependency on this process's real `cwd` being a git repository —
    /// deterministic without a fixture beyond the engine itself.
    ///
    /// Unlike the editor/debug-output scenarios above, the sidebar's wheel path never
    /// reaches `handle_mouse_scroll_msg`'s `dispatch_scroll` at all:
    /// `try_route_sidebar_mouse_event` claims every `UiEvent::Scroll` whose position
    /// falls inside the painted sidebar bounds *earlier* in the same `handle_dispatch`,
    /// and hands it to `Engine::handle_sc_sidebar_ui_event`. This scenario is the
    /// proof that the convergence covers it too, not just the `scroll_surfaces`
    /// registered arms.
    ///
    /// Also unlike the other two scenarios, quadraui's own `SidebarSystem::handle_inner`
    /// scrolls by a **fixed one row per event, regardless of the delta's magnitude**.
    /// So magnitude for *this* surface is tested by dispatching a wheel event more than
    /// once, not by one bigger-delta event — a single `scroll_at` call moves exactly
    /// one row.
    ///
    /// Load-bearing for #554's convention: `try_route_sidebar_mouse_event` is called
    /// *before* the `UiEvent::Scroll` match arm that negates `delta.y` back to GTK-raw
    /// — so unlike the three `handle_mouse_scroll_msg` consumers (all written against
    /// GTK-raw polarity), `SidebarSystem` receives the **un-negated, quadraui-native**
    /// delta straight off the `UiEvent`, matching `ScrollDelta`'s own documented
    /// convention directly.
    mod issue_1266_sidebar_scroll_router_convergence {
        use quadraui::testing::ConformanceDriver;

        /// 80 synthetic unstaged files so the panel body overflows one
        /// screen — single-token paths (`gitf0000` .. `gitf0079`, no `/`
        /// or extra punctuation) for the same reason `engine_fixture`'s
        /// `L%04d` lines are single tokens in the sibling scenario above.
        fn engine_fixture() -> crate::core::Engine {
            let mut engine = crate::core::Engine::new_for_test();
            engine.settings.use_nerd_fonts = Some(false);
            engine.app_shell.show_panel(&quadraui::WidgetId::new(
                crate::core::engine::sidebar::PANEL_GIT,
            ));
            engine.sc_file_statuses = (0..80)
                .map(|i| crate::core::git::FileStatus {
                    path: format!("gitf{i:04}"),
                    unstaged: Some(crate::core::git::StatusKind::Modified),
                    ..Default::default()
                })
                .collect();
            engine
        }

        /// The seeded row whose painted y is closest to `anchor_y`, as
        /// `(index, trimmed text)`.
        ///
        /// Every seeded row stays present in `inventory()` even once it
        /// has scrolled off the visible band — this harness's text-run
        /// recorder captures paint calls ahead of whatever clip region a
        /// real display would apply, so "is a given row still reported at
        /// all" can't distinguish a scrolled-away row from one that never
        /// scrolled, and a fixed needle (e.g. always re-scrolling at
        /// `"gitf0000"`) eventually dispatches the wheel event at a
        /// `position` outside the painted sidebar bounds once that row
        /// scrolls out of it — `try_route_sidebar_mouse_event` requires
        /// `position` to land inside `sidebar_content_bounds` to route the
        /// event at all, so that reads as a silent, misleading "scroll
        /// stopped working" rather than the test-anchoring bug it is.
        ///
        /// Anchoring on "whichever row now sits near this fixed *screen*
        /// position" instead mirrors what a real mouse wheel does — the
        /// pointer doesn't move, the content slides under it — and keeps
        /// every dispatch inside the panel. The row's parsed index is a
        /// direct, monotonic proxy for scroll position, since the fixture
        /// seeds `gitf%04d` in ascending order. Panel rows render with a
        /// one-column status-marker gutter ahead of the filename, so each
        /// row is its own run with a leading space (`" gitf0000"`) rather
        /// than starting with `"gitf"` — hence `trim()` before parsing.
        fn nearest_row<D: ConformanceDriver>(driver: &D, anchor_y: f32) -> (u32, String) {
            driver
                .inventory()
                .text_runs()
                .iter()
                .filter_map(|run| {
                    let trimmed = run.text.trim();
                    trimmed
                        .strip_prefix("gitf")
                        .and_then(|digits| digits.parse::<u32>().ok())
                        .map(|idx| (idx, trimmed.to_string(), (run.bounds.y - anchor_y).abs()))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(idx, text, _)| (idx, text))
                .expect(
                    "at least one gitf%04d row must be painted somewhere — \
                     the fixture seeded 80 of them",
                )
        }

        /// The shared scenario body, generic over the driver — see the
        /// three `#[test]` functions below for why this isn't a
        /// `crate::backend_conformance!` invocation like its siblings
        /// above: that macro's single `size: (w, h)` is handed to every
        /// backend arm as both GTK's pixels *and* TUI's cells, which this
        /// scenario can't tolerate — a 480-*cell*-tall terminal (fine as
        /// 480 *pixels* for GTK) pushes the git panel's layout far enough
        /// past realistic terminal geometry that `try_route_sidebar_mouse_
        /// event`'s bounds check and this scenario's own wheel-position
        /// anchoring stopped agreeing in practice (confirmed empirically:
        /// identical logic, identical dispatch, real row movement on GTK,
        /// zero movement on TUI at that size; a realistic terminal size
        /// fixed it). Each backend below sizes its own harness instead.
        fn run_scenario<D: ConformanceDriver>(driver: &mut D) {
            // The screen point every wheel event in this scenario
            // dispatches at: `gitf0000`'s initial painted position — the
            // panel's top content row, which stays well inside
            // `sidebar_content_bounds` for every step below.
            let anchor_y = driver
                .inventory()
                .text_runs()
                .iter()
                .find(|run| run.text.trim() == "gitf0000")
                .map(|run| run.bounds.y)
                .expect(
                    "precondition: the git panel must open scrolled to the \
                     first (alphabetically-first) file, gitf0000",
                );

            // Direction: one wheel-down notch must advance which row sits
            // at the anchor screen point.
            let (idx0, needle0) = (0u32, "gitf0000".to_string());
            driver.scroll_at(&needle0, -1);
            let (idx1, needle1) = nearest_row(driver, anchor_y);
            assert!(
                idx1 > idx0,
                "one wheel-down notch over the git sidebar must bring a \
                 later row to this screen point (row index {idx0} -> \
                 greater), got {idx1} — direction is inverted, or the \
                 event never reached the sidebar's shared routing call \
                 site at all"
            );

            // Magnitude: four more notches (five total) must advance it
            // further still — SidebarSystem's own `UiEvent::Scroll` arm
            // moves exactly one row per *event* (ignoring delta magnitude,
            // see this module's doc), so magnitude here means "more events
            // moved it further", not "one bigger event moved it further".
            let mut needle = needle1;
            for _ in 0..4 {
                driver.scroll_at(&needle, -1);
                needle = nearest_row(driver, anchor_y).1;
            }
            let (idx5, needle5) = nearest_row(driver, anchor_y);
            assert!(
                idx5 > idx1,
                "five wheel-down notches must advance the row at this \
                 screen point further than one notch did (idx1={idx1}, \
                 idx5={idx5}) — repeat wheel events are being dropped or \
                 clamped"
            );

            // Direction, the other way: a wheel-up notch must bring an
            // earlier row back to the anchor point.
            driver.scroll_at(&needle5, 1);
            let (idx_up, _) = nearest_row(driver, anchor_y);
            assert!(
                idx_up < idx5,
                "wheel up over the git sidebar must bring an earlier row \
                 back to this screen point ({idx5} -> less), got {idx_up}"
            );
        }

        #[test]
        #[cfg(feature = "gui")]
        fn sidebar_wheel_scroll_direction_and_magnitude_gtk() {
            let mut h = crate::gtk::testing::conformance_harness(engine_fixture(), 800, 480);
            run_scenario(&mut h.driver);
        }

        #[test]
        fn sidebar_wheel_scroll_direction_and_magnitude_tui() {
            let mut h = crate::tui_main::testing::conformance_harness(engine_fixture(), 120, 40);
            run_scenario(&mut h.driver);
        }

        #[test]
        fn sidebar_wheel_scroll_direction_and_magnitude_tui_prod() {
            let mut h =
                crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 120, 40);
            run_scenario(&mut h.driver);
        }
    }
}
