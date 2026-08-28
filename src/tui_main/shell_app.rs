//! `TuiShellApp` — TUI counterpart to `src/gtk/mod.rs`'s `impl ShellApp for
//! App` (#493). Tracks vimcode#595.
//!
//! **Status: live.** #634 (Stage 6) flipped `main.rs`/`tui_bin.rs` over to
//! `tui_main::run` → `quadraui::tui::shell_runner::run_with_shell` and
//! deleted `event_loop()` along with the rest of the hand-rolled bootstrap,
//! so this module *is* the TUI. Line references to `mod.rs:NNNN` throughout
//! this file point at the deleted loop as it stood at its final revision
//! (`509b8fe`) — they document provenance, and `git show 509b8fe:src/
//! tui_main/mod.rs` is the way to read them.
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
//!      issue) is now fully painted. #605 closed the first two of five:
//!      **settings** and **source control** (header rows, focused-hint row,
//!      and full-area background clears were raw `set_cell` loops; now
//!      `fill_row`/`fill_rect`). Both renderers dropped their `&mut Frame`
//!      parameter, so there is one implementation shared by `draw_frame` and
//!      `render_content`, and `panels.rs`'s own source-control buffer
//!      snapshot test still passes unchanged — i.e. the conversion is
//!      byte-identical on the live path. **extensions** followed the same
//!      way (`render_ext_sidebar`'s two chrome rows were a local
//!      raw-`set_cell` `write_row` closure that is exactly what `fill_row`
//!      does).
//!
//!      #635 (Stage 6b, item C) closed the last two. The **settings chrome**
//!      specifically also got its clean long-term fix in the same stage:
//!      [JDonaghy/quadraui#531](https://github.com/JDonaghy/quadraui/issues/531)
//!      landed `Backend::draw_settings_chrome` as a real trait method, so
//!      the temporary `panels::draw_settings_chrome_via_backend` rule-row
//!      stand-in this doc used to describe is gone — `render_settings_panel`
//!      and `render_ext_panel` both call the trait method directly now. The
//!      **plugin extension panel**'s help-popup overlay now paints through
//!      `Backend::draw_tooltip` (manually-constructed `TooltipLayout`, since
//!      its content is a centered box rather than an anchor-relative
//!      tooltip) and its manual scrollbar through `fill_row`; the **AI
//!      panel** (`render_ai_sidebar`, previously the most raw of the lot —
//!      `buf: &mut ratatui::buffer::Buffer` with no backend parameter at
//!      all) now takes `&mut dyn Backend`, using the already-trait-pure
//!      `Backend::draw_message_list` for the chat history and `fill_row` for
//!      its plain chrome rows. See `render_ext_panel`'s and
//!      `render_ai_sidebar`'s own doc comments for the couple of minor,
//!      intentional cosmetic differences (no border-embedded popup title, no
//!      close glyph, message-list background sourced from
//!      `TuiBackend::current_theme.background` instead of an explicit
//!      parameter).
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
//!    Output tab. **Split terminal panes** (`Ctrl+\`,
//!    `panel.split_left_rows.is_some()`) used to be a known gap here —
//!    `render_terminal_panel`'s split arm drew its divider via the free
//!    `quadraui::tui::draw_terminal_divider` rasteriser, with no
//!    `Backend::draw_*` trait equivalent (same class of gap as
//!    `draw_settings_chrome`) — but #635 (Stage 6b, item B) closed it now
//!    that [JDonaghy/quadraui#533](https://github.com/JDonaghy/quadraui/issues/533)
//!    landed `Backend::draw_terminal_divider`: both panes and the divider
//!    now paint through the trait, mirroring `render_terminal_panel`'s live
//!    `draw_frame` path exactly. See `panels::render_terminal_panel_content`'s
//!    own doc comment.
//!
//!    The menu bar row used to be reserved in the layout math but not
//!    painted (out of scope for #601; folded into key dispatch, #603, then
//!    painting, #635 item A — see the Stage 6b section below). Cursor
//!    placement used to be a raw-buffer holdout in this list (it needs
//!    `Frame::set_cursor_position`, and `render_content` has no `Frame`)
//!    but #604 closed it a different way — see gap 3 below, now resolved.
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
//! Given (2) is now closed by #602, (3) is now closed by #604, and (1) is
//! now fully closed by #605 + #635 (Stage 6b), `handle()` below implements
//! every keyboard/mouse dispatch layer the deleted loop had: panel-key
//! accelerators, the "#318" Alt+menu-letter
//! "reveal menu bar" shim (mirrors `mod.rs:1319`-`:1338` — sets
//! `engine.menu_bar_visible = true` on an Alt+<letter> keypress so the same
//! keystroke both reveals and activates the menu), the `MenuSystem`
//! intercept, full mouse dispatch through `handle_mouse_event` (#602), and
//! — #603 (Stage 4) — the `KeyPressed` dispatch chain (modal dialog /
//! folder-picker-modal / context-menu intercepts, then the general
//! `Engine::handle_key` fallback that also resolves command-palette and
//! completion-popup state internally), plus — #635 (Stage 6b, item D) —
//! activity-bar-focused and command-output-selection (`cmd_sel`) tiers, and
//! — #634 (Stage 6) — the sidebar-focused tier
//! ([`handle_sidebar_focused_key`]), terminal/PTY key routing, the
//! Alt-modifier block, clipboard paste pre-load, Ctrl+Shift+V, the
//! Shift+F5/F11 debug shortcuts and the post-key epilogue. See
//! `handle_key_pressed`'s own doc comment for the exact precedence chain and
//! for why it's a free function rather than a `TuiShellApp` method.
//!
//! # Stage 6b (#635) cutover prerequisites: results
//!
//! #605 (Stage 6) swept `draw_frame`'s tail into `render_content` (separated
//! status, debug toolbar, wildmenu, global status bar, command line,
//! panel hover popup, folder picker, find/replace, unified picker, tab
//! switcher, context menu, dialog, toast stack), closed three of the five
//! sidebar-panel gaps (settings, source control, extensions), and recorded
//! six items (A–F) a cutover to `quadraui::tui::shell_runner::run_with_shell`
//! would otherwise silently regress. #635 (Stage 6b) is that follow-through:
//!
//! **A. Menu bar + command centre + menu dropdown — done.**
//! [JDonaghy/quadraui#532](https://github.com/JDonaghy/quadraui/issues/532)
//! landed `AppShell::set_title_bar_visible`, the runtime toggle this needed
//! (unlike `AppShell::with_title_bar`, a construction-time-only commitment —
//! see that method's own doc comment). [`TuiShellApp::shell_config`] seeds
//! the title-bar reservation from `engine.menu_bar_visible` at construction
//! (so the very first frame, painted before any `handle()` dispatch, is
//! already correct), and `handle()` keeps it synced via
//! `ShellContext::shell_mut().set_title_bar_visible(...)` on the way *out*
//! of every subsequent dispatch — after the dispatch, so a reveal performed
//! by the dispatch itself (Alt+F, `:set menu`) lands on the frame the runner
//! paints for that same event rather than a frame later. That reservation is
//! the *only* menu-bar row: `build_screen_for_shell_content` deliberately
//! carries no `menu_height` term of its own, since `main_content_bounds`
//! already has AppShell's title-bar row carved off it (see that function's
//! doc comment). `render_content` paints the menu bar + command
//! centre into `layout.title_bar_bounds` when reserved, and the menu
//! dropdown last (after the dialog, before the toast stack — mirrors
//! `draw_frame`'s own "rendered last so it draws on top of everything"
//! ordering). `draw_menu_bar`, `draw_command_center` and
//! `MenuSystem::render` were already trait calls, as this doc predicted —
//! the gap was purely the missing reservation toggle.
//!
//! A review of the first pass caught a second, more subtle gap in the same
//! area: `Self::shell_config` always sets `title_bar_height_lh = 1.0`
//! regardless of `has_title_bar` — deliberately, so a later
//! `set_title_bar_visible(true)` toggle reserves the same 1 row
//! `draw_frame`'s live path uses, not `AppShell`'s 1.5-line-height struct
//! default — but `quadraui::tui::shell_runner::build_shell_adapter` only
//! called `AppShell::with_title_bar` (which actually sets that height) when
//! `has_title_bar` was already `true` at construction, silently discarding
//! `title_bar_height_lh` on the (default) start-hidden path. That's a
//! `quadraui` gap, not a vimcode one — no local stand-in was written per the
//! Platform-Neutrality Rule; it was fixed upstream as
//! [JDonaghy/quadraui#547](https://github.com/JDonaghy/quadraui/issues/547)
//! ("honour `ShellConfig::title_bar_height_lh` regardless of
//! `has_title_bar`"), landed as `f702422`, and this repo's quadraui path-dep
//! checkout must carry it or later. Covered end-to-end (hidden at
//! construction → revealed at runtime → exactly one row of *total* menu-bar
//! footprint, measured against the pre-reveal baseline) by
//! `shell_config_hidden_then_revealed_reserves_exactly_one_title_bar_row`
//! below — confirmed to fail against pre-#547 quadraui and pass at #547+.
//!
//! A review of the *second* pass caught the vimcode half of the same
//! accounting: `build_screen_for_shell_content` still subtracted its own
//! `menu_height` row from `main_content_bounds`, which `AppShell` had
//! already carved the title-bar row out of — 2 rows consumed for a 1-row
//! bar, one of them blank, with the editor content pushed one row too far
//! down. That local term (and the matching one in `render_content`'s
//! tab-hover-tooltip offset) is gone; the test above now measures the total
//! footprint from the pre-reveal baseline instead of a delta between two
//! already-shifted frames, which is what let the double count hide.
//!
//! **B. Split terminal panes — done.**
//! [JDonaghy/quadraui#533](https://github.com/JDonaghy/quadraui/issues/533)
//! landed `Backend::draw_terminal_divider`. Both `render_terminal_panel`
//! (the live `draw_frame` path) and `render_terminal_panel_content` (the
//! `render_content` path) now paint both panes and the divider through it;
//! see the terminal-divider migration note in the painting section above.
//!
//! **C. Plugin extension panel + AI sidebar panel — done.**
//! `render_ext_panel`'s help-popup overlay now paints through
//! `Backend::draw_tooltip`, its manual scrollbar through `fill_row`;
//! `render_ai_sidebar` dropped its `buf: &mut Buffer` parameter for
//! `&mut dyn Backend`, using `Backend::draw_message_list` (already a trait
//! method — it just wasn't being called through it) for the chat history.
//! See the painting section above and each function's own doc comment for
//! the couple of minor, intentional cosmetic differences.
//!
//! **D. The three unported keyboard tiers — done.** Activity-bar-focused
//! (mirrors `mod.rs:1805`-`:1854`) and `cmd_sel` (mirrors `mod.rs:2651`-
//! `:2701`) landed in #635. The sidebar-focused tier — eight per-panel
//! dispatchers (search / debug / plugin extension panel / extensions /
//! settings / AI / source control / explorer) plus its own context-menu
//! intercept and Ctrl-W navigation, `mod.rs:1886`-`:2415` — landed in #634
//! as [`handle_sidebar_focused_key`], since the cutover would otherwise
//! have dropped every one of those keys through to the general
//! `Engine::handle_key` fallback.
//!
//! **E. `ShellConfig` build-out — done.** [`TuiShellApp::shell_config`]
//! derives its panel list from the same `PANEL_*` ids
//! `render::build_activity_bar`'s `fixed` array switches on (explorer,
//! search, debug, source control, extensions, AI), plus the menu hamburger
//! (top) and settings (bottom) — the two items outside that array. Dynamic
//! per-session extension panels (`engine.ext_panels`) are NOT included;
//! wiring those through `AppShell::add_panel`/`remove_panel` needs a live
//! `AppShell` instance, which nothing constructs until #634.
//! [`ShellApp::on_shell_event`] intercepts the hamburger's
//! `AppShellEvent::PanelChanged` and reveals the menu bar instead of
//! switching the sidebar to a nonexistent "Menu" panel. (`AppShell::
//! build_activity_bar` still leaves `active_accent`/`selection_bg` `None`
//! where vimcode's sets them from the theme; the TUI rasteriser falls back
//! to `theme.cursor`, so that's still a cosmetic difference, not a
//! blocker.)
//!
//! **F. `run()`'s own non-loop responsibilities — done.**
//! [`super::run`] (`mod.rs`, #635's `run_via_shell` renamed when #634 made
//! it the only entry point) reproduces the old `run()`'s panic
//! hook, emergency-engine registration, emergency swap flush, and custom
//! crash message around `run_with_shell` instead of `event_loop` — see its
//! own doc comment for the exact sequencing, and [`Self::prepare_for_live_run`]
//! / `ShellApp::setup`'s `self.live` gate for why `keyboard_enhanced` and
//! the (`unsafe`) emergency-engine pointer registration had to move into
//! `setup()` rather than staying in the wrapper: `run_with_shell` takes
//! `app` *by value* and moves it through several stack frames before it
//! settles, so a pointer captured before that call would already be stale.
//! #634 wired it into `main.rs`/`tui_bin.rs` and deleted the loop it used
//! to sit beside.
//!
//! Note that none of the above is reachable by `driver_with_shell` in the
//! sense of proving the *live* TUI works — see the pinned note on #605:
//! `TuiDriver` renders to `TestBackend` and never parses real ANSI, so
//! raw-mode, SGR mouse, and the embedded PTY pane stay outside its reach and
//! the cutover needs a human smoke pass regardless.

use std::cell::{Cell, RefCell};

use quadraui::{Reaction, ShellApp, ShellContext, UiEvent};

use super::*;

/// Link hit rects from a hover popup render: `(x, y, w, h, url)`, matching
/// `event_loop`'s `hover_link_rects`/`editor_hover_link_rects` locals
/// verbatim. Named alias so the `TuiShellApp` fields below don't trip
/// clippy's `type_complexity` lint.
type HoverLinkRects = Vec<(u16, u16, u16, u16, String)>;

/// Activity-bar item id for the menu hamburger.
///
/// #536 promoted the literal to `core::engine::sidebar` — it is now shared by
/// `render::build_activity_bar`'s hamburger `ActivityItem`, this module's
/// `ShellConfig` `PanelDefinition`, [`ShellApp::on_shell_event`]'s hamburger
/// check, *and* `Engine::activity_bar_item_id`'s keyboard-index-0 slot, so
/// there is exactly one definition rather than one per call site.
use crate::core::engine::sidebar::HAMBURGER_PANEL_ID;

/// TUI counterpart to GTK's `App` struct. Owns everything that is a local
/// `mut` variable in `event_loop()` today. Fields the (`&self`)
/// `render_content` needs to *write* during paint are wrapped in
/// `Cell`/`RefCell` — mirroring GTK's `App` (`menu_row_rect: Cell<Rect>`,
/// etc.) and the render-time caches `Engine` itself already uses
/// (`sc_panel_layout`, `explorer_tree_rect`, ...).
///
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
    /// Per-group tab-bar visible counts measured by the most recent
    /// `render_content`. `event_loop` collected these into a `draw_frame`
    /// out-param and fed them straight to `Engine::post_draw_apply_widths`
    /// (`mod.rs:1171`, `:1216`); `render_content` is `&self`, so they land
    /// here and `tick()` applies them on the next pass — see `tick`'s own
    /// comment for why a one-frame lag replaces the legacy two-pass repaint.
    tab_visible_counts: RefCell<Vec<(GroupId, usize)>>,
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
    /// protocol. Defaults to `false`, the same value `unwrap_or(false)`
    /// falls back to on any terminal that doesn't support the protocol —
    /// exactly what every `driver_with_shell` test gets, since
    /// `ShellApp::setup` only queries the real terminal when [`Self::live`]
    /// is set (see that field and `setup`'s own doc comments for why).
    keyboard_enhanced: bool,
    /// What this app believes the *runner's* `AppShell` (the
    /// `ShellAdapter`-owned instance that paints the activity bar and
    /// sidebar header — NOT `engine.app_shell`, the shadow copy
    /// `render_sidebar_content` reads) currently has as its active panel.
    /// Updated only from [`ShellApp::on_shell_event`]'s `PanelChanged`
    /// notifications — the single channel through which the runner reports
    /// its own state — and compared against the shadow's
    /// `active_panel_id()` in [`ShellApp::take_requested_panel`] to detect
    /// keyboard-/tick-driven panel switches the runner would otherwise
    /// never learn about (the #634 smoke-retry bug: sidebar content stuck
    /// on Explorer). Seeded to the hamburger — `AppShell::new` activates
    /// panel index 0, and [`Self::shell_config`] puts the hamburger first —
    /// so the very first `take_requested_panel` poll reconciles the runner
    /// onto the engine's real startup panel.
    last_shell_panel: Option<quadraui::WidgetId>,
    /// Set by [`ShellApp::take_requested_panel`] just before it returns
    /// `Some`, consumed by the `PanelChanged` arm of
    /// [`ShellApp::on_shell_event`]. `ShellAdapter::apply_requested_panel`
    /// re-notifies the app with the same `PanelChanged` a mouse click
    /// produces — but for a reconciliation echo the engine *already* holds
    /// that state (the keyboard tier that moved it also set the right
    /// focus flags), so the echo must only update [`Self::last_shell_panel`]
    /// and must NOT re-run the click path below (which would steal focus
    /// into the sidebar, e.g. `explorer_has_focus = true` on startup).
    suppress_shell_panel_echo: bool,
    /// Set by [`Self::prepare_for_live_run`], never by anything else — in
    /// particular never by a `driver_with_shell` test. Gates the two
    /// `ShellApp::setup` steps that are unsound or unsafe to run under a
    /// short-lived headless test instance (#635, Stage 6b item F):
    ///
    /// - `supports_keyboard_enhancement()` does a blocking round-trip
    ///   against the real terminal (enables raw mode if not already on,
    ///   writes a query escape sequence, and reads/polls for the
    ///   response — see crossterm's `query_keyboard_enhancement_flags_*`).
    ///   Under `driver_with_shell`'s `TestBackend` there is no real
    ///   terminal to answer, so every test using the driver would pay that
    ///   round-trip's latency (or worse, hang) for no benefit.
    /// - `core::swap::register_emergency_engine` stores a raw
    ///   `*const Engine` in a process-global `static`, on the explicit
    ///   contract that "the caller must ensure `engine` lives for the rest
    ///   of the process" (see that function's doc comment). A
    ///   `driver_with_shell` test's `TuiShellApp` is dropped at the end of
    ///   the test function — registering it would leave the global pointer
    ///   dangling for the rest of the *test binary's* process lifetime,
    ///   ready to be dereferenced by an unrelated later test's panic hook.
    ///   That's a genuine soundness bug, not just a slowdown, so this must
    ///   never run under test.
    live: bool,
}

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
            tab_visible_counts: RefCell::new(Vec::new()),
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
            completion_layout: RefCell::new(None),
            context_menu_layout: RefCell::new(None),
            dialog_layout: RefCell::new(None),
            last_sidebar_refresh: Cell::new(now),
            yank_hl_deadline: Cell::new(None),
            tab_switcher_last_cycle: Cell::new(None),
            last_shell_panel: Some(quadraui::WidgetId::new(HAMBURGER_PANEL_ID)),
            suppress_shell_panel_echo: false,
            keyboard_enhanced: false,
            live: false,
        }
    }

    fn theme(&self) -> Theme {
        Theme::from_name(&self.engine.settings.colorscheme)
    }

    /// Arm [`Self::live`] (#635, Stage 6b item F). Call exactly once,
    /// after [`Self::new`] and before handing `self` to
    /// `quadraui::tui::shell_runner::run_with_shell` (which takes
    /// ownership of `self` — never call this on an instance that's about
    /// to be dropped or moved into a `driver_with_shell` test instead).
    ///
    /// `run_with_shell(app, config)` moves `app` through several stack
    /// frames before it settles (`build_shell_adapter` → `ShellAdapter`'s
    /// own field → `tui::run::run`'s `mut app: A` local) — so a raw
    /// pointer to `self.engine` taken *before* that call would already be
    /// stale by the time anything could dereference it. `ShellApp::setup`
    /// runs from inside `tui::run::run`, after all of those moves are
    /// done and `self` has reached its final, stable address for the rest
    /// of the process — that's why the two operations this flag gates
    /// live in `setup()` rather than here or in the wrapper that calls
    /// this. This method only records the caller's intent; it doesn't do
    /// either operation itself.
    pub(super) fn prepare_for_live_run(&mut self) {
        self.live = true;
    }

    /// The live `ShellConfig` for `TuiShellApp` (#635, Stage 6b item E).
    ///
    /// The middle six panels (explorer, search, debug, source control,
    /// extensions, AI) are built by zipping
    /// `sidebar::FIXED_ACTIVITY_PANEL_IDS` — the shared order constant
    /// `render::build_activity_bar`'s (`render.rs:8147`) own `fixed` array is
    /// debug-asserted against — with this function's local icon/title/tooltip
    /// metadata array, so the *order* can't drift from `build_activity_bar`
    /// without both a compile error here (index/length mismatch) and a
    /// debug-assertion failure there. (Icon/title/tooltip strings are still a
    /// second hand-maintained copy — `PanelDefinition` and `ActivityItem` are
    /// different shapes with no shared metadata table to draw from — so a
    /// wording-only change to `build_activity_bar`'s tooltips still needs a
    /// matching edit here; only the *ordering* is now structurally shared.)
    /// Also registers the menu hamburger (top, matching its position in
    /// `build_activity_bar`'s `top` list) and settings (bottom, matching
    /// `build_activity_bar`'s `bottom` list) — the two items outside the
    /// `fixed` array. Per-session extension panels (`engine.ext_panels`)
    /// are appended dynamically by `build_activity_bar` itself and aren't
    /// representable in a static `ShellConfig`; wiring those through
    /// `AppShell::add_panel`/`remove_panel` needs a live `AppShell`
    /// instance to call them on, which nothing constructs until #634.
    ///
    /// The hamburger is a top-row `PanelDefinition`, not a bottom item, so
    /// clicking it produces `AppShellEvent::PanelChanged` the same way a
    /// real panel click does — [`ShellApp::on_shell_event`] below
    /// intercepts that and treats it as "open the menu" rather than
    /// switching the sidebar to a nonexistent "Menu" panel.
    ///
    /// `menu_bar_visible` seeds `AppShell`'s title-bar row reservation
    /// (#635, Stage 6b item A) with the engine's *construction-time* menu
    /// state (`engine.menu_bar_visible` — `true` when `is_vscode_mode()`,
    /// `false` otherwise) so the very first frame, painted before any
    /// `ShellApp::handle` dispatch gets a chance to call
    /// `ShellContext::shell_mut().set_title_bar_visible`, already reserves
    /// (or doesn't reserve) the row correctly. Every dispatch after that
    /// keeps it in sync — see the block at the *end* of `handle()` below.
    /// Always sets
    /// `title_bar_height_lh` to exactly 1 row (`draw_frame`'s own
    /// `menu_bar_height` constraint — `render_impl.rs`'s `top_chunks`) —
    /// not `AppShell::with_title_bar`'s 1.5-line-height default — so a
    /// later `set_title_bar_visible(true)` toggle (which preserves
    /// whatever height was last configured, never recomputing it) can't
    /// silently reserve the wrong row count. This reservation is the *only*
    /// row the menu bar consumes on the `render_content` path —
    /// `build_screen_for_shell_content` has no `menu_height` term of its own
    /// (see its doc comment), so 1 row here means 1 row on screen.
    /// `ShellConfig::has_title_bar`/
    /// `title_bar_height_lh` are the plain DTO fields `build_shell_adapter`
    /// (`quadraui::tui::shell_runner`) reads to decide whether to call
    /// `AppShell::with_title_bar` at construction — setting them directly
    /// here is simpler than routing through that builder twice.
    pub(super) fn shell_config(menu_bar_visible: bool) -> quadraui::ShellConfig {
        fn panel(id: &str, icon: &str, title: &str, tooltip: &str) -> quadraui::PanelDefinition {
            quadraui::PanelDefinition {
                id: quadraui::WidgetId::new(id),
                icon: icon.to_string(),
                title: title.to_string(),
                tooltip: tooltip.to_string(),
            }
        }

        // Icon/title/tooltip metadata for the fixed middle panels, in the
        // same order as `sidebar::FIXED_ACTIVITY_PANEL_IDS` — the shared
        // constant `render::build_activity_bar`'s own `fixed` array is
        // debug-asserted against, so both call sites are pinned to the same
        // order (index-zipped below, not hand-matched by id). The array
        // length is sized *from* `FIXED_ACTIVITY_PANEL_IDS::len()` itself
        // (not a hand-copied literal `6`), so adding/removing a panel there
        // is a compile error here until this array is resized to match —
        // `zip` alone would otherwise silently truncate to the shorter side.
        let mid_meta: [(&str, &str, &str);
            crate::core::engine::sidebar::FIXED_ACTIVITY_PANEL_IDS.len()] = [
            (icons::EXPLORER.s(), "Explorer", "Explorer (Ctrl+Shift+E)"),
            (icons::SEARCH.s(), "Search", "Search (Ctrl+Shift+F)"),
            (icons::DEBUG.s(), "Debug", "Debug"),
            (icons::GIT_BRANCH.s(), "Source Control", "Source Control"),
            (icons::EXTENSIONS.s(), "Extensions", "Extensions"),
            (icons::AI_CHAT.s(), "AI Assistant", "AI Assistant"),
        ];
        let mut panels = vec![panel(
            HAMBURGER_PANEL_ID,
            icons::HAMBURGER.s(),
            "Menu",
            "Menu",
        )];
        panels.extend(
            crate::core::engine::sidebar::FIXED_ACTIVITY_PANEL_IDS
                .into_iter()
                .zip(mid_meta)
                .map(|(id, (icon, title, tooltip))| panel(id, icon, title, tooltip)),
        );

        let mut cfg = quadraui::ShellConfig::new("VimCode", panels).with_bottom_items(vec![panel(
            PANEL_SETTINGS,
            icons::SETTINGS.s(),
            "Settings",
            "Settings",
        )]);

        cfg.has_title_bar = menu_bar_visible;
        cfg.title_bar_height_lh = 1.0;
        // #634: `AppShell` owns the width that carves `main_content_bounds`,
        // so its defaults have to be vimcode's, not quadraui's generic
        // 20/8/50 — otherwise the very first frame paints a 20-column
        // sidebar while every vimcode-side consumer of `self.sidebar_width`
        // (mouse hit-tests, `tick`'s viewport approximation) assumes 30, and
        // Alt+Right would silently stop at 50. The bounds match the clamps
        // `handle_key_pressed`'s Alt+Left/Right arms apply (15..=150).
        cfg.default_sidebar_width = SIDEBAR_WIDTH as f32;
        cfg.min_sidebar_width = 15.0;
        cfg.max_sidebar_width = 150.0;
        cfg
    }

    /// [`Self::shell_config`] plus the *current* set of plugin-provided
    /// extension panels (#557).
    ///
    /// `shell_config` is deliberately static — it describes the panels that
    /// exist for every session — but extension panels are per-session data
    /// living in `engine.ext_panels`, so the live runner has to derive its
    /// config from an actual `Engine`. Without this, the migrated `AppShell`
    /// activity bar painted only the seven built-ins and an extension like
    /// `git-insights` had no icon at all, even though
    /// `render::build_activity_bar` (the legacy `draw_frame` path) had been
    /// appending one since #133.
    ///
    /// Plugins can register panels at *any* time (`plugins.rs`'s
    /// `ctx.panel_registrations` drain runs after every Lua callback), so this
    /// only seeds frame zero — [`Self::sync_ext_activity_panels`] keeps the
    /// live `AppShell` converged from there.
    pub(super) fn live_shell_config(engine: &Engine) -> quadraui::ShellConfig {
        let mut cfg = Self::shell_config(engine.menu_bar_visible);
        cfg.panels.extend(engine.ext_activity_panels());
        cfg
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
                // #637: the event landed inside this panel's own body rect —
                // it must be claimed here unconditionally, even when the
                // inner `SidebarEvent` comes back `Ignored` (e.g. a click on
                // empty space below the last row, or between two headers
                // when the list is empty). Only returning `Redraw` on a
                // "successful" dispatch let an `Ignored` result fall through
                // to `mouse::handle_mouse`'s unrelated legacy dispatcher,
                // which interprets the *same* coordinates under a totally
                // different column-range model and can silently reset focus
                // this intercept just claimed (reproduced by
                // `debug_sidebar_intercept_claims_focus_on_mouse_down` /
                // `ext_sidebar_intercept_claims_focus_on_mouse_down`, which
                // hit exactly this path whenever the panel's list is empty).
                self.engine.dispatch_dap_sidebar_event(sidebar_event);
                return Reaction::Redraw;
            }
        }

        // ── SidebarSystem intercept: extensions sidebar (mirrors `mod.rs`
        // ~1473-1498). Not redundant with `handle_mouse`'s own
        // `PANEL_EXTENSIONS` arm — that arm explicitly declines rows 2+
        // ("handled by SidebarSystem mouse intercept in main loop",
        // `mouse.rs` ~2557), so skipping this would silently drop those
        // clicks.
        //
        // Also requires `self.sidebar.ext_panel_name.is_none()` (#637): a
        // plugin-provided extension panel (`render_ext_panel`,
        // `sidebar.ext_panel_name`) takes over the sidebar body without
        // touching `app_shell`'s active-panel id, so `active_panel_is(
        // PANEL_EXTENSIONS)` can still read true from a *previous* visit to
        // the Extensions marketplace panel while a plugin panel is what's
        // actually on screen. Without this guard, `ext_sidebar_body_rect` —
        // last populated when the marketplace panel was painted, and never
        // cleared when it stops being painted (`panels.rs::render_sidebar`
        // returns early for `ext_panel_name.is_some()` before ever touching
        // it) — goes stale but keeps `rect.contains(position)` matching the
        // same on-screen sidebar area, so every click/scroll meant for the
        // plugin panel gets silently swallowed by the marketplace intercept
        // instead. Mirrors the existing `ext_panel_showing` guard
        // `mouse.rs`'s raw scroll-wheel handler already uses for the same
        // reason (`mouse.rs` ~1244). ──
        if !intercepts_blocked
            && self.engine.app_shell.sidebar_visible()
            && self.engine.active_panel_is(PANEL_EXTENSIONS)
            && self.sidebar.ext_panel_name.is_none()
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
                // #637: see the matching comment on the debug-sidebar
                // intercept above — claim unconditionally once the event is
                // inside this panel's rect, regardless of dispatch result.
                self.engine.handle_ext_sidebar_ui_event(event.clone());
                return Reaction::Redraw;
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

    /// #557: reconcile the runner `AppShell`'s top-panel list with
    /// `engine.ext_panels` so plugin-provided panels contribute an
    /// activity-bar icon.
    ///
    /// [`Self::live_shell_config`] seeds frame zero, but a plugin can register
    /// (or a `:PluginReload` can drop) a panel at any point in the session —
    /// `Engine::apply_plugin_ctx` drains `ctx.panel_registrations` after every
    /// Lua callback — so the list has to be re-derived rather than assumed
    /// static. Called unconditionally on the way out of every dispatch, like
    /// the title-bar/width/visibility syncs beside it; the early return makes
    /// the steady state (nothing changed) a cheap id+icon comparison and no
    /// `AppShell` mutation at all.
    ///
    /// Reconciles by *rebuilding* the `"ext:"`-prefixed tail rather than
    /// diffing element-wise: the desired list is sorted by name, so a newly
    /// registered panel can land anywhere in it, and `AppShell::add_panel`
    /// only ever appends. Rebuilding keeps painted order == the sort order
    /// `Engine::activity_bar_activate`'s keyboard indices assume. Built-in
    /// panels never match the prefix and are left untouched, so the rebuild
    /// can't disturb the fixed part of the bar.
    fn sync_ext_activity_panels(&self, ctx: &ShellContext<'_>) {
        use crate::core::engine::sidebar::EXT_PANEL_ID_PREFIX;

        let desired = self.engine.ext_activity_panels();
        let is_ext = |p: &quadraui::PanelDefinition| p.id.as_str().starts_with(EXT_PANEL_ID_PREFIX);
        let unchanged = {
            let shell = ctx.shell();
            let current: Vec<_> = shell
                .panels()
                .iter()
                .filter(|p| is_ext(p))
                .map(|p| (p.id.as_str(), p.icon.as_str()))
                .collect();
            current.len() == desired.len()
                && current
                    .iter()
                    .zip(desired.iter())
                    .all(|((id, icon), d)| *id == d.id.as_str() && *icon == d.icon.as_str())
        };
        if unchanged {
            return;
        }

        let mut shell = ctx.shell_mut();
        let stale: Vec<quadraui::WidgetId> = shell
            .panels()
            .iter()
            .filter(|p| is_ext(p))
            .map(|p| p.id.clone())
            .collect();
        for id in stale {
            shell.remove_panel(&id);
        }
        for def in desired {
            shell.add_panel(def);
        }

        // Re-assert the highlight: removing the active panel clears/clamps
        // `AppShell::active_panel`, so an open extension panel would otherwise
        // lose its icon highlight (and the sidebar header its title) on any
        // rebuild. `take_requested_panel` can't cover this — it compares
        // against `last_shell_panel`, which already holds this id.
        if self.engine.app_shell.sidebar_visible() {
            if let Some(name) = self.engine.ext_panel_active.as_deref() {
                shell.show_panel(&quadraui::WidgetId::new(
                    crate::core::engine::sidebar::ext_panel_id(name),
                ));
            }
        }
    }

    /// #557: open the plugin-provided panel `name` in the sidebar.
    ///
    /// Verbatim mirror of `mouse::handle_mouse`'s
    /// `ActivityBarTarget::ExtensionPanel` "open" branch minus its
    /// toggle-vs-open decision — on the `ShellApp` path `AppShell` has already
    /// made that call and reports a *second* click on the open panel's icon as
    /// [`quadraui::AppShellEvent::SidebarHidden`], not `PanelChanged`.
    ///
    /// The `clear_sidebar_focus()` first is the #637 fix: a plugin panel
    /// taking over the sidebar body has to drop whatever built-in panel's
    /// focus flag was left set, or e.g. a stale `ext_sidebar_has_focus` from
    /// an earlier Extensions-marketplace visit keeps claiming clicks meant for
    /// this panel.
    fn activate_ext_panel(&mut self, name: &str) {
        self.engine.clear_sidebar_focus();
        self.sidebar.ext_panel_name = Some(name.to_string());
        if !self.engine.app_shell.sidebar_visible() {
            self.engine.toggle_sidebar();
        }
        self.sidebar.has_focus = true;
        self.engine.ext_panel_active = Some(name.to_string());
        self.engine.ext_panel_has_focus = true;
        self.engine.ext_panel_selected = 0;
        self.engine.plugin_event("panel_focus", name);
        self.engine.session.explorer_visible = self.engine.app_shell.sidebar_visible();
        let _ = self.engine.session.save();
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

        // ── #635 (Stage 6b item F): live-only setup ──────────────────────
        // Gated on `self.live` (set only by `Self::prepare_for_live_run`,
        // called only by the live `super::run` wrapper) — see
        // that field's doc comment for why running either of these under
        // `driver_with_shell` would be a real bug (a blocking terminal
        // round-trip, and an unsound dangling-pointer registration),
        // not just redundant work.
        if self.live {
            // Mirrors `event_loop`'s once-computed `keyboard_enhanced`
            // query (`mod.rs:696`) — same call, same fallback, just moved
            // to run once here instead of in the wrapper, since by this
            // point `self` has reached the stable address the SAFETY note
            // below also depends on.
            self.keyboard_enhanced = supports_keyboard_enhancement().unwrap_or(false);

            // SAFETY: `run_with_shell` → `build_shell_adapter` →
            // `tui::run::run`'s own `mut app: A` local is what finally
            // owns `self` for the rest of the process — `run_inner` (which
            // calls this `setup`) only ever touches it through `&mut app`
            // from here on, so `self`, and therefore `&self.engine`, is at
            // its final address. `self.live` is `true` only when
            // `super::run` called `Self::prepare_for_live_run`
            // immediately before moving `self` into `run_with_shell` — see
            // that method's doc comment — so `self.engine` living for the
            // rest of the process (this fn's safety contract) holds.
            unsafe {
                crate::core::swap::register_emergency_engine(&self.engine as *const _);
            }
        }
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

        // ── Per-frame nerd-font sync (mirrors mod.rs:1131) ───────────────
        // `setup()` pushes the flag once; the legacy loop re-pushed it every
        // frame so a runtime `:set nerdfonts` / `:set nonerdfonts` reaches
        // the rasterisers on the very next paint instead of never.
        render::sync_nerd_fonts(backend, &self.engine);

        // ── Menu bar + command centre (#635, Stage 6b item A) ────────────
        // Mirrors `draw_frame`'s own `menu_bar_area` block
        // (`render_impl.rs`, the `screen.menu_bar_visible` block right
        // after `top_chunks`). `AppShell::set_title_bar_visible`
        // (quadraui#532, synced from `engine.menu_bar_visible` by
        // `handle()`/seeded by `Self::shell_config` — see their doc
        // comments) is what makes `layout.title_bar_bounds` `Some` in the
        // first place; when the menu is hidden the shell never reserved
        // the row, so there's nothing to paint. `draw_menu_bar` and
        // `draw_command_center` were already trait calls — the gap this
        // stage closes was purely the missing runtime-toggleable reserved
        // row, not these calls themselves. The menu *dropdown* (the open
        // popup, as opposed to this horizontal bar) paints separately,
        // last, near the end of this function — see that block's comment
        // for why.
        //
        // #695: `layout.title_bar_bounds` is cached into
        // `engine.menu_bar_rect` unconditionally (mirrors GTK's
        // `self.menu_row_rect.set(...)`, `gtk/mod.rs:8299`-`:8300`) *before*
        // gating on `menu_bar_visible`, so the cache always reflects exactly
        // what the shell reserved this frame — empty when nothing was
        // reserved. `handle()`'s MenuSystem intercept and every `mouse.rs`
        // hit test now read this one value instead of separately
        // re-deriving "is there a menu-bar row" from `menu_bar_visible`
        // alone, which is what let paint and hit-test disagree (#695).
        let menu_bar_rect = layout.title_bar_bounds.unwrap_or_default();
        self.engine.menu_bar_rect.set(menu_bar_rect);
        if self.engine.menu_bar_visible {
            if menu_bar_rect.width >= 1.0 && menu_bar_rect.height >= 1.0 {
                let tb_area = Rect {
                    x: menu_bar_rect.x.round() as u16,
                    y: menu_bar_rect.y.round() as u16,
                    width: menu_bar_rect.width.round() as u16,
                    height: menu_bar_rect.height.round() as u16,
                };
                backend.set_theme(super::quadraui_tui::q_theme(&theme));
                let bar = self.engine.menu_system.borrow().menu_bar();
                let bar_rect = quadraui::Rect::new(
                    tb_area.x as f32,
                    tb_area.y as f32,
                    tb_area.width as f32,
                    tb_area.height as f32,
                );
                let mb_layout = backend.draw_menu_bar(bar_rect, &bar);

                let menu_end: u16 = mb_layout
                    .visible_items
                    .last()
                    .map(|vi| tb_area.x + (vi.bounds.x + vi.bounds.width).round() as u16)
                    .unwrap_or(tb_area.x);

                let title = self
                    .engine
                    .cwd
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "VimCode".to_string());
                let cc = render::build_command_center_view(
                    self.engine.tab_nav_can_go_back(),
                    self.engine.tab_nav_can_go_forward(),
                    &title,
                );
                let cc_area = Rect {
                    x: menu_end,
                    y: tb_area.y,
                    width: tb_area.width.saturating_sub(menu_end - tb_area.x),
                    height: tb_area.height,
                };
                let cc_q_rect = quadraui::Rect::new(
                    cc_area.x as f32,
                    cc_area.y as f32,
                    cc_area.width as f32,
                    cc_area.height as f32,
                );
                let cc_layout = backend.draw_command_center(cc_q_rect, &cc);
                self.engine.command_center_layout.replace(Some(cc_layout));
            }
        } else {
            self.engine.command_center_layout.replace(None);
        }

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
        let tab_bar_targets = render::tab_bar_draw_targets(&self.engine, &screen, 1.0, tui_tbh);
        {
            // Reset per-frame: `post_draw_apply_widths` wants this frame's
            // measurements, not an ever-growing accumulation.
            let mut counts = self.tab_visible_counts.borrow_mut();
            counts.clear();
            for target in &tab_bar_targets {
                let g_tab = Rect {
                    x: target.rect.x as u16,
                    y: target.rect.y as u16,
                    width: target.rect.width as u16,
                    height: 1,
                };
                let vis = render_tab_bar(backend, g_tab, target.bar, &theme);
                counts.push((target.group_id, vis));
            }
        }
        for t in render::breadcrumb_draw_targets(&screen, self.engine.terminal_maximized) {
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
        // only present when the editor is split into multiple groups, which
        // `screen.group_dividers` expresses by simply being empty otherwise,
        // so no `editor_group_split.is_some()` gate is needed (#551).
        // Mirrors `draw_frame`'s own divider block, ported to
        // `Backend::draw_status_bar` via `render_group_dividers` (see its
        // doc comment, and `group_divider_cells`'s for how the #481
        // phantom-divider-beside-scrollbar guard became a pure data
        // computation instead of a `Buffer` read-back).
        render_group_dividers(
            backend,
            &screen.group_dividers,
            &screen.windows,
            area,
            &theme,
        );

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
        // That offset already includes `AppShell`'s title-bar row whenever
        // the menu bar is visible (`compute_layout`'s `band_y += h`), so —
        // unlike `draw_frame`'s `menu_rows + 1` — there is no menu term to
        // add here; adding one would double-count the row (#635 item A, and
        // see `build_screen_for_shell_content`'s doc comment).
        if let Some(ref tooltip_text) = screen.tab_tooltip {
            render_tab_hover_tooltip(
                backend,
                area.x,
                area.y + 1,
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

        // ── Separated status line (#605) ─────────────────────────────────
        // Shown above the terminal panel when `window_status_line` is on but
        // `status_line_above_terminal` is off — `render_window_status_line`
        // was already trait-pure (#601 widened it), so this is a straight
        // port of `draw_frame`'s own block.
        if let Some(ref status) = screen.separated_status_line {
            render_window_status_line(
                backend,
                chrome.separated_status.x,
                chrome.separated_status.y,
                chrome.separated_status.width,
                status,
                &theme,
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

        // ─────────────────────────────────────────────────────────────────
        // #605 (Stage 6 parity sweep): the rest of `draw_frame`'s tail, in
        // its exact paint order. Everything below was already a
        // `Backend::draw_*` trait call on the live path (the one exception,
        // the command line, is now trait-pure too — see
        // `panels::render_command_line`), so these are straight ports, not
        // reimplementations.
        //
        // Screen-anchored overlays (modals, toasts, the panel hover popup)
        // deliberately use `layout.window_bounds` — the *whole* terminal —
        // rather than `area` (`main_content_bounds`, the editor column).
        // `draw_frame` centres them on `frame.area()`, so anchoring them to
        // the editor column instead would shift every modal right by the
        // activity-bar + sidebar width.
        let win = layout.window_bounds;
        let win_area = Rect {
            x: win.x.round() as u16,
            y: win.y.round() as u16,
            width: win.width.round() as u16,
            height: win.height.round() as u16,
        };

        // ── Debug toolbar strip ──────────────────────────────────────────
        if screen.debug_toolbar.is_some() {
            let q_rect = quadraui::Rect::new(
                chrome.debug_toolbar.x as f32,
                chrome.debug_toolbar.y as f32,
                chrome.debug_toolbar.width as f32,
                chrome.debug_toolbar.height as f32,
            );
            self.debug_toolbar_rect.set(q_rect);
            render::draw_debug_toolbar(backend, &self.engine, q_rect);
        }

        // ── Wildmenu bar (command Tab completion) ────────────────────────
        if let Some(ref wm) = screen.wildmenu {
            let bar = render::wildmenu_to_status_bar(wm, &theme);
            let q_rect = quadraui::Rect::new(
                chrome.wildmenu.x as f32,
                chrome.wildmenu.y as f32,
                chrome.wildmenu.width as f32,
                chrome.wildmenu.height as f32,
            );
            backend.draw_status_bar(q_rect, &bar, None, None);
        }

        // ── Global status bar ────────────────────────────────────────────
        if let Some(ref bar) = screen.global_status_bar {
            let q_rect = quadraui::Rect::new(
                chrome.status.x as f32,
                chrome.status.y as f32,
                chrome.status.width as f32,
                chrome.status.height as f32,
            );
            backend.draw_status_bar(q_rect, bar, None, None);
        }

        // ── Command line (+ mouse drag-selection inversion) ──────────────
        render_command_line(
            backend,
            chrome.cmd,
            &screen.command,
            &theme,
            self.cmd_sel.get(),
        );

        // ── Panel hover popup ────────────────────────────────────────────
        // Anchored just right of the sidebar's own right edge, which in the
        // shell layout is exactly `main_content_bounds.x` (`AppShell` puts
        // the resize divider between them and `area.x` is the first column
        // past it) — the same column `draw_frame` computes as `sep_x + 1`.
        {
            let mut rects = self.hover_link_rects.borrow_mut();
            rects.clear();
            self.hover_popup_rect.set(None);
            if let Some(sb) = layout.sidebar_content_bounds {
                if self.engine.app_shell.sidebar_visible()
                    && (self.sidebar.ext_panel_name.is_some()
                        || self.engine.active_panel_is(PANEL_GIT))
                {
                    let (new_rects, popup_rect) = render_panel_hover_popup(
                        backend,
                        &screen,
                        &theme,
                        area.x,
                        sb.y.round() as u16,
                        sb.height.round() as u16,
                        win_area,
                    );
                    *rects = new_rects;
                    self.hover_popup_rect.set(popup_rect);
                }
            }
        }

        // ── Folder / workspace picker modal ──────────────────────────────
        if let Some(ref picker) = self.folder_picker {
            // Sizing identical to `draw_frame`'s: 60% of viewport width
            // clamped to >= 50; 55% of viewport height clamped to >= 15.
            let width = (win_area.width * 3 / 5).max(50);
            let height = (win_area.height * 55 / 100).max(15);
            let popup_x = win_area.x + (win_area.width.saturating_sub(width)) / 2;
            let popup_y = win_area.y + (win_area.height.saturating_sub(height)) / 2;
            let palette = folder_picker_to_palette(picker, width as usize);
            let q_rect =
                quadraui::Rect::new(popup_x as f32, popup_y as f32, width as f32, height as f32);
            backend.draw_palette(q_rect, &palette);
        }

        // ── Find/replace overlay ─────────────────────────────────────────
        // `find_replace.group_bounds` is already absolute terminal-screen
        // space (#550), so the rect passed here only supplies the clip
        // viewport — see `draw_frame`'s longer comment on the `editor_left`
        // no-op translation.
        if let Some(ref find_replace) = screen.find_replace {
            let q_area = quadraui::Rect::new(
                win_area.x as f32,
                win_area.y as f32,
                win_area.width as f32,
                win_area.height as f32,
            );
            backend.draw_find_replace(q_area, find_replace);
        }

        // ── Unified picker modal ─────────────────────────────────────────
        if let Some(ref picker) = screen.picker {
            render_picker_popup(picker, win_area, &theme, backend);
        }

        // ── Tab switcher popup ───────────────────────────────────────────
        if let Some(ref ts) = screen.tab_switcher {
            if !ts.items.is_empty() {
                let width = (win_area.width * 45 / 100).clamp(40, 80);
                let max_visible = (win_area.height as usize).saturating_sub(4).min(20);
                let visible = ts.items.len().min(max_visible);
                let height = visible as u16 + 2;
                let x = win_area.x + (win_area.width.saturating_sub(width)) / 2;
                let y = win_area.y + (win_area.height.saturating_sub(height)) / 2;
                let list = render::tab_switcher_to_quadraui_list_view(ts, max_visible);
                let q_rect = quadraui::Rect::new(x as f32, y as f32, width as f32, height as f32);
                backend.draw_list(q_rect, &list);
            }
        }

        // ── Context menu popup ───────────────────────────────────────────
        // Painting this also closes the residual #602 seam noted in the
        // module doc: `handle_mouse` receives `context_menu_layout` from the
        // cell written here, so a click on a menu item now resolves to that
        // item instead of falling through to "close the menu".
        if let Some(ref ctx_menu) = screen.context_menu {
            let inner_viewport = quadraui::Rect::new(
                (win_area.x + 1) as f32,
                (win_area.y + 1) as f32,
                win_area.width.saturating_sub(2) as f32,
                win_area.height.saturating_sub(2) as f32,
            );
            let inset_panel = render::ContextMenuPanel {
                screen_col: ctx_menu.screen_col + 1,
                screen_row: ctx_menu.screen_row + 1,
                ..ctx_menu.clone()
            };
            let (menu, menu_layout) =
                render::context_menu_generic_layout(&inset_panel, inner_viewport, 1.0, 1.0, 1.0);
            let _ = backend.draw_context_menu(&menu, &menu_layout);
            *self.context_menu_layout.borrow_mut() = Some(menu_layout);
        } else {
            *self.context_menu_layout.borrow_mut() = None;
        }

        // ── Modal dialog ─────────────────────────────────────────────────
        if let Some(ref dialog) = screen.dialog {
            let viewport = quadraui::Rect::new(
                win_area.x as f32,
                win_area.y as f32,
                win_area.width as f32,
                win_area.height as f32,
            );
            let (q_dialog, dlg_layout) = render::dialog_generic_layout(dialog, viewport, 1.0, 1.0);
            let _ = backend.draw_dialog(&q_dialog, &dlg_layout);
            *self.dialog_layout.borrow_mut() = Some(dlg_layout);
        } else {
            *self.dialog_layout.borrow_mut() = None;
        }

        // ── Menu dropdown (#635, Stage 6b item A) — rendered after the
        // dialog so it draws on top of everything below it, mirroring
        // `draw_frame`'s own "rendered last so it draws on top of
        // everything" comment on this exact block (`render_impl.rs`, right
        // before the toast overlay). `MenuSystem::render` was already a
        // trait call; the only reason this couldn't paint before was the
        // same missing `layout.title_bar_bounds` reservation the menu bar
        // block above now provides. #695: read the same
        // `engine.menu_bar_rect` cache the bar-paint block above just wrote,
        // rather than re-reading `layout.title_bar_bounds` a second time —
        // one write, every reader downstream (paint and hit-test alike)
        // consumes it verbatim.
        if self.engine.menu_bar_visible {
            let bar_rect = self.engine.menu_bar_rect.get();
            if bar_rect.width >= 1.0 && bar_rect.height >= 1.0 {
                self.engine.menu_system.borrow().render(backend, bar_rect);
            }
        }

        // ── Toast overlay (#450) — last, so it sits on top of everything ─
        if let Some(stack) = render::build_toast_stack(&self.engine) {
            let q_toast_area = quadraui::Rect::new(
                win_area.x as f32,
                win_area.y as f32,
                win_area.width as f32,
                win_area.height as f32,
            );
            let toast_layout = backend.draw_toast_stack(q_toast_area, &stack);
            self.engine.toast_layout.replace(Some(toast_layout));
        } else {
            self.engine.toast_layout.replace(None);
        }

        // ── Cache the painted layout for mouse hit-testing (#634) ────────
        // `event_loop` stashed `build_screen_for_tui`'s result in its
        // `last_layout` local before drawing (`mod.rs:1133`) and passed it
        // to `mouse::handle_mouse` on the next mouse event. Without this,
        // `handle_mouse` gets `None` and every layout-dependent hit test
        // (click-to-position, tab-bar clicks, gutter, quickfix, find/replace,
        // terminal geometry) silently does nothing. Written last, once every
        // borrow of `screen` above has ended, so the value can simply be
        // moved in rather than cloned.
        //
        // Also mirrors `mod.rs:1164`-`:1169`'s popup-disappearance tracking.
        // The legacy loop followed it with `terminal.clear()`; the shell
        // runner owns the `Terminal` and exposes no repaint hook, so the flag
        // is kept (cheap, and the state it records is real) while the clear
        // itself is an upstream gap — see the Ctrl+L note in
        // `handle_key_pressed`.
        self.had_popup_overlay
            .set(screen.picker.is_some() || self.folder_picker.is_some());
        *self.last_layout.borrow_mut() = Some(screen);
    }

    fn handle(
        &mut self,
        event: UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &ShellContext<'_>,
    ) -> Reaction {
        // The dispatch below has several early exits; a labelled block (not
        // bare `return`s) is what keeps the title-bar sync that follows
        // reachable on *every* one of them, including any arm added later.
        let reaction = 'dispatch: {
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
                        break 'dispatch if needs_redraw {
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
            // #695: gate mirrors GTK's `menu_bar_visible || menu_system.is_open()`
            // (`gtk/mod.rs:9671`) rather than `menu_bar_visible` alone. Without
            // the `is_open()` arm, hiding the bar (`:set nomenu`, or the #318
            // Alt-letter shim's reveal never firing) while a dropdown is still
            // open would leave it painted — `render_content`'s dropdown block
            // above paints on `menu_bar_visible` too, so in practice the two
            // states track together — but unclickable the instant they ever
            // don't, since this intercept is what routes clicks/keys into it.
            // `bar_rect` reads the same `engine.menu_bar_rect` cache
            // `render_content` just wrote this frame (mirrors GTK's
            // `self.menu_row_rect.get()`, `gtk/mod.rs:9672`) instead of
            // hardcoding a `(0, 0, viewport.width, 1)` guess that only matched
            // by coincidence — the shell's title-bar band isn't always at the
            // screen origin (sidebar/activity-bar reservations, multi-row
            // bands), so a hardcoded rect silently drifts the moment that
            // assumption stops holding.
            //
            // Exception: the #318 Alt-letter shim just above can flip
            // `menu_bar_visible` from false to true and this intercept can
            // fire in the *same* dispatch — before `render_content` ever
            // runs again to refresh the cache, so `menu_bar_rect` can still
            // hold the stale empty rect from the last frame the bar was
            // hidden. Handing quadraui's `MenuSystem::handle` an empty rect
            // makes it lay out zero visible items and then index into that
            // empty list assuming at least one fits — a panic, not a no-op
            // (regression caught by `alt_letter_reveals_menu_bar_via_shell_app`).
            // Fall back to the same full-width single-row rect the paint
            // path below always uses (`title_bar_height_lh = 1.0`,
            // `Self::shell_config`'s doc comment) for exactly this one
            // just-revealed-but-not-yet-painted frame.
            //
            // Checks both `width` and `height` against the cached rect —
            // matching the paint block above's own `bar_rect.width >= 1.0
            // && bar_rect.height >= 1.0` guard exactly (review, #695
            // iteration 1) rather than `height` alone. A `title_bar_bounds`
            // that were ever `Some` with zero width but non-zero height
            // (e.g. a viewport whose computed width collapses to 0) would
            // otherwise hand `MenuSystem::handle` a real-but-zero-width
            // rect instead of falling back to the full-viewport-width one,
            // risking the same "lay out zero visible items, then index into
            // that empty list" panic the same-frame case above is guarded
            // against — while paint itself would have skipped drawing
            // anything for that frame.
            if self.engine.menu_bar_visible || self.engine.menu_system.borrow().is_open() {
                let viewport = backend.viewport();
                let cached_bar_rect = self.engine.menu_bar_rect.get();
                let bar_rect = if cached_bar_rect.width >= 1.0 && cached_bar_rect.height >= 1.0 {
                    cached_bar_rect
                } else {
                    quadraui::Rect::new(0.0, 0.0, viewport.width, 1.0)
                };
                let menu_system = self.engine.menu_system.clone();
                let menu_event = menu_system.borrow_mut().handle(&event, backend, bar_rect);
                match menu_event {
                    quadraui::MenuEvent::Activated(id) => {
                        let action = id.as_str().to_string();
                        if action == "open_file_dialog" {
                            self.engine
                                .open_picker(crate::core::engine::PickerSource::Files);
                        } else {
                            // #634: `dispatch_menu_action` returns an
                            // `EngineAction` the engine can't complete on its
                            // own (it needs terminal size / TUI-local state).
                            // Dropping it — as this arm did while dormant —
                            // made File▸Quit, File▸Open Folder, File▸Recent,
                            // Save Workspace As and Terminal▸New all no-ops.
                            // Mirrors `mod.rs:1450`-`:1498`.
                            let act = self.engine.dispatch_menu_action(&action);
                            let cols = viewport.width as u16;
                            let rows = self.engine.session.terminal_panel_rows;
                            match act {
                                EngineAction::OpenTerminal => {
                                    self.engine.terminal_new_tab(cols, rows);
                                }
                                EngineAction::RunInTerminal(cmd) => {
                                    self.engine.terminal_run_command(&cmd, cols, rows);
                                }
                                EngineAction::OpenFolderDialog => {
                                    self.folder_picker = Some(FolderPickerState::new(
                                        &self.engine.cwd.clone(),
                                        FolderPickerMode::OpenFolder,
                                        self.engine.settings.show_hidden_files,
                                    ));
                                }
                                EngineAction::OpenWorkspaceDialog => {
                                    self.sidebar = TuiSidebar::new();
                                    self.engine.explorer_rebuild_rows();
                                }
                                EngineAction::SaveWorkspaceAsDialog => {
                                    let ws_path = self.engine.cwd.join(".vimcode-workspace");
                                    self.engine.save_workspace_as(&ws_path);
                                }
                                EngineAction::OpenRecentDialog => {
                                    // #274: engine-driven picker; replaces
                                    // the TUI-local
                                    // `FolderPickerState::new_recent`.
                                    if self.engine.session.recent_workspaces.is_empty() {
                                        self.engine.message = "No recent workspaces".to_string();
                                    } else {
                                        self.engine.open_picker(
                                            crate::core::engine::PickerSource::RecentWorkspaces,
                                        );
                                    }
                                }
                                EngineAction::QuitWithUnsaved => {
                                    self.engine.show_quit_confirm();
                                }
                                act => {
                                    if handle_action(&mut self.engine, act) {
                                        break 'dispatch Reaction::Exit;
                                    }
                                }
                            }
                        }
                        break 'dispatch Reaction::Redraw;
                    }
                    quadraui::MenuEvent::StateChanged | quadraui::MenuEvent::Consumed => {
                        break 'dispatch Reaction::Redraw;
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
                    // The debug/DAP sidebar tier re-dispatches the *event*
                    // (not the decoded key) into `SidebarSystem::handle` —
                    // `event_loop` kept a `ui_event_saved` clone for exactly
                    // this (`mod.rs:1745`). Rebuilt rather than cloned up
                    // front so non-key events pay nothing.
                    let dap_event = UiEvent::KeyPressed {
                        key: key.clone(),
                        modifiers,
                        repeat,
                    };
                    handle_key_pressed(
                        key,
                        modifiers,
                        repeat,
                        &mut self.engine,
                        &mut self.sidebar,
                        &mut self.folder_picker,
                        self.keyboard_enhanced,
                        viewport.width as u16,
                        viewport.height as u16,
                        backend,
                        &mut KeyDispatchState {
                            sidebar_width: &mut self.sidebar_width,
                            quickfix_scroll_top: &mut self.quickfix_scroll_top,
                            last_clipboard_content: &mut self.last_clipboard_content,
                            cmd_sel: &self.cmd_sel,
                            yank_hl_deadline: &self.yank_hl_deadline,
                            ui_event: &dap_event,
                        },
                    )
                }
                // ── Bracketed paste (mirrors mod.rs:3032-:3035) ─────────
                // The runner maps crossterm's `Event::Paste` to
                // `UiEvent::ClipboardPaste`; without this arm a paste into
                // the TUI is silently dropped.
                UiEvent::ClipboardPaste(ref text) => {
                    self.engine.route_paste(text);
                    sync_tui_clipboard(&mut self.engine, &mut self.last_clipboard_content);
                    Reaction::Redraw
                }
                // ── Resize → PTY resize (mirrors mod.rs:3036-:3045) ─────
                // The runner already debounces the crossterm resize burst
                // (`RESIZE_SETTLE`) and re-reads the real terminal size for
                // painting every frame, so only the embedded shell's own
                // SIGWINCH needs forwarding here. The legacy loop's
                // accompanying `terminal.clear()` has no shell-runner
                // equivalent — see the Ctrl+L note in `handle_key_pressed`.
                UiEvent::WindowResized { viewport } => {
                    let term_rows = self.engine.session.terminal_panel_rows;
                    self.engine
                        .terminal_resize(viewport.width as u16, term_rows);
                    Reaction::Redraw
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
        };

        // ── #635 (Stage 6b item A): keep `AppShell`'s title-bar row
        // reservation in sync with `engine.menu_bar_visible` ────────────────
        // `layout.title_bar_bounds` (read by `render_content` above) is
        // computed by the shell runner from the *real*, `ShellAdapter`-owned
        // `AppShell` — not from anything on `self` — so toggling
        // `engine.menu_bar_visible` (the Alt+menu-letter shim above,
        // `dispatch_panel_accelerator_sizeless`, `:set menu`, ...) has no
        // effect on the painted layout unless it's also pushed through
        // `ShellContext::shell_mut()`. Doing this unconditionally — rather
        // than only in the specific arms that change the flag — is what
        // `AppShell::set_title_bar_visible`'s own doc comment recommends
        // (quadraui#532): "toggling this and calling [layout/render] next is
        // sufficient". `Self::shell_config` seeds the *first* frame (painted
        // before any `handle` call) from the same flag at construction time,
        // so this and that stay in lockstep from frame zero.
        //
        // Runs *after* the dispatch, not before it (where #635's first cut
        // put it): the runner renders as soon as `handle` returns, so syncing
        // on the way out lets a reveal performed *by this very event* —
        // Alt+F, `:set menu` — reserve and paint its row on the resulting
        // frame instead of lagging behind until some later, unrelated
        // keypress happens to run the sync. Since the reservation is now the
        // *only* menu-bar row (`build_screen_for_shell_content` deliberately
        // has no `menu_height` term of its own — see its doc comment), a
        // pre-dispatch sync would have meant Alt+F painting no menu bar at
        // all until the next keystroke.
        //
        // The one path this method can never reach is `on_shell_event`'s
        // hamburger reveal: `ShellAdapter::handle` consumes
        // `AppShellEvent::PanelChanged` itself and returns without ever
        // calling this method. #693: that path now runs its own copy of
        // this same sync from [`Self::on_shell_event_ctx`] (the
        // `ShellContext`-aware notification quadraui#617 added), so it no
        // longer waits for a later, unrelated dispatch to land it.
        ctx.shell_mut()
            .set_title_bar_visible(self.engine.menu_bar_visible);

        // ── Keep `AppShell`'s sidebar width == `self.sidebar_width` (#634) ─
        // Same problem, same shape as the title-bar sync above: `AppShell`
        // owns the width that carves `main_content_bounds` (and therefore
        // everything `render_content` paints), while `self.sidebar_width` is
        // what `mouse::handle_mouse`'s column math, `tick`'s viewport
        // approximation and `dispatch_panel_accelerator_sizeless` all read.
        // `event_loop` had one variable for both. Without this push, Alt+Left
        // / Alt+Right and a sidebar-divider drag would move vimcode's copy
        // and leave the painted layout unchanged, so hit tests would land a
        // few columns off the visible edge. `set_sidebar_width` clamps
        // internally and is idempotent, so calling it unconditionally on the
        // way out of every dispatch costs nothing.
        ctx.shell_mut().set_sidebar_width(self.sidebar_width as f32);

        // ── #557: keep the runner `AppShell`'s extension-panel icons ==
        // `engine.ext_panels` ───────────────────────────────────────────────
        // Same split-state shape as the three syncs around it, except the
        // drift here is in the panel *list* rather than a scalar. Runs before
        // the visibility sync below so a `show_panel` for a freshly-registered
        // extension panel can actually find it.
        self.sync_ext_activity_panels(ctx);

        // ── #634 smoke retry: keep the runner `AppShell`'s sidebar
        // *visibility* == the shadow's ──────────────────────────────────────
        // Same split-state problem as the title-bar and width syncs above:
        // every keyboard path (Ctrl+B-style toggles, `toggle_sidebar_panel`
        // via panel accelerators, autohide, Ctrl+W overflow) mutates
        // `engine.app_shell` — the shadow — while the runner's `AppShell`
        // owns whether `sidebar_content_bounds` exists at all. `event_loop`
        // had one instance for both. The active-*panel* half of this sync
        // lives in `take_requested_panel` (which also covers tick-driven
        // switches — this method never runs for those); visibility has no
        // equivalent adapter hook, so it's pushed here on the way out of
        // every dispatch, unconditionally and idempotently.
        let shadow_visible = self.engine.app_shell.sidebar_visible();
        let runner_visible = ctx.shell().sidebar_visible();
        if runner_visible != shadow_visible {
            if shadow_visible {
                // #557: while a plugin panel is open the shadow's
                // active-panel id still names the built-in that preceded it
                // (extension panels never touch it), so reveal *that* panel
                // and the runner's highlight jumps off the extension icon —
                // same reason `take_requested_panel` prefers
                // `ext_panel_active`.
                let id = self
                    .engine
                    .ext_panel_active
                    .as_deref()
                    .map(|n| quadraui::WidgetId::new(crate::core::engine::sidebar::ext_panel_id(n)))
                    .or_else(|| self.engine.app_shell.active_panel_id().cloned());
                if let Some(id) = id {
                    ctx.shell_mut().show_panel(&id);
                }
                // `AppShell::show_panel` only searches its top `panels`
                // list — the Settings cog is a bottom item, so the call
                // above can no-op. Force visibility alone in that case; the
                // runner's active-panel index (header title) is untouched,
                // the same tolerance band as `shell_config`'s
                // `active_accent`/`selection_bg` note.
                if !ctx.shell().sidebar_visible() {
                    ctx.shell_mut().toggle_sidebar();
                }
            } else {
                ctx.shell_mut().hide_sidebar();
            }
        }

        reaction
    }

    /// #634 smoke retry: the keyboard/tick half of the runner ↔ shadow
    /// panel sync (see [`Self::on_shell_event`]'s doc comment for the
    /// two-instance split). `ShellAdapter` polls this after every
    /// `handle()` *and* every `tick()`, and applies a returned id to the
    /// runner's `AppShell` via `show_panel` — updating the activity-bar
    /// highlight and sidebar-header title — then re-notifies
    /// `on_shell_event` with the same `PanelChanged` a mouse click
    /// produces. That echo is suppressed from re-running the click path
    /// via [`Self::suppress_shell_panel_echo`].
    ///
    /// Covers: the sidebar-focused / activity-bar-focused keyboard tiers
    /// (`activity_bar_activate`, `focus_sidebar_panel`), panel
    /// accelerators, tick-driven reveals (`process_pending_sidebar`'s DAP
    /// `dap_wants_sidebar`), and the startup frame (the runner boots with
    /// the hamburger active — `AppShell::new` activates index 0 — while
    /// the shadow starts on Explorer).
    fn take_requested_panel(&mut self) -> Option<quadraui::WidgetId> {
        if !self.engine.app_shell.sidebar_visible() {
            return None;
        }
        // #557: an extension panel takes over the sidebar body *without*
        // touching the shadow `app_shell`'s active-panel id (`mouse.rs`'s
        // `ActivityBarTarget::ExtensionPanel` arm and
        // `Engine::activity_bar_activate`'s `8 + idx` arm both leave it
        // alone), so `engine.ext_panel_active` — not `active_panel_id()` — is
        // what the runner has to follow while one is open. Without this the
        // reconciliation would immediately steer the runner back onto
        // whatever built-in panel was last active and the extension icon's
        // highlight would flicker off on the very next dispatch.
        if let Some(name) = self.engine.ext_panel_active.as_deref() {
            let id = quadraui::WidgetId::new(crate::core::engine::sidebar::ext_panel_id(name));
            if self.last_shell_panel.as_ref() == Some(&id) {
                return None;
            }
            self.suppress_shell_panel_echo = true;
            return Some(id);
        }
        let current = self.engine.app_shell.active_panel_id()?.clone();
        if self.last_shell_panel.as_ref() == Some(&current) {
            return None;
        }
        self.suppress_shell_panel_echo = true;
        Some(current)
    }

    /// The app ↔ runner-shell state bridge (#635 item E + the #634 smoke
    /// retry). Two `AppShell` instances exist on this path:
    ///
    /// - the **runner's** (`ShellAdapter`-owned) — hit-tests activity-bar
    ///   clicks, owns the painted chrome (icon strip, sidebar-header
    ///   title, divider) and the layout bounds `render_content` receives;
    /// - the **shadow** (`engine.app_shell`) — what every engine-side
    ///   consumer reads: `render_sidebar_content`'s panel dispatch,
    ///   `active_panel_is`, `sidebar_visible()` hit-test gates, session
    ///   persistence.
    ///
    /// `event_loop()` had one instance for both roles; the cutover split
    /// them, and this method is the click-direction half of keeping them
    /// converged (the keyboard/tick direction is `take_requested_panel` +
    /// the end-of-`handle` visibility sync). Without the `PanelChanged` →
    /// shadow mirror here, an activity-bar click switched only the
    /// runner's header title while the content pane stayed on Explorer
    /// forever — the #634 smoke-retry failure.
    ///
    /// The menu hamburger stays special (#635 item E): it's registered as
    /// a top-row `PanelDefinition` in [`Self::shell_config`] so it keeps
    /// its place before Explorer, but it isn't a real content panel — a
    /// click on it reveals the menu bar instead of switching the shadow,
    /// mirroring the Alt+menu-letter shim and `MenuSystem` intercept in
    /// `handle()` above.
    ///
    /// #693: `ShellAdapter::handle` consumes `AppShellEvent::PanelChanged`
    /// itself and returns immediately after calling this notification — it
    /// never falls through to `Self::handle`, so the title-bar sync at the
    /// end of that method (`ctx.shell_mut().set_title_bar_visible(...)`)
    /// never runs for a hamburger click. Before quadraui#617 landed
    /// `ShellApp::on_shell_event_ctx`, there was no way to reach the
    /// `ShellContext` from here at all, so the reveal was invisible —
    /// `engine.menu_bar_visible` flipped, but `AppShell` never reserved the
    /// row, so `render_content`'s `layout.title_bar_bounds` stayed `None`
    /// and nothing painted — until some unrelated later dispatch happened
    /// to run `handle()`'s sync. [`Self::on_shell_event_ctx`] below pushes
    /// the sync itself, on the same frame the click fires, closing that
    /// gap without waiting for a second event.
    fn on_shell_event(&mut self, event: &quadraui::AppShellEvent) {
        match event {
            quadraui::AppShellEvent::PanelChanged { panel_id } => {
                if panel_id.as_str() == HAMBURGER_PANEL_ID {
                    self.engine.menu_bar_visible = true;
                    // The runner's `AppShell` now points at the hamburger
                    // (`handle_activity_click` treats it as an ordinary
                    // panel). Recording that here makes the next
                    // `take_requested_panel` poll see the mismatch against
                    // the shadow's real panel and steer the runner straight
                    // back — so the "Menu" sidebar-header label the #635
                    // tolerance note accepted as persistent now lasts one
                    // frame instead of until the next real panel click.
                    self.last_shell_panel = Some(panel_id.clone());
                    return;
                }
                self.last_shell_panel = Some(panel_id.clone());
                // #557: a plugin-provided panel is now a real
                // `PanelDefinition` in the runner's `AppShell`
                // (`sync_ext_activity_panels`), so its icon click arrives here
                // like any other. It is *not* a shadow-`app_shell` panel
                // though — `render_sidebar_content` dispatches on
                // `sidebar.ext_panel_name`, not on the active panel id — so
                // mirror `mouse.rs`'s `ActivityBarTarget::ExtensionPanel` arm
                // instead of `focus_sidebar_panel`, which would fall through
                // to the explorer.
                if let Some(name) =
                    crate::core::engine::sidebar::ext_panel_name_from_id(panel_id.as_str())
                {
                    let name = name.to_string();
                    if std::mem::take(&mut self.suppress_shell_panel_echo) {
                        return;
                    }
                    self.activate_ext_panel(&name);
                    return;
                }
                if std::mem::take(&mut self.suppress_shell_panel_echo) {
                    // Echo of our own `take_requested_panel` reconciliation
                    // (see that method): the engine already holds this
                    // state — don't re-run the click path and steal focus.
                    return;
                }
                // ── #634 smoke retry: a real activity-bar click ─────────
                // `ShellAdapter` consumed the `MouseDown` and only reports
                // this semantic event, so the legacy `mouse::handle_mouse`
                // activity-bar arm (which called
                // `Engine::toggle_sidebar_panel` on the shadow) never runs
                // on this path. Without the mirror below, only the runner's
                // own chrome (sidebar-header title) switched while
                // `render_sidebar_content` — which reads the *shadow*
                // `engine.app_shell` — kept painting Explorer forever.
                // Mirrors `mouse.rs`'s `target_panel_id` arm minus the
                // toggle decision (the runner already made it: a
                // same-panel-while-visible click arrives as
                // `SidebarHidden`, not `PanelChanged`).
                self.sidebar.ext_panel_name = None;
                self.engine.ext_panel_has_focus = false;
                self.engine.ext_panel_active = None;
                self.engine.focus_sidebar_panel(panel_id.as_str());
                self.sidebar.has_focus = true;
            }
            // ── #634 smoke retry: second click on the active panel's icon —
            // the runner hid its own sidebar; mirror
            // `Engine::toggle_sidebar_panel`'s hide branch onto the shadow
            // so `sidebar_visible()` consumers (tick's viewport math,
            // `mouse::handle_mouse` hit tests, autohide, session
            // persistence) don't keep believing the sidebar is open.
            quadraui::AppShellEvent::SidebarHidden => {
                // #557: this is also how a *second* click on an open
                // extension panel's icon arrives, so drop the plugin-panel
                // state here too — `mouse.rs`'s `ExtensionPanel` arm clears
                // the same three fields in its own hide branch. Leaving them
                // set would make `take_requested_panel` keep steering the
                // runner back onto a panel whose sidebar the user just closed.
                self.sidebar.ext_panel_name = None;
                self.engine.ext_panel_has_focus = false;
                self.engine.ext_panel_active = None;
                self.engine.app_shell.hide_sidebar();
                self.engine.clear_sidebar_focus();
                self.engine.session.explorer_visible = false;
                let _ = self.engine.session.save();
            }
            // ── #634 smoke retry: the Settings cog is registered as a
            // *bottom item* (`shell_config`), and `AppShell` doesn't run
            // its panel toggle for those — it only reports the click. Run
            // the legacy toggle on the shadow (same call `mouse.rs`'s
            // `ActivityBarTarget::Settings` arm made); the end-of-`handle`
            // visibility sync + `take_requested_panel` then carry the
            // result back to the runner's `AppShell`.
            quadraui::AppShellEvent::BottomItemClicked { id } if id.as_str() == PANEL_SETTINGS => {
                self.sidebar.ext_panel_name = None;
                self.engine.ext_panel_has_focus = false;
                self.engine.ext_panel_active = None;
                self.engine.toggle_sidebar_panel(PANEL_SETTINGS);
                if self.engine.app_shell.sidebar_visible() {
                    self.sidebar.has_focus = true;
                }
            }
            // #634: the other half of the width sync `handle()` performs on
            // the way out — when `AppShell` resolves its *own* divider drag
            // it reports the settled width here, and vimcode's copy has to
            // follow or the next `handle()` would immediately push the stale
            // value back and undo the drag.
            quadraui::AppShellEvent::SidebarResized { new_width } => {
                self.sidebar_width = new_width.round().max(0.0) as u16;
            }
            _ => {}
        }
    }

    /// #693: the `ShellContext`-aware half of the runner ↔ shadow bridge.
    /// Runs the exact same dispatch [`Self::on_shell_event`] always has
    /// (calling it directly rather than duplicating its match), then —
    /// unconditionally, mirroring `handle()`'s own end-of-dispatch sync
    /// comment — pushes `engine.menu_bar_visible` into the *runner's*
    /// `AppShell` via `ctx.shell_mut().set_title_bar_visible(...)`.
    ///
    /// This is the one notification `ShellAdapter` delivers *outside* a
    /// `Self::handle` dispatch (`ShellAdapter::handle`'s `PanelChanged` arm
    /// returns right after calling it), so it is also the one place
    /// `handle()`'s own title-bar sync can never reach. Before
    /// quadraui#617 added the `ShellContext` parameter here, a hamburger
    /// click could only flip the engine flag and wait for some later,
    /// unrelated dispatch to reach `handle()` and paint the reservation —
    /// in a purely event-driven TUI with no further input, that frame
    /// might never come, so the menu bar stayed invisible indefinitely
    /// while still hit-testing as open (`handle()`'s `MenuSystem`
    /// intercept reads `engine.menu_bar_visible` directly, independent of
    /// what's painted). Pushing the sync here closes the gap on the same
    /// frame the click fires, the same way `handle()` already does for
    /// every other reveal path (Alt+<letter>, `:set menu`, panel
    /// accelerators).
    ///
    /// Unconditional rather than gated on the hamburger arm specifically
    /// (mirroring `handle()`'s reasoning) so any future `on_shell_event`
    /// arm that ends up flipping `menu_bar_visible` gets the same
    /// same-frame guarantee for free.
    fn on_shell_event_ctx(&mut self, event: &quadraui::AppShellEvent, ctx: &ShellContext<'_>) {
        #[allow(deprecated)]
        self.on_shell_event(event);
        ctx.shell_mut()
            .set_title_bar_visible(self.engine.menu_bar_visible);
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

        // ── Post-paint feedback from the last `render_content` (#634) ─────
        // `event_loop` did both of these *inside* its draw block, between
        // building the layout and the `terminal.draw` call (`mod.rs:1139`-
        // `:1149`) and immediately after it (`:1216`-`:1266`). `render_content`
        // is `&self` and can't touch `Engine`'s `&mut` API, so both read the
        // caches it left behind and apply them here — the runner calls `tick`
        // after every event batch, and returning `Reaction::Redraw` when
        // anything moved reproduces the legacy two-pass repaint one frame
        // later instead of within the same one.
        {
            let layout = self.last_layout.borrow();
            if let Some(ref screen) = *layout {
                // Exact per-window viewport dimensions from paint-time
                // geometry, so `ensure_cursor_visible` uses real column
                // counts rather than `tick`'s whole-screen approximation
                // above (which can't see splits).
                for rw in &screen.windows {
                    self.engine.set_viewport_for_window(
                        rw.window_id,
                        rw.lines.len().max(1),
                        rw.text_viewport_cols.max(1),
                    );
                }
            }
        }
        {
            // Apply the per-group tab-bar widths the paint measured and
            // re-check that every group's active tab is on screen. Shared
            // across backends — see `Engine::post_draw_apply_widths`.
            let counts = self.tab_visible_counts.borrow().clone();
            if !counts.is_empty() && self.engine.post_draw_apply_widths(&counts) {
                needs_redraw = true;
            }
        }

        // ── Terminal chrome the runner doesn't own (mirrors mod.rs:1268-:1286)
        // Cursor shape per mode and the emulator window title. Both are plain
        // escape sequences rather than anything ratatui buffers, so writing
        // them to the shared process stdout between frames is exactly what
        // `event_loop` did through `terminal.backend_mut()`. Live runs only:
        // under `driver_with_shell` there is no real terminal, and emitting
        // control sequences from a test binary would corrupt the harness'
        // own output.
        if self.live {
            let cursor_style = if !self.sidebar.has_focus && self.engine.pending_key == Some('r') {
                SetCursorStyle::SteadyUnderScore
            } else if !self.sidebar.has_focus && self.engine.mode == Mode::Insert {
                SetCursorStyle::BlinkingBar
            } else {
                SetCursorStyle::SteadyBlock
            };
            let mut out = io::stdout();
            let _ = execute!(out, cursor_style);
            let tui_title = self
                .engine
                .active_buffer_name()
                .map(|n| format!("VimCode \u{2014} {}", n))
                .unwrap_or_else(|| "VimCode".to_string());
            let _ = execute!(out, SetTitle(tui_title.as_str()));
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
///
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
/// 3. **Context menu** (`mod.rs:2703`-`:2706`) — checked ahead of
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
/// **#635 (Stage 6b item D) ported two of the three tiers `mod.rs:1629`-
/// `:2737` used to leave out:** the activity-bar-focused tier (mirrors
/// `mod.rs:1805`-`:1854` — `j`/`k`/`l`/`h`/`Enter`/`Esc`/`q` while
/// `engine.activity_bar_focused`, reachable now that gap 2/mouse is closed
/// by #602 and sets that flag for real) and command-output-selection
/// (`cmd_sel` — mirrors `mod.rs:2651`-`:2701`: Ctrl+C copies the selected
/// message/command-line substring via `tui_copy_to_clipboard`, any other
/// key clears it; `cmd_sel` itself was already populated by mouse drag and
/// painted by `panels::render_command_line`, so this closes the keyboard
/// side only).
///
/// **Still not ported:** the sidebar-focused tier (`mod.rs:1856`-`:2385` —
/// per-panel keyboard dispatch for search/debug/extension-panel/source-
/// control/explorer while `sidebar.has_focus`, plus its own nested
/// context-menu intercept and Ctrl-W toolbar/panel/editor navigation). At
/// ~500 lines across five nested per-panel dispatchers it's an order of
/// magnitude larger than the other two tiers and wasn't safely portable in
/// the same pass; left as the one open item from Stage 6b's item D. Until
/// it lands, a key press while `sidebar.has_focus` is true falls through
/// to the general `Engine::handle_key` fallback below, same as before this
/// stage — no regression, just the pre-existing gap narrowed rather than
/// closed.
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
///
/// The sidebar-focused keyboard tier — `event_loop`'s `mod.rs:1886`-`:2415`,
/// ported verbatim (#634, closing the last open item of #635's item D).
///
/// Split into its own function purely for size: at ~340 lines across eight
/// per-panel dispatchers plus two prologue blocks it dwarfs every other tier
/// in [`handle_key_pressed`], and inlining it there would bury the precedence
/// chain that function's doc comment describes.
///
/// **Unconditionally terminal.** The caller checks the outer guard
/// (`sidebar.has_focus && !picker_open && !terminal_has_focus && !Release`)
/// and returns whatever this returns — matching the legacy loop, where every
/// sub-block ends `needs_redraw = true; continue;` and the trailing explorer
/// block carries no panel guard of its own, so a key that reaches this tier
/// never falls through to the editor tier.
///
/// `ui_event` is the original, un-round-tripped [`UiEvent`]: the debug/DAP
/// panel re-dispatches it into `SidebarSystem::handle` for its navigation
/// keys before falling back to the action-key table, which is what
/// `event_loop`'s `ui_event_saved` clone (`mod.rs:1745`) existed for.
#[allow(clippy::too_many_arguments)]
fn handle_sidebar_focused_key(
    key_event: KeyEvent,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    screen_w: u16,
    screen_h: u16,
    backend: &mut dyn quadraui::Backend,
    ui_event: &UiEvent,
) -> Reaction {
    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);

    // #451: when an explorer context menu is open, intercept j/k/Enter/Esc
    // HERE — before the panel-specific dispatch below sends j/k to
    // `dispatch_explorer_key`. Without this, explorer-focused mode hijacks
    // the keys and the menu's own selection doesn't move.
    if engine.context_menu.is_some() {
        let effective_key = match key_event.code {
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Enter => "Return".to_string(),
            KeyCode::Esc => "Escape".to_string(),
            KeyCode::Char(c) => c.to_string(),
            _ => String::new(),
        };
        if !effective_key.is_empty() {
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
    }

    // Ctrl-W prefix: set pending state for window navigation. A Vim chord,
    // so it stays inline rather than becoming an accelerator.
    if ctrl && matches!(key_event.code, KeyCode::Char('w') | KeyCode::Char('W')) {
        sidebar.pending_ctrl_w = true;
        return Reaction::Redraw;
    }
    // Ctrl-W {h,l,Left,Right}: navigate between toolbar / panel / editor.
    if sidebar.pending_ctrl_w {
        sidebar.pending_ctrl_w = false;
        match key_event.code {
            KeyCode::Char('h') | KeyCode::Left => {
                // Panel → activity bar toolbar
                let idx = engine.activity_bar_toolbar_idx_for_active_panel();
                sidebar.has_focus = false;
                engine.clear_sidebar_focus();
                engine.activity_bar_focus_in_at(idx);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                // Panel → editor
                sidebar.has_focus = false;
                engine.clear_sidebar_focus();
            }
            _ => {} // Unknown Ctrl-W combo in the sidebar: ignore
        }
        return Reaction::Redraw;
    }

    // ── Search panel ────────────────────────────────────────────────────
    if engine.active_panel_is(PANEL_SEARCH) {
        // Ctrl+V paste (backend-specific clipboard access)
        if ctrl && key_event.code == KeyCode::Char('v') {
            let is_replace =
                engine.search_panel_form_focus.borrow().as_deref() == Some("search:replace");
            if let Some(text) = Engine::clipboard_paste() {
                engine.search_input_paste(is_replace, &text);
            }
            return Reaction::Redraw;
        }
        let key_name = match key_event.code {
            KeyCode::Enter => "Return",
            KeyCode::Backspace => "BackSpace",
            KeyCode::Delete => "Delete",
            KeyCode::Left => "Left",
            KeyCode::Right => "Right",
            KeyCode::Home => "Home",
            KeyCode::End => "End",
            KeyCode::Up => "Up",
            KeyCode::Down => "Down",
            KeyCode::Tab => "Tab",
            KeyCode::BackTab => "BackTab",
            KeyCode::Esc => "Escape",
            KeyCode::PageUp => "Page_Up",
            KeyCode::PageDown => "Page_Down",
            // Single-char keys use the char as the key name (via `unicode`
            // below); Ctrl+b is the one that needs an explicit name.
            KeyCode::Char('b') if ctrl => "b",
            KeyCode::Char(_) => "",
            _ => "",
        };
        let unicode = match key_event.code {
            KeyCode::Char(c) if !ctrl => Some(c),
            _ => None,
        };
        let alt = key_event.modifiers.contains(KeyModifiers::ALT);
        let key_str = if key_name.is_empty() {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        } else {
            key_name.to_string()
        };
        use crate::core::engine::SearchKeyResult;
        let result = engine.dispatch_search_sidebar_key_unified(&key_str, ctrl, alt, unicode);
        if matches!(result, SearchKeyResult::Unfocused) {
            sidebar.has_focus = false;
        }
        return Reaction::Redraw;
    }

    // ── Debug (DAP) panel ───────────────────────────────────────────────
    if engine.active_panel_is(PANEL_DEBUG) {
        // Route navigation keys through SidebarSystem first.
        render::populate_dap_sidebar_system(engine);
        let rect = engine.dap_sidebar_body_rect.get();
        let sidebar_event = engine
            .dap_sidebar_system
            .borrow_mut()
            .handle(ui_event, backend, rect);
        if !engine.dispatch_dap_sidebar_event(sidebar_event) {
            // Ignored by the MSV — handle action keys via shared dispatch.
            let key_name = match key_event.code {
                KeyCode::Char(c) => match c {
                    'q' => "q",
                    'x' => "x",
                    'd' => "d",
                    'b' if ctrl => {
                        engine.app_shell.hide_sidebar();
                        sidebar.has_focus = false;
                        engine.clear_sidebar_focus();
                        engine.session.explorer_visible = false;
                        let _ = engine.session.save();
                        ""
                    }
                    _ => "",
                },
                KeyCode::F(n @ 5..=11) => match n {
                    5 | 9 | 10 | 11 => {
                        let name = format!("F{n}");
                        engine.handle_key(&name, None, false);
                        return Reaction::Redraw;
                    }
                    6 => "F6",
                    _ => "",
                },
                code => tui_key_to_engine_name(code).unwrap_or(""),
            };
            if engine.dispatch_dap_sidebar_action_key(key_name) {
                sidebar.has_focus = false;
            }
        }
        return Reaction::Redraw;
    }

    // ── Plugin-provided extension panel ─────────────────────────────────
    if engine.ext_panel_has_focus && sidebar.ext_panel_name.is_some() {
        // When the input field is active, characters are input text rather
        // than navigation commands.
        if engine.ext_panel_input_active {
            let (ikey, ich): (&str, Option<char>) = match key_event.code {
                KeyCode::Esc => ("Escape", None),
                KeyCode::Enter => ("Return", None),
                KeyCode::Backspace => ("BackSpace", None),
                KeyCode::Char(ch) => ("char", Some(ch)),
                _ => ("", None),
            };
            if !ikey.is_empty() {
                let name = if ikey == "char" {
                    ich.map(|c| c.to_string()).unwrap_or_default()
                } else {
                    ikey.to_string()
                };
                engine.handle_ext_panel_input_key(&name, ctrl, ich);
            }
            return Reaction::Redraw;
        }
        // h/Left: the engine sets `activity_bar_focused` inside
        // `handle_ext_panel_key`.
        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => ("j", None),
            KeyCode::Char('k') | KeyCode::Up => ("k", None),
            KeyCode::Char('h') => ("h", None),
            KeyCode::Left => ("Left", None),
            KeyCode::Char('g') => ("g", None),
            KeyCode::Char('G') => ("G", None),
            KeyCode::Tab => ("Tab", None),
            KeyCode::Enter => ("Return", None),
            KeyCode::Char('q') | KeyCode::Esc => ("Escape", None),
            KeyCode::Char(ch) => ("char", Some(ch)),
            _ => ("", None),
        };
        if !key_name.is_empty() {
            let ch = if key_name == "char" { unicode } else { None };
            let name = if key_name == "char" {
                ch.map(|c| c.to_string()).unwrap_or_default()
            } else {
                key_name.to_string()
            };
            engine.handle_ext_panel_key(&name, ctrl, ch);
            if !engine.ext_panel_has_focus {
                sidebar.has_focus = false;
                // Keep `ext_panel_name` when focus moved to the activity bar
                // (the panel stays visible while the toolbar cursor shows).
                if !engine.activity_bar_focused {
                    sidebar.ext_panel_name = None;
                }
            }
        }
        return Reaction::Redraw;
    }

    // ── Extensions marketplace panel ────────────────────────────────────
    if engine.active_panel_is(PANEL_EXTENSIONS) {
        let (key_name, unicode) = match key_event.code {
            KeyCode::Char(c) => (c.to_string(), Some(c)),
            code => (
                tui_key_to_engine_name(code)
                    .map(str::to_string)
                    .unwrap_or_default(),
                None,
            ),
        };
        use crate::core::engine::ExtSidebarKeyResult;
        match engine.dispatch_ext_sidebar_key_unified(&key_name, unicode) {
            ExtSidebarKeyResult::Unfocused | ExtSidebarKeyResult::FocusActivityBar => {
                sidebar.has_focus = false;
            }
            ExtSidebarKeyResult::Consumed => {}
        }
        return Reaction::Redraw;
    }

    // ── Settings panel ──────────────────────────────────────────────────
    if engine.active_panel_is(PANEL_SETTINGS) {
        // h/Left focus-to-activity-bar lives inside `handle_settings_key`:
        // when the selected row is not an enum, `h` sets
        // `activity_bar_focused`.
        // Ctrl-V paste into the search input or an inline edit.
        if ctrl && key_event.code == KeyCode::Char('v') {
            if engine.settings_input_active || engine.settings_editing.is_some() {
                let text = match engine.clipboard_read {
                    Some(ref cb) => cb().ok(),
                    None => None,
                };
                if let Some(t) = text {
                    engine.settings_paste(&t);
                }
            }
            return Reaction::Redraw;
        }
        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => ("j", None),
            KeyCode::Char('k') | KeyCode::Up => ("k", None),
            KeyCode::Char('l') | KeyCode::Right => ("l", None),
            KeyCode::Char('h') | KeyCode::Left => ("h", None),
            KeyCode::Char(' ') => ("Space", None),
            KeyCode::Char('/') => ("/", None),
            KeyCode::Char('q') => ("Escape", None),
            KeyCode::Char(ch) => ("char", Some(ch)),
            code => (tui_key_to_engine_name(code).unwrap_or(""), None),
        };
        if !key_name.is_empty() {
            let ch = if key_name == "char" { unicode } else { None };
            engine.handle_settings_key(if key_name == "char" { "" } else { key_name }, ctrl, ch);
            if !engine.settings_has_focus {
                sidebar.has_focus = false;
            }
            // Keep the selected item visible after j/k navigation.
            let content_h = screen_h.saturating_sub(4) as usize;
            if content_h > 0 {
                if engine.settings_selected >= engine.settings_scroll_top + content_h {
                    engine.settings_scroll_top = engine.settings_selected - content_h + 1;
                } else if engine.settings_selected < engine.settings_scroll_top {
                    engine.settings_scroll_top = engine.settings_selected;
                }
            }
        }
        return Reaction::Redraw;
    }

    // ── AI assistant panel ──────────────────────────────────────────────
    if engine.active_panel_is(PANEL_AI) {
        // h/Left focus-to-activity-bar lives inside `handle_ai_panel_key`.
        if ctrl && key_event.code == KeyCode::Char('v') {
            let text = match engine.clipboard_read {
                Some(ref cb) => cb().ok(),
                None => None,
            };
            if let Some(t) = text {
                engine.ai_insert_text(&t);
            }
            return Reaction::Redraw;
        }
        let (key_name, unicode): (&str, Option<char>) = match key_event.code {
            KeyCode::Down if !engine.ai_input_active => ("j", None),
            KeyCode::Up if !engine.ai_input_active => ("k", None),
            KeyCode::Char('j') if !engine.ai_input_active => ("j", None),
            KeyCode::Char('k') if !engine.ai_input_active => ("k", None),
            KeyCode::Char('h') if !engine.ai_input_active && !ctrl => ("h", None),
            KeyCode::Left if !engine.ai_input_active => ("Left", None),
            KeyCode::Char('G') if !engine.ai_input_active => ("G", None),
            KeyCode::Char('g') if !engine.ai_input_active => ("g", None),
            KeyCode::Char('i') | KeyCode::Char('a') if !engine.ai_input_active => ("i", None),
            KeyCode::Enter => ("Return", None),
            KeyCode::Esc => ("Escape", None),
            KeyCode::Char('q') if !engine.ai_input_active => ("Escape", None),
            KeyCode::Backspace => ("BackSpace", None),
            KeyCode::Delete => ("Delete", None),
            KeyCode::Left => ("Left", None),
            KeyCode::Right => ("Right", None),
            KeyCode::Home => ("Home", None),
            KeyCode::End => ("End", None),
            KeyCode::Char('c') if ctrl => ("c", None),
            KeyCode::Char('a') if ctrl => ("a", None), // Ctrl-A → start of input
            KeyCode::Char('e') if ctrl => ("e", None),
            KeyCode::Char('k') if ctrl => ("k", None),
            KeyCode::Char(ch) => ("char", Some(ch)),
            _ => ("", None),
        };
        if !key_name.is_empty() {
            let (mapped, uni) = if key_name == "char" {
                ("", unicode)
            } else {
                (key_name, None)
            };
            engine.handle_ai_panel_key(mapped, ctrl, uni);
            if !engine.ai_has_focus {
                sidebar.has_focus = false;
            }
        }
        return Reaction::Redraw;
    }

    // ── Source Control panel ────────────────────────────────────────────
    if engine.active_panel_is(PANEL_GIT) {
        // h/Left focus-to-activity-bar lives inside
        // `dispatch_sc_sidebar_key_unified`. Ctrl+b hides the sidebar.
        if ctrl && matches!(key_event.code, KeyCode::Char('b')) {
            engine.app_shell.hide_sidebar();
            sidebar.has_focus = false;
            engine.clear_sidebar_focus();
            engine.session.explorer_visible = false;
            let _ = engine.session.save();
            return Reaction::Redraw;
        }
        // With keyboard enhancement (kitty protocol), Shift+s arrives as
        // Char('s') + SHIFT, not Char('S'). Resolve the actual character
        // before matching the whitelist.
        let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
        let (key_str, unicode): (&str, Option<char>) = match key_event.code {
            KeyCode::Enter => ("Return", None),
            KeyCode::Esc => ("Escape", None),
            KeyCode::Backspace => ("BackSpace", None),
            KeyCode::Delete => ("Delete", None),
            KeyCode::Up => ("Up", None),
            KeyCode::Down => ("Down", None),
            KeyCode::Left => ("Left", None),
            KeyCode::Right => ("Right", None),
            KeyCode::Home => ("Home", None),
            KeyCode::End => ("End", None),
            KeyCode::Tab => ("Tab", None),
            KeyCode::BackTab => ("BackTab", None),
            KeyCode::PageUp => ("Page_Up", None),
            KeyCode::PageDown => ("Page_Down", None),
            KeyCode::Char(ch) => {
                let resolved = if shift && ch.is_ascii_lowercase() {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                };
                let name = match resolved {
                    'j' => "j",
                    'k' => "k",
                    'h' => "h",
                    'l' => "l",
                    's' => "s",
                    'S' => "S",
                    'd' => "d",
                    'D' => "D",
                    'c' => "c",
                    'C' => "C",
                    'p' => "p",
                    'P' => "P",
                    'f' => "f",
                    'r' => "r",
                    'b' => "b",
                    'B' => "B",
                    'q' => "q",
                    '?' => "?",
                    '/' => "/",
                    _ => "",
                };
                (name, Some(resolved))
            }
            _ => ("", None),
        };
        if !key_str.is_empty() || unicode.is_some() {
            use crate::core::engine::ScKeyResult;
            let result = engine.dispatch_sc_sidebar_key_unified(key_str, ctrl, unicode);
            if matches!(
                result,
                ScKeyResult::Unfocused | ScKeyResult::FocusActivityBar
            ) {
                sidebar.has_focus = false;
            }
        }
        return Reaction::Redraw;
    }

    // ── Explorer (the fallback: no panel guard) ─────────────────────────
    {
        use crate::core::engine::ExplorerKeyResult;
        if ctrl && key_event.code == KeyCode::Char('b') {
            engine.app_shell.hide_sidebar();
            sidebar.has_focus = false;
            engine.clear_sidebar_focus();
            engine.session.explorer_visible = false;
            let _ = engine.session.save();
        } else {
            let key_name = match key_event.code {
                KeyCode::Esc => "Escape",
                KeyCode::Enter => "Return",
                KeyCode::Up => "Up",
                KeyCode::Down => "Down",
                KeyCode::Left => "Left",
                KeyCode::Right => "Right",
                KeyCode::Home => "Home",
                KeyCode::End => "End",
                KeyCode::PageUp => "PageUp",
                KeyCode::PageDown => "PageDown",
                KeyCode::Char(c) => match c {
                    'j' => "j",
                    'k' => "k",
                    'h' => "h",
                    'l' => "l",
                    'q' => "q",
                    _ => "",
                },
                _ => "",
            };
            let chr = if let KeyCode::Char(c) = key_event.code {
                Some(c)
            } else {
                None
            };
            match engine.dispatch_explorer_key(key_name, chr, ctrl) {
                // `dispatch_explorer_key` already called
                // `activity_bar_focus_in_at(1)` for `FocusToolbar`.
                ExplorerKeyResult::Unfocused | ExplorerKeyResult::FocusToolbar => {
                    sidebar.has_focus = false;
                }
                _ => {}
            }
        }
    }
    Reaction::Redraw
}

/// The per-keypress slice of `event_loop`'s loop-local `mut` state that
/// [`handle_key_pressed`] reads *and* writes (#634).
///
/// Bundled into one struct rather than appended as six more positional
/// parameters: the function already carries eleven, and every one of these
/// is "the same `let mut` the legacy loop kept across iterations", so
/// grouping them keeps that provenance legible at the call site.
///
/// `ui_event` is the *original* [`UiEvent`] the key was decoded from, not a
/// re-synthesised one — the debug/DAP sidebar tier re-dispatches it into
/// `SidebarSystem::handle`, which needs the backend-neutral event, exactly
/// as `event_loop`'s `ui_event_saved` clone did (`mod.rs:1745`, `:2042`).
struct KeyDispatchState<'a> {
    /// `event_loop`'s `sidebar_width` local — mutated by Alt+Left/Right.
    sidebar_width: &'a mut u16,
    /// `event_loop`'s `quickfix_scroll_top` local — kept in sync with the
    /// quickfix selection by the post-key epilogue.
    quickfix_scroll_top: &'a mut usize,
    /// `event_loop`'s `last_clipboard_content` local — the change-detection
    /// key for `sync_tui_clipboard` (`clipboard=unnamedplus`).
    last_clipboard_content: &'a mut Option<String>,
    /// Command-line / message-line selection (mouse-populated, keyboard-
    /// cleared).
    cmd_sel: &'a Cell<Option<(usize, usize)>>,
    /// 200 ms yank-highlight expiry, armed here and cleared in `tick`.
    yank_hl_deadline: &'a Cell<Option<Instant>>,
    /// See the struct doc — the un-round-tripped source event.
    ui_event: &'a UiEvent,
}

#[allow(clippy::too_many_arguments)]
fn handle_key_pressed(
    key: quadraui::Key,
    modifiers: quadraui::Modifiers,
    repeat: bool,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    folder_picker: &mut Option<FolderPickerState>,
    keyboard_enhanced: bool,
    screen_w: u16,
    screen_h: u16,
    backend: &mut dyn quadraui::Backend,
    state: &mut KeyDispatchState<'_>,
) -> Reaction {
    let cmd_sel = state.cmd_sel;
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

    // ── Activity bar (toolbar) focused (mirrors mod.rs:1805-:1854) ──────
    // #635 (Stage 6b item D): ported now that gap 2 (mouse) is closed by
    // #602 — `engine.activity_bar_focused` is set by `mouse::handle_mouse`
    // via `TuiShellApp::handle_mouse_event`, so this tier is reachable from
    // a real click sequence, not just synthetic test state.
    if engine.activity_bar_focused && !engine.picker_open && key_event.kind != KeyEventKind::Release
    {
        match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => {
                engine.activity_bar_move_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                engine.activity_bar_move_up();
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                use crate::core::engine::sidebar::ActivityBarActivation;
                let activation = engine.activity_bar_activate();
                match activation {
                    ActivityBarActivation::MenuToggled => {
                        if !engine.menu_bar_visible {
                            engine.menu_system.borrow_mut().close(backend);
                        }
                    }
                    ActivityBarActivation::PanelFocused => {
                        sidebar.ext_panel_name = None;
                        sidebar.has_focus = true;
                    }
                    ActivityBarActivation::ExtPanelFocused(name) => {
                        sidebar.ext_panel_name = Some(name);
                        sidebar.has_focus = true;
                    }
                    ActivityBarActivation::NoOp => {}
                }
            }
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
                // Leave toolbar, return focus to editor.
                engine.activity_bar_focus_out();
            }
            KeyCode::Char('q') => {
                // Collapse sidebar from toolbar.
                engine.activity_bar_focus_out();
                engine.app_shell.hide_sidebar();
                engine.clear_sidebar_focus();
                sidebar.has_focus = false;
                engine.session.explorer_visible = false;
                let _ = engine.session.save();
            }
            _ => {}
        }
        return Reaction::Redraw;
    }

    // ── Sidebar focused (mirrors mod.rs:1886-:2415) ─────────────────────
    // #634: the last of #635's item-D tiers. Suppressed while a picker
    // modal is open and while the terminal has focus (e.g. "Press Enter to
    // close…" after an extension install), exactly like the legacy loop.
    if sidebar.has_focus
        && !engine.picker_open
        && !engine.terminal_has_focus
        && key_event.kind != KeyEventKind::Release
    {
        // Unlike the tiers above, this one is unconditionally terminal:
        // every sub-block in `event_loop`'s copy ends `needs_redraw = true;
        // continue;`, and the trailing explorer block has no panel guard, so
        // once the outer guard passes the key never reaches the editor tier.
        return handle_sidebar_focused_key(
            key_event,
            engine,
            sidebar,
            screen_w,
            screen_h,
            backend,
            state.ui_event,
        );
    }

    if key_event.kind == KeyEventKind::Release {
        return Reaction::Continue;
    }
    let Some((key_name, unicode, ctrl)) = translate_key(key_event, keyboard_enhanced) else {
        return Reaction::Continue;
    };

    // ── Ctrl+L: force a full screen redraw (mirrors mod.rs:2429-:2435) ──
    // The legacy loop called `ratatui::Terminal::clear()`, which resets
    // ratatui's previous-frame buffer so the next draw re-emits every cell.
    // The shell runner owns the `Terminal`, and neither `Backend` nor
    // `ShellApp` exposes a "repaint everything" escape hatch, so all this
    // can do today is request an ordinary redraw. Tracked as an upstream
    // gap (a `Backend::request_full_repaint`-shaped hook) alongside the
    // popup-disappearance clear in `Self::render_content`; see #634's
    // hand-off notes. Consuming the key here rather than letting it fall
    // through preserves the legacy behaviour of Ctrl+L *not* reaching
    // `Engine::handle_key`.
    if ctrl && matches!(key_event.code, KeyCode::Char('l') | KeyCode::Char('L')) {
        return Reaction::Redraw;
    }

    // ── Terminal (PTY) key routing (#351, mirrors mod.rs:2439-:2513) ────
    // The engine decides the action; the backend performs the clipboard I/O
    // and the PTY writes.
    if engine.terminal_has_focus {
        use crate::core::engine::TerminalKeyAction;
        let mut tui_fn_buf = String::new();
        let (kn, uc) = match key_event.code {
            KeyCode::Enter => ("Return", None),
            KeyCode::Backspace => ("BackSpace", None),
            KeyCode::Esc => ("Escape", None),
            KeyCode::Tab => ("Tab", None),
            KeyCode::BackTab => ("ISO_Left_Tab", None),
            KeyCode::Up => ("Up", None),
            KeyCode::Down => ("Down", None),
            KeyCode::Left => ("Left", None),
            KeyCode::Right => ("Right", None),
            KeyCode::Home => ("Home", None),
            KeyCode::End => ("End", None),
            KeyCode::Delete => ("Delete", None),
            KeyCode::Insert => ("Insert", None),
            KeyCode::PageUp => ("Page_Up", None),
            KeyCode::PageDown => ("Page_Down", None),
            KeyCode::F(n) => {
                tui_fn_buf = format!("F{n}");
                (tui_fn_buf.as_str(), None)
            }
            KeyCode::Char(c) => ("", Some(c)),
            _ => ("", None),
        };
        let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key_event.modifiers.contains(KeyModifiers::ALT);
        match engine.handle_terminal_key(kn, uc, ctrl, shift, alt) {
            TerminalKeyAction::CopySelection => {
                let text = engine.active_terminal().and_then(|t| t.selected_text());
                if let Some(ref text) = text {
                    if let Some(ref cb) = engine.clipboard_write {
                        let _ = cb(text);
                    }
                    engine.message = "Copied".to_string();
                }
            }
            TerminalKeyAction::PasteClipboard => {
                let paste_text = engine
                    .clipboard_read
                    .as_ref()
                    .and_then(|cb| cb().ok())
                    .filter(|t| !t.is_empty())
                    .or_else(|| {
                        engine
                            .registers
                            .get(&'+')
                            .map(|(t, _)| t.clone())
                            .filter(|t| !t.is_empty())
                    })
                    .or_else(|| {
                        engine
                            .registers
                            .get(&'"')
                            .map(|(t, _)| t.clone())
                            .filter(|t| !t.is_empty())
                    });
                if let Some(text) = paste_text {
                    engine.terminal_write(b"\x1b[200~");
                    engine.terminal_write(text.as_bytes());
                    engine.terminal_write(b"\x1b[201~");
                    engine.poll_terminal();
                } else {
                    engine.message = "Nothing to paste".to_string();
                }
            }
            TerminalKeyAction::SendToPty(data) => {
                engine.terminal_write(&data);
                engine.poll_terminal();
            }
            TerminalKeyAction::Handled | TerminalKeyAction::Ignore => {}
        }
        return Reaction::Redraw;
    }

    // ── Alt-modifier block (mirrors mod.rs:2526-:2601) ──────────────────
    if key_event.modifiers.contains(KeyModifiers::ALT) {
        match key_event.code {
            // Alt+Left / Alt+Right: resize the sidebar.
            KeyCode::Left => {
                *state.sidebar_width = state.sidebar_width.saturating_sub(1).max(15);
                return Reaction::Redraw;
            }
            KeyCode::Right => {
                *state.sidebar_width = (*state.sidebar_width + 1).min(150);
                return Reaction::Redraw;
            }
            // Shift+Alt+F: LSP format document.
            KeyCode::Char('F') => {
                if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                    engine.lsp_format_current();
                    return Reaction::Redraw;
                }
            }
            // Alt+M: toggle Vim ↔ VSCode editing mode.
            KeyCode::Char('m') | KeyCode::Char('M') => {
                engine.toggle_editor_mode();
                return Reaction::Redraw;
            }
            // Alt+, / Alt+. — resize the editor group split.
            KeyCode::Char(',') => {
                engine.group_resize(-0.05);
                return Reaction::Redraw;
            }
            KeyCode::Char('.') => {
                engine.group_resize(0.05);
                return Reaction::Redraw;
            }
            // Alt+] / Alt+[ — cycle AI ghost-text alternatives.
            KeyCode::Char(']') => {
                if engine.mode == crate::core::Mode::Insert {
                    engine.ai_ghost_next_alt();
                    return Reaction::Redraw;
                }
            }
            KeyCode::Char('[') => {
                if engine.mode == crate::core::Mode::Insert {
                    engine.ai_ghost_prev_alt();
                    return Reaction::Redraw;
                }
            }
            // Alt+t is handled earlier (tab switcher).
            _ => {}
        }
        // VSCode mode: encode Alt+key into a key name for engine dispatch.
        if engine.is_vscode_mode() {
            let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
            let alt_key_name = match key_event.code {
                KeyCode::Up if shift => Some("Alt_Shift_Up"),
                KeyCode::Down if shift => Some("Alt_Shift_Down"),
                KeyCode::Up => Some("Alt_Up"),
                KeyCode::Down => Some("Alt_Down"),
                KeyCode::Char('z') | KeyCode::Char('Z') if !shift => Some("Alt_z"),
                _ => None,
            };
            if let Some(name) = alt_key_name {
                engine.handle_key(name, None, false);
                return Reaction::Redraw;
            }
        }
    }

    // ── Pre-load the system clipboard for paste keys (mirrors
    // mod.rs:2609-:2615) ─────────────────────────────────────────────────
    // `p`/`P` in normal/visual, Ctrl+V in VSCode mode. Detection and
    // register loading are shared engine methods (#381).
    if engine.needs_clipboard_for_paste(&key_name, unicode, ctrl) {
        let text = engine.clipboard_read.as_ref().and_then(|cb| cb().ok());
        engine.prepare_paste_clipboard(text);
    }

    // ── Ctrl+Shift+V: paste the system clipboard into the buffer (mirrors
    // mod.rs:2617-:2660) ─────────────────────────────────────────────────
    // With keyboard enhancement this event reaches the app instead of being
    // eaten by the terminal emulator. In Vim mode, load the clipboard into
    // the registers and replay `p`; in insert mode, insert the text.
    if ctrl && key_name == "V" && !engine.is_vscode_mode() {
        use crate::core::Mode;
        if let Some(ref cb_read) = engine.clipboard_read {
            if let Ok(text) = cb_read() {
                if !text.is_empty() {
                    engine.load_clipboard_for_paste(text);
                    match engine.mode {
                        Mode::Normal | Mode::Visual | Mode::VisualLine | Mode::VisualBlock => {
                            engine.handle_key("", Some('p'), false);
                        }
                        Mode::Insert | Mode::Replace => {
                            if let Some((content, _)) = engine.get_register_content('"') {
                                for ch in content.chars() {
                                    engine.handle_key(&ch.to_string(), Some(ch), false);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        return Reaction::Redraw;
    }

    // ── Shift+F5 → stop, Shift+F11 → stepout (mirrors mod.rs:2662-:2679) ─
    if key_event.modifiers.contains(KeyModifiers::SHIFT) {
        match key_event.code {
            KeyCode::F(5) => {
                let _ = engine.execute_command("stop");
                return Reaction::Redraw;
            }
            KeyCode::F(11) => {
                let _ = engine.execute_command("stepout");
                return Reaction::Redraw;
            }
            _ => {}
        }
    }

    // ── Command-line selection: Ctrl-C copies, any other key clears
    // (mirrors mod.rs:2651-:2701) ─────────────────────────────────────────
    // #635 (Stage 6b item D): `cmd_sel` itself is already populated by
    // mouse drag (`TuiShellApp::handle_mouse_event`, #602) and painted by
    // `panels::render_command_line` (#605) — this closes the keyboard
    // side, the last of item D's three named tiers.
    {
        use crate::core::Mode;
        let sel = cmd_sel.get();
        if ctrl && matches!(unicode, Some('c') | Some('C')) && sel.is_some() {
            if let Some((start, end)) = sel {
                let lo = start.min(end);
                let hi = start.max(end);
                // Determine the source text for the selection.
                let source = if matches!(engine.mode, Mode::Command | Mode::Search) {
                    // col 0 = ':' prefix, col 1+ = buffer chars
                    let buf_lo = lo.saturating_sub(1);
                    let buf_hi = hi.saturating_sub(1);
                    engine
                        .command_buffer
                        .chars()
                        .enumerate()
                        .filter(|(i, _)| *i >= buf_lo && *i <= buf_hi)
                        .map(|(_, c)| c)
                        .collect::<String>()
                } else {
                    // Normal mode message line — no prefix offset.
                    engine
                        .message
                        .chars()
                        .enumerate()
                        .filter(|(i, _)| *i >= lo && *i <= hi)
                        .map(|(_, c)| c)
                        .collect::<String>()
                };
                if !source.is_empty() {
                    tui_copy_to_clipboard(&source, engine);
                }
            }
            cmd_sel.set(None);
            return Reaction::Redraw;
        }
        if matches!(engine.mode, Mode::Command | Mode::Search) {
            // Any other key clears the selection.
            cmd_sel.set(None);
        } else if sel.is_some() {
            // In normal mode, any non-Ctrl-C key clears message selection.
            cmd_sel.set(None);
        }
    }

    // ── Context menu keyboard intercept (mirrors mod.rs:2703-:2706) ─────
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
        let cols = terminal_panel_cols(engine, screen_w, *state.sidebar_width);
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_new_tab(cols, rows);
    } else if action == EngineAction::ToggleTerminalMaximize {
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: terminal_panel_cols(engine, screen_w, *state.sidebar_width),
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

    // ── Post-key epilogue (mirrors mod.rs:2863-:2915) ───────────────────
    // Seven behaviours the legacy loop ran after *every* editor keypress.

    // Ctrl-W h/l overflow: move focus to the sidebar or the activity bar.
    if let Some(false) = engine.handle_nav_overflow() {
        if engine.app_shell.sidebar_visible() {
            sidebar.has_focus = true;
        } else {
            // No sidebar panel visible — focus the activity bar instead.
            let idx = engine.activity_bar_toolbar_idx_for_active_panel();
            engine.activity_bar_focus_in_at(idx);
        }
    }

    // Auto-hide the sidebar when focus returns to the editor.
    // (`sidebar_has_focus()` includes `activity_bar_focused`, so autohide is
    // suppressed while the user navigates the toolbar.)
    if engine.should_autohide_sidebar() {
        engine.app_shell.hide_sidebar();
    }

    // Drain macro playback (`@q`), which can itself request a quit.
    loop {
        let (has_more, action) = engine.advance_macro_playback();
        if handle_action(engine, action) {
            return Reaction::Exit;
        }
        if !has_more {
            break;
        }
    }

    // Sync the unnamed register → system clipboard (`clipboard=unnamedplus`).
    sync_tui_clipboard(engine, state.last_clipboard_content);

    // Rebuild the explorer tree if a file move just completed.
    if engine.explorer_needs_refresh {
        engine.explorer_needs_refresh = false;
        engine.explorer_rebuild_rows();
    }

    // Arm the 200 ms yank-highlight expiry (`tick` clears it).
    if engine.yank_highlight.is_some() {
        state
            .yank_hl_deadline
            .set(Some(Instant::now() + Duration::from_millis(200)));
    }

    // Keep the selected quickfix entry visible.
    if engine.quickfix_open {
        const QF_VISIBLE: usize = 5; // 6 rows − 1 header
        if engine.quickfix_selected < *state.quickfix_scroll_top {
            *state.quickfix_scroll_top = engine.quickfix_selected;
        } else if engine.quickfix_selected >= *state.quickfix_scroll_top + QF_VISIBLE {
            *state.quickfix_scroll_top = engine.quickfix_selected + 1 - QF_VISIBLE;
        }
    } else {
        *state.quickfix_scroll_top = 0;
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

    /// Owns the values a [`KeyDispatchState`] borrows, so the direct
    /// (`driver`-bypassing) `handle_key_pressed` tests can build one without
    /// declaring six locals apiece. Fields stay public to the test module so
    /// a test can seed (`scratch.cmd_sel.set(..)`) or assert on
    /// (`scratch.sidebar_width`) any of them.
    struct KeyScratch {
        sidebar_width: u16,
        quickfix_scroll_top: usize,
        last_clipboard_content: Option<String>,
        cmd_sel: Cell<Option<(usize, usize)>>,
        yank_hl_deadline: Cell<Option<Instant>>,
        /// Only the debug/DAP sidebar tier reads this, and no direct test
        /// exercises that panel today — any non-key event is inert there.
        ui_event: UiEvent,
    }

    impl KeyScratch {
        fn new() -> Self {
            Self {
                sidebar_width: SIDEBAR_WIDTH,
                quickfix_scroll_top: 0,
                last_clipboard_content: None,
                cmd_sel: Cell::new(None),
                yank_hl_deadline: Cell::new(None),
                ui_event: UiEvent::WindowFocused(true),
            }
        }

        fn state(&mut self) -> KeyDispatchState<'_> {
            KeyDispatchState {
                sidebar_width: &mut self.sidebar_width,
                quickfix_scroll_top: &mut self.quickfix_scroll_top,
                last_clipboard_content: &mut self.last_clipboard_content,
                cmd_sel: &self.cmd_sel,
                yank_hl_deadline: &self.yank_hl_deadline,
                ui_event: &self.ui_event,
            }
        }
    }
    use quadraui::Backend as _;

    fn config() -> quadraui::ShellConfig {
        let mut cfg = quadraui::ShellConfig::new(
            "VimCode",
            vec![quadraui::PanelDefinition {
                id: quadraui::WidgetId::new("panel:explorer"),
                title: "Explorer".to_string(),
                icon: String::new(),
                tooltip: String::new(),
            }],
        );
        // Match `TuiShellApp::shell_config`'s 1-row title bar rather than
        // `ShellConfig`'s own 1.5-line-height default, so tests that reveal
        // the menu bar at runtime through this minimal config measure the
        // same reservation the live config produces (quadraui#547).
        cfg.title_bar_height_lh = 1.0;
        cfg
    }

    /// A [`TuiShellApp`] whose sidebar is *deterministically* open.
    ///
    /// `TuiShellApp::new` runs the real `Engine::new`, which reads the
    /// developer's real `~/.config/vimcode` — `settings.json` and the
    /// global `session.json` — off disk. Sidebar visibility at
    /// construction is therefore **ambient**, not fixed:
    ///
    /// ```text
    /// // core/engine/mod.rs, Engine::new
    /// let show_sidebar = if settings.autohide_panels { false }
    ///     else { session.explorer_visible || settings.explorer_visible_on_startup };
    /// if !show_sidebar { engine.app_shell.hide_sidebar(); }
    /// ```
    ///
    /// Both inputs default to `false` (`settings.rs`'s
    /// `default_explorer_visible`, `session.rs`'s `Session` default), so on
    /// a machine with **no** vimcode config — every CI runner, and any
    /// fresh checkout — the sidebar boots *hidden*, while on a developer
    /// box that has ever opened the explorer it boots *visible*. Five tests
    /// in this module were written on the latter and silently inherited
    /// "sidebar open" as an unstated precondition; they passed locally and
    /// failed on CI (#634). Anything that reads `sidebar_visible()`, or
    /// that measures the editor pane's geometry, must pin this itself
    /// rather than inherit it.
    ///
    /// `show_panel` is the same call the production activity-bar click path
    /// makes: it sets the active panel *and* `sidebar_visible = true`.
    /// `session.explorer_visible` is the shadow half that
    /// `AppShellEvent::SidebarHidden` is supposed to clear, so it is seeded
    /// too — otherwise a test asserting that it ends up `false` would pass
    /// vacuously.
    fn app_with_sidebar_open() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        app.engine.session.explorer_visible = true;
        app
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

    /// #635 (Stage 6b item F): `live` must default to `false` (so every
    /// `driver_with_shell` test skips the blocking terminal query and the
    /// unsafe emergency-engine registration in `setup()` — see that
    /// field's doc comment), and `prepare_for_live_run` must only set the
    /// flag, not perform either of the operations it gates — so it's safe
    /// to call on an instance that's never actually handed to
    /// `run_with_shell`, like this one.
    #[test]
    fn prepare_for_live_run_only_sets_the_flag() {
        let mut app = TuiShellApp::new(None);
        assert!(!app.live);
        app.prepare_for_live_run();
        assert!(app.live);
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

    /// #635 (Stage 6b item E): [`TuiShellApp::shell_config`] must register
    /// exactly the panels `render::build_activity_bar`'s `fixed` array
    /// plus the hamburger (top) and settings (bottom) — the two items
    /// outside that array — in the same order, so the eventual live
    /// `AppShell` activity bar (#634) can't drift from what `draw_frame`
    /// paints today. The middle six ids are asserted against
    /// `sidebar::FIXED_ACTIVITY_PANEL_IDS` directly — the same array
    /// `shell_config` zips its metadata against and `build_activity_bar`
    /// debug-asserts its own `fixed` order against — rather than a
    /// hand-transcribed literal, so a reordering of the shared constant
    /// changes what this test expects automatically instead of needing a
    /// matching hand-edit here.
    #[test]
    fn shell_config_registers_every_build_activity_bar_panel() {
        let cfg = TuiShellApp::shell_config(false);
        let ids: Vec<&str> = cfg.panels.iter().map(|p| p.id.as_str()).collect();
        let mut expected = vec![HAMBURGER_PANEL_ID];
        expected.extend(crate::core::engine::sidebar::FIXED_ACTIVITY_PANEL_IDS);
        assert_eq!(ids, expected);
        assert_eq!(cfg.bottom_items.len(), 1);
        assert_eq!(cfg.bottom_items[0].id.as_str(), PANEL_SETTINGS);
    }

    /// The live config (unlike the single-panel `config()` test helper
    /// above) must also construct and paint a first frame without
    /// panicking through the real `driver_with_shell` harness — the same
    /// end-to-end smoke `shell_app_constructs_via_driver_with_shell` runs
    /// for the test-only config.
    #[test]
    fn shell_app_constructs_via_driver_with_shell_using_live_config() {
        let driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        let _ = driver.screen();
    }

    /// #635 (Stage 6b item E): a hamburger click reports as an ordinary
    /// `AppShellEvent::PanelChanged` (`AppShell` doesn't know the
    /// hamburger is special) — `on_shell_event` must recognize it by id
    /// and reveal the menu bar instead of leaving it to be silently
    /// mistaken for a sidebar-panel switch. Drives `on_shell_event`
    /// directly (mirrors `handle_key_pressed_dialog_intercepts_all_keys`'s
    /// approach below) since `ShellAdapter`'s fields are `pub(crate)` and
    /// there is no accessor from `driver_with_shell` back to this event.
    // #693 (quadraui#617): `on_shell_event` is `deprecated` in favour of
    // `on_shell_event_ctx`, kept purely for back-compat and now delegated
    // to by this file's `on_shell_event_ctx` override. This test drives
    // the deprecated hook directly on purpose (see its doc comment above)
    // rather than through a `ShellContext`, which has no public
    // constructor outside quadraui.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_hamburger_click_reveals_menu_bar() {
        let mut app = TuiShellApp::new(None);
        assert!(!app.engine.menu_bar_visible);
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(HAMBURGER_PANEL_ID),
        });
        assert!(app.engine.menu_bar_visible);
    }

    /// A `PanelChanged` for a real panel must NOT trip the hamburger
    /// special-case.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_real_panel_click_does_not_reveal_menu_bar() {
        let mut app = TuiShellApp::new(None);
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_EXPLORER),
        });
        assert!(!app.engine.menu_bar_visible);
    }

    // ── #634 smoke retry: runner-shell → shadow panel sync ──────────────
    //
    // The smoke test found activity-bar clicks switching only the runner
    // `AppShell`'s sidebar-header title while `render_sidebar_content`
    // (which dispatches on the *shadow* `engine.app_shell`) kept painting
    // Explorer forever. These tests pin the `on_shell_event` /
    // `take_requested_panel` bridge that closes the split.

    /// The core smoke-retry failure: a `PanelChanged` for a real panel
    /// (what `ShellAdapter` reports for an activity-bar click it consumed)
    /// must switch the shadow `engine.app_shell` — the state
    /// `render_sidebar_content` actually paints from — not just be ignored.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_panel_changed_switches_shadow_sidebar_content() {
        let mut app = TuiShellApp::new(None);
        assert!(app.engine.active_panel_is(PANEL_EXPLORER));
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_SEARCH),
        });
        assert!(
            app.engine.active_panel_is(PANEL_SEARCH),
            "a runner-consumed activity-bar click must reach the shadow \
             app_shell, or the sidebar content pane stays on Explorer \
             while the header claims Search (the #634 smoke failure)"
        );
        assert!(app.engine.app_shell.sidebar_visible());
        assert!(app.sidebar.has_focus);
    }

    /// `render_sidebar_content` checks `sidebar.ext_panel_name` *before*
    /// the active-panel dispatch, so a lingering plugin-panel takeover
    /// would keep painting over any panel the user clicks — mirror the
    /// legacy `mouse.rs` arm's clearing of all three plugin-panel fields.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_panel_changed_clears_plugin_ext_panel_takeover() {
        let mut app = TuiShellApp::new(None);
        app.sidebar.ext_panel_name = Some("git-insights".to_string());
        app.engine.ext_panel_active = Some("git-insights".to_string());
        app.engine.ext_panel_has_focus = true;
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_GIT),
        });
        assert!(app.sidebar.ext_panel_name.is_none());
        assert!(app.engine.ext_panel_active.is_none());
        assert!(!app.engine.ext_panel_has_focus);
        assert!(app.engine.active_panel_is(PANEL_GIT));
    }

    /// Second click on the active icon: the runner hides its own sidebar
    /// and reports `SidebarHidden` — the shadow must follow, or every
    /// `sidebar_visible()` consumer (tick viewport math, mouse hit tests,
    /// autohide, session persistence) keeps believing it's open.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_sidebar_hidden_syncs_shadow() {
        // Sidebar-open is this test's *precondition*, not its subject —
        // and it is ambient on a bare `TuiShellApp::new`. See
        // `app_with_sidebar_open`.
        let mut app = app_with_sidebar_open();
        assert!(app.engine.app_shell.sidebar_visible());
        app.on_shell_event(&quadraui::AppShellEvent::SidebarHidden);
        assert!(!app.engine.app_shell.sidebar_visible());
        assert!(!app.engine.session.explorer_visible);
    }

    /// The Settings cog is a *bottom item* (`shell_config`), for which
    /// `AppShell` runs no panel toggle of its own — `on_shell_event` must
    /// run the legacy toggle on the shadow, both directions.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_settings_bottom_item_toggles_shadow() {
        let mut app = TuiShellApp::new(None);
        let ev = quadraui::AppShellEvent::BottomItemClicked {
            id: quadraui::WidgetId::new(PANEL_SETTINGS),
        };
        app.on_shell_event(&ev);
        assert!(app.engine.active_panel_is(PANEL_SETTINGS));
        assert!(app.engine.app_shell.sidebar_visible());
        app.on_shell_event(&ev);
        assert!(
            !app.engine.app_shell.sidebar_visible(),
            "second Settings click must toggle the sidebar closed, \
             mirroring the legacy mouse.rs ActivityBarTarget::Settings arm"
        );
    }

    /// `take_requested_panel` is the keyboard/tick half of the sync:
    /// after an engine-side switch (keyboard tier, DAP reveal) it must
    /// hand the runner the new panel exactly once, and the `PanelChanged`
    /// echo `ShellAdapter::apply_requested_panel` sends back must settle
    /// the loop rather than re-firing forever.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn take_requested_panel_reconciles_keyboard_switch_once() {
        // `take_requested_panel` short-circuits to `None` while the sidebar
        // is hidden, and sidebar visibility on a bare `TuiShellApp::new` is
        // ambient — see `app_with_sidebar_open`.
        let mut app = app_with_sidebar_open();
        // Startup reconciliation: runner boots on the hamburger (panel
        // index 0), shadow on Explorer — first poll must correct that.
        let first = app.take_requested_panel();
        assert_eq!(
            first.as_ref().map(|w| w.as_str().to_string()),
            Some(PANEL_EXPLORER.to_string())
        );
        // Echo, as apply_requested_panel sends it.
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_EXPLORER),
        });
        assert!(app.take_requested_panel().is_none(), "echo must settle");

        // Keyboard-style switch on the shadow only.
        app.engine.focus_sidebar_panel(PANEL_SEARCH);
        let req = app.take_requested_panel();
        assert_eq!(
            req.as_ref().map(|w| w.as_str().to_string()),
            Some(PANEL_SEARCH.to_string()),
            "an engine-side panel switch must be offered to the runner, or \
             the activity-bar highlight/header stays on the old panel"
        );
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_SEARCH),
        });
        assert!(app.take_requested_panel().is_none());
    }

    /// The reconciliation echo must not re-run the click path — the
    /// engine already holds the state, and re-running it would steal
    /// focus into the sidebar (e.g. `explorer_has_focus = true` on the
    /// startup frame, yanking key routing away from the editor).
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn take_requested_panel_echo_does_not_steal_focus() {
        // Needs the sidebar open, or the `take_requested_panel` below
        // returns `None`, never arms `suppress_shell_panel_echo`, and the
        // echo is indistinguishable from a real click — see
        // `app_with_sidebar_open`.
        let mut app = app_with_sidebar_open();
        assert!(!app.engine.explorer_has_focus);
        let _ = app.take_requested_panel(); // returns Some(explorer), arms suppress
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_EXPLORER),
        });
        assert!(
            !app.engine.explorer_has_focus && !app.sidebar.has_focus,
            "the apply_requested_panel echo must only update the runner-state \
             belief, not steal focus like a user click"
        );
    }

    /// A hamburger click leaves the runner's active-panel index on the
    /// hamburger; the next `take_requested_panel` poll must steer it back
    /// to the shadow's real panel (so the sidebar header shows "Menu" for
    /// one frame at most, not until the next real panel click).
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn take_requested_panel_restores_runner_after_hamburger_click() {
        // Green on a hidden sidebar too, but only by accident: the first
        // poll returns `None`, so the echo below is *not* suppressed and
        // re-runs the click path, which happens to open the sidebar in
        // time for the real assertion. Pin the precondition instead.
        let mut app = app_with_sidebar_open();
        // Settle the startup reconciliation first.
        let _ = app.take_requested_panel();
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(PANEL_EXPLORER),
        });
        // Hamburger click, as the runner reports it.
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new(HAMBURGER_PANEL_ID),
        });
        assert!(app.engine.menu_bar_visible);
        assert_eq!(
            app.take_requested_panel()
                .as_ref()
                .map(|w| w.as_str().to_string()),
            Some(PANEL_EXPLORER.to_string())
        );
    }

    /// End-to-end through the real `driver_with_shell` harness + live
    /// config: a mouse click on the Search icon (activity-bar row 2 —
    /// hamburger@0, Explorer@1, Search@2, no title bar) must switch the
    /// sidebar *content* pane, and a second click on the same icon must
    /// toggle the sidebar closed — the exact #634 smoke-retry checklist
    /// item, driven through `ShellAdapter`'s real click consumption.
    /// `driver_with_shell` returns an opaque `TuiDriver<impl AppLogic>`
    /// (no path back to `TuiShellApp`'s fields), so the assertions read
    /// the painted grid: "Replace…" is the search form's replace-input
    /// placeholder — content only `render_search_panel` paints, never the
    /// explorer tree or the runner's own " Search " header chrome (which
    /// updated even while the content pane was stuck — the smoke bug).
    #[test]
    fn driver_click_on_search_icon_switches_and_toggles_sidebar() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        assert!(
            !driver.screen_contains("Replace…"),
            "precondition: startup sidebar shows Explorer, not Search"
        );
        driver.click(1.0, 2.0);
        assert!(
            driver.screen_contains("Replace…"),
            "clicking the Search icon must switch the sidebar content pane \
             (render_sidebar_content reads the shadow app_shell), not just \
             the runner's header title — screen was:\n{}",
            driver.screen()
        );

        driver.click(1.0, 2.0);
        assert!(
            !driver.screen_contains("Replace…"),
            "second click on the active icon must toggle the sidebar closed"
        );
    }

    // ── #557: extension panels in the migrated activity bar ─────────────
    //
    // The Git Insights extension registers a display name *and* an icon
    // (`core/extensions.rs`), and `render::build_activity_bar` — the legacy
    // `draw_frame` path — has appended an `ActivityItem` for it since #133.
    // The migrated `AppShell` bar is driven by `ShellConfig`/`PanelDefinition`
    // instead, and nothing fed extension panels into it, so the icon vanished
    // entirely on the `ShellApp` path.

    /// The single distinguishing glyph these tests look for on the painted
    /// screen. Deliberately not a Nerd Font glyph: `resolved_icon()` returns
    /// the *fallback* when Nerd Fonts are off, and these tests pin that flag
    /// off so the assertion can't depend on the developer's `use_nerd_fonts`
    /// setting (the ambient-config trap `app_with_sidebar_open`'s doc comment
    /// documents for sidebar visibility).
    const EXT_ICON: char = 'Ж';

    fn ext_panel_reg(name: &str, title: &str) -> crate::core::plugin::PanelRegistration {
        crate::core::plugin::PanelRegistration {
            name: name.to_string(),
            title: title.to_string(),
            icon: '\u{f113}',
            fallback_icon: Some(EXT_ICON),
            sections: Vec::new(),
        }
    }

    /// A `TuiShellApp` whose *only* extension panel is a synthetic
    /// "git-insights", with Nerd Fonts pinned off.
    ///
    /// `ext_panels` is cleared first because `TuiShellApp::new` runs the real
    /// `Engine::new`, which loads whatever plugins the developer has installed
    /// — on a machine with the real Git Insights extension the map would
    /// already be populated and the assertions would be measuring that instead
    /// of this fixture.
    fn app_with_ext_panel() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.use_nerd_fonts = false;
        crate::icons::set_nerd_fonts(false);
        app.engine.ext_panels.clear();
        app.engine.ext_panels.insert(
            "git-insights".to_string(),
            ext_panel_reg("git-insights", "Git Insights"),
        );
        app
    }

    /// #557, the headline acceptance: a plugin-registered panel must
    /// contribute a **visible** activity-bar icon on the `ShellApp` path.
    /// Black-box through the real `driver_with_shell` harness and the live
    /// config — the assertion reads the painted grid, not
    /// `ShellConfig::panels`, so a definition that is registered but never
    /// rasterised would still fail.
    #[test]
    fn driver_paints_a_registered_extension_panels_activity_bar_icon() {
        let app = app_with_ext_panel();
        let cfg = TuiShellApp::live_shell_config(&app.engine);
        let driver = driver_with_shell(app, cfg, 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains(EXT_ICON),
            "an extension-registered panel must paint an activity-bar icon \
             (#557: the Git Insights icon was missing entirely); screen:\n{screen}"
        );
    }

    /// The same claim for a panel registered *after* startup — plugins can
    /// register at any time (`Engine::apply_plugin_ctx` drains
    /// `ctx.panel_registrations` after every Lua callback), so seeding
    /// `ShellConfig` alone is not enough. Constructed with the *static*
    /// `shell_config` (no extension panels at all), which is exactly the
    /// runner's state at the moment a plugin registers mid-session; only
    /// `handle()`'s `sync_ext_activity_panels` can make the icon appear.
    #[test]
    fn driver_paints_an_extension_panel_registered_after_the_first_frame() {
        let app = app_with_ext_panel();
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);
        assert!(
            !driver.screen().contains(EXT_ICON),
            "precondition: the static config carries no extension panels"
        );

        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let screen = driver.screen();
        assert!(
            screen.contains(EXT_ICON),
            "a panel registered after startup must be synced into the live \
             AppShell on the next dispatch; screen:\n{screen}"
        );
    }

    /// End-to-end: clicking the extension panel's icon must open *that*
    /// panel's body. Row 7 = hamburger@0, explorer@1, search@2, debug@3,
    /// git@4, extensions@5, ai@6, ext@7 — the same index
    /// `resolve_activity_bar_click` (the legacy mouse path) assigns, since
    /// `Engine::ext_activity_panels` appends in the same sorted order.
    /// "Git Insights" is the panel *title*, which only `render_ext_panel`
    /// paints into the sidebar body.
    #[test]
    fn driver_click_on_extension_icon_opens_the_plugin_panel() {
        let app = app_with_ext_panel();
        let cfg = TuiShellApp::live_shell_config(&app.engine);
        let mut driver = driver_with_shell(app, cfg, 80, 24);
        // The runner boots with the hamburger active; one benign event lets
        // `take_requested_panel` steer it onto the shadow's real panel first
        // (see `menu_reveal_then_search_icon_click_opens_search_not_explorer`).
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();

        driver.click(1.0, 7.0);
        let screen = driver.screen();
        assert!(
            screen.to_uppercase().contains("GIT INSIGHTS"),
            "clicking an extension panel's activity-bar icon must open its \
             sidebar body; screen:\n{screen}"
        );
    }

    /// Unit half of the click path: `AppShell` reports an extension icon
    /// click as an ordinary `PanelChanged` (it has no notion of "extension"),
    /// so `on_shell_event` must recognise the `"ext:"` id and run the
    /// plugin-panel bookkeeping — `focus_sidebar_panel` would fall through to
    /// the explorer, leaving the sidebar painting the file tree under a
    /// highlighted extension icon.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_extension_panel_changed_opens_the_plugin_panel() {
        let mut app = app_with_ext_panel();
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new("ext:git-insights"),
        });
        assert_eq!(app.sidebar.ext_panel_name.as_deref(), Some("git-insights"));
        assert_eq!(app.engine.ext_panel_active.as_deref(), Some("git-insights"));
        assert!(app.engine.ext_panel_has_focus);
        assert!(app.engine.app_shell.sidebar_visible());
        assert!(
            !app.engine.explorer_has_focus,
            "the explorer must not also claim focus — `focus_sidebar_panel` \
             is the wrong call for an extension id"
        );
    }

    /// The second click on an open extension panel's icon arrives as
    /// `SidebarHidden` (the runner made the toggle decision itself), which
    /// must drop the plugin-panel state too — otherwise
    /// `take_requested_panel` keeps steering the runner back onto a panel
    /// whose sidebar the user just closed.
    // #693 (quadraui#617): see the `#[allow(deprecated)]` note on
    // `on_shell_event_hamburger_click_reveals_menu_bar`, above.
    #[allow(deprecated)]
    #[test]
    fn on_shell_event_sidebar_hidden_clears_extension_panel_state() {
        let mut app = app_with_ext_panel();
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new("ext:git-insights"),
        });
        app.on_shell_event(&quadraui::AppShellEvent::SidebarHidden);
        assert!(app.sidebar.ext_panel_name.is_none());
        assert!(app.engine.ext_panel_active.is_none());
        assert!(!app.engine.ext_panel_has_focus);
        assert!(!app.engine.app_shell.sidebar_visible());
    }

    /// While an extension panel is open the shadow `app_shell`'s active-panel
    /// id still names whatever built-in panel preceded it (extension panels
    /// deliberately never touch it), so `take_requested_panel` has to follow
    /// `ext_panel_active` instead — or every dispatch would offer the runner
    /// the *old* built-in panel and the extension icon's highlight would drop
    /// off on the very next event.
    #[test]
    fn take_requested_panel_follows_the_open_extension_panel() {
        let mut app = app_with_sidebar_open();
        app.last_shell_panel = Some(quadraui::WidgetId::new(PANEL_EXPLORER));
        app.engine.ext_panel_active = Some("git-insights".to_string());
        assert_eq!(
            app.take_requested_panel()
                .as_ref()
                .map(|w| w.as_str().to_string()),
            Some("ext:git-insights".to_string())
        );
        app.last_shell_panel = Some(quadraui::WidgetId::new("ext:git-insights"));
        assert!(
            app.take_requested_panel().is_none(),
            "the reconciliation must settle once the runner is on the \
             extension panel"
        );
    }

    /// `live_shell_config` == `shell_config` + the extension panels, in that
    /// order: the built-in ids must keep their positions (every activity-bar
    /// row index in this module's other tests depends on it) and the
    /// extensions must land after them.
    #[test]
    fn live_shell_config_appends_extension_panels_after_the_builtins() {
        let app = app_with_ext_panel();
        let base: Vec<String> = TuiShellApp::shell_config(false)
            .panels
            .iter()
            .map(|p| p.id.as_str().to_string())
            .collect();
        let live: Vec<String> = TuiShellApp::live_shell_config(&app.engine)
            .panels
            .iter()
            .map(|p| p.id.as_str().to_string())
            .collect();
        let mut expected = base;
        expected.push("ext:git-insights".to_string());
        assert_eq!(live, expected);
    }

    /// #634 recurring smoke bug — activity-bar click off-by-one after the
    /// menu bar is revealed at runtime. **Fixed upstream by quadraui#552
    /// (`7c8209d`); this test is the regression guard.**
    ///
    /// The root cause was in quadraui, not vimcode: the TUI rasteriser
    /// (`quadraui/src/tui/activity_bar.rs::draw_activity_bar`) returned
    /// `ActivityBarRowHit`s in **absolute** rows (`y_start: area.y +
    /// vi.bounds.y`), while every other producer — the GTK and macOS
    /// rasterisers and the shared no-paint helper
    /// (`backend.rs::activity_bar_hits`, which `activity_bar_layout` uses)
    /// — returned **rect-relative** rows. `AppShell::cached_activity_hit`
    /// (and `update_hover`) assume rect-relative and add the cached bar
    /// bounds' `ab.y` on top. While the menu bar was hidden `ab.y == 0`
    /// and the double-add was invisible; the moment
    /// `set_title_bar_visible(true)` reserved the title-bar row,
    /// `ab.y == 1` and every hit region shifted down one row — a click on
    /// Search's painted row fell inside Explorer's shifted region, and
    /// the offset persisted for as long as the menu bar stayed visible.
    /// Paint was unaffected (it doesn't read the hit cache), matching the
    /// smoke report's "visually correct, click mapping wrong".
    ///
    /// Per CLAUDE.md's Platform-Neutrality Rule the fix belonged in
    /// quadraui (make the TUI rasteriser return rect-relative hits like
    /// its three siblings), not in a vimcode patch — there was no
    /// vimcode-side hook anyway: `ShellAdapter::handle` runs
    /// `AppShell::handle`'s hit-test before the app ever sees the event.
    /// quadraui#552 landed exactly that, and #650 moved
    /// `quadraui-pin.txt` onto it (`f702422` → `f6d27c2`), which is what
    /// let this test run. It asserts the *correct* behaviour and fails on
    /// any pin older than the fix — so a red here means the pin went
    /// backwards, not that vimcode regressed.
    #[test]
    fn menu_reveal_then_search_icon_click_opens_search_not_explorer() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        assert!(
            !driver.screen_contains("Replace…"),
            "precondition: startup sidebar shows Explorer, not Search"
        );
        // Benign event first: `ShellAdapter` polls `take_requested_panel`
        // only after a `handle()`/`tick()`, and the runner boots with the
        // hamburger active (AppShell::new activates index 0) — the live
        // loop's constant ticks steer it onto the shadow's Explorer before
        // any user click, so replicate that here or the first hamburger
        // click reads as "hide the active panel" instead of PanelChanged.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        // Hamburger is row 0 with the menu bar hidden.
        driver.click(1.0, 0.0);
        // The title-bar reservation syncs at the end of
        // `TuiShellApp::handle` — which `ShellAdapter`'s PanelChanged arm
        // returns before reaching. Live, the next mouse-move covers it;
        // here, pump one more benign event.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let screen = driver.screen();
        assert!(
            screen.contains("File"),
            "menu bar should now be visible; screen:\n{screen}"
        );
        // Menu bar visible -> title-bar row reserved -> activity bar starts
        // at row 1: hamburger@1, Explorer@2, Search@3.
        driver.click(1.0, 3.0);
        let screen = driver.screen();
        assert!(
            screen.contains("Replace…"),
            "clicking the Search icon (row 3 with menu bar visible) must \
             open the Search panel; screen was:\n{screen}"
        );
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

    /// #674: `Ctrl-O` across tabs must *activate* the tab the jump was
    /// recorded in, not reopen the file into whatever pane happens to be
    /// current. Drives real key input (`ctrl_char('o')`) through the
    /// `TuiDriver` and asserts on the rendered tab bar and editor body —
    /// the black-box tier the #674 acceptance criteria calls for, not just
    /// an `Engine`-internal check that `jump_list` got populated.
    ///
    /// This fails against unfixed `develop`: the old
    /// `apply_jump_list_entry` only compared `(file, line, col)`, so the
    /// second `Ctrl-O` would move the cursor inside tab B's own buffer
    /// instead of switching back to tab A — the painted screen would still
    /// show `BBB674`, not `AAA674`.
    #[test]
    fn ctrl_o_activates_original_tab_via_shell_app() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_674_shell_app_jumplist_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file_a = dir.join("a674.txt");
        let file_b = dir.join("b674.txt");
        let content_a: String = (0..30).map(|i| format!("AAA674 line {}\n", i)).collect();
        let content_b: String = (0..30).map(|i| format!("BBB674 line {}\n", i)).collect();
        std::fs::write(&file_a, &content_a).unwrap();
        std::fs::write(&file_b, &content_b).unwrap();

        let mut app = TuiShellApp::new(None);
        app.engine
            .open_file_with_mode(&file_a, crate::core::engine::OpenMode::Permanent)
            .unwrap();
        app.engine.handle_key("G", Some('G'), false); // push jump in tab A; cursor -> bottom

        app.engine.new_tab(Some(&file_b));
        app.engine.handle_key("G", Some('G'), false); // push jump in tab B; cursor -> bottom
        assert_eq!(app.engine.active_group().active_tab, 1);

        let mut driver = driver_with_shell(app, config(), 100, 24);

        // First Ctrl-O stays inside tab B's own history.
        driver.ctrl_char('o');
        let screen = driver.screen();
        assert!(
            screen.contains("BBB674"),
            "first Ctrl-O should stay in tab B; screen:\n{screen}"
        );

        // Second Ctrl-O must switch back to tab A: the tab bar keeps
        // exactly one entry per file (no fresh reopen), and the editor
        // body paints A's text, not a stale copy of B's.
        driver.ctrl_char('o');
        let screen = driver.screen();
        let tab_row = screen.lines().next().unwrap_or_default();
        assert_eq!(
            tab_row.matches("a674.txt").count(),
            1,
            "tab bar should show exactly one a674.txt tab (no fresh reopen); row:\n{tab_row}"
        );
        assert_eq!(
            tab_row.matches("b674.txt").count(),
            1,
            "tab bar should still show exactly one b674.txt tab; row:\n{tab_row}"
        );
        assert!(
            screen.contains("AAA674"),
            "second Ctrl-O should activate tab A's own pane and paint its text; screen:\n{screen}"
        );
        assert!(
            !screen.contains("BBB674"),
            "tab A's pane must not still be showing tab B's text; screen:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #551: the unsplit (single editor group) case must still paint exactly
    /// one full-width tab bar on the editor's top row, and no group divider.
    ///
    /// This is the black-box guard for collapsing the single-group draw arm
    /// into the generic N-group one. Both backends used to carry a
    /// hand-written `else { /* exactly one group */ }` block; the tab bar is
    /// now drawn from `ScreenLayout::group_tab_bars` (one entry) and the
    /// dividers from `ScreenLayout::group_dividers` (empty) for every group
    /// count. If the split-of-1 bounds ever stopped reproducing the old
    /// hard-coded editor-origin rect, the tab label would move off row 0 or
    /// vanish — which is exactly what this asserts through the real
    /// `driver_with_shell` paint path.
    ///
    /// Paired with `render_content_paints_group_divider_via_shell_app` below,
    /// which pins the N >= 2 half of the same unified path.
    #[test]
    fn render_content_paints_single_group_tab_bar_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "short\n");
        assert_eq!(
            app.engine.group_layout.leaf_count(),
            1,
            "this test covers the unsplit case"
        );
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
            1,
            "an unsplit editor must paint exactly one tab bar, on row 0; row:\n{tab_row}"
        );

        // No editor-group divider exists with one group, so no '│' may appear
        // at or right of the tab label. (The sidebar, entirely to the left of
        // the tab label, paints its own unrelated '│' indent guides — those
        // are not the glyph under test, same carve-out the split-group test
        // makes.)
        let tab_start = starts[0];
        for (y, line) in lines.iter().enumerate().skip(1).take(15) {
            let stray = line
                .chars()
                .enumerate()
                .skip(tab_start)
                .find(|(_, c)| *c == '│')
                .map(|(i, _)| i);
            assert!(
                stray.is_none(),
                "row {y}: unexpected group-divider glyph at col {:?} with a \
                 single editor group; line:\n{line}",
                stray
            );
        }
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
    /// divider must land at or after the left pane's own tab label and no
    /// further right than the right pane's tab label — inclusive on the
    /// right because #700 dropped the tab label's leading `" N: "` pad, so
    /// a pane's tab text now starts in the *same* column its body divider
    /// occupies, rather than one-plus columns to the right of it.
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
                col > left_tab_start && col <= right_tab_start,
                "row {y}: divider at col {col} should land between the two \
                 panes' tab labels (cols {left_tab_start}..={right_tab_start}); \
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

    // ── #605 (Stage 6 parity sweep) ────────────────────────────────────────
    //
    // The rest of `draw_frame`'s tail, each asserted through
    // `driver_with_shell` so the claim is "it reaches the painted screen",
    // not "the call compiles".

    /// The `:`-command row must paint. In Normal mode `build_command_line`
    /// renders `engine.message` verbatim, which is the cheapest deterministic
    /// way to get known text onto that row — and it specifically exercises
    /// the trait-pure rewrite of `panels::render_command_line` (#605 replaced
    /// its `frame.buffer_mut()` `set_cell` loop with the
    /// `Backend::draw_status_bar` rule-row trick).
    #[test]
    fn render_content_paints_command_line_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.message = "ZQXW_605_CMDLINE_MARKER".to_string();

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW_605_CMDLINE_MARKER"),
            "command line should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// Command mode paints the typed `:command` *and* its inverted block
    /// cursor. The cursor is a colour inversion, invisible to `screen()`'s
    /// text dump, so this asserts on the text and relies on the run-batching
    /// in `render_command_line` not swallowing cells: a cursor mid-string
    /// splits the row into three colour runs, so a bug there would drop or
    /// duplicate characters rather than merely mis-colour them.
    #[test]
    fn render_content_paints_command_mode_text_with_cursor_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.mode = crate::core::Mode::Command;
        app.engine.command_buffer = "ZQXW605CMD".to_string();
        app.engine.command_cursor = 4;

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains(":ZQXW605CMD"),
            "command-mode text should paint intact around the inverted cursor cell; screen:\n{screen}"
        );
    }

    /// A modal dialog must paint *and* cache its `DialogLayout` — the layout
    /// is what `handle_key_pressed`'s dialog tier and `handle_mouse_event`
    /// hit-test against, so a paint that doesn't publish it is only half
    /// wired.
    #[test]
    fn render_content_paints_dialog_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.dialog = Some(crate::core::engine::Dialog {
            title: "ZQXW605DIALOG".to_string(),
            body: vec!["body line".to_string()],
            buttons: vec![crate::core::engine::DialogButton {
                label: "OK".to_string(),
                hotkey: 'o',
                action: "ok".to_string(),
            }],
            selected: 0,
            tag: String::new(),
            input: None,
        });

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW605DIALOG"),
            "modal dialog should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// The *settings* sidebar panel must now paint through `render_content`
    /// too — it was one of #607's documented gaps, blocked on
    /// `quadraui::tui::draw_settings_chrome` being a free `&mut Buffer`
    /// rasteriser. #635 (Stage 6b item B) switches this to the real
    /// `Backend::draw_settings_chrome` trait method (`quadraui#531`), so the
    /// `" SETTINGS"` header row (a literal, hence deterministic) should
    /// reach the screen.
    #[test]
    fn render_content_paints_settings_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_SETTINGS));

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("SETTINGS"),
            "settings panel chrome should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #635 (Stage 6b item A): with the menu bar visible at construction,
    /// `shell_config(true)` must reserve `layout.title_bar_bounds` (via
    /// `AppShell::with_title_bar`, quadraui#532) and `render_content` must
    /// actually paint into it — the menu bar's first label ("File",
    /// `MENU_STRUCTURE`'s first entry) should reach the screen, the same
    /// way `render_content_paints_editor_text_via_shell_app` proves the
    /// editor content path above.
    #[test]
    fn render_content_paints_menu_bar_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.menu_bar_visible = true;

        let driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("File"),
            "menu bar should paint via TuiShellApp::render_content when menu_bar_visible; screen:\n{screen}"
        );
    }

    /// #695 regression: `handle()`'s MenuSystem intercept now reads
    /// `engine.menu_bar_rect` (populated once per frame by
    /// `render_content`, mirroring GTK's `menu_row_rect` cache,
    /// `gtk/mod.rs:8299`-`:8300`) instead of freshly computing
    /// `Rect::new(0.0, 0.0, viewport.width, 1.0)` on every dispatch. That
    /// cache is empty (zero height) on the very first dispatch that reveals
    /// the bar — the #318 Alt-letter shim sets `engine.menu_bar_visible =
    /// true` and this intercept can run in the *same* `handle()` call,
    /// before `render_content` ever repaints and refreshes the cache.
    /// quadraui's `MenuSystem::handle` lays out zero visible items against
    /// an empty rect and then indexes into that empty list — a panic, not a
    /// silent no-op. Caught during #695's own development (this exact
    /// scenario panicked before the `cached_bar_rect.height >= 1.0` fallback
    /// in `handle()` was added) and pinned here as permanent coverage: the
    /// same-frame Alt+F reveal-and-activate must keep working every time
    /// `menu_bar_rect` gains a new reader.
    #[test]
    fn alt_letter_reveal_and_activate_survives_empty_menu_bar_rect_cache_shell_app() {
        // Sidebar closed (bare `TuiShellApp::new`'s ambient default is
        // fine here — this test's assertion doesn't depend on sidebar
        // geometry) so `engine.menu_bar_rect` starts at its `Rect::default()`
        // seed (`Engine::new_from_state`) with no prior paint to populate it.
        let app = TuiShellApp::new(None);
        assert!(!app.engine.menu_bar_visible);
        let mut driver = driver_with_shell(app, config(), 80, 24);

        // First frame's paint already ran (`driver_with_shell` == `TuiDriver::
        // new`, which paints once after `setup`), with the bar hidden — so
        // `engine.menu_bar_rect` is still the zero-height default. Alt+F both
        // reveals `menu_bar_visible` *and* activates the File menu in this
        // one dispatch (mirrors `alt_letter_reveals_menu_bar_via_shell_app`
        // above), which is exactly the same-frame edge case described above.
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
            "Alt+F should reveal + activate the File menu without panicking"
        );

        let screen = driver.screen();
        assert!(
            screen.contains("New Tab"),
            "the File dropdown's first item should paint after the same-frame \
             reveal-and-activate; screen:\n{screen}"
        );
    }

    /// #695 review (iteration 1): black-box coverage for the widened
    /// intercept gate — `menu_bar_visible || menu_system.borrow().is_open()`
    /// instead of `menu_bar_visible` alone — added so hiding the bar while
    /// a dropdown is open doesn't strand the dropdown painted-but-
    /// unclickable (mirrors GTK's identical gate, `gtk/mod.rs:9671`).
    ///
    /// The obvious repro, clicking the status-line `StatusAction::
    /// ToggleMenuBar` segment while the dropdown is open, is a dead end:
    /// quadraui's `MenuSystem::handle` `MouseDown` arm treats *any* click
    /// outside the bar/dropdown as "click outside → close" and consumes the
    /// event first (before it can ever reach the status-line hit-test), so
    /// a mouse-based test can never observe the widened gate — every
    /// attempt just closes the dropdown instead of hiding the bar under it.
    ///
    /// `Engine::toggle_menu_bar`'s own doc comment ("Does NOT close any
    /// open dropdown — callers with backend access should call
    /// `menu_system.close()` when hiding") names the real caller
    /// (`StatusAction::ToggleMenuBar`, `execute.rs:3358`) but not a second,
    /// keyboard-only path to it: VSCode-mode F10 (`vscode.rs:1283`) also
    /// calls `toggle_menu_bar()` directly, and F10 is a `KeyPressed` variant
    /// `MenuSystem::handle` doesn't recognise (none of its `Key` patterns
    /// match `NamedKey::F(10)`, so it falls to the `_ => Ignored` catch-all)
    /// — meaning it passes through untouched by the dropdown's click-
    /// outside-closes behaviour and reaches `Engine::handle_key`'s vscode
    /// dispatch instead.
    ///
    /// Sequence: Alt+F opens the File dropdown (`menu_bar_visible=true`,
    /// `is_open()=true`). F10 flips `menu_bar_visible` to `false` without
    /// touching the dropdown (`is_open()` stays `true`) — exactly the state
    /// the widened gate exists for. Enter is dispatched last: before #695's
    /// widening, `menu_bar_visible` alone would read `false` here and Enter
    /// would fall through to ordinary vscode key handling instead of
    /// `MenuSystem`; with the widening it still reaches `MenuSystem` and
    /// activates the still-selected first item — File ▸ New Tab (`action:
    /// "tabnew"`) — opening a second tab.
    ///
    /// Asserts on rendered output throughout, per this repo's black-box
    /// rule: `"New Tab"` disappearing after F10 proves the dropdown stops
    /// *painting* once the bar is hidden (the paint block is still gated on
    /// `menu_bar_visible` alone — only the intercept gate widened), and a
    /// second `"[No Name]"` tab appearing **in the tab-bar row** after Enter
    /// is the only way to observe that the key still reached `MenuSystem`
    /// while invisible. Revert the gate to `menu_bar_visible` alone and this
    /// goes red: Enter falls through instead, and the second tab never
    /// appears.
    #[test]
    fn menu_intercept_routes_via_is_open_when_bar_hidden_with_dropdown_open_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        // VSCode mode is required to reach F10 -> toggle_menu_bar() via
        // Engine::handle_key (vscode.rs:1283) — the #318 Alt-letter reveal
        // shim and the MenuSystem intercept above it are both mode-
        // independent, so opening the dropdown below is unaffected.
        app.engine.settings.editor_mode = crate::core::settings::EditorMode::Vscode;
        let mut driver = driver_with_shell(app, config(), 80, 24);

        // Alt+F reveals + opens the File dropdown in one dispatch (mirrors
        // `alt_letter_reveals_menu_bar_via_shell_app`).
        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('f'),
            modifiers: quadraui::Modifiers {
                alt: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });
        let screen = driver.screen();
        assert!(
            screen.contains("New Tab"),
            "Alt+F should reveal the menu bar and open the File dropdown; \
             screen:\n{screen}"
        );

        // F10: MenuSystem doesn't recognise it, so it falls through to
        // Engine::handle_key -> handle_vscode_key -> toggle_menu_bar(),
        // which flips menu_bar_visible false WITHOUT closing the dropdown.
        driver.press_named(quadraui::NamedKey::F(10));
        let screen = driver.screen();
        assert!(
            !screen.contains("New Tab"),
            "hiding the bar must stop the dropdown from painting, even \
             though it stays logically open underneath; screen:\n{screen}"
        );

        // Count tabs on the **tab-bar row only**, not the whole screen —
        // the status line also renders the active buffer's name, so a
        // whole-screen `matches("[No Name]")` count is a mix of two
        // unrelated surfaces and its absolute value depends on ambient
        // settings (an operator `~/.config/vimcode/settings.json` changes
        // what the status line renders, so a developer machine and a clean
        // CI runner disagree — that divergence is exactly what turned this
        // assertion red in CI while it stayed green locally). With the bar
        // hidden the tab bar is back on screen row 0 with no dropdown
        // overlaying it, so row 0 is the tab bar verbatim (same
        // `screen.lines().next()` idiom as
        // `ctrl_o_twice_returns_to_the_first_tab_via_shell_app` above).
        let tab_row = screen.lines().next().unwrap_or_default().to_string();
        let before = tab_row.matches("[No Name]").count();
        assert_eq!(
            before, 1,
            "a fresh session starts with exactly one untitled tab; row:\n{tab_row}"
        );

        // Enter only reaches MenuSystem (and activates File > New Tab) if
        // the intercept gate still routes to it with menu_bar_visible now
        // false — i.e. only if the #695 `is_open()` widening is in effect.
        driver.press_named(quadraui::NamedKey::Enter);
        let screen = driver.screen();
        let tab_row = screen.lines().next().unwrap_or_default().to_string();
        let after = tab_row.matches("[No Name]").count();
        assert_eq!(
            after,
            before + 1,
            "Enter should have reached MenuSystem via the widened \
             `menu_bar_visible || is_open()` gate and activated File > New \
             Tab, opening a second [No Name] tab in the tab bar; screen:\n{screen}"
        );
    }

    /// The title-bar row must NOT be reserved (and nothing painted into
    /// row 0) when the menu is hidden — `shell_config(false)` is the
    /// default `TuiShellApp::new` state, so this is the same driver setup
    /// `shell_app_constructs_via_driver_with_shell_using_live_config`
    /// above uses, just asserting the negative.
    #[test]
    fn render_content_does_not_paint_menu_bar_when_hidden_via_shell_app() {
        let app = TuiShellApp::new(None);
        assert!(!app.engine.menu_bar_visible);

        let driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);
        let screen = driver.screen();
        assert!(
            !screen.contains("File"),
            "menu bar should not paint when menu_bar_visible is false; screen:\n{screen}"
        );
    }

    /// #635 (Stage 6b item A) / quadraui#547 regression: the previous two
    /// tests each construct in a single, fixed `menu_bar_visible` state —
    /// neither exercises the hidden→shown *transition* at runtime, which is
    /// exactly the shape the review that reopened this item flagged as
    /// uncovered. `TuiShellApp::new` + `shell_config(false)` is the default
    /// start state (`engine.menu_bar_visible` is `false` outside VSCode
    /// mode), so `AppShell` is constructed with `has_title_bar: false`. Prior
    /// to [JDonaghy/quadraui#547](https://github.com/JDonaghy/quadraui/issues/547),
    /// `build_shell_adapter` only called `AppShell::with_title_bar` (which
    /// sets the height) when `has_title_bar` was already `true`, so
    /// `shell_config`'s `title_bar_height_lh = 1.0` was silently discarded in
    /// favour of `AppShell`'s own struct default of 1.5 line-heights
    /// (`compose/app_shell.rs`) — `ctx.shell_mut().set_title_bar_visible(true)`
    /// (run from `handle()`'s first block on every dispatch once
    /// `engine.menu_bar_visible` flips) would then reserve 2 rows, not the 1
    /// row `render_impl.rs`'s live `draw_frame` path always uses. #547 fixed
    /// `build_shell_adapter` to honour `title_bar_height_lh` unconditionally,
    /// so this must now reserve exactly 1 row.
    ///
    /// Measures the *total* menu-bar footprint — the marker's row before any
    /// dispatch (menu hidden, nothing reserved anywhere) against its row once
    /// the bar is revealed and painted — rather than a delta between two
    /// already-shifted frames. That is the assertion that actually pins the
    /// thing down: `build_screen_for_shell_content` used to subtract a
    /// second, vimcode-local `menu_height` row on top of the one
    /// `AppShell::compute_layout` had already carved out of
    /// `main_content_bounds`, so the total was 2 rows for a 1-row bar even
    /// with #547 in place — and a relative "frame N+1 is one below frame N"
    /// check passed either way. It no longer does (see that function's doc
    /// comment); this test fails at `+2` against either defect.
    ///
    /// Drives the reveal through the real #318 Alt+letter shim
    /// (`alt_letter_reveals_menu_bar_via_shell_app` below) rather than poking
    /// `engine.menu_bar_visible` directly — `driver_with_shell`'s
    /// `ShellAdapter` wrapper is `pub(crate)`-fielded with no accessor back
    /// to the concrete `TuiShellApp` (see this `mod tests`'s own doc
    /// comment), so `handle()`'s dispatch is the only way in. One dispatch is
    /// enough: `handle()` syncs `set_title_bar_visible` on its way *out*, so
    /// the reveal this very keypress performs is already reflected in the
    /// frame the runner paints for it.
    #[test]
    fn shell_config_hidden_then_revealed_reserves_exactly_one_title_bar_row() {
        // Same ambient-sidebar precondition as
        // `alt_letter_reveals_menu_bar_via_shell_app` below, for the same
        // reason: the Alt+F reveal also opens the File dropdown, which
        // would otherwise paint over the marker this test measures. See
        // `app_with_sidebar_open`.
        let mut app = app_with_sidebar_open();
        app.engine.buffer_mut().insert(0, "ZQXW547MARKER");
        assert!(!app.engine.menu_bar_visible);

        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);

        let before = driver.screen();
        let before_row = before
            .lines()
            .position(|l| l.contains("ZQXW547MARKER"))
            .expect("marker should paint before the Alt-reveal keypress");
        assert!(
            !before.contains("File"),
            "no menu bar should be painted while it is hidden; screen:\n{before}"
        );

        // 'f' is `MENU_STRUCTURE`'s alt-letter for "File" — same shim
        // `alt_letter_reveals_menu_bar_via_shell_app` below exercises.
        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('f'),
            modifiers: quadraui::Modifiers {
                alt: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });
        let after = driver.screen();
        let after_row = after
            .lines()
            .position(|l| l.contains("ZQXW547MARKER"))
            .expect("marker should still paint after the Alt-reveal keypress");

        assert!(
            after.contains("File"),
            "the revealed menu bar must actually paint into the row it \
             reserved, on the same frame the reveal happened; screen:\n{after}"
        );
        assert_eq!(
            after_row,
            before_row + 1,
            "the menu bar's TOTAL footprint must be exactly ONE row. `+2` \
             means either `title_bar_height_lh` was discarded and \
             `AppShell`'s 1.5-line-height struct default (rounds to 2 rows) \
             was used (quadraui#547 regression), or \
             `build_screen_for_shell_content` grew back a vimcode-local \
             `menu_height` term on top of AppShell's own reservation \
             (#635 item A); before:\n{before}\nafter:\n{after}"
        );
    }

    /// The *source control* sidebar panel, likewise — its header/clear/hint
    /// rows were raw `set_cell` loops until #605 routed them through
    /// `fill_rect`/`fill_row`. `sc_header_text` always contains the literal
    /// "SOURCE CONTROL" regardless of repo state, so the assertion holds
    /// whether or not the test process happens to sit in a git checkout.
    #[test]
    fn render_content_paints_source_control_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_GIT));

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("SOURCE CONTROL"),
            "source control panel should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// The *extensions* sidebar panel, likewise — its two chrome rows (the
    /// header and the search box) were a local raw-`set_cell` `write_row`
    /// closure that #605 collapsed into `panels::fill_row`.
    #[test]
    fn render_content_paints_extensions_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXTENSIONS));

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("EXTENSIONS"),
            "extensions panel chrome should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #635 (Stage 6b item C): the plugin extension panel — the last raw-
    /// `Frame` sidebar holdout (`render_ext_panel`'s help popup + manual
    /// scrollbar) — must paint via `TuiShellApp::render_content` now that
    /// it takes `&mut dyn Backend` instead of a concrete `TuiBackend` and a
    /// `&mut Frame`. `sidebar.ext_panel_name` (not
    /// `engine.app_shell.active_panel_id()`) is what selects this path —
    /// see `render_sidebar_content`'s dispatch.
    #[test]
    fn render_content_paints_ext_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.ext_panels.insert(
            "zqxw_plugin".to_string(),
            crate::core::plugin::PanelRegistration {
                name: "zqxw_plugin".to_string(),
                title: "ZQXW Plugin".to_string(),
                icon: '?',
                fallback_icon: None,
                sections: vec![],
            },
        );
        app.engine.ext_panel_active = Some("zqxw_plugin".to_string());
        app.sidebar.ext_panel_name = Some("zqxw_plugin".to_string());

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW Plugin"),
            "ext panel chrome should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// #635 (Stage 6b item C): the AI sidebar panel — the most raw-`Buffer`
    /// of the lot (`render_ai_sidebar` used to take `buf: &mut Buffer`
    /// outright, no backend parameter at all) — must paint via
    /// `TuiShellApp::render_content` now that it takes `&mut dyn Backend`.
    #[test]
    fn render_content_paints_ai_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_AI));

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("AI ASSISTANT"),
            "AI panel chrome should paint via TuiShellApp::render_content; screen:\n{screen}"
        );
    }

    /// Toasts are the last thing painted, on top of every other surface.
    #[test]
    fn render_content_paints_toast_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine
            .push_toast("ZQXW605TOAST", "body", quadraui::ToastSeverity::Info);

        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        assert!(
            screen.contains("ZQXW605TOAST"),
            "toast stack should paint via TuiShellApp::render_content; screen:\n{screen}"
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

    // ── #694 investigation: hamburger-reveal freeze ─────────────────────
    //
    // #694 reports vimcode intermittently freezing (terminal stops
    // responding, has to be killed) when the menu bar is exposed via the
    // AppShell hamburger — sometimes instead producing the separately
    // tracked "invisible menu bar" symptom (#693, fixed: see
    // `TuiShellApp::on_shell_event_ctx` and
    // `hamburger_click_paints_menu_bar_immediately_via_shell_app` below).
    // Both shared one trigger: the hamburger click terminates in
    // `ShellAdapter`'s own `PanelChanged` arm (`quadraui/src/
    // shell_adapter.rs::handle`, the `AppShellEvent::PanelChanged { .. }
    // => { ...; return Reaction::Redraw; }` branch) and never reaches
    // `TuiShellApp::handle` at all for that event — the `handle` tail's
    // shell-state syncs (title-bar reservation, sidebar visibility) and
    // `ShellAdapter::apply_requested_panel`'s `take_requested_panel` poll
    // (also only reachable from inside `handle`/`tick`) were both skipped
    // for the click itself. #693 closed the title-bar half of that gap
    // by pushing the same sync from `on_shell_event_ctx` (quadraui#617's
    // `ShellContext`-aware notification) instead of waiting on the next
    // dispatch; `take_requested_panel` convergence still lags a dispatch,
    // which is why the tests below still prime/pump around it — that part
    // is #694's territory, not #693's.
    //
    // These tests drive that exact sequence through the real
    // `driver_with_shell` → `ShellAdapter` → `TuiShellApp` pipeline (not
    // `on_shell_event` called directly, which the pre-existing
    // `on_shell_event_hamburger_click_reveals_menu_bar` family above
    // already covers) across every variation #694 calls out as changing
    // which branch the click takes: sidebar open vs. closed beforehand,
    // hamburger clicked twice in a row, and a dynamic extension panel
    // active. None of them panics or hangs — which rules out the
    // `menu_system` `RefCell` double-borrow candidate from #694's "still
    // open" list for this exact reachable ordering: `handle`'s menu-bar
    // arm (`self.engine.menu_system.clone().borrow_mut().handle(...)`,
    // this file's `handle` method) only ever holds the `RefMut` for the
    // duration of that one statement — it is a temporary, not bound to a
    // name, so it drops before `dispatch_menu_action` runs, and
    // `driver_hamburger_click_then_key{,_with_sidebar_open}_does_not_panic`
    // below exercise precisely that borrow (the first real key after the
    // click, which is what actually opens the `MenuSystem` dispatch — the
    // hamburger click itself never reaches it, per the `ShellAdapter`
    // early-return above).
    //
    // Every test below that clicks the hamburger as its *first* interaction
    // primes the driver with a benign `WindowFocused` + `render()` first,
    // and then asserts on painted output that the menu bar actually opened.
    // Both halves are mandatory, for the reason spelled out in
    // `driver_hamburger_click_sidebar_closed_does_not_panic`'s doc comment:
    // an unprimed first hamburger click deterministically takes
    // `handle_activity_click`'s `SidebarHidden` branch, and without an
    // assertion nothing notices that the test is exercising the wrong one.
    //
    // The other "still open" candidate — `take_requested_panel` failing to
    // converge because the runner's `AppShell::show_panel` no-ops on an
    // unregistered id — is exercised by
    // `driver_hamburger_click_with_ext_panel_active_does_not_panic`, which
    // registers the extension panel so the id *does* resolve and the
    // reconciliation converges. Note what that means for the bug hunt: the
    // non-convergence case needs an `engine.ext_panel_active` naming a panel
    // that is **not** in the runner's panel list, which on this path
    // `sync_ext_activity_panels` keeps reconciled on every dispatch. No
    // driver-reachable sequence found in this session produces that
    // divergence, so the candidate is neither confirmed nor ruled out.
    //
    // This does **not** close #694: `TuiDriver` renders to `TestBackend`
    // and never parses real ANSI (see this file's module doc, "raw-mode,
    // SGR mouse... stay outside its reach"), so it cannot exercise
    // anything specific to the live terminal — raw-mode escape sequence
    // parsing, a real PTY, `supports_keyboard_enhancement()`'s blocking
    // round-trip, or genuine OS-level blocking/deadlock. A live-terminal
    // repro attempt (this session: `vcd` under `tmux`, real SGR mouse
    // byte sequences, ~150+ hamburger interactions across the same
    // variations plus rapid randomized fuzzing and deliberately malformed
    // mouse-down-without-mouse-up sequences) also did not reproduce a
    // hang, crash, or CPU spike — so the bug remains genuinely
    // intermittent and unreproduced; do not read the green tests below as
    // a fix or a closed investigation.

    /// Hamburger click with the sidebar closed beforehand (the ambient
    /// state on a bare `TuiShellApp::new` — see `app_with_sidebar_open`'s
    /// doc comment) must not panic through the real dispatch pipeline.
    ///
    /// The benign `WindowFocused` dispatch + render before the click is
    /// not decorative: `driver_with_shell`'s runner `AppShell` boots with
    /// `active_panel = Some(0)` (the hamburger) *and* `sidebar_visible =
    /// true` (`AppShell::new`), and `ShellAdapter::setup`/`render` never
    /// converge that onto the shadow `engine.app_shell`'s real state —
    /// only `handle`/`tick` do. Without this priming step, the click
    /// below would be the runner's first-ever interaction and would hit
    /// `handle_activity_click`'s "already active" branch, returning
    /// `SidebarHidden` instead of `PanelChanged` — silently testing the
    /// wrong code path (see `menu_reveal_then_search_icon_click_opens_
    /// search_not_explorer`'s identical priming, above).
    #[test]
    fn driver_hamburger_click_sidebar_closed_does_not_panic() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let _ = driver.click(1.0, 0.0);
        // The title-bar reservation syncs at the end of `TuiShellApp::
        // handle`, which `ShellAdapter`'s `PanelChanged` arm returns
        // before reaching (see `menu_reveal_then_search_icon_click_opens_
        // search_not_explorer` above) — pump one more benign event before
        // asserting on the rendered menu bar.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let screen = driver.screen();
        assert!(
            screen.contains("File"),
            "hamburger click should have reached PanelChanged and opened \
             the menu bar; screen:\n{screen}"
        );
    }

    /// Hamburger clicked twice in a row, primed first (see the previous
    /// test's doc comment for why priming is required — without it, click
    /// 1 would hit the runner `AppShell`'s hard-coded "hamburger already
    /// active" default and report `SidebarHidden`, not `PanelChanged`).
    /// With priming, click 1 opens the menu (`PanelChanged`, `active_panel`
    /// becomes the hamburger), so click 2 lands on the now-active hamburger
    /// item and reports `SidebarHidden` instead of a second `PanelChanged`
    /// (`handle_activity_click`'s same-active-panel branch) — a different
    /// code path from the first click, and #694 calls out double-clicking
    /// as one of the dimensions worth varying.
    #[test]
    fn driver_hamburger_click_twice_does_not_panic() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let _ = driver.click(1.0, 0.0);
        // Pump the title-bar reservation sync (see the previous test's
        // comment) before asserting the first click actually opened the
        // menu.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let screen = driver.screen();
        assert!(
            screen.contains("File"),
            "first hamburger click should open the menu bar; screen:\n{screen}"
        );
        let _ = driver.click(1.0, 0.0);
        let _ = driver.screen();
    }

    /// Hamburger click with the sidebar already open on a real panel
    /// (Explorer) beforehand — the shadow/runner sidebar-visibility
    /// divergence #694 discusses (the runner's `AppShell` reveals its
    /// sidebar for the hamburger "panel" while the shadow `engine.
    /// app_shell` never does) must not panic.
    #[test]
    fn driver_hamburger_click_with_sidebar_open_does_not_panic() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        let _ = driver.click(1.0, 1.0); // explorer icon -> opens sidebar
        let _ = driver.click(1.0, 0.0); // hamburger
        let _ = driver.screen();
    }

    /// The first real key event *after* a hamburger click is what
    /// actually reaches `TuiShellApp::handle`'s `MenuSystem` dispatch
    /// (`self.engine.menu_system.clone().borrow_mut().handle(...)`) —
    /// the click itself never does, per this block's doc comment above.
    /// Exercises the `RefCell` double-borrow candidate from #694's "still
    /// open" list directly: it does not panic.
    ///
    /// Primed first (see `driver_hamburger_click_sidebar_closed_does_not_
    /// panic`'s doc comment) so the click actually takes the `PanelChanged`
    /// branch and sets `menu_bar_visible = true` — without priming, the
    /// click would report `SidebarHidden` instead, `menu_bar_visible`
    /// would stay `false`, and the `press` below would never reach the
    /// `menu_system` borrow this test claims to exercise. The assertion
    /// after the click makes that reachability failure loud instead of
    /// silent.
    #[test]
    fn driver_hamburger_click_then_key_does_not_panic() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        // hamburger
        let _ = driver.click(1.0, 0.0);
        // Pump the title-bar reservation sync (see the priming test above)
        // before asserting the click actually opened the menu — otherwise
        // this would silently pass even if the click took the wrong
        // (`SidebarHidden`) branch, exactly the failure mode this test
        // exists to catch.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let screen = driver.screen();
        assert!(
            screen.contains("File"),
            "hamburger click must open the menu bar so the following key \
             press reaches the menu_system RefCell borrow; screen:\n{screen}"
        );
        let _ = driver.press(quadraui::Key::Char('j'));
        let _ = driver.screen();
    }

    /// Same as [`driver_hamburger_click_then_key_does_not_panic`], with
    /// the sidebar open on a real panel beforehand — combines two of
    /// #694's variation dimensions in one sequence.
    #[test]
    fn driver_hamburger_click_then_key_with_sidebar_open_does_not_panic() {
        let mut driver = driver_with_shell(
            TuiShellApp::new(None),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        let _ = driver.click(1.0, 1.0); // explorer icon -> opens sidebar
        let _ = driver.click(1.0, 0.0); // hamburger
        let _ = driver.press(quadraui::Key::Char('j'));
        let _ = driver.screen();
    }

    /// Hamburger click while a dynamic plugin-provided extension panel is
    /// active (`engine.ext_panel_active`) — the `take_requested_panel`
    /// non-convergence candidate from #694's "still open" list is
    /// specifically reachable via this arm (see that method's doc
    /// comment). Must not panic or infinite-loop; `driver.press` returning
    /// at all after the click proves the sequence converges rather than
    /// looping forever inside a single `handle`/`tick` call.
    ///
    /// Three setup details are load-bearing, and the first was missing from
    /// the earlier versions of this test (review, iterations 1 and 2):
    ///
    /// 1. The panel is **registered** in `engine.ext_panels`
    ///    (`app_with_ext_panel`) *and* the driver is built from
    ///    [`TuiShellApp::live_shell_config`], so `"ext:git-insights"` is a
    ///    real `PanelDefinition` on the **runner**'s `AppShell`. Setting
    ///    `engine.ext_panel_active` alone is not enough:
    ///    `Engine::ext_activity_panels` reads `ext_panels`, so with an empty
    ///    registry it returns nothing, [`Self::sync_ext_activity_panels`]
    ///    short-circuits on `unchanged`, the runner never learns the id, and
    ///    `AppShell::show_panel` — which silently no-ops on an unknown id —
    ///    leaves the runner parked on `AppShell::new`'s hard-coded
    ///    `active_panel = Some(0)` (the hamburger) with `sidebar_visible`
    ///    still `true`.
    /// 2. The shadow `engine.app_shell`'s sidebar is opened, because
    ///    [`Self::take_requested_panel`] returns `None` outright while the
    ///    shadow sidebar is hidden — the ext arm below it would never run.
    /// 3. The priming `WindowFocused` + `render()` then drives one
    ///    `take_requested_panel` → `show_panel` cycle which, now that the id
    ///    resolves, actually moves the runner off the hamburger. Without it
    ///    the click below lands on the still-active hamburger and reports
    ///    `SidebarHidden`, whose [`Self::on_shell_event`] arm *clears*
    ///    `ext_panel_active` / `sidebar.ext_panel_name` — so the ext-panel
    ///    sequence this test is named for would be wiped out by the very
    ///    click that is supposed to exercise it.
    ///
    /// Both assertions pin that down on rendered output rather than state:
    /// `"File"` means the click reached `PanelChanged { hamburger }` and set
    /// `menu_bar_visible` (so the `press` below reaches the `menu_system`
    /// `RefCell` borrow), and the panel title still painting in the sidebar
    /// body means the extension panel survived the click, i.e. the ext arm
    /// of `take_requested_panel` is what keeps running — and converging — on
    /// every subsequent dispatch. Drop any of the three setup steps and both
    /// go red instead of silently exercising the `SidebarHidden` branch.
    #[test]
    fn driver_hamburger_click_with_ext_panel_active_does_not_panic() {
        let mut app = app_with_ext_panel();
        app.engine.ext_panel_active = Some("git-insights".to_string());
        app.sidebar.ext_panel_name = Some("git-insights".to_string());
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        let cfg = TuiShellApp::live_shell_config(&app.engine);
        let mut driver = driver_with_shell(app, cfg, 80, 24);
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let _ = driver.click(1.0, 0.0); // hamburger
                                        // Pump the title-bar reservation sync (see
                                        // `driver_hamburger_click_sidebar_closed_does_not_panic`) before
                                        // asserting on the painted menu bar.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();
        let screen = driver.screen();
        assert!(
            screen.contains("File"),
            "hamburger click should have reached PanelChanged and opened \
             the menu bar; screen:\n{screen}"
        );
        assert!(
            screen.to_uppercase().contains("GIT INSIGHTS"),
            "the extension panel must survive the hamburger click — if the \
             click took the SidebarHidden branch instead, on_shell_event \
             would have cleared ext_panel_active and this test would no \
             longer exercise take_requested_panel's ext arm; screen:\n{screen}"
        );
        let _ = driver.press(quadraui::Key::Char('j'));
        let _ = driver.screen();
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

    /// #637: pins the mechanism behind the "extension panel intermittently
    /// stops responding" report, independent of any extensions actually
    /// installed on the machine running the test (unlike the CI-observed
    /// failure of `ext_sidebar_intercept_claims_focus_on_mouse_down`, which
    /// depended on a clean `$HOME` — see that test's neighbouring history).
    ///
    /// A plugin-provided extension panel (`sidebar.ext_panel_name`, e.g.
    /// "git-insights" — `render_ext_panel` / `mouse.rs`'s
    /// `ActivityBarTarget::ExtensionPanel` path) takes over the sidebar
    /// body without ever touching `app_shell`'s active-panel id
    /// (`mouse.rs` ~2298-2320). So if the user visited the Extensions
    /// *marketplace* panel (`PANEL_EXTENSIONS`) earlier this session and
    /// then opened a plugin panel from the activity bar:
    ///
    /// 1. `active_panel_is(PANEL_EXTENSIONS)` still reads `true` while the
    ///    plugin panel is what's actually on screen, and the marketplace's
    ///    `ext_sidebar_body_rect` (last populated whenever the marketplace
    ///    panel was painted, and never cleared since
    ///    `panels.rs::render_sidebar` returns early for
    ///    `ext_panel_name.is_some()` before ever touching it) still matches
    ///    the same on-screen sidebar area. Every click meant for the
    ///    plugin panel would then get silently claimed by the stale
    ///    marketplace SidebarSystem intercept instead of reaching the
    ///    plugin panel's own handling in `mouse::handle_mouse`.
    /// 2. Opening the plugin panel via its activity-bar icon
    ///    (`ActivityBarTarget::ExtensionPanel`) never cleared the
    ///    marketplace's `ext_sidebar_has_focus` flag either, so it stays
    ///    stuck `true` — a second, independent way for the marketplace
    ///    panel to keep "winning" focus decisions after it's no longer
    ///    even painted.
    ///
    /// Both are exactly the "stops responding" symptom, and both
    /// reproduce on any machine regardless of installed extensions.
    #[test]
    fn plugin_ext_panel_wins_focus_and_clicks_after_marketplace_visit() {
        let mut app = TuiShellApp::new(None);
        // Register a fake plugin-provided extension panel (mirrors what a
        // real plugin like "git-insights" registers).
        app.engine.ext_panels.insert(
            "git-insights".to_string(),
            crate::core::plugin::PanelRegistration {
                name: "git-insights".to_string(),
                title: "Git Insights".to_string(),
                icon: ' ',
                fallback_icon: None,
                sections: Vec::new(),
            },
        );

        // Visit the Extensions marketplace panel first, as a user would
        // before ever opening a plugin panel this session.
        ensure_panel_active(&mut app.engine, PANEL_EXTENSIONS);
        app.engine
            .ext_sidebar_body_rect
            .set(quadraui::Rect::new(3.0, 2.0, 30.0, 20.0));
        assert!(app.engine.ext_sidebar_has_focus);

        let mut backend = backend_at(80.0, 24.0);

        // Click the plugin panel's activity-bar icon — row 7, after
        // menu(0)/explorer(1)/search(2)/debug(3)/git(4)/extensions(5)/ai(6)
        // (`resolve_activity_bar_click`).
        app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(1.0, 7.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );
        assert_eq!(app.sidebar.ext_panel_name.as_deref(), Some("git-insights"));
        assert!(
            app.engine.ext_panel_has_focus && !app.engine.ext_sidebar_has_focus,
            "opening a plugin panel from the activity bar must clear focus \
             flags left over from a previously-visited panel — the \
             Extensions marketplace panel isn't painted anymore, so its \
             ext_sidebar_has_focus must not linger true"
        );

        // A click inside the sidebar body — where the (now invisible)
        // marketplace panel's stale `ext_sidebar_body_rect` still overlaps
        // the same on-screen area — must be routed to the visible plugin
        // panel, not silently reclaimed by the marketplace SidebarSystem
        // intercept.
        let reaction = app.handle_mouse_event(
            UiEvent::MouseDown {
                widget: None,
                button: quadraui::MouseButton::Left,
                position: quadraui::Point::new(10.0, 5.0),
                modifiers: quadraui::Modifiers::default(),
            },
            &mut backend,
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            app.engine.ext_panel_has_focus && !app.engine.ext_sidebar_has_focus,
            "a click inside the stale marketplace `ext_sidebar_body_rect` \
             must be routed to the visible plugin extension panel \
             (ext_panel_has_focus), not silently claimed by the \
             marketplace SidebarSystem intercept for a panel that isn't \
             even painted anymore (ext_sidebar_has_focus)"
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

        // #605: the picker modal now actually paints (it was an unpainted gap
        // when this test was written), so the typed text is legitimately on
        // screen — in the palette's *query* row. Assert on that positively…
        let open_screen = driver.screen();
        assert!(
            open_screen.contains("iZQXW_TYPED"),
            "keys typed while the command palette is open should feed the \
             picker query; screen:\n{open_screen}"
        );

        // …then dismiss the palette and re-check: with the overlay gone, any
        // keystroke that had leaked through to the editor buffer would now be
        // visible in the editor area. This is the original assertion, just
        // moved past the point where the modal can mask it.
        driver.press_named(quadraui::NamedKey::Escape);
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
    /// The screen-visible effect of `engine.menu_bar_visible` flipping is
    /// that `handle()`'s on-the-way-out
    /// `ShellContext::shell_mut().set_title_bar_visible(true)` sync makes
    /// `AppShell` reserve its (one-row — see
    /// `shell_config_hidden_then_revealed_reserves_exactly_one_title_bar_row`
    /// above) title bar, pushing `main_content_bounds` down — shifting the
    /// marker text down by exactly one line is this test's proof the #318
    /// shim actually ran. Uses the minimal `config()` rather than
    /// `shell_config(false)`; both configure a 1-row title bar, so the shift
    /// is the same either way.
    #[test]
    fn alt_letter_reveals_menu_bar_via_shell_app() {
        // Alt+F does not just reveal the bar, it *activates* the File menu,
        // and that dropdown paints as a 22-column box hard against the left
        // edge, straight over the top-left of the editor pane — exactly
        // where a marker inserted at buffer offset 0 lands. With the
        // sidebar open the editor pane starts to the right of the dropdown
        // and the marker survives; with it closed the dropdown erases the
        // marker and the row lookup below panics. Sidebar visibility on a
        // bare `TuiShellApp::new` is ambient (see `app_with_sidebar_open`),
        // so pin it — the row shift under test is independent of it.
        let mut app = app_with_sidebar_open();
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
            .unwrap_or_else(|| {
                panic!("marker should still paint after the Alt-reveal keypress; after:\n{after}")
            });
        assert_eq!(
            after_row,
            before_row + 1,
            "revealing the menu bar should reserve one more row above the \
             editor content, shifting the marker down by exactly one line; \
             before:\n{before}\nafter:\n{after}"
        );
    }

    /// #693 acceptance: the hamburger reveal must paint on the very frame
    /// the click lands — the same behaviour
    /// `alt_letter_reveals_menu_bar_via_shell_app` (above) already locks in
    /// for the Alt+<letter> reveal path. Before the fix, a hamburger click
    /// only flipped `engine.menu_bar_visible`: `ShellAdapter::handle`
    /// consumes `AppShellEvent::PanelChanged` in its own arm and returns
    /// without ever reaching `TuiShellApp::handle`, so the title-bar
    /// reservation sync at the end of that method — the thing that makes
    /// `layout.title_bar_bounds` `Some` so `render_content` actually paints
    /// the strip — never ran for this click. The bar stayed invisible
    /// (while still hit-testing as open, since the `MenuSystem` intercept
    /// reads `engine.menu_bar_visible` directly) until some unrelated later
    /// dispatch happened to reach `handle`; in a purely event-driven TUI
    /// with no further input, that frame might never come.
    ///
    /// This delivers the click through the real `driver_with_shell` →
    /// `ShellAdapter` → `TuiShellApp` pipeline (not `on_shell_event` called
    /// directly, which only proves the engine flag flips — see
    /// `on_shell_event_hamburger_click_reveals_menu_bar`, above) and
    /// asserts on `driver.screen()` immediately after the click, with no
    /// intervening "pump" dispatch of the kind the `#694 investigation`
    /// tests below use to work around this exact gap. Must fail on
    /// `develop` today for the reason above — reverting
    /// `TuiShellApp::on_shell_event_ctx`'s `set_title_bar_visible` push
    /// turns this red while leaving `on_shell_event_hamburger_click_
    /// reveals_menu_bar` green, which is the point: that test alone can't
    /// catch this bug.
    #[test]
    fn hamburger_click_paints_menu_bar_immediately_via_shell_app() {
        let mut app = app_with_sidebar_open();
        app.engine.buffer_mut().insert(0, "ZQXW_HAMBURGER_MARKER");
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);

        // Prime the runner off the hamburger's default-active slot
        // (`AppShell::new` activates panel index 0, the hamburger) so the
        // click under test actually takes the `PanelChanged` branch
        // instead of `handle_activity_click`'s "already active"
        // `SidebarHidden` branch — see
        // `driver_hamburger_click_sidebar_closed_does_not_panic`'s doc
        // comment for why an unprimed first hamburger click hits the wrong
        // branch. This priming exercises no part of the bug under test —
        // it runs before the click, not after it.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();

        let before = driver.screen();
        assert!(
            !before.contains("File"),
            "menu bar must start hidden; before:\n{before}"
        );
        let before_row = before
            .lines()
            .position(|l| l.contains("ZQXW_HAMBURGER_MARKER"))
            .expect("marker should paint before the hamburger click");

        let reaction = driver.click(1.0, 0.0); // hamburger, top of activity bar
        assert_eq!(
            reaction,
            Reaction::Redraw,
            "hamburger click should reveal the menu bar and redraw"
        );

        // No priming, no extra dispatch — the frame the click itself
        // produced is what's under test.
        let after = driver.screen();
        assert!(
            after.contains("File") && after.contains("Edit"),
            "hamburger click should paint the File/Edit menu strip on row \
             0 on the same frame, with no further event needed to reveal \
             it; after:\n{after}"
        );
        let after_row = after
            .lines()
            .position(|l| l.contains("ZQXW_HAMBURGER_MARKER"))
            .unwrap_or_else(|| {
                panic!("marker should still paint after the hamburger click; after:\n{after}")
            });
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
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

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
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
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
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        let reaction = handle_key_pressed(
            quadraui::Key::Char('x'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
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
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(folder_picker.is_none(), "Esc should dismiss the picker");
    }

    /// `handle_key_pressed`'s context-menu branch must dispatch the
    /// confirmed item's action to [`handle_explorer_context_action`]
    /// (mirrors `mod.rs:2703`-`:2706`) — unlike `Engine::handle_key`'s own
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
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        let reaction = handle_key_pressed(
            quadraui::Key::Named(quadraui::NamedKey::Enter),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
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

    /// #635 (Stage 6b item D): the activity-bar-focused keyboard tier
    /// (mirrors `mod.rs:1805`-`:1854`) — `j`/`Down` must move the keyboard
    /// cursor, and `l`/Enter on the Explorer slot (toolbar index 1) must
    /// activate it, clearing `activity_bar_focused` and requesting sidebar
    /// focus (`ActivityBarActivation::PanelFocused`).
    #[test]
    fn handle_key_pressed_activity_bar_focused_moves_and_activates() {
        let mut engine = Engine::new();
        engine.activity_bar_focus_in_at(0); // hamburger slot
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        // 'j' moves the cursor from the hamburger (0) to Explorer (1).
        let reaction = handle_key_pressed(
            quadraui::Key::Char('j'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(engine.activity_bar_focused, "still toolbar-focused after j");
        assert_eq!(engine.activity_bar_selected, 1);

        // 'l' activates the selected (Explorer) slot.
        let reaction = handle_key_pressed(
            quadraui::Key::Char('l'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );
        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            !engine.activity_bar_focused,
            "activating a panel should clear toolbar focus"
        );
        assert!(
            sidebar.has_focus,
            "activating a real panel should request sidebar focus"
        );
    }

    /// #635 (Stage 6b item D): the `cmd_sel` keyboard tier (mirrors
    /// `mod.rs:2651`-`:2701`) — Ctrl+C copies the selected message-line
    /// text and clears the selection. `Engine::new()` (no
    /// `setup_tui_clipboard`) has no `clipboard_write` hook, so
    /// `tui_copy_to_clipboard` takes its "clipboard unavailable" branch —
    /// still enough to prove the selected substring reached the copy
    /// helper, deterministically, with no real clipboard I/O.
    #[test]
    fn handle_key_pressed_cmd_sel_ctrl_c_copies_and_clears() {
        let mut engine = Engine::new();
        engine.message = "hello world".to_string();
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();
        scratch.cmd_sel.set(Some((0usize, 4usize))); // "hello"

        let reaction = handle_key_pressed(
            quadraui::Key::Char('c'),
            quadraui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert!(
            scratch.cmd_sel.get().is_none(),
            "Ctrl+C should clear the selection"
        );
        assert!(
            engine.message.contains("hello"),
            "Ctrl+C should copy the selected substring; message: {}",
            engine.message
        );
    }

    /// A non-Ctrl+C key while a selection is active must clear it without
    /// copying anything (mirrors the "any other key clears" half of
    /// `mod.rs:2694`-`:2700`) — falls through to the general
    /// `Engine::handle_key` fallback afterward, same as before this stage.
    #[test]
    fn handle_key_pressed_cmd_sel_other_key_clears_without_copying() {
        let mut engine = Engine::new();
        engine.message = "hello world".to_string();
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();
        scratch.cmd_sel.set(Some((0usize, 4usize)));

        let _ = handle_key_pressed(
            quadraui::Key::Char('x'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );

        assert!(
            scratch.cmd_sel.get().is_none(),
            "any other key should clear the selection"
        );
        assert!(
            !engine.message.contains("Copied") && !engine.message.contains("clipboard"),
            "no copy should happen on a non-Ctrl+C key; message: {}",
            engine.message
        );
    }

    // ── #634 (Stage 6): the tiers the cutover would have regressed ──────

    /// The single highest-value regression guard for the cutover: with the
    /// sidebar focused, a bare letter must be consumed by the sidebar tier,
    /// not fall through to `Engine::handle_key` as a Vim command. Before
    /// #634 ported `handle_sidebar_focused_key`, `x` would have deleted a
    /// character out of the *editor buffer* while the user was navigating
    /// the file tree.
    #[test]
    fn sidebar_focused_key_does_not_reach_the_editor_buffer() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "abcdef");
        let mut sidebar = TuiSidebar::new();
        sidebar.has_focus = true;
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        let reaction = handle_key_pressed(
            quadraui::Key::Char('x'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );

        assert_eq!(reaction, Reaction::Redraw);
        assert_eq!(
            engine.buffer().to_string(),
            "abcdef",
            "a sidebar-focused keypress must not reach Engine::handle_key"
        );
    }

    /// Ctrl-W then `l` moves focus from the sidebar back to the editor
    /// (mirrors `mod.rs:1938`-`:1972`). Two keypresses, because the chord is
    /// stateful — the first only arms `pending_ctrl_w`.
    #[test]
    fn sidebar_focused_ctrl_w_l_returns_focus_to_the_editor() {
        let mut engine = Engine::new();
        let mut sidebar = TuiSidebar::new();
        sidebar.has_focus = true;
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        let ctrl = quadraui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        handle_key_pressed(
            quadraui::Key::Char('w'),
            ctrl,
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );
        assert!(sidebar.pending_ctrl_w, "Ctrl-W must arm the chord");
        assert!(sidebar.has_focus, "Ctrl-W alone must not move focus");

        handle_key_pressed(
            quadraui::Key::Char('l'),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );
        assert!(!sidebar.pending_ctrl_w, "the chord must be consumed");
        assert!(
            !sidebar.has_focus,
            "Ctrl-W l must return focus to the editor"
        );
    }

    /// Alt+Right / Alt+Left resize the sidebar and clamp at 15..=150
    /// (mirrors `mod.rs:2531`-`:2542`). `sidebar_width` lives on
    /// [`KeyDispatchState`] precisely so this tier can mutate it.
    #[test]
    fn alt_arrows_resize_the_sidebar_within_the_legacy_clamps() {
        let mut engine = Engine::new();
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();
        let alt = quadraui::Modifiers {
            alt: true,
            ..Default::default()
        };

        let press = |key,
                     engine: &mut Engine,
                     sidebar: &mut TuiSidebar,
                     folder_picker: &mut Option<FolderPickerState>,
                     backend: &mut dyn quadraui::Backend,
                     scratch: &mut KeyScratch| {
            handle_key_pressed(
                key,
                alt,
                false,
                engine,
                sidebar,
                folder_picker,
                false,
                80,
                24,
                backend,
                &mut scratch.state(),
            );
        };

        press(
            quadraui::Key::Named(quadraui::NamedKey::Right),
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            &mut backend,
            &mut scratch,
        );
        assert_eq!(scratch.sidebar_width, SIDEBAR_WIDTH + 1);

        scratch.sidebar_width = 15;
        press(
            quadraui::Key::Named(quadraui::NamedKey::Left),
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            &mut backend,
            &mut scratch,
        );
        assert_eq!(scratch.sidebar_width, 15, "Alt+Left must clamp at 15");

        scratch.sidebar_width = 150;
        press(
            quadraui::Key::Named(quadraui::NamedKey::Right),
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            &mut backend,
            &mut scratch,
        );
        assert_eq!(scratch.sidebar_width, 150, "Alt+Right must clamp at 150");
    }

    /// The post-key epilogue (`mod.rs:2863`-`:2915`) — verified through its
    /// most observable member, the quickfix scroll-into-view clamp. Before
    /// #634 nothing wrote `quickfix_scroll_top` after construction, so a
    /// selection past the fifth visible row simply scrolled off screen.
    #[test]
    fn post_key_epilogue_scrolls_the_quickfix_selection_into_view() {
        let mut engine = Engine::new();
        engine.quickfix_open = true;
        engine.quickfix_selected = 9;
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = None;
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        handle_key_pressed(
            // Any key that reaches the general fallback will do; Escape is
            // inert in Normal mode.
            quadraui::Key::Named(quadraui::NamedKey::Escape),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );

        // 6 panel rows − 1 header = 5 visible; selection 9 ⇒ top 5.
        assert_eq!(scratch.quickfix_scroll_top, 5);

        engine.quickfix_open = false;
        handle_key_pressed(
            quadraui::Key::Named(quadraui::NamedKey::Escape),
            quadraui::Modifiers::default(),
            false,
            &mut engine,
            &mut sidebar,
            &mut folder_picker,
            false,
            80,
            24,
            &mut backend,
            &mut scratch.state(),
        );
        assert_eq!(
            scratch.quickfix_scroll_top, 0,
            "closing the quickfix panel resets its scroll"
        );
    }

    /// Bracketed paste. The runner turns crossterm's `Event::Paste` into
    /// `UiEvent::ClipboardPaste`; `TuiShellApp::handle`'s catch-all `_` arm
    /// used to swallow it, so pasting into the TUI did nothing at all.
    #[test]
    fn bracketed_paste_reaches_the_buffer_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.mode = crate::core::Mode::Insert;
        let mut driver = driver_with_shell(app, config(), 80, 24);
        driver.dispatch(UiEvent::ClipboardPaste("ZQXW_PASTE_MARKER".to_string()));
        driver.render();
        assert!(
            driver.screen_contains("ZQXW_PASTE_MARKER"),
            "UiEvent::ClipboardPaste must route through Engine::route_paste; screen:\n{}",
            driver.screen()
        );
    }

    /// #673 black-box regression: closing a tab must activate the MRU
    /// successor, not whatever tab is positionally adjacent to the closed
    /// slot. Read through the *rendered* tab bar (not just engine state) so
    /// the test proves what the user actually sees.
    ///
    /// The naive repro ([A, B, C], close C then B => A) passes by adjacency
    /// accident even with the bug fully present (see the issue), so this
    /// uses a non-adjacent prior tab: tabs [W, X, A, Y, Z], active pinned to
    /// A, then open B and C (both far from A) and close them in turn. The
    /// MRU-correct successor is A; pure positional adjacency lands on Z.
    ///
    /// The active tab is distinguished in the tab bar only by background
    /// colour (`theme.tab_active_bg` vs `theme.tab_bar_bg` — see
    /// `quadraui::tui::tab_bar::draw_tab_bar`'s doc comment), not by any
    /// text difference, so this reads `style_at` on the resolved tab-label
    /// cell rather than scanning `screen()` for a marker.
    #[test]
    fn close_tab_after_close_reactivates_mru_tab_not_positional_neighbour() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_close_tab_673_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let make = |name: &str| -> std::path::PathBuf {
            let p = dir.join(name);
            std::fs::write(&p, "content").unwrap();
            p
        };
        let tab_w = make("tab_w.txt");
        let tab_x = make("tab_x.txt");
        let tab_a = make("tab_a.txt");
        let tab_y = make("tab_y.txt");
        let tab_z = make("tab_z.txt");
        let tab_b = make("tab_b.txt");
        let tab_c = make("tab_c.txt");

        let mut app = TuiShellApp::new(None);
        // Tabs open in order after the pre-existing [No Name] tab:
        // [No Name], W, X, A, Y, Z.
        app.engine.new_tab(Some(&tab_w));
        app.engine.new_tab(Some(&tab_x));
        app.engine.new_tab(Some(&tab_a));
        app.engine.new_tab(Some(&tab_y));
        app.engine.new_tab(Some(&tab_z));

        // Pin the active tab to A — not adjacent to where B/C will land.
        let a_idx = app
            .engine
            .active_group()
            .tabs
            .iter()
            .position(|t| {
                app.engine
                    .windows
                    .get(&t.active_window)
                    .and_then(|w| app.engine.buffer_manager.get(w.buffer_id))
                    .and_then(|s| s.file_path.as_ref())
                    == Some(&tab_a)
            })
            .expect("tab_a.txt should be open");
        app.engine.goto_tab(a_idx);

        app.engine.new_tab(Some(&tab_b));
        app.engine.new_tab(Some(&tab_c));

        // Close C then B — the buggy adjacency-only logic lands on Z here;
        // the MRU-aware fix lands back on A.
        assert!(app.engine.close_tab());
        assert!(app.engine.close_tab());

        let driver = driver_with_shell(app, config(), 200, 24);
        let screen = driver.screen();

        // Only B and C were closed — X, A, Y, Z all remain in the tab bar
        // throughout; this test is about which one is *active*, not which
        // ones survive.
        assert!(
            driver.find_bounds("tab_b.txt").is_none(),
            "tab_b.txt should have been closed; screen:\n{screen}"
        );
        assert!(
            driver.find_bounds("tab_c.txt").is_none(),
            "tab_c.txt should have been closed; screen:\n{screen}"
        );

        let a_bounds = driver
            .find_bounds("tab_a.txt")
            .unwrap_or_else(|| panic!("tab_a.txt should still be open; screen:\n{screen}"));
        let x_bounds = driver
            .find_bounds("tab_x.txt")
            .unwrap_or_else(|| panic!("tab_x.txt should still be open; screen:\n{screen}"));
        let z_bounds = driver
            .find_bounds("tab_z.txt")
            .unwrap_or_else(|| panic!("tab_z.txt should still be open; screen:\n{screen}"));

        let a_style = driver
            .style_at(a_bounds.x as u16, a_bounds.y as u16)
            .expect("a_bounds should be on-screen");
        // tab_x.txt is never touched after its initial open, so it is
        // guaranteed inactive throughout — the reference "inactive" style.
        // Comparing A only against Z (the buggy answer) would pass either
        // way the bug resolves (whichever one is active simply differs
        // from the other) — see #553 on tests that pass regardless of the
        // bug. Anchoring on a third, definitely-inactive tab breaks that
        // symmetry: A must differ from it (A is active) and Z must match
        // it (Z is *not* active, unlike what the adjacency bug would do).
        let x_style = driver
            .style_at(x_bounds.x as u16, x_bounds.y as u16)
            .expect("x_bounds should be on-screen");
        let z_style = driver
            .style_at(z_bounds.x as u16, z_bounds.y as u16)
            .expect("z_bounds should be on-screen");
        assert_ne!(
            a_style.bg, x_style.bg,
            "tab_a.txt must be painted with the active-tab background — \
             the MRU successor after closing C then B is A; screen:\n{screen}"
        );
        assert_eq!(
            z_style.bg, x_style.bg,
            "tab_z.txt must NOT be painted as active — that's the wrong \
             answer pure positional adjacency would pick; screen:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
