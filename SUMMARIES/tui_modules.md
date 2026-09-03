# TUI Backend Modules

> **Shape as of #766.** `fn event_loop` was deleted by #634 and `draw_frame` by
> #766 — `TuiShellApp` *is* the TUI, driven by
> `quadraui::tui::shell_runner::run_with_shell`. `backend.rs`, `events.rs` and
> `services.rs` were lifted into `quadraui::tui::*` (#268) and are now shims of
> 3–7 lines. Since #751–#766 every routing and composition decision is made in
> `render.rs`; these files are the wiring.

## src/tui_main/shell_app.rs — 11,172 lines (~3,989 production)
**The live TUI.** `pub struct TuiShellApp` + `impl ShellApp for TuiShellApp`.
- Entry points: `setup()`, `render_content()`, `handle()`, `tick()`
- Composition rungs: `compose_bottom_band_rungs()`, `paint_editor_band()` — the
  TUI halves of the shared `render::compose_frame` walk
- `handle_mouse_event()` — delegates into the shared mouse routers
- `on_shell_event()` / `on_shell_event_ctx()` — `AppShell` callbacks (panel
  switch, sidebar hide, hamburger)
- `take_requested_panel()`, `sync_ext_activity_panels()`, `activate_ext_panel()`
- `shell_config()` — activity-bar panels, chrome metrics, accelerators
- `new()` / `new_for_test()` — the latter skips workspace session restore so
  tests are ambient-state-free (#758)
- `TuiAccelHost` — `impl render::PanelAcceleratorHost`, the TUI half of the
  shared panel-accelerator rung (#761)
- `KeyDispatchState` — scratch state threaded through key dispatch

Line references of the form `mirrors mod.rs:NNNN` inside this file point at the
**deleted** `event_loop()` at its final revision (`509b8fe`); read them with
`git show 509b8fe:src/tui_main/mod.rs`. #734 exists because those pointers had
all drifted.

## src/tui_main/mouse.rs — 3,795 lines (~2,895 production)
`handle_mouse()` — the single mouse entry point, routing clicks/drags/scrolls
into the shared `render::` routers. Local helpers: `scrollbar_grab_offset`,
`apply_scrollbar_drag`, `apply_tui_sidebar_body_drag`, `apply_tui_editor_text_drag`,
`text_drag_widget_id`, `editor_hover_popup_link_rects`, `route_and_apply_chrome_click`.

Carries two of the nine recorded *"one-sided / do not converge"* verdicts (#751
and #752) — read them before trying to converge those rungs again.

## src/tui_main/render_impl.rs — 2,620 lines (~1,267 production)
Screen bridging and the paint helpers `render_content` calls.
- `build_screen_for_shell_content()`, `bottom_chrome_rects_for_shell_content()`
- `render_all_windows`, `render_window`, `render_window_status_line`
- `render_separators`, `render_group_dividers`, `group_divider_cells`,
  `draw_rule_row_themed` / `draw_rule_row_q`
- `render_tab_drag_overlay`, `render_tab_hover_tooltip`, `compute_tui_tab_drop_zone`
- `paint_editor_popups`, `render_picker_popup`, `folder_picker_to_palette`
- `char_col_to_visual`

`draw_frame()` is **gone** (#766). `build_screen_for_tui()` survives only under
`#[cfg(test)]`.

## src/tui_main/panels.rs — 1,616 lines (~1,208 production)
Sidebar panel rendering: activity bar, explorer, git, debug, extensions, AI,
search, terminal. 16 functions.

## src/tui_main/mod.rs — 976 lines (~933 production)
Module wiring plus: `run()` (builds the shell config and calls `run_with_shell`),
`TuiSidebar`, `FolderPickerState` / `FolderPickerMode` and its directory walk +
fuzzy filter, `setup_tui_clipboard` / `sync_tui_clipboard`,
`register_panel_accelerators`, terminal row/column helpers, `init_debug_log`, and
the `pub mod testing` re-export used by the acceptance crate.

## src/tui_main/quadraui_tui.rs — 102 lines
The few remaining `draw_*` wrappers not yet routed through a `Backend::draw_*`
trait method. #600 removed the wrappers for `ContextMenu`, `Completions`,
`Dialog`, `Tooltip`, `FindReplacePanel` and `RichTextPopup`.

## src/tui_main/backend.rs / events.rs / services.rs — 7 / 5 / 3 lines
Re-export and placeholder shims; the real `TuiBackend`, event translators and
`TuiPlatformServices` live in `quadraui::tui::*` (#268).
