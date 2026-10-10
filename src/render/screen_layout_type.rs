use super::*;

// ─── ScreenLayout ─────────────────────────────────────────────────────────────

/// The complete, platform-agnostic description of one editor frame.
/// Build it with [`build_screen_layout`], then hand it to the backend renderer.
///
/// # Every field has a composition verdict (#766)
///
/// #587 and #592 are both the same defect: a field populated here every frame
/// and painted by nobody, so the *state* said the surface was open while the
/// screen said it was not. #766's closing sweep audited all 36 fields against
/// the three composers ([`compose_frame`], [`compose_editor_band`],
/// [`compose_bottom_band`]) and the shared painters they call. The verdict:
///
/// * **All are composed** — either directly in a backend's rung arm, or in a
///   shared painter that arm calls (`bottom_tabs`, for instance, is composed by
///   [`paint_bottom_panel_rung`] from the `BottomOp::BottomPanel` arm on both
///   backends, not by either backend itself).
///
/// #766 recorded one exception, `menu_dropdown_open`: populated every frame,
/// painted by nobody, kept solely because the Win-GUI backend-parity harness
/// read it as *state* to declare its expected element set. #812 deleted that
/// harness (the backend it compared against was removed in 3e4bcff), which left
/// the field with zero readers, so the field went with it. The dropdown itself
/// is unaffected — it is painted by `MenuSystem::render` from the
/// [`FrameOp::MenuDropdown`] rung, which reads the `MenuSystem` directly.
///
/// Adding a field means adding its verdict — a rung in [`FRAME_Z_ORDER`] /
/// [`EDITOR_Z_ORDER`] / [`BOTTOM_Z_ORDER`], or a doc comment saying where it is
/// consumed instead and why that is not a silent drop.
#[derive(Debug)]
pub struct ScreenLayout {
    pub tab_bar: Vec<TabInfo>,
    pub windows: Vec<RenderedWindow>,
    /// Global status bar (when per-window status lines are disabled).
    pub global_status_bar: Option<quadraui::StatusBar>,
    pub command: CommandLineData,
    /// Wildmenu bar (Tab completion in command mode), or `None` when inactive.
    pub wildmenu: Option<WildmenuData>,
    pub active_window_id: WindowId,
    /// Completion popup to show, or `None` when inactive.
    pub completion: Option<CompletionMenu>,
    /// Hover information popup, or `None` when inactive.
    pub hover: Option<HoverPopup>,
    /// Quickfix bottom panel, or `None` when closed.
    pub quickfix: Option<QuickfixPanel>,
    /// Bottom panel tabs (Terminal / Debug Output) — always present.
    pub bottom_tabs: BottomPanelTabs,
    /// Signature help popup (shown in insert mode after `(` or `,`), or `None`.
    pub signature_help: Option<SignatureHelp>,
    /// Menu bar strip data, or `None` when the bar is hidden.
    pub menu_bar_visible: bool,
    /// Debug toolbar strip data, or `None` when hidden and no active session.
    pub debug_toolbar: Option<DebugToolbarData>,
    /// Debug sidebar data — always present (sections may be empty).
    pub debug_sidebar: DebugSidebarData,
    /// Source Control panel data — `Some` when the SC panel is the active sidebar panel.
    pub source_control: Option<SourceControlData>,
    /// Unified picker modal — `Some` when open.
    pub picker: Option<PickerPanel>,
    /// Tab switcher popup (Ctrl+Tab MRU list) — `Some` when open.
    pub tab_switcher: Option<TabSwitcherPanel>,
    /// Marker for "the editor area holds 2 or more groups", carrying the
    /// focused group + group count. `None` in the default single-group mode.
    /// The per-group chrome it used to own lives on `group_tab_bars` /
    /// `group_dividers` below, which are populated for *every* group count
    /// (#551).
    pub editor_group_split: Option<EditorGroupSplitData>,
    /// Tab bar + bounds for every editor group, in tree traversal order.
    /// Always populated — a single group is a split of one, so this holds
    /// exactly one entry in the default unsplit case rather than being empty
    /// with a parallel single-group field. Backends iterate it unconditionally
    /// (via `tab_bar_draw_targets`) instead of carrying a hand-written
    /// "exactly one group" draw path beside the generic N-group one (#551).
    pub group_tab_bars: Vec<GroupTabBar>,
    /// Divider lines *between* editor groups (`Ctrl+W v` / `Ctrl+W s`
    /// boundaries), in tree traversal order. Naturally empty when there is
    /// only one group — `GroupLayout::Leaf::dividers()` returns `vec![]` — so
    /// backends can paint it unconditionally (#551). Distinct from
    /// `window_dividers`, which are the `:split`/`:vsplit` boundaries *within*
    /// each group.
    /// **#764 audit verdict (#735 slice 3): composed here, on both backends.**
    /// Before #764 this was the field #735's own body flagged: populated every
    /// frame, hit-tested for divider drags on both backends
    /// (`screen_zone_hit_test`), and *painted* only by TUI — GTK discarded it
    /// into `_group_dividers` and drew only `window_dividers`, so a `Ctrl+W v`
    /// boundary on GTK was draggable but invisible. It is now the
    /// [`EditorOp::GroupDividers`] rung of [`EDITOR_Z_ORDER`], which both
    /// backends walk, so "populated but never composed" is a compile error
    /// here rather than something a `grep` has to notice.
    pub group_dividers: Vec<GroupDivider>,
    /// Board panel data (#521) — always `Some` so backends can check
    /// `has_focus`.
    pub board: Option<BoardData>,
    /// Extension-provided panel data — `Some` when an extension panel is the active sidebar panel.
    pub ext_panel: Option<ExtPanelData>,
    /// Breadcrumb bars for each editor group (empty when breadcrumbs are disabled).
    pub breadcrumbs: Vec<BreadcrumbBar>,
    /// Panel hover popup — `Some` when hovering over a sidebar panel item.
    pub panel_hover: Option<PanelHoverPopupData>,
    /// Editor hover popup — `Some` when hovering over an editor element (diagnostic, annotation, etc.).
    pub editor_hover: Option<EditorHoverPopupData>,
    /// Git diff peek popup — `Some` when the user is previewing a diff hunk.
    pub diff_peek: Option<DiffPeekPopup>,
    /// The change-review surface (#955, shared with #525) — `Some` when a
    /// tool-call `diff` (or a future #525 git-branch diff feed) is open
    /// for review. Cloned wholesale from `Engine::change_review` rather
    /// than converted field-by-field like `DiffPeekPopup`: it already
    /// carries a fully paint-ready `quadraui::DiffView` per entry, so
    /// there is nothing this projection needs to compute.
    pub change_review: Option<crate::core::review::ChangeReviewState>,
    // `diff_toolbar` used to sit here — the single-group mirror of
    // `GroupTabBar::diff_toolbar`.
    //
    // **#765 audit verdict (#735 slice 4): superseded, deleted.** Slice 1
    // recorded it as "never composed" and left the removal to whichever slice
    // could fold its last readers away; this is that slice. #551 made
    // `group_tab_bars` populated for *every* group count (a single group is a
    // split of one), and both backends have painted from the per-group
    // `gtb.diff_toolbar` via `tab_bar_draw_targets` ever since. Its only
    // remaining readers were the three `collect_*_ui_elements` parity
    // harnesses, each of which already carried a per-group branch immediately
    // beside the single-group one; those two branches are now the one
    // unconditional loop over `group_tab_bars` that the paint itself uses.
    // (TUI's `mouse.rs` never read this field — it reads `gtb.diff_toolbar`.)
    /// Modal dialog popup — `Some` when a dialog is open.
    pub dialog: Option<DialogPanel>,
    /// Inline find/replace overlay — `Some` when the find/replace popup is open.
    pub find_replace: Option<FindReplacePanel>,
    /// Context menu popup — `Some` when an engine context menu is open.
    pub context_menu: Option<ContextMenuPanel>,
    /// Tab hover tooltip: shortened file path to display near the hovered tab.
    pub tab_tooltip: Option<String>,
    // `tab_scroll_offset` and `tab_bar_primitive` used to sit here — the
    // single-group mirrors of `GroupTabBar::tab_scroll_offset` and
    // `GroupTabBar::bar`.
    //
    // **#764 audit verdict (#735 slice 3): superseded, deleted.** #551 made
    // `group_tab_bars` populated for *every* group count (a single group is a
    // split of one) and both backends have painted from the per-group
    // `gtb.bar` via `tab_bar_draw_targets` ever since. `tab_bar_primitive` had
    // zero readers anywhere, not even a test; `tab_scroll_offset`'s last
    // reader was one TUI test assertion, which now reads
    // `group_tab_bars[0].tab_scroll_offset` — the value the paint actually
    // uses. Slice 1 recorded both as "superseded, never composed" and left the
    // deletion to this slice because it lands in the editor band.
    /// The `quadraui::TabBarLayout` (char-cell columns, relative to the tab
    /// bar's left edge) for the single-group / active tab bar. A blank
    /// (zero-tab) layout in multi-group mode (each group carries its own
    /// layout on its `GroupTabBar::hit_regions`). Lets backends resolve
    /// tab-bar clicks through the shared `resolve_tab_bar_click` path instead
    /// of per-backend pixel maps. (#515, #822)
    pub tab_bar_hit_regions: quadraui::TabBarLayout,
    /// When `status_line_above_terminal` is OFF and the terminal panel is open,
    /// this carries the active window's status line to render as a dedicated row
    /// above the terminal panel. When `Some`, per-window `status_line` fields on
    /// individual `RenderedWindow`s are `None`.
    /// (Setting name is historical — the UI labels it "Status Line Inside Window";
    /// `true` keeps the bar inside each editor window, `false` extracts it.)
    pub separated_status_line: Option<WindowStatusLine>,
    /// Window-split (`:split`/`:vsplit`) dividers across all editor groups'
    /// active tabs. Independent of `editor_group_split` — window splits exist
    /// regardless of how many editor groups are open (#582).
    pub window_dividers: Vec<WindowDivider>,
    /// Code-overview minimap for *every* editor window (#35, #722) — one
    /// entry per `WindowId` that has the `minimap` setting on and enough
    /// width to spare the strip, empty when none do. A `:vsplit` therefore
    /// carries two entries, one per pane, instead of a single strip pinned
    /// to whichever pane happens to be active. Backends paint each with a
    /// `draw_minimap` call (see `draw_minimap_strip`, which loops over this
    /// vec) and route clicks through `MinimapLayout::hit_test`; every piece
    /// of sampling, scale arithmetic, colour aggregation and dot packing
    /// lives in quadraui.
    pub minimap: Vec<RenderedMinimap>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ─── ScreenLayout rendering tests ────────────────────────────────────

    /// Build a ScreenLayout for an engine with the given content at the given
    /// terminal dimensions (in character cells).
    pub(crate) fn render_engine(engine: &Engine, width: f64, height: f64) -> ScreenLayout {
        let bounds = WindowRect::new(0.0, 0.0, width, height);
        let (rects, _) = engine.calculate_group_window_rects(bounds, 1.0);
        let theme = Theme::onedark();
        build_screen_layout(
            engine,
            &theme,
            &rects,
            1.0,
            1.0,
            true,
            0.0,
            TUI_MINIMAP_SIZING,
        )
    }

    pub(crate) fn test_engine(text: &str) -> Engine {
        crate::core::session::suppress_disk_saves();
        // `Engine::new_for_test()` builds settings/session/history/git_branch
        // from in-memory defaults instead of loading ambient disk/git state
        // (#615, #439, #617) — see its doc comment for why call-then-overwrite
        // on `Engine::new()` doesn't reliably undo `app_shell.hide_sidebar()`,
        // and why leaving `git_branch` unset here matters: it feeds the
        // right-hand status segments rendered by this file's tests.
        let mut e = Engine::new_for_test();
        e.mode = Mode::Normal;
        if !text.is_empty() {
            e.buffer_mut().insert(0, text);
        }
        e
    }

    fn test_screen_layout_dirty_tab() {
        let mut e = test_engine("hello\n");
        // Make a change to dirty the buffer
        e.handle_key("i", Some('i'), false);
        e.handle_key("x", Some('x'), false);
        e.handle_key("Escape", None, false);

        let layout = render_engine(&e, 80.0, 24.0);
        assert!(
            layout.tab_bar[0].dirty,
            "modified buffer should show dirty tab"
        );
    }

    #[test]
    fn test_screen_layout_line_numbers() {
        let mut e = test_engine("line1\nline2\nline3\nline4\nline5\n");
        e.settings.line_numbers = LineNumberMode::Absolute;
        let layout = render_engine(&e, 80.0, 24.0);

        let win = &layout.windows[0];
        assert!(
            win.gutter_char_width > 0,
            "line numbers should produce a gutter"
        );
        // Gutter text should have line numbers
        assert!(win.lines[0].gutter_text.contains('1'));
        assert!(win.lines[1].gutter_text.contains('2'));
    }

    #[test]
    fn test_screen_layout_status_segments() {
        let e = test_engine("hello\n");
        let layout = render_engine(&e, 80.0, 24.0);

        // Per-window status lines should have segments
        let win = &layout.windows[0];
        if let Some(ref status) = win.status_line {
            assert!(
                !status.left_segments.is_empty(),
                "status should have left segments"
            );
            assert!(
                !status.right_segments.is_empty(),
                "status should have right segments"
            );

            // Mode should be shown
            let mode_text: String = status
                .left_segments
                .iter()
                .map(|s| s.text.as_str())
                .collect();
            assert!(
                mode_text.contains("NORMAL") || mode_text.contains("NOR"),
                "status should show normal mode, got: {mode_text}"
            );
        }
    }
}
