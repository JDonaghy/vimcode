use super::*;

pub(super) fn render_activity_bar(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    sidebar: &TuiSidebar,
    theme: &Theme,
    _menu_bar_visible: bool,
    engine: &Engine,
) {
    // Delegate to the shared adapter in render.rs (#133). TUI includes the
    // hamburger item (index 0) because there is no native menu bar.
    let bar =
        crate::render::build_activity_bar(engine, theme, true, sidebar.ext_panel_name.as_deref());
    super::quadraui_tui::draw_activity_bar(buf, area, &bar, theme);
}

// ─── Sidebar rendering ────────────────────────────────────────────────────────

pub(super) fn render_sidebar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    sidebar: &mut TuiSidebar,
    engine: &Engine,
    theme: &Theme,
    _explorer_drop_target: Option<usize>,
) {
    let buf = frame.buffer_mut();
    let default_fg = rc(theme.explorer_file_fg);
    let row_bg = rc(theme.tab_bar_bg);

    // Extension panel (plugin-provided)
    if sidebar.ext_panel_name.is_some() {
        // Drop the buffer borrow before passing frame to render_ext_panel
        // — the new TreeView-based renderer takes the backend + frame so it
        // can route draw calls through quadraui primitives.
        let _ = buf;
        render_ext_panel(backend, frame, area, engine, theme);
        return;
    }

    let active_id = engine.app_shell.active_panel_id().map(|w| w.as_str());
    match active_id {
        Some(PANEL_SETTINGS) => {
            render_settings_panel(backend, frame, area, theme, engine);
            return;
        }
        Some(PANEL_SEARCH) => {
            render_search_panel(backend, frame, area, engine, theme);
            return;
        }
        Some(PANEL_DEBUG) => {
            render_debug_sidebar(backend, frame, area, engine, theme);
            return;
        }
        Some(PANEL_GIT) => {
            render_source_control(backend, frame, area, engine, theme);
            return;
        }
        Some(PANEL_EXTENSIONS) => {
            render_ext_sidebar(backend, frame, area, engine, theme);
            return;
        }
        Some(PANEL_AI) => {
            render_ai_sidebar(buf, area, engine, theme);
            return;
        }
        _ => {}
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

    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    engine.explorer_tree_rect.set(q_rect);
    engine.explorer_viewport_rows.set(area.height as usize);
    render::populate_explorer_tree_controller(engine, theme);
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        engine.explorer_tree.borrow().render(b, q_rect);
    });

    // TreeController.render() draws the scrollbar internally.
    // Register a ScrollSurface for scroll-wheel dispatch only.
    engine
        .scroll_surfaces
        .borrow_mut()
        .push(quadraui::ScrollSurface {
            id: quadraui::WidgetId::new("explorer:sb"),
            bounds: quadraui::Rect::new(
                area.x as f32,
                area.y as f32,
                area.width as f32,
                area.height as f32,
            ),
            scrollbar: None,
        });
}

/// Render the settings panel — shows current key settings and the file path.
///
/// B5c.4: routes the form rendering through `Backend::draw_form` so
/// the form rasteriser and call site share the same code path GTK
/// uses. The buffer-only chrome (background fill, focus border)
/// stays inline.
pub(super) fn render_settings_panel(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    theme: &Theme,
    engine: &Engine,
) {
    let buf = frame.buffer_mut();
    use crate::core::settings::{setting_categories, SettingType, SETTING_DEFS};

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

    // Rows 0–1: header + search input chrome.
    let chrome_h = area.height.min(2);
    let chrome_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: chrome_h,
    };
    quadraui::tui::draw_settings_chrome(
        buf,
        chrome_area,
        " SETTINGS",
        &engine.settings_query,
        "",
        engine.settings_input_active,
        &super::quadraui_tui::q_theme(theme),
    );

    // Rows 2+: scrollable form content
    let content_start = area.y + 2;
    let content_height = area.height.saturating_sub(2) as usize;
    if content_height == 0 {
        return;
    }

    // Phase A.3b migration: when no inline edit is active, render the
    // field list via the shared `quadraui::Form` primitive. The legacy
    // inline renderer below still handles inline-edit modes (integer /
    // string cursor, enum cycling UI) until the `Form` primitive gains
    // text-cursor support.
    let has_inline_edit =
        engine.settings_editing.is_some() || engine.ext_settings_editing.is_some();
    if !has_inline_edit {
        render::populate_settings_form_controller(engine);
        let q_rect = quadraui::Rect::new(
            area.x as f32,
            content_start as f32,
            area.width as f32,
            content_height as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            engine
                .settings_form_controller
                .borrow_mut()
                .render_and_cache(b, q_rect);
        });
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
    let settings_scrollbar = if total > content_height && content_height > 0 {
        let sb_thumb = rc(theme.scrollbar_thumb);
        let sb_track = rc(theme.scrollbar_track);
        let sb_bg = rc(theme.background);
        let track_len = content_height;
        let thumb_len = (content_height * content_height / total).max(1);
        let thumb_start = scroll * track_len / total;
        for i in 0..track_len {
            let y = content_start + i as u16;
            let (ch, cfp) = if i >= thumb_start && i < thumb_start + thumb_len {
                ('█', sb_thumb)
            } else {
                ('░', sb_track)
            };
            set_cell(buf, sb_col, y, ch, cfp, sb_bg);
        }
        Some(quadraui::SurfaceScrollbar {
            axis: quadraui::ScrollAxis::Vertical,
            track_bounds: quadraui::Rect::new(
                sb_col as f32,
                content_start as f32,
                1.0,
                track_len as f32,
            ),
            thumb_bounds: quadraui::Rect::new(
                sb_col as f32,
                content_start as f32 + thumb_start as f32,
                1.0,
                thumb_len as f32,
            ),
            total_items: total,
            visible_items: content_height,
            scroll_offset: scroll,
            inverted: false,
        })
    } else {
        None
    };
    engine
        .scroll_surfaces
        .borrow_mut()
        .push(quadraui::ScrollSurface {
            id: quadraui::WidgetId::new("tui:settings"),
            bounds: quadraui::Rect::new(
                area.x as f32,
                content_start as f32,
                area.width as f32,
                content_height as f32,
            ),
            scrollbar: settings_scrollbar,
        });
}

/// Render the project search panel via SidebarSystem (Form + TreeView).
pub(super) fn render_search_panel(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }

    let q_len = engine.project_search_query.len();
    if engine.search_query_caret.get() > q_len {
        engine.search_query_caret.set(q_len);
    }
    let r_len = engine.project_replace_text.len();
    if engine.replace_text_caret.get() > r_len {
        engine.replace_text_caret.set(r_len);
    }

    render::populate_search_sidebar_system(engine, &engine.cwd);
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    engine.search_sidebar_body_rect.set(q_rect);

    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        engine.search_sidebar_system.borrow().render(b, q_rect);
    });
}

// ─── Status / command line ────────────────────────────────────────────────────

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
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    let buf = frame.buffer_mut();
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

    if area.height < 1 + commit_rows + 2 {
        return;
    }

    // ── Bottom slab: toolbar slot + sections via SidebarPanel (#509) ──────────
    // Passes the entire remaining area (just below commit input) to
    // draw_sc_sidebar_panel, which reserves one toolbar-height row for the
    // button row and returns content_bounds for the sections below. No
    // per-side padding rows — option (a) from the issue: tighter layout,
    // zero manual arithmetic.
    {
        let slab_y = area.y + 1 + commit_rows;
        let slab_h = (area.y + area.height).saturating_sub(slab_y);
        let slab_rect = quadraui::Rect::new(
            area.x as f32,
            slab_y as f32,
            area.width as f32,
            slab_h as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            render::draw_sc_sidebar_panel(b, engine, sc, slab_rect);
        });
    }

    // Read section-area origin from the cached layout.
    let section_start_y = {
        let l = engine.sc_panel_layout.borrow();
        l.as_ref()
            .map(|l| l.content_bounds.y as u16)
            .unwrap_or(area.y + 2 + commit_rows) // fallback: btn row + 1
    };
    if section_start_y >= area.y + area.height {
        return;
    }

    // Section rendering — migrated to `SidebarSystem` (#321).
    let section_area = Rect {
        x: area.x,
        y: section_start_y,
        width: area.width,
        height: (area.y + area.height).saturating_sub(section_start_y),
    };
    let q_rect = quadraui::Rect::new(
        section_area.x as f32,
        section_area.y as f32,
        section_area.width as f32,
        section_area.height as f32,
    );
    engine.sc_sidebar_body_rect.set(q_rect);
    render::populate_sc_sidebar_system(engine, theme);
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        engine.sc_sidebar_system.borrow().render(b, q_rect);
    });
    let buf = frame.buffer_mut();

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
///
/// Migrated to `quadraui::TreeView` (#476). Header + search-input chrome
/// route through `quadraui::tui::draw_settings_chrome`; the body rows
/// (sections + expandable tree items + badges + action labels) flow
/// through `render::ext_panel_to_tree_view()` + `Backend::draw_tree`.
/// The help-popup overlay and the scrollbar/scroll-surface registration
/// are panel-specific chrome that don't fit TreeView and stay inline.
pub(super) fn render_ext_panel(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let Some(ref panel) = screen.ext_panel else {
        return;
    };

    // ── Chrome: header (always) + search input (only when active or text). ─
    let input_visible = panel.input_active || !panel.input_text.is_empty();
    let chrome_h: u16 = (if input_visible { 2 } else { 1 }).min(area.height);
    let header_title = format!(" {}", panel.title);
    let chrome_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: chrome_h,
    };
    quadraui::tui::draw_settings_chrome(
        frame.buffer_mut(),
        chrome_area,
        &header_title,
        &panel.input_text,
        "",
        panel.input_active,
        &super::quadraui_tui::q_theme(theme),
    );

    // ── Body: TreeView rasterised via the shared primitive. ────────────────
    let body_h = area.height.saturating_sub(chrome_h);
    if body_h > 0 {
        let body_w = area.width.saturating_sub(1); // 1 col reserved for scrollbar
        let tree = render::ext_panel_to_tree_view(panel, theme);
        let body_q_rect = quadraui::Rect::new(
            area.x as f32,
            (area.y + chrome_h) as f32,
            body_w as f32,
            body_h as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_tree(body_q_rect, &tree);
        });

        // Manual scrollbar: `draw_tree` doesn't render scrollbars yet.
        // Total visible rows = tree.rows.len() (sections + their expanded
        // items, separators included — same flat count the legacy renderer
        // produced).
        let buf = frame.buffer_mut();
        let total = tree.rows.len();
        let track_h = body_h as usize;
        let ext_panel_scrollbar = if total > track_h && track_h > 0 {
            let scroll = panel.scroll_top;
            let sb_x = area.x + area.width - 1;
            let thumb_h = (track_h * track_h / total).max(1);
            let thumb_top = scroll * track_h / total;
            let sb_thumb = rc(theme.scrollbar_thumb);
            let sb_track = rc(theme.scrollbar_track);
            let sb_bg = rc(theme.background);
            for i in 0..track_h {
                let y = area.y + chrome_h + i as u16;
                let (ch, cfp) = if i >= thumb_top && i < thumb_top + thumb_h {
                    ('\u{2588}', sb_thumb)
                } else {
                    ('\u{2591}', sb_track)
                };
                set_cell(buf, sb_x, y, ch, cfp, sb_bg);
            }
            let track_start_y = (area.y + chrome_h) as f32;
            Some(quadraui::SurfaceScrollbar {
                axis: quadraui::ScrollAxis::Vertical,
                track_bounds: quadraui::Rect::new(sb_x as f32, track_start_y, 1.0, track_h as f32),
                thumb_bounds: quadraui::Rect::new(
                    sb_x as f32,
                    track_start_y + thumb_top as f32,
                    1.0,
                    thumb_h as f32,
                ),
                total_items: total,
                visible_items: track_h,
                scroll_offset: scroll,
                inverted: false,
            })
        } else {
            None
        };
        engine
            .scroll_surfaces
            .borrow_mut()
            .push(quadraui::ScrollSurface {
                id: quadraui::WidgetId::new("ext_panel:sb"),
                bounds: quadraui::Rect::new(
                    area.x as f32,
                    area.y as f32,
                    area.width as f32,
                    area.height as f32,
                ),
                scrollbar: ext_panel_scrollbar,
            });
    }

    let buf = frame.buffer_mut();

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
    let Some(ref ph) = screen.panel_hover else {
        return (vec![], None);
    };

    let lines = &ph.rendered.lines;
    if lines.is_empty() {
        return (vec![], None);
    }

    let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(10);
    // Available width to the right of the sidebar.
    let avail_w = term_area.width.saturating_sub(sidebar_right_x);
    if avail_w < 10 {
        return (vec![], None);
    }
    // Content width (excludes the 1-cell border on each side): matches
    // the legacy total-box-width clamp of `(max_len+4).clamp(12, avail_w)`.
    let content_w = ((max_len + 2) as f32)
        .max(10.0)
        .min((avail_w as f32 - 2.0).max(10.0));

    // Vertically align with the hovered item.
    let item_row = if ph.panel_name == "source_control" {
        // Derive section_start from the cached SidebarPanelLayout (#509):
        // content_bounds.y (absolute terminal row) minus sidebar_y gives the
        // sidebar-relative row where sections begin. Falls back to 2 (header +
        // btn, option-a layout) if the layout hasn't been populated yet.
        let section_start = screen
            .source_control
            .as_ref()
            .and_then(|sc| sc.sc_sections_start_y)
            .map(|y| (y as u16).saturating_sub(sidebar_y))
            .unwrap_or(2u16);
        section_start + ph.item_index as u16
    } else {
        ph.item_index as u16 + 1
    };
    let raw_y = sidebar_y + item_row;
    // Same secondary clamp the legacy renderer applied: don't let the
    // popup's top row start past the terminal or sidebar bottom edge.
    // (`height` here is an upper-bound estimate; the shared layout
    // engine reclamps precisely against the viewport below.)
    let est_height = (lines.len().min(render::PANEL_HOVER_MAX_ROWS) as u16) + 2;
    let top_row = raw_y.min(
        term_area
            .height
            .saturating_sub(est_height)
            .min(sidebar_y + sidebar_height.saturating_sub(1)),
    );

    let popup = render::panel_hover_to_quadraui_rich_text(ph, theme);
    let viewport = quadraui::Rect::new(
        term_area.x as f32,
        term_area.y as f32,
        term_area.width as f32,
        term_area.height as f32,
    );
    let measure = quadraui::RichTextPopupMeasure::new(content_w, 1.0);
    // Placement::Below adds one row height to anchor_y, so subtract it
    // here to land the box's top border exactly on `top_row`.
    let layout = popup.layout(
        sidebar_right_x as f32,
        top_row as f32 - 1.0,
        viewport,
        measure,
        |line_idx, start_byte, end_byte| {
            popup
                .line_text
                .get(line_idx)
                .map(|t| {
                    t[start_byte.min(t.len())..end_byte.min(t.len())]
                        .chars()
                        .count() as f32
                })
                .unwrap_or(0.0)
        },
    );

    super::quadraui_tui::draw_rich_text_popup(frame.buffer_mut(), &popup, &layout, theme);

    let link_rects: Vec<(u16, u16, u16, u16, String)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (
                rect.x.round() as u16,
                rect.y.round() as u16,
                rect.width.round() as u16,
                rect.height.round() as u16,
                url,
            )
        })
        .collect();

    let popup_rect = Some((
        layout.bounds.x.round() as u16,
        layout.bounds.y.round() as u16,
        layout.bounds.width.round() as u16,
        layout.bounds.height.round() as u16,
    ));

    (link_rects, popup_rect)
}

// ─── Editor hover popup ─────────────────────────────────────────────────────

/// Render an editor hover popup via the `quadraui::RichTextPopup`
/// primitive. Returns `(link_rects, popup_bounds, scrollbar_hit)` for
/// mouse hit-testing — derived from the primitive's resolved layout.
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
    Option<render::PopupScrollbarHit>,
) {
    if eh.rendered.lines.is_empty() {
        return (vec![], None, None);
    }
    let popup = render::editor_hover_to_quadraui_rich_text(eh, theme);
    // Content width: precomputed by engine (popup_width chars) clamped to
    // viewport - 4 to leave space for borders + minimum padding.
    let content_w = (eh.popup_width as f32)
        .max(10.0)
        .min((term_area.width as f32 - 4.0).max(10.0));
    let viewport = quadraui::Rect::new(
        term_area.x as f32,
        term_area.y as f32,
        term_area.width as f32,
        term_area.height as f32,
    );
    let measure = quadraui::RichTextPopupMeasure::new(content_w, 1.0);
    // TUI link widths: 1 cell per char.
    let layout = popup.layout(
        popup_x as f32,
        popup_y as f32,
        viewport,
        measure,
        |line_idx, start_byte, end_byte| {
            popup
                .line_text
                .get(line_idx)
                .map(|t| {
                    t[start_byte.min(t.len())..end_byte.min(t.len())]
                        .chars()
                        .count() as f32
                })
                .unwrap_or(0.0)
        },
    );

    super::quadraui_tui::draw_rich_text_popup(frame.buffer_mut(), &popup, &layout, theme);

    let link_rects: Vec<(u16, u16, u16, u16, String)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (
                rect.x.round() as u16,
                rect.y.round() as u16,
                rect.width.round() as u16,
                rect.height.round() as u16,
                url,
            )
        })
        .collect();

    let popup_rect = Some((
        layout.bounds.x.round() as u16,
        layout.bounds.y.round() as u16,
        layout.bounds.width.round() as u16,
        layout.bounds.height.round() as u16,
    ));
    let scrollbar_hit = layout.scrollbar.map(|sb| render::PopupScrollbarHit {
        track: sb.track,
        thumb: sb.thumb,
        visible_rows: render::EDITOR_HOVER_MAX_ROWS,
        total: popup.lines.len(),
    });
    (link_rects, popup_rect, scrollbar_hit)
}

// ─── Extensions sidebar panel ─────────────────────────────────────────────────

/// Render the Extensions sidebar panel.
///
/// Migrated to `quadraui::MultiSectionView` (#293). The panel header
/// row + search-input row stay panel-specific chrome; the two
/// "INSTALLED" / "AVAILABLE" sections (each with its own `TreeView`
/// body) are now a `MultiSectionView` built by
/// `render::ext_sidebar_to_multi_section_view` and rasterised via
/// `quadraui::tui::draw_multi_section_view`. Both the section-header
/// chevrons / titles and per-section scrollbars come from the
/// primitive — there is no per-backend section-walk code that paint
/// and click could disagree on (the structural fix for the #281 bug
/// classes).
pub(super) fn render_ext_sidebar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
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
    let default_fg = rc(theme.foreground);
    let dim_fg = rc(theme.line_number_fg);
    let sel_bg = rc(theme.fuzzy_selected_bg);
    let panel_bg = rc(theme.completion_bg);

    // ── Chrome rows: panel header (row 0) + search box (row 1) ───────────────
    {
        let buf = frame.buffer_mut();

        let write_row =
            |buf: &mut ratatui::buffer::Buffer, y: u16, text: &str, fg: RColor, bg: RColor| {
                for x in area.x..area.x + area.width {
                    set_cell(buf, x, y, ' ', fg, bg);
                }
                for (i, ch) in text.chars().enumerate().take(area.width as usize) {
                    set_cell(buf, area.x + i as u16, y, ch, fg, bg);
                }
            };

        if area.height >= 1 {
            let hdr = if ext.fetching {
                " \u{eb85} EXTENSIONS  (fetching…)".to_string()
            } else {
                " \u{eb85} EXTENSIONS".to_string()
            };
            write_row(buf, area.y, &hdr, header_fg, header_bg);
        }

        if area.height >= 2 {
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
            write_row(buf, area.y + 1, &search_text, search_fg, search_bg);
        }
    }

    // ── SidebarSystem body: rest of the panel ──────────────────────────────
    if area.height <= 2 {
        return;
    }
    let msv_rect = quadraui::Rect::new(
        area.x as f32,
        (area.y + 2) as f32,
        area.width as f32,
        (area.height - 2) as f32,
    );
    engine.ext_sidebar_body_rect.set(msv_rect);
    render::populate_ext_sidebar_system(engine);
    let q_theme = super::quadraui_tui::q_theme(theme);
    backend.set_current_theme(q_theme);
    backend.enter_frame_scope(frame, |b| {
        engine.ext_sidebar_system.borrow().render(b, msv_rect);
    });
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
    let q_user_fg = render::to_quadraui_color(theme.keyword);
    let q_asst_fg = render::to_quadraui_color(theme.string_lit);
    let q_default_fg = render::to_quadraui_color(theme.foreground);
    let q_panel_bg = render::to_quadraui_color(theme.completion_bg);
    let mut rows: Vec<quadraui::MessageRow> = Vec::new();
    for msg in &ai.messages {
        let is_user = msg.role == "user";
        let role_label = if is_user { "You:" } else { "AI:" };
        let role_fg = if is_user { q_user_fg } else { q_asst_fg };
        rows.push(quadraui::MessageRow::new(role_label, role_fg, 0.0));
        for line in msg.content.lines() {
            if line.is_empty() {
                rows.push(quadraui::MessageRow::new("", q_default_fg, 2.0));
                continue;
            }
            let chars: Vec<char> = line.chars().collect();
            let mut pos = 0;
            while pos < chars.len() {
                let end = (pos + wrap_w).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                rows.push(quadraui::MessageRow::new(chunk, q_default_fg, 2.0));
                pos = end;
            }
        }
        rows.push(quadraui::MessageRow::new("", q_panel_bg, 0.0)); // blank separator
    }

    let total = rows.len();
    let start = scroll.min(total.saturating_sub(msg_area_height as usize));
    let msg_list = quadraui::MessageList {
        id: quadraui::WidgetId::new("tui:ai:messages"),
        rows,
        scroll_top: start,
    };
    quadraui::tui::draw_message_list(
        buf,
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: msg_area_height,
        },
        &msg_list,
        q_panel_bg,
    );
    y += msg_area_height;

    // Fill any rows the message list didn't cover (when there are
    // fewer messages than the visible area).
    let painted = msg_list
        .rows
        .len()
        .saturating_sub(start)
        .min(msg_area_height as usize) as u16;
    let mut fill_y = area.y + 1 + painted;
    while fill_y < area.y + 1 + msg_area_height {
        for x in area.x..area.x + area.width {
            set_cell(buf, x, fill_y, ' ', dim_fg, panel_bg);
        }
        fill_y += 1;
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
    let cursor_line = cursor.checked_div(content_w).unwrap_or(0);
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
/// Migrated to four `quadraui::TreeView` instances (#281), one per
/// section. Panel header (row 0) + Run/Stop button (row 1) + per-section
/// title rows + per-section scrollbar overlays remain panel-specific
/// chrome; item rendering goes through `Backend::draw_tree`.
pub(super) fn render_debug_sidebar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }

    // Build minimal screen layout to get debug_sidebar data.
    let screen = render::build_screen_layout(engine, theme, &[], 1.0, 1.0, true);
    let sidebar = &screen.debug_sidebar;

    // ── Chrome rows (panel-specific): header + Run/Stop button via StatusBar. ──
    let (title_bar, action_bar) = render::debug_sidebar_chrome_to_status_bars(sidebar, theme);
    let q_theme = super::quadraui_tui::q_theme(theme);

    let title_rect = quadraui::Rect::new(area.x as f32, area.y as f32, area.width as f32, 1.0);
    backend.set_current_theme(q_theme);
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        let _ = b.draw_status_bar(title_rect, &title_bar, None, None);
    });

    if area.height < 2 {
        return;
    }

    let action_rect =
        quadraui::Rect::new(area.x as f32, (area.y + 1) as f32, area.width as f32, 1.0);
    backend.set_current_theme(q_theme);
    let hits = backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_status_bar(action_rect, &action_bar, None, None)
    });
    engine.dap_sidebar_action_hits.replace(Some(hits));

    // ── SidebarSystem body (the four sections). ──
    if area.height < 3 {
        return;
    }
    let msv_rect = quadraui::Rect::new(
        area.x as f32,
        (area.y + 2) as f32,
        area.width as f32,
        (area.height - 2) as f32,
    );
    engine.dap_sidebar_body_rect.set(msv_rect);
    render::populate_dap_sidebar_system(engine);
    backend.set_current_theme(q_theme);
    backend.enter_frame_scope(frame, |b| {
        engine.dap_sidebar_system.borrow().render(b, msv_rect);
    });
}

/// Render the bottom panel tab bar (Terminal | Debug Output) via
/// `quadraui::Backend::draw_tab_bar`. Returns `TabBarHits` for the
/// click handler (caller caches on `engine.bottom_tab_bar_hits`).
pub(super) fn render_bottom_panel_tabs(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    active: &render::BottomPanelKind,
    has_terminal: bool,
    has_debug_output: bool,
    theme: &Theme,
) -> quadraui::TabBarHits {
    let bar = render::build_bottom_panel_tab_bar(active, has_terminal, has_debug_output);
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_tab_bar(q_rect, &bar, None)
    })
}

// ─── Quickfix panel ───────────────────────────────────────────────────────────

pub(super) fn render_quickfix_panel(
    frame: &mut ratatui::Frame,
    area: Rect,
    qf: &render::QuickfixPanel,
    scroll_top: usize,
    theme: &Theme,
    backend: &mut super::backend::TuiBackend,
) {
    if area.height == 0 {
        return;
    }
    // Phase A.5 migration: quickfix panel now renders through the
    // shared `quadraui::ListView` primitive. The adapter produces a
    // ListView with a `QUICKFIX (N items)` header; `draw_list` renders
    // header + rows with selection indicator + dimmed detail.
    // Phase B.4 Stage 3a: route through `Backend::draw_list`.
    let mut list = render::quickfix_to_list_view(qf);
    list.scroll_offset = scroll_top;
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.enter_frame_scope(frame, |b| {
        use quadraui::Backend;
        b.draw_list(q_rect, &list);
    });
}

// ─── Terminal panel ───────────────────────────────────────────────────────────

/// Render the terminal toolbar row (find bar or tab strip) through
/// quadraui primitives. Returns cached hit data for click dispatch.
pub(super) fn render_terminal_toolbar(
    backend: &mut super::backend::TuiBackend,
    frame: &mut ratatui::Frame,
    area: Rect,
    panel: &render::TerminalPanel,
    theme: &Theme,
) -> crate::core::engine::TerminalToolbarHits {
    use crate::core::engine::TerminalToolbarHits;

    let toolbar = render::build_terminal_toolbar(panel, theme);
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    match toolbar {
        render::TerminalToolbar::FindBar(bar) => {
            let layout = backend.enter_frame_scope(frame, |b| {
                use quadraui::Backend;
                let _regions = b.draw_status_bar(q_rect, &bar, None, None);
                bar.layout(area.width as f32, 1.0, 2.0, |seg| {
                    quadraui::StatusSegmentMeasure::new(seg.text.chars().count() as f32)
                })
            });
            TerminalToolbarHits::FindBar {
                layout,
                origin_x: area.x as f64,
            }
        }
        render::TerminalToolbar::TabStrip(bar) => {
            let hits = backend.enter_frame_scope(frame, |b| {
                use quadraui::Backend;
                b.draw_tab_bar(q_rect, &bar, None)
            });
            TerminalToolbarHits::TabStrip(hits)
        }
    }
}

/// Render the terminal panel content via quadraui's `draw_terminal`.
pub(super) fn render_terminal_panel(
    frame: &mut ratatui::Frame,
    backend: &mut quadraui::tui::TuiBackend,
    area: Rect,
    panel: &render::TerminalPanel,
    theme: &Theme,
    engine: &Engine,
) {
    if area.height == 0 {
        return;
    }
    let content_rows = area.height as usize;
    let fg = RColor::Rgb(theme.status_fg.r, theme.status_fg.g, theme.status_fg.b);
    let term_bg = rc(theme.terminal_bg);
    let q_theme = super::quadraui_tui::q_theme(theme);

    // Clear with terminal background.
    for row in 0..area.height {
        for col in area.x..area.x + area.width {
            set_cell(frame.buffer_mut(), col, area.y + row, ' ', fg, term_bg);
        }
    }

    let q_area = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    let td = render::build_terminal_draw_data(panel, q_area, 1.0, 1.0, content_rows, None);
    engine.terminal_split_layout.replace(td.split);
    backend.set_current_theme(q_theme);
    if let Some(split) = &td.split {
        let left = td.left.as_ref().unwrap();
        let right = td.right.as_ref().unwrap();
        let sl = *split;
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_terminal(sl.left, left);
            b.draw_terminal(sl.right, right);
        });
        quadraui::tui::draw_terminal_divider(
            frame.buffer_mut(),
            split.divider_x as u16,
            area.y,
            area.height,
            &q_theme,
        );
    } else if let Some(ref term) = td.single {
        backend.enter_frame_scope(frame, |b| {
            use quadraui::Backend;
            b.draw_terminal(q_area, term);
        });
    }
}
