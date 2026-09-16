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

/// #983: a click anywhere inside `needle`'s own **painted row band** —
/// not just its text glyph, but the full vertical slot the panel
/// allocates it before the next painted row (`next_row_needle`) begins —
/// must act on `needle`'s own row, never silently act on the row painted
/// immediately below it.
///
/// This is a different bug shape than [`sweep_hit_band_integrity`]'s
/// #967 family: that helper samples strictly within a run's own painted
/// glyph bounds, which is exactly the zone this issue's bug does *not*
/// live in. #983's report (v0.11.0: "settings / git insights row click
/// selects the row below") traces to a GTK-only gap between a row's own
/// text-glyph height and the panel's real row pitch — e.g. the Settings
/// panel measured 23px-tall label glyphs spaced 32px apart, and
/// `render::handle_settings_form_ui_event`'s `handle_cached` path (the
/// `backend: None` branch of `quadraui::FormController::click_inner`)
/// resolves a click anywhere in that ~9px gap to the *next* field — a
/// point still visually inside the clicked row's own 32px band, by any
/// reasonable reading of "this row's own area" (there is no drawn
/// boundary at the glyph's own bottom edge for a user to see). The same
/// shape reproduces on the plugin/marketplace ext-panel's `SidebarSystem`
/// rows (the "git insights" report) with an even larger ~15-18px gap.
/// Both are a **constant** per-row offset, not a #967-style
/// accumulating one — measured identical (~4.5px into a 32px settings
/// row, both near row 3 and row 48) regardless of row index; see this
/// function's callers for the measurements.
///
/// TUI cannot reproduce this bug *by construction*, not merely "doesn't
/// happen to today": its row pitch is a fixed 1 cell, always exactly
/// equal to its own glyph height (`TextMetricsBackend` is a genuine
/// no-op there — see `src/tui_main/mod.rs`'s own doc), so there is no
/// sub-row gap for a click to land in. The sanity assert below makes
/// that structural claim self-checking rather than assumed: it fails
/// loudly (not silently no-ops) if this is ever pointed at a backend
/// whose glyph height already equals its row pitch, rather than
/// reporting a false "pass" that proves nothing.
///
/// `effect_after_click` reads back, from **painted** output only (never
/// engine state — CLAUDE.md's "assert on rendered output" rule, #587/
/// #592), whether the click acted on `needle`'s own row: `true` only
/// when the correct-row outcome is observed.
pub fn row_click_hits_its_own_row_not_the_row_below<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    needle: &str,
    next_row_needle: &str,
    mut effect_after_click: impl FnMut(&mut D) -> bool,
) {
    let locate = |d: &mut D, text: &str| -> quadraui::Rect {
        d.inventory()
            .text_runs()
            .iter()
            .find(|r| r.text.contains(text))
            .unwrap_or_else(|| {
                panic!("row_click_hits_its_own_row_not_the_row_below: {text:?} not painted")
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

    let x = bounds.x + bounds.width / 2.0;
    // Just inside `needle`'s own row slot, immediately above where the
    // next row's own label begins — self-measured from two real painted
    // positions, never a literal coordinate.
    let y = next_bounds.y - 0.5;
    assert!(
        y > bounds.y + bounds.height,
        "sanity: the probe point (y={y:.1}) must fall below {needle:?}'s own \
         text glyph (bottom={:.1}) — otherwise this only re-tests the glyph's \
         own centre, which every pre-existing click test in this panel \
         already covers, not the gap between a row's glyph and the next \
         row's own label this issue is about. A backend whose row pitch \
         already equals its glyph height (TUI, by construction) has no such \
         gap and will fail here — that is the point, not a bug in the probe: \
         this scenario should not be registered for that backend.",
        bounds.y + bounds.height,
    );

    driver.click(x, y);

    assert!(
        effect_after_click(driver),
        "a click inside {needle:?}'s own painted row band (x={x:.1}, y={y:.1}) \
         — below its text glyph, but still above where {next_row_needle:?} \
         begins painting — must act on {needle:?}'s own row, not the row \
         painted below it (#983)"
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
/// #983 is the first of those chained follow-ups to land a real entry —
/// two, one per backend-scoped scenario it adds (settings panel, ext-panel/
/// "git insights"). The remaining v0.11.0 bugs are separate issues chained
/// `--after` this one, each adding its own label here alongside its
/// scenario.
pub(crate) const KNOWN_BUGS: &[&str] = &[
    // #983: v0.11.0 bug report -- a click inside a settings-panel row's own
    // painted band, below its text glyph but still above the next row's own
    // label, resolves to the row below instead of the row clicked. GTK-only
    // by construction (TUI's row pitch always equals its glyph height, so it
    // has no such gap to fall into) -- see
    // `row_click_hits_its_own_row_not_the_row_below`'s own doc.
    "settings_row_click_selects_the_clicked_row_not_the_row_below::gtk", // #983
    // #983: the same shared-cause report against the "git insights" plugin
    // panel, reproduced here via the ext-panel/marketplace `SidebarSystem`
    // plumbing that panel id actually routes through today (see this
    // scenario's own fixture doc for why). GTK-only, same reason as above.
    "ext_panel_row_click_selects_the_clicked_row_not_the_row_below::gtk", // #983
    // #984: v0.11.0 bug report -- the file explorer's expand/collapse
    // chevron needs a double click, while its row's text label needs one.
    // The root cause is a missing `TreeControllerEvent::RowToggleExpand`
    // match arm in `Engine::dispatch_explorer_tree_event` (shared core
    // code, not backend-specific) -- see
    // `explorer_chevron_click_toggles_dir_with_same_arity_as_label_click`'s
    // own doc. Reproduces on both backends, unlike #983's GTK-only gap.
    "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click::gtk", // #984
    "explorer_chevron_click_toggles_dir_with_same_arity_as_label_click::tui", // #984
    // #987: v0.11.0 bug report -- a group's own vertical scrollbar is inert
    // (no `EditorHit::VScrollbar` hit-test exists anywhere in
    // `src/app.rs`'s shared mouse dispatch, on either backend), and when
    // that group sits immediately left of a group divider, the same click
    // also silently perturbs the split ratio via
    // `render::divider_ratio_from_pos`. Reproduces identically on both
    // backends -- `crate::harness`'s "tui" conformance arm drives the same
    // shared `crate::app::App` dispatch code as GTK, not the separately
    // hand-written production TUI stack -- see
    // `drag_group_scrollbar_column`'s own doc.
    "left_group_scrollbar_drag_scrolls_without_resizing::gtk", // #987
    "left_group_scrollbar_drag_scrolls_without_resizing::tui", // #987
    // #987 deliverable 3: the same inertness generalizes to *any* group,
    // not just one beside a divider -- the right group's scrollbar (no
    // divider on its own right edge in a two-group layout) is inert too,
    // just without the resize side effect. See
    // `right_group_scrollbar_drag_scrolls_without_resizing`'s own doc for
    // the RED-verification distinguishing this from the left-group case
    // above (only one assertion fails here, not both).
    "right_group_scrollbar_drag_scrolls_without_resizing::gtk", // #987
    "right_group_scrollbar_drag_scrolls_without_resizing::tui", // #987
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
///     backends: [gtk, tui],
///     engine: my_engine_fixture(),
///     size: (800, 480),
///     body: |driver| {
///         crate::harness::command_palette_filters_and_escape_dismisses(driver);
///     },
/// }
/// ```
///
/// expands to a `mod my_scenario { fn gtk() { .. } fn tui() { .. } }` with
/// one `#[test]` per backend arm — `cargo test`'s own `mod_path::backend`
/// test-name nesting is what keeps a single-backend failure self-locating,
/// the same property `..._on_gtk`/`..._on_tui` naming would give, without
/// needing identifier concatenation (no `concat_idents!`/proc-macro
/// dependency to get there). The `gtk` arm is gated on `feature = "gui"`,
/// the same gate `vimcode`'s own `required-features` puts on the GTK bin;
/// the `tui` arm has no gate — `quadraui/tui` is an unconditional feature
/// of the pinned dependency (see `Cargo.toml`), not an optional vimcode one.
///
/// Only `gtk`/`tui` are wired today. Growing this to `macos`/`win` is
/// adding their own `@arm` match below, mirroring their existing
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
    crate::backend_conformance! {
        label: sweep_hit_band_integrity_proof,
        backends: [gtk, tui],
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

// ── #983: settings / git-insights row click selects the row below ──────
//
// v0.11.0 bug suite. Both panels share the same root cause (see
// `row_click_hits_its_own_row_not_the_row_below`'s own doc): a GTK-only
// gap between a row's painted text-glyph height and the panel's real row
// pitch, constant per row (not growing like #967), that resolves a click
// in that gap to the row below. Reported and confirmed here on both the
// Settings panel (`FormController`) and the ext-panel/marketplace
// `SidebarSystem` that "git insights" (a plugin panel) routes through —
// see the second fixture's own doc for why that's the closest in-repo
// reproduction of the plugin panel specifically. TUI is excluded from
// both `backend_conformance!` registrations below, not silently skipped:
// its row pitch always equals its glyph height by construction (fixed
// `TextMetricsBackend` no-op, `src/tui_main/mod.rs`), so there is no gap
// for this bug to live in.
#[cfg(test)]
mod issue_983_row_click_selects_the_row_below {
    use super::*;
    use crate::core::engine::sidebar::PANEL_SETTINGS;
    use crate::core::extensions::ExtensionManifest;

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

    /// The ext-panel / plugin-panel family, with enough filler *installed*
    /// rows (20) ahead of one *available* extension that the "AVAILABLE"
    /// section header itself paints well below row 0 — same "well below
    /// row 0" requirement as the settings fixture above, applied to the
    /// panel this repo can actually paint content rows for.
    ///
    /// `ext_panel_active` is set to `"git-insights"` — the actual reported
    /// panel's name — rather than opening the built-in Extensions
    /// marketplace panel (`PANEL_EXTENSIONS`) directly. This is not merely
    /// cosmetic: `Engine::populate_ext_sidebar_system` (the function every
    /// `ext:<plugin>` panel id paints through, per `App`'s `id.starts_with
    /// ("ext:")` render arm) always builds its rows from the *marketplace*
    /// manifest list, regardless of which plugin id is active — a
    /// separate, already-documented gap (see
    /// `gtk::testing::focused_plugin_panel_outranks_a_stale_explorer_flag_on_gtk`'s
    /// own doc), not this issue's to fix. So this fixture genuinely routes
    /// through the git-insights plugin panel's own click/paint path; what
    /// it happens to *show* while doing so is marketplace content, which is
    /// exactly the shared `SidebarSystem` plumbing the actual git-insights
    /// panel would use for its own rows once that other gap is closed.
    fn engine_git_insights_scrolled_to_available() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        let mut manifests: Vec<ExtensionManifest> = (0..20)
            .map(|i| ExtensionManifest {
                name: format!("zqxw983installed{i}"),
                display_name: format!("Zqxw983Installed{i}"),
                ..Default::default()
            })
            .collect();
        for m in &manifests {
            engine
                .extension_state
                .mark_installed_version(&m.name, "1.0.0");
        }
        manifests.push(ExtensionManifest {
            name: "zqxw983avail".to_string(),
            display_name: "Zqxw983Avail".to_string(),
            ..Default::default()
        });
        engine.ext_registry = Some(manifests);
        engine.ext_panel_active = Some("git-insights".to_string());
        engine.ext_panel_has_focus = true;
        if !engine.app_shell.sidebar_visible() {
            engine.app_shell.toggle_sidebar();
        }
        engine
    }

    // ── Deliverable 1: click well below row 0, assert the painted
    // selection landed on the clicked row, not the row below ──────────

    // RED-verification (#983): with the KNOWN_BUGS entry below removed,
    // `cargo test --features gui --lib
    // issue_983_row_click_selects_the_row_below::settings_row_click_selects_the_clicked_row_not_the_row_below::gtk`
    // fails on the final assertion inside
    // `row_click_hits_its_own_row_not_the_row_below` — "Enable LSP"/"Format
    // on Save" are still painted after the click, proving it landed on
    // LSP's own first setting row instead of the LSP category header.
    // Restored (KNOWN_BUGS entry back in place) and confirmed green again.
    crate::backend_conformance! {
        label: settings_row_click_selects_the_clicked_row_not_the_row_below,
        backends: [gtk],
        engine: engine_settings_scrolled_to_lsp(),
        size: (1400, 900),
        body: |driver| {
            crate::harness::known_bug_gate(
                "settings_row_click_selects_the_clicked_row_not_the_row_below::gtk",
                || {
                    assert!(
                        driver.screen_has("Enable LSP") && driver.screen_has("Format on Save"),
                        "precondition: scrolling to the LSP category must paint both \
                         of its settings; painted: {:?}",
                        driver.inventory().text_runs()
                    );
                    crate::harness::row_click_hits_its_own_row_not_the_row_below(
                        driver,
                        "▼ LSP",
                        "Enable LSP",
                        |d| !(d.screen_has("Enable LSP") || d.screen_has("Format on Save")),
                    );
                },
            );
        },
    }

    // RED-verification (#983): same procedure as above, against
    // `ext_panel_row_click_selects_the_clicked_row_not_the_row_below::gtk`
    // — with its KNOWN_BUGS entry removed, the final assertion fails
    // because "Zqxw983Avail" is still painted after the click (the
    // AVAILABLE header failed to collapse; the click landed on the
    // available row itself, one row below the header). Restored and
    // confirmed green again.
    crate::backend_conformance! {
        label: ext_panel_row_click_selects_the_clicked_row_not_the_row_below,
        backends: [gtk],
        engine: engine_git_insights_scrolled_to_available(),
        size: (1400, 900),
        body: |driver| {
            crate::harness::known_bug_gate(
                "ext_panel_row_click_selects_the_clicked_row_not_the_row_below::gtk",
                || {
                    assert!(
                        driver.screen_has("AVAILABLE") && driver.screen_has("Zqxw983Avail"),
                        "precondition: the ext panel must paint the pushed-down \
                         AVAILABLE header and its one row; painted: {:?}",
                        driver.inventory().text_runs()
                    );
                    crate::harness::row_click_hits_its_own_row_not_the_row_below(
                        driver,
                        "AVAILABLE",
                        "Zqxw983Avail",
                        |d| !d.screen_has("Zqxw983Avail"),
                    );
                },
            );
        },
    }

    // ── Deliverable 2: sweep_hit_band_integrity over a settings row and
    // an ext-panel row — the "similar bugs" generalization. These sample
    // strictly inside the needle's own painted glyph bounds (per that
    // helper's own contract), which is the #967-shaped zone #983's own
    // bug does *not* live in (it lives in the gap *below* the glyph — see
    // `row_click_hits_its_own_row_not_the_row_below`'s doc) — so both are
    // expected to pass today, on every backend, with no KNOWN_BUGS entry.
    // What they protect against is a *different*, #967-style regression
    // creeping into either row-pitch formula later, and they cost nothing
    // extra to also run on TUI (`ConformanceDriver + DriverInput` is
    // TUI's own bound, not a GTK-only one — see this module's top doc on
    // "Which trait bound a scenario needs").
    crate::backend_conformance! {
        label: settings_row_sweep_hit_band_integrity,
        backends: [gtk, tui],
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

    // Not via `backend_conformance!`: this probe's fingerprint (the
    // AVAILABLE section's collapsed flag) needs to be force-reset via
    // direct `engine` access between samples, which the macro's
    // `|driver|`-only body has no way to reach. Same reason #971's own
    // GTK/macOS twins of this exact probe
    // (`ext_panel_header_click_hit_band_matches_the_painted_row_gtk`)
    // are hand-written rather than macro-generated — mirrored here,
    // just with the AVAILABLE header pushed well below row 0 instead of
    // sitting at the top of an empty installed section.
    //
    // `sweep_hit_band_integrity` (used for the settings probe above, and
    // for #971's *own* explorer/picker probes) assumes two clicks at the
    // same point cancel out. That assumption breaks here: quadraui's
    // `DoubleClickDetector` folds two same-spot `MouseDown`s in quick
    // succession into a `DoubleClick` on every backend, and
    // `SidebarSystem::double_click` has no header case (see #971's own
    // doc on `sc_panel_header_click_hit_band_matches_the_painted_row_gtk`
    // for the full mechanism) — so a plain same-point restore click
    // silently no-ops instead of re-expanding the section, and later
    // samples (landing at different y offsets, which *don't* trip the
    // detector) toggle from whatever state was actually left behind
    // rather than from a known baseline. Confirmed by observation before
    // settling on `_resetting` here: the plain sweep produced an
    // alternating true/false/true/false/true outcome sequence — exactly
    // the shape a silently-skipped restore produces, not a real
    // row-index-dependent hit-band disagreement.
    #[cfg(feature = "gui")]
    #[test]
    fn ext_panel_row_sweep_hit_band_integrity_gtk() {
        let mut h = crate::gtk::testing::conformance_harness(
            engine_git_insights_scrolled_to_available(),
            1400,
            900,
        );
        assert!(
            h.driver.screen_has("Zqxw983Avail"),
            "precondition: the pushed-down AVAILABLE row must be painted"
        );
        let engine = h.engine.clone();
        crate::harness::sweep_hit_band_integrity_resetting(
            &mut h.driver,
            "AVAILABLE",
            5,
            |d| {
                // Break the `DoubleClickDetector`'s position match before
                // every real probe (a harmless corner of the window, well
                // clear of the sidebar) — see this test's own doc.
                d.click(1380.0, 880.0);
                engine
                    .borrow_mut()
                    .ext_sidebar_system
                    .borrow_mut()
                    .set_collapsed(1, false);
                d.render();
            },
            |d| ConformanceDriver::screen_has(d, "Zqxw983Avail"),
        );
    }

    #[test]
    fn ext_panel_row_sweep_hit_band_integrity_tui() {
        let mut h = crate::tui_main::testing::conformance_harness(
            engine_git_insights_scrolled_to_available(),
            1400,
            900,
        );
        assert!(
            h.driver.screen_has("Zqxw983Avail"),
            "precondition: the pushed-down AVAILABLE row must be painted"
        );
        let engine = h.engine.clone();
        crate::harness::sweep_hit_band_integrity_resetting(
            &mut h.driver,
            "AVAILABLE",
            5,
            |d| {
                d.click(1380.0, 880.0);
                engine
                    .borrow_mut()
                    .ext_sidebar_system
                    .borrow_mut()
                    .set_collapsed(1, false);
                d.render();
            },
            |d| ConformanceDriver::screen_has(d, "Zqxw983Avail"),
        );
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
    /// `kkxxqq_dir984`, holding one child, `child984_marker` -- the root
    /// itself is expanded (so `kkxxqq_dir984`'s own row paints) but
    /// `kkxxqq_dir984` is deliberately left out of `explorer_expanded`, so
    /// it starts collapsed, matching this scenario's own precondition.
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
        std::fs::write(dir.join("kkxxqq_dir984").join("child984_marker"), b"").unwrap();

        let mut engine = crate::core::Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine.cwd = dir.clone();
        engine.explorer_expanded.insert(dir.clone());
        engine.explorer_rebuild_rows();
        engine.session.explorer_visible = true;
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
    // -- "child984_marker" is still not painted after one chevron click,
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
                    "child984_marker",
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
                    "child984_marker",
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
    crate::backend_conformance! {
        label: explorer_row_sweep_hit_band_integrity,
        backends: [gtk, tui],
        engine: engine_with_collapsed_explorer_dir("sweep"),
        size: (800, 480),
        body: |driver| {
            assert!(
                driver.screen_has("kkxxqq_dir984"),
                "precondition: the collapsed directory must be painted"
            );
            crate::harness::sweep_hit_band_integrity(driver, "kkxxqq_dir984", 5, |d| {
                d.screen_has("child984_marker")
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
        engine.settings.use_nerd_fonts = false;
        engine.settings.minimap = false;

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
