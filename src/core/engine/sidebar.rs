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
