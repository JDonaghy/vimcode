# src/gtk/explorer.rs — 4 lines (placeholder)

**Empty module.** All explorer state and logic moved to the engine
(`src/core/engine/explorer_ops.rs`), driven by `quadraui::TreeController` (#415,
quadraui#193) — selection, scroll, keyboard nav, inline rename/new-file editing
and scrollbar interaction are all shared, with zero per-backend explorer code.
The file is kept only so the module path still resolves.

Look in `core/engine/explorer_ops.rs` for the row model, the filesystem walk and
`reveal_path`; in `render.rs` for the `TreeView` adapter; and in
`quadraui::TreeController` for the interaction model.
