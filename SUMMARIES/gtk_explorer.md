# src/gtk/explorer.rs — 252 lines

GTK explorer sidebar state + data adapter for Phase A.2b migration to a `DrawingArea` + `quadraui_gtk::draw_tree` render path. Sub-phase 1 ships this module as inert scaffolding (no callsites yet); sub-phase 2 flips the wiring.

## Key Types
- `ExplorerRow { depth, name, path, is_dir, is_expanded }` — one visible row in the flat explorer list. Mirrors TUI's `ExplorerRow`.
- `ExplorerState { rows, expanded: HashSet<PathBuf>, selected, scroll_top }` — per-panel state owned by the GTK `App`.

## Key Functions
- `ExplorerState::new(root)` — initialise with root expanded.
- `ExplorerState::rebuild(root, show_hidden, case_insensitive)` — re-walk the filesystem into `rows`.
- `ExplorerState::toggle_dir(idx, ...)` — expand/collapse the directory at flat index `idx`.
- `ExplorerState::ensure_visible(viewport_rows)` — clamp `scroll_top` so `selected` is in-view.
- `ExplorerState::reveal_path(target, root, viewport_rows, ...)` — expand ancestors, rebuild, select target, scroll in.
- `build_explorer_rows(root, expanded, show_hidden, case_insensitive) -> Vec<ExplorerRow>` — filesystem walker (dirs first, alphabetical).
- `explorer_to_tree_view(state, has_focus, engine) -> quadraui::TreeView` — adapts rows + engine indicators (git status, LSP diagnostics) into the primitive.
