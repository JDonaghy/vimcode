use super::*;

// ─── quadraui::TabBar adapter (A.6c / A.6d) ──────────────────────────────────

/// The `WidgetId` every editor tab bar paints under.
///
/// Backends cache a resolved `quadraui::TabBarLayout` per `WidgetId` at paint
/// time, and quadraui#594's `GtkDriver::tab_center` / `tab_close_center` look
/// the layout back up by that id — so a test aiming a click at a specific tab
/// needs this exact string. Named here rather than spelled out at each call
/// site so the harness and the primitive cannot drift apart (#659).
///
/// Note this is a *per-bar* id, not a per-group one: in a split every group's
/// tab bar paints under the same id, so the cached layout is whichever group
/// painted last. That is fine for the single-group tests that consume it and
/// is why [`crate::core::window::GroupId`]-keyed geometry still exists on the
/// `App` side for production click routing.
pub const EDITOR_TAB_BAR_WIDGET_ID: &str = "tabs:group";

/// Build a `quadraui::TabBar` primitive from the render-level tab args.
/// Shared by TUI and GTK backends — the primitive is layout-agnostic;
/// backends interpret it against their own measurement / drawing models.
///
/// Right-side segment order (mirrors the pre-migration layout):
/// `[diff label?] [diff prev] [diff next] [diff fold?] [split right] [split down] [action menu]`
///
/// `active_accent` carries the active-tab accent colour only when the group
/// is focused. TUI interprets as underline; GTK as 2px top bar.
/// `width_cells` on each segment is a TUI hint; GTK measures with Pango.
pub fn build_tab_bar_primitive(
    tabs: &[TabInfo],
    show_split_btns: bool,
    diff_toolbar: Option<&DiffToolbarData>,
    tab_scroll_offset: usize,
    active_accent: Option<quadraui::Color>,
) -> quadraui::TabBar {
    let tab_items: Vec<quadraui::TabItem> = tabs
        .iter()
        .map(|t| quadraui::TabItem {
            label: t.name.clone(),
            is_active: t.active,
            is_dirty: t.dirty,
            is_preview: t.preview,
            is_closable: true,
        })
        .collect();

    let mut right: Vec<quadraui::TabBarSegment> = Vec::new();

    // Build a 3-cell tab-bar button segment from an `Icon`. When nerd
    // fonts are enabled, the icon glyph is rendered as a 2-cell wide
    // glyph (` <wide>`); otherwise the fallback ASCII char takes 1
    // cell, padded with spaces (` <c> `). Either way `width_cells = 3`
    // matches the rasteriser's per-cell stride so layout positions
    // line up with what gets painted.
    fn tab_btn_segment(
        icon: &crate::icons::Icon,
        id: &str,
        is_active: bool,
    ) -> quadraui::TabBarSegment {
        let text = if crate::icons::nerd_fonts_enabled() {
            format!(" {}", icon.s())
        } else {
            format!(" {} ", icon.s())
        };
        quadraui::TabBarSegment {
            text,
            width_cells: 3,
            id: Some(quadraui::WidgetId::new(id)),
            is_active,
        }
    }

    if let Some(dt) = diff_toolbar {
        if let Some(label) = &dt.change_label {
            let text = format!(" {label}");
            let width = text.chars().count() as u16;
            right.push(quadraui::TabBarSegment {
                text,
                width_cells: width,
                id: None,
                is_active: false,
            });
        }
        right.push(tab_btn_segment(
            &crate::icons::DIFF_PREV,
            "tab:diff_prev",
            false,
        ));
        right.push(tab_btn_segment(
            &crate::icons::DIFF_NEXT,
            "tab:diff_next",
            false,
        ));
        right.push(tab_btn_segment(
            &crate::icons::DIFF_FOLD,
            "tab:diff_toggle",
            dt.unchanged_hidden,
        ));
    }

    if show_split_btns {
        right.push(tab_btn_segment(
            &crate::icons::SPLIT_RIGHT,
            "tab:split_right",
            false,
        ));
        right.push(tab_btn_segment(
            &crate::icons::SPLIT_DOWN,
            "tab:split_down",
            false,
        ));
    }

    right.push(quadraui::TabBarSegment {
        // Action menu uses U+22EF (HORIZONTAL ELLIPSIS) which is a
        // standard Unicode glyph, not a Nerd Font codepoint.
        text: " \u{22EF} ".to_string(),
        width_cells: 3,
        id: Some(quadraui::WidgetId::new("tab:action_menu")),
        is_active: false,
    });

    quadraui::TabBar {
        id: quadraui::WidgetId::new(EDITOR_TAB_BAR_WIDGET_ID),
        tabs: tab_items,
        scroll_offset: tab_scroll_offset,
        right_segments: right,
        active_accent,
        show_tab_close: true,
        compact: false,
    }
}

/// Build a `quadraui::TabBar` for the bottom panel tab switcher
/// (Terminal / Debug Output). The close button (×) is a right segment.
/// Tabs with `close_width: 0.0` suppress per-tab close glyphs.
pub fn build_bottom_panel_tab_bar(
    active: &BottomPanelKind,
    has_terminal: bool,
    has_debug_output: bool,
) -> quadraui::TabBar {
    let mut tabs = Vec::new();
    if has_terminal {
        tabs.push(quadraui::TabItem {
            label: "Terminal".to_string(),
            is_active: *active == BottomPanelKind::Terminal,
            is_dirty: false,
            is_preview: false,
            is_closable: true,
        });
    }
    if has_debug_output {
        tabs.push(quadraui::TabItem {
            label: "Debug Output".to_string(),
            is_active: *active == BottomPanelKind::DebugOutput,
            is_dirty: false,
            is_preview: false,
            is_closable: true,
        });
    }

    let close_seg = quadraui::TabBarSegment {
        text: " \u{00d7} ".to_string(),
        width_cells: 3,
        id: Some(quadraui::WidgetId::new("bottom_tab:close")),
        is_active: false,
    };

    quadraui::TabBar {
        id: quadraui::WidgetId::new("tabs:bottom_panel"),
        tabs,
        scroll_offset: 0,
        right_segments: vec![close_seg],
        active_accent: None,
        show_tab_close: false,
        compact: true,
    }
}

// ─── Terminal toolbar adapter (#305) ─────────────────────────────────────────

/// The terminal toolbar is either a find bar or a tab strip.
pub enum TerminalToolbar {
    FindBar(quadraui::StatusBar),
    TabStrip(quadraui::TabBar),
}

/// Build a `TerminalToolbar` from the current terminal panel state.
pub fn build_terminal_toolbar(panel: &TerminalPanel, theme: &Theme) -> TerminalToolbar {
    if panel.find_active {
        let fg = theme.status_fg;
        let bg = theme.status_bg;

        let match_info = if panel.find_match_count == 0 {
            if panel.find_query.is_empty() {
                String::new()
            } else {
                " (no matches)".to_string()
            }
        } else {
            format!(
                " ({}/{})",
                panel.find_selected_idx + 1,
                panel.find_match_count
            )
        };
        let find_text = format!(" FIND: {}█{}", panel.find_query, match_info);

        TerminalToolbar::FindBar(quadraui::StatusBar {
            id: quadraui::WidgetId::new("term_toolbar"),
            left_segments: vec![quadraui::StatusBarSegment {
                text: find_text,
                fg,
                bg,
                bold: false,
                action_id: None,
            }],
            right_segments: vec![quadraui::StatusBarSegment {
                text: format!(" {} ", crate::icons::TERM_CLOSE.s()),
                fg,
                bg,
                bold: false,
                action_id: Some(quadraui::WidgetId::new("term_toolbar:find_close")),
            }],
        })
    } else {
        let mut tabs: Vec<quadraui::TabItem> = (0..panel.tab_count)
            .map(|i| quadraui::TabItem {
                label: format!("[{}]", i + 1),
                is_active: i == panel.active_tab,
                is_dirty: false,
                is_preview: false,
                is_closable: true,
            })
            .collect();

        if tabs.is_empty() {
            tabs.push(quadraui::TabItem {
                label: "TERMINAL".to_string(),
                is_active: false,
                is_dirty: false,
                is_preview: false,
                is_closable: true,
            });
        }

        let maxicon = if panel.maximized {
            crate::icons::TERM_UNMAXIMIZE.s()
        } else {
            crate::icons::TERM_MAXIMIZE.s()
        };

        let right = vec![
            quadraui::TabBarSegment {
                text: "+ ".to_string(),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:add")),
                is_active: false,
            },
            quadraui::TabBarSegment {
                text: format!("{} ", crate::icons::TERM_SPLIT.s()),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:split")),
                is_active: false,
            },
            quadraui::TabBarSegment {
                text: format!("{} ", maxicon),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:maximize")),
                is_active: false,
            },
            quadraui::TabBarSegment {
                text: format!("{} ", crate::icons::TERM_CLOSE.s()),
                width_cells: 2,
                id: Some(quadraui::WidgetId::new("term_toolbar:close")),
                is_active: false,
            },
        ];

        TerminalToolbar::TabStrip(quadraui::TabBar {
            id: quadraui::WidgetId::new("term_toolbar"),
            tabs,
            scroll_offset: 0,
            right_segments: right,
            active_accent: None,
            show_tab_close: false,
            compact: true,
        })
    }
}

/// Build the backend-agnostic `quadraui::Theme` from vimcode's rich
/// `render::Theme`. Shared by both TUI and GTK backends — every
/// `draw_*` delegate and `Backend::set_current_theme` call site uses
/// this single source of truth.
pub fn to_quadraui_theme(theme: &Theme) -> quadraui::Theme {
    let chrome = to_quadraui_theme_chrome(theme);
    to_quadraui_theme_editor(theme, chrome)
}

/// #1574: this literal is **exhaustive on purpose** — no
/// `..quadraui::Theme::default()` spread. Every `quadraui::Theme` field
/// vimcode's rasterisers paint with must come from vimcode's own themed
/// palette; a spread here would let a newly-added quadraui `Theme` field
/// silently fall back to quadraui's hardcoded dark default (exactly how
/// `inactive_selected_bg` shipped unmapped, painting `#333` text on a dark
/// navy `(35,40,58)` unfocused-selection row under `vscode-light`) instead
/// of failing the build with `error[E0063]: missing field`. quadraui's own
/// `theme.rs` uses the identical exhaustive-literal technique for exactly
/// this reason — that one guards quadraui's downstream consumers (e.g. the
/// unmapped `tab_active_border_top` it caught in quadraui#620) rather than a
/// consumer's own mapping, but the compile-time guarantee is the same one
/// this literal now gives vimcode. Adding a field to `quadraui::Theme` is
/// a compile error here until it's mapped (or deliberately assigned a
/// documented vimcode-side value below) — that is the point.
///
/// Some fields below (`editor_active_background` through `ghost_text_fg`)
/// are immediately re-asserted by [`to_quadraui_theme_editor`]'s own
/// `..chrome` struct-update — they're set here too, to the same value,
/// purely so this literal stays exhaustive; there is no drift risk
/// because both sites read the identical `theme.*` field.
fn to_quadraui_theme_chrome(theme: &Theme) -> quadraui::Theme {
    quadraui::Theme {
        background: theme.background,
        foreground: theme.foreground,
        tab_bar_bg: theme.tab_bar_bg,
        tab_active_bg: theme.tab_active_bg,
        tab_active_fg: theme.tab_active_fg,
        tab_inactive_fg: theme.tab_inactive_fg,
        tab_preview_active_fg: theme.tab_preview_active_fg,
        tab_preview_inactive_fg: theme.tab_preview_inactive_fg,
        separator: theme.separator,
        surface_bg: theme.fuzzy_bg,
        surface_fg: theme.fuzzy_fg,
        selected_bg: theme.fuzzy_selected_bg,
        // #1574: was missing entirely (fell through to quadraui's dark
        // `Theme::default()` navy) — this is the Explorer's
        // unfocused-selected-row background (`sidebar_sel_bg_inactive`,
        // e4e6f1 under vscode-light).
        inactive_selected_bg: theme.sidebar_sel_bg_inactive,
        border_fg: theme.fuzzy_border,
        title_fg: theme.fuzzy_title_fg,
        header_bg: theme.status_bg,
        header_fg: theme.status_fg,
        muted_fg: theme.line_number_fg,
        error_fg: theme.diagnostic_error,
        warning_fg: theme.diagnostic_warning,
        query_fg: theme.fuzzy_query_fg,
        match_fg: theme.fuzzy_match_fg,
        accent_fg: theme.cursor,
        hover_bg: theme.hover_bg,
        hover_fg: theme.hover_fg,
        hover_border: theme.hover_border,
        input_bg: theme.completion_bg,
        inactive_fg: theme.status_inactive_fg,
        selection_bg: theme.selection,
        link_fg: theme.md_link,
        completion_bg: theme.completion_bg,
        completion_fg: theme.completion_fg,
        completion_border: theme.completion_border,
        completion_selected_bg: theme.completion_selected_bg,
        accent_bg: theme.tab_active_accent,
        // #1574: was `theme.separator` — vimcode has its own dedicated
        // `scrollbar_track` field (distinct from `separator` in every
        // colourscheme); using it directly is both more correct and
        // consistent with `scrollbar_thumb` just below.
        scrollbar_track: theme.scrollbar_track,
        scrollbar_thumb: theme.scrollbar_thumb,
        // #1185: quadraui's `command_line_{bg,fg}` default to its own
        // hardcoded colours (`Theme::default()`'s `bg`/`fg`, unrelated to
        // this literal's `background`/`foreground` override above) unless
        // mapped explicitly. Both backends now paint the command line
        // through `Backend::draw_command_line_selection`, which reads
        // these two fields — without this mapping, adopting that call
        // would have silently swapped every colourscheme's themed command
        // line for quadraui's defaults (a regression for TUI, which used
        // to read `theme.command_{fg,bg}` by hand).
        command_line_bg: theme.command_bg,
        command_line_fg: theme.command_fg,

        // ── Editor lift fields — real values set (again) by
        // `to_quadraui_theme_editor`'s `..chrome` update below. Present
        // here only so this literal is exhaustive (see doc comment above);
        // read the doc on each corresponding field in
        // `to_quadraui_theme_editor` for the actual rationale.
        editor_active_background: theme.active_background,
        cursorline_bg: theme.cursorline_bg,
        dap_stopped_bg: theme.dap_stopped_bg,
        colorcolumn_bg: theme.colorcolumn_bg,
        diff_added_bg: theme.diff_added_bg,
        diff_removed_bg: theme.diff_removed_bg,
        diff_padding_bg: theme.diff_padding_bg,
        line_number_fg: theme.line_number_fg,
        line_number_active_fg: theme.line_number_active_fg,
        diagnostic_error: theme.diagnostic_error,
        diagnostic_warning: theme.diagnostic_warning,
        diagnostic_info: theme.diagnostic_info,
        diagnostic_hint: theme.diagnostic_hint,
        git_added: theme.git_added,
        git_modified: theme.git_modified,
        git_deleted: theme.git_deleted,
        lightbulb: theme.lightbulb,
        spell_error: theme.spell_error,
        cursor: theme.cursor,
        cursor_normal_alpha: theme.cursor_normal_alpha as f32,
        selection: theme.selection,
        selection_alpha: theme.selection_alpha as f32,
        yank_highlight_bg: theme.yank_highlight_bg,
        yank_highlight_alpha: theme.yank_highlight_alpha as f32,
        bracket_match_bg: theme.bracket_match_bg,
        indent_guide_fg: theme.indent_guide_fg,
        indent_guide_active_fg: theme.indent_guide_active_fg,
        annotation_fg: theme.annotation_fg,
        ghost_text_fg: theme.ghost_text_fg,

        // ── Board / kanban (#362) — vimcode's `PANEL_BOARD` /
        // `Backend::draw_board` (#521) were never given their own theme
        // mapping (#1574 audit); reuse the closest existing semantic
        // colour rather than leaving them on quadraui's dark default.
        // Board's selected-card / column-header look like a ListView
        // selection / a flat header strip respectively, so the same
        // source fields as `selected_bg`/`header_bg` above apply.
        board_selected_card_bg: theme.fuzzy_selected_bg,
        board_col_header_bg: theme.status_bg,
        // `BadgeStatus` has no vimcode-native equivalent; map onto the
        // nearest existing semantic colour rather than inventing new
        // theme fields (one per colourscheme) for a status vocabulary
        // vimcode doesn't otherwise have:
        //   running  -> lightbulb (amber "in progress" cue, distinct
        //               from the warning tone below)
        //   passed   -> git_added (green)
        //   warning  -> diagnostic_warning (amber/orange)
        //   blocked  -> diagnostic_error (red)
        badge_running: theme.lightbulb,
        badge_passed: theme.git_added,
        badge_warning: theme.diagnostic_warning,
        badge_blocked: theme.diagnostic_error,
        // `BoardCard::hint` is a callout strip — closest existing
        // surface is the hover/tooltip popup.
        card_hint_bg: theme.hover_bg,
        card_hint_fg: theme.hover_fg,
    }
}

fn to_quadraui_theme_editor(theme: &Theme, chrome: quadraui::Theme) -> quadraui::Theme {
    quadraui::Theme {
        editor_active_background: theme.active_background,
        cursorline_bg: theme.cursorline_bg,
        dap_stopped_bg: theme.dap_stopped_bg,
        colorcolumn_bg: theme.colorcolumn_bg,
        diff_added_bg: theme.diff_added_bg,
        diff_removed_bg: theme.diff_removed_bg,
        diff_padding_bg: theme.diff_padding_bg,
        line_number_fg: theme.line_number_fg,
        line_number_active_fg: theme.line_number_active_fg,
        diagnostic_error: theme.diagnostic_error,
        diagnostic_warning: theme.diagnostic_warning,
        diagnostic_info: theme.diagnostic_info,
        diagnostic_hint: theme.diagnostic_hint,
        git_added: theme.git_added,
        git_modified: theme.git_modified,
        git_deleted: theme.git_deleted,
        lightbulb: theme.lightbulb,
        spell_error: theme.spell_error,
        cursor: theme.cursor,
        cursor_normal_alpha: theme.cursor_normal_alpha as f32,
        selection: theme.selection,
        selection_alpha: theme.selection_alpha as f32,
        yank_highlight_bg: theme.yank_highlight_bg,
        yank_highlight_alpha: theme.yank_highlight_alpha as f32,
        bracket_match_bg: theme.bracket_match_bg,
        indent_guide_fg: theme.indent_guide_fg,
        indent_guide_active_fg: theme.indent_guide_active_fg,
        annotation_fg: theme.annotation_fg,
        ghost_text_fg: theme.ghost_text_fg,
        ..chrome
    }
}

// ─── quadraui::Editor adapter (#276 Stage 1C) ────────────────────────────────
//
// Convert a vimcode `RenderedWindow` (engine-side IR) into a
// `quadraui::Editor` for the lifted TUI / GTK rasterisers. Field-for-
// field mapping; the engine builder remains unchanged (`RenderedWindow`
// is still consumed by mouse hit-testing in `tui_main/mouse.rs` and
// `gtk/click.rs`, which is why we adapt at the boundary rather than
// retargeting the builder).

/// Build the [`quadraui::Editor`] + [`quadraui::EditorLayout`] pair for a
/// window using **exactly** the same construction paint uses
/// (`to_q_editor` + `Editor::layout(editor.rect, ...)`), so click-column
/// resolution and paint derive from one shared geometry computation
/// instead of two independently reconstructed ones (#560). Callers pass
/// the resulting `&Editor`/`&EditorLayout` straight into
/// `quadraui::Backend::editor_col_at_x` (GTK: exact Pango `xy_to_index`
/// against the same per-span-attributed layout `draw_editor` painted
/// with; TUI: `EditorLayout::col_at_x`'s uniform monospace division) —
/// neither backend hand-rolls its own text-column inverse anymore.
///
/// **GTK-correct, TUI-unsafe (#1040).** For GTK, `rw.rect` *is* the exact
/// sub-pixel float geometry Cairo paints into, so `editor.rect` genuinely
/// matches paint here. For TUI it does not: `rw.rect` comes from
/// continuous float split math (`quadraui::SplitTree::layout`, zero
/// divider thickness) and is not integer-valued in general — a vertical
/// group split at the default 50/50 ratio over an odd content width gives
/// the *right* pane's `rect.x` a `.5`-cell fractional origin. TUI's paint
/// path truncates that away to whole cells before drawing
/// (`tui_main::render_impl`'s `win_rect`/`editor_area`, both `rect.x as
/// u16`) — bypassing `editor.rect` entirely, since `Backend::draw_editor`
/// takes its viewport as an explicit `Rect` argument, not from the
/// `Editor` struct. Calling this function directly from TUI click code
/// therefore resolves columns against a viewport that was never actually
/// painted, landing one column left of the real one (clamped to 0 at the
/// pane's first column, so it "sometimes" doesn't — exactly the #1040
/// report). TUI click/drag/hover call sites must use
/// [`tui_editor_text_layout`] instead, which resolves against
/// [`tui_window_paint_rect`]'s whole-cell-truncated viewport.
pub fn editor_text_layout(
    rw: &RenderedWindow,
    char_width: f64,
    line_height: f64,
) -> (quadraui::Editor, quadraui::EditorLayout) {
    let editor = to_q_editor(rw);
    let layout = editor.layout(editor.rect, char_width as f32, line_height as f32);
    (editor, layout)
}

/// Truncate a window rect to whole terminal cells — the exact conversion
/// TUI's paint path applies before handing a window's geometry to
/// `ratatui`/quadraui's cell-grid rasteriser (`tui_main::render_impl`'s
/// `win_rect` in `render_all_windows`, and the `editor_area` derived from
/// it in `render_window`, both `rect.x as u16` etc.).
///
/// See [`editor_text_layout`]'s doc for why this exists: `RenderedWindow`
/// rects are produced by continuous float split math and are not
/// integer-valued in general, so click resolution must snap to the same
/// grid paint already snapped to, or it silently resolves against
/// geometry that was never painted (#1040).
///
/// TUI-only — GTK rects are real sub-pixel float geometry that Cairo
/// paints exactly as given; do not call this from `gtk/click.rs`.
pub fn tui_window_paint_rect(rect: &WindowRect) -> WindowRect {
    WindowRect::new(
        (rect.x as u16) as f64,
        (rect.y as u16) as f64,
        (rect.width as u16) as f64,
        (rect.height as u16) as f64,
    )
}

/// TUI-only variant of [`editor_text_layout`]: builds the same
/// [`quadraui::Editor`], but lays it out against
/// [`tui_window_paint_rect`]'s whole-cell-truncated viewport instead of
/// the raw (possibly fractional) `rw.rect` — the viewport TUI's paint
/// path actually drew into. `Editor::layout` only reads its `viewport`
/// argument for geometry (never the `Editor.rect` field itself), so this
/// does not disturb anything else `to_q_editor`'s `editor.rect` is used
/// for. Every TUI click/drag/hover call site that resolves a text column
/// must use this, not `editor_text_layout` (#1040).
pub fn tui_editor_text_layout(rw: &RenderedWindow) -> (quadraui::Editor, quadraui::EditorLayout) {
    let editor = to_q_editor(rw);
    let viewport = quadraui::Rect::from(tui_window_paint_rect(&rw.rect));
    let layout = editor.layout(viewport, 1.0, 1.0);
    (editor, layout)
}

/// Build a [`quadraui::Editor`] from a [`RenderedWindow`]. The
/// per-window status line is **not** included — the caller paints
/// it after calling `draw_editor` (status-line lift was Session 241).
pub fn to_q_editor(rw: &RenderedWindow) -> quadraui::Editor {
    let rect = quadraui::Rect::new(
        rw.rect.x as f32,
        rw.rect.y as f32,
        rw.rect.width as f32,
        rw.rect.height as f32,
    );
    let mut editor = quadraui::Editor::new(
        quadraui::WidgetId::new(format!("editor:{}", rw.window_id.0)),
        rect,
    )
    .with_lines(rw.lines.iter().map(to_q_editor_line).collect())
    .with_extra_cursors(
        rw.extra_cursors
            .iter()
            .copied()
            .map(to_q_cursor_pos)
            .collect(),
    )
    .with_extra_selections(rw.extra_selections.iter().map(to_q_selection).collect())
    .with_scroll_top(rw.scroll_top)
    .with_scroll_left(rw.scroll_left)
    .with_total_lines(rw.total_lines)
    .with_max_col(rw.max_col)
    .with_gutter_char_width(rw.gutter_char_width)
    .with_is_active(rw.is_active)
    .with_show_active_bg(rw.show_active_bg)
    .with_has_git_diff(rw.has_git_diff)
    .with_has_breakpoints(rw.has_breakpoints)
    .with_diagnostic_gutter(
        rw.diagnostic_gutter
            .iter()
            .map(|(&l, &s)| (l, to_q_severity(s)))
            .collect(),
    )
    .with_code_action_lines(rw.code_action_lines.iter().copied().collect())
    .with_bracket_match_positions(rw.bracket_match_positions.clone())
    .with_tabstop(rw.tabstop)
    .with_cursorline(rw.cursorline)
    .with_lightbulb_glyph(crate::icons::LIGHTBULB.c());

    if let Some(cursor) = rw.cursor.map(|(pos, shape)| quadraui::EditorCursor {
        pos: to_q_cursor_pos(pos),
        shape: to_q_cursor_shape(shape),
    }) {
        editor = editor.with_cursor(cursor);
    }
    if let Some(selection) = rw.selection.as_ref().map(to_q_selection) {
        editor = editor.with_selection(selection);
    }
    if let Some(yank_highlight) = rw.yank_highlight.as_ref().map(to_q_selection) {
        editor = editor.with_yank_highlight(yank_highlight);
    }
    if let Some(active_indent_col) = rw.active_indent_col {
        editor = editor.with_active_indent_col(active_indent_col);
    }
    editor
}

fn to_q_editor_line(rl: &RenderedLine) -> quadraui::EditorLine {
    quadraui::EditorLine {
        raw_text: rl.raw_text.clone(),
        gutter_text: rl.gutter_text.clone(),
        spans: rl.spans.iter().map(to_q_styled_span).collect(),
        line_idx: rl.line_idx,
        is_current_line: rl.is_current_line,
        is_fold_header: rl.is_fold_header,
        folded_line_count: rl.folded_line_count,
        git_diff: rl.git_diff.map(to_q_git_status),
        diff_status: rl.diff_status.map(to_q_diff_line),
        diagnostics: rl.diagnostics.iter().map(to_q_diagnostic_mark).collect(),
        spell_errors: rl.spell_errors.iter().map(to_q_spell_mark).collect(),
        is_breakpoint: rl.is_breakpoint,
        is_conditional_bp: rl.is_conditional_bp,
        is_dap_current: rl.is_dap_current,
        is_wrap_continuation: rl.is_wrap_continuation,
        segment_col_offset: rl.segment_col_offset,
        annotation: rl.annotation.clone(),
        ghost_suffix: rl.ghost_suffix.clone(),
        is_ghost_continuation: rl.is_ghost_continuation,
        indent_guides: rl.indent_guides.clone(),
        colorcolumns: rl.colorcolumns.clone(),
    }
}

fn to_q_styled_span(span: &StyledSpan) -> quadraui::EditorStyledSpan {
    quadraui::EditorStyledSpan {
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        style: quadraui::EditorStyle {
            fg: span.style.fg,
            bg: span.style.bg,
            bold: span.style.bold,
            italic: span.style.italic,
            font_scale: span.style.font_scale as f32,
        },
    }
}

fn to_q_cursor_pos(pos: CursorPos) -> quadraui::EditorCursorPos {
    quadraui::EditorCursorPos {
        view_line: pos.view_line,
        col: pos.col,
    }
}

fn to_q_cursor_shape(shape: CursorShape) -> quadraui::EditorCursorShape {
    match shape {
        CursorShape::Block => quadraui::EditorCursorShape::Block,
        CursorShape::Bar => quadraui::EditorCursorShape::Bar,
        CursorShape::Underline => quadraui::EditorCursorShape::Underline,
    }
}

fn to_q_selection(sel: &SelectionRange) -> quadraui::EditorSelection {
    quadraui::EditorSelection {
        kind: match sel.kind {
            SelectionKind::Char => quadraui::EditorSelectionKind::Char,
            SelectionKind::Line => quadraui::EditorSelectionKind::Line,
            SelectionKind::Block => quadraui::EditorSelectionKind::Block,
        },
        start_line: sel.start_line,
        start_col: sel.start_col,
        end_line: sel.end_line,
        end_col: sel.end_col,
    }
}

fn to_q_severity(s: crate::core::lsp::DiagnosticSeverity) -> quadraui::DiagnosticSeverity {
    use crate::core::lsp::DiagnosticSeverity as V;
    match s {
        V::Error => quadraui::DiagnosticSeverity::Error,
        V::Warning => quadraui::DiagnosticSeverity::Warning,
        V::Information => quadraui::DiagnosticSeverity::Information,
        V::Hint => quadraui::DiagnosticSeverity::Hint,
    }
}

fn to_q_git_status(s: GitLineStatus) -> quadraui::GitLineStatus {
    match s {
        GitLineStatus::Added => quadraui::GitLineStatus::Added,
        GitLineStatus::Modified => quadraui::GitLineStatus::Modified,
        GitLineStatus::Deleted => quadraui::GitLineStatus::Deleted,
    }
}

fn to_q_diff_line(d: DiffLine) -> quadraui::DiffLine {
    match d {
        DiffLine::Same => quadraui::DiffLine::Same,
        DiffLine::Added => quadraui::DiffLine::Added,
        DiffLine::Removed => quadraui::DiffLine::Removed,
        DiffLine::Padding => quadraui::DiffLine::Padding,
    }
}

fn to_q_diagnostic_mark(dm: &DiagnosticMark) -> quadraui::DiagnosticMark {
    quadraui::DiagnosticMark {
        start_col: dm.start_col,
        end_col: dm.end_col,
        severity: to_q_severity(dm.severity),
        message: dm.message.clone(),
    }
}

fn to_q_spell_mark(sm: &SpellMark) -> quadraui::SpellMark {
    quadraui::SpellMark {
        start_col: sm.start_col,
        end_col: sm.end_col,
    }
}

// ─── quadraui::StatusBar adapter (A.6a) ──────────────────────────────────────

/// String id encoding a `StatusAction`. Paired with [`status_action_from_id`].
/// Used to adapt vimcode's engine-side `StatusAction` enum to quadraui's
/// type-erased `WidgetId`-keyed segment actions.
pub fn status_action_id(action: &StatusAction) -> &'static str {
    match action {
        StatusAction::GoToLine => "status:goto_line",
        StatusAction::ChangeLanguage => "status:change_language",
        StatusAction::ChangeIndentation => "status:change_indentation",
        StatusAction::ChangeLineEnding => "status:change_line_ending",
        StatusAction::ChangeEncoding => "status:change_encoding",
        StatusAction::SwitchBranch => "status:switch_branch",
        StatusAction::LspInfo => "status:lsp_info",
        StatusAction::ToggleSidebar => "status:toggle_sidebar",
        StatusAction::TogglePanel => "status:toggle_panel",
        StatusAction::ToggleMenuBar => "status:toggle_menu_bar",
        StatusAction::DismissNotifications => "status:dismiss_notifications",
        StatusAction::ShowDiagnostics => "status:show_diagnostics",
    }
}

/// Inverse of [`status_action_id`]: decode a `WidgetId` string back into a
/// `StatusAction`. Returns `None` for unknown ids (plugin-emitted, future, etc.).
pub fn status_action_from_id(id: &str) -> Option<StatusAction> {
    match id {
        "status:goto_line" => Some(StatusAction::GoToLine),
        "status:change_language" => Some(StatusAction::ChangeLanguage),
        "status:change_indentation" => Some(StatusAction::ChangeIndentation),
        "status:change_line_ending" => Some(StatusAction::ChangeLineEnding),
        "status:change_encoding" => Some(StatusAction::ChangeEncoding),
        "status:switch_branch" => Some(StatusAction::SwitchBranch),
        "status:lsp_info" => Some(StatusAction::LspInfo),
        "status:toggle_sidebar" => Some(StatusAction::ToggleSidebar),
        "status:toggle_panel" => Some(StatusAction::TogglePanel),
        "status:toggle_menu_bar" => Some(StatusAction::ToggleMenuBar),
        "status:dismiss_notifications" => Some(StatusAction::DismissNotifications),
        "status:show_diagnostics" => Some(StatusAction::ShowDiagnostics),
        _ => None,
    }
}

/// Hit-test an absolute pixel point against a painted status bar's rect and
/// its segment zones, resolving to the `StatusAction` of whichever segment
/// it landed on (`None` if the point misses the bar entirely, or lands in a
/// gap between segments).
///
/// `zones` is `(start_x, end_x, action)` triples **local to `bar_rect`'s own
/// origin** — the same shape GTK's `status_segment_map` stores per bar
/// (populated from `Backend::status_bar_layout`'s hit regions at paint
/// time). Shared here so a backend with more than one on-screen status bar
/// (GTK's separated status line sits outside any window's own rect, so it
/// can't reuse `window_zone_hit_test`'s window-relative contract) resolves
/// clicks the same way the per-window bar does, instead of a bespoke
/// second copy of this arithmetic. TUI has no equivalent call site: its own
/// hit test (`tui_main/mouse.rs::status_segment_hit_test`) re-derives the
/// bar layout fresh from row/col on every click rather than consulting a
/// persistent pixel cache.
pub fn status_bar_zone_hit_test(
    bar_rect: quadraui::Rect,
    zones: &[(f64, f64, StatusAction)],
    x: f64,
    y: f64,
) -> Option<StatusAction> {
    let (bx, by, bw, bh) = (
        bar_rect.x as f64,
        bar_rect.y as f64,
        bar_rect.width as f64,
        bar_rect.height as f64,
    );
    if x < bx || x >= bx + bw || y < by || y >= by + bh {
        return None;
    }
    let local_x = x - bx;
    zones
        .iter()
        .find(|(start, end, _)| local_x >= *start && local_x < *end)
        .map(|(_, _, action)| action.clone())
}

/// Convert a `WindowStatusLine` (built by `build_window_status_line`) into a
/// `quadraui::StatusBar` primitive. Engine-owned `StatusAction` enums are
/// flattened to opaque `WidgetId` strings so the primitive is
/// engine-agnostic (plugin invariants §10).
///
/// `id` identifies the bar (useful if multiple status bars are rendered, e.g.
/// per-window). Callers can use e.g. `WidgetId::new("status:w0")`.
pub fn window_status_line_to_status_bar(
    status: &WindowStatusLine,
    id: quadraui::WidgetId,
) -> quadraui::StatusBar {
    fn to_seg(s: &StatusSegment) -> quadraui::StatusBarSegment {
        quadraui::StatusBarSegment {
            text: s.text.clone(),
            fg: quadraui::Color::rgb(s.fg.r, s.fg.g, s.fg.b),
            bg: quadraui::Color::rgb(s.bg.r, s.bg.g, s.bg.b),
            bold: s.bold,
            action_id: s
                .action
                .as_ref()
                .map(|a| quadraui::WidgetId::new(status_action_id(a))),
        }
    }
    quadraui::StatusBar {
        id,
        left_segments: status.left_segments.iter().map(to_seg).collect(),
        right_segments: status.right_segments.iter().map(to_seg).collect(),
    }
}

/// Paint the tab-hover tooltip (the small popup shown when the mouse
/// hovers a tab and lingers, naming the buffer under the cursor) through
/// `Backend::draw_status_bar` — the same "single-segment `StatusBar` stands
/// in for a plain text row" trick TUI's now-retired `draw_rule_row_q` used
/// (#609), generalized with an explicit `unit_w`/`unit_h` scale (#671)
/// mirroring the convention `editor_hover_popup_paint` /
/// `hover_popup_to_quadraui_tooltip` established in #669: `1.0`/`1.0` for
/// TUI's cell-native space, `char_width`/`line_height` in pixels for GTK.
/// `x`/`y`/`max_width` are in the *same* native units as `unit_w`/`unit_h`
/// (cells for TUI, pixels for GTK).
#[allow(clippy::too_many_arguments)]
pub fn tab_hover_tooltip_paint(
    backend: &mut dyn quadraui::Backend,
    x: f32,
    y: f32,
    max_width: f32,
    tooltip_text: &str,
    theme: &Theme,
    unit_w: f32,
    unit_h: f32,
) {
    if tooltip_text.is_empty() {
        return;
    }
    let uw = unit_w.max(0.001);
    let len = tooltip_text.chars().count() as f32;
    let max_chars = (max_width / uw).floor().max(0.0);
    let w_chars = len.min(max_chars);
    if w_chars <= 0.0 {
        return;
    }
    let text: String = tooltip_text.chars().take(w_chars as usize).collect();
    backend.set_theme(to_quadraui_theme(theme));
    let bar = quadraui::StatusBar {
        // Shares one literal ID across every call the way `draw_rule_row_q`'s
        // "tui:rule" ID did — inert today since every caller discards the
        // returned hit-region layout.
        id: quadraui::WidgetId::new("tooltip:tab"),
        left_segments: vec![quadraui::StatusBarSegment {
            text,
            fg: theme.hover_fg,
            bg: theme.hover_bg,
            bold: false,
            action_id: None,
        }],
        right_segments: vec![],
    };
    let rect = quadraui::Rect::new(x, y, w_chars * uw, unit_h);
    let _ = backend.draw_status_bar_interactive(rect, &bar, &quadraui::InteractionState::new());
}

pub fn build_command_line(engine: &Engine) -> CommandLineData {
    let (text, right_align, show_cursor, cursor_anchor_text) = match engine.mode {
        Mode::Command if engine.history_search_active => {
            let display = format!(
                "(reverse-i-search)'{}': {}",
                engine.history_search_query, engine.command_buffer
            );
            // Cursor sits after the full `:command_buffer` text (in the command line)
            let anchor = format!(":{}", engine.command_buffer);
            (display, false, true, anchor)
        }
        Mode::Command => {
            let prefix_chars: String = engine
                .command_buffer
                .chars()
                .take(engine.command_cursor)
                .collect();
            let anchor = format!(":{}", prefix_chars);
            let full = format!(":{}", engine.command_buffer);
            (full, false, true, anchor)
        }
        Mode::Search => {
            let ch = match engine.search_direction {
                SearchDirection::Forward => '/',
                SearchDirection::Backward => '?',
            };
            let prefix_chars: String = engine
                .command_buffer
                .chars()
                .take(engine.command_cursor)
                .collect();
            let anchor = format!("{}{}", ch, prefix_chars);
            let full = format!("{}{}", ch, engine.command_buffer);
            (full, false, true, anchor)
        }
        Mode::Normal | Mode::Visual | Mode::VisualLine => {
            if let Some(count) = engine.peek_count() {
                (count.to_string(), true, false, String::new())
            } else {
                (engine.message.clone(), false, false, String::new())
            }
        }
        _ => (engine.message.clone(), false, false, String::new()),
    };

    // Safety: strip newlines so the command line never exceeds one row.
    // A message that *starts* with a blank line (Neovim's `:digraphs`, no
    // bang, deliberately leads with one to mirror `listdigraphs` —
    // `format_digraph_table`, #1302) would otherwise make `.lines().next()`
    // yield `""` and paint nothing at all, even though the message holds
    // real content on its second line — so skip past any leading empty
    // lines first and show the first line that actually has something in
    // it. `trim_start_matches` only strips a *leading* run, so a message
    // that never had a blank line (every other caller) is unaffected.
    let text = text
        .trim_start_matches('\n')
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();

    CommandLineData {
        text,
        right_align,
        show_cursor,
        cursor_anchor_text,
    }
}
