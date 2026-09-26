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
/// #766/#607: reached only via `render_sidebar_content` below.
///
/// #1389: background fill + no-chrome carve + the `TreeController` body
/// itself all go through `SidebarPanelBody::render_with` (quadraui#1059) in
/// a single call — `render_with` takes the body as a plain closure with no
/// `Send + 'static` bound, so the `!Send`, `Rc<RefCell<_>>`-backed
/// `TreeController` on `Engine` can be the body directly instead of the
/// hand-copied `render::paint_sidebar_panel_chrome` split #1242 needed
/// before #1059 existed. GTK's `PANEL_EXPLORER` arm uses the same call with
/// `background: None` (unchanged behaviour there).
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

    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    let panel = render::SidebarPanelBody {
        background: Some(theme.tab_bar_bg),
        chrome: render::SidebarPanelChrome::None,
        scrollbar_gutter: None,
    };
    render::populate_explorer_tree_controller(engine, theme);
    let layout = panel.render_with(backend, q_rect, |backend, body_rect| {
        engine.explorer_tree_rect.set(body_rect);
        engine.explorer_viewport_rows.set(body_rect.height as usize);
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        engine.explorer_tree.borrow().render(backend, body_rect);
    });

    // TreeController.render() draws the scrollbar internally.
    // Register a ScrollSurface for scroll-wheel dispatch only.
    engine
        .scroll_surfaces
        .borrow_mut()
        .push(quadraui::ScrollSurface {
            id: quadraui::WidgetId::new("explorer:sb"),
            bounds: layout.body_rect,
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
/// #1252: takes the frame's own `screen: &render::ScreenLayout` — built once
/// per frame by `build_screen_for_shell_content` — instead of each panel
/// rebuilding its own via `render::build_screen_layout(engine, theme, &[],
/// ...)`. GTK's `paint_sidebar_panel_rung` (`src/app.rs`) already threads the
/// frame's `screen` this way; a second, independently-built `ScreenLayout`
/// mid-frame could disagree with the one the rest of the frame was composed
/// from, on top of the redundant per-frame rebuild cost.
pub(super) fn render_sidebar_content(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
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
        render_ext_panel(backend, screen, area, engine, theme);
        return;
    }

    match engine.app_shell.active_panel_id().map(|w| w.as_str()) {
        Some(PANEL_SEARCH) => render_search_panel(backend, area, engine, theme),
        Some(PANEL_DEBUG) => render_debug_sidebar(backend, screen, area, engine, theme),
        // #605: settings, source control and extensions are no longer
        // deferred — each had its raw `set_cell` chrome converted to the
        // rule-row trick.
        Some(PANEL_SETTINGS) => render_settings_panel(backend, area, theme, engine),
        Some(PANEL_GIT) => render_source_control(backend, screen, area, engine, theme),
        Some(PANEL_EXTENSIONS) => render_ext_sidebar(backend, area, engine, theme),
        // #635 (Stage 6b item C): AI is no longer deferred — `render_ai_sidebar`
        // dropped its `buf: &mut Buffer` parameter for `&mut dyn Backend`.
        Some(PANEL_AI) => render_ai_sidebar(backend, area, engine, theme),
        Some(PANEL_BOARD) => render_board_panel(backend, screen, area, engine, theme),
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
///
/// #1343: the shell's own `AppShell` sidebar header now paints " SETTINGS"
/// above `area` (quadraui#1055, landed via #1356's pin bump — Settings is a
/// bottom item, so it wasn't reliable until then). This panel used to paint
/// its own " SETTINGS" header row on top of that via
/// `render::paint_sidebar_panel_chrome`'s `SidebarPanelChrome::
/// HeaderAndSearch` — a duplicate header #1256 found (`extensions_header_
/// is_painted` had a GTK-side twin: GTK painted *no* chrome for either
/// panel, so the two backends drifted in opposite directions).
///
/// #1391: composed through `SidebarPanelBody::render_with` (quadraui#1059,
/// the composer `render_explorer_sidebar_content` uses, #1389) with
/// `SidebarPanelChrome::Search` (quadraui#1061) instead of the bespoke
/// `render::paint_sidebar_search_row` (deleted by this issue) — see
/// `render::search_only_chrome`'s doc for why the header stays owned by the
/// shell. GTK's `App::paint_sidebar_panel_rung` `PANEL_SETTINGS` arm builds
/// the identical chrome through the same helper.
pub(super) fn render_settings_panel(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    theme: &Theme,
    engine: &Engine,
) {
    if area.height == 0 {
        return;
    }

    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let q_rect = super::shell_app::to_q_rect(area);
    let panel = render::SidebarPanelBody {
        background: Some(theme.tab_bar_bg),
        chrome: render::search_only_chrome(
            &engine.settings_query,
            "",
            engine.settings_input_active,
            theme,
        ),
        scrollbar_gutter: None,
    };
    panel.render_with(backend, q_rect, |backend, body_rect| {
        // Scrollable form content, via the shared `quadraui::Form` +
        // `FormController` primitive (#479). Inline-edit rows are driven
        // through `FieldKind::TextInput` with a cursor (see
        // `render::settings_to_form`) so there is no separate manual
        // renderer for the edit-in-progress state.
        if body_rect.height <= 0.0 {
            return;
        }

        render::populate_settings_form_controller(engine);
        // Cache the exact rect this frame painted into (#1238) — mirrors
        // `explorer_tree_rect` / `ext_panel_content_rect`. `mouse.rs`'s
        // hit-tests read this back instead of re-deriving `y = area.y + 2`
        // by hand, which drifted the moment the sidebar's own origin was
        // not `y == 0` (e.g. the menu bar visible).
        engine.settings_form_rect.set(body_rect);
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        engine
            .settings_form_controller
            .borrow_mut()
            .render_and_cache(backend, body_rect);
    });
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

/// Paint the `:`-command line row (background fill, text, insert cursor,
/// and the mouse drag-selection highlight) through the shared
/// `quadraui::Backend::draw_command_line_selection` primitive (quadraui#1001).
///
/// #1185: this used to hand-compose a `(char, fg, bg)` cell vector and
/// invert fg/bg per cell for both the cursor and `selection` — the one
/// backend-specific paint path `CLAUDE.md`'s Platform-Neutrality Rule
/// exists to delete, and the reason GTK never got a visual selection
/// highlight at all (there was no shared primitive to paint it through).
/// `render::command_line_view` builds the same `quadraui::CommandLine`
/// descriptor GTK's `FrameOp::CommandLine` arm uses; `selection` (`cmd_sel`'s
/// `(start, end)` character indices, either order, into `command.text`) is
/// converted to the byte-offset pair the primitive expects via
/// `render::command_line_selection_bytes`, the exact twin of GTK's call.
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
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let cmd = render::command_line_view(command);
    let sel_bytes = selection.map(|sel| render::command_line_selection_bytes(&command.text, sel));
    backend.draw_command_line_selection(super::shell_app::to_q_rect(area), &cmd, sel_bytes);
}

// ─── Input translation ────────────────────────────────────────────────────────

/// #605: widened from `&mut TuiBackend` + `&mut Frame` to `&mut dyn Backend`.
/// The three raw-`Buffer` pieces — the full-area background wipe, the
/// focused-hint row, and the "SOURCE CONTROL" header row — all became
/// [`fill_rect`]/[`fill_row`] calls, so `TuiShellApp::render_content` can
/// paint this panel. Everything else here was already a `Backend::draw_*`
/// trait call.
///
/// #1390: the background wipe goes through `SidebarPanelBody::render_with`
/// (quadraui#1059, the composer `render_explorer_sidebar_content` uses,
/// #1389) instead of a standalone [`fill_rect`] call. `chrome` stays
/// [`render::SidebarPanelChrome::None`] — the shell's own sidebar header
/// already titles this panel "SOURCE CONTROL", so the header row painted
/// below (`render::sc_header_status_bar`, live branch/ahead-behind) is body
/// content, not a second title (#1256's double-header bug). `None` chrome
/// reserves no rows, so `render_with`'s `body_rect` is pixel-identical to
/// `area`; the closure keeps using the outer `area` rather than threading a
/// second rect through every call below.
pub(super) fn render_source_control(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let panel = render::SidebarPanelBody {
        background: Some(theme.tab_bar_bg),
        chrome: render::SidebarPanelChrome::None,
        scrollbar_gutter: None,
    };
    let q_rect = super::shell_app::to_q_rect(area);
    panel.render_with(backend, q_rect, |backend, _body_rect| {
        let hdr_fg = theme.status_fg;
        let hdr_bg = theme.status_bg;

        // #1252: SC data comes from the frame's own `screen` (built once by
        // `build_screen_for_shell_content`) instead of a second, independently
        // built `ScreenLayout` — see `render_sidebar_content`'s doc comment.
        let Some(ref sc) = screen.source_control else {
            return;
        };

        // #1361: whether the bottom row is reserved for the focused-hint comes
        // from `render::sc_sidebar_bands` — the exact same derivation
        // `mouse.rs`'s click router uses (row_height 1.0, commit_border 2.0,
        // `sc.has_focus`) — so the reservation this paints can never disagree
        // with what a click is hit-tested against. Painted as a shared
        // `StatusBar` (`render::sc_hint_status_bar`), same mechanism as GTK's
        // `PANEL_GIT` arm, not a raw `fill_row`.
        let bands = render::sc_sidebar_bands(
            &sc.commit_message,
            super::shell_app::to_q_rect(area),
            1.0,
            2.0,
            sc.has_focus,
        );
        if let Some(hint_rect) = bands.hint {
            backend.set_theme(super::quadraui_tui::q_theme(theme));
            let hint_bar = render::sc_hint_status_bar(theme);
            let _ = backend.draw_status_bar_interactive(
                hint_rect,
                &hint_bar,
                &quadraui::InteractionState::new(),
            );
        }
        // #1361 review: the pre-#1361 code additionally gated the whole hint
        // reservation on `area.height > 2`, guarding against a degenerate
        // 1-2 row panel. That guard is gone: `bands.hint.is_some()` alone
        // (i.e. `sc.has_focus`) now decides the reservation, matching GTK
        // (which never had a height guard here) and the single shared
        // `render::sc_sidebar_bands` derivation both painters and both click
        // routers call — see this function's own top-of-block comment.
        // `area.height - 1` cannot underflow: the `area.height == 0` guard at
        // the top of this function already returned, so `area.height >= 1`
        // here, and `1 - 1 == 0` is a valid (if degenerate) zero-row `Rect`,
        // not a panic. A real sidebar is never 1-2 rows tall in practice, so
        // the worst case is the hint/header rows painting over each other in
        // a pathologically tiny panel — a pre-existing cosmetic-only risk
        // `sc_sidebar_bands`'s own `.max(0.0)` on `slab_h` already bounds,
        // not a new crash surface this diff introduces.
        let area = if bands.hint.is_some() {
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
    });
}

// ─── Extension panel (plugin-provided) ───────────────────────────────────────

/// Render an extension-provided sidebar panel.
///
/// #1242: background(none), header/search chrome, tree body and scrollbar
/// gutter compose through quadraui#1041's `SidebarPanelBody::render` — this
/// rung's body is an *owned* per-frame value
/// (`render::ext_panel_to_tree_view`'s fresh `TreeView`, not a persistent
/// controller), so it uses the `&dyn BackendWidget` path
/// (`render::ExtPanelTreeBody`) rather than `render_with`'s closure form
/// (`render_explorer_sidebar_content` above, #1389). The help-popup overlay
/// and the scrollbar's own thumb/track paint stay inline — neither has a
/// `TreeView`/`SidebarPanelBody` equivalent.
pub(super) fn render_ext_panel(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }
    // #1252: reads the frame's own `screen` — see `render_sidebar_content`'s
    // doc comment.
    let Some(ref panel) = screen.ext_panel else {
        engine.ext_panel_tree_layout.replace(None);
        return;
    };

    // #1086: cache the exact rect this frame painted the ext panel into —
    // `AppShellLayout::sidebar_content_bounds`, verbatim, before subtracting
    // this function's own chrome — so click routing (`mouse.rs`'s
    // `SidebarOwner::ExtPanel` arm) can derive its row index from what was
    // actually painted instead of re-deriving the sidebar content's top row
    // from the menu-bar row count by hand. Mirrors `explorer_tree_rect` /
    // `dap_sidebar_body_rect`.
    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    engine.ext_panel_content_rect.set(q_rect);

    // Header (always) + search input (only when active or text) — see this
    // function's own doc for why `SidebarPanelBody::render`'s real
    // `&dyn BackendWidget` path applies here.
    let input_visible = panel.input_active || !panel.input_text.is_empty();
    let header_title = format!(" {}", panel.title);
    let sidebar_panel = render::SidebarPanelBody {
        background: None,
        chrome: if input_visible {
            render::SidebarPanelChrome::HeaderAndSearch {
                header: header_title,
                query: panel.input_text.clone(),
                placeholder: String::new(),
                active: panel.input_active,
            }
        } else {
            render::SidebarPanelChrome::Header(header_title)
        },
        // 1 col reserved for the scrollbar, unconditionally — `draw_tree`
        // "doesn't render scrollbars yet" (see below), so this file paints
        // its own into the gutter `layout.scrollbar_rect` reserves.
        scrollbar_gutter: Some(1.0),
    };
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let body = render::ExtPanelTreeBody(render::ext_panel_to_tree_view(panel, theme));
    let layout = sidebar_panel.render(backend, q_rect, &body);

    // ── Scrollbar + click-routing cache: only when the body actually got
    // rows to show (chrome could have consumed the whole area). ───────────
    if layout.body_rect.height > 0.0 {
        // #1089: cache the exact `Backend::tree_layout` this frame painted
        // with — the click router (`render::route_ext_panel_click`, shared
        // with the GTK/macOS/Win `App`) reads this instead of re-deriving
        // row geometry from a uniform row height. See
        // `Engine::ext_panel_tree_layout`'s own doc for why that matters on
        // the pixel backends even though TUI's own rows are uniform.
        let tree_layout = backend.tree_layout(layout.body_rect, &body.0);
        engine
            .ext_panel_tree_layout
            .replace(Some((layout.body_rect, tree_layout)));

        // Scrollbar: `draw_tree` doesn't render scrollbars yet. Total
        // visible rows = tree.rows.len() (sections + their expanded items,
        // separators included — same flat count the legacy renderer
        // produced). #635: the manual `set_cell` thumb/track loop is now
        // one [`fill_row`] call per row (the rule-row trick #605 used for
        // the settings/source-control/extensions sidebar chrome) instead
        // of a raw `Buffer` write.
        let total = body.0.rows.len();
        let ext_panel_scrollbar = if let Some(sb_rect) = layout.scrollbar_rect {
            let track_h = sb_rect.height as usize;
            if total > track_h && track_h > 0 {
                let scroll = panel.scroll_top;
                let sb_x = sb_rect.x as u16;
                let sb_y = sb_rect.y as u16;
                let thumb_h = (track_h * track_h / total).max(1);
                let thumb_top = scroll * track_h / total;
                for i in 0..track_h {
                    let y = sb_y + i as u16;
                    let (ch, fg) = if i >= thumb_top && i < thumb_top + thumb_h {
                        ('\u{2588}', theme.scrollbar_thumb)
                    } else {
                        ('\u{2591}', theme.scrollbar_track)
                    };
                    fill_row(backend, sb_x, y, 1, &ch.to_string(), fg, theme.background);
                }
                Some(quadraui::SurfaceScrollbar {
                    axis: quadraui::ScrollAxis::Vertical,
                    track_bounds: sb_rect,
                    thumb_bounds: quadraui::Rect::new(
                        sb_rect.x,
                        sb_rect.y + thumb_top as f32,
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
            }
        } else {
            None
        };
        engine
            .scroll_surfaces
            .borrow_mut()
            .push(quadraui::ScrollSurface {
                id: quadraui::WidgetId::new("ext_panel:sb"),
                bounds: q_rect,
                scrollbar: ext_panel_scrollbar,
            });
    } else {
        engine.ext_panel_tree_layout.replace(None);
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
/// Returns `(link_rects, popup_rect)`. `link_rects` carries the trailing
/// `is_native` flag (#1067) the same way GTK's `render::
/// panel_hover_popup_paint` does — both caches are now the exact same
/// element shape, so `render::route_panel_hover_popup_click` is callable
/// from either backend without an adapter.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn render_panel_hover_popup(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    theme: &Theme,
    sidebar_right_x: u16,
    sidebar_y: u16,
    sidebar_height: u16,
    term_area: Rect,
) -> (Vec<(quadraui::Rect, String, bool)>, Option<quadraui::Rect>) {
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
        // #1087: `ph.item_index` is a flat index across the whole panel list
        // (`route_sidebar_hover`'s `ExtPanel` arm sets `flat_idx =
        // ext_panel_scroll_top + row`), not a screen row — this used to
        // anchor straight off the flat index (`item_index + 1`), landing the
        // card dozens of rows below the viewport once the panel scrolled.
        // `ext_panel_hover_screen_row`/`ext_panel_chrome_rows` are the same
        // shared derivation `panel_hover_anchor_y` (GTK's twin of this
        // function) now uses, so the two backends can't drift on this again.
        let Some(panel) = screen.ext_panel.as_ref() else {
            return (vec![], None);
        };
        let Some(screen_row) = render::ext_panel_hover_screen_row(panel, ph.item_index) else {
            // Stale frame right after a scroll: `item_index` hasn't caught
            // up with `scroll_top` yet. Skip painting rather than underflow.
            return (vec![], None);
        };
        render::ext_panel_chrome_rows(panel) as u16 + screen_row as u16
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

    let is_native = render::panel_hover_link_is_native(&ph.panel_name);
    let link_rects: Vec<(quadraui::Rect, String, bool)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (*rect, url, is_native)
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

// ─── Extensions sidebar panel ─────────────────────────────────────────────────

/// Render the Extensions sidebar panel.
///
/// Migrated to `quadraui::MultiSectionView` (#293). The two "INSTALLED" /
/// "AVAILABLE" sections (each with its own `TreeView` body) are a
/// `MultiSectionView` built by `render::ext_sidebar_to_multi_section_view`
/// and rasterised via `quadraui::tui::draw_multi_section_view`. Both the
/// section-header chevrons / titles and per-section scrollbars come from
/// the primitive — there is no per-backend section-walk code that paint
/// and click could disagree on (the structural fix for the #281 bug
/// classes).
///
/// #1343: this panel used to also hand-paint its own " EXTENSIONS" header
/// row (via [`fill_row`]) on top of the shell's own `AppShell` sidebar
/// header — a real double-header #1256 found by driving the shipped TUI.
/// The shell already paints " EXTENSIONS " above `area`, so only the
/// search row is painted here.
///
/// #1391: composed through `SidebarPanelBody::render_with` (quadraui#1059)
/// with `SidebarPanelChrome::Search` (quadraui#1061) instead of the bespoke
/// `render::paint_sidebar_search_row` (deleted by this issue) — see
/// `render::search_only_chrome`'s doc. GTK's `App::paint_sidebar_panel_rung`
/// `PANEL_EXTENSIONS` arm builds the identical chrome through the same
/// helper.
pub(super) fn render_ext_sidebar(
    backend: &mut dyn quadraui::Backend,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    backend.set_theme(super::quadraui_tui::q_theme(theme));
    let q_rect = super::shell_app::to_q_rect(area);
    let panel = render::SidebarPanelBody {
        background: None,
        chrome: render::search_only_chrome(
            &engine.ext_sidebar_query,
            "Search extensions (press /)",
            engine.ext_sidebar_input_active,
            theme,
        ),
        scrollbar_gutter: None,
    };
    panel.render_with(backend, q_rect, |backend, body_rect| {
        // ── SidebarSystem body: rest of the panel ──────────────────────
        engine.ext_sidebar_body_rect.set(body_rect);
        if body_rect.height <= 0.0 {
            return;
        }
        render::populate_ext_sidebar_system(engine);
        backend.set_theme(super::quadraui_tui::q_theme(theme));
        engine
            .ext_sidebar_system
            .borrow()
            .render(backend, body_rect);
    });
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
    // #956 (ACP-5): slash-command completions, painted on top — no-op
    // unless the input matches an agent-declared command.
    render::paint_ai_command_completions(backend, engine, q_area);
}

// ─── Board panel (#521) ─────────────────────────────────────────────────────

/// Render the Board panel — a generic host for the shared `quadraui::Board`
/// component. Per the Platform-Neutrality Rule, the *only* TUI-specific code
/// here is picking the rect and calling `Backend::draw_board`/
/// `Backend::draw_status_bar`; GTK's `App::paint_sidebar_panel_rung`
/// `PANEL_BOARD` arm makes the identical pair of calls.
pub(super) fn render_board_panel(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let Some(ref board) = screen.board else {
        return;
    };
    let q_area = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    backend.set_theme(super::quadraui_tui::q_theme(theme));
    if let Some(ref model) = board.model {
        let layout = backend.draw_board(q_area, model);
        engine.board_layout.replace(Some(layout));
    } else {
        engine.board_layout.replace(None);
        if let Some(ref status) = board.status {
            let bar = render::board_status_bar(status, theme);
            let row = quadraui::Rect::new(q_area.x, q_area.y, q_area.width, 1.0);
            let _ =
                backend.draw_status_bar_interactive(row, &bar, &quadraui::InteractionState::new());
        }
    }
}

// ─── Debug sidebar panel ──────────────────────────────────────────────────────

/// Render the debug sidebar: title + Run/Stop button chrome, then the four
/// `quadraui::TreeView` sections (Variables, Watch, Call Stack, Breakpoints)
/// via `SidebarSystem`.
///
/// #1392: chrome composed through `SidebarPanelBody::render_with`
/// (quadraui#1059) with `SidebarPanelChrome::StatusBars` (quadraui#1061,
/// `render::debug_sidebar_chrome`) instead of slicing `area` into two rows
/// by hand and calling `Backend::draw_status_bar` on each directly. The
/// returned layout's `status_bar_hit_regions` — already in `area`'s own
/// absolute space — is stored straight onto `Engine::dap_sidebar_action_hits`
/// for `mouse::handle_mouse`'s `dap_sidebar_action_click_at` to read, so
/// paint and click share one geometry. GTK's `App::paint_sidebar_panel_rung`
/// `PANEL_DEBUG` arm builds the identical chrome through the same helper.
/// #607: `backend` widened to `&mut dyn quadraui::Backend` — this renderer
/// was already trait-pure, same rationale as `render_search_panel` above.
pub(super) fn render_debug_sidebar(
    backend: &mut dyn quadraui::Backend,
    screen: &render::ScreenLayout,
    area: Rect,
    engine: &Engine,
    theme: &Theme,
) {
    if area.height == 0 {
        return;
    }

    // #1252: reads the frame's own `screen` — see `render_sidebar_content`'s
    // doc comment — instead of rebuilding a second `ScreenLayout` just to
    // read `debug_sidebar`.
    let sidebar = &screen.debug_sidebar;
    let q_theme = super::quadraui_tui::q_theme(theme);
    backend.set_theme(q_theme);

    let q_rect = quadraui::Rect::new(
        area.x as f32,
        area.y as f32,
        area.width as f32,
        area.height as f32,
    );
    let panel = render::SidebarPanelBody {
        background: None,
        chrome: render::debug_sidebar_chrome(sidebar, theme),
        scrollbar_gutter: None,
    };
    let layout = panel.render_with(backend, q_rect, |backend, body_rect| {
        engine.dap_sidebar_body_rect.set(body_rect);
        render::populate_dap_sidebar_system(engine);
        backend.set_theme(q_theme);
        engine
            .dap_sidebar_system
            .borrow()
            .render(backend, body_rect);
    });
    engine
        .dap_sidebar_action_hits
        .replace(layout.status_bar_hit_regions);
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
        // #1252: `render_source_control` now reads the frame's own `screen`
        // instead of rebuilding one itself — build it once here, matching
        // what `render_content` does via `build_screen_for_shell_content`.
        let screen = render::build_screen_layout(
            engine,
            &theme,
            &[],
            1.0,
            1.0,
            true,
            0.0,
            render::TUI_MINIMAP_SIZING,
        );
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
                    render_source_control(backend, &screen, area, engine, &theme);
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
// The ring's ordering is the thing under test: hamburger, the seven fixed
// panels, the dynamic extension panels spliced in *before* Settings, and
// Settings pinned last — while the legacy `activity_bar_selected` index space
// numbers Settings at 8 and extension panels at 9+. Before #536 that mismatch
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

        for expected_row in 1..=7 {
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

    /// With no extension panels, `j` past the last fixed panel (Board, #521)
    /// lands on Settings — which paints *pinned to the bottom edge*, not on
    /// row 8 — and saturates there. `k` comes straight back to Board.
    #[test]
    fn ring_steps_from_board_to_bottom_pinned_settings_and_saturates() {
        let mut e = ring_engine();
        e.activity_bar_focus_in_at(7); // Board, the last fixed panel
        assert_eq!(painted_ring(&e).map(|(r, _)| r), Some(7));

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(BAR_H - 1),
            "Settings is bottom-pinned, so the ring must jump to the last row"
        );
        assert_eq!(e.activity_bar_selected, 8, "Settings is toolbar index 8");

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(BAR_H - 1),
            "j on the bottom-most item must saturate, not wrap to the top"
        );

        e.activity_bar_move_up();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(7),
            "k from Settings with no extension panels returns to Board"
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

    /// The headline ordering claim: extension panels splice in **between**
    /// Board (#521) and Settings in painted order (sorted by name), even
    /// though the legacy index space numbers them *after* Settings. Walking
    /// `j` from Board must visit both extension icons and only then reach
    /// Settings.
    #[test]
    fn ring_splices_extension_panels_between_board_and_settings() {
        let mut e = ring_engine();
        add_ext(&mut e, "zz-last", 'Z');
        add_ext(&mut e, "aa-first", 'A');
        e.activity_bar_focus_in_at(7); // Board

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e),
            Some((8, 'A')),
            "j from Board must land on the first extension panel (sorted by name)"
        );
        assert_eq!(e.activity_bar_selected, 9, "…which is toolbar index 9");

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e),
            Some((9, 'Z')),
            "j must then land on the second extension panel"
        );
        assert_eq!(e.activity_bar_selected, 10);

        e.activity_bar_move_down();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(BAR_H - 1),
            "only after the last extension panel does j reach bottom-pinned Settings"
        );
        assert_eq!(e.activity_bar_selected, 8);

        // …and `k` from Settings walks back onto the *last* extension panel.
        e.activity_bar_move_up();
        assert_eq!(painted_ring(&e), Some((9, 'Z')));
        assert_eq!(e.activity_bar_selected, 10);

        e.activity_bar_move_up();
        assert_eq!(painted_ring(&e), Some((8, 'A')));

        e.activity_bar_move_up();
        assert_eq!(
            painted_ring(&e).map(|(r, _)| r),
            Some(7),
            "k off the first extension panel returns to Board, not to Settings"
        );
        assert_eq!(e.activity_bar_selected, 7);
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
        e.activity_bar_focus_in_at(10); // second ext panel — no longer exists
        assert_eq!(
            painted_ring(&e),
            None,
            "a selection naming no item paints no ring"
        );

        e.activity_bar_move_up();
        assert_eq!(
            e.activity_bar_selected, 9,
            "k must recover onto the one extension panel that does exist"
        );
        assert_eq!(painted_ring(&e).map(|(r, _)| r), Some(8));
    }
}
