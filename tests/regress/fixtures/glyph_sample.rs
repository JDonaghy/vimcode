// Glyph-rendering sample for `scripts/regress --guided`.
//
// Deliberately mixes character widths and classes that are easy for a
// buffer renderer to get subtly wrong: plain ASCII, accented Latin,
// box-drawing, wide (CJK) characters, and an emoji. Opened by the
// "buffer-glyph-rendering" checklist item (tests/regress/checklist.toml).

// ASCII baseline: abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789
// Accented Latin: café, naïve, façade, Zürich, résumé, jalapeño
// Box drawing:    ┌───────────┐  │  └───────────┘  ╔═══╗  ║
// Wide (CJK):     日本語のテキスト 中文字符 한국어
// Emoji + ZWJ:    🦀 ✅ 🧵 👩‍💻
// Ligature-ish:   fi fl ffi -- -> => != <=

fn glyph_sample() {
    let café = "naïve façade";
    println!("{café}");
}
