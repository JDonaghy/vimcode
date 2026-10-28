//! Plugin-declared UI views — the `vimcode.ui.*` widget vocabulary.
//!
//! A Lua plugin registers a *view* (`vimcode.ui.register_view`) whose `render`
//! callback returns a declarative widget tree. Vimcode converts that tree into
//! a `quadraui` primitive (`render::plugin_view_to_form`), paints it through the
//! shared `quadraui::FormController` both backends already use for the Settings
//! panel, and routes the resulting widget events back to the plugin's
//! `on_event` callback.
//!
//! # ABI design: a curated subset-profile of quadraui's descriptor JSON
//!
//! This module defines **a small, documented vocabulary**, versioned by
//! [`VIEW_SCHEMA_VERSION`], that is a curated allow-list of
//! `quadraui::FieldKind`/`FormEvent` variants rather than an independent
//! representation: every [`ViewFieldKind`] variant's doc comment names the
//! `quadraui::FieldKind` it is translated to (and back), and the translation
//! itself lives in exactly one place, [`crate::render::plugin_view_to_form`].
//! Exposing a new primitive that has a direct one-field-to-one-field quadraui
//! counterpart is usually just an allow-list entry here plus a matching arm
//! in that one translation function — never a hand-invented representation
//! — but is not guaranteed to be *only* that: a primitive quadraui's own
//! `FormController` has no native interaction path for (as `Slider` turned
//! out to be, see its doc below) also needs vimcode's own keyboard-handling
//! arm in `core::engine::plugins`, plus the matching Lua parse/encode arms in
//! `plugin.rs`. The ceiling is still small and enumerable, not open-ended.
//!
//! Feeding quadraui's own `Serialize`/`Deserialize` impls straight from Lua
//! is avoided for three concrete reasons, all visible in `quadraui::Form`:
//!
//! 1. **quadraui primitives carry app-owned *interaction* state, not just a
//!    declaration.** `FieldKind::TextInput` has `cursor: Option<usize>` and
//!    `selection_anchor: Option<usize>` as *byte offsets*, and `Form` has
//!    `focused_field` / `scroll_offset` / `has_focus`. Those belong to vimcode
//!    (they must survive a re-render, and a plugin must not be able to park the
//!    cursor mid-codepoint). A verbatim ABI would make every plugin author
//!    hand-maintain them.
//! 2. **Text is `StyledText` (a span list), not `String`.** Verbatim serde
//!    would force `label = { spans = { { text = "URL", style = ... } } }` on
//!    every row of hand-written Lua.
//! 3. **The quadraui pin moves often** (see `Cargo.toml`'s pin history). A
//!    verbatim ABI would make every third-party plugin's UI a hostage of the
//!    next `rev = "..."` bump; here a breaking upstream change is absorbed by
//!    [`crate::render::plugin_view_to_form`] and nothing else.
//!
//! The design ensures ids are owned `String`s allocated at runtime,
//! events are plain data ([`PluginViewEvent`]) with no closures crossing the
//! Rust/Lua boundary, every type here is `Serialize + Deserialize`, ids are
//! namespaced per plugin by [`namespaced_widget_id`], every event names its
//! widget, and nothing borrows from engine state.
//!
//! # Schema version
//!
//! A view may declare `schema_version = N`. A view declaring a version newer
//! than [`VIEW_SCHEMA_VERSION`] is rejected at parse time with a Lua error
//! rather than being silently half-rendered, which is what makes it safe to add
//! required semantics to version 2 later.
//!
//! Adding a new [`ViewFieldKind`]/[`ViewEventKind`] variant (most recently
//! `Date`, `Slider`, `Section`, `Color`, and `Toolbar`) is additive and does
//! **not** bump
//! [`VIEW_SCHEMA_VERSION`]: every pre-existing variant's wire tag and fields
//! are untouched, so a `schema_version = 1` view written before the new
//! variant existed still parses byte-for-byte the same, and the new variant
//! only appears in views that opt in by declaring it. A bump to version 2 is
//! reserved for a change that would make an *existing* `type`/`kind` tag mean
//! something different, or make a currently-optional field required.

use serde::{Deserialize, Serialize};

/// The widget-vocabulary version this build understands.
pub const VIEW_SCHEMA_VERSION: u32 = 1;

/// Prefix reserved for plugin-owned widget ids.
pub const WIDGET_ID_PREFIX: &str = "plugin:";

/// Build the namespaced `quadraui::WidgetId` string for a plugin field, so two
/// plugins that both call a button `"send"` cannot collide.
pub fn namespaced_widget_id(view: &str, field: &str) -> String {
    format!("{WIDGET_ID_PREFIX}{view}:{field}")
}

/// Inverse of [`namespaced_widget_id`]: `("my-ext", "send")` from
/// `"plugin:my-ext:send"`. Returns `None` for any id not in the plugin
/// namespace (so a stray Settings-panel id can never be mistaken for one).
pub fn split_widget_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix(WIDGET_ID_PREFIX)?;
    let (view, field) = rest.split_once(':')?;
    if view.is_empty() {
        return None;
    }
    Some((view, field))
}

fn default_schema_version() -> u32 {
    VIEW_SCHEMA_VERSION
}

/// One plugin-declared view: either a vertical stack of fields or a single
/// [`ViewBody`] widget.
///
/// A view with `body: None` is a field stack — `fields` is the whole tree.
/// Nested containers inside a field stack are not supported; `quadraui::Form`
/// is itself a flat field stack. A view with `body: Some(_)` is instead one
/// list/tree/table/text-view widget filling the whole panel; `fields` is
/// ignored in that case. The two are mutually exclusive per view (a Lua
/// `render()` returns one shape or the other), not layered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginView {
    /// Plugin-chosen id for the tree as a whole (used as the form's id).
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub fields: Vec<ViewField>,
    /// An alternative single-widget body. `None` keeps the original field-stack
    /// shape; additive and does not require a schema version bump.
    #[serde(default)]
    pub body: Option<ViewBody>,
}

impl Default for PluginView {
    fn default() -> Self {
        Self {
            id: String::new(),
            schema_version: VIEW_SCHEMA_VERSION,
            fields: Vec::new(),
            body: None,
        }
    }
}

impl PluginView {
    /// Index of the field carrying `field_id`, or `None`.
    ///
    /// Unaffected by [`Self::visible_field_indices`]: a hidden field (a
    /// collapsed section's member) still has a stable index here, since
    /// click/selection-sync code resolves ids after decoding an event, not
    /// before — a plugin can still name a field it declared even while it's
    /// collapsed out of view.
    pub fn field_index(&self, field_id: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.id == field_id)
    }

    /// Indices into `self.fields` that are currently painted/focusable —
    /// every field except one nested under a currently-collapsed
    /// [`ViewFieldKind::Section`].
    ///
    /// A `Section` header is a flat marker, not a container: every field
    /// declared after one, up to the next `Section` header or the end of the
    /// list, is implicitly its member by declaration order. The header row
    /// itself is always visible, even while collapsed (its disclosure
    /// glyph/label is how a user re-expands it).
    ///
    /// The single source of truth for "is this field shown right now" —
    /// `render::plugin_view_to_form`'s field-building loop and
    /// `focused_field` lookup, `core::engine::plugins::handle_plugin_view_key`'s
    /// `focusable` set, and [`Self::first_focusable`] all compute visibility
    /// through this one method so they can never disagree about it.
    pub fn visible_field_indices(&self) -> Vec<usize> {
        let mut visible = Vec::with_capacity(self.fields.len());
        let mut hiding = false;
        for (i, f) in self.fields.iter().enumerate() {
            if let ViewFieldKind::Section {
                collapsible,
                collapsed,
            } = &f.kind
            {
                visible.push(i);
                hiding = *collapsible && *collapsed;
                continue;
            }
            if !hiding {
                visible.push(i);
            }
        }
        visible
    }

    /// The first field index that can take keyboard focus (skips `Label` and
    /// `ReadOnly` rows, which emit no events, and any field currently hidden
    /// inside a collapsed section), or `None` when the view is inert.
    pub fn first_focusable(&self) -> Option<usize> {
        self.visible_field_indices()
            .into_iter()
            .find(|&i| self.fields[i].is_interactive() && !self.fields[i].disabled)
    }
}

// ─── List / Tree / Table / TextView bodies ──────────────────────────────────

/// A single-widget view body covering lists, trees, tables, and text views.
///
/// The serde tag is the Lua-facing `kind = "..."` key on the table a
/// `render()` callback returns, exactly like [`ViewFieldKind`]'s `type` tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewBody {
    /// Selectable rows with an optional title and per-row detail text.
    /// Painted through `quadraui::ListView`/`ListItem` directly (see
    /// `crate::render::plugin_view_body`) — there is no composed quadraui
    /// controller for selection/scroll, so that part is hand-rolled in that
    /// backend-neutral module rather than through `FormController`.
    List {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        items: Vec<ViewListItem>,
    },
    /// Expandable nodes. Flattened to `quadraui::TreeRow`s and painted
    /// through `quadraui::TreeView`, with keyboard/expand-collapse handling
    /// going through quadraui's own `TreeController`.
    Tree {
        #[serde(default)]
        nodes: Vec<ViewTreeNode>,
    },
    /// Columns + rows, optionally with editable cells. Painted through
    /// `quadraui::DataTable` (`Column`/`DataRow`); like `List`, selection
    /// and scroll are hand-rolled since `DataTable` has no composed
    /// controller of its own.
    Table {
        #[serde(default)]
        columns: Vec<ViewTableColumn>,
        #[serde(default)]
        rows: Vec<ViewTableRow>,
        /// When true, rows support multi-selection (toggle/range-extend via
        /// click modifiers) in addition to the always-available single-row
        /// cursor/`ItemSelected`/`ItemActivated` path. Rendered as a
        /// `\u{2610}`/`\u{2611}` glyph prefix on the first cell of each row
        /// (the same checkbox convention `ai_chat_and_misc_builders`'s
        /// plan-entry glyph uses), since `quadraui::DataTable` has no native
        /// multi-row highlight to key off of.
        #[serde(default)]
        multi_select: bool,
        /// Pinned footer row, excluded from scrolling/selection, rendered
        /// via `quadraui::DataTable`'s own `footer: Option<DataRow>` field.
        /// One string per column; `None` renders no footer.
        #[serde(default)]
        footer: Option<Vec<String>>,
    },
    /// Multi-line read-only text with scrolling. An optional `filetype`
    /// hint enables syntax highlighting (e.g. `"json"`). Painted through
    /// `quadraui::TextDisplay`.
    TextView {
        #[serde(default)]
        text: String,
        #[serde(default)]
        filetype: Option<String>,
    },
    /// One or more numeric series, rendered as a sparkline, line, bar, or
    /// grouped-bar chart. Painted through `quadraui::Chart`/`ChartLayout`
    /// via `Backend::draw_chart`, which paints and returns a hit-testable
    /// layout in a single call — there is no separate scroll/selection
    /// state to hand-roll, unlike `List`/`Table`.
    Chart {
        #[serde(default)]
        chart_kind: ViewChartKind,
        #[serde(default)]
        series: Vec<ViewChartSeries>,
        #[serde(default)]
        x_label: Option<String>,
        #[serde(default)]
        y_label: Option<String>,
        #[serde(default)]
        show_legend: bool,
    },
}

impl ViewBody {
    /// The `kind = "..."` string this variant is authored as.
    pub fn kind_name(&self) -> &'static str {
        match self {
            ViewBody::List { .. } => "list",
            ViewBody::Tree { .. } => "tree",
            ViewBody::Table { .. } => "table",
            ViewBody::TextView { .. } => "text_view",
            ViewBody::Chart { .. } => "chart",
        }
    }
}

/// Which `quadraui::ChartKind` a [`ViewBody::Chart`] renders as.
///
/// Mirrors `quadraui::ChartKind` one-for-one rather than re-exporting it
/// directly, so the Lua-facing `chart_kind = "..."` string stays a plain
/// snake_case tag independent of quadraui's own `Serialize` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewChartKind {
    /// No axes, labels, or legend — a compact trend indicator.
    Sparkline,
    /// One connected line per series.
    #[default]
    Line,
    /// Bars; multiple series stack bottom-up (a no-op for a single series).
    Bar,
    /// Bars; multiple series sit side by side within each category.
    BarGrouped,
}

impl ViewChartKind {
    /// Translate to the quadraui primitive this paints through.
    pub fn into_quadraui(self) -> quadraui::ChartKind {
        match self {
            ViewChartKind::Sparkline => quadraui::ChartKind::Sparkline,
            ViewChartKind::Line => quadraui::ChartKind::Line,
            ViewChartKind::Bar => quadraui::ChartKind::Bar,
            ViewChartKind::BarGrouped => quadraui::ChartKind::BarGrouped,
        }
    }
}

/// One data series in a [`ViewBody::Chart`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewChartSeries {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub data: Vec<f64>,
    /// `"#rrggbb"` hex string. `None`, or a string that fails to parse,
    /// falls back to the backend's own per-series palette color — the same
    /// degrade-don't-error convention `ViewFieldKind::Color`'s malformed-hex
    /// handling uses.
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub fill: bool,
}

/// One row in a [`ViewBody::List`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewListItem {
    /// Plugin-authored row id, namespaced the same way a field id is.
    pub id: String,
    #[serde(default)]
    pub text: String,
    /// Optional right-aligned secondary text.
    #[serde(default)]
    pub detail: Option<String>,
}

/// One node in a [`ViewBody::Tree`]. Nesting is real here (unlike the field
/// stack) because a tree's whole point is hierarchy; `children` is walked
/// recursively to build the flattened `quadraui::TreeView` rows a frame
/// paints, and a `TreePath` (index chain) addresses back into it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewTreeNode {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// Whether this node is currently drawn open. Plugin-owned: vimcode
    /// emits `Expanded`/`Collapsed` on toggle and expects the next
    /// `render()` to reflect the plugin's own updated state, exactly like
    /// `ViewFieldKind::Toggle`'s `value`.
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub children: Vec<ViewTreeNode>,
}

impl ViewTreeNode {
    /// A branch (has a chevron) iff it declares any children — a leaf never
    /// shows one, matching `quadraui::TreeRow`'s `is_expanded: Option<bool>`
    /// convention (`None` = leaf).
    pub fn is_branch(&self) -> bool {
        !self.children.is_empty()
    }

    /// Resolve a `quadraui::TreePath` (`Vec<u16>`) index chain back to the
    /// node it addresses, or `None` if the path is stale (out of range).
    /// Takes `&[u16]` rather than re-exporting `quadraui::TreePath` here.
    /// This module stays free of quadraui types to isolate the ABI.
    pub fn resolve<'a>(nodes: &'a [ViewTreeNode], path: &[u16]) -> Option<&'a ViewTreeNode> {
        let (&first, rest) = path.split_first()?;
        let node = nodes.get(first as usize)?;
        if rest.is_empty() {
            Some(node)
        } else {
            ViewTreeNode::resolve(&node.children, rest)
        }
    }
}

/// One column in a [`ViewBody::Table`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewTableColumn {
    #[serde(default)]
    pub title: String,
    /// Whether cells in this column may be edited in place.
    /// Editable cells are keyed by `"r{row}c{col}"` address.
    #[serde(default)]
    pub editable: bool,
    /// Fixed column width, in the host surface's own unit (character cells
    /// on the TUI backend, pixels on GTK/macOS/Windows). Takes priority over
    /// `min_width` when both are set. `None` leaves the column proportional
    /// (`quadraui::ColumnWidth::Flex`), the same as a column that declares
    /// neither field.
    #[serde(default)]
    pub width: Option<f32>,
    /// Lower bound a content-sized column may shrink to, in the same unit as
    /// `width`. Ignored when `width` is set. `None` with `width` also unset
    /// leaves the column proportional.
    #[serde(default)]
    pub min_width: Option<f32>,
    /// Horizontal text alignment, translated to `quadraui::ColumnAlign` by
    /// `crate::render::plugin_view_body`. This module stays free of
    /// quadraui types (see [`ViewTreeNode::resolve`]), so the enum lives
    /// here rather than re-exporting quadraui's.
    #[serde(default)]
    pub align: TableColumnAlign,
}

/// Horizontal text alignment for a [`ViewTableColumn`]. Mirrors
/// `quadraui::ColumnAlign` without depending on quadraui from this module.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableColumnAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// One row in a [`ViewBody::Table`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewTableRow {
    /// Plugin-authored row id.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub cells: Vec<String>,
}

/// A single row: a label plus an input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewField {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub disabled: bool,
    /// Renders a red indicator + message; wins over `hint` visually.
    #[serde(default)]
    pub error: Option<String>,
    /// Renders a yellow indicator + message. Ignored when `error` is set.
    #[serde(default)]
    pub warning: Option<String>,
    pub kind: ViewFieldKind,
}

impl ViewField {
    /// Whether this row can emit a [`PluginViewEvent`] at all.
    ///
    /// A collapsible [`ViewFieldKind::Section`] header counts as interactive
    /// — it's the thing a user tabs to and presses Enter/Space on to
    /// toggle it — while a non-collapsible one is a plain header, as inert
    /// as `Label`.
    pub fn is_interactive(&self) -> bool {
        match &self.kind {
            ViewFieldKind::Label | ViewFieldKind::ReadOnly { .. } => false,
            ViewFieldKind::Section { collapsible, .. } => *collapsible,
            _ => true,
        }
    }
}

/// Default row count for a `text_area` field.
fn default_text_area_rows() -> usize {
    3
}

/// Default step for a `slider` field — matches quadraui's own
/// `FieldKind::Slider` default so an author who omits `step` gets the same
/// behaviour on both sides of the translation.
fn default_slider_step() -> f32 {
    1.0
}

/// Default `collapsible` for a `section` field — a header an author declares
/// with no `collapsible` key gets the more useful behaviour (a disclosure
/// triangle the user can act on) rather than silently being a dead label.
fn default_section_collapsible() -> bool {
    true
}

/// The input variant carried by a [`ViewField`].
///
/// The serde tag is the Lua-facing `type = "..."` key, so the Rust enum and the
/// documented Lua vocabulary cannot drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewFieldKind {
    /// A non-interactive section header. `label` carries the text.
    ///
    /// Translated to `quadraui::FieldKind::Label` directly — both sides are
    /// already just "a header row, no interaction, no `FormEvent`".
    Label,
    /// Single-line text input. Translated to `quadraui::FieldKind::TextInput`
    /// (its `cursor`/`selection_anchor` interaction state stays Engine-owned
    /// — see the module doc's firewall reasons). Multi-line input is the
    /// separate [`ViewFieldKind::TextArea`] variant, translated to
    /// `quadraui::FieldKind::TextArea` instead — `TextInput` itself has no
    /// multi-line flag on either side of the translation.
    Text {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    /// Single-line text input rendered masked. Translated to
    /// `quadraui::FieldKind::PasswordInput`. Value is plaintext on this
    /// side of the firewall.
    Password {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    /// Multi-line text input. Translated to `quadraui::FieldKind::TextArea`.
    TextArea {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default = "default_text_area_rows")]
        rows: usize,
    },
    /// Boolean checkbox. Translated to `quadraui::FieldKind::Toggle`.
    Toggle {
        #[serde(default)]
        value: bool,
    },
    /// A single clickable button; `label` is the caption. Translated to
    /// `quadraui::FieldKind::Button`.
    Button,
    /// Read-only value row. Translated to `quadraui::FieldKind::ReadOnly`.
    ReadOnly {
        #[serde(default)]
        value: String,
    },
    /// Single choice from a collapsed dropdown. Translated to
    /// `quadraui::FieldKind::Dropdown`.
    Dropdown {
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        selected: usize,
    },
    /// Single choice with every option visible. Translated to
    /// `quadraui::FieldKind::SegmentedControl`.
    Segmented {
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        selected: usize,
    },
    /// A horizontal row of buttons, each with its own id. Translated to
    /// `quadraui::FieldKind::ButtonRow`.
    Buttons {
        #[serde(default)]
        buttons: Vec<ViewButton>,
    },
    /// A horizontal row of named toggles, each with its own id. Translated
    /// to `quadraui::FieldKind::ToggleGroup`.
    Toggles {
        #[serde(default)]
        toggles: Vec<ViewToggle>,
    },
    /// A calendar date picker. Translated to `quadraui::FieldKind::DatePicker`.
    /// `value` is an ISO8601 `YYYY-MM-DD` string, or
    /// empty for unset. An empty or malformed `value` starts the picker on
    /// `1970-01-01` with no day selected — declare a real starting date to
    /// avoid landing the user decades away from today.
    ///
    /// While this field has focus, `Left`/`Right`/`Up`/`Down`/`PageUp`/
    /// `PageDown` scrub a day/month that paints immediately but is held by
    /// `quadraui::FormController`'s own cache, not by the plugin, until
    /// `Enter` confirms it as a [`ViewEventKind::DateChanged`] event. That
    /// cache survives this field being re-declared with a different
    /// `value` while it stays focused — a plugin whose `on_event` ignores
    /// `DateChanged` sees its own declared value repainted only once focus
    /// leaves and returns to this field, which reseeds the cache.
    Date {
        #[serde(default)]
        value: String,
    },
    /// A numeric slider within `[min, max]`, stepping by `step` per
    /// adjustment. Translated to `quadraui::FieldKind::Slider`; the field
    /// names below are exactly quadraui's own.
    ///
    /// `quadraui::FormController` has no native keyboard *or* mouse/drag
    /// path for sliders at all (unlike [`ViewFieldKind::Date`]'s own
    /// scrub-and-confirm keyboard path) — this variant is translated by
    /// `plugin_view_to_form`, but stepping it is vimcode's own hand-written
    /// handling in `core::engine::plugins`, not something the controller
    /// does for free. `Enter`/`Space` on a focused slider steps `value` by
    /// `step`, clamped to `[min, max]`, and reaches the plugin immediately
    /// as a [`ViewEventKind::SliderChanged`] — there is no separate
    /// "confirm" step, and no mouse/drag path exists yet on either side.
    Slider {
        value: f32,
        min: f32,
        max: f32,
        #[serde(default = "default_slider_step")]
        step: f32,
    },
    /// A color swatch. `value` is a `"#rrggbb"` hex string. A valid value is
    /// translated to `quadraui::FieldKind::ColorPicker`; an empty or malformed
    /// value falls back to a read-only row echoing the raw string, so the
    /// plugin author sees what they typed instead of a wrong swatch.
    ///
    /// `quadraui::FormController` emits no `FormEvent::ColorChanged` from any
    /// click or key path — the popup/hex-input editor the kind's own doc
    /// describes is explicitly left to the app. This variant is therefore
    /// display-only for now: Enter/Space on a focused `color` field is a
    /// no-op, same as [`Self::Label`]/[`Self::ReadOnly`], and there is no
    /// matching `ViewEventKind` yet. A plugin that wants to change the color
    /// re-declares `value` itself (e.g. from a `Buttons` row of presets).
    Color {
        #[serde(default)]
        value: String,
    },
    /// A horizontal strip of toolbar buttons (icons, key hints, separators).
    /// Translated to `quadraui::FieldKind::Toolbar`.
    ///
    /// `quadraui::FormController` hit-tests clicks against each button's id
    /// natively, but — like [`Self::Buttons`]/[`Self::Toggles`] — has no
    /// keyboard path: `Enter`/`Space` on a focused `toolbar` field is
    /// vimcode's own handling in `core::engine::plugins`, activating the
    /// first non-disabled [`ViewToolbarButton`] (`ToolbarButton::Separator`/
    /// `ToolbarButton::Label` entries have no id and are never activated) and
    /// reporting it as a [`ViewEventKind::ToolbarButtonClicked`].
    Toolbar {
        #[serde(default)]
        buttons: Vec<ViewToolbarButton>,
    },
    /// A group header for every field declared after it, up to the next
    /// `Section` or the end of the list. `label` carries the group's title.
    ///
    /// This is a flat marker, not a container — `PluginView::fields` stays a
    /// single flat `Vec`; see [`PluginView::visible_field_indices`] for how
    /// membership and visibility are derived from declaration order. A
    /// labelled group with a horizontal control row is a non-collapsible
    /// `Section` immediately followed by a [`Self::Buttons`]/[`Self::Toggles`]/
    /// [`Self::Segmented`] field.
    ///
    /// `collapsible: false` translates to `quadraui::FieldKind::Label` — a
    /// plain header, exactly like [`Self::Label`]. `collapsible: true`
    /// translates to `quadraui::FieldKind::Button` instead, so focusing it
    /// and pressing Enter/Space (or clicking it) raises quadraui's own
    /// `FormEvent::ButtonClicked`, which `render::plugin_view_to_form`'s
    /// callers special-case back into a [`ViewEventKind::SectionToggled`]
    /// carrying the *proposed* next `collapsed` value — the plugin, not
    /// vimcode, decides whether to re-render with members hidden, same
    /// convention as every other field kind here. The painted label is
    /// prefixed with a disclosure glyph (▸ collapsed, ▾ expanded).
    Section {
        #[serde(default = "default_section_collapsible")]
        collapsible: bool,
        #[serde(default)]
        collapsed: bool,
    },
}

impl ViewFieldKind {
    /// The `type = "..."` string this variant is authored as.
    pub fn type_name(&self) -> &'static str {
        match self {
            ViewFieldKind::Label => "label",
            ViewFieldKind::Text { .. } => "text",
            ViewFieldKind::Password { .. } => "password",
            ViewFieldKind::TextArea { .. } => "text_area",
            ViewFieldKind::Toggle { .. } => "toggle",
            ViewFieldKind::Button => "button",
            ViewFieldKind::ReadOnly { .. } => "read_only",
            ViewFieldKind::Dropdown { .. } => "dropdown",
            ViewFieldKind::Segmented { .. } => "segmented",
            ViewFieldKind::Buttons { .. } => "buttons",
            ViewFieldKind::Toggles { .. } => "toggles",
            ViewFieldKind::Date { .. } => "date",
            ViewFieldKind::Slider { .. } => "slider",
            ViewFieldKind::Color { .. } => "color",
            ViewFieldKind::Toolbar { .. } => "toolbar",
            ViewFieldKind::Section { .. } => "section",
        }
    }
}

/// One button inside a [`ViewFieldKind::Buttons`] row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewButton {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub disabled: bool,
}

/// One toggle inside a [`ViewFieldKind::Toggles`] row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewToggle {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: bool,
}

/// One button inside a [`ViewFieldKind::Toolbar`] row. Mirrors
/// `quadraui::ToolbarButton::Action`'s fields; a toolbar's `Separator`/
/// `Label` entries carry no id and so have no representation here — a
/// plugin that wants one declares two separate `Toolbar` fields instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewToolbarButton {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub key_hint: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

// ─── Events ──────────────────────────────────────────────────────────────────

/// What happened to a widget. Plain data with no closures crossing the
/// Rust/Lua boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ViewEventKind {
    ButtonClicked,
    ToggleChanged {
        value: bool,
    },
    DropdownChanged {
        selected: usize,
    },
    SegmentedChanged {
        selected: usize,
    },
    TextChanged {
        value: String,
    },
    TextCommitted {
        value: String,
    },
    /// A `date` field's selection was confirmed (Enter). `value` is an
    /// ISO8601 `YYYY-MM-DD` string.
    DateChanged {
        value: String,
    },
    /// A `slider` field's value changed — currently only an Enter/Space
    /// step; `quadraui::FormController` has no drag/click path for sliders
    /// to route through (see [`ViewFieldKind::Slider`]'s doc). `value` is
    /// already clamped to the field's declared `[min, max]`.
    SliderChanged {
        value: f32,
    },
    /// A collapsible `section` header was activated (Enter/Space/click).
    /// `collapsed` is the *proposed* next state (`!collapsed` of the
    /// field as currently declared) — same convention as every other
    /// field kind here: vimcode never mutates the plugin's declared state
    /// itself, it reports what the user asked for and re-renders with
    /// whatever the plugin's `on_event` declares back.
    SectionToggled {
        collapsed: bool,
    },
    /// A `toolbar` field's button was activated (Enter/Space/click). Carries
    /// no payload of its own — same convention as [`Self::ButtonClicked`]:
    /// the activated [`ViewToolbarButton::id`] is reported as the dispatched
    /// [`PluginViewEvent::widget_id`], not duplicated here.
    ToolbarButtonClicked,
    FocusChanged,
    // ── ViewBody events ──────────────────────────────────────────────────
    /// A `List`/`Table` row moved into selection (index into
    /// `items`/`rows`). `col` is the 0-based column a mouse click landed in
    /// — `Some` for every `Table` click, including a read-only column,
    /// since it reports what was clicked rather than what became editable;
    /// `None` for a `List` view (no columns) and for keyboard navigation
    /// (`j`/`k`/Home/End), which has no per-click column to report. Lua sees
    /// this as `nil` when `None`.
    ItemSelected {
        index: usize,
        col: Option<usize>,
    },
    /// A `List`/`Table` row was activated (Enter or double-click). `col`
    /// follows the same convention as [`Self::ItemSelected`]'s.
    ItemActivated {
        index: usize,
        col: Option<usize>,
    },
    /// A `Tree` node moved into selection, named by its declared id (a flat
    /// index would be unstable across expand/collapse).
    NodeSelected {
        id: String,
    },
    /// A `Tree` node was activated (Enter or double-click).
    NodeActivated {
        id: String,
    },
    /// A `Tree` node's chevron was opened.
    Expanded {
        id: String,
    },
    /// A `Tree` node's chevron was closed.
    Collapsed {
        id: String,
    },
    /// An editable `Table` cell was committed to a new value.
    CellEdited {
        row: usize,
        col: usize,
        value: String,
    },
    /// A `Table` column header was clicked. The engine's own display-order
    /// overlay (ascending/descending/unsorted) already cycled by the time
    /// this reaches the plugin, so a handler that wants to sort its own
    /// `rows` and `render` again is layering a second, plugin-owned sort on
    /// top rather than replacing the built-in one.
    HeaderClicked {
        col: usize,
    },
    /// A `Chart` data point was clicked.
    DataPointClicked {
        series_idx: usize,
        data_idx: usize,
    },
    /// A `Chart` legend entry was clicked.
    LegendClicked {
        series_idx: usize,
    },
    /// A `Table` view declaring `multi_select: true` changed its
    /// multi-selection set by mouse (click with Shift/Ctrl/Cmd, or plain
    /// click resetting it to one row — a double click, which only
    /// activates, never touches it). `indices` are row indices into the
    /// declared `rows` list (stable across header-sort reorders), sorted
    /// ascending and deduplicated. Fires alongside — never instead of —
    /// [`Self::ItemSelected`] on that click path, which keeps reporting the
    /// single-row cursor regardless of `multi_select`. Keyboard navigation
    /// never touches the multi-selection set, so only [`Self::ItemSelected`]
    /// fires for it.
    SelectionChanged {
        indices: Vec<usize>,
    },
}

impl ViewEventKind {
    /// The `event.kind` string handed to Lua.
    pub fn kind_name(&self) -> &'static str {
        match self {
            ViewEventKind::ButtonClicked => "ButtonClicked",
            ViewEventKind::ToggleChanged { .. } => "ToggleChanged",
            ViewEventKind::DropdownChanged { .. } => "DropdownChanged",
            ViewEventKind::SegmentedChanged { .. } => "SegmentedChanged",
            ViewEventKind::TextChanged { .. } => "TextChanged",
            ViewEventKind::TextCommitted { .. } => "TextCommitted",
            ViewEventKind::DateChanged { .. } => "DateChanged",
            ViewEventKind::SliderChanged { .. } => "SliderChanged",
            ViewEventKind::SectionToggled { .. } => "SectionToggled",
            ViewEventKind::ToolbarButtonClicked => "ToolbarButtonClicked",
            ViewEventKind::FocusChanged => "FocusChanged",
            ViewEventKind::ItemSelected { .. } => "ItemSelected",
            ViewEventKind::ItemActivated { .. } => "ItemActivated",
            ViewEventKind::NodeSelected { .. } => "NodeSelected",
            ViewEventKind::NodeActivated { .. } => "NodeActivated",
            ViewEventKind::Expanded { .. } => "Expanded",
            ViewEventKind::Collapsed { .. } => "Collapsed",
            ViewEventKind::CellEdited { .. } => "CellEdited",
            ViewEventKind::HeaderClicked { .. } => "HeaderClicked",
            ViewEventKind::DataPointClicked { .. } => "DataPointClicked",
            ViewEventKind::LegendClicked { .. } => "LegendClicked",
            ViewEventKind::SelectionChanged { .. } => "SelectionChanged",
        }
    }
}

/// Encode a `(row, col)` table-cell address as the `field_id` for an
/// editable table cell, reusing the text-entry model instead of a parallel one.
pub fn table_cell_field_id(row: usize, col: usize) -> String {
    format!("r{row}c{col}")
}

/// Inverse of [`table_cell_field_id`]. Returns `None` for any string that
/// isn't one of its outputs (defensive — a stray field id must never be
/// misread as a cell address).
pub fn parse_table_cell_field_id(field_id: &str) -> Option<(usize, usize)> {
    let rest = field_id.strip_prefix('r')?;
    let (row, rest) = rest.split_once('c')?;
    Some((row.parse().ok()?, rest.parse().ok()?))
}

/// One widget event, already resolved to the plugin view that owns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginViewEvent {
    /// The registered view name (the `ext:` panel name).
    pub view: String,
    /// The plugin-authored field id — *not* the namespaced `quadraui::WidgetId`.
    /// Plugins see the ids they wrote.
    pub widget_id: String,
    pub kind: ViewEventKind,
}

// ─── In-panel text entry ────────────────────────────────────────────────────

/// vimcode-owned edit state for whichever `Text`/`Password`/`TextArea` field
/// currently has keyboard focus, in either the sidebar or an editor-area tab.
///
/// This is `Engine`-side interaction state, never serialized across the
/// Rust/Lua boundary — plugins see only the committed value, via
/// [`ViewEventKind::TextChanged`]/[`ViewEventKind::TextCommitted`]. Plugins
/// cannot park the cursor mid-codepoint or corrupt vimcode's interaction state,
/// because they never have a handle to uncommitted state.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginViewTextEditState {
    /// The view that owns the focused field.
    pub view: String,
    /// The plugin-authored field id (not the namespaced `quadraui::WidgetId`).
    pub field_id: String,
    /// The live, possibly-uncommitted text.
    pub value: String,
    /// Byte offset into `value`.
    pub cursor: usize,
    /// Byte offset into `value`; `Some(n)` with `n != cursor` means the range
    /// between the two is selected. `None` means no selection.
    pub selection_anchor: Option<usize>,
}

impl PluginViewTextEditState {
    /// Sorted `(lo, hi)` selection byte range, or `None` when there is no
    /// selection (matches `Engine::explorer_rename`'s identical helper).
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_anchor
            .map(|a| (a.min(self.cursor), a.max(self.cursor)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_ids_round_trip_through_the_plugin_namespace() {
        let id = namespaced_widget_id("my-ext", "send");
        assert_eq!(id, "plugin:my-ext:send");
        assert_eq!(split_widget_id(&id), Some(("my-ext", "send")));
    }

    #[test]
    fn two_plugins_using_the_same_field_id_get_distinct_widget_ids() {
        assert_ne!(
            namespaced_widget_id("ext-a", "send"),
            namespaced_widget_id("ext-b", "send")
        );
    }

    #[test]
    fn a_non_plugin_widget_id_is_not_claimed_by_the_plugin_namespace() {
        // The Settings panel's own field ids must never resolve here.
        assert_eq!(split_widget_id("settings"), None);
        assert_eq!(split_widget_id("plugin:"), None);
        assert_eq!(split_widget_id("plugin::send"), None);
    }

    #[test]
    fn view_json_round_trips() {
        let view = PluginView {
            id: "main-form".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                ViewField {
                    id: "url".into(),
                    label: "URL".into(),
                    hint: String::new(),
                    disabled: false,
                    error: None,
                    warning: None,
                    kind: ViewFieldKind::Text {
                        value: "https://example.com".into(),
                        placeholder: "https://".into(),
                    },
                },
                ViewField {
                    id: "send".into(),
                    label: "Send".into(),
                    hint: String::new(),
                    disabled: false,
                    error: None,
                    warning: None,
                    kind: ViewFieldKind::Button,
                },
            ],
        };
        let json = serde_json::to_string(&view).expect("serialize");
        let back: PluginView = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(view, back);
    }

    #[test]
    fn field_kind_tag_is_the_lua_type_string() {
        let json = r#"{"type":"dropdown","options":["GET","POST"],"selected":1}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(kind.type_name(), "dropdown");
        assert_eq!(
            kind,
            ViewFieldKind::Dropdown {
                options: vec!["GET".into(), "POST".into()],
                selected: 1,
            }
        );
    }

    #[test]
    fn date_field_kind_tag_is_the_lua_type_string() {
        let json = r#"{"type":"date","value":"2024-03-15"}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(kind.type_name(), "date");
        assert_eq!(
            kind,
            ViewFieldKind::Date {
                value: "2024-03-15".into(),
            }
        );
    }

    #[test]
    fn slider_field_kind_tag_is_the_lua_type_string() {
        let json = r#"{"type":"slider","value":5.0,"min":0.0,"max":10.0,"step":0.5}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(kind.type_name(), "slider");
        assert_eq!(
            kind,
            ViewFieldKind::Slider {
                value: 5.0,
                min: 0.0,
                max: 10.0,
                step: 0.5,
            }
        );
    }

    #[test]
    fn slider_field_kind_step_defaults_like_quadraui() {
        let json = r#"{"type":"slider","value":5.0,"min":0.0,"max":10.0}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            kind,
            ViewFieldKind::Slider {
                value: 5.0,
                min: 0.0,
                max: 10.0,
                step: 1.0,
            }
        );
    }

    #[test]
    fn color_field_kind_tag_is_the_lua_type_string() {
        let json = r##"{"type":"color","value":"#ff00aa"}"##;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(kind.type_name(), "color");
        assert_eq!(
            kind,
            ViewFieldKind::Color {
                value: "#ff00aa".into(),
            }
        );
    }

    #[test]
    fn toolbar_field_kind_tag_is_the_lua_type_string() {
        let json = r#"{"type":"toolbar","buttons":[
            {"id":"save","label":"Save"},
            {"id":"undo","label":"Undo","disabled":true}
        ]}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(kind.type_name(), "toolbar");
        assert_eq!(
            kind,
            ViewFieldKind::Toolbar {
                buttons: vec![
                    ViewToolbarButton {
                        id: "save".into(),
                        label: "Save".into(),
                        icon: None,
                        key_hint: None,
                        disabled: false,
                    },
                    ViewToolbarButton {
                        id: "undo".into(),
                        label: "Undo".into(),
                        icon: None,
                        key_hint: None,
                        disabled: true,
                    },
                ],
            }
        );
    }

    #[test]
    fn adding_slider_is_additive_old_schema_version_1_json_still_parses() {
        // Exercises the serde shape directly; production code never
        // deserializes `PluginView` from JSON — the real author-facing path
        // is `lua_table_to_view` (see `plugin.rs`'s own
        // `an_unknown_field_type_is_an_error_not_a_silent_drop` and sibling
        // tests for additive-schema coverage through that path). This test
        // still earns its keep as the narrowest possible check that the
        // wire shape of a pre-existing variant is untouched by the new one.
        //
        // A `date` field declared before `Slider` existed, under the same
        // `schema_version = 1` this build still understands.
        let json = r#"{"id":"v","schema_version":1,"fields":[
            {"id":"dob","kind":{"type":"date","value":"2024-03-15"}}
        ]}"#;
        let view: PluginView = serde_json::from_str(json).expect("deserialize");
        assert_eq!(view.schema_version, 1);
        assert_eq!(
            view.fields[0].kind,
            ViewFieldKind::Date {
                value: "2024-03-15".into(),
            }
        );
    }

    #[test]
    fn first_focusable_skips_inert_rows() {
        let inert = |id: &str, kind: ViewFieldKind| ViewField {
            id: id.into(),
            label: String::new(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind,
        };
        let view = PluginView {
            id: "v".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                inert("hdr", ViewFieldKind::Label),
                inert(
                    "ro",
                    ViewFieldKind::ReadOnly {
                        value: "1.0".into(),
                    },
                ),
                inert("go", ViewFieldKind::Button),
            ],
        };
        assert_eq!(view.first_focusable(), Some(2));
        assert_eq!(view.field_index("go"), Some(2));
    }

    // ── Section ─────────────────────────────────────────────────────────

    #[test]
    fn section_field_kind_tag_is_the_lua_type_string() {
        let json = r#"{"type":"section","collapsible":true,"collapsed":false}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(kind.type_name(), "section");
        assert_eq!(
            kind,
            ViewFieldKind::Section {
                collapsible: true,
                collapsed: false,
            }
        );
    }

    #[test]
    fn section_field_kind_collapsible_defaults_to_true() {
        let json = r#"{"type":"section"}"#;
        let kind: ViewFieldKind = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            kind,
            ViewFieldKind::Section {
                collapsible: true,
                collapsed: false,
            }
        );
    }

    #[test]
    fn section_toggled_event_kind_name_is_section_toggled() {
        assert_eq!(
            ViewEventKind::SectionToggled { collapsed: true }.kind_name(),
            "SectionToggled"
        );
    }

    #[test]
    fn adding_section_is_additive_old_schema_version_1_json_still_parses() {
        // Narrowest possible check (mirrors
        // `adding_slider_is_additive_old_schema_version_1_json_still_parses`)
        // that a pre-existing variant's wire shape is untouched by the new
        // `section` tag.
        let json = r#"{"id":"v","schema_version":1,"fields":[
            {"id":"dob","kind":{"type":"date","value":"2024-03-15"}}
        ]}"#;
        let view: PluginView = serde_json::from_str(json).expect("deserialize");
        assert_eq!(view.schema_version, 1);
        assert_eq!(
            view.fields[0].kind,
            ViewFieldKind::Date {
                value: "2024-03-15".into(),
            }
        );
    }

    fn inert_field(id: &str, kind: ViewFieldKind) -> ViewField {
        ViewField {
            id: id.into(),
            label: String::new(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind,
        }
    }

    #[test]
    fn collapsed_section_hides_its_members_but_not_its_header() {
        let view = PluginView {
            id: "v".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                inert_field(
                    "hdr",
                    ViewFieldKind::Section {
                        collapsible: true,
                        collapsed: true,
                    },
                ),
                inert_field("member", ViewFieldKind::Button),
                inert_field("after", ViewFieldKind::Button),
            ],
        };
        // The header (index 0) stays visible; with no terminating `Section`
        // header, the collapse swallows every field after it through the
        // end of the list (indices 1 and 2 both hide).
        assert_eq!(view.visible_field_indices(), vec![0]);
        assert_eq!(view.first_focusable(), Some(0));
    }

    #[test]
    fn expanded_section_shows_its_members() {
        let view = PluginView {
            id: "v".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                inert_field(
                    "hdr",
                    ViewFieldKind::Section {
                        collapsible: true,
                        collapsed: false,
                    },
                ),
                inert_field("member", ViewFieldKind::Button),
            ],
        };
        assert_eq!(view.visible_field_indices(), vec![0, 1]);
    }

    #[test]
    fn non_collapsible_section_never_hides_members_and_is_inert() {
        let view = PluginView {
            id: "v".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                inert_field(
                    "hdr",
                    ViewFieldKind::Section {
                        collapsible: false,
                        collapsed: true,
                    },
                ),
                inert_field("member", ViewFieldKind::Button),
            ],
        };
        assert_eq!(view.visible_field_indices(), vec![0, 1]);
        // The non-collapsible header itself is skipped by `first_focusable`,
        // exactly like `Label` — it's a plain group title, not a control.
        assert_eq!(view.first_focusable(), Some(1));
    }

    #[test]
    fn two_sibling_sections_do_not_leak_into_each_other() {
        let view = PluginView {
            id: "v".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            body: None,
            fields: vec![
                inert_field(
                    "a-hdr",
                    ViewFieldKind::Section {
                        collapsible: true,
                        collapsed: true,
                    },
                ),
                inert_field("a-member", ViewFieldKind::Button),
                inert_field(
                    "b-hdr",
                    ViewFieldKind::Section {
                        collapsible: true,
                        collapsed: false,
                    },
                ),
                inert_field("b-member", ViewFieldKind::Button),
            ],
        };
        // `a-member` (index 1) hides under `a`'s collapse; `b-hdr`/`b-member`
        // (indices 2, 3) are unaffected by `a`'s state.
        assert_eq!(view.visible_field_indices(), vec![0, 2, 3]);
    }

    // ── ViewBody ────────────────────────────────────────────────────────

    #[test]
    fn view_body_list_round_trips_and_tags_as_list() {
        let json = r#"{"kind":"list","title":"Items","items":[
            {"id":"a","text":"Alpha","detail":"1"},
            {"id":"b","text":"Bravo"}
        ]}"#;
        let body: ViewBody = serde_json::from_str(json).expect("deserialize");
        assert_eq!(body.kind_name(), "list");
        match &body {
            ViewBody::List { title, items } => {
                assert_eq!(title.as_deref(), Some("Items"));
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].detail.as_deref(), Some("1"));
                assert_eq!(items[1].detail, None);
            }
            other => panic!("expected List, got {other:?}"),
        }
        let back: ViewBody = serde_json::from_str(&serde_json::to_string(&body).unwrap()).unwrap();
        assert_eq!(body, back);
    }

    #[test]
    fn view_body_chart_round_trips_and_tags_as_chart() {
        let json = r##"{"kind":"chart","chart_kind":"bar","series":[
            {"label":"cpu","data":[1.0,2.0,3.0],"color":"#ff0000","fill":false},
            {"label":"mem","data":[4.0,5.0,6.0]}
        ],"x_label":"time","y_label":"percent","show_legend":true}"##;
        let body: ViewBody = serde_json::from_str(json).expect("deserialize");
        assert_eq!(body.kind_name(), "chart");
        match &body {
            ViewBody::Chart {
                chart_kind,
                series,
                x_label,
                y_label,
                show_legend,
            } => {
                assert_eq!(*chart_kind, ViewChartKind::Bar);
                assert_eq!(series.len(), 2);
                assert_eq!(series[0].label, "cpu");
                assert_eq!(series[0].color.as_deref(), Some("#ff0000"));
                assert_eq!(series[1].color, None);
                assert_eq!(x_label.as_deref(), Some("time"));
                assert_eq!(y_label.as_deref(), Some("percent"));
                assert!(*show_legend);
            }
            other => panic!("expected Chart, got {other:?}"),
        }
        let back: ViewBody = serde_json::from_str(&serde_json::to_string(&body).unwrap()).unwrap();
        assert_eq!(body, back);
    }

    #[test]
    fn view_chart_kind_defaults_to_line() {
        let json = r#"{"kind":"chart","series":[{"label":"s","data":[1.0]}]}"#;
        let body: ViewBody = serde_json::from_str(json).expect("deserialize");
        match body {
            ViewBody::Chart { chart_kind, .. } => assert_eq!(chart_kind, ViewChartKind::Line),
            other => panic!("expected Chart, got {other:?}"),
        }
    }

    #[test]
    fn view_body_unknown_kind_is_a_parse_error() {
        let json = r#"{"kind":"grid","rows":[]}"#;
        let err = serde_json::from_str::<ViewBody>(json).unwrap_err();
        // Not asserting exact wording (serde's own), just that an unknown
        // `kind` is rejected rather than silently defaulting to some variant.
        assert!(err.to_string().contains("grid") || err.to_string().contains("kind"));
    }

    #[test]
    fn view_tree_node_resolve_walks_a_path() {
        let nodes = vec![ViewTreeNode {
            id: "root".into(),
            label: "Root".into(),
            expanded: true,
            children: vec![
                ViewTreeNode {
                    id: "child-a".into(),
                    label: "A".into(),
                    expanded: false,
                    children: vec![],
                },
                ViewTreeNode {
                    id: "child-b".into(),
                    label: "B".into(),
                    expanded: false,
                    children: vec![ViewTreeNode {
                        id: "grandchild".into(),
                        label: "GC".into(),
                        expanded: false,
                        children: vec![],
                    }],
                },
            ],
        }];
        assert_eq!(
            ViewTreeNode::resolve(&nodes, &[0]).map(|n| n.id.as_str()),
            Some("root")
        );
        assert_eq!(
            ViewTreeNode::resolve(&nodes, &[0, 1]).map(|n| n.id.as_str()),
            Some("child-b")
        );
        assert_eq!(
            ViewTreeNode::resolve(&nodes, &[0, 1, 0]).map(|n| n.id.as_str()),
            Some("grandchild")
        );
        assert_eq!(ViewTreeNode::resolve(&nodes, &[0, 9]), None);
        assert_eq!(ViewTreeNode::resolve(&nodes, &[]), None);
        assert!(nodes[0].is_branch());
        assert!(!nodes[0].children[0].is_branch());
    }

    #[test]
    fn view_body_table_round_trips() {
        let body = ViewBody::Table {
            columns: vec![
                ViewTableColumn {
                    title: "Key".into(),
                    editable: true,
                    width: None,
                    min_width: None,
                    align: TableColumnAlign::Left,
                },
                ViewTableColumn {
                    title: "Value".into(),
                    editable: true,
                    width: Some(12.0),
                    min_width: None,
                    align: TableColumnAlign::Right,
                },
            ],
            rows: vec![ViewTableRow {
                id: "row-1".into(),
                cells: vec!["Content-Type".into(), "application/json".into()],
            }],
            multi_select: true,
            footer: Some(vec!["Total".into(), "1 row".into()]),
        };
        let json = serde_json::to_string(&body).unwrap();
        let back: ViewBody = serde_json::from_str(&json).unwrap();
        assert_eq!(body, back);
        assert_eq!(back.kind_name(), "table");
    }

    #[test]
    fn view_body_text_view_defaults_filetype_to_none() {
        let json = r#"{"kind":"text_view","text":"hello"}"#;
        let body: ViewBody = serde_json::from_str(json).unwrap();
        assert_eq!(
            body,
            ViewBody::TextView {
                text: "hello".into(),
                filetype: None,
            }
        );
    }

    #[test]
    fn table_cell_field_id_round_trips() {
        let id = table_cell_field_id(3, 7);
        assert_eq!(id, "r3c7");
        assert_eq!(parse_table_cell_field_id(&id), Some((3, 7)));
        assert_eq!(parse_table_cell_field_id("not-a-cell"), None);
        assert_eq!(parse_table_cell_field_id("r3"), None);
        assert_eq!(parse_table_cell_field_id("rXc7"), None);
    }

    #[test]
    fn plugin_view_with_body_ignores_fields_by_convention() {
        let view = PluginView {
            id: "resp".into(),
            schema_version: VIEW_SCHEMA_VERSION,
            fields: vec![],
            body: Some(ViewBody::TextView {
                text: "{}".into(),
                filetype: Some("json".into()),
            }),
        };
        let json = serde_json::to_string(&view).unwrap();
        let back: PluginView = serde_json::from_str(&json).unwrap();
        assert_eq!(view, back);
        assert!(back.body.is_some());
    }

    #[test]
    fn event_kind_names_match_the_documented_lua_strings() {
        assert_eq!(ViewEventKind::ButtonClicked.kind_name(), "ButtonClicked");
        assert_eq!(
            ViewEventKind::ToggleChanged { value: true }.kind_name(),
            "ToggleChanged"
        );
        assert_eq!(
            ViewEventKind::TextCommitted { value: "x".into() }.kind_name(),
            "TextCommitted"
        );
        assert_eq!(
            ViewEventKind::DateChanged {
                value: "2024-03-15".into(),
            }
            .kind_name(),
            "DateChanged"
        );
        assert_eq!(
            ViewEventKind::SliderChanged { value: 3.5 }.kind_name(),
            "SliderChanged"
        );
        assert_eq!(
            ViewEventKind::ToolbarButtonClicked.kind_name(),
            "ToolbarButtonClicked"
        );
        assert_eq!(
            ViewEventKind::DataPointClicked {
                series_idx: 0,
                data_idx: 1,
            }
            .kind_name(),
            "DataPointClicked"
        );
        assert_eq!(
            ViewEventKind::LegendClicked { series_idx: 0 }.kind_name(),
            "LegendClicked"
        );
    }
}
