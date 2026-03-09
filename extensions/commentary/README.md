# VimCode Commentary

Toggle line comments with familiar Vim keybindings.

## Usage

| Key         | Mode   | Action                                |
|-------------|--------|---------------------------------------|
| `gcc`       | Normal | Toggle comment on current line        |
| `3gcc`      | Normal | Toggle comment on 3 lines from cursor |
| `gc`        | Visual | Toggle comment on selected lines      |
| `:Commentary`| Command | Toggle comment on current line       |
| `:Commentary N` | Command | Toggle comment on N lines from cursor |

The comment string is chosen automatically based on the file type (e.g. `//` for
Rust/Go/C/Java, `#` for Python/Ruby/Shell, `--` for Lua/SQL, `%` for LaTeX).

## How it works

- **Commenting:** Prepends the comment string + space to each line, respecting
  existing indentation (the comment marker is inserted after leading whitespace).
- **Uncommenting:** If *all* lines in the range are already commented, the comment
  prefix is stripped. A mix of commented and uncommented lines is treated as
  "not yet commented" and all lines receive the prefix.
- Empty/whitespace-only lines are left untouched.

## Acknowledgements

This plugin is directly inspired by
[vim-commentary](https://github.com/tpope/vim-commentary) by **Tim Pope**,
one of the most beloved and widely-used Vim plugins. Tim's elegant design —
minimal surface area, operator-pending `gc`, and automatic filetype detection —
set the standard for how comment toggling should work in a Vim-like editor.

VimCode Commentary reimplements the core idea using VimCode's Lua plugin API
rather than VimScript. All credit for the original concept and UX design
belongs to Tim Pope and the vim-commentary contributors.

- Original: <https://github.com/tpope/vim-commentary>
- License: Vim License (see the original repository)
