//! `TuiShellApp` — TUI counterpart to `src/gtk/mod.rs`'s `impl ShellApp for
//! App` (#493). Tracks vimcode#595.
//!
//! **Status: dormant scaffold, not wired into any live entry point.**
//! `tui_main::run()` / `event_loop()` (`mod.rs:635`/`:787`) remain the live
//! TUI path — this module compiles alongside them (same "coexistence"
//! pattern GTK's own #448-B dormant impl used before its live cutover) and
//! is exercised only by this module's own `#[cfg(test)]` `driver_with_shell`
//! tests.
//!
//! # What's fully ported (moved, not rewritten, from `event_loop()`)
//!
//! - [`ShellApp::setup`] — one-time init: MSV backend info, nerd-font
//!   detection, clipboard, panel-key accelerators, menu defs
//!   (`mod.rs:641`-`:676`, `:797`-`:830`).
//! - [`ShellApp::tick`] — per-frame viewport sync (`mod.rs:916`-`:967`) +
//!   idle background work (`mod.rs:1157`-`:1247`): `poll_idle`,
//!   format-on-save deferred quit, sidebar/SC auto-refresh, settings
//!   reload, pending terminal command, startup message, ext-panel focus
//!   request, yank-highlight expiry, tab-switcher auto-confirm.
//!
//! # What's intentionally NOT yet ported, and why
//!
//! Three structural gaps were found while scoping Stage 0 (recorded as
//! pinned `coord context` notes on vimcode#595). Gap 2 (mouse handling) is
//! largely closed by #602 — dispatch is wired and the panel intercepts are
//! ported — with one residual seam noted inline below; gaps 1 and 3
//! (painting, cursor placement) remain open:
//!
//! 1. **Painting.** `render_content(&self, backend: &mut dyn Backend, ...)`
//!    never gets a raw `ratatui::Frame` — confirmed structural, not just
//!    unwired: quadraui's `TuiBackend` stashes its frame pointer in a
//!    *private* field (`current_frame_ptr`) with no public accessor, and
//!    `render_content` runs inside quadraui's own `enter_frame_scope` (see
//!    `shell_adapter.rs::ShellAdapter::render` /
//!    `tui/run.rs::render_frame`) — so `Backend::draw_*` trait calls work
//!    fine (they use the smuggled pointer internally), but nothing needing
//!    raw `Frame`/`Buffer` access can ever work from this signature, full
//!    stop. #600 (Stage 1) converted the ~13 sites that were free-function
//!    calls with a trait equivalent; #601 (Stage 2) wires the
//!    now-trait-only subset — editor windows, tab bars, breadcrumb bars,
//!    per-window status lines, and the completion/hover/editor-hover/
//!    diff-peek/signature-help popups — into `render_content`, via
//!    `render_impl.rs::build_screen_for_shell_content` +
//!    `paint_editor_popups`. What #601 still cannot paint (true raw-buffer
//!    holdouts, each filed as its own follow-on, all blocking #605
//!    cutover): sidebar panel content (#607), quickfix panel + bottom
//!    panel/terminal PTY content (#608), window/group divider lines +
//!    tab-drag overlay + tab-hover tooltip (#609), and cursor placement
//!    (#604 / quadraui#466). The menu bar row is reserved in the layout
//!    math but not painted either (out of scope for #601; folds into key
//!    dispatch, #603).
//! 2. **Mouse handling (#602, largely resolved).** `mouse::handle_mouse`
//!    (~4,100 lines) takes `&mut quadraui::DragState` + `&mut
//!    quadraui::ModalStack` directly via `TuiBackend::drag_and_modal_mut()`
//!    — a concrete-only method the `Backend` trait deliberately doesn't
//!    expose, so it couldn't be called from `handle(&mut self, event,
//!    backend: &mut dyn Backend, ...)` as-is. `Backend::drag_and_modal_mut`
//!    (quadraui#467) landed a trait-level accessor, unblocking the fix:
//!    [`TuiShellApp::handle_mouse_event`] bridges a mouse-shaped [`UiEvent`]
//!    back to a crossterm `MouseEvent` and dispatches through
//!    `mouse::handle_mouse`, but *before* that it also ports the four panel
//!    intercepts `event_loop` runs ahead of `handle_mouse` (debug sidebar,
//!    extensions sidebar, debug toolbar hit-test, explorer
//!    `TreeController`; `mod.rs` ~1416-1605) — see that method's own doc
//!    comment for the details, including why the debug toolbar intercept is
//!    a documented no-op until gap 1 paints that toolbar's rect.
//!
//!    Residual seam (blocked on gap 1, not on #602): the context menu is
//!    still painted only on the raw-`Frame` path, so
//!    `self.context_menu_layout` stays `None` and `mouse.rs`'s `#459`
//!    modal-stack reconcile never registers an open menu. Dispatch stays
//!    *correct* — the intercepts gate on `engine.context_menu` directly
//!    (see `handle_mouse_event`) so menu clicks aren't stolen by the panel
//!    underneath — but `handle_mouse` receives `context_menu_layout: None`
//!    and therefore closes the menu instead of resolving the clicked item.
//!    Clears once the explorer menu paints through `render_content`, which
//!    lands with the sidebar-content holdout (#607) since the menu anchors
//!    over the sidebar, outside `main_content_bounds`.
//! 3. **Editor cursor placement.** `Backend::draw_editor`'s
//!    `EditorPaintResult::cursor_position` is documented "host applies via
//!    `Frame::set_cursor_position`", but no consumer of it exists anywhere
//!    in quadraui's `shell_adapter`/TUI runner — `render_content` has no
//!    Frame to call it on. Filed as quadraui#466: cache the position on
//!    `TuiBackend`, apply it in `tui/run.rs::render_frame` the same way
//!    `apply_selection_highlight` already runs post-`render_content`.
//!    Tracked as #604.
//!
//! Given (1), `handle()` below implements panel-key accelerators, the
//! `MenuSystem` intercept, and (per gap 2 above) full mouse dispatch through
//! `handle_mouse_event`, plus routes plain `KeyPressed` events (no mouse) to
//! `Engine::handle_key`, which is already backend-agnostic. Everything else
//! returns `Reaction::Continue` with a `// TODO(#595)` marker rather than a
//! half-correct guess.
//!
//! Also NOT yet ported: the "#318" Alt+menu-letter "reveal menu bar" shim
//! sitting between those two dispatch layers in `event_loop()`
//! (`mod.rs:1275`-`:1294`), which sets `engine.menu_bar_visible = true` on
//! an Alt+<letter> keypress so the same keystroke both reveals and
//! activates the menu. It's not a fourth structural gap — the blocker is
//! simply that `KeyPressed` routing is itself still a `// TODO(#595)` stub
//! below — but it's called out here explicitly so a future session doesn't
//! assume key dispatch is complete once that stub is filled in.

use std::cell::{Cell, RefCell};

use quadraui::{Reaction, ShellApp, ShellContext, UiEvent};

use super::*;

/// Link hit rects from a hover popup render: `(x, y, w, h, url)`, matching
/// `event_loop`'s `hover_link_rects`/`editor_hover_link_rects` locals
/// verbatim. Named alias so the `TuiShellApp` fields below don't trip
/// clippy's `type_complexity` lint.
type HoverLinkRects = Vec<(u16, u16, u16, u16, String)>;

/// TUI counterpart to GTK's `App` struct. Owns everything that is a local
/// `mut` variable in `event_loop()` today. Fields the (`&self`)
/// `render_content` needs to *write* during paint are wrapped in
/// `Cell`/`RefCell` — mirroring GTK's `App` (`menu_row_rect: Cell<Rect>`,
/// etc.) and the render-time caches `Engine` itself already uses
/// (`sc_panel_layout`, `explorer_tree_rect`, ...).
///
/// `#[allow(dead_code)]`: this is a dormant scaffold (vimcode#595 Stage 0)
/// — not yet constructed from `main.rs`/`tui_bin.rs`, only from this
/// module's own `#[cfg(test)]` tests, so a plain (non-test) `cargo build`
/// sees it as never-constructed. GTK's equivalent dormant impl (#448-B)
/// didn't need this because `App` was already live-constructed elsewhere;
/// `TuiShellApp` has no such site yet. Remove once Stage 6 wires this into
/// the live entry point — see `PLAN.md`'s "Staged plan" for #595.
#[allow(dead_code)]
pub(super) struct TuiShellApp {
    pub(super) engine: Engine,
    sidebar: TuiSidebar,
    sidebar_width: u16,
    folder_picker: Option<FolderPickerState>,
    quickfix_scroll_top: usize,
    dragging_sidebar: bool,
    dragging_terminal_resize: bool,
    dragging_terminal_split: bool,
    dragging_group_divider: Option<usize>,
    dragging_window_divider: Option<(GroupId, usize)>,
    hover_selecting: bool,
    fr_input_dragging: bool,
    last_layout: RefCell<Option<render::ScreenLayout>>,
    debug_toolbar_rect: Cell<quadraui::Rect>,
    last_click_time: Cell<Instant>,
    last_click_pos: Cell<(u16, u16)>,
    cmd_sel: Cell<Option<(usize, usize)>>,
    cmd_dragging: bool,
    explorer_sb_dragging: bool,
    explorer_drag_src: Option<usize>,
    explorer_drag_active: Option<(usize, Option<usize>)>,
    tab_drag_start: Option<(u16, u16)>,
    tab_dragging: bool,
    tui_drag_source: Option<(GroupId, usize)>,
    tui_drag_cursor: Option<(f64, f64)>,
    tui_tab_drop_zone: crate::core::window::DropZone,
    last_clipboard_content: Option<String>,
    pending_startup_msg: Option<String>,
    had_popup_overlay: Cell<bool>,
    hover_link_rects: RefCell<HoverLinkRects>,
    hover_popup_rect: Cell<Option<(u16, u16, u16, u16)>>,
    editor_hover_popup_rect: Cell<Option<(u16, u16, u16, u16)>>,
    editor_hover_link_rects: RefCell<HoverLinkRects>,
    editor_hover_scrollbar: RefCell<Option<render::PopupScrollbarHit>>,
    /// Last cursor position returned by `Backend::draw_editor`, cached for
    /// whenever a runner-side consumer exists (gap 3 above). Unused today.
    #[allow(dead_code)]
    last_editor_cursor: Cell<Option<(u16, u16)>>,
    completion_layout: RefCell<Option<quadraui::CompletionsLayout>>,
    context_menu_layout: RefCell<Option<quadraui::ContextMenuLayout>>,
    dialog_layout: RefCell<Option<quadraui::DialogLayout>>,
    last_sidebar_refresh: Cell<Instant>,
    yank_hl_deadline: Cell<Option<Instant>>,
    tab_switcher_last_cycle: Cell<Option<Instant>>,
}

#[allow(dead_code)] // see the struct-level #[allow(dead_code)] doc above
impl TuiShellApp {
    /// Construct the app, running the engine-only startup work that
    /// `tui_main::run()` currently does before entering raw mode
    /// (`mod.rs:641`-`:678`) — none of it needs a terminal or backend.
    pub(super) fn new(file_path: Option<PathBuf>) -> Self {
        let mut engine = Engine::new();
        let msv_metrics = quadraui::MsvLayoutMetrics {
            header_size: 1.0,
            divider_size: 0.0,
            scrollbar_size: 1.0,
            cell_quantum: 1.0,
        };
        engine
            .ext_sidebar_system
            .borrow_mut()
            .set_backend_info(1.0, msv_metrics);
        engine
            .sc_sidebar_system
            .borrow_mut()
            .set_backend_info(1.0, msv_metrics);
        engine
            .search_sidebar_system
            .borrow_mut()
            .set_backend_info(1.0, msv_metrics);

        let nerd_font_missing =
            engine.settings.use_nerd_fonts && !icons::detect_nerd_font_windows();
        if nerd_font_missing {
            engine.settings.use_nerd_fonts = false;
        }
        icons::set_nerd_fonts(engine.settings.use_nerd_fonts);
        engine.startup(file_path.as_deref());
        setup_tui_clipboard(&mut engine);

        let pending_startup_msg = if nerd_font_missing {
            Some(
                "No Nerd Font detected — using fallback icons. Install a Nerd Font and run \
                 :set nerdfonts to enable."
                    .to_string(),
            )
        } else {
            None
        };

        let now = Instant::now();
        Self {
            engine,
            sidebar: TuiSidebar::new(),
            sidebar_width: SIDEBAR_WIDTH,
            folder_picker: None,
            quickfix_scroll_top: 0,
            dragging_sidebar: false,
            dragging_terminal_resize: false,
            dragging_terminal_split: false,
            dragging_group_divider: None,
            dragging_window_divider: None,
            hover_selecting: false,
            fr_input_dragging: false,
            last_layout: RefCell::new(None),
            debug_toolbar_rect: Cell::new(quadraui::Rect::default()),
            last_click_time: Cell::new(now.checked_sub(Duration::from_secs(1)).unwrap_or(now)),
            last_click_pos: Cell::new((0, 0)),
            cmd_sel: Cell::new(None),
            cmd_dragging: false,
            explorer_sb_dragging: false,
            explorer_drag_src: None,
            explorer_drag_active: None,
            tab_drag_start: None,
            tab_dragging: false,
            tui_drag_source: None,
            tui_drag_cursor: None,
            tui_tab_drop_zone: crate::core::window::DropZone::None,
            last_clipboard_content: None,
            pending_startup_msg,
            had_popup_overlay: Cell::new(false),
            hover_link_rects: RefCell::new(Vec::new()),
            hover_popup_rect: Cell::new(None),
            editor_hover_popup_rect: Cell::new(None),
            editor_hover_link_rects: RefCell::new(Vec::new()),
            editor_hover_scrollbar: RefCell::new(None),
            last_editor_cursor: Cell::new(None),
            completion_layout: RefCell::new(None),
            context_menu_layout: RefCell::new(None),
            dialog_layout: RefCell::new(None),
            last_sidebar_refresh: Cell::new(now),
            yank_hl_deadline: Cell::new(None),
            tab_switcher_last_cycle: Cell::new(None),
        }
    }

    fn theme(&self) -> Theme {
        Theme::from_name(&self.engine.settings.colorscheme)
    }

    /// #602 (gap 2): translate a mouse-shaped [`UiEvent`] back into the
    /// crossterm [`MouseEvent`] `mouse::handle_mouse` — the ~4,100-line
    /// legacy handler `event_loop()` still drives — expects, and dispatch
    /// through it.
    ///
    /// Mirrors `event_loop`'s own bridge verbatim (see its
    /// `events::uievent_to_crossterm` call ahead of its `Event::Mouse` arm,
    /// `mod.rs`): fold `DoubleClick` back to `MouseDown` first (crossterm has
    /// no double-click concept; `handle_mouse` re-derives it from its own
    /// `last_click_time`/`last_click_pos` pair), then round-trip through
    /// [`super::events::uievent_to_crossterm`]. The one piece `event_loop`
    /// couldn't do — obtain `&mut DragState` *and* `&mut ModalStack` from a
    /// `&mut dyn Backend` — is exactly what `Backend::drag_and_modal_mut`
    /// (quadraui#467) was built for.
    ///
    /// Every other `&mut` parameter `handle_mouse` needs is threaded from a
    /// `TuiShellApp` field the struct already carries 1:1 for this purpose —
    /// each was ported from `event_loop`'s locals back in Stage 0 anticipating
    /// this stage. `Cell`/`RefCell`-wrapped fields are copied/borrowed out
    /// into locals and written back after the call, since `handle_mouse`
    /// wants plain `&mut`/`&`, not interior-mutability handles.
    ///
    /// Review fix (#602 iteration 1): `event_loop` doesn't send every mouse
    /// event straight to `handle_mouse` — it runs four panel intercepts
    /// first (debug sidebar, extensions sidebar, debug toolbar, explorer
    /// `TreeController`; `mod.rs` ~1436-1604), each gated by the `#459`
    /// modal-stack priority check that `mod.rs` computes into its own
    /// `ctx_blocks_event` local (`mod.rs` ~1416-1433 — the `// #459: Hit-test
    /// the modal stack` block), and short-circuits `handle_mouse` entirely
    /// when one of them consumes the event. `handle_mouse`'s own
    /// `PANEL_EXTENSIONS` arm explicitly relies on the sidebar intercept for
    /// rows 2+ ("handled by SidebarSystem mouse intercept in main loop",
    /// `mouse.rs` ~2557), so skipping this block silently drops those
    /// clicks. All four are ported below, in the same order, before the
    /// `DoubleClick` fold / `handle_mouse` dispatch.
    ///
    /// Review fix (#602 iteration 2): those intercepts are additionally
    /// gated on `engine.context_menu.is_none()`, which `event_loop` does
    /// *not* need. See the long comment on `ctx_menu_blocks_event` in the
    /// body for why the modal-stack gate alone is load-bearing there but
    /// inert here.
    fn handle_mouse_event(
        &mut self,
        event: UiEvent,
        backend: &mut dyn quadraui::Backend,
    ) -> Reaction {
        // #459: hit-test the modal stack first. The reconcile happens in
        // `mouse.rs` at the top of `handle_mouse` (`mouse.rs` ~255-267);
        // here it must run before the panel intercepts below, which have to
        // yield when the event lands inside a floating modal (e.g. an open
        // editor-hover popup or picker) — the same priority rule
        // `event_loop` applies (`mod.rs` ~1423-1433).
        let modal_blocks_event = {
            let event_pos: Option<quadraui::Point> = match &event {
                UiEvent::Scroll { position, .. }
                | UiEvent::MouseDown { position, .. }
                | UiEvent::MouseUp { position, .. }
                | UiEvent::MouseMoved { position, .. }
                | UiEvent::DoubleClick { position, .. } => Some(*position),
                _ => None,
            };
            let (_, modal_stack) = backend.drag_and_modal_mut();
            event_pos.is_some_and(|p| modal_stack.hit_test(p).is_some())
        };

        // Review fix (#602 iteration 2): `modal_blocks_event` alone is NOT
        // sufficient here, even though it is the only gate `event_loop`
        // needs. `mouse.rs`'s `#459` reconcile is what registers an open
        // context menu on the `ModalStack`, and it does so from the
        // `context_menu_layout` argument:
        //
        //     match context_menu_layout {
        //         Some(layout) => modal_stack.push(ctx_menu_id, layout.bounds),
        //         None => { modal_stack.pop(&ctx_menu_id); }
        //     }
        //
        // `event_loop` feeds that argument a layout its raw-`Frame` paint
        // produced (`draw_frame`'s `context_menu_layout_out` out-param,
        // `render_impl.rs` ~916, threaded via `mod.rs` ~1101/~1158), so its
        // modal stack really does learn about the menu. `TuiShellApp`'s
        // `self.context_menu_layout` is *never written* — the context-menu
        // paint still lives on the raw-`Frame` path `render_content` cannot
        // reach (module-doc gap 1; the explorer menu anchors over the
        // sidebar, i.e. outside `main_content_bounds`, so it lands with the
        // sidebar-content holdout #607). It is therefore permanently `None`
        // here, the reconcile always takes the `pop` branch, and
        // `modal_blocks_event` can never become `true` for a context menu.
        //
        // Without the extra engine-level gate below, right-clicking an
        // explorer file and then left-clicking a menu item would have the
        // click land inside `explorer_tree_rect` (the menu floats over the
        // tree), get claimed by the ported `TreeController` intercept as a
        // row activation, and never reach `mouse.rs`'s own context-menu
        // confirm (`mouse.rs` ~1651+) — exactly the bug class `#456`
        // guarded against. Gate on `engine.context_menu` directly: it is
        // the authoritative state, it costs nothing, and it stays correct
        // if/when #607 makes the modal-stack path work too.
        let ctx_menu_blocks_event = self.engine.context_menu.is_some();
        let intercepts_blocked = modal_blocks_event || ctx_menu_blocks_event;

        // ── SidebarSystem intercept: debug sidebar (mirrors `mod.rs`
        // ~1436-1471) ──
        if !intercepts_blocked
            && self.engine.app_shell.sidebar_visible()
            && self.engine.active_panel_is(PANEL_DEBUG)
        {
            let rect = self.engine.dap_sidebar_body_rect.get();
            let is_sidebar_mouse = rect.width > 0.0
                && match &event {
                    UiEvent::Scroll { position, .. }
                    | UiEvent::MouseDown { position, .. }
                    | UiEvent::MouseUp { position, .. }
                    | UiEvent::MouseMoved { position, .. } => rect.contains(*position),
                    _ => false,
                };
            if is_sidebar_mouse {
                if matches!(event, UiEvent::MouseDown { .. }) {
                    self.sidebar.has_focus = true;
                    self.engine.dap_sidebar_has_focus = true;
                }
                render::populate_dap_sidebar_system(&self.engine);
                let sidebar_event = self
                    .engine
                    .dap_sidebar_system
                    .borrow_mut()
                    .handle(&event, backend, rect);
                if self.engine.dispatch_dap_sidebar_event(sidebar_event) {
                    return Reaction::Redraw;
                }
            }
        }

        // ── SidebarSystem intercept: extensions sidebar (mirrors `mod.rs`
        // ~1473-1498). Not redundant with `handle_mouse`'s own
        // `PANEL_EXTENSIONS` arm — that arm explicitly declines rows 2+
        // ("handled by SidebarSystem mouse intercept in main loop",
        // `mouse.rs` ~2557), so skipping this would silently drop those
        // clicks. ──
        if !intercepts_blocked
            && self.engine.app_shell.sidebar_visible()
            && self.engine.active_panel_is(PANEL_EXTENSIONS)
        {
            let rect = self.engine.ext_sidebar_body_rect.get();
            let is_sidebar_mouse = rect.width > 0.0
                && match &event {
                    UiEvent::Scroll { position, .. }
                    | UiEvent::MouseDown { position, .. }
                    | UiEvent::MouseUp { position, .. }
                    | UiEvent::MouseMoved { position, .. } => rect.contains(*position),
                    _ => false,
                };
            if is_sidebar_mouse {
                if matches!(event, UiEvent::MouseDown { .. }) {
                    self.sidebar.has_focus = true;
                    self.engine.ext_sidebar_has_focus = true;
                }
                if self.engine.handle_ext_sidebar_ui_event(event.clone()) {
                    return Reaction::Redraw;
                }
            }
        }

        // ── Debug toolbar hover/press via `ToolbarLayout` hit-test (mirrors
        // `mod.rs` ~1500-1543, #510).
        //
        // `self.debug_toolbar_rect` is populated by the raw-`Frame` toolbar
        // paint (`render_impl.rs::draw_frame`'s `debug_toolbar_rect_out`
        // out-param), which `render_content` cannot call into yet (module
        // doc gap 1 — the debug toolbar isn't painted through
        // `ShellApp::render_content` today). Until that lands, the rect
        // stays zero-sized and this block is a documented no-op rather than
        // a silent gap — ported now so dispatch is already correct once
        // painting catches up. ──
        if !intercepts_blocked
            && self.engine.debug_toolbar_visible
            && self.debug_toolbar_rect.get().width > 0.0
        {
            let rect = self.debug_toolbar_rect.get();
            match &event {
                UiEvent::MouseDown { position, .. } => {
                    let p = *position;
                    if p.y >= rect.y && p.y < rect.y + rect.height {
                        let idx = self.engine.debug_button_hit(p.x, p.y);
                        self.engine.debug_button_pressed = idx;
                        if let Some(i) = idx {
                            if let Some(btn) = render::DEBUG_BUTTONS.get(i) {
                                let _ = self.engine.execute_command(btn.action);
                            }
                        }
                        return Reaction::Redraw;
                    }
                }
                UiEvent::MouseMoved { position, .. } => {
                    let p = *position;
                    let new_hover = if p.y >= rect.y && p.y < rect.y + rect.height {
                        self.engine.debug_button_hit(p.x, p.y)
                    } else {
                        None
                    };
                    if self.engine.debug_button_hovered != new_hover {
                        self.engine.debug_button_hovered = new_hover;
                        return Reaction::Redraw;
                    }
                }
                UiEvent::MouseUp { .. } => {
                    if self.engine.debug_button_pressed.is_some() {
                        self.engine.debug_button_pressed = None;
                        return Reaction::Redraw;
                    }
                }
                _ => {}
            }
        }

        // ── Explorer mouse events → `TreeController` (mirrors `mod.rs`
        // ~1545-1605). Routes mouse events through `TreeController::handle`
        // so the built-in scrollbar (click, thumb drag, track page) works;
        // `MouseDown`/`DoubleClick` for row selection, `MouseMoved` (left
        // held) and `MouseUp` for scrollbar drag lifecycle. The `MouseMoved`/
        // `MouseUp` arm intentionally ignores `intercepts_blocked` (matches
        // `event_loop`) so an in-flight scrollbar drag isn't interrupted by
        // the pointer momentarily crossing a modal's hit region. ──
        {
            let is_explorer_event = match &event {
                UiEvent::MouseDown { position, .. } | UiEvent::DoubleClick { position, .. } => {
                    let rect = self.engine.explorer_tree_rect.get();
                    !intercepts_blocked
                        && self.engine.app_shell.sidebar_visible()
                        && self.engine.active_panel_is(PANEL_EXPLORER)
                        && rect.width > 0.0
                        && rect.contains(*position)
                }
                UiEvent::MouseMoved { .. } | UiEvent::MouseUp { .. } => self.explorer_sb_dragging,
                _ => false,
            };
            if is_explorer_event {
                let rect = self.engine.explorer_tree_rect.get();
                let theme = self.theme();
                render::populate_explorer_tree_controller(&self.engine, &theme);
                let tree_event = self
                    .engine
                    .explorer_tree
                    .borrow_mut()
                    .handle(&event, backend, rect);
                let is_scrollbar =
                    matches!(tree_event, quadraui::TreeControllerEvent::ScrollChanged);
                match &event {
                    UiEvent::DoubleClick { .. } => {
                        self.engine.explorer_has_focus = true;
                        self.sidebar.has_focus = true;
                        self.engine.dispatch_explorer_tree_event(tree_event);
                    }
                    UiEvent::MouseDown { .. } => {
                        if is_scrollbar {
                            self.explorer_sb_dragging = true;
                        } else {
                            self.engine.explorer_has_focus = true;
                            self.sidebar.has_focus = true;
                        }
                        self.engine.handle_explorer_mouse_event(tree_event);
                    }
                    UiEvent::MouseUp { .. } => {
                        self.explorer_sb_dragging = false;
                    }
                    _ => {} // MouseMoved — TreeController drag_to() handles internally
                }
                return Reaction::Redraw;
            }
        }

        let event = match event {
            UiEvent::DoubleClick { position, .. } => UiEvent::MouseDown {
                button: quadraui::MouseButton::Left,
                position,
                modifiers: quadraui::Modifiers::default(),
                widget: None,
            },
            other => other,
        };

        // Every mouse-shaped `UiEvent` this method is called with round-trips
        // (see `synth_mouseevent`) — the `None`/non-`Mouse` arm is defensive,
        // not expected to be reached.
        let Some(Event::Mouse(mouse_event)) = super::events::uievent_to_crossterm(event) else {
            return Reaction::Continue;
        };

        let viewport = backend.viewport();
        let terminal_size = Some(Size::new(
            viewport.width.round() as u16,
            viewport.height.round() as u16,
        ));

        let mut last_click_time = self.last_click_time.get();
        let mut last_click_pos = self.last_click_pos.get();
        let mut cmd_sel = self.cmd_sel.get();
        let mut should_quit = false;
        let last_layout = self.last_layout.borrow();
        let hover_link_rects = self.hover_link_rects.borrow();
        let editor_hover_link_rects = self.editor_hover_link_rects.borrow();
        let completion_layout = self.completion_layout.borrow();
        let context_menu_layout = self.context_menu_layout.borrow();
        let dialog_layout = self.dialog_layout.borrow();
        let editor_hover_scrollbar = *self.editor_hover_scrollbar.borrow();
        let (drag_state, modal_stack) = backend.drag_and_modal_mut();

        let new_sidebar_width = super::mouse::handle_mouse(
            mouse_event,
            &mut self.sidebar,
            &mut self.engine,
            &terminal_size,
            self.sidebar_width,
            &mut self.dragging_sidebar,
            &mut self.dragging_terminal_resize,
            &mut self.dragging_terminal_split,
            &mut self.dragging_group_divider,
            &mut self.dragging_window_divider,
            drag_state,
            modal_stack,
            last_layout.as_ref(),
            &mut last_click_time,
            &mut last_click_pos,
            &mut self.folder_picker,
            &mut cmd_sel,
            &mut self.cmd_dragging,
            &mut should_quit,
            &mut self.explorer_drag_src,
            &mut self.explorer_drag_active,
            &mut self.tab_drag_start,
            &mut self.tab_dragging,
            &mut self.tui_drag_source,
            &mut self.tui_drag_cursor,
            &mut self.tui_tab_drop_zone,
            &hover_link_rects,
            self.hover_popup_rect.get(),
            self.editor_hover_popup_rect.get(),
            &editor_hover_link_rects,
            editor_hover_scrollbar,
            &mut self.hover_selecting,
            &mut self.fr_input_dragging,
            completion_layout.as_ref(),
            context_menu_layout.as_ref(),
            dialog_layout.as_ref(),
        );

        drop(last_layout);
        drop(hover_link_rects);
        drop(editor_hover_link_rects);
        drop(completion_layout);
        drop(context_menu_layout);
        drop(dialog_layout);

        self.sidebar_width = new_sidebar_width;
        self.last_click_time.set(last_click_time);
        self.last_click_pos.set(last_click_pos);
        self.cmd_sel.set(cmd_sel);

        if should_quit {
            return Reaction::Exit;
        }

        // Poll editor hover dwell / inline blame after every dispatched
        // mouse event so the timer can fire even when continuous mouse
        // events prevent idle polling — mirrors `event_loop`'s own
        // post-`handle_mouse` calls (`mod.rs`, right after both
        // `handle_mouse` call sites).
        self.engine.poll_editor_hover();
        self.engine.poll_blame();

        // Mouse events (clicks, drags) almost always change visual state —
        // mirrors `event_loop`'s own unconditional post-mouse redraw.
        Reaction::Redraw
    }
}

impl ShellApp for TuiShellApp {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // TUI menu bar can be fully hidden (unlike GTK where it acts as the
        // title bar) — mirrors `event_loop`'s `mod.rs:797`.
        self.engine.menu_bar_toggleable = true;

        render::sync_nerd_fonts(backend, &self.engine);
        register_panel_accelerators(backend, &self.engine.settings.panel_keys);
        self.engine
            .menu_system
            .borrow_mut()
            .set_menus(render::build_menu_defs(self.engine.is_vscode_mode()));
    }

    fn render_content(
        &self,
        backend: &mut dyn quadraui::Backend,
        layout: &quadraui::AppShellLayout,
    ) {
        // #601: paints the trait-portable subset of `draw_frame` — editor
        // windows, tab bars, breadcrumb bars, per-window status lines, and
        // the editor-anchored popups — into `layout.main_content_bounds`.
        // See the module doc's gap (1) for exactly what's still deferred
        // (sidebar content #607, quickfix/bottom panel #608, dividers/
        // drag-overlay/tab-tooltip #609, cursor placement #604) and why.
        let theme = self.theme();
        let main = layout.main_content_bounds;
        if main.width < 1.0 || main.height < 1.0 {
            return;
        }
        let area = Rect {
            x: main.x.round() as u16,
            y: main.y.round() as u16,
            width: main.width.round() as u16,
            height: main.height.round() as u16,
        };

        backend.set_theme(super::quadraui_tui::q_theme(&theme));

        let screen = build_screen_for_shell_content(&self.engine, &theme, area);

        // ── Tab bar(s) + breadcrumb bar(s) + editor windows ─────────────
        // Windows are painted first (matches `draw_frame`'s split-group
        // order — see its own comment) so window content can't overwrite
        // an adjacent group's tab bar; divider lines between windows are
        // skipped here (#609), same as passing `frame: None` skips them in
        // `render_all_windows`.
        render_all_windows(backend, None, &screen.windows, &theme);

        let tui_tbh: f64 = if self.engine.settings.breadcrumbs && !self.engine.terminal_maximized {
            2.0
        } else {
            1.0
        };
        let tab_bar_targets = render::tab_bar_draw_targets(
            &self.engine,
            &screen,
            1.0,
            tui_tbh,
            (area.x as f64, area.y as f64, area.width as f64),
        );
        for target in &tab_bar_targets {
            let g_tab = Rect {
                x: target.rect.x as u16,
                y: target.rect.y as u16,
                width: target.rect.width as u16,
                height: 1,
            };
            render_tab_bar(backend, g_tab, target.bar, &theme);
        }
        for t in render::breadcrumb_draw_targets(&screen, self.engine.terminal_maximized, 1.0) {
            let bc_rect = Rect {
                x: t.rect.x as u16,
                y: t.rect.y as u16,
                width: t.rect.width as u16,
                height: 1,
            };
            let bc_layout = draw_breadcrumb_bar(backend, bc_rect, t.bar, &theme);
            *t.draw_layout.borrow_mut() = Some(bc_layout);
        }

        // ── Editor-anchored popups (completion/hover/editor-hover/
        // diff-peek/signature-help) — same code `draw_frame` calls, all
        // already trait-only (#601's `paint_editor_popups` extraction).
        let mut completion_layout = self.completion_layout.borrow_mut();
        let mut editor_hover_link_rects = self.editor_hover_link_rects.borrow_mut();
        let mut editor_hover_popup_rect = self.editor_hover_popup_rect.get();
        let mut editor_hover_scrollbar = self.editor_hover_scrollbar.borrow_mut();
        paint_editor_popups(
            backend,
            &screen,
            area,
            &theme,
            &mut completion_layout,
            &mut editor_hover_link_rects,
            &mut editor_hover_popup_rect,
            &mut editor_hover_scrollbar,
        );
        self.editor_hover_popup_rect.set(editor_hover_popup_rect);
    }

    fn handle(
        &mut self,
        event: UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &ShellContext<'_>,
    ) -> Reaction {
        let _ = ctx;

        // ── Panel-key accelerators (mirrors `mod.rs:1259`-`:1273`) ──────────
        // Mouse-affecting accelerators (none today) would need gap (2)
        // first; the current set (`dispatch_panel_accelerator`) only
        // touches `engine`/`sidebar`, so it's fully portable as-is except
        // for its `terminal: &Terminal<...>` parameter, which every arm
        // uses only for `.size()` — satisfied here by `backend.viewport()`,
        // threading both `width` and `height` through (the
        // `ACC_TERMINAL_TOGGLE_MAX` arm needs both — see
        // `dispatch_panel_accelerator_sizeless`'s doc comment).
        //
        // NOT yet ported here: the "#318" Alt+menu-letter reveal shim
        // (`mod.rs:1275`-`:1294`) that sets `engine.menu_bar_visible = true`
        // on an Alt+<letter> keypress so the same keystroke both reveals
        // and activates the menu bar. It belongs between this block and the
        // MenuSystem intercept below, but depends on the `KeyPressed`
        // routing that's still a `// TODO(#595)` stub further down — so it
        // has no home yet either. Flagging it explicitly here (rather than
        // only in the module doc) so it isn't missed when that TODO is
        // finally implemented.
        if let UiEvent::Accelerator(ref acc_id, acc_mods) = event {
            if self.engine.dialog.is_none() {
                let viewport = backend.viewport();
                let mut needs_redraw = false;
                if dispatch_panel_accelerator_sizeless(
                    acc_id.as_str(),
                    acc_mods,
                    &mut self.engine,
                    &mut self.sidebar,
                    viewport.width as u16,
                    viewport.height as u16,
                    self.sidebar_width,
                    &mut needs_redraw,
                ) {
                    return if needs_redraw {
                        Reaction::Redraw
                    } else {
                        Reaction::Continue
                    };
                }
            }
        }

        // ── MenuSystem intercept (mirrors `mod.rs:1296`-`:1304`) ────────────
        if self.engine.menu_bar_visible {
            let viewport = backend.viewport();
            let bar_rect = quadraui::Rect::new(0.0, 0.0, viewport.width, 1.0);
            let menu_system = self.engine.menu_system.clone();
            let menu_event = menu_system.borrow_mut().handle(&event, backend, bar_rect);
            match menu_event {
                quadraui::MenuEvent::Activated(id) => {
                    let action = id.as_str().to_string();
                    if action == "open_file_dialog" {
                        self.engine
                            .open_picker(crate::core::engine::PickerSource::Files);
                    } else {
                        let _ = self.engine.dispatch_menu_action(&action);
                    }
                    return Reaction::Redraw;
                }
                quadraui::MenuEvent::StateChanged | quadraui::MenuEvent::Consumed => {
                    return Reaction::Redraw;
                }
                quadraui::MenuEvent::Ignored => {}
            }
        }

        match event {
            // TODO(#595): route through `Engine::handle_key` once the
            // mode/register plumbing that `event_loop`'s key arm wraps it
            // in (dialog/palette/completion/context-menu intercepts, all
            // of which currently live in `mouse.rs`-adjacent code) has a
            // Frame-free home. Left unimplemented rather than guessed.
            UiEvent::KeyPressed { .. } => Reaction::Continue,
            // #602 (gap 2): dispatch through the legacy `mouse::handle_mouse`
            // now that `Backend::drag_and_modal_mut` (quadraui#467) makes its
            // `&mut DragState`/`&mut ModalStack` params reachable through
            // `&mut dyn Backend`. See `Self::handle_mouse_event`.
            UiEvent::MouseDown { .. }
            | UiEvent::MouseUp { .. }
            | UiEvent::MouseMoved { .. }
            | UiEvent::Scroll { .. }
            | UiEvent::DoubleClick { .. } => self.handle_mouse_event(event, backend),
            _ => Reaction::Continue,
        }
    }

    fn tick(&mut self, backend: &mut dyn quadraui::Backend) -> Reaction {
        let mut needs_redraw = false;

        // ── Per-frame viewport sync (mirrors `mod.rs:916`-`:967`) ───────────
        // `event_loop` reads `terminal.size()`; the runner keeps
        // `backend.viewport()` in sync every frame via `begin_frame`, so it
        // is an exact substitute.
        let viewport = backend.viewport();
        let (vw, vh) = (viewport.width as u16, viewport.height as u16);
        {
            let engine = &mut self.engine;
            let qf_rows: u16 = if engine.quickfix_open { 6 } else { 0 };
            let trm_rows: u16 = if engine.terminal_open || engine.bottom_panel_open {
                let target = terminal_target_maximize_rows_tui(engine, vh);
                engine.effective_terminal_panel_rows(target) + 2
            } else {
                0
            };
            let menu_row: u16 = if engine.menu_bar_visible { 1 } else { 0 };
            let dbg_row: u16 = if engine.debug_toolbar_visible { 1 } else { 0 };
            let wm_row: u16 = if !engine.wildmenu_items.is_empty() {
                1
            } else {
                0
            };
            let content_rows =
                vh.saturating_sub(2 + qf_rows + trm_rows + menu_row + dbg_row + wm_row);
            let gutter_approx = 4u16;
            let sb_visible = engine.app_shell.sidebar_visible();
            let sidebar_cols = if sb_visible {
                self.sidebar_width + 1
            } else {
                0
            };
            let ab_w = if engine.settings.autohide_panels && !sb_visible {
                0
            } else {
                ACTIVITY_BAR_WIDTH
            };
            let content_cols = vw.saturating_sub(ab_w + sidebar_cols + gutter_approx);
            let show_breadcrumbs = engine.settings.breadcrumbs && !engine.terminal_maximized;
            let tab_bar_rows: u16 = {
                let has_single_tab = engine.active_group().tabs.len() <= 1;
                if engine.settings.hide_single_tab && has_single_tab {
                    u16::from(show_breadcrumbs)
                } else if show_breadcrumbs {
                    2
                } else {
                    1
                }
            };
            engine.set_viewport_lines(content_rows.saturating_sub(tab_bar_rows).max(1) as usize);
            engine.set_viewport_cols(content_cols.max(1) as usize);
        }

        // ── Idle background work (mirrors `mod.rs:1157`-`:1247`) ───────────
        if let Some(dl) = self.yank_hl_deadline.get() {
            if Instant::now() >= dl {
                self.engine.clear_yank_highlight();
                self.yank_hl_deadline.set(None);
                needs_redraw = true;
            }
        }

        if self.engine.tab_switcher_open {
            if let Some(last) = self.tab_switcher_last_cycle.get() {
                if last.elapsed() >= Duration::from_millis(500) {
                    self.engine.tab_switcher_confirm();
                    self.tab_switcher_last_cycle.set(None);
                    needs_redraw = true;
                }
            }
            return if needs_redraw {
                Reaction::Redraw
            } else {
                Reaction::Continue
            };
        }

        needs_redraw |= self.engine.poll_idle();

        if self.engine.format_save_quit_ready {
            self.engine.format_save_quit_ready = false;
            self.engine.cleanup_all_swaps();
            self.engine.lsp_shutdown();
            save_session(&mut self.engine);
            return Reaction::Exit;
        }

        if self.engine.app_shell.sidebar_visible()
            && self.last_sidebar_refresh.get().elapsed() >= Duration::from_secs(2)
        {
            self.engine.explorer_rebuild_rows();
            if self.engine.active_panel_is(PANEL_GIT) || self.engine.active_panel_is(PANEL_EXPLORER)
            {
                self.engine.sc_refresh();
            }
            self.last_sidebar_refresh.set(Instant::now());
            needs_redraw = true;
        }

        if self.engine.check_settings_reload() {
            needs_redraw = true;
        }

        if let Some(cmd) = self.engine.pending_terminal_command.take() {
            self.engine
                .terminal_run_command(&cmd, vw, self.engine.session.terminal_panel_rows);
            needs_redraw = true;
        }

        if let Some(msg) = self.pending_startup_msg.take() {
            self.engine.message = msg;
            needs_redraw = true;
        }

        if let Some(panel_name) = self.engine.ext_panel_focus_pending.take() {
            self.sidebar.ext_panel_name = Some(panel_name);
            if !self.engine.app_shell.sidebar_visible() {
                self.engine.toggle_sidebar();
            }
            self.sidebar.has_focus = true;
            needs_redraw = true;
        }

        if needs_redraw {
            Reaction::Redraw
        } else {
            Reaction::Continue
        }
    }
}

/// [`dispatch_panel_accelerator`] minus the `terminal: &Terminal<...>`
/// parameter, replaced with plain `screen_w: u16` / `screen_h: u16` —
/// every call site in the original only used `terminal` for `.size()`,
/// which returns both dimensions (`ACC_TERMINAL_TOGGLE_MAX` needs both:
/// `screen_w` for `terminal_cols`, `screen_h` for
/// `terminal_target_maximize_rows_tui`'s `screen_h` parameter — see
/// `mod.rs:225`-`:239`). Kept as a separate wrapper (rather than changing
/// the original's signature) so the still-live `event_loop()` call site is
/// untouched; the next stage that actually deletes `event_loop()` should
/// collapse these back into one function.
#[allow(clippy::too_many_arguments)]
fn dispatch_panel_accelerator_sizeless(
    id: &str,
    mods: quadraui::Modifiers,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    screen_w: u16,
    screen_h: u16,
    sidebar_width: u16,
    needs_redraw: &mut bool,
) -> bool {
    match id {
        ACC_TOGGLE_SIDEBAR => {
            engine.toggle_sidebar();
            if !engine.app_shell.sidebar_visible() {
                sidebar.has_focus = false;
            }
            *needs_redraw = true;
            true
        }
        ACC_FOCUS_EXPLORER => {
            if sidebar.has_focus && engine.explorer_has_focus {
                sidebar.has_focus = false;
                engine.clear_sidebar_focus();
            } else {
                engine.toggle_sidebar_panel(PANEL_EXPLORER);
                sidebar.has_focus = true;
            }
            *needs_redraw = true;
            true
        }
        ACC_FOCUS_SEARCH => {
            if sidebar.has_focus && engine.search_has_focus {
                sidebar.has_focus = false;
                engine.clear_sidebar_focus();
            } else {
                engine.toggle_sidebar_panel(PANEL_SEARCH);
                sidebar.has_focus = true;
            }
            *needs_redraw = true;
            true
        }
        ACC_FUZZY_FINDER => {
            engine.open_picker(crate::core::engine::PickerSource::Files);
            *needs_redraw = true;
            true
        }
        ACC_LIVE_GREP => {
            engine.open_picker(crate::core::engine::PickerSource::Grep);
            *needs_redraw = true;
            true
        }
        ACC_COMMAND_PALETTE => {
            engine.open_picker(crate::core::engine::PickerSource::Commands);
            *needs_redraw = true;
            true
        }
        ACC_OPEN_TERMINAL => {
            if engine.terminal_open && engine.terminal_has_focus {
                engine.close_terminal();
            } else if engine.terminal_open {
                engine.terminal_has_focus = true;
            } else {
                let cols = terminal_panel_cols(engine, screen_w, sidebar_width);
                if engine.terminal_panes.is_empty() {
                    engine.terminal_new_tab(cols, engine.session.terminal_panel_rows);
                } else {
                    engine.open_terminal(cols, engine.session.terminal_panel_rows);
                }
            }
            *needs_redraw = true;
            true
        }
        ACC_TERMINAL_TOGGLE_MAX => {
            let ctx = crate::core::engine::UiEventContext {
                terminal_cols: terminal_panel_cols(engine, screen_w, sidebar_width),
                terminal_max_rows: terminal_target_maximize_rows_tui(engine, screen_h),
            };
            engine.handle_ui_event(
                crate::core::engine::UiEvent::Accelerator(
                    quadraui::AcceleratorId::new(ACC_TERMINAL_TOGGLE_MAX),
                    mods,
                ),
                ctx,
            );
            *needs_redraw = true;
            true
        }
        ACC_ADD_CURSOR => {
            engine.add_cursor_at_next_match();
            *needs_redraw = true;
            true
        }
        ACC_SELECT_ALL_MATCHES => {
            engine.select_all_occurrences();
            *needs_redraw = true;
            true
        }
        ACC_SPLIT_EDITOR_RIGHT => {
            engine.open_editor_group(SplitDirection::Vertical);
            *needs_redraw = true;
            true
        }
        ACC_SPLIT_EDITOR_DOWN => {
            engine.open_editor_group(SplitDirection::Horizontal);
            *needs_redraw = true;
            true
        }
        ACC_NAV_BACK => {
            engine.tab_nav_back();
            *needs_redraw = true;
            true
        }
        ACC_NAV_FORWARD => {
            engine.tab_nav_forward();
            *needs_redraw = true;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    //! `TuiDriver`/`driver_with_shell` (quadraui's headless `ShellApp`
    //! harness) wraps the app in a `pub(crate)`-fielded `ShellAdapter` with
    //! no accessor back to the concrete `TuiShellApp` and no exposed
    //! `tick()` passthrough — so `setup`/`tick` are exercised directly here
    //! against a real `TuiBackend`, which is exactly what the live runner
    //! does under the hood. `driver_with_shell` is still used for the one
    //! true end-to-end smoke: does the whole `ShellConfig` wiring construct
    //! and paint a first frame without panicking.
    use super::*;
    use quadraui::tui::testing::driver_with_shell;
    use quadraui::Backend as _;

    fn config() -> quadraui::ShellConfig {
        quadraui::ShellConfig::new(
            "VimCode",
            vec![quadraui::PanelDefinition {
                id: quadraui::WidgetId::new("panel:explorer"),
                title: "Explorer".to_string(),
                icon: String::new(),
                tooltip: String::new(),
            }],
        )
    }

    fn backend_at(width: f32, height: f32) -> super::super::backend::TuiBackend {
        let mut backend = super::super::backend::TuiBackend::new();
        backend.begin_frame(quadraui::Viewport::new(width, height, 1.0));
        backend
    }

    /// `TuiShellApp::new` must not panic and must produce a usable `Engine`
    /// — the minimal "does the struct/field mapping even construct" smoke
    /// test for this stage.
    #[test]
    fn constructs_without_panicking() {
        let app = TuiShellApp::new(None);
        assert!(!app.engine.settings.colorscheme.is_empty());
    }

    /// `setup()` must register the panel-key accelerators and populate the
    /// menu system — the two pieces of state `handle()` depends on.
    #[test]
    fn setup_registers_menus_and_accelerators() {
        let mut app = TuiShellApp::new(None);
        let mut backend = backend_at(80.0, 24.0);
        app.setup(&mut backend);
        assert!(!app.engine.menu_system.borrow().menu_bar().items.is_empty());
    }

    /// `tick()` keeps the engine's viewport in sync with the backend's
    /// reported size (the `event_loop` behavior this stage moved verbatim).
    #[test]
    fn tick_syncs_viewport_from_backend_size() {
        let mut app = TuiShellApp::new(None);
        let mut backend = backend_at(100.0, 40.0);
        app.setup(&mut backend);
        app.tick(&mut backend);
        assert!(app.engine.viewport_cols() > 0);
        assert!(app.engine.viewport_lines() > 0);
    }

    /// End-to-end smoke: the `ShellConfig`/`PanelDefinition` wiring this
    /// stage introduced constructs through the real `driver_with_shell`
    /// harness and paints a first frame without panicking.
    #[test]
    fn shell_app_constructs_via_driver_with_shell() {
        let driver = driver_with_shell(TuiShellApp::new(None), config(), 80, 24);
        let _ = driver.screen();
    }

    /// #601: `render_content` must actually paint the active editor
    /// window's text through the `ShellApp` path — this is the core claim
    /// of the stage, so assert on it directly rather than just "didn't
    /// panic". `driver_with_shell` (via `TuiDriver::new`) runs `setup` +
    /// one `render()` pass immediately, so inserting the marker text into
    /// the engine *before* constructing the driver is enough for it to
    /// show up in the first painted frame.
    #[test]
    fn render_content_paints_editor_text_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .buffer_mut()
            .insert(0, "ZQXW_STAGE2_EDITOR_MARKER");
        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW_STAGE2_EDITOR_MARKER"),
            "editor content should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #601: `render_content` must also paint per-editor-group tab bars —
    /// exercise the multi-window code path (`render_all_windows` +
    /// `render::tab_bar_draw_targets` for `screen.editor_group_split`) by
    /// opening a vertical split before painting. Vim/vimcode splits show
    /// the *same* buffer in both panes, so the marker text (on line 0,
    /// visible from the top in a freshly split window) should appear once
    /// per pane — proving both windows actually painted, not just the
    /// first.
    #[test]
    fn render_content_paints_multiple_windows_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .buffer_mut()
            .insert(0, "ZQXW_STAGE2_SPLIT_MARKER");
        app.engine.open_editor_group(SplitDirection::Vertical);
        let driver = driver_with_shell(app, config(), 120, 24);
        let screen = driver.screen();
        let occurrences = screen.matches("ZQXW_STAGE2_SPLIT_MARKER").count();
        assert_eq!(
            occurrences, 2,
            "expected the marker text to paint once per split pane; screen:\n{screen}"
        );
    }

    /// `dispatch_panel_accelerator_sizeless`'s `ACC_TERMINAL_TOGGLE_MAX` arm
    /// must derive `terminal_max_rows` from `screen_h` (the terminal's row
    /// count), not `screen_w` — the bug review iteration 1 of vimcode#595
    /// caught: the wrapper silently fed `screen_w` into
    /// `terminal_target_maximize_rows_tui`, whose parameter is documented
    /// `screen_h`. Uses a screen far wider than it is tall so swapping the
    /// two arguments would produce a visibly different (larger) row count,
    /// making the regression this guards against actually detectable.
    #[test]
    fn terminal_toggle_max_uses_screen_height_not_width() {
        let mut engine = Engine::new();
        let mut sidebar = TuiSidebar::new();
        let mut needs_redraw = false;

        let screen_w: u16 = 200;
        let screen_h: u16 = 24;

        // `terminal_target_maximize_rows_tui` only returns a nonzero target
        // once the terminal panel is considered open (`bp_open` in
        // `compute_editor_layout`). In `dispatch_panel_accelerator_sizeless`,
        // the `ACC_TERMINAL_TOGGLE_MAX` arm builds `ctx` (which is where the
        // screen_w/screen_h bug lives) *before* calling
        // `Engine::handle_ui_event` — and it's that call that flips
        // `terminal_open` via `toggle_terminal_maximize`. So the bug is only
        // observable when the terminal panel is *already* open and just not
        // yet maximized (e.g. the user has a terminal open and presses
        // "maximize") — pre-seed that precondition so `bp_open` is already
        // true when `ctx` is built, matching the live-usage scenario this
        // regression test guards.
        engine.terminal_open = true;
        let expected_target = terminal_target_maximize_rows_tui(&engine, screen_h);
        // Sanity check: if width and height produced the same target, this
        // test couldn't distinguish the bug (screen_w used) from the fix
        // (screen_h used).
        assert_ne!(
            expected_target,
            terminal_target_maximize_rows_tui(&engine, screen_w),
            "fixture must pick w/h whose targets diverge, or this test proves nothing"
        );

        let dispatched = dispatch_panel_accelerator_sizeless(
            ACC_TERMINAL_TOGGLE_MAX,
            quadraui::Modifiers::default(),
            &mut engine,
            &mut sidebar,
            screen_w,
            screen_h,
            SIDEBAR_WIDTH,
            &mut needs_redraw,
        );

        assert!(dispatched);
        assert!(needs_redraw);
        assert!(engine.terminal_maximized);
        assert_eq!(engine.terminal_panes.len(), 1);
        let expected_rows = engine.effective_terminal_panel_rows(expected_target);
        assert_eq!(
            engine.terminal_panes[0].session.rows(),
            expected_rows,
            "spawned terminal's row count must derive from screen_h, not screen_w"
        );
    }

    // ── #602: mouse dispatch through `handle()` ─────────────────────────

    /// #602 core claim: `handle()` must actually route mouse events through
    /// `mouse::handle_mouse` (via `Self::handle_mouse_event`) instead of the
    /// prior blanket `Reaction::Continue` stub. Exercises the sidebar-resize
    /// drag end-to-end — `MouseDown` on the separator column arms
    /// `dragging_sidebar`, a subsequent left-button `MouseMoved` resizes
    /// `sidebar_width` to the new column (mirrors `mouse.rs`'s own
    /// `sep_col`/`new_w` logic verbatim), and `MouseUp` clears the drag —
    /// exactly like `event_loop`'s live mouse handling. Calls
    /// `handle_mouse_event` directly (a private method, reachable from this
    /// `mod tests` since it's nested inside `shell_app.rs`) rather than via
    /// `driver_with_shell`, so the drag state (`dragging_sidebar`,
    /// `sidebar_width`) can be asserted on directly — `driver_with_shell`'s
    /// `ShellAdapter` wrapper has no accessor back to the concrete
    /// `TuiShellApp` (see this module's `driver_with_shell` doc comment).
    #[test]
    fn mouse_drag_resizes_sidebar_via_handle_mouse_event() {
        let mut app = TuiShellApp::new(None);
        if !app.engine.app_shell.sidebar_visible() {
            app.engine.toggle_sidebar();
        }
        assert!(app.engine.app_shell.sidebar_visible());
        assert_eq!(app.sidebar_width, SIDEBAR_WIDTH);

        let mut backend = backend_at(120.0, 40.0);

        // MouseDown on the sidebar separator (activity bar width + sidebar
        // width) arms the drag — mirrors `mouse.rs`'s `sep_col`.
        let sep_col = (ACTIVITY_BAR_WIDTH + SIDEBAR_WIDTH) as f32;
        let reaction = app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(sep_col, 5.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(app.dragging_sidebar);
        assert_eq!(
            app.sidebar_width, SIDEBAR_WIDTH,
            "down alone must not resize yet"
        );

        // Drag with the left button held resizes to the new column.
        let new_col = 70.0_f32;
        let reaction = app.handle_mouse_event(
            UiEvent::MouseMoved {
                position: quadraui::Point::new(new_col, 5.0),
                buttons: quadraui::ButtonMask {
                    left: true,
                    ..quadraui::ButtonMask::default()
                },
            },
            &mut backend,
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(app.dragging_sidebar, "still dragging mid-move");
        assert_eq!(
            app.sidebar_width,
            new_col as u16 - ACTIVITY_BAR_WIDTH,
            "sidebar_width must track the drag column, matching mouse.rs's new_w math"
        );

        // MouseUp ends the drag.
        let reaction = app.handle_mouse_event(
            UiEvent::MouseUp {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(new_col, 5.0),
            },
            &mut backend,
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(!app.dragging_sidebar, "mouse-up must clear the drag");
    }

    /// #602 acceptance: mouse events must dispatch through `ShellApp::handle`
    /// via the real `driver_with_shell` production pipeline (backend
    /// translation → `ShellAdapter::handle` → `TuiShellApp::handle`), not
    /// just when `handle_mouse_event` is called directly. A click deep in
    /// the main content area (past the activity bar, sidebar hidden) isn't
    /// consumed by any `AppShell` chrome, so it reaches `TuiShellApp::handle`
    /// and, before this stage, would have hit the blanket
    /// `_ => Reaction::Continue` stub. Asserting `Redraw` proves the click
    /// actually ran through `mouse::handle_mouse` instead.
    #[test]
    fn driver_with_shell_click_dispatches_through_shell_app_handle() {
        let mut driver = driver_with_shell(TuiShellApp::new(None), config(), 80, 24);
        let reaction = driver.click(40.0, 10.0);
        assert_eq!(
            reaction,
            Reaction::Redraw,
            "a mouse click in the main content area must reach TuiShellApp::handle's \
             mouse arm (via ShellAdapter) and be dispatched through handle_mouse, not \
             fall through to the pre-#602 Reaction::Continue stub"
        );
    }

    /// #602: a full down → move → up drag sequence through `driver_with_shell`
    /// must not panic and must keep reporting `Redraw` — the acceptance
    /// criterion's "drag-select" case (text/scrollbar drags route through
    /// `mouse::handle_mouse` the same way the sidebar-resize drag above
    /// does).
    #[test]
    fn driver_with_shell_drag_sequence_does_not_panic() {
        let mut driver = driver_with_shell(TuiShellApp::new(None), config(), 80, 24);
        let reaction = driver.drag(40.0, 10.0, 45.0, 12.0);
        assert_eq!(reaction, Reaction::Redraw);
    }

    // ── Review fix (#602 iteration 1): the four panel intercepts ported
    // into `handle_mouse_event` ─────────────────────────────────────────
    //
    // `mouse::handle_mouse` carries its own, independent, column/row-derived
    // sidebar-click handling for every one of these panels (`mouse.rs`
    // ~2341 on: gated on `sidebar_visible() && col < ab_width +
    // sidebar_width`, i.e. `col < 33` given `ACTIVITY_BAR_WIDTH` (3) +
    // `SIDEBAR_WIDTH` (30)) — it sets the *same* `sidebar.has_focus` /
    // `*_has_focus` fields these new intercepts do, as a leftover
    // compatibility path. A click inside that column range can't tell you
    // which code path actually ran: an earlier version of these tests
    // placed the intercept rects at x=0..20 and asserted on those flags,
    // and they kept passing even with all four new intercepts stubbed out
    // (verified by temporarily forcing each `if` condition to `false` and
    // re-running — legacy `handle_mouse` alone satisfied every assertion).
    // Every rect below is instead placed at x=50, past legacy's `col < 33`
    // gate, so `mouse::handle_mouse` cannot reach its own sidebar-click
    // arms for these positions at all — leaving the new `SidebarSystem`/
    // `TreeController` intercepts as the *only* code that can produce the
    // observed focus-flag side effects.

    /// Puts `panel_id` in front and visible without accidentally toggling it
    /// *off* — `Engine::toggle_sidebar_panel` hides the sidebar if the
    /// requested panel is already the active + visible one, which would
    /// silently defeat these tests if called unconditionally.
    fn ensure_panel_active(engine: &mut Engine, panel_id: &str) {
        if !(engine.active_panel_is(panel_id) && engine.app_shell.sidebar_visible()) {
            engine.toggle_sidebar_panel(panel_id);
        }
        assert!(engine.active_panel_is(panel_id));
        assert!(engine.app_shell.sidebar_visible());
    }

    /// Review fix: `handle_mouse_event` must run the debug-sidebar
    /// `SidebarSystem` intercept (mirrors `event_loop`'s `mod.rs` ~1436-1471)
    /// before falling through to `mouse::handle_mouse` — the reviewer's
    /// blocking finding on #602 iteration 1 noted no test exercised this.
    /// A `MouseDown` inside `dap_sidebar_body_rect` while the debug panel is
    /// active must claim sidebar focus, proving the intercept branch itself
    /// ran (see the module-comment above for why the rect sits at x=50,
    /// outside `mouse::handle_mouse`'s own sidebar column range).
    #[test]
    fn debug_sidebar_intercept_claims_focus_on_mouse_down() {
        let mut app = TuiShellApp::new(None);
        ensure_panel_active(&mut app.engine, PANEL_DEBUG);
        app.engine
            .dap_sidebar_body_rect
            .set(quadraui::Rect::new(50.0, 1.0, 20.0, 10.0));

        let mut backend = backend_at(80.0, 24.0);
        let reaction = app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(55.0, 3.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            app.sidebar.has_focus && app.engine.dap_sidebar_has_focus,
            "a click inside dap_sidebar_body_rect (outside handle_mouse's own \
             sidebar column range) must be claimed by the debug SidebarSystem \
             intercept, not silently dropped"
        );
    }

    /// Review fix: same as above for the extensions-sidebar intercept
    /// (mirrors `mod.rs` ~1473-1498) — `mouse::handle_mouse`'s own
    /// `PANEL_EXTENSIONS` arm explicitly declines rows 2+ ("handled by
    /// SidebarSystem mouse intercept in main loop", `mouse.rs` ~2557), so a
    /// missing intercept here would silently drop those clicks.
    #[test]
    fn ext_sidebar_intercept_claims_focus_on_mouse_down() {
        let mut app = TuiShellApp::new(None);
        ensure_panel_active(&mut app.engine, PANEL_EXTENSIONS);
        app.engine
            .ext_sidebar_body_rect
            .set(quadraui::Rect::new(50.0, 1.0, 20.0, 10.0));

        let mut backend = backend_at(80.0, 24.0);
        let reaction = app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(55.0, 3.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            app.sidebar.has_focus && app.engine.ext_sidebar_has_focus,
            "a click inside ext_sidebar_body_rect (outside handle_mouse's own \
             sidebar column range) must be claimed by the extensions \
             SidebarSystem intercept, not silently dropped"
        );
    }

    /// Review fix: the explorer → `TreeController` intercept (mirrors
    /// `mod.rs` ~1545-1605) must claim a `MouseDown` inside
    /// `explorer_tree_rect` when the explorer panel is active — the
    /// reviewer flagged that `TuiShellApp` already carried the matching
    /// `explorer_sb_dragging` field (Stage 0) but nothing read/wrote it,
    /// "a strong signal this block was meant to be ported but wasn't".
    #[test]
    fn explorer_tree_intercept_claims_focus_on_mouse_down() {
        let mut app = TuiShellApp::new(None);
        ensure_panel_active(&mut app.engine, PANEL_EXPLORER);
        app.engine
            .explorer_tree_rect
            .set(quadraui::Rect::new(50.0, 1.0, 20.0, 10.0));

        let mut backend = backend_at(80.0, 24.0);
        let reaction = app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(55.0, 3.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            app.engine.explorer_has_focus && app.sidebar.has_focus,
            "a click inside explorer_tree_rect (outside handle_mouse's own \
             sidebar column range) must be claimed by the explorer \
             TreeController intercept, not silently dropped"
        );
    }

    /// Review fix / #459: a modal that hit-tests positive at the click
    /// position must make every panel intercept above yield — mirrors
    /// `event_loop`'s `ctx_blocks_event` check (`mod.rs` ~1416-1433, the
    /// `// #459: Hit-test the modal stack` block). Without it, an
    /// editor-hover popup or picker drawn over the explorer tree couldn't be
    /// clicked; the click would be swallowed by the tree intercept
    /// underneath instead. Reuses the x=50 rect placement from the tests
    /// above so `mouse::handle_mouse`'s own sidebar-click handling — which
    /// has no notion of the quadraui `ModalStack` at all — cannot itself
    /// explain `sidebar.has_focus` staying `false`; only correct
    /// `modal_blocks_event` gating can.
    #[test]
    fn modal_blocks_event_skips_explorer_intercept_when_modal_covers_click() {
        let mut app = TuiShellApp::new(None);
        ensure_panel_active(&mut app.engine, PANEL_EXPLORER);
        app.engine
            .explorer_tree_rect
            .set(quadraui::Rect::new(50.0, 1.0, 20.0, 10.0));

        let mut backend = backend_at(80.0, 24.0);
        {
            let (_, modal_stack) = backend.drag_and_modal_mut();
            modal_stack.push(
                quadraui::WidgetId::new("test:modal"),
                quadraui::Rect::new(50.0, 1.0, 20.0, 10.0),
            );
        }

        app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(55.0, 3.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );

        assert!(
            !app.sidebar.has_focus,
            "a modal covering the click position must block the explorer \
             TreeController intercept from claiming the event (#459)"
        );
    }

    /// Review fix (#602 iteration 2) / #456: the companion to the test
    /// above, for the gate that actually fires in practice.
    ///
    /// An *open explorer context menu* must block the `TreeController`
    /// intercept even though nothing is on the `ModalStack`. In
    /// `event_loop` the modal stack would carry the menu, because
    /// `mouse.rs`'s `#459` reconcile pushes `context_menu_layout.bounds`
    /// and `event_loop` feeds it a real layout from its raw-`Frame` paint.
    /// `TuiShellApp` has no such paint yet (module-doc gap 1 / #607), so
    /// `self.context_menu_layout` is always `None`, the reconcile always
    /// *pops*, and `modal_blocks_event` is permanently `false` for a
    /// context menu — hence the separate `engine.context_menu` gate.
    ///
    /// This test deliberately pushes **nothing** onto the modal stack, so
    /// it fails against a build gated on `modal_blocks_event` alone. As
    /// with the tests above, the tree rect sits at x=50 — outside
    /// `mouse::handle_mouse`'s own `col < 33` legacy sidebar range — so a
    /// passing assertion can only be explained by the new gate.
    #[test]
    fn open_context_menu_skips_explorer_intercept_without_modal_stack_entry() {
        let mut app = TuiShellApp::new(None);
        ensure_panel_active(&mut app.engine, PANEL_EXPLORER);
        app.engine
            .explorer_tree_rect
            .set(quadraui::Rect::new(50.0, 1.0, 20.0, 10.0));

        // Right-click outcome: an explorer context menu is open, floating
        // over the tree. Nothing is registered on the ModalStack.
        app.engine.open_explorer_context_menu(
            std::path::PathBuf::from("/tmp/zqxw.txt"),
            false,
            55,
            3,
        );
        assert!(
            app.engine.context_menu.is_some(),
            "test precondition: context menu must be open"
        );

        let mut backend = backend_at(80.0, 24.0);
        {
            let (_, modal_stack) = backend.drag_and_modal_mut();
            assert!(
                modal_stack
                    .hit_test(quadraui::Point::new(55.0, 3.0))
                    .is_none(),
                "test precondition: nothing may be on the modal stack, \
                 otherwise this would re-test modal_blocks_event"
            );
        }

        app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(55.0, 3.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );

        assert!(
            !app.engine.explorer_has_focus && !app.sidebar.has_focus,
            "an open explorer context menu must block the TreeController \
             intercept so the click reaches mouse.rs's ctx-menu confirm \
             instead of being consumed as a tree row activation (#456)"
        );
    }
}
