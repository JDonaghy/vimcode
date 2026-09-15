//! #522: `src/core/` and `src/render.rs` must carry no coordinator
//! vocabulary. vimcode is an editor that can *host* a coordinator client
//! (through the generic, coord-agnostic seam in `src/core/tool_client.rs`)
//! — it is not a coordinator client itself. Everything coord-specific
//! belongs in the coordinator extension bundle, never here.
//!
//! A plain `grep -ri "coord" src/core/ src/render.rs` legitimately turns up
//! matches today — "coordinate"/"coordinates" (screen/pixel geometry
//! terms) and "coordinator" used as a plain English noun (e.g. "Multi-
//! server LSP coordinator"). Those are incidental, per #522's acceptance
//! text ("should return nothing but incidental matches"). What must never
//! appear is "coord" as its own word: the CLI name itself, or fragments of
//! its vocabulary (`coord assign`, `coord test`, `coord merge`,
//! `report-result`, `coord-tui`, `coordinator.yml`, `coord.db`, ...).
//!
//! A plain substring search can't tell "coordinate" apart from "coord ".
//! This test uses a word-boundary regex instead: `\bcoord\b` matches
//! "coord" only when it is *not* immediately followed by another word
//! character, so "coordinate"/"coordinator" (where "coord" runs straight
//! into "inate"/"inator" with no boundary) keep passing, while standalone
//! "coord" — including hyphenated forms like `coord-tui`, since `-` is a
//! non-word character — fails it.

use regex::Regex;
use std::path::{Path, PathBuf};

/// Regex matching "coord" as its own word, case-insensitive. See the
/// module doc for why this (and not a plain substring match) is the right
/// check.
fn coord_word_regex() -> Regex {
    Regex::new(r"(?i)\bcoord\b").expect("valid regex")
}

/// Recursively collect every `.rs` file under `dir`.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Scan `path`'s contents for standalone "coord" occurrences, returning
/// one formatted `path:line: content` string per hit.
fn scan_file(path: &Path, re: &Regex, hits: &mut Vec<String>) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    for (i, line) in content.lines().enumerate() {
        if re.is_match(line) {
            hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
        }
    }
}

#[test]
fn core_and_render_have_no_coordinator_vocabulary() {
    let re = coord_word_regex();
    let mut hits = Vec::new();

    let core_dir = Path::new("src/core");
    assert!(
        core_dir.is_dir(),
        "expected src/core to exist relative to the crate root"
    );
    let mut files = Vec::new();
    collect_rs_files(core_dir, &mut files);
    assert!(
        !files.is_empty(),
        "expected to find .rs files under src/core"
    );
    for file in &files {
        scan_file(file, &re, &mut hits);
    }

    let render_rs = Path::new("src/render.rs");
    assert!(render_rs.is_file(), "expected src/render.rs to exist");
    scan_file(render_rs, &re, &mut hits);

    assert!(
        hits.is_empty(),
        "src/core/ and src/render.rs must carry no coordinator vocabulary (#522). \
         'coordinate'/'coordinator' as plain English are fine; standalone 'coord' \
         (CLI name, subcommands, coord-tui, coordinator.yml, ...) is not. Found:\n{}",
        hits.join("\n")
    );
}

/// Sanity check that the regex itself draws the line where the module doc
/// claims it does — this is the thing the main test's silence depends on,
/// so it earns its own coverage rather than being trusted by inspection.
#[test]
fn coord_word_regex_distinguishes_incidental_from_forbidden() {
    let re = coord_word_regex();

    // Incidental — plain English, must NOT match.
    assert!(!re.is_match("convert to screen coordinates"));
    assert!(!re.is_match("Multi-server LSP coordinator. None until first use."));
    assert!(!re.is_match("claude-coordinator#550 is closed"));

    // Forbidden — actual coordinator vocabulary, must match.
    assert!(re.is_match("run `coord assign` to dispatch"));
    assert!(re.is_match("call coord test --passed"));
    assert!(re.is_match("the coord-tui reference implementation"));
    assert!(re.is_match("poll coord.db for state"));
}
