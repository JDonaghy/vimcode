use super::*;

pub(crate) fn build_source_control_data(engine: &Engine) -> Option<SourceControlData> {
    // Only populate when the engine has been sc_refresh()ed at least once.
    // We always build it so both GTK and TUI backends can check sc_has_focus.
    let branch = engine
        .git_branch
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());

    // #991: conflicted files carry neither side, so they land here and
    // nowhere else.
    let merge: Vec<ScFileItem> = engine
        .sc_file_statuses
        .iter()
        .filter(|f| f.is_unmerged())
        .map(|f| ScFileItem {
            path: f.path.clone(),
            status_char: crate::core::git::StatusKind::Unmerged.label(),
            is_staged: false,
        })
        .collect();

    let staged: Vec<ScFileItem> = engine
        .sc_file_statuses
        .iter()
        .filter_map(|f| {
            f.staged.map(|s| ScFileItem {
                path: f.path.clone(),
                status_char: s.label(),
                is_staged: true,
            })
        })
        .collect();

    let unstaged: Vec<ScFileItem> = engine
        .sc_file_statuses
        .iter()
        .filter_map(|f| {
            f.unstaged.map(|s| ScFileItem {
                path: f.path.clone(),
                status_char: s.label(),
                is_staged: false,
            })
        })
        .collect();

    let worktrees: Vec<ScWorktreeItem> = engine
        .sc_worktrees
        .iter()
        .map(|wt| ScWorktreeItem {
            path: wt.path.display().to_string(),
            branch: wt.branch.clone().unwrap_or_else(|| "HEAD".to_string()),
            is_current: wt.is_current,
            is_main: wt.is_main,
        })
        .collect();

    let log: Vec<ScLogItem> = engine
        .sc_log
        .iter()
        .map(|e| ScLogItem {
            hash: e.hash.clone(),
            message: e.message.clone(),
        })
        .collect();

    Some(SourceControlData {
        branch,
        ahead: engine.sc_ahead,
        behind: engine.sc_behind,
        merge,
        staged,
        unstaged,
        worktrees,
        log,
        sections_expanded: engine.sc_sections_expanded,
        selected: engine.sc_selected,
        has_focus: engine.sc_has_focus,
        commit_message: engine.sc_commit_message.clone(),
        commit_cursor: engine.sc_commit_cursor,
        commit_input_active: engine.sc_commit_input_active,
        button_focused: engine.sc_button_focused,
        button_hovered: engine.sc_button_hovered,
        branch_picker: if engine.sc_branch_picker_open {
            let filtered = engine.sc_branch_picker_filtered();
            let results = filtered
                .iter()
                .map(|&(i, _)| {
                    let b = &engine.sc_branch_picker_branches[i];
                    (b.name.clone(), b.is_current)
                })
                .collect();
            Some(BranchPickerData {
                query: engine.sc_branch_picker_query.clone(),
                results,
                selected: engine.sc_branch_picker_selected,
                create_mode: false,
                create_input: String::new(),
            })
        } else if engine.sc_branch_create_mode {
            Some(BranchPickerData {
                query: String::new(),
                results: Vec::new(),
                selected: 0,
                create_mode: true,
                create_input: engine.sc_branch_create_input.clone(),
            })
        } else {
            None
        },
        help_open: engine.sc_help_open,
        sc_sections_start_y: engine
            .sc_panel_layout
            .borrow()
            .as_ref()
            .map(|l| l.content_bounds.y),
    })
}

/// Build the Source Control action-button row as a `quadraui::Toolbar`
/// (#505). Commit carries its label + `(c)` key hint and is disabled while
/// the commit message is empty; Push/Pull/Sync are icon-only. Button ids
/// come from [`crate::core::engine::SC_BUTTON_IDS`] so click dispatch can
/// map the hit-test result back to a button index. Both backends call this
/// and hand the result to `Backend::draw_toolbar`.
pub fn sc_button_toolbar(sc: &SourceControlData) -> quadraui::Toolbar {
    use crate::core::engine::SC_BUTTON_IDS;
    use crate::icons;
    use quadraui::{Toolbar, ToolbarButton, WidgetId};

    let action = |idx: usize, label: &str, icon: &str, key_hint: Option<&str>, enabled: bool| {
        ToolbarButton::Action {
            id: WidgetId::new(SC_BUTTON_IDS[idx]),
            label: label.to_string(),
            icon: Some(icon.to_string()),
            key_hint: key_hint.map(|s| s.to_string()),
            enabled,
            is_active: false,
            tooltip: String::new(),
        }
    };

    let commit_enabled = !sc.commit_message.trim().is_empty();
    Toolbar::new(WidgetId::new("sc:buttons")).with_buttons(vec![
        action(
            0,
            "Commit",
            icons::GIT_COMMIT.s(),
            Some("c"),
            commit_enabled,
        ),
        action(1, "", icons::GIT_PUSH.s(), None, true),
        action(2, "", icons::GIT_PULL.s(), None, true),
        action(3, "", icons::GIT_SYNC.s(), None, true),
    ])
}

/// Build the SC `SidebarPanel` — a `quadraui::SidebarPanel` wrapping the
/// action-button toolbar as its header slot (#509). `toolbar_height: None`
/// defers to each backend's idiomatic default (1 cell TUI, `line_height` GTK).
pub fn sc_sidebar_panel(sc: &SourceControlData) -> quadraui::SidebarPanel {
    use quadraui::{SidebarPanel, WidgetId};
    SidebarPanel {
        id: WidgetId::new("sc:panel"),
        toolbar: Some(sc_button_toolbar(sc)),
        toolbar_height: None,
    }
}

/// Draw the SC bottom slab (toolbar + sections) through backend `b` and
/// cache the full `SidebarPanelLayout` on `engine` for click/hover dispatch
/// (#509). `rect` covers the "bottom slab" — from just below the commit input
/// to the bottom of the panel. Both backends call this inside their frame
/// scope; section rendering then reads `content_bounds` from the cached
/// layout. Keyboard focus → pressed_id, mouse hover → hovered_id.
pub fn draw_sc_sidebar_panel(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    sc: &SourceControlData,
    rect: quadraui::Rect,
) {
    let panel = sc_sidebar_panel(sc);
    let hovered = sc.button_hovered.and_then(Engine::sc_button_id);
    let pressed = sc.button_focused.and_then(Engine::sc_button_id);
    let interaction = quadraui::InteractionState::from_parts(hovered, pressed);
    let layout = b.draw_sidebar_panel_interactive(rect, &panel, &interaction);
    engine.sc_panel_layout.replace(Some(layout));
}

/// Format the SC panel's header row text: branch name + ahead/behind
/// counts when present. Shared by both backends so the header text can't
/// drift between TUI and GTK renderers (#480).
pub fn sc_header_text(sc: &SourceControlData) -> String {
    let branch_icon = crate::icons::GIT_BRANCH.nerd;
    if sc.ahead > 0 || sc.behind > 0 {
        format!(
            "  {branch_icon} SOURCE CONTROL  {}  \u{2191}{} \u{2193}{}",
            sc.branch, sc.ahead, sc.behind
        )
    } else {
        format!("  {branch_icon} SOURCE CONTROL  {}", sc.branch)
    }
}

/// Build the SC panel's header row as a single-segment `quadraui::StatusBar`
/// (#480). GTK paints the header through this — TUI keeps its existing
/// direct `set_cell` text row (both read the same [`sc_header_text`]
/// string, so the two can't show different branch info even though the
/// paint mechanism differs).
pub fn sc_header_status_bar(sc: &SourceControlData, theme: &Theme) -> quadraui::StatusBar {
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("sc:header"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: sc_header_text(sc),
            fg: theme.status_fg,
            bg: theme.status_bg,
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    }
}

/// The SC panel's focused-hint text, shared so it can't drift between the
/// row-reservation math in [`sc_sidebar_bands`] and the text actually
/// painted (#1361).
pub const SC_HINT_TEXT: &str = " Press '?' for help";

/// Build the SC panel's focused-hint row as a single-segment
/// `quadraui::StatusBar` (#1361), mirroring [`sc_header_status_bar`]. Both
/// backends paint this through `draw_status_bar` into the row
/// [`sc_sidebar_bands`] reserves via `ScSidebarBands::hint`, so the hint
/// can't appear on one backend and not the other, or land on a different
/// row than what was hit-tested.
pub fn sc_hint_status_bar(theme: &Theme) -> quadraui::StatusBar {
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("sc:hint"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: SC_HINT_TEXT.to_string(),
            fg: theme.line_number_fg,
            bg: theme.status_bg,
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    }
}

/// Number of text rows in the SC commit message (at least 1, even when
/// empty). Shared raw line count — both backends derive their own
/// border/line-height-aware box height from this (#480).
pub fn sc_commit_input_row_count(commit_message: &str) -> u16 {
    commit_message.split('\n').count().max(1) as u16
}

/// Height in *rows* of the SC commit-input box on TUI, including the
/// `TextInput` primitive's 1-cell border on top and bottom (#480). TUI's
/// native unit is one screen cell, so the border costs exactly 2 whole
/// rows — this is the single source of truth shared by TUI's paint code
/// (`panels.rs`) and its click hit-test math (`mouse.rs`), so the two
/// can't drift out of sync the way the pre-migration hand-rolled geometry
/// did. GTK's native unit is pixels, where the same 1-*pixel* border is
/// negligible next to a `line_height` row — GTK computes its box height
/// directly from [`sc_commit_input_row_count`] instead of this function.
pub fn sc_commit_input_box_height(commit_message: &str) -> u16 {
    sc_commit_input_row_count(commit_message) + 2
}

/// The three fixed bands the git ("source control") sidebar stacks inside its
/// content area, top to bottom: the header status bar, the commit-message
/// `TextInput` box, and the toolbar-slab + section list that fills the rest.
///
/// Returned by [`sc_sidebar_bands`] so a backend's *painter* and its *click
/// router* read one derivation instead of two. Both used to inline this
/// arithmetic separately, which is exactly how the pre-#544 GTK click path
/// ended up hit-testing against DrawingArea-local `y` (`0` at the panel top)
/// while the ShellApp painter drew at absolute window coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScSidebarBands {
    /// Header row (branch name + summary).
    pub header: quadraui::Rect,
    /// Commit-message input box, including its border.
    pub commit_input: quadraui::Rect,
    /// The toolbar slab and the change sections — everything between the
    /// commit box and the focused-hint row (or the panel bottom, when
    /// unfocused).
    pub slab: quadraui::Rect,
    /// The "Press '?' for help" row (#1361), reserved at the panel's
    /// bottom only while the panel has keyboard focus — `None` means no
    /// row was reserved, so `slab` already extends to the panel bottom.
    pub hint: Option<quadraui::Rect>,
}

/// Split a git-sidebar content rect into its [`ScSidebarBands`].
///
/// `row_height` is one text row in the caller's native unit (pixels on GTK,
/// cells on TUI). `commit_border` is what the `TextInput` primitive's 1-unit
/// border on top *and* bottom costs in that same unit — 2.0 px on GTK, 2.0
/// rows on TUI (see [`sc_commit_input_box_height`], which is the row-unit
/// spelling of the same constant). `has_focus` reserves one `row_height`
/// row at the bottom for the focused-hint (#1361) — both the painter and
/// the click router pass the same `SourceControlData::has_focus` /
/// `engine.sc_has_focus` value so the reservation can't drift between the
/// two.
pub fn sc_sidebar_bands(
    commit_message: &str,
    rect: quadraui::Rect,
    row_height: f32,
    commit_border: f32,
    has_focus: bool,
) -> ScSidebarBands {
    let header_h = row_height;
    let commit_h = sc_commit_input_row_count(commit_message) as f32 * row_height + commit_border;
    let slab_y = rect.y + header_h + commit_h;
    let hint_h = if has_focus { row_height } else { 0.0 };
    let slab_h = (rect.y + rect.height - slab_y - hint_h).max(0.0);
    let hint = if has_focus {
        Some(quadraui::Rect::new(
            rect.x,
            slab_y + slab_h,
            rect.width,
            hint_h,
        ))
    } else {
        None
    };
    ScSidebarBands {
        header: quadraui::Rect::new(rect.x, rect.y, rect.width, header_h),
        commit_input: quadraui::Rect::new(rect.x, rect.y + header_h, rect.width, commit_h),
        slab: quadraui::Rect::new(rect.x, slab_y, rect.width, slab_h),
        hint,
    }
}

/// #1361: `sc_sidebar_bands` is the *only* place the focused-hint row's
/// reservation is computed — both `App::paint_sidebar_panel_rung`'s
/// `PANEL_GIT` arm (paint) and `App::route_sc_sidebar_event`/
/// `tui_main::mouse`'s `SidebarOwner::Git` arm (click routing) call this
/// exact function with the exact same `has_focus` value the frame was
/// painted with (`cached_sc_bands`/the per-click `engine.sc_has_focus`
/// read), so proving this function's own geometry is internally
/// consistent is what rules out the row-reservation half of "click
/// hit-testing for the rows below must account for the reserved row" —
/// the shared derivation can't drift between painter and router when
/// there is only one derivation for both to call.
///
/// A full click-driven sweep across a *painted row* (the more traditional
/// black-box proof, `crate::harness::sweep_hit_band_integrity`) was tried
/// here first and dropped: it lands on a pre-existing, focus-independent
/// GTK hit-band inaccuracy in `quadraui::SidebarSystem`'s own section
/// header (reproduced identically with `has_focus: false`, i.e. with no
/// hint row reserved at all, so it predates and is unrelated to this
/// fix) — a separate, out-of-scope defect for whoever picks it up next,
/// not a regression this issue introduces.
#[cfg(test)]
mod sc_sidebar_bands_tests {
    use super::*;

    fn rect() -> quadraui::Rect {
        quadraui::Rect::new(0.0, 0.0, 40.0, 20.0)
    }

    #[test]
    fn hint_is_none_when_unfocused_and_slab_fills_the_rest() {
        let unfocused = sc_sidebar_bands("", rect(), 1.0, 2.0, false);
        assert_eq!(unfocused.hint, None, "no row should be reserved unfocused");
        assert_eq!(
            unfocused.slab.y + unfocused.slab.height,
            rect().y + rect().height,
            "with nothing reserved, the slab must reach the panel's bottom edge"
        );
    }

    #[test]
    fn hint_reserves_exactly_one_row_height_at_the_bottom_when_focused() {
        let row_height = 1.0;
        let focused = sc_sidebar_bands("", rect(), row_height, 2.0, true);
        let hint = focused
            .hint
            .expect("a focused panel must reserve the hint row");
        assert_eq!(
            hint.height, row_height,
            "the hint row must be exactly one text row tall"
        );
        assert_eq!(
            hint.y + hint.height,
            rect().y + rect().height,
            "the hint row must sit flush against the panel's bottom edge"
        );
        assert_eq!(
            hint.x,
            rect().x,
            "the hint row must span the panel's full width, starting at its left edge"
        );
        assert_eq!(
            hint.width,
            rect().width,
            "the hint row must span the panel's full width"
        );
    }

    #[test]
    fn focused_slab_is_exactly_one_row_shorter_than_unfocused_and_never_overlaps_the_hint() {
        let row_height = 1.0;
        let unfocused = sc_sidebar_bands("", rect(), row_height, 2.0, false);
        let focused = sc_sidebar_bands("", rect(), row_height, 2.0, true);

        // Same header/commit-input geometry regardless of focus — only the
        // slab shrinks to make room for the hint (#1361's own "only the
        // paint mechanism differs" scoping, applied to geometry: nothing
        // above the slab should ever move because of focus).
        assert_eq!(unfocused.header, focused.header);
        assert_eq!(unfocused.commit_input, focused.commit_input);

        assert_eq!(
            unfocused.slab.height - focused.slab.height,
            row_height,
            "the slab must shrink by exactly one row when the hint is reserved"
        );
        let hint = focused.hint.unwrap();
        assert_eq!(
            focused.slab.y + focused.slab.height,
            hint.y,
            "the slab must end exactly where the hint row begins — no gap, no overlap"
        );
    }

    #[test]
    fn a_multiline_commit_message_shifts_the_hint_reservation_but_not_its_height() {
        // #1361 acceptance: hit-testing for rows *below* the commit box
        // must account for the reserved row regardless of how tall the
        // commit box itself is — the hint row's height must always stay
        // one row, only its `y` (and the slab's) should move.
        let row_height = 1.0;
        let one_line = sc_sidebar_bands("single line", rect(), row_height, 2.0, true);
        let three_lines = sc_sidebar_bands("a\nb\nc", rect(), row_height, 2.0, true);
        let hint1 = one_line.hint.unwrap();
        let hint3 = three_lines.hint.unwrap();
        assert_eq!(hint1.height, hint3.height, "hint height is always one row");
        assert_eq!(
            hint1.y, hint3.y,
            "both variants share the same overall rect, so the hint — \
             anchored to the panel's bottom edge, not the commit box — \
             must land at the same y regardless of commit message length"
        );
        assert!(
            three_lines.commit_input.height > one_line.commit_input.height,
            "sanity: the 3-line commit message must actually claim more \
             rows than the 1-line one, or this test proves nothing"
        );
    }
}

/// Adapt the SC commit-message state into a `quadraui::TextInput` (#480,
/// migrating the hand-rolled `set_cell` commit-row painter to the shared
/// primitive shipped in quadraui#222).
///
/// Converts the engine's byte-offset cursor (`sc.commit_cursor`, an index
/// into the flat `\n`-joined `commit_message` string) into the
/// primitive's `(cursor_line, cursor_col)` char-column coordinates.
/// Render-only: the engine's `handle_sc_commit_input_key` remains the sole
/// owner of edit logic — this function only builds a paint-time snapshot.
pub fn sc_commit_message_to_text_input(sc: &SourceControlData) -> quadraui::TextInput {
    use quadraui::{TextInput, WidgetId};

    let byte_cursor = sc.commit_cursor.min(sc.commit_message.len());
    let before = &sc.commit_message[..byte_cursor];
    let cursor_line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let cursor_col = before[line_start..].chars().count();

    let lines: Vec<String> = if sc.commit_message.is_empty() {
        vec![String::new()]
    } else {
        sc.commit_message.split('\n').map(str::to_string).collect()
    };

    let input = TextInput::new(WidgetId::new("sc:commit_input"))
        .with_lines(lines)
        .with_cursor_line(cursor_line)
        .with_cursor_col(cursor_col)
        .with_has_focus(sc.commit_input_active);
    // Only shown while not actively editing an empty message — matches the
    // pre-migration behaviour of hiding the prompt text as soon as the
    // cursor is live in an empty input.
    if sc.commit_input_active {
        input
    } else {
        input.with_placeholder("Message (press c)")
    }
}

/// Adapt the SC branch-picker popup state into a dual-mode
/// `quadraui::Palette` (#480, migrating the hand-rolled popup to the
/// primitive shipped in quadraui#224). `create_mode` maps to
/// `PaletteMode::Input` (free-text new-branch name); otherwise
/// `PaletteMode::List` with the fuzzy-filtered branch results, current
/// branch marked with a leading bullet.
///
/// Render-only, same as [`sc_commit_message_to_text_input`]: query/cursor
/// editing and selection remain owned by `Engine::handle_sc_branch_picker_key`
/// / `handle_sc_branch_create_key` — this is purely a paint-time snapshot,
/// not an adoption of `DualModePaletteController`'s own (would-be
/// duplicate) key-handling state machine.
pub fn sc_branch_picker_to_palette(bp: &BranchPickerData) -> quadraui::Palette {
    use quadraui::{Palette, PaletteItem, PaletteMode, StyledText, WidgetId};

    if bp.create_mode {
        return Palette {
            id: WidgetId::new("sc:branch_picker"),
            title: "New Branch".to_string(),
            query: bp.create_input.clone(),
            query_cursor: bp.create_input.len(),
            items: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            total_count: 0,
            has_focus: true,
            show_query: true,
            create_label: None,
            preview: None,
            mode: PaletteMode::Input,
        };
    }

    let items: Vec<PaletteItem> = bp
        .results
        .iter()
        .map(|(name, is_current)| PaletteItem {
            text: StyledText::plain(if *is_current {
                format!("\u{25cf} {name}")
            } else {
                format!("  {name}")
            }),
            detail: None,
            icon: None,
            match_positions: Vec::new(),
            depth: 0,
            expandable: false,
            expanded: false,
        })
        .collect();

    Palette {
        id: WidgetId::new("sc:branch_picker"),
        title: "Switch Branch".to_string(),
        query: bp.query.clone(),
        query_cursor: bp.query.len(),
        items,
        selected_idx: bp.selected,
        scroll_offset: 0,
        total_count: 0,
        has_focus: true,
        show_query: true,
        create_label: None,
        preview: None,
        mode: PaletteMode::List,
    }
}

/// Static keybindings table for the SC help dialog (#480, migrating the
/// hand-rolled 2-column popup to `Dialog` + `DialogTable`, shipped in
/// quadraui#225). Shared by both backends so the bindings list has one
/// source of truth.
pub fn sc_help_dialog() -> quadraui::Dialog {
    use quadraui::{Dialog, DialogButton, DialogTable, StyledText, WidgetId};

    const BINDINGS: &[(&str, &str)] = &[
        ("j/k", "Navigate"),
        ("s", "Stage / unstage"),
        ("S", "Stage all"),
        ("d", "Discard file"),
        ("D", "Discard all unstaged"),
        ("c", "Commit message"),
        ("b", "Switch branch"),
        ("B", "Create branch"),
        ("p", "Push"),
        ("P", "Pull"),
        ("f", "Fetch"),
        ("r", "Refresh"),
        ("Tab", "Expand / collapse"),
        ("Enter", "Open file"),
        ("q/Esc", "Close panel"),
    ];

    Dialog {
        id: WidgetId::new("sc:help"),
        title: StyledText::plain("Keybindings"),
        body: Vec::new(),
        buttons: vec![DialogButton {
            id: WidgetId::new("sc:help:close"),
            label: "Close".to_string(),
            is_default: true,
            is_cancel: true,
            tint: None,
        }],
        severity: None,
        vertical_buttons: false,
        table: Some(DialogTable {
            headers: Some(vec!["Key".to_string(), "Action".to_string()]),
            rows: BINDINGS
                .iter()
                .map(|(k, d)| vec![k.to_string(), d.to_string()])
                .collect(),
            column_widths: None,
        }),
        input: None,
    }
}

/// Compute the `DialogLayout` for [`sc_help_dialog`] from generic
/// char-cell/pixel metrics (TUI: `1.0, 1.0`; GTK: real `char_width`/
/// `line_height`, #546-style dual-backend convention). Mirrors
/// `dialog_generic_layout`'s char-cell approximation formula, but sized
/// from the table's own `tui_total_width`/`tui_total_height` helpers
/// since this dialog has no body text driving its width.
pub fn sc_help_dialog_layout(
    viewport: quadraui::Rect,
    char_width: f32,
    line_height: f32,
) -> (quadraui::Dialog, quadraui::DialogLayout) {
    let dialog = sc_help_dialog();
    let table = dialog
        .table
        .as_ref()
        .expect("sc_help_dialog always sets `table`");

    let table_h = table.tui_total_height() as f32 * line_height;
    let table_w = table.tui_total_width() as f32 * char_width + char_width * 2.0;

    let min_w = char_width * 30.0;
    let max_w = char_width * 60.0;
    let default_w = (viewport.width * 0.5).clamp(min_w, max_w);
    let width = default_w
        .max(table_w)
        .min(viewport.width - char_width * 4.0);

    let measure = quadraui::DialogMeasure {
        width,
        title_height: line_height,
        body_height: 0.0,
        table_height: table_h,
        input_height: 0.0,
        button_row_height: line_height,
        button_width: char_width * 8.0,
        button_gap: char_width * 2.0,
        padding: line_height,
    };
    let layout = dialog.layout(viewport, measure, |_| {
        quadraui::ToolbarItemMeasure::new(0.0)
    });
    (dialog, layout)
}

/// Build the `(Tooltip, TooltipLayout, TooltipChrome)` for a plugin
/// ext-panel's own `?`-triggered keybindings help popup (#636).
///
/// #635 (Stage 6b item C) migrated this popup from raw `set_cell`
/// box-drawing (full 4-sided border + centred title embedded in the top
/// border) to `Backend::draw_tooltip`, which at the time could only ever
/// paint TUI's side-bars-only chrome with no way to ask for a full border
/// or a title at all — a real regression, filed upstream as
/// JDonaghy/quadraui#541. #541 has since landed at this repo's pinned
/// quadraui rev: `TooltipChrome` (a sidecar value passed to the new
/// `Backend::draw_tooltip_with_chrome`, so it doesn't touch `Tooltip`'s or
/// `TooltipLayout`'s own field sets) carries exactly the `border`/`title`
/// vocabulary this popup needs, with `TooltipBorder::Full` + a title
/// landing it in the same full-border-with-embedded-title shape the
/// original raw-drawing code painted. This is the real fix, not the
/// "documented temporary stand-in" the issue anticipated needing — #541
/// was built (and references JDonaghy/vimcode#635 by name in its own doc
/// comment) specifically to unblock this call site.
///
/// `panel_name` namespaces the tooltip's `WidgetId` so two different
/// plugin panels' help popups (never shown simultaneously, but still)
/// don't collide; `bindings` is the plugin-registered `(key, description)`
/// list (`Engine::ext_panel_help_bindings`). `char_width`/`line_height`
/// follow the same generic char-cell/pixel convention
/// [`sc_help_dialog_layout`] uses (TUI: `1.0, 1.0`; GTK: real metrics).
pub fn ext_panel_help_tooltip_layout(
    panel_name: &str,
    bindings: &[(String, String)],
    viewport: quadraui::Rect,
    char_width: f32,
    line_height: f32,
) -> (
    quadraui::Tooltip,
    quadraui::TooltipLayout,
    quadraui::TooltipChrome,
) {
    let key_fg_width = bindings
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0)
        + 1;
    let lines: Vec<quadraui::StyledText> = bindings
        .iter()
        .map(|(key, desc)| quadraui::StyledText {
            spans: vec![
                quadraui::StyledSpan::plain(format!("{key:<key_fg_width$} ")),
                quadraui::StyledSpan::plain(desc.clone()),
            ],
        })
        .collect();

    let mut tooltip = quadraui_tooltip(
        quadraui::WidgetId::new(format!("ext:help:{panel_name}")),
        String::new(),
    );
    tooltip.styled_lines = Some(lines);

    let content_w = bindings
        .iter()
        .map(|(k, d)| k.chars().count() + 1 + d.chars().count())
        .max()
        .unwrap_or(0)
        .max("Keybindings".len());
    let popup_w = ((content_w as f32 + 2.0) * char_width)
        .min(viewport.width - char_width * 2.0)
        .max(char_width * 12.0);
    let popup_h = ((bindings.len() as f32 + 2.0) * line_height).min(viewport.height - line_height);
    let popup_x = viewport.x + (viewport.width - popup_w) / 2.0;
    let popup_y = viewport.y + (viewport.height - popup_h) / 2.0;

    let layout = quadraui::TooltipLayout {
        bounds: quadraui::Rect::new(popup_x, popup_y, popup_w, popup_h),
        resolved_placement: quadraui::ResolvedPlacement::Bottom,
    };
    let mut chrome = quadraui::TooltipChrome::new(quadraui::TooltipBorder::Full);
    chrome.title = Some("Keybindings".to_string());
    (tooltip, layout, chrome)
}

/// Populate the `SidebarSystem` on `engine.dap_sidebar_system` with
/// current row data for all 4 debug sidebar sections. Call once per
/// frame before `sidebar_system.render()` or `.handle()`.
pub fn populate_dap_sidebar_system(engine: &Engine) {
    let session_active = engine.dap_session_active;

    // ── Variables section ──
    let var_rows = build_dap_var_rows(engine, session_active);
    // ── Watch section ──
    let watch_rows = build_dap_watch_rows(engine, session_active);
    // ── Call Stack section ──
    let stack_rows = build_dap_stack_rows(engine, session_active);
    // ── Breakpoints section ──
    let bp_rows = build_dap_bp_rows(engine, session_active);

    let mut sidebar = engine.dap_sidebar_system.borrow_mut();
    sidebar.set_has_focus(engine.dap_sidebar_has_focus);
    if engine.dap_sidebar_has_focus && sidebar.active_section().is_none() {
        sidebar.set_active_section(Some(0));
    }
    sidebar.set_rows(0, var_rows);
    sidebar.set_rows(1, watch_rows);
    sidebar.set_rows(2, stack_rows);
    sidebar.set_rows(3, bp_rows);
}

pub fn populate_ext_sidebar_system(engine: &Engine) {
    engine.populate_ext_sidebar_system();
}

/// `MsvLayoutMetrics` for a pixel-unit GUI backend's `SidebarSystem`
/// instances (Source Control, plugin ext panels).
///
/// #971: `SidebarSystem::handle_cached` returns `SidebarEvent::Ignored`
/// unconditionally until `set_backend_info` has been called at least once,
/// and nothing on GTK/macOS ever called it — TUI's own one-time
/// `set_backend_info(1.0, ..)` at startup (`tui_main/shell_app.rs`'s
/// `App::from_engine`) is the *only* call site in the whole crate before
/// this one. So every content-row press on the Source Control and plugin
/// ext panels (header collapse, row select/activate) silently did nothing
/// on both GUI backends — it just looked like the feature had never been
/// wired up rather than "the same bug on every backend", because the one
/// existing GTK test for this panel (`sidebar_panel_clicks::
/// git_panel_click_activates_the_commit_box_but_not_the_header`) only
/// exercises the header/commit-input bands, which return early in
/// `route_sc_sidebar_click` before ever reaching `handle_cached`. #971's
/// own sweep tests are what surfaced it: `src/macos/mod.rs`'s
/// `sc_panel_header_click_hit_band_matches_the_painted_row` /
/// `ext_panel_header_click_hit_band_matches_the_painted_row` sanity-check
/// that one header click actually toggles the section *before* trusting
/// the sweep's own cross-sample comparison — a sweep whose probe silently
/// does nothing passes just as cleanly as one that works, since every
/// sample would agree with the (unchanged) baseline either way.
///
/// Called fresh every frame from each GTK/macOS `paint_sidebar_panel_rung`
/// call site, not once at startup like TUI's fixed metrics — a pixel
/// backend's `line_height` can change (zoom, font settings) where TUI's
/// cell grid cannot, the same #540/#967 drift class every other
/// pixel-backend hit-test in this file re-applies its metrics against
/// rather than trusting a stale snapshot.
pub fn gui_sidebar_system_metrics(line_height: f32) -> quadraui::MsvLayoutMetrics {
    quadraui::MsvLayoutMetrics {
        // Matches `SidebarSystem::compute_tree_layout`'s own row-height
        // formula (`(lh * 1.4).round()`) — headers paint at the same
        // height as a content row, and the two must agree since
        // `compute_layout`'s header band and `compute_tree_layout`'s row
        // pitch are what stack to build the panel's total content height,
        // both starting from the same `rect`.
        header_size: (line_height * 1.4).round(),
        divider_size: 0.0,
        // Matches the picker's own GTK/macOS scrollbar gutter width
        // (`gtk_picker_rows`'s `scrollbar_w`).
        scrollbar_size: 6.0,
        // Sub-pixel layout — quadraui's own doc on this field: "GTK leaves
        // it 0.0" (`MsvLayoutMetrics::cell_quantum`).
        cell_quantum: 0.0,
    }
}

/// A plain leaf `TreeRow`: no icon, no badge, not expandable, no inline
/// edit overlay, `Decoration::Normal` — just a `path`, `indent`, and
/// rendered `text`. Shared shape for the many sidebar-panel builders below
/// that previously each wrote out the full `TreeRow { … }` literal with
/// these same five fields fixed and only `path`/`indent`/`text` varying
/// (#1496).
pub(crate) fn plain_tree_row(
    path: Vec<u16>,
    indent: u16,
    text: quadraui::StyledText,
) -> quadraui::TreeRow {
    quadraui::TreeRow {
        path,
        indent,
        icon: None,
        text,
        badge: None,
        is_expanded: None,
        decoration: quadraui::Decoration::Normal,
        edit: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── SC SidebarPanel tests (#509) ────────────────────────────────────────

    #[test]
    fn test_sc_sidebar_panel_toolbar_slot_reserved_at_top() {
        use quadraui::{primitives::toolbar::ToolbarItemMeasure, SidebarPanelMeasure};

        let sc = empty_sc_data();
        let panel = sc_sidebar_panel(&sc);
        // TUI default: toolbar_height = 1 cell, item_width = 8 cells.
        let area = quadraui::Rect::new(0.0, 5.0, 60.0, 20.0);
        let measure = SidebarPanelMeasure::new(1.0, 8.0);
        let layout = panel.layout(area, measure, |_| ToolbarItemMeasure::new(8.0));

        // Toolbar slot is reserved at the top.
        let tb = layout.toolbar_bounds.expect("toolbar slot reserved");
        assert_eq!(tb.y, 5.0, "toolbar slot starts at panel top");
        assert_eq!(tb.height, 1.0, "TUI default toolbar height = 1 cell");
        // Content starts immediately below toolbar slot (no padding — option a).
        assert_eq!(
            layout.content_bounds.y, 6.0,
            "content starts at toolbar_y + 1"
        );
        assert_eq!(
            layout.content_bounds.height, 19.0,
            "content height = panel_height - 1"
        );
    }

    #[test]
    fn test_sc_sidebar_panel_hit_test_resolves_toolbar_button_and_content() {
        use crate::core::engine::Engine;
        use quadraui::{
            primitives::toolbar::ToolbarItemMeasure, SidebarPanelHit, SidebarPanelMeasure,
        };

        let mut sc = empty_sc_data();
        sc.commit_message = "feat: fix".into(); // non-empty → Commit enabled
        let panel = sc_sidebar_panel(&sc);
        let area = quadraui::Rect::new(0.0, 0.0, 60.0, 10.0);
        let measure = SidebarPanelMeasure::new(1.0, 8.0);
        let layout = panel.layout(area, measure, |_| ToolbarItemMeasure::new(8.0));

        // A hit in the toolbar slot (y=0, which is the toolbar row) on a button.
        let hit = layout.hit_test(0.5, 0.0);
        match hit {
            SidebarPanelHit::ToolbarButton(id) => {
                assert_eq!(
                    Engine::sc_button_index(&id),
                    Some(0),
                    "first button = Commit"
                );
            }
            other => panic!("expected ToolbarButton hit, got {other:?}"),
        }

        // A hit in the content area (y=2, content starts at y=1) returns content-local coords.
        let hit = layout.hit_test(5.0, 2.0);
        match hit {
            SidebarPanelHit::Content { x, y } => {
                assert_eq!(x, 5.0);
                assert_eq!(y, 1.0, "content-local y = abs_y - content_bounds.y = 2-1");
            }
            other => panic!("expected Content hit, got {other:?}"),
        }
    }

    #[test]
    fn test_sc_sidebar_panel_content_bounds_height() {
        use quadraui::{primitives::toolbar::ToolbarItemMeasure, SidebarPanelMeasure};

        let sc = empty_sc_data();
        let panel = sc_sidebar_panel(&sc);
        // Simulate a 15-row panel starting at y=3 (e.g. after 3 header/commit rows).
        let area = quadraui::Rect::new(0.0, 3.0, 40.0, 15.0);
        let measure = SidebarPanelMeasure::new(1.0, 8.0);
        let layout = panel.layout(area, measure, |_| ToolbarItemMeasure::new(8.0));

        assert_eq!(
            layout.content_bounds.height, 14.0,
            "content = panel_height(15) - toolbar_slot(1)"
        );
        assert_eq!(layout.content_bounds.y, 4.0, "content starts at y=3+1=4");
    }
}
