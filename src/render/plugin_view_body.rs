use super::*;

// ─── ViewBody: List / Tree / Table / TextView ───────────────────────────────
//
// A `ViewBody`-kind view paints through the matching quadraui primitive
// directly — `ListView`, `TreeView` (via `TreeController`), `DataTable`, or
// `TextDisplay` — one sibling translation per kind, rather than wedging four
// different widget shapes into `Form`'s flat field rows. `List`/`Table` have no
// composed quadraui controller (unlike `Tree`'s `TreeController`), so selection/
// scroll interaction for those two is hand-rolled here — in this backend-neutral
// module — following the "paint caches a `Backend::*_layout`, a click reads it
// back" split already established for the same reason.
//
// Selection/scroll reuse `ext_panel_selected`/`ext_panel_scroll_top` (sidebar)
// and `plugin_view_tab_selected`/`plugin_view_tab_scroll_top` (tab) verbatim
// instead of new per-body-kind fields — a view's body is one kind at a time,
// so there is never a collision, and it keeps `Engine`'s field count from
// growing by one pair per widget kind.

use crate::core::engine::{PluginTableColumnDrag, PluginViewHost};
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
        // Namespaced like every other plugin-owned id — a view-scoped body widget,
        // not a shared long-lived controller. This `ListView` is rebuilt fresh every
        // paint, so there is no retained-identity reason to keep it un-namespaced.
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
        // List-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
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
        // List-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
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
        kind: ViewEventKind::ItemSelected {
            index: idx,
            col: None,
        },
    });
    if is_double_click {
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemActivated {
                index: idx,
                col: None,
            },
        });
    }
    true
}

/// Keyboard navigation for a `List`/`Table` body — `j`/`Down`/`k`/`Up` move
/// the flat selection, `Enter` activates it. Shared by both kinds since both
/// reduce to "one selected index into a flat row count". Uses the
/// same vimcode-internal key-name vocabulary as `Engine::handle_plugin_view_
/// key`'s field-stack navigation (`"Down"`/`"j"`/... — not `quadraui::Key`),
/// since this is called from that same function. Returns `false` when
/// `len == 0` (nothing to navigate) or the key isn't a navigation key.
///
/// `row_order` maps a display position to the index the plugin declared the
/// row at, for a body whose painted order can differ from its declared one (a
/// sorted `Table`); `None` means the two coincide. The *selection* stays in
/// display space — so `j`/`k` always walk the rows the user can see — and only
/// the index carried by the dispatched event is translated back, since that is
/// what the plugin indexes its own data with.
fn navigate_flat_selection(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    len: usize,
    key: &str,
    row_order: Option<&[usize]>,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    if len == 0 {
        return false;
    }
    let declared = |display: usize| -> usize {
        row_order.map_or(display, |o| o.get(display).copied().unwrap_or(display))
    };
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
        // Without this, `Down`/`End`/`G` can walk `selected` out of
        // the visible scroll window on a list/table with more rows than fit
        // the viewport — see `Engine::plugin_view_ensure_visible`'s doc.
        engine.plugin_view_ensure_visible(host);
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemSelected {
                index: declared(new_sel),
                col: None,
            },
        });
        return true;
    }
    if matches!(key, "Return" | "Enter") {
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemActivated {
                index: declared(sel),
                col: None,
            },
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
    navigate_flat_selection(engine, name, host, len, key, None)
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
        // Tree-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
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
        // Tree-kind Dock support isn't implemented; the
        // `populate_plugin_view_tree_controller` call above already bailed
        // out for `Dock`, so this arm is unreachable in practice.
        PluginViewHost::Dock => return false,
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
        // Tree-kind Dock support isn't implemented; the
        // `populate_plugin_view_tree_controller` call above already bailed
        // out for `Dock`, so this arm is unreachable in practice.
        PluginViewHost::Dock => return false,
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
        // Fixed 20-row viewport guess, same fallback
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

/// The sort a `ViewBody::Table`-kind view is currently painted in, or `None`
/// for the plugin's declared row order. A stored column that a later
/// `ui.refresh` dropped is treated as "unsorted" rather than clamped onto a
/// neighbouring column the user never clicked.
fn active_table_sort(
    engine: &Engine,
    name: &str,
    column_count: usize,
) -> Option<(usize, quadraui::SortDirection)> {
    engine
        .plugin_view_table_sort
        .get(name)
        .copied()
        .filter(|&(col, _)| col < column_count)
}

/// A table's stored column-width overrides, or an empty slice when they are
/// stale.
///
/// Overrides are absolute widths held positionally, so they only describe the
/// column set they were dragged against: a plugin whose `render` returns a
/// different number of columns would otherwise have the old widths applied to
/// whichever columns happen to share those indices. Mirrors
/// [`active_table_sort`]'s own staleness filter.
fn active_table_col_overrides<'a>(
    engine: &'a Engine,
    name: &str,
    column_count: usize,
) -> &'a [Option<f32>] {
    engine
        .plugin_view_table_col_overrides
        .get(name)
        .map(Vec::as_slice)
        .filter(|o| o.len() == column_count)
        .unwrap_or(&[])
}

/// Display order for a table's rows: display position → index into the
/// plugin's declared `rows`. Identity when `sort` is `None`.
///
/// vimcode orders the rows itself because `quadraui::DataTable::sort` is
/// indicator-only — quadraui appends the direction glyph to the sorted
/// column's header and leaves `rows` exactly as handed to it.
///
/// Two cells that both parse as numbers compare numerically (so `"9"` sorts
/// before `"12"`, not after it); anything else compares case-insensitively.
/// The sort is stable, so rows with equal keys keep their declared order.
fn plugin_view_table_row_order(
    rows: &[ViewTableRow],
    sort: Option<(usize, quadraui::SortDirection)>,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    let Some((col, dir)) = sort else {
        return order;
    };
    let key = |idx: usize| -> &str {
        rows.get(idx)
            .and_then(|r| r.cells.get(col))
            .map(String::as_str)
            .unwrap_or("")
    };
    order.sort_by(|&a, &b| {
        let (ka, kb) = (key(a), key(b));
        let ord = match (ka.trim().parse::<f64>(), kb.trim().parse::<f64>()) {
            (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
            // Compare lowercased chars pairwise instead of `to_lowercase()`ing
            // both whole strings: this runs once per comparator call during a
            // sort, so an allocation per call here would add up on a large
            // table.
            _ => ka
                .chars()
                .flat_map(char::to_lowercase)
                .cmp(kb.chars().flat_map(char::to_lowercase)),
        };
        match dir {
            quadraui::SortDirection::Ascending => ord,
            quadraui::SortDirection::Descending => ord.reverse(),
        }
    });
    order
}

/// The sort a header click on `col` moves to, cycling
/// unsorted → ascending → descending → unsorted. Clicking a *different*
/// column starts that column's own cycle at ascending.
fn next_table_sort(
    current: Option<(usize, quadraui::SortDirection)>,
    col: usize,
) -> Option<(usize, quadraui::SortDirection)> {
    match current {
        Some((c, quadraui::SortDirection::Ascending)) if c == col => {
            Some((col, quadraui::SortDirection::Descending))
        }
        Some((c, quadraui::SortDirection::Descending)) if c == col => None,
        _ => Some((col, quadraui::SortDirection::Ascending)),
    }
}

/// A declared `width` fixes the column; a declared `min_width` with no
/// `width` sizes the column to its content, floored at `min_width`; neither
/// leaves the column proportional, matching every column declared before
/// these fields existed.
fn plugin_table_column_width(column: &ViewTableColumn) -> quadraui::ColumnWidth {
    match (column.width, column.min_width) {
        (Some(w), _) => quadraui::ColumnWidth::Fixed(w),
        (None, Some(min)) => quadraui::ColumnWidth::Content { min, max: f32::MAX },
        (None, None) => quadraui::ColumnWidth::Flex(1.0),
    }
}

/// Checkbox glyph prefixed onto column 0's cell text for a `multi_select:
/// true` table — the same `\u{2610}`/`\u{2611}` convention
/// `ai_chat_and_misc_builders`'s plan-entry glyph uses, chosen because
/// `quadraui::DataTable` has no native multi-row highlight to key a
/// "selected" decoration off of.
fn multi_select_prefix(declared_idx: usize, multi_selected: &[usize]) -> &'static str {
    if multi_selected.contains(&declared_idx) {
        "\u{2611} " // ☑
    } else {
        "\u{2610} " // ☐
    }
}

#[allow(clippy::too_many_arguments)]
fn plugin_view_to_table(
    view: &str,
    columns: &[ViewTableColumn],
    rows: &[ViewTableRow],
    selected: usize,
    scroll_top: usize,
    has_focus: bool,
    cell_edit: Option<&crate::core::plugin_ui::PluginViewTextEditState>,
    column_overrides: &[Option<f32>],
    sort: Option<(usize, quadraui::SortDirection)>,
    row_order: &[usize],
    multi_select: bool,
    multi_selected: &[usize],
    footer: Option<&[String]>,
) -> quadraui::DataTable {
    use crate::core::plugin_ui::namespaced_widget_id;
    use quadraui::{Column, ColumnAlign, DataRow, DataTable, Decoration, StyledText, WidgetId};
    let q_columns: Vec<Column> = columns
        .iter()
        .map(|c| Column {
            title: c.title.clone(),
            width: plugin_table_column_width(c),
            align: match c.align {
                crate::core::plugin_ui::TableColumnAlign::Left => ColumnAlign::Left,
                crate::core::plugin_ui::TableColumnAlign::Center => ColumnAlign::Center,
                crate::core::plugin_ui::TableColumnAlign::Right => ColumnAlign::Right,
            },
        })
        .collect();
    // Painted in display order; `row_idx` stays the *declared* index, so a
    // live cell edit keyed by `table_cell_field_id` still lands on its own
    // row under a sort.
    let q_rows: Vec<DataRow> = row_order
        .iter()
        .filter_map(|&row_idx| rows.get(row_idx).map(|row| (row_idx, row)))
        .map(|(row_idx, row)| DataRow {
            cells: row
                .cells
                .iter()
                .enumerate()
                .map(|(col_idx, cell)| {
                    // A live edit in progress on this exact cell paints the
                    // uncommitted buffer instead of the plugin's declared
                    // value — same "live overrides declared" rule
                    // `plugin_view_to_form`'s `Text` arm uses.
                    let text = match cell_edit
                        .filter(|e| e.field_id == table_cell_field_id(row_idx, col_idx))
                    {
                        Some(e) => e.value.clone(),
                        None => cell.clone(),
                    };
                    if multi_select && col_idx == 0 {
                        StyledText::plain(format!(
                            "{}{}",
                            multi_select_prefix(row_idx, multi_selected),
                            text
                        ))
                    } else {
                        StyledText::plain(text)
                    }
                })
                .collect(),
            decoration: Decoration::default(),
        })
        .collect();
    let q_footer = footer.map(|cells| DataRow {
        cells: cells.iter().map(|c| StyledText::plain(c.clone())).collect(),
        decoration: Decoration::default(),
    });
    DataTable {
        // See `plugin_view_to_list`'s matching comment —
        // rebuilt fresh every paint, so it's namespaced like every other
        // plugin-owned id rather than the shared controllers.
        id: WidgetId::new(namespaced_widget_id(view, "table")),
        columns: q_columns,
        rows: q_rows,
        selected_idx: (!rows.is_empty()).then(|| selected.min(rows.len().saturating_sub(1))),
        scroll_offset: scroll_top,
        sort,
        has_focus,
        show_scrollbar: true,
        min_total_width: None,
        h_scroll: 0.0,
        column_overrides: column_overrides.to_vec(),
        footer: q_footer,
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
    let Some(ViewBody::Table {
        columns,
        rows,
        multi_select,
        footer,
    }) = plugin_view_body(engine, name)
    else {
        return false;
    };
    let selected = engine.plugin_view_selected(host);
    let scroll_top = engine.plugin_view_scroll_top(host);
    let cell_edit = engine
        .plugin_view_text_edit
        .as_ref()
        .filter(|e| e.view == name);
    let column_overrides = active_table_col_overrides(engine, name, columns.len());
    let sort = active_table_sort(engine, name, columns.len());
    let row_order = plugin_view_table_row_order(rows, sort);
    // Filtered to the current `rows.len()` so a selection made before a
    // `ui.refresh` shrank the row count can't paint a `☑` on a declared
    // index outside the current row count.
    let multi_selected: Vec<usize> = engine
        .plugin_view_table_multi_selected
        .get(name)
        .map(|v| v.iter().copied().filter(|&i| i < rows.len()).collect())
        .unwrap_or_default();
    let table = plugin_view_to_table(
        name,
        columns,
        rows,
        selected,
        scroll_top,
        has_focus,
        cell_edit,
        column_overrides,
        sort,
        &row_order,
        *multi_select,
        &multi_selected,
        footer.as_deref(),
    );
    let layout = backend.draw_data_table(rect, &table, None);
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_table_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_table_layout,
        // Table-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
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
    modifiers: quadraui::Modifiers,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_table_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_table_layout,
        // Table-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
    };
    let Some(ViewBody::Table {
        columns,
        rows,
        multi_select,
        ..
    }) = plugin_view_body(engine, name)
    else {
        return false;
    };
    let multi_select = *multi_select;
    let total_rows = rows.len();
    let scroll_offset = engine.plugin_view_scroll_top(host);
    // `DataTableHit` (the pinned quadraui rev's hit-test
    // result) only ever reports `Row { idx }` — no column component — so a
    // click's column is resolved separately here from the same per-column
    // `x`/`width` (`ResolvedColumn`) the layout already carries for
    // painting, rather than a hand-rolled backend-specific hit-test.
    // `clicked_col` is unfiltered — it reports whatever column the pointer
    // landed in, read-only or not, for the dispatched event. `editable_col`
    // narrows that to only an `editable` column, since that is the one
    // thing worth arming a cell edit on: a click on a read-only column
    // leaves the last editable selection alone rather than clearing it to a
    // column `Enter` could never open anyway.
    let (idx, clicked_col, editable_col, divider, header, baseline) = {
        let borrowed = cache.borrow();
        let Some((rect, layout)) = borrowed.as_ref() else {
            return false;
        };
        let hit = layout.hit_test(pos.x - rect.x, pos.y - rect.y, scroll_offset, total_rows);
        let idx = match hit {
            quadraui::DataTableHit::Row { idx } => Some(idx),
            _ => None,
        };
        let header = match hit {
            quadraui::DataTableHit::Header { col } => Some(col),
            _ => None,
        };
        // A `HeaderDivider` hit takes priority over `Header`/row hits in the
        // same zone (quadraui's own `hit_test` already checks dividers
        // first) — a non-double-click divider hit arms a resize drag instead
        // of touching selection.
        let divider = match hit {
            quadraui::DataTableHit::HeaderDivider { col } if !is_double_click => Some(col),
            _ => None,
        };
        let local_x = pos.x - rect.x;
        let clicked_col = layout
            .columns
            .iter()
            .position(|c| local_x >= c.x && local_x < c.x + c.width);
        let editable_col = clicked_col.filter(|&c| columns.get(c).is_some_and(|c| c.editable));
        // Snapshotted only for a divider press, which is the only hit that
        // needs the pre-gesture geometry to survive the frames the drag
        // spans (`PluginTableColumnDrag::layout`).
        let baseline = divider.map(|_| (*rect, layout.clone()));
        (idx, clicked_col, editable_col, divider, header, baseline)
    };
    // Arm the resize drag and return before computing the sort cycle below:
    // `divider` and `header` come from mutually exclusive `DataTableHit`
    // arms, so a divider hit never needs the row reorder a header click
    // would, and there's no reason to pay for it.
    if let Some(col) = divider {
        let (rect, layout) = baseline.expect("a divider hit always snapshots its baseline");
        let column_count = columns.len();
        engine.plugin_view_table_resize_drag = Some(PluginTableColumnDrag {
            view: name.to_string(),
            host,
            col,
            rect,
            layout,
            overrides: active_table_col_overrides(engine, name, column_count).to_vec(),
        });
        return true;
    }
    let current_sort = active_table_sort(engine, name, columns.len());
    let row_order = plugin_view_table_row_order(rows, current_sort);
    // A header click cycles the sort for that column. Resolved while `rows` is
    // still borrowed so the selection can follow its row across the reorder:
    // selection is a display position, and the row the user had selected is at
    // a different one once the order changes.
    //
    // Every press on a header cycles, including the one a backend reports as
    // `DoubleClick` rather than `MouseDown` (the double-click detector
    // *replaces* the second rapid press). Two quick clicks on a header
    // therefore reach descending, exactly like two slow ones.
    let header_cycle = header.map(|col| {
        let next = next_table_sort(current_sort, col);
        let declared = row_order
            .get(
                engine
                    .plugin_view_selected(host)
                    .min(total_rows.saturating_sub(1)),
            )
            .copied()
            .unwrap_or(0);
        let next_order = plugin_view_table_row_order(rows, next);
        let next_sel = next_order.iter().position(|&d| d == declared).unwrap_or(0);
        (next, next_sel)
    });
    if let Some((next, next_sel)) = header_cycle {
        match next {
            Some(spec) => {
                engine.plugin_view_table_sort.insert(name.to_string(), spec);
            }
            None => {
                engine.plugin_view_table_sort.remove(name);
            }
        }
        engine.set_plugin_view_selected(host, next_sel);
        engine.plugin_view_ensure_visible(host);
        if let Some(col) = header {
            engine.dispatch_plugin_view_event(PluginViewEvent {
                view: name.to_string(),
                widget_id: String::new(),
                kind: ViewEventKind::HeaderClicked { col },
            });
        }
        return true;
    }
    let Some(idx) = idx else { return false };
    engine.set_plugin_view_selected(host, idx);
    if let Some(col) = editable_col {
        engine.set_plugin_view_table_col(host, col);
    }
    // The selection is a display position; the plugin indexes its own rows by
    // the order it declared them in, so that is what the event carries.
    let declared_idx = row_order.get(idx).copied().unwrap_or(idx);
    // Additive to (never a replacement for) the single-row cursor updated
    // just above: a `multi_select: true` table also tracks a selection set
    // keyed off click modifiers, dispatched as its own `SelectionChanged`
    // event alongside — not instead of — `ItemSelected` below. A double
    // click only ever activates a row; folding it through `Replace` here
    // would collapse a deliberately built multi-row selection the instant
    // before `ItemActivated` fires for it below.
    if multi_select && !is_double_click {
        let gesture = quadraui::SelectionGesture::from_modifiers(modifiers);
        // `anchor` is a *display* position (like `idx`, not `declared_idx`):
        // `Range` below has to measure contiguous rows the way the user sees
        // them on screen, which only matches declared order when no sort is
        // active. Anchoring on a position rather than on the anchored row's
        // identity means a header click between the two halves of a
        // shift-range re-points the anchor at whichever row now occupies that
        // position.
        //
        // Clamped to the current row count for the same reason `current`
        // below is filtered to it: the plugin can `ui.refresh` a shorter row
        // set between the anchoring click and the shift-click, and a stale
        // display position past the end would otherwise pass straight through
        // `row_order.get(..).unwrap_or(d)` as a declared index the plugin's
        // own rows have no entry for.
        let anchor = (*engine
            .plugin_view_table_selection_anchor
            .get(name)
            .unwrap_or(&idx))
        .min(total_rows.saturating_sub(1));
        let next = match gesture {
            quadraui::SelectionGesture::Range => {
                // Range math runs entirely in display space (`anchor..=idx`,
                // or `idx..=anchor`), then each display position is mapped
                // through `row_order` back to the declared index the plugin
                // sees — the same display->declared conversion `declared_idx`
                // above uses for the single-row cursor. `current` plays no
                // part in `Range`'s own result (quadraui replaces the set
                // with the contiguous span), so it's never built in display
                // space.
                let display_range = quadraui::apply_selection_gesture(&[], anchor, idx, gesture);
                let mut declared: Vec<usize> = display_range
                    .iter()
                    .map(|&d| row_order.get(d).copied().unwrap_or(d))
                    .collect();
                declared.sort_unstable();
                declared.dedup();
                declared
            }
            _ => {
                // Filtered to `total_rows` for the same reason the paint
                // path filters it: a stale entry from before a row-count
                // shrink can't be toggled off onto a row index outside the
                // current row count.
                let current: Vec<usize> = engine
                    .plugin_view_table_multi_selected
                    .get(name)
                    .map(|v| v.iter().copied().filter(|&i| i < total_rows).collect())
                    .unwrap_or_default();
                // `Toggle`/`Replace` only ever compare/insert `declared_idx`
                // against `current` (itself declared-space) — `anchor` is
                // unused by those gestures, so no display/declared
                // conversion is needed here.
                quadraui::apply_selection_gesture(&current, anchor, declared_idx, gesture)
            }
        };
        if !matches!(gesture, quadraui::SelectionGesture::Range) {
            engine
                .plugin_view_table_selection_anchor
                .insert(name.to_string(), idx);
        }
        engine
            .plugin_view_table_multi_selected
            .insert(name.to_string(), next.clone());
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::SelectionChanged { indices: next },
        });
    }
    engine.dispatch_plugin_view_event(PluginViewEvent {
        view: name.to_string(),
        widget_id: String::new(),
        kind: ViewEventKind::ItemSelected {
            index: declared_idx,
            col: clicked_col,
        },
    });
    if is_double_click {
        engine.dispatch_plugin_view_event(PluginViewEvent {
            view: name.to_string(),
            widget_id: String::new(),
            kind: ViewEventKind::ItemActivated {
                index: declared_idx,
                col: clicked_col,
            },
        });
    }
    true
}

/// Narrowest a plugin-view table column may be dragged to, in character
/// widths (converted to the backend's own unit by [`min_table_column_width`]).
const MIN_TABLE_COLUMN_WIDTH_CHARS: f32 = 3.0;

/// Floor a dragged (or the rightmost, slack-absorbing) plugin-view table
/// column is clamped to. A pixel literal here would violate the
/// platform-neutrality rule (TUI columns are cell-wide, GTK columns are
/// pixel-wide), so the floor is derived from the backend's own character
/// metric (`quadraui::Backend::char_width`) at the call site instead of a
/// single shared constant.
fn min_table_column_width(backend: &dyn quadraui::Backend) -> f32 {
    backend.char_width() * MIN_TABLE_COLUMN_WIDTH_CHARS
}

/// Continue a header-divider drag armed by `route_plugin_view_table_click`'s
/// `HeaderDivider` branch, recomputing `Engine::plugin_view_table_col_overrides`
/// from the pointer's current position via `quadraui::DataTableLayout::
/// drag_divider`. Returns `false` when there is no drag in progress (so the
/// caller's dispatch falls through to its usual "unconsumed" handling) rather
/// than when the drag didn't move anything — a stationary pointer still
/// "consumes" the event so it doesn't fall through to e.g. text selection.
///
/// `name`/`host` identify the view/host the *caller* is currently routing an
/// event for; they are checked against the armed drag's own `view`/`host`
/// (see [`crate::core::engine::PluginTableColumnDrag`]) so a drag armed by one
/// view or host is never continued — or later cleared — by an unrelated one.
///
/// Every move resolves against the drag's own pre-gesture baseline rather
/// than the live layout cache, which `paint_plugin_view_table` rewrites each
/// frame — see [`crate::core::engine::PluginTableColumnDrag::layout`].
pub(crate) fn continue_plugin_view_table_resize_drag(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    pos: quadraui::Point,
    backend: &dyn quadraui::Backend,
) -> bool {
    let Some(drag) = engine.plugin_view_table_resize_drag.as_ref() else {
        return false;
    };
    if drag.view != name || drag.host != host {
        return false;
    }
    let view = drag.view.clone();
    let min_width = min_table_column_width(backend);
    // Clamp to the viewport so a column can never be dragged so wide that
    // its own divider ends up past `viewport_width` — quadraui's `hit_test`
    // early-returns `Empty` there, which would otherwise make the divider
    // permanently unreachable (and the override permanent) for the rest of
    // the session.
    let target_x = (pos.x - drag.rect.x).min(drag.layout.viewport_width - 1.0);
    let next = drag
        .layout
        .drag_divider(&drag.overrides, drag.col, target_x, min_width);
    engine.plugin_view_table_col_overrides.insert(view, next);
    true
}

/// End a header-divider drag on `MouseUp`. Returns `false` when there was no
/// drag in progress for `name`/`host` — see [`continue_plugin_view_table_resize_drag`]
/// for why the check matters.
pub(crate) fn end_plugin_view_table_resize_drag(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
) -> bool {
    let matches = matches!(
        engine.plugin_view_table_resize_drag.as_ref(),
        Some(drag) if drag.view == name && drag.host == host
    );
    if !matches {
        return false;
    }
    engine.plugin_view_table_resize_drag.take().is_some()
}

/// Keyboard handling for a `ViewBody::Table`-kind view. Uses the
/// same vimcode-internal key-name vocabulary as `Engine::handle_plugin_view_key`
/// (`"Escape"`, `"BackSpace"`, ... — not `quadraui::Key`), since it is called
/// from the former.
///
/// Not editing: `j`/`k`/Up/Down move the row selection, `Left`/`Right` move
/// the *column* selection among the row's editable columns (so a second editable
/// column is actually reachable), and `Enter` on a row with at least one editable
/// column starts editing the currently-selected one (keyed by [`table_cell_field_id`])
/// — a row with no editable column instead emits `ItemActivated`.
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

    let Some(ViewBody::Table { columns, rows, .. }) = plugin_view_body(engine, name) else {
        return false;
    };
    // Snapshot everything the non-editing branches need as owned values so
    // the borrow of `engine.plugin_views` ends here — the branches below
    // call `engine.dispatch_plugin_view_event`/`navigate_flat_selection`,
    // which need `&mut Engine`.
    let rows_len = rows.len();
    // Every column marked `editable`, in declaration order.
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
    // Display position → declared row index, so a sorted table edits and
    // reports the row the user can actually see.
    let row_order =
        plugin_view_table_row_order(rows, active_table_sort(engine, name, columns.len()));
    let declared_row =
        |display: usize| -> usize { row_order.get(display).copied().unwrap_or(display) };
    let cell_value = |sel: usize, col: usize| -> String {
        rows.get(declared_row(sel))
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
        // rows — without it, `sel`'s `saturating_sub(1)`
        // clamp let Enter "edit" a row 0 that was never declared, which
        // could later dispatch `CellEdited { row: 0, .. }` for it.
        let sel = engine
            .plugin_view_selected(host)
            .min(rows_len.saturating_sub(1));
        let value = cell_value.unwrap_or_default();
        engine.plugin_view_text_edit = Some(PluginViewTextEditState {
            view: name.to_string(),
            field_id: table_cell_field_id(declared_row(sel), col),
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
        navigate_flat_selection(engine, name, host, rows_len, key, Some(&row_order))
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
        // See `plugin_view_to_list`'s matching comment.
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

// ── Chart ────────────────────────────────────────────────────────────────
//
// `Chart` has no scroll or selection state of its own — a data point can be
// clicked but there is nothing to navigate — so unlike List/Table it needs
// no `Engine` selection/scroll plumbing, only the layout cache. It also
// paints and lays out in a *single* `Backend::draw_chart` call rather than
// List's `draw_list` + `list_layout` pair, and that call's returned
// `ChartLayout` is in the ABSOLUTE coordinate frame (shifted by `rect.x`/
// `rect.y` already — see `Backend::chart_layout`'s doc comment), the
// opposite of `ListViewLayout`'s LOCAL frame. So the click router below
// must *not* subtract `rect.x`/`rect.y` before calling `hit_test`.

fn plugin_view_to_chart(
    view: &str,
    chart_kind: crate::core::plugin_ui::ViewChartKind,
    series: &[crate::core::plugin_ui::ViewChartSeries],
    x_label: &Option<String>,
    y_label: &Option<String>,
    show_legend: bool,
) -> quadraui::Chart {
    use crate::core::plugin_ui::namespaced_widget_id;
    use quadraui::{Chart, Series, WidgetId};
    Chart {
        // Namespaced like every other plugin-owned id — see
        // `plugin_view_to_list`'s matching comment.
        id: WidgetId::new(namespaced_widget_id(view, "chart")),
        kind: chart_kind.into_quadraui(),
        series: series
            .iter()
            .map(|s| Series {
                label: s.label.clone(),
                data: s.data.clone(),
                // A malformed or absent hex string falls back to the
                // backend's default palette colour rather than failing the
                // whole chart's render.
                color: s.color.as_deref().and_then(super::primitives::try_from_hex),
                fill: s.fill,
            })
            .collect(),
        x_label: x_label.clone(),
        y_label: y_label.clone(),
        y_range: None,
        x_range: None,
        show_legend,
        y_ticks: None,
        x_ticks: None,
        show_grid: false,
    }
}

/// Paint a `ViewBody::Chart`-kind view into `rect` and cache the resolved
/// `ChartLayout` for [`route_plugin_view_chart_click`]. Returns `false`
/// when `name` isn't a `Chart`-kind view, or `host` is `Dock` (Chart-kind
/// Dock support isn't implemented, matching List/Tree/Table).
pub(crate) fn paint_plugin_view_chart(
    engine: &Engine,
    name: &str,
    host: PluginViewHost,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    let Some(ViewBody::Chart {
        chart_kind,
        series,
        x_label,
        y_label,
        show_legend,
    }) = plugin_view_body(engine, name)
    else {
        return false;
    };
    let chart = plugin_view_to_chart(name, *chart_kind, series, x_label, y_label, *show_legend);
    let layout = backend.draw_chart(rect, &chart, None, None);
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_chart_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_chart_layout,
        // Chart-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
    };
    cache.replace(Some(layout));
    true
}

/// Resolve a click at `pos` (same units the last frame painted the chart
/// into) against the cached layout and dispatch `DataPointClicked` or
/// `LegendClicked` to the plugin. Returns `false` when there is no cached
/// layout, or the click missed every data point and legend entry.
///
/// `pos` is passed through to `hit_test`/`nearest_point` unshifted —
/// `ChartLayout` is in the ABSOLUTE frame already (unlike `ListViewLayout`'s
/// LOCAL frame in [`route_plugin_view_list_click`], which subtracts
/// `rect.x`/`rect.y`).
///
/// A hit of `ChartHit::Body` means the click landed inside the plot area but
/// not directly on a hit region quadraui registers for a data point — true
/// for every chart kind, since `Chart::layout` only ever pushes one `Body`
/// region covering the whole plot plus one `Legend` region per series (no
/// `DataPoint` hit region is ever constructed). Resolving the actual nearest
/// point is this function's job: it falls through to `ChartLayout::
/// nearest_point` with the same `line_height() * 2.0` snap radius the
/// quadraui `chart_app` example uses for `MouseMoved` hover, so a click
/// anywhere near a plotted bar or line point still dispatches
/// `DataPointClicked` for it.
pub(crate) fn route_plugin_view_chart_click(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    pos: quadraui::Point,
    backend: &dyn quadraui::Backend,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};
    let cache = match host {
        PluginViewHost::Sidebar => &engine.plugin_view_chart_layout,
        PluginViewHost::Tab => &engine.plugin_view_tab_chart_layout,
        // Chart-kind Dock support isn't implemented.
        PluginViewHost::Dock => return false,
    };
    let kind = {
        let borrowed = cache.borrow();
        let Some(layout) = borrowed.as_ref() else {
            return false;
        };
        match layout.hit_test(pos.x, pos.y) {
            quadraui::ChartHit::DataPoint(_, series_idx, data_idx) => {
                ViewEventKind::DataPointClicked {
                    series_idx,
                    data_idx,
                }
            }
            quadraui::ChartHit::Legend(_, series_idx) => {
                ViewEventKind::LegendClicked { series_idx }
            }
            quadraui::ChartHit::Body(_) => {
                let snap = backend.line_height() * 2.0;
                let Some((series_idx, data_idx)) = layout.nearest_point(pos.x, pos.y, snap) else {
                    return false;
                };
                ViewEventKind::DataPointClicked {
                    series_idx,
                    data_idx,
                }
            }
            quadraui::ChartHit::Empty => return false,
        }
    };
    engine.dispatch_plugin_view_event(PluginViewEvent {
        view: name.to_string(),
        widget_id: String::new(),
        kind,
    });
    true
}
