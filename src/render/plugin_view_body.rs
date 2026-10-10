use super::*;

// ─── ViewBody: List / Tree / Table / TextView (#1631) ───────────────────────
//
// #146/#1627's field stack paints through `quadraui::Form`/`FormController`.
// A `ViewBody`-kind view instead paints through the matching quadraui
// primitive directly — `ListView`, `TreeView` (via the existing
// `TreeController`, same one `Engine::explorer_tree` uses), `DataTable`, or
// `TextDisplay` — one sibling translation per kind, per #1631's design note,
// rather than wedging four different widget shapes into `Form`'s flat field
// rows. `List`/`Table` have no composed quadraui controller (unlike `Tree`'s
// `TreeController` and the field stack's `FormController`), so selection/
// scroll interaction for those two is hand-rolled here — in this
// backend-neutral module, not in `src/gtk/` or `src/tui_main/` — following
// the exact "paint caches a `Backend::*_layout`, a click reads it back" split
// `Engine::board_layout`/`Engine::ext_panel_tree_layout` already established
// for the same reason (#521/#1089).
//
// Selection/scroll reuse `ext_panel_selected`/`ext_panel_scroll_top` (sidebar)
// and `plugin_view_tab_selected`/`plugin_view_tab_scroll_top` (tab) verbatim
// instead of new per-body-kind fields — a view's body is one kind at a time,
// so there is never a collision, and it keeps `Engine`'s field count from
// growing by one pair per widget kind.

use crate::core::engine::PluginViewHost;
use crate::core::plugin_ui::{
    table_cell_field_id, ViewBody, ViewListItem, ViewTableColumn, ViewTableRow, ViewTreeNode,
};

/// Resolve the active view name for `host`, or `None` when nothing is hosted
/// there (mirrors the two paint arms' own resolution: `ext_panel_active` for
/// the sidebar, an explicit tab name for `Tab` — passed in directly since a
/// tab has no single "active view" field on `Engine`).
fn plugin_view_body<'a>(engine: &'a Engine, name: &str) -> Option<&'a ViewBody> {
    engine.plugin_views.get(name)?.body.as_ref()
}

// ── List ─────────────────────────────────────────────────────────────────

fn plugin_view_to_list(
    view: &str,
    title: &Option<String>,
    items: &[ViewListItem],
    selected: usize,
    scroll_top: usize,
    has_focus: bool,
) -> quadraui::ListView {
    use crate::core::plugin_ui::namespaced_widget_id;
    use quadraui::{Decoration, ListItem, ListView, StyledText, WidgetId};
    ListView {
        // #1631 review: namespaced like every other plugin-owned id (#146
        // invariant 4) — a view-scoped body widget, not a shared
        // long-lived controller like `Engine::plugin_view_tree_controller`
        // (whose own fixed id names the *host slot*, not the currently
        // active view; see that field's construction for why it's
        // different). This `ListView` is rebuilt fresh every paint, so
        // there is no retained-identity reason to keep it un-namespaced.
        id: WidgetId::new(namespaced_widget_id(view, "list")),
        title: title.as_deref().map(StyledText::plain),
        items: items
            .iter()
            .map(|it| ListItem {
                text: StyledText::plain(&it.text),
                icon: None,
                detail: it.detail.as_deref().map(StyledText::plain),
                decoration: Decoration::default(),
            })
            .collect(),
        selected_idx: selected.min(items.len().saturating_sub(1)),
        scroll_offset: scroll_top,
        has_focus,
        bordered: false,
        h_scroll: 0,
        max_content_width: None,
        show_v_scrollbar: true,
    }
}

/// Paint a `ViewBody::List`-kind view into `rect` and cache the resolved
/// `Backend::list_layout` for [`route_plugin_view_list_click`]. Returns
/// `false` when `name` isn't a `List`-kind view.
pub(crate) fn paint_plugin_view_list(
    engine: &Engine,
    name: &str,
    host: PluginViewHost,
    has_focus: bool,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    let Some(ViewBody::List { title, items }) = plugin_view_body(engine, name) else {
        return false;
    };
    let selected = engine.plugin_view_selected(host);
    let scroll_top = engine.plugin_view_scroll_top(host);
    let list = plugin_view_to_list(name, title, items, selected, scroll_top, has_focus);
    backend.draw_list(rect, &list);
    let layout = backend.list_layout(rect, &list);
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_list_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_list_layout,
    };
    cache.replace(Some((rect, layout)));
    true
}

/// Resolve a click at `pos` (same units the last frame painted the list
/// into) against the cached layout, move the selection, and dispatch
/// `ItemSelected`/`ItemActivated` to the plugin. Returns `false` when there
/// is no cached layout, or the click missed every row.
pub(crate) fn route_plugin_view_list_click(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    pos: quadraui::Point,
    is_double_click: bool,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_list_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_list_layout,
    };
    let idx = {
        let borrowed = cache.borrow();
        let Some((rect, layout)) = borrowed.as_ref() else {
            return false;
        };
        match layout.hit_test(pos.x - rect.x, pos.y - rect.y) {
            quadraui::ListViewHit::Item(i) => Some(i),
            quadraui::ListViewHit::Title | quadraui::ListViewHit::Empty => None,
        }
    };
    let Some(idx) = idx else { return false };
    engine.set_plugin_view_selected(host, idx);
    engine.dispatch_plugin_view_event(PluginViewEvent {
        view: name.to_string(),
        widget_id: String::new(),
        kind: ViewEventKind::ItemSelected { index: idx },
    });
    if is_double_click {
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemActivated { index: idx },
        });
    }
    true
}

/// Keyboard navigation for a `List`/`Table` body — `j`/`Down`/`k`/`Up` move
/// the flat selection, `Enter` activates it. Shared by both kinds since both
/// reduce to "one selected index into a flat row count" (#1631). Uses the
/// same vimcode-internal key-name vocabulary as `Engine::handle_plugin_view_
/// key`'s field-stack navigation (`"Down"`/`"j"`/... — not `quadraui::Key`),
/// since this is called from that same function. Returns `false` when
/// `len == 0` (nothing to navigate) or the key isn't a navigation key.
fn navigate_flat_selection(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    len: usize,
    key: &str,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    if len == 0 {
        return false;
    }
    let sel = engine.plugin_view_selected(host).min(len - 1);
    let new_sel = match key {
        "Down" | "j" => Some((sel + 1).min(len - 1)),
        "Up" | "k" => Some(sel.saturating_sub(1)),
        "Home" | "g" => Some(0),
        "End" | "G" => Some(len - 1),
        _ => None,
    };
    if let Some(new_sel) = new_sel {
        engine.set_plugin_view_selected(host, new_sel);
        // #1631: without this, `Down`/`End`/`G` can walk `selected` out of
        // the visible scroll window on a list/table with more rows than fit
        // the viewport — see `Engine::plugin_view_ensure_visible`'s doc.
        engine.plugin_view_ensure_visible(host);
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemSelected { index: new_sel },
        });
        return true;
    }
    if matches!(key, "Return" | "Enter") {
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemActivated { index: sel },
        });
        return true;
    }
    false
}

/// Keyboard handling for a `ViewBody::List`-kind view. Returns `false` when
/// `name` isn't a `List`-kind view or the key wasn't consumed.
pub(crate) fn handle_plugin_view_list_key(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    key: &str,
) -> bool {
    let Some(ViewBody::List { items, .. }) = plugin_view_body(engine, name) else {
        return false;
    };
    let len = items.len();
    navigate_flat_selection(engine, name, host, len, key)
}

/// Scroll-wheel handling for a `ViewBody::List`-kind view: adjusts the
/// scroll offset (not the selection) by `delta` rows, clamped to
/// `[0, items.len())`. Returns `false` when `name` isn't a `List`-kind view.
pub(crate) fn scroll_plugin_view_flat_selection_list(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    delta: i32,
) -> bool {
    let Some(ViewBody::List { items, .. }) = plugin_view_body(engine, name) else {
        return false;
    };
    let len = items.len();
    let cur = engine.plugin_view_scroll_top(host) as i64;
    let new_top = (cur + delta as i64).clamp(0, len.saturating_sub(1) as i64) as usize;
    engine.set_plugin_view_scroll_top(host, new_top);
    true
}

// ── Tree ─────────────────────────────────────────────────────────────────

/// Recursively flatten `nodes` into quadraui's pre-flattened `TreeRow` shape.
/// A node is a "branch" (shows a chevron, `is_expanded: Some(_)`) iff it
/// declares children; a collapsed branch's children are simply omitted from
/// the flattened list (mirrors `TreeView`'s own "purely declarative" backend
/// contract — vimcode, not quadraui, decides which rows are visible).
fn flatten_tree_nodes(
    nodes: &[ViewTreeNode],
    depth: u16,
    path: &mut quadraui::TreePath,
) -> Vec<quadraui::TreeRow> {
    use quadraui::{StyledText, TreeRow};
    let mut out = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        path.push(i as u16);
        let is_branch = node.is_branch();
        out.push(TreeRow {
            path: path.clone(),
            indent: depth,
            icon: None,
            text: StyledText::plain(&node.label),
            badge: None,
            is_expanded: is_branch.then_some(node.expanded),
            decoration: quadraui::Decoration::default(),
            edit: None,
        });
        if is_branch && node.expanded {
            out.extend(flatten_tree_nodes(&node.children, depth + 1, path));
        }
        path.pop();
    }
    out
}

/// Populate the shared `TreeController` (sidebar or tab, per `host`) from
/// the named view's `ViewBody::Tree`. Returns `false` when `name` isn't a
/// `Tree`-kind view. Mirrors `populate_explorer_tree_controller`/
/// `populate_plugin_view_form_controller`.
pub(crate) fn populate_plugin_view_tree_controller(
    engine: &Engine,
    name: &str,
    host: PluginViewHost,
    has_focus: bool,
) -> bool {
    let Some(ViewBody::Tree { nodes }) = plugin_view_body(engine, name) else {
        return false;
    };
    let controller = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_tree_controller,
        PluginViewHost::Tab => &engine.plugin_view_tab_tree_controller,
    };
    let mut tc = controller.borrow_mut();
    tc.set_has_focus(has_focus);
    tc.set_rows(flatten_tree_nodes(nodes, 0, &mut Vec::new()));
    true
}

/// Route an event over a `ViewBody::Tree`-kind view through its
/// `TreeController` and dispatch the resulting semantic action to the
/// plugin. Mirrors `route_explorer_tree_event`'s shape (a `TreeController`
/// needs a live `&mut dyn Backend` to `handle`, unlike `FormController`'s
/// cache-then-read split). Returns `false` when `name` isn't a `Tree`-kind
/// view.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_plugin_view_tree_event(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    has_focus: bool,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    if !populate_plugin_view_tree_controller(engine, name, host, has_focus) {
        return false;
    }
    let controller = match host {
        PluginViewHost::Sidebar => engine.plugin_view_tree_controller.clone(),
        PluginViewHost::Tab => engine.plugin_view_tab_tree_controller.clone(),
    };
    let tree_event = controller.borrow_mut().handle(event, backend, rect);

    let node_id = |path: &quadraui::TreePath| -> Option<String> {
        let ViewBody::Tree { nodes } = plugin_view_body(engine, name)? else {
            return None;
        };
        ViewTreeNode::resolve(nodes, path).map(|n| n.id.clone())
    };

    match tree_event {
        quadraui::TreeControllerEvent::RowSelected { path } => {
            let Some(id) = node_id(&path) else {
                return true;
            };
            engine.dispatch_plugin_view_event(PluginViewEvent {
                view: name.to_string(),
                widget_id: String::new(),
                kind: ViewEventKind::NodeSelected { id },
            });
            true
        }
        quadraui::TreeControllerEvent::RowActivated { path } => {
            let Some(id) = node_id(&path) else {
                return true;
            };
            engine.dispatch_plugin_view_event(PluginViewEvent {
                view: name.to_string(),
                widget_id: String::new(),
                kind: ViewEventKind::NodeActivated { id },
            });
            true
        }
        quadraui::TreeControllerEvent::RowToggleExpand { path } => {
            let Some(ViewBody::Tree { nodes }) = plugin_view_body(engine, name) else {
                return true;
            };
            let Some(node) = ViewTreeNode::resolve(nodes, &path) else {
                return true;
            };
            let kind = if node.expanded {
                ViewEventKind::Collapsed {
                    id: node.id.clone(),
                }
            } else {
                ViewEventKind::Expanded {
                    id: node.id.clone(),
                }
            };
            engine.dispatch_plugin_view_event(PluginViewEvent {
                view: name.to_string(),
                widget_id: String::new(),
                kind,
            });
            true
        }
        quadraui::TreeControllerEvent::ScrollChanged | quadraui::TreeControllerEvent::Consumed => {
            true
        }
        quadraui::TreeControllerEvent::Ignored => false,
        _ => true,
    }
}

/// Keyboard handling for a `ViewBody::Tree`-kind view, entirely backend-free
/// (unlike [`route_plugin_view_tree_event`], which needs a live
/// `&mut dyn Backend` for `TreeController::handle`'s mouse-oriented API) —
/// this is what lets it be reached from `Engine::handle_plugin_view_key`,
/// which (like every `core/engine` function) has no backend reference to
/// hand it. Walks `TreeController::rows()` directly: `j`/`Down`/`k`/`Up`
/// move the flat selection (dispatching `NodeSelected`), `Enter`/`Space`
/// toggles a branch's expand state or activates a leaf.
pub(crate) fn handle_plugin_view_tree_key(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    key: &str,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};

    if !populate_plugin_view_tree_controller(engine, name, host, true) {
        return false;
    }
    let controller = match host {
        PluginViewHost::Sidebar => engine.plugin_view_tree_controller.clone(),
        PluginViewHost::Tab => engine.plugin_view_tab_tree_controller.clone(),
    };
    let (rows, selected_path) = {
        let tc = controller.borrow();
        (tc.rows().to_vec(), tc.selected_path().cloned())
    };
    if rows.is_empty() {
        return false;
    }
    let cur = selected_path
        .and_then(|p| rows.iter().position(|r| r.path == p))
        .unwrap_or(0);

    let new_idx = match key {
        "Down" | "j" => Some((cur + 1).min(rows.len() - 1)),
        "Up" | "k" => Some(cur.saturating_sub(1)),
        "Home" | "g" => Some(0),
        "End" | "G" => Some(rows.len() - 1),
        _ => None,
    };
    if let Some(new_idx) = new_idx {
        let path = rows[new_idx].path.clone();
        // #1631: fixed 20-row viewport guess, same fallback
        // `Engine::plugin_view_ensure_visible`/`ext_panel_ensure_visible`
        // use for the List/Table/field-stack cases — without this a
        // `Down`/`End`/`G` press can walk the selection out of the visible
        // scroll window with nothing to compensate.
        let mut tc = controller.borrow_mut();
        tc.set_selected_path(Some(path.clone()));
        tc.scroll_to_visible(new_idx, 20);
        drop(tc);
        let Some(ViewBody::Tree { nodes }) = plugin_view_body(engine, name) else {
            return true;
        };
        let Some(id) = ViewTreeNode::resolve(nodes, &path).map(|n| n.id.clone()) else {
            return true;
        };
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::NodeSelected { id },
        });
        return true;
    }

    if matches!(key, "Return" | "Enter" | "Space" | " ") {
        let row = &rows[cur];
        let Some(ViewBody::Tree { nodes }) = plugin_view_body(engine, name) else {
            return true;
        };
        let Some(node) = ViewTreeNode::resolve(nodes, &row.path) else {
            return true;
        };
        let kind = match row.is_expanded {
            Some(true) => ViewEventKind::Collapsed {
                id: node.id.clone(),
            },
            Some(false) => ViewEventKind::Expanded {
                id: node.id.clone(),
            },
            None => ViewEventKind::NodeActivated {
                id: node.id.clone(),
            },
        };
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind,
        });
        return true;
    }
    false
}

// ── Table ────────────────────────────────────────────────────────────────

fn plugin_view_to_table(
    view: &str,
    columns: &[ViewTableColumn],
    rows: &[ViewTableRow],
    selected: usize,
    scroll_top: usize,
    has_focus: bool,
    cell_edit: Option<&crate::core::plugin_ui::PluginViewTextEditState>,
) -> quadraui::DataTable {
    use crate::core::plugin_ui::namespaced_widget_id;
    use quadraui::{Column, ColumnWidth, DataRow, DataTable, Decoration, StyledText, WidgetId};
    let q_columns: Vec<Column> = columns
        .iter()
        .map(|c| Column {
            title: c.title.clone(),
            width: ColumnWidth::Flex(1.0),
            align: quadraui::ColumnAlign::Left,
        })
        .collect();
    let q_rows: Vec<DataRow> = rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| DataRow {
            cells: row
                .cells
                .iter()
                .enumerate()
                .map(|(col_idx, cell)| {
                    // A live edit in progress on this exact cell paints the
                    // uncommitted buffer instead of the plugin's declared
                    // value — same "live overrides declared" rule
                    // `plugin_view_to_form`'s `Text` arm uses (#1627).
                    match cell_edit.filter(|e| e.field_id == table_cell_field_id(row_idx, col_idx))
                    {
                        Some(e) => StyledText::plain(&e.value),
                        None => StyledText::plain(cell),
                    }
                })
                .collect(),
            decoration: Decoration::default(),
        })
        .collect();
    DataTable {
        // #1631 review: see `plugin_view_to_list`'s matching comment —
        // rebuilt fresh every paint, so it's namespaced like every other
        // plugin-owned id rather than the shared controllers.
        id: WidgetId::new(namespaced_widget_id(view, "table")),
        columns: q_columns,
        rows: q_rows,
        selected_idx: (!rows.is_empty()).then(|| selected.min(rows.len().saturating_sub(1))),
        scroll_offset: scroll_top,
        sort: None,
        has_focus,
        show_scrollbar: true,
        min_total_width: None,
        h_scroll: 0.0,
        column_overrides: Vec::new(),
        footer: None,
    }
}

/// Paint a `ViewBody::Table`-kind view into `rect` and cache the resolved
/// `Backend::data_table_layout` for [`route_plugin_view_table_click`].
/// Returns `false` when `name` isn't a `Table`-kind view.
pub(crate) fn paint_plugin_view_table(
    engine: &Engine,
    name: &str,
    host: PluginViewHost,
    has_focus: bool,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    let Some(ViewBody::Table { columns, rows }) = plugin_view_body(engine, name) else {
        return false;
    };
    let selected = engine.plugin_view_selected(host);
    let scroll_top = engine.plugin_view_scroll_top(host);
    let cell_edit = engine
        .plugin_view_text_edit
        .as_ref()
        .filter(|e| e.view == name);
    let table = plugin_view_to_table(
        name, columns, rows, selected, scroll_top, has_focus, cell_edit,
    );
    let layout = backend.draw_data_table(rect, &table, None);
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_table_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_table_layout,
    };
    cache.replace(Some((rect, layout)));
    true
}

/// Resolve a click at `pos` against the cached table layout, move the
/// selection, and dispatch `ItemSelected`/`ItemActivated`. Returns `false`
/// when there is no cached layout or the click missed a row.
pub(crate) fn route_plugin_view_table_click(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    pos: quadraui::Point,
    is_double_click: bool,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_table_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_table_layout,
    };
    let Some(ViewBody::Table { columns, rows }) = plugin_view_body(engine, name) else {
        return false;
    };
    let total_rows = rows.len();
    let scroll_offset = engine.plugin_view_scroll_top(host);
    // #1631 review: `DataTableHit` (the pinned quadraui rev's hit-test
    // result) only ever reports `Row { idx }` — no column component — so a
    // click's column is resolved separately here from the same per-column
    // `x`/`width` (`ResolvedColumn`) the layout already carries for
    // painting, rather than a hand-rolled backend-specific hit-test. Only
    // an `editable` column is worth recording; a click on a read-only
    // column leaves the last editable selection alone rather than clearing
    // it to a column `Enter` could never open anyway.
    let (idx, col) = {
        let borrowed = cache.borrow();
        let Some((rect, layout)) = borrowed.as_ref() else {
            return false;
        };
        let idx = match layout.hit_test(pos.x - rect.x, pos.y - rect.y, scroll_offset, total_rows) {
            quadraui::DataTableHit::Row { idx } => Some(idx),
            _ => None,
        };
        let local_x = pos.x - rect.x;
        let col = layout
            .columns
            .iter()
            .position(|c| local_x >= c.x && local_x < c.x + c.width)
            .filter(|&c| columns.get(c).is_some_and(|c| c.editable));
        (idx, col)
    };
    let Some(idx) = idx else { return false };
    engine.set_plugin_view_selected(host, idx);
    if let Some(col) = col {
        engine.set_plugin_view_table_col(host, col);
    }
    engine.dispatch_plugin_view_event(PluginViewEvent {
        view: name.to_string(),
        widget_id: String::new(),
        kind: ViewEventKind::ItemSelected { index: idx },
    });
    if is_double_click {
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemActivated { index: idx },
        });
    }
    true
}

/// Keyboard handling for a `ViewBody::Table`-kind view (#1631). Uses the
/// same vimcode-internal key-name vocabulary as `Engine::handle_plugin_view_
/// key`/`handle_plugin_view_text_key` (`"Escape"`, `"BackSpace"`, ... — not
/// `quadraui::Key`), since it is called from the former.
///
/// Not editing: `j`/`k`/Up/Down move the row selection, `Left`/`Right` move
/// the *column* selection among the row's editable columns (when there is
/// more than one — e.g. #147's "Key"/"Value" pair — so a second `editable`
/// column is actually reachable, not just accepted by the vocabulary), and
/// `Enter` on a row with at least one editable column starts editing the
/// currently-selected one (reusing `Engine::plugin_view_text_edit`, #1627's
/// model, keyed by [`table_cell_field_id`]) — a row with no editable column
/// instead emits `ItemActivated`.
///
/// Editing: character keys insert, `BackSpace`/`Left`/`Right` edit/move the
/// cursor, `Enter` commits (`CellEdited`) and ends editing, `Escape` cancels
/// without emitting anything.
///
/// Returns `false` when `name` isn't a `Table`-kind view or the key wasn't
/// consumed.
pub(crate) fn handle_plugin_view_table_key(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    key: &str,
    unicode: Option<char>,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, PluginViewTextEditState, ViewEventKind};

    let Some(ViewBody::Table { columns, rows }) = plugin_view_body(engine, name) else {
        return false;
    };
    // Snapshot everything the non-editing branches need as owned values so
    // the borrow of `engine.plugin_views` ends here — the branches below
    // call `engine.dispatch_plugin_view_event`/`navigate_flat_selection`,
    // which need `&mut Engine`.
    let rows_len = rows.len();
    // Every column marked `editable`, in declaration order — not just the
    // first one (#1631 review: a table author who marks two columns
    // editable could previously never reach the second).
    let editable_cols: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.editable)
        .map(|(i, _)| i)
        .collect();
    // The engine's stored column selection, clamped to the nearest actually-
    // editable column — a stale value (a `ui.refresh` that dropped a column,
    // or the field's untouched `0` default when column 0 isn't editable)
    // never panics or points at a non-editable column, it just falls back to
    // `editable_cols[0]`.
    let current_col = |engine: &Engine| -> Option<usize> {
        let raw = engine.plugin_view_table_col(host);
        if editable_cols.contains(&raw) {
            Some(raw)
        } else {
            editable_cols.first().copied()
        }
    };
    let cell_value = |sel: usize, col: usize| -> String {
        rows.get(sel)
            .and_then(|r| r.cells.get(col))
            .cloned()
            .unwrap_or_default()
    };
    let editable_col = current_col(engine);
    let cell_value = if let Some(col) = editable_col {
        let sel = engine
            .plugin_view_selected(host)
            .min(rows_len.saturating_sub(1));
        Some(cell_value(sel, col))
    } else {
        None
    };

    if let Some(edit) = engine.plugin_view_text_edit.clone() {
        if edit.view != name {
            return false;
        }
        let Some((row, col)) = crate::core::plugin_ui::parse_table_cell_field_id(&edit.field_id)
        else {
            return false;
        };
        match key {
            "Escape" => {
                engine.plugin_view_text_edit = None;
                true
            }
            "Return" | "Enter" => {
                engine.plugin_view_text_edit = None;
                engine.dispatch_plugin_view_event(PluginViewEvent {
                    view: name.to_string(),
                    widget_id: String::new(),
                    kind: ViewEventKind::CellEdited {
                        row,
                        col,
                        value: edit.value,
                    },
                });
                true
            }
            "BackSpace" => {
                let mut e = edit;
                if e.cursor > 0 {
                    let prev = quadraui::text_util::prev_char_boundary(&e.value, e.cursor);
                    e.value.replace_range(prev..e.cursor, "");
                    e.cursor = prev;
                }
                engine.plugin_view_text_edit = Some(e);
                true
            }
            "Left" => {
                let mut e = edit;
                e.cursor = quadraui::text_util::prev_char_boundary(&e.value, e.cursor);
                engine.plugin_view_text_edit = Some(e);
                true
            }
            "Right" => {
                let mut e = edit;
                e.cursor = quadraui::text_util::next_char_boundary(&e.value, e.cursor);
                engine.plugin_view_text_edit = Some(e);
                true
            }
            _ => {
                let Some(c) = unicode.filter(|c| !c.is_control()) else {
                    return false;
                };
                let mut e = edit;
                e.value.insert(e.cursor, c);
                e.cursor += c.len_utf8();
                engine.plugin_view_text_edit = Some(e);
                true
            }
        }
    } else if let (true, true, Some(col)) = (
        rows_len > 0,
        matches!(key, "Return" | "Enter"),
        editable_col,
    ) {
        // Checked *before* `navigate_flat_selection` (which also treats
        // `Enter` as `ItemActivated`) — a row with an editable column starts
        // editing on Enter instead of activating, and `navigate_flat_
        // selection`'s own `Enter` arm would otherwise always win first.
        // `rows_len > 0` guards a table with an editable column but zero
        // rows (#1631 review) — without it, `sel`'s `saturating_sub(1)`
        // clamp let Enter "edit" a row 0 that was never declared, which
        // could later dispatch `CellEdited { row: 0, .. }` for it.
        let sel = engine
            .plugin_view_selected(host)
            .min(rows_len.saturating_sub(1));
        let value = cell_value.unwrap_or_default();
        engine.plugin_view_text_edit = Some(PluginViewTextEditState {
            view: name.to_string(),
            field_id: table_cell_field_id(sel, col),
            cursor: value.len(),
            selection_anchor: None,
            value,
        });
        true
    } else if matches!(key, "Left" | "Right") && editable_cols.len() > 1 {
        // Column selection among the row's editable columns — checked
        // before `navigate_flat_selection` (which doesn't handle Left/Right
        // at all, so this can't shadow anything there). A single (or zero)
        // editable column has nothing to move between, so the key falls
        // through unconsumed rather than becoming a no-op `true`.
        let cur = editable_col.and_then(|c| editable_cols.iter().position(|&x| x == c));
        let cur = cur.unwrap_or(0);
        let next = if key == "Left" {
            cur.saturating_sub(1)
        } else {
            (cur + 1).min(editable_cols.len() - 1)
        };
        engine.set_plugin_view_table_col(host, editable_cols[next]);
        true
    } else {
        navigate_flat_selection(engine, name, host, rows_len, key)
    }
}

/// Scroll-wheel handling for a `ViewBody::Table`-kind view — the `Table`
/// twin of [`scroll_plugin_view_flat_selection_list`].
pub(crate) fn scroll_plugin_view_flat_selection_table(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    delta: i32,
) -> bool {
    let Some(ViewBody::Table { rows, .. }) = plugin_view_body(engine, name) else {
        return false;
    };
    let len = rows.len();
    let cur = engine.plugin_view_scroll_top(host) as i64;
    let new_top = (cur + delta as i64).clamp(0, len.saturating_sub(1) as i64) as usize;
    engine.set_plugin_view_scroll_top(host, new_top);
    true
}

// ── TextView ─────────────────────────────────────────────────────────────

fn plugin_view_to_text_display(
    view: &str,
    text: &str,
    scroll_top: usize,
    has_focus: bool,
) -> quadraui::TextDisplay {
    use crate::core::plugin_ui::namespaced_widget_id;
    use quadraui::{StyledSpan, TextDisplay, TextDisplayLine, WidgetId};
    let lines: Vec<TextDisplayLine> = text
        .split('\n')
        .map(|line| TextDisplayLine {
            spans: vec![StyledSpan::plain(line)],
            decoration: quadraui::Decoration::default(),
            timestamp: None,
        })
        .collect();
    TextDisplay {
        // #1631 review: see `plugin_view_to_list`'s matching comment.
        id: WidgetId::new(namespaced_widget_id(view, "text")),
        lines,
        scroll_offset: scroll_top,
        auto_scroll: false,
        max_lines: 0,
        has_focus,
        title: None,
        show_scrollbar: true,
    }
}

/// Paint a `ViewBody::TextView`-kind view into `rect`. Returns `false` when
/// `name` isn't a `TextView`-kind view.
pub(crate) fn paint_plugin_view_text(
    engine: &Engine,
    name: &str,
    host: PluginViewHost,
    has_focus: bool,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    let Some(ViewBody::TextView { text, .. }) = plugin_view_body(engine, name) else {
        return false;
    };
    let scroll_top = engine.plugin_view_scroll_top(host);
    let display = plugin_view_to_text_display(name, text, scroll_top, has_focus);
    backend.draw_text_display(rect, &display);
    true
}

/// Scroll a `ViewBody::TextView`-kind view by `delta` lines (positive =
/// down). Returns `false` when `name` isn't a `TextView`-kind view.
pub(crate) fn scroll_plugin_view_text(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    delta: i32,
) -> bool {
    let Some(ViewBody::TextView { text, .. }) = plugin_view_body(engine, name) else {
        return false;
    };
    let line_count = text.split('\n').count();
    let cur = engine.plugin_view_scroll_top(host) as i64;
    let new_top = (cur + delta as i64).clamp(0, line_count.saturating_sub(1) as i64) as usize;
    engine.set_plugin_view_scroll_top(host, new_top);
    true
}

/// Keyboard handling for a `ViewBody::TextView`-kind view: `j`/`Down`/`k`/
/// `Up` scroll by one line, `PageDown`/`PageUp` by `page` lines, `Home`/`End`
/// (`g`/`G`) jump to the top/bottom. Uses the same vimcode-internal key-name
/// vocabulary as `Engine::handle_plugin_view_key` (not `quadraui::Key`).
/// Returns `false` when `name` isn't a `TextView` or the key wasn't a scroll
/// key.
pub(crate) fn handle_plugin_view_text_key(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    key: &str,
    page: i32,
) -> bool {
    if plugin_view_body(engine, name).is_none() {
        return false;
    }
    match key {
        "Down" | "j" => scroll_plugin_view_text(engine, name, host, 1),
        "Up" | "k" => scroll_plugin_view_text(engine, name, host, -1),
        "PageDown" | "Page_Down" => scroll_plugin_view_text(engine, name, host, page.max(1)),
        "PageUp" | "Page_Up" => scroll_plugin_view_text(engine, name, host, -page.max(1)),
        "Home" | "g" => scroll_plugin_view_text(engine, name, host, i32::MIN),
        "End" | "G" => scroll_plugin_view_text(engine, name, host, i32::MAX),
        _ => false,
    }
}
