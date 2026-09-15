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

    pub fn explorer_reveal_active_file(&mut self) {
        if let Some(path) = self.file_path().cloned() {
            self.explorer_reveal_path(&path);
        }
    }

    /// Expand every ancestor of `target`, rebuild the flattened row list,
    /// select `target`'s row and scroll it into view.
    ///
    /// # Why this still calls `TreeController::scroll_to_visible` (#659)
    ///
    /// #659 asked for the select-and-scroll half of this to be re-expressed on
    /// top of the composition quadraui#595 promoted *from this function*. That
    /// adoption is blocked, and the block is an altitude mismatch rather than
    /// a missing bump: quadraui#595 landed `reveal` on
    /// [`quadraui::SidebarSystem`] only —
    /// `SidebarSystem::reveal(section, path, rect)` — and there is no
    /// `TreeController::reveal`. `SidebarSystem::reveal` needs a
    /// `SidebarSystem` with `set_backend_info` already called and a viewport
    /// `Rect` to measure row capacity from; the explorer drives a bare
    /// `TreeController` (`self.explorer_tree`) with a pre-computed
    /// `self.explorer_viewport_rows`, and is not a `SidebarSystem` section at
    /// all — unlike `ext_sidebar_system` / `sc_sidebar_system` /
    /// `dap_sidebar_system`, which are.
    ///
    /// So the two ways to close this are: (a) quadraui factors the generic
    /// half of `SidebarSystem::reveal` down onto `TreeController` — something
    /// like `TreeController::reveal(&mut self, path: &TreePath, viewport_rows:
    /// usize)`, with `SidebarSystem::reveal` then being "expand the section,
    /// measure the viewport, delegate" — after which the three lines below
    /// collapse into one call; or (b) vimcode migrates the explorer onto
    /// `SidebarSystem`, which is a behaviour-bearing refactor #659 explicitly
    /// puts out of scope. (a) is the one that matches the promotion's intent,
    /// because the duplication quadraui#595 exists to remove is precisely
    /// *this* select-then-scroll pair, not the section bookkeeping around it.
    ///
    /// Until then this stays as-is on purpose: it is the reference
    /// composition, and rewriting it to *look* shared while still owning the
    /// math would be worse than leaving it honestly local. Per CLAUDE.md the
    /// next step is a quadraui issue for `TreeController::reveal`, not a
    /// vimcode-side approximation.
    pub fn explorer_reveal_path(&mut self, target: &Path) {
        let root = self.cwd.clone();
        if let Ok(rel) = target.strip_prefix(&root) {
            self.explorer_expanded.insert(root.clone());
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
        let viewport = self.explorer_viewport_rows.get();
        self.explorer_tree.borrow_mut().scroll_by(delta, viewport);
    }

    /// Handle a TreeController event originating from a mouse click.
    /// Single-click toggles dirs / previews files; double-click opens.
    /// Returns `true` if the event was consumed.
    pub fn handle_explorer_mouse_event(&mut self, event: quadraui::TreeControllerEvent) -> bool {
        match event {
            quadraui::TreeControllerEvent::RowSelected { ref path } => {
                let idx = path[0] as usize;
                if idx < self.explorer_rows.len() {
                    if self.explorer_rows[idx].is_dir {
                        self.explorer_toggle_dir(idx);
                    } else {
                        let file_path = self.explorer_rows[idx].path.clone();
                        self.open_file_preview(&file_path);
                    }
                }
                true
            }
            other => self.dispatch_explorer_tree_event(other),
        }
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
        let selected_idx = self.explorer_tree.borrow().selected_row_index();
        let idx = match selected_idx {
            Some(i) => i,
            None => {
                self.explorer_has_focus = false;
                self.activity_bar_focus_in_at(1);
                return ExplorerKeyResult::FocusToolbar;
            }
        };
        if idx >= self.explorer_rows.len() {
            self.explorer_has_focus = false;
            self.activity_bar_focus_in_at(1);
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
                self.explorer_has_focus = false;
                self.activity_bar_focus_in_at(1);
                ExplorerKeyResult::FocusToolbar
            }
        }
    }

    pub fn dispatch_explorer_crud(&mut self, action: ExplorerAction) -> ExplorerKeyResult {
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
                tree.start_editing(
                    tree_path,
                    String::new(),
                    0,
                    None,
                    Some(placeholder.to_string()),
                );
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
                    let stem_end = name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len());
                    let tree_path = vec![idx as u16];
                    self.explorer_tree.borrow_mut().start_editing(
                        tree_path,
                        name,
                        stem_end,
                        Some(0),
                        None,
                    );
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

/// Ordering used to sort explorer entries: directories before files, then by
/// name (respecting `case_insensitive`). Takes the directory-ness and name
/// already resolved rather than a filesystem handle, so it never touches the
/// filesystem and is a total order by construction -- see `collect_explorer_rows`
/// for why that matters (#631).
fn explorer_entry_cmp(
    a_is_dir: bool,
    a_name: &std::ffi::OsStr,
    b_is_dir: bool,
    b_name: &std::ffi::OsStr,
    case_insensitive: bool,
) -> std::cmp::Ordering {
    match (a_is_dir, b_is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => {
            if case_insensitive {
                let an = a_name.to_string_lossy().to_lowercase();
                let bn = b_name.to_string_lossy().to_lowercase();
                an.cmp(&bn)
            } else {
                a_name.cmp(b_name)
            }
        }
    }
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
    // Decorate-sort-undecorate: resolve each entry's directory-ness exactly once,
    // before sorting, rather than inside the comparator. `DirEntry::path().is_dir()`
    // issues a fresh `stat()` (following symlinks) on every call, so calling it from
    // the comparator re-stats the same entries repeatedly over the life of the sort.
    // If the directory mutates concurrently -- or an entry disappears, which
    // `is_dir()` silently reports as `false` -- the same entry can answer `true`
    // early in the sort and `false` later, breaking the total order `sort_by`
    // requires and panicking (#631). Precomputing the key makes the comparator pure
    // and total by construction, and this also reuses the same value for the row's
    // `is_dir` below instead of stat-ing again.
    let mut entries: Vec<(bool, std::fs::DirEntry)> = entries
        .filter_map(|e| e.ok())
        .map(|entry| {
            let is_dir = entry.path().is_dir();
            (is_dir, entry)
        })
        .collect();
    entries.sort_by(|(ad, a), (bd, b)| {
        explorer_entry_cmp(*ad, &a.file_name(), *bd, &b.file_name(), case_insensitive)
    });
    for (is_dir, entry) in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && !show_hidden {
            continue;
        }
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

#[cfg(test)]
mod explorer_sort_tests {
    use super::explorer_entry_cmp;
    use std::cmp::Ordering;
    use std::ffi::OsStr;

    // Regression coverage for #631: `explorer_entry_cmp` is the pure key
    // comparator extracted from `collect_explorer_rows`'s sort. Because it takes
    // an already-resolved `is_dir` bool instead of re-stat-ing the filesystem,
    // it is total and transitive by construction -- unlike the old comparator,
    // which could observe a directory mutate mid-sort and answer inconsistently,
    // violating the total order `sort_by` requires and panicking.
    //
    // "Factoring the comparator out to take a precomputed `is_dir` and unit-
    // testing its totality directly" is exactly the cheaper regression strategy
    // the issue calls out as preferable to racing a second thread against the
    // real sort.

    #[derive(Clone, Copy)]
    struct Entry<'a> {
        is_dir: bool,
        name: &'a str,
    }

    fn cmp(a: Entry, b: Entry, case_insensitive: bool) -> Ordering {
        explorer_entry_cmp(
            a.is_dir,
            OsStr::new(a.name),
            b.is_dir,
            OsStr::new(b.name),
            case_insensitive,
        )
    }

    #[test]
    fn dirs_sort_before_files_regardless_of_name() {
        let dir = Entry {
            is_dir: true,
            name: "zzz",
        };
        let file = Entry {
            is_dir: false,
            name: "aaa",
        };
        assert_eq!(cmp(dir, file, false), Ordering::Less);
        assert_eq!(cmp(file, dir, false), Ordering::Greater);
    }

    #[test]
    fn same_kind_sorts_by_name() {
        let a = Entry {
            is_dir: false,
            name: "alpha.txt",
        };
        let b = Entry {
            is_dir: false,
            name: "beta.rs",
        };
        assert_eq!(cmp(a, b, false), Ordering::Less);
        assert_eq!(cmp(b, a, false), Ordering::Greater);
    }

    #[test]
    fn case_insensitive_flag_is_respected() {
        let upper = Entry {
            is_dir: false,
            name: "Beta.rs",
        };
        let lower = Entry {
            is_dir: false,
            name: "alpha.txt",
        };
        // Case-sensitive: uppercase 'B' (0x42) sorts before lowercase 'a' (0x61).
        assert_eq!(cmp(upper, lower, false), Ordering::Less);
        // Case-insensitive: "alpha" < "beta" alphabetically.
        assert_eq!(cmp(upper, lower, true), Ordering::Greater);
    }

    #[test]
    fn comparator_is_reflexive_and_total_over_a_fixture_set() {
        // The same entry, asked twice, must always answer consistently -- the
        // exact property the old fs-querying comparator violated when a
        // directory disappeared or changed kind mid-sort.
        let entries = [
            Entry {
                is_dir: true,
                name: "subdir",
            },
            Entry {
                is_dir: true,
                name: "another",
            },
            Entry {
                is_dir: false,
                name: "alpha.txt",
            },
            Entry {
                is_dir: false,
                name: "beta.rs",
            },
            Entry {
                is_dir: false,
                name: ".hidden",
            },
        ];

        for &a in &entries {
            // Reflexivity: an entry compares equal to itself.
            assert_eq!(cmp(a, a, false), Ordering::Equal);
            for &b in &entries {
                // Antisymmetry: swapping arguments reverses the ordering.
                assert_eq!(cmp(a, b, false), cmp(b, a, false).reverse());
                for &c in &entries {
                    // Transitivity: a<=b and b<=c implies a<=c.
                    if cmp(a, b, false) != Ordering::Greater
                        && cmp(b, c, false) != Ordering::Greater
                    {
                        assert_ne!(
                            cmp(a, c, false),
                            Ordering::Greater,
                            "transitivity violated for {:?}/{:?}/{:?}",
                            a.name,
                            b.name,
                            c.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn sorting_a_fixture_set_never_panics_and_is_stable_on_repeat() {
        // A direct stand-in for the original panic: run the real `sort_by`
        // (which detects and panics on non-total comparators) over a fixture
        // set repeatedly and assert the resulting order never changes, i.e.
        // the comparator behaves as a pure, total order across repeated calls
        // rather than drifting the way a live fs-stat comparator could.
        let mut entries = vec![
            Entry {
                is_dir: true,
                name: "subdir",
            },
            Entry {
                is_dir: true,
                name: "another",
            },
            Entry {
                is_dir: false,
                name: "alpha.txt",
            },
            Entry {
                is_dir: false,
                name: "beta.rs",
            },
        ];
        entries.sort_by(|a, b| cmp(*a, *b, false));
        let first_pass: Vec<&str> = entries.iter().map(|e| e.name).collect();

        for _ in 0..50 {
            entries.sort_by(|a, b| cmp(*a, *b, false));
            let this_pass: Vec<&str> = entries.iter().map(|e| e.name).collect();
            assert_eq!(this_pass, first_pass);
        }

        assert_eq!(
            first_pass,
            vec!["another", "subdir", "alpha.txt", "beta.rs"]
        );
    }
}
