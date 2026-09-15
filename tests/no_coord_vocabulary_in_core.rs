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
//! appear is "coord" as its own word component: the CLI name itself, or
//! fragments of its vocabulary (`coord assign`, `coord test`, `coord
//! merge`, `report-result`, `coord-tui`, `coord.db`, ...) — including when
//! "coord" is glued to another word inside a single Rust identifier, e.g.
//! `coord_client`, `CoordClient`, `mod coord_schema`.
//!
//! A plain substring search can't tell "coordinate" apart from "coord ".
//! A naive `\bcoord\b` regex fixes that for whitespace/punctuation-
//! delimited occurrences, but it is *not* sufficient on its own: Rust's
//! `\b` only matches at a transition between a "word" character (letter,
//! digit, `_`) and a non-word character. Neither `_` nor a lowercase-to-
//! uppercase letter transition is such a boundary, so `\bcoord\b` fails to
//! match "coord" inside `coord_client`, `mod coord_schema;`, `let
//! coord_json = 1;`, or PascalCase compounds like `CoordClient`/
//! `CoordGate` — exactly the naming style idiomatic Rust code would use to
//! reintroduce coordinator vocabulary. This test instead tokenizes each
//! line into identifier-like runs (`\w+`) and splits each token into its
//! constituent words on `_` boundaries *and* lowercase-to-uppercase case
//! transitions, then checks whether any resulting word is exactly "coord"
//! (case-insensitive). That still lets "coordinate"/"coordinator" through
//! (neither contains an internal `_` or case transition, so each stays a
//! single word, "coordinate"/"coordinator", not "coord"), while catching
//! every compound that has "coord" as one of its components.

use regex::Regex;
use std::path::{Path, PathBuf};

/// Regex matching a single identifier-like token: a maximal run of word
/// characters (letters, digits, underscore). Used to break a line into
/// candidate identifiers before checking each one for a standalone
/// "coord" component.
fn token_regex() -> Regex {
    Regex::new(r"\w+").expect("valid regex")
}

/// Split an identifier token into its constituent words on `_` boundaries
/// and lowercase-to-uppercase case transitions (covers snake_case,
/// PascalCase and camelCase). E.g. `"coord_client"` -> `["coord",
/// "client"]`, `"CoordGate"` -> `["Coord", "Gate"]`, `"coordinate"` ->
/// `["coordinate"]` (no boundary anywhere, so it stays whole).
fn split_identifier_words(token: &str) -> Vec<String> {
    let mut words = Vec::new();
    for part in token.split('_') {
        if part.is_empty() {
            continue;
        }
        let mut current = String::new();
        let chars: Vec<char> = part.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if i > 0 && chars[i - 1].is_lowercase() && c.is_uppercase() {
                words.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        if !current.is_empty() {
            words.push(current);
        }
    }
    words
}

/// True if `line` contains "coord" as a standalone word component of some
/// token — including inside snake_case (`coord_client`) or PascalCase/
/// camelCase (`CoordClient`) compounds — not just as a whitespace/
/// punctuation-delimited occurrence. See the module doc for why this (and
/// not a plain `\bcoord\b` regex) is the right check.
fn line_has_coord_word(line: &str, tokens: &Regex) -> bool {
    tokens.find_iter(line).any(|m| {
        split_identifier_words(m.as_str())
            .iter()
            .any(|w| w.eq_ignore_ascii_case("coord"))
    })
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

/// Scan `path`'s contents for standalone "coord" word components, returning
/// one formatted `path:line: content` string per hit.
fn scan_file(path: &Path, tokens: &Regex, hits: &mut Vec<String>) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    for (i, line) in content.lines().enumerate() {
        if line_has_coord_word(line, tokens) {
            hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
        }
    }
}

#[test]
fn core_and_render_have_no_coordinator_vocabulary() {
    let tokens = token_regex();
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
        scan_file(file, &tokens, &mut hits);
    }

    let render_rs = Path::new("src/render.rs");
    assert!(render_rs.is_file(), "expected src/render.rs to exist");
    scan_file(render_rs, &tokens, &mut hits);

    assert!(
        hits.is_empty(),
        "src/core/ and src/render.rs must carry no coordinator vocabulary (#522). \
         'coordinate'/'coordinator' as plain English are fine; standalone 'coord' \
         (CLI name, subcommands, coord-tui, coordinator.yml, coord.db, coord_client, \
         CoordClient, ...) is not. Found:\n{}",
        hits.join("\n")
    );
}

/// Sanity check that the tokenizer draws the line where the module doc
/// claims it does — this is the thing the main test's silence depends on,
/// so it earns its own coverage rather than being trusted by inspection.
#[test]
fn line_has_coord_word_distinguishes_incidental_from_forbidden() {
    let tokens = token_regex();
    let check = |line: &str| line_has_coord_word(line, &tokens);

    // Incidental — plain English, must NOT match.
    assert!(!check("convert to screen coordinates"));
    assert!(!check(
        "Multi-server LSP coordinator. None until first use."
    ));
    assert!(!check("claude-coordinator#550 is closed"));
    assert!(!check("let screen_coordinates = compute();"));

    // Forbidden — whitespace/punctuation-delimited "coord", must match.
    assert!(check("run `coord assign` to dispatch"));
    assert!(check("call coord test --passed"));
    assert!(check("the coord-tui reference implementation"));
    assert!(check("poll coord.db for state"));

    // Forbidden — "coord" glued to another word via `_` or a case
    // transition, the exact false-negative a plain `\bcoord\b` regex has.
    assert!(check("coord_client.rs"));
    assert!(check("mod coord_schema;"));
    assert!(check("let coord_json = 1;"));
    assert!(check("struct CoordClient;"));
    assert!(check("fn coord_gate() {}"));
    assert!(check("use crate::CoordGate;"));
}
