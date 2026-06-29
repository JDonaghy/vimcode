//! `VimcodeTabGroupCtrl` — bridges [`quadraui::TabGroupController`] with the
//! engine's [`GroupId`]-keyed state.
// The controller-building helpers (`new_from_engine`, pane builders, etc.) are
// complete infrastructure but are not yet wired into the ShellApp render path.
// That wiring is the remaining phase of #515 and will eliminate these warnings.
#![allow(dead_code)]
//!
//! The controller is rebuilt from engine state at the start of each render
//! frame (when no tab drag or divider drag is in progress) and preserved
//! across frames during drags so the drag state is retained.
//!
//! # Layout approximation
//!
//! `TabGroupController` builds its split tree via `with_pane` +
//! `add_pane_with_tab`, which always uses `default_split_direction`.  For
//! layouts with 3+ panes and **mixed** split directions, the controller's
//! internal tree may not exactly match the engine's `GroupLayout`.  This is
//! a known limitation.  Single-group and 2-pane layouts are always correct.

use quadraui::{
    Backend, BackendWidget, DropEdge, GroupLayout as QGroupLayout, PaneTab, Rect,
    SplitDirection as QSplitDir, TabGroupController, TabGroupEvent,
};

use crate::core::{
    engine::Engine,
    tab::Tab,
    window::{DropZone, GroupId, GroupLayout, SplitDirection},
};

// ── No-op BackendWidget ──────────────────────────────────────────────────────
//
// `PaneTab::content` must be `Box<dyn BackendWidget>` which requires `Send +
// `static`.  Vimcode's engine is `Rc<RefCell<Engine>>` (not `Send`) so we
// cannot put real content here.  Editor content is drawn separately by the
// caller in the `content_bounds` returned by `TabGroupController::render`.

struct NoOpContent;

impl BackendWidget for NoOpContent {
    fn render(&self, _backend: &mut dyn Backend, _bounds: Rect) {}
}

// ── VimcodeTabGroupCtrl ──────────────────────────────────────────────────────

/// Bridges the engine's `GroupId`-keyed pane state to the pane-index-based
/// `TabGroupController` API.
///
/// Rebuild via [`VimcodeTabGroupCtrl::new_from_engine`] each frame when the
/// controller is not in a drag state.  During tab and divider drags, preserve
/// the existing controller so drag state is not lost.
pub struct VimcodeTabGroupCtrl {
    /// Underlying quadraui controller.
    pub ctrl: TabGroupController,
    /// `pane_gids[pane_idx]` is the engine `GroupId` that pane corresponds to.
    ///
    /// Captured **before** calling `handle_tab_drop` so that the pre-collapse
    /// indices can be used when translating events to engine operations.
    pub pane_gids: Vec<GroupId>,
}

impl VimcodeTabGroupCtrl {
    /// Build a fresh controller that mirrors the engine's current group state.
    ///
    /// Panes are ordered by [`GroupLayout::group_ids`] (left-to-right /
    /// top-to-bottom in-order traversal).  The root split direction is used
    /// as the controller's `default_split_direction`.
    pub fn new_from_engine(engine: &Engine) -> Self {
        let gids = engine.group_layout.group_ids();

        // Pick the root split direction as the controller's default.
        let root_dir = match &engine.group_layout {
            GroupLayout::Leaf(_) => QSplitDir::Horizontal,
            GroupLayout::Split { direction, .. } => vimcode_dir_to_quadraui(*direction),
        };

        let mut pane_gids: Vec<GroupId> = Vec::new();
        let mut ctrl_opt: Option<TabGroupController> = None;

        for &gid in &gids {
            let eg = match engine.editor_groups.get(&gid) {
                Some(g) => g,
                None => continue,
            };
            if eg.tabs.is_empty() {
                continue;
            }

            let active_tab_id = eg
                .tabs
                .get(eg.active_tab)
                .map(|t| format!("t:{}", t.id.0))
                .unwrap_or_default();

            match ctrl_opt {
                None => {
                    let tabs = build_pane_tabs(engine, gid);
                    ctrl_opt = Some(TabGroupController::with_pane(
                        pane_id_of(gid),
                        tabs,
                        active_tab_id,
                        root_dir,
                    ));
                }
                Some(ref mut ctrl) => {
                    // Add only the first tab via add_pane_with_tab (creates the pane).
                    let first_tab = match build_first_pane_tab(engine, gid) {
                        Some(t) => t,
                        None => continue,
                    };
                    let new_pane_idx = ctrl.add_pane_with_tab(pane_id_of(gid), first_tab);
                    // Add remaining tabs to the new pane.
                    for tab in build_pane_tabs_from(engine, gid, 1) {
                        ctrl.add_tab(new_pane_idx, tab);
                    }
                    // Activate the correct tab.
                    ctrl.switch_tab(new_pane_idx, &active_tab_id);
                }
            }
            pane_gids.push(gid);
        }

        // Fallback: empty controller when engine has no groups.
        let ctrl = ctrl_opt.unwrap_or_else(|| {
            TabGroupController::with_pane("g:0", vec![], "", QSplitDir::Horizontal)
        });

        let mut result = Self { ctrl, pane_gids };

        // Focus the engine's active group.
        if let Some(idx) = result.pane_idx_of(engine.active_group) {
            result.ctrl.focus_pane(idx);
        }
        result
    }

    /// Returns the `GroupId` for pane at `pane_idx`, or `None` if out of range.
    pub fn gid_of(&self, pane_idx: usize) -> Option<GroupId> {
        self.pane_gids.get(pane_idx).copied()
    }

    /// Returns the pane index for a given `GroupId`, or `None` if not found.
    pub fn pane_idx_of(&self, gid: GroupId) -> Option<usize> {
        self.pane_gids.iter().position(|&g| g == gid)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Format a pane ID from a `GroupId` (stable across rebuilds).
fn pane_id_of(gid: GroupId) -> String {
    format!("g:{}", gid.0)
}

/// Build all `PaneTab`s for a group (all tabs, 0-indexed).
fn build_pane_tabs(engine: &Engine, gid: GroupId) -> Vec<PaneTab> {
    build_pane_tabs_from(engine, gid, 0)
}

/// Build `PaneTab`s for a group starting at `from_idx`.
fn build_pane_tabs_from(engine: &Engine, gid: GroupId, from_idx: usize) -> Vec<PaneTab> {
    let group = match engine.editor_groups.get(&gid) {
        Some(g) => g,
        None => return vec![],
    };
    group
        .tabs
        .iter()
        .enumerate()
        .skip(from_idx)
        .map(|(i, tab)| {
            let label = tab_label_for(engine, tab, i);
            PaneTab {
                id: format!("t:{}", tab.id.0),
                label,
                closable: true,
                content: Box::new(NoOpContent),
            }
        })
        .collect()
}

/// Build just the FIRST `PaneTab` for a group (used with `add_pane_with_tab`).
fn build_first_pane_tab(engine: &Engine, gid: GroupId) -> Option<PaneTab> {
    build_pane_tabs_from(engine, gid, 0).into_iter().next()
}

/// Compute the display label for a tab (matches `render::build_tab_bar_for_group_by_id`).
fn tab_label_for(engine: &Engine, tab: &Tab, idx: usize) -> String {
    if let Some(win) = engine.windows.get(&tab.active_window) {
        if let Some(state) = engine.buffer_manager.get(win.buffer_id) {
            return format!(" {}: {} ", idx + 1, state.display_name());
        }
    }
    format!(" {}: [No Name] ", idx + 1)
}

// ── Direction conversion ─────────────────────────────────────────────────────

/// Convert vimcode's `SplitDirection` to quadraui's (naming is swapped).
///
/// | vimcode             | quadraui            | visual     |
/// |---------------------|---------------------|------------|
/// | `Vertical`          | `Horizontal`        | left/right |
/// | `Horizontal`        | `Vertical`          | top/bottom |
pub(crate) fn vimcode_dir_to_quadraui(dir: SplitDirection) -> QSplitDir {
    match dir {
        SplitDirection::Vertical => QSplitDir::Horizontal,
        SplitDirection::Horizontal => QSplitDir::Vertical,
    }
}

/// Convert a `DropEdge` from the controller into vimcode's (`SplitDirection`,
/// `new_first`) pair for `GroupLayout::split_at`.
pub(crate) fn drop_edge_to_engine(edge: DropEdge) -> (SplitDirection, bool) {
    // quadraui DropEdge naming:
    //   Left/Right → Horizontal split (side-by-side) → vimcode Vertical
    //   Top/Bottom → Vertical split (stacked)        → vimcode Horizontal
    match edge {
        DropEdge::Left => (SplitDirection::Vertical, true),
        DropEdge::Right => (SplitDirection::Vertical, false),
        DropEdge::Top => (SplitDirection::Horizontal, true),
        DropEdge::Bottom => (SplitDirection::Horizontal, false),
    }
}

// ── Tab ID helpers ───────────────────────────────────────────────────────────

/// Parse a tab id string (`"t:{n}"`) and find its index in the group.
pub(crate) fn tab_idx_from_id(engine: &Engine, gid: GroupId, tab_id: &str) -> Option<usize> {
    use crate::core::tab::TabId;
    let n: usize = tab_id.strip_prefix("t:")?.parse().ok()?;
    let tid = TabId(n);
    engine
        .editor_groups
        .get(&gid)?
        .tabs
        .iter()
        .position(|t| t.id == tid)
}

// ── Layout ratio helpers ─────────────────────────────────────────────────────

/// Sync the `divider_idx`-th split ratio (in-order traversal) from
/// `ctrl_layout` into the corresponding split in `engine_layout`.
///
/// Both trees are walked simultaneously so the indices always match, even
/// when quadraui and engine use different traversal orders for their
/// split-index schemes.
pub(crate) fn sync_split_ratio_at(
    ctrl_layout: &QGroupLayout,
    engine_layout: &mut GroupLayout,
    target: usize,
    counter: &mut usize,
) -> bool {
    match (ctrl_layout, engine_layout) {
        (
            QGroupLayout::Split {
                ratio,
                first: qf,
                second: qs,
                ..
            },
            GroupLayout::Split {
                ratio: er,
                first: ef,
                second: es,
                ..
            },
        ) => {
            // In-order: left subtree first, then this node, then right.
            if sync_split_ratio_at(qf, ef, target, counter) {
                return true;
            }
            let cur = *counter;
            *counter += 1;
            if cur == target {
                *er = *ratio as f64;
                return true;
            }
            sync_split_ratio_at(qs, es, target, counter)
        }
        _ => false,
    }
}

/// Sync ALL split ratios from `ctrl_layout` into `engine_layout` by walking
/// both trees in tandem.  Called after a divider drag completes.
pub(crate) fn sync_all_ratios(ctrl_layout: &QGroupLayout, engine_layout: &mut GroupLayout) {
    if let (
        QGroupLayout::Split {
            ratio,
            first: qf,
            second: qs,
            ..
        },
        GroupLayout::Split {
            ratio: er,
            first: ef,
            second: es,
            ..
        },
    ) = (ctrl_layout, engine_layout)
    {
        *er = *ratio as f64;
        sync_all_ratios(qf, ef);
        sync_all_ratios(qs, es);
    }
}

// ── Event application ────────────────────────────────────────────────────────

/// Convert a batch of `TabGroupEvent`s (from `handle_tab_drop`) into engine
/// mutations.
///
/// `pane_gids` must be the snapshot taken **before** calling `handle_tab_drop`
/// so that pre-collapse pane indices correctly map to `GroupId`s.
pub(crate) fn apply_tab_group_events(
    events: Vec<TabGroupEvent>,
    pane_gids: &[GroupId],
    engine: &mut Engine,
) {
    for event in events {
        apply_one_tab_group_event(event, pane_gids, engine);
    }
}

fn apply_one_tab_group_event(event: TabGroupEvent, pane_gids: &[GroupId], engine: &mut Engine) {
    match event {
        TabGroupEvent::TabActivated { pane_idx, tab_id } => {
            let gid = match pane_gids.get(pane_idx) {
                Some(&g) => g,
                None => return,
            };
            if let Some(tab_idx) = tab_idx_from_id(engine, gid, &tab_id) {
                if let Some(eg) = engine.editor_groups.get_mut(&gid) {
                    eg.active_tab = tab_idx;
                }
                engine.active_group = gid;
            }
        }

        TabGroupEvent::TabClosed { pane_idx, tab_id } => {
            let gid = match pane_gids.get(pane_idx) {
                Some(&g) => g,
                None => return,
            };
            if let Some(tab_idx) = tab_idx_from_id(engine, gid, &tab_id) {
                if let Some(eg) = engine.editor_groups.get_mut(&gid) {
                    if tab_idx < eg.tabs.len() {
                        eg.tabs.remove(tab_idx);
                        if eg.active_tab >= eg.tabs.len() && !eg.tabs.is_empty() {
                            eg.active_tab = eg.tabs.len() - 1;
                        }
                    }
                }
            }
        }

        TabGroupEvent::PaneCollapsed { pane_idx } => {
            let gid = match pane_gids.get(pane_idx) {
                Some(&g) => g,
                None => return,
            };
            // Idempotent: close_group_by_id is a no-op if already removed.
            engine.close_group_by_id(gid);
        }

        TabGroupEvent::PaneFocused { pane_idx } => {
            if let Some(&gid) = pane_gids.get(pane_idx) {
                engine.active_group = gid;
            }
        }

        TabGroupEvent::TabReordered {
            pane_idx,
            tab_id,
            to_idx,
            ..
        } => {
            let gid = match pane_gids.get(pane_idx) {
                Some(&g) => g,
                None => return,
            };
            if let Some(from_idx) = tab_idx_from_id(engine, gid, &tab_id) {
                engine.reorder_tab_in_group(gid, from_idx, to_idx);
            }
        }

        TabGroupEvent::TabMovedToPane {
            from_pane_idx,
            to_pane_idx,
            tab_id,
            insert_idx,
        } => {
            let from_gid = match pane_gids.get(from_pane_idx) {
                Some(&g) => g,
                None => return,
            };
            let to_gid = match pane_gids.get(to_pane_idx) {
                Some(&g) => g,
                None => return,
            };
            if let Some(tab_idx) = tab_idx_from_id(engine, from_gid, &tab_id) {
                // move_tab_to_target_group_at handles collapse of empty source.
                engine.move_tab_to_target_group_at(from_gid, tab_idx, to_gid, insert_idx);
            }
        }

        TabGroupEvent::TabSplitToNewPane {
            from_pane_idx,
            tab_id,
            target_pane_idx,
            edge,
            ..
        } => {
            let from_gid = match pane_gids.get(from_pane_idx) {
                Some(&g) => g,
                None => return,
            };
            let target_gid = match pane_gids.get(target_pane_idx) {
                Some(&g) => g,
                None => return,
            };
            if let Some(tab_idx) = tab_idx_from_id(engine, from_gid, &tab_id) {
                let (direction, new_first) = drop_edge_to_engine(edge);
                // move_tab_to_new_split handles collapse of empty source.
                engine.move_tab_to_new_split(from_gid, tab_idx, target_gid, direction, new_first);
            }
        }

        // DividerResized: ratio already synced during drag; nothing extra needed.
        TabGroupEvent::DividerResized { .. } => {}

        // NewTabRequested: app should open a new tab; out of scope for drag events.
        TabGroupEvent::NewTabRequested { .. } => {}

        // PaneAdded: the pane is created inside the controller; engine state is
        // updated by the accompanying TabMovedToPane / TabSplitToNewPane event.
        TabGroupEvent::PaneAdded { .. } => {}
    }
}

// ── Drop zone adapter (used by TUI path) ─────────────────────────────────────

/// Convert a vimcode `DropZone` into engine mutations for a tab drop.
///
/// `source_gid` and `source_tab_idx` identify the tab being dragged
/// (captured when the drag started).
pub(crate) fn apply_drop_zone(
    engine: &mut Engine,
    source_gid: GroupId,
    source_tab_idx: usize,
    zone: DropZone,
) {
    match zone {
        DropZone::Center(target) => {
            if target != source_gid {
                engine.move_tab_to_target_group(source_gid, source_tab_idx, target);
            }
        }
        DropZone::Split(target, direction, new_first) => {
            engine.move_tab_to_new_split(source_gid, source_tab_idx, target, direction, new_first);
        }
        DropZone::TabReorder(group_id, to_idx) => {
            if group_id == source_gid {
                engine.reorder_tab_in_group(group_id, source_tab_idx, to_idx);
            } else {
                engine.move_tab_to_target_group_at(source_gid, source_tab_idx, group_id, to_idx);
            }
        }
        DropZone::None => {}
    }
}
