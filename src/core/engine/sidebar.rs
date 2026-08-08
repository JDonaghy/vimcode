use super::Engine;

impl Engine {
    /// Check if a specific panel is the active sidebar panel.
    pub fn active_panel_is(&self, panel_id: &str) -> bool {
        self.app_shell
            .active_panel_id()
            .is_some_and(|id| id.as_str() == panel_id)
    }
}

pub const PANEL_EXPLORER: &str = "panel:explorer";
pub const PANEL_SEARCH: &str = "panel:search";
pub const PANEL_DEBUG: &str = "panel:debug";
pub const PANEL_GIT: &str = "panel:git";
pub const PANEL_EXTENSIONS: &str = "panel:extensions";
pub const PANEL_AI: &str = "panel:ai";
pub const PANEL_SETTINGS: &str = "bottom:settings";

/// The single source of truth for the fixed (non-hamburger, non-settings,
/// non-dynamic-extension) activity-bar panel order. `render::build_activity_bar`'s
/// `fixed` array and `tui_main::shell_app::TuiShellApp::shell_config`'s
/// `PanelDefinition` list both iterate this rather than hand-transcribing the
/// order twice — a reordering here is now a one-line change both call sites
/// pick up, instead of a silent drift only a snapshot test would catch.
pub const FIXED_ACTIVITY_PANEL_IDS: [&str; 6] = [
    PANEL_EXPLORER,
    PANEL_SEARCH,
    PANEL_DEBUG,
    PANEL_GIT,
    PANEL_EXTENSIONS,
    PANEL_AI,
];

impl Engine {
    /// Toggle a sidebar panel. If `panel_id` matches the active panel and the
    /// sidebar is already visible, hide it. Otherwise switch to the panel and
    /// show the sidebar. Focus flags are set/cleared automatically.
    pub fn toggle_sidebar_panel(&mut self, panel_id: &str) {
        let same = self
            .app_shell
            .active_panel_id()
            .is_some_and(|id| id.as_str() == panel_id);

        if same && self.app_shell.sidebar_visible() {
            self.app_shell.hide_sidebar();
            self.clear_sidebar_focus();
        } else {
            self.app_shell
                .show_panel(&quadraui::WidgetId::new(panel_id));
            self.clear_sidebar_focus();
            self.set_panel_focus(panel_id);
        }
        self.session.explorer_visible = self.app_shell.sidebar_visible();
        let _ = self.session.save();
    }

    /// Show a sidebar panel and give it focus (no toggle). Used for programmatic
    /// reveals like DAP session start.
    pub fn focus_sidebar_panel(&mut self, panel_id: &str) {
        self.app_shell
            .show_panel(&quadraui::WidgetId::new(panel_id));
        self.clear_sidebar_focus();
        self.set_panel_focus(panel_id);
        self.session.explorer_visible = true;
        let _ = self.session.save();
    }

    /// Map a panel ID to the correct engine focus flag.
    fn set_panel_focus(&mut self, panel_id: &str) {
        match panel_id {
            PANEL_EXPLORER => self.explorer_has_focus = true,
            PANEL_SEARCH => self.search_set_focus(true),
            PANEL_DEBUG => self.dap_sidebar_has_focus = true,
            PANEL_GIT => {
                self.sc_set_focus(true);
                self.sc_refresh();
            }
            PANEL_EXTENSIONS => {
                self.ext_sidebar_has_focus = true;
                if self.ext_registry.is_none() && !self.ext_registry_fetching {
                    self.ext_refresh();
                }
            }
            PANEL_AI => self.ai_has_focus = true,
            PANEL_SETTINGS => self.settings_has_focus = true,
            _ => {}
        }
    }

    /// Consume engine-internal sidebar flags. Called from `poll_idle()`.
    /// Returns `true` if a redraw is needed.
    pub(super) fn process_pending_sidebar(&mut self) -> bool {
        let mut dirty = false;

        if self.dap_wants_sidebar {
            self.dap_wants_sidebar = false;
            self.focus_sidebar_panel(PANEL_DEBUG);
            dirty = true;
        }

        dirty
    }

    /// Handle window-nav-overflow (Ctrl-W h/l past the last window).
    /// Call from the backend after processing keys. Sets engine focus flags
    /// and app_shell visibility; returns `Some(false)` for left overflow or
    /// `Some(true)` for right overflow if the backend needs to update local
    /// focus state, or `None` if nothing happened.
    pub fn handle_nav_overflow(&mut self) -> Option<bool> {
        let direction = self.window_nav_overflow.take()?;
        if !direction {
            if !self.app_shell.sidebar_visible() && self.settings.autohide_panels {
                if let Some(id) = self
                    .app_shell
                    .active_panel_id()
                    .map(|w| w.as_str().to_string())
                {
                    self.app_shell.show_panel(&quadraui::WidgetId::new(&id));
                    self.session.explorer_visible = true;
                    let _ = self.session.save();
                }
            }
            if self.app_shell.sidebar_visible() {
                if let Some(id) = self
                    .app_shell
                    .active_panel_id()
                    .map(|w| w.as_str().to_string())
                {
                    self.set_panel_focus(&id);
                }
            }
        }
        Some(direction)
    }

    /// Returns true if the sidebar should be auto-hidden (autohide setting
    /// is on, sidebar is visible, and no panel has focus).
    pub fn should_autohide_sidebar(&self) -> bool {
        self.settings.autohide_panels
            && self.app_shell.sidebar_visible()
            && !self.sidebar_has_focus()
    }

    /// Toggle sidebar visibility without changing the active panel.
    pub fn toggle_sidebar(&mut self) {
        self.app_shell.toggle_sidebar();
        if !self.app_shell.sidebar_visible() {
            self.clear_sidebar_focus();
        }
        self.session.explorer_visible = self.app_shell.sidebar_visible();
        let _ = self.session.save();
    }
}

// ─── Activity-bar keyboard focus state machine ────────────────────────────────

/// Outcome of activating (l/Enter) the currently selected activity bar item.
pub enum ActivityBarActivation {
    /// The menu-bar visibility was toggled.
    MenuToggled,
    /// A named sidebar panel was focused and made visible.
    PanelFocused,
    /// An extension panel (plugin-provided) was focused. Field is the panel name.
    ExtPanelFocused(String),
    /// Index was out of range; nothing happened.
    NoOp,
}

impl Engine {
    /// Give the activity bar keyboard focus at a specific toolbar index.
    ///
    /// Index mapping: 0=menu, 1=Explorer, 2=Search, 3=Debug, 4=Git,
    /// 5=Extensions, 6=AI, 7=Settings, 8+=extension panels (sorted by name).
    pub fn activity_bar_focus_in_at(&mut self, idx: u16) {
        self.activity_bar_focused = true;
        self.activity_bar_selected = idx;
    }

    /// Return the toolbar index that corresponds to the currently active panel.
    /// Falls back to 1 (Explorer) for unknown/extension panels.
    pub fn activity_bar_toolbar_idx_for_active_panel(&self) -> u16 {
        let id = self
            .app_shell
            .active_panel_id()
            .map(|w| w.as_str())
            .unwrap_or("");
        match id {
            PANEL_EXPLORER => 1,
            PANEL_SEARCH => 2,
            PANEL_DEBUG => 3,
            PANEL_GIT => 4,
            PANEL_EXTENSIONS => 5,
            PANEL_AI => 6,
            PANEL_SETTINGS => 7,
            _ => 1,
        }
    }

    /// Remove activity bar keyboard focus (return focus to the editor).
    pub fn activity_bar_focus_out(&mut self) {
        self.activity_bar_focused = false;
    }

    /// Move the keyboard cursor one position down in the activity bar.
    pub fn activity_bar_move_down(&mut self) {
        let ext_count = self.ext_panels.len() as u16;
        let max_ext = if ext_count > 0 { 7 + ext_count } else { 0 };
        let sel = self.activity_bar_selected;
        if sel < 6 {
            self.activity_bar_selected = sel + 1;
        } else if sel == 6 && ext_count > 0 {
            self.activity_bar_selected = 8; // first ext panel
        } else if sel == 6 {
            self.activity_bar_selected = 7; // settings
        } else if sel >= 8 && sel < max_ext {
            self.activity_bar_selected = sel + 1;
        } else if sel >= 8 && sel == max_ext {
            self.activity_bar_selected = 7; // settings
        }
        // sel == 7 (settings) → no movement (already at bottom)
    }

    /// Move the keyboard cursor one position up in the activity bar.
    pub fn activity_bar_move_up(&mut self) {
        let ext_count = self.ext_panels.len() as u16;
        let max_ext = if ext_count > 0 { 7 + ext_count } else { 0 };
        let sel = self.activity_bar_selected;
        if sel == 7 && ext_count > 0 {
            self.activity_bar_selected = max_ext; // settings → last ext
        } else if sel == 7 {
            self.activity_bar_selected = 6; // settings → AI
        } else if sel == 8 {
            self.activity_bar_selected = 6; // first ext → AI
        } else if sel > 8 {
            self.activity_bar_selected = sel - 1;
        } else {
            self.activity_bar_selected = sel.saturating_sub(1);
        }
    }

    /// Activate the currently selected activity bar item (l/Enter).
    ///
    /// Clears `activity_bar_focused` and updates engine state to focus the
    /// chosen panel. The backend should inspect the returned
    /// `ActivityBarActivation` to perform any backend-specific follow-up
    /// (e.g. setting `sidebar.has_focus`, closing TUI menu).
    pub fn activity_bar_activate(&mut self) -> ActivityBarActivation {
        let sel = self.activity_bar_selected;
        self.activity_bar_focused = false;
        match sel {
            0 => {
                self.toggle_menu_bar();
                ActivityBarActivation::MenuToggled
            }
            1..=6 => {
                let panel_id = match sel {
                    1 => PANEL_EXPLORER,
                    2 => PANEL_SEARCH,
                    3 => PANEL_DEBUG,
                    4 => PANEL_GIT,
                    5 => PANEL_EXTENSIONS,
                    _ => PANEL_AI,
                };
                self.ext_panel_has_focus = false;
                self.ext_panel_active = None;
                self.focus_sidebar_panel(panel_id);
                ActivityBarActivation::PanelFocused
            }
            7 => {
                self.ext_panel_has_focus = false;
                self.ext_panel_active = None;
                self.focus_sidebar_panel(PANEL_SETTINGS);
                ActivityBarActivation::PanelFocused
            }
            idx => {
                let ext_idx = (idx - 8) as usize;
                let mut ext_names: Vec<_> = self.ext_panels.keys().cloned().collect();
                ext_names.sort();
                if ext_idx < ext_names.len() {
                    let name = ext_names[ext_idx].clone();
                    if !self.app_shell.sidebar_visible() {
                        self.app_shell.show_panel(&quadraui::WidgetId::new(&name));
                        self.session.explorer_visible = true;
                        let _ = self.session.save();
                    }
                    self.ext_panel_active = Some(name.clone());
                    self.ext_panel_has_focus = true;
                    self.ext_panel_selected = 0;
                    self.plugin_event("panel_focus", &name);
                    ActivityBarActivation::ExtPanelFocused(name)
                } else {
                    ActivityBarActivation::NoOp
                }
            }
        }
    }
}
