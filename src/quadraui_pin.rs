//! Which quadraui is this build actually made of? (#691, formerly #638)
//!
//! quadraui is a **git dependency pinned to a `rev`** in `Cargo.toml` (see the
//! dependency comment there). `Cargo.lock` records the fully-resolved 40-char
//! SHA, so unlike the old sibling path dep, the exact commit vimcode was built
//! against is always attributable from tracked files alone — no separate pin
//! file or build-time checkout comparison needed.
//!
//! Before #691, quadraui was a *relative sibling path dependency*: Cargo
//! cannot pin those, so `Cargo.lock` had no entry for quadraui at all, and an
//! upstream quadraui merge could restate vimcode's rendering with **zero
//! vimcode commits**. That was the root cause of #625 — quadraui#472's
//! `char_cell_width` change staled six `snapshot_*` tests on every machine at
//! once, misdiagnosed as CI flakiness for weeks — and of two separate
//! `QUADRAUI PIN MISMATCH` incidents during #659's smoke, both caused purely
//! by `~/src/quadraui` (a checkout shared by every concurrently-running agent
//! on the machine) moving underneath a build with nothing wrong in vimcode
//! either time. A git rev pin closes that gap at the source: Cargo resolves
//! and locks it, so there is nothing left for a shared directory to disturb.
//!
//! `build.rs` bakes the resolved rev into the binary so the answer to "which
//! quadraui?" is in the output rather than something a human has to think to
//! go and ask.

/// The quadraui commit this binary was compiled against — the resolved rev
/// from `Cargo.lock` (or, if no lockfile existed yet at build time, the `rev`
/// pinned in `Cargo.toml`). `"unknown"` only if neither file was readable.
pub const RESOLVED_REV: &str = env!("VIMCODE_QUADRAUI_REV");

/// First 12 characters of a rev, or the whole string if it is shorter (e.g.
/// `"unknown"`).
fn short(rev: &str) -> &str {
    rev.get(..12).unwrap_or(rev)
}

/// One line naming the quadraui this build is made of, for `--version` output.
pub fn version_line() -> String {
    format!("quadraui {}", short(RESOLVED_REV))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_pin() {
        let line = version_line();
        assert!(line.starts_with("quadraui "), "got {line:?}");
        assert!(line.contains(short(RESOLVED_REV)), "got {line:?}");
    }

    #[test]
    fn short_handles_non_sha_values() {
        assert_eq!(short("unknown"), "unknown");
        assert_eq!(
            short("f702422e0d6f5b4c34dbfc449494bce2d222cf1d"),
            "f702422e0d6f"
        );
    }
}
