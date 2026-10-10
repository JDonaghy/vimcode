use super::*;

// ─── Overlay tail: the history the fold preserves (#735 slice 1, #766) ───────
//
// The paint twin of `route_modal_overlay_click` / `route_modal_key` above, and
// the first rung ladder of #735 to be stated once instead of twice.
//
// Frame *content* has been shared for a long time (`build_screen_layout` and
// the ~30 builders around it); frame *composition* — which surface is laid
// down, in what order — was still transcribed per backend, and had drifted in
// exactly the way #592 warned about:
//
//   GTK  menu dropdown → command centre → find/replace → picker →
//        tab switcher → **dialog → context menu** → toasts
//   TUI  find/replace → picker → tab switcher → **context menu → dialog** →
//        menu dropdown → command centre → toasts
//
// Two independent inversions in one band:
//
//  1. **dialog vs. context menu.** A modal dialog is the one surface that eats
//     every event (`route_modal_overlay_click` returns `Swallow` for anything
//     that isn't a press the moment `dialog_open` is set, ahead of every other
//     rung except toasts). On GTK it was nevertheless painted *under* the
//     context menu, so an editor/tab/explorer context menu left open when a
//     dialog opens paints on top of the thing that owns the input — input and
//     paint disagreeing, the #587/#592 failure shape.
//  2. **menu dropdown / command centre vs. the modal stack.** TUI painted the
//     title-bar band on top of every modal; GTK painted it underneath.
//
// The overlay tail of [`FRAME_Z_ORDER`] is now the single artefact both
// backends walk, so the order is no longer a property either one can hold an
// opinion about. The canonical order takes GTK's placement for the title-bar
// chrome (it is chrome, and modals should cover it) and TUI's placement for the
// modal stack (it is the one that agrees with `route_modal_overlay_click`'s own
// arbitration: tab switcher below dialog below toasts).
//
// **Not in the shared sequence, deliberately** — the rungs that exist on one
// backend only, and therefore cannot be part of a shared *sequence*:
//
//   * GTK's tab-drag drop overlay, app-icon slot and inline window controls.
//     TUI paints its tab-drag ghost in the editor band and has neither an app
//     icon nor in-canvas window controls. The app icon and window controls are
//     painted from the [`FrameOp::MenuDropdown`] arm, because
//     `MenuSystem::render` repaints the whole title-bar band and would erase
//     anything laid down before it (#676/#712).
//
// **Why here and not in quadraui** (`CLAUDE.md`'s "check quadraui first"):
// quadraui already owns the *shell* composition — `compose::app_shell::AppShell`
// hands both backends `AppShellLayout` (title-bar / activity-bar / sidebar /
// bottom-panel / main-content bounds), and vimcode consumes it verbatim. The
// overlay tail is a different thing: it is vimcode's own set of app-level
// surfaces, and quadraui's nearest neighbour — `ModalStack` — explicitly
// disclaims it ("**Painting**: the stack has no opinions on draw order. Apps
// still paint modals last (highest z); the stack is queried only when *events*
// arrive"). So the ordering is vimcode's to state, and `render.rs` is where
// vimcode states cross-backend contracts.

/// The expected frame sequence for the cross-backend **equality fixture**:
/// title bar visible, sidebar open on the settings panel, a wildmenu up, global
/// status lines on, and a context menu + an in-canvas modal dialog open
/// together.
///
/// **This is #735's headline acceptance criterion made runnable.** Both
/// backends drive an equivalent engine state and assert their recorded
/// `composed_frame` equals *this* value, so the two sequences are equal to each
/// other by construction. A single test cannot drive both backends (the GTK
/// `App` lives in the `vimcode` bin target, the pre-#1434 TUI shell in `vcd`), so "both
/// backends emit the same `FrameOp` sequence for a given `ScreenLayout`" is
/// expressed as two tests sharing one expected value — a single `#[cfg(test)]`
/// fn compiled into both bin targets, so the compiler keeps them in step
/// instead of comment discipline.
///
/// It could not be written before #766: until the chrome and overlay halves
/// were one sequence there was no single artefact to compare, only two that a
/// backend could get individually right and jointly wrong. Nine of the
/// fifteen rungs are live and six are not, which is what keeps it
/// *discriminating* — it must never degenerate into "whatever
/// [`FRAME_Z_ORDER`] contains".
#[cfg(test)]
pub(crate) fn frame_sequence_fixture() -> Vec<FrameOp> {
    compose_frame(&FramePresence {
        menu_row: true,
        sidebar_panel: true,
        wildmenu: true,
        status_bar: true,
        command_line: true,
        // Shared on both backends since #815, but not open in this fixture —
        // see `FrameOp::FolderPicker`.
        folder_picker: false,
        menu_dropdown: true,
        command_center: true,
        find_replace: false,
        unified_picker: false,
        tab_switcher: false,
        context_menu: true,
        change_review: false,
        dialog: true,
        toast_stack: false,
    })
}

/// The expected **overlay tail** for the "no app-level overlay open, only
/// title-bar chrome" cross-backend fixture
/// (`overlay_band_holds_only_the_title_bar_when_no_overlay_is_open_via_gtk_driver`
/// / `overlay_band_is_empty_when_no_overlay_is_open_via_shell_app`'s GTK twin).
///
/// GTK pins the title bar visible unconditionally (#552), so its "nothing open"
/// tail still has `MenuDropdown`/`CommandCenter` live; TUI's, with no title bar
/// reserved, is empty. Both are read off the same `composed_frame` field,
/// filtered to [`FrameOp::is_overlay`].
#[cfg(test)]
pub(crate) fn overlay_band_title_bar_only_fixture() -> Vec<FrameOp> {
    compose_frame(&FramePresence {
        menu_dropdown: true,
        command_center: true,
        ..FramePresence::default()
    })
}

// ─── QuickfixPanel ────────────────────────────────────────────────────────────

/// Data needed to render the quickfix (or location-list) bottom panel.
///
/// The same bottom "list rung" renders either the global quickfix list or
/// the active window's location list — never both at once, matching how
/// most Vim users actually work with them, and keeping the fixed
/// `BOTTOM_Z_ORDER` band stack this repo's rendering doc comments describe
/// (see the comment above [`BOTTOM_Z_ORDER`]) from having to grow a second
/// independent slot for #1155.
#[derive(Debug, Clone)]
pub struct QuickfixPanel {
    /// Formatted display strings: "file.rs:12: line text"
    pub items: Vec<String>,
    /// Currently selected item index.
    pub selected_idx: usize,
    /// Total number of items in the list.
    pub total_items: usize,
    /// Whether the quickfix panel has keyboard focus.
    pub has_focus: bool,
    /// `"QUICKFIX"` for the global list, `"LOCATION LIST"` when this panel
    /// is showing the active window's `:l*` list instead (#1155).
    pub title: &'static str,
}

/// Build a [`QuickfixPanel`] from a [`QuickfixList`] (global quickfix or a
/// per-window location list — see [`QuickfixPanel::title`]).
pub(crate) fn quickfix_list_to_panel(list: &QuickfixList, title: &'static str) -> QuickfixPanel {
    let items = list
        .items
        .iter()
        .map(|m| {
            let f = m.file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            let snippet: String = m.line_text.trim().chars().take(80).collect();
            format!("{}:{}: {}", f, m.line + 1, snippet)
        })
        .collect();
    QuickfixPanel {
        items,
        selected_idx: list.selected,
        total_items: list.items.len(),
        has_focus: list.has_focus,
        title,
    }
}

// ─── SourceControlData ────────────────────────────────────────────────────────

/// A single file-change item in the Source Control panel.
#[derive(Debug, Clone)]
pub struct ScFileItem {
    pub path: String,
    /// Single-char status label: A / M / D / R / C / ? / ! (conflict)
    pub status_char: char,
    pub is_staged: bool,
}

/// A single worktree item in the Source Control panel.
#[derive(Debug, Clone)]
pub struct ScWorktreeItem {
    pub path: String,
    pub branch: String,
    pub is_current: bool,
    pub is_main: bool,
}

/// A single git log entry in the Source Control panel.
#[derive(Debug, Clone)]
pub struct ScLogItem {
    /// Short (abbreviated) commit hash.
    pub hash: String,
    /// Commit subject line.
    pub message: String,
}

/// Rendering data for the Source Control panel sidebar.
#[derive(Debug, Clone)]
pub struct SourceControlData {
    /// Current git branch name (e.g. "main").
    pub branch: String,
    /// Number of commits ahead of the upstream.
    pub ahead: u32,
    /// Number of commits behind the upstream.
    pub behind: u32,
    /// Unmerged (conflicted) files — the "Merge Changes" section (#991).
    /// Empty in a conflict-free tree, which is what keeps that section
    /// from being always-on.
    pub merge: Vec<ScFileItem>,
    /// Staged files (index changes).
    pub staged: Vec<ScFileItem>,
    /// Unstaged / untracked files (working-tree changes).
    pub unstaged: Vec<ScFileItem>,
    /// Git worktrees.
    pub worktrees: Vec<ScWorktreeItem>,
    /// Recent git log entries.
    pub log: Vec<ScLogItem>,
    /// Which sections are expanded, indexed by the `SC_SECTION_*`
    /// constants: [merge, staged, unstaged, worktrees, log].
    pub sections_expanded: [bool; crate::core::engine::SC_SECTION_COUNT],
    /// Flat selection index.
    pub selected: usize,
    /// Whether the panel currently has keyboard focus.
    pub has_focus: bool,
    /// Commit message being typed in the input row.
    pub commit_message: String,
    /// Byte-offset cursor position within the commit message.
    pub commit_cursor: usize,
    /// True when the commit input row is in edit mode.
    pub commit_input_active: bool,
    /// Which action button is keyboard-focused (0=Commit 1=Push 2=Pull 3=Sync), or None.
    pub button_focused: Option<usize>,
    /// Which action button the mouse is hovering over, or None.
    pub button_hovered: Option<usize>,
    /// Branch picker popup data (None when closed).
    pub branch_picker: Option<BranchPickerData>,
    /// SC help dialog visible.
    pub help_open: bool,
    /// Y coordinate (in native units) where the sections area begins —
    /// the top of `SidebarPanelLayout.content_bounds` from the last paint
    /// (#509). TUI: terminal rows. GTK: pixels. `None` until first paint.
    pub sc_sections_start_y: Option<f32>,
}

/// Data for the branch picker / create popup in the SC panel.
#[derive(Debug, Clone)]
pub struct BranchPickerData {
    pub query: String,
    /// (branch_name, is_current)
    pub results: Vec<(String, bool)>,
    pub selected: usize,
    /// When true, the popup is in "create new branch" mode.
    pub create_mode: bool,
    /// The new branch name being typed (only in create mode).
    pub create_input: String,
}

// #1489: `ExtSidebarItem`/`ExtSidebarData`, `ext_sidebar_to_multi_section_view`
// and `build_ext_sidebar_data` used to live here as a second, entirely dead
// data pipeline for the Extensions sidebar — nothing ever read
// `ScreenLayout.ext_sidebar` (removed alongside them). The live path is
// `Engine::populate_ext_sidebar_system` (`core/engine/ext_panel.rs`), which
// paints straight into `engine.ext_sidebar_system` (`quadraui::SidebarSystem`).

// ─── BoardData ─────────────────────────────────────────────────────────────────

/// Rendering data for the Board panel (#521) — a generic host for the shared
/// `quadraui::Board` component. `model` comes straight from vimcode's board
/// contract (`Engine::board_model`, populated via the #522 `ToolClient`
/// seam); this type adds only what the panel's own chrome needs on top of
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardData {
    /// Whether the panel currently has keyboard focus.
    pub has_focus: bool,
    /// The last successfully fetched board, or `None` before the first
    /// fetch completes (or when no provider is configured).
    pub model: Option<quadraui::BoardModel>,
    /// A one-line status to show in place of the board when `model` is
    /// `None`: "no provider configured", "fetching…", or the last error.
    /// `None` alongside a `Some(model)` means nothing needs to be said.
    pub status: Option<String>,
}

// ─── ExtPanelData (extension-provided sidebar panels) ────────────────────────

/// Rendering data for a single extension-provided sidebar panel.
#[derive(Debug, Clone)]
pub struct ExtPanelData {
    pub name: String,
    pub title: String,
    pub sections: Vec<ExtPanelSectionData>,
    pub selected: usize,
    pub has_focus: bool,
    pub scroll_top: usize,
    pub input_text: String,
    pub input_active: bool,
    pub help_open: bool,
    pub help_bindings: Vec<(String, String)>,
}

/// A single section within an extension panel.
#[derive(Debug, Clone)]
pub struct ExtPanelSectionData {
    pub name: String,
    pub items: Vec<crate::core::plugin::ExtPanelItem>,
    pub expanded: bool,
}

// ─── PanelHoverPopupData ──────────────────────────────────────────────────────

/// Rendering data for a sidebar panel hover popup (rendered markdown).
#[derive(Debug, Clone)]
pub struct PanelHoverPopupData {
    /// Raw markdown source. Styled at paint time with the active theme —
    /// see `EditorHoverPopupData::markdown`'s doc (#821).
    pub markdown: String,
    /// Plain per-line text (markdown syntax stripped).
    pub line_text: Vec<String>,
    /// Per-line tree-sitter highlights for fenced code-block lines.
    pub code_highlights: Vec<Vec<crate::core::markdown::MdCodeHighlight>>,
    /// Clickable link regions: (line_idx, start_byte, end_byte, url).
    pub links: Vec<(usize, usize, usize, String)>,
    /// Flat item index being hovered (for positioning relative to panel).
    pub item_index: usize,
    /// The panel this hover belongs to (e.g. "source_control", ext panel name).
    pub panel_name: String,
}

/// Maximum number of sidebar-item hover popup rows shown at once
/// (matches the legacy `MAX_HEIGHT` constant it replaces — no
/// scrolling for this popup, content beyond this is truncated).
pub const PANEL_HOVER_MAX_ROWS: usize = 20;

/// Convert a `PanelHoverPopupData` into a `quadraui::RichTextPopup` for
/// the D6 layout pipeline, mirroring `editor_hover_to_quadraui_rich_text`.
/// The sidebar-item hover is read-only (no scroll, focus, selection, or
/// keyboard-link-nav state), so those fields are fixed defaults.
///
/// Placement is `Below`: callers pass `anchor_y = desired_top_row -
/// 1.0` (one row height) so the popup's top border lands exactly on
/// the row the legacy hand-rolled renderer used.
pub fn panel_hover_to_quadraui_rich_text(
    ph: &PanelHoverPopupData,
    theme: &Theme,
) -> quadraui::RichTextPopup {
    let (q_lines, line_scales) =
        markdown_hover_to_quadraui_lines(&ph.markdown, &ph.code_highlights, theme);
    let q_links = md_links_to_quadraui_rich_text_links(&ph.links);

    quadraui::RichTextPopup {
        id: quadraui::WidgetId::new("panel_hover"),
        lines: q_lines,
        line_text: ph.line_text.clone(),
        line_scales,
        scroll_top: 0,
        max_visible_rows: PANEL_HOVER_MAX_ROWS,
        has_focus: false,
        selection: None,
        links: q_links,
        focused_link: None,
        placement: quadraui::PopupPlacement::Below,
        padding: 0.0,
        fg: Some(theme.hover_fg),
        bg: Some(theme.hover_bg),
    }
}

/// On-screen content row (0-based, relative to the ext panel's own chrome)
/// for an ext-panel hover's flat `item_index` — the inverse of
/// `route_sidebar_hover`'s `SidebarOwner::ExtPanel` arm, which sets
/// `flat_idx = engine.ext_panel_scroll_top + row` (`render.rs`, just above
/// [`route_sidebar_hover`]). #1087: the anchor code used to skip this
/// subtraction entirely and anchor to the flat index as if it were a screen
/// row, so anything scrolled past the first screenful painted dozens of rows
/// below the viewport.
///
/// Returns `None` when `item_index` is behind the current scroll offset,
/// which can happen for one stale frame immediately after a scroll (the
/// hover popup should simply not paint that frame — an `f32` cast of a
/// wrapped `usize` subtraction would otherwise park it at infinity).
pub(crate) fn ext_panel_hover_screen_row(panel: &ExtPanelData, item_index: usize) -> Option<usize> {
    item_index.checked_sub(panel.scroll_top)
}

/// Chrome rows an ext panel paints above its first content row: the header,
/// plus the search-input row when it's visible. Mirrors `render_ext_panel`'s
/// own `chrome_h` (`tui_main/panels.rs`) — the same condition
/// `mouse.rs`'s `SidebarOwner::ExtPanel` click arm uses for
/// `SidebarBodyGeometry::header_rows` (#1086) — so hover and click can't
/// drift on what counts as chrome, *except* in the degenerate case
/// `render_ext_panel` guards and this doesn't: its `chrome_h` is
/// `.min(area.height)`, clamped to whatever the panel's viewport actually
/// has room for, while this always returns 1 or 2 regardless of viewport
/// size. Only matters for an ext-panel area under 2 rows tall (an
/// unusably narrow sidebar), where a hover anchor could drift by a row
/// from what was actually painted — not worth threading `area.height`
/// through the hover path for that corner case today, but a future
/// `SidebarBodyGeometry`-style unification (#1086) should derive both from
/// one place.
pub(crate) fn ext_panel_chrome_rows(panel: &ExtPanelData) -> usize {
    if panel.input_active || !panel.input_text.is_empty() {
        2
    } else {
        1
    }
}

/// Vertical anchor (top of the hovered row, in the caller's line units) for
/// [`panel_hover_popup_paint`]. Lifted from the now-dead
/// `src/gtk/draw.rs::draw_panel_hover_popup`'s source-control section walk
/// (#670) so both backends can share it instead of GTK re-deriving its own
/// copy. The non-source-control branch is generalized to take an explicit
/// `sidebar_top_y` rather than assuming the sidebar starts at row/pixel 0 —
/// true in the pre-#552 single-DA GTK architecture that dead code was
/// written against, no longer true now that a title-bar row can sit above
/// the sidebar.
///
/// Returns `None` when the anchor can't be computed this frame (#1087's
/// scroll-underflow guard, or no `screen.ext_panel` yet) — callers should
/// skip painting the popup for that frame rather than clamp to a garbage
/// position.
fn panel_hover_anchor_y(
    screen: &ScreenLayout,
    hover: &PanelHoverPopupData,
    sidebar_top_y: f32,
    unit_h: f32,
) -> Option<f32> {
    if hover.panel_name != "source_control" {
        // #1087: `hover.item_index` is a flat index across the whole panel
        // list, not a screen row — subtract the scroll offset back out, and
        // use the chrome rows this panel actually painted (1 or 2,
        // depending on whether the search input row is showing) instead of
        // the literal `1` this used to hardcode.
        let panel = screen.ext_panel.as_ref()?;
        let screen_row = ext_panel_hover_screen_row(panel, hover.item_index)?;
        let chrome_rows = ext_panel_chrome_rows(panel);
        return Some(sidebar_top_y + chrome_rows as f32 * unit_h + screen_row as f32 * unit_h);
    }
    // SC layout: `section_top` is read from the cached `SidebarPanelLayout`
    // (`sc_sections_start_y`, already an absolute coordinate — see its own
    // field doc) so this doesn't re-derive it; falls back to a one-frame-lag
    // estimate from the commit box's line count when that cache is still
    // empty (e.g. the very first frame the SC panel is shown).
    let item_height = (unit_h * 1.4).round();
    let section_top = screen
        .source_control
        .as_ref()
        .and_then(|sc| sc.sc_sections_start_y)
        .unwrap_or_else(|| {
            let gap = (unit_h * 0.3).round();
            let commit_rows = screen
                .source_control
                .as_ref()
                .map(|sc| sc.commit_message.split('\n').count().max(1))
                .unwrap_or(1) as f32;
            unit_h + gap + commit_rows * unit_h + unit_h
        });
    let Some(ref sc) = screen.source_control else {
        return Some(section_top + hover.item_index as f32 * unit_h);
    };
    // Walk sections to find the accumulated Y offset for the hovered flat
    // index. Headers occupy one row; expanded items occupy `item_height`
    // each. Staged + Unstaged always show; Worktrees only when there's more
    // than one; Log always shows — mirrors the SC sidebar's own section
    // list.
    use crate::core::engine::{
        SC_SECTION_CHANGES, SC_SECTION_LOG, SC_SECTION_MERGE, SC_SECTION_STAGED,
        SC_SECTION_WORKTREES,
    };
    let show_worktrees = sc.worktrees.len() > 1;
    let mut sections: Vec<(usize, bool)> = Vec::new();
    // #991: Merge Changes sits above Staged and is present only when the
    // tree has conflicts — mirrors `Engine::sc_visible_sections`.
    if !sc.merge.is_empty() {
        sections.push((sc.merge.len(), sc.sections_expanded[SC_SECTION_MERGE]));
    }
    sections.push((sc.staged.len(), sc.sections_expanded[SC_SECTION_STAGED]));
    sections.push((sc.unstaged.len(), sc.sections_expanded[SC_SECTION_CHANGES]));
    if show_worktrees {
        sections.push((
            sc.worktrees.len(),
            sc.sections_expanded[SC_SECTION_WORKTREES],
        ));
    }
    sections.push((sc.log.len(), sc.sections_expanded[SC_SECTION_LOG]));

    let mut y_off = section_top;
    let mut fi = 0usize;
    'outer: for &(count, expanded) in &sections {
        if fi == hover.item_index {
            break;
        }
        y_off += unit_h;
        fi += 1;
        if expanded {
            for _ in 0..count {
                if fi == hover.item_index {
                    break 'outer;
                }
                y_off += item_height;
                fi += 1;
            }
        }
    }
    Some(y_off)
}

/// Paint the sidebar-item hover popup (source-control / extension-panel item
/// dwell tooltip, rendered markdown) through the shared
/// `quadraui::RichTextPopup` primitive — the panel-hover twin of
/// [`editor_hover_popup_paint`] (#670). GTK previously hand-rolled this in
/// raw Cairo/Pango (`src/gtk/draw.rs::draw_panel_hover_popup`, no live
/// callers since the #540 Relm4->ShellApp migration); this instead reuses
/// the same `RichTextPopup` / `Backend::draw_rich_text_popup` path TUI's
/// `tui_main::panels::render_panel_hover_popup` already routes through, so
/// the two backends can't drift on the markdown rendering itself — only the
/// anchor geometry (source-control section walk vs. uniform per-row offset)
/// is backend-specific, and that's shared too via [`panel_hover_anchor_y`].
///
/// `unit_w` / `unit_h` are `1.0, 1.0` for TUI (cell-native) or `char_width,
/// line_height` in pixels for GTK. `popup_x` / `sidebar_top_y` / `viewport`
/// must already be expressed in that same space. As with
/// `editor_hover_popup_paint`, `RichTextPopup::layout`'s own
/// `PopupPlacement::Below` clamping against `viewport` means callers don't
/// need to pre-check whether the popup fits — it re-clamps precisely.
///
/// Returns `(link_rects, popup_bounds)` in the caller's units. Link rects
/// carry a trailing `is_native` flag — `true` for the source-control panel's
/// trusted links (open directly), `false` for extension-provided ones
/// (confirm before opening) — mirroring GTK's retired `Msg::PanelHoverClick`'s two
/// branches.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn panel_hover_popup_paint(
    backend: &mut dyn quadraui::Backend,
    screen: &ScreenLayout,
    theme: &Theme,
    popup_x: f32,
    sidebar_top_y: f32,
    viewport: quadraui::Rect,
    unit_w: f32,
    unit_h: f32,
) -> (Vec<(quadraui::Rect, String, bool)>, Option<quadraui::Rect>) {
    let Some(ref hover) = screen.panel_hover else {
        return (vec![], None);
    };
    if hover.line_text.is_empty() {
        return (vec![], None);
    }
    let is_native = panel_hover_link_is_native(&hover.panel_name);
    let popup = panel_hover_to_quadraui_rich_text(hover, theme);
    let max_len = popup
        .line_text
        .iter()
        .map(|t| t.chars().count())
        .max()
        .unwrap_or(10) as f32;
    let avail_w = (viewport.x + viewport.width - popup_x).max(10.0 * unit_w);
    let content_w = ((max_len + 2.0) * unit_w)
        .max(10.0 * unit_w)
        .min((avail_w - 2.0 * unit_w).max(10.0 * unit_w));
    // #1087: `None` means the anchor can't be trusted this frame (e.g. the
    // scroll-underflow guard in `ext_panel_hover_screen_row`) — skip
    // painting rather than fall back to a stale/garbage position.
    let Some(anchor_y) = panel_hover_anchor_y(screen, hover, sidebar_top_y, unit_h) else {
        return (vec![], None);
    };
    let measure = quadraui::RichTextPopupMeasure::new(content_w, unit_h);
    // `Placement::Below` adds one row height to the anchor, so subtract it
    // here to land the box's top border exactly on `anchor_y` — same trick
    // `editor_hover_popup_paint`/TUI's `render_panel_hover_popup` use.
    let layout = popup.layout(
        popup_x,
        anchor_y - unit_h,
        viewport,
        measure,
        |line_idx, start_byte, end_byte| {
            popup
                .line_text
                .get(line_idx)
                .map(|t| {
                    t[start_byte.min(t.len())..end_byte.min(t.len())]
                        .chars()
                        .count() as f32
                })
                .unwrap_or(0.0)
                * unit_w
        },
    );

    backend.draw_rich_text_popup(&popup, &layout);

    let link_rects: Vec<(quadraui::Rect, String, bool)> = layout
        .link_hit_regions
        .iter()
        .map(|(rect, idx)| {
            let url = popup
                .links
                .get(*idx)
                .map(|l| l.url.clone())
                .unwrap_or_default();
            (*rect, url, is_native)
        })
        .collect();

    let popup_rect = Some(layout.bounds);
    (link_rects, popup_rect)
}

// ─── SettingDef ───────────────────────────────────────────────────────────────

// SettingType, SettingDef, and SETTING_DEFS are defined in settings.rs and
// re-exported at the top of this file for backward compatibility.

/// Always present in `ScreenLayout`. Only the chrome-level fields that
/// `debug_sidebar_chrome` and the bottom-panel Debug Output tab actually
/// read survive here; the item lists (variables/watch/frames/breakpoints)
/// were a dead second pipeline — the live rows come from
/// `build_dap_{var,watch,stack,bp}_rows` → `populate_dap_sidebar_system`
/// (#1489).
#[derive(Debug, Clone)]
pub struct DebugSidebarData {
    pub session_active: bool,
    pub stopped: bool,
    pub launch_config_name: Option<String>,
    pub debug_output_lines: Vec<String>,
}

/// The two bottom panel tabs: Terminal and Debug Output.
#[derive(Debug)]
pub struct BottomPanelTabs {
    /// Which tab is currently active.
    pub active: BottomPanelKind,
    /// Terminal panel data (always built if terminal is open, regardless of active tab).
    pub terminal: Option<TerminalPanel>,
    /// Debug output lines for the Debug Output tab.
    pub output_lines: Vec<String>,
}

// ─── TerminalPanel ────────────────────────────────────────────────────────────

/// Data needed to render the integrated terminal bottom panel.
#[derive(Debug)]
pub struct TerminalPanel {
    /// Rendered cell grid: `rows[content_row][col]` — quadraui cells with all
    /// overlay flags (cursor, selection, find-match) already applied.
    pub rows: Vec<Vec<quadraui::TerminalCell>>,
    /// Number of content rows (excluding toolbar).
    pub content_rows: u16,
    /// Number of columns.
    pub content_cols: u16,
    /// Whether the terminal panel has keyboard focus.
    pub has_focus: bool,
    /// Rows scrolled up into scrollback (0 = live view).
    pub scroll_offset: usize,
    /// Number of scrollback rows stored in the VT100 parser buffer.
    pub scrollback_rows: usize,
    /// Total number of terminal tabs.
    pub tab_count: usize,
    /// Index of the currently active tab.
    pub active_tab: usize,
    /// Whether the inline find bar is open.
    pub find_active: bool,
    /// Current find query string.
    pub find_query: String,
    /// Total number of matches found.
    pub find_match_count: usize,
    /// Index (0-based) of the currently highlighted match.
    pub find_selected_idx: usize,
    /// In split view: cell grid for the LEFT pane (pane[0]).
    /// When `Some`, the main `rows` field represents the RIGHT pane (pane[1]).
    /// `None` in normal (non-split) mode.
    pub split_left_rows: Option<Vec<Vec<quadraui::TerminalCell>>>,
    /// Column count of the left pane in split view.
    pub split_left_cols: u16,
    /// Which pane has keyboard focus in split view: 0 = left, 1 = right.
    pub split_focus: u8,
    /// Whether the panel is currently maximized (fills editor area).
    /// Backends can render a different icon glyph based on this.
    pub maximized: bool,
}

/// Terminal scrollbar thumb position as fractions of track height.
/// Both backends use this for painting and `SurfaceScrollbar` registration.
#[derive(Debug, Clone, Copy)]
pub struct TerminalScrollbarGeom {
    pub thumb_top_frac: f64,
    pub thumb_height_frac: f64,
    pub total_items: usize,
    pub visible_items: usize,
}

/// Returns `None` when there's no scrollback (thumb fills entire track).
pub fn terminal_scrollbar_geometry(
    panel: &TerminalPanel,
    visible_rows: usize,
) -> Option<TerminalScrollbarGeom> {
    if panel.scrollback_rows == 0 {
        return None;
    }
    let total = panel.scrollback_rows + visible_rows;
    let thumb_frac = (visible_rows as f64 / total as f64).max(0.01);
    let max_off = panel.scrollback_rows as f64;
    let frac = if panel.scroll_offset == 0 {
        1.0
    } else {
        1.0 - (panel.scroll_offset as f64 / max_off).min(1.0)
    };
    let thumb_top_frac = frac * (1.0 - thumb_frac);
    Some(TerminalScrollbarGeom {
        thumb_top_frac,
        thumb_height_frac: thumb_frac,
        total_items: total,
        visible_items: visible_rows,
    })
}

/// Pre-built terminal primitives ready for `Backend::draw_terminal`.
/// Both backends call `build_terminal_draw_data` and then just do the
/// backend-specific drawing (clear background, enter frame scope, divider).
pub struct TerminalDrawData {
    pub single: Option<quadraui::Terminal>,
    pub left: Option<quadraui::Terminal>,
    pub right: Option<quadraui::Terminal>,
    pub split: Option<quadraui::TerminalSplitLayout>,
}

pub fn build_terminal_draw_data(
    panel: &TerminalPanel,
    area: quadraui::Rect,
    cell_width: f32,
    cell_height: f32,
    visible_rows: usize,
    sb_width: Option<u16>,
) -> TerminalDrawData {
    let sb = Some(quadraui::TerminalScrollbar {
        total_lines: panel.scrollback_rows + visible_rows,
        visible_lines: visible_rows,
        scroll_offset: panel.scroll_offset,
        inverted: true,
        width: sb_width,
    });
    if let Some(ref left_rows) = panel.split_left_rows {
        let sb_px = sb_width.unwrap_or(0) as f32;
        let split = quadraui::TerminalSplitLayout::new(
            area,
            panel.split_left_cols as usize,
            cell_width,
            cell_height,
            sb_px,
        );
        let left = quadraui::Terminal {
            id: quadraui::WidgetId::new("terminal:left"),
            cells: left_rows.clone(),
            scrollbar: None,
        };
        let right = quadraui::Terminal {
            id: quadraui::WidgetId::new("terminal:right"),
            cells: panel.rows.clone(),
            scrollbar: sb,
        };
        TerminalDrawData {
            single: None,
            left: Some(left),
            right: Some(right),
            split: Some(split),
        }
    } else {
        let term = quadraui::Terminal {
            id: quadraui::WidgetId::new("terminal:pane"),
            cells: panel.rows.clone(),
            scrollbar: sb,
        };
        TerminalDrawData {
            single: Some(term),
            left: None,
            right: None,
            split: None,
        }
    }
}
