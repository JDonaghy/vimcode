# src/core/engine/motions.rs — 4,628 lines

Cursor movement, text objects, word/paragraph/sentence navigation, bracket matching, completion, code folding, indent/format, and delete operations.

## Movement
- `move_left/right/up/down` — basic cursor movement
- `move_word_forward/backward` — w/b word motions
- `move_bigword_forward/backward` — W/B WORD motions
- `move_word_end/move_word_end_backward` — e/ge motions
- `move_bigword_end/move_bigword_end_backward` — E/gE motions
- `move_paragraph_forward/backward` — {/} paragraph motions
- `move_sentence_forward/backward` — (/( sentence motions
- `move_visual_down/up` — gj/gk wrapped-line motions

## Text Objects
- `find_text_object_range(kind, inner)` — dispatcher for all text objects
- `find_word_object` — iw/aw
- `find_bigword_object` — iW/aW
- `find_quote_object` — i"/a"/i'/a'/i`/a`
- `find_bracket_object` — i(/a(/i[/a[/i{/a{
- `find_paragraph_object` — ip/ap
- `find_sentence_object` — is/as
- `find_tag_text_object` — it/at (HTML/XML)
- `find_latex_environment_object` — LaTeX \begin{}\end{}
- `find_latex_command_object` — LaTeX \command{}
- `find_latex_math_object` — LaTeX $...$, $$...$$

## Bracket & Search
- `move_to_matching_bracket` — % motion
- `find_matching_bracket(line, col)` — bracket pair finder
- `update_bracket_match` — highlight matching bracket
- `search_forward_for_bracket` — find next bracket on line

## Editing
- `delete_lines(count)` — dd with count
- `delete_to_end_of_line` — D motion
- `increment_number_at_cursor(delta)` — Ctrl-A/Ctrl-X
- `auto_indent_lines(line, count)` — = operator
- `toggle_comment(start, end)` — comment/uncomment lines
- `format_lines(start, end)` — gq format operator
- `join_lines_no_space(count)` — gJ join without spaces
- `handle_replace_key(key, ctrl, unicode)` — r/R replace mode
- `paste_after/before_adjusted_indent` — ]p/[p indent-adjusted paste

## Completion
- `trigger_auto_completion` — start completion popup
- `apply_completion_candidate(idx)` — accept completion item
- `dismiss_completion` — close completion popup
- `completion_prefix_at_cursor` — extract word prefix for matching
- `word_completions_nearby/for_prefix` — buffer word scan

## Folding
- `toggle_fold_at_line` — za
- `cmd_fold_close/open/toggle` — zc/zo/za
- `cmd_fold_close_all` — zM
- `cmd_fold_open/close_progressive` — zr/zm
- `cmd_fold_create(start, end)` — zf (manual fold)
- `detect_fold_range(line)` — indent-based fold detection
