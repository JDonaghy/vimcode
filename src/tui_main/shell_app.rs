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
//! ported — with one residual seam noted inline below. Gap 3 (cursor
//! placement) closed by #604. Gap 1 (painting) remains open:
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
//!    `paint_editor_popups`. #607 (Stage 2a) additionally wires the
//!    trait-pure subset of sidebar panel content — explorer (the default
//!    panel), search, debug — into `layout.sidebar_content_bounds`, via
//!    `panels::render_sidebar_content`; see that function's doc comment for
//!    the per-panel audit. What's still unpainted (true raw-buffer
//!    holdouts, each filed as its own follow-on, all blocking #605
//!    cutover):
//!
//!    - The rest of the sidebar (#607's own known-gap list, no separate
//!      issue): **settings** (`render_settings_panel`'s header + search-box
//!      chrome is a free `quadraui::tui::draw_settings_chrome` rasteriser
//!      with no `Backend::draw_*` trait equivalent — its own "Stage 1 scope
//!      note" doc comment already flags this; the form body below the
//!      chrome *is* trait-pure via `FormController::render_and_cache`, but
//!      painting only the body and leaving the chrome blank was judged not
//!      worth the coordinate-mismatch risk for this stage), **source
//!      control** (header row, focused-hint row, and full-area background
//!      clear are raw `set_cell` loops over `frame.buffer_mut()` — the
//!      `draw_status_bar`-blank-segment trick #607 used for the explorer's
//!      background would work here too, just not attempted this stage),
//!      **extensions** (`render_ext_sidebar`'s two chrome rows are the same
//!      raw-`set_cell` pattern; likewise a `draw_status_bar` candidate),
//!      the **plugin extension panel** (`render_ext_panel`'s chrome is the
//!      same non-trait `draw_settings_chrome` as settings, *and* its help
//!      popup overlay and manual scrollbar are raw `set_cell` box-drawing
//!      with no primitive stand-in checked yet), and the **AI panel**
//!      (`render_ai_sidebar` takes `buf: &mut ratatui::buffer::Buffer`
//!      directly — no backend parameter at all, the most raw of the
//!      lot).
//!
//!    #609 (Stage 2c, closed) additionally wires the window/editor-group
//!    divider lines, the tab-drag ghost overlay, and the tab-hover tooltip
//!    into `render_content`. All three were raw `Buffer`/`Frame` writes in
//!    `draw_frame` with no `Backend::draw_*` trait equivalent at the time
//!    #601 scoped them out — #609 found none was actually needed: a
//!    1-cell-wide (or 1-row) `StatusBar` with a single solid/text segment,
//!    painted through `Backend::draw_status_bar`, reproduces a raw
//!    `set_cell` write exactly (`render_impl.rs::draw_rule_cell`/
//!    `draw_rule_row`) — the same trick quadraui's own
//!    `compose::app_shell::AppShell::render` already uses for its generic
//!    sidebar-resize divider, confirming the issue's hunch that no new
//!    primitive was required. `render_separators` (within-group
//!    `:split`/`:vsplit` dividers) and the group-level divider loop
//!    (`split.dividers`, between `Ctrl+W v`/`Ctrl+W s` groups) both moved
//!    to this trick and now run unconditionally in `render_all_windows`/
//!    `draw_frame` — including on the *live* `draw_frame` path, which used
//!    to read back `frame.buffer_mut()[(div_x - 1, y)]`'s painted symbol
//!    to avoid a phantom double-divider beside an overflowing window's own
//!    scrollbar (#481); that read-back is now `group_divider_cells`, a
//!    pure data computation over `RenderedWindow` geometry shared by both
//!    callers — see its doc comment. The tab-drag overlay reads
//!    `TuiShellApp`'s `tui_drag_source`/`tui_drag_cursor`/
//!    `tui_tab_drop_zone` fields, which #602 already wires
//!    `handle_mouse_event` to populate, so no further sequencing with #602
//!    was needed, just the paint call. The tab-hover tooltip sources
//!    straight from `engine.tab_hover_tooltip`, a plain field with no
//!    per-frame state of its own. Covered by
//!    `render_content_paints_group_divider_via_shell_app` (`driver_with_shell`,
//!    the required headless case per #609's acceptance bar),
//!    `render_content_paints_tab_drag_ghost_via_shell_app`, and
//!    `render_content_paints_tab_hover_tooltip_via_shell_app` below.
//!
//!    #608 (Stage 2b, closed) additionally wires the quickfix panel and the
//!    bottom panel (terminal/debug output) into `render_content`, via
//!    `render_impl.rs::bottom_chrome_rects_for_shell_content` (carves their
//!    rects out of `layout.main_content_bounds` by hand, mirroring
//!    `draw_frame`'s own `v_chunks` split — `AppShellLayout` has no concept
//!    of either row, and quadraui's own generic `ShellConfig
//!    ::with_bottom_panel` / `BottomPanelController`
//!    (`shell_adapter.rs`/`compose/app_shell.rs`) model a single resizable
//!    drawer, not vimcode's stack of independently-toggleable
//!    quickfix/terminal/debug-toolbar/wildmenu rows, and isn't wired for
//!    `TuiShellApp` regardless — see `bottom_chrome_rects_for_shell_content`'s
//!    doc comment for the full investigation) + `panels::render_quickfix_panel`
//!    / `panels::render_bottom_panel_tabs` / `panels::render_terminal_toolbar`
//!    (all three widened to `&mut dyn quadraui::Backend`, same treatment
//!    #607 gave `render_search_panel`/`render_debug_sidebar` — they were
//!    already trait-pure) + a new `panels::render_terminal_panel_content`
//!    (trait-pure counterpart to `render_terminal_panel`'s raw-`Frame`
//!    background-clear loop, using the same `draw_status_bar`-blank-segment
//!    trick #607 introduced) for the Terminal tab, or
//!    `Backend::draw_text_display` (already trait-pure) for the Debug
//!    Output tab. Known gap: **split terminal panes** (`Ctrl+\`,
//!    `panel.split_left_rows.is_some()`) are NOT painted —
//!    `render_terminal_panel`'s split arm draws the divider via
//!    `quadraui::tui::draw_terminal_divider`, a free rasteriser with no
//!    `Backend::draw_*` trait equivalent (same class of gap as
//!    `draw_settings_chrome`), so a correctly-divided split can't be
//!    painted from this signature; `render_terminal_panel_content` clears
//!    the background and leaves it otherwise blank rather than drawing an
//!    undivided pane that would misrepresent the split state. See
//!    `panels::render_terminal_panel_content`'s own doc comment.
//!
//!    The menu bar row is reserved in the layout math but not painted
//!    either (out of scope for #601; folds into key dispatch, #603).
//!    Cursor placement used to be a raw-buffer holdout in this list (it
//!    needs `Frame::set_cursor_position`, and `render_content` has no
//!    `Frame`) but #604 closed it a different way — see gap 3 below, now
//!    resolved.
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
//! 3. **Editor cursor placement (#604, closed).** `Backend::draw_editor`'s
//!    `EditorPaintResult::cursor_position` is documented "host applies via
//!    `Frame::set_cursor_position`", but no consumer of it existed anywhere
//!    in quadraui's `shell_adapter`/TUI runner — `render_content` has no
//!    `Frame` to call it on, and that's still true. quadraui#466 closed the
//!    gap without needing one: `TuiBackend::draw_editor` now caches
//!    `cursor_position` on itself unconditionally (regardless of whether
//!    the caller had a `Frame`), and `tui/run.rs::render_frame` — the fn
//!    `ShellAdapter::render` runs inside, shared by the live runner,
//!    `run_with_shell`, and `TuiDriver` — takes the cached value and calls
//!    `Frame::set_cursor_position` on the *real* `Frame` after
//!    `render_content` returns, the same way `apply_selection_highlight`
//!    already runs post-`render_content`. So `render_window`'s local
//!    `frame: None` (still passed everywhere on this path, per gap 1) no
//!    longer means "no cursor" — it only ever meant "can't paint dividers
//!    and can't call `set_cursor_position` *from inside `render_content`*",
//!    and the latter is now moot since `render_frame` does it one layer up.
//!    Verified end-to-end (typed insert-mode `Bar` cursor reaching
//!    `TestBackend`'s terminal cursor) by
//!    `insert_mode_bar_cursor_reaches_terminal_frame_via_shell_app` below.
//!
//! Given (2) is now closed by #602, (3) is now closed by #604, and (1)
//! remains the only open structural gap, `handle()` below implements every
//! dispatch layer that doesn't need raw Frame access: panel-key
//! accelerators, the "#318"
//! Alt+menu-letter "reveal menu bar" shim (mirrors `mod.rs:1319`-`:1338` —
//! sets `engine.menu_bar_visible = true` on an Alt+<letter> keypress so the
//! same keystroke both reveals and activates the menu), the `MenuSystem`
//! intercept, full mouse dispatch through `handle_mouse_event` (#602), and
//! — #603 (Stage 4) — the `KeyPressed` dispatch chain (modal dialog /
//! folder-picker-modal / context-menu intercepts, then the general
//! `Engine::handle_key` fallback that also resolves command-palette and
//! completion-popup state internally). This is *not* the full
//! `mod.rs:1629`-`:2737` precedence chain: activity-bar-focused,
//! sidebar-focused, and command-output-selection (`cmd_sel`) keyboard tiers
//! are still unported, since their gating focus state is set almost
//! entirely by `mouse::handle_mouse`. See `handle_key_pressed`'s own doc
//! comment for the exact precedence chain and this gap's full detail, and
//! for why it's a free function rather than a `TuiShellApp` method.

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
    /// Mirrors `event_loop`'s once-computed `keyboard_enhanced` flag
    /// (`mod.rs:696`, from `supports_keyboard_enhancement()` before the
    /// loop starts) — threaded into `translate_key` to disambiguate a
    /// handful of Ctrl-combo escape sequences (Ctrl+\, Ctrl+/,
    /// Ctrl+Shift+[/]) that arrive ambiguously without the kitty keyboard
    /// protocol. This dormant scaffold has no live terminal session to
    /// query yet — only `driver_with_shell`'s `TestBackend`, where
    /// querying would be meaningless and `supports_keyboard_enhancement()`'s
    /// real terminal round-trip could misbehave without a TTY — so this
    /// defaults to `false`, the same value `unwrap_or(false)` falls back to
    /// on any terminal that doesn't support the protocol. Stage 6 cutover
    /// (#605) should thread the real value in from wherever
    /// `run_with_shell` ends up being called.
    keyboard_enhanced: bool,
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
            keyboard_enhanced: false,
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
        // (quickfix/bottom panel #608, dividers/drag-overlay/tab-tooltip
        // #609, cursor placement #604) and why.
        let theme = self.theme();

        // ── Sidebar panel content (#607) ─────────────────────────────────
        // `AppShell::render` (quadraui, called by the runner before
        // `render_content`) already painted the generic sidebar chrome
        // (activity bar + header) — this paints the *active panel's* body
        // into `layout.sidebar_content_bounds`. Independent of
        // `main_content_bounds` below (distinct screen regions), so it
        // isn't gated on that guard. See `panels::render_sidebar_content`'s
        // doc comment for exactly which panels are ported (explorer —
        // the default panel, search, debug) vs. still a documented,
        // deferred gap for this stage (settings, source control,
        // extensions, AI, the plugin extension panel).
        if let Some(sb) = layout.sidebar_content_bounds {
            if sb.width >= 1.0 && sb.height >= 1.0 {
                let sb_area = Rect {
                    x: sb.x.round() as u16,
                    y: sb.y.round() as u16,
                    width: sb.width.round() as u16,
                    height: sb.height.round() as u16,
                };
                render_sidebar_content(backend, sb_area, &self.sidebar, &self.engine, &theme);
            }
        }

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
        // an adjacent group's tab bar. `render_all_windows` also paints the
        // within-group (`:split`/`:vsplit`) divider lines unconditionally
        // now (#609 routed `render_separators` through
        // `Backend::draw_status_bar` — see its doc comment — so it no
        // longer needs the raw `Frame` that `frame: None` used to skip it
        // for).
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

        // ── Group divider lines (#609) ───────────────────────────────────
        // Between-*group* dividers (`Ctrl+W v`/`Ctrl+W s`, as opposed to
        // `render_all_windows`'s within-group `render_separators` above) —
        // only present when the editor is split into multiple groups.
        // Mirrors `draw_frame`'s own divider block, ported to
        // `Backend::draw_status_bar` via `render_group_dividers` (see its
        // doc comment, and `group_divider_cells`'s for how the #481
        // phantom-divider-beside-scrollbar guard became a pure data
        // computation instead of a `Buffer` read-back).
        if let Some(ref split) = screen.editor_group_split {
            render_group_dividers(backend, &split.dividers, &screen.windows, area, &theme);
        }

        // ── Tab-drag ghost overlay (#609) ────────────────────────────────
        // Drag state (`tui_drag_source`/`tui_drag_cursor`/
        // `tui_tab_drop_zone`) is already live here — #602 wired
        // `handle_mouse_event` to mutate these three fields via
        // `mouse::handle_mouse` (see `handle()` below) — so painting from
        // it needs no further sequencing with #602, just the paint call
        // itself, which is this issue's scope.
        if self.tui_drag_source.is_some() {
            render_tab_drag_overlay(
                backend,
                &self.engine,
                area,
                &screen,
                &theme,
                self.tui_drag_source,
                self.tui_drag_cursor,
                &self.tui_tab_drop_zone,
            );
        }

        // ── Tab-hover tooltip (#609) ─────────────────────────────────────
        // Mirrors `draw_frame`'s own tooltip block, ported to
        // `Backend::draw_status_bar` via `render_tab_hover_tooltip`. Unlike
        // `draw_frame`'s `editor_area` (whose `y` is implicitly 0-based —
        // it's the live terminal frame's own top-level split), `area` here
        // is `layout.main_content_bounds`, already offset below whatever
        // `AppShell::render` painted above it — see that function's doc
        // comment for why the row math differs between the two callers.
        if let Some(ref tooltip_text) = screen.tab_tooltip {
            let menu_height: u16 = if self.engine.menu_bar_visible { 1 } else { 0 };
            render_tab_hover_tooltip(
                backend,
                area.x,
                area.y + menu_height + 1,
                area.width,
                tooltip_text,
                &theme,
            );
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

        // ── Quickfix panel + bottom panel (terminal/debug output) (#608) ──
        // `AppShellLayout` has no concept of either — see
        // `bottom_chrome_rects_for_shell_content`'s doc comment for why
        // their rects are carved out of `area` (== `main_content_bounds`)
        // by hand, matching `draw_frame`'s own `v_chunks` layout, rather
        // than sourced from `layout.bottom_panel_bounds` (quadraui's
        // generic single-drawer `BottomPanelController`, a different shape
        // than vimcode's stacked chrome, and unwired for `TuiShellApp`
        // regardless — always `None` here).
        let chrome = bottom_chrome_rects_for_shell_content(&self.engine, &screen, area);

        if let Some(ref qf) = screen.quickfix {
            render_quickfix_panel(
                chrome.quickfix,
                qf,
                self.quickfix_scroll_top,
                &theme,
                backend,
            );
        }

        if chrome.bottom_panel.height > 0 {
            self.engine.bottom_panel_geometry.replace(Some(
                crate::core::engine::BottomPanelGeometry {
                    top_y: chrome.bottom_panel.y as f64,
                    height: chrome.bottom_panel.height as f64,
                    toolbar_y: 1.0,
                    content_y: 2.0,
                    content_row_h: 1.0,
                },
            ));
            let tab_bar_area = Rect {
                x: chrome.bottom_panel.x,
                y: chrome.bottom_panel.y,
                width: chrome.bottom_panel.width,
                height: 1,
            };
            let content_area = Rect {
                x: chrome.bottom_panel.x,
                y: chrome.bottom_panel.y + 1,
                width: chrome.bottom_panel.width,
                height: chrome.bottom_panel.height.saturating_sub(1),
            };
            let hits = render_bottom_panel_tabs(
                backend,
                tab_bar_area,
                &self.engine.bottom_panel_kind,
                self.engine.terminal_open,
                !screen.bottom_tabs.output_lines.is_empty(),
                &theme,
            );
            self.engine.bottom_tab_bar_hits.replace(Some(hits));
            match self.engine.bottom_panel_kind {
                render::BottomPanelKind::Terminal => {
                    if let Some(ref term) = screen.bottom_tabs.terminal {
                        let toolbar_area = Rect {
                            x: content_area.x,
                            y: content_area.y,
                            width: content_area.width,
                            height: 1,
                        };
                        let hits = render_terminal_toolbar(backend, toolbar_area, term, &theme);
                        self.engine.terminal_toolbar_hits.replace(Some(hits));
                        let term_content = Rect {
                            x: content_area.x,
                            y: content_area.y + 1,
                            width: content_area.width,
                            height: content_area.height.saturating_sub(1),
                        };
                        render_terminal_panel_content(
                            backend,
                            term_content,
                            term,
                            &theme,
                            &self.engine,
                        );
                        self.engine
                            .scroll_surfaces
                            .borrow_mut()
                            .push(quadraui::ScrollSurface {
                                id: quadraui::WidgetId::new("terminal_scrollback"),
                                bounds: quadraui::Rect::new(
                                    term_content.x as f32,
                                    term_content.y as f32,
                                    term_content.width as f32,
                                    term_content.height as f32,
                                ),
                                scrollbar: None,
                            });
                    }
                }
                render::BottomPanelKind::DebugOutput => {
                    let td = render::debug_output_to_text_display(
                        &screen.bottom_tabs.output_lines,
                        self.engine.debug_output_scroll,
                        self.engine.debug_output_auto_scroll,
                    );
                    let q_rect = quadraui::Rect::new(
                        content_area.x as f32,
                        content_area.y as f32,
                        content_area.width as f32,
                        content_area.height as f32,
                    );
                    let td_layout = backend.text_display_layout(q_rect, &td);
                    backend.draw_text_display(q_rect, &td);
                    let scrollbar = td_layout.scrollbar_bounds.zip(td_layout.thumb_bounds).map(
                        |(track, thumb)| {
                            let offset_y = q_rect.y;
                            quadraui::SurfaceScrollbar {
                                axis: quadraui::ScrollAxis::Vertical,
                                track_bounds: quadraui::Rect::new(
                                    q_rect.x + track.x,
                                    offset_y + track.y,
                                    track.width,
                                    track.height,
                                ),
                                thumb_bounds: quadraui::Rect::new(
                                    q_rect.x + thumb.x,
                                    offset_y + thumb.y,
                                    thumb.width,
                                    thumb.height,
                                ),
                                total_items: td.lines.len(),
                                visible_items: td_layout.visible_lines.len(),
                                scroll_offset: td_layout.resolved_scroll_offset,
                                inverted: false,
                            }
                        },
                    );
                    self.engine
                        .scroll_surfaces
                        .borrow_mut()
                        .push(quadraui::ScrollSurface {
                            id: quadraui::WidgetId::new("debug_output"),
                            bounds: q_rect,
                            scrollbar,
                        });
                }
            }
        } else {
            self.engine.bottom_panel_geometry.replace(None);
        }
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

        // ── #318: Alt+menu-letter "reveal menu bar" shim (mirrors
        // `mod.rs:1319`-`:1338`) ─────────────────────────────────────────
        // When the menu bar is hidden, Alt+<letter> must still activate the
        // corresponding menu — otherwise the bare letter falls through to
        // `Engine::handle_key` (which ignores Alt) and triggers a Vim
        // motion (e.g. Alt+T → t-motion). Setting `menu_bar_visible` here
        // makes the *same* keystroke both reveal and activate the menu via
        // the `MenuSystem` intercept immediately below. Queries the live
        // menu system rather than hardcoding letters so the truth stays in
        // `MENU_STRUCTURE` (render.rs) → `MenuDef`.
        if !self.engine.menu_bar_visible {
            if let UiEvent::KeyPressed { key, modifiers, .. } = &event {
                if modifiers.alt {
                    if let quadraui::Key::Char(c) = key {
                        let bar = self.engine.menu_system.borrow().menu_bar();
                        if bar.find_alt_target(*c).is_some() {
                            self.engine.menu_bar_visible = true;
                        }
                    }
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
            // #603 (Stage 4): dialog / folder-picker / context-menu /
            // general `Engine::handle_key` fallback — see
            // `handle_key_pressed`'s doc comment for the precedence chain
            // and its unported-tiers gap note.
            UiEvent::KeyPressed {
                key,
                modifiers,
                repeat,
            } => {
                let viewport = backend.viewport();
                handle_key_pressed(
                    key,
                    modifiers,
                    repeat,
                    &mut self.engine,
                    &mut self.sidebar,
                    self.sidebar_width,
                    &mut self.folder_picker,
                    self.keyboard_enhanced,
                    viewport.width as u16,
                    viewport.height as u16,
                )
            }
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

/// Route a `KeyPressed` event through `Engine::handle_key`, replicating four
/// of `event_loop()`'s precedence tiers for it (`mod.rs:1629`-`:2737`) —
/// see the "Not ported" note below for the tiers this function deliberately
/// skips:
///
/// 1. **Modal dialog** (`mod.rs:1629`-`:1651`) intercepts *all* keys —
///    checked first and returns unconditionally, exactly like the legacy
///    loop's own early `continue`.
/// 2. **Folder picker modal** (`mod.rs:1653`-`:1708`) — checked next,
///    ahead of context-menu/general-fallback, because it operates purely
///    on `folder_picker: &mut Option<FolderPickerState>` (mirroring how
///    `sidebar`/`context_menu` state is already threaded through) and,
///    like the modal dialog above, must intercept every key once open —
///    type-to-filter, Up/Down/j/k, Enter, Esc, `-`, Backspace — so they
///    never fall through to `Engine::handle_key` and get misinterpreted as
///    Normal/Insert-mode editor input.
/// 3. **Context menu** (`mod.rs:2608`-`:2635`) — checked ahead of
///    `Engine::handle_key`, even though `Engine::handle_key` has its own
///    context-menu branch (`keys.rs:66`-`:71`), because the engine's copy
///    only consumes the key and discards the resulting action. This
///    function's copy dispatches that action to
///    [`handle_explorer_context_action`] (new_file/rename/delete/
///    open_terminal/find_in_folder/…), which needs TUI-local state
///    (`sidebar`, terminal size) `Engine::handle_key` has no access to.
/// 4. **General fallback** (`mod.rs:2637`-`:2737`) — this is also where
///    command-palette (`picker_open`) and completion-popup
///    (`completion_idx`) keys land: `Engine::handle_key` already resolves
///    both internally (see `keys.rs`'s own precedence chain), so this
///    function's job for those two is purely getting the key there,
///    then unpacking the `EngineAction` side effects `Engine::handle_key`
///    can't perform itself because they need backend-supplied terminal
///    size or TUI-local state (`folder_picker`, `sidebar`): open terminal,
///    toggle-maximize, run-in-terminal, folder/workspace/recent dialogs,
///    quit confirmation.
///
/// **Not ported (gap, tracked alongside #602):** `mod.rs:1629`-`:2737` also
/// contains an activity-bar-focused tier (`mod.rs:1711`), a sidebar-focused
/// tier with its own nested context-menu/explorer-key intercept
/// (`mod.rs:1760`+), and command-output-selection (`cmd_sel`) handling —
/// none of which this function replicates. Deferred alongside the
/// already-acknowledged mouse gap (#602) since all three tiers gate on
/// focus/selection state (`activity_bar_focused`, `sidebar.has_focus`,
/// `cmd_sel`) that today is set almost entirely by `mouse::handle_mouse`,
/// which this dormant `handle()` doesn't call yet; a future session should
/// re-check whether keyboard-only focus transitions exist before assuming
/// this is purely a mouse-side gap.
///
/// Translates the backend-neutral `Key`/`Modifiers` into the
/// `(key_name, unicode, ctrl)` shape `Engine::handle_key` expects by
/// reusing the legacy `translate_key` — synthesizing a crossterm
/// `KeyEvent` via quadraui's own `synth_keyevent` first, the same
/// `UiEvent -> crossterm::Event` round trip `event_loop()`'s live loop
/// already performs (`events::uievent_to_crossterm`) — rather than
/// re-deriving `translate_key`'s crossterm quirk table (Ctrl+\, Ctrl+/,
/// kitty shift-symbol resolution, …) a second time for `quadraui::Key`.
///
/// A free function (mirrors [`dispatch_panel_accelerator_sizeless`])
/// rather than a `TuiShellApp` method: `ShellContext` has no public
/// constructor (`pub(crate) fn new`, quadraui-internal), so
/// `TuiShellApp::handle()` itself can only be driven through
/// `driver_with_shell`, which has no accessor back to the concrete app's
/// fields. Structuring the real logic as a free function over borrowed
/// pieces keeps it directly unit-testable against a bare `Engine`.
#[allow(clippy::too_many_arguments)]
fn handle_key_pressed(
    key: quadraui::Key,
    modifiers: quadraui::Modifiers,
    repeat: bool,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    sidebar_width: u16,
    folder_picker: &mut Option<FolderPickerState>,
    keyboard_enhanced: bool,
    screen_w: u16,
    screen_h: u16,
) -> Reaction {
    let Some(key_event) = quadraui::tui::events::synth_keyevent(&key, modifiers, repeat) else {
        return Reaction::Continue;
    };

    // ── Modal dialog intercepts ALL keys (mirrors mod.rs:1629-:1651) ────
    if engine.dialog.is_some() {
        if let Some((key_name, unicode, ctrl)) = translate_key(key_event, keyboard_enhanced) {
            let action = engine.handle_key(&key_name, unicode, ctrl);
            if handle_action(engine, action) {
                return Reaction::Exit;
            }
        } else if key_event.kind != KeyEventKind::Release {
            match key_event.code {
                KeyCode::Tab => {
                    engine.handle_key("Tab", None, false);
                }
                KeyCode::BackTab => {
                    engine.handle_key("Shift_Tab", None, false);
                }
                _ => {}
            }
        }
        return Reaction::Redraw;
    }

    // ── Folder picker modal (mirrors mod.rs:1653-:1708) ─────────────────
    // Checked ahead of the context-menu/general-fallback tiers, exactly
    // like the legacy loop, so that once `EngineAction::OpenFolderDialog`
    // (below) populates `folder_picker`, every subsequent key — type-to-
    // filter, Up/Down/j/k, Enter, Esc, `-`, Backspace — goes to the picker
    // instead of falling through to `Engine::handle_key` and being
    // misinterpreted as Normal/Insert-mode editor input.
    if folder_picker.is_some() && key_event.kind != KeyEventKind::Release {
        let picker_ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
        let picker = folder_picker.as_mut().unwrap();
        match key_event.code {
            KeyCode::Esc => {
                *folder_picker = None;
            }
            KeyCode::Enter => {
                // Check if ".." was selected — navigate up instead of opening.
                let is_dotdot = picker
                    .filtered
                    .get(picker.selected)
                    .map(|p| p.as_os_str() == "..")
                    .unwrap_or(false);
                if is_dotdot {
                    picker.navigate_up();
                } else if let Some(path) = picker.selected_path() {
                    *folder_picker = None;
                    engine.open_folder(&path);
                    *sidebar = TuiSidebar::new();
                    engine.explorer_rebuild_rows();
                    if let Some(path) = engine.file_path().cloned() {
                        engine.explorer_reveal_path(&path);
                    }
                }
            }
            // '-' navigates up to the parent directory (like vim netrw).
            KeyCode::Char('-') if !picker_ctrl => {
                picker.navigate_up();
            }
            KeyCode::Up | KeyCode::Char('k') if !picker_ctrl => {
                picker.move_up();
            }
            KeyCode::Down | KeyCode::Char('j') if !picker_ctrl => {
                picker.move_down();
            }
            KeyCode::Backspace => {
                picker.pop_char();
            }
            KeyCode::Char(c) if !picker_ctrl => {
                picker.push_char(c);
            }
            _ => {}
        }
        // Keep scroll in sync with selection.
        if let Some(ref mut picker) = folder_picker {
            let popup_h = ((screen_h as usize) * 55 / 100).max(15);
            let visible_rows = popup_h.saturating_sub(4);
            picker.sync_scroll(visible_rows);
        }
        return Reaction::Redraw;
    }

    if key_event.kind == KeyEventKind::Release {
        return Reaction::Continue;
    }
    let Some((key_name, unicode, ctrl)) = translate_key(key_event, keyboard_enhanced) else {
        return Reaction::Continue;
    };

    // ── Context menu keyboard intercept (mirrors mod.rs:2608-:2635) ─────
    if engine.context_menu.is_some() {
        let effective_key = if key_name.is_empty() {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        } else {
            key_name.clone()
        };
        let ctx = engine.context_menu_target_path();
        let (consumed, action) = engine.handle_context_menu_key(&effective_key);
        if consumed {
            if let (Some(act), Some((ctx_path, ctx_is_dir))) = (action, ctx) {
                handle_explorer_context_action(
                    &act,
                    engine,
                    sidebar,
                    Some(Size::new(screen_w, screen_h)),
                    ctx_path,
                    ctx_is_dir,
                );
            }
            return Reaction::Redraw;
        }
    }

    // ── General fallback: `Engine::handle_key` (mirrors
    // mod.rs:2637-:2737) ─────────────────────────────────────────────────
    let action = engine.handle_key(&key_name, unicode, ctrl);
    if engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
        engine.ai_completion_reset_timer();
    }

    if action == EngineAction::OpenTerminal {
        let cols = terminal_panel_cols(engine, screen_w, sidebar_width);
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_new_tab(cols, rows);
    } else if action == EngineAction::ToggleTerminalMaximize {
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: terminal_panel_cols(engine, screen_w, sidebar_width),
            terminal_max_rows: terminal_target_maximize_rows_tui(engine, screen_h),
        };
        engine.handle_ui_event(
            crate::core::engine::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(ACC_TERMINAL_TOGGLE_MAX),
                quadraui::Modifiers::default(),
            ),
            ctx,
        );
    } else if let EngineAction::RunInTerminal(cmd) = &action {
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_run_command(cmd, screen_w, rows);
    } else if action == EngineAction::OpenFolderDialog {
        *folder_picker = Some(FolderPickerState::new(
            &engine.cwd.clone(),
            FolderPickerMode::OpenFolder,
            engine.settings.show_hidden_files,
        ));
    } else if action == EngineAction::OpenRecentDialog {
        if engine.session.recent_workspaces.is_empty() {
            engine.message = "No recent workspaces".to_string();
        } else {
            engine.open_picker(crate::core::engine::PickerSource::RecentWorkspaces);
        }
    } else if action == EngineAction::OpenWorkspaceDialog {
        *sidebar = TuiSidebar::new();
        engine.explorer_rebuild_rows();
    } else if action == EngineAction::SaveWorkspaceAsDialog {
        let ws_path = engine.cwd.join(".vimcode-workspace");
        engine.save_workspace_as(&ws_path);
    } else if action == EngineAction::QuitWithUnsaved {
        engine.show_quit_confirm();
    } else if handle_action(engine, action) {
        return Reaction::Exit;
    }

    Reaction::Redraw
}

#[cfg(test)]
mod tests {
    //! `TuiDriver`/`driver_with_shell` (quadraui's headless `ShellApp`
    //! harness) wraps the app in a `pub(crate)`-fielded `ShellAdapter` with
    //! no accessor back to the concrete `TuiShellApp` and no exposed
    //! `tick()` passthrough — so `setup`/`tick` are exercised directly here
    //! against a real `TuiBackend`, which is exactly what the live runner
    //! does under the hood. `driver_with_shell` is used for the end-to-end
    //! smokes (does the whole `ShellConfig` wiring construct and paint a
    //! first frame without panicking) and — #603 (Stage 4) — for
    //! `handle()`'s `KeyPressed` dispatch: `ShellContext::new` is
    //! quadraui-`pub(crate)`, so `handle()` itself can only be driven
    //! through the driver's key-injection helpers (`press`/`type_char`/
    //! `dispatch`), asserting on the painted screen rather than on internal
    //! `TuiShellApp` fields. `handle_key_pressed`'s dialog/context-menu
    //! branches are additionally exercised directly (bypassing `handle()`
    //! and the driver entirely) since that logic lives in a free function
    //! over a bare `&mut Engine`.
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

    /// #609: `render_content` must also paint the *group-level* divider
    /// line between split editor groups — `render_group_dividers`, ported
    /// from `draw_frame`'s raw-`Buffer`-read loop to
    /// `Backend::draw_status_bar` (see that function's and
    /// `group_divider_cells`'s doc comments). Content is short (well under
    /// the viewport height) so neither pane overflows and shows a
    /// scrollbar — the #481 guard that lets a pane's own scrollbar double
    /// as the separator would otherwise mask the divider glyph itself from
    /// this assertion.
    ///
    /// Rather than hard-code an expected column (fragile against
    /// `AppShell`'s own activity-bar/sidebar layout constants), the
    /// expected column is derived from the actual painted screen: both
    /// panes' tab bars share row 0 (`"[No Name]"` once per pane), so the
    /// divider must land strictly between the two tab labels' start
    /// columns on every row of the editor body.
    #[test]
    fn render_content_paints_group_divider_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "short\n");
        app.engine.open_editor_group(SplitDirection::Vertical);
        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        let lines: Vec<&str> = screen.lines().collect();

        let tab_row = lines[0];
        let starts: Vec<usize> = tab_row
            .match_indices("[No Name]")
            .map(|(i, _)| tab_row[..i].chars().count())
            .collect();
        assert_eq!(
            starts.len(),
            2,
            "expected two tab bars, one per pane; row:\n{tab_row}"
        );
        let (left_tab_start, right_tab_start) = (starts[0], starts[1]);

        // Only scan columns to the right of the left pane's own tab label —
        // the sidebar (a separate screen region, to the left of both panes)
        // can paint its own unrelated '│' glyphs (e.g. explorer tree
        // indent guides), which aren't the group divider under test.
        let mut found_divider = false;
        for (y, line) in lines.iter().enumerate().skip(1).take(15) {
            let chars: Vec<char> = line.chars().collect();
            let Some(col) = chars
                .iter()
                .enumerate()
                .skip(left_tab_start)
                .find(|(_, &c)| c == '│')
                .map(|(i, _)| i)
            else {
                continue;
            };
            found_divider = true;
            assert!(
                col > left_tab_start && col < right_tab_start,
                "row {y}: divider at col {col} should land strictly between the \
                 two panes' tab labels (cols {left_tab_start}..{right_tab_start}); \
                 line:\n{line}"
            );
        }
        assert!(
            found_divider,
            "expected the group divider glyph '│' to paint via \
             TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #609: `render_content` must also paint the tab-drag ghost overlay —
    /// `render_tab_drag_overlay`, ported from a raw-`Frame`-write tail
    /// (the ghost label) to `Backend::draw_status_bar` (see its doc
    /// comment). Drives a real drag through `TuiDriver`'s mouse harness
    /// (`mouse_down` + `mouse_move`, no `mouse_up`) so `tui_drag_source`
    /// is genuinely live when `render_content` runs — #602 already wires
    /// `handle_mouse_event` to populate it, so this is exercising the
    /// paint side, not the input side. `mouse.rs`'s drag-start detection
    /// requires the cursor to move `dx + dy >= 2` cells from the
    /// mouse-down position before activating (distinguishing a drag from
    /// a click), hence the `+4, +3` move. `driver_with_shell` wraps
    /// `TuiShellApp` in an opaque `ShellAdapter` with no accessor back to
    /// it (see this `mod tests`'s own doc comment), so this asserts on the
    /// painted screen — a third `"[No Name]"` occurrence, the ghost label,
    /// alongside the two static tab labels — rather than on
    /// `tui_drag_source` directly.
    #[test]
    fn render_content_paints_tab_drag_ghost_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.new_tab(None);
        let mut driver = driver_with_shell(app, config(), 80, 24);
        let (tx, ty) = driver
            .find("[No Name]")
            .expect("tab label should be painted on screen");
        driver.mouse_down(tx, ty);
        driver.mouse_move(tx + 4.0, ty + 3.0);

        let screen = driver.screen();
        let occurrences = screen.matches("[No Name]").count();
        assert!(
            occurrences >= 3,
            "expected the two static tab labels plus a drag-ghost label \
             (>= 3 occurrences of \"[No Name]\"), got {occurrences}; screen:\n{screen}"
        );
    }

    /// #609: `render_content` must also paint the tab-hover tooltip —
    /// `render_tab_hover_tooltip`, ported from `draw_frame`'s raw-`Buffer`
    /// write to `Backend::draw_status_bar` (see that function's doc
    /// comment). `screen.tab_tooltip` sources straight from
    /// `engine.tab_hover_tooltip` (`render.rs`'s `ScreenLayout` builder),
    /// a plain `Option<String>` with no real-time hover-dwell timer behind
    /// it — unlike the tab-drag overlay (real SGR mouse-drag sequencing,
    /// left to `SMOKE_TESTS`), so this is fully reachable headlessly by
    /// just setting the field directly before painting.
    #[test]
    fn render_content_paints_tab_hover_tooltip_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.tab_hover_tooltip = Some("ZQXW_609_TOOLTIP_MARKER".to_string());
        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW_609_TOOLTIP_MARKER"),
            "tab-hover tooltip should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #607: `render_content` must also paint the *sidebar's* content —
    /// the explorer tree, since explorer is the default active panel — into
    /// `layout.sidebar_content_bounds`, via
    /// `panels::render_sidebar_content`. Mirrors the temp-dir-plus-
    /// `explorer_rebuild_rows` pattern `core::engine::tests`'s explorer
    /// tests already use (`test_goto_tab_reveals_file_in_explorer` et al.):
    /// write a file with a distinctive name under a fresh temp dir, point
    /// `engine.cwd` at it, and reveal the file via `explorer_reveal_path`
    /// (which expands the root row *and* rebuilds the rows — the root
    /// starts collapsed, so `explorer_rebuild_rows` alone would only show
    /// the root folder's own row, not its children) before asserting the
    /// file name shows up in the painted sidebar column. The marker is kept
    /// short (well under `SIDEBAR_WIDTH`) so it survives the tree row's
    /// icon/indent prefix without truncation.
    #[test]
    fn render_content_paints_explorer_sidebar_content_via_shell_app() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_607_shell_app_explorer_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker_file = dir.join("zqxw607.txt");
        std::fs::write(&marker_file, "marker").unwrap();

        let mut app = TuiShellApp::new(None);
        app.engine.cwd = dir.clone();
        app.engine.explorer_reveal_path(&marker_file);

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("zqxw607.txt"),
            "explorer sidebar content should paint via TuiShellApp::render_content; screen:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #608: `render_content` must also paint the *quickfix panel* — a
    /// persistent bottom strip vimcode's own row math carves out of
    /// `layout.main_content_bounds` (quadraui's `AppShellLayout` has no
    /// concept of it) — via `bottom_chrome_rects_for_shell_content` +
    /// `panels::render_quickfix_panel`. Seeds a single match with a
    /// distinctive `line_text` (short enough to survive the
    /// `file:line: snippet` formatting `render.rs`'s quickfix adapter
    /// applies) and opens the panel directly on `engine` state, mirroring
    /// how the live find-references flow sets `quickfix_items` +
    /// `quickfix_open` (`core/engine/panels.rs`).
    #[test]
    fn render_content_paints_quickfix_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .quickfix_items
            .push(crate::core::project_search::ProjectMatch {
                file: PathBuf::from("zqxw608.rs"),
                line: 0,
                col: 0,
                line_text: "ZQXW_608_QUICKFIX_MARKER".to_string(),
            });
        app.engine.quickfix_open = true;

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW_608_QUICKFIX_MARKER"),
            "quickfix panel content should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #608: `render_content` must also paint the *bottom panel* (terminal
    /// tab bar + Debug Output content) via
    /// `bottom_chrome_rects_for_shell_content` + `render_bottom_panel_tabs`
    /// + the `Backend::draw_text_display` branch (already trait-pure, no
    /// raw `Frame`/`Buffer` access — see `shell_app.rs`'s module doc). Sets
    /// `bottom_panel_open` + `bottom_panel_kind` directly (mirroring how a
    /// real DAP session flips them, `core/engine/dap_ops.rs`) rather than
    /// running an actual debug session, and seeds `dap_output_lines` with a
    /// marker line.
    #[test]
    fn render_content_paints_bottom_panel_debug_output_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.bottom_panel_open = true;
        app.engine.bottom_panel_kind = render::BottomPanelKind::DebugOutput;
        app.engine
            .dap_output_lines
            .push("ZQXW_608_DEBUG_OUTPUT_MARKER".to_string());

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW_608_DEBUG_OUTPUT_MARKER"),
            "bottom panel debug-output content should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #608: the *terminal* branch of the bottom panel (tab bar label +
    /// `panels::render_terminal_panel_content`, the trait-pure counterpart
    /// to `render_terminal_panel`'s raw-`Frame` background-clear loop) must
    /// also paint without panicking. Spawns a real PTY via
    /// `terminal_new_tab` (same pattern `render.rs`'s own terminal tests
    /// use) rather than asserting on the shell's prompt text, which is
    /// environment-dependent — the tab bar's "Terminal" label
    /// (`render::build_bottom_panel_tab_bar`) is the one deterministic
    /// signal available that the terminal branch (not the Debug Output
    /// branch above) painted.
    #[test]
    fn render_content_paints_bottom_panel_terminal_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.terminal_new_tab(80, 10);

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("Terminal"),
            "bottom panel terminal tab bar should paint via TuiShellApp::render_content; screen:\n{screen}"
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

    /// #603 baseline: a plain `KeyPressed` sequence (no modal state open)
    /// must reach `Engine::handle_key` and actually mutate the buffer —
    /// establishes that the general fallback in `handle_key_pressed` is
    /// wired at all, so `command_palette_open_intercepts_keys_via_shell_app`
    /// below has something meaningful to contrast against.
    #[test]
    fn key_press_inserts_text_via_shell_app_general_fallback() {
        let app = TuiShellApp::new(None);
        let mut driver = driver_with_shell(app, config(), 80, 24);

        driver.type_char('i'); // Normal -> Insert
        for c in "ZQXW_TYPED".chars() {
            driver.type_char(c);
        }

        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW_TYPED"),
            "typed text should reach the buffer via Engine::handle_key; screen:\n{screen}"
        );
    }

    /// #604: closes gap 3. `render_content` never gets a raw `Frame` (see
    /// the module doc), so `render_window`/`render_all_windows` always call
    /// `Backend::draw_editor` with `frame: None` on the `ShellApp` path —
    /// but quadraui#466 moved the `Frame::set_cursor_position` handoff out
    /// of that call site entirely: `TuiBackend::draw_editor` now caches
    /// `EditorPaintResult::cursor_position` on itself unconditionally, and
    /// `quadraui::tui::run::render_frame` (the fn `ShellAdapter::render`
    /// runs inside, shared by the live runner/`run_with_shell`/`TuiDriver`)
    /// applies it to the real `Frame` *after* `render_content` returns — so
    /// the missing raw-`Frame` access in `render_content` no longer matters
    /// for cursor placement. Insert mode paints a `Bar` cursor (see
    /// `render.rs`'s cursor-shape match), which is one of the two shapes
    /// `draw_editor` reports a `cursor_position` for (the other is
    /// `Underline`; `Block` inverts a buffer cell instead and reports
    /// `None` — out of scope here). Derives the expected screen position
    /// from where the typed marker actually painted (`TuiDriver::find`)
    /// rather than hard-coding gutter/chrome offsets, so the assertion
    /// stays correct regardless of line-number gutter width or how many
    /// chrome rows (tab bar, breadcrumb bar) sit above the editor.
    #[test]
    fn insert_mode_bar_cursor_reaches_terminal_frame_via_shell_app() {
        const MARKER: &str = "ZQXW_CURSOR_MARKER";

        let app = TuiShellApp::new(None);
        let mut driver = driver_with_shell(app, config(), 80, 24);

        driver.type_char('i'); // Normal -> Insert, Bar cursor shape.
        for c in MARKER.chars() {
            driver.type_char(c);
        }

        let (marker_x, marker_y) = driver
            .find(MARKER)
            .expect("typed marker should be visible on screen");
        // `find` returns cell-*centre* coordinates (`col + 0.5`); the cursor
        // sits one cell past the marker's last character, in the same row.
        let marker_col = (marker_x - 0.5).round() as u16;
        let expected = (
            marker_col + MARKER.chars().count() as u16,
            (marker_y - 0.5).round() as u16,
        );

        assert_eq!(
            driver.terminal_cursor_position(),
            Some(expected),
            "draw_editor's Bar cursor_position should reach Frame::set_cursor_position \
             via render_frame, even though render_content passes frame: None"
        );

        // Confirm the handoff tracks a later frame too (not a first-paint
        // fluke) — typing another char must shift the applied position by
        // exactly one cell, mirroring quadraui's own
        // `editor_bar_cursor_position_updates_across_frames`.
        driver.type_char('!');
        assert_eq!(
            driver.terminal_cursor_position(),
            Some((expected.0 + 1, expected.1)),
            "a later frame's draw_editor call must overwrite the previous cursor position"
        );
    }

    /// #603 acceptance: once the command palette is open
    /// (`engine.picker_open`), `Engine::handle_key` resolves it internally
    /// (`keys.rs:152`) — the same "i" + marker keystrokes that insert text
    /// in `key_press_inserts_text_via_shell_app_general_fallback` above
    /// must instead feed the picker's query and never reach the buffer.
    /// Opens the palette via the already-wired (Stage 0) `ACC_COMMAND_PALETTE`
    /// accelerator, exactly how a real keybinding would.
    #[test]
    fn command_palette_open_intercepts_keys_via_shell_app() {
        let app = TuiShellApp::new(None);
        let mut driver = driver_with_shell(app, config(), 80, 24);

        let opened = driver.dispatch(quadraui::UiEvent::Accelerator(
            quadraui::AcceleratorId::new(ACC_COMMAND_PALETTE),
            quadraui::Modifiers::default(),
        ));
        assert_eq!(
            opened,
            Reaction::Redraw,
            "opening the command palette should request a redraw"
        );

        driver.type_char('i');
        for c in "ZQXW_TYPED".chars() {
            driver.type_char(c);
        }

        let screen = driver.screen();
        assert!(
            !screen.contains("ZQXW_TYPED"),
            "keys typed while the command palette is open must not reach the \
             editor buffer (they should feed the picker query instead); \
             screen:\n{screen}"
        );
    }

    /// #603 acceptance / #318: Alt+<menu-letter> must reveal the (hidden by
    /// default) menu bar and hand the very same keystroke to the
    /// `MenuSystem` intercept, which activates the matching top-level menu.
    /// `handle()` never paints the menu bar itself (out of `render_content`'s
    /// scope this stage — see the module doc), so the only screen-visible
    /// effect of `engine.menu_bar_visible` flipping is that
    /// `build_screen_for_shell_content` reserves one extra row above the
    /// editor content (`menu_height`) — shifting the marker text down by
    /// exactly one line is this test's proof the #318 shim actually ran.
    #[test]
    fn alt_letter_reveals_menu_bar_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "ZQXW_ALT_MARKER");
        let mut driver = driver_with_shell(app, config(), 80, 24);

        let before = driver.screen();
        let before_row = before
            .lines()
            .position(|l| l.contains("ZQXW_ALT_MARKER"))
            .expect("marker should paint before the Alt-reveal keypress");

        // 'f' is `MENU_STRUCTURE`'s alt-letter for the "File" menu
        // (`render.rs`: `("File", 'f', ...)`). The bar starts hidden —
        // `Engine::new()` defaults `menu_bar_visible = false` outside
        // vscode mode (`mod.rs:3685`/`:3963`) — so this exercises the #318
        // shim specifically, not an already-visible bar.
        let reaction = driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('f'),
            modifiers: quadraui::Modifiers {
                alt: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });
        assert_eq!(
            reaction,
            Reaction::Redraw,
            "Alt+F should reveal + activate the File menu, both of which redraw"
        );

        let after = driver.screen();
        let after_row = after
            .lines()
            .position(|l| l.contains("ZQXW_ALT_MARKER"))
            .expect("marker should still paint after the Alt-reveal keypress");
        assert_eq!(
            after_row,
            before_row + 1,
            "revealing the menu bar should reserve one more row above the \
             editor content, shifting the marker down by exactly one line; \
             before:\n{before}\nafter:\n{after}"
        );
    }

    /// `handle_key_pressed`'s dialog branch must route every key straight
    /// to `Engine::handle_key` while a dialog is open, bypassing the
    /// context-menu/general-fallback branches entirely (mirrors
    /// `mod.rs:1629`-`:1651`). Exercised directly against a bare `Engine`
    /// (see the test-module doc) rather than through `driver_with_shell`,
    /// since `ShellContext` has no public constructor.
    #[test]
    fn handle_key_pressed_dialog_intercepts_all_keys() {
        let mut engine = Engine::new();
        engine.show_quit_confirm();
        assert!(engine.dialog.is_some());
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;

        // "Down" cycles the selected dialog button (`panels.rs`'s
        // `handle_dialog_key`) — proving the key actually reached
        // `Engine::handle_key`'s dialog handling, not re-testing dialog
        // button navigation itself.
        let reaction = handle_key_pressed(
            quadraui::Key::Named(quadraui::NamedKey::Down),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            SIDEBAR_WIDTH,
            &mut folder_picker,
            false,
            80,
            24,
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            engine.dialog.is_some(),
            "dialog should remain open after a navigation key"
        );
        assert_eq!(engine.dialog.as_ref().unwrap().selected, 1);
    }

    /// `handle_key_pressed`'s folder-picker branch must intercept keys once
    /// `folder_picker` is populated (mirrors `mod.rs:1653`-`:1708`), ahead
    /// of the context-menu/general-fallback branches — this is the
    /// regression the tier guards against: without it, a keystroke like
    /// `'x'` (a bare-letter Normal-mode delete-char motion) would fall
    /// through to `Engine::handle_key` instead of updating the picker's
    /// type-to-filter `query`. Also confirms `Esc` dismisses the picker.
    #[test]
    fn handle_key_pressed_folder_picker_intercepts_keys() {
        let mut engine = Engine::new();
        let cwd = engine.cwd.clone();
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = Some(FolderPickerState::new(
            &cwd,
            FolderPickerMode::OpenFolder,
            engine.settings.show_hidden_files,
        ));

        let reaction = handle_key_pressed(
            quadraui::Key::Char('x'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            SIDEBAR_WIDTH,
            &mut folder_picker,
            false,
            80,
            24,
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert_eq!(
            folder_picker.as_ref().map(|p| p.query.as_str()),
            Some("x"),
            "'x' should filter the picker's entry list, not reach Engine::handle_key \
             as a delete-char motion"
        );

        let reaction = handle_key_pressed(
            quadraui::Key::Named(quadraui::NamedKey::Escape),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            SIDEBAR_WIDTH,
            &mut folder_picker,
            false,
            80,
            24,
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(folder_picker.is_none(), "Esc should dismiss the picker");
    }

    /// `handle_key_pressed`'s context-menu branch must dispatch the
    /// confirmed item's action to [`handle_explorer_context_action`]
    /// (mirrors `mod.rs:2608`-`:2635`) — unlike `Engine::handle_key`'s own
    /// context-menu branch (`keys.rs:66`-`:71`), which consumes the key but
    /// silently discards the resulting action. Confirms the first item
    /// ("New File...", action `"new_file"`) of a folder context menu and
    /// asserts on `explorer_new_entry_pending`, the observable side effect
    /// `dispatch_explorer_crud(ExplorerAction::NewFile)` produces.
    #[test]
    fn handle_key_pressed_context_menu_dispatches_explorer_action() {
        let mut engine = Engine::new();
        let dir = engine.cwd.clone();
        engine.open_explorer_context_menu(dir, true, 5, 5);
        assert!(engine.context_menu.is_some());
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;

        let reaction = handle_key_pressed(
            quadraui::Key::Named(quadraui::NamedKey::Enter),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            SIDEBAR_WIDTH,
            &mut folder_picker,
            false,
            80,
            24,
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            engine.context_menu.is_none(),
            "confirming an item should close the context menu"
        );
        assert!(
            engine.explorer_new_entry_pending.is_some(),
            "the 'New File...' action should reach `handle_explorer_context_action` \
             and dispatch `ExplorerAction::NewFile`"
        );
    }
}
