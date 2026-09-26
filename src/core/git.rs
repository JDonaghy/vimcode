use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLineStatus {
    Added,
    Modified,
    /// Lines were deleted at this position (shown as ▾ in gutter).
    Deleted,
}

// ─── Source Control: file status ──────────────────────────────────────────────

/// Change kind for a single file in a git status report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    /// Merge conflict — git reports the path as *unmerged*. Never appears
    /// in [`FileStatus::staged`] / [`FileStatus::unstaged`]: a conflicted
    /// path is carried by [`FileStatus::unmerged`] instead (see #991), and
    /// this variant exists so the Merge Changes rows have a label char
    /// from the same single source of truth as every other row.
    Unmerged,
}

impl StatusKind {
    /// Single-character label used in the SC panel.
    pub fn label(self) -> char {
        match self {
            StatusKind::Added => 'A',
            StatusKind::Modified => 'M',
            StatusKind::Deleted => 'D',
            StatusKind::Renamed => 'R',
            StatusKind::Copied => 'C',
            // VS Code's explorer/SC badge for an untracked file. Git's own
            // porcelain notation uses `?` here (see `parse_status_char`
            // below, which stays `?` — it parses `git status --porcelain`
            // output, a wire format this display label must not leak into
            // and must not be confused with) (#1051).
            StatusKind::Untracked => 'U',
            // VS Code's conflict marker.
            StatusKind::Unmerged => '!',
        }
    }

    /// Human-readable name used in panel hovers.
    pub fn description(self) -> &'static str {
        match self {
            StatusKind::Added => "Added",
            StatusKind::Modified => "Modified",
            StatusKind::Deleted => "Deleted",
            StatusKind::Renamed => "Renamed",
            StatusKind::Copied => "Copied",
            StatusKind::Untracked => "Untracked",
            StatusKind::Unmerged => "Conflict",
        }
    }
}

/// Which of git's seven *unmerged* `XY` porcelain codes a conflicted path
/// is in (#991). These cannot be classified one character at a time — `AA`
/// and `DD` are conflicts while a lone `A`/`D` on either side is an
/// ordinary add/delete — so the pair is always classified as a pair by
/// [`UnmergedKind::from_xy`], the single shared entry point used by both
/// [`status_detailed`] (the SC panel) and [`status_text`] (`:Git status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmergedKind {
    /// `DD`
    BothDeleted,
    /// `AU`
    AddedByUs,
    /// `UD`
    DeletedByThem,
    /// `UA`
    AddedByThem,
    /// `DU`
    DeletedByUs,
    /// `AA`
    BothAdded,
    /// `UU`
    BothModified,
}

impl UnmergedKind {
    /// Classify a porcelain `XY` **pair**. Returns `None` for every
    /// non-conflict pair, including a lone `A`/`D` on one side.
    pub fn from_xy(x: char, y: char) -> Option<Self> {
        Some(match (x, y) {
            ('D', 'D') => UnmergedKind::BothDeleted,
            ('A', 'U') => UnmergedKind::AddedByUs,
            ('U', 'D') => UnmergedKind::DeletedByThem,
            ('U', 'A') => UnmergedKind::AddedByThem,
            ('D', 'U') => UnmergedKind::DeletedByUs,
            ('A', 'A') => UnmergedKind::BothAdded,
            ('U', 'U') => UnmergedKind::BothModified,
            _ => return None,
        })
    }

    /// The two-character porcelain code this kind came from.
    pub fn code(self) -> &'static str {
        match self {
            UnmergedKind::BothDeleted => "DD",
            UnmergedKind::AddedByUs => "AU",
            UnmergedKind::DeletedByThem => "UD",
            UnmergedKind::AddedByThem => "UA",
            UnmergedKind::DeletedByUs => "DU",
            UnmergedKind::BothAdded => "AA",
            UnmergedKind::BothModified => "UU",
        }
    }

    /// Git's own wording for the conflict, as `git status` prints it.
    pub fn description(self) -> &'static str {
        match self {
            UnmergedKind::BothDeleted => "both deleted",
            UnmergedKind::AddedByUs => "added by us",
            UnmergedKind::DeletedByThem => "deleted by them",
            UnmergedKind::AddedByThem => "added by them",
            UnmergedKind::DeletedByUs => "deleted by us",
            UnmergedKind::BothAdded => "both added",
            UnmergedKind::BothModified => "both modified",
        }
    }
}

/// The classification of one porcelain `XY` pair (#991).
///
/// Exactly one shape is ever produced: either `unmerged` is `Some` and
/// both `staged`/`unstaged` are `None` (a merge conflict, which belongs to
/// the Merge Changes section alone and must never be swept into a
/// stage-all / discard-all), or `unmerged` is `None` and the two sides
/// carry the ordinary index/worktree kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XyStatus {
    pub staged: Option<StatusKind>,
    pub unstaged: Option<StatusKind>,
    pub unmerged: Option<UnmergedKind>,
}

impl XyStatus {
    /// True when the pair describes no change at all (e.g. `"  "`), i.e.
    /// there is nothing for the SC panel to show.
    pub fn is_empty(self) -> bool {
        self.staged.is_none() && self.unstaged.is_none() && self.unmerged.is_none()
    }
}

/// **The** classifier for a `git status --porcelain` `XY` pair (#991).
///
/// Both the SC panel ([`status_detailed`]) and the `:Git status` buffer
/// ([`status_text`]) route through this one function; the bug this replaced
/// existed twice precisely because each had hand-rolled its own table.
pub fn classify_xy(x: char, y: char) -> XyStatus {
    // A `U` on either side always means "unmerged" per gitstatus(1); the
    // seven named pairs are the only combinations git actually emits, but
    // fall back to "both modified" rather than letting a stray `U` be
    // mislabelled as an ordinary staged/unstaged change.
    if let Some(kind) = UnmergedKind::from_xy(x, y).or(if x == 'U' || y == 'U' {
        Some(UnmergedKind::BothModified)
    } else {
        None
    }) {
        return XyStatus {
            staged: None,
            unstaged: None,
            unmerged: Some(kind),
        };
    }
    XyStatus {
        staged: parse_status_char(x, false),
        unstaged: parse_status_char(y, true),
        unmerged: None,
    }
}

/// Status of a single file from `git status --porcelain`.
#[derive(Debug, Clone, Default)]
pub struct FileStatus {
    pub path: String,
    /// Status in the index (staged area). `None` = unmodified in index.
    pub staged: Option<StatusKind>,
    /// Status in the working tree (unstaged). `None` = unmodified on disk.
    pub unstaged: Option<StatusKind>,
    /// `Some(kind)` when git reports the path as *unmerged* (a merge
    /// conflict). Conflicted paths always carry `staged == unstaged ==
    /// None` so that every `staged.is_some()` / `unstaged.is_some()`
    /// filter in the SC panel excludes them by construction — they belong
    /// to the Merge Changes section only (#991).
    pub unmerged: Option<UnmergedKind>,
}

impl FileStatus {
    /// True when this path has a merge conflict.
    pub fn is_unmerged(&self) -> bool {
        self.unmerged.is_some()
    }
}

// ─── Source Control: worktrees ────────────────────────────────────────────────

/// A single entry from `git worktree list --porcelain`.
#[derive(Debug, Clone)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    /// Branch name, or `None` if detached HEAD.
    pub branch: Option<String>,
    /// True for the main worktree (first entry).
    pub is_main: bool,
    /// True if this is the worktree whose directory contains `dir`.
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    pub header: String,     // full "@@ -a,b +c,d @@" line
    pub lines: Vec<String>, // body lines (with +/-/space prefix)
}

/// Structured diff hunk with line-range mapping to the current buffer.
#[derive(Debug, Clone)]
pub struct DiffHunkInfo {
    pub file_header: String,
    pub hunk: Hunk,
    /// First line in the new (working) file, 0-indexed.
    pub new_start: usize,
    /// Number of new-file lines covered by this hunk.
    pub new_count: usize,
}

/// Create a Command with `CREATE_NO_WINDOW` on Windows to prevent
/// console window flashes in GUI mode.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Create a `git` Command with `CREATE_NO_WINDOW` on Windows to prevent
/// console window flashes in GUI mode.
fn git_command() -> Command {
    hidden_command("git")
}

/// Like [`hidden_command`], but also puts the child in a new process group
/// on Windows (`CREATE_NEW_PROCESS_GROUP`). Used by long-running child
/// processes (LSP/DAP servers) that need to be signaled as a group without
/// affecting our own console, in addition to not flashing a console window.
pub fn hidden_command_new_process_group(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x00000200 | 0x08000000); // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    }
    cmd
}

fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    git_command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let dir = if start.is_file() {
        start.parent()?
    } else {
        start
    };
    run_git(dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))
}

pub fn current_branch(dir: &Path) -> Option<String> {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
}

pub fn file_diff_text(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let text = run_git(dir, &["diff", "HEAD", "--", path.to_str()?])?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

// ─── Git status ───────────────────────────────────────────────────────────────

/// A single entry from `git status --porcelain`.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    /// Two-character XY status code (e.g. "M ", " M", "??").
    pub xy: String,
    /// File path relative to repo root.
    pub path: String,
}

/// Run `git status --porcelain` and return structured entries.
/// Returns an empty vec when not in a git repo or on error.
pub fn status(dir: &Path) -> Vec<StatusEntry> {
    let output = match run_git(dir, &["status", "--porcelain"]) {
        Some(o) => o,
        None => return vec![],
    };
    output
        .lines()
        .filter(|l| l.len() >= 4)
        .map(|l| StatusEntry {
            xy: l[..2].to_string(),
            path: l[3..].to_string(),
        })
        .collect()
}

/// Format git status for display in a buffer (similar to `git status` output).
pub fn status_text(dir: &Path) -> Option<String> {
    let branch = current_branch(dir)
        .map(|b| format!("On branch {}", b))
        .unwrap_or_else(|| "HEAD detached".to_string());

    let entries = status(dir);
    if entries.is_empty() {
        return Some(format!(
            "{}\n\nnothing to commit, working tree clean\n",
            branch
        ));
    }

    // #991: classify through the *shared* `classify_xy` rather than a
    // second hand-rolled table. The old per-character filters here put
    // `AA`/`DD`/`AU`/`DU` under "Changes to be committed" and dropped `UU`
    // from every section — the same bug the SC panel had, twice over.
    let mut staged: Vec<(&StatusEntry, StatusKind)> = Vec::new();
    let mut unstaged: Vec<(&StatusEntry, StatusKind)> = Vec::new();
    let mut untracked: Vec<&StatusEntry> = Vec::new();
    let mut unmerged: Vec<(&StatusEntry, UnmergedKind)> = Vec::new();
    for e in &entries {
        let mut ch = e.xy.chars();
        let x = ch.next().unwrap_or(' ');
        let y = ch.next().unwrap_or(' ');
        let cls = classify_xy(x, y);
        if let Some(kind) = cls.unmerged {
            unmerged.push((e, kind));
            continue;
        }
        if let Some(kind) = cls.staged {
            staged.push((e, kind));
        }
        match cls.unstaged {
            Some(StatusKind::Untracked) => untracked.push(e),
            Some(kind) => unstaged.push((e, kind)),
            None => {}
        }
    }

    // Deduplicate entries that appear in both staged and unstaged
    staged.dedup_by_key(|(e, _)| e.path.clone());
    unstaged.dedup_by_key(|(e, _)| e.path.clone());

    let mut out = format!("{}\n\n", branch);

    if !unmerged.is_empty() {
        out.push_str("You have unmerged paths.\n");
        out.push_str("  (fix conflicts and run \"git commit\")\n\n");
        out.push_str("Unmerged paths:\n");
        for (e, kind) in &unmerged {
            out.push_str(&format!("        {}: {}\n", kind.description(), e.path));
        }
        out.push('\n');
    }

    if !staged.is_empty() {
        out.push_str("Changes to be committed:\n");
        for (e, kind) in &staged {
            let label = match kind {
                StatusKind::Modified => "modified",
                StatusKind::Added => "new file",
                StatusKind::Deleted => "deleted",
                StatusKind::Renamed => "renamed",
                StatusKind::Copied => "copied",
                _ => "changed",
            };
            out.push_str(&format!("        {}: {}\n", label, e.path));
        }
        out.push('\n');
    }

    if !unstaged.is_empty() {
        out.push_str("Changes not staged for commit:\n");
        for (e, kind) in &unstaged {
            let label = match kind {
                StatusKind::Modified => "modified",
                StatusKind::Deleted => "deleted",
                _ => "changed",
            };
            out.push_str(&format!("        {}: {}\n", label, e.path));
        }
        out.push('\n');
    }

    if !untracked.is_empty() {
        out.push_str("Untracked files:\n");
        for e in &untracked {
            out.push_str(&format!("        {}\n", e.path));
        }
        out.push('\n');
    }

    Some(out)
}

// ─── Staging ──────────────────────────────────────────────────────────────────

/// Run `git add <path>`. Returns Ok(()) on success, Err(message) on failure.
pub fn stage_file(path: &Path) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent directory")?;
    let path_str = path.to_str().ok_or("invalid path")?;
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(["add", path_str])
        .output()
        .map_err(|e| format!("git add failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git add failed".to_string()
        } else {
            err
        })
    }
}

/// Run `git add -A` to stage all changes. Returns Ok(()) on success.
pub fn stage_all(dir: &Path) -> Result<(), String> {
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(["add", "-A"])
        .output()
        .map_err(|e| format!("git add -A failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git add -A failed".to_string()
        } else {
            err
        })
    }
}

/// Parse a unified diff string into a file header and a list of hunks.
pub fn parse_diff_hunks(diff: &str) -> (String, Vec<Hunk>) {
    let mut file_header = String::new();
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
            hunks.push(Hunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
        } else if !in_hunk {
            if !file_header.is_empty() {
                file_header.push('\n');
            }
            file_header.push_str(line);
        } else if let Some(last) = hunks.last_mut() {
            last.lines.push(line.to_string());
        }
    }
    (file_header, hunks)
}

/// Run a git command with stdin input, returning stdout or an error string.
fn run_git_stdin(dir: &Path, args: &[&str], input: &str) -> Result<String, String> {
    let mut child = git_command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git spawn failed: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("stdin write: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("git wait: {e}"))?;
    if out.status.success() {
        String::from_utf8(out.stdout).map_err(|e| format!("utf8: {e}"))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Stage a single hunk by piping a minimal patch to `git apply --cached`.
pub fn stage_hunk(dir: &Path, file_header: &str, hunk: &Hunk) -> Result<(), String> {
    let mut patch = String::new();
    patch.push_str(file_header);
    patch.push('\n');
    patch.push_str(&hunk.header);
    patch.push('\n');
    for line in &hunk.lines {
        patch.push_str(line);
        patch.push('\n');
    }
    run_git_stdin(dir, &["apply", "--cached", "-"], &patch).map(|_| ())
}

/// Revert a single hunk by applying the patch in reverse to the working tree.
pub fn revert_hunk(dir: &Path, file_header: &str, hunk: &Hunk) -> Result<(), String> {
    let mut patch = String::new();
    patch.push_str(file_header);
    patch.push('\n');
    patch.push_str(&hunk.header);
    patch.push('\n');
    for line in &hunk.lines {
        patch.push_str(line);
        patch.push('\n');
    }
    // `--index` applies the reverse patch to BOTH the working tree
    // and the index, so reverting a previously-staged hunk discards
    // it everywhere instead of leaving the staged copy behind. If
    // the hunk wasn't staged, --index degrades gracefully (the
    // index simply matches working tree before + after).
    run_git_stdin(dir, &["apply", "--reverse", "--index", "-"], &patch).map(|_| ())
}

// ─── Commit / push ────────────────────────────────────────────────────────────

/// Run `git commit -m <message>`. Returns Ok(summary) or Err(message).
pub fn commit(dir: &Path, message: &str) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("commit message cannot be empty".to_string());
    }
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(["commit", "-m", message])
        .output()
        .map_err(|e| format!("git commit failed: {}", e))?;
    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Return first line of git commit output (e.g. "[main abc1234] fix bug")
        Ok(out.lines().next().unwrap_or("committed").to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git commit failed".to_string()
        } else {
            err
        })
    }
}

/// Run a git remote command (push/pull/fetch) with SSH passphrase handling.
///
/// Uses `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force` to prevent SSH from
/// prompting on the parent terminal.  When `passphrase` is `Some`, an
/// ephemeral askpass helper echoes it; when `None`, the askpass helper
/// prints an empty line (handles keys with empty passphrases or keys
/// already loaded in ssh-agent).
///
/// # Windows leg (#1105)
///
/// On Windows this writes a `.bat` helper instead of a `#!/bin/sh` script.
/// Investigation for #1105 found:
///   - Git for Windows' own `git.exe` (native MinGW build) has its own
///     shebang-parsing spawn layer (`compat/mingw.c`'s `parse_interpreter` /
///     `try_shell_exec`) — but that layer only kicks in for programs *git
///     itself* spawns (editor, pager, `GIT_ASKPASS`/`core.askpass`).
///   - `SSH_ASKPASS` is instead invoked by whichever `ssh` binary git's
///     transport resolves — normally Git for Windows' own bundled
///     MSYS2-built `ssh.exe`, whose POSIX layer (derived from Cygwin, whose
///     user guide documents `#!`-prefixed files as recognized-executable)
///     likely *would* run a shebang script correctly. But if `ssh` instead
///     resolves to Windows' native OpenSSH client (`System32\OpenSSH\ssh.exe`,
///     a plain Win32 build with no shebang support), a `#!/bin/sh` file has
///     no interpreter to run it and the askpass helper silently fails,
///     leaving the fetch/push blocked on a prompt nobody can see — exactly
///     the bug this issue reports. A `.bat` file sidesteps the ambiguity
///     entirely: Windows dispatches `.bat` through `cmd.exe` regardless of
///     which `ssh.exe` (MSYS or native Win32) ends up invoking it, and both
///     the MSVC CRT spawn/exec family and Cygwin/MSYS's own exec layer are
///     documented to recognize and dispatch `.bat`/`.cmd` targets that way.
///   - This could not be exercised on an actual Windows host in this
///     session (no Windows machine available); the `.bat` leg is the
///     verifiable-by-construction choice rather than a bet on which `ssh`
///     a given install resolves.
///
/// ## Passphrase delivery differs per leg (review fix, #1105)
///
/// The POSIX leg passes the passphrase through the `VIMCODE_ASKPASS_PHRASE`
/// env var and echoes it with `echo "$VIMCODE_ASKPASS_PHRASE"` — safe,
/// because the shell expands a quoted `"$VAR"` to a single literal argument
/// with no re-parsing.
///
/// The `.bat` leg does **not** use `%VAR%` expansion for the passphrase,
/// because cmd.exe expands `%VAR%` while it is still scanning the line for
/// `&`/`|`/`<`/`>`/`^`, so a passphrase containing any of those characters
/// would be spliced into the batch file as a second command instead of
/// printed literally — and an empty passphrase collapses `echo ` (no
/// argument) into cmd printing its own echo-toggle state (`ECHO is off.`)
/// instead of a blank line. Both are real, deterministic bugs, not edge
/// cases: the first fires on every passphrase containing a cmd.exe
/// metacharacter, the second on every no-passphrase / agent-loaded-key
/// push. Instead, the Windows leg writes the passphrase to a sibling
/// "phrase file" and has the `.bat` stream it with `type "<path>"`. `type`
/// copies the file's bytes to stdout with no command-line re-parsing, so it
/// has neither failure mode — the empty-passphrase file just produces an
/// empty (newline-only) line, and any byte sequence in the passphrase file
/// is passed through unparsed.
fn run_git_remote(
    dir: &Path,
    args: &[&str],
    label: &str,
    passphrase: Option<&str>,
) -> Result<String, String> {
    // Build an ephemeral askpass helper. On Windows the passphrase is
    // delivered via a sibling "phrase file" streamed with `type` (see the
    // doc comment above for why `%VAR%` expansion isn't safe there); on
    // POSIX it's delivered via the VIMCODE_ASKPASS_PHRASE env var, which the
    // shell script echoes back quoted.
    let phrase = passphrase.unwrap_or("");
    let askpass_dir = std::env::temp_dir();
    let pid = std::process::id();
    #[cfg(windows)]
    let askpass_path = askpass_dir.join(format!("vimcode_askpass_{}.bat", pid));
    #[cfg(not(windows))]
    let askpass_path = askpass_dir.join(format!("vimcode_askpass_{}", pid));

    #[cfg(windows)]
    let phrase_path = askpass_dir.join(format!("vimcode_askpass_phrase_{}.txt", pid));
    #[cfg(windows)]
    {
        std::fs::write(&phrase_path, windows_askpass_phrase_file_contents(phrase))
            .map_err(|e| format!("{} failed: cannot create askpass phrase file: {}", label, e))?;
    }

    #[cfg(windows)]
    let script = windows_askpass_script(&phrase_path);
    #[cfg(not(windows))]
    let script = "#!/bin/sh\necho \"$VIMCODE_ASKPASS_PHRASE\"\n".to_string();

    std::fs::write(&askpass_path, script)
        .map_err(|e| format!("{} failed: cannot create askpass helper: {}", label, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&askpass_path, std::fs::Permissions::from_mode(0o700)).ok();
    }

    let mut command = git_command();
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("SSH_ASKPASS", &askpass_path)
        .env("SSH_ASKPASS_REQUIRE", "force")
        // DISPLAY must be set for SSH_ASKPASS to work on some systems.
        .env("DISPLAY", std::env::var("DISPLAY").unwrap_or_default());
    #[cfg(not(windows))]
    command.env("VIMCODE_ASKPASS_PHRASE", phrase);

    let output = command
        .output()
        .map_err(|e| format!("{} failed: {}", label, e));

    // Clean up the askpass script (and, on Windows, the phrase file).
    let _ = std::fs::remove_file(&askpass_path);
    #[cfg(windows)]
    let _ = std::fs::remove_file(&phrase_path);

    let output = output?;
    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let err_out = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok(if out.is_empty() { err_out } else { out })
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("{} failed", label)
        } else {
            err
        })
    }
}

/// Build the contents of the Windows askpass "phrase file" (#1105): the
/// passphrase plus a trailing newline, matching the POSIX leg's
/// `echo "$VIMCODE_ASKPASS_PHRASE"` output (which always terminates with a
/// newline, including when the phrase is empty). Kept as a standalone,
/// platform-independent function — no `cfg(windows)` gate — so the exact
/// bytes SSH will receive can be unit-tested without a Windows host.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_askpass_phrase_file_contents(phrase: &str) -> String {
    let mut contents = phrase.to_string();
    contents.push('\n');
    contents
}

/// Build the Windows askpass `.bat` body. It streams `phrase_path`'s bytes
/// via `type` rather than interpolating the passphrase into the script
/// through `%VAR%` expansion — see the doc comment on `run_git_remote` for
/// why that's unsafe (metacharacter injection, and a mis-rendered empty
/// line). The passphrase itself must never appear in this string; platform-
/// independent — no `cfg(windows)` gate — so that invariant is
/// unit-testable without a Windows host.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_askpass_script(phrase_path: &Path) -> String {
    format!("@echo off\r\ntype \"{}\"\r\n", phrase_path.display())
}

/// Returns `true` when the error message looks like an SSH authentication
/// failure that might be resolved by providing a passphrase.
pub fn is_ssh_auth_error(err: &str) -> bool {
    let low = err.to_lowercase();
    low.contains("permission denied")
        || low.contains("passphrase")
        || low.contains("authentication failed")
        || low.contains("could not read from remote")
        || low.contains("host key verification failed")
}

/// Run `git push`. Returns Ok(summary) or Err(message).
pub fn push(dir: &Path) -> Result<String, String> {
    run_git_remote(dir, &["push"], "git push", None)
}

/// Run `git push` with an explicit SSH passphrase.
pub fn push_with_passphrase(dir: &Path, passphrase: &str) -> Result<String, String> {
    run_git_remote(dir, &["push"], "git push", Some(passphrase))
}

/// Run `git pull`. Returns Ok(summary) or Err(message).
pub fn pull(dir: &Path) -> Result<String, String> {
    run_git_remote(dir, &["pull"], "git pull", None)
}

/// Run `git pull` with an explicit SSH passphrase.
pub fn pull_with_passphrase(dir: &Path, passphrase: &str) -> Result<String, String> {
    run_git_remote(dir, &["pull"], "git pull", Some(passphrase))
}

/// Run `git fetch`. Returns Ok(summary) or Err(message).
pub fn fetch(dir: &Path) -> Result<String, String> {
    run_git_remote(dir, &["fetch"], "git fetch", None)
}

/// Run `git fetch` with an explicit SSH passphrase.
pub fn fetch_with_passphrase(dir: &Path, passphrase: &str) -> Result<String, String> {
    run_git_remote(dir, &["fetch"], "git fetch", Some(passphrase))
}

/// Unstage all staged files (`git restore --staged .`).
#[allow(dead_code)]
pub fn unstage_all(dir: &Path) -> Result<(), String> {
    run_git_result(dir, &["restore", "--staged", "."])
}

// #991: the path-spec-wide `discard_all` (`git restore .`) was removed —
// it blew away the half-merged content of conflicted files along with the
// ordinary changes. `sc_discard_all_unstaged` now passes an explicit,
// conflict-free path list to [`discard_paths`].

// ─── Blame ────────────────────────────────────────────────────────────────────

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Convert a Unix timestamp (seconds since epoch) to a "YYYY-MM-DD" string (UTC).
fn epoch_to_date(secs: i64) -> String {
    let mut remaining = (secs / 86400) as i32;
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let month_days: [i32; 12] = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1i32;
    for &m in &month_days {
        if remaining < m {
            break;
        }
        remaining -= m;
        month += 1;
    }
    format!("{:04}-{:02}-{:02}", year, month, remaining + 1)
}

/// Format a Unix epoch timestamp as a human-readable absolute date string,
/// e.g. "March 21, 2026 4:30 PM" (GitLens-style).
/// `tz_offset_secs` is the author timezone offset in seconds east of UTC.
pub fn epoch_to_absolute(secs: i64, tz_offset_secs: i32) -> String {
    let secs = secs + tz_offset_secs as i64;
    // Date portion (reuse epoch_to_date logic)
    let mut remaining_days = (secs / 86400) as i32;
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let month_days: [i32; 12] = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    for &m in &month_days {
        if remaining_days < m {
            break;
        }
        remaining_days -= m;
        month += 1;
    }
    let day = remaining_days + 1;
    let month_names = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month_name = month_names.get(month).unwrap_or(&"???");
    // Time portion
    let time_of_day = secs.rem_euclid(86400);
    let hour24 = (time_of_day / 3600) as u32;
    let minute = ((time_of_day % 3600) / 60) as u32;
    let (hour12, ampm) = if hour24 == 0 {
        (12, "AM")
    } else if hour24 < 12 {
        (hour24, "AM")
    } else if hour24 == 12 {
        (12, "PM")
    } else {
        (hour24 - 12, "PM")
    };
    format!(
        "{} {}, {} {}:{:02} {}",
        month_name, day, year, hour12, minute, ampm
    )
}

/// Parse `git blame --porcelain` output into a human-readable per-line format.
///
/// Each output line has the form:
/// `<short-hash> (<author padded to 12> <YYYY-MM-DD>  <lineno>) <source content>`
fn parse_blame_porcelain(text: &str) -> String {
    // commit hash -> (author, date) — cached so repeated commits keep their info
    let mut commit_info: HashMap<String, (String, String)> = HashMap::new();
    let mut current_hash = String::new();
    let mut current_author = String::new();
    let mut current_date = String::new();
    let mut current_lineno: usize = 0;
    let mut out = String::new();

    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            // Source-content line — emit one formatted blame annotation.
            let (author, date) = commit_info
                .get(&current_hash)
                .map(|(a, d)| (a.as_str(), d.as_str()))
                .unwrap_or(("?", "????-??-??"));
            let short: String = current_hash.chars().take(8).collect();
            let author_col: String = author.chars().take(12).collect();
            out.push_str(&format!(
                "{} ({:<12} {}  {:>4}) {}\n",
                short, author_col, date, current_lineno, content
            ));
        } else if let Some(name) = line.strip_prefix("author ") {
            current_author = name.to_string();
        } else if let Some(ts) = line.strip_prefix("author-time ") {
            if let Ok(epoch) = ts.trim().parse::<i64>() {
                current_date = epoch_to_date(epoch);
            }
            // Register once we have both author and time (author line always precedes author-time).
            commit_info.insert(
                current_hash.clone(),
                (current_author.clone(), current_date.clone()),
            );
        } else {
            // Detect a blame header line: 40 hex chars, then a space, then numbers.
            let bytes = line.as_bytes();
            if bytes.len() >= 42
                && bytes[40] == b' '
                && bytes[..40].iter().all(|&b| b.is_ascii_hexdigit())
            {
                current_hash = line[..40].to_string();
                // The final line number is the 3rd space-separated field (index 2).
                let parts: Vec<&str> = line.splitn(5, ' ').collect();
                current_lineno = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                // Restore cached author/date for commits we've already seen.
                if let Some((a, d)) = commit_info.get(&current_hash) {
                    current_author = a.clone();
                    current_date = d.clone();
                }
            }
        }
    }

    out
}

/// Run `git blame --porcelain` on `path` and return a formatted blame buffer.
/// Returns `None` when the file is untracked, the repo has no commits, or
/// any other `git blame` failure occurs.
pub fn blame_text(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let path_str = path.to_str()?;
    let raw = run_git(dir, &["blame", "--porcelain", path_str])?;
    if raw.trim().is_empty() {
        return None;
    }
    Some(parse_blame_porcelain(&raw))
}

// ─── Diff ─────────────────────────────────────────────────────────────────────

/// Parse the `+start,count` portion of a `@@ ... @@` header. Returns 0-indexed start and count.
fn parse_hunk_new_range(header: &str) -> (usize, usize) {
    if let Some(rest) = header.strip_prefix("@@ ") {
        if let Some(plus) = rest.split('+').nth(1) {
            let parts: Vec<&str> = plus.split([',', ' ']).collect();
            let start = parts
                .first()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            let count = parts
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            return (start.saturating_sub(1), count);
        }
    }
    (0, 0)
}

/// Compute structured diff hunks for a file, with line-range info for the working copy.
pub fn compute_file_diff_hunks(path: &Path) -> Vec<DiffHunkInfo> {
    let dir = match path.parent() {
        Some(d) => d,
        None => return vec![],
    };
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return vec![],
    };
    let diff = match run_git(dir, &["diff", "HEAD", "--", path_str]) {
        Some(d) if !d.trim().is_empty() => d,
        _ => return vec![],
    };
    let (file_header, hunks) = parse_diff_hunks(&diff);
    hunks
        .into_iter()
        .map(|hunk| {
            let (new_start, new_count) = parse_hunk_new_range(&hunk.header);
            DiffHunkInfo {
                file_header: file_header.clone(),
                hunk,
                new_start,
                new_count,
            }
        })
        .collect()
}

/// Find which hunk covers a given buffer line (0-indexed). Returns the hunk index.
pub fn hunk_for_line(hunks: &[DiffHunkInfo], line: usize) -> Option<usize> {
    // Exact range match first.
    for (i, h) in hunks.iter().enumerate() {
        let end = h.new_start + h.new_count.max(1);
        if line >= h.new_start && line < end {
            return Some(i);
        }
    }
    // For pure deletions (new_count == 0), check if line is at the boundary.
    for (i, h) in hunks.iter().enumerate() {
        if h.new_count == 0 && (line == h.new_start || (h.new_start > 0 && line == h.new_start - 1))
        {
            return Some(i);
        }
    }
    None
}

pub fn compute_file_diff(path: &Path) -> Vec<Option<GitLineStatus>> {
    let dir = match path.parent() {
        Some(d) => d,
        None => return vec![],
    };
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return vec![],
    };
    let total = std::fs::read_to_string(path)
        .map(|c| c.lines().count().max(1))
        .unwrap_or(0);
    if total == 0 {
        return vec![];
    }

    // Untracked file: all lines Added (only if we're in a git repo at all)
    if run_git(dir, &["ls-files", "--error-unmatch", path_str]).is_none() {
        if find_repo_root(path).is_none() {
            return vec![];
        }
        return vec![Some(GitLineStatus::Added); total];
    }

    let diff = match run_git(dir, &["diff", "HEAD", "--", path_str]) {
        Some(d) if !d.trim().is_empty() => d,
        _ => return vec![],
    };

    parse_unified_diff(&diff, total)
}

fn parse_unified_diff(diff: &str, total_lines: usize) -> Vec<Option<GitLineStatus>> {
    let mut result = vec![None; total_lines];
    let mut new_line: usize = 0;
    let mut pending_del: usize = 0;

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            // Flush pending pure-deletions from previous hunk.
            if pending_del > 0 {
                let mark = new_line.min(total_lines.saturating_sub(1));
                if result[mark].is_none() {
                    result[mark] = Some(GitLineStatus::Deleted);
                }
            }
            // Parse +new_start from "@@ -a,b +new_start,c @@"
            if let Some(plus) = rest.split('+').nth(1) {
                let s = plus.split([',', ' ']).next().unwrap_or("1");
                new_line = s.parse::<usize>().unwrap_or(1).saturating_sub(1);
            }
            pending_del = 0;
        } else if line.starts_with('-') && !line.starts_with("---") {
            pending_del += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            if new_line < total_lines {
                result[new_line] = Some(if pending_del > 0 {
                    pending_del -= 1;
                    GitLineStatus::Modified
                } else {
                    GitLineStatus::Added
                });
            }
            new_line += 1;
        } else if !line.starts_with('\\') {
            // Context line — flush pending pure-deletions.
            if pending_del > 0 && new_line < total_lines && result[new_line].is_none() {
                result[new_line] = Some(GitLineStatus::Deleted);
            }
            pending_del = 0;
            new_line += 1;
        }
    }
    // Trailing pure-deletions at end of diff.
    if pending_del > 0 {
        let mark = new_line.min(total_lines.saturating_sub(1));
        if result[mark].is_none() {
            result[mark] = Some(GitLineStatus::Deleted);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Windows askpass leg (#1105) ─────────────────────────────────────────
    // These exercise the pure string-building helpers directly so the two
    // review-flagged bugs (empty-passphrase mis-render, metacharacter
    // injection via %VAR% expansion) stay caught without needing a Windows
    // host to run the actual .bat file.

    #[test]
    fn windows_askpass_phrase_file_empty_phrase_is_just_a_newline() {
        // The old `echo %VIMCODE_ASKPASS_PHRASE%` leg rendered an empty
        // phrase as the literal text "ECHO is off." (cmd.exe's bare-`echo`
        // toggle-state message) instead of a blank line. The phrase-file
        // approach must produce exactly a newline, with no such artifact.
        assert_eq!(windows_askpass_phrase_file_contents(""), "\n");
    }

    #[test]
    fn windows_askpass_phrase_file_preserves_metacharacters_literally() {
        // A passphrase containing cmd.exe metacharacters must survive
        // byte-for-byte in the phrase file — no reinterpretation, since
        // `type` never re-parses file contents as commands.
        let phrase = "abc&whoami|echo^pwned<x>y";
        assert_eq!(
            windows_askpass_phrase_file_contents(phrase),
            format!("{}\n", phrase)
        );
    }

    #[test]
    fn windows_askpass_script_never_embeds_the_passphrase() {
        // The .bat body must only ever reference the phrase file's path —
        // never the passphrase text itself. If a future edit reintroduces
        // `%VAR%`-style interpolation of the phrase into the script, this
        // test catches it.
        let phrase_path = Path::new(r"C:\Temp\vimcode_askpass_phrase_1234.txt");
        let script = windows_askpass_script(phrase_path);
        assert!(script.starts_with("@echo off"));
        assert!(script.contains("type "));
        assert!(script.contains("vimcode_askpass_phrase_1234.txt"));
    }

    // ── parse_diff_hunks ───────────────────────────────────────────────────

    #[test]
    fn test_parse_diff_hunks_empty() {
        let (header, hunks) = parse_diff_hunks("");
        assert!(header.is_empty());
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_parse_diff_hunks_single_hunk() {
        let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,3 +1,4 @@\n line1\n+line2\n line3\n line4\n";
        let (header, hunks) = parse_diff_hunks(diff);
        assert!(header.contains("diff --git"));
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].header, "@@ -1,3 +1,4 @@");
        assert_eq!(hunks[0].lines.len(), 4);
    }

    #[test]
    fn test_parse_diff_hunks_multi_hunk() {
        let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,2 +1,3 @@\n line1\n+added\n line2\n@@ -10,2 +11,2 @@\n lineA\n-lineB\n+lineC\n";
        let (_header, hunks) = parse_diff_hunks(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].header, "@@ -1,2 +1,3 @@");
        assert_eq!(hunks[1].header, "@@ -10,2 +11,2 @@");
        // No cross-contamination
        assert!(hunks[0].lines.iter().all(|l| !l.starts_with("-lineB")));
        assert!(hunks[1].lines.iter().all(|l| !l.starts_with("+added")));
    }

    #[test]
    fn test_parse_diff_hunks_no_file_header() {
        let diff = "@@ -1,2 +1,3 @@\n line1\n+new\n line2\n";
        let (header, hunks) = parse_diff_hunks(diff);
        assert!(header.is_empty());
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines.len(), 3);
    }

    #[test]
    fn test_parse_unified_diff_added_lines() {
        // Simulate adding 2 lines at the start of a 3-line file
        let diff = "@@ -0,0 +1,3 @@\n+line1\n+line2\n+line3\n";
        let result = parse_unified_diff(diff, 3);
        assert_eq!(result[0], Some(GitLineStatus::Added));
        assert_eq!(result[1], Some(GitLineStatus::Added));
        assert_eq!(result[2], Some(GitLineStatus::Added));
    }

    #[test]
    fn test_parse_unified_diff_modified_line() {
        // Simulate modifying line 2 (old line 2 removed, new line 2 added)
        let diff = "@@ -1,3 +1,3 @@\n line1\n-old line2\n+new line2\n line3\n";
        let result = parse_unified_diff(diff, 3);
        assert_eq!(result[0], None);
        assert_eq!(result[1], Some(GitLineStatus::Modified));
        assert_eq!(result[2], None);
    }

    #[test]
    fn test_parse_unified_diff_empty() {
        let result = parse_unified_diff("", 5);
        assert!(result.iter().all(|s| s.is_none()));
    }

    #[test]
    fn test_parse_unified_diff_pure_deletion() {
        // Deleting lines 2-3 from a 5-line file (old) → 3-line file (new)
        // @@ -1,5 +1,3 @$ context\n-del1\n-del2\n context\n context
        let diff = "@@ -1,5 +1,3 @@\n line1\n-del1\n-del2\n line2\n line3\n";
        let result = parse_unified_diff(diff, 3);
        // line 0 (line1) = context → None
        // line 1 (line2) = first context after deletion → Deleted marker
        // line 2 (line3) = context → None
        assert_eq!(result[0], None);
        assert_eq!(result[1], Some(GitLineStatus::Deleted));
        assert_eq!(result[2], None);
    }

    #[test]
    fn test_parse_unified_diff_trailing_deletion() {
        // Delete the last 2 lines. Old: 4 lines → New: 2 lines.
        let diff = "@@ -1,4 +1,2 @@\n line1\n line2\n-del1\n-del2\n";
        let result = parse_unified_diff(diff, 2);
        // line 0 (line1) = context → None
        // line 1 (line2) = last line, trailing deletions → Deleted marker
        assert_eq!(result[0], None);
        assert_eq!(result[1], Some(GitLineStatus::Deleted));
    }

    #[test]
    fn test_parse_unified_diff_deletion_at_start() {
        // Delete first 2 lines. Old: 4 lines → New: 2 lines.
        let diff = "@@ -1,4 +1,2 @@\n-del1\n-del2\n line1\n line2\n";
        let result = parse_unified_diff(diff, 2);
        // line 0 = first line after deletion → Deleted marker
        // line 1 = context → None
        assert_eq!(result[0], Some(GitLineStatus::Deleted));
        assert_eq!(result[1], None);
    }

    // ── hunk_for_line ────────────────────────────────────────────────────

    #[test]
    fn test_hunk_for_line_exact_match() {
        let hunks = vec![
            DiffHunkInfo {
                file_header: String::new(),
                hunk: Hunk {
                    header: "@@ -1,2 +1,3 @@".to_string(),
                    lines: vec![],
                },
                new_start: 0,
                new_count: 3,
            },
            DiffHunkInfo {
                file_header: String::new(),
                hunk: Hunk {
                    header: "@@ -10,2 +11,2 @@".to_string(),
                    lines: vec![],
                },
                new_start: 10,
                new_count: 2,
            },
        ];
        assert_eq!(hunk_for_line(&hunks, 0), Some(0));
        assert_eq!(hunk_for_line(&hunks, 2), Some(0));
        assert_eq!(hunk_for_line(&hunks, 3), None);
        assert_eq!(hunk_for_line(&hunks, 10), Some(1));
        assert_eq!(hunk_for_line(&hunks, 11), Some(1));
        assert_eq!(hunk_for_line(&hunks, 12), None);
    }

    #[test]
    fn test_hunk_for_line_pure_deletion() {
        // Pure deletion: new_count=0 at position 5.
        let hunks = vec![DiffHunkInfo {
            file_header: String::new(),
            hunk: Hunk {
                header: "@@ -5,2 +5,0 @@".to_string(),
                lines: vec![],
            },
            new_start: 5,
            new_count: 0,
        }];
        // Should match at the boundary lines.
        assert_eq!(hunk_for_line(&hunks, 4), Some(0));
        assert_eq!(hunk_for_line(&hunks, 5), Some(0));
        assert_eq!(hunk_for_line(&hunks, 3), None);
        assert_eq!(hunk_for_line(&hunks, 6), None);
    }

    // ── parse_hunk_new_range ────────────────────────────────────────────

    #[test]
    fn test_parse_hunk_new_range() {
        assert_eq!(parse_hunk_new_range("@@ -1,3 +1,4 @@"), (0, 4));
        assert_eq!(parse_hunk_new_range("@@ -10,2 +11,2 @@"), (10, 2));
        assert_eq!(parse_hunk_new_range("@@ -5,2 +5,0 @@"), (4, 0));
        assert_eq!(parse_hunk_new_range("@@ -1 +1 @@"), (0, 1));
    }

    // ── blame helpers ──────────────────────────────────────────────────────

    #[test]
    fn test_epoch_to_date_epoch_zero() {
        assert_eq!(epoch_to_date(0), "1970-01-01");
    }

    #[test]
    fn test_epoch_to_date_one_day() {
        assert_eq!(epoch_to_date(86400), "1970-01-02");
    }

    #[test]
    fn test_parse_blame_porcelain_single_line() {
        let hash = "a".repeat(40);
        let input = format!(
            "{h} 1 1 1\nauthor Alice\nauthor-mail <alice@example.com>\n\
             author-time 0\nauthor-tz +0000\ncommitter Alice\n\
             committer-mail <alice@example.com>\ncommitter-time 0\n\
             committer-tz +0000\nsummary Initial commit\nfilename src/main.rs\n\
             \tfn main() {{}}\n",
            h = hash
        );
        let result = parse_blame_porcelain(&input);
        assert!(result.contains("aaaaaaaa"), "should contain short hash");
        assert!(result.contains("Alice"), "should contain author");
        assert!(result.contains("1970-01-01"), "should contain date");
        assert!(
            result.contains("fn main()"),
            "should contain source content"
        );
        assert_eq!(
            result.lines().count(),
            1,
            "should produce exactly 1 output line"
        );
    }

    #[test]
    fn test_parse_blame_porcelain_repeated_commit() {
        let hash = "b".repeat(40);
        let input = format!(
            "{h} 1 1 2\nauthor Bob\nauthor-mail <bob@example.com>\n\
             author-time 86400\nauthor-tz +0000\ncommitter Bob\n\
             committer-mail <bob@example.com>\ncommitter-time 86400\n\
             committer-tz +0000\nsummary fix\nfilename f.rs\n\tline one\n\
             {h} 2 2\nfilename f.rs\n\tline two\n",
            h = hash
        );
        let result = parse_blame_porcelain(&input);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "should produce 2 output lines");
        assert!(
            lines[0].contains("line one"),
            "first line should contain content"
        );
        assert!(
            lines[1].contains("line two"),
            "second line should contain content"
        );
        assert!(lines[0].contains("Bob"), "first line should have author");
        assert!(
            lines[1].contains("Bob"),
            "second line should have author (cached)"
        );
    }

    #[test]
    fn test_parse_blame_porcelain_empty() {
        assert!(parse_blame_porcelain("").is_empty());
    }

    // ── show_file_at_ref ─────────────────────────────────────────────────

    #[test]
    fn test_show_file_at_ref_returns_head_content() {
        use std::process::Command;
        let dir = std::env::temp_dir().join("vimcode_show_ref_head");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_command()
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.email", "t@t.com"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.name", "T"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let file = dir.join("test.txt");
        std::fs::write(&file, "original\n").unwrap();
        git_command()
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["commit", "-m", "init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        // Modify working copy
        std::fs::write(&file, "modified\n").unwrap();

        let content = show_file_at_ref(&dir, "HEAD", "test.txt");
        assert!(content.is_some(), "should return HEAD content");
        assert_eq!(content.unwrap().trim(), "original");
    }

    #[test]
    fn test_show_file_at_ref_nonexistent() {
        use std::process::Command;
        let dir = std::env::temp_dir().join("vimcode_show_ref_nofile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_command()
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.email", "t@t.com"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.name", "T"])
            .current_dir(&dir)
            .output()
            .unwrap();
        // Create an empty commit so HEAD exists
        git_command()
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(&dir)
            .output()
            .unwrap();

        let content = show_file_at_ref(&dir, "HEAD", "doesnotexist.txt");
        assert!(content.is_none(), "should return None for nonexistent file");
    }

    #[test]
    fn test_checkout_branch_and_create_branch() {
        use std::process::Command;
        let dir = std::env::temp_dir().join("vimcode_branch_ops");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_command()
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.name", "Test"])
            .current_dir(&dir)
            .output()
            .unwrap();
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        git_command()
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["commit", "-m", "init"])
            .current_dir(&dir)
            .output()
            .unwrap();

        // Create a new branch
        assert!(create_branch(&dir, "feature-x").is_ok());
        assert_eq!(current_branch(&dir).as_deref(), Some("feature-x"));

        // Switch back to main/master
        let main = if checkout_branch(&dir, "main").is_ok() {
            "main"
        } else {
            checkout_branch(&dir, "master").unwrap();
            "master"
        };
        assert_eq!(current_branch(&dir).as_deref(), Some(main));

        // Switch to feature-x again
        assert!(checkout_branch(&dir, "feature-x").is_ok());
        assert_eq!(current_branch(&dir).as_deref(), Some("feature-x"));
    }

    // ── changed_files_between ────────────────────────────────────────────

    #[test]
    fn test_changed_files_between_lists_files_changed_on_head_since_base() {
        let dir = std::env::temp_dir().join("vimcode_changed_files_between");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_command()
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.email", "t@t.com"])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["config", "user.name", "T"])
            .current_dir(&dir)
            .output()
            .unwrap();
        std::fs::write(dir.join("unchanged.txt"), "same\n").unwrap();
        std::fs::write(dir.join("modified.txt"), "before\n").unwrap();
        git_command()
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["commit", "-m", "base"])
            .current_dir(&dir)
            .output()
            .unwrap();
        create_branch(&dir, "feature").unwrap();
        std::fs::write(dir.join("modified.txt"), "after\n").unwrap();
        std::fs::write(dir.join("new.txt"), "brand new\n").unwrap();
        git_command()
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["commit", "-m", "feature work"])
            .current_dir(&dir)
            .output()
            .unwrap();
        // A commit on `main`/`master` after the branch diverged must not
        // show up as a "changed" file on `feature` — three-dot semantics.
        checkout_branch(&dir, "main")
            .or_else(|_| checkout_branch(&dir, "master"))
            .unwrap();
        std::fs::write(dir.join("unchanged.txt"), "changed on base only\n").unwrap();
        git_command()
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .unwrap();
        git_command()
            .args(["commit", "-m", "base-only change"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let base = current_branch(&dir).unwrap();

        let mut files = changed_files_between(&dir, &base, "feature").expect("valid refs diff");
        files.sort();
        assert_eq!(
            files,
            vec!["modified.txt".to_string(), "new.txt".to_string()]
        );
    }

    #[test]
    fn test_changed_files_between_unknown_ref_is_none_not_an_empty_diff() {
        let dir = std::env::temp_dir().join("vimcode_changed_files_between_bad_ref");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_command()
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(
            changed_files_between(&dir, "nope-base", "nope-head").is_none(),
            "an unresolvable ref is a git failure, distinct from a real empty diff"
        );
    }

    /// Review non-blocking finding (#525): `base`/`head` come straight from
    /// an external provider's JSON with no shell involved, but a value
    /// starting with `-` could still be parsed by git as a flag rather than
    /// a revision when joined into `base...head`. Guard rejects it before
    /// it ever reaches git's argv.
    #[test]
    fn test_changed_files_between_rejects_flag_like_revisions() {
        let dir = std::env::temp_dir().join("vimcode_changed_files_between_flag_smuggle");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git_command()
            .args(["init"])
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(changed_files_between(&dir, "--output=/tmp/pwned", "HEAD").is_none());
        assert!(changed_files_between(&dir, "HEAD", "--output=/tmp/pwned").is_none());
    }

    // ── normalize_remote_url ────────────────────────────────────────────────

    #[test]
    fn test_normalize_https_url() {
        assert_eq!(
            normalize_remote_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo"
        );
        assert_eq!(
            normalize_remote_url("https://github.com/user/repo"),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_normalize_ssh_shorthand() {
        assert_eq!(
            normalize_remote_url("git@github.com:user/repo.git"),
            "https://github.com/user/repo"
        );
        assert_eq!(
            normalize_remote_url("git@gitlab.com:org/project.git"),
            "https://gitlab.com/org/project"
        );
    }

    #[test]
    fn test_normalize_ssh_scheme() {
        assert_eq!(
            normalize_remote_url("ssh://git@github.com/user/repo.git"),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_epoch_to_absolute() {
        // 1707426480 = 2024-02-08 21:08:00 UTC
        let s = super::epoch_to_absolute(1707426480, 0);
        assert!(s.contains("February"), "got: {}", s);
        assert!(s.contains("2024"), "got: {}", s);
        assert!(s.contains("9:08 PM"), "got: {}", s);
        // With timezone offset +0100 (3600s), should shift by 1 hour → 10:08 PM
        let s2 = super::epoch_to_absolute(1707426480, 3600);
        assert!(s2.contains("10:08 PM"), "got: {}", s2);
    }
}

// ─── Source Control helpers ───────────────────────────────────────────────────

/// Run `git status --porcelain` and return parsed `FileStatus` entries.
pub fn status_detailed(dir: &Path) -> Vec<FileStatus> {
    let output = match run_git(dir, &["status", "--porcelain", "-u"]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    parse_status_porcelain(&output)
}

/// Parse `git status --porcelain` output into [`FileStatus`] entries.
///
/// Split out of [`status_detailed`] (#991) so a test can drive the whole
/// parse→panel pipeline from a literal porcelain string — in particular
/// one case per unmerged `XY` code — without needing seven separate real
/// merge conflicts on disk.
pub fn parse_status_porcelain(output: &str) -> Vec<FileStatus> {
    output
        .lines()
        .filter_map(|line| {
            if line.len() < 3 {
                return None;
            }
            let xy: Vec<char> = line.chars().take(2).collect();
            let x = xy[0]; // index status
            let y = xy[1]; // working-tree status
            let path = line[3..].to_string();

            // Renamed lines have "old -> new" form; keep just the new path.
            let path = if let Some(pos) = path.find(" -> ") {
                path[pos + 4..].to_string()
            } else {
                path
            };

            let cls = classify_xy(x, y);
            if cls.is_empty() {
                return None;
            }
            Some(FileStatus {
                path,
                staged: cls.staged,
                unstaged: cls.unstaged,
                unmerged: cls.unmerged,
            })
        })
        .collect()
}

/// Classify a **single** porcelain status character. Not a valid
/// classifier on its own — the conflict codes (`AA`, `DD`, and anything
/// with a `U`) only mean "conflict" as a pair, so callers must go through
/// [`classify_xy`], which handles those first.
fn parse_status_char(ch: char, is_workdir: bool) -> Option<StatusKind> {
    match ch {
        'A' => Some(StatusKind::Added),
        'M' => Some(StatusKind::Modified),
        'D' => Some(StatusKind::Deleted),
        'R' => Some(StatusKind::Renamed),
        'C' => Some(StatusKind::Copied),
        '?' if is_workdir => Some(StatusKind::Untracked),
        _ => None,
    }
}

/// Stage a single path (equivalent to `git add <path>`).
pub fn stage_path(dir: &Path, path: &str) -> Result<(), String> {
    run_git_result(dir, &["add", "--", path])
}

/// Stage an explicit list of paths (`git add -- <paths…>`).
///
/// #991: the SC panel's "stage all" used to run `git add .`, which sweeps
/// *conflicted* files in too — and `git add` on a conflicted file is how
/// you mark it resolved, so a bulk stage silently resolved every conflict
/// with the markers still in the files. Callers now pass the exact set of
/// non-conflicted paths instead. Empty `paths` is a no-op.
pub fn stage_paths(dir: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["add", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    run_git_result(dir, &args)
}

/// Discard working-tree changes for an explicit list of paths
/// (`git restore -- <paths…>`). Empty `paths` is a no-op. See
/// [`stage_paths`] for why the bulk variants take explicit paths (#991).
pub fn discard_paths(dir: &Path, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["restore", "--"];
    args.extend(paths.iter().map(|s| s.as_str()));
    run_git_result(dir, &args)
}

/// Unstage a single path (equivalent to `git restore --staged <path>`).
pub fn unstage_path(dir: &Path, path: &str) -> Result<(), String> {
    run_git_result(dir, &["restore", "--staged", path])
}

/// Discard working-tree changes for a path (equivalent to `git checkout -- <path>`).
pub fn discard_path(dir: &Path, path: &str) -> Result<(), String> {
    run_git_result(dir, &["checkout", "--", path])
}

/// Switch to an existing branch.
pub fn checkout_branch(dir: &Path, branch: &str) -> Result<(), String> {
    run_git_result(dir, &["switch", branch])
}

/// Create a new branch and switch to it.
pub fn create_branch(dir: &Path, branch: &str) -> Result<(), String> {
    run_git_result(dir, &["switch", "-c", branch])
}

fn run_git_result(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = git_command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Parse `git worktree list --porcelain` output into `WorktreeEntry` vec.
pub fn worktree_list(dir: &Path) -> Vec<WorktreeEntry> {
    let output = match run_git(dir, &["worktree", "list", "--porcelain"]) {
        Some(o) => o,
        None => return Vec::new(),
    };

    // Resolve dir so we can match which worktree is "current"
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());

    let mut entries = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut is_first = true;

    for line in output.lines() {
        if let Some(wt_path) = line.strip_prefix("worktree ") {
            // Flush previous entry
            if let Some(path) = current_path.take() {
                let is_current = path
                    .canonicalize()
                    .map(|p| p.starts_with(&canonical_dir) || p == canonical_dir)
                    .unwrap_or(false);
                entries.push(WorktreeEntry {
                    path,
                    branch: current_branch.take(),
                    is_main: is_first,
                    is_current,
                });
                is_first = false;
            }
            current_path = Some(PathBuf::from(wt_path));
            current_branch = None;
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(rest.to_string());
        }
    }
    // Flush the last entry
    if let Some(path) = current_path {
        let is_current = path
            .canonicalize()
            .map(|p| p.starts_with(&canonical_dir) || p == canonical_dir)
            .unwrap_or(false);
        entries.push(WorktreeEntry {
            path,
            branch: current_branch,
            is_main: is_first,
            is_current,
        });
    }
    entries
}

/// Add a new worktree at `path` checked out on `branch`.
pub fn worktree_add(dir: &Path, path: &str, branch: &str) -> Result<(), String> {
    run_git_result(dir, &["worktree", "add", path, branch])
}

/// Remove an existing worktree at `path`.
pub fn worktree_remove(dir: &Path, path: &str) -> Result<(), String> {
    run_git_result(dir, &["worktree", "remove", path])
}

/// A single entry from `git log --oneline`.
#[derive(Debug, Clone)]
pub struct GitLogEntry {
    /// Short (abbreviated) commit hash.
    pub hash: String,
    /// Commit subject line.
    pub message: String,
}

/// A detailed log entry with author, date, and stat summary.
#[derive(Debug, Clone)]
pub struct DetailedLogEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub stat: String,
}

/// A single stash entry from `git stash list`.
#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub branch: String,
}

/// Return the last `limit` commits as `GitLogEntry` items.
pub fn git_log(dir: &Path, limit: usize) -> Vec<GitLogEntry> {
    let limit_str = format!("-{}", limit);
    let output = match run_git(dir, &["log", "--format=%H %s", &limit_str]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    output
        .lines()
        .filter_map(|line| {
            let (hash, rest) = line.split_once(' ')?;
            Some(GitLogEntry {
                hash: hash.to_string(),
                message: rest.to_string(),
            })
        })
        .collect()
}

/// Fetch a single commit's info by hash (for revealing commits not in the top N).
pub fn git_log_commit(dir: &Path, hash: &str) -> Option<GitLogEntry> {
    let output = run_git(dir, &["log", "--format=%H %s", "-1", hash])?;
    let line = output.lines().next()?;
    let (full_hash, message) = line.split_once(' ')?;
    Some(GitLogEntry {
        hash: full_hash.to_string(),
        message: message.to_string(),
    })
}

// ─── blame_line / log_file (for Lua plugin API) ───────────────────────────────

/// Parsed blame information for a single line.
#[derive(Debug, Clone)]
pub struct BlameInfo {
    /// Short (8-char) commit hash.
    pub hash: String,
    /// Commit author name.
    pub author: String,
    /// Unix timestamp of the commit.
    pub timestamp: i64,
    /// Timezone offset in seconds east of UTC (e.g. +0100 = 3600).
    pub tz_offset: i32,
    /// Commit subject line.
    pub message: String,
    /// Human-readable relative date (e.g. "3 days ago").
    pub relative_date: String,
    /// True when git reports the line as not yet committed (all-zero hash).
    pub not_committed: bool,
}

/// Return blame information for a single 1-indexed line using
/// `git blame -L {line},{line} --porcelain -- {file}`.
///
/// When `buf_contents` is supplied the buffer text is piped via stdin with
/// `--contents -` so that git sees the current in-memory state of the file
/// (including unsaved lines) rather than the stale on-disk version.
///
/// Returns `None` when the file is untracked, has no commits, or any git
/// failure occurs.
pub fn blame_line(
    repo_root: &Path,
    file: &Path,
    line: usize,
    buf_contents: Option<&str>,
) -> Option<BlameInfo> {
    let line_spec = format!("{},{}", line, line);
    let file_str = file.to_str()?;
    let raw = if let Some(contents) = buf_contents {
        run_git_stdin(
            repo_root,
            &[
                "blame",
                "-L",
                &line_spec,
                "--porcelain",
                "--contents",
                "-",
                "--",
                file_str,
            ],
            contents,
        )
        .ok()?
    } else {
        run_git(
            repo_root,
            &["blame", "-L", &line_spec, "--porcelain", "--", file_str],
        )?
    };
    if raw.trim().is_empty() {
        return None;
    }

    let mut hash = String::new();
    let mut author = String::new();
    let mut timestamp: i64 = 0;
    let mut summary = String::new();

    for blame_line in raw.lines() {
        if let Some(s) = blame_line.strip_prefix("summary ") {
            summary = s.to_string();
        } else if let Some(a) = blame_line.strip_prefix("author ") {
            author = a.to_string();
        } else if let Some(t) = blame_line.strip_prefix("author-time ") {
            timestamp = t.trim().parse().unwrap_or(0);
        } else {
            // Header line: 40 hex chars followed by space + line numbers
            let bytes = blame_line.as_bytes();
            if bytes.len() >= 42
                && bytes[40] == b' '
                && bytes[..40].iter().all(|&b| b.is_ascii_hexdigit())
            {
                hash = blame_line[..8].to_string();
            }
        }
    }

    if hash.is_empty() {
        return None;
    }

    // Compute relative date from timestamp
    let relative_date = epoch_to_relative(timestamp);

    Some(BlameInfo {
        not_committed: hash.chars().all(|c| c == '0'),
        hash,
        author,
        timestamp,
        tz_offset: 0,
        message: summary,
        relative_date,
    })
}

/// Convert a Unix timestamp into a human-readable relative string
/// (e.g. "just now", "3 hours ago", "2 days ago", "1 week ago", "Mar 2026").
/// `pub(crate)` so `engine::picker`'s `:AiSessions` picker (#1459) can reuse
/// it for each entry's "last used" detail instead of re-deriving the same
/// bucketing.
pub(crate) fn epoch_to_relative(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        let m = diff / 60;
        if m == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", m)
        }
    } else if diff < 86400 {
        let h = diff / 3600;
        if h == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", h)
        }
    } else if diff < 7 * 86400 {
        let d = diff / 86400;
        if d == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", d)
        }
    } else if diff < 30 * 86400 {
        let w = diff / (7 * 86400);
        if w == 1 {
            "1 week ago".to_string()
        } else {
            format!("{} weeks ago", w)
        }
    } else if diff < 365 * 86400 {
        let mo = diff / (30 * 86400);
        if mo == 1 {
            "1 month ago".to_string()
        } else {
            format!("{} months ago", mo)
        }
    } else {
        // Fall back to a YYYY-MM format
        epoch_to_date(ts)[..7].to_string()
    }
}

/// Return the last `limit` commits for a specific file as `GitLogEntry` items.
/// Uses `git log --oneline -N -- <file>`.
pub fn log_file(repo_root: &Path, file: &Path, limit: usize) -> Vec<GitLogEntry> {
    let limit_str = format!("-{}", limit);
    let file_str = match file.to_str() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let output = match run_git(repo_root, &["log", "--oneline", &limit_str, "--", file_str]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    output
        .lines()
        .filter_map(|line| {
            let (hash, rest) = line.split_once(' ')?;
            Some(GitLogEntry {
                hash: hash.to_string(),
                message: rest.to_string(),
            })
        })
        .collect()
}

// ─── Extended git API (for plugin system) ─────────────────────────────────────

/// Run `git show <hash>` and return the full output.
pub fn show_commit(dir: &Path, hash: &str) -> Option<String> {
    run_git(dir, &["show", hash])
}

/// Return a `DetailedLogEntry` for a single commit hash.
pub fn commit_detail(dir: &Path, hash: &str) -> Option<DetailedLogEntry> {
    let out = run_git(
        dir,
        &["log", "-1", "--format=%H%n%an%n%ar%n%B", "--stat", hash],
    )?;
    // Format: hash\nauthor\ndate\n<full body>\n\n<stat lines>\n summary line
    let mut lines = out.lines();
    let full_hash = lines.next()?.to_string();
    let author = lines.next()?.to_string();
    let date = lines.next()?.to_string();
    // Rest is body + stat; split at the stat block (lines matching "file | N ++-")
    let remaining: Vec<&str> = lines.collect();
    // The stat block starts after an empty line following the body,
    // and ends with a summary line like " N files changed, ..."
    let mut body_end = remaining.len();
    let mut stat_start = remaining.len();
    for (i, line) in remaining.iter().enumerate().rev() {
        if line.starts_with(' ')
            && line.contains("changed")
            && (line.contains("insertion") || line.contains("deletion"))
        {
            // This is the stat summary line — stat block starts a few lines before
            stat_start = i;
            // Walk backwards to find where stat lines begin (lines with " | ")
            for j in (0..i).rev() {
                if remaining[j].contains(" | ") {
                    stat_start = j;
                } else {
                    break;
                }
            }
            // Body ends at the blank line before stat
            body_end = if stat_start > 0 && remaining[stat_start - 1].is_empty() {
                stat_start - 1
            } else {
                stat_start
            };
            break;
        }
    }
    let message = remaining[..body_end].join("\n").trim().to_string();
    let stat = remaining[stat_start..].join("\n").trim().to_string();
    Some(DetailedLogEntry {
        hash: full_hash.chars().take(8).collect(),
        author,
        date,
        message,
        stat,
    })
}

/// Return `git diff --stat` output for a single file (staged or unstaged).
pub fn diff_stat_file(dir: &Path, path: &str, staged: bool) -> Option<String> {
    let args = if staged {
        vec!["diff", "--cached", "--stat", "--", path]
    } else {
        vec!["diff", "--stat", "--", path]
    };
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
    let out = run_git(dir, &arg_refs)?;
    if out.trim().is_empty() {
        None
    } else {
        Some(out.trim().to_string())
    }
}

/// Return the tracking remote branch for the current branch (e.g. "origin/main").
pub fn tracking_branch(dir: &Path) -> Option<String> {
    run_git(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Return the remote URL for `origin` (or the first remote), normalized to HTTPS.
pub fn remote_url(dir: &Path) -> Option<String> {
    let raw = run_git(dir, &["config", "--get", "remote.origin.url"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    Some(normalize_remote_url(&raw))
}

/// Normalize a git remote URL to an HTTPS web URL (no `.git` suffix).
///
/// Handles: `https://github.com/user/repo.git`, `git@github.com:user/repo.git`,
/// `ssh://git@github.com/user/repo`.
fn normalize_remote_url(url: &str) -> String {
    let mut s = url.to_string();
    // SSH shorthand: git@host:user/repo → https://host/user/repo
    if let Some(rest) = s.strip_prefix("git@") {
        if let Some(colon) = rest.find(':') {
            let host = &rest[..colon];
            let path = &rest[colon + 1..];
            s = format!("https://{}/{}", host, path);
        }
    }
    // ssh:// scheme
    if s.starts_with("ssh://") {
        s = s.replacen("ssh://", "https://", 1);
        // Remove user@ if present (e.g. https://git@github.com/...)
        let after_scheme = &s["https://".len()..];
        if let Some(at) = after_scheme.find('@') {
            let first_slash = after_scheme.find('/').unwrap_or(after_scheme.len());
            if at < first_slash {
                s = format!("https://{}", &after_scheme[at + 1..]);
            }
        }
    }
    // Strip trailing .git
    if s.ends_with(".git") {
        s.truncate(s.len() - 4);
    }
    s
}

/// Build a web URL for a commit on the hosting platform.
///
/// Returns `None` if the remote URL can't be resolved or isn't HTTPS.
pub fn commit_url(dir: &Path, hash: &str) -> Option<String> {
    let base = remote_url(dir)?;
    if !base.starts_with("https://") {
        return None;
    }
    // GitHub/GitLab/Bitbucket all use /commit/<hash>
    Some(format!("{}/commit/{}", base, hash))
}

/// Retrieve the contents of a file at a given git revision.
///
/// `rev` is typically `"HEAD"` but can be any ref/commit.
/// `rel_path` is the path relative to the repository root.
/// Returns `None` if the file doesn't exist at that revision or git fails.
pub fn show_file_at_ref(dir: &Path, rev: &str, rel_path: &str) -> Option<String> {
    run_git(dir, &["show", &format!("{rev}:{rel_path}")])
}

/// Run `git blame --porcelain` on the full file and return structured blame info
/// for every line.
pub fn blame_file_structured(
    repo_root: &Path,
    file: &Path,
    buf_contents: Option<&str>,
) -> Vec<BlameInfo> {
    let file_str = match file.to_str() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let raw = if let Some(contents) = buf_contents {
        match run_git_stdin(
            repo_root,
            &["blame", "--porcelain", "--contents", "-", "--", file_str],
            contents,
        ) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        }
    } else {
        match run_git(repo_root, &["blame", "--porcelain", "--", file_str]) {
            Some(r) => r,
            None => return Vec::new(),
        }
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    let mut hash = String::new();
    let mut author = String::new();
    let mut timestamp: i64 = 0;
    let mut tz_offset: i32 = 0;
    let mut summary = String::new();
    let mut commit_info: HashMap<String, (String, i64, i32, String)> = HashMap::new();

    for line in raw.lines() {
        if line.starts_with('\t') {
            // Source content line — emit blame entry
            if let Some((a, t, tz, s)) = commit_info.get(&hash) {
                author = a.clone();
                timestamp = *t;
                tz_offset = *tz;
                summary = s.clone();
            }
            let short_hash: String = hash.chars().take(8).collect();
            results.push(BlameInfo {
                not_committed: short_hash.chars().all(|c| c == '0'),
                relative_date: epoch_to_relative(timestamp),
                hash: short_hash,
                author: author.clone(),
                timestamp,
                tz_offset,
                message: summary.clone(),
            });
        } else if let Some(a) = line.strip_prefix("author ") {
            author = a.to_string();
        } else if let Some(t) = line.strip_prefix("author-time ") {
            timestamp = t.trim().parse().unwrap_or(0);
        } else if let Some(tz) = line.strip_prefix("author-tz ") {
            tz_offset = parse_tz_offset(tz.trim());
        } else if let Some(s) = line.strip_prefix("summary ") {
            summary = s.to_string();
        } else {
            let bytes = line.as_bytes();
            if bytes.len() >= 42
                && bytes[40] == b' '
                && bytes[..40].iter().all(|&b| b.is_ascii_hexdigit())
            {
                let full_hash = line[..40].to_string();
                // Store info if we have it, before switching to new hash
                if !hash.is_empty() {
                    commit_info
                        .entry(hash.clone())
                        .or_insert_with(|| (author.clone(), timestamp, tz_offset, summary.clone()));
                }
                hash = full_hash;
            }
        }
    }

    results
}

/// Parse a git timezone offset string like "+0100" or "-0530" into seconds east of UTC.
fn parse_tz_offset(s: &str) -> i32 {
    if s.len() < 5 {
        return 0;
    }
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let digits = &s[1..];
    let hours: i32 = digits[..2].parse().unwrap_or(0);
    let minutes: i32 = digits[2..4].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Run `git log -L start,end:file` and return detailed log entries for a line range.
pub fn log_line_range(
    repo_root: &Path,
    file: &Path,
    start: usize,
    end: usize,
    limit: usize,
) -> Vec<DetailedLogEntry> {
    let file_str = match file.to_str() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let range = format!("-L {},{}", start, end);
    // git log -L doesn't support --oneline well, use custom format
    let format_str = "--format=%H%n%an%n%ar%n%s";
    let limit_str = format!("-{}", limit);
    let output = match run_git(
        repo_root,
        &[
            "log",
            &limit_str,
            &range,
            format_str,
            "--no-patch",
            "--",
            file_str,
        ],
    ) {
        Some(o) => o,
        None => return Vec::new(),
    };

    // Parse groups of 4 lines: hash, author, date, message
    let lines: Vec<&str> = output.lines().collect();
    let mut entries = Vec::new();
    let mut i = 0;
    while i + 3 < lines.len() {
        // Skip empty lines
        if lines[i].is_empty() {
            i += 1;
            continue;
        }
        let hash = &lines[i];
        let author = lines[i + 1];
        let date = lines[i + 2];
        let message = lines[i + 3];
        entries.push(DetailedLogEntry {
            hash: hash.chars().take(8).collect(),
            author: author.to_string(),
            date: date.to_string(),
            message: message.to_string(),
            stat: String::new(),
        });
        i += 4;
    }
    entries
}

/// Run `git diff <ref>` and return the full diff output.
pub fn diff_against_ref(dir: &Path, ref_spec: &str) -> Option<String> {
    run_git(dir, &["diff", ref_spec])
}

/// Paths (relative to the repository root) that differ between `base` and
/// `head`, using git's three-dot ("what changed on `head` since it
/// diverged from `base`") comparison rather than a plain two-dot diff —
/// the right semantics for reviewing a feature branch against the branch
/// it was cut from, since it ignores commits `base` has picked up in the
/// meantime that `head` never saw. Both `base`/`head` must already be
/// resolvable in the local repository (already pulled/checked out) — this
/// is worktree-local, no fetch (#525's stated scope; the remote/ssh case
/// is #530).
///
/// Returns `None` on any git failure — no repo, an unknown ref, or a
/// `base`/`head` that looks like a flag (see below) — and `Some(vec![])`
/// for a real, successful diff that just happens to contain no changes.
/// Callers that only cared about "were there any changes" used to conflate
/// the two (both were an empty `Vec`); keeping them apart lets
/// `Engine::open_branch_review` report "no changes between X and Y"
/// only when that's actually true, rather than also for a misconfigured
/// provider's bogus branch name (review non-blocking finding, #525).
///
/// `base`/`head` typically arrive from an external board provider's JSON
/// (`BranchReviewTarget`, #522's seam) and are joined into a single
/// `base...head` positional argument below with no shell involved — so
/// this isn't classic injection, but a value starting with `-` could still
/// be parsed by git as a flag rather than a revision ("flag-smuggling").
/// Reject that case up front rather than let it reach git's argv.
pub fn changed_files_between(dir: &Path, base: &str, head: &str) -> Option<Vec<String>> {
    if base.starts_with('-') || head.starts_with('-') {
        return None;
    }
    let spec = format!("{base}...{head}");
    run_git(dir, &["diff", "--name-only", &spec]).map(|output| {
        output
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    })
}

/// Return detailed log entries for a specific file.
pub fn file_log_detailed(repo_root: &Path, file: &Path, limit: usize) -> Vec<DetailedLogEntry> {
    let file_str = match file.to_str() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let limit_str = format!("-{}", limit);
    let format_str = "--format=%h%n%an%n%ar%n%s";
    let output = match run_git(
        repo_root,
        &[
            "log",
            &limit_str,
            format_str,
            "--stat",
            "--stat-width=60",
            "--",
            file_str,
        ],
    ) {
        Some(o) => o,
        None => return Vec::new(),
    };

    // Parse: hash, author, date, message, then stat lines until blank line
    let lines: Vec<&str> = output.lines().collect();
    let mut entries = Vec::new();
    let mut i = 0;
    while i + 3 < lines.len() {
        if lines[i].is_empty() {
            i += 1;
            continue;
        }
        let hash = lines[i].to_string();
        let author = lines[i + 1].to_string();
        let date = lines[i + 2].to_string();
        let message = lines[i + 3].to_string();
        i += 4;
        // Collect stat lines until blank line or EOF
        let mut stat_lines = Vec::new();
        while i < lines.len() && !lines[i].is_empty() {
            stat_lines.push(lines[i]);
            i += 1;
        }
        let stat = if let Some(last) = stat_lines.last() {
            // The last stat line is the summary (e.g. "3 files changed, 12 insertions(+)")
            last.trim().to_string()
        } else {
            String::new()
        };
        entries.push(DetailedLogEntry {
            hash,
            author,
            date,
            message,
            stat,
        });
    }
    entries
}

/// Return the list of stash entries.
pub fn stash_list(dir: &Path) -> Vec<StashEntry> {
    let output = match run_git(dir, &["stash", "list"]) {
        Some(o) => o,
        None => return Vec::new(),
    };
    output
        .lines()
        .filter_map(|line| {
            // Format: "stash@{0}: On branch_name: message"
            let rest = line.strip_prefix("stash@{")?;
            let (idx_str, rest) = rest.split_once('}')?;
            let index: usize = idx_str.parse().ok()?;
            let rest = rest.strip_prefix(": ")?;
            // "On <branch>: <message>" or "WIP on <branch>: <hash> <message>"
            let (branch, message) = if let Some(r) = rest.strip_prefix("On ") {
                if let Some((b, m)) = r.split_once(": ") {
                    (b.to_string(), m.to_string())
                } else {
                    (r.to_string(), String::new())
                }
            } else if let Some(r) = rest.strip_prefix("WIP on ") {
                if let Some((b, m)) = r.split_once(": ") {
                    (b.to_string(), m.to_string())
                } else {
                    (r.to_string(), String::new())
                }
            } else {
                (String::new(), rest.to_string())
            };
            Some(StashEntry {
                index,
                message,
                branch,
            })
        })
        .collect()
}

/// Push changes to stash with an optional message.
pub fn stash_push(dir: &Path, msg: Option<&str>) -> Result<String, String> {
    let output = if let Some(m) = msg {
        git_command()
            .arg("-C")
            .arg(dir)
            .args(["stash", "push", "-m", m])
            .output()
    } else {
        git_command()
            .arg("-C")
            .arg(dir)
            .args(["stash", "push"])
            .output()
    };
    let output = output.map_err(|e| format!("git stash push failed: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Pop a stash entry by index.
pub fn stash_pop(dir: &Path, index: usize) -> Result<String, String> {
    let stash_ref = format!("stash@{{{}}}", index);
    let output = git_command()
        .arg("-C")
        .arg(dir)
        .args(["stash", "pop", &stash_ref])
        .output()
        .map_err(|e| format!("git stash pop failed: {}", e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Show a stash entry's diff by index.
pub fn stash_show(dir: &Path, index: usize) -> Option<String> {
    let stash_ref = format!("stash@{{{}}}", index);
    run_git(dir, &["stash", "show", "-p", &stash_ref])
}

/// Return `(ahead, behind)` commit counts relative to the upstream branch.
pub fn ahead_behind(dir: &Path) -> (u32, u32) {
    let out = match run_git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{u}"]) {
        Some(o) => o,
        None => return (0, 0),
    };
    let parts: Vec<&str> = out.split_whitespace().collect();
    let ahead = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let behind = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (ahead, behind)
}

/// A single entry from `git branch -a`.
#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
    pub upstream: Option<String>,
    pub ahead_behind: Option<String>,
}

/// List all branches (local + remote) with tracking info.
pub fn list_branches(dir: &Path) -> Vec<BranchEntry> {
    let out = match run_git(
        dir,
        &[
            "branch",
            "-a",
            "--format=%(refname:short)|%(HEAD)|%(upstream:short)|%(upstream:track)",
        ],
    ) {
        Some(o) => o,
        None => return Vec::new(),
    };
    out.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            let name = parts.first().copied().unwrap_or("").to_string();
            let is_current = parts.get(1).copied().unwrap_or("") == "*";
            let upstream_raw = parts.get(2).copied().unwrap_or("");
            let upstream = if upstream_raw.is_empty() {
                None
            } else {
                Some(upstream_raw.to_string())
            };
            let track_raw = parts.get(3).copied().unwrap_or("");
            let ahead_behind = if track_raw.is_empty() {
                None
            } else {
                Some(track_raw.to_string())
            };
            BranchEntry {
                name,
                is_current,
                upstream,
                ahead_behind,
            }
        })
        .collect()
}

#[cfg(test)]
mod sc_tests {
    use super::*;

    #[test]
    fn test_status_detailed_parses_porcelain() {
        // Manually test the parse_status_char helper
        assert_eq!(parse_status_char('A', false), Some(StatusKind::Added));
        assert_eq!(parse_status_char('M', false), Some(StatusKind::Modified));
        assert_eq!(parse_status_char('D', false), Some(StatusKind::Deleted));
        assert_eq!(parse_status_char('R', false), Some(StatusKind::Renamed));
        assert_eq!(parse_status_char('?', true), Some(StatusKind::Untracked));
        assert_eq!(parse_status_char('?', false), None); // '?' only applies to workdir
        assert_eq!(parse_status_char(' ', false), None);
    }

    #[test]
    fn test_status_kind_label() {
        assert_eq!(StatusKind::Added.label(), 'A');
        assert_eq!(StatusKind::Modified.label(), 'M');
        assert_eq!(StatusKind::Deleted.label(), 'D');
        assert_eq!(StatusKind::Renamed.label(), 'R');
        assert_eq!(StatusKind::Copied.label(), 'C');
        // #1051: VS Code's explorer/SC badge, not git's own '?' porcelain
        // notation (`parse_status_char` above still parses that '?').
        assert_eq!(StatusKind::Untracked.label(), 'U');
        // #991: VS Code's conflict marker.
        assert_eq!(StatusKind::Unmerged.label(), '!');
    }

    /// #991: unit coverage on the shared classifier for **all seven**
    /// unmerged XY codes. Each must come back as a conflict with *both*
    /// ordinary sides `None`, so every `staged.is_some()` /
    /// `unstaged.is_some()` filter in the SC panel excludes it.
    #[test]
    fn classify_xy_recognises_all_seven_unmerged_codes() {
        let cases = [
            ("DD", UnmergedKind::BothDeleted, "both deleted"),
            ("AU", UnmergedKind::AddedByUs, "added by us"),
            ("UD", UnmergedKind::DeletedByThem, "deleted by them"),
            ("UA", UnmergedKind::AddedByThem, "added by them"),
            ("DU", UnmergedKind::DeletedByUs, "deleted by us"),
            ("AA", UnmergedKind::BothAdded, "both added"),
            ("UU", UnmergedKind::BothModified, "both modified"),
        ];
        for (code, kind, description) in cases {
            let mut ch = code.chars();
            let cls = classify_xy(ch.next().unwrap(), ch.next().unwrap());
            assert_eq!(
                cls.unmerged,
                Some(kind),
                "{code} must classify as a merge conflict"
            );
            assert_eq!(cls.staged, None, "{code} must not look staged");
            assert_eq!(cls.unstaged, None, "{code} must not look unstaged");
            assert!(!cls.is_empty(), "{code} must not be dropped as no-change");
            assert_eq!(kind.code(), code);
            assert_eq!(kind.description(), description);
        }
    }

    /// #991 regression guard: a lone `A`/`D` on one side is an ordinary
    /// change, not a conflict — `AA`/`DD` are conflicts only *as pairs*,
    /// which is why the classifier takes the pair.
    #[test]
    fn classify_xy_keeps_ordinary_pairs_out_of_the_conflict_bucket() {
        let ordinary = ["A ", "M ", " M", "D ", " D", "R ", "MM", "AM", "??", "C "];
        for code in ordinary {
            let mut ch = code.chars();
            let cls = classify_xy(ch.next().unwrap(), ch.next().unwrap());
            assert_eq!(
                cls.unmerged, None,
                "{code:?} is an ordinary change, not a merge conflict"
            );
            assert!(!cls.is_empty(), "{code:?} must still produce a row");
        }
        // Truly-unmodified pairs produce nothing at all.
        assert!(classify_xy(' ', ' ').is_empty());
    }

    #[test]
    fn classify_xy_pairs_map_to_the_expected_sides() {
        assert_eq!(
            classify_xy('A', ' '),
            XyStatus {
                staged: Some(StatusKind::Added),
                unstaged: None,
                unmerged: None,
            }
        );
        assert_eq!(
            classify_xy('M', 'M'),
            XyStatus {
                staged: Some(StatusKind::Modified),
                unstaged: Some(StatusKind::Modified),
                unmerged: None,
            }
        );
        assert_eq!(
            classify_xy('?', '?'),
            XyStatus {
                staged: None,
                unstaged: Some(StatusKind::Untracked),
                unmerged: None,
            }
        );
    }

    #[test]
    fn test_worktree_list_parses_porcelain() {
        // We test the parsing logic by constructing a fake porcelain string
        // and calling a helper that mirrors the parse loop.
        let porcelain = "\
worktree /home/user/project\nHEAD abc123\nbranch refs/heads/main\n\n\
worktree /home/user/project-feat\nHEAD def456\nbranch refs/heads/feature/auth\n\n";

        let canonical_dir = PathBuf::from("/home/user/project");
        let entries = parse_worktree_porcelain(porcelain, &canonical_dir);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_main);
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(!entries[1].is_main);
        assert_eq!(entries[1].branch.as_deref(), Some("feature/auth"));
    }

    #[test]
    fn test_blame_line_porcelain_parses() {
        // Synthesise a minimal porcelain blame output for one line.
        let raw = "\
abc123456789012345678901234567890abc1234 1 1 1\n\
author Alice Smith\n\
author-mail <alice@example.com>\n\
author-time 1700000000\n\
author-tz +0000\n\
committer Alice Smith\n\
committer-mail <alice@example.com>\n\
committer-time 1700000000\n\
committer-tz +0000\n\
summary Fix a bug\n\
filename src/main.rs\n\
\tlet x = 1;\n";

        let info = parse_blame_line_porcelain(raw).expect("should parse");
        assert_eq!(info.hash, "abc12345");
        assert_eq!(info.author, "Alice Smith");
        assert_eq!(info.timestamp, 1_700_000_000);
        assert_eq!(info.message, "Fix a bug");
        assert!(!info.relative_date.is_empty());
    }

    #[test]
    fn test_blame_line_porcelain_empty_returns_none() {
        assert!(parse_blame_line_porcelain("").is_none());
        assert!(parse_blame_line_porcelain("no hash here\n").is_none());
    }

    #[test]
    fn test_epoch_to_relative_just_now() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        assert_eq!(epoch_to_relative(now), "just now");
    }

    #[test]
    fn test_epoch_to_relative_days() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // 3 days ago
        let ts = now - 3 * 86400;
        assert_eq!(epoch_to_relative(ts), "3 days ago");
    }

    #[test]
    fn test_epoch_to_relative_old() {
        // Far in the past — should return YYYY-MM format
        let result = epoch_to_relative(0); // Unix epoch: 1970-01
        assert!(result.starts_with("1970"), "expected YYYY-MM, got {result}");
    }

    #[test]
    fn test_stash_list_parses() {
        let input = "stash@{0}: On main: WIP save\nstash@{1}: WIP on feature: abc1234 half done\n";
        let entries = parse_stash_list(input);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].branch, "main");
        assert_eq!(entries[0].message, "WIP save");
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].branch, "feature");
        assert_eq!(entries[1].message, "abc1234 half done");
    }

    #[test]
    fn test_stash_list_empty() {
        let entries = parse_stash_list("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_blame_file_structured_basic() {
        let hash = "a".repeat(40);
        let input = format!(
            "{h} 1 1 2\nauthor Alice\nauthor-time 1700000000\nsummary Fix bug\nfilename foo.rs\n\tline one\n\
             {h} 2 2\nfilename foo.rs\n\tline two\n",
            h = hash
        );
        let entries = parse_blame_file_porcelain(&input);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].hash, "aaaaaaaa");
        assert_eq!(entries[0].author, "Alice");
        assert_eq!(entries[0].message, "Fix bug");
        assert_eq!(entries[1].hash, "aaaaaaaa");
        assert_eq!(entries[1].author, "Alice");
    }

    #[test]
    fn test_blame_file_structured_empty() {
        let entries = parse_blame_file_porcelain("");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_detailed_log_parse() {
        let input = "abc1234\nAlice\n3 days ago\nFix the bug\n 1 file changed, 3 insertions(+)\n";
        let entries = parse_detailed_log(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hash, "abc1234");
        assert_eq!(entries[0].author, "Alice");
        assert_eq!(entries[0].date, "3 days ago");
        assert_eq!(entries[0].message, "Fix the bug");
        assert_eq!(entries[0].stat, "1 file changed, 3 insertions(+)");
    }
}

/// Unit-testable helper — parse a `git blame --porcelain` output for a single
/// line as if called by `blame_line`. Defined after `mod sc_tests` so that
/// `use super::*` inside the module can pick it up.
#[cfg(test)]
pub(crate) fn parse_blame_line_porcelain(raw: &str) -> Option<BlameInfo> {
    let mut hash = String::new();
    let mut author = String::new();
    let mut timestamp: i64 = 0;
    let mut summary = String::new();

    for line in raw.lines() {
        if let Some(s) = line.strip_prefix("summary ") {
            summary = s.to_string();
        } else if let Some(a) = line.strip_prefix("author ") {
            author = a.to_string();
        } else if let Some(t) = line.strip_prefix("author-time ") {
            timestamp = t.trim().parse().unwrap_or(0);
        } else {
            let bytes = line.as_bytes();
            if bytes.len() >= 42
                && bytes[40] == b' '
                && bytes[..40].iter().all(|&b| b.is_ascii_hexdigit())
            {
                hash = line[..8].to_string();
            }
        }
    }
    if hash.is_empty() {
        return None;
    }
    Some(BlameInfo {
        not_committed: hash.chars().all(|c| c == '0'),
        relative_date: epoch_to_relative(timestamp),
        hash,
        author,
        timestamp,
        tz_offset: 0,
        message: summary,
    })
}

/// Exposed for testing — same parse logic as `worktree_list` but on a string.
#[cfg(test)]
fn parse_worktree_porcelain(output: &str, current_dir: &Path) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_branch: Option<String> = None;
    let mut is_first = true;
    for line in output.lines() {
        if line.starts_with("worktree ") {
            if let Some(path) = current_path.take() {
                let is_current = path == current_dir;
                entries.push(WorktreeEntry {
                    path,
                    branch: current_branch.take(),
                    is_main: is_first,
                    is_current,
                });
                is_first = false;
            }
            current_path = Some(PathBuf::from(&line["worktree ".len()..]));
            current_branch = None;
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(rest.to_string());
        }
    }
    if let Some(path) = current_path {
        let is_current = path == current_dir;
        entries.push(WorktreeEntry {
            path,
            branch: current_branch,
            is_main: is_first,
            is_current,
        });
    }
    entries
}

/// Unit-testable stash list parser — mirrors the parse logic in `stash_list`.
#[cfg(test)]
fn parse_stash_list(output: &str) -> Vec<StashEntry> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("stash@{")?;
            let (idx_str, rest) = rest.split_once('}')?;
            let index: usize = idx_str.parse().ok()?;
            let rest = rest.strip_prefix(": ")?;
            let (branch, message) = if let Some(r) = rest.strip_prefix("On ") {
                if let Some((b, m)) = r.split_once(": ") {
                    (b.to_string(), m.to_string())
                } else {
                    (r.to_string(), String::new())
                }
            } else if let Some(r) = rest.strip_prefix("WIP on ") {
                if let Some((b, m)) = r.split_once(": ") {
                    (b.to_string(), m.to_string())
                } else {
                    (r.to_string(), String::new())
                }
            } else {
                (String::new(), rest.to_string())
            };
            Some(StashEntry {
                index,
                message,
                branch,
            })
        })
        .collect()
}

/// Unit-testable blame file parser — mirrors the parse logic in `blame_file_structured`.
#[cfg(test)]
fn parse_blame_file_porcelain(raw: &str) -> Vec<BlameInfo> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let mut results = Vec::new();
    let mut hash = String::new();
    let mut author = String::new();
    let mut timestamp: i64 = 0;
    let mut tz_offset: i32 = 0;
    let mut summary = String::new();
    let mut commit_info: HashMap<String, (String, i64, i32, String)> = HashMap::new();

    for line in raw.lines() {
        if line.starts_with('\t') {
            if let Some((a, t, tz, s)) = commit_info.get(&hash) {
                author = a.clone();
                timestamp = *t;
                tz_offset = *tz;
                summary = s.clone();
            }
            let short_hash: String = hash.chars().take(8).collect();
            results.push(BlameInfo {
                not_committed: short_hash.chars().all(|c| c == '0'),
                relative_date: epoch_to_relative(timestamp),
                hash: short_hash,
                author: author.clone(),
                timestamp,
                tz_offset,
                message: summary.clone(),
            });
        } else if let Some(a) = line.strip_prefix("author ") {
            author = a.to_string();
        } else if let Some(t) = line.strip_prefix("author-time ") {
            timestamp = t.trim().parse().unwrap_or(0);
        } else if let Some(tz) = line.strip_prefix("author-tz ") {
            tz_offset = parse_tz_offset(tz.trim());
        } else if let Some(s) = line.strip_prefix("summary ") {
            summary = s.to_string();
        } else {
            let bytes = line.as_bytes();
            if bytes.len() >= 42
                && bytes[40] == b' '
                && bytes[..40].iter().all(|&b| b.is_ascii_hexdigit())
            {
                let full_hash = line[..40].to_string();
                if !hash.is_empty() {
                    commit_info
                        .entry(hash.clone())
                        .or_insert_with(|| (author.clone(), timestamp, tz_offset, summary.clone()));
                }
                hash = full_hash;
            }
        }
    }
    results
}

/// Unit-testable detailed log parser — mirrors the parse logic in `file_log_detailed`.
#[cfg(test)]
fn parse_detailed_log(output: &str) -> Vec<DetailedLogEntry> {
    let lines: Vec<&str> = output.lines().collect();
    let mut entries = Vec::new();
    let mut i = 0;
    while i + 3 < lines.len() {
        if lines[i].is_empty() {
            i += 1;
            continue;
        }
        let hash = lines[i].to_string();
        let author = lines[i + 1].to_string();
        let date = lines[i + 2].to_string();
        let message = lines[i + 3].to_string();
        i += 4;
        let mut stat_lines = Vec::new();
        while i < lines.len() && !lines[i].is_empty() {
            stat_lines.push(lines[i]);
            i += 1;
        }
        let stat = stat_lines
            .last()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        entries.push(DetailedLogEntry {
            hash,
            author,
            date,
            message,
            stat,
        });
    }
    entries
}

// ─── Commit file list ────────────────────────────────────────────────────────

/// A single file changed in a commit.
#[derive(Debug, Clone)]
pub struct CommitFileEntry {
    /// Status character: 'A' (added), 'M' (modified), 'D' (deleted), 'R' (renamed).
    pub status: char,
    pub path: String,
}

/// List files changed in a given commit.
pub fn commit_files(dir: &Path, hash: &str) -> Vec<CommitFileEntry> {
    let output = match run_git(
        dir,
        &["diff-tree", "--no-commit-id", "-r", "--name-status", hash],
    ) {
        Some(o) => o,
        None => return Vec::new(),
    };
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let status = parts.next()?.chars().next()?;
            let path = parts.next()?.to_string();
            Some(CommitFileEntry { status, path })
        })
        .collect()
}

/// Show a specific file's content at a given commit.
pub fn diff_file_at_commit(dir: &Path, hash: &str, path: &str) -> Option<String> {
    run_git(dir, &["show", &format!("{hash}:{path}")])
}

/// Show the diff for a specific file within a commit (like `git show <hash> -- <path>`).
pub fn show_commit_file(dir: &Path, hash: &str, path: &str) -> Option<String> {
    run_git(dir, &["show", hash, "--", path])
}
