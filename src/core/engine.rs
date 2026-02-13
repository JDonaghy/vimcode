use super::{Buffer, Cursor, Mode, Syntax};

pub struct Engine {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub mode: Mode,
    pub syntax: Syntax,
    pub highlights: Vec<(usize, usize, String)>,
}

impl Engine {
    pub fn new() -> Self {
        let mut engine = Self {
            buffer: Buffer::new(),
            cursor: Cursor::new(),
            mode: Mode::Normal,
            syntax: Syntax::new(),
            highlights: Vec::new(),
        };
        // Initial parse
        engine.update_syntax();
        engine
    }

    pub fn update_syntax(&mut self) {
        // PERF: Inefficient for large files
        let text = self.buffer.to_string();
        self.highlights = self.syntax.parse(&text);
    }

    pub fn handle_key(&mut self, key: &str) {
        let mut changed = false;
        match self.mode {
            Mode::Normal => match key {
                "h" => {
                    if self.cursor.col > 0 {
                        self.cursor.col -= 1
                    }
                }
                "j" => self.cursor.line += 1,
                "k" => {
                    if self.cursor.line > 0 {
                        self.cursor.line -= 1
                    }
                }
                "l" => self.cursor.col += 1,
                "i" => self.mode = Mode::Insert,
                _ => {}
            },
            Mode::Insert => match key {
                "Escape" => self.mode = Mode::Normal,
                "Backspace" => {
                    if self.cursor.col > 0 {
                        let char_idx = self.buffer.line_to_char(self.cursor.line) + self.cursor.col;
                        self.buffer.delete_range(char_idx - 1, char_idx);
                        self.cursor.col -= 1;
                        changed = true;
                    }
                }
                "Return" => {
                    let char_idx = self.buffer.line_to_char(self.cursor.line) + self.cursor.col;
                    self.buffer.insert(char_idx, "\n");
                    self.cursor.line += 1;
                    self.cursor.col = 0;
                    changed = true;
                }
                c => {
                    if c.len() == 1 {
                        let char_idx = self.buffer.line_to_char(self.cursor.line) + self.cursor.col;
                        self.buffer.insert(char_idx, c);
                        self.cursor.col += 1;
                        changed = true;
                    }
                }
            },
        }

        if changed {
            self.update_syntax();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_movement() {
        let mut engine = Engine::new();
        engine.buffer.insert(0, "Hello");

        engine.handle_key("l");
        assert_eq!(engine.cursor.col, 1);

        engine.handle_key("h");
        assert_eq!(engine.cursor.col, 0);
    }

    #[test]
    fn test_insert_mode() {
        let mut engine = Engine::new();
        engine.handle_key("i");
        assert_eq!(engine.mode, Mode::Insert);

        engine.handle_key("A");
        assert_eq!(engine.buffer.to_string(), "A");
        assert_eq!(engine.cursor.col, 1);

        engine.handle_key("Escape");
        assert_eq!(engine.mode, Mode::Normal);
    }
}
