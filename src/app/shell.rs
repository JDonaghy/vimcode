use super::dispatch::debug_assert_command_line_fits_viewport;
use super::*;

impl quadraui::ShellApp for App {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // #1428: read the live kitty-keyboard-protocol capability once, at
        // setup time — mirrors the pre-#1434 TUI shell's `setup`'s identical read
        // (`self.keyboard_enhanced = backend.backend_caps().kitty_keyboard`)
        // verbatim, except unconditional rather than gated behind TUI's own
        // `self.live` (that gate exists solely because a *second*,
        // TUI-only, direct crossterm round-trip used to live at this call
        // site before #1109 — see the pre-#1434 TUI shell's `setup`'s own doc; reading
        // `backend_caps()` itself is a plain field access on every backend,
        // never I/O, so there is nothing here for a "don't do this under a
        // test harness" gate to protect against). A GUI backend's
        // `BackendCaps::kitty_keyboard` is always `false` (GDK/AppKit/Win32
        // hand over an already-resolved keysym per physical key, never the
        // terminal-only ambiguity this flag exists to disambiguate — see
        // `render::engine_key_from_ui`'s own doc), so this is a no-op there;
        // TUI's own capability is unaffected by living on the shared `App`
        // instead of the pre-#1434 TUI shell — `backend.backend_caps()` reads the same
        // `TuiBackend` state either way.
        self.keyboard_enhanced = backend.backend_caps().kitty_keyboard;
        // #1864: `shell_config`'s `with_editor_font` seed only covers
        // family/size — the runner's own `set_editor_font` call (made from
        // that stored value, just before `setup()` runs) resets
        // `current_line_height` to this backend's *natural* font-metric
        // value every time, same as every other `set_editor_font` call
        // (`set_current_font`'s doc). Without re-applying the VS Code
        // override here too, frame 1's shell-level layout (sidebar width,
        // computed by the runner *before* `render_content`'s own per-frame
        // override ever runs) would see the natural line height while
        // frame 2+ (after `sync_per_frame_backend_state` has run once) sees
        // the override — a one-frame geometry snap that (#967-style) throws
        // off any hit-test computed against frame 1's painted rects. Same
        // call `sync_per_frame_backend_state` makes every frame; see
        // `resolve_editor_line_height_px`'s doc for the macOS-only scoping.
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
        // (#547) Seed the backend's nerd-fonts flag. The only prior call
        // site was the `Msg::CacheFontMetrics` arm, which stopped firing after
        // the #540 ShellApp migration, silently freezing quadraui's GTK backend
        // at its default of `false` — the cause of the explorer treeview
        // falling back to ASCII icons. (That arm had still never regained a
        // producer, so #732 deleted it; this call is the live replacement.)
        render::sync_nerd_fonts(backend, &self.engine.borrow());
        // (#937) Register the bundled Nerd Font subset and point the
        // backend's fallback cascade at it — required for glyphs to resolve
        // at all on Core Text/DirectWrite backends (macOS/Win-GUI); see
        // `render::register_nerd_font_fallback`'s doc for why this is a
        // one-time `setup()` call, not part of the per-frame sync above.
        render::register_nerd_font_fallback(backend);

        // Try to drop the server-side WM titlebar now, in favour of the
        // drawn CSD row; `setup()` runs before `run_with_shell`'s runner
        // calls `window.present()`, so `backend.window()` is very likely
        // still `None` here. `tick()` retries every frame until the window
        // is mapped, which is the reliable path (#552).
        self.capture_window_and_apply_csd(backend);

        // Same "very likely still None here, tick() retries" story as the
        // CSD drop above, for restoring the saved window size/position/
        // maximized state instead (#1529).
        self.restore_window_geometry(backend);

        // GTK draws its own VSCode-style menu bar (File/Edit/View/...) — it
        // acts as the client-side titlebar, always visible (unlike TUI, which
        // only shows it in vscode-mode or via Alt). Historical GTK behaviour
        // pre-#540; menu defs were never re-populated after the ShellApp
        // migration deleted the Relm4 headerbar wiring. (#552)
        //
        // #901: a backend that declares `BackendCaps::native_menu` (macOS's
        // `MacBackend`) has a real OS menu bar — installing the *drawn* row
        // on top of it would paint a redundant in-window menu underneath the
        // system one (the bug this issue exists to fix). Same `MenuDef`s
        // either way — `render::menu_defs_to_menu_bar` just reshapes them —
        // so the two paths can never disagree about what's in the menu.
        let is_vscode_mode = self.engine.borrow().is_vscode_mode();
        let menu_defs = render::build_menu_defs(is_vscode_mode);
        if backend.backend_caps().native_menu {
            let bar = render::menu_defs_to_menu_bar(&menu_defs);
            // `install_menu_bar`'s macOS implementation (`MacBackend`)
            // asserts it is called on the real AppKit main thread and
            // panics otherwise — a documented quadraui limitation with no
            // portable pre-check exposed through the `Backend` trait.
            // Every real invocation of `ShellApp::setup` *is* on the main
            // thread (`quadraui::macos::shell_runner`'s only entry point),
            // so this never fires outside a test harness — but
            // `quadraui::macos::testing::driver_with_shell` (used by
            // `src/macos/mod.rs::mac_driver_tests`) necessarily calls
            // `setup` from a spawned test thread, same as every other
            // `#[test]` fn, per Rust's own test runner. Catching it here
            // keeps `setup()` — which every backend, including the ones
            // with no native menu, must be able to complete without
            // aborting the process — from taking the whole test process
            // down over a call this method doesn't otherwise depend on.
            // Filed upstream: `install_menu_bar` should degrade
            // gracefully off-main-thread the way its own sibling test
            // helpers already do (`menu_bar_install.rs`'s `let Some(mtm)
            // = MainThreadMarker::new() else { return }`), not hard
            // `.expect()`.
            //
            // #1618 found `WinBackend` (quadraui#1200) briefly also
            // declared `native_menu: true` and implemented
            // `install_menu_bar` (a real Win32 `HMENU` via `SetMenu`) —
            // quadraui#1228 (#1629) reverted that: `WinBackend` no longer
            // declares `native_menu`, `install_menu_bar` is back to the
            // trait's no-op default on that backend, and Windows now
            // takes the `window_chrome` arm below instead (a drawn menu
            // row, matching GTK). `MacBackend` remains the only in-tree
            // `native_menu` implementation this `catch_unwind` guards
            // against, so its panic-recovery rationale stays
            // macOS-specific again.
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
            // client-side titlebar — pinned visible always, same as before
            // #1427 (this `else` used to be the only other arm).
            self.engine.borrow_mut().menu_bar_visible = true;
        } else {
            // #1427: no OS menu bar to hide behind (`native_menu`) and no
            // window chrome for the drawn row to double as (`window_chrome`)
            // — the `cell` profile (TUI-via-`App`) today. The bar is fully
            // hideable at runtime (mirrors the pre-#1434 TUI shell's `setup`,
            // `event_loop`'s `mod.rs:797`) and starts however
            // `Engine::new` already resolved `menu_bar_visible` (`true`
            // only in vscode-mode) — left untouched here, unlike the two
            // arms above, which both *force* a value regardless of what the
            // engine was constructed with.
            self.engine.borrow_mut().menu_bar_toggleable = true;
        }
        self.engine
            .borrow()
            .menu_system
            .borrow_mut()
            .set_menus(menu_defs);

        // #1498: initial CSS used to be applied here by hand (a GTK-only
        // `css_provider` theming the native file dialog). That provider is
        // gone — `GtkPlatformServices::set_theme` (JDonaghy/quadraui#1091)
        // now owns the equivalent stylesheet, and `render_content`'s first
        // frame already calls `sync_per_frame_backend_state`, which calls
        // `Backend::set_theme` unconditionally before anything paints — so
        // there is nothing left for `setup` to do here.

        // Register the panel-keys accelerator set (toggle sidebar, fuzzy
        // finder, live grep, command palette, ...) on the runner's backend.
        // This was previously only wired for TUI (`tui_main::run` calls it
        // right after `TuiBackend::new()`); the GTK side's registration
        // function existed but was never called after the ShellApp
        // migration, so none of these 14 global shortcuts — including
        // Ctrl+Shift+P for the command palette — ever fired on GTK (#587).
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

        // #1864 review round 1: resolved again here (cheap — `resolve_editor_font`
        // is a pure lookup over `settings`/`backend.default_fonts()`), so the
        // `FrameOp::SidebarPanel` arm below can briefly reset `backend` to
        // this font's *natural* line height for sidebar content, then
        // restore the editor's row-pitch override afterward — see that
        // arm's own comment.
        let (editor_family, editor_size_pt) = resolve_editor_font(&engine.settings, backend);
        let lh = self.cached_line_height.max(backend.line_height() as f64);
        let cw = self.cached_char_width.max(backend.char_width() as f64);
        // Publish the value this frame paints with so click-time hit-tests can
        // use it (#555). `render_content` takes `&self`, so it cannot write
        // the plain `cached_line_height` field — which is seeded once in
        // `setup()` from the runner's *default* metrics and can therefore be
        // smaller than the `lh` every frame actually paints with. Hit-testing
        // painted geometry against the smaller value put row boundaries in the
        // wrong place (the picker resolved clicks two rows off) and clipped
        // the bottom of every single-row band, breadcrumbs included.
        self.painted_line_height.set(Some(lh));
        self.painted_char_width.set(Some(cw));

        // #1427: correct this frame's layout for the hamburger's own
        // phantom sidebar reservation — a no-op unless
        // `engine.menu_bar_toggleable` (the `cell` profile; always `false`
        // on GTK/macOS/Win). See `render::reclaim_hamburger_sidebar_
        // reservation`'s own doc; mirrors pre-#1427
        // the pre-#1434 TUI shell's `render_content`'s identical rebind.
        let corrected_layout = render::reclaim_hamburger_sidebar_reservation(&engine, layout);
        let layout = &corrected_layout;

        let main = layout.main_content_bounds;
        // #1877: `h` is the *recovered* content height, not
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

        // #1877 review round 1 (blocking finding 1): `shell_config`'s
        // bottom-chrome reservation above vacates a strip left of
        // `main_content_bounds` — below the now-shrunk
        // `activity_bar_bounds`/`sidebar_header_bounds`/
        // `sidebar_content_bounds`/`divider_bounds` — that nothing else
        // paints (quadraui's own `AppShell::render` only fills those four
        // rects *as shrunk*; vimcode's own status-bar/command-line rows
        // below are anchored at `main_content_bounds.x`, never reaching
        // left of them either). Fill it explicitly with the same chrome
        // colour the activity bar itself paints, every frame, on every
        // backend — see `render::bottom_chrome_reservation_fill_rect`'s
        // own doc for exactly which rect and why `theme.tab_bar_bg`. A
        // no-op (`None`) once there is nothing left to reclaim — no
        // reservation active, or a window too short for either dimension
        // to be positive.
        if let Some(fill_rect) = render::bottom_chrome_reservation_fill_rect(layout) {
            backend.draw_solid_fill(fill_rect, theme.tab_bar_bg);
        }

        // ── Layout ────────────────────────────────────────────────────────────
        let tab_row_h = (self.units.tab_row_h)(lh);
        let tab_bar_h = (self.units.tab_bar_h)(lh, engine.settings.breadcrumbs);
        // Whether the *global* (non-per-window) status bar occupies its own
        // row — `global_status_bar_visible`, not `effective_window_status_line`
        // directly, since `'laststatus'` can hide the status line entirely
        // (0, or 1 with a single window) even while per-window status is off,
        // in which case there is still no separate global row to offset the
        // wildmenu past (#1235 follow-up).
        let global_status_visible = render::global_status_bar_visible(&engine);
        let el = render::compute_editor_layout(&engine, h, lh, false);
        // `el.status_bar_h` is `compute_editor_layout`'s single source of
        // truth for this (identical formula to the `wildmenu_px`/
        // `status_rows` locals this replaced); reusing it here — instead of
        // recomputing a second copy — is what makes `editor_area_h` below
        // `el.editor_bottom` correctly reserve quickfix's band too.
        let status_bar_h = el.status_bar_h;
        // `el.editor_bottom` already subtracts quickfix_h/terminal_h/
        // debug_toolbar_h/separated_status_h/status_bar_h from `h` (menu_h
        // is 0 for GTK — the menu bar lives outside `main_content_bounds`,
        // see `compute_editor_layout`'s `menu_in_viewport` doc). Before
        // #670 this was hand-rolled here without the `quickfix_h` term, so
        // an open quickfix panel never reserved space and editor content
        // painted straight through where the panel now paints.
        let editor_area_h = el.editor_bottom.max(0.0);

        let editor_bounds = WindowRect::new(x, y, w, editor_area_h);
        // Hand the exact bounds/tab-bar-height this frame painted with to the
        // click + drag handlers, so divider hit-tests land on the painted line
        // instead of on a second, differently-originated guess (#582).
        self.cached_editor_bounds
            .set(Some((editor_bounds, tab_bar_h)));
        // #1421: the raw `h` this frame's `compute_editor_layout` call above
        // was given — see `cached_main_content_height`'s own doc for why
        // this is `h`, not `editor_area_h`/`editor_bounds`'s height.
        self.cached_main_content_height.set(h);
        let (window_rects, _dividers) =
            engine.calculate_group_window_rects(editor_bounds, tab_bar_h);

        // #700: the breadcrumb row's own painted bounds must agree with the
        // fixed-pixel space `tab_bar_h` (above) already reserved for it above
        // the window content — plain `build_screen_layout` would assume the
        // breadcrumb row is exactly one `lh`-tall editor text line, which is
        // no longer true now that the row is a fixed 22px regardless of
        // `settings.font_size`.
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

        // #560 / #947 / #1104: mouse clicks resolve editor columns via the
        // per-glyph Pango inverse (`Backend::editor_col_at_x`), not a naive
        // uniform-cell division — see `pixel_to_click_target`'s call site for
        // the emoji/CJK drift #560 reported.
        //
        // Before #1104, click-time hit-testing ran against `self.backend`, a
        // SECOND `GtkBackend` vimcode constructed purely to mimic the real
        // one's font (`build_editor_click_context` + a `set_editor_font` echo
        // right here, every frame) — because the mouse-click handlers
        // (`handle_mouse_click_msg` and friends) never received the runner's
        // own live backend at all. #1104 threaded that live `backend`
        // (the exact instance `quadraui::gtk::run` paints every frame, already
        // carrying the stable `pango_ctx` its own `activate()` sets at widget
        // realize, and already up to date on `editor_font_*` via
        // `sync_per_frame_backend_state`'s `set_editor_font` call above)
        // through the whole click/drag/modal-stack call chain instead, so
        // there is no second backend left to keep in sync here — this frame's
        // `backend` *is* the click backend now.
        //
        // `self.backend` still exists for the handful of callers this refactor
        // could not reach: `explorer_ui_event`, `route_ai_sidebar_event` and
        // the DAP-sidebar key route all call
        // `quadraui::Backend::set_current_line_height`/`set_current_char_width`
        // (JDonaghy/quadraui#1086) to undo click drift, from deep inside the
        // keyboard/mouse dispatch tree where only `&mut self` (no live
        // `backend` reference) is available.

        // ══ Editor band (#764, #735 slice 3) ═════════════════════════════════
        // Composed from `render::compose_editor_band`, then the `FrameHitMap`
        // is recovered from the very objects that walk painted — see
        // `compose_editor_band_rungs`.
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

        // ══ Bottom band (#765, #735 slice 4) ═════════════════════════════════
        // Composed from `render::compose_bottom_band` — see
        // `compose_bottom_band_rungs`.
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

        // ══ Frame sequence (#766, #735 slice 6) ══════════════════════════════
        //
        // Composed from `render::compose_frame` — the single ordered artefact
        // both backends walk for everything around and on top of the editor
        // column. Slices 1-4 landed this as *four* ladders (editor, bottom,
        // chrome, overlay); slice 6 folds the chrome and overlay halves into
        // one `FrameOp` sequence, so the frame is no longer "a composer plus a
        // special-cased top band" a backend could get individually right and
        // jointly wrong. Geometry and rasterisation stay here, in pixels; only
        // the *order* and the *gates* are shared. `FRAME_Z_ORDER`'s doc comment
        // records which rungs changed position and the divergences that closed.
        //
        // Caches whose "absent" branch used to live in an `else` are cleared
        // here, before the walk: `compose_frame` returns only the live rungs,
        // so an absent rung has no arm left to run. Every arm gates itself on
        // the *value* it needs and, when it composes, records the rung by name
        // (`push(FrameOp::Dialog)`, never `push(op)`) — with `push(op)` the
        // record would follow the pattern the walk is at, so swapping two arms'
        // bodies would compose them in the wrong order while still recording
        // the right one.
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
        // a stale value (the #695 paint/hit-test disagreement in miniature).
        // The `MenuRow` arm below overwrites them when the rung is live.
        let menu_row_rect = layout.title_bar_bounds.unwrap_or_default();
        self.menu_row_rect.set(menu_row_rect);
        self.menu_items_rect.set(menu_row_rect);
        self.title_bar_rect.set(quadraui::Rect::default());
        let mut app_icon_rect = quadraui::Rect::default();
        let mut menu_items_rect = menu_row_rect;
        let mut controls_rect: Option<quadraui::Rect> = None;
        let mut command_center_rect: Option<quadraui::Rect> = None;

        // The overlay tail's caches are cleared here for the same reason
        // (#766): the tail is part of this one walk now, so a rung whose gate
        // is off has no arm left to run and cannot clear its own cache from an
        // `else`. A stale `dialog_layout` / `context_menu_layout` /
        // `picker_popup_rect` / `tab_switcher_popup_rect` resolves the next
        // click against last frame's geometry — the #587 class of bug.
        self.engine.borrow().command_center_layout.replace(None);
        self.picker_popup_rect.set(None);
        self.folder_picker_popup_rect.set(None);
        self.tab_switcher_popup_rect.set(None);
        *self.context_menu_layout.borrow_mut() = None;
        *self.dialog_layout.borrow_mut() = None;
        self.engine.borrow().toast_layout.replace(None);
        // #727's native-dialog edge trigger, hoisted out of the `Dialog` arm
        // (#766): a *native* dialog is not a frame rung, so the arm no longer
        // runs for it. A native dialog must be presented exactly once per open
        // — `native_dialog_shown` is the edge: the first `render_content` call
        // to see a given open queues the present (via `pending_native_dialog`,
        // drained by `tick()` since the blocking `PlatformServices` call can't
        // run from inside this paint callback, mirroring `PendingFileDialog`
        // #572) and flips the flag; a *closed* dialog re-arms it.
        //
        // #1432: `quadraui::native_dialog_options` is a pure content-shape
        // check (no table/no input) — it says nothing about whether *this*
        // backend actually has a native alert facility to show it with. Its
        // own doc says as much: "callers should consult [`BackendCaps::
        // native_dialogs`] before calling [`Backend::show_message_dialog`],
        // and fall back to `draw_dialog` when it returns `None`." Gating here
        // (rather than only inside `Some(opts) =>`) also keeps `presence.dialog`
        // below correct — an ungated `native_dialog_shown` would still have
        // suppressed the in-canvas rung on a backend with no native dialog at
        // all (TUI), leaving the dialog painted nowhere.
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
        // #815: shared with TUI now — see `render::FrameOp::FolderPicker`.
        presence.folder_picker = self.folder_picker.borrow().is_some();
        // #727: a natively-expressible dialog is presented by the OS, not
        // composed into this frame, so the rung is not live. `dialog_layout`
        // stays cleared above and nothing is recorded — the sequence describes
        // what reached the canvas.
        presence.dialog = screen.dialog.is_some() && !self.native_dialog_shown.get();

        // #939: measure the title-bar band whenever the Command Center rung
        // is live, independent of whether `FrameOp::MenuRow` itself composes.
        // Before this, `controls_rect`/`command_center_rect` were populated
        // *only* inside the `MenuRow` match arm below, which is gated on
        // `presence.menu_row` — i.e. on `menu_bar_visible`. A native-menu
        // backend (macOS's `MacBackend`) sets `menu_bar_visible = false` to
        // suppress the redundant in-window row under AppKit's real menu bar
        // (#901), which left `command_center_rect` permanently `None` and
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
            // #940: a client-side-titlebar-capable backend (macOS's
            // `MacBackend`, which honours `ShellConfig::client_side_titlebar`
            // as of quadraui#947 — requested unconditionally in
            // `shell_config`) reports how much of the band's leading edge its
            // own native controls already occupy. `Rect::default()` — the
            // trait default, and the only value GTK/Win-GUI/TUI ever return —
            // means "nothing of the backend's own is in this band", so
            // `render::backend_draws_own_window_controls` is `false` and
            // `leading_inset` is `0.0` there: every line below is then a
            // no-op and this arm behaves exactly as it did before #940.
            let control_inset = backend.titlebar_control_inset();
            let leading_inset = control_inset.width.max(0.0);
            let inset_menu_row_rect =
                render::inset_titlebar_row_leading_edge(menu_row_rect, leading_inset);

            let (real_icon_rect, real_items_rect) =
                render::split_menu_row_for_app_icon(menu_row_rect, leading_inset);
            let (items_for_measure, bar_for_measure) = if presence.menu_row {
                // `app_icon_rect` is only ever assigned here, so on macOS
                // (where `presence.menu_row` is always `false` — #901, the
                // AppKit system menu bar owns the drawn row) it never picks
                // up a real value. That is fine today: its only reader
                // (`FrameOp::MenuDropdown`'s `paint_title_bar_band`) is
                // itself gated on `presence.menu_dropdown`, which stays
                // coupled to `menu_bar_visible` and so is also always
                // `false` on macOS — the app icon genuinely does not paint
                // via this path there yet (pre-existing from #939/#901, not
                // a #940 regression; the omnibar is the only thing #940
                // actually offsets clear of the native controls).
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

            // A backend that draws its own controls must never *also* get
            // vimcode's drawn `controls_bar` — two sets of window controls is
            // exactly the bug #940 exists to prevent (see the module doc's
            // "keeps the native traffic lights" section). `presence.menu_row`
            // still gates it the same way it always did on every other
            // backend. Pure decision extracted to
            // `render::should_draw_window_controls` so it has a unit test
            // independent of any backend/driver (see that function's tests).
            let draw_controls =
                render::should_draw_window_controls(presence.menu_row, control_inset);
            // #1234: see `paint_title_bar_band`'s identical read for why
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

        // #955 review fix, the #1117 class: `presence.change_review` is the
        // exact gate `compose_frame` uses to decide whether
        // `FrameOp::ChangeReview` is even in this frame's op list, so that
        // rung's arm below can never run on the frame the surface *closes* —
        // an `else` inside it is dead code, and the full-viewport modal-stack
        // entry `paint_change_review_rung` pushes would stay registered
        // forever, routing every later click past `AppShell`'s chrome dispatch
        // (`ShellAdapter::handle` hit-tests the modal stack first). Reconciled
        // here, once, unconditionally, ungated by which rungs compose —
        // exactly how `reconcile_editor_hover_modal` is called.
        render::reconcile_change_review_modal_stack(
            backend,
            presence.change_review,
            popup_viewport,
        );

        let mut composed: Vec<render::FrameOp> = Vec::new();
        for op in render::compose_frame(&presence) {
            match op {
                // ── Menu bar row (client-side chrome; #552): measure only ────
                // quadraui's `run_with_shell` GTK runner (single-DA
                // architecture, #217) creates the window undecorated with no
                // native titlebar/menu hosting. `ShellConfig::with_title_bar()`
                // (set in `run()`) reserves a full-width band across the top of
                // the *entire* shell — above the activity bar and sidebar too,
                // not just `main_content_bounds` — so GTK's drawn menu bar +
                // inline window controls span the whole window like a real
                // titlebar, and the activity bar/sidebar/main content the runner
                // hands us are already shifted down to make room. Mirrors the
                // pre-#540 Relm4 headerbar and TUI's identical row via the same
                // shared `engine.menu_system` / `Backend::draw_menu_bar`.
                //
                // This is layout-only (`menu_bar_layout`, no draw): the bar
                // itself — and the whole `menu_row_rect` band — is painted from
                // the `FrameOp::MenuDropdown` arm below.
                // Drawing the controls or the Command Center *here* (as this
                // used to) is pointless because that later `menu_system.render()`
                // repaints `draw_menu_bar` across the entire band and erases
                // them (#552 round-2/3 "buttons render blank"), so this rung
                // only stashes their target rects.
                //
                // #939: the actual measurement — the #720 app-icon split,
                // `menu_items_rect`, `controls_rect`, `command_center_rect` —
                // moved above the walk, into the `presence.command_center`
                // block, because the Command Center rung now composes even
                // when this one does not (native-menu backends). `menu_row`
                // implies `command_center` (both require the band to exist;
                // `menu_row` additionally requires `menu_bar_visible`), so
                // that block has already run with the *real* menu bar by the
                // time this arm is reached — there is nothing left to do here
                // but record that the rung composed.
                render::FrameOp::MenuRow => {
                    composed.push(render::FrameOp::MenuRow);
                }

                // ── Sidebar panel body ───────────────────────────────────────
                // The quadraui AppShell chrome (activity bar, sidebar header,
                // separator) is painted by the runner before `render_content`
                // is entered; this fills only the content area it exposes.
                //
                // #1864 review round 1: `backend.current_line_height` holds
                // the editor's VS Code row-pitch override for the rest of
                // this frame (`sync_per_frame_backend_state` applied it at
                // the top, every frame) — correct for the editor band,
                // popups, dialogs and pickers above/below this arm, which
                // all paint and hit-test in those same `lh` units, but wrong
                // for sidebar content: `tree_layout`/`list_layout`/
                // `form_layout`/`msv_layout` (the file explorer, search,
                // Source Control, extensions, settings and debug panels, all
                // routed through `paint_sidebar_panel_rung` below) read that
                // field directly, so the override inflated every one of them
                // by the same multiplier — untested, unmeasured sidebar
                // growth. Bracket just this one call: reset to this font's
                // natural metric (`set_editor_font`'s own doc — it always
                // re-derives `current_line_height` from the font, the same
                // mechanism `sync_per_frame_backend_state` itself uses to
                // seed the override), paint, then restore the override so
                // every rung after this one keeps agreeing with `lh`/
                // `painted_line_height()`.
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
                        // Shares the command-line row whenever there is no
                        // separate global bar row to sit under — per-window
                        // status lines are on, or `'laststatus'` hides the
                        // status line entirely (#1235 follow-up).
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
                // #752: publish the painted rect for `route_chrome_click`, the
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
                    // #1549: catch a "row clipped by the window edge"
                    // regression the instant it's introduced, on every
                    // backend, rather than only on a live macOS run.
                    debug_assert_command_line_fits_viewport(cmd_rect, backend.viewport(), lh);
                    // #816: publish the painted rect for
                    // `render::command_line_click_char_idx` — the exact twin
                    // of `global_status_rect` above, and TUI's identical
                    // cache in `shell_app.rs`'s own `FrameOp::CommandLine`
                    // arm.
                    self.engine.borrow().command_line_rect.set(cmd_rect);
                    // #1185: paint through `draw_command_line_selection`
                    // (quadraui#1001) instead of the selection-blind
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

                // ── Folder / workspace picker modal (#815) ───────────────────
                // `quadraui::FolderPickerController::render` paints through
                // the shared `Palette` primitive — the identical method
                // TUI's `FrameOp::FolderPicker` arm calls, just with this
                // backend's own (pixel-unit) popup rect.
                render::FrameOp::FolderPicker => {
                    if let Some(ref picker) = *self.folder_picker.borrow() {
                        let popup_rect =
                            render::folder_picker_popup_rect(popup_viewport, lh as f32);
                        picker.render(popup_rect, backend);
                        // Cache the *painted* rect (#582/#646) — key/mouse
                        // handling read this instead of re-deriving it.
                        self.folder_picker_popup_rect.set(Some(popup_rect));
                        composed.push(render::FrameOp::FolderPicker);
                    }
                }

                // ── Menu dropdown overlay ────────────────────────────────────
                // First rung of the band: `MenuSystem::render` repaints
                // `draw_menu_bar` across the whole title-bar strip, so nothing
                // that wants to survive may be drawn into that band before it.
                // #735 moved the *modal* rungs above it (they used to paint
                // underneath on GTK and on top on TUI) — a modal dialog now
                // covers an open dropdown on both backends, matching
                // `route_modal_overlay_click`'s own "a dialog eats everything"
                // arbitration.
                // #766: the `engine.menu_bar_visible` check that used to open
                // this arm is `FramePresence::from_screen`'s now — and stricter,
                // because it also requires the shell to have reserved a band at
                // least one text line tall. This arm painted the whole title bar
                // into a degenerate rect before the fold.
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

                // ── Command Center: nav arrows + search box (#676) ────────────
                // Painted *after* `menu_system.render()` above, which repaints
                // `draw_menu_bar` across the entire `menu_row_rect` band and
                // would erase anything drawn here first — the identical
                // ordering hazard documented on the window controls (#552
                // round-2/3 "buttons render blank"). This is the VS Code-style
                // Command Center dropped by the #540 Relm4→ShellApp cutover and
                // never re-wired: it used to live in the deleted `impl
                // SimpleComponent for App` `view!` scaffolding. Cached into
                // `engine.command_center_layout` for `handle()`'s click
                // hit-test, mirroring TUI's `shell_app.rs` (#635 Stage 6b item
                // A) and `mouse.rs`'s "Menu bar row click — command center
                // only".
                render::FrameOp::CommandCenter => {
                    // #1877: on a native-menu backend (macOS) `presence.
                    // menu_row` is `false` (#901 suppresses the drawn
                    // `File Edit View` row under AppKit's real menu bar),
                    // so `FrameOp::MenuDropdown`'s own arm — the one that
                    // paints `paint_title_bar_band`'s themed background
                    // fill across `menu_row_rect` — never composes this
                    // frame (`presence.menu_dropdown` stays coupled to
                    // `presence.menu_row`, pinned by
                    // `command_center_liveness_is_split_from_menu_bar_visible`
                    // in `render.rs`). The Command Center rung below is
                    // then the *only* thing painting into this band, and
                    // its own rect starts at `menu_end` — one
                    // `titlebar_control_inset()`-wide step clear of the
                    // leading edge (#940), so vimcode never draws under
                    // the real traffic lights — leaving that leading
                    // strip with no vimcode paint call touching it at
                    // all. A plain CG/Cairo/Direct2D surface shows
                    // whatever the OS filled the view with there (macOS:
                    // the window's own background colour, not the theme),
                    // which is this issue's "traffic-light strip isn't
                    // themed" report. Fill the *entire* row — inset
                    // included — with the theme's title-bar colour first,
                    // the same empty-`MenuBar` background-only trick the
                    // app-icon slot filler above uses, mirroring VS Code's
                    // own macOS title bar (which paints the full row
                    // including behind the traffic lights).
                    //
                    // Review round 1 (#1877) caught an earlier draft of
                    // this comment claiming this is "a no-op repaint on
                    // every other backend" because `presence.menu_row` is
                    // supposedly `true` whenever this rung is live. That
                    // reasoning was wrong as *stated*: `presence.menu_row
                    // == screen.menu_bar_visible && title_bar_band_live`
                    // while `presence.command_center == title_bar_band_live`
                    // alone (`FramePresence::from_screen`, #939) — the two
                    // are deliberately *not* coupled, so `menu_bar_visible`
                    // can be `false` while this rung is still live on
                    // GTK/Win too (`Engine::toggle_menu_bar` at runtime, or
                    // TUI's `cell` profile booting with the menu bar
                    // hidden), independent of `presence.menu_row`.
                    //
                    // It is, however, still a no-op in *every case this
                    // codebase can currently reach* — for a more precise
                    // reason than the coupling claim above, not because of
                    // it. `command_center_rect` (below) is measured with
                    // `leading_inset = control_inset.width.max(0.0)`, and
                    // `Backend::titlebar_control_inset()` is provably
                    // `Rect::default()` (zero) on every backend except
                    // `MacBackend` (see `control_inset_is_default_because_
                    // mac_driver_never_sets_a_window`'s doc in
                    // `src/macos/mod.rs`). With a zero inset and
                    // `presence.menu_row == false`, `items_for_measure`
                    // collapses to zero width and `draw_controls` is also
                    // `false` (`should_draw_window_controls`), so
                    // `command_center_rect` already spans the *entire*
                    // row, x=0 included — this fill then paints the exact
                    // same rect the same colour immediately before
                    // `paint_command_center_rung` does, a real but
                    // invisible duplicate draw call. The one case where it
                    // is NOT redundant is a native-menu backend with a
                    // non-zero inset (macOS, #940's `leading_inset`): there
                    // `command_center_rect` starts *after* the inset, so
                    // this fill is the only thing painting the strip
                    // behind the traffic lights — this issue's actual
                    // fix. GTK's `command_center_stays_live_when_menu_bar_
                    // is_hidden` test pins the GTK-reachable case (row
                    // still paints `theme.tab_bar_bg`) but, per its own
                    // doc, cannot RED-verify this fill specifically —
                    // `paint_command_center_rung`'s own background already
                    // covers the same pixel there, same as this comment
                    // just explained.
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

                // ── Find/replace overlay (#671) ──────────────────────────────
                // Confirmed by #592 to open in engine state (`KEYDBG: OVERLAY
                // STATE OPEN: ["find_replace"]`) with nothing painting on GTK.
                // Unlike quickfix/panel_hover (#670) there *was* a dead painter
                // to port — `draw.rs::draw_find_replace_popup` — but it routed
                // through `Surface::FindReplace` with a rect the rasteriser
                // ignores; calling `Backend::draw_find_replace` directly (same
                // trait method TUI's the pre-#1434 TUI shell's `render_content` calls) is
                // simpler and identical in effect. The GTK rasteriser positions
                // the panel from its own `panel.group_bounds` (already absolute
                // pixel coordinates — #550, same as TUI's absolute cell
                // coordinates) and reads `current_line_height` /
                // `current_char_width` off the backend (set once per frame by
                // quadraui's GTK runner before `render_content` runs), so the
                // `rect` argument here is unused by the GTK rasteriser too;
                // passed for parity with the trait's signature and the TUI call
                // site.
                render::FrameOp::FindReplace => {
                    if let Some(ref find_replace) = screen.find_replace {
                        render::paint_find_replace_rung(backend, find_replace, popup_viewport);
                        composed.push(render::FrameOp::FindReplace);
                    }
                }

                // ── Picker / command-palette overlay (#587) ──────────────────
                // Same class of bug #546 fixed for dialog/context-menu: the
                // palette was painted only by the dead legacy `draw_editor`
                // Cairo path (`draw.rs::draw_picker_popup`), which has zero live
                // callers under ShellApp. So `Ctrl+Shift+P` opened the picker in
                // engine state (`picker_open = true`, items populated) but
                // nothing ever painted — the "command palette fails to open
                // silently" symptom. Geometry comes from the same generic
                // helpers the legacy path used (`PickerGeometry` + this
                // profile's own picker sizing), so no Pango/Cairo access is
                // needed here.
                render::FrameOp::UnifiedPicker => {
                    if let Some(ref picker) = screen.picker {
                        let rect = render::paint_picker_rung(
                            backend,
                            picker,
                            popup_viewport,
                            &(self.units.picker)(lh as f32),
                        );
                        // Hand the *painted* rect to the click/drag handlers (#555).
                        self.picker_popup_rect.set(Some(rect));
                        composed.push(render::FrameOp::UnifiedPicker);
                    }
                }

                // ── Tab switcher popup (Ctrl+Tab MRU list) (#671) ────────────
                // `self.tab_switcher_popup_rect` already exists and is read by
                // `handle_mouse_press`'s "Tab switcher modal arbitration" block
                // (added ahead of this painter, expecting to be fed) — this is
                // the first frame that actually sets it. Sizing/positioning
                // ported from the dead `draw.rs::draw_tab_switcher_popup_list`
                // (pixel-tuned clamp(350, 600) width, unlike TUI's
                // percent-of-terminal-columns sizing, which wouldn't make sense
                // in pixel space); content comes from the same shared
                // `render::tab_switcher_to_quadraui_list_view` adapter TUI's
                // the pre-#1434 TUI shell's `render_content` uses, through
                // `Backend::draw_list`.
                render::FrameOp::TabSwitcher => {
                    if let Some(ref ts) = screen.tab_switcher {
                        // #733: geometry comes from the shared
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

                // ── Context menu (#546, #1580) ────────────────────────────────
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
                        // #1580: a native popup (`Backend::show_context_menu`)
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

                // ── Change-review surface (#955, shared with #525) ───────────
                // `render::paint_change_review_rung` is the whole body — no
                // GTK-specific diff rendering, matching every other
                // `quadraui::DiffView` consumer. Uses the same
                // `popup_viewport` every other overlay rung anchors to. The
                // surface's modal-stack entry is reconciled *before* the walk
                // (see there), not from an `else` here — this arm cannot run
                // on a frame where the surface is closed.
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

                // ── Modal dialog (#546) ──────────────────────────────────────
                // Same #546 story as the context menu above: invisible AND
                // undismissable by mouse under ShellApp — `dialog.is_some()`
                // stayed true forever and `handle_mouse_click_msg`'s dialog block
                // swallowed all subsequent clicks. #735 moved it *above* the
                // context menu (it used to paint underneath on GTK, and on top
                // on TUI): a dialog is the surface `route_modal_overlay_click`
                // hands every event to, so it must also be the surface the user
                // can see.
                //
                // #727: a natively-expressible `screen.dialog` (no `DialogTable`,
                // no text input — `quadraui::native_dialog_options` is the single
                // source of truth for that split) goes through a real OS
                // `AlertDialog` instead of this in-canvas primitive, and is
                // therefore *not* live as a frame rung at all: nothing is
                // composed into this frame, so nothing is recorded. Both halves
                // of that split — the presence gate and the once-per-open native
                // present — are stated before the walk; this arm is the
                // in-canvas half only. A dialog reaching here carries a
                // `DialogTable` or a text input (e.g. the SSH-passphrase
                // prompt), which no native alert facility hosts.
                render::FrameOp::Dialog => {
                    if let Some(panel) = screen.dialog.as_ref() {
                        let dlayout =
                            render::paint_dialog_rung(backend, panel, popup_viewport, cw, lh);
                        *self.dialog_layout.borrow_mut() = Some(dlayout);
                        composed.push(render::FrameOp::Dialog);
                    }
                }

                // ── Toast overlay (#454) — top of the band ───────────────────
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
        // #1580: open the native context-menu popup exactly once, from
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
        // #1427: keep the runner's `AppShell` title-bar reservation in sync
        // with `engine.menu_bar_visible` — that flag can flip from any one
        // of several places inside `handle_dispatch` (the #1427 reveal/hide
        // routing at its top, a `:set menu`/`nomenu` ex-command, the
        // status-bar `[M]` toggle segment's `Engine::handle_status_action`,
        // ...), each with its own early `return`, so this single choke point
        // — run once, after every path through `handle_dispatch` has
        // already returned — is what makes the sync unconditional rather
        // than requiring one call per mutation site. See
        // `render::sync_menu_bar_title_row`'s own doc.
        //
        // Gated on `menu_bar_toggleable`, not unconditional: on GTK/macOS/
        // Win the title-bar *band* is a permanent, construction-time
        // reservation (`shell_config`'s `with_title_bar`) that survives
        // `menu_bar_visible` going `false` on its own — only the *drawn*
        // row/dropdown inside it are coupled to that flag (#939's own
        // regression pin, `gtk::testing::command_center_stays_live_when_
        // menu_bar_is_hidden`: a native-menu backend, or a test forcing the
        // flag false directly, must not lose the Command Center's anchor
        // band). Calling `set_title_bar_visible` there unconditionally
        // would collapse that reservation the moment anything sets the
        // flag false, which is wrong for every profile except the
        // toggleable one, where the band genuinely is meant to come and go
        // with the flag.
        if self.engine.borrow().menu_bar_toggleable {
            render::sync_menu_bar_title_row(ctx, &self.engine.borrow());
        }
        // #1427: keep the runner's `AppShell` sidebar *visibility* in sync
        // with the shadow's too, unconditionally — not just from the two
        // narrower call sites this method already had
        // (`run_post_key_epilogue`, `on_shell_event_ctx`'s `BottomItemClicked`
        // arm) — but *only* on the toggleable-menu-bar profile
        // (`menu_bar_toggleable`), where it's what corrects `AppShell::
        // new`'s "active + visible" construction-time default away from the
        // hamburger *before* the very first hamburger click ever lands, so
        // that click resolves as a reveal rather than `AppShell::
        // handle_activity_click`'s already-active toggle-hide branch — see
        // `render::sync_runner_sidebar_visibility`'s own doc.
        //
        // Deliberately gated, not unconditional like the title-row sync
        // above: GTK/macOS/Win's existing two call sites already keep their
        // runner/shadow sidebar visibility converged on every path that
        // needs it, and a *third*, blanket call on every dispatch surfaced
        // a pre-existing staleness in numerous test fixtures that mutate
        // `session.explorer_visible` post-construction without also calling
        // `Engine::sync_app_shell_sidebar_visibility`/`AppShell::show_panel`
        // (harmless before #1427, since nothing ever read the shadow's
        // `sidebar_visible()` this eagerly) — collapsing their sidebar
        // reservation the moment any dispatch ran. The toggleable profile
        // needs the correction regardless (that's the whole fix), but
        // GTK/macOS/Win do not, so this stays scoped to where #1427
        // actually requires it.
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

    /// #1064 (GOALS.md's 2026-09-16 audit, #1044, wave 2 item 12): the
    /// app-initiated half of the runner ↔ shadow panel sync. Was
    /// unoverridden on `App` (the trait default always returns `None`), so
    /// `ShellAdapter::apply_requested_panel` — polled once after every
    /// `handle()`/`tick()` dispatch — never saw a switch to apply, no
    /// matter what the engine did to its own `app_shell`/`ext_panel_active`.
    ///
    /// The gap: `App::render_content` paints the sidebar's *content* by
    /// reading `engine.app_shell`/`engine.ext_panel_active` directly (the
    /// shadow), so a panel switch the engine makes on its own — with no
    /// runner click involved, e.g. `Engine::process_pending_sidebar`'s DAP
    /// `dap_wants_sidebar` reveal, or `Self::toggle_focus_explorer`/
    /// `Self::toggle_focus_search`'s keyboard accelerators — always painted
    /// the *right* content. But the runner's own chrome (the activity-bar
    /// highlight, and the sidebar-header title `quadraui::AppShell::render`
    /// paints from **its own**, entirely separate, `active_panel()`) has no
    /// other channel to learn about the change — it only ever moves in
    /// response to `AppShell::handle`'s own click hit-testing, or this poll
    /// — so it silently kept showing the previous panel's title forever.
    ///
    /// the pre-#1434 TUI shell's `take_requested_panel` already had this override (see
    /// its own doc for the shared mechanics, mirrored verbatim here); this
    /// is the same logic against `App`'s fields.
    fn take_requested_panel(&mut self) -> Option<quadraui::WidgetId> {
        let engine = self.engine.borrow();
        if !engine.app_shell.sidebar_visible() {
            return None;
        }
        // #557: an extension panel takes over the sidebar body *without*
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

    /// #1057: the ctx-aware override TUI's the pre-#1434 TUI shell already had (its
    /// own title-bar sync, quadraui#617) — `App` only implemented the
    /// deprecated ctx-less [`Self::on_shell_event`] until now, so nothing
    /// here could ever push a shell-state change back into the runner's own
    /// `AppShell` on the same frame an event fires.
    ///
    /// That gap stayed invisible as long as every `AppShellEvent` arm's
    /// runner-visible outcome was something the runner had *already*
    /// decided before calling in — `PanelChanged`/`SidebarHidden` for a top
    /// panel: the runner's own `AppShell` toggles itself first (that's
    /// *why* it reports one or the other), and `on_shell_event` just
    /// mirrors that decision into `engine.app_shell`, the shadow copy.
    /// `BottomItemClicked` breaks that assumption: the runner never toggles
    /// a *bottom* item itself (see that arm's own doc, above — it only
    /// ever reports the click), so the toggle-to-hide decision is 100% made
    /// inside `on_shell_event`, entirely within `engine.app_shell`, with no
    /// way to tell the runner. Without this override, `engine.app_shell.
    /// sidebar_visible()` correctly flips to `false` on a second Settings
    /// click, but the runner's own `AppShell` — which is what actually
    /// determines whether `render_content`'s sidebar column exists in the
    /// composited frame, not `engine.app_shell` — never learns, and keeps
    /// painting the sidebar as if nothing changed. Push the shadow's new
    /// state through the same [`Self::sync_runner_sidebar_visibility`] the
    /// key-dispatch epilogue already uses for the same reason (#762).
    ///
    /// Verified directly: before this override existed,
    /// `bottom_item_second_click_collapses_sidebar`'s `gtk`/`tui` arms in
    /// `src/harness.rs` went red at the second-click assertion — the
    /// engine-side state was already correct (`sidebar_visible() == false`),
    /// only the paint wasn't following it.
    fn on_shell_event_ctx(
        &mut self,
        event: &quadraui::AppShellEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        // #1427: a `SidebarHidden` for the hamburger — the runner's second
        // click on it while the menu is open — has to be special-cased
        // *here*, before delegating to the ctx-less `Self::on_shell_event`
        // below, because only `ctx` (the runner's own `AppShell`) can tell
        // the hamburger apart from a real panel's own second click; the
        // shadow `engine.app_shell` has no hamburger `PanelDefinition` at
        // all (see `render::route_hamburger_panel_changed`'s doc). Mirrors
        // the pre-#1434 TUI shell's `on_shell_event_ctx`'s identical check, now
        // shared via `render::route_hamburger_sidebar_hidden`. A no-op check
        // on GTK/macOS/Win, which never register this panel id.
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
        // #1356 (quadraui bump for quadraui#1055): a bottom item's click
        // never moves the *real* `AppShell` on its own — the `BottomItemClicked`
        // arm above only toggles the engine-side shadow via `switch_panel`.
        // quadraui#1055 lets `show_panel` accept a bottom item's id (e.g.
        // "bottom:settings"), but an app has to opt in explicitly by calling
        // it from its own `BottomItemClicked` handler — exactly what
        // quadraui's own `AppShellDemo` does. Without this, the runner's own
        // `AppShell` (which is what `render` titles the sidebar header from)
        // never learns Settings now owns the sidebar, and the header stays
        // stuck on whatever top panel was open before.
        if let quadraui::AppShellEvent::BottomItemClicked { id } = event {
            if self.engine.borrow().app_shell.sidebar_visible() {
                ctx.shell_mut().show_panel(id);
            } else {
                ctx.shell_mut().hide_sidebar();
            }
        }
        self.sync_runner_sidebar_visibility(ctx);
        // #1427: `ShellAdapter::handle` consumes a `PanelChanged`/
        // `SidebarHidden` for a top-row panel itself and returns without
        // ever calling `Self::handle` — the one place `Self::handle`'s own
        // `render::sync_menu_bar_title_row` call can never reach. Mirrors
        // the pre-#1434 TUI shell's `on_shell_event_ctx`'s identical tail call
        // — see that function's own doc for why running this on every path
        // (not just the hamburger arm above) is what makes any *future* arm
        // that flips `menu_bar_visible` get the same same-frame guarantee
        // for free. Gated on `menu_bar_toggleable` for the same reason as
        // `Self::handle`'s own identical call — see that call site's doc.
        if self.engine.borrow().menu_bar_toggleable {
            render::sync_menu_bar_title_row(ctx, &self.engine.borrow());
        }
    }
}
