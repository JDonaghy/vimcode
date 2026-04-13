use super::*;

pub(super) fn render_activity_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &TuiSidebar,
    theme: &Theme,
    _menu_bar_visible: bool,
    engine: &Engine,
) {
    let bar_bg = rc(theme.tab_bar_bg);
    let icon_fg = rc(theme.activity_bar_fg);
    let accent_fg = rc(theme.cursor); // left-edge accent bar for active panel
    let toolbar_sel_bg = rc(theme.cursor); // highlight for toolbar-focused selection

    // Fill entire activity bar background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', icon_fg, bar_bg);
        }
    }

    // Row 0: Hamburger icon (menu bar toggle)
    if area.height >= 1 {
        let y = area.y;
        let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == 0;
        let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
        let fg = icon_fg;
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, crate::icons::HAMBURGER.c(), fg, row_bg);
        }
    }

    // Top buttons: Explorer (1), Search (2), Debug (3), Git (4), Extensions (5), AI (6)
    let top_buttons: &[(u16, TuiPanel, char)] = &[
        (1, TuiPanel::Explorer, crate::icons::EXPLORER.c()),
        (2, TuiPanel::Search, crate::icons::SEARCH.c()),
        (3, TuiPanel::Debug, crate::icons::DEBUG.c()),
        (4, TuiPanel::Git, crate::icons::GIT_BRANCH.c()),
        (5, TuiPanel::Extensions, crate::icons::EXTENSIONS.c()),
        (6, TuiPanel::Ai, crate::icons::AI_CHAT.c()),
    ];

    for &(row_off, panel, icon) in top_buttons {
        let y = area.y + row_off;
        if y >= area.y + area.height {
            break;
        }
        let is_active = sidebar.visible && sidebar.active_panel == panel;
        let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == row_off;
        let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
        let fg = icon_fg;
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, icon, fg, row_bg);
        }
        if is_active && !is_kbd_sel {
            // Left accent bar for active panel
            set_cell(buf, area.x, y, '▎', accent_fg, bar_bg);
        }
    }

    // Extension panel icons (after the fixed 6 panels, starting at row 7)
    {
        let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
        ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
        for (i, panel) in ext_panels.iter().enumerate() {
            let row_off = 7 + i as u16;
            let y = area.y + row_off;
            if y >= area.y + area.height.saturating_sub(1) {
                break; // leave room for settings at bottom
            }
            let is_active =
                sidebar.ext_panel_name.as_deref() == Some(&panel.name) && sidebar.visible;
            let toolbar_idx = 8 + i as u16; // 0=hamburger, 1-6=panels, 7=settings, 8+=ext
            let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == toolbar_idx;
            let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
            let fg = icon_fg;
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', fg, row_bg);
            }
            if area.width >= 3 {
                set_cell(buf, area.x + 1, y, panel.resolved_icon(), fg, row_bg);
            }
            if is_active && !is_kbd_sel {
                set_cell(buf, area.x, y, '▎', accent_fg, bar_bg);
            }
        }
    }

    // Settings button pinned to the bottom row (like VSCode)
    if area.height >= 1 {
        let y = area.y + area.height - 1;
        let is_active = sidebar.visible && sidebar.active_panel == TuiPanel::Settings;
        let is_kbd_sel = sidebar.toolbar_focused && sidebar.toolbar_selected == 7;
        let row_bg = if is_kbd_sel { toolbar_sel_bg } else { bar_bg };
        let fg = icon_fg;
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }
        if area.width >= 3 {
            set_cell(buf, area.x + 1, y, crate::icons::SETTINGS.c(), fg, row_bg);
        }
        if is_active && !is_kbd_sel {
            set_cell(buf, area.x, y, '▎', accent_fg, bar_bg);
        }
    }
}

// ─── Sidebar rendering ────────────────────────────────────────────────────────

pub(super) fn render_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    theme: &Theme,
    explorer_drop_target: Option<usize>,
) {
    let default_fg = rc(theme.explorer_file_fg);
    let row_bg = rc(theme.tab_bar_bg);
    let active_bg = rc(theme.explorer_active_bg);

    // The single active buffer path (the file shown in the active window)
    let active_path: Option<PathBuf> = engine
        .file_path()
        .and_then(|p| p.canonicalize().ok().or_else(|| Some(p.clone())));
    let sel_bg = if sidebar.has_focus {
        rc(theme.sidebar_sel_bg)
    } else {
        rc(theme.sidebar_sel_bg_inactive)
    };

    // Extension panel (plugin-provided)
    if sidebar.ext_panel_name.is_some() {
        render_ext_panel(buf, area, engine, theme);
        return;
    }

    // Settings panel
    if sidebar.active_panel == TuiPanel::Settings {
        render_settings_panel(buf, area, theme, engine);
        return;
    }

    // Search panel
    if sidebar.active_panel == TuiPanel::Search {
        render_search_panel(buf, area, sidebar, engine, theme);
        return;
    }

    // Debug panel
    if sidebar.active_panel == TuiPanel::Debug {
        render_debug_sidebar(buf, area, engine, theme);
        return;
    }

    // Source Control panel
    if sidebar.active_panel == TuiPanel::Git {
        render_source_control(buf, area, engine, theme);
        return;
    }

    // Extensions panel
    if sidebar.active_panel == TuiPanel::Extensions {
        render_ext_sidebar(buf, area, engine, theme);
        return;
    }

    // AI assistant panel
    if sidebar.active_panel == TuiPanel::Ai {
        render_ai_sidebar(buf, area, engine, theme);
        return;
    }

    // ── Background fill — covers empty space below tree rows ────────────
    if area.height == 0 {
        return;
    }
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', default_fg, row_bg);
        }
    }

    // ── Explorer indicators (git status + diagnostics) ─────────────────
    let (git_statuses, diag_counts) = engine.explorer_indicators();
    let git_added_fg = rc(theme.git_added);
    let git_modified_fg = rc(theme.git_modified);
    let git_deleted_fg = rc(theme.git_deleted);
    let diag_error_fg = rc(theme.diagnostic_error);
    let diag_warning_fg = rc(theme.diagnostic_warning);

    // ── Tree rows ────────────────────────────────────────────────────────
    let tree_height = area.height as usize;

    // Determine where a new-entry row should be inserted (right after parent dir).
    // `new_entry_after_row` is the sidebar.rows index after which we inject the
    // virtual new-entry row.  `None` = no active new entry, or parent is root
    // (insert at index 0 visually, before all rows).
    let new_entry_insert = engine.explorer_new_entry.as_ref().map(|ne| {
        // Find the parent dir row index, or usize::MAX for "before all rows"
        sidebar
            .rows
            .iter()
            .position(|r| r.is_dir && r.path == ne.parent_dir)
    });
    // `true` if parent is root (no matching row — insert before first row)
    let new_entry_at_top = new_entry_insert == Some(None);
    let new_entry_after_idx = new_entry_insert.and_then(|opt| opt);

    // We manually iterate to interleave the virtual new-entry row.
    let mut visual_row = 0usize;
    let mut row_iter_idx = sidebar.scroll_top;
    // If new entry goes at top and scroll_top == 0, render it first
    let mut new_entry_rendered = engine.explorer_new_entry.is_none();

    // Handle new-entry-at-top: if scroll_top == 0, render the new entry first
    if new_entry_at_top && !new_entry_rendered && sidebar.scroll_top == 0 {
        let ne = engine.explorer_new_entry.as_ref().unwrap();
        let screen_y = area.y;
        // depth 0: parent is root, so child is at depth 0
        render_new_entry_row(buf, area, screen_y, ne, 0, theme);
        visual_row += 1;
        new_entry_rendered = true;
    }

    while visual_row < tree_height && row_iter_idx < sidebar.rows.len() {
        let row_idx = row_iter_idx;
        let row = &sidebar.rows[row_iter_idx];
        row_iter_idx += 1;

        let i = visual_row;
        let screen_y = area.y + i as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Fill row background
        for x in area.x..area.x + area.width {
            set_cell(buf, x, screen_y, ' ', default_fg, row_bg);
        }

        // Determine colours
        let is_selected = row_idx == sidebar.selected;
        let is_drop_target = explorer_drop_target == Some(row_idx);
        let is_active = !row.is_dir
            && !engine.explorer_has_focus
            && active_path.as_ref().is_some_and(|ap| {
                row.path.canonicalize().unwrap_or_else(|_| row.path.clone()) == *ap
            });

        let drop_bg = rc(render::Color {
            r: 40,
            g: 60,
            b: 80,
        }); // muted blue highlight
            // Determine name color: error > warning > git modified > default.
        let canon = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());
        let name_fg = if let Some(&(errors, warnings)) = diag_counts.get(&canon) {
            if errors > 0 {
                diag_error_fg
            } else if warnings > 0 {
                diag_warning_fg
            } else {
                default_fg
            }
        } else if let Some(&label) = git_statuses.get(&canon) {
            match label {
                'A' | '?' => git_added_fg,
                'D' => git_deleted_fg,
                _ => git_modified_fg,
            }
        } else {
            default_fg
        };

        let (fg, bg) = if is_drop_target {
            (name_fg, drop_bg)
        } else if is_selected {
            (name_fg, sel_bg)
        } else if is_active {
            (name_fg, active_bg)
        } else {
            (name_fg, row_bg)
        };

        let mut x = area.x;
        // Indent with subtle vertical guide lines (skip outermost levels)
        let guide_fg = rc(theme.line_number_fg);
        for level in 0..row.depth {
            if x >= area.x + area.width {
                break;
            }
            // Show guide lines (skip level 0 = root indent)
            if level > 0 {
                set_cell(buf, x, screen_y, '│', guide_fg, bg);
            } else {
                set_cell(buf, x, screen_y, ' ', fg, bg);
            }
            x += 1;
            // One space after guide = 2-col indent per level
            if x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', fg, bg);
                x += 1;
            }
        }
        // Layout: [chevron (2 cols)] [icon (2 cols)] [space] [name]
        // Dirs: ▾/▸ + space, then folder icon
        // Files: 2 spaces (no chevron), then file icon
        // This keeps icons aligned at the same column for siblings.
        if row.is_dir {
            let chevron = if row.is_expanded { '▾' } else { '▸' };
            if x < area.x + area.width {
                set_cell(buf, x, screen_y, chevron, fg, bg);
                x += 1;
            }
            if x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', fg, bg);
                x += 1;
            }
        } else {
            // No chevron — 2 blank cols to align with dirs
            for _ in 0..2 {
                if x < area.x + area.width {
                    set_cell(buf, x, screen_y, ' ', fg, bg);
                    x += 1;
                }
            }
        }
        // Icon (file or folder)
        let icon_str = if row.is_dir {
            crate::icons::FOLDER.s()
        } else {
            let ext = row.path.extension().and_then(|e| e.to_str()).unwrap_or("");
            crate::icons::file_icon(ext)
        };
        for ch in icon_str.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, fg, bg);
            x += 1;
        }
        // Space after icon
        if x < area.x + area.width {
            set_cell(buf, x, screen_y, ' ', fg, bg);
            x += 1;
        }
        // Name — or inline rename input when active on this row
        let is_renaming = engine
            .explorer_rename
            .as_ref()
            .is_some_and(|r| r.path == row.path);
        if is_renaming {
            let rename = engine.explorer_rename.as_ref().unwrap();
            let input_bg = rc(theme.background);
            let input_fg = rc(theme.foreground);
            let sel_bg = rc(theme.fuzzy_selected_bg);
            // Compute selection range (byte offsets)
            let (sel_lo, sel_hi) = rename
                .selection_anchor
                .map(|a| (a.min(rename.cursor), a.max(rename.cursor)))
                .unwrap_or((0, 0));
            let has_selection = sel_lo != sel_hi;
            // Available columns for the input text
            let avail = (area.x + area.width).saturating_sub(x) as usize;
            // Cursor char position (0-based)
            let cursor_char = rename.input[..rename.cursor].chars().count();
            let total_chars = rename.input.chars().count();
            // Compute horizontal scroll offset (in chars) to keep cursor visible.
            // Reserve 1 col for the cursor-at-end block.
            let scroll = if total_chars < avail || cursor_char < avail.saturating_sub(1) {
                0
            } else {
                cursor_char.saturating_sub(avail.saturating_sub(2))
            };
            // Render the input text starting from scroll offset
            for (char_idx, (byte_idx, ch)) in rename.input.char_indices().enumerate() {
                if char_idx < scroll {
                    continue;
                }
                if x >= area.x + area.width {
                    break;
                }
                let in_sel = has_selection && byte_idx >= sel_lo && byte_idx < sel_hi;
                let is_cursor = byte_idx == rename.cursor && !has_selection;
                let (cell_fg, cell_bg) = if is_cursor {
                    (input_bg, input_fg)
                } else if in_sel {
                    (input_fg, sel_bg)
                } else {
                    (input_fg, input_bg)
                };
                set_cell(buf, x, screen_y, ch, cell_fg, cell_bg);
                x += 1;
            }
            // Cursor at end of input (append position) — only when no selection
            if !has_selection && rename.cursor >= rename.input.len() && x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', input_bg, input_fg);
                x += 1;
            }
            // Fill remaining width with input background
            while x < area.x + area.width {
                set_cell(buf, x, screen_y, ' ', input_fg, input_bg);
                x += 1;
            }
        } else {
            for ch in row.name.chars() {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, screen_y, ch, fg, bg);
                x += 1;
            }

            // Right-aligned indicators: diagnostics then git status
            if !row.is_dir {
                let canon = row.path.canonicalize().unwrap_or_else(|_| row.path.clone());
                let right_edge = area.x + area.width - 1; // reserve 1 col for scrollbar

                // Build indicator string right-to-left
                let git_label = git_statuses.get(&canon).copied();
                let diag = diag_counts.get(&canon).copied();

                // Calculate how many cols we need
                let mut parts: Vec<(String, ratatui::style::Color)> = Vec::new();
                if let Some((errors, warnings)) = diag {
                    if errors > 0 {
                        let s = if errors > 9 {
                            "9+".to_string()
                        } else {
                            format!("{}", errors)
                        };
                        parts.push((s, diag_error_fg));
                    }
                    if warnings > 0 {
                        let s = if warnings > 9 {
                            "9+".to_string()
                        } else {
                            format!("{}", warnings)
                        };
                        parts.push((s, diag_warning_fg));
                    }
                }
                if let Some(label) = git_label {
                    let color = match label {
                        'A' | '?' => git_added_fg,
                        'D' => git_deleted_fg,
                        _ => git_modified_fg,
                    };
                    parts.push((format!("{}", label), color));
                }

                if !parts.is_empty() {
                    // Total width: parts joined by spaces
                    let total_width: u16 = parts.iter().map(|(s, _)| s.len() as u16).sum::<u16>()
                        + (parts.len() as u16).saturating_sub(1); // spaces between
                    let start_x = right_edge.saturating_sub(total_width);
                    if x + 1 < start_x {
                        let mut px = start_x;
                        for (idx, (text, color)) in parts.iter().enumerate() {
                            if idx > 0 {
                                set_cell(buf, px, screen_y, ' ', *color, bg);
                                px += 1;
                            }
                            for ch in text.chars() {
                                if px < right_edge {
                                    set_cell(buf, px, screen_y, ch, *color, bg);
                                    px += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        visual_row += 1;

        // Inject virtual new-entry row after the parent dir row
        if !new_entry_rendered {
            if let Some(after_idx) = new_entry_after_idx {
                if row_idx == after_idx && visual_row < tree_height {
                    let ne = engine.explorer_new_entry.as_ref().unwrap();
                    let parent_depth = row.depth;
                    let screen_y = area.y + visual_row as u16;
                    if screen_y < area.y + area.height {
                        render_new_entry_row(buf, area, screen_y, ne, parent_depth, theme);
                        visual_row += 1;
                    }
                    new_entry_rendered = true;
                }
            }
        }
    }

    // Vertical scrollbar (rightmost column, tree rows only — not header)
    let total_rows = sidebar.rows.len();
    let visible_rows_count = tree_height;
    if total_rows > visible_rows_count && area.width >= 2 {
        let track_fg = rc(theme.separator);
        let thumb_fg = rc(theme.status_fg);
        let sb_bg = rc(theme.tab_bar_bg);
        let track_h = visible_rows_count as f64;
        let thumb_size = ((visible_rows_count as f64 / total_rows as f64) * track_h)
            .ceil()
            .max(1.0) as u16;
        let thumb_top = ((sidebar.scroll_top as f64 / total_rows as f64) * track_h).floor() as u16;
        let sb_x = area.x + area.width - 1;
        for dy in 0..visible_rows_count as u16 {
            let y = area.y + dy;
            if y >= area.y + area.height {
                break;
            }
            let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
            let ch = if in_thumb { '█' } else { '░' };
            let fg = if in_thumb { thumb_fg } else { track_fg };
            set_cell(buf, sb_x, y, ch, fg, sb_bg);
        }
    }
}

/// Render the inline new-file/folder entry row in the explorer tree.
fn render_new_entry_row(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    screen_y: u16,
    entry: &crate::core::engine::ExplorerNewEntryState,
    depth: usize,
    theme: &Theme,
) {
    let input_bg = rc(theme.background);
    let input_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let row_bg = rc(theme.tab_bar_bg);

    // Clear row
    for x in area.x..area.x + area.width {
        set_cell(buf, x, screen_y, ' ', input_fg, row_bg);
    }

    let mut x = area.x;

    // Indent (child of parent, so depth + 1) — 2-col per level
    let indent = "  ".repeat(depth + 1);
    for ch in indent.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, screen_y, ch, dim_fg, row_bg);
        x += 1;
    }

    // Icon prefix
    let icon_str = if entry.is_folder {
        "\u{f07b} " // folder icon
    } else {
        "  \u{f15b} " // file icon with spacing
    };
    for ch in icon_str.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, screen_y, ch, dim_fg, row_bg);
        x += 1;
    }

    // Editable input with inverted cursor — scroll if needed
    let avail = (area.x + area.width).saturating_sub(x) as usize;
    let cursor_char = entry.input[..entry.cursor].chars().count();
    let total_chars = entry.input.chars().count();
    let scroll = if total_chars < avail || cursor_char < avail.saturating_sub(1) {
        0
    } else {
        cursor_char.saturating_sub(avail.saturating_sub(2))
    };
    for (char_idx, (byte_idx, ch)) in entry.input.char_indices().enumerate() {
        if char_idx < scroll {
            continue;
        }
        if x >= area.x + area.width {
            break;
        }
        let is_cursor = byte_idx == entry.cursor;
        let cell_fg = if is_cursor { input_bg } else { input_fg };
        let cell_bg = if is_cursor { input_fg } else { input_bg };
        set_cell(buf, x, screen_y, ch, cell_fg, cell_bg);
        x += 1;
    }
    // Cursor at end of input (append position)
    if entry.cursor >= entry.input.len() && x < area.x + area.width {
        set_cell(buf, x, screen_y, ' ', input_bg, input_fg);
        x += 1;
    }
    // Fill remaining width with input background
    while x < area.x + area.width {
        set_cell(buf, x, screen_y, ' ', input_fg, input_bg);
        x += 1;
    }
}

/// Render the settings panel — shows current key settings and the file path.
pub(super) fn render_settings_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    theme: &Theme,
    engine: &Engine,
) {
    use crate::core::settings::{setting_categories, SettingType, SETTING_DEFS};

    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let fg = rc(theme.foreground);
    let bg = rc(theme.tab_bar_bg);
    let dim_fg = rc(theme.line_number_fg);
    let key_fg = rc(theme.keyword);
    let sel_bg = if engine.settings_has_focus {
        rc(theme.sidebar_sel_bg)
    } else {
        rc(theme.sidebar_sel_bg_inactive)
    };
    let cat_fg = rc(theme.keyword);

    if area.height == 0 {
        return;
    }

    // Fill background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }
    }

    // Row 0: Header " SETTINGS"
    let header_y = area.y;
    for x in area.x..area.x + area.width {
        set_cell(buf, x, header_y, ' ', header_fg, header_bg);
    }
    let mut x = area.x;
    for ch in " SETTINGS".chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, header_y, ch, header_fg, header_bg);
        x += 1;
    }

    // Row 1: Search input
    let search_y = area.y + 1;
    if search_y < area.y + area.height {
        let search_bg = if engine.settings_input_active {
            rc(theme.sidebar_sel_bg)
        } else {
            bg
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, search_y, ' ', fg, search_bg);
        }
        let mut x = area.x;
        set_cell(buf, x, search_y, ' ', dim_fg, search_bg);
        x += 1;
        set_cell(buf, x, search_y, '/', dim_fg, search_bg);
        x += 1;
        set_cell(buf, x, search_y, ' ', dim_fg, search_bg);
        x += 1;
        for ch in engine.settings_query.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, search_y, ch, fg, search_bg);
            x += 1;
        }
        if engine.settings_input_active && x < area.x + area.width {
            set_cell(buf, x, search_y, '█', fg, search_bg);
        }
    }

    // Rows 2+: scrollable form content
    let content_start = area.y + 2;
    let content_height = area.height.saturating_sub(2) as usize;
    if content_height == 0 {
        return;
    }

    let flat = engine.settings_flat_list();
    let cats = setting_categories();
    let total = flat.len();

    // Scrollbar column is the rightmost
    let sb_col = area.x + area.width - 1;
    let content_width = area.width.saturating_sub(1); // leave room for scrollbar

    let scroll = engine.settings_scroll_top;

    for vi in 0..content_height {
        let fi = scroll + vi;
        let y = content_start + vi as u16;
        if fi >= total {
            break;
        }

        use crate::core::engine::SettingsRow;
        let row = &flat[fi];
        let is_selected = fi == engine.settings_selected && engine.settings_has_focus;
        let row_bg = if is_selected { sel_bg } else { bg };

        // Fill row background
        for x in area.x..area.x + content_width {
            set_cell(buf, x, y, ' ', fg, row_bg);
        }

        let right_edge = area.x + content_width;

        match row {
            SettingsRow::CoreCategory(cat_idx) => {
                let collapsed = *cat_idx < engine.settings_collapsed.len()
                    && engine.settings_collapsed[*cat_idx];
                let arrow = if collapsed { '▶' } else { '▼' };
                let cat_name = if *cat_idx < cats.len() {
                    cats[*cat_idx]
                } else {
                    "?"
                };
                let mut x = area.x + 1;
                set_cell(buf, x, y, arrow, cat_fg, row_bg);
                x += 2;
                for ch in cat_name.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, cat_fg, row_bg);
                    x += 1;
                }
            }
            SettingsRow::ExtCategory(name) => {
                let collapsed = engine
                    .ext_settings_collapsed
                    .get(name)
                    .copied()
                    .unwrap_or(false);
                let arrow = if collapsed { '▶' } else { '▼' };
                // Use display_name if available, otherwise capitalize name
                let display = engine
                    .ext_available_manifests()
                    .into_iter()
                    .find(|m| &m.name == name)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_else(|| name.clone());
                let mut x = area.x + 1;
                set_cell(buf, x, y, arrow, cat_fg, row_bg);
                x += 2;
                for ch in display.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, cat_fg, row_bg);
                    x += 1;
                }
            }
            SettingsRow::CoreSetting(idx) => {
                let def = &SETTING_DEFS[*idx];
                let mut x = area.x + 3;
                for ch in def.label.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, fg, row_bg);
                    x += 1;
                }

                let editing_this = engine.settings_editing == Some(*idx);

                match &def.setting_type {
                    SettingType::Bool => {
                        let val = engine.settings.get_value_str(def.key);
                        let display = if val == "true" { "[✓]" } else { "[ ]" };
                        let val_len = 3u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx;
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::Integer { .. } => {
                        let display = if editing_this {
                            format!("{}█", engine.settings_edit_buf)
                        } else {
                            engine.settings.get_value_str(def.key)
                        };
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::Enum(_) | SettingType::DynamicEnum(_) => {
                        let val = engine.settings.get_value_str(def.key);
                        let display = format!("{val} ▸");
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::StringVal => {
                        let display = if editing_this {
                            format!("{}█", engine.settings_edit_buf)
                        } else {
                            let val = engine.settings.get_value_str(def.key);
                            if val.is_empty() {
                                "(empty)".to_string()
                            } else {
                                val
                            }
                        };
                        let max_val_width = content_width.saturating_sub(x - area.x + 2) as usize;
                        let truncated: String = display.chars().take(max_val_width).collect();
                        let val_len = truncated.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        let val_fg = if editing_this { fg } else { dim_fg };
                        for ch in truncated.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, val_fg, row_bg);
                            cx += 1;
                        }
                    }
                    SettingType::BufferEditor => {
                        let display = match def.key {
                            "keymaps" => {
                                format!("{} defined ▸", engine.settings.keymaps.len())
                            }
                            "extension_registries" => {
                                format!(
                                    "{} configured ▸",
                                    engine.settings.extension_registries.len()
                                )
                            }
                            _ => "▸".to_string(),
                        };
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                }
            }
            SettingsRow::ExtSetting(ext_name, ext_key) => {
                // Extension setting — render like core settings
                let def = engine.find_ext_setting_def(ext_name, ext_key);
                let label = def.as_ref().map(|d| d.label.as_str()).unwrap_or(ext_key);
                let mut x = area.x + 3;
                for ch in label.chars() {
                    if x >= area.x + content_width {
                        break;
                    }
                    set_cell(buf, x, y, ch, fg, row_bg);
                    x += 1;
                }

                let editing_this = engine
                    .ext_settings_editing
                    .as_ref()
                    .is_some_and(|(en, ek)| en == ext_name && ek == ext_key);
                let val = engine.get_ext_setting(ext_name, ext_key);
                let typ = def.as_ref().map(|d| d.r#type.as_str()).unwrap_or("string");

                match typ {
                    "bool" => {
                        let display = if val == "true" { "[✓]" } else { "[ ]" };
                        let val_len = 3u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx;
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    "enum" => {
                        let display = format!("{val} ▸");
                        let val_len = display.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        for ch in display.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, key_fg, row_bg);
                            cx += 1;
                        }
                    }
                    _ => {
                        // string/integer
                        let display = if editing_this {
                            format!("{}█", engine.settings_edit_buf)
                        } else if val.is_empty() {
                            "(empty)".to_string()
                        } else {
                            val
                        };
                        let max_val_width = content_width.saturating_sub(x - area.x + 2) as usize;
                        let truncated: String = display.chars().take(max_val_width).collect();
                        let val_len = truncated.chars().count() as u16;
                        let vx = right_edge.saturating_sub(val_len + 1);
                        let mut cx = vx.max(x);
                        let val_fg = if editing_this { fg } else { dim_fg };
                        for ch in truncated.chars() {
                            if cx >= right_edge {
                                break;
                            }
                            set_cell(buf, cx, y, ch, val_fg, row_bg);
                            cx += 1;
                        }
                    }
                }
            }
        }
    }

    // Scrollbar
    if total > content_height && content_height > 0 {
        let track_len = content_height;
        let thumb_len = (content_height * content_height / total).max(1);
        let thumb_start = scroll * track_len / total;
        for i in 0..track_len {
            let y = content_start + i as u16;
            let ch = if i >= thumb_start && i < thumb_start + thumb_len {
                '█'
            } else {
                '░'
            };
            set_cell(buf, sb_col, y, ch, dim_fg, bg);
        }
    }
}

/// Return the visual display row (0-based, including file-header rows) for a result index.
pub(super) fn result_idx_to_display_row(
    results: &[crate::core::ProjectMatch],
    target_idx: usize,
) -> usize {
    let mut row = 0usize;
    let mut last_file: Option<&std::path::Path> = None;
    for (idx, m) in results.iter().enumerate() {
        if last_file != Some(m.file.as_path()) {
            last_file = Some(m.file.as_path());
            row += 1; // file-header row
        }
        if idx == target_idx {
            return row;
        }
        row += 1;
    }
    0
}

/// Adjust `search_scroll_top` so that `selected_idx` is within the viewport.
/// Call this after changing the selection via keyboard — not during render.
pub(super) fn ensure_search_selection_visible(
    results: &[crate::core::ProjectMatch],
    selected_idx: usize,
    scroll_top: &mut usize,
    results_height: usize,
) {
    if results.is_empty() || results_height == 0 {
        return;
    }
    let display_row = result_idx_to_display_row(results, selected_idx);
    if display_row < *scroll_top {
        *scroll_top = display_row;
    } else if display_row >= *scroll_top + results_height {
        *scroll_top = display_row + 1 - results_height;
    }
}

/// Map a visual row index (0-based from top of results area) to a `project_search_results` index.
///
/// The results area interleaves file-header rows (not selectable) with result rows.
/// Returns `None` if the row falls on a file header.
pub(super) fn visual_row_to_result_idx(
    results: &[crate::core::ProjectMatch],
    visual_row: usize,
) -> Option<usize> {
    let mut row = 0usize;
    let mut last_file: Option<&std::path::Path> = None;
    for (idx, m) in results.iter().enumerate() {
        if last_file != Some(m.file.as_path()) {
            last_file = Some(m.file.as_path());
            if row == visual_row {
                return None; // file header row
            }
            row += 1;
        }
        if row == visual_row {
            return Some(idx);
        }
        row += 1;
    }
    None
}

/// Render the project search panel.
pub(super) fn render_search_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    theme: &Theme,
) {
    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let fg = rc(theme.foreground);
    let bg = rc(theme.tab_bar_bg);
    let dim_fg = rc(theme.line_number_fg);
    let sel_fg = bg;
    let sel_bg = fg;
    let file_header_fg = rc(theme.keyword);

    if area.height == 0 {
        return;
    }

    // Fill background
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }
    }

    // Row 0: panel header " SEARCH"
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', header_fg, header_bg);
    }
    let mut x = area.x;
    for ch in " SEARCH".chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, area.y, ch, header_fg, header_bg);
        x += 1;
    }

    if area.height < 2 {
        return;
    }

    // Row 1: search input box  "[ query___ ]"
    let input_y = area.y + 1;
    let query = &engine.project_search_query;
    let input_bg = rc(theme.active_background);
    let input_fg = fg;
    // Draw bracket prefix
    set_cell(buf, area.x, input_y, '[', dim_fg, bg);
    let end_bracket_x = if area.width > 1 {
        area.x + area.width - 1
    } else {
        area.x
    };
    set_cell(buf, end_bracket_x, input_y, ']', dim_fg, bg);
    // Fill input background
    for x in (area.x + 1)..end_bracket_x {
        set_cell(buf, x, input_y, ' ', input_fg, input_bg);
    }
    // Render query text
    let mut x = area.x + 1;
    for ch in query.chars() {
        if x >= end_bracket_x {
            break;
        }
        set_cell(buf, x, input_y, ch, input_fg, input_bg);
        x += 1;
    }
    // Cursor blinking indicator: show │ at cursor position when in input mode
    if sidebar.search_input_mode && !sidebar.replace_input_focused && x < end_bracket_x {
        set_cell(buf, x, input_y, '\u{258f}', rc(theme.cursor), input_bg); // ▏
    }

    if area.height < 3 {
        return;
    }

    // Row 2: replace input box  "[ replace_ ]"
    let replace_y = area.y + 2;
    let replace_text = &engine.project_replace_text;
    let replace_bg = if sidebar.replace_input_focused && sidebar.search_input_mode {
        input_bg
    } else {
        rc(theme.tab_bar_bg) // dimmer when unfocused
    };
    set_cell(buf, area.x, replace_y, '[', dim_fg, bg);
    let rep_end_x = if area.width > 1 {
        area.x + area.width - 1
    } else {
        area.x
    };
    set_cell(buf, rep_end_x, replace_y, ']', dim_fg, bg);
    for x in (area.x + 1)..rep_end_x {
        set_cell(buf, x, replace_y, ' ', input_fg, replace_bg);
    }
    // Placeholder or actual text
    if replace_text.is_empty() && !(sidebar.replace_input_focused && sidebar.search_input_mode) {
        let placeholder = "Replace…";
        let mut x = area.x + 1;
        for ch in placeholder.chars() {
            if x >= rep_end_x {
                break;
            }
            set_cell(buf, x, replace_y, ch, dim_fg, replace_bg);
            x += 1;
        }
    } else {
        let mut x = area.x + 1;
        for ch in replace_text.chars() {
            if x >= rep_end_x {
                break;
            }
            set_cell(buf, x, replace_y, ch, input_fg, replace_bg);
            x += 1;
        }
        if sidebar.replace_input_focused && sidebar.search_input_mode && x < rep_end_x {
            set_cell(buf, x, replace_y, '\u{258f}', rc(theme.cursor), replace_bg);
        }
    }

    if area.height < 4 {
        return;
    }

    // Row 3: toggle indicators (Aa / Ab| / .* ) + hint
    let toggle_y = area.y + 3;
    for x in area.x..area.x + area.width {
        set_cell(buf, x, toggle_y, ' ', dim_fg, bg);
    }
    {
        let opts = &engine.project_search_options;
        let active_fg = rc(theme.keyword);
        let mut tx = area.x;

        // Helper: render a label with active/inactive coloring
        let draw_toggle =
            |buf: &mut ratatui::buffer::Buffer, label: &str, active: bool, x: &mut u16| {
                let color = if active { active_fg } else { dim_fg };
                for ch in label.chars() {
                    if *x >= area.x + area.width {
                        break;
                    }
                    set_cell(buf, *x, toggle_y, ch, color, bg);
                    *x += 1;
                }
                // Space separator
                if *x < area.x + area.width {
                    set_cell(buf, *x, toggle_y, ' ', dim_fg, bg);
                    *x += 1;
                }
            };

        draw_toggle(buf, "Aa", opts.case_sensitive, &mut tx);
        draw_toggle(buf, "Ab|", opts.whole_word, &mut tx);
        draw_toggle(buf, ".*", opts.use_regex, &mut tx);

        // Hint text
        let hint = "Alt+C/W/R/H";
        if tx + 1 < area.x + area.width {
            // Small gap
            tx += 1;
            for ch in hint.chars() {
                if tx >= area.x + area.width {
                    break;
                }
                set_cell(buf, tx, toggle_y, ch, dim_fg, bg);
                tx += 1;
            }
        }
    }

    if area.height < 5 {
        return;
    }

    // Row 4: status / hint line
    let status_y = area.y + 4;
    let status_text = if engine.project_search_results.is_empty() {
        if query.is_empty() {
            " Type to search, Enter to run"
        } else {
            &engine.message
        }
    } else {
        &engine.message
    };
    // We borrow status_text potentially as &engine.message which is a &str reference,
    // so we just render it directly.
    let mut x = area.x;
    for ch in status_text.chars() {
        if x >= area.x + area.width {
            break;
        }
        set_cell(buf, x, status_y, ch, dim_fg, bg);
        x += 1;
    }

    if area.height < 6 {
        return;
    }

    // Rows 5+: results
    let results = &engine.project_search_results;
    if results.is_empty() {
        return;
    }

    let results_start_y = area.y + 5;
    let results_height = area.height.saturating_sub(5) as usize;

    // Build the flat display list (file headers + result rows)
    struct DisplayRow {
        text: String,
        is_header: bool,
        result_idx: Option<usize>,
    }

    let mut display_rows: Vec<DisplayRow> = Vec::new();
    let root = &sidebar.root;
    let mut last_file: Option<&std::path::Path> = None;

    for (idx, m) in results.iter().enumerate() {
        if last_file != Some(m.file.as_path()) {
            last_file = Some(m.file.as_path());
            let rel = m.file.strip_prefix(root).unwrap_or(&m.file);
            display_rows.push(DisplayRow {
                text: rel.display().to_string(),
                is_header: true,
                result_idx: None,
            });
        }
        let snippet = format!("  {}: {}", m.line + 1, m.line_text.trim());
        display_rows.push(DisplayRow {
            text: snippet,
            is_header: false,
            result_idx: Some(idx),
        });
    }

    let total_display = display_rows.len();
    let max_scroll = total_display.saturating_sub(results_height);

    // Viewport scrolls freely — only clamped to valid range.
    // Selection-tracking happens in the keyboard / poll handlers, not here.
    let scroll_top = sidebar.search_scroll_top.min(max_scroll);
    sidebar.search_scroll_top = scroll_top;

    for (i, dr) in display_rows
        .iter()
        .skip(scroll_top)
        .take(results_height)
        .enumerate()
    {
        let screen_y = results_start_y + i as u16;
        if screen_y >= area.y + area.height {
            break;
        }

        // Fill row background first
        for x in area.x..area.x + area.width {
            set_cell(buf, x, screen_y, ' ', fg, bg);
        }

        let is_selected = !dr.is_header
            && dr.result_idx == Some(engine.project_search_selected)
            && !sidebar.search_input_mode;

        let (row_fg, row_bg) = if is_selected {
            (sel_fg, sel_bg)
        } else if dr.is_header {
            (file_header_fg, bg)
        } else {
            (fg, bg)
        };

        // Re-fill with correct bg for selected rows
        if is_selected || dr.is_header {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, screen_y, ' ', row_fg, row_bg);
            }
        }

        let mut x = area.x;
        for ch in dr.text.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, screen_y, ch, row_fg, row_bg);
            x += 1;
        }
    }

    // Vertical scrollbar for results area
    let total_display = display_rows.len();
    if total_display > results_height && area.width >= 2 {
        let track_fg = rc(theme.separator);
        let thumb_fg = rc(theme.status_fg);
        let sb_bg = bg;
        let track_h = results_height as f64;
        let thumb_size = ((results_height as f64 / total_display as f64) * track_h)
            .ceil()
            .max(1.0) as u16;
        let thumb_top = ((scroll_top as f64 / total_display as f64) * track_h).floor() as u16;
        let sb_x = area.x + area.width - 1;
        for dy in 0..results_height as u16 {
            let y = results_start_y + dy;
            if y >= area.y + area.height {
                break;
            }
            let in_thumb = dy >= thumb_top && dy < thumb_top + thumb_size;
            let ch = if in_thumb { '█' } else { '░' };
            let fg_color = if in_thumb { thumb_fg } else { track_fg };
            set_cell(buf, sb_x, y, ch, fg_color, sb_bg);
        }
    }
}

// ─── Wildmenu (command Tab completion bar) ───────────────────────────────────

pub(super) fn render_wildmenu(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    wm: &WildmenuData,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let bg = rc(theme.wildmenu_bg);
    let fg = rc(theme.wildmenu_fg);
    let sel_bg = rc(theme.wildmenu_sel_bg);
    let sel_fg = rc(theme.wildmenu_sel_fg);

    // Fill background
    for x in area.x..area.x + area.width {
        let cell = &mut buf[(x, area.y)];
        cell.set_char(' ').set_fg(fg).set_bg(bg);
    }

    // Draw items separated by spaces
    let mut col = area.x;
    for (i, item) in wm.items.iter().enumerate() {
        if col >= area.x + area.width {
            break;
        }
        let is_selected = wm.selected == Some(i);
        let item_fg = if is_selected { sel_fg } else { fg };
        let item_bg = if is_selected { sel_bg } else { bg };

        // Leading space
        if col < area.x + area.width {
            buf[(col, area.y)]
                .set_char(' ')
                .set_fg(item_fg)
                .set_bg(item_bg);
            col += 1;
        }

        for ch in item.chars() {
            if col >= area.x + area.width {
                break;
            }
            buf[(col, area.y)]
                .set_char(ch)
                .set_fg(item_fg)
                .set_bg(item_bg);
            col += 1;
        }

        // Trailing space for selected item padding
        if is_selected && col < area.x + area.width {
            buf[(col, area.y)]
                .set_char(' ')
                .set_fg(item_fg)
                .set_bg(item_bg);
            col += 1;
        }
    }
}

// ─── Status / command line ────────────────────────────────────────────────────

pub(super) fn render_status_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    left: &str,
    right: &str,
    theme: &Theme,
) {
    let fg = rc(theme.status_fg);
    let bg = rc(theme.status_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }

    let right_chars: Vec<char> = right.chars().collect();
    let right_len = right_chars.len() as u16;
    let right_start = if right_len <= area.width {
        area.x + area.width - right_len
    } else {
        area.x + area.width
    };

    // Draw left text, stopping 1 col before right text to avoid overlap.
    let left_limit = if right_start > area.x {
        right_start - 1
    } else {
        area.x
    };
    let mut x = area.x;
    for ch in left.chars() {
        if x >= left_limit {
            break;
        }
        set_cell(buf, x, area.y, ch, fg, bg);
        x += 1;
    }

    // Draw right text, right-aligned.
    if right_len <= area.width {
        let mut rx = right_start;
        for &ch in &right_chars {
            if rx >= area.x + area.width {
                break;
            }
            set_cell(buf, rx, area.y, ch, fg, bg);
            rx += 1;
        }
    }
}

pub(super) fn render_command_line(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    command: &render::CommandLineData,
    theme: &Theme,
) {
    let fg = rc(theme.command_fg);
    let bg = rc(theme.command_bg);

    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', fg, bg);
    }

    if command.right_align {
        let chars: Vec<char> = command.text.chars().collect();
        let len = chars.len() as u16;
        if len <= area.width {
            let mut x = area.x + area.width - len;
            for &ch in &chars {
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, area.y, ch, fg, bg);
                x += 1;
            }
        }
    } else {
        let mut x = area.x;
        for ch in command.text.chars() {
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, area.y, ch, fg, bg);
            x += 1;
        }
    }

    // Command-line cursor (inverted block at insertion point)
    if command.show_cursor {
        let cursor_col = command.cursor_anchor_text.chars().count() as u16;
        let cx = area.x + cursor_col.min(area.width.saturating_sub(1));
        let buf_area = buf.area;
        if cx < buf_area.x + buf_area.width {
            let cell = &mut buf[(cx, area.y)];
            let old_fg = cell.fg;
            let old_bg = cell.bg;
            cell.set_fg(old_bg).set_bg(old_fg);
        }
    }
}

// ─── Input translation ────────────────────────────────────────────────────────

pub(super) fn render_source_control(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    // Clear the entire area first to prevent stale content from previous renders.
    {
        let clear_fg = rc(theme.foreground);
        let clear_bg = rc(theme.tab_bar_bg);
        for cy in area.y..area.y + area.height {
            for cx in area.x..area.x + area.width {
                set_cell(buf, cx, cy, ' ', clear_fg, clear_bg);
            }
        }
    }
    let item_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.tab_bar_bg);
    let add_fg = rc(theme.git_added);
    let del_fg = rc(theme.git_deleted);
    let mod_fg = rc(theme.git_modified);

    // Build SC data from engine state via the render abstraction.
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref sc) = screen.source_control else {
        return;
    };

    // Reserve bottom row for hint bar when focused.
    let area = if sc.has_focus && area.height > 2 {
        let hint_y = area.y + area.height - 1;
        let hint_text = " Press '?' for help";
        for cx in area.x..area.x + area.width {
            set_cell(buf, cx, hint_y, ' ', dim_fg, hdr_bg);
        }
        for (i, ch) in hint_text.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, hint_y, ch, dim_fg, hdr_bg);
        }
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height - 1,
        }
    } else {
        area
    };

    // ── Row 0: header "SOURCE CONTROL" ──────────────────────────────────────
    let branch_info = if sc.ahead > 0 || sc.behind > 0 {
        format!(
            "  \u{e702} SOURCE CONTROL  {}  \u{2191}{} \u{2193}{}",
            sc.branch, sc.ahead, sc.behind
        )
    } else {
        format!("  \u{e702} SOURCE CONTROL  {}", sc.branch)
    };
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in branch_info.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // ── Row 1+: commit input row(s) ──────────────────────────────────────────
    let commit_lines: Vec<&str> = sc.commit_message.split('\n').collect();
    let commit_rows = commit_lines.len().max(1) as u16;
    {
        let inp_bg = if sc.commit_input_active {
            sel_bg
        } else {
            row_bg
        };
        let prompt_fg = if sc.commit_input_active {
            item_fg
        } else {
            dim_fg
        };

        // Compute cursor line/col for active input.
        let (cursor_line, cursor_col) = if sc.commit_input_active {
            let before_cursor = &sc.commit_message[..sc.commit_cursor.min(sc.commit_message.len())];
            let cl = before_cursor.matches('\n').count();
            let line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
            (cl, before_cursor[line_start..].chars().count())
        } else {
            (0, 0)
        };
        let prefix = " \u{f044}  ";
        let pad = "    "; // 4 spaces — same visual width as prefix

        if sc.commit_message.is_empty() && !sc.commit_input_active {
            let commit_y = area.y + 1;
            let prompt = format!("{}Message (press c)", prefix);
            for x in area.x..area.x + area.width {
                set_cell(buf, x, commit_y, ' ', prompt_fg, inp_bg);
            }
            for (i, ch) in prompt.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, commit_y, ch, prompt_fg, inp_bg);
            }
        } else {
            for (line_idx, line) in commit_lines.iter().enumerate() {
                let commit_y = area.y + 1 + line_idx as u16;
                if commit_y >= area.y + area.height {
                    break;
                }
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, commit_y, ' ', prompt_fg, inp_bg);
                }
                let pfx = if line_idx == 0 { prefix } else { pad };
                let text = format!("{}{}", pfx, line);
                let pfx_len = pfx.chars().count();
                for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                    // Show cursor by inverting fg/bg at cursor position.
                    let (fg, bg) = if sc.commit_input_active
                        && line_idx == cursor_line
                        && i == pfx_len + cursor_col
                    {
                        (inp_bg, prompt_fg)
                    } else {
                        (prompt_fg, inp_bg)
                    };
                    set_cell(buf, area.x + i as u16, commit_y, ch, fg, bg);
                }
                // If cursor is at end of line, show inverted space after text.
                if sc.commit_input_active
                    && line_idx == cursor_line
                    && cursor_col >= line.chars().count()
                {
                    let cx = area.x + (pfx_len + cursor_col) as u16;
                    if cx < area.x + area.width {
                        set_cell(buf, cx, commit_y, ' ', inp_bg, prompt_fg);
                    }
                }
            }
        }
    }

    if area.height < 2 + commit_rows + 3 {
        return;
    }

    // ── Button row (after commit input, with 1 row padding above and below) ──
    {
        let pad_above = area.y + 1 + commit_rows;
        let btn_y = pad_above + 1;
        let pad_below = btn_y + 1;

        // Clear padding rows.
        for px in area.x..area.x + area.width {
            set_cell(buf, px, pad_above, ' ', dim_fg, row_bg);
            set_cell(buf, px, pad_below, ' ', dim_fg, row_bg);
        }

        // Button background — use a distinct bg so they look like buttons.
        let btn_bg = hdr_bg; // slightly lighter than panel_bg
        let hover_bg = match hdr_bg {
            RColor::Rgb(r, g, b) => RColor::Rgb(
                r.saturating_add(20),
                g.saturating_add(20),
                b.saturating_add(20),
            ),
            other => other,
        };

        // Commit gets ~50% of the width (with label text).
        // Push / Pull / Sync get equal shares of the remaining width, icon only.
        let commit_w = (area.width / 2).max(1);
        let remain = area.width.saturating_sub(commit_w);
        let icon_w = (remain / 3).max(1);

        // (x_offset_from_area_x, segment_width, display_text, button_index)
        let buttons: [(u16, u16, &str, usize); 4] = [
            (0, commit_w, " \u{e729} Commit", 0),
            (commit_w, icon_w, " \u{f093}", 1),
            (commit_w + icon_w, icon_w, " \u{f019}", 2),
            (
                commit_w + icon_w * 2,
                area.width.saturating_sub(commit_w + icon_w * 2),
                " \u{f021}",
                3,
            ),
        ];
        for (x_off, seg_w, text, btn_idx) in &buttons {
            let bx = area.x + x_off;
            let seg_end = if *btn_idx == 3 {
                area.x + area.width
            } else {
                (bx + seg_w).min(area.x + area.width)
            };
            let is_focused = sc.button_focused == Some(*btn_idx);
            let is_hovered = sc.button_hovered == Some(*btn_idx);
            let (fg, bg) = if is_focused {
                (hdr_bg, hdr_fg) // inverted = highlighted
            } else if is_hovered {
                (hdr_fg, hover_bg)
            } else {
                (hdr_fg, btn_bg)
            };
            for px in bx..seg_end {
                set_cell(buf, px, btn_y, ' ', fg, bg);
            }
            for (j, ch) in text.chars().enumerate() {
                let cx = bx + j as u16;
                if cx < seg_end {
                    set_cell(buf, cx, btn_y, ch, fg, bg);
                }
            }
        }
    }

    let section_start_y = area.y + 4 + commit_rows; // +2 for padding rows, +1 for btn row, +1 for next
    if section_start_y >= area.y + area.height {
        return;
    }

    // Sections: staged, unstaged, and optionally worktrees (only when linked worktrees exist).
    #[allow(clippy::type_complexity)]
    let sections: [(
        &str,
        &[render::ScFileItem],
        Option<&[render::ScWorktreeItem]>,
        usize,
    ); 3] = [
        ("\u{f055} STAGED CHANGES", &sc.staged, None, 0),
        ("\u{f02b} CHANGES", &sc.unstaged, None, 1),
        ("\u{e702} WORKTREES", &[], Some(&sc.worktrees), 2),
    ];
    // Only show WORKTREES section when there are linked worktrees (>1 total).
    let show_worktrees = sc.worktrees.len() > 1;

    let mut row_y = section_start_y; // start after header + commit rows + button row
    let max_y = area.y + area.height;
    let mut flat_row: usize = 0; // tracks position in flat selection space

    for (section_label, file_items, wt_items, sec_idx) in &sections {
        // Skip the WORKTREES section unless there are linked worktrees.
        if *sec_idx == 2 && !show_worktrees {
            continue;
        }
        if row_y >= max_y {
            break;
        }
        let is_expanded = sc.sections_expanded[*sec_idx];
        let expand_icon = if is_expanded { '\u{25bc}' } else { '\u{25b6}' }; // ▼ / ▶

        // Section header row
        let is_hdr_selected = sc.has_focus && sc.selected == flat_row;
        let (hdr_r_fg, hdr_r_bg) = if is_hdr_selected {
            (hdr_fg, sel_bg)
        } else {
            (hdr_fg, hdr_bg)
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, row_y, ' ', hdr_r_fg, hdr_r_bg);
        }
        // Expand icon + label
        let hdr_text = format!(" {} {}", expand_icon, section_label);
        // Item count badge
        let badge_text = {
            let count = if *sec_idx == 2 {
                wt_items.map(|v| v.len()).unwrap_or(0)
            } else {
                file_items.len()
            };
            if count > 0 {
                format!(" ({})", count)
            } else {
                String::new()
            }
        };
        let full_hdr = format!("{}{}", hdr_text, badge_text);
        for (i, ch) in full_hdr.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, row_y, ch, hdr_r_fg, hdr_r_bg);
        }
        row_y += 1;
        flat_row += 1;

        if !is_expanded {
            continue;
        }

        // Items
        let item_count = if *sec_idx == 2 {
            wt_items.map(|v| v.len()).unwrap_or(0)
        } else {
            file_items.len()
        };

        // Determine if we need a scrollbar.
        // For simplicity we don't scroll SC sections (they're usually small).
        // Reserve rightmost col for scrollbar if needed.
        let need_sb = item_count > (max_y.saturating_sub(row_y)) as usize;
        let text_w = if need_sb {
            (area.width as usize).saturating_sub(1)
        } else {
            area.width as usize
        };

        if item_count == 0 {
            // "(no changes)" hint
            if row_y < max_y {
                let hint = "  (no changes)";
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', dim_fg, row_bg);
                }
                for (i, ch) in hint.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, row_y, ch, dim_fg, row_bg);
                }
                row_y += 1;
            }
        } else if *sec_idx == 2 {
            // Worktrees section
            if let Some(wts) = wt_items {
                for wt in *wts {
                    if row_y >= max_y {
                        break;
                    }
                    let is_selected = sc.has_focus && sc.selected == flat_row;
                    let (row_fg, r_bg) = if is_selected {
                        (item_fg, sel_bg)
                    } else {
                        (item_fg, row_bg)
                    };
                    for x in area.x..area.x + area.width {
                        set_cell(buf, x, row_y, ' ', row_fg, r_bg);
                    }
                    let check = if wt.is_current { '\u{2713}' } else { ' ' }; // ✓
                    let main_marker = if wt.is_main { " [main]" } else { "" };
                    let text = format!("  {} {} {}{}", check, wt.branch, wt.path, main_marker);
                    for (i, ch) in text.chars().enumerate().take(text_w) {
                        set_cell(buf, area.x + i as u16, row_y, ch, row_fg, r_bg);
                    }
                    row_y += 1;
                    flat_row += 1;
                }
            }
        } else {
            // File items (staged or unstaged)
            for fi in *file_items {
                if row_y >= max_y {
                    break;
                }
                let is_selected = sc.has_focus && sc.selected == flat_row;
                let r_bg = if is_selected { sel_bg } else { row_bg };
                let status_color = match fi.status_char {
                    'A' => add_fg,
                    'D' => del_fg,
                    _ => mod_fg,
                };
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', dim_fg, r_bg);
                }
                // Status char colored, then path dimmed
                let prefix = format!("  {} ", fi.status_char);
                for (i, ch) in prefix.chars().enumerate().take(text_w) {
                    let ch_fg = if ch == fi.status_char {
                        status_color
                    } else {
                        dim_fg
                    };
                    set_cell(buf, area.x + i as u16, row_y, ch, ch_fg, r_bg);
                }
                let path_start = prefix.chars().count();
                let path_color = if is_selected { item_fg } else { dim_fg };
                for (i, ch) in fi.path.chars().enumerate() {
                    let col = path_start + i;
                    if col >= text_w {
                        break;
                    }
                    set_cell(buf, area.x + col as u16, row_y, ch, path_color, r_bg);
                }
                row_y += 1;
                flat_row += 1;
            }
        }
    }

    // ── Log section (RECENT COMMITS) ─────────────────────────────────────────
    if row_y < max_y {
        let is_expanded = sc.sections_expanded[3];
        let expand_icon = if is_expanded { '\u{25bc}' } else { '\u{25b6}' };

        // Section header
        let is_hdr_selected = sc.has_focus && sc.selected == flat_row;
        let (log_hdr_fg, log_hdr_bg) = if is_hdr_selected {
            (hdr_fg, sel_bg)
        } else {
            (hdr_fg, hdr_bg)
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, row_y, ' ', log_hdr_fg, log_hdr_bg);
        }
        let count_badge = if !sc.log.is_empty() {
            format!(" ({})", sc.log.len())
        } else {
            String::new()
        };
        let hdr_text = format!(" {} \u{f417} RECENT COMMITS{}", expand_icon, count_badge);
        for (i, ch) in hdr_text.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, row_y, ch, log_hdr_fg, log_hdr_bg);
        }
        row_y += 1;
        flat_row += 1;

        if is_expanded && row_y < max_y {
            if sc.log.is_empty() {
                // "(no commits)" hint
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', dim_fg, row_bg);
                }
                let hint = "  (no commits)";
                for (i, ch) in hint.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, row_y, ch, dim_fg, row_bg);
                }
                row_y += 1;
            } else {
                for entry in &sc.log {
                    if row_y >= max_y {
                        break;
                    }
                    let is_selected = sc.has_focus && sc.selected == flat_row;
                    let r_bg = if is_selected { sel_bg } else { row_bg };
                    for x in area.x..area.x + area.width {
                        set_cell(buf, x, row_y, ' ', dim_fg, r_bg);
                    }
                    // Hash in dim color, message in item_fg
                    let hash_text = format!("  {} ", entry.hash);
                    let hash_w = hash_text.chars().count();
                    for (i, ch) in hash_text.chars().enumerate().take(area.width as usize) {
                        set_cell(buf, area.x + i as u16, row_y, ch, dim_fg, r_bg);
                    }
                    let msg_fg = if is_selected { item_fg } else { dim_fg };
                    for (i, ch) in entry.message.chars().enumerate() {
                        let col = hash_w + i;
                        if col >= area.width as usize {
                            break;
                        }
                        set_cell(buf, area.x + col as u16, row_y, ch, msg_fg, r_bg);
                    }
                    row_y += 1;
                    flat_row += 1;
                }
            }
        }
    }
    let _ = (row_y, flat_row); // consumed by rendering loop

    // ── Branch picker / create popup ─────────────────────────────────────────
    if let Some(ref bp) = sc.branch_picker {
        let popup_bg = rc(theme.completion_bg);
        let popup_fg = rc(theme.completion_fg);
        let popup_border = rc(theme.completion_border);
        let popup_sel = rc(theme.completion_selected_bg);
        let popup_w = area.width.saturating_sub(2).min(40);
        let popup_h = if bp.create_mode {
            3u16
        } else {
            area.height.saturating_sub(4).min(15)
        };
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + 2;
        // Clear popup area
        for y in popup_y..popup_y + popup_h {
            for x in popup_x..popup_x + popup_w {
                set_cell(buf, x, y, ' ', popup_fg, popup_bg);
            }
        }
        // Top border
        if popup_w >= 2 {
            set_cell(buf, popup_x, popup_y, '┌', popup_border, popup_bg);
            set_cell(
                buf,
                popup_x + popup_w - 1,
                popup_y,
                '┐',
                popup_border,
                popup_bg,
            );
            for x in popup_x + 1..popup_x + popup_w - 1 {
                set_cell(buf, x, popup_y, '─', popup_border, popup_bg);
            }
            let title = if bp.create_mode {
                " New Branch "
            } else {
                " Switch Branch "
            };
            let title_x = popup_x + 1;
            for (i, ch) in title.chars().enumerate() {
                let x = title_x + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, popup_y, ch, popup_border, popup_bg);
                }
            }
        }
        if bp.create_mode {
            let iy = popup_y + 1;
            let label = "Name: ";
            for (i, ch) in label.chars().enumerate() {
                let x = popup_x + 1 + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, iy, ch, dim_fg, popup_bg);
                }
            }
            let input_x = popup_x + 1 + label.len() as u16;
            for (i, ch) in bp.create_input.chars().enumerate() {
                let x = input_x + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, iy, ch, popup_fg, popup_bg);
                }
            }
            let cx = input_x + bp.create_input.len() as u16;
            if cx < popup_x + popup_w - 1 {
                set_cell(buf, cx, iy, '▏', popup_fg, popup_bg);
            }
            let by = popup_y + popup_h - 1;
            set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
            set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
            for x in popup_x + 1..popup_x + popup_w - 1 {
                set_cell(buf, x, by, '─', popup_border, popup_bg);
            }
        } else {
            let iy = popup_y + 1;
            let prefix = " \u{f002} ";
            for (i, ch) in prefix.chars().enumerate() {
                let x = popup_x + i as u16;
                if x < popup_x + popup_w {
                    set_cell(buf, x, iy, ch, dim_fg, popup_bg);
                }
            }
            let qx = popup_x + prefix.chars().count() as u16;
            for (i, ch) in bp.query.chars().enumerate() {
                let x = qx + i as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, iy, ch, popup_fg, popup_bg);
                }
            }
            let list_y = popup_y + 2;
            let list_h = popup_h.saturating_sub(3) as usize;
            let scroll_off = if bp.selected >= list_h {
                bp.selected - list_h + 1
            } else {
                0
            };
            for (vi, (name, is_current)) in
                bp.results.iter().skip(scroll_off).take(list_h).enumerate()
            {
                let y = list_y + vi as u16;
                let is_sel = vi + scroll_off == bp.selected;
                let bg = if is_sel { popup_sel } else { popup_bg };
                for x in popup_x..popup_x + popup_w {
                    set_cell(buf, x, y, ' ', popup_fg, bg);
                }
                let marker = if *is_current { "● " } else { "  " };
                let display = format!("{marker}{name}");
                for (i, ch) in display.chars().enumerate() {
                    let x = popup_x + 1 + i as u16;
                    if x < popup_x + popup_w - 1 {
                        set_cell(buf, x, y, ch, popup_fg, bg);
                    }
                }
            }
            let by = popup_y + popup_h - 1;
            if by >= list_y {
                set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
                set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
                for x in popup_x + 1..popup_x + popup_w - 1 {
                    set_cell(buf, x, by, '─', popup_border, popup_bg);
                }
            }
        }
        // Side borders
        for y in popup_y + 1..popup_y + popup_h.saturating_sub(1) {
            set_cell(buf, popup_x, y, '│', popup_border, popup_bg);
            if popup_x + popup_w > 0 {
                set_cell(buf, popup_x + popup_w - 1, y, '│', popup_border, popup_bg);
            }
        }
    }

    // ── Help dialog ──────────────────────────────────────────────────────────
    if sc.help_open {
        let popup_bg = rc(theme.completion_bg);
        let popup_fg = rc(theme.completion_fg);
        let popup_border = rc(theme.completion_border);
        let bindings: &[(&str, &str)] = &[
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
        let popup_w = area.width.saturating_sub(2).min(36);
        let popup_h = (bindings.len() as u16 + 3).min(area.height.saturating_sub(2));
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_h)) / 2;
        for y in popup_y..popup_y + popup_h {
            for x in popup_x..popup_x + popup_w {
                set_cell(buf, x, y, ' ', popup_fg, popup_bg);
            }
        }
        set_cell(buf, popup_x, popup_y, '┌', popup_border, popup_bg);
        set_cell(
            buf,
            popup_x + popup_w - 1,
            popup_y,
            '┐',
            popup_border,
            popup_bg,
        );
        for x in popup_x + 1..popup_x + popup_w - 1 {
            set_cell(buf, x, popup_y, '─', popup_border, popup_bg);
        }
        let title = " Keybindings ";
        let tx = popup_x + (popup_w.saturating_sub(title.len() as u16)) / 2;
        for (i, ch) in title.chars().enumerate() {
            let x = tx + i as u16;
            if x > popup_x && x < popup_x + popup_w - 1 {
                set_cell(buf, x, popup_y, ch, popup_border, popup_bg);
            }
        }
        // Close hint
        let close_x = popup_x + popup_w - 2;
        if close_x > popup_x {
            set_cell(buf, close_x, popup_y, 'x', popup_border, popup_bg);
        }
        let key_fg = rc(theme.function);
        for (i, (key, desc)) in bindings.iter().enumerate() {
            let y = popup_y + 1 + i as u16;
            if y >= popup_y + popup_h - 1 {
                break;
            }
            for (j, ch) in key.chars().enumerate() {
                let x = popup_x + 2 + j as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, y, ch, key_fg, popup_bg);
                }
            }
            let desc_x = popup_x + 12;
            for (j, ch) in desc.chars().enumerate() {
                let x = desc_x + j as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, y, ch, popup_fg, popup_bg);
                }
            }
        }
        let by = popup_y + popup_h - 1;
        set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
        set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
        for x in popup_x + 1..popup_x + popup_w - 1 {
            set_cell(buf, x, by, '─', popup_border, popup_bg);
        }
        for y in popup_y + 1..popup_y + popup_h - 1 {
            set_cell(buf, popup_x, y, '│', popup_border, popup_bg);
            set_cell(buf, popup_x + popup_w - 1, y, '│', popup_border, popup_bg);
        }
    }
}

// ─── Extension panel (plugin-provided) ───────────────────────────────────────

/// Render an extension-provided sidebar panel.
pub(super) fn render_ext_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    use crate::core::plugin::ExtPanelStyle;

    if area.height == 0 {
        return;
    }
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref panel) = screen.ext_panel else {
        return;
    };

    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let accent_fg = rc(theme.keyword);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.tab_bar_bg);

    // Clear area
    for cy in area.y..area.y + area.height {
        for cx in area.x..area.x + area.width {
            set_cell(buf, cx, cy, ' ', item_fg, row_bg);
        }
    }

    // Row 0: header
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    let title = format!("  {}", panel.title);
    for (i, ch) in title.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // Row 1: search input field (when active or has text)
    let input_row_count = if panel.input_active || !panel.input_text.is_empty() {
        1
    } else {
        0
    };
    if input_row_count > 0 {
        let iy = area.y + 1;
        let search_bg = rc(theme.tab_bar_bg);
        let search_fg = if panel.input_active {
            rc(theme.foreground)
        } else {
            dim_fg
        };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, iy, ' ', search_fg, search_bg);
        }
        let prefix = " / ";
        for (i, ch) in prefix.chars().enumerate() {
            let x = area.x + i as u16;
            if x < area.x + area.width {
                set_cell(buf, x, iy, ch, dim_fg, search_bg);
            }
        }
        let text_start = area.x + prefix.len() as u16;
        for (i, ch) in panel.input_text.chars().enumerate() {
            let x = text_start + i as u16;
            if x < area.x + area.width {
                set_cell(buf, x, iy, ch, search_fg, search_bg);
            }
        }
        if panel.input_active {
            let cursor_x = text_start + panel.input_text.chars().count() as u16;
            if cursor_x < area.x + area.width {
                set_cell(buf, cursor_x, iy, '▏', rc(theme.cursor), search_bg);
            }
        }
    }

    // Build flat list of rows
    let content_area_height = (area.height - 1 - input_row_count as u16) as usize;
    let mut flat_rows: Vec<(String, String, bool, bool)> = Vec::new(); // (text, hint, is_header, is_selected)
    let mut flat_idx = 0usize;
    for section in &panel.sections {
        let is_sel = flat_idx == panel.selected;
        let arrow = if section.expanded { "▼" } else { "▶" };
        flat_rows.push((
            format!(" {} {}", arrow, section.name),
            String::new(),
            true,
            is_sel,
        ));
        flat_idx += 1;
        if section.expanded {
            for item in &section.items {
                if item.is_separator {
                    flat_rows.push(("─".repeat(area.width as usize), String::new(), false, false));
                    flat_idx += 1;
                    continue;
                }
                let is_sel = flat_idx == panel.selected;
                let indent = "  ".repeat(item.indent as usize + 1);
                let icon_part = if item.icon.is_empty() {
                    String::new()
                } else {
                    format!("{} ", item.icon)
                };
                // Tree chevron for expandable items
                let chevron = if item.expandable {
                    let tree_key = (panel.name.clone(), item.id.clone());
                    let is_expanded = engine
                        .ext_panel_tree_expanded
                        .get(&tree_key)
                        .copied()
                        .unwrap_or(item.expanded);
                    if is_expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                } else {
                    ""
                };
                let fg_marker = match item.style {
                    ExtPanelStyle::Header => 'H',
                    ExtPanelStyle::Dim => 'D',
                    ExtPanelStyle::Accent => 'A',
                    ExtPanelStyle::Normal => 'N',
                };
                // Build hint with optional badges and action labels
                let mut hint_parts = Vec::new();
                for badge in &item.badges {
                    hint_parts.push(format!("[{}]", badge.text));
                }
                for action in &item.actions {
                    hint_parts.push(format!("⟨{}⟩", action.label));
                }
                if !item.hint.is_empty() {
                    hint_parts.push(item.hint.clone());
                }
                let hint_combined = hint_parts.join(" ");
                flat_rows.push((
                    format!("{}{}{}{}", indent, chevron, icon_part, item.text),
                    format!("{}|{}", fg_marker, hint_combined),
                    false,
                    is_sel,
                ));
                flat_idx += 1;
            }
        }
    }

    // Apply scroll
    let scroll = panel.scroll_top;
    let visible_rows = &flat_rows[scroll.min(flat_rows.len())..];

    for (ri, (text, hint_raw, is_header, is_sel)) in
        visible_rows.iter().enumerate().take(content_area_height)
    {
        let y = area.y + 1 + input_row_count as u16 + ri as u16;
        let bg = if *is_sel && panel.has_focus {
            sel_bg
        } else {
            row_bg
        };
        let fg = if *is_header {
            hdr_fg
        } else if hint_raw.starts_with('D') {
            dim_fg
        } else if hint_raw.starts_with('A') {
            accent_fg
        } else {
            item_fg
        };

        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', fg, bg);
        }

        let w = area.width as usize;

        // Right-aligned hint (skip the style marker char and pipe)
        let hint = if hint_raw.len() > 2 {
            &hint_raw[2..]
        } else {
            ""
        };
        let hint_len = hint.chars().count();

        // Truncate text before the hint area, adding "…" if clipped
        let text_max = if !hint.is_empty() {
            w.saturating_sub(hint_len + 2) // 1 space gap + 1 for safety
        } else {
            w
        };
        let text_char_count = text.chars().count();
        if text_char_count > text_max && text_max > 1 {
            for (i, ch) in text.chars().take(text_max - 1).enumerate() {
                set_cell(buf, area.x + i as u16, y, ch, fg, bg);
            }
            set_cell(buf, area.x + (text_max - 1) as u16, y, '…', fg, bg);
        } else {
            for (i, ch) in text.chars().enumerate().take(text_max) {
                set_cell(buf, area.x + i as u16, y, ch, fg, bg);
            }
        }

        if !hint.is_empty() {
            let start = w.saturating_sub(hint_len + 1);
            for (i, ch) in hint.chars().enumerate() {
                let x = area.x + (start + i) as u16;
                if x < area.x + area.width {
                    set_cell(buf, x, y, ch, dim_fg, bg);
                }
            }
        }
    }

    // Scrollbar
    let total = flat_rows.len();
    if total > content_area_height && content_area_height > 0 {
        let sb_x = area.x + area.width - 1;
        let track_h = content_area_height;
        let thumb_h = (track_h * content_area_height / total).max(1);
        let thumb_top = scroll * track_h / total;
        for i in 0..track_h {
            let y = area.y + 1 + input_row_count as u16 + i as u16;
            let ch = if i >= thumb_top && i < thumb_top + thumb_h {
                '█'
            } else {
                '░'
            };
            set_cell(buf, sb_x, y, ch, dim_fg, row_bg);
        }
    }

    // ── Help popup overlay ──────────────────────────────────────────────────
    if panel.help_open && !panel.help_bindings.is_empty() {
        let popup_bg = rc(theme.completion_bg);
        let popup_fg = rc(theme.completion_fg);
        let popup_border = rc(theme.completion_border);
        let bindings = &panel.help_bindings;
        let popup_w = area.width.saturating_sub(2).min(36);
        let popup_h = (bindings.len() as u16 + 3).min(area.height.saturating_sub(2));
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_h)) / 2;
        for y in popup_y..popup_y + popup_h {
            for x in popup_x..popup_x + popup_w {
                set_cell(buf, x, y, ' ', popup_fg, popup_bg);
            }
        }
        set_cell(buf, popup_x, popup_y, '┌', popup_border, popup_bg);
        set_cell(
            buf,
            popup_x + popup_w - 1,
            popup_y,
            '┐',
            popup_border,
            popup_bg,
        );
        for x in popup_x + 1..popup_x + popup_w - 1 {
            set_cell(buf, x, popup_y, '─', popup_border, popup_bg);
        }
        let title = " Keybindings ";
        let tx = popup_x + (popup_w.saturating_sub(title.len() as u16)) / 2;
        for (i, ch) in title.chars().enumerate() {
            let x = tx + i as u16;
            if x > popup_x && x < popup_x + popup_w - 1 {
                set_cell(buf, x, popup_y, ch, popup_border, popup_bg);
            }
        }
        let close_x = popup_x + popup_w - 2;
        if close_x > popup_x {
            set_cell(buf, close_x, popup_y, 'x', popup_border, popup_bg);
        }
        let key_fg = rc(theme.function);
        for (i, (key, desc)) in bindings.iter().enumerate() {
            let y = popup_y + 1 + i as u16;
            if y >= popup_y + popup_h - 1 {
                break;
            }
            for (j, ch) in key.chars().enumerate() {
                let x = popup_x + 2 + j as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, y, ch, key_fg, popup_bg);
                }
            }
            let desc_x = popup_x + 12;
            for (j, ch) in desc.chars().enumerate() {
                let x = desc_x + j as u16;
                if x < popup_x + popup_w - 1 {
                    set_cell(buf, x, y, ch, popup_fg, popup_bg);
                }
            }
        }
        let by = popup_y + popup_h - 1;
        set_cell(buf, popup_x, by, '└', popup_border, popup_bg);
        set_cell(buf, popup_x + popup_w - 1, by, '┘', popup_border, popup_bg);
        for x in popup_x + 1..popup_x + popup_w - 1 {
            set_cell(buf, x, by, '─', popup_border, popup_bg);
        }
        for y in popup_y + 1..popup_y + popup_h - 1 {
            set_cell(buf, popup_x, y, '│', popup_border, popup_bg);
            set_cell(buf, popup_x + popup_w - 1, y, '│', popup_border, popup_bg);
        }
    }
}

// ─── Panel hover popup ─────────────────────────────────────────────────────────

/// Render a panel-item hover popup to the right of the sidebar.
///
/// The popup displays rendered markdown content and appears to the right of
/// the sidebar at the vertical position of the hovered item.
/// Returns (link_rects, popup_rect) where popup_rect is (x, y, w, h).
#[allow(clippy::type_complexity)]
pub(super) fn render_panel_hover_popup(
    frame: &mut ratatui::Frame,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar_right_x: u16,
    sidebar_y: u16,
    sidebar_height: u16,
    term_area: Rect,
) -> (
    Vec<(u16, u16, u16, u16, String)>,
    Option<(u16, u16, u16, u16)>,
) {
    use crate::core::markdown::MdStyle;

    let Some(ref ph) = screen.panel_hover else {
        return (vec![], None);
    };

    let lines = &ph.rendered.lines;
    if lines.is_empty() {
        return (vec![], None);
    }
    const MAX_HEIGHT: u16 = 20;

    let num_lines = lines.len().min(MAX_HEIGHT as usize) as u16;
    let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(10);
    // Available width to the right of the sidebar.
    let avail_w = term_area.width.saturating_sub(sidebar_right_x);
    if avail_w < 10 {
        return (vec![], None);
    }
    // +4 for left/right border + padding; +2 for top/bottom border rows.
    let width = (max_len as u16 + 4).clamp(12, avail_w);
    let height = num_lines + 2; // content rows + top/bottom border

    // Vertically align with the hovered item.
    let item_row = if ph.panel_name == "source_control" {
        let section_start: u16 = 5;
        section_start + ph.item_index as u16
    } else {
        ph.item_index as u16 + 1
    };
    let raw_y = sidebar_y + item_row;

    let x = sidebar_right_x;
    let y = raw_y.min(
        term_area
            .height
            .saturating_sub(height)
            .min(sidebar_y + sidebar_height.saturating_sub(1)),
    );

    let bg = rc(theme.hover_bg);
    let fg = rc(theme.hover_fg);
    let border = rc(theme.hover_border);
    let h1_fg = rc(theme.md_heading1);
    let h2_fg = rc(theme.md_heading2);
    let h3_fg = rc(theme.md_heading3);
    let code_fg = rc(theme.md_code);
    let link_fg = rc(theme.md_link);

    let buf = frame.buffer_mut();

    // ── Top border ───────────────────────────────────────────────────────────
    let top_y = y;
    if top_y < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx >= term_area.width {
                break;
            }
            let ch = if col == 0 {
                '┌'
            } else if col == width - 1 {
                '┐'
            } else {
                '─'
            };
            let cell = &mut buf[(cx, top_y)];
            cell.set_char(ch).set_fg(border).set_bg(bg);
        }
    }

    // ── Content rows ─────────────────────────────────────────────────────────
    for (li, text_line) in lines.iter().enumerate().take(num_lines as usize) {
        let row_y = y + 1 + li as u16; // +1 for top border
        if row_y >= term_area.height {
            break;
        }

        // Fill row background with left/right borders.
        for col in 0..width {
            let cx = x + col;
            if cx >= term_area.width {
                break;
            }
            let cell = &mut buf[(cx, row_y)];
            cell.set_bg(bg);
            let ch = if col == 0 || col == width - 1 {
                '│'
            } else {
                ' '
            };
            cell.set_char(ch).set_fg(border);
        }

        // Render styled text inside the border.
        let line_spans = ph.rendered.spans.get(li);
        let code_hl = ph.rendered.code_highlights.get(li);
        let has_code_hl = code_hl.is_some_and(|h| !h.is_empty());
        let display_text = format!(" {}", text_line);

        let mut col_x: u16 = 1; // inside left border
        let mut byte_pos: usize = 0;
        for ch in display_text.chars() {
            let ch_len = ch.len_utf8();
            let adj_byte = byte_pos.saturating_sub(1);
            let (ch_fg, bold) = if has_code_hl && byte_pos > 0 {
                // Use tree-sitter syntax highlighting for code block lines.
                code_hl
                    .unwrap()
                    .iter()
                    .find(|h| adj_byte >= h.start_byte && adj_byte < h.end_byte)
                    .map(|h| (rc(theme.scope_color(&h.scope)), false))
                    .unwrap_or((code_fg, false))
            } else if let Some(spans) = line_spans {
                spans
                    .iter()
                    .find(|sp| byte_pos > 0 && adj_byte >= sp.start_byte && adj_byte < sp.end_byte)
                    .map(|sp| match sp.style {
                        MdStyle::Heading(1) => (h1_fg, true),
                        MdStyle::Heading(2) => (h2_fg, true),
                        MdStyle::Heading(_) => (h3_fg, true),
                        MdStyle::Bold => (fg, true),
                        MdStyle::Italic => (fg, false),
                        MdStyle::BoldItalic => (fg, true),
                        MdStyle::Code | MdStyle::CodeBlock => (code_fg, false),
                        MdStyle::Link => (link_fg, false),
                        MdStyle::LinkUrl => (link_fg, false),
                        MdStyle::BlockQuote => (h3_fg, false),
                        MdStyle::ListBullet => (h1_fg, true),
                        _ => (fg, false),
                    })
                    .unwrap_or((fg, false))
            } else {
                (fg, false)
            };

            let cx = x + col_x;
            if col_x + 1 < width && cx < term_area.width {
                let cell = &mut buf[(cx, row_y)];
                cell.set_char(ch).set_fg(ch_fg).set_bg(bg);
                if bold {
                    cell.set_style(cell.style().add_modifier(ratatui::style::Modifier::BOLD));
                }
            }

            byte_pos += ch_len;
            col_x += 1;
        }
    }

    // ── Bottom border ────────────────────────────────────────────────────────
    let bot_y = y + 1 + num_lines;
    if bot_y < term_area.height {
        for col in 0..width {
            let cx = x + col;
            if cx >= term_area.width {
                break;
            }
            let ch = if col == 0 {
                '└'
            } else if col == width - 1 {
                '┘'
            } else {
                '─'
            };
            let cell = &mut buf[(cx, bot_y)];
            cell.set_char(ch).set_fg(border).set_bg(bg);
        }
    }

    // ── Compute link hit rects ───────────────────────────────────────────────
    let mut link_rects = Vec::new();
    for &(line_idx, start_byte, end_byte, ref url) in &ph.links {
        if line_idx >= num_lines as usize {
            continue;
        }
        if let Some(line_text) = lines.get(line_idx) {
            // Count characters before start_byte and between start/end to get column range.
            // The display has a 1-char " " prefix inside the left border.
            let prefix_chars = line_text[..start_byte.min(line_text.len())].chars().count() as u16;
            let link_chars = line_text
                [start_byte.min(line_text.len())..end_byte.min(line_text.len())]
                .chars()
                .count() as u16;
            let row = y + 1 + line_idx as u16; // +1 for top border
            let col_start = x + 2 + prefix_chars; // +2 for border + space prefix
            link_rects.push((col_start, row, link_chars, 1, url.clone()));
        }
    }
    (link_rects, Some((x, y, width, height)))
}

// ─── Editor hover popup ─────────────────────────────────────────────────────

/// Render an editor hover popup with rich markdown content.
/// Positioned above or below the anchor position in the editor viewport.
/// Returns (link_rects, popup bounding rect) for mouse hit-testing.
#[allow(clippy::type_complexity)]
pub(super) fn render_editor_hover_popup(
    frame: &mut ratatui::Frame,
    eh: &render::EditorHoverPopupData,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) -> (
    Vec<(u16, u16, u16, u16, String)>,
    Option<(u16, u16, u16, u16)>,
) {
    use crate::core::markdown::MdStyle;

    let lines = &eh.rendered.lines;
    if lines.is_empty() {
        return (vec![], None);
    }
    const MAX_HEIGHT: u16 = 20;
    let scroll = eh.scroll_top;
    let visible_count = lines.len().saturating_sub(scroll).min(MAX_HEIGHT as usize) as u16;
    if visible_count == 0 {
        return (vec![], None);
    }
    // Fixed height based on total content (capped), so scrolling doesn't shrink the popup
    let num_lines = lines.len().min(MAX_HEIGHT as usize) as u16;
    // Content width: 1 char left padding + content + 1 char right padding
    let content_w = (eh.popup_width as u16 + 2).clamp(12, term_area.width.saturating_sub(4));
    // Total width/height including border
    let width = content_w + 2;
    let height = num_lines + 2;

    // Place directly above the word (touching), otherwise below
    let y = if popup_y >= height {
        popup_y - height
    } else {
        popup_y + 1
    };
    let x = popup_x.min(term_area.width.saturating_sub(width));
    let y = y.min(term_area.height.saturating_sub(height));

    let bg = rc(theme.hover_bg);
    let fg = rc(theme.hover_fg);
    let border = rc(theme.hover_border);
    let h1_fg = rc(theme.md_heading1);
    let h2_fg = rc(theme.md_heading2);
    let h3_fg = rc(theme.md_heading3);
    let code_fg = rc(theme.md_code);
    let link_fg = rc(theme.md_link);

    // Border color: subtle when unfocused, blue when focused
    let border_fg = if eh.has_focus {
        rc(theme.md_link)
    } else {
        border
    };

    let buf = frame.buffer_mut();

    // Top border
    if y < term_area.height {
        for c in 0..width {
            let cx = x + c;
            if cx >= term_area.width {
                break;
            }
            let ch = if c == 0 {
                '┌'
            } else if c == width - 1 {
                '┐'
            } else {
                '─'
            };
            buf[(cx, y)].set_char(ch).set_fg(border_fg).set_bg(bg);
        }
    }

    // Content rows with left/right borders
    for (li, text_line) in lines
        .iter()
        .skip(scroll)
        .enumerate()
        .take(num_lines as usize)
    {
        let row_y = y + 1 + li as u16;
        if row_y >= term_area.height {
            break;
        }

        // Fill row with background + borders
        for col in 0..width {
            let cx = x + col;
            if cx >= term_area.width {
                break;
            }
            if col == 0 || col == width - 1 {
                buf[(cx, row_y)].set_char('│').set_fg(border_fg).set_bg(bg);
            } else {
                buf[(cx, row_y)].set_char(' ').set_bg(bg);
            }
        }

        // Draw text with 1-char padding inside left border
        let actual_line = scroll + li;
        let line_spans = eh.rendered.spans.get(actual_line);
        let code_hl = eh.rendered.code_highlights.get(actual_line);
        let has_code_hl = code_hl.is_some_and(|h| !h.is_empty());
        let mut col_x: u16 = 2; // border + 1 padding
        let mut byte_pos: usize = 0;
        for ch in text_line.chars() {
            let ch_len = ch.len_utf8();
            let (ch_fg, bold) = if has_code_hl {
                // Use tree-sitter syntax highlighting for code block lines.
                code_hl
                    .unwrap()
                    .iter()
                    .find(|h| byte_pos >= h.start_byte && byte_pos < h.end_byte)
                    .map(|h| (rc(theme.scope_color(&h.scope)), false))
                    .unwrap_or((code_fg, false))
            } else if let Some(spans) = line_spans {
                spans
                    .iter()
                    .find(|sp| byte_pos >= sp.start_byte && byte_pos < sp.end_byte)
                    .map(|sp| match sp.style {
                        MdStyle::Heading(1) => (h1_fg, true),
                        MdStyle::Heading(2) => (h2_fg, true),
                        MdStyle::Heading(_) => (h3_fg, true),
                        MdStyle::Bold => (fg, true),
                        MdStyle::Italic => (fg, false),
                        MdStyle::BoldItalic => (fg, true),
                        MdStyle::Code | MdStyle::CodeBlock => (code_fg, false),
                        MdStyle::Link => (link_fg, false),
                        MdStyle::LinkUrl => (link_fg, false),
                        MdStyle::BlockQuote => (h3_fg, false),
                        MdStyle::ListBullet => (h1_fg, true),
                        _ => (fg, false),
                    })
                    .unwrap_or((fg, false))
            } else {
                (fg, false)
            };

            let cx = x + col_x;
            let char_col = (col_x - 2) as usize; // 0-based char column (border + padding)
            if col_x + 1 < width && cx < term_area.width {
                let cell = &mut buf[(cx, row_y)];
                // Check if this character is within the text selection
                let in_selection = if let Some((sl, sc, el, ec)) = eh.selection {
                    let line = actual_line;
                    if sl == el {
                        line == sl && char_col >= sc && char_col < ec
                    } else if line == sl {
                        char_col >= sc
                    } else if line == el {
                        char_col < ec
                    } else {
                        line > sl && line < el
                    }
                } else {
                    false
                };
                if in_selection {
                    cell.set_char(ch).set_fg(bg).set_bg(ch_fg);
                } else {
                    cell.set_char(ch).set_fg(ch_fg).set_bg(bg);
                }
                if bold {
                    cell.set_style(cell.style().add_modifier(ratatui::style::Modifier::BOLD));
                }
                // Highlight focused link
                if eh.has_focus && !in_selection {
                    if let Some(focused) = eh.focused_link {
                        if let Some(&(link_line, start_b, end_b, _)) = eh.links.get(focused) {
                            if link_line == scroll + li && byte_pos >= start_b && byte_pos < end_b {
                                cell.set_style(
                                    cell.style()
                                        .add_modifier(ratatui::style::Modifier::UNDERLINED),
                                );
                            }
                        }
                    }
                }
            }

            byte_pos += ch_len;
            col_x += 1;
        }
    }

    // Bottom border
    let bot_y = y + 1 + num_lines;
    if bot_y < term_area.height {
        for c in 0..width {
            let cx = x + c;
            if cx >= term_area.width {
                break;
            }
            let ch = if c == 0 {
                '└'
            } else if c == width - 1 {
                '┘'
            } else {
                '─'
            };
            buf[(cx, bot_y)].set_char(ch).set_fg(border_fg).set_bg(bg);
        }
    }

    // Scrollbar on right border when content overflows
    let total = lines.len();
    let can_scroll = total > MAX_HEIGHT as usize;
    if can_scroll && num_lines > 0 {
        let sb_x = x + width - 1;
        if sb_x < term_area.width {
            let track_h = num_lines as usize;
            let thumb_h = (track_h * track_h / total).max(1);
            let max_scroll = total.saturating_sub(MAX_HEIGHT as usize);
            let thumb_top = if max_scroll > 0 {
                scroll * (track_h - thumb_h) / max_scroll
            } else {
                0
            };
            for i in 0..track_h {
                let ry = y + 1 + i as u16;
                if ry >= term_area.height {
                    break;
                }
                let cell = &mut buf[(sb_x, ry)];
                if i >= thumb_top && i < thumb_top + thumb_h {
                    cell.set_char('█').set_fg(border_fg).set_bg(bg);
                } else {
                    cell.set_char('░').set_fg(border).set_bg(bg);
                }
            }
        }
    }

    // Compute link hit rects for click-to-copy.
    let mut link_rects = Vec::new();
    for &(line_idx, start_byte, end_byte, ref url) in &eh.links {
        if line_idx < scroll || line_idx >= scroll + num_lines as usize {
            continue;
        }
        let vis_line = line_idx - scroll;
        if let Some(line_text) = lines.get(line_idx) {
            let prefix_chars = line_text[..start_byte.min(line_text.len())].chars().count() as u16;
            let link_chars = line_text
                [start_byte.min(line_text.len())..end_byte.min(line_text.len())]
                .chars()
                .count() as u16;
            let link_row = y + 1 + vis_line as u16;
            let link_col = x + 2 + prefix_chars; // border + padding
            link_rects.push((link_col, link_row, link_chars, 1u16, url.clone()));
        }
    }

    (link_rects, Some((x, y, width, height)))
}

// ─── Extensions sidebar panel ─────────────────────────────────────────────────

/// Render the Extensions sidebar panel.
pub(super) fn render_ext_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref ext) = screen.ext_sidebar else {
        return;
    };

    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let sec_bg = rc(theme.status_bg.darken(0.15));
    let default_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let panel_bg = rc(theme.completion_bg);

    // Helper: fill one row then write text chars
    let write_row =
        |buf: &mut ratatui::buffer::Buffer, y: u16, text: &str, fg: RColor, bg: RColor| {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', fg, bg);
            }
            for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, y, ch, fg, bg);
            }
        };

    let mut y = area.y;

    // ── Row 0: header ────────────────────────────────────────────────────────
    if y < area.y + area.height {
        let hdr = if ext.fetching {
            " \u{eb85} EXTENSIONS  (fetching…)".to_string()
        } else {
            " \u{eb85} EXTENSIONS".to_string()
        };
        write_row(buf, y, &hdr, header_fg, header_bg);
        y += 1;
    }

    // ── Row 1: search box ─────────────────────────────────────────────────────
    if y < area.y + area.height {
        let search_bg = if ext.input_active { sel_bg } else { panel_bg };
        let search_fg = if ext.input_active || !ext.query.is_empty() {
            default_fg
        } else {
            dim_fg
        };
        let search_text = if ext.input_active {
            format!(" \u{f002} {}|", ext.query)
        } else if ext.query.is_empty() {
            " \u{f002} Search extensions (press /)".to_string()
        } else {
            format!(" \u{f002} {}", ext.query)
        };
        write_row(buf, y, &search_text, search_fg, search_bg);
        y += 1;
    }

    // ── INSTALLED section ─────────────────────────────────────────────────────
    let installed_count = ext.items_installed.len();
    if y < area.y + area.height {
        let arrow = if ext.sections_expanded[0] {
            '▼'
        } else {
            '▶'
        };
        let sec_hdr = format!("  {} INSTALLED ({})", arrow, installed_count);
        write_row(buf, y, &sec_hdr, dim_fg, sec_bg);
        y += 1;
    }

    if ext.sections_expanded[0] {
        for (idx, item) in ext.items_installed.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let is_sel = ext.has_focus && ext.selected == idx;
            let (fg, bg) = if is_sel {
                (panel_bg, default_fg)
            } else {
                (default_fg, panel_bg)
            };
            let label = if item.update_available {
                format!("  ● {} \u{2191}", item.display_name) // ↑ update indicator
            } else {
                format!("  ● {}", item.display_name)
            };
            write_row(buf, y, &label, fg, bg);
            // Right-aligned hint
            let hint = if item.update_available {
                "[u] update"
            } else {
                "[d] remove"
            };
            let hint_start = area.x + area.width.saturating_sub(hint.len() as u16 + 1);
            for (i, ch) in hint.chars().enumerate() {
                let cx = hint_start + i as u16;
                if cx < area.x + area.width {
                    set_cell(buf, cx, y, ch, dim_fg, bg);
                }
            }
            y += 1;
        }
        if installed_count == 0 && y < area.y + area.height {
            write_row(buf, y, "    (none installed)", dim_fg, panel_bg);
            y += 1;
        }
    }

    // ── AVAILABLE section ─────────────────────────────────────────────────────
    let available_count = ext.items_available.len();
    if y < area.y + area.height {
        let arrow = if ext.sections_expanded[1] {
            '▼'
        } else {
            '▶'
        };
        let sec_hdr = format!("  {} AVAILABLE ({})", arrow, available_count);
        write_row(buf, y, &sec_hdr, dim_fg, sec_bg);
        y += 1;
    }

    if ext.sections_expanded[1] {
        for (idx, item) in ext.items_available.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let flat_idx = installed_count + idx;
            let is_sel = ext.has_focus && ext.selected == flat_idx;
            let (fg, bg) = if is_sel {
                (panel_bg, default_fg)
            } else {
                (default_fg, panel_bg)
            };
            write_row(buf, y, &format!("  ○ {}", item.display_name), fg, bg);
            // Right-aligned hint
            let hint = "[i] install";
            let hint_start = area.x + area.width.saturating_sub(hint.len() as u16 + 1);
            for (i, ch) in hint.chars().enumerate() {
                let cx = hint_start + i as u16;
                if cx < area.x + area.width {
                    set_cell(buf, cx, y, ch, dim_fg, bg);
                }
            }
            y += 1;
        }
        if available_count == 0 && y < area.y + area.height {
            let msg = if ext.fetching {
                "    Fetching registry…"
            } else {
                "    (all installed)"
            };
            write_row(buf, y, msg, dim_fg, panel_bg);
            y += 1;
        }
    }

    // Fill remainder with panel_bg
    while y < area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', dim_fg, panel_bg);
        }
        y += 1;
    }

    let _ = sel_bg;
}

// ─── AI assistant sidebar panel ───────────────────────────────────────────────

/// Render the AI assistant sidebar panel.
pub(super) fn render_ai_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref ai) = screen.ai_panel else {
        return;
    };

    let header_fg = rc(theme.status_fg);
    let header_bg = rc(theme.status_bg);
    let default_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let panel_bg = rc(theme.completion_bg);
    let user_fg = rc(theme.keyword);
    let asst_fg = rc(theme.string_lit);
    let input_bg = rc(theme.fuzzy_selected_bg);

    let write_row =
        |buf: &mut ratatui::buffer::Buffer, y: u16, text: &str, fg: RColor, bg: RColor| {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', fg, bg);
            }
            for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, y, ch, fg, bg);
            }
        };

    let mut y = area.y;

    // ── Row 0: header ─────────────────────────────────────────────────────────
    if y < area.y + area.height {
        let hdr = if ai.streaming {
            " \u{f0e5} AI ASSISTANT  (thinking…)"
        } else {
            " \u{f0e5} AI ASSISTANT"
        };
        write_row(buf, y, hdr, header_fg, header_bg);
        y += 1;
    }

    // ── Compute input height (grows with content) ─────────────────────────────
    let pfx_len = 3usize; // " > " / "   "
    let content_w = (area.width as usize).saturating_sub(pfx_len).max(1);
    let input_chars: Vec<char> = ai.input.chars().collect();
    let input_line_count = {
        let raw = if input_chars.is_empty() {
            1
        } else {
            input_chars.len().div_ceil(content_w)
        };
        // cap so messages keep at least 3 rows
        raw.min((area.height as usize).saturating_sub(5).max(1))
    };
    // +1 for separator row
    let input_rows = input_line_count as u16 + 1;
    let msg_area_height = area.height.saturating_sub(1 + input_rows); // 1 = header

    // ── Message history ───────────────────────────────────────────────────────
    let scroll = ai.scroll_top;
    let wrap_w = content_w.saturating_sub(1).max(10); // slightly narrower for "  " indent
    let mut all_rows: Vec<(String, RColor)> = Vec::new();
    for msg in &ai.messages {
        let is_user = msg.role == "user";
        let role_label = if is_user { "You:" } else { "AI:" };
        let role_fg = if is_user { user_fg } else { asst_fg };
        all_rows.push((role_label.to_string(), role_fg));
        for line in msg.content.lines() {
            if line.is_empty() {
                all_rows.push(("  ".to_string(), default_fg));
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = (pos + wrap_w).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                all_rows.push((format!("  {}", chunk), default_fg));
                pos = end;
            }
        }
        all_rows.push((" ".to_string(), panel_bg)); // blank separator
    }

    let total = all_rows.len();
    let start = scroll.min(total.saturating_sub(msg_area_height as usize));
    for (i, (text, fg)) in all_rows.iter().enumerate().skip(start) {
        if y >= area.y + 1 + msg_area_height {
            break;
        }
        write_row(buf, y, text, *fg, panel_bg);
        y += 1;
        let _ = i;
    }

    // Fill remaining message area
    while y < area.y + 1 + msg_area_height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, ' ', dim_fg, panel_bg);
        }
        y += 1;
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    if y < area.y + area.height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, y, '─', dim_fg, header_bg);
        }
        y += 1;
    }

    // ── Input area (multi-line, grows with content) ────────────────────────────
    let (inp_bg, inp_fg) = if ai.input_active {
        (input_bg, default_fg)
    } else {
        (panel_bg, dim_fg)
    };
    let cursor = ai.input_cursor.min(input_chars.len());
    let cursor_line = if content_w > 0 { cursor / content_w } else { 0 };
    let cursor_col = if content_w > 0 {
        cursor % content_w
    } else {
        cursor
    };

    if ai.input_active || !ai.input.is_empty() {
        // Split input into visual chunks
        let chunks: Vec<&[char]> = if input_chars.is_empty() {
            vec![&[][..]]
        } else {
            input_chars.chunks(content_w).collect()
        };
        for (line_idx, chunk) in chunks.iter().enumerate().take(input_line_count) {
            if y >= area.y + area.height {
                break;
            }
            // Fill background
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', inp_fg, inp_bg);
            }
            // Prefix: " > " on first line, "   " on continuations
            let pfx = if line_idx == 0 { " > " } else { "   " };
            for (i, ch) in pfx.chars().enumerate() {
                set_cell(buf, area.x + i as u16, y, ch, inp_fg, inp_bg);
            }
            // Content
            for (i, &ch) in chunk.iter().enumerate() {
                set_cell(
                    buf,
                    area.x + pfx_len as u16 + i as u16,
                    y,
                    ch,
                    inp_fg,
                    inp_bg,
                );
            }
            // Cursor (inverted cell on the cursor line)
            if ai.input_active && line_idx == cursor_line {
                let cx = area.x + pfx_len as u16 + cursor_col as u16;
                if cx < area.x + area.width {
                    let cursor_ch = input_chars.get(cursor).copied().unwrap_or(' ');
                    set_cell(buf, cx, y, cursor_ch, inp_bg, inp_fg);
                }
            }
            y += 1;
        }
    } else {
        // Placeholder when input is empty and not active
        if y < area.y + area.height {
            for x in area.x..area.x + area.width {
                set_cell(buf, x, y, ' ', inp_fg, inp_bg);
            }
            let placeholder = if ai.streaming {
                " (waiting for response…)"
            } else {
                " Press i to type…"
            };
            for (i, ch) in placeholder.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, y, ch, inp_fg, inp_bg);
            }
        }
    }
}

// ─── Debug sidebar panel ──────────────────────────────────────────────────────

/// Render the debug sidebar: header + run button + 4 sections (Variables, Watch, Call Stack, Breakpoints).
pub(super) fn render_debug_sidebar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    use render::DebugSidebarSection;
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let act_fg = rc(theme.status_fg.lighten(0.2));
    let row_bg = rc(theme.tab_bar_bg);

    // ── Row 0: header strip ──────────────────────────────────────────────────
    let cfg_name = engine
        .dap_launch_configs
        .get(engine.dap_selected_launch_config)
        .map(|c| c.name.as_str())
        .unwrap_or("no config");
    let header_text = format!("  \u{f188} DEBUG  |  {cfg_name}");
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in header_text.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // ── Row 1: Run / Stop button ─────────────────────────────────────────────
    let btn_y = area.y + 1;
    let (btn_label, btn_icon_fg) =
        if engine.dap_session_active && engine.dap_stopped_thread.is_some() {
            ("\u{f04b}  Continue", rc(theme.git_added))
        } else if engine.dap_session_active {
            ("\u{f04d}  Stop", rc(theme.diagnostic_error))
        } else {
            ("\u{f04b}  Start Debugging", rc(theme.git_added))
        };
    for x in area.x..area.x + area.width {
        set_cell(buf, x, btn_y, ' ', hdr_fg, hdr_bg);
    }
    // Icon character gets the semantic color; label text uses status_fg for readability.
    for (i, ch) in btn_label.chars().enumerate().take(area.width as usize) {
        let fg = if i == 0 { btn_icon_fg } else { hdr_fg };
        set_cell(buf, area.x + i as u16, btn_y, ch, fg, hdr_bg);
    }

    // ── Sections with fixed-height allocation + per-section scrolling ──────
    // Build minimal screen layout to get debug_sidebar data
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let sidebar = &screen.debug_sidebar;

    let sections: [(
        &str,
        &[render::DebugSidebarItem],
        DebugSidebarSection,
        usize,
    ); 4] = [
        (
            "\u{f6a9} VARIABLES",
            &sidebar.variables,
            DebugSidebarSection::Variables,
            0,
        ),
        (
            "\u{f06e} WATCH",
            &sidebar.watch,
            DebugSidebarSection::Watch,
            1,
        ),
        (
            "\u{f020e} CALL STACK",
            &sidebar.frames,
            DebugSidebarSection::CallStack,
            2,
        ),
        (
            "\u{f111} BREAKPOINTS",
            &sidebar.breakpoints,
            DebugSidebarSection::Breakpoints,
            3,
        ),
    ];

    // Available rows after header(1) + button(1) = 2 overhead rows.
    // Each section has 1 header row, so 4 section headers = 4 rows.
    // Content rows = available - 4 section headers.
    let available = (area.height as usize).saturating_sub(2);
    let section_header_rows = 4;
    let content_rows = available.saturating_sub(section_header_rows);

    // Compute per-section content heights (equal share; remainder to first).
    let mut heights = [0u16; 4];
    if content_rows > 0 {
        let base = content_rows / 4;
        let remainder = content_rows % 4;
        for (i, h) in heights.iter_mut().enumerate() {
            *h = (base + if i < remainder { 1 } else { 0 }) as u16;
        }
    }
    // Store back into engine for ensure_visible calculations.
    // (We can't mutate engine directly here since it's borrowed, but the heights
    // are also stored on the sidebar data for reference.)

    let track_fg = rc(theme.separator);
    let thumb_fg = rc(theme.scrollbar_thumb);
    let sb_bg = rc(theme.background);

    let mut row_y = area.y + 2;
    let max_y = area.y + area.height;

    for (section_label, items, section_kind, sec_idx) in &sections {
        if row_y >= max_y {
            break;
        }
        // Section header
        let is_active = sidebar.active_section == *section_kind;
        let sect_fg = if is_active { act_fg } else { hdr_fg };
        for x in area.x..area.x + area.width {
            set_cell(buf, x, row_y, ' ', sect_fg, hdr_bg);
        }
        for (i, ch) in section_label.chars().enumerate().take(area.width as usize) {
            set_cell(buf, area.x + i as u16, row_y, ch, sect_fg, hdr_bg);
        }
        row_y += 1;

        let sec_height = heights[*sec_idx] as usize;
        let scroll_off = sidebar.scroll_offsets[*sec_idx];
        let total_items = items.len().max(1); // at least 1 for "(empty)" hint

        // Render items within the allocated height
        for row_offset in 0..sec_height {
            if row_y >= max_y {
                break;
            }
            let item_idx = scroll_off + row_offset;
            if items.is_empty() && row_offset == 0 {
                // Empty hint
                let hint = if engine.dap_session_active {
                    "  (empty)"
                } else {
                    "  (not running)"
                };
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', item_fg, row_bg);
                }
                for (i, ch) in hint.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, row_y, ch, item_fg, row_bg);
                }
            } else if item_idx < items.len() {
                let item = &items[item_idx];
                let (fg, bg) = if item.is_selected {
                    (hdr_fg, sel_bg)
                } else {
                    (item_fg, row_bg)
                };
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', fg, bg);
                }
                let indent = item.indent as usize * 2;
                let text = format!("{:indent$}{}", "", item.text, indent = indent);
                // Leave rightmost column for scrollbar if needed
                let max_text_w = if items.len() > sec_height {
                    (area.width as usize).saturating_sub(1)
                } else {
                    area.width as usize
                };
                for (i, ch) in text.chars().enumerate().take(max_text_w) {
                    set_cell(buf, area.x + i as u16, row_y, ch, fg, bg);
                }
            } else {
                // Past end of items — blank row
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, row_y, ' ', item_fg, row_bg);
                }
            }
            row_y += 1;
        }

        // Draw scrollbar in the rightmost column if items exceed visible height
        if items.len() > sec_height && sec_height > 0 && area.width > 1 {
            let sb_x = area.x + area.width - 1;
            let sb_start_y = row_y - sec_height as u16;
            let thumb_size = ((sec_height * sec_height) / total_items).max(1);
            let thumb_pos = if total_items <= sec_height {
                0
            } else {
                (scroll_off * sec_height) / (total_items - sec_height)
            };
            let thumb_pos = thumb_pos.min(sec_height.saturating_sub(thumb_size));
            for r in 0..sec_height {
                let in_thumb = r >= thumb_pos && r < thumb_pos + thumb_size;
                let ch = if in_thumb { '█' } else { '░' };
                let fg = if in_thumb { thumb_fg } else { track_fg };
                let sy = sb_start_y + r as u16;
                if sy < max_y {
                    set_cell(buf, sb_x, sy, ch, fg, sb_bg);
                }
            }
        }
    }
}

/// Render the bottom panel tab bar (Terminal | Debug Output).
pub(super) fn render_bottom_panel_tabs(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    active: render::BottomPanelKind,
    has_terminal: bool,
    has_debug_output: bool,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let tab_bg = rc(theme.tab_bar_bg);
    let active_fg = rc(theme.tab_active_fg);
    let inactive_fg = rc(theme.tab_inactive_fg);

    // Fill background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', inactive_fg, tab_bg);
    }

    let all_tabs = [
        (
            "  Terminal  ",
            render::BottomPanelKind::Terminal,
            has_terminal,
        ),
        (
            "  Debug Output  ",
            render::BottomPanelKind::DebugOutput,
            has_debug_output,
        ),
    ];
    let mut cur_x = area.x;
    for (label, kind, visible) in &all_tabs {
        if !visible {
            continue;
        }
        let fg = if *kind == active {
            active_fg
        } else {
            inactive_fg
        };
        for (i, ch) in label.chars().enumerate() {
            let x = cur_x + i as u16;
            if x >= area.x + area.width {
                break;
            }
            set_cell(buf, x, area.y, ch, fg, tab_bg);
        }
        cur_x += label.len() as u16;
        if cur_x >= area.x + area.width {
            break;
        }
    }

    // Close button (×) at right edge
    let close_x = area.x + area.width.saturating_sub(2);
    if close_x > cur_x {
        set_cell(buf, close_x, area.y, '\u{00d7}', inactive_fg, tab_bg); // ×
    }
}

/// Render the debug output tab content with a scrollbar.
/// `scroll` = 0 shows the newest lines (bottom); larger values scroll toward older lines.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_debug_output(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    output_lines: &[String],
    scroll: usize,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.foreground);
    let row_bg = rc(theme.tab_bar_bg);
    let sb_active = rc(theme.scrollbar_thumb);
    let sb_track = rc(theme.separator);

    // Header row
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    let hdr_text = " DEBUG OUTPUT";
    for (i, ch) in hdr_text.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    let content_rows = area.height.saturating_sub(1) as usize;
    let total = output_lines.len();
    let max_scroll = total.saturating_sub(content_rows);
    let scroll = scroll.min(max_scroll);
    let show_sb = total > content_rows;
    // Index of the first visible line (0 = oldest).
    // scroll=0 → show lines [max_scroll..total]; scroll=max_scroll → show [0..content_rows].
    let start_idx = max_scroll.saturating_sub(scroll);
    let text_width = if show_sb {
        area.width.saturating_sub(1) as usize
    } else {
        area.width as usize
    };
    let sb_x = area.x + area.width - 1;

    // Content rows
    for row in 0..content_rows {
        let ry = area.y + 1 + row as u16;
        if ry >= area.y + area.height {
            break;
        }
        for x in area.x..area.x + text_width as u16 {
            set_cell(buf, x, ry, ' ', item_fg, row_bg);
        }
        if let Some(line_text) = output_lines.get(start_idx + row) {
            let text = format!("  {line_text}");
            for (i, ch) in text.chars().enumerate().take(text_width) {
                set_cell(buf, area.x + i as u16, ry, ch, item_fg, row_bg);
            }
        }
    }

    // Scrollbar
    if show_sb {
        let thumb_size = (content_rows * content_rows)
            .div_ceil(total)
            .max(1)
            .min(content_rows);
        let available = content_rows.saturating_sub(thumb_size);
        // scroll=0 → thumb at bottom; scroll=max_scroll → thumb at top
        let thumb_top = if max_scroll > 0 {
            (available as f64 * (max_scroll - scroll) as f64 / max_scroll as f64).round() as usize
        } else {
            0
        };
        for i in 0..content_rows {
            let sy = area.y + 1 + i as u16;
            let ch = if i >= thumb_top && i < thumb_top + thumb_size {
                '█'
            } else {
                '░'
            };
            let fg = if i >= thumb_top && i < thumb_top + thumb_size {
                sb_active
            } else {
                sb_track
            };
            set_cell(buf, sb_x, sy, ch, fg, row_bg);
        }
    }
}

// ─── Quickfix panel ───────────────────────────────────────────────────────────

pub(super) fn render_quickfix_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    qf: &render::QuickfixPanel,
    scroll_top: usize,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = rc(theme.status_fg);
    let hdr_bg = rc(theme.status_bg);
    let item_fg = rc(theme.fuzzy_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let row_bg = rc(theme.background);

    // Header row
    let focus_mark = if qf.has_focus { " [FOCUS]" } else { "" };
    let header = format!(" QUICKFIX ({} items){}", qf.total_items, focus_mark);
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in header.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    // Result rows
    let visible_rows = area.height.saturating_sub(1) as usize;
    for row_idx in 0..visible_rows {
        let item_idx = scroll_top + row_idx;
        let ry = area.y + 1 + row_idx as u16;
        if ry >= area.y + area.height {
            break;
        }
        let is_selected = item_idx == qf.selected_idx;
        let bg = if is_selected { sel_bg } else { row_bg };
        // Clear the row
        for x in area.x..area.x + area.width {
            set_cell(buf, x, ry, ' ', item_fg, bg);
        }
        if item_idx < qf.items.len() {
            let prefix = if is_selected { "▶ " } else { "  " };
            let text = format!("{}{}", prefix, qf.items[item_idx]);
            for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, ry, ch, item_fg, bg);
            }
        }
    }
}

// ─── Terminal panel ───────────────────────────────────────────────────────────

/// Nerd Font icons for the terminal toolbar.
pub(super) const NF_TERMINAL_CLOSE: &str = "󰅖"; // nf-md-close_box
pub(super) const NF_TERMINAL_SPLIT: &str = "󰤼"; // nf-md-view_split_vertical

pub(super) fn render_terminal_panel(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    panel: &render::TerminalPanel,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = RColor::Rgb(theme.status_fg.r, theme.status_fg.g, theme.status_fg.b);
    let hdr_bg = RColor::Rgb(theme.status_bg.r, theme.status_bg.g, theme.status_bg.b);

    // ── Toolbar row ──────────────────────────────────────────────────────────
    // Clear toolbar background
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }

    if panel.find_active {
        // Find bar mode: show query and match count in toolbar
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
        let find_str = format!(" FIND: {}█{}", panel.find_query, match_info);
        let max_chars = area.width.saturating_sub(3) as usize;
        for (i, ch) in find_str.chars().enumerate().take(max_chars) {
            set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
        }
        // Close icon right-aligned
        for (i, ch) in NF_TERMINAL_CLOSE.chars().enumerate() {
            let x = area.x + area.width.saturating_sub(1 + i as u16);
            set_cell(buf, x, area.y, ch, hdr_fg, hdr_bg);
        }
    } else {
        // Tab strip — each tab is exactly 4 chars: "[N] "
        const TERMINAL_TAB_COLS: u16 = 4;
        let mut cursor_x = area.x;
        for i in 0..panel.tab_count {
            let label: Vec<char> = format!("[{}] ", i + 1).chars().collect();
            let (tab_fg, tab_bg) = if i == panel.active_tab {
                (hdr_bg, hdr_fg) // inverted for active tab
            } else {
                (hdr_fg, hdr_bg)
            };
            for (j, &ch) in label.iter().enumerate().take(TERMINAL_TAB_COLS as usize) {
                let x = cursor_x + j as u16;
                if x >= area.x + area.width {
                    break;
                }
                set_cell(buf, x, area.y, ch, tab_fg, tab_bg);
            }
            cursor_x += TERMINAL_TAB_COLS;
            if cursor_x >= area.x + area.width {
                break;
            }
        }

        // If no tabs yet, show minimal title
        if panel.tab_count == 0 {
            for (i, ch) in " TERMINAL".chars().enumerate().take(area.width as usize) {
                set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
            }
        }

        // Right-aligned icons: + ⊞ ×
        let icons = format!("+ {} {}", NF_TERMINAL_SPLIT, NF_TERMINAL_CLOSE);
        let icon_chars: Vec<char> = icons.chars().collect();
        let icon_start = area.width.saturating_sub(icon_chars.len() as u16 + 1);
        for (i, &ch) in icon_chars.iter().enumerate() {
            set_cell(
                buf,
                area.x + icon_start + i as u16,
                area.y,
                ch,
                hdr_fg,
                hdr_bg,
            );
        }
    }

    // ── Scrollbar geometry ────────────────────────────────────────────────────
    let content_rows = area.height.saturating_sub(1) as usize;
    let sb_col = area.x + area.width.saturating_sub(1);
    // Compute thumb range (row indices into the content area).
    let total = panel.scrollback_rows + content_rows;
    let (thumb_start, thumb_end) = if panel.scrollback_rows == 0 || area.width < 2 {
        (0, content_rows) // no scrollback → full bar
    } else {
        let thumb_h = ((content_rows * content_rows) / total).max(1);
        let max_off = panel.scrollback_rows;
        // scroll_offset=0 → thumb at bottom (live view); max_off → thumb at top.
        let max_top = content_rows.saturating_sub(thumb_h);
        let thumb_top = {
            let frac = 1.0 - (panel.scroll_offset as f64 / max_off as f64).min(1.0);
            (frac * max_top as f64) as usize
        };
        (thumb_top, (thumb_top + thumb_h).min(content_rows))
    };

    // ── Split view: left pane | divider | right pane ─────────────────────────
    if let Some(ref left_rows) = panel.split_left_rows {
        let half_w = panel.split_left_cols; // left-pane column count (may reflect drag state)
        let div_col = area.x + half_w;

        for row_idx in 0..content_rows {
            let screen_row = area.y + 1 + row_idx as u16;
            if screen_row >= area.y + area.height {
                break;
            }
            let term_bg = rc(theme.terminal_bg);

            // Clear both halves.
            for x in area.x..area.x + area.width.saturating_sub(1) {
                set_cell(buf, x, screen_row, ' ', hdr_fg, term_bg);
            }

            // Left pane cells.
            render_terminal_pane_cells(buf, left_rows, area.x, screen_row, half_w, row_idx, theme);

            // Divider column.
            let div_fg = rc(theme.separator);
            set_cell(buf, div_col, screen_row, '│', div_fg, term_bg);

            // Right pane cells.
            render_terminal_pane_cells(
                buf,
                &panel.rows,
                div_col + 1,
                screen_row,
                half_w,
                row_idx,
                theme,
            );

            // Scrollbar in the last column.
            let (sb_char, sb_fg) = if row_idx >= thumb_start && row_idx < thumb_end {
                ('█', rc(theme.scrollbar_thumb))
            } else {
                ('░', rc(theme.separator))
            };
            set_cell(
                buf,
                sb_col,
                screen_row,
                sb_char,
                sb_fg,
                rc(theme.background),
            );
        }

        return;
    }

    // ── Normal single-pane content rows ──────────────────────────────────────
    let cell_width = area.width.saturating_sub(1); // leave last col for scrollbar
    for row_idx in 0..content_rows {
        let screen_row = area.y + 1 + row_idx as u16;
        if screen_row >= area.y + area.height {
            break;
        }
        let term_bg_default = rc(theme.terminal_bg);
        // Clear row with terminal default background (excluding scrollbar col).
        for x in area.x..area.x + cell_width {
            set_cell(buf, x, screen_row, ' ', hdr_fg, term_bg_default);
        }

        render_terminal_pane_cells(
            buf,
            &panel.rows,
            area.x,
            screen_row,
            cell_width,
            row_idx,
            theme,
        );

        // Scrollbar column — same colors as the editor scrollbar.
        let (sb_char, sb_fg) = if row_idx >= thumb_start && row_idx < thumb_end {
            ('█', rc(theme.scrollbar_thumb))
        } else {
            ('░', rc(theme.separator))
        };
        set_cell(
            buf,
            sb_col,
            screen_row,
            sb_char,
            sb_fg,
            rc(theme.background),
        );
    }
}

/// Render one row of terminal pane cells into a ratatui buffer.
pub(super) fn render_terminal_pane_cells(
    buf: &mut ratatui::buffer::Buffer,
    rows: &[Vec<render::TerminalCell>],
    start_x: u16,
    screen_row: u16,
    max_cols: u16,
    row_idx: usize,
    theme: &Theme,
) {
    if row_idx >= rows.len() {
        return;
    }
    let row = &rows[row_idx];
    for (col_idx, cell) in row.iter().enumerate() {
        let x = start_x + col_idx as u16;
        if x >= start_x + max_cols {
            break;
        }
        let fg = RColor::Rgb(cell.fg.0, cell.fg.1, cell.fg.2);
        let bg = RColor::Rgb(cell.bg.0, cell.bg.1, cell.bg.2);
        let (draw_fg, draw_bg) = if cell.is_cursor || cell.selected {
            (bg, fg)
        } else if cell.is_find_active {
            (rc(theme.search_match_fg), rc(theme.search_current_match_bg))
        } else if cell.is_find_match {
            (rc(theme.search_match_fg), rc(theme.search_match_bg))
        } else {
            (fg, bg)
        };
        let ch = if cell.ch == '\0' { ' ' } else { cell.ch };
        let mut modifier = Modifier::empty();
        if cell.bold {
            modifier |= Modifier::BOLD;
        }
        if cell.italic {
            modifier |= Modifier::ITALIC;
        }
        if cell.underline {
            modifier |= Modifier::UNDERLINED;
        }
        if modifier.is_empty() {
            set_cell(buf, x, screen_row, ch, draw_fg, draw_bg);
        } else {
            set_cell_styled(buf, x, screen_row, ch, draw_fg, draw_bg, modifier, None);
        }
    }
}
