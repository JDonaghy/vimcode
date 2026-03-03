#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    /// Overtype mode: printable chars overwrite rather than insert. Esc returns to Normal.
    Replace,
    Command,
    Search,
    Visual,
    VisualLine,
    VisualBlock,
}
