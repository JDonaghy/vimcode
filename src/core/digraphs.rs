//! Vim-compatible digraph table (`:h digraphs`, `:h i_CTRL-K`, #1160).
//!
//! A digraph is a two-character mnemonic (e.g. `a:`) that stands for one
//! Unicode character (`ä`). This is a curated subset of Vim's built-in
//! table (which itself follows RFC 1345) — the common Latin-1/Latin
//! Extended-A accented letters, currency/punctuation symbols, arrows, a
//! handful of math symbols, and the Greek alphabet. It is data, not logic;
//! `:h digraph-table` lists ~1400 entries in real Vim, so this table can
//! grow over time without touching the dispatch code in `engine/keys.rs`.
//!
//! Lookup order for a typed pair `(c1, c2)` is: exact match, then the pair
//! reversed — Vim accepts digraphs typed in either order for the ones where
//! that's unambiguous (`:h digraph-table`, "Some digraphs ... can be used in
//! two ways").

/// `(char1, char2, result)` triples. Keep sorted by `char1` then `char2` for
/// readability; lookup is linear (small table, not a hot path).
pub const BUILTIN_DIGRAPHS: &[(char, char, char)] = &[
    // ── Latin-1 / Latin Extended-A lowercase, accented vowels & consonants ──
    ('a', ':', 'ä'),
    ('a', '\'', 'á'),
    ('a', '!', 'à'),
    ('a', '>', 'â'),
    ('a', '?', 'ã'),
    ('a', '-', 'ā'),
    ('a', '(', 'ă'),
    ('a', ';', 'ą'),
    ('a', 'a', 'å'),
    ('a', 'e', 'æ'),
    ('o', ':', 'ö'),
    ('o', '\'', 'ó'),
    ('o', '!', 'ò'),
    ('o', '>', 'ô'),
    ('o', '?', 'õ'),
    ('o', '/', 'ø'),
    ('o', '"', 'ő'),
    ('o', 'e', 'œ'),
    ('e', ':', 'ë'),
    ('e', '\'', 'é'),
    ('e', '!', 'è'),
    ('e', '>', 'ê'),
    ('e', ';', 'ę'),
    ('e', '-', 'ē'),
    ('i', ':', 'ï'),
    ('i', '\'', 'í'),
    ('i', '!', 'ì'),
    ('i', '>', 'î'),
    ('i', '?', 'ĩ'),
    ('u', ':', 'ü'),
    ('u', '\'', 'ú'),
    ('u', '!', 'ù'),
    ('u', '>', 'û'),
    ('u', '0', 'ů'),
    ('u', '"', 'ű'),
    ('n', '?', 'ñ'),
    ('n', '\'', 'ń'),
    ('c', ',', 'ç'),
    ('c', '\'', 'ć'),
    ('c', '<', 'č'),
    ('c', '.', 'ċ'),
    ('s', '\'', 'ś'),
    ('s', '<', 'š'),
    ('s', ',', 'ş'),
    ('z', '\'', 'ź'),
    ('z', '<', 'ž'),
    ('z', '.', 'ż'),
    ('y', '\'', 'ý'),
    ('y', ':', 'ÿ'),
    ('d', '/', 'đ'),
    ('d', '<', 'ď'),
    ('l', '/', 'ł'),
    ('l', '\'', 'ĺ'),
    ('l', '<', 'ľ'),
    ('r', '<', 'ř'),
    ('t', '<', 'ť'),
    ('g', '(', 'ğ'),
    ('t', 'h', 'þ'),
    ('d', '-', 'ð'),
    ('s', 's', 'ß'),
    // ── Uppercase counterparts ──────────────────────────────────────────
    ('A', ':', 'Ä'),
    ('A', '\'', 'Á'),
    ('A', '!', 'À'),
    ('A', '>', 'Â'),
    ('A', '?', 'Ã'),
    ('A', 'A', 'Å'),
    ('A', 'E', 'Æ'),
    ('O', ':', 'Ö'),
    ('O', '\'', 'Ó'),
    ('O', '!', 'Ò'),
    ('O', '>', 'Ô'),
    ('O', '?', 'Õ'),
    ('O', '/', 'Ø'),
    ('O', 'E', 'Œ'),
    ('E', ':', 'Ë'),
    ('E', '\'', 'É'),
    ('E', '!', 'È'),
    ('E', '>', 'Ê'),
    ('I', ':', 'Ï'),
    ('I', '\'', 'Í'),
    ('I', '!', 'Ì'),
    ('I', '>', 'Î'),
    ('U', ':', 'Ü'),
    ('U', '\'', 'Ú'),
    ('U', '!', 'Ù'),
    ('U', '>', 'Û'),
    ('N', '?', 'Ñ'),
    ('C', ',', 'Ç'),
    ('C', '<', 'Č'),
    ('S', '<', 'Š'),
    ('Z', '<', 'Ž'),
    ('Y', '\'', 'Ý'),
    ('T', 'H', 'Þ'),
    ('D', '-', 'Ð'),
    // ── Punctuation, currency, symbols ───────────────────────────────────
    ('<', '<', '«'),
    ('>', '>', '»'),
    ('!', 'I', '¡'),
    ('?', 'I', '¿'),
    ('S', 'E', '§'),
    ('P', 'd', '£'),
    ('E', 'u', '€'),
    ('Y', 'e', '¥'),
    ('C', 't', '¢'),
    ('C', 'u', '¤'),
    ('C', 'o', '©'),
    ('R', 'g', '®'),
    ('T', 'M', '™'),
    ('D', 'G', '°'),
    ('+', '-', '±'),
    ('1', '4', '¼'),
    ('1', '2', '½'),
    ('3', '4', '¾'),
    ('1', 'S', '¹'),
    ('2', 'S', '²'),
    ('3', 'S', '³'),
    // ── Arrows ────────────────────────────────────────────────────────
    ('-', '>', '→'),
    ('<', '-', '←'),
    ('-', '!', '↑'),
    ('-', 'v', '↓'),
    ('<', '>', '↔'),
    ('U', 'D', '↕'),
    // ── Math ──────────────────────────────────────────────────────────
    ('R', 'T', '√'),
    ('0', '0', '∞'),
    ('=', '3', '≡'),
    ('!', '=', '≠'),
    ('=', '<', '≤'),
    ('>', '=', '≥'),
    ('?', '2', '≈'),
    // ── Greek (lowercase) ─────────────────────────────────────────────
    ('a', '*', 'α'),
    ('b', '*', 'β'),
    ('g', '*', 'γ'),
    ('d', '*', 'δ'),
    ('e', '*', 'ε'),
    ('z', '*', 'ζ'),
    ('y', '*', 'η'),
    ('h', '*', 'θ'),
    ('i', '*', 'ι'),
    ('k', '*', 'κ'),
    ('l', '*', 'λ'),
    ('m', '*', 'μ'),
    ('n', '*', 'ν'),
    ('c', '*', 'ξ'),
    ('o', '*', 'ο'),
    ('p', '*', 'π'),
    ('r', '*', 'ρ'),
    ('s', '*', 'σ'),
    ('t', '*', 'τ'),
    ('u', '*', 'υ'),
    ('f', '*', 'φ'),
    ('x', '*', 'χ'),
    ('q', '*', 'ψ'),
    ('w', '*', 'ω'),
    // ── Greek (uppercase) ─────────────────────────────────────────────
    ('A', '*', 'Α'),
    ('B', '*', 'Β'),
    ('G', '*', 'Γ'),
    ('D', '*', 'Δ'),
    ('E', '*', 'Ε'),
    ('Z', '*', 'Ζ'),
    ('Y', '*', 'Η'),
    ('H', '*', 'Θ'),
    ('I', '*', 'Ι'),
    ('K', '*', 'Κ'),
    ('L', '*', 'Λ'),
    ('M', '*', 'Μ'),
    ('N', '*', 'Ν'),
    ('C', '*', 'Ξ'),
    ('O', '*', 'Ο'),
    ('P', '*', 'Π'),
    ('R', '*', 'Ρ'),
    ('S', '*', 'Σ'),
    ('T', '*', 'Τ'),
    ('U', '*', 'Υ'),
    ('F', '*', 'Φ'),
    ('X', '*', 'Χ'),
    ('Q', '*', 'Ψ'),
    ('W', '*', 'Ω'),
];

/// Look up a digraph by the two characters typed, trying `(c1, c2)` and then
/// the reversed order. `extra` (from `Engine::custom_digraphs`, `:digraph`
/// user-defined entries) is consulted first so a user override wins.
pub fn lookup(
    c1: char,
    c2: char,
    extra: &std::collections::HashMap<(char, char), char>,
) -> Option<char> {
    if let Some(&ch) = extra.get(&(c1, c2)) {
        return Some(ch);
    }
    if let Some(&ch) = extra.get(&(c2, c1)) {
        return Some(ch);
    }
    for &(a, b, r) in BUILTIN_DIGRAPHS {
        if a == c1 && b == c2 {
            return Some(r);
        }
    }
    for &(a, b, r) in BUILTIN_DIGRAPHS {
        if a == c2 && b == c1 {
            return Some(r);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_common_digraphs() {
        let empty = std::collections::HashMap::new();
        assert_eq!(lookup('a', ':', &empty), Some('ä'));
        assert_eq!(lookup('-', '>', &empty), Some('→'));
        assert_eq!(lookup('s', 's', &empty), Some('ß'));
    }

    #[test]
    fn falls_back_to_reversed_order() {
        let empty = std::collections::HashMap::new();
        // ":a" isn't a table entry directly, but "a:" is.
        assert_eq!(lookup(':', 'a', &empty), Some('ä'));
    }

    #[test]
    fn custom_digraph_overrides_and_is_checked_first() {
        let mut extra = std::collections::HashMap::new();
        extra.insert(('z', 'z'), '★');
        assert_eq!(lookup('z', 'z', &extra), Some('★'));
        // A custom entry for a pair that also exists built-in wins.
        extra.insert(('a', ':'), '#');
        assert_eq!(lookup('a', ':', &extra), Some('#'));
    }

    #[test]
    fn unknown_pair_returns_none() {
        let empty = std::collections::HashMap::new();
        assert_eq!(lookup('q', 'q', &empty), None);
    }

    #[test]
    fn no_duplicate_keys_in_builtin_table() {
        let mut seen = std::collections::HashSet::new();
        for &(a, b, _) in BUILTIN_DIGRAPHS {
            assert!(seen.insert((a, b)), "duplicate digraph key {a}{b}");
        }
    }
}
