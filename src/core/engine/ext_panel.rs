use super::*;

pub enum ExtSidebarKeyResult {
    Consumed,
    Unfocused,
    /// h/Left from the Extensions panel moved focus to the activity bar.
    FocusActivityBar,
}

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

    /// Whether an item passes the panel's search-input filter.
    /// Returns true when there is no filter or when the item's text contains
    /// the filter substring (case-insensitive). Plugins that subscribe to
    /// `panel_input` and re-emit a filtered `set_items` still work — this is
    /// a default fallback for panels whose plugins don't intercept.
    pub(crate) fn ext_panel_filter_matches(
        &self,
        panel_name: &str,
        item: &plugin::ExtPanelItem,
    ) -> bool {
        match self.ext_panel_input_text.get(panel_name) {
            Some(s) if !s.is_empty() => item.text.to_lowercase().contains(&s.to_lowercase()),
            _ => true,
        }
    }

    /// Count visible items in a section (accounting for collapsed tree nodes
    /// and the panel's search-input filter).
    pub(crate) fn ext_panel_visible_count(
        &self,
        panel_name: &str,
        items: &[plugin::ExtPanelItem],
    ) -> usize {
        items
            .iter()
            .filter(|item| {
                self.ext_panel_item_visible(panel_name, item, items)
                    && self.ext_panel_filter_matches(panel_name, item)
            })
            .count()
    }

    /// Return the indices of visible items in a section (accounting for
    /// collapsed tree nodes and the panel's search-input filter).
    pub fn ext_panel_visible_indices(
        &self,
        panel_name: &str,
        items: &[plugin::ExtPanelItem],
    ) -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                self.ext_panel_item_visible(panel_name, item, items)
                    && self.ext_panel_filter_matches(panel_name, item)
            })
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
    ///
    /// Match precedence:
    ///   1. An exact `id` match (empty ids never match — a query is never empty
    ///      either, so this alone rules out the empty-id separator/header rows
    ///      that `git_log_panel.lua` emits).
    ///   2. Failing that, a single unambiguous hash-prefix match against a
    ///      top-level row (`parent_id` empty, not a separator) whose `id`
    ///      starts with `item_id`. A `<hash>:<path>` child row never qualifies
    ///      here even if its id happens to share the prefix, and if more than
    ///      one top-level row matches the prefix the result is ambiguous and
    ///      treated as no match — guessing would silently select the wrong
    ///      commit.
    ///
    /// Returns `None` if the panel, section, or a unique match is not found.
    pub fn ext_panel_find_flat_index(
        &self,
        panel_name: &str,
        section_name: &str,
        item_id: &str,
    ) -> Option<usize> {
        if item_id.is_empty() {
            return None;
        }
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
                        // Pass 1: exact id match (any visible row).
                        if let Some(offset) = visible
                            .iter()
                            .position(|&vi| !items[vi].id.is_empty() && items[vi].id == item_id)
                        {
                            return Some(pos + offset);
                        }
                        // Pass 2: unambiguous hash-prefix match against a
                        // top-level (commit) row only.
                        let mut prefix_matches = visible.iter().enumerate().filter(|&(_, &vi)| {
                            !items[vi].id.is_empty()
                                && !items[vi].is_separator
                                && items[vi].parent_id.is_empty()
                                && items[vi].id.starts_with(item_id)
                        });
                        if let Some((offset, _)) = prefix_matches.next() {
                            if prefix_matches.next().is_none() {
                                return Some(pos + offset);
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
        match self.ext_panel_find_flat_index(panel_name, section_name, item_id) {
            Some(flat_idx) => {
                self.ext_panel_selected = flat_idx;
                // Center the item in the viewport
                self.ext_panel_scroll_top = flat_idx.saturating_sub(5);
            }
            None => {
                // A reveal that quietly lands on the wrong row is worse than one that
                // reports it couldn't find a unique match — leave the selection as-is
                // and say so instead of silently falling back to row 0.
                self.message =
                    format!("Could not find \"{item_id}\" in {section_name} — selection unchanged");
            }
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
            "h" | "Left" => {
                // Leave this panel and focus the activity bar at the matching row.
                // Uses the same sorted-index mapping as the activity bar primitive.
                let mut ext_names: Vec<_> = self.ext_panels.keys().cloned().collect();
                ext_names.sort();
                let idx = ext_names
                    .iter()
                    .position(|n| self.ext_panel_active.as_deref() == Some(n.as_str()))
                    .unwrap_or(0);
                self.ext_panel_has_focus = false;
                self.activity_bar_focus_in_at(sidebar::TOOLBAR_IDX_EXT_BASE + idx as u16);
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
                self.ext_panel_selected = 0;
                self.ext_panel_scroll_top = 0;
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
                        self.ext_panel_selected = 0;
                        self.ext_panel_scroll_top = 0;
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
        let markdown = crate::core::markdown::linkify_bare_urls(markdown);
        let (line_text, links, code_highlights) =
            crate::core::markdown::hover_markdown_structure(&markdown);
        // Dismiss any active editor hover to avoid overlapping popups.
        self.dismiss_editor_hover();
        self.panel_hover = Some(PanelHoverPopup {
            markdown,
            line_text,
            code_highlights,
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
        use crate::core::engine::{
            SC_SECTION_CHANGES, SC_SECTION_LOG, SC_SECTION_MERGE, SC_SECTION_STAGED,
        };
        let (section, idx) = self.sc_flat_to_section_idx(flat_index);

        // Section headers: show branch info on the "Staged Changes" header.
        if idx == usize::MAX {
            if section == SC_SECTION_STAGED {
                // Branch info hover
                return self.sc_hover_branch_info();
            }
            return None; // Other headers: no hover
        }

        match section {
            // Merge / Staged / Unstaged file items
            SC_SECTION_MERGE | SC_SECTION_STAGED | SC_SECTION_CHANGES => {
                let files = self.sc_section_files(section);
                let file = files.get(idx)?;
                self.sc_hover_file(file, section == SC_SECTION_STAGED)
            }
            // Log items
            SC_SECTION_LOG => {
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
        let mut md = format!("### {} `{}`\n\n", crate::icons::GIT_BRANCH_ALT.nerd, branch);
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
        // #991: a conflicted file reports the conflict (and git's own
        // wording for which side did what) rather than a staged/unstaged
        // change it isn't.
        let (status, where_) = match file.unmerged {
            Some(kind) => (git::StatusKind::Unmerged, kind.description()),
            None if staged => (file.staged.unwrap_or(git::StatusKind::Modified), "staged"),
            None => (
                file.unstaged.unwrap_or(git::StatusKind::Modified),
                "unstaged",
            ),
        };
        let mut md = format!("### {}\n\n", file.path);
        md.push_str(&format!(
            "**Status:** {} ({})\n\n",
            status.description(),
            where_
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

    #[allow(dead_code)]
    pub fn has_diagnostic_on_line(&self, line: usize) -> bool {
        if let Some(path) = self.active_buffer_diagnostics_key() {
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
        if let Some(path) = self.active_buffer_diagnostics_key() {
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
        if let Some(path) = self.active_buffer_diagnostics_key() {
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
        let mut full_markdown = markdown.to_string();

        // Append "Go to" navigation links after actual LSP content (vim mode only).
        // Emitted as real `[label](url)` markdown — quadraui's renderer
        // (adopted below, #821) parses these into clickable links itself,
        // so no manual span bookkeeping is needed here.
        if add_goto_links && !self.is_vscode_mode() {
            let goto = self.lsp_goto_links();
            if !goto.is_empty() {
                full_markdown.push_str("\n\nGo to ");
                for (i, (label, keybind, url)) in goto.iter().enumerate() {
                    if i > 0 {
                        full_markdown.push_str(" | ");
                    }
                    full_markdown.push_str(&format!("[{label}]({url}) (:{keybind})"));
                }
            }
        }

        let full_markdown = crate::core::markdown::linkify_bare_urls(&full_markdown);
        let (line_text, links, code_highlights) =
            crate::core::markdown::hover_markdown_structure(&full_markdown);

        let popup_width = line_text
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
            markdown: full_markdown,
            line_text,
            code_highlights,
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
                    let max_scroll = hover.line_text.len().saturating_sub(20);
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
            let max_scroll = hover.line_text.len().saturating_sub(20);
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

    /// Set the editor hover popup scroll offset directly (clamped to
    /// valid range). Used by scrollbar drag / track-click handlers
    /// in both backends — they translate a `UiEvent::ScrollOffsetChanged`
    /// from `quadraui::dispatch_mouse_drag` into this call (#215).
    pub fn editor_hover_set_scroll(&mut self, new_offset: usize) -> bool {
        if let Some(hover) = &mut self.editor_hover {
            let max_scroll = hover.line_text.len().saturating_sub(20);
            let clamped = new_offset.min(max_scroll);
            if clamped != hover.scroll_top {
                hover.scroll_top = clamped;
                return true;
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
            sel.extract_text(&hover.line_text)
        } else {
            hover.line_text.join("\n")
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
        if let Some(path) = self.active_buffer_diagnostics_key() {
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

    /// Open a URL in the default browser. Validates the URL scheme via
    /// `is_safe_url` and, if safe, queues a
    /// [`PendingPlatformAction::OpenUrl`] for the runner to carry out
    /// through `PlatformServices` (#1134) — see
    /// `Engine::pending_platform_actions`'s doc for why this can't shell
    /// out directly from here.
    pub(crate) fn open_url(&mut self, url: &str) {
        if !is_safe_url(url) {
            return;
        }
        self.pending_platform_actions
            .push(PendingPlatformAction::OpenUrl(url.to_string()));
    }

    /// Get the file path of the active buffer (if it has one).
    pub(crate) fn active_buffer_path(&self) -> Option<PathBuf> {
        self.buffer_manager
            .get(self.active_window().buffer_id)
            .and_then(|bs| bs.file_path.clone())
    }

    /// The key [`Engine::lsp_diagnostics`] is stored under for the active
    /// buffer.
    ///
    /// That map is keyed by the **canonical** absolute path (#208: the LSP
    /// flush derives its key from the server's `file://` URI, and
    /// `panels.rs` re-keys notifications through `canonical_path`), and
    /// `build_rendered_window` looks the gutter up through
    /// `BufferState::canonical_path` for the same reason. Every diagnostics
    /// lookup on this side must use the same key or it silently misses
    /// whenever the buffer was opened through a path that is not already
    /// canonical — a symlinked directory, or any path carrying a `.`/`..`
    /// segment. On macOS that is the *common* case, not an exotic one:
    /// `std::env::temp_dir()` hands back `/var/folders/…`, a symlink to
    /// `/private/var/folders/…`, so the diagnostic gutter painted a marker
    /// that, when clicked, opened no hover at all.
    ///
    /// Falls back to the raw `file_path` for a buffer whose file does not
    /// exist on disk yet (`canonical_path` is `None` until the first
    /// successful `canonicalize`).
    pub(crate) fn active_buffer_diagnostics_key(&self) -> Option<PathBuf> {
        let state = self.buffer_manager.get(self.active_window().buffer_id)?;
        state
            .canonical_path
            .clone()
            .or_else(|| state.file_path.clone())
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
            if let Some(path) = self.active_buffer_diagnostics_key() {
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

    pub fn ext_selected_from_sidebar_system(&self) -> (bool, usize) {
        let sidebar = self.ext_sidebar_system.borrow();
        let section = sidebar.active_section().unwrap_or(0);
        let idx = sidebar
            .selected_path(section)
            .and_then(|p| p.first().copied())
            .unwrap_or(0) as usize;
        (section == 0, idx)
    }

    pub fn dispatch_ext_sidebar_event(&mut self, event: quadraui::SidebarEvent) -> bool {
        match event {
            quadraui::SidebarEvent::RowActivated { .. } => {
                self.ext_open_selected_readme();
                true
            }
            quadraui::SidebarEvent::RowSelected { .. } => {
                self.ext_sidebar_input_active = false;
                true
            }
            // #971: `SidebarSystem::click` already flips `collapsed[section]`
            // itself before returning this event — see
            // `Engine::dispatch_sc_sidebar_event`'s identical fix for the
            // full story (same double-toggle-cancels-out bug, same fix,
            // caught by this panel's own
            // `ext_panel_header_click_hit_band_matches_the_painted_row`
            // sanity check in `src/macos/mod.rs`).
            quadraui::SidebarEvent::HeaderActivated { .. } => true,
            quadraui::SidebarEvent::Ignored => false,
            _ => true,
        }
    }

    pub fn dispatch_ext_sidebar_action_key(&mut self, key: &str) -> bool {
        match key {
            "Escape" | "q" => {
                self.ext_sidebar_has_focus = false;
                self.ext_sidebar_system.borrow_mut().set_has_focus(false);
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
            "i" => {
                let (in_installed, idx) = self.ext_selected_from_sidebar_system();
                if in_installed {
                    let installed = self.ext_installed_items();
                    if let Some(m) = installed.get(idx) {
                        let name = &m.name;
                        self.message =
                            format!("Extension '{name}' is already installed. Use d to remove.");
                    }
                } else {
                    let available = self.ext_available_items();
                    if idx < available.len() {
                        let base_url = self.resolve_registry_base_url(&available[idx]);
                        let name = available[idx].name.clone();
                        let display = if available[idx].display_name.is_empty() {
                            name.clone()
                        } else {
                            available[idx].display_name.clone()
                        };
                        self.ext_install_from_registry(&name);
                        let readme_path = crate::core::paths::vimcode_config_dir()
                            .join("extensions")
                            .join(&name)
                            .join("README.md");
                        let content = std::fs::read_to_string(&readme_path)
                            .ok()
                            .or_else(|| crate::core::registry::fetch_readme(&base_url, &name));
                        if let Some(content) = content {
                            self.open_markdown_preview_in_tab(&content, &display);
                        }
                        self.ext_sidebar_system.borrow_mut().set_collapsed(0, false);
                        let new_installed = self.ext_installed_items();
                        if let Some(pos) = new_installed.iter().position(|m| m.name == name) {
                            self.ext_sidebar_system
                                .borrow_mut()
                                .set_active_section(Some(0));
                            self.ext_sidebar_system
                                .borrow_mut()
                                .set_selected_path(0, Some(vec![pos as u16]));
                        }
                    }
                }
                true
            }
            "d" => {
                let (in_installed, idx) = self.ext_selected_from_sidebar_system();
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
                let (in_installed, idx) = self.ext_selected_from_sidebar_system();
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
            "Return" => {
                self.ext_open_selected_readme();
                true
            }
            _ => false,
        }
    }

    pub fn populate_ext_sidebar_system(&self) {
        use quadraui::{Decoration, StyledText, TreeRow};

        // #1489: installed/available filtering used to be inlined here a
        // third time, alongside the identical filter in `ext_installed_items`
        // / `ext_available_items` (used by `dispatch_ext_sidebar_action_key`)
        // and a since-deleted dead copy in `render.rs::build_ext_sidebar_data`.
        // Reuse those two so there is exactly one definition of "installed"
        // and "matches the search query" left.
        let installed_rows: Vec<TreeRow> = self
            .ext_installed_items()
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let display = if m.display_name.is_empty() {
                    &m.name
                } else {
                    &m.display_name
                };
                let has_update = self.ext_has_update(&m.name);
                let label = if has_update {
                    format!("\u{25cf} {} \u{2191}", display)
                } else {
                    format!("\u{25cf} {}", display)
                };
                TreeRow {
                    path: vec![i as u16],
                    indent: 0,
                    icon: None,
                    text: StyledText::plain(label),
                    badge: None,
                    is_expanded: None,
                    decoration: Decoration::Normal,
                    edit: None,
                }
            })
            .collect();

        let available_rows: Vec<TreeRow> = self
            .ext_available_items()
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let display = if m.display_name.is_empty() {
                    &m.name
                } else {
                    &m.display_name
                };
                TreeRow {
                    path: vec![i as u16],
                    indent: 0,
                    icon: None,
                    text: StyledText::plain(format!("\u{25cb} {}", display)),
                    badge: None,
                    is_expanded: None,
                    decoration: Decoration::Normal,
                    edit: None,
                }
            })
            .collect();

        let mut sidebar = self.ext_sidebar_system.borrow_mut();
        sidebar.set_has_focus(self.ext_sidebar_has_focus);
        if self.ext_sidebar_has_focus && sidebar.active_section().is_none() {
            sidebar.set_active_section(Some(0));
        }
        sidebar.set_rows(0, installed_rows);
        sidebar.set_rows(1, available_rows);
    }

    fn ext_sidebar_navigate(&mut self, key: quadraui::Key) {
        self.populate_ext_sidebar_system();
        let rect = self.ext_sidebar_body_rect.get();
        let ev = quadraui::UiEvent::KeyPressed {
            key,
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        };
        let sidebar_event = self
            .ext_sidebar_system
            .borrow_mut()
            .handle_cached(&ev, rect);
        self.dispatch_ext_sidebar_event(sidebar_event);
    }

    pub fn dispatch_ext_sidebar_key_unified(
        &mut self,
        key: &str,
        unicode: Option<char>,
    ) -> ExtSidebarKeyResult {
        use quadraui::{Key, NamedKey};

        if self.ext_sidebar_input_active {
            match key {
                "Escape" => {
                    self.ext_sidebar_input_active = false;
                    ExtSidebarKeyResult::Consumed
                }
                "BackSpace" => {
                    self.ext_sidebar_query.pop();
                    ExtSidebarKeyResult::Consumed
                }
                "Down" => {
                    self.ext_sidebar_navigate(Key::Named(NamedKey::Down));
                    ExtSidebarKeyResult::Consumed
                }
                "Up" => {
                    self.ext_sidebar_navigate(Key::Named(NamedKey::Up));
                    ExtSidebarKeyResult::Consumed
                }
                "Return" => {
                    self.ext_open_selected_readme();
                    ExtSidebarKeyResult::Consumed
                }
                _ => {
                    if let Some(ch) = unicode {
                        if !ch.is_control() {
                            self.ext_sidebar_query.push(ch);
                        }
                    }
                    ExtSidebarKeyResult::Consumed
                }
            }
        } else {
            match key {
                "Escape" | "q" => {
                    self.ext_sidebar_has_focus = false;
                    self.ext_sidebar_system.borrow_mut().set_has_focus(false);
                    ExtSidebarKeyResult::Unfocused
                }
                "h" | "Left" => {
                    self.ext_sidebar_has_focus = false;
                    self.ext_sidebar_system.borrow_mut().set_has_focus(false);
                    self.activity_bar_focus_in_at(5);
                    ExtSidebarKeyResult::FocusActivityBar
                }
                "/" => {
                    self.ext_sidebar_input_active = true;
                    ExtSidebarKeyResult::Consumed
                }
                "r" => {
                    self.ext_refresh();
                    ExtSidebarKeyResult::Consumed
                }
                "i" | "d" | "u" | "Return" => {
                    self.dispatch_ext_sidebar_action_key(key);
                    ExtSidebarKeyResult::Consumed
                }
                _ => {
                    let nav_key = match key {
                        "j" => Some(Key::Char('j')),
                        "k" => Some(Key::Char('k')),
                        "Down" => Some(Key::Named(NamedKey::Down)),
                        "Up" => Some(Key::Named(NamedKey::Up)),
                        "Tab" => Some(Key::Named(NamedKey::Tab)),
                        // "ISO_Left_Tab" is TUI's (and, since #1060, GTK's
                        // own) `render::engine_key_from_ui` spelling for
                        // Shift+Tab; "BackTab" is kept for any caller still
                        // on the pre-#1060 name.
                        "BackTab" | "ISO_Left_Tab" => Some(Key::Named(NamedKey::BackTab)),
                        "Home" => Some(Key::Named(NamedKey::Home)),
                        "End" => Some(Key::Named(NamedKey::End)),
                        "Page_Up" => Some(Key::Named(NamedKey::PageUp)),
                        "Page_Down" => Some(Key::Named(NamedKey::PageDown)),
                        _ => None,
                    };
                    if let Some(k) = nav_key {
                        self.ext_sidebar_navigate(k);
                    }
                    ExtSidebarKeyResult::Consumed
                }
            }
        }
    }

    pub fn handle_ext_sidebar_ui_event(&mut self, event: quadraui::UiEvent) -> bool {
        self.populate_ext_sidebar_system();
        let rect = self.ext_sidebar_body_rect.get();
        let sidebar_event = self
            .ext_sidebar_system
            .borrow_mut()
            .handle_cached(&event, rect);
        self.dispatch_ext_sidebar_event(sidebar_event)
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
    pub fn handle_settings_key(&mut self, key: &str, ctrl: bool, unicode: Option<char>) {
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
                    // Lazy-init spell checker when toggled on via text entry
                    if def.key == "spell" && self.settings.spell {
                        self.ensure_spell_checker();
                    }
                    // Re-parse the active buffer when the syntax threshold
                    // changes via the form so newly-enabled highlighting
                    // appears on the currently-open huge file.
                    if def.key == "syntax_max_lines" {
                        self.update_syntax();
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

        // h/Left: focus the activity bar when the selected setting is not an
        // enum type (for enums, h/Left cycles the value backward instead).
        if (key == "h" || key == "Left") && !ctrl {
            use crate::core::settings::{SettingType, SETTING_DEFS};
            let is_enum = if self.settings_selected < flat.len() {
                match &flat[self.settings_selected] {
                    SettingsRow::CoreSetting(idx) => matches!(
                        SETTING_DEFS[*idx].setting_type,
                        SettingType::Enum(_) | SettingType::DynamicEnum(_)
                    ),
                    SettingsRow::ExtSetting(ext_name, ext_key) => self
                        .find_ext_setting_def(ext_name, ext_key)
                        .is_some_and(|d| d.r#type == "enum"),
                    _ => false,
                }
            } else {
                false
            };
            if !is_enum {
                self.settings_has_focus = false;
                self.activity_bar_focus_in_at(sidebar::TOOLBAR_IDX_SETTINGS);
                return;
            }
            // is_enum == true: fall through so the existing match arm cycles the value.
        }

        match key {
            "q" | "Escape" => {
                self.settings_has_focus = false;
            }
            "/" => {
                self.settings_input_active = true;
            }
            "j" | "Down" if total > 0 => {
                self.settings_selected = (self.settings_selected + 1).min(total - 1);
            }
            "k" | "Up" => {
                self.settings_selected = self.settings_selected.saturating_sub(1);
            }
            "Tab" | "Return" | "Space" | "l" | "Right" | "h" | "Left"
                if self.settings_selected < total =>
            {
                match &flat[self.settings_selected] {
                    SettingsRow::CoreCategory(cat_idx) => {
                        let cat_idx = *cat_idx;
                        if matches!(key, "Tab" | "Return" | "Space")
                            && cat_idx < self.settings_collapsed.len()
                        {
                            self.settings_collapsed[cat_idx] = !self.settings_collapsed[cat_idx];
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
                                    // Lazy-init spell checker when toggled on
                                    if def.key == "spell" && self.settings.spell {
                                        self.ensure_spell_checker();
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
                                    self.settings_edit_buf = self.settings.get_value_str(def.key);
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
                                        let new_val = if cur == "true" { "false" } else { "true" };
                                        self.set_ext_setting(&ext_name, &ext_key, new_val);
                                    }
                                }
                                "enum" => {
                                    let forward = matches!(key, "Return" | "Space" | "l" | "Right");
                                    let backward = matches!(key, "h" | "Left");
                                    if (forward || backward) && !def.options.is_empty() {
                                        let cur = self.get_ext_setting(&ext_name, &ext_key);
                                        if let Some(pos) =
                                            def.options.iter().position(|o| o == &cur)
                                        {
                                            let next = if forward {
                                                (pos + 1) % def.options.len()
                                            } else {
                                                (pos + def.options.len() - 1) % def.options.len()
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
             # Format: mode[!] keys rhs\n\
             # Modes: n (normal) v (visual) x (visual-only) o (operator-pending)\n\
             #        i (insert) c (command) s (select, unused)\n\
             # A trailing '!' on mode is noremap (rhs is not re-expanded).\n\
             # Keys:  single char (x), modifier (<C-x>), sequence (gcc), vim\n\
             #        notation (<Esc> <CR> <Tab> <leader> <Plug>...)\n\
             # Rhs:   an ex command prefixed with ':' (:Commentary), or a raw\n\
             #        key sequence fed back through the normal key path (<Esc>)\n\
             #\n\
             # This buffer is edited directly in vimcode's storage format; day\n\
             # to day, prefer the vim ex commands instead — :nnoremap, :imap,\n\
             # :vnoremap, :onoremap, :unmap, :mapclear, etc. — which write to\n\
             # this same list.\n\
             #\n\
             # In VSCode mode, \"n\" keymaps apply (use modifiers like <C-x>, <A-x>).\n\
             # Run :Keybindings to see all built-in keybindings and command names.\n\
             #\n\
             # Examples:\n\
             # n <C-/>  :Commentary\n\
             # v <C-/>  :Commentary\n\
             # n gcc    :Commentary\n\
             # n <A-j>  :move +1\n\
             # n <A-k>  :move -1\n\
             # i! jk    <Esc>\n\
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
        self.message = "Edit keymaps (one per line: mode[!] keys rhs). :w to save.".to_string();
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
                    "Invalid keymap on line {}: \"{}\" (expected: mode[!] keys rhs)",
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

        // Neovim opens the command-line window as a horizontal split in the
        // *current* tabpage (`:h cmdwin`), not a new tab (#1297) — push a
        // new window into the active tab's layout instead of a new `Tab`.
        // Per `:h cmdwin`, the window is "always ... positioned just above
        // the command-line" — i.e. always at the bottom, unlike an ordinary
        // horizontal split, which honors 'splitbelow'. `new_first: false`
        // pins it there unconditionally (confirmed against a live,
        // UI-attached `nvim` — `winlayout()` puts the cmdwin leaf second).
        let current_window_id = self.active_window_id();
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab = self.active_tab_mut();
        tab.layout.split_at(
            current_window_id,
            SplitDirection::Horizontal,
            window_id,
            false,
        );
        tab.focus_window(window_id);

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

        // Close the cmdline window — it's a split in the current tab
        // (#1297), not a whole tab, so close just the window.
        self.close_window();

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

    /// Send `text` as a user message. Callers (`ChatControllerEvent::Submit`
    /// dispatch, the `:AI` command, the palette's `chat_send:` action) own
    /// clearing whatever input widget held the text — this only mutates the
    /// conversation/request state.
    ///
    /// Transport is picked by whether any ACP agent is configured (#952,
    /// ACP-1; #958, ACP-7): a non-empty `settings.acp_agents` registry or a
    /// non-empty legacy `settings.acp_agent_command` both route through a
    /// live ACP agent subprocess (`ai_send_message_via_acp`); neither
    /// configured keeps the original direct-provider `curl` transport
    /// (`ai_send_message_via_curl`, `crate::core::ai`) — kept as a
    /// no-agent-binary escape hatch per #952's "Decide in this slice"
    /// through ACP-7.
    ///
    /// #1446: when neither transport is usable — no ACP agent *and* no
    /// resolvable API key for a provider that needs one — this fails
    /// synchronously with an actionable message instead of spawning curl,
    /// which used to surface as a bare "AI error: curl failed:" once the
    /// doomed request predictably failed. The failure follows the same
    /// shape as `ai_send_message_via_acp`'s spawn-failure arm: the user's
    /// turn still lands in the transcript (the panel must not silently
    /// swallow what was typed) and the explanation lands *in the
    /// transcript* as an `assistant-thought`, not only on the status line
    /// a chat user is not looking at.
    pub fn ai_send_message(&mut self, text: String) {
        let text = text.trim().to_string();
        // #1512: a message submitted while a turn is already in flight is
        // queued rather than silently dropped — `quadraui::ChatController`
        // itself swallows an empty `Submit` before it ever reaches this
        // function (see `try_submit`'s doc), so an empty `text` here can
        // only come from a call site other than the chat input's own
        // Enter/Ctrl+S (there is none today) — kept as a no-op, matching
        // the pre-#1512 behaviour for that case, rather than queuing an
        // empty message.
        if self.acp_mut().ai_streaming {
            if !text.is_empty() {
                self.ai_queue_message(text);
            }
            return;
        }
        if text.is_empty() {
            return;
        }
        let acp_configured = !self.settings.acp_agent_command.trim().is_empty()
            || !self.settings.acp_agents.is_empty();
        if acp_configured {
            self.ai_send_message_via_acp(text);
            return;
        }
        // #1446: fail fast with an actionable message when neither transport
        // is usable, instead of spawning curl and reporting whatever (often
        // empty) stderr it produces once the request inevitably fails.
        let provider = self.settings.ai_provider.clone();
        if crate::core::ai::provider_needs_api_key(&provider)
            && crate::core::ai::resolve_api_key(&provider, &self.settings.ai_api_key).is_empty()
        {
            self.acp_mut().ai_messages.push(AiMessage {
                role: "user".to_string(),
                content: text,
            });
            self.message = format!(
                "AI: no ACP agent configured (acp_agents / acp_agent_command) and no \
                 API key for provider \"{provider}\""
            );
            self.acp_mut().ai_messages.push(AiMessage {
                role: "assistant-thought".to_string(),
                content: format!(
                    "\u{26a0} Cannot send: no ACP agent is configured (set `acp_agents` or \
                     `acp_agent_command`) and no API key is available for provider \
                     \"{provider}\" (set `ai_api_key`, or the provider's API-key \
                     environment variable)."
                ),
            });
            return;
        }
        self.ai_send_message_via_curl(text);
    }

    /// Direct-provider transport: spawns the blocking `curl` background
    /// thread (`crate::core::ai::send_chat`), polled by `poll_ai`.
    fn ai_send_message_via_curl(&mut self, text: String) {
        self.acp_mut().ai_messages.push(AiMessage {
            role: "user".to_string(),
            content: text,
        });
        self.ai_dispatch_curl_request();
    }

    /// Spawn the blocking `curl` background thread off whatever's
    /// currently in `ai_messages` (`curl_transport_history` filters to
    /// `"user"`/`"assistant"` turns, dropping ACP-only roles like
    /// `"user-queued"`/`"assistant-thought"`). Split out from
    /// `ai_send_message_via_curl` so #1512's queued-message dispatch
    /// (`Engine::ai_dispatch_queued_message`) can reuse it without
    /// pushing a second transcript turn for a message that's already
    /// there — it just flips the existing dimmed `"user-queued"` turn
    /// back to plain `"user"` first.
    fn ai_dispatch_curl_request(&mut self) {
        self.acp_begin_streaming();

        let provider = self.settings.ai_provider.clone();
        let api_key = self.settings.ai_api_key.clone();
        let base_url = self.settings.ai_base_url.clone();
        let model = self.settings.ai_model.clone();
        let messages = curl_transport_history(&self.acp_mut().ai_messages);
        let system = String::new();

        let (tx, rx) = std::sync::mpsc::channel();
        self.ai_rx = Some(rx);

        std::thread::spawn(move || {
            let result = crate::core::ai::send_chat(
                &provider, &api_key, &base_url, &model, &messages, &system,
            );
            let _ = tx.send(result);
        });
    }

    /// ACP transport (#952, ACP-1): spawns (or reuses) a live ACP agent
    /// subprocess and drives it through `initialize` -> `session/new` ->
    /// `session/prompt`. All of the session-update chunk streaming and
    /// prompt-stop handling lives in `Engine::poll_acp`
    /// (`src/core/engine/acp_ops.rs`), driven off the non-blocking
    /// `AcpClient::poll` — nothing here blocks the tick.
    fn ai_send_message_via_acp(&mut self, text: String) {
        // #1449/#1450: chip line(s) naming what got attached (if anything)
        // go on the *displayed* transcript message — the wire content built
        // below (`acp_prompt_content_blocks`) carries the actual
        // `resource_link`/`resource` blocks regardless of whether these
        // chips are shown, so the two never disagree about what was
        // attached. The range/selection attachment (#1450) is only *read*
        // here (`.as_ref()`, not `.take()`) — `acp_prompt_content_blocks`
        // below is what consumes it, since a not-yet-connected session
        // defers that call to `poll_acp`'s `SessionCreated` handler instead.
        let mut chip_lines = Vec::new();
        if let Some((_, chip)) = self.acp_current_buffer_attachment() {
            chip_lines.push(chip);
        }
        if let Some(attachment) = self.acp_pending_attachment.as_ref() {
            chip_lines.push(attachment.chip(&self.acp_workspace_cwd()));
        }
        // #1464: one chip line per manually attached file/image, same
        // "named on the displayed transcript, regardless of whether the
        // chip is also shown live in the header" contract as the two
        // attachments above.
        for attachment in &self.acp_manual_attachments {
            chip_lines.push(attachment.chip());
        }
        let displayed_text = if chip_lines.is_empty() {
            text.clone()
        } else {
            format!("{}\n{text}", chip_lines.join("\n"))
        };

        // #1459: on the very first `:AI`/message of the process, and only
        // then, honour `acp_reopen_last_session` — resume the most recent
        // recorded session for the about-to-launch agent/workspace instead
        // of starting empty. `acp_client.is_none()` guards against this
        // ever firing on a message sent to an agent that's already running
        // (that's a continuation of a session already chosen, not a fresh
        // start to redirect). #1459 review: also honours the learned
        // `loadSession` capability the same way `acp_open_sessions_picker`
        // does — an agent that has already told us it doesn't support
        // resume must fall straight through to a fresh session below, not
        // attempt one that can only fail on the wire.
        let mut auto_resume_session_id: Option<String> = None;
        if !self.acp_startup_reopen_attempted {
            self.acp_startup_reopen_attempted = true;
            if self.settings.acp_reopen_last_session && self.acp_mut().client.is_none() {
                let agent_name = self.acp_active_agent_name();
                if self.acp_session_index.load_session_capability(&agent_name) != Some(false) {
                    let cwd = self.acp_workspace_cwd();
                    auto_resume_session_id = self
                        .acp_session_index
                        .sessions_for(&agent_name, &cwd)
                        .into_iter()
                        .next()
                        .map(|record| record.session_id);
                }
            }
        }

        if let Some(session_id) = auto_resume_session_id {
            // #1459 review: pushing the just-typed message immediately
            // (like the no-resume branch below does) would put it *above*
            // the replayed history that's about to land after it — the
            // resumed session's `session/update` notifications append to
            // whatever is already in `ai_messages`. Reset first (matching
            // the picker's `acp_resume_session` — a no-op here in practice
            // since this is the first message of the process, but keeps
            // the two resume entry points consistent) and hold the display
            // line back until the resume actually finishes or is abandoned
            // — see `AcpEvent::SessionLoaded`/`AcpEvent::RequestFailed` in
            // `acp_ops.rs`, and the `Err(e)` spawn-failure arm below.
            self.acp_reset_transcript_for_resume();
            self.acp_pending_resume = Some(session_id);
            self.acp_mut().pending_prompt_display = Some(displayed_text);
        } else {
            self.acp_mut().ai_messages.push(AiMessage {
                role: "user".to_string(),
                content: displayed_text,
            });
        }
        self.acp_begin_streaming();
        self.acp_mut().streaming_turn = None;

        if self.acp_mut().client.is_some() {
            if let Some(session_id) = self.acp_mut().session_id.clone() {
                let content = self.acp_prompt_content_blocks(&text);
                if let Some(client) = self.acp_mut().client.as_mut() {
                    client.prompt(&session_id, content);
                }
            } else {
                // The initialize -> session/new handshake from a previous
                // message is still in flight; `poll_acp`'s `SessionCreated`
                // handler sends this the moment the session id lands,
                // rebuilding the content blocks fresh at that point (see
                // its doc for why that's the correct order).
                self.acp_mut().pending_prompt = Some(text);
            }
            return;
        }

        // #958 (ACP-7): the registry (if configured) or the legacy
        // single-string setting — see `acp_resolve_agent_launch`'s doc for
        // why this is the one call site that knows the registry exists.
        let (argv, cwd, env, agent_label) = self.acp_resolve_agent_launch();
        let env_refs: Vec<(&str, &str)> =
            env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        // #1463: label this tab with the agent's registry *name* (e.g.
        // "claude"), not `agent_label` (the raw command string, used below
        // only for the spawn-failure message) — that's what the session
        // tab strip and `:AiNext`/`:AiPrev`'s status line show.
        self.acp_mut().label = self.acp_active_agent_name();
        match crate::core::acp::AcpClient::spawn_with_env(&argv, &cwd, &env_refs) {
            Ok(mut client) => {
                client.initialize();
                self.acp_mut().client = Some(client);
                self.acp_mut().pending_prompt = Some(text);
            }
            Err(e) => {
                // Acceptance (#952): agent binary missing from PATH must be
                // a clear, actionable panel message, not a crash or a
                // silently empty panel — so this lands in the transcript
                // itself, not just the status line.
                self.acp_mut().ai_streaming = false;
                // #1459: the auto-resume path deferred showing the user's
                // typed message until the resume finished — it never will
                // now (there's no client to finish it), so show it here
                // instead of silently dropping it.
                self.acp_pending_resume = None;
                if let Some(display) = self.acp_mut().pending_prompt_display.take() {
                    self.acp_mut().ai_messages.push(AiMessage {
                        role: "user".to_string(),
                        content: display,
                    });
                }
                self.message = format!("ACP agent failed to start: {e}");
                self.acp_mut().ai_messages.push(AiMessage {
                    role: "assistant-thought".to_string(),
                    content: format!("\u{26a0} Could not start ACP agent \"{agent_label}\": {e}"),
                });
            }
        }
    }

    // ── #1512: queue a message typed while the agent is busy ────────────

    /// Called from `ai_send_message` when a turn is already in flight
    /// (`ai_streaming`) instead of the pre-#1512 silent no-op. Only one
    /// message can be queued at a time — a second call while one is
    /// already queued *replaces* its content in place (same dimmed
    /// transcript turn, new text) rather than stacking a second one, so
    /// the status strip's "queued (1)" segment (`render::
    /// populate_ai_chat_controller`) never has to count higher than one.
    fn ai_queue_message(&mut self, text: String) {
        if let Some(idx) = self.acp_mut().queued_prompt_idx {
            if let Some(m) = self.acp_mut().ai_messages.get_mut(idx) {
                m.content = text.clone();
            }
        } else {
            // #1512: rendered dimmed (`render::populate_ai_chat_controller`'s
            // `"user-queued"` arm) and prefixed `"(queued) "` on the
            // painted text itself, not just a colour — a colour alone
            // isn't something a black-box test (or a low-color terminal)
            // can reliably read back, unlike a literal substring.
            let idx = self.acp_mut().ai_messages.len();
            self.acp_mut().ai_messages.push(AiMessage {
                role: "user-queued".to_string(),
                content: text.clone(),
            });
            self.acp_mut().queued_prompt_idx = Some(idx);
        }
        self.acp_mut().queued_prompt = Some(text);
        self.message = "Queued (1) \u{2014} sends automatically once the \
                         current turn finishes (Ctrl+G to send now, Ctrl+R to discard)."
            .to_string();
    }

    /// Drop whatever's queued without sending it (Ctrl+R,
    /// `dispatch_ai_chat_event` — the busy-panel analogue of that same key's
    /// existing "drop the most-recently-staged attachment" behaviour,
    /// falling through to this once neither attachment kind is staged).
    /// Returns `false` (and does nothing) if nothing was queued, so the
    /// caller can chain further Ctrl+R fallbacks / know whether to show a
    /// message.
    ///
    /// The dimmed transcript turn is left in place rather than removed —
    /// this panel's transcript is append-only everywhere else (e.g.
    /// `acp_cancel_turn`'s `"[cancelled by user]"` notice never removes
    /// the turn it's about either), and removing it would shift every
    /// `ai_messages` index recorded after it (`tool_call_anchor`,
    /// `thought_expanded`, `markdown_turn_cache`) out from under whatever
    /// they name. Relabelled `"(discarded)"` instead so the history stays
    /// honest about what happened without needing index surgery.
    pub(crate) fn ai_discard_queued_message(&mut self) -> bool {
        if self.acp_mut().queued_prompt.take().is_none() {
            return false;
        }
        if let Some(idx) = self.acp_mut().queued_prompt_idx.take() {
            if let Some(m) = self.acp_mut().ai_messages.get_mut(idx) {
                m.content = format!("{} (discarded)", m.content);
            }
        }
        true
    }

    /// Send whatever's queued right now instead of waiting for the
    /// in-flight turn to reach `PromptStopped` on its own (Ctrl+G,
    /// `dispatch_ai_chat_event`) — cancels the current turn
    /// (`acp_cancel_turn`, the same "[cancelled by user]" path `Ctrl+C`
    /// uses while streaming) and immediately dispatches the queued
    /// message. A no-op if nothing is queued.
    pub(crate) fn ai_send_queued_now(&mut self) {
        if self.acp_mut().queued_prompt.is_none() {
            return;
        }
        self.acp_cancel_turn();
        self.ai_dispatch_queued_message();
    }

    /// Actually send whatever's queued (`AcpSession::queued_prompt`) — the
    /// shared tail both `Engine::ai_send_queued_now` ("send now") and the
    /// natural `AcpEvent::PromptStopped`/curl-completion paths
    /// (`poll_acp`/`poll_ai`) call once a turn genuinely ends. Flips the
    /// dimmed `"user-queued"` transcript turn back to a plain `"user"`
    /// one (it's already showing the right text; no second push) and then
    /// dispatches it over whichever transport is currently configured —
    /// deliberately re-read from `self.settings` here rather than
    /// remembered from queue time, since a setting could plausibly change
    /// while the previous turn was still running. A no-op if nothing is
    /// queued.
    pub(crate) fn ai_dispatch_queued_message(&mut self) {
        let Some(text) = self.acp_mut().queued_prompt.take() else {
            return;
        };
        if let Some(idx) = self.acp_mut().queued_prompt_idx.take() {
            if let Some(m) = self.acp_mut().ai_messages.get_mut(idx) {
                m.role = "user".to_string();
            }
        }
        let acp_configured = !self.settings.acp_agent_command.trim().is_empty()
            || !self.settings.acp_agents.is_empty();
        if acp_configured && self.acp_mut().client.is_some() {
            self.acp_begin_streaming();
            self.acp_mut().streaming_turn = None;
            if let Some(session_id) = self.acp_mut().session_id.clone() {
                let content = self.acp_prompt_content_blocks(&text);
                if let Some(client) = self.acp_mut().client.as_mut() {
                    client.prompt(&session_id, content);
                }
            } else {
                // The handshake somehow isn't finished yet (shouldn't
                // normally happen — queuing only ever starts once
                // `ai_streaming` was already `true`, which implies a
                // session already existed) — fall back to the handshake
                // mechanism (`AcpEvent::SessionCreated`/`SessionLoaded`)
                // rather than losing the message.
                self.acp_mut().pending_prompt = Some(text);
            }
            return;
        }
        // #1446: same fail-fast shape as `ai_send_message`'s no-transport
        // branch — a queued message that can no longer be sent (agent
        // process died, or no API key) must say so, not just silently
        // vanish now that its dimmed turn has already flipped to plain
        // "user".
        let provider = self.settings.ai_provider.clone();
        if crate::core::ai::provider_needs_api_key(&provider)
            && crate::core::ai::resolve_api_key(&provider, &self.settings.ai_api_key).is_empty()
        {
            self.message = format!(
                "AI: no ACP agent configured (acp_agents / acp_agent_command) and no \
                 API key for provider \"{provider}\""
            );
            self.acp_mut().ai_messages.push(AiMessage {
                role: "assistant-thought".to_string(),
                content: format!(
                    "\u{26a0} Cannot send queued message: no ACP agent is configured \
                     (set `acp_agents` or `acp_agent_command`) and no API key is \
                     available for provider \"{provider}\" (set `ai_api_key`, or the \
                     provider's API-key environment variable)."
                ),
            });
            return;
        }
        self.ai_dispatch_curl_request();
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
        self.acp_mut().ai_streaming = false;
        match res {
            Ok(reply) => {
                self.acp_mut().ai_messages.push(AiMessage {
                    role: "assistant".to_string(),
                    content: reply,
                });
            }
            Err(e) => {
                self.message = format!("AI error: {e}");
            }
        }
        // #1512: the curl transport's analogue of `AcpEvent::PromptStopped`
        // — send whatever queued up while this request was in flight.
        self.ai_dispatch_queued_message();
        true
    }

    /// Clear the AI conversation history and cancel any in-flight request.
    ///
    /// When the panel is on the ACP transport, this also drops the live
    /// agent (`AcpClient::drop` kills the subprocess) rather than just
    /// clearing the local transcript — matching the curl transport's
    /// "conversation cleared" semantics: the next message starts a fresh
    /// `initialize` -> `session/new` handshake, not a continuation of
    /// whatever context the old agent process held.
    pub fn ai_clear(&mut self) {
        // #953 (ACP-2): a parked permission prompt must get its one reply
        // before the client (and its stdin) goes away below — the agent is
        // still alive at this point, only about to be killed.
        self.acp_cancel_pending_permission();
        self.acp_mut().remembered_decisions.clear();
        // #957 (ACP-6): the auth-choice dialog holds no reply to send (unlike
        // `acp_pending_permission`, it isn't a parked agent request — see
        // `"acp_auth_choice"`'s `process_dialog_result` arm), so it just
        // needs closing before `acp_auth_methods` (which it reads by id)
        // clears below, same as any other dialog referencing state this
        // function is about to drop.
        if self
            .dialog
            .as_ref()
            .is_some_and(|d| d.tag == "acp_auth_choice")
        {
            self.dialog = None;
        }
        self.acp_mut().ai_messages.clear();
        // #1510: drop cached markdown renders alongside the transcript they
        // describe — otherwise the next conversation's messages could land
        // at the same indices and (if a coincidentally equal content length
        // ever occurred) read a stale render meant for different text.
        self.acp_mut().markdown_turn_cache.get_mut().clear();
        self.ai_rx = None;
        self.acp_mut().ai_streaming = false;
        self.acp_mut().client = None;
        self.acp_mut().session_id = None;
        self.acp_mut().pending_prompt = None;
        // #1459: an unconsumed resume request (`:AiSessions` picked a
        // session, then the user ran `:AiClear` before the handshake
        // finished) must not resurface on whatever session starts next —
        // same for its deferred display line, if the auto-resume path had
        // queued one.
        self.acp_pending_resume = None;
        self.acp_mut().pending_prompt_display = None;
        // #1512: a queued message belongs to the conversation being
        // cleared — nothing left to send it to, or to un-dim once this
        // clear wipes the transcript it was queued into.
        self.acp_mut().queued_prompt = None;
        self.acp_mut().queued_prompt_idx = None;
        self.acp_mut().streaming_turn = None;
        // #956 (ACP-5): plan/commands/modes/usage are all session-scoped —
        // clearing the conversation ends the session, so none of it should
        // survive into whatever session starts next (same reasoning as
        // `acp_remembered_decisions.clear()` above).
        self.acp_mut().plan.clear();
        self.acp_mut().available_commands.clear();
        self.acp_mut().command_completion_idx = 0;
        self.acp_mut().modes.clear();
        self.acp_mut().current_mode_id = None;
        self.acp_mut().usage = None;
        // #957 (ACP-6): session-scoped, same as the rest above.
        self.acp_mut().auth_methods.clear();
        self.acp_mut().authenticated = false;
        // #1449: `promptCapabilities` came off the same `initialize`
        // response as `authMethods` — reset alongside it.
        self.acp_mut().prompt_capabilities = crate::core::acp::AcpPromptCapabilities::default();
        // #1487: same reasoning — `mcpCapabilities` came off that same
        // response, and the MCP servers the (now-ending) session started
        // with no longer apply to whatever session starts next.
        self.acp_mut().mcp_capabilities = crate::core::acp::AcpMcpCapabilities::default();
        self.acp_mut().active_mcp_servers.clear();
        // #1450: a staged Visual-selection/`:{range}AI` attachment is
        // composed content the user hasn't sent yet — clearing the
        // conversation drops it too, same as clearing the typed-but-
        // unsubmitted input would (the input itself is `ChatController`'s
        // own state, untouched here, matching this function's existing
        // scope).
        self.acp_pending_attachment = None;
        // #1464: manually attached files/images are composed content too,
        // same reasoning as `acp_pending_attachment` immediately above.
        self.acp_manual_attachments.clear();
        // #1513: staged `@symbol` mentions are composed content too, same
        // reasoning.
        self.acp_pending_symbol_mentions.clear();
        // #955 (ACP-4): tool calls and any open change-review surface are
        // session-scoped too — closing the conversation without deciding
        // still discards the surface itself (same "closing the session
        // ends it" reasoning as everything else in this block).
        self.acp_mut().tool_calls.clear();
        // #1511: the tool-call/thought expand state and the anchor/kind
        // maps `populate_ai_chat_controller` builds them from are all
        // session-scoped too, same reasoning as `tool_calls` just above —
        // otherwise a fresh conversation's tool calls could reuse an id or
        // index a previous one left expanded.
        self.acp_mut().tool_call_anchor.clear();
        self.acp_mut().tool_call_expanded.clear();
        self.acp_mut().thought_expanded.clear();
        self.acp_mut().transcript_turn_kinds.get_mut().clear();
        self.change_review = None;
        // #1460: the per-turn tracking + checkpoint history are session-
        // scoped too, same reasoning as `acp_tool_calls`/`change_review`
        // just above — clearing the conversation ends any restore point a
        // later `:AiRestore` could have picked from it.
        self.acp_current_turn_entries.clear();
        self.acp_turn_checkpoints.clear();
        self.turn_review_checkpoint_id = None;
        self.ai_chat.borrow_mut().set_transcript_scroll_top(0);
        // #1463: clearing the tab's transcript must also clear its label,
        // or `AcpSession::is_blank` would keep reporting this tab as
        // "used" forever — `:AiClose`-ing the last remaining tab (which
        // resets it via this same function, see `Engine::
        // acp_close_session`'s doc) needs `is_blank` to flip back to
        // `true` so a later `:AiNew` reuses it instead of piling up a
        // second tab next to a supposedly-empty one.
        self.acp_mut().label = String::new();
        self.message = "AI conversation cleared.".to_string();
    }

    /// Slash-command completions matching the AI panel input's current
    /// text, if the agent has declared any via `available_commands_update`
    /// and the input looks like a command still being typed (#956, ACP-5).
    ///
    /// `None` — never an empty popup — when the input doesn't start with
    /// `/`, already has a space after the command name (the user is past
    /// the command name into its arguments/body), or nothing matches.
    /// Reuses `render::CompletionMenu` — the same shape the editor's own
    /// word-completion popup uses — per the issue's "prefer vimcode's
    /// existing completion machinery" guidance; there is no bespoke widget
    /// here, only a different feeder for one that already exists.
    pub fn ai_command_completions(&self) -> Option<crate::render::CompletionMenu> {
        if self.acp().available_commands.is_empty() {
            return None;
        }
        let input = self.ai_chat.borrow().input_text().to_string();
        let prefix = input.strip_prefix('/')?;
        if prefix.contains(char::is_whitespace) {
            return None;
        }
        let prefix_lower = prefix.to_lowercase();
        let mut candidates: Vec<String> = self
            .acp()
            .available_commands
            .iter()
            .filter(|c| c.name.to_lowercase().starts_with(&prefix_lower))
            .map(|c| format!("/{}", c.name))
            .collect();
        if candidates.is_empty() {
            return None;
        }
        candidates.sort();
        let max_width = candidates
            .iter()
            .map(|c| c.chars().count())
            .max()
            .unwrap_or(0);
        let selected_idx = self.acp().command_completion_idx.min(candidates.len() - 1);
        Some(crate::render::CompletionMenu {
            candidates,
            selected_idx,
            max_width,
        })
    }

    /// Advance the slash-command completion selection to the next
    /// candidate (wrapping), if the popup is currently showing. Returns
    /// `false` (a no-op) when [`Self::ai_command_completions`] is `None`.
    pub fn ai_command_completion_cycle(&mut self) -> bool {
        let Some(menu) = self.ai_command_completions() else {
            return false;
        };
        self.acp_mut().command_completion_idx = (menu.selected_idx + 1) % menu.candidates.len();
        true
    }

    /// Accept the currently-selected slash-command completion: replace the
    /// AI panel input's whole text with `"/name "` (trailing space, ready
    /// for arguments). Invoking the command afterward is nothing more than
    /// submitting that text normally — the ACP v1 spec has no separate RPC
    /// for it (`ai_send_message` already sends the input verbatim as
    /// prompt content, slash prefix and all). Returns `false` (a no-op)
    /// when [`Self::ai_command_completions`] is `None`.
    pub fn ai_command_accept_selected(&mut self) -> bool {
        let Some(menu) = self.ai_command_completions() else {
            return false;
        };
        let chosen = menu.candidates[menu.selected_idx].clone();
        let mut chat = self.ai_chat.borrow_mut();
        chat.clear_input();
        chat.input_insert_str(&format!("{chosen} "));
        true
    }

    /// `@`-mention completions for the AI panel input (#1449): open
    /// buffers first, then workspace files, filtered by whatever's typed
    /// after the `@` in the trailing word currently under construction
    /// (see [`crate::core::acp::trailing_at_mention_query`] for why it's
    /// the *trailing* word rather than the *whole* input, unlike the
    /// slash-command case above). `None` — never an empty popup — when the
    /// trailing word isn't a `@mention` in progress, or nothing matches.
    /// Reuses `render::CompletionMenu`, same shape/widget as
    /// [`Self::ai_command_completions`] — there is no second popup here,
    /// only a different feeder.
    pub fn ai_mention_completions(&self) -> Option<crate::render::CompletionMenu> {
        let input = self.ai_chat.borrow().input_text().to_string();
        let (_, query) = crate::core::acp::trailing_at_mention_query(&input)?;
        let query_lower = query.to_lowercase();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Open buffers first (#1449's ordering requirement).
        let mut buffer_matches: Vec<String> = self
            .buffer_manager
            .iter()
            .filter_map(|(_, state)| {
                let path = state.file_path.as_ref()?;
                let rel = path.strip_prefix(&self.cwd).unwrap_or(path);
                Some(rel.to_string_lossy().into_owned())
            })
            .filter(|display| display.to_lowercase().contains(&query_lower))
            .filter(|display| seen.insert(display.clone()))
            .collect();
        buffer_matches.sort();

        // Then workspace files — same ignore-aware walk
        // `picker_populate_files` uses, capped so a huge repo can't make
        // every keystroke slow.
        const MAX_CANDIDATES: usize = 20;
        const MAX_SCANNED: usize = 5000;
        let mut file_matches: Vec<String> = Vec::new();
        if buffer_matches.len() < MAX_CANDIDATES {
            let show_hidden = self.settings.show_hidden_files;
            // #1545: same `explorer_exclude` pruning `picker_populate_files`
            // applies — with dotfiles shown by default the walk would burn
            // its `MAX_SCANNED` budget inside `.git/` and surface object
            // files as `@file` completions.
            let exclude = self.settings.explorer_exclude.clone();
            let walker = ignore::WalkBuilder::new(&self.cwd)
                .hidden(!show_hidden)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .filter_entry(move |entry| {
                    !super::explorer_ops::walk_entry_is_excluded(entry, &exclude)
                })
                .build();
            for (scanned, entry) in walker.enumerate() {
                if file_matches.len() + buffer_matches.len() >= MAX_CANDIDATES
                    || scanned >= MAX_SCANNED
                {
                    break;
                }
                let Ok(entry) = entry else { continue };
                // #1513: `@dir` mentions — a directory entry is a candidate
                // too, not just files, marked with a trailing `/` so it's
                // unambiguous at accept/send time (`acp_mention_content_
                // blocks` branches on exactly that suffix). Depth 0 is the
                // walk root itself (`self.cwd`) — never offered as `@.` /
                // `@` (empty relative path).
                let is_dir = entry.file_type().map(|f| f.is_dir()).unwrap_or(false);
                let is_file = entry.file_type().map(|f| f.is_file()).unwrap_or(false);
                if !(is_file || (is_dir && entry.depth() > 0)) {
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(&self.cwd) else {
                    continue;
                };
                let mut display = rel.to_string_lossy().into_owned();
                if is_dir {
                    display.push('/');
                }
                if display.to_lowercase().contains(&query_lower) && seen.insert(display.clone()) {
                    file_matches.push(display);
                }
            }
            file_matches.sort();
        }

        // #1513: `@symbol` candidates — from `self.ai_mention_symbol_cache`
        // (last `workspace/symbol` response `Engine::ai_mention_tick`
        // fetched for this query; see that method's doc for why the
        // request itself can't happen from this `&self` method). Formatted
        // `@path#Name` — unambiguous at both accept time (`ai_mention_
        // accept_selected` branches on the `#`) and send time
        // (`Engine::acp_mention_content_blocks` skips any mention
        // containing `#`, since the range only exists in `ai_mention_
        // symbol_cache`, not in anything re-derivable from the text
        // alone). Appended after files/dirs, same ordering rationale
        // (buffers, then files, then the broader/fuzzier symbol search).
        let mut symbol_matches: Vec<String> = Vec::new();
        let workspace_cwd = self.acp_workspace_cwd();
        for sym in &self.ai_mention_symbol_cache {
            if symbol_matches.len() + file_matches.len() + buffer_matches.len() >= MAX_CANDIDATES {
                break;
            }
            if !sym.name.to_lowercase().contains(&query_lower) {
                continue;
            }
            let Some(path) = sym.path.as_ref() else {
                continue;
            };
            let display = crate::core::acp::workspace_relative_display(path, &workspace_cwd);
            let candidate = format!("{display}#{}", sym.name);
            if seen.insert(candidate.clone()) {
                symbol_matches.push(candidate);
            }
        }

        let candidates: Vec<String> = buffer_matches
            .into_iter()
            .chain(file_matches)
            .take(MAX_CANDIDATES)
            .map(|display| format!("@{display}"))
            .chain(
                symbol_matches
                    .into_iter()
                    .map(|display| format!("@{display}")),
            )
            .take(MAX_CANDIDATES)
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let max_width = candidates
            .iter()
            .map(|c| c.chars().count())
            .max()
            .unwrap_or(0);
        let selected_idx = self.acp().mention_completion_idx.min(candidates.len() - 1);
        Some(crate::render::CompletionMenu {
            candidates,
            selected_idx,
            max_width,
        })
    }

    /// Advance the `@`-mention completion selection to the next candidate
    /// (wrapping), if the popup is currently showing. Returns `false` (a
    /// no-op) when [`Self::ai_mention_completions`] is `None`.
    pub fn ai_mention_completion_cycle(&mut self) -> bool {
        let Some(menu) = self.ai_mention_completions() else {
            return false;
        };
        self.acp_mut().mention_completion_idx = (menu.selected_idx + 1) % menu.candidates.len();
        true
    }

    /// Accept the currently-selected `@`-mention completion: splice
    /// `"@path "` (trailing space) in place of the `@`-word currently under
    /// construction, leaving the rest of the input untouched. Returns
    /// `false` (a no-op) when [`Self::ai_mention_completions`] is `None`.
    pub fn ai_mention_accept_selected(&mut self) -> bool {
        let Some(menu) = self.ai_mention_completions() else {
            return false;
        };
        let chosen = menu.candidates[menu.selected_idx].clone();
        let input = self.ai_chat.borrow().input_text().to_string();
        let Some((start, _)) = crate::core::acp::trailing_at_mention_query(&input) else {
            return false;
        };
        self.acp_mut().mention_completion_idx = 0;
        // #1513: a `@symbol` candidate (`@path#Name`, see `ai_mention_
        // completions`' doc for the format) stages an `AcpSymbolMention`
        // right now, while `ai_mention_symbol_cache` still has the entry
        // it was built from — `acp_mention_content_blocks` (send time)
        // deliberately can't re-derive a line number from the literal text
        // alone, so this is the one moment the range is actually known.
        if let Some(rest) = chosen.strip_prefix('@') {
            if let Some((path_display, name)) = rest.split_once('#') {
                if let Some(sym) = self
                    .ai_mention_symbol_cache
                    .iter()
                    .find(|s| s.name == name)
                    .cloned()
                {
                    // `sym.path` is always `Some` for a `workspace/symbol`
                    // response (`parse_symbol_information` requires
                    // `location.uri`) — the `self.cwd`-joined fallback below
                    // only matters if that ever changes.
                    let path = sym
                        .path
                        .clone()
                        .unwrap_or_else(|| self.cwd.join(path_display));
                    self.acp_pending_symbol_mentions
                        .push(crate::core::acp::AcpSymbolMention {
                            path,
                            name: sym.name.clone(),
                            line: sym.line,
                            detail: sym.detail.clone(),
                        });
                }
            }
        }
        let mut new_input = input[..start].to_string();
        new_input.push_str(&chosen);
        new_input.push(' ');
        let mut chat = self.ai_chat.borrow_mut();
        chat.clear_input();
        chat.input_insert_str(&new_input);
        true
    }

    /// Keep `ai_mention_symbol_cache` fresh for `@symbol` completion
    /// (#1513) — called once per frame from `poll_idle`, mirroring `tick_
    /// ai_completion`'s "debounce counter checked every tick" shape but
    /// simpler: no counter, just "did the trailing `@`-mention query
    /// change since the last fetch". `ai_mention_completions` is `&self`
    /// (read-only, called from the paint path) and can't itself fire an
    /// LSP request, which is why this exists as a separate `&mut self`
    /// step instead of living inside that method.
    ///
    /// A no-op whenever: the AI panel doesn't have keyboard focus; the
    /// trailing word isn't a `@`-mention in progress; the query is shorter
    /// than 2 chars (same floor the Command Center's `#`-prefixed
    /// workspace-symbol picker mode uses — a 1-char workspace-wide symbol
    /// query is rarely useful and every keystroke would refire); the query
    /// contains `/` (almost certainly a file/dir mention, not a symbol
    /// name — skips a pointless LSP round trip on every path the user
    /// types); or the query hasn't changed since the last fetch already
    /// covering it.
    pub fn ai_mention_tick(&mut self) {
        if !self.ai_has_focus {
            return;
        }
        let input = self.ai_chat.borrow().input_text().to_string();
        let Some((_, query)) = crate::core::acp::trailing_at_mention_query(&input) else {
            self.ai_mention_symbol_query.clear();
            return;
        };
        if query.len() < 2 || query.contains('/') {
            return;
        }
        if query == self.ai_mention_symbol_query || self.lsp_pending_ai_mention_symbols.is_some() {
            return;
        }
        if !self.settings.lsp_enabled {
            return;
        }
        self.ensure_lsp_manager();
        let Some(path) = self.active_buffer_path() else {
            return;
        };
        let query = query.to_string();
        if let Some(mgr) = &mut self.lsp_manager {
            if let Some(id) = mgr.request_workspace_symbols(&path, &query) {
                self.lsp_pending_ai_mention_symbols = Some(id);
                self.ai_mention_symbol_query = query;
            }
        }
    }

    /// Intercept a plain, unmodified character key that might be starting,
    /// continuing, or completing the `<leader>ai` focus-toggle gesture
    /// (#1507) — checked by `render::route_ai_chat_event` *before* the key
    /// reaches `ChatController::handle`'s ordinary text-insertion path,
    /// exactly like that function's existing slash-command/`@`-mention Tab
    /// and Enter intercepts above it.
    ///
    /// Only engages while `self.ai_chat`'s input buffer is empty — the same
    /// convention Vim's own `<leader>` sequences follow by only ever being
    /// read from Normal mode's "nothing pending" state — chosen specifically
    /// so this gesture can never eat characters out of a message the user is
    /// actually composing: the moment the buffer holds anything, this
    /// returns `false` unconditionally and every key goes back to being
    /// ordinary typed text.
    ///
    /// A separate small buffer ([`Engine::ai_leader_toggle_pending`]) tracks
    /// the match rather than reusing the Normal-mode leader machinery
    /// (`Engine::leader_partial`/`Engine::handle_leader_key`): that machinery
    /// runs from `Engine::handle_key`'s own dispatch ladder, which
    /// `render::route_focus_key` never reaches while `ai_has_focus` is set —
    /// every keystroke goes straight to this panel instead (see
    /// `render::route_focus_key`'s doc). There is no path left for the
    /// existing "ai" arm in `Engine::handle_leader_key` to fire a *second*
    /// time once the panel already has focus, which is exactly the gap
    /// #1507 reports.
    ///
    /// Returns `true` when the key was consumed as part of the gesture
    /// (still matching, or matched in full — either way the caller must not
    /// also feed it to `ChatController::handle`). Returns `false` when the
    /// key breaks a match: any previously-buffered prefix is first replayed
    /// into the input verbatim (via `ChatController::input_insert_str`) so
    /// nothing typed is silently dropped, then the caller is free to run the
    /// *current* key through the normal path itself.
    pub fn ai_leader_toggle_key(&mut self, ch: char) -> bool {
        if !self.ai_chat.borrow().input_text().is_empty() {
            self.ai_leader_toggle_pending.clear();
            return false;
        }
        let pending = std::mem::take(&mut self.ai_leader_toggle_pending);
        let expected = match pending.chars().count() {
            0 => self.settings.leader,
            1 => 'a',
            _ => 'i',
        };
        if ch != expected {
            if !pending.is_empty() {
                self.ai_chat.borrow_mut().input_insert_str(&pending);
            }
            return false;
        }
        let mut matched = pending;
        matched.push(ch);
        if matched.chars().count() >= 3 {
            // Full `<leader>ai` match: toggle focus back to the editor,
            // mirroring `dispatch_ai_chat_event`'s `Cancelled` (Escape) arm.
            self.ai_has_focus = false;
        } else {
            self.ai_leader_toggle_pending = matched;
        }
        true
    }

    /// Replay any in-flight `<leader>ai` partial match back into the input
    /// verbatim and clear the buffer (#1507 review).
    ///
    /// `ai_leader_toggle_key` only ever sees a plain, unmodified
    /// [`quadraui::Key::Char`] — `render::route_ai_chat_event` calls this
    /// instead for every key that function can never see at all (a modified
    /// `Char`, or any `Named` key such as Enter/Tab/Backspace/arrows), so a
    /// partial match doesn't silently vanish just because the *interrupting*
    /// key happens to arrive on a path `ai_leader_toggle_key` was never
    /// wired to intercept. Mirrors the replay `ai_leader_toggle_key` itself
    /// already does on an ordinary same-shape mismatch (e.g. typing `<leader>x`
    /// after `<leader>a`) — this just extends that same "never silently drop
    /// buffered keystrokes" guarantee to keys outside its own dispatch.
    ///
    /// Callers must not use this for `Escape`: that key is about to fire
    /// `ChatControllerEvent::Cancelled` and leave the panel, so the buffer
    /// should be discarded (`ai_leader_toggle_pending.clear()`) rather than
    /// replayed into an input the user is walking away from — see
    /// `dispatch_ai_chat_event`'s `Cancelled` arm, which does exactly that.
    pub fn ai_leader_toggle_flush(&mut self) {
        let pending = std::mem::take(&mut self.ai_leader_toggle_pending);
        if !pending.is_empty() {
            self.ai_chat.borrow_mut().input_insert_str(&pending);
        }
    }

    /// Apply a [`quadraui::ChatControllerEvent`] the AI panel's `ChatController`
    /// (`self.ai_chat`) returned from `handle()`. Shared by GTK and TUI via
    /// `render::route_ai_chat_event` (#819 — the ChatController adoption that
    /// replaced this panel's hand-rolled `handle_ai_panel_key`/`ai_insert_text`).
    ///
    /// Returns whether the panel should keep keyboard focus — `false` only on
    /// `Cancelled` (Escape), mirroring every other panel's "Escape leaves the
    /// panel" convention now that the always-focused `ChatController` input has
    /// no separate "not editing" mode to fall back into.
    pub fn dispatch_ai_chat_event(&mut self, event: quadraui::ChatControllerEvent) -> bool {
        use quadraui::ChatControllerEvent as Ev;
        match event {
            Ev::Submit { text } => {
                self.ai_send_message(text);
                self.ai_chat.borrow_mut().clear_input();
                true
            }
            Ev::Cancelled => {
                self.ai_has_focus = false;
                // #1507 review: an abandoned `<leader>ai` partial match must
                // not survive a focus-losing Escape — replaying it here
                // would inject stray text into an input the user is walking
                // away from, and leaving it buffered would let it resurface
                // and wrongly complete against an unrelated future message
                // once the panel regains focus (unlike `ai_leader_toggle_flush`,
                // which is for keys that *don't* end the session).
                self.ai_leader_toggle_pending.clear();
                false
            }
            // #1509 (quadraui#1137): the clickable Send/Stop segment reads
            // "Stop" while `ChatController::set_busy(true)` — i.e. exactly
            // while `engine.acp().ai_streaming` — and clicking it there
            // emits this event instead of touching the input buffer.
            // Routes to the identical `session/cancel` path as `Ctrl+C`
            // below rather than a full `ai_clear`: a mouse "Stop" click is
            // the same "abort the running turn, keep the conversation"
            // gesture, just via click instead of keyboard.
            // `acp_cancel_turn` is a documented no-op with no session in
            // flight, so no extra guard is needed here.
            Ev::StopRequested => {
                self.acp_cancel_turn();
                true
            }
            // Ctrl+C: clear the conversation. `ChatController` has no
            // built-in binding for it (only Escape/Ctrl+S/Alt+Enter/
            // Ctrl+Enter/PageUp/PageDown/Ctrl+A/Ctrl+E are handled
            // internally), so it falls to this app-hotkey escape hatch,
            // exactly as its own doc comment recommends.
            //
            // #953 (ACP-2): while an ACP turn is actually in flight, Ctrl+C
            // aborts *that turn* (`session/cancel`) instead of nuking the
            // whole session — "the user must be able to abort a running
            // turn from the panel" without losing the agent process and
            // conversation history the way a full `ai_clear` would. Idle
            // (not streaming) keeps the existing full-clear behaviour.
            Ev::KeyPressed { key, modifiers } if modifiers.ctrl && key == "Char('c')" => {
                if self.acp_mut().client.is_some() && self.acp_mut().ai_streaming {
                    self.acp_cancel_turn();
                } else {
                    self.ai_clear();
                }
                true
            }
            // #1450 point 4 / #1464 / #1512: Ctrl+R drops the most-
            // recently-staged "about to send" thing without sending it —
            // "let the user remove it before sending". Checks the
            // Visual-selection/`:{range}AI` range attachment first
            // (unchanged #1450 behaviour), then falls through to popping
            // the last manually attached file/image (#1464) once that's
            // empty, then finally a message already queued while the
            // agent was busy (#1512, `ai_discard_queued_message`) — one
            // key, most-recent-first, across all three. Same escape-hatch
            // shape as Ctrl+C above: `ChatController` doesn't bind Ctrl+R
            // internally, so it reaches here as a plain `KeyPressed`. A
            // no-op (still consumes the key) when nothing is staged at
            // all.
            Ev::KeyPressed { key, modifiers } if modifiers.ctrl && key == "Char('r')" => {
                if self.acp_pending_attachment.take().is_some() {
                    self.message = "Attachment removed.".to_string();
                } else if let Some(removed) = self.acp_manual_attachments.pop() {
                    self.message = format!("Removed {}.", removed.chip());
                } else if self.ai_discard_queued_message() {
                    self.message = "Queued message discarded.".to_string();
                }
                true
            }
            // #1512: Ctrl+G ("go now") — cancel the in-flight turn and
            // immediately dispatch whatever's queued instead of waiting
            // for it to be sent automatically once the turn reaches
            // `PromptStopped` on its own. `ChatController` doesn't bind
            // Ctrl+G internally (unlike Enter/Ctrl+S/Ctrl+Enter, all of
            // which fully consume "submit" — including on an *empty*
            // input, where `try_submit` returns `Ignored` before this
            // function is ever called — which is why "send now" can't
            // reuse the ordinary submit chord), so it reaches here as a
            // plain `KeyPressed`, same as Ctrl+C/Ctrl+R above. A no-op
            // (still consumes the key) when nothing is queued.
            Ev::KeyPressed { key, modifiers } if modifiers.ctrl && key == "Char('g')" => {
                self.ai_send_queued_now();
                true
            }
            // #1511: a click landed on transcript turn `turn_idx`, row
            // `row_in_turn` into it (`quadraui::ChatController`'s own doc:
            // row 0 is always the role-header row). See
            // `Self::ai_chat_turn_clicked` for what each turn kind does
            // with it — a tool-call card's header toggles it, a click
            // inside its expanded body jumps to its first location, a
            // thought card toggles on any row (it's a single summary row
            // while collapsed).
            Ev::TurnClicked {
                turn_idx,
                row_in_turn,
            } => {
                self.ai_chat_turn_clicked(turn_idx, row_in_turn);
                true
            }
            _ => true,
        }
    }

    /// Apply a [`quadraui::ChatControllerEvent::TurnClicked`] (#1511): a
    /// click or the equivalent focused-turn `Enter` (see
    /// `render::route_ai_chat_event`'s Enter intercept, which resolves the
    /// same way this does before calling this method) landed on transcript
    /// turn `turn_idx`. Resolves `turn_idx` back to what it actually is via
    /// `AcpSession::transcript_turn_kinds` — populated fresh every frame by
    /// `render::populate_ai_chat_controller`, which always runs before this
    /// can be reached (see that field's own doc) — and toggles/acts on it:
    ///
    /// - A genuine thought turn ([`crate::render::is_genuine_thought_chunk`])
    ///   toggles regardless of `row_in_turn` — a collapsed thought card is
    ///   already exactly one summary row, so there's no header/body
    ///   distinction to make.
    /// - A tool-call card toggles on `row_in_turn <= 1` — row 0 is
    ///   `ChatController`'s own role-header row ("System"/"System ▸"), row 1
    ///   is the card's own title line (`tool_call_title_line`/
    ///   `tool_call_summary_line`'s first line), present at that same row
    ///   position whether the card is collapsed (it's the whole body) or
    ///   expanded (it's the body's first line) — so this is "click the
    ///   card's header", collapsed or not. Any higher `row_in_turn` (only
    ///   reachable once expanded, since a collapsed card is exactly rows 0
    ///   and 1) instead jumps to the call's first location, if it has one
    ///   (`Self::ai_open_tool_call_location`) — per-line resolution (mapping
    ///   a *specific* higher `row_in_turn` to a *specific* `-> path:line`)
    ///   isn't attempted: the card's word-wrapped body means a given
    ///   `row_in_turn` doesn't map losslessly back to a specific source line
    ///   the way it would for unwrapped text, so "first location" is the
    ///   honest, unsurprising behaviour rather than a heuristic that's right
    ///   most of the time and silently wrong the rest.
    /// - An ordinary message turn has no collapse state at all — ignored.
    pub fn ai_chat_turn_clicked(&mut self, turn_idx: usize, row_in_turn: usize) {
        use crate::core::acp_session::TranscriptTurnKind;
        let Some(kind) = self
            .acp()
            .transcript_turn_kinds
            .borrow()
            .get(turn_idx)
            .cloned()
        else {
            return;
        };
        match kind {
            TranscriptTurnKind::Message(idx) => {
                if crate::render::is_genuine_thought_chunk(&self.acp().ai_messages, idx) {
                    self.ai_chat_toggle_turn(turn_idx);
                }
            }
            TranscriptTurnKind::ToolCall(id) => {
                if row_in_turn <= 1 {
                    self.ai_chat_toggle_turn(turn_idx);
                } else if let Some(loc) = self
                    .acp()
                    .tool_calls
                    .iter()
                    .find(|c| c.id == id)
                    .and_then(|c| c.locations.first().cloned())
                {
                    self.ai_open_tool_call_location(&loc.0, loc.1);
                }
            }
        }
    }

    /// Flip `turn_idx`'s persistent expand state (#1511) — the shared
    /// bottom half of [`Self::ai_chat_turn_clicked`] and
    /// `render::route_ai_chat_event`'s focused-turn `Enter` intercept.
    /// Resolves `turn_idx` via `AcpSession::transcript_turn_kinds` (same as
    /// its caller) and writes to `AcpSession::thought_expanded`/
    /// `tool_call_expanded` — **not** `quadraui::ChatController`'s own
    /// internal collapsed-turn map, which `populate_ai_chat_controller`
    /// treats as a pure render target and overwrites from these two maps
    /// every frame (see that function's doc on why the vimcode-side maps,
    /// not quadraui's, are the source of truth).
    pub(crate) fn ai_chat_toggle_turn(&mut self, turn_idx: usize) {
        use crate::core::acp_session::TranscriptTurnKind;
        let Some(kind) = self
            .acp()
            .transcript_turn_kinds
            .borrow()
            .get(turn_idx)
            .cloned()
        else {
            return;
        };
        match kind {
            TranscriptTurnKind::Message(idx) => {
                if !self.acp_mut().thought_expanded.remove(&idx) {
                    self.acp_mut().thought_expanded.insert(idx);
                }
            }
            TranscriptTurnKind::ToolCall(id) => {
                if !self.acp_mut().tool_call_expanded.remove(&id) {
                    self.acp_mut().tool_call_expanded.insert(id);
                }
            }
        }
    }

    /// Open `path` (resolved against the ACP workspace root if relative) and
    /// jump the active window's cursor to `line` (#1511, "jump to a
    /// location" — a `tool_call`'s `locations` line is 1-based per the ACP
    /// v1 schema, [`crate::core::acp::AcpToolCallInfo`]'s own doc; internal
    /// cursor positions are 0-indexed, same conversion
    /// `Engine::open_search_result` already does for project-search
    /// results). `line: None` just opens the file without moving the
    /// cursor.
    pub(crate) fn ai_open_tool_call_location(&mut self, path: &str, line: Option<u32>) {
        let p = std::path::Path::new(path);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.acp_workspace_cwd().join(p)
        };
        self.open_file_in_tab(&p);
        if let Some(line) = line {
            let wid = self.active_window_id();
            let line0 = (line as usize).saturating_sub(1);
            self.set_cursor_for_window(wid, line0, 0);
            self.ensure_cursor_visible();
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

/// Filter an AI panel transcript down to the turns valid as conversation
/// history for the direct-provider (`curl`) transport — only `"user"` and
/// `"assistant"` roles.
///
/// Review regression (#952): if the panel previously talked over ACP
/// (`Engine::ai_send_message_via_acp`), `self.acp_mut().ai_messages` can also contain
/// ACP-only roles like `"assistant-thought"` (real `agent_thought_chunk`
/// reasoning, and the system/error notices `Engine::poll_acp` appends — see
/// `acp_ops.rs`). A user who switches transports mid-session by clearing
/// `acp_agent_command` without running `:AiClear` would otherwise have
/// those roles sent verbatim in the request body
/// (`crate::core::ai::send_chat`'s `messages_to_json`/`send_ollama`), which
/// none of Anthropic/OpenAI/Ollama recognise and will reject.
fn curl_transport_history(messages: &[AiMessage]) -> Vec<AiMessage> {
    messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .cloned()
        .collect()
}

#[cfg(test)]
mod curl_transport_history_tests {
    use super::*;

    #[test]
    fn keeps_user_and_assistant_turns_unchanged() {
        let messages = vec![
            AiMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            },
            AiMessage {
                role: "assistant".to_string(),
                content: "hello".to_string(),
            },
        ];
        let filtered = curl_transport_history(&messages);
        let roles: Vec<&str> = filtered.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant"]);
    }

    /// RED verified: removing the `.filter(...)` call (sending
    /// `self.acp_mut().ai_messages.clone()` straight through) makes this fail — the
    /// stray `"assistant-thought"` turn survives into the direct-provider
    /// request body.
    #[test]
    fn drops_acp_only_assistant_thought_role() {
        let messages = vec![
            AiMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            },
            AiMessage {
                role: "assistant-thought".to_string(),
                content: "\u{26a0} session/prompt failed: boom".to_string(),
            },
            AiMessage {
                role: "assistant".to_string(),
                content: "hello".to_string(),
            },
        ];
        let filtered = curl_transport_history(&messages);
        let roles: Vec<&str> = filtered.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            vec!["user", "assistant"],
            "the ACP-only assistant-thought turn must not reach a \
             direct-provider request body: {filtered:?}"
        );
    }
}

#[cfg(test)]
mod ai_mention_completions_exclude_tests {
    use crate::core::Engine;

    /// #1545: `ai_mention_completions`' workspace-file walk must prune
    /// `explorer_exclude` entries via `filter_entry`, same as
    /// `picker_populate_files` — otherwise, with `show_hidden_files`
    /// defaulting on, typing `@` would walk into `.git/` and could surface
    /// its object files as `@file` completion candidates (and burn the
    /// `MAX_SCANNED` budget doing it in a repo with a large `.git/`).
    ///
    /// RED-verified against the branch without the `filter_entry` prune:
    /// commenting it out makes `.git/HEAD1545extpanel` show up in
    /// `menu.candidates` below (an empty query lists every scanned file).
    #[test]
    fn does_not_surface_git_internals_as_at_file_completions() {
        let mut engine = Engine::new_for_test();
        let workspace = std::env::temp_dir().join(format!(
            "vc1545_ext_panel_mention_exclude_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(workspace.join(".git")).expect("create .git dir");
        std::fs::write(
            workspace.join(".git").join("HEAD1545extpanel"),
            b"ref: refs/heads/main\n",
        )
        .expect("write .git/HEAD");
        std::fs::write(workspace.join("plain1545.rs"), "").expect("write plain file");

        engine.cwd = workspace.clone();
        engine.workspace_root = Some(workspace.clone());
        engine
            .ai_chat
            .borrow_mut()
            .input_insert_str("look at @1545");

        let menu = engine
            .ai_mention_completions()
            .expect("typing @ with a matching prefix should show mention completions");

        assert!(
            menu.candidates.contains(&"@plain1545.rs".to_string()),
            "the ordinary workspace file must still be offered: {:?}",
            menu.candidates
        );
        assert!(
            !menu
                .candidates
                .iter()
                .any(|c| c.contains("HEAD1545extpanel")),
            "'.git/' internals must never surface as @file completions \
             (#1545): {:?}",
            menu.candidates
        );

        let _ = std::fs::remove_dir_all(&workspace);
    }
}

#[cfg(test)]
mod ai_leader_toggle_key_tests {
    use crate::core::Engine;

    /// A full `<leader>ai` match (default leader: Space) while the chat
    /// input is empty toggles `ai_has_focus` off and leaves the pending
    /// buffer clean — the unit-level twin of the driver tests in
    /// `tui_main::app_on_tui_tests`/`gtk::testing` (#1507).
    #[test]
    fn full_match_toggles_focus_off() {
        let mut engine = Engine::new_for_test();
        engine.ai_has_focus = true;
        assert!(engine.ai_leader_toggle_key(' '), "1st key must be consumed");
        assert!(engine.ai_has_focus, "still armed after 1 of 3 keys");
        assert!(engine.ai_leader_toggle_key('a'), "2nd key must be consumed");
        assert!(engine.ai_has_focus, "still armed after 2 of 3 keys");
        assert!(engine.ai_leader_toggle_key('i'), "3rd key must be consumed");
        assert!(
            !engine.ai_has_focus,
            "the full <leader>ai sequence must toggle focus off"
        );
        assert_eq!(engine.ai_leader_toggle_pending, "");
    }

    /// A key that breaks the match replays the buffered prefix into the
    /// chat input verbatim and reports "not consumed", so the caller feeds
    /// the breaking key through the ordinary text-insertion path itself —
    /// nothing typed is silently dropped.
    ///
    /// RED verified: with the `if !pending.is_empty() { ...
    /// input_insert_str(&pending) }` replay removed from
    /// `ai_leader_toggle_key`, this fails — the input stays empty instead
    /// of holding the replayed `" a"` prefix.
    #[test]
    fn mismatch_replays_buffered_prefix_into_input() {
        let mut engine = Engine::new_for_test();
        engine.ai_has_focus = true;
        assert!(engine.ai_leader_toggle_key(' '));
        assert!(engine.ai_leader_toggle_key('a'));
        // 'x' breaks the "<leader>ai" match at the third key.
        assert!(!engine.ai_leader_toggle_key('x'));
        assert!(engine.ai_has_focus, "a broken match must not toggle focus");
        assert_eq!(
            engine.ai_chat.borrow().input_text(),
            " a",
            "the buffered ' a' prefix must be replayed into the input \
             verbatim rather than silently dropped"
        );
        assert_eq!(engine.ai_leader_toggle_pending, "");
    }

    /// Once the input already holds text, every key is ordinary typed text
    /// — the gesture never engages, so a message containing " ai" is never
    /// at risk of being swallowed as the toggle.
    #[test]
    fn never_engages_once_input_is_non_empty() {
        let mut engine = Engine::new_for_test();
        engine.ai_has_focus = true;
        engine.ai_chat.borrow_mut().input_insert_str("hello");
        assert!(!engine.ai_leader_toggle_key(' '));
        assert!(!engine.ai_leader_toggle_key('a'));
        assert!(!engine.ai_leader_toggle_key('i'));
        assert!(
            engine.ai_has_focus,
            "typing ' ai' into a non-empty input must never toggle focus"
        );
    }

    /// #1507 review: `ai_leader_toggle_flush` — the method
    /// `render::route_ai_chat_event` calls for every key shape
    /// `ai_leader_toggle_key` itself never sees (a modified `Char`, or any
    /// `Named` key) — must replay a buffered partial match into the input
    /// and clear the buffer, the unit-level twin of the black-box
    /// `leader_prefix_interrupted_by_enter_is_replayed_not_dropped_via_shell_app`
    /// driver test.
    ///
    /// RED verified: with `ai_leader_toggle_flush`'s body replaced with a
    /// bare `self.ai_leader_toggle_pending.clear();` (i.e. discarding
    /// instead of replaying), this fails.
    #[test]
    fn flush_replays_buffered_prefix_into_input() {
        let mut engine = Engine::new_for_test();
        engine.ai_has_focus = true;
        assert!(engine.ai_leader_toggle_key(' '));
        assert!(engine.ai_leader_toggle_key('a'));
        assert_eq!(engine.ai_leader_toggle_pending, " a");

        engine.ai_leader_toggle_flush();

        assert_eq!(
            engine.ai_chat.borrow().input_text(),
            " a",
            "the buffered ' a' prefix must be replayed into the input"
        );
        assert_eq!(
            engine.ai_leader_toggle_pending, "",
            "the buffer must be cleared once flushed"
        );
    }

    /// `ai_leader_toggle_flush` on an empty buffer is a no-op — the common
    /// case, since most keys arrive with no partial match pending at all.
    #[test]
    fn flush_is_a_no_op_when_nothing_is_pending() {
        let mut engine = Engine::new_for_test();
        engine.ai_has_focus = true;
        engine.ai_leader_toggle_flush();
        assert_eq!(engine.ai_chat.borrow().input_text(), "");
    }

    /// #1507 review, "related" point: `Cancelled` (Escape) must discard —
    /// not replay — a partial match still buffered when the panel loses
    /// focus, so it can never resurface and wrongly complete against an
    /// unrelated later message once the panel regains focus. The black-box
    /// twin of this is
    /// `leader_prefix_abandoned_via_escape_does_not_leak_into_next_session_via_shell_app`.
    ///
    /// RED verified: with the `self.ai_leader_toggle_pending.clear();` line
    /// removed from `dispatch_ai_chat_event`'s `Cancelled` arm, this fails.
    #[test]
    fn cancelled_discards_a_buffered_partial_match() {
        let mut engine = Engine::new_for_test();
        engine.ai_has_focus = true;
        assert!(engine.ai_leader_toggle_key(' '));
        assert!(engine.ai_leader_toggle_key('a'));
        assert_eq!(engine.ai_leader_toggle_pending, " a");

        let still_focused = engine.dispatch_ai_chat_event(quadraui::ChatControllerEvent::Cancelled);

        assert!(!still_focused);
        assert!(!engine.ai_has_focus);
        assert_eq!(
            engine.ai_leader_toggle_pending, "",
            "an abandoned partial match must not survive Escape"
        );
        assert_eq!(
            engine.ai_chat.borrow().input_text(),
            "",
            "an abandoned partial match must be discarded, not replayed, \
             into an input the user is walking away from"
        );
    }
}

#[cfg(test)]
mod issue_1513_at_symbol_and_at_dir_mentions {
    use crate::core::Engine;

    /// #1513: a workspace-symbol candidate from `ai_mention_symbol_cache`
    /// must show up in `ai_mention_completions` formatted `@path#Name`
    /// (the unambiguous-at-send-time convention `acp_mention_content_
    /// blocks`/`ai_mention_accept_selected` both rely on), filtered by the
    /// query, and appear after any file/dir matches.
    #[test]
    fn symbol_candidates_are_offered_formatted_path_hash_name() {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::env::temp_dir();
        engine.workspace_root = Some(engine.cwd.clone());
        engine.ai_mention_symbol_cache = vec![
            crate::core::lsp::SymbolInfo {
                name: "MyStruct".to_string(),
                kind: crate::core::lsp::SymbolKind::Struct,
                detail: Some("struct MyStruct".to_string()),
                container: None,
                path: Some(engine.cwd.join("src/lib.rs")),
                line: 41,
                character: 0,
                children: Vec::new(),
            },
            crate::core::lsp::SymbolInfo {
                name: "unrelated_fn".to_string(),
                kind: crate::core::lsp::SymbolKind::Function,
                detail: None,
                container: None,
                path: Some(engine.cwd.join("src/lib.rs")),
                line: 5,
                character: 0,
                children: Vec::new(),
            },
        ];
        engine
            .ai_chat
            .borrow_mut()
            .input_insert_str("look at @MyStr");

        let menu = engine
            .ai_mention_completions()
            .expect("a matching symbol query should show mention completions");
        assert!(
            menu.candidates.iter().any(|c| c == "@src/lib.rs#MyStruct"),
            "expected a @path#Name symbol candidate: {:?}",
            menu.candidates
        );
        assert!(
            !menu.candidates.iter().any(|c| c.contains("unrelated_fn")),
            "a non-matching symbol must be filtered out: {:?}",
            menu.candidates
        );
    }

    /// #1513: accepting a `@path#Name` symbol candidate stages an
    /// `AcpSymbolMention` (`Engine::acp_pending_symbol_mentions`) carrying
    /// the exact line the cached `SymbolInfo` reported, and splices the
    /// literal `@path#Name ` text into the input — the same "chosen text
    /// verbatim, plus a trailing space" contract `ai_mention_accept_
    /// selected` already has for files.
    ///
    /// RED verified: with the `chosen.strip_prefix('@')`/`split_once('#')`
    /// staging block removed from `ai_mention_accept_selected`, `acp_
    /// pending_symbol_mentions` stays empty and the second assertion below
    /// fails.
    #[test]
    fn accepting_a_symbol_candidate_stages_a_symbol_mention_and_splices_the_text() {
        let mut engine = Engine::new_for_test();
        engine.cwd = std::env::temp_dir();
        engine.workspace_root = Some(engine.cwd.clone());
        engine.ai_mention_symbol_cache = vec![crate::core::lsp::SymbolInfo {
            name: "MyStruct".to_string(),
            kind: crate::core::lsp::SymbolKind::Struct,
            detail: Some("struct MyStruct".to_string()),
            container: None,
            path: Some(engine.cwd.join("src/lib.rs")),
            line: 41,
            character: 0,
            children: Vec::new(),
        }];
        engine
            .ai_chat
            .borrow_mut()
            .input_insert_str("look at @MyStr");

        assert!(engine.ai_mention_accept_selected());

        assert_eq!(
            engine.ai_chat.borrow().input_text(),
            "look at @src/lib.rs#MyStruct ",
            "the literal @path#Name text must be spliced in verbatim"
        );
        assert_eq!(
            engine.acp_pending_symbol_mentions.len(),
            1,
            "accepting a symbol candidate must stage an AcpSymbolMention"
        );
        assert_eq!(engine.acp_pending_symbol_mentions[0].name, "MyStruct");
        assert_eq!(engine.acp_pending_symbol_mentions[0].line, 41);
    }

    /// #1513: a directory entry is offered as an `@`-mention candidate too,
    /// marked with a trailing `/` — `ai_mention_completions`' unambiguous
    /// marker for "this resolves to a directory, not a file" that `acp_
    /// mention_content_blocks` branches on at send time.
    #[test]
    fn directory_candidates_are_offered_with_a_trailing_slash() {
        let mut engine = Engine::new_for_test();
        let workspace = std::env::temp_dir().join(format!(
            "vimcode_test_1513_dir_candidate_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(workspace.join("subdir")).expect("create test dir");
        std::fs::write(workspace.join("subdir/a.rs"), "").expect("write file");
        engine.cwd = workspace.clone();
        engine.workspace_root = Some(workspace.clone());

        engine
            .ai_chat
            .borrow_mut()
            .input_insert_str("look at @subd");
        let menu = engine
            .ai_mention_completions()
            .expect("typing @ with a matching dir prefix should show mention completions");
        assert!(
            menu.candidates.contains(&"@subdir/".to_string()),
            "expected a trailing-slash directory candidate: {:?}",
            menu.candidates
        );

        let _ = std::fs::remove_dir_all(&workspace);
    }
}
