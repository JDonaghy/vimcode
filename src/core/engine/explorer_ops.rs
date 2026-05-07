use super::*;
use crate::core::settings::ExplorerAction;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerKeyResult {
    Consumed,
    Unfocused,
    FocusToolbar,
    Ignored,
}

impl Engine {
    pub fn explorer_rebuild_rows(&mut self) {
        let root = self.cwd.clone();
        self.explorer_rows = build_explorer_rows(
            &root,
            &self.explorer_expanded,
            self.settings.show_hidden_files,
            self.settings.explorer_sort_case_insensitive,
        );
        if let Some((ref parent_dir, _is_folder)) = self.explorer_new_entry_pending {
            let insert_at = self
                .explorer_rows
                .iter()
                .position(|r| r.is_dir && r.path == *parent_dir)
                .map(|i| i + 1)
                .unwrap_or(1);
            self.explorer_rows.insert(
                insert_at,
                ExplorerRow {
                    depth: self
                        .explorer_rows
                        .get(insert_at.saturating_sub(1))
                        .map(|r| r.depth + 1)
                        .unwrap_or(1),
                    name: String::new(),
                    path: parent_dir.join("__new__"),
                    is_dir: false,
                    is_expanded: false,
                },
            );
        }
        let mut tree = self.explorer_tree.borrow_mut();
        match tree.selected_row_index() {
            Some(idx) if idx >= self.explorer_rows.len() && !self.explorer_rows.is_empty() => {
                tree.set_selected_path(Some(vec![(self.explorer_rows.len() - 1) as u16]));
            }
            None if !self.explorer_rows.is_empty() => {
                tree.set_selected_path(Some(vec![0]));
            }
            _ => {}
        }
    }

    pub fn explorer_toggle_dir(&mut self, idx: usize) {
        if idx >= self.explorer_rows.len() || !self.explorer_rows[idx].is_dir {
            return;
        }
        let path = self.explorer_rows[idx].path.clone();
        if self.explorer_expanded.contains(&path) {
            self.explorer_expanded.remove(&path);
        } else {
            self.explorer_expanded.insert(path);
        }
        self.explorer_rebuild_rows();
    }

    pub fn explorer_reveal_path(&mut self, target: &Path) {
        let root = self.cwd.clone();
        if let Ok(rel) = target.strip_prefix(&root) {
            let mut accum = root.clone();
            for component in rel.parent().into_iter().flat_map(|p| p.components()) {
                accum.push(component);
                self.explorer_expanded.insert(accum.clone());
            }
        }
        self.explorer_rebuild_rows();
        if let Some(idx) = self.explorer_rows.iter().position(|r| r.path == target) {
            let mut tree = self.explorer_tree.borrow_mut();
            tree.set_selected_path(Some(vec![idx as u16]));
            tree.scroll_to_visible(idx, self.explorer_viewport_rows.get());
        }
    }

    pub fn explorer_activate_selected(&mut self) {
        let idx = match self.explorer_tree.borrow().selected_row_index() {
            Some(i) => i,
            None => return,
        };
        if idx >= self.explorer_rows.len() {
            return;
        }
        if self.explorer_rows[idx].is_dir {
            self.explorer_toggle_dir(idx);
        } else {
            let path = self.explorer_rows[idx].path.clone();
            self.open_file_in_tab(&path);
            self.explorer_has_focus = false;
        }
    }

    pub fn explorer_scroll(&mut self, delta: isize) {
        let mut tree = self.explorer_tree.borrow_mut();
        let viewport = self.explorer_viewport_rows.get();
        let max = self.explorer_rows.len().saturating_sub(viewport);
        let cur = tree.scroll_offset() as isize;
        let new = (cur + delta).max(0).min(max as isize) as usize;
        tree.set_scroll_offset(new);
    }

    pub fn dispatch_explorer_tree_event(&mut self, event: quadraui::TreeControllerEvent) -> bool {
        match event {
            quadraui::TreeControllerEvent::RowActivated { ref path } => {
                let idx = path[0] as usize;
                if idx < self.explorer_rows.len() {
                    if self.explorer_rows[idx].is_dir {
                        self.explorer_toggle_dir(idx);
                    } else {
                        let file_path = self.explorer_rows[idx].path.clone();
                        self.open_file_in_tab(&file_path);
                        self.explorer_has_focus = false;
                    }
                }
                true
            }
            quadraui::TreeControllerEvent::EditConfirmed {
                ref path,
                ref new_text,
            } => {
                self.handle_explorer_edit_confirmed(path, new_text);
                true
            }
            quadraui::TreeControllerEvent::EditCancelled { .. } => {
                self.handle_explorer_edit_cancelled();
                true
            }
            quadraui::TreeControllerEvent::RowSelected { .. } => true,
            quadraui::TreeControllerEvent::Ignored => false,
            _ => true,
        }
    }

    fn handle_explorer_edit_confirmed(&mut self, path: &[u16], new_text: &str) {
        let new_text = new_text.trim();
        if new_text.is_empty() {
            self.handle_explorer_edit_cancelled();
            return;
        }

        if let Some((parent_dir, is_folder)) = self.explorer_new_entry_pending.take() {
            let target = parent_dir.join(new_text);
            if target.exists() {
                self.message = format!("'{}' already exists", new_text);
            } else if is_folder {
                match std::fs::create_dir_all(&target) {
                    Ok(_) => self.message = format!("Created folder: {}", new_text),
                    Err(e) => self.message = format!("Error creating folder: {}", e),
                }
            } else {
                match std::fs::File::create(&target) {
                    Ok(_) => {
                        self.message = format!("Created: {}", new_text);
                        self.explorer_rebuild_rows();
                        self.open_file_in_tab(&target);
                        self.explorer_has_focus = false;
                        return;
                    }
                    Err(e) => self.message = format!("Error creating file: {}", e),
                }
            }
            self.explorer_rebuild_rows();
        } else {
            let idx = path[0] as usize;
            if idx < self.explorer_rows.len() {
                let old_path = self.explorer_rows[idx].path.clone();
                match self.rename_file(&old_path, new_text) {
                    Ok(()) => self.message = format!("Renamed to '{}'", new_text),
                    Err(e) => self.message = e,
                }
            }
            self.explorer_rebuild_rows();
        }
    }

    fn handle_explorer_edit_cancelled(&mut self) {
        self.explorer_new_entry_pending = None;
        self.explorer_rebuild_rows();
    }

    pub fn dispatch_explorer_key(
        &mut self,
        key_name: &str,
        chr: Option<char>,
        ctrl: bool,
    ) -> ExplorerKeyResult {
        if self.explorer_tree.borrow().is_editing() {
            let event = self.dispatch_explorer_edit_key(key_name, chr, ctrl);
            self.dispatch_explorer_tree_event(event);
            return ExplorerKeyResult::Consumed;
        }

        if self.explorer_rename.is_some() {
            self.handle_explorer_rename_key(key_name, chr, ctrl);
            return ExplorerKeyResult::Consumed;
        }
        if self.explorer_new_entry.is_some() {
            self.handle_explorer_new_entry_key(key_name, chr, ctrl);
            return ExplorerKeyResult::Consumed;
        }

        let viewport = self.explorer_viewport_rows.get();

        match key_name {
            "j" | "Down" => {
                self.explorer_tree
                    .borrow_mut()
                    .move_selection_by(1, viewport);
                ExplorerKeyResult::Consumed
            }
            "k" | "Up" => {
                self.explorer_tree
                    .borrow_mut()
                    .move_selection_by(-1, viewport);
                ExplorerKeyResult::Consumed
            }
            "Home" => {
                self.explorer_tree.borrow_mut().jump_to_edge(true, viewport);
                ExplorerKeyResult::Consumed
            }
            "End" => {
                self.explorer_tree
                    .borrow_mut()
                    .jump_to_edge(false, viewport);
                ExplorerKeyResult::Consumed
            }
            "Page_Up" | "PageUp" => {
                let step = viewport.saturating_sub(1).max(1) as isize;
                self.explorer_tree
                    .borrow_mut()
                    .move_selection_by(-step, viewport);
                ExplorerKeyResult::Consumed
            }
            "Page_Down" | "PageDown" => {
                let step = viewport.saturating_sub(1).max(1) as isize;
                self.explorer_tree
                    .borrow_mut()
                    .move_selection_by(step, viewport);
                ExplorerKeyResult::Consumed
            }
            "Return" | "KP_Enter" | "l" | "Right" => {
                self.explorer_activate_selected();
                if self.explorer_has_focus {
                    ExplorerKeyResult::Consumed
                } else {
                    ExplorerKeyResult::Unfocused
                }
            }
            "h" | "Left" => self.explorer_collapse_or_parent(),
            "Escape" | "q" => {
                self.explorer_has_focus = false;
                ExplorerKeyResult::Unfocused
            }
            _ => {
                if !ctrl {
                    if let Some(c) = chr {
                        if let Some(action) = self.settings.explorer_keys.resolve(c) {
                            return self.dispatch_explorer_crud(action);
                        }
                    }
                }
                ExplorerKeyResult::Ignored
            }
        }
    }

    fn dispatch_explorer_edit_key(
        &mut self,
        key_name: &str,
        chr: Option<char>,
        ctrl: bool,
    ) -> quadraui::TreeControllerEvent {
        use quadraui::{Key, Modifiers, NamedKey};

        let modifiers = Modifiers {
            ctrl,
            shift: false,
            alt: false,
            cmd: false,
        };

        match key_name {
            "Return" | "KP_Enter" => {
                let key = Key::Named(NamedKey::Enter);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "Escape" => {
                let key = Key::Named(NamedKey::Escape);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "BackSpace" => {
                let key = Key::Named(NamedKey::Backspace);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "Delete" => {
                let key = Key::Named(NamedKey::Delete);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "Left" => {
                let key = Key::Named(NamedKey::Left);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "Right" => {
                let key = Key::Named(NamedKey::Right);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "Home" => {
                let key = Key::Named(NamedKey::Home);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            "End" => {
                let key = Key::Named(NamedKey::End);
                self.explorer_tree
                    .borrow_mut()
                    .handle_edit_key_via(&key, &modifiers)
            }
            _ => {
                if ctrl {
                    if let Some(c) = chr {
                        let key = Key::Char(c);
                        return self
                            .explorer_tree
                            .borrow_mut()
                            .handle_edit_key_via(&key, &modifiers);
                    }
                }
                if let Some(c) = chr {
                    if !c.is_control() {
                        return self.explorer_tree.borrow_mut().edit_insert_char_via(c);
                    }
                }
                quadraui::TreeControllerEvent::Consumed
            }
        }
    }

    fn explorer_collapse_or_parent(&mut self) -> ExplorerKeyResult {
        let idx = match self.explorer_tree.borrow().selected_row_index() {
            Some(i) => i,
            None => return ExplorerKeyResult::FocusToolbar,
        };
        if idx >= self.explorer_rows.len() {
            return ExplorerKeyResult::FocusToolbar;
        }
        if self.explorer_rows[idx].is_dir && self.explorer_rows[idx].is_expanded {
            self.explorer_toggle_dir(idx);
            ExplorerKeyResult::Consumed
        } else {
            let target_depth = self.explorer_rows[idx].depth;
            if target_depth > 0 {
                let parent_idx = self.explorer_rows[..idx]
                    .iter()
                    .rposition(|r| r.depth < target_depth);
                if let Some(pi) = parent_idx {
                    let viewport = self.explorer_viewport_rows.get();
                    let mut tree = self.explorer_tree.borrow_mut();
                    tree.set_selected_path(Some(vec![pi as u16]));
                    tree.scroll_to_visible(pi, viewport);
                }
                ExplorerKeyResult::Consumed
            } else {
                ExplorerKeyResult::FocusToolbar
            }
        }
    }

    fn dispatch_explorer_crud(&mut self, action: ExplorerAction) -> ExplorerKeyResult {
        let idx = self
            .explorer_tree
            .borrow()
            .selected_row_index()
            .unwrap_or(0);

        match action {
            ExplorerAction::NewFile | ExplorerAction::NewFolder => {
                let target_dir = if idx < self.explorer_rows.len() {
                    let p = &self.explorer_rows[idx].path;
                    if p.is_dir() {
                        p.clone()
                    } else {
                        p.parent().unwrap_or(&self.cwd).to_path_buf()
                    }
                } else {
                    self.cwd.clone()
                };
                let is_folder = action == ExplorerAction::NewFolder;
                self.explorer_expanded.insert(target_dir.clone());
                self.explorer_new_entry_pending = Some((target_dir, is_folder));
                self.explorer_rebuild_rows();
                let insert_idx = self
                    .explorer_rows
                    .iter()
                    .position(|r| r.path.file_name().map(|n| n == "__new__").unwrap_or(false))
                    .unwrap_or(0);
                let tree_path = vec![insert_idx as u16];
                let placeholder = if is_folder {
                    "New folder name..."
                } else {
                    "New file name..."
                };
                let mut tree = self.explorer_tree.borrow_mut();
                tree.set_selected_path(Some(tree_path.clone()));
                tree.start_editing(tree_path, String::new(), Some(placeholder.to_string()));
            }
            ExplorerAction::Delete => {
                if idx < self.explorer_rows.len() {
                    let path = self.explorer_rows[idx].path.clone();
                    self.confirm_delete_file(&path);
                }
            }
            ExplorerAction::Rename => {
                if idx < self.explorer_rows.len() {
                    let name = self.explorer_rows[idx].name.clone();
                    let tree_path = vec![idx as u16];
                    self.explorer_tree
                        .borrow_mut()
                        .start_editing(tree_path, name, None);
                }
            }
            ExplorerAction::MoveFile => {
                if idx < self.explorer_rows.len() {
                    let path = self.explorer_rows[idx].path.clone();
                    let root = self.cwd.clone();
                    self.start_move_file_dialog(&path, &root);
                }
            }
        }
        ExplorerKeyResult::Consumed
    }
}

pub fn build_explorer_rows(
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
        collect_explorer_rows(root, 1, expanded, show_hidden, case_insensitive, &mut out);
    }
    out
}

fn collect_explorer_rows(
    dir: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    show_hidden: bool,
    case_insensitive: bool,
    out: &mut Vec<ExplorerRow>,
) {
    let entries = match std::fs::read_dir(dir) {
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
            collect_explorer_rows(
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
