use super::construction::{dialog_btn_index, PendingFileDialog};
use super::*;

impl App {
    /// Run a file dialog requested via [`PendingFileDialog`] (#572), using
    /// the runner-owned `backend`'s `PlatformServices` — `show_file_open_dialog`
    /// / `show_file_save_dialog` block (via quadraui's nested-mainloop pump,
    /// #427) until the user picks or cancels, then this returns synchronously.
    pub(crate) fn run_pending_file_dialog(
        &mut self,
        req: PendingFileDialog,
        backend: &mut dyn quadraui::Backend,
    ) {
        // Bodies shared with TUI's synchronous call sites — see
        // `render::run_open_file_dialog`/`run_save_workspace_as_dialog`'s
        // rung header comment (#1125) for why this stays one implementation
        // with two thin call sites instead of a per-backend copy.
        match req {
            PendingFileDialog::OpenFile => {
                let opened = {
                    let mut engine = self.engine.borrow_mut();
                    render::run_open_file_dialog(&mut engine, backend)
                };
                if opened.is_some() {
                    self.refresh_file_tree();
                }
            }
            PendingFileDialog::SaveWorkspaceAs => {
                let mut engine = self.engine.borrow_mut();
                render::run_save_workspace_as_dialog(&mut engine, backend);
            }
        }
        self.draw_needed.set(true);
    }

    /// Present the native message dialog queued by `render_content`'s
    /// edge-trigger check (#727), using the runner-owned `backend`'s
    /// `PlatformServices` — `show_message_dialog` blocks (via quadraui's
    /// nested-mainloop pump, #666, the same adapter #427's file dialogs
    /// use) until the user picks a button or dismisses it. Mirrors
    /// `run_pending_file_dialog` above.
    ///
    /// Maps the response back through the same `"dialog:btn:N"` id
    /// convention and `Engine::dialog_click_button` / `Engine::dialog_cancel`
    /// the in-canvas `DialogHit::Button(id)` mouse path
    /// (`handle_mouse_click_msg`) already uses — `None` (dismissed with no
    /// button chosen: Escape, close box) maps to `dialog_cancel()`, the
    /// same outcome the in-canvas dialog's Escape key produces — so both
    /// paths funnel through the identical `EngineAction` outcomes.
    pub(crate) fn run_pending_native_dialog(
        &mut self,
        opts: quadraui::MessageDialogOptions,
        backend: &mut dyn quadraui::Backend,
    ) {
        let choice = backend.services().show_message_dialog(opts);
        // Reset the edge-trigger flag *here*, before running the engine
        // callback below, rather than only lazily on the next
        // `render_content` call that observes `screen.dialog == None`. If
        // `dialog_click_button`/`dialog_cancel` ever opens a second dialog
        // synchronously (e.g. a chained "save failed, retry?" prompt), that
        // new dialog needs `native_dialog_shown == false` to be seen as a
        // fresh no-dialog-to-dialog edge and get queued for its own native
        // present — a stale `true` left over from the dialog that just
        // closed would otherwise suppress it silently. No such chain exists
        // in `process_dialog_result` today, but resetting eagerly here
        // costs nothing and removes the latent trap either way.
        self.native_dialog_shown.set(false);
        let action = match choice.as_ref().and_then(dialog_btn_index) {
            Some(idx) => self.engine.borrow_mut().dialog_click_button(idx),
            None => self.engine.borrow_mut().dialog_cancel(),
        };
        self.apply_dialog_action(action);
        self.draw_needed.set(true);
    }

    /// Apply the `EngineAction` produced by dismissing a dialog — clears
    /// `explorer_needs_refresh` (some dialog outcomes, e.g. "Discard &
    /// Close", can trigger a sidebar refresh) and handles quit/save-quit.
    /// Shared by the in-canvas mouse-click path
    /// (`handle_mouse_click_msg`'s dialog-button block) and the native
    /// message-dialog path (`run_pending_native_dialog`, #727) so both
    /// produce exactly the same outcome for a given `EngineAction`.
    pub(crate) fn apply_dialog_action(&mut self, action: EngineAction) {
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }
        match action {
            EngineAction::Quit | EngineAction::SaveQuit => {
                self.save_session_and_exit();
            }
            _ => {}
        }
    }
}

impl App {
    /// Minimize the application window (inline window-control button).
    ///
    /// Routed through `Backend::window()` (quadraui#950, #1124) rather than
    /// the old GTK-only `self.window`/`PlatformWindowHandle` seam (deleted
    /// #1234) — that seam was `None` on macOS/Win-GUI, so this used to be a
    /// silent no-op there; `WindowControl` is backed on every windowed
    /// backend.
    pub(crate) fn window_minimize(&mut self, backend: &mut dyn quadraui::Backend) {
        self.last_window_control_action
            .set(Some(render::WINDOW_MINIMIZE_ACTION));
        if let Some(w) = backend.window() {
            let _ = w.minimize();
        }
    }

    /// Maximize or restore the application window (inline window-control
    /// button).
    ///
    /// Goes through `Backend::toggle_window_maximize` (#813) rather than
    /// driving the OS window handle's maximize/unmaximize directly — the
    /// same toggle the CSD-titlebar double-click gesture already calls
    /// (`handle_dispatch`'s `UiEvent::DoubleClick` arm) — so there is only
    /// one place that operates the real OS window instead of two that have
    /// to agree.
    pub(crate) fn window_toggle_maximize(&mut self, backend: &mut dyn quadraui::Backend) {
        self.last_window_control_action
            .set(Some(render::WINDOW_MAXIMIZE_ACTION));
        backend.toggle_window_maximize();
    }

    /// Close the application window (inline window-control button).
    ///
    /// Routes through `show_quit_confirm` (#857) instead of driving the real
    /// OS window's `close()` directly: `close()` emits GTK's `close-request`
    /// signal *synchronously, on the same stack* — quadraui's handler for
    /// that signal re-enters `backend.borrow_mut()` while this dispatch path
    /// still holds it (see quadraui `run.rs:616`/`896`), which panics with
    /// `BorrowMutError` inside a signal trampoline that cannot unwind and so
    /// aborts the process instead of just panicking. `window_toggle_maximize`
    /// above already avoids the equivalent trap for maximize (#813) by
    /// routing through the engine instead of the OS window handle; this is
    /// the same fix applied to close. `show_quit_confirm` either raises the
    /// unsaved-changes dialog or sets `exit_requested`, which
    /// `ShellApp::handle` turns into `quadraui::Reaction::Exit` — the runner
    /// then tears the window down with `destroy()`, which does not re-enter
    /// `close-request`.
    pub(crate) fn window_close(&mut self) {
        self.last_window_control_action
            .set(Some(render::WINDOW_CLOSE_ACTION));
        self.show_quit_confirm();
    }

    /// User triggered quit; exit straight away when nothing is unsaved,
    /// otherwise raise the "unsaved changes" confirmation dialog.
    ///
    /// #823 item 4: the dialog body used to be restated here (byte-identical
    /// to `Engine::show_quit_confirm`, `core/engine/panels.rs`) instead of
    /// calling it — same `DialogButton` literals, just copy-pasted. TUI
    /// already calls the engine method directly (`tui_main/shell_app.rs`,
    /// `tui_main/mouse.rs`).
    pub(crate) fn show_quit_confirm(&mut self) {
        if !self.engine.borrow().has_any_unsaved() {
            self.save_session_and_exit();
            return;
        }
        self.engine.borrow_mut().show_quit_confirm();
        self.draw_needed.set(true);
    }

    /// Show a native "Open File" dialog.
    pub(crate) fn open_file_dialog(&mut self) {
        // Deferred to tick(), which has the runner-owned `backend`
        // handle PlatformServices needs — see PendingFileDialog (#572).
        self.pending_file_dialog
            .set(Some(PendingFileDialog::OpenFile));
        self.draw_needed.set(true);
    }

    /// Show the shared folder/workspace picker modal (#815).
    ///
    /// Before #815 this opened a *native* `gtk4::FileDialog` in
    /// "select folder" mode: at the time, `quadraui::PlatformServices` had no
    /// directory-select primitive (only `show_file_open_dialog` /
    /// `show_file_save_dialog`, both file pickers), so a native chooser was
    /// the only option. `quadraui::FolderPickerController` (shipped
    /// 2026-05-25, quadraui#166) made that escape hatch unnecessary — it does
    /// its own filesystem walk — so this opens the identical `Palette`-based
    /// picker TUI does; see `FrameOp::FolderPicker` and
    /// `handle_key_press`'s folder-picker rung. TUI's
    /// `new_folder_picker_controller` builds the same controller.
    ///
    /// `PlatformServices` has since gained `show_folder_open_dialog`
    /// (quadraui#935) — the premise above is now stale on its own, but the
    /// decision it led to isn't: this stays on `FolderPickerController`
    /// deliberately (#815; see issue #945), because it's the shape that
    /// behaves identically on every backend, whereas
    /// `show_folder_open_dialog` is a native-only primitive that TUI can't
    /// implement the same way GTK/macOS/Win-GUI would.
    pub(crate) fn open_folder_dialog(&mut self) {
        let engine = self.engine.borrow();
        let controller = quadraui::FolderPickerController::new(
            engine.cwd.clone(),
            vec![".vimcode-workspace".to_string()],
            engine.settings.show_hidden_files,
        );
        drop(engine);
        *self.folder_picker.borrow_mut() = Some(controller);
        self.draw_needed.set(true);
    }

    /// Drive an open folder picker with one raw `UiEvent` and apply the
    /// result. The decision (key→intent, filesystem walk, filtering,
    /// scroll) is entirely `quadraui::FolderPickerController`'s own (#815);
    /// this only applies the `Confirmed`/`Cancelled` outcomes to GTK-local
    /// state and the engine. Mirrors TUI's identical
    /// `apply_folder_picker_event` (`shell_app.rs`) — same controller, same
    /// outcome handling; the popup rect comes from the painted-rect cache
    /// (`folder_picker_popup_rect`, #582/#646) here instead of a fresh
    /// `Backend::viewport()` call, since this method has no live `backend`
    /// handle.
    pub(crate) fn apply_folder_picker_event(&mut self, event: &quadraui::UiEvent) {
        let Some(popup_rect) = self.folder_picker_popup_rect.get() else {
            return;
        };
        let lh = self.cached_line_height.max(1.0) as f32;
        let visible_rows = render::folder_picker_visible_rows(popup_rect, lh);
        let outcome = {
            let mut picker_ref = self.folder_picker.borrow_mut();
            let Some(picker) = picker_ref.as_mut() else {
                return;
            };
            picker.handle(event, visible_rows)
        };
        match outcome {
            quadraui::FolderPickerEvent::Confirmed { path } => {
                *self.folder_picker.borrow_mut() = None;
                self.engine.borrow_mut().open_folder(&path);
                self.refresh_file_tree();
            }
            quadraui::FolderPickerEvent::Cancelled => {
                *self.folder_picker.borrow_mut() = None;
            }
            quadraui::FolderPickerEvent::Consumed | quadraui::FolderPickerEvent::Ignored => {}
        }
    }

    /// Resolve a mouse press at `(x, y)` against the open folder picker's
    /// painted popup and apply the result. Mirrors TUI's identical block in
    /// `mouse::handle_mouse` — same shared `render::route_folder_picker_click`
    /// / `render::set_folder_picker_selected`, same "select row" / "consume"
    /// / "dismiss" outcomes.
    pub(crate) fn route_and_apply_folder_picker_click(&mut self, x: f64, y: f64) {
        let Some(rect) = self.folder_picker_popup_rect.get() else {
            // No painted rect to hit-test against (shouldn't happen while
            // `folder_picker` is open, since `render_content` always caches
            // one when it paints) — dismiss defensively rather than leave an
            // unreachable modal up.
            *self.folder_picker.borrow_mut() = None;
            self.draw_needed.set(true);
            return;
        };
        let lh = self.cached_line_height.max(1.0) as f32;
        let Some((scroll_top, total_filtered)) = self
            .folder_picker
            .borrow()
            .as_ref()
            .map(|p| (p.scroll_top(), p.filtered().len()))
        else {
            return;
        };
        let route = render::route_folder_picker_click(
            rect,
            x as f32,
            y as f32,
            lh,
            scroll_top,
            total_filtered,
        );
        match route {
            render::FolderPickerClickRoute::SelectRow(idx) => {
                if let Some(picker) = self.folder_picker.borrow_mut().as_mut() {
                    render::set_folder_picker_selected(picker, idx);
                    let visible_rows = render::folder_picker_visible_rows(rect, lh);
                    picker.sync_scroll(visible_rows);
                }
            }
            render::FolderPickerClickRoute::Consume => {}
            render::FolderPickerClickRoute::Dismiss => {
                *self.folder_picker.borrow_mut() = None;
            }
        }
        self.draw_needed.set(true);
    }

    /// #955 (ACP-4, shared with #525): the change-review surface's mouse
    /// handling, called from `handle_mouse_click_msg` — same shared
    /// `render::route_change_review_click` TUI's `mouse::handle_mouse`
    /// calls.
    pub(crate) fn route_and_apply_change_review_click(&mut self, x: f64, y: f64) {
        let diff_rect = self.engine.borrow().change_review_diff_rect.get();
        let view = self
            .engine
            .borrow()
            .change_review
            .as_ref()
            .and_then(|r| r.current_entry())
            .map(|e| e.view.clone());
        let Some(view) = view else {
            return;
        };
        let lh = self.cached_line_height.max(1.0) as f32;
        match render::route_change_review_click(diff_rect, &view, lh, x as f32, y as f32) {
            render::ChangeReviewClickRoute::Jump(hit) => {
                self.engine.borrow_mut().change_review_jump_to_hit(hit);
            }
            render::ChangeReviewClickRoute::Consume => {}
        }
        self.draw_needed.set(true);
    }

    /// Show a native "Save Workspace As" dialog.
    pub(crate) fn save_workspace_as_dialog(&mut self) {
        // Deferred to tick() — see PendingFileDialog (#572).
        self.pending_file_dialog
            .set(Some(PendingFileDialog::SaveWorkspaceAs));
        self.draw_needed.set(true);
    }

    /// Show the "Open Recent" workspace picker.
    pub(crate) fn open_recent_dialog(&mut self) {
        // #274: replaced the native gtk4::Dialog with the engine's
        // unified picker (PickerSource::RecentWorkspaces). Picker
        // confirm calls open_folder + sets explorer_needs_refresh
        // so the file tree rebuilds on the next render — no
        // backend-specific Msg dispatch needed here.
        let mut engine = self.engine.borrow_mut();
        if engine.session.recent_workspaces.is_empty() {
            engine.message = "No recent workspaces".to_string();
        } else {
            engine.open_picker(crate::core::engine::PickerSource::RecentWorkspaces);
        }
        drop(engine);
        self.draw_needed.set(true);
    }

    /// User clicked ✕ on a tab with unsaved changes — ask what to do.
    ///
    /// #823 item 4: was a byte-identical restatement of
    /// `Engine::show_close_tab_confirm` (`core/engine/panels.rs`) instead of
    /// calling it.
    pub(crate) fn show_close_tab_confirm(&mut self) {
        self.engine.borrow_mut().show_close_tab_confirm();
        self.draw_needed.set(true);
    }

    /// Editor content pixel width last painted by `render_content`
    /// (`cached_editor_bounds`) — the same width the bottom (terminal) panel
    /// spans, since `editor_bounds` is derived from `main_content_bounds`
    /// with the sidebar/activity bar already excluded (#582). Used by
    /// [`Self::terminal_panel_cols`] callers with no live pixel width of
    /// their own in scope (accelerator/menu/tick paths) — callers that DO
    /// have one (a click/drag handler's own `width` parameter) should pass
    /// that same-frame value to `terminal_panel_cols` directly instead, for
    /// the #1058 reason recorded on `terminal_panel_cols` itself.
    ///
    /// Falls back to the cached window width before the first frame has
    /// painted (`cached_editor_bounds` is still `None`).
    ///
    /// #1421: replaces the old hardcoded `terminal_cols() -> 80`.
    pub(crate) fn painted_editor_content_width(&self) -> f64 {
        self.cached_editor_bounds
            .get()
            .map(|(r, _)| r.width)
            .unwrap_or_else(|| self.cached_window_width.get() as f64)
    }

    /// Terminal panel pixel width reserved for the panel's own vertical
    /// scrollbar — the same strip `render_content` paints one into and
    /// `MouseDragRoute::TerminalSplitDivider` already clamps the divider
    /// drag against (`handle_mouse_drag_msg`).
    pub(crate) const TERMINAL_PANEL_SB_W: f64 = 6.0;

    /// Convert a *live* terminal-panel pixel width to a column count using
    /// the last-painted char advance (`cached_char_width`). #1058: this used
    /// to be a fallback of a hardcoded `terminal_cols() -> 80` (and, for the
    /// terminal-split finalize path specifically, a separate `da_w = 800.0`
    /// guess) rather than a real conversion of the live width, so any window
    /// that wasn't exactly 800px wide split the terminal into the wrong
    /// column counts. `cached_char_width` is seeded to a positive default in
    /// `App::new` and only ever grows from a real paint, so clamping it to a
    /// `1.0` floor (rather than branching on a `<= 0.0` fallback) is enough
    /// to avoid a divide-by-zero without a second hardcoded column count.
    pub(crate) fn terminal_panel_cols(&self, width: f64) -> u16 {
        let cw = self.cached_char_width.max(1.0);
        ((width - Self::TERMINAL_PANEL_SB_W).max(0.0) / cw) as u16
    }

    /// Terminal-maximize target row count for the next
    /// `terminal.toggle_maximize` dispatch — the exact
    /// `render::compute_editor_layout` call `render_content` makes every
    /// frame, replayed here against the content height/line height last
    /// painted (`cached_main_content_height`, `cached_line_height`) so
    /// accelerator/menu paths — which have no live pixel height of their own
    /// in scope — agree with what was actually painted. Same reasoning as
    /// [`Self::painted_editor_content_width`]'s fallback.
    ///
    /// #1421: replaces the old hardcoded `terminal_target_maximize_rows() ->
    /// 10`. Mirrors the TUI equivalent,
    /// `tui_main::terminal_target_maximize_rows_tui`.
    pub(crate) fn terminal_maximize_target_rows(&self, engine: &Engine) -> u16 {
        let h = self.cached_main_content_height.get();
        let lh = self.cached_line_height.max(1.0);
        render::compute_editor_layout(engine, h, lh, false).terminal_max_target_rows
    }
}
