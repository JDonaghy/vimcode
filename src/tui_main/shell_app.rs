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
//!    **Update (#765, #735 slice 4):** the four `panels::` helpers named
//!    above (`render_quickfix_panel`, `render_bottom_panel_tabs`,
//!    `render_terminal_toolbar`, `render_terminal_panel_content`) are gone —
//!    deleted along with GTK's bespoke bottom-band arms and replaced by the
//!    single shared walk over `render::compose_bottom_band` /
//!    `render::BOTTOM_Z_ORDER`, calling `render::paint_quickfix_rung` /
//!    `render::paint_bottom_panel_rung` / `render::paint_separated_status_rung`
//!    from both backends. See the "Bottom band" section of
//!    [`Self::render_content`] and the module doc at the top of `render.rs`.
//!
//!    The menu bar row used to be reserved in the layout math but not
//!    painted (out of scope for #601; folded into key dispatch, #603, then
//!    painting, #635 item A — see the Stage 6b section below). Cursor
//!    placement used to be a raw-buffer holdout in this list (it needs
//!    `Frame::set_cursor_position`, and `render_content` has no `Frame`)
//!    but #604 closed it a different way — see gap 3 below, now resolved.
//! 2. **Mouse handling (#602, largely resolved).** `mouse::handle_mouse`
//!    (~4,100 lines) takes `&mut quadraui::DragState` + `&mut
//!    quadraui::ModalStack` directly — once concrete-only state the
//!    `Backend` trait deliberately didn't expose, so it couldn't be called
//!    from `handle(&mut self, event, backend: &mut dyn Backend, ...)` as-is.
//!    `Backend::{drag_state_handle, modal_stack_handle}` (quadraui#704,
//!    superseding the `drag_and_modal_mut` accessor of quadraui#467) hand
//!    out `Rc<RefCell<_>>` handles, unblocking the fix:
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
//! ([`handle_focus_owner_key`]), terminal/PTY key routing, the
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
//! as [`handle_focus_owner_key`], since the cutover would otherwise
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
pub struct TuiShellApp {
    pub engine: Engine,
    sidebar: TuiSidebar,
    sidebar_width: u16,
    /// #815: `quadraui::FolderPickerController`, adopted from the old
    /// TUI-local `FolderPickerState`. GTK's `App` carries the identical
    /// field type — see `app.rs`.
    folder_picker: Option<quadraui::FolderPickerController>,
    quickfix_scroll_top: usize,
    dragging_sidebar: bool,
    dragging_terminal_resize: bool,
    dragging_terminal_split: bool,
    /// The divider (group boundary or `:split` boundary) currently grabbed.
    /// #753 collapsed the two mutually-exclusive `dragging_group_divider` /
    /// `dragging_window_divider` fields into the shared
    /// [`render::DividerGrab`], so "both grabbed at once" is unrepresentable.
    divider_grab: Option<render::DividerGrab>,
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
    /// Rect the last frame painted the debug toolbar into, or the zero rect
    /// when that frame composed no `BottomOp::DebugToolbar` — the cache
    /// `route_chrome_click` hit-tests button clicks against. An `Rc` so the
    /// `TuiDriver` harness can observe it across frames: quadraui's
    /// `driver_with_shell` wraps the app in a shell adapter, so `driver.app()`
    /// cannot reach `TuiShellApp`'s own fields (#765).
    debug_toolbar_rect: std::rc::Rc<Cell<quadraui::Rect>>,
    explorer_sb_dragging: bool,
    explorer_drag_src: Option<usize>,
    explorer_drag_active: Option<(usize, Option<usize>)>,
    /// Tab drag-and-drop arm → threshold → track → commit machine. #753
    /// replaced the five parallel fields (`tab_drag_start`, `tab_dragging`,
    /// `tui_drag_source`, `tui_drag_cursor`, `tui_tab_drop_zone`) with the
    /// shared [`render::TabDragState`], which GTK holds too.
    tab_drag: render::TabDragState,
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
    /// Bounds of the tab-switcher popup as `render_content` last painted
    /// it — the GTK backend's `tab_switcher_popup_rect` counterpart. Fed
    /// to `render::route_modal_overlay_click`; never recomputed at click
    /// time (#582 / #646).
    tab_switcher_popup_rect: RefCell<Option<quadraui::Rect>>,
    /// The frame rungs this frame actually composed, in composition order
    /// (#735, folded into one sequence by #766).
    ///
    /// Written by the single [`render::compose_frame`] walk in
    /// `render_content` — every arm that draws pushes its own
    /// [`render::FrameOp`], and arms whose surface turned out to be absent do
    /// not. `App` (`gtk/mod.rs`) keeps the identical field for the identical
    /// reason: it is the observable that makes "both backends compose the
    /// frame in the same order" a thing a test can assert, rather than
    /// something two hand-kept ladders promise each other in comments (they
    /// promised, and they had drifted — twice; see `FRAME_Z_ORDER`).
    ///
    /// Before #766 this was *two* fields — `painted_overlay_band` and
    /// `composed_chrome_band` — so "the frame's sequence" was still two
    /// observables a backend could get individually right and jointly wrong.
    ///
    /// `Rc` because `driver_with_shell` returns `TuiDriver<impl AppLogic>` —
    /// an opaque `ShellAdapter` with no accessor back to the concrete
    /// `TuiShellApp` — so a test has to clone the handle *before* handing the
    /// app over, exactly as `gtk/testing.rs`'s `Harness` does for every one of
    /// its painted-geometry observables.
    composed_frame: std::rc::Rc<RefCell<Vec<render::FrameOp>>>,
    /// The editor-band twin of [`Self::composed_frame`] (#764): written
    /// by the [`render::compose_editor_band`] walk, one [`render::EditorOp`]
    /// pushed by every arm that actually composed its rung. Same `Rc`
    /// rationale, same role — it is what makes "both backends compose the
    /// editor column in the same order" assertable rather than promised in
    /// comments, which is how GTK came to omit the group dividers entirely.
    composed_editor_band: std::rc::Rc<RefCell<Vec<render::EditorOp>>>,
    /// The bottom-band twin of [`Self::composed_editor_band`] (#765): written
    /// by the [`render::compose_bottom_band`] walk, one [`render::BottomOp`]
    /// pushed by every arm that actually composed its rung. Same `Rc`
    /// rationale, same role — and this backend had its own share of what it
    /// catches: the separated status line was composed two rungs early, and
    /// `debug_toolbar_rect` was never cleared when the toolbar hid.
    composed_bottom_band: std::rc::Rc<RefCell<Vec<render::BottomOp>>>,
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

/// A ratatui cell rect as the quadraui `Rect` every shared painter takes.
///
/// TUI composes in whole cells and quadraui's primitives are `f32`-based, so
/// this conversion sat inline at every bottom-band call site — four times in
/// `render_content` alone, and again in `draw_frame`.
pub(super) fn to_q_rect(r: Rect) -> quadraui::Rect {
    quadraui::Rect::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
}

/// The inverse of [`to_q_rect`]: a shell-supplied `quadraui::Rect` snapped back
/// to the whole-cell grid TUI's own painters take.
///
/// Rounds rather than truncates, matching every hand-written
/// `x.round() as u16` this replaces.
pub(super) fn to_cell_rect(r: quadraui::Rect) -> Rect {
    Rect {
        x: r.x.round() as u16,
        y: r.y.round() as u16,
        width: r.width.round() as u16,
        height: r.height.round() as u16,
    }
}

impl TuiShellApp {
    /// The window title stem shown in the Command Center — the workspace
    /// directory's own name, or `VimCode` when `cwd` has no file name (`/`).
    fn window_title_stem(&self) -> String {
        self.engine
            .cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "VimCode".to_string())
    }

    /// Compose the **bottom band** (#765, #735 slice 4): the chrome vimcode
    /// stacks below the editor column — quickfix, the terminal/debug bottom
    /// panel, the debug toolbar, the separated status line and the sidebar
    /// hover popup.
    ///
    /// Walks `render::compose_bottom_band`; geometry stays here, in cells.
    /// Extracted out of `render_content` (#766) so that function reads as the
    /// frame's *order* and nothing else, mirroring `gtk/mod.rs`'s
    /// `App::compose_bottom_band_rungs`.
    #[allow(clippy::too_many_arguments)]
    fn compose_bottom_band_rungs(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        layout: &quadraui::AppShellLayout,
        theme: &Theme,
        area: Rect,
        win_area: Rect,
        chrome: &BottomChromeRects,
    ) {
        // ══ Bottom band (#765, #735 slice 4) ═════════════════════════════
        //
        // Composed from `render::compose_bottom_band` — the single ordered
        // artefact both backends walk for the chrome stacked below the editor
        // column, exactly as `EDITOR_Z_ORDER` (above) is for the column itself
        // and `CHROME_Z_ORDER` (below) for the surrounding chrome. Geometry
        // stays here, in cells; only the *order* and the *gates* moved.
        // `BOTTOM_Z_ORDER`'s doc comment records the five divergences this
        // convergence closed — including the two this backend owned: the
        // separated status line composed two rungs early (contradicting this
        // file's *own* `bottom_chrome_rects_for_shell_content` constraint
        // order), and `debug_toolbar_rect` never cleared when the toolbar hid.
        //
        // Caches whose owning rung may be gated off are cleared *here*, before
        // the walk, never from an `else` arm inside it: `compose_bottom_band`
        // returns only the live rungs, so an absent rung has no arm to run.
        self.engine.bottom_panel_geometry.replace(None);
        self.debug_toolbar_rect
            .set(quadraui::Rect::new(0.0, 0.0, 0.0, 0.0));
        self.hover_popup_rect.set(None);
        self.hover_link_rects.borrow_mut().clear();

        let mut composed_bottom: Vec<render::BottomOp> = Vec::new();
        for op in render::compose_bottom_band(
            &self.engine,
            screen,
            layout.sidebar_content_bounds.is_some(),
        ) {
            match op {
                render::BottomOp::Quickfix => {
                    let Some(ref qf) = screen.quickfix else {
                        continue;
                    };
                    backend.set_theme(super::quadraui_tui::q_theme(theme));
                    render::paint_quickfix_rung(
                        backend,
                        qf,
                        to_q_rect(chrome.quickfix),
                        // Unlike GTK's stateless recompute, this offset is
                        // carried across frames so `:cnext` can advance it.
                        self.quickfix_scroll_top,
                    );
                    composed_bottom.push(render::BottomOp::Quickfix);
                }
                render::BottomOp::BottomPanel => {
                    backend.set_theme(super::quadraui_tui::q_theme(theme));
                    render::paint_bottom_panel_rung(
                        backend,
                        &self.engine,
                        screen,
                        theme,
                        to_q_rect(chrome.bottom_panel),
                        render::BottomPanelUnits::CELL,
                    );
                    composed_bottom.push(render::BottomOp::BottomPanel);
                }
                render::BottomOp::DebugToolbar => {
                    let q_rect = to_q_rect(chrome.debug_toolbar);
                    self.debug_toolbar_rect.set(q_rect);
                    render::draw_debug_toolbar(backend, &self.engine, q_rect);
                    composed_bottom.push(render::BottomOp::DebugToolbar);
                }
                // Shown below the terminal panel when `window_status_line` is
                // on but `status_line_above_terminal` is off. Composed *after*
                // the bottom panel and the debug toolbar now, which is where
                // `bottom_chrome_rects_for_shell_content` has always reserved
                // its row — this backend used to paint it two rungs earlier.
                render::BottomOp::SeparatedStatus => {
                    let Some(ref status) = screen.separated_status_line else {
                        continue;
                    };
                    backend.set_theme(super::quadraui_tui::q_theme(theme));
                    let _ = render::paint_separated_status_rung(
                        backend,
                        status,
                        to_q_rect(chrome.separated_status),
                    );
                    composed_bottom.push(render::BottomOp::SeparatedStatus);
                }
                // Anchored just right of the sidebar's own right edge, which in
                // the shell layout is exactly `main_content_bounds.x`
                // (`AppShell` puts the resize divider between them and `area.x`
                // is the first column past it) — the same column `draw_frame`
                // computes as `sep_x + 1`.
                //
                // Composed before the chrome band rather than after it, which
                // is where this backend used to paint it. Safe, and now
                // matching GTK, because the popup's width is capped to what
                // fits *right* of that anchor, so it can never be shifted back
                // over the sidebar the chrome band paints.
                //
                // The rasteriser stays per-backend here — the `EditorOp
                // ::Windows` precedent. TUI's `hover_popup_rect` /
                // `hover_link_rects` caches are `u16` cell tuples that the
                // mouse router reads directly, where GTK's are `f64` pixels
                // carrying an extra `is_native` flag; converging the two cache
                // *shapes* is a mouse-routing change, not a composition one,
                // and belongs with whichever slice owns that. What #765 fixes
                // is that the rung is now composed — and cleared — at the top
                // level on both backends.
                render::BottomOp::PanelHover => {
                    let Some(sb) = layout.sidebar_content_bounds else {
                        continue;
                    };
                    let (new_rects, popup_rect) = render_panel_hover_popup(
                        backend,
                        screen,
                        theme,
                        area.x,
                        sb.y.round() as u16,
                        sb.height.round() as u16,
                        win_area,
                    );
                    *self.hover_link_rects.borrow_mut() = new_rects;
                    self.hover_popup_rect.set(popup_rect);
                    composed_bottom.push(render::BottomOp::PanelHover);
                }
            }
        }
        *self.composed_bottom_band.borrow_mut() = composed_bottom;
        // Debug-only: a rung hoisted back out of the walk, or composed early,
        // shows up here as a diagnosable string rather than a visual mystery.
        if let Err(why) = render::check_bottom_band_order(&self.composed_bottom_band.borrow()) {
            debug_assert!(false, "{why}");
        }
    }

    /// Compose the **editor band** (#764, #735 slice 3) into `backend`.
    ///
    /// Walks `render::compose_editor_band` — the single ordered artefact both
    /// backends walk for the editor column, exactly as `FRAME_Z_ORDER` is for
    /// the surrounding chrome and the app-level overlays. Geometry stays here, in cells; the *order* and the *gates*
    /// live in `render.rs`. `EDITOR_Z_ORDER`'s doc comment records the two
    /// defects this convergence closed (GTK never painted the group dividers
    /// at all, and painted its tab-drag ghost above the editor popups).
    ///
    /// `area` is `layout.main_content_bounds` in cells. Extracted from
    /// `render_content` so the entry point *sequences* bands rather than
    /// inlining each one — the GTK twin splits its own heavier rungs the same
    /// way (`paint_editor_windows_rung` / `paint_tab_bars_rung`).
    fn paint_editor_band(
        &self,
        backend: &mut dyn quadraui::Backend,
        screen: &render::ScreenLayout,
        area: Rect,
        theme: &Theme,
    ) {
        // ══ Editor band (#764, #735 slice 3) ═════════════════════════════
        //
        // Composed from `render::compose_editor_band` — the single ordered
        // artefact both backends walk for the editor column, exactly as
        // `CHROME_Z_ORDER` (below) is for the surrounding chrome and
        // `OVERLAY_Z_ORDER` (below that) for the app-level overlays. Geometry
        // stays here, in cells; only the *order* and the *gates* moved.
        // `EDITOR_Z_ORDER`'s doc comment records the two defects this closes
        // (GTK never painted the group dividers at all, and painted its
        // tab-drag ghost above the editor popups).
        //
        // Reset per-frame: `post_draw_apply_widths` wants this frame's
        // measurements, not an ever-growing accumulation.
        self.tab_visible_counts.borrow_mut().clear();
        let tui_tbh: f64 = if self.engine.settings.breadcrumbs && !self.engine.terminal_maximized {
            2.0
        } else {
            1.0
        };
        let mut composed_editor: Vec<render::EditorOp> = Vec::new();
        for op in render::compose_editor_band(
            &self.engine,
            screen,
            // `is_dragging()`, not the `source().is_some()` this call site used
            // to test: they agree (`begin` sets both, `handle_release` clears
            // both), but `is_dragging` is the gate that method's own doc names
            // as "the gate both backends' drop overlays paint behind", and it
            // is the one GTK already used.
            self.tab_drag.is_dragging(),
            self.engine.terminal_maximized,
        ) {
            match op {
                // `render_all_windows` also paints the within-group
                // (`:split`/`:vsplit`) divider lines unconditionally now
                // (#609 routed `render_separators` through
                // `Backend::draw_status_bar` — see its doc comment — so it no
                // longer needs the raw `Frame` that `frame: None` used to
                // skip it for).
                render::EditorOp::Windows => {
                    render_all_windows(backend, None, &screen.windows, theme)
                }
                // #35/#722: minimap strips on every window's right edge (one
                // entry per `WindowId` in `screen.minimap`, not just the
                // active window's) — one call, the braille rasteriser is
                // quadraui's.
                render::EditorOp::Minimap => {
                    render::draw_minimap_strip(backend, screen);
                }
                render::EditorOp::TabBars => {
                    backend.set_theme(super::quadraui_tui::q_theme(theme));
                    let painted =
                        render::paint_tab_bars(backend, &self.engine, screen, 1.0, tui_tbh, None);
                    let mut counts = self.tab_visible_counts.borrow_mut();
                    for bar in &painted {
                        counts.push((bar.group_id, bar.hits.available_cols));
                    }
                }
                render::EditorOp::Breadcrumbs => {
                    backend.set_theme(super::quadraui_tui::q_theme(theme));
                    render::paint_breadcrumb_bars(backend, screen, self.engine.terminal_maximized);
                }
                // Between-*group* dividers (`Ctrl+W v`/`Ctrl+W s`, as opposed
                // to the `Windows` rung's within-group `render_separators`)
                // — ported to `Backend::draw_status_bar` via
                // `render_group_dividers` (see its doc comment, and
                // `group_divider_cells`'s for how the #481
                // phantom-divider-beside-scrollbar guard became a pure data
                // computation instead of a `Buffer` read-back). GTK
                // rasterises the same rung through quadraui's `Split`
                // primitive instead — see `render::draw_dividers_as_splits`
                // for why the two rasterisers legitimately differ.
                render::EditorOp::GroupDividers => render_group_dividers(
                    backend,
                    &screen.group_dividers,
                    &screen.windows,
                    area,
                    theme,
                ),
                // Drag state (the shared `render::TabDragState`) is already
                // live here — #602 wired `handle_mouse_event` to advance it
                // via `mouse::handle_mouse` (see `handle()` below).
                render::EditorOp::TabDragOverlay => render_tab_drag_overlay(
                    backend,
                    &self.engine,
                    screen,
                    theme,
                    self.tab_drag.source(),
                    self.tab_drag.cursor(),
                    self.tab_drag.zone(),
                ),
                // Unlike `draw_frame`'s `editor_area` (whose `y` is
                // implicitly 0-based — it's the live terminal frame's own
                // top-level split), `area` here is
                // `layout.main_content_bounds`, already offset below whatever
                // `AppShell::render` painted above it — see that function's
                // doc comment for why the row math differs between the two
                // callers. That offset already includes `AppShell`'s
                // title-bar row whenever the menu bar is visible
                // (`compute_layout`'s `band_y += h`), so — unlike
                // `draw_frame`'s `menu_rows + 1` — there is no menu term to
                // add here; adding one would double-count the row (#635 item
                // A, and see `build_screen_for_shell_content`'s doc comment).
                render::EditorOp::TabTooltip => {
                    if let Some(ref tooltip_text) = screen.tab_tooltip {
                        render_tab_hover_tooltip(
                            backend,
                            area.x,
                            area.y + 1,
                            area.width,
                            tooltip_text,
                            theme,
                        );
                    }
                }
            }
            composed_editor.push(op);
        }
        *self.composed_editor_band.borrow_mut() = composed_editor;
        // Same contract as the chrome/overlay bands: read back through the
        // field rather than the local, so the *stored* observable is what gets
        // validated — a frame that recorded one thing and composed another
        // would be a lie the tests then trusted.
        if let Err(why) = render::check_editor_band_order(&self.composed_editor_band.borrow()) {
            debug_assert!(false, "TUI {why}");
        }
    }

    /// Construct the app, running the engine-only startup work that
    /// `tui_main::run()` currently does before entering raw mode
    /// (`mod.rs:641`-`:678`) — none of it needs a terminal or backend.
    pub fn new(file_path: Option<PathBuf>) -> Self {
        Self::from_engine(Engine::new(), file_path, true)
    }

    /// Test-only deterministic twin of [`TuiShellApp::new`].
    ///
    /// [`TuiShellApp::new`] is **ambient in two places at once**, and both of
    /// them move the editor pane's geometry:
    ///
    /// 1. `Engine::new()` reads the developer's real
    ///    `~/.config/vimcode/{settings,session}.json` off disk, and uses
    ///    `session.explorer_visible || settings.explorer_visible_on_startup`
    ///    to decide whether to `app_shell.hide_sidebar()` before it returns
    ///    (#615/#634 — see [`tests::app_with_sidebar_open`]'s doc comment for
    ///    the five tests that already learned this the hard way). A machine
    ///    that has ever opened the explorer boots the sidebar *visible*; a
    ///    fresh checkout or CI runner boots it *hidden*, shifting every
    ///    editor column by `SIDEBAR_WIDTH`.
    /// 2. `Engine::startup(None)` then calls `restore_session_files()`, which
    ///    reopens whatever files/splits are listed in the *per-workspace*
    ///    session file for the process's `current_dir()` — so even the number
    ///    of editor *windows* is machine-dependent.
    ///
    /// Any test that measures painted editor geometry (`find_bounds` on a
    /// fixture line, `style_at` on a specific column) must therefore build the
    /// app from in-memory defaults instead of inheriting the developer's box.
    ///
    /// Both halves need an explicit fix, and swapping the engine constructor
    /// alone only fixes (1):
    ///
    /// * (1) is `Engine::new_for_test()`, which substitutes in-memory
    ///   `Settings::default()` / `SessionState::default()` for the two global
    ///   config reads.
    /// * (2) is **not** covered by that. `restore_session_files()` performs its
    ///   own independent `SessionState::load_for_workspace(&self.cwd)` disk
    ///   read, keyed on `current_dir()` rather than on whatever state the
    ///   engine was built from, and unlike its `save_for_workspace` counterpart
    ///   it has no `cfg(test)` stub (some tests legitimately assert a written
    ///   workspace session *is* restored). Running the test binary from a
    ///   checkout that has a real `~/.config/vimcode/sessions/<hash>.json` —
    ///   entirely plausible for a self-hosting editor — would therefore restore
    ///   that session's files and splits, pushing `windows.len()` past 1. So
    ///   this constructor routes through
    ///   [`Engine::startup_without_session_restore`] instead.
    ///
    /// Everything else runs the *same* `from_engine` body as the production
    /// constructor, so the two cannot drift.
    #[cfg(test)]
    pub fn new_for_test() -> Self {
        Self::from_engine(Engine::new_for_test(), None, false)
    }

    /// Shared body of [`TuiShellApp::new`] and [`TuiShellApp::new_for_test`] —
    /// everything except *where the engine's initial state came from*.
    ///
    /// `restore_session` is `true` for the production constructor and `false`
    /// for [`TuiShellApp::new_for_test`]; see that method for why skipping the
    /// per-workspace session restore is required for determinism.
    fn from_engine(mut engine: Engine, file_path: Option<PathBuf>, restore_session: bool) -> Self {
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
        if restore_session {
            engine.startup(file_path.as_deref());
        } else {
            engine.startup_without_session_restore(file_path.as_deref());
        }
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
            divider_grab: None,
            hover_selecting: false,
            fr_input_dragging: false,
            last_layout: RefCell::new(None),
            tab_visible_counts: RefCell::new(Vec::new()),
            debug_toolbar_rect: std::rc::Rc::new(Cell::new(quadraui::Rect::default())),
            explorer_sb_dragging: false,
            explorer_drag_src: None,
            explorer_drag_active: None,
            tab_drag: render::TabDragState::default(),
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
            tab_switcher_popup_rect: RefCell::new(None),
            composed_frame: std::rc::Rc::new(RefCell::new(Vec::new())),
            composed_editor_band: std::rc::Rc::new(RefCell::new(Vec::new())),
            composed_bottom_band: std::rc::Rc::new(RefCell::new(Vec::new())),
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
    pub fn shell_config(menu_bar_visible: bool) -> quadraui::ShellConfig {
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
        // Alt+Right would silently stop at 50. The bounds are the shared
        // rung's own clamps (#759): `render::alt_resized_sidebar_width` is
        // what Alt+Left/Right applies on both backends, so the `AppShell`
        // underneath must not narrow them further. GTK's
        // `build_shell_config` sets the identical pair from the same two
        // constants.
        cfg.default_sidebar_width = SIDEBAR_WIDTH as f32;
        cfg.min_sidebar_width = render::ALT_SIDEBAR_WIDTH_MIN as f32;
        cfg.max_sidebar_width = render::ALT_SIDEBAR_WIDTH_MAX as f32;
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
    /// no double-click concept to round-trip through
    /// [`super::events::uievent_to_crossterm`]), but capture the fact that it
    /// *was* a `DoubleClick` in `is_double_click` first and pass that through
    /// to `handle_mouse` as a plain `bool` (#817). `handle_mouse` used to
    /// re-derive the same verdict itself from a hand-rolled
    /// `last_click_time`/`last_click_pos` pair — a second, independent
    /// 400ms/position detector racing the backend's own
    /// `quadraui::DoubleClickDetector` that already classified this event as
    /// `DoubleClick` in the first place. The one piece `event_loop`
    /// couldn't do — obtain `&mut DragState` *and* `&mut ModalStack` from a
    /// `&mut dyn Backend` — is exactly what
    /// `Backend::{drag_state_handle, modal_stack_handle}` (quadraui#704,
    /// which replaced the earlier `drag_and_modal_mut` of quadraui#467) are
    /// for: they return independent `Rc<RefCell<_>>` handles, so the two
    /// simultaneous `&mut` borrows fall out of `borrow_mut()`ing each.
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
            let modal_rc = backend.modal_stack_handle();
            let modal_stack = modal_rc.borrow();
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
                // #637 / #754: the event landed inside this panel's own body
                // rect — it must be claimed here unconditionally, even when
                // the inner `SidebarEvent` comes back `Ignored` (e.g. a click
                // on empty space below the last row, or between two headers
                // when the list is empty). Only returning `Redraw` on a
                // "successful" dispatch let an `Ignored` result fall through
                // to `mouse::handle_mouse`'s unrelated legacy dispatcher,
                // which interprets the *same* coordinates under a totally
                // different column-range model and can silently reset focus
                // this intercept just claimed (reproduced by
                // `debug_sidebar_intercept_claims_focus_on_mouse_down` /
                // `ext_sidebar_intercept_claims_focus_on_mouse_down`, which
                // hit exactly this path whenever the panel's list is empty).
                // The dispatch itself is `render::dispatch_dap_sidebar_body_event`
                // — the same function GTK's `route_debug_sidebar_event` calls.
                render::dispatch_dap_sidebar_body_event(&mut self.engine, &event, rect, backend);
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
                // #754: the `TreeController` dispatch itself is
                // `render::route_explorer_tree_event`, the same function
                // GTK's `explorer_ui_event` and `mouse::handle_mouse`'s own
                // explorer arm call. `(1.0, 1.0)`: TUI's tree paints one
                // row/column per cell, so there is no pixel metric to
                // re-apply the way GTK does.
                let tree_event = render::route_explorer_tree_event(
                    &mut self.engine,
                    &event,
                    rect,
                    (1.0, 1.0),
                    &theme,
                    backend,
                );
                match &event {
                    UiEvent::DoubleClick { .. } => {
                        if let Some(tree_event) = tree_event {
                            self.engine.explorer_has_focus = true;
                            self.sidebar.has_focus = true;
                            self.engine.dispatch_explorer_tree_event(tree_event);
                        }
                    }
                    UiEvent::MouseDown { .. } => {
                        if let Some(tree_event) = tree_event {
                            let is_scrollbar =
                                matches!(tree_event, quadraui::TreeControllerEvent::ScrollChanged);
                            if is_scrollbar {
                                self.explorer_sb_dragging = true;
                            } else {
                                self.engine.explorer_has_focus = true;
                                self.sidebar.has_focus = true;
                            }
                            self.engine.handle_explorer_mouse_event(tree_event);
                        }
                    }
                    UiEvent::MouseUp { .. } => {
                        self.explorer_sb_dragging = false;
                    }
                    _ => {} // MouseMoved — TreeController drag_to() handles internally
                }
                return Reaction::Redraw;
            }
        }

        // #817: capture the backend's own double-click verdict before
        // folding it away for the crossterm round-trip below — this is the
        // single source of truth `handle_mouse` now reads instead of running
        // a second, independent timer.
        let is_double_click = matches!(event, UiEvent::DoubleClick { .. });
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

        let mut should_quit = false;
        let last_layout = self.last_layout.borrow();
        let hover_link_rects = self.hover_link_rects.borrow();
        let editor_hover_link_rects = self.editor_hover_link_rects.borrow();
        let completion_layout = self.completion_layout.borrow();
        let context_menu_layout = self.context_menu_layout.borrow();
        let dialog_layout = self.dialog_layout.borrow();
        let editor_hover_scrollbar = *self.editor_hover_scrollbar.borrow();
        // quadraui#704 removed `Backend::drag_and_modal_mut`; the two states
        // are now reached through independent `Rc<RefCell<_>>` handles, so
        // the simultaneous `&mut` borrow it existed to provide comes for
        // free — take both handles and `borrow_mut()` each.
        let drag_rc = backend.drag_state_handle();
        let modal_rc = backend.modal_stack_handle();
        let mut drag_state = drag_rc.borrow_mut();
        let mut modal_stack = modal_rc.borrow_mut();

        let new_sidebar_width = super::mouse::handle_mouse(
            mouse_event,
            &mut self.sidebar,
            &mut self.engine,
            &terminal_size,
            self.sidebar_width,
            &mut self.dragging_sidebar,
            &mut self.dragging_terminal_resize,
            &mut self.dragging_terminal_split,
            &mut self.divider_grab,
            &mut drag_state,
            &mut modal_stack,
            last_layout.as_ref(),
            is_double_click,
            &mut self.folder_picker,
            &mut should_quit,
            &mut self.explorer_drag_src,
            &mut self.explorer_drag_active,
            &mut self.tab_drag,
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
            *self.tab_switcher_popup_rect.borrow(),
        );

        // quadraui#704: these two are `RefMut` guards on the backend's own
        // `RefCell`s, not plain `&mut`. Release them as eagerly as the other
        // borrows so any later code in this function (or a future addition)
        // can touch `backend`'s drag/modal state without a double-borrow
        // panic.
        drop(drag_state);
        drop(modal_stack);
        drop(last_layout);
        drop(hover_link_rects);
        drop(editor_hover_link_rects);
        drop(completion_layout);
        drop(context_menu_layout);
        drop(dialog_layout);

        self.sidebar_width = new_sidebar_width;

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

    /// #557/#754: open the plugin-provided panel `name` in the sidebar.
    ///
    /// The switch itself is `render::apply_activity_panel_switch` — the same
    /// call `mouse::handle_mouse`'s `ActivityBarTarget::ExtensionPanel` arm
    /// makes — rather than a second hand-rolled copy of it. That matters
    /// here specifically: `on_shell_event`'s `PanelChanged` arm (this
    /// method's only caller) can run `activate_ext_panel` again for a
    /// panel that's already active — e.g. a plugin re-registering itself
    /// mid-session — and the un-shared version of this method used to reset
    /// `ext_panel_selected` to `0` and re-fire `plugin_event("panel_focus",
    /// …)` unconditionally on *every* call, scrolling the list back to the
    /// top and double-firing the plugin's own focus hook for a no-op
    /// re-activation. `apply_activity_panel_switch`'s `already_showing`
    /// check is exactly that guard, previously GTK-only.
    ///
    /// A literal second left-click on the icon never reaches here at all —
    /// `AppShell` reports that as [`quadraui::AppShellEvent::SidebarHidden`]
    /// instead of a repeat `PanelChanged` — but `apply_activity_panel_switch`
    /// still handles it correctly (hides the sidebar) on the rare path where
    /// it would.
    fn activate_ext_panel(&mut self, name: &str) {
        let switched =
            render::apply_activity_panel_switch(&mut self.engine, &format!("ext:{name}"));
        self.sidebar.ext_panel_name = switched.ext_panel;
        if switched.sidebar_visible {
            self.sidebar.has_focus = true;
        }
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

        // ── Per-frame nerd-font sync ─────────────────────────────────────
        // Not a keyboard rung and never was — it is already one shared call
        // (`render::sync_nerd_fonts`), so #762 only drops the stale
        // cross-file line-number citation it used to carry. `setup()`
        // pushes the flag once;
        // re-pushing it every frame is what makes a runtime `:set nerdfonts`
        // / `:set nonerdfonts` reach the rasterisers on the very next paint
        // instead of never.
        render::sync_nerd_fonts(backend, &self.engine);

        // The menu row and the sidebar panel body used to be composed here, at
        // the top of the frame; both are chrome-band rungs now (#763) and are
        // composed from `render::CHROME_Z_ORDER` further down. The one
        // observable difference is that a degenerate `main_content_bounds` —
        // the early `return` just below — now also skips the sidebar *body*,
        // converging on GTK, whose own guard already sat above its sidebar
        // block.
        let mut pending_command_center: Option<(quadraui::Rect, quadraui::CommandCenter)> = None;

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

        // ══ Editor band (#764, #735 slice 3) ═════════════════════════════
        // See `paint_editor_band` for the rung ladder and why it is one shared
        // artefact rather than two hand-kept transcriptions.
        self.paint_editor_band(backend, &screen, area, &theme);

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

        // ══ Bottom band (#765, #735 slice 4) ═════════════════════════════
        // Composed from `render::compose_bottom_band` — see
        // `compose_bottom_band_rungs`.
        self.compose_bottom_band_rungs(backend, &screen, layout, &theme, area, win_area, &chrome);

        // ══ Frame sequence (#766, #735 slice 6) ══════════════════════════
        //
        // Composed from `render::compose_frame` — the single ordered artefact
        // both backends walk for everything around and on top of the editor
        // column. Slices 1-4 landed this as *four* ladders (editor, bottom,
        // chrome, overlay); slice 6 folds the chrome and overlay halves into
        // one `FrameOp` sequence, so the frame is no longer "a composer plus a
        // special-cased top band" a backend could get individually right and
        // jointly wrong. Geometry stays here, in cells; only the *order* and
        // the *gates* are shared.
        //
        // Every cache a rung owns is cleared *here*, before the walk, never
        // from an `else` arm inside it: `compose_frame` returns only the live
        // rungs, so an absent rung has no arm to run. Both title-bar rects must
        // reflect exactly what the shell reserved this frame — empty when
        // nothing was — because `handle()`'s MenuSystem intercept and
        // `mouse.rs`/`route_chrome_click` read them instead of re-deriving the
        // bands, which is what let paint and hit-test disagree (#695, #752);
        // the modal layouts must be cleared for the same reason on the frame
        // their surface disappears (the #587 class of bug).
        //
        // Every arm gates itself on the *value* it needs and, when it composes,
        // records the rung it composed — naming the variant explicitly
        // (`push(FrameOp::Dialog)`), never `push(op)`. That distinction is the
        // difference between a test that can fail and one that cannot: with
        // `push(op)` the record follows the *pattern* the walk is currently at,
        // so swapping two arms' bodies composes them in the wrong order while
        // still recording the right one.
        self.engine
            .menu_bar_rect
            .set(layout.title_bar_bounds.unwrap_or_default());
        self.engine
            .global_status_rect
            .set(quadraui::Rect::default());
        self.engine.command_center_layout.replace(None);
        *self.tab_switcher_popup_rect.borrow_mut() = None;
        *self.context_menu_layout.borrow_mut() = None;
        *self.dialog_layout.borrow_mut() = None;
        self.engine.toast_layout.replace(None);

        // Screen-anchored overlays (modals, toasts) deliberately use
        // `layout.window_bounds` — the *whole* terminal — rather than `area`
        // (`main_content_bounds`, the editor column): anchoring them to the
        // editor column would shift every modal right by the activity-bar +
        // sidebar width.
        let win_q = quadraui::Rect::new(
            win_area.x as f32,
            win_area.y as f32,
            win_area.width as f32,
            win_area.height as f32,
        );

        // Built once, before the walk, because its presence gate and its
        // `ToastStack` arm need the same value and `build_toast_stack` is not
        // free.
        let toast_stack = render::build_toast_stack(&self.engine);

        let mut presence =
            render::FramePresence::from_screen(&screen, layout, render::FrameMetrics::CELL);
        presence.toast_stack = toast_stack.is_some();
        presence.folder_picker = self.folder_picker.is_some();

        let mut composed: Vec<render::FrameOp> = Vec::new();
        for op in render::compose_frame(&presence) {
            match op {
                // ── Menu row: measure only ───────────────────────────────
                // `draw_menu_bar` is *not* called here. The `MenuDropdown`
                // rung calls `MenuSystem::render`, which unconditionally
                // repaints `draw_menu_bar` across the entire reserved band —
                // including the command centre's columns to the right of the
                // last menu label — whether or not a dropdown is open.
                // Painting the command centre before that meant it was wiped
                // every frame, leaving a populated-but-invisible
                // `command_center_layout` (paint and hit-test disagreeing,
                // exactly the #695 menu-bar and #676 GTK command-centre bugs).
                // So this rung only *measures* (`menu_bar_layout`), and
                // `pending_command_center` carries the computed rect +
                // descriptor across to the `CommandCenter` arm (#712).
                render::FrameOp::MenuRow => {
                    let band = self.engine.menu_bar_rect.get();
                    backend.set_theme(super::quadraui_tui::q_theme(&theme));
                    let bar = self.engine.menu_system.borrow().menu_bar();
                    // TUI has no app-icon slot and no in-canvas window
                    // controls, so the items rect *is* the band and the
                    // controls band comes back zero-width; the Command Center
                    // takes everything from the last menu label to the right
                    // edge, which is what this computed by hand before #763.
                    let bands = render::measure_title_bar_bands(backend, band, band, &bar, None);
                    pending_command_center = Some((
                        bands.command_center,
                        render::build_command_center_view(
                            self.engine.tab_nav_can_go_back(),
                            self.engine.tab_nav_can_go_forward(),
                            &self.window_title_stem(),
                        ),
                    ));
                    composed.push(render::FrameOp::MenuRow);
                }

                // ── Sidebar panel body (#607) ────────────────────────────
                // `AppShell::render` (quadraui, called by the runner before
                // `render_content`) already painted the generic sidebar chrome
                // — activity bar, header and separator — so this paints only
                // the *active panel's* body into
                // `layout.sidebar_content_bounds`. See
                // `panels::render_sidebar_content`'s doc comment for which
                // panels are ported.
                render::FrameOp::SidebarPanel => {
                    if let Some(sb) = layout.sidebar_content_bounds {
                        render_sidebar_content(
                            backend,
                            to_cell_rect(sb),
                            &self.sidebar,
                            &self.engine,
                            &theme,
                        );
                        composed.push(render::FrameOp::SidebarPanel);
                    }
                }

                // ── Wildmenu bar (command Tab completion) ────────────────
                render::FrameOp::Wildmenu => {
                    if let Some(ref wm) = screen.wildmenu {
                        let bar = render::wildmenu_to_status_bar(wm, &theme);
                        backend.draw_status_bar(to_q_rect(chrome.wildmenu), &bar, None, None);
                        composed.push(render::FrameOp::Wildmenu);
                    }
                }

                // ── Global status bar ────────────────────────────────────
                render::FrameOp::StatusBar => {
                    if let Some(ref bar) = screen.global_status_bar {
                        let q_rect = to_q_rect(chrome.status);
                        self.engine.global_status_rect.set(q_rect);
                        backend.draw_status_bar(q_rect, bar, None, None);
                        composed.push(render::FrameOp::StatusBar);
                    }
                }

                // ── Command line (+ mouse drag-selection inversion) ──────
                render::FrameOp::CommandLine => {
                    // #816: publish the painted rect for
                    // `render::command_line_click_char_idx` — the twin of
                    // `global_status_rect` above, and GTK's identical cache.
                    self.engine.command_line_rect.set(to_q_rect(chrome.cmd));
                    render_command_line(
                        backend,
                        chrome.cmd,
                        &screen.command,
                        &theme,
                        self.engine.cmd_sel.get(),
                    );
                    composed.push(render::FrameOp::CommandLine);
                }

                // ── Folder / workspace picker modal (#815) ───────────────
                // First rung of the overlay tail — above every chrome rung,
                // below the title-bar band and the modal stack, which is
                // exactly where it was composed before #766 made it a named
                // rung instead of a stray paint between two walks.
                // `quadraui::FolderPickerController::render` paints through
                // the shared `Palette` primitive — GTK's identical arm calls
                // the same method with its own (pixel-unit) popup rect.
                render::FrameOp::FolderPicker => {
                    if let Some(ref picker) = self.folder_picker {
                        let popup_rect = render::folder_picker_popup_rect(win_q, 1.0);
                        picker.render(popup_rect, backend);
                        composed.push(render::FrameOp::FolderPicker);
                    }
                }

                // ── Menu dropdown (#635, Stage 6b item A) ────────────────
                // `MenuSystem::render` repaints `draw_menu_bar` across the
                // whole reserved row, so nothing that wants to survive may be
                // drawn into that row before it — which is exactly why this
                // comes before `CommandCenter`. #695: read the same
                // `engine.menu_bar_rect` cache the `MenuRow` rung wrote,
                // rather than re-reading `layout.title_bar_bounds` a second
                // time — one write, every reader downstream (paint and
                // hit-test alike) consumes it verbatim.
                render::FrameOp::MenuDropdown => {
                    self.engine
                        .menu_system
                        .borrow()
                        .render(backend, self.engine.menu_bar_rect.get());
                    composed.push(render::FrameOp::MenuDropdown);
                }

                // ── Command centre (#635 Stage 6b item A, #712) ──────────
                // Painted *after* `menu_system.render()` above, which repaints
                // `draw_menu_bar` across the entire band and would erase
                // anything drawn here first. `command_center_layout` must
                // agree with what actually got painted, not linger with last
                // frame's stale layout (mouse.rs's hit test reads this cache
                // directly) — hence the unconditional clear before the walk.
                render::FrameOp::CommandCenter => {
                    if let Some((cc_rect, cc)) = pending_command_center.take() {
                        let painted = backend.draw_command_center(cc_rect, &cc);
                        self.engine.command_center_layout.replace(Some(painted));
                        composed.push(render::FrameOp::CommandCenter);
                    }
                }

                // ── Find/replace overlay ─────────────────────────────────
                // `find_replace.group_bounds` is already absolute
                // terminal-screen space (#550), so the rect passed here only
                // supplies the clip viewport.
                render::FrameOp::FindReplace => {
                    if let Some(ref find_replace) = screen.find_replace {
                        backend.draw_find_replace(win_q, find_replace);
                        composed.push(render::FrameOp::FindReplace);
                    }
                }

                // ── Unified picker modal ─────────────────────────────────
                render::FrameOp::UnifiedPicker => {
                    if let Some(ref picker) = screen.picker {
                        render_picker_popup(picker, win_area, &theme, backend);
                        composed.push(render::FrameOp::UnifiedPicker);
                    }
                }

                // ── Tab switcher popup ───────────────────────────────────
                // #733: geometry from the shared `TabSwitcherGeometry` (same
                // `compute`, different sizing constant, as GTK), and the
                // *painted* rect is cached for `handle_mouse`'s modal-overlay
                // rung — before this the TUI had no tab-switcher mouse arm at
                // all, so a click on the popup fell through to the editor
                // underneath (#733).
                render::FrameOp::TabSwitcher => {
                    if let Some(ref ts) = screen.tab_switcher {
                        if let Some(geo) = render::TabSwitcherGeometry::compute(
                            win_q,
                            ts.items.len(),
                            &render::TUI_TAB_SWITCHER_SIZING,
                        ) {
                            let list =
                                render::tab_switcher_to_quadraui_list_view(ts, geo.max_visible);
                            backend.draw_list(geo.bounds, &list);
                            *self.tab_switcher_popup_rect.borrow_mut() = Some(geo.bounds);
                            composed.push(render::FrameOp::TabSwitcher);
                        }
                    }
                }

                // ── Context menu popup ───────────────────────────────────
                // Painting this also closes the residual #602 seam noted in
                // the module doc: `handle_mouse` receives `context_menu_layout`
                // from the cell written here, so a click on a menu item now
                // resolves to that item instead of falling through to "close
                // the menu".
                render::FrameOp::ContextMenu => {
                    if let Some(ctx_menu) =
                        screen.context_menu.as_ref().filter(|p| !p.items.is_empty())
                    {
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
                        let (menu, menu_layout) = render::context_menu_generic_layout(
                            &inset_panel,
                            inner_viewport,
                            1.0,
                            1.0,
                            1.0,
                        );
                        let _ = backend.draw_context_menu(&menu, &menu_layout);
                        *self.context_menu_layout.borrow_mut() = Some(menu_layout);
                        composed.push(render::FrameOp::ContextMenu);
                    }
                }

                // ── Modal dialog ─────────────────────────────────────────
                // Above the context menu, matching
                // `route_modal_overlay_click`'s own arbitration: once a dialog
                // is open it takes every event, so it must also be the surface
                // the user can see. GTK painted it *underneath* until #735.
                render::FrameOp::Dialog => {
                    if let Some(ref dialog) = screen.dialog {
                        let (q_dialog, dlg_layout) =
                            render::dialog_generic_layout(dialog, win_q, 1.0, 1.0);
                        let _ = backend.draw_dialog(&q_dialog, &dlg_layout);
                        *self.dialog_layout.borrow_mut() = Some(dlg_layout);
                        composed.push(render::FrameOp::Dialog);
                    }
                }

                // ── Toast overlay (#450) — top of the sequence ───────────
                render::FrameOp::ToastStack => {
                    if let Some(ref stack) = toast_stack {
                        let toast_layout = backend.draw_toast_stack(win_q, stack);
                        self.engine.toast_layout.replace(Some(toast_layout));
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
            debug_assert!(false, "TUI {why}");
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
            // ── Panel-key accelerators (shared with GTK — #761 / #734 slice 6) ──
            // `render::dispatch_panel_accelerator` needs a screen size for its
            // `TerminalToggleMax`/`OpenTerminal` hooks (`TuiAccelHost` below);
            // `backend.viewport()` is this backend's source for it.
            if let UiEvent::Accelerator(ref acc_id, acc_mods) = event {
                if self.engine.dialog.is_none() {
                    let viewport = backend.viewport();
                    let mut host = TuiAccelHost {
                        sidebar: &mut self.sidebar,
                        screen_w: viewport.width as u16,
                        screen_h: viewport.height as u16,
                        sidebar_width: self.sidebar_width,
                        mods: acc_mods,
                    };
                    if render::dispatch_panel_accelerator(
                        acc_id.as_str(),
                        &mut self.engine,
                        &mut host,
                    )
                    .is_some()
                    {
                        break 'dispatch Reaction::Redraw;
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
                                    self.folder_picker =
                                        Some(new_folder_picker_controller(&self.engine));
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
                            yank_hl_deadline: &self.yank_hl_deadline,
                            ui_event: &dap_event,
                        },
                    )
                }
                // ── Bracketed paste (#758 / #734 slice 3) ───────────────
                // The runner maps crossterm's `Event::Paste` to
                // `UiEvent::ClipboardPaste`; without this arm a paste into
                // the TUI is silently dropped. The rung itself is already
                // shared: `Engine::route_paste` is the one focus-priority
                // paste router both backends call (GTK's identical arm is in
                // `gtk/mod.rs`'s `UiEvent::ClipboardPaste`), and its terminal
                // branch now delegates the bracketed-paste decision to
                // quadraui's `TerminalSession::paste` (quadraui#343/#415)
                // instead of wrapping unconditionally. The extra
                // `sync_tui_clipboard` is TUI-only by necessity: crossterm
                // has no clipboard, so the `+` register is the backing store
                // (GTK reads the real system clipboard).
                UiEvent::ClipboardPaste(ref text) => {
                    self.engine.route_paste(text);
                    sync_tui_clipboard(&mut self.engine, &mut self.last_clipboard_content);
                    Reaction::Redraw
                }
                // ── Resize → PTY resize (#758 / #734 slice 3) ───────────
                // The runner already debounces the crossterm resize burst
                // (`RESIZE_SETTLE`) and re-reads the real terminal size for
                // painting every frame, so only the embedded shell's own
                // SIGWINCH needs forwarding here. The legacy loop's
                // accompanying `terminal.clear()` has no shell-runner
                // equivalent — see the Ctrl+L note in `handle_key_pressed`.
                //
                // `render::route_terminal_resize` rather than
                // `Engine::terminal_resize`: the latter resizes *every* pane
                // to the full panel width, which reflows a split's
                // half-width panes off the area they are painted into (#471).
                UiEvent::WindowResized { viewport } => {
                    let term_rows = self.engine.session.terminal_panel_rows;
                    render::route_terminal_resize(
                        &mut self.engine,
                        viewport.width as u16,
                        term_rows,
                    );
                    Reaction::Redraw
                }
                // #602 (gap 2): dispatch through the legacy `mouse::handle_mouse`
                // now that `Backend::{drag_state_handle, modal_stack_handle}`
                // (quadraui#704) make its `&mut DragState`/`&mut ModalStack`
                // params reachable through `&mut dyn Backend`.
                // See `Self::handle_mouse_event`.
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
        // `render::dispatch_panel_accelerator`, `:set menu`, ...) has no
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
        // approximation and `TuiAccelHost` all read.
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

        // ── Terminal chrome the runner doesn't own ───────────────────────
        // Not a keyboard rung; #762 only drops the stale cross-file
        // line-number citation it used to carry (the GTK loop it named is
        // gone, and this has no GTK counterpart — GTK4 owns cursor shape
        // and window title).
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

/// [`render::PanelAcceleratorHost`] impl for TUI: the five hooks that need
/// TUI-local state — `TuiSidebar::has_focus` (the single input-focus token a
/// terminal has to track by hand where GTK gets real widget focus from the
/// toolkit) and the screen-size-derived terminal column/row counts GTK's
/// stubbed `terminal_cols`/`terminal_target_maximize_rows` (#731) don't need.
/// See the rung's header comment in `render.rs` for why these five can't
/// share a body with GTK's [`crate::gtk`] equivalent.
struct TuiAccelHost<'a> {
    sidebar: &'a mut TuiSidebar,
    screen_w: u16,
    screen_h: u16,
    sidebar_width: u16,
    mods: quadraui::Modifiers,
}

impl render::PanelAcceleratorHost for TuiAccelHost<'_> {
    fn toggle_sidebar(&mut self, engine: &mut Engine) {
        engine.toggle_sidebar();
        if !engine.app_shell.sidebar_visible() {
            self.sidebar.has_focus = false;
        }
    }

    fn focus_explorer(&mut self, engine: &mut Engine) {
        if self.sidebar.has_focus && engine.explorer_has_focus {
            self.sidebar.has_focus = false;
            engine.clear_sidebar_focus();
        } else {
            engine.toggle_sidebar_panel(PANEL_EXPLORER);
            self.sidebar.has_focus = true;
        }
    }

    fn focus_search(&mut self, engine: &mut Engine) {
        if self.sidebar.has_focus && engine.search_has_focus {
            self.sidebar.has_focus = false;
            engine.clear_sidebar_focus();
        } else {
            engine.toggle_sidebar_panel(PANEL_SEARCH);
            self.sidebar.has_focus = true;
        }
    }

    fn open_terminal(&mut self, engine: &mut Engine) {
        if engine.terminal_open && engine.terminal_has_focus {
            engine.close_terminal();
        } else if engine.terminal_open {
            engine.terminal_has_focus = true;
        } else {
            let cols = terminal_panel_cols(engine, self.screen_w, self.sidebar_width);
            if engine.terminal_panes.is_empty() {
                engine.terminal_new_tab(cols, engine.session.terminal_panel_rows);
            } else {
                engine.open_terminal(cols, engine.session.terminal_panel_rows);
            }
        }
    }

    fn terminal_toggle_max(&mut self, engine: &mut Engine) {
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: terminal_panel_cols(engine, self.screen_w, self.sidebar_width),
            terminal_max_rows: terminal_target_maximize_rows_tui(engine, self.screen_h),
        };
        engine.handle_ui_event(
            crate::core::engine::UiEvent::Accelerator(
                quadraui::AcceleratorId::new(render::ACC_TERMINAL_TOGGLE_MAX),
                self.mods,
            ),
            ctx,
        );
    }
}

/// The [`render::FocusKeyRoute::ActivityBar`] arm of
/// [`handle_focus_owner_key`], split out so the panel arms read as one ladder.
/// Same table as GTK's `handle_activity_bar_key`; the TUI-local halves are
/// `TuiSidebar::{has_focus, ext_panel_name}` and closing the quadraui
/// `MenuSystem`, which needs the `&mut dyn Backend`.
fn handle_activity_bar_focused_key(
    key_event: KeyEvent,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    backend: &mut dyn quadraui::Backend,
) -> Reaction {
    use render::ActivityBarKeyAction;
    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);
    let key = match key_event.code {
        KeyCode::Char(c) => c.to_string(),
        code => tui_key_to_engine_name(code).unwrap_or("").to_string(),
    };
    match render::activity_bar_key_action(&key, ctrl) {
        ActivityBarKeyAction::MoveDown => engine.activity_bar_move_down(),
        ActivityBarKeyAction::MoveUp => engine.activity_bar_move_up(),
        ActivityBarKeyAction::Activate => {
            use crate::core::engine::sidebar::ActivityBarActivation;
            match engine.activity_bar_activate() {
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
        ActivityBarKeyAction::FocusOut => engine.activity_bar_focus_out(),
        ActivityBarKeyAction::Collapse => {
            engine.activity_bar_focus_out();
            engine.app_shell.hide_sidebar();
            engine.clear_sidebar_focus();
            sidebar.has_focus = false;
            engine.session.explorer_visible = false;
            let _ = engine.session.save();
        }
        ActivityBarKeyAction::Ignore => {}
    }
    Reaction::Redraw
}

/// The focus-owner keyboard *sink*: TUI's half of the rung
/// [`render::route_focus_key`] resolves (#757 / #734 slice 2), which is where
/// the ladder — and the four cross-backend divergences it used to hide — is
/// stated. Only the crossterm `KeyEvent` → key-name/unicode *translation*
/// stays backend-side (TUI's key spellings differ from GTK's — see
/// `tui_key_to_engine_name` vs `map_gtk_key_name` — and only TUI needs the
/// Ctrl+V clipboard pre-read, since quadraui's runner delivers Ctrl+V to GTK
/// as `UiEvent::ClipboardPaste` before any key event reaches this rung, per
/// `render::dispatch_sidebar_panel_key`'s `Search` arm doc comment).
///
/// The *mutation* — the six pure-`Engine` panel arms (Search, ExtPanel,
/// ExtSidebar, Settings, Ai, SourceControl) — is stated exactly once, in
/// [`render::dispatch_sidebar_panel_key`], which this function calls with
/// the translated key just as GTK's `handle_key_press` does (#762 / #734
/// slice 7). Debug and Explorer stay TUI-specific residue for the same
/// reason GTK keeps its own `dispatch_focus_owner_residual`: Debug needs the
/// live `&mut dyn Backend` the DAP panel's `SidebarSystem::handle`
/// re-dispatch takes, and Explorer's key table predates — and is entangled
/// with — TUI's own folder-rename/new-entry editing state.
///
/// **Unconditionally terminal:** every arm returns, and `Explorer` is the
/// resolver's fallback rather than a guarded arm, so a key reaching here never
/// falls through to the editor tier.
///
/// A free function (mirrors [`TuiAccelHost`]'s hooks) because
/// `TuiShellApp::handle()` is only reachable through `driver_with_shell`,
/// which has no accessor back to the concrete app's fields; over borrowed
/// pieces it stays directly unit-testable against a bare `Engine`. `ui_event`
/// is the original, un-round-tripped [`UiEvent`] the DAP arm re-dispatches.
#[allow(clippy::too_many_arguments)]
fn handle_focus_owner_key(
    route: render::FocusKeyRoute,
    key_event: KeyEvent,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    screen_h: u16,
    backend: &mut dyn quadraui::Backend,
    ui_event: &UiEvent,
) -> Reaction {
    let ctrl = key_event.modifiers.contains(KeyModifiers::CONTROL);

    // ── Activity bar (toolbar) ──────────────────────────────────────────
    if route == render::FocusKeyRoute::ActivityBar {
        return handle_activity_bar_focused_key(key_event, engine, sidebar, backend);
    }

    // Ctrl-W prefix: set pending state for window navigation. A Vim chord,
    // so it stays inline rather than becoming an accelerator. Still
    // TUI-only — GTK has no per-keypress chord latch to hang
    // `pending_ctrl_w` on, which is #406; converging it needs the latch
    // promoted into the engine and is out of scope for this slice.
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
    if route == render::FocusKeyRoute::Search {
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
            // Single-char keys use the char as the key name (via `unicode`
            // below); Ctrl+b is the one that needs an explicit name.
            KeyCode::Char('b') if ctrl => "b",
            KeyCode::Char(_) => "",
            code => tui_key_to_engine_name(code).unwrap_or(""),
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
        // Shared dispatch (#762 / #734 slice 7): the same pure-`Engine` arm
        // `render::dispatch_sidebar_panel_key` states for GTK.
        let still_focused =
            render::dispatch_sidebar_panel_key(engine, route, &key_str, unicode, None, ctrl, alt)
                .unwrap_or(true);
        if !still_focused {
            sidebar.has_focus = false;
        }
        return Reaction::Redraw;
    }

    // ── Debug (DAP) panel ───────────────────────────────────────────────
    if route == render::FocusKeyRoute::Debug {
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
    if route == render::FocusKeyRoute::ExtPanel {
        // One spelling for both sub-modes: `render::dispatch_sidebar_panel_key`
        // (and, under it, `Engine::handle_ext_panel_key`/`handle_ext_panel_input_key`)
        // branches on `ext_panel_input_active` itself, the same way GTK's
        // caller (`map_gtk_key_name`) never special-cases it either — neither
        // engine method reads `ctrl`, and both accept a raw named key or a
        // literal character.
        let (key_name, unicode) = match key_event.code {
            KeyCode::Char(c) => (c.to_string(), Some(c)),
            code => (
                tui_key_to_engine_name(code)
                    .map(str::to_string)
                    .unwrap_or_default(),
                None,
            ),
        };
        let still_focused = render::dispatch_sidebar_panel_key(
            engine, route, &key_name, unicode, None, ctrl, false,
        )
        .unwrap_or(true);
        if !still_focused {
            sidebar.has_focus = false;
            // Keep `ext_panel_name` when focus moved to the activity bar
            // (the panel stays visible while the toolbar cursor shows).
            if !engine.activity_bar_focused {
                sidebar.ext_panel_name = None;
            }
        }
        return Reaction::Redraw;
    }

    // ── Extensions marketplace panel ────────────────────────────────────
    if route == render::FocusKeyRoute::ExtSidebar {
        let (key_name, unicode) = match key_event.code {
            KeyCode::Char(c) => (c.to_string(), Some(c)),
            code => (
                tui_key_to_engine_name(code)
                    .map(str::to_string)
                    .unwrap_or_default(),
                None,
            ),
        };
        let still_focused = render::dispatch_sidebar_panel_key(
            engine, route, &key_name, unicode, None, ctrl, false,
        )
        .unwrap_or(true);
        if !still_focused {
            sidebar.has_focus = false;
        }
        return Reaction::Redraw;
    }

    // ── Settings panel ──────────────────────────────────────────────────
    if route == render::FocusKeyRoute::Settings {
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
            let mapped = if key_name == "char" { "" } else { key_name };
            let still_focused =
                render::dispatch_sidebar_panel_key(engine, route, mapped, ch, None, ctrl, false)
                    .unwrap_or(true);
            if !still_focused {
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
    if route == render::FocusKeyRoute::Ai {
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
            let still_focused =
                render::dispatch_sidebar_panel_key(engine, route, mapped, uni, None, ctrl, false)
                    .unwrap_or(true);
            if !still_focused {
                sidebar.has_focus = false;
            }
        }
        return Reaction::Redraw;
    }

    // ── Source Control panel ────────────────────────────────────────────
    if route == render::FocusKeyRoute::SourceControl {
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
            code => (tui_key_to_engine_name(code).unwrap_or(""), None),
        };
        if !key_str.is_empty() || unicode.is_some() {
            // `sc_unicode` (not `unicode`) is the slot
            // `render::dispatch_sidebar_panel_key`'s `SourceControl` arm reads —
            // the shift-resolved character computed above.
            let still_focused = render::dispatch_sidebar_panel_key(
                engine, route, key_str, None, unicode, ctrl, false,
            )
            .unwrap_or(true);
            if !still_focused {
                sidebar.has_focus = false;
            }
        }
        return Reaction::Redraw;
    }

    // ── Explorer (`FocusKeyRoute::Explorer`, the resolver's fallback) ───
    {
        use crate::core::engine::ExplorerKeyResult;
        if ctrl && key_event.code == KeyCode::Char('b') {
            engine.app_shell.hide_sidebar();
            sidebar.has_focus = false;
            engine.clear_sidebar_focus();
            engine.session.explorer_visible = false;
            let _ = engine.session.save();
        } else {
            // `tui_key_to_engine_name` rather than a fourth bespoke copy of
            // the same table: it also supplies "BackSpace"/"Delete", which
            // the old explorer-local table dropped even though
            // `dispatch_explorer_edit_key` handles them — so rename/new-entry
            // editing lost those two keys on TUI while GTK
            // (`map_gtk_key_name`) had them. `Page_Up`/`Page_Down` and
            // `PageUp`/`PageDown` are both accepted by the engine.
            let key_name = match key_event.code {
                KeyCode::Char('j') => "j",
                KeyCode::Char('k') => "k",
                KeyCode::Char('h') => "h",
                KeyCode::Char('l') => "l",
                KeyCode::Char('q') => "q",
                KeyCode::Char(_) => "",
                code => tui_key_to_engine_name(code).unwrap_or(""),
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
    folder_picker: &mut Option<quadraui::FolderPickerController>,
    keyboard_enhanced: bool,
    screen_w: u16,
    screen_h: u16,
    backend: &mut dyn quadraui::Backend,
    state: &mut KeyDispatchState<'_>,
) -> Reaction {
    let Some(key_event) = quadraui::tui::events::synth_keyevent(&key, modifiers, repeat) else {
        return Reaction::Continue;
    };

    // ── Shared modal keyboard rung (#734 slice 1) ──────────────────────
    let modal_route = render::route_modal_key(engine);
    if modal_route != render::ModalKeyRoute::None {
        return apply_modal_key_route(
            modal_route,
            key_event,
            keyboard_enhanced,
            engine,
            sidebar,
            screen_w,
            screen_h,
        );
    }

    // ── Shared folder-picker rung (#815 / #762 / #734 slice 7) ──────────
    // Above every non-modal tier: once `EngineAction::OpenFolderDialog`
    // (below) populates `folder_picker`, every key belongs to the picker.
    // `FolderPickerController::handle` owns the key→intent mapping itself
    // now (Escape/Enter/Up/Down/k/j/-/Backspace/printable, Ctrl-gated) — this
    // rung just feeds it the raw event and applies the outcome.
    if folder_picker.is_some() && key_event.kind != KeyEventKind::Release {
        apply_folder_picker_event(
            folder_picker,
            state.ui_event,
            engine,
            sidebar,
            screen_w,
            screen_h,
        );
        return Reaction::Redraw;
    }

    if key_event.kind == KeyEventKind::Release {
        return Reaction::Continue;
    }

    // ── Focus owners: activity bar + sidebar panels (#757 / slice 2) ────
    // Slice 7 converges the resolver's *position*: the activity bar outranks
    // the global debugger F-keys on both backends, a sidebar panel does not.
    // TUI used to resolve the whole ladder here, so a focused search panel
    // swallowed F5/F9/F10/F11 that GTK sent to the debugger.
    let focus_route = render::route_focus_key(engine, sidebar.has_focus);
    let dispatch_focus_owner =
        |engine: &mut Engine, sidebar: &mut TuiSidebar, backend: &mut dyn quadraui::Backend| {
            handle_focus_owner_key(
                focus_route,
                key_event,
                engine,
                sidebar,
                screen_h,
                backend,
                state.ui_event,
            )
        };
    if focus_route == render::FocusKeyRoute::ActivityBar {
        return dispatch_focus_owner(engine, sidebar, backend);
    }

    let Some((key_name, unicode, ctrl)) = translate_key(key_event, keyboard_enhanced) else {
        // Untranslatable (Tab/BackTab and friends) — no rung below can read
        // them, but a focused panel navigates with them.
        if focus_route != render::FocusKeyRoute::None {
            return dispatch_focus_owner(engine, sidebar, backend);
        }
        return Reaction::Continue;
    };

    // ── Shared Ctrl+L force-redraw rung (#762 / #734 slice 7) ───────────
    if render::is_force_redraw_key(&key_name, unicode, ctrl) {
        return Reaction::Redraw;
    }

    // ── Shared terminal (PTY) rung (#758 / #734 slice 3, #351) ──────────
    let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key_event.modifiers.contains(KeyModifiers::ALT);
    if render::route_terminal_key(engine, &key_name, unicode, ctrl, shift, alt) {
        return Reaction::Redraw;
    }

    // ── Shared debugger F-key rung (#762 / #734 slice 7) ────────────────
    // Global. Below the terminal rung (a focused PTY keeps its own F-keys —
    // vim/htop bind them) and above the sidebar panels.
    match render::route_debug_fkey(&key_name, ctrl, shift, alt) {
        Some(render::DebugFKey::Command(cmd)) => {
            let _ = engine.execute_command(cmd);
            return Reaction::Redraw;
        }
        Some(render::DebugFKey::EngineKey(name)) => {
            let action = engine.handle_key(name, None, false);
            if handle_action(engine, action) {
                return Reaction::Exit;
            }
            return Reaction::Redraw;
        }
        None => {}
    }

    if focus_route != render::FocusKeyRoute::None {
        return dispatch_focus_owner(engine, sidebar, backend);
    }

    // ── Shared Alt-modifier / VSCode-mode rung (#759 / #734 slice 4) ────
    match render::route_alt_key(engine, &key_name, unicode, shift, alt) {
        render::AltKeyOutcome::ResizeSidebar(delta) => {
            *state.sidebar_width = render::alt_resized_sidebar_width(*state.sidebar_width, delta);
            return Reaction::Redraw;
        }
        render::AltKeyOutcome::Handled => return Reaction::Redraw,
        render::AltKeyOutcome::Fallthrough => {}
    }

    // ── Shared clipboard-paste pre-load / Ctrl+Shift+V rung (#760 / slice 5)
    render::preload_paste_clipboard(engine, &key_name, unicode, ctrl);
    if render::route_ctrl_shift_v_paste(engine, &key_name, ctrl) {
        return Reaction::Redraw;
    }

    // ── Shared hover-popup copy rung (#762 / #734 slice 7) ──────────────
    if let Some(text) = render::route_hover_popup_copy(engine, &key_name, ctrl) {
        tui_copy_to_clipboard(&text, engine);
        engine.message = "Hover text copied".to_string();
        return Reaction::Redraw;
    }

    // ── Shared command-line selection rung (#762 / #734 slice 7 / #816) ─
    // `engine.cmd_sel` is mouse-populated (#602, and GTK's press/drag
    // handlers since #816); this is its keyboard side, shared by both
    // backends now that the state lives on `Engine` instead of TUI-only
    // local state. `Clear` deliberately falls through to the editor below.
    match render::route_cmdline_selection_key(engine, unicode, ctrl, engine.cmd_sel.get()) {
        render::CmdSelKeyRoute::Copy(text) => {
            if !text.is_empty() {
                tui_copy_to_clipboard(&text, engine);
            }
            engine.cmd_sel.set(None);
            return Reaction::Redraw;
        }
        render::CmdSelKeyRoute::Clear => engine.cmd_sel.set(None),
        render::CmdSelKeyRoute::Keep => {}
    }

    // ── General fallback: `Engine::handle_key` ──────────────────────────
    let action = engine.handle_key(&key_name, unicode, ctrl);
    if engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
        engine.ai_completion_reset_timer();
    }

    if dispatch_post_key_action(
        action,
        engine,
        sidebar,
        folder_picker,
        screen_w,
        screen_h,
        *state.sidebar_width,
    ) {
        return Reaction::Exit;
    }

    // ── Shared post-key epilogue (#762 / #734 slice 7) ──────────────────
    // Macro playback (`@q`) drains first: it can request a quit, and only
    // `handle_action` can apply the `EngineAction`s it yields.
    loop {
        let (has_more, action) = engine.advance_macro_playback();
        if handle_action(engine, action) {
            return Reaction::Exit;
        }
        if !has_more {
            break;
        }
    }

    let epilogue = render::post_key_epilogue(engine, Some(state.quickfix_scroll_top));
    if epilogue.focus_sidebar {
        sidebar.has_focus = true;
    }
    // Sync the unnamed register → system clipboard (`clipboard=unnamedplus`).
    sync_tui_clipboard(engine, state.last_clipboard_content);
    if epilogue.arm_yank_highlight {
        state
            .yank_hl_deadline
            .set(Some(Instant::now() + Duration::from_millis(200)));
    }

    Reaction::Redraw
}

/// TUI's counterpart to GTK's `App::dispatch_engine_action`: apply the
/// [`EngineAction`] the general keyboard fallback produced. Returns `true`
/// when the app should exit.
///
/// The eight named arms are the ones whose effect needs TUI-only state — the
/// terminal panel's column/row geometry, the `quadraui::FolderPickerController`
/// (#815; GTK's `App` carries the identical field), and `TuiSidebar`.
/// Everything else falls through to `handle_action`.
#[allow(clippy::too_many_arguments)]
fn dispatch_post_key_action(
    action: EngineAction,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    folder_picker: &mut Option<quadraui::FolderPickerController>,
    screen_w: u16,
    screen_h: u16,
    sidebar_width: u16,
) -> bool {
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
                quadraui::AcceleratorId::new(render::ACC_TERMINAL_TOGGLE_MAX),
                quadraui::Modifiers::default(),
            ),
            ctx,
        );
    } else if let EngineAction::RunInTerminal(cmd) = &action {
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_run_command(cmd, screen_w, rows);
    } else if action == EngineAction::OpenFolderDialog {
        *folder_picker = Some(new_folder_picker_controller(engine));
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
        return true;
    }
    false
}

/// Apply a resolved [`render::ModalKeyRoute`] on the TUI side.
///
/// The decision — spell suggestions → modal dialog → context menu — is shared
/// (`render::route_modal_key`); this is the `crossterm`-flavoured application
/// of it. Both arms consume the key unconditionally: that is what makes the
/// tier genuinely *modal* rather than a best-effort intercept.
fn apply_modal_key_route(
    route: render::ModalKeyRoute,
    key_event: KeyEvent,
    keyboard_enhanced: bool,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    screen_w: u16,
    screen_h: u16,
) -> Reaction {
    match route {
        render::ModalKeyRoute::Engine => {
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
            Reaction::Redraw
        }
        render::ModalKeyRoute::ContextMenu => {
            if key_event.kind == KeyEventKind::Release {
                return Reaction::Continue;
            }
            // `handle_context_menu_key` consumes every key while the menu is
            // open (its `_` arm closes it), so an untranslatable key is
            // swallowed rather than falling through to the tier below.
            if let Some((key_name, unicode, _ctrl)) = translate_key(key_event, keyboard_enhanced) {
                let effective_key = if key_name.is_empty() {
                    unicode.map(|c| c.to_string()).unwrap_or_default()
                } else {
                    key_name
                };
                let ctx = engine.context_menu_target_path();
                let (_consumed, action) = engine.handle_context_menu_key(&effective_key);
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
            }
            Reaction::Redraw
        }
        render::ModalKeyRoute::None => Reaction::Continue,
    }
}

/// Build a `quadraui::FolderPickerController` rooted at the engine's current
/// working directory, surfacing `.vimcode-workspace` marker files the same
/// way the old TUI-local `FolderPickerState` did. GTK's `App::open_folder_
/// dialog` builds the identical controller — see `app.rs`.
fn new_folder_picker_controller(engine: &Engine) -> quadraui::FolderPickerController {
    quadraui::FolderPickerController::new(
        engine.cwd.clone(),
        vec![".vimcode-workspace".to_string()],
        engine.settings.show_hidden_files,
    )
}

/// Drive an open folder picker with one raw `UiEvent` and apply the result.
/// The decision (key→intent, filesystem walk, filtering, scroll) is entirely
/// `quadraui::FolderPickerController`'s own (#815); this only applies the
/// `Confirmed`/`Cancelled` outcomes to TUI-local state (`folder_picker`,
/// `sidebar`) and the engine. GTK's `handle_key_press` calls the identical
/// controller method with its own popup rect — see `app.rs`.
fn apply_folder_picker_event(
    folder_picker: &mut Option<quadraui::FolderPickerController>,
    event: &quadraui::UiEvent,
    engine: &mut Engine,
    sidebar: &mut TuiSidebar,
    screen_w: u16,
    screen_h: u16,
) {
    let Some(picker) = folder_picker.as_mut() else {
        return;
    };
    let viewport = quadraui::Rect::new(0.0, 0.0, screen_w as f32, screen_h as f32);
    let popup_rect = render::folder_picker_popup_rect(viewport, 1.0);
    let visible_rows = render::folder_picker_visible_rows(popup_rect, 1.0);
    match picker.handle(event, visible_rows) {
        quadraui::FolderPickerEvent::Confirmed { path } => {
            *folder_picker = None;
            engine.open_folder(&path);
            *sidebar = TuiSidebar::new();
            engine.explorer_rebuild_rows();
            if let Some(path) = engine.file_path().cloned() {
                engine.explorer_reveal_path(&path);
            }
        }
        quadraui::FolderPickerEvent::Cancelled => *folder_picker = None,
        quadraui::FolderPickerEvent::Consumed | quadraui::FolderPickerEvent::Ignored => {}
    }
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
    /// a test can seed or assert on (`scratch.sidebar_width`) any of them.
    /// `cmd_sel` lives on `Engine` now (#816), not here — seed/assert via
    /// `engine.cmd_sel` directly in tests that need it.
    struct KeyScratch {
        sidebar_width: u16,
        quickfix_scroll_top: usize,
        last_clipboard_content: Option<String>,
        yank_hl_deadline: Cell<Option<Instant>>,
        /// Read by the debug/DAP sidebar tier (inert for any non-key event,
        /// and no direct test exercises that panel today) *and* the
        /// folder-picker rung (#815), which needs this to actually be the
        /// `UiEvent::KeyPressed` the `key`/`modifiers`/`repeat` arguments to
        /// `handle_key_pressed` were decoded from — a test that drives the
        /// picker must call [`Self::set_key`] first, matching what
        /// `TuiShellApp::handle`'s real `dap_event` construction does.
        ui_event: UiEvent,
    }

    impl KeyScratch {
        fn new() -> Self {
            Self {
                sidebar_width: SIDEBAR_WIDTH,
                quickfix_scroll_top: 0,
                last_clipboard_content: None,
                yank_hl_deadline: Cell::new(None),
                ui_event: UiEvent::WindowFocused(true),
            }
        }

        /// Set `ui_event` to the `UiEvent::KeyPressed` a test is about to
        /// feed `handle_key_pressed` as `key`/`modifiers`/`repeat` — see the
        /// field's doc comment for why this matters to the folder-picker
        /// rung specifically.
        fn set_key(&mut self, key: quadraui::Key, modifiers: quadraui::Modifiers, repeat: bool) {
            self.ui_event = UiEvent::KeyPressed {
                key,
                modifiers,
                repeat,
            };
        }

        fn state(&mut self) -> KeyDispatchState<'_> {
            KeyDispatchState {
                sidebar_width: &mut self.sidebar_width,
                quickfix_scroll_top: &mut self.quickfix_scroll_top,
                last_clipboard_content: &mut self.last_clipboard_content,
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

    /// #754: re-activating a plugin panel that's already the active one
    /// (but whose sidebar was hidden through some *other* path — e.g. a
    /// global sidebar-toggle key — without clearing `ext_panel_active`)
    /// must not reset its scroll position or re-fire the plugin's
    /// `panel_focus` hook. A second `PanelChanged` for the *visible*
    /// already-active panel is the toggle-closed case, covered by
    /// `on_shell_event_sidebar_hidden_clears_extension_panel_state` above
    /// (`AppShell` reports that specific case as `SidebarHidden`, not a
    /// repeat `PanelChanged`, but `apply_activity_panel_switch` handles it
    /// the same way if one ever arrived) — this test is the other of
    /// `apply_activity_panel_switch`'s two branches: `already_showing` but
    /// *not* currently visible.
    ///
    /// `activate_ext_panel`'s pre-#754 body was TUI-only and unconditional
    /// — every call, including one for a panel that's already active, reset
    /// `ext_panel_selected` to `0` and called `plugin_event("panel_focus",
    /// …)` again. `render::apply_activity_panel_switch`'s doc comment calls
    /// this out by name: "the re-entry guard was GTK-only" — GTK's
    /// `App::switch_panel` always routed through the shared function, so it
    /// never had this bug; TUI's real activity-bar entry point
    /// (`on_shell_event`'s `PanelChanged` arm, which every activity-bar
    /// click drives — see `driver_click_on_extension_icon_opens_the_plugin_
    /// panel`, above) called this method directly and did not. Verified RED
    /// against the pre-#754 `activate_ext_panel` (reinstating its old
    /// unconditional body resets `ext_panel_selected` back to `0` here).
    #[allow(deprecated)]
    #[test]
    fn reactivating_the_open_plugin_panel_does_not_reset_its_scroll_position() {
        let mut app = app_with_ext_panel();
        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new("ext:git-insights"),
        });
        assert_eq!(app.engine.ext_panel_active.as_deref(), Some("git-insights"));

        // Simulate the user having scrolled the panel's list, then hidden
        // the sidebar via some path that leaves `ext_panel_active` alone
        // (e.g. a global sidebar-toggle key) rather than the icon's own
        // "close" click (which clears it — see
        // `on_shell_event_sidebar_hidden_clears_extension_panel_state`).
        app.engine.ext_panel_selected = 5;
        app.engine.app_shell.hide_sidebar();
        assert!(!app.engine.app_shell.sidebar_visible());

        app.on_shell_event(&quadraui::AppShellEvent::PanelChanged {
            panel_id: quadraui::WidgetId::new("ext:git-insights"),
        });

        assert!(
            app.engine.app_shell.sidebar_visible(),
            "re-activating the icon must re-show the sidebar"
        );
        assert_eq!(
            app.engine.ext_panel_selected, 5,
            "re-activating the panel that was already active must not reset \
             its selection/scroll back to the top (#754 — \
             apply_activity_panel_switch's re-entry guard, previously \
             GTK-only)"
        );
        assert_eq!(
            app.engine.ext_panel_active.as_deref(),
            Some("git-insights"),
            "and it must still be the active plugin panel"
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

    /// Terminal column of the group divider glyph in one painted row,
    /// scanning only to the right of `after` so the sidebar's own tree-indent
    /// guides (a different screen region entirely) cannot be mistaken for it.
    /// Shared by the divider-drag tests below.
    ///
    /// Takes `TuiDriver::styled_row`'s per-*cell* vec rather than a
    /// `screen()` line: `screen()` emits one `char` per wide glyph, so a
    /// string index into it undercounts columns wherever the explorer's
    /// two-cell icons were painted, and the resulting column would not agree
    /// with the coordinates `find()` and `mouse_down()` speak.
    fn divider_col_on_row<S>(cells: &[(char, S)], after: usize) -> Option<usize> {
        cells
            .iter()
            .enumerate()
            .skip(after)
            .find(|(_, (c, _))| *c == '\u{2502}')
            .map(|(i, _)| i)
    }

    /// #753 (mouse ladder slice 3), TUI half: dragging the editor-group
    /// divider must actually move it, end to end through the real
    /// `driver_with_shell` pipeline (`TestBackend` -> `ShellAdapter::handle`
    /// -> `TuiShellApp::handle` -> `mouse::handle_mouse` -> the shared
    /// `render::route_divider_grab` / `render::apply_divider_drag`).
    ///
    /// Asserts on **rendered output** (`CLAUDE.md` rule 1): the painted
    /// `\u{2502}` column before the gesture versus after it. Asserting that
    /// `divider_grab` became `Some` would pass against a router that arms the
    /// grab and then never applies it — which is exactly the half of the rung
    /// this slice moved into shared code.
    ///
    /// The vertical-split fixture matches
    /// `render_content_paints_group_divider_via_shell_app` above (short
    /// content, so neither pane overflows and the #481 scrollbar-as-separator
    /// guard cannot mask the glyph).
    #[test]
    fn group_divider_drag_moves_the_painted_divider_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "short\n");
        app.engine.open_editor_group(SplitDirection::Vertical);
        let mut driver = driver_with_shell(app, config(), 80, 24);
        // The first painted frame is not the settled layout: `handle_mouse`
        // returns the sidebar width the app then adopts, so the sidebar (and
        // with it the editor origin, and with it the divider column) only
        // reaches its steady state after one dispatched mouse event. Settle
        // with a no-op release before measuring the "before" column, so the
        // two measurements below are taken in the same regime.
        driver.mouse_up(1.0, 1.0);

        // Derive the divider's painted column rather than hard-coding it —
        // `AppShell`'s activity-bar/sidebar reservation owns the origin.
        let (tab_x, _) = driver
            .find("[No Name]")
            .expect("each pane paints its own tab label");
        let after = tab_x as usize;
        let row = 5_usize;
        let before = divider_col_on_row(&driver.styled_row(row as u16), after)
            .expect("the vertical group split must paint a divider glyph");

        // Grab the divider and drag it 8 cells left.
        let target = before - 8;
        driver.mouse_down(before as f32, row as f32);
        driver.mouse_move(target as f32, row as f32);
        driver.mouse_up(target as f32, row as f32);

        let moved = divider_col_on_row(&driver.styled_row(row as u16), after)
            .expect("the divider must still be painted after the drag");
        let screen = driver.screen();
        assert!(
            moved < before,
            "dragging the group divider from col {before} to col {target} must \
             repaint it further left, but it stayed at col {moved}; screen:\n{screen}"
        );
        assert!(
            moved.abs_diff(target) <= 1,
            "the repainted divider (col {moved}) should track the drag column \
             ({target}) within a cell of rounding; screen:\n{screen}"
        );
    }

    /// #753, TUI half: the *release* end of the same rung. A left-press on the
    /// divider followed by a mouse-up **without** an intervening move must
    /// leave the divider exactly where it was — the grab is armed but nothing
    /// is applied, so a plain click on a split boundary is not a silent resize.
    ///
    /// This is the arm-without-apply case `route_divider_grab` and
    /// `apply_divider_drag` are deliberately split across; a router that
    /// applied on press would move the divider here.
    #[test]
    fn group_divider_click_without_move_leaves_the_divider_put_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "short\n");
        app.engine.open_editor_group(SplitDirection::Vertical);
        let mut driver = driver_with_shell(app, config(), 80, 24);
        // See the sibling test for why one dispatched no-op event is needed
        // before the layout is settled enough to measure.
        driver.mouse_up(1.0, 1.0);

        let (tab_x, _) = driver
            .find("[No Name]")
            .expect("each pane paints its own tab label");
        let after = tab_x as usize;
        let row = 5_usize;
        let before = divider_col_on_row(&driver.styled_row(row as u16), after)
            .expect("the vertical group split must paint a divider glyph");

        driver.mouse_down(before as f32, row as f32);
        driver.mouse_up(before as f32, row as f32);

        let after_cells = driver.styled_row(row as u16);
        let screen = driver.screen();
        assert_eq!(
            divider_col_on_row(&after_cells, after),
            Some(before),
            "a press-and-release on the divider with no drag must not move it; \
             screen:\n{screen}"
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

    /// #753 fix iteration 1: the tab-drag rung (`render::TabDragState`,
    /// shared by both backends via `mouse.rs`'s single-group tab-bar arm
    /// and GTK's `handle_mouse_drag_msg`) had black-box coverage on GTK
    /// (`tab_drag_past_a_neighbour_reorders_the_painted_tab_bar` in
    /// `src/gtk/testing.rs`) but none on TUI — the sibling test just above
    /// only proves the drag *ghost overlay* paints, never that a drop
    /// actually reorders the tab bar. This is the TUI twin of the GTK test:
    /// drive a real press → move → move → release through `TuiDriver` on
    /// the unsplit tab bar and assert the painted tab labels actually swap
    /// order, i.e. that `TabDragState::handle_release` ->
    /// `Engine::apply_tab_drop_zone` actually ran through `mouse.rs`'s
    /// `Crossed`/`Tracking` arms (which — unlike GTK's — trust the press
    /// point rather than re-resolving it, see those arms' comments).
    ///
    /// Confirmed RED against unfixed `develop` (pre-#753's own hand-rolled
    /// `tab_drag_start`/`tab_dragging` machinery): reverting this file's
    /// `tab_drag` field back to a manual `Option<(f64, f64)>` drag-start (no
    /// `handle_release` commit wired to a release with no drag arm) leaves
    /// this test asserting `left_after.x > right_after.x` against tabs that
    /// never moved — it fails.
    ///
    /// Two moves, not one, for the same reason the ghost-overlay test above
    /// and the GTK twin both need it: `TabDragState::handle_move`'s first
    /// call after `arm` only crosses the threshold (`TabDragMove::Crossed`
    /// -> `begin`); the second is the first one actually `track`ed into a
    /// drop zone. A single move would release with `DropZone::None` and
    /// drop nothing — the same sequencing the live backend has always had.
    #[test]
    fn tui_tab_drag_past_a_neighbour_reorders_the_painted_tab_bar() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_753_tui_tab_drag_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("zqa753.txt");
        let b = dir.join("zqb753.txt");
        std::fs::write(&a, "a\n").unwrap();
        std::fs::write(&b, "b\n").unwrap();

        let mut app = TuiShellApp::new(None);
        app.engine.new_tab(Some(&a));
        app.engine.new_tab(Some(&b));
        assert_eq!(
            app.engine.group_layout.leaf_count(),
            1,
            "this test covers the unsplit single-group tab bar arm"
        );
        let mut driver = driver_with_shell(app, config(), 100, 24);

        // `find_bounds` scans rows top-down and the tab bar always paints on
        // row 0 (see `render_content_paints_single_group_tab_bar_via_shell_app`
        // above), strictly above any breadcrumb/status-bar repaint of the
        // same file name further down — so the first match is always the
        // tab label, no trailing-space disambiguation needed (unlike the GTK
        // twin, which has no such row ordering to rely on).
        let left_before = driver
            .find_bounds("zqa753.txt")
            .expect("tab a should be painted on the tab bar");
        let right_before = driver
            .find_bounds("zqb753.txt")
            .expect("tab b should be painted on the tab bar");
        assert!(
            left_before.x < right_before.x,
            "new_tab appends, so a's tab should paint left of b's; a={left_before:?} b={right_before:?}"
        );

        let from = (
            left_before.x + left_before.width / 2.0,
            left_before.y + left_before.height / 2.0,
        );
        let to = (
            right_before.x + right_before.width / 2.0,
            right_before.y + right_before.height / 2.0,
        );
        driver.mouse_down(from.0, from.1);
        driver.mouse_move(to.0, to.1);
        driver.mouse_move(to.0, to.1);
        driver.mouse_up(to.0, to.1);

        let left_after = driver
            .find_bounds("zqa753.txt")
            .expect("tab a should still be painted after the drop");
        let right_after = driver
            .find_bounds("zqb753.txt")
            .expect("tab b should still be painted after the drop");
        assert!(
            left_after.x > right_after.x,
            "dragging a onto b must repaint it to the right of b \
             (was {} < {}, now {} vs {})",
            left_before.x,
            right_before.x,
            left_after.x,
            right_after.x
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #756 acceptance, TUI half of the drag rung: pressing and holding on the
    /// minimap and dragging must keep seeking — the GTK twin of this
    /// (`minimap_drag_keeps_seeking_while_the_button_is_held` in
    /// `src/gtk/testing.rs`) has passed since #35, and this backend had no
    /// equivalent behaviour at all.
    ///
    /// **RED against unfixed `develop`.** `mouse.rs`'s minimap arm matched
    /// `Down(Left) | Drag(Left)`, but it sits below an
    /// `if ev.kind != Down(Left) { return }` gate, so the `Drag` half was
    /// unreachable: a press seeked once and every subsequent held move fell
    /// through to the text-selection arm. Reverting `handle_mouse`'s
    /// `MouseDragRoute::Minimap` arm leaves the second assertion below
    /// comparing two identical screens — it fails.
    ///
    /// Asserted on the *painted* buffer lines (`CLAUDE.md` testing rule 1), not
    /// on `scroll_top`: the lowest `line N content` marker visible on screen
    /// must move further down the file after the held move than it did after
    /// the press alone.
    #[test]
    fn tui_minimap_drag_keeps_seeking_while_the_button_is_held() {
        /// Lowest `line N content` marker painted on screen — i.e. the first
        /// buffer line the viewport is showing.
        fn top_painted_line(screen: &str) -> Option<usize> {
            screen
                .match_indices("line ")
                .filter_map(|(i, _)| {
                    let rest = &screen[i + "line ".len()..];
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if digits.is_empty() || !rest[digits.len()..].starts_with(" content") {
                        return None;
                    }
                    digits.parse::<usize>().ok()
                })
                .min()
        }

        let mut app = TuiShellApp::new(None);
        let mut text = String::new();
        for i in 0..400 {
            text.push_str(&format!("line {i} content\n"));
        }
        app.engine.buffer_mut().insert(0, &text);
        assert!(
            app.engine.settings.minimap,
            "setup sanity: the minimap must default on, or this test proves nothing"
        );
        let mut driver = driver_with_shell(app, config(), 100, 24);

        // `minimap_reserved_width` gives a 100-column pane a 15-column strip
        // (`min(MINIMAP_TARGET_COLS, 100 * MINIMAP_WIDTH_FRACTION)`, clamped
        // into `[MINIMAP_MIN_COLS, MINIMAP_MAX_COLS]`), painted flush against
        // the pane's right edge — so column 97 is inside the strip with room
        // to spare on either side of the exact width.
        let strip_col = 97.0;

        driver.mouse_down(strip_col, 6.0);
        let after_down = top_painted_line(&driver.screen())
            .expect("the editor must paint `line N content` markers after the press");

        driver.mouse_move(strip_col, 17.0);
        let after_drag = top_painted_line(&driver.screen())
            .expect("the editor must still paint `line N content` markers after the drag");

        assert!(
            after_drag > after_down,
            "a held drag further down the minimap must scroll further into the \
             file, but the painted viewport still starts at line {after_down} \
             (press) vs {after_drag} (drag); screen:\n{}",
            driver.screen()
        );
    }

    /// #756 review fix: click-then-drag text selection was broken on TUI.
    /// TUI's own editor-click path (#565) arms the *same* shared
    /// `quadraui::DragState` this rung's `ArmedTarget` route checks first —
    /// with `DragTarget::TextSelection`, so `armed_target: drag_state.is_active()`
    /// made every text-selection drag resolve to `ArmedTarget` (whose handler,
    /// `apply_scrollbar_drag`, only reacts to `ScrollOffsetChanged` and drops
    /// `TextSelectionChanged` on the floor), leaving the `EditorText` rung —
    /// the only place that calls `Engine::mouse_drag` to actually extend the
    /// selection — unreachable for the whole gesture. The GTK twin of this
    /// (`an_editor_text_drag_paints_a_selection_through_the_shared_drag_router`
    /// in `src/gtk/testing.rs`) already covered GTK, which never arms
    /// `TextSelection` on its `DragState`; this is the missing TUI half.
    ///
    /// **RED against the unfixed router** (`armed_target: drag_state.is_active()`
    /// instead of `render::drag_state_arms_scrollbar(drag_state)`): the click
    /// arms `TextSelection` at mouse-down, so the `Drag` event right after it
    /// resolves to `ArmedTarget`, `apply_scrollbar_drag` finds no
    /// `ScrollOffsetChanged` to apply, and the probed cell's style never
    /// changes — `before == after`, and this test fails.
    ///
    /// Same probe-a-swept-cell's-*style* technique as the GTK twin (`CLAUDE.md`
    /// testing rule 1: assert on rendered output, not on state).
    ///
    /// # Why this test is built the way it is
    ///
    /// It measures painted *editor geometry*, which puts it squarely in the
    /// #615/#634 blast radius, and the first two attempts at it were red on
    /// other machines while green here. Three separate ambient inputs had to
    /// go before it was reproducible anywhere:
    ///
    /// 1. **Ambient engine state.** `TuiShellApp::new(None)` runs the real
    ///    `Engine::new()` (reads `~/.config/vimcode/{settings,session}.json`,
    ///    and hides the sidebar unless that session says otherwise) *and*
    ///    `Engine::startup(None)` → `restore_session_files()` (reopens the
    ///    developer's own files and splits). Sidebar visible vs hidden alone
    ///    moves every editor column by `SIDEBAR_WIDTH`; a restored split moves
    ///    the pane outright. [`TuiShellApp::new_for_test`] is the fix — see
    ///    its doc comment.
    /// 2. **The sidebar-width settle.** `driver_with_shell` paints frame 1
    ///    straight from the [`config`] helper, which leaves quadraui's generic
    ///    20-column `default_sidebar_width` in place rather than mirroring
    ///    `TuiShellApp::shell_config`'s #634 clamp to `SIDEBAR_WIDTH`. The
    ///    end-of-dispatch `set_sidebar_width(self.sidebar_width)` sync in
    ///    `handle()` re-widens it on the first event of *any* kind, so a
    ///    column measured off frame 1 is stale from frame 2 onwards. The
    ///    `Escape` below settles it before anything is measured.
    /// 3. **Wall-clock double-click detection.** `TuiDriver::click` is a bare
    ///    `MouseDown` (no release), and `TuiDriver::dispatch` routes every
    ///    injected event through `TuiBackend::translate_injected`, which folds
    ///    a second close `MouseDown` into `UiEvent::DoubleClick` inside its
    ///    `400ms`/1.5-cell `DoubleClickDetector` (`quadraui::dispatch`).
    ///    `mouse.rs`'s editor arm (#817) just reads that fold's verdict —
    ///    it no longer runs a second, independent timer of its own — but the
    ///    detector itself is still real wall-clock state: parking the cursor
    ///    and then pressing on that same cell would still race real time,
    ///    fast machine → word-select-then-extend, loaded machine → two plain
    ///    clicks. The park click below is deliberately one cell left of the
    ///    drag press so the detector's position check misses and no
    ///    `DoubleClick` fold happens regardless of how long the two
    ///    dispatches take (the same "don't race the 400ms detector" lesson
    ///    quadraui#592 baked into `TuiDriver::double_click`).
    ///
    /// The assertion sweeps the whole dragged span rather than one hardcoded
    /// probe column, so it states the property under test ("some cell the drag
    /// swept changed how it paints") instead of a guess about which cell the
    /// selection lands on.
    #[test]
    fn tui_editor_text_drag_paints_a_selection_through_the_shared_drag_router() {
        // (1) Deterministic engine state — no ambient settings/session.
        let mut app = TuiShellApp::new_for_test();
        let mut text = String::new();
        for i in 0..40 {
            text.push_str(&format!("line {i} content that is reasonably long\n"));
        }
        app.engine.buffer_mut().insert(0, &text);
        assert_eq!(
            app.engine.windows.len(),
            1,
            "setup sanity: this test measures editor-pane geometry, so it needs \
             exactly one unsplit window — `new_for_test` must not have restored \
             an ambient session's splits"
        );
        let mut driver = driver_with_shell(app, config(), 100, 24);

        // (2) Settle the sidebar width before measuring anything.
        driver.press_named(quadraui::NamedKey::Escape);

        let bounds = driver
            .find_bounds("line 5 content")
            .expect("the fixture line should be painted");
        let row = bounds.y as u16;
        let row_y = bounds.y + bounds.height / 2.0;
        let park_x = bounds.x;
        let start_x = bounds.x + 1.0;
        let end_x = bounds.x + 12.0;
        let swept: Vec<u16> = (start_x as u16 + 1..=end_x as u16).collect();
        assert!(
            !swept.is_empty(),
            "setup sanity: the drag must sweep at least one cell"
        );

        // (3) Park the cursor on the row first — one cell *left* of the drag
        // press, so the press below can never be promoted to a double-click.
        // The "before" sample then already includes any cursor-line highlight,
        // leaving the selection as the only thing the gesture can change.
        driver.click(park_x, row_y);
        let before: Vec<_> = swept.iter().map(|&x| driver.style_at(x, row)).collect();

        // Press to the left of the swept span and drag across it while held —
        // the press arms `DragTarget::TextSelection` on the shared
        // `DragState`, and the very next `Drag` event is the one the
        // regression swallowed.
        driver.mouse_down(start_x, row_y);
        driver.mouse_move(end_x, row_y);
        driver.mouse_up(end_x, row_y);

        let after: Vec<_> = swept.iter().map(|&x| driver.style_at(x, row)).collect();
        assert_ne!(
            before,
            after,
            "a held drag across the editor text must repaint at least one \
             swept cell with the selection style, but columns {:?} of row \
             {row} paint identically before and after the drag ({before:?}); \
             screen:\n{}",
            swept,
            driver.screen()
        );
    }

    /// #817 regression coverage: a genuine double-click must still select
    /// the *whole clicked word* once `handle_mouse_event`/`mouse::handle_mouse`
    /// stop running their own hand-rolled 400ms/position detector and instead
    /// read the verdict `TuiBackend`'s `quadraui::DoubleClickDetector`
    /// already folded into `UiEvent::DoubleClick`. Uses
    /// `TuiDriver::double_click` (quadraui#592) rather than two `click()`s,
    /// so the assertion is deterministic instead of racing real time.
    ///
    /// Probes two columns *inside the same word* (`probe_a` near its start,
    /// `probe_b` near its end) rather than one: a lone repainted cursor glyph
    /// — what a regression back to "fold every `DoubleClick` into a plain
    /// `MouseDown` and call `engine.mouse_click` instead of
    /// `mouse_double_click`" would still produce, since a plain click also
    /// repaints the one cell the cursor lands on — only ever touches
    /// `probe_a` (where the click landed). A real word *selection* paints
    /// every cell of the word, including `probe_b`, with the same highlight.
    /// Asserting `after_a == after_b` (and that both differ from their
    /// unselected "before" style) is what actually distinguishes "selected
    /// the word" from "moved the cursor to where the click landed" — an
    /// earlier draft of this test asserted only the single-probe version and
    /// stayed green with `is_double` hardcoded to `false` (verified by
    /// temporarily reintroducing that regression), i.e. it didn't fail
    /// against the bug it claims to cover. The two-probe design below does
    /// fail against that regression (see `#817`'s PR for the before/after run).
    ///
    /// The second word — never clicked — pins down that the change is
    /// scoped to the double-clicked word rather than some unrelated
    /// full-line repaint; the cursor is parked off in blank space first
    /// (rather than on the second word) so its own cursor-glyph styling
    /// doesn't leak into that "before" snapshot.
    #[test]
    fn tui_editor_double_click_selects_the_word_via_shared_dispatch() {
        let mut app = TuiShellApp::new_for_test();
        app.engine
            .buffer_mut()
            .insert(0, "wordZQXW817alpha tailZQXW817beta\n");
        assert_eq!(
            app.engine.windows.len(),
            1,
            "setup sanity: this test measures editor-pane geometry, so it needs \
             exactly one unsplit window — `new_for_test` must not have restored \
             an ambient session's splits"
        );
        let mut driver = driver_with_shell(app, config(), 100, 24);

        // Settle the sidebar width before measuring anything (see the drag
        // test above for why this matters).
        driver.press_named(quadraui::NamedKey::Escape);

        let first = driver
            .find_bounds("wordZQXW817alpha")
            .expect("the first word should be painted");
        let second = driver
            .find_bounds("tailZQXW817beta")
            .expect("the second word should be painted");
        let row = first.y as u16;
        assert_eq!(row, second.y as u16, "setup sanity: both words on one row");
        let row_y = first.y + first.height / 2.0;
        // Two probes inside the *first* word — near its start (where the
        // double-click itself lands) and near its end — plus one inside the
        // untouched second word.
        let probe_a = first.x as u16 + 2;
        let probe_b = first.x as u16 + first.width as u16 - 2;
        assert!(
            probe_b > probe_a + 1,
            "setup sanity: the word must be wide enough for two distinct probes"
        );
        let second_probe = second.x as u16 + 2;
        // Well past both words, same row — blank space the cursor can park
        // on without its own cursor-glyph styling landing on any probe
        // column (moving the plain cursor itself repaints whatever cell it
        // was on, independent of any selection, so parking *on* a probe
        // column would make that column's "before" snapshot include a
        // cursor glyph the double-click's cursor move alone would clear —
        // a false positive for "the untouched word's paint changed").
        let park_x = second.x + second.width + 3.0;

        // Park the cursor off in blank space first (single click — must
        // never be promoted to a double-click here) so the "before"
        // snapshot is the words' plain, unselected paint, and `mouse_up`
        // releases it. `TuiDriver::click` is a bare `MouseDown` (no
        // release), and mouse.rs's own scroll-surface rung
        // (`quadraui::dispatch_click` over `engine.scroll_surfaces`) still
        // has a registered-but-invisible `"explorer:sb"` surface at a small
        // column even with the sidebar hidden; if the park click's
        // `TextSelection` drag is left active, a later click landing on that
        // column gets misrouted as a scrollbar-drag continuation and
        // swallowed before it ever reaches the editor's double-click arm.
        // Not this test's bug to fix, just a trap it must not fall into.
        driver.click(park_x, row_y);
        driver.mouse_up(park_x, row_y);
        let before_a = driver.style_at(probe_a, row);
        let before_b = driver.style_at(probe_b, row);
        let before_second = driver.style_at(second_probe, row);

        driver.double_click(probe_a as f32, row_y);

        let after_a = driver.style_at(probe_a, row);
        let after_b = driver.style_at(probe_b, row);
        let after_second = driver.style_at(second_probe, row);

        assert_ne!(
            before_a,
            after_a,
            "double-clicking the first word must repaint it with a selection \
             style, but column {probe_a} of row {row} paints identically \
             before and after ({after_a:?}); screen:\n{}",
            driver.screen()
        );
        assert_eq!(
            after_a,
            after_b,
            "a double-click must select the *whole* word, not just move the \
             cursor to the clicked cell — column {probe_a} (click site) and \
             column {probe_b} (same word, far end) must paint with the same \
             selection style, but got {after_a:?} vs {after_b:?}; screen:\n{}",
            driver.screen()
        );
        assert_ne!(
            before_b,
            after_b,
            "the far end of the double-clicked word must also pick up the \
             selection style, but column {probe_b} of row {row} paints \
             identically before and after ({after_b:?}); screen:\n{}",
            driver.screen()
        );
        assert_eq!(
            before_second,
            after_second,
            "double-clicking the first word must not touch the second word's \
             paint, but column {second_probe} of row {row} changed \
             ({before_second:?} -> {after_second:?}); screen:\n{}",
            driver.screen()
        );
    }

    /// #817 regression coverage, settings-sidebar site (`mouse.rs`'s
    /// `SidebarOwner::Settings` arm, one of the four hand-rolled
    /// 400ms/position detectors this issue deleted): a genuine double-click
    /// on a boolean settings row must toggle it via
    /// `engine.handle_settings_key("Return", ...)`, reading the same
    /// `is_double_click` verdict the editor-word-selection test above
    /// exercises. "Cursor Line" (`cursorline`) is a `SettingType::Bool` that
    /// defaults to `true` (`default_cursorline`), so it paints `"[x]"` —
    /// flipping to `"[ ]"` (quadraui's TUI `Form` renderer's literal
    /// checkbox glyphs) is the rendered-output assertion.
    ///
    /// The click's row is derived from `engine.settings_flat_list()` — the
    /// same list `mouse.rs`'s `SidebarOwner::Settings` arm indexes into via
    /// `fi = settings_scroll_top + content_row` — rather than from
    /// `find_bounds("Cursor Line")`'s painted position: the two disagree by
    /// one row in this fixture (a pre-existing mismatch between where
    /// `render_settings_panel` paints row N and where the click router's
    /// `content_row = sidebar_row - 2` resolves row N, unrelated to #817 —
    /// tracked separately). Computing the target row from the same list the
    /// click router consults keeps this test about the double-click verdict,
    /// not that separate off-by-one.
    #[test]
    fn tui_settings_double_click_toggles_a_boolean_row_via_shared_dispatch() {
        let mut app = TuiShellApp::new_for_test();
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_SETTINGS));
        assert!(
            app.engine.settings.cursorline,
            "setup sanity: `cursorline` must default to true so the test can \
             observe a true -> false double-click toggle"
        );
        let flat = app.engine.settings_flat_list();
        let flat_idx = flat
            .iter()
            .position(|row| {
                matches!(
                    row,
                    crate::core::engine::SettingsRow::CoreSetting(idx)
                        if crate::core::settings::SETTING_DEFS[*idx].key == "cursorline"
                )
            })
            .expect("the flat settings list must contain the `cursorline` row");
        // Mirrors `mouse.rs`'s `SidebarOwner::Settings` arm: `sidebar_row =
        // row - menu_rows` (menu bar hidden here, so `menu_rows == 0`), then
        // `content_row = sidebar_row - 2` (header + search rows), then
        // `fi = settings_scroll_top + content_row` (scrolled to the top).
        let row = flat_idx as u16 + 2;
        let col = ACTIVITY_BAR_WIDTH + 2;

        let mut driver = driver_with_shell(app, config(), 100, 24);

        let before = driver.screen();
        let before_line = before
            .lines()
            .find(|l| l.contains("Cursor Line"))
            .expect("the Cursor Line settings row should be painted");
        assert!(
            before_line.contains("[x]"),
            "Cursor Line must paint checked (\"[x]\") before any click, since \
             it defaults to true; line: {before_line:?}"
        );

        driver.double_click(col as f32, row as f32 + 0.5);

        let after = driver.screen();
        let after_line = after
            .lines()
            .find(|l| l.contains("Cursor Line"))
            .expect("the Cursor Line settings row should still be painted");
        assert!(
            after_line.contains("[ ]"),
            "double-clicking the Cursor Line row must toggle it to unchecked \
             (\"[ ]\") via `engine.handle_settings_key(\"Return\", ..)`, but \
             the checkbox glyph didn't flip; line: {after_line:?}; screen:\n{after}"
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

    // ── #758 / #734 slice 3: the shared terminal (PTY) keyboard rung ───────

    /// TUI half of `terminal_ctrl_f_opens_the_painted_find_bar` (`gtk/
    /// testing.rs`): with the terminal focused, Ctrl+F must open the
    /// *terminal's* find bar, and subsequent characters must land in that
    /// bar's query — all the way through `driver_with_shell` ->
    /// `TuiShellApp::handle` -> `handle_key_pressed` ->
    /// `render::route_terminal_key` -> `Engine::handle_terminal_key`.
    ///
    /// Asserts on rendered output (`CLAUDE.md` rule 1): the terminal
    /// toolbar's painted `" FIND: …"` text
    /// (`render::build_terminal_toolbar`), not `terminal_find_active`. A test
    /// on the flag would pass against a backend that flipped it while the
    /// tab strip still painted — the #587/#592 failure shape.
    ///
    /// **Verified RED against unfixed `develop`:** deleting the
    /// `render::route_terminal_key` call from `handle_key_pressed` makes
    /// Ctrl+F fall through to `Engine::handle_key`, which opens the
    /// *editor*'s find/replace overlay instead; the `"FIND:"` assertion
    /// fires. (This rung existed on TUI before the slice — it is GTK that
    /// had none — so the removed-fix control is the router call itself.)
    #[test]
    fn terminal_ctrl_f_opens_the_painted_find_bar_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        // `terminal_new_tab` opens the panel and focuses it.
        app.engine.terminal_new_tab(80, 10);

        let mut driver = driver_with_shell(app, config(), 80, 24);
        driver.render();
        assert!(
            !driver.screen_contains("FIND:"),
            "precondition: the terminal toolbar starts as a tab strip; screen:\n{}",
            driver.screen()
        );

        driver.ctrl_char('f');
        driver.render();
        assert!(
            driver.screen_contains("FIND:"),
            "Ctrl+F with the terminal focused must open the terminal find bar, \
             not the editor find/replace overlay; screen:\n{}",
            driver.screen()
        );

        driver.type_char('z');
        driver.render();
        assert!(
            driver.screen_contains("FIND: z"),
            "characters typed after Ctrl+F must reach the terminal find query \
             through the shared router; screen:\n{}",
            driver.screen()
        );
    }

    /// #800: `ctrl_f_action` is now mode-derived instead of an unconditional
    /// `"find"` constant — Vim mode (the default, nothing set in
    /// `settings.json`) resolves to Vim's traditional Ctrl+F page-down, not
    /// the find/replace overlay `EditorMode::Vscode` still gets. With the
    /// editor (not the terminal) focused, Ctrl+F must scroll the viewport.
    ///
    /// Asserts on rendered output per `CLAUDE.md`'s black-box rule: the
    /// top-of-file marker line scrolling out of the painted screen, and the
    /// find/replace panel's always-drawn `"Aa"` case-sensitivity toggle
    /// staying absent — not on `engine.ctrl_f_action`/`find_replace_open`
    /// state, which could stay right while nothing painted (the #587/#592
    /// failure shape).
    ///
    /// **Verified RED against unfixed `develop`:** before #800,
    /// `default_ctrl_f_action()` unconditionally returned `"find"`, so this
    /// same Ctrl+F press opened the find/replace overlay (the `"Aa"` toggle
    /// appears) instead of paging the viewport.
    #[test]
    fn ctrl_f_pages_down_the_viewport_in_default_vim_mode_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        assert_eq!(
            app.engine.settings.editor_mode,
            crate::core::settings::EditorMode::Vim,
            "precondition: nothing set editor_mode away from its Vim default"
        );
        let mut text = String::new();
        for i in 0..80 {
            text.push_str(&format!("ZQXW_LINE_{i:03}\n"));
        }
        app.engine.buffer_mut().insert(0, &text);

        let mut driver = driver_with_shell(app, config(), 80, 24);
        assert!(
            driver.screen_contains("ZQXW_LINE_000"),
            "precondition: the top of the file must be visible before Ctrl+F; screen:\n{}",
            driver.screen()
        );
        assert!(
            driver.find_bounds("Aa").is_none(),
            "precondition: the find/replace overlay must start closed; screen:\n{}",
            driver.screen()
        );

        driver.ctrl_char('f');
        driver.render();

        assert!(
            driver.find_bounds("Aa").is_none(),
            "default (Vim) mode Ctrl+F must page the viewport down, not open \
             the find/replace overlay; screen:\n{}",
            driver.screen()
        );
        assert!(
            !driver.screen_contains("ZQXW_LINE_000"),
            "Ctrl+F must scroll the top-of-file marker line off the rendered \
             viewport; screen:\n{}",
            driver.screen()
        );
    }

    /// #800 review fix: `completion_keys.accept` is mode-derived at the
    /// settings layer, but `handle_insert_key` didn't actually consult it —
    /// `<Tab>` unconditionally accepted the display-only completion popup
    /// regardless of `editor_mode`. This wires the Tab/accept-key check to
    /// `self.settings.completion_keys.accept(self.settings.editor_mode)` via
    /// `Engine::key_matches_binding`, so default (Vim) mode's `<C-y>` default
    /// accepts and `<Tab>` falls through to plain indentation — matching
    /// Vim's own `i_CTRL-Y` (accepts when the completion menu is visible,
    /// otherwise its unrelated "insert char from line above" binding).
    ///
    /// Builds one real word-completion popup per driver (via actual typing —
    /// not by poking `engine.completion_idx` directly) from a shared fixture,
    /// then presses the one key under test on each independent instance.
    ///
    /// Asserts on rendered output — how many times the dictionary word
    /// `"ZQXWFOOBAR"` appears on screen — never on internal completion state
    /// (the #587/#592 failure shape): before either key it appears twice
    /// (once in the dictionary line, once as the popup's own candidate
    /// label); the popup dismisses either way, so afterward the count is the
    /// only thing that can tell accept from no-accept — 1 (just the
    /// dictionary line) if `<Tab>` correctly did *not* accept, 2 (dictionary
    /// line + the newly completed second line) if `<C-y>` correctly did.
    ///
    /// **Verified RED against unfixed `develop`:** before this fix,
    /// `handle_insert_key`'s Tab branch checked `key_name == "Tab"`
    /// unconditionally, so `<Tab>` accepted the popup even in default Vim
    /// mode — the `after_tab` assertion below (expected count 1) observes 2
    /// instead against that code.
    #[test]
    fn ctrl_y_accepts_tab_falls_through_for_completion_popup_in_default_vim_mode_via_shell_app() {
        let build = || {
            let mut app = TuiShellApp::new(None);
            assert_eq!(
                app.engine.settings.editor_mode,
                crate::core::settings::EditorMode::Vim,
                "precondition: nothing set editor_mode away from its Vim default"
            );
            app.engine.buffer_mut().insert(0, "ZQXWFOOBAR\n");

            let mut driver = driver_with_shell(app, config(), 80, 24);
            driver.render();
            // Go to the last line, open a new line below, and type a prefix
            // that matches the dictionary word above — triggers the real
            // word-completion auto-popup (not hand-set engine state).
            driver.type_char('G');
            driver.type_char('o');
            for c in "ZQXWFOO".chars() {
                driver.type_char(c);
            }
            driver.render();
            driver
        };

        let mut tab_driver = build();
        let before = tab_driver.screen();
        assert_eq!(
            before.matches("ZQXWFOOBAR").count(),
            2,
            "precondition: the popup must be showing the \"ZQXWFOOBAR\" \
             candidate (once in the dictionary line, once in the popup) \
             before either key is pressed; screen:\n{before}"
        );

        tab_driver.press_named(quadraui::NamedKey::Tab);
        tab_driver.render();
        let after_tab = tab_driver.screen();
        assert_eq!(
            after_tab.matches("ZQXWFOOBAR").count(),
            1,
            "<Tab> must NOT accept the completion popup in default (Vim) \
             mode — only the dictionary line's occurrence should remain; \
             screen:\n{after_tab}"
        );

        let mut ctrl_y_driver = build();
        ctrl_y_driver.ctrl_char('y');
        ctrl_y_driver.render();
        let after_ctrl_y = ctrl_y_driver.screen();
        assert_eq!(
            after_ctrl_y.matches("ZQXWFOOBAR").count(),
            2,
            "<C-y> must accept the completion popup in default (Vim) mode, \
             completing the new line to \"ZQXWFOOBAR\" too; screen:\n{after_ctrl_y}"
        );
    }

    /// A focused terminal must swallow ordinary keys so they never reach the
    /// editor buffer — the divergence that made GTK unusable (there, `x` ran
    /// vim's delete-char on the file while the user thought they were typing
    /// into a shell). Stated once here for TUI so the pair is symmetric, and
    /// so the router's `false` return (the "no terminal focus" path) is
    /// covered too.
    ///
    /// Asserts on the painted buffer text with a positive control: clearing
    /// `terminal_has_focus` and repeating the *identical* key must delete the
    /// character, so a fixture whose text simply could not change would fail
    /// the second half.
    #[test]
    fn focused_terminal_swallows_editor_keys_via_shell_app() {
        // Same fixture twice, differing only in `terminal_has_focus`.
        let build = |focused: bool| {
            let mut app = TuiShellApp::new(None);
            app.engine.buffer_mut().insert(0, "ZQXWTERM758\n");
            app.engine.terminal_new_tab(80, 6);
            app.engine.terminal_has_focus = focused;
            driver_with_shell(app, config(), 80, 24)
        };

        let mut driver = build(true);
        driver.render();
        assert!(
            driver.screen_contains("ZQXWTERM758"),
            "precondition: the buffer line must paint; screen:\n{}",
            driver.screen()
        );

        driver.type_char('x');
        driver.render();
        assert!(
            driver.screen_contains("ZQXWTERM758"),
            "`x` with the terminal focused must go to the PTY, not delete a \
             character from the editor buffer; screen:\n{}",
            driver.screen()
        );

        // Positive control: the same key on the same fixture, terminal
        // unfocused, must edit — so a buffer that simply could not change
        // would fail here.
        let mut control = build(false);
        control.render();
        control.type_char('x');
        control.render();
        assert!(
            control.screen_contains("QXWTERM758") && !control.screen_contains("ZQXWTERM758"),
            "control: with the terminal unfocused `x` must delete the first \
             character; screen:\n{}",
            control.screen()
        );
    }

    /// The `Shift_`-prefixed names `translate_key` hands the editor
    /// (`Shift_Up`, `Shift_Return`, …) have no PTY encoding — which is why
    /// the old TUI arm bypassed `translate_key` and re-derived names from the
    /// raw crossterm `KeyCode`. `render::canonical_terminal_key_name` strips
    /// the prefix (shift travels as its own argument) and reconciles the two
    /// backends' spellings of the same physical keys, so the bypass is gone.
    #[test]
    fn canonical_terminal_key_name_reconciles_both_backends_spellings() {
        use crate::render::canonical_terminal_key_name as canon;
        // TUI's editor-facing shift prefix.
        assert_eq!(canon("Shift_Up"), "Up");
        assert_eq!(canon("Shift_Return"), "Return");
        // GTK's `NamedKey` spellings vs TUI's / X11's.
        assert_eq!(canon("PageUp"), "Page_Up");
        assert_eq!(canon("PageDown"), "Page_Down");
        assert_eq!(canon("BackTab"), "ISO_Left_Tab");
        assert_eq!(canon("Enter"), "Return");
        // Already-canonical names and bare characters pass through.
        assert_eq!(canon("Page_Up"), "Page_Up");
        assert_eq!(canon("ISO_Left_Tab"), "ISO_Left_Tab");
        assert_eq!(canon("F5"), "F5");
        assert_eq!(canon("a"), "a");
    }

    // ── #754 (mouse ladder slice 4: panels) ────────────────────────────────

    /// The bottom panel's shared tab strip must switch which panel is
    /// **painted**, end to end through `driver_with_shell` -> `TuiShellApp::
    /// handle` -> `mouse::handle_mouse` -> `render::route_bottom_panel_click`
    /// -> `render::apply_bottom_panel_route`.
    ///
    /// Asserts on rendered output (`CLAUDE.md` rule 1): the Debug Output
    /// marker line has to actually reach the screen. Asserting
    /// `bottom_panel_kind == DebugOutput` would pass against a router that
    /// flips the field while the painter still draws the terminal — the
    /// #587/#592 failure shape.
    ///
    /// Before #754 the `TabBar` zone was resolved by a bespoke arm here and a
    /// different one on GTK; it is now the one `BottomPanelRoute::TabBar` both
    /// call.
    #[test]
    fn bottom_panel_tab_strip_click_switches_the_painted_panel_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.terminal_new_tab(80, 10);
        app.engine
            .dap_output_lines
            .push("ZQXW_754_DEBUG_MARKER".to_string());

        let mut driver = driver_with_shell(app, config(), 80, 24);
        // Settle the sidebar-derived layout before measuring — see
        // `group_divider_drag_moves_the_painted_divider_via_shell_app`.
        driver.mouse_up(1.0, 1.0);
        driver.render();
        assert!(
            !driver.screen_contains("ZQXW_754_DEBUG_MARKER"),
            "precondition: the Terminal tab owns the panel body; screen:\n{}",
            driver.screen()
        );

        let (dx, dy) = driver
            .find("Debug Output")
            .expect("the bottom panel tab strip must paint a Debug Output tab");
        driver.click(dx, dy);
        driver.render();

        assert!(
            driver.screen_contains("ZQXW_754_DEBUG_MARKER"),
            "clicking the Debug Output tab must repaint the panel body with the \
             debug output (#754 `BottomPanelRoute::TabBar`); screen:\n{}",
            driver.screen()
        );
    }

    /// An **open but empty** quickfix list must reserve no rows for mouse
    /// routing, because it reserves none for painting.
    ///
    /// `compute_editor_layout` gates the quickfix band on `quickfix_open &&
    /// !quickfix_items.is_empty()`, but `handle_mouse` asked `if
    /// engine.quickfix_open { 6 }` in four places — so `:copen` on an empty
    /// list moved every band below the editor six rows up from where it was
    /// painted. `render::quickfix_panel_rows` is now the single rule.
    ///
    /// The discriminator needs no knowledge of the panel's height: with the
    /// old rule the terminal's right-click *suppression band* starts six rows
    /// above the painted terminal, so a right-click three rows **above** the
    /// painted tab strip — plainly in the editor — was silently swallowed and
    /// no editor context menu appeared. Asserts on rendered output: the menu's
    /// own painted item text.
    #[test]
    fn empty_quickfix_does_not_displace_the_terminal_band_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.terminal_new_tab(80, 8);
        // `:copen` with nothing in the list — open, but paints nothing.
        app.engine.quickfix_open = true;
        app.engine.quickfix_items.clear();

        let mut driver = driver_with_shell(app, config(), 80, 24);
        driver.mouse_up(1.0, 1.0);
        driver.render();

        let (_, strip_y) = driver
            .find("Terminal")
            .expect("the bottom panel tab strip must paint a Terminal tab");
        // Three rows above the painted strip: editor text, and inside the
        // six-row band the old rule wrongly attributed to the terminal.
        let target_y = strip_y - 3.0;
        assert!(
            target_y > 1.0,
            "fixture must leave editor rows above the terminal panel; screen:\n{}",
            driver.screen()
        );
        driver.right_click(40.0, target_y);
        driver.render();

        assert!(
            driver.screen_contains("Go to Definition"),
            "a right-click in the editor must open the editor context menu even \
             with an empty quickfix open — the terminal's suppression band must \
             not be displaced by rows nothing painted (#754); screen:\n{}",
            driver.screen()
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

    // ─── Overlay-band z-order (#735 slice 1) ─────────────────────────────

    /// A dialog both backends paint **in-canvas**.
    ///
    /// The `input` field is what forces that: `quadraui::native_dialog_options`
    /// returns `None` for a dialog carrying a text input, so GTK's
    /// `OverlayOp::Dialog` arm draws the generic primitive instead of queueing
    /// a real OS `AlertDialog` (#727). A plain button-only dialog would go
    /// native on GTK and never enter its band at all, which would make the
    /// cross-backend comparison compare two different things.
    ///
    /// `gtk/testing.rs`'s `in_canvas_dialog` is the byte-identical twin — keep
    /// them in step.
    fn in_canvas_dialog(title: &str) -> crate::core::engine::Dialog {
        crate::core::engine::Dialog {
            title: title.to_string(),
            body: vec!["body line".to_string()],
            buttons: vec![crate::core::engine::DialogButton {
                label: "OK".to_string(),
                hotkey: 'o',
                action: "ok".to_string(),
            }],
            selected: 0,
            tag: String::new(),
            input: Some(crate::core::engine::DialogInput {
                label: "Passphrase".to_string(),
                value: String::new(),
                is_password: true,
            }),
        }
    }
    //
    // These are the TUI half of the acceptance test. `gtk/testing.rs`'s
    // `mod frame_sequence` carries the GTK half, asserting against the
    // **same expected `Vec<FrameOp>`** for the same engine state. A single
    // test cannot drive both backends (the GTK `App` lives in the `vimcode` bin
    // target, `TuiShellApp` in `vcd`), so "both backends emit the same
    // sequence" is expressed as two tests with one expected value; keep them in
    // step.
    //
    // The fixtures below turn the menu bar *on* because GTK's menu bar is its
    // client-side titlebar and `App::setup` pins it visible unconditionally
    // (#552) — the one intrinsic difference between the two ladders. Turning it
    // on here is what makes the two expected bands literally identical rather
    // than "identical modulo two rungs".

    /// **#735's headline acceptance criterion, TUI half:** the whole frame,
    /// as one `FrameOp` sequence, must equal what the GTK twin
    /// (`frame_sequence_matches_across_backends_via_gtk_driver`) records for
    /// the same state.
    ///
    /// Nine rungs live, five absent — the chrome band, the title-bar band and a
    /// context menu under a modal dialog — so the assertion cannot degenerate
    /// into "whatever `FRAME_Z_ORDER` contains". Both halves read
    /// `render::frame_sequence_fixture()`, a single `#[cfg(test)]` fn compiled
    /// into both bin targets.
    ///
    /// **RED-verified against unfixed `develop`**: this test could not be
    /// written there at all — the chrome and overlay halves were two fields
    /// (`composed_chrome_band`, `painted_overlay_band`) with two order
    /// constants, so there was no single sequence to compare. With the fold in
    /// place, reordering *one rung on one backend* (hoisting `FrameOp::Dialog`'s
    /// arm body above `FrameOp::ContextMenu`'s, out of the `compose_frame`
    /// walk) makes this fail with `[.., Dialog, ContextMenu]` while the GTK
    /// twin still reads `[.., ContextMenu, Dialog]`, and trips
    /// `check_frame_order`'s `debug_assert` in `render_content` on the way.
    /// Re-introduced, observed red, restored before committing.
    #[test]
    fn frame_sequence_matches_across_backends_via_shell_app() {
        let mut app = app_with_sidebar_open();
        // The *settings* panel, not the explorer: its body paints fixed chrome,
        // where the explorer's would be this checkout's own directory listing —
        // ambient state a test must not depend on (#762).
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_SETTINGS));
        app.engine.menu_bar_visible = true;
        // Explicit, not ambient (#762): a global status bar exists only when
        // per-window status lines are off, and the default is on.
        app.engine.settings.window_status_line = false;
        app.engine.wildmenu_items = vec!["ZQXWwildA".to_string(), "ZQXWwildB".to_string()];
        app.engine.wildmenu_selected = Some(0);
        app.engine.open_editor_context_menu(4, 4);
        assert!(
            app.engine
                .context_menu
                .as_ref()
                .is_some_and(|m| !m.items.is_empty()),
            "fixture needs a non-empty context menu — an empty one is not composed"
        );
        app.engine.dialog = Some(in_canvas_dialog("ZQXW766DIALOG"));

        let frame = app.composed_frame.clone();
        // `shell_config(true)`, not `config()`: `AppShell::set_title_bar_visible`
        // is what reserves `layout.title_bar_bounds`, and with no reserved row
        // the three title-bar rungs are not live at all.
        let driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        let screen = driver.screen();

        assert_eq!(
            *frame.borrow(),
            render::frame_sequence_fixture(),
            "expected frame sequence differs from the GTK twin's \
             (`frame_sequence_matches_across_backends_via_gtk_driver`); \
             screen:\n{screen}"
        );

        // Composition, not just bookkeeping (#587/#592): the rungs the record
        // claims must have reached the cells, and the dialog must land on top
        // of the context menu it was composed after.
        assert!(
            driver.find_bounds("File").is_some(),
            "MenuDropdown was composed but the menu bar never painted; screen:\n{screen}"
        );
        assert!(
            screen.contains("ZQXWwildA"),
            "Wildmenu was composed but no wildmenu entry painted; screen:\n{screen}"
        );
        assert!(
            screen.contains("ZQXW766DIALOG"),
            "recorded sequence claims the dialog painted, but its title is not \
             on screen — the recorder and the rasteriser disagree; screen:\n{screen}"
        );
    }

    /// Opens a context menu and a modal dialog in the same frame and asserts
    /// the *composed* overlay tail is `[ContextMenu, Dialog]` — the dialog on
    /// top.
    ///
    /// RED against unfixed `develop`: TUI already painted these two in this
    /// order, but nothing recorded or asserted it, so GTK's inverted copy
    /// (`Dialog` then `ContextMenu`) went unnoticed. Move either backend's arm
    /// out of the `compose_frame` walk — which is exactly the shape the bug
    /// had — and the recorded sequence changes here or in the GTK twin.
    #[test]
    fn overlay_band_paints_dialog_above_context_menu_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.menu_bar_visible = true;
        app.engine.open_editor_context_menu(4, 4);
        assert!(
            app.engine
                .context_menu
                .as_ref()
                .is_some_and(|m| !m.items.is_empty()),
            "fixture needs a non-empty context menu — an empty one is not painted"
        );
        app.engine.dialog = Some(in_canvas_dialog("ZQXW735DIALOG"));

        let frame = app.composed_frame.clone();
        // `shell_config(true)`, not `config()`: `AppShell::set_title_bar_visible`
        // is what reserves `layout.title_bar_bounds`, and with no reserved row
        // the menu-dropdown rung has nothing to paint into.
        let driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        let screen = driver.screen();

        assert_eq!(
            overlay_tail(&frame.borrow()),
            vec![
                render::FrameOp::MenuDropdown,
                render::FrameOp::CommandCenter,
                render::FrameOp::ContextMenu,
                render::FrameOp::Dialog,
            ],
            "two orderings are pinned here: the title-bar chrome below the modal \
             stack (TUI had that inverted before #735 — it painted the menu row \
             over open dialogs) and the dialog above the context menu (GTK had \
             *that* inverted); screen:\n{screen}"
        );
        // Paint, not just bookkeeping: the dialog really did reach the cells.
        assert!(
            screen.contains("ZQXW735DIALOG"),
            "recorded band claims the dialog painted, but its title is not on \
             screen — the recorder and the rasteriser disagree; screen:\n{screen}"
        );
    }

    /// A frame with no overlays open records an empty overlay tail — the
    /// recorder is not just "whatever `FRAME_Z_ORDER` contains".
    ///
    /// Guards the caches that must be cleared *before* the walk (stale
    /// `dialog_layout` / `context_menu_layout` / `tab_switcher_popup_rect`
    /// geometry is the #587 class of bug) without that being mistaken for a
    /// paint. Since #766 the absent rungs have no arm to run at all, so the
    /// clear has to be unconditional — this is the test that would catch it
    /// being folded back into an `else`.
    #[test]
    fn overlay_band_is_empty_when_no_overlay_is_open_via_shell_app() {
        let app = TuiShellApp::new(None);
        let frame = app.composed_frame.clone();
        let driver = driver_with_shell(app, config(), 80, 24);
        let _ = driver.screen();
        assert_eq!(
            overlay_tail(&frame.borrow()),
            Vec::<render::FrameOp>::new(),
            "no overlay was open, so nothing in the tail should have composed"
        );
        // The chrome half is unaffected: the command-line row is always
        // composed, so an empty *tail* is not an empty frame.
        assert!(
            frame.borrow().contains(&render::FrameOp::CommandLine),
            "the command line row is composed on every frame"
        );
    }

    // ── Chrome half of the sequence (#763, #735 slice 2) ────────────────────
    //
    // The TUI half of the chrome acceptance test. `gtk/testing.rs`'s
    // `mod chrome_band_order` carries the GTK half and asserts against the
    // **same expected `Vec<FrameOp>`** (`render::chrome_band_fixture`) for the
    // same engine state, exactly as the pair above does.

    /// The chrome rungs of `composed_frame` — everything before the overlay
    /// tail.
    fn chrome_half(frame: &[render::FrameOp]) -> Vec<render::FrameOp> {
        frame
            .iter()
            .copied()
            .filter(|op| !op.is_overlay())
            .collect()
    }

    /// The overlay rungs of `composed_frame` — the tail `OverlayOp` used to be
    /// its own enum for, before #766 folded it in.
    fn overlay_tail(frame: &[render::FrameOp]) -> Vec<render::FrameOp> {
        frame.iter().copied().filter(|op| op.is_overlay()).collect()
    }

    /// Every chrome rung live, composed in `FRAME_Z_ORDER`.
    ///
    /// **RED against unfixed `develop`**, in two independent ways. (1) TUI
    /// composed the wildmenu *before* the global status line and GTK composed
    /// it *after*, so no single expected vector could satisfy both; swapping
    /// the two arms' bodies back (hoisting `FrameOp::StatusBar`'s body above
    /// `FrameOp::Wildmenu`'s, out of the `compose_frame` walk) makes this fail
    /// with `[.., StatusBar, Wildmenu, ..]` and trips
    /// `check_frame_order`'s `debug_assert` in `render_content` on the
    /// way. (2) TUI composed the menu row and the sidebar panel body at the
    /// *top* of the frame, ahead of the editor — hoisting either arm back out
    /// of the walk drops its `FrameOp` from the record entirely. Both were
    /// re-introduced, observed red, and restored before committing.
    #[test]
    fn chrome_band_composes_in_canonical_order_via_shell_app() {
        let mut app = app_with_sidebar_open();
        // The *settings* panel, not the explorer: its body paints a fixed
        // "SETTINGS" heading, where the explorer's would be this checkout's
        // own directory listing — ambient state a test must not depend on
        // (#762).
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_SETTINGS));
        app.engine.menu_bar_visible = true;
        app.engine.wildmenu_items = vec!["ZQXWwildA".to_string(), "ZQXWwildB".to_string()];
        app.engine.wildmenu_selected = Some(0);
        app.engine.mode = crate::core::Mode::Command;
        app.engine.command_buffer = "ZQXWcmd".to_string();
        // Explicit, not ambient (#762): a global status bar exists only when
        // per-window status lines are off, and the default is on.
        app.engine.settings.window_status_line = false;

        let frame = app.composed_frame.clone();
        // `shell_config(true)`, not `config()`: `AppShell::set_title_bar_visible`
        // is what reserves `layout.title_bar_bounds`, and with no reserved row
        // the `MenuRow` rung is not live at all.
        let driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        let screen = driver.screen();

        assert_eq!(
            chrome_half(&frame.borrow()),
            render::chrome_band_fixture(true),
            "expected chrome band differs from the GTK twin's \
             (`chrome_band_composes_in_canonical_order_via_gtk_driver`); \
             screen:\n{screen}"
        );

        // Composition, not just bookkeeping (#587/#592): every rung the record
        // claims must have reached the cells, and the three stacked bottom
        // bands must land in the order the band declares — wildmenu above the
        // global status line above the command line. Rows are *located*, never
        // hardcoded (`CLAUDE.md` rule 1).
        assert!(
            driver.find_bounds("File").is_some(),
            "MenuRow was composed but the menu bar never painted; screen:\n{screen}"
        );
        assert!(
            driver.find_bounds("SETTINGS").is_some(),
            "SidebarPanel was composed but the settings panel never painted; \
             screen:\n{screen}"
        );
        let wm = driver
            .find_bounds("ZQXWwildA")
            .expect("Wildmenu was composed but no wildmenu entry painted");
        let cmd = driver
            .find_bounds(":ZQXWcmd")
            .expect("CommandLine was composed but the command line never painted");
        assert!(
            wm.y < cmd.y,
            "wildmenu row ({}) must sit above the command line ({}); screen:\n{screen}",
            wm.y,
            cmd.y
        );
    }

    /// With no wildmenu up, the `Wildmenu` rung drops out — the record is not
    /// simply "whatever `FRAME_Z_ORDER` contains", and the remaining four
    /// rungs keep their relative order.
    #[test]
    fn chrome_band_drops_the_wildmenu_rung_when_no_completion_is_up_via_shell_app() {
        let mut app = app_with_sidebar_open();
        app.engine.menu_bar_visible = true;
        app.engine.settings.window_status_line = false;
        assert!(
            app.engine.wildmenu_items.is_empty(),
            "fixture needs no wildmenu"
        );

        let frame = app.composed_frame.clone();
        let driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        let screen = driver.screen();
        assert_eq!(
            chrome_half(&frame.borrow()),
            render::chrome_band_fixture(false),
            "no completion was up, so the Wildmenu rung must not be composed; \
             screen:\n{screen}"
        );
    }

    /// The `MenuRow` rung is gated on the shell actually reserving a title-bar
    /// row, not on `engine.menu_bar_visible` alone — `config()` builds a shell
    /// with no title bar, so `layout.title_bar_bounds` is `None` and
    /// `compose_frame` must drop the rung even though the engine flag is set.
    ///
    /// This is the paint/hit-test agreement #695 is about, expressed as a
    /// composition fact: a `MenuRow` rung composed against a band the shell
    /// never reserved would publish a `menu_bar_rect` nothing paints into.
    /// #766: the same gate now also drops `MenuDropdown` and `CommandCenter`,
    /// which is the divergence the fold closed on the GTK side (its dropdown
    /// arm checked only `menu_bar_visible`).
    #[test]
    fn chrome_band_drops_the_menu_row_when_the_shell_reserved_no_title_bar_via_shell_app() {
        let mut app = app_with_sidebar_open();
        app.engine.menu_bar_visible = true;

        let frame = app.composed_frame.clone();
        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        for op in [
            render::FrameOp::MenuRow,
            render::FrameOp::MenuDropdown,
            render::FrameOp::CommandCenter,
        ] {
            assert!(
                !frame.borrow().contains(&op),
                "no title-bar row was reserved, so {op:?} must not be composed; \
                 screen:\n{screen}"
            );
        }
        assert!(
            driver.find_bounds("File").is_none(),
            "no title-bar row was reserved, so no menu bar should have painted; \
             screen:\n{screen}"
        );
        assert_eq!(
            render::check_frame_order(&frame.borrow()),
            Ok(()),
            "the surviving rungs must still be in canonical order"
        );
    }

    // ── Editor band (#764, #735 slice 3) ────────────────────────────────────
    //
    // The TUI half of the editor-band acceptance test. `gtk/testing.rs`'s
    // `mod editor_band_order` carries the GTK half and asserts against the
    // **same expected `Vec<EditorOp>`** (`render::editor_band_fixture`) for the
    // same engine state, exactly as the chrome- and overlay-band pairs do.

    /// A two-group `Ctrl+W v` split with breadcrumbs and the minimap on and a
    /// tab-hover tooltip up: every editor rung live except the tab-drag ghost,
    /// which needs a live pointer drag.
    ///
    /// Every knob is set explicitly rather than inherited from
    /// `Settings::default()` (#762) — an ambient default that flips would
    /// silently turn this from a seven-rung assertion into a five-rung one.
    /// Mirrors `gtk/testing.rs`'s `engine_with_every_editor_rung`.
    fn app_with_every_editor_rung() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.use_nerd_fonts = false;
        app.engine.settings.breadcrumbs = true;
        app.engine.settings.minimap = true;
        let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        app.engine.cwd = cwd.clone();
        let buf = app.engine.active_buffer_id();
        if let Some(state) = app.engine.buffer_manager.get_mut(buf) {
            state.file_path = Some(cwd.join("src").join("main.rs"));
        }
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        // Two editor groups, so `group_dividers` is non-empty.
        app.engine
            .open_editor_group(crate::core::window::SplitDirection::Vertical);
        app.engine.tab_hover_tooltip = Some("ZQXW764TIP".to_string());
        app
    }

    /// **RED against unfixed `develop`**, in two independent ways. (1) GTK
    /// never composed `EditorOp::GroupDividers` at all — it painted only
    /// `window_dividers` and discarded the group set into `_dividers` — so the
    /// record came back one rung short of this one and no single expected
    /// vector could satisfy both backends. (2) TUI composed its tab-drag ghost
    /// *before* the tooltip and GTK composed it after the editor popups
    /// entirely; hoisting either arm back out of the `compose_editor_band`
    /// walk drops its `EditorOp` from the record and trips
    /// `check_editor_band_order`'s `debug_assert` in `render_content` on the
    /// way. Both were re-introduced, observed red, and restored before
    /// committing.
    #[test]
    fn editor_band_composes_in_canonical_order_via_shell_app() {
        let app = app_with_every_editor_rung();
        let band = app.composed_editor_band.clone();
        // Wide enough for both panes to still clear `minimap_reserved_width`'s
        // "want + MINIMAP_MIN_TEXT_COLS" floor after the vertical split, so the
        // `Minimap` rung is genuinely live rather than silently gated off.
        let driver = driver_with_shell(app, config(), 160, 40);
        let screen = driver.screen();

        assert_eq!(
            *band.borrow(),
            render::editor_band_fixture(false),
            "expected editor band differs from the GTK twin's \
             (`editor_band_composes_in_canonical_order_via_gtk_driver`); \
             screen:\n{screen}"
        );

        // Composition, not just bookkeeping (#587/#592): the rungs the record
        // claims must have reached the cells.
        assert!(
            driver.find_bounds("ZQXW764TIP").is_some(),
            "TabTooltip was composed but the tooltip text never painted; \
             screen:\n{screen}"
        );
        assert!(
            driver.find_bounds("main.rs").is_some(),
            "TabBars/Breadcrumbs were composed but the buffer name never \
             painted; screen:\n{screen}"
        );
        assert!(
            screen
                .chars()
                .any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
            "Minimap was composed but no braille reached the cells; \
             screen:\n{screen}"
        );
    }

    /// A **live** tab drag composes the ghost rung, and composes it *before*
    /// the tab tooltip — the one relative ordering the two backends' hand-kept
    /// ladders disagreed about.
    ///
    /// **RED against unfixed `develop`**: GTK composed its drop overlay after
    /// the editor-anchored popups and the whole chrome band, ~900 lines below
    /// the editor column, so a popup left open when a drag starts painted over
    /// the drop-zone highlight that owns the pointer. With the rung hoisted
    /// back out of the walk this record loses `TabDragOverlay` entirely.
    ///
    /// The drag machine is driven directly rather than through synthetic mouse
    /// events: `begin` is the exact state transition `mouse::handle_mouse`
    /// performs once the travel threshold is crossed, and going through it
    /// keeps the test off the pointer-geometry ambient state the mouse route
    /// would drag in (#762).
    #[test]
    fn live_tab_drag_composes_the_ghost_rung_before_the_tooltip_via_shell_app() {
        let mut app = app_with_every_editor_rung();
        let gid = app.engine.active_group;
        app.tab_drag.begin((gid, 0), 12.0, 0.0);

        let band = app.composed_editor_band.clone();
        let driver = driver_with_shell(app, config(), 160, 40);
        let screen = driver.screen();
        let band = band.borrow();

        assert_eq!(
            *band,
            render::editor_band_fixture(true),
            "a drag is live, so every rung including the ghost must be \
             composed, in canonical order; screen:\n{screen}"
        );
        let ghost = band
            .iter()
            .position(|op| *op == render::EditorOp::TabDragOverlay)
            .expect("the ghost rung must be composed while a drag is live");
        let tooltip = band
            .iter()
            .position(|op| *op == render::EditorOp::TabTooltip)
            .expect("the fixture keeps a tab tooltip up");
        assert!(
            ghost < tooltip,
            "the drag ghost must be composed below the tab tooltip; got \
             {band:?}; screen:\n{screen}"
        );
    }

    /// A single-group frame composes no `GroupDividers` rung — the record is
    /// not just "whatever `EDITOR_Z_ORDER` contains", and the gate is real.
    /// The GTK twin is
    /// `unsplit_editor_composes_no_group_divider_rung_via_gtk_driver`.
    #[test]
    fn unsplit_editor_composes_no_group_divider_rung_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        let band = app.composed_editor_band.clone();
        let driver = driver_with_shell(app, config(), 80, 24);
        let screen = driver.screen();
        let band = band.borrow();
        assert!(
            !band.contains(&render::EditorOp::GroupDividers),
            "one editor group means no between-group boundary, so the rung must \
             not be composed; got {band:?}; screen:\n{screen}"
        );
        assert!(
            !band.contains(&render::EditorOp::TabDragOverlay),
            "no drag is live, so the ghost rung must not be composed; got \
             {band:?}; screen:\n{screen}"
        );
        assert_eq!(
            render::check_editor_band_order(&band),
            Ok(()),
            "the surviving rungs must still be in canonical order"
        );
    }

    /// `:set nominimap` drops the `Minimap` rung from the composed band, and
    /// no braille reaches the cells — the gate is the *reservation*
    /// (`screen.minimap` empty), not a second copy of `settings.minimap` in
    /// `compose_editor_band`, which is what would let the two drift.
    #[test]
    fn editor_band_drops_the_minimap_rung_when_the_setting_is_off_via_shell_app() {
        let mut app = app_with_every_editor_rung();
        app.engine.settings.minimap = false;
        let band = app.composed_editor_band.clone();
        let driver = driver_with_shell(app, config(), 160, 40);
        let screen = driver.screen();
        assert!(
            !band.borrow().contains(&render::EditorOp::Minimap),
            "`minimap: false` reserves no strip, so the rung must not be \
             composed; got {:?}; screen:\n{screen}",
            band.borrow()
        );
        assert!(
            !screen
                .chars()
                .any(|c| ('\u{2800}'..='\u{28FF}').contains(&c)),
            "the rung was not composed, so no braille may reach the cells; \
             screen:\n{screen}"
        );
    }

    // ── #765 / #735 slice 4: the shared bottom band ────────────────────────
    //
    // `gtk/testing.rs`'s `mod bottom_band_order` carries the GTK half and
    // asserts against the **same expected `Vec<BottomOp>`**
    // (`render::bottom_band_fixture`) for the same engine state, exactly as
    // the chrome-, editor- and overlay-band pairs do.

    /// Quickfix open with an item, the bottom panel up on Debug Output, the
    /// debug toolbar visible and the per-window status line extracted: every
    /// stacked bottom rung live. The hover popup needs a live sidebar dwell
    /// and is covered separately.
    ///
    /// Every knob is set explicitly rather than inherited from
    /// `Settings::default()` (#762) — an ambient default that flips would
    /// silently turn this from a four-rung assertion into a three-rung one.
    /// Mirrors `gtk/testing.rs`'s `engine_with_every_bottom_rung`.
    fn app_with_every_bottom_rung() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.use_nerd_fonts = false;
        // `separated_status_line` is `Some` only for
        // `window_status_line && !status_line_above_terminal && panel open`.
        app.engine.settings.window_status_line = true;
        app.engine.settings.status_line_above_terminal = false;
        app.engine
            .quickfix_items
            .push(crate::core::project_search::ProjectMatch {
                file: PathBuf::from("zqxw765.rs"),
                line: 0,
                col: 0,
                line_text: "ZQXW765QF".to_string(),
            });
        app.engine.quickfix_open = true;
        app.engine.bottom_panel_open = true;
        app.engine.bottom_panel_kind = render::BottomPanelKind::DebugOutput;
        app.engine.dap_output_lines.push("ZQXW765DBG".to_string());
        app.engine.debug_toolbar_visible = true;
        app
    }

    /// **RED against unfixed `develop`** in two independent ways. (1) This
    /// backend composed `SeparatedStatus` *second*, before `BottomPanel` and
    /// `DebugToolbar` — contradicting its own
    /// `bottom_chrome_rects_for_shell_content`, whose constraint array has
    /// always reserved that row *after* the debug toolbar. Against the unfixed
    /// order the record comes back
    /// `[Quickfix, SeparatedStatus, BottomPanel, DebugToolbar]`, which is not
    /// `bottom_band_fixture(false)` and additionally trips
    /// `check_bottom_band_order`'s `debug_assert` in `render_content` on the
    /// way past. (2) GTK composed the same four in yet another order, so no
    /// single expected vector could satisfy both backends at once. Both were
    /// re-introduced, observed red, and restored before committing.
    #[test]
    fn bottom_band_composes_in_canonical_order_via_shell_app() {
        let app = app_with_every_bottom_rung();
        let band = app.composed_bottom_band.clone();
        let driver = driver_with_shell(app, config(), 100, 40);
        let screen = driver.screen();

        assert_eq!(
            *band.borrow(),
            render::bottom_band_fixture(false),
            "expected bottom band differs from the GTK twin's \
             (`bottom_band_composes_in_canonical_order_via_gtk_driver`); \
             screen:\n{screen}"
        );

        // Composition, not just bookkeeping (#587/#592): the rungs the record
        // claims must have reached the cells.
        assert!(
            driver.find_bounds("ZQXW765QF").is_some(),
            "Quickfix was composed but its item never painted; screen:\n{screen}"
        );
        assert!(
            driver.find_bounds("ZQXW765DBG").is_some(),
            "BottomPanel was composed but the debug output never painted; \
             screen:\n{screen}"
        );
    }

    /// The separated status line is composed **after** the bottom panel and
    /// the debug toolbar, and lands on the row
    /// `bottom_chrome_rects_for_shell_content` reserved for it — below both.
    ///
    /// **RED against unfixed `develop`**: this is the ordering half of the
    /// divergence above, stated as *geometry* rather than as a record, so it
    /// fails even if someone "fixes" the record without moving the paint. The
    /// unfixed backend painted this rung two rungs early.
    #[test]
    fn separated_status_paints_below_the_bottom_panel_via_shell_app() {
        let app = app_with_every_bottom_rung();
        let band = app.composed_bottom_band.clone();
        let driver = driver_with_shell(app, config(), 100, 40);
        let screen = driver.screen();

        let composed = band.borrow().clone();
        let pos = |op: render::BottomOp| composed.iter().position(|o| *o == op);
        assert!(
            pos(render::BottomOp::SeparatedStatus) > pos(render::BottomOp::BottomPanel),
            "SeparatedStatus must be composed after BottomPanel; got {composed:?}"
        );
        assert!(
            pos(render::BottomOp::SeparatedStatus) > pos(render::BottomOp::DebugToolbar),
            "SeparatedStatus must be composed after DebugToolbar; got {composed:?}"
        );

        // …and the paint agrees with the record, top to bottom: the quickfix
        // item is painted strictly above the bottom panel's debug output,
        // which is the geometric order `BOTTOM_Z_ORDER` encodes. A record that
        // said one thing while the rects said another would fail here.
        let qf = driver
            .find_bounds("ZQXW765QF")
            .unwrap_or_else(|| panic!("quickfix must have painted; screen:\n{screen}"));
        let dbg = driver
            .find_bounds("ZQXW765DBG")
            .unwrap_or_else(|| panic!("bottom panel must have painted; screen:\n{screen}"));
        assert!(
            qf.y < dbg.y,
            "the quickfix panel must paint above the bottom panel \
             (quickfix at row {}, debug output at row {}); screen:\n{screen}",
            qf.y,
            dbg.y
        );
    }

    /// The debug toolbar's cached rect must be **cleared** when the toolbar is
    /// not composed.
    ///
    /// **RED against unfixed `develop`**: this backend's debug-toolbar rung had
    /// no `else` at all — GTK's zeroed its two equivalent caches, TUI's zeroed
    /// nothing — so `debug_toolbar_rect` kept the last frame's rect forever
    /// once the toolbar hid. `route_chrome_click` hit-tests against that rect,
    /// so clicks on the row the toolbar *used to* occupy kept resolving to
    /// toolbar buttons. Re-introduce the missing clear (drop the pre-walk
    /// `debug_toolbar_rect.set(default)`) and this fails on the second frame.
    #[test]
    fn hiding_the_debug_toolbar_clears_its_cached_rect_via_shell_app() {
        let app = app_with_every_bottom_rung();
        let band = app.composed_bottom_band.clone();
        let rect = app.debug_toolbar_rect.clone();
        let mut driver = driver_with_shell(app, config(), 100, 40);
        driver.render();
        assert!(
            band.borrow().contains(&render::BottomOp::DebugToolbar),
            "precondition: the toolbar rung is composed while visible"
        );
        assert!(
            rect.get().height > 0.0,
            "precondition: a visible toolbar caches a non-degenerate rect, got {:?}",
            rect.get()
        );

        // Hide it the way a user does — Shift+F5 is `stop`, and `dap_stop`
        // clears `debug_toolbar_visible` (#762's shared debugger F-key rung).
        press_with(
            &mut driver,
            quadraui::Key::Named(quadraui::NamedKey::F(5)),
            quadraui::Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        driver.render();

        assert!(
            !band.borrow().contains(&render::BottomOp::DebugToolbar),
            "hiding the toolbar must drop its rung from the band; got {:?}",
            band.borrow()
        );
        assert_eq!(
            rect.get().height,
            0.0,
            "hiding the toolbar must clear the rect `route_chrome_click` \
             hit-tests against, or clicks keep resolving to buttons that are \
             no longer painted; got {:?}",
            rect.get()
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

    /// Engine with a real `cwd` (so the search box's title isn't empty) and
    /// two tabs wired into `tab_nav_history` at index 1, so `tab_nav_back`
    /// has somewhere to go and `tab_nav_can_go_back()` reads `true` the
    /// moment the frame paints. Mirrors GTK's `engine_with_tab_history`
    /// (`src/gtk/testing.rs`'s `command_center` test module, #676) with one
    /// difference: tab1 opens this repo's own `Cargo.toml` (always
    /// present, always the same first line) rather than staying blank —
    /// GTK asserts tab-nav via `active_tab().id`, which TUI's driver has no
    /// accessor for (module doc: no way back to the concrete
    /// `TuiShellApp`), so the click-routing test below needs the two tabs
    /// to paint *visibly* differently (blank editor vs. `[package]`) to
    /// prove a click actually switched the active tab.
    fn app_with_menu_bar_and_tab_history() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.menu_bar_visible = true;
        app.engine.cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let group = app.engine.active_group;
        let tab0 = app.engine.active_tab().id;
        let cargo_toml = app.engine.cwd.join("Cargo.toml");
        app.engine.new_tab(Some(&cargo_toml));
        let tab1 = app.engine.active_tab().id;
        assert_ne!(tab0, tab1, "fixture needs two distinct tabs");
        app.engine.tab_nav_history = vec![(group, tab0), (group, tab1)];
        app.engine.tab_nav_index = 1;
        app
    }

    /// #712: the omnibar (quadraui `CommandCenter`) never painted on TUI —
    /// `render_content` computed and cached a fully populated
    /// `command_center_layout` (so it was hit-testable), but the paint
    /// itself, done *before* the menu-dropdown block further down, got
    /// silently erased: that later block calls `MenuSystem::render`, which
    /// unconditionally repaints `draw_menu_bar` across the *entire* bar
    /// row — including the command centre's columns — whether or not a
    /// dropdown is actually open. Fixed by deferring the command centre's
    /// paint until after that block (see `pending_command_center`'s doc
    /// comment in `render_content`), mirroring GTK's identical #676 fix
    /// and its `command_center_paints_between_menu_labels_and_window_controls`
    /// test. Verified this fails (screen has no ◀/▶/🔍 past "Help") against
    /// the pre-fix ordering.
    #[test]
    fn render_content_paints_command_center_after_menu_labels_via_shell_app() {
        let app = app_with_menu_bar_and_tab_history();
        let driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        let screen = driver.screen();

        let file = driver
            .find_bounds("File")
            .expect("the \"File\" menu label must paint");
        let back = driver
            .find_bounds("◀")
            .expect("the back arrow must paint on the menu-bar row");
        let fwd = driver
            .find_bounds("▶")
            .expect("the forward arrow must paint on the menu-bar row");
        let search = driver
            .find_bounds("🔍")
            .expect("the search-box icon must paint on the menu-bar row");

        assert_eq!(
            back.y, file.y,
            "the command centre must paint on the same row as the menu labels; screen:\n{screen}"
        );
        assert!(
            back.x > file.x && back.x < fwd.x && fwd.x < search.x,
            "back arrow, forward arrow, and search box must lay out left-to-right \
             to the right of the menu labels: file={file:?} back={back:?} \
             forward={fwd:?} search={search:?}"
        );
    }

    /// #712 companion: paint and hit-test must agree at the *same*
    /// coordinates — clicking the painted arrows/search box must actually
    /// drive tab-nav and open the picker, not just have a populated
    /// `command_center_layout` sitting unused (which is exactly what the
    /// pre-fix code had: `mouse.rs`'s hit test already worked against the
    /// cached layout, only the paint was missing). Mirrors GTK's
    /// `command_center_click_routes_nav_and_opens_picker`.
    #[test]
    fn command_center_click_routes_nav_and_opens_picker_via_shell_app() {
        let app = app_with_menu_bar_and_tab_history();
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);

        let back = driver.find_bounds("◀").expect("back arrow must paint");
        let fwd = driver.find_bounds("▶").expect("forward arrow must paint");
        let search = driver
            .find_bounds("🔍")
            .expect("search-box icon must paint");

        assert!(
            !driver.screen().contains("Search"),
            "no picker popup should be open before the search box is clicked; screen:\n{}",
            driver.screen()
        );

        // Search box -> opens the unified Command Center picker (mirrors
        // GTK #676; `PickerSource::CommandCenter`'s title is "Search",
        // `core/engine/picker.rs`).
        driver.click(
            search.x + search.width / 2.0,
            search.y + search.height / 2.0,
        );
        assert!(
            driver.screen().contains("Search"),
            "clicking the search box must open the Command Center picker \
             (its \"Search\" title must paint); screen:\n{}",
            driver.screen()
        );

        // Dismiss the picker before exercising the nav arrows below —
        // otherwise a click at the (now-covered) arrow coordinates would
        // hit the picker overlay instead of the command centre.
        driver.press_named(quadraui::NamedKey::Escape);
        assert!(
            !driver.screen().contains("Search"),
            "Escape must close the picker before the nav-arrow assertions below; screen:\n{}",
            driver.screen()
        );

        // Back arrow -> tab-nav history moves backward, from tab1
        // (`Cargo.toml`, active per the fixture's `tab_nav_index == 1`) to
        // tab0 (a blank buffer). `Cargo.toml`'s `[package]` first line is
        // the observable, engine-external proof the click actually reached
        // tab-nav and switched the *active* tab (as opposed to a hit-test
        // no-op) — `driver`'s only feedback channel is the painted screen
        // (module doc: no accessor back to the concrete `TuiShellApp`), and
        // the tab bar shows both tab labels regardless of which is active,
        // so only the editor content pane distinguishes them.
        assert!(
            driver.screen().contains("[package]"),
            "fixture must start on the Cargo.toml tab; screen:\n{}",
            driver.screen()
        );
        driver.click(back.x + back.width / 2.0, back.y + back.height / 2.0);
        assert!(
            !driver.screen().contains("[package]"),
            "clicking the back arrow must navigate away from the Cargo.toml \
             tab to the blank one; screen:\n{}",
            driver.screen()
        );

        // Forward arrow -> undoes the back navigation.
        driver.click(fwd.x + fwd.width / 2.0, fwd.y + fwd.height / 2.0);
        assert!(
            driver.screen().contains("[package]"),
            "clicking the forward arrow must undo the back navigation, \
             returning to the Cargo.toml tab; screen:\n{}",
            driver.screen()
        );
    }

    /// #712 companion: the reserved-row degenerate case (menu bar
    /// technically visible but this frame's `title_bar_bounds` collapsed
    /// to zero width/height) must clear `command_center_layout`, not leave
    /// last frame's stale layout live for `mouse.rs`'s hit test to keep
    /// routing clicks against — the "mechanism 2" this issue named
    /// alongside the paint-order bug the tests above cover. Exercised via
    /// the ordinary hidden-menu-bar path (the only reachable degenerate
    /// case from a black-box test — `menu_bar_visible=false` reserves no
    /// row at all, same effect on `command_center_layout` as a collapsed
    /// one) and asserted the same way GTK's sibling test is:
    /// click-behaviour-after-hide, not a bare `is_none()` state check
    /// (#553/#592 shape).
    #[test]
    fn command_center_layout_clears_when_menu_bar_hidden_via_shell_app() {
        // Measure the search box's coordinates on a *visible*-menu-bar frame
        // first, from a throwaway driver — `driver_with_shell` consumes its
        // app by value and paints immediately, so this needs its own
        // fixture instance before building the real (hidden) one below.
        let (search_x, search_y) = {
            let probe_driver = driver_with_shell(
                app_with_menu_bar_and_tab_history(),
                TuiShellApp::shell_config(true),
                80,
                24,
            );
            let sb = probe_driver
                .find_bounds("🔍")
                .expect("search-box icon must paint while the menu bar is visible");
            (sb.x + sb.width / 2.0, sb.y + sb.height / 2.0)
        };

        let mut app = app_with_menu_bar_and_tab_history();

        app.engine.menu_bar_visible = false;
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(true), 80, 24);
        assert!(
            driver.find_bounds("🔍").is_none(),
            "the search-box icon must not paint once the menu bar is hidden"
        );

        // Click at the coordinates the search box used to occupy: with the
        // menu bar hidden, that must no longer open the picker (a stale
        // cached layout would still hit-test and route the click).
        driver.click(search_x, search_y);
        assert!(
            !driver.screen().contains("Search"),
            "a click at the old command-centre coordinates must not open the \
             picker once the menu bar is hidden (stale hit-region); screen:\n{}",
            driver.screen()
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

    /// `TuiAccelHost::terminal_toggle_max` must derive `terminal_max_rows`
    /// from `screen_h` (the terminal's row count), not `screen_w` — the bug
    /// review iteration 1 of vimcode#595 caught: the wrapper silently fed
    /// `screen_w` into `terminal_target_maximize_rows_tui`, whose parameter
    /// is documented `screen_h`. Uses a screen far wider than it is tall so
    /// swapping the two arguments would produce a visibly different (larger)
    /// row count, making the regression this guards against actually
    /// detectable.
    #[test]
    fn terminal_toggle_max_uses_screen_height_not_width() {
        let mut engine = Engine::new();
        let mut sidebar = TuiSidebar::new();

        let screen_w: u16 = 200;
        let screen_h: u16 = 24;

        // `terminal_target_maximize_rows_tui` only returns a nonzero target
        // once the terminal panel is considered open (`bp_open` in
        // `compute_editor_layout`). In `TuiAccelHost::terminal_toggle_max`,
        // the arm builds `ctx` (which is where the screen_w/screen_h bug
        // lives) *before* calling `Engine::handle_ui_event` — and it's that
        // call that flips `terminal_open` via `toggle_terminal_maximize`. So
        // the bug is only observable when the terminal panel is *already*
        // open and just not yet maximized (e.g. the user has a terminal open
        // and presses "maximize") — pre-seed that precondition so `bp_open`
        // is already true when `ctx` is built, matching the live-usage
        // scenario this regression test guards.
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

        let mut host = TuiAccelHost {
            sidebar: &mut sidebar,
            screen_w,
            screen_h,
            sidebar_width: SIDEBAR_WIDTH,
            mods: quadraui::Modifiers::default(),
        };
        let dispatched = render::dispatch_panel_accelerator(
            render::ACC_TERMINAL_TOGGLE_MAX,
            &mut engine,
            &mut host,
        );

        assert!(dispatched.is_some());
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
            let modal_rc = backend.modal_stack_handle();
            modal_rc.borrow_mut().push(
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
        // `ensure_panel_active` focuses the explorer as a side effect, so
        // clear it first: the assertion below has to prove the
        // `TreeController` intercept did **not** *claim* focus, and it can
        // only do that from a known-unfocused start. (#751: before the
        // context-menu rung moved into `render::route_modal_overlay_click`,
        // this read as `!explorer_has_focus` only because `handle_mouse` fell
        // all the way through to the *editor* click path, which clears
        // sidebar focus — i.e. the assertion passed for the wrong reason,
        // and would have kept passing if the menu had been ignored entirely.)
        app.engine.explorer_has_focus = false;
        app.sidebar.has_focus = false;

        let mut backend = backend_at(80.0, 24.0);
        {
            let modal_rc = backend.modal_stack_handle();
            assert!(
                modal_rc
                    .borrow()
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
        assert!(
            app.engine.context_menu.is_none(),
            "the click must have reached the shared context-menu rung, which \
             dismisses a menu whose layout was never painted (#751)"
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
    /// accelerator, exactly how a real keybinding would. #761 / #734 slice 6:
    /// this is the TUI half of the cross-backend parity pair — GTK's
    /// `panel_accelerator_opens_command_palette_via_gtk_driver`
    /// (`gtk/testing.rs`) dispatches the identical `render::ACC_COMMAND_PALETTE`
    /// id and asserts the same `PickerSource::Commands` outcome, proving the
    /// shared `render::dispatch_panel_accelerator` resolves one accelerator to
    /// one action on both backends.
    #[test]
    fn command_palette_open_intercepts_keys_via_shell_app() {
        let app = TuiShellApp::new(None);
        let mut driver = driver_with_shell(app, config(), 80, 24);

        let opened = driver.dispatch(quadraui::UiEvent::Accelerator(
            quadraui::AcceleratorId::new(render::ACC_COMMAND_PALETTE),
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
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = Some(new_folder_picker_controller(&engine));
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        scratch.set_key(
            quadraui::Key::Char('x'),
            quadraui::Modifiers::default(),
            false,
        );
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
            folder_picker.as_ref().map(|p| p.query()),
            Some("x"),
            "'x' should filter the picker's entry list, not reach Engine::handle_key \
             as a delete-char motion"
        );

        scratch.set_key(
            quadraui::Key::Named(quadraui::NamedKey::Escape),
            quadraui::Modifiers::default(),
            false,
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
        engine.cmd_sel.set(Some((0usize, 4usize))); // "hello"

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
            engine.cmd_sel.get().is_none(),
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
        engine.cmd_sel.set(Some((0usize, 4usize)));

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
            engine.cmd_sel.get().is_none(),
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
    /// #634 ported the sidebar-focused tier (now `handle_focus_owner_key`), `x` would have deleted a
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
                     folder_picker: &mut Option<quadraui::FolderPickerController>,
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

    /// #804: in a real terminal, crossterm delivers `<C-h>` as ctrl+'h', not
    /// as a `BackSpace` key — `Engine::handle_insert_key` used to have no
    /// arm for that and fell through to its catch-all character-insert
    /// branch, typing a literal "h" instead of backspacing. Black-box per
    /// CLAUDE.md: assert on the *rendered* line, not on `Engine` state —
    /// a passing assertion on `engine.buffer()` would not have caught the
    /// original bug if the TUI's own key translation were what was broken
    /// instead (it wasn't, but this test doesn't get to assume that).
    ///
    /// Re-verified RED against the pre-fix code (#804 review): temporarily
    /// reverting `handle_insert_key`'s terminal-ctrl-alias remap block (the
    /// `let (key_name, ctrl) = if ctrl { match key_name { "h" => ... } }`
    /// arm) to a no-op and re-running this test fails it — the rendered
    /// line still shows the marker with a literal 'h' appended instead of
    /// losing its trailing character.
    #[test]
    fn ctrl_h_backspaces_in_insert_mode_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        let marker = "ZQXW804MARK";
        app.engine.buffer_mut().insert(0, marker);
        app.engine.mode = crate::core::Mode::Insert;
        app.engine.view_mut().cursor.col = marker.chars().count();
        let mut driver = driver_with_shell(app, config(), 80, 24);

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('h'),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });
        driver.render();

        let screen = driver.screen();
        assert!(
            !screen.contains(marker) && screen.contains(&marker[..marker.len() - 1]),
            "Ctrl+h in insert mode must backspace (drop the trailing 'K'), \
             not type a literal 'h' after the marker; screen:\n{screen}"
        );
    }

    /// #760 / #734 slice 5: Ctrl+Shift+V used to hand-roll its own paste path
    /// (read the clipboard, `load_clipboard_for_paste`, then either replay
    /// `p` or splice characters into insert mode) instead of calling
    /// `Engine::route_paste` the way plain Ctrl+V and bracketed paste
    /// (`bracketed_paste_reaches_the_buffer_via_shell_app` above) already do.
    /// That hand-rolled path only matched `Mode::{Normal,Visual,VisualLine,
    /// VisualBlock,Insert,Replace}` — `Mode::Command` fell into its `_ => {}`
    /// arm, so Ctrl+Shift+V while typing a `:command` silently did nothing
    /// but still consumed the keypress. `route_paste`'s `Mode::Command |
    /// Mode::Search` arm pastes into the command-line buffer instead, so this
    /// is RED against the pre-#760 code: the command line stays empty there.
    #[test]
    fn ctrl_shift_v_pastes_into_the_command_line_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.mode = crate::core::Mode::Command;
        app.engine.clipboard_read = Some(Box::new(|| Ok("ZQXW_SHIFT_PASTE_MARKER".to_string())));
        let mut driver = driver_with_shell(app, config(), 80, 24);

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('V'),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                shift: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });
        driver.render();

        assert!(
            driver.screen_contains(":ZQXW_SHIFT_PASTE_MARKER"),
            "Ctrl+Shift+V must route through Engine::route_paste into the \
             command line; screen:\n{}",
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

    // ── #703: per-tab language icons (quadraui `draw_tab_bar_icons`) ────────

    /// Render the tab row for two open files with Nerd Fonts either on or
    /// off, returning `(row, dir)` — the painted row-0 string plus the temp
    /// dir to clean up.
    ///
    /// Nerd Fonts is a thread-local (see `crate::icons`' module docs), and
    /// `TuiShellApp::new` sets it from `Settings`, so the flag has to be
    /// flipped *after* construction — exactly as `app_with_ext_panel` does.
    fn tab_row_with_nerd_fonts(on: bool) -> (String, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_tab_icons_703_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let alpha = dir.join("alpha703.rs");
        let beta = dir.join("beta703.rs");
        std::fs::write(&alpha, "fn main() {}\n").unwrap();
        std::fs::write(&beta, "fn other() {}\n").unwrap();

        let mut app = TuiShellApp::new(None);
        app.engine.settings.use_nerd_fonts = on;
        crate::icons::set_nerd_fonts(on);
        app.engine
            .open_file_with_mode(&alpha, crate::core::engine::OpenMode::Permanent)
            .unwrap();
        app.engine.new_tab(Some(&beta));

        let driver = driver_with_shell(app, config(), 100, 24);
        let row = driver
            .screen()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        (row, dir)
    }

    /// #703 acceptance (TUI): every tab slot shifts right by exactly
    /// `TabIcon::cols()` once icons are on, the language glyph paints in the
    /// column that reservation opened up, and the close × keeps its own
    /// column immediately after the label.
    ///
    /// # Why this fails against unfixed `develop`
    ///
    /// Before this change both backends called `Backend::draw_tab_bar`,
    /// which paints no icons at all: the `alpha703.rs` label started at the
    /// bar's column 0 with nothing before it, so `icon_shift` below would be
    /// `0`, not `TabIcon::cols()`, and no Rust glyph would be on the row.
    ///
    /// It is also the guard for the *measure* half. `render_tab_bar` hands
    /// the same sidecar to `draw_tab_bar_icons` that
    /// `compute_tab_bar_hit_regions` measured with; painting with icons
    /// while measuring with `&[]` would leave the second tab's label painted
    /// at a column the hit regions never cover.
    #[test]
    fn tab_bar_paints_language_icons_and_shifts_slots_via_shell_app() {
        let prev_nf = crate::icons::nerd_fonts_enabled();
        let (row_on, dir_on) = tab_row_with_nerd_fonts(true);
        let (row_off, dir_off) = tab_row_with_nerd_fonts(false);
        crate::icons::set_nerd_fonts(prev_nf);
        let _ = std::fs::remove_dir_all(&dir_on);
        let _ = std::fs::remove_dir_all(&dir_off);

        // The Rust glyph reserves its own display width plus a 1-column gap.
        let rust_icon = quadraui::TabIcon {
            glyph: crate::icons::FILE_RUST.nerd.to_string(),
            color: quadraui::Color::rgb(0, 0, 0),
        };
        let icon_cols = rust_icon.cols() as usize;
        assert!(icon_cols >= 2, "a glyph plus its gap is at least 2 columns");

        let on_alpha = tab_col(&row_on, "alpha703.rs");
        let off_alpha = tab_col(&row_off, "alpha703.rs");
        let on_beta = tab_col(&row_on, "beta703.rs");
        let off_beta = tab_col(&row_off, "beta703.rs");

        // Origin-free: the distance between two adjacent tab labels is that
        // tab's whole slot, so it does not depend on where the bar starts
        // (the sidebar shifts it right).  Without icons a slot is exactly
        // label + `TabInfo`'s trailing space + `TAB_CLOSE_COLS`.
        let bare_slot = "alpha703.rs".chars().count() + 1 + crate::render::TAB_CLOSE_COLS as usize;
        assert_eq!(
            off_beta - off_alpha,
            bare_slot,
            "with icons off, tab 0's slot must be label + trailing space + \
             close cols and nothing more; row:\n{row_off}"
        );
        assert_eq!(
            on_beta - on_alpha,
            bare_slot + icon_cols,
            "with icons on, tab 0's slot must widen by exactly the icon \
             reservation ({icon_cols} cols); row:\n{row_on}"
        );

        // The glyph itself paints in the column the reservation opened, i.e.
        // at the head of tab 0's slot, `icon_cols` left of the label.
        let glyph = crate::icons::FILE_RUST.nerd.chars().next().unwrap();
        assert_eq!(
            row_on.chars().position(|c| c == glyph),
            Some(on_alpha - icon_cols),
            "the Rust glyph must paint at the head of tab 0's slot; row:\n{row_on}"
        );
        assert!(
            !row_off.contains(crate::icons::FILE_RUST.nerd),
            "no Nerd Font glyph may paint with Nerd Fonts off; row:\n{row_off}"
        );

        // The close × keeps its own column right after the label + the
        // deliberate trailing space `TabInfo::name` carries, in both renders:
        // the icon widens the slot on the *left*, it does not push the glyph
        // off its column relative to the label it belongs to.
        for (row, label_at) in [(&row_on, on_alpha), (&row_off, off_alpha)] {
            let close_col = label_at + "alpha703.rs".chars().count() + 1;
            assert_eq!(
                row.chars().nth(close_col),
                Some('\u{00d7}'),
                "the close glyph must stay on its own column at {close_col}; row:\n{row}"
            );
        }
    }

    /// Column (not byte offset) at which `label` paints in a rendered row.
    /// Nerd Font glyphs are multi-byte, so `str::find` is not a column.
    fn tab_col(row: &str, label: &str) -> usize {
        let byte = row
            .find(label)
            .unwrap_or_else(|| panic!("tab bar must paint {label}; row:\n{row}"));
        row[..byte].chars().count()
    }

    /// #703: with Nerd Fonts off, `build_tab_bar_icons` returns `&[]` rather
    /// than ASCII fallbacks (a bare `R` before every label is noise, not
    /// parity) — and `&[]` makes `draw_tab_bar_icons` byte-identical to
    /// `draw_tab_bar`. So the tab bar must paint exactly what it painted
    /// before this feature existed: nothing between the bar's left edge and
    /// the first label, and no fallback letter either.
    #[test]
    fn nerd_fonts_off_keeps_tab_bar_geometry_byte_identical_via_shell_app() {
        let prev_nf = crate::icons::nerd_fonts_enabled();
        let (row, dir) = tab_row_with_nerd_fonts(false);
        crate::icons::set_nerd_fonts(prev_nf);
        let _ = std::fs::remove_dir_all(&dir);

        // No reservation: the two labels sit exactly one bare slot apart.
        let bare_slot = "alpha703.rs".chars().count() + 1 + crate::render::TAB_CLOSE_COLS as usize;
        assert_eq!(
            tab_col(&row, "beta703.rs") - tab_col(&row, "alpha703.rs"),
            bare_slot,
            "with Nerd Fonts off no column may be reserved for an icon; row:\n{row}"
        );
        // …and specifically not an ASCII fallback: `file_icon("rs")` would
        // return "R" with the flag off, which is what `&[]` exists to avoid.
        let before_alpha = row
            .chars()
            .nth(tab_col(&row, "alpha703.rs").saturating_sub(1));
        assert_ne!(
            before_alpha,
            Some('R'),
            "the ASCII fallback must not be painted as a tab badge; row:\n{row}"
        );
        for icon in [
            crate::icons::FILE_RUST.nerd,
            crate::icons::FILE_GENERIC.nerd,
        ] {
            assert!(
                !row.contains(icon),
                "no Nerd Font glyph may paint with Nerd Fonts off; row:\n{row}"
            );
        }
    }

    // ── Minimap (#35) ───────────────────────────────────────────────────

    /// Buffer with a lopsided indentation shape, long enough that the
    /// minimap has to down-sample.
    fn app_with_shaped_buffer() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        let text: String = (0..240)
            .map(|i| {
                let depth = if (80..160).contains(&i) { 3 } else { 0 };
                format!("{}line {i}\n", "    ".repeat(depth))
            })
            .collect();
        app.engine.buffer_mut().insert(0, &text);
        app
    }

    /// Column of the leftmost braille cell on `row`, or `None` if that row
    /// paints no braille. Locating the strip this way (rather than
    /// hardcoding a column) keeps the "locate targets, never hardcode
    /// coords" rule intact.
    fn braille_col(screen: &str, row: usize) -> Option<usize> {
        screen
            .lines()
            .nth(row)?
            .chars()
            .position(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
    }

    /// Whether `s` contains a *non-blank* braille glyph
    /// (`\u{2801}'..='\u{28FF}`, excluding the all-empty `\u{2800}` cell) —
    /// the minimap's own paint signature.
    fn has_braille(s: &str) -> bool {
        s.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c))
    }

    /// `app_with_shaped_buffer` split into two panes (`:vsplit`) — the
    /// fixture the split-minimap black-box tests below share.
    ///
    /// Sidebar/autohide state pinned explicitly rather than left ambient:
    /// `TuiShellApp::new`'s sidebar visibility reads the developer's real
    /// `~/.config/vimcode` off disk (`app_with_sidebar_open`'s doc comment
    /// above explains the split-state shape this produces), and per
    /// `on_shell_event`'s own doc comment the runner's *painted* `AppShell`
    /// only picks up the engine's (the "shadow"'s) sidebar/autohide state
    /// at the tail of a dispatch — never on the very first frame
    /// `driver_with_shell` paints before any `handle()` call. Pinning both
    /// here keeps `focus_change_does_not_move_either_panes_text_via_shell_app`
    /// from spuriously seeing the sidebar appear/disappear mid-test, which
    /// would otherwise be indistinguishable from a real minimap regression.
    fn app_with_split_shaped_buffer() -> TuiShellApp {
        let mut app = app_with_shaped_buffer();
        app.engine.settings.autohide_panels = false;
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        app.engine.split_window(SplitDirection::Vertical, None);
        app
    }

    /// #722 acceptance, painted-output tier: a `:vsplit` must paint **two**
    /// independent minimap strips, one over each pane's own buffer — not a
    /// single strip pinned to whichever pane happens to be active.
    ///
    /// The review that reopened #722 flagged that every prior test for this
    /// (`split_gives_every_pane_its_own_minimap_over_its_own_buffer` et al.
    /// in `render.rs`) only asserted on `ScreenLayout.minimap.len()` — a
    /// struct field populated by the pure layout function, never proven to
    /// reach a real frame. This drives the same fixture through the real
    /// `driver_with_shell` → `render_content` → `draw_minimap_strip` path
    /// and reads the painted braille back.
    ///
    /// RED against the pre-#722 code (single `Option<RenderedMinimap>`
    /// gated on `active_window_id`): only one half of the screen would ever
    /// paint braille — confirmed by hand by reverting `build_screen_layout`
    /// to `.find(|(id, _)| *id == active_window_id)` before restoring this
    /// fix.
    #[test]
    fn split_paints_two_independent_minimap_strips_via_shell_app() {
        let driver = driver_with_shell(app_with_split_shaped_buffer(), config(), 160, 30);
        let screen = driver.screen();

        // Both panes share the same fixture buffer, so a row that paints
        // buffer text paints the literal `"line "` prefix twice — once per
        // pane. Splitting the row at the second occurrence's start column
        // (rather than at a hardcoded fraction of the screen width) locates
        // each pane's own column span regardless of sidebar width, gutter
        // width or where exactly the window divider/scrollbar glyphs land.
        let mut left_hit = false;
        let mut right_hit = false;
        for row in 0..30usize {
            let Some(line) = screen.lines().nth(row) else {
                continue;
            };
            let starts: Vec<usize> = line.match_indices("line ").map(|(i, _)| i).collect();
            let Some(&second) = starts.get(1) else {
                continue; // row doesn't paint both panes' buffer text
            };
            let (left, right) = line.split_at(second);
            left_hit |= has_braille(left);
            right_hit |= has_braille(right);
        }
        assert!(
            left_hit && right_hit,
            "a `:vsplit` must paint minimap braille in both the left pane \
             and the right pane, not just one; screen:\n{screen}"
        );
    }

    /// #722 acceptance, painted-output tier: switching focus between panes
    /// of a `:vsplit` must not move either pane's text — coverage for the
    /// "migrates on focus change, reflowing both panes" symptom the issue
    /// called out as *worse* than the missing strip (the width reclaim was
    /// gated on the same `is_active` flag as the strip itself, so both
    /// panes reflowed on every focus change).
    ///
    /// Drives a live `<C-w>w` — the standard two-keystroke vim window-cycle
    /// chord (`Engine::handle_key`'s `pending_key = Some('\x17')` arm,
    /// consumed by the plain `w` that follows) — through the real
    /// `driver_with_shell` key path, and diffs two real painted frames: the
    /// right pane's own `"line "` text column (both panes share the fixture
    /// buffer, so this is exactly the column its own gutter/minimap layout
    /// puts it at) must land in the same place whichever pane is focused.
    ///
    /// A test that dispatches `<C-w>w` but never confirms it actually moved
    /// focus would pass vacuously if the chord silently no-ops (identical
    /// before/after frames trivially satisfy "column unchanged"), so this
    /// first pins that focus really moved by locating the block cursor's
    /// own painted cell — quadraui's TUI editor rasteriser paints
    /// `CursorShape::Block` (Normal mode's shape, the fixture's default) as
    /// a cell background recolour (`theme.cursor`), not ratatui's real
    /// terminal cursor, and only in the *active* window
    /// (`build_screen_layout`'s `is_active`-gated `cursor` field — see
    /// `render.rs`) — so that cell must relocate to the other pane.
    ///
    /// RED against the pre-#722 code: focusing the right pane would widen
    /// it (reclaiming the now-inactive left pane's minimap width) and
    /// narrow the left pane by the same amount, moving the right pane's
    /// `"line "` column — confirmed by hand by reverting the `minimap_w`
    /// reclaim to the old `is_active`-gated single value before restoring
    /// this fix.
    #[test]
    fn focus_change_does_not_move_either_panes_text_via_shell_app() {
        const WIDTH: u16 = 160;
        const HEIGHT: u16 = 30;

        /// Column of the *second* `"line "` occurrence on the first row
        /// that paints both panes' buffer text — i.e. where the right
        /// pane's own text starts.
        fn right_pane_text_col(screen: &str) -> usize {
            screen
                .lines()
                .find_map(|line| {
                    let mut hits = line.match_indices("line ");
                    hits.next()?;
                    hits.next().map(|(i, _)| i)
                })
                .expect(
                    "a `:vsplit` of the shared fixture must paint a row with \
                     both panes' \"line N\" text",
                )
        }

        let app = app_with_split_shaped_buffer();
        // Read the fixture's own resolved theme before `app` moves into the
        // driver, so the cursor-cell scan below matches whatever colour
        // this fixture actually paints with, ambient colorscheme setting
        // included, rather than a hardcoded theme that could silently
        // drift from it.
        let theme = Theme::from_name(&app.engine.settings.colorscheme);
        let cursor_bg = quadraui::tui::ratatui_color(super::quadraui_tui::q_theme(&theme).cursor);

        let mut driver = driver_with_shell(app, config(), WIDTH, HEIGHT);
        // Warm-up dispatch: the runner's own `AppShell` only picks up the
        // engine's pinned sidebar/autohide state (see
        // `app_with_split_shaped_buffer`'s doc comment) at the tail of a
        // `handle()` call, never on the construction-time first frame. An
        // inert `Escape` forces that sync to happen *before* `before` is
        // captured, so the real `<C-w>w` dispatch below isn't the one that
        // (spuriously) changes the sidebar's painted state.
        driver.press_named(quadraui::NamedKey::Escape);

        fn cursor_cell(
            driver: &quadraui::tui::testing::TuiDriver<impl quadraui::AppLogic>,
            cursor_bg: quadraui::tui::testing::Color,
        ) -> Option<(u16, u16)> {
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    if driver.style_at(x, y).map(|s| s.bg) == Some(cursor_bg) {
                        return Some((x, y));
                    }
                }
            }
            None
        }

        let cell_before = cursor_cell(&driver, cursor_bg)
            .expect("the active pane must paint a block cursor cell");
        let before = driver.screen();
        let col_before = right_pane_text_col(&before);

        driver.ctrl_char('w');
        driver.type_char('w');
        driver.render();

        let cell_after = cursor_cell(&driver, cursor_bg)
            .expect("the newly-active pane must paint a block cursor cell");
        assert_ne!(
            cell_before, cell_after,
            "test setup sanity: `<C-w>w` must actually move focus (and so \
             the painted block-cursor cell) to the other pane, or this \
             test isn't exercising a focus change at all"
        );

        let after = driver.screen();
        let col_after = right_pane_text_col(&after);

        assert_eq!(
            col_before, col_after,
            "cycling focus between panes of a `:vsplit` must not move \
             either pane's text (i.e. must not reflow either pane's text \
             width); before:\n{before}\nafter:\n{after}"
        );
    }

    /// #35: `render_content` must paint the minimap through the shell path,
    /// as braille — not just populate `ScreenLayout.minimap`.
    ///
    /// RED-first: commenting out this `render_content`'s own
    /// `render::draw_minimap_strip(backend, &screen)` call (the ShellApp
    /// path has a separate call site from the legacy `render_impl.rs`
    /// `draw_frame` path — see the module doc's gap list) makes the
    /// non-blank-braille assertion fail — confirmed by hand before
    /// restoring the fix.
    #[test]
    fn render_content_paints_minimap_braille_via_shell_app() {
        let driver = driver_with_shell(app_with_shaped_buffer(), config(), 100, 24);
        let screen = driver.screen();
        assert!(
            screen
                .chars()
                .any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
            "the minimap must paint non-blank braille via the shell path; \
             screen:\n{screen}"
        );
    }

    /// …and `:set nominimap` must take effect on the very next paint, with
    /// no restart: no braille anywhere.
    ///
    /// RED-first: forcing `minimap_reserved_width`'s `has` to ignore
    /// `engine.settings.minimap` (always reserve the strip) makes braille
    /// paint even with the setting off, failing the assertion below —
    /// confirmed by hand before restoring the fix.
    #[test]
    fn render_content_paints_no_minimap_when_the_setting_is_off() {
        let mut app = app_with_shaped_buffer();
        app.engine.settings.minimap = false;
        let driver = driver_with_shell(app, config(), 100, 24);
        let screen = driver.screen();
        assert!(
            !screen
                .chars()
                .any(|c| ('\u{2800}'..='\u{28FF}').contains(&c)),
            "`minimap: false` must paint no braille at all; screen:\n{screen}"
        );
    }

    /// Acceptance (#35): a click at the vertical middle of the strip scrolls
    /// the editor to ~50% of the file — the TUI half of the cross-backend
    /// claim, asserted on the painted line numbers rather than engine state.
    ///
    /// RED-first: with the shell path's `draw_minimap_strip` call commented
    /// out (see `render_content_paints_minimap_braille_via_shell_app`), the
    /// `braille_col` lookup below panics — nothing paints on `mid_row` to
    /// click — confirmed by hand before restoring the fix.
    #[test]
    fn minimap_click_at_the_middle_scrolls_to_half_the_file() {
        let mut driver = driver_with_shell(app_with_shaped_buffer(), config(), 100, 24);

        /// Lowest `line N` number visible on screen — the file's scroll
        /// position, read back out of the paint.
        fn top_line(screen: &str) -> Option<usize> {
            screen
                .split_whitespace()
                .collect::<Vec<_>>()
                .windows(2)
                .filter(|w| w[0] == "line")
                .filter_map(|w| w[1].parse::<usize>().ok())
                .min()
        }

        let before = driver.screen();
        assert_eq!(
            top_line(&before),
            Some(0),
            "fixture must start at the top of the file; screen:\n{before}"
        );

        // Find the strip on a row it actually paints, then click its middle.
        let mid_row = 12u16;
        let x = braille_col(&before, mid_row as usize).unwrap_or_else(|| {
            panic!("the minimap must paint braille on row {mid_row}; screen:\n{before}")
        });
        driver.click(x as f32 + 1.0, mid_row as f32);
        driver.render();

        let after = driver.screen();
        let top = top_line(&after)
            .unwrap_or_else(|| panic!("the editor must still paint line numbers:\n{after}"));
        let frac = top as f64 / 240.0;
        assert!(
            (0.3..0.7).contains(&frac),
            "clicking the middle of the minimap must scroll to ~50% of the \
             240-line file, landed on line {top} ({frac:.3}); screen:\n{after}"
        );
    }

    /// `app_with_shaped_buffer` with the sidebar forced off (mirrors
    /// `app_with_split_shaped_buffer`'s own doc comment on why: sidebar
    /// visibility is otherwise ambient, read off the developer's real
    /// `~/.config/vimcode`). The two tests below key off the *column* a
    /// scrollbar glyph paints at, and the explorer tree view paints its
    /// own `'█'`/`'░'` scrollbar too — indistinguishable by character from
    /// the editor's/minimap's — so it must be off, not just "usually
    /// absent", for those tests to reliably find the right column.
    fn app_with_shaped_buffer_no_sidebar() -> TuiShellApp {
        let mut app = app_with_shaped_buffer();
        app.engine.settings.autohide_panels = false;
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        app
    }

    /// Buffer short enough to fit entirely within the viewport — the other
    /// half of #723's acceptance: nothing overflows, so no scrollbar (and no
    /// minimap thumb) should paint anywhere. Sidebar forced off for the same
    /// reason as `app_with_shaped_buffer_no_sidebar`.
    fn app_with_short_buffer() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.autohide_panels = false;
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        let text: String = (0..5).map(|i| format!("line {i}\n")).collect();
        app.engine.buffer_mut().insert(0, &text);
        app
    }

    /// #723 acceptance (TUI half): with the minimap on and a file longer
    /// than the viewport, the pane shows **exactly one** vertical scroll
    /// affordance, and it sits *beside* the strip rather than on top of it —
    /// one column of `'█'`/`'░'` (quadraui's `tui::draw_editor` scrollbar),
    /// with the minimap's braille starting in the very next column.
    ///
    /// This is the invariant the first attempt at #723 broke: painting
    /// `MinimapLayout.scrollbar` over the strip via `Backend::draw_scrollbar`
    /// put a second solid bar in the strip's leftmost column, directly
    /// against the editor's own — two bars jammed together, which is exactly
    /// the operator-visible defect
    /// `render_impl::tests::test_tui_two_groups_single_boundary_scrollbar_481`
    /// exists to prevent (it went from 2 scrollbar columns to 4). The strip's
    /// own scroll feedback is quadraui's `viewport_highlight` band — a
    /// *background* accent across the visible rows, painted by both
    /// rasterisers — not a second foreground bar.
    ///
    /// RED against the reverted state: with `draw_minimap_strip` calling
    /// `draw_scrollbar`, the column right of the editor's scrollbar is a
    /// second `'░'`/`'█'` instead of braille, and the "exactly one" count is
    /// 2. Verified by hand by restoring that call.
    #[test]
    fn minimap_strip_does_not_double_the_scrollbar_via_shell_app() {
        let mut driver = driver_with_shell(app_with_shaped_buffer_no_sidebar(), config(), 100, 24);
        // Warm-up dispatch: the runner's own `AppShell` only picks up the
        // engine's pinned sidebar/autohide state at the tail of a
        // `handle()` call, never on the construction-time first frame
        // (see `app_with_shaped_buffer_no_sidebar`'s doc comment).
        driver.press_named(quadraui::NamedKey::Escape);
        let screen = driver.screen();

        fn is_scrollbar_glyph(c: char) -> bool {
            c == '█' || c == '░'
        }
        // Braille block: what `tui::draw_minimap` packs its dot cells from.
        fn is_braille(c: char) -> bool {
            ('\u{2800}'..='\u{28FF}').contains(&c)
        }

        let row = 15usize;
        let line: Vec<char> = screen
            .lines()
            .nth(row)
            .unwrap_or_else(|| panic!("row {row} must exist; screen:\n{screen}"))
            .chars()
            .collect();

        let sb_cols: Vec<usize> = line
            .iter()
            .enumerate()
            .filter(|(_, c)| is_scrollbar_glyph(**c))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            sb_cols.len(),
            1,
            "a pane with the minimap on must show exactly one vertical \
             scrollbar column on row {row}, got {sb_cols:?}; screen:\n{screen}"
        );

        let next = line.get(sb_cols[0] + 1).copied();
        assert!(
            next.is_some_and(is_braille),
            "the minimap strip must begin in the column immediately right of \
             the scrollbar ({}), painting braille rather than a second bar; \
             got {next:?}; screen:\n{screen}",
            sb_cols[0]
        );
    }

    /// #723 acceptance, the other half: a file that fits entirely within the
    /// viewport must paint no scroll affordance anywhere — neither the
    /// editor's own scrollbar nor a thumb over the minimap strip.
    #[test]
    fn minimap_paints_no_scroll_thumb_when_the_file_fits_via_shell_app() {
        let mut driver = driver_with_shell(app_with_short_buffer(), config(), 100, 24);
        // Warm-up dispatch — see `minimap_paints_a_scroll_thumb_when_the_file_overflows_via_shell_app`.
        driver.press_named(quadraui::NamedKey::Escape);
        let screen = driver.screen();
        assert!(
            !screen.contains('█') && !screen.contains('░'),
            "a file that fits entirely within the viewport must paint no \
             scroll thumb/track glyphs anywhere; screen:\n{screen}"
        );
    }

    // ── #733 slice 1: the shared modal-overlay mouse rung ────────────────
    //
    // `handle_mouse`'s top rung is now `render::route_modal_overlay_click`,
    // the same function `src/gtk/mod.rs::handle_mouse_click_msg` calls. The
    // rung this backend was missing entirely is the Ctrl+Tab switcher: TUI
    // painted the popup but had no mouse arm for it, so a click fell
    // straight through to the editor underneath.

    /// Two real file tabs so `open_tab_switcher` has more than one MRU
    /// entry (it no-ops with one) and the editor underneath has enough
    /// lines that a stray click would visibly move the cursor.
    ///
    /// Sidebar/autohide state is pinned explicitly for the reason
    /// `app_with_sidebar_open`'s doc comment spells out (#634):
    /// `TuiShellApp::new` reads the developer's real `~/.config/vimcode`
    /// off disk, so an explorer that was ever opened on this machine makes
    /// the sidebar boot *visible*. Both tests below hit-test against
    /// painted geometry — the popup's centred rect and a column the
    /// "outside" click must land on — and an ambient sidebar shifts the
    /// editor pane right underneath both of them, turning the outside
    /// click into an explorer click (which opens a file rather than moving
    /// the cursor). Pinned, the geometry is the same on a bare CI runner
    /// and a developer box.
    fn app_with_two_file_tabs_and_switcher_open() -> TuiShellApp {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_733_tab_switcher_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file_a = dir.join("a733.txt");
        let file_b = dir.join("b733.txt");
        let content: String = (0..40).map(|i| format!("AAA733 line {}\n", i)).collect();
        std::fs::write(&file_a, &content).unwrap();
        std::fs::write(&file_b, &content).unwrap();

        let mut app = TuiShellApp::new(None);
        app.engine.settings.autohide_panels = false;
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        app.engine
            .open_file_with_mode(&file_a, crate::core::engine::OpenMode::Permanent)
            .unwrap();
        app.engine.new_tab(Some(&file_b));
        app.engine.open_tab_switcher();
        assert!(
            app.engine.tab_switcher_open,
            "fixture must actually open the tab switcher"
        );
        app
    }

    /// #733 acceptance, TUI half: a click that lands inside the painted
    /// tab-switcher popup must dismiss it **and be consumed**, so the
    /// editor underneath never sees it.
    ///
    /// Both halves are read off the painted frame — the popup's own
    /// "Open Tabs" title (located with `find_bounds`, never a hardcoded
    /// coordinate) and the status line's `Ln N, Col N` readout, which is
    /// where a leaked editor click shows up.
    ///
    /// RED against unfixed `develop`: `handle_mouse` had no tab-switcher
    /// arm at all, so the click reached `dispatch_left_click`, the popup
    /// stayed open (first assertion fails) and the cursor jumped to the
    /// clicked row (second assertion fails).
    #[test]
    fn driver_click_inside_tab_switcher_popup_dismisses_and_is_consumed() {
        let mut driver = driver_with_shell(
            app_with_two_file_tabs_and_switcher_open(),
            config(),
            100,
            24,
        );
        let title = driver
            .find_bounds("Open Tabs")
            .expect("the tab-switcher popup must paint its title");
        assert!(
            driver.screen_contains("Ln 1, Col 1"),
            "precondition: the cursor starts on line 1; screen:\n{}",
            driver.screen()
        );

        // Warm-up dispatch: `last_layout` (which the editor click path
        // hit-tests against) is only populated by a render that follows a
        // dispatch, so without this a first click never reaches the editor
        // at all and the "was it consumed?" assertion below could not fail.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();

        // Two rows below the title sits the popup's item area — inside the
        // popup, and over the editor text underneath it.
        driver.click(title.x + 1.0, title.y + 2.0);

        let screen = driver.screen();
        assert!(
            !screen.contains("Open Tabs"),
            "a click inside the tab-switcher popup must dismiss it; screen:\n{screen}"
        );
        assert!(
            screen.contains("Ln 1, Col 1"),
            "the click must be consumed by the popup, not leak through to \
             the editor and move the cursor; screen:\n{screen}"
        );
    }

    /// The complementary half: a click *outside* the popup still dismisses
    /// it, but propagates — the editor takes the click and the cursor
    /// moves. Without this, the router could satisfy the test above by
    /// swallowing every click while the switcher is open.
    #[test]
    fn driver_click_outside_tab_switcher_popup_dismisses_and_propagates() {
        let mut driver = driver_with_shell(
            app_with_two_file_tabs_and_switcher_open(),
            config(),
            100,
            24,
        );
        let title = driver
            .find_bounds("Open Tabs")
            .expect("the tab-switcher popup must paint its title");
        assert!(
            driver.screen_contains("Ln 1, Col 1"),
            "precondition: the cursor starts on line 1"
        );

        // Warm-up dispatch: `last_layout` (which the editor click path
        // hit-tests against) is only populated by a render that follows a
        // dispatch, so without this a first click never reaches the editor
        // at all and the "was it consumed?" assertion below could not fail.
        driver.dispatch(quadraui::UiEvent::WindowFocused(true));
        driver.render();

        // Column 20 is editor text (right of the activity bar + gutter),
        // and still well left of the popup, which is centred in a
        // 100-column terminal with a 45-column body (left edge = 27).
        driver.click(20.0, title.y + 2.0);
        driver.render();

        let screen = driver.screen();
        assert!(
            !screen.contains("Open Tabs"),
            "a click outside the tab-switcher popup must dismiss it too; \
             screen:\n{screen}"
        );
        assert!(
            !screen.contains("Ln 1, Col 1"),
            "an outside click must propagate to the editor underneath and \
             move the cursor off line 1; screen:\n{screen}"
        );
    }

    // ── #734 slice 1: the shared modal keyboard rung ─────────────────────
    //
    // `handle_key_pressed`'s top rung is now `render::route_modal_key`, the
    // same function `src/gtk/mod.rs::handle_key_press` calls. Both tests
    // below focus the activity bar first, because that focus tier used to
    // sit ABOVE the modal it is now beaten by — the concrete shape of the
    // "the ladder is transcribed, not shared" defect #734 exists to fix.

    /// A misspelled word with a pending spell-suggestion prompt, and the
    /// activity bar holding keyboard focus.
    ///
    /// `spell_suggestions` is set through `Engine`'s own tuple shape rather
    /// than `spell_suggest_under_cursor` so the fixture does not depend on a
    /// dictionary being installed on the test machine.
    ///
    /// Sidebar/autohide pinned for the #634 reason `app_with_sidebar_open`
    /// documents: `TuiShellApp::new` reads the developer's real
    /// `~/.config/vimcode`, so the sidebar boots visible on a box that has
    /// ever opened the explorer and hidden on a bare CI runner. Neither
    /// test here should depend on which.
    fn app_with_spell_suggestions_and_activity_bar_focus() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.autohide_panels = false;
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        app.engine.buffer_mut().insert(0, "teh end\n");
        app.engine.spell_suggestions =
            Some(("teh".to_string(), vec!["the".to_string()], String::new()));
        app.engine.activity_bar_focus_in_at(0);
        assert!(
            app.engine.activity_bar_focused,
            "fixture must actually focus the activity bar"
        );
        app
    }

    /// #734 acceptance, TUI half: spell-suggestion selection is modal — it
    /// must beat the activity-bar focus tier, not sit below it.
    ///
    /// Asserted on the painted frame: the misspelled word is replaced in the
    /// editor text and the engine's confirmation message reaches the status
    /// line. Nothing here reads `spell_suggestions` directly.
    ///
    /// RED against unfixed `develop`: `handle_key_pressed` reached the
    /// spell-suggestion branch only via `Engine::handle_key` at the very
    /// bottom of the ladder, so the activity-bar tier swallowed `'1'` in its
    /// `_ => {}` arm and the buffer still read "teh end".
    #[test]
    fn driver_spell_suggestion_key_beats_a_focused_activity_bar() {
        let mut driver = driver_with_shell(
            app_with_spell_suggestions_and_activity_bar_focus(),
            config(),
            100,
            24,
        );
        assert!(
            driver.screen_contains("teh end"),
            "precondition: the misspelled word paints; screen:\n{}",
            driver.screen()
        );

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('1'),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });

        let screen = driver.screen();
        assert!(
            screen.contains("the end"),
            "'1' must reach the spell-suggestion handler and apply the \
             correction; screen:\n{screen}"
        );
        assert!(
            screen.contains("Changed"),
            "the engine's confirmation message must reach the status line; \
             screen:\n{screen}"
        );
    }

    /// The context-menu twin: an open context menu is modal too, so Escape
    /// must dismiss *it* rather than being spent unfocusing the activity
    /// bar underneath.
    ///
    /// RED against unfixed `develop`: the context-menu tier sat near the
    /// BOTTOM of `handle_key_pressed` (and a second copy inside
    /// `handle_focus_owner_key`), both below the activity-bar tier, so
    /// Escape ran `activity_bar_focus_out()` and the menu stayed painted.
    #[test]
    fn driver_context_menu_key_beats_a_focused_activity_bar() {
        let mut app = TuiShellApp::new(None);
        // Sidebar pinned for the same #634 reason as the fixture above.
        app.engine.settings.autohide_panels = false;
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        app.engine.open_editor_context_menu(20, 5);
        app.engine.activity_bar_focus_in_at(0);
        let mut driver = driver_with_shell(app, config(), 100, 24);
        assert!(
            driver.screen_contains("Paste"),
            "precondition: the context menu paints its always-enabled Paste \
             item; screen:\n{}",
            driver.screen()
        );

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Named(quadraui::NamedKey::Escape),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });

        let screen = driver.screen();
        assert!(
            !screen.contains("Paste"),
            "Escape must dismiss the modal context menu, not be eaten by the \
             focused activity bar underneath; screen:\n{screen}"
        );
    }

    // ─── #751: the modal rung, finished (context menu / picker / find-replace)
    //
    // TUI twins of the `modal_rung` module in `src/gtk/testing.rs`. Each rung
    // is arbitrated once, by `render::route_modal_overlay_click`.

    /// A frame with both the unified picker and a context menu open. A click
    /// on a menu item must fire the menu, because `render::OVERLAY_Z_ORDER`
    /// paints the context menu *above* the picker.
    ///
    /// **RED-verified against unfixed `develop`.** `handle_mouse` arbitrated
    /// the picker ~1,100 lines before the context menu and returned
    /// unconditionally once `engine.picker_open` was set, so the click was
    /// swallowed as a picker outside-click: "Paste" stayed on screen and the
    /// palette closed instead. Reinstating that order — moving the
    /// `ContextMenu` arm of `route_modal_overlay_click` below its
    /// `UnifiedPicker` arm — reproduces it. (The companion
    /// `render::tests::mouse_arbitration_is_the_inverse_of_the_paint_z_order`
    /// guards the *declared* order in `MOUSE_ARBITRATION_ORDER`; this one
    /// guards that the code actually follows it.) Restored before committing.
    #[test]
    fn context_menu_click_outranks_the_open_picker_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        app.engine
            .open_picker(crate::core::engine::PickerSource::LineEndings);
        // Anchored inside the centred palette, so the two overlays genuinely
        // overlap and the ladder has to arbitrate rather than luck out on
        // disjoint geometry.
        app.engine.open_editor_context_menu(45, 10);

        let mut driver = driver_with_shell(app, config(), 100, 24);
        let paste = driver
            .find_bounds("Paste")
            .expect("the context menu must paint its always-enabled Paste item");

        driver.click(paste.x + 1.0, paste.y);

        let screen = driver.screen();
        assert!(
            !screen.contains("Paste"),
            "the click must confirm the context-menu item and close the menu, \
             not be swallowed by the picker painted underneath it; \
             screen:\n{screen}"
        );
    }

    /// TUI twin of
    /// `gtk::testing::modal_rung::find_replace_toggle_click_lands_where_the_panel_painted_via_gtk_driver`:
    /// a click on the painted case-sensitivity toggle flips it, and the panel
    /// repaints the toggle in its active style.
    ///
    /// Not RED on this backend — TUI's own hit test already mirrored its
    /// rasteriser. It is here because the rung is now shared: if
    /// `render::FindReplaceHitGeometry::from_panel` is changed to suit GTK, or
    /// `TUI_FIND_REPLACE_ANCHOR` drifts from `quadraui::tui::find_replace`,
    /// this is what catches it before a user does.
    #[test]
    fn find_replace_toggle_click_lands_where_the_panel_painted_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        app.engine.find_replace_open = true;
        app.engine.find_replace_query = "ZQXW751FR".to_string();

        let mut driver = driver_with_shell(app, config(), 100, 24);
        assert!(
            driver.screen_contains("ZQXW751FR"),
            "fixture must actually paint the find/replace panel; screen:\n{}",
            driver.screen()
        );
        let toggle = driver
            .find_bounds("Aa")
            .expect("the panel must paint its Aa case-sensitivity toggle");
        let before = driver.screen();

        driver.click(toggle.x, toggle.y);

        let after = driver.screen();
        assert_ne!(
            before, after,
            "clicking the painted Aa toggle must repaint the panel; nothing \
             changed, so the click hit-tested somewhere the panel is not"
        );
        assert!(
            after.contains("ZQXW751FR"),
            "the toggle click must not close or displace the panel; \
             screen:\n{after}"
        );
    }

    /// A click on the *already selected* palette row confirms it and closes
    /// the palette — the behaviour `render::apply_picker_row_click` now states
    /// once for both backends (its GTK twin is
    /// `second_click_on_a_picker_row_confirms_it_via_gtk_driver`, which *is*
    /// RED against unfixed `develop`).
    ///
    /// Not RED on this backend: TUI already behaved this way, and that is
    /// precisely why it is worth pinning — the shared helper now has to keep
    /// doing so while GTK is brought onto it.
    #[test]
    fn second_click_on_a_picker_row_confirms_it_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        app.engine
            .open_picker(crate::core::engine::PickerSource::LineEndings);
        assert_eq!(
            app.engine.picker_selected, 0,
            "fixture assumes the palette opens on row 0, so row 1 is a \
             not-yet-selected row"
        );
        let title = app.engine.picker_title.clone();
        let row1_label = app.engine.picker_items[1].display.clone();

        let mut driver = driver_with_shell(app, config(), 100, 24);
        // Locate the row by the text it painted, never by arithmetic on the
        // popup's origin (`CLAUDE.md` rule 1).
        let row1 = driver
            .find_bounds(&row1_label)
            .unwrap_or_else(|| panic!("the palette must paint its {row1_label:?} row"));

        driver.click(row1.x, row1.y);
        assert!(
            driver.screen_contains(&title),
            "the first click only selects — the palette must still be painted; \
             screen:\n{}",
            driver.screen()
        );

        driver.click(row1.x, row1.y);
        let screen = driver.screen();
        assert!(
            !screen.contains(&title),
            "a second click on the already-selected row must confirm it and \
             close the palette; screen:\n{screen}"
        );
    }

    /// Black-box regression for #815 (adopting
    /// `quadraui::FolderPickerController`, replacing the deleted TUI-local
    /// `FolderPickerState`): the open picker must actually *paint* its
    /// entries through `FolderPickerController::render`
    /// (`FrameOp::FolderPicker`'s arm in `render_content`), typing must
    /// reach `FolderPickerController::handle` and filter the list, and Esc
    /// must dismiss it — the three things #815 rewired.
    ///
    /// This fails to even *compile* against unfixed `develop`: pre-#815,
    /// `TuiShellApp::folder_picker` was `Option<FolderPickerState>`, a type
    /// this test never mentions and that has none of
    /// `FolderPickerController`'s public API (`FolderPickerController::new`
    /// with an `extra_file_names` argument, `PALETTE_CHROME_ROWS`-based
    /// chrome, `.gitkeep`-free fuzzy filtering identical to the demo app).
    #[test]
    fn folder_picker_paints_and_filters_via_shell_app() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_815_shell_app_folder_picker_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("distinctive_child_dir_815")).unwrap();
        std::fs::create_dir_all(dir.join("another_unrelated_dir_815")).unwrap();

        let mut app = TuiShellApp::new(None);
        app.folder_picker = Some(quadraui::FolderPickerController::new(
            dir.clone(),
            vec![],
            false,
        ));

        let mut driver = driver_with_shell(app, config(), 100, 24);
        assert!(
            driver.screen_contains("distinctive_child_dir_815"),
            "the open picker must paint its entries via \
             `FolderPickerController::render`; screen:\n{}",
            driver.screen()
        );
        assert!(
            driver.screen_contains("another_unrelated_dir_815"),
            "screen:\n{}",
            driver.screen()
        );

        // Type a query that only matches one of the two child directories.
        for c in "distinctive".chars() {
            driver.type_char(c);
        }
        assert!(
            driver.screen_contains("distinctive_child_dir_815"),
            "typing must reach `FolderPickerController::handle` and keep \
             matching entries visible; screen:\n{}",
            driver.screen()
        );
        assert!(
            !driver.screen_contains("another_unrelated_dir_815"),
            "typing a query that only matches one entry must filter the \
             other one out; screen:\n{}",
            driver.screen()
        );

        driver.press_named(quadraui::NamedKey::Escape);
        assert!(
            !driver.screen_contains("distinctive_child_dir_815"),
            "Esc must dismiss the picker; screen:\n{}",
            driver.screen()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Companion to the paint/filter test above: confirming a highlighted
    /// entry must call `Engine::open_folder` with *that* entry's path, not
    /// just close the picker. Exercised directly against `handle_key_pressed`
    /// (bypassing `driver_with_shell`, like the dialog/context-menu tests
    /// elsewhere in this module) because `TuiDriver` has no accessor back to
    /// the concrete `Engine` to assert `cwd` against.
    #[test]
    fn folder_picker_enter_confirms_and_opens_the_selected_folder() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_815_confirm_folder_picker_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let child = dir.join("confirm_target_815");
        std::fs::create_dir_all(&child).unwrap();

        let mut engine = Engine::new();
        let mut sidebar = TuiSidebar::new();
        let mut folder_picker = Some(quadraui::FolderPickerController::new(
            dir.clone(),
            vec![],
            false,
        ));
        let mut backend = backend_at(80.0, 24.0);
        let mut scratch = KeyScratch::new();

        // Entries sort as [\"..\", \".\", \"confirm_target_815\"] — move down
        // twice to reach the child dir, then confirm it.
        for _ in 0..2 {
            scratch.set_key(
                quadraui::Key::Named(quadraui::NamedKey::Down),
                quadraui::Modifiers::default(),
                false,
            );
            handle_key_pressed(
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
        }

        scratch.set_key(
            quadraui::Key::Named(quadraui::NamedKey::Enter),
            quadraui::Modifiers::default(),
            false,
        );
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
        assert!(folder_picker.is_none(), "Enter must close the picker");
        let expected = child.canonicalize().unwrap_or_else(|_| child.clone());
        assert_eq!(
            engine.cwd, expected,
            "Enter on the highlighted entry must open *that* folder"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─── #752 / #733 slice 2: the chrome rung ────────────────────────────
    //
    // The TUI half of the cross-backend pair; the GTK half is
    // `gtk::testing::chrome_rung`. Breadcrumbs and all three status bands
    // are now sequenced by `render::route_chrome_click`, which both
    // `handle_mouse` and `App::handle_mouse_click_msg` call.

    /// An app whose **global** (bottom-of-screen) status bar paints, with a
    /// git branch decorated by ahead/behind counts.
    ///
    /// `window_status_line = false` is what makes `build_screen_layout` emit
    /// `global_status_bar` instead of per-window lines — the two are
    /// mutually exclusive.
    fn app_with_global_status_branch() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.window_status_line = false;
        app.engine.git_branch = Some("ZQXW752BR".to_string());
        app.engine.sc_ahead = 2;
        app.engine.sc_behind = 1;
        app
    }

    /// #752: clicking the git branch in the global status bar opens the
    /// branch picker.
    ///
    /// # Why this is red against unfixed `develop`
    ///
    /// TUI did not route this row at all. `handle_mouse` swallowed it with
    ///
    /// ```ignore
    /// // Global status bar row — consume click (no interactive segments).
    /// if row + 2 == term_height && !engine.settings.window_status_line && col >= ab_width {
    ///     return sidebar_width;
    /// }
    /// ```
    ///
    /// — a comment that had stopped being true: GTK routed the branch (badly;
    /// see the GTK twin), TUI routed nothing. Restore that early return and
    /// the picker never opens.
    ///
    /// The click target is located with `find`, i.e. from the **painted cell
    /// grid**, so it is the column the user actually sees — never a
    /// re-derivation of the range the production hit test uses.
    #[test]
    fn global_status_branch_click_opens_the_branch_picker_via_shell_app() {
        let mut driver = driver_with_shell(app_with_global_status_branch(), config(), 100, 24);
        assert!(
            driver.screen_contains("ZQXW752BR"),
            "fixture must actually paint the branch in the global status bar; screen:\n{}",
            driver.screen()
        );

        let (x, y) = driver
            .find("ZQXW752BR")
            .expect("the branch just asserted to be on screen must be locatable");
        driver.click(x, y);

        let screen = driver.screen();
        assert!(
            screen.contains("Switch Branch"),
            "clicking the painted branch must open the branch picker, whose \
             painted title is its own evidence; screen:\n{screen}"
        );
    }

    /// #752 companion: the **per-window** status line still fires its
    /// segments now that it is routed as a `render::StatusBand` from the
    /// shared rung, rather than by a bespoke arm buried ~250 lines deep in
    /// `handle_mouse`'s window walk.
    ///
    /// Not RED on this backend — TUI's own arm worked. It is here because
    /// that arm is gone: three status arms (per-window, separated, global)
    /// collapsed into one shared band walk, and this is what catches it if
    /// `window_status_line_zones` or the band's rect arithmetic drifts from
    /// what `render_window_status_line` paints.
    #[test]
    fn window_status_line_segment_click_still_fires_via_shell_app() {
        let mut app = TuiShellApp::new(None);
        // The default, but stated: this is the setting that selects the
        // per-window bar over the global one.
        app.engine.settings.window_status_line = true;
        app.engine.buffer_mut().insert(0, "alpha\nbeta\ngamma\n");
        // `Engine::new` picks the real repo's branch up, and on a long branch
        // name `TuiBackend::draw_status_bar` truncates the painted segment
        // while `StatusBar::layout`'s `chars().count()` measure does not — a
        // pre-existing TUI paint/hit-test drift, unrelated to this rung and
        // older than it (the deleted `status_segment_hit_test` measured the
        // same way). Cleared so this test asserts on the rung, not on that.
        app.engine.git_branch = None;

        let mut driver = driver_with_shell(app, config(), 100, 24);
        let (x, y) = driver
            .find("Ln 1, Col 1")
            .expect("the per-window status line must paint its cursor segment");

        driver.click(x, y);

        let screen = driver.screen();
        assert!(
            screen.contains("Go to Line") || screen.contains("Command"),
            "clicking the cursor-position segment must open the go-to-line \
             palette (`StatusAction::GoToLine`); screen:\n{screen}"
        );
    }

    /// #752: clicking a tab in an *unsplit* window must switch the painted
    /// editor pane to that tab's own buffer — regression coverage for the
    /// single-group arm of `handle_mouse` now delegating to
    /// `Engine::handle_tab_bar_click` (the split-group arm a few lines above
    /// it, and GTK's `dispatch_tab_bar_target`, already did) instead of
    /// hand-rolling `TabBarClickTarget` dispatch, so there is exactly one
    /// place this rung can drift from again.
    ///
    /// It is also the regression test for the geometry half of that arm:
    /// the hit test now measures against `GroupTabBar::bounds` — the rect
    /// this frame *painted* — instead of re-deriving `menu_rows` /
    /// `editor_left` from live engine state at click time.
    ///
    /// # RED-verification note
    ///
    /// **Dispatch half — not RED.** The removed hand-rolled arm's `Tab(i)`
    /// case still called `engine.goto_tab(i)`, which has unconditionally
    /// called `self.lsp_ensure_active_buffer()` internally since long before
    /// this PR (`windows.rs`, unchanged history). Restoring that exact
    /// removed code and re-running this test leaves it green: for a single,
    /// always-active group the omitted explicit `active_group = group_id`
    /// assignment is a no-op (there is only one group to point at), so this
    /// input does not reproduce the LSP-desync the fix's comment describes.
    /// Verified by restoring the removed arm and re-running, not asserted
    /// from reading the diff. There is no live LSP server under
    /// `driver_with_shell` to assert on directly, and
    /// `lsp_ensure_active_buffer` no-ops for a `.txt` fixture with no
    /// configured language server regardless.
    ///
    /// **Geometry half — RED.** Restore `let local_col = rel_col;` (i.e.
    /// `col - editor_left`) with the `row == menu_rows` gate in
    /// `mouse.rs`'s single-group arm and this test fails on the first
    /// assertion below, on any machine: the pinned precondition is a
    /// *hidden shadow sidebar* with the runner still booted on the
    /// hamburger panel, so the very click under test reconciles the runner
    /// (`take_requested_panel`) and collapses the painted sidebar. Live
    /// `editor_left` therefore drops by the sidebar's whole width *during*
    /// the click, `rel_col` lands ~31 columns right of the tab the user
    /// actually clicked, and the tab switch is silently swallowed — the
    /// user has to click the tab twice. Painted `bounds.x` does not move.
    ///
    /// # Why the sidebar state is pinned (#634)
    ///
    /// Sidebar visibility on a bare `TuiShellApp::new` is *ambient* — see
    /// `app_with_sidebar_open`'s doc comment. This test hides it explicitly
    /// for the opposite reason that fixture shows it: on a developer box
    /// whose session has the explorer open, the runner reconciles onto
    /// Explorer, the painted sidebar never collapses and the pre-fix bug
    /// does not reproduce at all. Pinning it here is what makes the RED
    /// note above true everywhere rather than only on a fresh checkout.
    #[test]
    fn tab_click_in_unsplit_window_switches_active_buffer_via_shell_app() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_752_tab_click_shell_app_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file_a = dir.join("a752.txt");
        let file_b = dir.join("b752.txt");
        std::fs::write(&file_a, "AAA752 content\n").unwrap();
        std::fs::write(&file_b, "BBB752 content\n").unwrap();

        let mut app = TuiShellApp::new(None);
        // Pin the ambient sidebar state (#634, see the doc comment above):
        // shadow sidebar hidden, while the runner's own `AppShell` still
        // boots on the hamburger panel with its sidebar shown. That is the
        // state a fresh checkout starts in, and it is what makes the click
        // under test change the painted geometry mid-dispatch.
        app.engine.app_shell.hide_sidebar();
        app.engine.session.explorer_visible = false;
        app.engine
            .open_file_with_mode(&file_a, crate::core::engine::OpenMode::Permanent)
            .unwrap();
        app.engine.new_tab(Some(&file_b));
        assert_eq!(
            app.engine.group_layout.leaf_count(),
            1,
            "this test covers the unsplit case"
        );
        assert_eq!(
            app.engine.active_group().active_tab,
            1,
            "opening tab B makes it the active tab"
        );

        // The live config (`TuiShellApp::shell_config`), not the bare
        // single-panel `config()` test helper: it seeds
        // `default_sidebar_width` from the same `SIDEBAR_WIDTH` constant
        // `mouse.rs`'s own click math reads back via `self.sidebar_width`
        // (`shell_config`'s doc comment above, #634). With the bare helper
        // the first painted frame lays the sidebar out at quadraui's
        // built-in default (20 cols) while the click math assumes 30,
        // so the tab bar's clickable region and its painted column
        // disagree by the difference — a test-fixture-only trap, not a
        // production bug (the real runner's `ShellConfig` always goes
        // through `shell_config`).
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 100, 24);
        assert!(
            driver.screen().contains("BBB752"),
            "fixture must start out on tab B's content; screen:\n{}",
            driver.screen()
        );

        let (x, y) = driver
            .find("a752.txt")
            .expect("tab A's label must be painted in the (unsplit) tab bar");
        driver.click(x, y);

        let screen = driver.screen();
        assert!(
            screen.contains("AAA752"),
            "clicking tab A's label must switch the painted editor pane to \
             its content; screen:\n{screen}"
        );
        assert!(
            !screen.contains("BBB752"),
            "the editor pane must not still be showing tab B's stale \
             content after the click; screen:\n{screen}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── #755 slice 5: the shared editor hover popup rung ──────────────────
    //
    // Link click, scrollbar grab, focus-or-select and dismiss-on-outside used
    // to be four blocks here and four differently-ordered, differently-behaving
    // blocks on GTK. They are now `render::route_editor_hover_popup_click` +
    // `render::apply_editor_hover_popup_route`, which both backends call. The
    // GTK twins live in `src/gtk/testing.rs::editor_mouse_rungs`.

    /// A `TuiShellApp` showing an editor hover popup whose body carries a
    /// `command:` link, plus a distinctive body word to assert the popup's
    /// presence on the painted grid with.
    fn app_with_editor_hover_link() -> TuiShellApp {
        let mut app = TuiShellApp::new(None);
        app.engine.settings.use_nerd_fonts = false;
        crate::icons::set_nerd_fonts(false);
        app.engine.session.explorer_visible = false;
        app.engine.buffer_mut().insert(0, "fn main() {}\n");
        app.engine.show_editor_hover(
            0,
            3,
            "HOVERBODY755\n\n[Gotodef755](command:definition)",
            crate::core::engine::EditorHoverSource::Lsp,
            false,
            false,
        );
        app
    }

    /// #272/#491 acceptance: clicking a `command:` link inside the editor
    /// hover popup must navigate **and close the popup**. TUI's bespoke arm
    /// ran `execute_hover_goto` and then left the popup up, covering the
    /// definition it had just jumped to; GTK's dismissed. The shared rung
    /// dismisses on both.
    ///
    /// **Not RED on TUI** (`CLAUDE.md` rule 2, stated honestly): reinstating
    /// the pre-#755 arm — `execute_hover_goto` with no dismiss — keeps this
    /// green, because `execute_hover_goto`'s own navigation already tears the
    /// popup down for a `command:definition` URI. The explicit dismiss in the
    /// shared rung matters for the `command:` URIs that *don't* navigate, and
    /// this test is the regression pin for the rung as a whole, which TUI had
    /// no black-box coverage of at all. The RED half of this slice is on GTK
    /// (`src/gtk/testing.rs::editor_mouse_rungs`, verified against both the
    /// missing double-click call and the hardcoded `grab_offset: 0.0`).
    ///
    /// The link's cell is located with `TuiDriver::find`, never hardcoded.
    #[test]
    fn driver_click_on_editor_hover_command_link_navigates_and_closes_the_popup() {
        let mut driver = driver_with_shell(
            app_with_editor_hover_link(),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        assert!(
            driver.screen_contains("HOVERBODY755"),
            "precondition: the hover popup body must paint; screen:\n{}",
            driver.screen()
        );
        let (x, y) = driver
            .find("Gotodef755")
            .expect("the popup's command link label must be painted");

        driver.click(x, y);

        assert!(
            !driver.screen_contains("HOVERBODY755"),
            "clicking a `command:` link in the editor hover popup must close \
             the popup so it does not cover the definition just jumped to \
             (#272/#491); screen:\n{}",
            driver.screen()
        );
    }

    /// The other half of the same rung: a press that lands **outside** a
    /// visible popup dismisses it *and* falls through, so the cursor still
    /// lands where the user aimed instead of costing them a second click.
    /// `EditorHoverPopupRoute::DismissAndFallThrough` is the only route that
    /// reports `consumed == false`, and this pins that it stays that way on
    /// the painted surface.
    #[test]
    fn driver_click_outside_editor_hover_popup_dismisses_it_and_falls_through() {
        let mut driver = driver_with_shell(
            app_with_editor_hover_link(),
            TuiShellApp::shell_config(false),
            80,
            24,
        );
        let (bx, by) = driver
            .find("HOVERBODY755")
            .expect("precondition: the hover popup body must paint");
        // Far from the popup, on the editor's own text row.
        let (tx, ty) = driver
            .find("fn main() {}")
            .expect("the buffer line must be painted");
        assert!(
            (tx, ty) != (bx, by),
            "the buffer line and the popup body must be distinct cells"
        );

        driver.click(tx, ty);

        assert!(
            !driver.screen_contains("HOVERBODY755"),
            "a click outside a visible editor hover popup must dismiss it; \
             screen:\n{}",
            driver.screen()
        );
    }

    // ── #757 slice 2: the shared focus-owner keyboard rung ─────────────
    //
    // `render::route_focus_key` now states the activity-bar → sidebar-panel
    // ladder once for both backends. Its GTK twins live in
    // `src/gtk/testing.rs` (`palette_outranks_a_focused_explorer_on_gtk`,
    // `focused_terminal_outranks_a_focused_explorer_on_gtk`,
    // `focused_plugin_panel_outranks_a_stale_explorer_flag_on_gtk`,
    // `visible_settings_panel_outranks_a_stale_explorer_flag_on_gtk`).

    /// An explorer sidebar showing one real file, revealed and focused, with
    /// a temp `cwd` the caller must clean up. Returns the app plus the temp
    /// dir so the test can remove it.
    fn app_with_focused_explorer(tag: &str) -> (TuiShellApp, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_757_{tag}_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("zqxw757.txt");
        std::fs::write(&marker, "marker").unwrap();

        let mut app = TuiShellApp::new(None);
        app.engine.cwd = dir.clone();
        app.engine.explorer_reveal_path(&marker);
        app.engine
            .app_shell
            .show_panel(&quadraui::WidgetId::new(PANEL_EXPLORER));
        app.engine.session.explorer_visible = true;
        app.engine.explorer_has_focus = true;
        app.sidebar.has_focus = true;
        (app, dir)
    }

    /// #757: Ctrl-L while the activity bar holds the keyboard must **not**
    /// activate the selected item.
    ///
    /// `render::activity_bar_key_action` states the toolbar's key table once
    /// and, following GTK, suppresses activate/focus-out under Ctrl. TUI's
    /// own copy matched a bare `KeyCode::Char('l')` with no modifier guard,
    /// so Ctrl-L in the toolbar switched panels on TUI and did nothing on
    /// GTK — one table, two behaviours.
    ///
    /// Asserts on the painted sidebar (does the Settings panel's `SETTINGS`
    /// header reach the screen?), never on `activity_bar_focused`, and pairs
    /// the negative with a positive: a bare `l` immediately afterwards
    /// *must* activate, so a fixture that simply cannot activate could not
    /// pass this test by accident.
    ///
    /// **Verified RED against unfixed `develop`:** the pre-#757 activity-bar
    /// tier reaches `activity_bar_activate()` for Ctrl-L, so `SETTINGS`
    /// paints after the first keypress and the first assertion fires.
    #[test]
    fn activity_bar_ctrl_l_does_not_activate_via_shell_app() {
        use crate::core::engine::sidebar::TOOLBAR_IDX_SETTINGS;

        let mut app = TuiShellApp::new(None);
        app.engine.activity_bar_focus_in_at(TOOLBAR_IDX_SETTINGS);
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);

        let before = driver.screen();
        assert!(
            !before.contains("SETTINGS"),
            "precondition: the Settings panel must not already be open; \
             screen:\n{before}"
        );

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('l'),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                ..quadraui::Modifiers::default()
            },
            repeat: false,
        });

        let after_ctrl = driver.screen();
        assert!(
            !after_ctrl.contains("SETTINGS"),
            "Ctrl-L in the activity bar must not activate the selected item \
             (`render::activity_bar_key_action` guards Activate on !ctrl); \
             screen:\n{after_ctrl}"
        );

        // ...but a bare `l` must, so this fixture demonstrably *can* activate.
        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('l'),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });

        let after_plain = driver.screen();
        assert!(
            after_plain.contains("SETTINGS"),
            "a bare `l` on the Settings slot must still activate it; \
             screen:\n{after_plain}"
        );
    }

    /// #757: Backspace must reach the explorer's inline edit.
    ///
    /// The explorer arm's key table was one of seven hand-rolled
    /// `KeyCode` → engine-key-name copies in the sidebar tier. Its copy
    /// listed Esc/Enter/arrows/Home/End/PageUp/PageDown and *not*
    /// Backspace or Delete, so those two arrived at
    /// `dispatch_explorer_key` as the empty string and were dropped —
    /// even though `dispatch_explorer_edit_key` handles both, and GTK
    /// (which routes through the single `map_gtk_key_name` table) had
    /// them. Renaming a file from the TUI explorer could not delete a
    /// character.
    ///
    /// #757 replaced that copy with the module's existing
    /// `tui_key_to_engine_name`, which is where the two names come from.
    ///
    /// **Verified RED against unfixed `develop`:** reinstating the old
    /// explorer-local table leaves the painted edit text at
    /// `ZQXWEDIT757` after the Backspace, failing the final assertion.
    #[test]
    fn explorer_inline_edit_backspace_reaches_the_engine_via_shell_app() {
        let (app, dir) = app_with_focused_explorer("edit_backspace");
        app.engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWEDIT757".to_string(),
            "ZQXWEDIT757".len(),
            None,
            None,
        );
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);

        let before = driver.screen();
        assert!(
            before.contains("ZQXWEDIT757"),
            "precondition: the explorer's inline edit text must paint before \
             the keypress; screen:\n{before}"
        );

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Named(quadraui::NamedKey::Backspace),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });

        let after = driver.screen();
        assert!(
            !after.contains("ZQXWEDIT757"),
            "Backspace must reach the explorer's inline edit and delete the \
             last character; screen:\n{after}"
        );
        assert!(
            after.contains("ZQXWEDIT75"),
            "Backspace must delete exactly one character; screen:\n{after}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #757 (divergence 3): a plugin panel's own focus must outrank a
    /// stale `explorer_has_focus` left set alongside it — the TUI half of
    /// `focused_plugin_panel_outranks_a_stale_explorer_flag_on_gtk`
    /// (`src/gtk/testing.rs`).
    ///
    /// Reached via the *real* keyboard-activation path
    /// (`Engine::activity_bar_activate`'s ext-panel branch), not by
    /// hand-set fields: `activity_bar_focus_in_at` parks the toolbar
    /// cursor on the only registered plugin panel, then a bare `l`
    /// activates it exactly as a user pressing Enter/`l` on the activity
    /// bar would. That branch sets `ext_panel_active` +
    /// `ext_panel_has_focus` without clearing the explorer's own (stale,
    /// from before the activity bar was ever touched) `explorer_has_focus`
    /// — see `render::route_focus_key`'s doc comment, divergence 3.
    ///
    /// **Not a RED test against unfixed TUI.** The old TUI ladder already
    /// checked the plugin panel before the explorer (its own removed doc
    /// comment: "… → ext panel → extensions → settings → … → explorer
    /// (unguarded fallback)"), so this fixture was already green pre-#757
    /// on this backend — divergence 3 was GTK-only (GTK checked the
    /// explorer first). Kept here anyway so both backends carry coverage
    /// for the *converged*, single-source-of-truth resolver, per
    /// CLAUDE.md's multi-backend testing rule.
    #[test]
    fn focused_plugin_panel_outranks_a_stale_explorer_flag_via_shell_app() {
        use crate::core::engine::sidebar::TOOLBAR_IDX_EXT_BASE;

        let (mut app, dir) = app_with_focused_explorer("ext_panel_stale");
        app.engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWEXT757".to_string(),
            "ZQXWEXT757".len(),
            None,
            None,
        );
        app.engine.ext_panels.clear();
        app.engine.ext_panels.insert(
            "git-insights".to_string(),
            ext_panel_reg("git-insights", "Git Insights"),
        );
        // Park the activity bar's keyboard cursor on the (only) plugin panel.
        app.engine.activity_bar_focus_in_at(TOOLBAR_IDX_EXT_BASE);

        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);

        let before = driver.screen();
        assert!(
            before.contains("ZQXWEXT757"),
            "precondition: the explorer's inline edit must paint before \
             the activity bar is activated; screen:\n{before}"
        );

        // Activate the plugin panel — the real `Engine::activity_bar_activate`
        // ext-panel branch, which leaves `explorer_has_focus` stale-true.
        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Char('l'),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });
        let mid = driver.screen();
        assert!(
            !mid.contains("ZQXWEXT757"),
            "precondition: the plugin panel must now own the sidebar body \
             (`render::sidebar_owner` prefers `ext_panel_active`); \
             screen:\n{mid}"
        );

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Named(quadraui::NamedKey::Backspace),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });

        // Reveal: click the explorer's own activity-bar icon (row 1) to
        // switch the visible panel back — the same production
        // `AppShellEvent::PanelChanged` path a real click takes, which
        // clears `ext_panel_active`/`ext_panel_has_focus` and refocuses
        // the explorer.
        driver.click(1.0, 1.0);

        let after = driver.screen();
        assert!(
            after.contains("ZQXWEXT757"),
            "a focused plugin panel must outrank a stale explorer focus \
             flag — Backspace must not have reached the explorer's inline \
             edit; screen:\n{after}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #757 (divergence 4): a real focus flag must outrank the merely
    /// *visible* explorer panel — the mirror-image bug of
    /// `visible_settings_panel_outranks_a_stale_explorer_flag_on_gtk`
    /// (`src/gtk/testing.rs`), this time on the backend that had it
    /// backwards: old TUI matched every panel exclusively by
    /// `active_panel_is` (the visible panel), never by the engine's own
    /// `*_has_focus` flags.
    ///
    /// `render::route_focus_key`'s settings arm matches on
    /// `engine.settings_has_focus || engine.active_panel_is(PANEL_SETTINGS)`
    /// — the union old TUI never took, since it kept only the second half.
    ///
    /// The explorer is left as the default *visible* panel (never
    /// switched away from) while `settings_has_focus` is set directly —
    /// the state a plugin or future codepath that sets the flag without
    /// also calling `app_shell.show_panel` would produce.
    /// `app_with_focused_explorer` also sets `explorer_has_focus`, which
    /// is harmless here: the resolver never consults that flag as a match
    /// arm, only as the unguarded fallback nothing else claims.
    ///
    /// **Verified RED against unfixed `develop`:** the old TUI ladder
    /// matched Settings only via `active_panel_is(PANEL_SETTINGS)`, which
    /// is false here (the visible panel is still Explorer), so its
    /// settings arm never matched; every other arm's `active_panel_is`
    /// check is equally false, so the key fell through to the unguarded
    /// explorer fallback and Backspace deleted a character from the
    /// painted inline edit.
    #[test]
    fn focused_settings_panel_outranks_the_default_visible_explorer_via_shell_app() {
        let (mut app, dir) = app_with_focused_explorer("settings_flag_vs_visible_explorer");
        app.engine.explorer_tree.borrow_mut().start_editing(
            vec![0u16],
            "ZQXWFLAG757".to_string(),
            "ZQXWFLAG757".len(),
            None,
            None,
        );
        // A real, current focus flag — but the visible panel (app_shell's
        // untouched default) is still the explorer.
        app.engine.settings_has_focus = true;

        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 80, 24);

        let before = driver.screen();
        assert!(
            before.contains("ZQXWFLAG757"),
            "precondition: the explorer's inline edit must paint (it is \
             still the visible panel); screen:\n{before}"
        );

        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key: quadraui::Key::Named(quadraui::NamedKey::Backspace),
            modifiers: quadraui::Modifiers::default(),
            repeat: false,
        });

        let after = driver.screen();
        assert!(
            after.contains("ZQXWFLAG757"),
            "a real settings_has_focus must outrank the merely-visible \
             explorer panel — Backspace must not reach the explorer's \
             inline edit; screen:\n{after}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── #759 / #734 slice 4: the shared Alt-modifier / VSCode-mode rung ──
    //
    // The GTK halves of these two live in `src/gtk/testing.rs`
    // (`alt_right_widens_the_painted_sidebar_on_gtk`,
    // `alt_z_toggles_word_wrap_only_in_vscode_mode_on_gtk`) and assert the
    // same observables from the other backend. The spelling-identity tier
    // (both backends' key names into one `route_alt_key` call) is
    // `render::alt_key_router_tests`.

    /// Press `key` with Alt (and optionally Shift) held.
    fn alt_press<A: quadraui::AppLogic>(
        driver: &mut quadraui::tui::testing::TuiDriver<A>,
        key: quadraui::Key,
        shift: bool,
    ) {
        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key,
            modifiers: quadraui::Modifiers {
                alt: true,
                shift,
                ..Default::default()
            },
            repeat: false,
        });
    }

    /// #759: Alt+Right must widen the sidebar the frame actually paints.
    ///
    /// Asserted through the editor text's painted column, not through
    /// `TuiShellApp::sidebar_width` (`CLAUDE.md` rule 1): the field was
    /// already being mutated before this change, and a test reading it would
    /// pass just as happily if the width never reached the `AppShell` that
    /// carves `main_content_bounds`.
    ///
    /// **Verified RED against unfixed `develop`:** deleting the
    /// `KeyCode::Right` arm from the old Alt block (equivalently, returning
    /// `AltKeyOutcome::Fallthrough` instead of `ResizeSidebar(1)`) leaves the
    /// marker in the same column and the `+ 1` assertion fires.
    #[test]
    fn alt_right_widens_the_painted_sidebar_via_shell_app() {
        let mut app = app_with_sidebar_open();
        app.engine.buffer_mut().insert(0, "ZQXW759W");
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 120, 24);

        let before = driver
            .find_bounds("ZQXW759W")
            .expect("the editor marker must paint before the resize");
        alt_press(
            &mut driver,
            quadraui::Key::Named(quadraui::NamedKey::Right),
            false,
        );
        let after = driver
            .find_bounds("ZQXW759W")
            .expect("the editor marker must still paint after the resize");

        assert_eq!(
            after.x,
            before.x + 1.0,
            "Alt+Right must widen the painted sidebar by one column, pushing \
             the editor one column right; screen:\n{}",
            driver.screen()
        );

        alt_press(
            &mut driver,
            quadraui::Key::Named(quadraui::NamedKey::Left),
            false,
        );
        let back = driver
            .find_bounds("ZQXW759W")
            .expect("the editor marker must still paint after Alt+Left");
        assert_eq!(
            back.x,
            before.x,
            "Alt+Left must narrow it straight back; screen:\n{}",
            driver.screen()
        );
    }

    /// #759: Alt+Z is a VS Code editor command (toggle word wrap) in VSCode
    /// mode and a pass-through in Vim mode — the vscode-mode divergence this
    /// slice converges, now stated once in `render::route_alt_key`.
    ///
    /// Asserts on the painted command line, which is where `engine.message`
    /// renders on both backends — the one observable the GTK harness can also
    /// reach for editor-level state (it records no painted text for editor
    /// glyphs), so the two backends' tests assert the same string.
    ///
    /// **Verified RED against unfixed `develop`:** dropping `Alt_z` from the
    /// resolver's VSCode arm (equivalently, the `KeyCode::Char('z')` arm of
    /// the old TUI block) leaves the command line blank and the first
    /// assertion fires.
    #[test]
    fn alt_z_toggles_word_wrap_only_in_vscode_mode_via_shell_app() {
        for (vscode, expect_message) in [(true, true), (false, false)] {
            let mut app = TuiShellApp::new(None);
            app.engine.settings.wrap = false;
            app.engine.settings.editor_mode = if vscode {
                crate::core::settings::EditorMode::Vscode
            } else {
                crate::core::settings::EditorMode::Vim
            };
            app.engine.mode = if vscode {
                crate::core::Mode::Insert
            } else {
                crate::core::Mode::Normal
            };
            let mut driver = driver_with_shell(app, TuiShellApp::shell_config(vscode), 100, 24);

            alt_press(&mut driver, quadraui::Key::Char('z'), false);

            let screen = driver.screen();
            assert_eq!(
                screen.contains("Word wrap on"),
                expect_message,
                "Alt+Z must toggle word wrap in VSCode mode and do nothing in \
                 Vim mode (vscode = {vscode}); screen:\n{screen}"
            );
        }
    }

    // ── #762 / #734 slice 7: the closing rungs, black-box ───────────────
    //
    // The behavioural half of the cross-backend parity assertion. Each test
    // below has a mirror in `src/gtk/testing.rs` that drives the *same* chord
    // against the *same* engine state and asserts the *same* rendered string,
    // so "both backends resolve this key + state to the same route" is
    // checked end to end rather than only at the resolver
    // (`render::slice7_router_tests` is that spelling-identity tier).

    /// Press `key` with the given modifiers, exactly as `TuiShellApp::handle`
    /// receives it from the live runner.
    fn press_with<A: quadraui::AppLogic>(
        driver: &mut quadraui::tui::testing::TuiDriver<A>,
        key: quadraui::Key,
        modifiers: quadraui::Modifiers,
    ) {
        driver.dispatch(quadraui::UiEvent::KeyPressed {
            key,
            modifiers,
            repeat: false,
        });
    }

    /// #762: Shift+F5 is `stop`, not a shifted spelling of F5's `continue`.
    ///
    /// Asserts on the painted command line (where `engine.message` renders on
    /// both backends) rather than on any DAP field, per `CLAUDE.md` rule 1.
    /// The GTK mirror is
    /// `gtk::testing::slice7_debug_fkey_tests::shift_f5_stops_instead_of_continuing_on_gtk`,
    /// which asserts the identical string — that pair *is* the parity check.
    ///
    /// **Verified RED by reordering/removing the rung:** make
    /// `render::route_debug_fkey` return `DebugFKey::EngineKey("F5")` for the
    /// shifted case (i.e. drop the `shift` branch, which is exactly what GTK
    /// did on unfixed `develop`) and the command line reads
    /// "DAP: starting Debug debug session…" instead, firing both assertions.
    #[test]
    fn shift_f5_stops_the_debug_session_via_shell_app() {
        // `new_for_test`, not `new`: the production constructor reads the
        // developer's real `~/.config/vimcode/{settings,session}.json` and
        // reopens that workspace session's files/splits, either of which can
        // paint over the command line this test reads. See
        // `TuiShellApp::new_for_test`'s doc comment.
        let app = TuiShellApp::new_for_test();
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 100, 24);

        press_with(
            &mut driver,
            quadraui::Key::Named(quadraui::NamedKey::F(5)),
            quadraui::Modifiers {
                shift: true,
                ..Default::default()
            },
        );

        let screen = driver.screen();
        assert!(
            screen.contains("DAP: session stopped"),
            "Shift+F5 must run `stop`; screen:\n{screen}"
        );
        assert!(
            !screen.contains("starting"),
            "Shift+F5 must NOT run `continue`; screen:\n{screen}"
        );
    }

    /// #762: the debugger F-keys are *global* — they must reach the debugger
    /// even while a sidebar panel holds the keyboard.
    ///
    /// This is the half TUI was missing: its debug F-key tier sat *below* the
    /// focus owners, so with the search panel focused the chord went to that
    /// panel's own key table and the debugger never saw it. GTK already
    /// resolved these above the panels; now both backends do, which is
    /// exactly the ladder-order parity this slice's acceptance asks for.
    ///
    /// **Verified RED by reordering one rung on one backend:** move the
    /// `render::route_debug_fkey` match in `handle_key_pressed` back below
    /// the `focus_route` dispatch (its pre-#762 position, and the position
    /// this test was first written against) and the focused search panel
    /// swallows the chord — the command line stays blank and the `contains`
    /// assertion fires. That is not hypothetical: this test failed exactly
    /// that way before the reorder landed.
    #[test]
    fn shift_f5_reaches_the_debugger_from_a_focused_panel_via_shell_app() {
        // `new_for_test` for the same ambient-state reason as the test above.
        let mut app = TuiShellApp::new_for_test();
        // Hidden sidebar, focused search panel: the panel still owns the
        // keyboard (`route_focus_key` keys off the focus flags, not
        // visibility) but the tree does not over-paint the message row this
        // test reads.
        app.engine.app_shell.hide_sidebar();
        app.engine.search_has_focus = true;
        app.sidebar.has_focus = true;
        assert_eq!(
            render::route_focus_key(&app.engine, app.sidebar.has_focus),
            render::FocusKeyRoute::Search,
            "precondition: the search panel must own the keyboard"
        );
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 100, 24);

        press_with(
            &mut driver,
            quadraui::Key::Named(quadraui::NamedKey::F(5)),
            quadraui::Modifiers {
                shift: true,
                ..Default::default()
            },
        );

        let screen = driver.screen();
        assert!(
            screen.contains("DAP: session stopped"),
            "Shift+F5 must reach the debugger past a focused sidebar panel; \
             screen:\n{screen}"
        );
    }

    /// #762: Ctrl+L is consumed as "repaint", never handed to the editor.
    ///
    /// Asserted through the painted editor text: an insert-mode `Ctrl+L` that
    /// reached the engine would type or move; a consumed one leaves the
    /// buffer byte-identical. (GTK gains the rung in this slice too, but its
    /// spelling of the chord is inert in `Engine::handle_key`, so only TUI
    /// has an observable to assert on — see the note in `gtk/testing.rs`.)
    ///
    /// **Verified RED by removing the rung:** make
    /// `render::is_force_redraw_key` return `false` and the buffer picks up
    /// the fall-through, changing the painted marker.
    ///
    /// Two ambient hazards this test learned the hard way (it passed on a
    /// developer box and failed on CI — #762 CI fix 1), both already
    /// documented on their owners and both fatal *specifically* because this
    /// test measures painted editor **geometry**:
    ///
    /// 1. `TuiShellApp::new` boots from the developer's real
    ///    `~/.config/vimcode/{settings,session}.json`, so the sidebar is
    ///    visible on a machine that has ever opened the explorer and hidden
    ///    on a fresh checkout or CI runner — a whole `SIDEBAR_WIDTH` of
    ///    column shift. `new_for_test` pins it to in-memory defaults.
    /// 2. Frame 1 is painted with quadraui's generic 20-column
    ///    `default_sidebar_width`, which the end-of-dispatch
    ///    `set_sidebar_width` sync in `handle()` corrects on the *first event
    ///    of any kind* — so a column measured off frame 1 is stale from
    ///    frame 2 onwards, and the Ctrl+L press below would be blamed for the
    ///    settle. The `Escape` settles it first (same fix as
    ///    `tui_editor_text_drag_paints_a_selection_through_the_shared_drag_router`);
    ///    the `i` after it puts the buffer back in Insert mode, which is what
    ///    makes a fall-through Ctrl+L observable as an edit.
    #[test]
    fn ctrl_l_is_consumed_and_never_edits_the_buffer_via_shell_app() {
        let mut app = TuiShellApp::new_for_test();
        app.engine.buffer_mut().insert(0, "ZQXW762CTRLL");
        let mut driver = driver_with_shell(app, TuiShellApp::shell_config(false), 100, 24);

        // Settle the sidebar width, then enter Insert mode — in that order,
        // so nothing but the Ctrl+L under test can move the marker.
        driver.press_named(quadraui::NamedKey::Escape);
        press_with(
            &mut driver,
            quadraui::Key::Char('i'),
            quadraui::Modifiers::default(),
        );
        assert!(
            driver.screen().contains("INSERT"),
            "setup sanity: the buffer must be in Insert mode, so a Ctrl+L that \
             reached the engine would type; screen:\n{}",
            driver.screen()
        );

        let before = driver
            .find_bounds("ZQXW762CTRLL")
            .expect("the editor marker must paint before Ctrl+L");
        press_with(
            &mut driver,
            quadraui::Key::Char('l'),
            quadraui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        let after = driver
            .find_bounds("ZQXW762CTRLL")
            .expect("Ctrl+L must not disturb the painted buffer");
        assert_eq!(
            (after.x, after.y),
            (before.x, before.y),
            "Ctrl+L must be consumed as a repaint request, not typed; \
             screen:\n{}",
            driver.screen()
        );
    }
}
