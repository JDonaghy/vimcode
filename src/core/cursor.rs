#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self { line: 0, col: 0 }
    }
}
