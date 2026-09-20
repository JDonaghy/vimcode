//! `undofile` / `undodir` persistence (#1156): saves a buffer's full undo
//! tree next to its saves, so `u`/`g-`/`:earlier` still work after quitting
//! and reopening the file. Mirrors `src/core/swap.rs`'s directory-and-hash
//! convention for where a per-file sidecar lives.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::buffer_manager::{UndoTree, UNDOFILE_VERSION};

/// Process-wide `'undofile'` on/off switch. Mirrors
/// `buffer_manager::SYNTAX_MAX_LINES`'s atomic-static pattern: cheap to read
/// on every save/open, thread-safe to update from `:set undofile`. Default
/// off, matching Vim/Neovim.
static UNDOFILE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Process-wide `'undodir'`. `None` means "use the default"
/// ([`default_undo_dir`]).
static UNDODIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Update the process-wide `'undofile'` switch.
pub fn set_enabled(enabled: bool) {
    UNDOFILE_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether `'undofile'` is currently on.
pub fn enabled() -> bool {
    UNDOFILE_ENABLED.load(Ordering::Relaxed)
}

/// Update the process-wide `'undodir'`. An empty string resets to the
/// default.
pub fn set_dir(dir: &str) {
    let mut guard = UNDODIR.lock().unwrap_or_else(|e| e.into_inner());
    *guard = if dir.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(dir))
    };
}

/// Current `'undodir'` (falls back to [`default_undo_dir`] when unset).
pub fn dir() -> PathBuf {
    UNDODIR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(default_undo_dir)
}

/// Default directory undofiles live in when `'undodir'` isn't overridden.
pub fn default_undo_dir() -> PathBuf {
    super::paths::vimcode_config_dir().join("undo")
}

/// Compute the undofile path for a given canonical file path, mangling the
/// path into the filename the way Vim's default `'undodir'` scheme does
/// (`//` first, falling back per-directory) — here, simply replacing path
/// separators with `%` so the mapping is human-readable and collision-free
/// for any two distinct absolute paths.
pub fn path_for(canonical: &Path, undodir: &Path) -> PathBuf {
    let mangled = canonical
        .to_string_lossy()
        .replace(['/', '\\'], "%")
        .replace(':', "-"); // Windows drive letters (`C:`)
    undodir.join(format!("{mangled}.undo"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UndoFileOnDisk {
    version: u32,
    tree: UndoTree,
}

/// Serialize `tree` to the undofile on-disk format (pretty JSON with a
/// version header field, `:h undo-persistence` equivalent).
fn serialize(tree: &UndoTree) -> String {
    serde_json::to_string_pretty(&UndoFileOnDisk {
        version: UNDOFILE_VERSION,
        tree: tree.clone(),
    })
    .unwrap_or_default()
}

/// Parse an undofile. Returns `None` if the data is malformed or was
/// written by an incompatible (newer or otherwise unrecognised) version.
fn parse(data: &str) -> Option<UndoTree> {
    let on_disk: UndoFileOnDisk = serde_json::from_str(data).ok()?;
    if on_disk.version != UNDOFILE_VERSION {
        return None;
    }
    Some(on_disk.tree)
}

/// Write `tree` to `path` atomically (write to `.tmp`, then rename).
/// No-ops during tests, like `swap::write_swap` — nothing here should touch
/// the real filesystem from `cargo test`.
pub fn write(path: &Path, tree: &UndoTree) {
    if cfg!(test) || crate::core::session::saves_suppressed() {
        return;
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension("tmp");
    let data = serialize(tree);
    let result = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data.as_bytes())?;
        f.flush()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// Read and parse an undofile. Returns `None` if it doesn't exist or is
/// malformed/incompatible.
pub fn read(path: &Path) -> Option<UndoTree> {
    let data = fs::read_to_string(path).ok()?;
    parse(&data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::buffer::Buffer;
    use crate::core::buffer_manager::BufferState;
    use crate::core::cursor::Cursor;

    #[test]
    fn path_for_is_deterministic_and_collision_free() {
        let dir = PathBuf::from("/home/user/.config/vimcode/undo");
        let a = path_for(Path::new("/home/user/project/a.rs"), &dir);
        let b = path_for(Path::new("/home/user/project/b.rs"), &dir);
        assert_ne!(a, b);
        assert_eq!(a, path_for(Path::new("/home/user/project/a.rs"), &dir));
    }

    /// Round-trips a real `UndoTree` — including a branch a plain `u` +
    /// edit would abandon — through `serialize`/`parse`, bypassing the
    /// `cfg!(test)`-suppressed `write`/`read` I/O wrappers the same way
    /// `swap::test_swap_roundtrip` bypasses `write_swap`'s suppression.
    #[test]
    fn undofile_roundtrip_preserves_branches() {
        let mut state = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
        // "a": root -> "a".
        state.start_undo_group(Cursor::new());
        state.record_insert(0, "a");
        state.buffer.insert(0, "a");
        state.finish_undo_group(Cursor { line: 0, col: 1 });
        // `u` back to root, then a *different* edit — an abandoned branch a
        // plain undo-stack would have discarded.
        state.undo();
        state.start_undo_group(Cursor::new());
        state.record_insert(0, "b");
        state.buffer.insert(0, "b");
        state.finish_undo_group(Cursor { line: 0, col: 1 });

        let data = serialize(&state.undo_tree);
        let restored = parse(&data).expect("undofile should parse");

        assert_eq!(restored.nodes.len(), 3); // root + "a" + "b"
        let root_children = &restored.nodes[0].children;
        assert_eq!(
            root_children.len(),
            2,
            "both the abandoned \"a\" branch and the \"b\" branch must survive persistence"
        );
        let texts: std::collections::HashSet<&str> =
            restored.nodes.iter().map(|n| n.text.as_str()).collect();
        assert!(texts.contains("a"));
        assert!(texts.contains("b"));
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let bad = r#"{"version": 999999, "tree": {"nodes": [], "current": 0, "next_seq": 0}}"#;
        assert!(parse(bad).is_none());
    }
}
