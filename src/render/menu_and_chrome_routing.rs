use super::*;

// ─── Menu-action `EngineAction` applier rung (#1063) ────────────────────────
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
// Before this rung, GTK's `App::handle_menu_action` restated a *subset* of
// this match by hand — five variants named explicitly behind a bare `_ =>
// {}` catch-all — instead of reusing `App::dispatch_engine_action`, its own
// already-exhaustive general-purpose applier (used by the keyboard/macro
// paths). That catch-all is exactly the shape that hid #984 for months: a
// future menu item wired to a variant nobody had added an arm for would
// silently no-op instead of failing to compile. TUI's menu arm
// (`tui_main/shell_app.rs`) already routed through its own general-purpose
// applier (`dispatch_post_key_action`) — exhaustive, but a *different*
// function from GTK's, so the two could still drift independently even
// though neither, on its own, had a reachability gap today.
//
// #1499: [`apply_engine_action`] used to take a `host: &mut impl
// EngineActionHost` seam — a per-backend struct implementing eleven hook
// methods — for the dozen or so effects `Engine` cannot finish alone. With
// the TUI backend gone, `App` is the only implementation left, so those
// eleven hooks are inlined directly into the match arms below, reading/
// mutating `engine` and `app`'s fields exactly as `GtkEngineActionHost` (the
// old per-backend host struct) used to. `open_terminal`/`open_workspace_dialog`
// used to be `App::new_terminal_tab`/`App::open_workspace_dialog` verbatim,
// until #1063 left both with no other caller (menu, key and macro dispatch
// all go through here now) and deleted them rather than ship dead code —
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
        // Matches the former inline `EngineAction::QuitWithError` arm in
        // `App::dispatch_engine_action` exactly — no `save_session_state`
        // (unlike `Quit`/`SaveQuit` above). This asymmetry is GTK-specific,
        // not something `tui_main::handle_action` itself used to do: the
        // pre-#1434 TUI shell's `Quit`/`SaveQuit` and `QuitWithError` arms
        // both called `save_session`, i.e. TUI treated the two variants
        // *symmetrically*. The divergence was cross-backend (GTK skipped the
        // save on `QuitWithError`, TUI didn't); preserved here exactly as it
        // behaved pre-#1063 rather than changed as a side effect of this fold.
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
            let controller = quadraui::FolderPickerController::new(
                engine.cwd.clone(),
                vec![".vimcode-workspace".to_string()],
                engine.settings.show_hidden_files,
            );
            *app.folder_picker.borrow_mut() = Some(controller);
            app.draw_needed.set(true);
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
            // #1134: queue rather than shell out here — `apply_engine_action`
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

// ─── Explorer context-menu applier (#1418) ─────────────────────────────────
//
// The explorer context menu's *decision* — which item is highlighted, which
// key/click confirms it — was already shared (`Engine::handle_context_menu_key`
// / `Engine::context_menu_target_path`), but what happened with the
// confirmed action *string* was written once per backend: TUI's
// `handle_explorer_context_action` (`tui_main/mod.rs`) and GTK's
// `App::dispatch_explorer_ctx_action` (`app.rs`). They had already drifted —
// TUI's `"delete"` used the context menu's own explicit target path
// (`Engine::confirm_delete_file`); GTK's routed through
// `dispatch_explorer_crud(Delete)`, which acts on the explorer tree's
// *selected* row instead, picking the wrong file whenever a right-click
// target and the tree's selection disagree. `"find_in_folder"` was worse:
// TUI opened the (workspace-wide, not folder-scoped) Grep picker, GTK just
// focused the Search sidebar panel — a different feature entirely, despite
// both being wired to a menu item labelled "Find in Folder...".
//
// [`apply_explorer_context_action`] is the one function both backends now
// call. It picks TUI's explicit-target behavior for `"delete"`/`"move_file"`
// (`Engine::confirm_delete_file`/`Engine::start_move_file_dialog` take the
// path directly — no tree-selection ambiguity possible), and gives
// `"find_in_folder"` one real behavior on both backends:
// `Engine::open_grep_picker_scoped`, a live-grep search restricted to the
// clicked folder (#1418), matching the menu label for the first time on
// either backend.
//
// `new_file`/`new_folder`/`rename` keep routing through
// `dispatch_explorer_crud` (selected-row-based) — both backends already
// agreed on that subset before this issue, so it is not a divergence this
// rung needs to resolve, only preserve.
//
// `open_terminal_at` covers the one action that genuinely needs
// backend-specific plumbing: `"open_terminal"` needs the live terminal
// pane's column count, which only each backend's own runner has in scope.
// Every other action is a plain `Engine` call with no backend seam —
// including the caller's own post-action redraw/refresh bookkeeping
// (`explorer_needs_refresh`, `draw_needed`), which stays at each call site
// exactly as before since it is generic "something changed" plumbing
// already shared across far more than context-menu actions, not specific to
// this rung.
//
// #1499: this used to be a `host: &mut impl ExplorerContextHost` one-method
// trait, so a second backend (the pre-#1434 TUI shell) could supply its own
// terminal-opening plumbing. With that backend gone, `open_terminal_at`
// (a plain `FnMut`) is enough: `App` passes a closure over its own `&mut
// self`; the test-only caller in `core::engine::picker` passes a no-op —
// no trait, no per-backend struct, and no dependency from `core`'s test code
// onto `crate::app::App`.

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

// ─── Native file-dialog rung (#1125) ────────────────────────────────────────
//
// TUI's `save_workspace_as_dialog` used to hardcode `engine.cwd.join(
// ".vimcode-workspace")` and write it unconditionally — no prompt, no way to
// cancel, and silently ignoring whatever path the user actually wanted. GTK
// already did this right (`App::run_pending_file_dialog`, driven by
// `PendingFileDialog`/`tick()` because its `backend` handle is only reachable
// there — see that type's doc comment for why). quadraui#965 shipped the TUI
// half of the same primitive — `TuiPlatformServices::show_file_open_dialog` /
// `show_file_save_dialog`, a nested draw-and-read loop over
// `FilePickerController` — so TUI can call it too.
//
// TUI doesn't need GTK's `tick()` deferral: the pre-#1434 TUI shell's `handle` already has
// `backend: &mut dyn quadraui::Backend` in scope at both call sites that can
// open one of these dialogs (the `open_file_dialog` menu action, and
// `EngineAction::SaveWorkspaceAsDialog` via `TuiEngineActionHost`, which now
// carries a `backend` field for exactly this). [`run_open_file_dialog`] and
// [`run_save_workspace_as_dialog`] are the one shared body both GTK's
// deferred call site and TUI's synchronous ones call — the point of this rung
// is one implementation, two thin call sites, not a parallel TUI-only copy.

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
/// nothing when the user cancels — the #1125 fix.
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

// ─── Shell-event shadow-sync rung (#1062) ────────────────────────────────────
//
// `AppShellEvent::PanelChanged`/`SidebarHidden`/`SidebarResized` all report a
// decision the *runner's* own `AppShell` already made — an activity-bar
// click, a divider drag. Both backends mirror that decision into
// `engine.app_shell`, the "shadow" copy every engine-side consumer actually
// reads (`render_sidebar_content`'s panel dispatch, `active_panel_is`,
// `sidebar_visible()` hit-test gates, session persistence): the runner's own
// `AppShell` is what the *paint* geometry comes from, but `Engine` itself
// never consults it directly.
//
// #988 was one of these three forgetting the mirror entirely:
// `PanelChanged { hamburger }` returned early, before any shadow-sync
// statement ran, because the sync was spelled out fresh at each call site
// instead of owned by one function every call site is required to reach.
// [`sync_shell_event_shadow`] is that one function. `App::on_shell_event`
// calls it unconditionally, as the first thing it does, before any of its
// own id-specific branching — so the shadow mutation itself can no longer be
// skipped by an early `return` reached before it.
//
// #1499: this used to take a `host: &impl ShellShadowSyncHost` with one
// hook, `panel_absent_from_shadow`, so a second backend (the pre-#1434 TUI
// shell) could answer it differently. With that backend gone, `App`'s own
// answer is the only one left, so the predicate is inlined below instead of
// asked through a trait: the shadow `engine.app_shell` never gets a
// `PanelDefinition` for the hamburger panel (`App::shell_config` only
// registers it in the runner's own `AppShell`, on the `cell` profile, and
// even there it's runner-only — see
// [`reclaim_hamburger_sidebar_reservation`]'s doc), so a `PanelChanged` for
// it must always skip the generic sync below.

/// Mirror a runner-decided [`quadraui::AppShellEvent`] onto the shadow
/// `engine.app_shell` (#1062). Called by `App::on_shell_event` first,
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

// ─── Shared menu-bar reveal/hide routing (#1427) ────────────────────────────
//
// A backend whose menu bar can be fully hidden (`Engine::menu_bar_toggleable`
// — pre-#1427 only the pre-#1434 TUI shell's `setup` ever set this; now also `App::setup`
// on any backend whose `BackendCaps::window_chrome` is `false`, i.e. no real
// window chrome to double as the titlebar the way GTK's does — #901/#552)
// needs three pieces of behaviour a permanently-visible menu bar
// (GTK/macOS/Win) never does: an Alt+<letter> shim so a hidden bar's
// accelerators still work (#318), a one-shot guard that hides the bar again
// on the stale click a hamburger reveal leaves behind (#988/#1029), and a
// hamburger `PanelDefinition` click that reveals the bar instead of
// switching the (nonexistent) "Menu" sidebar panel. All five functions below
// were the pre-#1434 TUI shell's private methods before #1427; they are pure
// `&mut Engine` (plus, where a hit-test needs one, `&dyn Backend`/
// `&ShellContext`) so both `App` and the pre-#1434 TUI shell can call the same body.
// Every one is a documented no-op on a backend that never sets
// `menu_bar_toggleable`/never registers the hamburger panel (GTK/macOS/Win
// today) — see each function's own doc — so callers wire them in
// unconditionally rather than branching on `BackendCaps` themselves.

/// #1029: one-shot spend of [`Engine::hamburger_stale_click_guard`] for
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

/// #1029: spend [`Engine::hamburger_stale_click_guard`] on the
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

/// #318/#1029: the Alt+<letter> reveal shim plus the stale-hamburger-corner
/// hide. Takes `stale_hamburger_corner_click` — the result of
/// [`consume_hamburger_stale_click_guard`] — as a parameter rather than
/// calling it internally, because that consume has to happen at the very
/// top of `handle`/`handle_dispatch`, before *any* early exit (including
/// ones this function's caller can't see, e.g. the pre-#1434 TUI shell's `handle`'s
/// panel-accelerator dispatch), while this function itself is called later,
/// immediately before the caller's own `MenuSystem` intercept — see
/// `consume_hamburger_stale_click_guard`'s own doc for why the two calls
/// can't be merged into one without narrowing that guarantee.
///
/// 1. **Alt+<letter> shim** — when the bar is hidden, Alt+<letter> must
///    still reveal *and* activate the matching menu (otherwise the bare
///    letter falls through to `Engine::handle_key`, which ignores Alt, and
///    triggers a Vim motion instead — Alt+T → t-motion). Sets
///    `engine.menu_bar_visible = true` and lets the event keep flowing: the
///    caller's own `MenuSystem` intercept, called immediately after this
///    returns `None`, is what actually opens the menu using the
///    just-flipped flag. #1764: gated on the same
///    [`alt_mnemonic_open_allowed`] predicate the caller's intercept uses,
///    so the reveal and the open stay in lock-step — a mode this predicate
///    refuses (mid-text-entry on the toggleable profile) never reveals the
///    bar in the first place, rather than revealing it and then having the
///    caller refuse to open anything into it.
/// 2. **Stale hamburger-corner click** — revealing the bar shifts the whole
///    activity bar (hamburger included) down one row; a second click at the
///    *exact screen position* that revealed it (muscle memory) lands one row
///    too high, on the title-bar band, where it would otherwise open
///    whatever menu happens to paint at that column (`File`, at the default
///    sidebar width — the #988 symptom). Recognised structurally (a
///    `MouseDown` inside the title-bar band whose column still falls within
///    the activity bar's own width) and bounded to the one click immediately
///    following a reveal by `stale_hamburger_corner_click`'s one-shot
///    source, so a later, deliberate click on `File` falls through to the
///    caller's `MenuSystem` intercept like any other menu click.
///
/// Returns `Some(Reaction::Redraw)` only for case 2 — the caller must return
/// immediately, without running its own dispatch for this event. `None`
/// means "not consumed" (including case 1's reveal — see above).
///
/// A no-op on a backend that never sets `menu_bar_toggleable` (so
/// `menu_bar_visible` is always already `true` — GTK/macOS/Win): the shim's
/// `!menu_bar_visible` gate never opens, and the guard this reads is only
/// ever armed by a hamburger panel click, which those backends never
/// register (`App::shell_config`'s `px` profile has no hamburger
/// `PanelDefinition` — see that method's own doc).
pub fn route_menu_bar_reveal(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    stale_hamburger_corner_click: bool,
    backend: &mut dyn quadraui::Backend,
    ctx: &quadraui::ShellContext<'_>,
) -> Option<quadraui::Reaction> {
    if !engine.menu_bar_visible {
        if let quadraui::UiEvent::KeyPressed { key, modifiers, .. } = event {
            // #1764 (review finding, round 1): this shim and the caller's
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

/// #695/#1029: resolve the rect [`quadraui::MenuSystem::handle`]/`::render`
/// should hit-test/paint against for a toggleable-menu-bar backend, given
/// this frame's cached title-bar-band rect (`App::menu_items_rect`/
/// `Engine::menu_bar_rect`, both written once per paint).
///
/// [`route_menu_bar_reveal`]'s Alt+<letter> shim can flip `menu_bar_visible`
/// from `false` to `true` and the caller's `MenuSystem` intercept can fire
/// in that *same* dispatch — before `render_content` ever runs again to
/// refresh the cache, so it can still hold the empty/never-painted rect
/// from the last frame the bar was hidden. Handing `MenuSystem::handle` that
/// rect makes it lay out zero visible items and then index into that empty
/// list assuming at least one fits — a panic, not a no-op. Falls back to a
/// full-viewport-width, one-row rect (matching every toggleable profile's
/// `title_bar_height_lh == 1.0`) for exactly that one just-revealed-but-not-
/// yet-painted frame; checks both `width` and `height` against the cached
/// rect, not `height` alone, so a viewport whose computed width collapses to
/// zero can't sneak a real-but-zero-width rect past this guard either.
///
/// `toggleable` gates the whole fallback: `false` (GTK/macOS/Win, whose
/// `menu_bar_visible` is pinned before this can ever matter) always returns
/// `cached` unchanged, byte-for-byte the pre-#1427 behaviour on those
/// backends.
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

/// #635 (Stage 6b item A): keep the runner's `AppShell` title-bar row
/// reservation in sync with `engine.menu_bar_visible` — that flag can flip
/// via [`route_menu_bar_reveal`] above, `render::dispatch_panel_accelerator`,
/// `:set menu`, or (for the hamburger specifically) [`route_hamburger_panel_
/// changed`]/[`route_hamburger_sidebar_hidden`] below, and none of those
/// touch the runner's own `AppShell` on their own — only
/// `AppShell::set_title_bar_visible` does. Call unconditionally on the way
/// out of every dispatch (`handle`/`handle_dispatch`'s tail,
/// `on_shell_event_ctx`'s tail) rather than only from the specific arms that
/// change the flag, per `AppShell::set_title_bar_visible`'s own doc
/// (quadraui#532): "toggling this and calling [layout/render] next is
/// sufficient". A no-op whenever the flag hasn't changed since the last
/// call.
pub fn sync_menu_bar_title_row(ctx: &quadraui::ShellContext<'_>, engine: &Engine) {
    ctx.shell_mut()
        .set_title_bar_visible(engine.menu_bar_visible);
}

/// #1029: the hamburger's own `AppShellEvent::PanelChanged` — reveals the
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

/// #1029 (defect 2 of #988): the hamburger's own second click — reported as
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

/// #1029: correct a frame's [`quadraui::AppShellLayout`] for the hamburger's
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

/// #634 smoke retry (widened #1427): keep the runner `AppShell`'s sidebar
/// *visibility* in sync with the shadow `engine.app_shell`'s. Every keyboard
/// path (Ctrl+B-style toggles, `toggle_sidebar_panel` via panel
/// accelerators, autohide, Ctrl+W overflow) mutates the shadow, while the
/// *runner's* own `AppShell` owns whether `sidebar_content_bounds` exists at
/// all in the painted layout — so this has to be pushed through explicitly.
/// The active-*panel* half of this sync lives in `ShellApp::
/// take_requested_panel` (also covers tick-driven switches, which never
/// reach here); this is the visibility half, with no adapter hook of its
/// own, so callers push it unconditionally and idempotently on the way out
/// of every dispatch.
///
/// #1029 (defect 2 of #988, widened #1427): the hamburger is special-cased
/// out of this sync entirely while it's the reveal actually in effect. The
/// shadow has no hamburger `PanelDefinition` at all (see
/// [`sync_shell_event_shadow`]'s `PanelChanged` arm), so
/// `shadow_visible` can never reflect a hamburger reveal — it's always
/// whatever the last *real* panel click left behind. Without the
/// `runner_shows_hamburger` guard, the very next dispatch after a hamburger
/// click (a `WindowFocused` pump, or any other unrelated keypress) would
/// read that stale `false` and force-hide the runner's own sidebar in
/// response — leaving `active_panel == Some(hamburger)` but
/// `sidebar_visible == false` by the time a second click lands. That
/// permanently blocks `AppShell::handle_activity_click`'s "already active +
/// visible → hide" branch for the hamburger: every click, first or Nth,
/// resolves as a fresh `PanelChanged` reveal, never a `SidebarHidden` — the
/// second click that's supposed to hide the menu bar again does nothing
/// (#988).
///
/// Gated on `engine.menu_bar_visible` too, not just "is the runner showing
/// the hamburger" — `AppShell::new` defaults `active_panel` to index 0,
/// which the hamburger occupies (`App::shell_config`'s `cell`-profile
/// branch / the pre-#1434 TUI shell's `build_shell_config` both put it first), so a
/// *fresh* runner starts "showing the hamburger" before any click ever
/// happens. `menu_bar_visible` is what distinguishes an actual, user-driven
/// reveal from that construction-time default — without it, this guard
/// would also suppress the very first dispatch's correction of
/// `AppShell::new`'s own `sidebar_visible: true` default, leaving a phantom
/// "Menu" sidebar pane reserved forever (and, on the toggleable-menu-bar
/// profile specifically, permanently blocking the very first hamburger
/// click from ever registering as a reveal in the first place — the
/// runner's own construction-time default already reads "active +
/// visible", so without this correction running once, unconditionally,
/// before the first real click, that click would resolve as the toggle-hide
/// branch instead).
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
            // #557: while a plugin panel is open the shadow's active-panel
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

// ─── Shared tick-chore rung (#1248) ──────────────────────────────────────────
//
// `App::handle_poll_tick`/`tick_dispatch` (GTK) and the pre-#1434 TUI shell's `tick` (TUI)
// each ran the *same* dozen-item background chore list every frame, written
// out twice: sync each window's viewport from the last paint, re-check
// tab-bar scroll offsets against what that paint measured, clear an expired
// yank highlight, poll LSP/DAP/search idle work, refresh source control on a
// timer, finish a deferred format-then-quit, run a queued terminal command,
// focus a just-revealed extension panel, drain queued platform actions, and
// sync the OS window title — a byte-identical `format!("VimCode \u{2014}
// {}", ...)` on both sides. [`run_shared_tick_chores`] is the one function
// both `tick()`s now call for all of it.
//
// The handful of effects that genuinely differ per backend — how each stores
// its "last paint" cache (`RefCell<Option<ScreenLayout>>` under two different
// field names), how quitting actually ends the process (GTK sets a
// `Cell<bool>` the runner polls next frame; TUI returns `Reaction::Exit`
// directly), whether a terminal command needs a live `backend` handle for
// sizing — used to live behind a `host: &mut impl TickHost` seam, following
// [`PanelAcceleratorHost`]/[`EngineActionHost`]'s established shape (#1499: a
// small per-backend host struct holding `&mut App`/its own fields plus
// `backend`, constructed fresh at each call site). With the TUI backend
// gone, `App` is the only implementation left, so [`run_shared_tick_chores`]
// takes `app: &mut App` and `backend: &mut dyn quadraui::Backend` directly
// and inlines what used to be `GtkTickHost`'s method bodies. The two
// TUI-only hooks the old trait gave a no-op default for on GTK —
// `tab_switcher_gate` (skip the rest of the chore list while TUI's
// tab-switcher popup is mid-cycle) and `on_sidebar_refresh_tick` (rebuild
// TUI's explorer row cache on the periodic sidebar tick) — are simply gone
// below rather than inlined as always-no-op calls: `App` never overrode
// either, so dropping them changes nothing observable.

/// The OS/taskbar window title vimcode uses everywhere — `"VimCode —
/// <buffer name>"`, or the bare app name with none open. Used to be
/// formatted identically, and independently, in `App::handle_poll_tick` and
/// the pre-#1434 TUI shell's `tick`; now the one string
/// [`run_shared_tick_chores`]'s window-title chore formats (#1248).
pub fn window_title(engine: &Engine) -> String {
    engine
        .active_buffer_name()
        .map(|n| format!("VimCode \u{2014} {}", n))
        .unwrap_or_else(|| "VimCode".to_string())
}

/// Carry out a queued [`crate::core::engine::PendingPlatformAction`] (open
/// URL / reveal in file manager) via `backend`'s `PlatformServices`. Was
/// `App::run_pending_platform_action` / the pre-#1434 TUI shell's `run_pending_platform_action`
/// — two copies differing only in how each reaches its `Engine` (#1248).
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
    }
}

/// Editor mode → hardware caret shape (#1109, moved here from
/// the pre-#1434 TUI shell's `caret_shape_for_mode` by #1428 so `App` can share it):
/// block for Normal/Visual, bar for Insert, underline for a pending
/// replace-char (`r`) command — the same three-way mapping the old
/// hand-rolled crossterm cursor-style write used, now feeding
/// `Backend::set_caret_shape` (quadraui#1015) instead.
///
/// `sidebar_has_focus` is the caller's own "is a sidebar panel, not the
/// editor, holding keyboard focus" answer — the pre-#1434 TUI shell passes
/// `self.sidebar.has_focus`, `App` passes `engine.sidebar_has_focus()` (see
/// each caller). Pure function, deliberately separate from either caller's
/// `self.live`/no-op-by-default write gate right after it — see
/// `Backend::set_caret_shape`'s own doc for why the write itself is
/// unobservable from a `TestBackend`-driven test (a real terminal write,
/// no test-mode guard of its own) while this *decision* is.
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

/// Run the tick-time background chore list `App::handle_poll_tick` shares
/// with the pre-#1434 TUI shell's `tick` (#1248) — see the rung's header
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
    // `rw.visible_line_capacity`, not `rw.lines.len()` (#1779): the latter
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
    // widths this frame's `TabBars` rung actually painted (#1165).
    let counts = app.tab_visible_counts.borrow().clone();
    if !counts.is_empty() && engine.post_draw_apply_widths(&counts) {
        needs_redraw = true;
    }

    // Sync the OS window title before any chore below could plausibly change
    // the active buffer name (mirrors the legacy TUI ordering). Routed
    // through `Backend::window()` (quadraui#950, #1124) rather than the old
    // GTK-only `self.window`/`PlatformWindowHandle` title setter (deleted
    // #1234) — that seam was `None` on macOS/Win-GUI, so this used to be a
    // silent no-op there; `WindowControl` is backed on every windowed
    // backend.
    let win_title = window_title(engine);
    if let Some(w) = backend.window() {
        // #1634: only write when the title actually changed since the last
        // tick — see `App::last_window_title`'s own doc (including its "not
        // confirmed as the flicker cause" caveat). `window_title` is stable
        // at idle (derived only from the active buffer's name), so before
        // this guard every idle tick re-issued the identical `WindowControl
        // ::set_title` call for no reason; on TUI that is a real OSC 0/2
        // escape sequence written straight to `std::io::stdout()`
        // (`TuiBackend::set_title` bypasses the `ratatui::Terminal`'s
        // buffered `Write` entirely) — invisible to #1583's in-process
        // idle-stability test, which only inspects the `TestBackend`/vt100
        // sink `Terminal::draw` writes to. The decision itself is
        // `crate::app::dedup_window_title`, a free function so it's
        // directly unit-tested without a `Backend` — see that function's
        // own doc for why a `Backend`-call-count driver test isn't
        // achievable here.
        if crate::app::dedup_window_title(&mut app.last_window_title, &win_title) {
            let _ = w.set_title(&win_title);
        }
        // Refresh the session-restore maximized cache (#1529) unconditionally
        // — unlike size/position below, this is exactly the one moment those
        // freeze, so it must always be current.
        let maximized = w.is_maximized();
        if let Ok(m) = maximized {
            app.cached_window_maximized.set(m);
        }
        // Refresh the session-restore size/position cache (#1234, extended
        // #1529 for position) — see `App::cached_window_width`'s doc for why
        // this is cached here rather than read live from
        // `App::save_session_and_exit`, and why it's gated on
        // `!is_maximized()`.
        if matches!(maximized, Ok(false)) {
            if let Ok(bounds) = w.bounds() {
                app.cached_window_width.set(bounds.width.round() as i32);
                app.cached_window_height.set(bounds.height.round() as i32);
                // `WindowControl::bounds`'s own doc: GTK/Wayland always
                // reports `x: 0.0, y: 0.0` here, meaning "unknown", not "at
                // the screen origin" — skip caching that sentinel rather than
                // saving a fake position a `set_bounds`-capable backend
                // could later misapply; see `App::cached_window_x`/`y`'s own
                // doc for the #1529 review finding this closes.
                if bounds.x != 0.0 || bounds.y != 0.0 {
                    app.cached_window_x.set(Some(bounds.x.round() as i32));
                    app.cached_window_y.set(Some(bounds.y.round() as i32));
                }
            }
        }
    }

    // Poll the yank-highlight deadline armed by `run_post_key_epilogue` (#813).
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
    // and the active panel actually being one SC touches — resolved through
    // `sidebar_owner` rather than a bare panel-id string compare so an
    // extension panel shown over Git/Explorer doesn't spuriously re-trigger
    // this (mirrors the legacy GTK gate exactly; TUI's own prior gate missed
    // the ext-panel-priority case `sidebar_owner` already handles).
    //
    // The elapsed check only resets `last_sc_refresh` while the sidebar is
    // actually visible — matching the old `host.sidebar_refresh_due()`
    // hook's short-circuited-`&&` evaluation exactly: while the sidebar is
    // hidden, the 2s timer is never reset, so it fires once (and resets)
    // the first tick after the sidebar is shown again, no matter how long
    // it was hidden for.
    //
    // #1650: kicking off `sc_refresh_async` here must NOT by itself set
    // `needs_redraw` — spawning a background thread has no visible effect
    // of its own, and this block runs every 2s indefinitely while the
    // Explorer/Git sidebar is visible (the default startup state). This
    // used to force a real `ratatui::Terminal::draw` call every single
    // time regardless of whether `git status`/`log`/`worktree list`
    // actually changed, and every one of those draws writes an invisible
    // SGR-reset + cursor-hide escape burst (`ratatui-crossterm`'s
    // `CrosstermBackend::draw`/`hide_cursor` do this unconditionally, even
    // for a zero-cell diff) — exactly the non-silent idle byte stream
    // `tests/smoke-spec/tui.yaml`'s `idle-truly-silent` step caught. The
    // only thing that should trigger a redraw is `poll_sc_refresh`
    // actually finding changed data (see its own doc), once the async
    // fetch below completes.
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
    // `core/engine/` has no handle to (#1134).
    let actions = std::mem::take(&mut engine.pending_platform_actions);
    if !actions.is_empty() {
        for action in actions {
            run_pending_platform_action(engine, action, backend);
        }
        needs_redraw = true;
    }

    needs_redraw
}

// ─── Menu-bar app icon (#720) ───────────────────────────────────────────────

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

/// Width reserved at the leading edge of the GTK menu row for the app icon.
///
/// Deliberately derived from the **menu row's own height**, not from
/// `settings.font_size` / the editor line height — the icon is window chrome
/// and must not grow when the user bumps the editor font, the same lesson
/// #719 applied to the activity bar's width. A square slot (`width == row
/// height`) is what VS Code uses for the logo left of `File`.
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
/// Shared (#940 review) by [`split_menu_row_for_app_icon`] — which uses it to
/// carve the drawn-menu-row's icon/items split clear of the backend's own
/// controls — and `App::render_content`'s native-menu-row path, which insets
/// the *whole* row the same way when the drawn menu row itself is suppressed
/// (#901) and there is no icon/items split to compute. Extracted so those two
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

/// True when the backend already draws its own titlebar window controls in
/// this band — macOS's native traffic lights, reported via a non-default
/// [`quadraui::Backend::titlebar_control_inset`] once a capable backend has
/// opted into [`quadraui::shell::ShellConfig::client_side_titlebar`] (#940,
/// quadraui#947).
///
/// `control_inset` is [`quadraui::Rect::default()`] — all-zero — on every
/// backend before a window exists to query (GTK, Win-GUI, TUI forever; macOS
/// too until `MacBackend::set_window` runs), so this is `false` there and
/// every #940 code path that guards on it is a no-op, exactly as it behaved
/// before #940.
pub fn backend_draws_own_window_controls(control_inset: quadraui::Rect) -> bool {
    control_inset.width > 0.0 || control_inset.height > 0.0
}

/// True when vimcode's own drawn window controls (`render::window_controls_status_bar`)
/// should paint into the title-bar band this frame.
///
/// `menu_row_present` is `presence.menu_row` — vimcode never drew inline
/// controls without also drawing the menu row itself, on any backend, before
/// #940. `control_inset` adds the second condition #940 introduces: even
/// with the menu row present, a backend that reports it draws its own
/// controls (`backend_draws_own_window_controls`) must never *also* get
/// vimcode's — two sets of window controls is exactly the bug #940 exists to
/// prevent. See the module doc's "keeps the native traffic lights" section
/// for why vimcode does not just draw over/instead of them on a capable
/// backend.
pub fn should_draw_window_controls(menu_row_present: bool, control_inset: quadraui::Rect) -> bool {
    menu_row_present && !backend_draws_own_window_controls(control_inset)
}

/// Split the full menu row band into `(icon_rect, items_rect)`.
///
/// `items_rect` is the rect the menu *items* live in — it is what must be
/// handed to `MenuSystem::render` **and** `MenuSystem::handle`, and to any
/// `Backend::menu_bar_layout` measurement, so paint geometry and hit-test
/// geometry cannot drift. That drift is the actual regression risk of adding a
/// leading element (quadraui's `MenuBar::layout_with_leading` doc calls out the
/// same #552 `TabBar` bug class); vimcode goes through `MenuSystem`, which owns
/// its own `MenuBar::layout` call and exposes no leading-width parameter, so
/// this function is vimcode's equivalent single source of the offset.
///
/// `icon_rect` is a square inset vertically inside the slot by
/// [`APP_ICON_INSET_FRACTION`], horizontally centred in the reserved slot.
///
/// `leading_inset` (#940) shifts where the band effectively *starts*,
/// without moving its trailing edge — the region
/// [`quadraui::Backend::titlebar_control_inset`] reports as occupied by the
/// backend's own drawn controls (macOS's native traffic lights, floating
/// over the leading edge of the client-side titlebar; see
/// [`quadraui::shell::ShellConfig::client_side_titlebar`]'s doc for why a
/// non-empty inset there means "AppKit owns these pixels", not "the app must
/// draw here"). `0.0` on every backend before #940, and on GTK/Win-GUI
/// forever until they grow the same opt-in — `Backend::titlebar_control_inset`
/// defaults to `Rect::default()` there. Clamped to `menu_row_rect.width` so a
/// pathologically large inset cannot produce a negative-width row.
///
/// A zero/negative-height row (menu bar hidden) yields an empty icon rect and
/// an unchanged (inset-adjusted) items rect, so callers can split
/// unconditionally.
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
    // slot is a one-line invariant worth holding regardless (#720 review).
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

/// The app icon as a `quadraui::Image`, backed by the raw SVG.
///
/// `fallback_text` is empty on purpose: the icon is purely decorative chrome,
/// so a backend that cannot rasterise (TUI) or a decode failure (no SVG
/// gdk-pixbuf loader installed) should leave the slot blank rather than paint
/// a placeholder string where a logo belongs.
///
/// Painting code wants `crate::app`'s `app_icon_image_for_paint()`, which
/// currently just forwards here — kept as a separate name so the paint site
/// documents *why* it's safe to hand `Backend::draw_image` the raw SVG
/// directly rather than pre-rasterising it itself: quadraui#1014 added a
/// decode cache inside `Backend::draw_image` (GTK and macOS so far), so this
/// function no longer needs a per-backend wrapper to dodge re-decoding the
/// 1024×1024 SVG through librsvg on every frame (#1102).
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

    // ── Outer window resize grip (#1528) ─────────────────────────────────
    //
    // Black-box coverage note (#1528 review, blocking finding #1/#2): this
    // section is unit tests, not a `GtkDriver` black-box test, and that is a
    // deliberate, stated exemption — not an oversight or a substitute chosen
    // for convenience. `src/gtk/testing.rs`'s own module doc says "No
    // window": `App::new_headless` never captures a `gtk4::Window`, so the
    // pinned quadraui rev's `GtkBackend::begin_window_resize`,
    // `begin_window_drag`, and `set_cursor` all early-return `false`/no-op on
    // `self.window.is_none()` before touching anything a test could observe.
    // Worse, `begin_window_resize`/`begin_window_drag` also require a
    // `self.pending_window_press` captured from a *real* GDK press event
    // (device/button/x/y/time) — something only `gtk/run.rs`'s live event
    // controllers populate, never `GtkDriver::dispatch`'s synthesised
    // `UiEvent`s. So in headless mode the resize-press block in
    // `App::handle` never actually consumes the event either way (old
    // full-row-height grip or this PR's thin grip) — both silently no-op
    // and fall through — which means the specific real-world regression
    // #1528 describes (a live window's resize arming and swallowing the
    // press before the scrollbar/minimap/command-line ever sees it) cannot
    // be reproduced through the existing harness at all, in either
    // direction. This mirrors commit `4e2bd1e` ("wire GTK window
    // edge-resize + resize cursor"), which shipped no test for the same
    // reason ("Needs a real WM to smoke (Xvfb has no compositor)").
    //
    // What IS extracted and unit-tested below, because it doesn't need a
    // window at all: `WINDOW_RESIZE_GRIP_PX`'s value (a regression guard on
    // the constant, not the dispatch order) and
    // `resize_edge_overlaps_change_review_band` (the actual decision
    // function `App::handle` calls for blocking finding #3's fix, tested
    // directly against its own inputs/outputs — this one *is* production
    // logic, not just a constant, and the test fails if the logic
    // regresses).
    //
    // Manual smoke test (see this fix's `SMOKE_TESTS` in the commit/PR):
    // drag from each outer edge and corner of a live GTK window, with and
    // without a change-review surface open, and confirm the cursor and
    // resize both behave as the acceptance criteria describe.

    /// #1528: the grip must be thin — a handful of pixels, VS Code/Electron
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

    /// #1528: the grip must stay inside the vertical scrollbar's own gutter
    /// (`scroll_gutter_width`, `scrollbar_reserve.max(char_width)`) rather
    /// than reach past it into the minimap strip, which paints a further
    /// full gutter-width in from the pane's true right edge (see the doc
    /// comment above `render_engine`'s `minimap_widths`/`minimap`
    /// construction, #1094) — i.e. the minimap's rightmost column is always
    /// safely beyond the grip as long as the grip doesn't exceed the
    /// scrollbar's own column width. Checked across a realistic range of GTK
    /// char widths — from a small readable font up to a very large one —
    /// with `scrollbar_reserve` at its common `0.0` (overlay-scrollbar,
    /// #828/quadraui#776) so the gutter collapses to `char_width` alone, the
    /// tightest case.
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

    /// #1528 review (blocking finding #3): the change-review guard on the
    /// edge-resize press must cover only the edges that actually overlap the
    /// change-review surface's own first row — `North`/`NorthEast`/
    /// `NorthWest` — and must NOT cover `South`/`SouthEast`/`SouthWest`/
    /// `East`/`West`. A regression that widened the guard back to "every
    /// edge" (the bug this test pins) would silently make a user unable to
    /// resize from any edge at all while a change-review surface is open,
    /// which is exactly the scope-creep the review caught.
    ///
    /// RED against the reverted fix: if `resize_edge_overlaps_change_review_
    /// band` unconditionally returned `true` (i.e. `App::handle` gated ALL
    /// edges on `change_review_open`, as this PR did before the fix), every
    /// `assert!(!...)` below would fail. Verified by hand: with the function
    /// body swapped for a bare `true`, `cargo test --lib resize_edge_overlaps`
    /// fails at the `!resize_edge_overlaps_change_review_band(ResizeEdge::
    /// South)` assertion.
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

    /// #816 review: `core::engine::keys`'s digit-accumulation arm sets both
    /// `count = Some(10_000)` AND `message = "Count limited to 10,000"` in
    /// the same keypress once the cap is hit, so a live count and a
    /// non-empty message ARE simultaneously reachable in `Mode::Normal` —
    /// not a hypothetical the reviewer raised for nothing. In that state
    /// `build_command_line` paints the count, not the message (see its
    /// `Mode::Normal | Visual | VisualLine` arm), so a selection gate that
    /// only checked `!message.is_empty()` would arm `cmd_sel` against text
    /// nobody painted — the click hit-tests the count string while Ctrl+C's
    /// copy reads `engine.message`. This must stay disallowed until the
    /// count is consumed/cleared.
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
    // ── Menu-bar app icon geometry (#720) ────────────────────────────────
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

    /// The icon is chrome: its size must track the *menu row height* and
    /// nothing else, so bumping `settings.font_size` (which changes the
    /// editor line height, not this row) cannot grow it. Same lesson #719
    /// applied to the activity bar's width.
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

    /// #720 review: a row narrower than it is tall must not let the
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

    // ── Client-side titlebar leading-edge inset (#940) ──────────────────

    /// A non-zero `leading_inset` (what `Backend::titlebar_control_inset`
    /// reports on a capable backend, e.g. macOS's native traffic lights)
    /// must push both the icon and the items strip clear of it, without
    /// moving the band's trailing edge.
    ///
    /// RED against the pre-#940 signature (no `leading_inset` parameter at
    /// all — every caller started the icon flush with `menu_row_rect.x`
    /// regardless of a reported control inset, which is exactly the "app
    /// icon collides with the traffic lights" bug this issue exists to
    /// prevent). With the parameter threaded through but ignored (e.g. a
    /// stub `let _ = leading_inset;`), this assertion fails because `icon.x`
    /// would still equal `row.x` instead of `row.x + inset`.
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
    /// native-menu-row (`presence.menu_row == false`) path both need (#940
    /// review — the two were duplicating the same three lines). Pins the
    /// exact contract both callers depend on: leading edge moves in by
    /// `leading_inset`, trailing edge never moves, y/height untouched.
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

    /// `backend_draws_own_window_controls` is the pure predicate
    /// `App::render_content` reads `Backend::titlebar_control_inset()`
    /// through (#940 review — extracted so it has a driver-independent
    /// unit test; see the app.rs review note on why an actual `MacDriver`
    /// can't drive a non-default inset today). `Rect::default()` — the
    /// value every backend without the client-side-titlebar opt-in reports,
    /// forever on GTK/Win-GUI/TUI and on macOS before a window exists — must
    /// read as "no native controls"; any non-zero width *or* height must
    /// read as "yes", since either alone is what
    /// `MacBackend::titlebar_control_inset` can report depending on which
    /// dimension AppKit's own button-cluster container constrains.
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
    /// controls" decision #940 exists to get right — extracted so all four
    /// combinations of (`menu_row_present`, `has_native_controls`) have a
    /// direct, fast, deterministic test independent of any backend/driver
    /// (#940 review). Native controls always win: they must suppress
    /// vimcode's drawn controls regardless of `menu_row_present`, since a
    /// backend reporting a control inset is macOS today, which also always
    /// reports `menu_row_present == false` (#901) — but the function must
    /// not rely on that correlation to be correct.
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
    /// desktop icon theme are the same artwork, by construction (#716/#720
    /// "do not add a third copy of the SVG").
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
