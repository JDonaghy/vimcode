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

/// Scan a plain-text chunk for bare `http://` / `https://` URLs and push
/// `MdStyle::LinkUrl` spans into `spans`.
///
/// * `chunk_bytes` — the raw bytes of the text chunk (UTF-8).
/// * `line_offset` — byte position where this chunk starts inside `cur_line`.
/// * `spans`       — destination span list for the current line.
///
/// No regex crate is used: we scan byte-by-byte for the scheme prefix, walk
/// forward to the next ASCII whitespace, then strip trailing punctuation
/// (`.`, `,`, `)`).
fn scan_bare_urls(chunk_bytes: &[u8], line_offset: usize, spans: &mut Vec<MdSpan>) {
    let len = chunk_bytes.len();
    let mut i = 0usize;
    while i < len {
        // Detect scheme prefix.
        let scheme_len = if chunk_bytes[i..].starts_with(b"https://") {
            8
        } else if chunk_bytes[i..].starts_with(b"http://") {
            7
        } else {
            i += 1;
            continue;
        };

        let url_start = i;

        // Walk to next ASCII whitespace.
        let mut j = i + scheme_len;
        while j < len && !chunk_bytes[j].is_ascii_whitespace() {
            j += 1;
        }

        // Strip trailing punctuation characters.
        while j > url_start + scheme_len {
            match chunk_bytes[j - 1] {
                b'.' | b',' | b')' => j -= 1,
                _ => break,
            }
        }

        if j > url_start + scheme_len {
            spans.push(MdSpan {
                start_byte: line_offset + url_start,
                end_byte: line_offset + j,
                style: MdStyle::LinkUrl,
            });
        }

        // Advance past the URL (or at least by 1 to avoid an infinite loop).
        i = j.max(url_start + 1);
    }
}

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
                    // For command: URIs, display as "(:Name?args)" instead.
                    if let Some(url) = link_url.take() {
                        if !url.is_empty() {
                            let display_url = if let Some(rest) = url.strip_prefix("command:") {
                                format!(":{}", rest)
                            } else {
                                url.clone()
                            };
                            let prefix = " (";
                            cur_line.push_str(prefix);
                            let url_start = cur_line.len();
                            cur_line.push_str(&display_url);
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
                            // Plain text: scan for bare http/https URLs.
                            scan_bare_urls(chunk.as_bytes(), start, &mut cur_spans);
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

// ─── Hover-popup markdown (quadraui-backed, #821) ────────────────────────────
//
// `render_markdown` above (and the `MdRendered`/`MdSpan`/`MdStyle` types it
// returns) remain the engine's markdown pipeline for the editor-buffer inline
// highlighter (`src/render.rs`'s `md_inline_spans` — compensates for no
// tree-sitter markdown injection, out of scope for #821) and for markdown
// *preview* buffers (`Engine::open_markdown_preview*`). Neither of those
// consumes styled colour, so they stay on the local pulldown-cmark pipeline.
//
// The **hover-popup** path (`EditorHoverPopup` / `PanelHoverPopup`) instead
// adopts `quadraui::compose::markdown::render_markdown_to_styled` — the
// shared, cross-backend markdown renderer quadraui ships (quadraui#262) —
// rather than hand-rolling the markdown-style-enum-to-color span walk vimcode
// used to do in `render.rs` (`hover_line_to_styled_text`,
// `markdown_rendered_to_quadraui_lines`, now removed). Structure (plain line
// text, link ranges) is theme-independent, so it's computed once here, at
// hover-show time; the *styled* spans (which need the live `render::Theme`,
// not available in `core::`) are recomputed at paint time in `render.rs` from
// the same markdown string.
//
// One real feature gap vs. the local `render_markdown`: quadraui's parser
// only recognizes `[text](url)` links, not bare `http://`/`https://`
// autolinks (verified against the pinned rev — see `linkify_bare_urls`
// below, which is the compatibility shim for that gap; a proper upstream
// fix should still be filed against quadraui). Tree-sitter code-block syntax
// highlighting is also quadraui-agnostic by design (its own doc: "Tree-sitter-
// capable callers opt into per-language highlighting" via the `code_blocks`
// side-channel) — `hover_markdown_structure` below does exactly that,
// reusing the same `MdCodeHighlight`/`Syntax` machinery `render_markdown`
// uses for markdown-preview code blocks. Distinct heading colors
// (`Theme::md_heading1/2/3`) are *not* preserved — quadraui renders every
// heading level as bold + a larger `line_scales` factor, with no per-level
// color; this is an accepted, documented divergence, not a bug.

/// Wrap bare `http://` / `https://` URLs in `[url](url)` markdown link
/// syntax so quadraui's `render_markdown_to_styled` (which only recognizes
/// bracketed links) still makes them clickable. A URL immediately preceded
/// by `(` is assumed to already be a link destination (`[text](url)`) and is
/// left alone. Fenced code blocks are skipped entirely (their contents
/// should never be rewritten). Adapted from `scan_bare_urls` above, which
/// performs the equivalent scan for the local pulldown-cmark pipeline.
pub fn linkify_bare_urls(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_code_block = false;
    for (i, line) in input.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            out.push_str(line);
            continue;
        }
        if in_code_block {
            out.push_str(line);
            continue;
        }
        linkify_bare_urls_in_line(line, &mut out);
    }
    out
}

fn linkify_bare_urls_in_line(line: &str, out: &mut String) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    while i < len {
        let scheme_len = if bytes[i..].starts_with(b"https://") {
            Some(8usize)
        } else if bytes[i..].starts_with(b"http://") {
            Some(7usize)
        } else {
            None
        };
        let Some(scheme_len) = scheme_len else {
            let ch = line[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        };
        if i > 0 && bytes[i - 1] == b'(' {
            // Already a markdown link destination — leave untouched.
            let ch = line[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        let mut j = i + scheme_len;
        while j < len && !bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        while j > start + scheme_len {
            match bytes[j - 1] {
                b'.' | b',' | b')' => j -= 1,
                _ => break,
            }
        }
        let url = &line[start..j];
        out.push('[');
        out.push_str(url);
        out.push_str("](");
        out.push_str(url);
        out.push(')');
        i = j;
    }
}

/// `(line_text, links, code_highlights)` — see [`hover_markdown_structure`]'s
/// doc for what each element means.
pub type HoverMarkdownStructure = (
    Vec<String>,
    Vec<(usize, usize, usize, String)>,
    Vec<Vec<MdCodeHighlight>>,
);

/// The theme-independent half of hover-popup markdown rendering: plain
/// per-line text, validated+dispatch-ready link ranges, and per-line
/// tree-sitter code-block highlights. Computed once when a hover popup is
/// shown (`Engine::show_editor_hover` / `show_panel_hover`); the styled
/// (theme-colored) `quadraui::StyledText` spans are computed separately, at
/// paint time, in `render.rs`.
///
/// `markdown` should already have been passed through [`linkify_bare_urls`].
///
/// Returns `(line_text, links, code_highlights)`:
/// - `line_text[i]` — plain text of rendered line `i` (markdown syntax
///   stripped), used for scroll-bound math, clipboard copy, and selection
///   extraction.
/// - `links` — `(line_idx, start_byte, end_byte, url)`, byte ranges into
///   `line_text[line_idx]`; only `is_safe_url`-passing schemes are kept.
/// - `code_highlights[i]` — tree-sitter highlight spans for fenced
///   code-block line `i` (empty for non-code-block lines or unrecognized
///   languages). Byte offsets are relative to that line's own code text
///   (no code-rail prefix) — see `render.rs`'s `overlay_code_highlights`,
///   which applies these directly to the *last* span of a code-block
///   `StyledText` line (always the raw code span; the two before it are the
///   code-rail indent + bar, per quadraui's `render_code_content`).
pub fn hover_markdown_structure(markdown: &str) -> HoverMarkdownStructure {
    let rendered = quadraui::compose::markdown::render_markdown_to_styled(
        markdown,
        &quadraui::Theme::default(),
    );

    let links = rendered
        .links
        .iter()
        .filter_map(|(line_idx, range, url)| {
            // Command URIs are never displayed in shortened ":Name?args" form
            // by any current producer, but restore the prefix defensively —
            // matches the pre-#821 `extract_hover_links` behaviour.
            let url = match url.strip_prefix(':') {
                Some(rest) => format!("command:{rest}"),
                None => url.clone(),
            };
            crate::core::engine::is_safe_url(&url).then_some((
                *line_idx,
                range.start,
                range.end,
                url,
            ))
        })
        .collect();

    let code_highlights = hover_code_highlights(&rendered);

    (rendered.line_text, links, code_highlights)
}

/// Tree-sitter-highlight every fenced code block in `rendered` (via
/// `rendered.code_blocks`), producing per-line highlight spans keyed to each
/// code-content line's *own* raw text (i.e. the last span of that line's
/// `StyledText` — see `hover_markdown_structure`'s doc).
fn hover_code_highlights(
    rendered: &quadraui::compose::markdown::RenderedMarkdown,
) -> Vec<Vec<MdCodeHighlight>> {
    let mut out: Vec<Vec<MdCodeHighlight>> = vec![Vec::new(); rendered.lines.len()];

    for cb in &rendered.code_blocks {
        let Some(lang) = cb.lang.as_deref().and_then(SyntaxLanguage::from_name) else {
            continue;
        };
        let content_start = cb.fence_open + 1;
        let content_end = cb.fence_close.unwrap_or(rendered.lines.len());
        if content_start >= content_end {
            continue;
        }

        let raw_lines: Vec<&str> = (content_start..content_end)
            .map(|i| {
                rendered
                    .lines
                    .get(i)
                    .and_then(|st| st.spans.last())
                    .map(|s| s.text.as_str())
                    .unwrap_or("")
            })
            .collect();
        let code_text = raw_lines.join("\n");

        let mut syntax = Syntax::new_for_language(lang);
        let highlights = syntax.parse(&code_text);

        let mut line_byte_starts = Vec::with_capacity(raw_lines.len());
        let mut offset = 0usize;
        for raw in &raw_lines {
            line_byte_starts.push(offset);
            offset += raw.len() + 1; // +1 for the joining '\n'
        }

        for (start, end, scope) in &highlights {
            let raw_line_idx = match line_byte_starts.binary_search(start) {
                Ok(i) => i,
                Err(i) => i.saturating_sub(1),
            };
            let out_line_idx = content_start + raw_line_idx;
            if out_line_idx >= out.len() {
                continue;
            }
            let line_start = line_byte_starts[raw_line_idx];
            let local_start = start.saturating_sub(line_start);
            let local_end = (*end - line_start).min(raw_lines[raw_line_idx].len());
            if local_start >= local_end {
                continue;
            }
            out[out_line_idx].push(MdCodeHighlight {
                start_byte: local_start,
                end_byte: local_end,
                scope: scope.clone(),
            });
        }
    }

    out
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

    // ─── Hover-popup markdown (quadraui-backed, #821) ───────────────────

    #[test]
    fn linkify_bare_urls_wraps_a_bare_url() {
        let out = linkify_bare_urls("See https://example.com for details");
        assert_eq!(
            out,
            "See [https://example.com](https://example.com) for details"
        );
    }

    #[test]
    fn linkify_bare_urls_leaves_existing_markdown_links_alone() {
        let out = linkify_bare_urls("see [label](https://example.com) here");
        assert_eq!(out, "see [label](https://example.com) here");
    }

    #[test]
    fn linkify_bare_urls_skips_fenced_code_blocks() {
        let out = linkify_bare_urls("```\nsee https://example.com\n```");
        assert_eq!(out, "```\nsee https://example.com\n```");
    }

    #[test]
    fn linkify_bare_urls_strips_trailing_punctuation() {
        let out = linkify_bare_urls("visit https://example.com, please.");
        assert_eq!(
            out,
            "visit [https://example.com](https://example.com), please."
        );
    }

    #[test]
    fn hover_markdown_structure_extracts_bold_code_and_link() {
        let (line_text, links, _) =
            hover_markdown_structure("**bold** and `code` and [label](https://example.com)");
        assert_eq!(line_text.len(), 1);
        assert_eq!(line_text[0], "bold and code and label");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].3, "https://example.com");
        let (_, s, e) = (links[0].0, links[0].1, links[0].2);
        assert_eq!(&line_text[0][s..e], "label");
    }

    #[test]
    fn hover_markdown_structure_extracts_bare_url_after_linkify() {
        let markdown = linkify_bare_urls("See https://example.com for details");
        let (line_text, links, _) = hover_markdown_structure(&markdown);
        assert_eq!(line_text[0], "See https://example.com for details");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].3, "https://example.com");
    }

    #[test]
    fn hover_markdown_structure_rejects_unsafe_url_scheme() {
        let (_, links, _) = hover_markdown_structure("[click](javascript:alert(1))");
        assert!(
            links.is_empty(),
            "javascript: URLs must not become clickable links"
        );
    }

    #[test]
    fn hover_markdown_structure_highlights_fenced_code_block() {
        let (line_text, _, code_highlights) =
            hover_markdown_structure("```rust\nfn main() { let x = 42; }\n```");
        let code_line_idx = line_text
            .iter()
            .position(|l| l.contains("fn main"))
            .expect("expected code line");
        assert!(
            !code_highlights[code_line_idx].is_empty(),
            "expected tree-sitter highlights for Rust code block, got none"
        );
        assert!(
            code_highlights[code_line_idx]
                .iter()
                .any(|h| h.scope == "keyword"),
            "expected 'keyword' scope in highlights: {:?}",
            code_highlights[code_line_idx]
        );
    }
}
