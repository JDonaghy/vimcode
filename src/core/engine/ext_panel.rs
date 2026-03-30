use super::*;

impl Engine {
    // ─── Extension Panel helpers ────────────────────────────────────────────

    /// Compute the total number of flat items across all sections of the active extension panel.
    /// Check if a tree item is visible (all ancestors expanded).
    pub(crate) fn ext_panel_item_visible(
        &self,
        panel_name: &str,
        item: &plugin::ExtPanelItem,
        items: &[plugin::ExtPanelItem],
    ) -> bool {
        if item.parent_id.is_empty() {
            return true;
        }
        // Walk up the parent chain
        let mut pid = &item.parent_id;
        loop {
            if pid.is_empty() {
                return true;
            }
            // Find the parent item
            if let Some(parent) = items.iter().find(|i| i.id == *pid) {
                let is_expanded = self
                    .ext_panel_tree_expanded
                    .get(&(panel_name.to_string(), parent.id.clone()))
                    .copied()
                    .unwrap_or(parent.expanded);
                if !is_expanded {
                    return false;
                }
                pid = &parent.parent_id;
            } else {
                return true; // parent not found, show the item
            }
        }
    }

    /// Count visible items in a section (accounting for collapsed tree nodes).
    pub(crate) fn ext_panel_visible_count(
        &self,
        panel_name: &str,
        items: &[plugin::ExtPanelItem],
    ) -> usize {
        items
            .iter()
            .filter(|item| self.ext_panel_item_visible(panel_name, item, items))
            .count()
    }

    /// Return the indices of visible items in a section.
    pub fn ext_panel_visible_indices(
        &self,
        panel_name: &str,
        items: &[plugin::ExtPanelItem],
    ) -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.ext_panel_item_visible(panel_name, item, items))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn ext_panel_flat_len(&self) -> usize {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => return 0,
        };
        let reg = match self.ext_panels.get(&panel_name) {
            Some(r) => r,
            None => return 0,
        };
        let expanded = self.ext_panel_sections_expanded.get(&panel_name);
        let mut count = 0;
        for (si, section) in reg.sections.iter().enumerate() {
            count += 1; // section header
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.clone(), section.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    count += self.ext_panel_visible_count(&panel_name, items);
                }
            }
        }
        count
    }

    /// Given a flat index, return (section_index, item_index_within_section).
    /// If the flat index lands on a section header, item_index is `usize::MAX`.
    /// item_index refers to the original (unfiltered) index in the items Vec.
    pub fn ext_panel_flat_to_section(&self, flat: usize) -> Option<(usize, usize)> {
        let panel_name = self.ext_panel_active.clone()?;
        let reg = self.ext_panels.get(&panel_name)?;
        let expanded = self.ext_panel_sections_expanded.get(&panel_name);
        let mut pos = 0;
        for (si, section) in reg.sections.iter().enumerate() {
            if pos == flat {
                return Some((si, usize::MAX));
            }
            pos += 1;
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.clone(), section.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    let visible = self.ext_panel_visible_indices(&panel_name, items);
                    if flat < pos + visible.len() {
                        return Some((si, visible[flat - pos]));
                    }
                    pos += visible.len();
                }
            }
        }
        None
    }

    /// Find the flat index of an item by its ID within a specific section.
    /// Returns `None` if the panel, section, or item is not found.
    pub fn ext_panel_find_flat_index(
        &self,
        panel_name: &str,
        section_name: &str,
        item_id: &str,
    ) -> Option<usize> {
        let reg = self.ext_panels.get(panel_name)?;
        let expanded = self.ext_panel_sections_expanded.get(panel_name);
        let mut pos = 0;
        for (si, section) in reg.sections.iter().enumerate() {
            pos += 1; // section header
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.to_string(), section.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    let visible = self.ext_panel_visible_indices(panel_name, items);
                    if section == section_name {
                        for &vi in &visible {
                            if items[vi].id == item_id
                                || items[vi].id.starts_with(item_id)
                                || item_id.starts_with(&items[vi].id)
                            {
                                return Some(pos + visible.iter().position(|&x| x == vi).unwrap());
                            }
                        }
                    }
                    pos += visible.len();
                }
            } else if section == section_name {
                // Section is collapsed — can't find the item
                return None;
            }
        }
        None
    }

    /// Programmatically reveal an item in an extension panel: expand its section,
    /// set the selection to point at it, and adjust scroll.
    pub fn ext_panel_reveal_item(&mut self, panel_name: &str, section_name: &str, item_id: &str) {
        // Ensure the target section is expanded
        if let Some(reg) = self.ext_panels.get(panel_name) {
            if let Some(si) = reg.sections.iter().position(|s| s == section_name) {
                let expanded = self
                    .ext_panel_sections_expanded
                    .entry(panel_name.to_string())
                    .or_insert_with(|| vec![true; reg.sections.len()]);
                if let Some(v) = expanded.get_mut(si) {
                    *v = true;
                }
            }
        }
        // Find the flat index and set selection
        if let Some(flat_idx) = self.ext_panel_find_flat_index(panel_name, section_name, item_id) {
            self.ext_panel_selected = flat_idx;
            // Center the item in the viewport
            self.ext_panel_scroll_top = flat_idx.saturating_sub(5);
        }
    }

    /// Ensure the selected ext panel item is visible by adjusting scroll.
    /// `visible_rows` is the approximate number of rows visible in the panel viewport.
    pub(crate) fn ext_panel_ensure_visible(&mut self, visible_rows: usize) {
        let rows = if visible_rows == 0 { 20 } else { visible_rows };
        if self.ext_panel_selected < self.ext_panel_scroll_top {
            self.ext_panel_scroll_top = self.ext_panel_selected;
        } else if self.ext_panel_selected >= self.ext_panel_scroll_top + rows {
            self.ext_panel_scroll_top = self.ext_panel_selected.saturating_sub(rows - 1);
        }
    }

    /// Handle keyboard input for an extension panel.
    /// Returns `true` if the key was consumed.
    pub fn handle_ext_panel_key(&mut self, key: &str, _ctrl: bool, _unicode: Option<char>) -> bool {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => {
                self.ext_panel_has_focus = false;
                return true;
            }
        };

        // Any key closes help popup
        if self.ext_panel_help_open {
            self.ext_panel_help_open = false;
            return true;
        }

        match key {
            "q" | "Escape" => {
                self.ext_panel_has_focus = false;
            }
            "j" | "Down" => {
                let max = self.ext_panel_flat_len();
                if max > 0 && self.ext_panel_selected + 1 < max {
                    self.ext_panel_selected += 1;
                }
                self.ext_panel_ensure_visible(0);
            }
            "k" | "Up" => {
                if self.ext_panel_selected > 0 {
                    self.ext_panel_selected -= 1;
                }
                self.ext_panel_ensure_visible(0);
            }
            "g" => {
                self.ext_panel_selected = 0;
                self.ext_panel_scroll_top = 0;
            }
            "G" => {
                let max = self.ext_panel_flat_len();
                if max > 0 {
                    self.ext_panel_selected = max - 1;
                }
                self.ext_panel_ensure_visible(0);
            }
            "/" => {
                // Activate the input field for filtering/searching within the panel.
                self.ext_panel_input_active = true;
            }
            "Tab" => {
                // Toggle expand/collapse — works on section headers AND expandable tree items
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    if item_idx == usize::MAX {
                        // Section header: toggle section expand
                        let expanded = self
                            .ext_panel_sections_expanded
                            .entry(panel_name.clone())
                            .or_default();
                        while expanded.len() <= si {
                            expanded.push(true);
                        }
                        expanded[si] = !expanded[si];
                    } else {
                        // Item: toggle tree node expand if expandable
                        let reg = self.ext_panels.get(&panel_name).cloned();
                        if let Some(reg) = reg {
                            if let Some(section) = reg.sections.get(si) {
                                let key = (panel_name.clone(), section.clone());
                                let is_expandable = self
                                    .ext_panel_items
                                    .get(&key)
                                    .and_then(|items| items.get(item_idx))
                                    .map(|item| item.expandable)
                                    .unwrap_or(false);
                                if is_expandable {
                                    let item_id = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.id.clone())
                                        .unwrap_or_default();
                                    let default_expanded = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.expanded)
                                        .unwrap_or(false);
                                    let tree_key = (panel_name.clone(), item_id.clone());
                                    let currently = self
                                        .ext_panel_tree_expanded
                                        .get(&tree_key)
                                        .copied()
                                        .unwrap_or(default_expanded);
                                    self.ext_panel_tree_expanded.insert(tree_key, !currently);
                                    // Fire expand/collapse event
                                    let event = if currently {
                                        "panel_collapse"
                                    } else {
                                        "panel_expand"
                                    };
                                    let arg = format!(
                                        "{}|{}|{}||{}",
                                        panel_name, section, item_id, self.ext_panel_selected
                                    );
                                    self.plugin_event(event, &arg);
                                }
                            }
                        }
                    }
                }
            }
            "Return" => {
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    if item_idx == usize::MAX {
                        // Section header: toggle section expand
                        let expanded = self
                            .ext_panel_sections_expanded
                            .entry(panel_name.clone())
                            .or_default();
                        while expanded.len() <= si {
                            expanded.push(true);
                        }
                        expanded[si] = !expanded[si];
                    } else {
                        // Check if item is expandable — if so, toggle expand
                        let reg = self.ext_panels.get(&panel_name).cloned();
                        let mut toggled = false;
                        if let Some(ref reg) = reg {
                            if let Some(section) = reg.sections.get(si) {
                                let key = (panel_name.clone(), section.clone());
                                let is_expandable = self
                                    .ext_panel_items
                                    .get(&key)
                                    .and_then(|items| items.get(item_idx))
                                    .map(|item| item.expandable)
                                    .unwrap_or(false);
                                if is_expandable {
                                    let item_id = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.id.clone())
                                        .unwrap_or_default();
                                    let default_expanded = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.expanded)
                                        .unwrap_or(false);
                                    let tree_key = (panel_name.clone(), item_id.clone());
                                    let currently = self
                                        .ext_panel_tree_expanded
                                        .get(&tree_key)
                                        .copied()
                                        .unwrap_or(default_expanded);
                                    self.ext_panel_tree_expanded.insert(tree_key, !currently);
                                    let event = if currently {
                                        "panel_collapse"
                                    } else {
                                        "panel_expand"
                                    };
                                    let arg = format!(
                                        "{}|{}|{}||{}",
                                        panel_name, section, item_id, self.ext_panel_selected
                                    );
                                    self.plugin_event(event, &arg);
                                    toggled = true;
                                }
                            }
                        }
                        // If not expandable, fire panel_select
                        if !toggled {
                            if let Some(reg) = reg {
                                if let Some(section) = reg.sections.get(si) {
                                    let key = (panel_name.clone(), section.clone());
                                    let id = self
                                        .ext_panel_items
                                        .get(&key)
                                        .and_then(|items| items.get(item_idx))
                                        .map(|item| item.id.clone())
                                        .unwrap_or_default();
                                    let arg =
                                        format!("{}|{}|{}||{}", panel_name, section, id, item_idx);
                                    self.plugin_event("panel_select", &arg);
                                }
                            }
                        }
                    }
                }
            }
            "?" => {
                if self.ext_panel_help_bindings.contains_key(&panel_name) {
                    self.ext_panel_help_open = true;
                }
            }
            other => {
                // Check if the key matches an action button on the selected item
                let mut action_label = None;
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    if item_idx != usize::MAX {
                        let reg = self.ext_panels.get(&panel_name).cloned();
                        if let Some(reg) = &reg {
                            if let Some(section) = reg.sections.get(si) {
                                let key = (panel_name.clone(), section.clone());
                                if let Some(items) = self.ext_panel_items.get(&key) {
                                    if let Some(item) = items.get(item_idx) {
                                        for action in &item.actions {
                                            if action.key == other {
                                                action_label = Some(action.label.clone());
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Fire panel_action event
                if let Some((si, item_idx)) =
                    self.ext_panel_flat_to_section(self.ext_panel_selected)
                {
                    let reg = self.ext_panels.get(&panel_name).cloned();
                    if let Some(reg) = reg {
                        if let Some(section) = reg.sections.get(si) {
                            let key = (panel_name.clone(), section.clone());
                            let id = if item_idx != usize::MAX {
                                self.ext_panel_items
                                    .get(&key)
                                    .and_then(|items| items.get(item_idx))
                                    .map(|item| item.id.clone())
                                    .unwrap_or_default()
                            } else {
                                String::new()
                            };
                            // Use action label as key if matched, otherwise original key
                            let event_key = action_label.as_deref().unwrap_or(other);
                            let arg = format!(
                                "{}|{}|{}|{}|{}",
                                panel_name, section, id, event_key, self.ext_panel_selected
                            );
                            self.plugin_event("panel_action", &arg);
                        }
                    }
                }
            }
        }
        true
    }

    /// Handle double-click on an extension panel item.
    /// Fires `panel_double_click` event (same arg format as `panel_select`).
    pub fn handle_ext_panel_double_click(&mut self) {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => return,
        };
        if let Some((si, item_idx)) = self.ext_panel_flat_to_section(self.ext_panel_selected) {
            if item_idx != usize::MAX {
                let reg = self.ext_panels.get(&panel_name).cloned();
                if let Some(reg) = reg {
                    if let Some(section) = reg.sections.get(si) {
                        let key = (panel_name.clone(), section.clone());
                        let id = self
                            .ext_panel_items
                            .get(&key)
                            .and_then(|items| items.get(item_idx))
                            .map(|item| item.id.clone())
                            .unwrap_or_default();
                        let arg = format!(
                            "{}|{}|{}||{}",
                            panel_name, section, id, self.ext_panel_selected
                        );
                        self.plugin_event("panel_double_click", &arg);
                    }
                }
            }
        }
    }

    /// Open a context menu for an extension panel item.
    /// Fires `panel_context_menu` with the selected item info.
    pub fn open_ext_panel_context_menu(&mut self, x: u16, y: u16) {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => return,
        };
        if let Some((si, item_idx)) = self.ext_panel_flat_to_section(self.ext_panel_selected) {
            let reg = self.ext_panels.get(&panel_name).cloned();
            if let Some(reg) = reg {
                if let Some(section) = reg.sections.get(si) {
                    let key = (panel_name.clone(), section.clone());
                    let id = if item_idx != usize::MAX {
                        self.ext_panel_items
                            .get(&key)
                            .and_then(|items| items.get(item_idx))
                            .map(|item| item.id.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let arg = format!(
                        "{}|{}|{}||{}",
                        panel_name, section, id, self.ext_panel_selected
                    );
                    self.plugin_event("panel_context_menu", &arg);
                }
            }
        }
        let _ = (x, y); // Position reserved for future native menu rendering.
    }

    /// Handle keyboard input for the extension panel input field.
    /// Returns `true` if the key was consumed.
    pub fn handle_ext_panel_input_key(
        &mut self,
        key: &str,
        _ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        let panel_name = match &self.ext_panel_active {
            Some(n) => n.clone(),
            None => {
                self.ext_panel_input_active = false;
                return true;
            }
        };

        match key {
            "Escape" => {
                self.ext_panel_input_active = false;
            }
            "Return" => {
                // Fire panel_input event with the current text, then deactivate.
                let text = self
                    .ext_panel_input_text
                    .get(&panel_name)
                    .cloned()
                    .unwrap_or_default();
                let arg = format!("{}|||{}|", panel_name, text);
                self.plugin_event("panel_input", &arg);
                self.ext_panel_input_active = false;
            }
            "BackSpace" => {
                if let Some(text) = self.ext_panel_input_text.get_mut(&panel_name) {
                    text.pop();
                }
                // Fire panel_input on every change for live filtering.
                let text = self
                    .ext_panel_input_text
                    .get(&panel_name)
                    .cloned()
                    .unwrap_or_default();
                let arg = format!("{}|||{}|", panel_name, text);
                self.plugin_event("panel_input", &arg);
            }
            _ => {
                if let Some(ch) = unicode {
                    if !ch.is_control() {
                        self.ext_panel_input_text
                            .entry(panel_name.clone())
                            .or_default()
                            .push(ch);
                        // Fire panel_input on every change for live filtering.
                        let text = self
                            .ext_panel_input_text
                            .get(&panel_name)
                            .cloned()
                            .unwrap_or_default();
                        let arg = format!("{}|||{}|", panel_name, text);
                        self.plugin_event("panel_input", &arg);
                    }
                }
            }
        }
        true
    }

    // ── Panel hover popup methods ──────────────────────────────────────────

    /// Show a hover popup with rendered markdown for a sidebar panel item.
    pub fn show_panel_hover(
        &mut self,
        panel_name: &str,
        item_id: &str,
        item_index: usize,
        markdown: &str,
    ) {
        let rendered = crate::core::markdown::render_markdown(markdown);
        let links = Self::extract_hover_links(&rendered);
        // Dismiss any active editor hover to avoid overlapping popups.
        self.dismiss_editor_hover();
        self.panel_hover = Some(PanelHoverPopup {
            rendered,
            links,
            panel_name: panel_name.to_string(),
            item_id: item_id.to_string(),
            item_index,
        });
    }

    /// Schedule a delayed dismiss of the hover popup (250ms grace period).
    /// The popup stays visible until `poll_panel_hover` sees the deadline pass.
    /// If the mouse moves back onto the popup or item, call `cancel_panel_hover_dismiss`.
    pub fn dismiss_panel_hover(&mut self) {
        if self.panel_hover.is_some() && self.panel_hover_dismiss_at.is_none() {
            self.panel_hover_dismiss_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
        }
        // Always clear dwell so a new hover won't restart on the old item.
        self.panel_hover_dwell = None;
    }

    /// Immediately dismiss the hover popup with no delay.
    pub fn dismiss_panel_hover_now(&mut self) {
        self.panel_hover = None;
        self.panel_hover_dwell = None;
        self.panel_hover_dismiss_at = None;
    }

    /// Cancel a pending delayed dismiss (mouse returned to popup or item).
    pub fn cancel_panel_hover_dismiss(&mut self) {
        self.panel_hover_dismiss_at = None;
    }

    /// Track mouse movement over a sidebar panel item for dwell detection.
    /// Returns true if the dwell state changed (item changed).
    pub fn panel_hover_mouse_move(
        &mut self,
        panel_name: &str,
        item_id: &str,
        item_index: usize,
    ) -> bool {
        let _ = item_id;
        // If mouse returned to the item that spawned the current popup, cancel dismiss.
        if let Some(ref ph) = self.panel_hover {
            if ph.panel_name == panel_name && ph.item_index == item_index {
                self.panel_hover_dismiss_at = None;
                return false;
            }
        }
        if let Some((ref pn, idx, _)) = self.panel_hover_dwell {
            if pn == panel_name && idx == item_index {
                // Same dwell item. Only cancel dismiss if this item owns
                // the current popup (not if a *different* popup is lingering).
                let owns_popup = self
                    .panel_hover
                    .as_ref()
                    .is_some_and(|ph| ph.panel_name == panel_name && ph.item_index == item_index);
                if owns_popup {
                    self.panel_hover_dismiss_at = None;
                }
                return false; // Same item, dwell still running
            }
        }
        // Different item — schedule delayed dismiss for the active popup
        // (so it lingers while the user moves the mouse toward it) and start
        // dwell tracking on the new item.
        if self.panel_hover.is_some() && self.panel_hover_dismiss_at.is_none() {
            self.panel_hover_dismiss_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
        }
        // If no popup is showing, clear any stale dismiss.
        if self.panel_hover.is_none() {
            self.panel_hover_dismiss_at = None;
        }
        self.panel_hover_dwell = Some((
            panel_name.to_string(),
            item_index,
            std::time::Instant::now(),
        ));
        true
    }

    /// Called from poll/tick loops. Handles dwell-to-show and delayed dismiss.
    /// Returns true if a redraw is needed.
    pub fn poll_panel_hover(&mut self) -> bool {
        if self.settings.hover_delay == 0 {
            return false;
        }
        // Check delayed dismiss deadline.
        if let Some(deadline) = self.panel_hover_dismiss_at {
            if std::time::Instant::now() >= deadline {
                self.panel_hover = None;
                self.panel_hover_dismiss_at = None;
                return true; // redraw to remove popup
            }
        }

        let Some((ref panel_name, item_index, started)) = self.panel_hover_dwell else {
            return false;
        };
        if self.panel_hover.is_some() {
            return false; // Already showing
        }
        if started.elapsed() < std::time::Duration::from_millis(self.settings.hover_delay as u64) {
            return false; // Not yet
        }
        let panel_name = panel_name.clone();

        // Native source control panel hovers.
        if panel_name == "source_control" {
            if let Some(md) = self.sc_hover_markdown(item_index) {
                self.show_panel_hover(&panel_name, "", item_index, &md);
                return true;
            }
            self.panel_hover_dwell = None;
            return false;
        }

        // Extension panel: resolve item_id and check plugin registry.
        let item_id = self.resolve_panel_hover_item_id(&panel_name, item_index);
        let md = self
            .panel_hover_registry
            .get(&(panel_name.clone(), item_id.clone()))
            .cloned();
        if let Some(md) = md {
            self.show_panel_hover(&panel_name, &item_id, item_index, &md);
            return true;
        }
        // Prevent re-polling: clear dwell so we don't keep trying every tick.
        self.panel_hover_dwell = None;
        false
    }

    /// Generate hover markdown for a Source Control panel item at the given flat index.
    pub(crate) fn sc_hover_markdown(&self, flat_index: usize) -> Option<String> {
        let (section, idx) = self.sc_flat_to_section_idx(flat_index);

        // Section headers: show branch info on the "Staged Changes" header (section 0)
        if idx == usize::MAX {
            if section == 0 {
                // Branch info hover
                return self.sc_hover_branch_info();
            }
            return None; // Other headers: no hover
        }

        match section {
            // Staged/Unstaged file items
            0 | 1 => {
                let is_staged = section == 0;
                let files: Vec<&git::FileStatus> = if is_staged {
                    self.sc_file_statuses
                        .iter()
                        .filter(|f| f.staged.is_some())
                        .collect()
                } else {
                    self.sc_file_statuses
                        .iter()
                        .filter(|f| f.unstaged.is_some())
                        .collect()
                };
                let file = files.get(idx)?;
                self.sc_hover_file(file, is_staged)
            }
            // Log items
            3 => {
                let entry = self.sc_log.get(idx)?;
                self.sc_hover_log_entry(entry)
            }
            _ => None,
        }
    }

    /// Branch info hover (shown on the Staged Changes section header).
    pub(crate) fn sc_hover_branch_info(&self) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        let branch = git::current_branch(&cwd)?;
        let tracking = git::tracking_branch(&cwd).unwrap_or_else(|| "none".to_string());
        let mut md = format!("### {} `{}`\n\n", "\u{e725}", branch); // nf-dev-git_branch
        md.push_str(&format!("**Remote:** `{}`\n\n", tracking));
        if self.sc_ahead > 0 || self.sc_behind > 0 {
            md.push_str(&format!(
                "\u{2191}{} \u{2193}{}",
                self.sc_ahead, self.sc_behind
            ));
            if self.sc_ahead > 0 {
                md.push_str(" — commits to push");
            }
            if self.sc_behind > 0 {
                md.push_str(" — commits to pull");
            }
            md.push('\n');
        } else {
            md.push_str("Up to date with remote\n");
        }
        Some(md)
    }

    /// File hover: show status and diff stats.
    pub(crate) fn sc_hover_file(&self, file: &git::FileStatus, staged: bool) -> Option<String> {
        let status = if staged {
            file.staged.unwrap_or(git::StatusKind::Modified)
        } else {
            file.unstaged.unwrap_or(git::StatusKind::Modified)
        };
        let status_label = match status {
            git::StatusKind::Added => "Added",
            git::StatusKind::Modified => "Modified",
            git::StatusKind::Deleted => "Deleted",
            git::StatusKind::Renamed => "Renamed",
            git::StatusKind::Untracked => "Untracked",
        };
        let mut md = format!("### {}\n\n", file.path);
        md.push_str(&format!(
            "**Status:** {} ({})\n\n",
            status_label,
            if staged { "staged" } else { "unstaged" }
        ));
        // Get diff stats (blocking but fast for a single file)
        let cwd = std::env::current_dir().ok()?;
        if let Some(stat) = git::diff_stat_file(&cwd, &file.path, staged) {
            md.push_str("```\n");
            md.push_str(&stat);
            md.push_str("\n```\n");
        }
        Some(md)
    }

    /// Log entry hover: show commit details.
    pub(crate) fn sc_hover_log_entry(&self, entry: &git::GitLogEntry) -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        if let Some(detail) = git::commit_detail(&cwd, &entry.hash) {
            let mut md = String::new();
            // If we can build a commit URL, make the hash a clickable link.
            if let Some(url) = git::commit_url(&cwd, &detail.hash) {
                md.push_str(&format!("### [{}]({})\n\n", detail.hash, url));
            } else {
                md.push_str(&format!("### `{}`\n\n", detail.hash));
            }
            md.push_str(&format!("**Author:** {}\n\n", detail.author));
            md.push_str(&format!("**Date:** {}\n\n", detail.date));
            if !detail.message.is_empty() {
                md.push_str(&detail.message);
                md.push_str("\n\n");
            }
            if !detail.stat.is_empty() {
                md.push_str("```\n");
                md.push_str(&detail.stat);
                md.push_str("\n```\n");
            }
            Some(md)
        } else {
            // Fallback to basic info
            Some(format!("### `{}`\n\n{}\n", entry.hash, entry.message))
        }
    }

    /// Resolve the item_id for a given panel name and flat index.
    pub(crate) fn resolve_panel_hover_item_id(
        &self,
        panel_name: &str,
        flat_index: usize,
    ) -> String {
        let Some(reg) = self.ext_panels.get(panel_name) else {
            return String::new();
        };
        let expanded = self.ext_panel_sections_expanded.get(panel_name);
        let mut idx = 0usize;
        for (si, section_name) in reg.sections.iter().enumerate() {
            if idx == flat_index {
                return String::new(); // It's a section header
            }
            idx += 1;
            let is_expanded = expanded.and_then(|v| v.get(si)).copied().unwrap_or(true);
            if is_expanded {
                let key = (panel_name.to_string(), section_name.clone());
                if let Some(items) = self.ext_panel_items.get(&key) {
                    for item in items {
                        if idx == flat_index {
                            return item.id.clone();
                        }
                        idx += 1;
                    }
                }
            }
        }
        String::new()
    }

    // ── Editor hover popup ────────────────────────────────────────────────────

    /// Trigger the editor hover popup at the current cursor position.
    /// Assembles content from multiple providers: diagnostics, annotations,
    /// plugin hover content, and LSP hover. Also requests LSP hover async.
    pub fn trigger_editor_hover_at_cursor(&mut self) {
        let line = self.cursor().line;
        let col = self.cursor().col;
        self.show_editor_hover_at(line, col, true, true);
    }

    /// Check if any diagnostic touches the given line.
    pub fn has_diagnostic_on_line(&self, line: usize) -> bool {
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                return diags.iter().any(|d| {
                    let sl = d.range.start.line as usize;
                    let el = d.range.end.line as usize;
                    line >= sl && line <= el
                });
            }
        }
        false
    }

    /// Trigger editor hover for a diagnostic gutter click on the given line.
    /// Shows ALL diagnostics that touch this line, regardless of column.
    pub fn trigger_editor_hover_for_line(&mut self, line: usize) {
        let mut sections: Vec<String> = Vec::new();
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                for diag in diags {
                    let start_line = diag.range.start.line as usize;
                    let end_line = diag.range.end.line as usize;
                    if line >= start_line && line <= end_line {
                        let severity = match diag.severity {
                            crate::core::lsp::DiagnosticSeverity::Error => "Error",
                            crate::core::lsp::DiagnosticSeverity::Warning => "Warning",
                            crate::core::lsp::DiagnosticSeverity::Information => "Info",
                            crate::core::lsp::DiagnosticSeverity::Hint => "Hint",
                        };
                        let source_str = diag
                            .source
                            .as_deref()
                            .map(|s| format!(" ({})", s))
                            .unwrap_or_default();
                        sections.push(format!(
                            "**{}**{}\n\n`{}`",
                            severity, source_str, diag.message
                        ));
                    }
                }
            }
        }
        if !sections.is_empty() {
            let combined = sections.join("\n\n---\n\n");
            self.show_editor_hover(
                line,
                0,
                &combined,
                EditorHoverSource::Diagnostic,
                true,
                false,
            );
        }
    }

    /// Assemble and show the editor hover popup at a given buffer position.
    /// If `request_lsp` is true, also fires an LSP hover request (async).
    /// If `take_focus` is true, the popup grabs keyboard focus (j/k scroll, Tab links).
    pub fn show_editor_hover_at(
        &mut self,
        line: usize,
        col: usize,
        request_lsp: bool,
        take_focus: bool,
    ) {
        self.show_editor_hover_at_inner(line, col, request_lsp, take_focus, true);
    }

    /// Inner implementation — `include_annotations` controls whether annotation
    /// hover content is included (false for mouse dwell over code text, true for
    /// keyboard triggers and mouse dwell over ghost text).
    pub(crate) fn show_editor_hover_at_inner(
        &mut self,
        line: usize,
        col: usize,
        request_lsp: bool,
        take_focus: bool,
        include_annotations: bool,
    ) {
        let mut sections: Vec<(EditorHoverSource, String)> = Vec::new();

        // 1. Diagnostics at this position
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                for diag in diags {
                    let start_line = diag.range.start.line as usize;
                    let end_line = diag.range.end.line as usize;
                    let start_col = diag.range.start.character as usize;
                    let end_col = diag.range.end.character as usize;
                    let in_range = if start_line == end_line {
                        line == start_line && col >= start_col && col <= end_col
                    } else {
                        (line == start_line && col >= start_col)
                            || (line == end_line && col <= end_col)
                            || (line > start_line && line < end_line)
                    };
                    if in_range {
                        let severity = match diag.severity {
                            crate::core::lsp::DiagnosticSeverity::Error => "Error",
                            crate::core::lsp::DiagnosticSeverity::Warning => "Warning",
                            crate::core::lsp::DiagnosticSeverity::Information => "Info",
                            crate::core::lsp::DiagnosticSeverity::Hint => "Hint",
                        };
                        let source_str = diag
                            .source
                            .as_deref()
                            .map(|s| format!(" ({})", s))
                            .unwrap_or_default();
                        let md = format!("**{}**{}\n\n`{}`", severity, source_str, diag.message);
                        sections.push((EditorHoverSource::Diagnostic, md));
                    }
                }
            }
        }

        // 2. Plugin hover content for this line (only when over annotation area)
        if include_annotations {
            if let Some(md) = self.editor_hover_content.get(&line) {
                sections.push((EditorHoverSource::Annotation, md.clone()));
            }
        }

        // 3. Line annotation text (simple inline blame, etc.)
        if include_annotations && sections.is_empty() {
            if let Some(annotation) = self.line_annotations.get(&line) {
                if !annotation.is_empty() {
                    // Query plugin hover providers for annotation content
                    let md = format!("`{}`", annotation.trim());
                    sections.push((EditorHoverSource::Annotation, md));
                }
            }
        }

        // 4. Existing LSP hover text (if already available)
        if let Some(hover_text) = &self.lsp_hover_text {
            sections.push((EditorHoverSource::Lsp, hover_text.clone()));
        }

        // Build the popup if we have content
        let has_lsp_section = sections
            .iter()
            .any(|(s, _)| matches!(s, EditorHoverSource::Lsp));
        let is_annotation_only = !sections.is_empty()
            && sections
                .iter()
                .all(|(s, _)| matches!(s, EditorHoverSource::Annotation));
        if !sections.is_empty() {
            let combined = sections
                .iter()
                .map(|(_, md)| md.as_str())
                .collect::<Vec<_>>()
                .join("\n\n---\n\n");
            let source = sections[0].0.clone();
            // Annotation-only hovers don't auto-focus — user clicks to focus.
            let focus = take_focus && !is_annotation_only;
            self.show_editor_hover(line, col, &combined, source, focus, false);
        } else if take_focus {
            self.editor_hover_has_focus = true;
        }

        // Request LSP hover only if we don't already have LSP content and
        // the popup isn't purely annotation-sourced (avoids LSP null response
        // dismissing the annotation popup).
        if request_lsp && !is_annotation_only && !has_lsp_section {
            // For mouse hover: skip if LSP already returned null for this position.
            if !take_focus && self.lsp_hover_null_pos == Some((line, col)) {
                return;
            }
            self.lsp_hover_request_pos = Some((line, col));
            let prev_pending = self.lsp_pending_hover;
            self.lsp_request_hover_at(line, col);
            let sent_new =
                self.lsp_pending_hover != prev_pending && self.lsp_pending_hover.is_some();
            if sent_new && take_focus && self.editor_hover.is_none() {
                // Explicit keyboard hover (gh/:hover) — show "Loading..." immediately.
                self.show_editor_hover(
                    line,
                    col,
                    "Loading...",
                    EditorHoverSource::Lsp,
                    true,
                    false,
                );
                // Auto-dismiss after 3s if LSP never responds.
                self.editor_hover_dismiss_at =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            }
            // Mouse hover: no "Loading..." — popup appears only if LSP returns content.
        }
    }

    /// Show an editor hover popup with the given markdown content.
    /// If `take_focus` is true, the popup grabs keyboard focus (for `gh` / `:hover`).
    pub fn show_editor_hover(
        &mut self,
        anchor_line: usize,
        anchor_col: usize,
        markdown: &str,
        source: EditorHoverSource,
        take_focus: bool,
        add_goto_links: bool,
    ) {
        let mut rendered = crate::core::markdown::render_markdown(markdown);
        let mut links = Self::extract_hover_links(&rendered);

        // Append "Go to" navigation links after actual LSP content (vim mode only).
        if add_goto_links && !self.is_vscode_mode() {
            let goto = self.lsp_goto_links();
            if !goto.is_empty() {
                use crate::core::markdown::{MdSpan, MdStyle};
                // Separator line.
                rendered.lines.push(String::new());
                rendered.spans.push(Vec::new());
                rendered.code_highlights.push(Vec::new());
                // Build: "Go to Definition (:gd) | Type Definition (:gy) | ..."
                // "Go to" is default fg; labels are link-colored and clickable.
                let nav_line_idx = rendered.lines.len();
                let mut nav_text = String::from("Go to ");
                let mut nav_spans = Vec::new();
                for (i, (label, keybind, url)) in goto.iter().enumerate() {
                    if i > 0 {
                        nav_text.push_str(" | ");
                    }
                    let start = nav_text.len();
                    nav_text.push_str(label);
                    let end = nav_text.len();
                    nav_spans.push(MdSpan {
                        start_byte: start,
                        end_byte: end,
                        style: MdStyle::Link,
                    });
                    links.push((nav_line_idx, start, end, url.to_string()));
                    nav_text.push_str(&format!(" (:{})", keybind));
                }
                rendered.lines.push(nav_text);
                rendered.spans.push(nav_spans);
                rendered.code_highlights.push(Vec::new());
            }
        }

        let popup_width = rendered
            .lines
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(10)
            .clamp(10, 80);
        let (frozen_scroll_top, frozen_scroll_left) = {
            let v = self.view();
            (v.scroll_top, v.scroll_left)
        };
        // Dismiss any active panel hover to avoid overlapping popups.
        self.dismiss_panel_hover_now();
        self.editor_hover = Some(EditorHoverPopup {
            rendered,
            links,
            anchor_line,
            anchor_col,
            source,
            scroll_top: 0,
            focused_link: None,
            popup_width,
            frozen_scroll_top,
            frozen_scroll_left,
            selection: None,
        });
        if take_focus {
            self.editor_hover_has_focus = true;
        }
    }

    /// Dismiss the editor hover popup.
    pub fn dismiss_editor_hover(&mut self) {
        self.editor_hover = None;
        self.editor_hover_has_focus = false;
        self.editor_hover_dwell = None;
        self.editor_hover_dismiss_at = None;
        self.lsp_hover_text = None;
    }

    /// Dismiss editor hover with a delay (for mouse leave events).
    #[allow(dead_code)]
    pub fn dismiss_editor_hover_delayed(&mut self) {
        if self.editor_hover.is_some() && self.editor_hover_dismiss_at.is_none() {
            self.editor_hover_dismiss_at =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(350));
        }
        self.editor_hover_dwell = None;
    }

    /// Cancel a pending delayed editor hover dismiss.
    #[allow(dead_code)]
    pub fn cancel_editor_hover_dismiss(&mut self) {
        self.editor_hover_dismiss_at = None;
    }

    /// Handle keyboard input when the editor hover popup has focus.
    pub fn handle_editor_hover_key(&mut self, key: &str, ctrl: bool) {
        match key {
            "y" | "Y" => {
                self.copy_hover_selection();
            }
            "c" if ctrl => {
                self.copy_hover_selection();
            }
            "Escape" | "q" => {
                self.dismiss_editor_hover();
            }
            "Tab" => {
                // Cycle to next link
                if let Some(hover) = &mut self.editor_hover {
                    if !hover.links.is_empty() {
                        hover.focused_link = Some(match hover.focused_link {
                            Some(i) => (i + 1) % hover.links.len(),
                            None => 0,
                        });
                    }
                }
            }
            "ISO_Left_Tab" | "BackTab" => {
                // Cycle to previous link
                if let Some(hover) = &mut self.editor_hover {
                    if !hover.links.is_empty() {
                        hover.focused_link = Some(match hover.focused_link {
                            Some(0) | None => hover.links.len() - 1,
                            Some(i) => i - 1,
                        });
                    }
                }
            }
            "Return" => {
                // Open focused link
                let url = self.editor_hover.as_ref().and_then(|h| {
                    h.focused_link
                        .and_then(|i| h.links.get(i).map(|(_, _, _, u)| u.clone()))
                });
                if let Some(url) = url {
                    if url.starts_with("command:") {
                        self.execute_hover_goto(&url);
                    } else {
                        self.open_url(&url);
                        self.dismiss_editor_hover();
                    }
                } else {
                    self.dismiss_editor_hover();
                }
            }
            "j" | "Down" => {
                // Scroll down — stop when last line is visible
                if let Some(hover) = &mut self.editor_hover {
                    let max_scroll = hover.rendered.lines.len().saturating_sub(20);
                    if hover.scroll_top < max_scroll {
                        hover.scroll_top += 1;
                    }
                }
            }
            "k" | "Up" => {
                // Scroll up
                if let Some(hover) = &mut self.editor_hover {
                    if hover.scroll_top > 0 {
                        hover.scroll_top -= 1;
                    }
                }
            }
            // Ignore bare modifier keys (GTK sends these as separate key events)
            "Control_L" | "Control_R" | "Shift_L" | "Shift_R" | "Alt_L" | "Alt_R" | "Super_L"
            | "Super_R" | "Meta_L" | "Meta_R" | "ISO_Level3_Shift" => {}
            _ => {
                // Any other key dismisses and passes through
                self.dismiss_editor_hover();
            }
        }
    }

    /// Track mouse movement for editor hover dwell detection.
    /// Call from backends on mouse motion over the editor area.
    /// Only triggers on word characters (identifiers), not whitespace or operators.
    /// Called by backends when the mouse moves over the editor area.
    /// `mouse_on_popup` should be true if the mouse is currently over the hover popup rect.
    pub fn editor_hover_mouse_move(&mut self, line: usize, col: usize, mouse_on_popup: bool) {
        if self.settings.hover_delay == 0 {
            return;
        }
        // If hover popup is already visible and focused, don't interfere
        if self.editor_hover_has_focus {
            return;
        }
        // Find the word boundaries under the cursor (if any)
        let (word_range, line_char_len) = {
            let buf = self.buffer();
            if line < buf.len_lines() {
                let line_text: String = buf.content.line(line).chars().collect();
                let chars: Vec<char> = line_text.chars().collect();
                let char_len =
                    chars
                        .len()
                        .saturating_sub(if chars.last() == Some(&'\n') { 1 } else { 0 });
                let wr = if col < chars.len() && (chars[col].is_alphanumeric() || chars[col] == '_')
                {
                    let mut start = col;
                    while start > 0
                        && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_')
                    {
                        start -= 1;
                    }
                    let mut end = col + 1;
                    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                        end += 1;
                    }
                    Some((start, end))
                } else {
                    None
                };
                (wr, char_len)
            } else {
                (None, 0)
            }
        };

        // Annotation hover content only counts when the mouse is past the end
        // of the actual line text (i.e. over the ghost text region).
        let on_annotation = col >= line_char_len
            && (self.editor_hover_content.contains_key(&line)
                || self.line_annotations.contains_key(&line));

        // Check if we're on the same word as the current popup
        if let Some(hover) = &self.editor_hover {
            // If popup is anchored to this line and mouse is on annotation, keep it
            if hover.anchor_line == line && on_annotation {
                return;
            }
            if let Some((start, end)) = word_range {
                if hover.anchor_line == line && hover.anchor_col >= start && hover.anchor_col < end
                {
                    // Still on the popup's word — nothing to do
                    return;
                }
            }
            // Not on the popup's word — but if mouse is on the popup itself, keep it
            if mouse_on_popup {
                return;
            }
            // Off both word and popup — dismiss (no cooldown for natural mouse-off)
            self.editor_hover = None;
            self.editor_hover_has_focus = false;
            self.editor_hover_dwell = None;
            self.editor_hover_dismiss_at = None;
            self.lsp_hover_text = None;
            return;
        }

        // No popup visible — handle dwell logic
        if word_range.is_none() && !on_annotation {
            self.editor_hover_dwell = None;
            return;
        }
        // Check if we're still on the same word/line as the current dwell
        if let Some((dl, dc, _)) = &self.editor_hover_dwell {
            if *dl == line {
                // If mouse is on annotation area and no word boundary, stay dwelling
                if on_annotation && word_range.is_none() {
                    return;
                }
                if let Some((start, end)) = word_range {
                    if *dc >= start && *dc < end {
                        // Same word — keep dwelling
                        return;
                    }
                }
            }
        }
        // New word — start fresh dwell timer and clear null-hover suppression.
        self.lsp_hover_null_pos = None;
        self.editor_hover_dwell = Some((line, col, std::time::Instant::now()));
    }

    /// Scroll the editor hover popup by the given delta (positive = down, negative = up).
    /// Returns true if the popup was scrolled.
    pub fn editor_hover_scroll(&mut self, delta: i32) -> bool {
        if let Some(hover) = &mut self.editor_hover {
            let max_scroll = hover.rendered.lines.len().saturating_sub(20);
            if delta > 0 {
                let new = (hover.scroll_top + delta as usize).min(max_scroll);
                if new != hover.scroll_top {
                    hover.scroll_top = new;
                    return true;
                }
            } else {
                let new = hover.scroll_top.saturating_sub((-delta) as usize);
                if new != hover.scroll_top {
                    hover.scroll_top = new;
                    return true;
                }
            }
        }
        false
    }

    /// Give the editor hover popup keyboard focus (e.g. on click).
    pub fn editor_hover_focus(&mut self) {
        if self.editor_hover.is_some() {
            self.editor_hover_has_focus = true;
        }
    }

    /// Extract the selected text from the editor hover popup (or all text if no selection).
    /// Returns `None` if there is no hover popup or content is empty.
    pub fn hover_selection_text(&self) -> Option<String> {
        let hover = self.editor_hover.as_ref()?;
        let text = if let Some(ref sel) = hover.selection {
            sel.extract_text(&hover.rendered.lines)
        } else {
            hover.rendered.lines.join("\n")
        };
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Copy the selected text from the editor hover popup to the clipboard.
    /// If no selection is active, copies all popup text.
    /// Uses the engine's `clipboard_write` callback (set by TUI backend).
    /// GTK backend should call `hover_selection_text()` and use its own clipboard.
    pub fn copy_hover_selection(&mut self) {
        let text = match self.hover_selection_text() {
            Some(t) => t,
            None => return,
        };
        if let Some(ref cb) = self.clipboard_write {
            if cb(&text).is_ok() {
                self.message = "Hover text copied".to_string();
                return;
            }
        }
        self.message = "Clipboard unavailable".to_string();
    }

    /// Start a text selection in the editor hover popup at the given content position.
    pub fn editor_hover_start_selection(&mut self, line: usize, col: usize) {
        if let Some(hover) = &mut self.editor_hover {
            hover.selection = Some(HoverSelection {
                anchor_line: line,
                anchor_col: col,
                active_line: line,
                active_col: col,
            });
        }
    }

    /// Extend the text selection in the editor hover popup to the given content position.
    pub fn editor_hover_extend_selection(&mut self, line: usize, col: usize) {
        if let Some(hover) = &mut self.editor_hover {
            if let Some(sel) = &mut hover.selection {
                sel.active_line = line;
                sel.active_col = col;
            }
        }
    }

    /// Poll editor hover dwell and delayed dismiss timers.
    /// Call from backends in the event loop tick.
    pub fn poll_editor_hover(&mut self) -> bool {
        if self.settings.hover_delay == 0 {
            return false;
        }
        let mut changed = false;
        // Check dwell timeout
        if let Some((line, col, start)) = self.editor_hover_dwell {
            if start.elapsed() >= std::time::Duration::from_millis(self.settings.hover_delay as u64)
            {
                self.editor_hover_dwell = None;
                // Re-validate position: on a word character or annotation ghost text
                let (on_annotation, on_word) = {
                    let buf = self.buffer();
                    let line_char_len = if line < buf.len_lines() {
                        let lt: String = buf.content.line(line).chars().collect();
                        let chars: Vec<char> = lt.chars().collect();
                        chars
                            .len()
                            .saturating_sub(if chars.last() == Some(&'\n') { 1 } else { 0 })
                    } else {
                        0
                    };
                    let ann = col >= line_char_len
                        && (self.editor_hover_content.contains_key(&line)
                            || self.line_annotations.contains_key(&line));
                    let word = if !ann && line < buf.len_lines() {
                        let line_text: String = buf.content.line(line).chars().collect();
                        line_text
                            .chars()
                            .nth(col)
                            .is_some_and(|c| c.is_alphanumeric() || c == '_')
                    } else {
                        false
                    };
                    (ann, word)
                };
                if on_annotation || on_word {
                    self.show_editor_hover_at_inner(line, col, true, false, on_annotation);
                    changed = true;
                }
            }
        }
        // Check delayed dismiss
        if let Some(deadline) = self.editor_hover_dismiss_at {
            if std::time::Instant::now() >= deadline {
                self.dismiss_editor_hover();
                changed = true;
            }
        }
        changed
    }

    /// Check if there's a diagnostic at the given position.
    #[allow(dead_code)]
    pub(crate) fn has_diagnostic_at(&self, line: usize, col: usize) -> bool {
        if let Some(path) = self.active_buffer_path() {
            if let Some(diags) = self.lsp_diagnostics.get(&path) {
                for diag in diags {
                    let sl = diag.range.start.line as usize;
                    let el = diag.range.end.line as usize;
                    let sc = diag.range.start.character as usize;
                    let ec = diag.range.end.character as usize;
                    let in_range = if sl == el {
                        line == sl && col >= sc && col <= ec
                    } else {
                        (line == sl && col >= sc)
                            || (line == el && col <= ec)
                            || (line > sl && line < el)
                    };
                    if in_range {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Open a URL in the default browser.
    pub(crate) fn open_url(&self, url: &str) {
        if !is_safe_url(url) {
            return;
        }
        #[cfg(not(test))]
        {
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open")
                    .arg(url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = std::process::Command::new("xdg-open")
                    .arg(url)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        }
    }

    /// Get the file path of the active buffer (if it has one).
    pub(crate) fn active_buffer_path(&self) -> Option<PathBuf> {
        self.buffer_manager
            .get(self.active_window().buffer_id)
            .and_then(|bs| bs.file_path.clone())
    }

    /// Handle LSP hover response by updating the editor hover popup.
    /// Called when the hover response arrives asynchronously.
    pub fn update_editor_hover_with_lsp(&mut self, hover_text: &str) {
        if let Some(hover) = &self.editor_hover {
            let anchor_line = hover.anchor_line;
            let anchor_col = hover.anchor_col;
            let had_focus = self.editor_hover_has_focus;

            // Rebuild: diagnostics at this position + new LSP text (replaces any old LSP content)
            let mut sections: Vec<String> = Vec::new();

            // Re-collect diagnostics for this anchor position
            if let Some(path) = self.active_buffer_path() {
                if let Some(diags) = self.lsp_diagnostics.get(&path) {
                    for diag in diags {
                        let sl = diag.range.start.line as usize;
                        let el = diag.range.end.line as usize;
                        let sc = diag.range.start.character as usize;
                        let ec = diag.range.end.character as usize;
                        let in_range = if sl == el {
                            anchor_line == sl && anchor_col >= sc && anchor_col <= ec
                        } else {
                            (anchor_line == sl && anchor_col >= sc)
                                || (anchor_line == el && anchor_col <= ec)
                                || (anchor_line > sl && anchor_line < el)
                        };
                        if in_range {
                            let severity = match diag.severity {
                                crate::core::lsp::DiagnosticSeverity::Error => "Error",
                                crate::core::lsp::DiagnosticSeverity::Warning => "Warning",
                                crate::core::lsp::DiagnosticSeverity::Information => "Info",
                                crate::core::lsp::DiagnosticSeverity::Hint => "Hint",
                            };
                            let source_str = diag
                                .source
                                .as_deref()
                                .map(|s| format!(" ({})", s))
                                .unwrap_or_default();
                            sections.push(format!(
                                "**{}**{}\n\n`{}`",
                                severity, source_str, diag.message
                            ));
                        }
                    }
                }
            }

            // Add LSP hover text
            if !hover_text.is_empty() {
                sections.push(hover_text.to_string());
            }

            let combined = sections.join("\n\n---\n\n");
            self.show_editor_hover(
                anchor_line,
                anchor_col,
                &combined,
                EditorHoverSource::Lsp,
                had_focus,
                true,
            );
        } else {
            // No existing popup — create one from LSP content
            let line = self.cursor().line;
            let col = self.cursor().col;
            let had_focus = self.editor_hover_has_focus;
            self.show_editor_hover(
                line,
                col,
                hover_text,
                EditorHoverSource::Lsp,
                had_focus,
                true,
            );
        }
    }

    /// Handle keyboard input for the Extensions sidebar panel.
    /// Returns `true` if the key was consumed.
    pub fn handle_ext_sidebar_key(
        &mut self,
        key: &str,
        _ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        // Search input active — route printable chars to query
        if self.ext_sidebar_input_active {
            match key {
                "Escape" => {
                    self.ext_sidebar_input_active = false;
                }
                "BackSpace" => {
                    self.ext_sidebar_query.pop();
                    self.ext_sidebar_selected = 0;
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.ext_sidebar_query.push(ch);
                            self.ext_sidebar_selected = 0;
                        }
                    }
                }
            }
            return true;
        }

        match key {
            "q" | "Escape" => {
                self.ext_sidebar_has_focus = false;
                true
            }
            "/" => {
                self.ext_sidebar_input_active = true;
                true
            }
            "r" => {
                self.ext_refresh();
                true
            }
            "Tab" => {
                // Toggle the section the cursor is in
                let (in_installed, _) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    self.ext_sidebar_sections_expanded[0] = !self.ext_sidebar_sections_expanded[0];
                } else {
                    self.ext_sidebar_sections_expanded[1] = !self.ext_sidebar_sections_expanded[1];
                }
                true
            }
            "j" | "Down" => {
                let total = self.ext_flat_item_count();
                if total > 0 {
                    self.ext_sidebar_selected = (self.ext_sidebar_selected + 1).min(total - 1);
                }
                true
            }
            "k" | "Up" => {
                self.ext_sidebar_selected = self.ext_sidebar_selected.saturating_sub(1);
                true
            }
            "Return" => {
                self.ext_open_selected_readme();
                true
            }
            "i" => {
                // Install the selected extension
                let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = &m.name;
                        self.message =
                            format!("Extension '{name}' is already installed. Use d to remove.");
                    }
                } else {
                    let available = self.ext_available_items();
                    let avail_idx = idx;
                    if avail_idx < available.len() {
                        let base_url = self.resolve_registry_base_url(&available[avail_idx]);
                        let name = available[avail_idx].name.clone();
                        let display = if available[avail_idx].display_name.is_empty() {
                            name.clone()
                        } else {
                            available[avail_idx].display_name.clone()
                        };
                        self.ext_install_from_registry(&name);
                        // Try to open README after install
                        let readme_path = paths::vimcode_config_dir()
                            .join("extensions")
                            .join(&name)
                            .join("README.md");
                        let content = std::fs::read_to_string(&readme_path)
                            .ok()
                            .or_else(|| registry::fetch_readme(&base_url, &name));
                        if let Some(content) = content {
                            self.open_markdown_preview_in_tab(&content, &display);
                        }
                        // Move cursor to the newly installed item.
                        self.ext_sidebar_sections_expanded[0] = true;
                        let new_installed = self.ext_installed_items();
                        self.ext_sidebar_selected = new_installed
                            .iter()
                            .position(|m| m.name == name)
                            .unwrap_or(0);
                    }
                }
                true
            }
            "d" => {
                let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = m.name.clone();
                        self.ext_show_remove_dialog(&name);
                    }
                }
                true
            }
            "u" => {
                // Update the selected installed extension
                let (in_installed, idx) = self.ext_selected_to_section(self.ext_sidebar_selected);
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = m.name.clone();
                        if self.ext_has_update(&name) {
                            self.ext_update_one(&name);
                        } else {
                            self.message = format!("Extension '{name}' is already up to date");
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Returns the filtered list of installed extension manifests.
    pub fn ext_installed_items(&self) -> Vec<crate::core::extensions::ExtensionManifest> {
        let q = self.ext_sidebar_query.to_lowercase();
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| self.extension_state.is_installed(&m.name))
            .filter(|m| {
                q.is_empty()
                    || m.name.to_lowercase().contains(&q)
                    || m.display_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Returns the filtered list of available (not yet installed) extension manifests.
    pub fn ext_available_items(&self) -> Vec<crate::core::extensions::ExtensionManifest> {
        let q = self.ext_sidebar_query.to_lowercase();
        self.ext_available_manifests()
            .into_iter()
            .filter(|m| !self.extension_state.is_installed(&m.name))
            .filter(|m| {
                q.is_empty()
                    || m.name.to_lowercase().contains(&q)
                    || m.display_name.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Total number of flat items in the sidebar (installed + available, respecting collapse).
    pub(crate) fn ext_flat_item_count(&self) -> usize {
        let installed = if self.ext_sidebar_sections_expanded[0] {
            self.ext_installed_items().len()
        } else {
            0
        };
        let available = if self.ext_sidebar_sections_expanded[1] {
            self.ext_available_items().len()
        } else {
            0
        };
        installed + available
    }

    /// Map the flat selected index to (section, index_within_section),
    /// accounting for collapsed sections.
    /// Returns `(true, idx)` for installed items, `(false, idx)` for available.
    pub(crate) fn ext_selected_to_section(&self, sel: usize) -> (bool, usize) {
        let installed_vis = if self.ext_sidebar_sections_expanded[0] {
            self.ext_installed_items().len()
        } else {
            0
        };
        if sel < installed_vis {
            (true, sel)
        } else {
            (false, sel - installed_vis)
        }
    }

    // ── Settings sidebar panel ──────────────────────────────────────────────────

    /// Row types for the settings flat list.
    /// Build the flat list of rows for the Settings sidebar.
    /// Includes both core settings and extension-declared settings.
    pub fn settings_flat_list(&self) -> Vec<SettingsRow> {
        use crate::core::settings::{setting_categories, SETTING_DEFS};
        let cats = setting_categories();
        let query = self.settings_query.to_lowercase();
        let mut rows = Vec::new();

        // Core settings
        for (cat_idx, &cat) in cats.iter().enumerate() {
            let matching: Vec<usize> = SETTING_DEFS
                .iter()
                .enumerate()
                .filter(|(_, d)| d.category == cat)
                .filter(|(_, d)| {
                    query.is_empty()
                        || d.label.to_lowercase().contains(&query)
                        || d.key.to_lowercase().contains(&query)
                        || d.description.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect();

            if matching.is_empty() {
                continue;
            }

            rows.push(SettingsRow::CoreCategory(cat_idx));

            let collapsed =
                cat_idx < self.settings_collapsed.len() && self.settings_collapsed[cat_idx];
            if !collapsed {
                for def_idx in matching {
                    rows.push(SettingsRow::CoreSetting(def_idx));
                }
            }
        }

        // Extension settings — one section per installed extension that declares settings
        for manifest in self.ext_available_manifests() {
            if manifest.settings.is_empty() || !self.extension_state.is_installed(&manifest.name) {
                continue;
            }
            let matching: Vec<&crate::core::extensions::ExtSettingDef> = manifest
                .settings
                .iter()
                .filter(|s| {
                    query.is_empty()
                        || s.label.to_lowercase().contains(&query)
                        || s.key.to_lowercase().contains(&query)
                        || s.description.to_lowercase().contains(&query)
                })
                .collect();
            if matching.is_empty() {
                continue;
            }

            rows.push(SettingsRow::ExtCategory(manifest.name.clone()));

            let collapsed = self
                .ext_settings_collapsed
                .get(&manifest.name)
                .copied()
                .unwrap_or(false);
            if !collapsed {
                for def in matching {
                    rows.push(SettingsRow::ExtSetting(
                        manifest.name.clone(),
                        def.key.clone(),
                    ));
                }
            }
        }

        rows
    }

    /// Load an extension's settings from disk, merging with manifest defaults.
    pub fn load_ext_settings(&mut self, ext_name: &str) {
        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name == ext_name);
        let manifest = match manifest {
            Some(m) => m,
            None => return,
        };
        let mut values = HashMap::new();
        // Start with defaults from manifest
        for def in &manifest.settings {
            values.insert(def.key.clone(), def.default.clone());
        }
        // Overlay with saved values from disk
        let path = paths::vimcode_config_dir()
            .join("extensions")
            .join(ext_name)
            .join("settings.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(saved) = serde_json::from_str::<HashMap<String, String>>(&data) {
                for (k, v) in saved {
                    values.insert(k, v);
                }
            }
        }
        if !values.is_empty() {
            self.ext_settings.insert(ext_name.to_string(), values);
        }
    }

    /// Save an extension's settings to disk.
    pub(crate) fn save_ext_settings(&self, ext_name: &str) {
        if let Some(values) = self.ext_settings.get(ext_name) {
            let path = paths::vimcode_config_dir()
                .join("extensions")
                .join(ext_name)
                .join("settings.json");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(values) {
                let _ = std::fs::write(&path, json);
            }
        }
    }

    /// Get an extension setting value by `ext_name` and `key`.
    pub fn get_ext_setting(&self, ext_name: &str, key: &str) -> String {
        self.ext_settings
            .get(ext_name)
            .and_then(|m| m.get(key))
            .cloned()
            .unwrap_or_default()
    }

    /// Set an extension setting value and save to disk.
    pub fn set_ext_setting(&mut self, ext_name: &str, key: &str, value: &str) {
        self.ext_settings
            .entry(ext_name.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        self.save_ext_settings(ext_name);
    }

    /// Look up an `ExtSettingDef` by extension name and key.
    pub fn find_ext_setting_def(
        &self,
        ext_name: &str,
        key: &str,
    ) -> Option<crate::core::extensions::ExtSettingDef> {
        self.ext_available_manifests()
            .into_iter()
            .find(|m| m.name == ext_name)
            .and_then(|m| m.settings.into_iter().find(|s| s.key == key))
    }

    /// Handle a key press while the settings panel has focus.
    pub fn handle_settings_key(&mut self, key: &str, _ctrl: bool, unicode: Option<char>) {
        use crate::core::settings::{SettingType, SETTING_DEFS};

        // Search input active — route printable chars to query
        if self.settings_input_active {
            match key {
                "Escape" | "Return" => {
                    self.settings_input_active = false;
                }
                "BackSpace" => {
                    self.settings_query.pop();
                    self.settings_selected = 0;
                    self.settings_scroll_top = 0;
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.settings_query.push(ch);
                            self.settings_selected = 0;
                            self.settings_scroll_top = 0;
                        }
                    }
                }
            }
            return;
        }

        // Inline editing active — core setting (string/int)
        if let Some(def_idx) = self.settings_editing {
            match key {
                "Escape" => {
                    self.settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "Return" => {
                    let def = &SETTING_DEFS[def_idx];
                    let val = self.settings_edit_buf.clone();
                    if self.settings.set_value_str(def.key, &val).is_ok() {
                        let _ = self.settings.save();
                    }
                    self.settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "BackSpace" => {
                    self.settings_edit_buf.pop();
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let def = &SETTING_DEFS[def_idx];
                            if matches!(def.setting_type, SettingType::Integer { .. }) {
                                if ch.is_ascii_digit() {
                                    self.settings_edit_buf.push(ch);
                                }
                            } else {
                                self.settings_edit_buf.push(ch);
                            }
                        }
                    }
                }
            }
            return;
        }

        // Inline editing active — extension setting (string/int)
        if let Some((ref ext_name, ref ext_key)) = self.ext_settings_editing.clone() {
            match key {
                "Escape" => {
                    self.ext_settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "Return" => {
                    let val = self.settings_edit_buf.clone();
                    self.set_ext_setting(ext_name, ext_key, &val);
                    self.ext_settings_editing = None;
                    self.settings_edit_buf.clear();
                }
                "BackSpace" => {
                    self.settings_edit_buf.pop();
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let is_int = self
                                .find_ext_setting_def(ext_name, ext_key)
                                .is_some_and(|d| d.r#type == "integer");
                            if is_int {
                                if ch.is_ascii_digit() {
                                    self.settings_edit_buf.push(ch);
                                }
                            } else {
                                self.settings_edit_buf.push(ch);
                            }
                        }
                    }
                }
            }
            return;
        }

        // Normal navigation
        let flat = self.settings_flat_list();
        let total = flat.len();

        match key {
            "q" | "Escape" => {
                self.settings_has_focus = false;
            }
            "/" => {
                self.settings_input_active = true;
            }
            "j" | "Down" => {
                if total > 0 {
                    self.settings_selected = (self.settings_selected + 1).min(total - 1);
                }
            }
            "k" | "Up" => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            "Tab" | "Return" | "Space" | "l" | "Right" | "h" | "Left" => {
                if self.settings_selected < total {
                    match &flat[self.settings_selected] {
                        SettingsRow::CoreCategory(cat_idx) => {
                            let cat_idx = *cat_idx;
                            if matches!(key, "Tab" | "Return" | "Space")
                                && cat_idx < self.settings_collapsed.len()
                            {
                                self.settings_collapsed[cat_idx] =
                                    !self.settings_collapsed[cat_idx];
                            }
                        }
                        SettingsRow::CoreSetting(idx) => {
                            let idx = *idx;
                            let def = &SETTING_DEFS[idx];
                            match &def.setting_type {
                                SettingType::Bool => {
                                    if matches!(key, "Return" | "Space") {
                                        let cur = self.settings.get_value_str(def.key);
                                        let new_val = if cur == "true" { "false" } else { "true" };
                                        if self.settings.set_value_str(def.key, new_val).is_ok() {
                                            let _ = self.settings.save();
                                        }
                                    }
                                }
                                SettingType::Enum(options) => {
                                    let forward = matches!(key, "Return" | "Space" | "l" | "Right");
                                    let backward = matches!(key, "h" | "Left");
                                    if forward || backward {
                                        let cur = self.settings.get_value_str(def.key);
                                        if let Some(pos) =
                                            options.iter().position(|&o| o == cur.as_str())
                                        {
                                            let next = if forward {
                                                (pos + 1) % options.len()
                                            } else {
                                                (pos + options.len() - 1) % options.len()
                                            };
                                            if self
                                                .settings
                                                .set_value_str(def.key, options[next])
                                                .is_ok()
                                            {
                                                let _ = self.settings.save();
                                            }
                                        }
                                    }
                                }
                                SettingType::DynamicEnum(options_fn) => {
                                    let forward = matches!(key, "Return" | "Space" | "l" | "Right");
                                    let backward = matches!(key, "h" | "Left");
                                    if forward || backward {
                                        let options = options_fn();
                                        let cur = self.settings.get_value_str(def.key);
                                        if let Some(pos) = options.iter().position(|o| o == &cur) {
                                            let next = if forward {
                                                (pos + 1) % options.len()
                                            } else {
                                                (pos + options.len() - 1) % options.len()
                                            };
                                            if self
                                                .settings
                                                .set_value_str(def.key, &options[next])
                                                .is_ok()
                                            {
                                                let _ = self.settings.save();
                                            }
                                        }
                                    }
                                }
                                SettingType::Integer { .. } | SettingType::StringVal => {
                                    if matches!(key, "Return") {
                                        self.settings_editing = Some(idx);
                                        self.settings_edit_buf =
                                            self.settings.get_value_str(def.key);
                                    }
                                }
                                SettingType::BufferEditor => {
                                    if matches!(key, "Return" | "Space" | "l" | "Right") {
                                        match def.key {
                                            "keymaps" => self.open_keymaps_editor(),
                                            "extension_registries" => self.open_registries_editor(),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        SettingsRow::ExtCategory(name) => {
                            if matches!(key, "Tab" | "Return" | "Space") {
                                let collapsed = self
                                    .ext_settings_collapsed
                                    .entry(name.clone())
                                    .or_insert(false);
                                *collapsed = !*collapsed;
                            }
                        }
                        SettingsRow::ExtSetting(ext_name, ext_key) => {
                            let ext_name = ext_name.clone();
                            let ext_key = ext_key.clone();
                            if let Some(def) = self.find_ext_setting_def(&ext_name, &ext_key) {
                                match def.r#type.as_str() {
                                    "bool" => {
                                        if matches!(key, "Return" | "Space") {
                                            let cur = self.get_ext_setting(&ext_name, &ext_key);
                                            let new_val =
                                                if cur == "true" { "false" } else { "true" };
                                            self.set_ext_setting(&ext_name, &ext_key, new_val);
                                        }
                                    }
                                    "enum" => {
                                        let forward =
                                            matches!(key, "Return" | "Space" | "l" | "Right");
                                        let backward = matches!(key, "h" | "Left");
                                        if (forward || backward) && !def.options.is_empty() {
                                            let cur = self.get_ext_setting(&ext_name, &ext_key);
                                            if let Some(pos) =
                                                def.options.iter().position(|o| o == &cur)
                                            {
                                                let next = if forward {
                                                    (pos + 1) % def.options.len()
                                                } else {
                                                    (pos + def.options.len() - 1)
                                                        % def.options.len()
                                                };
                                                self.set_ext_setting(
                                                    &ext_name,
                                                    &ext_key,
                                                    &def.options[next],
                                                );
                                            }
                                        }
                                    }
                                    _ => {
                                        if matches!(key, "Return") {
                                            self.settings_edit_buf =
                                                self.get_ext_setting(&ext_name, &ext_key);
                                            self.ext_settings_editing = Some((ext_name, ext_key));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Paste clipboard text into the active settings input (search query or inline edit buffer).
    pub fn settings_paste(&mut self, text: &str) {
        // Strip newlines — settings values are single-line.
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if self.settings_input_active {
            self.settings_query.push_str(&clean);
            self.settings_selected = 0;
            self.settings_scroll_top = 0;
        } else if self.settings_editing.is_some() {
            self.settings_edit_buf.push_str(&clean);
        }
    }

    /// Open a scratch buffer for editing user keymaps (one per line).
    pub fn open_keymaps_editor(&mut self) {
        // If a keymaps buffer already exists, switch to it
        let existing_buf_id = self
            .buffer_manager
            .iter()
            .find(|(_, state)| state.is_keymaps_buf)
            .map(|(id, _)| *id);

        if let Some(buf_id) = existing_buf_id {
            // Find a tab showing this buffer
            let tab_idx = self
                .active_group()
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| {
                    self.windows
                        .get(&tab.active_window)
                        .is_some_and(|w| w.buffer_id == buf_id)
                })
                .map(|(i, _)| i);

            if let Some(idx) = tab_idx {
                self.active_group_mut().active_tab = idx;
            } else {
                // Buffer exists but not shown — point current window at it
                self.active_window_mut().buffer_id = buf_id;
                self.view_mut().cursor.line = 0;
                self.view_mut().cursor.col = 0;
            }
            self.settings_has_focus = false;
            return;
        }

        // Build content: header comment + one keymap per line
        let mut content = String::from(
            "# User keymaps — one per line.  :w to save.\n\
             # Format: mode keys :command\n\
             # Modes: n (normal), v (visual), i (insert), c (command)\n\
             # Keys:  single char (x), modifier (<C-x>, <A-x>), sequence (gcc)\n\
             #\n\
             # In VSCode mode, \"n\" keymaps apply (use modifiers like <C-x>, <A-x>).\n\
             # Run :Keybindings to see all built-in keybindings and command names.\n\
             #\n\
             # Examples:\n\
             # n <C-/> :Commentary\n\
             # v <C-/> :Commentary\n\
             # n gcc   :Commentary\n\
             # n <A-j> :move +1\n\
             # n <A-k> :move -1\n\
             #\n",
        );
        for km in &self.settings.keymaps {
            content.push_str(km);
            content.push('\n');
        }
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.is_keymaps_buf = true;
            state.dirty = false;
        }

        // Open in a new tab (same pattern as open_file_in_tab)
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.settings_has_focus = false;
        self.message = "Edit keymaps (one per line: mode keys :command). :w to save.".to_string();
    }

    /// Save keymaps buffer content back to settings.
    pub fn save_keymaps_buffer(&mut self) -> Result<(), String> {
        let state = self.active_buffer_state();
        let rope = &state.buffer.content;
        let mut keymaps = Vec::new();
        for line_idx in 0..rope.len_lines() {
            let line: String = rope.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Validate the keymap definition
            if parse_keymap_def(trimmed).is_none() {
                return Err(format!(
                    "Invalid keymap on line {}: \"{}\" (expected: mode keys :command)",
                    line_idx + 1,
                    trimmed
                ));
            }
            keymaps.push(trimmed.to_string());
        }

        self.settings.keymaps = keymaps;
        self.rebuild_user_keymaps();
        let _ = self.settings.save();
        let count = self.settings.keymaps.len();
        self.active_buffer_state_mut().dirty = false;
        self.message = format!(
            "{} keymap{} saved to settings",
            count,
            if count == 1 { "" } else { "s" }
        );
        Ok(())
    }

    /// Open a command-line window (`q:` for commands, `q/`/`q?` for searches).
    /// Shows history in a scratch buffer. Enter on a line executes it.
    pub fn open_cmdline_window(&mut self, is_search: bool) {
        let history = if is_search {
            &self.history.search_history
        } else {
            &self.history.command_history
        };

        // Build content: one history entry per line, empty line at end for new entry
        let mut content = String::new();
        for entry in history.iter() {
            content.push_str(entry);
            content.push('\n');
        }
        content.push('\n'); // empty line at bottom for new command

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.is_cmdline_buf = true;
            state.cmdline_is_search = is_search;
            state.dirty = false;
            state.scratch_name = Some(if is_search {
                "[Search History]".to_string()
            } else {
                "[Command History]".to_string()
            });
        }

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        // Move cursor to last line (the empty line for new entry)
        let total = self.buffer().len_lines();
        self.view_mut().cursor.line = total.saturating_sub(1);
        self.view_mut().cursor.col = 0;

        self.mode = Mode::Normal;
        self.message = "Press Enter to execute, q to close".to_string();
    }

    /// Execute the current line in a command-line window buffer.
    /// Called when Enter is pressed in a cmdline buffer in Normal mode.
    pub fn cmdline_window_execute(&mut self) -> EngineAction {
        let is_search = self.active_buffer_state().cmdline_is_search;
        let line_idx = self.view().cursor.line;
        let line: String = self
            .buffer()
            .content
            .line(line_idx)
            .chars()
            .collect::<String>()
            .trim()
            .to_string();

        if line.is_empty() {
            return EngineAction::None;
        }

        // Close the cmdline window
        self.close_tab();

        if is_search {
            // Execute as a forward search
            self.search_query = line;
            self.search_direction = SearchDirection::Forward;
            self.run_search();
            self.search_next();
        } else {
            // Execute as an ex command
            return self.execute_command(&line);
        }
        EngineAction::None
    }

    /// Open a read-only reference buffer listing all default keybindings.
    /// `force_vscode`: `None` = auto-detect from current mode,
    /// `Some(true)` = VSCode, `Some(false)` = Vim.
    pub fn open_keybindings_reference_for(&mut self, force_vscode: Option<bool>) {
        let is_vscode = force_vscode.unwrap_or_else(|| self.is_vscode_mode());
        let scratch_name = if is_vscode {
            "Keybindings (VSCode)"
        } else {
            "Keybindings (Vim)"
        };

        // Reuse existing buffer for the same mode if already open
        let existing_buf_id = self
            .buffer_manager
            .iter()
            .find(|(_, state)| state.scratch_name.as_deref() == Some(scratch_name))
            .map(|(id, _)| *id);

        if let Some(buf_id) = existing_buf_id {
            let tab_idx = self
                .active_group()
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| {
                    self.windows
                        .get(&tab.active_window)
                        .is_some_and(|w| w.buffer_id == buf_id)
                })
                .map(|(i, _)| i);
            if let Some(idx) = tab_idx {
                self.active_group_mut().active_tab = idx;
            } else {
                self.active_window_mut().buffer_id = buf_id;
                self.view_mut().cursor.line = 0;
                self.view_mut().cursor.col = 0;
            }
            return;
        }

        let content = if is_vscode {
            keybindings_reference_vscode()
        } else {
            keybindings_reference_vim()
        };

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.scratch_name = Some(scratch_name.to_string());
            state.read_only = true;
            state.dirty = false;
        }

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        let mode_name = if is_vscode { "VSCode" } else { "Vim" };
        self.message = format!(
            "{mode_name} keybindings reference — use / to search. Try :Keybindings {}",
            if is_vscode { "vim" } else { "vscode" }
        );
    }

    /// Open a scratch buffer for editing extension registry URLs (one per line).
    pub fn open_registries_editor(&mut self) {
        // If a registries buffer already exists, switch to it
        let existing_buf_id = self
            .buffer_manager
            .iter()
            .find(|(_, state)| state.is_registries_buf)
            .map(|(id, _)| *id);

        if let Some(buf_id) = existing_buf_id {
            let tab_idx = self
                .active_group()
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| {
                    self.windows
                        .get(&tab.active_window)
                        .is_some_and(|w| w.buffer_id == buf_id)
                })
                .map(|(i, _)| i);

            if let Some(idx) = tab_idx {
                self.active_group_mut().active_tab = idx;
            } else {
                self.active_window_mut().buffer_id = buf_id;
                self.view_mut().cursor.line = 0;
                self.view_mut().cursor.col = 0;
            }
            self.settings_has_focus = false;
            return;
        }

        // Build content: header comment + one URL per line
        let mut content = String::from(
            "# Extension registries — one URL per line.\n\
             # Lines starting with # are comments.\n",
        );
        for url in &self.settings.extension_registries {
            content.push_str(url);
            content.push('\n');
        }

        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.is_registries_buf = true;
            state.dirty = false;
        }

        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.settings_has_focus = false;
        self.message =
            "Edit extension registries (one URL per line, # comments). :w to save.".to_string();
    }

    /// Save registries buffer content back to settings.
    pub fn save_registries_buffer(&mut self) -> Result<(), String> {
        let state = self.active_buffer_state();
        let rope = &state.buffer.content;
        let mut urls = Vec::new();
        for line_idx in 0..rope.len_lines() {
            let line: String = rope.line(line_idx).chars().collect();
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
                return Err(format!(
                    "Invalid URL on line {}: \"{}\" (must start with http:// or https://)",
                    line_idx + 1,
                    trimmed
                ));
            }
            urls.push(trimmed.to_string());
        }

        self.settings.extension_registries = urls;
        let _ = self.settings.save();
        let count = self.settings.extension_registries.len();
        self.active_buffer_state_mut().dirty = false;
        self.message = format!(
            "{} registr{} saved to settings",
            count,
            if count == 1 { "y" } else { "ies" }
        );
        Ok(())
    }

    // ── AI assistant panel ─────────────────────────────────────────────────────

    /// Send the current `ai_input` as a user message; clears input and spawns background thread.
    pub fn ai_send_message(&mut self) {
        let text = self.ai_input.trim().to_string();
        if text.is_empty() || self.ai_streaming {
            return;
        }
        self.ai_messages.push(AiMessage {
            role: "user".to_string(),
            content: text,
        });
        self.ai_input.clear();
        self.ai_input_cursor = 0;
        self.ai_streaming = true;

        let provider = self.settings.ai_provider.clone();
        let api_key = self.settings.ai_api_key.clone();
        let base_url = self.settings.ai_base_url.clone();
        let model = self.settings.ai_model.clone();
        let messages = self.ai_messages.clone();
        let system = String::new();

        let (tx, rx) = std::sync::mpsc::channel();
        self.ai_rx = Some(rx);

        std::thread::spawn(move || {
            let result = crate::core::ai::send_chat(
                &provider, &api_key, &base_url, &model, &messages, &system,
            );
            let _ = tx.send(result);
        });

        // Scroll to bottom so the user sees the new message
        self.ai_scroll_top = self.ai_messages.len().saturating_sub(1);
    }

    /// Non-blocking poll for a completed AI response. Returns `true` if something changed.
    pub fn poll_ai(&mut self) -> bool {
        let result = if let Some(rx) = &self.ai_rx {
            rx.try_recv().ok()
        } else {
            return false;
        };
        let Some(res) = result else {
            return false;
        };
        self.ai_rx = None;
        self.ai_streaming = false;
        match res {
            Ok(reply) => {
                self.ai_messages.push(AiMessage {
                    role: "assistant".to_string(),
                    content: reply,
                });
                self.ai_scroll_top = self.ai_messages.len().saturating_sub(1);
            }
            Err(e) => {
                self.message = format!("AI error: {e}");
            }
        }
        true
    }

    /// Clear the AI conversation history and cancel any in-flight request.
    pub fn ai_clear(&mut self) {
        self.ai_messages.clear();
        self.ai_rx = None;
        self.ai_streaming = false;
        self.ai_scroll_top = 0;
        self.message = "AI conversation cleared.".to_string();
    }

    /// Handle keyboard input for the AI sidebar panel.
    /// Returns `true` if the key was consumed.
    /// Insert text at the current ai_input cursor position (used for paste).
    pub fn ai_insert_text(&mut self, text: &str) {
        let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
        self.ai_input.insert_str(byte, text);
        self.ai_input_cursor += text.chars().count();
    }

    pub fn handle_ai_panel_key(&mut self, key: &str, ctrl: bool, unicode: Option<char>) -> bool {
        if self.ai_input_active {
            let char_len = self.ai_input.chars().count();
            match key {
                "Escape" => {
                    self.ai_input_active = false;
                }
                "Return" if !ctrl => {
                    self.ai_send_message();
                    self.ai_input_active = false;
                }
                "BackSpace" => {
                    if self.ai_input_cursor > 0 {
                        self.ai_input_cursor -= 1;
                        let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                        let next = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor + 1);
                        self.ai_input.drain(byte..next);
                    }
                }
                "Delete" => {
                    if self.ai_input_cursor < char_len {
                        let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                        let next = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor + 1);
                        self.ai_input.drain(byte..next);
                    }
                }
                "Left" => {
                    self.ai_input_cursor = self.ai_input_cursor.saturating_sub(1);
                }
                "Right" => {
                    self.ai_input_cursor = (self.ai_input_cursor + 1).min(char_len);
                }
                "Home" => {
                    self.ai_input_cursor = 0;
                }
                "End" => {
                    self.ai_input_cursor = char_len;
                }
                _ if ctrl && key == "a" => {
                    self.ai_input_cursor = 0;
                }
                _ if ctrl && key == "e" => {
                    self.ai_input_cursor = char_len;
                }
                _ if ctrl && key == "k" => {
                    let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                    self.ai_input.truncate(byte);
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            let byte = cmd_char_to_byte(&self.ai_input, self.ai_input_cursor);
                            self.ai_input.insert(byte, ch);
                            self.ai_input_cursor += 1;
                        }
                    }
                }
            }
            return true;
        }

        match key {
            "q" | "Escape" => {
                self.ai_has_focus = false;
                true
            }
            "i" | "a" | "Return" => {
                self.ai_input_active = true;
                true
            }
            "j" | "Down" => {
                self.ai_scroll_top = self.ai_scroll_top.saturating_add(1);
                true
            }
            "k" | "Up" => {
                self.ai_scroll_top = self.ai_scroll_top.saturating_sub(1);
                true
            }
            "G" => {
                self.ai_scroll_top = self.ai_messages.len().saturating_sub(1);
                true
            }
            "g" => {
                self.ai_scroll_top = 0;
                true
            }
            "c" if ctrl => {
                // Ctrl-C: clear conversation
                self.ai_clear();
                true
            }
            _ => false,
        }
    }

    // ── AI inline completions (ghost text) ───────────────────────────────────

    /// Clear any visible ghost text and cancel the pending completion timer.
    pub fn ai_ghost_clear(&mut self) {
        self.ai_ghost_text = None;
        self.ai_ghost_alternatives.clear();
        self.ai_ghost_alt_idx = 0;
        self.ai_completion_ticks = None;
        // Don't close the rx; the background thread may still send — we'll
        // just ignore the result since ai_completion_rx is checked only when
        // ticks fires.
    }

    /// Reset the debounce counter. Called after every insert-mode keystroke
    /// when `settings.ai_completions` is enabled.
    pub fn ai_completion_reset_timer(&mut self) {
        // Clear any stale ghost text; the counter will fire a new request.
        self.ai_ghost_text = None;
        self.ai_ghost_alternatives.clear();
        self.ai_ghost_alt_idx = 0;
        // ~15 ticks ≈ 250 ms at 60 fps; backends decrement each frame.
        self.ai_completion_ticks = Some(15);
    }

    /// Called by the backend each frame. Decrements the tick counter and
    /// fires a completion request when it reaches zero. Returns `true` if
    /// a redraw is needed.
    pub fn tick_ai_completion(&mut self) -> bool {
        // First, check if a background completion has arrived.
        let mut redraw = false;
        if let Some(rx) = &self.ai_completion_rx {
            if let Ok(result) = rx.try_recv() {
                self.ai_completion_rx = None;
                match result {
                    Ok(mut alternatives) => {
                        if !alternatives.is_empty() {
                            // Strip any leading characters that the AI repeated from the
                            // prefix (e.g. the model returns `"PlayerObject":` when the
                            // buffer already ends with `"` before the cursor).
                            // We check overlaps up to 16 chars and strip the longest match.
                            let tail = std::mem::take(&mut self.ai_completion_prefix_tail);
                            for alt in &mut alternatives {
                                let max_n = alt.chars().count().min(tail.chars().count()).min(16);
                                let overlap_bytes = (1..=max_n).rev().find_map(|n| {
                                    let alt_prefix: String = alt.chars().take(n).collect();
                                    if tail.ends_with(alt_prefix.as_str()) {
                                        Some(alt_prefix.len()) // String::len() = byte length
                                    } else {
                                        None
                                    }
                                });
                                if let Some(b) = overlap_bytes {
                                    *alt = alt[b..].to_string();
                                }
                            }
                            alternatives.retain(|a| !a.is_empty());
                        }
                        if !alternatives.is_empty() {
                            self.ai_ghost_alternatives = alternatives;
                            self.ai_ghost_alt_idx = 0;
                            self.ai_ghost_text = Some(self.ai_ghost_alternatives[0].clone());
                            redraw = true;
                        }
                    }
                    Err(_) => {
                        // Silently ignore errors for inline completions.
                    }
                }
            }
        }

        // Decrement the countdown and fire when it hits zero.
        if let Some(ticks) = self.ai_completion_ticks {
            if ticks == 0 {
                self.ai_completion_ticks = None;
                self.ai_fire_completion_request();
            } else {
                self.ai_completion_ticks = Some(ticks - 1);
            }
        }

        redraw
    }

    /// Spawn a background thread to request a ghost-text completion.
    pub(crate) fn ai_fire_completion_request(&mut self) {
        if !self.settings.ai_completions {
            return;
        }
        // Only trigger in Insert mode.
        if self.mode != Mode::Insert {
            return;
        }

        // Build prefix: all text in the active buffer up to the cursor.
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let cursor_char = line_start + col;
        let total_chars = self.buffer().content.len_chars();

        // Limit prefix to last ~2000 chars to keep latency reasonable.
        let prefix_start = cursor_char.saturating_sub(2000);
        let prefix: String = self
            .buffer()
            .content
            .slice(prefix_start..cursor_char)
            .chars()
            .collect();

        // Suffix: text after the cursor on the same line (for FIM models).
        let line_end = self
            .buffer()
            .content
            .slice(..)
            .chars()
            .enumerate()
            .skip(cursor_char)
            .find(|&(_, c)| c == '\n')
            .map(|(i, _)| i)
            .unwrap_or(total_chars);
        let suffix: String = self
            .buffer()
            .content
            .slice(cursor_char..line_end)
            .chars()
            .collect();

        let provider = self.settings.ai_provider.clone();
        let api_key = self.settings.ai_api_key.clone();
        let base_url = self.settings.ai_base_url.clone();
        let model = self.settings.ai_model.clone();

        // Store the last 64 chars of the prefix so tick_ai_completion can detect
        // and strip overlap when the AI repeats characters already in the buffer.
        self.ai_completion_prefix_tail = prefix
            .chars()
            .rev()
            .take(64)
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        let (tx, rx) = std::sync::mpsc::channel();
        self.ai_completion_rx = Some(rx);

        std::thread::spawn(move || {
            let result =
                crate::core::ai::complete(&provider, &api_key, &base_url, &model, &prefix, &suffix)
                    .map(|text| {
                        // Trim leading/trailing whitespace that many models add.
                        let trimmed = text.trim_end_matches('\n').to_string();
                        // Return a single alternative for now.
                        vec![trimmed]
                    });
            let _ = tx.send(result);
        });
    }

    // ── Swap file crash recovery ───────────────────────────────────────────

    /// Create a swap file for the given buffer.
    pub(crate) fn swap_create_for_buffer(&self, buf_id: BufferId) {
        if !self.settings.swap_file {
            return;
        }
        let state = match self.buffer_manager.get(buf_id) {
            Some(s) => s,
            None => return,
        };
        // Don't create swaps for preview buffers — they're temporary.
        if state.preview {
            return;
        }
        let canonical = match &state.canonical_path {
            Some(p) => p,
            None => return,
        };
        let swap_path = crate::core::swap::swap_path_for(canonical);
        let header = crate::core::swap::SwapHeader {
            file_path: canonical.clone(),
            pid: std::process::id(),
            modified: crate::core::swap::now_iso8601(),
        };
        let content = state.buffer.to_string();
        crate::core::swap::write_swap(&swap_path, &header, &content);
    }

    /// Check for a stale swap file when opening a file.
    /// Returns `true` if a recovery dialog is now pending (caller should stop).
    pub(crate) fn swap_check_on_open(&mut self, buf_id: BufferId) -> bool {
        if !self.settings.swap_file {
            return false;
        }
        // Don't overwrite an existing recovery dialog.  Don't create a
        // fresh swap either — the stale swap must survive until the user
        // dismisses the current dialog and we re-scan.
        if self.pending_swap_recovery.is_some() {
            return false;
        }
        let (canonical, file_path) = {
            let state = match self.buffer_manager.get(buf_id) {
                Some(s) => s,
                None => return false,
            };
            // Don't create swaps for preview buffers.
            if state.preview {
                return false;
            }
            let canonical = match &state.canonical_path {
                Some(p) => p.clone(),
                None => return false,
            };
            let file_path = match &state.file_path {
                Some(p) => p.clone(),
                None => return false,
            };
            (canonical, file_path)
        };
        let swap_path = crate::core::swap::swap_path_for(&canonical);
        if !swap_path.exists() {
            // No swap file — create a fresh one.
            self.swap_create_for_buffer(buf_id);
            return false;
        }
        // Swap file exists — parse it.
        let (header, content) = match crate::core::swap::read_swap(&swap_path) {
            Some(pair) => pair,
            None => {
                // Malformed swap file — delete and create fresh.
                crate::core::swap::delete_swap(&swap_path);
                self.swap_create_for_buffer(buf_id);
                return false;
            }
        };
        if crate::core::swap::is_pid_alive(header.pid) {
            if header.pid == std::process::id() {
                // Same process re-opening the file — just update the swap.
                self.swap_create_for_buffer(buf_id);
                return false;
            }
            // Another live process is editing this file.
            let fname = file_path.file_name().unwrap_or_default().to_string_lossy();
            self.message = format!(
                "W: \"{}\" is being edited by PID {} — opening read-only copy",
                fname, header.pid
            );
            return false;
        }
        // PID is dead — but does the swap actually differ from the file on disk?
        // If the content is identical the buffer was never modified before the
        // crash, so silently discard the stale swap instead of bothering the user.
        let disk_content = std::fs::read_to_string(&file_path).unwrap_or_default();
        if content == disk_content {
            crate::core::swap::delete_swap(&swap_path);
            self.swap_create_for_buffer(buf_id);
            return false;
        }

        // Content differs → offer recovery via dialog.
        let fname = file_path.file_name().unwrap_or_default().to_string_lossy();
        self.pending_swap_recovery = Some(SwapRecovery {
            swap_path,
            recovered_content: content,
            buffer_id: buf_id,
        });
        self.show_dialog(
            "swap_recovery",
            "Swap File Found",
            vec![
                format!("A swap file was found for \"{}\".", fname),
                format!("Modified: {}", header.modified),
                format!("Original PID: {} (no longer running)", header.pid),
            ],
            vec![
                DialogButton {
                    label: "Recover".into(),
                    hotkey: 'r',
                    action: "recover".into(),
                },
                DialogButton {
                    label: "Delete swap".into(),
                    hotkey: 'd',
                    action: "delete".into(),
                },
                DialogButton {
                    label: "Abort".into(),
                    hotkey: 'a',
                    action: "abort".into(),
                },
            ],
        );
        true
    }

    /// Process the result of a swap recovery dialog action.
    pub(crate) fn process_swap_dialog_action(&mut self, action: &str) -> EngineAction {
        let recovery = match self.pending_swap_recovery.take() {
            Some(r) => r,
            None => return EngineAction::None,
        };
        match action {
            "recover" => {
                let state = self.buffer_manager.get_mut(recovery.buffer_id);
                if let Some(state) = state {
                    let len = state.buffer.len_chars();
                    state.buffer.delete_range(0, len);
                    if !recovery.recovered_content.is_empty() {
                        state.buffer.insert(0, &recovery.recovered_content);
                    }
                    state.dirty = true;
                }
                crate::core::swap::delete_swap(&recovery.swap_path);
                self.swap_create_for_buffer(recovery.buffer_id);
                self.message = "Recovered from swap file".to_string();
            }
            "delete" => {
                crate::core::swap::delete_swap(&recovery.swap_path);
                self.swap_create_for_buffer(recovery.buffer_id);
                self.message = "Swap file deleted".to_string();
            }
            "abort" | "cancel" => {
                crate::core::swap::delete_swap(&recovery.swap_path);
                self.close_tab();
                self.message.clear();
            }
            _ => {}
        }
        // Check remaining open buffers for more stale swaps.
        // Skip when disk saves are suppressed (integration tests) because
        // delete_swap/write_swap are no-ops, so the swap file persists on
        // disk and would trigger an infinite recovery loop.
        if !crate::core::session::saves_suppressed() {
            self.swap_recheck_open_buffers();
        }
        EngineAction::None
    }
}
