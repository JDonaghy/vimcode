//! Which quadraui is this build actually made of? (#638)
//!
//! quadraui is a *relative sibling path dependency*, so Cargo cannot pin it and
//! `Cargo.lock` has no entry for it. Historically that meant an upstream
//! quadraui merge could restate vimcode's rendering with zero vimcode commits —
//! the root cause behind #625, where quadraui#472's `char_cell_width` change
//! staled six `snapshot_*` tests on every machine simultaneously and was
//! misdiagnosed for weeks as CI flakiness.
//!
//! `build.rs` resolves the sibling checkout's `HEAD` and compares it against
//! `quadraui-pin.txt`, failing the build on a mismatch. This module surfaces
//! both revs at runtime so the answer to "which quadraui?" is *in the output*
//! rather than something a human has to think to go and ask.

/// The quadraui commit recorded in `quadraui-pin.txt` — what vimcode's
/// behaviour and snapshots were produced against.
pub const PINNED_REV: &str = env!("VIMCODE_QUADRAUI_PINNED_REV");

/// The quadraui commit this binary was *actually* compiled against, or
/// `"unknown"` when the sibling checkout is not a git repository.
///
/// Equal to [`PINNED_REV`] unless the build opted out with
/// `VIMCODE_QUADRAUI_UNPINNED=1`.
pub const RESOLVED_REV: &str = env!("VIMCODE_QUADRAUI_REV");

/// First 12 characters of a rev, or the whole string if it is shorter (e.g.
/// `"unknown"`).
fn short(rev: &str) -> &str {
    rev.get(..12).unwrap_or(rev)
}

/// One line naming the quadraui this build is made of, for `--version` output.
///
/// Reads `quadraui f6d27c239203` when pinned, and calls out drift loudly
/// otherwise — an unpinned build's rendering differences are not attributable
/// to vimcode, and the version string should say so.
pub fn version_line() -> String {
    if RESOLVED_REV == PINNED_REV {
        format!("quadraui {}", short(PINNED_REV))
    } else {
        format!(
            "quadraui {} (UNPINNED — pin is {})",
            short(RESOLVED_REV),
            short(PINNED_REV)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pin file must hold a real, full-length SHA. A truncated or
    /// placeholder value would let `build.rs` compare against garbage.
    #[test]
    fn pinned_rev_is_a_full_sha() {
        assert_eq!(PINNED_REV.len(), 40, "pin must be a full 40-char sha");
        assert!(
            PINNED_REV.chars().all(|c| c.is_ascii_hexdigit()),
            "pin must be hex"
        );
    }

    /// The load-bearing assertion of #638.
    ///
    /// A test run against an off-pin quadraui produces failures that look
    /// exactly like vimcode regressions — that is precisely how #625 cost
    /// weeks. Fail here, first and by name, so the next person reads
    /// "quadraui moved" instead of hunting a phantom snapshot bug.
    ///
    /// Deliberately unpinned co-development builds are exempt: `build.rs`
    /// already warned loudly at compile time.
    #[test]
    fn test_run_is_against_the_pinned_quadraui() {
        if std::env::var("VIMCODE_QUADRAUI_UNPINNED").is_ok_and(|v| !v.is_empty() && v != "0") {
            eprintln!(
                "vimcode: VIMCODE_QUADRAUI_UNPINNED set; {} — snapshot failures in \
                 this run are not necessarily vimcode's fault.",
                version_line()
            );
            return;
        }

        // Surface the rev unconditionally: `cargo test -- --nocapture` should
        // answer "which quadraui?" without anyone having to go looking.
        eprintln!("vimcode: built against {}", version_line());

        assert_eq!(
            RESOLVED_REV, PINNED_REV,
            "this build used quadraui {RESOLVED_REV}, but quadraui-pin.txt pins \
             {PINNED_REV}. Rendering/behaviour differences in this run are \
             attributable to quadraui, not to vimcode. See quadraui-pin.txt."
        );
    }

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
