//! CI gate for issue #197: every Nerd Font codepoint referenced by
//! `Icon::new(...)` in `src/icons.rs` must actually be present in the
//! bundled `data/fonts/vimcode-icons.ttf` subset, or the GTK explorer tree
//! (and anywhere else that glyph is drawn) silently falls back to whatever
//! the system font substitutes for the PUA codepoint -- tofu or a
//! wrong-looking glyph, with no build-time signal.
//!
//! This test parses the codepoint list straight out of `src/icons.rs`'s
//! source text (so it can never drift from the actual `Icon::new` calls)
//! and the font's own `cmap` table (parsed by hand, format 4 + format 12 --
//! no font-parsing crate dependency needed for this one check), and asserts
//! every "nerd" codepoint (>= U+E000; see `scripts/gen_icon_font.py`'s
//! module doc for why that threshold is the right split) is covered.
//!
//! Regenerate the subset with `scripts/gen_icon_font.py` if this fails after
//! a new `Icon::new` call is added -- see that script's docstring for the
//! source fonts it needs and why two are needed (upstream nerd-fonts >=
//! v3.1.0 dropped three codepoints vimcode still uses).
//!
//! No `#[cfg(feature = ...)]` gate: this doesn't touch GTK, TUI, or any
//! optional dependency, so it runs in both the `--no-default-features` and
//! default-features CI lanes for free.

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
