# src/core/engine/buffers.rs — 3,244 lines

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

## Undo/Redo
- `undo()` / `redo()` — undo/redo operations
- `start_undo_group()` / `finish_undo_group()` — group edits for atomic undo
- `record_edit(line, old, new)` — record edit for undo history

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
