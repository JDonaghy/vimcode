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
//! `quadraui::FieldKind` it is translated to (and back) in exactly one place,
//! [`crate::render::plugin_view_to_form`]. Exposing a new primitive is an
//! allow-list entry here plus a matching arm in that one translation
//! function — never a hand-invented representation.
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
//! `Date` and `Slider`) is additive and does **not** bump
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
    pub fn field_index(&self, field_id: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.id == field_id)
    }

    /// The first field index that can take keyboard focus (skips `Label` and
    /// `ReadOnly` rows, which emit no events), or `None` when the view is inert.
    pub fn first_focusable(&self) -> Option<usize> {
        self.fields
            .iter()
            .position(|f| f.is_interactive() && !f.disabled)
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
    List {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        items: Vec<ViewListItem>,
    },
    /// Expandable nodes.
    Tree {
        #[serde(default)]
        nodes: Vec<ViewTreeNode>,
    },
    /// Columns + rows, optionally with editable cells.
    Table {
        #[serde(default)]
        columns: Vec<ViewTableColumn>,
        #[serde(default)]
        rows: Vec<ViewTableRow>,
    },
    /// Multi-line read-only text with scrolling. An optional `filetype`
    /// hint enables syntax highlighting (e.g. `"json"`).
    TextView {
        #[serde(default)]
        text: String,
        #[serde(default)]
        filetype: Option<String>,
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
        }
    }
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
    pub fn is_interactive(&self) -> bool {
        !matches!(
            self.kind,
            ViewFieldKind::Label | ViewFieldKind::ReadOnly { .. }
        )
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

/// The input variant carried by a [`ViewField`].
///
/// The serde tag is the Lua-facing `type = "..."` key, so the Rust enum and the
/// documented Lua vocabulary cannot drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewFieldKind {
    /// A non-interactive section header. `label` carries the text.
    ///
    /// No `quadraui::FieldKind` — `plugin_view_to_form` paints this as a
    /// label-only `FormField` with no input, the one row shape quadraui's
    /// own descriptor has no need to name separately.
    Label,
    /// Single-line text input. Translated to `quadraui::FieldKind::TextInput`
    /// (its `multiline: false`, `cursor`/`selection_anchor` interaction state
    /// stays Engine-owned — see the module doc's firewall reasons).
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
    /// adjustment. Translated to `quadraui::FieldKind::Slider` — the
    /// vocabulary's demonstration that adding a primitive is an allow-list
    /// entry (this variant plus one arm in
    /// [`crate::render::plugin_view_to_form`]), not a new representation:
    /// the field names below are exactly quadraui's own.
    ///
    /// `quadraui::FormController` has no native keyboard path for sliders
    /// (unlike [`ViewFieldKind::Date`]'s scrub-and-confirm); `Enter`/`Space`
    /// on a focused slider steps `value` by `step`, clamped to
    /// `[min, max]`, and reaches the plugin immediately as a
    /// [`ViewEventKind::SliderChanged`] — there is no separate "confirm"
    /// step.
    Slider {
        value: f32,
        min: f32,
        max: f32,
        #[serde(default = "default_slider_step")]
        step: f32,
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
    /// A `slider` field's value changed (Enter/Space step, or a drag/click
    /// through `quadraui::FormController`). `value` is already clamped to
    /// the field's declared `[min, max]`.
    SliderChanged {
        value: f32,
    },
    FocusChanged,
    // ── ViewBody events ──────────────────────────────────────────────────
    /// A `List`/`Table` row moved into selection (index into
    /// `items`/`rows`).
    ItemSelected {
        index: usize,
    },
    /// A `List`/`Table` row was activated (Enter or double-click).
    ItemActivated {
        index: usize,
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
            ViewEventKind::FocusChanged => "FocusChanged",
            ViewEventKind::ItemSelected { .. } => "ItemSelected",
            ViewEventKind::ItemActivated { .. } => "ItemActivated",
            ViewEventKind::NodeSelected { .. } => "NodeSelected",
            ViewEventKind::NodeActivated { .. } => "NodeActivated",
            ViewEventKind::Expanded { .. } => "Expanded",
            ViewEventKind::Collapsed { .. } => "Collapsed",
            ViewEventKind::CellEdited { .. } => "CellEdited",
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
    fn adding_slider_is_additive_old_schema_version_1_json_still_parses() {
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
                },
                ViewTableColumn {
                    title: "Value".into(),
                    editable: true,
                },
            ],
            rows: vec![ViewTableRow {
                id: "row-1".into(),
                cells: vec!["Content-Type".into(), "application/json".into()],
            }],
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
    }
}
