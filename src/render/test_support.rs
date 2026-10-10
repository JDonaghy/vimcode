use super::*;

// ─── Test-only geometry/key helpers (#812) ────────────────────────────
//
// These four were `pub fn` in the production body of this file with zero
// callers outside `mod tests` — every backend routes the real thing
// through the click/keymap paths instead. #812 moved them here rather
// than deleting them: the tests below are the only consumers, and they
// pin behaviour (tab-expansion, close-button hit box, `<C-S-x>` parsing)
// that is still worth asserting. If a backend ever needs one, lift it
// back out — do not add a second copy.

/// Compute the scrollbar-to-scroll-top mapping from a click position.
/// Returns the new `scroll_top` value.
///
/// - `click_pos`: relative position of click within the scrollbar track (0.0 .. track_len).
/// - `track_len`: total length of the scrollbar track in pixels (or cells).
/// - `total_lines`: total number of lines in the buffer.
/// - `viewport_lines`: number of visible lines in the viewport.
pub(crate) fn scrollbar_click_to_scroll_top(
    click_pos: f64,
    track_len: f64,
    total_lines: usize,
    viewport_lines: usize,
) -> usize {
    if track_len <= 0.0 || total_lines <= viewport_lines {
        return 0;
    }
    let ratio = (click_pos / track_len).clamp(0.0, 1.0);
    let max_scroll = total_lines.saturating_sub(viewport_lines);
    ((ratio * max_scroll as f64).round() as usize).min(max_scroll)
}

/// Compute the display column from a pixel/cell X offset within the text area.
/// Handles tab expansion (tabs = `tabstop` display columns).
///
/// - `line_text`: the text of the buffer line.
/// - `x_offset`: click position relative to the text area start, in character-width units
///   (i.e. `(pixel_x - gutter_px) / char_width` for pixel backends, or `col - gutter` for TUI).
/// - `tabstop`: tab stop width (default 4).
/// - `scroll_left`: horizontal scroll offset in display columns.
///
/// Returns the buffer column index.
pub(crate) fn display_col_to_buffer_col(
    line_text: &str,
    x_offset: usize,
    tabstop: usize,
    scroll_left: usize,
) -> usize {
    let target_display_col = x_offset + scroll_left;
    let mut display_col = 0usize;
    for (i, ch) in line_text.chars().enumerate() {
        if display_col >= target_display_col {
            return i;
        }
        if ch == '\t' {
            display_col += tabstop - (display_col % tabstop);
        } else {
            display_col += 1;
        }
    }
    line_text.chars().count()
}

/// Check if a click at `col` within a tab of total width `tab_width` is on the close button.
/// Close button occupies the rightmost `close_cols` columns of the tab.
pub(crate) fn is_tab_close_click(col_in_tab: usize, tab_width: usize, close_cols: usize) -> bool {
    tab_width > close_cols && col_in_tab >= tab_width - close_cols
}

/// Matches a key binding string (e.g. `<C-S-e>`) against abstract modifier flags
/// and a key name/char. This is the backend-agnostic core of key matching.
///
/// - `binding`: Vim-style binding string like `<C-b>`, `<C-S-e>`, `<A-x>`.
/// - `ctrl`, `shift`, `alt`: whether these modifiers are pressed.
/// - `key_char`: the lowercase character of the pressed key (if printable).
/// - `is_tab`: true if the pressed key is Tab.
/// - `is_space`: true if the pressed key is Space.
/// - `is_escape`: true if the pressed key is Escape.
#[allow(clippy::too_many_arguments)]
pub(crate) fn matches_key_binding(
    binding: &str,
    ctrl: bool,
    shift: bool,
    alt: bool,
    key_char: Option<char>,
    is_tab: bool,
    is_space: bool,
    is_escape: bool,
) -> bool {
    let Some((want_ctrl, want_shift, want_alt, key_name)) =
        crate::core::settings::parse_key_binding_named(binding)
    else {
        return false;
    };
    if want_ctrl != ctrl || want_shift != shift || want_alt != alt {
        return false;
    }
    match key_name.as_str() {
        "Tab" | "tab" => is_tab,
        "Space" | "space" => is_space,
        "Escape" | "Esc" => is_escape,
        s if s.chars().count() == 1 => {
            let want = s.chars().next().unwrap().to_ascii_lowercase();
            key_char
                .map(|c| c.to_ascii_lowercase() == want)
                .unwrap_or(false)
        }
        _ => false,
    }
}
