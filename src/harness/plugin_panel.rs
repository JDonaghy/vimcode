//! Backend-neutral **plugin-panel** fixture + scenarios (#1090).
//!
//! # Why this module exists
//!
//! Four defects in the `git-insights` sidebar panel — #1086 (every click
//! landed one row low), #1087 (the hover card anchored off-viewport once
//! the panel was scrolled), #1088 (`panel.reveal` selected the wrong row)
//! and #1089 (three of four backends paint the extension *marketplace*
//! instead of the plugin's own panel) — all shipped green. `tests/
//! ext_panel.rs`'s 50 tests assert `Engine` state, which stayed correct
//! throughout; and the GTK/macOS hit-band sweeps that *look* like they
//! cover this panel
//! (`gtk::testing::…::ext_panel_row_sweep_hit_band_integrity_gtk`,
//! `macos::…::ext_panel_header_click_hit_band_matches_the_painted_row`)
//! are pointed at `engine_with_marketplace_as_ext_panel` — a fixture whose
//! rows come from `Engine::populate_ext_sidebar_system`, i.e. the
//! marketplace manifest list, not from a `PanelRegistration` at all.
//!
//! The whole class collapses to one sentence:
//!
//! > **The row you click is the row that gets selected, and a hover
//! > anchors to the row you hovered — measured against the painted
//! > geometry, on every backend.**
//!
//! Everything below drives *that* property against a genuine
//! plugin-registered panel, reading only painted output
//! (`inventory().text_runs()` bounds, `screen_has`, popup bounds) — never
//! `engine.ext_panel_selected`, which was already right while the paint
//! step was wrong (CLAUDE.md's "rendered output, not state" rule, learned
//! via #587/#592).
//!
//! # `KNOWN_BUGS`-gated GUI arms (#1089, resolved)
//!
//! Before #1089, `crate::app::App` — the cross-backend-shared shell `gtk`,
//! `macos`, `win` **and** the `tui` control arm all wrap — painted every
//! `ext:<name>` panel through `render::populate_ext_sidebar_system`, which
//! built its rows from the marketplace manifest list regardless of which
//! plugin id was active (`src/app.rs`, the `id if id.starts_with("ext:")`
//! arm), while only `crate::tui_main::panels::render_ext_panel` — reached
//! via `TuiShellApp`, i.e. the `tui_prod` arm — painted a
//! `PanelRegistration`'s own sections through `render::ext_panel_to_tree_view`.
//!
//! So the scenarios here used to be green on `tui_prod` and red everywhere
//! else, exactly as #1090 predicted. They are *not* `#[cfg]`-gated per
//! backend — that would make them vacuously pass, the failure mode
//! `scripts/platform-conformance.sh` exists to prevent (#645) — they were
//! instead wrapped in [`crate::harness::known_bug_gate`] with their labels
//! listed in [`crate::harness::KNOWN_BUGS`], which is the *opposite* of
//! vacuous: that gate fails the build the moment a listed body starts
//! passing, so a fix can't land without deleting the entry and turning the
//! scenario into an ordinary, enforced conformance test. #1089 fixed the
//! shared `gtk`/`tui`/`win` paint path; its three `::macos`-suffixed
//! entries stayed gated only because no macOS runner existed yet to confirm
//! them, and #1276 confirmed and deleted them once one did. `KNOWN_BUGS` is
//! therefore empty today — every arm below takes the plain `Pass` path, not
//! `ExpectedFail`.
//!
//! # Where the arms live
//!
//! `gtk` / `tui` / `tui_prod` are registered in this file's own
//! `#[cfg(test)]` module; the `macos` arm lives here too, behind
//! `#[cfg(all(feature = "macos", target_os = "macos"))]`, driving
//! `crate::macos::conformance_harness` (the `pub(crate)` constructor
//! `src/macos/mod.rs` exposes for exactly this). `scripts/
//! platform-conformance.sh` picks all four up without a script change:
//! every lane it runs is a `cargo test` invocation over this crate's lib
//! tests.

use std::cell::RefCell;
use std::rc::Rc;

use quadraui::testing::{ConformanceDriver, DriverInput};

use crate::core::plugin::{ExtPanelBadge, ExtPanelItem, PanelRegistration};
use crate::core::Engine;

// ── Deliverable 1: the shared plugin-panel fixture ──────────────────────
//
// Modelled on `git-insights`' real shape (a summary section with a
// branch row carrying a badge and a hint, a separator, a log section of
// expandable commits with file children, and a section that is legitimately
// empty) but synthetic and distinctively named — it never needs a repo with
// history, never consults the extension registry, and its needles cannot
// collide with anything else this suite paints.
//
// Every label is deliberately short. The shared `App::shell_config()`
// sidebar is narrow (~30 columns on TUI at these viewport sizes) and a
// truncated row is a row `screen_has`/`find_bounds` cannot see — the same
// column-budget trap `crate::harness`'s `engine_with_collapsed_explorer_dir`
// documents for `child984mk`.

/// The plugin's registered panel name (`ext_panel_active` / the `ext:` id
/// suffix).
pub const PANEL: &str = "zqxw1090-insights";
/// The panel's title, painted into the sidebar chrome header.
pub const PANEL_TITLE: &str = "Zqxw1090";

/// Section 0 — the branch/summary section (a badge+hint row, a separator,
/// then filler rows that make scrolling meaningful).
pub const SEC_SUMMARY: &str = "SummZQXW1090";
/// Section 1 — expandable commits, each with one file child.
pub const SEC_COMMITS: &str = "CommZQXW1090";
/// Section 2 — legitimately empty, so a header with no body is covered.
pub const SEC_EMPTY: &str = "EmptZQXW1090";

/// Section 0's first row: carries a badge *and* a hint, the richest row
/// shape `ext_panel_to_tree_view` can emit.
///
/// Shorter than the other needles on purpose: the badge and hint are
/// right-aligned into the *same* narrow sidebar row, and at 12 characters
/// the label was painted truncated (`mainZQXW109[HEAD]`), which
/// `screen_has` cannot match — the column-budget trap this module's own
/// header comment describes, hit for real during #1090's development.
pub const ROW_BRANCH: &str = "mainZ1090";
/// Section 1's first commit — expandable, expanded by default.
pub const ROW_COMMIT_A: &str = "c0ffZQXW1090";
/// `ROW_COMMIT_A`'s one file child — the painted signal that says whether
/// A is expanded.
pub const ROW_CHILD_A: &str = "fileA1090";
/// Section 1's second commit — expandable, expanded by default. Present so
/// "selects that row **and no other**" has a neighbour to be violated
/// against.
pub const ROW_COMMIT_B: &str = "d00dZQXW1090";
/// `ROW_COMMIT_B`'s one file child.
pub const ROW_CHILD_B: &str = "fileB1090";

/// `ROW_COMMIT_A`'s item id — what a `panel.reveal` query resolves against.
pub const ID_COMMIT_A: &str = "c0ffee101090aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/// The short-hash prefix a real `:GitShow` "Open Commit" link reveals by.
pub const REVEAL_QUERY: &str = "c0ffee10";

/// How many filler rows section 0 carries. Chosen so `ROW_COMMIT_A` sits
/// well below row 0 (the "#983 well below row 0" requirement, applied to a
/// panel that can finally paint its own content rows) while the whole
/// panel still fits a 30-row viewport unscrolled.
pub const FILLER_ROWS: usize = 12;

/// Flat index of `SEC_COMMITS`' header: `SEC_SUMMARY` header (1) +
/// `ROW_BRANCH` (1) + separator (1) + `FILLER_ROWS`.
pub const FLAT_SEC_COMMITS: usize = 3 + FILLER_ROWS;
/// Flat index of `ROW_COMMIT_A`: the `SEC_COMMITS` header, then that
/// section's own leading empty-id separator (`git_log_panel.lua`'s real
/// shape, and #1088's trap — see [`engine_with_plugin_panel`]).
pub const FLAT_COMMIT_A: usize = FLAT_SEC_COMMITS + 2;

/// How the fixture's panel should be *positioned* and *decorated* before
/// the first frame paints. Every field maps to one of #1090's six
/// scenarios; `Default` is the unscrolled, chrome-minimal baseline.
#[derive(Clone, Debug, Default)]
pub struct PluginPanelFixture {
    /// `ext_panel_scroll_top` — scenario 3 ("the current bugs are
    /// scroll-invariant, but the fix must be too").
    pub scroll_top: usize,
    /// Show the panel's search-input row (`chrome_h == 2` in
    /// `tui_main::panels::render_ext_panel`, `input_rows == 1` in
    /// `tui_main::mouse`) — scenario 4, the *second*, independent constant
    /// the click router has to budget for.
    ///
    /// Set via `ext_panel_input_active`, with the input text left empty, so
    /// the chrome row appears without `Engine::ext_panel_filter_matches`
    /// also filtering rows out from under the assertions.
    pub search_input_visible: bool,
    /// Pre-seed a panel-hover popup on the item at this flat index, with
    /// this markdown body — scenario 5.
    ///
    /// Seeded rather than produced by a live 350 ms dwell: the popup only
    /// materialises from `Engine::poll_panel_hover`, which needs a
    /// wall-clock idle tick that `quadraui::testing::ConformanceDriver` does
    /// not expose (`tick()` is inherent on `TuiDriver` alone). Same approach
    /// the existing #1087 regression tests on both backends take; what this
    /// scenario asserts is the half that was actually broken — where the
    /// popup *paints* relative to the row it names.
    pub hover: Option<(usize, String)>,
    /// Drive `Engine::ext_panel_reveal_item` for this query before the
    /// first frame — scenario 6. The `(section, query)` pair is what
    /// `Engine::apply_plugin_ctx`'s `panel_reveal_request` arm passes for a
    /// `panel.reveal` call from Lua.
    pub reveal: Option<(String, String)>,
}

impl PluginPanelFixture {
    /// The unscrolled baseline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Scroll the panel so `SEC_COMMITS`' header and both commits sit near
    /// the top of the viewport and `SEC_SUMMARY`'s header has scrolled off
    /// — scenario 3, and #499's real complaint (only the *top* section
    /// header ever toggled) re-stated as "section 2..n must work too".
    pub fn scrolled(mut self) -> Self {
        self.scroll_top = FLAT_SEC_COMMITS - 3;
        self
    }

    /// Show the search-input chrome row (scenario 4).
    pub fn with_search_input(mut self) -> Self {
        self.search_input_visible = true;
        self
    }

    /// Pre-seed a hover popup on `flat_idx` (scenario 5).
    pub fn with_hover(mut self, flat_idx: usize, body: &str) -> Self {
        self.hover = Some((flat_idx, body.to_string()));
        self
    }

    /// Drive a `panel.reveal` for `query` in `section` (scenario 6).
    pub fn with_reveal(mut self, section: &str, query: &str) -> Self {
        self.reveal = Some((section.to_string(), query.to_string()));
        self
    }
}

fn item(text: &str, id: &str) -> ExtPanelItem {
    ExtPanelItem {
        text: text.to_string(),
        id: id.to_string(),
        ..Default::default()
    }
}

/// Build the shared fixture engine: a genuine [`PanelRegistration`] with
/// three sections and populated `ext_panel_items`.
///
/// Deliberately **not** `engine_with_marketplace_as_ext_panel`: that
/// fixture leaves `ext_panels` empty, so `render::build_ext_panel_data`
/// returns `None` and the only thing that can paint is the marketplace
/// `SidebarSystem`. Everything here flows from the registration, exactly as
/// a live plugin's `vimcode.panel.register` + `panel.set_items` would.
pub fn engine_with_plugin_panel(fx: &PluginPanelFixture) -> Engine {
    let mut engine = Engine::new_for_test();
    engine.settings.use_nerd_fonts = Some(false);
    crate::icons::set_nerd_fonts(false);

    // A machine with real plugins installed would otherwise have its own
    // registrations in here (`Engine::new` loads `~/.config/vimcode/
    // plugins/`), and the assertions below would be measuring those.
    engine.ext_panels.clear();
    engine.ext_panels.insert(
        PANEL.to_string(),
        PanelRegistration {
            name: PANEL.to_string(),
            title: PANEL_TITLE.to_string(),
            icon: '\u{f113}',
            fallback_icon: Some('Z'),
            sections: vec![
                SEC_SUMMARY.to_string(),
                SEC_COMMITS.to_string(),
                SEC_EMPTY.to_string(),
            ],
        },
    );

    // ── Section 0: a badge+hint row, a separator, then filler. ──────────
    let mut summary = vec![
        ExtPanelItem {
            text: ROW_BRANCH.to_string(),
            id: "branch".to_string(),
            hint: "ahead 2".to_string(),
            badges: vec![ExtPanelBadge {
                text: "HEAD".to_string(),
                color: "green".to_string(),
            }],
            ..Default::default()
        },
        ExtPanelItem {
            text: String::new(),
            id: String::new(),
            is_separator: true,
            ..Default::default()
        },
    ];
    summary.extend((0..FILLER_ROWS).map(|i| item(&format!("f{i:02}ZQXW1090"), &format!("f{i}"))));
    engine
        .ext_panel_items
        .insert((PANEL.to_string(), SEC_SUMMARY.to_string()), summary);

    // ── Section 1: two expandable commits, one file child each. ─────────
    //
    // The leading empty-id separator is `git_log_panel.lua`'s real shape
    // and #1088's trap: `ext_panel_find_flat_index` used to accept
    // `item_id.starts_with(&id)` as a match, and an empty id is a prefix of
    // *every* hash — so every `panel.reveal` resolved to this row instead of
    // the commit it named. It has to live in the section the reveal
    // searches, not merely somewhere in the panel, or the trap is
    // unreachable and `reveal_selects_the_revealed_row` cannot fail.
    engine.ext_panel_items.insert(
        (PANEL.to_string(), SEC_COMMITS.to_string()),
        vec![
            ExtPanelItem {
                text: String::new(),
                id: String::new(),
                is_separator: true,
                ..Default::default()
            },
            ExtPanelItem {
                text: ROW_COMMIT_A.to_string(),
                id: ID_COMMIT_A.to_string(),
                expandable: true,
                expanded: true,
                ..Default::default()
            },
            ExtPanelItem {
                text: ROW_CHILD_A.to_string(),
                id: format!("{ID_COMMIT_A}:a"),
                parent_id: ID_COMMIT_A.to_string(),
                indent: 1,
                ..Default::default()
            },
            ExtPanelItem {
                text: ROW_COMMIT_B.to_string(),
                id: "d00dbeef1090bbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                expandable: true,
                expanded: true,
                ..Default::default()
            },
            ExtPanelItem {
                text: ROW_CHILD_B.to_string(),
                id: "d00dbeef1090bbbbbbbbbbbbbbbbbbbbbbbbbbbb:b".to_string(),
                parent_id: "d00dbeef1090bbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                indent: 1,
                ..Default::default()
            },
        ],
    );

    // ── Section 2: registered, populated with nothing. ──────────────────
    engine
        .ext_panel_items
        .insert((PANEL.to_string(), SEC_EMPTY.to_string()), Vec::new());

    engine.ext_panel_active = Some(PANEL.to_string());
    engine.ext_panel_has_focus = true;
    engine.ext_panel_scroll_top = fx.scroll_top;
    engine.ext_panel_input_active = fx.search_input_visible;
    // `ext_panel_focus_pending` is what `TuiShellApp::tick` consumes to put
    // the sidebar onto this panel — the same handoff
    // `Engine::apply_plugin_ctx` performs for a live `panel.reveal`.
    engine.ext_panel_focus_pending = Some(PANEL.to_string());
    // Raw `AppShell::toggle_sidebar`, never `Engine::toggle_sidebar` — the
    // latter persists to the developer's real session file (the same reason
    // `gtk::testing::sidebar_panel_clicks::panel_harness` avoids it).
    if !engine.app_shell.sidebar_visible() {
        engine.app_shell.toggle_sidebar();
    }
    // Start the shadow `AppShell` on a panel that is **not** the explorer.
    //
    // `AppShell::show_panel` is a silent no-op for an id that is not in its
    // own `panels` list, and an `ext:` id never is (plugin panels bypass
    // `AppShell` registration — `render::apply_activity_panel_switch`'s own
    // doc), so asking for `ext:<name>` here would leave the explorer active
    // and frame 0 would paint the explorer tree. That matters on the
    // shipped TUI shell specifically: `TuiShellApp::handle_mouse_event`'s
    // `TreeController` intercept claims any `MouseDown` inside the *cached*
    // `explorer_tree_rect` while `active_panel_is(PANEL_EXPLORER)`, and that
    // rect survives the switch to a plugin panel — so every click in this
    // fixture's panel silently went to the explorer instead. Exactly the
    // "not this test's bug to fix, just a trap it must not fall into" note
    // `tui_ext_panel_double_click_on_a_section_header_does_not_toggle_it`
    // already carries; mirrored here.
    //
    // Harmless for the `App`-based arms: `App::paint_sidebar_panel_rung`
    // resolves *which* panel to paint through `render::sidebar_owner`, and
    // an active `ext_panel_active` outranks `app_shell`'s own active id
    // there — so those arms still route through the `ext:` paint arm.
    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));

    if let Some((section, query)) = &fx.reveal {
        engine.ext_panel_reveal_item(PANEL, section, query);
    }
    if let Some((flat_idx, body)) = &fx.hover {
        let item_id = engine
            .ext_panel_flat_to_section(*flat_idx)
            .and_then(|(si, item_idx)| {
                if item_idx == usize::MAX {
                    return None;
                }
                let reg = engine.ext_panels.get(PANEL)?;
                let section = reg.sections.get(si)?.clone();
                let items = engine.ext_panel_items.get(&(PANEL.to_string(), section))?;
                items.get(item_idx).map(|i| i.id.clone())
            })
            .unwrap_or_default();
        engine.show_panel_hover(PANEL, &item_id, *flat_idx, body);
    }
    engine
}

// ── Deliverable 2: the scenarios ────────────────────────────────────────

/// The painted bounds of the first text run containing `needle`, or a
/// panic naming everything that *was* painted.
///
/// Never a hardcoded coordinate — the same rule every body in
/// `crate::harness` follows.
pub fn painted_bounds<D: ConformanceDriver>(driver: &D, needle: &str) -> quadraui::Rect {
    driver
        .inventory()
        .text_runs()
        .iter()
        .find(|r| r.text.contains(needle))
        .map(|r| r.bounds)
        .unwrap_or_else(|| {
            panic!(
                "plugin panel: {needle:?} is not painted — painted runs were {:?}",
                driver
                    .inventory()
                    .text_runs()
                    .iter()
                    .map(|r| r.text.clone())
                    .collect::<Vec<_>>()
            )
        })
}

/// Click a point guaranteed to be somewhere harmless, purely to break the
/// backend's `DoubleClickDetector` position match before the next probe.
///
/// Every backend folds two same-position `MouseDown`s in quick succession
/// into a `DoubleClick` (quadraui#592), and `Engine::handle_ext_panel_
/// double_click` has no section-header case — so without this, a sweep's
/// restore click silently no-ops and later samples toggle from an unknown
/// baseline. `crate::harness`'s own #971/#983 sweeps solve it the same way
/// (they click a window corner); this aims at the panel's own *empty*
/// section header instead, which is inside the surface under test, is
/// guaranteed painted, and whose toggle changes nothing visible because the
/// section has no rows.
fn break_double_click<D: ConformanceDriver + DriverInput>(driver: &mut D) {
    let b = painted_bounds(driver, SEC_EMPTY);
    driver.click(b.x + b.width / 2.0, b.y + b.height / 2.0);
}

/// Sweep every point in `needle`'s painted vertical band and assert each
/// one resolves to `needle`'s own row — scenarios 1 and 2.
///
/// `probe` is painted text that disappears when `needle`'s row is toggled
/// (a section's first item for a header sweep; a commit's file child for an
/// item sweep) and `witness` is painted text that must be **unaffected** —
/// the "and no other" half of the property, which a fingerprint on `probe`
/// alone cannot see.
///
/// Built on [`crate::harness::sweep_hit_band_integrity_resetting`] rather
/// than the plain sweep for the reason that helper's own doc gives: the
/// restore click is not self-cancelling here (see [`break_double_click`]).
/// The `setup` closure re-establishes the expanded baseline through the
/// panel's *own* painted centre click and **asserts it worked**, so a sweep
/// whose restore silently fails fails loudly instead of reporting five
/// agreeing-but-meaningless samples.
pub fn sweep_row_hit_band<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    needle: &str,
    probe: &str,
    witness: &str,
    samples: usize,
) {
    assert!(
        driver.screen_has(needle) && driver.screen_has(probe) && driver.screen_has(witness),
        "precondition: {needle:?}, {probe:?} and {witness:?} must all be \
         painted before the sweep; painted runs were {:?}",
        driver
            .inventory()
            .text_runs()
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>()
    );

    // Sanity: one centre click on the target must actually hide `probe`.
    // Without this a sweep whose probe does nothing at all passes exactly
    // as cleanly as one that works — every sample agrees with the
    // (unchanged) baseline either way. #971 learned this the expensive way.
    let centre = painted_bounds(driver, needle);
    driver.click(
        centre.x + centre.width / 2.0,
        centre.y + centre.height / 2.0,
    );
    assert!(
        !driver.screen_has(probe),
        "sanity: a centre click on {needle:?} must collapse it and hide \
         {probe:?}; screen still shows it. Painted runs: {:?}",
        driver
            .inventory()
            .text_runs()
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        driver.screen_has(witness),
        "sanity: collapsing {needle:?} must not disturb {witness:?} — the \
         click reached more rows than the one it landed on"
    );

    crate::harness::sweep_hit_band_integrity_resetting(
        driver,
        needle,
        samples,
        |d| {
            // Order matters: break, restore, break. The click before the
            // restore and the click after it both sit at a *different*
            // position from the restore itself, so neither the previous
            // sample's click nor the next one can fold into it.
            break_double_click(d);
            if !d.screen_has(probe) {
                let b = painted_bounds(d, needle);
                d.click(b.x + b.width / 2.0, b.y + b.height / 2.0);
            }
            assert!(
                d.screen_has(probe),
                "sweep setup: {needle:?} must be back in its expanded \
                 baseline before the next sample, but {probe:?} is still \
                 hidden — the restore click did not land"
            );
            break_double_click(d);
        },
        |d| ConformanceDriver::screen_has(d, probe),
    );

    assert!(
        driver.screen_has(witness),
        "after the sweep, {witness:?} must still be painted — no sample may \
         have toggled a row other than {needle:?}"
    );
}

/// Scenario 1 — every sample inside `SEC_COMMITS`' painted header band
/// toggles *that* section.
///
/// Targets section index 1, not 0: #499's report was that only the top
/// section header ever responded, so a fixture that only ever probes the
/// first header cannot see the bug it is meant to catch.
pub fn section_header_hit_band<D: ConformanceDriver + DriverInput>(driver: &mut D) {
    // Witness is the *next* section's header, not a row in the section
    // above: it is the one thing that stays painted whether or not
    // `SEC_COMMITS` is collapsed, in both the scrolled and unscrolled
    // fixtures, so a click that collapsed more than the section it landed
    // on shows up here.
    sweep_row_hit_band(driver, SEC_COMMITS, ROW_COMMIT_A, SEC_EMPTY, 5);
}

/// Scenario 2 — every sample inside `ROW_COMMIT_A`'s painted row band
/// toggles *that* commit, and leaves `ROW_COMMIT_B`'s child alone.
///
/// The fingerprint is the commit's own file child appearing/disappearing:
/// a painted, text-level signal on every backend. A selection *highlight*
/// would need per-backend pixel access (`TuiDriver::style_at` /
/// `GtkDriver::pixel`) and so could not be one shared body — see
/// [`reveal_selects_the_revealed_row`], which takes the probe as a closure
/// for exactly that reason.
pub fn item_row_hit_band<D: ConformanceDriver + DriverInput>(driver: &mut D) {
    sweep_row_hit_band(driver, ROW_COMMIT_A, ROW_CHILD_A, ROW_CHILD_B, 5);
}

/// Scenario 5 — a hover card must paint *beside the row it names*, inside
/// the viewport, with the panel scrolled.
///
/// #1087: `hover.item_index` is a **flat** index across the whole panel
/// (`route_sidebar_hover`'s `ExtPanel` arm computes `ext_panel_scroll_top +
/// row`); the anchor code added the scroll offset back in but never
/// subtracted it out again, so a scrolled panel anchored the card dozens of
/// rows below the hovered item — off the bottom of the viewport entirely.
///
/// Reads only painted geometry: the hovered row's own bounds and the
/// popup's own body text bounds, both located by `find_bounds`, never
/// `engine.panel_hover.item_index` (which was correct the whole time).
/// `row_pitch` is the painted height of the hovered row — the natural unit
/// for "adjacent", and one that works unchanged in cells (TUI) or pixels
/// (GTK/macOS).
pub fn hover_card_anchors_beside_the_hovered_row<D: ConformanceDriver>(
    driver: &D,
    hovered: &str,
    popup_needle: &str,
    viewport_h: f32,
) {
    let row = painted_bounds(driver, hovered);
    let popup = painted_bounds(driver, popup_needle);
    let row_pitch = row.height.max(1.0);

    assert!(
        (popup.y - row.y).abs() <= row_pitch * 3.0,
        "the hover card must anchor next to the row it names: {hovered:?} \
         painted at y={}, but the card's body {popup_needle:?} painted at \
         y={} — {} row-heights away (#1087 anchored it at the raw flat \
         index, dozens of rows down)",
        row.y,
        popup.y,
        (popup.y - row.y).abs() / row_pitch,
    );
    assert!(
        popup.y >= 0.0 && popup.y + popup.height <= viewport_h,
        "the hover card must land inside the {viewport_h}-unit viewport, \
         not be pushed off it; card band was [{}, {}]",
        popup.y,
        popup.y + popup.height,
    );
}

/// Scenario 6 — `panel.reveal` must put the painted selection highlight on
/// the row carrying the queried id.
///
/// `is_highlighted` is supplied per backend (`TuiDriver::style_at`,
/// `GtkDriver::pixel`) because "is this row painted as selected" has no
/// text-level expression — the same shape
/// [`crate::harness::sweep_hit_band_integrity`] uses for its `fingerprint`.
/// Asserting the *contrast* between the revealed row and an unrelated row
/// (rather than a specific colour) keeps this theme-independent.
///
/// #1088: `ext_panel_find_flat_index`'s three-way fuzzy id match treated
/// the empty-id separator row as a prefix of every hash, so a reveal landed
/// on the separator instead of the commit. The fixture keeps a leading
/// empty-id separator at the head of `SEC_COMMITS` — the section the reveal
/// actually searches — precisely so that trap stays reachable.
///
/// Takes `&mut D` and an `FnMut` probe, not `&D`/`Fn`: `GtkDriver::pixel`
/// needs `&mut self` (it re-reads the Cairo surface), so a shared-reference
/// signature would exclude the one backend whose probe is a real pixel
/// read.
pub fn reveal_selects_the_revealed_row<D: ConformanceDriver>(
    driver: &mut D,
    revealed: &str,
    unrelated: &str,
    mut is_highlighted: impl FnMut(&mut D, quadraui::Rect) -> bool,
) {
    let target = painted_bounds(driver, revealed);
    let other = painted_bounds(driver, unrelated);

    assert!(
        is_highlighted(driver, target),
        "panel.reveal must paint the selection highlight on {revealed:?} \
         (painted at y={}) — it is not highlighted",
        target.y
    );
    assert!(
        !is_highlighted(driver, other),
        "panel.reveal must highlight only the revealed row, but the \
         unrelated row {unrelated:?} (painted at y={}) is highlighted too",
        other.y
    );
}

/// Scenario 7 (#1236) — hovering a row near a section-header/item pitch
/// boundary resolves the hover card to the row **actually painted under the
/// pointer**, not one a uniform-row-height formula guesses at.
///
/// `route_sidebar_hover`'s `ExtPanel` arm used to hit-test through
/// `SidebarBodyGeometry::content_row`, which divides by one `row_h` for the
/// whole body. GTK/macOS/Win pitch a tree's `Decoration::Header` rows at
/// `line_height * 1.2` and every other row at `line_height * 1.4`
/// (`quadraui::gtk::tree`'s own doc; `Engine::ext_panel_tree_layout`'s doc
/// restates it) — both taller than `content_row`'s assumed `row_h ==
/// line_height` — so the per-row error compounds with every row above the
/// probe point. By the time it reaches `target` (well below `SEC_COMMITS`'
/// own header, past `FILLER_ROWS` other rows), the accumulated drift is
/// large enough that the formula's row no longer matches the row painted at
/// that pixel.
///
/// Drives a **real** `UiEvent::MouseMoved` at `target`'s own painted centre
/// (never a hand-derived y) so the row-resolution logic under test —
/// [`ext_panel_hit_flat_index`](crate::render::ext_panel_hit_flat_index) —
/// runs for real. The 350ms dwell-to-paint delay itself is bypassed by
/// backdating `Engine::panel_hover_dwell`'s own `Instant` (rather than
/// hand-picking a flat index) and calling `Engine::poll_panel_hover`
/// directly — the same "no wall clock on `ConformanceDriver`" constraint
/// [`PluginPanelFixture::hover`] documents, applied without pre-seeding the
/// *routed* row, so the row the dwell fires on is still whatever the real
/// dispatch resolved. A second identical `MouseMoved` repaints — every
/// `MouseMoved` inside the sidebar body sets `draw_needed` regardless of
/// which row it lands on — so the assertions below read the frame that
/// `poll_panel_hover` actually populated.
///
/// Asserts only painted text (`screen_has`), never `engine.panel_hover`
/// itself — the CLAUDE.md "rendered output, not state" rule (#587/#592):
/// `panel_hover_dwell`/`poll_panel_hover`'s return value are read only to
/// drive the popup into existence without a real sleep, not as the proof.
pub fn hover_resolves_to_the_row_under_the_pointer<D: ConformanceDriver + DriverInput>(
    driver: &mut D,
    engine: &Rc<RefCell<Engine>>,
    target: &str,
    target_flat_idx: usize,
) {
    // Single short word, not `format!("... {target}")`: the popup's own
    // content column is narrow enough that a longer, multi-word body wraps
    // (or gets column-clipped) onto more than one painted text run —
    // `screen_has` matches within one run only, the same column-budget trap
    // this module's own header doc describes for `ROW_BRANCH`.
    let target_md = "HOVER1236OK";
    {
        let mut e = engine.borrow_mut();
        let target_id = e.resolve_panel_hover_item_id(PANEL, target_flat_idx);
        e.panel_hover_registry
            .insert((PANEL.to_string(), target_id), target_md.to_string());
        // A real, if tiny, delay — `poll_panel_hover` no-ops entirely when
        // this is 0 (`Engine::poll_panel_hover`'s own early return).
        e.settings.hover_delay = 1;
    }

    let centre = painted_bounds(driver, target);
    let move_to_target = || quadraui::UiEvent::MouseMoved {
        position: quadraui::Point::new(
            centre.x + centre.width / 2.0,
            centre.y + centre.height / 2.0,
        ),
        buttons: quadraui::ButtonMask::default(),
    };
    driver.dispatch(move_to_target());

    assert!(
        engine.borrow().panel_hover_dwell.is_some(),
        "hovering {target:?} (painted at y={}) must start dwell tracking on \
         some row — panel_hover_dwell is still None after the MouseMoved",
        centre.y
    );

    // Backdate the dwell instant the real dispatch just set, in place —
    // keeps whatever row `route_sidebar_hover` actually resolved, only
    // fast-forwarding past the wait.
    {
        let mut e = engine.borrow_mut();
        if let Some((panel, idx, _)) = e.panel_hover_dwell.take() {
            e.panel_hover_dwell = Some((
                panel,
                idx,
                std::time::Instant::now() - std::time::Duration::from_secs(1),
            ));
        }
    }
    let shown = engine.borrow_mut().poll_panel_hover();
    assert!(
        shown,
        "poll_panel_hover found no registered markdown for the row \
         hovering {target:?} (painted at y={}) actually resolved to — a \
         uniform-row-height formula would have drifted onto a row with no \
         (or a mismatched) id here (#1236)",
        centre.y
    );

    driver.dispatch(move_to_target());

    assert!(
        driver.screen_has(target_md),
        "hovering {target:?} must show *its own* hover card; painted texts \
         were {:?}",
        driver
            .inventory()
            .text_runs()
            .iter()
            .map(|r| r.text.clone())
            .collect::<Vec<_>>()
    );
}

#[cfg(test)]
mod tests;
