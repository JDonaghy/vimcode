//! #1492: every "run this string through the user's shell" spawn site must
//! go through `core::terminal::shell_cmd()` — the single construction
//! point that wraps `Command::new(shell)` in `git::hidden_command` so the
//! console window stays hidden on Win-GUI (`CREATE_NO_WINDOW`).
//!
//! Before this issue, `:!`, `:r !`, `!{motion}` filters and plugin shell
//! requests each hand-rolled `let (shell, flag) = shell_command();
//! Command::new(shell).arg(flag).arg(cmd)` directly, bypassing
//! `hidden_command` entirely — only the LSP install-command spawn site
//! wrapped it (by hand, not through a shared helper). This test guards
//! against that pattern reappearing at a *new* call site: it greps every
//! `src/core/**/*.rs` file other than `terminal.rs` itself (which defines
//! `shell_cmd`) for a literal `Command::new(shell)` / `Command::new(&shell)`
//! construction — the exact shape that bypasses `hidden_command`.
//!
//! Verified RED against unfixed develop: before #1492, this grep matched
//! three sites (`execute.rs` x2, `windows.rs`) plus a `Command::new(shell)`
//! in `plugins.rs`, so this test failed loudly instead of passing against
//! the bug.

use std::path::Path;

/// Recursively collect every `.rs` file under `dir`.
fn collect_rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_direct_shell_command_construction_outside_terminal_rs() {
    let core_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core");
    let mut files = Vec::new();
    collect_rs_files(&core_dir, &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .rs files under src/core"
    );

    let mut offenders = Vec::new();
    for path in &files {
        // `shell_cmd()` itself lives here and is the one legitimate place
        // that constructs a shell `Command` directly.
        if path.file_name().and_then(|n| n.to_str()) == Some("terminal.rs") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in contents.lines().enumerate() {
            if line.contains("Command::new(shell)") || line.contains("Command::new(&shell)") {
                offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "found direct shell-`Command` construction bypassing `terminal::shell_cmd()` \
         (loses Win-GUI console hiding, #1492):\n{}",
        offenders.join("\n")
    );
}

/// The five sites named in #1492 must all route through `shell_cmd(`.
#[test]
fn known_shell_spawn_sites_use_shell_cmd() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let sites = [
        "src/core/engine/execute.rs",
        "src/core/engine/windows.rs",
        "src/core/engine/plugins.rs",
        "src/core/lsp_manager.rs",
    ];
    for site in sites {
        let path = manifest.join(site);
        let contents = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        assert!(
            contents.contains("shell_cmd("),
            "{} no longer calls terminal::shell_cmd() — did a shell spawn \
             site get reverted to hand-rolling `Command::new(shell)`? (#1492)",
            site
        );
    }
}
