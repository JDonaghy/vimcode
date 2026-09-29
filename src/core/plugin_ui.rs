//! Plugin-declared UI views — the `vimcode.ui.*` widget vocabulary (#146).
//!
//! A Lua plugin registers a *view* (`vimcode.ui.register_view`) whose `render`
//! callback returns a declarative widget tree. Vimcode converts that tree into
//! a `quadraui` primitive (`render::plugin_view_to_form`), paints it through the
//! shared `quadraui::FormController` both backends already use for the Settings
//! panel, and routes the resulting widget events back to the plugin's
//! `on_event` callback.
//!
//! # ABI decision: a vimcode-owned vocabulary, not quadraui's serde shape
//!
//! #1403 asks which wire format plugins author against. This module is the
//! answer: **a small vimcode-owned vocabulary**, versioned by
//! [`VIEW_SCHEMA_VERSION`], converted to quadraui primitives in one place.
//! Feeding quadraui's own `Serialize`/`Deserialize` impls straight from Lua
//! ("verbatim quadraui serde") was rejected for three concrete reasons, all
//! visible in `quadraui::Form` as pinned today:
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
//! The design invariants #146 lists are still honoured, because this vocabulary
//! keeps them rather than because quadraui's structs are used directly: ids are
//! owned `String`s allocated at runtime (1), events are plain data
//! ([`PluginViewEvent`]) with no closures crossing the Rust/Lua boundary (2),
//! every type here is `Serialize + Deserialize` (3), ids are namespaced per
//! plugin by [`namespaced_widget_id`] (4), every event names its widget (5),
//! and nothing borrows from engine state (6).
//!
//! # Schema version
//!
//! A view may declare `schema_version = N`. A view declaring a version newer
//! than [`VIEW_SCHEMA_VERSION`] is rejected at parse time with a Lua error
//! rather than being silently half-rendered, which is what makes it safe to add
//! required semantics to version 2 later.

use serde::{Deserialize, Serialize};

/// The widget-vocabulary version this build understands.
pub const VIEW_SCHEMA_VERSION: u32 = 1;

/// Prefix reserved for plugin-owned widget ids (#146 invariant 4).
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

/// One plugin-declared view: a vertical stack of fields.
///
/// This is the whole tree in schema version 1. Nested containers are a
/// deliberate non-goal for now — `quadraui::Form` is itself a flat field stack,
/// and every widget the vocabulary exposes maps onto one of its rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginView {
    /// Plugin-chosen id for the tree as a whole (used as the form's id).
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub fields: Vec<ViewField>,
}

impl Default for PluginView {
    fn default() -> Self {
        Self {
            id: String::new(),
            schema_version: VIEW_SCHEMA_VERSION,
            fields: Vec::new(),
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

/// The input variant carried by a [`ViewField`].
///
/// The serde tag is the Lua-facing `type = "..."` key, so the Rust enum and the
/// documented Lua vocabulary cannot drift.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ViewFieldKind {
    /// A non-interactive section header. `label` carries the text.
    Label,
    /// Single-line text input.
    Text {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    /// Single-line text input rendered masked. Value is plaintext.
    Password {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
    /// Multi-line text input.
    TextArea {
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default = "default_text_area_rows")]
        rows: usize,
    },
    /// Boolean checkbox.
    Toggle {
        #[serde(default)]
        value: bool,
    },
    /// A single clickable button; `label` is the caption.
    Button,
    /// Read-only value row.
    ReadOnly {
        #[serde(default)]
        value: String,
    },
    /// Single choice from a collapsed dropdown.
    Dropdown {
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        selected: usize,
    },
    /// Single choice with every option visible.
    Segmented {
        #[serde(default)]
        options: Vec<String>,
        #[serde(default)]
        selected: usize,
    },
    /// A horizontal row of buttons, each with its own id.
    Buttons {
        #[serde(default)]
        buttons: Vec<ViewButton>,
    },
    /// A horizontal row of named toggles, each with its own id.
    Toggles {
        #[serde(default)]
        toggles: Vec<ViewToggle>,
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

/// What happened to a widget. Plain data — no closures cross the Rust/Lua
/// boundary (#146 invariant 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ViewEventKind {
    ButtonClicked,
    ToggleChanged { value: bool },
    DropdownChanged { selected: usize },
    SegmentedChanged { selected: usize },
    TextChanged { value: String },
    TextCommitted { value: String },
    FocusChanged,
}

impl ViewEventKind {
    /// The `event.kind` string handed to Lua. Matches #146's illustrative API
    /// (`event.kind == "ButtonClicked"`).
    pub fn kind_name(&self) -> &'static str {
        match self {
            ViewEventKind::ButtonClicked => "ButtonClicked",
            ViewEventKind::ToggleChanged { .. } => "ToggleChanged",
            ViewEventKind::DropdownChanged { .. } => "DropdownChanged",
            ViewEventKind::SegmentedChanged { .. } => "SegmentedChanged",
            ViewEventKind::TextChanged { .. } => "TextChanged",
            ViewEventKind::TextCommitted { .. } => "TextCommitted",
            ViewEventKind::FocusChanged => "FocusChanged",
        }
    }
}

/// One widget event, already resolved to the plugin view that owns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginViewEvent {
    /// The registered view name (the `ext:` panel name).
    pub view: String,
    /// The plugin-authored field id — *not* the namespaced `quadraui::WidgetId`.
    /// Plugins see the ids they wrote (#146's `event.widget_id == "send"`).
    pub widget_id: String,
    pub kind: ViewEventKind,
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
    }
}
