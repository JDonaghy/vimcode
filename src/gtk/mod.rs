// TreeView/TreeStore are deprecated in GTK4 4.10+ but still functional
// TODO: Migrate to ListView/ColumnView in a future phase
#![allow(deprecated)]

use gio::prelude::{FileExt, FileMonitorExt};
use gtk4::gdk;
use gtk4::pango;
use gtk4::prelude::*;
use pangocairo::functions as pangocairo;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::core;
use crate::icons;
use crate::render;

use core::engine::EngineAction;
use core::settings::LineNumberMode;
use core::{Engine, OpenMode, WindowRect};
use render::Theme;

use copypasta_ext::ClipboardProviderExt;
use std::collections::HashMap;

mod backend;
mod click;
mod css;
mod events;
mod explorer;
mod services;
// #657: also compiled under `test-support` so the sealed acceptance suite in
// `tests/acceptance.rs` — a separate crate — can reach the #646 harness.
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
mod util;

use click::*;
use css::*;
use util::*;

use crate::core::engine::sidebar::*;

fn is_ext_panel_id(id: &str) -> bool {
    id.starts_with("ext:")
}

type TabSlotMap = HashMap<usize, Vec<(f64, f64)>>;

/// Pango font family for UI panels (menu bar, sidebars, dropdown,
/// dialogs, hover popups). Size is appended at use via [`UI_FONT`]
/// from the configured `settings.ui_font_size` (#217).
///
/// Re-homed from the deleted `src/gtk/draw.rs` (#672) — draw.rs was
/// dead under `ShellApp`, but this const and its three siblings below
/// were still live (read by the raw-Pango chrome `render_content`
/// paints directly, e.g. the menu-bar font and the breadcrumb-heading
/// font), so they moved rather than being deleted with the rest of
/// the file.
///
/// #704 item 1: the old list (`"Segoe UI, Ubuntu, Droid Sans, Sans"`)
/// led with two names that never resolve on Linux — Segoe UI is
/// Windows-only, Droid Sans was retired from Android a decade ago —
/// and never listed Cantarell, the default UI font on GNOME (the most
/// common Linux desktop and the one this project targets). On a
/// GNOME box without the `Ubuntu` font package installed, fontconfig
/// fell through the whole list to the trailing generic `Sans`, which
/// resolves to DejaVu Sans: wider, with a taller x-height, than what
/// VS Code lands on at the same nominal point size (VS Code's own
/// `-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, Ubuntu,
/// "Droid Sans", sans-serif` stack has the identical problem, but
/// Electron's Chromium has extra fallback logic Pango/fontconfig does
/// not). Reordered so the two real Linux desktop UI fonts — Cantarell
/// (GNOME) and Ubuntu (Ubuntu/Unity) — are tried first, ahead of the
/// Windows/legacy names kept only for a hypothetical native-Windows
/// GTK build; `Sans` remains the final catch-all so a system with none
/// of the above still gets *a* font rather than a Pango parse failure.
/// This was blocked on quadraui#624 landing `Backend::set_ui_font`
/// reaching non-dialog chrome (tab bar, status bar, tree, menu bar) —
/// before that, changing this constant only affected `draw_dialog`/
/// `draw_rich_text_popup` and nothing else (see the issue's "Negative
/// example" reference to #700 item 1's no-op shape). `ui_font_size`
/// (`core::settings::default_ui_font_size`, 10pt ≈ 13.3px at 96dpi) is
/// left as-is: VS Code's 13px default is a ~2% difference, dwarfed by
/// the metric change from fixing the family, so nudging both at once
/// would make it impossible to tell which change did what.
const UI_FONT_FAMILY: &str = "Cantarell, Ubuntu, Segoe UI, Droid Sans, Sans";

/// Process-global UI font size (points). Synced from
/// `settings.ui_font_size` at the start of each frame by
/// [`sync_ui_font_size`]. Read everywhere a Pango font description
/// is built — avoids threading `&Settings` through every draw
/// function for what's effectively one shared knob (#217).
static UI_FONT_SIZE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(10);

/// Update the process-global UI font size from `settings`. Called
/// once per frame at the top of [`App::render_content`] (#672 —
/// `draw.rs::draw_editor`'s only live caller before the delete).
fn sync_ui_font_size(settings: &core::settings::Settings) {
    UI_FONT_SIZE.store(
        settings.ui_font_size.max(6),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Pango font description string for UI chrome at the currently
/// configured size. Call sites do `FontDescription::from_string(&UI_FONT())`.
#[allow(non_snake_case)]
fn UI_FONT() -> String {
    format!(
        "{} {}",
        UI_FONT_FAMILY,
        UI_FONT_SIZE.load(std::sync::atomic::Ordering::Relaxed)
    )
}

/// Absolute per-group close-glyph hit rects captured during `render_content`.
/// Keyed by `group_id.0` → `(bar_y_top, bar_y_bottom, per-tab Some((x0, x1)))`.
/// All coordinates are in **absolute surface pixels** (same space as the raw
/// mouse position), so hover hit-testing needs no geometry re-derivation. The
/// x-ranges are the *tight* close-glyph zone (see [`CLOSE_*` metrics] and
/// [`tighten_close_bounds`]), matching the × highlight the rasteriser draws —
/// so a hover shows the exact box that a click would close. (#515)
type TabCloseAbsMap = HashMap<usize, (f64, f64, Vec<Option<(f64, f64)>>)>;

/// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`).
/// See `ShellApp::cached_tab_slots_abs` for the full doc comment. (#515)
type TabSlotsAbsMap = HashMap<usize, Vec<(f32, f32)>>;

// ── GTK editor-group tab-bar close-glyph metrics ─────────────────────────────
// The quadraui GTK rasteriser lays each non-compact tab out as
// `tab_pad | label | tab_inner_gap | × | tab_pad | tab_outer_gap` and reports a
// *padded* close-button hit zone spanning `[label_end, tab_right_edge]`. That
// zone is far wider than the drawn × glyph, so a click well before the glyph
// used to close the tab with no warning (#515). We trim the padded zone back to
// the glyph the rasteriser actually painted — plus the same 2px hover halo it
// draws behind the ×, so the clickable box equals the highlighted box.
//
// These mirror the non-compact constants in quadraui's `gtk::backend`
// (`tab_pad = 14`, `tab_inner_gap = 10`, `tab_outer_gap = 1`) and the 2px hover
// pad in `gtk::tab_bar`. Editor-group bars are always built with
// `compact: false` (see `render::build_tab_bar_primitive`). This duplication is
// the interim until quadraui exposes the tight glyph rect directly
// (quadraui#395 tracks the API gap); `tighten_close_bounds` is the single place
// it lives.
const CLOSE_TAB_INNER_GAP: f64 = 10.0;
const CLOSE_TAB_PAD: f64 = 14.0;
const CLOSE_TAB_OUTER_GAP: f64 = 1.0;
const CLOSE_HOVER_PAD: f64 = 2.0;

/// What the git sidebar's commit-message `TextInput` border costs vertically in
/// GTK's native unit: 1px on top + 1px on bottom. TUI's whole-cell equivalent
/// is `render::sc_commit_input_box_height`'s `+ 2` rows. Fed to
/// `render::sc_sidebar_bands` by both the painter and the click router so the
/// two agree (#544).
const SC_COMMIT_BORDER_PX: f32 = 2.0;

/// Trim a *padded* close-button hit zone `(start, end)` — as reported by
/// `quadraui::Backend::tab_bar_layout` — down to the tight × glyph box the
/// rasteriser actually draws (including its 2px hover halo). Leading
/// `tab_inner_gap` and trailing `tab_pad + tab_outer_gap` are dead padding that
/// should select the tab, not close it. Returns `None` if the padded zone is
/// degenerate (too small to contain a glyph). (#515)
fn tighten_close_bounds(start: f64, end: f64) -> Option<(f64, f64)> {
    let tight_start = start + CLOSE_TAB_INNER_GAP - CLOSE_HOVER_PAD;
    let tight_end = end - CLOSE_TAB_PAD - CLOSE_TAB_OUTER_GAP + CLOSE_HOVER_PAD;
    if tight_end > tight_start {
        Some((tight_start, tight_end))
    } else {
        None
    }
}

/// Per-group pixel-accurate tab-bar hit geometry recovered from
/// [`quadraui::Backend::tab_bar_layout`] during the ShellApp `render_content`
/// pass. All x-ranges are **relative to the group's tab-bar left edge** — the
/// same space as `render::screen_zone_hit_test`'s `local_x`.
///
/// This replaces the char-cell `hit_regions` approximation for GTK tab clicks.
/// GTK draws tabs with proportional-font Pango widths + fixed pixel padding
/// (`tab_pad`, `inner_gap`, close-glyph width), so a `name.chars() * char_width`
/// estimate under-measures every tab — shifting the tab/close boundaries and
/// making mid-tab clicks land on the close button and right-edge clicks land on
/// the next tab (#515 regression). The rasteriser reports the exact drawn
/// geometry, so we hit-test against that. (`hit_regions` stays authoritative for
/// the monospace TUI backend, whose char-cell layout matches its draw.)
#[derive(Default, Clone)]
pub(super) struct TabBarPixelHits {
    /// `(start_x, end_x)` per tab index; `(0.0, 0.0)` for scrolled-off tabs.
    pub slots: Vec<(f64, f64)>,
    /// `Some((start_x, end_x))` close-button zone per tab, or `None`.
    pub close: Vec<Option<(f64, f64)>>,
    /// Right-segment hit zones (split / diff / action buttons) as
    /// `(start_x, end_x, target)`, disjoint from the tab slots.
    pub segments: Vec<(f64, f64, crate::core::engine::TabBarClickTarget)>,
}

/// Key = `group_id.0` (single-group mode keys under the active group's id, which
/// is what `screen_zone_hit_test` reports for it).
type TabPixelHitMap = HashMap<usize, TabBarPixelHits>;

/// Convert a rasteriser [`quadraui::TabBarHits`] (absolute pixel x, from
/// `Backend::tab_bar_layout`) plus its source [`quadraui::TabBar`] into a
/// [`TabBarPixelHits`] with every x-range shifted to be **relative to
/// `bar_left_x`** (the group tab bar's left edge). Right-segment ids are mapped
/// to their `TabBarClickTarget` using the same `"tab:*"` ids that
/// `build_tab_bar_primitive` emits (mirrors `draw::draw_tab_bar`).
fn tab_hits_to_pixel_hits(
    hits: &quadraui::TabBarHits,
    bar: &quadraui::TabBar,
    bar_left_x: f64,
) -> TabBarPixelHits {
    use crate::core::engine::TabBarClickTarget as T;
    let rel = |a: f64, b: f64| (a - bar_left_x, b - bar_left_x);
    let slots = hits
        .slot_positions
        .iter()
        .map(|&(a, b)| {
            if (a, b) == (0.0, 0.0) {
                (0.0, 0.0) // scrolled-off sentinel — leave as zero-width
            } else {
                rel(a, b)
            }
        })
        .collect();
    // Trim the padded close zone the rasteriser reports down to the tight ×
    // glyph box (relative to the bar's left edge), so clicks/hover only fire on
    // the drawn glyph — not the ~25px of surrounding tab padding. (#515)
    let close = hits
        .close_bounds
        .iter()
        .map(|c| c.and_then(|(a, b)| tighten_close_bounds(a, b).map(|(ta, tb)| rel(ta, tb))))
        .collect();
    let mut segments = Vec::new();
    for (i, seg) in bar.right_segments.iter().enumerate() {
        let Some((a, b)) = hits.right_segment_bounds.get(i).copied() else {
            continue;
        };
        let Some(ref id) = seg.id else { continue };
        let target = match id.as_str() {
            "tab:split_right" => Some(T::SplitRight),
            "tab:split_down" => Some(T::SplitDown),
            "tab:diff_prev" => Some(T::DiffPrev),
            "tab:diff_next" => Some(T::DiffNext),
            "tab:diff_toggle" => Some(T::DiffToggle),
            "tab:action_menu" => Some(T::ActionMenu),
            _ => None,
        };
        if let Some(t) = target {
            let (s, e) = rel(a, b);
            segments.push((s, e, t));
        }
    }
    TabBarPixelHits {
        slots,
        close,
        segments,
    }
}

/// Build the absolute close-glyph hit record for one tab bar from its
/// bar-relative (already-tightened) close bounds. `bar_left_x` is the bar's
/// absolute left edge; `y_top`/`y_bot` bracket the tab row. Consumed by
/// `tab_close_hit_test` for hover. (#515)
fn abs_close_record(
    ph_close: &[Option<(f64, f64)>],
    bar_left_x: f64,
    y_top: f64,
    y_bot: f64,
) -> (f64, f64, Vec<Option<(f64, f64)>>) {
    let xs = ph_close
        .iter()
        .map(|c| c.map(|(a, b)| (a + bar_left_x, b + bar_left_x)))
        .collect();
    (y_top, y_bot, xs)
}

/// Collect the visible tab slots (absolute x-ranges) from a `TabBarHits`,
/// dropping the `(0.0, 0.0)` sentinels for scrolled-off / non-fitting tabs.
/// The result is a contiguous run starting at the tab bar's `scroll_offset`,
/// which the drop-zone reorder logic offsets back to absolute tab indices.
/// (#515)
fn abs_visible_slots(hits: &quadraui::TabBarHits) -> Vec<(f32, f32)> {
    hits.slot_positions
        .iter()
        .filter(|&&(a, b)| (a, b) != (0.0, 0.0))
        .map(|&(a, b)| (a as f32, b as f32))
        .collect()
}

/// Cached diff toolbar button positions per group: group_id -> (prev_start, prev_end, next_start, next_end, fold_start, fold_end).
/// Populated during draw_tab_bar, used for click hit-testing.
type DiffBtnMap = HashMap<usize, (f64, f64, f64, f64, f64, f64)>;

/// Cached split button pixel widths per group: group_id -> (both_btns_px, btn_right_px).
/// Only populated when split buttons are visible (active group in multi-group, or single-group mode).
type SplitBtnMap = HashMap<usize, (f64, f64)>;

/// Cached action menu button pixel range per group: group_id -> (start_x, end_x).
type ActionBtnMap = HashMap<usize, (f64, f64)>;

/// Cached per-window status segment hit zones: window_id -> Vec<(start_x, end_x, action)>.
/// Populated in `render_content`'s per-window/separated status bar paint
/// (#672 — re-homed off the dead `draw.rs::draw_window_status_bar`),
/// consumed by click hit-testing.
type StatusSegmentMap = HashMap<usize, Vec<(f64, f64, crate::core::engine::StatusAction)>>;

// ─── Panel-key accelerator registry ─────────────────────────────────────────
//
// Stable accelerator IDs for the `panel_keys` settings, registered on
// `GtkBackend` at App startup. Mirrors the TUI's `tui.panel.*`
// registry. As of Phase B.5b Stage 2 the editor key handler runs a
// single `match_keypress` lookup against this registry and routes
// matches through `dispatch_gtk_panel_accelerator` — replacing 13
// inline `matches_gtk_key` arms that used to scan the bindings linearly.

pub(super) const ACC_TOGGLE_SIDEBAR: &str = "gtk.panel.toggle_sidebar";
pub(super) const ACC_FOCUS_EXPLORER: &str = "gtk.panel.focus_explorer";
pub(super) const ACC_FOCUS_SEARCH: &str = "gtk.panel.focus_search";
pub(super) const ACC_FUZZY_FINDER: &str = "gtk.panel.fuzzy_finder";
pub(super) const ACC_LIVE_GREP: &str = "gtk.panel.live_grep";
pub(super) const ACC_COMMAND_PALETTE: &str = "gtk.panel.command_palette";
pub(super) const ACC_OPEN_TERMINAL: &str = "gtk.panel.open_terminal";
pub(super) const ACC_TERMINAL_TOGGLE_MAX: &str = "terminal.toggle_maximize";
pub(super) const ACC_ADD_CURSOR: &str = "gtk.panel.add_cursor";
pub(super) const ACC_SELECT_ALL_MATCHES: &str = "gtk.panel.select_all_matches";
pub(super) const ACC_SPLIT_EDITOR_RIGHT: &str = "gtk.panel.split_editor_right";
pub(super) const ACC_SPLIT_EDITOR_DOWN: &str = "gtk.panel.split_editor_down";
pub(super) const ACC_NAV_BACK: &str = "gtk.panel.nav_back";
pub(super) const ACC_NAV_FORWARD: &str = "gtk.panel.nav_forward";

/// Register the panel-keys accelerator set on the backend. Re-runs on each
/// settings reload so live rebinding takes effect.
///
/// Called from `ShellApp::setup` (#587) — mirrors `tui_main`'s call at
/// startup. Takes the trait object (`&mut dyn quadraui::Backend`) rather
/// than the concrete `backend::GtkBackend` because the ShellApp runner
/// only ever hands `setup`/`handle` a `&mut dyn quadraui::Backend`; the
/// separate `self.backend: GtkBackend` field is a distinct instance used
/// only for click hit-testing (see the #560 comment in `render_content`)
/// and never receives `UiEvent::Accelerator`, so registering against it
/// would be silently ineffective.
fn register_panel_accelerators(
    backend: &mut dyn quadraui::Backend,
    pk: &crate::core::settings::PanelKeys,
) {
    let entries: [(&str, &str); 14] = [
        (ACC_TOGGLE_SIDEBAR, &pk.toggle_sidebar),
        (ACC_FOCUS_EXPLORER, &pk.focus_explorer),
        (ACC_FOCUS_SEARCH, &pk.focus_search),
        (ACC_FUZZY_FINDER, &pk.fuzzy_finder),
        (ACC_LIVE_GREP, &pk.live_grep),
        (ACC_COMMAND_PALETTE, &pk.command_palette),
        (ACC_OPEN_TERMINAL, &pk.open_terminal),
        (ACC_TERMINAL_TOGGLE_MAX, &pk.toggle_terminal_maximize),
        (ACC_ADD_CURSOR, &pk.add_cursor),
        (ACC_SELECT_ALL_MATCHES, &pk.select_all_matches),
        (ACC_SPLIT_EDITOR_RIGHT, &pk.split_editor_right),
        (ACC_SPLIT_EDITOR_DOWN, &pk.split_editor_down),
        (ACC_NAV_BACK, &pk.nav_back),
        (ACC_NAV_FORWARD, &pk.nav_forward),
    ];
    for (id, binding) in entries {
        let acc_id = quadraui::AcceleratorId::new(id);
        if binding.is_empty() {
            backend.unregister_accelerator(&acc_id);
            continue;
        }
        backend.register_accelerator(&quadraui::Accelerator {
            id: acc_id,
            binding: quadraui::KeyBinding::Literal(binding.to_string()),
            scope: quadraui::AcceleratorScope::Global,
            label: None,
        });
    }
}

// ─── Phase B.5b Stage 2: panel-key accelerator dispatcher ───────────────────
//
// Mirrors `tui_main::dispatch_panel_accelerator`. Replaces 13 inline
// `if matches_gtk_key(&pk.X, ...)` arms in the editor key handler with
// a single registry lookup → match-on-id dispatcher. The action set
// matches what the legacy arms did (Msg dispatch where the existing
// update() handler runs the side effect; direct engine mutation where
// the legacy arms also called engine directly).
//
// Returns `true` if the id was handled — caller should `return Stop`.
// Returns `false` for unknown ids so the caller can fall through.
//
// `ACC_TERMINAL_TOGGLE_MAX` is included for completeness; the engine's
// own `match_accelerator` block handles the same key first and
// returns Stop, so this arm is only reachable if the engine's
// registration is removed.
fn dispatch_gtk_panel_accelerator(
    id: &str,
    deferred: &DeferredQueue,
    engine: &Rc<RefCell<Engine>>,
) -> bool {
    match id {
        ACC_OPEN_TERMINAL => {
            deferred.send(DeferredAction::ToggleTerminal);
            true
        }
        ACC_TOGGLE_SIDEBAR => {
            deferred.send(DeferredAction::ToggleSidebar);
            true
        }
        ACC_FOCUS_EXPLORER => {
            deferred.send(DeferredAction::ToggleFocusExplorer);
            true
        }
        ACC_FOCUS_SEARCH => {
            deferred.send(DeferredAction::ToggleFocusSearch);
            true
        }
        ACC_FUZZY_FINDER => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Files);
            deferred.send(DeferredAction::Resize);
            true
        }
        ACC_LIVE_GREP => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Grep);
            deferred.send(DeferredAction::Resize);
            true
        }
        ACC_COMMAND_PALETTE => {
            engine
                .borrow_mut()
                .open_picker(core::engine::PickerSource::Commands);
            deferred.send(DeferredAction::Resize);
            true
        }
        ACC_TERMINAL_TOGGLE_MAX => {
            deferred.send(DeferredAction::ToggleTerminalMaximize);
            true
        }
        ACC_ADD_CURSOR => {
            engine.borrow_mut().add_cursor_at_next_match();
            deferred.send(DeferredAction::Resize);
            true
        }
        ACC_SELECT_ALL_MATCHES => {
            engine.borrow_mut().select_all_occurrences();
            deferred.send(DeferredAction::Resize);
            true
        }
        ACC_SPLIT_EDITOR_RIGHT => {
            engine
                .borrow_mut()
                .open_editor_group(crate::core::window::SplitDirection::Vertical);
            true
        }
        ACC_SPLIT_EDITOR_DOWN => {
            engine
                .borrow_mut()
                .open_editor_group(crate::core::window::SplitDirection::Horizontal);
            true
        }
        ACC_NAV_BACK => {
            engine.borrow_mut().tab_nav_back();
            true
        }
        ACC_NAV_FORWARD => {
            engine.borrow_mut().tab_nav_forward();
            true
        }
        _ => false,
    }
}

/// Work that a GTK callback with no `&mut App` in hand must hand back to the
/// next frame.
///
/// #732 tranche 3: the nine deferrals below are all that is left of the
/// Relm4-era `Msg` bus. They are genuine deferrals, not translations — each
/// originates somewhere that cannot call an `&mut self` method at all: a
/// `gio::FileMonitor` signal, a `glib::timeout_add_local_once` closure, or
/// `dispatch_gtk_panel_accelerator`, which is a free function holding only a
/// clone of the queue.
///
/// quadraui has no deferral seam of its own to move onto — `ShellApp::tick`
/// *is* the seam (its doc names "draining channels" as the intended use), and
/// the queued payload is necessarily app-specific, so the queue stays here
/// rather than becoming a quadraui gap to file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeferredAction {
    /// Clear the yank highlight after the flash duration has elapsed.
    ClearYankHighlight,
    /// Refresh the file tree from the current working directory.
    RefreshFileTree,
    /// Redraw after an accelerator mutated engine state directly.
    Resize,
    /// `settings.json` changed on disk.
    SettingsFileChanged,
    /// Toggle focus between the explorer and the editor.
    ToggleFocusExplorer,
    /// Toggle focus between the search panel and the editor.
    ToggleFocusSearch,
    /// Toggle sidebar visibility.
    ToggleSidebar,
    /// Toggle the integrated terminal panel open/closed.
    ToggleTerminal,
    /// Toggle the "terminal maximized" state.
    ToggleTerminalMaximize,
}

/// Shared queue of [`DeferredAction`]s, drained by `ShellApp::tick`.
#[derive(Clone)]
struct DeferredQueue(Rc<RefCell<VecDeque<DeferredAction>>>);

impl DeferredQueue {
    fn new() -> Self {
        DeferredQueue(Rc::new(RefCell::new(VecDeque::new())))
    }

    /// Enqueue an action for processing in the next `tick()` call.
    fn send(&self, action: DeferredAction) {
        self.0.borrow_mut().push_back(action);
    }

    /// Take all pending actions, leaving the queue empty.
    fn drain(&self) -> Vec<DeferredAction> {
        let mut q = self.0.borrow_mut();
        q.drain(..).collect()
    }
}

struct App {
    engine: Rc<RefCell<Engine>>,
    /// Set to true in update() whenever a draw is needed; cleared by the #[watch] block.
    /// This prevents the 20/sec SearchPollTick timer from unconditionally calling queue_draw().
    draw_needed: Rc<Cell<bool>>,
    /// A file dialog requested this frame, drained by the next `tick()`
    /// call (which has the `backend` handle `PlatformServices` needs).
    /// See [`PendingFileDialog`] (#572).
    pending_file_dialog: Cell<Option<PendingFileDialog>>,
    cached_line_height: f64,
    cached_char_width: f64,
    /// Position of the wheel event currently being handled, in **absolute**
    /// surface pixels (the same frame `render_content` paints in). Read by
    /// `handle_mouse_scroll_msg` to route the wheel to the registered scroll surface
    /// or editor pane under the cursor (#240) — matches TUI behaviour.
    ///
    /// Written by `ShellApp::handle`'s `UiEvent::Scroll` arm from the event's
    /// own `position`. It used to be written by the Relm4 build's
    /// `EventControllerMotion`; the #540 ShellApp migration removed that
    /// controller and left no writer, so this stayed `None` forever and every
    /// wheel event fell through to the focused window while the
    /// `dispatch_scroll` surface routing (terminal scrollback, editor-hover
    /// popup, debug output) never ran at all. Sourcing it from the wheel event
    /// itself — rather than from a preceding motion event — is what makes it
    /// impossible to regress the same way again (#646).
    last_editor_pointer: Rc<Cell<Option<(f64, f64)>>>,
    /// Cached line height for the UI font (sidebars, panels).
    /// Computed alongside `cached_line_height` in `CacheFontMetrics`.
    cached_ui_line_height: f64,
    /// Cached dialog layout from the last `render_content` paint (#546) —
    /// mirrors `context_menu_layout` below. Button-click and outside-click
    /// hit-testing both read `DialogLayout::hit_test` off this instead of a
    /// hand-rolled per-backend rect cache.
    dialog_layout: Rc<RefCell<Option<quadraui::DialogLayout>>>,
    /// Edge-trigger flag for #727's native message-dialog path: `true`
    /// once a native present has been queued (or already shown) for the
    /// `engine.dialog` currently open. A native `AlertDialog` cannot be
    /// re-presented every frame the way the in-canvas `Dialog` primitive
    /// is repainted, so `render_content` only queues one when this is
    /// `false`, then sets it `true`. Reset to `false` by `render_content`
    /// when `engine.dialog` goes back to `None` (dialog closed), arming
    /// the trigger for the next open.
    ///
    /// `Rc`-wrapped (like `dialog_layout` above) so `testing::harness` can
    /// keep a handle after `App` is moved into the driver — the #727 test
    /// asserts the present-exactly-once behaviour by repainting several
    /// frames and checking this never re-queues.
    native_dialog_shown: Rc<Cell<bool>>,
    /// A native message dialog queued by `render_content`'s edge-trigger
    /// check, drained by `tick()`. Mirrors `PendingFileDialog` (#572):
    /// `PlatformServices::show_message_dialog` blocks via quadraui's
    /// nested-mainloop pump, which must not run from inside the paint
    /// callback `render_content` runs under, so the request is stashed
    /// here and the actual call happens in `tick()` instead.
    ///
    /// `Rc`-wrapped for the same testing reason as `native_dialog_shown`.
    pending_native_dialog: Rc<Cell<Option<quadraui::MessageDialogOptions>>>,
    /// Shared with the drawing-area resize callback so scrollbars can be
    /// repositioned synchronously (before each frame) without going through
    /// Relm4's async message queue.
    line_height_cell: Rc<Cell<f64>>,
    char_width_cell: Rc<Cell<f64>>,
    /// Current mouse position, updated directly from the motion callback (no Relm4 message).
    mouse_pos_cell: Rc<Cell<(f64, f64)>>,
    /// Shared with draw closure: which window (if any) has an active h scrollbar drag.
    h_sb_drag_cell: Rc<Cell<Option<core::WindowId>>>,
    /// True while user is drag-selecting text inside a find/replace input field.
    fr_input_dragging: bool,
    #[allow(dead_code)] // Kept alive to continue monitoring settings.json
    settings_monitor: Option<gio::FileMonitor>,
    deferred: DeferredQueue,
    /// Last content written to system clipboard.
    /// Used to avoid redundant writes on every keystroke.
    last_clipboard_content: Option<String>,
    /// Which tab close button (×) the mouse is over: (group_id.0, tab_idx).
    tab_close_hover: Option<(usize, usize)>,
    /// Cached tab slot widths per group, populated during draw_tab_bar for click hit-testing.
    /// Key = group_id.0 (or usize::MAX for single-group mode), Value = cumulative x positions.
    tab_slot_positions: Rc<RefCell<TabSlotMap>>,
    /// Absolute tight close-glyph rects captured in `render_content`. Consumed
    /// by `tab_close_hit_test` (hover) so it hit-tests against the exact drawn
    /// geometry — including the activity-bar/sidebar x-offset — instead of
    /// re-deriving group rects from a `(0,0)` content origin (which ignored the
    /// offset and made hover never fire in ShellApp mode). (#515)
    cached_tab_close_abs: Rc<RefCell<TabCloseAbsMap>>,
    /// Absolute visible tab-slot x-ranges per group (`group_id.0` → `[(x0,x1)]`),
    /// captured in `render_content`. Feeds the tab drop-zone computation so a
    /// short drag inside a group's own tab bar resolves to a `TabReorder` (with
    /// an insertion bar) rather than a new-split overlay. (#515)
    cached_tab_slots_abs: Rc<RefCell<TabSlotsAbsMap>>,
    /// Pixel-accurate per-group tab-bar hit geometry from the ShellApp
    /// `render_content` pass (via `Backend::tab_bar_layout`). Consumed by the
    /// GTK tab-bar click hit-test instead of the char-cell `hit_regions`, which
    /// don't match GTK's proportional-font tab layout. (#515)
    cached_tab_pixel_hits: Rc<RefCell<TabPixelHitMap>>,
    /// Cached diff toolbar button pixel positions, populated during draw_tab_bar.
    diff_btn_map: Rc<RefCell<DiffBtnMap>>,
    split_btn_map: Rc<RefCell<SplitBtnMap>>,
    action_btn_map: Rc<RefCell<ActionBtnMap>>,
    /// Cached per-window status bar segment hit zones from draw_window_status_bar.
    status_segment_map: Rc<RefCell<StatusSegmentMap>>,
    /// Painted rect of the separated status line's status bar (#671/#672),
    /// or `None` if the last frame drew no separated line
    /// (`window_status_line` off, `status_line_above_terminal` on, or no
    /// bottom panel open — see `compute_editor_layout`'s `has_separated`).
    /// Exists purely so the `status_segment_map` entry keyed by
    /// `active_window_id` (inserted right alongside this) can be located by
    /// pixel — the live click path itself needs no such cache, since
    /// `status_segment_map`'s `local_x` is already bar-relative and
    /// `window_zone_hit_test`/`screen_zone_hit_test` resolve the y-band
    /// independently. Mirrors the existing `picker_popup_rect` /
    /// `tab_switcher_popup_rect` "painted rect for click+test" pattern.
    separated_status_bar_rect: Rc<Cell<Option<quadraui::Rect>>>,
    /// Segment hit zones for the **global** (bottom-of-screen) status bar,
    /// local to `Engine::global_status_rect`'s own origin (#752).
    ///
    /// Kept in its own field rather than in `status_segment_map` because that
    /// map is keyed by `WindowId` and the global bar belongs to no window — it
    /// shows the active buffer's summary in the shell's bottom band. Populated
    /// by `render_content` from `Backend::status_bar_layout`, the same
    /// measurement pass that positions the bar's glyphs, so the git-branch
    /// segment's clickable span is by construction the span it painted at.
    global_status_zones: Rc<RefCell<render::StatusZones>>,
    /// Cached ScreenLayout from the last draw_editor paint pass. Click handlers
    /// read this instead of recomputing geometry from engine state (#344).
    cached_screen_layout: Rc<RefCell<Option<render::ScreenLayout>>>,
    /// Accumulated `quadraui::FrameHitMap` covering the `Editor`/`TabBar`
    /// zones painted in `render_content` (#449). Built via
    /// `quadraui::ScreenLayout::hit_map()` (quadraui#425): pushes the SAME
    /// `Editor`/`TabBar` objects and rects already painted at their existing
    /// call sites, purely for hit-testing, so it can never reorder or
    /// duplicate real painting. `click::pixel_to_click_target` consults this
    /// first to resolve the top-level Editor/TabBar zone, falling back to
    /// `render::screen_zone_hit_test`'s manual rect-walk for
    /// breadcrumb/divider zones (which have no `FrameZone` equivalent) and
    /// for the brief window before the first paint populates this cache.
    cached_frame_hit_map: Rc<RefCell<Option<quadraui::FrameHitMap>>>,
    /// Parallel table for resolving `FrameZone::TabBar { idx }`, keyed by the
    /// *global* surface index `FrameZone::TabBar { idx }` actually carries —
    /// `ScreenLayout::zone_for`'s `idx` enumerates ALL surfaces pushed into
    /// `cached_frame_hit_map` (editors THEN tab bars), not a per-tab-bar
    /// count, so a tab bar's global index is offset by however many editor
    /// surfaces were pushed before it. A plain `Vec` indexed 0.. (the
    /// original #449 shape) silently mismatched by that offset and made
    /// every `FrameZone::TabBar` lookup miss whenever at least one editor
    /// window was on screen — i.e. always — falling back to
    /// `screen_zone_hit_test` without ever exercising the new path. Keying
    /// by the real global `idx` instead of position fixes that regardless
    /// of how many editor windows precede the tab bars.
    cached_tab_bar_zones: Rc<RefCell<HashMap<usize, (core::window::GroupId, quadraui::Rect)>>>,
    /// A sidebar panel claimed the current press, so the rest of the gesture
    /// (`MouseMoved` with the left button held, then `MouseUp`) belongs to it —
    /// that is what lets a panel scrollbar thumb or a tree drag keep tracking
    /// once the pointer strays outside the sidebar. Cleared on release. An
    /// *unclaimed* drag is never intercepted, so an editor text-selection drag
    /// that wanders over the sidebar still finalises in the editor (#544).
    sidebar_pointer_captured: Cell<bool>,
    /// Git-sidebar band geometry (header / commit input / toolbar slab) as the
    /// last `render_content` pass painted it, from `render::sc_sidebar_bands`.
    /// `route_sc_sidebar_event` resolves presses against this so the click and
    /// paint derivations cannot drift (#544).
    cached_sc_bands: Cell<Option<render::ScSidebarBands>>,
    /// Debug-sidebar action-button row rect as last painted. The hit regions in
    /// `engine.dap_sidebar_action_hits` are relative to this rect's origin, so
    /// the router needs it to translate an absolute press (#544).
    cached_dap_action_rect: Cell<Option<quadraui::Rect>>,
    /// AI-sidebar band geometry (header / message history / input) as the
    /// last `render_content` pass painted it, from
    /// `render::draw_ai_sidebar_panel`. `route_ai_sidebar_event` resolves
    /// presses against this so the click and paint derivations cannot drift
    /// (#544/#730).
    cached_ai_bands: Cell<Option<render::AiSidebarBands>>,
    /// Per-group tab-drop geometry (absolute pixel bounds) computed each frame in
    /// `render_content`. Both the drag overlay (same frame) and the drag hit-test
    /// in `handle_mouse_drag_msg` (next mouse-move) read this, so the drop-zone
    /// detection and the highlight always use one identical bounds source. (#515)
    cached_drop_groups: Rc<RefCell<Vec<render::TabDropGroup>>>,
    /// Effective tab-bar height (px) paired with `cached_drop_groups`.
    cached_drop_tbh: Rc<Cell<f32>>,
    /// Backend (line_height, char_width) captured at the instant the file
    /// explorer tree was rendered. The backend's `current_line_height` is mutable
    /// per-frame state and may differ by click time, which made the explorer
    /// hit-test resolve the wrong row (it ran `tree_layout` at a different line
    /// height than `draw_tree` used). Re-applied before hit-testing so draw and
    /// hit agree. (#540 ShellApp port)
    cached_explorer_metrics: Rc<Cell<(f64, f64)>>,
    /// Pixel y-offset where the debug toolbar was last drawn.
    debug_toolbar_y_offset: Rc<Cell<f64>>,
    /// Pixel height of the debug toolbar (last draw).
    debug_toolbar_height: Rc<Cell<f64>>,
    /// Cached menu-dropdown hit regions from the last draw of the
    /// dropdown overlay. Each entry is `(x, y, w, h, action_id)`
    /// where `action_id` is e.g. `menu:7`. Click + motion handlers
    /// walk this list to map (x, y) → engine-side
    /// `MENU_STRUCTURE.items` index instead of computing row indices
    /// True while the user drags the terminal header row to resize the panel.
    terminal_resize_dragging: bool,
    /// True while the user drags the terminal split divider left/right.
    terminal_split_dragging: bool,
    /// The divider currently grabbed — an editor-group boundary or a
    /// `:split`/`:vsplit` window boundary (#582; each group's `WindowLayout`
    /// numbers its own splits independently, so the owning group travels with
    /// it). #753 collapsed the two mutually-exclusive `group_divider_dragging`
    /// / `window_divider_dragging` fields into the shared
    /// [`render::DividerGrab`], which TUI holds too.
    divider_grab: Option<render::DividerGrab>,
    /// Tab drag-and-drop arm → threshold → track → commit machine. #753
    /// replaced the four parallel fields (`tab_dragging`, `tab_drag_start`,
    /// `tab_drag_source`, `tab_drag_drop_zone`) with the shared
    /// [`render::TabDragState`], which TUI holds too.
    tab_drag: render::TabDragState,
    /// GTK window handle — set in `ShellApp::setup` once the runner creates the window.
    window: Option<gtk4::Window>,
    /// Editor content bounds + tab-bar height as used by the LAST
    /// `render_content` pass, in the same **absolute** DA coordinate frame
    /// that mouse events arrive in (#550, #582).
    ///
    /// `render_content` derives `editor_bounds` from
    /// `AppShellLayout::main_content_bounds`, whose origin is offset by the
    /// activity bar / sidebar (x) and the title-bar band (y). The divider
    /// hit-test and drag handlers used to re-derive their own bounds at
    /// `(0.0, 0.0)` with a *different* height formula — so every divider they
    /// computed sat roughly one activity-bar-width left (and one title-bar
    /// height up) of the line actually painted. `:vsplit` dividers were
    /// consequently unhittable and the press fell through to text-selection
    /// (#582 iteration-2 smoke failure); `:split` only appeared to work
    /// because its y-error was small enough that a click on the per-window
    /// status bar landed inside the 6px band by luck.
    ///
    /// Caching what the renderer actually used — rather than recomputing —
    /// makes hit-test-agrees-with-paint true *by construction* instead of by
    /// two formulas being kept in sync by hand.
    cached_editor_bounds: Cell<Option<(core::WindowRect, f64)>>,
    /// Menu bar row rect (full content width, `lh` tall) computed in
    /// `render_content` each frame. Reused by `handle()` so `MenuSystem`'s
    /// click/key routing tests against the exact rect the bar was drawn
    /// into. (#552)
    ///
    /// `Rc`-wrapped (like [`Self::title_bar_rect`]) so the headless test
    /// harness can clone a handle and *aim* pixel probes at the row the
    /// renderer actually used, instead of hardcoding chrome coordinates (#720).
    menu_row_rect: Rc<Cell<quadraui::Rect>>,
    /// The sub-rect of [`Self::menu_row_rect`] the menu *items* were actually
    /// drawn into — `menu_row_rect` minus the app-icon slot at its leading
    /// edge (#720). Equal to `menu_row_rect` when the icon slot is empty
    /// (menu bar hidden / zero-height row).
    ///
    /// `handle()` routes `MenuSystem` clicks against **this**, not
    /// `menu_row_rect`: the icon shifts every item's x-origin right, so a
    /// hit-test run against the unshifted band would resolve a click on
    /// `File` to whatever item now sits one slot to its left. Written once
    /// per frame by `render_content` from the same
    /// `render::split_menu_row_for_app_icon` call that positions the paint,
    /// so paint and hit-test cannot disagree (the #552 `TabBar` bug class,
    /// which quadraui's `MenuBar::layout_with_leading` doc calls out for
    /// exactly this feature).
    menu_items_rect: Cell<quadraui::Rect>,
    /// Rect of the drawn inline window-control buttons (minimize/maximize/
    /// close), to the right of the menu items within `menu_row_rect`. (#552)
    /// `Rc`-wrapped (like `picker_popup_rect` etc.) so the headless test
    /// harness can clone a handle and assert the Command Center (#676)
    /// never overlaps it.
    title_bar_rect: Rc<Cell<quadraui::Rect>>,
    /// Hover/press/click tracker for the window-control buttons, shared with
    /// quadraui's own `full_chrome_demo` reference title bar (quadraui#402)
    /// — replaces a hand-rolled `StatusBarLayout::hit_test` call with the
    /// same primitive so the buttons get real hover/press highlighting and
    /// click-on-release semantics instead of firing on press. (#552)
    title_bar_interaction: RefCell<quadraui::StatusBarInteraction>,
    /// Last time sc_refresh() was called for the Git sidebar auto-refresh.
    last_sc_refresh: std::time::Instant,
    /// Link hit rects populated during hover popup draw: (x, y, w, h, url, is_native).
    #[allow(clippy::type_complexity)]
    panel_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String, bool)>>>,
    /// Popup bounding rect (x, y, w, h) — set during draw, used for motion hit-testing.
    #[allow(dead_code, clippy::type_complexity)]
    panel_hover_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Editor hover popup bounding rect (x, y, w, h) — set during draw, used for click hit-testing.
    #[allow(clippy::type_complexity)]
    editor_hover_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Completion popup layout — set during draw, used for hit-test in
    /// the click handler. None when the popup isn't visible.
    completion_layout: Rc<RefCell<Option<quadraui::CompletionsLayout>>>,
    /// Context menu layout — set during draw, used for hit-test in
    /// both click and motion handlers. None when no menu is visible.
    context_menu_layout: Rc<RefCell<Option<quadraui::ContextMenuLayout>>>,
    /// Tab-switcher popup bounding rect (x, y, w, h) — set during
    /// draw, used for `ModalStack` registration in the click
    /// handler. (B.5b Stage 7.)
    #[allow(clippy::type_complexity)]
    tab_switcher_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// The overlay rungs this frame actually painted, in paint order (#735).
    ///
    /// Written by the [`render::OVERLAY_Z_ORDER`] walk at the tail of
    /// `render_content` — every arm that draws pushes its own
    /// [`render::OverlayOp`], and arms that only clear a stale hit-test cache
    /// do not. It is the *observable* that makes "both backends compose the
    /// overlay band in the same order" testable: `TuiShellApp` keeps the
    /// identical field, and the two backends' recorded sequences are asserted
    /// equal against the same expected `Vec<OverlayOp>`.
    ///
    /// Cheap enough to keep in release builds (at most
    /// `OVERLAY_Z_ORDER.len()` pushes into a reused `Vec` per frame) and
    /// useful there too — `check_overlay_band_order` turns a z-order
    /// inversion into a diagnosable string rather than a visual mystery.
    painted_overlay_band: Rc<RefCell<Vec<render::OverlayOp>>>,
    /// Picker/command-palette popup rect `(x, y, w, h)` **as the last frame
    /// actually painted it** — see [`App::compute_picker_popup_bounds`] for
    /// why the click path must not re-derive it (#555).
    #[allow(clippy::type_complexity)]
    picker_popup_rect: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    /// Line height the last frame actually painted with, published by
    /// `render_content` — see [`App::painted_line_height`] (#555).
    painted_line_height: Rc<Cell<Option<f64>>>,
    /// Character-cell advance the last frame actually painted with — the
    /// horizontal twin of [`Self::painted_line_height`], and published for
    /// exactly the same reason (#751).
    ///
    /// `render_content` paints at `cached_char_width.max(backend.char_width())`,
    /// but click-time hit-tests read the plain `cached_char_width`, which is
    /// seeded once in `setup()` from the runner's *default* metrics. With a
    /// real font those differ (8.0 vs. ~9.14 in the headless harness), so a
    /// cell-unit overlay hit-tested against the smaller value drifted further
    /// right the further into the panel the pointer went — the find/replace
    /// toggles resolved to the *input field* four cells to their left.
    painted_char_width: Rc<Cell<Option<f64>>>,
    /// The sidebar content area the last frame painted a panel into
    /// (`ShellContext::layout.sidebar_content_bounds`), or `None` when the
    /// sidebar was hidden. Published purely so the headless harness can aim a
    /// click at the panel the renderer actually drew instead of guessing pixel
    /// offsets — the same "locate targets, never hardcode coords" rule
    /// `screen_layout` / `tab_slots_abs` exist for (#544).
    painted_sidebar_bounds: Rc<Cell<Option<quadraui::Rect>>>,
    /// Link hit rects populated during editor hover popup draw: (x, y, w, h, url).
    #[allow(clippy::type_complexity)]
    editor_hover_link_rects: Rc<RefCell<Vec<(f64, f64, f64, f64, String)>>>,
    /// Editor hover popup scrollbar geometry (#215). Populated by
    /// `draw_editor_hover_popup`; consumed by click + drag handlers
    /// in this file.
    editor_hover_scrollbar: Rc<Cell<Option<render::PopupScrollbarHit>>>,
    /// CSS provider registered with the GTK display — updated when colorscheme changes.
    ///
    /// `None` only under the headless test harness ([`App::new_headless`], #646):
    /// `gtk4::CssProvider::new()` asserts `gtk::init` has run, which it cannot
    /// with no display, and a provider that is attached to no `GdkDisplay`
    /// styles nothing anyway. Always `Some` in a live run.
    css_provider: Option<gtk4::CssProvider>,
    /// Colorscheme name at the time the CSS was last applied.
    last_colorscheme: String,
    /// Cross-backend modal-overlay tracking. Pushed to when a palette /
    /// `quadraui::Backend`-impl handle. Owns the canonical
    /// accelerators / event-queue / viewport / services / modal-stack /
    /// drag-state. Call sites reach modal-stack and drag-state via
    /// `self.backend.borrow().modal_stack_handle()` and
    /// `drag_state_handle()` (B.5b Stage 11 dropped the alias `Rc`
    /// clones that previously lived at `App.modal_stack` /
    /// `App.drag_state`). The `init` drain timer holds a clone and
    /// pumps `poll_events()` every 16 ms.
    backend: Rc<RefCell<backend::GtkBackend>>,
}

/// Decode an activity bar widget ID into a panel ID for [`App::switch_panel`].
/// Dead in ShellApp mode until the activity bar DA is re-wired (#448-C follow-on).
#[allow(dead_code)]
fn activity_id_to_panel_id(id: &str) -> Option<String> {
    match id {
        "activity:explorer" => Some(PANEL_EXPLORER.to_string()),
        "activity:search" => Some(PANEL_SEARCH.to_string()),
        "activity:debug" => Some(PANEL_DEBUG.to_string()),
        "activity:git" => Some(PANEL_GIT.to_string()),
        "activity:extensions" => Some(PANEL_EXTENSIONS.to_string()),
        "activity:ai" => Some(PANEL_AI.to_string()),
        "activity:settings" => Some(PANEL_SETTINGS.to_string()),
        other => other
            .strip_prefix("activity:ext:")
            .map(|name| format!("ext:{name}")),
    }
}

/// Map GDK key names to the engine's expected key names.
///
/// This is the canonical superset mapping — callers that only care about a
/// subset simply ignore the extra translations (they're harmless).
fn map_gtk_key_name(gdk_name: &str) -> &str {
    match gdk_name {
        "Return" | "KP_Enter" => "Return",
        "Escape" => "Escape",
        "BackSpace" => "BackSpace",
        "Delete" => "Delete",
        "Tab" => "Tab",
        "ISO_Left_Tab" => "BackTab",
        "Up" => "Up",
        "Down" => "Down",
        "Left" => "Left",
        "Right" => "Right",
        "Home" => "Home",
        "End" => "End",
        "Page_Down" | "KP_Page_Down" => "PageDown",
        "Page_Up" | "KP_Page_Up" => "PageUp",
        "space" => " ",
        "slash" => "/",
        "question" => "?",
        other => other,
    }
}

fn gtk_key_name_to_quadraui(mapped: &str, ctrl: bool) -> Option<quadraui::UiEvent> {
    use quadraui::{Key, Modifiers, NamedKey, UiEvent};
    let key = match mapped {
        "Down" => Key::Named(NamedKey::Down),
        "Up" => Key::Named(NamedKey::Up),
        "Home" => Key::Named(NamedKey::Home),
        "End" => Key::Named(NamedKey::End),
        "PageDown" => Key::Named(NamedKey::PageDown),
        "PageUp" => Key::Named(NamedKey::PageUp),
        "Tab" => Key::Named(NamedKey::Tab),
        "Return" => Key::Named(NamedKey::Enter),
        " " => Key::Char(' '),
        "j" => Key::Char('j'),
        "k" => Key::Char('k'),
        "g" => Key::Char('g'),
        "G" => Key::Char('G'),
        _ => return None,
    };
    Some(UiEvent::KeyPressed {
        key,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::default()
        },
        repeat: false,
    })
}

/// Map a GDK key name and extract the unicode character for input-mode handlers.
///
/// Returns `(mapped_key_name, unicode)`.  Special keys return `None` for unicode;
/// single-character key names return the character as `Some(ch)`.
fn map_gtk_key_with_unicode(gdk_name: &str) -> (&str, Option<char>) {
    match gdk_name {
        "Return" | "KP_Enter" => ("Return", None),
        "Escape" => ("Escape", None),
        "BackSpace" => ("BackSpace", None),
        "Delete" => ("Delete", None),
        "Up" => ("Up", None),
        "Down" => ("Down", None),
        "Left" => ("Left", None),
        "Right" => ("Right", None),
        "Home" => ("Home", None),
        "End" => ("End", None),
        "Tab" | "ISO_Left_Tab" => ("Tab", None),
        "Page_Up" => ("Page_Up", None),
        "Page_Down" => ("Page_Down", None),
        "question" => ("?", Some('?')),
        "slash" => ("/", Some('/')),
        other => {
            let mut chars = other.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                (other, Some(ch))
            } else {
                (other, None)
            }
        }
    }
}

/// Set up system clipboard callbacks on the engine via copypasta_ext.
///
/// On X11 we prefer `x11_bin` (xclip/xsel subprocesses) over `try_context`'s
/// default `x11_fork`: the fork variant opens its own in-process X11 connection
/// and contends with GTK's main-thread X11 event loop. Subprocess reads do not.
fn setup_gtk_clipboard(engine: &mut Engine) {
    let ctx: Option<Box<dyn ClipboardProviderExt>> = {
        #[cfg(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        ))]
        if copypasta_ext::display::is_x11() {
            copypasta_ext::x11_bin::ClipboardContext::new()
                .ok()
                .map(|c| Box::new(c) as Box<dyn ClipboardProviderExt>)
                .or_else(|| {
                    // xclip/xsel aren't on PATH, so `x11_bin` failed. Do NOT
                    // fall back to `copypasta_ext::try_context()` here — on
                    // X11 that prefers `x11_fork::ClipboardContext` by
                    // default (#587 Problem 2, discovered via manual GTK
                    // smoke test): its `set_contents` calls `fork()` inside
                    // this GTK4 process, and its `get_contents` opens its
                    // own in-process X11 connection. Forking a process with
                    // GTK's thread pool, glib workers, gdbus and Cairo/Mesa
                    // threads risks the child inheriting a mutex locked by a
                    // thread that doesn't exist in the child and hanging
                    // forever; the extra connection also contends with
                    // GTK's main-thread X11 event loop per the module doc
                    // above. Any machine without xclip/xsel installed hit
                    // this fallback and froze the whole app on clipboard
                    // access. `X11ClipboardContext` (used here directly,
                    // bypassing `x11_fork`) does the same I/O on a
                    // background thread with a bounded (3s) read timeout
                    // and never calls `fork()`.
                    use copypasta_ext::copypasta::x11_clipboard::{Clipboard, X11ClipboardContext};
                    X11ClipboardContext::<Clipboard>::new()
                        .ok()
                        .map(|c| Box::new(c) as Box<dyn ClipboardProviderExt>)
                })
        } else {
            copypasta_ext::try_context()
        }
        #[cfg(not(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )))]
        copypasta_ext::try_context()
    };

    let Some(ctx) = ctx else { return };
    // `engine.clipboard_{read,write}` are `Fn` (shared-ref callbacks), but
    // `ClipboardProviderExt::{get,set}_contents` take `&mut self`. Wrap the
    // provider in `Rc<RefCell<…>>` so both closures can share it and acquire
    // a mutable borrow at call time.
    let ctx = Rc::new(RefCell::new(ctx));

    let read_ctx = ctx.clone();
    engine.clipboard_read = Some(Box::new(move || {
        read_ctx
            .borrow_mut()
            .get_contents()
            .map_err(|e| format!("clipboard read: {e}"))
    }));

    let write_ctx = ctx;
    engine.clipboard_write = Some(Box::new(move |text: &str| {
        write_ctx
            .borrow_mut()
            .set_contents(text.to_string())
            .map_err(|e| format!("clipboard write: {e}"))
    }));
}

/// A native file dialog requested by [`App::open_file_dialog`] /
/// [`App::save_workspace_as_dialog`], deferred to the next `tick()` (#572).
///
/// Neither of those methods has a
/// `backend: &mut dyn quadraui::Backend` in scope, but `PlatformServices`
/// (and the re-entrancy-guarded nested-mainloop pump backing it, see
/// `quadraui::gtk::services` #427) is only reachable through that
/// runner-owned `backend` parameter. `tick()` receives it every frame, so
/// the request is stashed here and drained there instead of threading
/// `backend` through the whole `dispatch_engine_action`/`handle_menu_action`
/// call graph.
///
/// Deliberately **not** routed through `self.backend` (`App`'s own
/// `Rc<RefCell<GtkBackend>>`, used for modal-stack/drag-state handles) —
/// that is a separate `GtkBackend` instance from the one
/// `quadraui::gtk::run::run` constructs internally and passes as the
/// trait-object `backend` param. Its `PlatformServices` has its own,
/// unshared `pump_depth` counter, so pumping the nested main loop through
/// it would not signal the runner's own event controllers to skip their
/// `backend.borrow_mut()` calls — reintroducing the double-borrow panic
/// #427's guard exists to prevent.
#[derive(Debug, Clone, Copy)]
enum PendingFileDialog {
    OpenFile,
    SaveWorkspaceAs,
}

/// Parse the button index out of a `"dialog:btn:N"` id — the synthesized
/// id convention `render::dialog_panel_to_quadraui_dialog` uses (backends
/// dispatch clicks by index via `Engine::dialog_click_button(idx)`, since
/// `DialogPanel.buttons` carries no engine-side id). Shared (#727) by the
/// in-canvas `DialogHit::Button(id)` hit-test path and the native
/// message-dialog response mapping so both parse the id the same way.
fn dialog_btn_index(id: &quadraui::WidgetId) -> Option<usize> {
    id.as_str()
        .strip_prefix("dialog:btn:")
        .and_then(|s| s.parse::<usize>().ok())
}

/// Create a new `App` instance.
///
/// All widget-dependent setup (window handle, CSS) is deferred to
/// `ShellApp::setup()`, called by the runner once the window exists.
impl App {
    fn new(file_path: Option<PathBuf>) -> Self {
        // Icon search path setup.
        if let Some(home) = std::env::var_os("HOME") {
            let icon_dir = std::path::PathBuf::from(home).join(".local/share/icons");
            if let Some(display) = gdk::Display::default() {
                let icon_theme = gtk4::IconTheme::for_display(&display);
                icon_theme.add_search_path(&icon_dir);
            }
        }
        install_bundled_icon_font();

        let mut engine = {
            let mut e = Engine::new();
            icons::set_nerd_fonts(e.settings.use_nerd_fonts);
            e.startup(file_path.as_deref());
            e
        };
        setup_gtk_clipboard(&mut engine);

        let initial_theme = Theme::from_name(&engine.settings.colorscheme);
        let css_provider = Some(load_css(&initial_theme));
        let last_colorscheme = engine.settings.colorscheme.clone();
        if let Some(gtk_settings) = gtk4::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(!initial_theme.is_light());
        }

        let engine = Rc::new(RefCell::new(engine));
        unsafe {
            crate::core::swap::register_emergency_engine(
                engine.as_ptr() as *const crate::core::Engine
            );
        }

        let deferred = DeferredQueue::new();

        // File watcher for settings.json hot-reload.
        let settings_path = std::env::var("HOME")
            .map(|h| format!("{}/.config/vimcode/settings.json", h))
            .unwrap_or_else(|_| ".config/vimcode/settings.json".to_string());
        let file = gio::File::for_path(&settings_path);
        let settings_monitor =
            match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
                Ok(monitor) => {
                    let deferred_for_monitor = deferred.clone();
                    monitor.connect_changed(move |_, _, _, event| {
                        if event == gio::FileMonitorEvent::ChangesDoneHint {
                            deferred_for_monitor.send(DeferredAction::SettingsFileChanged);
                        }
                    });
                    Some(monitor)
                }
                Err(_) => None,
            };

        Self::assemble(
            engine,
            deferred,
            css_provider,
            last_colorscheme,
            settings_monitor,
        )
    }

    /// Build the `App` struct itself from already-prepared, display-*independent*
    /// inputs.
    ///
    /// Split out of [`App::new`] (#646) so the headless GTK test harness
    /// ([`App::new_headless`]) can reach the same field initialisation without
    /// re-running `new`'s display-dependent prologue — `load_css` unwraps
    /// `gdk::Display::default()` and panics outright with no `DISPLAY`, and
    /// `register_emergency_engine` would leave a dangling `*const Engine` in a
    /// process-global once a short-lived test's `App` is dropped (the same
    /// soundness trap #635 documented on `TuiShellApp::live`).
    ///
    /// Everything below this line is plain `Rc`/`Cell`/`RefCell` allocation
    /// plus a `GtkBackend::new()`; none of it touches GDK.
    fn assemble(
        engine: Rc<RefCell<Engine>>,
        deferred: DeferredQueue,
        css_provider: Option<gtk4::CssProvider>,
        last_colorscheme: String,
        settings_monitor: Option<gio::FileMonitor>,
    ) -> Self {
        let backend = Rc::new(RefCell::new(backend::GtkBackend::new()));

        App {
            engine,
            draw_needed: Rc::new(Cell::new(false)),
            pending_file_dialog: Cell::new(None),
            cached_line_height: 24.0,
            cached_char_width: 9.0,
            last_editor_pointer: Rc::new(Cell::new(None)),
            cached_ui_line_height: 20.0,
            dialog_layout: Rc::new(RefCell::new(None)),
            native_dialog_shown: Rc::new(Cell::new(false)),
            pending_native_dialog: Rc::new(Cell::new(None)),
            line_height_cell: Rc::new(Cell::new(24.0)),
            char_width_cell: Rc::new(Cell::new(9.0)),
            mouse_pos_cell: Rc::new(Cell::new((-1.0, -1.0))),
            h_sb_drag_cell: Rc::new(Cell::new(None)),
            fr_input_dragging: false,
            settings_monitor,
            deferred,
            last_clipboard_content: None,
            tab_close_hover: None,
            tab_slot_positions: Rc::new(RefCell::new(HashMap::new())),
            cached_tab_close_abs: Rc::new(RefCell::new(HashMap::new())),
            cached_tab_slots_abs: Rc::new(RefCell::new(HashMap::new())),
            cached_tab_pixel_hits: Rc::new(RefCell::new(HashMap::new())),
            diff_btn_map: Rc::new(RefCell::new(HashMap::new())),
            split_btn_map: Rc::new(RefCell::new(HashMap::new())),
            action_btn_map: Rc::new(RefCell::new(HashMap::new())),
            status_segment_map: Rc::new(RefCell::new(HashMap::new())),
            separated_status_bar_rect: Rc::new(Cell::new(None)),
            global_status_zones: Rc::new(RefCell::new(Vec::new())),
            cached_screen_layout: Rc::new(RefCell::new(None)),
            cached_frame_hit_map: Rc::new(RefCell::new(None)),
            sidebar_pointer_captured: Cell::new(false),
            cached_sc_bands: Cell::new(None),
            cached_dap_action_rect: Cell::new(None),
            cached_ai_bands: Cell::new(None),
            cached_tab_bar_zones: Rc::new(RefCell::new(HashMap::new())),
            cached_drop_groups: Rc::new(RefCell::new(Vec::new())),
            cached_drop_tbh: Rc::new(Cell::new(0.0)),
            cached_explorer_metrics: Rc::new(Cell::new((16.0, 8.0))),
            debug_toolbar_y_offset: Rc::new(Cell::new(0.0)),
            debug_toolbar_height: Rc::new(Cell::new(0.0)),
            terminal_resize_dragging: false,
            terminal_split_dragging: false,
            divider_grab: None,
            tab_drag: render::TabDragState::default(),
            window: None,
            cached_editor_bounds: Cell::new(None),
            menu_row_rect: Rc::new(Cell::new(quadraui::Rect::default())),
            menu_items_rect: Cell::new(quadraui::Rect::default()),
            title_bar_rect: Rc::new(Cell::new(quadraui::Rect::default())),
            title_bar_interaction: RefCell::new(quadraui::StatusBarInteraction::new()),
            last_sc_refresh: std::time::Instant::now(),
            panel_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            panel_hover_popup_rect: Rc::new(Cell::new(None)),
            editor_hover_popup_rect: Rc::new(Cell::new(None)),
            completion_layout: Rc::new(RefCell::new(None)),
            context_menu_layout: Rc::new(RefCell::new(None)),
            tab_switcher_popup_rect: Rc::new(Cell::new(None)),
            painted_overlay_band: Rc::new(RefCell::new(Vec::new())),
            picker_popup_rect: Rc::new(Cell::new(None)),
            painted_sidebar_bounds: Rc::new(Cell::new(None)),
            painted_line_height: Rc::new(Cell::new(None)),
            painted_char_width: Rc::new(Cell::new(None)),
            editor_hover_link_rects: Rc::new(RefCell::new(Vec::new())),
            editor_hover_scrollbar: Rc::new(Cell::new(None)),
            css_provider,
            last_colorscheme,
            backend,
        }
    }

    /// Build an `App` around a caller-supplied, fully in-memory [`Engine`] with
    /// **no** display-dependent setup — the GTK twin of `TuiShellApp::new` for
    /// tests (#646). Feed the result to `crate::gtk::testing::harness`, which
    /// wraps it in `quadraui::gtk::testing::driver_with_shell`.
    ///
    /// Deliberately skips, relative to [`App::new`]:
    ///
    /// - `gdk::Display::default()` icon-theme search paths and
    ///   `install_bundled_icon_font` (writes to `~/.local/share/fonts` and
    ///   shells out to `fc-cache` — a test must not touch the user's system).
    /// - `load_css`, which `unwrap()`s `gdk::Display::default()` and therefore
    ///   panics with no `DISPLAY`. `css_provider` is left `None` — even
    ///   `gtk4::CssProvider::new()` asserts `gtk::init` has run, and a provider
    ///   attached to no display styles nothing.
    /// - `gtk4::Settings::default()` (needs a display).
    /// - `setup_gtk_clipboard`, which probes X11 / spawns `xclip`.
    /// - `Engine::startup`, which would restore *the developer's real last
    ///   session*. Tests pass the exact buffers/groups they mean to assert on.
    /// - `core::swap::register_emergency_engine`, whose contract is that the
    ///   engine outlives the process; a test's `App` is dropped at the end of
    ///   the test function, leaving a dangling `*const Engine` for any later
    ///   test's panic hook to dereference (#635 documents the same trap on the
    ///   TUI side).
    /// - The `gio` settings-file monitor (no runtime main loop to service it).
    ///
    /// Takes the engine behind an `Rc` so the caller keeps a handle for
    /// assertions — `GtkDriver` only exposes the opaque `ShellAdapter`, with no
    /// accessor back to the concrete `App` (the same constraint the TUI tests
    /// document on `driver_with_shell`).
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn new_headless(engine: Rc<RefCell<Engine>>) -> Self {
        let (use_nerd_fonts, last_colorscheme) = {
            let e = engine.borrow();
            (e.settings.use_nerd_fonts, e.settings.colorscheme.clone())
        };
        icons::set_nerd_fonts(use_nerd_fonts);
        Self::assemble(engine, DeferredQueue::new(), None, last_colorscheme, None)
    }
}

impl App {
    /// Run a file dialog requested via [`PendingFileDialog`] (#572), using
    /// the runner-owned `backend`'s `PlatformServices` — `show_file_open_dialog`
    /// / `show_file_save_dialog` block (via quadraui's nested-mainloop pump,
    /// #427) until the user picks or cancels, then this returns synchronously.
    fn run_pending_file_dialog(
        &mut self,
        req: PendingFileDialog,
        backend: &mut dyn quadraui::Backend,
    ) {
        use quadraui::FileDialogOptions;
        match req {
            PendingFileDialog::OpenFile => {
                let path = backend.services().show_file_open_dialog(FileDialogOptions {
                    title: Some("Open File".to_string()),
                    ..Default::default()
                });
                if let Some(path) = path {
                    let _ = self
                        .engine
                        .borrow_mut()
                        .open_file_with_mode(&path, crate::core::engine::OpenMode::Permanent);
                    self.refresh_file_tree();
                }
            }
            PendingFileDialog::SaveWorkspaceAs => {
                let path = backend.services().show_file_save_dialog(FileDialogOptions {
                    title: Some("Save Workspace As".to_string()),
                    initial_filename: Some(".vimcode-workspace".to_string()),
                    ..Default::default()
                });
                if let Some(path) = path {
                    self.engine.borrow_mut().save_workspace_as(&path);
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Present the native message dialog queued by `render_content`'s
    /// edge-trigger check (#727), using the runner-owned `backend`'s
    /// `PlatformServices` — `show_message_dialog` blocks (via quadraui's
    /// nested-mainloop pump, #666, the same adapter #427's file dialogs
    /// use) until the user picks a button or dismisses it. Mirrors
    /// `run_pending_file_dialog` above.
    ///
    /// Maps the response back through the same `"dialog:btn:N"` id
    /// convention and `Engine::dialog_click_button` / `Engine::dialog_cancel`
    /// the in-canvas `DialogHit::Button(id)` mouse path
    /// (`handle_mouse_click_msg`) already uses — `None` (dismissed with no
    /// button chosen: Escape, close box) maps to `dialog_cancel()`, the
    /// same outcome the in-canvas dialog's Escape key produces — so both
    /// paths funnel through the identical `EngineAction` outcomes.
    fn run_pending_native_dialog(
        &mut self,
        opts: quadraui::MessageDialogOptions,
        backend: &mut dyn quadraui::Backend,
    ) {
        let choice = backend.services().show_message_dialog(opts);
        // Reset the edge-trigger flag *here*, before running the engine
        // callback below, rather than only lazily on the next
        // `render_content` call that observes `screen.dialog == None`. If
        // `dialog_click_button`/`dialog_cancel` ever opens a second dialog
        // synchronously (e.g. a chained "save failed, retry?" prompt), that
        // new dialog needs `native_dialog_shown == false` to be seen as a
        // fresh no-dialog-to-dialog edge and get queued for its own native
        // present — a stale `true` left over from the dialog that just
        // closed would otherwise suppress it silently. No such chain exists
        // in `process_dialog_result` today, but resetting eagerly here
        // costs nothing and removes the latent trap either way.
        self.native_dialog_shown.set(false);
        let action = match choice.as_ref().and_then(dialog_btn_index) {
            Some(idx) => self.engine.borrow_mut().dialog_click_button(idx),
            None => self.engine.borrow_mut().dialog_cancel(),
        };
        self.apply_dialog_action(action);
        self.draw_needed.set(true);
    }

    /// Apply the `EngineAction` produced by dismissing a dialog — clears
    /// `explorer_needs_refresh` (some dialog outcomes, e.g. "Discard &
    /// Close", can trigger a sidebar refresh) and handles quit/save-quit.
    /// Shared by the in-canvas mouse-click path
    /// (`handle_mouse_click_msg`'s dialog-button block) and the native
    /// message-dialog path (`run_pending_native_dialog`, #727) so both
    /// produce exactly the same outcome for a given `EngineAction`.
    fn apply_dialog_action(&mut self, action: EngineAction) {
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }
        match action {
            EngineAction::Quit | EngineAction::SaveQuit => {
                self.save_session_and_exit();
            }
            _ => {}
        }
    }

    /// Open the tab context menu for `tab_idx` in `group_id`, anchored at the
    /// click's pixel position.
    ///
    /// #732 tranche 1: was `Msg::TabRightClick`, constructed by
    /// `ShellApp::handle` from a `UiEvent::MouseDown` it already held and
    /// immediately decoded again by `dispatch`.
    fn handle_tab_right_click(
        &mut self,
        group_id: core::window::GroupId,
        tab_idx: usize,
        x: f64,
        y: f64,
    ) {
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        let cx = (x / cw) as u16;
        let cy = (y / lh) as u16;
        self.engine
            .borrow_mut()
            .open_tab_context_menu(group_id, tab_idx, cx, cy);
        self.draw_needed.set(true);
    }

    /// Open the editor (buffer text) context menu at the click's pixel
    /// position, unless a focused modal wants to swallow the click.
    fn handle_editor_right_click(&mut self, x: f64, y: f64) {
        // Swallow if the click landed on a focused modal that
        // wants to consume it (#216 — editor hover popup).
        self.reconcile_editor_hover_modal();
        let stack_rc = self.backend.borrow().modal_stack_handle();
        let in_modal = stack_rc
            .borrow()
            .hit_test(quadraui::Point {
                x: x as f32,
                y: y as f32,
            })
            .is_some();
        if in_modal {
            return;
        }
        let cw = self.cached_char_width.max(1.0);
        let lh = self.cached_line_height.max(1.0);
        let cx = (x / cw) as u16;
        let cy = (y / lh) as u16;
        self.engine.borrow_mut().open_editor_context_menu(cx, cy);
        self.draw_needed.set(true);
    }

    /// Handle a window/viewport resize.
    fn handle_resize(&mut self) {
        // #731: both branches here were gated on `self.overlay` /
        // `self.drawing_area`, permanently `None` under the
        // ShellApp runner (nothing assigns either field) — so
        // this was already a no-op: the backend viewport is
        // re-derived every frame by the runner itself, and
        // terminal-pane resize-on-window-resize has not fired
        // since the #540 cutover. Re-deriving live terminal
        // pane sizing needs a way to read the live DA's pixel
        // size without a widget handle — see `terminal_cols`.
        self.draw_needed.set(true);
    }

    /// Ctrl+Click — plant a secondary cursor at the clicked buffer position.
    ///
    /// The retired `Msg::CtrlMouseClick` also carried `width`/`height`, but
    /// the arm bound both to `_`, so they are dropped from the signature
    /// rather than threaded through unused.
    fn handle_ctrl_mouse_click(&mut self, x: f64, y: f64) {
        let layout_ref = self.cached_screen_layout.borrow();
        if let Some(ref layout) = *layout_ref {
            let mut engine = self.engine.borrow_mut();
            if !engine.picker_open {
                if let ClickTarget::BufferPos(_, line, col) = pixel_to_click_target(
                    &mut engine,
                    &self.backend,
                    x,
                    y,
                    self.cached_line_height,
                    self.cached_char_width,
                    layout,
                    &self.cached_tab_pixel_hits.borrow(),
                    &self.tab_slot_positions.borrow(),
                    &self.diff_btn_map.borrow(),
                    &self.split_btn_map.borrow(),
                    &self.action_btn_map.borrow(),
                    self.cached_frame_hit_map.borrow().as_ref(),
                    &self.cached_tab_bar_zones.borrow(),
                    true, // real click: focus/tab/gutter side effects are intended
                ) {
                    engine.add_cursor_at_pos(line, col);
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Double-click in the editor drawing area at the given pixel position.
    ///
    /// As with [`App::handle_ctrl_mouse_click`], the `width`/`height` the
    /// retired `Msg::MouseDoubleClick` carried were bound to `_` and are
    /// dropped from the signature.
    fn handle_mouse_double_click_msg(&mut self, x: f64, y: f64) {
        // #490: a double-click landing on the editor hover popup used to fall
        // straight through to the editor's word-select underneath, because
        // this handler never consulted the popup at all. It runs the same
        // shared rung the single-click path does, first.
        if self.route_and_apply_editor_hover_popup(x, y) {
            return;
        }
        let mut engine = self.engine.borrow_mut();
        if engine.picker_open {
            let in_tree_mode = engine.picker_source
                == crate::core::engine::PickerSource::CommandCenter
                && engine.picker_query == "@";
            if in_tree_mode && engine.picker_toggle_expand() {
                engine.picker_load_preview();
            } else {
                let _action = engine.picker_confirm();
            }
            self.draw_needed.set(true);
        } else {
            // Breadcrumb double-click: same shared resolution as the
            // single-click path above (#555). This used to re-derive
            // the bar's geometry by hand — `y >= lh && y < lh * 2.0`
            // plus a per-`char_width` walk over the *active* group's
            // segments — which is pre-#540 Relm4 geometry: under the
            // ShellApp runner the breadcrumb row sits below the title
            // bar, the menu bar and a `1.6 * lh` tab row, so that band
            // never contained it (double-click was dead) while still
            // matching chrome rows that could fire the wrong segment.
            let mut bc_handled = false;
            if engine.settings.breadcrumbs {
                let lh = self.painted_line_height();
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    match render::resolve_breadcrumb_click(&layout.breadcrumbs, x, y, lh) {
                        render::BreadcrumbClickResult::Hit(group_id, idx) => {
                            drop(layout_ref);
                            engine.handle_breadcrumb_double_click(group_id, idx);
                            bc_handled = true;
                        }
                        render::BreadcrumbClickResult::OnBar => {
                            bc_handled = true;
                        }
                        render::BreadcrumbClickResult::Miss => {}
                    }
                }
            }
            if !bc_handled {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    handle_mouse_double_click(
                        &mut engine,
                        &self.backend,
                        x,
                        y,
                        self.cached_line_height,
                        self.cached_char_width,
                        layout,
                        &self.cached_tab_pixel_hits.borrow(),
                        &self.tab_slot_positions.borrow(),
                        &self.diff_btn_map.borrow(),
                        &self.split_btn_map.borrow(),
                        &self.action_btn_map.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                    );
                }
            }
        }
        self.draw_needed.set(true);
    }

    /// Mouse wheel over the editor drawing area.
    ///
    /// `delta_y` arrives in **GTK's raw polarity** (positive = wheel down) —
    /// see the negation comment at the `UiEvent::Scroll` call site in
    /// `ShellApp::handle`.
    fn handle_mouse_scroll_msg(&mut self, delta_x: f64, delta_y: f64) {
        let mut engine = self.engine.borrow_mut();
        // Picker open: scroll the picker results.
        //
        // #191: previously used `(delta_y * 3.0).round()`, which
        // rounded small trackpad deltas (dy<0.17) down to 0 and
        // made scrolling feel dead. `.ceil()` on the absolute
        // value guarantees every non-zero event advances at
        // least one row, and the `5.0` amplification is closer
        // to native-app conventions for wheel notches.
        if engine.picker_open && delta_y.abs() > 0.01 {
            let step = (delta_y.abs() * 5.0).ceil() as isize;
            let delta = if delta_y > 0.0 { step } else { -step };
            engine.picker_scroll(delta, 20);
            drop(engine);
            self.draw_needed.set(true);
            return;
        }
        // Route scroll through dispatch_scroll using cached scroll surfaces.
        if let Some((px, py)) = self.last_editor_pointer.get() {
            let surfaces = engine.scroll_surfaces.borrow();
            let scroll_events = quadraui::dispatch_scroll(
                &self.backend.borrow().modal_stack_handle().borrow(),
                &surfaces,
                quadraui::Point {
                    x: px as f32,
                    y: py as f32,
                },
                quadraui::ScrollDelta::new(delta_x as f32, delta_y as f32),
            );
            drop(surfaces);
            for sev in &scroll_events {
                if let quadraui::UiEvent::Scroll {
                    widget: Some(id),
                    delta,
                    ..
                } = sev
                {
                    match id.as_str() {
                        "editor_hover" => {
                            let step = (delta.y * 3.0).round() as i32;
                            engine.editor_hover_scroll(step);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        "debug_output" => {
                            engine.handle_debug_output_scroll(delta.y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        "terminal_scrollback" => {
                            // #533: single shared scroll entry point.
                            // delta.y < 0 = up (into history); > 0 =
                            // down (toward live).  Policy + forwarding
                            // live in Engine::handle_terminal_scroll.
                            engine.handle_terminal_scroll(delta.y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
        // #240: route to the window under the pointer, falling back
        // to the active window when the pointer is missing or over
        // a non-window region. Hovering an unfocused group's pane
        // scrolls *that* pane without changing focus or moving its
        // cursor — matches TUI behaviour.
        // #646: resolve the hovered pane against the bounds
        // `render_content` actually painted with (`cached_editor_bounds`,
        // absolute coords including the activity-bar/sidebar x-offset and
        // the title-bar y-offset), not against a re-derived
        // `(0, 0, da.width(), …)` rect. `self.drawing_area` is never
        // assigned under the ShellApp runner — the runner owns the single
        // DrawingArea — so the old `if let Some(da)` arm never ran and this
        // was unconditionally `None`; and even had it run, a `(0, 0)`
        // origin is the exact coordinate-frame mismatch #582 fixed for
        // divider hit-testing.
        let hovered_window_id = self
            .last_editor_pointer
            .get()
            .zip(self.cached_editor_bounds.get())
            .and_then(|((x, y), (editor_bounds, tab_bar_height))| {
                let (rects, _) = engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
                rects
                    .iter()
                    .find(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
                    .map(|(id, _)| *id)
            });
        if delta_y.abs() > 0.01 {
            let scroll_count = (delta_y * 3.0).round().abs() as usize;
            let active_id = engine.active_window_id();
            let target = hovered_window_id.unwrap_or(active_id);
            if target == active_id {
                let dir = if delta_y > 0.0 { 1 } else { -1 };
                engine.scroll_viewport_with_cursor(dir, scroll_count);
            } else {
                let dir = if delta_y > 0.0 { 1 } else { -1 };
                engine.scroll_viewport_with_cursor_for_window(target, dir, scroll_count);
            }
            engine.sync_scroll_binds();
        }
        if delta_x.abs() > 0.01 {
            let win_id = engine.active_window_id();
            let current = engine.view().scroll_left;
            let scroll_amount = (delta_x * 3.0).round() as isize;
            let new_left = (current as isize + scroll_amount).max(0) as usize;
            engine.set_scroll_left_for_window(win_id, new_left);
        }
        drop(engine);
        self.draw_needed.set(true);
    }

    /// Clear the yank highlight after the flash duration has elapsed.
    fn clear_yank_highlight(&mut self) {
        self.engine.borrow_mut().clear_yank_highlight();
        self.draw_needed.set(true);
    }

    /// `settings.json` changed on disk — reload it and, if the reload took,
    /// refresh the file tree (`show_hidden_files` may have flipped).
    fn settings_file_changed(&mut self) {
        if self.engine.borrow_mut().check_settings_reload() {
            self.refresh_file_tree();
            self.draw_needed.set(true);
        }
    }

    /// Reveal `target` in the explorer sidebar: expand all ancestors,
    /// rebuild the row list, select the matching row, scroll into view,
    /// and queue a redraw of the explorer DrawingArea. Phase A.2b-2
    /// replacement for `highlight_file_in_tree` (which operated on the
    /// native `gtk4::TreeView`).
    fn reveal_path_in_explorer(&self, target: &Path) {
        if let Ok(mut engine) = self.engine.try_borrow_mut() {
            engine.explorer_reveal_path(target);
            drop(engine);
            self.queue_explorer_draw();
        }
    }

    fn refresh_explorer(&self) {
        self.engine.borrow_mut().explorer_rebuild_rows();
        self.queue_explorer_draw();
    }

    /// Save the current session state and schedule process exit via idle callback.
    /// Uses `idle_add_local_once` so `process::exit` runs outside any GTK signal
    /// emission chain, avoiding UB from unwinding through extern "C" trampolines.
    fn save_session_and_exit(&self) {
        let mut engine = self.engine.borrow_mut();
        let buffer_id = engine.active_buffer_id();
        if let Some(path) = engine
            .buffer_manager
            .get(buffer_id)
            .and_then(|s| s.file_path.as_deref())
            .map(|p| p.to_path_buf())
        {
            let view = engine.active_window().view.clone();
            engine.session.save_file_position(
                &path,
                view.cursor.line,
                view.cursor.col,
                view.scroll_top,
            );
        }
        engine.session.window.width = self
            .window
            .as_ref()
            .map(|w| w.default_width())
            .unwrap_or(800);
        engine.session.window.height = self
            .window
            .as_ref()
            .map(|w| w.default_height())
            .unwrap_or(600);
        engine.collect_session_open_files();
        if let Some(ref root) = engine.workspace_root.clone() {
            engine.save_session_for_workspace(root);
        }
        let _ = engine.session.save();
        engine.cleanup_all_swaps();
        engine.lsp_shutdown();
        drop(engine);
        gtk4::glib::idle_add_local_once(|| std::process::exit(0));
    }

    /// Dispatch an `EngineAction` produced by `handle_key` or macro playback.
    ///
    /// `is_macro`: when true, `OpenTerminal` toggles instead of creating a new
    /// tab, and dialog-open actions are suppressed (macros can't drive dialogs).
    fn dispatch_engine_action(&mut self, action: EngineAction, is_macro: bool) {
        match action {
            EngineAction::Quit | EngineAction::SaveQuit => {
                self.save_session_and_exit();
            }
            EngineAction::OpenFile(path) => {
                let mut engine = self.engine.borrow_mut();
                if let Err(e) = engine.open_file_with_mode(&path, OpenMode::Permanent) {
                    engine.message = e;
                }
            }
            EngineAction::OpenTerminal => {
                if is_macro {
                    self.toggle_terminal();
                } else {
                    self.new_terminal_tab();
                }
            }
            EngineAction::ToggleTerminalMaximize => {
                self.toggle_terminal_maximize();
            }
            EngineAction::RunInTerminal(cmd) => {
                self.run_command_in_terminal(cmd);
            }
            EngineAction::OpenFolderDialog => {
                if !is_macro {
                    self.open_folder_dialog();
                }
            }
            EngineAction::OpenWorkspaceDialog => {
                if !is_macro {
                    self.open_workspace_dialog();
                }
            }
            EngineAction::SaveWorkspaceAsDialog => {
                if !is_macro {
                    self.save_workspace_as_dialog();
                }
            }
            EngineAction::OpenRecentDialog => {
                if !is_macro {
                    self.open_recent_dialog();
                }
            }
            EngineAction::QuitWithUnsaved => {
                self.show_quit_confirm();
            }
            EngineAction::ToggleSidebar => {
                // Engine handles this internally; sync local cache.
                self.sync_sidebar_from_engine();
            }
            EngineAction::QuitWithError => {
                let mut engine = self.engine.borrow_mut();
                engine.cleanup_all_swaps();
                engine.lsp_shutdown();
                drop(engine);
                gtk4::glib::idle_add_local_once(|| std::process::exit(1));
            }
            EngineAction::OpenUrl(url) => {
                open_url(&url);
            }
            EngineAction::None | EngineAction::Error => {}
        }
    }

    /// Return focus to the main editor drawing area when a sidebar loses
    /// focus.
    ///
    /// #731: was `if let Some(ref drawing) = *self.drawing_area.borrow()`
    /// — that field is permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has been a no-op since #540. Kept as
    /// a named function (rather than deleting every call site) so the
    /// intent stays legible at each of its ~10 callers; a real fix needs a
    /// live way to grab GTK keyboard focus on the editor DA from here,
    /// which nothing in this file currently has under ShellApp.
    fn focus_editor_if_needed(&self, _still_focused: bool) {}

    /// Sync the unnamed `"` register (and explicit `+` register) to the system clipboard
    /// whenever their content changes (clipboard=unnamedplus semantics).
    fn sync_plus_register_to_clipboard(&mut self) {
        let engine = self.engine.borrow();
        // Check both `"` (auto-yank) and `+` (explicit clipboard writes from plugins)
        let new_content = engine
            .registers
            .get(&'+')
            .filter(|(s, _)| !s.is_empty())
            .map(|(s, _)| s.clone())
            .or_else(|| {
                engine
                    .registers
                    .get(&'"')
                    .filter(|(s, _)| !s.is_empty())
                    .map(|(s, _)| s.clone())
            });

        if new_content != self.last_clipboard_content {
            if let (Some(ref content), Some(ref cb)) = (&new_content, &engine.clipboard_write) {
                let _ = cb(content.as_str());
            }
            drop(engine);
            self.last_clipboard_content = new_content;
        }
    }

    #[allow(clippy::too_many_lines)]
    /// `ctx` is threaded in solely for the shared Alt rung's
    /// [`render::AltKeyOutcome::ResizeSidebar`] arm: GTK's authoritative
    /// sidebar width *is* the runner's `AppShell` (TUI keeps its own copy and
    /// syncs it out at end of dispatch), and `ShellContext::shell_mut` is the
    /// only handle to it.
    fn handle_key_press(
        &mut self,
        key_name: String,
        unicode: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        ctx: &quadraui::ShellContext<'_>,
    ) {
        // ── Shared modal keyboard rung (#734 slice 1) ──────────────────
        // `render::route_modal_key` states the spell-suggestion → dialog →
        // context-menu ladder once for both backends. GTK used to hand-roll
        // the context-menu rung here (and a second copy in the now-deleted
        // `handle_explorer_ctx_menu_key`), diverging from
        // `Engine::handle_context_menu_key` in three ways: `l` did not
        // confirm, `q`/`h` did not close, j/k did not skip disabled items,
        // and every other key both closed the menu *and* fell through to the
        // editor. It also had no top-level dialog rung at all, so a dialog
        // opened while the activity bar / explorer / an extension panel held
        // focus lost its keys to that panel.
        // Bound to a local first: a `RefCell::borrow()` temporary in a `match`
        // scrutinee lives for the whole `match`, and the arms `borrow_mut()`.
        let modal_route = render::route_modal_key(&self.engine.borrow());
        match modal_route {
            render::ModalKeyRoute::Engine => {
                let action = {
                    let mut engine = self.engine.borrow_mut();
                    engine.handle_key(&key_name, unicode, ctrl)
                };
                self.dispatch_engine_action(action, false);
                self.queue_explorer_draw();
                self.draw_needed.set(true);
                return;
            }
            render::ModalKeyRoute::ContextMenu => {
                self.dispatch_context_menu_key(&key_name, unicode);
                return;
            }
            render::ModalKeyRoute::None => {}
        }

        // Dismiss any panel hover popup on key press.
        self.engine.borrow_mut().dismiss_panel_hover_now();

        // ── Shared clipboard-paste pre-load rung (#760 / #734 slice 5) ─────
        // `render::preload_paste_clipboard` states the "if it needs it, read
        // it, load it" glue once for both backends — see its header comment
        // in `render.rs`. GTK has no Ctrl+Shift+V arm to converge here:
        // quadraui's runner intercepts that chord (and plain Ctrl+V) before
        // it ever reaches this method and redelivers it as
        // `UiEvent::ClipboardPaste`, handled by the arm further down this
        // file that already calls `Engine::route_paste` directly.
        render::preload_paste_clipboard(&mut self.engine.borrow_mut(), &key_name, unicode, ctrl);

        // ── Shared focus-owner keyboard rung (#757 / #734 slice 2) ─────
        // `render::route_focus_key` states the activity-bar → sidebar-panel
        // ladder once for both backends; GTK used to hand-roll it as a chain
        // of `if engine.*_has_focus` blocks that disagreed with TUI's chain in
        // four ways, all enumerated (with the resolution) at `route_focus_key`.
        // GTK keeps no "the sidebar band holds the keyboard" latch of its own,
        // so it passes `Engine::sidebar_has_focus()` — the disjunction of the
        // very flags the resolver's arms test, making that gate a no-op here
        // and preserving GTK's per-flag behaviour exactly.
        let focus_route = {
            let engine = self.engine.borrow();
            let band = engine.sidebar_has_focus();
            render::route_focus_key(&engine, band)
        };

        if focus_route == render::FocusKeyRoute::ActivityBar {
            self.handle_activity_bar_key(&key_name, ctrl);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared terminal (PTY) keyboard rung (#758 / #734 slice 3) ──
        // GTK had no terminal rung at all between #540 and this change: the
        // block that forwarded keys to the PTY lived in the Relm4 `view!`'s
        // `EventControllerKey` closure and was deleted with it, so every key
        // typed into a focused terminal fell through to `Engine::handle_key`
        // and ran vim commands on the *editor buffer* (#471). It sits here,
        // directly below the focus owners and above the debug F-keys, so the
        // ladder matches TUI's exactly: `route_focus_key` already returns
        // `FocusKeyRoute::None` while the terminal holds focus, and a focused
        // terminal must take F5/F9/F10/F11 to the PTY rather than the
        // debugger — which is what `vim`/`htop` running inside it expect.
        if render::route_terminal_key(
            &mut self.engine.borrow_mut(),
            &key_name,
            unicode,
            ctrl,
            shift,
            alt,
        ) {
            self.sync_plus_register_to_clipboard();
            self.draw_needed.set(true);
            return;
        }

        // Debug F-keys must reach the engine regardless of which panel
        // has focus — F5 (continue), F9 (breakpoint), F10 (step over),
        // F11 (step in) are global debugger commands.
        if !ctrl && !alt {
            match key_name.as_str() {
                "F5" | "F9" | "F10" | "F11" => {
                    let mapped = map_gtk_key_name(&key_name);
                    let action = self.engine.borrow_mut().handle_key(mapped, None, false);
                    self.dispatch_engine_action(action, false);
                    self.draw_needed.set(true);
                    return;
                }
                _ => {}
            }
        }

        // GTK focus on sidebar DrawingAreas is unreliable (grab_focus does not
        // stick), so the engine focus flags the resolver read above are the
        // only truth — the same approach the TUI backend uses. Each arm yields
        // `Some(panel_still_focused)` for the shared epilogue below, replacing
        // seven verbatim copies of it. The `if engine.dialog.is_some()`
        // patch-up the Debug and AI arms used to open with is gone: it
        // re-stated the dialog rung `render::route_modal_key` now resolves at
        // the top of this function, so it was unreachable.
        let panel_still_focused: Option<bool> = match focus_route {
            render::FocusKeyRoute::ExtPanel => {
                let mut engine = self.engine.borrow_mut();
                let mapped = map_gtk_key_name(key_name.as_str());
                if engine.ext_panel_input_active {
                    engine.handle_ext_panel_input_key(mapped, false, unicode);
                } else {
                    engine.handle_ext_panel_key(mapped, false, unicode);
                }
                // h/Left moves focus to the activity bar; other exits go to the editor.
                let outcome = engine.ext_panel_has_focus && engine.dialog.is_none();
                drop(engine);
                self.sync_plus_register_to_clipboard();
                Some(outcome)
            }
            render::FocusKeyRoute::ExtSidebar => {
                let mut engine = self.engine.borrow_mut();
                let mapped = map_gtk_key_name(key_name.as_str());
                engine.dispatch_ext_sidebar_key_unified(mapped, unicode);
                Some(engine.ext_sidebar_has_focus && engine.dialog.is_none())
            }
            render::FocusKeyRoute::Settings => {
                let mut engine = self.engine.borrow_mut();
                let mapped = map_gtk_key_name(key_name.as_str());
                engine.handle_settings_key(mapped, ctrl, unicode);
                Some(engine.settings_has_focus && engine.dialog.is_none())
            }
            render::FocusKeyRoute::Search => {
                let mut engine = self.engine.borrow_mut();
                let mapped = map_gtk_key_name(key_name.as_str());
                // Ctrl+V no longer reaches here: quadraui's runner intercepts
                // it and delivers `UiEvent::ClipboardPaste` straight to
                // `ShellApp::handle`, which routes through
                // `Engine::route_paste` (covering the search/replace fields)
                // before any key event is dispatched (#593).
                engine.dispatch_search_sidebar_key_unified(mapped, ctrl, alt, unicode);
                Some(engine.search_has_focus)
            }
            render::FocusKeyRoute::SourceControl => {
                let mut engine = self.engine.borrow_mut();
                let (mapped, sc_unicode) = map_gtk_key_with_unicode(key_name.as_str());
                engine.dispatch_sc_sidebar_key_unified(mapped, ctrl, sc_unicode);
                Some(engine.sc_has_focus)
            }
            render::FocusKeyRoute::Debug => {
                let mut engine = self.engine.borrow_mut();
                let mapped = map_gtk_key_name(key_name.as_str());
                let rect = engine.dap_sidebar_body_rect.get();
                render::populate_dap_sidebar_system(&engine);
                let consumed = if let Some(ui_event) = gtk_key_name_to_quadraui(mapped, ctrl) {
                    let backend_rc = self.backend.clone();
                    let sidebar_event = engine.dap_sidebar_system.borrow_mut().handle(
                        &ui_event,
                        &mut *backend_rc.borrow_mut(),
                        rect,
                    );
                    engine.dispatch_dap_sidebar_event(sidebar_event)
                } else {
                    false
                };
                if !consumed {
                    engine.dispatch_dap_sidebar_action_key(mapped);
                }
                Some(engine.dap_sidebar_has_focus)
            }
            render::FocusKeyRoute::Ai => {
                let mut engine = self.engine.borrow_mut();
                engine.handle_ai_panel_key(&key_name, ctrl, unicode);
                Some(engine.ai_has_focus)
            }
            render::FocusKeyRoute::Explorer => {
                // Explorer keys used to be routed through a per-DrawingArea
                // key controller when the DA had focus (#732 retired the
                // `Msg` variant it sent; nothing has produced it since #540).
                let key_mapped = map_gtk_key_name(key_name.as_str()).to_string();
                self.handle_explorer_da_key(key_mapped, unicode, ctrl);
                self.draw_needed.set(true);
                return;
            }
            render::FocusKeyRoute::ActivityBar | render::FocusKeyRoute::None => None,
        };
        if let Some(still_focused) = panel_still_focused {
            self.focus_after_sidebar_key(still_focused);
            self.draw_needed.set(true);
            return;
        }

        // ── Shared Alt-modifier / VSCode-mode rung (#759 / #734 slice 4) ──
        // GTK had no Alt tier at all: this method took `alt` and used it only
        // to feed the terminal router and to suppress the debug F-keys, so
        // Alt+Left/Right, Alt+M, Alt+,/., Alt+]/[ and every VSCode-mode
        // `Alt_*` chord fell through to `Engine::handle_key` — which has no
        // `alt` parameter — and were silently dropped. Placed directly below
        // the focus owners and above `Engine::handle_key`, the same slot TUI's
        // deleted block occupied. See the rung's header comment in
        // `render.rs` for the full divergence list.
        // Bound to a local first, for the same reason the modal rung at the
        // top of this method is: a `RefCell::borrow_mut()` temporary in a
        // `match` scrutinee lives for the whole `match`.
        let alt_outcome = render::route_alt_key(
            &mut self.engine.borrow_mut(),
            &key_name,
            unicode,
            shift,
            alt,
        );
        match alt_outcome {
            render::AltKeyOutcome::ResizeSidebar(delta) => {
                let current = ctx.shell().sidebar_width().round().max(0.0) as u16;
                let next = render::alt_resized_sidebar_width(current, delta);
                ctx.shell_mut().set_sidebar_width(next as f32);
                self.draw_needed.set(true);
                return;
            }
            render::AltKeyOutcome::Handled => {
                self.draw_needed.set(true);
                return;
            }
            render::AltKeyOutcome::Fallthrough => {}
        }

        // Hover popup copy: intercept y/Ctrl-C when hover is focused so the
        // engine's clipboard_write callback is invoked with the hover selection.
        {
            let engine = self.engine.borrow();
            let is_hover_copy = engine.editor_hover_has_focus
                && (key_name == "y" || key_name == "Y" || (ctrl && key_name == "c"));
            if is_hover_copy {
                if let Some(text) = engine.hover_selection_text() {
                    if let Some(ref cb) = engine.clipboard_write {
                        let _ = cb(text.as_str());
                    }
                    drop(engine);
                    let mut engine = self.engine.borrow_mut();
                    engine.message = "Hover text copied".to_string();
                    self.draw_needed.set(true);
                    return;
                }
            }
        }

        let action = {
            let mut engine = self.engine.borrow_mut();
            let a = engine.handle_key(&key_name, unicode, ctrl);
            // After any key press in insert mode, reset the AI completion
            // debounce timer so a new suggestion fires after idle.
            if engine.mode == crate::core::Mode::Insert && engine.settings.ai_completions {
                engine.ai_completion_reset_timer();
            }
            a
        };

        self.dispatch_engine_action(action, false);
        self.draw_needed.set(true);

        // Process macro playback queue if active
        loop {
            let (has_more, action) = {
                let mut engine = self.engine.borrow_mut();
                engine.advance_macro_playback()
            };

            self.dispatch_engine_action(action, true);

            if !has_more {
                break;
            }
        }

        // Ctrl-W h/l overflow: show sidebar and focus the active panel.
        {
            let overflow = self.engine.borrow_mut().window_nav_overflow.take();
            if let Some(false) = overflow {
                let current = self.current_active_panel_id();
                let panel_id = if is_ext_panel_id(&current) {
                    PANEL_EXPLORER.to_string()
                } else {
                    current
                };
                self.engine.borrow_mut().focus_sidebar_panel(&panel_id);
                self.sync_sidebar_from_engine();
            }
        }

        // Sync the unnamed register to the system clipboard if it changed.
        // The comparison is O(1); actual write is deferred to the background thread.
        self.sync_plus_register_to_clipboard();

        // If a yank just happened, schedule a 200 ms one-shot to clear the highlight.
        if self.engine.borrow().yank_highlight.is_some() {
            let q = self.deferred.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                q.send(DeferredAction::ClearYankHighlight);
            });
        }

        self.draw_needed.set(true);
    }

    fn handle_poll_tick(&mut self) {
        // Reload CSS if the colorscheme changed (e.g. via :colorscheme command).
        {
            let current = self.engine.borrow().settings.colorscheme.clone();
            if current != self.last_colorscheme {
                let theme = Theme::from_name(&current);
                let combined = format!("{STATIC_CSS}\n{}", make_theme_css(&theme));
                if let Some(p) = &self.css_provider {
                    p.load_from_data(&combined);
                }
                // Update GTK dark/light preference for native widgets & menus.
                if let Some(gtk_settings) = gtk4::Settings::default() {
                    gtk_settings.set_gtk_application_prefer_dark_theme(!theme.is_light());
                }
                self.last_colorscheme = current;
                self.draw_needed.set(true);
            }
        }

        // #731: a ~135-line block used to live here polling
        // `self.mouse_pos_cell` at 20Hz for four distinct hover features —
        // h-scrollbar hover, tab-close (×) hover + tab tooltip, debug
        // toolbar button hover, and LSP hover-on-dwell popups
        // (`Engine::editor_hover_mouse_move`). All four were gated on a
        // `da_size` derived from `self.drawing_area`, permanently `None`
        // under the ShellApp runner (nothing assigns it) — so none of the
        // four have worked since the #540 cutover, and nothing else in
        // this file writes `h_sb_hovered`/`tab_close_hover`/
        // `debug_button_hovered`/calls `editor_hover_mouse_move`. This is
        // the single biggest confirmed-dead surface this issue found (see
        // the PR description) — restoring it needs a live, correctly
        // absolute-coordinate DA size (the removed code used a `(0, 0)`
        // origin the neighboring comment already flagged as the #582/#646
        // coordinate-frame bug, so it was not simply "wire the same code
        // back up"), which is follow-up work, not a dead-code deletion.
        //
        // Sync per-window viewport dimensions from the paint-time ScreenLayout
        // so ensure_cursor_visible uses exact geometry.  This block is outside
        // the `da_size` guard because `cached_screen_layout` is populated by
        // render_content() regardless of whether `self.drawing_area` is set —
        // which it is not under the quadraui ShellApp runner (the runner owns
        // the single DrawingArea, not vimcode).
        {
            let layout_ref = self.cached_screen_layout.borrow();
            if let Some(ref layout) = *layout_ref {
                let mut engine = self.engine.borrow_mut();
                for rw in &layout.windows {
                    engine.set_viewport_for_window(
                        rw.window_id,
                        rw.lines.len().max(1),
                        rw.text_viewport_cols.max(1),
                    );
                }
            }
        }

        // Run all periodic background work (LSP, DAP, terminal, search, etc.)
        // poll_idle() consumes dap_wants_sidebar internally.
        let idle_dirty = self.engine.borrow_mut().poll_idle();
        if idle_dirty {
            self.sync_sidebar_from_engine();
        }
        // Format-on-save + :wq/:x deferred quit
        if self.engine.borrow().format_save_quit_ready {
            self.engine.borrow_mut().format_save_quit_ready = false;
            self.quit_confirmed();
        }
        // Run pending terminal commands (needs backend-supplied terminal size).
        if self.engine.borrow().pending_terminal_command.is_some() {
            let cmd = self
                .engine
                .borrow_mut()
                .pending_terminal_command
                .take()
                .unwrap();
            self.run_command_in_terminal(cmd);
        }
        let active_panel = self.current_active_panel_id();
        // Explorer refresh after confirmed file move.
        if self.engine.borrow().explorer_needs_refresh {
            self.engine.borrow_mut().explorer_needs_refresh = false;
            self.refresh_file_tree();
        }
        // Auto-refresh SC panel periodically (gated on sidebar visibility).
        if self.current_sidebar_visible()
            && (active_panel == PANEL_GIT || active_panel == PANEL_EXPLORER)
            && self.last_sc_refresh.elapsed() >= std::time::Duration::from_secs(2)
        {
            self.engine.borrow_mut().sc_refresh_async();
            self.last_sc_refresh = std::time::Instant::now();
        }
        if self.engine.borrow_mut().poll_sc_refresh() {
            self.draw_needed.set(true);
        }
        // Check for panel reveal request from plugins.
        // Extract into a separate binding so the RefMut drops before the
        // re-borrows inside the body (Rust 2021 temporary lifetime rule).
        let pending_panel = self.engine.borrow_mut().ext_panel_focus_pending.take();
        if let Some(panel_name) = pending_panel {
            {
                let mut engine = self.engine.borrow_mut();
                if !engine.app_shell.sidebar_visible() {
                    engine.app_shell.toggle_sidebar();
                }
                engine.ext_panel_has_focus = true;
                engine.ext_panel_active = Some(panel_name);
            }
            self.sync_sidebar_widgets();
        }
        // Sync the OS window title with the active buffer name (taskbar/pager).
        let win_title = self
            .engine
            .borrow()
            .active_buffer_name()
            .map(|n| format!("VimCode \u{2014} {}", n))
            .unwrap_or_else(|| "VimCode".to_string());
        if let Some(ref w) = self.window {
            w.set_title(Some(&win_title));
        }
    }

    /// Map a pixel x-offset within the editor hover popup's content
    /// area to a character column on `content_line`, using Pango to
    /// measure proportional UI-font widths (#218). The legacy code
    /// did `(rel_x / cached_char_width)` which drifts as the column
    /// index grows because UI_FONT is proportional. Heading rows
    /// (font scale > 1.0) need the scale applied to the layout so
    /// `xy_to_index` returns the right position.
    ///
    /// #731: the Pango-measured path below was gated on
    /// `self.drawing_area`, permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has always taken the approximate
    /// `rel_x / char-width`-style fallback in practice — see `terminal_cols`
    /// for the same "no live font-metrics source without a widget handle"
    /// root cause.
    /// Run the shared editor-hover-popup rung (#755) against this frame's
    /// painted geometry and apply whatever it decides.
    ///
    /// Returns `true` when the press belonged to the popup and must not fall
    /// through to the editor. Called from `handle_mouse_click_msg` **above**
    /// the scroll-surface dispatch — this backend used to run its bespoke
    /// copy ~90 lines *below* it, which is why a click aimed at the popup's
    /// own scrollbar was swallowed by the surface painted behind it
    /// (#229/#486) — and from `handle_mouse_double_click_msg`, which never
    /// consulted the popup at all, so a double-click on it fell through to
    /// the editor's word-select (#490).
    fn route_and_apply_editor_hover_popup(&self, x: f64, y: f64) -> bool {
        let (visible, has_focus) = {
            let engine = self.engine.borrow();
            (engine.editor_hover.is_some(), engine.editor_hover_has_focus)
        };
        let links: Vec<(quadraui::Rect, String)> = self
            .editor_hover_link_rects
            .borrow()
            .iter()
            .map(|(lx, ly, lw, lh, uri)| {
                (
                    quadraui::Rect::new(*lx as f32, *ly as f32, *lw as f32, *lh as f32),
                    uri.clone(),
                )
            })
            .collect();
        let route = render::route_editor_hover_popup_click(
            visible,
            &render::EditorHoverPopupState {
                popup: self.editor_hover_popup_rect.get().map(|(px, py, pw, ph)| {
                    quadraui::Rect::new(px as f32, py as f32, pw as f32, ph as f32)
                }),
                links: &links,
                scrollbar: self.editor_hover_scrollbar.get(),
                has_focus,
                // `draw_editor_hover_popup` insets its text by 4px on both
                // axes; the content grid is the editor's own cell size.
                content: render::PopupContentMetrics {
                    pad_x: 4.0,
                    pad_y: 4.0,
                    col_width: self.cached_char_width.max(1.0) as f32,
                    line_height: self.cached_line_height.max(1.0) as f32,
                },
            },
            x,
            y,
        );
        let effect = render::apply_editor_hover_popup_route(&mut self.engine.borrow_mut(), route);
        if let Some(url) = effect.open_url {
            open_url(&url);
        }
        if let Some(target) = effect.begin_drag {
            let drag_rc = self.backend.borrow().drag_state_handle();
            drag_rc.borrow_mut().begin(target);
            // Seek immediately, with the same thumb-aware math the drag
            // frames will use. This backend used to run a *second*,
            // ratio-based calculation at press time, so the thumb jumped
            // once on press and again on the first drag frame.
            let drag = drag_rc.borrow().clone();
            for ev in quadraui::dispatch_mouse_drag(
                &drag,
                quadraui::Point {
                    x: x as f32,
                    y: y as f32,
                },
                Default::default(),
            ) {
                if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                    if widget.as_str() == "editor_hover" {
                        self.engine.borrow_mut().editor_hover_set_scroll(new_offset);
                    }
                }
            }
        }
        self.draw_needed.set(true);
        effect.consumed
    }

    /// Popup-content column under `rel_x` (pixels from the content origin).
    ///
    /// Used by the hover-selection *drag* follow-through; the press itself
    /// goes through `render::route_editor_hover_popup_click`, which divides by
    /// the same `col_width`. Before #755 this returned `rel_x as usize` — a
    /// column per *pixel* — so a drag-selection inside the popup ran off the
    /// end of the line on the first few pixels of travel and never agreed
    /// with the column the press had chosen.
    fn pixel_to_editor_hover_col(&self, rel_x: f64, _content_line: usize) -> usize {
        (rel_x.max(0.0) / self.cached_char_width.max(1.0)) as usize
    }

    /// Push or pop the editor hover popup on the modal stack so
    /// click dispatch can decide modal-vs-base for both left- and
    /// right-clicks (#216). The popup is registered whenever it's
    /// visible (focused or not) so right-clicks anywhere inside it
    /// stop falling through to the editor's context menu. Picker-
    /// style reconcile: `push` dedupes on id, so calling this every
    /// click is safe.
    fn reconcile_editor_hover_modal(&self) {
        let editor_hover_id = quadraui::WidgetId::new("editor_hover");
        let engine = self.engine.borrow();
        let visible = engine.editor_hover.is_some();
        let rect = self.editor_hover_popup_rect.get();
        drop(engine);
        let stack_rc = self.backend.borrow().modal_stack_handle();
        let mut stack = stack_rc.borrow_mut();
        match (visible, rect) {
            (true, Some((px, py, pw, ph))) => {
                stack.push(
                    editor_hover_id,
                    quadraui::Rect {
                        x: px as f32,
                        y: py as f32,
                        width: pw as f32,
                        height: ph as f32,
                    },
                );
            }
            _ => {
                stack.pop(&editor_hover_id);
            }
        }
    }

    /// Route a left-click against the currently open engine-drawn context
    /// menu — `engine.context_menu` is shared by the editor, tab-bar, and
    /// explorer sources, so this applies uniformly regardless of which one
    /// opened it. Mirrors the modal-stack arbitration
    /// (`quadraui::dispatch_mouse_down` for outside-click dismissal,
    /// `ContextMenuLayout::hit_test` for inner row resolution) that used to
    /// live inline in `handle_mouse_click_msg` (Phase B.5b Stage 4).
    ///
    /// Returns `true` iff a menu was open and this call consumed the click
    /// (dismissed it, fired an item, or kept it open on an inert row) — the
    /// caller should treat that as "handled, stop routing". Returns `false`
    /// when no menu was open, after defensively popping any stale
    /// modal-stack entry left by an Esc/Enter close the click handler never
    /// saw; the caller should then proceed with its own routing.
    ///
    /// Callable from both `handle_mouse_click_msg` (main-content clicks) and
    /// `try_route_sidebar_mouse_event` (#546 FAILED-2: an explorer-sourced
    /// menu typically renders inside the sidebar's own content bounds, so
    /// without giving it priority there too, clicks on it fell straight
    /// through to `TreeController`'s row hit-test underneath instead of
    /// firing the menu action or dismissing it).
    fn dispatch_context_menu_click(&mut self, x: f64, y: f64) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        if self.engine.borrow().context_menu.is_none() {
            // Defensive cleanup: the menu may have closed via Esc/Enter while
            // no click was seen by us. Pop any stale entry.
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&cm_id);
            return false;
        }

        // Keep the menu's painted bounds on the modal stack so any other modal
        // that might be open (picker, dialog) is arbitrated against it by the
        // *drag* guard, which still consults the stack.
        if let Some(bounds) = self.context_menu_layout.borrow().as_ref().map(|l| l.bounds) {
            self.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .push(cm_id, bounds);
        }

        match self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress) {
            render::ModalOverlayRoute::ContextMenu(route) => {
                self.apply_context_menu_route(route);
            }
            // A dialog or a toast outranks the menu; the shared router already
            // said so, and re-deciding that here is what let the two backends
            // drift in the first place.
            _ => self.draw_needed.set(true),
        }
        true
    }

    /// Editor content bounds + tab-bar height **as last painted**, in the
    /// absolute DA coordinate frame mouse events arrive in (#582).
    ///
    /// Divider hit-testing must run against the geometry the renderer used, not
    /// a parallel re-derivation: `render_content` anchors `editor_bounds` at
    /// `AppShellLayout::main_content_bounds` (offset right by the activity
    /// bar/sidebar, down by the title-bar band), so any handler that rebuilt
    /// bounds at `(0.0, 0.0)` hit-tested a phantom divider displaced by that
    /// offset — the `:vsplit` failure in #582.
    ///
    /// `None` only before the first frame has been painted, when there is no
    /// divider on screen to hit anyway.
    fn painted_editor_bounds(&self) -> Option<(core::WindowRect, f64)> {
        self.cached_editor_bounds.get()
    }

    /// Left edge of the bottom panel as last painted — the same `x`
    /// `render_content` hands `draw_tab_bar` / the terminal pane, i.e. the
    /// editor's left edge, right of the activity bar and sidebar.
    ///
    /// [`render::BottomPanelMetrics::panel_left`] (#754). Falls back to `0.0`
    /// before the first frame, when there is no panel on screen to click.
    fn painted_bottom_panel_left(&self) -> f64 {
        self.cached_editor_bounds
            .get()
            .map(|(r, _)| r.x)
            .unwrap_or(0.0)
    }

    /// Both divider lists for the frame just painted, plus whether `(x, y)`
    /// lands on a group's tab bar — everything
    /// [`render::route_divider_grab`] needs from this backend.
    ///
    /// Derived from [`Self::painted_editor_bounds`] rather than a fresh
    /// drawing-area measurement, for the #582 reason recorded there.
    /// #753 named it because the click arm and the drag arm both needed it and
    /// each used to re-derive its own half.
    ///
    /// `on_tab_bar` exists so a click on a group's tab bar reaches the tab
    /// handlers instead of arming a group-divider drag; it is deliberately
    /// GTK-only (see `render::DividerState::on_tab_bar`). It is also skipped
    /// entirely in single-group mode, where `group_dividers` is empty and
    /// nothing could match anyway.
    fn painted_divider_geometry(
        &self,
        x: f64,
        y: f64,
    ) -> Option<(
        Vec<core::window::GroupDivider>,
        Vec<core::window::WindowDivider>,
        bool,
    )> {
        let (content_bounds, tab_bar_h) = self.painted_editor_bounds()?;
        let engine = self.engine.borrow();
        let single = engine.group_layout.is_single_group();
        let group_dividers = if single {
            Vec::new()
        } else {
            engine.group_layout.dividers(content_bounds, &mut 0)
        };
        let on_tab_bar = !single
            && engine
                .group_layout
                .calculate_group_rects(content_bounds, tab_bar_h)
                .iter()
                .any(|(gid, grect)| {
                    if engine.is_tab_bar_hidden(*gid) {
                        return false;
                    }
                    let ty = grect.y - tab_bar_h;
                    y >= ty && y < ty + tab_bar_h && x >= grect.x && x < grect.x + grect.width
                });
        let (window_rects, _) = engine.calculate_group_window_rects(content_bounds, tab_bar_h);
        let window_dividers = engine.calculate_window_dividers(&window_rects);
        Some((group_dividers, window_dividers, on_tab_bar))
    }

    /// Re-resolve a tab-drag press point to `(group, tab index)`, or `None`
    /// when the press was not on a tab after all.
    ///
    /// GTK arms the drag for the whole tab-bar band (its tab geometry is
    /// proportional-font pixel bounds, resolved by `pixel_to_click_target`, not
    /// the exact cell hit TUI gets for free), so the confirmation that
    /// `render::TabDragMove::Crossed` asks for is a real second hit-test here.
    fn tab_drag_source_at(&self, x: f64, y: f64) -> Option<(core::window::GroupId, usize)> {
        let layout_ref = self.cached_screen_layout.borrow();
        let layout = layout_ref.as_ref()?;
        let mut engine = self.engine.borrow_mut();
        let target = pixel_to_click_target(
            &mut engine,
            &self.backend,
            x,
            y,
            self.cached_line_height,
            self.cached_char_width,
            layout,
            &self.cached_tab_pixel_hits.borrow(),
            &self.tab_slot_positions.borrow(),
            &self.diff_btn_map.borrow(),
            &self.split_btn_map.borrow(),
            &self.action_btn_map.borrow(),
            self.cached_frame_hit_map.borrow().as_ref(),
            &self.cached_tab_bar_zones.borrow(),
            true, // resolving the original tab-bar mouse-down; switching tabs is intended
        );
        if !matches!(target, ClickTarget::TabBar) {
            return None;
        }
        // The tab was already switched by `pixel_to_click_target`, so the
        // active group + active tab *is* the drag source.
        let gid = engine.active_group;
        let tidx = engine
            .editor_groups
            .get(&gid)
            .map(|g| g.active_tab)
            .unwrap_or(0);
        Some((gid, tidx))
    }

    /// Resolve the modal-overlay rung (#733) for one point/action against
    /// the layouts the last frame actually painted
    /// (`dialog_layout`, `tab_switcher_popup_rect`, `completion_layout`,
    /// `Engine::toast_layout`), never freshly recomputed ones (#582/#646).
    ///
    /// Shared by every mouse-button path that needs to know whether a
    /// modal overlay owns the event — left-click dispatch
    /// (`handle_mouse_click_msg`, `ModalMouseAction::LeftPress`) and
    /// right-click dispatch (the `MouseButton::Right` arm of `handle`,
    /// `ModalMouseAction::Other`) both call this rather than re-deriving
    /// the state. TUI's `handle_mouse` already funnels every mouse event
    /// through one call to `render::route_modal_overlay_click`; this is
    /// GTK's equivalent single call site.
    fn route_modal_overlay(
        &self,
        x: f64,
        y: f64,
        action: render::ModalMouseAction,
    ) -> render::ModalOverlayRoute {
        let engine_ref = self.engine.borrow();
        let toast = engine_ref.toast_layout.borrow().clone();
        let dialog = self.dialog_layout.borrow().clone();
        let completion = self.completion_layout.borrow().clone();
        let context_menu = self.context_menu_layout.borrow().clone();
        let tab_switcher_bounds = self.tab_switcher_popup_rect.get().map(|(px, py, pw, ph)| {
            quadraui::Rect::new(px as f32, py as f32, pw as f32, ph as f32)
        });
        let lh = self.painted_line_height() as f32;
        // Both geometries come from what the last frame PAINTED — the picker's
        // own published rect, and the `FindReplacePanel` the frame was built
        // from — never a re-derivation off the drawing-area size (#555/#582).
        let picker = self.picker_popup_rect.get().map(|(px, py, pw, ph)| {
            render::PickerHitGeometry::new(
                quadraui::Rect::new(px as f32, py as f32, pw as f32, ph as f32),
                lh,
                engine_ref.picker_preview.is_some(),
                &render::gtk_picker_rows(lh),
                &engine_ref,
            )
        });
        let screen_ref = self.cached_screen_layout.borrow();
        let find_replace = screen_ref
            .as_ref()
            .and_then(|s| s.find_replace.as_ref())
            .map(|panel| {
                render::FindReplaceHitGeometry::from_panel(
                    panel,
                    (self.painted_char_width() as f32, lh),
                    &render::GTK_FIND_REPLACE_ANCHOR,
                )
            });

        render::route_modal_overlay_click(
            &render::ModalOverlayState {
                toast: toast.as_ref(),
                dialog_open: engine_ref.dialog.is_some(),
                dialog: dialog.as_ref(),
                context_menu_open: engine_ref.context_menu.is_some(),
                context_menu: context_menu.as_ref(),
                // The GTK rasteriser strokes the menu's border *inside*
                // `ContextMenuLayout::bounds`, so there is no frame outside it.
                context_menu_border: 0.0,
                tab_switcher_open: engine_ref.tab_switcher_open,
                tab_switcher_bounds,
                completion_open: engine_ref.completion_idx.is_some(),
                completion: completion.as_ref(),
                picker_open: engine_ref.picker_open,
                picker,
                find_replace_open: engine_ref.find_replace_open,
                find_replace,
            },
            x as f32,
            y as f32,
            action,
        )
    }

    /// Apply a unified-picker verdict from
    /// [`render::route_modal_overlay_click`].
    ///
    /// The verdict itself — which result row, thumb vs. track, inside vs.
    /// outside — is `render::PickerHitGeometry`'s, shared with TUI's
    /// `handle_mouse`. Before #751 each backend resolved it from its own
    /// re-derivation of the popup geometry, and the two had already drifted:
    /// GTK jumped the offset proportionally on a track click and grabbed the
    /// thumb at zero, TUI paged the track and grabbed with an offset, and
    /// clicking an already-selected row confirmed it on TUI but did nothing on
    /// GTK.
    fn apply_picker_route(&mut self, route: render::PickerRoute) {
        let picker_id = quadraui::WidgetId::new("picker");
        let Some((px, py, pw, ph)) = self.picker_popup_rect.get() else {
            return;
        };
        let lh = self.painted_line_height() as f32;
        let geo = {
            let engine = self.engine.borrow();
            render::PickerHitGeometry::new(
                quadraui::Rect::new(px as f32, py as f32, pw as f32, ph as f32),
                lh,
                engine.picker_preview.is_some(),
                &render::gtk_picker_rows(lh),
                &engine,
            )
        };
        // Keep the stack in step: the drag guard in `handle_mouse_drag_msg`
        // consults it to stop a gesture leaking to the editor behind the modal
        // (#192).
        self.backend
            .borrow()
            .modal_stack_handle()
            .borrow_mut()
            .push(picker_id.clone(), geo.bounds);

        match route {
            render::PickerRoute::Row(idx) => {
                render::apply_picker_row_click(&mut self.engine.borrow_mut(), idx);
            }
            render::PickerRoute::ScrollbarThumb { grab_offset } => {
                self.backend
                    .borrow()
                    .drag_state_handle()
                    .borrow_mut()
                    .begin(geo.drag_target(picker_id, grab_offset));
            }
            render::PickerRoute::ScrollbarTrack { toward_end } => {
                render::apply_picker_scroll_offset(
                    &mut self.engine.borrow_mut(),
                    geo.paged_offset(toward_end),
                    geo.visible_rows,
                );
            }
            render::PickerRoute::Consume => {}
            render::PickerRoute::Dismiss => {
                self.engine.borrow_mut().close_picker();
                self.backend
                    .borrow()
                    .modal_stack_handle()
                    .borrow_mut()
                    .pop(&picker_id);
            }
        }
    }

    /// Apply a context-menu verdict from [`render::route_modal_overlay_click`].
    ///
    /// Returns `true` when the event was consumed. The route itself — which
    /// item, hover vs. click, dismiss vs. keep-open — is decided once in
    /// `render.rs` and shared with TUI's `handle_mouse`; what stays here is
    /// GTK's own plumbing (modal-stack bookkeeping, file-tree refresh).
    fn apply_context_menu_route(&mut self, route: render::ContextMenuRoute) -> bool {
        let cm_id = quadraui::WidgetId::new("context_menu");
        let pop_stack = |app: &Self| {
            app.backend
                .borrow()
                .modal_stack_handle()
                .borrow_mut()
                .pop(&cm_id);
        };
        match route {
            render::ContextMenuRoute::Item(idx) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(ref mut cm) = engine.context_menu {
                    cm.selected = idx;
                }
                let _act = engine.context_menu_confirm();
                let needs_tree_refresh = engine.explorer_needs_refresh;
                if needs_tree_refresh {
                    engine.explorer_needs_refresh = false;
                }
                drop(engine);
                pop_stack(self);
                if needs_tree_refresh {
                    self.refresh_file_tree();
                }
            }
            render::ContextMenuRoute::Hover(idx) => {
                let mut engine = self.engine.borrow_mut();
                if let Some(ref mut cm) = engine.context_menu {
                    if cm.selected == idx {
                        return true;
                    }
                    cm.selected = idx;
                }
            }
            render::ContextMenuRoute::Consume => {}
            render::ContextMenuRoute::Dismiss => {
                self.engine.borrow_mut().close_context_menu();
                pop_stack(self);
            }
            render::ContextMenuRoute::Fallthrough => return false,
        }
        self.draw_needed.set(true);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_mouse_click_msg(&mut self, x: f64, y: f64, width: f64, height: f64, alt: bool) {
        self.reconcile_editor_hover_modal();

        // ── Modal-overlay rung (#733) ─────────────────────────────────────
        //
        // Toast → dialog → tab switcher → completion, sequenced ONCE in
        // `render::route_modal_overlay_click` and shared verbatim with
        // TUI's `handle_mouse`. This backend used to hand-roll the order
        // (toast, then tab switcher, then completion, with the dialog
        // ~600 lines further down, *below* find/replace) while TUI ran a
        // different one — the precedence drift #733 exists to kill.
        let modal_route = self.route_modal_overlay(x, y, render::ModalMouseAction::LeftPress);
        match modal_route {
            render::ModalOverlayRoute::Toast(hit) => {
                if self.engine.borrow_mut().handle_toast_hit(hit) {
                    self.draw_needed.set(true);
                    return;
                }
            }
            render::ModalOverlayRoute::Dialog(hit) => {
                match hit {
                    quadraui::DialogHit::Button(id) => {
                        if let Some(idx) = dialog_btn_index(&id) {
                            let action = self.engine.borrow_mut().dialog_click_button(idx);
                            self.apply_dialog_action(action);
                        }
                    }
                    quadraui::DialogHit::Outside => {
                        let mut engine = self.engine.borrow_mut();
                        engine.dialog = None;
                        engine.pending_move = None;
                    }
                    quadraui::DialogHit::Body | quadraui::DialogHit::BodyToolbarButton(_) => {}
                }
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::ContextMenu(route) => {
                if self.apply_context_menu_route(route) {
                    return;
                }
            }
            render::ModalOverlayRoute::TabSwitcher { inside } => {
                // Click anywhere dismisses; inside also consumes so the
                // editor underneath doesn't take a cursor move through it.
                self.engine.borrow_mut().tab_switcher_open = false;
                self.draw_needed.set(true);
                if inside {
                    return;
                }
            }
            render::ModalOverlayRoute::Completion(hit) => {
                let consumed = self.engine.borrow_mut().handle_completion_click(hit);
                self.draw_needed.set(true);
                if consumed {
                    return;
                }
            }
            render::ModalOverlayRoute::UnifiedPicker(hit) => {
                self.apply_picker_route(hit);
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::FindReplace(hit) => {
                if let render::FindReplaceRoute::Target { target, is_input } = hit {
                    if is_input {
                        self.fr_input_dragging = true;
                    }
                    self.engine.borrow_mut().handle_find_replace_click(target);
                }
                self.draw_needed.set(true);
                return;
            }
            render::ModalOverlayRoute::Swallow => return,
            render::ModalOverlayRoute::None => {}
        }

        // ── Editor hover popup rung (#755) ────────────────────────────────
        //
        // Link click, scrollbar grab, focus-or-select and dismiss-on-outside,
        // sequenced ONCE in `render::route_editor_hover_popup_click` and
        // shared verbatim with TUI's `handle_mouse`. The ~100 lines this
        // replaced sat *below* the scroll-surface dispatch, so a press aimed
        // at the popup's own scrollbar was consumed by the surface painted
        // behind it (#229/#486). It runs above that dispatch now — where TUI
        // always had it — because the popup paints on top of the editor.
        if self.route_and_apply_editor_hover_popup(x, y) {
            return;
        }

        // ── Scroll-surface click dispatch (scrollbar thumb-drag + track-page). ──
        {
            let surfaces = self.engine.borrow().scroll_surfaces.borrow().clone();
            let modal = self.backend.borrow().modal_stack_handle().borrow().clone();
            let mut drag = self.backend.borrow().drag_state_handle().borrow().clone();
            let click_events = quadraui::dispatch_click(
                &modal,
                &surfaces,
                &[],
                &mut drag,
                quadraui::Point {
                    x: x as f32,
                    y: y as f32,
                },
                quadraui::MouseButton::Left,
                Default::default(),
            );
            *self.backend.borrow().drag_state_handle().borrow_mut() = drag;
            for cev in &click_events {
                match cev {
                    quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset }
                        if widget.as_str() == "debug_output" =>
                    {
                        let mut engine = self.engine.borrow_mut();
                        engine.debug_output_scroll = *new_offset;
                        engine.debug_output_auto_scroll = false;
                        drop(engine);
                        self.draw_needed.set(true);
                        return;
                    }
                    quadraui::UiEvent::MouseDown {
                        widget: Some(id), ..
                    } if id.as_str() == "debug_output" => {
                        return;
                    }
                    quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset }
                        if widget.as_str() == "terminal_scrollback" =>
                    {
                        if let Some(term) = self.engine.borrow_mut().active_terminal_mut() {
                            term.set_scroll_offset(*new_offset);
                        }
                        self.draw_needed.set(true);
                        return;
                    }
                    _ => {}
                }
            }
        }

        // #751: the context-menu, find/replace and unified-picker rungs that
        // used to be transcribed here — ~370 lines — are now decided by
        // `render::route_modal_overlay_click` at the top of this handler and
        // applied by `apply_context_menu_route` / `apply_picker_route`. The
        // shared router also fixed their order: this backend arbitrated the
        // context menu *below* find/replace and the picker, while
        // `render::OVERLAY_Z_ORDER` paints it above both.

        // ── Chrome rung (#752) ────────────────────────────────────────────
        //
        // Breadcrumbs → status bands → global status bar, sequenced ONCE in
        // `render::route_chrome_click` and shared verbatim with TUI's
        // `handle_mouse`. What used to live here was the breadcrumb arm, and
        // ~60 lines further down a git-branch hit test that re-derived
        // `build_status_line`'s formatting by hand and measured it in UTF-8
        // bytes against a character column. Both are gone.
        if self.route_and_apply_chrome_click(x, y, render::ChromeMouseAction::LeftPress) {
            return;
        }

        // Debug toolbar click: resolve via cached ToolbarLayout on engine (#510).
        {
            let dbg_y = self.debug_toolbar_y_offset.get();
            let dbg_h = self.debug_toolbar_height.get();
            if dbg_h > 0.0 && y >= dbg_y && y < dbg_y + dbg_h {
                let idx = self.engine.borrow().debug_button_hit(x as f32, y as f32);
                self.engine.borrow_mut().debug_button_pressed = idx;
                self.draw_needed.set(true);
                if let Some(i) = idx {
                    if let Some(btn) = render::DEBUG_BUTTONS.get(i) {
                        let _ = self.engine.borrow_mut().execute_command(btn.action);
                        return;
                    }
                }
                return;
            }
        }

        // #733: the dialog rung moved to the shared modal-overlay router
        // at the top of this handler (`render::route_modal_overlay_click`),
        // which TUI's `handle_mouse` calls too. Control only reaches here
        // when no dialog is open, so what used to be the `else` arm of the
        // dialog block is now unconditional. The `ModalStack` push/pop dance
        // that arm maintained is gone with it: `DialogHit::Outside` already
        // answers the inside/outside question the stack round-trip was
        // recomputing.
        {
            // #752: the git-branch hit test that used to open this block —
            // ~60 lines re-deriving `build_status_line`'s own formatting, then
            // comparing a `cached_char_width`-derived column against a UTF-8
            // *byte* range — is now the global-status-bar rung of
            // `render::route_chrome_click`, called at the top of this handler.
            //
            // Clicking in the editor clears every sidebar's keyboard focus.
            // Without this, focus stays on whichever sidebar grabbed it last
            // (Source Control, Extensions, Settings, AI, DAP, …) and the
            // editor key handler keeps routing keys to that sidebar's
            // handler — so the editor "can't be interacted with" until the
            // user explicitly Escapes out of the sidebar. The DAP-only
            // version of this clear was incomplete; tracked all fields via
            // `clear_sidebar_focus()` instead.
            self.engine.borrow_mut().clear_sidebar_focus();
            // ── Bottom panel (tab strip / toolbar / terminal content) — #754 ──
            // Zone, split hit-test and pane-cell translation are all
            // `render::route_bottom_panel_click`, shared verbatim with TUI's
            // `handle_mouse`. What this replaced computed the pane column as a
            // bare `x / cached_char_width` against a *window-absolute* `x`,
            // while `render_content` paints the panel at the editor's left
            // edge — so with the sidebar open every terminal click landed
            // roughly `(activity_bar + sidebar) / char_width` columns right of
            // the glyph aimed at. `panel_left` is now a required input.
            let route = render::route_bottom_panel_click(
                &self.engine.borrow(),
                x,
                y,
                render::BottomPanelMetrics {
                    panel_left: self.painted_bottom_panel_left(),
                    col_width: self.cached_char_width.max(1.0),
                },
            );
            if let Some(route) = route {
                if !matches!(route, render::BottomPanelRoute::TabBar) {
                    self.terminal_resize_dragging = false;
                }
                let ctx = crate::core::engine::UiEventContext {
                    terminal_cols: self.terminal_cols(),
                    terminal_max_rows: self.terminal_target_maximize_rows(),
                };
                let effect =
                    render::apply_bottom_panel_route(&mut self.engine.borrow_mut(), route, x, ctx);
                self.terminal_split_dragging |= effect.split_drag;
                self.terminal_resize_dragging |= effect.resize_drag;
                if effect.relayout {
                    self.handle_resize();
                    return;
                }
                self.draw_needed.set(true);
            } else {
                {
                    let mut engine = self.engine.borrow_mut();
                    // Clicking outside the terminal panel returns focus to the editor.
                    engine.terminal_has_focus = false;
                }

                // Dropdown clicks are fully handled by the menu_dropdown_da overlay
                // widget (which has can_target=true while a menu is open).
                // If we reach here, no menu is open and we proceed with normal handling.

                // ── H scrollbar hit-test (before editor click) ────────────────
                // If the click lands on a Cairo h scrollbar:
                //   - on the thumb → start a DragTarget::ScrollbarX drag.
                //   - on the empty track → page-jump toward the click.
                // Either way, consume the click.
                {
                    let lh = self.cached_line_height;
                    let cw = self.cached_char_width;
                    let engine = self.engine.borrow();
                    let rects = compute_editor_window_rects(&engine, width, height, lh);
                    if let Some((win_id, scroll_left)) =
                        h_scrollbar_hit_test(&engine, x, y, &rects, cw, lh)
                    {
                        let win_rect = rects.iter().find(|(id, _)| *id == win_id).map(|(_, r)| *r);
                        let geom = win_rect
                            .and_then(|rect| h_scrollbar_geometry(&engine, win_id, &rect, cw, lh));
                        drop(engine);
                        if let Some((
                            track_x,
                            _ty,
                            track_w,
                            _sb_h,
                            thumb_x,
                            thumb_w,
                            scroll_range,
                            _,
                        )) = geom
                        {
                            let max_scroll = scroll_range.round() as usize;
                            let page_cols = (track_w / cw).floor() as usize;
                            if x < thumb_x {
                                let mut engine = self.engine.borrow_mut();
                                let new_left = scroll_left.saturating_sub(page_cols);
                                engine.set_scroll_left_for_window(win_id, new_left);
                                self.draw_needed.set(true);
                                return;
                            } else if x >= thumb_x + thumb_w {
                                let mut engine = self.engine.borrow_mut();
                                let new_left = (scroll_left + page_cols).min(max_scroll);
                                engine.set_scroll_left_for_window(win_id, new_left);
                                self.draw_needed.set(true);
                                return;
                            }
                            let grab_offset = (x - thumb_x) as f32;
                            let drag_rc = self.backend.borrow().drag_state_handle();
                            drag_rc
                                .borrow_mut()
                                .begin(quadraui::DragTarget::ScrollbarX {
                                    widget: quadraui::WidgetId::new(format!(
                                        "editor:h_sb:{}",
                                        win_id.0
                                    )),
                                    track_start: track_x as f32,
                                    track_length: track_w as f32,
                                    thumb_length: thumb_w as f32,
                                    max_scroll,
                                    grab_offset,
                                    inverted: false,
                                });
                            self.h_sb_drag_cell.set(Some(win_id));
                            self.draw_needed.set(true);
                            return;
                        }
                    }
                }

                // ── Divider hit-test (#753 shared rung) ───────────────────────
                // Editor-group boundaries then `:split`/`:vsplit` boundaries
                // (#582), sequenced by `render::route_divider_grab`. GTK's only
                // contribution is its own painted geometry and its own grab
                // margin — a symmetric 6px around the thin drawn line, against
                // continuous positions (`quantize: false`); TUI's cell metrics
                // differ, the ordering does not.
                if let Some((group_dividers, window_dividers, on_tab_bar)) =
                    self.painted_divider_geometry(x, y)
                {
                    if let Some(grab) = render::route_divider_grab(
                        &render::DividerState {
                            group_dividers: &group_dividers,
                            window_dividers: &window_dividers,
                            metrics: render::GTK_DIVIDER_METRICS,
                            on_tab_bar,
                        },
                        x,
                        y,
                    ) {
                        self.divider_grab = Some(grab);
                        return;
                    }
                }

                {
                    let mut engine = self.engine.borrow_mut();

                    if engine.is_vscode_mode() {
                        engine.vscode_clear_selection();
                    }
                    let (click_result, engine_action) = {
                        let layout_ref = self.cached_screen_layout.borrow();
                        if let Some(ref layout) = *layout_ref {
                            handle_mouse_click(
                                &mut engine,
                                &self.backend,
                                x,
                                y,
                                alt,
                                self.cached_line_height,
                                self.cached_char_width,
                                layout,
                                &self.cached_tab_pixel_hits.borrow(),
                                &self.tab_slot_positions.borrow(),
                                &self.diff_btn_map.borrow(),
                                &self.split_btn_map.borrow(),
                                &self.action_btn_map.borrow(),
                                self.cached_frame_hit_map.borrow().as_ref(),
                                &self.cached_tab_bar_zones.borrow(),
                            )
                        } else {
                            (None, None)
                        }
                    };
                    match engine_action {
                        Some(core::engine::EngineAction::ToggleSidebar) => {
                            drop(engine);
                            self.sync_sidebar_from_engine();
                            return;
                        }
                        Some(core::engine::EngineAction::OpenTerminal) => {
                            // Create the terminal tab immediately (not via
                            // the deferred `DeferredAction::ToggleTerminal`)
                            // so the panel appears on this same draw cycle.
                            let cols = self.terminal_cols();
                            let rows = engine.session.terminal_panel_rows;
                            engine.terminal_new_tab(cols, rows);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                        _ => {}
                    }
                    match click_result {
                        Some(true) => {
                            drop(engine);
                            self.show_close_tab_confirm();
                            self.draw_needed.set(true);
                            return;
                        }
                        Some(false) => {
                            // Buffer click — fire hooks and reveal file
                        }
                        None => {
                            // Engine-drawn action menu is already opened with the
                            // correct anchor by click.rs::handle_mouse_click. The
                            // engine-drawn renderer at draw.rs:906 + click dispatch
                            // at line ~6022 take over from here (#395).
                            if engine.context_menu.as_ref().is_some_and(|cm| {
                                matches!(
                                    cm.target,
                                    core::engine::ContextMenuTarget::EditorActionMenu { .. }
                                )
                            }) {
                                drop(engine);
                                self.draw_needed.set(true);
                                return;
                            }
                            // Tab bar / split button click — skip hooks.
                            // Record drag start position for tab drag-and-drop.
                            self.tab_drag.arm(x, y);
                            drop(engine);
                            self.draw_needed.set(true);
                            return;
                        }
                    }

                    // Fire cursor_move hook so plugins (e.g. git-insights blame)
                    // see the new cursor position after a mouse click.
                    engine.fire_cursor_move_hook();
                    drop(engine);
                    self.draw_needed.set(true);
                }
            }
        } // close else (dialog not open)
    }

    /// Line height the last frame actually painted with, falling back to the
    /// `setup()`-seeded `cached_line_height` before the first frame.
    ///
    /// Every click hit-test that measures *painted* geometry must use this
    /// rather than `cached_line_height` (#555) — see the note where
    /// `render_content` publishes it.
    fn painted_line_height(&self) -> f64 {
        self.painted_line_height
            .get()
            .unwrap_or(self.cached_line_height)
            .max(1.0)
    }

    /// Assemble this backend's [`render::ChromeState`] from the geometry the
    /// last frame actually painted, run the shared chrome rung over it, and
    /// apply whatever it decides. Returns `true` when the event was consumed.
    ///
    /// Every rect fed in here is a *painted* one — `status_segment_map` and
    /// `separated_status_bar_rect` are filled by `render_content` from the
    /// same `Surface::StatusBar` rects it draws, `global_status_rect` likewise
    /// (#752), and the breadcrumb bars carry their own draw-time layout. That
    /// is the #555 rule: never hit-test against freshly recomputed geometry.
    fn route_and_apply_chrome_click(
        &mut self,
        x: f64,
        y: f64,
        action: render::ChromeMouseAction,
    ) -> bool {
        let lh = self.painted_line_height();

        let layout_ref = self.cached_screen_layout.borrow();
        let Some(ref screen) = *layout_ref else {
            return false;
        };
        let engine = self.engine.borrow();
        let segment_map = self.status_segment_map.borrow();

        // The separated status line is listed first: it is painted in its own
        // full-width band *outside* every window's rect, so it can never be
        // reached through the per-window bars' geometry, and a click in that
        // band must not fall through to whatever sits underneath it.
        let mut bands: Vec<render::StatusBand<'_>> = Vec::new();
        if let Some(rect) = self.separated_status_bar_rect.get() {
            if let Some(zones) = segment_map.get(&screen.active_window_id.0) {
                bands.push(render::StatusBand { rect, zones });
            }
        }
        for rw in &screen.windows {
            if rw.status_line.is_none() || rw.rect.height <= lh {
                continue;
            }
            let Some(zones) = segment_map.get(&rw.window_id.0) else {
                continue;
            };
            // The status line occupies the window's bottom row — the same
            // `rect.height - lh` `render_content` subtracts before painting it.
            bands.push(render::StatusBand {
                rect: quadraui::Rect::new(
                    rw.rect.x as f32,
                    (rw.rect.y + rw.rect.height - lh) as f32,
                    rw.rect.width as f32,
                    lh as f32,
                ),
                zones,
            });
        }

        // The global bar last, spatially and in arbitration: it is the bottom
        // band of the shell, below every window.
        let global_rect = engine.global_status_rect.get();
        let global_zones;
        if global_rect.width > 0.0 && global_rect.height > 0.0 {
            global_zones = self.global_status_zones.borrow().clone();
            bands.push(render::StatusBand {
                rect: global_rect,
                zones: &global_zones,
            });
        }

        // The same shared hit test, with the same tolerances, the window-split
        // divider rung in `handle_mouse_click_msg` runs — see
        // `render::ChromeState::on_window_divider` (#582/#752).
        let on_window_divider =
            self.painted_editor_bounds()
                .is_some_and(|(content_bounds, tab_bar_h)| {
                    let (window_rects, _) =
                        engine.calculate_group_window_rects(content_bounds, tab_bar_h);
                    render::divider_hit_test(
                        &engine.calculate_window_dividers(&window_rects),
                        x,
                        y,
                        (6.0, 6.0),
                        (6.0, 6.0),
                        false,
                    )
                    .is_some()
                });

        let route = render::route_chrome_click(
            &render::ChromeState {
                breadcrumbs_enabled: engine.settings.breadcrumbs,
                breadcrumbs: &screen.breadcrumbs,
                line_height: lh,
                status_bands: &bands,
                on_window_divider,
            },
            action,
            x,
            y,
        );

        drop(segment_map);
        drop(engine);
        drop(layout_ref);

        match route {
            render::ChromeRoute::None => return false,
            render::ChromeRoute::Breadcrumb { group_id, idx } => {
                self.engine
                    .borrow_mut()
                    .handle_breadcrumb_click(group_id, idx);
            }
            render::ChromeRoute::StatusAction(action) => {
                let cols = self.terminal_cols();
                let follow_up =
                    render::apply_status_action(&mut self.engine.borrow_mut(), &action, cols);
                if matches!(
                    follow_up,
                    Some(crate::core::engine::EngineAction::ToggleSidebar)
                ) {
                    self.sync_sidebar_from_engine();
                }
            }
            render::ChromeRoute::BreadcrumbBar | render::ChromeRoute::StatusBar => {}
        }
        self.draw_needed.set(true);
        true
    }

    /// Character-cell advance the last frame actually painted with — the
    /// horizontal twin of [`Self::painted_line_height`]. See the field's doc
    /// for why `cached_char_width` is the wrong number at click time (#751).
    fn painted_char_width(&self) -> f64 {
        self.painted_char_width
            .get()
            .unwrap_or(self.cached_char_width)
            .max(1.0)
    }

    /// Compute the picker popup's bounds in DA-local pixels. Shared by
    /// the click handler (to push into the modal stack) and the drag
    /// guard (to decide if a drag started inside the popup).
    ///
    /// Prefers the rect the last frame **actually painted**
    /// (`picker_popup_rect`), and only re-derives from `width`/`height` when
    /// no frame has painted the picker yet.
    ///
    /// #555: re-deriving was wrong on two counts, and together they put the
    /// hit rect in a different place than the pixels. `render_content` centres
    /// the popup in `backend.viewport()` (the whole window) at
    /// `gtk_picker_sizing(line_height)`, whereas both callers here pass the
    /// `width`/`height` of `ctx.layout.main_content_bounds` — the editor area
    /// only, minus activity bar / sidebar / title bar — anchored at `(0, 0)`,
    /// and a `line_h: 1.0, header_h: 0.0` sizing. So with any shell chrome
    /// present the modal rect pushed onto the `ModalStack` was both offset and
    /// differently sized from the visible popup: clicks on the painted
    /// dropdown either missed the modal entirely or resolved to the wrong
    /// result row. That is what made the breadcrumb dropdown look inert once
    /// it finally started painting.
    fn compute_picker_popup_bounds(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
        if let Some(rect) = self.picker_popup_rect.get() {
            return rect;
        }
        let engine = self.engine.borrow();
        let has_preview = engine.picker_preview.is_some();
        drop(engine);
        let sizing = render::PickerSizing {
            header_h: 0.0,
            line_h: 1.0,
            ..render::gtk_picker_sizing(1.0)
        };
        let geo =
            render::PickerGeometry::compute(width as f32, height as f32, has_preview, &sizing);
        (
            geo.popup_x as f64,
            geo.popup_y as f64,
            geo.popup_w as f64,
            geo.popup_h as f64,
        )
    }

    // ── Drag-follow-through rung (#756, mouse-ladder slice 6) ────────────────
    //
    // Which gesture owns a move-with-the-button-held is
    // `render::route_mouse_drag`, sequenced ONCE and shared verbatim with TUI's
    // `handle_mouse`. This backend used to state its own order here — armed
    // scrollbar → hover popup → modal swallow → tab drag → divider → split →
    // resize → terminal → editor — while TUI stated a different one, and each
    // knew scrollbar widget ids the other did not. See the rung's banner in
    // `render.rs`.
    fn handle_mouse_drag_msg(&mut self, x: f64, y: f64, width: f64, height: f64) {
        // Keep the picker's modal-stack entry fresh before anything hit-tests
        // the stack: the popup's size depends on `has_preview`, which can change
        // mid-picker.
        let picker_open = self.engine.borrow().picker_open;
        {
            let picker_id = quadraui::WidgetId::new("picker");
            let stack_rc = self.backend.borrow().modal_stack_handle();
            let mut stack = stack_rc.borrow_mut();
            if picker_open {
                let (px, py, pw, ph) = self.compute_picker_popup_bounds(width, height);
                stack.push(
                    picker_id,
                    quadraui::Rect {
                        x: px as f32,
                        y: py as f32,
                        width: pw as f32,
                        height: ph as f32,
                    },
                );
            } else {
                stack.pop(&picker_id);
            }
        }

        let bottom_metrics = render::BottomPanelMetrics {
            panel_left: self.painted_bottom_panel_left(),
            col_width: self.cached_char_width.max(1.0),
        };
        let drag_rc = self.backend.borrow().drag_state_handle();
        let stack_rc = self.backend.borrow().modal_stack_handle();
        let route = {
            let engine = self.engine.borrow();
            let layout_ref = self.cached_screen_layout.borrow();
            let state = render::MouseDragState {
                layout: layout_ref.as_ref(),
                armed_target: render::drag_state_arms_scrollbar(&drag_rc.borrow()),
                hover_popup_selecting: engine.editor_hover_has_focus
                    && engine
                        .editor_hover
                        .as_ref()
                        .is_some_and(|h| h.selection.is_some())
                    && self.editor_hover_popup_rect.get().is_some(),
                modal_hit: stack_rc
                    .borrow()
                    .hit_test(quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    })
                    .is_some(),
                // GTK has no canvas sidebar separator, command-line selection or
                // explorer drag-and-drop: the separator is a `gtk::Paned`, the
                // command line paints through `Surface::CommandLine` (which
                // exposes no character hit test — the quadraui gap #752
                // recorded), and the file tree is a native widget with its own
                // DnD. Stated here rather than omitted so the asymmetry is
                // visible at the call site.
                sidebar_resizing: false,
                sidebar_dnd: false,
                sidebar_body: None,
                command_line_selecting: false,
                tab_dragging: self.tab_drag.is_armed_or_dragging(),
                divider_grabbed: self.divider_grab.is_some(),
                terminal_split_dragging: self.terminal_split_dragging,
                terminal_panel_resizing: self.terminal_resize_dragging,
                // #756 review: mirrors TUI's guard — see the field's doc
                // comment in `render.rs`. GTK's `EditorText` arm doesn't run
                // through the shared `DragState`, but it drives the same
                // `Engine::mouse_drag`, so `mouse_drag_active` is just as
                // valid a "already extending" signal here.
                text_selection_active: engine.mouse_drag_active,
                in_terminal_content: render::in_terminal_pane_content(
                    &engine,
                    x,
                    y,
                    bottom_metrics,
                ),
                cell: (
                    self.cached_char_width.max(1.0),
                    self.cached_line_height.max(1.0),
                ),
            };
            render::route_mouse_drag(&state, x, y)
        };

        match route {
            render::MouseDragRoute::ArmedTarget => {
                let events = quadraui::dispatch_mouse_drag(
                    &drag_rc.borrow(),
                    quadraui::Point {
                        x: x as f32,
                        y: y as f32,
                    },
                    Default::default(),
                );
                let picker_visible_rows = if picker_open {
                    let lh = self.cached_line_height.max(1.0);
                    let has_preview = self.engine.borrow().picker_preview.is_some();
                    render::PickerGeometry::compute(
                        width as f32,
                        height as f32,
                        has_preview,
                        &render::gtk_picker_sizing(lh as f32),
                    )
                    .visible_rows
                } else {
                    0
                };
                for ev in &events {
                    if let quadraui::UiEvent::ScrollOffsetChanged { widget, new_offset } = ev {
                        // #756: the widget-id → scroll-state table is
                        // `render::apply_scroll_offset`, shared with TUI. The
                        // copy this replaced knew `picker` and `editor:h_sb:N`
                        // and nothing else — see the rung's banner in
                        // `render.rs`, point 2, for why two half-tables is a
                        // silent trap rather than a live bug.
                        render::apply_scroll_offset(
                            &mut self.engine.borrow_mut(),
                            widget.as_str(),
                            *new_offset,
                            render::ScrollApplyContext {
                                picker_visible_rows,
                            },
                        );
                    }
                }
            }
            render::MouseDragRoute::HoverPopupSelection => {
                if let Some((px, py, _pw, _ph)) = self.editor_hover_popup_rect.get() {
                    let padding = 4.0;
                    let lh = self.cached_line_height.max(1.0);
                    let scroll = self
                        .engine
                        .borrow()
                        .editor_hover
                        .as_ref()
                        .map(|h| h.scroll_top)
                        .unwrap_or(0);
                    let rel_x = x - px - padding;
                    let rel_y = y - py - padding;
                    let content_line = (rel_y / lh).max(0.0) as usize + scroll;
                    let content_col = self.pixel_to_editor_hover_col(rel_x, content_line);
                    self.engine
                        .borrow_mut()
                        .editor_hover_extend_selection(content_line, content_col);
                }
            }
            render::MouseDragRoute::TabDrag => {
                // `64.0` is the squared 8-device-pixel threshold.
                match self.tab_drag.handle_move(x, y, 64.0) {
                    render::TabDragMove::Tracking => {
                        // Cursor and the cached per-group bounds are both in
                        // absolute surface coordinates, so the hit-test matches
                        // what the overlay draws (#515).
                        let groups = self.cached_drop_groups.borrow();
                        let zone = render::compute_tab_drop_zone(
                            x as f32,
                            y as f32,
                            &groups,
                            self.cached_drop_tbh.get(),
                        );
                        drop(groups);
                        self.tab_drag.track(zone);
                    }
                    render::TabDragMove::Crossed { press_x, press_y } => {
                        // Unlike TUI, this backend's arm fires for the whole
                        // tab-bar band, so the press has to be re-resolved to
                        // confirm it was on a tab. If it was not, disarm and
                        // re-route the same event with the machine idle — the
                        // one rung that can decline after being asked.
                        if let Some(source) = self.tab_drag_source_at(press_x, press_y) {
                            self.tab_drag.begin(source, x, y);
                        } else {
                            self.tab_drag.disarm();
                            self.draw_needed.set(true);
                            self.handle_mouse_drag_msg(x, y, width, height);
                            return;
                        }
                    }
                    render::TabDragMove::Pending | render::TabDragMove::Idle => {}
                }
            }
            render::MouseDragRoute::Divider => {
                if let (Some(grab), Some((group_dividers, window_dividers, _))) =
                    (self.divider_grab, self.painted_divider_geometry(x, y))
                {
                    render::apply_divider_drag(
                        &mut self.engine.borrow_mut(),
                        grab,
                        &group_dividers,
                        &window_dividers,
                        x,
                        y,
                    );
                }
            }
            render::MouseDragRoute::TerminalSplitDivider => {
                if self.cached_char_width > 0.0 {
                    const SB_W: f64 = 6.0;
                    let min_x = self.cached_char_width * 5.0;
                    let max_x = (width - SB_W - self.cached_char_width * 5.0).max(min_x);
                    let clamped_x = x.clamp(min_x, max_x);
                    let left_cols = (clamped_x / self.cached_char_width) as u16;
                    self.engine
                        .borrow_mut()
                        .terminal_split_set_drag_cols(left_cols);
                }
            }
            render::MouseDragRoute::TerminalPanelResize => {
                if self.cached_line_height > 0.0 {
                    let global_status_rows = if self.engine.borrow().settings.window_status_line {
                        0.0
                    } else {
                        1.0
                    };
                    let status_h = (1.0 + global_status_rows) * self.cached_line_height;
                    let available = (height - y - status_h).max(0.0);
                    // Leave at least 4 editor lines visible (+ tab bar chrome)
                    let min_editor_lines = 4.0 + 1.0;
                    let max_rows =
                        ((height - status_h - min_editor_lines * self.cached_line_height)
                            / self.cached_line_height) as u16;
                    let max_rows = max_rows.saturating_sub(2).max(5);
                    let new_rows = ((available / self.cached_line_height) as u16)
                        .saturating_sub(2)
                        .clamp(5, max_rows);
                    self.engine.borrow_mut().session.terminal_panel_rows = new_rows;
                }
            }
            render::MouseDragRoute::Minimap => {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    render::apply_minimap_click(&mut engine, layout, x, y);
                }
            }
            render::MouseDragRoute::TerminalContent => {
                // #533: shared drag handler — tries forward_mouse(Move) when the
                // child has mouse reporting, falls back to local selection.
                render::apply_terminal_content_drag(
                    &mut self.engine.borrow_mut(),
                    x,
                    y,
                    bottom_metrics,
                );
            }
            render::MouseDragRoute::EditorText => {
                let layout_ref = self.cached_screen_layout.borrow();
                if let Some(ref layout) = *layout_ref {
                    let mut engine = self.engine.borrow_mut();
                    handle_mouse_drag(
                        &mut engine,
                        &self.backend,
                        x,
                        y,
                        self.cached_line_height,
                        self.cached_char_width,
                        layout,
                        &self.cached_tab_pixel_hits.borrow(),
                        &self.tab_slot_positions.borrow(),
                        &self.diff_btn_map.borrow(),
                        &self.split_btn_map.borrow(),
                        &self.action_btn_map.borrow(),
                        self.cached_frame_hit_map.borrow().as_ref(),
                        &self.cached_tab_bar_zones.borrow(),
                    );
                }
            }
            // #192: a drag inside an open modal with nothing armed is swallowed
            // so it cannot leak to the editor underneath.
            render::MouseDragRoute::ModalSwallow
            | render::MouseDragRoute::SidebarResize
            | render::MouseDragRoute::SidebarBody
            | render::MouseDragRoute::CommandLine
            | render::MouseDragRoute::None => {}
        }
        self.draw_needed.set(true);
    }

    fn handle_mouse_up_msg(&mut self) {
        // Clear debug toolbar pressed state (#510).
        if self.engine.borrow().debug_button_pressed.is_some() {
            self.engine.borrow_mut().debug_button_pressed = None;
            self.draw_needed.set(true);
        }

        // Phase B.4: clear any active cross-backend drag state. The
        // dispatcher returns a MouseUp event we could forward to the
        // engine later, but today no consumer cares about mouse-up
        // beyond clearing drag state.
        {
            let drag_rc = self.backend.borrow().drag_state_handle();
            let mut drag = drag_rc.borrow_mut();
            if drag.is_active() {
                let stack_rc = self.backend.borrow().modal_stack_handle();
                let stack = stack_rc.borrow();
                let _events = quadraui::dispatch_mouse_up(
                    &stack,
                    &mut drag,
                    quadraui::Point { x: 0.0, y: 0.0 },
                    quadraui::MouseButton::Left,
                );
            }
        }

        // Tab drag drop (#753 — the same `handle_release` TUI calls; it also
        // clears any armed-but-never-dragged press, which is what the bare
        // `tab_drag_start = None` this replaced was for).
        if self.tab_drag.handle_release(&mut self.engine.borrow_mut()) {
            self.draw_needed.set(true);
        }
        if self.terminal_split_dragging {
            self.terminal_split_dragging = false;
            if self.cached_char_width > 0.0 {
                let engine = self.engine.borrow();
                let left_cols = if engine.terminal_split_left_cols > 0 {
                    engine.terminal_split_left_cols
                } else if !engine.terminal_panes.is_empty() {
                    engine.terminal_panes[0].session.cols()
                } else {
                    0
                };
                let rows = engine.session.terminal_panel_rows;
                drop(engine);
                if left_cols > 0 {
                    // #731: was `if let Some(da) = self.drawing_area…`,
                    // permanently `None` under the ShellApp runner — see
                    // `terminal_cols`.
                    let da_w = 800.0;
                    const SB_W: f64 = 6.0;
                    let total_cols = ((da_w - SB_W) / self.cached_char_width) as u16;
                    let right_cols = total_cols.saturating_sub(left_cols);
                    self.engine
                        .borrow_mut()
                        .terminal_split_finalize_drag(left_cols, right_cols, rows);
                }
            }
        }
        if self.terminal_resize_dragging {
            self.terminal_resize_dragging = false;
            let rows = self.engine.borrow().session.terminal_panel_rows;
            // #731: was `if let Some(da) = self.drawing_area…`, permanently
            // `None` under the ShellApp runner — see `terminal_cols`.
            let cols = self.terminal_cols();
            self.engine.borrow_mut().terminal_resize(cols, rows);
            let _ = self.engine.borrow().session.save();
        }
        self.h_sb_drag_cell.set(None);
        self.divider_grab = None;
        {
            let mut engine = self.engine.borrow_mut();
            engine.mouse_drag_active = false;
            engine.mouse_drag_origin_window = None;
            // #533: auto-copy terminal selection on mouse-release, mirroring
            // TUI.  terminal_autocopy_selection() is a no-op when the
            // terminal isn't focused or has no selection.
            engine.terminal_autocopy_selection();
        }
        self.draw_needed.set(true);
    }

    /// Toggle the integrated terminal panel open/closed.
    fn toggle_terminal(&mut self) {
        let needs_new_tab = {
            let engine = self.engine.borrow();
            (!engine.terminal_open || !engine.terminal_has_focus)
                && engine.terminal_panes.is_empty()
        };
        if needs_new_tab {
            // Use the actual drawing area width so the PTY matches the visible panel.
            let cols = self.terminal_cols();
            let rows = self.engine.borrow().session.terminal_panel_rows;
            self.engine.borrow_mut().terminal_new_tab(cols, rows);
        } else {
            self.engine.borrow_mut().toggle_terminal();
        }
        self.draw_needed.set(true);
    }

    /// Toggle the "terminal maximized" state (panel fills editor area).
    fn toggle_terminal_maximize(&mut self) {
        // Phase B.2: route through engine's UiEvent dispatch — same
        // path as the keybinding above + the EngineAction handler
        // + the toolbar click handler.
        let ctx = crate::core::engine::UiEventContext {
            terminal_cols: self.terminal_cols(),
            terminal_max_rows: self.terminal_target_maximize_rows(),
        };
        self.engine.borrow_mut().handle_ui_event(
            crate::core::engine::UiEvent::Accelerator(
                crate::core::engine::AcceleratorId::new("terminal.toggle_maximize"),
                quadraui::Modifiers::default(),
            ),
            ctx,
        );
        self.draw_needed.set(true);
    }

    /// Open a new terminal tab rooted at `dir`.
    fn open_terminal_at(&mut self, dir: PathBuf) {
        let cols = self.terminal_cols();
        let rows = self.engine.borrow().session.terminal_panel_rows;
        self.engine
            .borrow_mut()
            .terminal_new_tab_at(cols, rows, Some(&dir));
        self.draw_needed.set(true);
    }

    /// Open a new terminal tab.
    fn new_terminal_tab(&mut self) {
        let cols = self.terminal_cols();
        let rows = self.engine.borrow().session.terminal_panel_rows;
        self.engine.borrow_mut().terminal_new_tab(cols, rows);
        self.draw_needed.set(true);
    }

    /// Run `cmd` in a visible terminal pane (used for extension installs).
    fn run_command_in_terminal(&mut self, cmd: String) {
        let cols = self.terminal_cols();
        let rows = self.engine.borrow().session.terminal_panel_rows;
        self.engine
            .borrow_mut()
            .terminal_run_command(&cmd, cols, rows);
        self.draw_needed.set(true);
    }

    /// #731: was `if let Some(ref da) = *self.menu_dropdown_da.borrow()`
    /// — that field is permanently `None` under the ShellApp runner
    /// (nothing assigns it), so this has been a no-op since #540. The menu
    /// bar is repainted every frame by `render_content` from engine state
    /// instead (see the `ActivityBarActivation::MenuToggled` comment).
    /// Kept as a named no-op so its two call sites stay self-documenting.
    fn sync_menu_overlay(&self) {}

    /// Dispatch a menu action by command string, as produced by
    /// `quadraui::MenuEvent::Activated`.
    fn handle_menu_action(&mut self, action: String) {
        match action.as_str() {
            "open_file_dialog" => {
                self.open_file_dialog();
            }
            "open_folder_dialog" => {
                self.open_folder_dialog();
            }
            "open_workspace_dialog" => {
                self.engine.borrow_mut().open_workspace_from_file();
                self.refresh_file_tree();
            }
            "save_workspace_as_dialog" => {
                self.save_workspace_as_dialog();
            }
            "openrecent" => {
                self.open_recent_dialog();
            }
            "find" => {
                self.engine.borrow_mut().open_find_replace();
                self.draw_needed.set(true);
            }
            "quit_menu" => {
                if self.engine.borrow().has_any_unsaved() {
                    self.show_quit_confirm();
                } else {
                    self.save_session_and_exit();
                }
            }
            _ => {
                let engine_action = self.engine.borrow_mut().dispatch_menu_action(&action);
                match engine_action {
                    EngineAction::Quit | EngineAction::SaveQuit => {
                        self.quit_confirmed();
                    }
                    EngineAction::QuitWithUnsaved => {
                        self.show_quit_confirm();
                    }
                    EngineAction::ToggleSidebar => {
                        self.sync_sidebar_from_engine();
                    }
                    EngineAction::OpenTerminal => {
                        self.new_terminal_tab();
                    }
                    _ => {}
                }
            }
        }
        self.sync_menu_overlay();
        self.draw_needed.set(true);
    }

    /// Effective sidebar visibility — reads directly from
    /// `engine.app_shell` (owned by quadraui per #385). Replaces the
    /// former `App.sidebar_visible` local cache so GTK and engine state
    /// can never drift.
    fn current_sidebar_visible(&self) -> bool {
        self.engine.borrow().app_shell.sidebar_visible()
    }

    /// Effective active panel id, accounting for ext-panel synthetic IDs.
    /// Extension panels bypass AppShell (no dynamic registration on the
    /// quadraui side), so if `engine.ext_panel_active` is set we synthesise
    /// `ext:{name}`; otherwise we read AppShell's active panel.
    fn current_active_panel_id(&self) -> String {
        let engine = self.engine.borrow();
        if let Some(ref name) = engine.ext_panel_active {
            return format!("ext:{name}");
        }
        engine
            .app_shell
            .active_panel_id()
            .map(|id| id.as_str().to_string())
            .unwrap_or_else(|| PANEL_EXPLORER.to_string())
    }

    /// Re-sync GTK widget tree from engine sidebar state. Was previously
    /// `sync_sidebar_from_engine` which copied into local cache fields;
    /// the cache is gone (engine.app_shell is the single source of truth)
    /// so this is now just a redraw trigger.
    fn sync_sidebar_from_engine(&mut self) {
        self.sync_sidebar_widgets();
    }

    /// Queue a redraw after sidebar visibility/focus state changes.
    ///
    /// Used to update GTK widget visibility (revealer + panel boxes) and
    /// grab focus on the active panel DA under the pre-#540 Relm4 widget
    /// tree. Under the ShellApp runner there is no such widget tree to
    /// sync — `render_content` repaints the whole sidebar from
    /// `engine.app_shell` every frame — so this is now just the redraw
    /// trigger (#731).
    fn sync_sidebar_widgets(&mut self) {
        self.draw_needed.set(true);
    }

    /// Toggle sidebar visibility.
    fn toggle_sidebar_panel(&mut self) {
        self.engine.borrow_mut().toggle_sidebar();
        self.sync_sidebar_from_engine();
    }

    /// Switch the sidebar to a different panel.
    ///
    /// #754: the ext-panel-vs-built-in bookkeeping this used to spell out is
    /// `render::apply_activity_panel_switch`, shared with TUI's activity-bar
    /// arm. The only thing left here is this backend's own widget re-sync,
    /// which differs by branch (a plugin panel does not move
    /// `app_shell.active_panel_id()`, so `sync_sidebar_from_engine` has nothing
    /// to sync for it).
    fn switch_panel(&mut self, panel_id: String) {
        let is_ext = panel_id.starts_with("ext:");
        render::apply_activity_panel_switch(&mut self.engine.borrow_mut(), &panel_id);
        if is_ext {
            self.sync_sidebar_widgets();
        } else {
            self.sync_sidebar_from_engine();
        }
    }

    /// Explorer CRUD action triggered by a keyboard shortcut or context menu.
    fn explorer_action(&mut self, action_str: String) {
        use crate::core::settings::ExplorerAction;
        let action = match action_str.as_str() {
            "new_file" => Some(ExplorerAction::NewFile),
            "new_folder" => Some(ExplorerAction::NewFolder),
            "rename" => Some(ExplorerAction::Rename),
            "delete" => Some(ExplorerAction::Delete),
            "move_file" => Some(ExplorerAction::MoveFile),
            _ => None,
        };
        if let Some(action) = action {
            self.engine.borrow_mut().dispatch_explorer_crud(action);
            self.queue_explorer_draw();
            self.draw_needed.set(true);
        }
    }

    /// Refresh the file tree from the current working directory.
    fn refresh_file_tree(&mut self) {
        self.refresh_explorer();
        if let Some(path) = self.engine.borrow().file_path().cloned() {
            self.reveal_path_in_explorer(&path);
        }
        self.draw_needed.set(true);
    }

    /// Toggle focus between the explorer and the editor.
    fn toggle_focus_explorer(&mut self) {
        if self.engine.borrow().explorer_has_focus {
            self.engine.borrow_mut().explorer_has_focus = false;
        } else {
            let mut engine = self.engine.borrow_mut();
            engine.ext_panel_active = None;
            engine.focus_sidebar_panel(PANEL_EXPLORER);
            drop(engine);
            self.sync_sidebar_widgets();
        }
        self.draw_needed.set(true);
    }

    /// Toggle focus between the search panel and the editor.
    fn toggle_focus_search(&mut self) {
        if self.current_active_panel_id() == PANEL_SEARCH && self.current_sidebar_visible() {
            // Just give the editor DA back keyboard focus.
        } else {
            let mut engine = self.engine.borrow_mut();
            engine.ext_panel_active = None;
            engine.focus_sidebar_panel(PANEL_SEARCH);
            drop(engine);
            self.sync_sidebar_widgets();
        }
        self.draw_needed.set(true);
    }

    /// `UiEvent` (scroll, mouse) over the explorer panel — routed through
    /// `TreeController::handle` for scrollbar interaction.
    /// Sidebar routing for the Explorer panel (#540/#754).
    ///
    /// The `TreeController` widget dispatch itself — populate, re-apply the
    /// paint-time metrics, `handle()`, resolve a `ContextMenuRequested` —
    /// is [`render::route_explorer_tree_event`], shared with TUI's
    /// `TuiShellApp::handle_mouse_event` explorer intercept. What stays here
    /// is GTK-only plumbing: which events this panel claims at all
    /// (`dominated`), pulling the metrics/backend/theme it needs to make the
    /// call, and its own draw-invalidation bookkeeping.
    fn explorer_ui_event(&mut self, ev: quadraui::UiEvent) {
        let dominated = matches!(
            ev,
            quadraui::UiEvent::MouseDown { .. }
                | quadraui::UiEvent::DoubleClick { .. }
                | quadraui::UiEvent::MouseUp { .. }
                | quadraui::UiEvent::Scroll { .. }
        ) || matches!(
            ev,
            quadraui::UiEvent::MouseMoved {
                buttons: quadraui::ButtonMask { left: true, .. },
                ..
            }
        );
        if !dominated {
            return;
        }
        let rect = self.engine.borrow().explorer_tree_rect.get();
        if rect.width <= 0.0 {
            return;
        }
        let theme = {
            let eng = self.engine.borrow();
            render::Theme::from_name(&eng.settings.colorscheme)
        };
        // Re-apply the metrics the tree was drawn with so the hit-test row
        // math matches the rendered rows (#540). `set_current_line_height`/
        // `set_current_char_width` are inherent on `GtkBackend`, not trait
        // methods on `dyn Backend`, so they must be set from here rather
        // than inside the shared function.
        let metrics = self.cached_explorer_metrics.get();
        let backend_rc = self.backend.clone();
        let mut b = backend_rc.borrow_mut();
        b.set_current_line_height(metrics.0);
        b.set_current_char_width(metrics.1);
        let tree_event = {
            let mut engine = self.engine.borrow_mut();
            render::route_explorer_tree_event(&mut engine, &ev, rect, metrics, &theme, &mut *b)
        };
        drop(b);

        // `None` means either the event was fully resolved inside
        // `route_explorer_tree_event` (a `ContextMenuRequested` — #546) or
        // the rect wasn't paintable; either way this panel already did
        // everything it needs to.
        let Some(tree_event) = tree_event else {
            self.queue_explorer_draw();
            self.draw_needed.set(true);
            return;
        };
        if matches!(ev, quadraui::UiEvent::DoubleClick { .. }) {
            self.engine
                .borrow_mut()
                .dispatch_explorer_tree_event(tree_event);
        } else if matches!(ev, quadraui::UiEvent::MouseDown { .. }) {
            self.engine
                .borrow_mut()
                .handle_explorer_mouse_event(tree_event);
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// Find the runner-created top-level window once it is mapped/visible.
    /// Returns `None` until then — see `capture_window_and_apply_csd`. (#552)
    fn find_visible_window() -> Option<gtk4::Window> {
        // `list_toplevels` asserts GTK is initialized, which it never is under
        // the headless test harness (#646). `run()` calls `gtk4::init()` before
        // building the `App`, so this is unconditionally `true` in a live run
        // and the guard costs production nothing.
        if !gtk4::is_initialized() {
            return None;
        }
        gtk4::Window::list_toplevels()
            .into_iter()
            .filter_map(|obj| obj.downcast::<gtk4::Window>().ok())
            .find(|w| w.is_visible())
    }

    /// Capture the runner's GTK window (if not already captured) and drop
    /// GTK's server-side WM titlebar in favour of the drawn CSD row from
    /// `render_content`. Called from both `setup()` (fast path, usually too
    /// early — the runner hasn't called `window.present()` yet) and `tick()`
    /// (reliable path — retried every frame until the window is mapped).
    /// (#552)
    fn capture_window_and_apply_csd(&mut self) {
        if self.window.is_some() {
            return;
        }
        if let Some(w) = Self::find_visible_window() {
            w.set_decorated(false);
            self.window = Some(w);
        }
    }

    /// Forward a pointer event over the sidebar content area to the active panel's
    /// controller. In ShellApp mode the sidebar has no dedicated per-panel
    /// `DrawingArea`, so events the Relm4 build delivered straight to each panel's
    /// DA must be routed here instead. Returns `true` when the event was
    /// consumed. (#540 ShellApp port, #544 non-explorer panels)
    ///
    /// The panel arms mirror `render_content`'s own `match active_id` — each one
    /// feeds the very controller (`TreeController` / `SidebarSystem` /
    /// `FormController`) that painted the panel, at the rect it painted into.
    /// That is the whole reason most arms are just a line or two of dispatch
    /// and carry no GTK-specific hit-test: the geometry already lives in the
    /// shared controller, exactly as `tui_main::shell_app`'s equivalent
    /// intercepts use it. A few panels (settings, extensions, debug/git via
    /// their helper functions below) need a bit more — focus bookkeeping or
    /// translating a press into a chrome band's local coordinate space — but
    /// none of them re-derive hit geometry the painter doesn't already own.
    ///
    /// # Drag / release follow-through
    ///
    /// A press claimed here sets `sidebar_pointer_captured`, and while that is
    /// set the subsequent `MouseMoved`(left held) / `MouseUp` are routed to the
    /// same panel so scrollbar thumbs and tree drags track the pointer. An
    /// *unclaimed* move/release is deliberately left alone, so an editor
    /// text-drag that happens to cross into the sidebar still finalizes through
    /// the editor's own mouse-up path.
    fn try_route_sidebar_mouse_event(
        &mut self,
        event: &quadraui::UiEvent,
        ctx: &quadraui::ShellContext<'_>,
    ) -> bool {
        use quadraui::UiEvent;

        let Some(sb) = ctx.layout.sidebar_content_bounds else {
            self.sidebar_pointer_captured.set(false);
            return false;
        };
        let dragging = self.sidebar_pointer_captured.get();
        let pos = match event {
            UiEvent::MouseDown { position, .. }
            | UiEvent::DoubleClick { position, .. }
            | UiEvent::Scroll { position, .. } => *position,
            // Follow-through only: never *start* an interaction from a move or
            // a release (see the doc comment above).
            UiEvent::MouseUp { position, .. } if dragging => {
                self.sidebar_pointer_captured.set(false);
                *position
            }
            UiEvent::MouseMoved {
                position,
                buttons: quadraui::ButtonMask { left: true, .. },
            } if dragging => *position,
            _ => return false,
        };
        // A captured drag keeps its grab even when the pointer leaves the
        // sidebar — otherwise dragging a scrollbar thumb sideways would silently
        // hand the rest of the gesture to the editor.
        let starts_interaction =
            !matches!(event, UiEvent::MouseUp { .. } | UiEvent::MouseMoved { .. });
        if starts_interaction
            && (pos.x < sb.x
                || pos.x >= sb.x + sb.width
                || pos.y < sb.y
                || pos.y >= sb.y + sb.height)
        {
            return false;
        }
        // Only a *press* moves keyboard focus into the panel. A wheel notch is
        // deliberately excluded: hovering-and-scrolling must not steal focus,
        // the same rule the editor's own wheel path follows (#240/#646).
        let is_press = matches!(
            event,
            UiEvent::MouseDown { .. } | UiEvent::DoubleClick { .. }
        );

        // An open picker / command palette is painted *over* the sidebar and
        // owns every press while it is up (#555). `render_content` centres the
        // popup on the whole window, so with the sidebar open its left half
        // sits on top of the explorer tree — and without this the tree's row
        // hit-test underneath ate those presses before they could reach
        // `handle_mouse_click_msg`'s picker block. The dropdown a breadcrumb
        // click opens therefore looked completely inert on its left half:
        // rows highlighted nothing, selection never moved.
        //
        // Falling through is also what makes *dismissal* correct: a press on
        // the sidebar while the picker is up reaches the picker's own
        // modal-stack dispatch, which resolves it as an outside-click and
        // closes the popup (rather than silently driving the tree beneath it).
        if self.engine.borrow().picker_open {
            return false;
        }

        // An engine-drawn context menu (editor / tab-bar / explorer — they
        // all share `engine.context_menu`) takes priority over the sidebar's
        // own click routing. An explorer-sourced menu typically renders
        // inside these same sidebar bounds, so without this a click on it —
        // an item, or an outside-click meant to dismiss it — fell straight
        // through to `TreeController`'s row hit-test underneath: the menu
        // *looked* interactive but every click acted on the tree row instead
        // (#546 FAILED-2). Only a left press drives the menu's own
        // hit-test/dismissal (mirrors `handle_mouse_click_msg`); any other
        // press/double-click/scroll while a menu is open is swallowed here
        // rather than leaking through to the tree underneath it.
        if self.engine.borrow().context_menu.is_some() {
            if matches!(
                event,
                UiEvent::MouseDown {
                    button: quadraui::MouseButton::Left,
                    ..
                }
            ) {
                self.dispatch_context_menu_click(pos.x as f64, pos.y as f64);
            }
            self.draw_needed.set(true);
            return true;
        }

        // Which panel owns the sidebar body? `render::sidebar_owner` states
        // that precedence once (#754) — `ext_panel_active` first, then
        // `app_shell.active_panel_id()`, Explorer as the fallback — so the
        // click router, the hover router and the painter can never disagree
        // about who is on screen. This used to be an inline `format!("ext:{}")`
        // here and an `if …is_some() / else if active_panel_is(…)` chain on
        // TUI.
        let owner = render::sidebar_owner(&self.engine.borrow());

        let consumed = match &owner {
            render::SidebarOwner::Explorer => {
                self.explorer_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Search => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.search_set_focus(true);
                }
                engine.handle_search_sidebar_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Debug => {
                self.route_debug_sidebar_event(event, pos, starts_interaction)
            }
            render::SidebarOwner::Git => {
                self.route_sc_sidebar_event(event, pos, starts_interaction)
            }
            render::SidebarOwner::Extensions => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_sidebar_has_focus = true;
                }
                engine.handle_ext_sidebar_ui_event(event.clone());
                if matches!(event, UiEvent::DoubleClick { .. }) {
                    engine.ext_open_selected_readme();
                }
                true
            }
            render::SidebarOwner::Settings => {
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.settings_has_focus = true;
                }
                // `handle_settings_form_ui_event`'s own `bool` return (whether
                // `FormController` recognized a row/field under the point) is
                // deliberately ignored here: the position is already confirmed
                // to be inside the sidebar's content bounds (checked above),
                // so even a click on empty panel padding belongs to this panel,
                // not the editor underneath it. Honoring `false` would let that
                // click fall through to `handle_mouse_click_msg` at sidebar-local
                // coordinates, which is exactly the leak every other arm in this
                // match also guards against by returning `true` unconditionally.
                render::handle_settings_form_ui_event(&mut engine, event, sb);
                true
            }
            render::SidebarOwner::ExtPanel(_) => {
                // Plugin-provided panel: `render_content` paints it through the
                // same `ext_sidebar_system` at the same rect, so it routes the
                // same way.
                let mut engine = self.engine.borrow_mut();
                if is_press {
                    engine.ext_sidebar_has_focus = true;
                }
                engine.handle_ext_sidebar_ui_event(event.clone());
                true
            }
            render::SidebarOwner::Ai => self.route_ai_sidebar_event(pos, starts_interaction),
            // Unknown panel id: nothing was painted, so there is nothing
            // for a click to hit — let it fall through rather than
            // swallow it.
            render::SidebarOwner::Unknown => false,
        };

        if consumed {
            if starts_interaction {
                self.sidebar_pointer_captured
                    .set(matches!(event, UiEvent::MouseDown { .. }));
            }
            self.draw_needed.set(true);
        }
        consumed
    }

    /// Sidebar routing for the Debug panel (#544/#754).
    ///
    /// `render_content` stacks two chrome rows above the body: a title bar and
    /// an action-button bar whose `StatusBarLayout` it stashes in
    /// `engine.dap_sidebar_action_hits`. Those hit regions are **bar-relative**
    /// (`StatusBar::layout` lays out from `0,0`; `quadraui::gtk::draw_status_bar`
    /// returns them verbatim), so the press has to be translated into the
    /// action row's own space before hit-testing
    /// ([`render::dap_sidebar_action_click_at`]). Everything below goes to
    /// the shared `SidebarSystem` at the body rect it painted into
    /// ([`render::dispatch_dap_sidebar_body_event`]) — the same two shared
    /// functions TUI calls for this panel.
    fn route_debug_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let action_rect = self.cached_dap_action_rect.get();
        let body_rect = self.engine.borrow().dap_sidebar_body_rect.get();
        if body_rect.width <= 0.0 {
            return false;
        }
        let mut engine = self.engine.borrow_mut();
        if starts_interaction {
            engine.dap_sidebar_has_focus = true;
        }
        // Chrome band (title + action row) — above the body rect.
        if starts_interaction && pos.y < body_rect.y {
            if let Some(ar) = action_rect {
                render::dap_sidebar_action_click_at(&mut engine, pos.x - ar.x, pos.y - ar.y);
            }
            // Claimed either way: the press landed on this panel's own chrome,
            // so it must not leak through to the editor beneath (#637's rule
            // for the TUI twin of this intercept).
            return true;
        }
        let backend_rc = self.backend.clone();
        render::dispatch_dap_sidebar_body_event(
            &mut engine,
            event,
            body_rect,
            &mut *backend_rc.borrow_mut(),
        );
        true
    }

    /// Sidebar routing for the git ("source control") panel (#544/#754).
    ///
    /// The panel is three stacked bands — header, commit-message input, and the
    /// toolbar slab + change sections. `render_content` derives them via
    /// `render::sc_sidebar_bands` and caches the result here, so this resolves a
    /// press against the exact geometry that was painted rather than
    /// re-deriving it (the pre-#544 handler assumed `DrawingArea`-local
    /// coordinates with the panel top at `y == 0`, which the ShellApp painter
    /// never produces). The dispatch itself is
    /// [`render::route_sc_sidebar_click`], shared with TUI.
    fn route_sc_sidebar_event(
        &mut self,
        event: &quadraui::UiEvent,
        pos: quadraui::Point,
        starts_interaction: bool,
    ) -> bool {
        let Some(bands) = self.cached_sc_bands.get() else {
            return false;
        };
        let mut engine = self.engine.borrow_mut();
        render::route_sc_sidebar_click(&mut engine, event, pos, &bands, starts_interaction);
        true
    }

    /// Sidebar routing for the AI panel (#544/#730/#754).
    ///
    /// `render_content` caches the header/messages/input bands in
    /// `cached_ai_bands` at paint time (`render::draw_ai_sidebar_panel`'s
    /// return value) — resolving a press against that means the click
    /// router can never derive a different layout than the one actually on
    /// screen (#544/#582/#646). The dispatch itself is
    /// [`render::route_ai_sidebar_click`] — TUI paints this same panel but
    /// has never cached its bands for click routing, so it does not call
    /// this yet; see that function's doc comment. Consumes the press
    /// unconditionally like every other panel arm in
    /// `try_route_sidebar_mouse_event` — a click on empty panel padding
    /// still belongs to this panel, not the editor underneath it.
    fn route_ai_sidebar_event(&mut self, pos: quadraui::Point, starts_interaction: bool) -> bool {
        let Some(bands) = self.cached_ai_bands.get() else {
            return false;
        };
        let mut engine = self.engine.borrow_mut();
        render::route_ai_sidebar_click(&mut engine, pos, &bands, starts_interaction);
        true
    }

    fn handle_explorer_da_key(&mut self, key_name: String, unicode: Option<char>, ctrl: bool) {
        // #734 slice 1: the #426 explorer-ctx-menu intercept and the
        // dialog patch-up ("route keys to the dialog handler, not the
        // explorer dispatch") that used to open this function are gone —
        // both were local re-statements of rungs `render::route_modal_key`
        // now resolves at the top of `handle_key_press`, above the
        // `explorer_has_focus` rung that is this function's only caller.

        // Panel-nav shortcuts before engine dispatch.
        let (pk_toggle, pk_explorer, pk_search) = {
            let eng = self.engine.borrow();
            (
                eng.settings.panel_keys.toggle_sidebar.clone(),
                eng.settings.panel_keys.focus_explorer.clone(),
                eng.settings.panel_keys.focus_search.clone(),
            )
        };
        let printable = match (ctrl, unicode) {
            (true, Some(c)) => format!("Ctrl-{}", c.to_ascii_uppercase()),
            (false, Some(c)) => c.to_string(),
            _ => key_name.clone(),
        };
        if printable == pk_toggle {
            self.toggle_sidebar_panel();
            return;
        }
        if printable == pk_explorer {
            self.toggle_focus_explorer();
            return;
        }
        if printable == pk_search {
            self.toggle_focus_search();
            return;
        }

        use crate::core::engine::ExplorerKeyResult;
        let result = self
            .engine
            .borrow_mut()
            .dispatch_explorer_key(&key_name, unicode, ctrl);

        match result {
            ExplorerKeyResult::Unfocused => {
                self.engine.borrow_mut().explorer_has_focus = false;
            }
            ExplorerKeyResult::FocusToolbar => {
                // engine.activity_bar_focus_in_at(1) was already called inside
                // dispatch_explorer_key. Redraw the activity bar for the
                // selection highlight; key events route through the editor DA
                // whose handle_key_press checks activity_bar_focused and
                // dispatches to handle_activity_bar_key. The activity bar DA
                // has no EventControllerKey, so grab_focus on it drops keys.
                self.engine.borrow_mut().explorer_has_focus = false;
            }
            _ => {}
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// #731: was a redraw hint on `self.explorer_sidebar_da_ref`, permanently
    /// `None` under the ShellApp runner. `render_content` repaints the whole
    /// sidebar from engine state every frame, so callers only need
    /// `self.draw_needed.set(true)` — kept as a named no-op (rather than
    /// touching every call site) so the intent at each call site stays
    /// legible.
    fn queue_explorer_draw(&self) {}

    /// After a sidebar panel processes a key, queue a redraw of the activity
    /// bar if the engine just set `activity_bar_focused`, and in all cases
    /// give GTK widget focus to the editor DA so its `handle_key_press` can
    /// route the next key via engine flags (`activity_bar_focused`,
    /// `ext_panel_has_focus`, …).
    ///
    /// Why the editor DA, not the activity bar DA?  The activity bar DA has
    /// no `EventControllerKey`; routing GTK focus there drops subsequent key
    /// events.  The editor DA's capture-phase controller checks engine focus
    /// flags and dispatches to `handle_activity_bar_key` when needed — the
    /// same engine-flag routing that the TUI backend uses.
    ///
    /// `fallback_focused` is the "panel still has focus" flag passed through
    /// to `focus_editor_if_needed` when neither activity-bar nor editor focus
    /// applies (i.e. the sidebar panel kept focus → don't steal it).
    fn focus_after_sidebar_key(&self, fallback_focused: bool) {
        if self.engine.borrow().activity_bar_focused {
            // Activity bar has logical focus — `render_content` repaints it
            // every frame from engine state, so there's no separate redraw
            // hint to give here. Key routing flows through the editor DA.
            self.focus_editor_if_needed(false);
        } else {
            self.focus_editor_if_needed(fallback_focused);
        }
    }

    /// Handle a key press while the activity bar has keyboard focus. The key
    /// table itself is shared (`render::activity_bar_key_action`); this is the
    /// GTK sink for the actions it names.
    fn handle_activity_bar_key(&mut self, key_name: &str, ctrl: bool) {
        use render::ActivityBarKeyAction;
        match render::activity_bar_key_action(map_gtk_key_name(key_name), ctrl) {
            ActivityBarKeyAction::MoveDown => self.engine.borrow_mut().activity_bar_move_down(),
            ActivityBarKeyAction::MoveUp => self.engine.borrow_mut().activity_bar_move_up(),
            ActivityBarKeyAction::Activate => {
                use crate::core::engine::sidebar::ActivityBarActivation;
                let activation = self.engine.borrow_mut().activity_bar_activate();
                match activation {
                    // The menu bar is repainted every frame by
                    // `render_content`'s `ShellApp` path (no dedicated overlay
                    // DA to invalidate under the #540 cutover).
                    ActivityBarActivation::MenuToggled => self.draw_needed.set(true),
                    ActivityBarActivation::PanelFocused
                    | ActivityBarActivation::ExtPanelFocused(_) => {
                        self.sync_sidebar_from_engine();
                    }
                    ActivityBarActivation::NoOp => {}
                }
            }
            ActivityBarKeyAction::FocusOut => self.engine.borrow_mut().activity_bar_focus_out(),
            ActivityBarKeyAction::Collapse => {
                let mut engine = self.engine.borrow_mut();
                engine.activity_bar_focus_out();
                engine.app_shell.hide_sidebar();
                engine.clear_sidebar_focus();
                engine.session.explorer_visible = false;
                let _ = engine.session.save();
            }
            ActivityBarKeyAction::Ignore => {}
        }
        // Suppress the default engine key handler — key is consumed.
    }

    /// #734 slice 1: the single GTK-side sink for the shared context-menu
    /// key rung (`render::ModalKeyRoute::ContextMenu`).
    ///
    /// Replaces two hand-rolled copies — the block that opened
    /// `handle_key_press` and `handle_explorer_ctx_menu_key` (#426) on the
    /// explorer DA path — both of which reimplemented selection movement
    /// inline instead of calling `Engine::handle_context_menu_key`, and so
    /// disagreed with TUI on `l` (confirm), `q`/`h` (close) and disabled-item
    /// skipping. The engine owns all of that now; the only GTK-specific part
    /// left is dispatching the confirmed action, since `new_file` /
    /// `open_terminal` / `find_in_folder` need backend plumbing.
    fn dispatch_context_menu_key(&mut self, key_name: &str, unicode: Option<char>) {
        let effective_key = if key_name.is_empty() {
            unicode.map(|c| c.to_string()).unwrap_or_default()
        } else {
            key_name.to_string()
        };
        let target = self.engine.borrow().context_menu_target_path();
        let action = {
            let mut engine = self.engine.borrow_mut();
            let (_consumed, action) = engine.handle_context_menu_key(&effective_key);
            action
        };
        if let (Some(ref act), Some((ref path, _is_dir))) = (action, target) {
            self.dispatch_explorer_ctx_action(act, path);
        }
        let needs_refresh = {
            let mut engine = self.engine.borrow_mut();
            let r = engine.explorer_needs_refresh;
            engine.explorer_needs_refresh = false;
            r
        };
        if needs_refresh {
            self.refresh_file_tree();
        }
        self.queue_explorer_draw();
        self.draw_needed.set(true);
    }

    /// #426: Map the action string returned by `context_menu_confirm` for
    /// an explorer ctx menu to the appropriate backend Msg. Engine-side
    /// actions (copy_path, reveal, select_for_diff, etc.) were already
    /// handled inside `context_menu_confirm`; this only covers actions
    /// that require GTK plumbing.
    fn dispatch_explorer_ctx_action(&mut self, action: &str, target: &std::path::Path) {
        match action {
            "new_file" | "new_folder" | "rename" | "delete" | "move_file" => {
                self.explorer_action(action.to_string());
            }
            "open_terminal" => {
                let dir = if target.is_dir() {
                    target.to_path_buf()
                } else {
                    target
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                };
                self.open_terminal_at(dir);
            }
            "find_in_folder" => {
                self.toggle_focus_search();
            }
            _ => {} // engine-handled actions (copy_path, reveal, etc.)
        }
    }

    /// Minimize the application window (inline window-control button).
    fn window_minimize(&mut self) {
        if let Some(ref w) = self.window {
            w.minimize();
        }
    }

    /// Maximize or restore the application window (inline window-control
    /// button).
    fn window_toggle_maximize(&mut self) {
        if self.window.as_ref().is_some_and(|w| w.is_maximized()) {
            if let Some(ref w) = self.window {
                w.unmaximize();
            }
        } else if let Some(ref w) = self.window {
            w.maximize();
        }
    }

    /// Close the application window (inline window-control button).
    fn window_close(&mut self) {
        if let Some(ref w) = self.window {
            w.close();
        }
    }

    /// User triggered quit; exit straight away when nothing is unsaved,
    /// otherwise raise the "unsaved changes" confirmation dialog.
    fn show_quit_confirm(&mut self) {
        if !self.engine.borrow().has_any_unsaved() {
            self.save_session_and_exit();
            return;
        }
        use crate::core::engine::DialogButton;
        self.engine.borrow_mut().show_dialog(
            "quit_unsaved",
            "Unsaved Changes",
            vec![
                "You have unsaved changes.".to_string(),
                "Do you want to save before quitting?".to_string(),
            ],
            vec![
                DialogButton {
                    label: "Save All & Quit".into(),
                    hotkey: 's',
                    action: "save_quit".into(),
                },
                DialogButton {
                    label: "Quit Without Saving".into(),
                    hotkey: 'q',
                    action: "discard_quit".into(),
                },
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
            ],
        );
        self.draw_needed.set(true);
    }

    /// Show a native "Open File" dialog.
    fn open_file_dialog(&mut self) {
        // Deferred to tick(), which has the runner-owned `backend`
        // handle PlatformServices needs — see PendingFileDialog (#572).
        self.pending_file_dialog
            .set(Some(PendingFileDialog::OpenFile));
        self.draw_needed.set(true);
    }

    /// Show a native "Open Folder" dialog.
    fn open_folder_dialog(&mut self) {
        // #572: still direct gtk4::FileDialog — quadraui::PlatformServices
        // has no folder-select mode yet (only show_file_open_dialog /
        // show_file_save_dialog, both file pickers). Needs a new
        // quadraui issue (folder-select support) before this can move.
        let engine = self.engine.clone();
        let deferred2 = self.deferred.clone();
        let dialog = gtk4::FileDialog::new();
        dialog.set_title("Open Folder");
        dialog.set_accept_label(Some("Open Folder"));
        let win = self.window.clone();
        dialog.select_folder(win.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
            if let Ok(file) = result {
                // Use UFCS to call gtk4's FileExt::path (avoids gio version conflict)
                if let Some(path) = gtk4::prelude::FileExt::path(&file) {
                    engine.borrow_mut().open_folder(&path);
                    deferred2.send(DeferredAction::RefreshFileTree);
                }
            }
        });
        self.draw_needed.set(true);
    }

    /// Finish an "Open Workspace" action started in the engine.
    fn open_workspace_dialog(&mut self) {
        // open_workspace_from_file() already ran in the engine;
        // just refresh the file tree.
        self.refresh_file_tree();
        self.draw_needed.set(true);
    }

    /// Show a native "Save Workspace As" dialog.
    fn save_workspace_as_dialog(&mut self) {
        // Deferred to tick() — see PendingFileDialog (#572).
        self.pending_file_dialog
            .set(Some(PendingFileDialog::SaveWorkspaceAs));
        self.draw_needed.set(true);
    }

    /// Show the "Open Recent" workspace picker.
    fn open_recent_dialog(&mut self) {
        // #274: replaced the native gtk4::Dialog with the engine's
        // unified picker (PickerSource::RecentWorkspaces). Picker
        // confirm calls open_folder + sets explorer_needs_refresh
        // so the file tree rebuilds on the next render — no
        // backend-specific Msg dispatch needed here.
        let mut engine = self.engine.borrow_mut();
        if engine.session.recent_workspaces.is_empty() {
            engine.message = "No recent workspaces".to_string();
        } else {
            engine.open_picker(crate::core::engine::PickerSource::RecentWorkspaces);
        }
        drop(engine);
        self.draw_needed.set(true);
    }

    /// User confirmed quit — save session state then exit the process.
    fn quit_confirmed(&mut self) {
        // Save session state then exit the process.
        self.save_session_and_exit();
    }

    /// User clicked ✕ on a tab with unsaved changes — ask what to do.
    fn show_close_tab_confirm(&mut self) {
        use crate::core::engine::DialogButton;
        self.engine.borrow_mut().show_dialog(
            "close_tab_confirm",
            "Unsaved Changes",
            vec!["This file has unsaved changes.".to_string()],
            vec![
                DialogButton {
                    label: "Save & Close".into(),
                    hotkey: 's',
                    action: "save_close".into(),
                },
                DialogButton {
                    label: "Discard & Close".into(),
                    hotkey: 'd',
                    action: "discard".into(),
                },
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
            ],
        );
        self.draw_needed.set(true);
    }

    /// #731: was `if let Some(da) = self.drawing_area…` — that field is
    /// permanently `None` under the ShellApp runner (see its removal in
    /// #731), so this always took the `else` branch. The real fix is a way
    /// to read the live DA's pixel width without a widget handle (e.g. from
    /// `backend: &mut dyn quadraui::Backend`, which none of this method's
    /// callers currently have in scope) — until then this is pinned at the
    /// fallback, same as it was silently pinned at runtime before the dead
    /// field was deleted.
    #[allow(dead_code)]
    fn terminal_cols(&self) -> u16 {
        80
    }

    /// #731: see `terminal_cols` — was `if let Some(da) =
    /// self.drawing_area…`, permanently `None`, so this always took the
    /// `else` branch.
    fn terminal_target_maximize_rows(&self) -> u16 {
        10
    }
}

// ── Dormant ShellApp impl (#448-B) ──────────────────────────────────────────
// This impl compiles alongside the Relm4 path but is NOT wired up.
impl App {
    /// Paint the title-bar band: the menu bar + any open dropdown, the app-icon
    /// slot, and the inline window controls.
    ///
    /// One function because all three draw into the *same* strip and their
    /// order within it is fixed by a rasteriser detail rather than by taste:
    /// `MenuSystem::render` calls `draw_menu_bar` across the whole band, so the
    /// icon slot and the controls must follow it or get erased (the #552
    /// round-2/3 "buttons render blank" regression). Kept off
    /// [`render::OVERLAY_Z_ORDER`] because TUI has neither an app icon nor
    /// in-canvas window controls, so they cannot be part of a *shared*
    /// sequence; they ride the `MenuDropdown` rung instead (#735).
    #[allow(clippy::too_many_arguments)]
    fn paint_title_bar_band(
        &self,
        backend: &mut dyn quadraui::Backend,
        engine: &Engine,
        theme: &Theme,
        menu_row_rect: quadraui::Rect,
        menu_items_rect: quadraui::Rect,
        app_icon_rect: quadraui::Rect,
        controls_rect: Option<quadraui::Rect>,
    ) {
        {
            // `menu_items_rect`, not `menu_row_rect` — the app icon owns the
            // leading slot (#720). `MenuSystem::render` positions the open
            // dropdown from this same rect, so passing the narrowed one is
            // what keeps a dropdown under the label that opened it.
            engine.menu_system.borrow().render(backend, menu_items_rect);

            // ── App icon, left of `File` (#720) ──────────────────────────
            // `draw_menu_bar` above only filled `menu_items_rect`, so the
            // reserved slot still shows the frame-clear colour
            // (`theme.background`) rather than the bar's own `tab_bar_bg`.
            // Painting an *item-less* `MenuBar` across the slot fills it
            // through the very same rasteriser as the strip beside it, so
            // the two backgrounds cannot drift apart the way a hand-picked
            // theme colour would.
            if app_icon_rect.width > 0.0 && app_icon_rect.height > 0.0 {
                let filler = quadraui::MenuBar {
                    id: quadraui::WidgetId::new("app_icon_slot"),
                    items: Vec::new(),
                    open_item: None,
                    focused_item: None,
                };
                let _ = backend.draw_menu_bar(
                    quadraui::Rect::new(
                        menu_row_rect.x,
                        menu_row_rect.y,
                        (menu_items_rect.x - menu_row_rect.x).max(0.0),
                        menu_row_rect.height,
                    ),
                    &filler,
                );
                // `util::app_icon_image`, not `render::app_icon_image`: the
                // former hands over a once-rasterised small PNG instead of the
                // 1024x1024 SVG, which `Backend::draw_image` would otherwise
                // re-render through librsvg on *every* frame (+16.5ms per
                // repaint — see that function's doc comment).
                let _ = backend.draw_image(app_icon_rect, &util::app_icon_image());
            }
        }

        // ── Inline window controls (min/max/close) — after the bar (#552) ────
        // `menu_system.render()` above repaints `draw_menu_bar` across the full
        // `menu_row_rect` band, so the controls must be painted *after* it or
        // they get erased (the round-2/3 "buttons render blank" regression).
        // The controls sit in the title-bar band, to the right of the menu
        // labels; the dropdown body drops *below* the band, so painting here
        // never covers an open dropdown.
        //
        // #735 moved this from the very end of `render_content` (below the
        // dialog and context menu) to here. It is title-bar chrome, so the
        // modal rungs of `render::OVERLAY_Z_ORDER` now paint over it — which is
        // the point: a modal dialog covering the window controls is what
        // "modal" means, and it is what TUI already did with everything it
        // painted into its own title-bar row.
        if let Some(controls_rect) = controls_rect {
            let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
            let controls_bar = render::window_controls_status_bar(theme, maximized);
            let interaction = self.title_bar_interaction.borrow();
            let hits = backend.draw_status_bar(
                controls_rect,
                &controls_bar,
                interaction.hovered_id(),
                interaction.pressed_id(),
            );
            interaction.set_layout(hits);
        }
    }
}

impl quadraui::ShellApp for App {
    fn setup(&mut self, backend: &mut dyn quadraui::Backend) {
        // Seed cached metrics from runner defaults.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.cached_ui_line_height = self.cached_line_height;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);
        // (#547) Seed the backend's nerd-fonts flag. The only prior call
        // site was the `Msg::CacheFontMetrics` arm, which stopped firing after
        // the #540 ShellApp migration, silently freezing quadraui's GTK backend
        // at its default of `false` — the cause of the explorer treeview
        // falling back to ASCII icons. (That arm had still never regained a
        // producer, so #732 deleted it; this call is the live replacement.)
        render::sync_nerd_fonts(backend, &self.engine.borrow());

        // Try to grab the runner-created GTK window now so minimize/maximize/
        // close work and the server-side WM titlebar is dropped in favour of
        // the drawn CSD row; `setup()` runs before `run_with_shell`'s runner
        // calls `window.present()`, so it is very likely not yet mapped and
        // this lookup finds nothing. `tick()` retries every frame until the
        // window is mapped, which is the reliable path (#552).
        self.capture_window_and_apply_csd();

        // GTK draws its own VSCode-style menu bar (File/Edit/View/...) — it
        // acts as the client-side titlebar, always visible (unlike TUI, which
        // only shows it in vscode-mode or via Alt). Historical GTK behaviour
        // pre-#540; menu defs were never re-populated after the ShellApp
        // migration deleted the Relm4 headerbar wiring. (#552)
        let is_vscode_mode = self.engine.borrow().is_vscode_mode();
        self.engine.borrow_mut().menu_bar_visible = true;
        self.engine
            .borrow()
            .menu_system
            .borrow_mut()
            .set_menus(render::build_menu_defs(is_vscode_mode));

        // Apply initial CSS (no-op under the headless test harness, which has
        // no display to attach a provider to — see the field's doc, #646).
        if let Some(p) = &self.css_provider {
            let theme = Theme::from_name(&self.engine.borrow().settings.colorscheme);
            let combined = format!("{STATIC_CSS}\n{}", make_theme_css(&theme));
            p.load_from_data(&combined);
        }

        // Register the panel-keys accelerator set (toggle sidebar, fuzzy
        // finder, live grep, command palette, ...) on the runner's backend.
        // This was previously only wired for TUI (`tui_main::run` calls it
        // right after `TuiBackend::new()`); the GTK side's registration
        // function existed but was never called after the ShellApp
        // migration, so none of these 14 global shortcuts — including
        // Ctrl+Shift+P for the command palette — ever fired on GTK (#587).
        register_panel_accelerators(backend, &self.engine.borrow().settings.panel_keys);
    }

    fn render_content(
        &self,
        backend: &mut dyn quadraui::Backend,
        layout: &quadraui::AppShellLayout,
    ) {
        use quadraui::{ScreenLayout as QSL, Surface};

        let engine = self.engine.borrow();
        let theme = Theme::from_name(&engine.settings.colorscheme);
        backend.set_theme(render::to_quadraui_theme(&theme));
        // (#547) Re-synced every frame so runtime toggles (`:set
        // nonerdfonts`) take effect immediately, matching TUI.
        render::sync_nerd_fonts(backend, &engine);
        // Re-synced every frame so a runtime `:set guifont`/font-size change
        // takes effect immediately (#217/#672). Ported from the dead
        // `draw.rs::draw_editor`'s top-of-frame call, which was the only
        // live caller — `UI_FONT()` (used a few paint calls below for the
        // raw-Pango chrome that doesn't go through `Backend::set_ui_font`)
        // read the process-global atomic this writes, so before this port it
        // silently stayed pinned at the default size forever.
        sync_ui_font_size(&engine.settings);
        // #705 item 3 / quadraui#624: push the same UI_FONT() family+size
        // onto the *paint* backend's `ui_font`, which `draw_status_bar`
        // (breadcrumbs, per-window/global status lines), `draw_tree`
        // (explorer), `draw_tab_bar_icons`, and `draw_menu_bar` now all
        // honour for both paint and their no-paint measurement twins
        // (quadraui#624). Before this call `ui_font` on the paint backend
        // (a *separate* `GtkBackend` instance from `self.backend`, which
        // `sync_nerd_fonts` above already keeps synced for click-time
        // hit-testing) was never touched, so it
        // sat at quadraui's own "Sans 11" default forever: chrome text
        // didn't track `settings.ui_font_size`, and — per #700's item 3 —
        // status-bar-painted breadcrumb text had no font of its own to
        // decouple it from whatever font a prior draw call in the frame
        // left on the shared Pango layout. Re-set every frame (not just
        // once from `setup()`) so a runtime `:set ui_font_size=N` takes
        // effect immediately, matching `sync_ui_font_size`/`sync_nerd_fonts`
        // just above.
        backend.set_ui_font(&UI_FONT());

        // #672: scroll surfaces are re-registered from scratch every frame
        // (mirrors TUI's `render_impl.rs` `scroll_surfaces.borrow_mut().clear()`)
        // so a panel that closes — or moves — doesn't leave a stale entry
        // behind for `dispatch_scroll`/`dispatch_click` to hit-test against.
        // Ported from the dead `draw.rs::draw_editor`'s equivalent top-of-frame
        // clear, which was this list's only writer under `ShellApp` (#592/#672).
        engine.scroll_surfaces.borrow_mut().clear();
        // Per-window status bar segment hit zones (#672): `click.rs`'s
        // `pixel_to_click_target` reads this to resolve `WindowZone::StatusBar`
        // clicks (goto-line, change-language, switch-branch, ...) to a
        // `StatusAction`, but under `ShellApp` nothing ever populated it — the
        // dead `draw.rs::draw_window_status_bar` was the only writer, so every
        // per-window status bar segment click silently resolved to
        // `ClickTarget::None`. Cleared here and re-inserted per window below
        // (and for the separated status line further down) so a window that
        // closes doesn't leave a stale, now-wrong entry keyed by its id.
        self.status_segment_map.borrow_mut().clear();

        let lh = self.cached_line_height.max(backend.line_height() as f64);
        let cw = self.cached_char_width.max(backend.char_width() as f64);
        // Publish the value this frame paints with so click-time hit-tests can
        // use it (#555). `render_content` takes `&self`, so it cannot write
        // the plain `cached_line_height` field — which is seeded once in
        // `setup()` from the runner's *default* metrics and can therefore be
        // smaller than the `lh` every frame actually paints with. Hit-testing
        // painted geometry against the smaller value put row boundaries in the
        // wrong place (the picker resolved clicks two rows off) and clipped
        // the bottom of every single-row band, breadcrumbs included.
        self.painted_line_height.set(Some(lh));
        self.painted_char_width.set(Some(cw));

        let main = layout.main_content_bounds;
        let (x, y, w, h) = (
            main.x as f64,
            main.y as f64,
            main.width as f64,
            main.height as f64,
        );
        if w < 1.0 || h < 1.0 {
            return;
        }

        // ── Menu bar row (client-side chrome; #552) ─────────────────────────────
        // quadraui's `run_with_shell` GTK runner (single-DA architecture, #217)
        // creates the window undecorated with no native titlebar/menu hosting.
        // `ShellConfig::with_title_bar()` (set in `run()`) reserves a
        // full-width band across the top of the *entire* shell — above the
        // activity bar and sidebar too, not just `main_content_bounds` — so
        // GTK's drawn menu bar + inline window controls span the whole
        // window like a real titlebar, and the activity bar/sidebar/main
        // content the runner hands us below are already shifted down to
        // make room. Mirrors the pre-#540 Relm4 headerbar and TUI's
        // identical row (render_impl.rs) via the same shared
        // `engine.menu_system` / `Backend::draw_menu_bar`.
        let menu_row_rect = layout.title_bar_bounds.unwrap_or_default();
        self.menu_row_rect.set(menu_row_rect);
        // #720: reserve a square, row-height slot at the leading edge of the
        // band for the VimCode app icon (VS Code puts its logo left of
        // `File`), and lay the menu items out in what's left. This is the
        // *only* place the split is computed: `menu_items_rect` is what the
        // measurement below, `menu_system.render()` and — via
        // `self.menu_items_rect` — `handle()`'s click routing all use, so the
        // icon's x-shift can never be applied to the paint but not the
        // hit-test (quadraui's `MenuBar::layout_with_leading` doc names this
        // exact hazard; vimcode goes through `MenuSystem`, which owns its own
        // `MenuBar::layout` call and takes no leading width, so the narrowed
        // rect is how vimcode expresses the same thing).
        let (app_icon_rect, menu_items_rect) = if engine.menu_bar_visible {
            render::split_menu_row_for_app_icon(menu_row_rect)
        } else {
            (quadraui::Rect::default(), menu_row_rect)
        };
        self.menu_items_rect.set(menu_items_rect);
        // Compute where the menu labels end so the inline window controls can
        // sit to their right. This is layout-only (`menu_bar_layout`, no draw):
        // the menu bar itself — and the whole `menu_row_rect` band — is painted
        // by `menu_system.render()` near the end of this method. Drawing the
        // controls *here* (as this used to) is pointless because that later
        // `render()` repaints `draw_menu_bar` across the entire band and erases
        // them (#552 round-2/3 "buttons render blank"). So we only stash the
        // target rect now and paint the buttons *after* `render()` below.
        let (controls_rect, command_center_rect) =
            if engine.menu_bar_visible && menu_row_rect.height > 0.0 {
                let bar = engine.menu_system.borrow().menu_bar();
                let mb_layout = backend.menu_bar_layout(menu_items_rect, &bar);
                // `vi.bounds.x` is already absolute (quadraui's `MenuBar::layout`
                // starts its cursor at `bounds.x`, the rect passed above) — do not
                // add `menu_row_rect.x` again.
                let menu_end = mb_layout
                    .visible_items
                    .last()
                    .map(|vi| vi.bounds.x + vi.bounds.width)
                    // #720: an item-less bar still starts *after* the app-icon
                    // slot, so the Command Center / window controls must not
                    // be handed the icon's real estate back.
                    .unwrap_or(menu_items_rect.x);
                let full_rect = quadraui::Rect::new(
                    menu_end,
                    menu_row_rect.y,
                    (menu_row_rect.x + menu_row_rect.width - menu_end).max(0.0),
                    menu_row_rect.height,
                );
                // #676: narrow the controls rect to the window-control buttons'
                // *actual* painted width instead of handing them the entire
                // menu_end→right-edge band. That full band, background-filled
                // end-to-end by `window_controls_status_bar` (via
                // `Backend::draw_status_bar`), is exactly what silently ate the
                // VS Code-style Command Center's real estate after the #540
                // Relm4→ShellApp cutover dropped it. `status_bar_layout` mirrors
                // `draw_status_bar`'s own measurement (its doc comment: "audited
                // under #552 and ruled out" — paint and no-paint agree), so this
                // is the same width the buttons paint at, a few hundred lines
                // below.
                let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
                let controls_bar = render::window_controls_status_bar(&theme, maximized);
                let controls_layout = backend.status_bar_layout(full_rect, &controls_bar);
                let controls_start = controls_layout
                    .visible_segments
                    .iter()
                    .map(|vs| vs.bounds.x)
                    .fold(f32::INFINITY, f32::min);
                let controls_start = if controls_start.is_finite() {
                    controls_start
                } else {
                    full_rect.width
                };
                let rect = quadraui::Rect::new(
                    full_rect.x + controls_start,
                    full_rect.y,
                    (full_rect.width - controls_start).max(0.0),
                    full_rect.height,
                );
                self.title_bar_rect.set(rect);

                // The gap freed up between the last menu label and the
                // now-narrow window controls is exactly where the VS Code-style
                // Command Center (nav arrows + search box) belongs — painted via
                // `render::build_command_center_view` / `Backend::draw_command_center`
                // after `menu_system.render()` below (same ordering constraint
                // as the window controls: that call repaints the full
                // `menu_row_rect` band and would erase anything painted first).
                let cc_rect = quadraui::Rect::new(
                    menu_end,
                    menu_row_rect.y,
                    (rect.x - menu_end).max(0.0),
                    menu_row_rect.height,
                );
                (Some(rect), Some(cc_rect))
            } else {
                self.title_bar_rect.set(quadraui::Rect::default());
                (None, None)
            };

        // ── Layout ────────────────────────────────────────────────────────────
        let tab_row_h = render::tab_row_height_px(lh);
        let tab_bar_h = render::tab_bar_height_px(lh, engine.settings.breadcrumbs);
        let per_window_status = engine.settings.window_status_line;
        let el = render::compute_editor_layout(&engine, h, lh, false);
        // `el.status_bar_h` is `compute_editor_layout`'s single source of
        // truth for this (identical formula to the `wildmenu_px`/
        // `status_rows` locals this replaced); reusing it here — instead of
        // recomputing a second copy — is what makes `editor_area_h` below
        // `el.editor_bottom` correctly reserve quickfix's band too.
        let status_bar_h = el.status_bar_h;
        // `el.editor_bottom` already subtracts quickfix_h/terminal_h/
        // debug_toolbar_h/separated_status_h/status_bar_h from `h` (menu_h
        // is 0 for GTK — the menu bar lives outside `main_content_bounds`,
        // see `compute_editor_layout`'s `menu_in_viewport` doc). Before
        // #670 this was hand-rolled here without the `quickfix_h` term, so
        // an open quickfix panel never reserved space and editor content
        // painted straight through where the panel now paints.
        let editor_area_h = el.editor_bottom.max(0.0);

        let editor_bounds = WindowRect::new(x, y, w, editor_area_h);
        // Hand the exact bounds/tab-bar-height this frame painted with to the
        // click + drag handlers, so divider hit-tests land on the painted line
        // instead of on a second, differently-originated guess (#582).
        self.cached_editor_bounds
            .set(Some((editor_bounds, tab_bar_h)));
        let (window_rects, _dividers) =
            engine.calculate_group_window_rects(editor_bounds, tab_bar_h);

        // #700: the breadcrumb row's own painted bounds must agree with the
        // fixed-pixel space `tab_bar_h` (above) already reserved for it above
        // the window content — plain `build_screen_layout` would assume the
        // breadcrumb row is exactly one `lh`-tall editor text line, which is
        // no longer true now that the row is a fixed 22px regardless of
        // `settings.font_size`.
        let screen = render::build_screen_layout_with_breadcrumb_row(
            &engine,
            &theme,
            &window_rects,
            lh,
            cw,
            false,
            render::BREADCRUMB_ROW_HEIGHT_PX,
        );

        // Cache for click handlers (move into RefCell, then borrow back for drawing).
        *self.cached_screen_layout.borrow_mut() = Some(screen);
        let screen_ref = self.cached_screen_layout.borrow();
        let screen = screen_ref.as_ref().unwrap();

        // #560: give the *click* backend a correctly-fonted editor Pango
        // context so mouse clicks resolve columns via the per-glyph Pango
        // inverse rather than a naive uniform-cell division.
        //
        // vimcode keeps a SEPARATE `GtkBackend` (`self.backend`) for click-time
        // hit-testing than the one quadraui's ShellApp runner creates and
        // paints with (`quadraui::gtk::run` owns the single DrawingArea and its
        // backend; see the `self.drawing_area` note in `tick`). The runner's
        // backend is the one that stashes `last_editor_pango_layout` during the
        // `frame.draw(backend)` calls below and is handed to `render_content`
        // as `backend` — but it is NOT `self.backend`, and the trait exposes no
        // way to copy its Pango context across. So `self.backend`, used by
        // `pixel_to_click_target -> editor_col_at_x`, had neither a stashed
        // editor layout nor a Pango context of its own and fell through to
        // `EditorLayout::col_at_x`'s uniform per-cell division: exact for
        // monospace glyphs, but drifting +1 column for every preceding wide
        // glyph (emoji ✅/🟡/❌/⏭, CJK) — the reported #560 symptom. Mirroring
        // what the runner does for its own backend (`gtk::run` sets the
        // DrawingArea's Pango context), we hand the click backend an
        // editor-fonted PangoCairo context, so `editor_col_at_x` resolves via
        // `quadraui::gtk::editor_col_at_x`'s exact per-glyph `xy_to_index` path.
        // Rebuilt each frame so runtime font changes (`:set guifont`) take
        // effect immediately.
        //
        // CRITICAL: the context must reproduce the *painted* font's glyph
        // advances, NOT `settings.font_*`. The runner paints the editor with a
        // hardcoded "Monospace 11" (see `quadraui::gtk::run`), ignoring
        // `settings.font_family`/`font_size`; `cw` (== `backend.char_width()`)
        // is that painted cell width, and `build_screen_layout` above used it.
        // Fonting this context from `settings.font_size` (14) while the paint
        // ran at 11 was the #560 iteration-3 smoke failure: `xy_to_index`
        // scaled columns by the wrong cell width and drifted left, the drift
        // growing with `x`, on plain/bold/italic/scrolled lines alike.
        // `build_editor_click_context` matches by measuring against `cw`.
        if let Some(click_ctx) = click::build_editor_click_context(cw) {
            self.backend.borrow_mut().set_pango_context(click_ctx);
        }

        // ── Draw editor windows ───────────────────────────────────────────────
        // `window_editors` stashes each window's owned `quadraui::Editor`
        // past the loop (#449) so the FrameHitMap built just below can
        // reference the SAME objects just painted, instead of constructing
        // a second copy that could drift from what's on screen.
        //
        // #731: no vertical (or horizontal) scrollbar is painted for the
        // editor on GTK today. quadraui's `gtk::editor::draw_editor` (the
        // rasteriser `Surface::Editor` below calls into) documents that it
        // deliberately skips scrollbars on GTK and defers to "the host" —
        // meaning the Relm4-era native `gtk4::Scrollbar` overlay path this
        // issue deleted (`sync_scrollbar`/`create_window_scrollbars`, plus
        // the pixel-inset math in the now-deleted
        // `native_scrollbar_margin_start`: `rect.x + rect.width -
        // minimap_width - scrollbar_width - 2.0`, clamped to `rect.x`).
        // That path never ran under the ShellApp runner (nothing assigns
        // `self.overlay`/`self.drawing_area`), so it was already dead
        // before this cleanup — this was not a working feature this PR
        // broke.
        //
        // TUI's equivalent rasteriser (`quadraui::tui::editor`) paints an
        // inline vertical + horizontal scrollbar column as part of the
        // `Editor` primitive itself, narrowing the text viewport to make
        // room — see `super::draw_scrollbar` calls in that module. GTK has
        // no equivalent; closing this gap means teaching
        // `quadraui::gtk::editor::draw_editor` to do the same (Cairo-paint
        // a scrollbar inside `editor.rect`, mirroring TUI), which also
        // makes the minimap inset automatic (the viewport is already
        // narrowed by `minimap_reserved_width` before the rect reaches the
        // rasteriser) — no separate margin-inset formula would be needed
        // there. That is quadraui-side work per `CLAUDE.md`'s
        // Platform-Neutrality Rule (file a quadraui issue; do not
        // reintroduce GTK-specific scrollbar widget plumbing here). #723's
        // fix (`e02a824`) targeted the dead native-widget path and cannot
        // have been visible on screen; it needs re-verifying once this
        // lands there.
        let mut window_editors: Vec<quadraui::Editor> = Vec::with_capacity(screen.windows.len());
        for rw in &screen.windows {
            let editor = render::to_q_editor(rw);
            let rect = editor.rect;
            let mut frame = QSL::new();
            frame.push(Surface::Editor {
                rect,
                editor: &editor,
            });
            frame.draw(backend);
            window_editors.push(editor);

            // Per-window status bar (when window_status_line=true, which is
            // the default; global_status_bar is None in that mode).
            if let Some(ref status) = rw.status_line {
                let bar_y = rw.rect.y + rw.rect.height - lh;
                let sb_rect = quadraui::Rect::new(
                    rw.rect.x as f32,
                    bar_y as f32,
                    rw.rect.width as f32,
                    lh as f32,
                );
                let win_bar = render::window_status_line_to_status_bar(
                    status,
                    quadraui::WidgetId::new(format!("status:{}", rw.window_id.0)),
                );
                let mut frame = QSL::new();
                frame.push(Surface::StatusBar {
                    rect: sb_rect,
                    bar: &win_bar,
                    hovered: None,
                    pressed: None,
                });
                frame.draw(backend);

                // #672: recover segment hit zones the same way the dead
                // `draw.rs::draw_window_status_bar` did, so
                // `pixel_to_click_target`'s `WindowZone::StatusBar` arm has a
                // real `status_segment_map` entry to resolve against instead
                // of an always-empty one. `status_bar_layout` lays segments
                // out bar-relative from `(0, 0)` regardless of `sb_rect`'s own
                // origin (see `route_debug_sidebar_event`'s doc comment above
                // for the same "`StatusBar::layout` always starts at 0,0"
                // contract), which is exactly the window-relative `local_x`
                // `window_zone_hit_test` hit-tests with — no coordinate
                // translation needed.
                let sb_layout = backend.status_bar_layout(sb_rect, &win_bar);
                self.status_segment_map.borrow_mut().insert(
                    rw.window_id.0,
                    render::status_bar_zones_from_layout(&sb_layout),
                );
            }
        }

        // ── Window-split divider lines (#582 follow-up) ────────────────────────
        // `:split`/`:vsplit` boundaries had no visual of their own in GTK —
        // nothing told the user where to grab. Paint one via quadraui's `Split`
        // primitive (already wired for both backends via `Surface::Split` ->
        // `Backend::draw_split`) rather than hand-rolling Cairo here.
        //
        // Both axes are painted. The iteration-2 smoke found `:split` only
        // *seemed* draggable because the per-window status bar happens to sit
        // one line above the boundary and reads as a divider; that is a
        // coincidence of an unrelated feature, not a handle, and it leaves the
        // real 6px hit band invisible. A true line on both axes makes what is
        // grabbable match what is drawn.
        for div in &screen.window_dividers {
            let (split, rect) = render::divider_to_split(
                div,
                quadraui::WidgetId::new(format!("wdiv:{}:{}", div.group_id.0, div.split_index)),
            );
            let mut frame = QSL::new();
            frame.push(Surface::Split {
                rect,
                split: &split,
            });
            frame.draw(backend);
        }

        // #35/#722: minimap strips on every window's right edge (one entry
        // per `WindowId` in `screen.minimap`, not just the active window's)
        // — one call, the font-scaling rasteriser is quadraui's.
        render::draw_minimap_strip(backend, screen);

        // ── Recover a FrameHitMap for Editor/TabBar zone detection (#449) ──────
        // Pure `.push()` accumulation into a *separate* `ScreenLayout`, built
        // from the same `Editor` objects just painted above (`window_editors`,
        // same order as `screen.windows` so `FrameZone::Editor { idx }` maps
        // straight back to `cached_layout.windows[idx]`) plus the `TabBar`
        // surfaces pushed in the loop just below. `ScreenLayout::hit_map()`
        // (quadraui#425) makes no `backend.draw_*()` calls, so accumulating
        // into it can never reorder or repeat the real painting done above —
        // see `click::pixel_to_click_target` for the consumer side.
        let mut hit_frame = QSL::new();
        for editor in &window_editors {
            hit_frame.push(Surface::Editor {
                rect: editor.rect,
                editor,
            });
        }

        // ── Draw tab bar(s) — one per editor group ────────────────────────────
        // Multi-group (post-split) layouts have a tab bar per group, each drawn
        // at the top edge of its own bounds. Single-group draws one full-width
        // bar at the editor top. Previously only the single-group primitive was
        // drawn, so split groups rendered with no tab bar at all. (#515)
        // Reset the pixel-accurate hit caches; repopulated per tab bar below so
        // the click / hover hit-tests use the exact drawn geometry (#515).
        let mut pixel_hits = self.cached_tab_pixel_hits.borrow_mut();
        let mut close_abs = self.cached_tab_close_abs.borrow_mut();
        let mut slots_abs = self.cached_tab_slots_abs.borrow_mut();
        pixel_hits.clear();
        close_abs.clear();
        slots_abs.clear();
        // Parallel table for `FrameZone::TabBar { idx }` resolution (#449),
        // keyed by the *global* surface index — `hit_frame` already has
        // `window_editors.len()` `Surface::Editor`s pushed into it above, so
        // the first tab bar's `FrameZone::TabBar { idx }` is
        // `window_editors.len()`, not `0`. See `cached_tab_bar_zones`'s doc
        // comment for why a plain `Vec` indexed from 0 was wrong.
        let mut tab_bar_zones: HashMap<usize, (core::window::GroupId, quadraui::Rect)> =
            HashMap::new();
        for (next_surface_idx, target) in (window_editors.len()..).zip(
            render::tab_bar_draw_targets(&engine, screen, tab_row_h, tab_bar_h),
        ) {
            let tb_rect = target.rect;
            let hover = self
                .tab_close_hover
                .and_then(|(gid, i)| (gid == target.group_id.0).then_some(i));
            // #703: painted via `Backend::draw_tab_bar_icons` directly rather
            // than a `Surface::TabBar` push, because quadraui's `Surface` enum
            // carries no icon sidecar (adding a field to it would be the same
            // hard break on downstream consumers that kept the icons off
            // `TabItem` in the first place). With an empty sidecar the two are
            // byte-identical — quadraui's `draw_tab_bar` forwards to
            // `draw_tab_bar_icons` with `&[]` — so this is a pure superset of
            // the old call. `hit_frame` below still gets a `Surface::TabBar`:
            // it is only ever consumed via `hit_map()` (never drawn) and its
            // zones are whole-bar rects, which icons do not move.
            backend.draw_tab_bar_icons(tb_rect, target.bar, target.icons, hover);
            // `target.bar` borrows from `screen` (function-scoped), so this
            // can push directly into `hit_frame` without hoisting (#449).
            hit_frame.push(Surface::TabBar {
                rect: tb_rect,
                bar: target.bar,
                hovered_close: hover,
            });
            tab_bar_zones.insert(next_surface_idx, (target.group_id, tb_rect));
            // Recover the exact pixel geometry the rasteriser just drew and
            // cache it (relative to the bar's left edge) for hit-testing.
            //
            // #703: must be the `_icons` twin, with the *same* sidecar the
            // paint above used. The icon reservation widens every decorated
            // tab, so the icon-less `tab_bar_layout` reports slot and close
            // bounds shifted left of the painted glyphs — i.e. the close × of
            // tab N lands inside tab N+1's painted slot, and clicking it
            // closes the wrong tab. Exactly the measure/paint desync of #654.
            let hits = backend.tab_bar_layout_icons(tb_rect, target.bar, target.icons);
            let ph = tab_hits_to_pixel_hits(&hits, target.bar, tb_rect.x as f64);
            let bar_top = tb_rect.y as f64;
            close_abs.insert(
                target.group_id.0,
                abs_close_record(&ph.close, tb_rect.x as f64, bar_top, bar_top + tab_row_h),
            );
            slots_abs.insert(target.group_id.0, abs_visible_slots(&hits));
            pixel_hits.insert(target.group_id.0, ph);
        }
        drop(close_abs);
        drop(slots_abs);
        drop(pixel_hits);
        *self.cached_frame_hit_map.borrow_mut() = Some(hit_frame.hit_map());
        *self.cached_tab_bar_zones.borrow_mut() = tab_bar_zones;

        // ── Draw breadcrumb bar(s) below tab bar(s) ─────────────────────────────
        // (#547) `render_content` is the active ShellApp draw path since the
        // #540 Relm4→ShellApp migration; the legacy `draw.rs::draw_editor`
        // path that used to draw breadcrumbs is dead (no callers) and this
        // step was never ported over, so breadcrumbs stopped rendering even
        // though layout space for them was still reserved (`tab_bar_h`
        // above) and clicks were still hit-tested against them.
        for t in render::breadcrumb_draw_targets(screen, engine.terminal_maximized) {
            let mut frame = QSL::new();
            frame.push(Surface::StatusBar {
                rect: t.rect,
                bar: t.bar,
                hovered: None,
                pressed: None,
            });
            frame.draw(backend);
            *t.draw_layout.borrow_mut() = Some(backend.status_bar_layout(t.rect, t.bar));
        }

        // ── Tab-hover tooltip (#671) ─────────────────────────────────────────
        // Small popup shown when the mouse lingers over a tab, naming the
        // buffer under the cursor. `screen.tab_tooltip` was populated by the
        // engine the whole time (#592's root cause) but had no GTK painter at
        // all — unlike quickfix/panel_hover (#670) there was no dead
        // `draw.rs` version to port either; `draw.rs:425` painted it with raw
        // Cairo/Pango, never through `Backend`. Routed through the new shared
        // `render::tab_hover_tooltip_paint` (mirrors TUI's
        // `render_tab_hover_tooltip`, just called with GTK's `cw`/`lh` pixel
        // scale instead of TUI's 1.0/1.0 cell scale) so paint logic isn't
        // reimplemented per backend. Positioned one *tab row* below the top
        // of the editor column — `tab_row_h` (computed above), not `lh` —
        // mirroring TUI's `area.y + 1`: TUI's tab bar is exactly one
        // *cell row* tall regardless of the breadcrumbs setting (see
        // `mouse.rs`'s `tab_bar_rows`), so its `+1` clears only the tab row
        // itself, same as GTK's `tab_row_h` here (as opposed to
        // `tab_bar_h`, which also reserves the breadcrumb row when that
        // setting is on — using `lh` alone landed the tooltip's top edge
        // inside the tab row's own vertical span, painting over tab labels
        // instead of below them, since GTK's tab row is `1.6×` a line
        // height, not `1×` like TUI's).
        if let Some(ref tooltip_text) = screen.tab_tooltip {
            render::tab_hover_tooltip_paint(
                backend,
                x as f32,
                (y + tab_row_h) as f32,
                w as f32,
                tooltip_text,
                &theme,
                cw as f32,
                lh as f32,
            );
        }

        // ── Draw editor-anchored popups (on top of everything else) ────────────
        // Completion menu, LSP hover, editor hover (rich markdown), diff peek,
        // signature help. (#669) Ported from the dead `src/gtk/draw.rs` path —
        // same class of gap as the breadcrumb note above (#547): the #540
        // Relm4->ShellApp migration dropped this paint step even though the
        // engine has populated these `screen.*` fields unchanged the whole
        // time. Content comes from the same shared `render::` adapters TUI's
        // `paint_editor_popups` uses (`completion_menu_to_quadraui_completions`,
        // `hover_popup_to_quadraui_tooltip`, `signature_help_to_quadraui_tooltip`,
        // `diff_peek_to_quadraui_tooltip`, `editor_hover_popup_paint`); geometry
        // is expressed in GTK's native pixel units (`lh`/`cw`) rather than TUI's
        // cell units, which those adapters accept as an explicit `unit_w`/
        // `unit_h` scale — mirroring how `Completions::layout`/
        // `RichTextPopup::layout` already take an explicit `line_height`/
        // `row_height` rather than assuming cells.
        if let Some(active_win) = screen
            .windows
            .iter()
            .find(|w| w.window_id == screen.active_window_id)
        {
            let gutter_w = active_win.gutter_char_width as f64 * cw;
            let h_scroll = active_win.scroll_left as f64 * cw;
            let win_x = active_win.rect.x;
            let win_y = active_win.rect.y;
            let win_viewport = quadraui::Rect::new(
                active_win.rect.x as f32,
                active_win.rect.y as f32,
                active_win.rect.width as f32,
                active_win.rect.height as f32,
            );
            let main_viewport = quadraui::Rect::new(x as f32, y as f32, w as f32, h as f32);

            // Completion popup — cache the layout so the click handler
            // (B.5b Stage 5) can hit-test items and register the popup on
            // the modal stack.
            *self.completion_layout.borrow_mut() = None;
            if let (Some(menu), Some((cursor_pos, _))) = (&screen.completion, &active_win.cursor) {
                let cursor_x = win_x + gutter_w + cursor_pos.col as f64 * cw - h_scroll;
                let cursor_y = win_y + cursor_pos.view_line as f64 * lh;
                // Longest candidate + 2 cells of padding/border, floored at 100px.
                let popup_w = ((menu.max_width + 2) as f64 * cw).max(100.0);
                let max_popup_h = 10.0 * lh;
                let completions = render::completion_menu_to_quadraui_completions(menu);
                let q_layout = completions.layout(
                    cursor_x as f32,
                    cursor_y as f32,
                    lh as f32,
                    win_viewport,
                    popup_w as f32,
                    max_popup_h as f32,
                    |_| quadraui::CompletionItemMeasure::new(lh as f32),
                );
                let mut frame = QSL::new();
                frame.push(Surface::Completions {
                    completions: &completions,
                    layout: &q_layout,
                });
                frame.draw(backend);
                *self.completion_layout.borrow_mut() = Some(q_layout);
            }

            // Simple LSP hover popup (plain text, non-interactive).
            if let Some(ref hover) = screen.hover {
                let anchor_view = hover.anchor_line.saturating_sub(active_win.scroll_top) as f64;
                let anchor_x = win_x + gutter_w + hover.anchor_col as f64 * cw - h_scroll;
                let anchor_y = win_y + anchor_view * lh;
                let (tooltip, tip_layout) = render::hover_popup_to_quadraui_tooltip(
                    hover,
                    anchor_x as f32,
                    anchor_y as f32,
                    main_viewport,
                    cw as f32,
                    lh as f32,
                );
                let mut frame = QSL::new();
                frame.push(Surface::Tooltip {
                    tooltip: &tooltip,
                    layout: &tip_layout,
                });
                frame.draw(backend);
            }

            // Signature-help popup (insert mode, cursor inside a call).
            if let Some(ref sig) = screen.signature_help {
                let anchor_view = sig.anchor_line.saturating_sub(active_win.scroll_top) as f64;
                let anchor_x = win_x + gutter_w + sig.anchor_col as f64 * cw - h_scroll;
                let anchor_y = win_y + anchor_view * lh;
                let (tooltip, tip_layout) = render::signature_help_to_quadraui_tooltip(
                    sig,
                    anchor_x as f32,
                    anchor_y as f32,
                    main_viewport,
                    &theme,
                    cw as f32,
                    lh as f32,
                );
                let mut frame = QSL::new();
                frame.push(Surface::Tooltip {
                    tooltip: &tooltip,
                    layout: &tip_layout,
                });
                frame.draw(backend);
            }

            // Diff-peek popup (inline git hunk preview).
            if let Some(ref peek) = screen.diff_peek {
                let anchor_view = peek.anchor_line.saturating_sub(active_win.scroll_top) as f64;
                let anchor_x = win_x + gutter_w;
                let anchor_y = win_y + anchor_view * lh;
                let (tooltip, tip_layout) = render::diff_peek_to_quadraui_tooltip(
                    peek,
                    anchor_x as f32,
                    anchor_y as f32,
                    main_viewport,
                    &theme,
                    cw as f32,
                    lh as f32,
                );
                let mut frame = QSL::new();
                frame.push(Surface::Tooltip {
                    tooltip: &tooltip,
                    layout: &tip_layout,
                });
                frame.draw(backend);
            }

            // Editor hover popup (rich markdown; `gh` key, diagnostic/
            // annotation/plugin hovers, or mouse dwell). Bounds/link rects/
            // scrollbar geometry are cached for the click + drag handlers
            // (#215), same as `draw.rs::draw_editor_hover_popup` did.
            self.editor_hover_popup_rect.set(None);
            self.editor_hover_link_rects.borrow_mut().clear();
            self.editor_hover_scrollbar.set(None);
            if let Some(ref eh) = screen.editor_hover {
                let anchor_view = eh.anchor_line.saturating_sub(eh.frozen_scroll_top) as f64;
                let vis_col = eh.anchor_col.saturating_sub(eh.frozen_scroll_left) as f64;
                let anchor_x = win_x + gutter_w + vis_col * cw;
                let anchor_y = win_y + anchor_view * lh;
                let (links, rect, sb) = render::editor_hover_popup_paint(
                    backend,
                    eh,
                    anchor_x as f32,
                    anchor_y as f32,
                    win_viewport,
                    &theme,
                    cw as f32,
                    lh as f32,
                );
                self.editor_hover_popup_rect
                    .set(rect.map(|(rx, ry, rw, rh)| (rx as f64, ry as f64, rw as f64, rh as f64)));
                *self.editor_hover_link_rects.borrow_mut() = links
                    .into_iter()
                    .map(|(lx, ly, lw, lh2, url)| {
                        (lx as f64, ly as f64, lw as f64, lh2 as f64, url)
                    })
                    .collect();
                self.editor_hover_scrollbar.set(sb);
            }
        }

        // ── Draw quickfix panel + bottom panel (terminal/debug output) +
        //    debug toolbar (#670) ────────────────────────────────────────────
        // Ported from the dead `src/gtk/draw.rs` path (no live callers since
        // the #540 Relm4->ShellApp migration) onto `render_content` — same
        // class of gap as the editor-popup block above (#669): the
        // `screen.quickfix` / `screen.bottom_tabs` / `screen.debug_toolbar`
        // fields were (and still are) populated by the engine the whole
        // time, only the paint calls were missing. The adapters
        // (`quickfix_to_list_view`, `build_bottom_panel_tab_bar`,
        // `build_terminal_toolbar`, `build_terminal_draw_data`,
        // `debug_output_to_text_display`, `draw_debug_toolbar`) are the same
        // `render::` functions TUI's own `TuiShellApp::render_content`
        // (`shell_app.rs`) already routes through — only the geometry below
        // is GTK-pixel-native, mirroring `compute_editor_layout`'s
        // `unit_h = line_height` convention.
        //
        // Stacking (top to bottom, matching TUI's `bottom_chrome_rects_for_
        // shell_content` v_chunks order exactly): editor | quickfix |
        // terminal/debug-output | debug toolbar | separated-status (not
        // painted here, #592-C) | status bar. `editor_area_h` above
        // (`el.editor_bottom`) already reserves all of this, so these bands
        // sit directly below it with no gap and no overlap with `status_y`.
        let quickfix_y = y + editor_area_h;
        if let Some(ref qf) = screen.quickfix {
            // Scroll-to-selection: reserve one row for the header, then keep
            // the selected item within the remaining visible rows — matches
            // the dead `draw.rs::draw_quickfix_panel`'s behaviour. GTK has no
            // persistent `quickfix_scroll_top` field to update from key
            // events (unlike TUI's `TuiShellApp`), so this recomputes a
            // stateless "keep selection visible" scroll each frame instead.
            let visible_rows = ((el.quickfix_h / lh) as usize).saturating_sub(1);
            let scroll_top = if visible_rows == 0 {
                0
            } else {
                (qf.selected_idx + 1).saturating_sub(visible_rows)
            };
            let mut list = render::quickfix_to_list_view(qf);
            list.scroll_offset = scroll_top;
            backend.draw_list(
                quadraui::Rect::new(x as f32, quickfix_y as f32, w as f32, el.quickfix_h as f32),
                &list,
            );
        }

        let terminal_y = quickfix_y + el.quickfix_h;
        if el.terminal_h > 0.0 {
            engine
                .bottom_panel_geometry
                .replace(Some(crate::core::engine::BottomPanelGeometry {
                    top_y: terminal_y,
                    height: el.terminal_h,
                    toolbar_y: lh,
                    content_y: 2.0 * lh,
                    content_row_h: lh,
                }));
            let tab_bar = render::build_bottom_panel_tab_bar(
                &screen.bottom_tabs.active,
                engine.terminal_open,
                !screen.bottom_tabs.output_lines.is_empty(),
            );
            let hits = backend.draw_tab_bar(
                quadraui::Rect::new(x as f32, terminal_y as f32, w as f32, lh as f32),
                &tab_bar,
                None,
            );
            engine.bottom_tab_bar_hits.replace(Some(hits));
            let content_y = terminal_y + 2.0 * lh;
            let content_h = (el.terminal_h - 2.0 * lh).max(0.0);
            match screen.bottom_tabs.active {
                render::BottomPanelKind::Terminal => {
                    if let Some(ref term_panel) = screen.bottom_tabs.terminal {
                        let toolbar_y = terminal_y + lh;
                        let toolbar_rect =
                            quadraui::Rect::new(x as f32, toolbar_y as f32, w as f32, lh as f32);
                        let hits = match render::build_terminal_toolbar(term_panel, &theme) {
                            render::TerminalToolbar::FindBar(bar) => {
                                let _ = backend.draw_status_bar(toolbar_rect, &bar, None, None);
                                // No raw `pango::Layout` is reachable from this
                                // `&mut dyn Backend`-only signature (same gap
                                // #669's `editor_hover_popup_paint` doc comment
                                // hit), so segment widths are approximated by
                                // char count * `cw` rather than exact glyph
                                // measurement — affects hit-region precision
                                // only, not paint.
                                let sb_layout = bar.layout(w as f32, lh as f32, 16.0, |seg| {
                                    quadraui::StatusSegmentMeasure::new(
                                        seg.text.chars().count() as f32 * cw as f32,
                                    )
                                });
                                crate::core::engine::TerminalToolbarHits::FindBar {
                                    layout: sb_layout,
                                    origin_x: x,
                                }
                            }
                            render::TerminalToolbar::TabStrip(bar) => {
                                let hits = backend.draw_tab_bar(toolbar_rect, &bar, None);
                                crate::core::engine::TerminalToolbarHits::TabStrip(hits)
                            }
                        };
                        engine.terminal_toolbar_hits.replace(Some(hits));

                        if content_h > 0.0 {
                            let visible_rows = (content_h / lh) as usize;
                            let q_area = quadraui::Rect::new(
                                x as f32,
                                content_y as f32,
                                w as f32,
                                content_h as f32,
                            );
                            let td = render::build_terminal_draw_data(
                                term_panel,
                                q_area,
                                cw as f32,
                                lh as f32,
                                visible_rows,
                                Some(6),
                            );
                            engine.terminal_split_layout.replace(td.split);
                            if let Some(split) = &td.split {
                                let left = td.left.as_ref().unwrap();
                                let right = td.right.as_ref().unwrap();
                                backend.draw_terminal(split.left, left);
                                backend.draw_terminal(split.right, right);
                                backend.draw_terminal_divider(quadraui::Rect::new(
                                    split.divider_x,
                                    content_y as f32,
                                    1.0,
                                    content_h as f32,
                                ));
                            } else if let Some(ref term) = td.single {
                                backend.draw_terminal(q_area, term);
                            }
                            let geom =
                                render::terminal_scrollbar_geometry(term_panel, visible_rows);
                            let surface_sb = geom.map(|g| {
                                let sb_w = 6.0;
                                let sb_x = w - sb_w;
                                let thumb_t = g.thumb_top_frac * content_h;
                                let thumb_h = (g.thumb_height_frac * content_h).max(4.0);
                                quadraui::SurfaceScrollbar {
                                    axis: quadraui::ScrollAxis::Vertical,
                                    track_bounds: quadraui::Rect::new(
                                        (x + sb_x) as f32,
                                        content_y as f32,
                                        sb_w as f32,
                                        content_h as f32,
                                    ),
                                    thumb_bounds: quadraui::Rect::new(
                                        (x + sb_x + 1.0) as f32,
                                        (content_y + thumb_t) as f32,
                                        (sb_w - 2.0) as f32,
                                        thumb_h as f32,
                                    ),
                                    total_items: g.total_items,
                                    visible_items: g.visible_items,
                                    scroll_offset: term_panel.scroll_offset,
                                    inverted: true,
                                }
                            });
                            engine
                                .scroll_surfaces
                                .borrow_mut()
                                .push(quadraui::ScrollSurface {
                                    id: quadraui::WidgetId::new("terminal_scrollback"),
                                    bounds: q_area,
                                    scrollbar: surface_sb,
                                });
                        }
                    }
                }
                render::BottomPanelKind::DebugOutput => {
                    if content_h > 0.0 {
                        let td = render::debug_output_to_text_display(
                            &screen.bottom_tabs.output_lines,
                            engine.debug_output_scroll,
                            engine.debug_output_auto_scroll,
                        );
                        let q_rect = quadraui::Rect::new(
                            x as f32,
                            content_y as f32,
                            w as f32,
                            content_h as f32,
                        );
                        let td_layout = backend.text_display_layout(q_rect, &td);
                        backend.draw_text_display(q_rect, &td);
                        let scrollbar = td_layout.scrollbar_bounds.zip(td_layout.thumb_bounds).map(
                            |(track, thumb)| quadraui::SurfaceScrollbar {
                                axis: quadraui::ScrollAxis::Vertical,
                                track_bounds: quadraui::Rect::new(
                                    q_rect.x + track.x,
                                    q_rect.y + track.y,
                                    track.width,
                                    track.height,
                                ),
                                thumb_bounds: quadraui::Rect::new(
                                    q_rect.x + thumb.x,
                                    q_rect.y + thumb.y,
                                    thumb.width,
                                    thumb.height,
                                ),
                                total_items: td.lines.len(),
                                visible_items: td_layout.visible_lines.len(),
                                scroll_offset: td_layout.resolved_scroll_offset,
                                inverted: false,
                            },
                        );
                        engine
                            .scroll_surfaces
                            .borrow_mut()
                            .push(quadraui::ScrollSurface {
                                id: quadraui::WidgetId::new("debug_output"),
                                bounds: q_rect,
                                scrollbar,
                            });
                    }
                }
            }
        } else {
            engine.bottom_panel_geometry.replace(None);
        }

        let debug_toolbar_y = terminal_y + el.terminal_h;
        if screen.debug_toolbar.is_some() {
            let rect = quadraui::Rect::new(x as f32, debug_toolbar_y as f32, w as f32, lh as f32);
            render::draw_debug_toolbar(backend, &engine, rect);
            self.debug_toolbar_y_offset.set(debug_toolbar_y);
            self.debug_toolbar_height.set(lh);
        } else {
            self.debug_toolbar_y_offset.set(0.0);
            self.debug_toolbar_height.set(0.0);
        }

        // ── Draw separated status line (#671) ───────────────────────────────
        // Shown above the terminal/status band when `window_status_line` is
        // on but `status_line_above_terminal` is off (`bp_open` guarded, see
        // `compute_editor_layout`'s `has_separated`). Never had a GTK
        // painter — `screen.separated_status_line` was populated by the
        // engine but nothing drew it (#592's root cause). Reuses the exact
        // `render::window_status_line_to_status_bar` adapter the per-window
        // status bar above (and TUI's `render_window_status_line`) already
        // routes through, so this can't drift from either. `el.editor_bottom`
        // already reserved `el.separated_status_h` of vertical space right
        // here — between the debug toolbar and `status_y` below — so this
        // band sits exactly where `compute_editor_layout` accounted for it.
        let separated_status_y = debug_toolbar_y + el.debug_toolbar_h;
        if let Some(ref status) = screen.separated_status_line {
            let sb_rect = quadraui::Rect::new(
                x as f32,
                separated_status_y as f32,
                w as f32,
                el.separated_status_h as f32,
            );
            let bar = render::window_status_line_to_status_bar(
                status,
                quadraui::WidgetId::new("status:separated"),
            );
            let mut frame = QSL::new();
            frame.push(Surface::StatusBar {
                rect: sb_rect,
                bar: &bar,
                hovered: None,
                pressed: None,
            });
            frame.draw(backend);

            // #672: same segment hit-zone recovery as the per-window status
            // bar above, keyed by `active_window_id` — the separated line
            // shows the active window's status, so that's the id
            // `pixel_to_click_target` looks its zones up under, matching the
            // dead `draw.rs::draw_window_status_bar` call site this replaces.
            let sb_layout = backend.status_bar_layout(sb_rect, &bar);
            self.status_segment_map.borrow_mut().insert(
                screen.active_window_id.0,
                render::status_bar_zones_from_layout(&sb_layout),
            );
            self.separated_status_bar_rect.set(Some(sb_rect));
        } else {
            self.separated_status_bar_rect.set(None);
        }

        // ── Draw global status bar / wildmenu ─────────────────────────────────
        // Simplified to `h - status_bar_h`: quickfix/terminal/debug-toolbar/
        // separated-status all stack *above* this point now (see the
        // quickfix/bottom-panel/debug-toolbar block above), so the status
        // bar's own band no longer needs to re-subtract them.
        let status_y = y + h - status_bar_h;
        // #752: publish the painted rect for `route_chrome_click`, the twin of
        // TUI's `render_impl.rs` call site. The bespoke branch hit-test this
        // replaces re-derived the band from `height - lh * rows - wildmenu_px`
        // in the click handler — a second copy of the arithmetic three lines
        // above, and one that had no way to know what was really drawn.
        if let Some(ref bar) = screen.global_status_bar {
            let sb_rect = quadraui::Rect::new(x as f32, status_y as f32, w as f32, lh as f32);
            self.engine.borrow().global_status_rect.set(sb_rect);
            // Same zone recovery as the per-window and separated bars above.
            *self.global_status_zones.borrow_mut() =
                render::status_bar_zones_from_layout(&backend.status_bar_layout(sb_rect, bar));
            let mut frame = QSL::new();
            frame.push(Surface::StatusBar {
                rect: sb_rect,
                bar,
                hovered: None,
                pressed: None,
            });
            frame.draw(backend);
        } else {
            self.engine
                .borrow()
                .global_status_rect
                .set(quadraui::Rect::default());
            self.global_status_zones.borrow_mut().clear();
        }
        if let Some(ref wm) = screen.wildmenu {
            let wm_bar = render::wildmenu_to_status_bar(wm, &theme);
            let wm_y = if per_window_status {
                status_y
            } else {
                status_y + lh
            };
            let wm_rect = quadraui::Rect::new(x as f32, wm_y as f32, w as f32, lh as f32);
            let mut frame = QSL::new();
            frame.push(Surface::StatusBar {
                rect: wm_rect,
                bar: &wm_bar,
                hovered: None,
                pressed: None,
            });
            frame.draw(backend);
        }

        // ── Draw command line ─────────────────────────────────────────────────
        let cmd_y = status_y + (status_bar_h - lh);
        let cmd = quadraui::CommandLine {
            id: "cmd".into(),
            text: screen.command.text.clone(),
            cursor_offset: if screen.command.show_cursor {
                Some(screen.command.cursor_anchor_text.len())
            } else {
                None
            },
            right_align: screen.command.right_align,
        };
        let cmd_rect = quadraui::Rect::new(x as f32, cmd_y as f32, w as f32, lh as f32);
        let mut frame = QSL::new();
        frame.push(Surface::CommandLine {
            rect: cmd_rect,
            cmd: &cmd,
        });
        frame.draw(backend);

        // ── Draw sidebar panel content ─────────────────────────────────────────
        // The quadraui AppShell chrome (activity bar + sidebar header) is rendered
        // by the runner; we fill only the content area it exposes.
        self.painted_sidebar_bounds
            .set(layout.sidebar_content_bounds);
        if let Some(q_sb) = layout.sidebar_content_bounds {
            // Which panel is active?  Extension panels bypass AppShell.
            let active_id: String = if let Some(ref name) = engine.ext_panel_active {
                format!("ext:{name}")
            } else {
                engine
                    .app_shell
                    .active_panel_id()
                    .map(|id| id.as_str().to_string())
                    .unwrap_or_else(|| PANEL_EXPLORER.to_string())
            };

            match active_id.as_str() {
                PANEL_EXPLORER => {
                    render::populate_explorer_tree_controller(&engine, &theme);
                    // Capture the exact metrics the tree is drawn with so the
                    // click hit-test (which reads the backend's mutable
                    // current_line_height at a later, possibly-different time) can
                    // re-apply them and resolve the correct row. (#540)
                    self.cached_explorer_metrics
                        .set((backend.line_height() as f64, backend.char_width() as f64));
                    engine.explorer_tree_rect.set(q_sb);
                    engine.explorer_viewport_rows.set(q_sb.height as usize);
                    engine.explorer_tree.borrow().render(backend, q_sb);
                }
                PANEL_SEARCH => {
                    render::populate_search_sidebar_system(&engine, &engine.cwd);
                    engine.search_sidebar_body_rect.set(q_sb);
                    engine.search_sidebar_system.borrow().render(backend, q_sb);
                }
                PANEL_DEBUG => {
                    let (title_bar, action_bar) =
                        render::debug_sidebar_chrome_to_status_bars(&screen.debug_sidebar, &theme);
                    let title_rect = quadraui::Rect::new(q_sb.x, q_sb.y, q_sb.width, lh as f32);
                    let action_rect =
                        quadraui::Rect::new(q_sb.x, q_sb.y + lh as f32, q_sb.width, lh as f32);
                    let body_y = q_sb.y + 2.0 * lh as f32;
                    let body_h = (q_sb.height - 2.0 * lh as f32).max(0.0);
                    let body_rect = quadraui::Rect::new(q_sb.x, body_y, q_sb.width, body_h);
                    let _ = backend.draw_status_bar(title_rect, &title_bar, None, None);
                    let hits = backend.draw_status_bar(action_rect, &action_bar, None, None);
                    engine.dap_sidebar_action_hits.replace(Some(hits));
                    // `hits` are relative to `action_rect`'s origin; the click
                    // router needs the rect to translate into that space (#544).
                    self.cached_dap_action_rect.set(Some(action_rect));
                    engine.dap_sidebar_body_rect.set(body_rect);
                    render::populate_dap_sidebar_system(&engine);
                    engine
                        .dap_sidebar_system
                        .borrow()
                        .render(backend, body_rect);
                }
                PANEL_GIT => {
                    if let Some(ref sc) = screen.source_control {
                        // Header row + commit-input box (#480). Previously
                        // entirely unpainted under ShellApp — the only place
                        // that ever drew them was the dead
                        // `draw.rs::draw_source_control_panel` Cairo painter,
                        // which has zero live callers (superseded by this
                        // `render_content` path back when the 14 legacy DAs
                        // were collapsed into one, #493). Paint them for
                        // real now that quadraui#222 (TextInput) has landed,
                        // through the same `render::sc_*` adapters TUI uses
                        // so the two renderers can't drift.
                        // Band geometry (header / commit box / slab) comes from
                        // the shared `render::sc_sidebar_bands` so the click
                        // router in `try_route_sidebar_mouse_event` resolves a
                        // press against the *same* derivation that painted it
                        // (#544). `SC_COMMIT_BORDER_PX` is the primitive's 1px
                        // border top+bottom — GTK's native unit is pixels,
                        // unlike TUI's whole-cell border (see
                        // `render::sc_commit_input_box_height` doc).
                        let bands = render::sc_sidebar_bands(
                            &sc.commit_message,
                            q_sb,
                            lh as f32,
                            SC_COMMIT_BORDER_PX,
                        );
                        self.cached_sc_bands.set(Some(bands));
                        let header_bar = render::sc_header_status_bar(sc, &theme);
                        let _ = backend.draw_status_bar(bands.header, &header_bar, None, None);

                        let ti = render::sc_commit_message_to_text_input(sc);
                        backend.draw_text_input(bands.commit_input, &ti);

                        // Render the toolbar-slab + section list below the
                        // header + commit input.
                        let slab_rect = bands.slab;
                        render::draw_sc_sidebar_panel(backend, &engine, sc, slab_rect);
                        let body_rect = engine
                            .sc_panel_layout
                            .borrow()
                            .as_ref()
                            .map(|l| l.content_bounds)
                            .unwrap_or(slab_rect);
                        engine.sc_sidebar_body_rect.set(body_rect);
                        render::populate_sc_sidebar_system(&engine, &theme);
                        engine.sc_sidebar_system.borrow().render(backend, body_rect);

                        // Branch picker / create popup (dual-mode Palette,
                        // quadraui#224) and help dialog (Dialog + DialogTable,
                        // quadraui#225) — both keyboard-reachable via
                        // `dispatch_sc_sidebar_key_unified` even though the
                        // git sidebar has no live mouse-click routing yet
                        // (#449 tracks that separately). Render over the
                        // whole sidebar content area, same popup-over-panel
                        // z-order TUI uses.
                        if let Some(ref bp) = sc.branch_picker {
                            let palette = render::sc_branch_picker_to_palette(bp);
                            let popup_w = q_sb.width.min(40.0 * cw as f32);
                            let popup_h = if bp.create_mode {
                                4.0 * lh as f32
                            } else {
                                (q_sb.height * 0.6).min(15.0 * lh as f32)
                            };
                            let popup_x = q_sb.x + (q_sb.width - popup_w) / 2.0;
                            let popup_y = q_sb.y + 2.0 * lh as f32;
                            backend.draw_palette(
                                quadraui::Rect::new(popup_x, popup_y, popup_w, popup_h),
                                &palette,
                            );
                        }

                        if sc.help_open {
                            let viewport = q_sb;
                            let (dialog, dlayout) =
                                render::sc_help_dialog_layout(viewport, cw as f32, lh as f32);
                            backend.draw_dialog(&dialog, &dlayout);
                        }
                    } else {
                        // Git panel is the active tab but there's no repo open
                        // (e.g. the user closed it, or switched to a non-git
                        // folder, without also switching sidebar tabs) — nothing
                        // paints this frame. Clear the cached band geometry so a
                        // stray click doesn't get resolved against stale
                        // coordinates from the last time a repo *was* open
                        // (`route_sc_sidebar_event` reads this cache directly).
                        self.cached_sc_bands.set(None);
                    }
                }
                PANEL_EXTENSIONS => {
                    render::populate_ext_sidebar_system(&engine);
                    engine.ext_sidebar_body_rect.set(q_sb);
                    engine.ext_sidebar_system.borrow().render(backend, q_sb);
                }
                PANEL_SETTINGS => {
                    render::populate_settings_form_controller(&engine);
                    engine
                        .settings_form_controller
                        .borrow_mut()
                        .render_and_cache(backend, q_sb);
                }
                id if id.starts_with("ext:") => {
                    // Extension panel — render via ext_sidebar_system.
                    render::populate_ext_sidebar_system(&engine);
                    engine.ext_sidebar_body_rect.set(q_sb);
                    engine.ext_sidebar_system.borrow().render(backend, q_sb);
                }
                PANEL_AI => {
                    // #730: the last of #592's 14 `ScreenLayout` fields,
                    // and the straggler #670 deferred. Paints through the
                    // same `render::draw_ai_sidebar_panel` builder TUI's
                    // `render_ai_sidebar` calls — `unit_w`/`unit_h` here are
                    // the pixel `char_width`/`line_height` GTK's other
                    // row-based panels (PANEL_GIT, PANEL_DEBUG) already
                    // convert through, unlike TUI's cell-native `1.0, 1.0`.
                    // Bands are cached for `route_ai_sidebar_event` so the
                    // click and paint derivations cannot drift (#544).
                    if let Some(ref ai) = screen.ai_panel {
                        let bands = render::draw_ai_sidebar_panel(
                            backend, q_sb, ai, &theme, cw as f32, lh as f32,
                        );
                        self.cached_ai_bands.set(Some(bands));
                    } else {
                        self.cached_ai_bands.set(None);
                    }
                }
                _ => {
                    // Unknown panel id: nothing painted, nothing to route a
                    // click to.
                    self.cached_ai_bands.set(None);
                }
            }

            // ── Sidebar-item hover popup (#670) ─────────────────────────────
            // Source-control / extension-panel item dwell tooltip, rendered
            // markdown. Ported from the dead `draw.rs::draw_panel_hover_popup`
            // (raw Cairo/Pango, no live callers) onto the shared
            // `quadraui::RichTextPopup` path via `render::panel_hover_popup_
            // paint` — the same primitive TUI's `render_panel_hover_popup`
            // already uses. Paints after the panel body above (same
            // paint-after-content z-order the dead code used), clamped
            // against the full content viewport so it can extend rightward
            // into the editor area past the sidebar's own bounds.
            self.panel_hover_popup_rect.set(None);
            self.panel_hover_link_rects.borrow_mut().clear();
            if screen.panel_hover.is_some() {
                let hover_viewport = quadraui::Rect::new(x as f32, y as f32, w as f32, h as f32);
                let (links, rect) = render::panel_hover_popup_paint(
                    backend,
                    screen,
                    &theme,
                    q_sb.x + q_sb.width,
                    q_sb.y,
                    hover_viewport,
                    cw as f32,
                    lh as f32,
                );
                self.panel_hover_popup_rect
                    .set(rect.map(|(rx, ry, rw, rh)| (rx as f64, ry as f64, rw as f64, rh as f64)));
                *self.panel_hover_link_rects.borrow_mut() = links
                    .into_iter()
                    .map(|(lx, ly, lw, lh2, url, is_native)| {
                        (lx as f64, ly as f64, lw as f64, lh2 as f64, url, is_native)
                    })
                    .collect();
            }
        }

        // ── Cache per-group tab-drop geometry ─────────────────────────────────
        // Compute the absolute drop-group bounds from the shared screen layout and
        // stash them so the drag hit-test (handle_mouse_drag_msg) and the overlay
        // below use one identical source.
        //
        // Origin convention: `gtb.bounds` are always absolute (built from absolute
        // window rects), so there is no origin offset to apply — adding (x,y)
        // again would double-count it and shift the highlight off the group (the
        // prior "covers half the group" bug, #515). This used to branch on
        // `editor_group_split.is_some()` because the single-group arm of
        // `screen_to_drop_group_bounds` derived its rect from a caller-supplied
        // origin/size instead; `group_tab_bars` now covers one group too, so both
        // the branch and the parameters it fed are gone (#551).
        {
            let bounds = render::screen_to_drop_group_bounds(screen);
            // Per-tab slot x-positions (absolute) were captured while drawing the
            // tab bars above. Feeding them here makes a drag inside a group's own
            // tab bar resolve to a `TabReorder` (insertion bar) instead of falling
            // through to a new-split/center overlay. (#515)
            let slots_abs = self.cached_tab_slots_abs.borrow();
            let (groups, eff_tbh) =
                render::build_tab_drop_groups(&bounds, &engine, tab_bar_h as f32, &slots_abs);
            drop(slots_abs);
            *self.cached_drop_groups.borrow_mut() = groups;
            self.cached_drop_tbh.set(eff_tbh);
        }

        // ── Draw tab drag overlay ─────────────────────────────────────────────
        // When a tab drag is in progress, paint the drop-zone highlight + insertion
        // bar on top of all other content, using the geometry cached just above.
        if self.tab_drag.is_dragging() {
            let groups = self.cached_drop_groups.borrow();
            let eff_tbh = self.cached_drop_tbh.get();
            let (mx, my) = self.mouse_pos_cell.get();
            if let Some(ov) = render::compute_tab_drop_overlay(
                self.tab_drag.zone(),
                &groups,
                (mx as f32, my as f32),
                eff_tbh,
                2.0,
                lh as f32,
            ) {
                backend.draw_drop_overlay(&quadraui::DropOverlay {
                    highlight: ov.highlight,
                    insertion_bar: ov.insertion_bar,
                    ghost_position: Some(ov.ghost_position),
                });
            }
        }

        // ══ Overlay band (#735 slice 1) ══════════════════════════════════════
        //
        // Everything from here to the end of `render_content` is composed from
        // `render::OVERLAY_Z_ORDER` — the single ordered artefact both backends
        // walk, replacing the two hand-kept transcriptions that had already
        // inverted twice against each other (see that constant's own comment
        // for the two inversions and which order won). Geometry and
        // rasterisation stay here, in pixels, because that is the genuine
        // per-backend difference; only the *order* moved.
        //
        // Every arm gates itself and, when it paints, records the rung it
        // painted — naming the variant explicitly (`push(OverlayOp::Dialog)`),
        // never `push(op)`. That distinction is the difference between a test
        // that can fail and one that cannot: with `push(op)` the record follows
        // the *pattern* the walk is currently at, so swapping two arms' bodies
        // paints them in the wrong order while still recording the right one.
        // Naming the variant makes the record describe what was drawn, so
        // `check_overlay_band_order` (and the black-box tests reading this
        // field) catch that swap.
        //
        // Arms whose surface is absent still run — several own
        // a hit-test cache (`picker_popup_rect`, `dialog_layout`,
        // `context_menu_layout`, `tab_switcher_popup_rect`,
        // `command_center_layout`, `toast_layout`) that must be *cleared* on
        // the frame the surface disappears, or the next click resolves against
        // last frame's geometry (the #587 class of bug).
        //
        // Not part of the shared band, and deliberately left above it: the tab
        // drag overlay (TUI paints its drag ghost in the editor band instead)
        // and — see the `MenuDropdown` arm — the app-icon slot and inline
        // window controls, neither of which TUI has at all.
        let popup_vp = backend.viewport();
        let popup_viewport = quadraui::Rect::new(0.0, 0.0, popup_vp.width, popup_vp.height);
        let mut painted_band: Vec<render::OverlayOp> = Vec::new();

        for op in render::OVERLAY_Z_ORDER {
            match op {
                // ── Menu dropdown overlay ────────────────────────────────────
                // First rung of the band: `MenuSystem::render` repaints
                // `draw_menu_bar` across the whole title-bar strip, so nothing
                // that wants to survive may be drawn into that band before it.
                // #735 moved the *modal* rungs above it (they used to paint
                // underneath on GTK and on top on TUI) — a modal dialog now
                // covers an open dropdown on both backends, matching
                // `route_modal_overlay_click`'s own "a dialog eats everything"
                // arbitration.
                render::OverlayOp::MenuDropdown => {
                    if !engine.menu_bar_visible {
                        continue;
                    }
                    self.paint_title_bar_band(
                        backend,
                        &engine,
                        &theme,
                        menu_row_rect,
                        menu_items_rect,
                        app_icon_rect,
                        controls_rect,
                    );
                    painted_band.push(render::OverlayOp::MenuDropdown);
                }

                // ── Command Center: nav arrows + search box (#676) ────────────
                // Painted *after* `menu_system.render()` above, which repaints
                // `draw_menu_bar` across the entire `menu_row_rect` band and
                // would erase anything drawn here first — the identical
                // ordering hazard documented on the window controls (#552
                // round-2/3 "buttons render blank"). This is the VS Code-style
                // Command Center dropped by the #540 Relm4→ShellApp cutover and
                // never re-wired: it used to live in the deleted `impl
                // SimpleComponent for App` `view!` scaffolding. Cached into
                // `engine.command_center_layout` for `handle()`'s click
                // hit-test, mirroring TUI's `shell_app.rs` (#635 Stage 6b item
                // A) and `mouse.rs`'s "Menu bar row click — command center
                // only".
                render::OverlayOp::CommandCenter => {
                    if let Some(cc_rect) = command_center_rect.filter(|r| r.width >= 1.0) {
                        let title = engine
                            .cwd
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "VimCode".to_string());
                        let cc = render::build_command_center_view(
                            engine.tab_nav_can_go_back(),
                            engine.tab_nav_can_go_forward(),
                            &title,
                        );
                        let cc_layout = backend.draw_command_center(cc_rect, &cc);
                        engine.command_center_layout.replace(Some(cc_layout));
                        painted_band.push(render::OverlayOp::CommandCenter);
                    } else {
                        engine.command_center_layout.replace(None);
                    }
                }

                // ── Find/replace overlay (#671) ──────────────────────────────
                // Confirmed by #592 to open in engine state (`KEYDBG: OVERLAY
                // STATE OPEN: ["find_replace"]`) with nothing painting on GTK.
                // Unlike quickfix/panel_hover (#670) there *was* a dead painter
                // to port — `draw.rs::draw_find_replace_popup` — but it routed
                // through `Surface::FindReplace` with a rect the rasteriser
                // ignores; calling `Backend::draw_find_replace` directly (same
                // trait method TUI's `TuiShellApp::render_content` calls) is
                // simpler and identical in effect. The GTK rasteriser positions
                // the panel from its own `panel.group_bounds` (already absolute
                // pixel coordinates — #550, same as TUI's absolute cell
                // coordinates) and reads `current_line_height` /
                // `current_char_width` off the backend (set once per frame by
                // quadraui's GTK runner before `render_content` runs), so the
                // `rect` argument here is unused by the GTK rasteriser too;
                // passed for parity with the trait's signature and the TUI call
                // site.
                render::OverlayOp::FindReplace => {
                    if let Some(ref find_replace) = screen.find_replace {
                        backend.draw_find_replace(popup_viewport, find_replace);
                        painted_band.push(render::OverlayOp::FindReplace);
                    }
                }

                // ── Picker / command-palette overlay (#587) ──────────────────
                // Same class of bug #546 fixed for dialog/context-menu: the
                // palette was painted only by the dead legacy `draw_editor`
                // Cairo path (`draw.rs::draw_picker_popup`), which has zero live
                // callers under ShellApp. So `Ctrl+Shift+P` opened the picker in
                // engine state (`picker_open = true`, items populated) but
                // nothing ever painted — the "command palette fails to open
                // silently" symptom. Geometry comes from the same generic
                // helpers the legacy path used (`PickerGeometry` +
                // `gtk_picker_sizing`), so no Pango/Cairo access is needed here.
                render::OverlayOp::UnifiedPicker => {
                    if let Some(ref picker) = screen.picker {
                        let has_preview = picker.preview.is_some();
                        let geo = render::PickerGeometry::compute(
                            popup_vp.width,
                            popup_vp.height,
                            has_preview,
                            &render::gtk_picker_sizing(lh as f32),
                        );
                        let palette = render::picker_panel_to_palette(picker);
                        let mut frame = QSL::new();
                        frame.push(Surface::Palette {
                            rect: quadraui::Rect::new(
                                geo.popup_x,
                                geo.popup_y,
                                geo.popup_w,
                                geo.popup_h,
                            ),
                            palette: &palette,
                        });
                        frame.draw(backend);
                        // Hand the *painted* rect to the click/drag handlers (#555).
                        self.picker_popup_rect.set(Some((
                            geo.popup_x as f64,
                            geo.popup_y as f64,
                            geo.popup_w as f64,
                            geo.popup_h as f64,
                        )));
                        painted_band.push(render::OverlayOp::UnifiedPicker);
                    } else {
                        self.picker_popup_rect.set(None);
                    }
                }

                // ── Tab switcher popup (Ctrl+Tab MRU list) (#671) ────────────
                // `self.tab_switcher_popup_rect` already exists and is read by
                // `handle_mouse_press`'s "Tab switcher modal arbitration" block
                // (added ahead of this painter, expecting to be fed) — this is
                // the first frame that actually sets it. Sizing/positioning
                // ported from the dead `draw.rs::draw_tab_switcher_popup_list`
                // (pixel-tuned clamp(350, 600) width, unlike TUI's
                // percent-of-terminal-columns sizing, which wouldn't make sense
                // in pixel space); content comes from the same shared
                // `render::tab_switcher_to_quadraui_list_view` adapter TUI's
                // `TuiShellApp::render_content` uses, through
                // `Backend::draw_list`.
                render::OverlayOp::TabSwitcher => {
                    self.tab_switcher_popup_rect.set(None);
                    if let Some(ref ts) = screen.tab_switcher {
                        // #733: geometry comes from the shared
                        // `TabSwitcherGeometry` so the rect handed to
                        // `route_modal_overlay_click` below is the rect that was
                        // painted, and TUI resolves the identical popup through
                        // the same code with its own sizing constant.
                        if let Some(geo) = render::TabSwitcherGeometry::compute(
                            popup_viewport,
                            ts.items.len(),
                            &render::gtk_tab_switcher_sizing(lh as f32),
                        ) {
                            let list =
                                render::tab_switcher_to_quadraui_list_view(ts, geo.visible_rows);
                            backend.draw_list(geo.bounds, &list);
                            self.tab_switcher_popup_rect.set(Some((
                                geo.bounds.x as f64,
                                geo.bounds.y as f64,
                                geo.bounds.width as f64,
                                geo.bounds.height as f64,
                            )));
                            painted_band.push(render::OverlayOp::TabSwitcher);
                        }
                    }
                }

                // ── Context menu (#546) ──────────────────────────────────────
                // The ShellApp render path never painted `screen.context_menu`
                // at all — its draw + click-geometry cache was populated only by
                // the dead legacy `draw_editor` Cairo path (src/gtk/draw.rs),
                // which has zero live callers under ShellApp, leaving right-click
                // menus invisible and unclickable. Drawn with only generic
                // `Backend` metrics (`render::context_menu_generic_layout`,
                // shared with TUI) since this fn has no raw Pango/Cairo access.
                render::OverlayOp::ContextMenu => {
                    match screen.context_menu.as_ref().filter(|p| !p.items.is_empty()) {
                        Some(panel) => {
                            let (menu, mlayout) = render::context_menu_generic_layout(
                                panel,
                                popup_viewport,
                                cw,
                                lh,
                                0.0,
                            );
                            let mut frame = QSL::new();
                            frame.push(Surface::ContextMenu {
                                menu: &menu,
                                layout: &mlayout,
                            });
                            frame.draw(backend);
                            *self.context_menu_layout.borrow_mut() = Some(mlayout);
                            painted_band.push(render::OverlayOp::ContextMenu);
                        }
                        None => *self.context_menu_layout.borrow_mut() = None,
                    }
                }

                // ── Modal dialog (#546) ──────────────────────────────────────
                // Same #546 story as the context menu above: invisible AND
                // undismissable by mouse under ShellApp — `dialog.is_some()`
                // stayed true forever and `handle_mouse_click_msg`'s dialog block
                // swallowed all subsequent clicks. #735 moved it *above* the
                // context menu (it used to paint underneath on GTK, and on top
                // on TUI): a dialog is the surface `route_modal_overlay_click`
                // hands every event to, so it must also be the surface the user
                // can see.
                //
                // #727: a natively-expressible `screen.dialog` (no `DialogTable`,
                // no text input — `quadraui::native_dialog_options` is the single
                // source of truth for that split, no hand-maintained tag list
                // here) goes through a real OS `AlertDialog` instead of this
                // in-canvas primitive. Unlike this primitive, which is happily
                // repainted every frame, a native dialog must be presented
                // exactly once per open — `native_dialog_shown` is the
                // edge-trigger: the first `render_content` call to see a given
                // open queues the present (via `pending_native_dialog`, drained
                // by `tick()` since the blocking `PlatformServices` call can't
                // run from inside this paint callback, mirroring
                // `PendingFileDialog` #572) and flips the flag; every subsequent
                // call before the dialog closes just suppresses the in-canvas
                // draw without re-queuing. A native dialog is *not* recorded in
                // `painted_band`: nothing was composed into this frame.
                render::OverlayOp::Dialog => {
                    match screen
                        .dialog
                        .as_ref()
                        .map(render::dialog_panel_to_quadraui_dialog)
                    {
                        Some(dialog) => match quadraui::native_dialog_options(&dialog) {
                            Some(opts) => {
                                if !self.native_dialog_shown.get() {
                                    self.native_dialog_shown.set(true);
                                    self.pending_native_dialog.set(Some(opts));
                                }
                                *self.dialog_layout.borrow_mut() = None;
                            }
                            None => {
                                // Carries a `DialogTable` or text input (e.g. the
                                // SSH-passphrase prompt) — no native alert
                                // facility hosts either, so this stays in-canvas
                                // exactly as before.
                                let panel =
                                    screen.dialog.as_ref().expect("just matched Some above");
                                let (dialog, dlayout) =
                                    render::dialog_generic_layout(panel, popup_viewport, cw, lh);
                                let mut frame = QSL::new();
                                frame.push(Surface::Dialog {
                                    dialog: &dialog,
                                    layout: &dlayout,
                                });
                                frame.draw(backend);
                                *self.dialog_layout.borrow_mut() = Some(dlayout);
                                painted_band.push(render::OverlayOp::Dialog);
                            }
                        },
                        None => {
                            self.native_dialog_shown.set(false);
                            *self.dialog_layout.borrow_mut() = None;
                        }
                    }
                }

                // ── Toast overlay (#454) — top of the band ───────────────────
                // Anchored to the full window viewport (matches TUI's
                // `layout.window_bounds`), not just `main_content_bounds`, so it
                // sits in the bottom-right corner of the whole app like the
                // TUI/VSCode toasts. `Backend::draw_toast_stack` does its own
                // pango measurement internally (unlike `dialog`/`context_menu`
                // above, whose generic layout is computed vimcode-side), so its
                // returned layout is the only source of truth — cached for
                // `handle_mouse_click_msg`'s hit-test → `handle_toast_hit`
                // dispatch, and the first rung `route_modal_overlay_click`
                // arbitrates.
                render::OverlayOp::ToastStack => {
                    if let Some(stack) = render::build_toast_stack(&engine) {
                        let toast_layout = backend.draw_toast_stack(popup_viewport, &stack);
                        engine.toast_layout.replace(Some(toast_layout));
                        painted_band.push(render::OverlayOp::ToastStack);
                    } else {
                        engine.toast_layout.replace(None);
                    }
                }
            }
        }

        *self.painted_overlay_band.borrow_mut() = painted_band;
        // Read back through the field rather than the local, so the *stored*
        // observable is what gets validated — a frame that recorded one thing
        // and painted another would be a lie the tests then trusted.
        if let Err(why) = render::check_overlay_band_order(&self.painted_overlay_band.borrow()) {
            debug_assert!(false, "GTK {why}");
        }
    }

    fn handle(
        &mut self,
        event: quadraui::UiEvent,
        backend: &mut dyn quadraui::Backend,
        ctx: &quadraui::ShellContext<'_>,
    ) -> quadraui::Reaction {
        use quadraui::{Key, MouseButton, NamedKey, UiEvent};

        // ── Menu system intercept (#552) ─────────────────────────────────────
        // GTK's menu bar is always visible (see `ShellApp::setup`) and its
        // dropdown overlay must intercept keys/clicks before the sidebar or
        // editor sees them — same precedence TUI uses (mod.rs "MenuSystem
        // intercept" block) via the identical shared `menu_system.handle()`.
        let (menu_bar_visible, menu_system) = {
            let eng = self.engine.borrow();
            (eng.menu_bar_visible, eng.menu_system.clone())
        };
        if menu_bar_visible || menu_system.borrow().is_open() {
            // `menu_items_rect`, not `menu_row_rect` (#720): the app icon
            // occupies a leading slot, so the items the last frame *painted*
            // start one slot right of the band's left edge. Hit-testing
            // against the full band would resolve a click on `File` to
            // whatever label now sits a slot to its left. `render_content`
            // writes this from the same `split_menu_row_for_app_icon` call
            // that positions the paint.
            let bar_rect = self.menu_items_rect.get();
            let menu_event = menu_system.borrow_mut().handle(&event, backend, bar_rect);
            match menu_event {
                quadraui::MenuEvent::Activated(id) => {
                    self.handle_menu_action(id.as_str().to_string());
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
                quadraui::MenuEvent::StateChanged | quadraui::MenuEvent::Consumed => {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
                quadraui::MenuEvent::Ignored => {}
            }
        }

        // ── Command Center: nav arrows + search box (#676) ────────────────────
        // Checked before the window-control buttons below and the CSD
        // titlebar drag-to-move fallback further down, so a click in the
        // command center (which sits inside the title-bar band the
        // drag-to-move check would otherwise claim) routes to tab-nav / the
        // picker instead of starting a window drag. Mirrors TUI's
        // `mouse.rs` "Menu bar row click — command center only" precedence.
        // The nav-arrow / search-box actions are the shared
        // `render::apply_command_center_hit` (#752) — the pre-#540 Relm4
        // `Msg::MruNavBack` / `MruNavForward` / `OpenCommandCenter` variants
        // for this exact action, already wired end-to-end but never
        // dispatched from anywhere since the cutover. This block was their
        // first live caller (#676); #732 turned them into plain methods,
        // and #752 converged those methods with TUI's identical match arm
        // into the one function in `render.rs`.
        if let UiEvent::MouseDown {
            button: MouseButton::Left,
            position,
            ..
        } = &event
        {
            let cc_hit = self
                .engine
                .borrow()
                .command_center_layout
                .borrow()
                .as_ref()
                .map(|l| l.hit_test(position.x, position.y));
            // `Bar` (command-center background, not an interactive segment)
            // and `Outside`/`None` fall through so the drag-to-move fallback
            // below still works for genuine empty-band clicks.
            if let Some(hit) = cc_hit {
                if crate::render::apply_command_center_hit(&mut self.engine.borrow_mut(), hit) {
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
            }
        }

        // ── Inline window-control buttons: minimize/maximize/close (#552) ───
        // Shared `StatusBarInteraction` hover/press/click tracker — the same
        // primitive quadraui's own `full_chrome_demo` reference title bar
        // uses (quadraui#402) — instead of a hand-rolled `StatusBarHit`
        // lookup. Gets the buttons real hover/press highlighting for free
        // and click-on-release semantics (a press that drags off the button
        // before release no longer fires it), matching native window
        // controls. Runs on every event (not just MouseDown) so hover state
        // updates as the pointer moves.
        {
            let rect = self.title_bar_rect.get();
            if rect.width > 0.0 {
                let action = self.title_bar_interaction.borrow_mut().handle(&event, rect);
                match action {
                    quadraui::StatusBarAction::Clicked(id) => {
                        match id.as_str() {
                            render::WINDOW_MINIMIZE_ACTION => self.window_minimize(),
                            render::WINDOW_MAXIMIZE_ACTION => self.window_toggle_maximize(),
                            render::WINDOW_CLOSE_ACTION => self.window_close(),
                            _ => {}
                        }
                        self.draw_needed.set(true);
                        return quadraui::Reaction::Redraw;
                    }
                    quadraui::StatusBarAction::Redraw => {
                        self.draw_needed.set(true);
                        return quadraui::Reaction::Redraw;
                    }
                    quadraui::StatusBarAction::Ignored => {}
                }
            }
        }

        // ── Outer window border: edge-resize cursor hint (quadraui#406) ──
        // Pure side effect on hover — hint the resize pointer over the outer
        // window border, default everywhere else (including the non-resizable
        // full-width CSD title bar, which owns the top edge). Falls through so
        // the editor/sidebar hover handling below still runs. Mirrors
        // `full_chrome_demo`'s `MouseMoved` arm. GTK-only; TUI `set_cursor`
        // is a documented no-op.
        if let UiEvent::MouseMoved { position, .. } = &event {
            let shape = if ctx.in_title_bar(position.x, position.y) {
                quadraui::PointerShape::Default
            } else {
                match ctx.window_edge(position.x, position.y, backend.line_height()) {
                    Some(edge) => quadraui::PointerShape::Resize(edge),
                    None => quadraui::PointerShape::Default,
                }
            };
            backend.set_cursor(shape);
        }

        // ── Sidebar hover — #754 rung ─────────────────────────────────────
        // This backend already *painted* `screen.panel_hover` (the
        // `RichTextPopup` block in `render_content`) and already tracked the
        // popup's own rect, but nothing on this side ever set
        // `engine.panel_hover` or `engine.sc_button_hovered`: the router was
        // ~78 lines of TUI-only code. That is the #499/#484 mechanism — paint
        // without input on one backend, input without a second painter on the
        // other. `render::route_sidebar_hover` is now the single router and
        // both backends call it.
        if let UiEvent::MouseMoved { position, .. } = &event {
            if let Some(sb) = ctx.layout.sidebar_content_bounds {
                let lh = backend.line_height();
                let on_popup = self
                    .panel_hover_popup_rect
                    .get()
                    .is_some_and(|(px, py, pw, ph)| {
                        let (mx, my) = (position.x as f64, position.y as f64);
                        mx >= px && mx < px + pw && my >= py && my < py + ph
                    });
                let owner = render::sidebar_owner(&self.engine.borrow());
                let changed = render::route_sidebar_hover(
                    &mut self.engine.borrow_mut(),
                    &owner,
                    position.x,
                    position.y,
                    render::SidebarBodyGeometry {
                        bounds: sb,
                        row_h: lh.max(1.0),
                        header_rows: 1.0,
                    },
                    true,
                    on_popup,
                );
                if changed {
                    self.draw_needed.set(true);
                }
            }
        }

        // ── CSD titlebar background: drag-to-move / double-click-maximize ──
        // (quadraui#400) + outer window border: edge-resize (quadraui#406).
        // Runs after the menu-item intercept and the window-control-button
        // check above, so both take priority — only a press/double-click that
        // lands in the title bar band but misses every interactive segment
        // (menu item, min/max/close button) reaches here, matching
        // `Backend::begin_window_drag`'s documented contract. The title bar
        // takes priority over the top window edge (a full-width CSD header
        // owns it), so `in_title_bar` is checked before `window_edge` —
        // mirrors quadraui's `full_chrome_demo` reference. TUI has no window,
        // so `begin_window_drag`/`begin_window_resize`/`toggle_window_maximize`
        // are all documented no-ops there; this path is GTK-only.
        match &event {
            UiEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } if ctx.in_title_bar(position.x, position.y) => {
                backend.begin_window_drag();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::DoubleClick { position, .. } if ctx.in_title_bar(position.x, position.y) => {
                backend.toggle_window_maximize();
                self.draw_needed.set(true);
                return quadraui::Reaction::Redraw;
            }
            UiEvent::MouseDown {
                button: MouseButton::Left,
                position,
                ..
            } => {
                if let Some(edge) = ctx.window_edge(position.x, position.y, backend.line_height()) {
                    backend.begin_window_resize(edge);
                    self.draw_needed.set(true);
                    return quadraui::Reaction::Redraw;
                }
            }
            _ => {}
        }

        // Pointer events over the sidebar content area are forwarded to the active
        // panel's controller before the editor click path sees them. In ShellApp
        // mode there is no per-panel DrawingArea, so without this the file explorer
        // never receives clicks. (#540 ShellApp port)
        if self.try_route_sidebar_mouse_event(&event, ctx) {
            return if self.draw_needed.get() {
                self.draw_needed.set(false);
                quadraui::Reaction::Redraw
            } else {
                quadraui::Reaction::Continue
            };
        }

        match event {
            UiEvent::KeyPressed { key, modifiers, .. } => {
                let (key_name, unicode) = match key {
                    Key::Char(c) => (c.to_string(), Some(c)),
                    Key::Named(ref named) => {
                        let n: &str = match named {
                            NamedKey::Escape => "Escape",
                            NamedKey::Tab => "Tab",
                            NamedKey::BackTab => "BackTab",
                            NamedKey::Enter => "Return",
                            NamedKey::Backspace => "BackSpace",
                            NamedKey::Delete => "Delete",
                            NamedKey::Insert => "Insert",
                            NamedKey::Home => "Home",
                            NamedKey::End => "End",
                            NamedKey::PageUp => "PageUp",
                            NamedKey::PageDown => "PageDown",
                            NamedKey::Up => "Up",
                            NamedKey::Down => "Down",
                            NamedKey::Left => "Left",
                            NamedKey::Right => "Right",
                            NamedKey::F(1) => "F1",
                            NamedKey::F(2) => "F2",
                            NamedKey::F(3) => "F3",
                            NamedKey::F(4) => "F4",
                            NamedKey::F(5) => "F5",
                            NamedKey::F(6) => "F6",
                            NamedKey::F(7) => "F7",
                            NamedKey::F(8) => "F8",
                            NamedKey::F(9) => "F9",
                            NamedKey::F(10) => "F10",
                            NamedKey::F(11) => "F11",
                            NamedKey::F(12) => "F12",
                            _ => "",
                        };
                        (n.to_string(), None)
                    }
                };
                if !key_name.is_empty() || unicode.is_some() {
                    self.handle_key_press(
                        key_name,
                        unicode,
                        modifiers.ctrl,
                        modifiers.shift,
                        modifiers.alt,
                        ctx,
                    );
                }
            }
            UiEvent::CharTyped(c) => {
                // Ctrl-modified characters arrive via KeyPressed; CharTyped is
                // for IME-composed printable characters only.
                self.handle_key_press(c.to_string(), Some(c), false, false, false, ctx);
            }
            UiEvent::Accelerator(id, _mods) => {
                let id_str = id.as_str().to_string();
                dispatch_gtk_panel_accelerator(&id_str, &self.deferred, &self.engine);
                self.draw_needed.set(true);
            }
            UiEvent::MouseDown {
                button,
                position,
                modifiers,
                ..
            } => {
                let main = ctx.layout.main_content_bounds;
                let (w, h) = (main.width as f64, main.height as f64);
                match button {
                    MouseButton::Left if modifiers.ctrl => {
                        self.handle_ctrl_mouse_click(position.x as f64, position.y as f64);
                    }
                    MouseButton::Left => {
                        self.handle_mouse_click_msg(
                            position.x as f64,
                            position.y as f64,
                            w,
                            h,
                            modifiers.alt,
                        );
                    }
                    MouseButton::Right => {
                        let rx = position.x as f64;
                        let ry = position.y as f64;
                        // ── Modal-overlay rung (#733 review) ────────────
                        // A modal dialog eats every event, including
                        // right-clicks, so it can't be right-clicked
                        // through to the editor/tab context menu
                        // underneath — TUI's `handle_mouse` already
                        // returns unconditionally for any event kind
                        // while `engine.dialog.is_some()`. This backend's
                        // left-click path goes through
                        // `route_modal_overlay_click` via
                        // `handle_mouse_click_msg`, but the right-click
                        // path used to skip straight to tab/editor
                        // resolution below without consulting it, so a
                        // right-click on an open dialog opened the
                        // editor's context menu behind it. Route through
                        // the same shared rung (`ModalMouseAction::Other`)
                        // before doing anything else.
                        let modal_route =
                            self.route_modal_overlay(rx, ry, render::ModalMouseAction::Other);
                        if modal_route == render::ModalOverlayRoute::Swallow {
                            self.draw_needed.set(true);
                        } else {
                            // #546 FAILED-1: this used to unconditionally build
                            // `EditorRightClick`, so right-clicking a tab opened
                            // the *editor's* context menu (identical item list to
                            // right-clicking in the buffer) instead of a
                            // tab-specific one. Resolve the click against the
                            // last-painted tab-bar geometry first — read-only, no
                            // engine mutation — and only fall back to the editor
                            // menu when it isn't over a tab.
                            let tab_target = {
                                let engine = self.engine.borrow();
                                let layout_ref = self.cached_screen_layout.borrow();
                                layout_ref.as_ref().and_then(|layout| {
                                    resolve_tab_right_click(
                                        &engine,
                                        rx,
                                        ry,
                                        self.cached_line_height,
                                        self.cached_char_width,
                                        layout,
                                        &self.cached_tab_pixel_hits.borrow(),
                                        self.cached_frame_hit_map.borrow().as_ref(),
                                        &self.cached_tab_bar_zones.borrow(),
                                    )
                                })
                            };
                            if let Some((group_id, tab_idx)) = tab_target {
                                self.handle_tab_right_click(group_id, tab_idx, rx, ry);
                            } else {
                                self.handle_editor_right_click(rx, ry);
                            }
                        }
                    }
                    _ => {}
                }
                // Mouse clicks always require a redraw (cursor movement, selection,
                // focus change). draw_needed may already be set by the handler
                // above, but set it unconditionally so handle() returns
                // Reaction::Redraw even when a handler takes an early-return path.
                self.draw_needed.set(true);
            }
            UiEvent::DoubleClick { position, .. } => {
                self.handle_mouse_double_click_msg(position.x as f64, position.y as f64);
                self.draw_needed.set(true);
            }
            UiEvent::MouseMoved { position, buttons } => {
                self.mouse_pos_cell
                    .set((position.x as f64, position.y as f64));
                // ── Modal-overlay hover rung (#751) ─────────────────────
                // An open context menu tracks the pointer, exactly as TUI's
                // `handle_mouse` has always done. This backend had no hover
                // arm at all, so whichever item was selected when the menu
                // opened stayed highlighted wherever the pointer went (#373)
                // — and a keyboard Down after a mouse hover then moved from
                // the wrong row.
                if !buttons.left {
                    if let render::ModalOverlayRoute::ContextMenu(route) = self.route_modal_overlay(
                        position.x as f64,
                        position.y as f64,
                        render::ModalMouseAction::Move,
                    ) {
                        self.apply_context_menu_route(route);
                    }
                }
                if buttons.left {
                    let main = ctx.layout.main_content_bounds;
                    self.handle_mouse_drag_msg(
                        position.x as f64,
                        position.y as f64,
                        main.width as f64,
                        main.height as f64,
                    );
                }
            }
            UiEvent::MouseUp { .. } => {
                self.handle_mouse_up_msg();
            }
            UiEvent::Scroll {
                delta, position, ..
            } => {
                // #646: record where the wheel event happened before dispatching.
                // `handle_mouse_scroll_msg` takes only the delta, and reads the pointer
                // back out of `last_editor_pointer` to decide which window (or
                // registered scroll surface) the wheel targets. Nothing set that
                // cell after the #540 Relm4→ShellApp migration removed the
                // `EventControllerMotion` that used to — see the field's doc — so
                // it was permanently `None` and every wheel event fell through to
                // the *focused* window regardless of the pointer (#240 behaviour
                // dead on GTK, still live on TUI). A wheel event carries its own
                // position, so use that directly rather than depending on a
                // preceding motion event.
                self.last_editor_pointer
                    .set(Some((position.x as f64, position.y as f64)));
                // #554: **negate y back to GTK's raw polarity.**
                //
                // Two conventions meet at this line and they disagree:
                //
                // - GDK's `EventControllerScroll` reports *positive dy = wheel
                //   down*.
                // - `UiEvent::Scroll.delta` follows quadraui's convention,
                //   *positive y = up toward the top of the content*.
                //   `quadraui::gtk::events::gdk_scroll_to_uievent` is what
                //   flips one into the other — it constructs
                //   `ScrollDelta::new(dx, -dy)`.
                //
                // Everything downstream of `handle_mouse_scroll_msg` — the
                // `delta_y > 0.0 => dir = 1` viewport step, the `picker_scroll`
                // sign, `Engine::handle_terminal_scroll`'s "> 0 = toward live"
                // policy — was written against GTK's raw polarity and is
                // unchanged since before the #540 Relm4→ShellApp migration.
                // Pre-migration the Relm4 `connect_scroll` closure fed it GTK's
                // `dy` directly (as the retired `Msg::MouseScroll`'s payload
                // — the whole bus is gone as of #732)
                // and *separately* pushed the negated `gdk_scroll_to_uievent`
                // form onto the backend event queue. The migration deleted that
                // closure and left the runner's already-negated `UiEvent::Scroll`
                // as the only source, so every wheel notch reached the engine
                // with the sign flipped and the editor scrolled backwards.
                //
                // Only y is negated: `gdk_scroll_to_uievent` passes `dx`
                // through unchanged, so `delta.x` is already GTK-raw.
                self.handle_mouse_scroll_msg(delta.x as f64, -(delta.y as f64));
            }
            UiEvent::WindowResized { .. } => {
                // Runner sets new line_height/char_width after resize.
                self.cached_line_height = backend.line_height() as f64;
                self.cached_char_width = backend.char_width() as f64;
                self.line_height_cell.set(self.cached_line_height);
                self.char_width_cell.set(self.cached_char_width);
                self.handle_resize();
            }
            UiEvent::WindowClose => {
                self.show_quit_confirm();
            }
            // #593: quadraui's runner reads the system clipboard on Ctrl+V /
            // Ctrl+Shift+V / middle-click and delivers the text here,
            // unconditionally consuming the key — there is no raw KeyPressed
            // fallback to catch a paste with. `Engine::route_paste` is the
            // same focus-priority router TUI's `UiEvent::ClipboardPaste` arm
            // already calls (`tui_main/shell_app.rs`), so this one arm covers
            // the command line, search/replace fields, explorer rename, and
            // the editor buffer — see that fn's doc for the full priority
            // chain.
            UiEvent::ClipboardPaste(text) => {
                self.engine.borrow_mut().route_paste(&text);
                self.draw_needed.set(true);
            }
            _ => {}
        }

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }

    fn tick(&mut self, backend: &mut dyn quadraui::Backend) -> quadraui::Reaction {
        // Keep cached metrics up to date.
        self.cached_line_height = backend.line_height() as f64;
        self.cached_char_width = backend.char_width() as f64;
        self.line_height_cell.set(self.cached_line_height);
        self.char_width_cell.set(self.cached_char_width);

        // Retry the window capture until the runner has mapped it — see
        // `capture_window_and_apply_csd` (#552). No-ops once `self.window`
        // is `Some`.
        self.capture_window_and_apply_csd();

        // Drain the actions async GTK callbacks queued for this frame.
        for action in self.deferred.drain() {
            match action {
                DeferredAction::ClearYankHighlight => self.clear_yank_highlight(),
                DeferredAction::RefreshFileTree => self.refresh_file_tree(),
                DeferredAction::Resize => self.handle_resize(),
                DeferredAction::SettingsFileChanged => self.settings_file_changed(),
                DeferredAction::ToggleFocusExplorer => self.toggle_focus_explorer(),
                DeferredAction::ToggleFocusSearch => self.toggle_focus_search(),
                DeferredAction::ToggleSidebar => self.toggle_sidebar_panel(),
                DeferredAction::ToggleTerminal => self.toggle_terminal(),
                DeferredAction::ToggleTerminalMaximize => self.toggle_terminal_maximize(),
            }
        }

        // Run a file dialog requested this frame — needs the runner-owned
        // `backend` handle for `PlatformServices` (#572). See
        // `PendingFileDialog` for why this can't happen in the
        // `open_file_dialog` / `save_workspace_as_dialog` handlers themselves.
        if let Some(req) = self.pending_file_dialog.take() {
            self.run_pending_file_dialog(req, backend);
        }

        // Run a native message dialog queued by `render_content`'s
        // edge-trigger check (#727) — same reason as the file dialog above:
        // needs the runner-owned `backend` for `PlatformServices`, which
        // `render_content`'s paint callback must not block inside.
        if let Some(opts) = self.pending_native_dialog.take() {
            self.run_pending_native_dialog(opts, backend);
        }

        // Periodic background work: LSP, DAP, git, search, etc.
        self.handle_poll_tick();

        if self.draw_needed.get() {
            self.draw_needed.set(false);
            quadraui::Reaction::Redraw
        } else {
            quadraui::Reaction::Continue
        }
    }

    fn on_shell_event(&mut self, event: &quadraui::AppShellEvent) {
        use quadraui::AppShellEvent;
        match event {
            AppShellEvent::PanelChanged { panel_id } => {
                // #557: plugin-provided panels are now real `PanelDefinition`s
                // in the runner's `AppShell` (`build_shell_config`), so their
                // icon clicks arrive here like any built-in panel's. They are
                // *not* engine-`AppShell` panels though — `render_content`
                // dispatches on `engine.ext_panel_active`, which
                // `show_panel` would leave untouched (and, since the engine's
                // AppShell has no such panel, it would no-op entirely) — so
                // route them through the existing `switch_panel` handler
                // that owns the ext-panel focus/toggle bookkeeping.
                if is_ext_panel_id(panel_id.as_str()) {
                    self.switch_panel(panel_id.as_str().to_string());
                    return;
                }
                // Sync the runner's active panel into the engine's AppShell so
                // render_content() draws the correct sidebar panel content.
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.app_shell.show_panel(panel_id);
                    // Switching to a built-in panel has to drop the plugin
                    // panel's claim on the sidebar body, or
                    // `current_active_panel_id` keeps synthesising
                    // `ext:{name}` and the built-in panel never paints.
                    engine.ext_panel_active = None;
                    engine.ext_panel_has_focus = false;
                }
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarHidden => {
                {
                    let mut engine = self.engine.borrow_mut();
                    engine.app_shell.hide_sidebar();
                    // #557: this is also how a *second* click on an open
                    // extension panel's icon arrives, so drop the plugin
                    // panel's claim too (`switch_panel`'s own toggle
                    // branch clears the same two fields). Re-opening still
                    // works: `AppShell::handle_activity_click` reports a click
                    // on the active panel as `PanelChanged`, not
                    // `SidebarHidden`, once the sidebar is hidden.
                    engine.ext_panel_active = None;
                    engine.ext_panel_has_focus = false;
                }
                self.draw_needed.set(true);
            }
            AppShellEvent::SidebarResized { new_width } => {
                self.engine
                    .borrow_mut()
                    .app_shell
                    .set_sidebar_width(*new_width);
            }
            AppShellEvent::BottomItemClicked { id } => {
                // The runner treats bottom activity-bar items as action buttons
                // (not sidebar panels), so it does not change its own sidebar
                // visibility.  For vimcode, bottom items like "bottom:settings"
                // represent sidebar panels stored in the engine's AppShell.
                // Sync the active panel so render_content() draws the correct
                // content the next time the sidebar is visible.
                self.engine.borrow_mut().app_shell.show_panel(id);
                self.draw_needed.set(true);
            }
            _ => {}
        }
    }
}

// view_row_to_buf_line and view_row_to_buf_pos_wrap are now shared functions
// in render.rs — use render::view_row_to_buf_line / render::view_row_to_buf_pos_wrap.

/// Calculate gutter width in pixels based on line number mode and buffer size
#[allow(dead_code)]
fn calculate_gutter_width(mode: LineNumberMode, total_lines: usize, char_width: f64) -> f64 {
    match mode {
        LineNumberMode::None => 0.0,
        LineNumberMode::Absolute => {
            // Width = number of digits + 2 chars padding (1 on each side)
            let digits = total_lines.to_string().len().max(1);
            (digits + 2) as f64 * char_width
        }
        LineNumberMode::Relative | LineNumberMode::Hybrid => {
            // Relative numbers can be large for long files, use at least 3 digits + 2 padding
            let max_relative = total_lines.saturating_sub(1);
            let digits = max_relative.to_string().len().max(3);
            (digits + 2) as f64 * char_width
        }
    }
}

/// Compute the editor area bottom Y coordinate.  Must match draw_editor (draw.rs)
/// so that group rects and divider positions are consistent across draw and click.
/// Compute the target `terminal_panel_rows` when maximizing the GTK panel.
///
/// The rendered terminal panel takes `(terminal_panel_rows + 2) * lh` pixels
/// (2 chrome rows = bottom-panel tab bar + terminal toolbar). Editor tab bar
/// stays visible (1 row reserved); breadcrumbs are suppressed elsewhere so
/// we don't reserve a row for them here. Called every frame from `draw_frame`
fn gtk_editor_bottom(engine: &Engine, _da_width: f64, da_height: f64, line_height: f64) -> f64 {
    render::compute_editor_layout(engine, da_height, line_height, false).editor_bottom
}

/// Compute editor window rects with the same formula `render_content` uses
/// (previously shared with the now-deleted `sync_scrollbar`, #731), so event
/// handlers can do hit-testing without duplicating the layout logic.
fn compute_editor_window_rects(
    engine: &Engine,
    da_width: f64,
    da_height: f64,
    line_height: f64,
) -> Vec<(core::WindowId, core::WindowRect)> {
    let tab_bar_height = render::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
    let editor_bounds = core::WindowRect::new(
        0.0,
        0.0,
        da_width,
        gtk_editor_bottom(engine, da_width, da_height, line_height),
    );
    let (rects, _dividers) = engine.calculate_group_window_rects(editor_bounds, tab_bar_height);
    rects
}

/// Compute the thumb geometry for one window's h scrollbar.
/// Returns `(track_x, track_y, track_w, sb_height, thumb_x, thumb_w, scroll_range, px_per_col)`.
/// Returns `None` when no scrollbar is needed (content fits).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn h_scrollbar_geometry(
    engine: &Engine,
    window_id: core::WindowId,
    rect: &core::WindowRect,
    char_width: f64,
    line_height: f64,
) -> Option<(f64, f64, f64, f64, f64, f64, f64, f64)> {
    let window = engine.windows.get(&window_id)?;
    let buffer_state = engine.buffer_manager.get(window.buffer_id)?;

    // max_col is pre-computed and cached in BufferState on every edit — O(1) vs O(N_lines).
    let max_line_length = buffer_state.max_col as f64;

    let v_scrollbar_px = 8.0_f64;
    let track_w = (rect.width - v_scrollbar_px).max(1.0);
    let visible_cols = (track_w / char_width).floor().max(1.0);

    if max_line_length <= visible_cols {
        return None;
    }

    let sb_height = (line_height * 0.35).round().max(4.0);
    let track_x = rect.x;
    // Per-window status line lives at `rect.y + rect.height -
    // line_height` and paints after the scrollbars, so anchor the
    // h-scrollbar above it when the status line is on. Otherwise the
    // status bar overdraws the entire scrollbar (it's `line_height`
    // tall vs the scrollbar's ~5px). `render::window_status_row_reserved`
    // is the single source of truth for whether that row is actually
    // painted (#728) — this used to check `window_status_line &&
    // !terminal_maximized` directly, which (unlike the shared helper)
    // never accounted for `status_line_above_terminal`/bottom-panel state
    // pulling the status line out into a separated bar instead, and so
    // could disagree with `build_screen_layout` about whether this row is
    // free.
    let status_offset = if render::window_status_row_reserved(engine) {
        line_height
    } else {
        0.0
    };
    let track_y = rect.y + rect.height - sb_height - status_offset;
    let scroll_range = (max_line_length - visible_cols).max(1.0);
    let thumb_frac = visible_cols / max_line_length;
    let thumb_w = (thumb_frac * track_w).max(20.0).min(track_w);
    let px_per_col = (track_w - thumb_w) / scroll_range;
    let scroll_left = window.view.scroll_left as f64;
    let thumb_x = track_x + (scroll_left / scroll_range) * (track_w - thumb_w);

    Some((
        track_x,
        track_y,
        track_w,
        sb_height,
        thumb_x,
        thumb_w,
        scroll_range,
        px_per_col,
    ))
}

/// Hit-test a point against all h scrollbars. Returns `(window_id,
/// scroll_left_at_click)` when the point is on any h scrollbar track (not only
/// the thumb), so the caller can decide between thumb-drag and track-click.
fn h_scrollbar_hit_test(
    engine: &Engine,
    x: f64,
    y: f64,
    window_rects: &[(core::WindowId, core::WindowRect)],
    char_width: f64,
    line_height: f64,
) -> Option<(core::WindowId, usize)> {
    for (window_id, rect) in window_rects {
        if let Some((track_x, track_y, track_w, sb_height, _, _, _, _)) =
            h_scrollbar_geometry(engine, *window_id, rect, char_width, line_height)
        {
            if x >= track_x && x <= track_x + track_w && y >= track_y && y <= track_y + sb_height {
                let scroll_left = engine
                    .windows
                    .get(window_id)
                    .map(|w| w.view.scroll_left)
                    .unwrap_or(0);
                return Some((*window_id, scroll_left));
            }
        }
    }
    None
}

// #731: `tab_close_hit_test`, `tab_tooltip_hit_test`, and `shorten_path`
// used to live here. Their only caller was the ~135-line hover-polling
// block removed from `App::tick` by this issue (see that removal's doc
// comment) — dead since #540, so deleting them changes nothing on screen.
// `App::cached_tab_close_abs` (mentioned in `tab_close_hit_test`'s old doc)
// is still populated by `render_content` and read elsewhere (click
// handling), so that mechanism itself is unaffected.

/// Entry point for GTK mode.
///
/// `pub` rather than `pub(crate)` since #657: the caller is `src/main.rs`,
/// which is now a separate crate from this module's.
pub fn run(file_path: Option<PathBuf>) {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        std::env::set_var("DISPLAY", ":0");
    }

    // Install panic hook that flushes swap files + writes crash log.
    {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Emergency: flush swap files for all dirty buffers.
            crate::core::swap::run_emergency_flush();

            if let Some(path) = crate::core::swap::write_crash_log(info) {
                eprintln!("VimCode crashed. Details written to {}", path.display());
                eprintln!("Unsaved buffers written to swap files for recovery.");
                eprintln!("Please report this at https://github.com/JDonaghy/vimcode/issues");
            }
            prev_hook(info);
        }));
    }

    install_icon_and_desktop();
    unsafe {
        gtk4::glib::ffi::g_log_set_writer_func(Some(gtk_log_writer), std::ptr::null_mut(), None);
    }
    // Initialize GTK before App::new() so that CssProvider, Display,
    // and Settings calls inside App::new() find an initialized toolkit.
    // Under the old Relm4 path this happened inside RelmApp::create_and_run();
    // with the ShellApp runner it happens inside gapp.run() which is called
    // by run_with_shell() — too late for App::new().
    gtk4::init().expect("Failed to initialize GTK");
    // Create the App and run via the quadraui ShellApp runner.
    // The runner creates its own GTK Application + window; vimcode's engine
    // and event handling are wired in via impl ShellApp for App above.
    let vimcode_app = App::new(file_path);
    let config = build_shell_config(&vimcode_app);
    quadraui::gtk::shell_runner::run_with_shell(vimcode_app, config);
}

/// Derive the runner's [`quadraui::ShellConfig`] from an [`App`]'s engine state.
///
/// Split out of [`run`] (#646) so the headless test harness
/// (`crate::gtk::testing`) can hand `driver_with_shell` the *same* config the
/// live runner uses, instead of a hand-written approximation that would drift
/// from it silently (the failure mode the TUI side's `config()` test helper
/// already has to work around).
fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    // Mirror the engine's AppShell panel list into the ShellConfig so the
    // quadraui runner renders the activity bar icons.  The engine stores all
    // panels (including "bottom:settings") in a single `panels()` slice;
    // ShellConfig wants top-pinned panels in its first arg and bottom-pinned
    // items via `with_bottom_items()`, so split on the "bottom:" ID prefix.
    // Fill in activity-bar icons before building ShellConfig.  The engine's
    // AppShell initialises all PanelDefinition.icon fields to "" because the
    // engine itself is backend-agnostic; the GTK runner is responsible for
    // mapping each panel ID to the correct Nerd-Font / fallback glyph.
    let panels_with_icons: Vec<_> = app
        .engine
        .borrow()
        .app_shell
        .panels()
        .iter()
        .cloned()
        .map(|mut p| {
            p.icon = match p.id.as_str() {
                "panel:explorer" => crate::icons::EXPLORER.s().to_string(),
                "panel:search" => crate::icons::SEARCH_COD.s().to_string(),
                "panel:debug" => crate::icons::DEBUG.s().to_string(),
                "panel:git" => crate::icons::GIT_BRANCH.s().to_string(),
                "panel:extensions" => crate::icons::EXTENSIONS.s().to_string(),
                "panel:ai" => crate::icons::AI_CHAT.s().to_string(),
                "bottom:settings" => crate::icons::SETTINGS.s().to_string(),
                _ => p.icon,
            };
            p
        })
        .collect();
    let (mut top_panels, bottom_items): (Vec<_>, Vec<_>) = panels_with_icons
        .into_iter()
        .partition(|p| !p.id.as_str().starts_with("bottom:"));
    // #557: plugin-provided panels (e.g. the Git Insights extension) live in
    // `engine.ext_panels`, not in the engine's `AppShell` — nothing registers
    // them there — so they have to be appended explicitly or the runner's
    // activity bar renders no icon for them at all. `ext_activity_panels`
    // already carries each panel's resolved icon, so the id→glyph match above
    // deliberately doesn't need an arm for them.
    top_panels.extend(app.engine.borrow().ext_activity_panels());
    // (#552) Reserve a full-width title-bar band across the top of the shell
    // (above activity bar + sidebar + main content, not just main content) —
    // GTK draws its own client-side menu bar + inline window controls into
    // it since `run_with_shell` creates an undecorated-chrome-free window.
    // Always on: GTK's menu bar acts as its titlebar (matches pre-#540
    // behaviour), unlike TUI where it's optional.
    //
    // #710 item 2: `height_lh` is a line-height *multiple* of the editor's
    // `current_line_height` (`AppShell::compute_layout`, quadraui
    // `compose/app_shell.rs`) — there is no fixed-px reservation API yet, so
    // this band is unavoidably coupled to editor font metrics until one
    // exists (see the doc comment on `quadraui::ShellConfig::with_title_bar`
    // and file a quadraui px-based-reservation issue if this residual needs
    // closing — #710's PR should note whether that filing happened). 1.0
    // measured ~18px in the headless GTK test harness — one editor text
    // line, visibly squat next to VS Code's 35px title bar / ~26px command
    // centre pill (`quadraui::gtk::command_center::draw_command_center`
    // paints the pill at `band_height - 4`). 1.7 measures ~31px band / 27px
    // pill here — much closer to parity without needing the fixed-px API.
    let mut cfg = quadraui::ShellConfig::new("VimCode", top_panels)
        .with_bottom_items(bottom_items)
        .with_title_bar(1.7)
        // #719: quadraui#656 builders — route the WM app id / icon name
        // through the single `APP_ID` constant #716 introduced, rather than
        // a fresh string literal, so there's exactly one identity string.
        .with_app_id(util::APP_ID)
        .with_icon_name(util::APP_ID)
        // #719: quadraui#657 fixed-px form. The activity bar's row height is
        // already the fixed `ACTIVITY_ROW_PX = 48.0` (VS Code parity), so
        // sizing the bar's *width* from the editor font (the old default)
        // made it oblong; pin the width to the same 48px instead of using
        // the font-relative unit form.
        .with_activity_bar_width_px(48.0);
    // #759: the shared Alt rung's sidebar clamps, so Alt+Left/Right resolve
    // identically on both backends. TUI has set exactly this pair since #634
    // (with a comment naming the failure — "Alt+Right would silently stop at
    // 50"); GTK kept quadraui's generic 8/50 because it had no Alt rung to
    // stop short in the first place. `default_sidebar_width` is deliberately
    // left at quadraui's 20: GTK's unit is a line-height, not a column, so
    // TUI's 30-*column* default is not the same quantity.
    cfg.min_sidebar_width = render::ALT_SIDEBAR_WIDTH_MIN as f32;
    cfg.max_sidebar_width = render::ALT_SIDEBAR_WIDTH_MAX as f32;
    cfg
}

// #731: the `native_scrollbar_placement_tests` module that used to live
// here (#723) tested `native_scrollbar_margin_start`'s pure inset
// arithmetic — that function guarded the native `gtk4::Scrollbar` overlay
// path deleted by this issue (`sync_scrollbar`/`create_window_scrollbars`),
// which never ran under the ShellApp runner in the first place (nothing
// assigns `self.overlay`/`self.drawing_area`), so #723's fix was never
// live on screen. See the doc comment above the `Surface::Editor` push in
// `render_content` for where the minimap-inset decision needs to move
// (quadraui's `gtk::editor::draw_editor`, mirroring TUI's inline
// scrollbar column) and the quadraui issue that needs filing first.

#[cfg(test)]
mod h_scrollbar_status_offset_tests {
    //! #728: `h_scrollbar_geometry`'s status-row offset used to check
    //! `window_status_line && !terminal_maximized` directly, while
    //! `render::build_screen_layout`'s reservation of that same row used
    //! `per_window_status && !separate_status` — two independent answers to
    //! "is a per-window status row painted here", each covering an axis the
    //! other didn't (`terminal_maximized` vs. `separate_status`). Both now
    //! go through `render::window_status_row_reserved`; these pin that the
    //! scrollbar's track actually moves in lockstep with it rather than
    //! re-diverging.
    use super::h_scrollbar_geometry;
    use crate::core::{Engine, WindowRect};

    /// A window whose longest line overflows a narrow viewport, so
    /// `h_scrollbar_geometry` returns `Some` rather than `None` ("content
    /// fits" — nothing to offset).
    fn engine_needing_h_scrollbar() -> Engine {
        let mut e = Engine::new_for_test();
        e.buffer_mut().insert(0, &"x".repeat(500));
        // `max_col` (what `h_scrollbar_geometry` reads) is a cache
        // refreshed by `update_syntax`, not by a raw `Buffer::insert` —
        // force it so the 500-char line above is actually reflected.
        let wid = e.active_window_id();
        let buffer_id = e.windows.get(&wid).unwrap().buffer_id;
        e.buffer_manager.get_mut(buffer_id).unwrap().update_syntax();
        e
    }

    #[test]
    fn track_moves_up_by_exactly_one_row_when_the_status_row_is_reserved() {
        let mut e = engine_needing_h_scrollbar();
        e.settings.window_status_line = true;
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
        let line_height = 20.0;

        let (_, track_y_with, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("an overflowing line needs an h-scrollbar");

        e.settings.window_status_line = false;
        let (_, track_y_without, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("still overflowing with the status line off");

        assert_eq!(
            track_y_without - track_y_with,
            line_height,
            "the status row must shift the h-scrollbar up by exactly one line_height"
        );
    }

    /// #728 regression: with `status_line_above_terminal` OFF and the
    /// bottom panel open, the active window's status is pulled into a
    /// *separated* bar above the terminal instead of painting inside this
    /// window — `render::window_status_row_reserved` reports the row as
    /// free, and the h-scrollbar must agree. The old
    /// `window_status_line && !terminal_maximized` predicate never checked
    /// this axis and would have offset for a row nothing paints here.
    /// RED against that predicate (verified while writing this fix): 13.0
    /// vs. 33.0 — the old code offset the track by a full `line_height` for
    /// a status row that was actually painted as a separated bar elsewhere.
    #[test]
    fn track_does_not_move_when_status_is_separated_above_the_terminal() {
        let mut e = engine_needing_h_scrollbar();
        e.settings.window_status_line = true;
        e.settings.status_line_above_terminal = false;
        e.terminal_open = true;
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
        let line_height = 20.0;

        let (_, track_y_separated, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("an overflowing line needs an h-scrollbar");

        e.settings.window_status_line = false;
        let (_, track_y_no_status, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("still overflowing with the status line off");

        assert_eq!(
            track_y_separated, track_y_no_status,
            "a separated status bar must not offset the h-scrollbar — this \
             window's own bottom row is free"
        );
    }

    /// #728 regression: while the terminal panel is maximized, editor
    /// windows are not the visible surface, so nothing paints a per-window
    /// status row even with the setting on — the h-scrollbar must not
    /// offset for one. This is the axis `build_screen_layout`'s old
    /// predicate never checked (only GTK's did).
    #[test]
    fn track_does_not_move_when_the_terminal_is_maximized() {
        let mut e = engine_needing_h_scrollbar();
        e.settings.window_status_line = true;
        e.terminal_maximized = true;
        let wid = e.active_window_id();
        let rect = WindowRect::new(0.0, 0.0, 100.0, 40.0);
        let line_height = 20.0;

        let (_, track_y_maximized, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("an overflowing line needs an h-scrollbar");

        e.settings.window_status_line = false;
        let (_, track_y_no_status, ..) = h_scrollbar_geometry(&e, wid, &rect, 8.0, line_height)
            .expect("still overflowing with the status line off");

        assert_eq!(track_y_maximized, track_y_no_status);
    }
}

#[cfg(test)]
mod shell_config_identity_tests {
    //! #719: quadraui#656/#657 landed `ShellConfig::with_app_id()` /
    //! `with_icon_name()` / `with_activity_bar_width_px()`, but a pin bump
    //! alone doesn't prove `build_shell_config` actually calls them — a
    //! headless build can't assert the WM taskbar/alt-tab icon (that's the
    //! SMOKE_TESTS item), but it *can* assert the values reach the
    //! `ShellConfig` GTK's toplevel is built from, which is the only part
    //! of this fix source-level tests can reach.
    use super::{build_shell_config, util, App};
    use crate::core::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn app_id_and_icon_name_reach_shell_config() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(engine);
        let config = build_shell_config(&app);
        assert_eq!(config.app_id, util::APP_ID);
        assert_eq!(config.icon_name.as_deref(), Some(util::APP_ID));
    }

    #[test]
    fn activity_bar_pinned_to_48px_matching_its_own_row_height() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let app = App::new_headless(engine);
        let config = build_shell_config(&app);
        assert_eq!(config.activity_bar_width_px, Some(48.0));
    }
}

#[cfg(test)]
mod chrome_paint_tests {
    //! Headless pixel-paint regression test for the CSD title bar's inline
    //! window-control buttons (#552). A round-2 smoke test reported the
    //! minimize/maximize/close glyphs as completely invisible even though
    //! their click hit-regions were live. Paints
    //! `render::window_controls_status_bar` into an in-memory Cairo
    //! `ImageSurface` (no display required — same pattern quadraui's own
    //! `gtk/tab_bar.rs` headless paint tests use) and reads back pixels to
    //! confirm the button glyphs actually paint non-background pixels.
    use crate::render::{self, Theme};
    use pangocairo::cairo::{Context, Format, ImageSurface};

    const W: i32 = 400;
    const ROW_H: i32 = 28;
    const LINE_H: f64 = 20.0;

    /// Read an RGB triple from an ARgb32 surface at pixel (x, y).
    fn pixel(data: &[u8], stride: usize, x: i32, y: i32) -> (u8, u8, u8) {
        let off = y as usize * stride + x as usize * 4;
        (data[off + 2], data[off + 1], data[off])
    }

    /// Perceptual (sRGB-weighted) luminance, 0..255.
    fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
        0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
    }

    /// Paint `render::window_controls_status_bar(theme, false)` into a fresh
    /// headless surface and return the max luminance delta, against the
    /// bar's own background fill, found **within each button's own
    /// hit-region x-range** — keyed by the button's `action_id`.
    ///
    /// #715: the original version of this helper measured the max delta
    /// found *anywhere in the whole row*. Three buttons share one row, so
    /// two glyphs painting fine was enough to clear the floor even though
    /// the third (minimize, `U+2500` — a hairline box-drawing rule with no
    /// coverage in the resolved UI font) contributed zero pixels. Per-segment
    /// measurement is the only version of this check that can fail on a
    /// single invisible button rather than needing all three to break at
    /// once. Segment x-ranges come from the `StatusBarLayout` `draw_status_bar`
    /// returns — the same hit-region data the real click handler resolves
    /// against (`render::window_controls_status_bar`'s doc comment) — rather
    /// than hardcoded pixel columns, so a future layout change can't
    /// silently desync the test from what's actually painted.
    ///
    /// A glyph that paints but has near-zero contrast against its own
    /// background (e.g. white-on-near-white) is exactly as invisible to a
    /// user as a glyph that paints nothing at all — a plain "differs from
    /// background" check would pass in both cases, so this measures the
    /// actual perceptual gap instead.
    fn per_segment_contrast_deltas(theme: &Theme) -> Vec<(String, f64)> {
        let bar = render::window_controls_status_bar(theme, false);

        let mut surface =
            ImageSurface::create(Format::ARgb32, W, ROW_H).expect("create ImageSurface");
        let layout = {
            let cr = Context::new(&surface).expect("Context::new");
            // Fill with a color that can't be confused with any themed fg/bg.
            cr.set_source_rgb(1.0, 0.0, 1.0);
            cr.paint().ok();

            let pango_layout = pangocairo::functions::create_layout(&cr);
            quadraui::gtk::draw_status_bar(
                &cr,
                &pango_layout,
                0.0,
                0.0,
                W as f64,
                LINE_H,
                &bar,
                &render::to_quadraui_theme(theme),
                None,
                None,
            )
        };
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().expect("surface data");

        let bg = {
            let c = render::to_quadraui_color(theme.tab_bar_bg);
            (c.r, c.g, c.b)
        };
        let bg_lum = luminance(bg);

        layout
            .hit_regions
            .iter()
            .filter_map(|(rect, hit)| match hit {
                quadraui::StatusBarHit::Segment(id) => Some((rect, id)),
                quadraui::StatusBarHit::Empty => None,
            })
            .map(|(rect, id)| {
                let x0 = rect.x.round() as i32;
                let x1 = (rect.x + rect.width).round() as i32;
                let mut max_delta = 0.0f64;
                // Scan every row of the painted surface, not just one
                // mid-line scanline (#715): a thin glyph like an em dash
                // sits at a specific baseline offset that a single sampled
                // row can miss even though the glyph paints fine — that
                // would be a false "invisible" failure caused by the test's
                // own sampling, not a real bug. Scanning the full height
                // means only a genuinely unpainted segment reports zero.
                for y in 0..ROW_H {
                    for x in x0.max(0)..x1.min(W) {
                        let px = pixel(&data, stride, x, y);
                        if px == (255, 0, 255) {
                            continue; // untouched sentinel fill — not part of the bar.
                        }
                        max_delta = max_delta.max((luminance(px) - bg_lum).abs());
                    }
                }
                (id.as_str().to_string(), max_delta)
            })
            .collect()
    }

    /// #552 round-2 smoke test: the minimize/maximize/close glyphs rendered
    /// with zero visible pixels. Root cause: `window_controls_status_bar`
    /// paired its glyph `fg` with `theme.status_fg` (designed to contrast
    /// against `status_bg`, the *bottom* status line's background) instead
    /// of a color actually paired with `tab_bar_bg` — the background this
    /// row uses. The `vs_light` theme (`tab_bar_bg` #ececec, old `status_fg`
    /// #ffffff) rendered white-on-near-white, which is as good as invisible
    /// even though pixels technically get painted. Runs across every
    /// built-in theme (not just the default) so a future contrast
    /// regression on any one theme fails loudly instead of only surfacing
    /// in a manual smoke test against a theme nobody happened to try.
    ///
    /// #715: checked **per button**, not row-max. On the reporter's real GTK
    /// desktop the old minimize glyph (`U+2500`, a box-drawing hairline)
    /// painted zero visible pixels while `□`/`✕` painted fine — a row-max
    /// check only needs *one* of the three buttons visible to pass, so it
    /// shipped anyway. (This headless Cairo/Pango environment happens to
    /// have box-drawing coverage in its fallback font, so it can't reproduce
    /// that exact zero-pixel case — verified instead by temporarily blanking
    /// a segment's glyph entirely, which *is* reproducible headlessly and
    /// exercises the identical "one button visible, one isn't" failure
    /// shape.) Asserting on each of the three `action_id`s independently is
    /// the only version of this check that can catch a single dead button.
    #[test]
    fn window_control_buttons_are_visible_against_their_background_in_every_theme() {
        let expected_actions = [
            render::WINDOW_MINIMIZE_ACTION,
            render::WINDOW_MAXIMIZE_ACTION,
            render::WINDOW_CLOSE_ACTION,
        ];
        for name in Theme::available_names() {
            let theme = Theme::from_name(&name);
            let deltas = per_segment_contrast_deltas(&theme);
            for action in expected_actions {
                let delta = deltas
                    .iter()
                    .find(|(id, _)| id == action)
                    .unwrap_or_else(|| {
                        panic!(
                            "theme {name:?}: no hit-region painted for window-control \
                             action {action:?} — button missing from the row entirely"
                        )
                    })
                    .1;
                // WCAG-ish floor: anything much below this reads as "same
                // color" at a glance, which is exactly the bug this test
                // guards against.
                assert!(
                    delta > 40.0,
                    "theme {name:?}: window-control button {action:?} has only \
                     {delta:.1} luminance contrast against tab_bar_bg — \
                     effectively invisible"
                );
            }
        }
    }
}
