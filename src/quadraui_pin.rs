//! Which quadraui is this build actually made of? (#691, formerly #638)
//!
//! quadraui is a **crates.io dependency, version-pinned** in `Cargo.toml`
//! (since #1848; see the dependency comment there). `Cargo.lock` records the
//! fully-resolved version, so the exact release vimcode was built against is
//! always attributable from tracked files alone — no separate pin file or
//! build-time checkout comparison needed.
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
//! either time. #691 closed that gap with a git rev pin, which Cargo resolved
//! and locked, so there was nothing left for a shared directory to disturb.
//! #1848 moved the same guarantee onto quadraui's published crates.io
//! release: a `version =` requirement, resolved and locked by Cargo the same
//! way.
//!
//! `build.rs` bakes the resolved version into the binary so the answer to
//! "which quadraui?" is in the output rather than something a human has to
//! think to go and ask.

/// The quadraui version this binary was compiled against — the resolved
/// version from `Cargo.lock` (or, if no lockfile existed yet at build time,
/// the `version` requirement in `Cargo.toml`). `"unknown"` only if neither
/// file was readable.
pub const RESOLVED_VERSION: &str = env!("VIMCODE_QUADRAUI_VERSION");

/// One line naming the quadraui this build is made of, for `--version` output.
pub fn version_line() -> String {
    format!("quadraui {RESOLVED_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_version() {
        let line = version_line();
        assert!(line.starts_with("quadraui "), "got {line:?}");
        assert!(line.contains(RESOLVED_VERSION), "got {line:?}");
    }
}
