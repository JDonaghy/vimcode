//! Backend-neutral **plugin-view** fixture + scenarios (#146).
//!
//! # What this covers
//!
//! `vimcode.ui.register_view` lets a Lua plugin declare a widget tree that
//! vimcode paints as a `quadraui::Form` and routes widget events back from. The
//! property under test is the whole round trip, measured only on painted output:
//!
//! > **What the plugin declared is what the sidebar paints, and activating a
//! > widget runs the plugin's `on_event` handler whose effect shows up in the
//! > next painted frame.**
//!
//! Every assertion below reads `ConformanceDriver::screen_has` /
//! `inventory().text_runs()`. Nothing asserts that `Engine::plugin_views` is
//! populated — that field was populated the whole time during development while
//! `App`'s `ext:` arm still painted tree rows, which is exactly the
//! state-vs-paint trap CLAUDE.md's rule (learned via #587/#592) names.
//!
//! # Why a real `PluginManager`, not a hand-built `PluginView`
//!
//! The fixture loads genuine Lua from a temp dir, so the `render` callback, the
//! Lua-table → [`crate::core::plugin_ui::PluginView`] parse, the stored-callback
//! dispatch (#1214's live-engine seam) and the paint are all in the measured
//! path. A hand-built `PluginView` would skip the three parts most likely to
//! break.

use std::path::PathBuf;

use quadraui::testing::ConformanceDriver;

use crate::core::Engine;

/// The registered view's name (the `ext:` panel id suffix).
pub const VIEW: &str = "zqxw146view";
/// Its activity-bar / sidebar-header title.
pub const VIEW_TITLE: &str = "Zq146";

// Needles are deliberately short and distinctive. The shared `App` sidebar is
// only ~30 columns wide on TUI at the viewport sizes below, and a `Form` row
// splits that between a label column and a value column — a needle that doesn't
// fit is a needle `screen_has` cannot see (the same column-budget trap
// `crate::harness::plugin_panel`'s module doc records for the tree panel).

/// A `label`-kind row: non-interactive, spans the whole row width.
pub const ROW_HEADER: &str = "HdZQ146";
/// A `text`-kind row's label.
pub const ROW_TEXT_LABEL: &str = "UrZQ146";
/// That row's plugin-declared value.
pub const ROW_TEXT_VALUE: &str = "vaZQ146";
/// A `button`-kind row's caption — the click target.
pub const ROW_BUTTON: &str = "SdZQ146";
/// A `toggle`-kind row's label.
pub const ROW_TOGGLE: &str = "TgZQ146";

/// The counter row's painted text before any activation, and after one.
///
/// Carried by a `label` row (not a value column) so the whole needle is
/// guaranteed room on a narrow sidebar row. This pair is the round-trip signal:
/// `n0…` → `n1…` can only happen if the click reached Lua's `on_event`, the
/// handler's mutation survived, `render` ran again, and the new tree painted.
pub const COUNT_0: &str = "n0ZQ146";
/// See [`COUNT_0`].
pub const COUNT_1: &str = "n1ZQ146";

/// The Lua plugin the fixture loads.
///
/// `count` is plugin-side state that only `on_event` mutates and only `render`
/// reads — the shape every real extension will have.
fn plugin_source() -> String {
    format!(
        r#"
local count = 0

vimcode.ui.register_view("{VIEW}", {{
  title = "{VIEW_TITLE}",
  icon = "V",
  fallback_icon = "V",
  render = function(ctx)
    return {{
      id = "main",
      schema_version = 1,
      fields = {{
        {{ type = "label",  id = "hdr",  label = "{ROW_HEADER}" }},
        {{ type = "label",  id = "cnt",  label = "n" .. count .. "ZQ146" }},
        {{ type = "text",   id = "url",  label = "{ROW_TEXT_LABEL}",
           value = "{ROW_TEXT_VALUE}" }},
        {{ type = "toggle", id = "tls",  label = "{ROW_TOGGLE}",
           value = count % 2 == 1 }},
        {{ type = "button", id = "send", label = "{ROW_BUTTON}" }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.widget_id == "send" and event.kind == "ButtonClicked" then
      count = count + 1
    elseif event.widget_id == "tls" and event.kind == "ToggleChanged" then
      count = count + 1
    end
  end,
}})
"#
    )
}

/// Write the fixture plugin into a fresh temp directory and return it.
///
/// Per-process-unique so two lanes running concurrently (the TUI and GTK arms
/// are separate `#[test]`s in the same binary) cannot race on the same path.
fn plugin_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vc_plugin_view_146_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture plugin dir");
    std::fs::write(dir.join("zqxw146.lua"), plugin_source()).expect("write fixture plugin");
    dir
}

/// Build the fixture engine: a genuine `vimcode.ui.register_view` registration
/// loaded from real Lua, with its panel active and focused in the sidebar.
///
/// The `_dir` return keeps the temp directory alive for the caller's use; the
/// plugin is already loaded into the Lua state by the time this returns, so the
/// files are only needed for diagnosis.
pub fn engine_with_plugin_view() -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    engine.settings.use_nerd_fonts = Some(false);
    crate::icons::set_nerd_fonts(false);
    engine.settings.plugins_enabled = true;
    // A machine with real plugins installed would otherwise contribute its own
    // registrations here (see `crate::harness::plugin_panel`'s same guard).
    engine.ext_panels.clear();
    engine.plugin_views.clear();

    let mut pm = crate::core::plugin::PluginManager::new().expect("lua state");
    pm.load_plugins_dir(&dir, &[]);
    assert!(
        pm.plugins.iter().all(|p| p.error.is_none()),
        "fixture plugin must load cleanly; errors: {:?}",
        pm.plugins
            .iter()
            .filter_map(|p| p.error.clone())
            .collect::<Vec<_>>()
    );
    // Mirror `Engine::init_plugins`' harvest: panel registrations into
    // `ext_panels` (what the activity bar and the `ext:` paint arm read), then
    // the manager itself (which seeds `plugin_views`).
    for (name, panel) in &pm.panels {
        engine
            .ext_panel_sections_expanded
            .insert(name.clone(), vec![true; panel.sections.len()]);
        engine.ext_panels.insert(name.clone(), panel.clone());
    }
    engine.set_plugin_manager(pm);
    assert!(
        engine.is_plugin_view(VIEW),
        "fixture must register {VIEW:?} as a view-backed panel"
    );

    engine.ext_panel_active = Some(VIEW.to_string());
    engine.ext_panel_has_focus = true;
    // The activation path a live activity-bar click takes (`panel_focus` hook +
    // first `render` + selection parked on something `Enter` can act on).
    engine.on_ext_panel_focused(VIEW);

    // Raw `AppShell::toggle_sidebar`, never `Engine::toggle_sidebar` — the
    // latter persists to the developer's real session file (same reason
    // `crate::harness::plugin_panel`'s fixture avoids it).
    //
    // This is not optional decoration: without it the shadow `AppShell` reports
    // `sidebar_visible() == false`. The *first* frame still paints the panel
    // (`App::paint_sidebar_panel_rung` resolves the active panel through
    // `render::sidebar_owner`, where `ext_panel_active` outranks `AppShell`), but
    // the shell's own mouse routing and post-event layout use `AppShell`, so any
    // click collapses the sidebar to zero width and every later frame paints
    // nothing there.
    if !engine.app_shell.sidebar_visible() {
        engine.app_shell.toggle_sidebar();
    }
    // Start the shadow `AppShell` on a panel that is not the explorer:
    // `AppShell::show_panel` silently ignores an `ext:` id (plugin panels bypass
    // `AppShell` registration), and leaving the explorer active lets the TUI
    // shell's `TreeController` intercept claim clicks inside the stale cached
    // `explorer_tree_rect`. Mirrors `plugin_panel`'s fixture, which documents the
    // same trap at length.
    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));
    (engine, dir)
}

// ── Shared scenario bodies ──────────────────────────────────────────────

/// Every painted text run, for failure messages — a missing needle is nearly
/// always a column-budget clamp or a "wrong panel painted" problem, and both are
/// obvious from the run list.
fn painted(driver: &impl ConformanceDriver) -> Vec<String> {
    driver
        .inventory()
        .text_runs()
        .iter()
        .map(|r| r.text.clone())
        .collect()
}

fn assert_paints(driver: &impl ConformanceDriver, needles: &[&str], what: &str) {
    for needle in needles {
        assert!(
            driver.screen_has(needle),
            "{what}: expected the plugin view to paint {needle:?}; painted runs \
             were {:?}",
            painted(driver)
        );
    }
}

/// Scenario 1 — the declared tree is what gets painted.
///
/// Fails against a `develop` without #146: the `ext:` panel arm paints
/// `ExtPanelItem` tree rows, and a view-backed panel has no items, so none of
/// these needles appear at all.
pub fn declared_tree_is_painted(driver: &impl ConformanceDriver) {
    assert_paints(
        driver,
        &[
            ROW_HEADER,
            ROW_TEXT_LABEL,
            ROW_TEXT_VALUE,
            ROW_TOGGLE,
            ROW_BUTTON,
            COUNT_0,
        ],
        "the plugin-declared widget tree",
    );
}

/// Scenario 2 — clicking a declared button runs the plugin's `on_event`, and the
/// effect shows up in the *painted* frame.
pub fn clicking_a_button_reaches_the_plugin_handler(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted runs \
         were {:?}",
        painted(driver)
    );
    driver.click_text(ROW_BUTTON);
    assert!(
        driver.screen_has(COUNT_1),
        "clicking {ROW_BUTTON:?} must reach the plugin's on_event handler and \
         the next painted frame must show its effect ({COUNT_1:?}); painted \
         runs were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(COUNT_0),
        "the stale counter row {COUNT_0:?} must not still be painted after the \
         view re-rendered; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 3 — keyboard activation of the focused widget, same round trip.
///
/// `j` walks to the button (skipping the two `label` rows and stopping on
/// interactive ones), `Enter` activates it.
pub fn keyboard_activation_reaches_the_plugin_handler(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted runs \
         were {:?}",
        painted(driver)
    );
    // Selection starts on the first interactive row (the `text` row); one `j`
    // reaches the toggle, a second reaches the button.
    driver.type_char('j');
    driver.type_char('j');
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(COUNT_1),
        "Enter on the focused button row must reach the plugin's on_event \
         handler and repaint ({COUNT_1:?}); painted runs were {:?}",
        painted(driver)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cell viewport for the TUI arm.
    const TUI_W: u16 = 120;
    const TUI_H: u16 = 30;
    /// Pixel viewport for the GTK arm.
    #[cfg(feature = "gui")]
    const W: i32 = 1400;
    #[cfg(feature = "gui")]
    const H: i32 = 900;

    fn tui() -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view();
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    #[test]
    fn plugin_view_declared_tree_is_painted_on_tui() {
        let (h, _dir) = tui();
        declared_tree_is_painted(&h.driver);
    }

    #[test]
    fn plugin_view_button_click_reaches_the_handler_on_tui() {
        let (mut h, _dir) = tui();
        clicking_a_button_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_keyboard_activation_reaches_the_handler_on_tui() {
        let (mut h, _dir) = tui();
        keyboard_activation_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    fn gtk() -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view();
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_declared_tree_is_painted_on_gtk() {
        let (h, _dir) = gtk();
        declared_tree_is_painted(&h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_button_click_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk();
        clicking_a_button_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_keyboard_activation_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk();
        keyboard_activation_reaches_the_plugin_handler(&mut h.driver);
    }
}
