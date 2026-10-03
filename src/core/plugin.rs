//! Lua plugin system for VimCode.
//!
//! Plugins are `.lua` files (or directories with `init.lua`) placed in
//! `~/.config/vimcode/plugins/`. They are loaded in alphabetical order at
//! startup and have access to the `vimcode.*` Lua API.
//!
//! The plugin system is intentionally unrestricted (Neovim-style): plugins have
//! full access to file I/O, OS processes, and the network. Users are
//! responsible for trusting the plugins they install.
//!
//! # Two API tiers: legacy queued (`vimcode.buf.*`) vs immediate (`vimcode.buffer.*`)
//!
//! There are two ways for Lua to touch the editor, and they differ in *when*
//! the edit lands (#1214):
//!
//! * **Legacy, queued — `vimcode.buf.*`, `vimcode.opt.*`, …** Reads come from
//!   a snapshot taken *before* the callback ran ([`PluginCallContext`]); writes
//!   are appended to `Vec`s on that context and replayed by
//!   `Engine::apply_plugin_ctx` *after* the callback returns. Read-after-write
//!   inside one callback therefore sees the *old* text. This behaviour is
//!   relied on by shipped extensions and is preserved exactly.
//! * **Immediate — `vimcode.buffer.*`, `vimcode.window.*`.** These reach a live
//!   `&mut Engine` and take effect at call time, so a `set_lines` followed by a
//!   `get_lines` in the same callback reads back what was just written, and a
//!   buffer other than the active one can be targeted by handle.
//!
//! **Mixing the two in one callback:** every immediate call has already been
//! applied by the time the callback returns, and the queued calls are applied
//! afterwards — so a queued write always lands *after* (and therefore wins
//! over) an immediate write to the same lines, no matter the order the Lua
//! source called them in.
//!
//! ## Handles
//!
//! Buffers and windows are plain integers (`BufferId` / `WindowId`), with `0`
//! meaning "current". Real ids always start at 1, so `0` is never ambiguous.
//!
//! ## Line and cursor conventions (immediate API)
//!
//! * `get_lines(buf, start, end)` / `set_lines(buf, start, end, lines)` are
//!   **0-indexed with an exclusive end**, and a negative index counts back from
//!   the end of the buffer — the same convention as the legacy
//!   `vimcode.buf.get_lines` / `set_lines`.
//! * `get_lines` returns lines **without their trailing newline**. (The legacy
//!   `vimcode.buf.get_lines` passes ropey's `Rope::line()` straight through, so
//!   its strings *do* include the terminator. That difference is deliberate;
//!   the legacy behaviour is frozen for back-compat.)
//! * `line_count(buf)` counts **logical** lines: a buffer ending in a newline
//!   does *not* report ropey's phantom trailing empty line, so
//!   `set_lines(b, 0, line_count(b), get_lines(b, 0, line_count(b)))` is a
//!   round-trip. (Legacy `vimcode.buf.line_count()` returns the raw
//!   `Rope::len_lines()` and so is one larger on such buffers; also frozen.)
//! * Every line written by `set_lines` is newline-terminated, matching the
//!   queued replay path.
//! * Cursors are `{line, col}` **1-indexed**, matching `vimcode.buf.cursor()`.
//!   `window.get_cursor` returns a table carrying both named (`line`, `col`)
//!   and positional (`[1]`, `[2]`) fields, and `window.set_cursor` accepts
//!   either shape.

use mlua::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use super::buffer::{DecorOpts, HlGroupDef, VirtTextChunk, VirtTextPos};
use super::engine::{
    expand_leader_tokens, parse_key_sequence, Engine, UserKeymap, UserKeymapAction,
};

/// A request from Lua to open a scratch buffer with content.
pub struct ScratchBufferRequest {
    pub name: String,
    pub content: String,
    pub read_only: bool,
    pub filetype: Option<String>,
    /// None = current window, "vertical" or "horizontal" split.
    pub split: Option<String>,
}

/// A request from Lua to run a shell command in a background thread.
pub struct AsyncShellRequest {
    pub command: String,
    pub callback_event: String,
    pub stdin: Option<String>,
    pub cwd: Option<PathBuf>,
}

use super::git;

// ─── Live engine access for the immediate API (#1214) ────────────────────────

thread_local! {
    /// The engine currently loaned to Lua, or null when no plugin dispatch is
    /// in flight. Thread-local because `mlua::Lua` is `!Send`: every callback
    /// runs on the thread that owns the `PluginManager`, which is the thread
    /// that owns the `Engine`.
    static LIVE_ENGINE: Cell<*mut Engine> = const { Cell::new(std::ptr::null_mut()) };
    /// Set while a `&mut Engine` reborrow of [`LIVE_ENGINE`] is live, so a
    /// nested reborrow can be refused instead of aliasing.
    static ENGINE_BORROWED: Cell<bool> = const { Cell::new(false) };
}

/// RAII loan of `&mut Engine` into the Lua state for the duration of one
/// plugin dispatch.
///
/// # Why a guarded raw pointer and not `Lua::scope`
///
/// #1214 requires that a Lua function which was **stored and invoked later**
/// (kept in a Lua table or the registry and fired from a subsequent event) can
/// use the immediate API — that is the shape a timer callback and a UI widget
/// event handler will have. `Lua::scope` cannot express that: everything it
/// creates is non-`'static` and may not escape the scope, so a callback stored
/// at load time could not be handed a scoped buffer/window object, and a scoped
/// *function* cannot be put in the registry at all. Integer handles plus a
/// pointer parked outside the Lua value graph have no lifetime relationship to
/// the Lua values that use them, so "stored at load, invoked much later" needs
/// no special support: whatever dispatch is in flight installs the loan, and
/// every Lua function reached from it — however it was reached — sees it.
///
/// # Borrow discipline (asserted, not assumed)
///
/// 1. `EngineLoan` borrows the `Engine` for the loan's whole lifetime, so the
///    caller cannot touch `self` while Lua may be mutating through the pointer.
/// 2. Reborrows are short: [`with_live_engine`] hands `&mut Engine` to exactly
///    one closure and drops it before returning to Lua.
/// 3. While a reborrow is live, `ENGINE_BORROWED` is set and a nested
///    [`with_live_engine`] returns `None` (debug builds assert) rather than
///    creating a second `&mut`.
/// 4. Nested *dispatch* — Lua → engine edit → event → Lua — cannot happen at
///    all: `Engine::plugin_event` defers a nested event onto a queue instead of
///    re-entering Lua (see `Engine::plugin_dispatch_depth`).
/// 5. Loans nest lexically (`prev` is restored on drop), so an engine that
///    somehow dispatches from inside a dispatch still restores the outer
///    pointer rather than clearing it.
pub(crate) struct EngineLoan<'a> {
    prev: *mut Engine,
    /// Holds the `&mut Engine` borrow for the loan's lifetime (rule 1).
    _borrow: PhantomData<&'a mut Engine>,
}

impl<'a> EngineLoan<'a> {
    pub(crate) fn new(engine: &'a mut Engine) -> Self {
        let ptr: *mut Engine = engine;
        let prev = LIVE_ENGINE.with(|c| c.replace(ptr));
        Self {
            prev,
            _borrow: PhantomData,
        }
    }
}

impl Drop for EngineLoan<'_> {
    fn drop(&mut self) {
        LIVE_ENGINE.with(|c| c.set(self.prev));
    }
}

/// Run `f` against the engine currently loaned to Lua.
///
/// Returns `None` when there is no live engine (the immediate API was called
/// outside any plugin dispatch) or when a reborrow is already live.
pub(crate) fn with_live_engine<R>(f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    let ptr = LIVE_ENGINE.with(|c| c.get());
    if ptr.is_null() {
        return None;
    }
    if ENGINE_BORROWED.with(|b| b.replace(true)) {
        debug_assert!(
            false,
            "vimcode immediate API re-entered while the engine was already \
             borrowed — see EngineLoan's borrow discipline"
        );
        return None;
    }
    /// Clears the borrow flag even if `f` panics (a Lua error unwinds through
    /// here), so one failed call cannot wedge the API for the whole session.
    struct ClearOnDrop;
    impl Drop for ClearOnDrop {
        fn drop(&mut self) {
            ENGINE_BORROWED.with(|b| b.set(false));
        }
    }
    let _clear = ClearOnDrop;
    // SAFETY: `ptr` came from `EngineLoan::new(&mut Engine)`, whose lifetime
    // holds that borrow open; the loan is still on the stack (it is what put a
    // non-null pointer here, and it nulls/restores the slot on drop), so the
    // `Engine` is alive and not otherwise aliased. `ENGINE_BORROWED` above
    // guarantees this is the only live reborrow, and the thread-local keeps
    // this the same thread that created the loan.
    let engine: &mut Engine = unsafe { &mut *ptr };
    Some(f(engine))
}

/// [`with_live_engine`], but surfacing the "no live engine" case to Lua as a
/// runtime error rather than a silent `nil`.
fn live_engine<R>(what: &str, f: impl FnOnce(&mut Engine) -> R) -> LuaResult<R> {
    with_live_engine(f).ok_or_else(|| {
        LuaError::RuntimeError(format!(
            "{what}: no live editor — the immediate vimcode.buffer/vimcode.window \
             API is only available while a vimcode callback is running"
        ))
    })
}

// ─── Lua → plugin_ui vocabulary (#146) ───────────────────────────────────────

/// Parse the table a view's `render()` returned into a
/// [`crate::core::plugin_ui::PluginView`].
///
/// Hand-written rather than derived through `mlua`'s serde bridge on purpose:
/// the errors are the ones a plugin author needs ("field 3: unknown type
/// \"buton\"") and the parse cannot be perturbed by an `mlua` feature flag.
/// The accepted vocabulary is still pinned to the serde tags — every `type`
/// string below is a [`crate::core::plugin_ui::ViewFieldKind::type_name`].
fn lua_table_to_view(tbl: &LuaTable) -> LuaResult<crate::core::plugin_ui::PluginView> {
    use crate::core::plugin_ui::{
        PluginView, ViewButton, ViewField, ViewFieldKind, ViewToggle, VIEW_SCHEMA_VERSION,
    };

    let schema_version: u32 = tbl.get("schema_version").unwrap_or(VIEW_SCHEMA_VERSION);
    if schema_version > VIEW_SCHEMA_VERSION {
        return Err(LuaError::RuntimeError(format!(
            "view declares schema_version {schema_version}, but this vimcode \
             understands at most {VIEW_SCHEMA_VERSION}"
        )));
    }
    let id: String = tbl.get("id").unwrap_or_default();

    // #1631: a `kind = "list"/"tree"/"table"/"text_view"` table is a single
    // `ViewBody` widget, not a field stack — parse it and return early,
    // exactly the "one sibling translation per kind" the issue asks for,
    // kept out of the `fields` loop below rather than interleaved with it.
    if let Ok(kind) = tbl.get::<_, String>("kind") {
        let body = lua_table_to_view_body(&kind, tbl)?;
        return Ok(PluginView {
            id,
            schema_version,
            fields: Vec::new(),
            body: Some(body),
        });
    }

    let mut fields = Vec::new();
    if let Ok(fields_tbl) = tbl.get::<_, LuaTable>("fields") {
        for (idx, row) in fields_tbl.sequence_values::<LuaTable>().enumerate() {
            let row = row?;
            let type_name: String = row.get("type").unwrap_or_else(|_| "label".to_string());
            let field_id: String = row.get("id").unwrap_or_default();
            let label: String = row.get("label").unwrap_or_default();
            let hint: String = row.get("hint").unwrap_or_default();
            let disabled: bool = row.get("disabled").unwrap_or(false);
            let error: Option<String> =
                row.get::<_, String>("error").ok().filter(|s| !s.is_empty());
            let warning: Option<String> = row
                .get::<_, String>("warning")
                .ok()
                .filter(|s| !s.is_empty());
            let value_str = |key: &str| -> String { row.get::<_, String>(key).unwrap_or_default() };
            let options = |key: &str| -> Vec<String> {
                row.get::<_, LuaTable>(key)
                    .map(|t| t.sequence_values::<String>().flatten().collect())
                    .unwrap_or_default()
            };
            // `selected` is 0-based, matching the Rust/JSON shape. Lua's
            // 1-based array convention applies to the `options` table itself,
            // not to an index the plugin stores as data.
            let selected: usize = row.get("selected").unwrap_or(0);

            let kind = match type_name.as_str() {
                "label" => ViewFieldKind::Label,
                "text" => ViewFieldKind::Text {
                    value: value_str("value"),
                    placeholder: value_str("placeholder"),
                },
                "password" => ViewFieldKind::Password {
                    value: value_str("value"),
                    placeholder: value_str("placeholder"),
                },
                "text_area" => ViewFieldKind::TextArea {
                    value: value_str("value"),
                    placeholder: value_str("placeholder"),
                    rows: row.get("rows").unwrap_or(3),
                },
                "toggle" => ViewFieldKind::Toggle {
                    value: row.get("value").unwrap_or(false),
                },
                "button" => ViewFieldKind::Button,
                "read_only" => ViewFieldKind::ReadOnly {
                    value: value_str("value"),
                },
                "dropdown" => ViewFieldKind::Dropdown {
                    options: options("options"),
                    selected,
                },
                "segmented" => ViewFieldKind::Segmented {
                    options: options("options"),
                    selected,
                },
                "buttons" => {
                    let mut buttons = Vec::new();
                    if let Ok(t) = row.get::<_, LuaTable>("buttons") {
                        for b in t.sequence_values::<LuaTable>().flatten() {
                            buttons.push(ViewButton {
                                id: b.get("id").unwrap_or_default(),
                                label: b.get("label").unwrap_or_default(),
                                disabled: b.get("disabled").unwrap_or(false),
                            });
                        }
                    }
                    ViewFieldKind::Buttons { buttons }
                }
                "toggles" => {
                    let mut toggles = Vec::new();
                    if let Ok(t) = row.get::<_, LuaTable>("toggles") {
                        for b in t.sequence_values::<LuaTable>().flatten() {
                            toggles.push(ViewToggle {
                                id: b.get("id").unwrap_or_default(),
                                label: b.get("label").unwrap_or_default(),
                                value: b.get("value").unwrap_or(false),
                            });
                        }
                    }
                    ViewFieldKind::Toggles { toggles }
                }
                other => {
                    return Err(LuaError::RuntimeError(format!(
                        "view field {} (id {field_id:?}): unknown type {other:?}",
                        idx + 1
                    )));
                }
            };
            fields.push(ViewField {
                id: field_id,
                label,
                hint,
                disabled,
                error,
                warning,
                kind,
            });
        }
    }

    Ok(PluginView {
        id,
        schema_version,
        fields,
        body: None,
    })
}

/// Parse a `kind = "..."` table into a [`crate::core::plugin_ui::ViewBody`]
/// (#1631). Sibling of the field-stack loop in [`lua_table_to_view`], not
/// interleaved with it — see that function's call site for why.
fn lua_table_to_view_body(
    kind: &str,
    tbl: &LuaTable,
) -> LuaResult<crate::core::plugin_ui::ViewBody> {
    use crate::core::plugin_ui::{ViewBody, ViewListItem, ViewTableColumn, ViewTableRow};

    match kind {
        "list" => {
            let title: Option<String> =
                tbl.get::<_, String>("title").ok().filter(|s| !s.is_empty());
            let mut items = Vec::new();
            if let Ok(items_tbl) = tbl.get::<_, LuaTable>("items") {
                for (idx, row) in items_tbl.sequence_values::<LuaTable>().enumerate() {
                    let row = row?;
                    let id: String = row.get("id").map_err(|_| {
                        LuaError::RuntimeError(format!("list item {}: missing id", idx + 1))
                    })?;
                    items.push(ViewListItem {
                        id,
                        text: row.get("text").unwrap_or_default(),
                        detail: row
                            .get::<_, String>("detail")
                            .ok()
                            .filter(|s| !s.is_empty()),
                    });
                }
            }
            Ok(ViewBody::List { title, items })
        }
        "tree" => {
            let nodes_tbl: LuaTable = tbl.get("nodes").map_err(|_| {
                LuaError::RuntimeError("tree view: missing \"nodes\" table".to_string())
            })?;
            Ok(ViewBody::Tree {
                nodes: lua_table_to_tree_nodes(nodes_tbl)?,
            })
        }
        "table" => {
            let mut columns = Vec::new();
            if let Ok(cols_tbl) = tbl.get::<_, LuaTable>("columns") {
                for (idx, row) in cols_tbl.sequence_values::<LuaTable>().enumerate() {
                    let row = row?;
                    let title: String = row.get("title").map_err(|_| {
                        LuaError::RuntimeError(format!("table column {}: missing title", idx + 1))
                    })?;
                    columns.push(ViewTableColumn {
                        title,
                        editable: row.get("editable").unwrap_or(false),
                    });
                }
            }
            let mut rows = Vec::new();
            if let Ok(rows_tbl) = tbl.get::<_, LuaTable>("rows") {
                for (idx, row) in rows_tbl.sequence_values::<LuaTable>().enumerate() {
                    let row = row?;
                    let cells: Vec<String> = row
                        .get::<_, LuaTable>("cells")
                        .map_err(|_| {
                            LuaError::RuntimeError(format!(
                                "table row {}: missing \"cells\" table",
                                idx + 1
                            ))
                        })?
                        .sequence_values::<String>()
                        .collect::<LuaResult<_>>()?;
                    rows.push(ViewTableRow {
                        id: row.get("id").unwrap_or_default(),
                        cells,
                    });
                }
            }
            Ok(ViewBody::Table { columns, rows })
        }
        "text_view" => Ok(ViewBody::TextView {
            text: tbl.get("text").unwrap_or_default(),
            filetype: tbl
                .get::<_, String>("filetype")
                .ok()
                .filter(|s| !s.is_empty()),
        }),
        other => Err(LuaError::RuntimeError(format!(
            "view: unknown kind {other:?} (expected \"list\", \"tree\", \"table\", or \"text_view\")"
        ))),
    }
}

/// Recursively parse a `nodes` table into [`crate::core::plugin_ui::ViewTreeNode`]s.
fn lua_table_to_tree_nodes(tbl: LuaTable) -> LuaResult<Vec<crate::core::plugin_ui::ViewTreeNode>> {
    use crate::core::plugin_ui::ViewTreeNode;

    let mut nodes = Vec::new();
    for (idx, row) in tbl.sequence_values::<LuaTable>().enumerate() {
        let row = row?;
        let id: String = row
            .get("id")
            .map_err(|_| LuaError::RuntimeError(format!("tree node {}: missing id", idx + 1)))?;
        let children = match row.get::<_, LuaTable>("children") {
            Ok(children_tbl) => lua_table_to_tree_nodes(children_tbl)?,
            Err(_) => Vec::new(),
        };
        nodes.push(ViewTreeNode {
            id,
            label: row.get("label").unwrap_or_default(),
            expanded: row.get("expanded").unwrap_or(false),
            children,
        });
    }
    Ok(nodes)
}

// ─── Extension panel types ──────────────────────────────────────────────────

/// Registration info for an extension-provided sidebar panel.
#[derive(Debug, Clone)]
pub struct PanelRegistration {
    pub name: String,
    pub title: String,
    pub icon: char,
    /// ASCII/Unicode fallback icon for when Nerd Fonts are disabled.
    /// If `None` and nerd fonts are off, the first letter of `title` is used.
    pub fallback_icon: Option<char>,
    pub sections: Vec<String>,
}

impl PanelRegistration {
    /// Return the icon to display, respecting the current thread's
    /// `use_nerd_fonts` flag (see `icons` module docs).
    pub fn resolved_icon(&self) -> char {
        if crate::icons::nerd_fonts_enabled() {
            self.icon
        } else {
            self.fallback_icon
                .unwrap_or_else(|| self.title.chars().next().unwrap_or('?'))
        }
    }
}

/// A single item in an extension panel section.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtPanelItem {
    pub text: String,
    pub hint: String,
    pub icon: String,
    pub indent: u8,
    pub style: ExtPanelStyle,
    pub id: String,
    // ── Tree node fields ──
    /// Whether this item can be expanded/collapsed (shows chevron).
    pub expandable: bool,
    /// Current expand/collapse state (only meaningful when `expandable` is true).
    pub expanded: bool,
    /// Parent item ID (empty = top-level). Children are hidden when parent is collapsed.
    pub parent_id: String,
    // ── Rich layout fields ──
    /// Action buttons rendered as clickable badges on the right side.
    pub actions: Vec<ExtPanelAction>,
    /// Colored badge/tag pills displayed inline with the item text.
    pub badges: Vec<ExtPanelBadge>,
    /// When true, renders as a horizontal divider line instead of text.
    pub is_separator: bool,
}

/// An action button on a panel item (rendered as a clickable badge).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtPanelAction {
    /// Display label (e.g. "Stage", "Discard").
    pub label: String,
    /// Key shortcut that triggers this action when the item is selected.
    pub key: String,
}

/// A colored badge/tag pill displayed on a panel item.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtPanelBadge {
    /// Badge text (e.g. "main", "3 ahead").
    pub text: String,
    /// CSS-style color name or hex (e.g. "green", "#4ec9b0").
    pub color: String,
}

impl Default for ExtPanelItem {
    fn default() -> Self {
        Self {
            text: String::new(),
            hint: String::new(),
            icon: String::new(),
            indent: 0,
            style: ExtPanelStyle::Normal,
            id: String::new(),
            expandable: false,
            expanded: false,
            parent_id: String::new(),
            actions: Vec::new(),
            badges: Vec::new(),
            is_separator: false,
        }
    }
}

/// Visual style for an extension panel item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtPanelStyle {
    #[default]
    Normal,
    Header,
    Dim,
    Accent,
}

// ─── Public types ─────────────────────────────────────────────────────────────

// ─── Plugin-declared UI views (#146) ─────────────────────────────────────────

/// A `vimcode.ui.register_view` registration, harvested at load time.
///
/// The two Lua functions live in the Lua registry (not in a `PluginCallContext`)
/// precisely because they are **stored and invoked later** — the shape #1214's
/// live-engine seam exists to support, and what lets an `on_event` handler use
/// the immediate `vimcode.buffer.*` API.
struct ViewRegistration {
    panel: PanelRegistration,
    render: LuaRegistryKey,
    on_event: Option<LuaRegistryKey>,
}

/// A registered view's stored callbacks, keyed by view name in
/// [`PluginManager::views`].
struct StoredView {
    render: LuaRegistryKey,
    on_event: Option<LuaRegistryKey>,
}

/// Manages all loaded Lua plugins and their registered callbacks.
pub struct PluginManager {
    lua: Lua,
    /// Metadata for every plugin that was discovered (including disabled/errored ones).
    pub plugins: Vec<LoadedPlugin>,
    /// Registered `:Command` → Lua function.
    commands: HashMap<String, LuaRegistryKey>,
    /// Registered `(mode, key)` → Lua function.
    keymaps: HashMap<(String, String), LuaRegistryKey>,
    /// Registered `event` → list of Lua functions.
    hooks: HashMap<String, Vec<LuaRegistryKey>>,
    /// Extension panel registrations harvested from plugin scripts.
    pub panels: HashMap<String, PanelRegistration>,
    /// Extension panel help bindings harvested from plugin scripts.
    pub help_bindings: HashMap<String, Vec<(String, String)>>,
    /// `vimcode.ui.register_view` registrations, by view name (#146).
    views: HashMap<String, StoredView>,
    /// `vimcode.keymap.set` registrations harvested at load time (#1623),
    /// leader-expanded and merged into `Engine::user_keymaps` by
    /// `Engine::set_plugin_manager` (via [`Self::take_raw_lua_keymaps`]) so
    /// they are consulted through the same before-built-ins path as a config
    /// keymap.
    raw_lua_keymaps: Vec<UserKeymap>,
    /// `id -> callback` for every Lua keymap, assigned by
    /// [`Self::register_lua_keymap`] — both the load-time entries above and
    /// ones registered at runtime (from inside a callback, via
    /// `vimcode.keymap.set(..., {buffer = ...})`, where only `&PluginManager`
    /// is available). `RefCell` for that runtime case.
    lua_keymap_callbacks: RefCell<HashMap<u64, LuaKeymapCallback>>,
    /// Next id [`Self::register_lua_keymap`] hands out.
    lua_keymap_next_id: Cell<u64>,
    /// Per-buffer `BufWriteCmd`-style write-override callbacks (#1623),
    /// registered via `vimcode.buffer.set_write_handler` and keyed by the
    /// buffer's handle. `RefCell` for the same reason as
    /// `lua_keymap_callbacks`: registration happens through the immediate
    /// API, with only `&PluginManager` available.
    write_handlers: RefCell<HashMap<i64, LuaRegistryKey>>,
    /// `id -> callback` for every live `vimcode.loop.timer`/`vimcode.schedule`/
    /// `vimcode.defer` registration (#1624). The scheduling bookkeeping
    /// (interval, next-due, repeat) lives on `Engine::plugin_timers`, keyed
    /// by the same id — this map only holds the Lua side (the callback
    /// itself), mirroring `write_handlers`/`lua_keymap_callbacks`.
    timer_callbacks: RefCell<HashMap<u64, LuaRegistryKey>>,
    /// `id -> callbacks` for every live `vimcode.loop.spawn` handle (#1624).
    /// The process/pipes/receiver live on `Engine::plugin_spawns`, keyed by
    /// the same id.
    spawn_callbacks: RefCell<HashMap<u64, SpawnCallbacks>>,
    /// Next id [`Self::register_timer_callback`]/[`Self::register_spawn_callbacks`]/
    /// [`Self::register_picker`]/[`Self::register_picker_item`] hand out.
    /// Shared across all four — they land in disjoint `Engine`-side maps (or,
    /// for picker items, a disjoint per-picker map), so there is no collision
    /// risk, and one counter is simpler than several.
    next_handle_id: Cell<u64>,
    /// Live `vimcode.picker.open` registrations (#1630), keyed by the id
    /// [`Self::register_picker`] handed out. `RefCell` for the same reason as
    /// `spawn_callbacks`: `set_items`/`append`/`close` reach here through the
    /// immediate API, with only `&PluginManager` available.
    ///
    /// Per-item `LuaRegistryKey`s (`PluginPickerItemEntry::data`) are dropped
    /// via plain `HashMap::remove`/`clear` here, not an explicit
    /// `lua.remove_registry_value`/`Lua::expire_registry_values()` call —
    /// same pattern as `timer_callbacks`/`spawn_callbacks` above, so not a
    /// regression, but pickers are the first surface where per-item counts
    /// can run into the thousands (e.g. live-grep results), which will make
    /// any latent registry-growth cost from that pattern far more visible
    /// than it was for a handful of timer/spawn callbacks (#1630 review).
    pickers: RefCell<HashMap<u64, PluginPicker>>,
    /// `id -> callback` for every in-flight `vimcode.http.request` handle
    /// (#1632). The child process/receiver live on `Engine::
    /// plugin_http_requests`, keyed by the same id — mirrors `spawn_callbacks`
    /// exactly, minus the "three callbacks" shape (HTTP has exactly one).
    http_callbacks: RefCell<HashMap<u64, LuaRegistryKey>>,
}

/// A `vimcode.loop.spawn` handle's registered callbacks (#1624). Any of the
/// three may be absent — a plugin that only cares about the exit status need
/// not pass `on_stdout`/`on_stderr`.
struct SpawnCallbacks {
    on_stdout: Option<LuaRegistryKey>,
    on_stderr: Option<LuaRegistryKey>,
    on_exit: Option<LuaRegistryKey>,
}

/// A live `vimcode.picker.open` registration (#1630): the three optional
/// callbacks plus every item currently in the list, keyed by an id private to
/// this picker (assigned by [`PluginManager::register_picker_item`]) so
/// `PickerAction::Custom("plugin_item:<picker_id>:<item_id>")` can look the
/// item back up when the user confirms it.
struct PluginPicker {
    on_select: Option<LuaRegistryKey>,
    on_cancel: Option<LuaRegistryKey>,
    on_query: Option<LuaRegistryKey>,
    items: HashMap<u64, PluginPickerItemEntry>,
}

/// One item's plugin-owned payload: the opaque `data` value handed back to
/// `on_select` verbatim, and where its preview content comes from (if any).
struct PluginPickerItemEntry {
    data: Option<LuaRegistryKey>,
    preview: Option<PluginPickerPreview>,
}

/// Where a `vimcode.picker.open` item's preview content comes from (#1630).
/// Plain data (no Lua registry involved), so `Engine::picker_load_preview`
/// can read it without calling back into Lua.
#[derive(Debug, Clone)]
pub(crate) enum PluginPickerPreview {
    /// A file path (resolved against `Engine::cwd` if relative) and an
    /// optional 1-indexed line to center on, mirroring `PickerAction::
    /// OpenFileAtLine`'s convention.
    File(PathBuf, Option<usize>),
    /// A buffer handle (as given to Lua — `0` means "current", matching
    /// every other immediate-API handle) and an optional 1-indexed line.
    Buffer(i64, Option<usize>),
}

/// One `vimcode.picker.open`/`:set_items`/`:append` item, parsed from Lua but
/// not yet registered with a picker (that needs `&PluginManager`, done by
/// `Engine::plugin_api_picker_set_items`).
pub(crate) struct PluginPickerItemSpec {
    pub display: String,
    pub filter_text: Option<String>,
    pub detail: Option<String>,
    /// Parsed from the item table but currently a no-op: `PickerItem::icon`
    /// has no rendering path in the unified picker at all yet (true for
    /// every other `PickerItem` producer too, not something new here) — see
    /// `render.rs`'s `PickerPanelItem`. Kept on the documented item shape so
    /// a future patch can wire it in without an API break; a plugin author
    /// who sets it today will just see it silently do nothing (#1630 review).
    pub icon: Option<String>,
    pub(crate) data: Option<LuaRegistryKey>,
    pub(crate) preview: Option<PluginPickerPreview>,
}

/// A registered `vimcode.keymap.set` callback (#1623), looked up by the
/// opaque id a [`UserKeymapAction::Lua`] carries.
struct LuaKeymapCallback {
    callback: LuaRegistryKey,
    /// `opts.expr == true`: the callback's return value (a key-notation
    /// string) is fed back through the normal key path instead of the
    /// callback being expected to act directly.
    expr: bool,
}

/// A `vimcode.keymap.set` call harvested during load-time script execution,
/// before an id has been assigned (that needs `&mut PluginManager`, not
/// available from inside the Lua closure that runs during `exec()`).
struct PendingLuaKeymap {
    mode: String,
    /// Tokenized lhs, with a literal `"<leader>"` marker left unexpanded
    /// (see [`PluginManager::raw_lua_keymaps`]'s doc comment).
    keys: Vec<String>,
    callback: LuaRegistryKey,
    expr: bool,
    desc: Option<String>,
}

/// Metadata about a single plugin file / directory.
pub struct LoadedPlugin {
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    /// False when the plugin was skipped because it appears in `disabled_plugins`.
    pub enabled: bool,
    /// Non-None when the plugin produced an error at load time.
    pub error: Option<String>,
}

/// Data passed into (and modified by) Lua callbacks during a plugin call.
///
/// `cwd`, `buf_path`, `buf_rope` are inputs; the output fields start empty.
///
/// `buf_rope` holds a cheap clone of the active buffer's `Rope` (the ropey
/// `Rope` is reference-counted, so cloning is O(1)). Lua callers that ask
/// for line contents via `vimcode.buf.lines()` / `buf.line(n)` /
/// `buf.get_lines(s, e)` pay the `String` allocation only for the range
/// they actually read. A previous design eagerly materialised every line
/// into `buf_lines: Vec<String>` on every plugin event — on a 190k-line
/// buffer that was a 100 MB allocation per event, which pegged CPU at
/// 100 % when any plugin chained `async_shell` calls in its callback
/// (each completion fired a plugin event → new ctx → new allocation).
/// See issue #153.
#[derive(Default)]
pub struct PluginCallContext {
    // ── Inputs ──────────────────────────────────────────────────────────────
    pub cwd: String,
    pub buf_path: Option<String>,
    pub buf_rope: Option<ropey::Rope>,
    /// Legacy eager-lines cache. Kept for back-compat with callers that
    /// populate it manually (e.g. tests); new code should leave it empty
    /// and read via `buf_rope`. The Lua API accessors fall back to this
    /// only when `buf_rope` is `None`.
    pub buf_lines: Vec<String>,
    /// Cursor line (1-indexed) in the active buffer.
    pub cursor_line: usize,
    /// Cursor column (1-indexed) in the active buffer.
    pub cursor_col: usize,
    /// Filesystem path for the current working directory (for git operations).
    pub cwd_path: Option<PathBuf>,
    /// Filesystem path for the active buffer (for git operations).
    pub buf_path_os: Option<PathBuf>,
    /// Whether the active buffer has unsaved changes.
    /// When false, `vimcode.git.blame_line()` skips piping the in-memory
    /// contents via `--contents -` and uses the faster committed-content path.
    pub buf_dirty: bool,
    /// Current mode name (e.g. "Normal", "Insert", "Visual").
    pub mode_name: String,
    /// Snapshot of registers: `char -> (content, is_linewise)`.
    pub registers_snapshot: HashMap<char, (String, bool)>,
    /// Snapshot of marks for the active buffer: `char -> (line, col)` (1-indexed).
    pub marks_snapshot: HashMap<char, (usize, usize)>,
    /// Filetype / language ID of the active buffer (e.g. "rust", "python").
    pub filetype: String,
    /// Snapshot of all settings as key-value string pairs.
    pub settings_snapshot: HashMap<String, String>,
    /// Snapshot of panel input field texts: panel_name → current text.
    pub panel_input_snapshot: HashMap<String, String>,
    // ── Outputs written by callbacks ────────────────────────────────────────
    pub message: Option<String>,
    /// `(0-based line index, new text)` — applied by the engine after the call.
    pub set_lines: Vec<(usize, String)>,
    /// VimCode commands to execute after the call (e.g. `"w"` to save).
    pub run_commands: Vec<String>,
    /// Inline annotations to set: `(1-indexed line, annotation text)`.
    pub annotate_lines: Vec<(usize, String)>,
    /// When true, all existing line annotations are cleared first.
    pub clear_annotations: bool,
    /// Requests to run shell commands in background threads.
    pub async_shell_requests: Vec<AsyncShellRequest>,
    /// Set cursor position: `(line, col)` (1-indexed). Applied with bounds clamping.
    pub set_cursor: Option<(usize, usize)>,
    /// Settings to apply: `(key, value)` pairs processed via `Settings::set_value_str()`.
    pub set_settings: Vec<(String, String)>,
    /// Lines to insert: `(1-indexed line, text)`. Inserted before the given line.
    pub insert_lines: Vec<(usize, String)>,
    /// Lines to delete: 1-indexed line numbers (processed in reverse order).
    pub delete_lines: Vec<usize>,
    /// Registers to set: `(char, content, is_linewise)`.
    pub set_registers: Vec<(char, String, bool)>,
    /// Comment style overrides: `(lang_id, line, block_open, block_close)`.
    pub comment_style_overrides: Vec<(String, String, String, String)>,
    /// Scratch buffers to open after the callback returns.
    pub scratch_buffers: Vec<ScratchBufferRequest>,
    /// Extension panel registrations collected during script init.
    pub panel_registrations: Vec<PanelRegistration>,
    /// Extension panel item updates: `(panel_name, section_name, items)`.
    pub panel_set_items: Vec<(String, String, Vec<ExtPanelItem>)>,
    /// Panel hover content registrations: `(panel_name, item_id, markdown)`.
    pub panel_hover_entries: Vec<(String, String, String)>,
    /// Panel help bindings: `(panel_name, [(key, description)])`.
    pub panel_help_entries: Vec<(String, Vec<(String, String)>)>,
    /// Panel input field text values to set: `(panel_name, text)`.
    pub panel_input_values: Vec<(String, String)>,
    /// `vimcode.ui.refresh(name)` requests — view names whose `render` callback
    /// should run again after this call returns (#146).
    pub plugin_view_refresh: Vec<String>,
    /// Editor hover content registrations: `(0-indexed line, markdown)`.
    pub editor_hover_entries: Vec<(usize, String)>,
    /// Panel reveal request: `(panel_name, section_name, item_id)`.
    /// Causes the sidebar to switch to the named panel and highlight the item.
    pub panel_reveal_request: Option<(String, String, String)>,
    /// Commit file diff request: `(hash, rel_path)`.
    /// Opens a side-by-side diff of the file at `hash` vs `hash~1`.
    pub commit_file_diff: Option<(String, String)>,
    /// URLs to open in the default browser.
    pub open_urls: Vec<String>,
    /// Key sequences to feed into the engine (parsed like `send_keys`).
    pub feedkeys_sequences: Vec<String>,
    /// Range-based line replacements: `(start_0idx, end_0idx, replacement_lines)`.
    /// Replaces lines `[start, end)` with the given lines (Neovim-compatible).
    pub set_lines_range: Vec<(usize, usize, Vec<String>)>,
}

// ─── Internal registration accumulator ───────────────────────────────────────

/// Stored in Lua app_data during a plugin's top-level execution so that
/// `vimcode.command()`, `vimcode.on()`, `vimcode.keymap()` calls can
/// accumulate registrations that the engine harvests afterwards.
#[derive(Default)]
struct PluginRegistrations {
    commands: HashMap<String, LuaRegistryKey>,
    keymaps: HashMap<(String, String), LuaRegistryKey>,
    hooks: HashMap<String, Vec<LuaRegistryKey>>,
    panels: Vec<PanelRegistration>,
    help_bindings: Vec<(String, Vec<(String, String)>)>,
    views: Vec<ViewRegistration>,
    lua_keymaps: Vec<PendingLuaKeymap>,
}

// ─── Lua → picker vocabulary (#1630) ─────────────────────────────────────────

/// Parse a `vimcode.picker.open`/`:set_items`/`:append` item's optional
/// `preview` table: `{ file = "path", line = 3 }` or `{ buffer = 0, line = 3 }`
/// (`file` wins if both are somehow present). `line` is 1-indexed, matching
/// every other line convention this API surface uses (module doc).
fn lua_table_to_picker_preview(t: &LuaTable) -> Option<PluginPickerPreview> {
    let line: Option<usize> = t.get::<_, i64>("line").ok().map(|n| n.max(1) as usize);
    if let Ok(file) = t.get::<_, String>("file") {
        if !file.is_empty() {
            return Some(PluginPickerPreview::File(PathBuf::from(file), line));
        }
    }
    if let Ok(buf) = t.get::<_, i64>("buffer") {
        return Some(PluginPickerPreview::Buffer(buf, line));
    }
    None
}

/// Parse one item table into a [`PluginPickerItemSpec`]. `data` is stashed in
/// the Lua registry as-is (any value, including `nil`/absent) so `on_select`
/// gets back exactly what the plugin put in, unmodified.
fn lua_table_to_picker_item_spec(lua: &Lua, t: &LuaTable) -> LuaResult<PluginPickerItemSpec> {
    let display: String = t.get("display").unwrap_or_default();
    let filter_text: Option<String> = t.get::<_, String>("filter_text").ok();
    let detail: Option<String> = t.get::<_, String>("detail").ok();
    let icon: Option<String> = t.get::<_, String>("icon").ok();
    let data = match t.get::<_, LuaValue>("data") {
        Ok(LuaValue::Nil) | Err(_) => None,
        Ok(v) => Some(lua.create_registry_value(v)?),
    };
    let preview = match t.get::<_, LuaTable>("preview") {
        Ok(pt) => lua_table_to_picker_preview(&pt),
        Err(_) => None,
    };
    Ok(PluginPickerItemSpec {
        display,
        filter_text,
        detail,
        icon,
        data,
        preview,
    })
}

/// Parse a whole `items` array-table into specs, in order.
fn lua_table_to_picker_items(lua: &Lua, items: &LuaTable) -> LuaResult<Vec<PluginPickerItemSpec>> {
    items
        .clone()
        .sequence_values::<LuaTable>()
        .map(|row| lua_table_to_picker_item_spec(lua, &row?))
        .collect()
}

/// Build the handle `vimcode.picker.open` returns: `id` plus the four
/// live-update methods, every one an immediate-API call keyed by `id` —
/// mirrors `vimcode.loop.spawn`'s handle (`write`/`close_stdin`/`kill`).
fn make_picker_handle(lua: &Lua, id: u64) -> LuaResult<LuaTable<'_>> {
    let handle = lua.create_table()?;
    handle.set("id", id)?;
    handle.set(
        "set_items",
        lua.create_function(move |lua, (_self, items): (LuaValue, LuaTable)| {
            let specs = lua_table_to_picker_items(lua, &items)?;
            live_engine("vimcode.picker:set_items", move |e| {
                e.plugin_api_picker_set_items(id, specs, true)
            })
        })?,
    )?;
    handle.set(
        "append",
        lua.create_function(move |lua, (_self, items): (LuaValue, LuaTable)| {
            let specs = lua_table_to_picker_items(lua, &items)?;
            live_engine("vimcode.picker:append", move |e| {
                e.plugin_api_picker_set_items(id, specs, false)
            })
        })?,
    )?;
    handle.set(
        "set_loading",
        lua.create_function(move |_, (_self, loading): (LuaValue, bool)| {
            live_engine("vimcode.picker:set_loading", move |e| {
                e.plugin_api_picker_set_loading(id, loading)
            })
        })?,
    )?;
    handle.set(
        "close",
        lua.create_function(move |_, _self: LuaValue| {
            live_engine("vimcode.picker:close", move |e| {
                e.plugin_api_picker_close(id)
            })
        })?,
    )?;
    Ok(handle)
}

// ─── `vimcode.json` (#1632) ───────────────────────────────────────────────────
//
// Hand-rolled `LuaValue <-> serde_json::Value` conversion rather than mlua's
// optional `serialize` feature (as `lua_table_to_view` above already does for
// the same reason): the errors are the ones a plugin author needs, the empty-
// table array/object ambiguity below needs an explicit decision this repo
// controls, and the parse can't be perturbed by an `mlua` feature flag.
//
// Lua has one table type for both JSON arrays and objects, so an *empty*
// table is genuinely ambiguous — `vimcode.json.encode({})` defaults to `"[]"`
// (Lua's `{}` is idiomatically "empty list" far more often than "empty
// object" in practice), and `vimcode.json.empty_object` is a sentinel value a
// plugin can pass instead (in place of, or as a table field's value) to force
// `"{}"`. `vimcode.json.null` is the parallel sentinel for a JSON `null`
// living *inside* a table — a bare Lua `nil` can't be stored as a table value
// at all (it deletes the key), so a real `nil`/`null` distinction needs its
// own marker, the same problem `vim.NIL` solves in Neovim. Both sentinels are
// plain empty tables created once per `Lua` and compared by reference
// (`Table`'s `PartialEq` is `rawequal`) — nothing else in this Lua state can
// construct a table `==` to them.

const JSON_NULL_REGISTRY_KEY: &str = "vimcode_json_null_sentinel";
const JSON_EMPTY_OBJECT_REGISTRY_KEY: &str = "vimcode_json_empty_object_sentinel";

/// Convert a Lua value into a [`serde_json::Value`], honouring the `null`/
/// `empty_object` sentinels described above.
fn lua_value_to_json(lua: &Lua, v: LuaValue) -> LuaResult<serde_json::Value> {
    if let LuaValue::Table(ref t) = v {
        if let Ok(null_sentinel) = lua.named_registry_value::<LuaTable>(JSON_NULL_REGISTRY_KEY) {
            if *t == null_sentinel {
                return Ok(serde_json::Value::Null);
            }
        }
        if let Ok(empty_obj) = lua.named_registry_value::<LuaTable>(JSON_EMPTY_OBJECT_REGISTRY_KEY)
        {
            if *t == empty_obj {
                return Ok(serde_json::Value::Object(serde_json::Map::new()));
            }
        }
    }
    match v {
        LuaValue::Nil => Ok(serde_json::Value::Null),
        LuaValue::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        LuaValue::Integer(i) => Ok(serde_json::Value::Number(i.into())),
        LuaValue::Number(n) => {
            if !n.is_finite() {
                return Err(LuaError::RuntimeError(
                    "vimcode.json.encode: cannot encode NaN/Infinity".to_string(),
                ));
            }
            Ok(serde_json::Number::from_f64(n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null))
        }
        LuaValue::String(s) => Ok(serde_json::Value::String(s.to_str()?.to_string())),
        LuaValue::Table(t) => json_encode_table(lua, &t),
        other => Err(LuaError::RuntimeError(format!(
            "vimcode.json.encode: cannot encode a Lua {}",
            other.type_name()
        ))),
    }
}

/// Encode a plain (non-sentinel) Lua table: a 1..n contiguous integer-keyed
/// table becomes a JSON array, anything else (including an empty table —
/// see the module doc above) becomes a JSON object.
fn json_encode_table(lua: &Lua, t: &LuaTable) -> LuaResult<serde_json::Value> {
    let len = t.raw_len();
    if len > 0 {
        let mut is_array = true;
        for pair in t.clone().pairs::<LuaValue, LuaValue>() {
            let (k, _) = pair?;
            let in_range = match k {
                LuaValue::Integer(i) => i >= 1 && (i as usize) <= len,
                LuaValue::Number(n) => n.fract() == 0.0 && n >= 1.0 && (n as usize) <= len,
                _ => false,
            };
            if !in_range {
                is_array = false;
                break;
            }
        }
        if is_array {
            let mut items = Vec::with_capacity(len);
            for i in 1..=len {
                let v: LuaValue = t.get(i)?;
                items.push(lua_value_to_json(lua, v)?);
            }
            return Ok(serde_json::Value::Array(items));
        }
    }
    if len == 0 && t.clone().pairs::<LuaValue, LuaValue>().next().is_none() {
        // Ambiguous empty table, no explicit sentinel — see module doc.
        return Ok(serde_json::Value::Array(Vec::new()));
    }
    let mut map = serde_json::Map::new();
    for pair in t.clone().pairs::<LuaValue, LuaValue>() {
        let (k, v) = pair?;
        let key = match k {
            LuaValue::String(s) => s.to_str()?.to_string(),
            LuaValue::Integer(i) => i.to_string(),
            LuaValue::Number(n) => n.to_string(),
            other => {
                return Err(LuaError::RuntimeError(format!(
                    "vimcode.json.encode: unsupported table key type {}",
                    other.type_name()
                )))
            }
        };
        map.insert(key, lua_value_to_json(lua, v)?);
    }
    Ok(serde_json::Value::Object(map))
}

/// Convert a [`serde_json::Value`] into a Lua value, the inverse of
/// [`lua_value_to_json`] (including the `null`/`empty_object` sentinels, so
/// `encode(decode(s))` round-trips `"{}"` and `"null"` back to themselves).
fn json_to_lua_value<'lua>(lua: &'lua Lua, v: &serde_json::Value) -> LuaResult<LuaValue<'lua>> {
    match v {
        serde_json::Value::Null => Ok(LuaValue::Table(
            lua.named_registry_value::<LuaTable>(JSON_NULL_REGISTRY_KEY)?,
        )),
        serde_json::Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(LuaValue::Integer(i))
            } else {
                Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0)))
            }
        }
        serde_json::Value::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
        serde_json::Value::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.set(i + 1, json_to_lua_value(lua, item)?)?;
            }
            Ok(LuaValue::Table(t))
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return Ok(LuaValue::Table(lua.named_registry_value::<LuaTable>(
                    JSON_EMPTY_OBJECT_REGISTRY_KEY,
                )?));
            }
            let t = lua.create_table()?;
            for (k, val) in map {
                t.set(k.clone(), json_to_lua_value(lua, val)?)?;
            }
            Ok(LuaValue::Table(t))
        }
    }
}

// ─── Plugin-scoped storage (#1632) ────────────────────────────────────────────
//
// One JSON file per (plugin, scope): `<vimcode data dir>/plugin_storage/
// <plugin>/store.json` for the global scope, `.../<plugin>/workspaces/
// <hash of cwd>/store.json` for the per-workspace scope (`{workspace = true}`
// in `get`/`set`/`delete`/`keys`'s optional `opts`). Values round-trip through
// the exact same `lua_value_to_json`/`json_to_lua_value` `vimcode.json` uses,
// so "values are JSON-serialisable" is enforced for free, with the same
// errors. Writes go through a temp-file-then-rename so a crash mid-write
// can't leave a torn/partial file — `std::fs::rename` replaces the
// destination atomically on every platform vimcode ships for (Windows'
// `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` behind `std::fs::rename` included).
//
// Namespacing (`plugins can't read each other's data`) is resolved from the
// Lua *call stack*, not a separately-tracked "current plugin" field: see
// [`current_plugin_chunk_name`]'s doc for why that works even from inside a
// callback fired long after load time, with no changes needed to any of
// `PluginManager`'s existing registration paths.

/// Identify which plugin's Lua chunk is calling right now by walking the
/// interpreter's call stack (`Lua::inspect_stack`) up from the native
/// `vimcode.storage.*` function currently executing (level 0) until a Lua
/// (or "main" chunk) frame is found. That frame's `source` is exactly the
/// chunk name `PluginManager::load_one_plugin` set via `.set_name(name)` —
/// the plugin's own file/directory stem — because a Lua closure's `source`
/// is fixed at the point it was *defined*, not where/when it is *called*.
///
/// This is what makes storage calls "just work" identically whether they
/// happen at top-level load time (no `PluginCallContext`/live engine exists
/// yet) or from inside a command/timer/spawn/http callback fired much later:
/// no separate bookkeeping is needed across any of `PluginManager`'s existing
/// registration kinds, because the answer is derived fresh from wherever the
/// call is actually coming from.
fn current_plugin_chunk_name(lua: &Lua) -> Option<String> {
    for level in 1..16 {
        let dbg = lua.inspect_stack(level)?;
        let src = dbg.source();
        if src.what == "Lua" || src.what == "main" {
            return src.source.map(|s| s.into_owned());
        }
    }
    None
}

fn storage_no_plugin_err() -> LuaError {
    LuaError::RuntimeError("vimcode.storage: must be called from plugin code".to_string())
}

fn storage_opt_workspace(opts: &Option<LuaTable>) -> bool {
    opts.as_ref()
        .and_then(|o| o.get::<_, bool>("workspace").ok())
        .unwrap_or(false)
}

/// Turn an arbitrary plugin name into a safe single path component —
/// defensive only (plugin names already come from `file_stem()`/`file_name()`
/// in `load_plugins_dir`, so they can't contain a path separator in
/// practice), but cheap enough to apply unconditionally rather than trust it.
fn sanitize_storage_component(name: &str) -> String {
    if name.is_empty() {
        return "_".to_string();
    }
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A short, filesystem-safe key derived from the current working directory,
/// used to shard `{workspace = true}` storage by workspace. `std::env::
/// current_dir()` (not `Engine::cwd`) so this resolves identically whether
/// called at plugin-load time (no live engine loaned yet) or from any later
/// callback — vimcode's own `:cd` already calls `std::env::set_current_dir`
/// (`Engine::buffers.rs`), so the two never disagree in practice.
fn workspace_storage_key() -> String {
    use sha2::{Digest, Sha256};
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let digest = Sha256::digest(cwd.to_string_lossy().as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

fn plugin_storage_path(plugin: &str, workspace: bool) -> PathBuf {
    let base = crate::core::paths::vimcode_data_dir()
        .join("plugin_storage")
        .join(sanitize_storage_component(plugin));
    if workspace {
        base.join("workspaces")
            .join(workspace_storage_key())
            .join("store.json")
    } else {
        base.join("store.json")
    }
}

/// Read `path`'s JSON object into a map. Missing file, unreadable file, or a
/// file that isn't a JSON object (corrupt/foreign content) all fall back to
/// an empty map rather than erroring — `get`/`keys` degrade to "nothing
/// stored yet", and a subsequent `set` self-heals the file on its next write.
fn load_storage_map(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| match v {
            serde_json::Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Write `map` to `path` atomically: serialize to a sibling `.tmp` file, then
/// `rename` it over `path`. The rename is what makes this atomic — a reader
/// (or a crash) never observes a partially-written file.
fn save_storage_map(
    path: &Path,
    map: &serde_json::Map<String, serde_json::Value>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_vec(map).map_err(std::io::Error::other)?;
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ─── PluginManager implementation ────────────────────────────────────────────

impl PluginManager {
    /// Create a new `PluginManager` and set up the `vimcode.*` Lua API.
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        Self::setup_vimcode_api(&lua)?;
        Ok(Self {
            lua,
            plugins: Vec::new(),
            commands: HashMap::new(),
            keymaps: HashMap::new(),
            hooks: HashMap::new(),
            panels: HashMap::new(),
            help_bindings: HashMap::new(),
            views: HashMap::new(),
            raw_lua_keymaps: Vec::new(),
            lua_keymap_callbacks: RefCell::new(HashMap::new()),
            lua_keymap_next_id: Cell::new(0),
            write_handlers: RefCell::new(HashMap::new()),
            timer_callbacks: RefCell::new(HashMap::new()),
            spawn_callbacks: RefCell::new(HashMap::new()),
            next_handle_id: Cell::new(0),
            pickers: RefCell::new(HashMap::new()),
            http_callbacks: RefCell::new(HashMap::new()),
        })
    }

    /// Scan `dir` for `.lua` files and `*/init.lua` directories, load each one.
    /// Files whose stem appears in `disabled` are recorded but not executed.
    pub fn load_plugins_dir(&mut self, dir: &Path, disabled: &[String]) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();

        for path in paths {
            if path.extension().map(|e| e == "lua").unwrap_or(false) {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let enabled = !disabled.iter().any(|d| d == &name);
                self.load_one_plugin(&path, &name, enabled);
            } else if path.is_dir() {
                let init = path.join("init.lua");
                if init.exists() {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let enabled = !disabled.iter().any(|d| d == &name);
                    self.load_one_plugin(&init, &name, enabled);
                }
            }
        }
    }

    /// Execute a single plugin file and harvest its registrations.
    fn load_one_plugin(&mut self, path: &Path, name: &str, enabled: bool) {
        if !enabled {
            self.plugins.push(LoadedPlugin {
                name: name.to_string(),
                path: path.to_path_buf(),
                enabled: false,
                error: None,
            });
            return;
        }

        let code = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                self.plugins.push(LoadedPlugin {
                    name: name.to_string(),
                    path: path.to_path_buf(),
                    enabled: true,
                    error: Some(format!("read error: {e}")),
                });
                return;
            }
        };

        // Install registration accumulator so vimcode.command/on/keymap can write to it.
        self.lua.set_app_data(PluginRegistrations::default());

        let result = self.lua.load(&code).set_name(name).exec();

        // Harvest whatever was registered before any error.
        if let Some(reg) = self.lua.remove_app_data::<PluginRegistrations>() {
            for (cmd_name, key) in reg.commands {
                self.commands.insert(cmd_name, key);
            }
            for (km, key) in reg.keymaps {
                self.keymaps.insert(km, key);
            }
            for (event, keys) in reg.hooks {
                self.hooks.entry(event).or_default().extend(keys);
            }
            for panel in reg.panels {
                self.panels.insert(panel.name.clone(), panel);
            }
            for (panel_name, bindings) in reg.help_bindings {
                self.help_bindings.insert(panel_name, bindings);
            }
            // #146: a view-backed panel is registered in `panels` like any other
            // (so the activity bar and `Engine::ext_panels` need no new path)
            // *and* in `views`, whose presence is what makes the sidebar paint a
            // `quadraui::Form` instead of tree rows.
            for view in reg.views {
                let name = view.panel.name.clone();
                self.panels.insert(name.clone(), view.panel);
                self.views.insert(
                    name,
                    StoredView {
                        render: view.render,
                        on_event: view.on_event,
                    },
                );
            }
            // #1623: `vimcode.keymap.set` calls made at load time. Assign
            // each an id now (needs `&mut self`, unavailable from inside the
            // Lua closure that ran during `exec()` above) and stash the
            // resulting `UserKeymap` for `Engine::set_plugin_manager` to
            // leader-expand and merge into `user_keymaps`.
            for pending in reg.lua_keymaps {
                let id = self.register_lua_keymap(pending.callback, pending.expr);
                self.raw_lua_keymaps.push(UserKeymap {
                    mode: pending.mode,
                    noremap: true,
                    keys: pending.keys,
                    action: UserKeymapAction::Lua(id),
                    buffer: None,
                    desc: pending.desc,
                });
            }
        }

        let error = result.err().map(|e| e.to_string());
        self.plugins.push(LoadedPlugin {
            name: name.to_string(),
            path: path.to_path_buf(),
            enabled: true,
            error,
        });
    }

    // ─── Dispatch helpers ──────────────────────────────────────────────────

    /// Execute a registered `:Command`. Returns `(found, updated_context)`.
    pub fn call_command(
        &self,
        name: &str,
        args: &str,
        ctx: PluginCallContext,
    ) -> (bool, PluginCallContext) {
        let Some(key) = self.commands.get(name) else {
            return (false, ctx);
        };
        self.lua.set_app_data(ctx);
        if let Ok(f) = self.lua.registry_value::<LuaFunction>(key) {
            let _ = f.call::<String, ()>(args.to_string());
        }
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (true, ctx)
    }

    /// Return true if at least one hook is registered for `event`.
    /// Use this to skip expensive context construction when no hooks exist.
    pub fn has_event_hooks(&self, event: &str) -> bool {
        self.hooks
            .get(event)
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    }

    /// Fire all hooks registered for `event`. Returns the updated context.
    pub fn call_event(&self, event: &str, arg: &str, ctx: PluginCallContext) -> PluginCallContext {
        let Some(hooks) = self.hooks.get(event) else {
            return ctx;
        };
        if hooks.is_empty() {
            return ctx;
        }
        self.lua.set_app_data(ctx);
        for key in hooks {
            if let Ok(f) = self.lua.registry_value::<LuaFunction>(key) {
                let _ = f.call::<String, ()>(arg.to_string());
            }
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// Check if an operatorfunc has been registered via `vimcode.set_operatorfunc()`.
    #[allow(dead_code)]
    pub fn has_operatorfunc(&self) -> bool {
        self.lua
            .named_registry_value::<LuaRegistryKey>("__operatorfunc")
            .is_ok()
    }

    /// Call the registered operatorfunc with the given motion type ("line", "char", "block").
    /// Returns the updated context with any modifications made by the callback.
    pub fn call_operatorfunc(
        &self,
        motion_type: &str,
        ctx: PluginCallContext,
    ) -> (bool, PluginCallContext) {
        let key = match self
            .lua
            .named_registry_value::<LuaRegistryKey>("__operatorfunc")
        {
            Ok(k) => k,
            Err(_) => return (false, ctx),
        };
        self.lua.set_app_data(ctx);
        if let Ok(f) = self.lua.registry_value::<LuaFunction>(&key) {
            let _ = f.call::<String, ()>(motion_type.to_string());
        }
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (true, ctx)
    }

    /// Check if any registered keymap for `mode` starts with `prefix`.
    pub fn has_keymap_prefix(&self, mode: &str, prefix: &str) -> bool {
        self.keymaps
            .keys()
            .any(|(m, k)| m == mode && k.starts_with(prefix))
    }

    /// Execute a registered keymap for `(mode, key)`. Returns `(found, updated_context)`.
    pub fn call_keymap(
        &self,
        mode: &str,
        key: &str,
        ctx: PluginCallContext,
    ) -> (bool, PluginCallContext) {
        let Some(reg_key) = self.keymaps.get(&(mode.to_string(), key.to_string())) else {
            return (false, ctx);
        };
        self.lua.set_app_data(ctx);
        if let Ok(f) = self.lua.registry_value::<LuaFunction>(reg_key) {
            let _ = f.call::<(), ()>(());
        }
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (true, ctx)
    }

    // ─── `vimcode.keymap.set` keymaps (#1623) ──────────────────────────────

    /// Take every load-time `vimcode.keymap.set` registration, for
    /// `Engine::set_plugin_manager` to leader-expand and merge into
    /// `user_keymaps`. Leaves `raw_lua_keymaps` empty — called exactly once,
    /// right after `PluginManager::new`'s plugin-loading is done.
    pub(crate) fn take_raw_lua_keymaps(&mut self) -> Vec<UserKeymap> {
        std::mem::take(&mut self.raw_lua_keymaps)
    }

    /// Register a Lua keymap callback and return the id a
    /// [`UserKeymapAction::Lua`] carries. Called both at load-time harvest
    /// (`&mut self`, coerces fine) and at runtime from the immediate API
    /// (only `&PluginManager` available, hence the `RefCell`).
    fn register_lua_keymap(&self, callback: LuaRegistryKey, expr: bool) -> u64 {
        let id = self.lua_keymap_next_id.get();
        self.lua_keymap_next_id.set(id + 1);
        self.lua_keymap_callbacks
            .borrow_mut()
            .insert(id, LuaKeymapCallback { callback, expr });
        id
    }

    /// Fire the Lua keymap `id` names. Returns `(expr_result, updated_ctx)`:
    /// `expr_result` is `Some(keys)` only for an `expr` map whose callback
    /// returned a string — the caller (`Engine::dispatch_lua_keymap`) feeds
    /// it back through the normal key path, same as a `UserKeymapAction::Keys`
    /// rhs.
    pub fn call_lua_keymap(
        &self,
        id: u64,
        ctx: PluginCallContext,
    ) -> (Option<String>, PluginCallContext) {
        self.lua.set_app_data(ctx);
        // Look the function up and drop the borrow before calling it — the
        // callback might itself call `vimcode.keymap.set` (a fresh
        // registration), which needs to `borrow_mut()` the same `RefCell`.
        let (f, expr) = {
            let callbacks = self.lua_keymap_callbacks.borrow();
            match callbacks.get(&id) {
                Some(entry) => (
                    self.lua.registry_value::<LuaFunction>(&entry.callback).ok(),
                    entry.expr,
                ),
                None => (None, false),
            }
        };
        let result = match f {
            Some(f) if expr => f.call::<(), Option<String>>(()).unwrap_or(None),
            Some(f) => {
                let _ = f.call::<(), ()>(());
                None
            }
            None => None,
        };
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (result, ctx)
    }

    /// List every active keymap — config-defined and Lua-registered alike —
    /// as `(mode, lhs, buffer_handle, desc)`, for `vimcode.keymap.list()`
    /// (#1623), the enumeration a which-key style extension renders from.
    /// `buffer_handle` is `None` for a global mapping.
    pub(crate) fn list_keymaps_from(
        keymaps: &[UserKeymap],
    ) -> Vec<(String, String, Option<i64>, Option<String>)> {
        keymaps
            .iter()
            .map(|km| {
                (
                    km.mode.clone(),
                    km.keys.join(""),
                    km.buffer.map(|b| b.0 as i64),
                    km.desc.clone(),
                )
            })
            .collect()
    }

    // ─── `vimcode.buffer.set_write_handler` (#1623) ────────────────────────

    /// Register `callback` as the write-override for buffer `buf` (a handle,
    /// already resolved — never `0`/"current").
    pub(crate) fn set_write_handler(&self, buf: i64, callback: LuaRegistryKey) {
        self.write_handlers.borrow_mut().insert(buf, callback);
    }

    /// Whether `buf` has a registered write-override.
    pub fn has_write_handler(&self, buf: i64) -> bool {
        self.write_handlers.borrow().contains_key(&buf)
    }

    /// Fire `buf`'s write-override callback with the buffer handle as its
    /// sole argument. Returns `(found, updated_ctx)`.
    pub fn call_write_handler(
        &self,
        buf: i64,
        ctx: PluginCallContext,
    ) -> (bool, PluginCallContext) {
        self.lua.set_app_data(ctx);
        let f = {
            let handlers = self.write_handlers.borrow();
            handlers
                .get(&buf)
                .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok())
        };
        let found = f.is_some();
        if let Some(f) = f {
            let _ = f.call::<i64, ()>(buf);
        }
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (found, ctx)
    }

    // ─── `vimcode.loop`/`vimcode.schedule`/`vimcode.defer` (#1624) ─────────

    fn next_handle_id(&self) -> u64 {
        let id = self.next_handle_id.get();
        self.next_handle_id.set(id + 1);
        id
    }

    /// Register a timer/schedule/defer callback and return its id. Called
    /// from `Engine::plugin_api_register_timer` while a live engine is
    /// loaned, so the returned id can be inserted into
    /// `Engine::plugin_timers` in the same call.
    pub(crate) fn register_timer_callback(&self, callback: LuaRegistryKey) -> u64 {
        let id = self.next_handle_id();
        self.timer_callbacks.borrow_mut().insert(id, callback);
        id
    }

    /// Drop a timer's stored callback (on `stop()`, on fire-once completion,
    /// or when the owning plugin is unloaded).
    pub(crate) fn remove_timer_callback(&self, id: u64) {
        self.timer_callbacks.borrow_mut().remove(&id);
    }

    /// Fire timer/schedule/defer id `id`'s callback with no arguments.
    pub(crate) fn call_timer_callback(&self, id: u64, ctx: PluginCallContext) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let f = self
            .timer_callbacks
            .borrow()
            .get(&id)
            .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok());
        if let Some(f) = f {
            let _ = f.call::<(), ()>(());
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// Register a `vimcode.loop.spawn` handle's callbacks and return its id.
    pub(crate) fn register_spawn_callbacks(
        &self,
        on_stdout: Option<LuaRegistryKey>,
        on_stderr: Option<LuaRegistryKey>,
        on_exit: Option<LuaRegistryKey>,
    ) -> u64 {
        let id = self.next_handle_id();
        self.spawn_callbacks.borrow_mut().insert(
            id,
            SpawnCallbacks {
                on_stdout,
                on_stderr,
                on_exit,
            },
        );
        id
    }

    /// Drop a spawn handle's stored callbacks (on exit, on `kill()`-then-exit,
    /// or when the owning plugin is unloaded).
    pub(crate) fn remove_spawn_callbacks(&self, id: u64) {
        self.spawn_callbacks.borrow_mut().remove(&id);
    }

    fn call_spawn_str_callback(
        &self,
        id: u64,
        text: &str,
        ctx: PluginCallContext,
        which: impl Fn(&SpawnCallbacks) -> Option<&LuaRegistryKey>,
    ) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let f = {
            let cbs = self.spawn_callbacks.borrow();
            cbs.get(&id)
                .and_then(which)
                .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok())
        };
        if let Some(f) = f {
            let _ = f.call::<String, ()>(text.to_string());
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// Fire spawn `id`'s `on_stdout` callback with one streamed chunk.
    pub(crate) fn call_spawn_stdout(
        &self,
        id: u64,
        chunk: &str,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        self.call_spawn_str_callback(id, chunk, ctx, |c| c.on_stdout.as_ref())
    }

    /// Fire spawn `id`'s `on_stderr` callback with one streamed chunk.
    pub(crate) fn call_spawn_stderr(
        &self,
        id: u64,
        chunk: &str,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        self.call_spawn_str_callback(id, chunk, ctx, |c| c.on_stderr.as_ref())
    }

    /// Fire spawn `id`'s `on_exit` callback with `(code, signal)` — exactly
    /// one of the two is non-nil (a normal exit carries a code, a
    /// signal/kill death carries a signal number; see `execute::spawn_piped`).
    pub(crate) fn call_spawn_exit(
        &self,
        id: u64,
        code: Option<i32>,
        signal: Option<i32>,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let f = {
            let cbs = self.spawn_callbacks.borrow();
            cbs.get(&id)
                .and_then(|c| c.on_exit.as_ref())
                .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok())
        };
        if let Some(f) = f {
            let _ = f.call::<(Option<i64>, Option<i64>), ()>((
                code.map(i64::from),
                signal.map(i64::from),
            ));
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    // ─── `vimcode.http` (#1632) ─────────────────────────────────────────────

    /// Register a `vimcode.http.request` handle's callback and return its id.
    pub(crate) fn register_http_callback(&self, callback: LuaRegistryKey) -> u64 {
        let id = self.next_handle_id();
        self.http_callbacks.borrow_mut().insert(id, callback);
        id
    }

    /// Drop a request's stored callback (on response, on `cancel()`, or when
    /// the owning plugin is unloaded).
    pub(crate) fn remove_http_callback(&self, id: u64) {
        self.http_callbacks.borrow_mut().remove(&id);
    }

    /// Fire request `id`'s callback with its result, converted to the
    /// `{status, headers, body, elapsed_ms}` / `{error}` table shape the
    /// issue's `vimcode.http.request` callback signature promises.
    pub(crate) fn call_http_response(
        &self,
        id: u64,
        result: crate::core::engine::execute::HttpResult,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let f = self
            .http_callbacks
            .borrow()
            .get(&id)
            .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok());
        if let Some(f) = f {
            if let Ok(resp_table) = self.lua.create_table() {
                match result {
                    crate::core::engine::execute::HttpResult::Ok(resp) => {
                        let _ = resp_table.set("status", resp.status);
                        if let Ok(headers_tbl) = self.lua.create_table() {
                            for (k, v) in &resp.headers {
                                let _ = headers_tbl.set(k.as_str(), v.as_str());
                            }
                            let _ = resp_table.set("headers", headers_tbl);
                        }
                        let _ = resp_table.set("body", resp.body);
                        let _ = resp_table.set("elapsed_ms", resp.elapsed_ms);
                    }
                    crate::core::engine::execute::HttpResult::Err(msg) => {
                        let _ = resp_table.set("error", msg);
                    }
                }
                let _ = f.call::<LuaTable, ()>(resp_table);
            }
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    // ─── `vimcode.picker` (#1630) ───────────────────────────────────────────

    /// Register a new picker's callbacks and return its id. Called from
    /// `Engine::plugin_api_picker_open` while a live engine is loaned, so the
    /// returned id can be embedded into a fresh `PickerSource::Custom
    /// ("plugin:<id>")` in the same call.
    pub(crate) fn register_picker(
        &self,
        on_select: Option<LuaRegistryKey>,
        on_cancel: Option<LuaRegistryKey>,
        on_query: Option<LuaRegistryKey>,
    ) -> u64 {
        let id = self.next_handle_id();
        self.pickers.borrow_mut().insert(
            id,
            PluginPicker {
                on_select,
                on_cancel,
                on_query,
                items: HashMap::new(),
            },
        );
        id
    }

    /// Drop picker `id`'s callbacks and every item it currently holds (on
    /// select, on cancel, on an explicit `:close()`, or when the owning
    /// plugin is unloaded) — a single `HashMap::remove` releases the whole
    /// per-item `data`/preview map with it.
    pub(crate) fn remove_picker(&self, id: u64) {
        self.pickers.borrow_mut().remove(&id);
    }

    /// Number of live `vimcode.picker.open` registrations this manager
    /// currently holds. Not used by any production code path — it exists so
    /// black-box tests (`tests/extensions.rs`) can prove a picker's
    /// registration was actually released rather than merely becoming
    /// unreachable (the #1630 review's leak findings: `plugin_api_picker_
    /// open` superseding a still-registered handle, and `close_picker()`
    /// being called without plugin teardown on a non-Escape dismiss path).
    pub fn picker_registration_count(&self) -> usize {
        self.pickers.borrow().len()
    }

    /// Drop picker `id`'s current items without touching its callbacks —
    /// `Engine::plugin_api_picker_set_items`'s `replace = true` case (a fresh
    /// `:set_items()` call) uses this before registering the new list.
    pub(crate) fn clear_picker_items(&self, id: u64) {
        if let Some(p) = self.pickers.borrow_mut().get_mut(&id) {
            p.items.clear();
        }
    }

    /// Register one item's `data`/preview under picker `id` and return its
    /// item id. A no-op (item id still returned, just never stored) if `id`
    /// names a picker that no longer exists — defensive only: callers reach
    /// here after `Engine::plugin_picker_is_active` already confirmed the
    /// picker is live, so this should not normally happen.
    pub(crate) fn register_picker_item(
        &self,
        id: u64,
        data: Option<LuaRegistryKey>,
        preview: Option<PluginPickerPreview>,
    ) -> u64 {
        let item_id = self.next_handle_id();
        if let Some(p) = self.pickers.borrow_mut().get_mut(&id) {
            p.items
                .insert(item_id, PluginPickerItemEntry { data, preview });
        }
        item_id
    }

    /// Read back item `item_id`'s preview source, if it declared one.
    pub(crate) fn picker_item_preview(&self, id: u64, item_id: u64) -> Option<PluginPickerPreview> {
        self.pickers
            .borrow()
            .get(&id)?
            .items
            .get(&item_id)?
            .preview
            .clone()
    }

    /// Fire picker `id`'s `on_select` callback with item `item_id`'s `data`
    /// value (`nil` if the item declared none, or if the item/picker is
    /// somehow gone by the time this runs).
    pub(crate) fn call_picker_select(
        &self,
        id: u64,
        item_id: u64,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let (on_select, data) = {
            let pickers = self.pickers.borrow();
            let on_select = pickers
                .get(&id)
                .and_then(|p| p.on_select.as_ref())
                .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok());
            let data = pickers
                .get(&id)
                .and_then(|p| p.items.get(&item_id))
                .and_then(|e| e.data.as_ref())
                .and_then(|k| self.lua.registry_value::<LuaValue>(k).ok())
                .unwrap_or(LuaValue::Nil);
            (on_select, data)
        };
        if let Some(f) = on_select {
            let _ = f.call::<LuaValue, ()>(data);
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// Fire picker `id`'s `on_cancel` callback with no arguments.
    pub(crate) fn call_picker_cancel(&self, id: u64, ctx: PluginCallContext) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let on_cancel = {
            let pickers = self.pickers.borrow();
            pickers
                .get(&id)
                .and_then(|p| p.on_cancel.as_ref())
                .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok())
        };
        if let Some(f) = on_cancel {
            let _ = f.call::<(), ()>(());
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// Fire picker `id`'s `on_query` callback with the current query text —
    /// a no-op if the picker declared none (plain fuzzy-filtering is enough
    /// for most pickers; `on_query` is only for a dynamic source that wants
    /// to re-run per keystroke, e.g. live grep).
    pub(crate) fn call_picker_query(
        &self,
        id: u64,
        query: &str,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        self.lua.set_app_data(ctx);
        let on_query = {
            let pickers = self.pickers.borrow();
            pickers
                .get(&id)
                .and_then(|p| p.on_query.as_ref())
                .and_then(|k| self.lua.registry_value::<LuaFunction>(k).ok())
        };
        if let Some(f) = on_query {
            let _ = f.call::<String, ()>(query.to_string());
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    // ─── Plugin-declared UI views (#146) ───────────────────────────────────

    /// Names of every `vimcode.ui.register_view` view, in unspecified order.
    pub fn view_names(&self) -> Vec<String> {
        self.views.keys().cloned().collect()
    }

    /// Whether `name` is a view-backed panel (as opposed to a
    /// `vimcode.panel.register` tree panel).
    pub fn is_view(&self, name: &str) -> bool {
        self.views.contains_key(name)
    }

    /// Run a view's `render` callback and parse the widget tree it returns.
    ///
    /// Returns `(updated_context, Ok(view) | Err(message))`. A Lua error or a
    /// malformed tree is surfaced as `Err` rather than panicking, so one broken
    /// plugin cannot take the sidebar down.
    pub fn render_view(
        &self,
        name: &str,
        ctx: PluginCallContext,
    ) -> (
        PluginCallContext,
        Result<crate::core::plugin_ui::PluginView, String>,
    ) {
        let Some(stored) = self.views.get(name) else {
            return (ctx, Err(format!("no registered view named {name:?}")));
        };
        self.lua.set_app_data(ctx);
        let result = match self.lua.registry_value::<LuaFunction>(&stored.render) {
            Ok(f) => match f.call::<LuaTable, LuaValue>(self.view_ctx_table(name)) {
                Ok(LuaValue::Table(t)) => lua_table_to_view(&t).map_err(|e| e.to_string()),
                Ok(LuaValue::Nil) => Ok(crate::core::plugin_ui::PluginView::default()),
                Ok(_) => Err(format!(
                    "view {name:?}: render() must return a table (got a non-table value)"
                )),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        };
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (ctx, result)
    }

    /// Fire a view's `on_event` callback for one widget event.
    ///
    /// Returns the updated context. A view without an `on_event` handler, or one
    /// whose handler errors, leaves the context otherwise untouched.
    pub fn call_view_event(
        &self,
        event: &crate::core::plugin_ui::PluginViewEvent,
        ctx: PluginCallContext,
    ) -> PluginCallContext {
        let Some(stored) = self.views.get(&event.view) else {
            return ctx;
        };
        let Some(key) = stored.on_event.as_ref() else {
            return ctx;
        };
        self.lua.set_app_data(ctx);
        if let Ok(f) = self.lua.registry_value::<LuaFunction>(key) {
            if let Ok(tbl) = self.view_event_table(event) {
                let _ = f.call::<(LuaTable, LuaTable), ()>((self.view_ctx_table(&event.view), tbl));
            }
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// The `ctx` table handed to `render(ctx)` / `on_event(ctx, event)`.
    ///
    /// Deliberately minimal: everything a callback needs is already reachable
    /// through `vimcode.*` (including the immediate `vimcode.buffer.*` API,
    /// #1214). `ctx` only names *which* view is being rendered, so one Lua
    /// function can serve several registered views.
    fn view_ctx_table(&self, view: &str) -> LuaTable<'_> {
        let tbl = self.lua.create_table().unwrap_or_else(|_| {
            // `create_table` only fails on OOM; there is no useful fallback, and
            // an empty table keeps the signature infallible for callers.
            self.lua.create_table().expect("lua table")
        });
        let _ = tbl.set("view", view);
        tbl
    }

    /// Convert a [`crate::core::plugin_ui::PluginViewEvent`] into the Lua table
    /// `on_event` receives: `{widget_id=, kind=, view=, value=}`.
    fn view_event_table(
        &self,
        event: &crate::core::plugin_ui::PluginViewEvent,
    ) -> LuaResult<LuaTable<'_>> {
        use crate::core::plugin_ui::ViewEventKind;
        let tbl = self.lua.create_table()?;
        tbl.set("view", event.view.as_str())?;
        tbl.set("widget_id", event.widget_id.as_str())?;
        tbl.set("kind", event.kind.kind_name())?;
        match &event.kind {
            ViewEventKind::ToggleChanged { value } => tbl.set("value", *value)?,
            ViewEventKind::DropdownChanged { selected }
            | ViewEventKind::SegmentedChanged { selected } => {
                // 0-based, matching the `selected` the plugin authored.
                tbl.set("value", *selected as i64)?
            }
            ViewEventKind::TextChanged { value } | ViewEventKind::TextCommitted { value } => {
                tbl.set("value", value.as_str())?
            }
            ViewEventKind::ItemSelected { index } | ViewEventKind::ItemActivated { index } => {
                tbl.set("index", *index as i64)?
            }
            ViewEventKind::NodeSelected { id }
            | ViewEventKind::NodeActivated { id }
            | ViewEventKind::Expanded { id }
            | ViewEventKind::Collapsed { id } => tbl.set("node_id", id.as_str())?,
            ViewEventKind::CellEdited { row, col, value } => {
                tbl.set("row", *row as i64)?;
                tbl.set("col", *col as i64)?;
                tbl.set("value", value.as_str())?;
            }
            ViewEventKind::ButtonClicked | ViewEventKind::FocusChanged => {}
        }
        Ok(tbl)
    }

    // ─── Lua API setup ─────────────────────────────────────────────────────

    /// Install the `vimcode.*` global table into the Lua state.
    ///
    /// Registration callbacks (`vimcode.command`, `vimcode.on`, `vimcode.keymap`)
    /// write into `PluginRegistrations` stored in app_data during loading.
    ///
    /// Runtime callbacks (`vimcode.message`, `vimcode.buf.*`, etc.) read/write
    /// `PluginCallContext` stored in app_data during dispatch.
    fn setup_vimcode_api(lua: &Lua) -> LuaResult<()> {
        let vimcode = lua.create_table()?;

        // ── Harden the global `load` (#1632 review) ──────────────────────────
        //
        // `current_plugin_chunk_name` (used to namespace `vimcode.storage.*`)
        // trusts a Lua chunk's own debug `source`, which stock Lua's `load(chunk,
        // chunkname)` lets *any* caller set to an arbitrary string. Left alone,
        // `load(payload, "other-plugin-name")()` would let a malicious plugin
        // impersonate another plugin's storage namespace — all plugins share one
        // `Lua` VM (`PluginManager::new`), so nothing else stops it.
        //
        // Replace the global `load` with a wrapper that ignores whatever
        // chunkname the *caller* passes and always substitutes the caller's own
        // real identity instead (itself derived via `current_plugin_chunk_name`,
        // walking the *actual* call stack at the point `load` is invoked — this
        // is unforgeable because it reflects where the code calling `load` was
        // truly defined, not any user-suppliable string). Dynamically loaded
        // code therefore always inherits its caller's namespace, never an
        // impersonated one. This only affects the Lua-visible `load`; the
        // trusted host-side load in `load_one_plugin` uses `Lua::load` directly
        // (a different, Rust-only API) and is unaffected.
        {
            let globals = lua.globals();
            let real_load: LuaFunction = globals.get("load")?;
            let real_load_key = lua.create_registry_value(real_load)?;
            let hardened_load = lua.create_function(move |lua, args: LuaMultiValue| {
                let owner =
                    current_plugin_chunk_name(lua).unwrap_or_else(|| "<unknown>".to_string());
                let mut it = args.into_iter();
                let chunk = it.next().unwrap_or(LuaNil);
                let _discarded_chunkname = it.next();
                let mode = it.next();
                let env = it.next();
                let mut call_args = vec![chunk, LuaValue::String(lua.create_string(&owner)?)];
                call_args.extend(mode);
                call_args.extend(env);
                let real_load: LuaFunction = lua.registry_value(&real_load_key)?;
                real_load.call::<_, LuaMultiValue>(LuaMultiValue::from_vec(call_args))
            })?;
            globals.set("load", hardened_load)?;
        }

        // ── vimcode.json (#1632) ─────────────────────────────────────────────
        //
        // The `null`/`empty_object` sentinels are created once here (not
        // lazily inside `encode`/`decode`) so every comparison in
        // `lua_value_to_json`/`json_to_lua_value` sees the exact same table
        // for the lifetime of this `Lua` — see that section's module doc.
        lua.set_named_registry_value(JSON_NULL_REGISTRY_KEY, lua.create_table()?)?;
        lua.set_named_registry_value(JSON_EMPTY_OBJECT_REGISTRY_KEY, lua.create_table()?)?;

        let json_tbl = lua.create_table()?;
        json_tbl.set(
            "encode",
            lua.create_function(|lua, (value, opts): (LuaValue, Option<LuaTable>)| {
                let json_val = lua_value_to_json(lua, value)?;
                let pretty = opts
                    .and_then(|o| o.get::<_, bool>("pretty").ok())
                    .unwrap_or(false);
                let s = if pretty {
                    serde_json::to_string_pretty(&json_val)
                } else {
                    serde_json::to_string(&json_val)
                }
                .map_err(|e| LuaError::RuntimeError(format!("vimcode.json.encode: {e}")))?;
                Ok(s)
            })?,
        )?;
        json_tbl.set(
            "decode",
            lua.create_function(|lua, s: String| {
                let json_val: serde_json::Value = serde_json::from_str(&s)
                    .map_err(|e| LuaError::RuntimeError(format!("vimcode.json.decode: {e}")))?;
                json_to_lua_value(lua, &json_val)
            })?,
        )?;
        json_tbl.set(
            "null",
            lua.named_registry_value::<LuaTable>(JSON_NULL_REGISTRY_KEY)?,
        )?;
        json_tbl.set(
            "empty_object",
            lua.named_registry_value::<LuaTable>(JSON_EMPTY_OBJECT_REGISTRY_KEY)?,
        )?;
        vimcode.set("json", json_tbl)?;

        // ── vimcode.storage (#1632) ──────────────────────────────────────────
        let storage_tbl = lua.create_table()?;
        storage_tbl.set(
            "get",
            lua.create_function(
                |lua, (key, opts): (String, Option<LuaTable>)| -> LuaResult<LuaValue> {
                    let plugin =
                        current_plugin_chunk_name(lua).ok_or_else(storage_no_plugin_err)?;
                    let path = plugin_storage_path(&plugin, storage_opt_workspace(&opts));
                    match load_storage_map(&path).get(&key) {
                        Some(v) => json_to_lua_value(lua, v),
                        None => Ok(LuaValue::Nil),
                    }
                },
            )?,
        )?;
        storage_tbl.set(
            "set",
            lua.create_function(
                |lua,
                 (key, value, opts): (String, LuaValue, Option<LuaTable>)|
                 -> LuaResult<bool> {
                    let plugin =
                        current_plugin_chunk_name(lua).ok_or_else(storage_no_plugin_err)?;
                    let path = plugin_storage_path(&plugin, storage_opt_workspace(&opts));
                    let mut map = load_storage_map(&path);
                    let json_val = lua_value_to_json(lua, value)?;
                    map.insert(key, json_val);
                    save_storage_map(&path, &map)
                        .map_err(|e| LuaError::RuntimeError(format!("vimcode.storage.set: {e}")))?;
                    Ok(true)
                },
            )?,
        )?;
        storage_tbl.set(
            "delete",
            lua.create_function(
                |lua, (key, opts): (String, Option<LuaTable>)| -> LuaResult<bool> {
                    let plugin =
                        current_plugin_chunk_name(lua).ok_or_else(storage_no_plugin_err)?;
                    let path = plugin_storage_path(&plugin, storage_opt_workspace(&opts));
                    let mut map = load_storage_map(&path);
                    let existed = map.remove(&key).is_some();
                    if existed {
                        save_storage_map(&path, &map).map_err(|e| {
                            LuaError::RuntimeError(format!("vimcode.storage.delete: {e}"))
                        })?;
                    }
                    Ok(existed)
                },
            )?,
        )?;
        storage_tbl.set(
            "keys",
            lua.create_function(|lua, opts: Option<LuaTable>| -> LuaResult<LuaTable> {
                let plugin = current_plugin_chunk_name(lua).ok_or_else(storage_no_plugin_err)?;
                let path = plugin_storage_path(&plugin, storage_opt_workspace(&opts));
                let mut keys: Vec<String> = load_storage_map(&path).keys().cloned().collect();
                keys.sort();
                let t = lua.create_table()?;
                for (i, k) in keys.into_iter().enumerate() {
                    t.set(i + 1, k)?;
                }
                Ok(t)
            })?,
        )?;
        vimcode.set("storage", storage_tbl)?;

        // ── Registration callbacks ──────────────────────────────────────────

        // vimcode.on(event, fn)
        vimcode.set(
            "on",
            lua.create_function(|lua, (event, f): (String, LuaFunction)| {
                let key = lua.create_registry_value(f)?;
                if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.hooks.entry(event).or_default().push(key);
                }
                Ok(())
            })?,
        )?;

        // vimcode.command(name, fn)
        vimcode.set(
            "command",
            lua.create_function(|lua, (name, f): (String, LuaFunction)| {
                let key = lua.create_registry_value(f)?;
                if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.commands.insert(name, key);
                }
                Ok(())
            })?,
        )?;

        // vimcode.keymap(mode, key, fn) — legacy (unchanged, #1214-and-earlier
        // back-compat): dispatched *after* built-ins, via
        // `Engine::plugin_run_keymap` / `PluginManager::call_keymap`.
        //
        // vimcode.keymap.set(mode, lhs, fn, opts) / vimcode.keymap.list() —
        // the new API (#1623): a table with a `__call` metamethod keeps the
        // legacy call form working (`vimcode.keymap("v", "Q", fn)` calls the
        // table, invoking `legacy_call` below with the table as the extra
        // first argument Lua's `t(...)` sugar passes to `__call`).
        //
        // `.set` maps are consulted *before* built-ins (`Engine::user_keymaps`
        // / `try_user_keymap`), which is what lets a plugin own a key with a
        // built-in meaning (`s`, `ys`, `gc`, …). `opts` (all optional):
        // `expr` (bool — the callback's return value is fed back through the
        // key path instead of being expected to act directly), `buffer`
        // (handle — restricts the map to one buffer; only meaningful when
        // `.set` is called from inside a callback, where a live buffer
        // exists), `desc` (string — surfaced by `.list()`).
        let keymap_tbl = lua.create_table()?;
        let keymap_mt = lua.create_table()?;
        keymap_mt.set(
            "__call",
            lua.create_function(
                |lua, (_self, mode, k, f): (LuaValue, String, String, LuaFunction)| {
                    let key = lua.create_registry_value(f)?;
                    if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                        reg.keymaps.insert((mode, k), key);
                    }
                    Ok(())
                },
            )?,
        )?;
        keymap_tbl.set_metatable(Some(keymap_mt));

        keymap_tbl.set(
            "set",
            lua.create_function(
                |lua, (mode, lhs, cb, opts): (String, String, LuaFunction, Option<LuaTable>)| {
                    let expr = opts
                        .as_ref()
                        .and_then(|o| o.get::<_, bool>("expr").ok())
                        .unwrap_or(false);
                    let desc = opts.as_ref().and_then(|o| o.get::<_, String>("desc").ok());
                    let buffer_handle: Option<i64> =
                        opts.as_ref().and_then(|o| o.get::<_, i64>("buffer").ok());
                    let keys = parse_key_sequence(&lhs);
                    let key = lua.create_registry_value(cb)?;
                    // Load time: accumulate for harvest, like every other
                    // `vimcode.*` registration call. `buffer` is ignored here
                    // — there is no live buffer yet to resolve it against; a
                    // plugin wanting a buffer-local map registers it from
                    // inside a callback instead (e.g. `BufEnter`), which
                    // takes the runtime path below.
                    if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                        reg.lua_keymaps.push(PendingLuaKeymap {
                            mode,
                            keys,
                            callback: key,
                            expr,
                            desc,
                        });
                        return Ok(());
                    }
                    // Runtime: resolve `buffer` against the live engine and
                    // push directly onto `user_keymaps` — there is no later
                    // harvest step for a call made after load time.
                    live_engine("vimcode.keymap.set", move |e| {
                        let buffer = buffer_handle.and_then(|h| e.plugin_api_resolve_buf(h));
                        let leader = e.settings.leader.to_string();
                        let keys = expand_leader_tokens(keys, &leader);
                        if let Some(pm) = e.plugin_manager.clone() {
                            let id = pm.register_lua_keymap(key, expr);
                            e.user_keymaps.push(UserKeymap {
                                mode,
                                noremap: true,
                                keys,
                                action: UserKeymapAction::Lua(id),
                                buffer,
                                desc,
                            });
                        }
                    })?;
                    Ok(())
                },
            )?,
        )?;

        keymap_tbl.set(
            "list",
            lua.create_function(|lua, ()| {
                let entries = live_engine("vimcode.keymap.list", |e| {
                    PluginManager::list_keymaps_from(&e.user_keymaps)
                })?;
                let t = lua.create_table()?;
                for (i, (mode, lhs, buf, desc)) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("mode", mode)?;
                    row.set("lhs", lhs)?;
                    row.set("buffer", buf)?;
                    row.set("desc", desc)?;
                    t.set(i + 1, row)?;
                }
                Ok(t)
            })?,
        )?;

        vimcode.set("keymap", keymap_tbl)?;

        // vimcode.fire_event(name) — fire a plugin-defined "User" event
        // (#1623), the escape hatch for a plugin to signal its own
        // lifecycle points to other plugins: `vimcode.on("User", function(name)
        // if name == "MyPluginReady" then ... end end)`.
        vimcode.set(
            "fire_event",
            lua.create_function(|_, name: String| {
                live_engine("vimcode.fire_event", move |e| {
                    e.plugin_event("User", &name);
                })
            })?,
        )?;

        // vimcode.set_operatorfunc(fn) — register a function for g@{motion}
        vimcode.set(
            "set_operatorfunc",
            lua.create_function(|lua, f: LuaFunction| {
                let key = lua.create_registry_value(f)?;
                // Store in named registry slot for retrieval by call_operatorfunc
                lua.set_named_registry_value("__operatorfunc", key)?;
                Ok(())
            })?,
        )?;

        // ── Runtime callbacks ───────────────────────────────────────────────

        // vimcode.message(text)
        vimcode.set(
            "message",
            lua.create_function(|lua, text: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.message = Some(text);
                }
                Ok(())
            })?,
        )?;

        // vimcode.cwd()
        vimcode.set(
            "cwd",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.cwd.clone())
                    .unwrap_or_default())
            })?,
        )?;

        // vimcode.command_run(cmd)
        vimcode.set(
            "command_run",
            lua.create_function(|lua, cmd: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.run_commands.push(cmd);
                }
                Ok(())
            })?,
        )?;

        // vimcode.feedkeys(keys) — inject keystrokes into the engine.
        // Uses the same notation as Neovim: "dw", "<Esc>", "<C-a>", "<CR>".
        vimcode.set(
            "feedkeys",
            lua.create_function(|lua, keys: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.feedkeys_sequences.push(keys);
                }
                Ok(())
            })?,
        )?;

        // vimcode.eval(expr) — evaluate simple Vim-like expressions.
        // Supports: @a (register), &option (setting), line('.'), col('.').
        vimcode.set(
            "eval",
            lua.create_function(|lua, expr: String| -> LuaResult<LuaValue> {
                let ctx = lua.app_data_ref::<PluginCallContext>();
                let ctx = match ctx {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                let expr = expr.trim();
                if let Some(reg_char) = expr.strip_prefix('@') {
                    // Register contents: @a, @", @+, etc.
                    let ch = reg_char.chars().next().unwrap_or('"');
                    Ok(ctx
                        .registers_snapshot
                        .get(&ch)
                        .map(|(content, _)| LuaValue::String(lua.create_string(content).unwrap()))
                        .unwrap_or(LuaValue::Nil))
                } else if let Some(opt_name) = expr.strip_prefix('&') {
                    // Option value: &tabstop, &shiftwidth, etc.
                    Ok(ctx
                        .settings_snapshot
                        .get(opt_name)
                        .map(|v| LuaValue::String(lua.create_string(v).unwrap()))
                        .unwrap_or(LuaValue::Nil))
                } else if expr == "line('.')" {
                    Ok(LuaValue::Integer(ctx.cursor_line as i64))
                } else if expr == "col('.')" {
                    Ok(LuaValue::Integer(ctx.cursor_col as i64))
                } else if expr == "line('$')" {
                    let n = ctx
                        .buf_rope
                        .as_ref()
                        .map(|r| r.len_lines() as i64)
                        .unwrap_or(ctx.buf_lines.len() as i64);
                    Ok(LuaValue::Integer(n))
                } else if expr == "mode()" {
                    Ok(LuaValue::String(lua.create_string(&ctx.mode_name).unwrap()))
                } else {
                    Ok(LuaValue::Nil)
                }
            })?,
        )?;

        // vimcode.open_url(url)
        vimcode.set(
            "open_url",
            lua.create_function(|lua, url: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.open_urls.push(url);
                }
                Ok(())
            })?,
        )?;

        // vimcode.async_shell(command, callback_event [, options_table])
        // options: { stdin = "...", cwd = "..." }
        vimcode.set(
            "async_shell",
            lua.create_function(|lua, args: LuaMultiValue| {
                let command: String = args
                    .get(0)
                    .and_then(|v| match v {
                        LuaValue::String(s) => Some(s.to_str().ok()?.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let callback_event: String = args
                    .get(1)
                    .and_then(|v| match v {
                        LuaValue::String(s) => Some(s.to_str().ok()?.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                if command.is_empty() || callback_event.is_empty() {
                    return Ok(());
                }
                let mut stdin = None;
                let mut cwd = None;
                if let Some(LuaValue::Table(opts)) = args.get(2) {
                    if let Ok(s) = opts.get::<_, String>("stdin") {
                        stdin = Some(s);
                    }
                    if let Ok(s) = opts.get::<_, String>("cwd") {
                        cwd = Some(PathBuf::from(s));
                    }
                }
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.async_shell_requests.push(AsyncShellRequest {
                        command,
                        callback_event,
                        stdin,
                        cwd,
                    });
                }
                Ok(())
            })?,
        )?;

        // vimcode.async_shell_exit_code(callback_event) → integer | nil (#1624)
        //
        // `async_shell`'s callback signature is frozen (`function(output)`),
        // so the exit status this issue adds is surfaced out of band instead
        // of as a second argument: read it from inside (or after) the
        // callback for `callback_event` using the same live-engine seam
        // every other runtime accessor uses. `nil` means "no result has
        // landed yet" (or never will — the event name was never used).
        vimcode.set(
            "async_shell_exit_code",
            lua.create_function(|_, event: String| {
                live_engine("vimcode.async_shell_exit_code", move |e| {
                    e.async_shell_last_exit.get(&event).copied().flatten()
                })
            })?,
        )?;

        // vimcode.schedule(fn) — run `fn` once, on the next idle tick (#1624).
        // Implemented as a zero-delay, non-repeating `vimcode.loop.timer`:
        // "next idle tick" is exactly what an already-due, non-repeating
        // timer entry gives for free from `Engine::poll_plugin_timers`.
        vimcode.set(
            "schedule",
            lua.create_function(|lua, cb: LuaFunction| {
                let key = lua.create_registry_value(cb)?;
                live_engine("vimcode.schedule", move |e| {
                    e.plugin_api_register_timer(0, false, key);
                })
            })?,
        )?;

        // vimcode.defer(ms, fn) — run `fn` once, after `ms` milliseconds (#1624).
        vimcode.set(
            "defer",
            lua.create_function(|lua, (ms, cb): (i64, LuaFunction)| {
                let key = lua.create_registry_value(cb)?;
                live_engine("vimcode.defer", move |e| {
                    e.plugin_api_register_timer(ms, false, key);
                })
            })?,
        )?;

        // ── vimcode.loop (#1624) ─────────────────────────────────────────────
        let loop_tbl = lua.create_table()?;

        // vimcode.loop.timer(ms, fn, {repeat=bool}) → handle with :stop()
        //
        // Note for plugin authors: `repeat` is a reserved word in Lua, so it
        // cannot appear as a bare `{ repeat = true }` table-constructor key
        // (that is a Lua *syntax* error, not a vimcode limitation) — write
        // `{ ["repeat"] = true }` instead. Reading it from Rust as the plain
        // string key `"repeat"` below is unaffected either way.
        loop_tbl.set(
            "timer",
            lua.create_function(
                |lua, (ms, cb, opts): (i64, LuaFunction, Option<LuaTable>)| {
                    let repeat = opts
                        .as_ref()
                        .and_then(|o| o.get::<_, bool>("repeat").ok())
                        .unwrap_or(false);
                    let key = lua.create_registry_value(cb)?;
                    let id = live_engine("vimcode.loop.timer", move |e| {
                        e.plugin_api_register_timer(ms, repeat, key)
                    })?;
                    let handle = lua.create_table()?;
                    handle.set("id", id)?;
                    handle.set(
                        "stop",
                        lua.create_function(move |_, _self: LuaValue| {
                            live_engine("vimcode.loop.timer:stop", move |e| {
                                e.plugin_api_stop_timer(id);
                            })
                        })?,
                    )?;
                    Ok(handle)
                },
            )?,
        )?;

        // vimcode.loop.spawn(cmd, args, {cwd, env, on_stdout, on_stderr, on_exit})
        // → handle with :write(data), :close_stdin(), :kill()
        //
        // `on_stdout`/`on_stderr` are called with one streamed chunk each time
        // data arrives (not necessarily line-buffered); `on_exit` is called
        // once with `(code, signal)` — see `PluginManager::call_spawn_exit`.
        loop_tbl.set(
            "spawn",
            lua.create_function(
                |lua, (cmd, args, opts): (String, Option<Vec<String>>, Option<LuaTable>)| {
                    let args = args.unwrap_or_default();
                    let mut cwd = None;
                    let mut env: Vec<(String, String)> = Vec::new();
                    let mut on_stdout = None;
                    let mut on_stderr = None;
                    let mut on_exit = None;
                    if let Some(ref o) = opts {
                        if let Ok(c) = o.get::<_, String>("cwd") {
                            if !c.is_empty() {
                                cwd = Some(PathBuf::from(c));
                            }
                        }
                        if let Ok(e) = o.get::<_, LuaTable>("env") {
                            for pair in e.pairs::<String, String>().flatten() {
                                env.push(pair);
                            }
                        }
                        if let Ok(f) = o.get::<_, LuaFunction>("on_stdout") {
                            on_stdout = Some(lua.create_registry_value(f)?);
                        }
                        if let Ok(f) = o.get::<_, LuaFunction>("on_stderr") {
                            on_stderr = Some(lua.create_registry_value(f)?);
                        }
                        if let Ok(f) = o.get::<_, LuaFunction>("on_exit") {
                            on_exit = Some(lua.create_registry_value(f)?);
                        }
                    }
                    let id = live_engine("vimcode.loop.spawn", move |e| {
                        e.plugin_api_spawn(cmd, args, cwd, env, on_stdout, on_stderr, on_exit)
                    })?;
                    let Some(id) = id else {
                        return Err(LuaError::RuntimeError(
                            "vimcode.loop.spawn: failed to start process".to_string(),
                        ));
                    };
                    let handle = lua.create_table()?;
                    handle.set("id", id)?;
                    handle.set(
                        "write",
                        lua.create_function(move |_, (_self, data): (LuaValue, String)| {
                            live_engine("vimcode.loop.spawn:write", move |e| {
                                e.plugin_api_spawn_write(id, &data)
                            })
                        })?,
                    )?;
                    handle.set(
                        "close_stdin",
                        lua.create_function(move |_, _self: LuaValue| {
                            live_engine("vimcode.loop.spawn:close_stdin", move |e| {
                                e.plugin_api_spawn_close_stdin(id)
                            })
                        })?,
                    )?;
                    handle.set(
                        "kill",
                        lua.create_function(move |_, _self: LuaValue| {
                            live_engine("vimcode.loop.spawn:kill", move |e| {
                                e.plugin_api_spawn_kill(id)
                            })
                        })?,
                    )?;
                    Ok(handle)
                },
            )?,
        )?;

        vimcode.set("loop", loop_tbl)?;

        // ── vimcode.http (#1632) ─────────────────────────────────────────────
        let http_tbl = lua.create_table()?;

        // vimcode.http.request({method, url, headers, body, timeout_ms}, cb)
        // → handle with :cancel(). `cb` is called exactly once, on the main
        // thread via `Engine::poll_plugin_http`, with `{status, headers,
        // body, elapsed_ms}` on success or `{error}` on failure — never both,
        // never neither. Delivered the same way as every other stored
        // callback (#1624's `PluginCallContext`/`EngineLoan` machinery), so
        // it can use the immediate `vimcode.buffer`/`vimcode.window` API and
        // `vimcode.ui.refresh` just like a timer or spawn callback can.
        http_tbl.set(
            "request",
            lua.create_function(|lua, (opts, cb): (LuaTable, LuaFunction)| {
                let method: String = opts
                    .get::<_, String>("method")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "GET".to_string());
                let url: String = opts.get::<_, String>("url").unwrap_or_default();
                if url.is_empty() {
                    return Err(LuaError::RuntimeError(
                        "vimcode.http.request: 'url' is required".to_string(),
                    ));
                }
                let mut headers = Vec::new();
                if let Ok(h) = opts.get::<_, LuaTable>("headers") {
                    for (k, v) in h.pairs::<String, String>().flatten() {
                        // Reject CR/LF in header names/values here, at the
                        // Lua boundary, so a plugin gets an immediate, clear
                        // error instead of a generic "failed to start"
                        // further down — `spawn_http_request` also rejects
                        // these defensively (#1632 review: unstripped CR/LF
                        // let a header value smuggle extra header lines onto
                        // the wire, e.g. from response data or a template).
                        if k.contains('\r')
                            || k.contains('\n')
                            || v.contains('\r')
                            || v.contains('\n')
                        {
                            return Err(LuaError::RuntimeError(format!(
                                "vimcode.http.request: header {k:?} must not contain CR or LF"
                            )));
                        }
                        headers.push((k, v));
                    }
                }
                let body: Option<String> = opts.get::<_, String>("body").ok();
                let timeout_ms: u64 = opts
                    .get::<_, i64>("timeout_ms")
                    .ok()
                    .filter(|ms| *ms > 0)
                    .map(|ms| ms as u64)
                    .unwrap_or(30_000);
                let key = lua.create_registry_value(cb)?;
                let id = live_engine("vimcode.http.request", move |e| {
                    e.plugin_api_http_request(method, url, headers, body, timeout_ms, key)
                })?;
                let Some(id) = id else {
                    return Err(LuaError::RuntimeError(
                        "vimcode.http.request: failed to start request".to_string(),
                    ));
                };
                let handle = lua.create_table()?;
                handle.set("id", id)?;
                handle.set(
                    "cancel",
                    lua.create_function(move |_, _self: LuaValue| {
                        live_engine("vimcode.http.request:cancel", move |e| {
                            e.plugin_api_http_cancel(id)
                        })
                    })?,
                )?;
                Ok(handle)
            })?,
        )?;

        vimcode.set("http", http_tbl)?;

        // ── vimcode.buf subtable ────────────────────────────────────────────
        let buf = lua.create_table()?;

        // vimcode.buf.lines() → table of strings (1-indexed). Lazy: reads
        // from `buf_rope` on demand so plugins pay only for what they ask.
        buf.set(
            "lines",
            lua.create_function(|lua, ()| {
                let ctx = lua.app_data_ref::<PluginCallContext>();
                let t = lua.create_table()?;
                if let Some(ctx) = ctx {
                    if let Some(ref rope) = ctx.buf_rope {
                        for (i, line) in rope.lines().enumerate() {
                            t.set(i + 1, line.to_string())?;
                        }
                    } else {
                        for (i, line) in ctx.buf_lines.iter().enumerate() {
                            t.set(i + 1, line.as_str())?;
                        }
                    }
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buf.line(n) → string or nil (1-indexed)
        buf.set(
            "line",
            lua.create_function(|lua, n: usize| {
                let ctx = lua.app_data_ref::<PluginCallContext>();
                let Some(ctx) = ctx else { return Ok(None) };
                if n == 0 {
                    return Ok(None);
                }
                let idx = n - 1;
                if let Some(ref rope) = ctx.buf_rope {
                    if idx >= rope.len_lines() {
                        return Ok(None);
                    }
                    Ok(Some(rope.line(idx).to_string()))
                } else {
                    Ok(ctx.buf_lines.get(idx).cloned())
                }
            })?,
        )?;

        // vimcode.buf.set_line(n, text) (1-indexed)
        buf.set(
            "set_line",
            lua.create_function(|lua, (n, text): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.set_lines.push((n - 1, text));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.get_lines(start, end) → table of strings (0-indexed, exclusive end)
        // Neovim-compatible: nvim_buf_get_lines(0, start, end, false)
        buf.set(
            "get_lines",
            lua.create_function(|lua, (start, end): (i64, i64)| {
                let ctx = lua.app_data_ref::<PluginCallContext>();
                let t = lua.create_table()?;
                if let Some(ctx) = ctx {
                    let len = if let Some(ref rope) = ctx.buf_rope {
                        rope.len_lines() as i64
                    } else {
                        ctx.buf_lines.len() as i64
                    };
                    // Negative indices count from end (Neovim convention)
                    let s = if start < 0 {
                        (len + start).max(0) as usize
                    } else {
                        (start as usize).min(len as usize)
                    };
                    let e = if end < 0 {
                        (len + end).max(0) as usize
                    } else {
                        (end as usize).min(len as usize)
                    };
                    if let Some(ref rope) = ctx.buf_rope {
                        for (out_i, line_i) in (s..e).enumerate() {
                            t.set(out_i + 1, rope.line(line_i).to_string())?;
                        }
                    } else {
                        for (i, line) in ctx.buf_lines[s..e].iter().enumerate() {
                            t.set(i + 1, line.as_str())?;
                        }
                    }
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buf.set_lines(start, end, lines) (0-indexed, exclusive end)
        // Neovim-compatible: nvim_buf_set_lines(0, start, end, false, lines)
        buf.set(
            "set_lines",
            lua.create_function(|lua, (start, end, lines): (i64, i64, LuaTable)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    let len = if let Some(ref rope) = ctx.buf_rope {
                        rope.len_lines() as i64
                    } else {
                        ctx.buf_lines.len() as i64
                    };
                    let s = if start < 0 {
                        (len + start).max(0) as usize
                    } else {
                        start as usize
                    };
                    let e = if end < 0 {
                        (len + end).max(0) as usize
                    } else {
                        end as usize
                    };
                    let mut new_lines = Vec::new();
                    for i in 1..=lines.len().unwrap_or(0) {
                        if let Ok(line) = lines.get::<_, String>(i) {
                            new_lines.push(line);
                        }
                    }
                    ctx.set_lines_range.push((s, e, new_lines));
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.path() → string or nil
        buf.set(
            "path",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.buf_path.clone()))
            })?,
        )?;

        // vimcode.buf.line_count() → integer
        buf.set(
            "line_count",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| {
                        ctx.buf_rope
                            .as_ref()
                            .map(|r| r.len_lines())
                            .unwrap_or(ctx.buf_lines.len())
                    })
                    .unwrap_or(0))
            })?,
        )?;

        // vimcode.buf.cursor() → {line, col}  (1-indexed)
        buf.set(
            "cursor",
            lua.create_function(|lua, ()| {
                let t = lua.create_table()?;
                if let Some(ctx) = lua.app_data_ref::<PluginCallContext>() {
                    t.set("line", ctx.cursor_line)?;
                    t.set("col", ctx.cursor_col)?;
                } else {
                    t.set("line", 1usize)?;
                    t.set("col", 1usize)?;
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buf.set_cursor(line, col)  (1-indexed)
        buf.set(
            "set_cursor",
            lua.create_function(|lua, (line, col): (usize, usize)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if line > 0 && col > 0 {
                        ctx.set_cursor = Some((line, col));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.insert_line(n, text)  (1-indexed, inserts before line n)
        buf.set(
            "insert_line",
            lua.create_function(|lua, (n, text): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.insert_lines.push((n, text));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.delete_line(n)  (1-indexed)
        buf.set(
            "delete_line",
            lua.create_function(|lua, n: usize| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.delete_lines.push(n);
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.annotate_line(n, text)  (1-indexed)
        buf.set(
            "annotate_line",
            lua.create_function(|lua, (n, text): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.annotate_lines.push((n, text));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.clear_annotations()
        buf.set(
            "clear_annotations",
            lua.create_function(|lua, ()| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.clear_annotations = true;
                    ctx.annotate_lines.clear();
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.open_scratch(name, content, opts)
        // opts: { readonly=true, filetype="diff", split="vertical"|"horizontal"|nil }
        buf.set(
            "open_scratch",
            lua.create_function(
                |lua, (name, content, opts): (String, String, Option<LuaTable>)| {
                    let mut read_only = true;
                    let mut filetype = None;
                    let mut split = None;
                    if let Some(ref t) = opts {
                        if let Ok(ro) = t.get::<_, bool>("readonly") {
                            read_only = ro;
                        }
                        if let Ok(ft) = t.get::<_, String>("filetype") {
                            filetype = Some(ft);
                        }
                        if let Ok(s) = t.get::<_, String>("split") {
                            split = Some(s);
                        }
                    }
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.scratch_buffers.push(ScratchBufferRequest {
                            name,
                            content,
                            read_only,
                            filetype,
                            split,
                        });
                    }
                    Ok(())
                },
            )?,
        )?;

        vimcode.set("buf", buf)?;

        // ── vimcode.opt subtable ────────────────────────────────────────────
        let opt = lua.create_table()?;

        // vimcode.opt.get(key) → string
        opt.set(
            "get",
            lua.create_function(|lua, key: String| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| {
                        let v = ctx.settings_snapshot.get(&key)?;
                        if v.is_empty() {
                            None
                        } else {
                            Some(v.clone())
                        }
                    })
                    .unwrap_or_default())
            })?,
        )?;

        // vimcode.opt.set(key, value) — applied after callback returns
        opt.set(
            "set",
            lua.create_function(|lua, (key, value): (String, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.set_settings.push((key, value));
                }
                Ok(())
            })?,
        )?;

        vimcode.set("opt", opt)?;

        // ── vimcode.state subtable ──────────────────────────────────────────
        let state = lua.create_table()?;

        // vimcode.state.mode() → string (e.g. "Normal", "Insert", "Visual")
        state.set(
            "mode",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.mode_name.clone())
                    .unwrap_or_default())
            })?,
        )?;

        // vimcode.state.register(char) → {content, linewise} or nil
        state.set(
            "register",
            lua.create_function(|lua, reg: String| {
                let ch = match reg.chars().next() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                let ctx = match lua.app_data_ref::<PluginCallContext>() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                match ctx.registers_snapshot.get(&ch) {
                    Some((content, linewise)) => {
                        let t = lua.create_table()?;
                        t.set("content", content.clone())?;
                        t.set("linewise", *linewise)?;
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.state.set_register(char, content, linewise)
        state.set(
            "set_register",
            lua.create_function(|lua, (reg, content, linewise): (String, String, bool)| {
                if let Some(ch) = reg.chars().next() {
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.set_registers.push((ch, content, linewise));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.state.mark(char) → {line, col} (1-indexed) or nil
        state.set(
            "mark",
            lua.create_function(|lua, mark: String| {
                let ch = match mark.chars().next() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                let ctx = match lua.app_data_ref::<PluginCallContext>() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                match ctx.marks_snapshot.get(&ch) {
                    Some((line, col)) => {
                        let t = lua.create_table()?;
                        t.set("line", *line)?;
                        t.set("col", *col)?;
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.state.filetype() → string (e.g. "rust", "python") or ""
        state.set(
            "filetype",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.filetype.clone())
                    .unwrap_or_default())
            })?,
        )?;

        vimcode.set("state", state)?;

        // ── vimcode.git subtable ────────────────────────────────────────────
        let git_tbl = lua.create_table()?;

        // vimcode.git.blame_line(n) → {hash, author, date, relative_date, message} or nil
        git_tbl.set(
            "blame_line",
            lua.create_function(|lua, n: usize| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return Ok(LuaValue::Nil),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                // Only pipe in-memory content when the buffer has unsaved changes.
                // For a clean buffer the committed content on disk is identical,
                // so we skip the expensive `--contents -` path (which requires
                // building a full-file String and spawning git with stdin).
                // buf_lines come from Ropey's line() which includes the trailing
                // \n on each line, so join with "" not "\n".
                let buf_content: Option<String> = {
                    let ctx = lua.app_data_ref::<PluginCallContext>();
                    ctx.and_then(|c| {
                        if c.buf_dirty {
                            Some(c.buf_lines.join(""))
                        } else {
                            None
                        }
                    })
                };
                let info = match git::blame_line(&repo_root, &file, n, buf_content.as_deref()) {
                    Some(i) => i,
                    None => return Ok(LuaValue::Nil),
                };
                let t = lua.create_table()?;
                t.set("hash", info.hash)?;
                t.set("author", info.author)?;
                t.set("date", info.timestamp)?;
                t.set("relative_date", info.relative_date)?;
                t.set("message", info.message)?;
                t.set("not_committed", info.not_committed)?;
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.log_file(limit) → [{hash, message}, ...]
        git_tbl.set(
            "log_file",
            lua.create_function(|lua, limit: usize| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => {
                            return lua.create_table();
                        }
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table(),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let entries = git::log_file(&repo_root, &file, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("message", e.message)?;
                    t.set(i + 1, row)?;
                }
                Ok(t)
            })?,
        )?;

        // vimcode.git.show(hash) → string or nil
        git_tbl.set(
            "show",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::show_commit(&dir, &hash) {
                    Some(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.blame_file() → [{hash, author, date, relative_date, message, not_committed}, ...]
        git_tbl.set(
            "blame_file",
            lua.create_function(|lua, ()| {
                let (cwd_path, buf_path_os, buf_dirty) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return lua.create_table().map(LuaValue::Table),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone(), ctx.buf_dirty)
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let buf_content: Option<String> = if buf_dirty {
                    lua.app_data_ref::<PluginCallContext>()
                        .map(|c| c.buf_lines.join(""))
                } else {
                    None
                };
                let entries = git::blame_file_structured(&repo_root, &file, buf_content.as_deref());
                let t = lua.create_table()?;
                for (i, info) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", info.hash)?;
                    row.set("author", info.author)?;
                    row.set("date", info.timestamp)?;
                    row.set("relative_date", info.relative_date)?;
                    row.set("message", info.message)?;
                    row.set("not_committed", info.not_committed)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.line_log(start, end, limit) → [{hash, author, date, message}, ...]
        git_tbl.set(
            "line_log",
            lua.create_function(|lua, (start, end, limit): (usize, usize, usize)| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return lua.create_table().map(LuaValue::Table),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let entries = git::log_line_range(&repo_root, &file, start, end, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("author", e.author)?;
                    row.set("date", e.date)?;
                    row.set("message", e.message)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.diff_ref(ref) → string or nil
        git_tbl.set(
            "diff_ref",
            lua.create_function(|lua, ref_spec: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::diff_against_ref(&dir, &ref_spec) {
                    Some(s) if !s.trim().is_empty() => Ok(LuaValue::String(lua.create_string(&s)?)),
                    _ => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.file_log(limit) → [{hash, author, date, message, stat}, ...]
        // (detailed version, replaces the simple log_file for richer data)
        git_tbl.set(
            "file_log_detailed",
            lua.create_function(|lua, limit: usize| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return lua.create_table().map(LuaValue::Table),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let entries = git::file_log_detailed(&repo_root, &file, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("author", e.author)?;
                    row.set("date", e.date)?;
                    row.set("message", e.message)?;
                    row.set("stat", e.stat)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.repo_root() → string or nil
        git_tbl.set(
            "repo_root",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                match cwd_path.and_then(|p| git::find_repo_root(&p)) {
                    Some(root) => Ok(LuaValue::String(
                        lua.create_string(root.to_string_lossy().as_bytes())?,
                    )),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.branch() → string or nil
        git_tbl.set(
            "branch",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::current_branch(&dir) {
                    Some(b) => Ok(LuaValue::String(lua.create_string(&b)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.stash_list() → [{index, message, branch}, ...]
        git_tbl.set(
            "stash_list",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let entries = git::stash_list(&dir);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("index", e.index)?;
                    row.set("message", e.message)?;
                    row.set("branch", e.branch)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.stash_push(msg) → string (result message)
        git_tbl.set(
            "stash_push",
            lua.create_function(|lua, msg: Option<String>| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok("no working directory".to_string()),
                };
                match git::stash_push(&dir, msg.as_deref()) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            })?,
        )?;

        // vimcode.git.stash_pop(index) → string (result message)
        git_tbl.set(
            "stash_pop",
            lua.create_function(|lua, index: Option<usize>| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok("no working directory".to_string()),
                };
                match git::stash_pop(&dir, index.unwrap_or(0)) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            })?,
        )?;

        // vimcode.git.stash_show(index) → string or nil
        git_tbl.set(
            "stash_show",
            lua.create_function(|lua, index: Option<usize>| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::stash_show(&dir, index.unwrap_or(0)) {
                    Some(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.log(limit) → [{hash, message}, ...] (repo-wide log)
        git_tbl.set(
            "log",
            lua.create_function(|lua, limit: usize| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let entries = git::git_log(&dir, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("message", e.message)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.log_commit(hash) → {hash, message} or nil
        git_tbl.set(
            "log_commit",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::git_log_commit(&dir, &hash) {
                    Some(e) => {
                        let row = lua.create_table()?;
                        row.set("hash", e.hash)?;
                        row.set("message", e.message)?;
                        Ok(LuaValue::Table(row))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.branches() → [{name, is_current, upstream, ahead_behind}]
        git_tbl.set(
            "branches",
            lua.create_function(|lua, ()| {
                let cwd = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let branches = git::list_branches(&dir);
                let t = lua.create_table()?;
                for (i, b) in branches.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("name", b.name.as_str())?;
                    row.set("is_current", b.is_current)?;
                    row.set(
                        "upstream",
                        if b.upstream.is_some() {
                            LuaValue::String(
                                lua.create_string(b.upstream.as_deref().unwrap_or(""))?,
                            )
                        } else {
                            LuaValue::Nil
                        },
                    )?;
                    row.set(
                        "ahead_behind",
                        if b.ahead_behind.is_some() {
                            LuaValue::String(
                                lua.create_string(b.ahead_behind.as_deref().unwrap_or(""))?,
                            )
                        } else {
                            LuaValue::Nil
                        },
                    )?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.commit_url(hash) → string or nil (HTTPS URL to commit on hosting platform)
        git_tbl.set(
            "commit_url",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::commit_url(&repo_root, &hash) {
                    Some(url) => Ok(LuaValue::String(lua.create_string(&url)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.remote_url() → string or nil (HTTPS base URL of the origin remote)
        git_tbl.set(
            "remote_url",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::remote_url(&repo_root) {
                    Some(url) => Ok(LuaValue::String(lua.create_string(&url)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.commit_files(hash) → [{status="M", path="src/main.rs"}, ...] or nil
        git_tbl.set(
            "commit_files",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                let files = git::commit_files(&repo_root, &hash);
                if files.is_empty() {
                    return Ok(LuaValue::Nil);
                }
                let t = lua.create_table()?;
                for (i, f) in files.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("status", lua.create_string(f.status.to_string())?)?;
                    row.set("path", lua.create_string(&f.path)?)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.diff_file(hash, path) → string or nil (file content at commit)
        git_tbl.set(
            "diff_file",
            lua.create_function(|lua, (hash, path): (String, String)| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::diff_file_at_commit(&repo_root, &hash, &path) {
                    Some(content) => Ok(LuaValue::String(lua.create_string(&content)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.open_diff(hash, path) — open side-by-side diff (hash~1 vs hash)
        git_tbl.set(
            "open_diff",
            lua.create_function(|lua, (hash, path): (String, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.commit_file_diff = Some((hash, path));
                }
                Ok(())
            })?,
        )?;

        // vimcode.git.show_file(hash, path) → string or nil (diff for a file in a commit)
        git_tbl.set(
            "show_file",
            lua.create_function(|lua, (hash, path): (String, String)| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::show_commit_file(&repo_root, &hash, &path) {
                    Some(content) => Ok(LuaValue::String(lua.create_string(&content)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.commit_detail(hash) → {hash, author, date, message, stat} or nil
        git_tbl.set(
            "commit_detail",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::commit_detail(&repo_root, &hash) {
                    Some(detail) => {
                        let t = lua.create_table()?;
                        t.set("hash", lua.create_string(&detail.hash)?)?;
                        t.set("author", lua.create_string(&detail.author)?)?;
                        t.set("date", lua.create_string(&detail.date)?)?;
                        t.set("message", lua.create_string(&detail.message)?)?;
                        t.set("stat", lua.create_string(&detail.stat)?)?;
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        vimcode.set("git", git_tbl)?;

        // ── vimcode.panel subtable ──────────────────────────────────────────
        let panel_tbl = lua.create_table()?;

        // vimcode.panel.register(name, opts) — register an extension panel
        panel_tbl.set(
            "register",
            lua.create_function(|lua, (name, opts): (String, LuaTable)| {
                let title: String = opts.get("title").unwrap_or_default();
                let icon_str: String = opts.get("icon").unwrap_or_default();
                let icon = icon_str
                    .chars()
                    .next()
                    .unwrap_or(crate::icons::PLUGIN_FALLBACK.c());
                let fb_str: String = opts.get("fallback_icon").unwrap_or_default();
                let fallback_icon = fb_str.chars().next();
                let sections_val: LuaTable = opts.get("sections")?;
                let mut sections = Vec::new();
                for (_, s) in sections_val.pairs::<usize, String>().flatten() {
                    sections.push(s);
                }
                if sections.is_empty() {
                    sections.push("Default".to_string());
                }
                let reg = PanelRegistration {
                    name: name.clone(),
                    title,
                    icon,
                    fallback_icon,
                    sections,
                };
                // During load_one_plugin: write to PluginRegistrations
                if let Some(mut regs) = lua.app_data_mut::<PluginRegistrations>() {
                    regs.panels.push(reg.clone());
                }
                // During runtime calls: write to PluginCallContext
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.panel_registrations.push(reg);
                }
                Ok(())
            })?,
        )?;

        // vimcode.panel.set_items(name, section, items)
        panel_tbl.set(
            "set_items",
            lua.create_function(
                |lua, (name, section, items_tbl): (String, String, LuaTable)| {
                    let mut items = Vec::new();
                    for (_, row) in items_tbl.pairs::<usize, LuaTable>().flatten() {
                        let text: String = row.get("text").unwrap_or_default();
                        let hint: String = row.get("hint").unwrap_or_default();
                        let icon: String = row.get("icon").unwrap_or_default();
                        let indent: u8 = row.get("indent").unwrap_or(0);
                        let style_str: String = row.get("style").unwrap_or_default();
                        let style = match style_str.as_str() {
                            "header" => ExtPanelStyle::Header,
                            "dim" => ExtPanelStyle::Dim,
                            "accent" => ExtPanelStyle::Accent,
                            _ => ExtPanelStyle::Normal,
                        };
                        let id: String = row.get("id").unwrap_or_default();
                        let expandable: bool = row.get("expandable").unwrap_or(false);
                        let expanded: bool = row.get("expanded").unwrap_or(false);
                        let parent_id: String = row.get("parent_id").unwrap_or_default();
                        let is_separator: bool = row.get("is_separator").unwrap_or(false);
                        // Parse actions: [{label="Stage", key="s"}, ...]
                        let mut actions = Vec::new();
                        if let Ok(acts_tbl) = row.get::<_, LuaTable>("actions") {
                            for (_, act) in acts_tbl.pairs::<usize, LuaTable>().flatten() {
                                actions.push(ExtPanelAction {
                                    label: act.get("label").unwrap_or_default(),
                                    key: act.get("key").unwrap_or_default(),
                                });
                            }
                        }
                        // Parse badges: [{text="main", color="green"}, ...]
                        let mut badges = Vec::new();
                        if let Ok(bdg_tbl) = row.get::<_, LuaTable>("badges") {
                            for (_, bdg) in bdg_tbl.pairs::<usize, LuaTable>().flatten() {
                                badges.push(ExtPanelBadge {
                                    text: bdg.get("text").unwrap_or_default(),
                                    color: bdg.get("color").unwrap_or_default(),
                                });
                            }
                        }
                        items.push(ExtPanelItem {
                            text,
                            hint,
                            icon,
                            indent,
                            style,
                            id,
                            expandable,
                            expanded,
                            parent_id,
                            actions,
                            badges,
                            is_separator,
                        });
                    }
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.panel_set_items.push((name, section, items));
                    }
                    Ok(())
                },
            )?,
        )?;

        // vimcode.panel.set_hover(panel_name, item_id, markdown) — register hover content
        panel_tbl.set(
            "set_hover",
            lua.create_function(
                |lua, (panel_name, item_id, markdown): (String, String, String)| {
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.panel_hover_entries
                            .push((panel_name, item_id, markdown));
                    }
                    Ok(())
                },
            )?,
        )?;

        // vimcode.panel.set_help(panel_name, bindings) — register help popup bindings
        // bindings is a table of {key, description} pairs
        panel_tbl.set(
            "set_help",
            lua.create_function(|lua, (panel_name, bindings): (String, LuaTable)| {
                let mut entries = Vec::new();
                for pair in bindings.sequence_values::<LuaTable>() {
                    let t = pair?;
                    let key: String = t.get(1)?;
                    let desc: String = t.get(2)?;
                    entries.push((key, desc));
                }
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.panel_help_entries.push((panel_name, entries));
                } else if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.help_bindings.push((panel_name, entries));
                }
                Ok(())
            })?,
        )?;

        // vimcode.panel.parse_event(arg) — split "|"-delimited event arg into table
        panel_tbl.set(
            "parse_event",
            lua.create_function(|lua, arg: String| {
                let parts: Vec<&str> = arg.splitn(5, '|').collect();
                let t = lua.create_table()?;
                t.set("panel", parts.first().copied().unwrap_or(""))?;
                t.set("section", parts.get(1).copied().unwrap_or(""))?;
                t.set("id", parts.get(2).copied().unwrap_or(""))?;
                t.set("key", parts.get(3).copied().unwrap_or(""))?;
                t.set(
                    "index",
                    parts
                        .get(4)
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0),
                )?;
                Ok(t)
            })?,
        )?;

        // vimcode.panel.get_input(panel_name) — get the current input field text
        panel_tbl.set(
            "get_input",
            lua.create_function(|lua, panel_name: String| {
                if let Some(ctx) = lua.app_data_ref::<PluginCallContext>() {
                    if let Some(text) = ctx.panel_input_snapshot.get(&panel_name) {
                        return Ok(text.clone());
                    }
                }
                Ok(String::new())
            })?,
        )?;

        // vimcode.panel.set_input(panel_name, text) — set the input field text
        panel_tbl.set(
            "set_input",
            lua.create_function(|lua, (panel_name, text): (String, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.panel_input_values.push((panel_name, text));
                }
                Ok(())
            })?,
        )?;

        // vimcode.panel.reveal(panel_name, section_name, item_id) — switch to panel and highlight item
        panel_tbl.set(
            "reveal",
            lua.create_function(
                |lua, (panel_name, section_name, item_id): (String, String, String)| {
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.panel_reveal_request = Some((panel_name, section_name, item_id));
                    }
                    Ok(())
                },
            )?,
        )?;

        vimcode.set("panel", panel_tbl)?;

        // ── vimcode.ui subtable (#146) ─────────────────────────────────────
        let ui_tbl = lua.create_table()?;

        // vimcode.ui.register_view(name, {title=, icon=, fallback_icon=,
        //                                 render=fn, on_event=fn})
        //
        // Load-time only (like `vimcode.command` / `vimcode.keymap`): the two
        // callbacks are stashed in the Lua registry so they survive the load and
        // can be fired from any later dispatch. `PluginRegistrations` is only
        // installed while a plugin's top-level chunk runs, so a `register_view`
        // from inside a callback is a no-op — documented in EXTENSIONS.md.
        ui_tbl.set(
            "register_view",
            lua.create_function(|lua, (name, opts): (String, LuaTable)| {
                if name.is_empty() {
                    return Err(LuaError::RuntimeError(
                        "vimcode.ui.register_view: name must not be empty".to_string(),
                    ));
                }
                let render: LuaFunction = opts.get("render").map_err(|_| {
                    LuaError::RuntimeError(format!(
                        "vimcode.ui.register_view({name:?}): `render` must be a function"
                    ))
                })?;
                let render_key = lua.create_registry_value(render)?;
                let on_event_key = match opts.get::<_, LuaFunction>("on_event") {
                    Ok(f) => Some(lua.create_registry_value(f)?),
                    Err(_) => None,
                };
                let title: String = opts.get("title").unwrap_or_default();
                let title = if title.is_empty() {
                    name.clone()
                } else {
                    title
                };
                let icon_str: String = opts.get("icon").unwrap_or_default();
                let icon = icon_str
                    .chars()
                    .next()
                    .unwrap_or(crate::icons::PLUGIN_FALLBACK.c());
                let fb_str: String = opts.get("fallback_icon").unwrap_or_default();
                let panel = PanelRegistration {
                    name: name.clone(),
                    title,
                    icon,
                    fallback_icon: fb_str.chars().next(),
                    // A view-backed panel paints a `quadraui::Form`, not
                    // sections of tree rows — but the sidebar's activity-bar
                    // registration path is shared with `vimcode.panel.register`,
                    // which requires at least one section name.
                    sections: vec!["Default".to_string()],
                };
                if let Some(mut regs) = lua.app_data_mut::<PluginRegistrations>() {
                    regs.views.push(ViewRegistration {
                        panel,
                        render: render_key,
                        on_event: on_event_key,
                    });
                }
                Ok(())
            })?,
        )?;

        // vimcode.ui.refresh(name) — re-run a view's `render` callback.
        ui_tbl.set(
            "refresh",
            lua.create_function(|lua, name: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if !ctx.plugin_view_refresh.contains(&name) {
                        ctx.plugin_view_refresh.push(name);
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.ui.open_view(name, {location = "tab"}) — open a registered
        // view as an editor-area tab (#1627), the second of the two hosting
        // surfaces #146 deferred ("Not yet supported" in EXTENSIONS.md).
        // `"tab"` is the only supported `location` today; a call with any
        // other value (or a `name` that isn't a registered view) is a Lua
        // error rather than a silent no-op, matching `register_view`'s own
        // "fail loudly at the Lua/Rust boundary" convention.
        //
        // Immediate ("live engine") API, like `vimcode.buffer.*`: opening a
        // tab is a structural engine mutation (`Engine::
        // open_plugin_view_tab` — a real `Tab`/`Window`/scratch `BufferId`,
        // via the pre-existing `Engine::new_tab`), not queued output data, so
        // it needs `live_engine` the way `vimcode.buffer.create` does rather
        // than a `PluginCallContext` field `apply_plugin_ctx` applies later.
        ui_tbl.set(
            "open_view",
            lua.create_function(|_, (name, opts): (String, Option<LuaTable>)| {
                let location: String = opts
                    .as_ref()
                    .and_then(|t| t.get::<_, String>("location").ok())
                    .unwrap_or_else(|| "tab".to_string());
                if location != "tab" {
                    return Err(LuaError::RuntimeError(format!(
                        "vimcode.ui.open_view({name:?}): unsupported location {location:?} \
                         (only \"tab\" is supported)"
                    )));
                }
                let opened = live_engine("vimcode.ui.open_view", move |e| {
                    e.open_plugin_view_tab(&name)
                })?;
                if !opened {
                    return Err(LuaError::RuntimeError(
                        "vimcode.ui.open_view: name is not a registered view".to_string(),
                    ));
                }
                Ok(())
            })?,
        )?;

        vimcode.set("ui", ui_tbl)?;

        // ── vimcode.picker (#1630) ──────────────────────────────────────────
        //
        // `vimcode.picker.open({title, items, on_select, on_cancel, on_query})`
        // opens the built-in fuzzy picker (`src/core/engine/picker.rs`) fed by
        // plugin data instead of a built-in `PickerSource` — the telescope-
        // style seam #1212's epic asks for. Items are `{display, filter_text?,
        // detail?, icon?, data?, preview?}`; `data` is an opaque value handed
        // back to `on_select` verbatim (any Lua value, including `nil`).
        // `preview` is `{file=path, line=n}` or `{buffer=handle, line=n}`
        // (`line` optional and 1-indexed either way).
        //
        // The returned handle supports live updates, for an async source fed
        // by e.g. `vimcode.loop.spawn`'s streamed stdout: `:set_items(items)`
        // replaces the whole list, `:append(items)` adds to it (re-filtering
        // against the current query without resetting the selection),
        // `:set_loading(bool)` marks it still-fetching, `:close()` closes it
        // early. All four are immediate-API calls, like `vimcode.loop.spawn`'s
        // handle methods — and, like those, callable with `.` or `:` (the
        // first parameter is an ignored `self`).
        let picker_tbl = lua.create_table()?;
        picker_tbl.set(
            "open",
            lua.create_function(|lua, opts: LuaTable| {
                let title: String = opts.get("title").unwrap_or_default();
                let on_select = match opts.get::<_, LuaFunction>("on_select") {
                    Ok(f) => Some(lua.create_registry_value(f)?),
                    Err(_) => None,
                };
                let on_cancel = match opts.get::<_, LuaFunction>("on_cancel") {
                    Ok(f) => Some(lua.create_registry_value(f)?),
                    Err(_) => None,
                };
                let on_query = match opts.get::<_, LuaFunction>("on_query") {
                    Ok(f) => Some(lua.create_registry_value(f)?),
                    Err(_) => None,
                };
                let id = live_engine("vimcode.picker.open", move |e| {
                    e.plugin_api_picker_open(title, on_select, on_cancel, on_query)
                })?;
                let Some(id) = id else {
                    return Err(LuaError::RuntimeError(
                        "vimcode.picker.open: no live plugin manager".to_string(),
                    ));
                };
                if let Ok(items) = opts.get::<_, LuaTable>("items") {
                    let specs = lua_table_to_picker_items(lua, &items)?;
                    live_engine("vimcode.picker.open", move |e| {
                        e.plugin_api_picker_set_items(id, specs, true)
                    })?;
                }
                make_picker_handle(lua, id)
            })?,
        )?;
        vimcode.set("picker", picker_tbl)?;

        // ── vimcode.editor subtable ────────────────────────────────────────
        let editor_tbl = lua.create_table()?;

        // vimcode.editor.set_hover(line, markdown) — set hover content for a buffer line
        editor_tbl.set(
            "set_hover",
            lua.create_function(|lua, (line_1indexed, markdown): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if line_1indexed > 0 {
                        ctx.editor_hover_entries.push((line_1indexed - 1, markdown));
                    }
                }
                Ok(())
            })?,
        )?;

        vimcode.set("editor", editor_tbl)?;

        // ── vimcode.set_comment_style(lang_id, opts) ────────────────────────
        vimcode.set(
            "set_comment_style",
            lua.create_function(|lua, (lang_id, opts): (String, LuaTable)| {
                let line: String = opts.get("line").unwrap_or_default();
                let block_open: String = opts.get("block_open").unwrap_or_default();
                let block_close: String = opts.get("block_close").unwrap_or_default();
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.comment_style_overrides
                        .push((lang_id, line, block_open, block_close));
                }
                Ok(())
            })?,
        )?;

        // ── vimcode.buffer / vimcode.window: the immediate API (#1214) ──────
        Self::setup_live_api(lua, &vimcode)?;

        lua.globals().set("vimcode", vimcode)?;
        Ok(())
    }

    /// Install `vimcode.buffer.*` and `vimcode.window.*` — the immediate,
    /// handle-based API that reads and writes through a live `&mut Engine`
    /// instead of the [`PluginCallContext`] snapshot/queue.
    ///
    /// See this module's doc comment for the handle, line-index and cursor
    /// conventions, and [`EngineLoan`] for the borrow discipline these
    /// closures depend on.
    fn setup_live_api(lua: &Lua, vimcode: &LuaTable) -> LuaResult<()> {
        // ── vimcode.buffer ─────────────────────────────────────────────────
        let buffer = lua.create_table()?;

        // vimcode.buffer.current() → handle of the active buffer
        buffer.set(
            "current",
            lua.create_function(|_, ()| {
                live_engine("vimcode.buffer.current", |e| e.active_buffer_id().0 as i64)
            })?,
        )?;

        // vimcode.buffer.is_valid(buf) → bool
        buffer.set(
            "is_valid",
            lua.create_function(|_, handle: Option<i64>| {
                let handle = handle.unwrap_or(0);
                live_engine("vimcode.buffer.is_valid", move |e| {
                    e.plugin_api_resolve_buf(handle).is_some()
                })
            })?,
        )?;

        // vimcode.buffer.line_count(buf) → logical line count (0 if invalid)
        buffer.set(
            "line_count",
            lua.create_function(|_, handle: Option<i64>| {
                let handle = handle.unwrap_or(0);
                live_engine("vimcode.buffer.line_count", move |e| {
                    e.plugin_api_resolve_buf(handle)
                        .map(|b| e.plugin_api_line_count(b) as i64)
                        .unwrap_or(0)
                })
            })?,
        )?;

        // vimcode.buffer.get_lines(buf, start, end) → table of strings,
        // newline-terminator stripped.
        buffer.set(
            "get_lines",
            lua.create_function(|lua, (handle, start, end): (i64, i64, i64)| {
                let lines = live_engine("vimcode.buffer.get_lines", move |e| {
                    e.plugin_api_resolve_buf(handle)
                        .map(|b| e.plugin_api_get_lines(b, start, end))
                        .unwrap_or_default()
                })?;
                let t = lua.create_table()?;
                for (i, line) in lines.into_iter().enumerate() {
                    t.set(i + 1, line)?;
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buffer.set_lines(buf, start, end, lines) → bool (false when
        // the handle does not name a live buffer). Takes effect immediately.
        buffer.set(
            "set_lines",
            lua.create_function(
                |_, (handle, start, end, lines): (i64, i64, i64, LuaTable)| {
                    let mut new_lines = Vec::new();
                    for i in 1..=lines.len().unwrap_or(0) {
                        if let Ok(line) = lines.get::<_, String>(i) {
                            new_lines.push(line);
                        }
                    }
                    live_engine("vimcode.buffer.set_lines", move |e| {
                        match e.plugin_api_resolve_buf(handle) {
                            Some(b) => {
                                e.plugin_api_set_lines(b, start, end, new_lines);
                                true
                            }
                            None => false,
                        }
                    })
                },
            )?,
        )?;

        // vimcode.buffer.create({scratch=bool, name=string}) → handle
        //
        // `listed` is deliberately absent: vimcode has no unlisted-buffer
        // concept yet, and inventing one is outside #1214's seam-only scope.
        buffer.set(
            "create",
            lua.create_function(|_, opts: Option<LuaTable>| {
                let mut scratch = false;
                let mut name = None;
                if let Some(ref t) = opts {
                    if let Ok(v) = t.get::<_, bool>("scratch") {
                        scratch = v;
                    }
                    if let Ok(v) = t.get::<_, String>("name") {
                        if !v.is_empty() {
                            name = Some(v);
                        }
                    }
                }
                live_engine("vimcode.buffer.create", move |e| {
                    e.plugin_api_create_buffer(scratch, name).0 as i64
                })
            })?,
        )?;

        // vimcode.buffer.set_write_handler(buf, fn) — claim `buf`'s writes
        // (#1623, the oil.nvim `BufWriteCmd` shape): `:w`/`save()` on `buf`
        // calls `fn(buf)` instead of writing to disk. `fn` is responsible for
        // whatever persistence makes sense; the buffer is marked clean
        // afterwards (there is no `vim.bo.modified`-equivalent setter yet for
        // it to do that itself). Returns `false` when `buf` doesn't name a
        // live buffer.
        buffer.set(
            "set_write_handler",
            lua.create_function(|lua, (handle, cb): (i64, LuaFunction)| {
                let key = lua.create_registry_value(cb)?;
                live_engine("vimcode.buffer.set_write_handler", move |e| {
                    match (e.plugin_api_resolve_buf(handle), e.plugin_manager.clone()) {
                        (Some(buf), Some(pm)) => {
                            pm.set_write_handler(buf.0 as i64, key);
                            true
                        }
                        _ => false,
                    }
                })
            })?,
        )?;

        vimcode.set("buffer", buffer)?;

        // ── vimcode.window ─────────────────────────────────────────────────
        let window = lua.create_table()?;

        // vimcode.window.current() → handle of the focused window
        window.set(
            "current",
            lua.create_function(|_, ()| {
                live_engine("vimcode.window.current", |e| e.active_window_id().0 as i64)
            })?,
        )?;

        // vimcode.window.is_valid(win) → bool
        window.set(
            "is_valid",
            lua.create_function(|_, handle: Option<i64>| {
                let handle = handle.unwrap_or(0);
                live_engine("vimcode.window.is_valid", move |e| {
                    e.plugin_api_resolve_win(handle).is_some()
                })
            })?,
        )?;

        // vimcode.window.get_buf(win) → buffer handle (0 when invalid)
        window.set(
            "get_buf",
            lua.create_function(|_, handle: Option<i64>| {
                let handle = handle.unwrap_or(0);
                live_engine("vimcode.window.get_buf", move |e| {
                    e.plugin_api_win_get_buf(handle)
                        .map(|b| b.0 as i64)
                        .unwrap_or(0)
                })
            })?,
        )?;

        // vimcode.window.set_buf(win, buf) → bool. Shows `buf` in `win`.
        window.set(
            "set_buf",
            lua.create_function(|_, (win, buf): (i64, i64)| {
                live_engine("vimcode.window.set_buf", move |e| {
                    e.plugin_api_win_set_buf(win, buf)
                })
            })?,
        )?;

        // vimcode.window.get_cursor(win) → {line=, col=, [1]=line, [2]=col}
        // (1-indexed, like vimcode.buf.cursor()).
        window.set(
            "get_cursor",
            lua.create_function(|lua, handle: Option<i64>| {
                let handle = handle.unwrap_or(0);
                let pos = live_engine("vimcode.window.get_cursor", move |e| {
                    e.plugin_api_win_get_cursor(handle)
                })?;
                let t = lua.create_table()?;
                let (line, col) = pos.unwrap_or((1, 1));
                t.set("line", line)?;
                t.set("col", col)?;
                t.set(1, line)?;
                t.set(2, col)?;
                Ok(t)
            })?,
        )?;

        // vimcode.window.set_cursor(win, {line, col}) → bool. Accepts either
        // named (`line`/`col`) or positional (`[1]`/`[2]`) fields.
        window.set(
            "set_cursor",
            lua.create_function(|_, (handle, pos): (i64, LuaTable)| {
                let line = pos
                    .get::<_, usize>("line")
                    .or_else(|_| pos.get::<_, usize>(1))
                    .unwrap_or(1);
                let col = pos
                    .get::<_, usize>("col")
                    .or_else(|_| pos.get::<_, usize>(2))
                    .unwrap_or(1);
                live_engine("vimcode.window.set_cursor", move |e| {
                    e.plugin_api_win_set_cursor(handle, line, col)
                })
            })?,
        )?;

        vimcode.set("window", window)?;

        // ── vimcode.decor (#1653, Native API P5) ─────────────────────────────
        Self::setup_decor_api(lua, vimcode)?;

        // ── vimcode.syntax / vimcode.undo / vimcode.diagnostics (#1654, P6) ──
        Self::setup_syntax_api(lua, vimcode)?;
        Self::setup_undo_api(lua, vimcode)?;
        Self::setup_diagnostics_api(lua, vimcode)?;

        Ok(())
    }

    /// Install `vimcode.decor.*`: namespaces, namespaced extmarks (optionally
    /// ranged), named highlight groups, and the virtual-text/sign options a
    /// mark can carry. See `core::buffer`'s decoration types for the
    /// underlying model and `Engine::plugin_api_decor_*` (in
    /// `engine/plugins.rs`) for the handle-resolution logic these wrappers
    /// stay thin over.
    fn setup_decor_api(lua: &Lua, vimcode: &LuaTable) -> LuaResult<()> {
        let decor = lua.create_table()?;

        // vimcode.decor.namespace(name) → namespace id (idempotent by name).
        decor.set(
            "namespace",
            lua.create_function(|_, name: String| {
                live_engine("vimcode.decor.namespace", move |e| {
                    e.plugin_api_decor_namespace(&name)
                })
            })?,
        )?;

        // vimcode.decor.set_hl(name, {fg=, bg=, bold=, italic=, underline=,
        // link=}) — registers/updates a named highlight group. `fg`/`bg` are
        // `"#rrggbb"`/`"#rrggbbaa"` strings; `link` names another group whose
        // resolved colours are used instead (re-resolved on `ColorScheme`,
        // not frozen at `set_hl` time).
        decor.set(
            "set_hl",
            lua.create_function(|_, (name, opts): (String, LuaTable)| {
                let def = lua_table_to_hl_def(&opts)?;
                live_engine("vimcode.decor.set_hl", move |e| {
                    e.plugin_api_decor_set_hl(&name, def);
                })
            })?,
        )?;

        // vimcode.decor.set_mark(buf, ns, {row, col, end_row?, end_col?,
        // hl_group?, virt_text?, virt_text_pos?, sign_text?, sign_hl?}) →
        // mark id, or `nil` when `buf` isn't a live buffer. `row`/`col` are
        // 0-indexed, matching `vimcode.buffer.get_lines`/`set_lines`.
        decor.set(
            "set_mark",
            lua.create_function(|_, (buf, ns, opts): (i64, i64, LuaTable)| {
                let row: usize = opts.get::<_, Option<usize>>("row")?.unwrap_or(0);
                let col: usize = opts.get::<_, Option<usize>>("col")?.unwrap_or(0);
                let end_row: Option<usize> = opts.get("end_row")?;
                let end_col: Option<usize> = opts.get("end_col")?;
                let decor_opts = lua_table_to_decor_opts(&opts)?;
                let id = live_engine("vimcode.decor.set_mark", move |e| {
                    e.plugin_api_decor_set_mark(buf, ns, row, col, end_row, end_col, decor_opts)
                })?;
                Ok(id)
            })?,
        )?;

        // vimcode.decor.get_mark(buf, ns, id) → table or nil. `nil` both for
        // a stale id and for one belonging to a different namespace (a
        // plugin can only read back its own marks).
        decor.set(
            "get_mark",
            lua.create_function(|lua, (buf, ns, id): (i64, i64, i64)| {
                let mark = live_engine("vimcode.decor.get_mark", move |e| {
                    e.plugin_api_decor_get_mark(buf, ns, id)
                })?;
                match mark {
                    Some(m) => {
                        let t = lua.create_table()?;
                        t.set("row", m.row)?;
                        t.set("col", m.col)?;
                        t.set("end_row", m.end_row)?;
                        t.set("end_col", m.end_col)?;
                        if let Some(hl) = &m.opts.hl_group {
                            t.set("hl_group", hl.clone())?;
                        }
                        if let Some(st) = &m.opts.sign_text {
                            t.set("sign_text", st.clone())?;
                        }
                        if let Some(sh) = &m.opts.sign_hl {
                            t.set("sign_hl", sh.clone())?;
                        }
                        // #1653 review: a mark with virtual text couldn't be
                        // round-tripped back through `get_mark` at all —
                        // only hl_group/sign_text/sign_hl came back. Mirror
                        // `set_mark`'s own `virt_text = {{text, hl_group},
                        // ...}` shape exactly so a caller can feed this
                        // table's `virt_text`/`virt_text_pos` straight back
                        // into another `set_mark` call.
                        if !m.opts.virt_text.is_empty() {
                            let chunks = lua.create_table()?;
                            for (i, c) in m.opts.virt_text.iter().enumerate() {
                                let chunk = lua.create_table()?;
                                chunk.set("text", c.text.clone())?;
                                if let Some(hl) = &c.hl_group {
                                    chunk.set("hl_group", hl.clone())?;
                                }
                                chunks.set(i + 1, chunk)?;
                            }
                            t.set("virt_text", chunks)?;
                        }
                        if let Some(pos) = m.opts.virt_text_pos {
                            t.set(
                                "virt_text_pos",
                                match pos {
                                    crate::core::buffer::VirtTextPos::Eol => "eol",
                                    crate::core::buffer::VirtTextPos::Overlay => "overlay",
                                    crate::core::buffer::VirtTextPos::Inline => "inline",
                                },
                            )?;
                        }
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.decor.del_mark(buf, ns, id) → bool.
        decor.set(
            "del_mark",
            lua.create_function(|_, (buf, ns, id): (i64, i64, i64)| {
                live_engine("vimcode.decor.del_mark", move |e| {
                    e.plugin_api_decor_del_mark(buf, ns, id)
                })
            })?,
        )?;

        // vimcode.decor.clear(buf, ns, start?, end?) — removes every mark in
        // `ns` (optionally restricted to the 0-indexed, exclusive-`end` row
        // range `[start, end)`) that `buf` is carrying. A plugin names only
        // its own `ns`, so this can't touch another plugin's marks even
        // though every namespace shares one per-buffer store. `(buf, ns,
        // ...)` matches `set_mark`/`get_mark`/`del_mark`'s argument order.
        decor.set(
            "clear",
            lua.create_function(
                |_, (buf, ns, start, end): (i64, i64, Option<usize>, Option<usize>)| {
                    let range = match (start, end) {
                        (Some(s), Some(e)) => Some((s, e)),
                        _ => None,
                    };
                    live_engine("vimcode.decor.clear", move |e| {
                        e.plugin_api_decor_clear(buf, ns, range)
                    })
                },
            )?,
        )?;

        vimcode.set("decor", decor)?;
        Ok(())
    }

    /// Install `vimcode.syntax.*` (#1654, Native API P6): read-only
    /// tree-sitter access over whatever parse the engine already keeps for
    /// highlighting (`BufferState::syntax`) — no Lua-side tree object, every
    /// call returns a plain table. See `Engine::plugin_api_syntax_node_at`/
    /// `_query` (`engine/plugins.rs`) and `core::syntax::Syntax::node_at`/
    /// `query_captures` for the underlying logic.
    fn setup_syntax_api(lua: &Lua, vimcode: &LuaTable) -> LuaResult<()> {
        let syntax = lua.create_table()?;

        // vimcode.syntax.node_at(buf, row, col) → {type=, range=, language=,
        // parent=}. `row` is 0-indexed; `col` is a 0-indexed **byte** offset
        // within that row (tree-sitter's native unit — see `SyntaxRangeInfo`'s
        // doc comment in `core/syntax.rs`).
        syntax.set(
            "node_at",
            lua.create_function(|lua, (buf, row, col): (i64, usize, usize)| {
                let node = live_engine("vimcode.syntax.node_at", move |e| {
                    e.plugin_api_syntax_node_at(buf, row, col)
                })?
                .map_err(LuaError::RuntimeError)?;
                syntax_node_info_to_lua_table(lua, &node)
            })?,
        )?;

        // vimcode.syntax.query(buf, query_string, range?) → array of
        // {name=, type=, range=}. `range`, if given, is
        // `{start_row, end_row}` (0-indexed, exclusive `end_row`).
        syntax.set(
            "query",
            lua.create_function(
                |lua, (buf, query_src, range): (i64, String, Option<LuaTable>)| {
                    let row_range = match &range {
                        Some(t) => {
                            let start: usize = t.get(1)?;
                            let end: usize = t.get(2)?;
                            Some((start, end))
                        }
                        None => None,
                    };
                    let (_language, captures) = live_engine("vimcode.syntax.query", move |e| {
                        e.plugin_api_syntax_query(buf, &query_src, row_range)
                    })?
                    .map_err(LuaError::RuntimeError)?;
                    let out = lua.create_table()?;
                    for (i, c) in captures.iter().enumerate() {
                        let row = lua.create_table()?;
                        row.set("name", c.name.clone())?;
                        row.set("type", c.kind.clone())?;
                        row.set("range", syntax_range_to_lua_table(lua, &c.range)?)?;
                        out.set(i + 1, row)?;
                    }
                    Ok(out)
                },
            )?,
        )?;

        vimcode.set("syntax", syntax)?;
        Ok(())
    }

    /// Install `vimcode.undo.*` (#1654, Native API P6): read the live undo
    /// tree and jump straight to a recorded state by `seq`. See
    /// `Engine::plugin_api_undo_tree`/`_jump` (`engine/plugins.rs`).
    fn setup_undo_api(lua: &Lua, vimcode: &LuaTable) -> LuaResult<()> {
        let undo = lua.create_table()?;

        // vimcode.undo.tree(buf) → array of {seq=, parent=, time=, current=},
        // oldest first.
        undo.set(
            "tree",
            lua.create_function(|lua, buf: Option<i64>| {
                let buf = buf.unwrap_or(0);
                let nodes = live_engine("vimcode.undo.tree", move |e| e.plugin_api_undo_tree(buf))?;
                let out = lua.create_table()?;
                for (i, n) in nodes.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("seq", n.seq)?;
                    row.set("parent", n.parent)?;
                    let millis = n
                        .time
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    row.set("time", millis)?;
                    row.set("current", n.current)?;
                    out.set(i + 1, row)?;
                }
                Ok(out)
            })?,
        )?;

        // vimcode.undo.jump(buf, seq) → bool
        undo.set(
            "jump",
            lua.create_function(|_, (buf, seq): (i64, usize)| {
                live_engine("vimcode.undo.jump", move |e| {
                    e.plugin_api_undo_jump(buf, seq)
                })
            })?,
        )?;

        vimcode.set("undo", undo)?;
        Ok(())
    }

    /// Install `vimcode.diagnostics.get` (#1654, Native API P6). The
    /// `DiagnosticChanged` event (fired from `Engine::set_diagnostics_for_
    /// path`, `engine/panels.rs`) uses the existing generic `vimcode.on`
    /// registration — no separate wiring needed here.
    fn setup_diagnostics_api(lua: &Lua, vimcode: &LuaTable) -> LuaResult<()> {
        let diagnostics = lua.create_table()?;

        // vimcode.diagnostics.get(buf?) → array of {range=, severity=,
        // message=, source=, code=}. `buf` defaults to the active buffer.
        diagnostics.set(
            "get",
            lua.create_function(|lua, buf: Option<i64>| {
                let diags = live_engine("vimcode.diagnostics.get", move |e| {
                    e.plugin_api_diagnostics_get(buf)
                })?;
                let out = lua.create_table()?;
                for (i, d) in diags.iter().enumerate() {
                    let row = lua.create_table()?;
                    let range = lua.create_table()?;
                    range.set("start_row", d.range.start.line)?;
                    range.set("start_col", d.range.start.character)?;
                    range.set("end_row", d.range.end.line)?;
                    range.set("end_col", d.range.end.character)?;
                    row.set("range", range)?;
                    row.set(
                        "severity",
                        match d.severity {
                            crate::core::lsp::DiagnosticSeverity::Error => "error",
                            crate::core::lsp::DiagnosticSeverity::Warning => "warning",
                            crate::core::lsp::DiagnosticSeverity::Information => "information",
                            crate::core::lsp::DiagnosticSeverity::Hint => "hint",
                        },
                    )?;
                    row.set("message", d.message.clone())?;
                    row.set("source", d.source.clone())?;
                    row.set("code", d.code.clone())?;
                    out.set(i + 1, row)?;
                }
                Ok(out)
            })?,
        )?;

        vimcode.set("diagnostics", diagnostics)?;
        Ok(())
    }
}

/// Convert a [`crate::core::syntax::SyntaxRangeInfo`] into the
/// `{start_row=, start_col=, end_row=, end_col=}` table shape shared by
/// `vimcode.syntax.node_at`/`query`.
fn syntax_range_to_lua_table<'lua>(
    lua: &'lua Lua,
    range: &crate::core::syntax::SyntaxRangeInfo,
) -> LuaResult<LuaTable<'lua>> {
    let t = lua.create_table()?;
    t.set("start_row", range.start_row)?;
    t.set("start_col", range.start_col)?;
    t.set("end_row", range.end_row)?;
    t.set("end_col", range.end_col)?;
    Ok(t)
}

/// Convert a [`crate::core::syntax::SyntaxNodeInfo`] into the table
/// `vimcode.syntax.node_at` returns.
fn syntax_node_info_to_lua_table<'lua>(
    lua: &'lua Lua,
    node: &crate::core::syntax::SyntaxNodeInfo,
) -> LuaResult<LuaTable<'lua>> {
    let t = lua.create_table()?;
    t.set("type", node.kind.clone())?;
    t.set("range", syntax_range_to_lua_table(lua, &node.range)?)?;
    t.set("language", node.language.clone())?;
    if let Some(parent) = &node.parent {
        let p = lua.create_table()?;
        p.set("type", parent.kind.clone())?;
        p.set("range", syntax_range_to_lua_table(lua, &parent.range)?)?;
        t.set("parent", p)?;
    }
    Ok(t)
}

/// Parse a `vimcode.decor.set_hl` options table into an [`HlGroupDef`].
fn lua_table_to_hl_def(t: &LuaTable) -> LuaResult<HlGroupDef> {
    Ok(HlGroupDef {
        fg: t.get("fg")?,
        bg: t.get("bg")?,
        bold: t.get::<_, Option<bool>>("bold")?.unwrap_or(false),
        italic: t.get::<_, Option<bool>>("italic")?.unwrap_or(false),
        underline: t.get::<_, Option<bool>>("underline")?.unwrap_or(false),
        link: t.get("link")?,
    })
}

fn lua_parse_virt_text_pos(s: &str) -> Option<VirtTextPos> {
    match s {
        "eol" => Some(VirtTextPos::Eol),
        "overlay" => Some(VirtTextPos::Overlay),
        "inline" => Some(VirtTextPos::Inline),
        _ => None,
    }
}

/// Parse the subset of a `vimcode.decor.set_mark` options table that becomes
/// a [`DecorOpts`] (the `row`/`col`/`end_row`/`end_col` anchor fields are
/// read separately by the caller).
fn lua_table_to_decor_opts(t: &LuaTable) -> LuaResult<DecorOpts> {
    let hl_group: Option<String> = t.get("hl_group")?;
    let sign_text: Option<String> = t.get("sign_text")?;
    let sign_hl: Option<String> = t.get("sign_hl")?;
    let mut virt_text = Vec::new();
    if let Some(vt) = t.get::<_, Option<LuaTable>>("virt_text")? {
        for i in 1..=vt.raw_len() {
            if let Ok(chunk) = vt.get::<_, LuaTable>(i) {
                let text: String = chunk
                    .get::<_, Option<String>>(1)?
                    .or(chunk.get::<_, Option<String>>("text")?)
                    .unwrap_or_default();
                let hl: Option<String> = chunk
                    .get::<_, Option<String>>(2)?
                    .or(chunk.get::<_, Option<String>>("hl_group")?);
                virt_text.push(VirtTextChunk { text, hl_group: hl });
            }
        }
    }
    let virt_text_pos = t
        .get::<_, Option<String>>("virt_text_pos")?
        .and_then(|s| lua_parse_virt_text_pos(&s));
    Ok(DecorOpts {
        hl_group,
        virt_text,
        virt_text_pos,
        sign_text,
        sign_hl,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_plugin(dir: &std::path::Path, name: &str, code: &str) -> PathBuf {
        let path = dir.join(format!("{name}.lua"));
        std::fs::write(&path, code).unwrap();
        path
    }

    #[test]
    fn test_plugin_command_registered_and_callable() {
        let dir = std::env::temp_dir().join("vc_plugin_test_cmd");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "test",
            r#"
            vimcode.command("Hello", function(args)
                vimcode.message("Hello: " .. args)
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        assert_eq!(pm.plugins.len(), 1);
        assert!(pm.plugins[0].error.is_none());

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("Hello", "world", ctx);
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("Hello: world"));
    }

    #[test]
    fn test_plugin_on_save_hook_fires() {
        let dir = std::env::temp_dir().join("vc_plugin_test_save");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "savehook",
            r#"
            vimcode.on("save", function(path)
                vimcode.message("saved: " .. path)
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let ctx = pm.call_event("save", "/tmp/foo.rs", ctx);
        assert_eq!(ctx.message.as_deref(), Some("saved: /tmp/foo.rs"));
    }

    #[test]
    fn test_plugin_disabled_command_not_registered() {
        let dir = std::env::temp_dir().join("vc_plugin_test_disabled");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "myplugin",
            r#"
            vimcode.command("ShouldNotExist", function(args)
                vimcode.message("should not run")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &["myplugin".to_string()]);

        assert_eq!(pm.plugins.len(), 1);
        assert!(!pm.plugins[0].enabled);

        let ctx = PluginCallContext::default();
        let (found, _) = pm.call_command("ShouldNotExist", "", ctx);
        assert!(!found);
    }

    #[test]
    fn test_plugin_load_error_recorded() {
        let dir = std::env::temp_dir().join("vc_plugin_test_err");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(&dir, "broken", "this is not valid lua @@@");

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        assert_eq!(pm.plugins.len(), 1);
        assert!(pm.plugins[0].error.is_some());
    }

    #[test]
    fn test_plugin_keymap_registered_and_callable() {
        let dir = std::env::temp_dir().join("vc_plugin_test_km");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "keys",
            r#"
            vimcode.keymap("n", "<leader>x", function()
                vimcode.message("keymap fired")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_keymap("n", "<leader>x", ctx);
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("keymap fired"));
    }

    #[test]
    fn test_plugin_buf_api() {
        let dir = std::env::temp_dir().join("vc_plugin_test_buf");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "buftest",
            r#"
            vimcode.command("BufInfo", function(_)
                local count = vimcode.buf.line_count()
                local first = vimcode.buf.line(1) or "(nil)"
                vimcode.message("lines=" .. count .. " first=" .. first)
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext {
            buf_lines: vec!["hello".to_string(), "world".to_string()],
            ..Default::default()
        };
        let (found, ctx) = pm.call_command("BufInfo", "", ctx);
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("lines=2 first=hello"));
    }

    #[test]
    fn test_async_shell_request_registered() {
        let dir = std::env::temp_dir().join("vc_plugin_test_async_shell");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "asynctest",
            r#"
            vimcode.command("RunAsync", function(_)
                vimcode.async_shell("echo hello", "my_callback")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("RunAsync", "", ctx);
        assert!(found);
        assert_eq!(ctx.async_shell_requests.len(), 1);
        assert_eq!(ctx.async_shell_requests[0].command, "echo hello");
        assert_eq!(ctx.async_shell_requests[0].callback_event, "my_callback");
        assert!(ctx.async_shell_requests[0].stdin.is_none());
        assert!(ctx.async_shell_requests[0].cwd.is_none());
    }

    #[test]
    fn test_async_shell_with_options() {
        let dir = std::env::temp_dir().join("vc_plugin_test_async_opts");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "asyncopts",
            r#"
            vimcode.command("RunAsyncOpts", function(_)
                vimcode.async_shell("cat", "cat_result", { stdin = "hello world", cwd = "/tmp" })
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("RunAsyncOpts", "", ctx);
        assert!(found);
        assert_eq!(ctx.async_shell_requests.len(), 1);
        assert_eq!(ctx.async_shell_requests[0].command, "cat");
        assert_eq!(ctx.async_shell_requests[0].callback_event, "cat_result");
        assert_eq!(
            ctx.async_shell_requests[0].stdin.as_deref(),
            Some("hello world")
        );
        assert_eq!(
            ctx.async_shell_requests[0].cwd.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
    }

    #[test]
    fn test_async_shell_empty_args_ignored() {
        let dir = std::env::temp_dir().join("vc_plugin_test_async_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "asyncempty",
            r#"
            vimcode.command("RunEmpty", function(_)
                vimcode.async_shell("", "my_callback")
                vimcode.async_shell("echo hi", "")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("RunEmpty", "", ctx);
        assert!(found);
        // Both calls should be silently ignored (empty command or empty event).
        assert_eq!(ctx.async_shell_requests.len(), 0);
    }

    // ── #146: `vimcode.ui.*` view registration and dispatch ─────────────

    fn pm_with_view(tag: &str, code: &str) -> PluginManager {
        let dir = std::env::temp_dir().join(format!("vc_plugin_view_unit_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(&dir, "viewext", code);
        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);
        assert!(
            pm.plugins.iter().all(|p| p.error.is_none()),
            "plugin must load cleanly: {:?}",
            pm.plugins
                .iter()
                .filter_map(|p| p.error.clone())
                .collect::<Vec<_>>()
        );
        pm
    }

    #[test]
    fn view_registration_also_registers_an_activity_bar_panel() {
        let pm = pm_with_view(
            "reg",
            r#"
            vimcode.ui.register_view("vext", {
                title = "V Ext",
                icon = "V",
                render = function() return { fields = {} } end,
            })
            "#,
        );
        assert!(pm.is_view("vext"));
        assert_eq!(pm.view_names(), vec!["vext".to_string()]);
        // The same registration must show up as an ordinary sidebar panel, so the
        // activity bar and `Engine::ext_panels` need no view-specific path.
        let panel = pm.panels.get("vext").expect("panel registration");
        assert_eq!(panel.title, "V Ext");
        assert_eq!(panel.icon, 'V');
    }

    #[test]
    fn render_callback_returns_the_declared_tree() {
        use crate::core::plugin_ui::{ViewFieldKind, VIEW_SCHEMA_VERSION};
        let pm = pm_with_view(
            "render",
            r#"
            vimcode.ui.register_view("vext", {
                render = function(ctx)
                    return {
                        id = "main",
                        fields = {
                            { type = "label", id = "hdr", label = "H" },
                            { type = "text", id = "url", label = "URL", value = "u" },
                            { type = "dropdown", id = "m", options = {"GET","POST"},
                              selected = 1 },
                            { type = "button", id = "go", label = "Go" },
                        },
                    }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let view = view.expect("render must succeed");
        assert_eq!(view.id, "main");
        assert_eq!(view.schema_version, VIEW_SCHEMA_VERSION);
        assert_eq!(view.fields.len(), 4);
        assert_eq!(view.fields[1].label, "URL");
        assert_eq!(
            view.fields[1].kind,
            ViewFieldKind::Text {
                value: "u".to_string(),
                placeholder: String::new(),
            }
        );
        assert_eq!(
            view.fields[2].kind,
            ViewFieldKind::Dropdown {
                options: vec!["GET".to_string(), "POST".to_string()],
                selected: 1,
            }
        );
        assert_eq!(view.fields[3].kind, ViewFieldKind::Button);
    }

    #[test]
    fn an_unknown_field_type_is_an_error_not_a_silent_drop() {
        let pm = pm_with_view(
            "badtype",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return { fields = { { type = "buton", id = "go" } } }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let err = view.expect_err("an unknown widget type must not be dropped");
        assert!(
            err.contains("buton"),
            "the error must name the offending type: {err}"
        );
    }

    // ── #1631: `kind = "list"/"tree"/"table"/"text_view"` view bodies ────

    #[test]
    fn a_list_body_parses_its_items() {
        use crate::core::plugin_ui::ViewBody;
        let pm = pm_with_view(
            "list",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return {
                        kind = "list",
                        title = "Requests",
                        items = {
                            { id = "r1", text = "GET /users", detail = "200" },
                            { id = "r2", text = "POST /login" },
                        },
                    }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let view = view.expect("render must succeed");
        match view.body.expect("list body") {
            ViewBody::List { title, items } => {
                assert_eq!(title.as_deref(), Some("Requests"));
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].id, "r1");
                assert_eq!(items[0].detail.as_deref(), Some("200"));
                assert_eq!(items[1].detail, None);
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn a_list_item_missing_id_is_an_error_not_a_silent_drop() {
        let pm = pm_with_view(
            "list-badid",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return { kind = "list", items = { { text = "no id" } } }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let err = view.expect_err("a list item without an id must not be dropped");
        assert!(err.contains("list item"), "error was: {err}");
    }

    #[test]
    fn a_tree_body_parses_nested_nodes() {
        use crate::core::plugin_ui::ViewBody;
        let pm = pm_with_view(
            "tree",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return {
                        kind = "tree",
                        nodes = {
                            { id = "folder", label = "src", expanded = true,
                              children = {
                                { id = "file", label = "main.rs" },
                              } },
                        },
                    }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let view = view.expect("render must succeed");
        match view.body.expect("tree body") {
            ViewBody::Tree { nodes } => {
                assert_eq!(nodes.len(), 1);
                assert_eq!(nodes[0].id, "folder");
                assert!(nodes[0].expanded);
                assert!(nodes[0].is_branch());
                assert_eq!(nodes[0].children[0].id, "file");
                assert!(!nodes[0].children[0].is_branch());
            }
            other => panic!("expected Tree, got {other:?}"),
        }
    }

    #[test]
    fn a_tree_body_without_nodes_is_an_error() {
        let pm = pm_with_view(
            "tree-missing",
            r#"
            vimcode.ui.register_view("vext", {
                render = function() return { kind = "tree" } end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let err = view.expect_err("a tree view without \"nodes\" must not silently paint empty");
        assert!(err.contains("nodes"), "error was: {err}");
    }

    #[test]
    fn a_table_body_parses_columns_and_rows() {
        use crate::core::plugin_ui::ViewBody;
        let pm = pm_with_view(
            "table",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return {
                        kind = "table",
                        columns = {
                            { title = "Key", editable = true },
                            { title = "Value", editable = true },
                        },
                        rows = {
                            { id = "h1", cells = { "Content-Type", "application/json" } },
                        },
                    }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let view = view.expect("render must succeed");
        match view.body.expect("table body") {
            ViewBody::Table { columns, rows } => {
                assert_eq!(columns.len(), 2);
                assert!(columns[0].editable);
                assert_eq!(rows[0].cells, vec!["Content-Type", "application/json"]);
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn a_table_row_missing_cells_is_an_error() {
        let pm = pm_with_view(
            "table-badrow",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return { kind = "table", rows = { { id = "r1" } } }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let err = view.expect_err("a table row without cells must not be dropped");
        assert!(err.contains("cells"), "error was: {err}");
    }

    #[test]
    fn a_text_view_body_parses_text_and_filetype() {
        use crate::core::plugin_ui::ViewBody;
        let pm = pm_with_view(
            "textview",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return { kind = "text_view", text = "{}", filetype = "json" }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let view = view.expect("render must succeed");
        assert_eq!(
            view.body,
            Some(ViewBody::TextView {
                text: "{}".to_string(),
                filetype: Some("json".to_string()),
            })
        );
    }

    #[test]
    fn an_unknown_view_kind_is_an_error_not_a_silent_drop() {
        let pm = pm_with_view(
            "badkind",
            r#"
            vimcode.ui.register_view("vext", {
                render = function() return { kind = "grid" } end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let err = view.expect_err("an unknown view kind must not be dropped");
        assert!(err.contains("grid"), "error was: {err}");
    }

    #[test]
    fn item_activated_event_carries_the_index() {
        use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
        let pm = pm_with_view(
            "list-event",
            r#"
            local last = "none"
            vimcode.ui.register_view("vext", {
                render = function() return { kind = "list", items = {} } end,
                on_event = function(ctx, event)
                    last = event.kind .. "/" .. tostring(event.index)
                end,
            })
            vimcode.command("Last", function() vimcode.message(last) end)
            "#,
        );
        let ctx = pm.call_view_event(
            &PluginViewEvent {
                view: "vext".to_string(),
                widget_id: String::new(),
                kind: ViewEventKind::ItemActivated { index: 2 },
            },
            PluginCallContext::default(),
        );
        assert!(ctx.message.is_none());
        let (found, ctx) = pm.call_command("Last", "", PluginCallContext::default());
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("ItemActivated/2"));
    }

    #[test]
    fn cell_edited_event_carries_row_col_and_value() {
        use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
        let pm = pm_with_view(
            "table-event",
            r#"
            local last = "none"
            vimcode.ui.register_view("vext", {
                render = function() return { kind = "table" } end,
                on_event = function(ctx, event)
                    last = event.kind .. "/" .. event.row .. "/" .. event.col
                        .. "/" .. event.value
                end,
            })
            vimcode.command("Last", function() vimcode.message(last) end)
            "#,
        );
        let ctx = pm.call_view_event(
            &PluginViewEvent {
                view: "vext".to_string(),
                widget_id: String::new(),
                kind: ViewEventKind::CellEdited {
                    row: 1,
                    col: 0,
                    value: "text/plain".to_string(),
                },
            },
            PluginCallContext::default(),
        );
        assert!(ctx.message.is_none());
        let (found, ctx) = pm.call_command("Last", "", PluginCallContext::default());
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("CellEdited/1/0/text/plain"));
    }

    #[test]
    fn a_future_schema_version_is_refused() {
        let pm = pm_with_view(
            "schema",
            r#"
            vimcode.ui.register_view("vext", {
                render = function()
                    return { schema_version = 99, fields = {} }
                end,
            })
            "#,
        );
        let (_ctx, view) = pm.render_view("vext", PluginCallContext::default());
        let err = view.expect_err("a newer schema version must be refused");
        assert!(
            err.contains("schema_version 99"),
            "the error must name the declared version: {err}"
        );
    }

    #[test]
    fn on_event_receives_the_plugin_authored_widget_id_and_kind() {
        use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
        let pm = pm_with_view(
            "event",
            r#"
            local last = "none"
            vimcode.ui.register_view("vext", {
                render = function() return { fields = {} } end,
                on_event = function(ctx, event)
                    last = ctx.view .. "/" .. event.widget_id .. "/" .. event.kind
                        .. "/" .. tostring(event.value)
                end,
            })
            vimcode.command("Last", function() vimcode.message(last) end)
            "#,
        );
        let ctx = pm.call_view_event(
            &PluginViewEvent {
                view: "vext".to_string(),
                // The plugin sees the id it authored, never the namespaced
                // `plugin:vext:tls` `WidgetId` the paint layer uses.
                widget_id: "tls".to_string(),
                kind: ViewEventKind::ToggleChanged { value: true },
            },
            PluginCallContext::default(),
        );
        assert!(ctx.message.is_none(), "on_event must not message by itself");
        let (found, ctx) = pm.call_command("Last", "", PluginCallContext::default());
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("vext/tls/ToggleChanged/true"));
    }

    #[test]
    fn ui_refresh_queues_the_view_name_on_the_context() {
        let pm = pm_with_view(
            "refresh",
            r#"
            vimcode.ui.register_view("vext", {
                render = function() return { fields = {} } end,
            })
            vimcode.command("Poke", function()
                vimcode.ui.refresh("vext")
                vimcode.ui.refresh("vext")
            end)
            "#,
        );
        let (found, ctx) = pm.call_command("Poke", "", PluginCallContext::default());
        assert!(found);
        // De-duplicated: two calls in one dispatch are one refresh.
        assert_eq!(ctx.plugin_view_refresh, vec!["vext".to_string()]);
    }

    #[test]
    fn register_view_without_a_render_callback_is_an_error() {
        let dir = std::env::temp_dir().join("vc_plugin_view_unit_norender");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "viewext",
            r#"vimcode.ui.register_view("vext", { title = "No Render" })"#,
        );
        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);
        let err = pm.plugins[0]
            .error
            .as_deref()
            .expect("a view with no render callback must be a load error");
        assert!(err.contains("render"), "error must name `render`: {err}");
        assert!(!pm.is_view("vext"));
    }
}
