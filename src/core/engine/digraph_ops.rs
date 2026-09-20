use super::*;
use crate::core::digraphs;

impl Engine {
    /// Look up a digraph, checking user-defined entries (`:digraph`) before
    /// the builtin table (`:h digraphs`, #1160).
    pub(crate) fn digraph_lookup(&self, c1: char, c2: char) -> Option<char> {
        digraphs::lookup(c1, c2, &self.custom_digraphs)
    }

    /// `:digraphs` (`:h :digraphs`). No args: list every known digraph
    /// (custom entries first, then the builtin table). With args of the
    /// form `{char1}{char2} {number} [{char1}{char2} {number} ...]`: define
    /// one or more custom digraphs (`:h digraph-usage`), each `{number}` a
    /// decimal Unicode codepoint.
    pub(crate) fn ex_digraphs(&mut self, args: &str) -> EngineAction {
        if args.is_empty() {
            let mut lines: Vec<String> = Vec::new();
            for (&(c1, c2), &ch) in &self.custom_digraphs {
                lines.push(format!("{c1}{c2} {ch}  {}", ch as u32));
            }
            for &(c1, c2, ch) in digraphs::BUILTIN_DIGRAPHS {
                lines.push(format!("{c1}{c2} {ch}  {}", ch as u32));
            }
            self.message = lines.join("\n");
            return EngineAction::None;
        }

        // Parse repeated `{c1}{c2} {number}` groups, whitespace-separated.
        let tokens: Vec<&str> = args.split_whitespace().collect();
        let mut i = 0;
        let mut added = 0usize;
        let mut errors: Vec<String> = Vec::new();
        while i + 1 < tokens.len() {
            let pair = tokens[i];
            let num_str = tokens[i + 1];
            i += 2;
            let mut chars = pair.chars();
            let (Some(c1), Some(c2)) = (chars.next(), chars.next()) else {
                errors.push(format!(
                    "E1214: Digraph must be just two characters: {pair}"
                ));
                continue;
            };
            match num_str.parse::<u32>().ok().and_then(char::from_u32) {
                Some(ch) => {
                    self.custom_digraphs.insert((c1, c2), ch);
                    added += 1;
                }
                None => {
                    errors.push(format!("E39: Number expected: {num_str}"));
                }
            }
        }
        if !errors.is_empty() {
            self.message = errors.join("\n");
        } else if added > 0 {
            self.message = format!("{added} digraph(s) added");
        } else {
            self.message = "E474: Invalid argument".to_string();
        }
        EngineAction::None
    }

    /// `<C-k>{c1}{c2}` in Insert mode (`:h i_CTRL-K`, #1160): look up the
    /// digraph and insert its character, or ring the bell (no-op) if it's
    /// not a known pair.
    pub(crate) fn insert_digraph(&mut self, c1: char, c2: char, changed: &mut bool) {
        if let Some(ch) = self.digraph_lookup(c1, c2) {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            let s = ch.to_string();
            self.insert_with_undo(char_idx, &s);
            self.insert_text_buffer.push_str(&s);
            self.view_mut().cursor.col += 1;
            *changed = true;
        } else {
            self.message = format!("E790: No digraph for {c1}{c2}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::engine_with_text;

    #[test]
    fn digraphs_list_includes_builtin_entries() {
        let mut engine = engine_with_text("");
        engine.ex_digraphs("");
        assert!(engine.message.contains("a: ä"));
        assert!(engine.message.contains("-> →"));
    }

    #[test]
    fn digraphs_add_custom_entry_and_lookup() {
        let mut engine = engine_with_text("");
        engine.ex_digraphs("zz 9733");
        assert_eq!(engine.message, "1 digraph(s) added");
        assert_eq!(engine.digraph_lookup('z', 'z'), Some('★'));
    }

    #[test]
    fn digraphs_add_bad_number_reports_error() {
        let mut engine = engine_with_text("");
        engine.ex_digraphs("zz notanumber");
        assert!(engine.message.contains("E39"));
    }
}
