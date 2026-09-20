# src/core/engine/buffers.rs — 4,023 lines

File I/O, buffer management, syntax updates, undo/redo, git diff, markdown preview, netrw directory browser, and workspace operations.

## File Operations
- `open_file(path)` / `open_file_in_tab(path)` / `open_file_preview(path)` — file opening variants
- `open_file_with_mode(path, mode)` — open with specific split mode
- `save()` / `save_as(path)` — write buffer to disk
- `save_all()` — save all dirty buffers
- `reload_file()` — re-read file from disk
- `close_buffer(id)` — close and clean up buffer

## Buffer State
- `syntax_update()` — re-parse tree-sitter syntax for current buffer
- `refresh_git_diff()` — update git line status markers
- `tick_syntax_debounce()` — debounced syntax refresh (150ms)

## Undo/Redo (#1156: real undo tree, not a linear stack — see `buffer_manager::UndoTree`)
- `undo()` / `redo()` — undo/redo, thin wrappers over `BufferState::undo`/`redo`
- `start_undo_group()` / `start_undo_group_at(cursor)` / `finish_undo_group()` — group edits into one undo-tree node
- `g_earlier()` / `g_later()` — `g-`/`g+`, cross branches (unlike plain `u`/`<C-r>`)
- `ex_earlier(spec)` / `ex_later(spec)` — `:earlier`/`:later`, count or `{N}[smhd]` time offset
- `report_undo_nav(cursor, label)` — shared side effects (cursor/dirty/message) for every undo-tree jump
- `parse_undo_time_spec(spec)` (free fn) — parses `{N}[smhd]` into a `SystemTime` cutoff

## Navigation
- `switch_buffer(id)` — switch active window to buffer
- `switch_window_buffer(buf_id)` — show buffer in current window
- `netrw_open(dir)` / `netrw_activate_entry()` — directory browser

## Markdown & Diff
- `open_markdown_preview()` — side-by-side rendered markdown
- `open_diff_view(path)` — git diff view
- `sc_open_selected_async()` — async diff for source control panel

## Inline New File/Folder
- `start_explorer_new_file(parent_dir)` — begin inline new-file entry in explorer
- `start_explorer_new_folder(parent_dir)` — begin inline new-folder entry in explorer
- `handle_explorer_new_entry_key(key, unicode, ctrl)` — key dispatch for inline creation (Enter creates, Escape cancels)

## Workspace
- `open_folder(path)` — change working directory
- `add_workspace_folder(path)` — multi-root workspace
