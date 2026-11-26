//! Black-box coverage for the crash-report dialog: a previous run's crash
//! log, left on disk by `swap::write_crash_log`, must surface a dialog on
//! the next startup offering to file it as a GitHub issue.
//!
//! `swap::take_crash_report()` is a no-op under `cfg(test)` so a developer's
//! real crash log is never read or deleted by `cargo test --lib`; these are
//! integration tests (`tests/*.rs` link the library normally, so
//! `cfg(test)` is false here), matching how `tests/swap_recovery.rs` tests
//! the sibling swap-recovery dialog against the real, computed swap path.

mod common;
use common::*;
use std::fs;
use std::sync::Mutex;
use vimcode_core::core::engine::PendingPlatformAction;
use vimcode_core::core::swap;

/// `swap::crash_log_path()` is one fixed path under the real
/// `vimcode_config_dir()` shared by every test in this binary (integration
/// tests link the library as an external crate, so `paths::set_test_home`'s
/// thread-local override is unreachable here) — `cargo test` runs them in
/// parallel threads by default, so without this lock one test's
/// write/remove races another's.
static CRASH_LOG_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn crash_log_on_disk_opens_the_report_dialog() {
    let _guard = CRASH_LOG_TEST_LOCK.lock().unwrap();
    let path = swap::crash_log_path();
    let _ = fs::remove_file(&path);
    fs::write(
        &path,
        "PANIC: kaboom\n  at src/foo.rs:1:1\nbacktrace:\nframe0\n",
    )
    .unwrap();

    let mut e = engine_with("");
    e.offer_crash_report_if_pending();

    assert!(
        !path.exists(),
        "the crash log must be consumed so a clean run never re-shows this dialog"
    );
    let dialog = e.dialog.as_ref().expect("crash dialog should be open");
    assert_eq!(dialog.tag, "crash_report");
    assert_eq!(dialog.buttons.len(), 2);
    assert!(e.pending_crash_report.is_some());

    let _ = fs::remove_file(&path);
}

#[test]
fn no_crash_log_opens_no_dialog() {
    let _guard = CRASH_LOG_TEST_LOCK.lock().unwrap();
    let path = swap::crash_log_path();
    let _ = fs::remove_file(&path);

    let mut e = engine_with("");
    e.offer_crash_report_if_pending();

    assert!(e.dialog.is_none());
    assert!(e.pending_crash_report.is_none());
}

#[test]
fn reporting_a_disk_backed_crash_queues_the_github_issue_url() {
    let _guard = CRASH_LOG_TEST_LOCK.lock().unwrap();
    let path = swap::crash_log_path();
    let _ = fs::remove_file(&path);
    let report = "PANIC: kaboom\n  at src/foo.rs:1:1\nbacktrace:\nframe0\n";
    fs::write(&path, report).unwrap();

    let mut e = engine_with("");
    e.offer_crash_report_if_pending();
    e.dialog_click_button(0); // "Report on GitHub" is button 0.

    assert_eq!(
        e.pending_platform_actions,
        vec![PendingPlatformAction::OpenUrl(
            swap::crash_report_issue_url(report)
        )]
    );

    let _ = fs::remove_file(&path);
}
