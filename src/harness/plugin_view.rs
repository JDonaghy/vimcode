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
//! Each scenario's acceptance assertion reads `ConformanceDriver::screen_has`
//! / `inventory().text_runs()`; the multi-select gesture scenarios add a read
//! of the engine's selection set as a *secondary* check beside the painted
//! checkbox glyphs, never in place of one. The fixture loads genuine Lua from a temp dir, so
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
/// A `color`-kind row's label. `tui::form::draw_field_color_picker` paints a
/// swatch glyph plus the hex value once `start_col > label_end + 1`; unlike
/// the slider's track, the color row's total painted width (swatch + `#` +
/// 6 hex digits) comfortably clears that guard even at the sidebar's
/// 20-column default, so this row needs no editor-tab fixture split.
pub const ROW_COLOR_LABEL: &str = "ClZQ146";
/// A `toolbar`-kind row's label.
pub const ROW_TOOLBAR_LABEL: &str = "TbZQ146";
/// The caption of the `toolbar` row's single action button — the click
/// target for [`clicking_a_toolbar_button_reaches_the_plugin_handler`].
pub const ROW_TOOLBAR_BUTTON: &str = "GoZQ146";

/// Registered view name for the `section`-kind fixture: a labelled,
/// non-collapsible group (a `Section` header immediately followed by a
/// `buttons` row) plus a collapsible section with one member field.
pub const SECTION_VIEW: &str = "zqxw146section";
/// [`SECTION_VIEW`]'s activity-bar / sidebar-header title.
pub const SECTION_VIEW_TITLE: &str = "Zq146Sec";
/// The labelled group's non-collapsible `Section` header.
pub const ROW_GROUP_HEADER: &str = "GrZQ146";
/// The first button in the group header's horizontal control row.
pub const ROW_GROUP_BUTTON1: &str = "G1ZQ146";
/// The second button in the group header's horizontal control row.
pub const ROW_GROUP_BUTTON2: &str = "G2ZQ146";
/// The collapsible section's header label (sans disclosure glyph).
pub const ROW_COLLAPSIBLE_HEADER: &str = "CoZQ146";
/// [`ROW_COLLAPSIBLE_HEADER`] as painted while expanded — `render::
/// plugin_view_to_form` prefixes a collapsible section's label with a
/// disclosure glyph reflecting its current `collapsed` state.
pub const ROW_COLLAPSIBLE_HEADER_EXPANDED: &str = "▾ CoZQ146";
/// [`ROW_COLLAPSIBLE_HEADER`] as painted while collapsed.
pub const ROW_COLLAPSIBLE_HEADER_COLLAPSED: &str = "▸ CoZQ146";
/// The collapsible section's single member field — hidden whenever the
/// section is collapsed, per [`crate::core::plugin_ui::PluginView::
/// visible_field_indices`].
pub const ROW_COLLAPSIBLE_MEMBER: &str = "CmZQ146";

/// Registered view names for each `ViewBody` kind's fixture.
pub const LIST_VIEW: &str = "zqxw1631list";
pub const TREE_VIEW: &str = "zqxw1631tree";
pub const TABLE_VIEW: &str = "zqxw1631table";
pub const TEXT_VIEW: &str = "zqxw1631text";
pub const CHART_VIEW: &str = "zqxw2065chart";

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
/// Second marker row's painted text before any body-row click reaches the
/// plugin as `ItemSelected`.
pub const HEADER_EVENT_CELL_NONE: &str = "HeCN1631";
/// Painted once `on_event` has seen `ItemSelected{{col: Some(1)}}` from a
/// click on row `m1`'s column 1 — both of this fixture's columns are
/// read-only (neither declares `editable = true`), so this is the
/// read-only-column case the event's `col` field reports for.
pub const HEADER_EVENT_CELL_COL1_SEEN: &str = "HeMC1631";

/// Registered view name for a dedicated table fixture with a declared
/// column `width` — same two-column shape as [`TABLE_VIEW`] but column 0
/// names an explicit `width`, so its painted layout can be compared against
/// [`TABLE_VIEW`]'s undeclared (purely `Flex`) default.
pub const WIDTH_VIEW: &str = "zqxw1631width";
pub const WIDTH_COL0: &str = "W01631";
pub const WIDTH_COL1: &str = "W11631";

/// Registered view name for a dedicated table fixture exercising
/// `multi_select` and `footer` — kept separate from [`TABLE_VIEW`] so the
/// checkbox-prefix and pinned-footer row never change the column count or
/// row height the sort/resize/short-panel scenarios above were sized around.
pub const MULTI_SELECT_VIEW: &str = "zqxw1631multi";
pub const MULTI_COL: &str = "Mc1631";
pub const MULTI_ROW0: &str = "Mr01631";
pub const MULTI_ROW1: &str = "Mr11631";
pub const MULTI_FOOTER: &str = "MsFt1631";
/// Painted once `on_event` has seen `SelectionChanged{{indices: [0]}}` from
/// a click on row `m0`.
pub const MULTI_SELECTION_ROW0: &str = "MsR01631";
/// Painted before any `SelectionChanged` has reached the plugin.
pub const MULTI_SELECTION_NONE: &str = "MsN1631";

/// Registered view name for a dedicated `multi_select` table fixture with
/// three rows whose text sorts (ascending, by column 0) into display order
/// `[1, 0, 2]`, for exercising ctrl-click toggle and shift-click range
/// selection in both identity order and under a non-trivial sort, without
/// risking the simpler, already-passing [`MULTI_SELECT_VIEW`] fixture.
pub const RANGE_SELECT_VIEW: &str = "zqxw2064range";
pub const RANGE_COL: &str = "Rc2064";
/// Declared row 0 — sorts to display position 1 (`"Rb" > "Ra"`).
pub const RANGE_ROW0: &str = "Rb2064";
/// Declared row 1 — sorts to display position 0, lexically first.
pub const RANGE_ROW1: &str = "Ra2064";
/// Declared row 2 — sorts to display position 2, lexically last.
pub const RANGE_ROW2: &str = "Rz2064";
/// The range view's footer text before it has handled any
/// `SelectionChanged`: the plugin rewrites it to [`RANGE_BOUNDS_OK`] or
/// [`RANGE_BOUNDS_BAD`] from its `on_event`, which makes the bounds of the
/// indices the editor dispatched readable straight off the painted frame.
pub const RANGE_BOUNDS_NONE: &str = "Rn2064";
/// The range view's footer once every index of a dispatched
/// `SelectionChanged` indexed a row the plugin actually declared.
pub const RANGE_BOUNDS_OK: &str = "Rk2064";
/// The range view's footer once a dispatched `SelectionChanged` carried an
/// index past the end of the plugin's own row list.
pub const RANGE_BOUNDS_BAD: &str = "Rx2064";

/// Registered view name for a dedicated table fixture with one default
/// (left-aligned) column and one `align = "right"` column, for asserting
/// the painted x-offset of a right-aligned cell's text run is to the right
/// of its own column's left edge, unlike a left-aligned run's.
pub const ALIGN_VIEW: &str = "zqxw2064align";
pub const ALIGN_COL_LEFT: &str = "AlL2064";
pub const ALIGN_COL_RIGHT: &str = "AlR2064";
pub const ALIGN_CELL_LEFT: &str = "AlLv2064";
pub const ALIGN_CELL_RIGHT: &str = "AlRv2064";

/// `TextView` fixture needles — first and last of 50 generated lines, far
/// enough apart that only one is visible at a time on a narrow viewport.
pub const TEXT_FIRST_LINE: &str = "Ln0X1631";
pub const TEXT_LAST_LINE: &str = "Ln49X1631";

/// `Chart` fixture needles. Both series' labels are painted into the legend
/// row (`quadraui`'s `paint_legend`/`primitives::chart::paint_legend` draw
/// one swatch-plus-label text run per series), so each doubles as both a
/// paint-assertion needle and a `click_text` target for triggering
/// `LegendClicked`. `CHART_Y_LABEL_IDLE`/`_SEEN` is the `y_label` field,
/// which doubles as this fixture's round-trip marker — same local-state
/// convention as `list_sel`/`tree_open`/`last_edit` above.
///
/// These two drop the other fixtures' `1631` suffix convention: the legend
/// row only has room for a short swatch-plus-label run on the narrow TUI
/// sidebar viewport, and a 4-digit suffix pushed the second series' label
/// past the column budget and into truncation.
pub const CHART_SERIES0_LABEL: &str = "A631";
pub const CHART_SERIES1_LABEL: &str = "B631";
pub const CHART_X_LABEL: &str = "Cx1631";
pub const CHART_Y_LABEL_IDLE: &str = "Cy1631";
/// Painted once `on_event` has seen `LegendClicked{series_idx: 0}`.
pub const CHART_Y_LABEL_SEEN: &str = "CyS1631";
/// Painted once `on_event` has seen `DataPointClicked{series_idx: 0,
/// data_idx: 1}` — the point a bar/line-body click resolves to via
/// `ChartLayout::nearest_point`.
pub const CHART_POINT_SEEN: &str = "CpS1631";

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

local chart_legend_seen = false
local chart_point_seen = false
vimcode.ui.register_view("{CHART_VIEW}", {{
  title = "Zq2065C", icon = "C", fallback_icon = "C",
  render = function()
    return {{
      kind = "chart",
      chart_kind = "bar",
      series = {{
        {{ label = "{CHART_SERIES0_LABEL}", data = {{ 1, 2, 3 }} }},
        {{ label = "{CHART_SERIES1_LABEL}", data = {{ 3, 2, 1 }} }},
      }},
      x_label = chart_point_seen and "{CHART_POINT_SEEN}" or "{CHART_X_LABEL}",
      y_label = chart_legend_seen and "{CHART_Y_LABEL_SEEN}" or "{CHART_Y_LABEL_IDLE}",
      show_legend = true,
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "LegendClicked" and event.series_idx == 0 then
      chart_legend_seen = true
    elseif event.kind == "DataPointClicked" and event.series_idx == 0 and event.data_idx == 1 then
      chart_point_seen = true
    end
  end,
}})

local header_event_col = -1
local cell_event_col = -1
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
    local cell_marker
    if cell_event_col == 1 then
      cell_marker = "{HEADER_EVENT_CELL_COL1_SEEN}"
    else
      cell_marker = "{HEADER_EVENT_CELL_NONE}"
    end
    return {{
      kind = "table",
      columns = {{
        {{ title = "{HEADER_EVENT_COL0}" }},
        {{ title = "{HEADER_EVENT_COL1}" }},
      }},
      rows = {{
        {{ id = "m0", cells = {{ marker, "{HEADER_EVENT_COL1}v" }} }},
        {{ id = "m1", cells = {{ cell_marker, "{HEADER_EVENT_COL1}v2" }} }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "HeaderClicked" then
      header_event_col = event.col
    elseif event.kind == "ItemSelected" then
      cell_event_col = event.col
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

local ms_selected_marker = "{MULTI_SELECTION_NONE}"
vimcode.ui.register_view("{MULTI_SELECT_VIEW}", {{
  title = "Zq1631M", icon = "M", fallback_icon = "M",
  render = function()
    return {{
      kind = "table",
      multi_select = true,
      columns = {{ {{ title = "{MULTI_COL}" }} }},
      rows = {{
        {{ id = "m0", cells = {{ "{MULTI_ROW0}" }} }},
        {{ id = "m1", cells = {{ "{MULTI_ROW1}" }} }},
        {{ id = "m2", cells = {{ ms_selected_marker }} }},
      }},
      footer = {{ "{MULTI_FOOTER}" }},
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "SelectionChanged" then
      local has0 = false
      for _, idx in ipairs(event.indices) do
        if idx == 0 then has0 = true end
      end
      ms_selected_marker = has0 and "{MULTI_SELECTION_ROW0}" or "{MULTI_SELECTION_NONE}"
    end
  end,
}})

local range_row_count = 3
local range_bounds_marker = "{RANGE_BOUNDS_NONE}"
vimcode.ui.register_view("{RANGE_SELECT_VIEW}", {{
  title = "Zq2064R", icon = "R", fallback_icon = "R",
  render = function()
    return {{
      kind = "table",
      multi_select = true,
      columns = {{ {{ title = "{RANGE_COL}" }} }},
      rows = {{
        {{ id = "r0", cells = {{ "{RANGE_ROW0}" }} }},
        {{ id = "r1", cells = {{ "{RANGE_ROW1}" }} }},
        {{ id = "r2", cells = {{ "{RANGE_ROW2}" }} }},
      }},
      footer = {{ range_bounds_marker }},
    }}
  end,
  on_event = function(ctx, event)
    if event.kind == "SelectionChanged" then
      local out_of_range = false
      for _, idx in ipairs(event.indices) do
        if idx < 0 or idx >= range_row_count then out_of_range = true end
      end
      range_bounds_marker =
        out_of_range and "{RANGE_BOUNDS_BAD}" or "{RANGE_BOUNDS_OK}"
    end
  end,
}})

vimcode.ui.register_view("{ALIGN_VIEW}", {{
  title = "Zq2064A", icon = "A", fallback_icon = "A",
  render = function()
    return {{
      kind = "table",
      columns = {{
        {{ title = "{ALIGN_COL_LEFT}" }},
        {{ title = "{ALIGN_COL_RIGHT}", align = "right" }},
      }},
      rows = {{
        {{ id = "a0", cells = {{ "{ALIGN_CELL_LEFT}", "{ALIGN_CELL_RIGHT}" }} }},
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

-- The same for the `chart`-kind view, so the data-point click gesture can be
-- driven in an editor tab as well as the sidebar.
vimcode.command("Zq2065ChartOpenTab", function()
  vimcode.ui.open_view("{CHART_VIEW}", {{ location = "tab" }})
end)

-- A real `:Command` around `vimcode.ui.open_view` with `location = "dock"`,
-- so the bottom-panel-hosted fixture exercises the actual Lua binding
-- rather than reaching straight for `Engine::open_plugin_view_dock`.
vimcode.command("Zq2059TextOpenDock", function()
  vimcode.ui.open_view("{TEXT_VIEW}", {{ location = "dock" }})
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

/// The registered view's name for the dock's row-0 click-routing regression
/// fixture: a view whose *first* declared field is the clickable one.
///
/// [`VIEW`]'s own button ([`ROW_BUTTON`]) sits at field index 4, well inside
/// `BottomPanelZone::Content` — it cannot catch a routing bug confined to
/// `BottomPanelZone::Toolbar`, the zone a docked `Form`'s very first painted
/// row falls into (`render::route_bottom_panel_click`'s doc explains why).
/// [`ROW0_VIEW`] exists only to put a clickable field at index 0, so a dock
/// click there exercises that zone.
pub const ROW0_VIEW: &str = "zqxw2060row0";
/// [`ROW0_VIEW`]'s sole button — field index 0.
pub const ROW0_BUTTON: &str = "SdZQ2060";
/// [`ROW0_VIEW`]'s counter label before the button is clicked.
pub const ROW0_COUNT_0: &str = "n0ZQ2060";
/// See [`ROW0_COUNT_0`].
pub const ROW0_COUNT_1: &str = "n1ZQ2060";

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
        {{ type = "color",  id = "swt",  label = "{ROW_COLOR_LABEL}",
           value = '#ab12cd' }},
        {{ type = "toolbar", id = "tbr", label = "{ROW_TOOLBAR_LABEL}",
           buttons = {{ {{ id = "go", label = "{ROW_TOOLBAR_BUTTON}" }} }} }},
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
    elseif event.widget_id == "go" and event.kind == "ToolbarButtonClicked" then
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

-- A real `:Command` around `vimcode.ui.open_view` with `location = "dock"`
-- for {VIEW} — the `Form`-kind (default `body`) view, mirroring
-- `Zq2059TextOpenDock` above but exercising the dock's click-routing path
-- (`render::handle_plugin_view_dock_ui_event`) rather than its scroll-only
-- path, since a `Form` body accepts button/toggle/text clicks a `TextView`
-- body does not.
vimcode.command("Zq2060FormOpenDock", function()
  vimcode.ui.open_view("{VIEW}", {{ location = "dock" }})
end)

-- A second, minimal `Form`-kind view whose *first* declared field (index 0)
-- is the clickable button, unlike {VIEW} above, whose button sits at field
-- index 4. A docked `Form`'s row 0 falls in `BottomPanelZone::Toolbar`, not
-- `Content` (`render::route_bottom_panel_click`'s doc explains why), so a
-- regression confined to that zone needs the clickable field to be row 0
-- to be caught at all.
local row0_count = 0
vimcode.ui.register_view("{ROW0_VIEW}", {{
  title = "{ROW0_VIEW}",
  render = function()
    return {{
      id = "main",
      schema_version = 1,
      fields = {{
        {{ type = "button", id = "go0", label = "{ROW0_BUTTON}" }},
        {{ type = "label", id = "cnt0", label = "n" .. tostring(row0_count) .. "ZQ2060" }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.widget_id == "go0" and event.kind == "ButtonClicked" then
      row0_count = row0_count + 1
    end
  end,
}})

vimcode.command("Zq2060Row0OpenDock", function()
  vimcode.ui.open_view("{ROW0_VIEW}", {{ location = "dock" }})
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

-- A labelled, non-collapsible group (a `Section` header immediately
-- followed by a horizontal `buttons` row) plus a collapsible section with
-- one member field — `SectionToggled` is the only event this view's
-- `on_event` needs: the plugin stores the proposed `collapsed` value and
-- `render` echoes it back on the next frame.
local section_collapsed = false
vimcode.ui.register_view("{SECTION_VIEW}", {{
  title = "{SECTION_VIEW_TITLE}",
  icon = "S",
  fallback_icon = "S",
  render = function()
    return {{
      id = "main",
      schema_version = 1,
      fields = {{
        {{ type = "section", id = "grp", label = "{ROW_GROUP_HEADER}",
           collapsible = false }},
        {{ type = "buttons", id = "grpbtns", buttons = {{
           {{ id = "g1", label = "{ROW_GROUP_BUTTON1}" }},
           {{ id = "g2", label = "{ROW_GROUP_BUTTON2}" }},
        }} }},
        {{ type = "section", id = "col", label = "{ROW_COLLAPSIBLE_HEADER}",
           collapsible = true, collapsed = section_collapsed }},
        {{ type = "button", id = "mem", label = "{ROW_COLLAPSIBLE_MEMBER}" }},
      }},
    }}
  end,
  on_event = function(ctx, event)
    if event.widget_id == "col" and event.kind == "SectionToggled" then
      section_collapsed = event.collapsed
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
        MULTI_SELECT_VIEW,
        RANGE_SELECT_VIEW,
        ALIGN_VIEW,
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
        engine.is_plugin_view(SECTION_VIEW),
        "fixture must register {SECTION_VIEW:?} as a view-backed panel"
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

/// Dock twin of [`engine_with_plugin_view_tab`] — opens [`TEXT_VIEW`] into
/// the bottom-panel dock via the fixture's real `Zq2059TextOpenDock`
/// `:Command`, which calls `vimcode.ui.open_view(TEXT_VIEW, {location =
/// "dock"})`. Driving the real Lua command keeps the Lua binding itself in
/// the measured path, exactly like the tab fixture above.
pub fn engine_with_plugin_view_dock() -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    load_fixture_plugin(&mut engine, &dir);

    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));

    assert!(
        engine.plugin_run_command("Zq2059TextOpenDock", ""),
        "fixture plugin must register the Zq2059TextOpenDock command"
    );
    assert!(
        engine.bottom_panel_open,
        "vimcode.ui.open_view(..., {{location = \"dock\"}}) must open the bottom panel"
    );
    assert_eq!(
        engine.bottom_panel_kind,
        crate::core::engine::BottomPanelKind::Plugin(TEXT_VIEW.to_string()),
        "vimcode.ui.open_view(..., {{location = \"dock\"}}) must point the bottom \
         panel at the docked view"
    );
    (engine, dir)
}

/// `Form`-kind twin of [`engine_with_plugin_view_dock`] — opens [`VIEW`] (the
/// default-body, field-stack fixture, not [`TEXT_VIEW`]) into the
/// bottom-panel dock via the fixture's real `Zq2060FormOpenDock` `:Command`.
/// Exercises the dock's click-routing path
/// (`render::handle_plugin_view_dock_ui_event`) rather than the scroll-only
/// path [`engine_with_plugin_view_dock`]'s `TEXT_VIEW` exercises.
pub fn engine_with_plugin_view_form_dock() -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    load_fixture_plugin(&mut engine, &dir);

    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));

    assert!(
        engine.plugin_run_command("Zq2060FormOpenDock", ""),
        "fixture plugin must register the Zq2060FormOpenDock command"
    );
    assert!(
        engine.bottom_panel_open,
        "vimcode.ui.open_view(..., {{location = \"dock\"}}) must open the bottom panel"
    );
    assert_eq!(
        engine.bottom_panel_kind,
        crate::core::engine::BottomPanelKind::Plugin(VIEW.to_string()),
        "vimcode.ui.open_view(..., {{location = \"dock\"}}) must point the bottom \
         panel at the docked view"
    );
    (engine, dir)
}

/// Row-0 twin of [`engine_with_plugin_view_form_dock`] — opens [`ROW0_VIEW`],
/// whose first (and only clickable) field sits at index 0, into the
/// bottom-panel dock via the fixture's `Zq2060Row0OpenDock` `:Command`.
///
/// [`VIEW`]'s own click-routing coverage ([`engine_with_plugin_view_form_dock`]
/// paired with [`clicking_a_button_reaches_the_plugin_handler`]) only reaches
/// [`ROW_BUTTON`] at field index 4, inside `BottomPanelZone::Content` — it
/// cannot catch a routing bug confined to `BottomPanelZone::Toolbar`, the zone
/// a docked `Form`'s very first painted row falls into
/// (`render::route_bottom_panel_click`'s doc explains why). This fixture
/// exists to put the clickable field at row 0 so that zone gets exercised.
pub fn engine_with_plugin_view_row0_dock() -> (Engine, PathBuf) {
    let dir = plugin_dir();
    let mut engine = Engine::new_for_test();
    load_fixture_plugin(&mut engine, &dir);

    engine.app_shell.show_panel(&quadraui::WidgetId::new(
        crate::core::engine::sidebar::PANEL_SETTINGS,
    ));

    assert!(
        engine.plugin_run_command("Zq2060Row0OpenDock", ""),
        "fixture plugin must register the Zq2060Row0OpenDock command"
    );
    assert!(
        engine.bottom_panel_open,
        "vimcode.ui.open_view(..., {{location = \"dock\"}}) must open the bottom panel"
    );
    assert_eq!(
        engine.bottom_panel_kind,
        crate::core::engine::BottomPanelKind::Plugin(ROW0_VIEW.to_string()),
        "vimcode.ui.open_view(..., {{location = \"dock\"}}) must point the bottom \
         panel at the docked view"
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

/// Scenario 1c — the declared `color` row is painted. Unlike the slider's,
/// its guard clears at the sidebar's default width (see [`ROW_COLOR_LABEL`]'s
/// doc), so this needs no editor-tab split.
pub fn declared_color_row_is_painted(driver: &impl ConformanceDriver) {
    assert_paints(driver, &[ROW_COLOR_LABEL], "the plugin-declared color row");
}

/// Scenario 1d — the declared `toolbar` row, including its action button's
/// caption, is painted.
pub fn declared_toolbar_row_is_painted(driver: &impl ConformanceDriver) {
    assert_paints(
        driver,
        &[ROW_TOOLBAR_LABEL, ROW_TOOLBAR_BUTTON],
        "the plugin-declared toolbar row",
    );
}

/// Scenario 2b — clicking a declared toolbar's action button runs the
/// plugin's `on_event` as a [`crate::core::plugin_ui::ViewEventKind::
/// ToolbarButtonClicked`], reported with the *button's* id (not the
/// toolbar field's), and the effect shows up in the next painted frame.
///
/// Currently red on every backend: both `#[test]`s calling this are
/// `#[ignore]`d. `quadraui::FormController::handle_cached` (the only entry
/// point vimcode calls for plugin-view clicks) always measures fields with
/// its generic, backend-free `form_field_measure` fallback, and that
/// fallback has no `FieldKind::Toolbar` arm (unlike `ButtonRow`,
/// `ToggleGroup`, `SegmentedControl`, `TextArea`, which all do) — so it
/// never produces a per-button hit region for a toolbar row, and a click
/// anywhere on the row resolves to the row's own id, not the button's. The
/// vimcode-side translation of `FormEvent::ToolbarButtonClicked` into
/// [`crate::core::plugin_ui::ViewEventKind::ToolbarButtonClicked`] is
/// already wired (see `plugin_view_event_from_form_event`); it is simply
/// unreachable until the quadraui fallback gains the missing arm.
pub fn clicking_a_toolbar_button_reaches_the_plugin_handler(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted runs \
         were {:?}",
        painted(driver)
    );
    driver.click_text(ROW_TOOLBAR_BUTTON);
    assert!(
        driver.screen_has(COUNT_1),
        "clicking {ROW_TOOLBAR_BUTTON:?} must reach the plugin's on_event \
         handler and the next painted frame must show its effect \
         ({COUNT_1:?}); painted runs were {:?}",
        painted(driver)
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

/// Row-0 twin of [`clicking_a_button_reaches_the_plugin_handler`] — clicks
/// [`ROW0_BUTTON`], the first (field index 0) row of a docked `Form`, the
/// row that falls in `BottomPanelZone::Toolbar` rather than `Content`
/// (`render::route_bottom_panel_click`'s doc explains why). Pairs with
/// [`engine_with_plugin_view_row0_dock`] to cover the routing path
/// [`clicking_a_button_reaches_the_plugin_handler`] cannot reach.
pub fn clicking_the_dock_row0_button_reaches_the_plugin_handler(
    driver: &mut impl ConformanceDriver,
) {
    assert!(
        driver.screen_has(ROW0_COUNT_0),
        "precondition: the counter row must start at {ROW0_COUNT_0:?}; painted \
         runs were {:?}",
        painted(driver)
    );
    driver.click_text(ROW0_BUTTON);
    assert!(
        driver.screen_has(ROW0_COUNT_1),
        "clicking {ROW0_BUTTON:?} at dock row 0 must reach the plugin's \
         on_event handler and the next painted frame must show its effect \
         ({ROW0_COUNT_1:?}); painted runs were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(ROW0_COUNT_0),
        "the stale counter row {ROW0_COUNT_0:?} must not still be painted \
         after the view re-rendered; painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario — a non-collapsible `Section` header immediately followed by a
/// horizontal `buttons` row paints as a labelled group with its control row.
pub fn labelled_group_with_a_control_row_is_painted(driver: &impl ConformanceDriver) {
    assert_paints(
        driver,
        &[ROW_GROUP_HEADER, ROW_GROUP_BUTTON1, ROW_GROUP_BUTTON2],
        "a labelled group with a horizontal control row",
    );
}

/// Scenario — clicking a collapsible `Section` header reaches the plugin's
/// `on_event` handler as a [`crate::core::plugin_ui::ViewEventKind::
/// SectionToggled`], and the `collapsed` value it echoes back on the next
/// `render()` call hides the section's member field and flips the header's
/// disclosure glyph.
pub fn clicking_a_collapsible_section_header_toggles_its_member(
    driver: &mut impl ConformanceDriver,
) {
    assert!(
        driver.screen_has(ROW_COLLAPSIBLE_HEADER_EXPANDED),
        "precondition: the collapsible section must start expanded, painting \
         its disclosure glyph as {ROW_COLLAPSIBLE_HEADER_EXPANDED:?}; painted \
         runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(ROW_COLLAPSIBLE_MEMBER),
        "precondition: an expanded section's member row ({ROW_COLLAPSIBLE_MEMBER:?}) \
         must be painted; painted runs were {:?}",
        painted(driver)
    );
    driver.click_text(ROW_COLLAPSIBLE_HEADER);
    assert!(
        driver.screen_has(ROW_COLLAPSIBLE_HEADER_COLLAPSED),
        "clicking the collapsible header must reach the plugin's on_event \
         handler and the next painted frame must flip its disclosure glyph \
         to {ROW_COLLAPSIBLE_HEADER_COLLAPSED:?}; painted runs were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(ROW_COLLAPSIBLE_MEMBER),
        "a collapsed section must hide its member row ({ROW_COLLAPSIBLE_MEMBER:?}); \
         painted runs were {:?}",
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

/// Scenario 3b — `Shift+Tab` (`NamedKey::BackTab`) walks the focus
/// *backward* to the previous interactive field.
///
/// Three `Tab`s from the initial focus (already on `url`(0)) reach
/// [`ROW_EMPTY_TEXT_LABEL`]'s empty `text` field (`url`(0) -> `tls`(1) ->
/// `send`(2) -> `etx`(3)) — the same count `typing_edits_the_focused_text_
/// field` uses to land there. One `BackTab` must then move focus back onto
/// the `send` button (idx 2), not leave it parked on the text field. Typing
/// right after `BackTab` is the discriminator: the shared counter bumps
/// identically whether `send` or `etx` ends up handling `Enter` later, so it
/// cannot tell correct backward navigation apart from `BackTab` being
/// silently ignored — but only a *text* field inserts typed characters into
/// its painted value, so if focus is still (wrongly) on `etx`, typing paints
/// the typed text; if `BackTab` moved focus to the `send` button as it
/// should, the characters are dropped and the empty field's painted value
/// never changes. The discriminator characters (`x`, `z`) are plain letters
/// with no sidebar-chrome binding of their own, so dropping them while
/// `send` has focus has no side effect beyond being ignored.
pub fn shift_tab_navigates_backward_to_the_previous_field(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(COUNT_0),
        "precondition: the counter row must start at {COUNT_0:?}; painted runs \
         were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::Tab);
    driver.press_named(quadraui::NamedKey::BackTab);
    driver.type_char('x');
    driver.type_char('z');
    assert!(
        !driver.screen_has("xz"),
        "BackTab after three Tabs must move focus off the empty text field \
         and back onto the `send` button, so typing 'xz' must not land in \
         the text field's painted value; painted runs were {:?}",
        painted(driver)
    );
    assert!(
        driver.screen_has(COUNT_0),
        "typing into a focused button must not reach the plugin as a \
         TextChanged event, so the counter must still read {COUNT_0:?}; \
         painted runs were {:?}",
        painted(driver)
    );
    driver.press_named(quadraui::NamedKey::Enter);
    assert!(
        driver.screen_has(COUNT_1),
        "Enter must now activate the `send` button BackTab moved focus back \
         onto, reaching the plugin's on_event handler ({COUNT_1:?}); \
         painted runs were {:?}",
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

/// A `kind = "chart"` view paints its axis label and both series' legend
/// entries, and a click on a legend entry reaches the plugin's `on_event`
/// as `LegendClicked{series_idx}` (proved by the `y_label` marker flipping
/// from idle to seen).
pub fn chart_paints_and_legend_click_reaches_the_plugin(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[
            CHART_X_LABEL,
            CHART_Y_LABEL_IDLE,
            CHART_SERIES0_LABEL,
            CHART_SERIES1_LABEL,
        ],
        "the chart-kind view's initial paint (legend and axis label)",
    );
    driver.click_text(CHART_SERIES0_LABEL);
    assert!(
        driver.screen_has(CHART_Y_LABEL_SEEN),
        "clicking series 0's legend entry must dispatch LegendClicked{{series_idx: 0}}; \
         painted runs were {:?}",
        painted(driver)
    );
}

/// Scenario — a click on a chart's plotted body (not text, so it can't use
/// `click_text`) resolves through `ChartLayout::nearest_point` and reaches
/// the plugin's `on_event` as `DataPointClicked{series_idx, data_idx}`,
/// proved by the `x_label` marker flipping from idle to seen. Exercises the
/// `ChartHit::Body` fallback path `route_plugin_view_chart_click` takes when
/// quadraui's own `hit_test` never returns `ChartHit::DataPoint` directly.
///
/// Drives the sidebar-hosted copy of the chart;
/// [`chart_tab_data_point_click_reaches_the_plugin`] is the editor-tab twin.
pub fn chart_sidebar_data_point_click_reaches_the_plugin<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    chart_data_point_click_reaches_the_plugin(h, crate::core::engine::PluginViewHost::Sidebar);
}

/// Editor-tab twin of [`chart_sidebar_data_point_click_reaches_the_plugin`].
pub fn chart_tab_data_point_click_reaches_the_plugin<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    chart_data_point_click_reaches_the_plugin(h, crate::core::engine::PluginViewHost::Tab);
}

fn chart_data_point_click_reaches_the_plugin<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
    host: crate::core::engine::PluginViewHost,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    use crate::core::engine::PluginViewHost;
    assert_paints(
        &h.driver,
        &[CHART_X_LABEL],
        "the chart-kind view's initial paint, before any data-point click",
    );
    let (x, y) = {
        let engine = h.engine.borrow();
        let cached = match host {
            PluginViewHost::Sidebar => engine.plugin_view_chart_layout.borrow(),
            PluginViewHost::Tab => engine.plugin_view_tab_chart_layout.borrow(),
            PluginViewHost::Dock => unreachable!(
                "chart_data_point_click_reaches_the_plugin is never called with \
                 PluginViewHost::Dock -- the bottom-panel dock does not render \
                 chart-kind views yet"
            ),
        };
        let layout = cached
            .as_ref()
            .expect("chart_data_point_click_reaches_the_plugin: no frame painted yet");
        let &(_, _, x, y) = layout
            .data_point_positions
            .iter()
            .find(|&&(series_idx, data_idx, _, _)| series_idx == 0 && data_idx == 1)
            .expect("series 0's data point 1 must be in the painted layout");
        (x, y)
    };
    h.driver.click(x, y);
    assert!(
        h.driver.screen_has(CHART_POINT_SEEN),
        "a click on series 0's data point 1 must dispatch \
         DataPointClicked{{series_idx: 0, data_idx: 1}} to the plugin and \
         repaint the x_label marker as {CHART_POINT_SEEN:?}; painted runs \
         were {:?}",
        painted(&h.driver)
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

/// Scenario 10a — a `kind = "table"` view's column headers paint even when
/// its host panel is much shorter and narrower than the harness's usual
/// generous viewport, e.g. a squeezed sidebar panel with room for only a
/// couple of rows below the header.
pub fn table_header_paints_at_a_short_narrow_panel(driver: &mut impl ConformanceDriver) {
    assert_paints(
        driver,
        &[
            TABLE_COL_KEY,
            TABLE_COL_VALUE,
            TABLE_ROW0_KEY,
            TABLE_ROW0_VALUE,
        ],
        "the table-kind view's header and first row at a short, narrow panel",
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

/// Locate `needle`'s *final* painted text run within the current frame.
///
/// Under GTK a cell's text can appear in more than one run for the same
/// needle — the inventory accumulates a measuring/placeholder paint ahead of
/// the settled one — while TUI's cell-grid scan yields exactly one run per
/// cell, so there the first and last match are the same run. Only callers
/// that care about a cell's *settled* x-origin (such as the right-align
/// assertion below) need the last match; `painted_run`'s first-match
/// semantics stay the shared default since other callers rely on them.
fn painted_run_last(driver: &impl ConformanceDriver, needle: &str) -> quadraui::event::Rect {
    let inventory = driver.inventory();
    let runs = inventory.text_runs();
    runs.iter()
        .rfind(|r| r.text.contains(needle))
        .map(|r| r.bounds)
        .unwrap_or_else(|| {
            panic!(
                "painted_run_last: {needle:?} not painted; painted runs were {:?}",
                painted(driver)
            )
        })
}

/// The checked multi-select checkbox glyph a `multi_select` table prefixes
/// a selected row's column-0 cell with.
const CHECKED: char = '\u{2611}';
/// The unchecked multi-select checkbox glyph, prefixed onto every
/// unselected row of a `multi_select` table.
const UNCHECKED: char = '\u{2610}';

/// Whether `glyph` is painted as the checkbox prefix of the row whose
/// column-0 cell contains `label`.
///
/// Whether the glyph and the row's label share one painted text run depends
/// on the backend: GTK paints `"☐ Mr01631"` as a single run (the run
/// containing the label already contains the glyph), while
/// `TuiDriver::inventory` splits runs on whitespace (its cell-grid scan has
/// no notion of "same cell group" beyond a run of non-space columns — see
/// `quadraui::tui::testing::TuiDriver::inventory`), so the glyph lands in
/// its own run just to the left of the label's, sometimes merged with a
/// neighboring non-space character such as the column border `│`. So "is
/// this row checked" has to look in both places: the label's own run, and
/// any other run on the same painted line to the left of it.
fn glyph_left_of<D>(h: &crate::harness::ConformanceHarness<D>, glyph: char, label: &str) -> bool
where
    D: ConformanceDriver,
{
    let inventory = h.driver.inventory();
    let runs = inventory.text_runs();
    let Some(label_run) = runs.iter().find(|r| r.text.contains(label)) else {
        return false;
    };
    if label_run.text.contains(glyph) {
        return true;
    }
    let label_bounds = label_run.bounds;
    runs.iter().any(|r| {
        r.text.contains(glyph) && r.bounds.y == label_bounds.y && r.bounds.x < label_bounds.x
    })
}

/// Assert, on painted output alone, that the row whose column-0 cell
/// contains `label` is checked (`checked = true`) or unchecked.
///
/// Both directions are asserted — the wanted glyph present *and* the other
/// one absent — so a paint that lost the prefix entirely, or that painted
/// both states, fails rather than passing on a half-match. `context`
/// describes the gesture that is supposed to have produced this state.
fn assert_row_checkbox<D>(
    h: &crate::harness::ConformanceHarness<D>,
    label: &str,
    checked: bool,
    context: &str,
) where
    D: ConformanceDriver,
{
    let (want, unwanted) = if checked {
        (CHECKED, UNCHECKED)
    } else {
        (UNCHECKED, CHECKED)
    };
    assert!(
        glyph_left_of(h, want, label),
        "{context}: row {label:?} must paint its checkbox as {want:?}; \
         painted runs were {:?}",
        painted(&h.driver)
    );
    assert!(
        !glyph_left_of(h, unwanted, label),
        "{context}: row {label:?} must not paint the opposite checkbox \
         glyph {unwanted:?}; painted runs were {:?}",
        painted(&h.driver)
    );
}

/// How far to shift a modified click's x-coordinate from the previous
/// click's, in the driver's own coordinate unit.
///
/// quadraui's `DoubleClickDetector` folds a second `MouseDown` into a
/// modifier-less `DoubleClick` when it lands within 400 ms *and* within 1.5
/// units of the previous one on both axes (`quadraui::dispatch`). Two
/// gesture clicks on adjacent rows are one row-height apart, which TUI's
/// character-cell grid measures as a single unit — inside that radius — so
/// consecutive clicks in the gesture scenarios alternate this nudge on x to
/// stay outside it. Far enough to clear the 1.5-unit radius, small enough
/// to stay inside the clicked column on both backends, and not timing
/// dependent the way a sleep past the 400 ms window would be.
const CLICK_NUDGE: f32 = 2.0;

/// Dispatch a `MouseDown` carrying `modifiers` at `(x, y)`.
///
/// `ConformanceDriver::click` carries no modifiers, so the ctrl/shift
/// gestures have to be dispatched as raw `UiEvent`s; this is the same
/// `route_plugin_view_body_event` path a real modified click reaches.
fn dispatch_modified_click<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
    (x, y): (f32, f32),
    modifiers: quadraui::Modifiers,
) where
    D: ConformanceDriver + quadraui::testing::DriverInput,
{
    h.driver.dispatch(quadraui::UiEvent::MouseDown {
        widget: None,
        button: quadraui::MouseButton::Left,
        position: quadraui::Point::new(x, y),
        modifiers,
    });
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
            PluginViewHost::Dock => unreachable!(
                "table_divider_drag_resizes_the_column is never called with \
                 PluginViewHost::Dock — the bottom-panel dock does not render \
                 table-kind views yet"
            ),
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

/// Scenario 10e2 — a click on a table **body** row's column dispatches
/// `ItemSelected{{col}}` carrying the clicked column, resolved against *all*
/// columns rather than only `editable` ones. [`HEADER_EVENT_VIEW`] declares
/// neither column `editable`, so row `m1`'s column 1 is a read-only cell:
/// clicking it must still report `col: Some(1)` on the dispatched event, even
/// though the same click leaves cell-edit arming untouched (there is no
/// editable column to arm here at all).
pub fn table_body_click_reports_the_clicked_column_even_when_read_only<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[HEADER_EVENT_CELL_NONE],
        "the fixture's initial paint, before any body-row click",
    );

    let (x, y) = {
        let engine = h.engine.borrow();
        let cached = engine.plugin_view_table_layout.borrow();
        let (rect, layout) = cached.as_ref().expect(
            "table_body_click_reports_the_clicked_column_even_when_read_only: \
             no frame painted yet",
        );
        let column = &layout.columns[1];
        (
            rect.x + column.x + column.width / 2.0,
            rect.y + layout.header_height + layout.row_height + layout.row_height / 2.0,
        )
    };
    h.driver.click(x, y);
    assert!(
        h.driver.screen_has(HEADER_EVENT_CELL_COL1_SEEN),
        "a click on row m1's read-only column 1 must dispatch \
         ItemSelected{{col: Some(1)}} to the plugin and repaint the cell marker \
         row as {HEADER_EVENT_CELL_COL1_SEEN:?}, even though neither column \
         declares editable = true; painted runs were {:?}",
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

/// Scenario 10g — a `multi_select = true` table paints every row with an
/// unchecked checkbox glyph and a pinned `footer` row, checks the clicked
/// row's glyph, and dispatches `SelectionChanged` to the plugin alongside
/// the single-row cursor's own `ItemSelected`.
pub fn table_multi_select_click_checks_the_row_and_paints_the_footer<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[MULTI_ROW0, MULTI_ROW1, MULTI_FOOTER],
        "the multi-select table's initial paint",
    );
    assert_row_checkbox(h, MULTI_ROW0, false, "before any click");
    assert_row_checkbox(h, MULTI_ROW1, false, "before any click");

    let (x, y) = {
        let engine = h.engine.borrow();
        let cached = engine.plugin_view_table_layout.borrow();
        let (rect, layout) = cached
            .as_ref()
            .expect("the multi-select table must have painted and cached a layout");
        let column = &layout.columns[0];
        (
            rect.x + column.x + column.width / 2.0,
            rect.y + layout.header_height + layout.row_height / 2.0,
        )
    };
    h.driver.click(x, y);

    assert_row_checkbox(h, MULTI_ROW0, true, "a plain click on row 0");
    assert_row_checkbox(
        h,
        MULTI_ROW1,
        false,
        "a plain click on row 0 (row 1 was never clicked)",
    );
    assert!(
        h.driver.screen_has(MULTI_SELECTION_ROW0),
        "the click must dispatch SelectionChanged{{indices: [0]}} to the \
         plugin; painted runs were {:?}",
        painted(&h.driver)
    );
    assert!(
        h.driver.screen_has(MULTI_FOOTER),
        "the pinned footer row must still paint after a body-row click; \
         painted runs were {:?}",
        painted(&h.driver)
    );
}

/// Scenario 10h — a plain click followed by a ctrl-click on a different row
/// toggles that second row into the multi-selection *without* dropping the
/// first, and a further ctrl-click on the first row toggles it back out
/// again, leaving the second row still selected and the never-clicked third
/// row untouched throughout. Exercises `quadraui::SelectionGesture::Toggle`
/// in [`RANGE_SELECT_VIEW`]'s declared (identity) row order — no header
/// click, so declared and display space coincide here.
pub fn table_ctrl_click_toggles_a_row_without_clearing_the_others<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[RANGE_ROW0, RANGE_ROW1, RANGE_ROW2],
        "the range-select table's initial paint",
    );

    // `nudge` shifts x off the previous click's so the pair can't fold into
    // a `DoubleClick` — see [`CLICK_NUDGE`].
    let row_xy =
        |h: &crate::harness::ConformanceHarness<D>, row: usize, nudge: f32| -> (f32, f32) {
            let engine = h.engine.borrow();
            let cached = engine.plugin_view_table_layout.borrow();
            let (rect, layout) = cached.as_ref().expect(
                "table_ctrl_click_toggles_a_row_without_clearing_the_others: \
             no frame painted yet",
            );
            let column = &layout.columns[0];
            (
                rect.x + column.x + column.width / 2.0 + nudge,
                rect.y + layout.header_height + layout.row_height * (row as f32 + 0.5),
            )
        };
    let selected = |h: &crate::harness::ConformanceHarness<D>| -> Vec<usize> {
        let engine = h.engine.borrow();
        let mut v = engine
            .plugin_view_table_multi_selected
            .get(RANGE_SELECT_VIEW)
            .cloned()
            .unwrap_or_default();
        v.sort_unstable();
        v
    };

    for (row, label) in [(0, RANGE_ROW0), (1, RANGE_ROW1), (2, RANGE_ROW2)] {
        assert_row_checkbox(h, label, false, &format!("row {row} before any click"));
    }

    let (x0, y0) = row_xy(h, 0, 0.0);
    h.driver.click(x0, y0);
    assert_row_checkbox(h, RANGE_ROW0, true, "a plain click on declared row 0");
    assert_row_checkbox(
        h,
        RANGE_ROW1,
        false,
        "a plain click on declared row 0 (row 1 unclicked)",
    );
    assert_eq!(
        selected(h),
        vec![0],
        "a plain click on declared row 0 must select only it"
    );

    dispatch_modified_click(
        h,
        row_xy(h, 1, CLICK_NUDGE),
        quadraui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    );
    assert_row_checkbox(
        h,
        RANGE_ROW0,
        true,
        "a ctrl-click on declared row 1 must leave row 0 checked",
    );
    assert_row_checkbox(h, RANGE_ROW1, true, "a ctrl-click on declared row 1");
    assert_eq!(
        selected(h),
        vec![0, 1],
        "a ctrl-click on declared row 1 must add it to the selection, \
         keeping row 0 selected"
    );

    dispatch_modified_click(
        h,
        row_xy(h, 0, 0.0),
        quadraui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    );
    assert_row_checkbox(
        h,
        RANGE_ROW0,
        false,
        "a second ctrl-click on declared row 0 must uncheck it",
    );
    assert_row_checkbox(
        h,
        RANGE_ROW1,
        true,
        "a second ctrl-click on declared row 0 must leave row 1 checked",
    );
    assert_row_checkbox(
        h,
        RANGE_ROW2,
        false,
        "row 2 was never clicked and must stay unchecked",
    );
    assert_eq!(
        selected(h),
        vec![1],
        "a second ctrl-click on declared row 0 must toggle it back out, \
         leaving row 1 still selected"
    );
}

/// Scenario 10i — a `Shift`-extended range selection is measured in
/// *display* space (the order the rows currently paint in, after a header
/// sort) and only then translated back to the declared indices
/// `SelectionChanged` and `plugin_view_table_multi_selected` both use. One
/// header click on [`RANGE_SELECT_VIEW`] reorders its rows from declared
/// `[0, 1, 2]` to display `[1, 0, 2]` (ascending by the single column's
/// text); a plain click at display position 1 (declared row 0) followed by
/// a shift-click at display position 2 (declared row 2) must select exactly
/// declared rows `{0, 2}` — *not* declared row 1, even though it sits
/// between them in the view's declared order, because display position 0
/// (declared row 1) sits *outside* the `[1, 2]` display range the shift-click
/// spans.
pub fn table_shift_click_ranges_over_display_order_under_sort<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[RANGE_ROW0, RANGE_ROW1, RANGE_ROW2],
        "the range-select table's initial paint",
    );

    let header_xy = |h: &crate::harness::ConformanceHarness<D>| -> (f32, f32) {
        let engine = h.engine.borrow();
        let cached = engine.plugin_view_table_layout.borrow();
        let (rect, layout) = cached.as_ref().expect(
            "table_shift_click_ranges_over_display_order_under_sort: \
             no frame painted yet",
        );
        let col0 = &layout.columns[0];
        (
            rect.x + col0.x + col0.width / 2.0,
            rect.y + layout.header_height / 2.0,
        )
    };
    let (header_x, header_y) = header_xy(h);
    h.driver.click(header_x, header_y);
    assert!(
        h.driver.inventory().above(RANGE_ROW1, RANGE_ROW0)
            && h.driver.inventory().above(RANGE_ROW0, RANGE_ROW2),
        "precondition: one header click must sort ascending into display \
         order [{RANGE_ROW1:?}, {RANGE_ROW0:?}, {RANGE_ROW2:?}]; painted \
         runs were {:?}",
        painted(&h.driver)
    );

    // `nudge` shifts x off the previous click's so the pair can't fold into
    // a `DoubleClick` — see [`CLICK_NUDGE`].
    let display_xy =
        |h: &crate::harness::ConformanceHarness<D>, display: usize, nudge: f32| -> (f32, f32) {
            let engine = h.engine.borrow();
            let cached = engine.plugin_view_table_layout.borrow();
            let (rect, layout) = cached.as_ref().expect(
                "table_shift_click_ranges_over_display_order_under_sort: \
             no frame painted yet",
            );
            let column = &layout.columns[0];
            (
                rect.x + column.x + column.width / 2.0 + nudge,
                rect.y + layout.header_height + layout.row_height * (display as f32 + 0.5),
            )
        };
    let selected = |h: &crate::harness::ConformanceHarness<D>| -> Vec<usize> {
        let engine = h.engine.borrow();
        let mut v = engine
            .plugin_view_table_multi_selected
            .get(RANGE_SELECT_VIEW)
            .cloned()
            .unwrap_or_default();
        v.sort_unstable();
        v
    };

    for (display, label) in [(0, RANGE_ROW1), (1, RANGE_ROW0), (2, RANGE_ROW2)] {
        assert_row_checkbox(
            h,
            label,
            false,
            &format!("display position {display} before any row click"),
        );
    }

    // Display position 1 is declared row 0 (`RANGE_ROW0`'s "Rb" sorts second).
    let (x1, y1) = display_xy(h, 1, 0.0);
    h.driver.click(x1, y1);
    assert_row_checkbox(
        h,
        RANGE_ROW0,
        true,
        "a plain click at display position 1 (declared row 0)",
    );

    // Display position 2 is declared row 2 (`RANGE_ROW2`'s "Rz" sorts last).
    dispatch_modified_click(
        h,
        display_xy(h, 2, CLICK_NUDGE),
        quadraui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    // The range spans display positions [1, 2] — declared rows 0 and 2 —
    // so declared row 1 must stay unchecked even though it sits *between*
    // them in declared order: it paints at display position 0, outside the
    // span. Reading the glyphs rather than the selection set is what pins
    // that the display->declared mapping survives all the way to the frame.
    assert_row_checkbox(
        h,
        RANGE_ROW0,
        true,
        "a shift-click at display position 2 (range start stays checked)",
    );
    assert_row_checkbox(
        h,
        RANGE_ROW2,
        true,
        "a shift-click at display position 2 (range end)",
    );
    assert_row_checkbox(
        h,
        RANGE_ROW1,
        false,
        "a shift-click over display positions [1, 2] must not reach \
         declared row 1, which paints at display position 0",
    );
    assert_eq!(
        selected(h),
        vec![0, 2],
        "a shift-click at display position 2 must range over display \
         positions [1, 2] (declared rows 0 and 2), excluding declared row 1 \
         even though it sits between them in declared order"
    );
}

/// Scenario 10j — a selection anchor left pointing past the end of a
/// shorter row set cannot leak an out-of-range index into the plugin's
/// `SelectionChanged`. A plugin may `ui.refresh` fewer rows between the
/// click that anchors a selection and the `Shift`-click that ranges from it,
/// so the stored anchor is clamped to the current row count: the range it
/// spans ends at the last row that exists, and the plugin — which reports
/// out-of-range indices in [`RANGE_SELECT_VIEW`]'s painted footer — sees
/// only indices into rows it declared.
pub fn table_shift_click_clamps_a_stale_anchor_to_the_row_count<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[RANGE_ROW0, RANGE_ROW1, RANGE_ROW2, RANGE_BOUNDS_NONE],
        "the range-select table's initial paint",
    );

    // Stands in for the row-count shrink a plugin's own `ui.refresh` would
    // cause between two gesture clicks: an anchor position past the view's
    // live rows. Writing it directly keeps the scenario to the one
    // behaviour it measures, since the refresh itself is covered elsewhere.
    h.engine
        .borrow_mut()
        .plugin_view_table_selection_anchor
        .insert(RANGE_SELECT_VIEW.to_string(), 99);

    let (x, y) = {
        let engine = h.engine.borrow();
        let cached = engine.plugin_view_table_layout.borrow();
        let (rect, layout) = cached.as_ref().expect(
            "table_shift_click_clamps_a_stale_anchor_to_the_row_count: \
             no frame painted yet",
        );
        let column = &layout.columns[0];
        (
            rect.x + column.x + column.width / 2.0,
            rect.y + layout.header_height + layout.row_height * 1.5,
        )
    };
    dispatch_modified_click(
        h,
        (x, y),
        quadraui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );

    assert!(
        h.driver.screen_has(RANGE_BOUNDS_OK),
        "the clamped anchor must dispatch only indices into rows the plugin \
         declared, which it reports by painting {RANGE_BOUNDS_OK:?} in its \
         footer; painted runs were {:?}",
        painted(&h.driver)
    );
    assert!(
        !h.driver.screen_has(RANGE_BOUNDS_BAD),
        "no dispatched index may point past the plugin's last row; painted \
         runs were {:?}",
        painted(&h.driver)
    );
    // No header click here, so display and declared order coincide: the
    // range runs from the clamped anchor (the last row) back to row 1.
    assert_row_checkbox(h, RANGE_ROW1, true, "a shift-click with a stale anchor");
    assert_row_checkbox(h, RANGE_ROW2, true, "a shift-click with a stale anchor");
    assert_row_checkbox(
        h,
        RANGE_ROW0,
        false,
        "a shift-click with a stale anchor must not reach row 0, which sits \
         outside the clamped range",
    );
}

/// Scenario 10k — a column declaring `align = "right"` paints its cell's
/// text run further from its own column's left edge than a column with no
/// `align` override does, using [`ALIGN_VIEW`]'s two single-row columns
/// (left-default, right-declared) so the comparison is within one frame
/// rather than across two fixtures with potentially different column
/// widths.
pub fn table_align_right_column_indents_its_cell_text<D>(
    h: &mut crate::harness::ConformanceHarness<D>,
) where
    D: quadraui::testing::ConformanceDriver + quadraui::testing::DriverInput,
{
    assert_paints(
        &h.driver,
        &[ALIGN_CELL_LEFT, ALIGN_CELL_RIGHT],
        "the align table's initial paint",
    );

    let engine = h.engine.borrow();
    let cached = engine.plugin_view_table_layout.borrow();
    let (rect, layout) = cached
        .as_ref()
        .expect("table_align_right_column_indents_its_cell_text: no frame painted yet");
    let col_left = &layout.columns[0];
    let col_right = &layout.columns[1];
    let col_left_x = rect.x + col_left.x;
    let col_right_x = rect.x + col_right.x;
    drop(cached);
    drop(engine);

    let left_run = painted_run_last(&h.driver, ALIGN_CELL_LEFT);
    let right_run = painted_run_last(&h.driver, ALIGN_CELL_RIGHT);
    let left_offset = left_run.x - col_left_x;
    let right_offset = right_run.x - col_right_x;

    assert!(
        right_offset > left_offset,
        "a column declaring align = \"right\" must paint its cell's text run \
         further from its own column's left edge than the default-aligned \
         column does: left offset {left_offset}, right offset {right_offset}"
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

/// Scenario 12 — `location = "dock"` paints the view into the bottom-panel
/// dock, and re-invoking the open command for the same name re-focuses that
/// one dock slot (stays open, stays pointed at the same view) rather than
/// closing it or swapping in a different panel kind. Takes the whole
/// harness, not just its driver, so it can drive a second `:Command`
/// invocation through the live fixture engine between paints.
pub fn dock_paints_and_reuses_by_name<
    D: ConformanceDriver + crate::harness::rest_client::DrivesIdle,
>(
    h: &mut crate::harness::ConformanceHarness<D>,
) {
    assert!(
        h.driver.screen_has(TEXT_FIRST_LINE),
        "a view docked via location = \"dock\" must paint in the bottom panel; \
         painted runs were {:?}",
        painted(&h.driver)
    );
    assert_eq!(
        h.engine.borrow().bottom_panel_kind,
        crate::core::engine::BottomPanelKind::Plugin(TEXT_VIEW.to_string()),
        "the bottom panel must point at the docked view"
    );

    assert!(
        h.engine
            .borrow_mut()
            .plugin_run_command("Zq2059TextOpenDock", ""),
        "fixture plugin must register the Zq2059TextOpenDock command"
    );
    assert!(
        h.engine.borrow().bottom_panel_open,
        "re-opening the same docked view by name must leave the bottom \
         panel open, not toggle it closed"
    );
    assert_eq!(
        h.engine.borrow().bottom_panel_kind,
        crate::core::engine::BottomPanelKind::Plugin(TEXT_VIEW.to_string()),
        "re-opening the same docked view by name must reuse the single \
         dock slot, not replace it with a different panel kind"
    );
    h.driver.repaint();
    assert!(
        h.driver.screen_has(TEXT_FIRST_LINE),
        "the docked view must still paint after being re-opened by name; \
         painted runs were {:?}",
        painted(&h.driver)
    );
}

/// Nearest painted `LnNX1631` text-view line to `anchor_y` — keeps
/// `scroll_at`'s needle valid across repeated wheel scrolls, since the exact
/// line at a given screen row changes as the view scrolls (mirrors
/// `crate::harness`'s own `nearest_row` helper for the sidebar's wheel-scroll
/// scenario).
fn nearest_text_line<D: ConformanceDriver>(driver: &D, anchor_y: f32) -> (u32, String) {
    driver
        .inventory()
        .text_runs()
        .iter()
        .filter_map(|run| {
            let trimmed = run.text.trim();
            trimmed
                .strip_prefix("Ln")
                .and_then(|rest| rest.strip_suffix("X1631"))
                .and_then(|digits| digits.parse::<u32>().ok())
                .map(|idx| (idx, trimmed.to_string(), (run.bounds.y - anchor_y).abs()))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(idx, text, _)| (idx, text))
        .expect(
            "at least one LnNX1631 line must be painted somewhere — the \
             fixture seeded 50 of them",
        )
}

/// Scenario 13 — a mouse-wheel scroll over the dock's body reaches the
/// docked `text_view`, via the `"plugin_dock"` scroll surface
/// (`frame_editor_band.rs`) and `Engine::handle_plugin_view_dock_scroll`.
pub fn dock_scrolls_on_mouse_wheel(driver: &mut impl ConformanceDriver) {
    assert!(
        driver.screen_has(TEXT_FIRST_LINE),
        "precondition: the dock starts scrolled to the top; painted runs \
         were {:?}",
        painted(driver)
    );
    assert!(
        !driver.screen_has(TEXT_LAST_LINE),
        "precondition: with 50 lines in a short dock, the last line must not \
         be visible before scrolling; painted runs were {:?}",
        painted(driver)
    );

    let anchor_y = driver
        .inventory()
        .text_runs()
        .iter()
        .find(|run| run.text.trim() == TEXT_FIRST_LINE)
        .map(|run| run.bounds.y)
        .expect("precondition: the first text-view line must be painted somewhere");

    let mut needle = TEXT_FIRST_LINE.to_string();
    let mut idx = 0u32;
    for _ in 0..30 {
        driver.scroll_at(&needle, -1);
        let (new_idx, new_needle) = nearest_text_line(driver, anchor_y);
        idx = new_idx;
        needle = new_needle;
        if driver.screen_has(TEXT_LAST_LINE) {
            break;
        }
    }
    assert!(
        idx > 0,
        "scrolling the mouse wheel over the docked text view must move it \
         off its first line"
    );
    assert!(
        driver.screen_has(TEXT_LAST_LINE),
        "scrolling the mouse wheel over the docked text view must reach its \
         last line; painted runs were {:?}",
        painted(driver)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cell viewport for the TUI arm.
    const TUI_W: u16 = 120;
    const TUI_H: u16 = 30;
    /// A much shorter, narrower cell viewport than `TUI_W`x`TUI_H` — shaped
    /// like a squeezed sidebar panel rather than a full-screen window.
    const TUI_SHORT_W: u16 = 40;
    const TUI_SHORT_H: u16 = 8;
    /// Pixel viewport for the GTK arm.
    #[cfg(feature = "gui")]
    const W: i32 = 1400;
    #[cfg(feature = "gui")]
    const H: i32 = 900;
    /// A much shorter, narrower pixel viewport than `W`x`H`, scaled by the
    /// same proportions as `TUI_SHORT_W`/`TUI_SHORT_H` relative to
    /// `TUI_W`/`TUI_H`.
    #[cfg(feature = "gui")]
    const W_SHORT: i32 = 480;
    #[cfg(feature = "gui")]
    const H_SHORT: i32 = 240;

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

    /// Sidebar fixture for a named `ViewBody`-kind view at a short, narrow
    /// viewport (TUI arm) — see `TUI_SHORT_W`/`TUI_SHORT_H`.
    fn tui_named_short(
        view: &str,
    ) -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_named_plugin_view(view);
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_SHORT_W, TUI_SHORT_H);
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

    /// Bottom-panel dock fixture for [`TEXT_VIEW`] (TUI arm).
    fn tui_dock() -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_dock();
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    #[test]
    fn plugin_view_dock_paints_and_reuses_by_name_on_tui() {
        let (mut h, _dir) = tui_dock();
        dock_paints_and_reuses_by_name(&mut h);
    }

    #[test]
    fn plugin_view_dock_scrolls_on_mouse_wheel_on_tui() {
        let (mut h, _dir) = tui_dock();
        dock_scrolls_on_mouse_wheel(&mut h.driver);
    }

    /// Bottom-panel dock fixture for [`VIEW`] (`Form`-kind body, TUI arm).
    fn tui_form_dock() -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_form_dock();
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    #[test]
    fn plugin_view_form_dock_button_click_reaches_the_plugin_handler_on_tui() {
        let (mut h, _dir) = tui_form_dock();
        clicking_a_button_reaches_the_plugin_handler(&mut h.driver);
    }

    /// Bottom-panel dock fixture for [`ROW0_VIEW`] (clickable field at index 0,
    /// TUI arm).
    fn tui_row0_dock() -> (
        crate::harness::ConformanceHarness<
            quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_row0_dock();
        let mut h = crate::tui_main::testing::conformance_harness(engine, TUI_W, TUI_H);
        h.driver.render();
        (h, dir)
    }

    #[test]
    fn plugin_view_dock_row0_button_click_reaches_the_plugin_handler_on_tui() {
        let (mut h, _dir) = tui_row0_dock();
        clicking_the_dock_row0_button_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_list_paints_and_click_selects_the_right_index_on_tui() {
        let (mut h, _dir) = tui_named(LIST_VIEW);
        list_paints_and_click_selects_the_right_index(&mut h.driver);
    }

    #[test]
    fn plugin_view_chart_paints_and_legend_click_reaches_the_plugin_on_tui() {
        let (mut h, _dir) = tui_named(CHART_VIEW);
        chart_paints_and_legend_click_reaches_the_plugin(&mut h.driver);
    }

    #[test]
    fn plugin_view_chart_data_point_click_reaches_the_plugin_on_tui() {
        let (mut h, _dir) = tui_named(CHART_VIEW);
        chart_sidebar_data_point_click_reaches_the_plugin(&mut h);
    }

    #[test]
    fn plugin_view_chart_tab_data_point_click_reaches_the_plugin_on_tui() {
        let (mut h, _dir) = tui_named_tab(CHART_VIEW, "Zq2065ChartOpenTab");
        chart_tab_data_point_click_reaches_the_plugin(&mut h);
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
    fn plugin_view_table_header_paints_at_a_short_narrow_panel_on_tui() {
        let (mut h, _dir) = tui_named_short(TABLE_VIEW);
        table_header_paints_at_a_short_narrow_panel(&mut h.driver);
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
    fn plugin_view_table_body_click_reports_the_clicked_column_even_when_read_only_on_tui() {
        let (mut h, _dir) = tui_named(HEADER_EVENT_VIEW);
        table_body_click_reports_the_clicked_column_even_when_read_only(&mut h);
    }

    #[test]
    fn plugin_view_table_declared_width_narrows_the_column_on_tui() {
        let (mut narrow, _narrow_dir) = tui_named(WIDTH_VIEW);
        let (mut baseline, _baseline_dir) = tui_named(TABLE_VIEW);
        table_declared_width_narrows_the_column(&mut narrow, &mut baseline);
    }

    #[test]
    fn plugin_view_table_multi_select_click_checks_the_row_and_paints_the_footer_on_tui() {
        let (mut h, _dir) = tui_named(MULTI_SELECT_VIEW);
        table_multi_select_click_checks_the_row_and_paints_the_footer(&mut h);
    }

    #[test]
    fn plugin_view_table_ctrl_click_toggles_a_row_without_clearing_the_others_on_tui() {
        let (mut h, _dir) = tui_named(RANGE_SELECT_VIEW);
        table_ctrl_click_toggles_a_row_without_clearing_the_others(&mut h);
    }

    #[test]
    fn plugin_view_table_shift_click_ranges_over_display_order_under_sort_on_tui() {
        let (mut h, _dir) = tui_named(RANGE_SELECT_VIEW);
        table_shift_click_ranges_over_display_order_under_sort(&mut h);
    }

    #[test]
    fn plugin_view_table_shift_click_clamps_a_stale_anchor_to_the_row_count_on_tui() {
        let (mut h, _dir) = tui_named(RANGE_SELECT_VIEW);
        table_shift_click_clamps_a_stale_anchor_to_the_row_count(&mut h);
    }

    #[test]
    fn plugin_view_table_align_right_column_indents_its_cell_text_on_tui() {
        let (mut h, _dir) = tui_named(ALIGN_VIEW);
        table_align_right_column_indents_its_cell_text(&mut h);
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
    fn plugin_view_declared_color_row_is_painted_on_tui() {
        let (h, _dir) = tui();
        declared_color_row_is_painted(&h.driver);
    }

    #[test]
    fn plugin_view_declared_toolbar_row_is_painted_on_tui() {
        let (h, _dir) = tui();
        declared_toolbar_row_is_painted(&h.driver);
    }

    // Red on every backend — see `clicking_a_toolbar_button_reaches_the_plugin_handler`'s
    // doc comment. Blocked on a quadraui fix, not a vimcode backend bug.
    #[test]
    #[ignore = "blocked on quadraui: form_field_measure has no FieldKind::Toolbar arm"]
    fn plugin_view_toolbar_button_click_reaches_the_handler_on_tui() {
        let (mut h, _dir) = tui();
        clicking_a_toolbar_button_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_labelled_group_with_a_control_row_is_painted_on_tui() {
        let (h, _dir) = tui_named(SECTION_VIEW);
        labelled_group_with_a_control_row_is_painted(&h.driver);
    }

    #[test]
    fn plugin_view_clicking_a_collapsible_section_header_toggles_its_member_on_tui() {
        let (mut h, _dir) = tui_named(SECTION_VIEW);
        clicking_a_collapsible_section_header_toggles_its_member(&mut h.driver);
    }

    #[test]
    fn plugin_view_keyboard_activation_reaches_the_handler_on_tui() {
        let (mut h, _dir) = tui();
        keyboard_activation_reaches_the_plugin_handler(&mut h.driver);
    }

    #[test]
    fn plugin_view_shift_tab_navigates_backward_on_tui() {
        let (mut h, _dir) = tui();
        shift_tab_navigates_backward_to_the_previous_field(&mut h.driver);
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

    /// Sidebar fixture for a named `ViewBody`-kind view at a short, narrow
    /// viewport (GTK arm) — see `W_SHORT`/`H_SHORT`.
    #[cfg(feature = "gui")]
    fn gtk_named_short(
        view: &str,
    ) -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_named_plugin_view(view);
        let mut h = crate::gtk::testing::conformance_harness(engine, W_SHORT, H_SHORT);
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

    /// Bottom-panel dock fixture for [`TEXT_VIEW`] (GTK arm).
    #[cfg(feature = "gui")]
    fn gtk_dock() -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_dock();
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_dock_paints_and_reuses_by_name_on_gtk() {
        let (mut h, _dir) = gtk_dock();
        dock_paints_and_reuses_by_name(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_dock_scrolls_on_mouse_wheel_on_gtk() {
        let (mut h, _dir) = gtk_dock();
        dock_scrolls_on_mouse_wheel(&mut h.driver);
    }

    /// Bottom-panel dock fixture for [`VIEW`] (`Form`-kind body, GTK arm).
    #[cfg(feature = "gui")]
    fn gtk_form_dock() -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_form_dock();
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_form_dock_button_click_reaches_the_plugin_handler_on_gtk() {
        let (mut h, _dir) = gtk_form_dock();
        clicking_a_button_reaches_the_plugin_handler(&mut h.driver);
    }

    /// Bottom-panel dock fixture for [`ROW0_VIEW`] (clickable field at index 0,
    /// GTK arm).
    #[cfg(feature = "gui")]
    fn gtk_row0_dock() -> (
        crate::harness::ConformanceHarness<
            quadraui::gtk::testing::GtkDriver<impl quadraui::AppLogic>,
        >,
        PathBuf,
    ) {
        let (engine, dir) = engine_with_plugin_view_row0_dock();
        let mut h = crate::gtk::testing::conformance_harness(engine, W, H);
        h.driver.render();
        (h, dir)
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_dock_row0_button_click_reaches_the_plugin_handler_on_gtk() {
        let (mut h, _dir) = gtk_row0_dock();
        clicking_the_dock_row0_button_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_list_paints_and_click_selects_the_right_index_on_gtk() {
        let (mut h, _dir) = gtk_named(LIST_VIEW);
        list_paints_and_click_selects_the_right_index(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_chart_paints_and_legend_click_reaches_the_plugin_on_gtk() {
        let (mut h, _dir) = gtk_named(CHART_VIEW);
        chart_paints_and_legend_click_reaches_the_plugin(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_chart_data_point_click_reaches_the_plugin_on_gtk() {
        let (mut h, _dir) = gtk_named(CHART_VIEW);
        chart_sidebar_data_point_click_reaches_the_plugin(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_chart_tab_data_point_click_reaches_the_plugin_on_gtk() {
        let (mut h, _dir) = gtk_named_tab(CHART_VIEW, "Zq2065ChartOpenTab");
        chart_tab_data_point_click_reaches_the_plugin(&mut h);
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
    fn plugin_view_table_header_paints_at_a_short_narrow_panel_on_gtk() {
        let (mut h, _dir) = gtk_named_short(TABLE_VIEW);
        table_header_paints_at_a_short_narrow_panel(&mut h.driver);
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
    fn plugin_view_table_body_click_reports_the_clicked_column_even_when_read_only_on_gtk() {
        let (mut h, _dir) = gtk_named(HEADER_EVENT_VIEW);
        table_body_click_reports_the_clicked_column_even_when_read_only(&mut h);
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
    fn plugin_view_table_multi_select_click_checks_the_row_and_paints_the_footer_on_gtk() {
        let (mut h, _dir) = gtk_named(MULTI_SELECT_VIEW);
        table_multi_select_click_checks_the_row_and_paints_the_footer(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_ctrl_click_toggles_a_row_without_clearing_the_others_on_gtk() {
        let (mut h, _dir) = gtk_named(RANGE_SELECT_VIEW);
        table_ctrl_click_toggles_a_row_without_clearing_the_others(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_shift_click_ranges_over_display_order_under_sort_on_gtk() {
        let (mut h, _dir) = gtk_named(RANGE_SELECT_VIEW);
        table_shift_click_ranges_over_display_order_under_sort(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_shift_click_clamps_a_stale_anchor_to_the_row_count_on_gtk() {
        let (mut h, _dir) = gtk_named(RANGE_SELECT_VIEW);
        table_shift_click_clamps_a_stale_anchor_to_the_row_count(&mut h);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_table_align_right_column_indents_its_cell_text_on_gtk() {
        let (mut h, _dir) = gtk_named(ALIGN_VIEW);
        table_align_right_column_indents_its_cell_text(&mut h);
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
    fn plugin_view_declared_color_row_is_painted_on_gtk() {
        let (h, _dir) = gtk();
        declared_color_row_is_painted(&h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_declared_toolbar_row_is_painted_on_gtk() {
        let (h, _dir) = gtk();
        declared_toolbar_row_is_painted(&h.driver);
    }

    // Red on every backend — see `clicking_a_toolbar_button_reaches_the_plugin_handler`'s
    // doc comment. Blocked on a quadraui fix, not a vimcode backend bug.
    #[cfg(feature = "gui")]
    #[test]
    #[ignore = "blocked on quadraui: form_field_measure has no FieldKind::Toolbar arm"]
    fn plugin_view_toolbar_button_click_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk();
        clicking_a_toolbar_button_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_labelled_group_with_a_control_row_is_painted_on_gtk() {
        let (h, _dir) = gtk_named(SECTION_VIEW);
        labelled_group_with_a_control_row_is_painted(&h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_clicking_a_collapsible_section_header_toggles_its_member_on_gtk() {
        let (mut h, _dir) = gtk_named(SECTION_VIEW);
        clicking_a_collapsible_section_header_toggles_its_member(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_keyboard_activation_reaches_the_handler_on_gtk() {
        let (mut h, _dir) = gtk();
        keyboard_activation_reaches_the_plugin_handler(&mut h.driver);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn plugin_view_shift_tab_navigates_backward_on_gtk() {
        let (mut h, _dir) = gtk();
        shift_tab_navigates_backward_to_the_previous_field(&mut h.driver);
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
