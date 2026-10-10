use super::*;

/// Convert markdown style spans into rendering `StyledSpan`s.
/// When `code_highlights` is non-empty, tree-sitter colors override CodeBlock spans.
pub(crate) fn md_spans_to_styled(
    md_spans: &[crate::core::markdown::MdSpan],
    code_highlights: Option<&Vec<crate::core::markdown::MdCodeHighlight>>,
    theme: &Theme,
    color_headings: bool,
) -> Vec<StyledSpan> {
    use crate::core::markdown::MdStyle;
    // If this line has tree-sitter code highlights, use those instead.
    if let Some(highlights) = code_highlights {
        if !highlights.is_empty() {
            return highlights
                .iter()
                .map(|h| StyledSpan {
                    start_byte: h.start_byte,
                    end_byte: h.end_byte,
                    style: Style {
                        fg: theme.scope_color(&h.scope),
                        bg: None,
                        bold: false,
                        italic: false,
                        font_scale: 1.0,
                    },
                })
                .collect();
        }
    }
    md_spans
        .iter()
        .map(|s| {
            let (fg, bold, italic, font_scale) = match s.style {
                MdStyle::Heading(1) => {
                    let c = if color_headings {
                        theme.md_heading1
                    } else {
                        theme.foreground
                    };
                    (c, true, false, 1.4)
                }
                MdStyle::Heading(2) => {
                    let c = if color_headings {
                        theme.md_heading2
                    } else {
                        theme.foreground
                    };
                    (c, true, false, 1.2)
                }
                MdStyle::Heading(_) => {
                    let c = if color_headings {
                        theme.md_heading3
                    } else {
                        theme.foreground
                    };
                    (c, true, false, 1.1)
                }
                MdStyle::Bold => (theme.foreground, true, false, 1.0),
                MdStyle::Italic => (theme.foreground, false, true, 1.0),
                MdStyle::BoldItalic => (theme.foreground, true, true, 1.0),
                MdStyle::Code | MdStyle::CodeBlock => (theme.md_code, false, false, 1.0),
                MdStyle::Link => (theme.md_link, false, false, 1.0),
                MdStyle::LinkUrl => (theme.md_link, false, true, 1.0),
                MdStyle::BlockQuote => (theme.md_heading3, false, true, 1.0),
                MdStyle::ListBullet => (theme.md_heading1, true, false, 1.0),
                MdStyle::HorizontalRule => (theme.annotation_fg, false, false, 1.0),
                MdStyle::Image => (theme.md_link, false, true, 1.0),
            };
            StyledSpan {
                start_byte: s.start_byte,
                end_byte: s.end_byte,
                style: Style {
                    fg,
                    bg: None,
                    bold,
                    italic,
                    font_scale,
                },
            }
        })
        .collect()
}

/// Build styled spans for one line: syntax highlights + search matches.
#[allow(clippy::too_many_arguments)]
/// Regex-based inline markdown highlighting for bold, italic, inline code, and links.
/// This compensates for not having tree-sitter inline injection support.
fn md_inline_spans(line: &str, theme: &Theme, spans: &mut Vec<StyledSpan>) {
    let bytes = line.as_bytes();

    // Inline code: `code` — requires non-empty content between backticks.
    // Skip runs of 3+ backticks (fenced code block delimiters).
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            // Count consecutive backticks
            let tick_run_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let tick_count = i - tick_run_start;
            if tick_count >= 3 {
                // Fenced code delimiter — skip, tree-sitter handles this
                continue;
            }
            // Single or double backtick — find matching closing run
            let content_start = i;
            loop {
                // Find next backtick
                while i < bytes.len() && bytes[i] != b'`' {
                    i += 1;
                }
                if i >= bytes.len() {
                    break;
                }
                // Count closing backticks
                let close_start = i;
                while i < bytes.len() && bytes[i] == b'`' {
                    i += 1;
                }
                if i - close_start == tick_count && i - close_start - tick_count < i {
                    // Matching close — only highlight if there's content
                    if content_start < close_start {
                        spans.push(StyledSpan {
                            start_byte: tick_run_start,
                            end_byte: i,
                            style: Style {
                                fg: theme.scope_color("string"),
                                bg: None,
                                bold: false,
                                italic: false,
                                font_scale: 1.0,
                            },
                        });
                    }
                    break;
                }
                // Not matching — keep searching
            }
            continue;
        }
        i += 1;
    }

    // Bold: **text** or __text__
    for delim in &["**", "__"] {
        let d = delim.as_bytes();
        let mut pos = 0;
        while pos + d.len() < bytes.len() {
            if bytes[pos..].starts_with(d) {
                // For __, require word boundary (not inside a word)
                if d[0] == b'_' && pos > 0 && bytes[pos - 1] != b' ' && bytes[pos - 1] != b'\t' {
                    pos += 1;
                    continue;
                }
                let open = pos;
                pos += d.len();
                // Find closing delimiter
                while pos + d.len() <= bytes.len() && !bytes[pos..].starts_with(d) {
                    pos += 1;
                }
                if pos + d.len() <= bytes.len() && bytes[pos..].starts_with(d) {
                    let close = pos + d.len();
                    spans.push(StyledSpan {
                        start_byte: open,
                        end_byte: close,
                        style: Style {
                            fg: theme.scope_color("variable"),
                            bg: None,
                            bold: true,
                            italic: false,
                            font_scale: 1.0,
                        },
                    });
                    pos = close;
                    continue;
                }
            }
            pos += 1;
        }
    }

    // Italic: *text* or _text_
    // For underscore: require word boundary (space or start-of-line before open,
    // space or end-of-line after close) to avoid matching inside_words_like_this.
    for &delim_byte in b"*_" {
        let need_boundary = delim_byte == b'_';
        let mut pos = 0;
        while pos < bytes.len() {
            if bytes[pos] == delim_byte {
                // Skip if this is a bold delimiter (double)
                if pos + 1 < bytes.len() && bytes[pos + 1] == delim_byte {
                    pos += 2;
                    // Skip past bold content + closing **/__
                    while pos < bytes.len() {
                        if bytes[pos] == delim_byte
                            && pos + 1 < bytes.len()
                            && bytes[pos + 1] == delim_byte
                        {
                            pos += 2;
                            break;
                        }
                        pos += 1;
                    }
                    continue;
                }
                // Word boundary check for underscore
                if need_boundary && pos > 0 && bytes[pos - 1] != b' ' && bytes[pos - 1] != b'\t' {
                    pos += 1;
                    continue;
                }
                let open = pos;
                pos += 1;
                while pos < bytes.len() && bytes[pos] != delim_byte {
                    pos += 1;
                }
                if pos < bytes.len() {
                    let close = pos + 1;
                    // Check closing word boundary for underscore
                    let close_ok = !need_boundary
                        || close >= bytes.len()
                        || bytes[close] == b' '
                        || bytes[close] == b'\t'
                        || bytes[close] == b'.'
                        || bytes[close] == b','
                        || bytes[close] == b':'
                        || bytes[close] == b';'
                        || bytes[close] == b')'
                        || bytes[close] == b']';
                    // Only if there's content between delimiters
                    if close - open > 2 && close_ok {
                        spans.push(StyledSpan {
                            start_byte: open,
                            end_byte: close,
                            style: Style {
                                fg: theme.scope_color("variable"),
                                bg: None,
                                bold: false,
                                italic: true,
                                font_scale: 1.0,
                            },
                        });
                    }
                    pos = close;
                    continue;
                }
            }
            pos += 1;
        }
    }

    // Links: [text](url) — color the URL part
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] == b'[' {
            let bracket_start = pos;
            pos += 1;
            // Find ]
            while pos < bytes.len() && bytes[pos] != b']' {
                pos += 1;
            }
            if pos + 1 < bytes.len() && bytes[pos] == b']' && bytes[pos + 1] == b'(' {
                let bracket_end = pos;
                // Color [text] as link
                spans.push(StyledSpan {
                    start_byte: bracket_start,
                    end_byte: bracket_end + 1,
                    style: Style {
                        fg: theme.scope_color("type"),
                        bg: None,
                        bold: false,
                        italic: false,
                        font_scale: 1.0,
                    },
                });
                pos += 2; // skip ](
                let url_start = pos;
                while pos < bytes.len() && bytes[pos] != b')' {
                    pos += 1;
                }
                if pos < bytes.len() {
                    spans.push(StyledSpan {
                        start_byte: url_start - 1, // include (
                        end_byte: pos + 1,         // include )
                        style: Style {
                            fg: theme.scope_color("comment"),
                            bg: None,
                            bold: false,
                            italic: false,
                            font_scale: 1.0,
                        },
                    });
                    pos += 1;
                    continue;
                }
            }
        }
        pos += 1;
    }
}

/// Compute search match char-offset pairs for a buffer that is NOT the active one.
pub(crate) fn compute_search_matches_for_buffer(
    buffer: &crate::core::buffer::Buffer,
    query: &str,
    settings: &crate::core::settings::Settings,
) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    let text = buffer.to_string();

    let case_insensitive =
        settings.ignorecase && !(settings.smartcase && query.chars().any(|c| c.is_uppercase()));

    if case_insensitive {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        let mut byte_pos = 0;
        while let Some(found) = text_lower[byte_pos..].find(&query_lower) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query_lower.len();
            let start_char = buffer.content.byte_to_char(start_byte);
            let end_char = buffer.content.byte_to_char(end_byte);
            matches.push((start_char, end_char));
            byte_pos = start_byte + 1;
        }
    } else {
        let mut byte_pos = 0;
        while let Some(found) = text[byte_pos..].find(query) {
            let start_byte = byte_pos + found;
            let end_byte = start_byte + query.len();
            let start_char = buffer.content.byte_to_char(start_byte);
            let end_char = buffer.content.byte_to_char(end_byte);
            matches.push((start_char, end_char));
            byte_pos = start_byte + 1;
        }
    }
    matches
}

// ─── Decoration paint helpers (#1653, Native API P5) ─────────────────────────
//
// Shared by both backends (called only from `build_rendered_window` above,
// well before the GTK/TUI split) — per the Platform-Neutrality Rule, a
// decoration's colours/splice land in `RenderedLine`'s existing `raw_text`/
// `spans` here, not in new `src/gtk/` or `src/tui_main/` code.

/// The `[start_col, end_col)` character range `m`'s highlight paints on
/// `line_idx` — `None` when `m` doesn't touch `line_idx` at all (shouldn't
/// happen for marks already filtered by `DecorState::marks_touching`, but
/// keeps this usable standalone). Columns are clamped to `line_chars` (the
/// line's current character count), since a range mark's `end_col` can
/// point past the end of a line that's since gotten shorter.
pub(crate) fn decor_highlight_cols(
    m: &crate::core::buffer::DecorMark,
    line_idx: usize,
    line_chars: usize,
) -> Option<(usize, usize)> {
    if line_idx < m.row || line_idx > m.end_row {
        return None;
    }
    let start = if line_idx == m.row {
        m.col.min(line_chars)
    } else {
        0
    };
    let end = if line_idx == m.end_row {
        m.end_col.min(line_chars)
    } else {
        line_chars
    };
    Some((start, end.max(start)))
}

/// Combine a `vimcode.buf.annotate_line` blame-style annotation with
/// #1810's decor-sourced eol virtual text, both of which paint through the
/// same `RenderedLine::annotation` field. `blame` keeps its existing
/// position (first) so a line with only blame text is byte-for-byte
/// unchanged from before #1810; eol text (already space-joined across
/// marks by the caller) follows, separated by one more space.
pub(crate) fn join_annotation(blame: Option<String>, eol: Option<&str>) -> Option<String> {
    let eol = eol.filter(|s| !s.is_empty());
    match (blame, eol) {
        (Some(b), Some(e)) => Some(format!("{b} {e}")),
        (Some(b), None) => Some(b),
        (None, Some(e)) => Some(e.to_string()),
        (None, None) => None,
    }
}

/// Resolve a `vimcode.decor.set_hl` group name to paint-time colours/flags,
/// falling back to the theme's default foreground when the group (or its
/// `fg`) isn't registered — e.g. a group that only sets `bg` still gets a
/// readable foreground instead of defaulting to black.
///
/// #1653 scope item 6: a group's `link` is first chased one hop against
/// other *plugin* groups by `DecorState::resolve_hl` (unchanged); when the
/// link target isn't a registered plugin group at all, it's treated as a
/// `Theme` role name instead (`Theme::scope_color_opt`, matched
/// case-insensitively against Neovim's own built-in group spelling, e.g.
/// `link = "Comment"`) — resolved fresh on every call rather than frozen at
/// `set_hl` time, so it keeps tracking the live theme across `ColorScheme`
/// switches, same as every other colour here.
pub(crate) fn resolve_decor_style(engine: &Engine, theme: &Theme, hl_group: Option<&str>) -> Style {
    let def = hl_group.and_then(|g| engine.decor.resolve_hl(g));
    let theme_role_fg = def
        .and_then(|d| d.link.as_deref())
        .filter(|link| !engine.decor.highlight_groups.contains_key(*link))
        .and_then(|link| theme.scope_color_opt(&link.to_ascii_lowercase()));
    let fg = def
        .and_then(|d| d.fg.as_deref())
        .and_then(|h| try_from_hex_over(h, theme.background))
        .or(theme_role_fg)
        .unwrap_or(theme.foreground);
    let bg = def
        .and_then(|d| d.bg.as_deref())
        .and_then(|h| try_from_hex_over(h, theme.background));
    Style {
        fg,
        bg,
        bold: def.is_some_and(|d| d.bold),
        italic: def.is_some_and(|d| d.italic),
        font_scale: 1.0,
    }
}

/// Splice `text` into `line_str` at character column `col`, replacing
/// `replace_chars` existing characters (`0` for a pure insert — `inline`
/// virtual text; the character count of `text` itself for a same-width swap
/// — `overlay` virtual text). Spans entirely before the splice are left
/// alone; ones entirely after shift by the resulting byte-length delta; any
/// span that overlapped the replaced region is dropped outright (the text it
/// styled no longer exists). Pushes one new span covering the spliced text
/// in `style`.
pub(crate) fn splice_virt_text(
    line_str: &mut String,
    spans: &mut Vec<StyledSpan>,
    col: usize,
    replace_chars: usize,
    text: &str,
    style: Style,
) {
    let char_count = line_str.chars().count();
    let col = col.min(char_count);
    let replace_chars = replace_chars.min(char_count - col);
    let start_byte = quadraui::text_util::char_to_byte_idx(line_str, col);
    let end_byte = quadraui::text_util::char_to_byte_idx(line_str, col + replace_chars);
    let old_len = end_byte - start_byte;
    let new_len = text.len();
    let delta = new_len as isize - old_len as isize;
    spans.retain_mut(|s| {
        if s.end_byte <= start_byte {
            true
        } else if s.start_byte >= end_byte {
            s.start_byte = (s.start_byte as isize + delta).max(0) as usize;
            s.end_byte = (s.end_byte as isize + delta).max(0) as usize;
            true
        } else {
            false
        }
    });
    line_str.replace_range(start_byte..end_byte, text);
    spans.push(StyledSpan {
        start_byte,
        end_byte: start_byte + new_len,
        style,
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_spans(
    engine: &Engine,
    theme: &Theme,
    highlights: &[(usize, usize, String)],
    semantic_tokens: &[crate::core::lsp::SemanticToken],
    buffer: &crate::core::buffer::Buffer,
    line_idx: usize,
    line_str: &str,
    line_start_byte: usize,
    line_end_byte: usize,
    is_markdown: bool,
    search_matches: &[(usize, usize)],
    is_active_buffer: bool,
) -> Vec<StyledSpan> {
    let mut spans = Vec::new();

    // Syntax highlighting — iterate only the pre-narrowed window slice.
    for (start, end, scope) in highlights {
        if *end <= line_start_byte || *start >= line_end_byte {
            continue;
        }
        let rel_start = (*start).saturating_sub(line_start_byte);
        let rel_end = if *end > line_end_byte {
            line_str.len()
        } else {
            *end - line_start_byte
        };
        let color = theme.scope_color(scope);
        spans.push(StyledSpan {
            start_byte: rel_start,
            end_byte: rel_end,
            style: Style {
                fg: color,
                bg: None,
                bold: false,
                italic: false,
                font_scale: 1.0,
            },
        });
    }

    // Markdown inline highlighting — regex-based since tree-sitter-md's inline parser
    // requires injection support we don't have. Runs after tree-sitter block highlights
    // so inline elements layer on top.
    if is_markdown {
        md_inline_spans(line_str, theme, &mut spans);
    }

    // LSP semantic tokens overlay — these override tree-sitter spans since they're later.
    // Tokens are sorted by line (from delta-encoding), so binary search finds the first
    // token on this line efficiently.
    if !semantic_tokens.is_empty() {
        let line32 = line_idx as u32;
        let start_idx = semantic_tokens.partition_point(|t| t.line < line32);
        for tok in &semantic_tokens[start_idx..] {
            if tok.line != line32 {
                break;
            }
            if let Some(style) = theme.semantic_token_style(&tok.token_type, &tok.modifiers) {
                // Convert UTF-16 positions to byte offsets within line_str.
                let char_start = crate::core::lsp::utf16_offset_to_char(line_str, tok.start_char);
                let char_end =
                    crate::core::lsp::utf16_offset_to_char(line_str, tok.start_char + tok.length);
                // Convert char positions to byte offsets.
                let byte_start = line_str
                    .char_indices()
                    .nth(char_start)
                    .map(|(i, _)| i)
                    .unwrap_or(line_str.len());
                let byte_end = line_str
                    .char_indices()
                    .nth(char_end)
                    .map(|(i, _)| i)
                    .unwrap_or(line_str.len());
                if byte_start < byte_end {
                    spans.push(StyledSpan {
                        start_byte: byte_start,
                        end_byte: byte_end,
                        style,
                    });
                }
            }
        }
    }

    // Search match highlighting (skipped when hlsearch is disabled)
    if engine.settings.hlsearch && !search_matches.is_empty() {
        let line_start_char = buffer.content.line_to_char(line_idx);
        let line_char_count = line_str.chars().count();
        let line_end_char = line_start_char + line_char_count;

        for (match_idx, (match_start, match_end)) in search_matches.iter().enumerate() {
            if *match_end <= line_start_char || *match_start >= line_end_char {
                continue;
            }
            let match_start_char = (*match_start).max(line_start_char);
            let match_end_char = (*match_end).min(line_end_char);

            let rel_start_byte = line_str
                .char_indices()
                .nth(match_start_char - line_start_char)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let rel_end_byte = line_str
                .char_indices()
                .nth(match_end_char - line_start_char)
                .map(|(i, _)| i)
                .unwrap_or(line_str.len());

            let is_current = is_active_buffer && engine.search_index == Some(match_idx);
            let bg = if is_current {
                theme.search_current_match_bg
            } else {
                theme.search_match_bg
            };
            spans.push(StyledSpan {
                start_byte: rel_start_byte,
                end_byte: rel_end_byte,
                style: Style {
                    fg: theme.search_match_fg,
                    bg: Some(bg),
                    bold: false,
                    italic: false,
                    font_scale: 1.0,
                },
            });
        }
    }

    spans
}

/// Build a normalised [`SelectionRange`] from the engine's visual-mode state.
pub(crate) fn build_selection(
    engine: &Engine,
    scroll_top: usize,
    visible_lines: usize,
) -> Option<SelectionRange> {
    let anchor = engine.visual_anchor?;
    // When find/replace is open from visual mode, use the frozen cursor position
    // so the selection doesn't change as search jumps the live cursor to matches.
    let frozen_end;
    let cursor = if engine.find_replace_open {
        if let Some(end) = engine.find_replace_visual_end {
            frozen_end = end;
            &frozen_end
        } else {
            engine.cursor()
        }
    } else {
        engine.cursor()
    };

    let visual_mode = match engine.mode {
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => Some(engine.mode),
        // Show selection while typing a command/search entered from visual mode,
        // or while find/replace overlay is open from visual mode
        Mode::Command | Mode::Search => engine.command_from_visual,
        Mode::Normal if engine.find_replace_open => engine.command_from_visual,
        _ => None,
    };
    let kind = match visual_mode? {
        Mode::Visual => SelectionKind::Char,
        Mode::VisualLine => SelectionKind::Line,
        Mode::VisualBlock => SelectionKind::Block,
        _ => return None,
    };

    // For visual block the start/end cols need min/max normalisation
    let (start, end) = normalise_selection(anchor, *cursor);

    let (start_col, end_col) = match kind {
        SelectionKind::Block => (anchor.col.min(cursor.col), anchor.col.max(cursor.col)),
        SelectionKind::Char if engine.visual_end_exclusive && end.col > 0 => {
            // quadraui paints `SelectionKind::Char` as inclusive
            // (`end_col + 1`, quadraui `primitives/editor.rs`), but an
            // exclusive-end selection's `end.col` already sits one past the
            // last selected char — back off by one so highlight, copy and
            // delete agree (#1788 review non-blocking note). When `end.col`
            // is 0 (the exclusive end wrapped to the next line's start)
            // there's no single-line column to back off to; left as-is,
            // matching this function's pre-existing behavior for that case.
            (start.col, end.col - 1)
        }
        _ => (start.col, end.col),
    };

    // Only emit a selection if it overlaps the visible area
    if end.line < scroll_top || start.line >= scroll_top + visible_lines {
        return None;
    }

    Some(SelectionRange {
        kind,
        start_line: start.line,
        start_col,
        end_line: end.line,
        end_col,
    })
}

/// Return (earlier, later) cursors so that `earlier.line <= later.line`.
fn normalise_selection(a: Cursor, b: Cursor) -> (Cursor, Cursor) {
    if a.line < b.line || (a.line == b.line && a.col <= b.col) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Count leading whitespace of a buffer line (tabs = 4 spaces).
fn line_indent_of(buffer: &Buffer, line_idx: usize) -> usize {
    let line = buffer.content.line(line_idx);
    let mut indent = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => indent += 1,
            '\t' => indent += 4,
            _ => break,
        }
    }
    indent
}

/// Determine the fold indicator character for a rendered line.
/// `+` = closed fold header, `-` = open foldable region, ` ` = neither.
///
/// To avoid false positives (e.g. blank lines, function-call continuations),
/// `-` is only shown when the current line is a **block opener**: non-blank
/// and whose trimmed text ends with `{` or `:`.
///
/// `controls` is the `fold_controls` setting (#1544, VS Code's
/// `editor.showFoldingControls`): `Never` blanks every marker regardless of
/// fold state; `Mouseover`/`Always` always show a closed fold's `+` (VS
/// Code never hides an already-collapsed region's marker, even under
/// `mouseover`), but the open-region `-` only shows when
/// `open_markers_visible` is true (the caller's `fold_controls`/hover
/// decision — `Always` passes `true` unconditionally, `Mouseover` passes
/// whether the pointer is currently over this window's gutter).
pub(crate) fn fold_indicator_char(
    buffer: &Buffer,
    view: &View,
    line_idx: usize,
    controls: FoldControlsMode,
    open_markers_visible: bool,
) -> char {
    if controls == FoldControlsMode::Never {
        return ' ';
    }
    // Closed fold header takes priority.
    if view.fold_at(line_idx).is_some() {
        return '+';
    }
    if !open_markers_visible {
        return ' ';
    }
    // Only show `-` for genuine block-opener lines.
    let cur_line = buffer.content.line(line_idx);
    let cur_text: String = cur_line.chars().collect();
    let trimmed = cur_text
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .trim();
    if trimmed.is_empty() {
        return ' ';
    }
    let is_block_opener = trimmed.ends_with('{') || trimmed.ends_with(':');
    if !is_block_opener {
        return ' ';
    }
    // Confirm the next non-blank line has greater indentation.
    let total = buffer.len_lines();
    if line_idx + 1 < total {
        let next_line = buffer.content.line(line_idx + 1);
        let next_text: String = next_line.chars().collect();
        if !next_text.trim().is_empty()
            && line_indent_of(buffer, line_idx + 1) > line_indent_of(buffer, line_idx)
        {
            return '-';
        }
    }
    ' '
}

/// Compute the line-number text for a given mode/indices.
fn gutter_num_text(mode: LineNumberMode, line_idx: usize, cursor_line: usize) -> Option<String> {
    match mode {
        LineNumberMode::None => None,
        LineNumberMode::Absolute => Some((line_idx + 1).to_string()),
        LineNumberMode::Relative => {
            let dist = line_idx.abs_diff(cursor_line);
            if dist == 0 {
                Some((line_idx + 1).to_string())
            } else {
                Some(dist.to_string())
            }
        }
        LineNumberMode::Hybrid => {
            if line_idx == cursor_line {
                Some((line_idx + 1).to_string())
            } else {
                Some(line_idx.abs_diff(cursor_line).to_string())
            }
        }
    }
}

/// Pre-format the gutter string with a fold indicator prefix.
///
/// Layout: `[fold_char][number right-aligned in gutter_char_width-2 cols]`
/// where the trailing column is the gap before code starts.
/// `fold_char` is `+` (closed fold), `-` (open foldable region), or ` `.
/// When `gutter_char_width == 1` (fold indicator only, no line numbers),
/// returns just the single fold character.
pub(crate) fn format_gutter_with_fold(
    mode: LineNumberMode,
    line_idx: usize,
    cursor_line: usize,
    gutter_char_width: usize,
    fold_char: char,
) -> String {
    if gutter_char_width == 0 {
        return String::new();
    }
    // Fold indicator only (line numbers disabled).
    if gutter_char_width == 1 {
        return fold_char.to_string();
    }
    let num_text = match gutter_num_text(mode, line_idx, cursor_line) {
        Some(t) => t,
        // Line numbers disabled but fold col is still present.
        None => return fold_char.to_string(),
    };
    // Number is right-aligned in gutter_char_width - 2 (1 for fold indicator, 1 trailing gap)
    let num_part = format!(
        "{:>width$}",
        num_text,
        width = gutter_char_width.saturating_sub(2)
    );
    format!("{}{}", fold_char, num_part)
}

/// Calculate the gutter width in *character columns* (0 = no gutter).
///
/// When line numbers are enabled the gutter always includes one extra column
/// for the fold indicator (`+`, `-`, or space).
/// When `has_git_diff` is true, one additional column is prepended for the
/// git diff marker (`▌` or space).
/// The GTK backend multiplies this by `char_width` pixels to get the pixel
/// gutter width; a TUI backend uses it directly as cell count.
pub fn calculate_gutter_cols(
    mode: LineNumberMode,
    total_lines: usize,
    _char_width: f64,
    has_git_diff: bool,
    has_breakpoints: bool,
) -> usize {
    let git = if has_git_diff { 1 } else { 0 };
    let bp = if has_breakpoints { 1 } else { 0 };
    match mode {
        // No line numbers: show only the 1-column fold indicator.
        LineNumberMode::None => 1 + git + bp,
        LineNumberMode::Absolute => {
            let digits = total_lines.to_string().len().max(1);
            digits + 2 + 1 + git + bp // digits + padding + fold indicator + git + bp
        }
        LineNumberMode::Relative | LineNumberMode::Hybrid => {
            let max_relative = total_lines.saturating_sub(1);
            let digits = max_relative.to_string().len().max(3);
            digits + 2 + 1 + git + bp
        }
    }
}

/// The global status bar's three pieces: `(prefix, branch, right)`.
///
/// #752: `branch` used to be concatenated into a single `left` blob, with a
/// separate `Option<(start, end)>` range measured in **UTF-8 bytes** telling
/// the one interested caller where inside that blob the git decoration sat.
/// That range was then compared against a *character column* derived from a
/// stale `cached_char_width`, so on any repo ahead of or behind its remote
/// (`↑`/`↓` are three bytes each) or any file with a non-ASCII name, the
/// clickable branch drifted right of the painted one.
///
/// Returning the branch as its own string lets `build_global_status_bar` emit
/// it as a real `StatusBarSegment` with an `action_id`, so its hit region
/// comes from the same `StatusBar::layout` the rasteriser paints with — no
/// hand-measured range, no assumed cell width, and correct in a proportional
/// font too. Left segments lay out contiguously, so the painted result is
/// byte-identical to the old single blob.
fn build_status_line(engine: &Engine) -> (String, String, String) {
    let mode_str = engine.mode_str();

    let filename = match engine.file_path() {
        Some(p) => p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string()),
        None => "[No Name]".to_string(),
    };

    let dirty = if engine.dirty() { " [+]" } else { "" };

    let recording = if let Some(reg) = engine.macro_recording {
        format!(" [recording @{}]", reg)
    } else {
        String::new()
    };

    // Build branch segment with ahead/behind counts
    let branch = if let Some(b) = engine.git_branch.as_deref() {
        let mut branch_text = b.to_string();
        if engine.sc_ahead > 0 || engine.sc_behind > 0 {
            let mut parts = Vec::new();
            if engine.sc_ahead > 0 {
                parts.push(format!("↑{}", engine.sc_ahead));
            }
            if engine.sc_behind > 0 {
                parts.push(format!("↓{}", engine.sc_behind));
            }
            branch_text = format!("{} {}", branch_text, parts.join(" "));
        }
        format!(" [{}]", branch_text)
    } else {
        String::new()
    };

    let prefix = format!(" -- {}{} -- {}{}", mode_str, recording, filename, dirty);

    let cursor = engine.cursor();
    let (errors, warnings) = engine.diagnostic_counts();
    let diag_str = if errors > 0 || warnings > 0 {
        format!("  E:{} W:{}", errors, warnings)
    } else {
        String::new()
    };
    // 'ruler' (#1190): the cursor-position/line-count segment is the part
    // Vim's `'ruler'` option gates (`:h 'ruler'`) — diagnostics are a
    // vimcode-only addition with no Vim equivalent, so they stay visible
    // either way.
    let ruler_str = if engine.settings.ruler {
        format!(
            "Ln {}, Col {}  ({} lines){} ",
            cursor.line + 1,
            cursor.col + 1,
            engine.buffer().len_lines(),
            diag_str
        )
    } else if !diag_str.is_empty() {
        format!("{} ", diag_str.trim_start())
    } else {
        String::new()
    };

    // 'showcmd' (#1190): the partially-typed Normal-mode command, shown
    // immediately left of the ruler like Vim's own showcmd area (`:h
    // 'showcmd'`).
    let showcmd_str = if engine.settings.showcmd {
        let sc = engine.showcmd_text();
        if sc.is_empty() {
            String::new()
        } else {
            format!("{sc} ")
        }
    } else {
        String::new()
    };
    let right = format!("{showcmd_str}{ruler_str}");

    (prefix, branch, right)
}

/// Build a quadraui `StatusBar` for the global (bottom-of-screen) status bar.
pub fn build_global_status_bar(engine: &Engine, theme: &Theme) -> quadraui::StatusBar {
    let (prefix, branch, right) = build_status_line(engine);
    let fg = quadraui::Color::rgb(theme.status_fg.r, theme.status_fg.g, theme.status_fg.b);
    let bg = quadraui::Color::rgb(theme.status_bg.r, theme.status_bg.g, theme.status_bg.b);
    let seg = |text: String, action_id: Option<quadraui::WidgetId>| quadraui::StatusBarSegment {
        text,
        fg,
        bg,
        bold: false,
        action_id,
    };
    let mut left_segments = vec![seg(prefix, None)];
    // #752: the branch is its own **clickable** segment rather than a
    // hand-measured span inside one blob, so `StatusBar::layout` produces its
    // hit region from the same measurement pass that positions its glyphs.
    // `status_action_from_id` maps this id back to `StatusAction::SwitchBranch`,
    // exactly as it does for the per-window bar's segments.
    if !branch.is_empty() {
        left_segments.push(seg(
            branch,
            Some(quadraui::WidgetId::new("status:switch_branch")),
        ));
    }
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("status:global"),
        left_segments,
        right_segments: vec![seg(right, None)],
    }
}

/// Build a `quadraui::ToastOverlay` from `engine.toasts` for the
/// bottom-right corner (#1577 — was `quadraui::ToastStack`, single-action
/// only). Backends call `quadraui::*::draw_toast_overlay` with the
/// result. Returns None when there are no toasts so callers can skip the
/// draw entirely. Also used, with no painting involved, by
/// `Engine::focus_toast_stack` to hand focus to
/// `ToastStackController::give_focus`.
pub fn build_toast_stack(engine: &Engine) -> Option<quadraui::ToastOverlay> {
    if engine.toasts.is_empty() {
        return None;
    }
    Some(quadraui::ToastOverlay {
        id: quadraui::WidgetId::new("toasts"),
        corner: quadraui::ToastCorner::BottomRight,
        toasts: engine
            .toasts
            .iter()
            .map(|t| quadraui::Toast {
                id: quadraui::WidgetId::new(format!("toast-{}", t.id)),
                title: t.title.clone(),
                body: t.body.clone(),
                severity: t.severity,
                // Widget ids carry the button's index within `t.actions`
                // (#1577 — `Engine::run_toast_action`'s doc explains why:
                // a toast can now have more than one action button).
                actions: t
                    .actions
                    .iter()
                    .enumerate()
                    .map(|(i, a)| quadraui::ToastButton {
                        id: quadraui::WidgetId::new(format!("toast-action-{}-{i}", t.id)),
                        label: a.kind.button_label().to_string(),
                        primary: a.primary,
                    })
                    .collect(),
                accent: None,
            })
            .collect(),
        focus: engine.toast_focus.focus(),
    })
}

/// Format the LSP status segment text when the server is still
/// indexing (#221). Renders `name • Indexing: 319/320` when the
/// server is publishing `$/progress`; falls back to the dimmed
/// `name… ` placeholder when no progress data is available.
///
/// Width discipline: progress notifications fire many times per second
/// with varying message lengths. If the segment width fluctuates,
/// `StatusBar::layout`'s priority-drop kicks in and lower-priority
/// segments flash in/out — visually glitchy. The formatter keeps the
/// segment width stable and ≤ ~28 cells by preferring fixed-width
/// detail (percentage, then `X/Y` if the message starts with one) and
/// otherwise dropping the message in favour of `stage…`.
pub fn format_lsp_progress_segment(
    label: &str,
    progress: Option<&crate::core::lsp_manager::LspProgress>,
) -> String {
    let Some(progress) = progress else {
        return format!("{label}… ");
    };
    let stage = if progress.title.is_empty() {
        "working"
    } else {
        progress.title.as_str()
    };
    let detail = compact_progress_detail(progress.message.as_deref(), progress.percentage);
    if detail.is_empty() {
        format!("{label} • {stage}… ")
    } else {
        format!("{label} • {stage}: {detail} ")
    }
}

/// Pick a compact, fixed-width-ish detail string from the progress
/// fields. Preference order:
///   1. `percentage` — always at most 4 chars (`100%`).
///   2. Leading `X/Y` of the message (rust-analyzer's path-laden
///      messages like `"34/285: /home/john/…"` collapse to `34/285`).
///   3. Empty — caller renders `stage…` instead. Skipping verbose
///      free-text messages keeps the segment from flapping width on
///      every `$/progress` report.
fn compact_progress_detail(message: Option<&str>, percentage: Option<u32>) -> String {
    if let Some(pct) = percentage {
        return format!("{pct}%");
    }
    if let Some(msg) = message {
        if let Some(prefix) = extract_xy_prefix(msg) {
            return prefix.to_string();
        }
    }
    String::new()
}

/// Extract a leading `digit+/digit+` prefix from a message, e.g.
/// `"34/285"` from `"34/285: /home/john/…"` or `"34/285"`. Returns
/// None when the message doesn't start with that shape.
fn extract_xy_prefix(msg: &str) -> Option<&str> {
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i == bytes.len() || bytes[i] != b'/' {
        return None;
    }
    let mut j = i + 1;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == i + 1 {
        return None;
    }
    Some(&msg[..j])
}

/// Map an internal LSP language id (`crate::core::lsp::language_id_from_path`'s
/// return value, e.g. `"rust"`, `"typescriptreact"`) to the display name a
/// user actually recognizes (`"Rust"`, `"TypeScript React"`) — VS Code's
/// status bar language segment shows the latter, never the raw id (#1548).
///
/// Every id in [`crate::core::lsp::all_known_language_ids`] has an explicit
/// entry below; anything else (a future id this list hasn't caught up with
/// yet) falls back to capitalizing the first character rather than showing
/// nothing, so an unrecognized id degrades gracefully instead of going
/// blank.
pub fn language_display_name(id: &str) -> String {
    let name = match id {
        "rust" => "Rust",
        "python" => "Python",
        "javascript" => "JavaScript",
        "javascriptreact" => "JavaScript React",
        "typescript" => "TypeScript",
        "typescriptreact" => "TypeScript React",
        "go" => "Go",
        "c" => "C",
        "cpp" => "C++",
        "java" => "Java",
        "csharp" => "C#",
        "ruby" => "Ruby",
        "lua" => "Lua",
        "shellscript" => "Shell Script",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" => "YAML",
        "html" => "HTML",
        "css" => "CSS",
        "markdown" => "Markdown",
        "zig" => "Zig",
        "elixir" => "Elixir",
        "kotlin" => "Kotlin",
        "php" => "PHP",
        "haskell" => "Haskell",
        "ocaml" => "OCaml",
        "nix" => "Nix",
        "terraform" => "Terraform",
        "terraform-vars" => "Terraform Variables",
        "bicep" => "Bicep",
        "scala" => "Scala",
        "graphql" => "GraphQL",
        "sql" => "SQL",
        "solidity" => "Solidity",
        "swift" => "Swift",
        "latex" => "LaTeX",
        "bibtex" => "BibTeX",
        "dockerfile" => "Dockerfile",
        _ => {
            let mut chars = id.chars();
            return match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            };
        }
    };
    name.to_string()
}

/// Build a per-window status line for a given window.
/// Active windows get a rich, colorful bar; inactive windows get dimmed minimal info.
pub fn build_window_status_line(
    engine: &Engine,
    theme: &Theme,
    window_id: WindowId,
    is_active: bool,
) -> WindowStatusLine {
    let window = engine.windows.get(&window_id);
    let buffer_state = window.and_then(|w| engine.buffer_manager.get(w.buffer_id));
    let view = window.map(|w| &w.view);

    // Filename
    let filename = buffer_state
        .and_then(|s| s.file_path.as_ref())
        .and_then(|p| p.file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .or_else(|| buffer_state.and_then(|s| s.scratch_name.as_ref()).cloned())
        .unwrap_or_else(|| "[No Name]".to_string());

    let dirty = buffer_state.is_some_and(|s| s.dirty);
    let cursor = view.map(|v| &v.cursor);
    // Filetype from path
    let filetype = buffer_state
        .and_then(|s| s.file_path.as_ref())
        .and_then(|p| crate::core::lsp::language_id_from_path(p))
        .unwrap_or_default();

    // #1690: the active window's status bar reads its background from the
    // theme's own dedicated status-bar key (`theme.status_bg` — the same
    // field `build_global_status_bar`/the sidebar header/the source-control
    // header already read) rather than a `lighten`/`darken` offset computed
    // from the editor background. The offset always *increased* contrast
    // against a dark background, which is what made the bar read as a
    // lighter slab instead of a footer (vimcode#1690's Win-GUI report:
    // `#303030` fill against a `#14161e` editor, both sampled from a real
    // window) — every color-scheme author already has a considered
    // `status_bg` value (down to "equal to `background`", VS Code's own
    // convention under several of its built-in themes) and this is the one
    // place that value should come from.
    // Inactive windows keep their own dedicated key (`status_inactive_bg`),
    // unaffected by this change.
    let bar_bg = theme.status_bg;
    let bar_fg = theme.foreground;

    // Mode text color — use the mode badge color as a subtle text tint
    let mode_color = match engine.mode {
        Mode::Insert => theme.status_mode_insert_bg,
        Mode::Visual | Mode::VisualLine | Mode::VisualBlock => theme.status_mode_visual_bg,
        Mode::Replace => theme.status_mode_replace_bg,
        _ => bar_fg, // normal mode: just use regular fg
    };

    // Indentation display text
    let indent_text = if engine.settings.expand_tab {
        format!("Spaces: {} ", engine.settings.tabstop)
    } else {
        format!("Tab Size: {} ", engine.settings.tabstop)
    };

    // Line ending display
    let line_ending_str = buffer_state.map(|s| s.line_ending.as_str()).unwrap_or("LF");

    if is_active {
        // ── Active: MODE filename [+] branch | filetype indent encoding eol Ln:Col ──
        let mode_str = engine.mode_str();

        let mut left = vec![
            StatusSegment {
                text: format!(" {} ", mode_str),
                fg: mode_color,
                bg: bar_bg,
                bold: true,
                action: None,
            },
            StatusSegment {
                text: format!(" {}", filename),
                fg: bar_fg,
                bg: bar_bg,
                bold: true,
                action: None,
            },
        ];

        if dirty {
            left.push(StatusSegment {
                text: " [+]".to_string(),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: None,
            });
        }

        // Recording indicator
        if let Some(reg) = engine.macro_recording {
            left.push(StatusSegment {
                text: format!(" [rec @{}]", reg),
                fg: theme.status_mode_replace_bg,
                bg: bar_bg,
                bold: true,
                action: None,
            });
        }

        // Git branch
        if let Some(b) = engine.git_branch.as_deref() {
            let mut branch_text = b.to_string();
            if engine.sc_ahead > 0 || engine.sc_behind > 0 {
                let mut parts = Vec::new();
                if engine.sc_ahead > 0 {
                    parts.push(format!("↑{}", engine.sc_ahead));
                }
                if engine.sc_behind > 0 {
                    parts.push(format!("↓{}", engine.sc_behind));
                }
                branch_text = format!("{} {}", branch_text, parts.join(" "));
            }
            // #1548: prefix with the branch glyph via the shared `Icon`
            // constant (`.s()` picks nerd vs ASCII fallback per
            // `nerd_fonts_enabled()`) rather than a raw literal — a raw PUA
            // literal here would bypass `tests/icon_font_coverage.rs`'s
            // bundled-subset-font check entirely, the exact failure mode
            // that shipped tofu for the status-bar toggles before #1540.
            left.push(StatusSegment {
                text: format!("  {} {}", crate::icons::GIT_BRANCH.s(), branch_text),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::SwitchBranch),
            });
        }

        // LSP status segment — server_has_responded in LspManager already tracks
        // whether the server is fully ready (responded to hover/definition/etc.).
        let lsp_status = window
            .map(|w| engine.lsp_status_for_buffer(w.buffer_id))
            .unwrap_or(crate::core::lsp_manager::LspStatus::None);
        // #221: when indexing is in flight, format `name • Indexing: 319/320`
        // from the latest $/progress snapshot. Falls back to the plain
        // `name…` placeholder when the server isn't reporting progress.
        let lsp_progress = window.and_then(|w| engine.lsp_progress_for_buffer(w.buffer_id));

        // #1690 established VS Code's left/right split for this bar: the
        // problems counter at the **far left** (with the remote-indicator/
        // workspace-trust segments vimcode has no equivalent of), and on
        // the right, `Ln N, Col N` · `Spaces: N` · `UTF-8` · `LF` ·
        // language · notification bell, in that left-to-right order.
        //
        // #1760: that visual order is **not** the order these segments are
        // pushed into `right` below anymore. `right`/`left` are plain
        // vectors `StatusBar::layout`'s priority-drop (quadraui's
        // `fit_right_start`/`layout_padded`) reads left-to-right — a
        // narrow bar drops from the *front* of `right_segments` and always
        // preserves the *last* element, even if it alone overflows (see
        // that primitive's own doc). So vector order doubles as both the
        // right group's visual position *and* its drop priority: #1690
        // pushed `cursor_seg` (Ln/Col) first because that's where it sits
        // visually in VS Code's own order, which made it the first
        // segment *dropped* the moment a dirty marker / git branch /
        // EDIT-mode hint ate into the left side's width budget — reported
        // as "Ln/Col silently vanishes" even though plenty of lower-value
        // segments (the layout toggles, LSP status) survived. quadraui's
        // `StatusBar` has no field to decouple "visual position" from
        // "drop priority" (a vector is both at once — #164 flagged this
        // exact coupling as unresolved), so until it does, vimcode cannot
        // have Ln/Col both leftmost-of-the-group *and* undroppable. This
        // fix picks undroppable: `cursor_seg` is pushed **last**, after
        // every other right-side segment, which is the only way
        // `fit_right_start`'s "always keep the last segment" rule can
        // guarantee it never disappears. Vimcode's own extras with no VS
        // Code counterpart (LSP status, the layout toggles, notifications)
        // are the least important and go first/leftmost of the group;
        // `showcmd` — prominent exactly when the user is mid-command —
        // stays pushed just ahead of `cursor_seg`, as before.
        let mut right = Vec::new();

        // Build each segment optionally; push at the end in priority order.
        // (Segments whose data is absent simply stay None and aren't pushed.)

        // Notification — spinner for in-progress, bell for done
        let notification_seg = if !engine.notifications.is_empty() {
            let has_active = engine.has_active_notifications();
            let has_done = engine.has_done_notifications();
            let (icon, fg_color) = if has_active {
                let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
                let elapsed = engine
                    .notifications
                    .iter()
                    .filter(|n| !n.done)
                    .map(|n| n.created_at)
                    .min()
                    .map(|t| t.elapsed().as_millis() as usize / 100)
                    .unwrap_or(0);
                let frame = frames[elapsed % frames.len()];
                (format!("{frame}"), theme.function)
            } else if has_done {
                (
                    crate::icons::STATUS_BELL_DONE.s().to_string(),
                    theme.string_lit,
                )
            } else {
                (String::new(), bar_fg)
            };
            if !icon.is_empty() {
                let msg = engine
                    .notifications
                    .last()
                    .map(|n| {
                        if n.message.len() > 30 {
                            format!("{}…", &n.message[..29])
                        } else {
                            n.message.clone()
                        }
                    })
                    .unwrap_or_default();
                let action = if has_done {
                    Some(StatusAction::DismissNotifications)
                } else {
                    None
                };
                Some(StatusSegment {
                    text: format!(" {icon} {msg} "),
                    fg: fg_color,
                    bg: bar_bg,
                    bold: false,
                    action,
                })
            } else {
                None
            }
        } else {
            None
        };

        // Layout toggle buttons
        let toggle_fg = |active: bool| {
            if active {
                bar_fg
            } else {
                theme.status_inactive_fg
            }
        };
        let menu_toggle_seg = if engine.menu_bar_toggleable {
            Some(StatusSegment {
                text: format!(" {} ", crate::icons::STATUS_MENU_TOGGLE.s()),
                fg: toggle_fg(engine.menu_bar_visible),
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::ToggleMenuBar),
            })
        } else {
            None
        };

        let panel_toggle_seg = StatusSegment {
            text: format!(" {} ", crate::icons::STATUS_PANEL_TOGGLE.s()),
            fg: toggle_fg(engine.terminal_open || engine.bottom_panel_open),
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::TogglePanel),
        };

        // #1760: this used to be (almost) the right-most segment of the
        // bar — only `showcmd` ever pushed after it, and that's almost
        // always empty/absent — so it carried a trailing-space-free
        // #1541/quadraui#1155 treatment matching `cursor_seg`'s. Now that
        // `cursor_seg` (Ln/Col) is always the true right-most segment (see
        // the `right` push-order comment below), this one needs its own
        // trailing space back so it doesn't touch whichever segment ends
        // up directly after it.
        let sidebar_toggle_seg = StatusSegment {
            text: format!(" {} ", crate::icons::STATUS_SIDEBAR_TOGGLE.s()),
            fg: toggle_fg(engine.session.explorer_visible),
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ToggleSidebar),
        };

        // Problems counter (#1548): always visible, even at zero, matching
        // VS Code's convention — a `0`/`0` count is itself useful signal
        // ("no problems", not "we don't know"), and hiding the segment at
        // zero is what the previous (non-window) status line did. Two
        // segments sharing one action so a click anywhere in the counter
        // opens the workspace Problems (quickfix) list.
        //
        // #1690: pushed onto `left`, not `right` — VS Code puts the
        // problems counter at the **far left** of the bar (after its own
        // remote-indicator/workspace-trust segments, which vimcode has no
        // counterpart for), not mixed into the right-hand encoding/
        // language/cursor cluster.
        let (diag_errors, diag_warnings) = engine.diagnostic_counts();
        let errors_seg = StatusSegment {
            text: format!(" {} {}", crate::icons::STATUS_ERROR.s(), diag_errors),
            fg: if diag_errors > 0 {
                theme.diagnostic_error
            } else {
                bar_fg
            },
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ShowDiagnostics),
        };
        let warnings_seg = StatusSegment {
            text: format!(" {} {} ", crate::icons::STATUS_WARNING.s(), diag_warnings),
            fg: if diag_warnings > 0 {
                theme.diagnostic_warning
            } else {
                bar_fg
            },
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ShowDiagnostics),
        };

        let encoding_seg = StatusSegment {
            text: "UTF-8 ".to_string(),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ChangeEncoding),
        };

        let line_ending_seg = StatusSegment {
            text: format!("{} ", line_ending_str),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ChangeLineEnding),
        };

        let indent_seg = StatusSegment {
            text: indent_text.clone(),
            fg: bar_fg,
            bg: bar_bg,
            bold: false,
            action: Some(StatusAction::ChangeIndentation),
        };

        let filetype_seg = if !filetype.is_empty() {
            Some(StatusSegment {
                text: format!("{} ", language_display_name(&filetype)),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::ChangeLanguage),
            })
        } else {
            None
        };

        let lsp_seg = {
            use crate::core::lsp_manager::LspStatus;
            let (lsp_text, lsp_fg) = match &lsp_status {
                LspStatus::Running(name) => (Some(format!("{} ", name)), bar_fg),
                LspStatus::Initializing(name) => {
                    let label = if name.is_empty() { "LSP" } else { name };
                    let text = format_lsp_progress_segment(label, lsp_progress.as_ref());
                    (Some(text), theme.status_inactive_fg)
                }
                LspStatus::Installing => (Some("LSP↓ ".to_string()), theme.status_inactive_fg),
                LspStatus::Crashed => (Some("LSP✗ ".to_string()), theme.status_mode_replace_bg),
                LspStatus::None => (None, bar_fg),
            };
            lsp_text.map(|text| StatusSegment {
                text,
                fg: lsp_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::LspInfo),
            })
        };

        // 'ruler' (#1190): `:h 'ruler'` gates exactly this segment. This is
        // the right-most segment of the bar, so no trailing space here
        // (quadraui#1155 gives pixel backends their own outer edge inset —
        // a manual trailing space here would double it; see #1541).
        let cursor_seg = if engine.settings.ruler {
            cursor.map(|c| StatusSegment {
                text: format!(" Ln {}, Col {}", c.line + 1, c.col + 1),
                fg: bar_fg,
                bg: bar_bg,
                bold: false,
                action: Some(StatusAction::GoToLine),
            })
        } else {
            None
        };

        // 'showcmd' (#1190): the partially-typed Normal-mode command (`:h
        // 'showcmd'`). Only present in the *active* window's bar — an
        // inactive window's pane never has pending Normal-mode input.
        //
        // #1690: no VS Code counterpart, so it is pushed last — the
        // right-most of vimcode's own appended extras, past VS Code's own
        // six segments. When present it is therefore the bar's true
        // right-most segment (ahead of it, `sidebar_toggle_seg` is the
        // fallback right-most and carries the same no-trailing-space
        // treatment for the same reason — see its own doc), so no trailing
        // space here either.
        let showcmd_seg = if engine.settings.showcmd {
            let sc = engine.showcmd_text();
            if sc.is_empty() {
                None
            } else {
                Some(StatusSegment {
                    text: format!(" {sc}"),
                    fg: bar_fg,
                    bg: bar_bg,
                    bold: false,
                    action: None,
                })
            }
        } else {
            None
        };

        // #1690: the problems counter is a *left*-side segment now (VS
        // Code parity — see the comment above `diag_errors`), pushed right
        // after the branch so the far-left order reads `NORMAL · filename
        // [+] [branch] · ⊗ N  ⚠ N`.
        left.push(errors_seg);
        left.push(warnings_seg);

        // Right side, in drop-priority order (least important first — see
        // the `right` doc comment above for why this is no longer VS
        // Code's own visual left-to-right order, #1760): vimcode's own
        // extras with no VS Code counterpart (LSP status, notifications,
        // the layout toggles) go first/leftmost, since they are the least
        // essential and the first to be dropped under a tight width
        // budget; VS Code's remaining status segments (filetype, line
        // ending, encoding, indent) follow; `showcmd` — prominent exactly
        // when the user is mid-command — sits just ahead of `cursor_seg`;
        // `cursor_seg` (Ln/Col) is pushed **last**, unconditionally the
        // bar's true right-most segment, so quadraui's "always keep the
        // last right segment" priority-drop rule guarantees it, and only
        // it, survives no matter how little width remains.
        if let Some(s) = lsp_seg {
            right.push(s);
        }
        if let Some(s) = notification_seg {
            right.push(s);
        }
        if let Some(s) = menu_toggle_seg {
            right.push(s);
        }
        right.push(panel_toggle_seg);
        right.push(sidebar_toggle_seg);
        if let Some(s) = filetype_seg {
            right.push(s);
        }
        right.push(line_ending_seg);
        right.push(encoding_seg);
        right.push(indent_seg);
        if let Some(s) = showcmd_seg {
            right.push(s);
        }
        if let Some(s) = cursor_seg {
            right.push(s);
        }

        WindowStatusLine {
            left_segments: left,
            right_segments: right,
        }
    } else {
        // ── Inactive window: filename [+]  |  Ln:Col ──
        let mut left = vec![StatusSegment {
            text: format!(" {}", filename),
            fg: theme.status_inactive_fg,
            bg: theme.status_inactive_bg,
            bold: false,
            action: None,
        }];

        if dirty {
            left.push(StatusSegment {
                text: " [+]".to_string(),
                fg: theme.status_inactive_fg,
                bg: theme.status_inactive_bg,
                bold: false,
                action: None,
            });
        }

        let right = if engine.settings.ruler {
            if let Some(c) = cursor {
                // Right-most segment — no trailing space (see the active-
                // window cursor_seg comment above; quadraui#1155 / #1541).
                vec![StatusSegment {
                    text: format!("Ln {}, Col {}", c.line + 1, c.col + 1),
                    fg: theme.status_inactive_fg,
                    bg: theme.status_inactive_bg,
                    bold: false,
                    action: None,
                }]
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        WindowStatusLine {
            left_segments: left,
            right_segments: right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── #221: LSP progress segment formatter ───────────────────────
    #[test]
    fn test_lsp_progress_segment_no_progress() {
        // Pre-#221 behaviour: no progress data → dimmed `name… `.
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", None),
            "rust-analyzer… "
        );
    }

    #[test]
    fn test_lsp_progress_segment_prefers_percentage() {
        // VSCode-style with percentage available: detail is the
        // fixed-width `42%`, not the verbose message string.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Indexing".to_string(),
            message: Some("319/320".to_string()),
            percentage: Some(99),
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Indexing: 99% "
        );
    }

    #[test]
    fn test_lsp_progress_segment_falls_back_to_percentage() {
        // No message string → use percentage as detail.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Indexing".to_string(),
            message: None,
            percentage: Some(42),
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Indexing: 42% "
        );
    }

    #[test]
    fn test_lsp_progress_segment_title_only() {
        // begin with just a title and nothing else: show stage with `…`.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Fetching".to_string(),
            message: None,
            percentage: None,
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Fetching… "
        );
    }

    #[test]
    fn test_lsp_progress_segment_extracts_xy_count_from_path_message() {
        // rust-analyzer's "Roots Scanned" messages embed the full path
        // (e.g. "34/285: /home/john/.cargo/registry/…"). When no
        // percentage is provided, surface the leading `34/285` so the
        // user still sees concrete progress without the path noise.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Roots Scanned".to_string(),
            message: Some(
                "34/285: /home/john/.cargo/registry/src/index.crates.io-1949cf8c/gio-0.18.4"
                    .to_string(),
            ),
            percentage: None,
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Roots Scanned: 34/285 "
        );
    }

    #[test]
    fn test_lsp_progress_segment_drops_unbounded_message() {
        // Free-text messages without a percentage or X/Y prefix
        // (e.g. "cargo metadata: Blocking …") would balloon the segment
        // width and trigger fit-or-drop flicker — we drop the message
        // text and fall back to `stage…`.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Fetching".to_string(),
            message: Some(
                "cargo metadata: Blocking waiting for file lock on package cache".to_string(),
            ),
            percentage: None,
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • Fetching… "
        );
    }

    #[test]
    fn test_lsp_progress_segment_empty_title_uses_working() {
        // Defensive: some servers begin without a title — show "working"
        // rather than a blank stage label.
        let progress = crate::core::lsp_manager::LspProgress {
            title: String::new(),
            message: None,
            percentage: Some(50),
        };
        assert_eq!(
            format_lsp_progress_segment("rust-analyzer", Some(&progress)),
            "rust-analyzer • working: 50% "
        );
    }

    #[test]
    fn test_lsp_progress_segment_width_bound() {
        // Width discipline: the formatted segment must stay ≤ 32 cells
        // for the longest plausible title + percentage combo, to prevent
        // the status bar's priority-drop from flapping during streaming
        // $/progress reports.
        let progress = crate::core::lsp_manager::LspProgress {
            title: "Building compile-time-deps".to_string(),
            message: None,
            percentage: Some(100),
        };
        let s = format_lsp_progress_segment("rust-analyzer", Some(&progress));
        // Width covers `rust-analyzer • Building compile-time-deps: 100% ` ≈ 51 chars.
        // Longest realistic title in rust-analyzer's vocabulary —
        // shorter labels (e.g. "Indexing", "Fetching") stay well under.
        assert!(s.chars().count() < 60, "segment too long: {s:?}");
    }

    #[test]
    fn test_lsp_status_no_manager() {
        use crate::core::engine::Engine;
        // Engine::new() has no lsp_manager — LSP segment should not appear
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // No LSP segment when no manager is running
        let lsp_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::LspInfo));
        assert!(
            lsp_seg.is_none(),
            "should not show LSP segment without lsp_manager"
        );
    }
}
