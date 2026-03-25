# Known Bugs

## GTK Scrollbar / Tab Group Divider Overlap

When two or more editor tab groups are side by side, the vertical scrollbar of the left group extends slightly beyond the group boundary into the adjacent group's space. This is because the GTK scrollbar overlay widget is not clipped to the window rect.

Additionally, a capture-phase gesture on the overlay intercepts divider drags before the scrollbar receives them (6px hit zone). This means it is easy to accidentally start resizing the tab groups when intending to click-to-scroll on the scrollbar near the divider boundary.

## Resolved

- **GTK core dump from draw callback panic** — Panic in extern "C" `set_draw_func` callback aborts the process. Fixed with `catch_unwind` wrapper + replacing all `.unwrap()` with `.ok()` on Cairo fill/stroke/save/restore/paint operations.
- **GStrInteriorNulError from NUL hotkey** — `DialogButton.hotkey: '\0'` produced NUL in button label string via `format_button_label()`. Fixed to return label unchanged when `hotkey == '\0'`. Also sanitized NUL bytes from file content in render.
- **Lightbulb duplication on wrapped lines** — Code action lightbulb icon rendered on every wrapped continuation line. Fixed with `is_wrap_continuation` guard in both GTK and TUI backends.
- **Phantom "Loading..." hover popup** — Mouse hover triggered "Loading..." popup → LSP null response → dismiss → dwell re-fires → infinite flash loop. Fixed: mouse hover no longer shows "Loading..." (popup appears only when LSP returns content); null-position suppression prevents re-request loops; keyboard hover (`gh`) retains "Loading..." with 3s auto-dismiss. Also: only installed extensions can start LSP servers (built-in registry gated).

- **Flatpak build broken** — `cargo-sources.json` had stale vendored crates (tree-sitter 0.24.7) while `Cargo.lock` required 0.26.7. Regenerated with `flatpak-cargo-generator.py`. Going forward, re-run the generator whenever `Cargo.lock` changes.
- **Extension "u" update key does nothing** — `ext_sidebar_selected` is a flat index across installed + available sections, but key handlers compared it directly against `installed.len()` without accounting for collapsed sections. When installed section was collapsed, `sel=0` mapped to `installed[0]` instead of `available[0]`. Fixed with `ext_selected_to_section()` helper that correctly maps flat index to `(is_installed, index_within_section)`. Updated all 5 affected handlers: "Tab", "i", "d", "u", and `ext_open_selected_readme()`.

- **Search panel input broken** — TUI click handler for search panel didn't set `sidebar.has_focus = true`, so keystrokes fell through to editor. Fixed.
- **Git insights hover on non-cursor lines** — `clear_annotations()` cleared `line_annotations` but not `editor_hover_content`, so old hover content accumulated for every visited line. Fixed.
- **Semantic tokens disappear after hover popup** — LSP server returning `result: null` for semantic token requests (e.g. when busy with hover) was parsed as empty tokens and stored, wiping existing highlighting. Fixed by only accepting responses with an actual `data` array. Additionally, stale position-based data (semantic tokens + diagnostics) cleared on edit to prevent wrong-line highlighting.
- **Terminal backspace key-hold batching** — `poll_terminal()` only ran during idle time, so held-key output wasn't rendered until release. Fixed by polling immediately after `terminal_write()`.
- **Sidebar scrollbar drag leaks to other handlers** — TUI: explorer/ext panel scrollbar drags had no persistent state, so mouse-move events leaked to editor text selection. Fixed with `dragging_generic_sb` state variable. GTK: ext panel scrollbar `GestureDrag` never claimed the event sequence, allowing the sidebar resize gesture to steal the drag. Fixed by claiming in `drag_begin`.
- **Tab hover tooltip** — Hovering over a tab now shows the file's full path with `~` shortening for the home directory. GTK: Cairo-drawn popup below the tab bar. TUI: overlay text on the breadcrumbs row. Also fixed `tab_close_hit_test` Y-coordinate bug when breadcrumbs are enabled (`grect.y - line_height` → `grect.y - tab_bar_height`).
