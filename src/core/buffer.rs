use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use ropey::Rope;

/// Unique identifier for a buffer within the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);

#[derive(Debug, Clone)]
pub struct Buffer {
    #[allow(dead_code)]
    pub id: BufferId,
    pub content: Rope,
}

impl Buffer {
    pub fn new(id: BufferId) -> Self {
        Self {
            id,
            content: Rope::new(),
        }
    }

    #[allow(dead_code)]
    pub fn from_text(id: BufferId, text: &str) -> Self {
        Self {
            id,
            content: Rope::from_str(text),
        }
    }

    /// Load buffer contents from a file. Returns an io::Error if reading fails.
    pub fn from_file(id: BufferId, path: &Path) -> Result<Self, io::Error> {
        let text = fs::read_to_string(path)?;
        Ok(Self {
            id,
            content: Rope::from_str(&text),
        })
    }

    /// Write buffer contents to a file.
    pub fn save_to_file(&self, path: &Path) -> Result<(), io::Error> {
        fs::write(path, self.to_string())
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

    #[allow(dead_code)]
    pub fn len_chars(&self) -> usize {
        self.content.len_chars()
    }

    pub fn line_to_char(&self, line_idx: usize) -> usize {
        self.content.line_to_char(line_idx)
    }

    /// Returns the number of visible lines in the buffer.
    ///
    /// Ropey's `len_lines()` counts a trailing `\n` as starting a new (empty)
    /// line. For cursor navigation we want the count of lines that actually
    /// contain content, so we subtract 1 when the text ends with `\n`.
    pub fn len_lines(&self) -> usize {
        let n = self.content.len_lines();
        if n > 1
            && self.content.len_chars() > 0
            && self.content.char(self.content.len_chars() - 1) == '\n'
        {
            n - 1
        } else {
            n
        }
    }

    pub fn line_len_chars(&self, line_idx: usize) -> usize {
        if line_idx >= self.len_lines() {
            return 0;
        }
        self.content.line(line_idx).len_chars()
    }
}

impl fmt::Display for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_editing() {
        let mut buffer = Buffer::new(BufferId(1));
        buffer.insert(0, "Hello");
        assert_eq!(buffer.to_string(), "Hello");

        buffer.insert(5, " World");
        assert_eq!(buffer.to_string(), "Hello World");

        buffer.delete_range(5, 11);
        assert_eq!(buffer.to_string(), "Hello");
    }
}
