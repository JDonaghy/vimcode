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
//! `crate::macos`'s own test module, and `crate::win`'s
//! (`src/win/mod.rs::win_driver_tests`). That split is exactly what the
//! Platform-Neutrality Rule asks for: no decision lives in a backend
//! directory, only the unavoidable "which driver type" plumbing.
//!
//! `crate::win`'s arm used to be Windows-only in a stronger sense than
//! `crate::macos`'s: not just "only actually runs on real hardware" (true of
//! every native-GUI arm here, `macos` included — none of these headless
//! drivers has a CI runner in this fleet) but "does not even *type-check*
//! off Windows", because quadraui's own `win::testing` module was gated on
//! `target_os = "windows"` rather than stubbed per WinAPI call the way
//! `win::backend`/`run`/`shell_runner` are. quadraui#1038 fixed that — the
//! pinned rev's `win::testing` now compiles under `feature = "win"` alone —
//! so `crate::win::win_driver_tests`'s `ConformanceHarness<WinDriver<...>>`
//! instantiation is exactly as "neutral" as `crate::macos`'s now: verified
//! on every host via `cargo check --tests`, only *run* on real Windows.
//! That was the last of the four backends (gtk, tui, macos, win) for which
//! this crate's own `ConformanceHarness` plumbing didn't type-check off its
//! native platform — closing vimcode#928's acceptance criterion #2.
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
//!
//! # Which trait bound a scenario needs (#982)
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
//! # `tui` vs `tui_prod` — which TUI shell a scenario actually drives (#1043)
//!
//! "TUI" above names one *bound*, but [`backend_conformance!`] wires it to
//! **two different shells**, each its own arm:
//!
//! - `tui` → [`crate::tui_main::testing::conformance_harness`], wrapping
//!   [`App`] (the cross-backend-shared shell every other arm also wraps) on
//!   `quadraui::tui::TuiBackend`. This is the *control*: a scenario failing
//!   only here means the two rasterisers disagree, nothing about the TUI
//!   binary users actually run.
//! - `tui_prod` → [`crate::tui_main::testing::conformance_harness_prod`],
//!   wrapping [`crate::tui_main::testing::TuiShellApp`] — the independently
//!   hand-written shell `tui_main::run` really ships (its own mouse
//!   routing, its own render path). A scenario green on `gtk`+`tui` but red
//!   on `tui_prod` is, by construction, the shipped TUI diverging from the
//!   shared shell — exactly the class of bug #1025 was, caught mechanically
//!   here instead of by a user.
//!
//! Before #1043 only `tui` existed, so nothing in this file could ever see
//! the second kind of divergence. `tui_prod` cannot yet accept every
//! scenario — see [`crate::tui_main::testing::conformance_harness_prod`]'s
//! own doc for the `ConformanceHarness::engine`/`::screen_layout` gap that
//! currently excludes the #987 scrollbar-drag family and #983's
//! `_resetting` sweep from it.

#![cfg(any(test, feature = "test-support"))]

/// #1090: the shared plugin-panel fixture and the six painted-geometry
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
    /// The `App`'s own painted `render::ScreenLayout` cache (#987), or
    /// `None` if the harness was built via [`Self::new`] rather than
    /// [`Self::new_with_screen_layout`] (every existing caller predating
    /// #987 — the field defaults to empty for them rather than making this
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

    /// [`Self::new`], plus a live [`Self::screen_layout`] handle (#987) —
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
/// `units` (#1426) is the one place this shared builder is *not*
/// backend-neutral: the caller already knows which `quadraui::Backend` it's
/// wrapping (`GtkBackend`, `MacBackend`, `TuiBackend`, ...), so it passes the
/// matching [`crate::render::UnitProfile`] — `px()` from
/// `crate::gtk::testing::conformance_harness`/`src/macos/mod.rs`/
/// `src/win/mod.rs`, `cell()` from
/// `crate::tui_main::testing::conformance_harness`.
pub(crate) fn build_app_and_config(
    engine: Rc<RefCell<Engine>>,
    backend: Rc<RefCell<Box<dyn TextMetricsBackend>>>,
    units: crate::render::UnitProfile,
) -> (App, quadraui::ShellConfig) {
    let app = App::new_headless_with_backend(engine, backend, units);
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
// ── #991: a real merge-conflict fixture repo ────────────────────────────

/// Name of the one conflicted file [`make_conflicted_repo`] leaves behind.
/// Short on purpose — the SC sidebar is 30 cells wide on TUI, and a row
/// that gets truncated is a row a `find_bounds`/`screen_has` assertion
/// can't see.
pub const CONFLICT_FIXTURE_FILE: &str = "zqxw991conf.txt";

/// Build a throwaway git repo in a temp dir with **one real merge
/// conflict** (`UU` — both modified, the common case), created with plain
/// `git` exactly as a user would hit it: two branches editing the same
/// line, then a failing `git merge` (#991).
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

/// Like [`sweep_hit_band_integrity`], for a probe whose click is not
/// self-restoring (#971).
///
/// [`sweep_hit_band_integrity`]'s click-then-click-to-restore pattern
/// depends on the *second* click undoing whatever the first one did — true
/// for a directory row's expand/collapse flip, and for a `SidebarSystem`
/// section header's identical flip (#971's source-control and ext-panel
/// sweeps). It is **not** true for the unified picker's row click
/// (`render::apply_picker_row_click`): the first click on a not-yet-selected
/// row only selects it, but the second click at that same point now lands on
/// an *already*-selected row, which confirms it (`Engine::picker_confirm`)
/// and closes the popup outright — unless the row is `expandable`, the one
/// documented escape hatch, and no `PickerItem` in this codebase ever sets
/// `expandable: true` (see `Engine::build_symbol_tree_items`'s own #262
/// comment), so that branch is unreachable in practice. Reusing
/// `sweep_hit_band_integrity` here would close the popup after sample 0's
/// restore click, leaving every later sample clicking dead space behind a
/// dismissed modal.
///
/// `setup` rebuilds a fresh, comparable starting state before **every**
/// sample (not just once) instead of relying on a self-cancelling second
/// click — e.g. re-opening the picker with nothing yet selected on the
/// target row. Unlike [`sweep_hit_band_integrity`], `needle`'s bounds are
/// re-located after every `setup()` call rather than once up front — a
/// modal popup that gets torn down and rebuilt this many times is not
/// guaranteed to repaint at the exact same pixel position each time (a
/// picker popup was observed centering itself a few pixels differently
/// after its first couple of opens in this repo's own #971 development —
/// unrelated to hit-testing, but enough to make a once-only bounds lookup
/// flaky), and re-locating costs nothing `setup` was not already going to
/// pay for with its own repaint.
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

/// Which side of a row's own text glyph a probe point sits on, within
/// that row's **painted background band**.
///
/// Every panel in this repo paints a row taller than the text glyph it
/// centres inside it (GTK: a 32px band around a 23px label), so each row
/// owns a strip of background *above* its glyph and another *below* it.
/// Fixing one edge and not the other only moves a hit-band bug, so #1028
/// probes both — see
/// [`row_click_in_its_painted_band_hits_its_own_row`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowBandEdge {
    /// The topmost pixel of the band, above the row's own text glyph.
    AboveGlyph,
    /// The bottommost pixel of the band, below the row's own text glyph.
    BelowGlyph,
}

/// Measure one painted row's background band by walking a single column
/// of **real painted pixels** outward from `seed_y` while the colour is
/// unchanged, returning the half-open band `[top, bottom)`.
///
/// This exists because `FrameInventory::text_runs()` only exposes *glyph*
/// bounds, and a row's glyph is strictly smaller than the band the panel
/// paints for it — so glyph bounds cannot answer "which row does this
/// point belong to". #983's original reproduction derived its probe point
/// from glyph bounds alone and, as a direct result, aimed *past* the
/// clicked row's band and into the next row's own band (see
/// [`row_click_in_its_painted_band_hits_its_own_row`]'s doc for the
/// measurements). Reading the band off the pixels the frame actually
/// painted is the only way to state "inside this row" without asserting a
/// hardcoded coordinate — `CLAUDE.md`'s Testing rule 1.
///
/// `pixel(y)` must sample a column that is background at `seed_y` — i.e.
/// clear of the row's own glyph, its chevron and any scrollbar gutter.
/// Panics if the band cannot be resolved (the whole `0..limit` column is
/// one colour), rather than silently returning a band that proves nothing.
///
/// The row being measured must also be painted in a colour its immediate
/// neighbours do *not* share — a section header (`theme.header_bg`) or the
/// focused row (`theme.selected_bg`) — or the walk runs straight through
/// the boundary into the next identically-filled row. Callers get that
/// checked for them: `row_click_in_its_painted_band_hits_its_own_row`
/// asserts the returned band actually contains the needle's own glyph, so
/// a band measured off the wrong row fails loudly there instead of
/// silently probing a neighbour's padding.
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

/// #983 / #1028: a click anywhere inside `needle`'s own **painted
/// background band** — the full vertical slot the panel fills for that
/// row, on either side of its text glyph — must act on `needle`'s own
/// row, never on the row painted above or below it.
///
/// `band` is `needle`'s painted band as measured by
/// [`painted_row_band`] from the pixels of the current frame; `edge`
/// picks which extreme of it to probe. Both edges are exercised by
/// #1028's callers: fixing only the bottom edge would just move the bug
/// to the top one.
///
/// # What #983 actually turned out to be (measured, #1028)
///
/// The v0.11.0 report ("settings / git insights row click selects the
/// row below") was originally reproduced here by probing
/// `next_row_needle`'s **glyph** top minus half a pixel, on the theory
/// that a GTK-only gap between a row's text-glyph height and the panel's
/// row pitch let a click inside a row's own band resolve to the next
/// row. Measuring the frame instead of the glyphs shows that is not what
/// is happening. On a 1400x900 GTK frame, driving the two reported
/// panels through their real click paths:
///
/// | panel | row | painted band (bg pixels) | hit band (click sweep) |
/// |---|---|---|---|
/// | Settings (`FormController`) | `▼ LSP` header | `[389, 421)` | `[389, 421)` |
/// | Ext panel (`SidebarSystem`) | `AVAILABLE` header | `[741, 773)` | `[741, 773)` |
///
/// The hit band matches the painted band **to the pixel** on both
/// panels, and both bands are painted in a *visibly different* colour
/// from their neighbours (`theme.header_bg` vs `theme.tab_bar_bg` —
/// `51,51,76` vs `38,38,51` on the default theme), so the boundary is
/// drawn, not invisible. The original probe point was not inside the
/// clicked row's band at all: it sat 4px (settings) / 7.5px (ext panel)
/// *below* the next row's band top edge, inside that next row's own
/// painted background. So neither vimcode's
/// `render::handle_settings_form_ui_event` nor quadraui's
/// `FormController::click_inner` / `SidebarSystem` row resolution has a
/// geometry bug to fix — no quadraui issue was filed, because there is
/// no upstream gap to file.
///
/// What this helper guards now is the property the report *meant*: the
/// row a click lands on is the row whose painted band contains the
/// point, everywhere in that band including the padding strips above and
/// below the glyph — which is a strictly stronger claim than the single
/// glyph-derived point the original probed, and the claim a paint/hit
/// drift (#967's family) would actually break.
///
/// `effect_after_click` reads back, from **painted** output only (never
/// engine state — `CLAUDE.md`'s "assert on rendered output" rule,
/// #587/#592), whether the click acted on `needle`'s own row: `true`
/// only when the correct-row outcome is observed.
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

/// #1360: clicking a panel's activity-bar icon must move keyboard focus
/// into that panel, on every backend — the exact repro this issue reports:
/// with a multi-line buffer, clicking the Search icon then typing `jj` on
/// unfixed GTK left focus on the editor (`jj` moved the cursor down two
/// lines, `Ln 1` → `Ln 3`), while TUI already routed `jj` into the search
/// box.
///
/// Locates the Search activity-bar icon via
/// `FrameInventory::zones()` — [`crate::core::engine::sidebar::PANEL_SEARCH`]
/// is literally the `"panel:search"` `WidgetId`
/// `compose::app_shell::AppShell::render`'s `register_chrome_zones` keys
/// every activity-bar item's zone by (see that struct's own doc) — rather
/// than `click_text`, since the icon's fallback glyph (`crate::icons::
/// SEARCH`'s ASCII fallback is `"/"`) collides with ordinary path
/// separators the explorer tree paints for any repo with subdirectories.
///
/// Asserts on the painted global status bar's "Ln N, Col N" cursor-position
/// segment (`render::build_status_line`, painted whenever `'ruler'` is on —
/// the engine default) rather than reading `engine.search_has_focus`
/// directly: this is the exact rendered signal a user watches, and it is
/// what the issue's own repro named ("Ln 1 → Ln 3"). Painting-only
/// assertions are the CLAUDE.md rule #587/#592 exist to enforce — reading
/// the flag instead would pass even if the click moved the *shadow*
/// `engine.app_shell`'s belief but some other bug left the real key
/// dispatch routed to the editor regardless.
///
/// Two Escapes, not one, are needed to fully return focus to the editor:
/// `Engine::dispatch_search_sidebar_key_unified`'s query-input Escape arm
/// only demotes the query field to the panel's own results list (a second,
/// in-panel focus move), matching the identical two-step return this
/// scenario's `tui_prod` arm already has today — the first Escape must
/// still block the following motion key from reaching the editor, or this
/// scenario would not be exercising the real contract.
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

/// #1419 (closes #406): `Ctrl-W` from a focused sidebar panel arms a
/// one-keystroke chord; `l`/`Right` moves keyboard focus to the editor.
/// `render::route_sidebar_chord_key` is the shared rung both backends now
/// call; before this issue GTK had no per-keypress chord latch to hang the
/// `Ctrl-W` arm on at all (TUI's own, now-deleted `TuiSidebar::
/// pending_ctrl_w` was the only place it lived), so `Ctrl-W l` silently did
/// nothing on GTK.
///
/// A motion key typed right after must move the cursor (`Ln 1,` ->
/// `Ln 2,`), which only happens once the editor holds keyboard focus —
/// painting-only per CLAUDE.md's #587/#592 rule (never
/// `engine.search_has_focus` directly). Before this fix on GTK, the panel
/// still held focus, so the following `j` stayed on `Ln 1,`.
///
/// See [`sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar`] for the
/// `h`/`Left` half and why it is a *separate* scenario (own fresh driver)
/// rather than a second phase reusing this one's.
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

/// #1419 (closes #406): the `h`/`Left` half of the `Ctrl-W` sidebar chord —
/// see [`sidebar_ctrl_w_l_returns_focus_to_the_editor`]'s doc for the shared
/// background. `j` then `l` (`ActivityBarKeyAction::MoveDown` then
/// `::Activate`) only move the toolbar's own selection cursor and activate
/// whatever it lands on when the activity bar toolbar itself holds keyboard
/// focus; a still-panel-focused or still-editor-focused `j`/`l` does
/// something else entirely (panel-local navigation / a plain editor motion +
/// line-join respectively), never switches the sidebar to a different
/// panel. So Search -> `Ctrl-W h` (arm+toolbar-focus) -> `j` (select the
/// next toolbar item, Debug) -> `l` (activate it) landing on the Debug
/// panel's own painted content (case-insensitive `"debug"` — GTK's shared
/// `App` titles this panel "RUN AND DEBUG", `TuiShellApp`'s own independent
/// `shell_config` titles it "Debug"; `"debug"` is a substring of both)
/// proves the chord actually moved focus to the toolbar.
///
/// Deliberately not `q` (`ActivityBarKeyAction::Collapse`): that hits an
/// unrelated, pre-existing gap confirmed by hand while developing this
/// scenario — `App::handle_activity_bar_key`'s call site returns before
/// `Self::sync_runner_sidebar_visibility` ever runs, so on `gtk` the
/// *shadow* `engine.app_shell` correctly collapses but the *runner's* own
/// chrome (which alone decides whether `render_content`'s sidebar column
/// exists at all — see that method's own doc) keeps painting it open. That
/// gap is real but has nothing to do with this issue's `Ctrl-W` fix and
/// deserves its own issue, not a workaround folded in here. `j`/`l`
/// (`MoveDown`/`Activate`) only ever change *which panel's content* the
/// shadow's active-panel id points at — never sidebar *visibility* — so
/// they never touch that gap at all.
///
/// A fresh driver/single click, deliberately not a second phase appended to
/// [`sidebar_ctrl_w_l_returns_focus_to_the_editor`]: a *second* real
/// activity-bar mouse click in the same running app hits a second unrelated
/// quadraui `ShellAdapter` quirk (also confirmed by hand — after the first
/// click, `AppShell::handle`'s own activity-bar hit-test is never reached
/// again for a later click; `on_shell_event` fires exactly once). One click
/// per scenario, matching every other single-click scenario in this module
/// (`activity_bar_click_focuses_search_panel`, above), sidesteps it
/// entirely.
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
    assert!(
        screen_has_ci(driver, "debug"),
        "Ctrl-W h must move keyboard focus to the activity bar toolbar -- \
         'j' then 'l' from there must move the toolbar cursor onto Debug \
         (the next fixed panel after Search) and activate it, which a \
         still-panel-focused or still-editor-focused 'j'/'l' would not do"
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

/// #984: v0.11.0 bug report — a single click on the file explorer's
/// expand/collapse chevron does nothing; expanding the directory needs a
/// *second* click, while a single click anywhere on the same row's text
/// label expands it immediately. The two zones must not have different
/// click arities — this scenario encodes that parity requirement on one
/// row, not just "the chevron eventually works".
///
/// # Root cause (confirmed by reading, not guessed)
///
/// `quadraui::TreeController::click` (`compose/tree_controller.rs`)
/// resolves a chevron-zone hit to `TreeControllerEvent::RowToggleExpand`
/// — a different enum variant than the `RowSelected` a label-zone hit
/// (`TreeViewHit::Row`) produces. `Engine::handle_explorer_mouse_event`
/// (`src/core/engine/explorer_ops.rs`) special-cases `RowSelected`,
/// toggling the directory for it, but forwards every other variant —
/// `RowToggleExpand` included — to `Engine::dispatch_explorer_tree_event`,
/// whose `match` has no arm for `RowToggleExpand`: it falls through the
/// catch-all `_ => true`, a silent no-op that still reports the event as
/// consumed (so nothing downstream retries it). A *second* click at the
/// same point resolves through quadraui's `DoubleClickDetector` into
/// `RowActivated` instead, which **is** handled (both
/// `handle_explorer_mouse_event`'s own dispatch and
/// `dispatch_explorer_tree_event`'s `RowActivated` arm toggle a directory)
/// — hence "the chevron needs two clicks".
///
/// This routing lives entirely in `src/core/engine/explorer_ops.rs` —
/// shared engine code reached identically from GTK's and TUI's mouse
/// handlers via `render::route_explorer_tree_event` — not in either
/// backend's own hit-testing. So the bug, and this scenario, reproduce
/// identically on every backend wired through `ConformanceDriver +
/// DriverInput`: there is no backend-specific chevron geometry to fix,
/// per the Platform-Neutrality Rule.
///
/// # Locating the chevron without a hardcoded x
///
/// `▸` (`quadraui::TreeStyle::chevron_collapsed`'s default glyph) is
/// painted as its own standalone text run by both backends' tree
/// rasterisers — GTK's `draw_tree` calls `layout.set_text(chevron)` then
/// paints it alone, recorded verbatim by `painted_text::show_layout`;
/// TUI's `draw_tree` writes it between a leading indent space and a
/// trailing separator space, so `TuiDriver::inventory`'s
/// whitespace-delimited run scan also sees it as its own run. `dir_name`
/// must be the *only* collapsed branch row painted, so searching
/// `text_runs()` for the literal glyph unambiguously names this row's own
/// chevron — the same "locate via `inventory().text_runs()`, never a
/// literal coordinate" rule every other scenario in this module follows.
///
/// # Why the twin assertion collapses `dir_name` back down, not a second directory
///
/// After the chevron click expands `dir_name` (`child_name` becomes
/// painted), a single click on `dir_name`'s own label — the *same* row,
/// re-located from the frame the chevron click just repainted — must
/// collapse it back (`child_name` stops being painted). That is the
/// actual parity requirement #984 asks for: not "the chevron works" in
/// isolation, but "the chevron and the label need the same one click,
/// on the same row" — which also makes this resistant to a "fix" that
/// only stops being buggy by making the label need two clicks too (the
/// label assertion below would fail that just as loudly as the chevron
/// assertion catches today's bug).
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

    let chevron_bounds = locate(driver, "▸");
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

/// What [`drag_group_scrollbar_column`] observed, in terms a caller can
/// assert directly against #987's own two-halves report — never a raw
/// `view.scroll_top`/ratio read (CLAUDE.md's "assert on rendered output"
/// rule): [`Self::top_line_still_painted`] comes from
/// [`quadraui::testing::ConformanceDriver::screen_has`], and the two
/// `_resized` fields come from the same painted
/// [`crate::render::ScreenLayout`] rects every other scenario in this
/// module locates windows through.
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

/// #987: v0.11.0 bug report — "the scrollbar in the left editor tab group
/// doesn't work, and clicking it instead triggers a resize between tab
/// groups". Drags `target`'s own painted vertical-scrollbar column (its
/// window's own rightmost unit-wide slice, near the top of its track,
/// where `quadraui::Editor::layout`'s `v_scrollbar_bounds` always sits
/// regardless of backend — see this function's own doc for how that
/// column was confirmed, not assumed) downward, and reports what actually
/// happened so the caller can assert the two independent halves of the
/// report: did `target` scroll, and did the drag also resize the split
/// `target`/`other` share.
///
/// # Locating the scrollbar without a hardcoded coordinate
///
/// There is no `inventory().text_runs()` entry for a scrollbar thumb (it
/// paints no text), so this locates the *window* instead, via
/// [`window_rect`] against the harness's own painted
/// `ConformanceHarness::screen_layout` (#987 grew that field onto the
/// backend-neutral harness for exactly this — see its own doc). The click
/// point is then derived purely from that rect:
/// `x = target.rect.x + target.rect.width - 1.0` (one unit inside the
/// window's own right edge — the last column/pixel belonging to it, where
/// `quadraui::Editor::layout_with_options` always reserves exactly one
/// `cell_width`-wide `v_scrollbar_bounds` slice whenever
/// `total_lines > visible_lines`, on *both* backends — confirmed by
/// reading that shared quadraui primitive, not GTK/TUI-specific code) and
/// `y = target.rect.y + 1.0` down to `target.rect.y + height * 0.6` (well
/// within the track, and — since every caller here scrolls from the very
/// top with a large enough buffer that the thumb starts at the track's own
/// top — within the thumb itself for the first sample, exactly where a
/// user would actually grab it).
///
/// # Why a *vertical-only* drag, not a horizontal one
///
/// A real user reaching for a vertical scrollbar drags **down**, not
/// sideways — this reproduces that motion exactly (`x` never changes
/// between mouse-down and mouse-up). That the *group divider* still
/// resizes the split from a drag whose `x` never moves is not a mistake in
/// this helper: `render::divider_ratio_from_pos` reads whatever `x` the
/// press landed at (here, deliberately inside `target`'s own scrollbar
/// column, one unit off whatever the split's exact boundary is) and writes
/// that back as the new ratio on every subsequent move event, even one
/// that only ever restates the same `x` — so a vertical-only gesture that
/// starts inside the divider's grab band still perturbs the ratio by
/// however far off-center the press happened to land. That is precisely
/// the "click resizes" half of #987's report, reproduced faithfully rather
/// than avoided.
///
/// `top_line_needle` must be the exact text of `target`'s own first
/// visible line (e.g. its buffer's line 0) — unique enough that it cannot
/// also match `other`'s content (give the two windows distinct buffers, as
/// every caller here does), so [`ScrollbarDragOutcome::top_line_still_painted`]
/// unambiguously reads `target`'s own scroll position, not `other`'s.
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

/// #987 negative-space case: a drag on the group divider **itself** (not
/// near a scrollbar) must still resize the split — the check that catches
/// a "fix" which makes [`drag_group_scrollbar_column`]'s report pass by
/// deleting the divider's hit zone outright rather than by reordering hit
/// tests or adding the missing scrollbar one. Unlike
/// [`drag_group_scrollbar_column`], this drag moves `x` by a real amount
/// (`+20.0`) — a vertical-only drag exactly *on* the mathematical split
/// centre reproduces the same ratio it started from (confirmed while
/// developing this scenario: `divider_ratio_from_pos` at the exact centre
/// is a no-op), which would make this negative-space check pass for the
/// wrong reason.
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

/// #969: conformance assertion for [`TextMetricsBackend`]'s two load-bearing
/// setters — `set_current_line_height`/`set_current_char_width`. Both are
/// `&mut self` methods with no return value, so an empty ("stub") body
/// type-checks identically to a correct forwarding one; nothing short of
/// setting a value through the trait object and reading it back through the
/// `quadraui::Backend` getter it is supposed to feed
/// (`Backend::line_height`/`Backend::char_width`) can tell the two apart.
///
/// This is exactly the gap #967 fell into: `quadraui::macos::MacBackend`'s
/// `TextMetricsBackend` impl stubbed both setters (correct when #859 wrote
/// it — the inherent setters did not exist yet on that backend), quadraui#934
/// later added them, and the impl was never updated to forward to them. That
/// shipped for two days with clicks landing on the wrong explorer row before
/// #967 found and fixed it (see `src/macos/mod.rs`'s `TextMetricsBackend for
/// quadraui::macos::MacBackend` doc for the full mechanism). This function is
/// the seam that would have caught it on the commit that landed
/// quadraui#934's setters with no matching vimcode-side update: call it once
/// per concrete backend, in whatever driver-tier (or lighter) lane that
/// backend already has — see `crate::gtk` / `src/macos/mod.rs` /
/// `src/win/mod.rs` for the three call sites this issue adds.
///
/// The two probe values are distinctive and deliberately unlike any
/// backend's `::new()` default (`WinBackend::new()`'s is `16.0`/`8.0`; GTK's
/// and macOS's are effectively `0.0` until a real paint sets them), so a
/// stubbed setter that silently no-ops leaves the getter reporting its own
/// construction-time default instead of the probe value — which fails the
/// assertions below exactly the way #967's stub would have.
pub(crate) fn assert_text_metrics_backend_applies_metrics<B: TextMetricsBackend>(backend: &mut B) {
    const LINE_HEIGHT: f64 = 971.25;
    const CHAR_WIDTH: f64 = 483.5;

    backend.set_current_line_height(LINE_HEIGHT);
    backend.set_current_char_width(CHAR_WIDTH);

    assert_eq!(
        backend.line_height(),
        LINE_HEIGHT as f32,
        "TextMetricsBackend::set_current_line_height did not reach \
         Backend::line_height() — a stubbed setter silently disables the \
         #540/#819 click drift guard (#967); see #969"
    );
    assert_eq!(
        backend.char_width(),
        CHAR_WIDTH as f32,
        "TextMetricsBackend::set_current_char_width did not reach \
         Backend::char_width() — a stubbed setter silently disables the \
         #540/#819 click drift guard (#967); see #969"
    );
}

// ── The `KNOWN_BUGS` bidirectional gate (#982) ──────────────────────────
//
// Mirrors `tests/nvim_conformance.rs`'s `KNOWN_DEVIATIONS` idiom (#799) for
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
/// Labels of scenarios whose bodies are *expected* to panic today — see the
/// module-level section above. Each entry carries its issue number in a
/// trailing comment, same as `KNOWN_DEVIATIONS`.
///
/// Empty as of #982 (that issue shipped the mechanism and a self-test of
/// both gate directions, below, not any of the six real bug scenarios).
/// The remaining v0.11.0 bugs are separate issues chained `--after` that
/// one, each adding its own label here alongside its scenario.
///
/// #983 briefly held two entries (settings panel, ext-panel/"git
/// insights") and #1028 removed both: measuring the frame rather than the
/// glyph bounds showed the reported hit-band gap does not exist — the
/// painted row band and the click hit band match to the pixel on both
/// panels. See
/// [`row_click_in_its_painted_band_hits_its_own_row`]'s doc for the
/// measurements and for the (stronger) property those scenarios assert
/// now that they are ungated.
// #1117 removed the last live entry here (`::tui_prod`'s chevron-click
// scenario, filed by #1043 without a follow-up — see that issue and this
// one for the full history). Root cause was a genuine `tui_main`-only
// desync: `TuiShellApp::handle_mouse_event`'s `TreeController` intercept
// required `engine.app_shell.sidebar_visible()` — a shadow copy of sidebar
// visibility tracked independently of what the runner's own `AppShell`
// actually painted — in addition to `explorer_tree_rect.width > 0.0`, the
// one condition the shared dispatch `App::explorer_ui_event` (`app.rs`,
// what GTK and the `::tui`/`::gtk` harness arms both go through) checks.
// That shadow could go stale relative to what actually painted (e.g. a
// caller mutating `engine.session` on an already-built `Engine`, as
// `Engine::new_for_test`'s own doc warns against, which is exactly what
// this scenario's fixture did) and decline a click the shared dispatch
// would have claimed, dropping it through to the legacy `mouse::handle_mouse`
// path, which collapsed the sidebar instead. The fix converges the
// intercept onto the shared predicate — it no longer reads the shadow at
// all — and makes that predicate accurate by having `render_content` reset
// `explorer_tree_rect` to zero-width, once per frame, whenever the sidebar
// body doesn't paint (`presence.sidebar_panel == false`) — checked
// unconditionally, not from inside the `SidebarPanel` rung's own arm, since
// that rung is entirely absent from a hidden-sidebar frame's composed op
// list and so can never reset anything on exactly the frame that needs it.
// So a hidden sidebar can no longer leave a stale, non-zero rect behind for
// a later click to land in. Converging exposed one more gap the shadow
// check had been accidentally masking: `handle_mouse_event`'s panel
// intercepts (Explorer included) had no equivalent of GTK's
// `try_route_sidebar_mouse_event` early-exit on `engine.picker_open` /
// `handle_mouse_click_msg`'s on `self.folder_picker` — an outside click
// meant to dismiss an open picker could be swallowed by a panel intercept
// instead of ever reaching the picker's own dismiss routing. Folded into
// the same `intercepts_blocked` gate every panel intercept already shares.
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
pub(crate) const KNOWN_BUGS: &[&str] = &[
    // ── #1425: cross-backend `backend_conformance!` scenarios ──────────
    // category: quadraui (shared ratatui `TabBar` widget's own hit-test
    // conflates the close glyph with the tab label) — target: quadraui
    // tab-close rect fix.
    "tab_bar_click_closes_via_shared_dispatch::tui",
    "split_tab_bar_click_closes_via_shared_dispatch::tui",
    // ── #1426: same "feature" menu-bar-row divergence as the two entries
    // below, isolated to just the top-anchored row pair — see
    // `issue_1426_app_on_tui_rows_match_shipped_tui`'s own module doc.
    // target: menu-bar caps.
    "render_content_paints_buffer_status_and_command_line_on_shipped_tui_rows::tui",
    // ── #1425: crate::tui_main::app_on_tui_tests driver-test inventory ──
    // Each entry's category/target-child rationale lives as a `// #1425
    // gate:` comment directly above its own `known_bug_gate` call in that
    // module — this list only needs to stay in sync with those labels.
    //
    // category: unit (`App::render_content` used to reserve
    // `render::TAB_ROW_HEIGHT_PX`/`BREADCRUMB_ROW_HEIGHT_PX` as cell-grid
    // *rows* rather than pixels, collapsing the editor content band — and,
    // transitively, everything anchored to it — at a realistic terminal
    // height). #1426 fixed this (target: `render::UnitProfile`) — the 13
    // scenarios that gated purely on the collapsed band are gone from this
    // list; `known_bug_gate`'s `FixLanded` panic on each confirmed it (every
    // one was RED before #1426 and green after, verified by running this
    // module both ways).
    //
    // The four scenarios below stayed gated even after #1426: with the band
    // no longer collapsed, each now fails on a *different*, genuinely
    // unrelated gap #1425 had misdiagnosed as the same collapse (all four
    // ran short-circuited before #1426 — the collapsed band failed them
    // first, so nothing ever exercised what actually breaks them). Each has
    // its own `// #1426 gate:` comment in `app_on_tui_tests.rs` with the
    // corrected root cause.
    //
    // #1427 fixed the "feature" pair below (`App::setup`'s `BackendCaps::
    // window_chrome`/`native_menu` three-way branch — the "menu-bar caps"
    // target these two entries named) — `known_bug_gate`'s `FixLanded`
    // panic on each confirmed it, so they're gone from this list; the
    // remaining two (quadraui, product×2) are unrelated to menu-bar
    // visibility and still open.
    //
    // category: quadraui (`quadraui::native_dialog_options` reports a
    // `Dialog` as "natively expressible" whenever it has no table/input,
    // with no backend-capability gate — so `App::render_content` queues a
    // `PlatformServices::show_message_dialog` present and never paints the
    // in-canvas `Dialog` rung, on *every* backend, not just ones that
    // actually have a native alert facility. TUI has none. A quadraui issue
    // for a `BackendCaps`-gated native-dialog capability must be filed) —
    // target: quadraui.
    "app_on_tui::dialog_intercepts_all_keys",
    "app_on_tui::context_menu_delete_opens_confirm_dialog",
    // category: product (`Ctrl-W v`'s key-dispatch route into
    // `Engine::open_editor_group`/window-split creation doesn't produce a
    // second window on this path — a dispatch gap unrelated to geometry
    // units) — target: TBD, needs its own filed issue.
    "app_on_tui::ctrl_w_v_reserves_one_column_for_the_divider_via_shell_app",
    // category: product (the `Terminal` menu's open-terminal accelerator
    // doesn't open a terminal pane through `App` — a dispatch gap unrelated
    // to geometry units) — target: TBD, needs its own filed issue.
    "app_on_tui::menu_terminal_activation_opens_terminal_pane_via_shell_app",
    // ── #1430 tranche 1 (key dispatch, activity bar, sidebar panels) ────
    // category: product — after Source Control is opened, clicking a
    // *different* activity-bar icon (confirmed with Extensions; the SC
    // panel's own commit-message input is the suspected culprit) never
    // switches the sidebar body, even though the click lands on the
    // correct chrome zone (same coordinates that work when SC was never
    // opened first). See the test's own `#1430 gate:` comment in
    // `app_on_tui_tests.rs` for the isolation that ruled out the
    // Extensions panel itself. Target: needs its own filed issue (SC
    // panel's click handling should release/ignore clicks outside its own
    // bounds).
    "app_on_tui::driver_click_on_every_activity_bar_icon_opens_its_panel_via_shell_app",
    // ── #1431 tranche 2 (popups, pickers, tab bar/drag, bottom band,
    // terminal, dialogs) ─────────────────────────────────────────────────
    // category: unit — `render::editor_popup_anchors`'s completion-popup
    // anchor lands at the window's own local x=0 rather than the real
    // terminal column, i.e. it omits the active window's own rect.x
    // offset. On the shipped TUI (`TuiShellApp`) the editor window's rect
    // always starts at terminal column 0 (no persistent left chrome), so
    // the two coincide and the bug never surfaces there; `App`'s
    // `AppShell` reserves a permanent activity-bar column even with the
    // sidebar body collapsed, so its editor window starts a few columns
    // in and the omission becomes visible — observed popup x=2 (matching
    // the activity-bar+divider width) against a real terminal cursor at
    // x=19. Target: TBD, needs its own filed issue
    // (`render::editor_popup_anchors`).
    "app_on_tui::completion_popup_anchors_at_the_real_cursor_column_on_tab_indented_line_via_shell_app",
    // category: quadraui — same root cause as `popups::dialog_intercepts_
    // all_keys` above: `quadraui::native_dialog_options` reports a
    // button-only, no-input `Dialog` (or the `:CheckNerdFonts` dialog,
    // also button-only) as natively expressible with no `BackendCaps`
    // gate, so `App::render_content` queues a native
    // `PlatformServices::show_message_dialog` present instead of painting
    // the in-canvas `Dialog` rung, on every backend — TUI has none.
    // Target: quadraui.
    "app_on_tui::render_content_paints_dialog_via_shell_app",
    "app_on_tui::check_nerd_fonts_dialog_paints_both_variants_via_shell_app",
];

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
    /// and nobody deleted the `KNOWN_BUGS` entry. Reported as a failure:
    /// this is the direction that rots silently if untested (#982's own
    /// self-test below exists because of this arm specifically).
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
/// Uses `catch_unwind` (unwinding is the default in this crate — no `panic
/// = "abort"` in `Cargo.toml` — same pattern as `src/app.rs:7295`,
/// `src/render.rs:8246`, `src/tui_main/mod.rs:404`), wrapped in
/// [`std::panic::AssertUnwindSafe`] exactly as those three call sites are:
/// a `ConformanceHarness` closes over `Rc<RefCell<_>>`/interior-mutable
/// state throughout (`Engine`, the ratatui `Terminal`, ...), none of which
/// is `UnwindSafe` by the auto-trait's strict definition, but that
/// strictness is about *reading possibly-torn state after recovering from
/// a panic* — this function never does that: on either outcome the whole
/// `body` (harness included) has already been dropped by the time
/// `catch_unwind` returns, so nothing torn is ever observed.
///
/// While `label` is listed, the process's panic hook is replaced with a
/// no-op for the duration of the call (restored before returning either
/// way, via a guard so a panicking `body` can't skip the restore) — a
/// listed, still-open bug's expected panic should not spam a green `cargo
/// test` run with a backtrace every time it runs. **Not** suppressed when
/// `label` is unlisted: an unexpected panic (the [`Regression`](GateOutcome::Regression)
/// arm) is exactly the case that should stay loud.
///
/// # The guard-poisoning trap
///
/// A `ConformanceHarness` holds a [`crate::test_paint::PaintGuard`] and a
/// [`crate::test_cwd::CwdReadGuard`] — process-wide `Mutex`/`RwLock`s. If
/// `body` panics while one is alive (the expected shape for a
/// [`ExpectedFail`](GateOutcome::ExpectedFail) run), that lock is
/// poisoned by the unwind. Both guards already handle this at the
/// primitive level — `PaintGuard::acquire`/`CwdReadGuard::acquire` both
/// recover via `.unwrap_or_else(|e| e.into_inner())`, their own module
/// docs spelling out exactly why: "one red test must stay one red test
/// rather than cascading into every later painting test panicking on a
/// poisoned lock".
/// [`tests::known_bug_gate_panic_does_not_poison_guards_for_a_later_harness`]
/// below exercises this directly rather than trusting that description: it
/// runs an `ExpectedFail` body that panics while holding a real
/// `conformance_harness`-built harness, then builds and uses a second,
/// fresh harness afterwards on the same thread, proving the first
/// harness's teardown left nothing poisoned for the second to trip on.
///
/// # A residual limitation, stated rather than silently accepted
///
/// `std::panic::set_hook`/`take_hook` are process-global, not scoped to one
/// call — `cargo test`'s thread pool means a genuinely unrelated test
/// panicking on another thread during this call's brief suppression
/// window would have *its* backtrace swallowed too. `KNOWN_BUGS` is empty
/// as of #982 and grows by exactly one entry per already-triaged, already
/// slow-to-fix bug (not a general-purpose "hide flaky panics" tool), so the
/// window in practice is rare and short; a `HOOK_LOCK` mutex below at least
/// serialises concurrent [`known_bug_gate`] calls against each other, which
/// is the part actually under this module's control.
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

// ── The cross-backend runner (#982) ─────────────────────────────────────
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
/// own `mod_path::backend` test-name nesting is what keeps a single-backend
/// failure self-locating, the same property `..._on_gtk`/`..._on_tui`
/// naming would give, without needing identifier concatenation (no
/// `concat_idents!`/proc-macro dependency to get there). The `gtk` arm is
/// gated on `feature = "gui"`, the same gate `vimcode`'s own
/// `required-features` puts on the GTK bin; the `tui`/`tui_prod` arms have
/// no gate — `quadraui/tui` is an unconditional feature of the pinned
/// dependency (see `Cargo.toml`), not an optional vimcode one.
///
/// # `tui` vs `tui_prod` (#1043)
///
/// These are **two different TUI arms**, not a typo for one — see
/// `crate::tui_main::testing`'s own "Two TUI arms" doc for the full
/// reasoning. In short: `tui` wraps [`crate::app::App`] (the
/// cross-backend-shared shell, also what `gtk` wraps) on
/// `quadraui::tui::TuiBackend` — it is the *control* that isolates
/// "rasteriser difference" from "implementation difference". `tui_prod`
/// wraps [`crate::tui_main::testing::TuiShellApp`] — the independently
/// hand-written shell `tui_main::run` actually ships. A scenario green on
/// `gtk`+`tui` but red on `tui_prod` is, by construction, the shipped TUI
/// diverging from the shared shell, not a paint-surface artifact — exactly
/// the class of bug #1025 was before a user found it by hand. Not every
/// scenario can run on `tui_prod` yet — see
/// `crate::tui_main::testing::conformance_harness_prod`'s own doc for
/// which trait bounds it satisfies and which (`ConformanceHarness::engine`/
/// `::screen_layout`) it does not.
///
/// Only `gtk`/`tui`/`tui_prod` are wired today. Growing this to `macos`/
/// `win` is adding their own `@arm` match below, mirroring their existing
/// `conformance_harness` constructors (`src/macos/mod.rs:243`,
/// `src/win/mod.rs:188`) — each behind that backend's own vimcode feature
/// gate, same shape as the `gtk` arm.
///
/// `engine`/`size`/`body` are each re-evaluated once per backend arm (not
/// shared across them) — the intended shape, since every existing fixture
/// in this repo that touches the filesystem (`scratch_dir`/
/// `scratch_explorer_dir` helpers) already disambiguates by
/// `std::thread::current().id()`, and `cargo test` runs each generated
/// `#[test]` fn on its own thread, so two backends' arms never collide on
/// the same path even though this macro duplicates the fixture-building
/// expression textually.
///
/// `#[macro_export]` (rather than a manual `pub(crate) use`) so the `@arm`
/// recursive expansion below can call itself via `$crate::backend_conformance!`
/// — `$crate` always resolves against the crate root, which is where
/// `#[macro_export]` places a macro regardless of which module defines it.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal engine fixture for the cross-backend proof slice below:
    /// an explorer rooted at a scratch dir, with the root and `src`
    /// expanded so `src`'s child `core` paints directly beneath it — the
    /// same shape `src/macos/mod.rs`'s own `sweep_hit_band_integrity`
    /// coverage (`explorer_click_hit_band_matches_the_painted_row`, #967/
    /// #968) uses, reproduced here rather than shared with it (that
    /// fixture is private to `src/macos/mod.rs`'s own test module) so this
    /// module's proof slice doesn't reach into another backend's file.
    ///
    /// `tag` must be distinct per caller (this fn is called once per
    /// generated backend arm below) — combined with the calling thread's
    /// id, per `backend_conformance!`'s own doc on why that's enough to
    /// avoid two backends' arms colliding on the same scratch directory.
    fn engine_with_expanded_explorer(tag: &str) -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_982_harness_proof_{tag}_{:?}",
            std::thread::current().id()
        ));
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
    // `sweep_hit_band_integrity` is the scenario the issue itself names as
    // bounded by `ConformanceDriver + DriverInput` (not
    // `PixelClickConformance`), i.e. exactly the one this proof slice needs
    // to demonstrate the macro expands identically on both GTK and TUI.
    // RED-verification: with `impl TextMetricsBackend for TuiBackend`'s two
    // setters temporarily changed to write a value the getter never reads
    // back (impossible to construct meaningfully here, since `TuiBackend`'s
    // getters are hardcoded — see that impl's own doc) there is no
    // TUI-side drift to provoke; this proof slice's RED-verification is
    // therefore the same one `src/macos/mod.rs`'s
    // `explorer_click_hit_band_matches_the_painted_row` already carries
    // (reverting the GTK/macOS `TextMetricsBackend` fix takes the *gtk*
    // arm here red), confirming this is the same shared scenario body,
    // not a fork of it.
    //
    // #1043 adds `tui_prod`: `TuiShellApp`'s own explorer click routing
    // (`tui_main::mouse`) is a completely independent implementation of the
    // same #967 hit-band contract, so this is genuine new coverage, not
    // just a third copy of the same assertion.
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
    // Ruling from #1258 item 4: the SC panel's bottom row reads "Press '?'
    // for help" on both backends while the panel has keyboard focus, and
    // the row isn't reserved when it doesn't. Before this fix, the hint
    // existed only in `tui_main::panels::render_source_control` (gated on
    // `sc.has_focus`) — GTK's `App::paint_sidebar_panel_rung` `PANEL_GIT`
    // arm had no equivalent, so a real click on the SC activity-bar icon
    // (#1360) never painted a hint row on GTK. Fixed by moving the row
    // reservation into the shared `render::sc_sidebar_bands` (an
    // `Option<Rect>` `hint` field, gated on the same `has_focus` bool both
    // backends already pass it) and painting it through one shared
    // `render::sc_hint_status_bar`, exactly as `render::sc_header_status_bar`
    // already does for the row above it.
    //
    // `tui` (the `crate::app::App`-wrapped control) used to be omitted from
    // this scenario's `backends` list on the grounds that it "adds no
    // independent signal" — `App`'s `PANEL_GIT` arm is the exact GTK code
    // path under test (`src/app.rs`'s `paint_sidebar_panel_rung`, shared by
    // both `crate::gtk::run` and this harness's `gtk` arm), not a second,
    // independent implementation the way `tui_prod`'s
    // `tui_main::panels::render_source_control` is, unlike
    // `sweep_hit_band_integrity_proof` above where isolating "rasteriser"
    // from "implementation" is the point. #1425's App-on-TUI gap inventory
    // asked for that reasoning to be checked rather than trusted: it *is*
    // checked here now — `App` painting through `quadraui::tui::TuiBackend`
    // does reach the same `PANEL_GIT` bands/`Escape` focus-release path as
    // `gtk`, so this arm passes ungated, confirming "no independent signal"
    // was correct rather than merely assumed.
    //
    // RED-verified (#1361): with `App::paint_sidebar_panel_rung`'s
    // `PANEL_GIT` arm's `bands.hint` paint call removed, the `gtk` arm below
    // failed — `screen_has("Press '?' for help")` was false even though
    // `sc.has_focus` was true, reproducing the issue's own "Actual" table.
    fn engine_with_sc_panel(tag: &str, focused: bool) -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1361_sc_hint_{tag}_{:?}",
            std::thread::current().id()
        ));
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

            // Move focus back to the editor (#1361's second half: "when it
            // doesn't have focus, the row isn't reserved"). `Escape` is
            // `Engine::handle_sc_key`'s own focus-release binding
            // (`self.sc_set_focus(false)`), shared by both backends.
            driver.press_named(quadraui::NamedKey::Escape);
            assert!(
                !driver.screen_has("Press '?' for help"),
                "the SC panel lost keyboard focus, so the hint row must no \
                 longer be reserved/painted"
            );
        },
    }

    // ── #1361 review: hit-testing must account for the reserved hint row ──
    //
    // The acceptance bar for this half names `sweep_hit_band_integrity`
    // — tried first against the SC panel's "STAGED CHANGES" section
    // header (focused, hint row reserved), fingerprinting on whether
    // "sc1361file.txt" stayed visible across the section's collapse
    // toggle. That attempt (and its `_resetting` twin, built for exactly
    // the "same-spot restore click" confound this widget has, per
    // `src/gtk/testing.rs`'s existing `hit_band_sweep_971` module) reports
    // the identical alternating true/false/true/false/true outcome
    // pattern, at the identical y-offsets, on `gtk` — and reproduces
    // byte-for-byte with `has_focus: false` (no hint row reserved at
    // all). That rules out both the click-restore mechanism and this
    // issue's own fix as the cause: it is a pre-existing,
    // focus-independent hit-band inaccuracy in `quadraui::SidebarSystem`'s
    // own section-header row — unrelated to #1361, but flagged here
    // rather than silently dropped: whoever next touches the SC/ext-panel
    // section-header hit band should file a quadraui issue for it (the
    // repro is `sweep_hit_band_integrity(driver, "STAGED CHANGES", 5,
    // |d| d.screen_has("sc1361file.txt"))` against `engine_with_sc_panel`
    // below, on `gtk`) before relying on `sweep_hit_band_integrity`
    // against any `SidebarSystem` header again.
    //
    // The review's own minimal bar — "even a single click-and-assert on
    // a content row with focus/hint reserved would close the gap" — is
    // what the two `#[test]` scenarios below deliver, sidestepping the
    // header bug entirely by clicking a plain content row (a file entry)
    // and the hint row itself, neither of which is a `SidebarSystem`
    // header:
    //
    //   1. `sc_content_row_click_still_opens_it_when_hint_reserved_gtk`:
    //      with the panel already focused (hint painted, slab one row
    //      shorter), a double-click on a real file's own row must still
    //      open *that* file — proving the click router resolves against
    //      the exact shrunk geometry the painter used.
    //   2. `sc_hint_row_click_does_not_open_a_content_row_gtk`: a
    //      double-click landing inside the hint row's own painted band
    //      must have no effect on any content row beneath it — the other
    //      half of "the hint row is reserved": reservation must apply to
    //      the click router, not just the painter.
    //
    // Both are real regressions to catch, not just re-statements of the
    // geometry unit tests: `render::sc_sidebar_bands_tests` (next to
    // `sc_sidebar_bands`, in `src/render.rs`) proves the *pure* band
    // math is internally consistent (slab shrinks by exactly one row,
    // hint sits flush with no gap/overlap, header/commit-input never
    // move) — it says nothing about whether the real `SidebarSystem`
    // widget (`sc_panel_layout`/`sc_sidebar_body_rect`) actually hit-tests
    // against that same shrunk rect once wired through `Engine`/`App`.
    // The two scenarios below are what closes that integration gap.
    //
    // RED-verification: reverting `App::paint_sidebar_panel_rung`'s
    // `PANEL_GIT` arm to drop its `bands.hint` paint call (this issue's
    // own fix, as `sc_hint_row_shows_only_while_focused` above already
    // documents) fails both scenarios' own precondition assert
    // (`screen_has("Press '?' for help")`) on `gtk` before either ever
    // reaches its double-click — the same shared precondition
    // `sc_hint_row_shows_only_while_focused` uses, RED-verified there.
    //
    // `gtk`-only, **not** registered on `tui_prod`: driving either
    // scenario through `tui_prod` (`TuiShellApp` + `super::mouse::
    // handle_mouse`) hits a second, entirely different pre-existing bug
    // from the header one above — a genuine `MouseDown` dispatched at
    // *any* point inside the SC panel (reproduced with a bare click on
    // the "SOURCE CONTROL" header text itself, no hint/focus/content-row
    // involvement at all) blanks the **whole** sidebar (header, commit
    // box, toolbar, section list all vanish; only the activity-bar
    // column and chrome borders remain) on the very first click, for
    // both `sc_has_focus: true` and `sc_has_focus: false`. Reproduced
    // against `engine_with_sc_panel`, the same fixture
    // `sc_hint_row_shows_only_while_focused` already uses (which never
    // clicks — only `press_named`/`screen_has` — so it never hit this),
    // at both this issue's own (800, 480) viewport and a plain (100, 30)
    // one, so it is not a viewport-size artifact either. `tui_prod`'s
    // sibling explorer coverage (`sweep_hit_band_integrity_proof` above)
    // clicks through the identical `TuiShellApp`/`handle_mouse` pipeline
    // successfully, so this is specific to the SC panel's own click arm
    // in `tui_main::mouse::handle_mouse`, not a `conformance_harness_prod`
    // limitation in general. This diff's own `tui_main/mouse.rs` change is
    // a one-line, backward-compatible parameter addition (`engine.
    // sc_has_focus` passed to the already-existing `sc_sidebar_bands`
    // call) — reproducing with a header click and `sc_has_focus: false`
    // rules this diff out as the cause the same way the SidebarSystem
    // header bug above was ruled out for #1361. Left here rather than
    // silently dropped, same as the header bug: whoever next touches
    // `tui_main::mouse`'s SC panel arm or adds `tui_prod` click coverage
    // to this panel should file a vimcode issue for it first.

    /// Like [`engine_with_sc_panel`], but with one real (untracked) file
    /// on disk so a double-click on its row has a genuine, painted side
    /// effect (`sc_activate_row`'s open path) to assert on — the same
    /// technique `src/tui_main/shell_app.rs`'s
    /// `tui_sc_sidebar_double_click_on_a_changed_file_opens_it` (#817)
    /// uses for this exact click path, run here with `sc_has_focus: true`
    /// from the first frame so the hint row is already reserved (slab one
    /// row shorter) when the click below lands.
    fn engine_with_sc_panel_and_file(
        tag: &str,
        file_name: &str,
        marker: &str,
    ) -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1361_sc_row_{tag}_{:?}",
            std::thread::current().id()
        ));
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
        // Untracked, like #817's fixture: `sc_activate_row` takes the
        // plain-open path rather than the diff-split path, so no real
        // `git show`/`diff` subprocess is needed.
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
        // (`quadraui::dispatch::DoubleClickDetector`, the same folding
        // `src/gtk/testing.rs`'s `hit_band_sweep_971` module's own doc
        // explains) — `sc_activate_row`'s open path, same mechanism as
        // #817's `tui_sc_sidebar_double_click_on_a_changed_file_opens_it`.
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
        // other half of "the hint row is reserved" (#1361's own acceptance
        // bar).
        h.driver.click_text_at("Press '?' for help", Anchor::Center);
        h.driver.click_text_at("Press '?' for help", Anchor::Center);

        assert!(
            !h.driver.screen_has("SC1361HINTMARKER"),
            "a double-click on the reserved hint row must not open any \
             content row beneath it — the reservation the painter applied \
             must also be excluded from the click router's own geometry"
        );
    }

    // ── Deliverable 4, item 2: both gate directions (#982) ──────────────
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

    // ── Coverage for the real public wrapper, not just the outcome fn ───
    //
    // The four tests above exercise `known_bug_gate_outcome_for_test` (a
    // test-only twin that can pretend any label is listed). These two
    // exercise `known_bug_gate` itself — the panic-or-not wrapper every
    // real per-bug scenario `#[test]` will actually call — against its two
    // reachable arms given `KNOWN_BUGS` is empty (#982 ships no real entry
    // yet): unlisted-passing (must not panic) and unlisted-panicking (must
    // panic, i.e. fail the calling test). The listed arms
    // (`ExpectedFail`/`FixLanded`) are exactly what the table-driven tests
    // above already cover via the parameterized twin.
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

// ── #983 / #1028: settings / git-insights row click vs. the painted row ──
//
// v0.11.0 bug suite. #983 reported that a click in a row's padding — below
// its text glyph but above the next row's label — acted on the row below,
// on the Settings panel (`FormController`) and on the ext-panel/marketplace
// `SidebarSystem` that "git insights" (a plugin panel) routes through.
//
// #1028 went to fix it and found there is nothing to fix: measured against
// the real frame rather than against `text_runs()` glyph bounds, each row's
// *painted background band* and its *click hit band* are identical to the
// pixel on both panels (Settings `▼ LSP`: painted [389, 421), hit
// [389, 421); ext panel `AVAILABLE`: painted [741, 773), hit [741, 773)) —
// and both bands are filled in a visibly different colour from their
// neighbours, so the boundary is drawn, not invisible. The original probe
// point (`next_row_glyph.y - 0.5`) was not inside the clicked row's band at
// all; it sat 4px / 7.5px *below* the next row's band top edge, inside that
// next row's own painted background. Neither vimcode's
// `render::handle_settings_form_ui_event` nor quadraui's
// `FormController::click_inner` / `SidebarSystem` row resolution is at
// fault, so no quadraui issue was filed. See
// `row_click_in_its_painted_band_hits_its_own_row`'s own doc for the full
// table and method.
//
// The scenarios below therefore assert the property the report *meant*,
// ungated: a click anywhere in a row's painted band — the padding strip
// above its glyph *and* the one below it, deliberately both, so fixing one
// edge can never quietly move a bug to the other — acts on that row. TUI is
// excluded, not silently skipped: its row pitch always equals its glyph
// height by construction (fixed `TextMetricsBackend` no-op,
// `src/tui_main/mod.rs`), so it has no padding strip to probe and the
// helper's own sanity assert fails loudly there rather than reporting a
// false pass.
#[cfg(test)]
mod issue_983_row_click_selects_the_row_below {
    use super::*;
    use crate::core::engine::sidebar::PANEL_SETTINGS;

    // #1089's own retirement: this module used to also carry
    // `engine_git_insights_scrolled_to_available` and four tests built on
    // it (`ext_panel_row_click_below_its_glyph_hits_its_own_row_gtk`,
    // `ext_panel_row_click_above_its_glyph_hits_its_own_row_gtk`,
    // `ext_panel_row_sweep_hit_band_integrity_gtk`,
    // `ext_panel_row_sweep_hit_band_integrity_tui`). That fixture set
    // `ext_panel_active` to `"git-insights"` — a name with **no**
    // `PanelRegistration` — and its own doc comment named exactly the gap
    // #1089 closes: "`Engine::populate_ext_sidebar_system` … always builds
    // its rows from the *marketplace* manifest list, regardless of which
    // plugin id is active — a separate, already-documented gap … not this
    // issue's [#983's] to fix." Now that `App::paint_sidebar_panel_rung`'s
    // `ext:` arm paints a real `PanelRegistration`'s own sections (#1089),
    // that fixture paints nothing and all four tests fail on their shared
    // precondition ("AVAILABLE" is not painted). The edge-of-glyph and
    // sweep-integrity properties they existed to pin are now covered
    // against a genuine plugin panel by `crate::harness::plugin_panel`
    // (`src/harness/plugin_panel/tests.rs`'s `plugin_panel_section_header_
    // hit_band_on_gtk`/`_on_tui_shared_app` and `plugin_panel_item_row_hit_
    // band_on_gtk`/`_on_tui_shared_app`), built on `engine_with_plugin_panel`
    // (a real registration + `ext_panel_items`) per #1089's own "Shape of
    // the work" item 3. The settings-panel half of this module (unaffected
    // by #1089 — Settings paints through `FormController`, not
    // `ext_sidebar_system`) stays below.

    /// The Settings panel, scrolled so the "LSP" category — flat row 47 of
    /// 62, well below row 0 per this issue's own instruction not to reuse
    /// `settings_panel_click_toggles_the_clicked_category`'s row-0 target —
    /// paints inside a normal 1400x900 viewport instead of needing an
    /// oversized window. LSP's own first two settings ("Enable LSP",
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
        engine.settings_scroll_top = 40;
        engine
    }

    // ── Deliverable 1: click the padding strip on *both* sides of a row's
    // own text glyph, well below row 0, and assert the painted outcome
    // landed on the clicked row ────────────────────────────────────────
    //
    // Hand-written rather than `backend_conformance!`-registered: they are
    // GTK-only by construction — see this module's top doc — so the
    // macro's per-backend expansion buys nothing, and (b) the probe point
    // is derived from the frame's own *pixels* (`GtkDriver::pixel`, via
    // `harness::painted_row_band`), and `pixel` is an inherent `GtkDriver`
    // method, not part of the backend-neutral `ConformanceDriver` trait the
    // macro's `|driver|` body is generic over.
    //
    // RED-verification (#1028): these are not green by accident —
    // widening the returned band by a single pixel past the measured
    // boundary (`(b.0, b.1 + 1.0)` in `probe_band`) fails the
    // `BelowGlyph` test on its *final* assertion ("y=421.5, band
    // [389, 422)") — because that one extra pixel is already the first
    // row of the *next* row's fill, and the click lands there. That
    // one-pixel sensitivity is the whole content of these tests: they
    // pin paint and hit to the same boundary, which is exactly what a
    // #967-family drift would break. Seeding `probe_band` off a
    // *neighbouring* row ("Enable LSP" instead of "▼ LSP") likewise fails
    // on the band-containment sanity assert ("the measured band [421, 461)
    // must contain \"▼ LSP\"'s own glyph ([393.5, 416.5])"), so a
    // mis-seeded band can never masquerade as a passing probe.

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

    // ── Deliverable 2: sweep_hit_band_integrity over a settings row — the
    // "similar bugs" generalization. Samples strictly inside the needle's
    // own painted glyph bounds (per that helper's own contract), i.e. the
    // interior of the band Deliverable 1 above probes the *edges* of (see
    // `row_click_in_its_painted_band_hits_its_own_row`'s doc) — so this is
    // expected to pass today, on every backend, with no KNOWN_BUGS entry.
    // What it protects against is a *different*, #967-style regression
    // creeping into the row-pitch formula later, and it costs nothing
    // extra to also run on TUI (`ConformanceDriver + DriverInput` is
    // TUI's own bound, not a GTK-only one — see this module's top doc on
    // "Which trait bound a scenario needs"), nor on `tui_prod` (#1043) —
    // `TuiShellApp` renders the Settings panel through the same
    // `render::handle_settings_form_ui_event` shared code `App` does.
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

// #984: v0.11.0 bug report -- the file explorer's expand/collapse chevron
// needs a double click, while its row's text label needs one. See
// `explorer_chevron_click_toggles_dir_with_same_arity_as_label_click`'s own
// doc for the root cause (a missing `TreeControllerEvent::RowToggleExpand`
// match arm in `Engine::dispatch_explorer_tree_event`, shared core code) and
// why it reproduces on both backends -- unlike #983's GTK-only row-pitch gap,
// this bug lives entirely above the backend split, so both gated tests below
// are listed in `KNOWN_BUGS`, not just one.
#[cfg(test)]
mod issue_984_explorer_chevron_needs_a_double_click {
    use super::*;

    /// An explorer rooted at a scratch dir with one collapsed directory,
    /// `kkxxqq_dir984`, holding one child, `child984mk` -- the root
    /// itself is expanded (so `kkxxqq_dir984`'s own row paints) but
    /// `kkxxqq_dir984` is deliberately left out of `explorer_expanded`, so
    /// it starts collapsed, matching this scenario's own precondition.
    ///
    /// `child984mk` is short on purpose, like `CONFLICT_FIXTURE_FILE` above
    /// -- at this fixture's own depth-2 nesting (root -> `kkxxqq_dir984` ->
    /// this file), the shared `App::shell_config()` conformance harness's
    /// (unwidened, quadraui-default) sidebar leaves only ~12 columns for a
    /// TUI row label, and a name past that column budget paints truncated
    /// (e.g. the `_marker` suffix this fixture used to carry never actually
    /// reached the screen on TUI, independent of and unrelated to #984's own
    /// chevron bug). `screen_has` needs the *whole* name painted, so this
    /// fixture stays inside that budget rather than one `assert!` silently
    /// depending on a name that happens to fit today.
    ///
    /// `tag` must be distinct per caller (each backend's `#[test]` below
    /// calls this once) -- combined with the calling thread's id, same
    /// disambiguation rule `backend_conformance!`'s own doc spells out for
    /// every other filesystem-touching fixture in this module.
    fn engine_with_collapsed_explorer_dir(tag: &str) -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_984_explorer_chevron_{tag}_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("kkxxqq_dir984")).unwrap();
        std::fs::write(dir.join("kkxxqq_dir984").join("child984mk"), b"").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir.clone();
        engine.explorer_expanded.insert(dir.clone());
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
        // #1427: `session.explorer_visible` alone only reaches the shadow
        // `engine.app_shell`'s `sidebar_visible()` through
        // `Engine::sync_app_shell_sidebar_visibility`, which already ran
        // (inside `Engine::new_for_test()`, above) *before* this line set
        // the field — the same "mutate an already-built Engine" gap that
        // method's own doc describes. `TuiShellApp::from_engine` (the
        // `tui_prod` arm's constructor) re-runs that sync itself, picking
        // this up; the `tui` arm's `App::new_headless_with_backend` does
        // not, so the shadow's `sidebar_visible()` would otherwise stay
        // stale-`false` — which `render::sync_runner_sidebar_visibility`
        // (called unconditionally on every dispatch, #1427) then mirrors
        // onto the *runner's* own `AppShell`, collapsing the sidebar
        // column entirely after the very first click and erasing the
        // chevron-expand this scenario asserts on. `show_panel` sets the
        // shadow's `sidebar_visible` directly, sidestepping the staleness.
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine
    }

    // ── Deliverables 1+2: chevron/label click-arity parity on the same
    // row (the horizontal axis of the row) ─────────────────────────────
    //
    // Not via `backend_conformance!`: unlike #983's GTK-only gated
    // scenarios (which only ever need one backend arm, so the label
    // string is a single literal), this bug reproduces on *both*
    // backends, and each arm needs its own `KNOWN_BUGS` label suffix
    // (`::gtk` / `::tui`) baked into the body -- the macro expands one
    // `$body` token stream verbatim into every listed backend arm, with
    // no way for that body to know which arm it's in. Hand-written here,
    // mirroring the macro's own `@arm` expansion shape 1:1 (same
    // `conformance_harness` call, same `let driver = &mut __h.driver`)
    // so the only real difference from a macro-generated test is the
    // label string -- the same reason #983's `_resetting` sweep tests are
    // hand-written instead of macro-generated.
    //
    // RED-verification (#984): with both `KNOWN_BUGS` entries below
    // removed, `cargo test --lib
    // issue_984_explorer_chevron_needs_a_double_click` fails both tests on
    // the first assertion inside
    // `explorer_chevron_click_toggles_dir_with_same_arity_as_label_click`
    // -- "child984mk" is still not painted after one chevron click,
    // on both backends. Restored (entries back in place) and confirmed
    // green again -- see this issue's PR notes for the captured failure.
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

    // #1043: the `tui_prod` twin — same scenario, driven through the actual
    // production TUI shell (`TuiShellApp`, its own independently
    // hand-written `tui_main::mouse` hit-testing) instead of the shared
    // `App`. Its own `::tui_prod`-suffixed `KNOWN_BUGS` label, per this
    // module's own disambiguation rule (each backend arm needs its own
    // suffix once a body can diverge per-arm — see the `::gtk`/`::tui`
    // labels above).
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

    // ── Deliverable 3: sweep_hit_band_integrity across the directory
    // row's own painted label (the vertical axis of the same row) ──────
    //
    // Ungated -- passes on both backends today, with no `KNOWN_BUGS`
    // entry. This is not a duplicate of the horizontal parity check
    // above: it samples multiple y-offsets strictly inside the label's
    // own painted glyph bounds (the zone `sweep_hit_band_integrity`'s own
    // #967 family lives in), proving no #967-style vertical hit-band
    // drift on the explorer's own expand/collapse toggle -- exactly the
    // self-restoring toggle that helper's doc names as the reference
    // case it was built for.
    //
    // #1043: also registered on `tui_prod` — the explorer tree is exactly
    // the surface #1025's production-only right-click regression lived in
    // (`tui_main::mouse`'s own, independently hand-written hit-testing), so
    // this is a scenario worth actually pointing at the shipped TUI rather
    // than only at the shared `App`.
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

// #987: v0.11.0 bug report -- "the scrollbar in the left editor tab group
// doesn't work, and clicking it instead triggers a resize between tab
// groups". `SCROLLBAR_IMPLEMENTATION.md`'s own "Known Limitations" section
// already named the first half ("non-active window scrollbars are
// visual-only") -- this issue is test-only (no fix), pinning both halves as
// one conformance case per the Platform-Neutrality Rule's "prove it before
// you fix it" posture.
//
// # Root cause, confirmed by driving the harness (not guessed)
//
// `crate::app::App` -- the one shared `ShellApp` both the real GTK backend
// and this harness's own "tui" conformance arm dispatch mouse events
// through (see this module's own "Which trait bound a scenario needs" doc:
// `crate::tui_main::testing::conformance_harness` wraps `App`, not the
// production `TuiShellApp`/`tui_main::mouse.rs` -- a separate, hand-written
// stack that has its own, independently-written version of this same bug
// shape, out of this issue's `src/harness.rs` scope) has **no vertical
// scrollbar hit-test at all** in `handle_mouse_click_msg`: it hit-tests the
// horizontal scrollbar (`h_scrollbar_hit_test`), then the group/window
// dividers (`render::route_divider_grab`), then falls through to ordinary
// editor click handling -- there is no third rung for
// `quadraui::EditorLayout::hit_test`'s `EditorHit::VScrollbar` arm, even
// though quadraui has painted a real per-window vertical scrollbar since
// quadraui#968 (`quadraui::Editor::layout_with_options` reserves exactly
// one `cell_width`-wide column at the window's own right edge whenever
// `total_lines > visible_lines`, on both backends).
//
// That gap makes the scrollbar inert on **every** window, active or not --
// not just "non-active" as the stale doc above says (confirmed: the bug
// reproduces identically whether the window being dragged is
// `engine.active_window_id()` or not; this suite's own fixtures focus the
// *other* group precisely to rule out "maybe it only breaks when focused"
// as the explanation). Whether that inert click also **resizes** depends on
// nothing more than geometry: `render::divider_ratio_from_pos` reads
// whatever `x` a press landed at and re-derives the ratio from it on every
// later move event, and a window's own scrollbar column is always the last
// `cell_width` before that window's edge -- which, for any window sitting
// immediately left of a group divider (the reported "left group"), is
// within `GTK_DIVIDER_METRICS`'s 6-unit grab band around the divider's own
// `position` (confirmed: with the default minimap painting a wide strip
// between a window's own rect and the next group's divider, the two don't
// overlap and the resize half does not fire -- every fixture below
// explicitly disables the minimap, `engine.settings.minimap = false`, to
// reproduce the adjacency the report describes). A window with no divider
// on its scrollbar-adjacent side (the **right** group in a two-group
// layout) is simply inert, with no resize side effect -- deliverable 3
// below pins exactly that distinction.
//
// # TUI reproduces too -- this is shared-code, not backend-specific
//
// Per this issue's own "report whether TUI reproduces" acceptance item:
// **yes**, identically, because both `backend_conformance!` arms below
// drive the exact same `src/app.rs` dispatch code (`ConformanceHarness`'s
// "tui" arm wraps `App`, not `TuiShellApp` -- see above). That is good news
// under the Platform-Neutrality Rule: there is no backend-specific
// scrollbar hit-test to delete, because neither backend has *any*
// vertical-scrollbar hit-test in the shared dispatch path the fix would add
// to -- one `EditorHit::VScrollbar` rung in `src/app.rs`'s divider-rung
// neighbourhood fixes both at once. (The real production TUI stack,
// `tui_main::mouse.rs`, has its own separately hand-written vertical
// scrollbar hit-test that runs *after* its own `route_divider_grab` call --
// an independently-arrived-at instance of the identical ordering bug, left
// undisturbed here since fixing it is out of this test-only issue's scope
// and its own file is not part of `src/harness.rs` + per-backend
// registration.)
// #1043: no `tui_prod` coverage in this module. Every scenario here
// (`drag_group_scrollbar_column`, `drag_group_divider_resizes`) takes the
// whole `&mut ConformanceHarness<D>` and reads `h.screen_layout` to locate
// a window's own painted rect (a scrollbar thumb has no text run to search
// for) — `App`'s `cached_screen_layout` is an `Rc<RefCell<Option<ScreenLayout>>>`
// specifically so `ConformanceHarness::new_with_screen_layout` can keep a
// live handle to it after `App` is moved into `driver_with_shell`.
// `TuiShellApp`'s own layout cache (`last_layout`) is a private, non-`Rc`
// `RefCell`, so `crate::tui_main::testing::conformance_harness_prod` has no
// live handle to hand back (`ConformanceHarness::screen_layout` is `None`
// there — see that function's own doc). Porting this family needs either a
// `TuiShellApp`-side `Rc`-wrapped accessor (a `shell_app.rs` change outside
// this harness-wiring issue's file scope) or a window-rect probe that
// doesn't depend on it — filed as a follow-up rather than silently
// skipped.
#[cfg(test)]
mod issue_987_group_scrollbar_inert_and_click_resizes {
    use super::*;
    use crate::core::window::SplitDirection;
    use crate::core::{Engine, WindowId};

    /// A two-group vertical split, each group showing a distinct 2000-line
    /// buffer (`{left,right}line{0..2000}_{tag}`) so
    /// [`drag_group_scrollbar_column`]'s `top_line_needle` can never
    /// ambiguously match the other window's content, and so each window
    /// genuinely needs a vertical scrollbar (`total_lines` far exceeds any
    /// viewport this harness paints). `focus_left` picks which group ends
    /// up active -- deliverable 3 needs the *other* group focused from
    /// whichever one its own scrollbar drag targets, so both directions
    /// share this one fixture rather than two near-identical copies.
    ///
    /// `engine.settings.minimap = false` reproduces the adjacency the
    /// report describes -- see this module's own top doc for why a
    /// default-on minimap would put ~50 columns of unrelated space between
    /// a window's own scrollbar and the group divider, hiding the "click
    /// resizes" half entirely.
    fn engine_two_groups(tag: &str, focus_left: bool) -> (Engine, WindowId, WindowId) {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.settings.minimap = false;
        // #1427: pin the shadow `engine.app_shell`'s sidebar open (on
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
    // RED-verification (#987): both assertions below were observed to fail
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

    // ── Deliverable 3: the right group's own scrollbar, left focused ────
    //
    // Generalizes deliverable 1 from "the left one" to "any group": the
    // right group has no divider on its scrollbar-adjacent side (the
    // screen's own right edge, in a two-group layout), so only the
    // "inert" half of #987 is expected to reproduce here -- confirmed by
    // observation (see this test's own RED note) that dragging it changes
    // *neither* window's rect at all. Still wrapped in one `known_bug_gate`
    // call (the missing scroll still panics the body), but the resize
    // assertion is ordered *first* so it is genuinely exercised (and would
    // fail loudly, independent of the gate, if a future change ever made
    // this group's scrollbar resize-prone too) before the expected-fail
    // scroll assertion ends the body.
    //
    // RED-verification (#987): with this label's `KNOWN_BUGS` entry
    // removed, this test fails on the *second* assertion only --
    // `top_line_still_painted` was `true` ("rightline0_right_gtk"/
    // "rightline0_right_tui" stayed painted) while `target_resized`/
    // `other_resized` were both already `false` -- i.e. only the inert
    // half of the report reproduces here, the resize half does not. That
    // is the "which of the two assertions fails" distinction this issue's
    // acceptance bar asks for, and it is *different* from deliverable 1's
    // "both fail" -- exactly the "hit-test ordering vs. a missing hit zone
    // entirely" distinction a fix author needs (#987).
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

/// #1061 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 9): the editor
/// scrollbar's click-vs-drag decision was hand-rolled **twice** — once in
/// `tui_main/mouse.rs` (TUI's own v/h scrollbar arms), once in `app.rs`
/// (GTK's v/h scrollbar arms) — the exact "one rung shared, the other
/// hand-rolled and free to drift" shape #987 itself was before it reproduced
/// a user-visible bug in this same neighbourhood. Both hand-rolled copies
/// now call `render::resolve_editor_scrollbar_click` (see that function's
/// own doc for the shared three-way page-back/page-forward/begin-drag
/// decision) instead of re-deriving it — TUI additionally had a *fifth*
/// stand-alone re-derivation of the same thumb math
/// (`scrollbar_grab_offset`), deleted outright rather than routed through
/// the shared function, since its only job (computing `grab_offset`) is
/// now the shared function's own job too.
///
/// This is a structural convergence fix, not a bug fix: neither hand-rolled
/// copy had a known bug (unlike #987's own scrollbar-inert/click-resizes
/// report), so this scenario is not expected to go red against pre-#1061
/// `develop` — what it proves is that the *architecture* converged. Per the
/// issue's own "Proving it actually converged" section: `#1043`'s
/// `tui_prod` arm is the only harness lens that actually drives
/// `TuiShellApp`/`mouse.rs` — `issue_987_group_scrollbar_inert_and_click_
/// resizes`'s own `tui` arm (immediately above) wraps the *shared* `App`
/// instead (see that module's own "no `tui_prod` coverage" note), so it
/// could never have caught `mouse.rs`'s copy drifting from `app.rs`'s. This
/// module is that missing coverage.
///
/// # Why not `drag_group_scrollbar_column`
///
/// That helper (used by `issue_987_...`'s `gtk`/`tui` arms) locates a
/// window's own painted rect via `ConformanceHarness::screen_layout` —
/// unavailable on `tui_prod`, whose harness has no live `App` to clone it
/// from (see `conformance_harness_prod`'s own doc, "`ConformanceHarness::
/// engine`/`::screen_layout` are not live here"). This scenario instead
/// uses a single, unsplit window filling the whole terminal (no sidebar, no
/// minimap) so the window's own right edge — where `quadraui::Editor::
/// layout_with_options` always reserves the vertical scrollbar's one-cell
/// column, on both backends — coincides with the *terminal's* own right
/// edge, a structurally known quantity (`width - 1`) rather than a magic
/// number, mirroring the reasoning `drag_group_scrollbar_column`'s own doc
/// gives for its `target.rect.x + target.rect.width - 1.0`. The row is
/// located via `driver.find` against the buffer's own first painted line —
/// never a hardcoded coordinate.
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

// ── #1429: port the TUI-only mouse features into the shared drag/click
// routes (explorer drag-and-drop, hover-popup press-and-drag selection,
// tree empty-space right-click) ─────────────────────────────────────────
//
// All three used to live only in `src/tui_main/mouse.rs`/`shell_app.rs`
// (`tui_prod`-only, by construction) — `GOALS.md`/`IRREDUCIBLE_SURFACE`
// §7 named this as a feature gap that would have been silently dropped
// the day TUI cuts over onto the shared `App`. `render::
// apply_explorer_drag_move`/`apply_explorer_drop`/
// `route_tree_empty_space_context_menu` are the shared routes both
// backends now call; each scenario below is registered on `gtk`/`tui`
// (proving `App` gained the behaviour) and `tui_prod` (proving the
// shipped TUI shell still has it, unregressed).
//
// RED-verification: with `render::apply_explorer_drag_move`/
// `apply_explorer_drop` deleted and `App::handle_mouse_drag_msg`'s
// `explorer_dnd_active` forced back to `false` (this issue's own
// `git diff`, reverted), `explorer_drag_and_drop_...::gtk`/`::tui` fail —
// dropping the file row on the folder row never opens the move-confirm
// dialog. With `route_tree_empty_space_context_menu` deleted and its two
// call sites removed, `explorer_right_click_below_last_row_...::gtk`/
// `::tui`/`::tui_prod` fail — see this module's own RED-verification note
// for the exact repro. The hover-popup scenario was already green on
// every arm before this issue (`App` had carried `hover_popup_selecting`/
// the `HoverPopupSelection` drag arm since #785) — it is registered here
// as the pin `GOALS.md` calls for, not a bug fix.
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

    // `tui_prod`-only via the macro: `confirm_move_file`'s Yes/No dialog
    // has no table/input, so `quadraui::native_dialog_options` resolves it
    // to a *native* message dialog wherever `App` is the shell — that's
    // `render_content`'s own `native_dialog_shown` edge-trigger rung
    // (#727), shared code with no backend check at all, so it fires for
    // `gtk` *and* `tui` (both wrap `App`) alike, not just `gtk`. Only
    // `tui_prod` (`TuiShellApp`, its own independent paint path, never
    // touches that rung) actually composes the dialog in-canvas, so only
    // that arm gets `screen_has("Confirm Move")` — `gtk` gets the
    // `native_dialog_shown`/`pending_native_dialog` edge-trigger hand-
    // written twin below instead, mirroring `context_menu_delete_opens_
    // confirm_dialog`'s own `gtk`/`tui_prod` split.
    //
    // No `tui` (App-on-`TuiBackend`) driver twin: unlike `gtk`, its
    // `crate::tui_main::testing::conformance_harness` has no bespoke
    // `Harness`-with-cloned-`Rc`-fields constructor the way `crate::gtk::
    // testing::harness` does (`explorer_drag_and_drop_opens_confirm_move_
    // dialog_gtk`, below) — `ConformanceHarness`'s own `driver.app()`
    // returns an opaque `&impl AppLogic` on every backend, with no way to
    // reach `native_dialog_shown`/`pending_native_dialog` through it. The
    // `render.rs` unit test `explorer_drag_move_then_drop_opens_confirm_
    // move_dialog` already covers the exact same shared `render::
    // apply_explorer_drag_move`/`apply_explorer_drop` calls `App`'s `tui`
    // arm would otherwise re-prove backend-neutrally, so this is not an
    // uncovered gap, only a driver-level one.
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

    /// A one-character buffer (`z`) so a post-copy `P` (paste-before) has
    /// an unambiguous, single-row target, plus a focused hover popup over
    /// six short, distinct words — short enough that the popup's
    /// content-driven width (#1429's own risk: a wrapped line would break
    /// a `drag_text` needle across two painted rows) never has to wrap in
    /// either backend's 800-wide fixture.
    fn engine_with_hover_popup_drag_fixture() -> crate::core::Engine {
        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        let buf = engine.active_buffer_id();
        if let Some(st) = engine.buffer_manager.get_mut(buf) {
            st.buffer.content = ropey::Rope::from_str("z\n");
        }
        // Hermetic in-memory clipboard -- mirrors `setup_gtk_clipboard`'s
        // shape without touching the real desktop clipboard.
        // `tui_prod`'s own `TuiShellApp::from_engine` installs its
        // equally-hermetic thread-local stand-in over this (see
        // `tui_main::setup_tui_clipboard`'s `#[cfg(test)]` twin) -- either
        // way the write-then-read round trip this scenario drives through
        // Ctrl+C then `P` stays in-process.
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

    // Registered on `gtk`/`tui` (proving `App` carries the drag
    // follow-through — it has since #785, this pins it) and `tui_prod`
    // (the original `hover_selecting` owner). Drives the *drag itself*,
    // not just the press: a zero-length selection from the press alone
    // would still paste something, but never "gamma"/"delta" — the words
    // strictly between the drag's start (row 0, "alpha beta gamma") and
    // end (row 1, "delta epsilon zulu").
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

    // Registered on all three arms: `route_tree_empty_space_context_menu`
    // is the one genuinely shared stopgap (Files list: `src/render.rs`)
    // for the still-open quadraui#1045 item 4 — see that function's own
    // doc for the deletion plan once it ships.
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
            // `app-shell:sidebar-content` (the quadraui `AppShell` chrome
            // zone) is only registered by the shared `App` — `tui_prod`'s
            // `TuiShellApp` doesn't compose through `AppShell` at all (its
            // own doc: "the independently hand-written shell"), so the
            // empty-space point is instead derived from the last painted
            // row's own bounds, which every arm paints identically: several
            // row-heights straight down from it, same column. Never a
            // hardcoded coordinate.
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

// ── #986: v0.11.0 bug suite -- oracle-backed `:s///c` confirm-prompt spec
// (#801 Phase 2 never built) ────────────────────────────────────────────
//
// #801 deliberately never built the `:s///c` confirm loop: `execute.rs`'s
// `flags.contains('c')` check always errored loudly ("E-vimcode: the :s 'c'
// (confirm) flag is not implemented") instead of entering it. #1031 (#801
// Phase 2) replaced that error branch with a real confirm loop --
// `Engine::confirm_sub` / `handle_confirm_sub_key` in `execute.rs`.
// `tests/nvim_conformance.rs`'s `"sub:c ..."` cases
// cover the buffer/cursor half of the contract (oracle-compared against a
// real `nvim --headless`), but that harness only ever compares buffer text
// + cursor position -- it cannot tell "the engine silently didn't implement
// this" from "the prompt never painted", and per CLAUDE.md rule 1 (assert on
// rendered output, never on state alone -- the #587/#592 failure mode) a
// state-only assertion here would be worthless anyway, since there is no
// confirm *state* to assert on yet. The gap that genuinely needs a
// driver/paint-tier check instead lives here: is the confirm prompt
// actually *painted* on the command line (not just held in some field)?
// See `confirm_prompt_text_is_painted` below. (A second rendering gap --
// is the pending match itself visually highlighted, not just the prompt
// text -- was attempted and deliberately left uncovered; see this module's
// own doc further down for why, mirroring this issue's honest treatment of
// `^E`/`^Y`.)
//
// Plus one non-rendering gap that's still easiest to prove with a live
// driver rather than by hand-deriving the 'report' option's interaction
// with skipped matches: does the post-substitute report line count only
// *actual* replacements, excluding matches answered 'n'? See
// `confirm_report_line_excludes_skipped_matches` below.
//
// #986 itself shipped no implementation -- every scenario here was
// `known_bug_gate`-wrapped and listed in `KNOWN_BUGS`, so the suite stayed
// green while `execute.rs` still errored loudly. #1031 (#801 Phase 2) is
// the fix: `run_substitute` now enters a real confirm loop (`Engine::
// confirm_sub` + `handle_confirm_sub_key` in `execute.rs`) and both entries
// are gone from `KNOWN_BUGS` -- `known_bug_gate` would fail the build on a
// listed-but-passing scenario otherwise.
#[cfg(test)]
mod issue_986_confirm_prompt_never_built {
    use super::*;

    /// The exact prompt Neovim v0.12.5 paints for `:%s/zqxw986abc/zqxw986def/gc`
    /// -- captured verbatim from a real `nvim --headless` run (this issue's
    /// own "do not invent the UI contract -- derive it from Neovim"
    /// instruction), via `nvim_buf_set_lines`/`nvim_feedkeys` driving a real
    /// `:%s` and reading nvim's own stdout. Longer and more spelled-out than
    /// `:help :s_c`'s classic-Vim `(y/n/a/q/l/^E/^Y)?` shorthand -- this is
    /// Neovim's own wording, not a harness artifact: reproduced identically
    /// across a dozen separate probes (varying the fixture, the replacement
    /// text, and the answer sequence), none of them a window/scroll-relative
    /// read -- not the #805 headless-oracle-artifact class of case.
    ///
    /// Full captured confirm contract (see this issue's PR description for
    /// the complete table): `y` replaces-and-continues, `n` skips-and-
    /// continues, `a` replaces this-and-all-remaining, `q`/`<Esc>` quit
    /// without replacing the pending match, `l` replaces-the-pending-match-
    /// then-quits ("last"). `^E`/`^Y` (scroll the window while the prompt is
    /// up) do not hang a headless oracle -- confirmed by feeding them
    /// directly -- but produce no buffer/cursor difference for
    /// `tests/nvim_conformance.rs`'s comparison to observe (scroll position
    /// isn't part of what that harness compares), so they are the two rows
    /// left uncovered, honestly, per this issue's own allowance.
    const NVIM_CONFIRM_PROMPT: &str =
        "replace with zqxw986def? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)";

    /// Three matches across three lines, all sharing one grep-safe,
    /// screen-collision-safe token (`zqxw986abc`, mirroring #983's
    /// `zqxw983...` convention) so `screen_has`/`find_bounds` below can
    /// never accidentally match unrelated painted chrome.
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

    // ── Deliverable 2, item 1: the prompt itself is painted (both backends,
    // via the shared macro -- `ConformanceDriver::screen_has` is portable) ──
    //
    // RED-verification: with the `confirm_prompt_text_is_painted` entry
    // removed from `KNOWN_BUGS`, both
    // `issue_986_confirm_prompt_never_built::confirm_prompt_text_is_painted::gtk`
    // (`cargo test --features gui --lib`) and its `::tui` twin
    // (`cargo test --no-default-features --lib`) fail on the final
    // assertion: today `:%s/zqxw986abc/zqxw986def/gc<CR>` hits the
    // `flags.contains('c')` early return and paints the "E-vimcode: ... not
    // implemented" message instead, so `screen_has(NVIM_CONFIRM_PROMPT)` is
    // false on both. Restored (entry back in place) and confirmed green
    // again on both.
    //
    // #1043 adds `tui_prod`, still under the same un-suffixed
    // `KNOWN_BUGS` label: `execute.rs`'s `flags.contains('c')` early return
    // is core code every shell (`App` and `TuiShellApp` alike) calls
    // through the same `Engine::execute_command` path, so this bug (and
    // its eventual fix) is identical on all three arms — see this module's
    // own top doc on why the label is shared rather than per-backend here.
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

    // ── Deliverable "report line" item: skipped ('n') matches must not
    // count toward the post-substitute "N substitutions on M lines" report
    // (both backends, shared macro) ──────────────────────────────────────
    //
    // RED-verification: same procedure as above, against
    // `confirm_report_line_excludes_skipped_matches::gtk`/`::tui` -- with
    // its `KNOWN_BUGS` entry removed, both fail on the final assertion (no
    // confirm loop exists yet, so nothing ever paints a report line at
    // all, let alone the correctly-counted one). Restored and confirmed
    // green again on both.
    //
    // #1043 adds `tui_prod`, same shared-label rationale as
    // `confirm_prompt_text_is_painted` above.
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

    // ── Deliverable 2, item 2: the pending match is visually highlighted --
    // an honest gap, NOT covered by an automated test (see below) ────────
    //
    // This issue's own instruction (#252/CLAUDE.md rule 1: assert on
    // rendered output, never on state alone) calls for pinning that the
    // pending match is visually distinguished, not just that a plain
    // prompt string appears. Two attempts were made and both had to be
    // discarded as unusable, not merely inconvenient:
    //
    //   1. Same cell's `style_at`/`pixel`, sampled before vs. after the
    //      confirm prompt opens: measured a real but entirely unrelated
    //      global repaint between the harness's first frame and the first
    //      frame after any ex-command round-trips through the engine (an
    //      unrelated corner of the screen shifted color identically, with
    //      zero relation to `:s` or `c`), so the assertion passed today,
    //      for the wrong reason, against unfixed `develop` -- exactly the
    //      false positive CLAUDE.md's "state that the new test was
    //      observed RED against unfixed develop" rule exists to catch.
    //   2. Two different matches' cells sampled from the *same* frame
    //      (the pending match vs. one not yet reached), meant to sidestep
    //      attempt 1's confound: still measurably different colors at a
    //      1400x900 driver size with **zero keys pressed at all** (a pure
    //      construction-time baseline), so whatever painted the
    //      difference predates any interaction with this feature too.
    //      Shrinking the driver to a realistic 120x30 made the fixture
    //      buffer stop painting altogether (the default sidebar/explorer
    //      `Engine::new_for_test()` starts with consumes the whole width
    //      at that size), so there was no size at which this approach
    //      produced a signal traceable to the confirm prompt specifically.
    //
    // Building a confound-free per-cell probe would mean first learning
    // exactly how a real confirm-loop implementation paints the highlight
    // (cursor-line color? a dedicated match style? something else?) --
    // knowledge that does not exist yet, since no implementation exists
    // (this issue is test-only, no implementation lands here). Rather than
    // ship a test that either passes for the wrong reason or asserts a
    // guessed rendering mechanism this issue was explicitly told not to
    // invent ("do not invent the UI contract"), this half of deliverable 2
    // is left as a stated, honest gap -- the same treatment this issue
    // gives `^E`/`^Y` below. [`confirm_prompt_text_is_painted`] above still
    // satisfies the deliverable's core, confound-free half: the prompt
    // *text* itself is asserted as painted output, not engine state. A
    // future confirm-loop implementation issue should add the highlight
    // assertion once its actual rendering mechanism is known.
}

/// #1053 (GOALS.md's 2026-09-16 audit, #1044, wave 1 item 1): `mouse.rs`
/// used to carry a ~55-line hand-rolled `ActivityBarTarget` dispatch block
/// (resolving `resolve_activity_bar_click`, including a `MenuToggle` arm)
/// that #988's own "likely shape" guess pointed at as the bug site, wrongly
/// -- it was confirmed-unreachable for a genuine single click: `AppShell`
/// (`TuiShellApp::shell_config` registers every activity-bar item,
/// including the hamburger, as a real `PanelDefinition`) consumes the click
/// into a semantic `AppShellEvent` upstream of `TuiShellApp::handle` ->
/// `mouse::handle_mouse` entirely. Deleted.
///
/// `tui_prod`-only (not `backend_conformance!`'s usual `[gtk, tui,
/// tui_prod]`): the dead block, and the deletion, are both specific to
/// `src/tui_main/mouse.rs` -- there is no GTK or shared-`App` counterpart to
/// register a twin against (GTK never had an equivalent hand-rolled
/// dispatch; `App`'s own activity-bar handling is a different, already-
/// shared rung -- see `GOALS.md`'s audit table).
///
/// This is the systematic convergence-audit registration the issue's own
/// "Proving it actually converged" section asks for, alongside (not instead
/// of) the more thorough in-crate black-box coverage in
/// `shell_app.rs`'s `driver_click_on_every_activity_bar_icon_opens_its_
/// panel_via_shell_app` (all 6 fixed panels, Settings, and the hamburger,
/// via `TuiShellApp::new_for_test` + real single clicks). This scenario
/// samples two of those targets through the independent `conformance_
/// harness_prod` lens instead of repeating all of them: an ordinary fixed
/// panel (Search) and the specific arm #988 named (the hamburger).
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

/// #1057 (GOALS.md's 2026-09-16 audit, #1044, wave 1 item 5): a bottom
/// activity-bar item (`shell_config`'s only one today, "bottom:settings")
/// drifted between backends on a second click of the item it had *already*
/// opened. `TuiShellApp::on_shell_event`'s `BottomItemClicked` arm ran
/// `Engine::toggle_sidebar_panel`, collapsing the sidebar on a second click
/// -- `App::on_shell_event`'s own arm just called
/// `app_shell.show_panel(id)` unconditionally, so a second Settings click
/// there re-showed the panel it was already showing instead of collapsing
/// it.
///
/// Checked against VS Code before picking a side, per the issue's own
/// instruction not to default to "toggle" without checking: VS Code does
/// collapse the Panel when its already-active tab is clicked again -- there
/// is no VS Code bottom item that stays permanently open once toggled on.
/// So TUI's existing behaviour was the one to keep, and GTK's was the
/// drift.
///
/// Converged both backends onto `render::apply_activity_panel_switch`, the
/// same shared toggle-decision function `App::switch_panel` and
/// `TuiShellApp::activate_ext_panel` already called for the ext-panel case
/// -- see `src/app.rs`'s `BottomItemClicked` arm (now `self.switch_panel
/// (id.as_str().to_string())`) and `src/tui_main/shell_app.rs`'s, same arm
/// (now `render::apply_activity_panel_switch(&mut self.engine, ...)`), for
/// the actual fix. Neither backend hand-rolls the toggle decision anymore.
///
/// Registered on `gtk`, `tui` *and* `tui_prod`: the fix touches
/// `src/app.rs` (shared by both the `gtk` arm, through `run_with_shell`,
/// and the `tui` arm, which wraps the same `App`) and
/// `src/tui_main/shell_app.rs` (`tui_prod` only) -- this is the scenario
/// that proves the two independent call sites actually converged on the
/// same decision, per the issue's "Proving it actually converged" section,
/// rather than each merely doing *something* plausible in isolation.
///
/// Verified RED against unfixed `develop`: reverting `src/app.rs`'s
/// `BottomItemClicked` arm to its pre-#1057 body (`self.engine.borrow_mut()
/// .app_shell.show_panel(id); self.draw_needed.set(true);`) turns the `gtk`
/// and `tui` arms red -- the second click leaves "SETTINGS" painted instead
/// of collapsing the sidebar -- while `tui_prod` stays green, since TUI's
/// own arm was never broken. Restored after confirming red.
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

/// #1062 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 10): the third of
/// the three `AppShellEvent` shadow-`engine.app_shell` sync arms this issue
/// converges -- `PanelChanged`/`SidebarHidden`/`SidebarResized` -- onto one
/// shared function, [`render::sync_shell_event_shadow`]. #988 was one of
/// these three forgetting the sync entirely: `PanelChanged { hamburger }`
/// returned early, before any shadow-sync statement ran, because the sync
/// used to be spelled out fresh at each call site instead of owned by one
/// function every call site is required to reach. That specific hamburger
/// scenario already has its own `tui_prod` coverage
/// (`issue_1053_dead_activity_bar_block`, above); this scenario covers the
/// *other* two arms, `PanelChanged`/`SidebarHidden` for an ordinary panel,
/// on all three backends.
///
/// This is a structural-convergence scenario, not a bug reproduction --
/// the closest sibling in this wave is #1059's tab-bar-dispatch rung, whose
/// own doc makes the same call: both `App::on_shell_event` and
/// `TuiShellApp::on_shell_event` already produced the *same* observable
/// result for a real (non-hamburger, non-`ext:`) panel's open/close pair
/// before this issue -- what was duplicated was the sync statements
/// themselves, not the behaviour they produced. So this is not expected to
/// go red against pre-#1062 `develop`; what it proves, registered on `gtk`,
/// `tui` **and** `tui_prod`, is that all three arms still agree *after*
/// being collapsed onto the one shared function -- per the issue's own
/// "Proving it actually converged" section, the `tui_prod` arm is the only
/// one that could have caught `TuiShellApp::on_shell_event`'s copy
/// drifting from `App`'s during the convergence, since it is the only arm
/// that drives the shipped `TuiShellApp` rather than the shared `App`.
///
/// Exercises the Search panel's activity-bar icon: one click opens it
/// (`PanelChanged`), a second click on the now-open icon closes it
/// (`SidebarHidden`) -- covering two of this issue's three converged arms
/// end to end through painted output. The third, `SidebarResized`, has no
/// rendered proxy on TUI to assert on: `render::sync_shell_event_shadow`'s
/// `SidebarResized` arm now pushes the drag-settled width into the shadow
/// `engine.app_shell` on TUI too (previously nothing did -- nothing reads
/// that copy's width back on TUI, `TuiShellApp::sidebar_width` is the
/// separate field its own column math actually reads), so there is no
/// painted difference to assert on without asserting on state directly,
/// which this repo's own testing rule (`CLAUDE.md`, "Rendered output, not
/// state") forbids.
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
            // Two genuine, independent, fully-released clicks on the same
            // icon -- `drag_text`, not `click_text`, and double-click
            // folding disabled, for the same reason
            // `bottom_item_second_click_collapses_sidebar` (#1057, above)
            // needs both: a bare down-only click leaves the simulated
            // button latched for the second press, and two real clicks
            // close together in simulated time would otherwise fold into a
            // `DoubleClick`, which bypasses `ShellAdapter`'s semantic
            // `AppShellEvent` dispatch -- and so this scenario's own
            // subject -- entirely.
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

/// #1063 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 11): GTK's
/// `App::handle_menu_action` used to restate a *subset* of its own
/// general-purpose `EngineAction` applier (`App::dispatch_engine_action`) by
/// hand -- five variants named explicitly behind a bare `_ => {}` catch-all
/// -- instead of calling it; TUI's menu arm already called its own
/// general-purpose applier (`dispatch_post_key_action`), but that was a
/// *different* function from GTK's, so the two could still drift
/// independently. Both now call one shared function,
/// `render::apply_engine_action`, via a `render::EngineActionHost` per
/// backend (`GtkEngineActionHost` in `app.rs`, `TuiEngineActionHost` in
/// `tui_main/shell_app.rs`) -- see that function's rung header comment in
/// `render.rs` for the full "why".
///
/// This is a structural-convergence scenario, not a bug reproduction -- the
/// closest sibling in this wave is #1062's shadow-sync rung, whose own doc
/// makes the same call. Every menu item reachable in `MENU_STRUCTURE` today
/// that could reach GTK's old catch-all already had an explicit arm there
/// (`Quit`/`SaveQuit`/`QuitWithUnsaved`/`ToggleSidebar`/`OpenTerminal`) --
/// the catch-all was a latent trap for a *future* menu item, not a live
/// #984-shaped bug, so this is not expected to go red against pre-#1063
/// `develop`. What it proves, registered on `tui` **and** `tui_prod`
/// (see the next doc comment for why there's no `gtk` arm here), is that
/// both still open a terminal pane via Terminal &#9656; New Terminal
/// *after* being collapsed onto the one shared applier -- per #1063's own
/// "Proving it actually converged" section, `tui_prod` is the only arm
/// that could have caught `TuiShellApp`'s own `dispatch_post_key_action`
/// drifting from the shared function during the convergence, since it is
/// the only arm that drives the shipped `TuiShellApp` rather than the
/// shared `App`; `tui` (wrapping `App`, the same shell `gtk` wraps) is
/// what could have caught `GtkEngineActionHost` itself double-borrowing
/// `Engine` or otherwise regressing GTK's menu path (see that struct's own
/// doc, `app.rs`, for the double-borrow hazard this scenario's mere
/// passing rules out).
///
/// TUI-only (`tui` + `tui_prod`, no `gtk` arm): reveals the menu bar via
/// Alt+T, the same #318 shim `alt_letter_reveals_menu_bar_via_shell_app`
/// (`tui_main/shell_app.rs`) exercises, then activates "New Terminal" with
/// Enter -- a raw modifier-carrying `UiEvent::KeyPressed`, which is
/// `TuiDriver`'s own inherent `dispatch`/`press_named`, not part of the
/// cross-backend `ConformanceDriver` bound (`type_char`/`type_text`/
/// `press_named`/`screen_has`/`inventory` only -- see this module's own
/// "Which trait bound a scenario needs" doc), so this can't be a `gtk` arm
/// of [`backend_conformance!`] without a click-based redesign; the shared
/// generic parameter (`T: quadraui::AppLogic`) still lets the same body run
/// against both TUI shells, which is what this issue's convergence proof
/// needs.
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

    // #1043's own `tui`/`tui_prod` pair shape (see e.g.
    // `explorer_chevron_click_toggles_dir_with_same_arity_as_label_click_tui`/
    // `_tui_prod`, above): `tui` is the control (`App`, the shell `gtk` also
    // wraps -- `GtkEngineActionHost`'s code runs here even though this
    // arm never touches real GTK), `tui_prod` is the shipped `TuiShellApp`.
    // Structural-convergence proof, not a bug reproduction -- see this
    // module's own doc above for why it's not expected to go red against
    // pre-#1063 `develop`.
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

/// #1059 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 7): `tui_main::mouse`
/// hand-rolled the tab-bar-row dispatch match **twice** (once for a split
/// group's tab bar, once for a single group's) instead of calling the shared
/// `click::dispatch_tab_bar_target` GTK has called since #814 -- the same
/// "one rung routed through the shared function, the other hand-rolled, free
/// to drift" shape #1025 was before a user hit it. Both hand-rolled copies
/// are now deleted: `src/tui_main/mouse.rs`'s two tab-bar-row arms call
/// `click::dispatch_tab_bar_target` directly and only handle its two
/// deliberately-deferred exceptions (`ActionMenuButton`'s popup placement,
/// `CloseTab`'s confirm-or-close decision) themselves -- exactly as
/// `src/click.rs::handle_mouse_click` (GTK's own caller of the same
/// function) already did.
///
/// This is a structural convergence fix, not a bug fix: both hand-rolled
/// copies already produced the same tab-switch/tab-close behaviour the
/// shared function produces (confirmed by reading both match arms side by
/// side against `dispatch_tab_bar_target`'s body -- there is no reported
/// user-visible bug here to reproduce), so this scenario is not expected to
/// go red against pre-#1059 `develop`; what it proves is that the
/// *architecture* actually converged -- the #1043 `tui_prod` arm is the only
/// one that drives `TuiShellApp`/`mouse.rs` rather than the shared `App`, so
/// it's the only arm that could ever have caught the two deleted copies
/// drifting apart from each other or from GTK, per the issue's "Proving it
/// actually converged" section.
///
/// One behavioural difference *is* deliberate and left uncovered here: the
/// old `ActionMenu` arms set `engine.active_group` before opening the popup;
/// `dispatch_tab_bar_target` doesn't (`src/click.rs`'s own GTK caller never
/// did either), so TUI no longer does either. That only affects a
/// split-view edge case (which group's tab bar highlights as active while a
/// *different* group's action-menu popup is open), and aligns TUI with the
/// behaviour GTK already shipped -- not exercised here since it's a
/// highlight-only cosmetic edge case (there is no rendered proxy for "which
/// group is `engine.active_group`" that isn't itself state, per this repo's
/// own "assert on rendered output, never on state" testing rule) and adds no
/// coverage of the actual dispatch rung; worth a tracking issue if it ever
/// needs to be locked down.
///
/// The split-group tab-bar arm itself -- the *other* rung this issue
/// converged, `mouse.rs`'s `if let Some(ref split) = layout.editor_group_split`
/// branch -- **is** exercised, by the `split_two_group_fixture`-based
/// scenarios below: every scenario above builds from `two_tab_fixture()`,
/// which has a single editor group, so `editor_group_split` is always `None`
/// for them and only the single-group arm ever runs.
///
/// Registered on `gtk`, `tui` **and** `tui_prod` for the Tab-switch scenario
/// below (`gtk`/`tui` both wrap the shared `App`, which already routed
/// through `click::dispatch_tab_bar_target` before this issue -- they're the
/// control, expected green before and after; `tui_prod` wraps the real
/// `TuiShellApp`/`mouse.rs` this issue's fix touches, so it's the arm that
/// actually exercises the deleted hand-rolled copies' replacement). The
/// CloseTab scenario drops the `tui` arm -- see its own doc comment for an
/// unrelated pre-existing gap that scenario's development surfaced.
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
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1059_tab_bar_dispatch_{:?}",
            std::thread::current().id()
        ));
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
        let inventory = driver.inventory();
        let runs = inventory.text_runs();
        let close = runs
            .iter()
            .filter(|r| r.text.contains(label_needle))
            .find_map(|label| {
                runs.iter().find(|r| {
                    r.text.contains('\u{00d7}')
                        && r.bounds.y == label.bounds.y
                        && r.bounds.x > label.bounds.x
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "{label_needle:?}'s tab label and its close button must both be \
                     painted, on the same row, close button to the right"
                )
            })
            .bounds;
        (close.x + close.width / 2.0, close.y + close.height / 2.0)
    }

    // ── Tab arm ──────────────────────────────────────────────────────────
    // Click the *inactive* tab's label. Resolves to `TabBarClickTarget::Tab`,
    // applied via one `Engine::handle_tab_bar_click` call inside
    // `click::dispatch_tab_bar_target` -- the same call #752's comment
    // (right above the single-group arm in `mouse.rs`) says the hand-rolled
    // copy it replaced used to skip for an unsplit window, silently leaving
    // the LSP pointed at the previously-active buffer.
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

    // ── CloseTab arm ───────────────────────────────────────────────────────
    // Click the *inactive* tab's (a1059.txt) close button directly. Resolves
    // to `ClickTarget::CloseTab`, which `dispatch_tab_bar_target`
    // deliberately leaves unapplied for the caller -- `mouse.rs`'s own arm
    // (mirroring `click::handle_mouse_click`'s) makes the one
    // `Engine::handle_tab_bar_click` call that decides confirm vs. close.
    //
    // `tui` here (only `gtk` and `tui_prod` were registered before #1425):
    // while developing this scenario, a click at the close button's own
    // painted center resolved as a plain tab-select instead of a close
    // specifically on `tui` -- `App` driven by `quadraui::tui::TuiBackend`,
    // i.e. quadraui's own generic ratatui `TabBar` widget rendering, as
    // opposed to `tui_main::render_impl`'s independent hand-written
    // rasteriser (`tui_prod`) or GTK's pixel-precise `tab_pixel_hits` cache
    // (`gtk`) -- both of which resolved the same click correctly. That is a
    // paint/hit-test disagreement inside quadraui's own TUI backend
    // rendering of a primitive neither of this issue's two files
    // (`mouse.rs`, `click.rs`) builds or interprets, so it's out of scope
    // here; `App`+`TuiBackend` is also never what `tui_main::run` actually
    // ships (`tui_prod` is), so no real user is affected by it.
    //
    // #1425 adds the `tui` arm as a `KNOWN_BUGS`-gated inventory entry
    // (`tab_bar_click_closes_via_shared_dispatch::tui`, category
    // **quadraui** -- the shared ratatui `TabBar` widget's own hit-test
    // conflates the close glyph with the label, a gap in the primitive
    // itself, not in this crate's dispatch) rather than leaving the gap
    // silently un-exercised, per that issue's "gap inventory as executable
    // tests, not a report" mandate. RED-verified: with the `KNOWN_BUGS`
    // entry removed, this arm fails on the final assertion -- the click
    // switches to `a1059` instead of closing it (`"a1059"` stays painted,
    // `"BBBB_1059_CONTENT"` does not reappear), exactly the resolved-as-
    // tab-select symptom this comment already named by hand.
    crate::backend_conformance! {
        label: tab_bar_click_closes_via_shared_dispatch,
        backends: [gtk, tui_prod],
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

    #[test]
    fn tab_bar_click_closes_via_shared_dispatch_tui() {
        let mut __h = crate::tui_main::testing::conformance_harness(two_tab_fixture(), 800, 480);
        let driver = &mut __h.driver;
        crate::harness::known_bug_gate("tab_bar_click_closes_via_shared_dispatch::tui", || {
            assert!(
                driver.screen_has("b1059") && driver.screen_has("a1059"),
                "precondition: both tabs are painted"
            );

            let (cx, cy) = tab_close_button_center(driver, "a1059");
            driver.drag(cx, cy, cx, cy);
            assert!(
                !driver.screen_has("a1059") && driver.screen_has("BBBB_1059_CONTENT"),
                "clicking a1059.txt's close button must close it and fall \
                 back to the only remaining tab, b1059.txt, on every backend"
            );
        });
    }

    // ── Split-group coverage (review follow-up) ─────────────────────────
    //
    // Every scenario above builds its fixture from `two_tab_fixture()`,
    // which has exactly one editor group. `render::build_screen_layout`
    // only produces `Some(editor_group_split)` once `n >= 2` groups exist
    // (`editor_group_split = (n >= 2).then_some(...)`), so with a single
    // group `mouse.rs`'s `if let Some(ref split) = layout.editor_group_split`
    // branch -- the *split*-group tab-bar arm, the other rung #1059 routed
    // through `click::dispatch_tab_bar_target` -- is never entered; only the
    // single-group arm below it runs. That left the split-group rung
    // "asserted, not verified": a future typo, wrong `idx`, or a hand-rolled
    // shortcut reintroduced for that branch only would compile and pass every
    // test above without being caught, reproducing the exact #1025 "one rung
    // shared, one hand-rolled, free to drift" shape this issue exists to
    // close -- for the one rung this issue's own fix touches that had no
    // execution coverage at all.
    //
    // `split_two_group_fixture` below gives `editor_group_split` a genuine
    // `Some` by opening a second editor group, so a click aimed at the
    // *non-active* (left) group's tab bar row is hit-tested and dispatched by
    // the split branch specifically.
    fn split_two_group_fixture() -> crate::core::Engine {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_1059_split_tab_bar_dispatch_{:?}",
            std::thread::current().id()
        ));
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

    // ── Split-group CloseTab arm ──────────────────────────────────────────
    // Click the *inactive* tab's (`left_a1059.txt`) close button directly, in
    // the non-active (left) group's tab bar. Resolves to
    // `ClickTarget::CloseTab`, which -- same as the single-group scenario
    // above -- `dispatch_tab_bar_target` deliberately leaves unapplied for
    // the caller; `mouse.rs`'s split-group arm makes the
    // `Engine::handle_tab_bar_click` call that decides confirm vs. close.
    //
    // `tui` (only `gtk`/`tui_prod` were registered before #1425): same
    // reason `tab_bar_click_closes_via_shared_dispatch` above didn't have
    // one either -- a close-button click resolves as a plain tab-select on
    // `tui` (`App` + `quadraui::tui::TuiBackend`'s own generic `TabBar`
    // widget hit-test), independent of which group the tab bar belongs to.
    // See that scenario's doc comment for the full call-out; the same
    // quadraui-side gap applies here unchanged, so #1425 gates this arm
    // under the same category (**quadraui**) with its own
    // `::tui`-suffixed `KNOWN_BUGS` label. RED-verified: with the entry
    // removed, this arm fails identically to the single-group twin --
    // `"left_a1059"` stays painted instead of closing.
    crate::backend_conformance! {
        label: split_tab_bar_click_closes_via_shared_dispatch,
        backends: [gtk, tui_prod],
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

    #[test]
    fn split_tab_bar_click_closes_via_shared_dispatch_tui() {
        let mut __h =
            crate::tui_main::testing::conformance_harness(split_two_group_fixture(), 1600, 480);
        let driver = &mut __h.driver;
        crate::harness::known_bug_gate(
            "split_tab_bar_click_closes_via_shared_dispatch::tui",
            || {
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
        );
    }
}

/// #1064 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 12):
/// `quadraui::ShellApp::take_requested_panel` was unoverridden on `App` —
/// the trait default always returns `None` — so `ShellAdapter::
/// apply_requested_panel` (polled once after every `handle()`/`tick()`
/// dispatch) never saw a switch to apply on GTK, no matter what the engine
/// did to its own `app_shell`/`ext_panel_active`.
///
/// The gap only shows up for an **app-initiated** panel switch — one the
/// engine makes on its own, with no runner click involved
/// (`Engine::process_pending_sidebar`'s DAP `dap_wants_sidebar` reveal, or
/// `App::toggle_focus_explorer`/`App::toggle_focus_search`'s keyboard
/// accelerators, are the production callers). `App::render_content` paints
/// the sidebar's *content* from `engine.app_shell`/`engine.ext_panel_active`
/// directly (the shadow), so that part always painted correctly. But the
/// runner's own chrome — the sidebar-header title `quadraui::AppShell::
/// render` paints from **its own**, entirely separate, `active_panel()` —
/// has no channel to learn about the switch other than a click hit-test or
/// this poll, so it silently kept showing the previous panel's title
/// forever. `screen_has("EXPLORER")` is a safe proxy for that title
/// specifically: that literal all-caps string appears nowhere in either
/// panel's own painted *content* (checked directly — `render.rs`/
/// `tui_main/panels.rs` have no such literal), only in the
/// `PanelDefinition::title` fields `Engine::new` seeds the shadow
/// `app_shell` with, which `quadraui::AppShell::render` echoes into the
/// header. `screen_has("SEARCH")` is *not* an equally clean proxy — the
/// Search panel's own content pane paints that literal too, via
/// `Engine::new`'s `search_sidebar_system`
/// (`SidebarSectionDef::form("chrome", "SEARCH")` at
/// `src/core/engine/mod.rs:3977`), which `quadraui::SidebarSystem::render`
/// paints as a section header regardless of runner-chrome convergence. The
/// assertion below is a conjunction (`screen_has("SEARCH") &&
/// !screen_has("EXPLORER")`), so it is the `!screen_has("EXPLORER")` half
/// that actually diagnoses the bug; `screen_has("SEARCH")` alone would pass
/// even with the header stuck on the wrong panel.
///
/// `gtk` and `tui` below both wrap the shared `crate::app::App` — `gtk` on
/// GTK, `tui` on `quadraui::tui::TuiBackend` (`crate::tui_main::testing::
/// conformance_harness`) — the same shape as `crate::gtk::testing::
/// conformance_harness`, just on a different `quadraui::Backend`.
/// `take_requested_panel` lives on `App` itself with no backend-conditional
/// code inside it, so both arms exercise the identical new override and
/// both are expected to flip together. `tui_prod`
/// (`crate::tui_main::testing::conformance_harness_prod`) is different in
/// kind: it wraps `TuiShellApp`, the independently hand-written production
/// TUI shell, which already had its own equivalent
/// `take_requested_panel`/`last_shell_panel`/`suppress_shell_panel_echo`
/// override *before* this issue — untouched by this fix — so it was
/// already green and stays green throughout. Its role here is not to prove
/// the new code red/green like `gtk`/`tui`; it's the "Proving it actually
/// converged" check from the issue: showing the newly-added `App` override
/// reconciles the same contract the shipped TUI shell already enforced,
/// rather than merely doing *something* plausible on `App` in isolation.
/// `tui_prod`'s own arm can't reach the trigger the `gtk`/`tui` arms use
/// below (a direct `engine.focus_sidebar_panel` call through
/// `ConformanceHarness::engine` — `conformance_harness_prod`'s own doc:
/// that field is a disconnected placeholder for this arm, since
/// `TuiShellApp` owns its `Engine` directly, not behind a shared `Rc`), so
/// it drives the identical reconciliation through the Search-focus panel
/// accelerator instead — see its own doc below for why that is a
/// genuinely equivalent trigger, not a weaker substitute.
///
/// Verified RED against unfixed `develop`: deleting `App`'s
/// `take_requested_panel` override (falling back to the trait default
/// `None`) and running this whole module — including `tui()`, not just
/// `gtk()` — turns **both** the `gtk` and `tui` arms red, exactly as
/// expected from them sharing `App`: the header stays on "EXPLORER"
/// forever after the direct `focus_sidebar_panel(PANEL_SEARCH)` call
/// below, in each arm. `tui_prod` stays green throughout, since
/// `TuiShellApp`'s own override was never touched. Restored after
/// confirming red.
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

        // Poke the runner so `ShellAdapter::handle`/`apply_requested_panel`
        // polls `take_requested_panel` again — the same "direct engine
        // mutation, then a dispatch to force the poll" shape
        // `TuiShellApp`'s own `take_requested_panel_reconciles_keyboard_
        // switch_once` unit test uses (`shell_app.rs`), applied here as a
        // black-box assertion on painted output instead of the
        // `Option<WidgetId>` `take_requested_panel` returns directly.
        // `dispatch` repaints on its own whenever the reaction is
        // `Redraw` (both `GtkDriver`/`TuiDriver`'s own doc), so no
        // separate `render()` call is needed here — and isn't available
        // through the `ConformanceDriver + DriverInput` bound anyway.
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
        // #1427: `App::shell_config`'s `cell`-profile hamburger
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

    /// `conformance_harness_prod` wraps the shipped `TuiShellApp`, which
    /// owns its `Engine` directly rather than behind a shared `Rc` — its
    /// `ConformanceHarness::engine` is a disconnected placeholder (see that
    /// constructor's own doc), so the direct-mutation trigger the `gtk`/
    /// `tui` arms above use has nothing live to reach on this arm.
    ///
    /// Dispatching the Search-focus panel accelerator instead reaches the
    /// identical code shape: `TuiAccelHost::focus_search`
    /// (`shell_app.rs`) calls `engine.toggle_sidebar_panel(PANEL_SEARCH)`
    /// directly on the shadow, synchronously inside this one
    /// `TuiShellApp::handle` dispatch — no runner click, exactly like the
    /// direct-mutation trigger above. (GTK's own accelerator host instead
    /// *defers* the equivalent call to `tick()` via `App::
    /// toggle_focus_search`'s `DeferredAction` queue — `GtkDriver`'s
    /// headless harness has no way to pump `tick()` at all, per its own
    /// module doc's "No main loop" limit, which is why the `gtk` arm above
    /// needs the direct-engine-mutation shape instead of this one.)
    #[test]
    fn tui_prod() {
        let mut h = crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 800, 480);
        let driver = &mut h.driver;

        // Unlike `App::new_headless_with_backend` (the `gtk`/`tui` arms'
        // own constructor), `TuiShellApp::from_engine` boots with the
        // sidebar hidden — Explorer is still the default *active* panel
        // (index 0), just not visible yet, so one real click reveals it
        // (`AppShell::handle_activity_click`'s "different panel, or
        // already-active-but-hidden" branch — see the shared function
        // above for the mirror-image case, an already-*visible* active
        // panel, which toggles closed instead).
        driver.click_text(crate::icons::EXPLORER.s());
        assert!(
            driver.screen_has("Explorer"),
            "precondition: clicking the Explorer icon must open the \
             sidebar on Explorer via the real runner click path"
        );

        driver.dispatch(quadraui::UiEvent::Accelerator(
            quadraui::AcceleratorId::new(crate::render::ACC_FOCUS_SEARCH),
            quadraui::Modifiers::default(),
        ));

        // Not `screen_has("Search")`: `TuiShellApp::shell_config`'s own
        // panel titles are title-case ("Explorer"/"Search"), unlike the
        // shared `App`'s all-caps `PanelDefinition`s — and the Search
        // panel's own *content* paints a "Search…" input placeholder
        // (`render.rs`) regardless of whether the runner's chrome caught
        // up, so a positive `screen_has("Search")` can't tell the two
        // apart here (it already passed before this issue's fix, since
        // the content pane was never the broken half). `!screen_has
        // ("Explorer")` is the half that's actually diagnostic: the
        // runner's stale chrome is the only remaining place "Explorer"
        // could still be painted once the shadow has moved to Search.
        assert!(
            !driver.screen_has("Explorer"),
            "#1064: an app-initiated panel switch (the Search-focus \
             accelerator, which moves only the shadow `engine.app_shell`, \
             not the runner's own chrome) must still steer the runner's \
             sidebar-header title off the previous panel — proving \
             TuiShellApp's own pre-existing `take_requested_panel` still \
             agrees with App's new one"
        );
    }
}

/// #1256/#1343: does GTK compose the Settings/Extensions sidebar header and
/// search chrome somewhere other than `App::paint_sidebar_panel_rung`
/// (whose `PANEL_SETTINGS`/`PANEL_EXTENSIONS` arms used to paint only the
/// body widget), or is it missing? Originally answered by driving all
/// three shells, each panel opened by a real click on its activity-bar
/// icon, which found:
///
/// | | `gtk` / `tui` (shared `App`) | `tui_prod` (`TuiShellApp`) |
/// |---|---|---|
/// | Extensions header | " EXTENSIONS " — quadraui `AppShell::render`'s sidebar-header row, titled from `PanelDefinition.title` (`core/engine/sidebar.rs`) | runner header "Extensions" **and** `render_ext_sidebar`'s own " EXTENSIONS" row (doubled) |
/// | Extensions search row | **none** | "Search extensions (press /)" |
/// | Settings header | " SETTINGS " — fixed by #1356's quadraui bump (quadraui#1055/#1056): `AppShell::show_panel` now accepts the Settings bottom item's id, and `App::on_shell_event_ctx`'s `BottomItemClicked` arm opts into it by calling `ctx.shell_mut().show_panel(id)`, same as quadraui's own `AppShellDemo` | runner header "Menu", masked by `render_settings_panel`'s own " SETTINGS" row |
/// | Settings filter row | **none** — `settings_query` still filters the form, invisibly | "/ <query>" via `draw_settings_chrome` |
///
/// #1343 (#1258's ruling) converged both gaps: the shell's own `AppShell`
/// header is the *only* header on both backends now (title-case
/// "Extensions"/"Settings" on `tui_prod`'s own `shell_config`, all-caps
/// "EXTENSIONS"/"SETTINGS" from the shared `App`'s `PanelDefinition`s —
/// that casing split is real and stays, see the scenarios' own doc
/// comments below), and both panels paint a search/filter row through
/// `SidebarPanelBody::render_with` (quadraui#1059) with
/// `SidebarPanelChrome::Search` chrome (quadraui#1061, consumed by #1391 —
/// before that, a bespoke `render::paint_sidebar_search_row`), built by the
/// shared `render::search_only_chrome`, called by GTK's `App::
/// paint_sidebar_panel_rung` and by the shipped TUI's `tui_main::panels::
/// render_settings_panel`/`render_ext_sidebar` (whose own hand-painted
/// duplicate header rows are deleted). Every scenario below is ungated
/// (`KNOWN_BUGS` is empty) as of #1343.
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
        // Case differs by arm, same as `settings_then_explorer_reclaims_
        // header` below: the shared `App`'s `PanelDefinition` titles are
        // all-caps ("EXTENSIONS"), `TuiShellApp::shell_config`'s own are
        // title-case ("Extensions") — both are the shell's *one* header,
        // not a second hand-painted row (#1343 deleted the shipped TUI's
        // duplicate " EXTENSIONS" row that used to paper over this exact
        // casing gap by accident).
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
        // Case differs by arm, same as `settings_then_explorer_reclaims_
        // header` below and `extensions_header_is_painted` above: the
        // shared `App`'s `PanelDefinition` titles are all-caps
        // ("SETTINGS"), `TuiShellApp::shell_config`'s own bottom-item
        // title is title-case ("Settings").
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

    /// #1356 (quadraui bump for quadraui#1055/#1056's second commit,
    /// 3020d9e): once Settings — a bottom item — owns the sidebar header,
    /// clicking a *top* panel's icon must take the header back rather than
    /// leaving it stuck on Settings. Before 3020d9e,
    /// `AppShell::handle_activity_click`'s top-panel branch never cleared
    /// `sidebar_bottom_owner`, so the header (and, on the shared `App`,
    /// the painted body) would have stayed on Settings even though
    /// Explorer's icon was clicked.
    fn settings_then_explorer_reclaims_header<D: ConformanceDriver>(driver: &mut D) {
        open_settings(driver);
        assert!(
            driver.screen_has("Appearance"),
            "precondition: clicking the Settings icon must paint the \
             settings form body"
        );

        driver.click_text(crate::icons::EXPLORER.s());

        // Case differs by arm: the shared `App`'s `PanelDefinition`
        // titles are all-caps ("EXPLORER"), `TuiShellApp::shell_config`'s
        // own are title-case ("Explorer") — see this module's doc table.
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

    /// #1343 new assertion: the shipped TUI must paint exactly *one*
    /// header naming the Extensions panel — not the shell's own header
    /// row plus a second, hand-painted duplicate. Before this fix,
    /// `render_ext_sidebar` painted its own " EXTENSIONS" row on top of
    /// the shell's — this counts painted text runs instead of just
    /// checking presence (`extensions_header_is_painted` above), since
    /// presence alone can't distinguish "painted once" from "painted
    /// twice" and passed throughout the #1256 double-header bug. Scoped
    /// to `tui_prod` only: GTK and the `tui` control arm never had a
    /// hand-painted second header (see this module's doc table above).
    #[test]
    fn tui_prod_paints_exactly_one_extensions_header() {
        let mut h = crate::tui_main::testing::conformance_harness_prod(engine_fixture(), 100, 30);
        open_extensions(&mut h.driver);
        let inv = h.driver.inventory();
        // Case differs by arm (see `extensions_header_is_painted`'s doc) —
        // `TuiShellApp::shell_config`'s title is "Extensions", not
        // "EXTENSIONS"; sum both so a regression that reintroduces the
        // all-caps duplicate still trips this either way.
        let count = inv.count("EXTENSIONS") + inv.count("Extensions");
        assert_eq!(
            count, 1,
            "#1343: the shipped TUI must paint exactly one Extensions \
             header, not the shell's plus a second hand-painted \
             duplicate: {inv:?}"
        );
    }

    /// Settings twin of
    /// [`tui_prod_paints_exactly_one_extensions_header`] — before this
    /// fix, `render_settings_panel` painted its own " SETTINGS" row on
    /// top of the shell's (stale, pre-#1356) header.
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

/// #1360: cross-backend wiring for
/// [`activity_bar_click_focuses_search_panel`] — see that scenario's own
/// doc for the repro and the assertions' rationale.
///
/// `tui` used to be omitted here (only `gtk` and `tui_prod` were
/// registered) on the strength of a by-hand observation: "at this
/// harness's viewport size, `App` driven by `quadraui::tui::TuiBackend`
/// never paints the editor's buffer content at all". #1425's App-on-TUI
/// gap inventory re-checked that claim by actually running the `tui` arm
/// (rather than trusting the comment) and it no longer reproduces — the
/// precondition (`screen_has("Ln 1,")`) and the rest of
/// [`activity_bar_click_focuses_search_panel`]'s body all pass cleanly at
/// this scenario's `(800, 480)` size. Whatever regressed the by-hand
/// observation has since been fixed elsewhere (most likely #700's move to
/// fixed-pixel `TAB_ROW_HEIGHT_PX`/`BREADCRUMB_ROW_HEIGHT_PX` no longer
/// scaling with `line_height`, so the huge `lh` a `TuiBackend` used to
/// report no longer inflates the reserved chrome past the viewport) —
/// registering the `tui` arm plain (no `KNOWN_BUGS` gate) is itself the
/// record that this gap has closed.
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
        size: (800, 480),
        body: |driver| {
            crate::harness::activity_bar_click_focuses_search_panel(driver);
        },
    }
}

/// #1426's own acceptance bar: at a realistic `(80, 24)` terminal, the
/// shared `App` (driven by `quadraui::tui::TuiBackend`) must paint the
/// buffer's first line directly below a 1-row tab bar, with the status bar
/// on row 22 and the command line on row 23 — proof that
/// `render::UnitProfile::cell()` reserves the tab bar as one cell row (not
/// `TAB_ROW_HEIGHT_PX`'s 35 rows) rather than collapsing the whole editor
/// content band the way the pre-#1426 hardcoded pixel constants did.
///
/// `tui_prod` (`TuiShellApp`) is not run as a second arm here: reaching an
/// explorer-collapsed baseline on it needs two activity-bar clicks (its
/// first click only selects Explorer — the runner's own `AppShell` starts
/// on the hamburger panel while the engine's shadow copy already defaults
/// to Explorer, so the very first click just reconciles the label, not the
/// reservation — verified by hand while writing this scenario), and the
/// resulting chrome shape then fuses the tab label into the same row as the
/// hamburger icon and window-control glyphs, one row layout than `App`'s.
/// That is a real, pre-existing quirk of `conformance_harness_prod`'s
/// default reveal state — unrelated to `UnitProfile`, and orthogonal to
/// what this scenario exists to pin down — so it is not encoded here as a
/// hardcoded row expectation. `TuiShellApp`'s own row-0-tab-bar and
/// last-two-rows-status/command-line facts are already established
/// separately and extensively by `shell_app.rs`'s own test suite (e.g. the
/// `#[test]` this file's `tab_bar` module mirrors,
/// `render_content_paints_command_line_via_shell_app`, and the sibling
/// tests around it) — this scenario's job is only to confirm `App` now
/// matches that shape too, not to re-derive it a second time.
///
/// The row-0/row-1 pair is gated: `App::setup()` unconditionally sets
/// `engine.menu_bar_visible = true` for any non-native-menu backend
/// (`src/app.rs`), while `TuiShellApp::from_engine` only does so in
/// vscode-mode or after an Alt-reveal — so `App`'s tab bar paints on row 1
/// (below its own permanent menu-bar row) instead of row 0. This is the
/// exact same "feature" divergence
/// `tab_bar::render_content_paints_single_group_tab_bar_via_shell_app` /
/// `tab_bar::two_groups_paint_two_tab_bars` (`crate::tui_main::
/// app_on_tui_tests`) already gate — target: menu-bar caps (conditional
/// menu-bar reveal on `App`), not `UnitProfile`. Row 22/row 23 (status bar,
/// command line) are unaffected by the top-of-screen offset and pass
/// ungated — only the top-anchored tab-bar/buffer-row pair is gated.
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

    /// Click the already-active Explorer activity-bar icon a second time,
    /// collapsing the sidebar's screen-space reservation — mirrors
    /// `crate::tui_main::app_on_tui_tests`'s own `collapse_sidebar` helper
    /// (not reusable directly: that one is private to its own module).
    fn collapse_sidebar(driver: &mut quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>) {
        let explorer_zone = driver
            .inventory()
            .zones()
            .iter()
            .find(|z| z.id.as_str() == crate::core::engine::sidebar::PANEL_EXPLORER)
            .map(|z| z.bounds)
            .expect("the Explorer activity-bar icon must register a chrome zone");
        driver.click(
            explorer_zone.x + explorer_zone.width / 2.0,
            explorer_zone.y + explorer_zone.height / 2.0,
        );
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

        // #1426 gate: the top-anchored tab-bar/buffer-row pair, unlike the
        // bottom-anchored rows above, shifts down by one on `App` — see
        // this module's own doc.
        crate::harness::known_bug_gate(
            "render_content_paints_buffer_status_and_command_line_on_shipped_tui_rows::tui",
            || {
                assert!(
                    lines[0].contains("[No Name]"),
                    "row 0 must paint the tab bar; row:\n{}",
                    lines[0]
                );
                assert!(
                    lines[1].contains("ZQXW1426_ROWLINE"),
                    "row 1, directly below the 1-row tab bar, must paint \
                     the buffer's first line; row:\n{}",
                    lines[1]
                );
            },
        );
    }
}

/// #1419: cross-backend wiring for
/// [`sidebar_ctrl_w_l_returns_focus_to_the_editor`] and
/// [`sidebar_ctrl_w_h_moves_focus_to_the_activity_bar_toolbar`] — see those
/// scenarios' own docs for the repro and the assertions' rationale.
///
/// The `l` proof's `tui` arm used to be omitted on the same by-hand claim
/// `issue_1360_activity_bar_click_focuses_panel`'s own note above once made:
/// "`App` driven by `quadraui::tui::TuiBackend` never paints the editor's
/// `Ln N, Col N` status-bar segment at this harness's viewport size". #1425's
/// App-on-TUI gap inventory re-checked that claim here too (same engine
/// fixture and `(800, 480)` size as `issue_1360`'s scenario) and it no
/// longer reproduces — registering the `tui` arm plain (no `KNOWN_BUGS`
/// gate) is itself the record that this gap has closed, for the same
/// likely reason (#700's fixed-pixel row heights) given there. The `h`
/// proof only reads panel-title text (already gap-free on `tui`, per
/// `activity_bar_click_focuses_search_panel` above running clean there in
/// spirit) but is still pinned to `[gtk, tui_prod]` for symmetry with its
/// sibling.
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
        size: (800, 480),
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
}

/// #1418: `render::apply_explorer_context_action` converges what used to be
/// two hand-rolled, drifted appliers — TUI's `handle_explorer_context_action`
/// and GTK's `App::dispatch_explorer_ctx_action` — into one function both
/// backends now call from `render::ModalKeyRoute::ContextMenu`'s key rung.
/// One conformance scenario per action below, each pre-building the explorer
/// context menu with the target item already highlighted (mirroring
/// `Engine::open_explorer_context_menu`'s fixed folder-menu order) so the
/// scenario body only has to press `Enter` — `Engine::handle_context_menu_key`'s
/// own `"Return"` confirm binding — and read the painted result.
///
/// `backends: [gtk, tui_prod]` only (no `tui`): `App::dispatch_context_menu_key`
/// is shared code between the `gtk` arm and the `tui` "control" arm (`App` on
/// `quadraui::tui::TuiBackend`), so `tui` would just be a second copy of the
/// same result — `tui_prod` (`TuiShellApp`, the shell `tui_main::run` actually
/// ships) is the one genuinely independent implementation, same reasoning
/// `sc_hint_row_shows_only_while_focused` gives above.
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
            // "file name..." rather than the full "New file name..." —
            // TUI's `paint_edit_input` (quadraui `tui/tree.rs`) always
            // overwrites the placeholder's first painted cell with an
            // inverted-space cursor block, so the leading "N" never
            // survives into a painted text run there (GTK draws the caret
            // as a separate overlay rectangle instead, leaving its own
            // text run intact) — a backend rasterization difference, not
            // part of what this scenario is proving.
            assert!(
                !driver.screen_has("file name..."),
                "precondition: nothing is being edited yet"
            );
            driver.press_named(NamedKey::Enter);
            assert!(
                driver.screen_has("file name..."),
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
    // table, no text input) is natively-expressible on GTK (quadraui#666)
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

    // "Find in Folder..." (idx 4) is the one action that used to have two
    // genuinely *different* behaviors, not just two copies of the same
    // one: TUI opened the (workspace-wide) Grep picker,
    // GTK just focused the Search sidebar panel — neither actually
    // scoped to the clicked folder the menu label names. Confirming must
    // now open the Grep picker, titled `"Grep in scoped_dir/"` (#1438's
    // "show the scope in the picker title" requirement), scoped to
    // `scoped_dir` (`Engine::open_grep_picker_scoped`): typing the marker
    // query both fixture files share must surface only `scoped_dir`'s own
    // match.
    //
    // RED-verified: with `apply_explorer_context_action`'s
    // `"find_in_folder"` arm changed to a bare `_ => {}`, no picker ever
    // opens on either arm below — the same failure both actual pre-fix
    // behaviors would also hit, since neither GTK's old
    // `self.toggle_focus_search()` (opens the Search panel, no scoped
    // "Grep in ..." title at all) nor TUI's old unscoped
    // `engine.open_picker(PickerSource::Grep)` (would fail this scenario's
    // *last* assertion instead, surfacing `other_dir`'s match too, and
    // would show the plain "Live Grep" title rather than a scoped one)
    // leaves this scenario green.
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

/// #1427: the TUI menu-bar reveal/hide rung (the #318 Alt+<letter> shim,
/// the #988/#1029 hamburger-corner-click guard, and the hamburger panel's
/// own reveal/hide) lifted out of `TuiShellApp` into shared `render.rs`
/// functions (`route_menu_bar_reveal`, `route_hamburger_panel_changed`,
/// `route_hamburger_sidebar_hidden`, `sync_menu_bar_title_row`,
/// `reclaim_hamburger_sidebar_reservation`) so `App` — the cross-backend-
/// shared shell every `gtk`/`tui`/`macos`/`win` conformance arm already
/// drives — shares them too, gated on `BackendCaps::window_chrome` in
/// `App::setup` rather than reimplemented per backend.
///
/// These are `tui`-arm ports of scenarios that, before #1427, only existed
/// as `TuiShellApp`-only unit tests in `src/tui_main/shell_app.rs` (still
/// present there, unchanged, as the `tui_prod` proof the shipped TUI binary
/// keeps agreeing with the shared implementation): `alt_letter_reveals_
/// menu_bar_via_shell_app`, `driver_click_on_settings_toggle_with_menu_bar_
/// visible_flips_its_own_row_via_shell_app`, `hamburger_relocated_click_
/// after_reveal_hides_menu_bar`, and `hamburger_stale_click_position_after_
/// reveal_still_hides_menu_bar`. Driving `App` on `quadraui::tui::TuiBackend`
/// (via `crate::tui_main::testing::conformance_harness`) rather than
/// `TuiShellApp` proves the shared implementation, not a second TUI-only
/// reimplementation of it — see this module's own header doc, "Why `App`,
/// not a fresh mock".
///
/// **RED-before-fix, verified by hand**: with this module's test bodies kept
/// as-is but `src/app.rs`, `src/render.rs`, `src/tui_main/shell_app.rs` and
/// `src/tui_main/app_on_tui_tests.rs` reverted to their pre-#1427 state
/// (commit `50d6af2`, the parent of the #1427 fix commit — i.e. before
/// `App::setup` grew the three-way `BackendCaps` branch and before
/// `App::shell_config` prepended the hamburger `PanelDefinition`), three of
/// these four tests failed:
///
/// - `alt_letter_reveals_menu_bar_via_app_on_tui`: `assertion left == right
///   failed` — `left: 3, right: 4`. Alt+F redrew (the `MenuSystem` intercept
///   already existed) but did *not* reveal a title-bar row, because pre-fix
///   `App` never sets `menu_bar_toggleable`/hides the bar on a
///   no-window-chrome backend in the first place — the marker stayed on the
///   same row instead of shifting down by one.
/// - `hamburger_relocated_click_after_reveal_hides_menu_bar_via_app_on_tui`
///   and `hamburger_stale_click_position_after_reveal_still_hides_menu_bar_
///   via_app_on_tui`: both panicked at their own `"precondition: menu bar
///   starts hidden"` assert — pre-fix `App::setup`'s `else` branch pinned
///   `engine.menu_bar_visible = true` unconditionally on a no-native-menu/
///   no-window-chrome backend (today's now-three-way branch's `window_chrome`
///   arm), so the bar painted "File Edit View …" from the very first frame
///   and there was no hamburger `PanelDefinition` to click at all.
///
/// The fourth, `driver_click_on_settings_toggle_with_menu_bar_visible_flips_
/// its_own_row_via_app_on_tui`, stayed green on both revisions: it forces
/// `engine.menu_bar_visible = true` directly in its own fixture rather than
/// exercising the reveal/hide toggle, and pre-#1427 `App` already pinned the
/// bar visible unconditionally in that state, so the row-resolution behaviour
/// it checks was already correct before this issue — it is a non-regression
/// port, not new RED-to-green coverage, and is included for parity with its
/// `tui_prod` twin rather than because it caught a bug.
#[cfg(test)]
mod issue_1427_menu_bar_reveal_shared {
    use super::*;
    use crate::core::engine::sidebar::PANEL_EXPLORER;

    fn engine_fixture() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    /// [`engine_fixture`], plus the sidebar pinned open on Explorer — the
    /// `App`-on-`TuiBackend` twin of `shell_app.rs`'s own
    /// `app_with_sidebar_open` fixture, needed for the same two reasons
    /// that helper documents: (1) `Engine::new_for_test()`'s sidebar is
    /// hidden by default (`default_explorer_visible()` — a deterministic
    /// fixture must not depend on ambient `~/.config/vimcode` state, unlike
    /// a bare `App::new`/`TuiShellApp::new`), which would leave the File
    /// dropdown painting flush against the left edge and erasing an
    /// editor-offset-0 marker; and (2), specific to the hamburger tests
    /// below: `App::shell_config`'s `cell`-profile hamburger
    /// `PanelDefinition` occupies index 0 in the *runner's* fresh
    /// `AppShell` (`quadraui::AppShell::new` always activates index 0),
    /// so the very first hamburger click only produces the reveal
    /// (`AppShellEvent::PanelChanged`) this scenario needs — rather than a
    /// toggle-hide (`SidebarHidden`, `AppShell::handle_activity_click`'s
    /// "already active and visible" branch) — once construction's initial
    /// `take_requested_panel` poll has somewhere real (Explorer) to
    /// reconcile the runner onto first. Explicit `show_panel` (which also
    /// sets `sidebar_visible: true`) rather than relying solely on
    /// `session.explorer_visible` is what actually flips the shadow
    /// `engine.app_shell` here — the session field alone only take effect
    /// through `Engine::sync_app_shell_sidebar_visibility`, which runs once
    /// at construction, before this fixture gets a chance to set it.
    fn engine_with_sidebar_open() -> Engine {
        let mut engine = engine_fixture();
        engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        engine.session.explorer_visible = true;
        engine
    }

    /// Port of `alt_letter_reveals_menu_bar_via_shell_app` (`shell_app.rs`):
    /// Alt+F must reveal the (hidden by default) menu bar *and* hand the
    /// same keystroke to the `MenuSystem` intercept, which activates the
    /// File menu — both of which redraw, and the reveal reserves one more
    /// row above the editor content, shifting a marker at buffer offset 0
    /// down by exactly one line.
    #[test]
    fn alt_letter_reveals_menu_bar_via_app_on_tui() {
        let mut engine = engine_with_sidebar_open();
        engine.buffer_mut().insert(0, "ZQXW_ALT_MARKER");
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
    /// menu_bar` (#988/#1029): a second click at the *same physical
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
