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
//! ## Look-around (`\@=` / `\@!` / `\@<=` / `\@<!`, #1157)
//!
//! Unlike `\zs`/`\ze`, real look-around has a direct target in `fancy_regex`
//! (already linked for `\1`..`\9` backreferences, #1004), so it's a syntax
//! rewrite of the atom that was just emitted rather than an engine-level
//! workaround: `\(bar\)\@=` becomes `(?=(bar))` — the inner capturing group
//! is kept so Vim's "groups are numbered by position, look-around or not"
//! rule still holds for `\1`..`\9`.
//!
//! ## Position assertions (`\%23l` / `\%23c` / `\%V`, #1157)
//!
//! Neither `regex` nor `fancy_regex` has an "absolute line/column" or
//! "inside this byte range" primitive, so these translate to no regex text
//! at all — they're recorded as [`PosConstraint`]s and applied by
//! [`Compiled`] as a post-match filter, retrying at the next candidate
//! position when a raw engine match fails one.
//!
//! ## Rejection, never fallback
//!
//! A pattern that cannot be translated returns `Err` with a Vim-style message.
//! Callers must surface it — falling back to literal matching is exactly the
//! silent-wrong-answer failure mode #801 exists to remove. `\&` (branch
//! concat) and `\%[...]` (optional sequence) stay in this category: neither
//! `regex` nor `fancy_regex` exposes a primitive they can be translated to.

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

/// Comparison requested by a `\%Nl` / `\%<Nl` / `\%>Nl` (or `c`) position atom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmp {
    /// `\%23l` — line/col equals exactly.
    Eq,
    /// `\%<23l` — line/col is less than.
    Lt,
    /// `\%>23l` — line/col is greater than.
    Gt,
}

/// A zero-width position assertion that can't be expressed inside the
/// translated regex itself (`regex`/`fancy_regex` have no "current absolute
/// line/column" or "inside this byte range" primitive) — recorded here and
/// applied as a post-match filter by [`Compiled`] instead (#1157).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PosConstraint {
    /// `\%23l` / `\%<23l` / `\%>23l` — 1-indexed line number of the match start.
    Line(Cmp, usize),
    /// `\%23c` / `\%<23c` / `\%>23c` — 1-indexed byte column of the match start.
    Col(Cmp, usize),
    /// `\%V` — the match start must fall inside the last Visual selection,
    /// supplied to [`compile`] as a byte range.
    Visual,
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
    /// `true` if the pattern contains a `\1`..`\9` backreference, which
    /// `regex`'s linear-time automaton cannot execute — such a pattern must
    /// be compiled with `fancy_regex` instead (#1004).
    pub has_backref: bool,
    /// `true` if the pattern contains `\@=` / `\@!` / `\@<=` / `\@<!`
    /// look-around, which also requires `fancy_regex` (#1157).
    pub has_lookaround: bool,
    /// Zero-width position assertions (`\%23l`, `\%23c`, `\%V`) that must be
    /// applied as a post-match filter — see [`PosConstraint`].
    pub pos_constraints: Vec<PosConstraint>,
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
    /// `true` once a `\1`..`\9` backreference atom has been emitted.
    has_backref: bool,
    /// `true` once a `\@=`/`\@!`/`\@<=`/`\@<!` look-around has been emitted.
    has_lookaround: bool,
    /// Output byte offset at which each currently-open bracketed group
    /// (capturing `\(` or non-capturing `\%(`) began — used by `\@=` and
    /// friends to find the atom they apply to when it closes.
    bracket_stack: Vec<usize>,
    /// Output byte offset at which the most recently completed atom began,
    /// so `\@=` / `\@!` / `\@<=` / `\@<!` know what to wrap. `None` right
    /// after something that isn't a repeatable/lookaround-able atom (pattern
    /// start, `\|`, an unclosed group).
    last_atom_start: Option<usize>,
    /// Position assertions collected from `\%23l` / `\%23c` / `\%V`.
    pos_constraints: Vec<PosConstraint>,
}

/// Placeholder marker for a not-yet-resolved backreference: NUL followed by
/// the referenced Vim group's ASCII digit. `\zs`/`\ze` insert `(`/`)` into
/// `out` *after* the whole pattern has been scanned, which can renumber
/// groups that opened after the injection point — so a backreference can't
/// be resolved to its final Rust group number until that renumbering is
/// done. NUL never otherwise appears in translated output (no atom emits
/// it), so it is safe to use as a private marker byte here.
const BACKREF_MARK: char = '\u{0}';

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
        let offset = self.out.len();
        self.group_offsets.push(offset);
        self.bracket_stack.push(offset);
        self.out.push('(');
    }

    /// Shift every recorded offset `>= start` by `delta` bytes — called after
    /// `\@=`/`\@!`/`\@<=`/`\@<!` inserts a prefix ahead of an already-emitted
    /// atom, which pushes everything from `start` onward forward in `out`.
    /// Without this, `\zs`/`\ze`/group-numbering offsets recorded *inside*
    /// the wrapped atom (e.g. `\(foo\zsbar\)\@=`) would point at the wrong
    /// byte once the lookaround prefix is spliced in ahead of them.
    fn shift_offsets_from(&mut self, start: usize, delta: usize) {
        if let Some(z) = self.zs {
            if z >= start {
                self.zs = Some(z + delta);
            }
        }
        if let Some(z) = self.ze {
            if z >= start {
                self.ze = Some(z + delta);
            }
        }
        for o in self.group_offsets.iter_mut() {
            if *o >= start {
                *o += delta;
            }
        }
        for o in self.bracket_stack.iter_mut() {
            if *o >= start {
                *o += delta;
            }
        }
    }

    /// `\%d123` / `\%x2a` — a decimal or hex character code, emitted as the
    /// literal character it names.
    fn push_char_code(&mut self, radix: u32, max_digits: usize) -> Result<(), String> {
        let mut digits = String::new();
        while digits.len() < max_digits {
            match self.peek() {
                Some(c) if c.is_digit(radix) => {
                    digits.push(c);
                    self.i += 1;
                }
                _ => break,
            }
        }
        if digits.is_empty() {
            return Err("E-vimcode: \\%d / \\%x require at least one digit".to_string());
        }
        let code = u32::from_str_radix(&digits, radix)
            .map_err(|_| "E-vimcode: invalid character code".to_string())?;
        let ch =
            char::from_u32(code).ok_or_else(|| "E-vimcode: invalid character code".to_string())?;
        push_literal(&mut self.out, ch);
        Ok(())
    }

    /// `\%23l` / `\%<23l` / `\%>23l` (or `c` for column) — parse the number
    /// and comparator and record a [`PosConstraint`]. `first` is the char
    /// already consumed after `\%` — either the first digit, or `<`/`>`.
    fn pos_constraint(&mut self, first: char) -> Result<(), String> {
        let cmp = match first {
            '<' => Cmp::Lt,
            '>' => Cmp::Gt,
            _ => Cmp::Eq,
        };
        let mut digits = String::new();
        if cmp == Cmp::Eq {
            digits.push(first);
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        if digits.is_empty() {
            return Err("E-vimcode: \\%< / \\%> require a number".to_string());
        }
        let n: usize = digits
            .parse()
            .map_err(|_| "E-vimcode: number too large".to_string())?;
        match self.bump() {
            Some('l') => self.pos_constraints.push(PosConstraint::Line(cmp, n)),
            Some('c') => self.pos_constraints.push(PosConstraint::Col(cmp, n)),
            Some('v') => {
                return Err(
                    "E-vimcode: \\%v (virtual column) is not supported by this regex engine"
                        .to_string(),
                )
            }
            _ => {
                return Err("E-vimcode: expected 'l' or 'c' after \\%<n / \\%>n / \\%n".to_string())
            }
        }
        Ok(())
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
            ')' => {
                self.out.push(')');
                if let Some(offset) = self.bracket_stack.pop() {
                    self.last_atom_start = Some(offset);
                }
            }
            '|' => {
                self.out.push('|');
                self.at_start = true;
                return Ok(true);
            }
            '+' => self.out.push('+'),
            '?' | '=' => self.out.push('?'),
            '{' => self.multi()?,
            '@' => {
                // `\@=` / `\@!` / `\@<=` / `\@<!` — look-around, applied to
                // the atom that was just emitted (almost always a `\(...\)`
                // or `\%(...\)` group, but any single atom qualifies).
                // `regex`'s automaton can't do look-around at all, but
                // `fancy_regex` (already linked for `\1`..`\9`, #1004)
                // supports it natively, so this is a pure syntax rewrite of
                // the atom already sitting in `self.out`.
                let Some(start) = self.last_atom_start.take() else {
                    return Err("E-vimcode: \\@=/\\@!/\\@<=/\\@<! must follow an atom".to_string());
                };
                let prefix =
                    match self.bump() {
                        Some('=') => "(?=",
                        Some('!') => "(?!",
                        Some('<') => match self.bump() {
                            Some('=') => "(?<=",
                            Some('!') => "(?<!",
                            _ => return Err("E59: Invalid character after \\@<".to_string()),
                        },
                        Some('>') => return Err(
                            "E-vimcode: \\@> (atomic group) is not supported by this regex engine"
                                .to_string(),
                        ),
                        _ => return Err("E59: Invalid character after \\@".to_string()),
                    };
                let body = self.out.split_off(start);
                self.out.push_str(prefix);
                self.shift_offsets_from(start, prefix.len());
                self.out.push_str(&body);
                self.out.push(')');
                self.has_lookaround = true;
            }
            '<' => self.out.push_str("\\b"),
            '>' => self.out.push_str("\\b"),
            '%' => {
                match self.bump() {
                    Some('(') => {
                        let offset = self.out.len();
                        self.bracket_stack.push(offset);
                        self.out.push_str("(?:");
                        self.at_start = true;
                    }
                    Some('^') => {
                        self.out.push_str("\\A");
                        self.at_start = true;
                    }
                    Some('$') => {
                        self.out.push_str("\\z");
                        self.at_start = true;
                    }
                    Some('V') => {
                        self.pos_constraints.push(PosConstraint::Visual);
                        self.at_start = true;
                    }
                    Some('[') => {
                        return Err(
                            "E-vimcode: \\%[...] (optional sequence) is not supported by this \
                             regex engine — no backtracking-optional-sequence primitive exists \
                             in `regex`/`fancy_regex` (tracked in #1157)"
                                .to_string(),
                        )
                    }
                    Some('d') => self.push_char_code(10, usize::MAX)?,
                    Some('x') => self.push_char_code(16, 2)?,
                    Some(c @ ('<' | '>' | '0'..='9')) => {
                        self.pos_constraint(c)?;
                        self.at_start = true;
                    }
                    Some(other) => {
                        return Err(format!(
                            "E-vimcode: \\%{other} is not supported by this regex engine"
                        ))
                    }
                    None => return Err("E682: Invalid search pattern".to_string()),
                }
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
            // Byte offset in `out` before this iteration emits anything —
            // whatever gets appended between here and the end of the loop
            // body is this iteration's atom, the target `\@=`/`\@!` (etc.)
            // would wrap if it appears right after. `special()` manages
            // `last_atom_start` itself for `(` / `)` / `\@=` (a bracket can
            // span many iterations, and `\@=` consumes the marker outright);
            // `last_atom_before` lets the generic tracking below detect that
            // and skip clobbering it.
            let atom_start = self.out.len();
            let last_atom_before = self.last_atom_start;
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
                        // The final Rust group number isn't known until the
                        // whole pattern has been scanned (`\zs`/`\ze` can
                        // still renumber groups that open later) — emit a
                        // placeholder and resolve it in `translate()`.
                        self.has_backref = true;
                        self.out.push(BACKREF_MARK);
                        self.out.push(n);
                    }
                    '&' => {
                        return Err(
                            "E-vimcode: \\& (branch concat) is not supported by this regex \
                             engine — no bounded-backtracking primitive for \"match this \
                             branch, but report the last one\" exists in `regex`/`fancy_regex` \
                             (tracked in #1157)"
                                .to_string(),
                        )
                    }
                    '_' => match self.bump() {
                        // `\_x` — like the plain atom `x`, but end-of-line
                        // (the `\n` embedded in the whole-buffer match text)
                        // is also accepted, so the atom can span lines.
                        Some('.') => self.out.push_str("(?:.|\\n)"),
                        Some('^') => self.out.push('^'),
                        Some('$') => self.out.push('$'),
                        Some('[') => {
                            let mark = self.out.len();
                            self.collection()?;
                            let cls = self.out.split_off(mark);
                            self.out.push_str("(?:");
                            self.out.push_str(&cls);
                            self.out.push_str("|\\n)");
                        }
                        Some(x) if class_for(x).is_some() => {
                            let cls = class_for(x).expect("checked Some above");
                            self.out.push_str("(?:");
                            self.out.push_str(cls);
                            self.out.push_str("|\\n)");
                        }
                        Some(other) => {
                            return Err(format!(
                                "E-vimcode: \\_{other} is not supported by this regex engine"
                            ))
                        }
                        None => return Err("E682: Invalid search pattern".to_string()),
                    },
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
                if self.last_atom_start == last_atom_before && self.out.len() > atom_start {
                    self.last_atom_start = Some(atom_start);
                }
                continue;
            }

            if !self.bare_special(c) {
                if self.magic == Magic::VeryMagic && (c.is_alphanumeric() || c == '_') {
                    self.out.push(c);
                } else {
                    push_literal(&mut self.out, c);
                }
                self.last_atom_start = Some(atom_start);
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
            if self.last_atom_start == last_atom_before && self.out.len() > atom_start {
                self.last_atom_start = Some(atom_start);
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
        has_backref: false,
        has_lookaround: false,
        bracket_stack: Vec::new(),
        last_atom_start: None,
        pos_constraints: Vec::new(),
    };
    t.run()?;

    let Translator {
        mut out,
        group_offsets,
        zs,
        ze,
        case_override,
        has_backref,
        has_lookaround,
        pos_constraints,
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

    if has_backref {
        out = resolve_backref_marks(&out, &group_map);
    }

    Ok(Translation {
        regex: out,
        case_override,
        span_group,
        group_map,
        has_backref,
        has_lookaround,
        pos_constraints,
    })
}

/// Replace each deferred `BACKREF_MARK<digit>` placeholder with the Rust
/// backreference syntax `\N`, where `N` is the Vim group's final Rust group
/// number — resolved only now that `\zs`/`\ze` renumbering (if any) is known.
///
/// A digit outside `group_map`'s range (referencing a group the pattern
/// never defines) is passed through unmapped; `fancy_regex::Regex::new`
/// rejects that as `Err(InvalidBackref)` rather than panicking, which is the
/// desired "fail, don't crash" behaviour for `\0` and undefined groups
/// (#1004).
fn resolve_backref_marks(src: &str, group_map: &[usize]) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars();
    while let Some(c) = chars.next() {
        if c == BACKREF_MARK {
            let Some(d) = chars.next() else {
                break;
            };
            let vim_n = d.to_digit(10).expect("marker always followed by a digit") as usize;
            let rust_n = group_map.get(vim_n).copied().unwrap_or(vim_n);
            out.push('\\');
            out.push_str(&rust_n.to_string());
        } else {
            out.push(c);
        }
    }
    out
}

/// A single match, uniform across [`CompiledRegex::Fast`]'s `regex::Match`
/// and [`CompiledRegex::Backref`]'s `fancy_regex::Match` — same shape, two
/// crates.
#[derive(Clone, Copy)]
pub struct CapMatch<'t> {
    start: usize,
    end: usize,
    text: &'t str,
}

impl<'t> CapMatch<'t> {
    pub fn start(&self) -> usize {
        self.start
    }
    pub fn end(&self) -> usize {
        self.end
    }
    pub fn as_str(&self) -> &'t str {
        self.text
    }
}

/// One match's capture groups, uniform across the fast/backref engines.
pub enum Captures<'t> {
    Fast(regex::Captures<'t>),
    Backref(fancy_regex::Captures<'t>),
}

impl<'t> Captures<'t> {
    pub fn get(&self, i: usize) -> Option<CapMatch<'t>> {
        match self {
            Captures::Fast(c) => c.get(i).map(|m| CapMatch {
                start: m.start(),
                end: m.end(),
                text: m.as_str(),
            }),
            Captures::Backref(c) => c.get(i).map(|m| CapMatch {
                start: m.start(),
                end: m.end(),
                text: m.as_str(),
            }),
        }
    }
}

/// The compiled regex backing a [`Compiled`] pattern.
///
/// `regex`'s automaton is provably linear-time, which is a permanent design
/// constraint that rules out backreferences entirely — not a missing
/// feature to work around. `fancy_regex` layers a backtracking VM on top of
/// `regex` and is used *only* when the pattern actually contains `\1`..`\9`,
/// so the common (and hot) case keeps `regex`'s speed and guarantees (#1004).
#[derive(Debug)]
pub enum CompiledRegex {
    Fast(regex::Regex),
    Backref(fancy_regex::Regex),
}

/// Does `actual` satisfy `cmp` against `want`?
fn cmp_ok(cmp: Cmp, actual: usize, want: usize) -> bool {
    match cmp {
        Cmp::Eq => actual == want,
        Cmp::Lt => actual < want,
        Cmp::Gt => actual > want,
    }
}

/// 1-indexed (line, byte-column) of `byte_pos` within `text`, counting `\n`
/// bytes seen so far — `text` is always the whole-buffer string `vim_regex`
/// matches against (see module docs), so this needs no `Rope`/engine
/// coupling, just a linear scan.
fn line_col_at(text: &str, byte_pos: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut last_nl = None;
    for (i, b) in text.as_bytes()[..byte_pos.min(text.len())]
        .iter()
        .enumerate()
    {
        if *b == b'\n' {
            line += 1;
            last_nl = Some(i);
        }
    }
    let col = match last_nl {
        Some(nl) => byte_pos - nl,
        None => byte_pos + 1,
    };
    (line, col)
}

/// Translate and compile in one step, applying `'ignorecase'` / `'smartcase'`.
///
/// * `ignorecase` / `smartcase` are the option values;
/// * `smartcase_applies` is false for `*`, `#` and `gd`, which per
///   `:h 'smartcase'` never consult the option;
/// * a `\c` / `\C` in the pattern overrides both;
/// * `visual_range` is the last Visual selection's byte range within the
///   text this will be matched against, used only if the pattern contains
///   `\%V`.
#[derive(Debug)]
pub struct Compiled {
    pub regex: CompiledRegex,
    pub span_group: Option<usize>,
    pub group_map: Vec<usize>,
    pos_constraints: Vec<PosConstraint>,
    visual_range: Option<(usize, usize)>,
}

impl Compiled {
    /// Byte span the match *reports*, honouring `\zs` / `\ze`.
    pub fn span(&self, caps: &Captures) -> (usize, usize) {
        match self.span_group.and_then(|g| caps.get(g)) {
            Some(m) => (m.start(), m.end()),
            None => {
                let m = caps.get(0).expect("group 0 always matches");
                (m.start(), m.end())
            }
        }
    }

    /// Does the match starting at byte `start` satisfy every `\%23l` /
    /// `\%23c` / `\%V` constraint the pattern carries?
    fn pos_ok(&self, text: &str, start: usize) -> bool {
        if self.pos_constraints.is_empty() {
            return true;
        }
        let mut line_col: Option<(usize, usize)> = None;
        for constraint in &self.pos_constraints {
            match constraint {
                PosConstraint::Line(cmp, n) => {
                    let (line, _) = *line_col.get_or_insert_with(|| line_col_at(text, start));
                    if !cmp_ok(*cmp, line, *n) {
                        return false;
                    }
                }
                PosConstraint::Col(cmp, n) => {
                    let (_, col) = *line_col.get_or_insert_with(|| line_col_at(text, start));
                    if !cmp_ok(*cmp, col, *n) {
                        return false;
                    }
                }
                PosConstraint::Visual => match self.visual_range {
                    Some((lo, hi)) if start >= lo && start < hi => {}
                    _ => return false,
                },
            }
        }
        true
    }

    /// First raw engine match at-or-after byte offset `at`, with no
    /// constraint filtering — the pre-#1157 behaviour of `captures_at`.
    fn raw_captures_at<'t>(&self, text: &'t str, at: usize) -> Option<Captures<'t>> {
        match &self.regex {
            CompiledRegex::Fast(re) => re.captures_at(text, at).map(Captures::Fast),
            CompiledRegex::Backref(re) => re
                .captures_from_pos(text, at)
                .ok()
                .flatten()
                .map(Captures::Backref),
        }
    }

    pub fn is_match(&self, text: &str) -> bool {
        if self.pos_constraints.is_empty() {
            return match &self.regex {
                CompiledRegex::Fast(re) => re.is_match(text),
                // A catastrophic-backtrack pattern hits fancy_regex's backtrack
                // limit and returns `Err`; treat that as "no match" rather than
                // propagating a panic (#1004).
                CompiledRegex::Backref(re) => re.is_match(text).unwrap_or(false),
            };
        }
        self.captures_at(text, 0).is_some()
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<Captures<'t>> {
        if self.pos_constraints.is_empty() {
            return match &self.regex {
                CompiledRegex::Fast(re) => re.captures(text).map(Captures::Fast),
                CompiledRegex::Backref(re) => {
                    re.captures(text).ok().flatten().map(Captures::Backref)
                }
            };
        }
        self.captures_at(text, 0)
    }

    /// Captures for the first match starting at-or-after byte offset `at`
    /// that also satisfies every `\%23l`/`\%23c`/`\%V` constraint — retries
    /// at the next candidate start when a raw engine match fails one.
    pub fn captures_at<'t>(&self, text: &'t str, at: usize) -> Option<Captures<'t>> {
        if self.pos_constraints.is_empty() {
            return self.raw_captures_at(text, at);
        }
        let mut pos = at;
        loop {
            if pos > text.len() {
                return None;
            }
            let caps = self.raw_captures_at(text, pos)?;
            let m = caps.get(0).expect("group 0 always matches");
            let (mstart, mend) = (m.start(), m.end());
            if self.pos_ok(text, mstart) {
                return Some(caps);
            }
            // Constraint failed at this position — advance past the whole
            // raw match (or by one char for an empty match) and retry, same
            // non-overlap rule `match_spans` uses below.
            let next = if mend > pos { mend } else { pos + 1 };
            pos = next;
            while pos < text.len() && !text.is_char_boundary(pos) {
                pos += 1;
            }
        }
    }

    /// Every non-overlapping match's *reported* span (honouring `\zs`/`\ze`).
    ///
    /// Vim enumerates matches **non-overlapping**: `searchit()` restarts its
    /// scan at `endpos.col`, so `/o\+` over `fooo` is one match, not three.
    pub fn match_spans(&self, text: &str) -> Vec<(usize, usize)> {
        if self.pos_constraints.is_empty() {
            return match &self.regex {
                CompiledRegex::Fast(re) => re
                    .captures_iter(text)
                    .map(|caps| self.span(&Captures::Fast(caps)))
                    .collect(),
                CompiledRegex::Backref(re) => re
                    .captures_iter(text)
                    .filter_map(|c| c.ok())
                    .map(|caps| self.span(&Captures::Backref(caps)))
                    .collect(),
            };
        }
        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(caps) = self.captures_at(text, pos) {
            let m0 = caps.get(0).expect("group 0 always matches");
            let next = if m0.end() > pos { m0.end() } else { pos + 1 };
            out.push(self.span(&caps));
            pos = next;
            while pos < text.len() && !text.is_char_boundary(pos) {
                pos += 1;
            }
            if pos > text.len() {
                break;
            }
        }
        out
    }
}

pub fn compile(
    pattern: &str,
    ignorecase: bool,
    smartcase: bool,
    smartcase_applies: bool,
    last_sub: &str,
    visual_range: Option<(usize, usize)>,
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
    let regex = if t.has_backref || t.has_lookaround {
        match fancy_regex::Regex::new(&src) {
            Ok(re) => CompiledRegex::Backref(re),
            Err(e) => return Err(format!("E383: Invalid pattern: {pattern} ({e})")),
        }
    } else {
        match regex::Regex::new(&src) {
            Ok(re) => CompiledRegex::Fast(re),
            Err(e) => return Err(format!("E383: Invalid pattern: {pattern} ({e})")),
        }
    };
    Ok(Compiled {
        regex,
        span_group: t.span_group,
        group_map: t.group_map,
        pos_constraints: t.pos_constraints,
        visual_range,
    })
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
    fn backrefs_translate_to_rust_backref_syntax() {
        let t = translate("\\(foo\\)\\1", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "(foo)\\1");
        assert!(t.has_backref);
    }

    #[test]
    fn backref_renumbers_with_zs_injected_group() {
        // `\zs` injects a group ahead of the user's `\(b\)`, shifting its
        // Rust group number from 1 to 2 — the backreference must follow.
        // (No `\ze`, so the injected group's close defaults to the pattern's
        // end and wraps the backref too — that's correct: the reported span
        // starts at `\zs` and runs to the natural end of the match.)
        let t = translate("\\zs\\(b\\)\\1", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "((b)\\2)");
        assert_eq!(t.group_map, vec![0, 2]);
    }

    #[test]
    fn backref_matches_repeated_text() {
        // The #1004 oracle case: `/\(foo\)\1` must match "foofoo" — not stop
        // at the first "foo", and not treat `\1` as a literal "1".
        let c = compile("\\(foo\\)\\1", false, false, true, "", None).unwrap();
        let text = "foo foofoo";
        let caps = c.captures(text).expect("matches foofoo");
        let (s, e) = c.span(&caps);
        assert_eq!(&text[s..e], "foofoo");
        assert_eq!(s, 4);
    }

    #[test]
    fn backref_does_not_match_mismatched_repeat() {
        let c = compile("\\(foo\\)\\1", false, false, true, "", None).unwrap();
        assert!(!c.is_match("foobar"));
    }

    #[test]
    fn nested_group_backref() {
        // `\2` refers to the inner `\(b\)`; requires "ab" immediately
        // followed by another "b".
        let c = compile("\\(a\\(b\\)\\)\\2", false, false, true, "", None).unwrap();
        assert!(c.is_match("abb"));
        assert!(!c.is_match("aba"));
    }

    #[test]
    fn backref_used_twice() {
        let c = compile("\\(a\\)\\1\\1", false, false, true, "", None).unwrap();
        assert!(c.is_match("aaa"));
        assert!(!c.is_match("aab"));
    }

    #[test]
    fn backref_to_zero_translates_to_a_literal_digit() {
        // `\0` is not a valid pattern-atom backreference in Vim (only `:s`
        // replacement text gives `\0` that meaning) — it must not panic.
        // Falls back to the existing "unrecognised escape → literal char" rule.
        let t = translate("\\(a\\)\\0", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "(a)0");
        assert!(!t.has_backref);
    }

    #[test]
    fn backref_to_undefined_group_fails_rather_than_panics() {
        // Only one group is defined; `\2` has nothing to refer to.
        let err = compile("\\(a\\)\\2", false, false, true, "", None).unwrap_err();
        assert!(err.contains("Invalid pattern"), "{err}");
    }

    // #1157: look-around routes through `fancy_regex` (already linked for
    // `\1`..`\9`, #1004) instead of being rejected — one test per form, plus
    // an end-to-end `compile()` case proving the translated regex actually
    // matches/rejects the right text, not just that translation succeeds.

    #[test]
    fn lookahead_positive_wraps_the_preceding_group() {
        let t = translate("\\vfoo(bar)@=", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "foo(?=(bar))");
        assert!(t.has_lookaround);
    }

    #[test]
    fn lookahead_negative() {
        assert_eq!(tr("\\vfoo(bar)@!"), "foo(?!(bar))");
    }

    #[test]
    fn lookbehind_positive() {
        assert_eq!(tr("\\v(foo)@<=bar"), "(?<=(foo))bar");
    }

    #[test]
    fn lookbehind_negative() {
        assert_eq!(tr("\\v(foo)@<!bar"), "(?<!(foo))bar");
    }

    #[test]
    fn lookaround_in_magic_mode_backslash_form() {
        assert_eq!(tr("foo\\(bar\\)\\@="), "foo(?=(bar))");
    }

    #[test]
    fn lookahead_requires_a_preceding_atom() {
        assert!(translate("\\@=", Magic::Magic, "").is_err());
    }

    #[test]
    fn lookaround_atomic_group_is_rejected_with_a_clear_message() {
        let err = translate("\\(foo\\)\\@>", Magic::Magic, "").unwrap_err();
        assert!(err.contains("atomic group"), "{err}");
    }

    #[test]
    fn lookahead_matches_without_consuming() {
        // `foo(?=bar)` matches the "foo" in "foobar" but not in "foobaz" —
        // the lookahead is zero-width, so the reported match is just "foo".
        let c = compile("foo\\(bar\\)\\@=", false, false, true, "", None).unwrap();
        let caps = c.captures("xx foobar").expect("matches");
        let (s, e) = c.span(&caps);
        assert_eq!(&"xx foobar"[s..e], "foo");
        assert!(!c.is_match("xx foobaz"));
    }

    #[test]
    fn lookahead_negative_excludes_the_match() {
        let c = compile("foo\\(bar\\)\\@!", false, false, true, "", None).unwrap();
        assert!(c.is_match("foobaz"));
        assert!(!c.is_match("foobar"));
    }

    #[test]
    fn lookbehind_positive_requires_preceding_text() {
        let c = compile("\\(foo\\)\\@<=bar", false, false, true, "", None).unwrap();
        assert!(c.is_match("foobar"));
        assert!(!c.is_match("bazbar"));
    }

    #[test]
    fn lookbehind_negative_excludes_preceding_text() {
        let c = compile("\\(foo\\)\\@<!bar", false, false, true, "", None).unwrap();
        assert!(c.is_match("bazbar"));
        assert!(!c.is_match("foobar"));
    }

    #[test]
    fn lookaround_group_still_backreferenceable() {
        // Vim numbers `\(...\)` groups by position regardless of whether
        // they end up inside a look-around, so `\1` after `\(foo\)\@=\1`
        // must still resolve to the lookahead's own captured text.
        let t = translate("\\(foo\\)\\@=foo", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "(?=(foo))foo");
        assert!(!t.has_backref);
    }

    #[test]
    fn lookaround_renumbers_zs_offsets_inside_the_wrapped_group() {
        // `\zs` sits *inside* the group that `\@=` wraps — the injected
        // `(?=`/closing `)` must shift the `\zs` capture-group offset that
        // `translate()` later inserts around it, or the reported span lands
        // on the wrong bytes.
        let t = translate("\\(a\\zsb\\)\\@=", Magic::Magic, "").unwrap();
        assert_eq!(t.regex, "(?=(a(b)))");
        assert_eq!(t.span_group, Some(2));
        // Vim group 1 (the user's `\(...\)`) still maps to the outer capture
        // even though it's now nested inside a non-capturing lookahead.
        assert_eq!(t.group_map, vec![0, 1]);
    }

    // #1157: `\_x` — cross-line character classes. The whole buffer is
    // matched as one string with `(?m)` set (see `compile`), so "cross-line"
    // just means "also accept the embedded `\n`".

    #[test]
    fn underscore_dot_matches_newline() {
        assert_eq!(tr("\\_."), "(?:.|\\n)");
        let c = compile("a\\_.b", false, false, true, "", None).unwrap();
        assert!(c.is_match("a\nb"));
        assert!(c.is_match("axb"));
    }

    #[test]
    fn underscore_class_matches_newline() {
        assert_eq!(tr("\\_s"), "(?:[ \\t]|\\n)");
        let c = compile("a\\_s\\+b", false, false, true, "", None).unwrap();
        assert!(c.is_match("a \t\nb"));
    }

    #[test]
    fn underscore_collection_matches_newline() {
        assert_eq!(tr("\\_[abc]"), "(?:[abc]|\\n)");
        let c = compile("x\\_[abc]y", false, false, true, "", None).unwrap();
        assert!(c.is_match("x\ny"));
        assert!(c.is_match("xay"));
        assert!(!c.is_match("xzy"));
    }

    #[test]
    fn underscore_caret_and_dollar_are_anchors_anywhere() {
        // Unlike bare `^`/`$`, `\_^`/`\_$` are documented as usable anywhere
        // in the pattern, not just at the very start/end.
        assert_eq!(tr("a\\_^b"), "a^b");
        assert_eq!(tr("a\\_$b"), "a$b");
    }

    #[test]
    fn underscore_unknown_atom_is_rejected() {
        assert!(translate("\\_q", Magic::Magic, "").is_err());
    }

    #[test]
    fn buffer_anchors() {
        assert_eq!(tr("\\%^foo"), "\\Afoo");
        assert_eq!(tr("foo\\%$"), "foo\\z");
    }

    // #1157: `\%d123` / `\%x2a` — decimal/hex character-code literals.

    #[test]
    fn percent_d_decimal_char_code() {
        // 65 = 'A'
        assert_eq!(tr("\\%d65"), "A");
    }

    #[test]
    fn percent_x_hex_char_code() {
        // 0x2a = '*', which must come out escaped since it's regex-special.
        assert_eq!(tr("\\%x2a"), "\\*");
    }

    #[test]
    fn percent_x_reads_at_most_two_hex_digits() {
        // `\%x412` is codepoint 0x41 ('A') followed by a literal '2'.
        assert_eq!(tr("\\%x412"), "A2");
    }

    #[test]
    fn percent_d_requires_a_digit() {
        assert!(translate("\\%dx", Magic::Magic, "").is_err());
    }

    // #1157: `\%23l` / `\%23c` — absolute line/column position assertions,
    // applied as a post-match filter (`Compiled::pos_ok`) since neither
    // `regex` nor `fancy_regex` has an "absolute line number" primitive.

    #[test]
    fn percent_l_restricts_match_to_that_line() {
        let c = compile("\\%2lfoo", false, false, true, "", None).unwrap();
        let text = "foo\nfoo\nfoo";
        // Only line 2's "foo" (bytes 4..7) satisfies `\%2l`.
        let caps = c.captures(text).expect("matches on line 2");
        let (s, e) = c.span(&caps);
        assert_eq!((s, e), (4, 7));
        assert_eq!(c.match_spans(text), vec![(4, 7)]);
    }

    #[test]
    fn percent_lt_and_gt_line_comparators() {
        let text = "foo\nfoo\nfoo";
        let before = compile("\\%<2lfoo", false, false, true, "", None).unwrap();
        assert_eq!(before.match_spans(text), vec![(0, 3)]);
        let after = compile("\\%>2lfoo", false, false, true, "", None).unwrap();
        assert_eq!(after.match_spans(text), vec![(8, 11)]);
    }

    #[test]
    fn percent_c_restricts_match_to_that_column() {
        // Column is 1-indexed and byte-based; "xfoo" has 'f' at column 2.
        let c = compile("\\%2cfoo", false, false, true, "", None).unwrap();
        assert!(c.is_match("xfoo"));
        assert!(!c.is_match("xxfoo"));
    }

    #[test]
    fn percent_l_with_no_matching_line_reports_no_match() {
        let c = compile("\\%99lfoo", false, false, true, "", None).unwrap();
        assert!(!c.is_match("foo\nfoo"));
    }

    // #1157: `\%V` — restrict the match to the last Visual selection, given
    // to `compile` as a byte range (the engine, not `vim_regex`, knows what
    // the last selection was).

    #[test]
    fn percent_v_restricts_match_to_the_visual_range() {
        let text = "foo bar foo";
        // Only the first "foo" (bytes 0..3) falls inside [0, 7).
        let c = compile("\\%Vfoo", false, false, true, "", Some((0, 7))).unwrap();
        assert_eq!(c.match_spans(text), vec![(0, 3)]);
    }

    #[test]
    fn percent_v_with_no_visual_range_never_matches() {
        let c = compile("\\%Vfoo", false, false, true, "", None).unwrap();
        assert!(!c.is_match("foo"));
    }

    // #1157: `\&` and `\%[...]` stay rejected (no faithful translation exists
    // in either regex crate) but with a clearer message than before.

    #[test]
    fn branch_concat_is_rejected_with_a_clear_message() {
        let err = translate("foo\\&bar", Magic::Magic, "").unwrap_err();
        assert!(err.contains("\\&"), "{err}");
        assert!(err.contains("1157"), "{err}");
    }

    #[test]
    fn optional_sequence_is_rejected_with_a_clear_message() {
        let err = translate("r\\%[ead]", Magic::Magic, "").unwrap_err();
        assert!(err.contains("\\%[...]"), "{err}");
        assert!(err.contains("1157"), "{err}");
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
        let c = compile("foo", true, true, true, "", None).unwrap();
        assert!(c.is_match("FOO"));
        // uppercase in pattern → sensitive
        let c = compile("Foo", true, true, true, "", None).unwrap();
        assert!(!c.is_match("FOO"));
        // `*` sets smartcase_applies = false → stays insensitive
        let c = compile("Foo", true, true, false, "", None).unwrap();
        assert!(c.is_match("FOO"));
    }

    #[test]
    fn compile_rejects_invalid_pattern() {
        let err = compile("a\\{2,1}", false, false, true, "", None).unwrap_err();
        assert!(err.contains("Invalid pattern"), "{err}");
    }

    #[test]
    fn multiline_flag_is_on_so_anchors_are_per_line() {
        let c = compile("^b", false, false, true, "", None).unwrap();
        assert!(c.is_match("a\nb"));
    }
}
