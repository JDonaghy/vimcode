use super::*;

// ─── PickerPanel (unified) ─────────────────────────────────────────────────

/// A single item in the unified picker display.
#[derive(Debug, Clone)]
pub struct PickerPanelItem {
    /// Text shown in the result list.
    pub display: String,
    /// Right-aligned hint (shortcut, line number, etc.).
    pub detail: Option<String>,
    /// Byte positions in `display` that matched the query (for highlight).
    pub match_positions: Vec<usize>,
    /// Tree nesting depth (0 = top-level).
    pub depth: usize,
    /// Whether this item has children (shows expand arrow).
    pub expandable: bool,
    /// Whether this item's children are currently visible.
    pub expanded: bool,
}

/// Data needed to render the unified picker modal.
#[derive(Debug, Clone)]
pub struct PickerPanel {
    /// Title shown in the header bar.
    pub title: String,
    /// Current query typed by the user.
    pub query: String,
    /// Filtered items to display.
    pub items: Vec<PickerPanelItem>,
    /// Index of the currently highlighted item.
    pub selected_idx: usize,
    /// Scroll offset into the filtered list.
    pub scroll_top: usize,
    /// Total number of source items (for the "N/M" counter).
    pub total_count: usize,
    /// Preview lines: (1-based line number, text, is_highlighted).
    /// When `Some`, the picker is rendered in two-pane mode.
    pub preview: Option<Vec<(usize, String, bool)>>,
    /// Scroll offset for the preview pane.
    pub preview_scroll: usize,
}

// ─── PickerGeometry ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct PickerSizing {
    pub min_w: (f32, f32),
    pub min_h: (f32, f32),
    pub left_pane_ratio: f32,
    pub header_h: f32,
    pub line_h: f32,
}

pub const TUI_PICKER_SIZING: PickerSizing = PickerSizing {
    min_w: (55.0, 60.0),
    min_h: (16.0, 18.0),
    left_pane_ratio: 0.35,
    header_h: 4.0,
    line_h: 1.0,
};

pub fn gtk_picker_sizing(line_height: f32) -> PickerSizing {
    PickerSizing {
        min_w: (500.0, 600.0),
        min_h: (350.0, 400.0),
        left_pane_ratio: 0.40,
        header_h: 2.0 * line_height + 2.0,
        line_h: line_height,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PickerGeometry {
    pub popup_x: f32,
    pub popup_y: f32,
    pub popup_w: f32,
    pub popup_h: f32,
    pub left_pane_w: f32,
    pub visible_rows: usize,
}

impl PickerGeometry {
    pub fn compute(
        viewport_w: f32,
        viewport_h: f32,
        has_preview: bool,
        sizing: &PickerSizing,
    ) -> Self {
        let popup_w = if has_preview {
            (viewport_w * 0.8).max(sizing.min_w.1)
        } else {
            (viewport_w * 0.55).max(sizing.min_w.0)
        };
        let popup_h = if has_preview {
            (viewport_h * 0.65).max(sizing.min_h.1)
        } else {
            (viewport_h * 0.60).max(sizing.min_h.0)
        };
        let popup_x = (viewport_w - popup_w) / 2.0;
        let popup_y = (viewport_h - popup_h) / 2.0;
        let left_pane_w = if has_preview {
            popup_w * sizing.left_pane_ratio
        } else {
            0.0
        };
        let results_h = (popup_h - sizing.header_h).max(0.0);
        let visible_rows = (results_h / sizing.line_h) as usize;
        PickerGeometry {
            popup_x,
            popup_y,
            popup_w,
            popup_h,
            left_pane_w,
            visible_rows,
        }
    }
}

// ─── TabSwitcherPanel ─────────────────────────────────────────────────────

/// Data needed to render the tab switcher popup (Ctrl+Tab MRU list).
#[derive(Debug, Clone)]
pub struct TabSwitcherPanel {
    /// MRU-ordered items: (filename, full_path, is_dirty).
    pub items: Vec<(String, String, bool)>,
    /// Index of the currently highlighted item.
    pub selected_idx: usize,
}

/// Convert a `TabSwitcherPanel` into a bordered `quadraui::ListView`.
///
/// Each item carries the filename (with a trailing `●` when dirty)
/// and uses the full path as the right-aligned `detail`. The list is
/// bordered with the title `" Open Tabs "` overlayed on the top
/// border. `scroll_offset` is set so the selected item is always
/// visible inside `max_visible` rows.
pub fn tab_switcher_to_quadraui_list_view(
    ts: &TabSwitcherPanel,
    max_visible: usize,
) -> quadraui::ListView {
    use quadraui::{ListItem, ListView, StyledText, WidgetId};

    let items: Vec<ListItem> = ts
        .items
        .iter()
        .map(|(name, path, dirty)| {
            let label = if *dirty {
                format!("{} ●", name)
            } else {
                name.clone()
            };
            ListItem {
                text: StyledText::plain(label),
                icon: None,
                detail: if path.is_empty() {
                    None
                } else {
                    Some(StyledText::plain(path.clone()))
                },
                decoration: quadraui::Decoration::Normal,
            }
        })
        .collect();

    // Scroll so the selected item is on screen. Window is `max_visible`
    // items tall; scroll forward by enough to keep selected_idx in view.
    let scroll_offset = if ts.selected_idx >= max_visible {
        ts.selected_idx + 1 - max_visible
    } else {
        0
    };

    ListView {
        id: WidgetId::new("tab_switcher"),
        title: Some(StyledText::plain("Open Tabs")),
        items,
        selected_idx: ts.selected_idx,
        scroll_offset,
        has_focus: true,
        bordered: true,
        h_scroll: 0,
        max_content_width: None,
        show_v_scrollbar: false,
    }
}

// ─── TabSwitcherGeometry ──────────────────────────────────────────────────
//
// #733 slice 1: the tab-switcher popup rect used to be computed twice — in
// the pre-#1434 TUI shell's `render_content` (cell units, percent-of-columns) and in
// `src/gtk/mod.rs::render_content` (pixel units, clamp(350, 600)) — and the
// GTK copy was *also* the only one fed to a click handler, which is why TUI
// had no tab-switcher rung at all. One `compute`, two sizing constants,
// mirroring the `PickerSizing` / `PickerGeometry` split just above.

/// Backend-specific sizing inputs for the tab-switcher popup.
#[derive(Debug, Clone, Copy)]
pub struct TabSwitcherSizing {
    /// Fraction of the viewport width the popup occupies.
    pub width_ratio: f32,
    /// `(min, max)` clamp applied to the computed width.
    pub width_clamp: (f32, f32),
    /// Height of one list row (1.0 in cell space, `line_height` in pixels).
    pub line_h: f32,
    /// Extra height added to `visible * line_h` for the popup border.
    pub border_h: f32,
    /// Fraction of viewport height available for rows.
    pub max_visible_ratio: f32,
    /// Height subtracted from the row budget before dividing by `line_h`.
    pub max_visible_reserve: f32,
    /// Hard cap on visible rows.
    pub max_visible_cap: usize,
    /// Snap the resolved rect to whole cells (TUI only). Keeps the
    /// hit-test rect byte-identical to the integer arithmetic the TUI
    /// painter used before this was shared.
    pub snap_to_cells: bool,
}

/// TUI sizing: 45% of terminal columns, clamped to 40..=80 cells.
pub const TUI_TAB_SWITCHER_SIZING: TabSwitcherSizing = TabSwitcherSizing {
    width_ratio: 0.45,
    width_clamp: (40.0, 80.0),
    line_h: 1.0,
    border_h: 2.0,
    max_visible_ratio: 1.0,
    max_visible_reserve: 4.0,
    max_visible_cap: 20,
    snap_to_cells: true,
};

/// GTK sizing: 40% of viewport width, clamped to 350..=600 px.
pub fn gtk_tab_switcher_sizing(line_height: f32) -> TabSwitcherSizing {
    TabSwitcherSizing {
        width_ratio: 0.40,
        width_clamp: (350.0, 600.0),
        line_h: line_height,
        border_h: 1.5 * line_height,
        max_visible_ratio: 0.6,
        max_visible_reserve: 0.0,
        max_visible_cap: 20,
        snap_to_cells: false,
    }
}

/// Resolved tab-switcher popup geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabSwitcherGeometry {
    /// Absolute popup bounds, in the caller's units.
    pub bounds: quadraui::Rect,
    /// Rows actually shown (`item_count` capped by the height budget).
    pub visible_rows: usize,
    /// The uncapped height budget, i.e. the `max_visible` the list
    /// adapter uses to decide its scroll offset.
    pub max_visible: usize,
}

impl TabSwitcherGeometry {
    /// Resolve the popup rect for `item_count` entries inside `viewport`.
    ///
    /// Returns `None` when there is nothing to show — both painters
    /// already skipped an empty item list, and the router must agree so
    /// an empty switcher can't swallow clicks.
    pub fn compute(
        viewport: quadraui::Rect,
        item_count: usize,
        sizing: &TabSwitcherSizing,
    ) -> Option<Self> {
        if item_count == 0 {
            return None;
        }
        let max_visible =
            ((viewport.height * sizing.max_visible_ratio - sizing.max_visible_reserve).max(0.0)
                / sizing.line_h) as usize;
        let max_visible = max_visible.min(sizing.max_visible_cap);
        let visible_rows = item_count.min(max_visible);

        let mut w =
            (viewport.width * sizing.width_ratio).clamp(sizing.width_clamp.0, sizing.width_clamp.1);
        let mut h = visible_rows as f32 * sizing.line_h + sizing.border_h;
        if sizing.snap_to_cells {
            w = w.floor();
            h = h.floor();
        }
        let mut x = viewport.x + (viewport.width - w).max(0.0) / 2.0;
        let mut y = viewport.y + (viewport.height - h).max(0.0) / 2.0;
        if sizing.snap_to_cells {
            x = x.floor();
            y = y.floor();
        }
        Some(TabSwitcherGeometry {
            bounds: quadraui::Rect::new(x, y, w, h),
            visible_rows,
            max_visible,
        })
    }
}

// ─── Modal-overlay mouse router (#733 slice 1, finished in #751) ──────────
//
// The top rung of the mouse precedence ladder, implemented once. Both
// backends used to hand-roll it, in *different* orders: TUI ran
// toast → find/replace → dialog, GTK ran toast → tab-switcher →
// completion → context-menu → find/replace → … → dialog. TUI had no
// tab-switcher arm at all (clicking an open Ctrl+Tab popup fell through
// to the editor underneath and moved the cursor); GTK resolved the
// dialog's inside/outside verdict through a `ModalStack` +
// `dispatch_mouse_down` round-trip that `DialogHit::Outside` already
// answers directly.
//
// #733 slice 1 shared four rungs — toast, dialog, tab switcher,
// completion. #751 folded in the last three, whose per-backend copies had
// drifted in the same two ways:
//
//  1. **Precedence.** TUI arbitrated the unified picker and the
//     find/replace overlay ~1,100 lines *before* the context menu, and GTK
//     arbitrated the context menu *after* find/replace and the picker —
//     while [`FRAME_Z_ORDER`] paints the menu above both. Input and
//     paint disagreeing is the #587/#592 failure shape.
//     [`MOUSE_ARBITRATION_ORDER`] is now the router's own declared order,
//     asserted against `FRAME_Z_ORDER` reversed.
//  2. **Missing rungs.** GTK had no context-menu *hover* arm at all — its
//     `MouseMoved` handler did nothing unless the left button was held —
//     so a menu's highlight never followed the pointer (#373). GTK also
//     hit-tested find/replace against the drawing-area width rather than
//     the active group's, so with a sidebar open the clickable panel sat
//     a couple of hundred pixels left of the painted one; and it neither
//     confirmed a click on the already-selected picker row nor paged its
//     scrollbar track the way TUI did.
//
// Deliberately *not* pushed into quadraui: `FrameHitMap` (quadraui
// `frame.rs`) resolves "which registered rect is topmost", but the order
// below is gated on vimcode engine state (`dialog.is_some()`,
// `tab_switcher_open`, `completion_idx`, `picker_open`), which is app
// knowledge. The per-surface `hit_test()` calls this function sequences
// *are* quadraui's, and are used as-is.
//
// Note every layout below is the one the last frame actually PAINTED
// (`dialog_layout`, `tab_switcher_popup_rect`, `completion_layout`,
// `context_menu_layout`, `picker_popup_rect`, `Engine::toast_layout`),
// never a freshly recomputed one — #582 / #646.
//
// **Not a rung, deliberately:** the folder/workspace picker
// (`quadraui::FolderPickerController`, #815). Both backends now paint it —
// see [`FrameOp::FolderPicker`] — but it swallows every mouse event while
// open, the same way a modal dialog does, so it is checked directly by both
// backends ([`route_folder_picker_click`]) ahead of this router rather than
// competing for z-order through [`MOUSE_ARBITRATION_ORDER`].

/// The subset of mouse actions this rung distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalMouseAction {
    /// Left button pressed.
    LeftPress,
    /// Left button *released*. Kept distinct from [`Self::LeftPress`]
    /// because the two rungs disagree about it on purpose:
    ///
    /// * The context menu accepts either (#456 — alacritty via SGR mouse
    ///   mode + tmux, and gnome-terminal in some configurations, drop
    ///   `Down(Left)` and only emit `Up(Left)`; without accepting the
    ///   release the menu item never fires).
    /// * Every other rung accepts `LeftPress` alone, so a terminal that
    ///   sends *both* can't double-fire a dialog button.
    LeftRelease,
    /// Pointer motion with no button held — the hover rung.
    Move,
    /// Anything else: drag, right-click, scroll.
    Other,
}

impl ModalMouseAction {
    /// Does this action open/confirm a surface? `LeftPress` only — see
    /// [`Self::LeftRelease`] for why the release is deliberately excluded.
    fn is_press(self) -> bool {
        self == ModalMouseAction::LeftPress
    }

    /// Does this action count as "the user clicked the context menu"?
    fn is_menu_click(self) -> bool {
        matches!(
            self,
            ModalMouseAction::LeftPress | ModalMouseAction::LeftRelease
        )
    }
}

/// The painted modal overlays, in the order they are arbitrated.
///
/// Each `*_open` flag is the engine-state gate; the matching layout is
/// the cached paint result. A gate that is `true` with a `None` layout
/// means "opened this frame, not painted yet" and is handled
/// conservatively (swallow rather than dismiss something the user has
/// not seen).
#[derive(Debug, Clone, Copy, Default)]
pub struct ModalOverlayState<'a> {
    pub toast: Option<&'a quadraui::ToastStackLayout>,
    pub dialog_open: bool,
    pub dialog: Option<&'a quadraui::DialogLayout>,
    /// `Engine::context_menu.is_some()`.
    pub context_menu_open: bool,
    pub context_menu: Option<&'a quadraui::ContextMenuLayout>,
    /// Slack, in the caller's own units, around
    /// [`quadraui::ContextMenuLayout::bounds`] that still counts as "on the
    /// menu". The TUI rasteriser draws the menu's box-drawing border *outside*
    /// `bounds` (1 cell), so a click on the frame must consume rather than
    /// dismiss; GTK paints its border inside `bounds` and passes `0.0`.
    pub context_menu_border: f32,
    pub tab_switcher_open: bool,
    pub tab_switcher_bounds: Option<quadraui::Rect>,
    pub completion_open: bool,
    pub completion: Option<&'a quadraui::CompletionsLayout>,
    /// `Engine::picker_open`.
    pub picker_open: bool,
    pub picker: Option<PickerHitGeometry>,
    /// `Engine::find_replace_open`.
    pub find_replace_open: bool,
    pub find_replace: Option<FindReplaceHitGeometry<'a>>,
}

/// What the context-menu rung decided about one event.
///
/// Split out of [`ModalOverlayRoute`] so the *engine index* — the one piece
/// both backends previously re-derived from `"context:N"` themselves — is
/// resolved here, once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuRoute {
    /// Fire the action for item `idx`: set `selected`, then
    /// `Engine::context_menu_confirm`.
    Item(usize),
    /// Pointer moved onto item `idx` — set `selected` and consume. Both
    /// backends must do this for keyboard/mouse selection to agree; before
    /// #751 only TUI did, so on GTK whichever item was selected when the menu
    /// opened stayed highlighted no matter where the pointer went (#373).
    Hover(usize),
    /// On the menu but not on an actionable item (separator, disabled row,
    /// border) — consume, change nothing.
    Consume,
    /// Click landed outside the menu — close it and consume.
    Dismiss,
    /// The menu is open but does not arbitrate this event (right-click,
    /// scroll, drag) — the caller continues down its own ladder.
    Fallthrough,
}

/// What the find/replace rung decided about one event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FindReplaceRoute {
    /// The click resolved to `target`; input targets already carry their
    /// character offset.
    Target {
        target: quadraui::FindReplaceClickTarget,
        /// `target` is a text input, so the caller arms its input-drag flag.
        is_input: bool,
    },
    /// On the panel but not on any hit region (border, padding) — consume.
    Consume,
}

/// What the unified-picker rung decided about one event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerRoute {
    /// Result row `idx`, already offset by the effective scroll offset.
    Row(usize),
    /// The scrollbar thumb was grabbed — begin a `ScrollbarY` drag with this
    /// grab offset (distance from the thumb's top edge to the pointer).
    ScrollbarThumb { grab_offset: f32 },
    /// The scrollbar *track* was clicked; page one screen in that direction.
    ScrollbarTrack { toward_end: bool },
    /// Inside the popup but on neither a row nor the scrollbar — consume.
    Consume,
    /// Outside the popup — dismiss the picker.
    Dismiss,
}

/// The find/replace overlay's painted geometry, in whatever unit the caller's
/// mouse events arrive in.
///
/// TUI passes cells and a `(1.0, 1.0)` cell size; GTK passes device pixels and
/// its painted `(char_width, line_height)`. The *hit-region walk* underneath —
/// the part that was transcribed twice and could drift — is unit-free once the
/// caller has stated its own scale here.
#[derive(Debug, Clone, Copy)]
pub struct FindReplaceHitGeometry<'a> {
    /// Outer panel bounds including borders.
    pub bounds: quadraui::Rect,
    /// Top-left of the first content row, one cell inside the border.
    pub content_origin: (f32, f32),
    /// One cell, in the caller's units.
    pub cell: (f32, f32),
    /// The regions `compute_find_replace_hit_regions` produced for the frame
    /// that was actually painted.
    pub hit_regions: &'a [(quadraui::FrHitRegion, quadraui::FindReplaceClickTarget)],
}

/// Where a backend's rasteriser anchors the find/replace panel inside the
/// active editor group.
///
/// The panel's *shape* is identical on both backends (a `panel_width`-cell box
/// with a one-cell border and one or two content rows); only the gap it leaves
/// and the units it measures in differ, and both of those are data. Stating
/// them as constants — the same shape [`PickerSizing`] / [`TUI_PICKER_SIZING`]
/// already uses in this file — lets one hit-test function serve both backends
/// instead of each transcribing its rasteriser's arithmetic by hand.
///
/// That transcription is exactly what had rotted on GTK: the rasteriser
/// (`quadraui::gtk::find_replace`) anchors the panel to
/// `group_bounds.x + group_bounds.width`, but `handle_mouse_click_msg`
/// hit-tested against the *drawing area* width and a `line_height * 2.5 + 2`
/// top edge, so with a sidebar open, a vertical split, or any window whose
/// editor area was narrower than the DA, the clickable panel sat somewhere the
/// painted panel wasn't.
#[derive(Debug, Clone, Copy)]
pub struct FindReplaceAnchor {
    /// Gap between the panel's right edge and the group's right edge.
    pub right_gap: f32,
    /// Offset of the panel's top edge below the group's top edge.
    pub top_offset: f32,
    /// Lower clamp on the panel's top edge.
    pub min_y: f32,
    /// Round the resolved origin down to whole cells (TUI's integer grid).
    pub snap_to_cells: bool,
}

/// TUI: one cell of gap, panel top clamped below the menu-bar row.
pub const TUI_FIND_REPLACE_ANCHOR: FindReplaceAnchor = FindReplaceAnchor {
    right_gap: 1.0,
    top_offset: 0.0,
    min_y: 1.0,
    snap_to_cells: true,
};

/// GTK: a 10-pixel gap and a 2-pixel drop, matching
/// `quadraui::gtk::find_replace::draw`.
pub const GTK_FIND_REPLACE_ANCHOR: FindReplaceAnchor = FindReplaceAnchor {
    right_gap: 10.0,
    top_offset: 2.0,
    min_y: 0.0,
    snap_to_cells: false,
};

impl<'a> FindReplaceHitGeometry<'a> {
    /// Resolve the panel's painted geometry from the panel primitive the frame
    /// was built with, in `cell`-sized units.
    ///
    /// Mirrors `quadraui::{tui,gtk}::find_replace::draw` — the *paint* math —
    /// so the hit rect and the pixels can no longer disagree.
    pub fn from_panel(
        panel: &'a quadraui::FindReplacePanel,
        cell: (f32, f32),
        anchor: &FindReplaceAnchor,
    ) -> Self {
        let (cw, ch) = (cell.0.max(f32::EPSILON), cell.1.max(f32::EPSILON));
        let panel_w = panel.panel_width as f32 * cw;
        let row_count = if panel.show_replace { 2.0 } else { 1.0 };
        let panel_h = (row_count + 2.0) * ch;

        let gb = panel.group_bounds;
        let mut x = (gb.x + gb.width - panel_w - anchor.right_gap).max(gb.x);
        let mut y = (gb.y + anchor.top_offset).max(anchor.min_y);
        if anchor.snap_to_cells {
            x = x.floor();
            y = y.floor();
        }

        FindReplaceHitGeometry {
            bounds: quadraui::Rect::new(x, y, panel_w, panel_h),
            content_origin: (x + cw, y + ch),
            cell: (cw, ch),
            hit_regions: &panel.hit_regions,
        }
    }
}

/// How a backend's palette rasteriser lays result rows out inside the popup.
///
/// Companion to [`PickerSizing`] (which sizes the popup): this sizes the
/// *results band* inside it, and is the second half of the geometry both
/// backends' click handlers used to re-derive by hand.
#[derive(Debug, Clone, Copy)]
pub struct PickerRowMetrics {
    /// Height of the header band (prompt + query) above the first result row.
    pub header_h: f32,
    /// Space reserved below the last result row.
    pub bottom_inset: f32,
    /// Share of the popup width given to the results list when a preview pane
    /// is shown.
    pub list_ratio: f32,
    /// Width of the scrollbar gutter, inset from the list pane's right edge.
    pub scrollbar_w: f32,
}

/// TUI: a three-row header, a one-row bottom border, a one-cell scrollbar.
pub const TUI_PICKER_ROWS: PickerRowMetrics = PickerRowMetrics {
    header_h: 3.0,
    bottom_inset: 1.0,
    list_ratio: 0.4,
    scrollbar_w: 1.0,
};

/// GTK: header and rows scale with the painted line height; the scrollbar is a
/// fixed 6-pixel gutter.
pub fn gtk_picker_rows(line_height: f32) -> PickerRowMetrics {
    PickerRowMetrics {
        header_h: line_height * 2.0 + 1.0,
        bottom_inset: 4.0,
        list_ratio: 0.4,
        scrollbar_w: 6.0,
    }
}

/// The unified picker's painted geometry, in the caller's own units.
///
/// Every field below is a *paint* result, never a fresh recomputation (#582 /
/// #646). The derived quantities — effective scroll offset, thumb length,
/// thumb top — used to be hand-rolled on both backends from these same inputs,
/// and had already drifted: TUI paged the track and grabbed the thumb with an
/// offset, GTK jumped proportionally and grabbed at zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickerHitGeometry {
    /// Popup bounds.
    pub bounds: quadraui::Rect,
    /// Width of the results list pane, measured from `bounds.x`. Equals
    /// `bounds.width` when no preview pane is shown.
    pub list_width: f32,
    /// Top edge of the first result row.
    pub rows_top: f32,
    /// Height of one result row (`1.0` on TUI, the painted line height on GTK).
    pub row_height: f32,
    /// Width of the scrollbar gutter, measured back from the list pane's right
    /// edge.
    pub scrollbar_width: f32,
    pub visible_rows: usize,
    pub total_items: usize,
    pub scroll_top: usize,
    pub selected: usize,
}

impl PickerHitGeometry {
    /// Resolve the results band from the popup rect the frame actually
    /// painted plus the backend's row metrics.
    pub fn new(
        bounds: quadraui::Rect,
        row_height: f32,
        has_preview: bool,
        metrics: &PickerRowMetrics,
        engine: &Engine,
    ) -> Self {
        let row_height = row_height.max(f32::EPSILON);
        let rows_top = bounds.y + metrics.header_h;
        let rows_bottom = bounds.y + bounds.height - metrics.bottom_inset;
        let visible_rows = ((rows_bottom - rows_top).max(0.0) / row_height) as usize;
        let list_width = if has_preview {
            (bounds.width * metrics.list_ratio).round()
        } else {
            bounds.width
        };
        PickerHitGeometry {
            bounds,
            list_width,
            rows_top,
            row_height,
            scrollbar_width: metrics.scrollbar_w,
            visible_rows,
            total_items: engine.picker_items.len(),
            scroll_top: engine.picker_scroll_top,
            selected: engine.picker_selected,
        }
    }

    /// The scroll offset the picker was actually *drawn* at.
    ///
    /// `draw_palette` clamps its offset to keep `selected` on screen, so the
    /// raw `scroll_top` is not what the user is looking at. Both backends
    /// carried a verbatim copy of this eight-line clamp.
    pub fn effective_offset(&self) -> usize {
        let max_offset = self.total_items.saturating_sub(self.visible_rows);
        if self.visible_rows == 0 {
            0
        } else if self.selected < self.scroll_top {
            self.selected
        } else if self.selected >= self.scroll_top + self.visible_rows {
            self.selected + 1 - self.visible_rows
        } else {
            self.scroll_top
        }
        .min(max_offset)
    }

    /// Height of the results viewport (== the scrollbar track length).
    pub fn track_length(&self) -> f32 {
        self.visible_rows as f32 * self.row_height
    }

    /// Is there anything to scroll?
    pub fn has_scrollbar(&self) -> bool {
        self.total_items > self.visible_rows && self.visible_rows > 0
    }

    /// Length of the scrollbar thumb.
    pub fn thumb_length(&self) -> f32 {
        (self.track_length() * self.visible_rows as f32 / self.total_items.max(1) as f32)
            .max(self.row_height.max(1.0))
    }

    /// Top edge of the scrollbar thumb, for the offset actually painted.
    pub fn thumb_top(&self) -> f32 {
        let max_scroll = self.total_items.saturating_sub(self.visible_rows);
        let effective_track = (self.track_length() - self.thumb_length()).max(1.0);
        let ratio = if max_scroll == 0 {
            0.0
        } else {
            (self.effective_offset() as f32 / max_scroll as f32).clamp(0.0, 1.0)
        };
        self.rows_top + ratio * effective_track
    }

    /// The offset a [`PickerRoute::ScrollbarTrack`] click pages to.
    pub fn paged_offset(&self, toward_end: bool) -> usize {
        let max_scroll = self.total_items.saturating_sub(self.visible_rows);
        let page = self.visible_rows.max(1);
        let current = self.effective_offset();
        if toward_end {
            (current + page).min(max_scroll)
        } else {
            current.saturating_sub(page)
        }
    }

    /// The drag quadraui should run for a [`PickerRoute::ScrollbarThumb`] grab.
    pub fn drag_target(
        &self,
        widget: quadraui::WidgetId,
        grab_offset: f32,
    ) -> quadraui::DragTarget {
        quadraui::DragTarget::ScrollbarY {
            widget,
            track_start: self.rows_top,
            track_length: self.track_length(),
            thumb_length: self.thumb_length(),
            max_scroll: self.total_items.saturating_sub(self.visible_rows),
            grab_offset,
            inverted: false,
        }
    }

    /// Resolve a point against the painted popup.
    pub fn resolve(&self, x: f32, y: f32) -> PickerRoute {
        let b = self.bounds;
        let inside = x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height;
        if !inside {
            return PickerRoute::Dismiss;
        }

        let rows_bottom = self.rows_top + self.track_length();
        let on_rows_band = y >= self.rows_top && y < rows_bottom;

        if self.has_scrollbar() && on_rows_band {
            let sb_right = b.x + self.list_width;
            let sb_left = sb_right - self.scrollbar_width;
            if x >= sb_left && x < sb_right {
                let thumb_top = self.thumb_top();
                let dy = y - thumb_top;
                if dy >= 0.0 && dy < self.thumb_length() {
                    return PickerRoute::ScrollbarThumb { grab_offset: dy };
                }
                return PickerRoute::ScrollbarTrack {
                    toward_end: y >= thumb_top,
                };
            }
        }

        if on_rows_band {
            let row = ((y - self.rows_top) / self.row_height.max(f32::EPSILON)) as usize;
            let idx = self.effective_offset() + row;
            if idx < self.total_items {
                return PickerRoute::Row(idx);
            }
        }

        PickerRoute::Consume
    }
}

/// Which modal overlay owns a mouse event, plus its resolved hit.
///
/// `None` means no overlay claimed the point — the caller continues down
/// its own ladder. `TabSwitcher`/`Completion` carry enough for the caller
/// to decide whether to keep going (both dismiss on any click, but only
/// consume when the click landed inside).
#[derive(Debug, Clone, PartialEq)]
pub enum ModalOverlayRoute {
    Toast(quadraui::ToastHit),
    Dialog(quadraui::DialogHit),
    ContextMenu(ContextMenuRoute),
    TabSwitcher {
        inside: bool,
    },
    Completion(quadraui::CompletionsHit),
    UnifiedPicker(PickerRoute),
    FindReplace(FindReplaceRoute),
    /// An overlay is up and must eat this event without acting on it
    /// (e.g. mouse motion while a modal dialog is open).
    Swallow,
    /// Nothing claimed it.
    None,
}

/// The overlay rungs [`route_modal_overlay_click`] arbitrates, **highest z
/// first** — the exact inverse of [`FRAME_Z_ORDER`]'s overlay tail, restricted
/// to the rungs that take mouse input.
///
/// Input precedence and paint order are the same fact stated twice, and #587 /
/// #592 are what happens when the two disagree: whatever is painted on top is
/// what the user is aiming at. Keeping this as a `const` rather than a comment
/// lets `mouse_arbitration_matches_paint_z_order` assert the relationship
/// instead of asking reviewers to eyeball it.
///
/// The completion popup is deliberately absent: it is editor-anchored, composed
/// outside the shared frame sequence (see [`FRAME_Z_ORDER`]'s doc), so it has
/// no paint rung to agree with.
pub const MOUSE_ARBITRATION_ORDER: [FrameOp; 6] = [
    FrameOp::ToastStack,
    FrameOp::Dialog,
    FrameOp::ContextMenu,
    FrameOp::TabSwitcher,
    FrameOp::UnifiedPicker,
    FrameOp::FindReplace,
];

/// Resolve the top rung of the mouse ladder against the painted overlays.
pub fn route_modal_overlay_click(
    state: &ModalOverlayState<'_>,
    x: f32,
    y: f32,
    action: ModalMouseAction,
) -> ModalOverlayRoute {
    let press = action.is_press();

    // ── Toast (× dismiss / action) ────────────────────────────────────
    // Painted above every other overlay, so it is arbitrated first —
    // both backends already did this, identically.
    if press {
        if let Some(layout) = state.toast {
            let hit = layout.hit_test(x, y);
            if hit != quadraui::ToastHit::Empty {
                return ModalOverlayRoute::Toast(hit);
            }
        }
    }

    // ── Modal dialog ──────────────────────────────────────────────────
    // A dialog is modal: it eats everything, including motion, so the
    // editor underneath can't hover-highlight through it. This is TUI's
    // long-standing behaviour; GTK inherits it here.
    if state.dialog_open {
        if !press {
            return ModalOverlayRoute::Swallow;
        }
        // No cached layout yet (opened this frame, not yet painted) —
        // treat as a body click rather than risk dismissing a dialog the
        // user hasn't seen.
        let hit = state
            .dialog
            .map(|dl| dl.hit_test(x, y))
            .unwrap_or(quadraui::DialogHit::Body);
        return ModalOverlayRoute::Dialog(hit);
    }

    // ── Context menu ──────────────────────────────────────────────────
    // Modal for clicks *and* hovers, which is why it sits above the tab
    // switcher / picker / find-replace rather than ~1,100 lines below
    // them as it did on TUI. Painted directly under the dialog
    // (`FRAME_Z_ORDER`), so it is arbitrated directly after it.
    if state.context_menu_open {
        if action.is_menu_click() {
            let Some(cl) = state.context_menu else {
                // Menu state with nothing painted (empty item list, or
                // opened after the last frame). Close defensively rather
                // than leaving an invisible modal eating clicks — both
                // backends already did exactly this.
                return ModalOverlayRoute::ContextMenu(ContextMenuRoute::Dismiss);
            };
            let route = match cl.hit_test(x, y) {
                quadraui::ContextMenuHit::Item(id) => {
                    match crate::core::engine::context_menu_hit_to_idx(
                        &quadraui::ContextMenuHit::Item(id),
                    ) {
                        Some(idx) => ContextMenuRoute::Item(idx),
                        None => ContextMenuRoute::Consume,
                    }
                }
                quadraui::ContextMenuHit::Inert => ContextMenuRoute::Consume,
                quadraui::ContextMenuHit::Empty => {
                    if point_in_menu_frame(cl, state.context_menu_border, x, y) {
                        ContextMenuRoute::Consume
                    } else {
                        ContextMenuRoute::Dismiss
                    }
                }
            };
            return ModalOverlayRoute::ContextMenu(route);
        }
        if action == ModalMouseAction::Move {
            let route = state
                .context_menu
                .map(|cl| cl.hit_test(x, y))
                .and_then(|hit| crate::core::engine::context_menu_hit_to_idx(&hit))
                .map(ContextMenuRoute::Hover)
                .unwrap_or(ContextMenuRoute::Consume);
            return ModalOverlayRoute::ContextMenu(route);
        }
        // Right-click / scroll / drag: the caller keeps going (a
        // right-click closes this menu and opens the one under the
        // pointer).
        return ModalOverlayRoute::ContextMenu(ContextMenuRoute::Fallthrough);
    }

    // ── Tab switcher (Ctrl+Tab MRU popup) ─────────────────────────────
    if state.tab_switcher_open && press {
        let inside = state
            .tab_switcher_bounds
            .is_some_and(|b| x >= b.x && x < b.x + b.width && y >= b.y && y < b.y + b.height);
        return ModalOverlayRoute::TabSwitcher { inside };
    }

    // ── Completion popup ──────────────────────────────────────────────
    if state.completion_open && press {
        let hit = state
            .completion
            .map(|cl| cl.hit_test(x, y))
            .unwrap_or(quadraui::CompletionsHit::Empty);
        return ModalOverlayRoute::Completion(hit);
    }

    // ── Unified picker / command palette ──────────────────────────────
    if state.picker_open && press {
        if let Some(geo) = state.picker {
            return ModalOverlayRoute::UnifiedPicker(geo.resolve(x, y));
        }
        // Open but not painted yet — swallow rather than let the click
        // reach the editor behind a modal the user has already summoned.
        return ModalOverlayRoute::UnifiedPicker(PickerRoute::Consume);
    }

    // ── Find/replace overlay ──────────────────────────────────────────
    // Bottom of the arbitration ladder, matching its bottom-of-band paint
    // position. A click that misses the panel falls through to whatever
    // is underneath — unlike the picker, this overlay is not modal.
    if state.find_replace_open && press {
        if let Some(geo) = state.find_replace {
            if let Some(route) = geo.resolve(x, y) {
                return ModalOverlayRoute::FindReplace(route);
            }
        }
    }

    ModalOverlayRoute::None
}

/// Is `(x, y)` on the context menu, counting `border` units of frame drawn
/// outside [`quadraui::ContextMenuLayout::bounds`]?
fn point_in_menu_frame(cl: &quadraui::ContextMenuLayout, border: f32, x: f32, y: f32) -> bool {
    let b = &cl.bounds;
    x >= b.x - border
        && x < b.x + b.width + border
        && y >= b.y - border
        && y < b.y + b.height + border
}

impl FindReplaceHitGeometry<'_> {
    /// Resolve a point against the painted panel.
    ///
    /// `None` means the point missed the panel entirely and the caller should
    /// keep going down its own ladder.
    pub fn resolve(&self, x: f32, y: f32) -> Option<FindReplaceRoute> {
        let b = self.bounds;
        if x < b.x || x >= b.x + b.width || y < b.y || y >= b.y + b.height {
            return None;
        }

        let (cw, ch) = (self.cell.0.max(f32::EPSILON), self.cell.1.max(f32::EPSILON));
        let (ox, oy) = self.content_origin;
        // `u16::MAX` for "above/left of the content box" matches no region,
        // which is what both backends' own out-of-range sentinel did.
        let rel_col = if x >= ox {
            ((x - ox) / cw) as u16
        } else {
            u16::MAX
        };
        let rel_row = if y >= oy {
            ((y - oy) / ch) as u16
        } else {
            u16::MAX
        };

        let matched = self.hit_regions.iter().find(|(region, _)| {
            region.row == rel_row && rel_col >= region.col && rel_col < region.col + region.width
        });

        let Some((region, target)) = matched else {
            return Some(FindReplaceRoute::Consume);
        };

        use quadraui::FindReplaceClickTarget::*;
        let char_pos = rel_col.saturating_sub(region.col) as usize;
        let target = match target {
            FindInput(_) => FindInput(char_pos),
            ReplaceInput(_) => ReplaceInput(char_pos),
            other => *other,
        };
        Some(FindReplaceRoute::Target {
            target,
            is_input: matches!(target, FindInput(_) | ReplaceInput(_)),
        })
    }
}

/// Apply a scroll offset the unified-picker rung produced, keeping the
/// selection inside the new viewport and reloading the preview.
///
/// Extracted because both backends carried this identical five-line
/// clamp-then-reload in **three** places each (scrollbar drag, track page,
/// thumb click).
pub fn apply_picker_scroll_offset(engine: &mut Engine, new_offset: usize, visible_rows: usize) {
    engine.picker_scroll_top = new_offset;
    if engine.picker_selected < new_offset {
        engine.picker_selected = new_offset;
    } else if visible_rows > 0 && engine.picker_selected >= new_offset + visible_rows {
        engine.picker_selected = new_offset + visible_rows - 1;
    }
    engine.picker_load_preview();
}

/// Apply a click on unified-picker result row `idx`.
///
/// Clicking the already-selected row confirms it — or, in the command
/// centre's `@` tree mode, expands/collapses it. Clicking a different row
/// selects it. Before #751 only TUI did the confirm/expand half, so a GTK user
/// had to click a row and then press Enter.
pub fn apply_picker_row_click(engine: &mut Engine, idx: usize) {
    if idx >= engine.picker_items.len() {
        return;
    }
    if engine.picker_selected == idx {
        let in_tree_mode = engine.picker_source == crate::core::engine::PickerSource::CommandCenter
            && engine.picker_query == "@";
        if in_tree_mode && engine.picker_toggle_expand() {
            engine.picker_load_preview();
        } else {
            engine.picker_confirm();
        }
    } else {
        engine.picker_selected = idx;
        engine.picker_load_preview();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── #733: the shared modal-overlay rung ──────────────────────────────

    #[test]
    fn tab_switcher_geometry_matches_the_two_backends_old_inline_math() {
        // TUI: width = (cols * 45 / 100).clamp(40, 80); rows capped at
        // height-4 and 20; height = visible + 2; centred in the viewport.
        let geo = TabSwitcherGeometry::compute(
            quadraui::Rect::new(0.0, 0.0, 100.0, 24.0),
            2,
            &TUI_TAB_SWITCHER_SIZING,
        )
        .expect("two items must yield a popup");
        assert_eq!(geo.visible_rows, 2);
        assert_eq!(geo.max_visible, 20);
        assert_eq!(geo.bounds.width, 45.0);
        assert_eq!(geo.bounds.height, 4.0);
        assert_eq!(geo.bounds.x, 27.0);
        assert_eq!(geo.bounds.y, 10.0);

        // The clamp still bites on a narrow terminal.
        let narrow = TabSwitcherGeometry::compute(
            quadraui::Rect::new(0.0, 0.0, 60.0, 24.0),
            2,
            &TUI_TAB_SWITCHER_SIZING,
        )
        .unwrap();
        assert_eq!(narrow.bounds.width, 40.0);

        // GTK: 40% of viewport width clamped to [350, 600] px, height
        // = (visible + 1.5) * line_height, no cell snapping.
        let gtk = TabSwitcherGeometry::compute(
            quadraui::Rect::new(0.0, 0.0, 1400.0, 900.0),
            3,
            &gtk_tab_switcher_sizing(20.0),
        )
        .unwrap();
        assert_eq!(gtk.bounds.width, 560.0);
        assert_eq!(gtk.bounds.height, (3.0 + 1.5) * 20.0);
        assert_eq!(gtk.max_visible, 20); // (900 * 0.6) / 20 = 27, capped

        // No items → no popup, so an empty switcher can't swallow clicks.
        assert!(TabSwitcherGeometry::compute(
            quadraui::Rect::new(0.0, 0.0, 100.0, 24.0),
            0,
            &TUI_TAB_SWITCHER_SIZING,
        )
        .is_none());
    }

    #[test]
    fn modal_overlay_router_orders_dialog_above_tab_switcher_and_completion() {
        // A dialog outranks everything below it, whatever else is open.
        let state = ModalOverlayState {
            dialog_open: true,
            tab_switcher_open: true,
            tab_switcher_bounds: Some(quadraui::Rect::new(0.0, 0.0, 100.0, 100.0)),
            completion_open: true,
            ..Default::default()
        };
        assert!(matches!(
            route_modal_overlay_click(&state, 5.0, 5.0, ModalMouseAction::LeftPress),
            ModalOverlayRoute::Dialog(quadraui::DialogHit::Body),
        ));
        // …including for non-press events, which it swallows outright so
        // the editor underneath can't hover through a modal.
        assert_eq!(
            route_modal_overlay_click(&state, 5.0, 5.0, ModalMouseAction::Other),
            ModalOverlayRoute::Swallow
        );

        // With the dialog closed, the tab switcher takes the click and
        // reports whether it landed inside its painted bounds.
        let state = ModalOverlayState {
            tab_switcher_open: true,
            tab_switcher_bounds: Some(quadraui::Rect::new(10.0, 10.0, 20.0, 4.0)),
            completion_open: true,
            ..Default::default()
        };
        assert_eq!(
            route_modal_overlay_click(&state, 15.0, 11.0, ModalMouseAction::LeftPress),
            ModalOverlayRoute::TabSwitcher { inside: true }
        );
        assert_eq!(
            route_modal_overlay_click(&state, 1.0, 1.0, ModalMouseAction::LeftPress),
            ModalOverlayRoute::TabSwitcher { inside: false }
        );

        // Nothing open → nothing claimed, and the caller keeps walking.
        assert_eq!(
            route_modal_overlay_click(
                &ModalOverlayState::default(),
                1.0,
                1.0,
                ModalMouseAction::LeftPress,
            ),
            ModalOverlayRoute::None
        );
    }

    /// #734 slice 1: the keyboard ladder's top rung, pinned in the order
    /// `Engine::handle_key` itself uses — spell suggestions, then a modal
    /// dialog, then the context menu. Both backends match on this, so a
    /// reorder here is a cross-backend behaviour change and should have to
    /// break a test to happen.
    #[test]
    fn modal_key_router_orders_spell_and_dialog_above_the_context_menu() {
        let mut engine = Engine::new();
        assert_eq!(route_modal_key(&engine), ModalKeyRoute::None);

        // A context menu alone routes to its own (backend-dispatched) arm.
        engine.open_editor_context_menu(5, 5);
        assert_eq!(route_modal_key(&engine), ModalKeyRoute::ContextMenu);

        // A modal dialog outranks it — the rung GTK was missing entirely.
        engine.show_quit_confirm();
        assert!(engine.dialog.is_some());
        assert!(engine.context_menu.is_some());
        assert_eq!(route_modal_key(&engine), ModalKeyRoute::Engine);

        // Spell-suggestion selection outranks everything, as in `keys.rs`.
        engine.spell_suggestions = Some(("teh".into(), vec!["the".into()], String::new()));
        assert_eq!(route_modal_key(&engine), ModalKeyRoute::Engine);
    }

    #[test]
    fn test_diff_peek_to_tooltip_per_line_colors_and_action_bar() {
        let theme = Theme::onedark();
        let peek = DiffPeekPopup {
            anchor_line: 5,
            hunk_lines: vec![
                " let x = 1;".to_string(),
                "-old line".to_string(),
                "+new line".to_string(),
            ],
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, layout) =
            diff_peek_to_quadraui_tooltip(&peek, 30.0, 10.0, viewport, &theme, 1.0, 1.0);

        // Multi-line styled path active.
        let lines = tooltip.styled_lines.as_ref().expect("styled_lines");
        // 3 hunk lines + 1 action bar = 4 rows.
        assert_eq!(lines.len(), 4);

        let added = theme.git_added;
        let deleted = theme.git_deleted;
        let fg = theme.hover_fg;

        // Context line: hover_fg.
        assert_eq!(lines[0].spans[0].fg, Some(fg));
        // Deleted line: git_deleted.
        assert_eq!(lines[1].spans[0].fg, Some(deleted));
        // Added line: git_added.
        assert_eq!(lines[2].spans[0].fg, Some(added));
        // Action bar: default fg, contains hotkey labels.
        let action: String = lines[3].spans.iter().map(|s| s.text.as_str()).collect();
        assert!(action.contains("[s] Stage"));
        assert!(action.contains("[r] Revert"));
        assert!(action.contains("[q] Close"));
        assert_eq!(lines[3].spans[0].fg, Some(fg));

        // Placement: prefers Bottom (legacy diff peek always rendered below).
        // Anchor at y=10 with viewport height 50 → fits below → Bottom resolved.
        assert_eq!(
            layout.resolved_placement,
            quadraui::ResolvedPlacement::Bottom
        );
        assert!(layout.bounds.y > 10.0);
        // Multi-row height (4 rows).
        assert_eq!(layout.bounds.height, 4.0);
    }

    #[test]
    fn test_signature_help_active_param_out_of_range_falls_back() {
        let theme = Theme::onedark();
        // active_param index points past end of params list — adapter falls
        // back to no-highlight path.
        let sig = SignatureHelp {
            label: "fn foo(x: i32)".to_string(),
            params: vec![(7, 13)],
            active_param: Some(5), // out of range
            anchor_line: 0,
            anchor_col: 0,
        };
        let viewport = quadraui::Rect::new(0.0, 0.0, 200.0, 50.0);
        let (tooltip, _layout) =
            signature_help_to_quadraui_tooltip(&sig, 10.0, 5.0, viewport, &theme, 1.0, 1.0);
        let lines = tooltip.styled_lines.as_ref().expect("styled spans");
        assert_eq!(lines.len(), 1);
        let styled = &lines[0];
        // Fallback: 3 spans (leading-pad, whole-label, trailing-pad).
        assert_eq!(styled.spans.len(), 3);
        assert_eq!(styled.spans[1].text, "fn foo(x: i32)");
    }

    #[test]
    fn test_breadcrumb_bounds_do_not_overlap_first_line() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let mut engine = Engine::new();
        engine.settings.breadcrumbs = true;
        engine.buffer_mut().insert(0, "line 1\nline 2\nline 3\n");

        let line_height = 20.0;
        let char_width = 8.0;
        let tbh = tab_bar_height_px(line_height, true);
        let wid = engine.active_window_id();
        let rects = vec![(wid, WindowRect::new(0.0, tbh, 800.0, 600.0 - tbh))];
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );

        assert!(!layout.breadcrumbs.is_empty());
        let bc = &layout.breadcrumbs[0];
        // Breadcrumb bounds must sit ABOVE the window content, not overlap it.
        let window_top = layout.windows[0].rect.y;
        assert!(
            bc.bounds.y + bc.bounds.height <= window_top,
            "breadcrumb bottom ({}) must not exceed window top ({})",
            bc.bounds.y + bc.bounds.height,
            window_top,
        );

        // Clicking at the window top (line 1) must return Window, not Breadcrumb.
        let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);
        let zone = screen_zone_hit_test(
            &layout,
            100.0,
            window_top,
            tbh,
            single_tab_hidden,
            engine.active_group,
        );
        assert!(
            matches!(zone, ScreenZone::Window { .. }),
            "click at window_top should hit Window zone, got {:?}",
            zone,
        );
    }

    /// #546 FAILED-3 regression: GTK's `main_content_bounds` gained a
    /// persistent nonzero `(x, y)` offset once the always-visible
    /// menu/title-bar chrome band landed (#552) — every window rect (and
    /// every click coordinate) GTK builds lives in that same absolute
    /// space, not one that starts at `(0, 0)`. The single-group branch of
    /// `screen_zone_hit_test` used to hardcode `y >= 0.0` as the tab row's
    /// top, so a click on the actually-rendered (offset) tab bar — e.g. a
    /// tab's close button — was silently misclassified as a `Window` hit
    /// instead of `TabBar`, and the click just moved the cursor instead of
    /// closing the tab. This pins the fix: derive the bar's bounds from the
    /// real (possibly-offset) window rects, exactly like the multi-group
    /// branch above already does via `GroupTabBar::bounds`.
    #[test]
    fn test_single_group_tab_bar_hit_test_with_editor_offset() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let engine = Engine::new();
        let line_height = 20.0;
        let char_width = 8.0;
        let tbh = tab_bar_height_px(line_height, false);

        // Simulate a chrome-shifted `main_content_bounds`: editor content
        // starts at (50, 100), not (0, 0).
        let content_x = 50.0;
        let content_y = 100.0;
        let wid = engine.active_window_id();
        let rects = vec![(
            wid,
            WindowRect::new(content_x, content_y + tbh, 800.0, 600.0),
        )];
        let theme = Theme::onedark();
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        let single_tab_hidden = engine.is_tab_bar_hidden(engine.active_group);

        // A click at the tab row's actual (offset) position must resolve to
        // TabBar, not fall through to a Window hit underneath.
        let zone = screen_zone_hit_test(
            &layout,
            content_x + 5.0,
            content_y + 2.0,
            tbh,
            single_tab_hidden,
            engine.active_group,
        );
        match zone {
            ScreenZone::TabBar {
                group_id, local_x, ..
            } => {
                assert_eq!(group_id, engine.active_group);
                assert!(
                    (local_x - 5.0).abs() < f64::EPSILON,
                    "local_x should be relative to the bar's left edge, got {local_x}"
                );
            }
            other => panic!("expected TabBar zone, got {other:?}"),
        }

        // A click below the tab bar, inside the window, must still resolve
        // to Window — the offset derivation shouldn't just widen the band
        // indefinitely.
        let zone = screen_zone_hit_test(
            &layout,
            content_x + 5.0,
            content_y + tbh + 5.0,
            tbh,
            single_tab_hidden,
            engine.active_group,
        );
        assert!(
            matches!(zone, ScreenZone::Window { .. }),
            "click below the tab bar should hit Window zone, got {zone:?}"
        );
    }

    /// #553 (click-side counterpart of #549's draw-loop unification): the
    /// single-group and split-group tab-bar hit bands come out of ONE
    /// derivation, and in both shapes the band's top edge is
    /// `window_content_top - tab_bar_height` — never a hardcoded origin.
    ///
    /// The regression this guards is asymmetric by construction: with a
    /// chrome-shifted content origin the split arm kept working (it derived the
    /// top from `GroupTabBar::bounds`) while the single arm went dead (it
    /// assumed `y >= 0.0`), which is exactly the "works with 2+ groups, dead
    /// with 1" symptom #553 reports. So both shapes are asserted here against
    /// the same offset layout.
    #[test]
    fn test_tab_bar_hit_bands_single_and_split_share_one_derivation() {
        use crate::core::engine::Engine;
        use crate::core::window::{SplitDirection, WindowRect};

        let line_height = 20.0;
        let char_width = 8.0;
        let tbh = tab_bar_height_px(line_height, false);
        // Chrome-shifted content origin — the #552 menu/title-bar band.
        let content = WindowRect::new(50.0, 100.0, 800.0, 600.0);
        let theme = Theme::onedark();

        // ── Single group ──────────────────────────────────────────────────
        let mut engine = Engine::new();
        engine.new_tab(None); // 2 tabs, so `hide_single_tab` can't suppress the bar
        let (rects, _) = engine.calculate_group_window_rects(content, tbh);
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        let bands = tab_bar_hit_bands(
            &layout,
            tbh,
            engine.is_tab_bar_hidden(engine.active_group),
            engine.active_group,
        );
        assert_eq!(bands.len(), 1, "one group draws one tab bar: {bands:?}");
        assert_eq!(bands[0].group_id, engine.active_group);
        assert_eq!(
            bands[0].y, content.y,
            "the single-group band must start at the *content* origin minus the bar height, \
             not at 0.0 (#546 FAILED-3 / #553): {bands:?}"
        );
        assert!(bands[0].contains(content.x + 5.0, content.y + 2.0));
        assert!(
            !bands[0].contains(content.x + 5.0, content.y - 1.0),
            "the band must not extend above the reserved chrome"
        );

        // ── Split groups ──────────────────────────────────────────────────
        engine.open_editor_group(SplitDirection::Vertical);
        let (rects, _) = engine.calculate_group_window_rects(content, tbh);
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        let split_bands = tab_bar_hit_bands(
            &layout,
            tbh,
            engine.is_tab_bar_hidden(engine.active_group),
            engine.active_group,
        );
        assert_eq!(
            split_bands.len(),
            2,
            "two groups draw two tab bars: {split_bands:?}"
        );
        for band in &split_bands {
            assert_eq!(
                band.y, content.y,
                "every split band uses the same content-derived top edge as the \
                 single-group one: {split_bands:?}"
            );
            assert!(band.width > 0.0);
            assert!(band.contains(band.x + 1.0, band.y + 1.0));
        }
        // The two bands tile the content width without overlapping.
        let mut xs: Vec<f64> = split_bands.iter().map(|b| b.x).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(xs[0] < xs[1], "split bands must sit side by side: {xs:?}");
    }

    /// Pins that `bc.bounds.y` (row units, matching TUI's convention) shifts
    /// up by one row when a single-tab group's tab bar is hidden
    /// (`hide_single_tab`), and down by one when it's shown. TUI's
    /// single-group breadcrumb draw used to special-case
    /// `is_tab_bar_hidden` itself instead of trusting `bc.bounds.y` (#547);
    /// this pins the equivalence that made unifying it onto
    /// `breadcrumb_draw_targets` safe — `calculate_group_window_rects` →
    /// `adjust_group_rects_for_hidden_tabs` is what actually shifts the
    /// window (and thus breadcrumb) bounds.
    #[test]
    fn test_single_group_breadcrumb_bounds_reflect_hidden_tab_bar() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 1.0; // TUI row units.
        let char_width = 1.0;
        let theme = Theme::onedark();

        let bounds_y_for = |hide_single_tab: bool| -> f64 {
            let mut engine = Engine::new();
            engine.settings.breadcrumbs = true;
            engine.settings.hide_single_tab = hide_single_tab;
            // TUI's own row-unit convention (`tui_tab_bar_height` in
            // `render_impl.rs`), NOT `tab_bar_height_px` — that helper rounds
            // to a pixel-oriented `line_height * 1.6` tab row for GTK/Win-GUI,
            // which doesn't map to a clean row count in TUI's 1-row-per-line
            // units.
            let tbh = 2.0;
            let content_bounds = WindowRect::new(0.0, 0.0, 80.0, 24.0);
            let (rects, _) = engine.calculate_group_window_rects(content_bounds, tbh);
            let layout = build_screen_layout(
                &engine,
                &theme,
                &rects,
                line_height,
                char_width,
                true,
                0.0,
                TUI_MINIMAP_SIZING,
            );
            assert!(!layout.breadcrumbs.is_empty());
            layout.breadcrumbs[0].bounds.y
        };

        // Tab bar shown (default): breadcrumb sits one row below it.
        assert_eq!(bounds_y_for(false), 1.0);
        // Tab bar hidden (single tab, hide_single_tab=true): breadcrumb
        // claims the row the tab bar would have used.
        assert_eq!(bounds_y_for(true), 0.0);
    }

    /// Direct unit test for `breadcrumb_draw_targets` itself (#547 review
    /// finding: the test above only pins the pre-existing `build_screen_layout`
    /// bounds computation, never the new shared helper). Covers the
    /// `terminal_maximized` early return, the pass-through of already-absolute
    /// bounds (#550 — the `origin_offset` translation this used to carry was
    /// dropped once both backends feed absolute window rects), the
    /// `segments.is_empty()` filter, and the zero-width fallback filter.
    #[test]
    fn test_breadcrumb_draw_targets_offset_terminal_maximized_and_filters() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 20.0;
        let char_width = 8.0;
        let theme = Theme::onedark();

        let build_screen = || {
            let mut engine = Engine::new();
            engine.settings.breadcrumbs = true;
            // A default `Engine::new()` buffer has no `file_path`, which
            // produces zero breadcrumb segments (see
            // `build_breadcrumbs_for_group`) — give it a path so the
            // non-maximized case below actually has a segment to draw.
            let buf_id = engine.active_buffer_id();
            engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
                Some(std::path::PathBuf::from("src/main.rs"));
            let tbh = 24.0;
            // Non-zero origin to prove `breadcrumb_draw_targets` passes
            // through absolute bounds untouched rather than assuming (0,0).
            let content_bounds = WindowRect::new(10.0, 20.0, 800.0, 600.0);
            let (rects, _) = engine.calculate_group_window_rects(content_bounds, tbh);
            build_screen_layout(
                &engine,
                &theme,
                &rects,
                line_height,
                char_width,
                true,
                8.0,
                gtk_minimap_sizing(),
            )
        };

        let screen = build_screen();
        assert_eq!(screen.breadcrumbs.len(), 1);
        assert!(!screen.breadcrumbs[0].segments.is_empty());
        assert!(screen.breadcrumbs[0].bounds.width > 0.0);

        // `terminal_maximized` short-circuits to empty.
        let targets = breadcrumb_draw_targets(&screen, true);
        assert!(
            targets.is_empty(),
            "terminal_maximized must suppress all breadcrumb targets"
        );

        // Not maximized: one target, matching the already-absolute bounds.
        let targets = breadcrumb_draw_targets(&screen, false);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].rect.x, screen.breadcrumbs[0].bounds.x as f32);
        assert_eq!(targets[0].rect.y, screen.breadcrumbs[0].bounds.y as f32);
        assert_eq!(
            targets[0].rect.width,
            screen.breadcrumbs[0].bounds.width as f32
        );
        assert_eq!(targets[0].rect.height, line_height as f32);

        // Empty segments are filtered out even when not maximized.
        let mut screen_no_segments = build_screen();
        screen_no_segments.breadcrumbs[0].segments.clear();
        let targets = breadcrumb_draw_targets(&screen_no_segments, false);
        assert!(
            targets.is_empty(),
            "a breadcrumb bar with no segments must not be drawn"
        );

        // Zero-width bounds (the `min_x == f64::MAX` fallback for a group with
        // no matching window rects) are filtered out too, so GTK doesn't need
        // its own `rect.width > 0.0` guard (unlike TUI's pre-existing one).
        let mut screen_zero_width = build_screen();
        screen_zero_width.breadcrumbs[0].bounds.width = 0.0;
        let targets = breadcrumb_draw_targets(&screen_zero_width, false);
        assert!(
            targets.is_empty(),
            "a zero-width breadcrumb bar must not be drawn"
        );
    }

    /// Direct unit test for `tab_bar_draw_targets` (#549, follow-up to
    /// #547's `breadcrumb_draw_targets`; rewritten for #551).
    ///
    /// The single-group case used to be served by a hand-written `else` arm
    /// that painted a caller-supplied `(x, y, width)` rect. #551 deleted it:
    /// `ScreenLayout::group_tab_bars` now holds one entry for one group, and
    /// the generic `bounds.y - reserved_h` math must reproduce *exactly* the
    /// full-width editor-top rect the deleted arm hard-coded. That equivalence
    /// is the whole point of the refactor, so it is asserted explicitly below
    /// against the `content_bounds` the caller laid the frame out with.
    #[test]
    fn test_tab_bar_draw_targets_single_and_split() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let line_height = 20.0;
        let char_width = 8.0;
        let theme = Theme::onedark();
        let tab_row_h = 32.0; // lh * 1.6
        let reserved_h = 32.0; // no breadcrumbs: reserved == tab row height

        // ── Single-group mode ───────────────────────────────────────────
        // Non-zero origin so "the generic path reproduces the old hard-coded
        // editor-origin rect" is a real claim, not a zero-origin coincidence.
        let mut engine = Engine::new();
        let content_bounds = WindowRect::new(10.0, 20.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, reserved_h);
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        assert!(screen.editor_group_split.is_none());
        // #551: the per-group chrome is populated even with a single group.
        assert_eq!(
            screen.group_tab_bars.len(),
            1,
            "one group must still produce one GroupTabBar (split-of-1)"
        );
        assert!(
            screen.group_dividers.is_empty(),
            "one group has no inter-group dividers"
        );

        let targets = tab_bar_draw_targets(&engine, &screen, tab_row_h, reserved_h);
        assert_eq!(targets.len(), 1);
        // Exactly the rect the deleted single-group arm used to hard-code from
        // the caller's editor origin/width.
        assert_eq!(targets[0].rect.x, content_bounds.x as f32);
        assert_eq!(targets[0].rect.y, content_bounds.y as f32);
        assert_eq!(targets[0].rect.width, content_bounds.width as f32);
        assert_eq!(targets[0].rect.height, tab_row_h as f32);
        assert_eq!(targets[0].group_id, engine.active_group);

        // Hiding the single group's tab bar suppresses the target entirely.
        engine.settings.hide_single_tab = true;
        let screen_hidden = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        let targets = tab_bar_draw_targets(&engine, &screen_hidden, tab_row_h, reserved_h);
        assert!(
            targets.is_empty(),
            "a hidden single-group tab bar must not be drawn"
        );

        // ── Split-group mode ────────────────────────────────────────────
        // Non-zero content_bounds origin to prove `tab_bar_draw_targets`
        // passes through absolute bounds untouched rather than assuming (0,0).
        let mut engine = Engine::new();
        engine.execute_command("EditorGroupSplit");
        assert_eq!(engine.group_layout.leaf_count(), 2);
        let content_bounds = WindowRect::new(5.0, 7.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, reserved_h);
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        assert!(
            screen.editor_group_split.is_some(),
            "2 groups must produce Some(editor_group_split)"
        );
        assert_eq!(screen.group_tab_bars.len(), 2);
        assert_eq!(
            screen.group_dividers.len(),
            1,
            "a 2-group split has exactly one inter-group divider"
        );

        // Rect derived from the already-absolute `bounds.y - reserved_h`.
        let targets = tab_bar_draw_targets(&engine, &screen, tab_row_h, reserved_h);
        assert_eq!(targets.len(), 2);
        for (target, gtb) in targets.iter().zip(screen.group_tab_bars.iter()) {
            assert_eq!(target.group_id, gtb.group_id);
            assert_eq!(target.rect.x, gtb.bounds.x as f32);
            assert_eq!(target.rect.y, (gtb.bounds.y - reserved_h) as f32);
            assert_eq!(target.rect.width, gtb.bounds.width as f32);
            assert_eq!(target.rect.height, tab_row_h as f32);
        }

        // Note: `is_tab_bar_hidden` only ever returns true in single-group
        // mode (`hide_single_tab` + `leaf_count() <= 1`, see
        // `Engine::is_tab_bar_hidden`), so there's no reachable per-group
        // "hidden" state to exercise here in split mode — the
        // `is_tab_bar_hidden` filter is defensive, matching what the
        // pre-existing per-backend loops did.

        // Zero-width bounds (the `min_x == f64::MAX` fallback) are filtered
        // out too, mirroring `breadcrumb_draw_targets`.
        let mut screen_zero_width = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            gtk_minimap_sizing(),
        );
        screen_zero_width.group_tab_bars[0].bounds.width = 0.0;
        let targets = tab_bar_draw_targets(&engine, &screen_zero_width, tab_row_h, reserved_h);
        assert_eq!(
            targets.len(),
            1,
            "a zero-width group tab bar must not be drawn"
        );
    }

    /// #551: the single `GroupTabBar` synthesised for an unsplit editor must
    /// carry byte-for-byte the same tab content the old single-group-only
    /// fields did. If these ever diverge, the unified draw path would silently
    /// paint a *different* tab bar than the pre-#551 code did — the exact
    /// class of regression #547 hit when a single-group calculation drifted
    /// from the generic one.
    #[test]
    fn test_single_group_tab_bar_matches_legacy_single_group_fields() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let theme = Theme::onedark();
        let mut engine = Engine::new();
        // More than one tab so tab labels/active flags are non-trivial.
        engine.execute_command("tabnew");
        engine.execute_command("tabnew");
        assert_eq!(engine.group_layout.leaf_count(), 1);

        let content_bounds = WindowRect::new(3.0, 4.0, 120.0, 40.0);
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, 1.0);
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        assert_eq!(screen.group_tab_bars.len(), 1);
        let gtb = &screen.group_tab_bars[0];
        assert_eq!(gtb.group_id, engine.active_group);
        assert_eq!(
            gtb.tabs.len(),
            screen.tab_bar.len(),
            "group tab list must match the legacy single-group tab list"
        );
        for (a, b) in gtb.tabs.iter().zip(screen.tab_bar.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.active, b.active);
            assert_eq!(a.dirty, b.dirty);
        }
        // #764: `ScreenLayout::tab_scroll_offset` — the single-group mirror
        // this used to compare against — is deleted; `GroupTabBar` is the only
        // copy now, so the equivalence this line asserted is structural.
        // #822: `ScreenLayout::tab_bar_hit_regions` is now a clone of this
        // group's own `TabBarLayout` (`quadraui::TabBarLayout` derives
        // `PartialEq`), so the equality is exact, not just debug-string equal.
        assert_eq!(
            gtb.hit_regions, screen.tab_bar_hit_regions,
            "group tab bar layout must match the legacy single-group mirror"
        );
        // The group's bounds must be the editor content area, i.e. the tab row
        // recovered from it lands exactly on the editor's top edge.
        assert_eq!(gtb.bounds.x, content_bounds.x);
        assert_eq!(gtb.bounds.y - 1.0, content_bounds.y);
        assert_eq!(gtb.bounds.width, content_bounds.width);
    }

    /// #551: `screen_to_drop_group_bounds` lost its
    /// origin/size/tab-bar-height parameters along with the single-group arm
    /// that needed them. The bounds it returns for an unsplit editor must
    /// still be the *content* area (past the tab bar), which is what
    /// `build_tab_drop_groups` reconstructs the tab-bar band from — the #477
    /// regression this pins.
    #[test]
    fn test_drop_group_bounds_single_group_is_content_area() {
        use crate::core::engine::Engine;
        use crate::core::window::WindowRect;

        let theme = Theme::onedark();
        let engine = Engine::new();
        let content_bounds = WindowRect::new(6.0, 2.0, 90.0, 30.0);
        let tab_bar_height = 1.0;
        let (rects, _) = engine.calculate_group_window_rects(content_bounds, tab_bar_height);
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            1.0,
            1.0,
            false,
            0.0,
            TUI_MINIMAP_SIZING,
        );

        let bounds = screen_to_drop_group_bounds(&screen);
        assert_eq!(bounds.len(), 1);
        assert_eq!(bounds[0].group_id, engine.active_group);
        assert_eq!(bounds[0].x, content_bounds.x as f32);
        assert_eq!(
            bounds[0].y,
            (content_bounds.y + tab_bar_height) as f32,
            "drop bounds start below the tab bar"
        );
        assert_eq!(bounds[0].width, content_bounds.width as f32);
        assert_eq!(
            bounds[0].content_height,
            (content_bounds.height - tab_bar_height) as f32
        );
    }

    /// Explorer tree row icons must always carry both a Nerd Font glyph and
    /// a distinct fallback (#547) — selection between them is entirely the
    /// backend's job (`Backend::set_nerd_fonts`), not `build_explorer_tree_rows`'s.
    /// This is the platform-neutral half of the #547 icon regression: the
    /// shared row-building logic was never the problem, but pinning it
    /// guards against the fix drifting back to a GTK-only icon shim.
    #[test]
    fn test_explorer_tree_rows_carry_glyph_and_fallback_icons() {
        use crate::core::engine::{Engine, ExplorerRow};
        use std::path::PathBuf;

        let engine = Engine::new();
        let theme = Theme::onedark();
        let rows = vec![
            ExplorerRow {
                depth: 0,
                name: "src".to_string(),
                path: PathBuf::from("src"),
                is_dir: true,
                is_expanded: true,
            },
            ExplorerRow {
                depth: 1,
                name: "main.rs".to_string(),
                path: PathBuf::from("src/main.rs"),
                is_dir: false,
                is_expanded: false,
            },
        ];

        let tree_rows = build_explorer_tree_rows(&rows, &engine, &theme);
        assert_eq!(tree_rows.len(), 2);
        for row in &tree_rows {
            let icon = row.icon.as_ref().expect("every explorer row has an icon");
            assert!(!icon.glyph.is_empty(), "glyph must not be empty");
            assert!(!icon.fallback.is_empty(), "fallback must not be empty");
            assert_ne!(
                icon.glyph, icon.fallback,
                "glyph and fallback must differ so backend selection is observable"
            );
        }
    }
}
