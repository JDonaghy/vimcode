use ropey::Rope;

#[derive(Debug, Clone)]
pub struct Buffer {
    pub content: Rope,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            content: Rope::new(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            content: Rope::from_str(text),
        }
    }

    pub fn insert(&mut self, char_idx: usize, text: &str) {
        if char_idx <= self.content.len_chars() {
            self.content.insert(char_idx, text);
        }
    }

    pub fn delete_range(&mut self, start_idx: usize, end_idx: usize) {
        if start_idx < end_idx && end_idx <= self.content.len_chars() {
            self.content.remove(start_idx..end_idx);
        }
    }

    pub fn len_chars(&self) -> usize {
        self.content.len_chars()
    }

    pub fn line_to_char(&self, line_idx: usize) -> usize {
        self.content.line_to_char(line_idx)
    }

    pub fn to_string(&self) -> String {
        self.content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_editing() {
        let mut buffer = Buffer::new();
        buffer.insert(0, "Hello");
        assert_eq!(buffer.to_string(), "Hello");

        buffer.insert(5, " World");
        assert_eq!(buffer.to_string(), "Hello World");

        buffer.delete_range(5, 11);
        assert_eq!(buffer.to_string(), "Hello");
    }
}
