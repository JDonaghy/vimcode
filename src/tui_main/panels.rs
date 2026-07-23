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
            render_search_panel(backend, area, engine, theme);
            return;
        }
        Some(PANEL_DEBUG) => {
            render_debug_sidebar(backend, area, engine, theme);
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
    // Do NOT open a nested `enter_frame_scope` here — `render_sidebar` is
    // called from `draw_frame`, which already runs inside the caller's
    // single `with_frame_scope` (see mod.rs's `terminal.draw` closures).
    // Re-entering would just be a no-op round trip on `current_frame_ptr`,
    // but it contradicts the "entered once per draw closure" invariant.
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    engine.explorer_tree.borrow().render(backend, q_rect);

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

    let fg = rc(theme.foreground);
    let bg = rc(theme.tab_bar_bg);

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
    // Stage 1 scope note: `draw_settings_chrome` is a free rasteriser with no
    // `Backend::draw_*` trait equivalent (checked against quadraui's Backend
    // trait), so calling it directly on `buf` is correct and out of scope here.
    quadraui::tui::draw_settings_chrome(
        buf,
        chrome_area,
        " SETTINGS",
        &engine.settings_query,
        "",
        engine.settings_input_active,
        &super::quadraui_tui::q_theme(theme),
    );

    // Rows 2+: scrollable form content, via the shared `quadraui::Form` +
    // `FormController` primitive (#479). Inline-edit rows are driven
    // through `FieldKind::TextInput` with a cursor (see
    // `render::settings_to_form`) so there is no separate manual
    // renderer for the edit-in-progress state.
    let content_start = area.y + 2;
    let content_height = area.height.saturating_sub(2) as usize;
    if content_height == 0 {
        return;
    }

    render::populate_settings_form_controller(engine);
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        content_start as f32,
        area.width as f32,
        content_height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    engine
        .settings_form_controller
        .borrow_mut()
        .render_and_cache(backend, q_rect);
}

/// Render the project search panel via SidebarSystem (Form + TreeView).
pub(super) fn render_search_panel(
    backend: &mut super::backend::TuiBackend,
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
    engine
        .search_sidebar_system
        .borrow()
        .render(backend, q_rect);
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
    let dim_fg = rc(theme.line_number_fg);

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
    let branch_info = render::sc_header_text(sc);
    for x in area.x..area.x + area.width {
        set_cell(buf, x, area.y, ' ', hdr_fg, hdr_bg);
    }
    for (i, ch) in branch_info.chars().enumerate().take(area.width as usize) {
        set_cell(buf, area.x + i as u16, area.y, ch, hdr_fg, hdr_bg);
    }

    if area.height < 2 {
        return;
    }

    // ── Row 1+: commit input box (quadraui::TextInput, #480) ─────────────────
    // Migrated from a hand-rolled `set_cell` multi-line editor to the shared
    // `TextInput` primitive (quadraui#222). `commit_box_h` includes the
    // primitive's 1-row border on top and bottom — see
    // `render::sc_commit_input_box_height` doc for why this height is the
    // single source of truth shared with `mouse.rs`'s click hit-test.
    let ti = render::sc_commit_message_to_text_input(sc);
    let commit_box_h = render::sc_commit_input_box_height(&sc.commit_message);
    {
        let paint_h = commit_box_h.min(area.height.saturating_sub(1));
        let ti_rect = quadraui::Rect::new(
            area.x as f32,
            (area.y + 1) as f32,
            area.width as f32,
            paint_h as f32,
        );
        use quadraui::Backend;
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_text_input(ti_rect, &ti);
    }

    if area.height < 1 + commit_box_h {
        return;
    }

    // ── Bottom slab: toolbar slot + sections via SidebarPanel (#509) ──────────
    // Passes the entire remaining area (just below commit input) to
    // draw_sc_sidebar_panel, which reserves one toolbar-height row for the
    // button row and returns content_bounds for the sections below. No
    // per-side padding rows — option (a) from the issue: tighter layout,
    // zero manual arithmetic.
    {
        let slab_y = area.y + 1 + commit_box_h;
        let slab_h = (area.y + area.height).saturating_sub(slab_y);
        let slab_rect = quadraui::Rect::new(
            area.x as f32,
            slab_y as f32,
            area.width as f32,
            slab_h as f32,
        );
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        render::draw_sc_sidebar_panel(backend, engine, sc, slab_rect);
    }

    // Read section-area origin from the cached layout.
    let section_start_y = {
        let l = engine.sc_panel_layout.borrow();
        l.as_ref()
            .map(|l| l.content_bounds.y as u16)
            .unwrap_or(area.y + 1 + commit_box_h + 1) // fallback: btn row + 1
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
    engine.sc_sidebar_system.borrow().render(backend, q_rect);
    // ── Branch picker / create popup (quadraui::Palette dual-mode, #480) ─────
    // Migrated from a hand-rolled popup to the dual-mode `Palette` primitive
    // shipped in quadraui#224 (list mode = switch branch, input mode =
    // create branch). Scroll is authoritative in the TUI rasteriser (keeps
    // `selected_idx` in view), so no manual scroll-offset math is needed
    // here the way the hand-rolled version required.
    if let Some(ref bp) = sc.branch_picker {
        let palette = render::sc_branch_picker_to_palette(bp);
        let popup_w = area.width.saturating_sub(2).min(40);
        let popup_h = if bp.create_mode {
            4u16
        } else {
            area.height.saturating_sub(4).min(15)
        };
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + 2;
        let q_rect = quadraui::Rect::new(
            popup_x as f32,
            popup_y as f32,
            popup_w as f32,
            popup_h as f32,
        );
        use quadraui::Backend;
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_palette(q_rect, &palette);
    }

    // ── Help dialog (quadraui::Dialog + DialogTable, #480) ───────────────────
    // Migrated from a hand-rolled 2-column popup to `Dialog`'s table slot,
    // shipped in quadraui#225. Bindings list lives once in
    // `render::sc_help_dialog` instead of being duplicated per backend.
    if sc.help_open {
        use quadraui::Backend;
        let viewport = quadraui::Rect::new(
            area.x as f32,
            area.y as f32,
            area.width as f32,
            area.height as f32,
        );
        let (dialog, layout) = render::sc_help_dialog_layout(viewport, 1.0, 1.0);
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        let _ = backend.draw_dialog(&dialog, &layout);
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
    // Stage 1 scope note: `draw_settings_chrome` is a free rasteriser with no
    // `Backend::draw_*` trait equivalent (checked against quadraui's Backend
    // trait), so calling it directly on the buffer is correct and out of scope here.
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
        use quadraui::Backend;
        backend.set_current_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_tree(body_q_rect, &tree);

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
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn render_panel_hover_popup(
    backend: &mut super::backend::TuiBackend,
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

    use quadraui::Backend;
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.draw_rich_text_popup(&popup, &layout);

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
/// `backend` is `&mut dyn quadraui::Backend` (not the concrete `TuiBackend`)
/// so this is callable from `TuiShellApp::render_content` (#601) — see
/// `render_impl.rs::render_tab_bar`'s doc comment for the general rationale.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn render_editor_hover_popup(
    backend: &mut dyn quadraui::Backend,
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

    backend.set_theme(super::quadraui_tui::q_theme(theme));
    backend.draw_rich_text_popup(&popup, &layout);

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
    engine.ext_sidebar_system.borrow().render(backend, msv_rect);
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
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    use quadraui::Backend;

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
    let _ = backend.draw_status_bar(title_rect, &title_bar, None, None);

    if area.height < 2 {
        return;
    }

    let action_rect =
        quadraui::Rect::new(area.x as f32, (area.y + 1) as f32, area.width as f32, 1.0);
    backend.set_current_theme(q_theme);
    let hits = backend.draw_status_bar(action_rect, &action_bar, None, None);
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
    engine.dap_sidebar_system.borrow().render(backend, msv_rect);
}

/// Render the bottom panel tab bar (Terminal | Debug Output) via
/// `quadraui::Backend::draw_tab_bar`. Returns `TabBarHits` for the
/// click handler (caller caches on `engine.bottom_tab_bar_hits`).
pub(super) fn render_bottom_panel_tabs(
    backend: &mut super::backend::TuiBackend,
    area: Rect,
    active: &render::BottomPanelKind,
    has_terminal: bool,
    has_debug_output: bool,
    theme: &Theme,
) -> quadraui::TabBarHits {
    use quadraui::Backend;
    let bar = render::build_bottom_panel_tab_bar(active, has_terminal, has_debug_output);
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_current_theme(super::quadraui_tui::q_theme(theme));
    backend.draw_tab_bar(q_rect, &bar, None)
}

// ─── Quickfix panel ───────────────────────────────────────────────────────────

pub(super) fn render_quickfix_panel(
    area: Rect,
    qf: &render::QuickfixPanel,
    scroll_top: usize,
    theme: &Theme,
    backend: &mut super::backend::TuiBackend,
) {
    use quadraui::Backend;
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
    backend.draw_list(q_rect, &list);
}

// ─── Terminal panel ───────────────────────────────────────────────────────────

/// Render the terminal toolbar row (find bar or tab strip) through
/// quadraui primitives. Returns cached hit data for click dispatch.
pub(super) fn render_terminal_toolbar(
    backend: &mut super::backend::TuiBackend,
    area: Rect,
    panel: &render::TerminalPanel,
    theme: &Theme,
) -> crate::core::engine::TerminalToolbarHits {
    use crate::core::engine::TerminalToolbarHits;
    use quadraui::Backend;

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
            let _regions = backend.draw_status_bar(q_rect, &bar, None, None);
            let layout = bar.layout(area.width as f32, 1.0, 2.0, |seg| {
                quadraui::StatusSegmentMeasure::new(seg.text.chars().count() as f32)
            });
            TerminalToolbarHits::FindBar {
                layout,
                origin_x: area.x as f64,
            }
        }
        render::TerminalToolbar::TabStrip(bar) => {
            let hits = backend.draw_tab_bar(q_rect, &bar, None);
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
    {
        use quadraui::Backend;
        if let Some(split) = &td.split {
            let left = td.left.as_ref().unwrap();
            let right = td.right.as_ref().unwrap();
            backend.draw_terminal(split.left, left);
            backend.draw_terminal(split.right, right);
            // Stage 1 scope note: `draw_terminal_divider` is a free rasteriser
            // with no `Backend::draw_*` trait equivalent (checked against
            // quadraui's Backend trait), so calling it directly is correct
            // and out of scope here.
            quadraui::tui::draw_terminal_divider(
                frame.buffer_mut(),
                split.divider_x as u16,
                area.y,
                area.height,
                &q_theme,
            );
        } else if let Some(ref term) = td.single {
            backend.draw_terminal(q_area, term);
        }
    }
}

// ─── Source Control panel rendering tests (#480) ─────────────────────────────
//
// Drives `render_source_control` through the same headless
// `ratatui::Terminal<TestBackend>` harness `render_impl.rs`'s test module
// uses for full-frame rendering — vimcode's equivalent of quadraui's
// `TuiDriver`. Exercises the migrated `TextInput` / dual-mode `Palette` /
// `Dialog`+`DialogTable` paint paths end-to-end (build_screen_layout →
// render_source_control → backend rasterisers) rather than only unit-testing
// the `render::sc_*` adapters in isolation, so a regression in the wiring
// (wrong rect, wrong field) would show up as a rendered-buffer mismatch.
#[cfg(test)]
mod sc_panel_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    /// Hermetic engine with the Source Control panel active and focused.
    /// Resets git-derived fields so snapshots don't depend on the repo
    /// state of whatever machine/branch the test happens to run on.
    fn test_engine() -> Engine {
        crate::core::session::suppress_disk_saves();
        let mut e = Engine::new();
        e.settings = crate::core::settings::Settings::default();
        e.extension_state = crate::core::session::ExtensionState::default();
        e.ext_registry = None;
        e.git_branch = None;
        e.sc_ahead = 0;
        e.sc_behind = 0;
        e.sc_has_focus = true;
        e.app_shell.show_panel(&quadraui::WidgetId::new(PANEL_GIT));
        e
    }

    /// Render just the SC panel and return the rasterised buffer as lines.
    fn render_sc(engine: &Engine, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let theme = crate::render::Theme::onedark();
        let mut tui_backend = super::super::backend::TuiBackend::new();
        let area = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        terminal
            .draw(|frame| {
                // #600: `render_source_control` calls `Backend::draw_*` trait
                // methods directly now (no per-call `enter_frame_scope`), so
                // this harness needs to open the scope itself — mirrors what
                // `event_loop`'s two `terminal.draw` closures do in `mod.rs`.
                super::with_frame_scope(&mut tui_backend, frame, |backend, frame| {
                    render_source_control(backend, frame, area, engine, &theme);
                });
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let mut line = String::new();
                for x in 0..width {
                    line.push_str(buf[(x, y)].symbol());
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    fn contains(lines: &[String], substr: &str) -> bool {
        lines.iter().any(|l| l.contains(substr))
    }

    #[test]
    fn empty_commit_message_shows_placeholder() {
        let e = test_engine();
        let lines = render_sc(&e, 40, 20);
        assert!(
            contains(&lines, "Message (press c)"),
            "expected commit-input placeholder, got: {lines:#?}"
        );
    }

    #[test]
    fn active_commit_input_renders_typed_message_not_placeholder() {
        let mut e = test_engine();
        e.sc_commit_message = "Fix the thing".to_string();
        e.sc_commit_cursor = e.sc_commit_message.len();
        e.sc_commit_input_active = true;
        let lines = render_sc(&e, 40, 20);
        assert!(
            contains(&lines, "Fix the thing"),
            "expected typed commit message, got: {lines:#?}"
        );
        assert!(
            !contains(&lines, "Message (press c)"),
            "placeholder should not show while actively editing, got: {lines:#?}"
        );
    }

    #[test]
    fn multiline_commit_message_renders_every_line() {
        let mut e = test_engine();
        e.sc_commit_message = "Summary line\n\nBody line one\nBody line two".to_string();
        e.sc_commit_cursor = 0;
        e.sc_commit_input_active = true;
        // Tall enough for the multi-line TextInput box + toolbar + sections.
        let lines = render_sc(&e, 40, 24);
        assert!(contains(&lines, "Summary line"), "{lines:#?}");
        assert!(contains(&lines, "Body line one"), "{lines:#?}");
        assert!(contains(&lines, "Body line two"), "{lines:#?}");
    }

    #[test]
    fn branch_picker_list_mode_renders_branches_and_marks_current() {
        let mut e = test_engine();
        e.sc_branch_picker_open = true;
        e.sc_branch_picker_branches = vec![
            crate::core::git::BranchEntry {
                name: "main".to_string(),
                is_current: true,
                upstream: None,
                ahead_behind: None,
            },
            crate::core::git::BranchEntry {
                name: "feature/foo".to_string(),
                is_current: false,
                upstream: None,
                ahead_behind: None,
            },
        ];
        let lines = render_sc(&e, 50, 24);
        assert!(contains(&lines, "Switch Branch"), "{lines:#?}");
        assert!(contains(&lines, "main"), "{lines:#?}");
        assert!(contains(&lines, "feature/foo"), "{lines:#?}");
    }

    #[test]
    fn branch_picker_create_mode_renders_typed_name() {
        let mut e = test_engine();
        e.sc_branch_create_mode = true;
        e.sc_branch_create_input = "wip-feature".to_string();
        let lines = render_sc(&e, 50, 24);
        assert!(contains(&lines, "New Branch"), "{lines:#?}");
        assert!(contains(&lines, "wip-feature"), "{lines:#?}");
    }

    #[test]
    fn help_dialog_renders_keybindings_table() {
        let mut e = test_engine();
        e.sc_help_open = true;
        let lines = render_sc(&e, 60, 24);
        assert!(contains(&lines, "Keybindings"), "{lines:#?}");
        assert!(contains(&lines, "Navigate"), "{lines:#?}");
        assert!(contains(&lines, "Close"), "{lines:#?}");
    }

    #[test]
    fn renders_without_panicking_at_minimum_size() {
        // Regression guard: the migrated TextInput/Palette/Dialog primitives
        // must degrade gracefully instead of panicking when the panel is
        // squeezed very small (e.g. a tiny terminal or heavily split window).
        let mut e = test_engine();
        e.sc_commit_message = "line one\nline two".to_string();
        e.sc_commit_input_active = true;
        let _ = render_sc(&e, 10, 3);
        e.sc_help_open = true;
        let _ = render_sc(&e, 10, 3);
    }
}
