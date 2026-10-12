use super::dispatch::debug_assert_command_line_fits_viewport;
use super::*;

impl quadraui::ShellApp for App {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // Read the live kitty-keyboard-protocol capability once, at setup time.
        // The backend field access is plain, never I/O. A GUI backend's
        // `BackendCaps::kitty_keyboard` is always `false` (GDK/AppKit/Win32
        // hand over an already-resolved keysym per physical key); TUI's
        // capability is unaffected by living on the shared `App` instead of a
        // TUI-only shell — `backend.backend_caps()` reads the same `TuiBackend`
        // state either way.
        self.keyboard_enhanced = backend.backend_caps().kitty_keyboard;
        // Lets `Engine` render command-palette/picker shortcut hints in the
        // running backend's convention (`⌘S` on macOS, `Ctrl+S` elsewhere) —
        // see `format_shortcut_for_platform`.
        self.engine.borrow_mut().platform_name = backend.services().platform_name();
        // VS Code's editor scrollbar is 14px wide; a
        // no-op on TUI, whose scrollbar is one cell.
        backend.set_editor_v_scrollbar_width(Some(14.0));
        // `shell_config`'s `with_editor_font` seed covers family/size only.
        // The runner's `set_editor_font` call resets `current_line_height` to
        // this backend's natural font metric every time. Re-apply the VS Code
        // override here too, so frame 1's shell-level layout (computed by the
        // runner *before* `render_content`'s per-frame override) sees the same
        // line height as frame 2+ — avoiding one-frame geometry snap that throws
        // off hit-tests. Same call `sync_per_frame_backend_state` makes every
        // frame; see `resolve_editor_line_height_px`'s doc for the macOS-only
        // scoping.
        let (_, editor_size_pt) = resolve_editor_font(&self.engine.borrow().settings, backend);
        if let Some(lh) =
            resolve_editor_line_height_px(&self.engine.borrow().settings, backend, editor_size_pt)
        {
            quadraui::Backend::set_current_line_height(backend, lh);
        }
        // Seed cached metrics from runner defaults.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.cached_ui_line_height = self.cached_line_height;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);
        // Seed the backend's nerd-fonts flag. This call is the live replacement
        // for the prior `Msg::CacheFontMetrics` arm, which stopped firing after
        // the ShellApp migration, silently freezing quadraui's GTK backend at
        // its default of `false` — the cause of the explorer treeview falling
        // back to ASCII icons.
        render::sync_nerd_fonts(backend, &self.engine.borrow());
        // Register the bundled Nerd Font subset and point the
        // backend's fallback cascade at it — required for glyphs to resolve
        // at all on Core Text/DirectWrite backends (macOS/Win-GUI); see
        // `render::register_nerd_font_fallback`'s doc for why this is a
        // one-time `setup()` call, not part of the per-frame sync above.
        render::register_nerd_font_fallback(backend);

        // Try to drop the server-side WM titlebar now, in favour of the
        // drawn CSD row; `setup()` runs before `run_with_shell`'s runner
        // calls `window.present()`, so `backend.window()` is very likely
        // still `None` here. `tick()` retries every frame until the window
        // is mapped, which is the reliable path.
        self.capture_window_and_apply_csd(backend);

        // Same "very likely still None here, tick() retries" story as the
        // CSD drop above, for restoring the saved window size/position/
        // maximized state instead.
        self.restore_window_geometry(backend);

        // GTK draws its own VSCode-style menu bar (File/Edit/View/...) — it
        // acts as the client-side titlebar, always visible. A backend that
        // declares `BackendCaps::native_menu` (macOS's `MacBackend`) has a real
        // OS menu bar — installing the *drawn* row on top of it would paint a
        // redundant in-window menu underneath the system one. Same `MenuDef`s
        // either way — `render::menu_defs_to_menu_bar` just reshapes them.
        let is_vscode_mode = self.engine.borrow().is_vscode_mode();
        let menu_defs = render::build_menu_defs(is_vscode_mode);
        if backend.backend_caps().native_menu {
            let bar = render::menu_defs_to_menu_bar(&menu_defs);
            // `install_menu_bar`'s macOS implementation (`MacBackend`) asserts it is
            // called on the real AppKit main thread and panics otherwise. Every real
            // invocation of `ShellApp::setup` is on the main thread, so this never
            // fires outside a test harness — but test helpers call it from spawned
            // test threads. Catching it here keeps `setup()` from taking the whole
            // test process down over a call it doesn't otherwise depend on. The real
            // fix upstream is for `install_menu_bar` to degrade gracefully off-main-
            // thread the way its sibling test helpers already do. `WinBackend` no
            // longer declares `native_menu` (reverted in quadraui#1228), so
            // `MacBackend` remains the only in-tree `native_menu` implementation.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                backend.install_menu_bar(&bar);
            }))
            .is_err()
            {
                eprintln!(
                    "vimcode: Backend::install_menu_bar panicked (quadraui \
                     main-thread assertion, see vimcode#901) -- the native \
                     menu bar may be missing"
                );
            }
            self.engine.borrow_mut().menu_bar_visible = false;
        } else if backend.backend_caps().window_chrome {
            // GTK's (and any future Win-GUI's) drawn menu bar doubles as the
            // client-side titlebar — pinned visible always.
            self.engine.borrow_mut().menu_bar_visible = true;
        } else {
            // No OS menu bar to hide behind (`native_menu`) and no window chrome for
            // the drawn row to double as (`window_chrome`) — the `cell` profile
            // (TUI-via-`App`) today. The bar is fully hideable at runtime and starts
            // however `Engine::new` already resolved `menu_bar_visible` — left
            // untouched here, unlike the two arms above, which both *force* a value.
            self.engine.borrow_mut().menu_bar_toggleable = true;
        }
        self.engine
            .borrow()
            .menu_system
            .borrow_mut()
            .set_menus(menu_defs);

        // Register the panel-keys accelerator set (toggle sidebar, fuzzy
        // finder, live grep, command palette, ...) on the runner's backend.
        // This was previously only wired for TUI; the GTK side's registration
        // function existed but was never called after the ShellApp migration.
        render::register_panel_accelerators(backend, &self.engine.borrow().settings.panel_keys);
    }

    fn render_content(
        &self,
        backend: &mut dyn quadraui::Backend,
        layout: &quadraui::AppShellLayout,
    ) {
        let engine = self.engine.borrow();
        let theme = Theme::from_name(&engine.settings.colorscheme);
        self.sync_per_frame_backend_state(backend, &engine, &theme);

        // Resolved here (cheap — `resolve_editor_font` is a pure lookup over
        // `settings`/`backend.default_fonts()`), so the `FrameOp::SidebarPanel`
        // arm below can briefly reset `backend` to this font's natural line
        // height for sidebar content, then restore the editor's override.
        let (editor_family, editor_size_pt) = resolve_editor_font(&engine.settings, backend);
        let lh = self.cached_line_height.max(backend.line_height() as f64);
        let cw = self.cached_char_width.max(backend.char_width() as f64);
        // Publish the value this frame paints with so click-time hit-tests can
        // use it. `render_content` takes `&self`, so it cannot write
        // the plain `cached_line_height` field — which is seeded once in
        // `setup()` from the runner's *default* metrics and can therefore be
        // smaller than the `lh` every frame actually paints with. Hit-testing
        // painted geometry against the smaller value put row boundaries in the
        // wrong place (the picker resolved clicks two rows off) and clipped
        // the bottom of every single-row band, breadcrumbs included.
        self.painted_line_height.set(Some(lh));
        self.painted_char_width.set(Some(cw));

        // Correct this frame's layout for the hamburger's own phantom sidebar
        // reservation — a no-op unless `engine.menu_bar_toggleable` (the `cell`
        // profile; always `false` on GTK/macOS/Win).
        let corrected_layout = render::reclaim_hamburger_sidebar_reservation(&engine, layout);
        let layout = &corrected_layout;

        let main = layout.main_content_bounds;
        // `h` is the *recovered* content height, not
        // `main.height` directly — `shell_config`'s
        // `with_command_line()`/`with_status_bar()` reservation already
        // shrank `main_content_bounds` by one static row each so the
        // activity bar/sidebar stop above them; every line below this one
        // keeps assuming `h` reaches the window's true bottom edge (it
        // subtracts vimcode's own *dynamic* bottom-chrome height itself),
        // so `h` has to be un-shrunk back to that same true height here —
        // see `render::main_content_true_height`'s doc.
        let (x, y, w, h) = (
            main.x as f64,
            main.y as f64,
            main.width as f64,
            render::main_content_true_height(layout) as f64,
        );
        if w < 1.0 || h < 1.0 {
            return;
        }

        // `shell_config`'s bottom-chrome reservation vacates a strip left of
        // `main_content_bounds` — below the now-shrunk activity bar / sidebar —
        // that nothing else paints. Fill it explicitly with the same chrome
        // colour the activity bar itself paints, every frame, on every backend.
        // A no-op (`None`) once there is nothing left to reclaim — no
        // reservation active, or a window too short for either dimension to be
        // positive.
        if let Some(fill_rect) = render::bottom_chrome_reservation_fill_rect(layout) {
            backend.draw_solid_fill(fill_rect, theme.tab_bar_bg);
        }

        // ── Layout ────────────────────────────────────────────────────────────
        let tab_row_h = (self.units.tab_row_h)(lh);
        let tab_bar_h = (self.units.tab_bar_h)(lh, engine.settings.breadcrumbs);
        // Whether the *global* (non-per-window) status bar occupies its own row —
        // `global_status_bar_visible`, not `effective_window_status_line`
        // directly. `'laststatus'` can hide the status line entirely (0, or 1
        // with a single window) even while per-window status is off, in which
        // case there is still no separate global row to offset the wildmenu past.
        let global_status_visible = render::global_status_bar_visible(&engine);
        let el = render::compute_editor_layout(&engine, h, lh, false);
        // `el.status_bar_h` is `compute_editor_layout`'s single source of
        // truth for this (identical formula to the `wildmenu_px`/
        // `status_rows` locals this replaced); reusing it here — instead of
        // recomputing a second copy — is what makes `editor_area_h` below
        // `el.editor_bottom` correctly reserve quickfix's band too.
        let status_bar_h = el.status_bar_h;
        // `el.editor_bottom` already subtracts quickfix_h/terminal_h/
        // debug_toolbar_h/separated_status_h/status_bar_h from `h` (menu_h is 0
        // for GTK). This is `editor_area_h` — the exact space the editor fills.
        let editor_area_h = el.editor_bottom.max(0.0);

        let editor_bounds = WindowRect::new(x, y, w, editor_area_h);
        // Hand the exact bounds/tab-bar-height this frame painted with to the
        // click + drag handlers, so divider hit-tests land on the painted line
        // instead of on a second, differently-originated guess.
        self.cached_editor_bounds
            .set(Some((editor_bounds, tab_bar_h)));
        // The raw `h` this frame's `compute_editor_layout` call above
        // was given — see `cached_main_content_height`'s own doc for why
        // this is `h`, not `editor_area_h`/`editor_bounds`'s height.
        self.cached_main_content_height.set(h);
        let (window_rects, _dividers) =
            engine.calculate_group_window_rects(editor_bounds, tab_bar_h);

        // The breadcrumb row's painted bounds must agree with the fixed-pixel
        // space `tab_bar_h` already reserved for it — plain `build_screen_layout`
        // would assume the breadcrumb row is exactly one `lh`-tall editor text
        // line, which is no longer true now that the row is a fixed 22px.
        let screen = render::build_screen_layout_with_breadcrumb_row(
            &engine,
            &theme,
            &window_rects,
            lh,
            cw,
            false,
            (self.units.breadcrumb_row_h)(lh),
            backend.scrollbar_reserve() as f64,
            self.units.minimap,
        );

        // Cache for click handlers (move into RefCell, then borrow back for drawing).
        *self.cached_screen_layout.borrow_mut() = Some(screen);
        let screen_ref = self.cached_screen_layout.borrow();
        let screen = screen_ref.as_ref().unwrap();

        // Mouse clicks resolve editor columns via the per-glyph Pango inverse
        // (`Backend::editor_col_at_x`), not a naive uniform-cell division — see
        // `pixel_to_click_target`'s call site for the emoji/CJK drift it fixed.
        // Before #1104, click-time hit-testing ran against a second `GtkBackend`
        // kept in sync every frame. #1104 threaded the live `backend` through the
        // click/drag/modal-stack call chain instead. `self.backend` still exists
        // for callers that cannot reach the live backend: `explorer_ui_event`,
        // `route_ai_sidebar_event` and DAP-sidebar key route call
        // `quadraui::Backend::set_current_line_height`/`set_current_char_width`
        // to undo click drift, from deep inside the keyboard/mouse dispatch tree.

        // ══ Editor band ═════════════════════════════════════════════════════
        // Composed from `render::compose_editor_band`, then the `FrameHitMap`
        // is recovered from the very objects that walk painted.
        self.compose_editor_band_rungs(
            backend,
            &engine,
            screen,
            &theme,
            quadraui::Rect::new(x as f32, y as f32, w as f32, 0.0),
            (lh, cw),
            (tab_row_h, tab_bar_h),
        );

        // ── Editor-anchored popups (on top of the editor band) ────────────────
        // Completion menu, LSP hover, editor hover (rich markdown), diff peek,
        // signature help — see `paint_editor_popups_rung`. Not a `FrameOp`
        // rung: they are anchored to the *active window's* cursor rather than
        // to a band, and TUI composes them through its own
        // `paint_editor_popups` at exactly this point in the frame, between
        // the editor band and the bottom band.
        self.paint_editor_popups_rung(
            backend,
            screen,
            &theme,
            quadraui::Rect::new(x as f32, y as f32, w as f32, h as f32),
            lh,
            cw,
        );

        // ══ Bottom band ═════════════════════════════════════════════════════
        // Composed from `render::compose_bottom_band`.
        self.compose_bottom_band_rungs(
            backend,
            &engine,
            screen,
            layout,
            &theme,
            quadraui::Rect::new(x as f32, y as f32, w as f32, h as f32),
            &el,
            editor_area_h,
            (lh, cw),
        );

        // ══ Frame sequence ══════════════════════════════════════════════════
        // Composed from `render::compose_frame` — the single ordered artefact
        // both backends walk for everything around and on top of the editor
        // column. Geometry and rasterisation stay here, in pixels; only the
        // *order* and the *gates* are shared. Caches whose "absent" branch used
        // to live in an `else` are cleared here, before the walk: `compose_frame`
        // returns only the live rungs, so an absent rung has no arm left to run.
        let status_y = y + h - status_bar_h;
        self.engine
            .borrow()
            .global_status_rect
            .set(quadraui::Rect::default());
        self.global_status_zones.borrow_mut().clear();
        self.painted_sidebar_bounds
            .set(layout.sidebar_content_bounds);

        // The title-bar band's rects are published unconditionally — empty when
        // the shell reserved nothing this frame — because `handle()`'s click
        // routing reads them on a *later* frame and must never resolve against
        // a stale value. The `MenuRow` arm below overwrites them when the rung
        // is live.
        let menu_row_rect = layout.title_bar_bounds.unwrap_or_default();
        self.menu_row_rect.set(menu_row_rect);
        self.menu_items_rect.set(menu_row_rect);
        self.title_bar_rect.set(quadraui::Rect::default());
        let mut app_icon_rect = quadraui::Rect::default();
        let mut menu_items_rect = menu_row_rect;
        let mut controls_rect: Option<quadraui::Rect> = None;
        let mut command_center_rect: Option<quadraui::Rect> = None;

        // The overlay tail's caches are cleared here for the same reason: the
        // tail is part of this one walk now, so a rung whose gate is off has no
        // arm left to run. A stale `dialog_layout` / `context_menu_layout` /
        // `picker_popup_rect` / `tab_switcher_popup_rect` resolves the next
        // click against last frame's geometry — the #587 class of bug.
        self.engine.borrow().command_center_layout.replace(None);
        self.picker_popup_rect.set(None);
        self.folder_picker_popup_rect.set(None);
        self.tab_switcher_popup_rect.set(None);
        *self.context_menu_layout.borrow_mut() = None;
        *self.dialog_layout.borrow_mut() = None;
        self.engine.borrow().toast_layout.replace(None);
        // A *native* dialog is not a frame rung, so the arm no longer runs for it.
        // A native dialog must be presented exactly once per open — `native_dialog_shown`
        // is the edge: the first `render_content` call to see a given open queues
        // the present and flips the flag; a *closed* dialog re-arms it. This edge
        // trigger is hoisted out of the `Dialog` arm because it doesn't compose.
        // `quadraui::native_dialog_options` is a pure content-shape check — it
        // says nothing about whether *this* backend actually has a native alert
        // facility to show it with. Gating here keeps `presence.dialog` below
        // correct — an ungated `native_dialog_shown` would suppress the in-canvas
        // rung on a backend with no native dialog at all (TUI), leaving the
        // dialog painted nowhere.
        match screen
            .dialog
            .as_ref()
            .map(render::dialog_panel_to_quadraui_dialog)
            .as_ref()
            .and_then(quadraui::native_dialog_options)
            .filter(|_| backend.backend_caps().native_dialogs)
        {
            Some(opts) => {
                if !self.native_dialog_shown.get() {
                    self.native_dialog_shown.set(true);
                    self.pending_native_dialog.set(Some(opts));
                }
            }
            None => {
                if screen.dialog.is_none() {
                    self.native_dialog_shown.set(false);
                }
            }
        }

        let popup_vp = backend.viewport();
        let popup_viewport = quadraui::Rect::new(0.0, 0.0, popup_vp.width, popup_vp.height);
        // Built once, before the walk, because its presence gate and its
        // `ToastStack` arm need the same value and `build_toast_stack` is not
        // free.
        let toast_stack = render::build_toast_stack(&engine);

        let mut presence =
            render::FramePresence::from_screen(screen, layout, render::FrameMetrics::px(lh, cw));
        presence.toast_stack = toast_stack.is_some();
        // Shared with TUI now — see `render::FrameOp::FolderPicker`.
        presence.folder_picker = self.folder_picker.borrow().is_some();
        // A natively-expressible dialog is presented by the OS, not
        // composed into this frame, so the rung is not live. `dialog_layout`
        // stays cleared above and nothing is recorded — the sequence describes
        // what reached the canvas.
        presence.dialog = screen.dialog.is_some() && !self.native_dialog_shown.get();

        // Measure the title-bar band whenever the Command Center rung
        // is live, independent of whether `FrameOp::MenuRow` itself composes.
        // Before this, `controls_rect`/`command_center_rect` were populated
        // *only* inside the `MenuRow` match arm below, which is gated on
        // `presence.menu_row` — i.e. on `menu_bar_visible`. A native-menu
        // backend (macOS's `MacBackend`) sets `menu_bar_visible = false` to
        // suppress the redundant in-window row under AppKit's real menu bar,
        // which left `command_center_rect` permanently `None` and
        // the `FrameOp::CommandCenter` arm's `command_center_rect.filter(...)`
        // guard always failed — the omnibar never painted even once its own
        // presence gate was split from `menu_row`'s (see
        // `render::FramePresence::from_screen`).
        //
        // When the drawn row itself is suppressed, measure with an *empty*
        // `MenuBar` and no controls bar: `measure_title_bar_bands` collapses
        // the menu-item and controls slots to zero width in that case (see
        // its doc), so the Command Center gets the *entire* band rather than
        // reserving room for labels and buttons that will never paint. This
        // is also why `controls_rect` stays `None` on a native-menu backend:
        // the only arm that ever paints from it (`FrameOp::MenuDropdown`'s
        // `paint_title_bar_band`) stays gated on `presence.menu_dropdown` —
        // itself still coupled to `menu_bar_visible` — so this does not
        // resurrect drawn window controls under AppKit's own traffic lights.
        if presence.command_center {
            // A client-side-titlebar-capable backend (macOS's `MacBackend`) reports
            // how much of the band's leading edge its own native controls already
            // occupy. `Rect::default()` means "nothing of the backend's own is in
            // this band", so on GTK/Win-GUI/TUI `leading_inset` is `0.0` and every
            // line below is a no-op.
            let control_inset = backend.titlebar_control_inset();
            let leading_inset = control_inset.width.max(0.0);
            let inset_menu_row_rect =
                render::inset_titlebar_row_leading_edge(menu_row_rect, leading_inset);

            let (real_icon_rect, real_items_rect) =
                render::split_menu_row_for_app_icon(menu_row_rect, leading_inset);
            let (items_for_measure, bar_for_measure) = if presence.menu_row {
                // `app_icon_rect` is only ever assigned here, so on macOS (where
                // `presence.menu_row` is always `false` — #901 suppresses the drawn row
                // under AppKit's real menu bar) it never picks up a real value. Its only
                // reader (`FrameOp::MenuDropdown`'s `paint_title_bar_band`) is itself
                // gated on `presence.menu_dropdown`, which stays coupled to
                // `menu_bar_visible` and so is also always `false` on macOS.
                app_icon_rect = real_icon_rect;
                (real_items_rect, engine.menu_system.borrow().menu_bar())
            } else {
                (
                    inset_menu_row_rect,
                    quadraui::MenuBar {
                        id: quadraui::WidgetId::new("native_menu_row_suppressed"),
                        items: Vec::new(),
                        open_item: None,
                        focused_item: None,
                    },
                )
            };
            menu_items_rect = items_for_measure;
            self.menu_items_rect.set(menu_items_rect);

            // A backend that draws its own controls must never *also* get vimcode's
            // drawn `controls_bar` — two sets of window controls is exactly the bug
            // #940 exists to prevent. `presence.menu_row` still gates it the same way
            // it always did on every other backend. Pure decision extracted to
            // `render::should_draw_window_controls` so it has a unit test independent
            // of any backend/driver. Queries `WindowControl::is_maximized()` live
            // instead of caching.
            let draw_controls =
                render::should_draw_window_controls(presence.menu_row, control_inset);
            // See `paint_title_bar_band`'s identical read for why
            // this queries `WindowControl::is_maximized()` live instead of
            // caching.
            let maximized = backend
                .window()
                .and_then(|w| w.is_maximized().ok())
                .unwrap_or(false);
            let controls_bar =
                draw_controls.then(|| render::window_controls_status_bar(&theme, maximized));
            let bands = render::measure_title_bar_bands(
                backend,
                menu_row_rect,
                items_for_measure,
                &bar_for_measure,
                controls_bar.as_ref(),
            );
            self.title_bar_rect.set(bands.controls);
            controls_rect = draw_controls.then_some(bands.controls);
            command_center_rect = Some(bands.command_center);
        }

        // `presence.change_review` is the exact gate `compose_frame` uses to
        // decide whether `FrameOp::ChangeReview` is in this frame's op list.
        // That rung's arm below can never run on the frame the surface *closes*.
        // Reconcile the modal-stack entry here, once, unconditionally, ungated by
        // which rungs compose — exactly how `reconcile_editor_hover_modal` is.
        // called.
        render::reconcile_change_review_modal_stack(
            backend,
            presence.change_review,
            popup_viewport,
        );

        let mut composed: Vec<render::FrameOp> = Vec::new();
        for op in render::compose_frame(&presence) {
            match op {
                // ── Menu bar row (client-side chrome): measure only ──────────────
                // `ShellConfig::with_title_bar()` reserves a full-width band across the
                // top of the *entire* shell — above the activity bar and sidebar too.
                // GTK's drawn menu bar + inline window controls span the whole window like
                // a real titlebar. This is layout-only (`menu_bar_layout`, no draw): the
                // bar itself is painted from the `FrameOp::MenuDropdown` arm below.
                // Drawing the controls or the Command Center *here* is pointless because
                // that later `menu_system.render()` repaints `draw_menu_bar` across the
                // entire band and erases them. The actual measurement moved above the walk
                // into the `presence.command_center` block, because the Command Center
                // rung now composes even when this one does not (native-menu backends).
                render::FrameOp::MenuRow => {
                    composed.push(render::FrameOp::MenuRow);
                }

                // ── Sidebar panel body ──────────────────────────────────────────
                // The quadraui AppShell chrome (activity bar, sidebar header,
                // separator) is painted by the runner before `render_content` is
                // entered; this fills only the content area it exposes. The backend holds
                // the editor's VS Code row-pitch override for the rest of this frame —
                // correct for the editor band and overlays, but wrong for sidebar content,
                // which reads the override directly and gets inflated. Bracket just this
                // call: reset to this font's natural metric, paint, then restore the
                // override so every rung after stays agreeing with `lh`.
                render::FrameOp::SidebarPanel => {
                    if let Some(q_sb) = layout.sidebar_content_bounds {
                        backend.set_editor_font(&editor_family, editor_size_pt);
                        let natural_lh = backend.line_height() as f64;
                        self.paint_sidebar_panel_rung(
                            backend, &engine, screen, &theme, q_sb, natural_lh, cw,
                        );
                        if let Some(px) =
                            resolve_editor_line_height_px(&engine.settings, backend, editor_size_pt)
                        {
                            quadraui::Backend::set_current_line_height(backend, px);
                        }
                        composed.push(render::FrameOp::SidebarPanel);
                    }
                }

                // ── Wildmenu bar (command Tab completion) ────────────────────
                render::FrameOp::Wildmenu => {
                    if let Some(ref wm) = screen.wildmenu {
                        // Shares the command-line row whenever there is no separate global bar
                        // row to sit under — per-window status lines are on, or `'laststatus'`
                        // hides the status line entirely.
                        let wm_y = if global_status_visible {
                            status_y + lh
                        } else {
                            status_y
                        };
                        let wm_rect =
                            quadraui::Rect::new(x as f32, wm_y as f32, w as f32, lh as f32);
                        render::paint_wildmenu_rung(backend, wm, &theme, wm_rect);
                        composed.push(render::FrameOp::Wildmenu);
                    }
                }

                // ── Global status bar ────────────────────────────────────────
                // Publish the painted rect for `route_chrome_click`, the
                // twin of TUI's call site. The bespoke branch hit-test this
                // replaces re-derived the band from
                // `height - lh * rows - wildmenu_px` in the click handler — a
                // second copy of the arithmetic, and one that had no way to
                // know what was really drawn.
                render::FrameOp::StatusBar => {
                    if let Some(ref bar) = screen.global_status_bar {
                        let sb_rect =
                            quadraui::Rect::new(x as f32, status_y as f32, w as f32, lh as f32);
                        let sb_layout =
                            render::paint_global_status_bar_rung(backend, &engine, bar, sb_rect);
                        // Same zone recovery as the per-window and separated bars above.
                        *self.global_status_zones.borrow_mut() =
                            render::status_bar_zones_from_layout(&sb_layout);
                        composed.push(render::FrameOp::StatusBar);
                    }
                }

                // ── Command line ─────────────────────────────────────────────
                render::FrameOp::CommandLine => {
                    let cmd_y = status_y + (status_bar_h - lh);
                    let cmd = render::command_line_view(&screen.command);
                    let cmd_rect = quadraui::Rect::new(x as f32, cmd_y as f32, w as f32, lh as f32);
                    // Catch a "row clipped by the window edge"
                    // regression the instant it's introduced, on every
                    // backend, rather than only on a live macOS run.
                    debug_assert_command_line_fits_viewport(cmd_rect, backend.viewport(), lh);
                    // Publish the painted rect for
                    // `render::command_line_click_char_idx` — the exact twin
                    // of `global_status_rect` above, and TUI's identical
                    // cache in `shell_app.rs`'s own `FrameOp::CommandLine`
                    // arm.
                    self.engine.borrow().command_line_rect.set(cmd_rect);
                    // Paint through `draw_command_line_selection`
                    // instead of the selection-blind
                    // `draw_command_line` — `cmd_sel` is character indices
                    // into `screen.command.text`, converted to the byte
                    // offsets the primitive expects.
                    let sel_bytes =
                        self.engine.borrow().cmd_sel.get().map(|sel| {
                            render::command_line_selection_bytes(&screen.command.text, sel)
                        });
                    backend.draw_command_line_selection(cmd_rect, &cmd, sel_bytes);
                    composed.push(render::FrameOp::CommandLine);
                }

                // ── Folder / workspace picker modal ──────────────────────────
                // `quadraui::FolderPickerController::render` paints through
                // the shared `Palette` primitive — the identical method
                // TUI's `FrameOp::FolderPicker` arm calls, just with this
                // backend's own (pixel-unit) popup rect.
                render::FrameOp::FolderPicker => {
                    if let Some(ref picker) = *self.folder_picker.borrow() {
                        let popup_rect =
                            render::folder_picker_popup_rect(popup_viewport, lh as f32);
                        picker.render(popup_rect, backend);
                        // Cache the *painted* rect — key/mouse
                        // handling read this instead of re-deriving it.
                        self.folder_picker_popup_rect.set(Some(popup_rect));
                        composed.push(render::FrameOp::FolderPicker);
                    }
                }

                // ── Menu dropdown overlay ───────────────────────────────────────
                // First rung of the band: `MenuSystem::render` repaints `draw_menu_bar`
                // across the whole title-bar strip, so nothing that wants to survive may
                // be drawn into that band before it. Modal rungs paint above it — a modal
                // dialog now covers an open dropdown on both backends, matching
                // `route_modal_overlay_click`'s own "a dialog eats everything"
                // arbitration. This arm painted the whole title bar into a degenerate
                // rect before the fold.
                render::FrameOp::MenuDropdown => {
                    self.paint_title_bar_band(
                        backend,
                        &engine,
                        &theme,
                        menu_row_rect,
                        menu_items_rect,
                        app_icon_rect,
                        controls_rect,
                    );
                    composed.push(render::FrameOp::MenuDropdown);
                }

                // ── Command Center: nav arrows + search box ────────────────────
                // Painted *after* `menu_system.render()` above, which repaints
                // `draw_menu_bar` across the entire `menu_row_rect` band and would erase
                // anything drawn here first — the identical ordering hazard documented on
                // the window controls. On a native-menu backend (macOS) `presence.
                // menu_row` is `false`, so `FrameOp::MenuDropdown`'s arm — the one that
                // paints the themed background fill — never composes this frame. The
                // Command Center rung below is then the *only* thing painting into this
                // band, and its own rect starts at `menu_end`, leaving that leading
                // strip with no vimcode paint call touching it at all. Fill the *entire*
                // row — inset included — with the theme's title-bar colour first, so the
                // traffic-light strip is themed, matching VS Code's own macOS title bar.
                render::FrameOp::CommandCenter => {
                    let mut row_filled = false;
                    if !presence.menu_row {
                        let filler = quadraui::MenuBar {
                            id: quadraui::WidgetId::new("title_row_background_fill"),
                            items: Vec::new(),
                            open_item: None,
                            focused_item: None,
                        };
                        let _ = backend.draw_menu_bar(menu_row_rect, &filler);
                        row_filled = true;
                    }
                    let mut cc_painted = false;
                    if let Some(cc_rect) = command_center_rect.filter(|r| r.width >= 1.0) {
                        let title = engine
                            .cwd
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "VimCode".to_string());
                        let cc = render::build_command_center_view(
                            engine.tab_nav_can_go_back(),
                            engine.tab_nav_can_go_forward(),
                            &title,
                        );
                        render::paint_command_center_rung(backend, &engine, cc_rect, &cc);
                        cc_painted = true;
                    }
                    // `composed` records exactly what reached the canvas
                    // this frame (this file's own contract elsewhere) —
                    // push it whenever *either* paint call above actually
                    // ran, not only when the Command Center's own rect
                    // painted, so a degenerate/`None` `command_center_rect`
                    // with the row-fill still active isn't silently
                    // dropped from the recorded sequence.
                    if row_filled || cc_painted {
                        composed.push(render::FrameOp::CommandCenter);
                    }
                }

                // ── Find/replace overlay ────────────────────────────────────────
                // Confirmed to open in engine state with nothing painting on GTK.
                // Unlike quickfix/panel_hover there was a dead painter to port —
                // `draw.rs::draw_find_replace_popup` — but it routed through a rect the
                // rasteriser ignores; calling `Backend::draw_find_replace` directly
                // (same trait method TUI calls) is simpler and identical in effect.
                render::FrameOp::FindReplace => {
                    if let Some(ref find_replace) = screen.find_replace {
                        render::paint_find_replace_rung(backend, find_replace, popup_viewport);
                        composed.push(render::FrameOp::FindReplace);
                    }
                }

                // ── Picker / command-palette overlay ────────────────────────────
                // The palette was painted only by the dead legacy `draw_editor` Cairo
                // path, which has zero live callers under ShellApp. So `Ctrl+Shift+P`
                // opened the picker in engine state but nothing ever painted — the
                // "command palette fails to open silently" symptom. Geometry comes from
                // the same generic helpers the legacy path used.
                render::FrameOp::UnifiedPicker => {
                    if let Some(ref picker) = screen.picker {
                        // Scoped to the editor+sidebar band only, never the
                        // full window: `modal_scrim_rect` derives its bottom
                        // edge from *this frame's* `el`, so it stops above
                        // every row already painted by an earlier rung — the
                        // bottom-chrome rungs `FRAME_Z_ORDER` composes first
                        // (`FrameOp::StatusBar`, wildmenu row included,
                        // `FrameOp::CommandLine`, quickfix, terminal) and the
                        // active window's own status row inside the band.
                        // Only `popup_viewport` (the popup's own centring
                        // anchor) stays full-window.
                        let scrim_rect = render::modal_scrim_rect(
                            &engine,
                            &el,
                            popup_viewport.width,
                            y as f32,
                            lh,
                        );
                        let rect = render::paint_picker_rung(
                            backend,
                            picker,
                            popup_viewport,
                            scrim_rect,
                            &(self.units.picker)(lh as f32),
                        );
                        // Hand the *painted* rect to the click/drag handlers.
                        self.picker_popup_rect.set(Some(rect));
                        composed.push(render::FrameOp::UnifiedPicker);
                    }
                }

                // ── Tab switcher popup (Ctrl+Tab MRU list) ──────────────────────
                // `self.tab_switcher_popup_rect` is read by `handle_mouse_press`'s tab
                // switcher modal arbitration block. Sizing/positioning ported from the
                // dead `draw.rs::draw_tab_switcher_popup_list` (pixel-tuned clamp(350,
                // 600) width); content comes from the same shared `render::
                // tab_switcher_to_quadraui_list_view` adapter TUI uses.
                render::FrameOp::TabSwitcher => {
                    if let Some(ref ts) = screen.tab_switcher {
                        // Geometry comes from the shared
                        // `TabSwitcherGeometry` so the rect handed to
                        // `route_modal_overlay_click` below is the rect that was
                        // painted, and TUI resolves the identical popup through
                        // the same code with its own sizing constant.
                        if let Some(geo) = render::TabSwitcherGeometry::compute(
                            popup_viewport,
                            ts.items.len(),
                            &(self.units.tab_switcher)(lh as f32),
                        ) {
                            let list =
                                render::tab_switcher_to_quadraui_list_view(ts, geo.visible_rows);
                            backend.draw_list(geo.bounds, &list);
                            self.tab_switcher_popup_rect.set(Some(geo.bounds));
                            composed.push(render::FrameOp::TabSwitcher);
                        }
                    }
                }

                // ── Context menu ──────────────────────────────────────────────
                // The ShellApp render path never painted `screen.context_menu`
                // at all — its draw + click-geometry cache was populated only by
                // the dead legacy `draw_editor` Cairo path (src/gtk/draw.rs),
                // which has zero live callers under ShellApp, leaving right-click
                // menus invisible and unclickable. Drawn with only generic
                // `Backend` metrics (`render::context_menu_generic_layout`,
                // shared with TUI) since this fn has no raw Pango/Cairo access.
                render::FrameOp::ContextMenu => {
                    if let Some(panel) =
                        screen.context_menu.as_ref().filter(|p| !p.items.is_empty())
                    {
                        // A native popup (`Backend::show_context_menu`)
                        // was already shown from the event handler that opened
                        // it (`App::handle`'s `open_context_menu_now_if_native`
                        // choke point) — never from here. This rung paints
                        // nothing in-window when the backend resolves `Native`
                        // (no layout to cache, no rung to record as painted),
                        // and must not call `show_context_menu` itself: doing
                        // so from inside `render_content` re-enters AppKit's
                        // modal popup loop from a paint closure that still
                        // holds the borrows it needs to finish its own frame.
                        if backend.effective_menu_style() == quadraui::ResolvedMenuStyle::Custom {
                            let layout = render::paint_context_menu_rung(
                                backend,
                                panel,
                                popup_viewport,
                                cw,
                                lh,
                                0.0,
                            );
                            *self.context_menu_layout.borrow_mut() = Some(layout);
                            composed.push(render::FrameOp::ContextMenu);
                        } else {
                            *self.context_menu_layout.borrow_mut() = None;
                        }
                    }
                }

                // ── Change-review surface ───────────────────────────────────────
                // `render::paint_change_review_rung` is the whole body — no GTK-specific
                // diff rendering, matching every other `quadraui::DiffView` consumer.
                // Uses the same `popup_viewport` every other overlay rung anchors to.
                render::FrameOp::ChangeReview => {
                    if let Some(review) = screen.change_review.as_ref() {
                        render::paint_change_review_rung(
                            backend,
                            &engine,
                            review,
                            popup_viewport,
                            &theme,
                        );
                        composed.push(render::FrameOp::ChangeReview);
                    }
                }

                // ── Modal dialog ────────────────────────────────────────────────
                // Same story as the context menu: invisible AND undismissable by mouse
                // under ShellApp — `dialog.is_some()` stayed true forever. Modal rungs
                // paint above it: a dialog is the surface `route_modal_overlay_click`
                // hands every event to, so it must also be the surface the user can see.
                // A natively-expressible `screen.dialog` (no `DialogTable`, no text
                // input) goes through a real OS `AlertDialog` instead, and is therefore
                // *not* live as a frame rung at all. A dialog reaching here carries a
                // `DialogTable` or a text input (e.g. the SSH-passphrase prompt).
                render::FrameOp::Dialog => {
                    if let Some(panel) = screen.dialog.as_ref() {
                        let dlayout =
                            render::paint_dialog_rung(backend, panel, popup_viewport, cw, lh);
                        *self.dialog_layout.borrow_mut() = Some(dlayout);
                        composed.push(render::FrameOp::Dialog);
                    }
                }

                // ── Toast overlay — top of the band ──────────────────────────
                // Anchored to the full window viewport (matches TUI's
                // `layout.window_bounds`), not just `main_content_bounds`, so it
                // sits in the bottom-right corner of the whole app like the
                // TUI/VSCode toasts. `Backend::draw_toast_stack` does its own
                // pango measurement internally (unlike `dialog`/`context_menu`
                // above, whose generic layout is computed vimcode-side), so its
                // returned layout is the only source of truth — cached for
                // `handle_mouse_click_msg`'s hit-test → `handle_toast_hit`
                // dispatch, and the first rung `route_modal_overlay_click`
                // arbitrates.
                render::FrameOp::ToastStack => {
                    if let Some(ref stack) = toast_stack {
                        render::paint_toast_stack_rung(backend, &engine, stack, popup_viewport);
                        composed.push(render::FrameOp::ToastStack);
                    }
                }
            }
        }

        *self.composed_frame.borrow_mut() = composed;
        // Read back through the field rather than the local, so the *stored*
        // observable is what gets validated — a frame that recorded one thing
        // and composed another would be a lie the tests then trusted.
        if let Err(why) = render::check_frame_order(&self.composed_frame.borrow()) {
            debug_assert!(false, "GTK {why}");
        }
    }

    fn handle(
        &mut self,
        event: quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) -> quadraui::Reaction {
        let had_context_menu_before = self.engine.borrow().context_menu.is_some();
        let reaction = self.handle_dispatch(event, backend, ctx);
        // Open the native context-menu popup exactly once, from
        // this event-handler choke point — never from `render_content`'s
        // paint rung. Gated on the open *transition* (`None` -> `Some`),
        // not just `is_some()`, so a native menu that's still open (the
        // real runner blocks on AppKit's own modal loop for the whole
        // `show_context_menu` call, so this can't actually re-enter, but a
        // headless test driver that calls `handle` again while nothing
        // closed the menu must not re-show it either).
        if !had_context_menu_before {
            self.open_context_menu_now_if_native(backend);
        }
        // Keep the runner's `AppShell` title-bar reservation in sync with
        // `engine.menu_bar_visible` — that flag can flip from several places
        // inside `handle_dispatch`, each with its own early `return`, so this
        // single choke point is what makes the sync unconditional. Gated on
        // `menu_bar_toggleable`, not unconditional: on GTK/macOS/Win the title-
        // bar *band* is a permanent construction-time reservation that survives
        // `menu_bar_visible` going `false` — only the *drawn* row/dropdown
        // inside it are coupled to that flag. Calling `set_title_bar_visible`
        // unconditionally would collapse that reservation, which is wrong for
        // every profile except the toggleable one.
        if self.engine.borrow().menu_bar_toggleable {
            render::sync_menu_bar_title_row(ctx, &self.engine.borrow());
        }
        // Keep the runner's `AppShell` sidebar *visibility* in sync with the
        // shadow's too, unconditionally on the toggleable-menu-bar profile
        // (`menu_bar_toggleable`), where it corrects `AppShell::new`'s
        // "active + visible" construction-time default away from the hamburger
        // *before* the very first hamburger click, so that click resolves as a
        // reveal rather than a toggle-hide. GTK/macOS/Win's existing call sites
        // already keep convergence on every path that needs it; this stays
        // scoped to where the profile actually requires it.
        if self.engine.borrow().menu_bar_toggleable {
            self.sync_runner_sidebar_visibility(ctx);
        }
        if self.exit_requested.get() {
            return quadraui::Reaction::Exit;
        }
        reaction
    }

    fn tick(&mut self, backend: &mut dyn quadraui::Backend) -> quadraui::Reaction {
        let reaction = self.tick_dispatch(backend);
        if self.exit_requested.get() {
            return quadraui::Reaction::Exit;
        }
        reaction
    }

    /// The app-initiated half of the runner ↔ shadow panel sync. Was
    /// unoverridden on `App` (the trait default always returns `None`), so
    /// `ShellAdapter::apply_requested_panel` never saw a switch to apply,
    /// no matter what the engine did to its own `app_shell`/`ext_panel_active`.
    /// `App::render_content` paints the sidebar's *content* by reading
    /// `engine.app_shell`/`engine.ext_panel_active` directly (the shadow),
    /// so a panel switch the engine makes on its own always painted the
    /// *right* content. But the runner's own chrome (the activity-bar
    /// highlight, and the sidebar-header title `quadraui::AppShell::render`
    /// paints) has no other channel to learn about the change — it only ever
    /// moves in response to `AppShell::handle`'s own click hit-testing or
    /// this poll. This override mirrors the pre-#1434 TUI shell's
    /// `take_requested_panel` with the same mechanics.
    fn take_requested_panel(&mut self) -> Option<quadraui::WidgetId> {
        let engine = self.engine.borrow();
        if !engine.app_shell.sidebar_visible() {
            return None;
        }
        // An extension panel takes over the sidebar body *without*
        // touching the shadow `app_shell`'s active-panel id (`switch_panel`'s
        // `render::apply_activity_panel_switch` call leaves it alone for an
        // `ext:` id), so `engine.ext_panel_active` — not `active_panel_id()`
        // — is what the runner has to follow while one is open.
        if let Some(name) = engine.ext_panel_active.as_deref() {
            let id = quadraui::WidgetId::new(crate::core::engine::sidebar::ext_panel_id(name));
            if self.last_shell_panel.as_ref() == Some(&id) {
                return None;
            }
            self.suppress_shell_panel_echo = true;
            return Some(id);
        }
        let current = engine.app_shell.active_panel_id()?.clone();
        if self.last_shell_panel.as_ref() == Some(&current) {
            return None;
        }
        self.suppress_shell_panel_echo = true;
        Some(current)
    }

    /// Ctx-aware override: implements the context-aware shell event handling
    /// that the pre-existing ctx-less [`Self::on_shell_event`] cannot provide.
    /// Some `AppShellEvent` arms (like `BottomItemClicked`) make decisions
    /// entirely within `engine.app_shell` that must be reflected back into
    /// the runner's own `AppShell` on the same frame, so the runner knows
    /// what to render. `BottomItemClicked` is one such case: the runner never
    /// toggles a bottom item itself, so the toggle-to-hide decision is 100%
    /// made inside `on_shell_event`. Without this, `engine.app_shell.sidebar_visible()`
    /// correctly flips, but the runner's `AppShell` — which determines whether
    /// `render_content`'s sidebar column exists — never learns.
    /// Push the shadow's new state through [`Self::sync_runner_sidebar_visibility`]
    /// so the runner stays in sync.
    fn on_shell_event_ctx(
        &mut self,
        event: &quadraui::AppShellEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        // Special-case `SidebarHidden` for the hamburger panel: only `ctx`
        // (the runner's own `AppShell`) can tell it from a real panel's second
        // click, because the shadow `engine.app_shell` has no hamburger
        // `PanelDefinition` at all (see `render::route_hamburger_panel_changed`'s
        // doc). This check is routed through `render::route_hamburger_sidebar_hidden`.
        // A no-op check on GTK/macOS/Win, which never register this panel id.
        if matches!(event, quadraui::AppShellEvent::SidebarHidden)
            && ctx
                .shell()
                .active_panel_id()
                .map(quadraui::WidgetId::as_str)
                == Some(crate::core::engine::sidebar::HAMBURGER_PANEL_ID)
        {
            render::route_hamburger_sidebar_hidden(&mut self.engine.borrow_mut(), ctx);
            return;
        }
        self.dispatch_shell_event(event);
        // A bottom item's click never moves the runner's real `AppShell` on its own.
        // The `BottomItemClicked` arm only toggles the engine-side shadow via
        // `switch_panel`. Call `show_panel` or `hide_sidebar` on the runner's
        // `AppShell` to keep it in sync with the shadow, so `render` knows what to title.
        if let quadraui::AppShellEvent::BottomItemClicked { id } = event {
            if self.engine.borrow().app_shell.sidebar_visible() {
                ctx.shell_mut().show_panel(id);
            } else {
                ctx.shell_mut().hide_sidebar();
            }
        }
        self.sync_runner_sidebar_visibility(ctx);
        // `ShellAdapter::handle` consumes `PanelChanged`/`SidebarHidden` for
        // top-row panels and never calls `Self::handle`. This call ensures
        // `render::sync_menu_bar_title_row` runs on every path, so any future
        // arm that flips `menu_bar_visible` gets the same-frame guarantee.
        // Gated on `menu_bar_toggleable` for the same reason as `Self::handle`'s
        // own call — see that call site's doc.
        if self.engine.borrow().menu_bar_toggleable {
            render::sync_menu_bar_title_row(ctx, &self.engine.borrow());
        }
    }
}
