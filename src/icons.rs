//! Nerd Font file-type icons shared by both the GTK and TUI backends.
//!
//! All icons are characters from the Nerd Fonts patched font set.
//! Requires a Nerd Font to be installed and configured as the editor font
//! for the glyphs to render correctly.

/// Return a Nerd Font icon character for the given file extension.
/// Returns the generic file icon for unknown extensions.
pub fn file_icon(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "rs" => "\u{e7a8}",                         // nf-dev-rust
        "py" => "\u{f81f}",                         // nf-seti-python
        "js" | "jsx" | "mjs" | "cjs" => "\u{f81d}", // nf-seti-javascript
        "ts" | "tsx" => "\u{e628}",                 // nf-dev-typescript
        "go" => "\u{e724}",                         // nf-dev-go
        "cpp" | "cc" | "cxx" | "c" => "\u{e61d}",   // nf-dev-cplusplus
        "h" | "hpp" => "\u{f0fd}",                  // nf-fa-h_square
        "md" | "markdown" => "\u{f48a}",            // nf-fa-markdown
        "json" => "\u{e60b}",                       // nf-seti-json
        "toml" => "\u{e6b2}",                       // nf-seti-config
        "yaml" | "yml" => "\u{e6a8}",               // nf-seti-yaml
        "html" | "htm" => "\u{f13b}",               // nf-fa-html5
        "css" => "\u{e749}",                        // nf-dev-css3
        "sh" | "bash" | "zsh" => "\u{f489}",        // nf-fa-terminal
        "lua" => "\u{e620}",                        // nf-dev-lua
        "txt" => "\u{f0f6}",                        // nf-fa-file_text_o
        _ => "\u{f15b}",                            // nf-fa-file (generic)
    }
}
