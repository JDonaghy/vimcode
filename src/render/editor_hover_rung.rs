use super::*;

// ─── Editor hover popup rung ──────────────────────────────────────────────────────
//
// Four consecutive rungs arbitrate clicks in the hover popup. Both backends had
// independent implementations that drifted in several ways:
//
//  * **Order.** TUI tested link rects first, GTK tested scrollbar first. The
//    scrollbar is painted on top, so scrollbar-first matches what the user sees.
//  * **Position in the ladder.** TUI ran this above the scroll-surface dispatch;
//    GTK ran it below, so scrollbar clicks were eaten by scroll surfaces behind.
//  * **Thumb grab offset.** TUI preserved cursor offset within the thumb; GTK
//    hardcoded 0.0, teleporting the thumb under the cursor.
//  * **Track click.** TUI armed a drag; GTK computed a separate ratio-based
//    offset. One seek, computed once, now.
//  * **Link hit-test.** TUI required exact row match; GTK used <= on edges,
//    causing adjacent rects to overlap. Uses half-open [x, x+w) × [y, y+h).
//  * **command: links.** TUI left popup up covering the jumped-to definition;
//    GTK dismissed it. Both dismiss now.
//
// Only two steps are genuinely per-backend: what "open a plain URL" means
// (clipboard on TUI, browser on GTK), and drag registration.

/// The content-box metrics of the painted hover popup: how far the text is
/// inset from the popup's own top-left, and the cell size it is laid out on.
///
/// TUI measures in whole cells (`col_width`/`line_height` of 1.0, a 2×1
/// inset); GTK in pixels. Expressing both as an inset plus a cell size is
/// what lets one `content_line`/`content_col` inverse serve both.
#[derive(Debug, Clone, Copy)]
pub struct PopupContentMetrics {
    pub pad_x: f32,
    pub pad_y: f32,
    pub col_width: f32,
    pub line_height: f32,
}

/// Everything a backend cached at paint time for the editor hover popup.
///
/// All rects are in the backend's own coordinate space — the router never
/// converts, it only compares, so cells and pixels both work unchanged.
pub struct EditorHoverPopupState<'a> {
    /// Popup bounds as painted, `None` when nothing was painted this frame.
    pub popup: Option<quadraui::Rect>,
    /// Painted link rects and their URIs, in paint order.
    pub links: &'a [(quadraui::Rect, String)],
    /// Painted scrollbar track/thumb, `None` when the content fits.
    pub scrollbar: Option<PopupScrollbarHit>,
    /// `Engine::editor_hover_has_focus`.
    pub has_focus: bool,
    /// Where the popup's text starts, and the cell size it uses.
    pub content: PopupContentMetrics,
}

/// What a left-press on (or near) the editor hover popup means.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorHoverPopupRoute {
    /// A `command:` URI link — navigate, then dismiss.
    Command(String),
    /// A plain URL link — the backend opens or copies it, then dismisses.
    Link(String),
    /// The popup's scrollbar was grabbed. Begin this drag, then apply it.
    Scrollbar(Box<quadraui::DragTarget>),
    /// An unfocused popup's body was clicked — give it focus.
    Focus,
    /// A focused popup's body was clicked — start a text selection there.
    StartSelection {
        content_line: usize,
        content_col: usize,
    },
    /// The click missed a visible popup: dismiss it, but let the press fall
    /// through so the cursor still lands where the user aimed (rather than
    /// costing them a second click).
    DismissAndFallThrough,
    /// No popup is visible — nothing to arbitrate.
    None,
}

/// Half-open hit test, `[x, x+w) × [y, y+h)` — the convention every other
/// rect test in this file uses, and the one that makes abutting link rects
/// tile without overlapping.
fn rect_contains(r: quadraui::Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Arbitrate a left-press against the painted editor hover popup.
///
/// Called by TUI's `handle_mouse` and GTK's `handle_mouse_click_msg` /
/// `handle_mouse_double_click_msg` **before** either backend's scroll
/// surfaces, so the popup — which paints on top of the editor — also wins the
/// click that lands on it.
///
/// Reads only `visible` (`Engine::editor_hover.is_some()`) and the caller's
/// painted geometry, so it is directly unit-testable without an `Engine`.
pub fn route_editor_hover_popup_click(
    visible: bool,
    state: &EditorHoverPopupState<'_>,
    x: f64,
    y: f64,
) -> EditorHoverPopupRoute {
    if !visible {
        return EditorHoverPopupRoute::None;
    }
    let (cx, cy) = (x as f32, y as f32);
    let Some(popup) = state.popup.filter(|p| rect_contains(*p, cx, cy)) else {
        return EditorHoverPopupRoute::DismissAndFallThrough;
    };

    // 1. Scrollbar — painted on top of the content, so it is tested first.
    if let Some(sb) = state.scrollbar {
        let on_thumb = rect_contains(sb.thumb, cx, cy);
        let on_track = !on_thumb && rect_contains(sb.track, cx, cy);
        if on_thumb || on_track {
            return EditorHoverPopupRoute::Scrollbar(Box::new(quadraui::DragTarget::ScrollbarY {
                widget: quadraui::WidgetId::new("editor_hover"),
                track_start: sb.track.y,
                track_length: sb.track.height,
                thumb_length: sb.thumb.height,
                max_scroll: sb.total.saturating_sub(sb.visible_rows),
                // Grabbing the thumb keeps the cursor where it landed on
                // it; clicking the empty track seeks the thumb's *top* to
                // the cursor, which is what a single shared apply-pass
                // produces without a second bespoke ratio calculation.
                grab_offset: if on_thumb { cy - sb.thumb.y } else { 0.0 },
                inverted: false,
            }));
        }
    }

    // 2. Links — first painted rect that contains the point wins.
    for (rect, uri) in state.links {
        if rect_contains(*rect, cx, cy) {
            return if uri.starts_with("command:") {
                EditorHoverPopupRoute::Command(uri.clone())
            } else {
                EditorHoverPopupRoute::Link(uri.clone())
            };
        }
    }

    // 3. Body — focus it, or (once focused) start a selection in it.
    if !state.has_focus {
        return EditorHoverPopupRoute::Focus;
    }
    let m = state.content;
    let rel_x = (cx - popup.x - m.pad_x).max(0.0);
    let rel_y = (cy - popup.y - m.pad_y).max(0.0);
    EditorHoverPopupRoute::StartSelection {
        content_line: (rel_y / m.line_height.max(1.0)) as usize,
        content_col: (rel_x / m.col_width.max(1.0)) as usize,
    }
}

/// What the caller still has to do after [`apply_editor_hover_popup_route`]
/// has mutated the engine.
#[derive(Debug, Default)]
pub struct EditorHoverPopupEffect {
    /// `true` when the press belongs to the popup and must not fall through.
    pub consumed: bool,
    /// A drag to register on this backend's own `DragState` handle, then
    /// apply once at the press position.
    pub begin_drag: Option<quadraui::DragTarget>,
    /// A plain URL to open (GTK) or copy (TUI) — the one genuinely
    /// per-backend step in the whole rung.
    pub open_url: Option<String>,
    /// `true` when a popup text selection was started, so the backend can arm
    /// its drag follow-through.
    pub selecting: bool,
}

/// Apply an [`EditorHoverPopupRoute`] to the engine.
///
/// `scroll_top` is added to the router's viewport-relative `content_line`
/// here rather than in the router so the router stays engine-free.
pub fn apply_editor_hover_popup_route(
    engine: &mut Engine,
    route: EditorHoverPopupRoute,
) -> EditorHoverPopupEffect {
    let mut effect = EditorHoverPopupEffect {
        consumed: true,
        ..Default::default()
    };
    match route {
        EditorHoverPopupRoute::None => effect.consumed = false,
        EditorHoverPopupRoute::DismissAndFallThrough => {
            engine.dismiss_editor_hover();
            effect.consumed = false;
        }
        EditorHoverPopupRoute::Command(uri) => {
            engine.execute_hover_goto(&uri);
            // Leaving the popup up would cover the definition just jumped to
            // — TUI's old copy did exactly that.
            engine.dismiss_editor_hover();
        }
        EditorHoverPopupRoute::Link(url) => {
            effect.open_url = Some(url);
            engine.dismiss_editor_hover();
        }
        EditorHoverPopupRoute::Scrollbar(target) => effect.begin_drag = Some(*target),
        EditorHoverPopupRoute::Focus => engine.editor_hover_focus(),
        EditorHoverPopupRoute::StartSelection {
            content_line,
            content_col,
        } => {
            let scroll = engine
                .editor_hover
                .as_ref()
                .map(|h| h.scroll_top)
                .unwrap_or(0);
            engine.editor_hover_start_selection(content_line + scroll, content_col);
            effect.selecting = true;
        }
    }
    effect
}

/// Is a panel-hover link "native" (source-control, trusted, open directly)
/// or extension-provided? Mirrors `panel_hover_popup_paint`'s own
/// `is_native` derivation so the paint step and any future confirm-before-
/// open policy can't independently drift on what counts as trusted. Neither
/// backend currently branches on this — see [`PanelHoverPopupRoute`]'s doc.
pub fn panel_hover_link_is_native(panel_name: &str) -> bool {
    panel_name == "source_control"
}

/// What a left-press on a painted panel-hover-popup link means: run a command
/// (with code: URI) or open a URL, and dismiss the popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelHoverPopupRoute {
    /// A `command:` URI link — run it, then dismiss.
    Command(String),
    /// A plain URL link — the backend opens or copies it, then dismisses.
    Link(String),
    /// The press didn't land on a link.
    None,
}

/// Arbitrate a left-press against the panel-hover popup's painted link
/// rects. `links` carries the trailing `is_native` flag
/// [`panel_hover_popup_paint`] produces; unused for now (see
/// [`panel_hover_link_is_native`]'s doc) but kept so both backends' caches
/// share one shape instead of TUI silently dropping the field.
pub fn route_panel_hover_popup_click(
    links: &[(quadraui::Rect, String, bool)],
    x: f64,
    y: f64,
) -> PanelHoverPopupRoute {
    let (cx, cy) = (x as f32, y as f32);
    for (rect, uri, _is_native) in links {
        if rect_contains(*rect, cx, cy) {
            return if uri.starts_with("command:") {
                PanelHoverPopupRoute::Command(uri.clone())
            } else {
                PanelHoverPopupRoute::Link(uri.clone())
            };
        }
    }
    PanelHoverPopupRoute::None
}

/// What the caller still has to do after [`apply_panel_hover_popup_route`]
/// has mutated the engine.
#[derive(Debug, Default)]
pub struct PanelHoverPopupEffect {
    /// `true` when the press landed on a link and was consumed here.
    pub consumed: bool,
    /// A plain URL to open (GTK) or copy (TUI) — the one genuinely
    /// per-backend step, same split as [`EditorHoverPopupEffect::open_url`].
    pub open_url: Option<String>,
}

/// Apply a [`PanelHoverPopupRoute`] to the engine.
pub fn apply_panel_hover_popup_route(
    engine: &mut Engine,
    route: PanelHoverPopupRoute,
) -> PanelHoverPopupEffect {
    let mut effect = PanelHoverPopupEffect::default();
    match route {
        PanelHoverPopupRoute::None => {}
        PanelHoverPopupRoute::Command(uri) => {
            engine.execute_command_uri(&uri);
            engine.dismiss_panel_hover_now();
            effect.consumed = true;
        }
        PanelHoverPopupRoute::Link(url) => {
            effect.open_url = Some(url);
            engine.dismiss_panel_hover_now();
            effect.consumed = true;
        }
    }
    effect
}

/// The painted divider geometry for one frame, plus the caller's grab metrics.
#[derive(Debug, Clone, Copy)]
pub struct DividerState<'a> {
    /// Boundaries between editor groups, as painted.
    pub group_dividers: &'a [GroupDivider],
    /// `:split`/`:vsplit` boundaries within each group, as painted.
    pub window_dividers: &'a [WindowDivider],
    /// The caller's grab tolerances.
    pub metrics: DividerMetrics,
    /// `true` when this point sits on a group's tab bar and the caller wants
    /// tab-bar clicks to reach the tab handlers instead of arming a group
    /// divider.
    ///
    /// Only GTK sets this. On TUI a *horizontal* group divider **is** the lower
    /// group's whole tab-bar block (there is no separate glyph to aim at), which
    /// is why `group_horizontal`'s tolerance runs `(0.0, tab_bar_rows)` there —
    /// excluding the tab bar would make horizontal group splits unresizable.
    pub on_tab_bar: bool,
}

/// Arbitrate a left-press against the painted dividers.
///
/// Group dividers are tested first because they are the outer boundary: a
/// window split lives *inside* one group, so a point on a group boundary can
/// never also be on a window boundary of the group it separates, and testing
/// the outer one first keeps the nesting order explicit.
pub fn route_divider_grab(state: &DividerState<'_>, x: f64, y: f64) -> Option<DividerGrab> {
    if !state.on_tab_bar {
        if let Some(i) = divider_hit_test(
            state.group_dividers,
            x,
            y,
            state.metrics.group_vertical,
            state.metrics.group_horizontal,
            state.metrics.quantize,
        ) {
            return Some(DividerGrab::Group {
                split_index: state.group_dividers[i].split_index,
            });
        }
    }
    let i = divider_hit_test(
        state.window_dividers,
        x,
        y,
        state.metrics.window_vertical,
        state.metrics.window_horizontal,
        state.metrics.quantize,
    )?;
    let div = &state.window_dividers[i];
    Some(DividerGrab::Window {
        group_id: div.group_id,
        split_index: div.split_index,
    })
}

/// Push a pointer position into the ratio of an already-grabbed divider.
///
/// Returns `true` when a divider matching `grab` was found and its ratio
/// updated, so the caller can consume the event and request a redraw. The
/// divider slices are the *current* frame's geometry — the grab is remembered
/// by `split_index`, not by a snapshot of the divider, so a resize that
/// reflows the layout mid-drag keeps tracking the right boundary.
pub fn apply_divider_drag(
    engine: &mut Engine,
    grab: DividerGrab,
    group_dividers: &[GroupDivider],
    window_dividers: &[WindowDivider],
    x: f64,
    y: f64,
) -> bool {
    match grab {
        DividerGrab::Group { split_index } => {
            let Some(div) = group_dividers.iter().find(|d| d.split_index == split_index) else {
                return false;
            };
            let ratio = divider_ratio_from_pos(div, x, y);
            engine.group_layout.set_ratio_at_index(split_index, ratio);
            true
        }
        DividerGrab::Window {
            group_id,
            split_index,
        } => {
            let Some(div) = window_dividers
                .iter()
                .find(|d| d.group_id == group_id && d.split_index == split_index)
            else {
                return false;
            };
            let ratio = divider_ratio_from_pos(div, x, y);
            let Some(group) = engine.editor_groups.get_mut(&group_id) else {
                return false;
            };
            group
                .active_tab_mut()
                .layout
                .set_ratio_at_index(split_index, ratio);
            true
        }
    }
}

/// What [`TabDragState::handle_move`] wants from the caller after a move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabDragMove {
    /// Nothing armed and nothing dragging — the caller continues down its own
    /// mouse ladder.
    Idle,
    /// A press is armed but the pointer has not travelled far enough yet.
    /// **Consume the event** and do nothing: this is what stops a twitchy click
    /// on a tab from starting a drag, and equally what stops the move from
    /// falling through and extending an editor text selection instead.
    Pending,
    /// The threshold was just crossed. The caller must decide whether the
    /// *press* point was really on a tab — TUI knows it was (only the tab-bar
    /// arm arms the drag), GTK re-resolves it through `pixel_to_click_target`
    /// because its arm fires for the whole tab-bar band — and then call
    /// [`TabDragState::begin`] or [`TabDragState::disarm`]. Consume either way.
    Crossed { press_x: f64, press_y: f64 },
    /// A drag is live: compute the drop zone for `(x, y)` in the caller's own
    /// geometry and hand it to [`TabDragState::track`]. Consume.
    Tracking,
}

/// The arm → threshold → track → commit state machine behind tab drag-and-drop,
/// held as a single field by both backends.
///
/// Five parallel fields per backend became one, which makes the invariants
/// checkable: a `source` can only exist while `dragging`, and `press` and
/// `dragging` are never both set.
#[derive(Debug, Clone)]
pub struct TabDragState {
    /// Where the left button went down inside a tab bar, until either the
    /// threshold is crossed or the button is released.
    press: Option<(f64, f64)>,
    /// `(group, tab index)` being dragged; `Some` exactly while `dragging`.
    source: Option<(GroupId, usize)>,
    /// Latest pointer position, for the drag ghost.
    cursor: Option<(f64, f64)>,
    /// Latest computed drop zone, for the commit. No longer read by the drop
    /// *overlay* — recomputed fresh from painted geometry each frame.
    zone: crate::core::window::DropZone,
    dragging: bool,
}

impl Default for TabDragState {
    fn default() -> Self {
        Self {
            press: None,
            source: None,
            cursor: None,
            zone: crate::core::window::DropZone::None,
            dragging: false,
        }
    }
}

impl TabDragState {
    /// Remember a left-press inside a tab bar as a *potential* drag.
    pub fn arm(&mut self, x: f64, y: f64) {
        self.press = Some((x, y));
    }

    /// Forget any armed press without starting a drag.
    pub fn disarm(&mut self) {
        self.press = None;
    }

    /// `true` while a drag is live — the gate both backends' drop overlays
    /// paint behind.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// `true` while the machine would consume a move — either a live drag or an
    /// armed press still under the travel threshold.
    ///
    /// [`route_mouse_drag`] asks this rather than `is_dragging` because a
    /// still-armed press must also win the event: it is the move that promotes
    /// it, and letting a lower rung claim that move first is how an
    /// almost-a-drag turns into a stray text selection.
    pub fn is_armed_or_dragging(&self) -> bool {
        self.dragging || self.press.is_some()
    }

    /// Latest pointer position during a live drag, for the ghost label.
    pub fn cursor(&self) -> Option<(f64, f64)> {
        self.cursor
    }

    /// `(group, tab index)` being dragged, for the ghost label's text.
    pub fn source(&self) -> Option<(GroupId, usize)> {
        self.source
    }

    /// Advance the machine for a left-button move at `(x, y)`.
    ///
    /// `threshold_sq` is the squared travel distance that promotes an armed
    /// press into a drag, in the caller's own units: `2.0` for TUI (which is
    /// exactly the old Manhattan `dx + dy >= 2` over integer cells — the
    /// minimum of `dx² + dy²` subject to `|dx| + |dy| = 2` is 2, and any
    /// Manhattan distance ≤ 1 has `dx² + dy² ≤ 1`) and `64.0` for GTK's 8
    /// device pixels.
    pub fn handle_move(&mut self, x: f64, y: f64, threshold_sq: f64) -> TabDragMove {
        if self.dragging {
            self.cursor = Some((x, y));
            return TabDragMove::Tracking;
        }
        let Some((px, py)) = self.press else {
            return TabDragMove::Idle;
        };
        let (dx, dy) = (x - px, y - py);
        if dx * dx + dy * dy >= threshold_sq {
            TabDragMove::Crossed {
                press_x: px,
                press_y: py,
            }
        } else {
            TabDragMove::Pending
        }
    }

    /// Promote a crossed threshold into a live drag of `source`.
    pub fn begin(&mut self, source: (GroupId, usize), x: f64, y: f64) {
        self.source = Some(source);
        self.cursor = Some((x, y));
        self.zone = crate::core::window::DropZone::None;
        self.dragging = true;
        self.press = None;
    }

    /// Record the drop zone the caller computed for the current pointer
    /// position. No-op unless a drag is live.
    pub fn track(&mut self, zone: crate::core::window::DropZone) {
        if self.dragging {
            self.zone = zone;
        }
    }

    /// Commit on left-release. Applies the pending drop through
    /// `Engine::apply_tab_drop_zone` and clears the machine; returns `true`
    /// when a drag was live (so the caller consumes and redraws).
    ///
    /// Always clears any armed press, drag or not — a release without travel is
    /// a plain click and must not leave the machine armed for the *next*
    /// unrelated move.
    pub fn handle_release(&mut self, engine: &mut Engine) -> bool {
        self.press = None;
        if !self.dragging {
            return false;
        }
        self.dragging = false;
        let zone = std::mem::replace(&mut self.zone, crate::core::window::DropZone::None);
        if let Some((gid, tab_idx)) = self.source.take() {
            engine.apply_tab_drop_zone(gid, tab_idx, zone);
        }
        self.cursor = None;
        true
    }
}

// ═══ Drag-follow-through rung ═════════════════════════════════════════════════════
//
// This rung arbitrates events after a press: pointer moves with the left button
// still held. Both backends had independent, separately-ordered copies that drifted
// in three ways:
//
//  1. **The minimap had no TUI drag arm.** GTK checks `apply_minimap_click` first
//     in `handle_mouse_drag`, so holding the button keeps seeking. TUI's minimap
//     arm sat below an `if ev.kind != Down(Left)` gate, making its `Drag` match
//     unreachable — press-and-hold scrolled once and froze.
//
//  2. **The armed-scrollbar table was split.** Both run `quadraui::dispatch_mouse_drag`
//     and switch on `ScrollOffsetChanged`. TUI's switch knew `explorer:sb`,
//     `ext_panel:sb`, `tui:search_results`, `debug_sidebar:*` and
//     `tui:editor:N:vsb|hsb`; GTK's knew `picker` and `editor:h_sb:N`; the three
//     ids they shared were written out twice. Today each missing id names a surface
//     only the other backend registers, making the failure mode silent.
//     [`apply_scroll_offset`] is the union, once.
//
//  3. **Orders disagreed on which gesture wins.** TUI ran sidebar-resize →
//     hover-popup-selection → sidebar-body → explorer-DnD → tab-drag →
//     command-line → armed-scrollbar; GTK ran armed-scrollbar →
//     hover-popup-selection → modal-swallow → tab-drag → divider. Divergence
//     becomes visible when surfaces overlap. [`route_mouse_drag`] states the
//     order once; tests pin it and verify both backends land on the same rung.
//
// Per backend: the *apply* half for gestures whose arithmetic is in the backend's
// units — terminal-panel resize clamp and split-divider column clamp. Policy
// (which gesture owns the event) moved here; unit conversion stays per backend.

/// Which rung of the pointer-drag ladder owns a move-with-button-held event.
///
/// Resolved by [`route_mouse_drag`] from geometry and drag bookkeeping alone —
/// no engine mutation — so a test can ask "who would win here?" on either
/// backend without driving a real gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseDragRoute {
    /// A `quadraui::DragState` target is armed — a scrollbar or picker thumb.
    /// The caller runs `dispatch_mouse_drag` and feeds the resulting
    /// `ScrollOffsetChanged` ids to [`apply_scroll_offset`].
    ArmedTarget,
    /// The editor hover popup is mid text-selection.
    HoverPopupSelection,
    /// The point is inside an open modal and nothing is armed — swallow it so
    /// the drag cannot leak to the editor underneath.
    ModalSwallow,
    /// The sidebar separator is being dragged to resize the sidebar.
    SidebarResize,
    /// The sidebar body owns the gesture: the search / settings form
    /// controllers' own drag handling.
    SidebarBody,
    /// An explorer drag-and-drop is in flight — a file/folder row was picked up
    /// and the gesture tracks the row under the pointer until release. See
    /// [`apply_explorer_drag_move`] and [`apply_explorer_drop`]. Split out of
    /// [`Self::SidebarBody`] so the two gestures are independently testable.
    ExplorerDnd,
    /// The tab drag machine ([`TabDragState`]) is armed or already tracking.
    TabDrag,
    /// Command-line / message-line text selection.
    CommandLine,
    /// A group or `:split` divider is grabbed.
    Divider,
    /// The terminal split's vertical divider is being dragged.
    TerminalSplitDivider,
    /// The bottom panel's top edge is being dragged (panel resize).
    TerminalPanelResize,
    /// The pointer is over a minimap strip with nothing armed. Continues armed as
    /// a scroll drag on press ([`minimap_press`]), so subsequent moves hit
    /// [`Self::ArmedTarget`] instead; this arm is a no-op for drags that started
    /// elsewhere and swept over the minimap.
    Minimap,
    /// The pointer is inside the terminal's content rows — extend the
    /// terminal's own selection (or forward the move to the child).
    TerminalContent,
    /// The editor text area — extend the visual selection.
    EditorText,
    /// Nothing owns it.
    None,
}

/// Whether `drag_state` is armed with the rung [`MouseDragRoute::ArmedTarget`]
/// means: a scrollbar or picker thumb, not a text selection.
///
/// TUI's editor click path arms the shared `DragState` with
/// `DragTarget::TextSelection` so a later `Drag` event can recover which window
/// the selection started in. That gesture belongs to [`MouseDragRoute::EditorText`],
/// not `ArmedTarget`, whose handler only reacts to `ScrollOffsetChanged`.
/// GTK never arms `TextSelection` on its shared `DragState`, so this is a no-op
/// there today — called from both backends anyway so the exclusion is stated once.
pub fn drag_state_arms_scrollbar(drag_state: &quadraui::DragState) -> bool {
    drag_state.is_active()
        && !matches!(
            drag_state.target(),
            Some(quadraui::DragTarget::TextSelection { .. })
        )
}

/// Everything [`route_mouse_drag`] needs, in the caller's own units.
///
/// The booleans are the caller's drag bookkeeping (its own `dragging_sidebar` /
/// `divider_grab` / … flags); the rects are what the last frame actually
/// painted. Nothing here is recomputed by the router.
#[derive(Debug, Clone, Copy)]
pub struct MouseDragState<'a> {
    /// The layout the last frame painted, for the minimap and editor hit tests.
    pub layout: Option<&'a ScreenLayout>,
    /// Whether an `ArmedTarget`-rung gesture (scrollbar/picker thumb) is in
    /// progress. Compute with [`drag_state_arms_scrollbar`] — **not**
    /// `quadraui::DragState::is_active()` directly; see that function's doc
    /// comment for why the two differ on TUI.
    pub armed_target: bool,
    /// A text selection is in progress inside the editor hover popup.
    pub hover_popup_selecting: bool,
    /// `ModalStack::hit_test(point).is_some()`.
    pub modal_hit: bool,
    /// The sidebar separator is being dragged.
    pub sidebar_resizing: bool,
    /// The sidebar body's painted bounds, when that body wants drag events
    /// (the search / settings form controllers). `None` when the sidebar is
    /// hidden or its panel has no drag behaviour. Explorer DnD used to be armed
    /// here too; see [`Self::explorer_dnd_active`] for why it split.
    pub sidebar_body: Option<quadraui::Rect>,
    /// [`TabDragState`] is armed or dragging.
    pub tab_dragging: bool,
    /// A command-line / message-line selection is in progress.
    pub command_line_selecting: bool,
    /// A divider is grabbed.
    pub divider_grabbed: bool,
    /// The terminal split divider is being dragged.
    pub terminal_split_dragging: bool,
    /// The bottom panel is being resized.
    pub terminal_panel_resizing: bool,
    /// An explorer drag-and-drop is in flight. Armed rather than
    /// geometric, for the same reason [`Self::tab_dragging`]/
    /// `text_selection_active` are: once a row has been picked up the
    /// gesture belongs to the tree even while the pointer is outside the
    /// sidebar (dragged away, no target yet), so a geometric test alone
    /// would hand the move to the editor the moment the pointer left the
    /// panel. Callers compute this as
    /// `explorer_drag_src.is_some() || explorer_drag_active.is_some()`.
    pub explorer_dnd_active: bool,
    /// `Engine::mouse_drag_active` — a previous move in this gesture already
    /// extended the editor's visual selection. Armed (not geometric) so a later
    /// move straying over the minimap or terminal panel keeps extending the
    /// selection instead of being stolen by geometry, like a native text-selection
    /// drag past a window's edge. Set from engine state (shared by both backends)
    /// rather than from `armed_target` because this field ensures that exclusion
    /// keeps a geometry-steals-gesture bug from re-opening.
    pub text_selection_active: bool,
    /// `true` when the point is inside a painted terminal pane's content cells
    /// — ask [`in_terminal_pane_content`], which both backends call so the
    /// question cannot be answered differently on each.
    pub in_terminal_content: bool,
    /// One text cell in the caller's units: `(char_width, line_height)`.
    /// `(1.0, 1.0)` on TUI, the measured font advance/line height on GTK.
    pub cell: (f64, f64),
}

impl Default for MouseDragState<'_> {
    fn default() -> Self {
        Self {
            layout: None,
            armed_target: false,
            hover_popup_selecting: false,
            modal_hit: false,
            sidebar_resizing: false,
            sidebar_body: None,
            tab_dragging: false,
            command_line_selecting: false,
            divider_grabbed: false,
            terminal_split_dragging: false,
            terminal_panel_resizing: false,
            explorer_dnd_active: false,
            text_selection_active: false,
            in_terminal_content: false,
            // Cell metrics default to the TUI's whole-cell grid; GTK always
            // states its measured font metrics explicitly.
            cell: (1.0, 1.0),
        }
    }
}

/// Arbitrate a pointer move with the left button held.
///
/// The order below is the converged one; see this section's banner for what the
/// two per-backend orders it replaced each got wrong. Two properties are load
/// bearing and pinned by tests:
///
/// * **Armed gestures beat geometry.** Every flag-driven rung (an armed drag
///   target, a live divider grab, a tab drag, …) is tested before any hit test,
///   because a gesture that has already started must not be stolen by whatever
///   the pointer happens to fly over mid-drag.
/// * **The minimap beats the editor text area.** The strip is carved off the
///   active window's right edge, so `find_window_at` matches there too; testing
///   the editor first would make press-and-hold on the strip select text
///   instead of seeking, which is the GTK behaviour TUI was missing.
pub fn route_mouse_drag(state: &MouseDragState<'_>, x: f64, y: f64) -> MouseDragRoute {
    // ── Armed gestures, in the order they can be armed ──────────────────────
    if state.armed_target {
        return MouseDragRoute::ArmedTarget;
    }
    if state.hover_popup_selecting {
        return MouseDragRoute::HoverPopupSelection;
    }
    if state.sidebar_resizing {
        return MouseDragRoute::SidebarResize;
    }
    if state.explorer_dnd_active {
        return MouseDragRoute::ExplorerDnd;
    }
    if state.tab_dragging {
        return MouseDragRoute::TabDrag;
    }
    if state.command_line_selecting {
        return MouseDragRoute::CommandLine;
    }
    if state.divider_grabbed {
        return MouseDragRoute::Divider;
    }
    if state.terminal_split_dragging {
        return MouseDragRoute::TerminalSplitDivider;
    }
    if state.terminal_panel_resizing {
        return MouseDragRoute::TerminalPanelResize;
    }
    if state.text_selection_active {
        return MouseDragRoute::EditorText;
    }

    // ── Modal swallow ───────────────────────────────────────────────────────
    // Below the armed rungs (a scrollbar thumb *inside* a modal is an armed
    // target and must keep dragging) and above every geometric rung (nothing
    // painted under a modal may see the event).
    if state.modal_hit {
        return MouseDragRoute::ModalSwallow;
    }

    // ── Geometry ────────────────────────────────────────────────────────────
    if state
        .sidebar_body
        .is_some_and(|r| x >= r.x as f64 && x < (r.x + r.width) as f64 && y >= r.y as f64)
    {
        return MouseDragRoute::SidebarBody;
    }
    if let Some(layout) = state.layout {
        if minimap_click_line(layout, x, y).is_some() {
            return MouseDragRoute::Minimap;
        }
    }
    if state.in_terminal_content {
        return MouseDragRoute::TerminalContent;
    }
    // The editor area needs no left-edge gate: `layout.windows` holds only the
    // painted editor windows, so a point outside them simply matches none.
    if let Some(layout) = state.layout {
        if let Some(idx) = find_window_at(layout, x, y) {
            let rw = &layout.windows[idx];
            let zone = window_zone_hit_test(
                rw,
                x - rw.rect.x,
                y - rw.rect.y,
                state.cell.1.max(f64::MIN_POSITIVE),
                state.cell.0.max(f64::MIN_POSITIVE),
            );
            if matches!(zone, WindowZone::TextArea { .. }) {
                return MouseDragRoute::EditorText;
            }
        }
    }
    MouseDragRoute::None
}

/// `true` when `(x, y)` is inside a painted terminal pane's *content* cells —
/// the [`MouseDragRoute::TerminalContent`] gate, asked the same way on both
/// backends so the two cannot disagree about where the terminal starts.
///
/// Deliberately the same resolver the press path uses
/// ([`route_bottom_panel_click`]), so a press that anchored a terminal
/// selection and the drags that extend it can never land in different spaces.
pub fn in_terminal_pane_content(
    engine: &Engine,
    x: f64,
    y: f64,
    metrics: BottomPanelMetrics,
) -> bool {
    matches!(
        route_bottom_panel_click(engine, x, y, metrics),
        Some(BottomPanelRoute::Pane { .. })
            | Some(BottomPanelRoute::Split(
                quadraui::TerminalSplitHit::LeftPane { .. }
                    | quadraui::TerminalSplitHit::RightPane { .. }
            ))
    )
}

/// Apply a pointer *drag* inside the bottom panel's content rows, reading the
/// same pane-local cell the press path resolved. Returns `true` when consumed.
pub fn apply_terminal_content_drag(
    engine: &mut Engine,
    x: f64,
    y: f64,
    metrics: BottomPanelMetrics,
) -> bool {
    match route_bottom_panel_click(engine, x, y, metrics) {
        Some(BottomPanelRoute::Pane { col, row_offset }) => {
            engine.handle_terminal_pane_drag(col, row_offset);
            true
        }
        Some(BottomPanelRoute::Split(
            quadraui::TerminalSplitHit::LeftPane { col, row }
            | quadraui::TerminalSplitHit::RightPane { col, row },
        )) => {
            engine.handle_terminal_pane_drag(col, row);
            true
        }
        _ => false,
    }
}

/// Per-frame context [`apply_scroll_offset`] needs for the widgets whose apply
/// depends on more than the offset itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollApplyContext {
    /// Rows the unified picker's result list can show, for the selection clamp
    /// [`apply_picker_scroll_offset`] applies. `0` when no picker is open.
    pub picker_visible_rows: usize,
}

/// Apply one `quadraui::UiEvent::ScrollOffsetChanged` to the scroll state the
/// `widget` id names. Returns `true` when the id was recognised.
///
/// This is the union of the two tables described in point 2 of this section's
/// banner, so a widget that scrolls on one backend now scrolls on both. Ids
/// that a backend never emits simply never match there — the cost of the union
/// is a few dead arms, and the cost of *not* unioning it was a silently
/// non-scrolling scrollbar.
///
/// Some ids are handled entirely inside quadraui's `SidebarSystem`
/// (`tui:search_results`, `debug_sidebar:*`); they return `true` so the caller
/// still consumes the event rather than letting it fall through to the editor.
pub fn apply_scroll_offset(
    engine: &mut Engine,
    widget: &str,
    new_offset: usize,
    ctx: ScrollApplyContext,
) -> bool {
    match widget {
        "picker" => {
            apply_picker_scroll_offset(engine, new_offset, ctx.picker_visible_rows);
            true
        }
        "explorer:sb" => {
            engine
                .explorer_tree
                .borrow_mut()
                .set_scroll_offset(new_offset);
            true
        }
        "ext_panel:sb" => {
            engine.ext_panel_scroll_top = new_offset;
            true
        }
        "editor_hover" => {
            engine.editor_hover_set_scroll(new_offset);
            true
        }
        // Inverted scrollbars: `Terminal::set_scroll_offset` and
        // `debug_output_scroll` both mean "lines from the bottom", and
        // `dispatch_mouse_drag` already reports the offset in that space.
        "terminal_scrollback" => {
            if let Some(term) = engine.active_terminal_mut() {
                term.set_scroll_offset(new_offset);
            }
            true
        }
        // TUI's drag path emits the `tui:`-prefixed id, its click path and GTK
        // emit the bare one. Both name the same surface.
        "debug_output" | "tui:debug_output" => {
            engine.debug_output_scroll = new_offset;
            engine.debug_output_auto_scroll = false;
            true
        }
        "tui:settings" => {
            engine.settings_scroll_top = new_offset;
            true
        }
        // Owned by quadraui's `SidebarSystem` — consume without applying.
        "tui:search_results" => true,
        other if other.starts_with("debug_sidebar:") => true,
        // Editor window scrollbars. TUI encodes both axes as
        // `tui:editor:<window_id>:<vsb|hsb>`; the shared dispatch encodes them as
        // `editor:h_sb:<window_id>` / `editor:v_sb:<window_id>`.
        other if other.starts_with("tui:editor:") => {
            let Some((wid_str, axis)) = other["tui:editor:".len()..].split_once(':') else {
                return false;
            };
            let Ok(wid) = wid_str.parse::<usize>() else {
                return false;
            };
            let window_id = crate::core::WindowId(wid);
            match axis {
                "vsb" => {
                    engine.set_scroll_top_for_window(window_id, new_offset);
                    engine.sync_scroll_binds();
                    true
                }
                "hsb" => {
                    engine.set_scroll_left_for_window(window_id, new_offset);
                    true
                }
                _ => false,
            }
        }
        other if other.starts_with("editor:h_sb:") => {
            let Ok(wid) = other["editor:h_sb:".len()..].parse::<usize>() else {
                return false;
            };
            engine.set_scroll_left_for_window(crate::core::WindowId(wid), new_offset);
            true
        }
        // GTK's own v-scrollbar rung — mirrors `editor:h_sb:`
        // immediately above, one id per axis.
        other if other.starts_with("editor:v_sb:") => {
            let Ok(wid) = other["editor:v_sb:".len()..].parse::<usize>() else {
                return false;
            };
            let window_id = crate::core::WindowId(wid);
            engine.set_scroll_top_for_window(window_id, new_offset);
            engine.sync_scroll_binds();
            true
        }
        // The minimap's viewport-highlight thumb. Armed on press; continued
        // drag-moves apply here like the editor's v-scrollbar, never re-running
        // the centring jump that only happens at press.
        other if other.starts_with("minimap:") => {
            let Ok(wid) = other["minimap:".len()..].parse::<usize>() else {
                return false;
            };
            let window_id = crate::core::WindowId(wid);
            engine.set_scroll_top_for_window(window_id, new_offset);
            engine.sync_scroll_binds();
            true
        }
        _ => false,
    }
}

// ═══ Panels rung ══════════════════════════════════════════════════════════════════
//
// The rung beneath the divider rung: once no modal, chrome band or resize handle
// claims the point, the *panels* get a look — the bottom panel (terminal / debug
// output), the activity bar's sidebar band, and the sidebar's hover feedback.
//
// What was wrong with the two transcriptions this replaces:
//
//  1. **The quickfix band height had three different rules.** The painter reserves
//     rows only for a quickfix with items, but TUI's handler asked `if
//     engine.quickfix.open` in four places. `:copen` on an empty list moved every
//     band below the editor six rows away, so clicks hit the wrong surface.
//     [`quickfix_panel_rows`] is the one rule, from the painter.
//
//  2. **GTK measured the terminal pane's columns from the wrong origin.** The
//     bottom panel is painted at the editor's left edge (right of activity bar
//     and sidebar), and TUI duly did `col.saturating_sub(editor_left)`. GTK did
//     a bare `x / cached_char_width` against window-absolute `x`, so every click
//     with the sidebar open landed columns to the right of what the user aimed at.
//     [`BottomPanelMetrics::panel_left`] makes the origin an input.
//
//  3. **The `terminal_open` gate disagreed.** TUI refused to route unless
//     `engine.terminal_open`, GTK whenever geometry existed. The router asks
//     only what `Engine::resolve_bottom_panel_zone` returns.
//
//  4. **The sidebar hover rung existed on TUI and was blank on GTK** (#499/#484).
//     Geometry was cross-backend; only the ~78 lines that read it were TUI-only.
//     [`route_sidebar_hover`] is that code, once, in caller units.
//
// Per backend: TUI's terminal scrollback scrollbar. Only TUI paints a track and
// arms a drag for it. Geometry ([`terminal_scrollback_drag_target`]) is stated
// here so GTK reads the same numbers when it grows one.

/// Rows the quickfix/location-list panel occupies, as the **painter**
/// reserves them.
///
/// The single source of truth for "how tall is the quickfix band" on the
/// mouse-routing side, matching `compute_editor_layout`'s `quickfix_rows`
/// exactly — including the `!quickfix.items.is_empty()` term that TUI's four
/// hand-rolled `if engine.quickfix.open { 6 }` copies all omitted (see this
/// section's banner, point 1), and, since #1155, also the active window's
/// location list — the two share one bottom "list rung"
/// ([`QuickfixPanel::title`]).
pub fn quickfix_panel_rows(engine: &Engine) -> u16 {
    // A target with a real `WindowLayout` leaf reserves its own
    // space via the split tree, exactly like any other window — this
    // overlay band must not *also* reserve rows for it, or the two would
    // double up. `qf_has_real_window` is `false` for every existing caller
    // that only ever pokes `open`/`items` directly (see its own doc
    // comment), so this stays exactly as before for them.
    if engine.qf_has_real_window(None) || engine.qf_has_real_window(Some(engine.active_window_id()))
    {
        return 0;
    }
    let loc_open = engine
        .location_lists
        .get(&engine.active_window_id())
        .is_some_and(|l| l.open && !l.items.is_empty());
    if (engine.quickfix.open && !engine.quickfix.items.is_empty()) || loc_open {
        6
    } else {
        0
    }
}

/// The caller's own bottom-panel geometry, in the caller's own units.
///
/// Everything else the router needs is already cached on the engine at paint
/// time (`bottom_panel_geometry`, `terminal_split_layout`) by whichever backend
/// painted the frame, so these two numbers are the whole of the per-backend
/// input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BottomPanelMetrics {
    /// Left edge of the painted panel — the editor's left edge, i.e. right of
    /// the activity bar and sidebar. Cell column on TUI, pixels on GTK.
    ///
    /// Pane presses are reported *panel-relative* because that is what
    /// `Engine::handle_terminal_pane_press` documents ("0-based cells within
    /// the pane"); getting this wrong is bug 2 in the section banner.
    pub panel_left: f64,
    /// Width of one content column: `1.0` on TUI, the char advance on GTK.
    pub col_width: f64,
}

/// Where a press inside the bottom panel landed.
///
/// Resolved from geometry alone — no engine mutation — so a caller can look
/// before it leaps (GTK's right-click path needs "is this the terminal?"
/// without actually driving the terminal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BottomPanelRoute {
    /// The shared "TERMINAL / DEBUG CONSOLE" tab strip.
    TabBar,
    /// The per-panel toolbar row (terminal tab strip or find bar).
    Toolbar,
    /// A hit inside a split terminal's own layout — divider, pane gutter or
    /// scrollback track.
    Split(quadraui::TerminalSplitHit),
    /// A plain (unsplit) terminal pane press, already translated into
    /// pane-local cells.
    Pane { col: u16, row_offset: u16 },
}

/// Resolve a press at `(x, y)` against the bottom panel as last painted.
///
/// Returns `None` when the point is outside the panel — which is also the
/// answer when no panel is painted at all, because
/// `Engine::resolve_bottom_panel_zone` reads the geometry the painter cached
/// and clears (see the section banner, point 3: this replaced two different
/// `terminal_open` gates that were both trying to ask this).
pub fn route_bottom_panel_click(
    engine: &Engine,
    x: f64,
    y: f64,
    metrics: BottomPanelMetrics,
) -> Option<BottomPanelRoute> {
    use crate::core::engine::BottomPanelZone;
    if x < metrics.panel_left {
        return None;
    }
    let zone = engine.resolve_bottom_panel_zone(y)?;
    let geom = (*engine.bottom_panel_geometry.borrow())?;
    Some(match zone {
        BottomPanelZone::TabBar => BottomPanelRoute::TabBar,
        BottomPanelZone::Toolbar => BottomPanelRoute::Toolbar,
        BottomPanelZone::Content { row_offset } => {
            let split = *engine.terminal_split_layout.borrow();
            if let Some(sl) = split {
                // The split layout hit-tests in the *absolute* space it was
                // built in. TUI reconstructs that y from the cached geometry
                // (its `row_offset` has already lost the panel origin);
                // recomputing it here rather than at each call site is what
                // stops the two backends drifting on which `y` they pass.
                let abs_y = geom.top_y + geom.content_y + row_offset as f64 * geom.content_row_h;
                BottomPanelRoute::Split(sl.hit_test(x as f32, abs_y as f32))
            } else {
                let col = ((x - metrics.panel_left) / metrics.col_width.max(f64::EPSILON)) as u16;
                BottomPanelRoute::Pane { col, row_offset }
            }
        }
    })
}

/// The scrollback-scrollbar drag a [`quadraui::TerminalSplitHit::Scrollbar`]
/// arms, in the caller's own units.
///
/// Only TUI paints a scrollback track today, so only TUI calls this — but the
/// numbers are derived from the same cached `bottom_panel_geometry` both
/// backends write, so a future GTK track needs no new arithmetic.
pub fn terminal_scrollback_drag_target(engine: &Engine) -> Option<quadraui::DragTarget> {
    let geom = (*engine.bottom_panel_geometry.borrow())?;
    let track_start = (geom.top_y + geom.content_y) as f32;
    let track_length = (geom.height - geom.content_y).max(0.0) as f32;
    let total = engine
        .active_terminal()
        .map(|t| t.history_len())
        .unwrap_or(0);
    Some(quadraui::DragTarget::ScrollbarY {
        widget: quadraui::WidgetId::new("terminal_scrollback"),
        track_start,
        track_length,
        thumb_length: (track_length / total.max(1) as f32).max(1.0),
        max_scroll: total,
        grab_offset: 0.0,
        inverted: true,
    })
}

/// What the caller still has to do after [`apply_bottom_panel_route`].
///
/// Both backends kept these as bare `bool` locals set from inside the arm
/// (`*dragging_terminal_resize`, `self.terminal_resize_dragging`, …); returning
/// them makes the arm's contract explicit and stops one backend quietly
/// growing a follow-up the other lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BottomPanelEffect {
    /// A terminal-split divider grab started — track it until mouse-up.
    pub split_drag: bool,
    /// The panel's own resize handle was grabbed.
    pub resize_drag: bool,
    /// The panel's tab strip changed which panel is showing, so the caller
    /// must re-run its layout (GTK's `handle_resize`).
    pub relayout: bool,
}

/// Apply a resolved [`BottomPanelRoute`].
///
/// `toolbar_ctx` is the caller's terminal sizing context, used only by the
/// `Toolbar` arm. Focus bookkeeping is done here so both backends agree on it.
pub fn apply_bottom_panel_route(
    engine: &mut Engine,
    route: BottomPanelRoute,
    x: f64,
    toolbar_ctx: crate::core::engine::UiEventContext,
) -> BottomPanelEffect {
    use crate::core::engine::TerminalToolbarAction;
    let mut effect = BottomPanelEffect::default();
    match route {
        BottomPanelRoute::TabBar => {
            engine.handle_bottom_tab_bar_click(x);
            effect.relayout = true;
        }
        BottomPanelRoute::Toolbar => {
            engine.terminal_has_focus = true;
            let action = engine.resolve_terminal_toolbar_click(x);
            if !engine.execute_terminal_toolbar_action(action, toolbar_ctx)
                && matches!(action, TerminalToolbarAction::StartResize)
            {
                effect.resize_drag = true;
            }
        }
        BottomPanelRoute::Split(hit) => {
            engine.terminal_has_focus = true;
            // The button/mods are passed so a split-pane click can
            // `forward_mouse(Press)` to a child that has mouse reporting on.
            effect.split_drag = engine.handle_terminal_split_click(
                hit,
                quadraui::MouseButton::Left,
                quadraui::Modifiers::default(),
            );
        }
        BottomPanelRoute::Pane { col, row_offset } => {
            engine.terminal_has_focus = true;
            engine.handle_terminal_pane_press(
                col,
                row_offset,
                quadraui::MouseButton::Left,
                quadraui::Modifiers::default(),
            );
        }
    }
    effect
}

/// Which panel owns the sidebar body this frame.
///
/// Derived exactly once, from the same two engine fields `render_content` /
/// `render_sidebar` dispatch on, so the click router, the hover router and the
/// painter can never disagree about who is on screen. The precedence —
/// `ext_panel_active` **first**, `app_shell.active_panel_id()` second,
/// Explorer as the fallback — is the rule both backends had written out
/// longhand (TUI as an `if ext_panel_name.is_some() … else if
/// active_panel_is(…)` chain, GTK as a `format!("ext:{name}")` string).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarOwner {
    Explorer,
    Search,
    Debug,
    Git,
    Extensions,
    Settings,
    Ai,
    /// The Board panel.
    Board,
    /// A plugin-provided panel, by bare name (no `ext:` prefix).
    ExtPanel(String),
    /// A panel id nothing paints — a click on it belongs to whatever is
    /// underneath, not to the sidebar.
    Unknown,
}

impl SidebarOwner {
    /// The `app_shell` panel id this owner corresponds to, for the arms that
    /// still need to talk to the engine in strings.
    pub fn panel_id(&self) -> Option<&'static str> {
        use crate::core::engine::sidebar::*;
        Some(match self {
            SidebarOwner::Explorer => PANEL_EXPLORER,
            SidebarOwner::Search => PANEL_SEARCH,
            SidebarOwner::Debug => PANEL_DEBUG,
            SidebarOwner::Git => PANEL_GIT,
            SidebarOwner::Extensions => PANEL_EXTENSIONS,
            SidebarOwner::Settings => PANEL_SETTINGS,
            SidebarOwner::Ai => PANEL_AI,
            SidebarOwner::Board => PANEL_BOARD,
            SidebarOwner::ExtPanel(_) | SidebarOwner::Unknown => return None,
        })
    }

    /// The full engine panel-id string this owner was resolved from — covers
    /// `ExtPanel` (reconstructing its `"ext:{name}"` form) and `Unknown` (the
    /// empty string). Used by callers that need the id as a string to match
    /// against the `PANEL_*` constants rather than this enum.
    pub fn panel_id_string(&self) -> String {
        match self {
            SidebarOwner::ExtPanel(name) => format!("ext:{name}"),
            SidebarOwner::Unknown => String::new(),
            other => other.panel_id().unwrap_or_default().to_string(),
        }
    }
}

/// Resolve [`SidebarOwner`] for the current frame.
pub fn sidebar_owner(engine: &Engine) -> SidebarOwner {
    use crate::core::engine::sidebar::*;
    if let Some(name) = engine.ext_panel_active.as_ref() {
        return SidebarOwner::ExtPanel(name.clone());
    }
    let id = engine
        .app_shell
        .active_panel_id()
        .map(|id| id.as_str().to_string())
        .unwrap_or_else(|| PANEL_EXPLORER.to_string());
    match id.as_str() {
        PANEL_EXPLORER => SidebarOwner::Explorer,
        PANEL_SEARCH => SidebarOwner::Search,
        PANEL_DEBUG => SidebarOwner::Debug,
        PANEL_GIT => SidebarOwner::Git,
        PANEL_EXTENSIONS => SidebarOwner::Extensions,
        PANEL_SETTINGS => SidebarOwner::Settings,
        PANEL_AI => SidebarOwner::Ai,
        PANEL_BOARD => SidebarOwner::Board,
        other => match other.strip_prefix("ext:") {
            Some(name) => SidebarOwner::ExtPanel(name.to_string()),
            None => SidebarOwner::Unknown,
        },
    }
}

/// What an activity-bar panel switch left behind, for the caller's own
/// sidebar bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityPanelSwitch {
    /// Whether the sidebar is showing after the switch.
    pub sidebar_visible: bool,
    /// The plugin panel now owning the sidebar body, if any. TUI mirrors this
    /// into `TuiSidebar::ext_panel_name`.
    pub ext_panel: Option<String>,
}

/// Activate the activity-bar item for `panel_id` — click-on-an-icon behaviour.
///
/// `panel_id` is either a built-in `PANEL_*` id or an `ext:{name}` plugin panel.
/// Plugin panels bypass `AppShell` entirely, creating asymmetry both backends had
/// grown their own copy to handle. The two copies had drifted:
///
///   * **Focus clear was TUI-only.** A plugin panel taking over the sidebar must
///     drop whatever panel's focus flag was left set (since `app_shell`'s
///     active-panel id is deliberately *not* moved for plugins). Without it a
///     stale focus flag keeps old sidebar focus state showing.
///   * **Re-entry guard was GTK-only.** TUI re-fired the plugin-focus event on
///     *every* activation, including re-showing the already-active panel, so
///     clicking twice scrolled the list back to the top.
pub fn apply_activity_panel_switch(engine: &mut Engine, panel_id: &str) -> ActivityPanelSwitch {
    match panel_id.strip_prefix("ext:") {
        Some(name) => {
            let already_showing = engine.ext_panel_active.as_deref() == Some(name);
            if already_showing && engine.app_shell.sidebar_visible() {
                // Second click on the active plugin panel's icon — VS Code
                // hides the sidebar rather than re-showing it.
                engine.app_shell.hide_sidebar();
                engine.ext_panel_has_focus = false;
                engine.ext_panel_active = None;
            } else {
                engine.clear_sidebar_focus();
                if !engine.app_shell.sidebar_visible() {
                    engine.toggle_sidebar();
                }
                engine.ext_panel_active = Some(name.to_string());
                engine.ext_panel_has_focus = true;
                if !already_showing {
                    engine.ext_panel_selected = 0;
                    engine.on_ext_panel_focused(name);
                }
            }
        }
        None => {
            engine.ext_panel_has_focus = false;
            engine.ext_panel_active = None;
            engine.toggle_sidebar_panel(panel_id);
        }
    }
    engine.session.explorer_visible = engine.app_shell.sidebar_visible();
    let _ = engine.session.save();
    ActivityPanelSwitch {
        sidebar_visible: engine.app_shell.sidebar_visible(),
        ext_panel: engine.ext_panel_active.clone(),
    }
}

/// The painted sidebar body, in the caller's own units.
///
/// `row_h`/`header_rows` backed a uniform-row-height hit-test; that arm now routes
/// through the cached tree layout instead. The fields stay for future uniform-pitch
/// owners, but only `bounds` (via [`Self::contains_x`]) is read today.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarBodyGeometry {
    pub bounds: quadraui::Rect,
    /// Height of one list row.
    pub row_h: f32,
    /// Rows of panel chrome above the first content row. An ext panel paints a
    /// one-row header; a backend that paints more says so here rather than
    /// baking the offset into its own index arithmetic.
    pub header_rows: f32,
}

impl SidebarBodyGeometry {
    fn contains_x(&self, x: f32) -> bool {
        x >= self.bounds.x && x < self.bounds.x + self.bounds.width
    }
}

/// Resolve `pos` (absolute, same units the panel was last painted with) to
/// a flat row index into the plugin panel's tree, or `None` when the point
/// is outside the painted body, on the chrome above it, or past the last
/// row (`TreeViewHit::Empty`).
///
/// Reads `Engine::ext_panel_tree_layout` — the `Backend::tree_layout` cached
/// by whichever paint arm ran this frame — rather than re-deriving row
/// geometry from a uniform `row_h`; see that field's own doc for why
pub fn ext_panel_hit_flat_index(engine: &Engine, pos: quadraui::Point) -> Option<usize> {
    let cached = engine.ext_panel_tree_layout.borrow();
    let (body_rect, layout) = cached.as_ref()?;
    if pos.x < body_rect.x
        || pos.x >= body_rect.x + body_rect.width
        || pos.y < body_rect.y
        || pos.y >= body_rect.y + body_rect.height
    {
        return None;
    }
    match layout.hit_test(pos.x - body_rect.x, pos.y - body_rect.y) {
        quadraui::TreeViewHit::Row(i) | quadraui::TreeViewHit::Chevron(i) => Some(i),
        quadraui::TreeViewHit::Empty => None,
    }
}

/// Apply a press at `pos` to the plugin panel: select the row and perform the
/// select/toggle action a real `Enter` press (or double-click) would.
///
/// Shared by TUI and the cross-backend `App::try_route_sidebar_mouse_event`,
/// so the two backends' plugin-panel click geometry can't drift apart the way
/// their *paint* geometry used to. Both resolve `pos` from [`ext_panel_hit_flat_index`].
///
/// A no-op when `pos` doesn't land on a row or index is past the end of the flat
/// row list. Phase 0 only handles selection + open — single click selects, double
/// opens. Returns whether the click landed on something the panel owns.
pub fn route_board_click(engine: &mut Engine, pos: quadraui::Point, is_double_click: bool) -> bool {
    let hit = engine
        .board_layout
        .borrow()
        .as_ref()
        .map(|layout| layout.hit_test(pos.x, pos.y))
        .unwrap_or(quadraui::BoardHit::Empty);
    match hit {
        quadraui::BoardHit::Card(id) => {
            engine.apply_board_action(quadraui::BoardAction::SelectCard(id.clone()));
            if is_double_click {
                engine.apply_board_action(quadraui::BoardAction::OpenIssue(id));
            }
            true
        }
        quadraui::BoardHit::ColumnHeader(_) | quadraui::BoardHit::Empty => false,
    }
}

/// Resolve a right-click at `pos` (board-native units, same convention as
/// [`route_board_click`]) against the cached [`quadraui::BoardLayout`] to
/// the card it landed on, if any. A column-header or empty-space
/// right-click isn't a context-menu trigger. Callers convert `pos` to the
/// cell units `Engine::open_board_context_menu` expects — mirroring
/// `App::handle_tab_right_click`/`handle_editor_right_click`'s own
/// pixel→cell conversion — and are responsible for actually opening the
/// menu; this function only resolves *which card*, the same "paint caches,
/// click reads" split [`route_board_click`] uses.
pub fn board_right_click_card(engine: &Engine, pos: quadraui::Point) -> Option<quadraui::WidgetId> {
    let layout = engine.board_layout.borrow();
    match layout.as_ref()?.hit_test(pos.x, pos.y) {
        quadraui::BoardHit::Card(id) => Some(id),
        quadraui::BoardHit::ColumnHeader(_) | quadraui::BoardHit::Empty => None,
    }
}

/// A one-line status banner for the Board panel — "no provider configured",
/// "fetching…", or the last fetch error (see [`BoardData::status`]). Shared
/// by both backends so a status message can't drift in wording or style.
pub fn board_status_bar(status: &str, theme: &Theme) -> quadraui::StatusBar {
    quadraui::StatusBar {
        id: quadraui::WidgetId::new("board:status"),
        left_segments: vec![quadraui::StatusBarSegment {
            text: format!("  {status}"),
            fg: theme.status_fg,
            bg: theme.status_bg,
            bold: false,
            action_id: None,
        }],
        right_segments: Vec::new(),
    }
}

pub fn route_ext_panel_click(engine: &mut Engine, pos: quadraui::Point, is_double_click: bool) {
    let Some(flat_idx) = ext_panel_hit_flat_index(engine, pos) else {
        return;
    };
    if flat_idx >= engine.ext_panel_flat_len() {
        return;
    }
    engine.ext_panel_selected = flat_idx;
    if is_double_click {
        engine.handle_ext_panel_double_click();
    } else {
        // Single-click toggles sections/expandable items — suppressed on a
        // double-click so the second `Down` doesn't un-toggle what the
        // first one just toggled.
        engine.handle_ext_panel_key("Return", false, None);
    }
}

/// Update [`Engine::gutter_hover_window`] from the pointer position against the
/// last-painted `ScreenLayout`. Routes through the same [`find_window_at`] +
/// [`window_zone_hit_test`] pair every click-target resolver uses, so a gutter
/// decision can never drift from what a real click would resolve to.
pub fn route_gutter_hover(
    engine: &mut Engine,
    layout: &ScreenLayout,
    x: f64,
    y: f64,
    line_height: f64,
    char_width: f64,
) {
    engine.gutter_hover_window = find_window_at(layout, x, y).and_then(|idx| {
        let rw = &layout.windows[idx];
        let rel_x = x - rw.rect.x;
        let rel_y = y - rw.rect.y;
        let hit = window_zone_hit_test(rw, rel_x, rel_y, line_height, char_width);
        matches!(hit, WindowZone::Gutter { .. }).then_some(rw.window_id)
    });
}

/// Hover feedback for the sidebar — Source Control toolbar buttons and section
/// rows, and plugin ext-panel rows.
///
/// `mouse_on_popup` is the caller's answer to "is the pointer over the hover card
/// itself" — dismissing while on the card prevents reading it.
///
/// Returns `true` when this call actually changed something this function paints
/// synchronously — today that's only `engine.sc_button_hovered` (SC hover
/// highlight); other mutations only arm timers.
pub fn route_sidebar_hover(
    engine: &mut Engine,
    owner: &SidebarOwner,
    x: f32,
    y: f32,
    geometry: SidebarBodyGeometry,
    sidebar_visible: bool,
    mouse_on_popup: bool,
) -> bool {
    let inside = sidebar_visible && geometry.contains_x(x);
    let sc_button_hovered_before = engine.sc_button_hovered;
    match owner {
        SidebarOwner::Git if inside => {
            // Route via the cached `SidebarPanelLayout` — no per-frame
            // arithmetic, and it hit-tests in the same absolute space the
            // shared painter built it in, which is why this works unchanged on
            // both backends.
            let hit = {
                let layout = engine.sc_panel_layout.borrow();
                layout.as_ref().map(|l| l.hit_test(x, y))
            };
            match hit {
                Some(quadraui::SidebarPanelHit::ToolbarButton(_))
                | Some(quadraui::SidebarPanelHit::ToolbarEmpty) => {
                    engine.sc_button_hovered = engine.sc_button_hit(x, y);
                    if !mouse_on_popup {
                        engine.dismiss_panel_hover();
                    }
                }
                Some(quadraui::SidebarPanelHit::Content { y: content_y, .. }) => {
                    engine.sc_button_hovered = None;
                    if let Some((flat_idx, _is_header)) =
                        engine.sc_content_row_to_flat(content_y as usize, true)
                    {
                        engine.panel_hover_mouse_move("source_control", "", flat_idx);
                    } else if !mouse_on_popup {
                        engine.dismiss_panel_hover();
                    }
                }
                _ => {
                    engine.sc_button_hovered = None;
                    if !mouse_on_popup {
                        engine.dismiss_panel_hover();
                    }
                }
            }
        }
        SidebarOwner::ExtPanel(name) if inside => {
            let name = name.clone();
            // Route through the same `Backend::tree_layout`-cached hit-test
            // `route_ext_panel_click` uses, not `SidebarBodyGeometry::
            // content_row`'s uniform-row-height formula: a real multi-section
            // panel pitches header rows and item rows differently on
            // GTK/macOS/Win (`Engine::ext_panel_tree_layout`'s own doc), so a
            // linear formula resolves the wrong row near a section boundary
            //. `ext_panel_hit_flat_index` already returns an absolute
            // flat index (matching `ext_panel_to_tree_view`'s own numbering),
            // so no `ext_panel_scroll_top` offset is added here — adding one
            // would double-count the scroll the cached layout already baked
            // in.
            let flat_idx = ext_panel_hit_flat_index(engine, quadraui::Point { x, y })
                .filter(|&i| i < engine.ext_panel_flat_len());
            match flat_idx {
                Some(flat_idx) => {
                    engine.panel_hover_mouse_move(&name, "", flat_idx);
                }
                // Chrome above the body, past the last row, or a stale cache:
                // nothing to hover.
                None if !mouse_on_popup => engine.dismiss_panel_hover(),
                None => {}
            }
        }
        // Pointer left the panel that owns the hover card (or the sidebar
        // entirely) — drop it, unless the pointer is *on* the card.
        _ => {
            engine.sc_button_hovered = None;
            if engine.panel_hover.is_some() && !mouse_on_popup {
                engine.dismiss_panel_hover();
            }
        }
    }
    engine.sc_button_hovered != sc_button_hovered_before
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── editor_hover_to_quadraui_rich_text adapter tests ─────────────────────

    /// Build a minimal `EditorHoverPopupData` with a known link on line 0 so
    /// we can verify that `editor_hover_to_quadraui_rich_text` maps it into
    /// `RichTextPopup.links` with the correct byte offsets.  These offsets are
    /// exactly what the GTK `link_widths` closure indexes into via
    /// `pango_layout.index_to_pos(start_byte)` — wrong offsets would shift the
    /// hit-rect even if the Pango measurement itself is accurate.
    #[test]
    fn test_editor_hover_to_quadraui_rich_text_link_offsets() {
        let line = "See https://example.com for details";
        // byte offsets of "https://example.com": starts at 4, ends at 23
        let link_start = 4usize;
        let link_end = 23usize;
        assert_eq!(&line[link_start..link_end], "https://example.com");

        let eh = EditorHoverPopupData {
            markdown: line.to_string(),
            line_text: vec![line.to_string()],
            code_highlights: vec![vec![]],
            links: vec![(0, link_start, link_end, "https://example.com".to_string())],
            anchor_line: 0,
            anchor_col: 0,
            scroll_top: 0,
            focused_link: None,
            has_focus: false,
            popup_width: 40,
            frozen_scroll_top: 0,
            frozen_scroll_left: 0,
            selection: None,
        };
        let theme = Theme::onedark();
        let popup = editor_hover_to_quadraui_rich_text(&eh, &theme);

        // Exactly one link.
        assert_eq!(popup.links.len(), 1, "one link expected");
        let link = &popup.links[0];
        // Byte offsets must survive the conversion unchanged.
        assert_eq!(link.line, 0);
        assert_eq!(
            link.start_byte, link_start,
            "start_byte mismatch — GTK index_to_pos would compute wrong x0"
        );
        assert_eq!(
            link.end_byte, link_end,
            "end_byte mismatch — GTK index_to_pos would compute wrong x1"
        );
        assert_eq!(link.url, "https://example.com");
        // line_text[0] must equal the raw line so index_to_pos byte indices are valid.
        assert_eq!(
            popup.line_text.first().map(String::as_str),
            Some(line),
            "line_text must carry the raw text unchanged"
        );
        // Sanity: the byte range must index valid UTF-8 within line_text.
        let raw = &popup.line_text[0];
        assert_eq!(&raw[link.start_byte..link.end_byte], "https://example.com");
    }

    /// Multi-link hover: two URLs on different lines.  Verifies that lines and
    /// link indices stay in sync after the adapter — an off-by-one in `links`
    /// would cause the GTK closure to measure the wrong line or wrong span.
    #[test]
    fn test_editor_hover_to_quadraui_rich_text_multi_link() {
        let line0 = "Docs: https://docs.rs/foo";
        let line1 = "Also see https://crates.io/crates/foo";
        // "https://docs.rs/foo" starts at 6, ends at 25
        // "https://crates.io/crates/foo" starts at 9, ends at 37
        let (s0, e0) = (6, 25);
        let (s1, e1) = (9, 37);
        assert_eq!(&line0[s0..e0], "https://docs.rs/foo");
        assert_eq!(&line1[s1..e1], "https://crates.io/crates/foo");

        let eh = EditorHoverPopupData {
            markdown: format!("{line0}\n{line1}"),
            line_text: vec![line0.to_string(), line1.to_string()],
            code_highlights: vec![vec![], vec![]],
            links: vec![
                (0, s0, e0, "https://docs.rs/foo".to_string()),
                (1, s1, e1, "https://crates.io/crates/foo".to_string()),
            ],
            anchor_line: 0,
            anchor_col: 0,
            scroll_top: 0,
            focused_link: None,
            has_focus: false,
            popup_width: 40,
            frozen_scroll_top: 0,
            frozen_scroll_left: 0,
            selection: None,
        };
        let theme = Theme::onedark();
        let popup = editor_hover_to_quadraui_rich_text(&eh, &theme);

        assert_eq!(popup.links.len(), 2);
        assert_eq!(popup.links[0].line, 0);
        assert_eq!(popup.links[0].start_byte, s0);
        assert_eq!(popup.links[0].end_byte, e0);
        assert_eq!(popup.links[1].line, 1);
        assert_eq!(popup.links[1].start_byte, s1);
        assert_eq!(popup.links[1].end_byte, e1);
        // line_text must be in sync with link offsets.
        assert_eq!(&popup.line_text[0][s0..e0], "https://docs.rs/foo");
        assert_eq!(&popup.line_text[1][s1..e1], "https://crates.io/crates/foo");
    }

    #[test]
    fn test_tab_switcher_to_list_view_dirty_and_scroll() {
        let ts = TabSwitcherPanel {
            items: vec![
                ("main.rs".to_string(), "/src/main.rs".to_string(), false),
                ("lib.rs".to_string(), "/src/lib.rs".to_string(), true),
                (
                    "keys.rs".to_string(),
                    "/src/core/keys.rs".to_string(),
                    false,
                ),
                ("tests.rs".to_string(), "/src/tests.rs".to_string(), false),
                ("todo.md".to_string(), "".to_string(), false),
            ],
            selected_idx: 4,
        };
        let list = tab_switcher_to_quadraui_list_view(&ts, 3);

        // Bordered modal with title overlay.
        assert!(list.bordered);
        assert!(list.title.is_some());
        // 5 items, all present.
        assert_eq!(list.items.len(), 5);
        // Dirty marker appended to filename label (rendered as text,
        // not detail — matches legacy behavior).
        let lib_text: String = list.items[1]
            .text
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(lib_text.contains("●"));
        let main_text: String = list.items[0]
            .text
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(!main_text.contains("●"));
        // Paths appear as detail (right-aligned dimmed in the rasteriser).
        assert!(list.items[0].detail.is_some());
        // Empty path → no detail (avoids rendering a lone trailing space).
        assert!(list.items[4].detail.is_none());
        // Scroll so selected (idx=4) is visible inside max_visible=3:
        // offset = 4 + 1 - 3 = 2.
        assert_eq!(list.scroll_offset, 2);
        assert_eq!(list.selected_idx, 4);
    }
}
