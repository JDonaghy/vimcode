#!/usr/bin/env python3
"""Generate/verify the bundled vimcode-icons.ttf Nerd Font subset.

Parses the nerd-font codepoints referenced by `Icon::new(...)` calls in
`src/icons.rs`, then either subsets a source Nerd Font down to exactly those
codepoints (default mode) or verifies an existing subset font covers them
(`--verify` mode, no source font needed).

Only codepoints in the Private Use Area (>= U+E000) are treated as "nerd"
codepoints that must come from the Nerd Font subset. A handful of
`Icon::new` calls (the GTK client-side-titlebar window controls, #552/#715)
intentionally use ordinary BMP Unicode below U+E000 and are covered by any
system font instead -- they are deliberately excluded from both the subset
and the coverage check. See `src/icons.rs`'s "Window Controls" section for
why.

Usage:
    # Regenerate the bundled subset from a source Nerd Font. Get the source
    # from https://github.com/ryanoasis/nerd-fonts releases
    # (NerdFontsSymbolsOnly.zip -> SymbolsNerdFont-Regular.ttf) -- it is not
    # vendored in this repo.
    python3 scripts/gen_icon_font.py --source SymbolsNerdFont-Regular.ttf

    # Verify (used by hand, or CI) that the bundled subset covers every nerd
    # codepoint referenced in src/icons.rs -- no source font needed. This is
    # a convenience CLI; the actual CI gate is the Rust test
    # tests/icon_font_coverage.rs, which runs as part of `cargo test` with no
    # Python dependency.
    python3 scripts/gen_icon_font.py --verify

Requires `fontTools` (`pip install fonttools`), only for the non-`--verify`
(generation) path and for `--verify` itself if you want to use this script
rather than `cargo test`.
"""
import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ICONS_RS = REPO_ROOT / "src" / "icons.rs"
DEFAULT_OUTPUT = REPO_ROOT / "data" / "fonts" / "vimcode-icons.ttf"

# Nerd Font glyphs live in the Private Use Areas: U+E000-U+F8FF (BMP PUA) and
# U+F0000-U+FFFFD (Supplementary PUA-A, used by some extended Nerd Font
# glyphs such as \u{f035c}). Ordinary Unicode never uses these ranges, so
# `codepoint >= 0xE000` cleanly separates "must come from the Nerd Font
# subset" from "ordinary glyph, any system font covers it" -- see the module
# docstring.
NERD_RANGE_START = 0xE000

# Matches only the first ("nerd") string literal argument of `Icon::new(...)`
# -- the second (fallback) argument is deliberately not captured here, so
# this can never accidentally require a PUA glyph to cover a fallback char.
ICON_NEW_RE = re.compile(r'Icon::new\(\s*"\\u\{([0-9a-fA-F]+)\}"')


def referenced_codepoints() -> list[int]:
    """Every codepoint passed as the first (`nerd`) argument to `Icon::new`
    in src/icons.rs, parsed straight out of the source so this list is never
    hand-maintained."""
    text = ICONS_RS.read_text()
    return sorted({int(m, 16) for m in ICON_NEW_RE.findall(text)})


def nerd_codepoints() -> list[int]:
    return [cp for cp in referenced_codepoints() if cp >= NERD_RANGE_START]


def verify(font_path: Path) -> int:
    from fontTools.ttLib import TTFont

    font = TTFont(str(font_path))
    cmap = font.getBestCmap()
    wanted = nerd_codepoints()
    missing = [cp for cp in wanted if cp not in cmap]
    print(
        f"{font_path}: {len(wanted)} nerd codepoints referenced in "
        f"icons.rs, {len(wanted) - len(missing)} covered"
    )
    if missing:
        print("MISSING:")
        for cp in missing:
            print(f"  U+{cp:04X}")
        return 1
    print("OK: all referenced nerd codepoints are covered.")
    return 0


def _merge_legacy_glyphs(primary_path: Path, legacy_path: Path, missing: list[int]) -> Path:
    """Copy glyph outlines for `missing` codepoints from `legacy_path` into a
    copy of `primary_path`, returning the path to the patched copy.

    Why this exists: nerd-fonts renumbered its Material Design Icons range
    between v3.0.0 and v3.1.0 upstream, which silently dropped three
    codepoints vimcode still references (U+F6A9 `DBG_VARIABLES`, U+F81D
    `FILE_JS`, U+F81F `FILE_PYTHON`) from every release from v3.1.0 through
    the current one (checked through v3.5.1) -- no current release's font,
    Symbols-only or Complete, contains them. They *are* present in the
    v2.1.0 nerd-fonts release (e.g. `Hack.zip`'s "Hack Regular Nerd Font
    Complete.ttf"), which is what `--legacy-source` is for. Only reach for
    this if `verify()` on a fresh primary-only subset reports one of these
    three (or another codepoint upstream has since dropped) as missing --
    don't use it preemptively.

    Requires the legacy glyphs to be simple (non-composite) TrueType outlines
    and both fonts to share `unitsPerEm`, which holds for the pairing above;
    raises if either assumption doesn't hold rather than silently emitting a
    mis-scaled or broken glyph.
    """
    from fontTools.ttLib import TTFont

    primary = TTFont(str(primary_path))
    legacy = TTFont(str(legacy_path))

    if primary["head"].unitsPerEm != legacy["head"].unitsPerEm:
        raise SystemExit(
            f"--legacy-source unitsPerEm ({legacy['head'].unitsPerEm}) does not "
            f"match primary source ({primary['head'].unitsPerEm}); refusing to "
            "copy glyph outlines without rescaling."
        )

    legacy_cmap = legacy.getBestCmap()
    primary_cmap = primary.getBestCmap()
    primary_glyf = primary["glyf"]
    primary_hmtx = primary["hmtx"]
    legacy_glyf = legacy["glyf"]
    legacy_hmtx = legacy["hmtx"]

    for cp in missing:
        if cp not in legacy_cmap:
            raise SystemExit(f"U+{cp:04X} is missing from --legacy-source too; cannot merge it in.")
        legacy_gname = legacy_cmap[cp]
        legacy_glyph = legacy_glyf[legacy_gname]
        if legacy_glyph.isComposite():
            raise SystemExit(
                f"U+{cp:04X} ({legacy_gname}) is a composite glyph in --legacy-source; "
                "this merge only supports simple outlines."
            )

        new_gname = f"legacy_{cp:04x}"
        primary.getGlyphOrder()  # ensure glyphOrder is populated before mutating
        primary["glyf"].glyphs[new_gname] = legacy_glyph
        if new_gname not in primary.getGlyphOrder():
            primary.setGlyphOrder(primary.getGlyphOrder() + [new_gname])
        primary_hmtx[new_gname] = legacy_hmtx[legacy_gname]
        primary_cmap[cp] = new_gname

        for table in primary["cmap"].tables:
            if table.isUnicode():
                table.cmap[cp] = new_gname

    patched_path = primary_path.with_name(primary_path.stem + ".legacy-merged.ttf")
    primary.save(str(patched_path))
    return patched_path


def generate(source: Path, output: Path, legacy_source: Path | None = None) -> int:
    from fontTools import subset
    from fontTools.ttLib import TTFont

    codepoints = nerd_codepoints()

    actual_source = source
    if legacy_source is not None:
        primary_cmap = TTFont(str(source)).getBestCmap()
        missing = [cp for cp in codepoints if cp not in primary_cmap]
        if missing:
            actual_source = _merge_legacy_glyphs(source, legacy_source, missing)

    unicodes_arg = ",".join(f"U+{cp:04X}" for cp in codepoints)

    args = [
        str(actual_source),
        f"--unicodes={unicodes_arg}",
        f"--output-file={output}",
        "--glyph-names",
        "--layout-features=*",
        "--name-IDs=*",
        "--name-legacy",
        "--notdef-outline",
        "--recommended-glyphs",
    ]
    subset.main(args)

    return verify(output)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--source",
        type=Path,
        help="Source Nerd Font to subset (e.g. SymbolsNerdFont-Regular.ttf)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="Output path (default: data/fonts/vimcode-icons.ttf)",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Verify an existing subset covers all referenced codepoints; "
        "no --source needed",
    )
    parser.add_argument(
        "--legacy-source",
        type=Path,
        help="Fallback font to pull individual glyphs from when --source is "
        "missing some referenced codepoint (see _merge_legacy_glyphs "
        "docstring -- as of this writing, upstream nerd-fonts >= v3.1.0 "
        "dropped U+F6A9/U+F81D/U+F81F; v2.1.0's Hack Complete still has them)",
    )
    args = parser.parse_args()

    if args.verify:
        return verify(args.output)

    if not args.source:
        parser.error("--source is required unless --verify is given")

    return generate(args.source, args.output, args.legacy_source)


if __name__ == "__main__":
    sys.exit(main())
