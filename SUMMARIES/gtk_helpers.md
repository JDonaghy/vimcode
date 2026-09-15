# GTK Helper Files

> Most of what this file used to describe was **lifted into quadraui** (#270).
> `backend.rs`, `events.rs`, `services.rs` and `explorer.rs` are now re-export or
> placeholder shims of 3–15 lines each; the real implementations live in
> `quadraui::gtk::*` and `engine/explorer_ops.rs`. Read them there.

## src/gtk/backend.rs — 7 lines
`pub use quadraui::gtk::GtkBackend;` — kept so `use super::backend::GtkBackend`
call sites still compile.

> **The `GtkBackend` Rc-handle asymmetry lives here conceptually and blocks the
> macOS port.** `App` calls `modal_stack_handle()` / `drag_state_handle()` at 44
> call sites; those are *inherent* methods on the concrete struct, not on the
> generic `quadraui::Backend` trait, and `MacBackend` has no equivalent. See
> `PLAN.md`, "#47 re-audit findings".

## src/gtk/events.rs — 15 lines
Re-exports the GDK ↔ `UiEvent` translators from `quadraui::gtk::events`. Since
#448-C the shell runner owns input translation; these survive for sidebar-panel
drawing areas.

## src/gtk/services.rs — 3 lines
Placeholder. `GtkPlatformServices` lives in `quadraui::gtk::services`;
`GtkBackend` constructs it internally.

## src/gtk/explorer.rs — 4 lines
Placeholder. All explorer state and logic is on the engine
(`core/engine/explorer_ops.rs`), driven by `quadraui::TreeController`.

## src/gtk/click.rs — 1,634 lines
- `pixel_to_click_target()` — pixel → click target, delegating zone resolution to
  `render::screen_zone_hit_test` / `window_zone_hit_test`
- `tab_bar_inner_hit_test`, `resolve_pixel_tab_click`, `resolve_charcell_tab_click`
  — tab-bar slot resolution; the Pango-vs-char-cell split is one of the
  deliberately-not-converged rungs
- `resolve_tab_right_click`, `dispatch_tab_bar_target`, `execute_gutter_action`
- `handle_mouse_click`, `handle_mouse_double_click`, `handle_mouse_drag`
- `build_editor_click_context()` — Pango context for click-time measurement

## src/gtk/css.rs — 507 lines
`make_theme_css()`, `STATIC_CSS`, `load_css()`. Genuinely GTK-only and expected
to stay per-backend.

## src/gtk/util.rs — 349 lines
`open_url()`, bundled Nerd Font subset install into `~/.local/share/fonts/`, and
assorted GTK utilities.

## src/gtk/testing.rs — 8,019 lines
The in-crate headless GTK black-box harness (#646), behind the `test-support`
feature. Wraps the real `App` in `quadraui::gtk::testing::driver_with_shell` so a
test clicks, types and scrolls against production dispatch and paint code with no
display. The GTK twin of `TuiDriver`. Almost all of its bulk is tests.
