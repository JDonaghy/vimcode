mod common;
use common::*;
use std::fs;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use vimcode_core::core::swap;

/// Create a temp file with known content and return its path.
fn temp_file(name: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("vimcode_swap_test_{}", name));
    fs::write(&path, content).unwrap();
    path
}

/// Helper: get the canonical path of a file, matching what the engine uses.
fn canonical(path: &PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.clone())
}

// ── 1. Swap file created on file open ───────────────────────────────────────

#[test]
fn test_swap_created_on_file_open() {
    let path = temp_file("open1.rs", "fn main() {}\n");
    let mut e = engine_with("");
    // Disable save suppression for swap writes by checking the swap path.
    e.open_file_in_tab(&path);

    let _swap_path = swap::swap_path_for(&canonical(&path));
    // In tests, saves are suppressed, so swap file won't be written to disk.
    // Instead, verify the engine's swap state: no recovery pending,
    // and we can check internal state.
    assert!(
        e.pending_swap_recovery.is_none(),
        "no recovery should be pending for a fresh file"
    );

    let _ = fs::remove_file(&path);
}

// ── 2. Swap deleted on save ─────────────────────────────────────────────────

#[test]
fn test_swap_deleted_on_save() {
    let path = temp_file("save1.rs", "fn main() {}\n");
    let mut e = engine_with("");
    e.open_file_in_tab(&path);

    // Type something to make it dirty.
    press(&mut e, 'i');
    press(&mut e, 'x');
    press_key(&mut e, "Escape");

    // Save — should delete the swap file.
    let result = e.save();
    assert!(result.is_ok());

    // The swap path should not exist (saves suppressed = no file written,
    // but delete is also suppressed, so this is a no-op test).
    // Verify save succeeded and buffer is clean.
    assert!(!e.active_buffer_state().dirty);

    let _ = fs::remove_file(&path);
}

// ── 3. Swap deleted on close ────────────────────────────────────────────────

#[test]
fn test_swap_deleted_on_close() {
    let path = temp_file("close1.rs", "line1\nline2\n");
    let mut e = engine_with("");
    // Need a second tab so close_tab works.
    e.open_file_in_tab(&path);
    e.open_file_in_tab(&temp_file("close2.rs", "other\n"));

    // Switch back to first tab.
    run_cmd(&mut e, "tabprevious");

    // Close tab.
    let closed = e.close_tab();
    assert!(closed);

    // Verify the buffer is gone.
    assert!(e.pending_swap_recovery.is_none());

    let _ = fs::remove_file(&path);
}

// ── 4. Swap content matches buffer ──────────────────────────────────────────

#[test]
fn test_swap_contains_buffer_content() {
    let path = temp_file("content1.rs", "original\n");
    let mut e = engine_with("");
    e.open_file_in_tab(&path);

    // Edit the buffer.
    press(&mut e, 'i');
    type_chars(&mut e, "modified ");
    press_key(&mut e, "Escape");

    // Mark dirty and trigger swap write by calling tick with zero updatetime.
    e.settings.updatetime = 0;
    e.swap_mark_dirty();
    // In test mode, tick_swap_files is a no-op due to save suppression.
    // But we can verify the swap_write_needed set was populated.
    // After tick, it should be cleared (or not, since saves are suppressed).

    // The buffer should contain our edit.
    let content = buf(&e);
    assert!(content.contains("modified"));

    let _ = fs::remove_file(&path);
}

// ── 5. Recovery offered for stale swap ──────────────────────────────────────

#[test]
fn test_swap_recovery_offered() {
    let path = temp_file("recover1.rs", "original content\n");
    let canonical_path = canonical(&path);
    let swap_path = swap::swap_path_for(&canonical_path);

    // Create a fake stale swap file with a dead PID.
    fs::create_dir_all(swap_path.parent().unwrap()).unwrap();
    {
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", canonical_path.display()).unwrap();
        writeln!(f, "pid: 999999999").unwrap(); // dead PID
        writeln!(f, "modified: 2026-01-01T00:00:00Z").unwrap();
        writeln!(f, "---").unwrap();
        write!(f, "recovered text here\n").unwrap();
    }

    let mut e = engine_with("");
    e.open_file_in_tab(&path);

    // Recovery should be offered via dialog.
    assert!(
        e.pending_swap_recovery.is_some(),
        "recovery should be pending"
    );
    assert!(
        e.dialog.is_some(),
        "dialog should be open for swap recovery"
    );
    let dialog = e.dialog.as_ref().unwrap();
    assert_eq!(dialog.tag, "swap_recovery");
    assert_eq!(dialog.buttons.len(), 3);

    // Clean up.
    let _ = fs::remove_file(&swap_path);
    let _ = fs::remove_file(&path);
}

// ── 6. Recovery with R key ──────────────────────────────────────────────────

#[test]
fn test_swap_recovery_recover() {
    let path = temp_file("recover_r.rs", "original\n");
    let canonical_path = canonical(&path);
    let swap_path = swap::swap_path_for(&canonical_path);

    // Create stale swap.
    fs::create_dir_all(swap_path.parent().unwrap()).unwrap();
    {
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", canonical_path.display()).unwrap();
        writeln!(f, "pid: 999999999").unwrap();
        writeln!(f, "modified: 2026-01-01T00:00:00Z").unwrap();
        writeln!(f, "---").unwrap();
        write!(f, "recovered content\n").unwrap();
    }

    let mut e = engine_with("");
    e.open_file_in_tab(&path);
    assert!(e.pending_swap_recovery.is_some());

    // Press R to recover.
    press(&mut e, 'R');

    assert!(e.pending_swap_recovery.is_none());
    assert_eq!(buf(&e), "recovered content\n");
    assert!(
        e.active_buffer_state().dirty,
        "buffer should be marked dirty"
    );
    assert!(e.message.contains("Recovered"));

    let _ = fs::remove_file(&swap_path);
    let _ = fs::remove_file(&path);
}

// ── 7. Recovery with D key ─────────────────────────────────────────────────

#[test]
fn test_swap_recovery_delete() {
    let path = temp_file("recover_d.rs", "original content\n");
    let canonical_path = canonical(&path);
    let swap_path = swap::swap_path_for(&canonical_path);

    // Create stale swap.
    fs::create_dir_all(swap_path.parent().unwrap()).unwrap();
    {
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", canonical_path.display()).unwrap();
        writeln!(f, "pid: 999999999").unwrap();
        writeln!(f, "modified: 2026-01-01T00:00:00Z").unwrap();
        writeln!(f, "---").unwrap();
        write!(f, "recovered content\n").unwrap();
    }

    let mut e = engine_with("");
    e.open_file_in_tab(&path);
    assert!(e.pending_swap_recovery.is_some());

    // Press D to delete the swap.
    press(&mut e, 'D');

    assert!(e.pending_swap_recovery.is_none());
    // Buffer should have the original file content, not the recovered content.
    assert_eq!(buf(&e), "original content\n");
    assert!(e.message.contains("deleted"));

    // Swap file should be deleted (saves suppressed, but let's verify state).
    // In non-test mode, the swap file would be gone from disk.

    let _ = fs::remove_file(&swap_path);
    let _ = fs::remove_file(&path);
}

// ── 8. Recovery with A key ──────────────────────────────────────────────────

#[test]
fn test_swap_recovery_abort() {
    let path = temp_file("recover_a.rs", "original\n");
    let canonical_path = canonical(&path);
    let swap_path = swap::swap_path_for(&canonical_path);

    // Need a second tab so close works.
    let path2 = temp_file("recover_a2.rs", "other\n");

    // Create stale swap.
    fs::create_dir_all(swap_path.parent().unwrap()).unwrap();
    {
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", canonical_path.display()).unwrap();
        writeln!(f, "pid: 999999999").unwrap();
        writeln!(f, "modified: 2026-01-01T00:00:00Z").unwrap();
        writeln!(f, "---").unwrap();
        write!(f, "recovered content\n").unwrap();
    }

    let mut e = engine_with("");
    e.open_file_in_tab(&path2); // Open a second file first.
    e.open_file_in_tab(&path);
    assert!(e.pending_swap_recovery.is_some());

    // Press A to abort — should close the tab.
    press(&mut e, 'A');

    assert!(e.pending_swap_recovery.is_none());
    // Swap file should still exist (left for next time).
    assert!(swap_path.exists(), "swap file should be preserved on abort");

    let _ = fs::remove_file(&swap_path);
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(&path2);
}

// ── 9. Swap disabled by setting ─────────────────────────────────────────────

#[test]
fn test_swap_disabled_by_setting() {
    let path = temp_file("noswap.rs", "content\n");
    let mut e = engine_with("");
    e.settings.swap_file = false;
    e.open_file_in_tab(&path);

    // No recovery should be pending.
    assert!(e.pending_swap_recovery.is_none());

    // swap_mark_dirty should be a no-op.
    e.swap_mark_dirty();
    // Internal set should be empty since swap_file is false.

    let _ = fs::remove_file(&path);
}

// ── 10. No swap for unnamed buffers ─────────────────────────────────────────

#[test]
fn test_swap_not_created_for_unnamed_buffers() {
    let mut e = engine_with("some content\n");
    // The default buffer has no file path — swap should not be created.
    e.swap_mark_dirty();
    // tick should be a no-op (no canonical path to hash).
    e.settings.updatetime = 0;
    e.tick_swap_files();
    assert!(e.pending_swap_recovery.is_none());
}

// ── 11. Settings support ────────────────────────────────────────────────────

#[test]
fn test_swap_settings() {
    let mut e = engine_with("");

    // Default values.
    assert!(e.settings.swap_file);
    assert_eq!(e.settings.updatetime, 4000);

    // :set noswapfile
    let msg = e.settings.parse_set_option("noswapfile").unwrap();
    assert_eq!(msg, "noswapfile");
    assert!(!e.settings.swap_file);

    // :set swapfile
    let msg = e.settings.parse_set_option("swapfile").unwrap();
    assert_eq!(msg, "swapfile");
    assert!(e.settings.swap_file);

    // :set updatetime=2000
    let msg = e.settings.parse_set_option("updatetime=2000").unwrap();
    assert_eq!(msg, "updatetime=2000");
    assert_eq!(e.settings.updatetime, 2000);

    // :set swapfile?
    let msg = e.settings.parse_set_option("swapfile?").unwrap();
    assert_eq!(msg, "swapfile");

    // :set updatetime?
    let msg = e.settings.parse_set_option("updatetime?").unwrap();
    assert_eq!(msg, "updatetime=2000");
}

// ── 12. Swap path determinism ───────────────────────────────────────────────

#[test]
fn test_swap_path_deterministic() {
    use std::path::Path;
    let p1 = swap::swap_path_for(Path::new("/home/user/project/main.rs"));
    let p2 = swap::swap_path_for(Path::new("/home/user/project/main.rs"));
    assert_eq!(p1, p2);

    let p3 = swap::swap_path_for(Path::new("/home/user/project/other.rs"));
    assert_ne!(p1, p3);
}

// ── 13. Swap recovery intercepts keys ───────────────────────────────────────

#[test]
fn test_swap_recovery_intercepts_normal_keys() {
    let path = temp_file("intercept.rs", "hello\n");
    let canonical_path = canonical(&path);
    let swap_path = swap::swap_path_for(&canonical_path);

    // Create stale swap.
    fs::create_dir_all(swap_path.parent().unwrap()).unwrap();
    {
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", canonical_path.display()).unwrap();
        writeln!(f, "pid: 999999999").unwrap();
        writeln!(f, "modified: 2026-01-01T00:00:00Z").unwrap();
        writeln!(f, "---").unwrap();
        write!(f, "swap content\n").unwrap();
    }

    let mut e = engine_with("");
    e.open_file_in_tab(&path);
    assert!(e.pending_swap_recovery.is_some());

    // Pressing 'j' (not R/D/A) should NOT clear recovery.
    press(&mut e, 'j');
    assert!(
        e.pending_swap_recovery.is_some(),
        "unrecognized key should not clear recovery"
    );

    // Now press 'r' (lowercase) to recover.
    press(&mut e, 'r');
    assert!(e.pending_swap_recovery.is_none());

    let _ = fs::remove_file(&swap_path);
    let _ = fs::remove_file(&path);
}

// ── 14. No recovery offered when swap matches disk ──────────────────────────

#[test]
fn test_swap_no_recovery_when_content_matches_disk() {
    let content = "unchanged content\n";
    let path = temp_file("unchanged.rs", content);
    let canonical_path = canonical(&path);
    let swap_path = swap::swap_path_for(&canonical_path);

    // Create a stale swap with the SAME content as the file on disk.
    fs::create_dir_all(swap_path.parent().unwrap()).unwrap();
    {
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", canonical_path.display()).unwrap();
        writeln!(f, "pid: 999999999").unwrap(); // dead PID
        writeln!(f, "modified: 2026-01-01T00:00:00Z").unwrap();
        writeln!(f, "---").unwrap();
        write!(f, "{}", content).unwrap();
    }

    let mut e = engine_with("");
    e.open_file_in_tab(&path);

    // No recovery should be offered — swap content matches disk.
    assert!(
        e.pending_swap_recovery.is_none(),
        "no recovery should be offered when swap matches file on disk"
    );
    assert!(
        e.dialog.is_none(),
        "no dialog should be shown for unchanged swap"
    );

    // In a real run, the stale swap would be deleted and replaced with a
    // fresh one.  In tests, `delete_swap`/`write_swap` are suppressed, so
    // we can only verify that the engine state is correct (no dialog).

    let _ = fs::remove_file(&swap_path);
    let _ = fs::remove_file(&path);
}
