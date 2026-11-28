//! Hosting a quadraui `TreeView` from vimcode's own event loop.
//!
//! A `TreeView` is a controlled descriptor: its selection, scroll offset and
//! each row's rename session are fields the host owns, and the widget's
//! `Interactive` reports every change it makes as a `TreeEvent` for the host
//! to store. vimcode's trees (the explorer, a plugin's `ViewBody::Tree`)
//! rebuild their rows from engine data every frame and drive the widget
//! from raw `UiEvent`s outside quadraui's routing engine, so [`TreeHost`]
//! keeps the fields between frames — selection and scroll in quadraui's own
//! `TreeNavCore`, the rename session beside the rows — builds the
//! descriptor from them, and writes each reported `TreeEvent` back.

use quadraui::behaviour::nav::{Edge, NavOutcome, TreeNavCore};
use quadraui::primitives::tree::{
    tree_rename_insert, tree_rename_key, TreeRenameKey, TreeViewState,
};
use quadraui::{
    Backend, Handled, Key, Modifiers, Rect, SelectionMode, TreeEvent, TreePath, TreeRow,
    TreeRowEditState, TreeView, UiEvent, WidgetId,
};

/// A `TreeView`'s host-owned state, between frames.
#[derive(Debug)]
pub struct TreeHost {
    id: WidgetId,
    rows: Vec<TreeRow>,
    nav: TreeNavCore,
    has_focus: bool,
    edit: Option<(TreePath, TreeRowEditState)>,
    state: TreeViewState,
}

impl TreeHost {
    /// An empty, focused tree with nothing selected.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: WidgetId::new(id.into()),
            rows: Vec::new(),
            nav: TreeNavCore::new(),
            has_focus: true,
            edit: None,
            state: TreeViewState::default(),
        }
    }

    /// Replace the rows. The rename session, if any, stays with its path.
    pub fn set_rows(&mut self, rows: Vec<TreeRow>) {
        self.rows = rows;
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn set_has_focus(&mut self, has_focus: bool) {
        self.has_focus = has_focus;
    }

    pub fn selected_path(&self) -> Option<&TreePath> {
        self.nav.selected_path()
    }

    pub fn set_selected_path(&mut self, path: Option<TreePath>) {
        self.nav.set_selected_path(path);
    }

    /// The selected row's index in [`Self::rows`], found by path.
    pub fn selected_row_index(&self) -> Option<usize> {
        self.nav.selected_row_index(&self.rows)
    }

    pub fn scroll_offset(&self) -> usize {
        self.nav.scroll_offset()
    }

    pub fn set_scroll_offset(&mut self, offset: usize) {
        self.nav.set_scroll_offset(offset);
    }

    /// Move the selection by `delta` rows, scrolling it into view.
    pub fn move_selection_by(&mut self, delta: isize, viewport_rows: usize) -> NavOutcome {
        self.nav.move_selection_by(&self.rows, delta, viewport_rows)
    }

    /// Move the selection to the first or last row, scrolling it into view.
    pub fn move_selection_to_edge(&mut self, edge: Edge, viewport_rows: usize) -> NavOutcome {
        self.nav
            .move_selection_to_edge(&self.rows, edge, viewport_rows)
    }

    pub fn scroll_to_visible(&mut self, row: usize, viewport_rows: usize) {
        self.nav.scroll_to_visible(row, viewport_rows);
    }

    pub fn scroll_by(&mut self, delta: isize, viewport_rows: usize) {
        self.nav.scroll_by(delta, self.rows.len(), viewport_rows);
    }

    /// Begin renaming the row at `path`, selecting it. `cursor` and
    /// `selection_anchor` are byte offsets into `text`.
    pub fn start_editing(
        &mut self,
        path: TreePath,
        text: String,
        cursor: usize,
        selection_anchor: Option<usize>,
        placeholder: Option<String>,
    ) {
        self.nav.set_selected_path(Some(path.clone()));
        self.edit = Some((
            path,
            TreeRowEditState {
                text,
                cursor,
                selection_anchor,
                placeholder,
            },
        ));
    }

    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    /// The open rename session's current text, if any.
    pub fn editing_text(&self) -> Option<&str> {
        self.edit.as_ref().map(|(_, edit)| edit.text.as_str())
    }

    /// Resolve one keystroke against the open rename session through
    /// quadraui's shared rename editor, storing the edited text. `None`
    /// while no row is being renamed.
    pub fn rename_key(&mut self, key: &Key, modifiers: Modifiers) -> Option<TreeEvent> {
        let (path, edit) = self.edit.as_ref()?;
        let event = match tree_rename_key(edit, key, modifiers) {
            TreeRenameKey::Commit => TreeEvent::RowRenameCommitted {
                path: path.clone(),
                text: edit.text.clone(),
            },
            TreeRenameKey::Cancel => TreeEvent::RowRenameCancelled { path: path.clone() },
            TreeRenameKey::Edited(next) | TreeRenameKey::Moved(next) => {
                TreeEvent::RowRenameChanged {
                    path: path.clone(),
                    text: next.text,
                    cursor: next.cursor,
                    selection_anchor: next.selection_anchor,
                }
            }
            TreeRenameKey::Unhandled => return None,
        };
        self.apply(&event);
        Some(event)
    }

    /// Type `text` into the open rename session at its caret. `None` while
    /// no row is being renamed.
    pub fn rename_insert(&mut self, text: &str) -> Option<TreeEvent> {
        let (path, edit) = self.edit.as_ref()?;
        let next = tree_rename_insert(edit, text);
        let event = TreeEvent::RowRenameChanged {
            path: path.clone(),
            text: next.text,
            cursor: next.cursor,
            selection_anchor: next.selection_anchor,
        };
        self.apply(&event);
        Some(event)
    }

    /// The descriptor this host paints and routes as.
    pub fn view(&self) -> TreeView {
        let mut rows = self.rows.clone();
        if let Some((path, edit)) = &self.edit {
            if let Some(row) = rows.iter_mut().find(|r| &r.path == path) {
                row.edit = Some(edit.clone());
            }
        }
        TreeView {
            id: self.id,
            rows,
            selection_mode: SelectionMode::Single,
            selected_path: self.nav.selected_path().cloned(),
            scroll_offset: self.nav.scroll_offset(),
            items_window: None,
            scroll_px: self.scroll_px(),
            style: quadraui::TreeStyle {
                vim_keys: true,
                ..Default::default()
            },
            has_focus: self.has_focus,
        }
    }

    /// The pixel-precise scroll position to paint at: the one the widget's
    /// own wheel handling accumulated, while it still sits on the row the
    /// host stored; otherwise whatever the host's navigation set.
    fn scroll_px(&self) -> Option<f32> {
        if self.state.nav.scroll_offset() == self.nav.scroll_offset() {
            self.state.nav.scroll_px().or(self.nav.scroll_px())
        } else {
            self.nav.scroll_px()
        }
    }

    /// Paint the tree into `rect`.
    pub fn paint(&self, backend: &mut dyn Backend, rect: Rect) {
        quadraui::DrawsPrimitives::draw_primitive(backend, rect, &self.view());
    }

    /// Drive the tree with one raw event, laid out at `rect` exactly as
    /// [`Self::paint`] places it, and store every change it reports.
    pub fn handle(
        &mut self,
        event: &UiEvent,
        backend: &dyn Backend,
        rect: Rect,
    ) -> Handled<TreeEvent> {
        let view = self.view();
        let Some(driven) = quadraui::drive_ui_event(&view, &mut self.state, event, backend, rect)
        else {
            return Handled::Ignored;
        };
        for emitted in &driven.emitted {
            if let UiEvent::Tree(_, tree_event) = emitted {
                self.apply(tree_event);
            }
        }
        if let Handled::Event(tree_event) = &driven.handled {
            self.apply(tree_event);
        }
        driven.handled
    }

    /// Store what `event` changed.
    pub fn apply(&mut self, event: &TreeEvent) {
        match event {
            TreeEvent::RowClicked { path, .. }
            | TreeEvent::RowDoubleClicked { path }
            | TreeEvent::RowActivated { path }
            | TreeEvent::SelectionChanged { path }
            | TreeEvent::ContextMenuRequested { path, .. } => {
                self.nav.set_selected_path(Some(path.clone()));
            }
            TreeEvent::ScrollChanged { scroll_offset } => {
                self.nav.set_scroll_offset(*scroll_offset);
            }
            TreeEvent::RowRenameChanged {
                path,
                text,
                cursor,
                selection_anchor,
            } => {
                if let Some((p, edit)) = self.edit.as_mut() {
                    if p == path {
                        edit.text = text.clone();
                        edit.cursor = *cursor;
                        edit.selection_anchor = *selection_anchor;
                    }
                }
            }
            TreeEvent::RowRenameCommitted { .. } | TreeEvent::RowRenameCancelled { .. } => {
                self.edit = None;
            }
            _ => {}
        }
    }
}
