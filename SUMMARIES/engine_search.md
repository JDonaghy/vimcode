# src/core/engine/search.rs — 1,209 lines

Cursor helpers, search helpers (word under cursor, word-bounded search), and the unified find/replace overlay (Ctrl+F).

## Key Methods — Cursor & Search Helpers
- `get_max_cursor_col()`, `clamp_cursor_col()` — cursor column bounds
- `ensure_cursor_visible()`, `ensure_cursor_visible_wrap()` — viewport scroll
- `scroll_cursor_center()`, `scroll_cursor_top()`, `scroll_cursor_bottom()` — viewport centering
- `word_under_cursor()` — extract word at cursor position
- `search_word_under_cursor()` — `*` / `#` word search
- `build_word_bounded_matches()` — whole-word match filtering

## Key Methods — Find/Replace Overlay
- `open_find_replace()` — open overlay, capture visual selection (single-line → query, multi-line → in_selection), pre-fill from search_query
- `close_find_replace()` — close overlay, reset in_selection state
- `run_find_replace_search()` — populate search_matches using FindReplaceOptions (case, word, regex with multiline, in_selection filtering)
- `find_replace_next()` / `find_replace_prev()` — navigate matches
- `find_replace_replace_current()` — replace match at search_index, advance cursor past replacement
- `find_replace_replace_all()` — replace in buffer (respects in_selection range)
- `toggle_find_replace_case/whole_word/regex/preserve_case/in_selection()` — toggle options
- `fr_input_selection()` / `fr_delete_selection()` — input field text selection helpers
- `handle_find_replace_key()` — key handler: Escape, Return, Up/Down, Tab, BackSpace, Delete, Left/Right/Home/End, Ctrl+A/V/Z/H, Alt+C/W/R, printable char insertion (all support selection-replaces-text)

## Key Methods — Project Search
- `start_project_search()` / `poll_project_search()` / `apply_search_results()` — async background search
- `start_project_replace()` / `poll_project_replace()` / `apply_replace_result()` — async background replace
- `toggle_project_search_case/whole_word/regex()` — project search toggles
- `project_search_select_next/prev()` — result navigation
