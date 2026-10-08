//! CI gate for issue #197: every Nerd Font codepoint referenced by
//! `Icon::new(...)` in `src/icons.rs` must actually be present in the
//! bundled `data/fonts/vimcode-icons.ttf`, or the GTK explorer tree (and
//! anywhere else that glyph is drawn) silently falls back to whatever the
//! system font substitutes for the PUA codepoint -- tofu or a wrong-looking
//! glyph, with no build-time signal.
//!
//! This test parses the codepoint list straight out of `src/icons.rs`'s
//! source text (so it can never drift from the actual `Icon::new` calls)
//! and the font's own `cmap` table (parsed by hand, format 4 + format 12 --
//! no font-parsing crate dependency needed for this one check), and asserts
//! every "nerd" codepoint (>= U+E000; see `scripts/gen_icon_font.py`'s
//! module doc for why that threshold is the right split) is covered.
//!
//! Regenerate the font with `scripts/gen_icon_font.py` if this fails after
//! a new `Icon::new` call is added -- see that script's docstring for the
//! source fonts it needs and why two are needed (upstream nerd-fonts >=
//! v3.1.0 dropped three codepoints vimcode still uses).
//!
//! ## #1853: the bundled font is the *entire* Symbols Nerd Font, not a subset
//!
//! Before #1853, `data/fonts/vimcode-icons.ttf` was subset down to exactly
//! the 114 codepoints `src/icons.rs` referenced. That covered every icon
//! vimcode itself draws but nothing else: a `vimcode-ext` registry
//! extension's panel icon is an arbitrary Nerd Font codepoint the extension
//! author picked, not one `scripts/gen_icon_font.py` ever knew about, so it
//! rendered as tofu on any platform without a system-wide Nerd Font (native
//! macOS/Windows, or GTK without one installed) -- caught via the Git
//! Insights extension's `git_log_panel.lua`, which declares U+F1D3
//! (`nf-fa-git`) as its panel icon and U+F15B/U+E7A8/U+F81F/U+E74E/U+E628/
//! U+E620/U+E626 for file-type glyphs, none of which `src/icons.rs`
//! referenced. The font now bundles nerd-fonts' entire Symbols Nerd Font
//! Mono cmap (~10,600 codepoints), so this class of bug can't recur for any
//! codepoint a Nerd Font actually assigns.
//! `bundled_font_covers_the_git_insights_extension_panel_icon` below is a
//! named regression anchor for exactly the codepoints that motivated it;
//! `bundled_font_covers_every_icon_codepoint` above keeps covering
//! vimcode's own icons specifically.
//!
//! No `#[cfg(feature = ...)]` gate: this doesn't touch GTK, TUI, or any
//! optional dependency, so it runs in both the `--no-default-features` and
//! default-features CI lanes for free.
//!
//! ## #1540: raw PUA literals bypass this gate entirely
//!
//! The check above only ever sees codepoints that go through `Icon::new(...)`
//! in `src/icons.rs`. #1540 was a macOS status bar showing tofu for the
//! sidebar/panel/menu toggles and the done-notification bell: those glyphs
//! were `\u{f0616}`/`\u{f018d}`/`\u{f009e}` written directly as raw literals
//! in `render.rs` (a couple as literal PUA *characters* in the source, not
//! even `\u{...}` escapes), so `scripts/gen_icon_font.py` and the test above
//! never knew the codepoints existed, and the bundled subset font never
//! picked them up. `no_raw_pua_literals_outside_icons_rs` below closes that
//! gap structurally: it scans every `.rs` file under `src/` (except
//! `icons.rs` itself and the test-support exemptions documented on
//! `is_exempt_path`) for any PUA character or `\u{...}` PUA escape, so a
//! future icon literal added straight to `render.rs` (or anywhere else) fails
//! the build immediately instead of silently shipping as tofu on whatever
//! platform's system font doesn't happen to share the same PUA assignment a
//! Nerd Font uses.

use regex::Regex;

const ICONS_RS_SOURCE: &str = include_str!("../src/icons.rs");
const FONT_BYTES: &[u8] = include_bytes!("../data/fonts/vimcode-icons.ttf");

/// Nerd Font glyphs live in the Private Use Areas: U+E000-U+F8FF (BMP PUA)
/// and U+F0000-U+FFFFD (Supplementary PUA-A, used by extended glyphs like
/// `\u{f035c}`). A handful of `Icon::new` calls (GTK client-side-titlebar
/// window controls, #552/#715) deliberately use ordinary BMP Unicode below
/// this and are covered by any system font -- they're not part of this
/// font's job. Keep this threshold in sync with
/// `scripts/gen_icon_font.py::NERD_RANGE_START`.
const NERD_RANGE_START: u32 = 0xE000;

/// Every codepoint passed as the first (`nerd`) argument to `Icon::new` in
/// `src/icons.rs`, parsed straight out of the source text so this list can
/// never drift from the actual constants -- mirrors
/// `scripts/gen_icon_font.py::referenced_codepoints`.
fn referenced_nerd_codepoints() -> Vec<u32> {
    // The `[^"]*` after the `\u{...}` escape tolerates trailing literal
    // characters before the closing quote (e.g. `"\u{f0da} "` -- a trailing
    // space, as used by EXPAND_DOWN/COLLAPSE_RIGHT in src/icons.rs to pad the
    // rendered glyph). Requiring the closing `"` to immediately follow `}`
    // silently dropped those codepoints from the "wanted" set (#197
    // fix-iteration-1 review finding). Keep in sync with
    // `scripts/gen_icon_font.py::ICON_NEW_RE`.
    let re = Regex::new(r#"Icon::new\(\s*"\\u\{([0-9a-fA-F]+)\}[^"]*""#).unwrap();
    let mut codepoints: Vec<u32> = re
        .captures_iter(ICONS_RS_SOURCE)
        .map(|caps| u32::from_str_radix(&caps[1], 16).expect("hex codepoint"))
        .filter(|&cp| cp >= NERD_RANGE_START)
        .collect();
    codepoints.sort_unstable();
    codepoints.dedup();
    codepoints
}

// ─── Minimal hand-rolled sfnt/cmap reader ───────────────────────────────────
//
// Deliberately not a crate dependency: this is the one check that must stay
// runnable with zero extra tooling (`cargo test`, nothing else), matching
// the design note in the issue this file closes out. It only needs to
// answer "is this codepoint mapped to a nonzero glyph ID", not shape text,
// so it only implements cmap subtable formats 4 and 12 -- the two that
// `fontTools`'s subsetter actually emits (verified against the generated
// font: platform (0,3)/(3,1) format 4 for the BMP part, (0,4)/(3,10) format
// 12 for the full range including the two supplementary-plane glyphs).

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_i16(data: &[u8], offset: usize) -> i16 {
    i16::from_be_bytes([data[offset], data[offset + 1]])
}

/// Locate the `cmap` table within an sfnt (TrueType) blob and return its
/// byte range `[start, end)` within `data`.
fn find_cmap_table(data: &[u8]) -> (usize, usize) {
    let num_tables = read_u16(data, 4) as usize;
    const DIR_START: usize = 12;
    const ENTRY_SIZE: usize = 16;
    for i in 0..num_tables {
        let entry = DIR_START + i * ENTRY_SIZE;
        let tag = &data[entry..entry + 4];
        if tag == b"cmap" {
            let offset = read_u32(data, entry + 8) as usize;
            let length = read_u32(data, entry + 12) as usize;
            return (offset, offset + length);
        }
    }
    panic!("no cmap table found in font");
}

/// Every codepoint mapped to a nonzero glyph ID by cmap subtable format 4,
/// which starts at `data[sub_start]`.
fn format4_codepoints(data: &[u8], sub_start: usize) -> Vec<u32> {
    let seg_count_x2 = read_u16(data, sub_start + 6) as usize;
    let seg_count = seg_count_x2 / 2;
    let end_codes = sub_start + 14;
    let start_codes = end_codes + seg_count_x2 + 2; // +2 skips reservedPad
    let id_deltas = start_codes + seg_count_x2;
    let id_range_offsets = id_deltas + seg_count_x2;

    let mut out = Vec::new();
    for seg in 0..seg_count {
        let end_code = read_u16(data, end_codes + seg * 2) as u32;
        let start_code = read_u16(data, start_codes + seg * 2) as u32;
        if start_code == 0xFFFF && end_code == 0xFFFF {
            continue; // terminator segment
        }
        let id_delta = read_i16(data, id_deltas + seg * 2);
        let id_range_offset = read_u16(data, id_range_offsets + seg * 2);

        for cp in start_code..=end_code {
            let glyph_id: u32 = if id_range_offset == 0 {
                (cp as i32 + id_delta as i32) as u16 as u32
            } else {
                // Per the OpenType spec's format-4 lookup algorithm.
                let addr = id_range_offsets
                    + seg * 2
                    + id_range_offset as usize
                    + (cp - start_code) as usize * 2;
                let raw = read_u16(data, addr) as u32;
                if raw == 0 {
                    0
                } else {
                    ((raw as i32 + id_delta as i32) as u16) as u32
                }
            };
            if glyph_id != 0 {
                out.push(cp);
            }
        }
    }
    out
}

/// Every codepoint mapped to a nonzero glyph ID by cmap subtable format 12,
/// which starts at `data[sub_start]`.
fn format12_codepoints(data: &[u8], sub_start: usize) -> Vec<u32> {
    let num_groups = read_u32(data, sub_start + 12) as usize;
    let groups_start = sub_start + 16;
    let mut out = Vec::new();
    for g in 0..num_groups {
        let base = groups_start + g * 12;
        let start_char = read_u32(data, base);
        let end_char = read_u32(data, base + 4);
        let start_glyph = read_u32(data, base + 8);
        for (i, cp) in (start_char..=end_char).enumerate() {
            if start_glyph + i as u32 != 0 {
                out.push(cp);
            }
        }
    }
    out
}

/// Union of every codepoint covered by any format-4 or format-12 subtable in
/// the font's `cmap` table.
fn font_covered_codepoints(data: &[u8]) -> std::collections::HashSet<u32> {
    let (cmap_start, _cmap_end) = find_cmap_table(data);
    let num_subtables = read_u16(data, cmap_start + 2) as usize;

    let mut covered = std::collections::HashSet::new();
    for i in 0..num_subtables {
        let record = cmap_start + 4 + i * 8;
        let sub_offset = read_u32(data, record + 4) as usize;
        let sub_start = cmap_start + sub_offset;
        let format = read_u16(data, sub_start);
        match format {
            4 => covered.extend(format4_codepoints(data, sub_start)),
            12 => covered.extend(format12_codepoints(data, sub_start)),
            _ => {} // formats other than 4/12 aren't emitted by our subsetter; skip
        }
    }
    covered
}

#[test]
fn bundled_font_covers_every_icon_codepoint() {
    let wanted = referenced_nerd_codepoints();
    assert!(
        !wanted.is_empty(),
        "regex found zero Icon::new(...) nerd codepoints in src/icons.rs -- \
         the parser almost certainly broke, not that icons.rs went empty"
    );

    let covered = font_covered_codepoints(FONT_BYTES);

    let missing: Vec<u32> = wanted
        .iter()
        .copied()
        .filter(|cp| !covered.contains(cp))
        .collect();

    assert!(
        missing.is_empty(),
        "data/fonts/vimcode-icons.ttf is missing {} codepoint(s) referenced \
         by Icon::new(...) in src/icons.rs: {}\n\
         Regenerate with scripts/gen_icon_font.py -- see that script's \
         docstring for source fonts and the --legacy-source merge it needs.",
        missing.len(),
        missing
            .iter()
            .map(|cp| format!("U+{cp:04X}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn sanity_known_codepoints_are_parsed_from_icons_rs() {
    // Anchors the regex against known constants so a change to Icon::new's
    // call shape (not just its argument list) fails loudly here instead of
    // silently returning zero matches (which the test above only catches via
    // the "found zero" assertion, a much less specific signal).
    let wanted = referenced_nerd_codepoints();
    assert!(
        wanted.contains(&0xF07C),
        "EXPLORER/FOLDER_OPEN (\\u{{f07c}})"
    );
    assert!(wanted.contains(&0xEA76), "FIND_CLOSE (\\u{{ea76}})");
    assert!(wanted.contains(&0xF81F), "FILE_PYTHON (\\u{{f81f}})");
    // Plain-Unicode window-control glyphs (#552/#715) must NOT show up here
    // -- they're below the PUA threshold and are intentionally not part of
    // this font's job (see NERD_RANGE_START doc comment).
    assert!(
        !wanted.contains(&0x2014),
        "WINDOW_MINIMIZE is plain Unicode, not a nerd codepoint"
    );
}

#[test]
fn bundled_font_covers_the_explorer_view_actions_glyphs() {
    // #1693 named-regression anchor: the Explorer view-actions toolbar
    // introduced the first two `Icon::new` codepoints that were *not*
    // already in the bundled subset (U+F066 `fa-compress` for Collapse
    // All, U+F141 `fa-ellipsis_h` for the "..." overflow menu), so the
    // branch shipped a font that rendered both as tofu until the subset
    // was regenerated. `bundled_font_covers_every_icon_codepoint` above
    // catches this generically; this test names the two codepoints so the
    // failure says *which* feature lost its glyphs rather than just
    // printing a bare codepoint list.
    let covered = font_covered_codepoints(FONT_BYTES);
    for (cp, what) in [
        (0xF066, "EXPLORER_COLLAPSE_ALL (nf-fa-compress)"),
        (0xF141, "EXPLORER_OVERFLOW (nf-fa-ellipsis_h)"),
    ] {
        assert!(
            covered.contains(&cp),
            "data/fonts/vimcode-icons.ttf is missing U+{cp:04X}, used by \
             {what} -- regenerate the subset with scripts/gen_icon_font.py"
        );
    }
}

#[test]
fn bundled_font_covers_the_git_insights_extension_panel_icon() {
    // #1853 named-regression anchor: the Git Insights extension
    // (`vimcode-ext`'s `git_log_panel.lua`) declares `icon = "\u{f1d3}"`
    // (nf-fa-git) for its sidebar panel, plus U+F15B/U+E7A8/U+F81F/U+E74E/
    // U+E628/U+E620/U+E626 for file-type glyphs it draws in the log view --
    // none of which `src/icons.rs` references, so none were covered by the
    // pre-#1853 114-codepoint subset and all rendered as tofu on any
    // platform without a system-wide Nerd Font (native macOS/Windows, GTK
    // without one installed). `bundled_font_covers_every_icon_codepoint`
    // above only ever checks codepoints `src/icons.rs` itself references,
    // so it could not have caught this -- these codepoints are a registry
    // extension's own choice, invisible to that regex. This test names them
    // directly against the bundled font's cmap so a future shrink back to a
    // vimcode-only subset fails loudly here instead of shipping silent tofu
    // to extension authors again.
    let covered = font_covered_codepoints(FONT_BYTES);
    for (cp, what) in [
        (0xF1D3, "Git Insights panel icon (nf-fa-git)"),
        (0xF15B, "git_log_panel.lua file-type glyph"),
        (0xE7A8, "git_log_panel.lua file-type glyph"),
        (0xF81F, "git_log_panel.lua file-type glyph (nf-dev-python)"),
        (0xE74E, "git_log_panel.lua file-type glyph"),
        (0xE628, "git_log_panel.lua file-type glyph"),
        (0xE620, "git_log_panel.lua file-type glyph"),
        (0xE626, "git_log_panel.lua file-type glyph"),
    ] {
        assert!(
            covered.contains(&cp),
            "data/fonts/vimcode-icons.ttf is missing U+{cp:04X}, used by \
             the Git Insights extension's {what} -- the bundled font must \
             be the full Symbols Nerd Font (see scripts/gen_icon_font.py), \
             not a vimcode-icons.rs-only subset"
        );
    }
}

// ─── #1540: no raw PUA literal outside icons.rs ─────────────────────────────

/// Same PUA ranges the #1540 bug used: BMP Private Use Area, plus the
/// Supplementary Private Use Areas nerd-fonts' extended glyphs live in (see
/// this file's module doc and `scripts/gen_icon_font.py`'s `NERD_RANGE_START`
/// doc comment -- this check additionally covers U+F0000 and up, which that
/// simpler `>= U+E000` threshold already includes).
fn is_pua_codepoint(cp: u32) -> bool {
    (0xE000..=0xF8FF).contains(&cp) || cp >= 0xF0000
}

/// Paths exempt from the "no raw PUA literal outside `icons.rs`" rule.
/// These are test-support code where a PUA literal is legitimate *data*,
/// not a new, unaudited UI icon:
///
/// - `src/harness/**`: fixtures that register plugin panels via
///   `PanelRegistration`/`ExtPanelItem`, whose `icon` fields are
///   provider-supplied at runtime -- inherently arbitrary data no
///   `icons.rs` table could enumerate, not a vimcode-owned glyph.
/// - `**/testing.rs` (e.g. `src/gtk/testing.rs`, the `GtkDriver` harness
///   from #646) and `**/*_tests.rs`: `#[cfg(test)]`-only modules that
///   sometimes assert against a *specific known* codepoint on purpose
///   (including, in one regression test, a deliberately-deleted constant's
///   old codepoint) -- the literal there is the point of the assertion.
fn is_exempt_from_pua_scan(repo_relative_path: &str) -> bool {
    let path = std::path::Path::new(repo_relative_path);
    if path.components().any(|c| c.as_os_str() == "harness") {
        return true;
    }
    match path.file_name().and_then(|f| f.to_str()) {
        Some("testing.rs") => true,
        Some(name) => name.ends_with("_tests.rs"),
        None => false,
    }
}

/// Recursively collect every `.rs` file under `dir`.
fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("read_dir entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_raw_pua_literals_outside_icons_rs() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_root = manifest_dir.join("src");

    let mut files = Vec::new();
    collect_rs_files(&src_root, &mut files);
    assert!(
        !files.is_empty(),
        "found zero .rs files under src/ -- collect_rs_files almost \
         certainly broke, not that src/ went empty"
    );

    // Mirrors `ICON_NEW_RE`'s escape shape, but unanchored (any `\u{...}`
    // anywhere on a line, not just inside an `Icon::new(...)` call) since
    // this check's whole point is catching PUA literals *outside* that call
    // shape.
    let escape_re = Regex::new(r"\\u\{([0-9a-fA-F]{4,6})\}").unwrap();

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(manifest_dir).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str == "src/icons.rs" || is_exempt_from_pua_scan(&rel_str) {
            continue;
        }

        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        for (line_no, line) in text.lines().enumerate() {
            for ch in line.chars() {
                if is_pua_codepoint(ch as u32) {
                    violations.push(format!(
                        "{rel_str}:{}: raw PUA character U+{:04X}",
                        line_no + 1,
                        ch as u32
                    ));
                }
            }
            for caps in escape_re.captures_iter(line) {
                if let Ok(cp) = u32::from_str_radix(&caps[1], 16) {
                    if is_pua_codepoint(cp) {
                        violations.push(format!(
                            "{rel_str}:{}: \\u{{...}} escape for U+{cp:04X}",
                            line_no + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "found {} raw PUA literal(s) outside src/icons.rs (see #1540) -- \
         move each one into an `Icon::new(...)` constant in src/icons.rs, \
         reference it via `crate::icons::<NAME>`, and regenerate the \
         bundled subset font with scripts/gen_icon_font.py:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn pua_scan_exemptions_match_known_test_support_files() {
    // Anchors `is_exempt_from_pua_scan` against the concrete files it exists
    // to exempt, so a future refactor of the predicate can't silently widen
    // (or narrow) it without a test noticing.
    assert!(is_exempt_from_pua_scan("src/harness/plugin_panel.rs"));
    assert!(is_exempt_from_pua_scan("src/harness/plugin_panel/tests.rs"));
    assert!(is_exempt_from_pua_scan("src/gtk/testing.rs"));
    assert!(is_exempt_from_pua_scan("src/tui_main/app_on_tui_tests.rs"));
    assert!(!is_exempt_from_pua_scan("src/render.rs"));
    assert!(!is_exempt_from_pua_scan("src/icons.rs"));
    assert!(!is_exempt_from_pua_scan("src/core/lsp.rs"));
}
