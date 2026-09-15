//! Vim pattern → Rust `regex` translation (#801).
//!
//! Vim's regular expressions are *not* PCRE and are *not* Rust `regex` syntax:
//! quantifiers are backslash-escaped in the default `magic` mode (`\+`, `\{n,m}`),
//! word boundaries are `\<` / `\>`, the match span can be trimmed with `\zs` /
//! `\ze`, and four different "magic" levels change which punctuation is special.
//!
//! This module is the single translation point.  It is deliberately free of any
//! `Engine` knowledge so it can be unit-tested in isolation — see the tests at
//! the bottom, one per Vim atom.
//!
//! ## `\zs` / `\ze`
//!
//! Rust's `regex` has no look-around, so a trimmed match span is expressed by
//! wrapping the *kept* part in an injected capture group:
//!
//! ```text
//! foo\zsbar    →  (?:foo)(bar)       span = group N
//! foo\zebar    →  (foo)(?:bar)       span = group N
//! a\zsb\zec    →  a(b)c              span = group N
//! ```
//!
//! Injecting a group renumbers the user's own `\(` groups, so
//! [`Translation::group_map`] records the Vim-group → Rust-group mapping that
//! replacement expansion must apply to `\1` … `\9`.
//!
//! ## Rejection, never fallback
//!
//! A pattern that cannot be translated returns `Err` with a Vim-style message.
//! Callers must surface it — falling back to literal matching is exactly the
//! silent-wrong-answer failure mode #801 exists to remove.

/// Vim's four "magic" levels (`:h /magic`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Magic {
    /// `\V` — only `\` is special.
    VeryNoMagic,
    /// `\M` — `^` and `$` are special, `.` and `*` are not.
    NoMagic,
    /// `\m` — Vim's default: `. * [] ^ $ ~` special, quantifiers backslashed.
    Magic,
    /// `\v` — "very magic": all ASCII punctuation except `_` is special.
    VeryMagic,
}

/// Inline case override requested by `\c` / `\C` anywhere in the pattern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseOverride {
    /// `\c` — ignore case regardless of `'ignorecase'`.
    Ignore,
    /// `\C` — match case regardless of `'ignorecase'`.
    Match,
}

/// The result of translating one Vim pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translation {
    /// Rust `regex` source, without any inline flags.
    pub regex: String,
    /// `\c` / `\C` seen in the pattern, if any.
    pub case_override: Option<CaseOverride>,
    /// Capture-group index holding the reported match span when `\zs` / `\ze`
    /// trimmed it, or `None` when the whole match is the span.
    pub span_group: Option<usize>,
    /// `group_map[n]` is the Rust group number for the Vim group `\n`
    /// (index 0 unused). Only differs from identity when `\zs` / `\ze` injected
    /// a group ahead of a user group.
    pub group_map: Vec<usize>,
}

impl Translation {
    /// Total number of capture groups in the translated regex.
    pub fn group_count(&self) -> usize {
        self.group_map.len().saturating_sub(1) + usize::from(self.span_group.is_some())
    }
}

/// Does `pat` contain an uppercase character that should defeat `'smartcase'`?
///
/// Mirrors Vim's `pat_has_uppercase()`: a backslash escape and the character it
/// escapes are skipped, so `/\Sfoo` is *not* "has uppercase".
pub fn pat_has_uppercase(pat: &str) -> bool {
    let chars: Vec<char> = pat.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            match chars.get(i + 1) {
                Some('_') | Some('%') if i + 2 < chars.len() => i += 3,
                Some(_) => i += 2,
                None => i += 1,
            }
        } else if chars[i].is_uppercase() {
            return true;
        } else {
            i += 1;
        }
    }
    false
}

/// Escape `s` so it matches literally inside a Rust regex.
fn push_literal(out: &mut String, c: char) {
    if "\\.+*?()|[]{}^$#&~-".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

/// Escape a whole string as a literal Rust-regex fragment.
pub fn escape_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        push_literal(&mut out, c);
    }
    out
}

/// Escape a string so it matches literally when re-parsed as a **Vim** pattern
/// in `magic` mode. Used by `*` / `#`, which wrap the word under the cursor in
/// `\<` … `\>`.
pub fn escape_vim_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\/.*$^~[]".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Character-class expansions for Vim's single-letter classes.
fn class_for(c: char) -> Option<&'static str> {
    Some(match c {
        's' => "[ \\t]",
        'S' => "[^ \\t]",
        'd' => "[0-9]",
        'D' => "[^0-9]",
        'w' => "[0-9A-Za-z_]",
        'W' => "[^0-9A-Za-z_]",
        'a' => "[A-Za-z]",
        'A' => "[^A-Za-z]",
        'l' => "[a-z]",
        'L' => "[^a-z]",
        'u' => "[A-Z]",
        'U' => "[^A-Z]",
        'x' => "[0-9A-Fa-f]",
        'X' => "[^0-9A-Fa-f]",
        'o' => "[0-7]",
        'O' => "[^0-7]",
        'h' => "[A-Za-z_]",
        'H' => "[^A-Za-z_]",
        'i' => "[0-9A-Za-z_]",
        'I' => "[A-Za-z_]",
        'k' => "[0-9A-Za-z_]",
        'K' => "[A-Za-z_]",
        'f' => "[^ \\t]",
        'F' => "[^ \\t0-9]",
        'p' => "[ -~]",
        'P' => "[ -~&&[^0-9]]",
        _ => return None,
    })
}

struct Translator<'a> {
    chars: Vec<char>,
    i: usize,
    magic: Magic,
    out: String,
    /// Output byte offset at which each Vim capture group opened.
    group_offsets: Vec<usize>,
    zs: Option<usize>,
    ze: Option<usize>,
    case_override: Option<CaseOverride>,
    /// `~` expands to this (already Vim-escaped) previous substitute string.
    last_sub: &'a str,
    /// True right after `\(`, `\|`, or at pattern start — where `^` is an anchor.
    at_start: bool,
}

impl<'a> Translator<'a> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }

    /// Is `c` special *without* a backslash at the current magic level?
    fn bare_special(&self, c: char) -> bool {
        match self.magic {
            Magic::VeryMagic => !(c.is_alphanumeric() || c == '_'),
            Magic::Magic => matches!(c, '.' | '*' | '[' | ']' | '^' | '$' | '~' | '\\'),
            Magic::NoMagic => matches!(c, '^' | '$' | '\\'),
            Magic::VeryNoMagic => c == '\\',
        }
    }

    /// Consume a `{...}` / `\{...}` multi and emit the Rust equivalent.
    ///
    /// Vim accepts `\{n,m}`, `\{n,m\}`, `\{-}` (non-greedy `*`), `\{-n,m}` and
    /// the degenerate `\{}` (= `*`).
    fn multi(&mut self) -> Result<(), String> {
        let mut body = String::new();
        loop {
            match self.bump() {
                Some('}') => break,
                Some('\\') => match self.bump() {
                    Some('}') => break,
                    Some(c) => body.push(c),
                    None => return Err("E554: Syntax error in \\{...}".to_string()),
                },
                Some(c) => body.push(c),
                None => return Err("E554: Syntax error in \\{...}".to_string()),
            }
        }
        let (lazy, body) = match body.strip_prefix('-') {
            Some(rest) => (true, rest.to_string()),
            None => (false, body),
        };
        if body.is_empty() {
            self.out.push('*');
        } else {
            if !body.chars().all(|c| c.is_ascii_digit() || c == ',') {
                return Err("E554: Syntax error in \\{...}".to_string());
            }
            self.out.push('{');
            self.out.push_str(&body);
            self.out.push('}');
        }
        if lazy {
            self.out.push('?');
        }
        Ok(())
    }

    /// Copy a `[...]` collection through, translating Vim quirks.
    fn collection(&mut self) -> Result<(), String> {
        // `[` with no closing `]` is a literal `[` in Vim.
        let close = {
            let mut j = self.i;
            if self.chars.get(j) == Some(&'^') {
                j += 1;
            }
            if self.chars.get(j) == Some(&']') {
                j += 1;
            }
            let mut found = None;
            while j < self.chars.len() {
                if self.chars[j] == '\\' {
                    j += 2;
                    continue;
                }
                if self.chars[j] == '[' && self.chars.get(j + 1) == Some(&':') {
                    // POSIX class — skip to `:]`
                    j += 2;
                    while j < self.chars.len() && self.chars[j] != ':' {
                        j += 1;
                    }
                    j += 2;
                    continue;
                }
                if self.chars[j] == ']' {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            found
        };
        let Some(close) = close else {
            self.out.push_str("\\[");
            return Ok(());
        };

        self.out.push('[');
        if self.peek() == Some('^') {
            self.out.push('^');
            self.i += 1;
        }
        if self.peek() == Some(']') {
            // Vim's `[]abc]` — a leading `]` is literal.
            self.out.push_str("\\]");
            self.i += 1;
        }
        while self.i < close {
            let c = self.chars[self.i];
            self.i += 1;
            match c {
                '\\' => {
                    let n = self.bump().unwrap_or('\\');
                    match n {
                        'n' => self.out.push_str("\\n"),
                        't' => self.out.push_str("\\t"),
                        'r' => self.out.push_str("\\r"),
                        'e' => self.out.push_str("\\x1b"),
                        '\\' => self.out.push_str("\\\\"),
                        ']' => self.out.push_str("\\]"),
                        '^' => self.out.push_str("\\^"),
                        '-' => self.out.push_str("\\-"),
                        other => {
                            if "[](){}.*+?|$&~#".contains(other) {
                                self.out.push('\\');
                            }
                            self.out.push(other);
                        }
                    }
                }
                '[' if self.chars.get(self.i) == Some(&':') => {
                    // POSIX class: copy verbatim, Rust understands these.
                    self.out.push_str("[:");
                    self.i += 1;
                    while self.i < self.chars.len() && self.chars[self.i] != ':' {
                        self.out.push(self.chars[self.i]);
                        self.i += 1;
                    }
                    self.out.push_str(":]");
                    self.i += 2;
                }
                '[' => self.out.push_str("\\["),
                '&' => self.out.push_str("\\&"),
                '~' => self.out.push_str("\\~"),
                '#' => self.out.push_str("\\#"),
                other => self.out.push(other),
            }
        }
        self.out.push(']');
        self.i = close + 1;
        Ok(())
    }

    fn open_group(&mut self) {
        self.group_offsets.push(self.out.len());
        self.out.push('(');
    }

    /// Handle an atom whose Vim meaning is "special", regardless of whether it
    /// arrived bare (very-magic) or backslashed (magic).
    fn special(&mut self, c: char) -> Result<bool, String> {
        match c {
            '(' => {
                self.open_group();
                self.at_start = true;
                return Ok(true);
            }
            ')' => self.out.push(')'),
            '|' => {
                self.out.push('|');
                self.at_start = true;
                return Ok(true);
            }
            '+' => self.out.push('+'),
            '?' | '=' => self.out.push('?'),
            '{' => self.multi()?,
            '@' => {
                // \v...@= / @! / @<= / @<! — look-around, unsupported by `regex`.
                return Err(
                    "E-vimcode: look-around (\\@=, \\@!) is not supported by this regex engine"
                        .to_string(),
                );
            }
            '<' => self.out.push_str("\\b"),
            '>' => self.out.push_str("\\b"),
            '%' => {
                match self.bump() {
                    Some('(') => self.out.push_str("(?:"),
                    Some('^') => self.out.push_str("\\A"),
                    Some('$') => self.out.push_str("\\z"),
                    Some(other) => {
                        return Err(format!(
                            "E-vimcode: \\%{other} is not supported by this regex engine"
                        ))
                    }
                    None => return Err("E682: Invalid search pattern".to_string()),
                }
                self.at_start = true;
                return Ok(true);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn run(&mut self) -> Result<(), String> {
        while let Some(c) = self.bump() {
            let was_start = self.at_start;
            self.at_start = false;
            if c == '\\' {
                let Some(n) = self.bump() else {
                    return Err("E682: Invalid search pattern".to_string());
                };
                match n {
                    'v' => {
                        self.magic = Magic::VeryMagic;
                        self.at_start = was_start;
                    }
                    'V' => {
                        self.magic = Magic::VeryNoMagic;
                        self.at_start = was_start;
                    }
                    'm' => {
                        self.magic = Magic::Magic;
                        self.at_start = was_start;
                    }
                    'M' => {
                        self.magic = Magic::NoMagic;
                        self.at_start = was_start;
                    }
                    'c' => {
                        self.case_override = Some(CaseOverride::Ignore);
                        self.at_start = was_start;
                    }
                    'C' => {
                        self.case_override = Some(CaseOverride::Match);
                        self.at_start = was_start;
                    }
                    'z' => match self.bump() {
                        Some('s') => {
                            self.zs = Some(self.out.len());
                            self.at_start = was_start;
                        }
                        Some('e') => {
                            self.ze = Some(self.out.len());
                            self.at_start = was_start;
                        }
                        _ => return Err("E68: Invalid character after \\z".to_string()),
                    },
                    'n' => self.out.push_str("\\n"),
                    't' => self.out.push_str("\\t"),
                    'r' => self.out.push_str("\\r"),
                    'e' => self.out.push_str("\\x1b"),
                    '1'..='9' => {
                        return Err(format!(
                            "E-vimcode: back-reference \\{n} in a pattern is not supported by \
                             this regex engine"
                        ))
                    }
                    '&' => {
                        return Err(
                            "E-vimcode: \\& (branch concat) is not supported by this regex engine"
                                .to_string(),
                        )
                    }
                    '_' => {
                        return Err(
                            "E-vimcode: \\_ (match across lines) is not supported by this regex \
                             engine"
                                .to_string(),
                        )
                    }
                    _ => {
                        if let Some(cls) = class_for(n) {
                            self.out.push_str(cls);
                        } else if self.magic == Magic::VeryMagic {
                            // In very-magic a backslash always makes the next
                            // character literal.
                            push_literal(&mut self.out, n);
                        } else if self.special(n)? {
                            // handled
                        } else {
                            push_literal(&mut self.out, n);
                        }
                    }
                }
                continue;
            }

            if !self.bare_special(c) {
                if self.magic == Magic::VeryMagic && (c.is_alphanumeric() || c == '_') {
                    self.out.push(c);
                } else {
                    push_literal(&mut self.out, c);
                }
                continue;
            }

            match c {
                '^' => {
                    if was_start {
                        self.out.push('^');
                    } else {
                        self.out.push_str("\\^");
                    }
                }
                '$' => {
                    if self.at_dollar_end() {
                        self.out.push('$');
                    } else {
                        self.out.push_str("\\$");
                    }
                }
                '.' => self.out.push('.'),
                '*' => {
                    if self.out.is_empty() || was_start {
                        self.out.push_str("\\*");
                    } else {
                        self.out.push('*');
                    }
                }
                '[' => self.collection()?,
                ']' => self.out.push_str("\\]"),
                '~' => {
                    let sub = self.last_sub.to_string();
                    self.out.push_str(&escape_literal(&sub));
                }
                _ => {
                    if self.magic == Magic::VeryMagic {
                        if !self.special(c)? {
                            push_literal(&mut self.out, c);
                        }
                    } else {
                        push_literal(&mut self.out, c);
                    }
                }
            }
        }
        Ok(())
    }

    /// `$` is an anchor only at the very end of the pattern or immediately
    /// before `\|` / `\)` (or their very-magic bare forms).
    fn at_dollar_end(&self) -> bool {
        match self.chars.get(self.i) {
            None => true,
            Some('\\') => matches!(self.chars.get(self.i + 1), Some('|') | Some(')')),
            Some('|') | Some(')') => self.magic == Magic::VeryMagic,
            _ => false,
        }
    }
}

/// Translate a Vim pattern into Rust `regex` source.
///
/// `last_sub` is the previous `:s` replacement text, which `~` expands to.
pub fn translate(pattern: &str, magic: Magic, last_sub: &str) -> Result<Translation, String> {
    let mut t = Translator {
        chars: pattern.chars().collect(),
        i: 0,
        magic,
        out: String::new(),
        group_offsets: Vec::new(),
        zs: None,
        ze: None,
        case_override: None,
        last_sub,
        at_start: true,
    };
    t.run()?;

    let Translator {
        mut out,
        group_offsets,
        zs,
        ze,
        case_override,
        ..
    } = t;

    let mut span_group = None;
    let mut group_map: Vec<usize> = (0..=group_offsets.len()).collect();

    if zs.is_some() || ze.is_some() {
        let open_at = zs.unwrap_or(0);
        let close_at = ze.unwrap_or(out.len());
        if close_at < open_at {
            return Err("E-vimcode: \\ze appears before \\zs".to_string());
        }
        // Insert the closing paren first so the opening insert offset stays valid.
        out.insert(close_at, ')');
        out.insert(open_at, '(');
        // The injected group's number is one more than the count of user groups
        // that open before it.
        let before = group_offsets.iter().filter(|&&o| o < open_at).count();
        span_group = Some(before + 1);
        group_map = (0..=group_offsets.len())
            .map(|n| if n > before { n + 1 } else { n })
            .collect();
    }

    Ok(Translation {
        regex: out,
        case_override,
        span_group,
        group_map,
    })
}

/// Translate and compile in one step, applying `'ignorecase'` / `'smartcase'`.
///
/// * `ignorecase` / `smartcase` are the option values;
/// * `smartcase_applies` is false for `*`, `#` and `gd`, which per
///   `:h 'smartcase'` never consult the option;
/// * a `\c` / `\C` in the pattern overrides both.
#[derive(Debug)]
pub struct Compiled {
    pub regex: regex::Regex,
    pub span_group: Option<usize>,
    pub group_map: Vec<usize>,
}

impl Compiled {
    /// Byte span the match *reports*, honouring `\zs` / `\ze`.
    pub fn span(&self, caps: &regex::Captures) -> (usize, usize) {
        match self.span_group.and_then(|g| caps.get(g)) {
            Some(m) => (m.start(), m.end()),
            None => {
                let m = caps.get(0).expect("group 0 always matches");
                (m.start(), m.end())
            }
        }
    }
}

pub fn compile(
    pattern: &str,
    ignorecase: bool,
    smartcase: bool,
    smartcase_applies: bool,
    last_sub: &str,
) -> Result<Compiled, String> {
    if pattern.is_empty() {
        return Err("E35: No previous regular expression".to_string());
    }
    let t = translate(pattern, Magic::Magic, last_sub)?;
    let case_insensitive = match t.case_override {
        Some(CaseOverride::Ignore) => true,
        Some(CaseOverride::Match) => false,
        None => ignorecase && !(smartcase && smartcase_applies && pat_has_uppercase(pattern)),
    };
    let src = format!("(?m{}){}", if case_insensitive { "i" } else { "" }, t.regex);
    match regex::Regex::new(&src) {
        Ok(regex) => Ok(Compiled {
            regex,
            span_group: t.span_group,
            group_map: t.group_map,
        }),
        Err(e) => Err(format!("E383: Invalid pattern: {pattern} ({e})")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(p: &str) -> String {
        translate(p, Magic::Magic, "").expect("translates").regex
    }

    #[test]
    fn plain_text_is_literal() {
        assert_eq!(tr("foo"), "foo");
    }

    #[test]
    fn caret_anchors_only_at_start() {
        assert_eq!(tr("^foo"), "^foo");
        assert_eq!(tr("a^b"), "a\\^b");
    }

    #[test]
    fn dollar_anchors_only_at_end() {
        assert_eq!(tr("foo$"), "foo$");
        assert_eq!(tr("a$b"), "a\\$b");
        assert_eq!(tr("a$\\|b"), "a$|b");
    }

    #[test]
    fn word_boundaries() {
        assert_eq!(tr("\\<foo\\>"), "\\bfoo\\b");
    }

    #[test]
    fn magic_dot_and_star() {
        assert_eq!(tr("a.c"), "a.c");
        assert_eq!(tr("ab*c"), "ab*c");
        assert_eq!(tr("a\\.c"), "a\\.c");
    }

    #[test]
    fn escaped_quantifiers() {
        assert_eq!(tr("a\\+"), "a+");
        assert_eq!(tr("a\\?"), "a?");
        assert_eq!(tr("a\\="), "a?");
        // Bare `+` is a literal in magic mode.
        assert_eq!(tr("a+"), "a\\+");
    }

    #[test]
    fn braces_and_non_greedy() {
        assert_eq!(tr("a\\{2}"), "a{2}");
        assert_eq!(tr("a\\{2,3}"), "a{2,3}");
        assert_eq!(tr("a\\{2,3\\}"), "a{2,3}");
        assert_eq!(tr("a\\{-}"), "a*?");
        assert_eq!(tr("a\\{-1,}"), "a{1,}?");
        assert_eq!(tr("a\\{}"), "a*");
    }

    #[test]
    fn groups_and_alternation() {
        assert_eq!(tr("\\(foo\\)"), "(foo)");
        assert_eq!(tr("foo\\|bar"), "foo|bar");
        assert_eq!(tr("\\%(foo\\)"), "(?:foo)");
        // Bare parens/pipe are literals in magic mode.
        assert_eq!(tr("(a)"), "\\(a\\)");
        assert_eq!(tr("a|b"), "a\\|b");
    }

    #[test]
    fn very_magic() {
        assert_eq!(tr("\\v(a|b)+"), "(a|b)+");
        assert_eq!(tr("\\vo+"), "o+");
        assert_eq!(tr("\\v\\(a\\)"), "\\(a\\)");
        assert_eq!(tr("\\v<foo>"), "\\bfoo\\b");
    }

    #[test]
    fn very_nomagic() {
        assert_eq!(tr("\\Va.c"), "a\\.c");
        assert_eq!(tr("\\Va*"), "a\\*");
    }

    #[test]
    fn nomagic() {
        assert_eq!(tr("\\Ma.c"), "a\\.c");
        assert_eq!(tr("\\M^a$"), "^a$");
    }

    #[test]
    fn character_classes() {
        assert_eq!(tr("\\d\\+"), "[0-9]+");
        assert_eq!(tr("\\w"), "[0-9A-Za-z_]");
        assert_eq!(tr("\\s"), "[ \\t]");
        assert_eq!(tr("\\S"), "[^ \\t]");
    }

    #[test]
    fn collections() {
        assert_eq!(tr("[bc]a"), "[bc]a");
        assert_eq!(tr("[^abc]"), "[^abc]");
        assert_eq!(tr("[]a]"), "[\\]a]");
        assert_eq!(tr("[a-z]"), "[a-z]");
        // Unterminated `[` is a literal bracket.
        assert_eq!(tr("[abc"), "\\[abc");
    }

    #[test]
    fn escapes() {
        assert_eq!(tr("a\\nb"), "a\\nb");
        assert_eq!(tr("a\\tb"), "a\\tb");
    }

    #[test]
    fn case_overrides() {
        let t = translate("\\cfoo", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "foo");
        assert_eq!(t.case_override, Some(CaseOverride::Ignore));
        let t = translate("\\CFOO", Magic::Magic, "").unwrap();
        assert_eq!(t.case_override, Some(CaseOverride::Match));
    }

    #[test]
    fn zs_wraps_the_kept_tail() {
        let t = translate("foo\\zsbar", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "foo(bar)");
        assert_eq!(t.span_group, Some(1));
    }

    #[test]
    fn ze_wraps_the_kept_head() {
        let t = translate("foo\\zebar", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "(foo)bar");
        assert_eq!(t.span_group, Some(1));
    }

    #[test]
    fn zs_and_ze_together() {
        let t = translate("a\\zsb\\zec", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "a(b)c");
        assert_eq!(t.span_group, Some(1));
    }

    #[test]
    fn zs_renumbers_user_groups() {
        let t = translate("\\(a\\)\\zs\\(b\\)", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "(a)((b))");
        assert_eq!(t.span_group, Some(2));
        // Vim group 1 → Rust 1, Vim group 2 → Rust 3.
        assert_eq!(t.group_map, vec![0, 1, 3]);
    }

    #[test]
    fn tilde_expands_to_last_substitute() {
        let t = translate("~x", Magic::Magic, "a.b").unwrap();
        assert_eq!(t.regex, "a\\.bx");
        // Escaped `\~` stays literal.
        assert_eq!(tr("\\~"), "\\~");
    }

    #[test]
    fn backrefs_in_pattern_are_rejected_not_silently_literal() {
        let err = translate("\\(foo\\)\\1", Magic::Magic, "").unwrap_err();
        assert!(err.contains("back-reference"), "{err}");
    }

    #[test]
    fn lookaround_is_rejected() {
        assert!(translate("\\vfoo(bar)@=", Magic::Magic, "").is_err());
    }

    #[test]
    fn buffer_anchors() {
        assert_eq!(tr("\\%^foo"), "\\Afoo");
        assert_eq!(tr("foo\\%$"), "foo\\z");
    }

    #[test]
    fn smartcase_uppercase_detection_skips_escapes() {
        assert!(!pat_has_uppercase("foo"));
        assert!(pat_has_uppercase("Foo"));
        // `\S` is an escape, not an uppercase letter.
        assert!(!pat_has_uppercase("\\Sfoo"));
        assert!(pat_has_uppercase("\\SfoO"));
    }

    #[test]
    fn compile_applies_smartcase_only_when_it_applies() {
        // ignorecase + smartcase, lowercase pattern → insensitive
        let c = compile("foo", true, true, true, "").unwrap();
        assert!(c.regex.is_match("FOO"));
        // uppercase in pattern → sensitive
        let c = compile("Foo", true, true, true, "").unwrap();
        assert!(!c.regex.is_match("FOO"));
        // `*` sets smartcase_applies = false → stays insensitive
        let c = compile("Foo", true, true, false, "").unwrap();
        assert!(c.regex.is_match("FOO"));
    }

    #[test]
    fn compile_rejects_invalid_pattern() {
        let err = compile("a\\{2,1}", false, false, true, "").unwrap_err();
        assert!(err.contains("Invalid pattern"), "{err}");
    }

    #[test]
    fn multiline_flag_is_on_so_anchors_are_per_line() {
        let c = compile("^b", false, false, true, "").unwrap();
        assert!(c.regex.is_match("a\nb"));
    }
}
