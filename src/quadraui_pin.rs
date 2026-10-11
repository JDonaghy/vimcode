//! Which quadraui is this build actually made of?
//!
//! quadraui is a **crates.io dependency, version-pinned** in `Cargo.toml`
//! and recorded in `Cargo.lock`, so the exact release vimcode was built
//! against is always attributable from tracked files alone.
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
