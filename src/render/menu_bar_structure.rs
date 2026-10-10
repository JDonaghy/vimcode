use super::*;

// ─── Menu bar / debug toolbar ─────────────────────────────────────────────────

/// One item in a menu dropdown.
#[derive(Debug, Clone)]
pub struct MenuItemData {
    /// Display label shown in the dropdown (e.g. "Save").
    pub label: &'static str,
    /// Right-aligned keyboard shortcut hint in Vim mode (e.g. "u" for Undo).
    pub shortcut: &'static str,
    /// Right-aligned keyboard shortcut hint in VSCode mode (e.g. "Ctrl+Z" for Undo).
    /// Empty string means fall back to `shortcut`.
    pub vscode_shortcut: &'static str,
    /// Command string dispatched to the engine when activated (e.g. "w").
    /// Empty string means no action (for separators).
    pub action: &'static str,
    /// Whether this item is currently enabled.
    pub enabled: bool,
    /// If true, render as a horizontal divider line instead of a regular item.
    pub separator: bool,
}

/// One button in the debug toolbar strip.
#[derive(Debug, Clone)]
pub struct DebugButton {
    /// Nerd Font glyph string.
    pub icon: &'static str,
    /// Short label shown next to the icon.
    pub label: &'static str,
    /// Key hint shown in the button (e.g. "F5").
    pub key_hint: &'static str,
    /// Command string passed to `engine.execute_command()` when the button is clicked.
    pub action: &'static str,
    /// Whether this button is currently clickable.
    pub enabled: bool,
}

/// Data for the debug toolbar strip.
#[derive(Debug)]
pub struct DebugToolbarData {
    /// Buttons to render (in order, with a `│` separator after index 3).
    pub buttons: Vec<DebugButton>,
    /// True when a DAP session is active; drives future enabled/greyed-out state.
    pub session_active: bool,
}

/// Build the debug action-button toolbar as a `quadraui::Toolbar` (#510).
/// Button ids come from [`crate::core::engine::DEBUG_BUTTON_IDS`] so
/// click dispatch can map the hit-test result back to a button index and
/// action string. A `ToolbarButton::Separator` is inserted between the
/// Restart (index 3) and Step Over (index 4) buttons.
///
/// `enabled` state follows the per-button DAP rules:
/// - Continue / Step Over / Step Into / Step Out: `dap_session_active && dap_stopped_thread.is_some()`
/// - Pause: `dap_session_active && dap_stopped_thread.is_none()`
/// - Stop / Restart: `dap_session_active`
///
/// Both backends call this and hand the result to `Backend::draw_toolbar`.
pub fn debug_toolbar(engine: &Engine) -> quadraui::Toolbar {
    use crate::core::engine::DEBUG_BUTTON_IDS;
    use crate::icons;
    use quadraui::{Toolbar, ToolbarButton, WidgetId};

    let session = engine.dap_session_active;
    let stopped = engine.dap_stopped_thread.is_some();

    let action = |idx: usize, label: &str, icon: &str, key_hint: Option<&str>, enabled: bool| {
        ToolbarButton::Action {
            id: WidgetId::new(DEBUG_BUTTON_IDS[idx]),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            key_hint: key_hint.map(|s| s.to_string()),
            enabled,
            is_active: false,
            tooltip: String::new(),
        }
    };

    Toolbar::new(WidgetId::new("debug:toolbar")).with_buttons(vec![
        // 0: Continue — enabled when session active and stopped
        action(
            0,
            "Continue",
            icons::DBG_CONTINUE.fallback,
            Some("F5"),
            session && stopped,
        ),
        // 1: Pause — enabled when session active and running (not stopped)
        action(
            1,
            "Pause",
            icons::DBG_PAUSE.fallback,
            Some("F6"),
            session && !stopped,
        ),
        // 2: Stop — enabled when session active
        action(2, "Stop", icons::DBG_STOP.fallback, Some("⇧F5"), session),
        // 3: Restart — enabled when session active
        action(
            3,
            "Restart",
            icons::DBG_RESTART.fallback,
            Some("^⇧F5"),
            session,
        ),
        // Separator between restart and step controls
        ToolbarButton::Separator,
        // 4: Step Over — enabled when session active and stopped
        action(
            4,
            "Step Over",
            icons::DBG_STEP_OVER.fallback,
            Some("F10"),
            session && stopped,
        ),
        // 5: Step Into — enabled when session active and stopped
        action(
            5,
            "Step Into",
            icons::DBG_RESTART.fallback,
            Some("F11"),
            session && stopped,
        ),
        // 6: Step Out — enabled when session active and stopped
        action(
            6,
            "Step Out",
            icons::DBG_STEP_OUT.fallback,
            Some("⇧F11"),
            session && stopped,
        ),
    ])
}

/// Draw the debug action-button toolbar through backend `b` and cache its
/// layout on `engine` for click/hover dispatch (#510). Both backends call
/// this inside their frame scope; the only per-backend input is `rect`
/// (cell units for TUI, pixels for GTK). Mouse hover → `debug_button_hovered`,
/// visual press → `debug_button_pressed`, both read from the engine.
pub fn draw_debug_toolbar(b: &mut dyn quadraui::Backend, engine: &Engine, rect: quadraui::Rect) {
    use crate::core::engine::Engine;
    let bar = debug_toolbar(engine);
    let hovered = engine
        .debug_button_hovered
        .and_then(Engine::debug_button_id);
    let pressed = engine
        .debug_button_pressed
        .and_then(Engine::debug_button_id);
    let interaction = quadraui::InteractionState::from_parts(hovered, pressed);
    let layout = b.draw_toolbar_interactive(rect, &bar, &interaction);
    engine.debug_toolbar_layout.replace(Some(layout));
}

/// Build the debug sidebar's `SidebarPanelChrome::StatusBars` (quadraui#1061,
/// issue #1392) — row 0 = title ("DEBUG | config_name"), row 1 = the
/// Run/Stop/Continue action button. Both backends now paint this chrome
/// through `SidebarPanelBody::render_with` (`tui_main::panels::
/// render_debug_sidebar`, `App::paint_sidebar_panel_rung`'s `PANEL_DEBUG`
/// arm) instead of slicing two rows off `area` by hand and calling
/// `Backend::draw_status_bar` on each directly — `StatusBars` reserves one
/// row per bar and paints each through the same rasteriser the old code
/// called separately for each row, and surfaces its hit regions on
/// `SidebarPanelBodyLayout::
/// status_bar_hit_regions` in the same absolute space the caller's `rect`
/// was in — the caller stores that directly on `Engine::
/// dap_sidebar_action_hits` (see its doc) rather than re-deriving the
/// action row's rect as a second, independently-computed value the way
/// `cached_dap_action_rect` used to (paint and click could disagree).
pub fn debug_sidebar_chrome(sidebar: &DebugSidebarData, theme: &Theme) -> SidebarPanelChrome {
    let (title, action) = debug_sidebar_status_bars(sidebar, theme);
    SidebarPanelChrome::StatusBars(vec![title, action])
}

/// Build the debug sidebar's title/action-button `StatusBar` pair. Split out
/// of [`debug_sidebar_chrome`] only because constructing the two bars is
/// easier to read un-nested from the `Vec` wrapper; not called directly by
/// either backend any more (see that function's doc).
fn debug_sidebar_status_bars(
    sidebar: &DebugSidebarData,
    theme: &Theme,
) -> (quadraui::StatusBar, quadraui::StatusBar) {
    let bg = theme.status_bg;
    let fg = theme.status_fg;
    let green = theme.git_added;
    let red = theme.diagnostic_error;

    let cfg_name = sidebar.launch_config_name.as_deref().unwrap_or("no config");
    let title = quadraui::StatusBar {
        id: quadraui::WidgetId::new("debug_sidebar_title"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: format!("  {} DEBUG  |  {cfg_name}", icons::DEBUG.s()),
            fg,
            bg,
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    };

    let action_id = Some(quadraui::WidgetId::new("debug_sidebar:action"));
    let (icon, label, icon_fg) = if sidebar.session_active && sidebar.stopped {
        (icons::DBG_PLAY.s(), "  Continue", green)
    } else if sidebar.session_active {
        (icons::DBG_STOP_ALT.s(), "  Stop", red)
    } else {
        (icons::DBG_PLAY.s(), "  Start Debugging", green)
    };
    let action = quadraui::StatusBar {
        id: quadraui::WidgetId::new("debug_sidebar_action"),
        left_segments: vec![
            quadraui::StatusBarSegment {
                text: icon.to_string(),
                fg: icon_fg,
                bg,
                bold: false,
                action_id: action_id.clone(),
            },
            quadraui::StatusBarSegment {
                text: label.to_string(),
                fg,
                bg,
                bold: false,
                action_id,
            },
        ],
        right_segments: Vec::new(),
    };

    (title, action)
}

/// Build the Explorer header's view-actions toolbar row (#1693): New File,
/// New Folder, Refresh, Collapse All, and a "..." overflow menu — mirrors
/// VS Code's Explorer view-actions row. Returned as `SidebarPanelChrome::
/// StatusBars(vec![...])`'s single bar so `paint_sidebar_panel_rung`'s
/// `PANEL_EXPLORER` arm can thread it through the same composer the Debug
/// sidebar's title/action bars (`debug_sidebar_chrome`) and the SC panel's
/// own toolbar (`sc_sidebar_panel`) already use — no new paint/hit-test
/// mechanism, just a new row through an existing one.
///
/// Unlike `debug_sidebar_status_bars`'s title bar, this carries no title
/// text of its own: the shell's own sidebar header already titles this
/// panel "EXPLORER" (`sidebar.rs`'s `fixed_panel_title_tooltip`), and a
/// second title row here would be the exact double-header shape
/// `sc_sidebar_panel`'s doc warns against (#1256).
pub fn explorer_toolbar_status_bar(theme: &Theme) -> quadraui::StatusBar {
    let bg = theme.status_bg;
    let fg = theme.status_fg;
    let button = |idx: usize, icon: &icons::Icon| quadraui::StatusBarSegment {
        text: format!(" {} ", icon.s()),
        fg,
        bg,
        bold: false,
        action_id: Engine::explorer_toolbar_action_id(idx),
    };
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("explorer:toolbar"),
        left_segments: Vec::new(),
        right_segments: vec![
            button(0, &icons::EXPLORER_NEW_FILE),
            button(1, &icons::EXPLORER_NEW_FOLDER),
            button(2, &icons::EXPLORER_REFRESH),
            button(3, &icons::EXPLORER_COLLAPSE_ALL),
            button(4, &icons::EXPLORER_OVERFLOW),
        ],
    }
}

/// Resolve a press against the Explorer header's view-actions toolbar row
/// (#1693), given `pos` in the same absolute space `SidebarPanelBody::
/// render_with` painted the chrome into — the same contract
/// `dap_sidebar_action_click_at` follows for the Debug sidebar's own
/// chrome row (see that function's doc). `engine.explorer_toolbar_hits` is
/// populated straight from `SidebarPanelBodyLayout::status_bar_hit_regions`
/// at paint time, so there is no per-backend translation step.
///
/// Button 4 (the "..." overflow menu) is handled by the caller instead of
/// dispatched through `Engine::explorer_activate_toolbar_action` — opening
/// the popup needs the click's own cell-space position and the toolbar
/// row's trigger height, neither of which this engine-only function has
/// (`App::try_route_sidebar_mouse_event`'s Explorer arm has both). Returns
/// the matched button index, or `None` when `pos` doesn't land on a toolbar
/// segment at all (the caller falls through to the tree's own click
/// routing in that case).
pub fn explorer_toolbar_hit_at(engine: &Engine, pos: quadraui::Point) -> Option<usize> {
    let hits = engine.explorer_toolbar_hits.borrow();
    hits.iter().find_map(|(rect, hit)| {
        if !rect.contains(pos) {
            return None;
        }
        match hit {
            quadraui::StatusBarHit::Segment(id) => Engine::explorer_toolbar_action_index(id),
            _ => None,
        }
    })
}

/// `action_id` for each inline window-control button drawn by
/// [`window_controls_status_bar`]. Shared with the GTK click handler so the
/// two sides can't drift.
pub const WINDOW_MINIMIZE_ACTION: &str = "window:minimize";
pub const WINDOW_MAXIMIZE_ACTION: &str = "window:maximize";
pub const WINDOW_CLOSE_ACTION: &str = "window:close";

/// Width, in pixels, of the outer-window resize grip passed to
/// [`quadraui::AppShell::window_edge`] (#1528).
///
/// The grip used to be `backend.line_height()` — 16-22px, the full height of
/// the title bar and command-line rows. That made the grip band as tall as
/// those rows, so it always ran *underneath* them: the top-row title-bar drag
/// check and the bottom-row command-line click always resolved first, and
/// North/South resize was dead everywhere except the corners the title bar
/// or command line didn't cover. The same oversized band on the East edge
/// also swallowed the vertical scrollbar and the minimap's rightmost column
/// with a single editor group, where both sit within that many pixels of the
/// window's true right edge.
///
/// A few pixels — VS Code/Electron frameless windows use a comparable
/// margin — is still comfortably grabbable with a mouse, is thinner than one
/// scrollbar/minimap gutter (`app_support::scrollbar_thumb_geometry`'s
/// track, `render::minimap`'s gutter offset), and is thin enough to sit
/// *inside* the title bar and command-line rows instead of spanning them, so
/// the edge grip can win only in that sliver and fall through to the
/// row/scrollbar/minimap hit-test everywhere else in the row.
///
/// Caveat (#1528 acceptance criteria, stated explicitly so a reader of
/// `window_resize_grip_stays_inside_the_scrollbar_gutter` below doesn't read
/// it as proving *zero* overlap): the acceptance bar is "a press in the
/// outermost ~4 px still resizes (E)", i.e. some overlap with the
/// scrollbar/minimap gutter's own outermost pixels is expected and accepted
/// — the fix's job is shrinking that overlap from a full row-height band down
/// to this margin, not eliminating it.
pub const WINDOW_RESIZE_GRIP_PX: f32 = 4.0;

/// Does an outer-window resize press at `edge` land inside the band the
/// change-review surface's own first row occupies (#1528 review of #955)?
///
/// The change-review surface is genuinely full-viewport, and its first diff
/// row paints underneath the (visually hidden but still logically live) CSD
/// title bar — i.e. it only ever overlaps the *top* of the window, never the
/// bottom/left/right edges or the corners that don't touch North. So a press
/// in the resize grip should still be allowed to arm a resize everywhere
/// *except* where it would otherwise be reinterpreted as a click on that
/// first diff row: `North`, and the two corners that include it
/// (`NorthEast`/`NorthWest`).
///
/// Extracted to a pure function (rather than left as an inline `matches!` at
/// the one call site in `App::handle`) so the *decision* — which edges a
/// change-review surface may steal from resize — is unit-testable on its
/// own, independent of `quadraui::gtk::testing::GtkDriver`'s inability to
/// arm a real resize headlessly (no `gtk4::Window`, no captured GDK press —
/// see `src/gtk/testing.rs`'s module doc "No window").
pub fn resize_edge_overlaps_change_review_band(edge: quadraui::ResizeEdge) -> bool {
    matches!(
        edge,
        quadraui::ResizeEdge::North
            | quadraui::ResizeEdge::NorthEast
            | quadraui::ResizeEdge::NorthWest
    )
}

/// Build the inline minimize/maximize/close window-control buttons for the
/// GTK client-side titlebar (#552).
///
/// quadraui's `run_with_shell` GTK runner creates an undecorated-chrome-free
/// window with no native titlebar hosting (single-DA architecture, #217) —
/// GTK draws its own CSD-style controls at the right edge of the menu-bar
/// row using the same `StatusBar` primitive already used for the debug
/// sidebar's action row, so the click hit-testing reuses the existing
/// `StatusBarHit::Segment` mechanism rather than any new backend API.
///
/// TUI has no window-chrome equivalent (a terminal has no window to
/// minimize/maximize) — this is GTK-only, called from `src/gtk/mod.rs`.
pub fn window_controls_status_bar(theme: &Theme, maximized: bool) -> quadraui::StatusBar {
    let bg = theme.tab_bar_bg;
    // `tab_inactive_fg` — NOT `status_fg` — pairs with `tab_bar_bg` by theme
    // design (it's what `draw_menu_bar` already uses for the File/Edit/...
    // labels painted immediately to the left, against this exact
    // background). `status_fg` is paired with `status_bg` (the bottom
    // status line's own background) instead; at least one shipped theme
    // (`vs_light`: `tab_bar_bg` #ececec, `status_fg` #ffffff) renders
    // near-invisible white-on-near-white glyphs with that mismatched
    // pairing — a real, reproducible cause of the #552 round-2 "buttons
    // render with zero visible pixels" report.
    let fg = theme.tab_inactive_fg;
    let maximize_icon = if maximized {
        icons::WINDOW_RESTORE.s()
    } else {
        icons::WINDOW_MAXIMIZE.s()
    };
    let seg = |text: String, action: &str| quadraui::StatusBarSegment {
        text,
        fg,
        bg,
        bold: false,
        action_id: Some(quadraui::WidgetId::new(action)),
    };
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("window_controls"),
        left_segments: Vec::new(),
        right_segments: vec![
            seg(
                format!("  {}  ", icons::WINDOW_MINIMIZE.s()),
                WINDOW_MINIMIZE_ACTION,
            ),
            seg(format!("  {maximize_icon}  "), WINDOW_MAXIMIZE_ACTION),
            seg(
                format!("  {}  ", icons::WINDOW_CLOSE.s()),
                WINDOW_CLOSE_ACTION,
            ),
        ],
    }
}

/// Build a `TextDisplay` for the debug output panel.
pub fn debug_output_to_text_display(
    output_lines: &[String],
    scroll_offset: usize,
    auto_scroll: bool,
) -> quadraui::TextDisplay {
    let lines: Vec<quadraui::TextDisplayLine> = output_lines
        .iter()
        .map(|line| quadraui::TextDisplayLine {
            spans: vec![quadraui::StyledSpan {
                text: format!("  {line}"),
                fg: None,
                bg: None,
                bold: false,
                italic: false,
                underline: false,
            }],
            decoration: quadraui::Decoration::Normal,
            timestamp: None,
        })
        .collect();

    quadraui::TextDisplay {
        id: quadraui::WidgetId::new("debug_output"),
        lines,
        scroll_offset,
        auto_scroll,
        max_lines: 0,
        has_focus: false,
        title: None,
        show_scrollbar: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Debug toolbar tests (#510) ──────────────────────────────────────────

    /// Helper: return an engine with DAP session state as specified.
    fn engine_with_dap(session_active: bool, stopped_thread: Option<u64>) -> Engine {
        let mut e = Engine::new();
        e.debug_toolbar_visible = true;
        e.dap_session_active = session_active;
        e.dap_stopped_thread = stopped_thread;
        e
    }

    #[test]
    fn debug_toolbar_button_ids_round_trip() {
        use crate::core::engine::{Engine, DEBUG_BUTTON_IDS};
        use quadraui::ToolbarButton;

        let engine = engine_with_dap(true, Some(1u64));
        let bar = debug_toolbar(&engine);

        // 8 entries: 7 action buttons + 1 separator after index 3.
        assert_eq!(bar.buttons.len(), 8);

        let mut action_idx = 0usize;
        for btn in &bar.buttons {
            match btn {
                ToolbarButton::Action { id, .. } => {
                    // id matches DEBUG_BUTTON_IDS[action_idx]
                    assert_eq!(
                        id.as_str(),
                        DEBUG_BUTTON_IDS[action_idx],
                        "button {action_idx} id mismatch"
                    );
                    // round-trip: id → index → same action_idx
                    let idx = Engine::debug_button_index(id).expect("index for valid id");
                    assert_eq!(idx, action_idx);
                    action_idx += 1;
                }
                ToolbarButton::Separator => {
                    // separator sits between button 3 (Restart) and button 4 (Step Over)
                    assert_eq!(action_idx, 4, "separator must come after index 3");
                }
                ToolbarButton::Label { .. } => {
                    panic!("unexpected Label variant in debug toolbar");
                }
            }
        }
        assert_eq!(action_idx, 7, "expected 7 action buttons");
    }

    #[test]
    fn debug_toolbar_disabled_when_no_session() {
        use quadraui::ToolbarButton;

        let engine = engine_with_dap(false, None);
        let bar = debug_toolbar(&engine);
        for btn in &bar.buttons {
            if let ToolbarButton::Action { enabled, label, .. } = btn {
                assert!(
                    !enabled,
                    "button '{label}' should be disabled with no session"
                );
            }
        }
    }

    #[test]
    fn debug_toolbar_steps_disabled_while_running() {
        use quadraui::ToolbarButton;

        // Session active, not stopped (running).
        let engine = engine_with_dap(true, None);
        let bar = debug_toolbar(&engine);

        let get_enabled = |label: &str| {
            bar.buttons.iter().find_map(|b| {
                if let ToolbarButton::Action {
                    enabled, label: l, ..
                } = b
                {
                    if l == label {
                        return Some(*enabled);
                    }
                }
                None
            })
        };

        // Running → Continue/Step* disabled, Pause enabled, Stop/Restart enabled.
        assert_eq!(get_enabled("Continue"), Some(false));
        assert_eq!(get_enabled("Step Over"), Some(false));
        assert_eq!(get_enabled("Step Into"), Some(false));
        assert_eq!(get_enabled("Step Out"), Some(false));
        assert_eq!(get_enabled("Pause"), Some(true));
        assert_eq!(get_enabled("Stop"), Some(true));
        assert_eq!(get_enabled("Restart"), Some(true));
    }

    #[test]
    fn debug_toolbar_steps_enabled_when_stopped() {
        use quadraui::ToolbarButton;

        // Session active, stopped at thread 1.
        let engine = engine_with_dap(true, Some(1u64));
        let bar = debug_toolbar(&engine);

        let get_enabled = |label: &str| {
            bar.buttons.iter().find_map(|b| {
                if let ToolbarButton::Action {
                    enabled, label: l, ..
                } = b
                {
                    if l == label {
                        return Some(*enabled);
                    }
                }
                None
            })
        };

        // Stopped → Continue/Step* enabled, Pause disabled, Stop/Restart enabled.
        assert_eq!(get_enabled("Continue"), Some(true));
        assert_eq!(get_enabled("Step Over"), Some(true));
        assert_eq!(get_enabled("Step Into"), Some(true));
        assert_eq!(get_enabled("Step Out"), Some(true));
        assert_eq!(get_enabled("Pause"), Some(false));
        assert_eq!(get_enabled("Stop"), Some(true));
        assert_eq!(get_enabled("Restart"), Some(true));
    }

    #[test]
    fn debug_toolbar_hit_test_resolves_each_button() {
        use crate::core::engine::Engine;
        use quadraui::ToolbarHit;

        let engine = engine_with_dap(true, Some(1u64));
        let bar = debug_toolbar(&engine);
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);

        // Hit-test each visible_item that is clickable and assert that it
        // resolves back to its expected DEBUG_BUTTON_IDS entry.
        for item in &layout.visible_items {
            if !item.clickable {
                continue;
            }
            let hit = layout.hit_test(item.bounds.x + 0.5, item.bounds.y);
            match hit {
                ToolbarHit::Button(ref id) => {
                    let idx = Engine::debug_button_index(id)
                        .unwrap_or_else(|| panic!("unknown id {:?}", id.as_str()));
                    assert!(idx < 7, "index {idx} out of range");
                }
                ToolbarHit::Empty => {
                    panic!(
                        "clickable item hit_test returned Empty at {:?}",
                        item.bounds
                    );
                }
            }
        }
    }

    #[test]
    fn debug_toolbar_disabled_button_not_clickable() {
        use quadraui::ToolbarHit;

        // No session → all buttons disabled.
        let engine = engine_with_dap(false, None);
        let bar = debug_toolbar(&engine);
        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let layout = quadraui::tui::tui_toolbar_layout(&bar, area);

        // Every visible_item must be not clickable and hit_test must return Empty.
        for item in &layout.visible_items {
            assert!(
                !item.clickable,
                "disabled button at {:?} should not be clickable",
                item.bounds
            );
            assert_eq!(
                layout.hit_test(item.bounds.x + 0.5, item.bounds.y),
                ToolbarHit::Empty,
                "disabled button hit_test should return Empty"
            );
        }
    }

    #[test]
    fn test_ext_panel_to_tree_view_shape() {
        use crate::core::plugin::{ExtPanelAction, ExtPanelBadge, ExtPanelItem, ExtPanelStyle};
        use quadraui::Decoration;

        let mut item_a = ExtPanelItem {
            text: "Item A".into(),
            id: "a".into(),
            indent: 0,
            style: ExtPanelStyle::Normal,
            expandable: true,
            expanded: true,
            badges: vec![ExtPanelBadge {
                text: "main".into(),
                color: "green".into(),
            }],
            actions: vec![ExtPanelAction {
                label: "Stage".into(),
                key: "s".into(),
            }],
            hint: "h".into(),
            ..Default::default()
        };
        item_a.icon = crate::icons::DBG_PLAY.nerd.into();

        let item_b_child = ExtPanelItem {
            text: "Child".into(),
            id: "a_child".into(),
            indent: 1,
            parent_id: "a".into(),
            style: ExtPanelStyle::Accent,
            ..Default::default()
        };

        let item_c_dim = ExtPanelItem {
            text: "Dim".into(),
            id: "c".into(),
            style: ExtPanelStyle::Dim,
            ..Default::default()
        };

        let item_sep = ExtPanelItem {
            is_separator: true,
            ..Default::default()
        };

        let panel = ExtPanelData {
            name: "my_ext".into(),
            title: "MY EXT".into(),
            sections: vec![
                ExtPanelSectionData {
                    name: "Open".into(),
                    items: vec![item_a, item_b_child, item_sep, item_c_dim],
                    expanded: true,
                },
                ExtPanelSectionData {
                    name: "Closed".into(),
                    items: vec![ExtPanelItem {
                        text: "Hidden".into(),
                        ..Default::default()
                    }],
                    expanded: false,
                },
            ],
            // Select the second visible item (`Child`, flat idx 2: header=0, item_a=1, child=2).
            selected: 2,
            has_focus: true,
            scroll_top: 0,
            input_text: String::new(),
            input_active: false,
            help_open: false,
            help_bindings: vec![],
        };

        let theme = Theme::onedark();
        let tv = ext_panel_to_tree_view(&panel, &theme);

        // Expect rows: [0]=Open header, [0,0]=Item A, [0,1]=Child, [0,2]=separator,
        // [0,3]=Dim, [1]=Closed header (collapsed → no children).
        assert_eq!(tv.rows.len(), 6, "rows: {:?}", tv.rows.len());
        assert_eq!(tv.rows[0].path, vec![0]);
        assert_eq!(tv.rows[0].decoration, Decoration::Header);
        assert_eq!(tv.rows[0].is_expanded, Some(true));
        assert_eq!(tv.rows[1].path, vec![0, 0]);
        assert_eq!(tv.rows[1].indent, 1);
        assert_eq!(tv.rows[1].is_expanded, Some(true)); // expandable item
        assert!(
            tv.rows[1].badge.is_some(),
            "badges + action + hint combined"
        );
        assert!(tv.rows[1].icon.is_some(), "icon converted");
        assert_eq!(tv.rows[2].path, vec![0, 1]);
        assert_eq!(tv.rows[2].indent, 2); // indent 1 + 1
        assert_eq!(tv.rows[2].is_expanded, None); // not expandable
                                                  // Separator is muted line glyph.
        assert_eq!(tv.rows[3].decoration, Decoration::Muted);
        assert_eq!(tv.rows[3].text.spans[0].text, "\u{2500}");
        // Dim item maps to Muted.
        assert_eq!(tv.rows[4].decoration, Decoration::Muted);
        // Collapsed section: header only, no children.
        assert_eq!(tv.rows[5].path, vec![1]);
        assert_eq!(tv.rows[5].is_expanded, Some(false));

        // Selection: flat idx 2 = Child → path [0, 1].
        assert_eq!(tv.selected_path, Some(vec![0, 1]));
        assert_eq!(tv.scroll_offset, 0);
        assert!(tv.has_focus);
    }

    #[test]
    fn test_ext_panel_to_tree_view_no_focus_no_selection() {
        let panel = ExtPanelData {
            name: "x".into(),
            title: "X".into(),
            sections: vec![ExtPanelSectionData {
                name: "S".into(),
                items: vec![],
                expanded: true,
            }],
            selected: 0,
            has_focus: false,
            scroll_top: 5,
            input_text: String::new(),
            input_active: false,
            help_open: false,
            help_bindings: vec![],
        };
        let tv = ext_panel_to_tree_view(&panel, &Theme::onedark());
        assert_eq!(tv.selected_path, None);
        assert_eq!(tv.scroll_offset, 5);
        assert!(!tv.has_focus);
    }

    #[test]
    fn test_try_from_hex() {
        assert_eq!(try_from_hex("#ff0000"), Some(Color::from_rgb(255, 0, 0)));
        assert_eq!(try_from_hex("00ff00"), Some(Color::from_rgb(0, 255, 0)));
        assert_eq!(
            try_from_hex("#abc"),
            Some(Color::from_rgb(0xaa, 0xbb, 0xcc))
        );
        // 8-digit hex (alpha discarded)
        assert_eq!(try_from_hex("#ff000080"), Some(Color::from_rgb(255, 0, 0)));
        assert_eq!(try_from_hex("xyz"), None);
        assert_eq!(try_from_hex(""), None);
    }

    #[test]
    fn test_try_from_hex_non_ascii_returns_none_instead_of_panicking() {
        // #1494: "日本" is 6 *bytes* (two 3-byte UTF-8 chars) but not
        // ASCII, so it used to match the `6 | 8` byte-length arm and then
        // panic slicing `&s[0..2]` — byte offset 2 lands inside the first
        // character, not on a UTF-8 char boundary. Also cover an 8-byte
        // non-ASCII string, matching `try_from_hex_over`'s `8 =>` arm.
        assert_eq!(try_from_hex("日本"), None);
        assert_eq!(try_from_hex_over("日本é", Color::from_rgb(0, 0, 0)), None);
    }

    /// #1494 CI: the band-clamp that stops the Command Center's
    /// overflowing search box from eating the window-control buttons'
    /// clicks. Built from the real primitive (`CommandCenter::layout`)
    /// rather than a hand-written `CommandCenterLayout` literal, so the
    /// overflow this guards against is the one quadraui actually produces.
    #[test]
    fn command_center_hit_in_band_ignores_the_overflowing_search_box() {
        let cc = build_command_center_view(true, true, "vimcode");
        // 272px band — the width `measure_title_bar_bands` hands the
        // Command Center on an 800px window with a wide UI font. The
        // primitive's content floor is 2*24 arrows + 2*8 gaps + a 280px
        // search box = 344px, so 72px of it overflows to the right.
        let band = quadraui::Rect::new(367.0, 0.0, 272.0, 46.0);
        let measure =
            quadraui::CommandCenterMeasure::from_char_width(cc.search_label.as_str(), 8.0, 46.0);
        let layout = cc.layout(band, measure);

        let search = layout
            .search_bounds
            .expect("the search box must be laid out for this to test anything");
        assert!(
            search.x + search.width > band.x + band.width,
            "precondition: this band must be narrow enough that the search \
             box overflows it; search={search:?} band={band:?}"
        );

        // Inside the band, on the search box — still a SearchBox hit.
        let inside_x = band.x + band.width - 1.0;
        assert_eq!(
            command_center_hit_in_band(&layout, inside_x, 10.0),
            Some(quadraui::CommandCenterHit::SearchBox),
            "a click inside the band must still resolve normally"
        );
        assert_eq!(
            command_center_hit_in_band(&layout, band.x + 4.0, 10.0),
            Some(quadraui::CommandCenterHit::Back),
            "the nav arrows must keep working"
        );

        // Past the band's right edge — where the window-control buttons
        // live — the overflowing search box must NOT claim the click.
        // `hit_test` alone does, which is the bug.
        let overflow_x = band.x + band.width + 1.0;
        assert!(
            overflow_x < search.x + search.width,
            "precondition: this probe must land on the overflowing part of \
             the search box"
        );
        assert_eq!(
            layout.hit_test(overflow_x, 10.0),
            quadraui::CommandCenterHit::SearchBox,
            "precondition: the unclamped primitive really does claim this \
             out-of-band point — if this ever stops being true the clamp \
             is obsolete, not the test"
        );
        assert_eq!(
            command_center_hit_in_band(&layout, overflow_x, 10.0),
            None,
            "a point outside the painted band must resolve to nothing, so \
             the caller falls through to whatever owns that pixel"
        );

        // Outside vertically, too.
        assert_eq!(
            command_center_hit_in_band(&layout, band.x + 4.0, band.y + band.height + 1.0),
            None,
            "below the band is outside the band"
        );
    }

    #[test]
    fn test_strip_json_comments() {
        let input = r#"{
  // line comment
  "key": "value", /* block */
  "str": "has // no comment"
}"#;
        let stripped = quadraui::text_util::strip_json_comments(input);
        let val: serde_json::Value = serde_json::from_str(&stripped).unwrap();
        assert_eq!(val["key"], "value");
        assert_eq!(val["str"], "has // no comment");
    }

    #[test]
    fn test_lighten_darken() {
        let c = Color::from_rgb(100, 100, 100);
        let lighter = c.lighten(0.5);
        assert!(lighter.r > 100 && lighter.r < 255);
        let darker = c.darken(0.5);
        assert!(darker.r < 100 && darker.r > 0);
        // Extremes
        assert_eq!(c.lighten(1.0), Color::from_rgb(255, 255, 255));
        assert_eq!(c.darken(1.0), Color::from_rgb(0, 0, 0));
    }

    #[test]
    fn test_from_vscode_json() {
        let dir = crate::harness::scratch_dir("vimcode_test_theme");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-theme.json");
        std::fs::write(
            &path,
            r##"{
            // Test VSCode theme
            "name": "Test Theme",
            "colors": {
                "editor.background": "#1e1e2e",
                "editor.foreground": "#cdd6f4",
                "editorCursor.foreground": "#f5e0dc",
                "editor.selectionBackground": "#585b7066",
                "editorLineNumber.foreground": "#6c7086",
                "statusBar.background": "#181825",
                "statusBar.foreground": "#cdd6f4"
            },
            "tokenColors": [
                {
                    "scope": ["keyword", "keyword.control"],
                    "settings": { "foreground": "#cba6f7" }
                },
                {
                    "scope": "string",
                    "settings": { "foreground": "#a6e3a1" }
                },
                {
                    "scope": "comment",
                    "settings": { "foreground": "#6c7086" }
                }
            ]
        }"##,
        )
        .unwrap();

        let theme = Theme::from_vscode_json(&path).unwrap();
        assert_eq!(theme.background, try_from_hex("#1e1e2e").unwrap());
        assert_eq!(theme.foreground, try_from_hex("#cdd6f4").unwrap());
        assert_eq!(theme.cursor, try_from_hex("#f5e0dc").unwrap());
        assert_eq!(theme.keyword, try_from_hex("#cba6f7").unwrap());
        assert_eq!(theme.string_lit, try_from_hex("#a6e3a1").unwrap());
        assert_eq!(theme.comment, try_from_hex("#6c7086").unwrap());
        assert_eq!(theme.status_bg, try_from_hex("#181825").unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1547: `activityBar.activeBorder` is VS Code's own colour key for
    /// the active-item accent line (`activity_active_accent`'s doc). When
    /// an imported theme sets it explicitly to something other than
    /// `activityBar.foreground`, the explicit value must win — the two are
    /// independent colours, not one derived from the other.
    #[test]
    fn from_vscode_json_prefers_explicit_active_border_over_foreground() {
        let dir = crate::harness::scratch_dir("vimcode_test_1547_active_border");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-theme.json");
        std::fs::write(
            &path,
            r##"{
            "name": "Test Theme",
            "colors": {
                "activityBar.foreground": "#c8c8d2",
                "activityBar.activeBorder": "#61afef"
            }
        }"##,
        )
        .unwrap();

        let theme = Theme::from_vscode_json(&path).unwrap();
        assert_eq!(theme.activity_bar_fg, try_from_hex("#c8c8d2").unwrap());
        assert_eq!(
            theme.activity_active_accent,
            try_from_hex("#61afef").unwrap(),
            "an explicit `activityBar.activeBorder` must win over \
             `activityBar.foreground`"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1547: VS Code's own default for `activityBar.activeBorder` is
    /// `activityBar.foreground` (see `ACTIVITY_BAR_ACTIVE_BORDER`'s
    /// `dark`/`light` entries in VS Code's `colorRegistry`) — so an
    /// imported theme that sets only `activityBar.foreground` must have
    /// `activity_active_accent` fall back to that value, not stay at
    /// whatever built-in default `Theme::onedark()` seeded it with.
    #[test]
    fn from_vscode_json_falls_back_to_foreground_when_active_border_absent() {
        let dir = crate::harness::scratch_dir("vimcode_test_1547_active_border_fallback");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-theme.json");
        std::fs::write(
            &path,
            r##"{
            "name": "Test Theme",
            "colors": {
                "activityBar.foreground": "#93a1a1"
            }
        }"##,
        )
        .unwrap();

        let theme = Theme::from_vscode_json(&path).unwrap();
        assert_eq!(theme.activity_bar_fg, try_from_hex("#93a1a1").unwrap());
        assert_eq!(
            theme.activity_active_accent,
            try_from_hex("#93a1a1").unwrap(),
            "with no explicit `activityBar.activeBorder`, `activity_active_accent` \
             must fall back to `activityBar.foreground`"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1547: `build_activity_bar`'s `active_accent` must come from the
    /// dedicated `theme.activity_active_accent` colour, not `theme.cursor`
    /// (the field it silently borrowed before this issue introduced a
    /// colour dedicated to the activity-bar accent — see that field's own
    /// doc). `vscode_light`'s `cursor` (`#000000`) and
    /// `activity_active_accent` (`#646e6e`) are deliberately different, so
    /// a regression back to `theme.cursor` fails loudly here rather than
    /// only looking wrong on screen.
    ///
    /// Note: as of #1434, `render::build_activity_bar` itself has no
    /// production caller — `App` renders through
    /// `quadraui::compose::app_shell::AppShell::build_activity_bar`
    /// instead, which hardcodes `active_accent: None` pending a quadraui
    /// hook (that function's own doc cites quadraui#381). This test pins
    /// the adapter's own field-mapping correctness so it's ready the
    /// moment such a hook lands and vimcode wires this function (or its
    /// theme field) back in; it does not claim the accent line paints in
    /// the shipped app today.
    #[test]
    fn build_activity_bar_active_accent_uses_activity_active_accent_not_cursor() {
        let engine = crate::core::Engine::new_for_test();
        let theme = Theme::vscode_light();
        assert_ne!(
            theme.cursor, theme.activity_active_accent,
            "fixture must use a theme where the two colours differ, or this \
             test can't distinguish the fix from the bug"
        );

        let bar = build_activity_bar(&engine, &theme, true, None);

        let expected = quadraui::Color::rgb(
            theme.activity_active_accent.r,
            theme.activity_active_accent.g,
            theme.activity_active_accent.b,
        );
        assert_eq!(bar.active_accent, Some(expected));
    }

    /// #1688: the *descriptor* boundary for the editor tab-bar accent line.
    /// `build_screen_layout`'s per-group loop sets `accent = Some(theme.
    /// tab_active_accent)` only for `gid == engine.active_group`, so this
    /// pins that the `quadraui::TabBar` handed to every backend already
    /// carries the right `active_accent` for both the active and inactive
    /// group of a split — before any backend rasteriser gets a chance to
    /// drop it. #1688 found Win-GUI painting zero accent pixels anywhere in
    /// the tab strip; this test rules out "the descriptor itself is wrong"
    /// as the cause, narrowing the bug to the Win-GUI rasteriser (tracked
    /// upstream, see `docs/PENDING_QUADRAUI_ISSUES.md`'s "Seven Win-GUI
    /// rasterisers" entry / quadraui#1261) rather than anything in this
    /// file.
    #[test]
    fn build_screen_layout_sets_tab_bar_active_accent_only_on_active_group() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 20.0;
        let char_width = 8.0;
        let theme = Theme::vscode_dark();

        let mut engine = Engine::new();
        engine.execute_command("EditorGroupSplit");
        assert_eq!(engine.group_layout.leaf_count(), 2);
        let content_bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, 32.0);
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        assert_eq!(screen.group_tab_bars.len(), 2);

        let expected = Some(theme.tab_active_accent);
        let mut saw_active_some = false;
        for gtb in &screen.group_tab_bars {
            if gtb.group_id == engine.active_group {
                assert_eq!(
                    gtb.bar.active_accent, expected,
                    "the active group's tab bar must carry `theme.tab_active_accent`"
                );
                saw_active_some = true;
            } else {
                assert_eq!(
                    gtb.bar.active_accent, None,
                    "an inactive group's tab bar must carry no accent"
                );
            }
        }
        assert!(saw_active_some, "exactly one group must be active");
    }

    /// #1127: `themes_dir()` must derive from the cross-platform
    /// `core::paths::vimcode_config_dir()` (which handles `APPDATA` on
    /// Windows), not read `$HOME` directly — reading `$HOME` raw would put
    /// custom VS Code theme JSON files in the wrong directory on Windows.
    #[test]
    fn themes_dir_sits_under_vimcode_config_dir() {
        let themes_dir = Theme::themes_dir().expect("themes_dir should resolve");
        let config_dir = crate::core::paths::vimcode_config_dir();
        assert_eq!(themes_dir, config_dir.join("themes"));
        assert!(
            themes_dir.starts_with(&config_dir),
            "themes_dir {themes_dir:?} should be nested under config_dir {config_dir:?}"
        );
    }

    #[test]
    fn test_format_button_label() {
        assert_eq!(super::format_button_label("Recover", 'r'), "[R]ecover");
        assert_eq!(
            super::format_button_label("Delete swap", 'd'),
            "[D]elete swap"
        );
        assert_eq!(super::format_button_label("Abort", 'a'), "[A]bort");
        assert_eq!(super::format_button_label("OK", 'o'), "[O]K");
        // Hotkey not in label → prepended.
        assert_eq!(super::format_button_label("Yes", 'z'), "[Z] Yes");
    }

    #[test]
    fn test_diff_toolbar_on_both_group_tab_bars() {
        use crate::core::engine::{Engine, OpenMode};
        use crate::core::window::SplitDirection;

        let dir = crate::harness::scratch_dir("vimcode_render_diff_groups");
        std::fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "same\nold\nsame\n").unwrap();
        std::fs::write(&f2, "same\nnew\nsame\n").unwrap();

        let mut engine = Engine::new();
        engine
            .open_file_with_mode(&f1, OpenMode::Permanent)
            .unwrap();
        engine.execute_command("diffthis");

        // Create a second editor group and open the second file.
        engine.open_editor_group(SplitDirection::Vertical);
        engine
            .open_file_with_mode(&f2, OpenMode::Permanent)
            .unwrap();
        engine.execute_command("diffthis");
        assert!(engine.is_in_diff_view());

        // Build window rects for both groups.
        let content_bounds = WindowRect::new(0.0, 1.0, 80.0, 24.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, 1.0);
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        // Both group tab bars should have diff_toolbar populated.
        assert!(
            layout.editor_group_split.is_some(),
            "should have editor group split"
        );
        assert!(
            layout.group_tab_bars.len() >= 2,
            "should have 2+ group tab bars"
        );
        for gtb in &layout.group_tab_bars {
            assert!(
                gtb.diff_toolbar.is_some(),
                "group {:?} should have diff toolbar, but it's None",
                gtb.group_id
            );
        }
    }

    #[test]
    fn test_spell_errors_in_rendered_lines() {
        use crate::core::Engine;

        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "the quik brown fox\n");
        engine.settings.spell = true;
        engine.ensure_spell_checker();

        let rects = vec![(
            engine.active_window_id(),
            WindowRect::new(0.0, 0.0, 80.0, 24.0),
        )];
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        // The first window's first line should have a spell error on "quik".
        let window = &layout.windows[0];
        let first_line = &window.lines[0];
        assert!(
            !first_line.spell_errors.is_empty(),
            "expected spell errors on 'the quik brown fox', got none"
        );
        assert_eq!(first_line.spell_errors[0].start_col, 4);
        assert_eq!(first_line.spell_errors[0].end_col, 8);
    }

    // ── RenderedWindow::visible_line_capacity (#1779) ────────────────────────

    /// A one-line buffer in a 24-row window must report a row *capacity*
    /// of 24 even though only 1 row of actual content was painted —
    /// `run_shared_tick_chores` (`src/render.rs`) feeds this straight into
    /// `Engine::set_viewport_for_window`, and `ensure_cursor_visible` (run
    /// on every keystroke, including the one that grows this buffer to 2
    /// lines) trusts that number to decide whether the viewport is tall
    /// enough to show the cursor's line without scrolling.
    ///
    /// This is a supplementary unit test pinning the new field's *value*,
    /// not the black-box coverage for #1779 — that is the real-pty
    /// end-to-end test (`tests/pty_open_line_below_paints_all_lines.rs`)
    /// and the in-process `TuiDriver` regression test
    /// (`src/tui_main/app_on_tui_tests.rs`'s
    /// `opening_a_line_below_the_last_line_paints_every_line_in_order_1779`),
    /// both of which assert on painted screen content, not on this field.
    ///
    /// RED against the pre-fix shape (`visible_line_capacity` not a field;
    /// callers read `lines.len()` instead): `lines.len()` here is `1`, not
    /// `24` — exactly the stale-viewport value that made `ensure_cursor_visible`
    /// believe a 24-row window could show only one line, and scroll line 0
    /// out of view the moment `o<text><Esc>` grew the buffer to two lines.
    #[test]
    fn visible_line_capacity_is_the_window_row_capacity_not_the_painted_line_count() {
        use crate::core::Engine;

        let mut engine = Engine::new();
        // No per-window status row, so the window's whole 24-row rect is
        // text rows — keeps the expected capacity a round number instead
        // of also pinning `window_status_row_reserved`'s own default.
        engine.settings.window_status_line = false;
        engine.buffer_mut().insert(0, "ZQXW_ONELINE\n");

        let rects = vec![(
            engine.active_window_id(),
            WindowRect::new(0.0, 0.0, 80.0, 24.0),
        )];
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        let window = &layout.windows[0];
        assert_eq!(
            window.lines.len(),
            1,
            "precondition: a 1-line buffer only paints 1 line of content"
        );
        assert_eq!(
            window.visible_line_capacity, 24,
            "the window's row capacity must reflect its actual 24-row \
             rect, not how many lines the buffer currently has content \
             for"
        );
    }

    // ── status_bar_zone_hit_test (#672) ──────────────────────────────────────

    #[test]
    fn status_bar_zone_hit_test_resolves_the_segment_a_point_lands_in() {
        let bar_rect = quadraui::Rect::new(100.0, 800.0, 400.0, 20.0);
        let zones = vec![
            (0.0, 50.0, StatusAction::SwitchBranch),
            (50.0, 120.0, StatusAction::GoToLine),
        ];
        // Absolute x = 180 -> local_x = 180 - 100 = 80, inside the second
        // zone's [50, 120) local range.
        let hit = status_bar_zone_hit_test(bar_rect, &zones, 180.0, 810.0);
        assert_eq!(
            hit,
            Some(StatusAction::GoToLine),
            "a point inside the second zone's local_x range must resolve to it"
        );
        let hit_first = status_bar_zone_hit_test(bar_rect, &zones, 120.0, 810.0);
        assert_eq!(
            hit_first,
            Some(StatusAction::SwitchBranch),
            "a point inside the first zone's local_x range must resolve to it"
        );
    }

    #[test]
    fn status_bar_zone_hit_test_misses_outside_the_bar_rect() {
        let bar_rect = quadraui::Rect::new(100.0, 800.0, 400.0, 20.0);
        let zones = vec![(0.0, 400.0, StatusAction::GoToLine)];
        // Same x/y span as the zone, but outside the bar's own rect on each axis.
        assert_eq!(
            status_bar_zone_hit_test(bar_rect, &zones, 50.0, 810.0),
            None,
            "a point left of the bar's x origin must miss, even if the local_x \
             arithmetic alone would land inside a zone"
        );
        assert_eq!(
            status_bar_zone_hit_test(bar_rect, &zones, 200.0, 700.0),
            None,
            "a point above the bar's y band must miss"
        );
        assert_eq!(
            status_bar_zone_hit_test(bar_rect, &zones, 200.0, 900.0),
            None,
            "a point below the bar's y band must miss"
        );
    }

    #[test]
    fn status_bar_zone_hit_test_misses_a_gap_between_segments() {
        let bar_rect = quadraui::Rect::new(0.0, 0.0, 400.0, 20.0);
        let zones = vec![
            (0.0, 50.0, StatusAction::SwitchBranch),
            // Gap between 50 and 100 — no segment painted there (e.g. the
            // bar's own inter-segment padding).
            (100.0, 150.0, StatusAction::GoToLine),
        ];
        assert_eq!(
            status_bar_zone_hit_test(bar_rect, &zones, 75.0, 10.0),
            None,
            "a point in the gap between two segments must not resolve to either"
        );
    }

    // ── Per-window status line tests ─────────────────────────────────────────

    #[test]
    fn test_window_status_line_active() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello world\nsecond line\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // Active window should have a mode badge as the first left segment
        assert!(!status.left_segments.is_empty());
        assert!(
            status.left_segments[0].text.contains("NORMAL"),
            "expected NORMAL mode badge, got '{}'",
            status.left_segments[0].text
        );
        assert!(status.left_segments[0].bold);

        // Should have right segments with cursor position
        assert!(!status.right_segments.is_empty());
        let right_text: String = status
            .right_segments
            .iter()
            .map(|s| s.text.clone())
            .collect();
        assert!(
            right_text.contains("Ln 1"),
            "expected cursor position, got '{}'",
            right_text
        );
    }

    /// #1541 established the rule this test covers; #1690 temporarily
    /// moved the ruler to the *leftmost* of the right group (VS Code
    /// parity) which made `sidebar_toggle_seg` the accidental right-most
    /// segment by default; #1760 moved the ruler (`Ln N, Col N`) back to
    /// being the bar's unconditional right-most segment — not for visual
    /// parity this time, but because that is the only position
    /// `StatusBar::layout`'s priority-drop (quadraui's `fit_right_start`/
    /// `layout_padded`) treats as undroppable, and #1760 reported the
    /// ruler silently vanishing once other optional segments competed for
    /// space. Its text must not carry a trailing space — quadraui#1155
    /// reserves that outer-edge margin on pixel backends (GTK/Win/macOS),
    /// and TUI has never had a scrollbar-style gutter to hide a trailing
    /// blank column in. A stray trailing space here would double the gap
    /// on the backends that already get one and would be a visible
    /// dangling blank on TUI, which gets none.
    ///
    /// RED against the pre-#1541 body (`format!(" Ln {}, Col {} ", ...)`,
    /// trailing space included): `last.text` ends in `" "`, and the second
    /// assertion fails — confirmed by reverting just `cursor_seg`'s format
    /// string and re-running.
    #[test]
    fn test_window_status_line_right_most_segment_has_no_trailing_space() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.settings.ruler = true;
        engine.buffer_mut().insert(0, "hello world\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // #1760: the ruler is the bar's unconditional right-most segment —
        // see this test's own doc for why that is no longer a VS-Code-
        // visual-parity claim but a drop-priority one.
        let last = status
            .right_segments
            .last()
            .expect("ruler on: the active window's bar must have a right-most segment");
        assert!(
            last.text.contains("Ln 1"),
            "expected the ruler to be the right-most segment of the right \
             group, got {:?}",
            status
                .right_segments
                .iter()
                .map(|s| &s.text)
                .collect::<Vec<_>>()
        );
        assert!(
            !last.text.ends_with(' '),
            "the right-most segment must not carry a trailing space \
             (quadraui#1155 gives pixel backends their own outer edge \
             inset); got {:?}",
            last.text
        );
    }

    #[test]
    fn test_window_status_line_inactive() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, false);

        // Inactive should NOT have mode badge
        assert!(!status.left_segments.is_empty());
        assert!(
            !status.left_segments[0].text.contains("NORMAL"),
            "inactive status should not contain mode badge"
        );
        // All segments should use inactive colors
        for seg in &status.left_segments {
            assert_eq!(seg.fg, theme.status_inactive_fg);
        }
    }

    /// #1541: same claim as
    /// [`test_window_status_line_ruler_segment_has_no_trailing_space`], for
    /// the inactive-window bar's own (differently-formatted, no leading
    /// space) ruler segment.
    ///
    /// RED against the pre-#1541 body (`format!("Ln {}, Col {} ", ...)`):
    /// the segment ends in `" "` and this assertion fails — confirmed by
    /// reverting just this segment's format string and re-running.
    #[test]
    fn test_window_status_line_inactive_ruler_segment_has_no_trailing_space() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.settings.ruler = true;
        engine.buffer_mut().insert(0, "hello world\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, false);

        let last = status
            .right_segments
            .last()
            .expect("ruler on: the inactive window's bar must have a right-most segment");
        assert!(
            last.text.contains("Ln 1"),
            "expected the ruler to be the right-most segment, got '{}'",
            last.text
        );
        assert!(
            !last.text.ends_with(' '),
            "the right-most segment must not carry a trailing space \
             (quadraui#1155 gives pixel backends their own outer edge \
             inset); got {:?}",
            last.text
        );
    }

    #[test]
    fn test_window_status_line_dirty_indicator() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "text\n");
        engine
            .buffer_manager
            .get_mut(engine.active_buffer_id())
            .unwrap()
            .dirty = true;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        let left_text: String = status
            .left_segments
            .iter()
            .map(|s| s.text.clone())
            .collect();
        assert!(
            left_text.contains("[+]"),
            "expected dirty indicator, got '{}'",
            left_text
        );
    }

    #[test]
    fn test_window_status_line_insert_mode() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.mode = crate::core::Mode::Insert;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        assert!(status.left_segments[0].text.contains("INSERT"));
        // Mode color used as text tint, not background
        assert_eq!(status.left_segments[0].fg, theme.status_mode_insert_bg);
        // #1690: background comes straight from the theme's own
        // `status_bg` key now, not a `background.lighten(0.10)` offset.
        assert_eq!(status.left_segments[0].bg, theme.status_bg);
    }

    #[test]
    fn test_build_screen_layout_per_window_status() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine
            .buffer_mut()
            .insert(0, "line 1\nline 2\nline 3\nline 4\nline 5\n");

        let wid = engine.active_window_id();
        let rects = vec![(wid, WindowRect::new(0.0, 0.0, 80.0, 24.0))];
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        // Each window should have a status_line
        assert!(layout.windows[0].status_line.is_some());

        // visible_lines should be rect height - 1 (status bar takes 1 row)
        assert_eq!(
            layout.windows[0].lines.len(),
            5, // only 5 lines of content, less than 23 visible lines
            "lines should contain the buffer's actual lines"
        );

        // Global status bar should be None when per-window is on
        assert!(layout.global_status_bar.is_none());
    }

    #[test]
    fn test_build_screen_layout_no_per_window_status() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let mut engine = Engine::new();
        engine.settings.window_status_line = false;
        engine.buffer_mut().insert(0, "hello\n");

        let wid = engine.active_window_id();
        let rects = vec![(wid, WindowRect::new(0.0, 0.0, 80.0, 24.0))];
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        // No per-window status line
        assert!(layout.windows[0].status_line.is_none());

        // Global status bar should be populated
        assert!(layout.global_status_bar.is_some());
    }
}
