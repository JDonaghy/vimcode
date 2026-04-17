# src/core/engine/tests.rs — 25,085 lines

All engine unit and integration tests. ~900 test functions covering every Vim feature, command, motion, text object, and edge case. Includes 356 Neovim-mined conformance tests (`test_nvim_*`).

## Test Helpers
- `engine_with(text)` — create engine with initial buffer content; resets settings and keymaps for hermeticity
- `keys(engine, sequence)` — simulate key sequence (parses `<C-w>`, `<CR>`, etc.)
- `drain_macro_queue(engine)` — flush macro playback after `@register`
- `assert_buf(engine, expected)` — assert buffer content
- `assert_cursor(engine, line, col)` — assert cursor position

## Test Coverage Areas
Normal mode keys, insert mode, visual mode (char/line/block), operators (d/c/y/>/</=), text objects (w/W/"/'/(/[/{/p/s/t + LaTeX), motions (hjkl/w/b/e/0/$/%/f/t/gg/G/{/}/(/)), ex commands (:w/:q/:e/:sp/:vs/:set/:norm/:grep/:map/etc.), branch picker, macros, registers, marks, search (/, ?, n, N, *, #), folding, completion, undo/redo, multi-cursor, repeat (.), git commands, LSP operations, comment toggle, indent, join, number increment, replace mode, and more.
