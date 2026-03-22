//! Lightweight markdown → styled plain text converter.
//!
//! Uses `pulldown-cmark` to parse CommonMark and emits clean text with byte-
//! offset style spans that both the GTK and TUI backends can render using their
//! native bold/italic/colour support.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::syntax::{Syntax, SyntaxLanguage};

// ─── Output types ────────────────────────────────────────────────────────────

/// The kind of visual style applied to a span of markdown-rendered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdStyle {
    Heading(u8), // 1–6
    Bold,
    Italic,
    BoldItalic,
    Code,
    CodeBlock,
    Link,
    LinkUrl,
    BlockQuote,
    ListBullet,
    HorizontalRule,
    Image,
}

/// A styled byte-range within one rendered line.
#[derive(Debug, Clone)]
pub struct MdSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub style: MdStyle,
}

/// A syntax-highlight span within a code-block line (byte offsets + tree-sitter scope).
#[derive(Debug, Clone)]
pub struct MdCodeHighlight {
    pub start_byte: usize,
    pub end_byte: usize,
    pub scope: String,
}

/// The complete result of rendering a markdown document to styled plain text.
#[derive(Debug, Clone)]
pub struct MdRendered {
    /// One entry per rendered line (plain text, markdown syntax stripped).
    pub lines: Vec<String>,
    /// Per-line style spans (byte offsets into the corresponding `lines` entry).
    pub spans: Vec<Vec<MdSpan>>,
    /// Per-line tree-sitter highlights for code-block lines.
    /// Empty for non-code-block lines.
    pub code_highlights: Vec<Vec<MdCodeHighlight>>,
}

// ─── Rendering ───────────────────────────────────────────────────────────────

/// Convert a markdown string into styled plain text.
pub fn render_markdown(input: &str) -> MdRendered {
    let mut lines: Vec<String> = Vec::new();
    let mut spans: Vec<Vec<MdSpan>> = Vec::new();
    let mut code_highlights: Vec<Vec<MdCodeHighlight>> = Vec::new();
    // Current line being built.
    let mut cur_line = String::new();
    let mut cur_spans: Vec<MdSpan> = Vec::new();

    // Style stack: (bold, italic, heading level, code, link, blockquote, image)
    let mut bold = false;
    let mut italic = false;
    let mut heading: Option<u8> = None;
    let code_inline = false;
    let mut in_link = false;
    let mut link_url: Option<String> = None;
    let mut blockquote_depth: usize = 0;
    let mut in_code_block = false;
    let mut in_image = false;

    // Code block language + accumulated raw text for syntax highlighting.
    let mut code_block_lang: Option<SyntaxLanguage> = None;
    let mut code_block_text = String::new();
    let mut code_block_start_line: usize = 0;

    // List tracking: stack of (ordered?, counter).
    let mut list_stack: Vec<(bool, u64)> = Vec::new();
    let mut need_list_bullet = false;
    let mut in_list_item = false;

    // Whether a new line has been started since the last content.
    let mut at_line_start = true;

    let flush_line = |lines: &mut Vec<String>,
                      spans: &mut Vec<Vec<MdSpan>>,
                      code_highlights: &mut Vec<Vec<MdCodeHighlight>>,
                      cur_line: &mut String,
                      cur_spans: &mut Vec<MdSpan>| {
        lines.push(std::mem::take(cur_line));
        spans.push(std::mem::take(cur_spans));
        code_highlights.push(Vec::new());
    };

    let opts = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(input, opts);

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    heading = Some(level as u8);
                }
                Tag::Emphasis => italic = true,
                Tag::Strong => bold = true,
                Tag::CodeBlock(kind) => {
                    in_code_block = true;
                    // Capture language from fenced code block.
                    code_block_lang = match &kind {
                        CodeBlockKind::Fenced(lang) => {
                            let lang_str = lang.split_whitespace().next().unwrap_or("");
                            SyntaxLanguage::from_name(lang_str)
                        }
                        CodeBlockKind::Indented => None,
                    };
                    code_block_text.clear();
                    // Start code block on a new line.
                    if !cur_line.is_empty() {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                    }
                    code_block_start_line = lines.len();
                }
                Tag::Link { dest_url, .. } => {
                    in_link = true;
                    link_url = Some(dest_url.to_string());
                }
                Tag::BlockQuote(_) => {
                    blockquote_depth += 1;
                }
                Tag::List(start) => {
                    if let Some(n) = start {
                        list_stack.push((true, n));
                    } else {
                        list_stack.push((false, 0));
                    }
                }
                Tag::Item => {
                    need_list_bullet = true;
                    in_list_item = true;
                }
                Tag::Image { .. } => {
                    in_image = true;
                }
                Tag::Paragraph => {
                    // Ensure paragraph starts on its own line.
                    if !cur_line.is_empty() {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                    }
                    at_line_start = true;
                }
                _ => {}
            },

            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    heading = None;
                    flush_line(
                        &mut lines,
                        &mut spans,
                        &mut code_highlights,
                        &mut cur_line,
                        &mut cur_spans,
                    );
                    // Blank line after heading.
                    lines.push(String::new());
                    spans.push(Vec::new());
                    code_highlights.push(Vec::new());
                    at_line_start = true;
                }
                TagEnd::Emphasis => italic = false,
                TagEnd::Strong => bold = false,
                TagEnd::CodeBlock => {
                    // Flush any remaining code line.
                    if !cur_line.is_empty() {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                    }
                    // Run tree-sitter on the accumulated code block text.
                    if let Some(lang) = code_block_lang.take() {
                        let mut syntax = Syntax::new_for_language(lang);
                        let highlights = syntax.parse(&code_block_text);
                        // Map highlights (byte offsets into code_block_text)
                        // back to per-line MdCodeHighlight spans, adjusting
                        // for the 4-space indent prefix.
                        let indent = 4usize;
                        // Build a line-start-byte map for the raw code text.
                        let raw_lines: Vec<&str> = code_block_text.split('\n').collect();
                        let mut line_byte_starts = Vec::with_capacity(raw_lines.len());
                        let mut offset = 0usize;
                        for raw in &raw_lines {
                            line_byte_starts.push(offset);
                            offset += raw.len() + 1; // +1 for '\n'
                        }
                        for (start, end, scope) in &highlights {
                            // Find which raw line this highlight starts on.
                            let raw_line_idx = match line_byte_starts.binary_search(start) {
                                Ok(i) => i,
                                Err(i) => i.saturating_sub(1),
                            };
                            let out_line_idx = code_block_start_line + raw_line_idx;
                            if out_line_idx >= code_highlights.len() {
                                continue;
                            }
                            let line_start = line_byte_starts[raw_line_idx];
                            let local_start = start - line_start + indent;
                            let local_end = end - line_start + indent;
                            code_highlights[out_line_idx].push(MdCodeHighlight {
                                start_byte: local_start,
                                end_byte: local_end,
                                scope: scope.clone(),
                            });
                        }
                    }
                    code_block_text.clear();
                    in_code_block = false;
                    at_line_start = true;
                }
                TagEnd::Link => {
                    // Append " (url)" after link text.
                    if let Some(url) = link_url.take() {
                        if !url.is_empty() {
                            let prefix = " (";
                            cur_line.push_str(prefix);
                            let url_start = cur_line.len();
                            cur_line.push_str(&url);
                            let url_end = cur_line.len();
                            cur_line.push(')');
                            cur_spans.push(MdSpan {
                                start_byte: url_start,
                                end_byte: url_end,
                                style: MdStyle::LinkUrl,
                            });
                        }
                    }
                    in_link = false;
                }
                TagEnd::BlockQuote(_) => {
                    blockquote_depth = blockquote_depth.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                    // Blank line after top-level list.
                    if list_stack.is_empty() && !cur_line.is_empty() {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                    }
                }
                TagEnd::Item => {
                    in_list_item = false;
                    if !cur_line.is_empty() {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                    }
                }
                TagEnd::Paragraph => {
                    if !cur_line.is_empty() {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                    }
                    // Blank line after paragraph (but not inside list items).
                    if !in_list_item {
                        lines.push(String::new());
                        spans.push(Vec::new());
                        code_highlights.push(Vec::new());
                    }
                    at_line_start = true;
                }
                TagEnd::Image => {
                    in_image = false;
                }
                _ => {}
            },

            Event::Text(text) => {
                if in_image {
                    let label = format!("[Image: {text}]");
                    let start = cur_line.len();
                    cur_line.push_str(&label);
                    cur_spans.push(MdSpan {
                        start_byte: start,
                        end_byte: cur_line.len(),
                        style: MdStyle::Image,
                    });
                    continue;
                }

                if in_code_block {
                    // Accumulate raw text for tree-sitter.
                    code_block_text.push_str(&text);
                    // Code block: 4-space indent each line.
                    for (i, code_line) in text.split('\n').enumerate() {
                        if i > 0 {
                            flush_line(
                                &mut lines,
                                &mut spans,
                                &mut code_highlights,
                                &mut cur_line,
                                &mut cur_spans,
                            );
                        }
                        if !code_line.is_empty() || i == 0 {
                            let start = cur_line.len();
                            cur_line.push_str("    ");
                            cur_line.push_str(code_line);
                            cur_spans.push(MdSpan {
                                start_byte: start,
                                end_byte: cur_line.len(),
                                style: MdStyle::CodeBlock,
                            });
                        }
                    }
                    continue;
                }

                // Process text line by line (handles literal newlines in source).
                for (i, chunk) in text.split('\n').enumerate() {
                    if i > 0 {
                        flush_line(
                            &mut lines,
                            &mut spans,
                            &mut code_highlights,
                            &mut cur_line,
                            &mut cur_spans,
                        );
                        at_line_start = true;
                    }

                    if chunk.is_empty() && i > 0 {
                        continue;
                    }

                    // Blockquote prefix.
                    if at_line_start && blockquote_depth > 0 && cur_line.is_empty() {
                        for _ in 0..blockquote_depth {
                            let start = cur_line.len();
                            cur_line.push_str("│ ");
                            cur_spans.push(MdSpan {
                                start_byte: start,
                                end_byte: cur_line.len(),
                                style: MdStyle::BlockQuote,
                            });
                        }
                    }

                    // List bullet / number.
                    if need_list_bullet && cur_line.is_empty() {
                        let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                        cur_line.push_str(&indent);
                        let start = cur_line.len();
                        if let Some((ordered, counter)) = list_stack.last_mut() {
                            if *ordered {
                                let label = format!("{counter}. ");
                                cur_line.push_str(&label);
                                *counter += 1;
                            } else {
                                cur_line.push_str("• ");
                            }
                        }
                        cur_spans.push(MdSpan {
                            start_byte: start,
                            end_byte: cur_line.len(),
                            style: MdStyle::ListBullet,
                        });
                        need_list_bullet = false;
                    }

                    let start = cur_line.len();
                    cur_line.push_str(chunk);
                    let end = cur_line.len();

                    if start < end {
                        let style = if let Some(h) = heading {
                            MdStyle::Heading(h)
                        } else if code_inline {
                            MdStyle::Code
                        } else if bold && italic {
                            MdStyle::BoldItalic
                        } else if bold {
                            MdStyle::Bold
                        } else if italic {
                            MdStyle::Italic
                        } else if in_link {
                            MdStyle::Link
                        } else {
                            at_line_start = false;
                            continue;
                        };

                        cur_spans.push(MdSpan {
                            start_byte: start,
                            end_byte: end,
                            style,
                        });
                    }
                    at_line_start = false;
                }
            }

            Event::Code(code) => {
                // Inline code.
                let start = cur_line.len();
                cur_line.push_str(&code);
                cur_spans.push(MdSpan {
                    start_byte: start,
                    end_byte: cur_line.len(),
                    style: MdStyle::Code,
                });
            }

            Event::SoftBreak => {
                // Treat soft break as a space.
                cur_line.push(' ');
            }

            Event::HardBreak => {
                flush_line(
                    &mut lines,
                    &mut spans,
                    &mut code_highlights,
                    &mut cur_line,
                    &mut cur_spans,
                );
                at_line_start = true;
            }

            Event::Rule => {
                if !cur_line.is_empty() {
                    flush_line(
                        &mut lines,
                        &mut spans,
                        &mut code_highlights,
                        &mut cur_line,
                        &mut cur_spans,
                    );
                }
                let rule = "────────────────────────────────────────";
                let start = cur_line.len();
                cur_line.push_str(rule);
                cur_spans.push(MdSpan {
                    start_byte: start,
                    end_byte: cur_line.len(),
                    style: MdStyle::HorizontalRule,
                });
                flush_line(
                    &mut lines,
                    &mut spans,
                    &mut code_highlights,
                    &mut cur_line,
                    &mut cur_spans,
                );
                // Blank line after rule.
                lines.push(String::new());
                spans.push(Vec::new());
                code_highlights.push(Vec::new());
                at_line_start = true;
            }

            _ => {}
        }
    }

    // Flush any remaining content.
    if !cur_line.is_empty() {
        flush_line(
            &mut lines,
            &mut spans,
            &mut code_highlights,
            &mut cur_line,
            &mut cur_spans,
        );
    }

    MdRendered {
        lines,
        spans,
        code_highlights,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_produces_bold_text() {
        let r = render_markdown("# Hello World");
        assert!(!r.lines.is_empty());
        assert_eq!(r.lines[0], "Hello World");
        assert!(r.spans[0].iter().any(|s| s.style == MdStyle::Heading(1)));
    }

    #[test]
    fn bold_and_italic() {
        let r = render_markdown("**bold** and *italic*");
        assert!(r.lines[0].contains("bold"));
        assert!(r.lines[0].contains("italic"));
        assert!(r.spans[0].iter().any(|s| s.style == MdStyle::Bold));
        assert!(r.spans[0].iter().any(|s| s.style == MdStyle::Italic));
    }

    #[test]
    fn inline_code() {
        let r = render_markdown("Use `foo()` here");
        assert!(r.lines[0].contains("foo()"));
        assert!(r.spans[0].iter().any(|s| s.style == MdStyle::Code));
    }

    #[test]
    fn code_block_indented() {
        let r = render_markdown("```\nfn main() {}\n```");
        let code_line = r.lines.iter().find(|l| l.contains("fn main")).unwrap();
        assert!(code_line.starts_with("    "));
    }

    #[test]
    fn unordered_list() {
        let r = render_markdown("- one\n- two\n- three");
        let bullets: Vec<_> = r.lines.iter().filter(|l| l.contains('•')).collect();
        assert_eq!(bullets.len(), 3);
    }

    #[test]
    fn ordered_list() {
        let r = render_markdown("1. first\n2. second");
        assert!(r.lines.iter().any(|l| l.contains("1.")));
        assert!(r.lines.iter().any(|l| l.contains("2.")));
    }

    #[test]
    fn blockquote() {
        let r = render_markdown("> quoted text");
        assert!(r.lines.iter().any(|l| l.contains("│ ")));
    }

    #[test]
    fn horizontal_rule() {
        let r = render_markdown("---");
        assert!(r.lines.iter().any(|l| l.contains("────")));
    }

    #[test]
    fn link_shows_url() {
        let r = render_markdown("[click](https://example.com)");
        assert!(r.lines[0].contains("click"));
        assert!(r.lines[0].contains("https://example.com"));
        assert!(r.spans[0].iter().any(|s| s.style == MdStyle::LinkUrl));
    }

    #[test]
    fn image_alt_text() {
        let r = render_markdown("![logo](img.png)");
        assert!(r.lines.iter().any(|l| l.contains("[Image: logo]")));
    }

    #[test]
    fn multiple_headings() {
        let r = render_markdown("# H1\n## H2\n### H3");
        assert!(r
            .spans
            .iter()
            .flatten()
            .any(|s| s.style == MdStyle::Heading(1)));
        assert!(r
            .spans
            .iter()
            .flatten()
            .any(|s| s.style == MdStyle::Heading(2)));
        assert!(r
            .spans
            .iter()
            .flatten()
            .any(|s| s.style == MdStyle::Heading(3)));
    }

    #[test]
    fn bold_italic_combined() {
        let r = render_markdown("***both***");
        assert!(r.spans[0].iter().any(|s| s.style == MdStyle::BoldItalic));
    }

    #[test]
    fn empty_input() {
        let r = render_markdown("");
        assert!(r.lines.is_empty());
    }

    #[test]
    fn plain_text_no_spans() {
        let r = render_markdown("Just some text");
        assert!(r.lines[0].contains("Just some text"));
        // Plain text should have no special spans.
        assert!(r.spans[0].is_empty());
    }

    #[test]
    fn nested_list() {
        // pulldown-cmark needs a proper nested list with blank line or correct indent.
        let r = render_markdown("- outer\n\n  - inner");
        // Both lines should be present.
        assert!(
            r.lines.iter().any(|l| l.contains("outer")),
            "missing 'outer' in: {:?}",
            r.lines
        );
        assert!(
            r.lines.iter().any(|l| l.contains("inner")),
            "missing 'inner' in: {:?}",
            r.lines
        );
    }

    #[test]
    fn code_block_syntax_highlights() {
        let r = render_markdown("```rust\nfn main() { let x = 42; }\n```");
        // Should have at least one code block line.
        let code_line_idx = r
            .lines
            .iter()
            .position(|l| l.contains("fn main"))
            .expect("expected code line");
        // Tree-sitter should produce highlights for Rust code.
        assert!(
            !r.code_highlights[code_line_idx].is_empty(),
            "expected syntax highlights for Rust code block, got none"
        );
        // Check that a "keyword" scope exists (for `fn` or `let`).
        assert!(
            r.code_highlights[code_line_idx]
                .iter()
                .any(|h| h.scope == "keyword"),
            "expected 'keyword' scope in highlights: {:?}",
            r.code_highlights[code_line_idx]
        );
    }

    #[test]
    fn code_block_unknown_lang_no_highlights() {
        let r = render_markdown("```unknownlang\nsome code here\n```");
        // Unknown language should have no code highlights.
        for hl in &r.code_highlights {
            assert!(hl.is_empty());
        }
    }

    #[test]
    fn code_highlights_parallel_to_lines() {
        let r = render_markdown("Hello\n\n```rust\nlet x = 1;\n```\n\nWorld");
        assert_eq!(
            r.lines.len(),
            r.code_highlights.len(),
            "code_highlights length must match lines length"
        );
    }
}
