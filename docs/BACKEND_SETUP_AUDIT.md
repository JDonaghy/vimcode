# Backend Setup Audit (Phase B.5d / #260)

Comparison of TUI vs GTK init / event-loop code as it stands on develop after Phase B.5c. Goal: identify which parts are genuinely backend-specific vs. boilerplate that a runner crate (Phase B.5e / #261) can absorb.

This document is the diagnostic step that informs the runner-crate API. It does not introduce any new code.

## 1. Entry-point structure

| | TUI | GTK |
|---|---|---|
| Entry function | `src/tui_main/mod.rs::run(file_path, debug_log_path)` | `relm4::RelmApp::new(...).run::<App>(file_path)` |
| Setup phase | Imperative (raw stdout, alternate screen, panic hook, mouse capture, kbd enhancement, terminal init) | Component lifecycle (`SimpleComponent::init` builds widgets, applies CSS, registers signal callbacks) |
| Run phase | Manual `loop { backend.wait_events(timeout); ...handle...; render }` in `event_loop()` | Relm4 event loop drives `update()` and `view()` against the model |
| Tear-down | Restore terminal state in `restore_terminal()` + emergency swap flush on panic | GTK manages widget destruction; emergency hook registered via `register_emergency_engine` |

**Common shape:**
1. Build the engine.
2. Apply CLI args / restore session.
3. Wire native services (clipboard, file dialogs, panic hook, emergency-flush registration).
4. Initialise the backend (`TuiBackend::new()` / `GtkBackend::new()`).
5. Register accelerators (panel keys etc.).
6. Drive the event loop.

Steps 1–5 are nearly identical in intent. Step 6 looks superficially different but both backends now route through `Backend::wait_events` / `Backend::poll_events` (Phase B.4 / B.5).

## 2. Event flow

| | TUI | GTK |
|---|---|---|
| Event source | `crossterm::event::read()` (blocking, thread-local) | GTK widget signal callbacks, async via Relm4 message queue |
| Trait method | `TuiBackend::wait_events(timeout) -> Vec<UiEvent>` — blocks via `crossterm::event::poll` | `GtkBackend::wait_events(timeout)` — drains an internal `VecDeque<UiEvent>` populated by signal handlers, runs `glib::MainContext::iteration(false)` to give GTK a tick |
| Accelerator translation | `apply_accelerators` runs over the translated events to rewrite `KeyPressed` matching a registered binding into `UiEvent::Accelerator(id, mods)` | Same — done inside `GtkBackend`'s adapter layer |
| Modal stack | `Backend::modal_stack_mut()` — apps push / pop on modal open / close. `quadraui::dispatch::dispatch_mouse_down` consults it before routing | Same; backed by `Rc<RefCell<ModalStack>>` shared across signal callbacks |
| Drag state | `Backend::drag_and_modal_mut()` returns disjoint mutable borrows | Same shape; `Rc<RefCell<DragState>>` because GTK signal callbacks need shared access |

**The key architectural win from B.4 + B.5b:** both backends now expose `wait_events` / `poll_events` as the canonical event source. Apps that consume the trait don't need to know about crossterm or GDK. This is what makes a runner crate possible.

## 3. Frame-loop / draw

| | TUI | GTK |
|---|---|---|
| Driver | `event_loop` calls `terminal.draw(|frame| { … })` once per iteration | Each DrawingArea has a `set_draw_func(da, |da, cr, _, _| { … })` callback invoked by GTK on `queue_draw()` |
| Frame scope | `TuiBackend::enter_frame_scope(frame, |b| { … b.draw_*(...) })` stashes `&mut Frame` for the duration of `f` | `GtkBackend::enter_frame_scope(cr, layout, |b| { … b.draw_*(...) })` stashes `&Context` + `&pango::Layout` |
| Theme sync | `set_current_theme(q_theme(theme))` once before draws | Same |
| Per-frame metrics | `current_line_height` / `current_char_width` are not used in TUI (cells = 1) | Set once per frame from font metrics; consumed by primitives that need pixel-cell mapping |
| Layout caching | `last_layout: Option<ScreenLayout>` cached at end of draw for mouse hit-testing on next iteration | Equivalent caches stored as `Rc<RefCell<...>>` cells (`tab_slot_positions`, `dialog_btn_rects`, etc.) |

**Common shape:** before drawing any primitive, set theme + (GTK) line-height/char-width on the backend. Then all `draw_*` calls happen inside `enter_frame_scope`. The trait method calls are unit-of-work-equivalent across backends.

## 4. Native services

| Service | TUI | GTK |
|---|---|---|
| Clipboard | `setup_tui_clipboard(&mut engine)` — copypasta-with-X11-fork-fallback | `copypasta_ext::try_context()` w/ x11_bin override on X11; trait via `Backend::services().clipboard()` |
| File dialogs | TUI returns `None` from `show_file_open_dialog` / `show_file_save_dialog`; engine has its own folder-picker overlay | GTK uses `gtk4::FileDialog` natively |
| Notifications | `Backend::services().send_notification` — TUI is no-op; GTK emits via gio | Same trait |
| URL opening | `Backend::services().open_url` — TUI uses `xdg-open` / `open` / `start`; GTK uses `gtk::show_uri` |
| Platform name | `services().platform_name() -> &'static str` — `"tui"` / `"gtk"` | Same |
| Emergency swap-flush | `register_emergency_engine` global pointer for the panic handler | Same global — set during init |

**Already on the trait:** clipboard, file dialogs, notifications, URL opening, platform name. **Not yet on the trait:** panic-hook installation, emergency swap registration, terminal-state save/restore (TUI), CSS load (GTK), icon-theme search-path setup (GTK).

The non-trait items are reasonable to keep app-side OR move into the runner. Most are init-once-at-startup.

## 5. Per-backend boilerplate (numbers)

| Metric | TUI | GTK |
|---|---|---|
| `mod.rs` lines | 4321 | 11331 |
| Connect-style signal hookups (`connect_*`, `set_*_func`, `add_controller`, gestures) | n/a | ~146 |
| Distinct `set_draw_func` calls (per DrawingArea) | n/a | ~20 |
| `event_loop()` body (TUI) / Relm4 `update()` arms (GTK) | ~1,000 lines | ~1,800 lines (`update()` matches ~140 `Msg::*` variants) |

The 11k-vs-4k gap isn't just GTK overhead — most of it is the `view!{}` macro expansion + signal-callback boilerplate. The runner-crate target is to absorb that gap by collapsing the boilerplate to ~10 lines.

## 6. App-level state ownership

**TUI** holds state directly in `event_loop()`'s scope as locals:
- `engine`, `sidebar`, `sidebar_width`, scroll offsets, drag flags, click-tracking, modal-confirm flags, last_layout cache, hover popups, …
- ~50+ named locals, all in one stack frame.

**GTK** holds state on the `App` struct (~80 fields):
- `engine: Rc<RefCell<Engine>>`, `widgets`, theme caches, scroll cells, `tab_close_hover`, `tab_slot_positions`, `dialog_btn_rects`, …
- Many fields are `Rc<RefCell<>>` because signal callbacks share them.

**Shared:** the engine, the modal/drag state (now on the backend), per-frame caches that the next frame's hit-tests consult.

**Genuinely backend-specific:** GTK's widget refs (DrawingAreas, native dialogs, the relm4 sender). TUI's `terminal` handle and raw-mode flags.

## 7. Cross-backend abstractions already in place

| Abstraction | Where | Status |
|---|---|---|
| `Backend` trait | `quadraui/src/backend.rs` | Used by both backends; trait coverage substantially complete after B.5c |
| `UiEvent` | `quadraui/src/event.rs` | Used by both backends |
| `ModalStack` | `quadraui/src/modal_stack.rs` | Lives on the backend; both consult it for routing |
| `DragState` | `quadraui/src/dispatch.rs` | Same |
| `Accelerator` registry + `apply_accelerators` | `quadraui/src/accelerator.rs` | Both wire through |
| `dispatch_mouse_down` / `_drag` / `_up` | `quadraui/src/dispatch.rs` | Both call into for routing |
| Theme adapter `q_theme(&render::Theme) -> quadraui::Theme` | `src/{tui_main,gtk}/quadraui_*.rs` | Per-backend, but identical structure |
| Per-frame layout cache (slot_positions, etc.) | App-side | Per-backend, but the data is the same shape |

## 8. What a runner crate would absorb

Per item, lifted into either `quadraui_tui::run` or `quadraui_gtk::run`:

- **Always-runner**: panic hook + emergency swap registration; CSS / icon-theme setup (GTK); terminal raw-mode + alternate-screen + mouse capture (TUI); kbd-enhancement push/pop (TUI); accelerator registration helper.
- **Always-runner with hooks**: backend construction, `enter_frame_scope` orchestration, `wait_events` / `poll_events` orchestration, theme application.
- **App-provided via trait**: rendering (`AppLogic::render(ctx)`), event handling (`AppLogic::handle(event, ctx) -> Reaction`), per-frame state advancement.

What stays app-side either way: engine state (apps own their state), CLI parsing, session restore, custom keybindings.

## 9. Sketch of the runner-crate API for B.5e (#261)

The audit above suggests the following shape for the runner crates. Concrete details land in B.5e itself; this is the working sketch.

```rust
// quadraui::AppLogic — all the trait that a runner-driven app needs.
pub trait AppLogic {
    /// Per-frame paint. The context bundles the backend's mutable
    /// reference, frame scope, theme, and metrics. The app calls
    /// `ctx.backend.draw_*(...)` for each primitive it wants drawn.
    fn render(&self, ctx: &mut RenderCtx);

    /// Per-event dispatch. Returns what the runner should do next
    /// (continue, exit, force-redraw, …).
    fn handle(&mut self, event: UiEvent, ctx: &mut EventCtx) -> Reaction;

    /// One-time setup hook for accelerator registration, file
    /// associations, etc. Runner calls this after backend construction
    /// but before the first frame.
    fn setup(&mut self, ctx: &mut SetupCtx);
}

pub enum Reaction {
    Continue,
    Redraw,
    ExitApp,
}

// User code:
struct MyApp { /* engine, view state, etc */ }

impl quadraui::AppLogic for MyApp {
    fn setup(&mut self, ctx: &mut SetupCtx) { /* register accelerators */ }
    fn render(&self, ctx: &mut RenderCtx) { /* paint primitives */ }
    fn handle(&mut self, event: UiEvent, ctx: &mut EventCtx) -> Reaction { /* dispatch */ }
}

fn main() {
    let app = MyApp::new();
    #[cfg(feature = "gui")] quadraui_gtk::run(app);
    #[cfg(feature = "tui")] quadraui_tui::run(app);
}
```

### Open design questions for B.5e

1. **`RenderCtx` shape.** Does it expose the raw backend (`&mut dyn Backend`) and let the app call `draw_*` methods directly, or does it wrap and expose only `ctx.draw_status_bar(...)` etc? The wrapper is cleaner but adds API surface. Direct backend access is more flexible.

2. **`EventCtx` shape.** Apps need access to `services()` (clipboard, dialogs) and `modal_stack_mut()` from the event handler. Pass through?

3. **CLI / startup args.** TUI currently takes `(file_path, debug_log_path)` directly to `run()`. GTK takes `file_path` via Relm4's `Init`. Should `quadraui_*::run()` take a single `Args` struct, or should setup happen entirely inside `AppLogic::setup`?

4. **Theme application.** Apps build a `quadraui::Theme` per frame from their own theme system (vimcode does this via `q_theme`). Where does that fit — `RenderCtx::set_theme()` once per frame, or part of `AppLogic::render()`?

5. **Accelerators / panel keys.** The runner registers them in `setup()`. But `Backend::register_accelerator` is on the trait already — apps could call directly. Whichever is less ceremony.

6. **Multi-DrawingArea on GTK.** Vimcode has many DrawingAreas (editor, sidebar, status bar, activity bar, …). Each has its own `set_draw_func`. The runner needs a way to express "render scene N to DrawingArea N." Options: render method named per-area (`render_editor`, `render_sidebar`, …), or a single `render(ctx)` that uses `ctx.target(area_id)` to route paint commands.

7. **Component-local state on GTK.** Vimcode's `App` struct has many `Rc<RefCell<…>>` cells that survive across signal callbacks (tab_slot_positions, etc.). Some belong on the backend (modal_stack, drag); others are per-frame caches the app uses. The runner needs to provide some persistent "app context" object the app can stash these on.

### Migration target

After B.5e ships:
- `src/main.rs` collapses to ~10 lines: parse CLI, build `MyApp`, call `quadraui_{tui,gtk}::run(app)`.
- `src/tui_main/mod.rs::run()` + `event_loop()` collapses to a `MyApp` impl that's mostly engine state + a render method.
- `src/gtk/mod.rs::App::init()` + the `view!{}` macro + the 11k-line `update()` collapse to a similar `MyApp` impl. Widget creation moves into the runner (one DrawingArea per render method).

Estimate: vimcode binary post-runner is ~1,000 lines (most of it engine state + render/handle). Down from ~16,000 lines today across `src/{tui_main,gtk}/`.

## 10. What the audit does NOT change

- The `Backend` trait surface (set by B.5c).
- Existing call sites that already consume the trait (B.5b's draws, B.5c's hit-region returns).
- Per-backend rasterisers in `quadraui::{tui,gtk}::*`.
- Engine state shape.

The audit only documents what exists. The runner crates (B.5e) absorb the boilerplate; they don't change what the boilerplate does.
