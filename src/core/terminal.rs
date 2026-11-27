/// Re-export the quadraui primitive so engine/render code can import from one place.
pub use quadraui::terminal_engine::default_shell;
/// Portable "run a command string through the shell" seam:
/// returns `(shell, flag)` — `("sh", "-c")` on Unix, `("cmd", "/C")` on
/// Windows (honouring `$SHELL`/`%COMSPEC%` where set). Sibling of
/// `default_shell()` above; use this instead of hardcoding `Command::new("sh")`
/// anywhere a single command string needs to run through "the user's shell".
pub use quadraui::terminal_engine::shell_command;
pub use quadraui::terminal_engine::TerminalSelection as TermSelection;

/// Build a [`std::process::Command`] that runs `cmd` through the user's
/// shell (`shell_command()` above), with the console window hidden on
/// Windows (`git::hidden_command`) so it can't flash into view.
pub fn shell_cmd(cmd: &str) -> std::process::Command {
    let (shell, flag) = shell_command();
    let mut command = crate::core::git::hidden_command(shell);
    command.arg(flag).arg(cmd);
    command
}

/// Context for a terminal pane running an install command.
/// Stored alongside the pane so we can register the LSP/DAP server after the command finishes.
#[derive(Clone, Debug)]
pub struct InstallContext {
    /// Extension name (e.g. "bicep", "rust").
    pub ext_name: String,
    /// The `lang_id` key used to track in-progress installs (e.g. "ext:bicep:lsp").
    pub install_key: String,
}

/// Metadata attached to a `TerminalSlot` opened by `vimcode.terminal.open`
/// (the extension host-service API), as opposed to a pane the editor
/// opened itself (a plain tab, an install run, an ACP terminal-auth login).
///
/// Lets `Engine::poll_terminal` route this pane's PTY output/exit status to
/// the extension's `on_output`/`on_exit` callbacks, and lets
/// `Engine::plugin_api_terminal_send/close/focus` find the right pane back
/// by its handle id — `terminal_panes` is scanned for a matching
/// `handle_id` rather than keeping a separate index, so the mapping can
/// never desync from a `Vec::remove` elsewhere shifting indices.
#[derive(Clone, Debug)]
pub struct ExtTerminalState {
    /// Handle id returned to Lua — also the key into both
    /// `PluginManager`'s terminal-callback table and `Engine::
    /// plugin_terminals` (the owning-manager liveness map).
    pub handle_id: u64,
    /// `title = "..."` from `vimcode.terminal.open`'s opts, if given.
    /// Stored for the extension's own bookkeeping; no backend currently
    /// paints a per-pane title into the terminal tab strip (that is
    /// `TerminalPanel`/`build_terminal_toolbar` territory, out of this
    /// issue's scope), so this is not yet visible anywhere in the UI.
    pub title: Option<String>,
    /// Line injected right before the command runs — everything between
    /// this and `end_marker_prefix` in `TerminalSession::full_text()` is
    /// the command's own output. Mirrors `acp_terminal_create`'s approach
    /// (`build_acp_terminal_wrapper`/`extract_acp_terminal_output`).
    pub begin_marker: String,
    /// Prefix of the line the wrapper prints right after the command
    /// exits, immediately followed by its numeric exit code.
    pub end_marker_prefix: String,
    /// How many bytes of the captured output (the slice between the two
    /// markers) have already been delivered to `on_output`. Only the
    /// newly-appended suffix is delivered on each poll tick.
    pub delivered_len: usize,
    /// Set once `on_exit` has fired, so a later poll tick never delivers it
    /// twice (the shell keeps running after the wrapped command exits —
    /// unlike an install pane, this pane is not auto-closed, matching the
    /// "shells, REPLs, ssh" interactive use case from the issue).
    pub exit_delivered: bool,
}
