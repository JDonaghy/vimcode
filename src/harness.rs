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

// ── Explorer hit-band fixture (shared) ───────────────────────────────────

/// Row labels the explorer paints for [`explorer_repro_tree`], in visual
/// order below the root. Pass to [`sweep_hit_band_integrity`]'s selector so
/// the sweep targets the tree's own rows — a bounding box would also catch
/// the tab bar, which paints inside the sidebar's x-range.
pub const EXPLORER_ROW_LABELS: [&str; 7] =
    ["src", "core", "mod.rs", "app.rs", "click.rs", "tests", "README.md"];

/// The reported macOS repro's directory shape, materialised on disk so the
/// real explorer walker populates from it: a root holding `src/` (with a
/// `core/` child and two files), a `tests/` sibling and a `README.md`.
///
/// Returns the **canonical** path. On macOS `std::env::temp_dir()` is a
/// `/var/...` symlink into `/private/var/...` and the explorer keys
/// `Engine::explorer_expanded` by the resolved path, so an uncanonicalised
/// root silently fails to match the expand set and the tree paints
/// collapsed — which would let a sweep "pass" by having almost nothing to
/// sweep. `tag` keeps concurrent backends' trees apart.
pub fn explorer_repro_tree(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("vimcode_hitband_{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src").join("core")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("src").join("core").join("mod.rs"), "// mod\n").unwrap();
    std::fs::write(root.join("src").join("app.rs"), "// app\n").unwrap();
    std::fs::write(root.join("src").join("click.rs"), "// click\n").unwrap();
    std::fs::write(root.join("README.md"), "# readme\n").unwrap();
    std::fs::canonicalize(&root).unwrap()
}

/// An [`Engine`] rooted at `root` with the explorer visible and both the
/// root and `src/` expanded — the state the macOS report describes ("open
/// the folder, expand src, click near the bottom of the word src").
pub fn explorer_engine(root: &PathBuf) -> Engine {
    use crate::core::engine::sidebar::PANEL_EXPLORER;

    let mut engine = Engine::new_for_test();
    engine.cwd = root.clone();
    engine.buffer_mut().insert(0, "fn main() {}\n");
    engine.explorer_expanded.insert(root.clone());
    engine.explorer_expanded.insert(root.join("src"));
    engine.explorer_rebuild_rows();
    engine
        .app_shell
        .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
    engine.session.explorer_visible = true;
    // Tab icons off for the same reason `src/macos/mod.rs`'s own fixtures
    // turn them off: the macOS per-tab icon gap (quadraui#620) is a
    // documented backend gap, not this sweep's subject, and leaving it on
    // would fire a `debug_assert!` that has nothing to do with hit bands.
    engine.settings.use_nerd_fonts = false;
    engine
}

// ── Hit-band integrity sweep ─────────────────────────────────────────────
//
// The gap this closes: every click-driven test in this repo, in quadraui's
// conformance scenarios, and in `click_text`/`click_text_at` itself, aims at
// a painted run's *centre* (or an `Anchor` edge). A hit region that is
// offset from the glyphs it belongs to — so the top of a row resolves to
// that row and the bottom resolves to its neighbour — passes every one of
// them. That is not hypothetical: it is the reported macOS explorer-tree
// symptom (clicking the lower half of `src` toggles the `core` row beneath
// it), and it is the same family as quadraui#552 (activity row) and
// vimcode#515 (tab close vs tab body).
//
// The invariant below needs no per-row expected-value table, which is what
// makes it cheap to point at a new surface: a painted text run is emitted by
// one widget for one row, so **every point inside one run must resolve to
// the same thing**. Two probes inside one run that disagree is a defect
// regardless of which one is "right" — and the report says which run, which
// probe points, and how the outcomes differed, so a failure converts to an
// issue without re-deriving anything.

/// One painted run whose interior did not resolve uniformly.
#[derive(Debug, Clone)]
pub struct BandMismatch {
    /// The painted text whose box was probed.
    pub text: String,
    /// That text's painted bounds, in the driver's native unit.
    pub bounds: quadraui::Rect,
    /// The probe that established the baseline outcome.
    pub baseline_probe: (&'static str, f32, f32),
    /// The probe that disagreed with it.
    pub divergent_probe: (&'static str, f32, f32),
    /// First painted line that differs between the two outcomes,
    /// `(baseline, divergent)`.
    pub first_difference: (String, String),
}

/// Outcome of a [`sweep_hit_band_integrity`] run.
#[derive(Debug, Clone, Default)]
pub struct BandReport {
    pub label: String,
    pub runs_swept: usize,
    /// Runs too thin to hold distinct interior probes (TUI's one-cell rows).
    pub runs_too_thin: usize,
    pub mismatches: Vec<BandMismatch>,
}

impl BandReport {
    pub fn is_clean(&self) -> bool {
        self.mismatches.is_empty()
    }

    /// Issue-ready rendering: every mismatch with the coordinates and the
    /// observed divergence, so a failure can be pasted into a bug report
    /// without re-running anything.
    pub fn report(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let _ = writeln!(
            s,
            "hit-band integrity [{}]: {} run(s) swept, {} too thin to probe, {} mismatch(es)",
            self.label,
            self.runs_swept,
            self.runs_too_thin,
            self.mismatches.len()
        );
        for m in &self.mismatches {
            let (bl, bx, by) = m.baseline_probe;
            let (dl, dx, dy) = m.divergent_probe;
            let _ = writeln!(
                s,
                "\n  run {:?} painted at x={:.1} y={:.1} w={:.1} h={:.1}\n    \
                 {bl:>6} ({bx:.1}, {by:.1}) -> {:?}\n    \
                 {dl:>6} ({dx:.1}, {dy:.1}) -> {:?}\n    \
                 ^ two points inside one painted run resolved differently",
                m.text,
                m.bounds.x,
                m.bounds.y,
                m.bounds.width,
                m.bounds.height,
                m.first_difference.0,
                m.first_difference.1,
            );
        }
        s
    }
}

/// Paint signature: every painted run in paint order, with position. Two
/// clicks that leave the app in the same visible state produce the same
/// signature; any difference in what is drawn, or where, shows up here.
fn paint_signature<D: ConformanceDriver>(driver: &D) -> Vec<String> {
    driver
        .inventory()
        .text_runs
        .iter()
        .map(|r| format!("{} @ {:.1},{:.1}", r.text, r.bounds.x, r.bounds.y))
        .collect()
}

fn first_difference(a: &[String], b: &[String]) -> (String, String) {
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i), b.get(i));
        if x != y {
            return (
                x.cloned().unwrap_or_else(|| "<nothing>".into()),
                y.cloned().unwrap_or_else(|| "<nothing>".into()),
            );
        }
    }
    ("<identical>".into(), "<identical>".into())
}

/// Sweep the interior of every painted text run inside `region` and report
/// any run whose interior does not resolve uniformly.
///
/// `make` must return a **fresh** harness each call: every probe click is
/// applied to a pristine instance, so probes cannot contaminate each other
/// and a click that opens a menu or expands a folder is measured in
/// isolation. Each harness is dropped before the next is built (the paint
/// and cwd guards are process-wide — holding two at once would deadlock).
///
/// `select` chooses which painted runs to sweep, by text and bounds — a
/// caller names the surface it means (the explorer's row labels, say)
/// rather than trusting a bounding box, because sibling chrome can and does
/// paint inside the same rectangle.
///
/// Probes five points per run: the vertical triple (top / middle / bottom)
/// that catches a row-band offset, and the horizontal pair (left / right)
/// that catches the tab-bar class (vimcode#515, where a right-edge click
/// landed on the next tab). Runs under 3 units tall or wide cannot hold
/// distinct interior points — one TUI cell — and are counted, not probed,
/// so a TUI run of this sweep honestly reports "nothing to probe" rather
/// than a misleading pass.
pub fn sweep_hit_band_integrity<D, F, P>(make: F, select: P, label: &str) -> BandReport
where
    D: ConformanceDriver + DriverInput,
    F: Fn() -> ConformanceHarness<D>,
    P: Fn(&str, quadraui::Rect) -> bool,
{
    // Collect the target runs from one frame, then drop that harness before
    // building any probe instance.
    let targets: Vec<(String, quadraui::Rect)> = {
        let h = make();
        h.driver
            .inventory()
            .text_runs
            .iter()
            .filter(|r| !r.text.trim().is_empty() && select(&r.text, r.bounds))
            .map(|r| (r.text.clone(), r.bounds))
            .collect()
    };

    let mut report = BandReport {
        label: label.to_string(),
        ..Default::default()
    };

    for (text, b) in targets {
        if b.height < 3.0 || b.width < 3.0 {
            report.runs_too_thin += 1;
            continue;
        }
        report.runs_swept += 1;

        let cx = b.x + b.width / 2.0;
        let cy = b.y + b.height / 2.0;
        let probes: [(&'static str, f32, f32); 5] = [
            ("mid", cx, cy),
            ("top", cx, b.y + 1.0),
            ("bottom", cx, b.y + b.height - 1.0),
            ("left", b.x + 1.0, cy),
            ("right", b.x + b.width - 1.0, cy),
        ];

        let mut baseline: Option<(&'static str, f32, f32, Vec<String>)> = None;
        for (name, px, py) in probes {
            let sig = {
                let mut h = make();
                h.driver.click(px, py);
                paint_signature(&h.driver)
            };
            match &baseline {
                None => baseline = Some((name, px, py, sig)),
                Some((bn, bx, by, bsig)) => {
                    if *bsig != sig {
                        report.mismatches.push(BandMismatch {
                            text: text.clone(),
                            bounds: b,
                            baseline_probe: (bn, *bx, *by),
                            divergent_probe: (name, px, py),
                            first_difference: first_difference(bsig, &sig),
                        });
                        // One mismatch per run is enough to file on; keep
                        // sweeping the remaining runs.
                        break;
                    }
                }
            }
        }
    }

    report
}
