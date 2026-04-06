use super::*;

impl Engine {
    // ─── Source Control ────────────────────────────────────────────────────────

    /// Refresh SC panel: re-run `git status` and `git worktree list`.
    pub fn sc_refresh(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        self.sc_file_statuses = git::status_detailed(&dir);
        self.sc_worktrees = git::worktree_list(&dir);
        let (ahead, behind) = git::ahead_behind(&dir);
        self.sc_ahead = ahead;
        self.sc_behind = behind;
        self.sc_log = git::git_log(&dir, 20);
    }

    /// Stage or unstage the currently selected SC item.
    /// If the cursor is on a section header (idx == usize::MAX):
    ///   - STAGED header → unstage all
    ///   - CHANGES header → stage all
    pub fn sc_stage_selected(&mut self) {
        let (section, idx) = self.sc_flat_to_section_idx(self.sc_selected);
        // Section header: bulk operation
        if idx == usize::MAX {
            if section == 0 {
                self.sc_unstage_all();
            } else if section == 1 {
                self.sc_stage_all();
            }
            return;
        }
        // Use git repo root so paths from `git status` (relative to git root) work
        // correctly even when cwd is a sub-directory.
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        if section == 0 {
            // Staged section: unstage the selected file
            let staged: Vec<git::FileStatus> = self
                .sc_file_statuses
                .iter()
                .filter(|f| f.staged.is_some())
                .cloned()
                .collect();
            if let Some(f) = staged.get(idx) {
                let path = f.path.clone();
                match git::unstage_path(&dir, &path) {
                    Ok(()) => {}
                    Err(e) => self.message = format!("unstage: {e}"),
                }
            }
        } else if section == 1 {
            // Unstaged/untracked section: stage the selected file
            let unstaged: Vec<git::FileStatus> = self
                .sc_file_statuses
                .iter()
                .filter(|f| f.unstaged.is_some())
                .cloned()
                .collect();
            if let Some(f) = unstaged.get(idx) {
                let path = f.path.clone();
                match git::stage_path(&dir, &path) {
                    Ok(()) => {}
                    Err(e) => self.message = format!("stage: {e}"),
                }
            }
        }
        self.sc_refresh();
    }

    /// Stage all unstaged/untracked files.
    pub fn sc_stage_all(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        let _ = git::stage_path(&dir, ".");
        self.sc_refresh();
    }

    /// Unstage all staged files.
    #[allow(dead_code)]
    pub fn sc_unstage_all(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        let _ = git::unstage_all(&dir);
        self.sc_refresh();
    }

    /// Discard all unstaged working-tree changes (called after dialog confirmation).
    pub fn sc_discard_all_unstaged(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        let _ = git::discard_all(&dir);
        self.sc_refresh();
    }

    /// Show a confirmation dialog before discarding all unstaged changes.
    pub fn sc_confirm_discard_all(&mut self) {
        self.pending_sc_discard = Some(String::new()); // empty = all
        self.show_dialog(
            "confirm_sc_discard",
            "Discard Changes",
            vec![
                "Discard ALL unstaged changes?".to_string(),
                "This cannot be undone.".to_string(),
            ],
            vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "Discard All".into(),
                    hotkey: 'd',
                    action: "discard".into(),
                },
            ],
        );
    }

    /// Run `git pull` and show the result as a status message.
    pub fn sc_pull(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        match git::pull(&dir) {
            Ok(msg) => {
                self.message = if msg.is_empty() {
                    "Already up to date.".to_string()
                } else {
                    msg
                };
            }
            Err(e) => {
                if git::is_ssh_auth_error(&e) {
                    self.sc_show_passphrase_dialog("pull");
                } else {
                    self.message = format!("pull: {}", e);
                }
            }
        }
        self.sc_refresh();
    }

    /// Run `git fetch` and show the result as a status message.
    pub fn sc_fetch(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        match git::fetch(&dir) {
            Ok(msg) => {
                self.message = if msg.is_empty() {
                    "Fetched.".to_string()
                } else {
                    msg
                };
            }
            Err(e) => {
                if git::is_ssh_auth_error(&e) {
                    self.sc_show_passphrase_dialog("fetch");
                } else {
                    self.message = format!("fetch: {}", e);
                }
            }
        }
        self.sc_refresh();
    }

    /// Run `git push` and show the result as a status message.
    pub fn sc_push(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        match git::push(&dir) {
            Ok(msg) => {
                self.message = if msg.is_empty() {
                    "Pushed.".to_string()
                } else {
                    msg
                };
            }
            Err(e) => {
                if git::is_ssh_auth_error(&e) {
                    self.sc_show_passphrase_dialog("push");
                } else {
                    self.message = format!("push: {}", e);
                }
            }
        }
        self.sc_refresh();
    }

    /// Show an SSH passphrase dialog for a failed remote git operation.
    pub(crate) fn sc_show_passphrase_dialog(&mut self, op: &str) {
        self.pending_git_remote_op = Some(op.to_string());
        let mut dialog = Dialog {
            tag: "ssh_passphrase".to_string(),
            title: "SSH Key Passphrase".to_string(),
            body: vec![format!(
                "Enter passphrase for SSH key (leave empty if none):"
            )],
            buttons: vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "OK".into(),
                    hotkey: '\0',
                    action: "ok".into(),
                },
            ],
            selected: 1,
            input: Some(DialogInput {
                label: "Passphrase".into(),
                value: String::new(),
                is_password: true,
            }),
        };
        // Suppress hotkeys — input dialog uses printable chars for typing.
        dialog.buttons[0].hotkey = '\0';
        dialog.buttons[1].hotkey = '\0';
        self.dialog = Some(dialog);
    }

    /// Commit all staged changes with the current commit message.
    pub fn sc_do_commit(&mut self) {
        let msg = self.sc_commit_message.trim().to_string();
        if msg.is_empty() {
            self.message = "Commit message is empty".to_string();
            return;
        }
        let dir = self.cwd.clone();
        match git::commit(&dir, &msg) {
            Ok(out) => {
                // Use first line only — multi-line output would corrupt the status bar
                let first_line = out.lines().next().unwrap_or("Committed.");
                self.message = first_line.to_string();
                self.sc_commit_message.clear();
                self.sc_commit_cursor = 0;
                self.sc_commit_input_active = false;
            }
            Err(e) => self.message = format!("commit: {}", e),
        }
        self.sc_refresh();
    }

    /// Handle a key when the commit message input row is active.
    pub fn handle_sc_commit_input_key(
        &mut self,
        key: &str,
        ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        // Clamp cursor in case message was modified externally.
        let len = self.sc_commit_message.len();
        if self.sc_commit_cursor > len {
            self.sc_commit_cursor = len;
        }

        match key {
            "Return" | "KP_Enter" => {
                if ctrl {
                    self.sc_do_commit();
                } else {
                    self.sc_commit_message.insert(self.sc_commit_cursor, '\n');
                    self.sc_commit_cursor += 1;
                }
                true
            }
            "Escape" => {
                self.sc_commit_input_active = false;
                true
            }
            "BackSpace" => {
                if self.sc_commit_cursor > 0 {
                    // Find the previous char boundary.
                    let prev = self.sc_commit_message[..self.sc_commit_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.sc_commit_message.remove(prev);
                    self.sc_commit_cursor = prev;
                }
                true
            }
            "Delete" => {
                if self.sc_commit_cursor < self.sc_commit_message.len() {
                    self.sc_commit_message.remove(self.sc_commit_cursor);
                }
                true
            }
            "Left" => {
                if self.sc_commit_cursor > 0 {
                    let prev = self.sc_commit_message[..self.sc_commit_cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.sc_commit_cursor = prev;
                }
                true
            }
            "Right" => {
                if self.sc_commit_cursor < self.sc_commit_message.len() {
                    let ch = self.sc_commit_message[self.sc_commit_cursor..]
                        .chars()
                        .next()
                        .unwrap();
                    self.sc_commit_cursor += ch.len_utf8();
                }
                true
            }
            "Home" => {
                // Move to start of current line.
                let before = &self.sc_commit_message[..self.sc_commit_cursor];
                self.sc_commit_cursor = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                true
            }
            "End" => {
                // Move to end of current line.
                let after = &self.sc_commit_message[self.sc_commit_cursor..];
                self.sc_commit_cursor += after.find('\n').unwrap_or(after.len());
                true
            }
            "Up" => {
                // Move cursor up one line (same column if possible).
                let before = &self.sc_commit_message[..self.sc_commit_cursor];
                let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let col = self.sc_commit_cursor - line_start;
                if line_start > 0 {
                    // There is a previous line.
                    let prev_line_end = line_start - 1; // the '\n'
                    let prev_before = &self.sc_commit_message[..prev_line_end];
                    let prev_line_start = prev_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                    let prev_line_len = prev_line_end - prev_line_start;
                    self.sc_commit_cursor = prev_line_start + col.min(prev_line_len);
                }
                true
            }
            "Down" => {
                // Move cursor down one line (same column if possible).
                let before = &self.sc_commit_message[..self.sc_commit_cursor];
                let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let col = self.sc_commit_cursor - line_start;
                let after = &self.sc_commit_message[self.sc_commit_cursor..];
                if let Some(nl) = after.find('\n') {
                    let next_line_start = self.sc_commit_cursor + nl + 1;
                    let rest = &self.sc_commit_message[next_line_start..];
                    let next_line_len = rest.find('\n').unwrap_or(rest.len());
                    self.sc_commit_cursor = next_line_start + col.min(next_line_len);
                }
                true
            }
            _ => {
                if ctrl {
                    // Ctrl+V paste from system clipboard.
                    if unicode == Some('v') || unicode == Some('V') || key == "v" {
                        if let Some(text) = Self::clipboard_paste() {
                            self.sc_commit_message
                                .insert_str(self.sc_commit_cursor, &text);
                            self.sc_commit_cursor += text.len();
                            return true;
                        }
                    }
                    // Ctrl+A = select all / move to start, Ctrl+E = move to end
                    if unicode == Some('a') || key == "a" {
                        self.sc_commit_cursor = 0;
                        return true;
                    }
                    if unicode == Some('e') || key == "e" {
                        self.sc_commit_cursor = self.sc_commit_message.len();
                        return true;
                    }
                    false
                } else if let Some(ch) = unicode {
                    self.sc_commit_message.insert(self.sc_commit_cursor, ch);
                    self.sc_commit_cursor += ch.len_utf8();
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Try to paste from the system clipboard. Returns None on failure.
    pub fn clipboard_paste() -> Option<String> {
        #[cfg(test)]
        {
            None
        }
        #[cfg(not(test))]
        {
            use std::process::Command;
            // Try xclip first, then xsel, then wl-paste (Wayland).
            for cmd in &[
                &["xclip", "-selection", "clipboard", "-o"][..],
                &["xsel", "--clipboard", "--output"][..],
                &["wl-paste", "--no-newline"][..],
                &["pbpaste"][..],
            ] {
                if let Ok(out) = Command::new(cmd[0]).args(&cmd[1..]).output() {
                    if out.status.success() {
                        return Some(String::from_utf8_lossy(&out.stdout).into_owned());
                    }
                }
            }
            None
        }
    }

    /// Activate an SC action button by index: 0=Commit, 1=Push, 2=Pull, 3=Sync.
    pub fn sc_activate_button(&mut self, idx: usize) {
        match idx {
            0 => self.sc_do_commit(),
            1 => self.sc_push(),
            2 => self.sc_pull(),
            3 => self.sc_sync(),
            _ => {}
        }
    }

    /// Pull, then push (VSCode-style "Sync Changes").
    pub fn sc_sync(&mut self) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        match git::pull(&dir) {
            Ok(_) => match git::push(&dir) {
                Ok(msg) => {
                    self.message = format!("sync: {}", msg.lines().next().unwrap_or("done"));
                }
                Err(e) => {
                    self.message = format!("sync push failed: {}", e.lines().next().unwrap_or(&e));
                }
            },
            Err(e) => {
                self.message = format!("sync pull failed: {}", e.lines().next().unwrap_or(&e));
            }
        }
        self.sc_refresh();
    }

    /// Handle a key when a button row button has keyboard focus.
    pub(crate) fn handle_sc_button_key(&mut self, key: &str, btn: usize) -> bool {
        match key {
            "h" | "Left" => {
                self.sc_button_focused = Some(btn.saturating_sub(1));
                true
            }
            "l" | "Right" => {
                self.sc_button_focused = Some((btn + 1).min(3));
                true
            }
            "Return" | "Enter" | " " => {
                self.sc_activate_button(btn);
                true
            }
            // Drop back to file list without leaving the panel.
            "j" | "k" | "Tab" | "Escape" => {
                self.sc_button_focused = None;
                true
            }
            // Fully exit SC panel.
            "q" => {
                self.sc_button_focused = None;
                self.sc_has_focus = false;
                true
            }
            _ => true,
        }
    }

    /// Discard working-tree changes for the selected unstaged file (called after dialog confirmation).
    pub fn sc_discard_file(&mut self, path: &str) {
        let dir = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        match git::discard_path(&dir, path) {
            Ok(()) => {}
            Err(e) => self.message = format!("discard: {e}"),
        }
        self.sc_refresh();
    }

    /// Show a confirmation dialog before discarding changes for the selected file.
    pub fn sc_confirm_discard_selected(&mut self) {
        let (section, idx) = self.sc_flat_to_section_idx(self.sc_selected);
        if section != 1 || idx == usize::MAX {
            return;
        }
        let unstaged: Vec<git::FileStatus> = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.unstaged.is_some())
            .cloned()
            .collect();
        if let Some(f) = unstaged.get(idx) {
            let name = f.path.rsplit('/').next().unwrap_or(&f.path);
            self.pending_sc_discard = Some(f.path.clone());
            self.show_dialog(
                "confirm_sc_discard",
                "Discard Changes",
                vec![
                    format!("Discard changes to '{name}'?"),
                    "This cannot be undone.".to_string(),
                ],
                vec![
                    DialogButton {
                        label: "Cancel".into(),
                        hotkey: '\0',
                        action: "cancel".into(),
                    },
                    DialogButton {
                        label: "Discard".into(),
                        hotkey: 'd',
                        action: "discard".into(),
                    },
                ],
            );
        }
    }

    /// Switch to the selected worktree's directory.
    pub fn sc_switch_worktree(&mut self, idx: usize) {
        if let Some(wt) = self.sc_worktrees.get(idx) {
            let path = wt.path.clone();
            self.open_folder(&path);
        }
    }

    /// Handle a key press when the Source Control panel has focus.
    /// Returns true if the key was consumed.
    pub fn handle_sc_key(&mut self, key: &str, ctrl: bool, unicode: Option<char>) -> bool {
        // Help dialog: any key closes it.
        if self.sc_help_open {
            self.sc_help_open = false;
            return true;
        }
        // Branch picker popup.
        if self.sc_branch_picker_open {
            return self.handle_sc_branch_picker_key(key, ctrl, unicode);
        }
        // Branch create input.
        if self.sc_branch_create_mode {
            return self.handle_sc_branch_create_key(key, unicode);
        }
        // If commit input is active, delegate to the input handler.
        if self.sc_commit_input_active {
            return self.handle_sc_commit_input_key(key, ctrl, unicode);
        }
        // If a button is focused, delegate to the button handler.
        if let Some(btn) = self.sc_button_focused {
            return self.handle_sc_button_key(key, btn);
        }
        let flat_len = self.sc_flat_len();
        match key {
            "j" | "Down" => {
                if flat_len > 0 {
                    self.sc_selected = (self.sc_selected + 1).min(flat_len - 1);
                }
                true
            }
            "k" | "Up" => {
                self.sc_selected = self.sc_selected.saturating_sub(1);
                true
            }
            "s" => {
                self.sc_stage_selected();
                true
            }
            "S" => {
                self.sc_stage_all();
                true
            }
            "d" => {
                self.sc_confirm_discard_selected();
                true
            }
            "D" => {
                self.sc_confirm_discard_all();
                true
            }
            "c" => {
                self.sc_commit_input_active = true;
                true
            }
            "C" => {
                // Commit immediately if there's a message, otherwise enter input mode.
                if self.sc_commit_message.trim().is_empty() {
                    self.sc_commit_input_active = true;
                } else {
                    self.sc_do_commit();
                }
                true
            }
            "p" => {
                self.sc_pull();
                true
            }
            "P" => {
                self.sc_push();
                true
            }
            "f" => {
                self.sc_fetch();
                true
            }
            "Tab" => {
                let (section, idx) = self.sc_flat_to_section_idx(self.sc_selected);
                if idx == usize::MAX {
                    // On a section header: toggle expand/collapse.
                    if section < 4 {
                        self.sc_sections_expanded[section] = !self.sc_sections_expanded[section];
                    }
                } else {
                    // On a file/item row: enter button row.
                    self.sc_button_focused = Some(0);
                }
                true
            }
            "Return" | "Enter" => {
                let (section, idx) = self.sc_flat_to_section_idx(self.sc_selected);
                if section == 2 {
                    // Worktree: switch and keep panel focus.
                    self.sc_switch_worktree(idx);
                } else if section == 3 && idx != usize::MAX {
                    // Log entry: show the commit hash + message in the status bar.
                    if let Some(entry) = self.sc_log.get(idx) {
                        self.message = format!("{} {}", entry.hash, entry.message);
                    }
                } else if idx != usize::MAX {
                    // File row: open in editor, keep SC panel focus so the user
                    // can continue navigating / staging without re-clicking.
                    // (Press q / Escape to return focus to the editor.)
                    let statuses = self.sc_file_statuses.clone();
                    let all_files: Vec<&git::FileStatus> = if section == 0 {
                        statuses.iter().filter(|f| f.staged.is_some()).collect()
                    } else {
                        statuses.iter().filter(|f| f.unstaged.is_some()).collect()
                    };
                    if let Some(f) = all_files.get(idx) {
                        // Use the git repo root to resolve the path so that
                        // git-relative paths work even when cwd is a sub-dir.
                        let git_root =
                            git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
                        let path = git_root.join(&f.path);
                        if !path.exists() {
                            self.message = format!("SC: file not found: {}", path.display());
                        } else {
                            // For files with a HEAD version, open diff split.
                            // For untracked/new files, open normally.
                            let is_new = matches!(f.unstaged, Some(git::StatusKind::Untracked))
                                || matches!(f.staged, Some(git::StatusKind::Added));
                            let has_head = !is_new
                                && git::show_file_at_ref(&git_root, "HEAD", &f.path).is_some();
                            if has_head {
                                self.cmd_git_diff_split(&path);
                            } else {
                                let _ = self
                                    .open_file_with_mode(&path, crate::core::OpenMode::Permanent);
                            }
                            // Clear focus so the editor receives keys after opening.
                            self.sc_has_focus = false;
                            self.sc_button_focused = None;
                        }
                    }
                }
                // Header rows (idx == usize::MAX): no action for Enter.
                true
            }
            "q" | "Escape" => {
                self.sc_has_focus = false;
                self.sc_button_focused = None;
                true
            }
            "r" => {
                self.sc_refresh();
                true
            }
            "b" => {
                self.sc_open_branch_picker();
                true
            }
            "B" => {
                self.sc_open_branch_create();
                true
            }
            "?" => {
                self.sc_help_open = true;
                true
            }
            _ => false,
        }
    }

    // ── Branch picker ─────────────────────────────────────────────────

    pub(crate) fn sc_open_branch_picker(&mut self) {
        let root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        self.sc_branch_picker_branches = git::list_branches(&root);
        self.sc_branch_picker_query.clear();
        self.sc_branch_picker_selected = 0;
        self.sc_branch_picker_open = true;
    }

    pub(crate) fn sc_close_branch_picker(&mut self) {
        self.sc_branch_picker_open = false;
        self.sc_branch_picker_query.clear();
        self.sc_branch_picker_branches.clear();
        self.sc_branch_picker_selected = 0;
    }

    pub fn sc_branch_picker_filtered(&self) -> Vec<(usize, i32)> {
        let q = &self.sc_branch_picker_query;
        let mut results: Vec<(usize, i32)> = self
            .sc_branch_picker_branches
            .iter()
            .enumerate()
            .filter_map(|(i, b)| Self::fuzzy_score(&b.name, q).map(|s| (i, s)))
            .collect();
        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.truncate(50);
        results
    }

    pub(crate) fn sc_branch_picker_confirm(&mut self) {
        let filtered = self.sc_branch_picker_filtered();
        if let Some(&(idx, _)) = filtered.get(self.sc_branch_picker_selected) {
            let branch = self.sc_branch_picker_branches[idx].name.clone();
            let root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
            // Strip "remotes/origin/" prefix for remote branches.
            let name = branch.strip_prefix("remotes/origin/").unwrap_or(&branch);
            match git::checkout_branch(&root, name) {
                Ok(()) => {
                    self.message = format!("Switched to {name}");
                    self.sc_refresh();
                }
                Err(e) => self.message = format!("Switch failed: {e}"),
            }
        }
        self.sc_close_branch_picker();
    }

    pub(crate) fn handle_sc_branch_picker_key(
        &mut self,
        key: &str,
        ctrl: bool,
        unicode: Option<char>,
    ) -> bool {
        match key {
            "Escape" | "q" => {
                self.sc_close_branch_picker();
            }
            "Return" | "Enter" => {
                self.sc_branch_picker_confirm();
            }
            "Up" | "k" => {
                self.sc_branch_picker_selected = self.sc_branch_picker_selected.saturating_sub(1);
            }
            "Down" | "j" => {
                let count = self.sc_branch_picker_filtered().len();
                if count > 0 {
                    self.sc_branch_picker_selected =
                        (self.sc_branch_picker_selected + 1).min(count - 1);
                }
            }
            "BackSpace" => {
                self.sc_branch_picker_query.pop();
                self.sc_branch_picker_selected = 0;
            }
            _ => {
                if ctrl {
                    return true;
                }
                if let Some(ch) = unicode {
                    if !ch.is_control() {
                        self.sc_branch_picker_query.push(ch);
                        self.sc_branch_picker_selected = 0;
                    }
                }
            }
        }
        true
    }

    // ── Branch create ────────────────────────────────────────────────

    pub(crate) fn sc_open_branch_create(&mut self) {
        self.sc_branch_create_mode = true;
        self.sc_branch_create_input.clear();
    }

    pub(crate) fn sc_branch_create_confirm(&mut self) {
        let name = self.sc_branch_create_input.trim().to_string();
        if name.is_empty() {
            self.message = "Branch name cannot be empty".to_string();
            self.sc_branch_create_mode = false;
            return;
        }
        let root = git::find_repo_root(&self.cwd).unwrap_or_else(|| self.cwd.clone());
        match git::create_branch(&root, &name) {
            Ok(()) => {
                self.message = format!("Created and switched to {name}");
                self.sc_refresh();
            }
            Err(e) => self.message = format!("Create branch failed: {e}"),
        }
        self.sc_branch_create_mode = false;
        self.sc_branch_create_input.clear();
    }

    pub(crate) fn handle_sc_branch_create_key(&mut self, key: &str, unicode: Option<char>) -> bool {
        match key {
            "Escape" => {
                self.sc_branch_create_mode = false;
                self.sc_branch_create_input.clear();
            }
            "Return" | "Enter" => {
                self.sc_branch_create_confirm();
            }
            "BackSpace" => {
                self.sc_branch_create_input.pop();
            }
            _ => {
                if let Some(ch) = unicode {
                    if !ch.is_control() {
                        self.sc_branch_create_input.push(ch);
                    }
                }
            }
        }
        true
    }

    /// Number of visible flat rows across all sections.
    /// The WORKTREES section is only counted when there are linked worktrees
    /// (i.e. `sc_worktrees.len() > 1` — the main worktree is always present).
    pub fn sc_flat_len(&self) -> usize {
        let staged_count = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.staged.is_some())
            .count();
        let unstaged_count = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.unstaged.is_some())
            .count();
        let show_worktrees = self.sc_worktrees.len() > 1;
        // 2–3 section headers (staged + unstaged + optional worktrees) + 1 log header
        let base = if show_worktrees { 4 } else { 3 };
        base + if self.sc_sections_expanded[0] {
            staged_count
        } else {
            0
        } + if self.sc_sections_expanded[1] {
            unstaged_count
        } else {
            0
        } + if show_worktrees && self.sc_sections_expanded[2] {
            self.sc_worktrees.len()
        } else {
            0
        } + if self.sc_sections_expanded[3] {
            self.sc_log.len()
        } else {
            0
        }
    }

    /// Map a visual row index (0=panel header, 1=commit input, 2+=sections)
    /// to `Some((flat_idx, is_section_header))`, or `None` if the row is
    /// outside the section area.
    ///
    /// `empty_section_hint`: if `true`, expanded sections with 0 items show
    /// an extra visual "(no changes)" row that has no flat-index entry (TUI
    /// behaviour). Set `false` for the GTK backend which skips that row.
    pub fn sc_visual_row_to_flat(
        &self,
        visual_row: usize,
        empty_section_hint: bool,
    ) -> Option<(usize, bool)> {
        if visual_row < 3 {
            // Rows 0 (header), 1 (commit input), 2 (button row) are not selectable.
            return None;
        }
        let staged_count = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.staged.is_some())
            .count();
        let unstaged_count = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.unstaged.is_some())
            .count();
        let wt_count = self.sc_worktrees.len();
        // Only show the WORKTREES section when there are linked worktrees.
        let three_counts = [staged_count, unstaged_count, wt_count];
        let two_counts = [staged_count, unstaged_count];
        let counts: &[usize] = if wt_count > 1 {
            &three_counts
        } else {
            &two_counts
        };

        let mut row = 3usize; // sections start after header + commit + button rows
        let mut flat = 0usize;

        for (sec, &count) in counts.iter().enumerate() {
            // Section header row
            if row == visual_row {
                return Some((flat, true));
            }
            row += 1;
            flat += 1;

            if self.sc_sections_expanded[sec] {
                if count == 0 && empty_section_hint {
                    // TUI renders a "(no changes)" hint row; skip it without
                    // advancing the flat index.
                    row += 1;
                } else {
                    for _ in 0..count {
                        if row == visual_row {
                            return Some((flat, false));
                        }
                        row += 1;
                        flat += 1;
                    }
                }
            }
        }
        // Log section (always present, section index 3)
        let log_count = self.sc_log.len();
        if row == visual_row {
            return Some((flat, true)); // log header
        }
        row += 1;
        flat += 1;
        if self.sc_sections_expanded[3] {
            if log_count == 0 && empty_section_hint {
                row += 1;
            } else {
                for _ in 0..log_count {
                    if row == visual_row {
                        return Some((flat, false));
                    }
                    row += 1;
                    flat += 1;
                }
            }
        }
        let _ = row; // used for loop tracking only
        None
    }

    /// Map a flat index to (section_idx, item_idx_within_section).
    /// Sections: 0=staged, 1=unstaged, 2=worktrees (optional), 3=log.
    /// Header rows are represented as item_idx = usize::MAX.
    pub fn sc_flat_to_section_idx(&self, flat: usize) -> (usize, usize) {
        let staged_count = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.staged.is_some())
            .count();
        let unstaged_count = self
            .sc_file_statuses
            .iter()
            .filter(|f| f.unstaged.is_some())
            .count();
        let wt_count = self.sc_worktrees.len();
        let show_worktrees = wt_count > 1;

        let mut pos = 0usize;
        // Staged section header
        if flat == pos {
            return (0, usize::MAX);
        }
        pos += 1;
        if self.sc_sections_expanded[0] {
            if flat < pos + staged_count {
                return (0, flat - pos);
            }
            pos += staged_count;
        }
        // Unstaged section header
        if flat == pos {
            return (1, usize::MAX);
        }
        pos += 1;
        if self.sc_sections_expanded[1] {
            if flat < pos + unstaged_count {
                return (1, flat - pos);
            }
            pos += unstaged_count;
        }
        // Worktrees section (only when there are linked worktrees)
        if show_worktrees {
            if flat == pos {
                return (2, usize::MAX);
            }
            pos += 1;
            if self.sc_sections_expanded[2] {
                if flat < pos + wt_count {
                    return (2, flat - pos);
                }
                pos += wt_count;
            }
        }
        // Log section (always present)
        let log_count = self.sc_log.len();
        if flat == pos {
            return (3, usize::MAX);
        }
        pos += 1;
        if self.sc_sections_expanded[3] && flat < pos + log_count {
            return (3, flat - pos);
        }
        (3, usize::MAX) // fallback
    }
}
