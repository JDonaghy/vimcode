# src/core/engine/keys.rs — 7,533 lines

All keyboard input handling. Routes keys by mode, processes operators, macros, repeat, user keymaps, mouse events, and clipboard.

## Key Methods — Input Dispatch
- `handle_key(key, ctrl, unicode)` — top-level key router; delegates by mode
- `handle_normal_key(key, ctrl, unicode)` — normal mode key handler (motions, operators, commands)
- `handle_insert_key(key, ctrl, unicode)` — insert/replace mode key handler
- `handle_command_key(key, unicode)` — `:` command line input
- `handle_search_key(key, unicode, ctrl)` — `/` and `?` search input
- `handle_visual_key(key, ctrl, unicode)` — visual/visual-line/visual-block mode
- `handle_pending_key(key, ctrl, unicode)` — multi-key sequences (g, z, Ctrl-W, [, ], etc.)
- `handle_leader_key(unicode)` — leader key sequences (<leader>rn, <leader>ca, etc.)

## Key Methods — Operators & Motions
- `handle_operator_motion(op, key, ctrl, unicode)` — applies operator (d/c/y/>/</=) with motion
- `apply_charwise_operator(op, start, end)` — char-range operator application
- `apply_linewise_operator(op, start_line, end_line)` — line-range operator application
- `apply_operator_text_object(op, kind, inner)` — text object operator (iw, a", etc.)
- `apply_case_range(start, end)` — toggle case over range
- `apply_rot13_range(start, end)` — ROT13 encode range

## Key Methods — Macros & Repeat
- `advance_macro_playback()` — process next key from macro queue
- `decode_macro_sequence()` — parse macro register content into key events
- `repeat_last_change(count)` — `.` command replay
- `parse_key_sequence(seq)` — parse `<C-w>`, `<CR>` style sequences

## Key Methods — Completion & Keymaps
- `complete_command(partial)` — tab completion for ex commands
- `rebuild_user_keymaps()` — parse settings.keymaps into lookup table
- `try_user_keymap(mode, key)` — check for user remapping before built-in
- `available_commands()` — list of all ex command names
- `setting_names()` — list of all setting names

## Key Methods — Mouse & Clipboard
- `mouse_click(window_id, line, col)` — handle editor click
- `mouse_drag(window_id, line, col)` — handle editor drag
- `mouse_double_click(window_id, line, col)` — word selection on double-click
- `paste_clipboard_to_input()` — paste into active input field
- `load_clipboard_register(text)` — load system clipboard into `"` register
- `feed_keys(keys)` — inject key sequence string (Neovim notation: `<Esc>`, `<CR>`, `<C-a>`)
