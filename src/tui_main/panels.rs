use super::*;

#[cfg(test)]
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

/// Render the explorer tree panel's body: background fill + the
/// `TreeController` itself + its scroll-surface registration.
///
/// #766: used to be reached two ways — the (now-deleted) `draw_frame`'s test
/// harness via a `render_sidebar` dispatcher that lived here, and
/// `TuiShellApp::render_content` (`shell_app.rs`, which never has a raw
/// `Frame`/`Buffer` — see that module's doc comment) via `render_sidebar_content`
/// (below). `draw_frame`'s test suite never asserted on sidebar content, so
/// `render_sidebar` and its panel-dispatch `match` were dead weight once it
/// was gone; `render_sidebar_content` still falls through to this function
/// for the explorer panel. The background
/// fill that used to be a raw `set_cell` loop over `frame.buffer_mut()` is
/// now painted via `Backend::draw_status_bar` with a single blank segment
/// per row — `draw_status_bar`'s TUI rasteriser always fills the *entire*
/// row with the first segment's `bg` before painting segment text
/// (`quadraui/src/tui/status_bar.rs`'s `fill_bg` loop), so an empty-text
/// segment is enough to reproduce the old solid-fill behavior exactly. This
/// is the same "solid `StatusBar` as background fill" trick quadraui's own
/// `AppShell::render` uses for its resize divider (`compose/app_shell.rs`'s
/// `divider_bounds` block) — the issue's suggested stand-in for raw
/// background fills that have no direct `Backend::draw_*` equivalent.
pub(super) fn render_explorer_sidebar_content(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }

    backend.set_theme(super::quadraui_tui::q_theme(theme));

    let bg_bar = quadraui::StatusBar {
        id: quadraui::WidgetId::new("explorer:bg"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: String::new(),
            fg: theme.explorer_file_fg,
            bg: theme.tab_bar_bg,
            bold: false,
            action_id: None,
        }],
        right_segments: vec![],
    };
    for y in area.y..area.y + area.height {
        let row_rect = quadraui::Rect::new(area.x as f32, y as f32, area.width as f32, 1.0);
        let _ = backend.draw_status_bar(row_rect, &bg_bar, None, None);
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
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    engine.explorer_tree.borrow().render(backend, q_rect);

    // TreeController.render() draws the scrollbar internally.
    // Register a ScrollSurface for scroll-wheel dispatch only.
    engine
        .scroll_surfaces
        .borrow_mut()
        .push(quadraui::ScrollSurface {
            id: quadraui::WidgetId::new("explorer:sb"),
            bounds: q_rect,
            scrollbar: None,
        });
}

/// Sidebar panel content for [`super::shell_app::TuiShellApp::render_content`]
/// (#607). Before #766 this was a parallel, narrower dispatcher next to a
/// `render_sidebar` used only by `draw_frame`'s test harness, over the
/// subset of panels whose renderers need nothing but `Backend::draw_*` trait
/// calls — no raw `Frame`/`Buffer` access. `draw_frame` and its `render_sidebar`
/// dispatcher are gone now (#766), so this is the one dispatcher, mirroring
/// how `render_content` itself has its own entry points for editor content
/// (`build_screen_for_shell_content` + `paint_editor_popups` in
/// `render_impl.rs`, #601) and key dispatch
/// (`render::dispatch_panel_accelerator`, `handle_key_pressed`, above in
/// `shell_app.rs`).
///
/// Ported: explorer (default panel, via [`render_explorer_sidebar_content`]),
/// search (`render_search_panel`, already trait-pure — no raw buffer use at
/// all), debug (`render_debug_sidebar`, likewise already trait-pure), and —
/// #605 — **settings**, **source control** and **extensions**, whose raw
/// `set_cell` chrome (background wipe, header rows, focused-hint row, search
/// boxes) was converted to [`fill_rect`] / [`fill_row`] /
/// `Backend::draw_settings_chrome`. All three renderers dropped their
/// `&mut Frame` parameter entirely as a result, so `draw_frame` and
/// `render_content` now share one implementation of each rather than the
/// live path keeping a frame-having variant.
///
/// #635 (Stage 6b item C) closed the last two: the **plugin extension
/// panel** (`render_ext_panel`'s help-popup overlay now paints through
/// `Backend::draw_tooltip`, its manual scrollbar through [`fill_row`]) and
/// the **AI panel** (`render_ai_sidebar` dropped its `buf: &mut Buffer`
/// parameter for `&mut dyn Backend`, using `Backend::draw_message_list` for
/// the chat history and [`fill_row`] for its plain chrome rows). See each
/// function's own doc comment for the specific tradeoffs.
///
pub(super) fn render_sidebar_content(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    sidebar: &TuiSidebar,
    engine: &Engine,
    theme: &Theme,
) {
    if sidebar.ext_panel_name.is_some() {
        // #635 (Stage 6b item C): no longer deferred — `render_ext_panel`
        // dropped its `&mut Frame` parameter (help popup + scrollbar now
        // paint through `Backend::draw_tooltip`/`fill_row`; see that
        // function's doc comment).
        render_ext_panel(backend, area, engine, theme);
        return;
    }

    match engine.app_shell.active_panel_id().map(|w| w.as_str()) {
        Some(PANEL_SEARCH) => render_search_panel(backend, area, engine, theme),
        Some(PANEL_DEBUG) => render_debug_sidebar(backend, area, engine, theme),
        // #605: settings, source control and extensions are no longer
        // deferred — each had its raw `set_cell` chrome converted to the
        // rule-row trick.
        Some(PANEL_SETTINGS) => render_settings_panel(backend, area, theme, engine),
        Some(PANEL_GIT) => render_source_control(backend, area, engine, theme),
        Some(PANEL_EXTENSIONS) => render_ext_sidebar(backend, area, engine, theme),
        // #635 (Stage 6b item C): AI is no longer deferred — `render_ai_sidebar`
        // dropped its `buf: &mut Buffer` parameter for `&mut dyn Backend`.
        Some(PANEL_AI) => render_ai_sidebar(backend, area, engine, theme),
        _ => render_explorer_sidebar_content(backend, area, engine, theme),
    }
}

// ─── Trait-only stand-ins for raw-`Buffer` chrome (#605) ─────────────────────
//
// Perf note: every call below goes through `render_impl::draw_rule_row_q`,
// which constructs one `StatusBar` + segment `Vec` per row (see that fn's
// doc comment) rather than writing cells directly — a real per-row
// allocation increase over the old two-pass `set_cell` loops. Unlikely to
// matter for a handful of sidebar rows at TUI frame rates; worth a look if a
// future profiling pass finds TUI paint time regressed.

/// Fill `width` cells at `(x, y)` with `text`, space-padded (or truncated) to
/// exactly `width` characters, in one [`render_impl::draw_rule_row_q`] call.
///
/// This is the trait-only equivalent of the "blank the row with `set_cell`,
/// then write the text over it with `set_cell`" two-pass pattern the sidebar
/// panels used before #605. Padding produces the identical result — cells past
/// the end of `text` stay blank in the same `fg`/`bg` — but reaches the screen
/// through `&mut dyn Backend`, which `Buffer` writes cannot.
fn fill_row_q(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    fg: quadraui::Color,
    bg: quadraui::Color,
) {
    if width == 0 {
        return;
    }
    let mut row: String = text.chars().take(width as usize).collect();
    let painted = row.chars().count();
    for _ in painted..width as usize {
        row.push(' ');
    }
    super::render_impl::draw_rule_row_q(backend, x, y, &row, fg, bg);
}

/// [`fill_row_q`] over vimcode's own `Color`.
fn fill_row(
    backend: &mut dyn quadraui::Backend,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    fg: Color,
    bg: Color,
) {
    fill_row_q(backend, x, y, width, text, fg, bg);
}

/// Clear `area` to `bg` — the trait-only equivalent of the nested
/// `for y { for x { set_cell(..) } }` background wipe the panels open with.
fn fill_rect(backend: &mut dyn quadraui::Backend, area: Rect, fg: Color, bg: Color) {
    for y in area.y..area.y + area.height {
        fill_row(backend, area.x, y, area.width, "", fg, bg);
    }
}

/// Render the settings panel — shows current key settings and the file path.
///
/// B5c.4: routes the form rendering through `Backend::draw_form` so
/// the form rasteriser and call site share the same code path GTK
/// uses.
///
/// #605: `backend` widened from `&mut TuiBackend` + `&mut Frame` to
/// `&mut dyn Backend`. The background wipe went through [`fill_rect`]; the
/// header/search-box chrome was a local stand-in
/// (`draw_settings_chrome_via_backend`) for the missing
/// `Backend::draw_settings_chrome` trait method
/// ([JDonaghy/quadraui#531](https://github.com/JDonaghy/quadraui/issues/531)).
/// #635 (Stage 6b) retires that stand-in now that #531 has landed: the
/// chrome paints through the real trait call below.
pub(super) fn render_settings_panel(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    theme: &Theme,
    engine: &Engine,
) {
    if area.height == 0 {
        return;
    }

    // Fill background
    fill_rect(backend, area, theme.foreground, theme.tab_bar_bg);

    // Rows 0–1: header + search input chrome.
    let chrome_h = area.height.min(2);
    let chrome_area = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        chrome_h as f32,
    );
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    backend.draw_settings_chrome(
        chrome_area,
        " SETTINGS",
        &engine.settings_query,
        "",
        engine.settings_input_active,
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
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    engine
        .settings_form_controller
        .borrow_mut()
        .render_and_cache(backend, q_rect);
}

/// Render the project search panel via SidebarSystem (Form + TreeView).
///
/// `backend` is `&mut dyn quadraui::Backend` (not the concrete `TuiBackend`)
/// — this renderer was already trait-pure (no raw `Frame`/`Buffer` access),
/// so #607 widened the parameter the same way #601 did for
/// `render_tab_bar`/`draw_breadcrumb_bar`, letting
/// `TuiShellApp::render_content` call it via [`render_sidebar_content`]
/// without a concrete backend.
pub(super) fn render_search_panel(
    backend: &mut dyn quadraui::Backend,
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

    backend.set_theme(super::quadraui_tui::q_theme(theme));
    engine
        .search_sidebar_system
        .borrow()
        .render(backend, q_rect);
}

// ─── Status / command line ────────────────────────────────────────────────────

/// Paint the `:`-command line row (background fill, text, inverted block
/// cursor, and the mouse drag-selection inversion).
///
/// #605 (Stage 6 parity sweep): this used to write straight into
/// `frame.buffer_mut()` via `set_cell`, which made it unreachable from
/// `TuiShellApp::render_content`'s `&mut dyn Backend`-only signature. It now
/// composes the row into a `(char, fg, bg)` cell vector and paints it through
/// [`render_impl::draw_rule_row_themed`] — the same
/// `Backend::draw_status_bar`-stands-in-for-a-raw-`set_cell` trick #609
/// introduced for the window dividers (see that helper's doc comment).
///
/// The two inversions (cursor, then `selection`) are applied to the composed
/// cells *before* painting rather than as buffer read-back passes afterwards.
/// That's behaviour-identical to the old two-pass version — including the
/// double-invert-cancels case where the cursor cell also falls inside the
/// selection — but needs no `Buffer` access. `selection` is `event_loop`'s
/// `cmd_sel` local (`(start, end)` character indices, either order).
pub(super) fn render_command_line(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    command: &render::CommandLineData,
    theme: &Theme,
    selection: Option<(usize, usize)>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let fg = theme.command_fg;
    let bg = theme.command_bg;
    let width = area.width as usize;

    // Row composition: background fill first, then the text on top.
    let mut cells: Vec<(char, Color, Color)> = vec![(' ', fg, bg); width];
    let chars: Vec<char> = command.text.chars().collect();
    if command.right_align {
        // Right-aligned text that doesn't fit is dropped entirely — matches
        // the old `if len <= area.width` guard.
        if chars.len() <= width {
            let start = width - chars.len();
            for (i, &ch) in chars.iter().enumerate() {
                cells[start + i].0 = ch;
            }
        }
    } else {
        for (i, &ch) in chars.iter().enumerate() {
            if i >= width {
                break;
            }
            cells[i].0 = ch;
        }
    }

    // Command-line cursor (inverted block at insertion point).
    if command.show_cursor {
        let cursor_col = command.cursor_anchor_text.chars().count();
        let idx = cursor_col.min(width - 1);
        let cell = &mut cells[idx];
        std::mem::swap(&mut cell.1, &mut cell.2);
    }

    // Mouse drag-selection: invert fg/bg for the selected span.
    if let Some((start, end)) = selection {
        let lo = start.min(end);
        let hi = start.max(end);
        for cell in cells.iter_mut().take(hi + 1).skip(lo) {
            std::mem::swap(&mut cell.1, &mut cell.2);
        }
    }

    // Paint, batching runs of identically-coloured cells into one
    // `draw_status_bar` call so a plain uncoloured command line costs one
    // draw rather than `width` of them.
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let mut run_start = 0usize;
    while run_start < width {
        let (_, run_fg, run_bg) = cells[run_start];
        let mut run_end = run_start + 1;
        while run_end < width && cells[run_end].1 == run_fg && cells[run_end].2 == run_bg {
            run_end += 1;
        }
        let text: String = cells[run_start..run_end].iter().map(|c| c.0).collect();
        super::render_impl::draw_rule_row_themed(
            backend,
            area.x + run_start as u16,
            area.y,
            &text,
            run_fg,
            run_bg,
        );
        run_start = run_end;
    }
}

// ─── Input translation ────────────────────────────────────────────────────────

/// #605: widened from `&mut TuiBackend` + `&mut Frame` to `&mut dyn Backend`.
/// The three raw-`Buffer` pieces — the full-area background wipe, the
/// focused-hint row, and the "SOURCE CONTROL" header row — all became
/// [`fill_rect`]/[`fill_row`] calls, so `TuiShellApp::render_content` can
/// paint this panel. Everything else here was already a `Backend::draw_*`
/// trait call.
pub(super) fn render_source_control(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let hdr_fg = theme.status_fg;
    let hdr_bg = theme.status_bg;
    // Clear the entire area first to prevent stale content from previous renders.
    fill_rect(backend, area, theme.foreground, theme.tab_bar_bg);
    let dim_fg = theme.line_number_fg;

    // Build SC data from engine state via the render abstraction.
    let screen = render::build_screen_layout(
        engine,
        theme,
        &[],
        1.0,
        1.0,
        true,
        0.0,
        render::TUI_MINIMAP_SIZING,
    );
    let Some(ref sc) = screen.source_control else {
        return;
    };

    // Reserve bottom row for hint bar when focused.
    let area = if sc.has_focus && area.height > 2 {
        let hint_y = area.y + area.height - 1;
        fill_row(
            backend,
            area.x,
            hint_y,
            area.width,
            " Press '?' for help",
            dim_fg,
            hdr_bg,
        );
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
    fill_row(
        backend,
        area.x,
        area.y,
        area.width,
        &branch_info,
        hdr_fg,
        hdr_bg,
    );

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
        backend.set_theme(super::quadraui_tui::q_theme(theme));
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
        backend.set_theme(super::quadraui_tui::q_theme(theme));
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
    backend.set_theme(super::quadraui_tui::q_theme(theme));
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
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_palette(q_rect, &palette);
    }

    // ── Help dialog (quadraui::Dialog + DialogTable, #480) ───────────────────
    // Migrated from a hand-rolled 2-column popup to `Dialog`'s table slot,
    // shipped in quadraui#225. Bindings list lives once in
    // `render::sc_help_dialog` instead of being duplicated per backend.
    if sc.help_open {
        let viewport = quadraui::Rect::new(
            area.x as f32,
            area.y as f32,
            area.width as f32,
            area.height as f32,
        );
        let (dialog, layout) = render::sc_help_dialog_layout(viewport, 1.0, 1.0);
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        let _ = backend.draw_dialog(&dialog, &layout);
    }
}

// ─── Extension panel (plugin-provided) ───────────────────────────────────────

/// Render an extension-provided sidebar panel.
///
/// Migrated to `quadraui::TreeView` (#476). Header + search-input chrome
/// route through `Backend::draw_settings_chrome`; the body rows (sections +
/// expandable tree items + badges + action labels) flow through
/// `render::ext_panel_to_tree_view()` + `Backend::draw_tree`. The
/// help-popup overlay and the scrollbar are panel-specific chrome that
/// don't fit `TreeView` and stay inline — as of #635 (Stage 6b item C)
/// through `Backend::draw_tooltip`/[`fill_row`] rather than raw `set_cell`,
/// so `backend` widens to `&mut dyn Backend` and `frame` drops out of the
/// signature entirely (this was the panel's own doc-flagged "no primitive
/// stand-in checked yet" gap — see `shell_app.rs`'s module doc).
pub(super) fn render_ext_panel(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    let screen = render::build_screen_layout(
        engine,
        theme,
        &[],
        1.0,
        1.0,
        true,
        0.0,
        render::TUI_MINIMAP_SIZING,
    );
    let Some(ref panel) = screen.ext_panel else {
        return;
    };

    // ── Chrome: header (always) + search input (only when active or text). ─
    let input_visible = panel.input_active || !panel.input_text.is_empty();
    let chrome_h: u16 = (if input_visible { 2 } else { 1 }).min(area.height);
    let header_title = format!(" {}", panel.title);
    let chrome_area = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        chrome_h as f32,
    );
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    backend.draw_settings_chrome(
        chrome_area,
        &header_title,
        &panel.input_text,
        "",
        panel.input_active,
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
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_tree(body_q_rect, &tree);

        // Scrollbar: `draw_tree` doesn't render scrollbars yet. Total
        // visible rows = tree.rows.len() (sections + their expanded items,
        // separators included — same flat count the legacy renderer
        // produced). #635: the manual `set_cell` thumb/track loop is now
        // one [`fill_row`] call per row (the rule-row trick #605 used for
        // the settings/source-control/extensions sidebar chrome) instead
        // of a raw `Buffer` write.
        let total = tree.rows.len();
        let track_h = body_h as usize;
        let ext_panel_scrollbar = if total > track_h && track_h > 0 {
            let scroll = panel.scroll_top;
            let sb_x = area.x + area.width - 1;
            let thumb_h = (track_h * track_h / total).max(1);
            let thumb_top = scroll * track_h / total;
            for i in 0..track_h {
                let y = area.y + chrome_h + i as u16;
                let (ch, fg) = if i >= thumb_top && i < thumb_top + thumb_h {
                    ('\u{2588}', theme.scrollbar_thumb)
                } else {
                    ('\u{2591}', theme.scrollbar_track)
                };
                fill_row(backend, sb_x, y, 1, &ch.to_string(), fg, theme.background);
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

    // ── Help popup overlay ──────────────────────────────────────────────────
    // #635 (Stage 6b item C): was raw `set_cell` box-drawing (full border +
    // centered title in the border + close 'x' glyph). `Backend::draw_tooltip`
    // exists in quadraui's `Backend` trait, but its TUI rasteriser only
    // draws side-bar borders (`│` on the first/last column, no top/bottom
    // border or border-embedded title — see `quadraui::tui::draw_tooltip`'s
    // doc comment), so the title moves into the content as its own styled
    // row instead of being centered in a top border. `TooltipLayout` is
    // built by hand rather than via `Tooltip::layout` (an anchor-relative
    // placement API that doesn't fit this popup's "centered over `area`"
    // positioning) — its fields are public for exactly this kind of direct
    // construction. The close glyph had no click handler anywhere
    // (`ext_panel_help_open` only ever closes via a key press — see
    // `core/engine/ext_panel.rs`), so dropping it changes no behavior.
    if panel.help_open && !panel.help_bindings.is_empty() {
        let bindings = &panel.help_bindings;
        let popup_w = area.width.saturating_sub(2).min(36);
        let popup_h = (bindings.len() as u16 + 3).min(area.height.saturating_sub(2));
        let popup_x = area.x + (area.width.saturating_sub(popup_w)) / 2;
        let popup_y = area.y + (area.height.saturating_sub(popup_h)) / 2;

        let q_popup_fg = theme.completion_fg;
        let q_key_fg = theme.function;
        let mut lines: Vec<quadraui::StyledText> = vec![quadraui::StyledText::plain("Keybindings")];
        for (key, desc) in bindings.iter() {
            lines.push(quadraui::StyledText {
                spans: vec![
                    quadraui::StyledSpan::with_fg(format!("{key:<9} "), q_key_fg),
                    quadraui::StyledSpan::with_fg(desc.clone(), q_popup_fg),
                ],
            });
        }

        let mut tooltip =
            render::quadraui_tooltip(quadraui::WidgetId::new("ext_panel:help"), String::new());
        tooltip.styled_lines = Some(lines);
        tooltip.bg = Some(theme.completion_bg);
        tooltip.fg = Some(q_popup_fg);
        let layout = quadraui::TooltipLayout {
            bounds: quadraui::Rect::new(
                popup_x as f32,
                popup_y as f32,
                popup_w as f32,
                popup_h as f32,
            ),
            resolved_placement: quadraui::ResolvedPlacement::Bottom,
        };
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        backend.draw_tooltip(&tooltip, &layout);
    }
}

// ─── Panel hover popup ─────────────────────────────────────────────────────────

/// Render a panel-item hover popup to the right of the sidebar.
///
/// The popup displays rendered markdown content and appears to the right of
/// the sidebar at the vertical position of the hovered item.
/// Returns `(link_rects, popup_rect)`, both `quadraui::Rect`-based (#831) —
/// the same type GTK's `render::panel_hover_popup_paint` returns, so the
/// mouse router (`render::route_sidebar_hover` and friends) never has to
/// know which backend painted the popup.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn render_panel_hover_popup(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar_right_x: u16,
    sidebar_y: u16,
    sidebar_height: u16,
    term_area: Rect,
) -> (Vec<(quadraui::Rect, String)>, Option<quadraui::Rect>) {
    let Some(ref ph) = screen.panel_hover else {
        return (vec![], None);
    };

    let lines = &ph.line_text;
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

    backend.set_theme(super::quadraui_tui::q_theme(theme));
    backend.draw_rich_text_popup(&popup, &layout);

    let link_rects: Vec<(quadraui::Rect, String)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (*rect, url)
        })
        .collect();

    // `layout.bounds` is cached (and later hit-tested) as the raw `f32`
    // `quadraui::Rect` the primitive returned — no `.round()`-to-cell step
    // the way the pre-#831 hand-rolled `u16` cache had. That's only safe
    // because every input feeding `RichTextPopup::layout` here is already
    // integral in cell units: `padding: 0.0` (`panel_hover_to_quadraui_rich_text`,
    // `render.rs`) and the primitive's own `border` (fixed at `1.0`,
    // `rich_text_popup.rs`) are the only two offsets `layout()` adds beyond
    // the whole-cell `popup_x`/`top_row` this function passes in. If either
    // ever became fractional (e.g. a future padding tweak), the painted box
    // and the cached hit-test rect would still agree with each other — both
    // come from this one `layout` call — but would silently stop landing on
    // whole terminal cells.
    (link_rects, Some(layout.bounds))
}

// ─── Editor hover popup ─────────────────────────────────────────────────────

/// Render an editor hover popup via the shared `render::editor_hover_popup_paint`
/// (#831) — this used to be its own ~90-line copy of that function's
/// `RichTextPopup::layout` + hit-region-extraction, differing from GTK's only
/// in rounding the result to `u16` cells instead of keeping `quadraui::Rect`'s
/// native `f32`. Now that the caches on both backends are `quadraui::Rect`
/// (there is no unit parameter in quadraui's layout API — TUI's cell
/// coordinates and GTK's pixel coordinates are both plain `f32`), TUI calls
/// the exact same paint step GTK does, with `unit_w`/`unit_h` = `1.0, 1.0`.
///
/// `backend` is `&mut dyn quadraui::Backend` (not the concrete `TuiBackend`)
/// so this is callable from `TuiShellApp::render_content` (#601) — see
/// `render_impl.rs::render_tab_bar`'s doc comment for the general rationale.
#[allow(clippy::type_complexity)]
pub(super) fn render_editor_hover_popup(
    backend: &mut dyn quadraui::Backend,
    eh: &render::EditorHoverPopupData,
    popup_x: u16,
    popup_y: u16,
    term_area: Rect,
    theme: &Theme,
) -> (
    Vec<(quadraui::Rect, String)>,
    Option<quadraui::Rect>,
    Option<render::PopupScrollbarHit>,
) {
    let viewport = quadraui::Rect::new(
        term_area.x as f32,
        term_area.y as f32,
        term_area.width as f32,
        term_area.height as f32,
    );
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    render::editor_hover_popup_paint(
        backend,
        eh,
        popup_x as f32,
        popup_y as f32,
        viewport,
        theme,
        1.0,
        1.0,
    )
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
/// #605: widened from `&mut TuiBackend` + `&mut Frame` to `&mut dyn Backend`.
/// The two chrome rows were the only raw-`Buffer` writes left; the local
/// `write_row` closure they used is exactly what [`fill_row`] does, so it
/// collapsed into that.
pub(super) fn render_ext_sidebar(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let screen = render::build_screen_layout(
        engine,
        theme,
        &[],
        1.0,
        1.0,
        true,
        0.0,
        render::TUI_MINIMAP_SIZING,
    );
    let Some(ref ext) = screen.ext_sidebar else {
        return;
    };

    let header_fg = theme.status_fg;
    let header_bg = theme.status_bg;
    let default_fg = theme.foreground;
    let dim_fg = theme.line_number_fg;
    let sel_bg = theme.fuzzy_selected_bg;
    let panel_bg = theme.completion_bg;

    // ── Chrome rows: panel header (row 0) + search box (row 1) ───────────────
    if area.height >= 1 {
        let hdr = if ext.fetching {
            " \u{eb85} EXTENSIONS  (fetching…)".to_string()
        } else {
            " \u{eb85} EXTENSIONS".to_string()
        };
        fill_row(
            backend, area.x, area.y, area.width, &hdr, header_fg, header_bg,
        );
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
        fill_row(
            backend,
            area.x,
            area.y + 1,
            area.width,
            &search_text,
            search_fg,
            search_bg,
        );
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
    backend.set_theme(q_theme);
    engine.ext_sidebar_system.borrow().render(backend, msv_rect);
}

// ─── AI assistant sidebar panel ───────────────────────────────────────────────

/// Render the AI assistant sidebar panel.
///
/// #819: delegates its entire paint to the shared `engine.ai_chat`
/// (`quadraui::ChatController`), the same controller GTK's `render_content`
/// `PANEL_AI` arm now also renders — one implementation instead of two,
/// mirroring `render_explorer_sidebar_content`'s `explorer_tree` pattern.
/// `area` is already in character cells (TUI's native coordinate space);
/// `ChatController` reads `Backend::line_height`/`char_width` itself
/// (`1.0`/`1.0` on TUI) rather than taking them as parameters.
pub(super) fn render_ai_sidebar(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let q_area = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    render::populate_ai_chat_controller(engine, theme);
    engine.ai_chat_rect.set(q_area);
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    engine.ai_chat.borrow().render(backend, q_area);
}

// ─── Debug sidebar panel ──────────────────────────────────────────────────────

/// Render the debug sidebar: header + run button + 4 sections (Variables, Watch, Call Stack, Breakpoints).
/// Migrated to four `quadraui::TreeView` instances (#281), one per
/// section. Panel header (row 0) + Run/Stop button (row 1) + per-section
/// title rows + per-section scrollbar overlays remain panel-specific
/// chrome; item rendering goes through `Backend::draw_tree`.
/// #607: `backend` widened to `&mut dyn quadraui::Backend` — this renderer
/// was already trait-pure, same rationale as `render_search_panel` above.
pub(super) fn render_debug_sidebar(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }

    // Build minimal screen layout to get debug_sidebar data.
    let screen = render::build_screen_layout(
        engine,
        theme,
        &[],
        1.0,
        1.0,
        true,
        0.0,
        render::TUI_MINIMAP_SIZING,
    );
    let sidebar = &screen.debug_sidebar;

    // ── Chrome rows (panel-specific): header + Run/Stop button via StatusBar. ──
    let (title_bar, action_bar) = render::debug_sidebar_chrome_to_status_bars(sidebar, theme);
    let q_theme = super::quadraui_tui::q_theme(theme);

    let title_rect = quadraui::Rect::new(area.x as f32, area.y as f32, area.width as f32, 1.0);
    backend.set_theme(q_theme);
    let _ = backend.draw_status_bar(title_rect, &title_bar, None, None);

    if area.height < 2 {
        return;
    }

    let action_rect =
        quadraui::Rect::new(area.x as f32, (area.y + 1) as f32, area.width as f32, 1.0);
    backend.set_theme(q_theme);
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
    backend.set_theme(q_theme);
    engine.dap_sidebar_system.borrow().render(backend, msv_rect);
}

// The bottom-band rungs that used to live here — `render_bottom_panel_tabs`,
// `render_quickfix_panel`, `render_terminal_toolbar`, `render_terminal_panel`
// and `render_terminal_panel_content` — moved to `render.rs` as
// `render::paint_quickfix_rung` and `render::paint_bottom_panel_rung` (#765,
// #735 slice 4). Each was one `render::build_*` adapter call plus one
// `Backend::draw_*`, transcribed once here and again in GTK's `render_content`;
// the shared painters take the caller's unit system as a `render
// ::BottomPanelUnits` instead, so pixels and cells run the same code.

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
    /// `Engine::new_for_test()` builds settings/session/history/git_branch
    /// from in-memory defaults instead of loading ambient disk/git state
    /// (#615, #439, #617), so snapshots don't depend on the repo state of
    /// whatever machine/branch the test happens to run on — see its doc
    /// comment for why call-then-overwrite on `Engine::new()` doesn't
    /// reliably undo `app_shell.hide_sidebar()`. `extension_state` and
    /// `ext_registry` are still loaded from disk/cache unconditionally by
    /// `new_from_state()`, so they're reset explicitly here, matching
    /// `render_impl.rs`'s `test_engine()`.
    fn test_engine() -> Engine {
        crate::core::session::suppress_disk_saves();
        let mut e = Engine::new_for_test();
        e.extension_state = crate::core::session::ExtensionState::default();
        e.ext_registry = None;
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
                // #605: the renderer no longer needs the `Frame` at all, but
                // the scope entry is still what gives its `draw_*` calls a
                // buffer to land in.
                super::with_frame_scope(&mut tui_backend, frame, |backend, _frame| {
                    render_source_control(backend, area, engine, &theme);
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
        // #677 audit: the test's own name promises "marks_current", but until
        // now nothing checked that `main` (`is_current: true`) is painted any
        // differently from `feature/foo` -- both branch-name asserts above
        // pass even if the current-branch marker is deleted. render.rs's
        // `sc_branch_picker_to_palette` prefixes the current branch with
        // U+25CF ("\u{25cf} name") and non-current branches with two spaces
        // ("  name"), so assert on that distinction directly. Verified
        // vacuous by mutation: forcing `is_current` to `false` unconditionally
        // in `sc_branch_picker_to_palette`'s formatting closure left the two
        // asserts above green and only these two red.
        assert!(
            contains(&lines, "\u{25cf} main"),
            "the current branch must be marked with the current-branch glyph; {lines:#?}"
        );
        assert!(
            !contains(&lines, "\u{25cf} feature/foo"),
            "a non-current branch must not carry the current-branch glyph; {lines:#?}"
        );
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

// ─── Activity-bar keyboard ring (#536) ───────────────────────────────────────
//
// Black-box coverage for the migration of the activity-bar keyboard cursor
// onto quadraui's `AppShell` (quadraui#386). Every assertion reads the
// **rasterised** activity-bar strip — the row whose background is the
// selection colour — rather than `Engine::activity_bar_selected`, so a
// selection index that moves correctly but paints on the wrong icon (the
// #587/#592 failure mode: state populated, nothing painted) still fails here.
//
// The ring's ordering is the thing under test: hamburger, the six fixed
// panels, the dynamic extension panels spliced in *before* Settings, and
// Settings pinned last — while the legacy `activity_bar_selected` index space
// numbers Settings at 7 and extension panels at 8+. Before #536 that mismatch
// was reconciled by a hand-rolled `if sel < 6 { … } else if sel == 6 && …`
// chain in `core::engine::sidebar`; it is now `AppShell`'s cursor.
#[cfg(test)]
mod activity_bar_keyboard_ring_tests {
    use super::*;
    use crate::core::plugin::PanelRegistration;
    use ratatui::buffer::Buffer;

    const BAR_W: u16 = 3;
    const BAR_H: u16 = 12;

    fn ring_engine() -> Engine {
        crate::core::session::suppress_disk_saves();
        let mut e = Engine::new_for_test();
        e.extension_state = crate::core::session::ExtensionState::default();
        e.ext_registry = None;
        e.ext_panels.clear();
        e
    }

    fn add_ext(e: &mut Engine, name: &str, icon: char) {
        e.ext_panels.insert(
            name.to_string(),
            PanelRegistration {
                name: name.to_string(),
                title: name.to_string(),
                icon,
                fallback_icon: Some(icon),
                sections: vec![],
            },
        );
    }

    /// Paint the activity bar and return `(row, icon_char)` for the single row
    /// carrying the keyboard-selection background, or `None` when no row does.
    ///
    /// `draw_activity_bar` fills the selected row with `bar.selection_bg`
    /// (`theme.cursor`) and every other row with `theme.tab_bar_bg`, so the
    /// probe is "which row's background is the cursor colour" — the same thing
    /// a user sees. The icon glyph comes back with it so the assertions can
    /// name the item rather than a bare row number (#555: probe, don't
    /// hardcode).
    fn painted_ring(engine: &Engine) -> Option<(u16, char)> {
        let theme = crate::render::Theme::onedark();
        let sel = ratatui::style::Color::Rgb(theme.cursor.r, theme.cursor.g, theme.cursor.b);
        let area = Rect {
            x: 0,
            y: 0,
            width: BAR_W,
            height: BAR_H,
        };
        let mut buf = Buffer::empty(area);
        let sidebar = TuiSidebar::new();
        render_activity_bar(&mut buf, area, &sidebar, &theme, false, engine);

        let mut hit = None;
        for y in 0..BAR_H {
            if buf[(0, y)].bg == sel {
                assert!(
                    hit.is_none(),
                    "more than one row painted the selection ring"
                );
                hit = Some((y, buf[(1, y)].symbol().chars().next().unwrap_or(' ')));
            }
        }
        hit
    }

    /// The ring only paints while the bar holds keyboard focus, and `j` walks
    /// the fixed panels top-down from the hamburger.
    #[test]
    fn ring_paints_only_when_focused_and_j_walks_the_fixed_panels() {
        let mut e = ring_engine();
        assert_eq!(
            painted_ring(&e),
            None,
            "no ring should paint while the activity bar is unfocused"
        );

        e.activity_bar_focus_in_at(0);
        let (hamburger_row, _) = painted_ring(&e).expect("focusing the bar must paint a ring");
        assert_eq!(hamburger_row, 0, "index 0 is the hamburger, the top row");

        for expected_row in 1..=6 {
            e.activity_bar_move_down();
            let (row, _) = painted_ring(&e).expect("ring must stay painted while stepping");
            assert_eq!(
                row,
                expected_row,
                "j from row {} must land on row {expected_row}",
                expected_row - 1
            );
        }
    }

    /// With no extension panels, `j` past the last fixed panel (AI) lands on
    /// Settings — which paints *pinned to the bottom edge*, not on row 7 — and
    /// saturates there. `k` comes straight back to AI.
    #[test]
    fn ring_steps_from_ai_to_bottom_pinned_settings_and_saturates() {
        let mut e = ring_engine();
        e.activity_bar_focus_in_at(6); // AI, the last fixed panel
        assert_eq!(painted_ring(&e).map(|(r, _)| r), Some(6));

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(BAR_H - 1),
            "Settings is bottom-pinned, so the ring must jump to the last row"
        );
        assert_eq!(e.activity_bar_selected, 7, "Settings is toolbar index 7");

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(BAR_H - 1),
            "j on the bottom-most item must saturate, not wrap to the top"
        );

        e.activity_bar_move_up();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(6),
            "k from Settings with no extension panels returns to AI"
        );
    }

    /// `k` on the top-most item saturates rather than wrapping to Settings.
    #[test]
    fn ring_saturates_at_the_hamburger() {
        let mut e = ring_engine();
        e.activity_bar_focus_in_at(0);
        e.activity_bar_move_up();
        assert_eq!(painted_ring(&e).map(|(r, _)| r), Some(0));
        assert_eq!(e.activity_bar_selected, 0);
    }

    /// The headline ordering claim: extension panels splice in **between** AI
    /// and Settings in painted order (sorted by name), even though the legacy
    /// index space numbers them *after* Settings. Walking `j` from AI must
    /// visit both extension icons and only then reach Settings.
    #[test]
    fn ring_splices_extension_panels_between_ai_and_settings() {
        let mut e = ring_engine();
        add_ext(&mut e, "zz-last", 'Z');
        add_ext(&mut e, "aa-first", 'A');
        e.activity_bar_focus_in_at(6); // AI

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e),
            Some((7, 'A')),
            "j from AI must land on the first extension panel (sorted by name)"
        );
        assert_eq!(e.activity_bar_selected, 8, "…which is toolbar index 8");

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e),
            Some((8, 'Z')),
            "j must then land on the second extension panel"
        );
        assert_eq!(e.activity_bar_selected, 9);

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(BAR_H - 1),
            "only after the last extension panel does j reach bottom-pinned Settings"
        );
        assert_eq!(e.activity_bar_selected, 7);

        // …and `k` from Settings walks back onto the *last* extension panel.
        e.activity_bar_move_up();
        assert_eq!(painted_ring(&e), Some((8, 'Z')));
        assert_eq!(e.activity_bar_selected, 9);

        e.activity_bar_move_up();
        assert_eq!(painted_ring(&e), Some((7, 'A')));

        e.activity_bar_move_up();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(6),
            "k off the first extension panel returns to AI, not to Settings"
        );
        assert_eq!(e.activity_bar_selected, 6);
    }

    /// A selection left pointing at an extension panel that has since been
    /// unregistered (`:PluginReload`) must not wedge the cursor: the next
    /// `k` has to move somewhere real. Pre-#536 the bespoke `sel > 8` arm
    /// stepped to 8; the `AppShell` cursor clamps to the last item first and
    /// then steps, landing in the same place.
    #[test]
    fn ring_recovers_from_a_stale_extension_index() {
        let mut e = ring_engine();
        add_ext(&mut e, "only-one", 'O');
        e.activity_bar_focus_in_at(9); // second ext panel — no longer exists
        assert_eq!(
            painted_ring(&e),
            None,
            "a selection naming no item paints no ring"
        );

        e.activity_bar_move_up();
        assert_eq!(
            e.activity_bar_selected, 8,
            "k must recover onto the one extension panel that does exist"
        );
        assert_eq!(painted_ring(&e).map(|(r, _)| r), Some(7));
    }
}
