//! Backend-neutral **plugin-view** fixture + scenarios.
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
//! `inventory().text_runs()`. The fixture loads genuine Lua from a temp dir, so
//! the `render` callback, the Lua-table → [`crate::core::plugin_ui::PluginView`]
//! parse, the stored-callback dispatch and the paint are all in the measured
//! path.

use std::path::PathBuf;

use quadraui::testing::ConformanceDriver;

use crate::core::Engine;

/// The registered view's name (the `ext:` panel id suffix).
pub const VIEW: &str = "zqxw146view";
/// Its activity-bar / sidebar-header title.
pub const VIEW_TITLE: &str = "Zq146";

/// A second view registered with `sidebar = false`: a tab-only view with no
/// activity-bar icon. Exists alongside [`VIEW`] so a single fixture load
/// proves both halves of the `sidebar` option at once — [`VIEW`] still gets
/// an `ext:` activity-bar zone, this one does not.
pub const VIEW_NO_SIDEBAR: &str = "zqxw146viewtab";
/// [`VIEW_NO_SIDEBAR`]'s tab title.
pub const VIEW_NO_SIDEBAR_TITLE: &str = "Zq146Tab";
/// The single row [`VIEW_NO_SIDEBAR`] declares — enough to prove it still
/// paints as a tab even though it never reaches the activity bar.
pub const ROW_NO_SIDEBAR_LABEL: &str = "NsZQ146";

// Needles are deliberately short and distinctive. The TUI sidebar defaults to
// 20 columns (`render::hit_test_geometry::UnitProfile::cell().sidebar_width_lh`),
// and a `Form` row splits that between a label column and a value column — a
// needle that doesn't fit is a needle `screen_has` cannot see (the same
// column-budget trap `crate::harness::plugin_panel`'s module doc records for
// the tree panel). A row whose track genuinely cannot fit in 20 columns (the
// slider's) is measured through the editor-tab fixture instead
// (`engine_with_plugin_view_tab`/`tui_tab`/`gtk_tab`), which has the whole
// editor column's width to work with.

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
/// A second `text`-kind row, declared with an *empty* value — used by the
/// typing scenario. Growing an already-long value (`ROW_TEXT_VALUE`) by even
/// a couple of characters can cross the sidebar's label/value column split
/// (`quadraui::tui::form`'s `start_col > label_end + 1` guard) and suppress
/// the whole row's paint — the same column-budget trap this module's doc
/// comment already names, just triggered by *editing* rather than by an
/// already-too-long declared value. Starting empty leaves ample room.
pub const ROW_EMPTY_TEXT_LABEL: &str = "EtZQ146";
/// A `date`-kind row's label. Short on purpose: quadraui's inline `Form` row
/// paint for `FieldKind::DatePicker` only draws the formatted value when
/// `ix > label_right + 8.0` (`primitives/form.rs`), and the value here is a
/// full ISO8601 date (10 chars, longer than this fixture's other row values)
/// — the same column-budget trap [`ROW_EMPTY_TEXT_LABEL`] and [`LIST_DETAIL`]
/// already name, just needing a shorter *label* rather than a shorter value
/// to leave the column room the value needs.
pub const ROW_DATE_LABEL: &str = "DtZ";
/// That row's plugin-declared initial value (ISO8601 `YYYY-MM-DD`).
pub const ROW_DATE_INITIAL: &str = "2024-01-01";
/// The value painted after one `Right` keypress scrubs the focused day
/// forward without yet confirming it
/// (`FormController::handle_date_picker_key`'s `FocusMoved` arm — a
/// `Consumed` navigation event that never reaches the plugin). Confirms the
/// live scrub repaints the field even though [`COUNT_0`] must not have
/// bumped yet.
pub const ROW_DATE_SCRUBBED: &str = "2024-01-02";
/// The value painted after `Down` shifts the focused day back from
/// [`ROW_DATE_INITIAL`] by one week (`DatePicker::navigate`'s `"Down"` arm
/// is `focused.add_days(7)`).
pub const ROW_DATE_DOWN: &str = "2024-01-08";
/// The value painted after `PageDown` shifts the focused day forward from
/// [`ROW_DATE_INITIAL`] by one month (`DatePicker::navigate`'s
/// `"PageDown"` arm is `focused.add_months(1)`).
pub const ROW_DATE_PAGE_FORWARD: &str = "2024-02-01";
/// A `slider`-kind row's label. Short on purpose, same column-budget trap
/// [`ROW_DATE_LABEL`] already names: `tui::form::draw_field_slider` only
/// paints the `[====--------] value` track once
/// `start_col > label_end + 1`, and the track alone (12 cells + brackets +
/// the two-decimal value) needs roughly 28+ columns of row width to clear
/// that guard at all — more than the sidebar's 20-column TUI default has to
/// give even with the shortest practical label. Scenarios that need this row
/// painted use the editor-tab fixture (`tui_tab`/`gtk_tab`) instead of the
/// sidebar one, since the editor column has the width to spare.
pub const ROW_SLIDER_LABEL: &str = "SlZ";
/// That row's plugin-declared initial value, as `quadraui`'s
/// `draw_field_slider` paints it (`format!("{{value:.2}}")`).
pub const ROW_SLIDER_INITIAL: &str = "6.00";
/// The value painted after one `Enter`/`Space` steps the slider by its
/// declared `step` (3.0), clamped to `[min, max]` — there is no separate
/// scrub-then-confirm step like [`ROW_DATE_SCRUBBED`]'s, since
/// `quadraui::FormController` has no native keyboard path for a slider and
/// vimcode reaches the plugin directly.
pub const ROW_SLIDER_STEPPED: &str = "9.00";

/// Registered view names for each `ViewBody` kind's fixture.
pub const LIST_VIEW: &str = "zqxw1631list";
pub const TREE_VIEW: &str = "zqxw1631tree";
pub const TABLE_VIEW: &str = "zqxw1631table";
pub const TEXT_VIEW: &str = "zqxw1631text";

/// `List` fixture needles. `LIST_ROW0`/`LIST_ROW1` carry a `SEL`/`idl` tag
/// reflecting `Engine::ext_panel_selected` as of the last `on_event` —
/// row 0 is selected by default, so `LIST_ROW0_SEL`/`LIST_ROW1_IDLE` are
/// the fixture's initial painted state.
pub const LIST_TITLE: &str = "LTtl1631";
pub const LIST_ROW0_SEL: &str = "Lr0SEL1631";
pub const LIST_ROW0_IDLE: &str = "Lr0idl1631";
pub const LIST_ROW1_SEL: &str = "Lr1SEL1631";
pub const LIST_ROW1_IDLE: &str = "Lr1idl1631";
/// Short on purpose — `ListView`'s TUI rasteriser skips the right-aligned
/// detail span "when there isn't room past the main text" (`tui::list`'s
/// module doc), and the sidebar body in this fixture's viewport is narrow.
pub const LIST_DETAIL: &str = "D1";
/// Row count of the `List` fixture — more than any real viewport (sidebar or
/// tab) fits, so keyboard navigation to the last row needs a scroll to see it.
pub const LIST_OVERFLOW_COUNT: usize = 80;
/// The last row's painted text — the fixture's `render` gives every row
/// from index 2 onward a generic `"Lx{{i}}1631"` label except the very
/// last one, which gets this distinctive tag instead.
pub const LIST_LAST_ROW: &str = "LlastROW1631";

/// `Tree` fixture needles.
pub const TREE_FOLDER_CLOSED: &str = "Fd1631CLS";
pub const TREE_FOLDER_OPEN: &str = "Fd1631OPN";
pub const TREE_CHILD: &str = "Ch1631";

/// `Table` fixture needles.
pub const TABLE_COL_KEY: &str = "K1631";
pub const TABLE_COL_VALUE: &str = "V1631";
pub const TABLE_ROW0_KEY: &str = "Hk01631";
pub const TABLE_ROW0_VALUE: &str = "Hv01631";
pub const TABLE_ROW1_KEY: &str = "Hk11631";
pub const TABLE_ROW1_VALUE: &str = "Hv11631";
pub const TABLE_EDITED: &str = "EDITED1631";
/// Painted when `CellEdited{{row: 0, col: 1, ...}}` reaches the plugin — the
/// *second* editable column. Short on purpose — same column-budget constraint
/// that `LIST_DETAIL`'s doc comment names (columns narrow enough that a longer
/// needle gets ellipsis-truncated and never matches exactly).
pub const TABLE_EDITED_COL1: &str = "EDC1631";

/// Registered view name for a dedicated table fixture whose only job is
/// reporting `HeaderClicked` — kept separate from [`TABLE_VIEW`] so the
/// sort-cycle and resize scenarios above never observe the extra plugin
/// state this adds.
pub const HEADER_EVENT_VIEW: &str = "zqxw1631hevent";
pub const HEADER_EVENT_COL0: &str = "HeC01631";
pub const HEADER_EVENT_COL1: &str = "HeC11631";
/// Marker row's painted text before any header click reaches the plugin.
pub const HEADER_EVENT_NONE: &str = "HeN1631";
/// Painted once `on_event` has seen `HeaderClicked{{col: 0}}`.
pub const HEADER_EVENT_COL0_SEEN: &str = "He01631";
/// Painted once `on_event` has seen `HeaderClicked{{col: 1}}`.
pub const HEADER_EVENT_COL1_SEEN: &str = "He11631";

/// Registered view name for a dedicated table fixture with a declared
/// column `width` — same two-column shape as [`TABLE_VIEW`] but column 0
/// names an explicit `width`, so its painted layout can be compared against
/// [`TABLE_VIEW`]'s undeclared (purely `Flex`) default.
pub const WIDTH_VIEW: &str = "zqxw1631width";
pub const WIDTH_COL0: &str = "W01631";
pub const WIDTH_COL1: &str = "W11631";

/// `TextView` fixture needles — first and last of 50 generated lines, far
/// enough apart that only one is visible at a time on a narrow viewport.
pub const TEXT_FIRST_LINE: &str = "Ln0X1631";
pub const TEXT_LAST_LINE: &str = "Ln49X1631";

/// Lua source registering one view per `ViewBody` kind, appended
/// into the same fixture plugin the field-stack view above uses — one real
/// `PluginManager` load exercises every kind's parse + paint + event round
/// trip, per this module's "why a real `PluginManager`" doc.
fn plugin_source_view_bodies() -> String {
    format!(
        r#"
local list_sel = 0
vimcode.ui.register_view("{LIST_VIEW}", {{
  title = "Zq1631L", icon = "L", fallback_icon = "L",
  render = function()
    local function tag(i)
      if i == list_sel then return "SEL" else return "idl" end
    end
    local items = {{
      {{ id = "r0", text = "Lr0" .. tag(0) .. "1631" }},
      {{ id = "r1", text = "Lr1" .. tag(1) .. "1631", detail = "{LIST_DETAIL}" }},
    }}
    -- #1631 review: more rows than any real viewport fits, so a keyboard
    -- walk to the last row needs an actual scroll to see it.
    for i = 2, {LIST_OVERFLOW_COUNT} - 2 do
      items[#items + 1] = {{ id = "r" .. i, text = "Lx" .. i .. "1631" }}
    end
    items[#items + 1] = {{ id = "rlast", text = "{LIST_LAST_ROW}" }}
    return {{
      kind = "list",
      title = "{LIST_TITLE}",
      items = items,
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "ItemSelected" then list_sel = event.index end
  end,
}})

local tree_open = false
vimcode.ui.register_view("{TREE_VIEW}", {{
  title = "Zq1631T", icon = "T", fallback_icon = "T",
  render = function()
    return {{
      kind = "tree",
      nodes = {{
        {{ id = "folder1631",
           label = tree_open and "{TREE_FOLDER_OPEN}" or "{TREE_FOLDER_CLOSED}",
           expanded = tree_open,
           children = {{ {{ id = "child1631", label = "{TREE_CHILD}" }} }} }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "Expanded" then tree_open = true end
    if event.kind == "Collapsed" then tree_open = false end
  end,
}})

local last_edit = "none"
vimcode.ui.register_view("{TABLE_VIEW}", {{
  title = "Zq1631B", icon = "B", fallback_icon = "B",
  render = function()
    return {{
      kind = "table",
      columns = {{
        {{ title = "{TABLE_COL_KEY}", editable = true }},
        {{ title = "{TABLE_COL_VALUE}", editable = true }},
      }},
      rows = {{
        {{ id = "h0", cells = {{
           last_edit == "0:1" and "{TABLE_EDITED_COL1}" or "{TABLE_ROW0_KEY}",
           last_edit == "0:0" and "{TABLE_EDITED}" or "{TABLE_ROW0_VALUE}" }} }},
        {{ id = "h1", cells = {{ "{TABLE_ROW1_KEY}", "{TABLE_ROW1_VALUE}" }} }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "CellEdited" then
      last_edit = tostring(event.row) .. ":" .. tostring(event.col)
    end
  end,
}})

vimcode.ui.register_view("{TEXT_VIEW}", {{
  title = "Zq1631X", icon = "X", fallback_icon = "X",
  render = function()
    local lines = {{}}
    for i = 0, 49 do
      lines[#lines + 1] = "Ln" .. i .. "X1631"
    end
    return {{ kind = "text_view", text = table.concat(lines, "\n"), filetype = "json" }}
  end,
}})

local header_event_col = -1
vimcode.ui.register_view("{HEADER_EVENT_VIEW}", {{
  title = "Zq1631E", icon = "E", fallback_icon = "E",
  render = function()
    local marker
    if header_event_col == 0 then
      marker = "{HEADER_EVENT_COL0_SEEN}"
    elseif header_event_col == 1 then
      marker = "{HEADER_EVENT_COL1_SEEN}"
    else
      marker = "{HEADER_EVENT_NONE}"
    end
    return {{
      kind = "table",
      columns = {{
        {{ title = "{HEADER_EVENT_COL0}" }},
        {{ title = "{HEADER_EVENT_COL1}" }},
      }},
      rows = {{
        {{ id = "m0", cells = {{ marker, "{HEADER_EVENT_COL1}v" }} }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "HeaderClicked" then
      header_event_col = event.col
    end
  end,
}})

vimcode.ui.register_view("{WIDTH_VIEW}", {{
  title = "Zq1631W", icon = "W", fallback_icon = "W",
  render = function()
    return {{
      kind = "table",
      columns = {{
        {{ title = "{WIDTH_COL0}", width = 8 }},
        {{ title = "{WIDTH_COL1}" }},
      }},
      rows = {{
        {{ id = "w0", cells = {{ "{WIDTH_COL0}v", "{WIDTH_COL1}v" }} }},
      }},
    }}
  end,
}})

-- #1631 review: a real `:Command` around `vimcode.ui.open_view` for the
-- `List`-kind view, mirroring `Zq146OpenTab` above — exercises the
-- editor-tab-hosted body-kind click/scroll routing this review round added
-- (`render::route_plugin_view_body_event`), not just the sidebar's.
vimcode.command("Zq1631ListOpenTab", function()
  vimcode.ui.open_view("{LIST_VIEW}", {{ location = "tab" }})
end)

-- The same for the `table`-kind view, so the header-divider resize gesture can
-- be driven in an editor tab as well as the sidebar.
vimcode.command("Zq1631TableOpenTab", function()
  vimcode.ui.open_view("{TABLE_VIEW}", {{ location = "tab" }})
end)
"#
    )
}

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
local date_value = "{ROW_DATE_INITIAL}"
local slider_value = 6.0

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
        {{ type = "text",   id = "etx",  label = "{ROW_EMPTY_TEXT_LABEL}",
           value = "" }},
        {{ type = "date",   id = "dob",  label = "{ROW_DATE_LABEL}",
           value = date_value }},
        {{ type = "slider", id = "vol",  label = "{ROW_SLIDER_LABEL}",
           value = slider_value, min = 0.0, max = 10.0, step = 3.0 }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.widget_id == "send" and event.kind == "ButtonClicked" then
      count = count + 1
    elseif event.widget_id == "tls" and event.kind == "ToggleChanged" then
      count = count + 1
    elseif event.widget_id == "etx"
        and (event.kind == "TextChanged" or event.kind == "TextCommitted") then
      -- Every keystroke into the focused text field reaches here as a
      -- TextChanged event, and Enter reaches here once more as
      -- TextCommitted with the same (by-then-final) value.
      count = count + 1
    elseif event.widget_id == "dob" and event.kind == "DateChanged" then
      date_value = event.value
      count = count + 1
    elseif event.widget_id == "vol" and event.kind == "SliderChanged" then
      slider_value = event.value
      count = count + 1
    end
  end,
}})

-- #1627: a real `:Command` around `vimcode.ui.open_view`, so the editor-tab
-- fixture exercises the actual Lua binding rather than reaching straight for
-- `Engine::open_plugin_view_tab` — `open_view` needs a *live* engine loan
-- (`live_engine`'s doc), which only exists while a callback like this one is
-- running, not during the top-level load this whole file runs as.
vimcode.command("Zq146OpenTab", function()
  vimcode.ui.open_view("{VIEW}", {{ location = "tab" }})
end)

-- `sidebar = false`: registered as a view, routable through `open_view`, but
-- never harvested into `ext_panels` — no activity-bar icon.
vimcode.ui.register_view("{VIEW_NO_SIDEBAR}", {{
  title = "{VIEW_NO_SIDEBAR_TITLE}",
  sidebar = false,
  render = function(ctx)
    return {{
      id = "main",
      schema_version = 1,
      fields = {{
        {{ type = "label", id = "hdr", label = "{ROW_NO_SIDEBAR_LABEL}" }},
      }},
    }}
  end,
}})

vimcode.command("Zq146NoSidebarOpenTab", function()
  vimcode.ui.open_view("{VIEW_NO_SIDEBAR}", {{ location = "tab" }})
end)
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
    std::fs::write(dir.join("zqxw1631.lua"), plugin_source_view_bodies())
        .expect("write #1631 fixture plugin");
    dir
}

/// Load the fixture plugin from `dir` into `engine`: a genuine
/// `vimcode.ui.register_view` (+ `Zq146OpenTab` command) registration from
/// real Lua, harvested the same way `Engine::init_plugins` harvests a real
/// plugin directory. Shared by every fixture builder in this module — the
/// sidebar one ([`engine_with_plugin_view`]) and the editor-tab one
/// ([`engine_with_plugin_view_tab`]) — so the two can't drift on how the
/// plugin gets loaded, only on what they do with it afterward.
fn load_fixture_plugin(engine: &mut Engine, dir: &std::path::Path) {
    engine.settings.use_nerd_fonts = Some(false);
    crate::icons::set_nerd_fonts(false);
    engine.settings.plugins_enabled = true;
    // A machine with real plugins installed would otherwise contribute its own
    // registrations here (see `crate::harness::plugin_panel`'s same guard).
    engine.ext_panels.clear();
    engine.plugin_views.clear();

    let mut pm = crate::core::plugin::PluginManager::new().expect("lua state");
    pm.load_plugins_dir(dir, &[]);
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
    for view in [
        LIST_VIEW,
        TREE_VIEW,
        TABLE_VIEW,
        TEXT_VIEW,
        HEADER_EVENT_VIEW,
        WIDTH_VIEW,
    ] {
        assert!(
            engine.is_plugin_view(view),
            "fixture must register {view:?} as a view-backed panel"
        );
    }
    assert!(
        engine.is_plugin_view(VIEW_NO_SIDEBAR),
        "fixture must register {VIEW_NO_SIDEBAR:?} as a view-backed panel"
    );
    assert!(
        !engine.ext_panels.contains_key(VIEW_NO_SIDEBAR),
        "{VIEW_NO_SIDEBAR:?} was registered with sidebar = false; it must not \
         get an activity-bar panel"
    );
}

/// Build the fixture engine: a genuine `vimcode.ui.register_view` registration
/// loaded from real Lua, with its panel active and focused in the sidebar.
///
/// The `_dir` return keeps the temp directory alive for the caller's use; the
/// plugin is already loaded into the Lua state by the time this returns, so the
/// files are only needed for diagnosis.
pub fn engine_with_plugin_view() -> (Engine, PathBuf) {
    engine_with_named_plugin_view(VIEW)
}

/// Sidebar fixture for a `ViewBody`-kind view — same construction as
/// [`engine_with_plugin_view`], generalized to any of the fixture's
/// registered view names so each kind's tests can pick its own.
pub fn engine_with_named_plugin_view(view: &str) -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    load_fixture_plugin(&mut engine, &dir);

    engine.ext_panel_active = Some(view.to_string());
    engine.ext_panel_has_focus = true;
    // The activation path a live activity-bar click takes (`panel_focus` hook +
    // first `render` + selection parked on something `Enter` can act on).
    engine.on_ext_panel_focused(view);

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

/// Build a fixture engine with the same registered view, opened as an
/// **editor-area tab** instead of the sidebar — via the fixture's real
/// `Zq146OpenTab` `:Command`, which calls `vimcode.ui.open_view(VIEW,
/// {location = "tab"})`. Driving the real Lua command (rather than calling
/// `Engine::open_plugin_view_tab` directly) keeps the Lua binding itself in
/// the measured path.
pub fn engine_with_plugin_view_tab() -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    load_fixture_plugin(&mut engine, &dir);

    // `quadraui::AppShell::new` defaults `sidebar_visible: true` with the
    // Explorer panel active, which on a narrow test viewport leaves no room
    // for the editor column at all. Not part of what #1627 is testing — move
    // the active panel to Settings instead (mirrors `engine_with_plugin_
    // view`'s sidebar fixture, which does the same for the same reason).
    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));

    let before_tabs = engine.active_group().tabs.len();
    assert!(
        engine.plugin_run_command("Zq146OpenTab", ""),
        "fixture plugin must register the Zq146OpenTab command"
    );
    assert_eq!(
        engine.active_group().tabs.len(),
        before_tabs + 1,
        "vimcode.ui.open_view(..., {{location = \"tab\"}}) must open a new tab"
    );
    (engine, dir)
}

/// Tab twin of [`engine_with_plugin_view_tab`] for a body-kind view —
/// `command` is the fixture's own `:Command` name for opening `view` as a tab
/// (e.g. `"Zq1631ListOpenTab"` for [`LIST_VIEW`]).
pub fn engine_with_named_plugin_view_tab(view: &str, command: &str) -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    load_fixture_plugin(&mut engine, &dir);

    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));

    let before_tabs = engine.active_group().tabs.len();
    assert!(
        engine.plugin_run_command(command, ""),
        "fixture plugin must register the {command:?} command"
    );
    assert_eq!(
        engine.active_group().tabs.len(),
        before_tabs + 1,
        "{command:?} must open {view:?} as a new tab"
    );
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

/// Scenario — `sidebar = false` suppresses the activity-bar zone.
///
/// Reads the real `quadraui::compose::app_shell::AppShell` render path's zone
/// registry rather than `Engine` state: [`VIEW`] (`sidebar` defaults to
/// `true`) must still register an `ext:`-prefixed activity-bar zone;
/// [`VIEW_NO_SIDEBAR`] (`sidebar = false`) must not — it is only reachable
/// through `vimcode.ui.open_view(..., {location = "tab"})`.
pub fn sidebar_false_view_has_no_activity_bar_zone(driver: &impl ConformanceDriver) {
    let zones = driver.inventory().zones().to_vec();
    let has_zone = |view: &str| {
        let id = quadraui::WidgetId::new(crate::core::engine::sidebar::ext_panel_id(view));
        zones.iter().any(|z| z.id == id)
    };
    assert!(
        has_zone(VIEW),
        "{VIEW:?} is registered with the sidebar default (true); it must still \
         get an activity-bar zone; zones were {:?}",
        zones
            .iter()
            .map(|z| z.id.as_str().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        !has_zone(VIEW_NO_SIDEBAR),
        "{VIEW_NO_SIDEBAR:?} was registered with sidebar = false; it must not \
         get an activity-bar zone"
    );
}

/// Scenario — a `sidebar = false` view still opens and paints as a tab, with
/// its declared `title` — not the raw view name — as the tab's label.
///
/// `sidebar = false` only suppresses the activity-bar icon; `open_view(...,
/// {location = "tab"})` remains the view's entry point and still renders its
/// declared tree. The tab title comes from `Engine::plugin_view_title`,
/// which (for a tab-only view with no `ext_panels` entry) must read
/// `PluginManager::view_title` instead of falling back to the raw view name
/// (`VIEW_NO_SIDEBAR`, e.g. `"zqxw146viewtab"`).
pub fn tab_only_view_paints_its_declared_row(driver: &impl ConformanceDriver) {
    assert_paints(
        driver,
        &[ROW_NO_SIDEBAR_LABEL, VIEW_NO_SIDEBAR_TITLE],
        "sidebar = false view opened as a tab",
    );
}

/// Scenario 1 — the declared tree is what gets painted, in the sidebar.
///
/// Covers every row kind except `slider`, which is asserted separately by
/// [`declared_slider_row_is_painted`] — see that function's doc for why.
pub fn declared_tree_is_painted(driver: &impl ConformanceDriver) {
    assert_paints(
        driver,
        &[
            ROW_HEADER,
            ROW_TEXT_LABEL,
            ROW_TEXT_VALUE,
            ROW_TOGGLE,
            ROW_BUTTON,
            ROW_DATE_LABEL,
            ROW_DATE_INITIAL,
            COUNT_0,
        ],
        "the plugin-declared widget tree",
    );
}

/// Scenario 1b — the declared `slider` row is painted.
///
/// Kept separate from [`declared_tree_is_painted`] because the two backends
/// need different fixtures to exercise it: on TUI, `quadraui::tui::form`'s
/// column-budget guard means the slider's track only clears in the wider
/// editor-tab fixture (`tui_tab`; see [`ROW_SLIDER_LABEL`]'s doc). GTK has no
/// such guard — its sidebar fixture (`gtk`) paints the row fine — but the
/// scenario stays split rather than folded back into the sidebar's shared
/// assertion list, so both backends' callers run the identical scenario body.
pub fn declared_slider_row_is_painted(driver: &impl ConformanceDriver) {
    assert_paints(
        driver,
        &[ROW_SLIDER_LABEL, ROW_SLIDER_INITIAL],
        "the plugin-declared slider row",
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
/// `Tab` walks to the button (skipping the two `label` rows and stopping on
/// interactive ones), `Enter` activates it.
///
/// Field-to-field navigation is `Tab`/`Shift+Tab` rather than `j`/`k` because
/// selection starts on the first interactive row — the `text` field — and since
/// that field supports real text entry, `j`/`k` are literal characters to type
/// into it rather than navigation commands.
pub fn keyboard_activation_reaches_the_plugin_handler(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted runs \
         were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(COUNT_1),
        "Enter on the focused button row must reach the plugin's on_event \
         handler and repaint ({COUNT_1:?}); painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 4 — typing into a focused `Text` field inserts characters and
/// paints a caret; the plugin sees `TextChanged` per keystroke and
/// `TextCommitted` on Enter with the typed value.
///
/// Navigates to [`ROW_EMPTY_TEXT_LABEL`]'s field (declared with an empty
/// value) via three `Tab`s rather than typing into the already-focused `url`
/// field — see that constant's doc for why growing [`ROW_TEXT_VALUE`] would
/// risk tripping the sidebar's column-budget suppression.
pub fn typing_edits_the_focused_text_field(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted runs \
         were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.type_char('!');
    driver.type_char('?');
    assert!(
        driver.screen_has("!?"),
        "typing '!?' into the focused (empty) text field must insert it \
         into the painted value; painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has("n2ZQ146"),
        "each keystroke must reach the plugin's on_event handler as a \
         TextChanged event, bumping the shared counter twice; painted runs \
         were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has("n3ZQ146"),
        "Enter must commit the field (TextCommitted), bumping the counter a \
         third time; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 4b — a focused `date` field scrubs its live day with
/// `Left`/`Right`/`Up`/`Down`/`PageUp`/`PageDown`, repainting on every
/// keystroke, but only hands the plugin a
/// [`crate::core::plugin_ui::ViewEventKind::DateChanged`] once `Enter`
/// confirms the scrubbed day — `quadraui`'s
/// `FormController::handle_date_picker_key` returns a plain `Consumed`
/// (not a `FormAction`) for the scrub keys themselves, per its own "Date
/// picker fields" doc.
///
/// `Tab` x4 walks from the initially-focused `url` field past `tls`, `send`
/// and `etx` onto `dob`. Each navigation key is
/// pressed and then undone with its opposite (`Right`/`Left`,
/// `Down`/`Up`, `PageDown`/`PageUp`) to land back on
/// [`ROW_DATE_INITIAL`] before the final `Right` + `Enter`, so every arm
/// of `DatePicker::navigate` is exercised while the assertions stay
/// anchored to a small, known set of needles.
pub fn scrubbing_and_confirming_a_date_field_reaches_the_plugin_handler(
    driver: &mut impl ConformanceDriver,
) {
    assert!(
        driver.screen_has(ROW_DATE_INITIAL),
        "precondition: the date row must start at {ROW_DATE_INITIAL:?}; \
         painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Right);
    assert!(
        driver.screen_has(ROW_DATE_SCRUBBED),
        "Right on the focused date field must scrub the live day forward \
         and repaint it ({ROW_DATE_SCRUBBED:?}); painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(COUNT_0),
        "scrubbing alone (no Enter) must not yet reach the plugin's \
         on_event handler; painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Left);
    assert!(
        driver.screen_has(ROW_DATE_INITIAL),
        "Left must undo the prior Right, scrubbing the focused day back to \
         {ROW_DATE_INITIAL:?}; painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Down);
    assert!(
        driver.screen_has(ROW_DATE_DOWN),
        "Down on the focused date field must scrub the live day forward by \
         a week ({ROW_DATE_DOWN:?}); painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Up);
    assert!(
        driver.screen_has(ROW_DATE_INITIAL),
        "Up must undo the prior Down, scrubbing the focused day back to \
         {ROW_DATE_INITIAL:?}; painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::PageDown);
    assert!(
        driver.screen_has(ROW_DATE_PAGE_FORWARD),
        "PageDown on the focused date field must scrub the live day \
         forward by a month ({ROW_DATE_PAGE_FORWARD:?}); painted runs were \
         {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::PageUp);
    assert!(
        driver.screen_has(ROW_DATE_INITIAL),
        "PageUp must undo the prior PageDown, scrubbing the focused day \
         back to {ROW_DATE_INITIAL:?}; painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(COUNT_0),
        "none of the Left/Down/Up/PageDown/PageUp scrubs above must reach \
         the plugin's on_event handler; painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Right);
    assert!(
        driver.screen_has(ROW_DATE_SCRUBBED),
        "Right on the focused date field must scrub the live day forward \
         and repaint it ({ROW_DATE_SCRUBBED:?}); painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(ROW_DATE_SCRUBBED),
        "Enter must confirm the scrubbed day and keep it painted; painted \
         runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(COUNT_1),
        "Enter on the focused date field must reach the plugin's on_event \
         handler as DateChanged and repaint ({COUNT_1:?}); painted runs \
         were {:?}",
        painted(driver)
    );
}

/// Scenario 4c — a focused `slider` field's `Enter`/`Space` steps its value by
/// one `step` (clamped to `[min, max]`) and reaches the plugin's `on_event`
/// handler as a `SliderChanged` — unlike the date field above,
/// `quadraui::FormController` has no native keyboard path for a slider, so
/// vimcode computes the next value directly and there is no separate
/// scrub-then-confirm step.
///
/// `Tab` x5 walks from the initially-focused `url` field past `tls`, `send`,
/// `etx` and `dob` onto `vol`, the last declared field.
///
/// RED-verified: replacing `engine::plugins`'s `ViewFieldKind::Slider` match
/// arm with a no-op `return true` (matching the `Label`/`ReadOnly` arms,
/// which also decline to act on Enter/Space) makes the first assertion
/// below fail — the screen still shows the stale `"6.00"` instead of
/// `"9.00"` — while the rest of the suite, including every other
/// `plugin_view` scenario, stays green.
pub fn stepping_a_slider_field_reaches_the_plugin_handler(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(ROW_SLIDER_INITIAL),
        "precondition: the slider row must start at {ROW_SLIDER_INITIAL:?}; \
         painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted \
         runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(ROW_SLIDER_STEPPED),
        "Enter on the focused slider field must step its value by one \
         `step` and repaint it ({ROW_SLIDER_STEPPED:?}); painted runs were \
         {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(COUNT_1),
        "Enter on the focused slider field must reach the plugin's on_event \
         handler as SliderChanged and repaint ({COUNT_1:?}); painted runs \
         were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(ROW_SLIDER_INITIAL),
        "the stale slider value {ROW_SLIDER_INITIAL:?} must not still be \
         painted after the view re-rendered; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 5 — a view opened via `vimcode.ui.open_view(...,
/// {location = "tab"})` paints in the editor area, not the sidebar, and a
/// click on its button reaches the plugin's `on_event` handler exactly the
/// way a sidebar click does.
pub fn tab_paints_and_click_reaches_the_plugin_handler(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[ROW_HEADER, ROW_BUTTON, COUNT_0],
        "the editor-tab-hosted plugin view",
    );
    driver.click_text(ROW_BUTTON);
    assert!(
        driver.screen_has(COUNT_1),
        "clicking {ROW_BUTTON:?} in the editor-tab-hosted view must reach the \
         plugin's on_event handler; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 5b — a body-kind (`List`) view opened as an
/// editor-area tab reaches mouse clicks exactly like its sidebar twin
/// ([`list_paints_and_click_selects_the_right_index`]) does: a click on a
/// row selects it, reaching the plugin as `ItemSelected{{index}}`.
///
/// Fails against a `develop` without the review fix: `handle_mouse_click_
/// msg`'s tab-hosted arm called `render::handle_plugin_view_tab_ui_event`
/// unconditionally, which builds a field-stack `Form` with zero fields for
/// a body-kind view (`view.fields` is empty whenever `body` is `Some`) —
/// every click on the tab would be silently swallowed and
/// [`LIST_ROW1_SEL`] would never paint.
pub fn list_tab_click_selects_the_right_index(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[LIST_TITLE, LIST_ROW0_SEL, LIST_ROW1_IDLE],
        "the editor-tab-hosted list-kind view's initial paint",
    );
    driver.click_text(LIST_ROW1_IDLE);
    assert!(
        driver.screen_has(LIST_ROW1_SEL),
        "clicking row 1 in the editor-tab-hosted list must select it \
         (ItemSelected{{index: 1}}); painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(LIST_ROW0_IDLE),
        "row 0 must no longer be selected after row 1 was clicked; painted \
         runs were {:?}",
        painted(driver)
    );
}

/// Scenario 6 — closing an editor-tab-hosted plugin view's tab removes it,
/// exactly like any other tab.
///
/// `Tab`, `Tab` moves focus off the initially-focused `Text` field onto the
/// `Button` row first, so the following `:` falls through to Command-mode
/// entry instead of being typed literally into the field.
pub fn closing_the_tab_removes_it(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(ROW_HEADER),
        "precondition: the view must be painted before its tab is closed; \
         painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.type_text(":tabclose");
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        !driver.screen_has(ROW_HEADER),
        ":tabclose must remove the plugin-view tab; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 7 — a `kind = "list"` view paints its title, items and detail
/// text, and a click on a row reaches the plugin's `on_event` with that
/// row's index (proved by the SEL/idl tag flipping to the clicked row,
/// never the other one).
pub fn list_paints_and_click_selects_the_right_index(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[LIST_TITLE, LIST_ROW0_SEL, LIST_ROW1_IDLE, LIST_DETAIL],
        "the list-kind view's initial paint (row 0 selected by default)",
    );
    driver.click_text(LIST_ROW1_IDLE);
    assert!(
        driver.screen_has(LIST_ROW1_SEL),
        "clicking row 1 must select it (ItemSelected{{index: 1}}); painted runs \
         were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(LIST_ROW0_IDLE),
        "row 0 must no longer be selected after row 1 was clicked; painted \
         runs were {:?}",
        painted(driver)
    );
}

/// Scenario 8 — keyboard `Down` on a list moves selection to the right
/// index, exactly like a click does.
pub fn list_key_navigation_selects_the_right_index(driver: &mut impl ConformanceDriver) {
    assert!(driver.screen_has(LIST_ROW0_SEL));
    driver.press_named(quadraui::NamedKey::Down);
    assert!(
        driver.screen_has(LIST_ROW1_SEL),
        "Down must select row 1 (ItemSelected{{index: 1}}); painted runs were \
         {:?}",
        painted(driver)
    );
}

/// Scenario 8b — walking the selection all the way to the
/// last of [`LIST_OVERFLOW_COUNT`] rows (far more than any real viewport
/// fits) must scroll the list to keep the selection visible.
///
/// Fails against a `develop` without the review fix: `navigate_flat_
/// selection` moved `selected` on every `Down` but never adjusted
/// `scroll_top`/called `Engine::plugin_view_ensure_visible`, so the
/// highlighted row would walk off the bottom of the viewport with nothing
/// to compensate — [`LIST_LAST_ROW`] would never scroll into view.
pub fn list_down_key_navigation_keeps_selection_visible(driver: &mut impl ConformanceDriver) {
    assert!(driver.screen_has(LIST_ROW0_SEL));
    assert!(
        !driver.screen_has(LIST_LAST_ROW),
        "precondition: with {LIST_OVERFLOW_COUNT} rows in a narrow viewport, \
         the last row must not already be visible; painted runs were {:?}",
        painted(driver)
    );
    for _ in 0..LIST_OVERFLOW_COUNT - 1 {
        driver.press_named(quadraui::NamedKey::Down);
    }
    assert!(
        driver.screen_has(LIST_LAST_ROW),
        "walking Down to the last row must scroll the list to keep the \
         selection visible; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 9 — a `kind = "tree"` view paints its declared (collapsed)
/// node and not its child; `Enter` on the selected branch toggles it open
/// (`Expanded{{id}}`) and the child then paints.
pub fn tree_paints_collapsed_and_enter_expands_it(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(TREE_FOLDER_CLOSED),
        "precondition: the tree's root node paints collapsed; painted runs \
         were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(TREE_CHILD),
        "a collapsed branch's child must not be painted; painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(TREE_FOLDER_OPEN),
        "Enter on the selected branch must expand it (Expanded{{id}}); \
         painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(TREE_CHILD),
        "the expanded branch's child must now be painted; painted runs were \
         {:?}",
        painted(driver)
    );
}

/// Scenario 10 — a `kind = "table"` view paints its columns and rows; two
/// `Enter` presses on the default-selected, editable cell start and then
/// commit an edit, reaching the plugin's `on_event` as `CellEdited{{row: 0,
/// col: 0, ...}}` — the right cell, not some other one.
pub fn table_paints_and_enter_commits_a_cell_edit(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[
            TABLE_COL_KEY,
            TABLE_COL_VALUE,
            TABLE_ROW0_KEY,
            TABLE_ROW0_VALUE,
            TABLE_ROW1_KEY,
            TABLE_ROW1_VALUE,
        ],
        "the table-kind view's initial paint",
    );
    // First Enter starts editing row 0 / col 0 (the first editable column);
    // second Enter commits it unchanged, which is enough to prove the event
    // named the right (row, col) — the fixture only paints `TABLE_EDITED`
    // when `event.row == 0 and event.col == 0`.
    driver.press_named(quadraui::NamedKey::Enter);
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(TABLE_EDITED),
        "committing the edit must reach the plugin as CellEdited{{row: 0, \
         col: 0, ...}}; painted runs were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(TABLE_ROW0_VALUE),
        "the stale cell value must not still be painted after the commit; \
         painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario 10b — a table author who marks *more than one* column `editable`
/// (e.g. a "Key"/"Value" pair) can reach the second editable column too, not
/// just the first: `Right` moves the column selection before `Enter` starts
/// editing.
pub fn table_right_key_reaches_the_second_editable_column(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[TABLE_ROW0_KEY, TABLE_ROW0_VALUE],
        "the table-kind view's initial paint",
    );
    driver.press_named(quadraui::NamedKey::Right);
    driver.press_named(quadraui::NamedKey::Enter);
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(TABLE_EDITED_COL1),
        "Right then two Enters must start and commit an edit on column 1 \
         (the second editable column), reaching the plugin as \
         CellEdited{{row: 0, col: 1, ...}}; painted runs were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(TABLE_EDITED),
        "column 0 must be untouched — only column 1 was ever selected for \
         editing; painted runs were {:?}",
        painted(driver)
    );
}

/// Locate `needle`'s painted text run.
fn painted_run(driver: &impl ConformanceDriver, needle: &str) -> quadraui::event::Rect {
    driver
        .inventory()
        .text_runs()
        .iter()
        .find(|r| r.text.contains(needle))
        .map(|r| r.bounds)
        .unwrap_or_else(|| {
            panic!(
                "painted_run: {needle:?} not painted; painted runs were {:?}",
                painted(driver)
            )
        })
}

/// Column 1's header x-position, located by *order* rather than by text
/// content: the run immediately to the right of column 0's header
/// (`left_run`) on its own painted row. A table with a fixed total width
/// redistributes space between columns on every drag, so a wide enough
/// widen of column 0 can truncate column 1's header text down to one or two
/// characters — ordering survives that truncation; matching on the full
/// `TABLE_COL_VALUE` string would not.
fn next_header_run_x(driver: &impl ConformanceDriver, left_run: quadraui::event::Rect) -> f32 {
    driver
        .inventory()
        .text_runs()
        .iter()
        .filter(|r| {
            (r.bounds.y - left_run.y).abs() < 0.5 && r.bounds.x >= left_run.x + left_run.width - 0.5
        })
        .map(|r| r.bounds.x)
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or_else(|| {
            panic!(
                "next_header_run_x: nothing painted right of {left_run:?}; painted runs were {:?}",
                painted(driver)
            )
        })
}

/// Scenario 10c — dragging the divider between a table's first two header
/// columns widens the left column, which shifts column 1's painted header
/// right on screen, mirroring `crate::harness::drag_group_divider_resizes`'s
/// pattern for the window-group divider. The drag goes rightward: pulling a
/// divider right always grows the left column by construction
/// (`DataTableLayout::drag_divider`'s `target` is unbounded above), so this
/// holds regardless of how narrow the fixture's starting columns are —
/// unlike a leftward drag, which the min-width floor could clamp back up.
///
/// The drag's start point is still read from the cached layout
/// (`engine.plugin_view_table_layout`, or `plugin_view_tab_table_layout` for
/// `host == PluginViewHost::Tab`) — same as
/// [`table_header_click_sorts_the_painted_rows`]'s header click — because
/// that only ever drives the input, never judges the outcome:
/// the pass/fail assertion below reads nothing but `inventory().text_runs()`,
/// the same painted output a user would see. The drag distance is a small,
/// fixed amount rather than derived from the column's own width: a table
/// has a fixed total width, so widening column 0 shrinks every column after
/// it, and a large drag can squeeze column 1 down to nothing legible on a
/// narrow TUI viewport before the assertion ever gets to look at it.
///
/// Drives the sidebar-hosted copy of the table;
/// [`table_header_divider_drag_in_a_tab_resizes_the_column`] is the editor-tab
/// twin. The two hosts reach the engine through entirely separate routing — the
/// sidebar's via `App::try_route_sidebar_mouse_event`, an editor tab's via
/// `App::try_route_tab_plugin_table_drag` — so both need driving.
pub fn table_header_divider_drag_resizes_the_column<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    table_divider_drag_resizes_the_column(h, crate::core::engine::PluginViewHost::Sidebar);
}

/// Editor-tab twin of [`table_header_divider_drag_resizes_the_column`]: same
/// gesture and same assertion, against a table opened with `location = "tab"`,
/// whose `MouseMoved`/`MouseUp` follow-through is routed by
/// `App::try_route_tab_plugin_table_drag` instead of the sidebar's capture path.
pub fn table_header_divider_drag_in_a_tab_resizes_the_column<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    table_divider_drag_resizes_the_column(h, crate::core::engine::PluginViewHost::Tab);
}

fn table_divider_drag_resizes_the_column<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
    host: crate::core::engine::PluginViewHost,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    use crate::core::engine::PluginViewHost;
    assert_paints(
        &h.driver,
        &[TABLE_COL_KEY, TABLE_COL_VALUE],
        "the table-kind view's initial paint",
    );
    let (divider_x, y) = {
        let engine = h.engine.borrow();
        let cached = match host {
            PluginViewHost::Sidebar => engine.plugin_view_table_layout.borrow(),
            PluginViewHost::Tab => engine.plugin_view_tab_table_layout.borrow(),
        };
        let (rect, layout) = cached
            .as_ref()
            .expect("table_divider_drag_resizes_the_column: no frame painted yet");
        let col0 = &layout.columns[0];
        (
            rect.x + col0.x + col0.width,
            rect.y + layout.header_height / 2.0,
        )
    };
    let col0_run_before = painted_run(&h.driver, TABLE_COL_KEY);
    let col1_x_before = next_header_run_x(&h.driver, col0_run_before);
    h.driver.drag(divider_x, y, divider_x + 2.0, y);
    let col0_run_after = painted_run(&h.driver, TABLE_COL_KEY);
    let col1_x_after = next_header_run_x(&h.driver, col0_run_after);
    assert!(
        col1_x_after > col1_x_before,
        "dragging the header divider right must widen column 0, shifting \
         column 1's painted header right: before {col1_x_before}, after \
         {col1_x_after}; painted runs were {:?}",
        painted(&h.driver)
    );
}

/// Scenario 10d — clicking a table's column header cycles the sort for that
/// column, and the *painted* row order follows: unsorted → ascending (the
/// fixture's declared order already is ascending by key) → descending (the
/// two rows swap places on screen) → unsorted again.
///
/// The click lands on the middle of the header cell, read back from the
/// cached layout like [`table_header_divider_drag_resizes_the_column`] does,
/// which keeps it clear of the header *divider* (a resize) for any column
/// wider than twice quadraui's `DIVIDER_GRAB_PX` — comfortably true of the
/// fixture's columns on both backends.
pub fn table_header_click_sorts_the_painted_rows<D>(h: &mut crate::harness::ConformanceHarness<D>)
where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[TABLE_COL_KEY, TABLE_ROW0_KEY, TABLE_ROW1_KEY],
        "the table-kind view's initial paint",
    );
    assert!(
        h.driver.inventory().above(TABLE_ROW0_KEY, TABLE_ROW1_KEY),
        "precondition: the table starts in the plugin's declared row order, \
         {TABLE_ROW0_KEY:?} above {TABLE_ROW1_KEY:?}; painted runs were {:?}",
        painted(&h.driver)
    );
    let (header_x, header_y) = {
        let engine = h.engine.borrow();
        let cached = engine.plugin_view_table_layout.borrow();
        let (rect, layout) = cached
            .as_ref()
            .expect("table_header_click_sorts_the_painted_rows: no frame painted yet");
        let col0 = &layout.columns[0];
        (
            rect.x + col0.x + col0.width / 2.0,
            rect.y + layout.header_height / 2.0,
        )
    };

    h.driver.click(header_x, header_y);
    assert!(
        h.driver.inventory().above(TABLE_ROW0_KEY, TABLE_ROW1_KEY),
        "one header click sorts column 0 ascending, which for this fixture is \
         the order it already painted in ({TABLE_ROW0_KEY:?} above \
         {TABLE_ROW1_KEY:?}); painted runs were {:?}",
        painted(&h.driver)
    );

    h.driver.click(header_x, header_y);
    assert!(
        h.driver.inventory().above(TABLE_ROW1_KEY, TABLE_ROW0_KEY),
        "a second header click sorts column 0 descending, so {TABLE_ROW1_KEY:?} \
         must now paint above {TABLE_ROW0_KEY:?}; painted runs were {:?}",
        painted(&h.driver)
    );

    h.driver.click(header_x, header_y);
    assert!(
        h.driver.inventory().above(TABLE_ROW0_KEY, TABLE_ROW1_KEY),
        "a third header click clears the sort, restoring the plugin's declared \
         row order ({TABLE_ROW0_KEY:?} above {TABLE_ROW1_KEY:?}); painted runs \
         were {:?}",
        painted(&h.driver)
    );
}

/// Scenario 10e — a click on a table's column header reaches the plugin's
/// `on_event` as `HeaderClicked{{col}}`, carrying the clicked column's
/// index, independently of the engine's own internal sort-cycle overlay
/// [`table_header_click_sorts_the_painted_rows`] exercises. The dedicated
/// [`HEADER_EVENT_VIEW`] fixture's marker row is the round-trip signal: state
/// only `on_event` mutates, only `render` reads — the same shape every other
/// reachability scenario in this module uses.
pub fn table_header_click_reaches_the_plugin_handler<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[HEADER_EVENT_NONE],
        "the fixture's initial paint, before any header click",
    );

    let header_mid_x = |h: &crate::harness::ConformanceHarness<D>, col: usize| -> (f32, f32) {
        let engine = h.engine.borrow();
        let cached = engine.plugin_view_table_layout.borrow();
        let (rect, layout) = cached
            .as_ref()
            .expect("table_header_click_reaches_the_plugin_handler: no frame painted yet");
        let column = &layout.columns[col];
        (
            rect.x + column.x + column.width / 2.0,
            rect.y + layout.header_height / 2.0,
        )
    };

    let (col0_x, col0_y) = header_mid_x(h, 0);
    h.driver.click(col0_x, col0_y);
    assert!(
        h.driver.screen_has(HEADER_EVENT_COL0_SEEN),
        "a click on column 0's header must dispatch HeaderClicked{{col: 0}} to \
         the plugin and repaint the marker row as {HEADER_EVENT_COL0_SEEN:?}; \
         painted runs were {:?}",
        painted(&h.driver)
    );

    let (col1_x, col1_y) = header_mid_x(h, 1);
    h.driver.click(col1_x, col1_y);
    assert!(
        h.driver.screen_has(HEADER_EVENT_COL1_SEEN),
        "a click on column 1's header must dispatch HeaderClicked{{col: 1}} to \
         the plugin and repaint the marker row as {HEADER_EVENT_COL1_SEEN:?}; \
         painted runs were {:?}",
        painted(&h.driver)
    );
}

/// Scenario 10f — a column's declared `width` is honored as a `Fixed`
/// quadraui column width rather than the even `Flex` split every column got
/// before this field existed. [`WIDTH_VIEW`]'s column 0 declares a `width`
/// far below what an even `Flex` split of the same sidebar body would give
/// it, so column 1's painted header must land further left than it does in
/// [`TABLE_VIEW`]'s undeclared, purely proportional fixture — same viewport,
/// same column count, differing only in whether column 0 names a `width`.
pub fn table_declared_width_narrows_the_column<D>(
    narrow: &mut crate::harness::ConformanceHarness<D>,
    baseline: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &narrow.driver,
        &[WIDTH_COL0, WIDTH_COL1],
        "the declared-width fixture's initial paint",
    );
    assert_paints(
        &baseline.driver,
        &[TABLE_COL_KEY, TABLE_COL_VALUE],
        "the undeclared-width fixture's initial paint",
    );

    let col0_run_narrow = painted_run(&narrow.driver, WIDTH_COL0);
    let col1_x_narrow = next_header_run_x(&narrow.driver, col0_run_narrow);

    let col0_run_baseline = painted_run(&baseline.driver, TABLE_COL_KEY);
    let col1_x_baseline = next_header_run_x(&baseline.driver, col0_run_baseline);

    assert!(
        col1_x_narrow < col1_x_baseline,
        "declaring column 0's width far below its proportional Flex share \
         must narrow it, so column 1's header paints further left than the \
         undeclared-width fixture's: narrow {col1_x_narrow}, baseline \
         {col1_x_baseline}"
    );
}

/// Scenario 11 — a `kind = "text_view"` with more lines than fit the
/// viewport starts scrolled to the top and scrolls to the bottom on `End`.
pub fn text_view_scrolls_to_the_bottom_on_end(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(TEXT_FIRST_LINE),
        "precondition: the text view starts scrolled to the top; painted \
         runs were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(TEXT_LAST_LINE),
        "precondition: with 50 lines in a narrow viewport, the last line \
         must not be visible before scrolling; painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::End);
    assert!(
        driver.screen_has(TEXT_LAST_LINE),
        "End must scroll the text view to its last line; painted runs were \
         {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(TEXT_FIRST_LINE),
        "the first line must have scrolled out of view once the view jumped \
         to the bottom; painted runs were {:?}",
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

    fn tui_tab() -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_tab();
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    /// Sidebar fixture for a named `ViewBody`-kind view (TUI arm).
    fn tui_named(
        view: &str,
    ) -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_named_plugin_view(view);
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    /// Editor-tab fixture for a named body-kind view (TUI arm).
    fn tui_named_tab(
        view: &str,
        command: &str,
    ) -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_named_plugin_view_tab(view, command);
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    #[test]
    fn plugin_view_list_paints_and_click_selects_the_right_index_on_tui() {
        let (mut h, _dir) = tui_named(LIST_VIEW);
        list_paints_and_click_selects_the_right_index(&mut h.driver);
    }

    #[test]
    fn plugin_view_list_key_navigation_selects_the_right_index_on_tui() {
        let (mut h, _dir) = tui_named(LIST_VIEW);
        list_key_navigation_selects_the_right_index(&mut h.driver);
    }

    #[test]
    fn plugin_view_list_down_key_navigation_keeps_selection_visible_on_tui() {
        let (mut h, _dir) = tui_named(LIST_VIEW);
        list_down_key_navigation_keeps_selection_visible(&mut h.driver);
    }

    #[test]
    fn plugin_view_list_tab_click_selects_the_right_index_on_tui() {
        let (mut h, _dir) = tui_named_tab(LIST_VIEW, "Zq1631ListOpenTab");
        list_tab_click_selects_the_right_index(&mut h.driver);
    }

    #[test]
    fn plugin_view_tree_paints_collapsed_and_enter_expands_it_on_tui() {
        let (mut h, _dir) = tui_named(TREE_VIEW);
        tree_paints_collapsed_and_enter_expands_it(&mut h.driver);
    }

    #[test]
    fn plugin_view_table_paints_and_enter_commits_a_cell_edit_on_tui() {
        let (mut h, _dir) = tui_named(TABLE_VIEW);
        table_paints_and_enter_commits_a_cell_edit(&mut h.driver);
    }

    #[test]
    fn plugin_view_table_right_key_reaches_the_second_editable_column_on_tui() {
        let (mut h, _dir) = tui_named(TABLE_VIEW);
        table_right_key_reaches_the_second_editable_column(&mut h.driver);
    }

    #[test]
    fn plugin_view_table_header_divider_drag_resizes_the_column_on_tui() {
        let (mut h, _dir) = tui_named(TABLE_VIEW);
        table_header_divider_drag_resizes_the_column(&mut h);
    }

    #[test]
    fn plugin_view_table_tab_header_divider_drag_resizes_the_column_on_tui() {
        let (mut h, _dir) = tui_named_tab(TABLE_VIEW, "Zq1631TableOpenTab");
        table_header_divider_drag_in_a_tab_resizes_the_column(&mut h);
    }

    #[test]
    fn plugin_view_table_header_click_sorts_the_painted_rows_on_tui() {
        let (mut h, _dir) = tui_named(TABLE_VIEW);
        table_header_click_sorts_the_painted_rows(&mut h);
    }

    #[test]
    fn plugin_view_table_header_click_reaches_the_plugin_handler_on_tui() {
        let (mut h, _dir) = tui_named(HEADER_EVENT_VIEW);
        table_header_click_reaches_the_plugin_handler(&mut h);
    }

    #[test]
    fn plugin_view_table_declared_width_narrows_the_column_on_tui() {
        let (mut narrow, _narrow_dir) = tui_named(WIDTH_VIEW);
        let (mut baseline, _baseline_dir) = tui_named(TABLE_VIEW);
        table_declared_width_narrows_the_column(&mut narrow, &mut baseline);
    }

    #[test]
    fn plugin_view_text_view_scrolls_to_the_bottom_on_end_on_tui() {
        let (mut h, _dir) = tui_named(TEXT_VIEW);
        text_view_scrolls_to_the_bottom_on_end(&mut h.driver);
    }

    #[test]
    fn plugin_view_declared_tree_is_painted_on_tui() {
        let (h, _dir) = tui();
        declared_tree_is_painted(&h.driver);
    }

    #[test]
    fn plugin_view_declared_slider_row_is_painted_on_tui() {
        // The sidebar's 20-column TUI default is too narrow for the slider
        // row's track (`ROW_SLIDER_LABEL`'s doc comment) — the editor-tab
        // fixture has room for it.
        let (h, _dir) = tui_tab();
        declared_slider_row_is_painted(&h.driver);
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

    #[test]
    fn plugin_view_typing_edits_the_focused_text_field_on_tui() {
        let (mut h, _dir) = tui();
        typing_edits_the_focused_text_field(&mut h.driver);
    }

    #[test]
    fn plugin_view_scrubbing_and_confirming_a_date_field_reaches_the_handler_on_tui() {
        let (mut h, _dir) = tui();
        scrubbing_and_confirming_a_date_field_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_stepping_a_slider_field_reaches_the_handler_on_tui() {
        // Same reason as `plugin_view_declared_slider_row_is_painted_on_tui`:
        // the slider row only paints in the wider editor-tab fixture.
        let (mut h, _dir) = tui_tab();
        stepping_a_slider_field_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_tab_paints_and_click_reaches_the_handler_on_tui() {
        let (mut h, _dir) = tui_tab();
        tab_paints_and_click_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_closing_the_tab_removes_it_on_tui() {
        let (mut h, _dir) = tui_tab();
        closing_the_tab_removes_it(&mut h.driver);
    }

    #[test]
    fn plugin_view_sidebar_false_view_has_no_activity_bar_zone_on_tui() {
        let (h, _dir) = tui();
        sidebar_false_view_has_no_activity_bar_zone(&h.driver);
    }

    #[test]
    fn plugin_view_tab_only_view_paints_its_declared_row_on_tui() {
        let (h, _dir) = tui_named_tab(VIEW_NO_SIDEBAR, "Zq146NoSidebarOpenTab");
        tab_only_view_paints_its_declared_row(&h.driver);
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
    fn gtk_tab() -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_tab();
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    /// Sidebar fixture for a named `ViewBody`-kind view (GTK arm).
    #[cfg(feature = "gui")]
    fn gtk_named(
        view: &str,
    ) -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_named_plugin_view(view);
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    /// Editor-tab fixture for a named body-kind view (GTK arm).
    #[cfg(feature = "gui")]
    fn gtk_named_tab(
        view: &str,
        command: &str,
    ) -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_named_plugin_view_tab(view, command);
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_list_paints_and_click_selects_the_right_index_on_gtk() {
        let (mut h, _dir) = gtk_named(LIST_VIEW);
        list_paints_and_click_selects_the_right_index(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_list_key_navigation_selects_the_right_index_on_gtk() {
        let (mut h, _dir) = gtk_named(LIST_VIEW);
        list_key_navigation_selects_the_right_index(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_list_down_key_navigation_keeps_selection_visible_on_gtk() {
        let (mut h, _dir) = gtk_named(LIST_VIEW);
        list_down_key_navigation_keeps_selection_visible(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_list_tab_click_selects_the_right_index_on_gtk() {
        let (mut h, _dir) = gtk_named_tab(LIST_VIEW, "Zq1631ListOpenTab");
        list_tab_click_selects_the_right_index(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_tree_paints_collapsed_and_enter_expands_it_on_gtk() {
        let (mut h, _dir) = gtk_named(TREE_VIEW);
        tree_paints_collapsed_and_enter_expands_it(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_paints_and_enter_commits_a_cell_edit_on_gtk() {
        let (mut h, _dir) = gtk_named(TABLE_VIEW);
        table_paints_and_enter_commits_a_cell_edit(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_right_key_reaches_the_second_editable_column_on_gtk() {
        let (mut h, _dir) = gtk_named(TABLE_VIEW);
        table_right_key_reaches_the_second_editable_column(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_header_divider_drag_resizes_the_column_on_gtk() {
        let (mut h, _dir) = gtk_named(TABLE_VIEW);
        table_header_divider_drag_resizes_the_column(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_tab_header_divider_drag_resizes_the_column_on_gtk() {
        let (mut h, _dir) = gtk_named_tab(TABLE_VIEW, "Zq1631TableOpenTab");
        table_header_divider_drag_in_a_tab_resizes_the_column(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_header_click_sorts_the_painted_rows_on_gtk() {
        let (mut h, _dir) = gtk_named(TABLE_VIEW);
        table_header_click_sorts_the_painted_rows(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_header_click_reaches_the_plugin_handler_on_gtk() {
        let (mut h, _dir) = gtk_named(HEADER_EVENT_VIEW);
        table_header_click_reaches_the_plugin_handler(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_declared_width_narrows_the_column_on_gtk() {
        let (mut narrow, _narrow_dir) = gtk_named(WIDTH_VIEW);
        let (mut baseline, _baseline_dir) = gtk_named(TABLE_VIEW);
        table_declared_width_narrows_the_column(&mut narrow, &mut baseline);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_text_view_scrolls_to_the_bottom_on_end_on_gtk() {
        let (mut h, _dir) = gtk_named(TEXT_VIEW);
        text_view_scrolls_to_the_bottom_on_end(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_declared_tree_is_painted_on_gtk() {
        let (h, _dir) = gtk();
        declared_tree_is_painted(&h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_declared_slider_row_is_painted_on_gtk() {
        // Unlike TUI, GTK's `form.rs` has no column-budget guard gating the
        // slider track (it lays out pixel widths, not fixed character
        // columns), and the sidebar fixture (`gtk`) has been confirmed to
        // paint this row — so this uses `gtk` rather than `gtk_tab`.
        let (h, _dir) = gtk();
        declared_slider_row_is_painted(&h.driver);
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

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_typing_edits_the_focused_text_field_on_gtk() {
        let (mut h, _dir) = gtk();
        typing_edits_the_focused_text_field(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_scrubbing_and_confirming_a_date_field_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk();
        scrubbing_and_confirming_a_date_field_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_stepping_a_slider_field_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk_tab();
        stepping_a_slider_field_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_tab_paints_and_click_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk_tab();
        tab_paints_and_click_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_closing_the_tab_removes_it_on_gtk() {
        let (mut h, _dir) = gtk_tab();
        closing_the_tab_removes_it(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_sidebar_false_view_has_no_activity_bar_zone_on_gtk() {
        let (h, _dir) = gtk();
        sidebar_false_view_has_no_activity_bar_zone(&h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_tab_only_view_paints_its_declared_row_on_gtk() {
        let (h, _dir) = gtk_named_tab(VIEW_NO_SIDEBAR, "Zq146NoSidebarOpenTab");
        tab_only_view_paints_its_declared_row(&h.driver);
    }
}
