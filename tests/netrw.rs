mod common;
use common::*;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Create a temp directory with known structure for testing.
fn make_test_dir() -> std::path::PathBuf {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("vimcode_netrw_{}_{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("subdir")).unwrap();
    fs::create_dir_all(dir.join("another")).unwrap();
    fs::write(dir.join("alpha.txt"), "alpha content\n").unwrap();
    fs::write(dir.join("beta.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.join(".hidden"), "secret\n").unwrap();
    fs::write(dir.join("subdir/inner.txt"), "inner\n").unwrap();
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

// ── :Explore opens listing ──────────────────────────────────────────────

#[test]
fn explore_opens_listing() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));
    let content = buf(&e);

    // Header line
    assert!(content.starts_with("\" "), "should start with header");
    assert!(content.contains(&format!("{}/", dir.display())));

    // Should have ../
    assert!(content.contains("../"));

    // Directories and files present
    assert!(content.contains("subdir/"));
    assert!(content.contains("another/"));
    assert!(content.contains("alpha.txt"));
    assert!(content.contains("beta.rs"));

    cleanup(&dir);
}

// ── :Explore with dir arg ───────────────────────────────────────────────

#[test]
fn explore_with_dir_arg() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.join("subdir").display()));
    let content = buf(&e);

    assert!(content.contains("inner.txt"));
    assert!(!content.contains("alpha.txt")); // parent dir files not shown

    cleanup(&dir);
}

// ── :Explore defaults to file's parent dir ──────────────────────────────

#[test]
fn explore_defaults_to_file_dir() {
    let dir = make_test_dir();
    let file_path = dir.join("alpha.txt");
    let mut e = engine_with("");
    e.cwd = std::path::PathBuf::from("/tmp"); // different from file's dir

    // Open the file so the engine knows about it
    e.open_file_in_tab(&file_path);

    exec(&mut e, "Explore");
    let content = buf(&e);

    // Should show directory listing of the file's parent
    assert!(content.contains("alpha.txt"));
    assert!(content.contains("beta.rs"));

    cleanup(&dir);
}

// ── :Explore defaults to cwd when no file ───────────────────────────────

#[test]
fn explore_defaults_to_cwd() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, "Explore");
    let content = buf(&e);

    assert!(content.contains("alpha.txt"));
    assert!(content.contains("subdir/"));

    cleanup(&dir);
}

// ── :Vexplore creates vsplit ────────────────────────────────────────────

#[test]
fn vexplore_creates_vsplit() {
    let dir = make_test_dir();
    let mut e = engine_with("original\n");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Vexplore {}", dir.display()));

    // Should have 2 windows now
    assert!(e.windows.len() >= 2, "should have at least 2 windows");

    // Active window should be netrw
    let content = buf(&e);
    assert!(content.contains("../"), "active window should show netrw");

    cleanup(&dir);
}

// ── :Sexplore creates hsplit ────────────────────────────────────────────

#[test]
fn sexplore_creates_hsplit() {
    let dir = make_test_dir();
    let mut e = engine_with("original\n");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Sexplore {}", dir.display()));

    assert!(e.windows.len() >= 2, "should have at least 2 windows");

    let content = buf(&e);
    assert!(content.contains("../"), "active window should show netrw");

    cleanup(&dir);
}

// ── Enter on directory navigates ────────────────────────────────────────

#[test]
fn enter_on_directory_navigates() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));

    // Find the line with "subdir/"
    let lines = get_lines(&e);
    let subdir_line = lines
        .iter()
        .position(|l| l.trim() == "subdir/")
        .expect("should find subdir/");

    // Move cursor to subdir line
    e.view_mut().cursor.line = subdir_line;
    e.view_mut().cursor.col = 0;

    // Press Enter
    press_key(&mut e, "Return");

    let content = buf(&e);
    // Should now show subdir contents
    assert!(content.contains("inner.txt"), "should show subdir contents");
    assert!(
        !content.contains("alpha.txt"),
        "should not show parent files"
    );

    cleanup(&dir);
}

// ── Enter on file opens file ────────────────────────────────────────────

#[test]
fn enter_on_file_opens_file() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));

    // Find the line with "alpha.txt"
    let lines = get_lines(&e);
    let file_line = lines
        .iter()
        .position(|l| l.trim() == "alpha.txt")
        .expect("should find alpha.txt");

    e.view_mut().cursor.line = file_line;
    e.view_mut().cursor.col = 0;

    press_key(&mut e, "Return");

    let content = buf(&e);
    assert_eq!(content, "alpha content\n", "should open the file content");

    // Should no longer be a netrw buffer
    assert!(
        e.active_buffer_state().netrw_dir.is_none(),
        "should not be netrw anymore"
    );

    cleanup(&dir);
}

// ── Minus goes to parent ────────────────────────────────────────────────

#[test]
fn minus_goes_to_parent() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    // Open subdir
    exec(&mut e, &format!("Explore {}", dir.join("subdir").display()));
    let content = buf(&e);
    assert!(content.contains("inner.txt"));

    // Press -
    press(&mut e, '-');

    let content = buf(&e);
    // Should now show parent directory
    assert!(
        content.contains("alpha.txt"),
        "should show parent dir after -"
    );
    assert!(content.contains("subdir/"));

    cleanup(&dir);
}

// ── Netrw is read-only ──────────────────────────────────────────────────

#[test]
fn netrw_is_read_only() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));

    // Try to enter insert mode
    press(&mut e, 'i');
    assert_msg_contains(&e, "read-only");

    cleanup(&dir);
}

// ── Respects show_hidden setting ────────────────────────────────────────

#[test]
fn respects_show_hidden() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    // Hidden files off (default)
    e.settings.show_hidden_files = false;
    exec(&mut e, &format!("Explore {}", dir.display()));
    let content = buf(&e);
    assert!(
        !content.contains(".hidden"),
        "should not show hidden files when show_hidden_files=false"
    );

    // Hidden files on
    e.settings.show_hidden_files = true;
    exec(&mut e, &format!("Explore {}", dir.display()));
    let content = buf(&e);
    assert!(
        content.contains(".hidden"),
        "should show hidden files when show_hidden_files=true"
    );

    cleanup(&dir);
}

// ── Dirs before files, both sorted ──────────────────────────────────────

#[test]
fn dirs_before_files() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));
    let lines = get_lines(&e);

    // Skip header (line 0) and ../ (line 1)
    let entries: Vec<&str> = lines.iter().skip(2).map(|l| l.trim()).collect();

    // Find boundary between dirs and files
    let first_file_idx = entries
        .iter()
        .position(|e| !e.ends_with('/'))
        .unwrap_or(entries.len());

    // All items before first_file_idx should be dirs
    for (i, entry) in entries.iter().enumerate() {
        if entry.is_empty() {
            continue;
        }
        if i < first_file_idx {
            assert!(
                entry.ends_with('/'),
                "expected dir at position {i}: {entry}"
            );
        } else {
            assert!(
                !entry.ends_with('/'),
                "expected file at position {i}: {entry}"
            );
        }
    }

    // Dirs should be sorted
    let dirs: Vec<&&str> = entries[..first_file_idx]
        .iter()
        .filter(|e| !e.is_empty())
        .collect();
    let mut sorted_dirs = dirs.clone();
    sorted_dirs.sort();
    assert_eq!(dirs, sorted_dirs, "directories should be sorted");

    // Files should be sorted
    let files: Vec<&&str> = entries[first_file_idx..]
        .iter()
        .filter(|e| !e.is_empty())
        .collect();
    let mut sorted_files = files.clone();
    sorted_files.sort();
    assert_eq!(files, sorted_files, "files should be sorted");

    cleanup(&dir);
}

// ── Header line ─────────────────────────────────────────────────────────

#[test]
fn header_line() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));
    let lines = get_lines(&e);

    assert!(
        lines[0].starts_with("\" "),
        "header should start with quote-space"
    );
    assert!(
        lines[0].contains(&dir.to_string_lossy().to_string()),
        "header should contain directory path"
    );

    cleanup(&dir);
}

// ── Enter on header is no-op ────────────────────────────────────────────

#[test]
fn enter_on_header_noop() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.display()));

    // Move to header line
    e.view_mut().cursor.line = 0;
    e.view_mut().cursor.col = 0;

    let before = buf(&e);
    press_key(&mut e, "Return");
    let after = buf(&e);

    assert_eq!(before, after, "Enter on header should be no-op");

    cleanup(&dir);
}

// ── Abbreviations work ──────────────────────────────────────────────────

#[test]
fn abbreviations() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    // :Ex
    exec(&mut e, &format!("Ex {}", dir.display()));
    assert!(buf(&e).contains("../"), ":Ex should work");

    // :Sex (need new buffer context)
    let mut e2 = engine_with("text\n");
    e2.cwd = dir.clone();
    exec(&mut e2, &format!("Sex {}", dir.display()));
    assert!(buf(&e2).contains("../"), ":Sex should work");

    // :Vex
    let mut e3 = engine_with("text\n");
    e3.cwd = dir.clone();
    exec(&mut e3, &format!("Vex {}", dir.display()));
    assert!(buf(&e3).contains("../"), ":Vex should work");

    cleanup(&dir);
}

// ── Enter on ../ goes to parent ─────────────────────────────────────────

#[test]
fn enter_on_dotdot_goes_to_parent() {
    let dir = make_test_dir();
    let mut e = engine_with("");
    e.cwd = dir.clone();

    exec(&mut e, &format!("Explore {}", dir.join("subdir").display()));

    // Cursor should be on line 1 (../)
    assert_eq!(e.cursor().line, 1);
    let lines = get_lines(&e);
    assert_eq!(lines[1].trim(), "../");

    press_key(&mut e, "Return");

    let content = buf(&e);
    assert!(
        content.contains("alpha.txt"),
        "Enter on ../ should go to parent"
    );

    cleanup(&dir);
}
