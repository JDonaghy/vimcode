//! Sticky scroll (#1546): pin the header lines of the buffer's enclosing
//! scopes (e.g. `impl Foo {`, `fn bar() {`) at the top of the editor pane
//! while scrolling, VS Code's `editor.stickyScroll.enabled`.
//!
//! # Scope source
//!
//! The issue asks for LSP `documentSymbol` ranges as the primary source,
//! falling back to tree-sitter/indent folding ranges. This module ships
//! **only the fallback half**: `SymbolInfo` (`crate::core::lsp`) currently
//! only carries a symbol's `selectionRange` start position (`line`/
//! `character` — used for "jump to symbol" in the command-center picker),
//! not the full `range` a `DocumentSymbol` also reports, so there is no
//! `end_line` to test "does this symbol's body enclose the top-of-viewport
//! line" with. Adding that would mean threading a new field through
//! `SymbolInfo` and its ~28 construction sites across `lsp.rs`/`picker.rs`/
//! `tests.rs` — out of scope for this change's file list
//! (`src/core/settings.rs`, `src/render.rs`, `src/core/engine/`). Filed as a
//! follow-up rather than silently shipped as "the real thing": see the PR
//! description for #1546.
//!
//! What ships here is a real, useful, independent implementation: the
//! buffer's indent hierarchy, computed the same way
//! `Engine::compute_indent_folds` does for `'foldmethod'=indent` but
//! **independent of `'foldmethod'`** (sticky scroll must work regardless of
//! the user's fold settings, exactly as VS Code's does) — using the same
//! "the less-indented line immediately before a block is that block's
//! header" heuristic `Engine::detect_fold_range`'s doc comment already
//! documents and uses elsewhere in this crate.

use crate::core::buffer::Buffer;

/// VS Code's `editor.stickyScroll.maxLineCount` default, and the cap named
/// explicitly in the issue.
pub(crate) const STICKY_SCROLL_MAX_LINES: usize = 5;

/// Count leading whitespace columns on `line_idx` (spaces = 1, tabs = 4) —
/// a buffer-generic twin of `Engine::line_indent`, needed here because
/// sticky scroll must compute headers for *any* window's buffer, not just
/// the active one `Engine::line_indent` is hard-wired to (`self.buffer()`).
fn line_indent(buffer: &Buffer, line_idx: usize) -> usize {
    if line_idx >= buffer.len_lines() {
        return 0;
    }
    let line = buffer.content.line(line_idx);
    let tab_width = 4usize;
    let mut indent = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => indent += 1,
            '\t' => indent += tab_width,
            _ => break,
        }
    }
    indent
}

/// Is `line_idx` blank (whitespace only, including a bare `\n`/`\r\n`)?
fn is_blank_line(buffer: &Buffer, line_idx: usize) -> bool {
    if line_idx >= buffer.len_lines() {
        return true;
    }
    buffer
        .content
        .line(line_idx)
        .chars()
        .all(|c| c.is_whitespace())
}

/// The buffer-line indices of the header lines enclosing `top_line`
/// (the window's `scroll_top` — the first buffer line the viewport would
/// otherwise show), outermost first, capped at [`STICKY_SCROLL_MAX_LINES`].
///
/// Walks upward from `top_line`, treating each new record-low indent level
/// as the header of the block that encloses everything below it down to
/// `top_line` — the same heuristic `detect_fold_range` uses for "what's the
/// header of the block under the cursor", just walking up instead of down.
/// Blank lines are skipped (they carry no indent signal). The scan is
/// bounded by the first ancestor at indent level 0 (the outermost possible
/// scope) or by `top_line` reaching 0, so it's O(distance to the nearest
/// column-0 line above the viewport) per frame, not O(buffer length) —
/// cheap for realistically-formatted source, worst case for a pathological
/// buffer with no column-0 line above (e.g. one giant unindented block)
/// bounded by `top_line` itself.
pub(crate) fn enclosing_scope_headers(
    buffer: &Buffer,
    top_line: usize,
    shift_width: usize,
) -> Vec<usize> {
    if top_line == 0 {
        return Vec::new();
    }
    let shift_width = shift_width.max(1);
    let level_of = |line_idx: usize| line_indent(buffer, line_idx) / shift_width;

    // Seed the "current" level from the nearest non-blank line at or after
    // `top_line` — if `top_line` itself is blank, its own indent is
    // meaningless (0), which would wrongly stop the scan immediately.
    let total = buffer.len_lines();
    let mut min_level = (top_line..total)
        .find(|&l| !is_blank_line(buffer, l))
        .map(level_of)
        .unwrap_or(usize::MAX);

    let mut headers = Vec::new();
    let mut i = top_line;
    while i > 0 && headers.len() < STICKY_SCROLL_MAX_LINES {
        i -= 1;
        if is_blank_line(buffer, i) {
            continue;
        }
        let lvl = level_of(i);
        if lvl < min_level {
            headers.push(i);
            min_level = lvl;
            if lvl == 0 {
                break;
            }
        }
    }
    headers.reverse(); // outermost first, matching pin order top-to-bottom.
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::buffer::{Buffer, BufferId};

    fn buf(text: &str) -> Buffer {
        Buffer::from_text(BufferId(0), text)
    }

    #[test]
    fn top_of_buffer_has_no_headers() {
        let b = buf("fn main() {\n    let x = 1;\n}\n");
        assert_eq!(enclosing_scope_headers(&b, 0, 4), Vec::<usize>::new());
    }

    #[test]
    fn single_level_nesting_finds_one_header() {
        let text = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        let b = buf(text);
        // top_line = 2 ("let y = 2;") is enclosed by line 0's "fn main() {".
        assert_eq!(enclosing_scope_headers(&b, 2, 4), vec![0]);
    }

    #[test]
    fn nested_blocks_return_outermost_first() {
        let text = "impl Foo {\n    fn bar() {\n        let x = 1;\n        let y = 2;\n    }\n}\n";
        let b = buf(text);
        // top_line = 3 ("let y = 2;") is enclosed by "fn bar() {" (line 1)
        // which is enclosed by "impl Foo {" (line 0).
        assert_eq!(enclosing_scope_headers(&b, 3, 4), vec![0, 1]);
    }

    #[test]
    fn caps_at_max_lines() {
        // 7 levels of nesting — more than STICKY_SCROLL_MAX_LINES (5).
        let mut text = String::new();
        for i in 0..7 {
            text.push_str(&" ".repeat(i * 4));
            text.push_str("if true {\n");
        }
        text.push_str(&" ".repeat(7 * 4));
        text.push_str("deep();\n");
        let b = buf(&text);
        let headers = enclosing_scope_headers(&b, 7, 4);
        assert_eq!(headers.len(), STICKY_SCROLL_MAX_LINES);
        // Nearest 5 ancestors to the top line, outermost-first within that
        // window: levels 2..6 (0-indexed lines 2..6), not the true global
        // outermost (line 0) — matches VS Code's own "closest N" behaviour
        // when a scope stack exceeds the cap.
        assert_eq!(headers, vec![2, 3, 4, 5, 6]);
    }

    #[test]
    fn blank_lines_are_skipped_not_treated_as_headers() {
        let text = "fn main() {\n\n    let x = 1;\n}\n";
        let b = buf(text);
        assert_eq!(enclosing_scope_headers(&b, 2, 4), vec![0]);
    }
}
