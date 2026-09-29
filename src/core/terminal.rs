/// Re-export the quadraui primitive so engine/render code can import from one place.
pub use quadraui::terminal_engine::default_shell;
/// Portable "run a command string through the shell" seam (quadraui#970):
/// returns `(shell, flag)` — `("sh", "-c")` on Unix, `("cmd", "/C")` on
/// Windows (honouring `$SHELL`/`%COMSPEC%` where set). Sibling of
/// `default_shell()` above; use this instead of hardcoding `Command::new("sh")`
/// anywhere a single command string needs to run through "the user's shell".
pub use quadraui::terminal_engine::shell_command;
pub use quadraui::terminal_engine::TerminalSelection as TermSelection;

/// Build a [`std::process::Command`] that runs `cmd` through the user's
/// shell (`shell_command()` above), with the console window hidden on
/// Windows (`git::hidden_command`) so it can't flash into view.
///
/// Single construction point for every "run this string through the
/// shell" spawn site — `:!`, `:r !`, `!{motion}` filters, plugin shell
/// requests, and install commands. Before #1492, only the LSP spawn site
/// wrapped its `Command` in `hidden_command`; the other four sites built
/// `Command::new(shell)` directly, so on Win-GUI each one flashed a console
/// window. Use this instead of hand-rolling `Command::new(shell).arg(flag)
/// .arg(cmd)` at a new call site.
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
