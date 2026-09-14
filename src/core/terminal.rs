/// Re-export the quadraui primitive so engine/render code can import from one place.
pub use quadraui::terminal_engine::default_shell;
/// Portable "run a command string through the shell" seam (quadraui#970):
/// returns `(shell, flag)` — `("sh", "-c")` on Unix, `("cmd", "/C")` on
/// Windows (honouring `$SHELL`/`%COMSPEC%` where set). Sibling of
/// `default_shell()` above; use this instead of hardcoding `Command::new("sh")`
/// anywhere a single command string needs to run through "the user's shell".
pub use quadraui::terminal_engine::shell_command;
pub use quadraui::terminal_engine::TerminalSelection as TermSelection;

/// Context for a terminal pane running an install command.
/// Stored alongside the pane so we can register the LSP/DAP server after the command finishes.
#[derive(Clone, Debug)]
pub struct InstallContext {
    /// Extension name (e.g. "bicep", "rust").
    pub ext_name: String,
    /// The `lang_id` key used to track in-progress installs (e.g. "ext:bicep:lsp").
    pub install_key: String,
}
