#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self { line: 0, col: 0 }
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}
