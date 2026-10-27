use super::*;

// ─── Menu-action `EngineAction` applier rung ────────────────────────────────
//
// `Engine::dispatch_menu_action` (`core/engine/vscode.rs`) turns a fired
// `quadraui::MenuEvent::Activated` id into an `EngineAction` the caller still
// has to apply. Most variants are pure bookkeeping the engine already
// finished (`None`/`Error`, and every menu item whose own `execute_command`
// arm does its work and returns `None` — `sidebar`'s toggle, for instance,
// happens inside the engine itself). The dozen or so that remain name an
// effect `Engine` cannot finish alone: opening a terminal needs the live
// pane's column/row count, a dialog needs to write into backend-local dialog
// state, quitting needs the backend's own shutdown sequence.
//
// [`apply_engine_action`] used to take a `host: &mut impl
// EngineActionHost` seam — a per-backend struct implementing eleven hook
// methods — for the dozen or so effects `Engine` cannot finish alone. With
// the TUI backend gone, `App` is the only implementation left, so those
// eleven hooks are inlined directly into the match arms below, reading/
// mutating `engine` and `app`'s fields exactly as `GtkEngineActionHost` (the
// old per-backend host struct) used to. `open_terminal`/`open_workspace_dialog`
// used to be `App::new_terminal_tab`/`App::open_workspace_dialog` verbatim —
// this fold does not resurrect them.

/// Apply the [`crate::core::engine::EngineAction`] produced by
/// `Engine::dispatch_menu_action` (a fired `MenuEvent::Activated`). Returns
/// `true` when the action means the caller should exit — `Quit`/`SaveQuit`;
/// `QuitWithError` never returns at all.
///
/// `OpenFile`/`OpenUrl`/`None`/`Error` need no `app` help — `Engine` already
/// did (or need do) everything for those — so they're handled inline without
/// touching `app` at all.
///
/// Most arms below read/mutate the `engine: &mut Engine` parameter directly
/// rather than going through `app.engine.borrow()/borrow_mut()`: `engine` is
/// already a live `RefMut` borrow of that same `Rc<RefCell<Engine>>` at every
/// call site, so a second, independent `app.engine.borrow_mut()` from in here
/// would double-borrow the same `RefCell` and panic at runtime.
pub(crate) fn apply_engine_action(
    action: crate::core::engine::EngineAction,
    engine: &mut Engine,
    app: &mut crate::app::App,
) -> bool {
    use crate::core::engine::{EngineAction, PendingPlatformAction};
    match action {
        EngineAction::None | EngineAction::Error => false,
        EngineAction::Quit | EngineAction::SaveQuit => {
            app.quit_and_save_session(engine);
            true
        }
        // `Quit`/`SaveQuit` above save the session; `QuitWithError` does not.
        // This asymmetry is preserved as it behaved before this consolidation.
        EngineAction::QuitWithError => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            std::process::exit(1);
        }
        EngineAction::QuitWithUnsaved => {
            if !engine.has_any_unsaved() {
                app.quit_and_save_session(engine);
            } else {
                engine.show_quit_confirm();
                app.draw_needed.set(true);
            }
            false
        }
        EngineAction::OpenFile(path) => {
            if let Err(e) = engine.open_file_with_mode(&path, crate::core::OpenMode::Permanent) {
                engine.message = e;
            }
            false
        }
        EngineAction::OpenTerminal => {
            let cols = app.terminal_panel_cols(app.painted_editor_content_width());
            let rows = engine.session.terminal_panel_rows;
            engine.terminal_new_tab(cols, rows);
            app.draw_needed.set(true);
            false
        }
        EngineAction::ToggleTerminalMaximize => {
            let ctx = crate::core::engine::UiEventContext {
                terminal_cols: app.terminal_panel_cols(app.painted_editor_content_width()),
                terminal_max_rows: app.terminal_maximize_target_rows(engine),
            };
            engine.handle_ui_event(
                crate::core::engine::UiEvent::Accelerator(
                    crate::core::engine::AcceleratorId::new("terminal.toggle_maximize"),
                    quadraui::Modifiers::default(),
                ),
                ctx,
            );
            app.draw_needed.set(true);
            false
        }
        EngineAction::RunInTerminal(cmd) => {
            let cols = app.terminal_panel_cols(app.painted_editor_content_width());
            let rows = engine.session.terminal_panel_rows;
            engine.terminal_run_command(&cmd, cols, rows);
            app.draw_needed.set(true);
            false
        }
        EngineAction::OpenFolderDialog => {
            app.open_folder_dialog_native();
            false
        }
        // Inlines the `refresh_file_tree` / `refresh_explorer` /
        // `reveal_path_in_explorer` chain the deleted `App::open_workspace_dialog`
        // called — `queue_explorer_draw` (that chain's last step) is a
        // documented no-op under the `ShellApp` runner, so dropping it
        // changes nothing.
        EngineAction::OpenWorkspaceDialog => {
            engine.explorer_rebuild_rows();
            if let Some(path) = engine.file_path().cloned() {
                engine.explorer_reveal_path(&path);
            }
            app.draw_needed.set(true);
            false
        }
        // Touches no engine state, so this calls `App::save_workspace_as_dialog`
        // straight through rather than inlining it.
        EngineAction::SaveWorkspaceAsDialog => {
            app.save_workspace_as_dialog();
            false
        }
        EngineAction::OpenRecentDialog => {
            if engine.session.recent_workspaces.is_empty() {
                engine.message = "No recent workspaces".to_string();
            } else {
                engine.open_picker(crate::core::engine::PickerSource::RecentWorkspaces);
            }
            app.draw_needed.set(true);
            false
        }
        // A redraw trigger only under the `ShellApp` runner (see
        // `App::sync_sidebar_from_engine`'s own doc comment, the method this
        // arm used to call).
        EngineAction::ToggleSidebar => {
            app.draw_needed.set(true);
            false
        }
        EngineAction::OpenUrl(url) => {
            // Queue rather than shell out here — `apply_engine_action`
            // has no `backend` handle of its own. `App::tick_dispatch` (GTK) /
            // the pre-#1434 TUI shell's `tick` (TUI) drain `pending_platform_actions`
            // through `PlatformServices` (`is_safe_url` was already applied
            // by whichever engine path produced this `EngineAction`).
            engine
                .pending_platform_actions
                .push(PendingPlatformAction::OpenUrl(url));
            false
        }
    }
}

// ─── Explorer context-menu applier ─────────────────────────────────────────
//
// The explorer context menu's decision — which item is highlighted, which
// key/click confirms it — was already shared (`Engine::handle_context_menu_key`
// / `Engine::context_menu_target_path`). This function is the one shared body
// both backends call for the confirmed action. It uses `Engine::confirm_delete_file`
// and `Engine::start_move_file_dialog` (explicit-target-based), and
// `Engine::open_grep_picker_scoped` for `"find_in_folder"` (live-grep search
// restricted to the clicked folder).
//
// `new_file`/`new_folder`/`rename` keep routing through
// `dispatch_explorer_crud` (selected-row-based) — both backends already
// agreed on that subset, so it is not a divergence this function needs to
// resolve, only preserve.
//
// `open_terminal_at` covers the one action that genuinely needs
// backend-specific plumbing: `"open_terminal"` needs the live terminal
// pane's column count. Every other action is a plain `Engine` call with no
// backend seam.

/// Resolve the "open_terminal"/"find_in_folder" target directory: `target`
/// itself when it is already a directory, its parent otherwise (falling
/// back to `engine.cwd` for a target with no parent).
fn explorer_ctx_action_dir(
    engine: &Engine,
    target: &std::path::Path,
    is_dir: bool,
) -> std::path::PathBuf {
    if is_dir {
        target.to_path_buf()
    } else {
        target.parent().unwrap_or(&engine.cwd).to_path_buf()
    }
}

/// Apply the action string [`Engine::context_menu_confirm`] returned for an
/// explorer context menu (`target`/`is_dir` are that same confirm's
/// `Engine::context_menu_target_path`, captured by the caller *before*
/// confirming — see e.g. `crate::app::App::dispatch_context_menu_key`).
///
/// `copy_path`/`copy_relative_path`/`reveal`/`open_side`/`open_side_vsplit`/
/// `select_for_diff`/`diff_with_selected` are engine-owned — already fully
/// handled inside `context_menu_confirm` itself — so they (and any other
/// unrecognised action) are a deliberate no-op here.
///
/// `open_terminal_at` opens a new terminal tab rooted at the given directory
/// — needs the live pane's column count, which only the caller's own runner
/// has in scope, so it's the caller's job, not this function's.
pub fn apply_explorer_context_action(
    engine: &mut Engine,
    action: &str,
    target: &std::path::Path,
    is_dir: bool,
    open_terminal_at: &mut dyn FnMut(&mut Engine, std::path::PathBuf),
) {
    match action {
        "new_file" | "new_folder" | "rename" => {
            if let Some(crud_action) =
                crate::core::settings::ExplorerAction::from_action_str(action)
            {
                engine.dispatch_explorer_crud(crud_action);
            }
        }
        "delete" => engine.confirm_delete_file(target),
        "move_file" => {
            let root = engine.cwd.clone();
            engine.start_move_file_dialog(target, &root);
        }
        "open_terminal" => {
            let dir = explorer_ctx_action_dir(engine, target, is_dir);
            open_terminal_at(engine, dir);
        }
        "find_in_folder" => {
            let dir = explorer_ctx_action_dir(engine, target, is_dir);
            engine.open_grep_picker_scoped(&dir);
        }
        _ => {}
    }
}

// ─── Native file-dialog rung ────────────────────────────────────────────────
//
// [`run_open_file_dialog`] and [`run_save_workspace_as_dialog`] are the one
// shared body both backends call — the point of this rung is one
// implementation, two thin call sites.

/// Show a native "Open File" dialog via `backend`'s `PlatformServices` and
/// open the chosen file in `engine`. Returns the opened path (`None` if the
/// user cancelled) so a caller can refresh backend-local UI — e.g. GTK's
/// file-tree selection — only when something was actually opened.
pub fn run_open_file_dialog(
    engine: &mut Engine,
    backend: &mut dyn quadraui::Backend,
) -> Option<std::path::PathBuf> {
    let path = backend
        .services()
        .show_file_open_dialog(quadraui::FileDialogOptions {
            title: Some("Open File".to_string()),
            // Browse from the current workspace root, not wherever the OS
            // process happened to start (`FileDialogOptions::initial_dir`
            // defaults to `std::env::current_dir()` when `None` — see
            // `TuiPlatformServices::show_file_open_dialog`'s doc — which can
            // diverge from `engine.cwd` after an in-app `Open Folder`/`:cd`
            // that never actually `chdir`s the process).
            initial_dir: Some(engine.cwd.clone()),
            ..Default::default()
        })?;
    let _ = engine.open_file_with_mode(&path, crate::core::engine::OpenMode::Permanent);
    Some(path)
}

/// Show a native "Save Workspace As" dialog via `backend`'s
/// `PlatformServices` and save the workspace to the chosen path. Does
/// nothing when the user cancels.
pub fn run_save_workspace_as_dialog(engine: &mut Engine, backend: &mut dyn quadraui::Backend) {
    if let Some(path) = backend
        .services()
        .show_file_save_dialog(quadraui::FileDialogOptions {
            title: Some("Save Workspace As".to_string()),
            initial_filename: Some(".vimcode-workspace".to_string()),
            // See `run_open_file_dialog`'s identical `initial_dir` comment.
            initial_dir: Some(engine.cwd.clone()),
            ..Default::default()
        })
    {
        engine.save_workspace_as(&path);
    }
}

// ─── Shell-event shadow-sync rung ────────────────────────────────────────────
//
// `AppShellEvent::PanelChanged`/`SidebarHidden`/`SidebarResized` all report a
// decision the *runner's* own `AppShell` already made. Both backends mirror
// that decision into `engine.app_shell`, the "shadow" copy every engine-side
// consumer reads.
//
// [`sync_shell_event_shadow`] mirrors a runner-decided event onto the shadow
// `engine.app_shell`. `App::on_shell_event` calls it unconditionally, as the
// first thing it does, before any of its own id-specific branching — so the
// shadow mutation itself can never be skipped by an early `return`.

/// Mirror a runner-decided [`quadraui::AppShellEvent`] onto the shadow
/// `engine.app_shell`. Called by `App::on_shell_event` first,
/// unconditionally, before any of the event's other id-specific handling —
/// see the rung's header comment above for why the call must come first.
pub fn sync_shell_event_shadow(event: &quadraui::AppShellEvent, engine: &mut Engine) {
    match event {
        quadraui::AppShellEvent::PanelChanged { panel_id } => {
            if crate::app_support::is_ext_panel_id(panel_id.as_str())
                || panel_id.as_str() == crate::core::engine::sidebar::HAMBURGER_PANEL_ID
            {
                return;
            }
            engine.app_shell.show_panel(panel_id);
            engine.ext_panel_active = None;
            engine.ext_panel_has_focus = false;
        }
        quadraui::AppShellEvent::SidebarHidden => {
            engine.app_shell.hide_sidebar();
            engine.ext_panel_active = None;
            engine.ext_panel_has_focus = false;
        }
        quadraui::AppShellEvent::SidebarResized { new_width } => {
            engine.app_shell.set_sidebar_width(*new_width);
        }
        _ => {}
    }
}

// ─── Shared menu-bar reveal/hide routing ────────────────────────────────────
//
// A backend whose menu bar can be fully hidden needs three pieces of
// behaviour a permanently-visible menu bar never does: an Alt+<letter> shim
// so a hidden bar's accelerators still work, a one-shot guard that hides the
// bar again on the stale click a hamburger reveal leaves behind, and a
// hamburger `PanelDefinition` click that reveals the bar instead of switching
// a sidebar panel. The functions below are pure `&mut Engine` (plus, where a
// hit-test needs one, `&dyn Backend` / `&ShellContext`) so callers wire them
// in unconditionally. Every one is a documented no-op on a backend that never
// sets `menu_bar_toggleable` (GTK/macOS/Win today).

/// One-shot spend of [`Engine::hamburger_stale_click_guard`] for
/// `event`, on the *direct-dispatch* path (`ShellApp::handle`/
/// `handle_dispatch`) — call at the very top, before any early exit, so the
/// guard is spent for exactly one event no matter which arm of the caller's
/// own dispatch that event ends up in. Returns `true` only for the one
/// `MouseDown` this guard exists to catch (the caller's own corner-position
/// check, [`route_menu_bar_reveal`], decides whether it actually landed on
/// the stale corner). See [`Engine::hamburger_stale_click_guard`]'s own doc
/// for the full one-shot lifecycle and why pointer/window plumbing
/// (`MouseMoved`/`MouseUp`/focus/resize/DPI) leaves the guard armed instead
/// of spending it.
pub fn consume_hamburger_stale_click_guard(engine: &mut Engine, event: &quadraui::UiEvent) -> bool {
    if !engine.hamburger_stale_click_guard {
        return false;
    }
    match event {
        quadraui::UiEvent::MouseDown { .. } => {
            engine.hamburger_stale_click_guard = false;
            true
        }
        quadraui::UiEvent::MouseUp { .. }
        | quadraui::UiEvent::MouseMoved { .. }
        | quadraui::UiEvent::MouseEntered { .. }
        | quadraui::UiEvent::MouseLeft { .. }
        | quadraui::UiEvent::WindowResized { .. }
        | quadraui::UiEvent::WindowFocused(_)
        | quadraui::UiEvent::DpiChanged(_)
        | quadraui::UiEvent::WindowStateChanged { .. } => false,
        _ => {
            engine.hamburger_stale_click_guard = false;
            false
        }
    }
}

/// Spend [`Engine::hamburger_stale_click_guard`] on the
/// *shell-consumed* path — the half [`consume_hamburger_stale_click_guard`]
/// can never see, because `ShellAdapter` hit-tests the activity bar itself
/// and reports a real panel click as a semantic `AppShellEvent` without ever
/// falling through to `ShellApp::handle`. Call from every `on_shell_event`/
/// `on_shell_event_ctx` arm that represents genuine user input (a real panel
/// icon, a divider drag, a bottom item) — every one of those is exactly as
/// much "the user moved on" as a keystroke is, so the guard has to die there
/// too, or a `File` click arriving afterwards is misread as the stale corner.
/// Not called for the hamburger's own reveal (that arm *arms* the guard) nor
/// for a suppressed `take_requested_panel` echo (the app reconciling itself,
/// not user input).
pub fn disarm_hamburger_stale_click_guard(engine: &mut Engine) {
    engine.hamburger_stale_click_guard = false;
}

/// The Alt+<letter> reveal shim plus the stale-hamburger-corner hide.
/// Takes `stale_hamburger_corner_click` — the result of
/// [`consume_hamburger_stale_click_guard`] — as a parameter rather than
/// calling it internally, because that consume must happen at the very
/// top of `handle`/`handle_dispatch`, before any early exit.
///
/// 1. **Alt+<letter> shim** — when the bar is hidden, Alt+<letter> must
///    still reveal *and* activate the matching menu. Sets
///    `engine.menu_bar_visible = true` and lets the event keep flowing: the
///    caller's own `MenuSystem` intercept is what actually opens the menu.
/// 2. **Stale hamburger-corner click** — revealing the bar shifts the whole
///    activity bar down one row; a second click at the *exact screen position*
///    that revealed it lands one row too high, on the title-bar band. Recognised
///    structurally and bounded by `stale_hamburger_corner_click`'s one-shot
///    source, so a later, deliberate click on a menu item falls through to the
///    caller's `MenuSystem` intercept like any other menu click.
///
/// Returns `Some(Reaction::Redraw)` only for case 2 — the caller must return
/// immediately. `None` means "not consumed" (including case 1's reveal).
///
/// A no-op on a backend that never sets `menu_bar_toggleable`: the shim's
/// `!menu_bar_visible` gate never opens.
pub fn route_menu_bar_reveal(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    stale_hamburger_corner_click: bool,
    backend: &mut dyn quadraui::Backend,
    ctx: &quadraui::ShellContext<'_>,
) -> Option<quadraui::Reaction> {
    if !engine.menu_bar_visible {
        if let quadraui::UiEvent::KeyPressed { key, modifiers, .. } = event {
            // This shim and the caller's
            // own `MenuSystem` intercept are two halves of one action (see
            // this function's own doc, point 1) — gating only the intercept
            // and not this reveal left the bar *half*-opened: a fused pty
            // chord flipped `menu_bar_visible` true here, the caller then
            // refused to open the dropdown, and the bar row stayed revealed
            // with nothing in it, consuming a terminal row and shifting the
            // whole layout for the rest of the session. Same predicate as
            // the caller's gate, so the two halves agree in every mode.
            if modifiers.alt && alt_mnemonic_open_allowed(engine.mode, engine.menu_bar_toggleable) {
                if let quadraui::Key::Char(c) = key {
                    let bar = engine.menu_system.borrow().menu_bar();
                    if bar.find_alt_target(*c).is_some() {
                        engine.menu_bar_visible = true;
                    }
                }
            }
        }
    }

    if stale_hamburger_corner_click && engine.menu_bar_visible {
        if let quadraui::UiEvent::MouseDown { position, .. } = event {
            let viewport = backend.viewport();
            let area = quadraui::Rect::new(0.0, 0.0, viewport.width, viewport.height);
            let layout = ctx.shell().layout(area, backend.line_height());
            let ab = layout.activity_bar_bounds;
            let in_hamburger_corner = ctx.in_title_bar(position.x, position.y)
                && position.x >= ab.x
                && position.x < ab.x + ab.width;
            if in_hamburger_corner {
                engine.menu_bar_visible = false;
                ctx.shell_mut().hide_sidebar();
                ctx.shell_mut().set_title_bar_visible(false);
                return Some(quadraui::Reaction::Redraw);
            }
        }
    }

    None
}

/// Resolve the rect [`quadraui::MenuSystem::handle`]/`::render` should use
/// for a toggleable-menu-bar backend, given this frame's cached title-bar-band
/// rect. Falls back to a full-viewport-width, one-row rect when the cache
/// holds an empty rect from a just-revealed-but-not-yet-painted frame.
///
/// `toggleable` gates the whole fallback: `false` always returns `cached`
/// unchanged.
pub fn menu_bar_intercept_rect(
    toggleable: bool,
    cached: quadraui::Rect,
    viewport_width: f32,
) -> quadraui::Rect {
    if !toggleable || (cached.width >= 1.0 && cached.height >= 1.0) {
        cached
    } else {
        quadraui::Rect::new(0.0, 0.0, viewport_width, 1.0)
    }
}

/// Keep the runner's `AppShell` title-bar row
/// reservation in sync with `engine.menu_bar_visible` — that flag can flip
/// via [`route_menu_bar_reveal`] above, `render::dispatch_panel_accelerator`,
/// `:set menu`, or (for the hamburger specifically) [`route_hamburger_panel_
/// changed`]/[`route_hamburger_sidebar_hidden`] below, and none of those
/// touch the runner's own `AppShell` on their own — only
/// `AppShell::set_title_bar_visible` does. Call unconditionally on the way
/// out of every dispatch (`handle`/`handle_dispatch`'s tail,
/// `on_shell_event_ctx`'s tail) rather than only from the specific arms that
/// change the flag, per `AppShell::set_title_bar_visible`'s own doc:
/// "toggling this and calling [layout/render] next is
/// sufficient". A no-op whenever the flag hasn't changed since the last
/// call.
pub fn sync_menu_bar_title_row(ctx: &quadraui::ShellContext<'_>, engine: &Engine) {
    ctx.shell_mut()
        .set_title_bar_visible(engine.menu_bar_visible);
}

/// The hamburger's own `AppShellEvent::PanelChanged` — reveals the
/// menu bar and, on a genuine reveal (the `false -> true` transition, not a
/// `take_requested_panel` echo of an already-open bar), arms the stale-click
/// guard [`route_menu_bar_reveal`] later reads. Returns `true` when
/// `panel_id` names the hamburger, in which case the caller must skip its
/// own generic `PanelChanged` handling for this event (there is no shadow
/// `PanelDefinition` for the hamburger to switch to — see
/// [`sync_shell_event_shadow`]'s `PanelChanged` arm).
pub fn route_hamburger_panel_changed(engine: &mut Engine, panel_id: &quadraui::WidgetId) -> bool {
    if panel_id.as_str() != HAMBURGER_PANEL_ID {
        return false;
    }
    let just_revealed = !engine.menu_bar_visible;
    engine.menu_bar_visible = true;
    if just_revealed {
        engine.hamburger_stale_click_guard = true;
    }
    true
}

/// The hamburger's own second click — reported as
/// `AppShellEvent::SidebarHidden` for whichever panel the runner's `AppShell`
/// currently has active — hides the menu bar again. Only `ctx.shell()`, the
/// *runner's* own state, can tell "this `SidebarHidden` is the hamburger
/// closing" apart from a real panel's own second click (the shadow
/// `engine.app_shell` has no hamburger entry to check against), so the
/// caller must guard this call itself:
/// `ctx.shell().active_panel_id() == Some(HAMBURGER_PANEL_ID)`, checked
/// *before* delegating to its own generic `SidebarHidden` handling (none of
/// that generic handling applies here — the reveal never touched the shadow
/// sidebar in the first place).
pub fn route_hamburger_sidebar_hidden(engine: &mut Engine, ctx: &quadraui::ShellContext<'_>) {
    disarm_hamburger_stale_click_guard(engine);
    engine.menu_bar_visible = false;
    ctx.shell_mut().set_title_bar_visible(false);
}

/// Correct a frame's [`quadraui::AppShellLayout`] for the hamburger's
/// own phantom sidebar reservation. The hamburger is a top-row
/// `PanelDefinition` in the *runner's* `AppShell` (so a click on it produces
/// a real `PanelChanged`/hit-testable activity-bar icon) but has no matching
/// entry in the shadow `engine.app_shell` (see
/// [`sync_shell_event_shadow`]'s `PanelChanged` arm) — so while the
/// hamburger is the runner's active panel, `AppShellLayout` still reserves a
/// sidebar column for it even though nothing real is behind it. Left alone,
/// `render_content` would paint that reservation as an empty sidebar band
/// (or worse, stale content from whatever panel was active before).
///
/// A no-op — returns `layout.clone()` unchanged — unless all three hold:
/// `engine.menu_bar_toggleable` (only ever `true` alongside a runner that
/// can register the hamburger panel at all), `engine.menu_bar_visible`
/// (there is a reservation to reclaim only while the bar — and therefore the
/// hamburger's reveal — is showing), and the shadow's own sidebar is hidden
/// (`!engine.app_shell.sidebar_visible()` — if a *real* panel is genuinely
/// visible, e.g. the very first frame before any dispatch has run the
/// sidebar-visibility sync yet, this must not blank out its real content).
/// Reclaims the sidebar + divider width back onto `main_content_bounds`,
/// mirroring `AppShellLayout`'s own `!sidebar_visible` branch.
pub fn reclaim_hamburger_sidebar_reservation(
    engine: &Engine,
    layout: &quadraui::AppShellLayout,
) -> quadraui::AppShellLayout {
    if !engine.menu_bar_toggleable
        || !engine.menu_bar_visible
        || engine.app_shell.sidebar_visible()
        || layout.sidebar_content_bounds.is_none()
    {
        return layout.clone();
    }
    let ab = layout.activity_bar_bounds;
    let main = layout.main_content_bounds;
    let reclaimed_main = quadraui::Rect::new(
        ab.x + ab.width,
        main.y,
        (layout.window_bounds.x + layout.window_bounds.width - (ab.x + ab.width)).max(0.0),
        main.height,
    );
    quadraui::AppShellLayout {
        sidebar_header_bounds: None,
        sidebar_content_bounds: None,
        divider_bounds: None,
        main_content_bounds: reclaimed_main,
        ..layout.clone()
    }
}

/// Keep the runner `AppShell`'s sidebar visibility in sync with the shadow
/// `engine.app_shell`'s. The shadow is mutated by every keyboard path, while
/// the runner's `AppShell` owns whether `sidebar_content_bounds` exists at all
/// in the painted layout. The hamburger is special-cased out of this sync
/// entirely while it's the reveal actually in effect, since the shadow has no
/// hamburger `PanelDefinition`. Also gated on `engine.menu_bar_visible` to
/// distinguish an actual, user-driven reveal from the construction-time default.
pub fn sync_runner_sidebar_visibility(engine: &Engine, ctx: &quadraui::ShellContext<'_>) {
    let shadow_visible = engine.app_shell.sidebar_visible();
    let runner_visible = ctx.shell().sidebar_visible();
    let runner_shows_hamburger = engine.menu_bar_visible
        && ctx
            .shell()
            .active_panel_id()
            .map(quadraui::WidgetId::as_str)
            == Some(HAMBURGER_PANEL_ID);
    if runner_visible != shadow_visible && !runner_shows_hamburger {
        if shadow_visible {
            // While a plugin panel is open the shadow's active-panel
            // id still names the built-in that preceded it (extension
            // panels never touch it), so reveal *that* panel and the
            // runner's highlight jumps off the extension icon — same
            // reason `ShellApp::take_requested_panel` prefers
            // `ext_panel_active`.
            let id = engine
                .ext_panel_active
                .as_deref()
                .map(|n| quadraui::WidgetId::new(crate::core::engine::sidebar::ext_panel_id(n)))
                .or_else(|| engine.app_shell.active_panel_id().cloned());
            if let Some(id) = id {
                ctx.shell_mut().show_panel(&id);
            }
            // `AppShell::show_panel` only searches its top `panels` list —
            // the Settings cog is a bottom item, so the call above can
            // no-op. Force visibility alone in that case; the runner's
            // active-panel index (header title) is untouched.
            if !ctx.shell().sidebar_visible() {
                ctx.shell_mut().toggle_sidebar();
            }
        } else {
            ctx.shell_mut().hide_sidebar();
        }
    }
}

// ─── Shared tick-chore rung ──────────────────────────────────────────────────
//
// [`run_shared_tick_chores`] is the one function both backends' tick()
// implementations call for the shared dozen-item background chore list:
// viewport sync, tab-bar scroll offsets, yank highlight expiry, LSP/DAP/
// search idle work, source control refresh, deferred format-then-quit,
// queued terminal commands, extension panel focus, queued platform actions,
// and OS window title.

/// The OS/taskbar window title vimcode uses everywhere — `"VimCode —
/// <buffer name>"`, or the bare app name with none open. Used to be
/// formatted identically, and independently, in `App::handle_poll_tick` and
/// the pre-#1434 TUI shell's `tick`; now the one string
/// [`run_shared_tick_chores`]'s window-title chore formats.
pub fn window_title(engine: &Engine) -> String {
    engine
        .active_buffer_name()
        .map(|n| format!("VimCode \u{2014} {}", n))
        .unwrap_or_else(|| "VimCode".to_string())
}

/// Carry out a queued [`crate::core::engine::PendingPlatformAction`] (open
/// URL / reveal in file manager) via `backend`'s `PlatformServices`.
pub fn run_pending_platform_action(
    engine: &mut Engine,
    action: crate::core::engine::PendingPlatformAction,
    backend: &mut dyn quadraui::Backend,
) {
    use crate::core::engine::PendingPlatformAction;
    match action {
        PendingPlatformAction::OpenUrl(url) => {
            if let Err(e) = backend.services().open_url_result(&url) {
                engine.message = format!("Could not open URL: {e:?}");
            }
        }
        PendingPlatformAction::Reveal(path) => {
            if let Err(e) = backend.services().reveal_in_file_manager(&path) {
                engine.message = format!("Could not reveal in file manager: {e:?}");
            }
        }
        PendingPlatformAction::OpenPath(path) => {
            if let Err(e) = backend.services().open_path(&path) {
                engine.message = format!("Could not open path: {e:?}");
            }
        }
        PendingPlatformAction::Notify {
            title,
            body,
            urgent,
        } => {
            let mut notification = quadraui::Notification::new(title, body);
            notification.urgent = urgent;
            backend.services().send_notification(notification);
        }
        PendingPlatformAction::PluginDialog(req) => {
            run_pending_plugin_dialog(engine, req, backend);
        }
    }
}

/// Resolve a `vimcode.platform.show_*_dialog` request: show the matching
/// native dialog when the running backend's capability for it is present,
/// otherwise deliver `None` straight away (the TUI fallback — no backend
/// implements the in-canvas `Dialog` primitive for this yet, so plugins
/// must treat a `nil` path as "no native picker available" and fall back to
/// their own prompt).
fn run_pending_plugin_dialog(
    engine: &mut Engine,
    req: crate::core::engine::PluginDialogRequest,
    backend: &mut dyn quadraui::Backend,
) {
    use crate::core::engine::PluginDialogKind;

    let caps = engine.backend_caps;
    let has_cap = match req.kind {
        PluginDialogKind::Open | PluginDialogKind::Save => caps.file_dialogs,
        PluginDialogKind::Folder => caps.folder_dialogs,
    };

    let path = if has_cap {
        let opts = quadraui::FileDialogOptions {
            title: req.title,
            initial_dir: req.initial_dir,
            initial_filename: req.initial_filename,
            filters: req.filters,
        };
        match req.kind {
            PluginDialogKind::Open => backend.services().show_file_open_dialog(opts),
            PluginDialogKind::Save => backend.services().show_file_save_dialog(opts),
            PluginDialogKind::Folder => backend.services().show_folder_open_dialog(opts),
        }
    } else {
        None
    };

    engine.deliver_plugin_dialog_result(req.callback_id, path);
}

/// Editor mode → hardware caret shape: block for Normal/Visual, bar for
/// Insert, underline for a pending replace-char (`r`) command.
///
/// `sidebar_has_focus` is the caller's own "is a sidebar panel, not the
/// editor, holding keyboard focus" answer — pure function, deliberately
/// separate from the caller's write gate so the *decision* can be unit-tested.
pub fn caret_shape_for_mode(
    engine: &Engine,
    sidebar_has_focus: bool,
) -> quadraui::EditorCursorShape {
    if !sidebar_has_focus && engine.pending_key == Some('r') {
        quadraui::EditorCursorShape::Underline
    } else if !sidebar_has_focus && engine.mode == Mode::Insert {
        quadraui::EditorCursorShape::Bar
    } else {
        quadraui::EditorCursorShape::Block
    }
}

/// Run the shared tick-time background chore list — see the rung's header
/// comment above for the full list. Returns `true` when anything changed
/// this tick that warrants a redraw.
pub(crate) fn run_shared_tick_chores(
    engine: &mut Engine,
    app: &mut crate::app::App,
    backend: &mut dyn quadraui::Backend,
) -> bool {
    let mut needs_redraw = false;

    // Exact per-window viewport dimensions from the last paint, so
    // `ensure_cursor_visible` uses real geometry rather than a whole-screen
    // approximation that can't see splits.
    //
    // `rw.visible_line_capacity`, not `rw.lines.len()`: the latter
    // is how many rows the *buffer* had content for last frame, which on a
    // buffer shorter than the window is less than the window's actual row
    // capacity — `RenderedWindow::lines`'s own doc has the full case this
    // under-counted. Feeding that short count into `view.viewport_lines`
    // pinned a freshly-opened one-line buffer's viewport to "1 row tall";
    // the very next edit that grew the buffer (`o<text><Esc>`) then ran
    // `ensure_cursor_visible` against that stale 1-row belief and scrolled
    // line 0 out of view to keep the cursor's new line "on screen" —
    // reproduced both over a real pty (tests/pty_open_line_below_paints_all_lines.rs) and in-process (src/tui_main/app_on_tui_tests.rs's opening_a_line_below_the_last_line_paints_every_line_in_order_1779, via TuiDriver::tick()) between the idle tick that caches this value and the keystroke that acts on it.
    if let Some(layout) = app.cached_screen_layout.borrow().as_ref() {
        for rw in &layout.windows {
            engine.set_viewport_for_window(
                rw.window_id,
                rw.visible_line_capacity.max(1),
                rw.text_viewport_cols.max(1),
            );
        }
    }

    // Re-check every group's active tab is still on-screen against the
    // widths this frame's `TabBars` rung actually painted.
    let counts = app.tab_visible_counts.borrow().clone();
    if !counts.is_empty() && engine.post_draw_apply_widths(&counts) {
        needs_redraw = true;
    }

    // Sync the OS window title. Routed through `Backend::window()`
    // rather than a backend-specific title setter — this is backed on
    // every windowed backend.
    let win_title = window_title(engine);
    if let Some(w) = backend.window() {
        // Only write when the title actually changed since the last tick.
        // `window_title` is stable at idle, so without this guard every
        // idle tick re-issues the identical `WindowControl::set_title` call.
        if crate::app::dedup_window_title(&mut app.last_window_title, &win_title) {
            let _ = w.set_title(&win_title);
        }
        // Refresh the session-restore maximized cache unconditionally
        // — unlike size/position below, this is exactly the one moment those
        // freeze, so it must always be current.
        let maximized = w.is_maximized();
        if let Ok(m) = maximized {
            app.cached_window_maximized.set(m);
        }
        // Refresh the session-restore size/position cache — see
        // `App::cached_window_width`'s doc for why this is cached here,
        if matches!(maximized, Ok(false)) {
            if let Ok(bounds) = w.bounds() {
                app.cached_window_width.set(bounds.width.round() as i32);
                app.cached_window_height.set(bounds.height.round() as i32);
                // `WindowControl::bounds` always reports `x: 0.0, y: 0.0`
                // on GTK/Wayland (meaning "unknown", not "at the screen
                // origin") — skip caching that sentinel.
                if bounds.x != 0.0 || bounds.y != 0.0 {
                    app.cached_window_x.set(Some(bounds.x.round() as i32));
                    app.cached_window_y.set(Some(bounds.y.round() as i32));
                }
            }
        }
    }

    // Poll the yank-highlight deadline armed by `run_post_key_epilogue`.
    if let Some(deadline) = app.yank_hl_deadline.get() {
        if std::time::Instant::now() >= deadline {
            engine.clear_yank_highlight();
            app.yank_hl_deadline.set(None);
            needs_redraw = true;
        }
    }

    // Periodic background work: LSP, DAP, git, search, etc.
    if engine.poll_idle() {
        needs_redraw = true;
        // A redraw trigger only under the `ShellApp` runner (see
        // `App::sync_sidebar_from_engine`'s own doc comment, the method this
        // used to call).
        app.draw_needed.set(true);
    }

    // Auto-refresh source control periodically, gated on sidebar visibility
    // and the active panel being one SC touches. The elapsed check only resets
    // `last_sc_refresh` while the sidebar is actually visible, so while hidden
    // the 2s timer is never reset and fires once the first tick after showing.
    //
    // Spawning a background thread has no visible effect of its own, so this
    // must NOT set `needs_redraw`. The only thing that should trigger a redraw
    // is `poll_sc_refresh` actually finding changed data, once the async
    // fetch completes.
    if engine.app_shell.sidebar_visible()
        && app.last_sc_refresh.elapsed() >= std::time::Duration::from_secs(2)
    {
        app.last_sc_refresh = std::time::Instant::now();
        if matches!(
            sidebar_owner(engine),
            SidebarOwner::Git | SidebarOwner::Explorer
        ) {
            engine.sc_refresh_async();
        }
    }
    if engine.poll_sc_refresh() {
        needs_redraw = true;
    }

    // Format-on-save + :wq/:x deferred quit.
    if engine.format_save_quit_ready {
        engine.format_save_quit_ready = false;
        app.quit_and_save_session(engine);
    }

    // Run a pending terminal command (needs a live column/row count).
    if let Some(cmd) = engine.pending_terminal_command.take() {
        let cols = app.terminal_panel_cols(app.painted_editor_content_width());
        let rows = engine.session.terminal_panel_rows;
        engine.terminal_run_command(&cmd, cols, rows);
        needs_redraw = true;
    }

    // Focus a panel revealed by plugin logic this frame.
    if let Some(panel_name) = engine.ext_panel_focus_pending.take() {
        if !engine.app_shell.sidebar_visible() {
            engine.app_shell.toggle_sidebar();
        }
        engine.ext_panel_has_focus = true;
        engine.ext_panel_active = Some(panel_name);
        app.sync_sidebar_widgets();
        needs_redraw = true;
    }

    // Drain platform actions (open URL / reveal in file manager) queued by
    // engine logic this frame — needs `backend`'s `PlatformServices`, which
    // `core/engine/` has no handle to.
    let actions = std::mem::take(&mut engine.pending_platform_actions);
    if !actions.is_empty() {
        for action in actions {
            run_pending_platform_action(engine, action, backend);
        }
        needs_redraw = true;
    }

    needs_redraw
}

// ─── Menu-bar app icon ──────────────────────────────────────────────────────

/// The VimCode app icon's encoded bytes.
///
/// The **single** embedded copy in the tree: `gtk::util::install_icon_and_desktop`
/// writes these same bytes into the icon theme, and the GTK menu row paints
/// them via [`app_icon_image`]. #716 consolidated three competing app
/// identities onto `data/icons/io.github.jdonaghy.VimCode.svg`; #720 hangs a
/// second consumer off it and must not fork a second `include_bytes!` of the
/// same file (see that issue's "do not add a third copy of the SVG").
pub const APP_ICON_SVG: &[u8] = include_bytes!("../../data/icons/io.github.jdonaghy.VimCode.svg");

/// Intrinsic size declared by [`APP_ICON_SVG`]'s `viewBox` (`0 0 1024 1024`).
///
/// Handed to `quadraui::Image::intrinsic_size` so `Image::layout` can preserve
/// the aspect ratio *without* decoding. It happens to be square, so
/// [`ImageFit::Contain`](quadraui::ImageFit::Contain) never letterboxes inside
/// the square slot [`split_menu_row_for_app_icon`] hands it — but the field is
/// still populated rather than left `None`, because `None` silently degrades
/// to `Fill` (stretch) if the asset is ever replaced with a non-square one.
const APP_ICON_INTRINSIC_SIZE: (u32, u32) = (1024, 1024);

/// Fraction of the menu row's height left as breathing room above and below
/// the app icon, so the glyph doesn't touch the window edge or the tab bar.
const APP_ICON_INSET_FRACTION: f32 = 0.18;

/// Width reserved at the leading edge of the menu row for the app icon.
///
/// Deliberately derived from the menu row's own height, not from
/// `settings.font_size` — the icon is window chrome and must not grow when
/// the user bumps the editor font.
pub fn menu_bar_app_icon_slot_width_px(menu_row_height: f32) -> f32 {
    if menu_row_height <= 0.0 {
        0.0
    } else {
        menu_row_height
    }
}

/// Shift `row`'s leading edge in by `leading_inset` without moving its
/// trailing edge, clamping so the result never has negative width or moves
/// past the row's own trailing edge (a pathologically large inset yields a
/// zero-width row, not a wrapped/negative one).
///
/// Shared by [`split_menu_row_for_app_icon`] — which uses it to
/// carve the drawn-menu-row's icon/items split clear of the backend's own
/// controls — and `App::render_content`'s native-menu-row path, which insets
/// the *whole* row the same way when the drawn menu row itself is suppressed
/// and there is no icon/items split to compute. Extracted so those two
/// call sites can't compute the same clamp arithmetic two different ways.
pub fn inset_titlebar_row_leading_edge(row: quadraui::Rect, leading_inset: f32) -> quadraui::Rect {
    let leading_inset = leading_inset.max(0.0).min(row.width);
    quadraui::Rect::new(
        row.x + leading_inset,
        row.y,
        (row.width - leading_inset).max(0.0),
        row.height,
    )
}

/// True when the backend already draws its own titlebar window controls
/// in this band — macOS's native traffic lights, reported via a non-default
/// [`quadraui::Backend::titlebar_control_inset`]. `control_inset` is
/// [`quadraui::Rect::default()`] on every backend that has not opted into
/// [`quadraui::shell::ShellConfig::client_side_titlebar`], so this is `false`
pub fn backend_draws_own_window_controls(control_inset: quadraui::Rect) -> bool {
    control_inset.width > 0.0 || control_inset.height > 0.0
}

/// True when vimcode's own drawn window controls should paint into the
/// title-bar band this frame. When both `menu_row_present` and a backend's
/// native controls are true, the native controls must win — never paint both.
pub fn should_draw_window_controls(menu_row_present: bool, control_inset: quadraui::Rect) -> bool {
    menu_row_present && !backend_draws_own_window_controls(control_inset)
}

/// Split the full menu row band into `(icon_rect, items_rect)`. `items_rect`
/// is what must be handed to `MenuSystem::render` and `MenuSystem::handle`.
///
/// `icon_rect` is a square inset vertically inside the slot by
/// [`APP_ICON_INSET_FRACTION`], horizontally centred in the reserved slot.
///
/// `leading_inset` shifts where the band effectively starts without moving its
/// trailing edge — the region occupied by the backend's own drawn controls
/// (macOS's native traffic lights). `0.0` on every backend without the
/// client-side-titlebar opt-in. Clamped to `menu_row_rect.width` so a
/// pathologically large inset cannot produce a negative-width row.
pub fn split_menu_row_for_app_icon(
    menu_row_rect: quadraui::Rect,
    leading_inset: f32,
) -> (quadraui::Rect, quadraui::Rect) {
    let row = inset_titlebar_row_leading_edge(menu_row_rect, leading_inset);
    let slot = menu_bar_app_icon_slot_width_px(row.height).min(row.width);
    if slot <= 0.0 {
        return (quadraui::Rect::new(row.x, row.y, 0.0, 0.0), row);
    }
    let inset = row.height * APP_ICON_INSET_FRACTION;
    // Clamp to `slot`: `slot = height.min(width)`, so in the pathological
    // case of a row narrower than it is tall, an unclamped `side` (derived
    // from height alone) could extend past `items`' left edge. Not
    // reachable with any real window -- row height is always far smaller
    // than window width -- but keeping the icon inside its own reserved
    // slot is a one-line invariant worth holding regardless.
    let side = (row.height - 2.0 * inset).max(1.0).min(slot);
    let icon = quadraui::Rect::new(
        row.x + ((slot - side) / 2.0).max(0.0),
        row.y + inset,
        side,
        side,
    );
    let items = quadraui::Rect::new(row.x + slot, row.y, (row.width - slot).max(0.0), row.height);
    (icon, items)
}

/// The app icon as a `quadraui::Image`, backed by the raw SVG. `fallback_text`
/// is empty by design: the icon is purely decorative chrome, so a backend
/// that cannot rasterise or a decode failure should leave the slot blank.
pub fn app_icon_image() -> quadraui::Image {
    quadraui::Image {
        id: quadraui::WidgetId::new("app-icon"),
        source: quadraui::ImageSource::Bytes(APP_ICON_SVG.to_vec()),
        intrinsic_size: Some(APP_ICON_INTRINSIC_SIZE),
        fit: quadraui::ImageFit::Contain,
        fallback_text: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Outer window resize grip ─────────────────────────────────────────
    //
    // Unit tests extracted because they don't need a window at all:
    // `WINDOW_RESIZE_GRIP_PX`'s value (a regression guard on the constant)
    // and `resize_edge_overlaps_change_review_band` (the decision function
    // `App::handle` calls).

    /// The grip must be thin — a handful of pixels, VS Code/Electron
    /// style — not `backend.line_height()` (16-22px). A full row-height grip
    /// is exactly what made North/South resize dead: it made the grip band as
    /// tall as the title bar / command-line rows, so those rows' own
    /// unconditional click handling always resolved first. Pinned as an
    /// upper bound well below any real line height so a future edit can't
    /// silently widen the constant back into that failure mode; see
    /// `WINDOW_RESIZE_GRIP_PX`'s own doc for the full rationale.
    #[test]
    fn window_resize_grip_is_a_thin_pixel_margin_not_a_row_height() {
        assert!(
            WINDOW_RESIZE_GRIP_PX > 0.0,
            "a zero-width grip could never resize at all"
        );
        assert!(
            WINDOW_RESIZE_GRIP_PX <= 8.0,
            "grip {WINDOW_RESIZE_GRIP_PX}px is no longer a thin pixel margin \
             — a real GTK title-bar/command-line row is comfortably taller \
             than this, which is the whole point (#1528)"
        );
    }

    /// The grip must stay inside the vertical scrollbar's own gutter rather
    /// than reach past it into the minimap strip. Checked across a realistic
    /// range of GTK char widths with `scrollbar_reserve` at its common `0.0`
    /// (overlay-scrollbar) so the gutter collapses to `char_width` alone.
    #[test]
    fn window_resize_grip_stays_inside_the_scrollbar_gutter() {
        for char_width in [6.0_f64, 7.0, 8.8, 11.0, 16.0, 24.0] {
            let gutter = scroll_gutter_width(0.0, char_width);
            assert!(
                f64::from(WINDOW_RESIZE_GRIP_PX) <= gutter,
                "grip {WINDOW_RESIZE_GRIP_PX}px exceeds the scroll gutter \
                 ({gutter}px) at char_width {char_width}px — it would reach \
                 past the vertical scrollbar's own column into the minimap \
                 strip beyond it"
            );
        }
    }

    /// The change-review guard must cover only the edges that actually
    /// overlap the change-review surface's first row — `North`/`NorthEast`/
    /// `NorthWest` — and must NOT cover other edges. A regression that
    /// widened the guard would silently make resizing impossible while a
    /// change-review surface is open.
    #[test]
    fn resize_edge_overlaps_change_review_band_only_covers_the_top() {
        use quadraui::ResizeEdge;

        assert!(resize_edge_overlaps_change_review_band(ResizeEdge::North));
        assert!(resize_edge_overlaps_change_review_band(
            ResizeEdge::NorthEast
        ));
        assert!(resize_edge_overlaps_change_review_band(
            ResizeEdge::NorthWest
        ));

        assert!(!resize_edge_overlaps_change_review_band(ResizeEdge::South));
        assert!(!resize_edge_overlaps_change_review_band(
            ResizeEdge::SouthEast
        ));
        assert!(!resize_edge_overlaps_change_review_band(
            ResizeEdge::SouthWest
        ));
        assert!(!resize_edge_overlaps_change_review_band(ResizeEdge::East));
        assert!(!resize_edge_overlaps_change_review_band(ResizeEdge::West));
    }

    #[test]
    fn command_line_selection_allowed_gates_on_mode_and_message() {
        let mut e = Engine::new();
        e.mode = Mode::Command;
        assert!(command_line_selection_allowed(&e));
        e.mode = Mode::Search;
        assert!(command_line_selection_allowed(&e));

        e.mode = Mode::Normal;
        e.message.clear();
        assert!(!command_line_selection_allowed(&e));
        e.message = "hello".to_string();
        assert!(command_line_selection_allowed(&e));

        e.mode = Mode::Insert;
        assert!(
            !command_line_selection_allowed(&e),
            "Insert mode has no command/message line to select"
        );
    }

    /// When a count and message are both present — a valid state when the
    /// digit-accumulation cap is hit — `build_command_line` paints the count,
    /// not the message. A selection gate checking only `!message.is_empty()`
    /// would arm selection against text nobody painted.
    #[test]
    fn command_line_selection_allowed_false_when_count_shadows_message() {
        let mut e = Engine::new();
        e.mode = Mode::Normal;
        e.count = Some(10_000);
        e.message = "Count limited to 10,000".to_string();
        assert!(
            !command_line_selection_allowed(&e),
            "a live count shadows the message in build_command_line's paint \
             path, so selection must not arm against the (unpainted) message"
        );

        // Once the count is consumed, the same message alone is selectable.
        e.count = None;
        assert!(command_line_selection_allowed(&e));
    }

    #[test]
    fn command_line_click_char_idx_maps_x_to_a_character_column() {
        let rect = quadraui::Rect::new(10.0, 5.0, 20.0, 1.0);
        // ":wq" at char_width 1.0 — column 1 is 'w'.
        let idx = command_line_click_char_idx(rect, ":wq", 1.0, quadraui::Point::new(11.5, 5.0));
        assert_eq!(idx, Some(1));
    }

    #[test]
    fn command_line_click_char_idx_none_outside_the_painted_row() {
        let rect = quadraui::Rect::new(10.0, 5.0, 20.0, 1.0);
        // Above the row.
        assert_eq!(
            command_line_click_char_idx(rect, ":wq", 1.0, quadraui::Point::new(15.0, 4.0)),
            None
        );
        // Left of the row (e.g. the activity bar / sidebar columns on TUI).
        assert_eq!(
            command_line_click_char_idx(rect, ":wq", 1.0, quadraui::Point::new(5.0, 5.0)),
            None
        );
    }

    #[test]
    fn command_line_click_char_idx_clamps_past_the_end_of_text() {
        let rect = quadraui::Rect::new(0.0, 0.0, 20.0, 1.0);
        // ":wq" is 3 chars; clicking column 15 must clamp to the end, not
        // return an out-of-range index (`ee26268`'s raw `col - editor_left`
        // had no such clamp).
        let idx = command_line_click_char_idx(rect, ":wq", 1.0, quadraui::Point::new(15.0, 0.0));
        assert_eq!(idx, Some(3));
    }

    #[test]
    fn command_line_click_char_idx_accounts_for_multibyte_chars() {
        // ":éditer" — 'é' is 2 bytes; the returned offset is a CHAR count
        // (matching `Engine::command_cursor`'s unit), not a byte count.
        let rect = quadraui::Rect::new(0.0, 0.0, 20.0, 1.0);
        let idx = command_line_click_char_idx(rect, ":éditer", 1.0, quadraui::Point::new(2.5, 0.0));
        // Column 2 ('d', right after the 2-byte 'é') is char index 2.
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn command_line_selection_bytes_converts_char_indices_to_byte_offsets() {
        // ":éditer" — 'é' is 2 bytes. Inclusive char selection (1, 3) covers
        // chars 1..=3 ("édi"); char 1 starts at byte 1, char 4 ('t', one
        // past the inclusive end) starts at byte 5.
        let (lo, hi) = command_line_selection_bytes(":éditer", (1, 3));
        assert_eq!((lo, hi), (1, 5));
    }

    #[test]
    fn command_line_selection_bytes_is_order_independent_and_end_inclusive() {
        // No multibyte prefix: char count and byte count coincide. (1, 2)
        // inclusive covers chars 1 and 2 ("wq" of ":wq!") -> exclusive byte
        // range [1, 3). Either endpoint order gives the same result.
        assert_eq!(command_line_selection_bytes(":wq!", (1, 2)), (1, 3));
        assert_eq!(command_line_selection_bytes(":wq!", (2, 1)), (1, 3));
    }

    #[test]
    // ── Menu-bar app icon geometry ───────────────────────────────────────
    #[test]
    fn app_icon_slot_shifts_menu_items_by_exactly_its_width() {
        let row = quadraui::Rect::new(10.0, 5.0, 800.0, 30.0);
        let (icon, items) = split_menu_row_for_app_icon(row, 0.0);
        let slot = menu_bar_app_icon_slot_width_px(row.height);

        assert_eq!(slot, 30.0, "the slot is the row height, making it square");
        assert_eq!(items.x, row.x + slot);
        assert_eq!(items.width, row.width - slot);
        assert_eq!(items.y, row.y);
        assert_eq!(items.height, row.height);
        assert!(
            icon.x + icon.width <= items.x,
            "the icon must not overlap the items strip; icon={icon:?} items={items:?}"
        );
    }

    /// The icon is chrome: its size must track the menu row height and nothing
    /// else, so bumping `settings.font_size` cannot grow it.
    #[test]
    fn app_icon_size_tracks_row_height_only() {
        let short = split_menu_row_for_app_icon(quadraui::Rect::new(0.0, 0.0, 800.0, 24.0), 0.0).0;
        let tall = split_menu_row_for_app_icon(quadraui::Rect::new(0.0, 0.0, 800.0, 48.0), 0.0).0;
        assert!(tall.height > short.height);
        // Square, and strictly inside the row on both edges.
        for (icon, h) in [(short, 24.0_f32), (tall, 48.0_f32)] {
            assert_eq!(icon.width, icon.height, "icon must be square: {icon:?}");
            assert!(
                icon.height < h,
                "icon must be inset inside the row: {icon:?}"
            );
            assert!(
                icon.y > 0.0,
                "icon must be inset from the row top: {icon:?}"
            );
        }
        // Widening the row (a wider window) must not change the icon at all.
        let wide = split_menu_row_for_app_icon(quadraui::Rect::new(0.0, 0.0, 4000.0, 24.0), 0.0).0;
        assert_eq!(wide, short);
    }

    /// A hidden / zero-height menu row must reserve nothing, so callers can
    /// split unconditionally without the items strip silently losing width.
    #[test]
    fn zero_height_menu_row_reserves_no_icon_slot() {
        let row = quadraui::Rect::new(0.0, 0.0, 800.0, 0.0);
        let (icon, items) = split_menu_row_for_app_icon(row, 0.0);
        assert_eq!(menu_bar_app_icon_slot_width_px(row.height), 0.0);
        assert_eq!(icon.width, 0.0);
        assert_eq!(icon.height, 0.0);
        assert_eq!(items, row, "the items strip must be the untouched row");
    }

    /// A window narrower than the slot must not produce a negative-width
    /// items rect (which would wrap or panic downstream in `MenuBar::layout`).
    #[test]
    fn menu_row_narrower_than_the_icon_slot_clamps_to_zero_width_items() {
        let row = quadraui::Rect::new(0.0, 0.0, 8.0, 30.0);
        let (_, items) = split_menu_row_for_app_icon(row, 0.0);
        assert!(items.width >= 0.0, "got {items:?}");
        assert_eq!(items.width, 0.0);
    }

    /// A row narrower than it is tall must not let the
    /// height-derived `side` spill past the reserved `slot` (`slot =
    /// height.min(width)`) into, or past, `items`' left edge. Not reachable
    /// with any real window (row height is always far smaller than window
    /// width), but the invariant -- the icon rect never extends beyond its
    /// own slot -- should hold unconditionally.
    #[test]
    fn menu_row_narrower_than_the_icon_slot_keeps_the_icon_inside_the_slot() {
        let row = quadraui::Rect::new(0.0, 0.0, 8.0, 30.0);
        let (icon, items) = split_menu_row_for_app_icon(row, 0.0);
        let slot = menu_bar_app_icon_slot_width_px(row.height).min(row.width);
        assert!(
            icon.x + icon.width <= row.x + slot,
            "icon must stay within its reserved slot; icon={icon:?} slot_end={}",
            row.x + slot
        );
        assert!(
            icon.x + icon.width <= items.x,
            "icon must not extend into the items rect; icon={icon:?} items={items:?}"
        );
    }

    // ── Client-side titlebar leading-edge inset ─────────────────────────

    /// A non-zero `leading_inset` must push both the icon and the items
    /// strip clear of it, without moving the band's trailing edge.
    #[test]
    fn leading_inset_pushes_icon_and_items_clear_of_native_controls() {
        let row = quadraui::Rect::new(10.0, 5.0, 800.0, 30.0);
        let inset = 78.0; // roughly macOS's traffic-light cluster width
        let (icon, items) = split_menu_row_for_app_icon(row, inset);
        let (icon_uninset, _) = split_menu_row_for_app_icon(row, 0.0);

        assert_eq!(
            icon.x,
            icon_uninset.x + inset,
            "the icon must be shifted exactly `inset` past where it would \
             otherwise sit"
        );
        assert_eq!(
            icon.width, icon_uninset.width,
            "the inset shifts the icon, it must not resize it"
        );
        assert!(
            items.x >= icon.x + icon.width,
            "items must not overlap the icon; items={items:?} icon={icon:?}"
        );
        assert_eq!(
            items.x + items.width,
            row.x + row.width,
            "the trailing edge of the band must not move — only the leading \
             edge is inset"
        );
    }

    /// An inset wider than the whole row must clamp to an empty band rather
    /// than produce a negative-width rect (which would panic or wrap
    /// downstream in `MenuBar::layout`, the same hazard
    /// `menu_row_narrower_than_the_icon_slot_clamps_to_zero_width_items`
    /// guards against for the icon-slot width alone).
    #[test]
    fn leading_inset_wider_than_the_row_clamps_to_an_empty_band() {
        let row = quadraui::Rect::new(0.0, 0.0, 50.0, 30.0);
        let (icon, items) = split_menu_row_for_app_icon(row, 500.0);
        assert!(icon.width >= 0.0 && items.width >= 0.0);
        assert_eq!(items.width, 0.0);
    }

    /// `inset_titlebar_row_leading_edge` is the shared clamp arithmetic
    /// `split_menu_row_for_app_icon` and `App::render_content`'s
    /// native-menu-row path both need. Pins the exact contract both
    /// callers depend on: leading edge moves in by `leading_inset`,
    /// trailing edge never moves, y/height untouched.
    #[test]
    fn inset_titlebar_row_leading_edge_moves_only_the_leading_edge() {
        let row = quadraui::Rect::new(10.0, 5.0, 800.0, 30.0);
        let inset = inset_titlebar_row_leading_edge(row, 78.0);
        assert_eq!(inset.x, row.x + 78.0);
        assert_eq!(inset.y, row.y);
        assert_eq!(inset.width, row.width - 78.0);
        assert_eq!(inset.height, row.height);
        assert_eq!(
            inset.x + inset.width,
            row.x + row.width,
            "trailing edge must not move"
        );
    }

    /// Same clamp-to-empty behaviour as `split_menu_row_for_app_icon`'s
    /// equivalent case, since both now share this helper: an inset wider
    /// than the row must never produce a negative width.
    #[test]
    fn inset_titlebar_row_leading_edge_clamps_an_oversized_inset() {
        let row = quadraui::Rect::new(0.0, 0.0, 50.0, 30.0);
        let inset = inset_titlebar_row_leading_edge(row, 500.0);
        assert_eq!(inset.width, 0.0);
        assert!(inset.x <= row.x + row.width);
    }

    /// `backend_draws_own_window_controls` is the pure predicate for whether
    /// the backend reports it draws its own window controls. `Rect::default()`
    /// — reported by every backend without the client-side-titlebar opt-in —
    /// must read as "no native controls"; any non-zero width or height must
    /// read as "yes".
    #[test]
    fn backend_draws_own_window_controls_reads_either_nonzero_dimension() {
        assert!(!backend_draws_own_window_controls(quadraui::Rect::default()));
        assert!(!backend_draws_own_window_controls(quadraui::Rect::new(
            5.0, 5.0, 0.0, 0.0
        )));
        assert!(backend_draws_own_window_controls(quadraui::Rect::new(
            0.0, 0.0, 78.0, 0.0
        )));
        assert!(backend_draws_own_window_controls(quadraui::Rect::new(
            0.0, 0.0, 0.0, 28.0
        )));
    }

    /// `should_draw_window_controls` is the exact "exactly one set of window
    /// controls" decision. Native controls always win: they must suppress
    /// vimcode's drawn controls regardless of `menu_row_present`.
    #[test]
    fn should_draw_window_controls_covers_all_four_combinations() {
        let none = quadraui::Rect::default();
        let native = quadraui::Rect::new(0.0, 0.0, 78.0, 28.0);

        assert!(
            should_draw_window_controls(true, none),
            "menu row present, no native controls -> vimcode draws its own \
             (every backend before #940, and GTK/Win-GUI forever)"
        );
        assert!(
            !should_draw_window_controls(false, none),
            "menu row suppressed, no native controls -> nobody draws \
             controls (would be a floating button cluster with no menu bar)"
        );
        assert!(
            !should_draw_window_controls(true, native),
            "menu row present but backend has native controls -> vimcode \
             must still suppress its own, or two sets of controls paint"
        );
        assert!(
            !should_draw_window_controls(false, native),
            "menu row suppressed and backend has native controls -> macOS \
             today: AppKit's traffic lights only, nothing drawn"
        );
    }

    /// The icon painted in the menu row and the icon installed into the
    /// desktop icon theme are the same artwork.
    #[test]
    fn app_icon_image_carries_the_shipped_svg_and_its_viewbox_size() {
        let img = app_icon_image();
        assert_eq!(
            img.source,
            quadraui::ImageSource::Bytes(APP_ICON_SVG.to_vec())
        );
        assert_eq!(img.intrinsic_size, Some(APP_ICON_INTRINSIC_SIZE));
        assert_eq!(img.fit, quadraui::ImageFit::Contain);
        let svg = std::str::from_utf8(APP_ICON_SVG).expect("the icon asset is text SVG");
        assert!(
            svg.contains("viewBox=\"0 0 1024 1024\""),
            "APP_ICON_INTRINSIC_SIZE must match the asset's own viewBox"
        );
    }
}
