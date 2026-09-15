mod common;
use common::*;
use vimcode_core::core::window::SplitDirection;

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
    e.move_tab_to_target_group(src, 1, dst);

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
    e.move_tab_to_target_group(src, 0, dst);

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

    e.move_tab_to_new_split(gid, 1, gid, SplitDirection::Vertical, false);

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

    e.move_tab_to_new_split(gid, 0, gid, SplitDirection::Vertical, true);

    assert_eq!(e.editor_groups.len(), 2);
    assert!(!e.group_layout.is_single_group());
}

#[test]
fn move_tab_to_new_split_top() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    let gid = e.active_group;

    e.move_tab_to_new_split(gid, 0, gid, SplitDirection::Horizontal, true);

    assert_eq!(e.editor_groups.len(), 2);
}

#[test]
fn move_tab_to_new_split_bottom() {
    let mut e = engine_with("file1\n");
    exec(&mut e, "tabnew");
    let gid = e.active_group;

    e.move_tab_to_new_split(gid, 0, gid, SplitDirection::Horizontal, false);

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

    // Drag the sole tab from 'gid' to split right of 'other'
    e.move_tab_to_new_split(gid, 0, other, SplitDirection::Vertical, false);

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

    e.reorder_tab_in_group(gid, 0, 2);

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

    // Move tab 0 from src to position 1 in dst
    e.move_tab_to_target_group_at(src, 0, dst, 1);

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
