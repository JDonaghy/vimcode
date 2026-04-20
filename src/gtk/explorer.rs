//! GTK explorer sidebar state and data adapters.
//!
//! Phase A.2b migrates the GTK explorer off the native `gtk4::TreeView` +
//! `TreeStore` widget onto a `DrawingArea` that calls
//! `quadraui_gtk::draw_tree`. This module owns the flat row model the
//! DrawingArea renders from. It intentionally duplicates the TUI's
//! `ExplorerRow` / `collect_rows` shape (see `src/tui_main/mod.rs`) — a
//! future session can unify the two into `src/render.rs` once both
//! backends have stabilised on `quadraui::TreeView`.
//!
//! Sub-phase 1 ships this module as inert scaffolding (the atomic
//! TreeView → DrawingArea swap happens in sub-phase 2). The
//! `#[allow(dead_code)]` attributes reflect that — they are removed when
//! the callsites land.

#![allow(dead_code)]

use super::*;
use std::collections::HashSet;

/// One visible row in the flat explorer list.
pub(super) struct ExplorerRow {
    pub depth: usize,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_expanded: bool,
}

/// Per-panel state that lives on `App`.
pub(super) struct ExplorerState {
    pub rows: Vec<ExplorerRow>,
    pub expanded: HashSet<PathBuf>,
    pub selected: usize,
    pub scroll_top: usize,
}

impl ExplorerState {
    pub fn new(root: &Path) -> Self {
        let mut expanded = HashSet::new();
        expanded.insert(root.to_path_buf());
        let mut s = Self {
            rows: Vec::new(),
            expanded,
            selected: 0,
            scroll_top: 0,
        };
        s.rebuild(root, false, true);
        s
    }

    pub fn rebuild(&mut self, root: &Path, show_hidden: bool, case_insensitive: bool) {
        self.rows = build_explorer_rows(root, &self.expanded, show_hidden, case_insensitive);
        if !self.rows.is_empty() && self.selected >= self.rows.len() {
            self.selected = self.rows.len() - 1;
        }
    }

    pub fn toggle_dir(
        &mut self,
        idx: usize,
        root: &Path,
        show_hidden: bool,
        case_insensitive: bool,
    ) {
        if idx >= self.rows.len() || !self.rows[idx].is_dir {
            return;
        }
        let path = self.rows[idx].path.clone();
        if self.expanded.contains(&path) {
            self.expanded.remove(&path);
        } else {
            self.expanded.insert(path);
        }
        self.rebuild(root, show_hidden, case_insensitive);
    }

    pub fn ensure_visible(&mut self, viewport_rows: usize) {
        if viewport_rows == 0 {
            return;
        }
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + viewport_rows {
            self.scroll_top = self.selected + 1 - viewport_rows;
        }
    }

    /// Expand all ancestors of `target`, rebuild rows, select the matching
    /// row, scroll it into view. Matches TUI's `reveal_path` semantics.
    pub fn reveal_path(
        &mut self,
        target: &Path,
        root: &Path,
        viewport_rows: usize,
        show_hidden: bool,
        case_insensitive: bool,
    ) {
        if let Ok(rel) = target.strip_prefix(root) {
            let mut accum = root.to_path_buf();
            for component in rel.parent().into_iter().flat_map(|p| p.components()) {
                accum.push(component);
                self.expanded.insert(accum.clone());
            }
        }
        self.rebuild(root, show_hidden, case_insensitive);
        if let Some(idx) = self.rows.iter().position(|r| r.path == target) {
            self.selected = idx;
            self.ensure_visible(viewport_rows);
        }
    }
}

/// Build the flat row list for a workspace root, respecting the `expanded` set.
/// Always includes the root row at depth 0 at position 0.
pub(super) fn build_explorer_rows(
    root: &Path,
    expanded: &HashSet<PathBuf>,
    show_hidden: bool,
    case_insensitive: bool,
) -> Vec<ExplorerRow> {
    let mut out = Vec::new();
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let root_expanded = expanded.contains(root);
    out.push(ExplorerRow {
        depth: 0,
        name: root_name.to_uppercase(),
        path: root.to_path_buf(),
        is_dir: true,
        is_expanded: root_expanded,
    });
    if root_expanded {
        collect_rows(root, 1, expanded, show_hidden, case_insensitive, &mut out);
    }
    out
}

fn collect_rows(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    show_hidden: bool,
    case_insensitive: bool,
    out: &mut Vec<ExplorerRow>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by(|a, b| {
        let ad = a.path().is_dir();
        let bd = b.path().is_dir();
        match (ad, bd) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                if case_insensitive {
                    let an = a.file_name().to_string_lossy().to_lowercase();
                    let bn = b.file_name().to_string_lossy().to_lowercase();
                    an.cmp(&bn)
                } else {
                    a.file_name().cmp(&b.file_name())
                }
            }
        }
    });
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && !show_hidden {
            continue;
        }
        let is_dir = path.is_dir();
        let is_expanded = is_dir && expanded.contains(&path);
        out.push(ExplorerRow {
            depth,
            name,
            path: path.clone(),
            is_dir,
            is_expanded,
        });
        if is_expanded {
            collect_rows(
                &path,
                depth + 1,
                expanded,
                show_hidden,
                case_insensitive,
                out,
            );
        }
    }
}

/// Convert the GTK explorer state into a `quadraui::TreeView` for `draw_tree`.
/// Mirrors the TUI adapter at `src/tui_main/panels.rs::explorer_to_tree_view`.
pub(super) fn explorer_to_tree_view(
    state: &ExplorerState,
    has_focus: bool,
    engine: &Engine,
) -> quadraui::TreeView {
    use quadraui::{
        Badge, Decoration, Icon as QIcon, SelectionMode, StyledText, TreeRow, TreeStyle, TreeView,
        WidgetId,
    };

    let (git_statuses, diag_counts) = engine.explorer_indicators();

    let mut rows: Vec<TreeRow> = Vec::with_capacity(state.rows.len());
    for (row_idx, row) in state.rows.iter().enumerate() {
        let canon = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());

        let diag = diag_counts.get(&canon).copied();
        let git_label = git_statuses.get(&canon).copied();

        let decoration = match diag {
            Some((e, _)) if e > 0 => Decoration::Error,
            Some((_, w)) if w > 0 => Decoration::Warning,
            _ if git_label.is_some() => Decoration::Modified,
            _ => Decoration::Normal,
        };

        let badge = if let Some((errors, warnings)) = diag {
            if errors > 0 {
                Some(Badge::plain(if errors > 9 {
                    "9+".to_string()
                } else {
                    errors.to_string()
                }))
            } else if warnings > 0 {
                Some(Badge::plain(if warnings > 9 {
                    "9+".to_string()
                } else {
                    warnings.to_string()
                }))
            } else {
                git_label.map(|label| Badge::plain(label.to_string()))
            }
        } else {
            git_label.map(|label| Badge::plain(label.to_string()))
        };

        let icon = if row.is_dir {
            Some(QIcon::new(
                icons::FOLDER.nerd.to_string(),
                icons::FOLDER.fallback.to_string(),
            ))
        } else {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let glyph = icons::file_icon(ext).to_string();
            Some(QIcon::new(glyph, ".".to_string()))
        };

        rows.push(TreeRow {
            path: vec![row_idx as u16],
            indent: row.depth as u16,
            icon,
            text: StyledText::plain(&row.name),
            badge,
            is_expanded: if row.is_dir {
                Some(row.is_expanded)
            } else {
                None
            },
            decoration,
        });
    }

    let selected_path = if state.selected < rows.len() {
        Some(vec![state.selected as u16])
    } else {
        None
    };

    TreeView {
        id: WidgetId::new("explorer-tree"),
        rows,
        selection_mode: SelectionMode::Single,
        selected_path,
        scroll_offset: state.scroll_top,
        style: TreeStyle::default(),
        has_focus,
    }
}
