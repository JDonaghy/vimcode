use super::*;

// ─── Frame composition: the chrome band ─────────────────────────────────────
//
// Slice 1 landed the overlay band only. `OverlayOp` turned out to be the right
// shape and the wrong scope, so this slice generalises it downward into
// [`FrameOp`]/[`compose_frame`] and lands the **chrome band**: the non-editor
// surfaces vimcode itself paints around the editor column.
//
// The five rungs (menu-row, sidebar panel body, wildmenu, status bar, command
// line) occupy disjoint bands of the window and are painted in a single order
// on both backends. [`compose_frame`] states the gates for all of them once, in
// the units the caller paints in.
//
// **Not in this band, deliberately** — the rungs quadraui already owns:
//
//   * the **activity bar** and the **sidebar separator**. On both live paths
//     `AppShell::render` (quadraui `compose::app_shell`) paints these *before*
//     `render_content` is entered. The test-only `draw_frame` was deleted when
//     it had no production caller.
//   * the **debug toolbar**, **quickfix panel** and **bottom panel**. These are
//     vimcode's own stacked bottom chrome, which `AppShellLayout` has no concept
//     of. They are chrome, but they are *not* part of this slice.
//
// **Why here and not in quadraui**, for the five rungs that *are* in the band:
// `AppShell` composes the *shell's* own zones and stops there; the menu row's
// contents, the wildmenu, the global status line and the Vim command line are
// vimcode's app-level surfaces, and the sidebar *body* is explicitly handed back
// to the app as `sidebar_content_bounds` for the app to fill. quadraui states no
// order for any of them, so the ordering is vimcode's to state and `render.rs` is
// where vimcode states cross-backend contracts.

/// The metrics one frame is composed in — a *unit*, not a geometry.
///
/// GTK passes real pixels (`backend.line_height()` / `char_width()`), TUI
/// passes `1.0`/`1.0` for one terminal cell. [`compose_frame`] uses them only
/// to answer "is this reserved band at least one line tall / one column wide",
/// which is the one question whose answer differs between the two unit systems.
/// It never returns geometry: rect math stays per backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameMetrics {
    pub line_height: f32,
    pub char_width: f32,
}

impl FrameMetrics {
    /// One terminal cell.
    pub const CELL: Self = Self {
        line_height: 1.0,
        char_width: 1.0,
    };

    /// Real pixels — what `gtk::App::render_content` composes in.
    pub fn px(line_height: f64, char_width: f64) -> Self {
        Self {
            line_height: line_height as f32,
            char_width: char_width as f32,
        }
    }

    /// Is `rect` big enough to hold one line of chrome?
    fn holds_a_line(&self, rect: quadraui::Rect) -> bool {
        rect.width >= self.char_width && rect.height >= self.line_height
    }
}

/// One rung of the shared **frame sequence** — every non-editor surface
/// vimcode composes around and on top of the editor column, chrome first and
/// app-level overlays last.
///
/// Deliberately unit-agnostic: the rung says *what* is composed and *in what
/// order*, never where or how big. Geometry stays per backend (GTK composes in
/// pixels, TUI in cells) and rasterisation stays per backend (Cairo
/// painter-order vs. ratatui cell coalescence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameOp {
    /// The reserved title-bar band (`AppShellLayout::title_bar_bounds`):
    /// measure the menu bar, publish the band's rect for hit-testing, and work
    /// out where the command centre / window controls go.
    ///
    /// Measure-only on **both** backends. The band is actually *painted* from
    /// [`FrameOp::MenuDropdown`], because `MenuSystem::render` repaints
    /// `draw_menu_bar` across the whole band and would erase anything laid down
    /// first. It is still the first rung of the sequence: every later rung's gate,
    /// and the dropdown arm itself, read the rect this one publishes.
    MenuRow,
    /// The *active panel's body*, into `AppShellLayout::sidebar_content_bounds`
    /// — explorer / search / debug / source control / extensions / AI / plugin
    /// extension panels. The surrounding sidebar chrome (header, activity bar,
    /// separator) is quadraui's, painted before `render_content` is entered.
    SidebarPanel,
    /// The command-line Tab-completion bar (`Backend::draw_status_bar` with
    /// [`wildmenu_to_status_bar`]).
    Wildmenu,
    /// The global status line — drawn only when per-window status lines are
    /// *off*, which is what makes `ScreenLayout::global_status_bar` `None`.
    StatusBar,
    /// The Vim command line (`:`/`/`/`?` and messages). Unconditional on both
    /// backends: the row is always reserved and always painted, empty or not.
    CommandLine,
    /// The folder / workspace picker modal — the first rung of the overlay
    /// tail.
    ///
    /// `quadraui::FolderPickerController` renders through the existing
    /// `Palette` primitive (`Backend::draw_palette`) on **both** backends.
    FolderPicker,
    /// `MenuSystem::render` — repaints the whole title-bar band, so it must
    /// come before anything else that draws into that band. On GTK this arm
    /// also paints the app-icon slot and the inline window controls.
    ///
    /// First rung of the overlay tail, and gated identically to
    /// [`Self::MenuRow`] — the band it paints is the band that rung measured.
    MenuDropdown,
    /// `Backend::draw_command_center`. Must follow [`Self::MenuDropdown`]:
    /// `MenuSystem::render` repaints `draw_menu_bar` across the entire band,
    /// including the command centre's columns, and painting the centre first
    /// left a populated-but-invisible `command_center_layout` on both backends.
    CommandCenter,
    /// `Backend::draw_find_replace`.
    FindReplace,
    /// The unified picker / command palette — [`paint_picker_rung`] on both
    /// backends.
    UnifiedPicker,
    /// The Ctrl+Tab MRU popup (`Backend::draw_list`).
    TabSwitcher,
    /// `Backend::draw_context_menu`.
    ContextMenu,
    /// The change-review surface — [`paint_change_review_rung`] on both backends.
    /// A full-viewport `quadraui::DiffView` for the currently-shown proposed
    /// change, so it sits above every other overlay except a modal dialog and the
    /// toast stack.
    ChangeReview,
    /// The settings panel as a centered modal overlay — wider than the
    /// sidebar, covering ~80% of the viewport with a scrim behind it.
    /// Painted when `Engine::settings_modal_open` is true instead of the
    /// `PANEL_SETTINGS` arm of [`Self::SidebarPanel`].
    SettingsModal,
    /// `Backend::draw_dialog` — modal, so above every rung but the toasts,
    /// matching [`route_modal_overlay_click`]'s own arbitration.
    Dialog,
    /// `Backend::draw_toast_stack` — top of the sequence on both backends, and
    /// the first rung [`route_modal_overlay_click`] arbitrates.
    ToastStack,
}

impl FrameOp {
    /// Is this rung part of the **overlay tail** — the app-level surfaces laid
    /// on top of the chrome?
    ///
    /// Only two callers need the distinction (the mouse-arbitration agreement
    /// test, and the doc-level "chrome vs. overlay" split); production code
    /// walks the whole sequence.
    pub fn is_overlay(self) -> bool {
        matches!(
            self,
            FrameOp::FolderPicker
                | FrameOp::MenuDropdown
                | FrameOp::CommandCenter
                | FrameOp::FindReplace
                | FrameOp::UnifiedPicker
                | FrameOp::TabSwitcher
                | FrameOp::ContextMenu
                | FrameOp::ChangeReview
                | FrameOp::SettingsModal
                | FrameOp::Dialog
                | FrameOp::ToastStack
        )
    }
}

/// The canonical frame order, **lowest z first** (index 0 is composed first,
/// and everything after it may cover it).
///
/// Both backends iterate this one array and `match` each rung, so "which order
/// do we compose the frame in" is one artefact rather than two transcriptions.
/// Adding a surface means adding a variant here: a compile error in both
/// backends' `match` until both handle it.
///
/// The five chrome rungs come first, the eleven overlay rungs
/// ([`FrameOp::is_overlay`]) are the tail: every chrome rung is composed before
/// the first overlay rung, on both backends.
pub const FRAME_Z_ORDER: [FrameOp; 16] = [
    // ── chrome ───────────────────────────────────────────────────────────
    FrameOp::MenuRow,
    FrameOp::SidebarPanel,
    FrameOp::Wildmenu,
    FrameOp::StatusBar,
    FrameOp::CommandLine,
    // ── overlay tail ─────────────────────────────────────────────────────
    FrameOp::FolderPicker,
    FrameOp::MenuDropdown,
    FrameOp::CommandCenter,
    FrameOp::FindReplace,
    FrameOp::UnifiedPicker,
    FrameOp::TabSwitcher,
    FrameOp::ContextMenu,
    FrameOp::ChangeReview,
    FrameOp::SettingsModal,
    FrameOp::Dialog,
    FrameOp::ToastStack,
];

/// Which frame rungs are live this frame, as a (near-)pure function of state.
///
/// Separate from [`FRAME_Z_ORDER`] because the order is a constant and the
/// *live set* is not. This type is what lets a test say "given this state,
/// exactly these rungs must have been composed, in this order".
///
/// Twelve of the fourteen fields are derived from `ScreenLayout` +
/// `AppShellLayout` by [`Self::from_screen`]. The two that are not
/// (`toast_stack`, and any backend-specific suppression) are left to the
/// caller, because they are engine/geometry state rather than screen state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FramePresence {
    pub menu_row: bool,
    pub sidebar_panel: bool,
    pub wildmenu: bool,
    pub status_bar: bool,
    pub command_line: bool,
    /// See [`FrameOp::FolderPicker`]. Not derivable from `ScreenLayout` (the
    /// picker lives on each backend's own shell app / `App`, not the shared
    /// engine), so [`Self::from_screen`] leaves it `false` and the caller
    /// sets it from its own `Option<FolderPickerController>`.
    pub folder_picker: bool,
    pub menu_dropdown: bool,
    pub command_center: bool,
    pub find_replace: bool,
    pub unified_picker: bool,
    pub tab_switcher: bool,
    pub context_menu: bool,
    pub change_review: bool,
    pub settings_modal: bool,
    pub dialog: bool,
    pub toast_stack: bool,
}

impl FramePresence {
    /// The eleven rungs whose gate is a `ScreenLayout` / `AppShellLayout`
    /// field, so a caller only has to supply `toast_stack` (engine state).
    ///
    /// `layout` supplies the shell-reserved bands (title bar, sidebar content)
    /// and `screen` the app state; `metrics` decides "is this band at least one
    /// line tall", in the caller's own units — see [`FrameMetrics`].
    pub fn from_screen(
        screen: &ScreenLayout,
        layout: &quadraui::AppShellLayout,
        metrics: FrameMetrics,
    ) -> Self {
        // `title_bar_bounds` is `Some` only while `AppShell::set_title_bar_visible`
        // is set — but the band can still be degenerate on a window too small to hold
        // it, and a degenerate band must not be measured (its `menu_bar_layout` would
        // hand the command centre and the window controls a zero-width strip and leave
        // `command_center_layout` disagreeing with the paint).
        //
        // This single gate drives the two rungs that stand in for the *drawn* menu row
        // (the row itself and its dropdown). The measure rung's stricter gate is the
        // one that survives.
        let title_bar_band_live = layout
            .title_bar_bounds
            .is_some_and(|r| metrics.holds_a_line(r));
        let title_bar = screen.menu_bar_visible && title_bar_band_live;
        Self {
            menu_row: title_bar,
            sidebar_panel: layout
                .sidebar_content_bounds
                .is_some_and(|r| metrics.holds_a_line(r)),
            wildmenu: screen.wildmenu.is_some(),
            status_bar: screen.global_status_bar.is_some(),
            command_line: true,
            folder_picker: false,
            menu_dropdown: title_bar,
            // The omnibar shares the title-bar *band* with the drawn
            // menu row but not the drawn row's own liveness gate. A
            // native-menu backend (macOS) sets `menu_bar_visible = false` to
            // suppress the redundant in-window `File Edit View` row beneath
            // AppKit's real menu bar, but the command center still belongs in
            // that band — same as VS Code on macOS. So this rung depends only
            // on the band existing, not on whether the drawn menu row is
            // suppressed.
            command_center: title_bar_band_live,
            find_replace: screen.find_replace.is_some(),
            unified_picker: screen.picker.is_some(),
            tab_switcher: screen.tab_switcher.is_some(),
            // Matches both backends' own gate: an items-less context menu is
            // not painted (GTK skips it and clears `context_menu_layout`).
            context_menu: screen
                .context_menu
                .as_ref()
                .is_some_and(|p| !p.items.is_empty()),
            change_review: screen.change_review.is_some(),
            settings_modal: false,
            dialog: screen.dialog.is_some(),
            toast_stack: false,
        }
    }

    /// Is this rung live?
    pub fn is_live(&self, op: FrameOp) -> bool {
        match op {
            FrameOp::MenuRow => self.menu_row,
            FrameOp::SidebarPanel => self.sidebar_panel,
            FrameOp::Wildmenu => self.wildmenu,
            FrameOp::StatusBar => self.status_bar,
            FrameOp::CommandLine => self.command_line,
            FrameOp::FolderPicker => self.folder_picker,
            FrameOp::MenuDropdown => self.menu_dropdown,
            FrameOp::CommandCenter => self.command_center,
            FrameOp::FindReplace => self.find_replace,
            FrameOp::UnifiedPicker => self.unified_picker,
            FrameOp::TabSwitcher => self.tab_switcher,
            FrameOp::ContextMenu => self.context_menu,
            FrameOp::ChangeReview => self.change_review,
            FrameOp::SettingsModal => self.settings_modal,
            FrameOp::Dialog => self.dialog,
            FrameOp::ToastStack => self.toast_stack,
        }
    }
}

/// The ordered list of frame rungs a frame with this `presence` must compose.
///
/// This is both what the backends *walk* and the expected value their
/// *recorded* sequence is compared against.
///
/// Every rung an absent gate drops has no arm left to run, so any hit-test
/// cache a rung owns (`command_center_layout`, `dialog_layout`,
/// `context_menu_layout`, `tab_switcher_popup_rect`, `picker_popup_rect`,
/// `toast_layout`, `menu_bar_rect`, `global_status_rect`) must be cleared by
/// the caller *before* the walk, never from an `else` arm inside it.
pub fn compose_frame(presence: &FramePresence) -> Vec<FrameOp> {
    FRAME_Z_ORDER
        .iter()
        .copied()
        .filter(|op| presence.is_live(*op))
        .collect()
}

/// Assert a backend's *actually composed* frame sequence never runs backwards
/// against [`FRAME_Z_ORDER`].
///
/// The weaker half of the acceptance test: it does not care which rungs were
/// live, only that whatever *was* composed came out in canonical order. A rung
/// hoisted out of the shared walk fails this even when the exact live set
/// is awkward to pin.
///
/// Returns `Err` with a human-readable diagnosis rather than panicking, so
/// callers on either backend can attach their own context.
pub fn check_frame_order(composed: &[FrameOp]) -> Result<(), String> {
    let mut cursor = 0usize;
    for op in composed {
        match FRAME_Z_ORDER[cursor..].iter().position(|c| c == op) {
            Some(offset) => cursor += offset + 1,
            None => {
                return Err(format!(
                    "frame composed out of order: {composed:?}\n\
                     {op:?} came after a rung that FRAME_Z_ORDER puts above it.\n\
                     canonical order is {FRAME_Z_ORDER:?}"
                ));
            }
        }
    }
    Ok(())
}

// ─── Frame-op rung painters ───────────────────────────────────────────────
//
// Ten of `FrameOp`'s fourteen arms were 6-25 line bodies, transcribed once per
// backend with px swapped for cell units. This is that body, once, per arm.
// `rect` (and, where the two backends' units genuinely differ, `char_width`/
// `line_height`) stays a caller-supplied argument, never computed here.
//
// Two of the fourteen arms are not this shape: `FrameOp::CommandLine` paints
// cell-by-cell in TUI with selection-range handling, and `FrameOp::TabSwitcher`
// has per-backend geometry prep. Each stays a full per-backend arm in
// `render_content`.

/// The [`FrameOp::Wildmenu`] rung's whole body on both backends.
pub fn paint_wildmenu_rung(
    b: &mut dyn quadraui::Backend,
    wm: &WildmenuData,
    theme: &Theme,
    rect: quadraui::Rect,
) {
    let bar = wildmenu_to_status_bar(wm, theme);
    let _ = b.draw_status_bar_interactive(rect, &bar, &quadraui::InteractionState::new());
}

/// The [`FrameOp::StatusBar`] (global status line) rung's whole body on both
/// backends.
///
/// Returns the resolved [`quadraui::StatusBarLayout`] so both backends can
/// derive their click zones from the same measurement the paint produced.
pub fn paint_global_status_bar_rung(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    bar: &quadraui::StatusBar,
    rect: quadraui::Rect,
) -> quadraui::StatusBarLayout {
    engine.global_status_rect.set(rect);
    let _ = b.draw_status_bar_interactive(rect, bar, &quadraui::InteractionState::new());
    b.status_bar_layout(rect, bar)
}

/// The [`FrameOp::FindReplace`] rung's whole body on both backends.
///
/// `find_replace.group_bounds` (or, on GTK, `panel.group_bounds`) is already
/// absolute screen space, so `viewport` only supplies the clip
/// rectangle — both call sites already computed it for other rungs.
pub fn paint_find_replace_rung(
    b: &mut dyn quadraui::Backend,
    panel: &FindReplacePanel,
    viewport: quadraui::Rect,
) {
    b.draw_find_replace(viewport, panel);
}

/// The [`FrameOp::CommandCenter`] rung's whole body on both backends.
///
/// Building the `quadraui::CommandCenter` descriptor itself stays at the call
/// site; this is only the paint + cache-set once that descriptor and its rect
/// exist.
pub fn paint_command_center_rung(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    rect: quadraui::Rect,
    cc: &quadraui::CommandCenter,
) {
    let layout = b.draw_command_center(rect, cc);
    engine.command_center_layout.replace(Some(layout));
}

/// The backdrop scrim colour painted behind modal overlays
/// ([`paint_picker_rung`]): semi-transparent black, signalling modality by
/// separating the popup from the editor band behind it. Backends that
/// composite alpha (GTK) blend it into a dim tint; TUI's `draw_solid_fill`
/// drops the alpha channel and fills the band with flat scrim-coloured cells
/// instead, the terminal-idiomatic equivalent. Either way the fill is confined
/// to [`modal_scrim_rect`], so the chrome around the editor band stays
/// readable.
pub const MODAL_SCRIM_COLOR: quadraui::Color = quadraui::Color::rgba(0, 0, 0, 102);

/// Backdrop opacity for the fuzzy finder / unified picker popup
/// ([`paint_picker_rung`]): below `1.0` so the popup's body is translucent and
/// the scrim-dimmed editor/explorer behind it stays visible, rather than a
/// solid panel that occludes everything underneath. The value trades off
/// against legibility — the result rows and query text paint over this
/// backdrop and must stay readable, and only the selected row carries its own
/// opaque fill — so it leans mostly-opaque while still letting content bleed
/// through. `quadraui::Style::backdrop_opacity` applies this to `Palette`'s
/// backdrop alone; every other overlay keeps its opaque `surface_bg`, and GTK
/// blends real alpha while TUI pre-blends the backdrop against the editor
/// background on the CPU.
pub const PICKER_BACKDROP_OPACITY: f32 = 0.8;

/// The rect [`paint_picker_rung`]'s scrim fill should cover: the full window
/// width, from `content_top` down to the lowest row this frame paints *editor*
/// content into — the editor and sidebar band, and nothing below or above it.
///
/// `el` and `line_height` must be this frame's own
/// [`compute_editor_layout`] result and metrics, and `content_top`
/// `main_content_bounds.y`, because every row of bottom chrome is live or
/// absent per frame. The bottom edge is [`EditorLayout::editor_bottom`] — which
/// already excludes the status bar (the wildmenu's extra row included), the
/// command line, quickfix, the terminal/debug panel, the debug toolbar and the
/// terminal-separated status line — less the active window's own status row,
/// which sits *inside* the band at its bottom edge whenever per-window status
/// lines are on and the bottom panel has not moved it into its own band.
///
/// All of that precision is load-bearing: `FRAME_Z_ORDER` composes the chrome
/// rungs *before* [`FrameOp::UnifiedPicker`] and the editor band earlier still,
/// and on TUI the scrim fill is destructive (see [`paint_picker_rung`]), so a
/// rect reaching one row too far erases text that already painted this frame.
/// Neither the shell's *static* two-row reservation
/// (`main_content_bounds.height`) nor the status bar's own top edge is a safe
/// substitute: the first is one row short whenever the wildmenu is open, and
/// the second sits below the quickfix/terminal/per-window-status rows.
pub fn modal_scrim_rect(
    engine: &Engine,
    el: &EditorLayout,
    window_width: f32,
    content_top: f32,
    line_height: f64,
) -> quadraui::Rect {
    // Zero when the bottom panel pushed the status line below it: that row is
    // then `el.separated_status_h`, already outside `editor_bottom`.
    let window_status_row = if el.separated_status_h == 0.0 && effective_window_status_line(engine)
    {
        line_height
    } else {
        0.0
    };
    let height = (el.editor_bottom - window_status_row).max(0.0) as f32;
    quadraui::Rect::new(0.0, content_top, window_width, height)
}

/// The [`FrameOp::UnifiedPicker`] rung's whole body on both backends.
///
/// `viewport` is the pane the picker floats over (the editor area, not the
/// whole window), so the popup stays clear of the sidebar and activity bar.
///
/// Returns the popup rect it resolved and painted into, so GTK can cache it
/// for click/drag hit-testing; TUI recomputes the identical
/// geometry cheaply at click time instead and ignores the return value.
///
/// Paints [`MODAL_SCRIM_COLOR`] into `scrim_rect` before the popup itself, via
/// `Backend::draw_solid_fill` — the one rect-fill primitive every backend
/// implements — so the editor/sidebar behind the picker dim on every backend
/// with zero per-backend code here. `scrim_rect` is the caller's own concern:
/// it must cover only the editor/sidebar band, never a chrome rung
/// ([`FrameOp::StatusBar`], [`FrameOp::CommandLine`], [`FrameOp::MenuRow`])
/// that already painted earlier this frame — `FRAME_Z_ORDER` composes those
/// first, and on TUI `Backend::draw_solid_fill` overwrites every cell in its
/// rect with a blank glyph rather than blending an alpha channel, so handing
/// it a chrome rung's rect would erase that rung's text instead of dimming
/// it. `viewport` (the popup's own anchor rect) is unaffected by that and
/// stays full-window, since `PickerGeometry::compute` centres the popup
/// against the whole window regardless of how much of it the scrim covers.
pub fn paint_picker_rung(
    b: &mut dyn quadraui::Backend,
    picker: &PickerPanel,
    viewport: quadraui::Rect,
    scrim_rect: quadraui::Rect,
    sizing: &PickerSizing,
    theme: &Theme,
) -> quadraui::Rect {
    b.draw_solid_fill(scrim_rect, MODAL_SCRIM_COLOR);
    let has_preview = picker.preview.is_some();
    let geo = PickerGeometry::compute(viewport, picker.items.len(), has_preview, sizing);
    let palette = picker_panel_to_palette(picker, theme);
    let rect = quadraui::Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h);
    // Translucent backdrop for this popup only: save the backend's style, draw
    // the palette with a reduced `backdrop_opacity`, then restore so later
    // overlays in the same frame (e.g. the git branch picker) keep their
    // opaque default.
    b.draw_palette(rect, &palette);
    rect
}

/// Map vimcode's own [`crate::core::settings::MenuStyle`] onto
/// [`quadraui::MenuStyle`]. The two enums are
/// deliberately defined with the same three variants (`Auto`/`Native`/
/// `Custom`) so this is a straight 1:1 translation with no capability
/// logic of its own — [`quadraui::Backend::effective_menu_style`] (built
/// from [`quadraui::MenuStyle::resolve`] against
/// [`quadraui::BackendCaps::native_menu`]) does that, and is what callers
/// should consult instead of re-deriving a native/custom decision here.
pub fn to_quadraui_menu_style(style: crate::core::settings::MenuStyle) -> quadraui::MenuStyle {
    use crate::core::settings::MenuStyle;
    match style {
        MenuStyle::Auto => quadraui::MenuStyle::Auto,
        MenuStyle::Native => quadraui::MenuStyle::Native,
        MenuStyle::Custom => quadraui::MenuStyle::Custom,
    }
}

/// Build the render-side [`ContextMenuPanel`] from the engine's own
/// [`crate::core::engine::ContextMenuState`] — the same conversion the
/// `ScreenLayout` builder uses every frame, factored out so
/// [`show_context_menu_now`]'s event-handler-time caller can build the
/// identical value without waiting for a render pass.
pub fn context_menu_state_to_panel(cm: &crate::core::engine::ContextMenuState) -> ContextMenuPanel {
    ContextMenuPanel {
        items: cm
            .items
            .iter()
            .map(|item| ContextMenuRenderItem {
                label: item.label.clone(),
                shortcut: item.shortcut.clone(),
                separator_after: item.separator_after,
                enabled: item.enabled,
            })
            .collect(),
        selected_idx: cm.selected,
        screen_col: cm.screen_x,
        screen_row: cm.screen_y,
        trigger_height: cm.trigger_height,
    }
}

/// Show `panel` as a native OS context-menu popup right now, through
/// quadraui's single-call [`quadraui::ContextMenuController::open`]
/// — the event-handler-time counterpart to
/// [`paint_context_menu_rung`]'s render-time `Custom` half.
///
/// Callers **must** invoke this from event-handling code (typically right
/// after `Engine::open_*_context_menu`), never from `render_content`'s
/// paint rung: on the `Native` resolution, `ContextMenuController::open`
/// calls `Backend::show_context_menu`, which blocks on AppKit's modal
/// popup loop — running that from inside a paint closure re-enters
/// painting while the closure still holds borrows it needs to finish its
/// own frame. That re-entrancy was the macOS right-click bug this
/// function's introduction fixed (a queued
/// `UiEvent::ContextMenuItemActivated` that only got handled on the
/// *next* repaint, when it was handled at all; see `App::handle`'s
/// `open_context_menu_now_if_native`, the only call site, which already
/// confirms `effective_menu_style()` is `Native` before calling this, so
/// the controller's own resolve here is a second, harmless check rather
/// than the deciding one).
///
/// A fresh, throwaway [`quadraui::ContextMenuController`] is correct here
/// rather than a field kept on `App`: its `Native` branch (`self.close`,
/// then `backend.show_context_menu`) never touches the controller's own
/// `open` state, so there is nothing to leak when this transient instance
/// is dropped at the end of the call. A persistent controller would only
/// matter for driving the `Custom` branch too, which vimcode does not
/// route through this controller — see [`paint_context_menu_rung`]'s doc
/// for the capability gap (no `MouseMove` hover) that keeps the in-window
/// path on vimcode's own state for now.
///
/// Uses the same `ContextMenu`/`WidgetId`s [`paint_context_menu_rung`]'s
/// in-window path does, so `UiEvent::ContextMenuItemActivated` resolves
/// through the exact same `context_menu_hit_to_idx` conversion either
/// path takes.
pub fn show_context_menu_now(
    b: &mut dyn quadraui::Backend,
    panel: &ContextMenuPanel,
    char_width: f64,
    line_height: f64,
) {
    let menu = context_menu_panel_to_quadraui_context_menu(panel);
    let anchor = context_menu_anchor_point(panel, char_width, line_height);
    quadraui::ContextMenuController::new().open(menu, anchor, b);
}

/// The [`FrameOp::ContextMenu`] rung's whole body on both backends —
/// the in-window (`Custom`-resolved) painted path only. Callers
/// must check `backend.effective_menu_style()` themselves and skip this
/// entirely when it resolves `Native`.
///
/// `viewport`/`char_width`/`line_height`/`border_chrome_inset` are exactly
/// [`context_menu_generic_layout`]'s parameters. TUI's `+1`-inset viewport and
/// panel (to make room for the ASCII border) is per-backend geometry prep and
/// stays at the call site, not here.
///
/// # Why this stays vimcode's own state instead of quadraui controller
///
/// [`quadraui::ContextMenuController`] matches only `KeyPressed(Escape/Down/Up/
/// Enter)` and `MouseDown` — there is no `MouseMove` arm — so it cannot
/// host vimcode's tested hover-follows-pointer behaviour. The highlighted row
/// must move as the mouse moves over it, before any click. Once quadraui grows a
/// `MouseMove` hover arm for `ContextMenuController::handle`, this rung, the
/// click routing, and related state can fold into the controller.
pub fn paint_context_menu_rung(
    b: &mut dyn quadraui::Backend,
    panel: &ContextMenuPanel,
    viewport: quadraui::Rect,
    char_width: f64,
    line_height: f64,
    border_chrome_inset: f64,
) -> quadraui::ContextMenuLayout {
    let (menu, layout) = context_menu_generic_layout(
        panel,
        viewport,
        char_width,
        line_height,
        border_chrome_inset,
    );
    let _ = b.draw_context_menu(&menu, &layout);
    layout
}

/// The [`FrameOp::Dialog`] rung's whole body on both backends.
///
/// `char_width`/`line_height` are [`dialog_generic_layout`]'s — GTK passes
/// its measured pixel metrics, TUI passes `1.0`/`1.0` for one cell.
pub fn paint_dialog_rung(
    b: &mut dyn quadraui::Backend,
    panel: &DialogPanel,
    viewport: quadraui::Rect,
    char_width: f64,
    line_height: f64,
) -> quadraui::DialogLayout {
    let (dialog, layout) = dialog_generic_layout(panel, viewport, char_width, line_height);
    let _ = b.draw_dialog(&dialog, &layout);
    layout
}

/// The [`FrameOp::ToastStack`] rung's whole body on both backends.
pub fn paint_toast_stack_rung(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    stack: &quadraui::ToastOverlay,
    viewport: quadraui::Rect,
) {
    let layout = b.draw_toast_overlay(viewport, stack);
    engine.toast_layout.replace(Some(layout));
}

/// The change-review surface's modal-stack id — see
/// [`reconcile_change_review_modal_stack`].
fn change_review_modal_id() -> quadraui::WidgetId {
    quadraui::WidgetId::new("change_review")
}

/// Keep the change-review surface's full-viewport bounds on the backend's
/// `quadraui::ModalStack` in step with whether it's open. Same "reconcile so
/// chrome hit-testing yields to an open overlay" pattern used elsewhere.
///
/// Unlike other reconcile blocks, this must run from *paint*, not from
/// event handlers: TUI's `ShellAdapter::handle` consults the modal stack
/// **before** its own activity-bar/sidebar hit-test, and before ever calling
/// into `ShellApp::handle` (where `handle_mouse` lives) — so a stack entry
/// written only while a mouse event is *already* being dispatched can
/// never be there in time for the very first click after the surface opens.
///
/// Registers the *whole* `viewport` (diff pane + status footer), not just
/// the diff pane: a click on the footer must also bypass chrome rather than
/// being swallowed by whatever activity-bar icon happens to occupy that row
/// underneath.
///
/// **Call this unconditionally, once per frame, *before* the
/// [`compose_frame`] walk** — never from an `else` arm inside the
/// `FrameOp::ChangeReview` match arm. An `else` there is dead code on exactly
/// the frame that needs the pop, leaving the entry pushed after the surface
/// closes and silently killing activity-bar panel switching for the remainder
/// of the session.
pub fn reconcile_change_review_modal_stack(
    b: &mut dyn quadraui::Backend,
    open: bool,
    viewport: quadraui::Rect,
) {
    let stack_rc = b.modal_stack_handle();
    let mut stack = stack_rc.borrow_mut();
    if open {
        stack.push(change_review_modal_id(), viewport);
        // No quadraui rasteriser marks this surface painted — the
        // three `mark_painted` call sites upstream are wired only for
        // `draw_palette`/`draw_menu`/`draw_dialog`, and the change-review
        // surface is a `DiffView` + `StatusBar`, neither of which is
        // modal-capable there. Without this the entry shows up in
        // `ModalStack::unpainted_ids()` every frame it is open and each
        // backend's `end_frame` emits the "registered but invisible"
        // diagnostic against a surface that is, in fact, painted. Only the
        // `open` arm marks: a popped id is a no-op for `mark_painted`
        // anyway.
        stack.mark_painted(&change_review_modal_id());
    } else {
        stack.pop(&change_review_modal_id());
    }
}

/// The change-review surface's whole paint body: a full-viewport
/// `quadraui::DiffView` for the currently-shown entry, plus a one-row status
/// footer ("file i of n", the entry's path, and key hints). Both backends call
/// this verbatim from their own frame-op walk.
///
/// Caches the diff pane's own rect on `engine.change_review_diff_rect` so
/// a later mouse click can resolve through the *exact* geometry this call
/// painted.
///
/// Also reconciles the surface's modal-stack entry (see
/// [`reconcile_change_review_modal_stack`]) — required for click-to-jump
/// to actually reach the mouse handler rather than being swallowed by whatever
/// chrome happens to occupy those columns underneath the full-viewport overlay.
pub fn paint_change_review_rung(
    b: &mut dyn quadraui::Backend,
    engine: &Engine,
    review: &crate::core::review::ChangeReviewState,
    viewport: quadraui::Rect,
    theme: &Theme,
) {
    let Some(entry) = review.current_entry() else {
        // Open-but-empty: nothing reaches the canvas, so nothing may claim the
        // viewport's clicks either.
        reconcile_change_review_modal_stack(b, false, viewport);
        return;
    };
    reconcile_change_review_modal_stack(b, true, viewport);
    let line_height = b.line_height().max(1.0);
    let footer_h = line_height.min(viewport.height);
    let diff_rect = quadraui::Rect::new(
        viewport.x,
        viewport.y,
        viewport.width,
        (viewport.height - footer_h).max(0.0),
    );
    // Splice any pinned line comments into the rows that get painted
    // — see `crate::core::review::view_with_inline_comments`'s doc for why
    // this lives in core rather than as backend-specific text munging.
    let painted_view = crate::core::review::view_with_inline_comments(&review.comments, entry);
    let _ = b.draw_diff_view(diff_rect, &painted_view);
    engine.change_review_diff_rect.set(diff_rect);

    let decision = match entry.decision {
        crate::core::review::ChangeDecision::Pending => "pending",
        crate::core::review::ChangeDecision::Accepted => "accepted",
        crate::core::review::ChangeDecision::Rejected => "rejected",
    };
    // A turn review's `a`/`r` act on the hunk under the cursor, not the whole
    // file. A branch/proposal review keeps the whole-file-only legend, since its
    // `a`/`r` still mean exactly that.
    let edited_by_you = if engine.turn_review_checkpoint_id.is_some() {
        " \u{b7} edited by you"
    } else {
        ""
    };
    let msg = if engine.turn_review_checkpoint_id.is_some() {
        format!(
            " Change {}/{} ({decision}){} \u{b7} {} \u{b7} a=keep-hunk r=revert-hunk \
             A=keep-file R=revert-file ga/gr=all c=comment d=del-comment ]/[=hunk n/p=file \
             Esc=close ",
            review.current + 1,
            review.entries.len(),
            if engine.current_turn_hunk_edited_by_human() {
                edited_by_you
            } else {
                ""
            },
            entry.change.path,
        )
    } else {
        format!(
            " Change {}/{} ({decision}) \u{b7} {} \u{b7} a=accept r=reject c=comment d=del-comment \
             ]/[=hunk n/p=file Esc=close ",
            review.current + 1,
            review.entries.len(),
            entry.change.path,
        )
    };
    // Provenance: a git-branch-fed review (`Engine::
    // open_branch_review`) is showing the human a diff *of a real branch
    // checked out right here* — the footgun the issue calls out is a human
    // who edits/finalizes believing this is their own checkout. Right-
    // aligned so it never competes with the left segment's file/decision
    // info for the narrow-terminal case. `None` (an ACP tool-call-fed
    // review has no branch) means no segment at all, not a blank one.
    let right_segments = branch_review_provenance_segment(engine, theme)
        .into_iter()
        .collect();
    let status = quadraui::StatusBar {
        id: quadraui::WidgetId::new("change-review-status"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: msg,
            fg: theme.status_fg,
            bg: theme.status_bg,
            bold: false,
            action_id: None,
        }],
        right_segments,
    };
    let footer_rect = quadraui::Rect::new(
        viewport.x,
        diff_rect.y + diff_rect.height,
        viewport.width,
        footer_h,
    );
    let _ = b.draw_status_bar_interactive(footer_rect, &status, &quadraui::InteractionState::new());
}

/// Build the "you are editing a real branch checkout, not your own" status
/// segment for a branch-fed change review — `None` when `engine.review_target`
/// is unset.
fn branch_review_provenance_segment(
    engine: &Engine,
    theme: &Theme,
) -> Option<quadraui::StatusBarSegment> {
    let target = engine.review_target.as_ref()?;
    let root = engine
        .workspace_root
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "?".to_string());
    let text = match &target.host {
        Some(host) => format!(
            " reviewing branch '{}' on '{}' in {} ",
            target.branch, host, root
        ),
        None => format!(" reviewing branch '{}' in {} ", target.branch, root),
    };
    Some(quadraui::StatusBarSegment {
        text,
        fg: theme.status_fg,
        bg: theme.status_bg,
        bold: true,
        action_id: None,
    })
}

/// Where the menu bar's labels end, in absolute coordinates.
///
/// `vi.bounds.x` is already absolute — quadraui's `MenuBar::layout` starts its
/// cursor at the `bounds.x` it was handed — so adding the band's own `x` back
/// on double-counts it. `fallback_x` is what an item-less bar returns: the
/// leading edge the items *would* have started at.
pub fn menu_bar_items_end(layout: &quadraui::MenuBarLayout, fallback_x: f32) -> f32 {
    layout
        .visible_items
        .last()
        .map(|vi| vi.bounds.x + vi.bounds.width)
        .unwrap_or(fallback_x)
}

/// The min-gap [`measure_title_bar_bands`] reserves between the Command
/// Center and the inline window-control buttons, matching every
/// quadraui backend that supplies a `controls_bar`'s own `StatusBar::layout`
/// min-gap (`quadraui::gtk::MIN_GAP_PX`, `quadraui::win::MIN_GAP_DIP` — both
/// `16.0` at the pinned rev). See that function's doc for why a *second*
/// copy of this number has to live here rather than importing the `gtk`
/// one.
const TITLE_BAR_CONTROLS_MIN_GAP_PX: f32 = 16.0;

/// Total slack [`measure_title_bar_bands`] reserves to the *left* of the
/// window-control buttons' measured leading edge, so that repainting them
/// inside the narrowed `controls` rect doesn't re-trigger `StatusBar::layout_padded`'s
/// priority-drop and silently lose minimize.
///
/// Derivation — with `E` = [`quadraui::primitives::status_bar::PIXEL_EDGE_INSET`],
/// `G` = [`TITLE_BAR_CONTROLS_MIN_GAP_PX`] and `T` the buttons' total
/// padded width:
///
/// * The wide-band measurement puts the leftmost button at `W - E - T`.
/// * The repaint at `bar_width = B` keeps every button only while
///   `B - left_w - G - E >= T`. The repaint needs `B >= T + 2E + G`,
///   i.e. `E + G` more than the measurement reserved.
/// * One further `E` on top of that minimum keeps the comparison off the
///   knife edge: `B` is derived by subtracting from the band's absolute `W`,
///   where f32 has ~1e-4 resolution and an exact-fit `B` can round under
///   `T + 2E + G` and drop minimize anyway. The cost is 10px of Command
///   Center width; the benefit is that the fit no longer depends on `W`'s
///   rounding.
const TITLE_BAR_CONTROLS_SLACK_PX: f32 =
    TITLE_BAR_CONTROLS_MIN_GAP_PX + 2.0 * quadraui::primitives::status_bar::PIXEL_EDGE_INSET;

/// How the title-bar band divides up to the right of the last menu label.
///
/// Both bands are *measured*, never painted — see [`FrameOp::MenuRow`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TitleBarBands {
    /// The inline window-control buttons (GTK only; zero-width on TUI, which
    /// has no in-canvas window controls).
    pub controls: quadraui::Rect,
    /// The VS Code-style Command Center — everything between the last menu
    /// label and the window controls.
    pub command_center: quadraui::Rect,
}

/// Measure the title-bar band's two right-hand slots, in the caller's units.
///
/// The [`FrameOp::MenuRow`] rung's whole body on both backends. `band` is the
/// shell-reserved row (`AppShellLayout::title_bar_bounds`); `items` is the part
/// of it the menu labels are laid out in.
///
/// `controls_bar` is `Some` only on GTK. With `None` the controls band collapses
/// to zero width at the band's right edge and the Command Center takes
/// everything from the last menu label onward.
pub fn measure_title_bar_bands(
    backend: &mut dyn quadraui::Backend,
    band: quadraui::Rect,
    items: quadraui::Rect,
    menu_bar: &quadraui::MenuBar,
    controls_bar: Option<&quadraui::StatusBar>,
) -> TitleBarBands {
    let mb_layout = backend.menu_bar_layout(items, menu_bar);
    // An item-less bar still starts *after* the app-icon slot, so the
    // Command Center / window controls must not be handed the icon's real
    // estate back — hence `items.x`, not `band.x`, as the fallback.
    let menu_end = menu_bar_items_end(&mb_layout, items.x);
    let full = quadraui::Rect::new(
        menu_end,
        band.y,
        (band.x + band.width - menu_end).max(0.0),
        band.height,
    );
    // Narrow the controls band to the buttons' *actual* painted width
    // instead of handing them the entire menu_end→right-edge strip. That full
    // strip, background-filled end-to-end by `window_controls_status_bar`, is
    // exactly what ate the Command Center's real estate. `status_bar_layout`
    // mirrors `draw_status_bar`'s own measurement, so this is the width the
    // buttons paint at.
    let controls_start = controls_bar
        .map(|cb| {
            backend
                .status_bar_layout(full, cb)
                .visible_segments
                .iter()
                .map(|vs| vs.bounds.x)
                .fold(f32::INFINITY, f32::min)
        })
        .filter(|s| s.is_finite())
        // The value above is measured against the *wide* `full` band,
        // where `StatusBar::layout_padded`'s `bar_width - left_w - min_gap
        // - edge_inset` never goes negative and all three buttons come back
        // — but `controls` below is narrowed to (nearly) that painted width,
        // and it is *that* narrower rect `paint_title_bar_band` hands to
        // `Backend::draw_status_bar_interactive` for the actual paint.
        // `window_controls_status_bar` has no left segments, so re-laying-out
        // at a `bar_width` that only covers the buttons makes the identical
        // subtraction go negative and silently drops the front (lowest-
        // priority) segment — minimize — even though it fit a moment ago.
        // [`TITLE_BAR_CONTROLS_SLACK_PX`] is the slack that keeps the repaint
        // from re-triggering that drop; its doc derives the number. Every
        // backend that supplies a `controls_bar` (GTK, Win-GUI) reserves the
        // same 16px/16dip min-gap convention (`quadraui::gtk::MIN_GAP_PX`,
        // `quadraui::win::MIN_GAP_DIP`) — restated as a plain constant, not
        // imported, so this stays buildable without the `gui` feature
        // (`quadraui::gtk` is gated on `quadraui/gtk`, `render.rs` is not).
        .map(|s| (s - TITLE_BAR_CONTROLS_SLACK_PX).max(0.0))
        .unwrap_or(full.width);
    let controls = quadraui::Rect::new(
        full.x + controls_start,
        full.y,
        (full.width - controls_start).max(0.0),
        full.height,
    );
    TitleBarBands {
        command_center: quadraui::Rect::new(
            menu_end,
            band.y,
            (controls.x - menu_end).max(0.0),
            band.height,
        ),
        controls,
    }
}

/// The expected chrome rungs for the cross-backend fixture used by
/// `chrome_band_composes_in_canonical_order_via_gtk_driver` and
/// `..._via_shell_app`: menu bar visible, sidebar open, per-window status lines
/// off (so a global status bar exists), and `wildmenu` deciding whether the
/// Tab-completion bar is up.
///
/// Both backend tests call this one function — a single `#[cfg(test)]` fn in
/// `render.rs`, compiled into both bin targets — rather than each transcribing
/// its own `Vec<FrameOp>` literal, so the compiler keeps the two expectations
/// in step. Taking `wildmenu` as a parameter keeps the expectation
/// *discriminating*: it is not simply "whatever [`FRAME_Z_ORDER`] contains".
///
/// Chrome rungs only — the overlay tail is filtered out, because these two
/// tests pin the chrome half and leave the overlay half to
/// [`frame_sequence_fixture`].
#[cfg(test)]
pub(crate) fn chrome_band_fixture(wildmenu: bool) -> Vec<FrameOp> {
    FRAME_Z_ORDER
        .iter()
        .copied()
        .filter(|op| !op.is_overlay())
        .filter(|op| wildmenu || !matches!(op, FrameOp::Wildmenu))
        .collect()
}

/// The `quadraui::CommandLine` descriptor for this frame's Vim command line.
///
/// GTK's [`FrameOp::CommandLine`] rung and TUI's `panels::render_command_line`
/// both build the descriptor this way and hand it (plus `cmd_sel`, converted
/// via [`command_line_selection_bytes`]) to `Backend::
/// draw_command_line_selection` — the *descriptor* is app state, not
/// geometry, and belongs beside the rest of the chrome band rather than
/// buried in either backend.
pub fn command_line_view(command: &CommandLineData) -> quadraui::CommandLine {
    quadraui::CommandLine {
        id: "cmd".into(),
        text: command.text.clone(),
        cursor_offset: if command.show_cursor {
            Some(command.cursor_anchor_text.len())
        } else {
            None
        },
        right_align: command.right_align,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `window_status_row_reserved` is the only place `build_screen_layout`
    /// decides whether a window paints its own bottom-row status line. Pin every
    /// axis this function depends on: the base setting, `terminal_maximized`,
    /// and the `status_line_above_terminal`/bottom-panel combination that
    /// produces a *separated* status bar instead.
    #[test]
    fn window_status_row_reserved_covers_every_axis() {
        use crate::core::engine::Engine;

        let base = || {
            let mut e = Engine::new_for_test();
            e.settings.window_status_line = true;
            e
        };

        // Setting off → never reserved, regardless of anything else.
        let mut e = base();
        e.settings.window_status_line = false;
        assert!(!window_status_row_reserved(&e), "setting off");

        // Setting on, nothing else in play → reserved.
        let e = base();
        assert!(
            window_status_row_reserved(&e),
            "plain per-window status must reserve its own row"
        );

        // Terminal maximized → editor windows aren't the visible surface;
        // GTK's old predicate caught this, `build_screen_layout`'s didn't.
        let mut e = base();
        e.terminal_maximized = true;
        assert!(
            !window_status_row_reserved(&e),
            "a maximized terminal panel must suppress the per-window row"
        );

        // status_line_above_terminal ON (default) with the bottom panel
        // open: per-window status bars stay inside each window (still
        // "naturally above" the terminal), so the row is still reserved.
        let mut e = base();
        e.settings.status_line_above_terminal = true;
        e.terminal_open = true;
        assert!(
            window_status_row_reserved(&e),
            "status_line_above_terminal keeps the status row inside the window"
        );

        // status_line_above_terminal OFF with the bottom panel open: the
        // active window's status is pulled into a *separated* bar above the
        // terminal instead, freeing this row. GTK's old predicate
        // (`window_status_line && !terminal_maximized`) missed this axis
        // entirely and would have reported "reserved" here.
        let mut e = base();
        e.settings.status_line_above_terminal = false;
        e.terminal_open = true;
        assert!(
            !window_status_row_reserved(&e),
            "separated status must free the window's own bottom row"
        );

        // Same, but via the non-terminal bottom panel (debug output etc.)
        // rather than the terminal specifically.
        let mut e = base();
        e.settings.status_line_above_terminal = false;
        e.bottom_panel_open = true;
        assert!(
            !window_status_row_reserved(&e),
            "any open bottom panel — not just the terminal — must separate the status"
        );

        // status_line_above_terminal OFF but nothing open at the bottom:
        // `separate_status` requires `bottom_panel_open`, so the row stays
        // reserved in-window.
        let e = {
            let mut e = base();
            e.settings.status_line_above_terminal = false;
            e
        };
        assert!(
            window_status_row_reserved(&e),
            "no bottom panel open → nothing to separate the status from"
        );
    }

    /// Across every combination of the four settings `window_status_row_reserved`
    /// depends on, both backends must agree about whether a window's bottom row
    /// is free.
    #[test]
    fn window_status_row_reserved_is_deterministic_across_all_combinations() {
        use crate::core::engine::Engine;

        for window_status_line in [false, true] {
            for status_line_above_terminal in [false, true] {
                for bottom_panel_open in [false, true] {
                    for terminal_maximized in [false, true] {
                        let mut e = Engine::new_for_test();
                        e.settings.window_status_line = window_status_line;
                        e.settings.status_line_above_terminal = status_line_above_terminal;
                        e.bottom_panel_open = bottom_panel_open;
                        e.terminal_maximized = terminal_maximized;

                        let expected = window_status_line
                            && !terminal_maximized
                            && !(!status_line_above_terminal && bottom_panel_open);
                        assert_eq!(
                            window_status_row_reserved(&e),
                            expected,
                            "window_status_line={window_status_line} \
                             status_line_above_terminal={status_line_above_terminal} \
                             bottom_panel_open={bottom_panel_open} \
                             terminal_maximized={terminal_maximized}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_status_segments_have_actions() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // Right segments should include GoToLine on cursor position
        let goto = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::GoToLine));
        assert!(goto.is_some(), "expected GoToLine action on Ln/Col segment");

        // Right segments should include ChangeIndentation
        let indent = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeIndentation));
        assert!(
            indent.is_some(),
            "expected ChangeIndentation action on indent segment"
        );

        // Right segments should include ChangeEncoding
        let enc = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeEncoding));
        assert!(enc.is_some(), "expected ChangeEncoding action");

        // Right segments should include ChangeLineEnding
        let eol = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeLineEnding));
        assert!(eol.is_some(), "expected ChangeLineEnding action");

        // Inactive window segments should have no actions
        let inactive = build_window_status_line(&engine, &theme, wid, false);
        for seg in inactive
            .left_segments
            .iter()
            .chain(inactive.right_segments.iter())
        {
            assert_eq!(seg.action, None, "inactive segments should have no actions");
        }
    }

    /// The problems counter must always paint, even with zero
    /// diagnostics — VS Code shows `0`/`0`, not nothing, and the previous
    /// per-window bar had no counter segment at all (only the older,
    /// non-window `build_status_line`/`build_global_status_bar` had a
    /// diagnostics blob, and it hid entirely at zero). RED against the
    /// pre-fix body (no errors/warnings segments in `right`): this search
    /// for a `ShowDiagnostics`-actioned segment found nothing and the
    /// assertion failed — confirmed by reverting the `errors_seg`/
    /// `warnings_seg` push and re-running.
    #[test]
    fn test_window_status_line_problems_counter_always_shown_at_zero() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.buffer_mut().insert(0, "hello\n");

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        // The problems counter moved to `left_segments` (VS Code
        // parity — far left of the bar, not mixed into the right cluster).
        let diag_segs: Vec<&StatusSegment> = status
            .left_segments
            .iter()
            .filter(|s| s.action == Some(StatusAction::ShowDiagnostics))
            .collect();
        assert_eq!(
            diag_segs.len(),
            2,
            "expected an error segment and a warning segment, both clickable"
        );
        let combined: String = diag_segs.iter().map(|s| s.text.clone()).collect();
        assert!(
            combined.contains(crate::icons::STATUS_ERROR.s()),
            "expected the error icon even at zero, got '{combined}'"
        );
        assert!(
            combined.contains(crate::icons::STATUS_WARNING.s()),
            "expected the warning icon even at zero, got '{combined}'"
        );
        assert!(
            combined.contains('0'),
            "expected a zero count painted, got '{combined}'"
        );
    }

    /// Once diagnostics exist, the painted counts must reflect them
    /// (not just prove the segment exists at zero, above).
    #[test]
    fn test_window_status_line_problems_counter_reflects_diagnostic_counts() {
        use crate::core::engine::Engine;
        use crate::core::lsp::{Diagnostic, DiagnosticSeverity, LspRange};
        use std::path::PathBuf;

        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.active_buffer_state_mut().file_path = Some(PathBuf::from("/tmp/test_1548.rs"));
        engine.lsp_diagnostics.insert(
            PathBuf::from("/tmp/test_1548.rs"),
            vec![
                Diagnostic {
                    range: LspRange::default(),
                    severity: DiagnosticSeverity::Error,
                    message: "e1".to_string(),
                    source: None,
                    code: None,
                },
                Diagnostic {
                    range: LspRange::default(),
                    severity: DiagnosticSeverity::Error,
                    message: "e2".to_string(),
                    source: None,
                    code: None,
                },
                Diagnostic {
                    range: LspRange::default(),
                    severity: DiagnosticSeverity::Warning,
                    message: "w1".to_string(),
                    source: None,
                    code: None,
                },
            ],
        );

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);
        // See the sibling "always shown at zero" test above for why
        // this reads `left_segments` now.
        let combined: String = status
            .left_segments
            .iter()
            .filter(|s| s.action == Some(StatusAction::ShowDiagnostics))
            .map(|s| s.text.clone())
            .collect();
        assert!(
            combined.contains(&format!("{} 2", crate::icons::STATUS_ERROR.s())),
            "expected 2 errors painted, got '{combined}'"
        );
        assert!(
            combined.contains(&format!("{} 1", crate::icons::STATUS_WARNING.s())),
            "expected 1 warning painted, got '{combined}'"
        );
    }

    /// Clicking the problems counter opens the workspace-wide
    /// Problems (quickfix) list. RED against the pre-fix `StatusAction`
    /// enum (no `ShowDiagnostics` variant existed, so this action id could
    /// not resolve at all) — confirmed by reverting the enum + dispatch
    /// addition and re-running.
    #[test]
    fn test_status_action_show_diagnostics_opens_quickfix() {
        use crate::core::engine::Engine;
        use crate::core::lsp::{Diagnostic, DiagnosticSeverity, LspRange};
        use std::path::PathBuf;

        let mut engine = Engine::new();
        engine.lsp_diagnostics.insert(
            PathBuf::from("/tmp/test_1548_qf.rs"),
            vec![Diagnostic {
                range: LspRange::default(),
                severity: DiagnosticSeverity::Error,
                message: "boom".to_string(),
                source: None,
                code: None,
            }],
        );

        engine.handle_status_action(&StatusAction::ShowDiagnostics);

        assert!(engine.quickfix.open, "expected the quickfix list to open");
        assert_eq!(engine.quickfix.items.len(), 1);
        assert!(engine.quickfix.items[0].line_text.contains("boom"));
    }

    /// The encoding segment must read `UTF-8`, matching VS Code's casing.
    #[test]
    fn test_window_status_line_encoding_label_is_uppercase() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        let enc = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeEncoding))
            .expect("expected a ChangeEncoding segment");
        assert_eq!(enc.text, "UTF-8 ");
    }

    /// The language segment must show the display name ("Rust"),
    /// not the internal LSP id ("rust").
    #[test]
    fn test_window_status_line_language_segment_uses_display_name() {
        use crate::core::engine::Engine;

        // Unique per (process, thread) so concurrent test runs never collide.
        let rs_path = std::env::temp_dir().join(format!(
            "vimcode_status_bar_1548_{}_{:?}.rs",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&rs_path, "fn main() {}\n").unwrap();

        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        let _ = engine.open_file_with_mode(&rs_path, crate::core::engine::OpenMode::Permanent);

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        let lang = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeLanguage))
            .expect("expected a ChangeLanguage segment");
        assert_eq!(lang.text, "Rust ");
        assert!(
            !lang.text.contains("rust"),
            "must not leak the internal lowercase id"
        );

        let _ = std::fs::remove_file(&rs_path);
    }

    #[test]
    fn language_display_name_maps_known_ids_and_falls_back_gracefully() {
        assert_eq!(language_display_name("rust"), "Rust");
        assert_eq!(language_display_name("typescriptreact"), "TypeScript React");
        assert_eq!(language_display_name("csharp"), "C#");
        // Unknown id: soft fallback, not a blank string.
        assert_eq!(language_display_name("brainfuck"), "Brainfuck");
    }

    /// The branch segment must be prefixed with the branch icon via
    /// the shared `Icon` constant, not painted as bare text. RED against
    /// the pre-fix body (`format!("  {}", branch_text)`, no icon at all).
    #[test]
    fn test_window_status_line_branch_segment_has_icon_prefix() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.git_branch = Some("main".to_string());

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        let branch = status
            .left_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::SwitchBranch))
            .expect("expected a SwitchBranch segment");
        assert!(
            branch.text.contains(crate::icons::GIT_BRANCH.s()),
            "expected the branch icon in '{}'",
            branch.text
        );
        assert!(branch.text.contains("main"));
    }

    #[test]
    fn test_status_line_ending_segment() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        // Default is LF
        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);
        let eol_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeLineEnding))
            .expect("expected line ending segment");
        assert!(
            eol_seg.text.contains("LF"),
            "expected LF, got '{}'",
            eol_seg.text
        );
    }

    #[test]
    fn test_status_indentation_segment() {
        use crate::core::engine::Engine;
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.settings.expand_tab = true;
        engine.settings.tabstop = 4;

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);
        let indent_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ChangeIndentation))
            .expect("expected indent segment");
        assert!(
            indent_seg.text.contains("Spaces: 4"),
            "expected 'Spaces: 4', got '{}'",
            indent_seg.text
        );
    }

    /// The sidebar/panel/menu-bar status-bar toggles and the done-notification
    /// bell must be sourced from `crate::icons::STATUS_*` constants, matching
    /// what the font coverage tracking uses. Asserts the *rendered segment text*
    /// matches what the tracked constants resolve to, in both the Nerd Font
    /// and ASCII fallback cases.
    #[test]
    fn test_status_bar_toggle_and_bell_glyphs_come_from_icons_rs_constants() {
        use crate::core::engine::Engine;

        // ── Nerd Fonts on (default) ──────────────────────────────────────
        crate::icons::set_nerd_fonts(true);
        let mut engine = Engine::new();
        engine.settings.window_status_line = true;
        engine.menu_bar_toggleable = true;
        let notif_id = engine.notify(crate::core::engine::NotificationKind::GitOperation, "done");
        engine.notify_done(notif_id, None);

        let theme = Theme::onedark();
        let wid = engine.active_window_id();
        let status = build_window_status_line(&engine, &theme, wid, true);

        let panel_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::TogglePanel))
            .expect("expected panel toggle segment");
        assert_eq!(
            panel_seg.text,
            format!(" {} ", crate::icons::STATUS_PANEL_TOGGLE.s()),
            "panel toggle segment text must come from STATUS_PANEL_TOGGLE"
        );
        assert!(
            panel_seg
                .text
                .contains(crate::icons::STATUS_PANEL_TOGGLE.nerd),
            "expected the tracked STATUS_PANEL_TOGGLE nerd glyph, got '{}'",
            panel_seg.text
        );

        let sidebar_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ToggleSidebar))
            .expect("expected sidebar toggle segment");
        // Trailing space restored — `sidebar_toggle_seg` is no
        // longer (even by default) the bar's right-most segment now that
        // `cursor_seg` unconditionally is; see its own doc in
        // `build_window_status_line`.
        assert_eq!(
            sidebar_seg.text,
            format!(" {} ", crate::icons::STATUS_SIDEBAR_TOGGLE.s())
        );

        let menu_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ToggleMenuBar))
            .expect("expected menu toggle segment (menu_bar_toggleable = true)");
        assert_eq!(
            menu_seg.text,
            format!(" {} ", crate::icons::STATUS_MENU_TOGGLE.s())
        );

        let bell_seg = status
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::DismissNotifications))
            .expect("expected done-notification bell segment");
        assert!(
            bell_seg.text.contains(crate::icons::STATUS_BELL_DONE.nerd),
            "expected the tracked STATUS_BELL_DONE nerd glyph, got '{}'",
            bell_seg.text
        );

        // ── Nerd Fonts off (ASCII fallback) ──────────────────────────────
        crate::icons::set_nerd_fonts(false);
        let status_ascii = build_window_status_line(&engine, &theme, wid, true);
        let panel_seg_ascii = status_ascii
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::TogglePanel))
            .expect("expected panel toggle segment");
        assert_eq!(panel_seg_ascii.text, " [P] ");
        let sidebar_seg_ascii = status_ascii
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ToggleSidebar))
            .expect("expected sidebar toggle segment");
        // Trailing space restored — see the nerd-fonts-on assertion
        // above.
        assert_eq!(sidebar_seg_ascii.text, " [S] ");
        let menu_seg_ascii = status_ascii
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::ToggleMenuBar))
            .expect("expected menu toggle segment");
        assert_eq!(menu_seg_ascii.text, " [M] ");
        let bell_seg_ascii = status_ascii
            .right_segments
            .iter()
            .find(|s| s.action == Some(StatusAction::DismissNotifications))
            .expect("expected done-notification bell segment");
        assert!(bell_seg_ascii.text.contains('*'));

        // Restore the default so this test can't leak into others on the
        // same worker thread (icons.rs's USE_NERD_FONTS is thread-local,
        // but cargo test can reuse worker threads across tests).
        crate::icons::set_nerd_fonts(true);
    }

    #[test]
    fn test_line_ending_detection() {
        use crate::core::buffer_manager::LineEnding;
        assert_eq!(LineEnding::detect("hello\nworld\n"), LineEnding::LF);
        assert_eq!(LineEnding::detect("hello\r\nworld\r\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("no newline"), LineEnding::LF);
        assert_eq!(LineEnding::detect(""), LineEnding::LF);
    }

    /// `modal_scrim_rect` spans the full window width but stops above every
    /// row an earlier rung already painted text into, so the destructive TUI
    /// fill cannot erase chrome. With per-window status lines on (the default)
    /// the lowest such row is the active window's own status line, *inside*
    /// the band at `editor_bottom - line_height`.
    #[test]
    fn modal_scrim_rect_stops_above_the_active_window_status_row() {
        let engine = Engine::new_for_test();
        let lh = 20.0;
        // Content 770 tall below a 30px title band.
        let el = compute_editor_layout(&engine, 770.0, lh, false);
        let r = super::modal_scrim_rect(&engine, &el, 1200.0, 30.0, lh);

        assert!(
            effective_window_status_line(&engine),
            "fixture needs per-window status lines, the default"
        );
        assert_eq!((r.x, r.y, r.width), (0.0, 30.0, 1200.0));
        assert_eq!(r.height as f64, el.editor_bottom - lh);
    }

    /// A global status bar with the wildmenu up is the tallest bottom-chrome
    /// frame: `status_bar_height_px` reserves three rows where the shell's
    /// static reservation covers two. The scrim stops above all three — and
    /// subtracts no window status row, because there is none.
    #[test]
    fn modal_scrim_rect_clears_a_global_status_bar_under_an_open_wildmenu() {
        let mut engine = Engine::new_for_test();
        // A global status bar exists only when per-window status lines are off.
        engine.settings.window_status_line = false;
        engine.wildmenu_items = vec!["one".to_string(), "two".to_string()];
        let lh = 20.0;
        let el = compute_editor_layout(&engine, 770.0, lh, false);
        let r = super::modal_scrim_rect(&engine, &el, 1200.0, 30.0, lh);

        assert_eq!(el.status_bar_h, 3.0 * lh, "wildmenu + global bar + cmdline");
        assert_eq!(r.height as f64, el.editor_bottom);
        assert_eq!(r.height as f64, 770.0 - 3.0 * lh);
    }

    /// A degenerate band (chrome taller than the window, e.g. a window
    /// squeezed to a couple of rows) yields an empty rect rather than a
    /// negative-height one.
    #[test]
    fn modal_scrim_rect_clamps_an_inverted_band_to_empty() {
        let engine = Engine::new_for_test();
        let lh = 20.0;
        let el = compute_editor_layout(&engine, 10.0, lh, false);
        let r = super::modal_scrim_rect(&engine, &el, 1200.0, 0.0, lh);
        assert_eq!(r.height, 0.0);
    }
}
