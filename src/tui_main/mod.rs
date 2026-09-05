//! TUI (terminal UI) entry point for VimCode.
//!
//! Activated with the `--tui` CLI flag. Uses ratatui + crossterm to render
//! the same `ScreenLayout` produced by `render::build_screen_layout` that the
//! GTK backend consumes — just rendered to a terminal instead of a Cairo
//! surface.
//!
//! **No GTK/Cairo/Pango imports here.** All editor logic comes from `core`.
//! All rendering data comes from `render`.
#![allow(
    unused_assignments,
    clippy::collapsible_match,
    clippy::explicit_counter_loop
)]

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

mod backend;
mod events;
mod mouse;
mod panels;
mod quadraui_tui;
mod render_impl;
mod services;
mod shell_app;

/// #657 test-support seam — the TUI half of what the sealed acceptance suite
/// (`tests/acceptance.rs`, a *separate* crate) needs.
///
/// `shell_app` stays a private module: the only thing published here is
/// [`TuiShellApp`] itself, which is exactly what
/// `quadraui::tui::testing::driver_with_shell` takes. An acceptance slice
/// therefore drives the same `event → handle → render_content` path the
/// in-crate `#[cfg(test)]` suite in `shell_app.rs` does, with no privileged
/// access to internals beyond the public `engine` field.
///
/// Compiled under `cfg(test)` too so the in-crate suite and the sealed suite
/// cannot drift onto different seams.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    pub use super::shell_app::TuiShellApp;
}

#[allow(unused_imports)]
use mouse::*;
#[allow(unused_imports)]
use panels::*;
#[allow(unused_imports)]
use quadraui::Backend;
#[allow(unused_imports)]
use render_impl::*;

// ─── Debug logging ────────────────────────────────────────────────────────────

/// Global debug log file handle, set once at startup via `--debug <path>`.
static DEBUG_LOG: std::sync::OnceLock<Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Initialise the debug log.  Call once before the shell runner starts.
fn init_debug_log(path: &str) {
    match std::fs::File::create(path) {
        Ok(f) => {
            let _ = DEBUG_LOG.set(Mutex::new(f));
            // Also enable LSP debug logging (read by the reader thread in lsp.rs).
            std::env::set_var("VIMCODE_LSP_DEBUG", "1");
        }
        Err(e) => {
            eprintln!("Warning: cannot open debug log {path}: {e}");
        }
    }
}

/// Write a formatted message to the debug log (if enabled).  No-op when
/// `--debug` was not passed.
#[allow(unused_macros)]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if let Some(mtx) = $crate::tui_main::DEBUG_LOG.get() {
            if let Ok(mut f) = mtx.lock() {
                let _ = writeln!(f, $($arg)*);
                let _ = f.flush();
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use debug_log;

use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{supports_keyboard_enhancement, SetTitle};
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
// `RColor`/`Modifier` are only referenced by the `#[cfg(test)]` legacy paint
// helpers (`set_cell`, `rc`) now that `event_loop` is gone.
#[cfg(test)]
use ratatui::style::{Color as RColor, Modifier};
// The legacy full-frame paint path (`render_impl::draw_frame` and friends) was
// `#[cfg(test)]` from #634 (which deleted `event_loop`, its only production
// caller) until #766 deleted `draw_frame` itself; `with_frame_scope` below is
// what remains of that scaffolding, still used by `render_impl`'s test
// module, which reaches `Terminal` through this module's `use super::*`.
//
// (#657) The `CrosstermBackend` import that used to sit alongside it is gone:
// nothing has referenced it since #634, and promoting `tui_main` into
// `vimcode_core` moved these tests into the lib test target, which — unlike
// the old `vcd` bin — carries no crate-wide `allow(unused_imports)` to hide
// the dead import.
#[cfg(test)]
use ratatui::Terminal;

use crate::core::engine::EngineAction;
use crate::core::window::{GroupDivider, GroupId, SplitDirection};
use crate::core::{Engine, Mode, OpenMode, WindowRect};
use crate::icons;
use crate::render::{self, build_screen_layout, Color, RenderedWindow, Theme};

// ─── Key binding helpers ──────────────────────────────────────────────────────

/// Returns true if the given crossterm key event matches a panel_keys binding string.
/// Binding strings use Vim notation: `<C-b>`, `<C-S-e>`, `<A-x>`.
/// Return the effective content-row count for the terminal panel in the TUI.
pub(super) fn effective_terminal_panel_rows_tui(engine: &Engine, screen_h: u16) -> u16 {
    render::compute_editor_layout(engine, screen_h as f64, 1.0, true).terminal_content_rows
}

/// Max target rows for terminal maximize — delegates to shared layout.
pub(super) fn terminal_target_maximize_rows_tui(engine: &Engine, screen_h: u16) -> u16 {
    render::compute_editor_layout(engine, screen_h as f64, 1.0, true).terminal_max_target_rows
}

/// Terminal panel column count (editor column width, excluding sidebar +
/// activity bar). Matches GTK's `terminal_cols()` which divides the drawing
/// area pixel width by char advance.
pub(super) fn terminal_panel_cols(engine: &Engine, screen_w: u16, sidebar_width: u16) -> u16 {
    let sv = engine.app_shell.sidebar_visible();
    let ab = if engine.settings.autohide_panels && !sv {
        0
    } else {
        ACTIVITY_BAR_WIDTH
    };
    let sb = if sv { sidebar_width + 1 } else { 0 };
    screen_w.saturating_sub(ab + sb)
}

// ─── Phase B.4 Stage 6: panel-key accelerator registry ──────────────────────
//
// The 14-entry `PanelAccelerator` id table (`render::ACC_*`) and the
// dispatcher itself (`render::dispatch_panel_accelerator`) are shared with
// GTK (#761 / #734 slice 6) — see the rung's header comment in `render.rs`.
// `TuiAccelHost` (in `shell_app.rs`, next to its call sites) is the five-hook
// impl for the actions that need TUI-local state.

// `register_panel_accelerators` (the 14-entry id table + registration loop)
// moved to `render::register_panel_accelerators` in #823 item 1 — it was
// byte-identical to `app.rs`'s copy and had no backend-specific step.

// ─── Sidebar constants ────────────────────────────────────────────────────────

const SIDEBAR_WIDTH: u16 = 30;
const ACTIVITY_BAR_WIDTH: u16 = 3;

// ─── Activity bar panels ──────────────────────────────────────────────────────

use crate::core::engine::sidebar::*;

// ─── Sidebar data structures ──────────────────────────────────────────────────

struct TuiSidebar {
    has_focus: bool,
    /// True after Ctrl-W is pressed in a sidebar panel, waiting for h/j/k/l.
    pending_ctrl_w: bool,
    /// When set, sidebar renders an extension panel instead of the fixed panels.
    ext_panel_name: Option<String>,
}

impl TuiSidebar {
    fn new() -> Self {
        TuiSidebar {
            has_focus: false,
            pending_ctrl_w: false,
            ext_panel_name: None,
        }
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

// `ScrollDragState`, `SidebarScrollDrag`, and `DebugSidebarScrollDrag` were retired
// across Phase B.4 Stages 5c (sidebar / settings / debug-sidebar /
// terminal / debug-output) and 5d (editor v/h scrollbars). Every TUI
// scrollbar drag now flows through the shared `quadraui::DragState`.
// Widget ids route the dispatched offset to the right scroll-state
// field: `tui:search_results`, `tui:settings`, `tui:debug_sidebar:N`,
// `tui:terminal_scrollback`, `tui:debug_output`, and
// `tui:editor:<window_id>:vsb` / `:hsb`.

// The TUI-local `FolderPickerState`/`FolderPickerMode` and their
// `collect_dir_entries`/`filter_dir_entries`/`dir_fuzzy_score` helpers were
// removed in #815: `quadraui::FolderPickerController` (shipped 2026-05-25,
// quadraui#166) is the extracted-verbatim replacement, adopted by both
// backends now — see `shell_app.rs`'s `folder_picker` field and
// `render::folder_picker_popup_rect` / `render::folder_picker_visible_rows`.

// =============================================================================
// Clipboard setup helpers
// =============================================================================

/// Set up system clipboard callbacks on the engine, delegating entirely to
/// `quadraui::tui::TuiPlatformServices` (#508 — quadraui#269/#283).
///
/// The old TUI clipboard spawned xclip/xsel/wl-copy/wl-paste directly (with a
/// stderr-suppression + `DISPLAY=:0` hack and a manual stdin-EOF dance to
/// route around a copypasta_ext bug). All of that lived only to reach the
/// clipboard over SSH/tmux where a local desktop clipboard tool isn't always
/// reachable. `TuiPlatformServices` now covers the same ground upstream in
/// quadraui: arboard for the local desktop clipboard, OSC 52 (written to
/// both stdout and `/dev/tty`, with tmux DCS-passthrough) for SSH/tmux, and
/// a native-tool fallback leg for local-X11-inside-tmux — see
/// `quadraui::tui::services` for the full writeup. Reads stay arboard-only
/// (OSC 52 read is disabled in most terminals for security reasons).
fn setup_tui_clipboard(engine: &mut Engine) {
    use quadraui::PlatformServices;

    let services = std::rc::Rc::new(quadraui::tui::TuiPlatformServices::new());

    let read_services = services.clone();
    engine.clipboard_read = Some(Box::new(move || {
        read_services
            .clipboard()
            .read_text()
            .ok_or_else(|| "clipboard empty or unavailable".to_string())
    }));

    engine.clipboard_write = Some(Box::new(move |text: &str| {
        services.clipboard().write_text(text);
        Ok(())
    }));
}

/// Copy text to the system clipboard and show a status message.
fn tui_copy_to_clipboard(text: &str, engine: &mut Engine) {
    if let Some(ref cb) = engine.clipboard_write {
        if cb(text).is_ok() {
            engine.message = format!("Copied: {}", text);
            return;
        }
    }
    engine.message = format!("Link: {} (clipboard unavailable)", text);
}

/// Sync the unnamed `"` register to the system clipboard if its content changed.
/// Must be called after every keypress that might have yanked/cut text.
fn sync_tui_clipboard(engine: &mut Engine, last: &mut Option<String>) {
    let current = engine
        .registers
        .get(&'"')
        .filter(|(s, _)| !s.is_empty())
        .map(|(s, _)| s.clone());
    if current != *last {
        if let (Some(ref text), Some(ref cb_write)) = (&current, &engine.clipboard_write) {
            let _ = cb_write(text.as_str());
        }
        *last = current;
    }
}

/// The TUI entry point: initialise the engine and drive it through
/// `quadraui::tui::shell_runner::run_with_shell`.
///
/// #634 (Stage 6, vimcode#595): this *is* the live path now. It started life
/// in #635 (Stage 6b item F) as `run_via_shell`, a dormant sibling of the
/// hand-rolled `run()`/`event_loop()` pair, precisely so that flipping
/// `main.rs`/`tui_bin.rs` over would be a rename plus a deletion rather than
/// a re-architecture. The old `run()`, `event_loop()` (~2,130 lines) and
/// `restore_terminal()` are gone; `git show 509b8fe:src/tui_main/mod.rs`
/// reads them at their final revision, which is what the `mod.rs:NNNN` line
/// references scattered through `shell_app.rs` point at.
///
/// Keeps the non-loop responsibilities the old `run()` owned — the panic
/// hook, emergency-engine registration, the emergency swap flush, and the
/// custom crash message — around `run_with_shell`.
///
/// Unlike the old `run()`, this does **not** do its own raw-mode / alternate-screen
/// / mouse-capture / keyboard-enhancement terminal setup or teardown:
/// `run_with_shell` → `quadraui::tui::run::run` (`quadraui/src/tui/run.rs`)
/// already does all of that internally (`enable_raw_mode`,
/// `EnterAlternateScreen`, `EnableMouseCapture`, `EnableBracketedPaste`, the
/// kitty keyboard-enhancement push/pop), and always restores the terminal
/// — even on panic, via its own inner `catch_unwind` — before propagating
/// via `resume_unwind`. That's exactly what makes wrapping it in a second,
/// outer `catch_unwind` here safe and sufficient: this closure's
/// `catch_unwind` still observes the same panic payload, with the terminal
/// already back to normal, the same guarantee the old `run()`'s own outer
/// `catch_unwind` relied on around `event_loop`.
///
/// `keyboard_enhanced` (threaded into `translate_key` for Ctrl-combo
/// disambiguation) and the emergency-engine pointer registration both move
/// into `TuiShellApp::setup` instead of living here — see
/// [`shell_app::TuiShellApp::prepare_for_live_run`] and that `setup`
/// override's doc comments for why: `run_with_shell` takes `app` *by
/// value* and moves it through several stack frames
/// (`build_shell_adapter` → `ShellAdapter`'s own field →
/// `tui::run::run`'s `mut app: A` local) before it settles, so a raw
/// pointer captured here, before that call, would already be stale by the
/// time anything could read it — `setup()` runs only after all of those
/// moves are done.
pub fn run(file_path: Option<PathBuf>, debug_log_path: Option<String>) {
    if let Some(ref path) = debug_log_path {
        init_debug_log(path);
        debug_log!("=== VimCode TUI debug log started ===");
    }

    let mut app = shell_app::TuiShellApp::new(file_path);
    app.prepare_for_live_run();

    // Always install a panic hook that writes crash info to
    // /tmp/vimcode-crash.log AND to the debug log (if --debug is active) —
    // verbatim copy of the deleted `run()`'s own hook.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Emergency: flush swap files for all dirty buffers before
            // anything else, via the pointer `TuiShellApp::setup` registers
            // once `app` reaches its stable live-run address.
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                debug_log!("Crash log written to {}", path.display());
            }
            prev_hook(info);
        }));
    }

    // #557: `live_shell_config`, not the static `shell_config` — plugins have
    // already registered their sidebar panels by the time `App::new` returns,
    // so frame zero can paint their activity-bar icons rather than waiting for
    // the first dispatch's `sync_ext_activity_panels` to add them.
    let config = shell_app::TuiShellApp::live_shell_config(&app.engine);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        quadraui::tui::shell_runner::run_with_shell(app, config);
    }));

    if let Err(e) = result {
        // Unlike the deleted `run()`, there is no locally-owned `engine` to call
        // `emergency_swap_flush()` on directly here — `app` (and its
        // `engine`) moved into `run_with_shell` above and is gone by the
        // time a panic unwinds back to this frame. The panic hook already
        // ran `run_emergency_flush()` via the registered emergency-engine
        // pointer *before* unwinding started (while `engine` was still
        // fully valid), so the flush already happened; this block only
        // reproduces `run()`'s user-facing crash message.
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            format!("VimCode internal error: {s}")
        } else if let Some(s) = e.downcast_ref::<String>() {
            format!("VimCode internal error: {s}")
        } else {
            "VimCode internal error (unknown panic payload)".to_string()
        };
        let crash_path = crate::core::swap::crash_log_path();
        eprintln!("{msg}");
        eprintln!("Unsaved buffers written to swap files for recovery.");
        eprintln!("Crash details written to {}", crash_path.display());
        eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
        std::process::exit(1);
    }
}

// ─── Event loop ───────────────────────────────────────────────────────────────

/// Enter `backend`'s frame scope exactly once for the whole test-harness
/// paint call, while still handing the closure a genuine
/// `&mut ratatui::Frame` for the handful of raw buffer writes (separators,
/// cursor placement, ...) that have no `Backend::draw_*` trait equivalent
/// and are interleaved with trait calls in a z-order-sensitive sequence
/// (#600 Stage 1 — collapsing the ~30 `enter_frame_scope` sites the
/// now-deleted `draw_frame`/`panels.rs` used to open individually down to
/// the one this function makes). #766 deleted `draw_frame`; this helper's
/// one remaining caller is `render_impl::tests::render_tui_buffer_impl`.
///
/// Rust's borrow checker won't let a single closure passed to
/// `TuiBackend::enter_frame_scope(frame, |b| ...)` also capture the
/// outer `frame` binding — `frame` is already consumed as
/// `enter_frame_scope`'s own argument, so referencing it again inside
/// the closure is E0382 (use of moved value). Relaying it through a raw
/// pointer sidesteps that: it's the same type-erasure technique
/// `TuiBackend::enter_frame_scope` already uses internally to smuggle
/// `&mut Frame<'_>` past its own `Cell<*mut ()>` field, just applied one
/// layer higher so `f` can reach both `backend` and `frame` at once.
// #634: legacy full-frame paint scaffolding. `event_loop()` was its only
// production caller; with that gone this is reachable *only* from the
// `#[cfg(test)]` snapshot/assertion suite in `render_impl.rs`, so it is
// compiled out of shipping binaries rather than muted with
// `#[allow(dead_code)]` — the failure mode `src/gtk/draw.rs::draw_editor`
// demonstrated after the #540 GTK cutover (a zero-caller painter kept alive
// behind an `allow`, silently dropping every overlay it drew).
//
// #766 did the first half of #634's hand-off note: `draw_frame` itself —
// the raw-`ratatui::Frame` rasteriser this function used to scope for — is
// deleted, and the test suite that drove it now paints through
// `render_impl::tests::render_tui_buffer_impl`, a thinner walk over the
// same `render::compose_editor_band` / `render::compose_bottom_band`
// artefacts both live `render_content`s run. `with_frame_scope` itself
// survives because that walk still needs *some* `&mut ratatui::Frame` to
// bind `TuiBackend` to (the handful of raw writes noted above have no
// `Backend::draw_*` route either way) — retargeting it at
// `TuiShellApp::render_content` proper (an owned `TuiShellApp` +
// `driver_with_shell`, matching `shell_app.rs`'s own test style) is the
// remaining half, deferred because several of the tests that call
// `render_tui_buffer_impl` mutate `&Engine` again immediately after
// rendering and a `driver_with_shell`-based caller cannot get the engine
// back out to do that (see `render_tui_buffer_impl`'s own doc comment).
#[cfg(test)]
fn with_frame_scope<R>(
    backend: &mut backend::TuiBackend,
    frame: &mut ratatui::Frame<'_>,
    f: impl FnOnce(&mut backend::TuiBackend, &mut ratatui::Frame<'_>) -> R,
) -> R {
    // Reborrow (not move) so `frame` is still available to pass into
    // `enter_frame_scope` below; the raw pointer itself carries no
    // borrow-checker-tracked lifetime.
    let frame_ptr: *mut ratatui::Frame<'_> = &mut *frame as *mut ratatui::Frame<'_>;
    backend.enter_frame_scope(frame, |b| {
        // SAFETY: `frame_ptr` aliases the exact `Frame` `frame` refers
        // to. The outer `frame` binding above is not read again until
        // this closure returns (it was moved into the `enter_frame_scope`
        // call and `enter_frame_scope` itself only touches it through
        // its own type-erased pointer, never dereferencing it while `f`
        // runs — see that function's doc comment), so this is the only
        // live `&mut Frame` in play for the duration of `f`.
        let frame: &mut ratatui::Frame<'_> = unsafe { &mut *frame_ptr };
        f(b, frame)
    })
}

// ─── Explorer context menu action handler ────────────────────────────────────

/// Process explorer-specific context menu actions that need sidebar prompts.
/// Tab context menu actions (close, split, etc.) are handled directly by
/// `context_menu_confirm()` in the engine.
///
/// `ctx_path` / `ctx_is_dir` come from the context menu target — callers
/// extract them *before* `context_menu_confirm()` consumes the menu.
fn handle_explorer_context_action(
    action: &str,
    engine: &mut Engine,
    _sidebar: &TuiSidebar,
    terminal_size: Option<Size>,
    ctx_path: PathBuf,
    ctx_is_dir: bool,
) {
    let path = ctx_path;
    let is_dir = ctx_is_dir;

    match action {
        // #823 item 6: the string -> `ExplorerAction` resolution for these
        // two arms moved to `ExplorerAction::from_action_str` (shared with
        // GTK's `App::explorer_action`) — see its doc comment for why
        // `"delete"` (below) and `"move_file"` (no arm here at all) stay
        // backend-specific rather than also routing through it.
        "new_file" | "new_folder" | "rename" => {
            if let Some(crud_action) =
                crate::core::settings::ExplorerAction::from_action_str(action)
            {
                engine.dispatch_explorer_crud(crud_action);
            }
        }
        "delete" => {
            engine.confirm_delete_file(&path);
        }
        // copy_path, copy_relative_path, reveal, open_side, open_side_vsplit handled by engine
        "copy_path" | "copy_relative_path" | "reveal" | "open_side" | "open_side_vsplit" => {}
        "open_terminal" => {
            let dir = if is_dir {
                path.clone()
            } else {
                path.parent().unwrap_or(&engine.cwd).to_path_buf()
            };
            let cols = terminal_size.map(|s| s.width).unwrap_or(80);
            let rows = engine.session.terminal_panel_rows;
            engine.terminal_new_tab_at(cols, rows, Some(&dir));
        }
        // select_for_diff and diff_with_selected are handled by the engine
        "select_for_diff" | "diff_with_selected" => {}
        "find_in_folder" => {
            engine.open_picker(crate::core::engine::PickerSource::Grep);
        }
        _ => {}
    }
}

#[cfg(test)]
fn set_cell(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, ch: char, fg: RColor, bg: RColor) {
    let area = buf.area;
    if x < area.x + area.width && y < area.y + area.height {
        let cell = &mut buf[(x, y)];
        cell.set_char(ch).set_fg(fg).set_bg(bg);
        cell.modifier = Modifier::empty();
        cell.underline_color = RColor::Reset;
    }
}

// ─── Tab bar ──────────────────────────────────────────────────────────────────
// Tab/diff constants are defined in render_impl.rs and re-exported via `use render_impl::*;`.

fn shift_map_us(c: char) -> char {
    match c {
        '`' => '~',
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        // Letters: Shift+a → 'A' (crossterm usually already sends uppercase).
        c if c.is_ascii_lowercase() => c.to_ascii_uppercase(),
        _ => c,
    }
}

/// Map a crossterm `KeyCode` to the engine-facing keyname string used by the
/// sidebar panel dispatchers (`dispatch_ext_sidebar_key_unified`,
/// `handle_settings_key`, `dispatch_dap_sidebar_action_key`, …).
///
/// Covers the named navigation/control keys shared across the panels. Returns
/// `None` for `Char(_)`, `F(_)`, and anything else — callers handle those with
/// panel-specific remapping (e.g. Settings remaps `j`/`Down` both to `"j"`).
fn tui_key_to_engine_name(code: KeyCode) -> Option<&'static str> {
    Some(match code {
        KeyCode::Esc => "Escape",
        KeyCode::Enter => "Return",
        KeyCode::Backspace => "BackSpace",
        KeyCode::Delete => "Delete",
        KeyCode::Tab => "Tab",
        KeyCode::BackTab => "BackTab",
        KeyCode::Up => "Up",
        KeyCode::Down => "Down",
        KeyCode::Left => "Left",
        KeyCode::Right => "Right",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "Page_Up",
        KeyCode::PageDown => "Page_Down",
        _ => return None,
    })
}

fn translate_key(event: KeyEvent, keyboard_enhanced: bool) -> Option<(String, Option<char>, bool)> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let shift = event.modifiers.contains(KeyModifiers::SHIFT);
    match event.code {
        KeyCode::Char(c) => {
            let lower = c.to_ascii_lowercase();
            let (key_name, unicode) = if ctrl {
                // Engine dispatches Ctrl combos via key_name (e.g. "d" for Ctrl-D).
                // Space is a named key; use "space" to match GTK and the engine's convention.
                // Ctrl+Shift+X: the char arrives as uppercase (or SHIFT flag is set); keep
                // uppercase so the engine can distinguish Ctrl+P from Ctrl+Shift+P ("P").
                // Some special chars use GTK-style names to match GTK backend conventions.
                let name = if lower == ' ' {
                    "space".to_string()
                } else if lower == '\\' || (!keyboard_enhanced && lower == '4') {
                    // Ctrl+\ sends byte 0x1C; without keyboard enhancement crossterm decodes
                    // 0x1C as KeyCode::Char('4')+CONTROL (formula: 0x1C-0x1C+'4'='4').
                    // Map both to "backslash" so Ctrl+\ works in all terminals.
                    "backslash".to_string()
                } else if lower == '/' || (!keyboard_enhanced && lower == '7') {
                    // Ctrl+/ sends byte 0x1F; without keyboard enhancement crossterm
                    // decodes 0x1F as KeyCode::Char('7')+CONTROL (formula: 0x1F-0x1C+'4'='7').
                    // Map both to "slash" so Ctrl+/ works in all terminals.
                    "slash".to_string()
                } else if lower == '`' {
                    "grave".to_string()
                } else if lower == ',' {
                    "comma".to_string()
                } else if (lower == ']' || lower == '}' || (!keyboard_enhanced && lower == '5'))
                    && shift
                {
                    "Shift_bracketright".to_string()
                } else if (lower == '[' || lower == '{' || (!keyboard_enhanced && lower == '3'))
                    && shift
                {
                    "Shift_bracketleft".to_string()
                } else if lower == '}' {
                    // Ctrl+Shift+] without keyboard enhancement: terminal sends '}'
                    "Shift_bracketright".to_string()
                } else if lower == '{' {
                    // Ctrl+Shift+[ without keyboard enhancement: terminal sends '{'
                    "Shift_bracketleft".to_string()
                } else if lower == ']' || (!keyboard_enhanced && lower == '5') {
                    "bracketright".to_string()
                } else if lower == '[' || (!keyboard_enhanced && lower == '3') {
                    "bracketleft".to_string()
                } else if c.is_uppercase() || shift {
                    lower.to_ascii_uppercase().to_string()
                } else {
                    lower.to_string()
                };
                (name, Some(lower))
            } else {
                // With keyboard enhancement (Kitty protocol + REPORT_ALL_KEYS_AS_ESCAPE_CODES),
                // shifted symbol keys may arrive as the base key + SHIFT modifier instead of
                // the resulting character.  For example ':' comes as Char(';') + SHIFT, not
                // Char(':').  Apply the standard US keyboard shift mapping so the engine
                // receives the correct character.
                let resolved = if keyboard_enhanced && shift {
                    shift_map_us(c)
                } else {
                    c
                };
                ("".to_string(), Some(resolved))
            };
            Some((key_name, unicode, ctrl))
        }
        KeyCode::Esc => Some(("Escape".to_string(), None, false)),
        KeyCode::Enter if shift && ctrl => Some(("Shift_Return".to_string(), None, true)),
        KeyCode::Enter if ctrl => Some(("Return".to_string(), None, true)),
        KeyCode::Enter => Some(("Return".to_string(), None, false)),
        KeyCode::Backspace => Some(("BackSpace".to_string(), None, false)),
        KeyCode::Delete => Some(("Delete".to_string(), None, false)),
        KeyCode::Tab => Some(("Tab".to_string(), None, ctrl)),
        KeyCode::BackTab => Some(("ISO_Left_Tab".to_string(), None, ctrl)),
        // Shift+Arrow (no ctrl): emit as "Shift_X" for VSCode selection extension.
        KeyCode::Up if shift && !ctrl => Some(("Shift_Up".to_string(), None, false)),
        KeyCode::Down if shift && !ctrl => Some(("Shift_Down".to_string(), None, false)),
        KeyCode::Left if shift && !ctrl => Some(("Shift_Left".to_string(), None, false)),
        KeyCode::Right if shift && !ctrl => Some(("Shift_Right".to_string(), None, false)),
        KeyCode::Home if shift => Some(("Shift_Home".to_string(), None, false)),
        KeyCode::End if shift => Some(("Shift_End".to_string(), None, false)),
        // Ctrl+Shift+Arrow: emit as "Shift_X" with ctrl=true for word-level selection.
        KeyCode::Left if shift && ctrl => Some(("Shift_Left".to_string(), None, true)),
        KeyCode::Right if shift && ctrl => Some(("Shift_Right".to_string(), None, true)),
        KeyCode::Up => Some(("Up".to_string(), None, false)),
        KeyCode::Down => Some(("Down".to_string(), None, false)),
        KeyCode::Left => Some(("Left".to_string(), None, ctrl)),
        KeyCode::Right => Some(("Right".to_string(), None, ctrl)),
        KeyCode::Home => Some(("Home".to_string(), None, ctrl)),
        KeyCode::End => Some(("End".to_string(), None, ctrl)),
        KeyCode::PageUp => Some(("Page_Up".to_string(), None, false)),
        KeyCode::PageDown => Some(("Page_Down".to_string(), None, false)),
        KeyCode::F(n) => Some((format!("F{}", n), None, false)),
        _ => None,
    }
}

// ─── Engine action handling ───────────────────────────────────────────────────

fn handle_action(engine: &mut Engine, action: EngineAction) -> bool {
    match action {
        EngineAction::Quit | EngineAction::SaveQuit => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            true
        }
        EngineAction::OpenFile(path) => {
            if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                engine.message = e;
            }
            false
        }
        EngineAction::OpenTerminal | EngineAction::RunInTerminal(_) => false, // TUI handles terminal open in main event loop
        EngineAction::ToggleTerminalMaximize => false, // TUI handles in main event loop (needs viewport rows)
        EngineAction::OpenFolderDialog
        | EngineAction::OpenWorkspaceDialog
        | EngineAction::SaveWorkspaceAsDialog
        | EngineAction::OpenRecentDialog => false, // handled by caller
        EngineAction::QuitWithUnsaved => false, // handled by caller (shows quit confirm overlay)
        EngineAction::ToggleSidebar => false,   // engine handles internally; no-op here
        EngineAction::QuitWithError => {
            engine.cleanup_all_swaps();
            engine.lsp_shutdown();
            save_session(engine);
            std::process::exit(1);
        }
        EngineAction::OpenUrl(url) => {
            crate::core::engine::open_url_in_browser(&url);
            false
        }
        EngineAction::None | EngineAction::Error => false,
    }
}

/// Thin wrapper kept so this file's several call sites don't all need
/// rewriting to `engine.save_session_state()` — the actual body moved to
/// [`crate::core::engine::Engine::save_session_state`] in #823 item 5 (it
/// was the same ~20 lines as `app.rs`'s `save_session_and_exit`, modulo
/// GTK's window-size capture and shutdown epilogue).
fn save_session(engine: &mut Engine) {
    engine.save_session_state();
}

// ─── Color / index helpers ───────────────────────────────────────────────────

#[cfg(test)]
fn rc(c: Color) -> RColor {
    RColor::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod translate_key_tests {
    //! #804: crossterm decodes raw terminal control bytes into `KeyEvent`s
    //! *before* `translate_key` ever sees them — a real terminal never hands
    //! us the byte 0x08 directly, it hands us the `KeyEvent` crossterm
    //! already parsed it into. These tests build exactly the `KeyEvent`
    //! crossterm produces for each byte (per crossterm's own control-code
    //! decoding: bytes 0x01-0x1A become `Char(('a' - 1 + byte) as char)` +
    //! `CONTROL`, except the handful with dedicated `KeyCode` variants) and
    //! assert what `translate_key` does with it — this is what pinned down
    //! that `<C-h>`/`<C-j>`/`<C-c>` arrive as ctrl+letter, not as the named
    //! `BackSpace`/`Return`/`Escape` keys `handle_insert_key` matches on by
    //! name, which is *why* it needs its own explicit ctrl-letter arms
    //! (see `Engine::handle_insert_key`, `#804`).
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// 0x08 (^H): crossterm decodes this as ctrl+'h', not as a `BackSpace`
    /// keycode — `handle_insert_key` must special-case it itself.
    #[test]
    fn byte_0x08_is_ctrl_h_not_backspace() {
        let (name, unicode, ctrl) =
            translate_key(key(KeyCode::Char('h'), KeyModifiers::CONTROL), false).unwrap();
        assert_eq!(name, "h");
        assert_eq!(unicode, Some('h'));
        assert!(ctrl);
    }

    /// 0x0A (^J / <NL>): crossterm decodes this as ctrl+'j'.
    #[test]
    fn byte_0x0a_is_ctrl_j() {
        let (name, unicode, ctrl) =
            translate_key(key(KeyCode::Char('j'), KeyModifiers::CONTROL), false).unwrap();
        assert_eq!(name, "j");
        assert_eq!(unicode, Some('j'));
        assert!(ctrl);
    }

    /// 0x03 (^C / ETX): crossterm decodes this as ctrl+'c'.
    #[test]
    fn byte_0x03_is_ctrl_c() {
        let (name, unicode, ctrl) =
            translate_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL), false).unwrap();
        assert_eq!(name, "c");
        assert_eq!(unicode, Some('c'));
        assert!(ctrl);
    }

    /// 0x1B (ESC): crossterm has a dedicated `KeyCode::Esc` for this byte —
    /// unlike ^H/^J/^C it never arrives as ctrl+'['.
    #[test]
    fn byte_0x1b_is_escape() {
        let (name, unicode, ctrl) =
            translate_key(key(KeyCode::Esc, KeyModifiers::NONE), false).unwrap();
        assert_eq!(name, "Escape");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }

    /// 0x7F (DEL): the physical Backspace key on most terminals; crossterm
    /// has a dedicated `KeyCode::Backspace` for it.
    #[test]
    fn byte_0x7f_is_backspace() {
        let (name, unicode, ctrl) =
            translate_key(key(KeyCode::Backspace, KeyModifiers::NONE), false).unwrap();
        assert_eq!(name, "BackSpace");
        assert_eq!(unicode, None);
        assert!(!ctrl);
    }
}
