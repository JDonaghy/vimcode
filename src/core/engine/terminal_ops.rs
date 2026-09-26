//! Integrated-terminal engine operations (panes, tabs, mouse, selection).
//!
//! ## Design note (#564): why terminal selection does *not* use
//! `quadraui::dispatch::TextRegion` / `DragTarget::TextSelection`
//!
//! #564 asked whether the integrated terminal's click-drag text selection
//! should be re-routed through quadraui's generic selectable-region
//! pipeline (`Backend::register_text_region` +
//! `dispatch::{dispatch_click, dispatch_mouse_drag}` +
//! `DragTarget::TextSelection` + `UiEvent::TextSelectionChanged` /
//! `TextCopied`, as demoed in `examples/common/selection_app.rs`). After
//! reviewing that pipeline against what's already here, the answer is no —
//! this pane already delegates the exact same responsibility to a
//! **better-fitted** quadraui abstraction:
//!
//! - Selection state is [`TermSelection`], a re-export of
//!   `quadraui::terminal_engine::TerminalSelection` — already a
//!   quadraui-owned type, not a bespoke vimcode one.
//! - The forward-vs-select gate (`mouse_reporting_enabled` /
//!   `should_forward_mouse` / `forward_mouse`) is entirely inside
//!   `quadraui::terminal_engine::TerminalSession` — this file only calls
//!   it, per the doc comments below.
//! - Selection highlight painting and text extraction
//!   (`selected_text()`/`build_rows()`) are done in quadraui's
//!   `terminal_engine`, using **display-row** coordinates that already
//!   account for scrollback (`scroll_offset`) — both backends reach them
//!   only through `Backend::draw_terminal`, so there is zero paint code
//!   here or in `src/gtk/` / `src/tui_main/` for this.
//!
//! The generic `TextRegion` mechanism models a *fixed-bounds, currently
//! painted* rectangle of `lines: Vec<String>` with anchor/focus expressed
//! as screen `Point`s — it has no notion of scrollback. Routing terminal
//! selection through it would mean translating `Point` anchor/focus back
//! into `TerminalSelection`'s row/col space on every drag update (the
//! `Point` round-trip `text_selection_line_range` produces would need to be
//! re-mapped through `scroll_offset` anyway), for no behavioral gain — it
//! would only add an indirection layer around a type that already fits.
//! Confirming this isn't a vimcode-only view: quadraui's own canonical
//! terminal reference (`examples/common/terminal_app.rs`, cited below as
//! the gold standard these methods mirror) does not use `TextRegion` for
//! terminal selection either — it only uses the generic dispatch pipeline
//! for its scrollbar drag, and leaves PTY forwarding / local selection to
//! `TerminalSession` directly, exactly as this file does.
//!
//! `TextRegion` *is* the right tool where a panel is a plain, non-scrolling
//! text surface unrelated to a PTY (see `selection_app.rs`); the doc
//! comment on [`Backend::cancel_text_selection_drag`] in quadraui even
//! covers the composability case of such a panel sitting *next to* an
//! embedded terminal. That's not this pane's situation — the terminal's
//! own screen is the selectable surface, and it already has a
//! purpose-built quadraui type for that. See also
//! `docs/QUADRAUI_GUIDE.md` § "Terminal selection stays on
//! `TerminalSelection`, not `TextRegion` (#564)".
//!
//! No quadraui-side gap exists here, so no quadraui issue was filed for
//! this half of #508's follow-up work. #565 (routing the *editor's*
//! visual-selection drag-origin arbitration through `DragTarget`) is a
//! different, still-open question — that one is about document-model
//! drag state across split windows, not terminal cell selection, and is
//! unaffected by this note.

use super::*;
use crate::core::terminal::TermSelection;

impl Engine {
    // ── Integrated Terminal ────────────────────────────────────────────────

    /// Get a reference to the active terminal session, if any.
    pub fn active_terminal(&self) -> Option<&TerminalSession> {
        self.terminal_panes
            .get(self.terminal_active)
            .map(|s| &s.session)
    }

    /// Get a mutable reference to the active terminal session, if any.
    pub fn active_terminal_mut(&mut self) -> Option<&mut TerminalSession> {
        self.terminal_panes
            .get_mut(self.terminal_active)
            .map(|s| &mut s.session)
    }

    /// Open the terminal panel. If no panes exist, create the first one.
    /// If panes already exist, just show/focus the panel.
    pub fn open_terminal(&mut self, cols: u16, rows: u16) {
        if self.terminal_panes.is_empty() {
            self.terminal_new_tab(cols, rows);
        } else {
            self.terminal_open = true;
            self.terminal_has_focus = true;
        }
    }

    /// Create a new terminal tab (always spawns a fresh shell in the editor's CWD).
    pub fn terminal_new_tab(&mut self, cols: u16, rows: u16) {
        self.terminal_new_tab_at(cols, rows, None);
    }

    /// Create a new terminal tab, optionally at a specific working directory.
    /// If `dir` is None, uses the editor's CWD.
    pub fn terminal_new_tab_at(&mut self, cols: u16, rows: u16, dir: Option<&Path>) {
        let shell = default_shell();
        let cwd = dir.unwrap_or(&self.cwd).to_path_buf();
        let history_cap = self.settings.terminal_scrollback_lines;
        match TerminalSession::spawn(cols, rows, &shell, &cwd, history_cap) {
            Ok(sess) => {
                self.terminal_panes.push(TerminalSlot {
                    session: sess,
                    install_ctx: None,
                    acp_auth_pending: false,
                    install_finalized: false,
                });
                self.terminal_active = self.terminal_panes.len() - 1;
                self.terminal_open = true;
                self.terminal_has_focus = true;
            }
            Err(e) => self.message = format!("terminal: failed to open PTY: {e}"),
        }
    }

    /// Run a command in a new terminal pane (visible to the user).
    /// Used for extension installs so the user can see progress, errors, and enter
    /// sudo passwords. `poll_terminal` finalizes the install (#1396) as soon as
    /// the wrapper records the command's exit code — it does not wait for the
    /// pane's shell to exit, so the "Installing…" spinner resolves even if the
    /// user never notices the pane or presses Enter at its "Press Enter to
    /// close…" prompt. The prompt itself stays, so the output remains readable;
    /// pressing Enter afterwards just closes the pane and is a no-op for the
    /// install (`TerminalSlot::install_finalized` guards against a second
    /// finalize on that later shell-exit path).
    ///
    /// Spawns an interactive shell via the quadraui `TerminalSession` primitive, then
    /// immediately injects the wrapped command into the PTY so the shell executes it.
    /// The wrapper still ends with `exit` / `Exit` so the shell process eventually
    /// exits after Enter, closing the pane and removing its `TerminalSlot`.
    pub fn terminal_run_command(&mut self, command: &str, cols: u16, rows: u16) {
        let cwd = self.cwd.clone();
        let history_cap = self.settings.terminal_scrollback_lines;
        // Extract install context set by ext_install_from_registry.
        let ctx = self.pending_install_context.take();
        let shell = default_shell();
        let is_powershell =
            shell.to_lowercase().contains("powershell") || shell.to_lowercase().contains("pwsh");
        // Build a wrapper script that runs the command, shows the exit status, waits
        // for Enter, then exits the shell so `TerminalSession::is_exited()` fires and
        // `poll_terminal` can call `finalize_install_from_terminal`. Keyed by the
        // install's `install_key` so the wrapper can hand the command's real exit
        // status back across the PTY boundary (#1344) — falls back to a shared
        // "adhoc" key for terminal runs with no install context, whose exit code
        // nothing reads.
        let install_key = ctx.as_ref().map_or("adhoc", |c| c.install_key.as_str());
        // #1396 review: `install_key` is deterministic per extension
        // (`format!("ext:{ext_name}:lsp")`), so a leftover scratch file from an
        // earlier attempt under the same key (pane closed in the narrow window
        // after the wrapper wrote it but before this run started, or a crash)
        // would otherwise be mistaken by `poll_terminal`'s very first idle tick
        // for *this* run's result — finalizing immediately with a stale exit
        // code and permanently discarding the real outcome once it lands.
        // Deleting any stale file before the new wrapper is even injected
        // guarantees `poll_terminal` never observes a code that didn't come
        // from this attempt.
        invalidate_install_exit_code(install_key);
        let wrapped = build_terminal_install_wrapper(command, is_powershell, install_key);
        match TerminalSession::spawn(cols, rows, &shell, &cwd, history_cap) {
            Ok(mut sess) => {
                // Inject the wrapped command immediately.  The PTY master writer is
                // ready as soon as `spawn` returns — the kernel PTY subsystem buffers
                // the bytes and the shell reads them from its stdin when it starts
                // processing input, so there is no race between this write and the
                // shell's readiness.
                sess.write_input(wrapped.as_bytes());
                self.terminal_panes.push(TerminalSlot {
                    session: sess,
                    install_ctx: ctx,
                    acp_auth_pending: false,
                    install_finalized: false,
                });
                self.terminal_active = self.terminal_panes.len() - 1;
                self.terminal_open = true;
                self.terminal_has_focus = true;
            }
            Err(e) => self.message = format!("terminal: failed to run command: {e}"),
        }
    }

    /// Launch the ACP agent's own command line, plus the chosen auth
    /// method's own `args` (#1444), as an *interactive* process in a
    /// visible terminal pane, for a `type: "terminal"` auth method (#957,
    /// ACP-6). Terminal auth is not the `authenticate` RPC — per the ACP
    /// spec's distinction (`core::acp`'s module doc), the client must run
    /// the adapter attached to a real TTY so its own CLI can drive an
    /// interactive login (a browser OAuth flow, a device code prompt, ...),
    /// then re-initialize once that process exits
    /// (`Engine::acp_finish_terminal_login`, called from `poll_terminal`
    /// below / `terminal_close_active_tab`). This re-runs the exact same
    /// command used for the NDJSON transport — resolved through
    /// `acp_resolve_agent_launch` (the registry's active profile if one is
    /// configured, else the legacy single-string `acp_agent_command`,
    /// #958 ACP-7 / #1443) so terminal-auth login always launches whatever
    /// agent the AI panel actually spawned, not a stale unconditional read
    /// of `acp_agent_command`.
    ///
    /// Running that resolved command *bare* is wrong: the reference Claude
    /// ACP adapter starts its NDJSON server and waits on stdin when given no
    /// arguments, even attached to a real TTY — no login flow ever runs, and
    /// the pane just sits there until the user closes it (read as
    /// "abandoned"). What actually selects *which* login this specific
    /// method performs is `login_args`, taken verbatim from the chosen
    /// [`crate::core::acp::AcpAuthMethod::args`] the agent advertised on the
    /// wire (e.g. `["--cli", "auth", "login", "--claudeai"]`) and appended
    /// after the resolved command — vimcode has no agent-specific knowledge
    /// of what those args mean (nor should it, per the ACP track's
    /// agent-neutral design); it only forwards what the agent itself said
    /// this method needs.
    ///
    /// `TerminalSession::spawn` takes a shell *path*, not an argv
    /// (`quadraui::terminal_engine::TerminalSession::spawn`) — the same
    /// constraint `terminal_run_command` above already works around: spawn
    /// the user's interactive shell (in the resolved profile's `cwd`),
    /// then inject the command (plus `login_args`) and its `env` as PTY
    /// input (`build_acp_auth_wrapper`), reusing that existing pattern
    /// rather than inventing a second one. `login_args` are shell-quoted
    /// (`quote_shell_arg`) before being appended — they came off the wire,
    /// not the operator's own keyboard, so unlike the base command string
    /// itself (`parse_agent_command`'s doc comment) they get no benefit of
    /// the doubt about containing shell metacharacters.
    pub fn acp_launch_terminal_login(&mut self, method_name: &str, login_args: &[String]) {
        // #1443: route through the same registry-aware resolver the
        // NDJSON transport path uses (`ai_send_message_via_acp`), not
        // `settings.acp_agent_command` directly — that field is empty
        // whenever the agent came from `settings.acp_agents`, which made
        // this path fail with "No ACP agent command configured" even
        // though the very auth-method dialog it is answering only exists
        // because a registry agent just spawned successfully.
        let (_argv, cwd, env, agent_cmd) = self.acp_resolve_agent_launch();
        let agent_cmd = agent_cmd.trim().to_string();
        if agent_cmd.is_empty() {
            self.message = "No ACP agent command configured".to_string();
            return;
        }
        let history_cap = self.settings.terminal_scrollback_lines;
        let shell = default_shell();
        let is_powershell =
            shell.to_lowercase().contains("powershell") || shell.to_lowercase().contains("pwsh");
        let agent_cmd = if login_args.is_empty() {
            agent_cmd
        } else {
            let quoted_args: Vec<String> = login_args
                .iter()
                .map(|a| quote_shell_arg(a, is_powershell))
                .collect();
            format!("{agent_cmd} {}", quoted_args.join(" "))
        };
        let wrapped = build_acp_auth_wrapper(&agent_cmd, &env, is_powershell);
        // Reuse an already-open pane's dimensions if one exists (the most
        // recently painted size); otherwise fall back to a conventional
        // default — no viewport is available from this call site (invoked
        // from `process_dialog_result`, not a backend's resize/layout
        // path), and the pane can still be resized later like any other.
        let (cols, rows) = self
            .terminal_panes
            .first()
            .map(|slot| (slot.session.cols(), slot.session.rows()))
            .unwrap_or((80, 24));
        match TerminalSession::spawn(cols, rows, &shell, &cwd, history_cap) {
            Ok(mut sess) => {
                sess.write_input(wrapped.as_bytes());
                self.terminal_panes.push(TerminalSlot {
                    session: sess,
                    install_ctx: None,
                    acp_auth_pending: true,
                    install_finalized: false,
                });
                self.terminal_active = self.terminal_panes.len() - 1;
                self.terminal_open = true;
                self.terminal_has_focus = true;
                self.message =
                    format!("Complete {method_name} sign-in in the terminal panel\u{2026}");
            }
            Err(e) => {
                self.message = format!("ACP terminal auth failed to start: {e}");
            }
        }
    }

    /// Close the active terminal tab. If it was the last tab, close the panel.
    /// Closing either pane while in split mode also exits split view.
    pub fn terminal_close_active_tab(&mut self) {
        if self.terminal_panes.is_empty() {
            return;
        }
        // #957 (ACP-6): closing the pane before an ACP terminal-auth login
        // process exits on its own is "abandoned", not "succeeded" —
        // `acp_finish_terminal_login(None)` must run *before* the pane is
        // removed below reports it, matching `poll_terminal`'s exit-path
        // handling of the same field.
        let was_acp_auth = self
            .terminal_panes
            .get(self.terminal_active)
            .is_some_and(|s| s.acp_auth_pending);
        // Exiting split mode before removing the pane keeps tab indices sane.
        self.terminal_split = false;
        self.terminal_panes.remove(self.terminal_active);
        if was_acp_auth {
            self.acp_finish_terminal_login(None);
        }
        if self.terminal_panes.is_empty() {
            self.terminal_open = false;
            self.terminal_has_focus = false;
            self.terminal_active = 0;
        } else {
            self.terminal_active = self.terminal_active.min(self.terminal_panes.len() - 1);
        }
    }

    /// Enable horizontal split view.
    /// Ensures at least two panes exist (creates a second if needed), resizes both to
    /// `half_cols`, then sets focus to the right pane (index 1).
    pub fn terminal_open_split(&mut self, half_cols: u16, rows: u16) {
        let history_cap = self.settings.terminal_scrollback_lines;
        if self.terminal_panes.is_empty() {
            // Create two fresh panes.
            let shell = default_shell();
            let cwd = self.cwd.clone();
            for _ in 0..2 {
                match TerminalSession::spawn(half_cols, rows, &shell, &cwd, history_cap) {
                    Ok(sess) => {
                        self.terminal_panes.push(TerminalSlot {
                            session: sess,
                            install_ctx: None,
                            acp_auth_pending: false,
                            install_finalized: false,
                        });
                    }
                    Err(e) => {
                        self.message = format!("terminal: failed to open PTY: {e}");
                        return;
                    }
                }
            }
            self.terminal_open = true;
            self.terminal_has_focus = true;
        } else if self.terminal_panes.len() == 1 {
            // Resize existing pane to half-width, then spawn a second.
            self.terminal_panes[0].session.resize(half_cols, rows);
            let shell = default_shell();
            let cwd = self.cwd.clone();
            match TerminalSession::spawn(half_cols, rows, &shell, &cwd, history_cap) {
                Ok(sess) => {
                    self.terminal_panes.push(TerminalSlot {
                        session: sess,
                        install_ctx: None,
                        acp_auth_pending: false,
                        install_finalized: false,
                    });
                }
                Err(e) => {
                    self.message = format!("terminal: failed to open PTY: {e}");
                    return;
                }
            }
        } else {
            // Two or more panes exist — resize the first two to half-width.
            self.terminal_panes[0].session.resize(half_cols, rows);
            self.terminal_panes[1].session.resize(half_cols, rows);
        }
        self.terminal_split = true;
        self.terminal_active = 1; // right pane gets focus
    }

    /// Disable horizontal split view and return to single-pane / tab view.
    /// Panes are kept alive as regular tabs; `full_cols` is used to resize the
    /// active pane back to the full panel width.
    pub fn terminal_close_split(&mut self, full_cols: u16, rows: u16) {
        self.terminal_split = false;
        self.terminal_split_left_cols = 0;
        // Resize whatever is now the active pane to full width.
        if let Some(slot) = self.terminal_panes.get_mut(self.terminal_active) {
            slot.session.resize(full_cols, rows);
        }
    }

    /// Toggle split mode on/off. `full_cols` = total panel width (each pane gets half).
    pub fn terminal_toggle_split(&mut self, full_cols: u16, rows: u16) {
        if self.terminal_split {
            self.terminal_close_split(full_cols, rows);
        } else {
            self.terminal_open_split(full_cols / 2, rows);
        }
    }

    /// Switch keyboard focus between the two split panes (left ↔ right).
    /// No-op when not in split mode.
    pub fn terminal_split_switch_focus(&mut self) {
        if self.terminal_split && self.terminal_panes.len() >= 2 {
            self.terminal_active = 1 - self.terminal_active;
        }
    }

    /// Update the visual divider position during a drag (no PTY resize yet).
    /// Backends call this on every drag event; finalize with `terminal_split_finalize_drag`.
    pub fn terminal_split_set_drag_cols(&mut self, left_cols: u16) {
        self.terminal_split_left_cols = left_cols;
    }

    /// Commit a drag resize: resize both PTY panes to the new sizes.
    /// Clears `terminal_split_left_cols` so PTY cols become authoritative again.
    pub fn terminal_split_finalize_drag(&mut self, left_cols: u16, right_cols: u16, rows: u16) {
        self.terminal_split_left_cols = 0;
        if self.terminal_panes.len() >= 2 {
            self.terminal_panes[0].session.resize(left_cols, rows);
            self.terminal_panes[1].session.resize(right_cols, rows);
        }
    }

    /// Switch to the terminal tab at the given index (clamped to valid range).
    pub fn terminal_switch_tab(&mut self, idx: usize) {
        if !self.terminal_panes.is_empty() {
            self.terminal_active = idx.min(self.terminal_panes.len() - 1);
        }
    }

    /// Hide the terminal panel but keep all PTY panes running.
    pub fn close_terminal(&mut self) {
        self.terminal_maximized = false;
        self.terminal_open = false;
        self.terminal_has_focus = false;
    }

    /// Resolve which zone of the bottom panel contains the click y-coordinate
    /// using the geometry cached at paint time. Returns `None` if the panel
    /// isn't currently painted or `y` is above the panel top. `y` is in the
    /// caller's unit (pixels for GTK, character rows for TUI) — must match
    /// what the backend wrote into [`BottomPanelGeometry`] at paint time.
    pub fn resolve_bottom_panel_zone(&self, y: f64) -> Option<BottomPanelZone> {
        let g = (*self.bottom_panel_geometry.borrow())?;
        if y < g.top_y || y >= g.top_y + g.height {
            return None;
        }
        let rel = y - g.top_y;
        let zone = if rel < g.toolbar_y {
            BottomPanelZone::TabBar
        } else if rel < g.content_y {
            BottomPanelZone::Toolbar
        } else if g.content_row_h > 0.0 {
            BottomPanelZone::Content {
                row_offset: ((rel - g.content_y) / g.content_row_h) as u16,
            }
        } else {
            BottomPanelZone::Content { row_offset: 0 }
        };
        Some(zone)
    }

    /// Handle a mouse-button press on a non-split terminal pane.
    ///
    /// **Forwarding first**: when the child process has enabled SGR mouse
    /// reporting (`mouse_reporting_enabled()`) the click is forwarded as an
    /// SGR-1006 `Press` byte sequence and the function returns `true`.  In
    /// that case no local selection is started — the inner app owns the
    /// pointer.
    ///
    /// **Local fallback**: when forwarding returns `false` (ordinary shell on
    /// the primary screen) the function focuses the terminal, resets any
    /// scrollback offset, and starts a zero-length selection at `(col, row)`
    /// (0-based cells within the pane).
    ///
    /// Backends call this for every left- (or right-) click in the content
    /// area, passing the cell coordinates they already had to translate from
    /// their native pixel / cell space.  The forwarding policy lives entirely
    /// here — no backend-specific branching needed.
    ///
    /// Returns `true` when the event was forwarded to the child process.
    ///
    /// # Gold standard
    /// Mirrors `examples/common/terminal_app.rs` `UiEvent::MouseDown` arm
    /// (quadraui#279/#365) — zero backend-specific code.
    pub fn handle_terminal_pane_press(
        &mut self,
        col: u16,
        row: u16,
        button: quadraui::MouseButton,
        mods: quadraui::Modifiers,
    ) -> bool {
        use quadraui::terminal_engine::TerminalMouseKind;
        self.terminal_has_focus = true;
        self.terminal_scroll_reset();
        // Try to forward to the child first.  `forward_mouse` checks
        // `should_forward_mouse(Press)` which is gated on
        // `mouse_reporting_enabled()` only (not alt-screen — clicks are
        // only forwarded when the child explicitly asked for them).
        let forwarded = if let Some(term) = self.active_terminal_mut() {
            term.forward_mouse(TerminalMouseKind::Press, button, col, row, mods)
        } else {
            false
        };
        if !forwarded {
            // Local selection start.
            if let Some(term) = self.active_terminal_mut() {
                term.selection = Some(TermSelection {
                    start_row: row,
                    start_col: col,
                    end_row: row,
                    end_col: col,
                });
            }
        }
        forwarded
    }

    /// Backward-compat wrapper: press with left button and no modifiers.
    ///
    /// Callers that don't have button/modifier info (e.g. split-click
    /// helpers) use this.  New code should prefer
    /// [`handle_terminal_pane_press`](Self::handle_terminal_pane_press).
    #[allow(dead_code)]
    pub fn handle_terminal_pane_click(&mut self, col: u16, row: u16) {
        self.handle_terminal_pane_press(
            col,
            row,
            quadraui::MouseButton::Left,
            quadraui::Modifiers::default(),
        );
    }

    /// Update the active pane's selection endpoint during a mouse drag.
    ///
    /// **Forwarding first**: when the child has mouse reporting enabled the
    /// drag is forwarded as a `Move` (button-held) event.  The inner app
    /// sees the live pointer position and can act on it (e.g. select text
    /// inside a nested vim).
    ///
    /// **Local fallback**: when forwarding returns `false` the endpoint of
    /// the in-progress selection is extended to `(col, row)`.
    ///
    /// Both TUI and GTK call this with their pane-relative cell coordinates
    /// — no backend-specific branching.
    ///
    /// # Gold standard
    /// Mirrors the `UiEvent::MouseMoved { buttons: left, .. }` arm of
    /// `examples/common/terminal_app.rs`.
    pub fn handle_terminal_pane_drag(&mut self, col: u16, row: u16) {
        use quadraui::terminal_engine::TerminalMouseKind;
        let forwarded = if let Some(term) = self.active_terminal_mut() {
            term.forward_mouse(
                TerminalMouseKind::Move,
                quadraui::MouseButton::Left,
                col,
                row,
                quadraui::Modifiers::default(),
            )
        } else {
            false
        };
        if !forwarded {
            if let Some(term) = self.active_terminal_mut() {
                if let Some(ref mut sel) = term.selection {
                    sel.end_row = row;
                    sel.end_col = col;
                }
            }
        }
    }

    /// Handle a mouse-button release over the terminal content area.
    ///
    /// Forwards the release to the child when it has mouse reporting enabled
    /// (matches `UiEvent::MouseUp` in `terminal_app.rs`), then auto-copies
    /// any live selection to the clipboard via the engine's `clipboard_write`
    /// callback.
    ///
    /// Returns `true` when text was copied to the clipboard.
    ///
    /// Currently both backends call [`Self::terminal_autocopy_selection`]
    /// directly from their general mouse-release handler rather than routing
    /// through this method (doing so would incorrectly forward releases that
    /// originate outside the terminal panel to the child process).  This
    /// method is infrastructure for a future terminal-specific release handler
    /// that can safely pass the coordinates and button through.
    #[allow(dead_code)]
    pub fn handle_terminal_pane_release(
        &mut self,
        col: u16,
        row: u16,
        button: quadraui::MouseButton,
    ) -> bool {
        use quadraui::terminal_engine::TerminalMouseKind;
        // Forward release to child when it owns the pointer.
        if let Some(term) = self.active_terminal_mut() {
            term.forward_mouse(
                TerminalMouseKind::Release,
                button,
                col,
                row,
                quadraui::Modifiers::default(),
            );
        }
        // Auto-copy selection to clipboard.
        self.terminal_autocopy_selection()
    }

    /// Copy the active pane's current text selection to the clipboard via
    /// the engine's `clipboard_write` callback.
    ///
    /// Called from [`handle_terminal_pane_release`] and by each backend on
    /// mouse-up when the terminal has focus.  Returns `true` when text was
    /// copied.
    pub fn terminal_autocopy_selection(&mut self) -> bool {
        if !self.terminal_has_focus {
            return false;
        }
        let text = self.active_terminal().and_then(|t| t.selected_text());
        if let Some(ref text) = text {
            if let Some(ref cb) = self.clipboard_write {
                let _ = cb(text);
                return true;
            }
        }
        false
    }

    /// Handle a click on the terminal content area using a
    /// `TerminalSplitHit` from the cached layout. Sets pane focus,
    /// starts selection or forwards press, or signals a divider drag.
    /// Returns `true` if the caller should start a split-divider drag.
    ///
    /// `button` and `mods` are forwarded to
    /// [`handle_terminal_pane_press`](Self::handle_terminal_pane_press)
    /// for the pane-hit branches.
    pub fn handle_terminal_split_click(
        &mut self,
        hit: quadraui::TerminalSplitHit,
        button: quadraui::MouseButton,
        mods: quadraui::Modifiers,
    ) -> bool {
        use quadraui::TerminalSplitHit;
        self.terminal_has_focus = true;
        match hit {
            TerminalSplitHit::Divider => true,
            TerminalSplitHit::LeftPane { col, row } => {
                self.terminal_active = 0;
                self.handle_terminal_pane_press(col, row, button, mods);
                false
            }
            TerminalSplitHit::RightPane { col, row } => {
                self.terminal_active = 1;
                self.handle_terminal_pane_press(col, row, button, mods);
                false
            }
            TerminalSplitHit::Scrollbar | TerminalSplitHit::Outside => false,
        }
    }

    /// Dispatch a click on the bottom panel tab bar using the cached
    /// `TabBarHits` from the last paint. Returns `true` if the click
    /// was consumed (tab switch or panel close).
    pub fn handle_bottom_tab_bar_click(&mut self, click_x: f64) -> bool {
        enum Action {
            Close,
            Switch(BottomPanelKind),
            None,
        }
        let action = {
            let hits = self.bottom_tab_bar_hits.borrow();
            let Some(ref hits) = *hits else {
                return false;
            };
            if hits
                .right_segment_bounds
                .first()
                .is_some_and(|&(sx, ex)| click_x >= sx && click_x < ex)
            {
                Action::Close
            } else {
                let mut kinds = Vec::new();
                if self.terminal_open {
                    kinds.push(BottomPanelKind::Terminal);
                }
                if !self.dap_output_lines.is_empty() {
                    kinds.push(BottomPanelKind::DebugOutput);
                }
                hits.slot_positions
                    .iter()
                    .enumerate()
                    .find(|(_, &(sx, ex))| click_x >= sx && click_x < ex)
                    .and_then(|(idx, _)| kinds.get(idx).cloned())
                    .map_or(Action::None, Action::Switch)
            }
        };
        match action {
            Action::Close => {
                self.bottom_panel_open = false;
                self.close_terminal();
                true
            }
            Action::Switch(kind) => {
                self.bottom_panel_kind = kind;
                true
            }
            Action::None => false,
        }
    }

    /// Resolve a terminal toolbar click to an action using cached hit data.
    /// Both TUI (cell columns) and GTK (pixel positions) pass screen-absolute
    /// coordinates; the method accounts for coordinate-system differences
    /// between `StatusBarLayout` (bar-relative) and `TabBarHits` (absolute).
    pub fn resolve_terminal_toolbar_click(&self, click_x: f64) -> TerminalToolbarAction {
        let hits = self.terminal_toolbar_hits.borrow();
        let Some(ref hits) = *hits else {
            return TerminalToolbarAction::None;
        };
        match hits {
            TerminalToolbarHits::FindBar { layout, origin_x } => {
                let rel_x = click_x - origin_x;
                match layout.hit_test(rel_x as f32, 0.0) {
                    quadraui::StatusBarHit::Segment(id)
                        if id.as_str() == "term_toolbar:find_close" =>
                    {
                        TerminalToolbarAction::CloseFindBar
                    }
                    _ => TerminalToolbarAction::None,
                }
            }
            TerminalToolbarHits::TabStrip(hits) => {
                for (i, &(sx, ex)) in hits.right_segment_bounds.iter().enumerate() {
                    if click_x >= sx && click_x < ex {
                        return match i {
                            0 => TerminalToolbarAction::AddTab,
                            1 => TerminalToolbarAction::ToggleSplit,
                            2 => TerminalToolbarAction::ToggleMaximize,
                            3 => TerminalToolbarAction::CloseTab,
                            _ => TerminalToolbarAction::None,
                        };
                    }
                }
                for (idx, &(sx, ex)) in hits.slot_positions.iter().enumerate() {
                    if click_x >= sx && click_x < ex && sx < ex {
                        return TerminalToolbarAction::SwitchTab(idx);
                    }
                }
                TerminalToolbarAction::StartResize
            }
        }
    }

    /// Execute a terminal toolbar action. Returns `false` for `StartResize`
    /// (backend-local drag state) and `None`; returns `true` for all other
    /// actions handled internally.
    pub fn execute_terminal_toolbar_action(
        &mut self,
        action: TerminalToolbarAction,
        ctx: UiEventContext,
    ) -> bool {
        match action {
            TerminalToolbarAction::SwitchTab(idx) => self.terminal_switch_tab(idx),
            TerminalToolbarAction::CloseTab => self.terminal_close_active_tab(),
            TerminalToolbarAction::ToggleMaximize => {
                self.toggle_terminal_maximize();
                let effective = self.effective_terminal_panel_rows(ctx.terminal_max_rows);
                if self.terminal_panes.is_empty() {
                    self.terminal_new_tab(ctx.terminal_cols, effective);
                } else {
                    self.terminal_resize(ctx.terminal_cols, effective);
                }
            }
            TerminalToolbarAction::ToggleSplit => {
                let rows = self.session.terminal_panel_rows;
                self.terminal_toggle_split(ctx.terminal_cols, rows);
            }
            TerminalToolbarAction::AddTab => {
                let rows = self.session.terminal_panel_rows;
                self.terminal_new_tab(ctx.terminal_cols, rows);
            }
            TerminalToolbarAction::CloseFindBar => {
                self.terminal_find_active = false;
            }
            TerminalToolbarAction::StartResize | TerminalToolbarAction::None => return false,
        }
        true
    }

    /// Toggle "terminal maximized" state.
    ///
    /// This only flips `terminal_maximized`; the stored user-preferred panel
    /// height (`session.terminal_panel_rows`) is left untouched. Each
    /// backend's layout code is responsible for asking
    /// [`Engine::effective_terminal_panel_rows`] on every frame, so window
    /// resizes automatically re-derive the maximized panel size without any
    /// re-trigger from the keybinding / click handlers.
    ///
    /// Opens the terminal panel if it's not already visible, and grabs focus
    /// on maximize.
    pub fn toggle_terminal_maximize(&mut self) {
        if self.terminal_maximized {
            self.terminal_maximized = false;
        } else {
            self.terminal_open = true;
            self.terminal_has_focus = true;
            self.terminal_maximized = true;
        }
    }

    /// Return the effective content-row count for the terminal panel: either
    /// the maximized target (backend-computed `max_target_rows`) when the
    /// maximize flag is set, or the user-preferred `session.terminal_panel_rows`.
    ///
    /// Backends call this **every frame** during layout, after they've
    /// computed how many rows the panel could take given current window
    /// dimensions. That's what makes window-resize handling automatic.
    pub fn effective_terminal_panel_rows(&self, max_target_rows: u16) -> u16 {
        if self.terminal_maximized {
            max_target_rows.max(self.session.terminal_panel_rows).max(5)
        } else {
            self.session.terminal_panel_rows
        }
    }

    /// Toggle the integrated terminal:
    /// - If open and focused → close (hide)
    /// - If open but unfocused → give focus
    /// - If not open → signal UI to open (UI calls terminal_new_tab with correct dimensions)
    ///
    /// Also closes the debug output bottom panel if it is the only thing keeping
    /// the bottom panel visible (no terminal running).
    pub fn toggle_terminal(&mut self) {
        if self.terminal_open && self.terminal_has_focus {
            self.close_terminal();
            // Also close debug output panel if no terminal remains
            if self.bottom_panel_open && !self.terminal_open {
                self.bottom_panel_open = false;
            }
        } else if self.terminal_open {
            self.terminal_has_focus = true;
        } else if self.bottom_panel_open {
            // No terminal but debug output panel is open — close it
            self.bottom_panel_open = false;
        } else {
            // Signal UI to call terminal_new_tab with correct dimensions
            self.terminal_open = true;
            self.terminal_has_focus = true;
        }
    }

    /// Drain PTY output from all sessions and update VT100 screens.
    /// Returns true if a redraw is needed.
    /// Exited sessions are automatically removed; closes the panel when the last one exits.
    ///
    /// #1396: install panes finalize as soon as the command's exit-code scratch
    /// file appears (below), not only when the shell itself exits — the wrapper
    /// (`build_terminal_install_wrapper`) blocks on "Press Enter to close…"
    /// *after* the command finishes and *after* it has written that file, so
    /// waiting for `is_exited()` alone left the "Installing…" spinner stuck
    /// until the user noticed the pane and pressed Enter. The pane itself still
    /// stays open with its prompt so the user can read the output; `install_ctx`
    /// is only taken (and the slot removed) once the shell actually exits.
    pub fn poll_terminal(&mut self) -> bool {
        let mut got_data = false;
        for slot in &mut self.terminal_panes {
            got_data |= slot.session.poll();
        }
        // Finalize any install pane whose command has already recorded its exit
        // status, ahead of the shell-exit loop below. Collected into a separate
        // Vec first because `finalize_install_from_terminal` takes `&mut self`
        // and can't run while `terminal_panes` is borrowed by the scan. Skips
        // the scan (and its allocation) entirely when there are no unfinalized
        // install panes at all — the common case for plain terminal tabs.
        if self
            .terminal_panes
            .iter()
            .any(|slot| slot.install_ctx.is_some() && !slot.install_finalized)
        {
            let mut newly_finalized: Vec<(usize, InstallContext)> = Vec::new();
            for (i, slot) in self.terminal_panes.iter().enumerate() {
                if slot.install_finalized {
                    continue;
                }
                if let Some(ctx) = &slot.install_ctx {
                    if install_exit_code_ready(&ctx.install_key) {
                        newly_finalized.push((i, ctx.clone()));
                    }
                }
            }
            for (i, ctx) in newly_finalized {
                if let Some(slot) = self.terminal_panes.get_mut(i) {
                    slot.install_finalized = true;
                }
                self.finalize_install_from_terminal(&ctx);
            }
        }
        // Remove exited sessions in reverse order (preserves earlier indices during removal).
        // For install panes that weren't already finalized above (e.g. the pane was
        // closed before the command wrote its exit code), finalize before removing.
        let mut i = self.terminal_panes.len();
        while i > 0 {
            i -= 1;
            if self.terminal_panes[i].session.is_exited() {
                let ctx = self.terminal_panes[i].install_ctx.take();
                let already_finalized = self.terminal_panes[i].install_finalized;
                let was_acp_auth = self.terminal_panes[i].acp_auth_pending;
                let exit_code = self.terminal_panes[i].session.exit_code();
                if let Some(ctx) = ctx {
                    if !already_finalized {
                        self.finalize_install_from_terminal(&ctx);
                    }
                }
                self.terminal_panes.remove(i);
                // #957 (ACP-6): call after `remove` so `acp_finish_terminal_login`
                // (which may itself touch `terminal_panes` indirectly via
                // `acp_launch_terminal_login` on a later retry) never sees
                // the just-exited pane still present.
                if was_acp_auth {
                    self.acp_finish_terminal_login(exit_code);
                }
                if self.terminal_active > i {
                    self.terminal_active = self.terminal_active.saturating_sub(1);
                }
            }
        }
        if self.terminal_panes.is_empty() {
            self.terminal_open = false;
            self.terminal_has_focus = false;
            self.terminal_active = 0;
            self.terminal_split = false;
        } else {
            self.terminal_active = self.terminal_active.min(self.terminal_panes.len() - 1);
            // If a pane exited while in split and we're down to one, exit split.
            if self.terminal_split && self.terminal_panes.len() < 2 {
                self.terminal_split = false;
            }
        }
        // Keep find matches fresh if new terminal output arrived while find is active.
        if got_data && self.terminal_find_active {
            self.terminal_find_update_matches();
        }
        got_data
    }

    /// Called by `poll_terminal`, either as soon as the install command's exit
    /// code is available (#1396 — the common case, well before the pane's
    /// shell itself exits) or, as a fallback, when an unfinalized install pane's
    /// shell exits (e.g. the pane was closed before the command finished).
    /// Checks the install command's recorded exit status (#1344) and, if it
    /// succeeded, whether the binary is now resolvable via the shared
    /// `binary_on_path` lookup, registering the LSP/DAP server if so.
    fn finalize_install_from_terminal(&mut self, ctx: &InstallContext) {
        self.lsp_installing.remove(&ctx.install_key);
        // Clear the "Installing …" spinner notification.
        self.notify_done_by_kind(&NotificationKind::LspInstall, None);

        let ext_name = &ctx.ext_name;

        // #1344: the wrapper script records the install command's real exit
        // status to a scratch file (see `build_terminal_install_wrapper`)
        // because a PTY only tells us the *shell* exited, not what the command
        // it ran returned. A non-zero code is reported as a failure up front —
        // previously this was invisible and surfaced only as a confusing
        // "binary not found on PATH", which pointed at the wrong problem.
        if let Some(code) = read_install_exit_code(&ctx.install_key) {
            crate::core::lsp_manager::install_log(&format!(
                "[ext-install] '{ext_name}' install (key={}) exited with code {code}",
                ctx.install_key
            ));
            if code != 0 {
                self.message = format!(
                    "Install for '{ext_name}' failed (exit {code}) — see the terminal output"
                );
                return;
            }
        }

        let manifest = self
            .ext_available_manifests()
            .into_iter()
            .find(|m| m.name == *ext_name);
        let manifest = match manifest {
            Some(m) => m,
            None => return,
        };

        // Collected rather than assigned straight into `self.message` (review
        // finding on #1344): a manifest can declare both an LSP server and a
        // DAP adapter (`sample_manifests()` in `src/core/extensions.rs` has a
        // `rust` entry with both `lsp.binary` and `dap.adapter`/`dap.binary`
        // set), and a successful LSP install followed by an unresolved DAP
        // binary must not silently erase the LSP success message — a user
        // who installed `rust-analyzer` but not `codelldb` should see both
        // outcomes, not a lone "was not found" that reads like the whole
        // install failed.
        let mut outcomes: Vec<String> = Vec::new();

        // Check if LSP binary is now resolvable and register it.
        if !manifest.lsp.binary.is_empty() {
            let all_lsp: Vec<&str> = std::iter::once(manifest.lsp.binary.as_str())
                .chain(manifest.lsp.fallback_binaries.iter().map(|s| s.as_str()))
                .filter(|b| !b.is_empty())
                .collect();
            if let Some(bin) = all_lsp.iter().copied().find(|b| binary_on_path(b)) {
                self.ensure_lsp_manager();
                for lsp_lang in &manifest.language_ids {
                    let config = lsp::LspServerConfig {
                        command: bin.to_string(),
                        args: manifest.lsp.args.clone(),
                        languages: vec![lsp_lang.clone()],
                        ..Default::default()
                    };
                    if let Some(mgr) = &mut self.lsp_manager {
                        mgr.add_registry_entry(config);
                        mgr.ensure_server_for_language(lsp_lang);
                    }
                    self.lsp_reopen_buffers_for_language(lsp_lang);
                }
                outcomes.push(format!(
                    "LSP server for '{ext_name}' installed and started ({bin})"
                ));
            } else {
                outcomes.push(format!(
                    "Install for '{ext_name}' finished but LSP binary '{}' was not found — looked in {}",
                    manifest.lsp.binary,
                    crate::core::lsp_manager::probed_tool_dirs_description(&manifest.lsp.binary)
                ));
            }
        }

        // Check if DAP binary is now resolvable.
        if !manifest.dap.adapter.is_empty() && !manifest.dap.binary.is_empty() {
            if binary_on_path(&manifest.dap.binary) {
                outcomes.push(format!(
                    "DAP adapter for '{ext_name}' installed — press F5 to debug"
                ));
            } else {
                outcomes.push(format!(
                    "Install for '{ext_name}' finished but DAP binary '{}' was not found — looked in {}",
                    manifest.dap.binary,
                    crate::core::lsp_manager::probed_tool_dirs_description(&manifest.dap.binary)
                ));
            }
        }

        if !outcomes.is_empty() {
            self.message = outcomes.join(" | ");
        }
    }

    /// Send raw bytes to the active pane's PTY stdin.
    pub fn terminal_write(&mut self, data: &[u8]) {
        if let Some(term) = self.active_terminal_mut() {
            term.write_input(data);
        }
    }

    /// Paste `text` into the active pane's PTY, then poll it so the echo
    /// lands in the frame the caller is about to paint.
    ///
    /// Delegates the bracketed-paste decision to quadraui's
    /// `TerminalSession::paste` (quadraui#343/#415), which wraps in
    /// `ESC[200~ … ESC[201~` only when the child has actually enabled DEC
    /// private mode 2004. Both call sites used to hand-roll an
    /// *unconditional* wrap, which leaked literal `[200~` bytes into programs
    /// that do not strip them (`cat`, `less`, a shell without a line editor).
    pub fn terminal_paste(&mut self, text: &str) {
        if let Some(term) = self.active_terminal_mut() {
            term.paste(text);
        }
        self.poll_terminal();
    }

    /// Resize all terminal panes (shared panel height).
    pub fn terminal_resize(&mut self, cols: u16, rows: u16) {
        for slot in &mut self.terminal_panes {
            slot.session.resize(cols, rows);
        }
    }

    /// Return selected terminal text from the active pane for clipboard copy.
    ///
    /// #732: GTK's only caller was the `Msg::TerminalCopySelection` arm, which
    /// had no producer left after the #540 Relm4→ShellApp cutover (the
    /// per-DrawingArea key controller that used to send it went with it), and
    /// TUI has never called it — so the `vimcode` bin, which compiles `core` as
    /// a private module, now reports it dead. Kept (rather than deleted with
    /// the orphaned arm) because it is part of `vimcode_core`'s public surface
    /// and is what a re-wired terminal-copy binding on either backend will
    /// call; `#[allow]` documents that it is currently unreached, not unwanted.
    #[allow(dead_code)]
    pub fn terminal_copy_selection(&mut self) -> Option<String> {
        self.active_terminal()?.selected_text()
    }

    /// Scroll the active pane's scrollback view up (away from live output).
    pub fn terminal_scroll_up(&mut self, rows: usize) {
        if let Some(term) = self.active_terminal_mut() {
            term.scroll_up(rows);
        }
    }

    /// Scroll the active pane's scrollback view down (toward live output).
    pub fn terminal_scroll_down(&mut self, rows: usize) {
        if let Some(term) = self.active_terminal_mut() {
            term.scroll_down(rows);
        }
    }

    /// Route a scroll-wheel notch from a raw `UiEvent::Scroll` delta.
    ///
    /// Both the TUI and GTK backends emit `UiEvent::Scroll { delta, .. }`
    /// through `quadraui::dispatch_scroll`, where the canonical sign is:
    ///
    /// - `delta_y < 0` → scroll **up** into history
    /// - `delta_y > 0` → scroll **down** toward the live view
    ///
    /// This matches the convention in `examples/common/terminal_app.rs` and
    /// the GTK `EventControllerScroll` / TUI `crossterm::ScrollUp` (which
    /// vimcode maps to `delta_y = -1.0`).
    ///
    /// A step of `ceil(|delta_y| × 3)` rows mirrors the example app (3 rows
    /// per notch).  The forward-vs-scroll policy is delegated to
    /// [`terminal_wheel`](Self::terminal_wheel) which calls
    /// `TerminalSession::forward_mouse` / `scroll_up` / `scroll_down`.
    ///
    /// Backends call this in **one line** from their
    /// `"terminal_scrollback"` dispatch arm — no per-backend step
    /// computation, sign reversal, or forwarding logic needed.  When
    /// quadraui ships `TerminalSession::handle_wheel` (quadraui#365) this
    /// method will thin further to a single delegation.
    pub fn handle_terminal_scroll(&mut self, delta_y: f32) {
        if delta_y == 0.0 {
            return;
        }
        let step = (delta_y.abs() * 3.0).ceil() as usize;
        let up = delta_y < 0.0;
        self.terminal_wheel(up, step);
    }

    /// Route a mouse-wheel notch for the active terminal pane: forward it to
    /// the child when the child owns the wheel (alt-screen / mouse reporting),
    /// otherwise scroll local scrollback by `step` rows.
    ///
    /// Called by [`handle_terminal_scroll`](Self::handle_terminal_scroll)
    /// which backends should prefer.  Direct callers pass a pre-computed
    /// step count — use this when you already have `up`/`step` (e.g. tests).
    ///
    /// Mirrors quadraui's `examples/common/terminal_app.rs` scroll handler,
    /// which composes `forward_mouse()` + `scroll_up/down` the same way.
    /// The longer-term goal (quadraui#365) is to lift this into
    /// `TerminalSession::handle_wheel`.
    pub fn terminal_wheel(&mut self, up: bool, step: usize) {
        if !self.terminal_forward_wheel(up) {
            if up {
                self.terminal_scroll_up(step);
            } else {
                self.terminal_scroll_down(step);
            }
        }
    }

    /// Forward a mouse-wheel notch to the active pane's child process when it
    /// owns the alternate screen or has enabled mouse reporting (vim, less,
    /// tmux, claude, …). Returns `true` when the wheel was written to the
    /// child — in that case the caller MUST NOT scroll local scrollback.
    /// Returns `false` for an ordinary shell (primary screen, no mouse
    /// reporting), where the caller falls back to
    /// [`terminal_scroll_up`](Self::terminal_scroll_up) /
    /// [`terminal_scroll_down`](Self::terminal_scroll_down).
    ///
    /// quadraui gates this through `TerminalSession::should_forward_wheel()`
    /// (#514 stress-test: a stray wheel must never leak the previous command's
    /// output into the shell's scrollback while an app owns the alt-screen).
    ///
    /// Wheel events report at cell `(0, 0)`.  The pointer position for wheels
    /// is not yet passed through — this is tracked as part of the full
    /// `UiEvent` unification (quadraui#365).
    pub fn terminal_forward_wheel(&mut self, up: bool) -> bool {
        use quadraui::terminal_engine::TerminalMouseKind;
        if let Some(term) = self.active_terminal_mut() {
            let kind = if up {
                TerminalMouseKind::WheelUp
            } else {
                TerminalMouseKind::WheelDown
            };
            term.forward_mouse(
                kind,
                quadraui::MouseButton::Left,
                0,
                0,
                quadraui::Modifiers::default(),
            )
        } else {
            false
        }
    }

    /// Return the active pane to the live view (cancel any scrollback offset).
    pub fn terminal_scroll_reset(&mut self) {
        if let Some(term) = self.active_terminal_mut() {
            term.scroll_reset();
        }
    }

    // ── Terminal inline find bar ───────────────────────────────────────────

    /// Open the terminal find bar and reset the query.
    pub fn terminal_find_open(&mut self) {
        self.terminal_find_active = true;
        self.terminal_find_query.clear();
        self.terminal_find_selected = 0;
        self.terminal_find_matches.clear();
    }

    /// Close the terminal find bar and clear all match state.
    pub fn terminal_find_close(&mut self) {
        self.terminal_find_active = false;
        self.terminal_find_query.clear();
        self.terminal_find_selected = 0;
        self.terminal_find_matches.clear();
    }

    /// Append a character to the find query and refresh matches.
    pub fn terminal_find_char(&mut self, ch: char) {
        self.terminal_find_query.push(ch);
        self.terminal_find_selected = 0;
        self.terminal_find_update_matches();
    }

    /// Delete the last character from the find query and refresh matches.
    pub fn terminal_find_backspace(&mut self) {
        self.terminal_find_query.pop();
        self.terminal_find_selected = 0;
        self.terminal_find_update_matches();
    }

    /// Advance to the next match (wraps around) and scroll to it.
    pub fn terminal_find_next(&mut self) {
        let n = self.terminal_find_matches.len();
        if n > 0 {
            self.terminal_find_selected = (self.terminal_find_selected + 1) % n;
            let (req_offset, _, _) = self.terminal_find_matches[self.terminal_find_selected];
            if let Some(slot) = self.terminal_panes.get_mut(self.terminal_active) {
                slot.session.set_scroll_offset(req_offset);
            }
        }
    }

    /// Go back to the previous match (wraps around) and scroll to it.
    pub fn terminal_find_prev(&mut self) {
        let n = self.terminal_find_matches.len();
        if n > 0 {
            self.terminal_find_selected = (self.terminal_find_selected + n - 1) % n;
            let (req_offset, _, _) = self.terminal_find_matches[self.terminal_find_selected];
            if let Some(slot) = self.terminal_panes.get_mut(self.terminal_active) {
                slot.session.set_scroll_offset(req_offset);
            }
        }
    }

    /// Scan the entire history buffer and the live screen, rebuilding
    /// `terminal_find_matches`.  Case-insensitive.
    ///
    /// Matches are `(required_scroll_offset, row, col)` where:
    /// - History match at history row H: required_offset = `history_len - H`, row = 0.
    ///   Formula: visible_row = row + current_offset − required_offset.
    /// - Live match at screen row R:     required_offset = 0, row = R.
    ///
    /// Sorted oldest-first (highest required_offset first, then top-to-bottom).
    ///
    /// Uses the quadraui `TerminalSession` public API (`scrollback_text()` /
    /// `screen_text()`) to avoid touching private `history` / `parser` fields.
    fn terminal_find_update_matches(&mut self) {
        self.terminal_find_matches.clear();
        if !self.terminal_find_active || self.terminal_find_query.is_empty() {
            return;
        }
        let q_lower: Vec<char> = self.terminal_find_query.to_lowercase().chars().collect();
        let qlen = q_lower.len();
        let active_idx = self.terminal_active;
        let sess = match self.terminal_panes.get(active_idx) {
            Some(slot) => &slot.session,
            None => return,
        };

        let mut matches: Vec<(usize, u16, u16)> = Vec::new();

        // ── History rows via scrollback_text() ──────────────────────────────
        // hist_len is the total ring-buffer size (including any trailing blank rows
        // that scrollback_text() drops). required_offset uses hist_len so that
        // scroll navigation stays correct for all non-blank rows.
        let hist_len = sess.history_len();
        let scrollback = sess.scrollback_text();
        if !scrollback.is_empty() {
            for (hist_idx, hist_line) in scrollback.split('\n').enumerate() {
                let required_offset = hist_len - hist_idx;
                let row_lower: Vec<char> = hist_line
                    .chars()
                    .map(|ch| ch.to_lowercase().next().unwrap_or(ch))
                    .collect();
                if qlen <= row_lower.len() {
                    for c in 0..=(row_lower.len() - qlen) {
                        if row_lower[c..c + qlen] == q_lower[..] {
                            matches.push((required_offset, 0, c as u16));
                        }
                    }
                }
            }
        }

        // ── Live screen rows via screen_text() ──────────────────────────────
        let screen_str = sess.screen_text();
        if !screen_str.is_empty() {
            for (r, line) in screen_str.split('\n').enumerate() {
                let row_lower: Vec<char> = line
                    .chars()
                    .map(|ch| ch.to_lowercase().next().unwrap_or(ch))
                    .collect();
                if qlen <= row_lower.len() {
                    for c in 0..=(row_lower.len() - qlen) {
                        if row_lower[c..c + qlen] == q_lower[..] {
                            matches.push((0, r as u16, c as u16));
                        }
                    }
                }
            }
        }

        // Sort: oldest first (highest required_offset), then top-to-bottom.
        matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        self.terminal_find_matches = matches;
        if !self.terminal_find_matches.is_empty() {
            self.terminal_find_selected = self
                .terminal_find_selected
                .min(self.terminal_find_matches.len() - 1);
        } else {
            self.terminal_find_selected = 0;
        }
    }

    /// Shared terminal key dispatch (#351). The engine decides what a
    /// keypress means; the backend only needs to execute the returned
    /// action (clipboard I/O, PTY write). `key_name` uses the same
    /// canonical names as `handle_key` (e.g. "Return", "Escape", "Up").
    pub fn handle_terminal_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> TerminalKeyAction {
        // Alt+1–9: switch terminal tab.
        if alt && !ctrl && !shift {
            if let Some(ch) = unicode {
                if ch.is_ascii_digit() && ch != '0' {
                    self.terminal_switch_tab((ch as u8 - b'1') as usize);
                    return TerminalKeyAction::Handled;
                }
            }
        }

        // PageUp / PageDown: scroll scrollback.
        if !ctrl && !alt && !shift {
            if key_name == "Page_Up" || key_name == "Prior" {
                self.terminal_scroll_up(12);
                return TerminalKeyAction::Handled;
            }
            if key_name == "Page_Down" || key_name == "Next" {
                self.terminal_scroll_down(12);
                return TerminalKeyAction::Handled;
            }
        }

        // Ctrl+Y or Ctrl+Shift+C: copy selection.
        if ctrl && !alt {
            if let Some(ch) = unicode {
                if (ch == 'y' || ch == 'Y') && !shift {
                    return TerminalKeyAction::CopySelection;
                }
                if (ch == 'c' || ch == 'C') && shift {
                    return TerminalKeyAction::CopySelection;
                }
            }
        }

        // Ctrl+V / Ctrl+Shift+V: paste clipboard.
        if ctrl && !alt {
            if let Some(ch) = unicode {
                if ch == 'v' || ch == 'V' {
                    return TerminalKeyAction::PasteClipboard;
                }
            }
        }

        // Ctrl+F: toggle terminal find bar.
        if ctrl && !shift && !alt {
            if let Some(ch) = unicode {
                if ch == 'f' || ch == 'F' {
                    if self.terminal_find_active {
                        self.terminal_find_close();
                    } else {
                        self.terminal_find_open();
                    }
                    return TerminalKeyAction::Handled;
                }
            }
        }

        // Find bar active: intercept all keys for search navigation.
        if self.terminal_find_active {
            match key_name {
                "Escape" => self.terminal_find_close(),
                "Return" if shift => self.terminal_find_prev(),
                "Return" => self.terminal_find_next(),
                "BackSpace" => self.terminal_find_backspace(),
                _ => {
                    if !ctrl && !alt {
                        if let Some(ch) = unicode {
                            self.terminal_find_char(ch);
                        }
                    }
                }
            }
            return TerminalKeyAction::Handled;
        }

        // Ctrl+W in split mode: switch focus between panes.
        if ctrl && !shift && !alt && self.terminal_split {
            if let Some(ch) = unicode {
                if ch == 'w' || ch == 'W' {
                    self.terminal_split_switch_focus();
                    return TerminalKeyAction::Handled;
                }
            }
        }

        // Any other key: reset scrollback and forward to PTY.
        self.terminal_scroll_reset();
        let data = key_to_pty_bytes(key_name, unicode, ctrl);
        if data.is_empty() {
            TerminalKeyAction::Ignore
        } else {
            TerminalKeyAction::SendToPty(data)
        }
    }
}

/// Path to the per-install exit-code scratch file for `install_key` (#1344).
///
/// A PTY only tells `TerminalSession::is_exited()` that the *shell* exited —
/// not what exit status the command it ran returned. `build_terminal_install_wrapper`
/// writes that status here so `finalize_install_from_terminal` can read it back
/// once the pane closes. Keyed by `install_key` (e.g. `"ext:bicep:lsp"`) so two
/// installs — say an LSP and a DAP install for the same extension, run
/// back-to-back — never collide on the same scratch file. The key is sanitized
/// because it contains `:`, which is illegal in Windows filenames.
///
/// The sanitizer maps every non `[A-Za-z0-9._-]` character to `_`, so two
/// distinct keys that differ only in such characters (e.g. `"ext:bicep:lsp"`
/// vs. `"ext_bicep_lsp"`) could in theory collide on the same file. This is
/// assumed safe because `install_key` is always machine-constructed —
/// `format!("ext:{ext_name}:lsp")` / `format!("dap:{adapter}")` — never
/// user-free-text, so a colliding pair would require two *different* code
/// paths to independently choose the exact same literal key, which none do
/// today.
pub(crate) fn install_exit_code_path(install_key: &str) -> PathBuf {
    let safe: String = install_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::env::temp_dir().join(format!("vimcode-install-exit-{safe}.status"))
}

/// Read back the exit code `build_terminal_install_wrapper` recorded for
/// `install_key`, consuming (deleting) the scratch file so a stale code left
/// over from an earlier install under the same key is never mistaken for the
/// current one (#1344).
///
/// Returns `None` when the wrapper never got as far as writing the file —
/// e.g. the pane was closed before the command finished, or the run had no
/// install context (`terminal_run_command`'s "adhoc" key) in the first place.
/// Callers must treat `None` as "unknown", not "failed".
fn read_install_exit_code(install_key: &str) -> Option<i32> {
    let path = install_exit_code_path(install_key);
    let contents = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    contents.trim().parse::<i32>().ok()
}

/// Non-consuming check for whether the wrapper has written the exit-code
/// scratch file for `install_key` yet (#1396). `poll_terminal` uses this on
/// every idle tick to finalize an install as soon as the *command* finishes,
/// rather than waiting for the shell to exit (which requires the user to
/// notice the pane and press Enter at its "Press Enter to close…" prompt).
/// Deliberately just an existence check, not a read — the actual value is
/// consumed exactly once, by `read_install_exit_code` inside
/// `finalize_install_from_terminal`, so a peek here can't race the real read.
fn install_exit_code_ready(install_key: &str) -> bool {
    install_exit_code_path(install_key).exists()
}

/// Delete any leftover exit-code scratch file for `install_key` (#1396 review
/// finding). `terminal_run_command` calls this right before injecting the
/// wrapped command into the new pane's PTY, so a file left behind by an
/// earlier attempt under the same (deterministic, per-extension) `install_key`
/// can never be mistaken by `poll_terminal`'s eager `install_exit_code_ready`
/// check for the current run's result. Without this, a leftover from a pane
/// closed between the wrapper writing the file and the shell actually exiting
/// (or a crash in that window) would finalize the *new* install on its very
/// first idle tick with the *old* exit code, and — because finalization is
/// idempotent per pane — permanently discard the real outcome once it lands.
/// Best-effort: if the file doesn't exist, or can't be removed, there is
/// nothing stale to worry about (a fresh write by this run's wrapper will
/// simply create/overwrite it later).
fn invalidate_install_exit_code(install_key: &str) {
    let _ = std::fs::remove_file(install_exit_code_path(install_key));
}

/// Build the PTY-injected wrapper script for `terminal_run_command`.
///
/// Wraps `command` in a shell fragment that:
/// 1. Runs the command.
/// 2. Records its exit status to the `install_exit_code_path(install_key)` scratch
///    file (#1344) so `finalize_install_from_terminal` can tell a failed install
///    apart from a successful one whose binary just isn't resolvable.
/// 3. Prints a colour-coded success/failure banner.
/// 4. Prints "Press Enter to close…" and waits for the user.
/// 5. **Exits the shell** after "Press Enter to close…" — since #1396,
///    `poll_terminal` finalizes as soon as the exit-code scratch file appears
///    (step 2), *without* waiting for this exit, so the LSP/DAP registration no
///    longer depends on it. This final `exit` / `Exit` still matters for a
///    second, narrower reason: it's what makes `TerminalSession::is_exited()`
///    eventually fire so the pane closes and its `TerminalSlot` is removed
///    once the user is done reading the output and presses Enter — without
///    it, the shell would return to its PS1 prompt and the pane would stay
///    open (and unremovable) forever.
///
/// Extracted as a pure function so both the exit-suffix invariant and the
/// exit-code handoff can be tested without spawning a real PTY.
pub fn build_terminal_install_wrapper(
    command: &str,
    is_powershell: bool,
    install_key: &str,
) -> String {
    let exit_code_path = install_exit_code_path(install_key);
    if is_powershell {
        format!(
            concat!(
                "{cmd}; ",
                "$__ec = $LASTEXITCODE; ",
                "if ($null -eq $__ec) {{ $__ec = 0 }}; ",
                "Set-Content -Path '{path}' -Value $__ec -NoNewline; ",
                "Write-Host ''; ",
                "if ($__ec -eq 0) {{ ",
                "Write-Host \"`e[32m✓ Command completed successfully`e[0m\" ",
                "}} else {{ ",
                "Write-Host \"`e[31m✗ Command failed (exit code $__ec)`e[0m\" ",
                "}}; ",
                "Write-Host ''; ",
                "Write-Host 'Press Enter to close…'; ",
                "Read-Host; Exit\n"
            ),
            cmd = command,
            path = exit_code_path.display(),
        )
    } else {
        format!(
            "{cmd}\n__exit_code=$?\necho \"$__exit_code\" > \"{path}\" 2>/dev/null\necho ''\nif [ $__exit_code -eq 0 ]; then echo '\\033[32m✓ Command completed successfully\\033[0m'; else echo \"\\033[31m✗ Command failed (exit code $__exit_code)\\033[0m\"; fi\necho ''\necho 'Press Enter to close…'\nread __dummy\nexit\n",
            cmd = command,
            path = exit_code_path.display(),
        )
    }
}

/// Build the PTY-injected wrapper for an ACP terminal-auth login pane
/// (#957, ACP-6). Unlike [`build_terminal_install_wrapper`], there is no
/// scratch-file exit-code handoff and no "press Enter to close" pause —
/// `exit $?` / `Exit $LASTEXITCODE` runs immediately after the command,
/// with nothing in between to disturb `$?`/`$LASTEXITCODE`, so the outer
/// interactive shell's own exit status *is* the login command's exit
/// status directly. The pane is expected to close itself the moment the
/// login command finishes: `poll_terminal`'s exit handling reads
/// `TerminalSession::exit_code()` straight off the session it just
/// detected exited, no scratch file needed.
///
/// Extracted as a pure function, same rationale as
/// [`build_terminal_install_wrapper`]: testable without a real PTY.
///
/// `env` carries the active agent profile's extra environment variables
/// (`AcpAgentProfile::env` / `acp_resolve_agent_launch`, #958 ACP-7,
/// #1443). Unlike the NDJSON transport path (`AcpClient::spawn_with_env`,
/// a real subprocess `env` map), these vars are injected as shell
/// statements ahead of the command since the whole wrapper is text typed
/// into an interactive shell, not argv — `export KEY=VALUE` for POSIX,
/// `$env:KEY = "VALUE"` for PowerShell. Values are not shell-quoted
/// beyond that: same "operator-configured, not attacker input" posture
/// `parse_agent_command`'s doc comment already accepts for the command
/// string itself.
pub fn build_acp_auth_wrapper(
    command: &str,
    env: &[(String, String)],
    is_powershell: bool,
) -> String {
    if is_powershell {
        let env_lines: String = env
            .iter()
            .map(|(k, v)| format!("$env:{k} = \"{v}\"\n"))
            .collect();
        format!("{env_lines}{command}\nExit $LASTEXITCODE\n")
    } else {
        let env_lines: String = env
            .iter()
            .map(|(k, v)| format!("export {k}=\"{v}\"\n"))
            .collect();
        format!("{env_lines}{command}\nexit $?\n")
    }
}

/// Shell-quote a single argv word for injection into the interactive shell
/// wrapper `build_acp_auth_wrapper` builds — used for a `type: "terminal"`
/// auth method's own `args` (#1444, [`crate::core::acp::AcpAuthMethod::
/// args`]), which arrive off the wire rather than from the operator's own
/// keyboard, unlike the base command string (`parse_agent_command`'s doc
/// comment explains that string's own, more permissive, posture). POSIX
/// gets single-quoting with the standard `'\''`-splice for an embedded
/// single quote; PowerShell gets double-quoting with backtick-escaped
/// backticks and double quotes, matching `build_acp_auth_wrapper`'s own
/// per-shell split.
fn quote_shell_arg(arg: &str, is_powershell: bool) -> String {
    if is_powershell {
        format!("\"{}\"", arg.replace('`', "``").replace('"', "`\""))
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// Translate a key event to PTY input bytes. Shared by both backends (#351).
pub fn key_to_pty_bytes(key_name: &str, unicode: Option<char>, ctrl: bool) -> Vec<u8> {
    if ctrl {
        if let Some(ch) = unicode {
            let b = ch as u8;
            if b.is_ascii() {
                return vec![b & 0x1f];
            }
        }
        if key_name.len() == 1 {
            let b = key_name.as_bytes()[0].to_ascii_lowercase();
            if b.is_ascii_lowercase() {
                return vec![b & 0x1f];
            }
        }
        return match key_name {
            "Return" | "KP_Enter" => b"\r".to_vec(),
            "BackSpace" => b"\x7f".to_vec(),
            "Tab" => b"\t".to_vec(),
            _ => vec![],
        };
    }

    match key_name {
        "Return" | "KP_Enter" => b"\r".to_vec(),
        "BackSpace" => b"\x7f".to_vec(),
        "Tab" | "ISO_Left_Tab" => b"\t".to_vec(),
        "Escape" => b"\x1b".to_vec(),
        "Up" | "KP_Up" => b"\x1b[A".to_vec(),
        "Down" | "KP_Down" => b"\x1b[B".to_vec(),
        "Right" | "KP_Right" => b"\x1b[C".to_vec(),
        "Left" | "KP_Left" => b"\x1b[D".to_vec(),
        "Home" | "KP_Home" => b"\x1b[H".to_vec(),
        "End" | "KP_End" => b"\x1b[F".to_vec(),
        "Delete" | "KP_Delete" => b"\x1b[3~".to_vec(),
        "Insert" | "KP_Insert" => b"\x1b[2~".to_vec(),
        "Page_Up" | "KP_Page_Up" | "Prior" => b"\x1b[5~".to_vec(),
        "Page_Down" | "KP_Page_Down" | "Next" => b"\x1b[6~".to_vec(),
        "F1" => b"\x1bOP".to_vec(),
        "F2" => b"\x1bOQ".to_vec(),
        "F3" => b"\x1bOR".to_vec(),
        "F4" => b"\x1bOS".to_vec(),
        "F5" => b"\x1b[15~".to_vec(),
        "F6" => b"\x1b[17~".to_vec(),
        "F7" => b"\x1b[18~".to_vec(),
        "F8" => b"\x1b[19~".to_vec(),
        "F9" => b"\x1b[20~".to_vec(),
        "F10" => b"\x1b[21~".to_vec(),
        "F11" => b"\x1b[23~".to_vec(),
        "F12" => b"\x1b[24~".to_vec(),
        _ => {
            if let Some(ch) = unicode {
                ch.to_string().into_bytes()
            } else {
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_acp_auth_wrapper, build_terminal_install_wrapper, install_exit_code_path,
        install_exit_code_ready, invalidate_install_exit_code, read_install_exit_code,
    };
    use std::path::PathBuf;

    /// Verify that the POSIX wrapper ends with `\nexit\n` so the shell process
    /// exits after the user presses Enter, enabling `poll_terminal` to call
    /// `finalize_install_from_terminal` and register the LSP/DAP server.
    #[test]
    fn posix_wrapper_ends_with_exit() {
        let script = build_terminal_install_wrapper("pip install foo", false, "test:posix-exit");
        assert!(
            script.contains("read __dummy\nexit\n"),
            "POSIX wrapper must end with `read __dummy\\nexit\\n` so the shell exits; got:\n{script}"
        );
    }

    /// Verify that the PowerShell wrapper ends with `Read-Host; Exit\n` for the
    /// same reason.
    #[test]
    fn powershell_wrapper_ends_with_exit() {
        let script = build_terminal_install_wrapper("pip install foo", true, "test:ps-exit");
        assert!(
            script.contains("Read-Host; Exit\n"),
            "PowerShell wrapper must end with `Read-Host; Exit\\n` so the shell exits; got:\n{script}"
        );
    }

    /// The command appears verbatim at the start of both wrapper flavours.
    #[test]
    fn wrapper_contains_command() {
        let cmd = "cargo install my-tool";
        let posix = build_terminal_install_wrapper(cmd, false, "test:contains-cmd-sh");
        let ps = build_terminal_install_wrapper(cmd, true, "test:contains-cmd-ps");
        assert!(
            posix.starts_with(cmd),
            "POSIX wrapper must start with the command"
        );
        assert!(
            ps.starts_with(cmd),
            "PowerShell wrapper must start with the command"
        );
    }

    /// #957 (ACP-6): unlike the install wrapper, the ACP auth-login wrapper
    /// must end with `exit $?` / `Exit $LASTEXITCODE` *immediately* after
    /// the command — no scratch-file write, no "press Enter" pause — so
    /// the outer shell's own exit status is the login command's exit
    /// status directly.
    #[test]
    fn acp_auth_wrapper_posix_ends_with_bare_exit_of_command_status() {
        let script = build_acp_auth_wrapper("sh login.sh", &[], false);
        assert_eq!(
            script, "sh login.sh\nexit $?\n",
            "POSIX ACP auth wrapper must be exactly the command followed by \
             `exit $?`, nothing else; got:\n{script}"
        );
    }

    #[test]
    fn acp_auth_wrapper_powershell_ends_with_bare_exit_of_last_exit_code() {
        let script = build_acp_auth_wrapper("sh login.sh", &[], true);
        assert_eq!(
            script, "sh login.sh\nExit $LASTEXITCODE\n",
            "PowerShell ACP auth wrapper must be exactly the command \
             followed by `Exit $LASTEXITCODE`, nothing else; got:\n{script}"
        );
    }

    /// #1443: a registry profile's `env` must reach the terminal-auth
    /// login pane, not just the NDJSON transport's `spawn_with_env` — the
    /// wrapper injects it as `export KEY=VALUE` ahead of the command since
    /// the whole thing is typed into an interactive POSIX shell.
    #[test]
    fn acp_auth_wrapper_posix_exports_env_before_command() {
        let env = vec![("FOO".to_string(), "bar".to_string())];
        let script = build_acp_auth_wrapper("sh login.sh", &env, false);
        assert_eq!(
            script, "export FOO=\"bar\"\nsh login.sh\nexit $?\n",
            "POSIX ACP auth wrapper must export env vars ahead of the \
             command; got:\n{script}"
        );
    }

    /// Same as above, PowerShell flavour (`$env:KEY = "VALUE"`).
    #[test]
    fn acp_auth_wrapper_powershell_sets_env_before_command() {
        let env = vec![("FOO".to_string(), "bar".to_string())];
        let script = build_acp_auth_wrapper("sh login.sh", &env, true);
        assert_eq!(
            script, "$env:FOO = \"bar\"\nsh login.sh\nExit $LASTEXITCODE\n",
            "PowerShell ACP auth wrapper must set env vars ahead of the \
             command; got:\n{script}"
        );
    }

    /// #1344: the POSIX wrapper must write the command's `$?` to the scratch
    /// file `finalize_install_from_terminal` reads back via
    /// `read_install_exit_code`, using the exact path `install_exit_code_path`
    /// computes for the same key — otherwise the writer and reader would silently
    /// disagree on where the handoff lives.
    #[test]
    fn posix_wrapper_writes_exit_code_to_expected_path() {
        let key = "test:posix-writes-exit-code";
        let script = build_terminal_install_wrapper("false", false, key);
        let expected_path = install_exit_code_path(key);
        assert!(
            script.contains(&format!(
                "__exit_code=$?\necho \"$__exit_code\" > \"{}\"",
                expected_path.display()
            )),
            "POSIX wrapper must record $? to the install_exit_code_path for its key; got:\n{script}"
        );
    }

    /// #1344: same handoff, PowerShell flavour — `$LASTEXITCODE` written via
    /// `Set-Content` to the same path `install_exit_code_path` computes.
    #[test]
    fn powershell_wrapper_writes_exit_code_to_expected_path() {
        let key = "test:ps-writes-exit-code";
        let script = build_terminal_install_wrapper("exit 1", true, key);
        let expected_path = install_exit_code_path(key);
        assert!(
            script.contains(&format!(
                "Set-Content -Path '{}' -Value $__ec",
                expected_path.display()
            )),
            "PowerShell wrapper must record $LASTEXITCODE to the install_exit_code_path for its key; got:\n{script}"
        );
    }

    /// `install_key` values contain `:` (e.g. `"ext:bicep:lsp"`), which is
    /// illegal in Windows filenames — confirm the sanitized path never contains
    /// it, and that two different keys never collide on the same file.
    #[test]
    fn install_exit_code_path_sanitizes_key_and_avoids_collisions() {
        let lsp_path = install_exit_code_path("ext:bicep:lsp");
        let dap_path = install_exit_code_path("dap:bicep");
        assert!(!lsp_path.display().to_string().contains(':'));
        assert_ne!(lsp_path, dap_path);
    }

    /// #1344 core acceptance case: a wrapper-recorded non-zero exit code round-trips
    /// through `read_install_exit_code`, and reading consumes (deletes) the scratch
    /// file so a stale code can never leak into a later install reusing the same key.
    #[test]
    fn read_install_exit_code_round_trips_and_consumes_file() {
        let key = "test:round-trip-nonzero";
        let path = install_exit_code_path(key);
        std::fs::write(&path, "1").unwrap();
        assert_eq!(read_install_exit_code(key), Some(1));
        assert!(
            !path.exists(),
            "reading the exit code must delete the scratch file"
        );
        // A second read with nothing written finds nothing — never a stale hit.
        assert_eq!(read_install_exit_code(key), None);
    }

    /// A key that was never written (e.g. the pane was closed before the wrapper's
    /// exit-code line ran) must read back `None`, not `Some(0)` — callers rely on
    /// `None` meaning "unknown" so they fall through to the binary-lookup path
    /// instead of claiming success or failure they can't actually back up.
    #[test]
    fn read_install_exit_code_is_none_when_never_written() {
        let key = "test:never-written-exit-code";
        assert_eq!(read_install_exit_code(key), None);
    }

    /// #1396: `install_exit_code_ready` is `poll_terminal`'s eager-finalize
    /// gate — it must be false before the wrapper has written anything, and
    /// true once the scratch file exists, without consuming it (unlike
    /// `read_install_exit_code`, a second `_ready` check right after must
    /// still see it).
    #[test]
    fn install_exit_code_ready_reflects_scratch_file_existence_without_consuming() {
        let key = "test:ready-reflects-existence";
        let path = install_exit_code_path(key);
        let _ = std::fs::remove_file(&path);

        assert!(
            !install_exit_code_ready(key),
            "must be false before the wrapper has written anything"
        );

        std::fs::write(&path, "0").unwrap();
        assert!(
            install_exit_code_ready(key),
            "must be true once the scratch file exists"
        );
        assert!(
            install_exit_code_ready(key),
            "checking readiness must not consume the file, unlike read_install_exit_code"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// #1396 review (blocking finding): a leftover scratch file from an
    /// earlier attempt under the same `install_key` (e.g. the pane was closed
    /// in the narrow window after the wrapper wrote it but before the shell
    /// exited, or the app crashed) must not survive into a new attempt —
    /// `terminal_run_command` calls `invalidate_install_exit_code` right
    /// before injecting the new wrapper, and this confirms it actually clears
    /// both `install_exit_code_ready` and a subsequent `read_install_exit_code`.
    #[test]
    fn invalidate_install_exit_code_clears_stale_leftover_file() {
        let key = "test:invalidate-clears-stale-leftover";
        let path = install_exit_code_path(key);
        // Simulate a leftover from an earlier, never-finalized attempt.
        std::fs::write(&path, "1").unwrap();
        assert!(install_exit_code_ready(key), "setup: file must exist");

        invalidate_install_exit_code(key);

        assert!(
            !install_exit_code_ready(key),
            "a stale leftover must not be observed as ready after invalidation"
        );
        assert_eq!(
            read_install_exit_code(key),
            None,
            "a stale leftover must not be readable as a real exit code after invalidation"
        );
    }

    /// Calling `invalidate_install_exit_code` when no file exists yet (the
    /// common case — most installs are the extension's first attempt) must be
    /// a harmless no-op, not a panic.
    #[test]
    fn invalidate_install_exit_code_is_noop_when_nothing_to_invalidate() {
        let key = "test:invalidate-noop-when-absent";
        let path = install_exit_code_path(key);
        let _ = std::fs::remove_file(&path);

        invalidate_install_exit_code(key); // must not panic

        assert!(!install_exit_code_ready(key));
    }

    // -----------------------------------------------------------------------
    // #1344: one shared tool lookup — install-time check, finalize, and server
    // launch all agree on where a tool lives.
    // -----------------------------------------------------------------------
    //
    // Before this fix, `binary_on_path` (install-time checks + finalize) only
    // walked `PATH`, while `lsp_manager::resolve_command` (server launch) also
    // probed `~/.local/bin` and friends. A binary a desktop-launched vimcode
    // installed into `~/.local/bin` would resolve for server launch but read
    // as "not found on PATH" for the install check that decides whether to
    // register it — so a *successful* install never got its server started.
    // `binary_on_path` now delegates straight to `resolve_command`, so driving
    // it (and `finalize_install_from_terminal`, which calls it) with a fake
    // `$HOME/.local/bin` binary and a `PATH` that deliberately excludes it
    // proves the two are the same lookup now.
    use super::{binary_on_path, Engine, InstallContext};
    use crate::core::extensions::{ExtensionManifest, LspConfig};

    /// Create a fake `$HOME/.local/bin/<binary_name>` script and return
    /// `(home_dir, binary_path)`. Caller is responsible for cleanup.
    fn fake_home_with_local_bin_binary(tag: &str, binary_name: &str) -> (PathBuf, PathBuf) {
        let home = std::env::temp_dir().join(format!(
            "vimcode_test_home_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let local_bin = home.join(".local").join("bin");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&local_bin).unwrap();
        let binary_path = local_bin.join(binary_name);
        std::fs::write(&binary_path, "#!/bin/sh\necho fake\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (home, binary_path)
    }

    /// A binary present only in a fake `~/.local/bin` (never on `PATH`) is found
    /// by the shared install-time check (`binary_on_path`) — the exact scenario
    /// #1344 reports as broken pre-fix (successful install into `~/.local/bin`
    /// reported as "not found on PATH").
    #[test]
    fn binary_on_path_finds_binary_in_local_bin_when_not_on_path() {
        let binary_name = "vimcode-test-fake-lsp-1344-check";
        let (home, _binary_path) = fake_home_with_local_bin_binary("check", binary_name);

        // Thread-local, *not* `set_var("HOME", …)` — see
        // `core::paths::TEST_HOME_OVERRIDE` for the cross-test corruption
        // the process-global version caused (#957 smoke). No `PATH` guard
        // is needed alongside it: every binary name below is a
        // `vimcode-test-…` literal that cannot exist on a real `PATH`, so
        // the fake `~/.local/bin` probe is still the only way any of them
        // can resolve.
        let _home_guard = crate::core::paths::set_test_home(&home);

        assert!(
            binary_on_path(binary_name),
            "binary_on_path should find {binary_name} via ~/.local/bin even though PATH excludes it"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// #1344 core acceptance case: same fake `~/.local/bin` binary, but driven
    /// through `finalize_install_from_terminal` end-to-end — it must register
    /// and start an LSP server for the extension's language, not report
    /// "not found on PATH" for a tool that plainly *is* findable.
    #[test]
    fn finalize_install_from_terminal_registers_server_for_binary_in_local_bin() {
        let binary_name = "vimcode-test-fake-lsp-1344-finalize";
        let (home, _binary_path) = fake_home_with_local_bin_binary("finalize", binary_name);

        // Thread-local, *not* `set_var("HOME", …)` — see
        // `core::paths::TEST_HOME_OVERRIDE` for the cross-test corruption
        // the process-global version caused (#957 smoke). No `PATH` guard
        // is needed alongside it: every binary name below is a
        // `vimcode-test-…` literal that cannot exist on a real `PATH`, so
        // the fake `~/.local/bin` probe is still the only way any of them
        // can resolve.
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut engine = Engine::new();
        let ext_name = "vimcode-test-ext-1344".to_string();
        let language_id = "vimcode-test-lang-1344".to_string();
        engine.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.clone(),
            display_name: ext_name.clone(),
            language_ids: vec![language_id.clone()],
            lsp: LspConfig {
                binary: binary_name.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);
        // Mirrors what `ext_install_from_registry` does before ever launching
        // the install terminal — without this, `ensure_lsp_manager` wouldn't
        // treat the extension as installed and would skip it entirely.
        engine
            .extension_state
            .mark_installed_version(&ext_name, "0.0.1");

        let ctx = InstallContext {
            ext_name: ext_name.clone(),
            install_key: "test:finalize-local-bin".to_string(),
        };
        engine.finalize_install_from_terminal(&ctx);

        assert!(
            engine.message.contains("installed and started"),
            "finalize should report the server as installed and started, not \
             'not found'; got: {}",
            engine.message
        );
        let mgr = engine
            .lsp_manager
            .as_ref()
            .expect("finalize should have initialized the LSP manager");
        assert!(
            mgr.server_id_for_language(&language_id).is_some(),
            "finalize should have registered and started a server for the \
             extension's language"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// #1344: a non-zero recorded exit code must be reported as an install
    /// failure — even though the binary is findable — and must NOT claim
    /// "not found on PATH", which is exactly the misleading message this issue
    /// reports (a failed install command, e.g. vimcode-ext's broken terraform
    /// installer, used to surface only as a PATH-lookup miss).
    #[test]
    fn finalize_install_from_terminal_reports_failure_for_nonzero_exit_code() {
        // Deliberately give the binary a `~/.local/bin` home so it WOULD
        // resolve if finalize fell through to the binary-lookup path — this
        // proves failure reporting takes priority over a coincidentally
        // resolvable binary, not merely that lookup was skipped because the
        // binary happened to be absent.
        let binary_name = "vimcode-test-fake-lsp-1344-failure";
        let (home, _binary_path) = fake_home_with_local_bin_binary("failure", binary_name);

        // Thread-local, *not* `set_var("HOME", …)` — see
        // `core::paths::TEST_HOME_OVERRIDE` for the cross-test corruption
        // the process-global version caused (#957 smoke). No `PATH` guard
        // is needed alongside it: every binary name below is a
        // `vimcode-test-…` literal that cannot exist on a real `PATH`, so
        // the fake `~/.local/bin` probe is still the only way any of them
        // can resolve.
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut engine = Engine::new();
        let ext_name = "vimcode-test-ext-1344-failure".to_string();
        engine.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.clone(),
            display_name: ext_name.clone(),
            language_ids: vec!["vimcode-test-lang-1344-failure".to_string()],
            lsp: LspConfig {
                binary: binary_name.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);
        engine
            .extension_state
            .mark_installed_version(&ext_name, "0.0.1");

        let install_key = "test:finalize-nonzero-exit".to_string();
        // Simulate the wrapper script having recorded a failed install.
        std::fs::write(install_exit_code_path(&install_key), "3").unwrap();

        let ctx = InstallContext {
            ext_name: ext_name.clone(),
            install_key,
        };
        engine.finalize_install_from_terminal(&ctx);

        assert!(
            engine.message.contains("failed") && engine.message.contains("exit 3"),
            "finalize should report the recorded non-zero exit code as a \
             failure; got: {}",
            engine.message
        );
        assert!(
            !engine.message.to_lowercase().contains("not found"),
            "a failed install must not be reported as 'not found on PATH' — \
             that points at the wrong problem; got: {}",
            engine.message
        );
        assert!(
            engine.lsp_manager.is_none(),
            "a failed install must not register/start a server; lsp_manager \
             should still be uninitialized"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// #1344 blocking review finding, pure-function-level companion to
    /// `extension_install_lsp_success_survives_dap_not_found_via_shell_app`
    /// (`src/tui_main/shell_app.rs`, the driver-tier black-box version): a
    /// manifest with both an LSP server and a DAP adapter — the realistic
    /// shape `extensions::sample_manifests()`'s `rust` entry has — must not
    /// have a successful LSP install message erased by a DAP binary that
    /// isn't resolvable. Fast, in-crate coverage of the exact
    /// `Engine::message` value `finalize_install_from_terminal` produces,
    /// alongside the slower end-to-end version that proves the same text
    /// actually reaches the painted screen.
    #[test]
    fn finalize_install_from_terminal_keeps_lsp_success_alongside_dap_not_found() {
        // LSP binary resolvable via the fake `~/.local/bin`; DAP binary never
        // placed anywhere, so it stays unresolvable.
        let lsp_binary_name = "vimcode-test-fake-lsp-1344-combined-unit";
        let (home, _binary_path) =
            fake_home_with_local_bin_binary("combined-unit", lsp_binary_name);

        // Thread-local, *not* `set_var("HOME", …)` — see
        // `core::paths::TEST_HOME_OVERRIDE` for the cross-test corruption
        // the process-global version caused (#957 smoke). No `PATH` guard
        // is needed alongside it: every binary name below is a
        // `vimcode-test-…` literal that cannot exist on a real `PATH`, so
        // the fake `~/.local/bin` probe is still the only way any of them
        // can resolve.
        let _home_guard = crate::core::paths::set_test_home(&home);

        let mut engine = Engine::new();
        let ext_name = "vimcode-test-ext-1344-combined-unit".to_string();
        let language_id = "vimcode-test-lang-1344-combined-unit".to_string();
        engine.ext_registry = Some(vec![ExtensionManifest {
            name: ext_name.clone(),
            display_name: ext_name.clone(),
            language_ids: vec![language_id],
            lsp: LspConfig {
                binary: lsp_binary_name.to_string(),
                ..Default::default()
            },
            dap: crate::core::extensions::DapConfig {
                adapter: "vimcode-test-dap-adapter-1344-combined-unit".to_string(),
                binary: "vimcode-test-nonexistent-dap-binary-1344-combined-unit".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }]);
        engine
            .extension_state
            .mark_installed_version(&ext_name, "0.0.1");

        let ctx = InstallContext {
            ext_name: ext_name.clone(),
            install_key: "test:finalize-combined-unit".to_string(),
        };
        engine.finalize_install_from_terminal(&ctx);

        assert!(
            engine.message.contains("installed and started"),
            "the LSP success outcome must be present in the combined \
             message, not overwritten by the DAP outcome; got: {}",
            engine.message
        );
        assert!(
            engine.message.contains("was not found"),
            "the DAP not-found outcome must also be present in the combined \
             message; got: {}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(&home);
    }
}
