//! Per-lane registration of #1090's plugin-panel scenarios.
//!
//! Hand-written rather than `crate::backend_conformance!`-generated for
//! three reasons the macro cannot serve:
//!
//! 1. Each arm needs its **own** `KNOWN_BUGS` label (`…::gtk`, `…::macos`,
//!    …) baked into the body, and the macro expands one `$body` token
//!    stream verbatim into every arm with no way for that body to know
//!    which arm it is in — the same reason #984's chevron scenarios are
//!    hand-written (see `crate::harness::KNOWN_BUGS`).
//! 2. The `tui_prod` arm needs one `TuiDriver::tick()` before the first
//!    assertion (to let `TuiShellApp::tick` consume
//!    `ext_panel_focus_pending` and put the sidebar onto the plugin panel);
//!    `tick` is inherent on `TuiDriver`, not on `ConformanceDriver`.
//! 3. The macro has no `macos` arm.
//!
//! Everything *inside* each arm is the shared, backend-neutral scenario
//! body from the parent module — the per-arm code is harness construction
//! and nothing else.

use super::*;
use crate::harness::known_bug_gate;

const W: u32 = 1400;
const H: u32 = 900;
const TUI_W: u16 = 100;
const TUI_H: u16 = 30;

// ── tui_prod: the one lane that paints a `PanelRegistration` today ──────
//
// `TuiShellApp` -> `tui_main::panels::render_ext_panel` ->
// `render::ext_panel_to_tree_view` is the *only* path in this crate that
// turns a plugin's own sections into painted rows. Everything else goes
// through `App`'s `id.starts_with("ext:")` arm, which paints the
// marketplace (#1089).

fn prod(
    fx: &PluginPanelFixture,
) -> crate::harness::ConformanceHarness<
    quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
> {
    let mut h = crate::tui_main::testing::conformance_harness_prod(
        engine_with_plugin_panel(fx),
        TUI_W,
        TUI_H,
    );
    // `TuiShellApp::tick` is what consumes `ext_panel_focus_pending` and
    // sets `sidebar.ext_panel_name` — the same handoff a live
    // `panel.reveal` / activity-bar click performs.
    h.driver.tick();
    h.driver.render();
    h
}

/// Precondition for every `tui_prod` scenario below, asserted on its own so
/// a fixture that silently stops painting shows up as one obvious failure
/// rather than five confusing ones.
#[test]
fn plugin_panel_fixture_paints_all_three_sections_on_tui_prod() {
    let h = prod(&PluginPanelFixture::new());
    let painted: Vec<String> = h
        .driver
        .inventory()
        .text_runs()
        .iter()
        .map(|r| r.text.clone())
        .collect();
    for needle in [
        SEC_SUMMARY,
        SEC_COMMITS,
        SEC_EMPTY,
        ROW_BRANCH,
        ROW_COMMIT_A,
        ROW_CHILD_A,
        ROW_COMMIT_B,
        ROW_CHILD_B,
    ] {
        assert!(
            h.driver.screen_has(needle),
            "the plugin-panel fixture must paint {needle:?}; painted runs \
             were {painted:?}"
        );
    }
}

#[test]
fn plugin_panel_section_header_hit_band_on_tui_prod() {
    let mut h = prod(&PluginPanelFixture::new());
    section_header_hit_band(&mut h.driver);
}

#[test]
fn plugin_panel_item_row_hit_band_on_tui_prod() {
    let mut h = prod(&PluginPanelFixture::new());
    item_row_hit_band(&mut h.driver);
}

#[test]
fn plugin_panel_section_header_hit_band_scrolled_on_tui_prod() {
    let mut h = prod(&PluginPanelFixture::new().scrolled());
    assert!(
        !h.driver.screen_has(SEC_SUMMARY),
        "precondition: the scrolled fixture must have pushed the first \
         section's header off the top, so this probes a section the panel \
         is genuinely scrolled past"
    );
    section_header_hit_band(&mut h.driver);
}

#[test]
fn plugin_panel_item_row_hit_band_scrolled_on_tui_prod() {
    let mut h = prod(&PluginPanelFixture::new().scrolled());
    item_row_hit_band(&mut h.driver);
}

#[test]
fn plugin_panel_section_header_hit_band_with_search_input_on_tui_prod() {
    let mut h = prod(&PluginPanelFixture::new().with_search_input());
    section_header_hit_band(&mut h.driver);
}

#[test]
fn plugin_panel_item_row_hit_band_with_search_input_on_tui_prod() {
    let mut h = prod(&PluginPanelFixture::new().with_search_input());
    item_row_hit_band(&mut h.driver);
}

/// Scenario 4 belt-and-braces: the search-input row must actually be on
/// screen, otherwise the two tests above are silently re-running the
/// `chrome_h == 1` case.
#[test]
fn plugin_panel_search_input_chrome_row_is_painted_on_tui_prod() {
    let plain = prod(&PluginPanelFixture::new());
    let plain_top = painted_bounds(&plain.driver, SEC_SUMMARY).y;
    let with_input = prod(&PluginPanelFixture::new().with_search_input());
    let input_top = painted_bounds(&with_input.driver, SEC_SUMMARY).y;
    assert!(
        input_top > plain_top,
        "showing the panel's search input must push the body down by its \
         own chrome row (chrome_h 1 -> 2): first section header painted at \
         y={plain_top} without it and y={input_top} with it"
    );
}

#[test]
fn plugin_panel_hover_card_anchors_beside_the_row_on_tui_prod() {
    // Hover the first commit while the panel is scrolled — the exact shape
    // #1087 got wrong (flat index 16 vs on-screen row 4).
    let h = prod(
        &PluginPanelFixture::new()
            .scrolled()
            .with_hover(FLAT_COMMIT_A, "HOVERCARD1090 body"),
    );
    hover_card_anchors_beside_the_hovered_row(
        &h.driver,
        ROW_COMMIT_A,
        "HOVERCARD1090",
        TUI_H as f32,
    );
}

#[test]
fn plugin_panel_reveal_selects_the_revealed_row_on_tui_prod() {
    let h = prod(&PluginPanelFixture::new().with_reveal(SEC_COMMITS, REVEAL_QUERY));
    let bg_at = |d: &quadraui::tui::testing::TuiDriver<_>, r: quadraui::Rect| {
        d.style_at(r.x as u16, r.y as u16).map(|s| s.bg)
    };
    // "Highlighted" = this row's background differs from an ordinary,
    // definitely-unselected row's. Comparing against a painted reference
    // instead of a hardcoded colour keeps the assertion theme-independent.
    let reference = painted_bounds(&h.driver, ROW_CHILD_B);
    let reference_bg = bg_at(&h.driver, reference);
    reveal_selects_the_revealed_row(&h.driver, ROW_COMMIT_A, ROW_COMMIT_B, |d, r| {
        bg_at(d, r) != reference_bg
    });
}

// ── gtk / tui (shared `App`) / macos: red until #1089 ───────────────────
//
// See this module's parent doc. These are `known_bug_gate`d, not
// `#[cfg]`-skipped: the gate fails the build the moment a listed body
// starts passing, so #1089's fix cannot land without promoting them.

#[cfg(feature = "gui")]
#[test]
fn plugin_panel_section_header_hit_band_on_gtk() {
    known_bug_gate("plugin_panel_section_header_hit_band::gtk", || {
        let mut h = crate::gtk::testing::conformance_harness(
            engine_with_plugin_panel(&PluginPanelFixture::new()),
            W as i32,
            H as i32,
        );
        section_header_hit_band(&mut h.driver);
    });
}

#[cfg(feature = "gui")]
#[test]
fn plugin_panel_item_row_hit_band_on_gtk() {
    known_bug_gate("plugin_panel_item_row_hit_band::gtk", || {
        let mut h = crate::gtk::testing::conformance_harness(
            engine_with_plugin_panel(&PluginPanelFixture::new()),
            W as i32,
            H as i32,
        );
        item_row_hit_band(&mut h.driver);
    });
}

#[cfg(feature = "gui")]
#[test]
fn plugin_panel_section_header_hit_band_scrolled_on_gtk() {
    known_bug_gate(
        "plugin_panel_section_header_hit_band_scrolled::gtk",
        || {
            let mut h = crate::gtk::testing::conformance_harness(
                engine_with_plugin_panel(&PluginPanelFixture::new().scrolled()),
                W as i32,
                H as i32,
            );
            section_header_hit_band(&mut h.driver);
        },
    );
}

#[cfg(feature = "gui")]
#[test]
fn plugin_panel_section_header_hit_band_with_search_input_on_gtk() {
    known_bug_gate(
        "plugin_panel_section_header_hit_band_with_search_input::gtk",
        || {
            let mut h = crate::gtk::testing::conformance_harness(
                engine_with_plugin_panel(&PluginPanelFixture::new().with_search_input()),
                W as i32,
                H as i32,
            );
            section_header_hit_band(&mut h.driver);
        },
    );
}

#[cfg(feature = "gui")]
#[test]
fn plugin_panel_hover_card_anchors_beside_the_row_on_gtk() {
    known_bug_gate("plugin_panel_hover_card_anchors::gtk", || {
        let h = crate::gtk::testing::conformance_harness(
            engine_with_plugin_panel(
                &PluginPanelFixture::new()
                    .scrolled()
                    .with_hover(FLAT_COMMIT_A, "HOVERCARD1090 body"),
            ),
            W as i32,
            H as i32,
        );
        hover_card_anchors_beside_the_hovered_row(
            &h.driver,
            ROW_COMMIT_A,
            "HOVERCARD1090",
            H as f32,
        );
    });
}

#[cfg(feature = "gui")]
#[test]
fn plugin_panel_reveal_selects_the_revealed_row_on_gtk() {
    known_bug_gate("plugin_panel_reveal_selects_the_revealed_row::gtk", || {
        let h = crate::gtk::testing::conformance_harness(
            engine_with_plugin_panel(
                &PluginPanelFixture::new().with_reveal(SEC_COMMITS, REVEAL_QUERY),
            ),
            W as i32,
            H as i32,
        );
        let reference = painted_bounds(&h.driver, ROW_CHILD_B);
        let px = |d: &quadraui::gtk::testing::GtkDriver<_>, r: quadraui::Rect| {
            d.pixel((r.x - 4.0).max(0.0) as i32, (r.y + r.height / 2.0) as i32)
        };
        let reference_px = px(&h.driver, reference);
        reveal_selects_the_revealed_row(&h.driver, ROW_COMMIT_A, ROW_COMMIT_B, |d, r| {
            px(d, r) != reference_px
        });
    });
}

// The `tui` control arm wraps the same shared `App` GTK does, so it fails
// for the identical #1089 reason — kept so the shared shell's own regression
// is visible on a lane that needs no GTK dev libs.
#[test]
fn plugin_panel_section_header_hit_band_on_tui_shared_app() {
    known_bug_gate("plugin_panel_section_header_hit_band::tui", || {
        let mut h = crate::tui_main::testing::conformance_harness(
            engine_with_plugin_panel(&PluginPanelFixture::new()),
            TUI_W,
            TUI_H,
        );
        section_header_hit_band(&mut h.driver);
    });
}

#[test]
fn plugin_panel_item_row_hit_band_on_tui_shared_app() {
    known_bug_gate("plugin_panel_item_row_hit_band::tui", || {
        let mut h = crate::tui_main::testing::conformance_harness(
            engine_with_plugin_panel(&PluginPanelFixture::new()),
            TUI_W,
            TUI_H,
        );
        item_row_hit_band(&mut h.driver);
    });
}

#[cfg(all(feature = "macos", target_os = "macos"))]
#[test]
fn plugin_panel_section_header_hit_band_on_macos() {
    known_bug_gate("plugin_panel_section_header_hit_band::macos", || {
        let mut h = crate::macos::conformance_harness(
            engine_with_plugin_panel(&PluginPanelFixture::new()),
            W,
            H,
        );
        section_header_hit_band(&mut h.driver);
    });
}

#[cfg(all(feature = "macos", target_os = "macos"))]
#[test]
fn plugin_panel_item_row_hit_band_on_macos() {
    known_bug_gate("plugin_panel_item_row_hit_band::macos", || {
        let mut h = crate::macos::conformance_harness(
            engine_with_plugin_panel(&PluginPanelFixture::new()),
            W,
            H,
        );
        item_row_hit_band(&mut h.driver);
    });
}

#[cfg(all(feature = "macos", target_os = "macos"))]
#[test]
fn plugin_panel_hover_card_anchors_beside_the_row_on_macos() {
    known_bug_gate("plugin_panel_hover_card_anchors::macos", || {
        let h = crate::macos::conformance_harness(
            engine_with_plugin_panel(
                &PluginPanelFixture::new()
                    .scrolled()
                    .with_hover(FLAT_COMMIT_A, "HOVERCARD1090 body"),
            ),
            W,
            H,
        );
        hover_card_anchors_beside_the_hovered_row(
            &h.driver,
            ROW_COMMIT_A,
            "HOVERCARD1090",
            H as f32,
        );
    });
}
