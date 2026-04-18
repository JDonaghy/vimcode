use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};

/// A single captured terminal cell stored in the history ring buffer.
/// Uses vt100::Color to avoid duplicating the 256-colour palette lookup.
#[derive(Clone, Copy)]
pub struct HistCell {
    pub ch: char,
    pub fg: vt100::Color,
    pub bg: vt100::Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for HistCell {
    fn default() -> Self {
        HistCell {
            ch: ' ',
            fg: vt100::Color::Default,
            bg: vt100::Color::Default,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

/// Terminal text selection (row/col are 0-based into content area)
#[derive(Debug, Clone)]
pub struct TermSelection {
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col: u16,
}

/// Context for a terminal pane running an install command.
/// Stored on the pane so we can register the LSP/DAP server after the command finishes.
#[derive(Clone, Debug)]
pub struct InstallContext {
    /// Extension name (e.g. "bicep", "rust").
    pub ext_name: String,
    /// The `lang_id` key used to track in-progress installs (e.g. "ext:bicep:lsp").
    pub install_key: String,
}

/// A single integrated terminal pane backed by a real PTY.
pub struct TerminalPane {
    /// VT100 screen parser — holds current cell grid with colors/attrs.
    /// The parser is **always** kept at `scrollback_offset = 0` (live view).
    /// Historical content is captured into `history` instead.
    pub parser: vt100::Parser,
    /// Write side of the PTY master — sends keyboard input to the shell.
    writer: Box<dyn Write + Send>,
    /// PTY master — kept alive so we can call `resize()` on it.
    master: Box<dyn MasterPty + Send>,
    /// Shell child process.
    child: Box<dyn Child + Send + Sync>,
    /// Bytes coming from the shell (sent by background reader thread).
    rx: Receiver<Vec<u8>>,
    /// Current terminal width in columns.
    pub cols: u16,
    /// Current terminal height in content rows.
    pub rows: u16,
    /// Current mouse selection (if any).
    pub selection: Option<TermSelection>,
    /// True once the child shell process has exited.
    pub exited: bool,
    /// How many rows above the live bottom the user has scrolled.
    /// 0 = live view; max = `history.len()`.
    pub scroll_offset: usize,
    /// Ring buffer of captured historical rows (oldest at index 0).
    /// Populated by `poll()` as lines scroll off the vt100 live screen.
    pub history: VecDeque<Vec<HistCell>>,
    /// Maximum number of rows kept in `history` (from settings).
    history_capacity: usize,
    /// If set, this pane is running an install command (not an interactive shell).
    /// When the process exits, the engine checks whether the binary is now on PATH.
    pub install_context: Option<InstallContext>,
}

impl TerminalPane {
    /// Spawn a new terminal pane in the given working directory.
    ///
    /// `shell` defaults to `$SHELL` or `/bin/bash`.
    /// `cwd` is the initial working directory (use process CWD if the path is invalid).
    pub fn new(
        cols: u16,
        rows: u16,
        shell: &str,
        cwd: &Path,
        history_capacity: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        // Start the shell in the editor's working directory.
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd)?;

        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;
        let master = pair.master;

        // Spawn background thread that reads PTY output and forwards via channel.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                use std::io::Read;
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF: shell exited
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // 1 000-line vt100 scrollback is kept for internal rendering of the live
        // screen; we never use its set_scrollback() API for user scrolling.
        let parser = vt100::Parser::new(rows, cols, 1000);

        Ok(TerminalPane {
            parser,
            writer,
            master,
            child,
            rx,
            cols,
            rows,
            selection: None,
            exited: false,
            scroll_offset: 0,
            history: VecDeque::new(),
            history_capacity,
            install_context: None,
        })
    }

    /// Spawn a terminal pane that runs a single command instead of an interactive shell.
    /// The command runs via `sh -c "..."` and when it finishes, a status line is printed
    /// and the pane waits for the user to press Enter before exiting.
    pub fn new_command(
        cols: u16,
        rows: u16,
        command: &str,
        cwd: &Path,
        history_capacity: usize,
        install_ctx: Option<InstallContext>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Wrap the command so:
        // 1. It runs in a login shell (picks up user PATH for sudo, dotnet, etc.)
        // 2. Exit code is captured and displayed
        // 3. The pane stays open until the user presses Enter
        let shell = default_shell();
        let is_powershell =
            shell.to_lowercase().contains("powershell") || shell.to_lowercase().contains("pwsh");

        let (shell_flag, wrapped) = if is_powershell {
            // PowerShell wrapper: run command, check $LASTEXITCODE, wait for Enter.
            let ps = format!(
                concat!(
                    "{cmd}; ",
                    "$__ec = $LASTEXITCODE; ",
                    "Write-Host ''; ",
                    "if ($__ec -eq 0 -or $null -eq $__ec) {{ ",
                    "Write-Host \"`e[32m✓ Command completed successfully`e[0m\" ",
                    "}} else {{ ",
                    "Write-Host \"`e[31m✗ Command failed (exit code $__ec)`e[0m\" ",
                    "}}; ",
                    "Write-Host ''; ",
                    "Write-Host 'Press Enter to close…'; ",
                    "Read-Host"
                ),
                cmd = command
            );
            ("-Command", ps)
        } else {
            // Unix shell wrapper (bash/zsh/sh).
            let sh = format!(
                "{cmd}\n__exit_code=$?\necho ''\nif [ $__exit_code -eq 0 ]; then echo '\\033[32m✓ Command completed successfully\\033[0m'; else echo \"\\033[31m✗ Command failed (exit code $__exit_code)\\033[0m\"; fi\necho ''\necho 'Press Enter to close…'\nread __dummy",
                cmd = command
            );
            ("-c", sh)
        };

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(&shell);
        cmd.args([shell_flag, &wrapped]);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd)?;

        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;
        let master = pair.master;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                use std::io::Read;
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let parser = vt100::Parser::new(rows, cols, 1000);

        Ok(TerminalPane {
            parser,
            writer,
            master,
            child,
            rx,
            cols,
            rows,
            selection: None,
            exited: false,
            scroll_offset: 0,
            history: VecDeque::new(),
            history_capacity,
            install_context: install_ctx,
        })
    }

    /// Drain pending output from the PTY reader thread and feed it to the VT100 parser.
    /// Lines that scroll off the visible area are captured into `self.history`.
    /// Also checks if the child process has exited.
    /// Returns `true` if any new data was processed.
    pub fn poll(&mut self) -> bool {
        let mut got_data = false;
        while let Ok(data) = self.rx.try_recv() {
            got_data = true;
            // Process in chunks of at most `rows` newlines so we can capture
            // every row that scrolls off the vt100 live screen between chunks.
            self.process_with_capture(&data);
        }
        if !self.exited {
            if let Ok(Some(_)) = self.child.try_wait() {
                self.exited = true;
                got_data = true; // redraw to show "[Process exited]"
            }
        }
        got_data
    }

    /// Process a data chunk, splitting at `rows`-newline boundaries so that each
    /// sub-chunk causes at most `rows` lines to scroll off — the maximum we can
    /// safely read back from vt100's scrollback API (set_scrollback(N ≤ rows)).
    fn process_with_capture(&mut self, data: &[u8]) {
        let max_nl = self.rows as usize;
        let mut start = 0;
        let mut nl_count = 0;

        for (i, &b) in data.iter().enumerate() {
            if b == b'\n' {
                nl_count += 1;
                if nl_count >= max_nl {
                    let chunk = &data[start..=i];
                    self.parser.process(chunk);
                    self.capture_scrolled_rows(nl_count);
                    start = i + 1;
                    nl_count = 0;
                }
            }
        }
        // Remaining data (may or may not contain newlines).
        if start < data.len() {
            let chunk = &data[start..];
            let remaining_nl = chunk.iter().filter(|&&b| b == b'\n').count();
            self.parser.process(chunk);
            if remaining_nl > 0 {
                self.capture_scrolled_rows(remaining_nl);
            }
        }
    }

    /// Read the rows that *just* scrolled off the top of the vt100 live screen
    /// and append them to `self.history`.
    ///
    /// After processing `n_newlines` of new output, the vt100 scrollback deque
    /// contains those rows at its tail. We call `set_scrollback(n_to_capture)`
    /// (capped at `rows`, which is the safe vt100 maximum) to bring them into
    /// the visible window, read them, then restore the live view.
    fn capture_scrolled_rows(&mut self, n_newlines: usize) {
        let to_capture = n_newlines.min(self.rows as usize);
        // Temporarily shift the vt100 viewport to see the rows that just scrolled off.
        self.parser.set_scrollback(to_capture);
        {
            let screen = self.parser.screen();
            for r in 0..to_capture as u16 {
                let row: Vec<HistCell> = (0..self.cols)
                    .map(|c| match screen.cell(r, c) {
                        Some(cell) => {
                            let raw = cell.contents();
                            HistCell {
                                ch: raw.chars().next().unwrap_or(' '),
                                fg: cell.fgcolor(),
                                bg: cell.bgcolor(),
                                bold: cell.bold(),
                                italic: cell.italic(),
                                underline: cell.underline(),
                            }
                        }
                        None => HistCell::default(),
                    })
                    .collect();
                if self.history_capacity > 0 && self.history.len() >= self.history_capacity {
                    self.history.pop_front();
                }
                self.history.push_back(row);
            }
        }
        // Restore live view — always safe since we're back to offset 0.
        self.parser.set_scrollback(0);
    }

    /// Send raw bytes as keyboard input to the shell.
    pub fn write_input(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data);
        let _ = self.writer.flush();
    }

    /// Resize the PTY and update the VT100 parser dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.parser.set_size(rows, cols);
        // Notify the PTY master so the shell (and running programs) see SIGWINCH.
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Set the scroll offset (0 = live view, max = `history.len()`).
    pub fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset.min(self.history.len());
        // Keep the vt100 parser at the live view — history is served from our buffer.
        self.parser.set_scrollback(0);
    }

    /// Scroll up into history by `n` rows.
    pub fn scroll_up(&mut self, n: usize) {
        let new_offset = self.scroll_offset.saturating_add(n);
        self.set_scroll_offset(new_offset);
    }

    /// Scroll down toward the live view by `n` rows.
    pub fn scroll_down(&mut self, n: usize) {
        let new_offset = self.scroll_offset.saturating_sub(n);
        self.set_scroll_offset(new_offset);
    }

    /// Return to the live view (scroll_offset = 0).
    pub fn scroll_reset(&mut self) {
        self.set_scroll_offset(0);
    }

    /// Extract selected text from the current VT100 screen.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let screen = self.parser.screen();
        let mut lines: Vec<String> = Vec::new();
        let (r0, c0, r1, c1) = normalize_selection(sel);
        for row in r0..=r1 {
            let mut line = String::new();
            let col_start = if row == r0 { c0 } else { 0 };
            let col_end = if row == r1 {
                c1
            } else {
                self.cols.saturating_sub(1)
            };
            for col in col_start..=col_end {
                if let Some(cell) = screen.cell(row, col) {
                    let s = cell.contents();
                    if s.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(&s);
                    }
                }
            }
            // Trim trailing spaces from each row.
            lines.push(line.trim_end().to_string());
        }
        Some(lines.join("\n"))
    }
}

/// Normalize a selection so start ≤ end in reading order.
fn normalize_selection(sel: &TermSelection) -> (u16, u16, u16, u16) {
    if (sel.start_row, sel.start_col) <= (sel.end_row, sel.end_col) {
        (sel.start_row, sel.start_col, sel.end_row, sel.end_col)
    } else {
        (sel.end_row, sel.end_col, sel.start_row, sel.start_col)
    }
}

/// Return the user's preferred shell.
///
/// On Unix: reads `$SHELL`, falls back to `/bin/bash`.
/// On Windows: reads `$SHELL`, falls back to `powershell.exe`.
pub fn default_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        return shell;
    }
    #[cfg(target_os = "windows")]
    {
        "powershell.exe".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        "/bin/bash".to_string()
    }
}
