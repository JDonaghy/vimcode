use super::*;

impl Engine {
    // =======================================================================
    // Window operations
    // =======================================================================

    /// Create a new window ID.
    pub(crate) fn new_window_id(&mut self) -> WindowId {
        let id = WindowId(self.next_window_id);
        self.next_window_id += 1;
        id
    }

    /// Create a new tab ID.
    pub(crate) fn new_tab_id(&mut self) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id += 1;
        id
    }

    /// Split the active window in the given direction.
    pub fn split_window(&mut self, direction: SplitDirection, file_path: Option<&Path>) {
        let current_buffer_id = self.active_buffer_id();
        let current_window_id = self.active_window_id();

        // Determine which buffer the new window should show
        let new_buffer_id = if let Some(path) = file_path {
            match self.buffer_manager.open_file(path) {
                Ok(id) => {
                    self.buffer_manager
                        .apply_language_map(id, &self.settings.language_map);
                    id
                }
                Err(e) => {
                    self.message = format!("Error: {}", e);
                    return;
                }
            }
        } else {
            // Same buffer as current window
            current_buffer_id
        };

        // Create new window
        let new_window_id = self.new_window_id();
        let mut new_window = Window::new(new_window_id, new_buffer_id);

        // Copy view state if same buffer
        if new_buffer_id == current_buffer_id {
            new_window.view = self.active_window().view.clone();
        }

        self.windows.insert(new_window_id, new_window);

        // Update layout — respect splitbelow / splitright settings.
        // new_first=true means the *new* window goes first (top / left).
        let new_first = match direction {
            SplitDirection::Horizontal => !self.settings.splitbelow,
            SplitDirection::Vertical => !self.settings.splitright,
        };
        let tab = self.active_tab_mut();
        tab.layout
            .split_at(current_window_id, direction, new_window_id, new_first);
        tab.active_window = new_window_id;

        if file_path.is_some() {
            self.message = String::new();
            self.lsp_did_open(new_buffer_id);
        }
    }

    /// Close the active window. Returns true if the window was closed.
    pub fn close_window(&mut self) -> bool {
        let is_single_tab = self.active_group().tabs.len() == 1;
        let is_single_window = self.active_tab().layout.is_single_window();

        // Can't close the last window in the last tab of the last group
        if is_single_window && is_single_tab && self.group_layout.is_single_group() {
            self.message = "Cannot close last window".to_string();
            return false;
        }

        let window_id = self.active_tab().active_window;

        // If this is the last window in the tab, close the tab
        if is_single_window {
            return self.close_tab();
        }

        // Remove window from layout
        let tab = self.active_tab_mut();
        if let Some(new_layout) = tab.layout.remove(window_id) {
            tab.layout = new_layout;
            // Set new active window
            if let Some(new_active) = tab.layout.window_ids().first().copied() {
                tab.active_window = new_active;
            }
        }

        // Remove window from windows map and any scroll-bind pairs that referenced it.
        let closed_buf_id = self.windows.get(&window_id).map(|w| w.buffer_id);
        self.windows.remove(&window_id);
        self.scroll_bind_pairs
            .retain(|&(a, b)| a != window_id && b != window_id);
        if let Some((a, b)) = self.diff_window_pair.take() {
            if a == window_id || b == window_id {
                self.clear_diff_labels(a, b);
                self.diff_results.clear();
                self.diff_aligned.clear();
                self.diff_unchanged_hidden = false;
                // Close the partner diff window + clean up scratch buffers.
                let partner = if a == window_id { b } else { a };
                let partner_buf = self.windows.get(&partner).map(|w| w.buffer_id);
                // Remove partner window from layout and windows map.
                if self.windows.contains_key(&partner) {
                    let tab = self.active_tab_mut();
                    if let Some(new_layout) = tab.layout.remove(partner) {
                        tab.layout = new_layout;
                        if let Some(first) = tab.layout.window_ids().first().copied() {
                            tab.active_window = first;
                        }
                    }
                    self.windows.remove(&partner);
                    self.scroll_bind_pairs
                        .retain(|&(x, y)| x != partner && y != partner);
                }
                // Delete orphaned scratch buffers (the HEAD side).
                for buf_id in [closed_buf_id, partner_buf].into_iter().flatten() {
                    let still_used = self.windows.values().any(|w| w.buffer_id == buf_id);
                    if !still_used {
                        if let Some(state) = self.buffer_manager.get(buf_id) {
                            if state.scratch_name.is_some() {
                                let _ = self.buffer_manager.delete(buf_id, true);
                            }
                        }
                    }
                }
                // After removing both diff windows, the tab may have no
                // valid windows left.  Close the tab to avoid a broken state.
                let tab_empty = self
                    .active_tab()
                    .layout
                    .window_ids()
                    .iter()
                    .all(|wid| !self.windows.contains_key(wid));
                if tab_empty {
                    self.close_tab();
                    return true;
                }
            } else {
                // Not our window — restore the pair.
                self.diff_window_pair = Some((a, b));
            }
        }

        // Ensure the active window is still valid after removal.
        self.repair_active_window();
        true
    }

    /// Close all windows except the active one in the current tab.
    pub fn close_other_windows(&mut self) {
        let active_window_id = self.active_window_id();
        let tab = self.active_tab_mut();

        // Get all window IDs except active
        let windows_to_close: Vec<WindowId> = tab
            .layout
            .window_ids()
            .into_iter()
            .filter(|&id| id != active_window_id)
            .collect();

        // Reset layout to single window
        tab.layout = WindowLayout::leaf(active_window_id);

        // Remove closed windows and any scroll-bind pairs referencing them.
        for id in windows_to_close {
            self.windows.remove(&id);
            self.scroll_bind_pairs.retain(|&(a, b)| a != id && b != id);
            if let Some((a, b)) = self.diff_window_pair {
                if a == id || b == id {
                    self.clear_diff_labels(a, b);
                    self.diff_window_pair = None;
                    self.diff_results.clear();
                    self.diff_aligned.clear();
                }
            }
        }

        self.message = String::new();
    }

    /// Move focus to the next window in the current tab.
    pub fn focus_next_window(&mut self) {
        self.active_tab_mut().cycle_next_window();
    }

    /// Move focus to the previous window in the current tab.
    pub fn focus_prev_window(&mut self) {
        self.active_tab_mut().cycle_prev_window();
    }

    /// Move focus to a window in the given direction.
    /// Sets `window_nav_overflow` to `Some(false)` when trying to go left past
    /// the first window, or `Some(true)` when trying to go right past the last.
    pub fn focus_window_direction(&mut self, _direction: SplitDirection, forward: bool) {
        let win_ids = self.active_tab().window_ids();
        let current = self.active_tab().active_window;
        let at_boundary = if forward {
            win_ids.last() == Some(&current)
        } else {
            win_ids.first() == Some(&current)
        };

        if at_boundary {
            // Try to move to adjacent editor group
            let group_ids = self.group_layout.group_ids();
            let cur_idx = group_ids.iter().position(|&id| id == self.active_group);
            let adjacent = cur_idx.and_then(|idx| {
                if forward {
                    group_ids.get(idx + 1).copied()
                } else {
                    idx.checked_sub(1).map(|i| group_ids[i])
                }
            });
            if let Some(next_group) = adjacent {
                self.prev_active_group = Some(self.active_group);
                self.active_group = next_group;
            } else {
                // No adjacent group → signal overflow to TUI/GTK
                self.window_nav_overflow = Some(forward);
            }
        } else if forward {
            self.focus_next_window();
        } else {
            self.focus_prev_window();
        }
    }

    /// Switch `active_group` to whichever group owns `window_id`.
    pub(crate) fn focus_group_for_window(&mut self, window_id: WindowId) {
        for (&gid, group) in &self.editor_groups {
            for (ti, tab) in group.tabs.iter().enumerate() {
                if tab.window_ids().contains(&window_id) {
                    if gid != self.active_group {
                        self.prev_active_group = Some(self.active_group);
                    }
                    self.active_group = gid;
                    if let Some(g) = self.editor_groups.get_mut(&gid) {
                        g.active_tab = ti;
                    }
                    return;
                }
            }
        }
    }

    /// Set cursor position for a specific window and make it active.
    /// Clamps line and col to valid buffer positions.
    pub fn set_cursor_for_window(&mut self, window_id: WindowId, line: usize, col: usize) {
        // Make the window active
        if self.windows.contains_key(&window_id) {
            self.active_tab_mut().active_window = window_id;

            // Get buffer and clamp line
            let buffer = self.buffer();
            let max_line = buffer.content.len_lines().saturating_sub(1);
            let clamped_line = line.min(max_line);

            // Get max col for this line (excludes newline)
            let max_col = self.get_max_cursor_col(clamped_line);
            let clamped_col = col.min(max_col);

            // Set cursor position
            let view = self.view_mut();
            view.cursor.line = clamped_line;
            view.cursor.col = clamped_col;
        }
    }

    // =======================================================================
    // Tab operations
    // =======================================================================

    /// Create a new tab with an optional file.
    pub fn new_tab(&mut self, file_path: Option<&Path>) {
        let buffer_id = if let Some(path) = file_path {
            match self.buffer_manager.open_file(path) {
                Ok(id) => {
                    self.buffer_manager
                        .apply_language_map(id, &self.settings.language_map);
                    id
                }
                Err(e) => {
                    self.message = format!("Error: {}", e);
                    return;
                }
            }
        } else {
            self.buffer_manager.create()
        };

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buffer_id);
        self.windows.insert(window_id, window);

        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;
        self.tab_mru_touch();
        self.ensure_active_tab_visible();

        if file_path.is_some() {
            self.message = String::new();
            self.lsp_did_open(buffer_id);
            self.swap_check_on_open(buffer_id);
        }
    }

    /// Close the current tab. Returns true if closed.
    pub fn close_tab(&mut self) -> bool {
        if self.active_group().tabs.len() <= 1 {
            // If there is a second group, close this group instead of erroring.
            if !self.group_layout.is_single_group() {
                self.close_editor_group();
                return true;
            }
            self.message = "Cannot close last tab".to_string();
            return false;
        }

        // Collect the buffer IDs of windows being closed so we can clean them
        // up from the buffer manager if nothing else references them.
        let active_tab_idx = self.active_group().active_tab;
        let window_ids: Vec<WindowId> = self.active_group().tabs[active_tab_idx].window_ids();
        let closed_buffer_ids: Vec<BufferId> = window_ids
            .iter()
            .filter_map(|wid| self.windows.get(wid).map(|w| w.buffer_id))
            .collect();

        // Remove all windows in this tab
        for window_id in &window_ids {
            self.windows.remove(window_id);
            self.scroll_bind_pairs
                .retain(|&(a, b)| a != *window_id && b != *window_id);
            if let Some((a, b)) = self.diff_window_pair {
                if a == *window_id || b == *window_id {
                    self.clear_diff_labels(a, b);
                    self.diff_window_pair = None;
                    self.diff_results.clear();
                    self.diff_aligned.clear();
                }
            }
        }

        let closed_group = self.active_group;
        let closed_tab_id = self.active_group().tabs[active_tab_idx].id;
        self.active_group_mut().tabs.remove(active_tab_idx);

        // Remove the closed tab from nav history.
        self.tab_nav_history
            .retain(|&(g, t)| !(g == closed_group && t == closed_tab_id));
        if self.tab_nav_index >= self.tab_nav_history.len() {
            self.tab_nav_index = self.tab_nav_history.len().saturating_sub(1);
        }

        // Remove the closed tab from MRU and adjust indices
        self.tab_mru
            .retain(|&(g, idx)| !(g == closed_group && idx == active_tab_idx));
        for entry in &mut self.tab_mru {
            if entry.0 == closed_group && entry.1 > active_tab_idx {
                entry.1 -= 1;
            }
        }

        // Adjust active tab index
        let tabs_len = self.active_group().tabs.len();
        if self.active_group().active_tab >= tabs_len {
            self.active_group_mut().active_tab = tabs_len - 1;
        }
        self.tab_mru_touch();
        // Ensure the new active tab's window state is consistent.
        self.repair_active_window();
        self.ensure_active_tab_visible();

        // Remove any buffers that are no longer referenced by any window.
        // This prevents orphaned dirty buffers from falsely triggering `:qa`
        // unsaved-changes prompts after the user discards a tab.
        let still_referenced: std::collections::HashSet<BufferId> =
            self.windows.values().map(|w| w.buffer_id).collect();
        for buf_id in closed_buffer_ids {
            if !still_referenced.contains(&buf_id) {
                // Delete swap file before removing the buffer.
                self.swap_delete_for_buffer(buf_id);
                self.swap_write_needed.remove(&buf_id);
                let _ = self.buffer_manager.delete(buf_id, true /* force */);
                // Clean up markdown preview link.
                self.md_preview_links.remove(&buf_id);
            }
        }

        true
    }

    /// Close a specific tab by group and index. Used for right-click "Close" on non-active tabs.
    pub fn close_tab_at(&mut self, group_id: GroupId, tab_idx: usize) -> bool {
        // Switch to the target group/tab, then close it.
        if !self.editor_groups.contains_key(&group_id) {
            return false;
        }
        let tabs_len = self.editor_groups[&group_id].tabs.len();
        if tab_idx >= tabs_len {
            return false;
        }
        let prev_group = self.active_group;
        let prev_tab = self.active_group().active_tab;
        self.active_group = group_id;
        self.editor_groups.get_mut(&group_id).unwrap().active_tab = tab_idx;
        let closed = self.close_tab();
        // If we didn't close (last tab), restore.
        if !closed {
            self.active_group = prev_group;
            if let Some(g) = self.editor_groups.get_mut(&prev_group) {
                if prev_tab < g.tabs.len() {
                    g.active_tab = prev_tab;
                }
            }
        }
        closed
    }

    /// Close all tabs in the current group except the active one.
    pub fn close_other_tabs(&mut self) {
        let active_tab_idx = self.active_group().active_tab;
        let tabs_len = self.active_group().tabs.len();
        if tabs_len <= 1 {
            return;
        }
        // Close tabs from highest index to lowest, skipping active.
        for i in (0..tabs_len).rev() {
            if i == active_tab_idx {
                continue;
            }
            // Set active to the tab we want to close, then close it.
            self.active_group_mut().active_tab = i;
            self.close_tab();
            // After closing, the active_tab might have shifted.
        }
        // Ensure the originally active tab (now the only one) is selected.
        self.active_group_mut().active_tab = 0;
        self.tab_mru_touch();
        self.repair_active_window();
    }

    /// Close all tabs to the right of the active tab.
    pub fn close_tabs_to_right(&mut self) {
        let active_tab_idx = self.active_group().active_tab;
        let tabs_len = self.active_group().tabs.len();
        if active_tab_idx >= tabs_len - 1 {
            return;
        }
        // Close from rightmost inward.
        for i in (active_tab_idx + 1..tabs_len).rev() {
            self.active_group_mut().active_tab = i;
            self.close_tab();
        }
        self.active_group_mut().active_tab = active_tab_idx;
        self.tab_mru_touch();
        self.repair_active_window();
    }

    /// Close all tabs to the left of the active tab.
    pub fn close_tabs_to_left(&mut self) {
        let active_tab_idx = self.active_group().active_tab;
        if active_tab_idx == 0 {
            return;
        }
        // Close from index 0 up to (not including) active.
        // Each close at index 0 shifts everything left.
        for _ in 0..active_tab_idx {
            self.active_group_mut().active_tab = 0;
            self.close_tab();
        }
        self.active_group_mut().active_tab = 0;
        self.tab_mru_touch();
        self.repair_active_window();
    }

    /// Close all non-dirty tabs except the active one.
    pub fn close_saved_tabs(&mut self) {
        let active_tab_idx = self.active_group().active_tab;
        let tabs_len = self.active_group().tabs.len();
        if tabs_len <= 1 {
            return;
        }
        // Collect indices of non-dirty, non-active tabs.
        let mut to_close = Vec::new();
        for i in 0..tabs_len {
            if i == active_tab_idx {
                continue;
            }
            // Check if the tab's primary buffer is dirty.
            let tab = &self.active_group().tabs[i];
            let wid = tab.active_window;
            if let Some(w) = self.windows.get(&wid) {
                if let Some(bs) = self.buffer_manager.get(w.buffer_id) {
                    if bs.dirty {
                        continue;
                    }
                }
            }
            to_close.push(i);
        }
        // Close from highest index to lowest.
        for i in to_close.into_iter().rev() {
            self.active_group_mut().active_tab = i;
            self.close_tab();
        }
        // Recalculate the active tab (original one shifted down by removed tabs below it).
        let remaining = self.active_group().tabs.len();
        if self.active_group().active_tab >= remaining {
            self.active_group_mut().active_tab = remaining.saturating_sub(1);
        }
        self.tab_mru_touch();
        self.repair_active_window();
    }

    /// Get the file path of a tab's primary window buffer.
    pub fn tab_file_path(&self, group_id: GroupId, tab_idx: usize) -> Option<PathBuf> {
        let group = self.editor_groups.get(&group_id)?;
        let tab = group.tabs.get(tab_idx)?;
        let wid = tab.active_window;
        let w = self.windows.get(&wid)?;
        let bs = self.buffer_manager.get(w.buffer_id)?;
        bs.file_path.clone()
    }

    /// Open the system file manager at the given path's parent directory.
    pub fn reveal_in_file_manager(&self, path: &Path) {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }

    /// Return the path relative to cwd.
    pub fn copy_relative_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.cwd)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    // ── Context menu methods ─────────────────────────────────────────────────

    /// Open a context menu for a tab.
    pub fn open_tab_context_menu(&mut self, group_id: GroupId, tab_idx: usize, x: u16, y: u16) {
        let group = match self.editor_groups.get(&group_id) {
            Some(g) => g,
            None => return,
        };
        let tabs_len = group.tabs.len();
        if tab_idx >= tabs_len {
            return;
        }

        let has_file = self.tab_file_path(group_id, tab_idx).is_some();

        let items = vec![
            ContextMenuItem {
                label: "Close".into(),
                action: "close".into(),
                shortcut: "Ctrl+W".into(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Close Others".into(),
                action: "close_others".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: tabs_len > 1,
            },
            ContextMenuItem {
                label: "Close to the Right".into(),
                action: "close_right".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: tab_idx < tabs_len - 1,
            },
            ContextMenuItem {
                label: "Close Saved".into(),
                action: "close_saved".into(),
                shortcut: String::new(),
                separator_after: true,
                enabled: true,
            },
            ContextMenuItem {
                label: "Copy Path".into(),
                action: "copy_path".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: has_file,
            },
            ContextMenuItem {
                label: "Copy Relative Path".into(),
                action: "copy_relative_path".into(),
                shortcut: String::new(),
                separator_after: true,
                enabled: has_file,
            },
            ContextMenuItem {
                label: "Reveal in File Explorer".into(),
                action: "reveal".into(),
                shortcut: String::new(),
                separator_after: true,
                enabled: has_file,
            },
            ContextMenuItem {
                label: "Split Right".into(),
                action: "split_right".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Split Down".into(),
                action: "split_down".into(),
                shortcut: String::new(),
                separator_after: true,
                enabled: true,
            },
            ContextMenuItem {
                label: "Split Right to New Group".into(),
                action: "group_split_right".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Split Down to New Group".into(),
                action: "group_split_down".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
        ];

        // Select the first enabled item.
        let selected = items.iter().position(|i| i.enabled).unwrap_or(0);

        self.context_menu = Some(ContextMenuState {
            target: ContextMenuTarget::Tab { group_id, tab_idx },
            items,
            selected,
            screen_x: x,
            screen_y: y,
        });
    }

    /// Open the editor action menu ("..." button) for a tab bar group.
    pub fn open_editor_action_menu(&mut self, group_id: GroupId, x: u16, y: u16) {
        let group = match self.editor_groups.get(&group_id) {
            Some(g) => g,
            None => return,
        };
        let tabs_len = group.tabs.len();
        let active_tab = group.active_tab;
        let has_file = self.tab_file_path(group_id, active_tab).is_some();
        let wrap_label = if self.settings.wrap {
            "Word Wrap: Off"
        } else {
            "Word Wrap: On"
        };

        let items = vec![
            ContextMenuItem {
                label: "Close All".into(),
                action: "close_all".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Close Others".into(),
                action: "close_others".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: tabs_len > 1,
            },
            ContextMenuItem {
                label: "Close Saved".into(),
                action: "close_saved".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Close to the Right".into(),
                action: "close_right".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: active_tab < tabs_len.saturating_sub(1),
            },
            ContextMenuItem {
                label: "Close to the Left".into(),
                action: "close_left".into(),
                shortcut: String::new(),
                separator_after: true,
                enabled: active_tab > 0,
            },
            ContextMenuItem {
                label: wrap_label.into(),
                action: "toggle_wrap".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Change Language Mode".into(),
                action: "change_language".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            },
            ContextMenuItem {
                label: "Reveal in File Explorer".into(),
                action: "reveal".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: has_file,
            },
        ];

        let selected = items.iter().position(|i| i.enabled).unwrap_or(0);
        self.context_menu = Some(ContextMenuState {
            target: ContextMenuTarget::EditorActionMenu { group_id },
            items,
            selected,
            screen_x: x,
            screen_y: y,
        });
    }

    /// Close all tabs in the active group.
    pub fn close_all_tabs(&mut self) {
        let tabs_len = self.active_group().tabs.len();
        for _ in 0..tabs_len {
            self.active_group_mut().active_tab = 0;
            self.close_tab();
        }
    }

    /// Open a context menu for an explorer file/directory.
    pub fn open_explorer_context_menu(&mut self, path: PathBuf, is_dir: bool, x: u16, y: u16) {
        let mut items = vec![];
        if is_dir {
            // Folder context menu (matches VSCode folder menu)
            items.push(ContextMenuItem {
                label: "New File...".into(),
                action: "new_file".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "New Folder...".into(),
                action: "new_folder".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Open Containing Folder".into(),
                action: "reveal".into(),
                shortcut: "Ctrl+Alt+R".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Open in Integrated Terminal".into(),
                action: "open_terminal".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Find in Folder...".into(),
                action: "find_in_folder".into(),
                shortcut: "Shift+Alt+F".into(),
                separator_after: true,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Copy Path".into(),
                action: "copy_path".into(),
                shortcut: "Ctrl+Alt+C".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Copy Relative Path".into(),
                action: "copy_relative_path".into(),
                shortcut: "Ctrl+Shift+Alt+C".into(),
                separator_after: true,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Rename...".into(),
                action: "rename".into(),
                shortcut: "F2".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Delete".into(),
                action: "delete".into(),
                shortcut: "Delete".into(),
                separator_after: false,
                enabled: true,
            });
        } else {
            // File context menu (matches VSCode file menu)
            items.push(ContextMenuItem {
                label: "Open to the Side".into(),
                action: "open_side".into(),
                shortcut: "Ctrl+Enter".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Open to the Side (vsplit)".into(),
                action: "open_side_vsplit".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Open Containing Folder".into(),
                action: "reveal".into(),
                shortcut: "Ctrl+Alt+R".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Open in Integrated Terminal".into(),
                action: "open_terminal".into(),
                shortcut: String::new(),
                separator_after: false,
                enabled: true,
            });
            if self.diff_selected_file.is_some() {
                let sel_name = self
                    .diff_selected_file
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into());
                items.push(ContextMenuItem {
                    label: format!("Compare with '{sel_name}'"),
                    action: "diff_with_selected".into(),
                    shortcut: String::new(),
                    separator_after: false,
                    enabled: true,
                });
                items.push(ContextMenuItem {
                    label: "Select for Compare".into(),
                    action: "select_for_diff".into(),
                    shortcut: String::new(),
                    separator_after: true,
                    enabled: true,
                });
            } else {
                items.push(ContextMenuItem {
                    label: "Select for Compare".into(),
                    action: "select_for_diff".into(),
                    shortcut: String::new(),
                    separator_after: true,
                    enabled: true,
                });
            }
            items.push(ContextMenuItem {
                label: "Copy Path".into(),
                action: "copy_path".into(),
                shortcut: "Ctrl+Alt+C".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Copy Relative Path".into(),
                action: "copy_relative_path".into(),
                shortcut: "Ctrl+Shift+Alt+C".into(),
                separator_after: true,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Rename...".into(),
                action: "rename".into(),
                shortcut: "F2".into(),
                separator_after: false,
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Delete".into(),
                action: "delete".into(),
                shortcut: "Delete".into(),
                separator_after: false,
                enabled: true,
            });
        }

        let target = if is_dir {
            ContextMenuTarget::ExplorerDir { path }
        } else {
            ContextMenuTarget::ExplorerFile { path }
        };

        self.context_menu = Some(ContextMenuState {
            target,
            items,
            selected: 0,
            screen_x: x,
            screen_y: y,
        });
    }

    /// Open a context menu for the editor area (right-click on buffer text).
    pub fn open_editor_context_menu(&mut self, x: u16, y: u16) {
        let has_file = self.file_path().is_some();
        let has_lsp = self.lsp_manager.is_some();
        let has_selection = matches!(
            self.mode,
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        );
        let vsc = self.is_vscode_mode();
        let items = vec![
            ContextMenuItem {
                label: "Go to Definition".into(),
                action: "goto_definition".into(),
                shortcut: if vsc { "F12" } else { "gd" }.into(),
                separator_after: false,
                enabled: has_lsp,
            },
            ContextMenuItem {
                label: "Go to References".into(),
                action: "goto_references".into(),
                shortcut: if vsc { "Shift+F12" } else { "gr" }.into(),
                separator_after: false,
                enabled: has_lsp,
            },
            ContextMenuItem {
                label: "Rename Symbol".into(),
                action: "rename_symbol".into(),
                shortcut: if vsc { "F2" } else { "<leader>rn" }.into(),
                separator_after: true,
                enabled: has_lsp,
            },
            ContextMenuItem {
                label: "Open Changes".into(),
                action: "open_changes".into(),
                shortcut: "gD".into(),
                separator_after: true,
                enabled: has_file,
            },
            ContextMenuItem {
                label: "Cut".into(),
                action: "cut".into(),
                shortcut: if vsc { "Ctrl+X" } else { "" }.into(),
                separator_after: false,
                enabled: has_selection,
            },
            ContextMenuItem {
                label: "Copy".into(),
                action: "copy".into(),
                shortcut: if vsc { "Ctrl+C" } else { "" }.into(),
                separator_after: false,
                enabled: has_selection,
            },
            ContextMenuItem {
                label: "Paste".into(),
                action: "paste".into(),
                shortcut: if vsc { "Ctrl+V" } else { "" }.into(),
                separator_after: true,
                enabled: true,
            },
            ContextMenuItem {
                label: "Open to the Side (vsplit)".into(),
                action: "open_side_vsplit".into(),
                shortcut: String::new(),
                separator_after: true,
                enabled: has_file,
            },
            ContextMenuItem {
                label: "Command Palette".into(),
                action: "command_palette".into(),
                shortcut: "F1".into(),
                separator_after: false,
                enabled: true,
            },
        ];
        let selected = items.iter().position(|i| i.enabled).unwrap_or(0);
        self.context_menu = Some(ContextMenuState {
            target: ContextMenuTarget::Editor,
            items,
            selected,
            screen_x: x,
            screen_y: y,
        });
    }

    /// Close the context menu without executing any action.
    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// Confirm the currently selected context menu item. Returns the action string.
    pub fn context_menu_confirm(&mut self) -> Option<String> {
        let menu = self.context_menu.take()?;
        let item = menu.items.get(menu.selected)?;
        if !item.enabled {
            return None;
        }
        let action = item.action.clone();

        match &menu.target {
            ContextMenuTarget::Tab { group_id, tab_idx } => {
                let group_id = *group_id;
                let tab_idx = *tab_idx;
                match action.as_str() {
                    "close" => {
                        self.close_tab_at(group_id, tab_idx);
                    }
                    "close_others" => {
                        // Focus the target tab first, then close others.
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.close_other_tabs();
                    }
                    "close_right" => {
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.close_tabs_to_right();
                    }
                    "close_saved" => {
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.close_saved_tabs();
                    }
                    "copy_path" => {
                        if let Some(path) = self.tab_file_path(group_id, tab_idx) {
                            let text = path.to_string_lossy().into_owned();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                            self.message = format!("Copied: {text}");
                        }
                    }
                    "copy_relative_path" => {
                        if let Some(path) = self.tab_file_path(group_id, tab_idx) {
                            let text = self.copy_relative_path(&path);
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                            self.message = format!("Copied: {text}");
                        }
                    }
                    "reveal" => {
                        if let Some(path) = self.tab_file_path(group_id, tab_idx) {
                            self.reveal_in_file_manager(&path);
                        }
                    }
                    "split_right" => {
                        // Focus the target tab, then split.
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.split_window(SplitDirection::Vertical, None);
                    }
                    "split_down" => {
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.split_window(SplitDirection::Horizontal, None);
                    }
                    "group_split_right" => {
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.open_editor_group(SplitDirection::Vertical);
                    }
                    "group_split_down" => {
                        self.active_group = group_id;
                        if let Some(g) = self.editor_groups.get_mut(&group_id) {
                            if tab_idx < g.tabs.len() {
                                g.active_tab = tab_idx;
                            }
                        }
                        self.open_editor_group(SplitDirection::Horizontal);
                    }
                    _ => {}
                }
            }
            ContextMenuTarget::ExplorerFile { path } | ContextMenuTarget::ExplorerDir { path } => {
                match action.as_str() {
                    "copy_path" => {
                        let text = path.to_string_lossy().into_owned();
                        if let Some(ref cb) = self.clipboard_write {
                            let _ = cb(&text);
                        }
                        self.message = format!("Copied: {text}");
                    }
                    "copy_relative_path" => {
                        let text = self.copy_relative_path(path);
                        if let Some(ref cb) = self.clipboard_write {
                            let _ = cb(&text);
                        }
                        self.message = format!("Copied: {text}");
                    }
                    "reveal" => {
                        self.reveal_in_file_manager(path);
                    }
                    "select_for_diff" => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.display().to_string());
                        self.diff_selected_file = Some(path.clone());
                        self.message = format!(
                            "Selected '{name}' for compare. Right-click another file to compare."
                        );
                    }
                    "diff_with_selected" => {
                        if let Some(left_path) = self.diff_selected_file.take() {
                            self.open_file_in_tab(&left_path);
                            self.cmd_diffthis();
                            self.cmd_diffsplit(path);
                        } else {
                            self.message =
                                "No file selected for compare. Use 'Select for Compare' first."
                                    .to_string();
                        }
                    }
                    "open_side" => {
                        self.open_editor_group(crate::core::window::SplitDirection::Vertical);
                        // Replace the cloned buffer with the target file.
                        let path_str = path.display().to_string();
                        self.execute_command(&format!("e {path_str}"));
                    }
                    "open_side_vsplit" => {
                        self.split_window(SplitDirection::Vertical, None);
                        let _ = self.open_file_with_mode(path, OpenMode::Permanent);
                    }
                    // new_file, new_folder, rename, delete are handled by the UI backend
                    // since they involve sidebar prompts. Return the action string.
                    _ => {}
                }
            }
            ContextMenuTarget::Editor => match action.as_str() {
                "goto_definition" => {
                    self.lsp_request_definition();
                }
                "goto_references" => {
                    self.lsp_request_references();
                }
                "rename_symbol" => {
                    // Enter command mode with :Rename pre-filled for user to type new name.
                    self.mode = Mode::Command;
                    self.command_buffer = "Rename ".to_string();
                }
                "open_changes" => {
                    self.open_diff_peek();
                }
                "cut" => {
                    // Yank selection to clipboard, then delete.
                    if matches!(
                        self.mode,
                        Mode::Visual | Mode::VisualLine | Mode::VisualBlock
                    ) {
                        self.yank_visual_selection();
                        // Copy yanked text to system clipboard.
                        if let Some((ref text, _)) = self.registers.get(&'"') {
                            let text = text.clone();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                        }
                        let mut changed = false;
                        self.delete_visual_selection(&mut changed);
                    }
                }
                "copy" => {
                    if matches!(
                        self.mode,
                        Mode::Visual | Mode::VisualLine | Mode::VisualBlock
                    ) {
                        self.yank_visual_selection();
                        if let Some((ref text, _)) = self.registers.get(&'"') {
                            let text = text.clone();
                            if let Some(ref cb) = self.clipboard_write {
                                let _ = cb(&text);
                            }
                        }
                        self.mode = Mode::Normal;
                    }
                }
                "paste" => {
                    if let Some(ref cb_read) = self.clipboard_read {
                        if let Ok(text) = cb_read() {
                            if !text.is_empty() {
                                self.registers.insert('"', (text, false));
                                let mut changed = false;
                                self.paste_after(&mut changed);
                            }
                        }
                    }
                }
                "open_side_vsplit" => {
                    if let Some(path) = self.file_path().map(|p| p.to_path_buf()) {
                        self.split_window(SplitDirection::Vertical, None);
                        let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
                    }
                }
                "command_palette" => {
                    self.open_picker(PickerSource::Commands);
                }
                _ => {}
            },
            ContextMenuTarget::EditorActionMenu { group_id } => {
                let group_id = *group_id;
                self.active_group = group_id;
                match action.as_str() {
                    "close_all" => {
                        self.close_all_tabs();
                    }
                    "close_others" => {
                        self.close_other_tabs();
                    }
                    "close_saved" => {
                        self.close_saved_tabs();
                    }
                    "close_right" => {
                        self.close_tabs_to_right();
                    }
                    "close_left" => {
                        self.close_tabs_to_left();
                    }
                    "toggle_wrap" => {
                        self.settings.wrap = !self.settings.wrap;
                        self.message = format!(
                            "Word wrap {}",
                            if self.settings.wrap { "on" } else { "off" }
                        );
                    }
                    "change_language" => {
                        self.open_picker(PickerSource::Languages);
                    }
                    "reveal" => {
                        if let Some(path) = self.file_path().map(|p| p.to_path_buf()) {
                            self.reveal_in_file_manager(&path);
                        }
                    }
                    _ => {}
                }
            }
            ContextMenuTarget::ExtPanel {
                panel_name,
                item_id,
            } => {
                // Fire panel_context_menu event — Lua plugins handle all actions.
                let arg = format!("{}||{}|{}|", panel_name, item_id, action);
                self.plugin_event("panel_context_menu", &arg);
            }
        }

        Some(action)
    }

    /// Handle keyboard input for the context menu popup.
    /// Returns true if the key was consumed.
    pub fn handle_context_menu_key(&mut self, key_name: &str) -> (bool, Option<String>) {
        if self.context_menu.is_none() {
            return (false, None);
        }

        match key_name {
            "j" | "Down" => {
                if let Some(ref mut menu) = self.context_menu {
                    // Move to next enabled item.
                    let len = menu.items.len();
                    let mut next = (menu.selected + 1) % len;
                    let start = next;
                    loop {
                        if menu.items[next].enabled {
                            break;
                        }
                        next = (next + 1) % len;
                        if next == start {
                            break;
                        }
                    }
                    menu.selected = next;
                }
                (true, None)
            }
            "k" | "Up" => {
                if let Some(ref mut menu) = self.context_menu {
                    let len = menu.items.len();
                    let mut prev = if menu.selected == 0 {
                        len - 1
                    } else {
                        menu.selected - 1
                    };
                    let start = prev;
                    loop {
                        if menu.items[prev].enabled {
                            break;
                        }
                        prev = if prev == 0 { len - 1 } else { prev - 1 };
                        if prev == start {
                            break;
                        }
                    }
                    menu.selected = prev;
                }
                (true, None)
            }
            "Return" | "l" => {
                let action = self.context_menu_confirm();
                (true, action)
            }
            "Escape" | "q" | "h" => {
                self.close_context_menu();
                (true, None)
            }
            _ => {
                self.close_context_menu();
                (true, None)
            }
        }
    }

    /// Record the current (group, tab_index) as the most recently used tab.
    pub fn tab_mru_touch(&mut self) {
        let entry = (self.active_group, self.active_group().active_tab);
        self.tab_mru.retain(|e| *e != entry);
        self.tab_mru.insert(0, entry);
    }

    /// Record the current tab in the back/forward navigation history.
    /// Only call this on explicit user navigation actions (goto_tab, next/prev tab,
    /// tab switcher confirm, open file). NOT called on new_tab, close_tab, or session restore.
    /// Skipped when navigating via back/forward to avoid polluting the stack.
    pub(crate) fn tab_nav_push(&mut self) {
        if self.tab_nav_navigating {
            return;
        }
        let tab_id = self.active_tab().id;
        let entry = (self.active_group, tab_id);

        // Consecutive duplicate suppression.
        if self.tab_nav_history.last() == Some(&entry)
            && self.tab_nav_index == self.tab_nav_history.len().saturating_sub(1)
        {
            return;
        }

        // Truncate forward history when navigating to a new tab.
        if self.tab_nav_index + 1 < self.tab_nav_history.len() {
            self.tab_nav_history.truncate(self.tab_nav_index + 1);
        }

        self.tab_nav_history.push(entry);

        // Bound at 100 entries.
        if self.tab_nav_history.len() > 100 {
            self.tab_nav_history.remove(0);
        }

        self.tab_nav_index = self.tab_nav_history.len().saturating_sub(1);
    }

    /// Navigate backward in tab history (across all editor groups).
    pub fn tab_nav_back(&mut self) {
        if self.tab_nav_index == 0 {
            return;
        }
        self.tab_nav_index -= 1;
        let (group_id, tab_id) = self.tab_nav_history[self.tab_nav_index];
        self.tab_nav_switch_to(group_id, tab_id);
    }

    /// Navigate forward in tab history (across all editor groups).
    pub fn tab_nav_forward(&mut self) {
        if self.tab_nav_index + 1 >= self.tab_nav_history.len() {
            return;
        }
        self.tab_nav_index += 1;
        let (group_id, tab_id) = self.tab_nav_history[self.tab_nav_index];
        self.tab_nav_switch_to(group_id, tab_id);
    }

    /// Switch to a specific group+tab by TabId, used by back/forward nav.
    fn tab_nav_switch_to(&mut self, group_id: GroupId, tab_id: TabId) {
        // Find the group and tab index for this TabId.
        if let Some(group) = self.editor_groups.get(&group_id) {
            if let Some(idx) = group.tabs.iter().position(|t| t.id == tab_id) {
                self.tab_nav_navigating = true;
                self.active_group = group_id;
                self.active_group_mut().active_tab = idx;
                self.line_annotations.clear();
                self.blame_annotations_active = false;
                self.tab_mru_touch(); // update MRU but skip nav push (navigating=true)
                self.lsp_ensure_active_buffer();
                self.ensure_active_tab_visible();
                self.tab_nav_navigating = false;
                return;
            }
        }
        // Tab or group no longer exists — remove stale entries.
        self.tab_nav_history
            .retain(|&(g, t)| !(g == group_id && t == tab_id));
        if self.tab_nav_index >= self.tab_nav_history.len() {
            self.tab_nav_index = self.tab_nav_history.len().saturating_sub(1);
        }
    }

    /// Whether back navigation is available (across all editor groups).
    pub fn tab_nav_can_go_back(&self) -> bool {
        self.tab_nav_index > 0
    }

    /// Whether forward navigation is available (across all editor groups).
    pub fn tab_nav_can_go_forward(&self) -> bool {
        self.tab_nav_index + 1 < self.tab_nav_history.len()
    }

    /// Switch to the next tab.
    pub fn next_tab(&mut self) {
        let tabs_len = self.active_group().tabs.len();
        if !self.active_group().tabs.is_empty() {
            self.active_group_mut().active_tab = (self.active_group().active_tab + 1) % tabs_len;
            self.line_annotations.clear();
            self.blame_annotations_active = false;
            self.tab_mru_touch();
            self.tab_nav_push();
            self.lsp_ensure_active_buffer();
            self.ensure_active_tab_visible();
        }
    }

    /// Switch to the previous tab.
    pub fn prev_tab(&mut self) {
        if !self.active_group().tabs.is_empty() {
            let at = self.active_group().active_tab;
            let tabs_len = self.active_group().tabs.len();
            self.active_group_mut().active_tab = if at == 0 { tabs_len - 1 } else { at - 1 };
            self.line_annotations.clear();
            self.blame_annotations_active = false;
            self.tab_mru_touch();
            self.tab_nav_push();
            self.lsp_ensure_active_buffer();
            self.ensure_active_tab_visible();
        }
    }

    /// Open the tab switcher popup, pre-selecting the second MRU entry.
    pub fn open_tab_switcher(&mut self) {
        // Build a clean MRU list: only include entries that still exist
        self.tab_mru.retain(|&(g, idx)| {
            self.editor_groups
                .get(&g)
                .is_some_and(|grp| idx < grp.tabs.len())
        });
        // Ensure the current tab is at index 0
        let current = (self.active_group, self.active_group().active_tab);
        if self.tab_mru.first() != Some(&current) {
            self.tab_mru.retain(|e| *e != current);
            self.tab_mru.insert(0, current);
        }
        // Also add any tabs not yet in MRU (e.g. from before MRU tracking started)
        for (&gid, group) in &self.editor_groups {
            for idx in 0..group.tabs.len() {
                if !self.tab_mru.contains(&(gid, idx)) {
                    self.tab_mru.push((gid, idx));
                }
            }
        }

        if self.tab_mru.len() <= 1 {
            return; // Nothing to switch to
        }
        self.tab_switcher_open = true;
        self.tab_switcher_selected = 1; // Start on the second item (previous tab)
    }

    /// Confirm the tab switcher selection and close the popup.
    pub fn tab_switcher_confirm(&mut self) {
        if !self.tab_switcher_open {
            return;
        }
        let idx = self.tab_switcher_selected;
        if let Some(&(group_id, tab_idx)) = self.tab_mru.get(idx) {
            if self.editor_groups.contains_key(&group_id) {
                self.active_group = group_id;
                self.active_group_mut().active_tab = tab_idx;
                self.tab_mru_touch();
                self.tab_nav_push();
                self.line_annotations.clear();
                self.blame_annotations_active = false;
                self.lsp_ensure_active_buffer();
                self.ensure_active_tab_visible();
            }
        }
        self.tab_switcher_open = false;
    }

    /// Get display info for each MRU entry: (filename, path, is_dirty).
    pub fn tab_switcher_items(&self) -> Vec<(String, String, bool)> {
        self.tab_mru
            .iter()
            .filter_map(|&(gid, tab_idx)| {
                let group = self.editor_groups.get(&gid)?;
                let tab = group.tabs.get(tab_idx)?;
                let win = self.windows.get(&tab.active_window)?;
                let state = self.buffer_manager.get(win.buffer_id)?;
                let name = state.display_name();
                let path = state
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Some((name, path, state.dirty))
            })
            .collect()
    }

    /// Switch to a specific tab (0-indexed).
    #[allow(dead_code)]
    pub fn goto_tab(&mut self, index: usize) {
        if index < self.active_group().tabs.len() {
            self.active_group_mut().active_tab = index;
            self.line_annotations.clear();
            self.blame_annotations_active = false;
            self.tab_mru_touch();
            self.tab_nav_push();
            self.lsp_ensure_active_buffer();
            self.ensure_active_tab_visible();
        }
    }

    /// Compute the display width (in columns) of a single tab at index `i`
    /// in the given group.  Matches the TUI render format:
    /// `" N: name " + close(2) + separator(1)`.
    fn tab_display_width(&self, group: &EditorGroup, i: usize) -> usize {
        let tab = &group.tabs[i];
        let window_id = tab.active_window;
        let name_len = if let Some(window) = self.windows.get(&window_id) {
            if let Some(state) = self.buffer_manager.get(window.buffer_id) {
                // " N: display_name "
                let dn = state.display_name();
                // leading space + digits + ": " + name + trailing space
                1 + (i + 1).to_string().len() + 2 + dn.chars().count() + 1
            } else {
                // " N: [No Name] "
                1 + (i + 1).to_string().len() + 2 + 9 + 1
            }
        } else {
            1 + (i + 1).to_string().len() + 2 + 9 + 1
        };
        name_len + 2 // +1 close button + 1 separator
    }

    /// Count how many tabs fit in the available width starting from `offset`.
    fn tabs_fitting_from(&self, group: &EditorGroup, offset: usize, width: usize) -> usize {
        let mut used = 0;
        let mut count = 0;
        for i in offset..group.tabs.len() {
            let tw = self.tab_display_width(group, i);
            if used + tw > width {
                break;
            }
            used += tw;
            count += 1;
        }
        count
    }

    /// Adjust `tab_scroll_offset` on the active group so that the active tab
    /// is visible in the tab bar, while showing as many tabs as possible.
    ///
    /// Strategy: start from offset 0 (maximize visible tabs), then only
    /// increase the offset if the active tab wouldn't fit.  Uses actual
    /// tab name widths and the reported tab bar width for accuracy.
    pub(crate) fn ensure_active_tab_visible(&mut self) {
        let group = match self.editor_groups.get(&self.active_group) {
            Some(g) => g,
            None => return,
        };
        let active = group.active_tab;
        let width = group.tab_bar_width;

        // How many tabs fit starting from offset 0?
        let from_zero = self.tabs_fitting_from(group, 0, width);

        if active < from_zero {
            // Active tab is visible from offset 0 — use it.
            self.editor_groups
                .get_mut(&self.active_group)
                .unwrap()
                .tab_scroll_offset = 0;
            return;
        }

        // Active tab doesn't fit from offset 0.  Find the smallest offset
        // that makes the active tab visible (i.e. at the right edge).
        // Walk backwards from the active tab, accumulating widths.
        let mut used = 0;
        let mut best_offset = active;
        for i in (0..=active).rev() {
            let tw = self.tab_display_width(group, i);
            if used + tw > width {
                break;
            }
            used += tw;
            best_offset = i;
        }
        self.editor_groups
            .get_mut(&self.active_group)
            .unwrap()
            .tab_scroll_offset = best_offset;
    }

    /// Called by the renderer to report the available tab bar width in
    /// character columns for a given group.
    pub fn set_tab_visible_count(&mut self, group_id: GroupId, width_cols: usize) {
        if let Some(g) = self.editor_groups.get_mut(&group_id) {
            if width_cols > 0 {
                g.tab_bar_width = width_cols;
            }
        }
    }

    /// Re-run `ensure_active_tab_visible` logic for every editor group.
    /// Called after the renderer reports updated tab bar widths (e.g. after
    /// a terminal resize) so that no group's active tab is off-screen.
    pub fn ensure_all_groups_tabs_visible(&mut self) {
        let group_ids: Vec<GroupId> = self.editor_groups.keys().copied().collect();
        let saved = self.active_group;
        for gid in group_ids {
            self.active_group = gid;
            self.ensure_active_tab_visible();
        }
        self.active_group = saved;
    }

    // =======================================================================
    // Editor group management (VSCode-style split panes)
    // =======================================================================

    /// Split the active editor group in the given direction.
    /// The new group opens a view of the current buffer.
    pub fn open_editor_group(&mut self, direction: SplitDirection) {
        let buf_id = self.active_buffer_id();
        let new_window_id = self.new_window_id();
        let mut new_window = Window::new(new_window_id, buf_id);
        // Copy view state for visual continuity.
        if let Some(src) = self.windows.get(&self.active_window_id()) {
            new_window.view = src.view.clone();
        }
        self.windows.insert(new_window_id, new_window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, new_window_id);
        let new_id = self.new_group_id();
        self.editor_groups.insert(new_id, EditorGroup::new(tab));
        self.group_layout
            .split_at(self.active_group, direction, new_id, false);
        self.prev_active_group = Some(self.active_group);
        self.active_group = new_id;
        self.message = "Editor split".to_string();
    }

    /// Close the currently focused editor group.
    /// All windows in the closing group are removed; sibling is promoted.
    pub fn close_editor_group(&mut self) {
        if self.group_layout.is_single_group() {
            return;
        }
        let closing = self.active_group;
        // Collect window IDs before modifying groups
        if let Some(group) = self.editor_groups.get(&closing) {
            let window_ids: Vec<WindowId> =
                group.tabs.iter().flat_map(|t| t.window_ids()).collect();
            for wid in window_ids {
                self.windows.remove(&wid);
            }
        }
        self.editor_groups.remove(&closing);
        self.group_layout.remove(closing);
        self.active_group = self.group_layout.group_ids()[0];
        // Ensure the new active group's window state is consistent.
        self.repair_active_window();
    }

    /// Move focus to the next editor group (wraps around).
    pub fn focus_other_group(&mut self) {
        if let Some(next) = self.group_layout.next_group(self.active_group) {
            self.active_group = next;
        }
    }

    /// Move the current tab from the active group to the next group.
    pub fn move_tab_to_other_group(&mut self) {
        if self.group_layout.is_single_group() {
            return;
        }
        if self.active_group().tabs.len() <= 1 {
            self.message = "Cannot move last tab out of group".to_string();
            return;
        }
        let idx = self.active_group().active_tab;
        let tab = self.active_group_mut().tabs.remove(idx);
        // Adjust active tab if needed
        let tabs_len = self.active_group().tabs.len();
        if self.active_group().active_tab >= tabs_len && tabs_len > 0 {
            self.active_group_mut().active_tab = tabs_len - 1;
        }
        let other = self
            .group_layout
            .next_group(self.active_group)
            .unwrap_or(self.active_group);
        if let Some(other_group) = self.editor_groups.get_mut(&other) {
            other_group.tabs.push(tab);
            other_group.active_tab = other_group.tabs.len() - 1;
        }
        self.active_group = other;
    }

    /// Adjust the group split ratio of the parent split containing the active group.
    /// Positive delta expands the active group (shrinks its sibling).
    pub fn group_resize(&mut self, delta: f64) {
        if let Some((split_index, _dir, is_first)) =
            self.group_layout.parent_split_of(self.active_group)
        {
            // If active group is first child, increase ratio expands it.
            // If second child, decrease ratio expands it.
            let adjusted = if is_first { delta } else { -delta };
            self.group_layout
                .adjust_ratio_at_index(split_index, adjusted);
        }
    }

    // --- Tab drag-and-drop ---

    /// Begin dragging a tab from the given group.
    pub fn tab_drag_begin(&mut self, group_id: GroupId, tab_index: usize) {
        let name = self
            .editor_groups
            .get(&group_id)
            .and_then(|g| g.tabs.get(tab_index))
            .and_then(|t| self.windows.get(&t.active_window))
            .and_then(|w| self.buffer_manager.get(w.buffer_id))
            .map(|s| s.display_name())
            .unwrap_or_default();
        self.tab_drag = Some(TabDragState {
            source_group: group_id,
            source_tab_index: tab_index,
            tab_name: name,
        });
        self.tab_drop_zone = DropZone::None;
    }

    /// Cancel an in-progress tab drag.
    #[allow(dead_code)]
    pub fn tab_drag_cancel(&mut self) {
        self.tab_drag = None;
        self.tab_drag_mouse = None;
        self.tab_drop_zone = DropZone::None;
    }

    /// Execute the drop for the current tab drag.
    pub fn tab_drag_drop(&mut self, zone: DropZone) {
        let drag = match self.tab_drag.take() {
            Some(d) => d,
            None => return,
        };
        self.tab_drag_mouse = None;
        self.tab_drop_zone = DropZone::None;

        match zone {
            DropZone::Center(target) => {
                if target != drag.source_group {
                    self.move_tab_to_target_group(drag.source_group, drag.source_tab_index, target);
                }
            }
            DropZone::Split(target, direction, new_first) => {
                self.move_tab_to_new_split(
                    drag.source_group,
                    drag.source_tab_index,
                    target,
                    direction,
                    new_first,
                );
            }
            DropZone::TabReorder(group_id, to_idx) => {
                if group_id == drag.source_group {
                    self.reorder_tab_in_group(group_id, drag.source_tab_index, to_idx);
                } else {
                    // Drag to a specific position in another group
                    self.move_tab_to_target_group_at(
                        drag.source_group,
                        drag.source_tab_index,
                        group_id,
                        to_idx,
                    );
                }
            }
            DropZone::None => {}
        }
    }

    /// Move a tab from one group to another (appends at end).
    pub fn move_tab_to_target_group(
        &mut self,
        src_group: GroupId,
        tab_idx: usize,
        target_group: GroupId,
    ) {
        self.move_tab_to_target_group_at(src_group, tab_idx, target_group, usize::MAX);
    }

    /// Move a tab from one group to another at a specific insertion index.
    pub(crate) fn move_tab_to_target_group_at(
        &mut self,
        src_group: GroupId,
        tab_idx: usize,
        target_group: GroupId,
        insert_at: usize,
    ) {
        if src_group == target_group {
            return;
        }
        let tab = match self.editor_groups.get_mut(&src_group) {
            Some(g) if tab_idx < g.tabs.len() => {
                let t = g.tabs.remove(tab_idx);
                if g.active_tab >= g.tabs.len() && !g.tabs.is_empty() {
                    g.active_tab = g.tabs.len() - 1;
                }
                t
            }
            _ => return,
        };
        // Insert into target
        if let Some(tg) = self.editor_groups.get_mut(&target_group) {
            let idx = insert_at.min(tg.tabs.len());
            tg.tabs.insert(idx, tab);
            tg.active_tab = idx;
        }
        self.active_group = target_group;
        // If source group is now empty, close it
        if self
            .editor_groups
            .get(&src_group)
            .is_some_and(|g| g.tabs.is_empty())
        {
            self.close_group_by_id(src_group);
        }
    }

    /// Move a tab out of its group into a new split adjacent to `target_group`.
    pub(crate) fn move_tab_to_new_split(
        &mut self,
        src_group: GroupId,
        tab_idx: usize,
        target_group: GroupId,
        direction: SplitDirection,
        new_first: bool,
    ) {
        // Remove tab from source
        let tab = match self.editor_groups.get_mut(&src_group) {
            Some(g) if tab_idx < g.tabs.len() => {
                let t = g.tabs.remove(tab_idx);
                if g.active_tab >= g.tabs.len() && !g.tabs.is_empty() {
                    g.active_tab = g.tabs.len() - 1;
                }
                t
            }
            _ => return,
        };
        // Create new group with the removed tab
        let new_id = self.new_group_id();
        self.editor_groups.insert(new_id, EditorGroup::new(tab));
        self.group_layout
            .split_at(target_group, direction, new_id, new_first);
        self.prev_active_group = Some(self.active_group);
        self.active_group = new_id;
        // If source group is now empty, close it
        if self
            .editor_groups
            .get(&src_group)
            .is_some_and(|g| g.tabs.is_empty())
        {
            self.close_group_by_id(src_group);
        }
    }

    /// Reorder a tab within its group.
    pub fn reorder_tab_in_group(&mut self, group_id: GroupId, from_idx: usize, to_idx: usize) {
        if let Some(g) = self.editor_groups.get_mut(&group_id) {
            if from_idx >= g.tabs.len() {
                return;
            }
            let to = to_idx.min(g.tabs.len().saturating_sub(1));
            if from_idx == to {
                return;
            }
            let tab = g.tabs.remove(from_idx);
            g.tabs.insert(to, tab);
            g.active_tab = to;
        }
    }

    /// Close a specific editor group by ID (removes windows, promotes sibling).
    pub fn close_group_by_id(&mut self, group_id: GroupId) {
        if self.group_layout.is_single_group() {
            return;
        }
        if let Some(group) = self.editor_groups.get(&group_id) {
            let window_ids: Vec<WindowId> =
                group.tabs.iter().flat_map(|t| t.window_ids()).collect();
            for wid in window_ids {
                self.windows.remove(&wid);
            }
        }
        self.editor_groups.remove(&group_id);
        self.group_layout.remove(group_id);
        if self.active_group == group_id {
            self.active_group = self.group_layout.group_ids()[0];
        }
    }

    /// Compute window rectangles for all editor groups, and return group dividers.
    ///
    /// `content_bounds` is the full editor content area (excluding status/command bars).
    /// `tab_bar_height` is subtracted from each group's usable height for the window area.
    pub fn calculate_group_window_rects(
        &self,
        content_bounds: WindowRect,
        tab_bar_height: f64,
    ) -> (Vec<(WindowId, WindowRect)>, Vec<GroupDivider>) {
        let mut group_rects = self
            .group_layout
            .calculate_group_rects(content_bounds, tab_bar_height);
        self.adjust_group_rects_for_hidden_tabs(&mut group_rects, tab_bar_height);
        let mut all_rects: Vec<(WindowId, WindowRect)> = Vec::new();
        for (gid, rect) in &group_rects {
            if let Some(group) = self.editor_groups.get(gid) {
                all_rects.extend(group.active_tab().layout.calculate_rects(*rect));
            }
        }
        let dividers = self.group_layout.dividers(content_bounds, &mut 0);
        (all_rects, dividers)
    }

    /// Open a file from the explorer: switch to an existing tab that shows it,
    /// or create a new tab when no tab currently displays it.
    ///
    /// This is the correct handler for sidebar file clicks — it never replaces
    /// the current tab's contents.
    pub fn open_file_in_tab(&mut self, path: &Path) {
        // Clear per-buffer virtual text annotations when switching files.
        self.line_annotations.clear();
        self.blame_annotations_active = false;
        let buffer_id = match self.buffer_manager.open_file(path) {
            Ok(id) => id,
            Err(e) => {
                self.message = format!("Error: {}", e);
                return;
            }
        };
        self.buffer_manager
            .apply_language_map(buffer_id, &self.settings.language_map);

        // If this buffer is the current preview, just promote it in-place.
        if self.preview_buffer_id == Some(buffer_id) {
            self.promote_preview(buffer_id);
            self.refresh_git_diff(buffer_id);
            self.message = format!("\"{}\"", path.display());
            self.lsp_did_open(buffer_id);
            return;
        }

        // Switch to any existing tab whose active window already shows this buffer.
        let found = self
            .active_group()
            .tabs
            .iter()
            .enumerate()
            .find(|(_, tab)| {
                self.windows
                    .get(&tab.active_window)
                    .is_some_and(|w| w.buffer_id == buffer_id)
            })
            .map(|(idx, _)| idx);
        if let Some(tab_idx) = found {
            self.active_group_mut().active_tab = tab_idx;
            self.tab_mru_touch();
            self.tab_nav_push();
            self.ensure_active_tab_visible();
            self.refresh_git_diff(buffer_id);
            self.message = format!("\"{}\"", path.display());
            self.lsp_did_open(buffer_id);
            return;
        }

        // No existing tab shows this file — open it in a new tab.
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buffer_id);
        self.windows.insert(window_id, window);

        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;
        self.tab_mru_touch();
        self.tab_nav_push();
        self.ensure_active_tab_visible();

        // Restore saved cursor/scroll position.
        let view = self.restore_file_position(buffer_id);
        if let Some(w) = self.windows.get_mut(&window_id) {
            w.view = view;
        }

        self.refresh_git_diff(buffer_id);
        // Don't overwrite a pending dialog message.
        if self.dialog.is_none() {
            self.message = format!("\"{}\"", path.display());
        }
        self.lsp_did_open(buffer_id);

        // Swap file check: detect stale swaps and offer recovery.
        self.swap_check_on_open(buffer_id);
    }

    /// Open a file from the sidebar via single-click (preview mode).
    ///
    /// Behaviour mirrors VSCode:
    /// - If the file is already shown in any tab, just switch to that tab.
    /// - If there is an existing preview tab, replace it with this file.
    /// - Otherwise open a new preview tab.
    ///
    /// A preview buffer is marked italic/dimmed and is replaced by the next
    /// single-click. Double-clicking (or editing/saving) promotes it to
    /// permanent.
    pub fn open_file_preview(&mut self, path: &Path) {
        let buffer_id = match self.buffer_manager.open_file(path) {
            Ok(id) => id,
            Err(e) => {
                self.message = format!("Error: {}", e);
                return;
            }
        };
        self.buffer_manager
            .apply_language_map(buffer_id, &self.settings.language_map);

        // Already shown in any tab? Just switch to it (permanent or current preview).
        let found = self
            .active_group()
            .tabs
            .iter()
            .enumerate()
            .find(|(_, tab)| {
                self.windows
                    .get(&tab.active_window)
                    .is_some_and(|w| w.buffer_id == buffer_id)
            })
            .map(|(idx, _)| idx);
        if let Some(tab_idx) = found {
            self.active_group_mut().active_tab = tab_idx;
            self.ensure_active_tab_visible();
            self.refresh_git_diff(buffer_id);
            self.message = format!("\"{}\"", path.display());
            self.lsp_did_open(buffer_id);
            return;
        }

        // Find the existing preview tab, if any (within the active group).
        let mut preview_slot: Option<(usize, WindowId, BufferId)> = None;
        if let Some(preview_buf_id) = self.preview_buffer_id {
            for (idx, tab) in self.active_group().tabs.iter().enumerate() {
                let win_id = tab.active_window;
                if self
                    .windows
                    .get(&win_id)
                    .is_some_and(|w| w.buffer_id == preview_buf_id)
                {
                    preview_slot = Some((idx, win_id, preview_buf_id));
                    break;
                }
            }
        }

        if let Some((tab_idx, win_id, old_buf_id)) = preview_slot {
            // Reuse the existing preview tab: close old preview buffer and
            // point the window at the new one.
            let _ = self.delete_buffer(old_buf_id, true);
            if let Some(w) = self.windows.get_mut(&win_id) {
                w.buffer_id = buffer_id;
            }
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.preview = true;
            }
            self.preview_buffer_id = Some(buffer_id);
            self.active_group_mut().active_tab = tab_idx;
            let view = self.restore_file_position(buffer_id);
            if let Some(w) = self.windows.get_mut(&win_id) {
                w.view = view;
            }
        } else {
            // No preview tab yet — open a new one.
            let window_id = self.new_window_id();
            let window = Window::new(window_id, buffer_id);
            self.windows.insert(window_id, window);
            let tab_id = self.new_tab_id();
            let tab = Tab::new(tab_id, window_id);
            self.active_group_mut().tabs.push(tab);
            self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;
            if let Some(state) = self.buffer_manager.get_mut(buffer_id) {
                state.preview = true;
            }
            self.preview_buffer_id = Some(buffer_id);
            let view = self.restore_file_position(buffer_id);
            if let Some(w) = self.windows.get_mut(&window_id) {
                w.view = view;
            }
        }

        self.ensure_active_tab_visible();
        self.refresh_git_diff(buffer_id);
        self.message = format!("\"{}\"", path.display());
        self.lsp_did_open(buffer_id);
    }

    // =======================================================================
    // Buffer navigation
    // =======================================================================

    /// Switch the current window to the next buffer.
    pub fn next_buffer(&mut self) {
        let current = self.active_buffer_id();
        if let Some(next) = self.buffer_manager.next_buffer(current) {
            self.buffer_manager.alternate_buffer = Some(current);
            self.switch_window_buffer(next);
        }
    }

    /// Switch the current window to the previous buffer.
    pub fn prev_buffer(&mut self) {
        let current = self.active_buffer_id();
        if let Some(prev) = self.buffer_manager.prev_buffer(current) {
            self.buffer_manager.alternate_buffer = Some(current);
            self.switch_window_buffer(prev);
        }
    }

    /// Switch the current window to the alternate buffer.
    pub fn alternate_buffer(&mut self) {
        if let Some(alt) = self.buffer_manager.alternate_buffer {
            let current = self.active_buffer_id();
            self.buffer_manager.alternate_buffer = Some(current);
            self.switch_window_buffer(alt);
        } else {
            self.message = "No alternate buffer".to_string();
        }
    }

    /// Switch the current window to a buffer by number (1-indexed).
    pub fn goto_buffer(&mut self, num: usize) {
        if let Some(id) = self.buffer_manager.get_by_number(num) {
            let current = self.active_buffer_id();
            if id != current {
                self.buffer_manager.alternate_buffer = Some(current);
                self.switch_window_buffer(id);
            }
        } else {
            self.message = format!("Buffer {} does not exist", num);
        }
    }

    /// Switch the current window to a different buffer.
    pub(crate) fn switch_window_buffer(&mut self, buffer_id: BufferId) {
        if self.buffer_manager.get(buffer_id).is_none() {
            return;
        }

        // Save current buffer's cursor/scroll position before switching
        let current_id = self.active_window().buffer_id;
        if current_id != buffer_id {
            if let Some(path) = self
                .buffer_manager
                .get(current_id)
                .and_then(|s| s.file_path.as_deref())
                .map(|p| p.to_path_buf())
            {
                let view = &self.active_window().view;
                self.session.save_file_position(
                    &path,
                    view.cursor.line,
                    view.cursor.col,
                    view.scroll_top,
                );
            }
        }

        // Switch to the new buffer
        self.active_window_mut().buffer_id = buffer_id;

        // Restore saved position, clamped to actual buffer bounds
        let new_view = self.restore_file_position(buffer_id);
        self.active_window_mut().view = new_view;

        self.search_matches.clear();
        self.search_index = None;
    }

    /// Build a View restoring the saved position for a buffer, or return View::new().
    pub(crate) fn restore_file_position(&self, buffer_id: BufferId) -> View {
        let path = match self
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.as_deref())
            .map(|p| p.to_path_buf())
        {
            Some(p) => p,
            None => return View::new(),
        };

        let pos = match self.session.get_file_position(&path) {
            Some(p) => p,
            None => return View::new(),
        };

        let buf = self.buffer_manager.get(buffer_id).unwrap();
        let max_line = buf.buffer.len_lines().saturating_sub(1);
        let line = pos.line.min(max_line);
        let line_len = buf.buffer.line_len_chars(line);
        let max_col = line_len.saturating_sub(1);
        let col = pos.col.min(max_col);
        let scroll_top = pos.scroll_top.min(max_line);

        View {
            cursor: Cursor { line, col },
            scroll_top,
            ..View::new()
        }
    }

    /// Return the absolute paths of buffers currently shown in at least one window.
    /// Orphaned buffers (closed via :q but not yet freed) are intentionally excluded so
    /// that files the user explicitly closed are not restored on the next startup.
    #[allow(dead_code)]
    pub fn open_file_paths(&self) -> Vec<std::path::PathBuf> {
        let in_window: std::collections::HashSet<BufferId> =
            self.windows.values().map(|w| w.buffer_id).collect();
        self.buffer_manager
            .list()
            .into_iter()
            .filter(|id| in_window.contains(id))
            .filter_map(|id| {
                self.buffer_manager
                    .get(id)
                    .and_then(|s| s.file_path.clone())
            })
            .collect()
    }

    /// Snapshot the current open-file list and active file into session state, ready for saving.
    /// Only populates `session.active_file` (for file_positions); open_files are saved
    /// exclusively in per-workspace sessions to prevent cross-workspace bleed.
    pub fn collect_session_open_files(&mut self) {
        // Do NOT write open_files to the global session — they belong in the
        // per-workspace session only. This prevents files from workspace A
        // appearing in workspace B.
        self.session.open_files.clear();
        self.session.active_file = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.clone());
    }

    /// Restore open files from session state (called at startup when no CLI file is given).
    /// Each file gets its own tab; the previously-active file's tab is focused.
    /// Skips files that no longer exist. Removes the initial empty scratch buffer.
    pub fn restore_session_files(&mut self) {
        // Prefer per-workspace session if one exists for cwd
        let ws_session = SessionState::load_for_workspace(&self.cwd.clone());

        // Merge workspace file positions into current session.
        if !ws_session.open_files.is_empty() || ws_session.group_layout.is_some() {
            for (k, v) in ws_session.file_positions.clone() {
                self.session.file_positions.entry(k).or_insert(v);
            }
        }

        // New tree format takes priority if present.
        if let Some(ref tree_layout) = ws_session.group_layout {
            self.restore_session_from_tree(tree_layout, &ws_session.active_file);
            return;
        }

        // Fall back to old flat-field format.
        let paths_g1 = if !ws_session.open_files.is_empty() {
            ws_session.open_files_group1.clone()
        } else {
            vec![]
        };
        let saved_active_group = if !ws_session.open_files.is_empty() {
            ws_session.active_group
        } else {
            0
        };
        let saved_split_dir = if !ws_session.open_files.is_empty() {
            ws_session.group_split_direction
        } else {
            0u8
        };
        let saved_split_ratio = if !ws_session.open_files.is_empty() {
            ws_session.group_split_ratio
        } else {
            0.5
        };
        let (paths, active) = if !ws_session.open_files.is_empty() {
            (ws_session.open_files, ws_session.active_file)
        } else {
            // No workspace session for this directory — start with empty editor.
            // Do NOT fall back to the global session's open_files: that would
            // bleed files from a different workspace into the current one.
            (vec![], None)
        };

        if paths.is_empty() {
            return;
        }

        let initial_id = self.active_buffer_id();
        let mut any_opened = false;
        let mut first = true;

        for path in &paths {
            if !path.exists() {
                continue;
            }
            if first {
                // Reuse the initial window for the first file.
                if self.open_file_with_mode(path, OpenMode::Permanent).is_ok() {
                    any_opened = true;
                    first = false;
                }
            } else {
                // Each subsequent file gets its own tab.
                self.new_tab(Some(path));
                let buf_id = self.active_buffer_id();
                let view = self.restore_file_position(buf_id);
                let win_id = self.active_tab().active_window;
                if let Some(window) = self.windows.get_mut(&win_id) {
                    window.view = view;
                }
                any_opened = true;
            }
        }

        if !any_opened {
            return;
        }

        // Remove the initial empty scratch buffer now that real files are open.
        let _ = self.delete_buffer(initial_id, true);

        // Switch focus to the tab showing the previously-active file.
        if let Some(ref ap) = active {
            if let Ok(canonical_ap) = ap.canonicalize() {
                let tab_idx = self.active_group().tabs.iter().position(|t| {
                    self.windows
                        .get(&t.active_window)
                        .and_then(|w| self.buffer_manager.get(w.buffer_id))
                        .and_then(|s| s.file_path.as_ref())
                        .and_then(|p| p.canonicalize().ok())
                        .is_some_and(|p| p == canonical_ap)
                });
                if let Some(idx) = tab_idx {
                    self.active_group_mut().active_tab = idx;
                }
            }
        }

        // Restore editor group 1 if it was open (old flat-field format).
        let valid_g1: Vec<PathBuf> = paths_g1.into_iter().filter(|p| p.exists()).collect();
        if !valid_g1.is_empty() {
            let direction = if saved_split_dir == 1 {
                SplitDirection::Horizontal
            } else {
                SplitDirection::Vertical
            };
            self.open_editor_group(direction);
            // Set saved ratio on the root split (split_index 0).
            self.group_layout.set_ratio_at_index(0, saved_split_ratio);
            // open_editor_group switches to new group — open files there
            let mut first_g1 = true;
            for path in &valid_g1 {
                if first_g1 {
                    let _ = self.open_file_with_mode(path, OpenMode::Permanent);
                    first_g1 = false;
                } else {
                    self.new_tab(Some(path));
                }
            }
            // Restore active group: clamp to valid range
            let ids = self.group_layout.group_ids();
            if saved_active_group < ids.len() {
                self.active_group = ids[saved_active_group];
            }
        }

        // Seed nav history with just the active tab — arrows are greyed out
        // (can't go back from index 0) but the first manual switch will record
        // destination while this entry serves as the origin.
        self.tab_nav_history.clear();
        let seed_tab_id = self.active_tab().id;
        self.tab_nav_history.push((self.active_group, seed_tab_id));
        self.tab_nav_index = 0;

        // Check all restored buffers for stale swap files.
        self.swap_check_all_buffers();
    }

    /// Restore session from the recursive tree format.
    pub(crate) fn restore_session_from_tree(
        &mut self,
        tree_layout: &SessionGroupLayout,
        active_file: &Option<PathBuf>,
    ) {
        let initial_id = self.active_buffer_id();
        let initial_group = self.active_group;

        // Reconstruct the full group layout tree, creating groups/windows/buffers.
        let new_layout = self.restore_session_group_layout(tree_layout);

        // Remove the initial scratch group and its buffer.
        self.editor_groups.remove(&initial_group);
        let _ = self.delete_buffer(initial_id, true);

        // Install the new layout.
        self.group_layout = new_layout;
        let ids = self.group_layout.group_ids();
        self.active_group = ids.first().copied().unwrap_or(GroupId(0));

        // Focus the group+tab containing the previously-active file.
        if let Some(ref ap) = active_file {
            if let Ok(canonical_ap) = ap.canonicalize() {
                'outer: for &gid in &ids {
                    if let Some(group) = self.editor_groups.get(&gid) {
                        for (ti, tab) in group.tabs.iter().enumerate() {
                            let matches = self
                                .windows
                                .get(&tab.active_window)
                                .and_then(|w| self.buffer_manager.get(w.buffer_id))
                                .and_then(|s| s.file_path.as_ref())
                                .and_then(|p| p.canonicalize().ok())
                                .is_some_and(|p| p == canonical_ap);
                            if matches {
                                self.active_group = gid;
                                if let Some(g) = self.editor_groups.get_mut(&gid) {
                                    g.active_tab = ti;
                                }
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        // Notify LSP only for the active buffer — other buffers will get
        // lsp_did_open when the user actually switches to their tab.
        let active_bid = self.active_buffer_id();
        let has_file = self
            .buffer_manager
            .get(active_bid)
            .and_then(|s| s.file_path.as_ref())
            .is_some();
        if has_file {
            self.lsp_did_open(active_bid);
        }

        // Seed nav history with just the active tab — arrows greyed out until
        // the user manually switches tabs (matches VSCode behavior).
        self.tab_nav_history.clear();
        let seed_tab_id = self.active_tab().id;
        self.tab_nav_history.push((self.active_group, seed_tab_id));
        self.tab_nav_index = 0;

        // Check all restored buffers for stale swap files.
        self.swap_check_all_buffers();
    }

    /// Delete a buffer. Returns error if buffer is shown in any window or is dirty.
    pub fn delete_buffer(&mut self, id: BufferId, force: bool) -> Result<(), String> {
        // Check if buffer is shown in any window
        let in_use: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, w)| w.buffer_id == id)
            .map(|(wid, _)| *wid)
            .collect();

        if !in_use.is_empty() && self.buffer_manager.len() > 1 {
            // Switch those windows to another buffer
            let alt = self
                .buffer_manager
                .list()
                .into_iter()
                .find(|&bid| bid != id);

            if let Some(alt_id) = alt {
                for wid in in_use {
                    if let Some(window) = self.windows.get_mut(&wid) {
                        window.buffer_id = alt_id;
                        window.view = View::new();
                    }
                }
            }
        }

        // Clear preview tracking if deleting the preview buffer
        if self.preview_buffer_id == Some(id) {
            self.preview_buffer_id = None;
        }

        self.lsp_did_close(id);
        self.buffer_manager.delete(id, force)
    }

    /// Get the list of buffers for :ls display.
    pub fn list_buffers(&self) -> String {
        let active = self.active_buffer_id();
        let alternate = self.buffer_manager.alternate_buffer;

        let mut lines = Vec::new();
        for (i, id) in self.buffer_manager.list().iter().enumerate() {
            let state = self.buffer_manager.get(*id).unwrap();
            let num = i + 1;
            let active_flag = if *id == active { "%a" } else { "  " };
            let alt_flag = if Some(*id) == alternate { "#" } else { " " };
            let dirty_flag = if state.dirty { "+" } else { " " };
            let name = state.display_name();
            let preview_flag = if state.preview { " [Preview]" } else { "" };
            lines.push(format!(
                "{:3} {}{}{} \"{}\"{}",
                num, active_flag, alt_flag, dirty_flag, name, preview_flag
            ));
        }
        lines.join("\n")
    }
}

// ─── Additional methods (extracted from mod.rs) ─────────────────────────

impl Engine {
    // =======================================================================
    // Window resize (CTRL-W +/-/</>=/|/_)
    // =======================================================================

    /// Resize the window's parent split by delta steps.
    /// `direction`: which split direction to look for (Horizontal for +/-, Vertical for </>).
    /// `increase`: true = make active group bigger, false = smaller.
    pub(crate) fn resize_window_split(
        &mut self,
        direction: SplitDirection,
        increase: bool,
        count: usize,
    ) {
        let delta_per_step = 0.05;
        if let Some((split_idx, split_dir, is_first)) =
            self.group_layout.parent_split_of(self.active_group)
        {
            if split_dir == direction {
                // Active group is in first child → increasing ratio makes it bigger
                let delta = if (is_first && increase) || (!is_first && !increase) {
                    delta_per_step * count as f64
                } else {
                    -(delta_per_step * count as f64)
                };
                self.group_layout.adjust_ratio_at_index(split_idx, delta);
            }
        }
    }

    /// Equalize all split ratios to 0.5.
    pub(crate) fn equalize_splits(&mut self) {
        self.group_layout.set_all_ratios(0.5);
    }

    /// Maximize window in a given direction (CTRL-W _ for height, CTRL-W | for width).
    pub(crate) fn maximize_window_split(&mut self, direction: SplitDirection) {
        if let Some((split_idx, split_dir, is_first)) =
            self.group_layout.parent_split_of(self.active_group)
        {
            if split_dir == direction {
                let ratio = if is_first { 0.9 } else { 0.1 };
                self.group_layout.set_ratio_at_index(split_idx, ratio);
            }
        }
    }

    /// Execute a window command by character (`:wincmd {char}` and Ctrl-W {char}).
    pub(crate) fn execute_wincmd(&mut self, ch: char, count: usize) -> EngineAction {
        match ch {
            // Focus
            'h' => self.focus_window_direction(SplitDirection::Vertical, false),
            'j' => self.focus_window_direction(SplitDirection::Horizontal, true),
            'k' => self.focus_window_direction(SplitDirection::Horizontal, false),
            'l' => self.focus_window_direction(SplitDirection::Vertical, true),
            'w' | 'W' => self.focus_next_window(),
            'p' => {
                if let Some(prev) = self.prev_active_group {
                    if self.editor_groups.contains_key(&prev) {
                        let cur = self.active_group;
                        self.active_group = prev;
                        self.prev_active_group = Some(cur);
                    }
                }
            }
            't' => {
                if let Some(first) = self.group_layout.nth_leaf(0) {
                    if first != self.active_group {
                        self.prev_active_group = Some(self.active_group);
                    }
                    self.active_group = first;
                }
            }
            'b' => {
                let ids = self.group_layout.group_ids();
                if let Some(&last) = ids.last() {
                    if last != self.active_group {
                        self.prev_active_group = Some(self.active_group);
                    }
                    self.active_group = last;
                }
            }
            // Move
            'H' => self.move_window_to_edge(SplitDirection::Vertical, false),
            'J' => self.move_window_to_edge(SplitDirection::Horizontal, true),
            'K' => self.move_window_to_edge(SplitDirection::Horizontal, false),
            'L' => self.move_window_to_edge(SplitDirection::Vertical, true),
            'T' => self.move_window_to_new_group(),
            'x' => self.exchange_windows(),
            'r' => self.rotate_windows(true),
            'R' => self.rotate_windows(false),
            // Split / Close
            's' | 'S' => self.split_window(SplitDirection::Horizontal, None),
            'v' | 'V' => self.split_window(SplitDirection::Vertical, None),
            'c' | 'C' => {
                self.close_window();
            }
            'q' => {
                self.close_window();
            }
            'o' | 'O' => self.close_other_windows(),
            'n' => {
                let _ = self.execute_command("new");
            }
            // Editor groups
            'e' => self.open_editor_group(SplitDirection::Vertical),
            'E' => self.open_editor_group(SplitDirection::Horizontal),
            // Resize (count-aware)
            '+' => self.resize_window_split(SplitDirection::Horizontal, true, count),
            '-' => self.resize_window_split(SplitDirection::Horizontal, false, count),
            '>' => self.resize_window_split(SplitDirection::Vertical, true, count),
            '<' => self.resize_window_split(SplitDirection::Vertical, false, count),
            '=' => self.equalize_splits(),
            '_' => self.maximize_window_split(SplitDirection::Horizontal),
            '|' => self.maximize_window_split(SplitDirection::Vertical),
            // Composite
            'f' => {
                if let Some(path) = self.file_path_under_cursor() {
                    let abs_path = if path.is_absolute() {
                        path
                    } else {
                        self.cwd.join(&path)
                    };
                    self.split_window(SplitDirection::Horizontal, None);
                    return EngineAction::OpenFile(abs_path);
                } else {
                    self.message = "No file path under cursor".to_string();
                }
            }
            'd' => {
                self.split_window(SplitDirection::Horizontal, None);
                self.push_jump_location();
                self.lsp_request_definition();
            }
            _ => {
                self.message = format!("Unknown wincmd: {}", ch);
            }
        }
        EngineAction::None
    }

    /// Ctrl-W H/J/K/L: move current window to far edge.
    /// Creates a new editor group at the edge of the entire layout.
    pub(crate) fn move_window_to_edge(&mut self, direction: SplitDirection, forward: bool) {
        // Only meaningful with multiple groups
        let groups = self.group_layout.group_ids();
        if groups.len() <= 1 {
            // With a single group, split the group layout at the root
            let buf_id = self.active_buffer_id();
            let view_clone = self.view().clone();
            // Close current window if possible
            let could_close = self.close_window();
            if !could_close {
                return; // Last window, can't move
            }
            // Create new group at the edge
            let new_win_id = self.new_window_id();
            let mut new_win = Window::new(new_win_id, buf_id);
            new_win.view = view_clone;
            self.windows.insert(new_win_id, new_win);
            let tab = Tab::new(self.new_tab_id(), new_win_id);
            let new_gid = self.new_group_id();
            self.editor_groups.insert(new_gid, EditorGroup::new(tab));
            // Wrap the existing layout in a split with the new group at the desired edge
            let old_layout =
                std::mem::replace(&mut self.group_layout, GroupLayout::leaf(GroupId(0)));
            self.group_layout = if forward {
                GroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(old_layout),
                    second: Box::new(GroupLayout::leaf(new_gid)),
                }
            } else {
                GroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(GroupLayout::leaf(new_gid)),
                    second: Box::new(old_layout),
                }
            };
            self.prev_active_group = Some(self.active_group);
            self.active_group = new_gid;
            return;
        }
        // Multiple groups: remove window, create new group at edge
        let buf_id = self.active_buffer_id();
        let view_clone = self.view().clone();
        let could_close = self.close_window();
        if !could_close {
            return;
        }
        let new_win_id = self.new_window_id();
        let mut new_win = Window::new(new_win_id, buf_id);
        new_win.view = view_clone;
        self.windows.insert(new_win_id, new_win);
        let tab = Tab::new(self.new_tab_id(), new_win_id);
        let new_gid = self.new_group_id();
        self.editor_groups.insert(new_gid, EditorGroup::new(tab));
        let old_layout = std::mem::replace(&mut self.group_layout, GroupLayout::leaf(GroupId(0)));
        self.group_layout = if forward {
            GroupLayout::Split {
                direction,
                ratio: 0.5,
                first: Box::new(old_layout),
                second: Box::new(GroupLayout::leaf(new_gid)),
            }
        } else {
            GroupLayout::Split {
                direction,
                ratio: 0.5,
                first: Box::new(GroupLayout::leaf(new_gid)),
                second: Box::new(old_layout),
            }
        };
        self.prev_active_group = Some(self.active_group);
        self.active_group = new_gid;
    }

    /// Ctrl-W T: move current window to a new editor group.
    pub(crate) fn move_window_to_new_group(&mut self) {
        // Only meaningful with multiple windows in the current tab
        let tab = self.active_tab();
        if tab.layout.is_single_window() && self.active_group().tabs.len() == 1 {
            self.message = "Already the only window".to_string();
            return;
        }
        let buf_id = self.active_buffer_id();
        let view_clone = self.view().clone();
        let could_close = self.close_window();
        if !could_close {
            return;
        }
        // Open a new editor group with that buffer
        let new_win_id = self.new_window_id();
        let mut new_win = Window::new(new_win_id, buf_id);
        new_win.view = view_clone;
        self.windows.insert(new_win_id, new_win);
        let tab = Tab::new(self.new_tab_id(), new_win_id);
        let new_gid = self.new_group_id();
        self.editor_groups.insert(new_gid, EditorGroup::new(tab));
        self.group_layout
            .split_at(self.active_group, SplitDirection::Vertical, new_gid, false);
        self.prev_active_group = Some(self.active_group);
        self.active_group = new_gid;
    }

    /// Ctrl-W x: exchange current window with next window in the same tab.
    pub(crate) fn exchange_windows(&mut self) {
        let tab = self.active_tab();
        let ids = tab.layout.window_ids();
        if ids.len() < 2 {
            return;
        }
        let current_id = tab.active_window;
        let current_idx = ids.iter().position(|&id| id == current_id).unwrap_or(0);
        let next_idx = (current_idx + 1) % ids.len();
        let next_id = ids[next_idx];
        // Swap buffer_id and view between the two windows
        let current_buf = self.windows[&current_id].buffer_id;
        let current_view = self.windows[&current_id].view.clone();
        let next_buf = self.windows[&next_id].buffer_id;
        let next_view = self.windows[&next_id].view.clone();
        if let Some(w) = self.windows.get_mut(&current_id) {
            w.buffer_id = next_buf;
            w.view = next_view;
        }
        if let Some(w) = self.windows.get_mut(&next_id) {
            w.buffer_id = current_buf;
            w.view = current_view;
        }
    }

    /// Ctrl-W r/R: rotate windows in the current tab.
    /// `forward=true` rotates downward/rightward, `forward=false` rotates upward/leftward.
    pub(crate) fn rotate_windows(&mut self, forward: bool) {
        let tab = self.active_tab();
        let ids = tab.layout.window_ids();
        if ids.len() < 2 {
            return;
        }
        // Collect (buffer_id, view) for each window in layout order
        let mut data: Vec<_> = ids
            .iter()
            .map(|&id| {
                let w = &self.windows[&id];
                (w.buffer_id, w.view.clone())
            })
            .collect();
        // Rotate the data
        if forward {
            // Last element moves to front
            let last = data.pop().unwrap();
            data.insert(0, last);
        } else {
            // First element moves to back
            let first = data.remove(0);
            data.push(first);
        }
        // Apply rotated data back
        for (i, &id) in ids.iter().enumerate() {
            if let Some(w) = self.windows.get_mut(&id) {
                w.buffer_id = data[i].0;
                w.view = data[i].1.clone();
            }
        }
    }

    /// Jump to end of C-style comment block (]*  or  ]/).
    pub(crate) fn jump_comment_end(&mut self) {
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        for line_idx in start..total {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.contains("*/") {
                self.view_mut().cursor.line = line_idx;
                // Position cursor at the '*' of '*/'
                if let Some(pos) = line.find("*/") {
                    let col = line[..pos].chars().count();
                    self.view_mut().cursor.col = col;
                } else {
                    self.view_mut().cursor.col = 0;
                }
                self.clamp_cursor_col();
                return;
            }
        }
    }

    /// Jump to start of C-style comment block ([*  or  [/).
    pub(crate) fn jump_comment_start(&mut self) {
        let cursor_line = self.view().cursor.line;
        if cursor_line == 0 {
            return;
        }
        for line_idx in (0..cursor_line).rev() {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.contains("/*") {
                self.view_mut().cursor.line = line_idx;
                // Position cursor at the '/' of '/*'
                if let Some(pos) = line.find("/*") {
                    let col = line[..pos].chars().count();
                    self.view_mut().cursor.col = col;
                } else {
                    self.view_mut().cursor.col = 0;
                }
                self.clamp_cursor_col();
                return;
            }
        }
    }

    /// Jump forward to next unmatched `#else` or `#endif` (`]#`).
    /// Uses depth tracking: `#if`/`#ifdef`/`#ifndef` increase depth,
    /// `#endif` decreases depth, `#else`/`#elif` match at depth 0.
    pub(crate) fn jump_preproc_forward(&mut self) {
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        let mut depth: i32 = 0;
        for line_idx in start..total {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if let Some(directive) = Self::preproc_directive(trimmed) {
                match directive {
                    PreprocKind::If => depth += 1,
                    PreprocKind::ElseElif => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                    }
                    PreprocKind::Endif => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                        depth -= 1;
                    }
                }
            }
        }
    }

    /// Jump backward to previous unmatched `#if` or `#else` (`[#`).
    /// Uses depth tracking: `#endif` increases depth,
    /// `#if`/`#ifdef`/`#ifndef` decrease depth, `#else`/`#elif` match at depth 0.
    pub(crate) fn jump_preproc_backward(&mut self) {
        let cursor_line = self.view().cursor.line;
        if cursor_line == 0 {
            return;
        }
        let mut depth: i32 = 0;
        for line_idx in (0..cursor_line).rev() {
            let line: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if let Some(directive) = Self::preproc_directive(trimmed) {
                match directive {
                    PreprocKind::Endif => depth += 1,
                    PreprocKind::ElseElif => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                    }
                    PreprocKind::If => {
                        if depth == 0 {
                            self.view_mut().cursor.line = line_idx;
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            return;
                        }
                        depth -= 1;
                    }
                }
            }
        }
    }

    /// Classify a trimmed line as a preprocessor directive kind.
    pub(crate) fn preproc_directive(trimmed: &str) -> Option<PreprocKind> {
        if !trimmed.starts_with('#') {
            return None;
        }
        // Strip '#' and optional whitespace after it
        let after_hash = trimmed[1..].trim_start();
        if after_hash.starts_with("ifdef")
            || after_hash.starts_with("ifndef")
            || after_hash.starts_with("if ")
            || after_hash.starts_with("if\t")
            || after_hash == "if"
        {
            Some(PreprocKind::If)
        } else if after_hash.starts_with("else") || after_hash.starts_with("elif") {
            Some(PreprocKind::ElseElif)
        } else if after_hash.starts_with("endif") {
            Some(PreprocKind::Endif)
        } else {
            None
        }
    }

    /// `do` (diff obtain): in a diff view, replace the current line in the active
    /// window with the corresponding line from the other diff window.
    pub(crate) fn diff_obtain(&mut self, changed: &mut bool) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => {
                self.message = "Not in diff mode".to_string();
                return;
            }
        };
        let active = self.active_window_id();
        let other = if active == a_win {
            b_win
        } else if active == b_win {
            a_win
        } else {
            self.message = "Current window is not part of a diff".to_string();
            return;
        };
        let cursor_line = self.view().cursor.line;
        // Get the diff status for the active window
        let diff_status = self
            .diff_results
            .get(&active)
            .and_then(|v| v.get(cursor_line))
            .cloned();
        match diff_status {
            Some(DiffLine::Same) => {
                self.message = "Line is the same in both files".to_string();
            }
            Some(DiffLine::Added) | Some(DiffLine::Removed) | Some(DiffLine::Padding) | None => {
                // Get the corresponding line from the other window
                // For simplicity, use the same line number from the other buffer
                if let Some(other_win) = self.windows.get(&other) {
                    let other_buf_id = other_win.buffer_id;
                    if let Some(other_state) = self.buffer_manager.get(other_buf_id) {
                        if cursor_line < other_state.buffer.len_lines() {
                            let other_line: String = other_state
                                .buffer
                                .content
                                .line(cursor_line)
                                .chars()
                                .collect();
                            // Replace the current line
                            let line_start = self.buffer().line_to_char(cursor_line);
                            let line_end = if cursor_line + 1 < self.buffer().len_lines() {
                                self.buffer().line_to_char(cursor_line + 1)
                            } else {
                                self.buffer().len_chars()
                            };
                            self.start_undo_group();
                            self.delete_with_undo(line_start, line_end);
                            self.insert_with_undo(line_start, &other_line);
                            self.finish_undo_group();
                            self.view_mut().cursor.col = 0;
                            self.clamp_cursor_col();
                            *changed = true;
                            self.compute_diff();
                        } else {
                            self.message = "No corresponding line in other file".to_string();
                        }
                    }
                }
            }
        }
    }

    /// `dp` (diff put): in a diff view, replace the corresponding line in the
    /// other diff window with the current line from the active window.
    pub(crate) fn diff_put(&mut self, changed: &mut bool) {
        let (a_win, b_win) = match self.diff_window_pair {
            Some(pair) => pair,
            None => {
                self.message = "Not in diff mode".to_string();
                return;
            }
        };
        let active = self.active_window_id();
        let other = if active == a_win {
            b_win
        } else if active == b_win {
            a_win
        } else {
            self.message = "Current window is not part of a diff".to_string();
            return;
        };
        let cursor_line = self.view().cursor.line;
        // Get the current line from the active buffer
        let current_line: String = self.buffer().content.line(cursor_line).chars().collect();
        // Replace the corresponding line in the other buffer
        if let Some(other_win) = self.windows.get(&other) {
            let other_buf_id = other_win.buffer_id;
            if let Some(other_state) = self.buffer_manager.get_mut(other_buf_id) {
                if cursor_line < other_state.buffer.len_lines() {
                    let line_start = other_state.buffer.line_to_char(cursor_line);
                    let line_end = if cursor_line + 1 < other_state.buffer.len_lines() {
                        other_state.buffer.line_to_char(cursor_line + 1)
                    } else {
                        other_state.buffer.len_chars()
                    };
                    other_state.buffer.delete_range(line_start, line_end);
                    other_state.buffer.insert(line_start, &current_line);
                    other_state.dirty = true;
                    *changed = true;
                    self.compute_diff();
                } else {
                    // Other buffer is shorter — append the line
                    let end = other_state.buffer.len_chars();
                    let needs_newline = end > 0 && other_state.buffer.content.char(end - 1) != '\n';
                    if needs_newline {
                        other_state.buffer.insert(end, "\n");
                    }
                    let end = other_state.buffer.len_chars();
                    other_state.buffer.insert(end, &current_line);
                    other_state.dirty = true;
                    *changed = true;
                    self.compute_diff();
                }
            }
        }
    }

    /// Apply an operator in blockwise mode (rectangle region).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_blockwise_operator(
        &mut self,
        operator: char,
        start_line: usize,
        end_line: usize,
        left_col: usize,
        right_col: usize,
        changed: &mut bool,
    ) {
        match operator {
            'd' => {
                // Delete the block region
                self.start_undo_group();
                let mut deleted_text = String::new();
                // Process lines in reverse to keep indices stable
                for line_idx in (start_line..=end_line).rev() {
                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let text_len = if line_len > 0
                        && self.buffer().content.char(line_start + line_len - 1) == '\n'
                    {
                        line_len - 1
                    } else {
                        line_len
                    };
                    let from = left_col.min(text_len);
                    let to = (right_col + 1).min(text_len);
                    if from < to {
                        let del: String = self
                            .buffer()
                            .content
                            .slice((line_start + from)..(line_start + to))
                            .chars()
                            .collect();
                        deleted_text = del + "\n" + &deleted_text;
                        self.delete_with_undo(line_start + from, line_start + to);
                    }
                }
                let reg = self.active_register();
                self.set_delete_register(reg, deleted_text, false);
                self.clear_selected_register();
                self.view_mut().cursor.line = start_line;
                self.view_mut().cursor.col = left_col;
                self.clamp_cursor_col();
                self.finish_undo_group();
                *changed = true;
            }
            'y' => {
                // Yank the block region
                let mut yanked = String::new();
                for line_idx in start_line..=end_line {
                    let line_start = self.buffer().line_to_char(line_idx);
                    let line_len = self.buffer().line_len_chars(line_idx);
                    let text_len = if line_len > 0
                        && self.buffer().content.char(line_start + line_len - 1) == '\n'
                    {
                        line_len - 1
                    } else {
                        line_len
                    };
                    let from = left_col.min(text_len);
                    let to = (right_col + 1).min(text_len);
                    if from < to {
                        let chunk: String = self
                            .buffer()
                            .content
                            .slice((line_start + from)..(line_start + to))
                            .chars()
                            .collect();
                        yanked.push_str(&chunk);
                    }
                    yanked.push('\n');
                }
                let reg = self.active_register();
                self.set_yank_register(reg, yanked, false);
                self.clear_selected_register();
            }
            _ => {
                // For other operators (c, ~, u, U, etc.), fall back to charwise
                let start = self.buffer().line_to_char(start_line) + left_col;
                let end_char = self.buffer().line_to_char(end_line) + right_col + 1;
                let max = self.buffer().len_chars();
                self.apply_charwise_operator(operator, start, end_char.min(max), changed);
            }
        }
    }

    /// Try to parse and execute a range filter command like `1,5!sort` or `.!cmd`.
    /// Returns Some(action) if it matched, None otherwise.
    pub(crate) fn try_execute_filter_command(&mut self, cmd: &str) -> Option<EngineAction> {
        // Match patterns: N,M!cmd  or  .!cmd  or  .,.+N!cmd
        // Split on '!' — if there's a range before and a command after, it's a filter.
        let bang_pos = cmd.find('!')?;
        let range_str = &cmd[..bang_pos];
        let filter_cmd = cmd[bang_pos + 1..].trim();
        if filter_cmd.is_empty() || range_str.is_empty() {
            return None;
        }
        // Parse the range. Support: N,M  .,.+N  N  .  %
        let (start_line, end_line) = self.parse_simple_range(range_str)?;
        // Extract the text from the range
        let total_lines = self.buffer().len_lines();
        let start = start_line.min(total_lines.saturating_sub(1));
        let end = end_line.min(total_lines.saturating_sub(1));
        let mut lines_text = String::new();
        for i in start..=end {
            let line: String = self.buffer().content.line(i).chars().collect();
            lines_text.push_str(&line);
        }
        // Pipe through the command
        #[cfg(not(test))]
        let result = {
            use std::io::Write;
            let mut child = match std::process::Command::new("sh")
                .arg("-c")
                .arg(filter_cmd)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    self.message = format!("Filter error: {}", e);
                    return Some(EngineAction::None);
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(lines_text.as_bytes());
            }
            match child.wait_with_output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        self.message = format!("Filter error: {}", stderr.trim());
                        return Some(EngineAction::None);
                    }
                    stdout
                }
                Err(e) => {
                    self.message = format!("Filter error: {}", e);
                    return Some(EngineAction::None);
                }
            }
        };
        #[cfg(test)]
        let result = lines_text.clone(); // No-op in tests

        // Replace the range with the result
        let range_start = self.buffer().line_to_char(start);
        let range_end = if end + 1 < total_lines {
            self.buffer().line_to_char(end + 1)
        } else {
            self.buffer().len_chars()
        };
        self.start_undo_group();
        self.delete_with_undo(range_start, range_end);
        self.insert_with_undo(range_start, &result);
        self.finish_undo_group();
        self.view_mut().cursor.line = start;
        self.view_mut().cursor.col = 0;
        let line_count = result.lines().count();
        self.message = format!("{} lines filtered", line_count);
        Some(EngineAction::None)
    }

    /// Parse a simple line range like "1,5", ".", ".,.+3", "%".
    /// Returns 0-indexed (start_line, end_line).
    pub(crate) fn parse_simple_range(&self, range: &str) -> Option<(usize, usize)> {
        let current_line = self.view().cursor.line;
        let last_line = self.buffer().len_lines().saturating_sub(1);

        if range == "%" {
            return Some((0, last_line));
        }
        if range == "." {
            return Some((current_line, current_line));
        }

        if let Some((left, right)) = range.split_once(',') {
            let start = self.parse_line_addr(left.trim(), current_line, last_line)?;
            let end = self.parse_line_addr(right.trim(), current_line, last_line)?;
            Some((start, end))
        } else {
            let line = self.parse_line_addr(range.trim(), current_line, last_line)?;
            Some((line, line))
        }
    }

    /// Parse a single line address: number (1-indexed), ".", "$", ".+N", ".-N".
    pub(crate) fn parse_line_addr(&self, addr: &str, current: usize, last: usize) -> Option<usize> {
        if addr == "." {
            return Some(current);
        }
        if addr == "$" {
            return Some(last);
        }
        if let Some(offset) = addr.strip_prefix(".+") {
            let n: usize = offset.parse().ok()?;
            return Some((current + n).min(last));
        }
        if let Some(offset) = addr.strip_prefix(".-") {
            let n: usize = offset.parse().ok()?;
            return Some(current.saturating_sub(n));
        }
        // Plain number (1-indexed)
        let n: usize = addr.parse().ok()?;
        if n == 0 {
            return None;
        }
        Some((n - 1).min(last))
    }
}
