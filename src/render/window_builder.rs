use super::*;

// ─── Private builder helpers ──────────────────────────────────────────────────

pub(crate) fn build_tab_bar_for_group_by_id(engine: &Engine, group_id: GroupId) -> Vec<TabInfo> {
    let group = match engine.editor_groups.get(&group_id) {
        Some(g) => g,
        None => return vec![],
    };
    group
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let active = i == group.active_tab;
            let window_id = tab.active_window;
            let (name, dirty, preview) = if let Some(window) = engine.windows.get(&window_id) {
                if let Some(state) = engine.buffer_manager.get(window.buffer_id) {
                    (state.display_name(), state.dirty, state.preview)
                } else {
                    ("[No Name]".to_string(), false, false)
                }
            } else {
                ("[No Name]".to_string(), false, false)
            };
            // #700 review nit: a single trailing space so the close glyph
            // doesn't paint flush against the label — see `TabInfo::name`'s
            // doc for why this has to be added here, at the source, rather
            // than downstream where `TabInfo` becomes `quadraui::TabItem`.
            TabInfo {
                name: name + " ",
                active,
                dirty,
                preview,
            }
        })
        .collect()
}

pub(crate) fn build_tab_bar(engine: &Engine) -> Vec<TabInfo> {
    // ScreenLayout.tab_bar always holds the first group's tabs.
    let first_id = engine.group_layout.group_ids().first().copied();
    match first_id {
        Some(gid) => build_tab_bar_for_group_by_id(engine, gid),
        None => vec![],
    }
}

/// Offset table produced by expanding `'list'` glyphs (`\t`, plus any
/// `'listchars'` single-character substitutions — `trail`/`nbsp`/`space`,
/// #1206) in a line's text. Every position-based field a `RenderedLine`
/// carries for that line — byte-offset `StyledSpan`s *and* char-index
/// `DiagnosticMark`/`SpellMark`s — must remap through this single table
/// rather than recomputing the expansion delta twice, which is how #1208
/// happened: the byte-offset remap for `spans` shipped in #1190 with no
/// equivalent for the char-index diagnostic/spell marks.
///
/// Byte and char deltas are tracked *separately* (#1206): a tab expansion
/// changes both by the same amount only when the configured tab glyph is
/// pure ASCII, and a single-character substitution (`trail:·`, say) can
/// change the byte length while leaving the char count at exactly 1 — the
/// pre-#1206 shared-delta design (`:h` #1208's own doc) was only ever exact
/// for the ASCII-only `^I` fallback it was built for.
struct ListGlyphOffsets {
    /// `(old_byte_offset_just_past_this_char, old_char_offset_just_past_this_char, cumulative_byte_delta, cumulative_char_delta)`.
    breakpoints: Vec<(usize, usize, i64, i64)>,
}

impl ListGlyphOffsets {
    fn identity() -> Self {
        Self {
            breakpoints: Vec::new(),
        }
    }

    fn remap_byte(&self, old_byte: usize) -> usize {
        let shift = self
            .breakpoints
            .iter()
            .rev()
            .find(|(bp, _, _, _)| *bp <= old_byte)
            .map(|(_, _, d, _)| *d)
            .unwrap_or(0);
        (old_byte as i64 + shift) as usize
    }

    fn remap_char(&self, old_char: usize) -> usize {
        let shift = self
            .breakpoints
            .iter()
            .rev()
            .find(|(_, bp, _, _)| *bp <= old_char)
            .map(|(_, _, _, d)| *d)
            .unwrap_or(0);
        (old_char as i64 + shift) as usize
    }
}

/// Expand `'list'` glyphs in `text` per the current `'listchars'`
/// (`settings.listchars`) and `'tabstop'` (`settings.tabstop`), returning
/// the expanded text plus the offset table needed to remap any byte- or
/// char-indexed position that pointed into the original `text` (see
/// [`ListGlyphOffsets`]).
///
/// `tab` fills to the next `'tabstop'` stop the way Vim renders it
/// (`:h lcs-tab`) — `col` (visual column, 0-based) is tracked from the
/// start of `text` as if `text` began at column 0, matching how the rest of
/// this rendering pipeline already treats each `RenderedLine` segment (a
/// wrap-continuation segment's own tab/indent-guide math restarts at column
/// 0 too, e.g. the `cols`/`indent` loop building indent guides) — so this
/// isn't a new limitation, just consistent with the existing one. No
/// `tab:` item falls back to the literal `^I` Vim shows when `'listchars'`
/// doesn't mention tabs at all (`:h lcs-tab`, "When tab: is omitted, a tab
/// is shown as ^I").
fn compute_list_glyph_expansion(text: String, settings: &Settings) -> (String, ListGlyphOffsets) {
    let listchars = &settings.listchars;
    let tab_glyph = crate::core::settings::listchars_tab(listchars);
    let trail_glyph = crate::core::settings::listchars_char(listchars, "trail", None);
    let nbsp_glyph = crate::core::settings::listchars_char(listchars, "nbsp", None);
    let space_glyph = crate::core::settings::listchars_char(listchars, "space", None);

    if !text.contains('\t')
        && trail_glyph.is_none()
        && nbsp_glyph.is_none()
        && space_glyph.is_none()
    {
        return (text, ListGlyphOffsets::identity());
    }

    let tabstop = (settings.tabstop as usize).max(1);

    // Trailing-space run (char indices into `text`): where 'trail' applies
    // instead of 'space'/nothing (`:h lcs-trail`: "Overrides the space and
    // multispace settings for trailing spaces"). Excludes a trailing '\n'.
    let core_len = text.chars().count() - usize::from(text.ends_with('\n'));
    let core_chars: Vec<char> = text.chars().take(core_len).collect();
    let mut trail_start = core_len;
    while trail_start > 0 && core_chars[trail_start - 1] == ' ' {
        trail_start -= 1;
    }

    let mut out = String::with_capacity(text.len());
    let mut breakpoints: Vec<(usize, usize, i64, i64)> = Vec::new();
    let mut byte_delta: i64 = 0;
    let mut char_delta: i64 = 0;
    let mut col: usize = 0;

    for (old_char, (old_byte, ch)) in text.char_indices().enumerate() {
        let rendered: String = if ch == '\t' {
            let width = tabstop - (col % tabstop);
            match &tab_glyph {
                Some(g) => g.render(width),
                None => "^I".to_string(),
            }
        } else if ch == ' ' && old_char >= trail_start && old_char < core_len {
            trail_glyph.map_or_else(|| ch.to_string(), String::from)
        } else if ch == '\u{a0}' {
            nbsp_glyph.map_or_else(|| ch.to_string(), String::from)
        } else if ch == ' ' {
            space_glyph.map_or_else(|| ch.to_string(), String::from)
        } else {
            ch.to_string()
        };

        col += rendered.chars().count();
        out.push_str(&rendered);

        let new_byte_delta = byte_delta + rendered.len() as i64 - ch.len_utf8() as i64;
        let new_char_delta = char_delta + rendered.chars().count() as i64 - 1;
        if new_byte_delta != byte_delta || new_char_delta != char_delta {
            byte_delta = new_byte_delta;
            char_delta = new_char_delta;
            breakpoints.push((
                old_byte + ch.len_utf8(),
                old_char + 1,
                byte_delta,
                char_delta,
            ));
        }
    }

    (out, ListGlyphOffsets { breakpoints })
}

/// Vim's `'list'` (#1190, `'listchars'` support #1206): apply the configured
/// glyph set to one already-built `(raw_text, spans, diagnostics,
/// spell_errors)` tuple. Callers must not call this for a fold-header line
/// (`RenderedLine::is_fold_header`) — real vim's `'list'` never marks a
/// closed fold's display text (#1208).
///
/// `mark_eol` is `false` for every wrap-continuation segment except the
/// last — an `eol` glyph belongs at the true end of the buffer line, not at
/// each mid-line wrap point — and even on the last segment, nothing is
/// appended unless `'listchars'` actually has an `eol:` item (Neovim's real
/// default doesn't: `:h 'listchars'`'s default is `"tab:> ,trail:-,nbsp:+"`,
/// no `eol`, so `'list'` shows no trailing `$` out of the box — #1190's
/// hardcoded always-`$` only matched classic Vim's *empty*-`'listchars'`
/// fallback, not this).
///
/// Remaps `spans` (byte offsets), `diagnostics` and `spell_errors` (char
/// indices) through one shared [`ListGlyphOffsets`] table computed from the
/// expansion, rather than two parallel remap implementations that can drift
/// out of sync (#1208).
pub(crate) fn apply_list_glyphs(
    text: String,
    spans: Vec<StyledSpan>,
    diagnostics: Vec<DiagnosticMark>,
    spell_errors: Vec<SpellMark>,
    mark_eol: bool,
    settings: &Settings,
) -> (String, Vec<StyledSpan>, Vec<DiagnosticMark>, Vec<SpellMark>) {
    let (mut out, offsets) = compute_list_glyph_expansion(text, settings);
    if mark_eol {
        if let Some(eol) = crate::core::settings::listchars_char(&settings.listchars, "eol", None) {
            out = match out.strip_suffix('\n') {
                Some(stripped) => format!("{stripped}{eol}\n"),
                None => format!("{out}{eol}"),
            };
        }
    }

    let spans = spans
        .into_iter()
        .map(|s| StyledSpan {
            start_byte: offsets.remap_byte(s.start_byte),
            end_byte: offsets.remap_byte(s.end_byte),
            ..s
        })
        .collect();
    let diagnostics = diagnostics
        .into_iter()
        .map(|d| DiagnosticMark {
            start_col: offsets.remap_char(d.start_col),
            end_col: offsets.remap_char(d.end_col),
            ..d
        })
        .collect();
    let spell_errors = spell_errors
        .into_iter()
        .map(|s| SpellMark {
            start_col: offsets.remap_char(s.start_col),
            end_col: offsets.remap_char(s.end_col),
        })
        .collect();

    (out, spans, diagnostics, spell_errors)
}

/// Slice `spans` to cover only the byte range `[seg_start_byte, seg_end_byte)`,
/// adjusting `start_byte`/`end_byte` to be relative to `seg_start_byte`.
/// Used when splitting a wrapped line into per-segment `RenderedLine` entries.
fn slice_spans_for_segment(
    spans: &[StyledSpan],
    seg_start_byte: usize,
    seg_end_byte: usize,
) -> Vec<StyledSpan> {
    let mut result = Vec::new();
    for span in spans {
        let overlap_start = span.start_byte.max(seg_start_byte);
        let overlap_end = span.end_byte.min(seg_end_byte);
        if overlap_start < overlap_end {
            result.push(StyledSpan {
                start_byte: overlap_start - seg_start_byte,
                end_byte: overlap_end - seg_start_byte,
                style: span.style,
            });
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_rendered_window(
    engine: &Engine,
    theme: &Theme,
    window_id: WindowId,
    rect: &WindowRect,
    visible_lines: usize,
    char_width: f64,
    is_active: bool,
    multi_window: bool,
    color_headings: bool,
    scrollbar_reserve: f64,
    minimap_w: f64,
) -> RenderedWindow {
    let empty = |id: WindowId| RenderedWindow {
        window_id: id,
        rect: *rect,
        lines: vec![],
        visible_line_capacity: visible_lines.max(1),
        cursor: None,
        extra_cursors: vec![],
        selection: None,
        extra_selections: vec![],
        yank_highlight: None,
        scroll_top: 0,
        scroll_left: 0,
        total_lines: 0,
        gutter_char_width: 0,
        text_viewport_cols: 0,
        minimap_reserved_w: 0.0,
        is_active,
        show_active_bg: false,
        has_git_diff: false,
        has_breakpoints: false,
        max_col: 0,
        diagnostic_gutter: std::collections::HashMap::new(),
        code_action_lines: std::collections::HashSet::new(),
        bracket_match_positions: Vec::new(),
        active_indent_col: None,
        tabstop: engine.settings.tabstop.max(1) as usize,
        cursorline: engine.settings.cursorline,
        status_line: None,
        plugin_view: None,
    };

    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return empty(window_id),
    };
    let buffer_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return empty(window_id),
    };
    // #1627: an editor-tab-hosted plugin view's window paints a `Form`
    // (`App::paint_editor_windows_rung`), never buffer text — short-circuit
    // before any of the syntax/diagnostic/fold work below, all of which is
    // meaningless against the tiny scratch buffer backing it.
    if let Some(name) = &buffer_state.plugin_view {
        return RenderedWindow {
            plugin_view: Some(name.clone()),
            ..empty(window_id)
        };
    }

    let buffer = &buffer_state.buffer;
    let view = &window.view;
    let total_lines = buffer.len_lines();
    // Clamp scroll_top so that line_to_byte never panics when the cursor was
    // set to a line beyond the buffer (e.g. DAP exception in a stdlib file
    // that failed to open, leaving a small buffer with a large scroll offset).
    let scroll_top = view.scroll_top.min(total_lines);
    let cursor_line = view.cursor.line;

    // #1515: `badge` mode's gutter-marker overlay — the lines the most
    // recent still-outstanding ACP turn checkpoint changed in this buffer,
    // base = that checkpoint's `pre_turn_content` (not `git diff HEAD` —
    // see `crate::core::acp_turn::line_status`'s own doc for why the two
    // bases can legitimately disagree). Only painted under `Badge`: `Auto`
    // doesn't need it (the full-viewport modal already covers this —
    // "auto keeps today's behaviour"), and `Off`'s whole contract is
    // *nothing* until an explicit `:AiReview` ("neither the modal nor the
    // badge/gutter nudge" — see `AcpReviewOnTurnEnd::Off`'s own doc).
    // Also skipped when this buffer's path isn't part of any outstanding
    // checkpoint at all, so the (rare, temporary — only while an
    // unreviewed turn checkpoint exists) diff cost is paid only for a
    // buffer an agent turn actually just touched.
    let acp_turn_status: Vec<Option<GitLineStatus>> = if engine.settings.acp_review_on_turn_end
        == crate::core::settings::AcpReviewOnTurnEnd::Badge
    {
        buffer_state
            .file_path
            .as_deref()
            .and_then(|p| engine.acp_turn_pre_content_for_path(&p.to_string_lossy()))
            .map(|pre| crate::core::acp_turn::line_status(pre.as_deref(), &buffer.to_string()))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Whether this buffer has git diff data (or an outstanding ACP-turn
    // overlay — #1515 — to show in the same gutter column).
    let has_git = !buffer_state.git_diff.is_empty() || !acp_turn_status.is_empty();

    // #1517: in-buffer inline review — the still-outstanding hunks between
    // this buffer's checkpoint entry and its live content, gated the same
    // `badge`-only way `acp_turn_status` immediately above is (see that
    // computation's own doc). Reused below to paint a virtual
    // "[a] keep  [r] reject" action row right after each hunk's last line
    // — `crate::core::engine::acp_turn_ops::Engine::acp_inline_review_
    // hunks`'s own doc has the full "why not `ChangeReviewState`" reasoning.
    let inline_review_hunks: Vec<quadraui::DiffHunk> = buffer_state
        .file_path
        .as_deref()
        .map(|p| engine.acp_inline_review_hunks(&p.to_string_lossy()))
        .unwrap_or_default();

    // Look up LSP diagnostics for this buffer.
    // Diagnostics are keyed by absolute path (from LSP URIs), but buffer file_path
    // may be relative, so use the pre-computed canonical_path cached at file-open
    // time rather than calling canonicalize() (a filesystem syscall) every frame.
    let canonical_path = buffer_state.canonical_path.as_ref();
    let file_diagnostics = canonical_path.and_then(|p| engine.lsp_diagnostics.get(p));

    // Pre-index diagnostics by start line in a single pass.
    // This gives O(1) per-line lookup during visible-line rendering AND builds the gutter
    // severity map simultaneously, replacing two separate O(N_diags) scans with one.
    let mut diag_by_line: std::collections::HashMap<usize, Vec<&crate::core::lsp::Diagnostic>> =
        std::collections::HashMap::new();
    let mut diagnostic_gutter: std::collections::HashMap<
        usize,
        crate::core::lsp::DiagnosticSeverity,
    > = std::collections::HashMap::new();
    if let Some(diags) = file_diagnostics {
        for d in diags {
            let line = d.range.start.line as usize;
            diag_by_line.entry(line).or_default().push(d);
            let entry = diagnostic_gutter.entry(line).or_insert(d.severity);
            if (d.severity as u8) < (*entry as u8) {
                *entry = d.severity;
            }
        }
    }

    // DAP breakpoints for this buffer.
    // Use the raw buffer path as key (matches how dap_toggle_breakpoint stores them).
    let bp_file_key = buffer_state
        .file_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bp_infos: &[crate::core::dap::BreakpointInfo] = engine
        .dap_breakpoints
        .get(&bp_file_key)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let bp_lines: Vec<u64> = bp_infos.iter().map(|bp| bp.line).collect();

    // #1653: plugin decoration marks (`vimcode.decor.*`) touching the
    // viewport — fetched once per window per frame (not per visible line,
    // and never a full-buffer scan: `DecorState::marks_touching` only walks
    // its per-buffer `by_row` index up to `end_row`), then filtered to the
    // exact line inside the per-line loop below. `+1` covers the one extra
    // trailing row `approx_end_line` (below) also pads for.
    let decor_viewport_end = (scroll_top + visible_lines + 1).min(total_lines.saturating_sub(1));
    let decor_marks_near_viewport: Vec<&crate::core::buffer::DecorMark> = if total_lines > 0 {
        engine
            .decor
            .marks_touching(window.buffer_id, scroll_top, decor_viewport_end)
    } else {
        Vec::new()
    };
    // Gutter sign glyphs (`sign_text`) reuse the breakpoint column rather
    // than reserving a new one (see the `bp_part` composition below for the
    // paint-time priority: DAP-current-line/breakpoint beats a decor sign,
    // which beats blank) — so a buffer with decor signs but no real
    // breakpoints still needs that column shown.
    let has_decor_sign = decor_marks_near_viewport
        .iter()
        .any(|m| m.opts.sign_text.is_some());
    // Show the breakpoint column when any BP is set for this file, a DAP
    // session is active (so the column width stays stable during a
    // session), or a decor sign needs it.
    let has_bp = !bp_lines.is_empty() || engine.dap_session_active || has_decor_sign;

    // Stopped-line path for per-line comparison (try canonical, then raw).
    let dap_stop_path = engine.dap_current_line.as_ref().map(|(p, _)| p.as_str());

    // Markdown preview buffers never show line numbers.
    let line_number_mode = if buffer_state.md_rendered.is_some() {
        LineNumberMode::None
    } else {
        engine.settings.line_numbers
    };

    // Gutter width in character columns (always includes fold indicator column).
    let gutter_char_width =
        calculate_gutter_cols(line_number_mode, total_lines, char_width, has_git, has_bp);

    // Compute the accurate content width (in character columns) directly from the
    // precise pixel rect and measured char_width.  This avoids the approximate
    // viewport_cols that was stored during the resize callback (which used a
    // hardcoded char_width_approx of 9.0 px and a fixed gutter offset of 5).
    // For the TUI backend, rect.width is already in cell columns and char_width=1.0,
    // so the formula reduces to rect.width - gutter_char_width, which is exact.
    //
    // `scrollbar_reserve` is the caller's own backend's
    // `quadraui::Backend::scrollbar_reserve()` (#828/quadraui#776) — width
    // reserved alongside the text for a native scrollbar overlay (GTK's
    // `ScrolledWindow`; TUI has none, so its backend returns `0.0`). This
    // function has no opinion on what that value is; it only subtracts
    // whatever the caller measured.
    //
    // `minimap_w` (#1094): `rect` is now the *pane's* rect, unmodified by
    // the strip — see this function's caller (`build_screen_layout`'s
    // `minimap_widths`/`windows` map, where `minimap_w` is derived) for
    // the full rationale on why `rect` stopped being narrowed and what
    // `minimap_w` folds in. Subtracting it here alongside
    // `scrollbar_reserve` keeps the text column count from overestimating
    // into the strip's own columns, even though `rect` itself runs wider
    // than that now.
    let render_viewport_cols = if char_width > 0.0 {
        let total_chars =
            ((rect.width - scrollbar_reserve - minimap_w) / char_width).floor() as usize;
        total_chars.saturating_sub(gutter_char_width).max(1)
    } else {
        view.viewport_cols.max(1)
    };

    // #1094 review: `RenderedWindow.minimap_reserved_w` — the same
    // strip-plus-gutter width just subtracted above, but re-expressed
    // relative to `rect.width` (i.e. with `scrollbar_reserve` folded back
    // in) so `window_zone_hit_test` can find the strip's boundary directly
    // from `rect.width` without needing `scrollbar_reserve` threaded
    // through as a parameter of its own. `minimap_w` is already `0.0` when
    // there's no strip for this window, matching the field's own contract.
    let minimap_reserved_w = if minimap_w > 0.0 {
        minimap_w + scrollbar_reserve
    } else {
        0.0
    };

    // Narrow the highlights slice to only the visible window using binary search.
    // Tree-sitter emits highlights sorted by start_byte, so partition_point is valid.
    // This reduces build_spans from O(N_total_highlights) per line to O(N_window_highlights).
    let window_start_byte = buffer.content.line_to_byte(scroll_top);
    let approx_end_line = (scroll_top + visible_lines + 1).min(total_lines);
    let window_end_byte = if approx_end_line < total_lines {
        buffer.content.line_to_byte(approx_end_line)
    } else {
        buffer.content.len_bytes()
    };
    let hl_lo = buffer_state
        .highlights
        .partition_point(|h| h.1 <= window_start_byte);
    let hl_hi = buffer_state
        .highlights
        .partition_point(|h| h.0 < window_end_byte);
    let visible_hl = &buffer_state.highlights[hl_lo..hl_hi];

    // Compute search matches for this buffer.  The engine's `search_matches`
    // only indexes the *active* buffer, so for other visible buffers we must
    // compute matches from `search_query` against this buffer's text.
    let active_buf_id = engine
        .windows
        .get(&engine.active_window_id())
        .map(|w| w.buffer_id);
    let buf_search_matches: Vec<(usize, usize)> =
        if !engine.settings.hlsearch || engine.search_query.is_empty() {
            Vec::new()
        } else if Some(window.buffer_id) == active_buf_id {
            engine.search_matches.clone()
        } else {
            compute_search_matches_for_buffer(buffer, &engine.search_query, &engine.settings)
        };

    // Ghost text (AI inline completion): only in the active window, Insert mode.
    // Multi-line completions are stored in full (Tab-accept inserts everything).
    // The first line is shown after the cursor (ghost_suffix on the cursor line).
    // Subsequent lines are inserted as virtual ghost continuation rows so the
    // user can see the full suggestion before accepting with Tab.
    let (ghost_for_cursor_line, ghost_continuation_lines): (Option<String>, Vec<String>) =
        if is_active && engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
            match &engine.ai_ghost_text {
                None => (None, Vec::new()),
                Some(g) => {
                    let mut it = g.lines();
                    let first = it.next().unwrap_or("").to_string();
                    let cont: Vec<String> = it.map(|l| l.to_string()).collect();
                    (Some(first), cont)
                }
            }
        } else {
            (None, Vec::new())
        };

    // Look up aligned diff data for this window (for visual padding).
    let diff_aligned: Option<&[AlignedDiffEntry]> =
        engine.diff_aligned.get(&window_id).map(|v| v.as_slice());

    // Whether *open*-foldable-region markers (`-`) paint in this window's
    // gutter this frame (#1544, VS Code's `editor.showFoldingControls`).
    // Closed-fold `+` markers are handled separately, inside
    // `fold_indicator_char`, and always paint under `Mouseover` — a fold you
    // can't see is a fold you can't discover how to reopen. `Never` hides
    // both. `engine.gutter_hover_window` is updated by
    // `route_gutter_hover`, called from the same shared `MouseMoved` path on
    // both backends (`App::handle_dispatch`) — no per-backend hover logic.
    let fold_open_markers_visible = match engine.settings.fold_controls {
        FoldControlsMode::Always => true,
        FoldControlsMode::Never => false,
        FoldControlsMode::Mouseover => engine.gutter_hover_window == Some(window_id),
    };

    // Build rendered lines (fold-aware: skip hidden lines, jump over fold bodies)
    let mut lines = Vec::with_capacity(visible_lines);

    // #1653: inline virtual text (`virt_text_pos = "inline"`) shifts later
    // text on its line right — `(anchor_col, inserted_chars)` pairs per
    // `line_idx`, consulted once the cursor's screen column is computed
    // below (`view.cursor.col` itself stays in *buffer* coordinates; only
    // the painted cursor cell needs to account for text inserted before it).
    let mut inline_shifts: std::collections::HashMap<usize, Vec<(usize, usize)>> =
        std::collections::HashMap::new();

    // When aligned diff data exists, iterate through the aligned sequence
    // so padding lines appear at the correct visual positions.
    //
    // `view.aligned_top` (set by `sync_scroll_binds`) wins when present:
    // it pins the starting aligned index so both panes of a scroll-bound
    // pair land on exactly the same row. Without it we fall back to a
    // seek-from-`scroll_top` heuristic, then back up over any leading
    // padding so a hunk's filler rows render at the top of the viewport
    // when scroll_top lands just past them (#166).
    let mut aligned_idx: usize = if let Some(aligned) = diff_aligned {
        if let Some(top) = view.aligned_top {
            top.min(aligned.len())
        } else {
            let seek_idx = aligned
                .iter()
                .position(|e| e.source_line.is_some_and(|sl| sl >= scroll_top))
                .unwrap_or(0);
            let mut k = seek_idx;
            while k > 0 && aligned[k - 1].source_line.is_none() {
                k -= 1;
            }
            k
        }
    } else {
        0
    };
    // When `aligned_top` pins the start at a padding entry, advance
    // `line_idx` to the next real source line so the buffer-line-driven
    // outer loop emits padding for the leading None entries before
    // emitting that real line. Falls back to `scroll_top` when there is
    // no aligned data (the non-diff path) or no Some entry remains
    // (trailing-padding edge case).
    let mut line_idx = if let Some(aligned) = diff_aligned {
        aligned[aligned_idx..]
            .iter()
            .find_map(|e| e.source_line)
            .unwrap_or(scroll_top)
    } else {
        scroll_top
    };
    while lines.len() < visible_lines && line_idx < total_lines {
        // Skip hidden lines (fold bodies).
        if view.is_line_hidden(line_idx) {
            // Also advance aligned_idx past this hidden line's entry
            // (and any adjacent padding) so padding for folded regions
            // doesn't get emitted as blank lines.
            if let Some(aligned) = diff_aligned {
                while aligned_idx < aligned.len() {
                    match aligned[aligned_idx].source_line {
                        Some(sl) if sl == line_idx => {
                            aligned_idx += 1;
                            break;
                        }
                        Some(sl) if sl > line_idx => break,
                        _ => aligned_idx += 1, // skip padding or earlier source lines
                    }
                }
            }
            line_idx += 1;
            continue;
        }

        // Emit padding lines from the aligned diff sequence before this buffer line.
        if let Some(aligned) = diff_aligned {
            while aligned_idx < aligned.len() && lines.len() < visible_lines {
                let entry = &aligned[aligned_idx];
                if let Some(sl) = entry.source_line {
                    if sl >= line_idx {
                        break; // reached the current buffer line
                    }
                    // This source line is before scroll_top — skip it.
                    aligned_idx += 1;
                    continue;
                }
                // When unchanged lines are hidden (fold-filtered diff view),
                // suppress padding lines — alignment is meaningless when
                // the unchanged context between hunks is collapsed.
                if engine.diff_unchanged_hidden {
                    aligned_idx += 1;
                    continue;
                }
                // Padding entry — emit an empty rendered line.
                let padding_gutter = format!(
                    "{:>width$} ",
                    "",
                    width = gutter_char_width.saturating_sub(1)
                );
                lines.push(RenderedLine {
                    gutter_text: padding_gutter,
                    raw_text: String::new(),
                    spans: vec![],
                    line_idx,
                    git_diff: None,
                    diagnostics: vec![],
                    spell_errors: vec![],
                    diff_status: Some(DiffLine::Padding),
                    is_breakpoint: false,
                    is_conditional_bp: false,
                    is_dap_current: false,
                    is_wrap_continuation: false,
                    segment_col_offset: 0,
                    annotation: None,
                    ghost_suffix: None,
                    is_current_line: false,
                    is_fold_header: false,
                    folded_line_count: 0,
                    is_ghost_continuation: false,
                    indent_guides: vec![],
                    colorcolumns: vec![],
                });
                aligned_idx += 1;
            }
            if lines.len() >= visible_lines {
                break;
            }
            // Advance aligned_idx past this buffer line's entry.
            if aligned_idx < aligned.len() {
                if let Some(sl) = aligned[aligned_idx].source_line {
                    if sl == line_idx {
                        aligned_idx += 1;
                    }
                }
            }
        }

        let is_fold_header = view.fold_at(line_idx).is_some();
        let folded_line_count = view.fold_at(line_idx).map(|f| f.end - f.start).unwrap_or(0);

        let line = buffer.content.line(line_idx);
        let line_str = line.to_string().replace('\0', "");
        let line_start_byte = buffer.content.line_to_byte(line_idx);
        let line_end_byte = line_start_byte + line.len_bytes();

        let spans = if let Some(ref md) = buffer_state.md_rendered {
            if line_idx < md.spans.len() {
                let code_hl = md.code_highlights.get(line_idx);
                md_spans_to_styled(&md.spans[line_idx], code_hl, theme, color_headings)
            } else {
                vec![]
            }
        } else {
            let is_markdown = buffer_state
                .file_path
                .as_ref()
                .and_then(|p| p.to_str())
                .and_then(crate::core::syntax::SyntaxLanguage::from_path)
                == Some(crate::core::syntax::SyntaxLanguage::Markdown);
            build_spans(
                engine,
                theme,
                visible_hl,
                &buffer_state.semantic_tokens,
                buffer,
                line_idx,
                &line_str,
                line_start_byte,
                line_end_byte,
                is_markdown,
                &buf_search_matches,
                Some(window.buffer_id) == active_buf_id,
            )
        };

        // #1653: apply plugin decorations anchored to this line — highlight
        // spans first (merged into `spans` so the wrapped-line segmentation
        // below slices them exactly like any syntax span), then overlay/
        // inline virtual text, which mutates `line_str` + `spans` together
        // so later text and its highlighting both reflect the splice.
        let line_decor: Vec<&DecorMark> = decor_marks_near_viewport
            .iter()
            .filter(|m| m.row <= line_idx && m.end_row >= line_idx)
            .copied()
            .collect();
        let mut spans = spans;
        let mut line_str = line_str;
        // #1653 review: LSP diagnostics/spell-check ranges below are
        // computed from UTF-16/byte offsets measured against the buffer's
        // real text, *before* any decor splice below mutates `line_str` —
        // reusing the (possibly already-spliced) `line_str` there would
        // land those ranges on the wrong columns on a line that also has
        // an inline/overlay decoration. Snapshot the pre-splice text now,
        // while it's still guaranteed to match those ranges.
        let line_str_pre_decor = line_str.clone();
        for m in &line_decor {
            if let Some(hl) = &m.opts.hl_group {
                if let Some((start_col, end_col)) =
                    decor_highlight_cols(m, line_idx, line_str.chars().count())
                {
                    if end_col > start_col {
                        let start_byte =
                            quadraui::text_util::char_to_byte_idx(&line_str, start_col);
                        let end_byte = quadraui::text_util::char_to_byte_idx(&line_str, end_col);
                        spans.push(StyledSpan {
                            start_byte,
                            end_byte,
                            style: resolve_decor_style(engine, theme, Some(hl.as_str())),
                        });
                    }
                }
            }
        }
        // #1653 review: process left-to-right by anchor column, tracking
        // how many characters earlier *inline* splices on this same line
        // have already inserted (`col_shift`) — `m.col` is a buffer column,
        // fixed at `set_mark` time, but `line_str` keeps growing as each
        // earlier inline splice runs, so a second mark's splice point has
        // to be adjusted by however much text landed before it, or it (and
        // everything after it) lands `col_shift` characters too early.
        // Overlay splices don't need to bump `col_shift` themselves since
        // they replace exactly as many characters as they insert (net-zero
        // width change) — only `Inline` grows the line.
        //
        // #1810 split the row's virtual-text marks in two here: `Overlay`
        // and `Inline` splice into `line_str`/`spans` in the loop below,
        // while `Eol` (and an unset `virt_text_pos`, which means the same
        // thing) has no buffer column to splice at and paints through
        // `RenderedLine::annotation` instead — see `decor_eol_text` below.
        let (mut virt_text_marks, mut eol_marks): (Vec<&DecorMark>, Vec<&DecorMark>) = line_decor
            .iter()
            .filter(|m| m.row == line_idx && !m.opts.virt_text.is_empty())
            .copied()
            .partition(|m| {
                matches!(
                    m.opts.virt_text_pos,
                    Some(VirtTextPos::Overlay) | Some(VirtTextPos::Inline)
                )
            });
        virt_text_marks.sort_by_key(|m| m.col);
        let mut col_shift: usize = 0;
        for m in virt_text_marks {
            let text: String = m.opts.virt_text.iter().map(|c| c.text.as_str()).collect();
            if text.is_empty() {
                continue;
            }
            let first_chunk_hl = m.opts.virt_text[0].hl_group.as_deref();
            let style = resolve_decor_style(engine, theme, first_chunk_hl);
            let splice_col = m.col + col_shift;
            match m.opts.virt_text_pos {
                Some(VirtTextPos::Overlay) => {
                    let replace_chars = text.chars().count();
                    splice_virt_text(
                        &mut line_str,
                        &mut spans,
                        splice_col,
                        replace_chars,
                        &text,
                        style,
                    );
                }
                Some(VirtTextPos::Inline) => {
                    splice_virt_text(&mut line_str, &mut spans, splice_col, 0, &text, style);
                    let inserted = text.chars().count();
                    col_shift += inserted;
                    // `inline_shifts` drives the painted-cursor-column
                    // correction (below), which compares against
                    // `view.cursor.col` — a buffer column — so the anchor
                    // recorded here must stay in buffer-column space
                    // (`m.col`, not `splice_col`).
                    inline_shifts
                        .entry(line_idx)
                        .or_default()
                        .push((m.col, inserted));
                }
                Some(VirtTextPos::Eol) | None => {
                    // `partition`ed into `eol_marks` above (#1810), so this
                    // arm is dead for every mark that reaches this loop.
                    // Left as an explicit no-op rather than `unreachable!`
                    // so a future change to the partition predicate can
                    // never turn a decor mark into a render-path panic.
                }
            }
        }
        // #1810: end-of-line virtual text (`virt_text_pos = "eol"`, or no
        // `virt_text_pos` at all — the same default) has no buffer column
        // to splice at like Overlay/Inline above, and must never wrap or
        // push the line the way real spliced content would. It converges
        // on `vimcode.buf.annotate_line`'s existing paint path instead
        // (`RenderedLine::annotation`, below), which already paints after
        // the line's content in `theme.annotation_fg` and is hard-truncated
        // at the window edge rather than wrapped — exactly the behaviour
        // #1810 asks for. Several eol marks on one line draw in creation
        // order (oldest `set_mark` first, i.e. ascending `MarkId` — note
        // `DecorOpts` has no `priority` field at all; "creation order" is
        // what the code below actually does),
        // separated by a space.
        //
        // Two deliberate differences from the blame annotation it shares
        // that field with (both applied at the `annotation:` sites below):
        // on a wrapped line it rides the *last* visual segment, not the
        // first, and it is not muted while the user is in Insert mode. One
        // consequence of the first-row/last-row split: if the viewport cuts
        // off before this line's last wrap segment (the `lines.len() >=
        // visible_lines` guard below can stop early), `decor_eol_text` is
        // dropped for that line entirely — it never rides an earlier,
        // still-visible segment the way the blame annotation does. Probably
        // the right trade-off (there's no "end of line" to show until the
        // line's actual end scrolls into view), but it means a long wrapped
        // line's eol text can silently vanish below the fold.
        //
        // Per-mark colour (the chunk's own `hl_group`, as Overlay/Inline
        // get via `resolve_decor_style`) is NOT implemented: quadraui's
        // `EditorLine::annotation` is a single `Option<String>` painted in
        // one theme-wide `annotation_fg` colour — there is no quadraui
        // primitive for multiple independently-coloured runs in the
        // trailing-annotation area. That needs quadraui infra first (a
        // quadraui issue, per the platform-neutrality rule) before eol
        // text can paint in its own highlight's colour rather than
        // `annotation_fg` — drafted as a quadraui gap in
        // `docs/PENDING_QUADRAUI_ISSUES.md` (also covers the TUI-vs-GTK
        // leading-pad inconsistency) rather than left only in this comment.
        eol_marks.sort_by_key(|m| m.id.0);
        let decor_eol_text: Option<String> = {
            let parts: Vec<String> = eol_marks
                .iter()
                .map(|m| {
                    m.opts
                        .virt_text
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<String>()
                })
                .filter(|s| !s.is_empty())
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        };
        // A decor sign for this line's gutter slot (lowest priority — see
        // `bp_part` below): the first mark with `sign_text` touching this
        // line, truncated to its first character.
        //
        // #1653's own scope text asks for "1-2 display cells" of sign_text
        // plus `sign_hl`, but neither half of that can actually paint
        // through this column today, and the reason is a *quadraui*
        // limitation, not a vimcode one: both backends' gutter rasterisers
        // hardcode the breakpoint/sign slot at exactly one character —
        // quadraui's `gtk/editor.rs::paint_gutter_row_number` does
        // `rl.gutter_text.chars().take(1)` for the bp glyph and then
        // advances its own `char_offset` by a bare `1` before reading the
        // git column; `tui/editor.rs`'s gutter loop computes `git_offset =
        // bp_offset + 1` the same fixed way. Handing either backend a
        // 2-character `gutter_text` here would not render a wider sign —
        // it would misalign the git/line-number columns after it, since
        // neither rasteriser's `+1` is driven by any width this crate
        // controls. Likewise, colouring just the sign glyph with `sign_hl`
        // would need a per-glyph colour channel on quadraui's `EditorLine`
        // that doesn't exist (`gutter_text` is one plain `String`, painted
        // in one colour chosen from `is_breakpoint`/`is_dap_current`/
        // `git_diff`). Per the platform-neutrality rule this needs
        // quadraui-side infra first (a real `bp_col_width`/per-glyph-colour
        // API) rather than a per-backend workaround here — drafted as a
        // quadraui gap in `docs/PENDING_QUADRAUI_ISSUES.md` rather than
        // left only in this comment; until it lands, `sign_text` is truncated to 1
        // character and `sign_hl` is parsed/stored (round-trips through
        // `get_mark`) but has no paint effect.
        let decor_sign_glyph: Option<String> = line_decor
            .iter()
            .find_map(|m| m.opts.sign_text.as_deref())
            .and_then(|s| s.chars().next())
            .map(|c| c.to_string());

        // Git diff status for this line, falling back to the #1515
        // ACP-turn overlay (`acp_turn_status`) where the real git diff has
        // nothing to say about it — a file already git-dirty before the
        // turn keeps showing its real git status first.
        let git_status = if has_git {
            buffer_state
                .git_diff
                .get(line_idx)
                .copied()
                .flatten()
                .or_else(|| acp_turn_status.get(line_idx).copied().flatten())
        } else {
            None
        };

        // DAP: is there a breakpoint on this line? Is the adapter stopped here?
        let line_1based = line_idx as u64 + 1;
        let is_breakpoint = has_bp && bp_lines.binary_search(&line_1based).is_ok();
        let is_conditional_bp = is_breakpoint
            && bp_infos.iter().any(|bp| {
                bp.line == line_1based && (bp.condition.is_some() || bp.hit_condition.is_some())
            });
        let is_dap_current = engine
            .dap_current_line
            .as_ref()
            .map(|(path, l)| {
                *l == line_1based
                    && (dap_stop_path == Some(path.as_str())
                        || canonical_path
                            .map(|cp| cp.to_string_lossy().as_ref() == path.as_str())
                            .unwrap_or(false))
            })
            .unwrap_or(false);

        let fold_char = fold_indicator_char(
            buffer,
            view,
            line_idx,
            engine.settings.fold_controls,
            fold_open_markers_visible,
        );
        // Number of leading marker columns (bp + git) subtracted from the
        // numeric portion so line numbers fill their allotted width correctly.
        let marker_cols = if has_bp { 1 } else { 0 } + if has_git { 1 } else { 0 };
        let base_gutter = format_gutter_with_fold(
            line_number_mode,
            line_idx,
            cursor_line,
            gutter_char_width.saturating_sub(marker_cols),
            fold_char,
        );
        // Build gutter_text: [bp_char][git_char][fold+nums]
        let gutter_text = {
            let bp_part = if has_bp {
                if is_dap_current && is_breakpoint {
                    "◉" // breakpoint + current line
                } else if is_dap_current {
                    "▶" // current execution line (no breakpoint)
                } else if is_conditional_bp {
                    "◆" // conditional breakpoint
                } else if is_breakpoint {
                    "●" // breakpoint
                } else {
                    // #1653: a plugin decor sign shows in this column when
                    // there's no real breakpoint/DAP marker on the line —
                    // priority is DAP-current/breakpoint, then decor sign,
                    // then blank.
                    decor_sign_glyph.as_deref().unwrap_or(" ")
                }
            } else {
                ""
            };
            let git_part = if has_git {
                match git_status {
                    Some(GitLineStatus::Added) | Some(GitLineStatus::Modified) => "▌",
                    Some(GitLineStatus::Deleted) => "▾",
                    None => " ",
                }
            } else {
                ""
            };
            format!("{}{}{}", bp_part, git_part, base_gutter)
        };

        // LSP diagnostics for this line — O(1) lookup via pre-indexed map.
        let line_diagnostics: Vec<DiagnosticMark> = if let Some(diags) = diag_by_line.get(&line_idx)
        {
            diags
                .iter()
                .map(|d| {
                    // #1653 review: must use the pre-decor-splice text —
                    // `d.range` is measured against the buffer's real
                    // content, not whatever `line_str` looks like after an
                    // inline/overlay virtual-text splice shifted its bytes.
                    let start_col = crate::core::lsp::utf16_offset_to_char(
                        &line_str_pre_decor,
                        d.range.start.character,
                    );
                    let end_col = if d.range.end.line as usize == line_idx {
                        crate::core::lsp::utf16_offset_to_char(
                            &line_str_pre_decor,
                            d.range.end.character,
                        )
                    } else {
                        line_str_pre_decor.len()
                    };
                    DiagnosticMark {
                        start_col,
                        end_col: end_col.max(start_col + 1),
                        severity: d.severity,
                        message: d.message.clone(),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        // Spell-check errors for this line — computed on visible lines only.
        let line_spell_errors: Vec<SpellMark> = if engine.settings.spell {
            if let Some(ref checker) = engine.spell_checker {
                let syntax_lang = buffer_state
                    .file_path
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .and_then(crate::core::syntax::SyntaxLanguage::from_path);
                let line_start_byte = buffer.content.line_to_byte(line_idx);
                // #1653 review: same pre-splice-text rationale as the
                // diagnostics block above — spell errors are columns into
                // the buffer's real line text.
                crate::core::spell::check_line(
                    checker,
                    &line_str_pre_decor,
                    &buffer_state.highlights,
                    line_start_byte,
                    syntax_lang,
                )
                .into_iter()
                .map(|e| SpellMark {
                    start_col: e.start_col,
                    end_col: e.end_col,
                })
                .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Two-way diff status for this line.
        let diff_status = engine
            .diff_results
            .get(&window_id)
            .and_then(|v| v.get(line_idx))
            .copied();

        let is_md_preview = engine.md_preview_links.contains_key(&window.buffer_id);
        let wrap_on =
            (engine.settings.wrap || is_md_preview) && render_viewport_cols > 0 && !is_fold_header;
        // Wrap on the line's *content* only — without its trailing EOL.
        // Counting the `\n` as a character made a line whose text exactly
        // fills its last wrapped row spill the newline into an extra, blank
        // continuation row that `ensure_cursor_visible_wrap` (which counts
        // rows on the EOL-stripped text) never accounted for, so `G` could
        // land the cursor below the viewport (#1496).
        let wrap_text_len = line_str.trim_end_matches(['\n', '\r']).len();
        let line_char_len = line_str[..wrap_text_len].chars().count();
        // 'list' (#1190): applied only to the copy handed to the renderer
        // below (`raw_text`/`spans`) — `line_str` above (already consumed
        // by the diagnostics/spell-check UTF-16 offset math) and the word-
        // wrap segmentation just above stay on the untransformed text, so
        // this cannot perturb any semantic column math, only what paints.
        let list_mode = engine.settings.list;

        if wrap_on && line_char_len > render_viewport_cols {
            // Split long line into viewport-width segments with word-boundary wrapping.
            let vp = render_viewport_cols;
            // Build segment boundaries using word-aware splitting.
            let segment_boundaries = compute_word_wrap_segments(
                &line_str[..wrap_text_len],
                vp,
                engine.settings.linebreak,
            );
            let num_segments = segment_boundaries.len();
            let cursor_seg = if line_idx == cursor_line {
                // Find which segment contains the cursor column.
                segment_boundaries
                    .iter()
                    .position(|&(start, end)| view.cursor.col >= start && view.cursor.col < end)
                    .unwrap_or(num_segments.saturating_sub(1))
            } else {
                usize::MAX // won't match any segment
            };
            // Blank gutter for continuation rows (same width as normal gutter).
            let blank_gutter = " ".repeat(gutter_char_width);
            for (seg, &(seg_start_char, seg_end_char)) in segment_boundaries.iter().enumerate() {
                if lines.len() >= visible_lines {
                    break;
                }
                let seg_start_byte =
                    quadraui::text_util::char_to_byte_idx(&line_str, seg_start_char);
                let seg_end_byte = quadraui::text_util::char_to_byte_idx(&line_str, seg_end_char);
                let seg_text = line_str[seg_start_byte..seg_end_byte].to_string();
                let seg_spans = slice_spans_for_segment(&spans, seg_start_byte, seg_end_byte);
                let is_cont = seg > 0;
                let is_last_seg = seg + 1 == num_segments;
                // Diagnostics/spell marks are attached only to the segment
                // that isn't a continuation (index-0-relative, same as
                // `seg_text`), so they share that segment's glyph-expansion
                // offset table rather than a separately-recomputed one (#1208).
                let seg_diagnostics = if is_cont {
                    Vec::new()
                } else {
                    line_diagnostics.clone()
                };
                let seg_spell_errors = if is_cont {
                    Vec::new()
                } else {
                    line_spell_errors.clone()
                };
                let (seg_text, seg_spans, seg_diagnostics, seg_spell_errors) = if list_mode {
                    apply_list_glyphs(
                        seg_text,
                        seg_spans,
                        seg_diagnostics,
                        seg_spell_errors,
                        is_last_seg,
                        &engine.settings,
                    )
                } else {
                    (seg_text, seg_spans, seg_diagnostics, seg_spell_errors)
                };
                lines.push(RenderedLine {
                    raw_text: seg_text,
                    gutter_text: if is_cont {
                        blank_gutter.clone()
                    } else {
                        gutter_text.clone()
                    },
                    is_current_line: line_idx == cursor_line && seg == cursor_seg,
                    spans: seg_spans,
                    is_fold_header: false,
                    folded_line_count: 0,
                    line_idx,
                    git_diff: if is_cont { None } else { git_status },
                    diagnostics: seg_diagnostics,
                    spell_errors: seg_spell_errors,
                    diff_status,
                    is_breakpoint: !is_cont && is_breakpoint,
                    is_conditional_bp: !is_cont && is_conditional_bp,
                    is_dap_current,
                    is_wrap_continuation: is_cont,
                    segment_col_offset: seg_start_char,
                    annotation: join_annotation(
                        // Blame keeps its pre-#1810 placement exactly:
                        // first visual row only, hidden while typing.
                        if is_cont
                            || (engine.mode == crate::core::Mode::Insert
                                && !engine.is_vscode_mode())
                        {
                            None
                        } else {
                            engine.line_annotations.get(&line_idx).cloned()
                        },
                        // Decor eol text goes on the *last* visual row of a
                        // wrapped line — "end of line" means after all of
                        // the line's content, not after its first wrap
                        // segment — and stays visible in Insert mode (it is
                        // plugin-owned inlay/lint text, not the ambient
                        // blame annotation the Insert-mode mute exists for).
                        if is_last_seg {
                            decor_eol_text.as_deref()
                        } else {
                            None
                        },
                    ),
                    ghost_suffix: if line_idx == cursor_line && seg == cursor_seg {
                        ghost_for_cursor_line.clone()
                    } else {
                        None
                    },
                    is_ghost_continuation: false,
                    indent_guides: Vec::new(), // filled below
                    colorcolumns: Vec::new(),  // filled below
                });

                // After the cursor segment, insert ghost continuation rows.
                if line_idx == cursor_line && seg == cursor_seg {
                    for cont in &ghost_continuation_lines {
                        if lines.len() >= visible_lines {
                            break;
                        }
                        lines.push(RenderedLine {
                            raw_text: String::new(),
                            gutter_text: blank_gutter.clone(),
                            is_current_line: false,
                            spans: Vec::new(),
                            is_fold_header: false,
                            folded_line_count: 0,
                            line_idx,
                            git_diff: None,
                            diagnostics: Vec::new(),
                            spell_errors: Vec::new(),
                            diff_status: None,
                            is_breakpoint: false,
                            is_conditional_bp: false,
                            is_dap_current: false,
                            is_wrap_continuation: true,
                            segment_col_offset: 0,
                            annotation: None,
                            ghost_suffix: Some(cont.clone()),
                            is_ghost_continuation: true,
                            indent_guides: Vec::new(),
                            colorcolumns: Vec::new(),
                        });
                    }
                }
            }
        } else {
            // 'list' glyphs never apply to fold-header summary lines (#1208)
            // — real vim's 'list' doesn't touch the folded-line-count text,
            // and this branch is also where non-wrapped lines land, so the
            // gate has to live here rather than on `list_mode` alone.
            let (line_str, spans, line_diagnostics, line_spell_errors) =
                if list_mode && !is_fold_header {
                    apply_list_glyphs(
                        line_str,
                        spans,
                        line_diagnostics,
                        line_spell_errors,
                        true,
                        &engine.settings,
                    )
                } else {
                    (line_str, spans, line_diagnostics, line_spell_errors)
                };
            lines.push(RenderedLine {
                raw_text: line_str,
                gutter_text,
                is_current_line: line_idx == cursor_line,
                spans,
                is_fold_header,
                folded_line_count,
                line_idx,
                git_diff: git_status,
                diagnostics: line_diagnostics,
                spell_errors: line_spell_errors,
                diff_status,
                is_breakpoint,
                is_conditional_bp,
                is_dap_current,
                is_wrap_continuation: false,
                segment_col_offset: 0,
                // Same split as the wrapped branch above: blame is muted in
                // Insert mode, decor eol text is not (#1810).
                annotation: join_annotation(
                    if engine.mode == crate::core::Mode::Insert && !engine.is_vscode_mode() {
                        None
                    } else {
                        engine.line_annotations.get(&line_idx).cloned()
                    },
                    decor_eol_text.as_deref(),
                ),
                ghost_suffix: if line_idx == cursor_line {
                    ghost_for_cursor_line.clone()
                } else {
                    None
                },
                is_ghost_continuation: false,
                indent_guides: Vec::new(), // filled below
                colorcolumns: Vec::new(),  // filled below
            });

            // After the cursor line, insert ghost continuation rows.
            if line_idx == cursor_line {
                let blank_gutter = " ".repeat(gutter_char_width);
                for cont in &ghost_continuation_lines {
                    if lines.len() >= visible_lines {
                        break;
                    }
                    lines.push(RenderedLine {
                        raw_text: String::new(),
                        gutter_text: blank_gutter.clone(),
                        is_current_line: false,
                        spans: Vec::new(),
                        is_fold_header: false,
                        folded_line_count: 0,
                        line_idx,
                        git_diff: None,
                        diagnostics: Vec::new(),
                        spell_errors: Vec::new(),
                        diff_status: None,
                        is_breakpoint: false,
                        is_conditional_bp: false,
                        is_dap_current: false,
                        is_wrap_continuation: true,
                        segment_col_offset: 0,
                        annotation: None,
                        ghost_suffix: Some(cont.clone()),
                        is_ghost_continuation: true,
                        indent_guides: Vec::new(),
                        colorcolumns: Vec::new(),
                    });
                }
            }
        }

        // #1517: in-buffer inline review — right after the last line of
        // any still-outstanding hunk, paint a virtual `[a] keep  [r]
        // reject` action row (no buffer content of its own, same
        // `is_wrap_continuation: true` convention the ghost-completion
        // continuation rows above use to stay invisible to cursor
        // targeting — `render.rs`'s `l.line_idx == ec.line &&
        // !l.is_wrap_continuation` cursor-row lookup already skips it).
        if lines.len() < visible_lines {
            // `hunk_last_right_line_0based` is derived from `str::split
            // ('\n')` row indices (`quadraui::compute_hunks`'s own
            // convention), which has one *more* element than a ropey
            // buffer's real line count whenever the file ends in `\n`
            // (virtually every file) — a trailing phantom empty "row" a
            // hunk's context can extend into at end-of-file. Clamped to
            // the buffer's real last line so the action row lands there
            // instead of never matching any real `line_idx` at all (the
            // phantom index is one past every line the outer loop ever
            // visits).
            let clamped_total = total_lines.saturating_sub(1);
            if let Some(hunk) = inline_review_hunks.iter().find(|h| {
                crate::core::acp_turn::hunk_last_right_line_0based(h).map(|l| l.min(clamped_total))
                    == Some(line_idx)
            }) {
                let edited = buffer_state.file_path.as_deref().is_some_and(|p| {
                    engine.acp_inline_review_hunk_edited(&p.to_string_lossy(), hunk)
                });
                let label = if edited {
                    "  [a] keep (edited)   [r] revert to pre-turn"
                } else {
                    "  [a] keep hunk   [r] reject hunk"
                };
                lines.push(RenderedLine {
                    raw_text: label.to_string(),
                    gutter_text: " ".repeat(gutter_char_width),
                    is_current_line: false,
                    spans: Vec::new(),
                    is_fold_header: false,
                    folded_line_count: 0,
                    line_idx,
                    git_diff: None,
                    diagnostics: Vec::new(),
                    spell_errors: Vec::new(),
                    diff_status: None,
                    is_breakpoint: false,
                    is_conditional_bp: false,
                    is_dap_current: false,
                    is_wrap_continuation: true,
                    segment_col_offset: 0,
                    annotation: None,
                    ghost_suffix: None,
                    is_ghost_continuation: false,
                    indent_guides: Vec::new(),
                    colorcolumns: Vec::new(),
                });
            }
        }

        // Jump past the fold body for fold headers.
        if let Some(fold) = view.fold_at(line_idx) {
            line_idx = fold.end + 1;
        } else {
            line_idx += 1;
        }
    }

    // Cursor (only if visible) — find its index in the rendered lines array.
    let cursor = if is_active {
        lines
            .iter()
            .enumerate()
            .find(|(_, l)| l.is_current_line)
            .map(|(view_line, l)| {
                let shape = if engine.pending_key == Some('r') {
                    CursorShape::Underline
                } else if engine.is_vscode_mode() {
                    CursorShape::Bar
                } else {
                    match engine.mode {
                        Mode::Insert => CursorShape::Bar,
                        _ => CursorShape::Block,
                    }
                };
                // #1653: inline virtual text anchored before the cursor's
                // buffer column on this line shifted the painted text right
                // by its character count — the cursor's painted column has
                // to follow, even though `view.cursor.col` itself stays in
                // buffer coordinates (untouched by any decoration).
                let inline_shift: usize = inline_shifts
                    .get(&l.line_idx)
                    .map(|shifts| {
                        shifts
                            .iter()
                            .filter(|&&(anchor, _)| anchor <= view.cursor.col)
                            .map(|&(_, len)| len)
                            .sum()
                    })
                    .unwrap_or(0);
                // When wrapping, the cursor col is relative to the segment start.
                let col = (view.cursor.col + inline_shift).saturating_sub(l.segment_col_offset);
                (CursorPos { view_line, col }, shape)
            })
    } else {
        None
    };

    // Secondary cursors — map each extra cursor to its view_line + col.
    let extra_cursors: Vec<CursorPos> = view
        .extra_cursors
        .iter()
        .filter_map(|ec| {
            lines
                .iter()
                .enumerate()
                .find(|(_, l)| l.line_idx == ec.line && !l.is_wrap_continuation)
                .map(|(view_line, l)| {
                    let col = ec.col.saturating_sub(l.segment_col_offset);
                    CursorPos { view_line, col }
                })
        })
        .collect();

    // Visual selection (only for active window)
    let selection = if is_active {
        build_selection(engine, scroll_top, visible_lines)
    } else {
        None
    };

    // Yank highlight (only for active window)
    let yank_highlight = if is_active {
        engine.yank_highlight.map(|(start, end, is_linewise)| {
            let (s, e) = if (start.line, start.col) <= (end.line, end.col) {
                (start, end)
            } else {
                (end, start)
            };
            SelectionRange {
                kind: if is_linewise {
                    SelectionKind::Line
                } else {
                    SelectionKind::Char
                },
                start_line: s.line,
                start_col: s.col,
                end_line: e.line,
                end_col: e.col,
            }
        })
    } else {
        None
    };

    // Maximum line length across the whole buffer. When wrap is on, there is no
    // horizontal scrolling, so we report 0 to suppress the horizontal scrollbar.
    let is_md_preview = engine.md_preview_links.contains_key(&window.buffer_id);
    let max_col = if engine.settings.wrap || is_md_preview {
        0
    } else {
        buffer_state.max_col
    };

    // diagnostic_gutter is already built in the single-pass pre-indexing above.

    // ── Indent guides ──────────────────────────────────────────────────────
    let tabstop = engine.settings.tabstop.max(1) as usize;
    let mut active_indent_col: Option<usize> = None;
    if engine.settings.indent_guides {
        // Compute the indent level for each visible line (in columns).
        let line_indents: Vec<Option<usize>> = lines
            .iter()
            .map(|l| {
                if l.is_ghost_continuation || l.is_wrap_continuation {
                    return None; // not a real line for indent purposes
                }
                let text = &l.raw_text;
                let mut cols = 0usize;
                for ch in text.chars() {
                    match ch {
                        ' ' => cols += 1,
                        '\t' => cols += tabstop - (cols % tabstop),
                        _ => break,
                    }
                }
                // Blank lines (only whitespace/newline) return None so guides bridge
                let trimmed = text.trim_start();
                let non_ws = !trimmed.is_empty() && trimmed != "\n" && trimmed != "\r\n";
                if non_ws {
                    Some(cols)
                } else {
                    None // blank line — will be bridged
                }
            })
            .collect();

        // Determine active guide column from cursor line indent
        if let Some(cursor_pos) = &cursor {
            let cursor_view_line = cursor_pos.0.view_line;
            if cursor_view_line < line_indents.len() {
                if let Some(indent) = line_indents[cursor_view_line] {
                    // Active guide is the highest tabstop ≤ cursor indent
                    if indent >= tabstop {
                        let guide_col = (indent / tabstop) * tabstop;
                        // Use the guide one level below if cursor indent is exact multiple
                        active_indent_col = Some(guide_col - tabstop);
                    }
                }
            }
        }

        // Assign indent guides per line, bridging blank lines
        for (i, line) in lines.iter_mut().enumerate() {
            if line.is_ghost_continuation {
                continue;
            }
            let indent = match line_indents[i] {
                Some(ind) => ind,
                None => {
                    // Blank line: bridge using min indent of surrounding non-blank lines
                    let above = line_indents[..i].iter().rev().find_map(|x| *x).unwrap_or(0);
                    let below = line_indents[i + 1..].iter().find_map(|x| *x).unwrap_or(0);
                    above.min(below)
                }
            };
            let mut guides = Vec::new();
            let mut col = tabstop;
            while col <= indent {
                guides.push(col - tabstop); // guide at the start of each tabstop level
                col += tabstop;
            }
            line.indent_guides = guides;
        }
    }

    // ── Color columns ──────────────────────────────────────────────────────
    let cc_positions = engine.settings.colorcolumn_positions();
    if !cc_positions.is_empty() {
        for line in lines.iter_mut() {
            line.colorcolumns = cc_positions.clone();
        }
    }

    // ── Bracket match positions ────────────────────────────────────────────
    let mut bracket_match_positions = if engine.settings.match_brackets && is_active {
        if let Some((match_line, match_col)) = engine.bracket_match {
            let mut positions = Vec::with_capacity(2);
            // Cursor bracket position
            let cursor_line_idx = view.cursor.line;
            let cursor_col_idx = view.cursor.col;
            for (vi, l) in lines.iter().enumerate() {
                if l.line_idx == cursor_line_idx
                    && !l.is_ghost_continuation
                    && !l.is_wrap_continuation
                {
                    positions.push((vi, cursor_col_idx.saturating_sub(l.segment_col_offset)));
                }
                if l.line_idx == match_line && !l.is_ghost_continuation && !l.is_wrap_continuation {
                    positions.push((vi, match_col.saturating_sub(l.segment_col_offset)));
                }
            }
            positions.dedup();
            positions
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // `'showmatch'` (#1207): while `Engine::showmatch_flash` is armed (the one
    // frame right after a matched closing bracket was typed in Insert mode —
    // cleared at the top of the *next* `handle_insert_key`, see the field's
    // doc comment on `Engine`), highlight the opening bracket it matched with
    // the same background used for normal-mode `'matchpairs'` highlighting
    // (`bracket_match_bg`). This reuses the existing quadraui-consumed
    // `bracket_match_positions` channel rather than inventing a new one, so
    // both backends pick it up for free through `to_q_editor`.
    if is_active && engine.settings.showmatch {
        if let Some((match_line, match_col)) = engine.showmatch_flash {
            for (vi, l) in lines.iter().enumerate() {
                if l.line_idx == match_line && !l.is_ghost_continuation && !l.is_wrap_continuation {
                    let pos = (vi, match_col.saturating_sub(l.segment_col_offset));
                    if !bracket_match_positions.contains(&pos) {
                        bracket_match_positions.push(pos);
                    }
                }
            }
        }
    }

    // Extra selections for Ctrl+D multi-cursor word selections.
    // Each extra cursor sits at the END of a word; derive selection start
    // from the primary selection length.
    let extra_selections = if is_active && !view.extra_cursors.is_empty() {
        if let Some(sel) = selection
            .as_ref()
            .filter(|s| s.kind == SelectionKind::Char && s.start_line == s.end_line)
        {
            let sel_len = sel.end_col + 1 - sel.start_col; // inclusive
            view.extra_cursors
                .iter()
                .map(|ec| SelectionRange {
                    kind: SelectionKind::Char,
                    start_line: ec.line,
                    start_col: ec.col + 1 - sel_len,
                    end_line: ec.line,
                    end_col: ec.col,
                })
                .collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    // ── Sticky scroll (#1546) ────────────────────────────────────────────
    // Pin the header lines of `scroll_top`'s enclosing scopes at the top of
    // the viewport by splicing them directly over `lines[0..k]` — reusing
    // the exact same per-row paint + click-to-jump machinery every other
    // row already goes through (`RenderedLine::line_idx` is what click
    // hit-testing resolves screen row -> buffer line with). No backend-
    // specific overlay code, no new quadraui primitive: the covered rows
    // *are* the pinned band. See `sticky_scroll::enclosing_scope_headers`'s
    // doc for the scope-source scoping (indent-hierarchy fallback only;
    // LSP `documentSymbol` as the primary source is a deferred follow-up).
    //
    // Applied after indent guides / colorcolumns / bracket-match above, so
    // spliced header rows don't carry those (cosmetic-only gap — they still
    // get full syntax highlighting via `build_spans` below). Never covers
    // the cursor's own row: if the cursor is already at (or above) a row a
    // header would occupy, this is a no-op for that frame rather than
    // hiding the cursor.
    if engine.settings.sticky_scroll && scroll_top > 0 && !lines.is_empty() {
        let headers = crate::core::engine::sticky_scroll::enclosing_scope_headers(
            buffer,
            scroll_top,
            engine.settings.shift_width as usize,
        );
        let k = headers.len().min(lines.len());
        if k > 0 && !lines[..k].iter().any(|l| l.is_current_line) {
            let is_markdown = buffer_state
                .file_path
                .as_ref()
                .and_then(|p| p.to_str())
                .and_then(crate::core::syntax::SyntaxLanguage::from_path)
                == Some(crate::core::syntax::SyntaxLanguage::Markdown);
            for (row, &header_line) in headers.iter().take(k).enumerate() {
                let header_rope_line = buffer.content.line(header_line);
                let header_line_str = header_rope_line.to_string().replace('\0', "");
                let header_start_byte = buffer.content.line_to_byte(header_line);
                let header_end_byte = header_start_byte + header_rope_line.len_bytes();
                let spans = build_spans(
                    engine,
                    theme,
                    &buffer_state.highlights,
                    &buffer_state.semantic_tokens,
                    buffer,
                    header_line,
                    &header_line_str,
                    header_start_byte,
                    header_end_byte,
                    is_markdown,
                    &buf_search_matches,
                    Some(window.buffer_id) == active_buf_id,
                );
                let marker_cols = (if has_bp { 1 } else { 0 }) + (if has_git { 1 } else { 0 });
                let base_gutter = format_gutter_with_fold(
                    line_number_mode,
                    header_line,
                    cursor_line,
                    gutter_char_width.saturating_sub(marker_cols),
                    ' ',
                );
                let gutter_text = format!(
                    "{}{}{}",
                    if has_bp { " " } else { "" },
                    if has_git { " " } else { "" },
                    base_gutter
                );
                lines[row] = RenderedLine {
                    raw_text: header_line_str.trim_end_matches(['\n', '\r']).to_string(),
                    gutter_text,
                    is_current_line: false,
                    spans,
                    is_fold_header: false,
                    folded_line_count: 0,
                    line_idx: header_line,
                    git_diff: None,
                    diagnostics: Vec::new(),
                    spell_errors: Vec::new(),
                    diff_status: None,
                    is_breakpoint: false,
                    is_conditional_bp: false,
                    is_dap_current: false,
                    is_wrap_continuation: false,
                    segment_col_offset: 0,
                    annotation: None,
                    ghost_suffix: None,
                    is_ghost_continuation: false,
                    indent_guides: Vec::new(),
                    colorcolumns: Vec::new(),
                };
            }
        }
    }

    RenderedWindow {
        window_id,
        rect: *rect,
        lines,
        visible_line_capacity: visible_lines.max(1),
        cursor,
        extra_cursors,
        selection,
        extra_selections,
        yank_highlight,
        scroll_top,
        scroll_left: view.scroll_left,
        total_lines,
        gutter_char_width,
        text_viewport_cols: render_viewport_cols,
        minimap_reserved_w,
        is_active,
        show_active_bg: is_active && multi_window,
        has_git_diff: has_git,
        has_breakpoints: has_bp,
        max_col,
        diagnostic_gutter,
        bracket_match_positions,
        active_indent_col,
        tabstop: engine.settings.tabstop.max(1) as usize,
        code_action_lines: {
            // Only show lightbulb on the cursor line (like VSCode) — not on every
            // line that has cached actions, which would be noisy in Rust files where
            // rust-analyzer offers refactors on nearly every line.
            let cl = view.cursor.line;
            let has = canonical_path
                .and_then(|p| engine.lsp_code_actions.get(p))
                .and_then(|m| m.get(&cl))
                .is_some_and(|v| !v.is_empty());
            if has {
                std::collections::HashSet::from([cl])
            } else {
                std::collections::HashSet::new()
            }
        },
        cursorline: engine.settings.cursorline,
        status_line: None,
        plugin_view: None,
    }
}
