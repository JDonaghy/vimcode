use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use std::io::Write;
use std::sync::mpsc::{self, Receiver};

/// Terminal text selection (row/col are 0-based into content area)
#[derive(Debug, Clone)]
pub struct TermSelection {
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col: u16,
}

/// A single integrated terminal pane backed by a real PTY.
pub struct TerminalPane {
    /// VT100 screen parser — holds current cell grid with colors/attrs.
    pub parser: vt100::Parser,
    /// Write side of the PTY master — sends keyboard input to the shell.
    writer: Box<dyn Write + Send>,
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
    /// Rows scrolled up into the scrollback buffer (0 = at the live bottom).
    pub scroll_offset: usize,
    /// Monotonically-increasing total line count: max(scrollback + rows) ever seen.
    /// Used to compute a meaningful scrollbar thumb size even before the user scrolls.
    pub lines_written: usize,
}

impl TerminalPane {
    /// Spawn a new terminal pane.
    ///
    /// `shell` defaults to `$SHELL` or `/bin/bash`.
    pub fn new(cols: u16, rows: u16, shell: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        // Ensure TERM is set so programs know they have a real terminal.
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;

        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;

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

        let parser = vt100::Parser::new(rows, cols, 1000);

        Ok(TerminalPane {
            parser,
            writer,
            child,
            rx,
            cols,
            rows,
            selection: None,
            exited: false,
            scroll_offset: 0,
            lines_written: rows as usize,
        })
    }

    /// Drain pending output from the PTY reader thread and feed it to the VT100 parser.
    /// Also checks if the child process has exited.
    /// Returns `true` if any new data was processed.
    pub fn poll(&mut self) -> bool {
        let mut got_data = false;
        while let Ok(data) = self.rx.try_recv() {
            // Count raw newlines BEFORE processing so we have the byte slice available.
            let newlines = data.iter().filter(|&&b| b == b'\n').count();
            self.parser.process(&data);
            self.lines_written = self.lines_written.saturating_add(newlines);
            // vt100 auto-increments scrollback_offset as new lines scroll in while the
            // user is scrolled up (keeping the viewed content stable). Sync our field,
            // but re-clamp: vt100's visible_rows() subtracts scrollback_offset from
            // rows_len with plain usize arithmetic, so offset > rows causes a panic.
            let raw = self.parser.screen().scrollback();
            self.scroll_offset = raw.min(self.rows as usize);
            if raw > self.rows as usize {
                self.parser.set_scrollback(self.scroll_offset);
            }
            got_data = true;
        }
        if !self.exited {
            if let Ok(Some(_)) = self.child.try_wait() {
                self.exited = true;
                got_data = true; // redraw to show "[Process exited]"
            }
        }
        got_data
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
        // Best-effort resize signal to the PTY master; ignore errors (e.g. after child exits).
        // portable-pty doesn't expose resize on PtyMaster directly, so we re-open via the
        // system-level ioctl. For now we just update parser size; most shells will resize on
        // their own or we can add TIOCSWINSZ via nix if needed in the future.
    }

    /// Set scroll offset and sync the vt100 parser so `screen.cell()` shows the
    /// correct scrollback content.
    ///
    /// **vt100 limitation**: `visible_rows()` computes `rows_len - scrollback_offset`
    /// with plain usize subtraction; exceeding `rows` (the PTY height) panics in
    /// debug builds and wraps in release builds. We therefore cap the offset at
    /// `rows` — the maximum is one screenful of history.
    pub fn set_scroll_offset(&mut self, offset: usize) {
        // Cap to one screenful (vt100 API limit) and to available history.
        let history = self.lines_written.saturating_sub(self.rows as usize);
        let max = (self.rows as usize).min(history);
        self.scroll_offset = offset.min(max);
        self.parser.set_scrollback(self.scroll_offset);
    }

    /// Scroll up into scrollback by `rows` lines.
    pub fn scroll_up(&mut self, rows: usize) {
        let new_offset = self.scroll_offset.saturating_add(rows);
        self.set_scroll_offset(new_offset);
    }

    /// Scroll down toward the live view by `rows` lines.
    pub fn scroll_down(&mut self, rows: usize) {
        let new_offset = self.scroll_offset.saturating_sub(rows);
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

/// Return the user's preferred shell from `$SHELL`, falling back to `/bin/bash`.
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}
