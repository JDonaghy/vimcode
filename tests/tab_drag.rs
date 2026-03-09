mod common;
use common::*;
use vimcode_core::core::window::{DropZone, SplitDirection};

// ── Tab drag: begin and cancel ──────────────────────────────────────────────

#[test]
fn tab_drag_begin_sets_state() {
    let mut e = engine_with("hello\n");
    let gid = e.active_group;
    e.tab_drag_begin(gid, 0);
    assert!(e.tab_drag.is_some());
    assert_eq!(e.tab_drag.as_ref().unwrap().source_group, gid);
    assert_eq!(e.tab_drag.as_ref().unwrap().source_tab_index, 0);
}

#[test]
fn tab_drag_cancel_clears_state() {
    let mut e = engine_with("hello\n");
    let gid = e.active_group;
    e.tab_drag_begin(gid, 0);
    e.tab_drag_cancel();
    assert!(e.tab_drag.is_none());
    assert!(e.tab_drag_mouse.is_none());
    assert_eq!(e.tab_drop_zone, DropZone::None);
}

// ── Tab drag: drop to center of same group (no-op) ─────────────────────────

#[test]
fn tab_drag_drop_center_same_group_is_noop() {
    let mut e = engine_with("hello\n");
    exec(&mut e, "tabnew");
    let gid = e.active_group;
    let tabs_before = e.active_group().tabs.len();
    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::Center(gid));
    assert_eq!(e.active_group().tabs.len(), tabs_before);
    assert!(e.tab_drag.is_none());
}

// ── Tab drag: move tab to another group ─────────────────────────────────────

#[test]
fn move_tab_to_target_group() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    e.buffer_mut().insert(0, "file2\n");
    let src = e.active_group;
    // Create a second group
    e.open_editor_group(SplitDirection::Vertical);
    let dst = e.active_group;
    assert_ne!(src, dst);

    // Move tab 1 from src to dst
    e.tab_drag_begin(src, 1);
    e.tab_drag_drop(DropZone::Center(dst));

    // dst should now have 2 tabs
    assert_eq!(e.editor_groups.get(&dst).unwrap().tabs.len(), 2);
    // src should have 1 tab
    assert_eq!(e.editor_groups.get(&src).unwrap().tabs.len(), 1);
    assert_eq!(e.active_group, dst);
}

// ── Tab drag: move last tab closes source group ─────────────────────────────

#[test]
fn move_last_tab_closes_source_group() {
    let mut e = engine_with("file1\n");
    let src = e.active_group;
    e.open_editor_group(SplitDirection::Vertical);
    let dst = e.active_group;
    // src has 1 tab, dst has 1 tab
    assert!(e.editor_groups.contains_key(&src));

    // Move the only tab from src to dst
    e.tab_drag_begin(src, 0);
    e.tab_drag_drop(DropZone::Center(dst));

    // src should be removed from the layout
    assert!(!e.editor_groups.contains_key(&src));
    // dst should have 2 tabs
    assert_eq!(e.editor_groups.get(&dst).unwrap().tabs.len(), 2);
    assert!(e.group_layout.is_single_group());
}

// ── Tab drag: split right ───────────────────────────────────────────────────

#[test]
fn move_tab_to_new_split_right() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    e.buffer_mut().insert(0, "file2\n");
    let gid = e.active_group;
    let groups_before = e.editor_groups.len();

    e.tab_drag_begin(gid, 1);
    e.tab_drag_drop(DropZone::Split(gid, SplitDirection::Vertical, false));

    // Should have one more group
    assert_eq!(e.editor_groups.len(), groups_before + 1);
    // Source group should have 1 tab left
    assert_eq!(e.editor_groups.get(&gid).unwrap().tabs.len(), 1);
    // Active group is the new split
    assert_ne!(e.active_group, gid);
    assert!(!e.group_layout.is_single_group());
}

#[test]
fn move_tab_to_new_split_left() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    let gid = e.active_group;

    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::Split(gid, SplitDirection::Vertical, true));

    assert_eq!(e.editor_groups.len(), 2);
    assert!(!e.group_layout.is_single_group());
}

#[test]
fn move_tab_to_new_split_top() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    let gid = e.active_group;

    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::Split(gid, SplitDirection::Horizontal, true));

    assert_eq!(e.editor_groups.len(), 2);
}

#[test]
fn move_tab_to_new_split_bottom() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    let gid = e.active_group;

    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::Split(gid, SplitDirection::Horizontal, false));

    assert_eq!(e.editor_groups.len(), 2);
}

// ── Tab drag: split with last tab closes source ─────────────────────────────

#[test]
fn split_with_last_tab_closes_source_and_creates_new() {
    let mut e = engine_with("only\n");
    let gid = e.active_group;
    // Create second group so we can split from first
    e.open_editor_group(SplitDirection::Vertical);
    let other = e.active_group;

    // Now drag the sole tab from 'gid' to split right of 'other'
    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::Split(other, SplitDirection::Vertical, false));

    // gid should be gone (had only 1 tab)
    assert!(!e.editor_groups.contains_key(&gid));
    // Should still have 2 groups (other + new split)
    assert_eq!(e.editor_groups.len(), 2);
}

// ── Tab reorder within group ────────────────────────────────────────────────

#[test]
fn reorder_tab_in_group() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    e.buffer_mut().insert(0, "file2\n");
    exec(&mut e, "tabnew");
    e.buffer_mut().insert(0, "file3\n");
    let gid = e.active_group;
    assert_eq!(e.active_group().tabs.len(), 3);

    // Active tab is 2 (file3). Reorder tab 0 to position 2.
    e.reorder_tab_in_group(gid, 0, 2);
    assert_eq!(e.active_group().active_tab, 2);
}

#[test]
fn reorder_tab_via_drag_drop() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    exec(&mut e, "tabnew");
    let gid = e.active_group;
    let tabs_before = e.active_group().tabs.len();

    e.tab_drag_begin(gid, 0);
    e.tab_drag_drop(DropZone::TabReorder(gid, 2));

    // Same number of tabs, just reordered
    assert_eq!(e.active_group().tabs.len(), tabs_before);
}

// ── Tab drag: cross-group reorder (move to specific index) ──────────────────

#[test]
fn tab_reorder_to_different_group() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    let src = e.active_group;
    e.open_editor_group(SplitDirection::Vertical);
    let dst = e.active_group;
    // dst has 1 tab, src has 2 tabs
    exec(&mut e, "tabnew");
    // dst now has 2 tabs

    // Drag tab 0 from src to position 1 in dst
    e.tab_drag_begin(src, 0);
    e.tab_drag_drop(DropZone::TabReorder(dst, 1));

    assert_eq!(e.editor_groups.get(&dst).unwrap().tabs.len(), 3);
    assert_eq!(e.editor_groups.get(&src).unwrap().tabs.len(), 1);
    // Active tab in dst should be 1 (the inserted position)
    assert_eq!(e.editor_groups.get(&dst).unwrap().active_tab, 1);
}

// ── close_group_by_id ───────────────────────────────────────────────────────

#[test]
fn close_group_by_id_removes_group() {
    let mut e = engine_with("hello\n");
    let gid = e.active_group;
    e.open_editor_group(SplitDirection::Vertical);
    let new_gid = e.active_group;

    e.close_group_by_id(gid);
    assert!(!e.editor_groups.contains_key(&gid));
    assert!(e.editor_groups.contains_key(&new_gid));
    assert!(e.group_layout.is_single_group());
}

#[test]
fn close_group_by_id_on_single_group_is_noop() {
    let mut e = engine_with("hello\n");
    let gid = e.active_group;
    e.close_group_by_id(gid);
    // Should still exist (single group can't be closed)
    assert!(e.editor_groups.contains_key(&gid));
}
