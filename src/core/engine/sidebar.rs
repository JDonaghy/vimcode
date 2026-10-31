use super::Engine;

impl Engine {
    /// Check if a specific panel is the active sidebar panel.
    pub fn active_panel_is(&self, panel_id: &str) -> bool {
        self.app_shell
            .active_panel_id()
            .is_some_and(|id| id.as_str() == panel_id)
    }

    /// The sidebar-visibility verdict `autohide_panels` /
    /// `session.explorer_visible` / `explorer_visible_on_startup` derive —
    /// the single formula both [`Self::sync_app_shell_sidebar_visibility`]
    /// and `Engine::new_from_state`'s construction-time bake-in apply.
    /// Pulled out on its own so there is exactly one place that formula
    /// lives — see [`Self::sync_app_shell_sidebar_visibility`]'s
    /// doc for why a *second*, independent copy of it went stale.
    ///
    /// Always `false` in the no-workspace state
    /// ([`Engine::has_no_workspace`]): there is no workspace tree to show,
    /// regardless of what `autohide_panels`/`explorer_visible_on_startup`
    /// otherwise say.
    pub fn derived_sidebar_visible(&self) -> bool {
        if self.has_no_workspace() || self.settings.autohide_panels {
            false
        } else {
            self.session.explorer_visible || self.settings.explorer_visible_on_startup
        }
    }

    /// Make `self.app_shell`'s sidebar visibility agree with
    /// [`Self::derived_sidebar_visible`], regardless of which direction it
    /// currently disagrees in.
    ///
    /// `self.app_shell` is a fully independent [`quadraui::AppShell`]
    /// instance from the one the TUI runner actually paints through. It only
    /// gets to the right answer at construction time (`Engine::new_from_state`
    /// derives it there, once, before anything else can touch it) — a
    /// caller that mutates `self.session` on an already-built `Engine` (as
    /// `Engine::new_for_test`'s own doc warns against) leaves it behind.
    /// That staleness is not just cosmetic: the TUI shell's `handle`'s own
    /// resync pushes *this* shadow's `sidebar_visible()` onto the runner
    /// every dispatch, so a stale shadow doesn't just mis-answer a query —
    /// it actively hides a sidebar the runner had correctly showing.
    ///
    /// Called during construction (`Engine::new_from_state`) and when
    /// picking up mutations made on the engine after construction but before
    /// the shell app takes ownership of it.
    pub fn sync_app_shell_sidebar_visibility(&mut self) {
        let show_sidebar = self.derived_sidebar_visible();
        if show_sidebar != self.app_shell.sidebar_visible() {
            if show_sidebar {
                self.app_shell.toggle_sidebar();
            } else {
                self.app_shell.hide_sidebar();
            }
        }
    }
}

pub const PANEL_EXPLORER: &str = "panel:explorer";
pub const PANEL_SEARCH: &str = "panel:search";
pub const PANEL_DEBUG: &str = "panel:debug";
pub const PANEL_GIT: &str = "panel:git";
pub const PANEL_EXTENSIONS: &str = "panel:extensions";
pub const PANEL_AI: &str = "panel:ai";
/// The Board panel — a generic host for `quadraui::Board`, sourced
/// from whatever provider an extension declares. Named after the
/// *component*, not any one provider — see `crate::core::tool_client`'s
/// module doc for the placement rule this satisfies.
pub const PANEL_BOARD: &str = "panel:board";
/// VS Code pins two items at the bottom of the activity bar —
/// Accounts (sign-in identity) and Manage (this `Settings` gear). vimcode
/// has no account/sign-in concept anywhere in the engine (no OAuth, no
/// stored identity, no "signed in as" state) to back an Accounts item with,
/// so deliberately adding none rather than inventing a UI element with
/// nothing behind it — see that issue for the decision record. Revisit if
/// vimcode ever grows a real account concept (e.g. a hosted-sync or
/// extension-marketplace login).
pub const PANEL_SETTINGS: &str = "bottom:settings";

/// Activity-bar item id for the hamburger (menu) slot — keyboard index 0.
///
/// This slot is the one activity-bar id shared across both the fixed panel
/// id space and the [`quadraui::AppShell`] instance's own id space.
/// `render::build_activity_bar` mints this id for its hamburger
/// `ActivityItem`, and the engine's shadow `AppShell` registers it as a
/// panel.
///
/// GTK paints no hamburger (`include_hamburger = false`), but the slot still
/// exists in the keyboard sequence — navigation keys land on it and
/// toggle the menu bar.
pub const HAMBURGER_PANEL_ID: &str = "activity:menu";

/// Panel-id prefix for plugin-provided ("extension") sidebar panels — e.g. the
/// `git-insights` extension's panel is `"ext:git-insights"`.
///
/// This is the id both backends already synthesise by hand for extension
/// panels (GTK's `current_active_panel_id`/`App::switch_panel`), promoted to a
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
/// non-dynamic-extension) activity-bar panel order. `render::build_activity_bar`
/// and the engine's shadow `AppShell` construction both iterate this rather
/// than hand-transcribing the order twice — a reordering here is now a
/// one-line change both call sites pick up.
///
/// Order matches VS Code's activity bar top-to-bottom (Explorer,
/// Search, Source Control, Run and Debug, Extensions) — [`PANEL_GIT`]
/// before [`PANEL_DEBUG`].
pub const FIXED_ACTIVITY_PANEL_IDS: [&str; 7] = [
    PANEL_EXPLORER,
    PANEL_SEARCH,
    PANEL_GIT,
    PANEL_DEBUG,
    PANEL_EXTENSIONS,
    PANEL_AI,
    PANEL_BOARD,
];

/// Title/tooltip pair the engine's own "shadow" `AppShell` (see
/// [`Engine::sync_app_shell_sidebar_visibility`]'s doc for what that means)
/// shows for a built-in panel id — one entry per id in
/// [`FIXED_ACTIVITY_PANEL_IDS`] plus [`PANEL_SETTINGS`].
///
/// This is display metadata only; it has no bearing on activity-bar
/// keyboard order (that's [`FIXED_ACTIVITY_PANEL_IDS`] itself) or icon
/// resolution (`App::resolve_builtin_panel_icon`). Panics on any other id —
/// callers only ever feed it ids drawn from those two sources.
fn fixed_panel_title_tooltip(id: &str) -> (&'static str, &'static str) {
    match id {
        PANEL_EXPLORER => ("EXPLORER", "Explorer"),
        PANEL_SEARCH => ("SEARCH", "Search"),
        PANEL_DEBUG => ("RUN AND DEBUG", "Run and Debug"),
        PANEL_GIT => ("SOURCE CONTROL", "Source Control"),
        PANEL_EXTENSIONS => ("EXTENSIONS", "Extensions"),
        PANEL_AI => ("AI", "AI"),
        PANEL_BOARD => ("BOARD", "Board"),
        PANEL_SETTINGS => ("SETTINGS", "Settings"),
        _ => unreachable!("fixed_panel_title_tooltip called with a non-fixed panel id: {id:?}"),
    }
}

/// The engine's shadow `AppShell`'s panel list: [`FIXED_ACTIVITY_PANEL_IDS`]
/// in order, then [`PANEL_SETTINGS`] last, each with an empty icon (the
/// engine is backend-agnostic — see `App::shell_config`'s doc for who fills
/// icons in) and title/tooltip from [`fixed_panel_title_tooltip`].
///
/// GUI backends read the shadow shell's order straight through
/// `App::shell_config` (`self.engine.app_shell.panels()`), so this list
/// is the single order both backends' `ShellConfig`s ultimately trace
/// back to.
pub fn engine_app_shell_panel_definitions() -> Vec<quadraui::PanelDefinition> {
    FIXED_ACTIVITY_PANEL_IDS
        .into_iter()
        .chain(std::iter::once(PANEL_SETTINGS))
        .map(|id| {
            let (title, tooltip) = fixed_panel_title_tooltip(id);
            quadraui::PanelDefinition {
                id: quadraui::WidgetId::new(id),
                icon: String::new(),
                tooltip: tooltip.to_string(),
                title: title.to_string(),
            }
        })
        .collect()
}

/// Keyboard (toolbar) index of the bottom-pinned Settings item.
///
/// The activity bar's *painted* order is hamburger, the fixed panels, the
/// dynamic extension panels, then Settings pinned to the bottom — but the
/// legacy `activity_bar_selected` index space numbers Settings **before**
/// the extension panels, a mismatch that predates the `AppShell` migration.
/// Since the migration, stepping is done by `AppShell`'s cursor over the
/// *painted* order and these constants are a lookup table rather than a
/// sequencing rule.
pub const TOOLBAR_IDX_SETTINGS: u16 = FIXED_ACTIVITY_PANEL_IDS.len() as u16 + 1;

/// First keyboard (toolbar) index occupied by a dynamic extension panel.
/// See [`TOOLBAR_IDX_SETTINGS`] for why extension panels sit *after* Settings
/// in the index space while painting *before* it.
pub const TOOLBAR_IDX_EXT_BASE: u16 = TOOLBAR_IDX_SETTINGS + 1;

impl Engine {
    /// Activity-bar [`quadraui::PanelDefinition`]s for every plugin-registered
    /// extension panel, sorted by name.
    ///
    /// Both backends' `ShellConfig` builders append this to their static
    /// panel list, and the TUI additionally re-syncs it into the live
    /// `AppShell` after every dispatch so a plugin that registers a panel
    /// *after* startup still gets an icon.
    ///
    /// Sorted by name to match the order both the legacy rendering path and
    /// keyboard navigation use, so keyboard index and painted position agree.
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
            // Keyboard navigation into a sidebar panel takes priority over
            // an already-focused terminal: `terminal_has_focus` staying true
            // alongside the new panel's own flag would let
            // `route_focus_key`'s terminal gate outrank every sidebar arm,
            // e.g. routing Enter on the settings color-scheme picker to the
            // terminal PTY instead of activating the selection. Scoped to
            // this focus-*grant* branch only, not the hide branch above or
            // the shared `clear_sidebar_focus`, so collapsing/hiding a panel
            // while a terminal is genuinely focused leaves that focus alone.
            self.terminal_has_focus = false;
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
        // See the matching comment in `toggle_sidebar_panel`'s grant branch:
        // this is a focus-*grant* site, so it takes focus away from a
        // terminal explicitly rather than via the shared
        // `clear_sidebar_focus` primitive.
        self.terminal_has_focus = false;
        self.set_panel_focus(panel_id);
        self.session.explorer_visible = true;
        let _ = self.session.save();
    }

    /// Map a panel ID to the correct engine focus flag.
    fn set_panel_focus(&mut self, panel_id: &str) {
        match panel_id {
            PANEL_EXPLORER => {
                self.explorer_has_focus = true;
                // Bringing the Explorer to the front re-syncs it with the
                // active editor, the Explorer's equivalent of the data
                // refresh the Git/Extensions/Board arms below do:
                // `explorer_expanded` is in-memory only, so a tree the user
                // collapsed (or never expanded this session) would otherwise
                // show a bare root with the active file hidden and nothing
                // selected. A no-op when `explorer_auto_reveal` is off.
                self.explorer_reveal_active_file();
            }
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
            PANEL_AI => {
                self.ai_has_focus = true;
                // Not every caller of `set_panel_focus` routes
                // through `clear_sidebar_focus` first (`handle_nav_overflow`
                // doesn't), so reset the `<leader>ai` gesture buffer here
                // too rather than relying on that ordering.
                self.ai_leader_toggle_pending.clear();
            }
            PANEL_BOARD => {
                self.board_has_focus = true;
                if self.board_model.is_none() && !self.board_fetching {
                    self.board_refresh();
                }
            }
            PANEL_SETTINGS => {
                self.settings_has_focus = true;
                self.settings_modal_open = true;
            }
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
        self.app_shell.set_activity_keyboard_focused(true);
        self.sync_activity_cursor_to_app_shell();
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
        self.app_shell.set_activity_keyboard_focused(false);
    }

    /// Sync the engine's toolbar index to the real `AppShell` cursor so the
    /// TUI path (which reads `AppShell::build_activity_bar`) highlights the
    /// correct row.
    fn sync_activity_cursor_to_app_shell(&mut self) {
        let Some(id) = self.activity_bar_selected_item_id() else {
            return;
        };
        let np = self.app_shell.panels().len();
        let cursor = self
            .app_shell
            .panels()
            .iter()
            .position(|p| p.id.as_str() == id)
            .or_else(|| {
                self.app_shell
                    .bottom_items()
                    .iter()
                    .position(|p| p.id.as_str() == id)
                    .map(|i| np + i)
            });
        if let Some(c) = cursor {
            self.app_shell.activity_set_cursor(c);
        }
    }

    /// The activity bar's items **in painted order**, as a throwaway
    /// [`quadraui::AppShell`] whose keyboard cursor does the stepping for
    /// [`Self::activity_bar_move_down`] / [`Self::activity_bar_move_up`].
    ///
    /// Panel list and bottom-item list mirror the TUI's live `ShellConfig`:
    /// hamburger, the fixed panels, the dynamic extension panels sorted by
    /// name, then Settings pinned to the bottom. `AppShell`'s cursor spans
    /// `panels` then `bottom_items` as one sequence and saturates at both ends,
    /// which is vimcode's ordering — no vimcode-side arithmetic is needed.
    ///
    /// Built on demand rather than cached: the ext-panel list is derived from
    /// `self.ext_panels`, which a plugin can mutate at any point, so deriving
    /// it per keypress keeps the nav sequence from going stale.
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
    /// item's numeric toolbar index at the paint site.
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
    /// over [`Self::activity_nav_shell`].
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
            self.sync_activity_cursor_to_app_shell();
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
        self.app_shell.set_activity_keyboard_focused(false);
        // Dispatch on the item *id* rather than re-deriving `sel - 8`
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
            self.on_ext_panel_focused(&name);
            return ActivityBarActivation::ExtPanelFocused(name);
        }

        // A built-in panel: one of `FIXED_ACTIVITY_PANEL_IDS`, or Settings.
        self.ext_panel_has_focus = false;
        self.ext_panel_active = None;
        self.focus_sidebar_panel(&id);
        ActivityBarActivation::PanelFocused
    }
}
