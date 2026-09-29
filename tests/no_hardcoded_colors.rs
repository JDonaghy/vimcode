//! #1575: enforce, with a test, the owner's rule that vimcode contains no
//! hard-coded colours — every colour comes from the theme.
//!
//! Before this test the rule was only *partly* written down
//! (`docs/PATTERNS.md` §"Theme Colors (CRITICAL)" bans new hex literals for
//! derived `Theme` fields specifically, and is silent on paint code) and
//! not enforced anywhere: the one violation this audit found
//! (`render.rs`, since fixed by `def5437c`, "derive colorcolumn_bg from
//! background instead of hardcoded hex") slipped in unnoticed because
//! nothing scanned for it.
//!
//! This test scans every `.rs` file under `src/` for three call shapes
//! that name a colour directly instead of reading one from a `Theme`:
//!
//! - `Color::rgb(<int>, <int>, <int>)` / `Color::from_rgb(...)` (also
//!   matches the `quadraui::Color::rgb(...)` form, since `Color::rgb(` is
//!   a substring of it) called with integer literals. `Color::rgb(theme
//!   .cursor.r, theme.cursor.g, theme.cursor.b)` — the standard "convert a
//!   `Theme` field to a `quadraui::Color`" idiom used throughout
//!   `render.rs` — does NOT match, because its arguments are field
//!   accesses, not literals.
//! - `hex("#rrggbb")` / `from_hex("#rrggbb")` called with a literal hex
//!   string. The local `hex`/`try_from_hex` parser *helpers* themselves
//!   (`fn hex(s: &str) -> Color { quadraui::Color::from_hex(s)... }`) take
//!   a variable, not a literal, so they don't match either.
//! - `set_source_rgb(<float>, <float>, <float>)` / `set_source_rgba(...)`
//!   — the raw Cairo call some GTK paint code could reach for directly
//!   instead of going through a themed `Color`.
//!
//! ## Allowlist (each entry justified here, per #1575's acceptance bar)
//!
//! - **`#[cfg(test)]` code.** Assertions like `assert_eq!(theme.cursor,
//!   try_from_hex("#f5e0dc").unwrap())` and paint-sentinel colours like
//!   `cr.set_source_rgb(1.0, 0.0, 1.0)` (a magenta canary a contrast test
//!   fills the canvas with, precisely because it can't be confused with
//!   any themed colour — see `src/gtk/mod.rs`'s `per_segment_contrast_deltas`)
//!   are test fixtures asserting *against* the theme, not shipped chrome.
//!   They are outside the rule's scope by construction.
//!
//!   Finding where a `#[cfg(test)]` item *ends* means counting braces, and
//!   #1576 found that counting them naively (every `{`/`}` character,
//!   including the ones inside strings and comments) silently loses the
//!   exemption: `src/tui_main/app_on_tui_tests.rs` is wholly `#[cfg(test)]`
//!   yet its raw brace count never balances, so the block "never closed"
//!   and the entire test-only file got scanned. [`strip_non_code_runs`]
//!   is what makes this allowlist entry actually hold.
//! - **The theme-constructor bodies** (`Theme::onedark`, `gruvbox_dark`,
//!   `tokyo_night`, `solarized_dark`, `vscode_dark`, `vscode_light`).
//!   These functions ARE the palette: something has to spell out
//!   `hex("#61afef")` once, in one place, to define what "the theme" means.
//!   Every other call site in the codebase is expected to read a `Theme`
//!   field derived from one of these tables, not to name its own literal —
//!   which is exactly what the un-allowlisted regexes above check for.
//!
//! No entry is needed for `src/icons.rs`'s per-language icon-identity
//! palette (`ICON_BLUE`, `ICON_ORANGE`, ...; #703, "identity, not chrome" —
//! a `.rs` badge stays orange in every theme the same way its glyph stays
//! the same glyph). Those constants are plain `(u8, u8, u8)` tuples, and
//! the one place they reach a `Color` (`render::tab_icon_color`) converts
//! them through `Color::from_rgb(r, g, b)` with variable arguments, which
//! this scan's literal-argument check does not match — so the exemption
//! falls out of the regex shape rather than needing a carve-out.

use regex::Regex;
use std::path::{Path, PathBuf};

/// `Theme` constructor methods whose bodies are the one place colour
/// literals define the palette itself. See the module doc's "Allowlist"
/// section for why these, and only these, are exempt.
const ALLOWLISTED_THEME_FNS: &[&str] = &[
    "onedark",
    "gruvbox_dark",
    "tokyo_night",
    "solarized_dark",
    "vscode_dark",
    "vscode_light",
];

/// `Color::rgb(...)`/`Color::from_rgb(...)` (also matches the
/// `quadraui::Color::rgb(...)` form as a substring) called with three
/// integer literals rather than `Theme`-field expressions.
fn rgb_literal_call_regex() -> Regex {
    Regex::new(
        r"Color::(?:rgb|from_rgb)\s*\(\s*(?:0x[0-9a-fA-F]+|\d+)\s*,\s*(?:0x[0-9a-fA-F]+|\d+)\s*,\s*(?:0x[0-9a-fA-F]+|\d+)\s*[,)]",
    )
    .expect("valid regex")
}

/// `hex("#...")`/`from_hex("#...")` called with a literal hex string.
fn hex_literal_call_regex() -> Regex {
    Regex::new("(?:\\bhex|from_hex)\\(\\s*\"#[0-9a-fA-F]{3,8}\"").expect("valid regex")
}

/// Cairo `set_source_rgb`/`set_source_rgba` called with literal numbers.
fn set_source_rgb_literal_regex() -> Regex {
    Regex::new(r"set_source_rgba?\s*\(\s*[0-9.]+\s*,\s*[0-9.]+\s*,\s*[0-9.]+").expect("valid regex")
}

/// The three violation shapes this gate checks for, paired with a
/// human-readable reason shown in failure output.
fn violation_regexes() -> Vec<(&'static str, Regex)> {
    vec![
        (
            "Color::rgb/from_rgb called with integer literals instead of Theme fields",
            rgb_literal_call_regex(),
        ),
        (
            "hex()/from_hex() called with a literal hex string outside a theme definition",
            hex_literal_call_regex(),
        ),
        (
            "Cairo set_source_rgb(a) called with literal numbers instead of a themed Color",
            set_source_rgb_literal_regex(),
        ),
    ]
}

/// What [`strip_non_code_runs`] is in the middle of when a line ends: a
/// string, a raw string (carrying its `#` count), or a block comment
/// (carrying its nesting depth — Rust's `/* /* */ */` nest).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum ScanState {
    #[default]
    Code,
    Str,
    RawStr(usize),
    BlockComment(usize),
}

/// Blank out every run on `line` that cannot hold a *structural* brace: a
/// `//` line comment, a `/* ... */` comment, a string literal (plain or
/// raw), or a single-character literal. `state` carries the scan across line
/// boundaries — Rust strings, raw strings and block comments all span
/// lines — and must be threaded through consecutive lines by the caller.
/// [`block_end`] counts braces over the result.
///
/// Without this, one `format!("{")`-shaped brace inside a string
/// desynchronises the depth count for the rest of the file. That is not
/// hypothetical: `src/tui_main/app_on_tui_tests.rs` is a single 5.6k-line
/// `#[cfg(test)] mod tests`, and its raw brace count never returns to zero
/// (net +4, all from braces inside string literals and comments), so
/// `block_end` returned `None` and the whole test-only file was scanned —
/// the exact opposite of the "`#[cfg(test)]` code is exempt by
/// construction" allowlist in this file's module doc. #1576 hit it when the
/// first colour literal landed in that file's test bodies.
///
/// The cross-line state is load-bearing in the other direction too:
/// `render.rs`'s `test_strip_json_comments` opens a multi-line raw string
/// with `r#"{` and closes it with `}"#`. A line-scoped scan drops the `{`
/// (it is inside a string) but keeps the `}` (its line looks like code), so
/// it would end the enclosing `mod tests` ~6.5k lines early and start
/// flagging test assertions.
fn strip_non_code_runs(line: &str, state: &mut ScanState) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // First, finish whatever construct the previous line left open.
        match *state {
            ScanState::Str => {
                // Scan to the first unescaped `"`.
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    *state = ScanState::Code;
                }
                i += 1;
                continue;
            }
            ScanState::RawStr(hashes) => {
                if chars[i] == '"' && count_hashes(&chars, i + 1) >= hashes {
                    *state = ScanState::Code;
                    i += 1 + hashes;
                } else {
                    i += 1;
                }
                continue;
            }
            ScanState::BlockComment(depth) => {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    *state = ScanState::BlockComment(depth + 1);
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    *state = if depth <= 1 {
                        ScanState::Code
                    } else {
                        ScanState::BlockComment(depth - 1)
                    };
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            ScanState::Code => {}
        }

        let c = chars[i];
        // `//` line comment: nothing structural can follow on this line.
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        }
        // `/* ... */` comment, possibly spanning lines.
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            *state = ScanState::BlockComment(1);
            i += 2;
            continue;
        }
        // Raw string opener: `r"`, `r#"`, `r##"`, ...
        if c == 'r' {
            let hashes = count_hashes(&chars, i + 1);
            if chars.get(i + 1 + hashes) == Some(&'"') {
                *state = ScanState::RawStr(hashes);
                i += 2 + hashes;
                continue;
            }
        }
        // Plain string opener.
        if c == '"' {
            *state = ScanState::Str;
            i += 1;
            continue;
        }
        // Char literal — `'{'`, `'\''`, `'\u{7f}'` (note the braces that one
        // carries). A lifetime (`'a`, `'static`) is NOT a char literal, so
        // only consume when a closing quote is actually there.
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\\') {
                if let Some(close) = chars[i + 2..].iter().position(|c| *c == '\'') {
                    i += 2 + close + 1;
                    continue;
                }
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// How many consecutive `#` characters start at `from` (a raw string's hash
/// count, on either its opening or closing side).
fn count_hashes(chars: &[char], from: usize) -> usize {
    chars[from.min(chars.len())..]
        .iter()
        .take_while(|c| **c == '#')
        .count()
}

/// Find the line index (relative to `lines`, absolute) at which the brace
/// block starting on `lines[start]` closes — i.e. the first line, at or
/// after `start`, where running `{`/`}` counts from `start` return to 0
/// after having gone positive at least once. Braces inside comments, string
/// literals and char literals don't count (see [`strip_non_code_runs`]).
/// Returns `None` if the block never closes (shouldn't happen for
/// well-formed Rust source).
fn block_end(lines: &[&str], start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut started = false;
    let mut state = ScanState::default();
    for (offset, line) in lines[start..].iter().enumerate() {
        for ch in strip_non_code_runs(line, &mut state).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if started && depth <= 0 {
            return Some(start + offset);
        }
    }
    None
}

/// Blank out (replace with an empty line, preserving line numbers so
/// reported line numbers stay accurate) every `#[cfg(test)]`-attributed
/// item and every allowlisted theme-constructor body in `content`, so the
/// violation regexes never see them. See the module doc's "Allowlist"
/// section for why these two regions, and only these, are exempt.
fn strip_allowlisted_regions(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut skip = vec![false; lines.len()];

    // `#[cfg(test)]` items: skip from the attribute through the matching
    // close-brace of the item it guards (a `mod tests { ... }` or a
    // `#[test] fn ... { ... }`), tolerating other attributes in between
    // (e.g. `#[cfg(test)]\n#[test]\nfn foo() { ... }`).
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[cfg(test)]") {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim_start().starts_with('#') {
                j += 1;
            }
            if j < lines.len() {
                if let Some(end) = block_end(&lines, j) {
                    for line in skip.iter_mut().take(end + 1).skip(i) {
                        *line = true;
                    }
                    i = end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    // Theme-constructor bodies.
    for name in ALLOWLISTED_THEME_FNS {
        let marker = format!("pub fn {name}() -> Self {{");
        if let Some(start) = lines.iter().position(|l| l.trim() == marker) {
            if let Some(end) = block_end(&lines, start) {
                for line in skip.iter_mut().take(end + 1).skip(start) {
                    *line = true;
                }
            }
        }
    }

    lines
        .iter()
        .enumerate()
        .map(|(idx, line)| if skip[idx] { "" } else { *line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scan `content` (already run through [`strip_allowlisted_regions`]) for
/// colour-literal violations, returning one formatted `label:line: text
/// [reason]` string per hit.
fn scan_content(label: &str, content: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for (reason, re) in violation_regexes() {
        for m in re.find_iter(content) {
            let line_no = content[..m.start()].matches('\n').count() + 1;
            let line_text = content.lines().nth(line_no - 1).unwrap_or("").trim();
            hits.push(format!("{label}:{line_no}: {line_text}  [{reason}]"));
        }
    }
    hits
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

#[test]
fn no_hardcoded_color_literals_outside_theme_definitions() {
    let src_dir = Path::new("src");
    assert!(
        src_dir.is_dir(),
        "expected src/ to exist relative to the crate root"
    );
    let mut files = Vec::new();
    collect_rs_files(src_dir, &mut files);
    assert!(!files.is_empty(), "expected to find .rs files under src/");

    let mut hits = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file.display()));
        let filtered = strip_allowlisted_regions(&content);
        hits.extend(scan_content(&file.display().to_string(), &filtered));
    }

    assert!(
        hits.is_empty(),
        "vimcode must contain no hard-coded colours outside a Theme definition (#1575). \
         Every colour must come from `theme.<field>` (or a derived `Color::rgb(theme.x.r, \
         theme.x.g, theme.x.b)` conversion), not a literal RGB triple, hex string or Cairo \
         `set_source_rgb` call. Add the colour as a `Theme` field (or a value derived from \
         one) instead. Found:\n{}",
        hits.join("\n")
    );
}

/// Proves the scanner in this file actually catches something (and doesn't
/// just vacuously pass): a planted literal outside any allowlisted region
/// is flagged, the same literal inside `#[cfg(test)]` or a theme
/// constructor is not, and a `Theme`-field conversion (the idiom every
/// real call site uses) is never mistaken for a literal.
#[test]
fn scanner_flags_planted_violations_and_ignores_allowlisted_ones() {
    // A literal RGB triple in ordinary (non-test, non-theme) code: caught.
    let bad_rgb = "fn paint() {\n    let c = quadraui::Color::rgb(10, 20, 30);\n}\n";
    let hits = scan_content("bad_rgb.rs", &strip_allowlisted_regions(bad_rgb));
    assert!(
        !hits.is_empty(),
        "expected a literal Color::rgb(...) call outside any allowlist to be flagged"
    );

    // A literal hex string in ordinary code: caught.
    let bad_hex = "fn paint() {\n    let c = hex(\"#112233\");\n}\n";
    let hits = scan_content("bad_hex.rs", &strip_allowlisted_regions(bad_hex));
    assert!(
        !hits.is_empty(),
        "expected a literal hex(\"#...\") call outside any allowlist to be flagged"
    );

    // A literal Cairo set_source_rgb call in ordinary code: caught.
    let bad_cairo = "fn paint(cr: &Context) {\n    cr.set_source_rgb(0.1, 0.2, 0.3);\n}\n";
    let hits = scan_content("bad_cairo.rs", &strip_allowlisted_regions(bad_cairo));
    assert!(
        !hits.is_empty(),
        "expected a literal set_source_rgb(...) call outside any allowlist to be flagged"
    );

    // The exact same literal, wrapped in #[cfg(test)]: not flagged.
    let test_wrapped = "#[cfg(test)]\nmod tests {\n    fn t() {\n        let c = quadraui::Color::rgb(10, 20, 30);\n    }\n}\n";
    let hits = scan_content("test_wrapped.rs", &strip_allowlisted_regions(test_wrapped));
    assert!(
        hits.is_empty(),
        "#[cfg(test)] code must be exempt from the gate, found: {hits:?}"
    );

    // The exact same literal, inside an allowlisted theme constructor: not
    // flagged.
    let theme_wrapped = "impl Theme {\n    pub fn onedark() -> Self {\n        let bg = quadraui::Color::rgb(10, 20, 30);\n        Self {}\n    }\n}\n";
    let hits = scan_content(
        "theme_wrapped.rs",
        &strip_allowlisted_regions(theme_wrapped),
    );
    assert!(
        hits.is_empty(),
        "a Theme constructor body must be exempt from the gate, found: {hits:?}"
    );

    // The standard "convert a Theme field to a quadraui::Color" idiom used
    // throughout render.rs: never a literal, so never flagged.
    let field_conversion =
        "fn f(theme: &Theme) {\n    let c = quadraui::Color::rgb(theme.cursor.r, theme.cursor.g, theme.cursor.b);\n}\n";
    let hits = scan_content(
        "field_conversion.rs",
        &strip_allowlisted_regions(field_conversion),
    );
    assert!(
        hits.is_empty(),
        "a Color::rgb(...) call with Theme-field arguments must never be flagged, found: {hits:?}"
    );

    // `from_rgb(r, g, b)` with variable arguments (icons.rs's identity
    // palette -> Color conversion, #703): never flagged.
    let icon_conversion =
        "fn tab_icon_color(name: &str) -> Color {\n    let (r, g, b) = icons::file_icon_color_for_name(name);\n    Color::from_rgb(r, g, b)\n}\n";
    let hits = scan_content(
        "icon_conversion.rs",
        &strip_allowlisted_regions(icon_conversion),
    );
    assert!(
        hits.is_empty(),
        "Color::from_rgb(...) with variable arguments must never be flagged, found: {hits:?}"
    );
}

/// #1576: the `#[cfg(test)]` exemption must survive a stray brace inside a
/// string literal, a comment or a char literal. `block_end` used to count
/// every `{`/`}` character, so a single `"{"` in a test body desynchronised
/// the depth count for the rest of the file, the `mod tests` block never
/// appeared to close, and the exemption silently stopped applying from
/// there on.
#[test]
fn cfg_test_exemption_survives_braces_inside_strings_comments_and_chars() {
    // A brace that only appears inside a string literal, an escaped-quote
    // string, a raw string, a `//` comment and a char literal — the module
    // still closes on its real `}`, so the literal inside stays exempt.
    let tricky = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() {\n",
        "        let unbalanced = \"{\";\n",
        "        let escaped = \"he said \\\"{\\\" loudly\";\n",
        "        let raw = r#\"{{{\"#;\n",
        "        let brace_char = '{';\n",
        "        // a comment with a lone { in it\n",
        "        let c = quadraui::Color::rgb(10, 20, 30);\n",
        "    }\n",
        "}\n",
        "fn shipped() {\n",
        "    let c = quadraui::Color::rgb(40, 50, 60);\n",
        "}\n",
    );
    let hits = scan_content("tricky.rs", &strip_allowlisted_regions(tricky));
    assert_eq!(
        hits.len(),
        1,
        "the literal inside the #[cfg(test)] module must stay exempt and the \
         one in shipped code below it must still be flagged, found: {hits:?}"
    );
    assert!(
        hits[0].contains("40, 50, 60"),
        "the flagged hit must be the shipped-code literal, not the test one: {hits:?}"
    );

    // A lifetime is not a char literal: consuming `'a` as one would swallow
    // the rest of the line (including its braces) and break the count.
    let mut state = ScanState::default();
    assert!(
        strip_non_code_runs("impl<'a> Foo<'a> { fn f(&'a self) -> &'a str {", &mut state)
            .contains("{ "),
        "lifetimes must not be mistaken for char literals"
    );
    assert_eq!(state, ScanState::Code, "that line opens nothing");
    assert_eq!(
        strip_non_code_runs("let brace = '{'; if x {", &mut state)
            .matches('{')
            .count(),
        1,
        "only the structural brace should survive; the char literal's must not"
    );

    // A *multi-line* raw string — `render.rs`'s `test_strip_json_comments`
    // shape. The opening `{` and the closing `}` must both be invisible to
    // the brace count, or the enclosing block ends thousands of lines early.
    let mut state = ScanState::default();
    let opened = strip_non_code_runs("        let input = r#\"{", &mut state);
    assert!(!opened.contains('{'), "a raw string's brace must not count");
    assert_eq!(state, ScanState::RawStr(1), "the raw string stays open");
    let inside = strip_non_code_runs("  \"key\": \"value\", /* block */ }", &mut state);
    assert!(
        !inside.contains('}'),
        "a line wholly inside a raw string contributes no braces, got {inside:?}"
    );
    let closed = strip_non_code_runs("}\"#;", &mut state);
    assert!(
        !closed.contains('}'),
        "the raw string's closing line contributes no braces either, got {closed:?}"
    );
    assert_eq!(state, ScanState::Code, "the raw string closed on `\"#`");

    // A multi-line `/* ... */` comment, nesting included.
    let mut state = ScanState::default();
    assert!(!strip_non_code_runs("/* opens { and /* nests", &mut state).contains('{'));
    assert_eq!(state, ScanState::BlockComment(2));
    assert!(!strip_non_code_runs("still commented }", &mut state).contains('}'));
    assert_eq!(
        strip_non_code_runs("*/ */ fn f() {", &mut state)
            .matches('{')
            .count(),
        1,
        "code after the comment closes must count again"
    );
}

/// #1576 regression guard, against the real file that exposed the bug:
/// `src/tui_main/app_on_tui_tests.rs` is one 5.6k-line `#[cfg(test)] mod
/// tests`, and its raw brace count never returns to zero (braces inside
/// strings and comments), so before [`strip_non_code_runs`] the whole
/// test-only file was scanned. Asserts the file does contain literals the
/// gate would otherwise flag, and that the exemption blanks every one.
#[test]
fn a_wholly_cfg_test_file_with_colour_literals_is_exempt_end_to_end() {
    let path = Path::new("src/tui_main/app_on_tui_tests.rs");
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    assert!(
        content
            .lines()
            .next()
            .unwrap_or("")
            .starts_with("#[cfg(test)]"),
        "this guard assumes {} is wholly #[cfg(test)]",
        path.display()
    );
    let raw_hits = scan_content("raw", &content);
    assert!(
        !raw_hits.is_empty(),
        "this guard is only meaningful while {} still contains colour \
         literals in its test bodies — if it no longer does, point it at \
         another wholly-#[cfg(test)] file that does",
        path.display()
    );
    let filtered_hits = scan_content(
        &path.display().to_string(),
        &strip_allowlisted_regions(&content),
    );
    assert!(
        filtered_hits.is_empty(),
        "every colour literal in a wholly-#[cfg(test)] file must be exempt \
         (#1575's own allowlist says so), found: {filtered_hits:?}"
    );
}
