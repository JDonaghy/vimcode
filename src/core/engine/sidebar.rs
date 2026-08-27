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

/// Activity-bar item id for the hamburger (menu) slot — keyboard index 0.
///
/// The TUI registers this as `panels[0]` of its live [`quadraui::AppShell`]
/// (`tui_main::shell_app::TuiShellApp::live_shell_config`) and
/// `render::build_activity_bar` mints the same id for its hamburger
/// `ActivityItem`, so it is the one activity-bar id that is *already* shared
/// across the two id spaces described on [`EXT_PANEL_ID_PREFIX`]. Promoted out
/// of `tui_main::shell_app` (#536) so [`Engine::activity_bar_item_id`] can name
/// the slot without the core depending on a backend module.
///
/// GTK paints no hamburger (`include_hamburger = false`), but the slot still
/// exists in the keyboard sequence there — `k` from Explorer lands on it and
/// `l`/Enter toggles the menu bar. That predates #536 and is preserved by it.
pub const HAMBURGER_PANEL_ID: &str = "activity:menu";

/// Panel-id prefix for plugin-provided ("extension") sidebar panels — e.g. the
/// `git-insights` extension's panel is `"ext:git-insights"` (#557).
///
/// This is the id both backends already synthesise by hand for extension
/// panels (GTK's `current_active_panel_id`/`Msg::SwitchPanel`), promoted to a
/// shared constant so [`Engine::ext_activity_panels`] can hand *the same* ids
/// to each backend's `ShellConfig` builder.
///
/// Not to be confused with the `"activity:ext:"` ids
/// `render::build_activity_bar` mints: those name *`ActivityItem`s on the
/// legacy `draw_frame` path*, a separate `activity:`-namespaced id space that
/// also covers the built-ins (`"activity:explorer"` vs `PANEL_EXPLORER`).
/// These are sidebar **panel** ids, the same space `PANEL_EXPLORER` and
/// friends live in.
pub const EXT_PANEL_ID_PREFIX: &str = "ext:";

/// The activity-bar/sidebar panel id for a plugin-registered panel `name`.
pub fn ext_panel_id(name: &str) -> String {
    format!("{EXT_PANEL_ID_PREFIX}{name}")
}

/// Inverse of [`ext_panel_id`]: the plugin panel name inside an `"ext:"` id,
/// or `None` when `id` names a built-in panel.
pub fn ext_panel_name_from_id(id: &str) -> Option<&str> {
    id.strip_prefix(EXT_PANEL_ID_PREFIX)
}

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

/// Keyboard (toolbar) index of the bottom-pinned Settings item.
///
/// The activity bar's *painted* order is hamburger, the fixed panels, the
/// dynamic extension panels, then Settings pinned to the bottom — but the
/// legacy `activity_bar_selected` index space numbers Settings **before** the
/// extension panels, because extension panels were added after the built-in
/// indices had already been baked into call sites like
/// `Engine::activity_bar_focus_in_at(7)`. That mismatch is exactly why the
/// up/down stepping used to need bespoke arithmetic; since #536 the stepping
/// is done by quadraui's `AppShell` cursor over the *painted* order and these
/// two constants are all that remains of the index space — a lookup table,
/// not a sequencing rule.
pub const TOOLBAR_IDX_SETTINGS: u16 = FIXED_ACTIVITY_PANEL_IDS.len() as u16 + 1;

/// First keyboard (toolbar) index occupied by a dynamic extension panel.
/// See [`TOOLBAR_IDX_SETTINGS`] for why extension panels sit *after* Settings
/// in the index space while painting *before* it.
pub const TOOLBAR_IDX_EXT_BASE: u16 = TOOLBAR_IDX_SETTINGS + 1;

impl Engine {
    /// Activity-bar [`quadraui::PanelDefinition`]s for every plugin-registered
    /// extension panel, sorted by name (#557).
    ///
    /// Both backends' `ShellConfig` builders — `tui_main::shell_app::
    /// TuiShellApp::live_shell_config` and `gtk::build_shell_config` — append
    /// this to their static panel list, and the TUI additionally re-syncs it
    /// into the live `AppShell` after every dispatch so a plugin that
    /// registers a panel *after* startup still gets an icon. Without it the
    /// migrated `AppShell` activity bar renders only the built-in panels and
    /// e.g. the Git Insights extension has no icon at all.
    ///
    /// Sorted by name to match the order `render::build_activity_bar` (the
    /// legacy `draw_frame` path) and `Engine::activity_bar_activate`'s `8 +
    /// idx` extension arm both use, so keyboard index and painted position
    /// agree on either path.
    ///
    /// The icon is [`crate::core::plugin::PanelRegistration::resolved_icon`],
    /// so it honours the caller thread's Nerd-Fonts flag; callers that rebuild
    /// this list per frame therefore pick up a runtime `:set nonerdfonts`.
    pub fn ext_activity_panels(&self) -> Vec<quadraui::PanelDefinition> {
        let mut panels: Vec<_> = self.ext_panels.values().collect();
        panels.sort_by(|a, b| a.name.cmp(&b.name));
        panels
            .into_iter()
            .map(|p| quadraui::PanelDefinition {
                id: quadraui::WidgetId::new(ext_panel_id(&p.name)),
                icon: p.resolved_icon().to_string(),
                tooltip: p.title.clone(),
                title: p.title.clone(),
            })
            .collect()
    }

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
        if id == PANEL_SETTINGS {
            return TOOLBAR_IDX_SETTINGS;
        }
        FIXED_ACTIVITY_PANEL_IDS
            .iter()
            .position(|p| *p == id)
            .map(|i| i as u16 + 1)
            .unwrap_or(1)
    }

    /// Remove activity bar keyboard focus (return focus to the editor).
    pub fn activity_bar_focus_out(&mut self) {
        self.activity_bar_focused = false;
    }

    /// The activity bar's items **in painted order**, as a throwaway
    /// [`quadraui::AppShell`] whose keyboard cursor (quadraui#386) does the
    /// stepping for [`Self::activity_bar_move_down`] / [`Self::activity_bar_move_up`].
    ///
    /// Panel list and bottom-item list mirror the TUI's live `ShellConfig`
    /// (`tui_main::shell_app::TuiShellApp::live_shell_config`) exactly:
    /// hamburger, the fixed panels, the dynamic extension panels sorted by
    /// name, then Settings pinned to the bottom. `AppShell`'s cursor spans
    /// `panels` then `bottom_items` as one sequence and saturates at both ends,
    /// which *is* vimcode's ordering — so no vimcode-side arithmetic is left.
    ///
    /// Built on demand rather than cached as a field: the ext-panel list is
    /// derived from `self.ext_panels`, which a plugin can mutate at any point
    /// in the session, so deriving it per keypress is what makes it impossible
    /// for the nav sequence to go stale. It is a `Vec` of ~8 empty-metadata
    /// `PanelDefinition`s built once per `j`/`k`, which is not worth caching.
    ///
    /// Only ids matter here, so icon/tooltip/title are left empty — nothing
    /// paints this shell.
    fn activity_nav_shell(&self) -> quadraui::AppShell {
        fn def(id: &str) -> quadraui::PanelDefinition {
            quadraui::PanelDefinition {
                id: quadraui::WidgetId::new(id),
                icon: String::new(),
                tooltip: String::new(),
                title: String::new(),
            }
        }
        let mut panels = Vec::with_capacity(1 + FIXED_ACTIVITY_PANEL_IDS.len());
        panels.push(def(HAMBURGER_PANEL_ID));
        panels.extend(FIXED_ACTIVITY_PANEL_IDS.into_iter().map(def));
        // Same list, same sort order, that both backends' `ShellConfig`
        // builders and `render::build_activity_bar` use.
        panels.extend(self.ext_activity_panels());
        quadraui::AppShell::new(panels, 0.0).with_bottom_items(vec![def(PANEL_SETTINGS)])
    }

    /// The activity-bar item id at keyboard (toolbar) index `idx`, or `None`
    /// when `idx` names no item (e.g. a stale extension index after a
    /// `:PluginReload` dropped the panel).
    ///
    /// Index mapping: 0 = hamburger, 1..=6 = [`FIXED_ACTIVITY_PANEL_IDS`],
    /// [`TOOLBAR_IDX_SETTINGS`] = Settings, [`TOOLBAR_IDX_EXT_BASE`]`+ k` =
    /// the `k`-th extension panel (sorted by name).
    pub fn activity_bar_item_id(&self, idx: u16) -> Option<String> {
        if idx == 0 {
            return Some(HAMBURGER_PANEL_ID.to_string());
        }
        if idx == TOOLBAR_IDX_SETTINGS {
            return Some(PANEL_SETTINGS.to_string());
        }
        if idx < TOOLBAR_IDX_SETTINGS {
            return Some(FIXED_ACTIVITY_PANEL_IDS[idx as usize - 1].to_string());
        }
        let k = (idx - TOOLBAR_IDX_EXT_BASE) as usize;
        self.sorted_ext_panel_names()
            .get(k)
            .map(|n| ext_panel_id(n))
    }

    /// Inverse of [`Self::activity_bar_item_id`].
    pub fn activity_bar_idx_for_item_id(&self, id: &str) -> Option<u16> {
        if id == HAMBURGER_PANEL_ID {
            return Some(0);
        }
        if id == PANEL_SETTINGS {
            return Some(TOOLBAR_IDX_SETTINGS);
        }
        if let Some(i) = FIXED_ACTIVITY_PANEL_IDS.iter().position(|p| *p == id) {
            return Some(i as u16 + 1);
        }
        let name = ext_panel_name_from_id(id)?;
        self.sorted_ext_panel_names()
            .iter()
            .position(|n| n == name)
            .map(|i| i as u16 + TOOLBAR_IDX_EXT_BASE)
    }

    /// The activity-bar item id currently under the keyboard cursor.
    ///
    /// `render::build_activity_bar` compares each `ActivityItem`'s panel id
    /// against this to set `is_keyboard_selected`, instead of re-deriving the
    /// item's numeric toolbar index at the paint site (#536).
    pub fn activity_bar_selected_item_id(&self) -> Option<String> {
        self.activity_bar_item_id(self.activity_bar_selected)
    }

    /// Extension panel names in the order they are painted (sorted by name),
    /// matching [`Self::ext_activity_panels`].
    fn sorted_ext_panel_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.ext_panels.keys().cloned().collect();
        names.sort();
        names
    }

    /// Move the keyboard cursor one position down in the activity bar.
    pub fn activity_bar_move_down(&mut self) {
        self.activity_bar_step(true);
    }

    /// Move the keyboard cursor one position up in the activity bar.
    pub fn activity_bar_move_up(&mut self) {
        self.activity_bar_step(false);
    }

    /// Step the activity-bar keyboard cursor by delegating to
    /// [`quadraui::AppShell`]'s `activity_select_next`/`activity_select_prev`
    /// (quadraui#386) over [`Self::activity_nav_shell`].
    ///
    /// Translates the stored `activity_bar_selected` index into the shell's
    /// painted-order cursor, steps, and translates the resulting *item id*
    /// back.
    ///
    /// A selection that names no item — a stale extension index left behind
    /// when `:PluginReload` dropped a panel — is clamped to the last item
    /// before stepping, mirroring what `AppShell::remove_panel` does to its
    /// own cursor. Without that the cursor would be wedged: every subsequent
    /// `j`/`k` would find no id to start from and refuse to move.
    fn activity_bar_step(&mut self, forward: bool) {
        let mut shell = self.activity_nav_shell();
        let np = shell.panels().len();
        let total = np + shell.bottom_items().len();
        if total == 0 {
            return;
        }
        let cursor = self
            .activity_bar_selected_item_id()
            .and_then(|cur_id| {
                shell
                    .panels()
                    .iter()
                    .position(|p| p.id.as_str() == cur_id)
                    .or_else(|| {
                        shell
                            .bottom_items()
                            .iter()
                            .position(|p| p.id.as_str() == cur_id)
                            .map(|i| np + i)
                    })
            })
            .unwrap_or(total - 1);
        shell.activity_set_cursor(cursor);
        if forward {
            shell.activity_select_next();
        } else {
            shell.activity_select_prev();
        }
        let Some(next_id) = shell.activity_selected_id().map(|w| w.as_str().to_string()) else {
            return;
        };
        if let Some(idx) = self.activity_bar_idx_for_item_id(&next_id) {
            self.activity_bar_selected = idx;
        }
    }

    /// Activate the currently selected activity bar item (l/Enter).
    ///
    /// Clears `activity_bar_focused` and updates engine state to focus the
    /// chosen panel. The backend should inspect the returned
    /// `ActivityBarActivation` to perform any backend-specific follow-up
    /// (e.g. setting `sidebar.has_focus`, closing TUI menu).
    pub fn activity_bar_activate(&mut self) -> ActivityBarActivation {
        self.activity_bar_focused = false;
        // #536: dispatch on the item *id* rather than re-deriving `sel - 8`
        // here. `activity_bar_item_id` owns the one index↔id table, so an
        // out-of-range selection (stale extension index) yields `None` → NoOp,
        // exactly as the old `ext_idx < ext_names.len()` guard did.
        let Some(id) = self.activity_bar_selected_item_id() else {
            return ActivityBarActivation::NoOp;
        };

        if id == HAMBURGER_PANEL_ID {
            self.toggle_menu_bar();
            return ActivityBarActivation::MenuToggled;
        }

        if let Some(name) = ext_panel_name_from_id(&id) {
            let name = name.to_string();
            if !self.app_shell.sidebar_visible() {
                self.app_shell.show_panel(&quadraui::WidgetId::new(&name));
                self.session.explorer_visible = true;
                let _ = self.session.save();
            }
            self.ext_panel_active = Some(name.clone());
            self.ext_panel_has_focus = true;
            self.ext_panel_selected = 0;
            self.plugin_event("panel_focus", &name);
            return ActivityBarActivation::ExtPanelFocused(name);
        }

        // A built-in panel: one of `FIXED_ACTIVITY_PANEL_IDS`, or Settings.
        self.ext_panel_has_focus = false;
        self.ext_panel_active = None;
        self.focus_sidebar_panel(&id);
        ActivityBarActivation::PanelFocused
    }
}
