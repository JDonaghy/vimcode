//! Project-wide file search with regex, case-sensitivity, and whole-word toggles.
//!
//! Uses the `ignore` crate (same walker as ripgrep) to respect `.gitignore`
//! and skip binary files.  Entirely in `core/` — no UI dependencies.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of results returned to prevent memory issues on huge repos.
const MAX_RESULTS: usize = 10_000;

/// A single match found during a project search.
#[derive(Debug, Clone)]
pub struct ProjectMatch {
    /// Absolute path to the file.
    pub file: PathBuf,
    /// 0-indexed line number within the file.
    pub line: usize,
    /// Byte offset of the match start within the line.
    /// Reserved for future highlight support.
    #[allow(dead_code)]
    pub col: usize,
    /// Full text of the line (trimmed to avoid rendering issues).
    pub line_text: String,
}

/// A quickfix-shaped list of matches plus its own navigation/UI state.
///
/// Vim keeps exactly this shape twice: once globally (`:c*`, the quickfix
/// list) and once per window (`:l*`, the location list). Modelling it as one
/// struct — rather than four flat fields duplicated per binding — is what
/// lets `Engine`'s `qf_*` methods (`src/core/engine/picker.rs`) implement
/// open/close/navigate/jump exactly once and have both `:c*` and `:l*` call
/// through the same code with a different target (#1155).
#[derive(Debug, Clone, Default)]
pub struct QuickfixList {
    /// The list's entries, in order.
    pub items: Vec<ProjectMatch>,
    /// Currently selected entry (0-based).
    pub selected: usize,
    /// Whether the list's panel is visible.
    pub open: bool,
    /// Whether the panel currently has keyboard focus.
    pub has_focus: bool,
}

/// Search-mode toggles (mirrors VS Code's Aa / Ab| / .* buttons).
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// When `true`, matching is case-sensitive.
    pub case_sensitive: bool,
    /// When `true`, the query matches whole words only (`\b...\b`).
    pub whole_word: bool,
    /// When `true`, the query is interpreted as a regular expression.
    pub use_regex: bool,
}

/// Error returned when the user-supplied regex is invalid.
#[derive(Debug, Clone)]
pub struct SearchError(pub String);

/// Result of a project-wide replace operation.
#[derive(Debug, Clone)]
pub struct ReplaceResult {
    /// Total number of individual replacements made across all files.
    pub replacement_count: usize,
    /// Number of files that were modified.
    pub file_count: usize,
    /// Files that were skipped (e.g. dirty buffers).
    pub skipped_files: Vec<PathBuf>,
    /// Files that were actually written to.
    pub modified_files: Vec<PathBuf>,
}

/// Build a compiled regex from the user query and search options.
///
/// Shared by `search_in_project`, `replace_in_project`, and (#1806)
/// `grep_project_streaming`.
pub(crate) fn build_search_regex(
    query: &str,
    options: &SearchOptions,
) -> Result<regex::Regex, SearchError> {
    let escaped = if options.use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let with_boundary = if options.whole_word {
        format!(r"\b{}\b", escaped)
    } else {
        escaped
    };
    let full_pattern = if options.case_sensitive {
        with_boundary
    } else {
        format!("(?i){}", with_boundary)
    };
    regex::Regex::new(&full_pattern).map_err(|e| SearchError(e.to_string()))
}

/// Search all text files under `root` for `query` using the given `options`.
///
/// - Respects `.gitignore` rules via the `ignore` crate.
/// - Binary files (non-UTF-8) are silently skipped.
/// - Hidden files/directories are skipped by default (ignore crate behaviour).
/// - Results are sorted by file path, then line number.
/// - At most `MAX_RESULTS` matches are returned.
/// - Returns `Err(SearchError)` if `use_regex` is true and the pattern is invalid.
pub fn search_in_project(
    root: &Path,
    query: &str,
    options: &SearchOptions,
) -> Result<Vec<ProjectMatch>, SearchError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let re = build_search_regex(query, options)?;

    let mut results: Vec<ProjectMatch> = Vec::new();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Only process files, not directories.
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path();
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // binary or unreadable — skip
        };

        for (line_idx, line_text) in content.lines().enumerate() {
            if let Some(m) = re.find(line_text) {
                results.push(ProjectMatch {
                    file: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                    line_text: line_text.to_string(),
                });
                if results.len() >= MAX_RESULTS {
                    results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
                    return Ok(results);
                }
            }
        }
    }

    results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    Ok(results)
}

// ─── Streaming walk/grep for `vimcode.fs.*` (#1806) ────────────────────────
//
// The native `PickerSource::Files`/`PickerSource::Grep` pickers
// (`src/core/engine/picker.rs`) and `search_in_project` above all walk
// synchronously to completion before handing back a `Vec`. A Lua finder
// extension re-queries on every keystroke, so it needs the same
// gitignore-aware walk, but delivered in batches as they're found (so the
// first results can paint immediately) and interruptible mid-walk (so a
// stale query's walk can be stopped the moment a newer one starts, without
// waiting for it to finish). `walk_project_streaming`/`grep_project_streaming`
// are that: the same `ignore::WalkBuilder` setup as `search_in_project`
// above and `Engine::picker_populate_files`, just batched and
// cancellation-checked per entry instead of collected into one `Vec`.
//
// `Engine::plugin_api_fs_walk`/`plugin_api_fs_grep`
// (`src/core/engine/fs_api.rs`) run these on a background thread and
// stream the batches back through an `mpsc` channel polled from
// `poll_idle`, same shape as `vimcode.loop.spawn`/`vimcode.http.request`.
//
// `walk_project_streaming` additionally applies `explorer_ops::
// walk_entry_is_excluded` via `FsWalkOptions::explorer_exclude`
// (`Engine::plugin_api_fs_walk` fills it in from `Settings::explorer_exclude`
// before calling in) — the same `filter_entry` `Engine::picker_populate_files`
// installs, so e.g. `.git`/`.svn` and any user `:set explorer_exclude=...`
// entry are pruned for `vimcode.fs.walk` exactly like they are for the native
// Files picker it's meant to replace (#1806 review). `grep_project_streaming`
// does *not* apply it — it mirrors `search_in_project` above, which never
// has either, not `picker_populate_files`.

/// Maximum number of paths/matches [`walk_project_streaming`]/
/// [`grep_project_streaming`] buffer before handing a batch to the caller's
/// `on_batch`. Small enough that a finder's first results appear quickly,
/// large enough that a huge repo doesn't send one Lua call per file.
pub const FS_WALK_BATCH_SIZE: usize = 200;
/// Same as [`FS_WALK_BATCH_SIZE`], for [`grep_project_streaming`]'s matches
/// rather than [`walk_project_streaming`]'s paths — kept smaller since each
/// element carries a full matched line's text, not just a path.
pub const FS_GREP_BATCH_SIZE: usize = 100;

/// Default cap on the number of grep matches returned, mirroring
/// `search_in_project`'s `MAX_RESULTS` — a plugin can pass a smaller
/// `max_results` but not a larger one.
pub const FS_GREP_MAX_RESULTS: usize = MAX_RESULTS;

/// Options for [`walk_project_streaming`] — `vimcode.fs.walk`'s `opts` table.
#[derive(Debug, Clone, Default)]
pub struct FsWalkOptions {
    /// Include hidden files/directories (dotfiles). Mirrors
    /// `Settings::show_hidden_files`'s effect on the native Files picker.
    pub hidden: bool,
    /// Stop descending past this many directory levels below `root`
    /// (`ignore::WalkBuilder::max_depth`'s own convention: `root` itself is
    /// depth 0, so its direct children are depth 1). `None` means unlimited.
    /// `Some(0)` is accepted as-is (not rejected) but yields zero *files*,
    /// matching `ignore::WalkBuilder`: depth 0 is only the root directory
    /// entry itself, which `walk_project_streaming` always filters out as
    /// "not a file". Depths are effectively 1-based for file results.
    pub max_depth: Option<usize>,
    /// Glob patterns a path must match at least one of (gitignore glob
    /// syntax, matched relative to `root`) to be included. Empty means
    /// "no whitelist — everything not excluded is included".
    pub include: Vec<String>,
    /// Glob patterns that exclude a path even if it matched `include`.
    pub exclude: Vec<String>,
    /// `Settings::explorer_exclude` entries (`**/.git`, `**/.svn`, plus any
    /// user `:set explorer_exclude=...` addition) — filled in by
    /// `Engine::plugin_api_fs_walk` from live settings, not settable from
    /// Lua. Applied the same way `Engine::picker_populate_files` applies it:
    /// as a `filter_entry` that prunes a matching *directory* rather than
    /// walking into it and discarding every entry underneath (#1806 review).
    pub explorer_exclude: Vec<String>,
}

/// Options for [`grep_project_streaming`] — `vimcode.fs.grep`'s `opts` table.
#[derive(Debug, Clone)]
pub struct FsGrepOptions {
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub max_results: usize,
}

impl Default for FsGrepOptions {
    fn default() -> Self {
        FsGrepOptions {
            case_sensitive: false,
            use_regex: false,
            include: Vec::new(),
            exclude: Vec::new(),
            max_results: FS_GREP_MAX_RESULTS,
        }
    }
}

/// One `vimcode.fs.grep` match — a line containing the pattern.
#[derive(Debug, Clone)]
pub struct FsGrepMatch {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// 0-indexed line number within the file.
    pub line: usize,
    /// 0-indexed byte offset of the match start within the line.
    pub col: usize,
    /// Full text of the matching line.
    pub text: String,
}

/// Build an `ignore::overrides::Override` from `vimcode.fs.*`'s
/// `include`/`exclude` glob lists — `include` entries are plain patterns,
/// `exclude` entries are negated (`!pattern`), so a path matching any
/// `exclude` pattern is dropped even if it also matches an `include` one
/// (same precedence `rg --glob` gives `!`-prefixed patterns).
fn build_fs_overrides(
    root: &Path,
    include: &[String],
    exclude: &[String],
) -> Result<ignore::overrides::Override, SearchError> {
    let mut builder = ignore::overrides::OverrideBuilder::new(root);
    for pat in include {
        builder
            .add(pat)
            .map_err(|e| SearchError(format!("bad include glob {pat:?}: {e}")))?;
    }
    for pat in exclude {
        builder
            .add(&format!("!{pat}"))
            .map_err(|e| SearchError(format!("bad exclude glob {pat:?}: {e}")))?;
    }
    builder
        .build()
        .map_err(|e| SearchError(format!("bad glob pattern: {e}")))
}

/// Validate `opts`'s glob patterns eagerly, returning the built
/// `ignore::overrides::Override` on success. Called synchronously by
/// `Engine::plugin_api_fs_walk` *before* spawning the background thread, so
/// a bad glob fails the Lua call immediately (matching `vimcode.loop.spawn`/
/// `vimcode.http.request`'s "fails to start" convention) instead of only
/// being discovered once the walk is already running off-thread.
pub(crate) fn validate_fs_walk_options(
    root: &Path,
    opts: &FsWalkOptions,
) -> Result<ignore::overrides::Override, SearchError> {
    build_fs_overrides(root, &opts.include, &opts.exclude)
}

/// Same as [`validate_fs_walk_options`], but also compiles the grep regex
/// so a bad pattern or glob fails `vimcode.fs.grep` immediately too.
pub(crate) fn validate_fs_grep_options(
    root: &Path,
    pattern: &str,
    opts: &FsGrepOptions,
) -> Result<(regex::Regex, ignore::overrides::Override), SearchError> {
    let search_opts = SearchOptions {
        case_sensitive: opts.case_sensitive,
        whole_word: false,
        use_regex: opts.use_regex,
    };
    let re = build_search_regex(pattern, &search_opts)?;
    let overrides = build_fs_overrides(root, &opts.include, &opts.exclude)?;
    Ok((re, overrides))
}

/// Walk `root` the same way `Engine::picker_populate_files` does — including
/// its `opts.explorer_exclude` `filter_entry` pruning, see the module doc
/// above — calling `on_batch` with up to [`FS_WALK_BATCH_SIZE`] paths at a
/// time as they're found. Checks `cancelled` before visiting each entry and
/// after each batch flush, returning early (without flushing a final partial
/// batch) the moment it's set — a cancelled walk delivers no more batches,
/// promptly.
///
/// A `root` that doesn't exist yields zero batches and then a normal
/// `on_done` — `ignore::WalkBuilder`'s single `Err` entry for the missing
/// root is swallowed by the `Err(_) => continue` below, the same as any
/// other per-entry walk error, so this is indistinguishable from walking an
/// empty directory.
pub(crate) fn walk_project_streaming(
    root: &Path,
    opts: &FsWalkOptions,
    overrides: ignore::overrides::Override,
    cancelled: &std::sync::atomic::AtomicBool,
    mut on_batch: impl FnMut(Vec<PathBuf>),
) {
    use std::sync::atomic::Ordering;

    let explorer_exclude = opts.explorer_exclude.clone();
    let mut wb = ignore::WalkBuilder::new(root);
    wb.hidden(!opts.hidden)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .overrides(overrides)
        .filter_entry(move |entry| {
            !crate::core::engine::explorer_ops::walk_entry_is_excluded(entry, &explorer_exclude)
        });
    if let Some(depth) = opts.max_depth {
        wb.max_depth(Some(depth));
    }
    let walker = wb.build();

    let mut batch: Vec<PathBuf> = Vec::new();
    for entry in walker {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        batch.push(entry.into_path());
        if batch.len() >= FS_WALK_BATCH_SIZE {
            on_batch(std::mem::take(&mut batch));
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
        }
    }
    if !batch.is_empty() {
        on_batch(batch);
    }
}

/// Grep `root` the same way `search_in_project` does, calling `on_batch`
/// with up to [`FS_GREP_BATCH_SIZE`] matches at a time as they're found,
/// stopping once `opts.max_results` total matches have been delivered.
/// Checks `cancelled` the same way [`walk_project_streaming`] does.
pub(crate) fn grep_project_streaming(
    root: &Path,
    re: &regex::Regex,
    overrides: ignore::overrides::Override,
    max_results: usize,
    cancelled: &std::sync::atomic::AtomicBool,
    mut on_batch: impl FnMut(Vec<FsGrepMatch>),
) {
    use std::sync::atomic::Ordering;

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .overrides(overrides)
        .build();

    let mut batch: Vec<FsGrepMatch> = Vec::new();
    let mut total = 0usize;
    for entry in walker {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // binary or unreadable — skip
        };
        for (line_idx, line_text) in content.lines().enumerate() {
            if let Some(m) = re.find(line_text) {
                batch.push(FsGrepMatch {
                    path: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                    text: line_text.to_string(),
                });
                total += 1;
                if batch.len() >= FS_GREP_BATCH_SIZE {
                    on_batch(std::mem::take(&mut batch));
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }
                }
                if total >= max_results {
                    if !batch.is_empty() {
                        on_batch(batch);
                    }
                    return;
                }
            }
        }
    }
    if !batch.is_empty() {
        on_batch(batch);
    }
}

/// Replace all occurrences of `query` with `replacement` across files under `root`.
///
/// - Respects `.gitignore` rules via the `ignore` crate.
/// - Files whose canonicalized path appears in `skip_paths` are skipped (reported in result).
/// - In literal mode (`use_regex=false`), `$` in `replacement` is treated literally.
/// - In regex mode (`use_regex=true`), `$1`, `$2` etc. expand to capture groups.
/// - Only writes back files whose content actually changed.
pub fn replace_in_project(
    root: &Path,
    query: &str,
    replacement: &str,
    options: &SearchOptions,
    skip_paths: &HashSet<PathBuf>,
) -> Result<ReplaceResult, SearchError> {
    if query.is_empty() {
        return Ok(ReplaceResult {
            replacement_count: 0,
            file_count: 0,
            skipped_files: Vec::new(),
            modified_files: Vec::new(),
        });
    }

    let re = build_search_regex(query, options)?;

    let mut result = ReplaceResult {
        replacement_count: 0,
        file_count: 0,
        skipped_files: Vec::new(),
        modified_files: Vec::new(),
    };

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path().to_path_buf();

        // Check skip_paths using canonical path for reliable comparison.
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if skip_paths.contains(&canonical) {
            // Only report as skipped if the file actually has matches.
            if let Ok(content) = fs::read_to_string(&path) {
                if re.is_match(&content) {
                    result.skipped_files.push(path);
                }
            }
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let match_count = re.find_iter(&content).count();
        if match_count == 0 {
            continue;
        }

        // In literal mode, prevent $1 etc. from being interpreted as backreferences.
        let new_content = if options.use_regex {
            re.replace_all(&content, replacement).into_owned()
        } else {
            re.replace_all(&content, regex::NoExpand(replacement))
                .into_owned()
        };

        if new_content != content && fs::write(&path, &new_content).is_ok() {
            result.replacement_count += match_count;
            result.file_count += 1;
            result.modified_files.push(path);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_project(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vimcode_psearch_{}", test_name));
        // Clean up from any previous run
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // file1.txt — has "hello world" on line 0
        let mut f1 = fs::File::create(dir.join("file1.txt")).unwrap();
        writeln!(f1, "hello world").unwrap();
        writeln!(f1, "no match here").unwrap();
        writeln!(f1, "HELLO again").unwrap(); // case-insensitive match

        // subdir/file2.txt — has "Hello" on line 1
        fs::create_dir_all(dir.join("sub")).unwrap();
        let mut f2 = fs::File::create(dir.join("sub/file2.txt")).unwrap();
        writeln!(f2, "nothing").unwrap();
        writeln!(f2, "Hello from sub").unwrap();

        // .hidden/secret.txt — should be skipped
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        let mut fh = fs::File::create(dir.join(".hidden/secret.txt")).unwrap();
        writeln!(fh, "hello hidden").unwrap();

        dir
    }

    #[test]
    fn test_empty_query_returns_nothing() {
        let dir = make_temp_project("empty_query");
        let results = search_in_project(&dir, "", &SearchOptions::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_case_insensitive_match() {
        let dir = make_temp_project("case_insensitive");
        let results = search_in_project(&dir, "hello", &SearchOptions::default()).unwrap();
        // file1.txt lines 0 and 2, sub/file2.txt line 1 — hidden excluded
        assert_eq!(results.len(), 3);
        // Sorted by file path then line
        assert_eq!(results[0].line, 0);
        assert_eq!(results[0].line_text, "hello world");
        assert_eq!(results[1].line, 2);
        assert_eq!(results[1].line_text, "HELLO again");
        assert_eq!(results[2].line, 1);
        assert_eq!(results[2].line_text, "Hello from sub");
    }

    #[test]
    fn test_no_results() {
        let dir = make_temp_project("no_results");
        let results = search_in_project(&dir, "zzznomatch", &SearchOptions::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_hidden_dirs_skipped() {
        let dir = make_temp_project("hidden_dirs");
        let results = search_in_project(&dir, "hidden", &SearchOptions::default()).unwrap();
        assert!(
            results.is_empty(),
            "hidden directory should be skipped, got: {:?}",
            results
        );
    }

    #[test]
    fn test_col_offset() {
        let dir = make_temp_project("col_offset");
        let results = search_in_project(&dir, "world", &SearchOptions::default()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].col, 6); // "hello " = 6 bytes
    }

    // ── New tests for SearchOptions ──────────────────────────────────────

    #[test]
    fn test_case_sensitive_search() {
        let dir = make_temp_project("case_sensitive");
        let opts = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let results = search_in_project(&dir, "hello", &opts).unwrap();
        // Only "hello world" matches — "HELLO again" and "Hello from sub" are excluded
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_text, "hello world");
    }

    #[test]
    fn test_whole_word_search() {
        let dir = make_temp_project("whole_word");
        // "hello" should NOT match "helloworld" (if it were present)
        // but SHOULD match "hello world" (word boundary)
        // Add a file with "helloworld" concatenated
        let mut f = fs::File::create(dir.join("concat.txt")).unwrap();
        writeln!(f, "helloworld joined").unwrap();
        writeln!(f, "hello world apart").unwrap();

        let opts = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let results = search_in_project(&dir, "hello", &opts).unwrap();
        // "helloworld joined" should NOT match whole word
        for r in &results {
            assert!(
                !r.line_text.contains("helloworld"),
                "whole word should not match 'helloworld', got: {}",
                r.line_text
            );
        }
        // "hello world apart" should match
        assert!(
            results.iter().any(|r| r.line_text == "hello world apart"),
            "whole word should match 'hello world apart'"
        );
    }

    #[test]
    fn test_regex_search() {
        let dir = make_temp_project("regex_search");
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        // Use regex pattern that matches "hello" followed by any whitespace + word
        let results = search_in_project(&dir, r"hello\s+\w+", &opts).unwrap();
        assert!(!results.is_empty(), "regex should find matches");
        // All results should contain "hello" followed by space(s) + word chars
        for r in &results {
            let lower = r.line_text.to_lowercase();
            assert!(
                lower.contains("hello ") || lower.contains("hello\t"),
                "regex match should contain hello + whitespace: {}",
                r.line_text
            );
        }
    }

    #[test]
    fn test_invalid_regex_returns_error() {
        let dir = make_temp_project("invalid_regex");
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let result = search_in_project(&dir, "[bad", &opts);
        assert!(result.is_err(), "invalid regex should return Err");
        let err = result.unwrap_err();
        assert!(!err.0.is_empty(), "error message should be non-empty");
    }

    #[test]
    fn test_whole_word_regex_combo() {
        let dir = make_temp_project("word_regex");
        let mut f = fs::File::create(dir.join("combo.txt")).unwrap();
        writeln!(f, "testing test tested").unwrap();

        let opts = SearchOptions {
            use_regex: true,
            whole_word: true,
            ..Default::default()
        };
        let results = search_in_project(&dir, "test", &opts).unwrap();
        // "test" as whole word should match "test" but NOT "testing" or "tested"
        // The line "testing test tested" contains whole-word "test" so it matches
        assert!(
            results
                .iter()
                .any(|r| r.line_text.contains("testing test tested")),
            "should match line containing whole-word 'test'"
        );
    }

    #[test]
    fn test_gitignore_respected() {
        let dir = make_temp_project("gitignore");
        // Initialize a git repo so the ignore crate honours .gitignore
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("git init");
        // Create a .gitignore that ignores "ignored_dir/"
        let mut gi = fs::File::create(dir.join(".gitignore")).unwrap();
        writeln!(gi, "ignored_dir/").unwrap();
        fs::create_dir_all(dir.join("ignored_dir")).unwrap();
        let mut fi = fs::File::create(dir.join("ignored_dir/data.txt")).unwrap();
        writeln!(fi, "hello from ignored").unwrap();

        let results =
            search_in_project(&dir, "hello from ignored", &SearchOptions::default()).unwrap();
        assert!(
            results.is_empty(),
            ".gitignore should exclude ignored_dir/, got: {:?}",
            results
        );
    }

    // ── Streaming walk/grep tests (#1806) ───────────────────────────────

    /// Build a temp project with `n` plain files (`f0000.txt`..) directly
    /// under `dir`, each containing the single line `needle`. Used by the
    /// batching/cancel tests below, which need enough files to force
    /// several batches.
    fn make_streaming_project(test_name: &str, n: usize, needle: &str) -> PathBuf {
        // pid+thread-id suffixed (not just `test_name`) so two concurrent
        // `cargo test` *processes* (as opposed to the threads within one,
        // which `test_name` alone already disambiguates) don't fight over
        // the same directory (#1806 review).
        let dir = std::env::temp_dir().join(format!(
            "vimcode_fswalk_{test_name}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            let mut f = fs::File::create(dir.join(format!("f{i:04}.txt"))).unwrap();
            writeln!(f, "{needle}").unwrap();
        }
        dir
    }

    #[test]
    fn fs_walk_streaming_respects_gitignore_1806() {
        let dir = make_temp_project("fs_walk_gitignore_1806");
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("git init");
        let mut gi = fs::File::create(dir.join(".gitignore")).unwrap();
        writeln!(gi, "ignored_dir/").unwrap();
        fs::create_dir_all(dir.join("ignored_dir")).unwrap();
        fs::write(dir.join("ignored_dir/data.txt"), "hello from ignored").unwrap();

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let overrides = build_fs_overrides(&dir, &[], &[]).unwrap();
        let mut seen: Vec<PathBuf> = Vec::new();
        walk_project_streaming(
            &dir,
            &FsWalkOptions::default(),
            overrides,
            &cancelled,
            |batch| seen.extend(batch),
        );

        assert!(
            !seen
                .iter()
                .any(|p| p.to_string_lossy().contains("ignored_dir")),
            ".gitignore should exclude ignored_dir/, got: {:?}",
            seen
        );
        assert!(
            seen.iter().any(|p| p.ends_with("file1.txt")),
            "a non-ignored file must still be walked, got: {:?}",
            seen
        );
    }

    /// #1806 review: `walk_project_streaming` must apply
    /// `FsWalkOptions::explorer_exclude` the same way `Engine::
    /// picker_populate_files` applies `Settings::explorer_exclude` — pruning
    /// a matching directory (here `.git`, via the real default pattern
    /// `**/.git`) rather than descending into it. Uses `hidden: true` so a
    /// walk that *only* skipped dotfiles (the pre-fix behaviour) would still
    /// wrongly surface `.git`'s contents, isolating this from
    /// `fs_walk_streaming_respects_gitignore_1806` above (which never sets
    /// `hidden`).
    ///
    /// RED-verified by hand: with `FsWalkOptions::explorer_exclude` left
    /// empty (the pre-fix call site, which never populated it), this test
    /// fails — `seen` contains `.git/tracked.txt`.
    #[test]
    fn fs_walk_streaming_respects_explorer_exclude_1806() {
        let dir = make_temp_project("fs_walk_explorer_exclude_1806");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git/tracked.txt"), "not a real git object").unwrap();

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let overrides = build_fs_overrides(&dir, &[], &[]).unwrap();
        let opts = FsWalkOptions {
            hidden: true,
            explorer_exclude: vec!["**/.git".to_string()],
            ..Default::default()
        };
        let mut seen: Vec<PathBuf> = Vec::new();
        walk_project_streaming(&dir, &opts, overrides, &cancelled, |batch| {
            seen.extend(batch)
        });

        assert!(
            !seen
                .iter()
                .any(|p| p.components().any(|c| c.as_os_str() == ".git")),
            "explorer_exclude=**/.git must prune .git/ even with hidden=true, got: {:?}",
            seen
        );
        assert!(
            seen.iter().any(|p| p.ends_with(".hidden/secret.txt")),
            "hidden=true must still surface other dotfiles/dirs not matched by \
             explorer_exclude, got: {:?}",
            seen
        );
    }

    #[test]
    fn fs_walk_streaming_streams_in_batches_1806() {
        // More than two full batches, so the final (partial) flush is also
        // exercised.
        let n = FS_WALK_BATCH_SIZE * 2 + 17;
        let dir = make_streaming_project("fs_walk_batches_1806", n, "x");

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let overrides = build_fs_overrides(&dir, &[], &[]).unwrap();
        let mut batch_sizes: Vec<usize> = Vec::new();
        walk_project_streaming(
            &dir,
            &FsWalkOptions::default(),
            overrides,
            &cancelled,
            |batch| batch_sizes.push(batch.len()),
        );

        assert!(
            batch_sizes.len() >= 3,
            "{n} files at a batch size of {FS_WALK_BATCH_SIZE} must stream in at least 3 \
             batches, got batch sizes {:?}",
            batch_sizes
        );
        assert!(
            batch_sizes.iter().all(|&len| len <= FS_WALK_BATCH_SIZE),
            "no batch may exceed FS_WALK_BATCH_SIZE, got {:?}",
            batch_sizes
        );
        assert_eq!(
            batch_sizes.iter().sum::<usize>(),
            n,
            "every file must be delivered exactly once across all batches"
        );
    }

    #[test]
    fn fs_walk_streaming_cancel_stops_delivery_1806() {
        // Enough files that, uncancelled, this would stream several batches.
        let n = FS_WALK_BATCH_SIZE * 3;
        let dir = make_streaming_project("fs_walk_cancel_1806", n, "x");

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let overrides = build_fs_overrides(&dir, &[], &[]).unwrap();
        let mut batches_received = 0usize;
        walk_project_streaming(
            &dir,
            &FsWalkOptions::default(),
            overrides,
            &cancelled,
            |_| {
                batches_received += 1;
                // Simulate a plugin cancelling the handle as soon as the first
                // batch arrives (the finder-re-queries-on-every-keystroke case
                // the issue calls out) — set the same flag `:cancel()` sets.
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
            },
        );

        assert_eq!(
            batches_received, 1,
            "cancelling after the first batch must stop delivery promptly — no second batch \
             may arrive, even though {n} files remain unwalked"
        );
    }

    #[test]
    fn fs_grep_streaming_batches_and_respects_max_results_1806() {
        const NEEDLE: &str = "zqxwfsgrep1806";
        let total_matches = FS_GREP_BATCH_SIZE * 2 + 13;
        let dir = make_streaming_project("fs_grep_batches_1806", total_matches, NEEDLE);
        let max_results = FS_GREP_BATCH_SIZE + 5;

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let opts = FsGrepOptions {
            max_results,
            ..Default::default()
        };
        let (re, overrides) = validate_fs_grep_options(&dir, NEEDLE, &opts).unwrap();
        let mut batch_sizes: Vec<usize> = Vec::new();
        grep_project_streaming(&dir, &re, overrides, max_results, &cancelled, |batch| {
            batch_sizes.push(batch.len())
        });

        assert!(
            batch_sizes.len() >= 2,
            "{max_results} matches at a batch size of {FS_GREP_BATCH_SIZE} must stream in at \
             least 2 batches, got {:?}",
            batch_sizes
        );
        assert_eq!(
            batch_sizes.iter().sum::<usize>(),
            max_results,
            "max_results must cap the total delivered, not just truncate the last batch"
        );
    }

    #[test]
    fn fs_grep_streaming_cancel_stops_delivery_1806() {
        const NEEDLE: &str = "zqxwfsgrepcancel1806";
        let total_matches = FS_GREP_BATCH_SIZE * 3;
        let dir = make_streaming_project("fs_grep_cancel_1806", total_matches, NEEDLE);

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let opts = FsGrepOptions::default();
        let (re, overrides) = validate_fs_grep_options(&dir, NEEDLE, &opts).unwrap();
        let mut batches_received = 0usize;
        grep_project_streaming(&dir, &re, overrides, opts.max_results, &cancelled, |_| {
            batches_received += 1;
            cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        assert_eq!(
            batches_received, 1,
            "cancelling after the first batch must stop delivery promptly — no second batch \
             may arrive, even though {total_matches} matches remain unwalked"
        );
    }

    #[test]
    fn fs_walk_streaming_include_exclude_globs_1806() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_fswalk_globs_1806_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("keep.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("skip.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("other.txt"), "not rust").unwrap();

        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let overrides =
            build_fs_overrides(&dir, &["*.rs".to_string()], &["skip.rs".to_string()]).unwrap();
        let mut seen: Vec<PathBuf> = Vec::new();
        walk_project_streaming(
            &dir,
            &FsWalkOptions::default(),
            overrides,
            &cancelled,
            |batch| seen.extend(batch),
        );

        let names: Vec<String> = seen
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["keep.rs".to_string()],
            "include=*.rs must drop other.txt, exclude=skip.rs must still drop skip.rs: {:?}",
            names
        );
    }

    // ── Replace tests ────────────────────────────────────────────────────

    fn make_replace_project(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vimcode_preplace_{}", test_name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_replace_basic() {
        let dir = make_replace_project("basic");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "hello world").unwrap();
        writeln!(f, "hello again").unwrap();
        drop(f);

        let rr = replace_in_project(
            &dir,
            "hello",
            "hi",
            &SearchOptions::default(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 2);
        assert_eq!(rr.file_count, 1);
        assert!(rr.skipped_files.is_empty());
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("hi world"));
        assert!(content.contains("hi again"));
        assert!(!content.contains("hello"));
    }

    #[test]
    fn test_replace_case_insensitive() {
        let dir = make_replace_project("case_insensitive");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "Hello World").unwrap();
        writeln!(f, "HELLO AGAIN").unwrap();
        drop(f);

        let rr = replace_in_project(
            &dir,
            "hello",
            "hi",
            &SearchOptions::default(), // case_sensitive=false
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 2);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("hi World"));
        assert!(content.contains("hi AGAIN"));
    }

    #[test]
    fn test_replace_whole_word() {
        let dir = make_replace_project("whole_word");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "helloworld hello").unwrap();
        drop(f);

        let opts = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let rr = replace_in_project(&dir, "hello", "hi", &opts, &HashSet::new()).unwrap();
        assert_eq!(rr.replacement_count, 1);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("helloworld hi"));
    }

    #[test]
    fn test_replace_regex_capture_groups() {
        let dir = make_replace_project("regex_capture");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "foo_old bar_old").unwrap();
        drop(f);

        let opts = SearchOptions {
            use_regex: true,
            case_sensitive: true,
            ..Default::default()
        };
        let rr =
            replace_in_project(&dir, r"(\w+)_old", "${1}_new", &opts, &HashSet::new()).unwrap();
        assert_eq!(rr.replacement_count, 2);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("foo_new bar_new"));
    }

    #[test]
    fn test_replace_literal_dollar_sign() {
        let dir = make_replace_project("literal_dollar");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "price is 10").unwrap();
        drop(f);

        // In literal mode (use_regex=false), $1 in replacement should be literal.
        let rr = replace_in_project(
            &dir,
            "10",
            "$1.00",
            &SearchOptions::default(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 1);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(
            content.contains("$1.00"),
            "literal $1 should not be interpreted as backreference, got: {}",
            content
        );
    }

    #[test]
    fn test_replace_skip_dirty_files() {
        let dir = make_replace_project("skip_dirty");
        let path = dir.join("a.txt");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "hello world").unwrap();
        drop(f);

        let canonical = path.canonicalize().unwrap();
        let mut skip = HashSet::new();
        skip.insert(canonical);

        let rr = replace_in_project(&dir, "hello", "hi", &SearchOptions::default(), &skip).unwrap();
        assert_eq!(rr.replacement_count, 0);
        assert_eq!(rr.file_count, 0);
        assert_eq!(rr.skipped_files.len(), 1);
        // File should be unchanged
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_replace_empty_query() {
        let dir = make_replace_project("empty_query");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "hello").unwrap();
        drop(f);

        let rr =
            replace_in_project(&dir, "", "hi", &SearchOptions::default(), &HashSet::new()).unwrap();
        assert_eq!(rr.replacement_count, 0);
        assert_eq!(rr.file_count, 0);
    }

    #[test]
    fn test_replace_invalid_regex() {
        let dir = make_replace_project("invalid_regex");
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let result = replace_in_project(&dir, "[bad", "x", &opts, &HashSet::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_gitignore_respected() {
        let dir = make_replace_project("gitignore");
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("git init");
        let mut gi = fs::File::create(dir.join(".gitignore")).unwrap();
        writeln!(gi, "ignored/").unwrap();
        drop(gi);
        fs::create_dir_all(dir.join("ignored")).unwrap();
        let path = dir.join("ignored/data.txt");
        let mut fi = fs::File::create(&path).unwrap();
        writeln!(fi, "hello world").unwrap();
        drop(fi);

        let rr = replace_in_project(
            &dir,
            "hello",
            "hi",
            &SearchOptions::default(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 0);
        // File should be unchanged
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello world"));
    }
}
