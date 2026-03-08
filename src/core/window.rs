use super::buffer::BufferId;
use super::view::View;

/// Unique identifier for a window within the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub usize);

/// Direction of a window split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal, // split top/bottom
    Vertical,   // split left/right
}

/// A window is a viewport into a buffer.
/// Multiple windows can display the same buffer with independent cursors/scroll.
#[derive(Debug, Clone)]
pub struct Window {
    #[allow(dead_code)]
    pub id: WindowId,
    pub buffer_id: BufferId,
    pub view: View,
}

impl Window {
    pub fn new(id: WindowId, buffer_id: BufferId) -> Self {
        Self {
            id,
            buffer_id,
            view: View::new(),
        }
    }
}

/// Recursive tree structure for window layout.
/// Allows arbitrary nesting of horizontal and vertical splits.
#[derive(Debug, Clone)]
pub enum WindowLayout {
    /// A single window (leaf node).
    Leaf(WindowId),
    /// A split containing two sub-layouts.
    Split {
        direction: SplitDirection,
        /// Ratio of space given to the first child (0.0..1.0).
        ratio: f64,
        first: Box<WindowLayout>,
        second: Box<WindowLayout>,
    },
}

impl WindowLayout {
    /// Create a new leaf layout with a single window.
    pub fn leaf(window_id: WindowId) -> Self {
        WindowLayout::Leaf(window_id)
    }

    /// Split a window in the given direction.
    /// Returns the new layout and the ID of the new window slot (caller provides the new WindowId).
    pub fn split_at(
        &mut self,
        target: WindowId,
        direction: SplitDirection,
        new_window_id: WindowId,
        new_first: bool, // if true, new window is first (left/top)
    ) -> bool {
        match self {
            WindowLayout::Leaf(id) => {
                if *id == target {
                    let old_leaf = Box::new(WindowLayout::Leaf(*id));
                    let new_leaf = Box::new(WindowLayout::Leaf(new_window_id));
                    let (first, second) = if new_first {
                        (new_leaf, old_leaf)
                    } else {
                        (old_leaf, new_leaf)
                    };
                    *self = WindowLayout::Split {
                        direction,
                        ratio: 0.5,
                        first,
                        second,
                    };
                    true
                } else {
                    false
                }
            }
            WindowLayout::Split { first, second, .. } => {
                first.split_at(target, direction, new_window_id, new_first)
                    || second.split_at(target, direction, new_window_id, new_first)
            }
        }
    }

    /// Remove a window from the layout.
    /// Returns Some(remaining_layout) if successful, None if window not found.
    /// If removing the window leaves an empty split, the sibling is promoted.
    pub fn remove(&mut self, target: WindowId) -> Option<WindowLayout> {
        match self {
            WindowLayout::Leaf(id) => {
                if *id == target {
                    None // Can't remove the only window at this level
                } else {
                    Some(self.clone())
                }
            }
            WindowLayout::Split { first, second, .. } => {
                // Check if target is directly in first or second
                if let WindowLayout::Leaf(id) = first.as_ref() {
                    if *id == target {
                        return Some(second.as_ref().clone());
                    }
                }
                if let WindowLayout::Leaf(id) = second.as_ref() {
                    if *id == target {
                        return Some(first.as_ref().clone());
                    }
                }

                // Recursively try to remove from children
                if let Some(new_first) = first.remove(target) {
                    **first = new_first;
                    return Some(self.clone());
                }
                if let Some(new_second) = second.remove(target) {
                    **second = new_second;
                    return Some(self.clone());
                }

                Some(self.clone())
            }
        }
    }

    /// Get all window IDs in this layout (in order).
    pub fn window_ids(&self) -> Vec<WindowId> {
        match self {
            WindowLayout::Leaf(id) => vec![*id],
            WindowLayout::Split { first, second, .. } => {
                let mut ids = first.window_ids();
                ids.extend(second.window_ids());
                ids
            }
        }
    }

    /// Find the next window ID in the layout (for Ctrl-W w cycling).
    pub fn next_window(&self, current: WindowId) -> Option<WindowId> {
        let ids = self.window_ids();
        if ids.is_empty() {
            return None;
        }
        let current_idx = ids.iter().position(|&id| id == current)?;
        let next_idx = (current_idx + 1) % ids.len();
        Some(ids[next_idx])
    }

    /// Find the previous window ID in the layout.
    pub fn prev_window(&self, current: WindowId) -> Option<WindowId> {
        let ids = self.window_ids();
        if ids.is_empty() {
            return None;
        }
        let current_idx = ids.iter().position(|&id| id == current)?;
        let prev_idx = if current_idx == 0 {
            ids.len() - 1
        } else {
            current_idx - 1
        };
        Some(ids[prev_idx])
    }

    /// Check if layout contains only one window.
    pub fn is_single_window(&self) -> bool {
        matches!(self, WindowLayout::Leaf(_))
    }

    /// Get the single window ID if this is a leaf.
    #[allow(dead_code)]
    pub fn single_window_id(&self) -> Option<WindowId> {
        if let WindowLayout::Leaf(id) = self {
            Some(*id)
        } else {
            None
        }
    }
}

/// Represents a rectangular region for rendering.
#[derive(Debug, Clone, Copy)]
pub struct WindowRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WindowRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

impl WindowLayout {
    /// Calculate the pixel rectangles for each window in the layout.
    pub fn calculate_rects(&self, bounds: WindowRect) -> Vec<(WindowId, WindowRect)> {
        match self {
            WindowLayout::Leaf(id) => vec![(*id, bounds)],
            WindowLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_bounds, second_bounds) = match direction {
                    SplitDirection::Horizontal => {
                        let first_height = bounds.height * ratio;
                        let second_height = bounds.height - first_height;
                        (
                            WindowRect::new(bounds.x, bounds.y, bounds.width, first_height),
                            WindowRect::new(
                                bounds.x,
                                bounds.y + first_height,
                                bounds.width,
                                second_height,
                            ),
                        )
                    }
                    SplitDirection::Vertical => {
                        let first_width = bounds.width * ratio;
                        let second_width = bounds.width - first_width;
                        (
                            WindowRect::new(bounds.x, bounds.y, first_width, bounds.height),
                            WindowRect::new(
                                bounds.x + first_width,
                                bounds.y,
                                second_width,
                                bounds.height,
                            ),
                        )
                    }
                };

                let mut rects = first.calculate_rects(first_bounds);
                rects.extend(second.calculate_rects(second_bounds));
                rects
            }
        }
    }
}

// ===========================================================================
// GroupLayout — recursive tree structure for editor group splits
// ===========================================================================

/// Unique identifier for an editor group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub usize);

/// Where a dragged tab should be dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    /// Move tab into an existing group (merge).
    Center(GroupId),
    /// Create a new split adjacent to `GroupId`.
    /// `bool` = new_first (true → new group goes left/top).
    Split(GroupId, SplitDirection, bool),
    /// Reorder within a group: insert before the given tab index.
    TabReorder(GroupId, usize),
    /// No valid drop target.
    None,
}

/// Description of a single split divider in the group layout tree.
/// Used by backends for hit-testing and drawing divider lines.
#[derive(Debug, Clone)]
pub struct GroupDivider {
    /// Pre-order index of the Split node in the tree (for `set_ratio_at_index`).
    pub split_index: usize,
    /// Direction of this split.
    pub direction: SplitDirection,
    /// Position along the split axis (x for Vertical, y for Horizontal).
    pub position: f64,
    /// Start of parent rect along the split axis.
    pub axis_start: f64,
    /// Size of parent rect along the split axis.
    pub axis_size: f64,
    /// Start of divider line along the cross axis.
    pub cross_start: f64,
    /// Length of divider line along the cross axis.
    pub cross_size: f64,
}

/// Recursive tree structure for editor group layout.
/// Mirrors `WindowLayout` but for editor groups instead of windows.
#[derive(Debug, Clone)]
pub enum GroupLayout {
    /// A single editor group (leaf node).
    Leaf(GroupId),
    /// A split containing two sub-layouts.
    Split {
        direction: SplitDirection,
        ratio: f64,
        first: Box<GroupLayout>,
        second: Box<GroupLayout>,
    },
}

impl GroupLayout {
    /// Create a new leaf layout with a single group.
    pub fn leaf(id: GroupId) -> Self {
        GroupLayout::Leaf(id)
    }

    /// Split a group leaf in the given direction.
    /// `new_first`: if true, new group is first (left/top).
    pub fn split_at(
        &mut self,
        target: GroupId,
        direction: SplitDirection,
        new_id: GroupId,
        new_first: bool,
    ) -> bool {
        match self {
            GroupLayout::Leaf(id) => {
                if *id == target {
                    let old_leaf = Box::new(GroupLayout::Leaf(*id));
                    let new_leaf = Box::new(GroupLayout::Leaf(new_id));
                    let (first, second) = if new_first {
                        (new_leaf, old_leaf)
                    } else {
                        (old_leaf, new_leaf)
                    };
                    *self = GroupLayout::Split {
                        direction,
                        ratio: 0.5,
                        first,
                        second,
                    };
                    true
                } else {
                    false
                }
            }
            GroupLayout::Split { first, second, .. } => {
                first.split_at(target, direction, new_id, new_first)
                    || second.split_at(target, direction, new_id, new_first)
            }
        }
    }

    /// Remove a group from the layout, promoting its sibling.
    /// Returns `true` if the target was found and removed; mutates in-place.
    /// Returns `false` if target is the only leaf (can't remove root) or not found.
    pub fn remove(&mut self, target: GroupId) -> bool {
        match self {
            GroupLayout::Leaf(_) => false,
            GroupLayout::Split { first, second, .. } => {
                // If first child is the target leaf, promote second.
                if let GroupLayout::Leaf(id) = first.as_ref() {
                    if *id == target {
                        *self = second.as_ref().clone();
                        return true;
                    }
                }
                // If second child is the target leaf, promote first.
                if let GroupLayout::Leaf(id) = second.as_ref() {
                    if *id == target {
                        *self = first.as_ref().clone();
                        return true;
                    }
                }
                // Recurse into children.
                first.remove(target) || second.remove(target)
            }
        }
    }

    /// Get all group IDs in in-order (left-to-right / top-to-bottom) traversal.
    pub fn group_ids(&self) -> Vec<GroupId> {
        match self {
            GroupLayout::Leaf(id) => vec![*id],
            GroupLayout::Split { first, second, .. } => {
                let mut ids = first.group_ids();
                ids.extend(second.group_ids());
                ids
            }
        }
    }

    /// Find the next group ID (wrapping).
    pub fn next_group(&self, current: GroupId) -> Option<GroupId> {
        let ids = self.group_ids();
        if ids.is_empty() {
            return None;
        }
        let current_idx = ids.iter().position(|&id| id == current)?;
        let next_idx = (current_idx + 1) % ids.len();
        Some(ids[next_idx])
    }

    /// Find the previous group ID (wrapping).
    #[allow(dead_code)]
    pub fn prev_group(&self, current: GroupId) -> Option<GroupId> {
        let ids = self.group_ids();
        if ids.is_empty() {
            return None;
        }
        let current_idx = ids.iter().position(|&id| id == current)?;
        let prev_idx = if current_idx == 0 {
            ids.len() - 1
        } else {
            current_idx - 1
        };
        Some(ids[prev_idx])
    }

    /// True if this is a single group (no splits).
    pub fn is_single_group(&self) -> bool {
        matches!(self, GroupLayout::Leaf(_))
    }

    /// Count the number of leaf groups.
    pub fn leaf_count(&self) -> usize {
        match self {
            GroupLayout::Leaf(_) => 1,
            GroupLayout::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    /// Get the Nth leaf group (0-indexed, in-order). For Ctrl+1/2/3/… keybindings.
    pub fn nth_leaf(&self, n: usize) -> Option<GroupId> {
        let ids = self.group_ids();
        ids.get(n).copied()
    }

    /// Calculate the pixel rectangles for each group in the layout.
    /// Each leaf gets `y += tab_bar_height, height -= tab_bar_height` to reserve
    /// space for the tab bar drawn at the top of each group.
    pub fn calculate_group_rects(
        &self,
        bounds: WindowRect,
        tab_bar_height: f64,
    ) -> Vec<(GroupId, WindowRect)> {
        match self {
            GroupLayout::Leaf(id) => {
                vec![(
                    *id,
                    WindowRect::new(
                        bounds.x,
                        bounds.y + tab_bar_height,
                        bounds.width,
                        (bounds.height - tab_bar_height).max(0.0),
                    ),
                )]
            }
            GroupLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_bounds, second_bounds) = match direction {
                    SplitDirection::Horizontal => {
                        let first_h = bounds.height * ratio;
                        let second_h = bounds.height - first_h;
                        (
                            WindowRect::new(bounds.x, bounds.y, bounds.width, first_h),
                            WindowRect::new(bounds.x, bounds.y + first_h, bounds.width, second_h),
                        )
                    }
                    SplitDirection::Vertical => {
                        let first_w = bounds.width * ratio;
                        let second_w = bounds.width - first_w;
                        (
                            WindowRect::new(bounds.x, bounds.y, first_w, bounds.height),
                            WindowRect::new(bounds.x + first_w, bounds.y, second_w, bounds.height),
                        )
                    }
                };
                let mut rects = first.calculate_group_rects(first_bounds, tab_bar_height);
                rects.extend(second.calculate_group_rects(second_bounds, tab_bar_height));
                rects
            }
        }
    }

    /// Collect all split dividers with pre-order `split_index`.
    pub fn dividers(&self, bounds: WindowRect, counter: &mut usize) -> Vec<GroupDivider> {
        match self {
            GroupLayout::Leaf(_) => vec![],
            GroupLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let idx = *counter;
                *counter += 1;
                let divider = match direction {
                    SplitDirection::Vertical => {
                        let pos = bounds.x + bounds.width * ratio;
                        GroupDivider {
                            split_index: idx,
                            direction: *direction,
                            position: pos,
                            axis_start: bounds.x,
                            axis_size: bounds.width,
                            cross_start: bounds.y,
                            cross_size: bounds.height,
                        }
                    }
                    SplitDirection::Horizontal => {
                        let pos = bounds.y + bounds.height * ratio;
                        GroupDivider {
                            split_index: idx,
                            direction: *direction,
                            position: pos,
                            axis_start: bounds.y,
                            axis_size: bounds.height,
                            cross_start: bounds.x,
                            cross_size: bounds.width,
                        }
                    }
                };
                let (first_bounds, second_bounds) = match direction {
                    SplitDirection::Horizontal => {
                        let first_h = bounds.height * ratio;
                        let second_h = bounds.height - first_h;
                        (
                            WindowRect::new(bounds.x, bounds.y, bounds.width, first_h),
                            WindowRect::new(bounds.x, bounds.y + first_h, bounds.width, second_h),
                        )
                    }
                    SplitDirection::Vertical => {
                        let first_w = bounds.width * ratio;
                        let second_w = bounds.width - first_w;
                        (
                            WindowRect::new(bounds.x, bounds.y, first_w, bounds.height),
                            WindowRect::new(bounds.x + first_w, bounds.y, second_w, bounds.height),
                        )
                    }
                };
                let mut divs = vec![divider];
                divs.extend(first.dividers(first_bounds, counter));
                divs.extend(second.dividers(second_bounds, counter));
                divs
            }
        }
    }

    /// Find the Nth split node in pre-order and set its ratio (clamped to 0.1..0.9).
    pub fn set_ratio_at_index(&mut self, split_index: usize, ratio: f64) -> bool {
        self.set_ratio_at_index_impl(split_index, ratio, &mut 0)
    }

    fn set_ratio_at_index_impl(&mut self, target: usize, ratio: f64, counter: &mut usize) -> bool {
        match self {
            GroupLayout::Leaf(_) => false,
            GroupLayout::Split {
                ratio: r,
                first,
                second,
                ..
            } => {
                let idx = *counter;
                *counter += 1;
                if idx == target {
                    *r = ratio.clamp(0.1, 0.9);
                    return true;
                }
                first.set_ratio_at_index_impl(target, ratio, counter)
                    || second.set_ratio_at_index_impl(target, ratio, counter)
            }
        }
    }

    /// Find the Nth split node in pre-order and adjust its ratio by delta.
    pub fn adjust_ratio_at_index(&mut self, split_index: usize, delta: f64) {
        self.adjust_ratio_at_index_impl(split_index, delta, &mut 0);
    }

    fn adjust_ratio_at_index_impl(
        &mut self,
        target: usize,
        delta: f64,
        counter: &mut usize,
    ) -> bool {
        match self {
            GroupLayout::Leaf(_) => false,
            GroupLayout::Split {
                ratio,
                first,
                second,
                ..
            } => {
                let idx = *counter;
                *counter += 1;
                if idx == target {
                    *ratio = (*ratio + delta).clamp(0.1, 0.9);
                    return true;
                }
                first.adjust_ratio_at_index_impl(target, delta, counter)
                    || second.adjust_ratio_at_index_impl(target, delta, counter)
            }
        }
    }

    /// Set all split ratios in the tree to the given value (for equalize).
    pub fn set_all_ratios(&mut self, ratio: f64) {
        match self {
            GroupLayout::Leaf(_) => {}
            GroupLayout::Split {
                ratio: r,
                first,
                second,
                ..
            } => {
                *r = ratio.clamp(0.1, 0.9);
                first.set_all_ratios(ratio);
                second.set_all_ratios(ratio);
            }
        }
    }

    /// Find the parent split of a leaf. Returns `(split_index, direction, is_first_child)`.
    pub fn parent_split_of(&self, target: GroupId) -> Option<(usize, SplitDirection, bool)> {
        self.parent_split_of_impl(target, &mut 0)
    }

    fn parent_split_of_impl(
        &self,
        target: GroupId,
        counter: &mut usize,
    ) -> Option<(usize, SplitDirection, bool)> {
        match self {
            GroupLayout::Leaf(_) => None,
            GroupLayout::Split {
                direction,
                first,
                second,
                ..
            } => {
                let idx = *counter;
                *counter += 1;
                // Check if target is a direct child
                if let GroupLayout::Leaf(id) = first.as_ref() {
                    if *id == target {
                        return Some((idx, *direction, true));
                    }
                }
                if let GroupLayout::Leaf(id) = second.as_ref() {
                    if *id == target {
                        return Some((idx, *direction, false));
                    }
                }
                // Recurse
                if let Some(result) = first.parent_split_of_impl(target, counter) {
                    return Some(result);
                }
                second.parent_split_of_impl(target, counter)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_layout_single() {
        let layout = WindowLayout::leaf(WindowId(1));
        assert!(layout.is_single_window());
        assert_eq!(layout.window_ids(), vec![WindowId(1)]);
    }

    #[test]
    fn test_window_layout_split() {
        let mut layout = WindowLayout::leaf(WindowId(1));
        layout.split_at(WindowId(1), SplitDirection::Vertical, WindowId(2), false);

        assert!(!layout.is_single_window());
        assert_eq!(layout.window_ids(), vec![WindowId(1), WindowId(2)]);
    }

    #[test]
    fn test_window_layout_next_prev() {
        let mut layout = WindowLayout::leaf(WindowId(1));
        layout.split_at(WindowId(1), SplitDirection::Vertical, WindowId(2), false);

        assert_eq!(layout.next_window(WindowId(1)), Some(WindowId(2)));
        assert_eq!(layout.next_window(WindowId(2)), Some(WindowId(1)));
        assert_eq!(layout.prev_window(WindowId(1)), Some(WindowId(2)));
        assert_eq!(layout.prev_window(WindowId(2)), Some(WindowId(1)));
    }

    #[test]
    fn test_window_layout_remove() {
        let mut layout = WindowLayout::leaf(WindowId(1));
        layout.split_at(WindowId(1), SplitDirection::Vertical, WindowId(2), false);

        let new_layout = layout.remove(WindowId(2)).unwrap();
        assert!(new_layout.is_single_window());
        assert_eq!(new_layout.single_window_id(), Some(WindowId(1)));
    }

    #[test]
    fn test_calculate_rects_single() {
        let layout = WindowLayout::leaf(WindowId(1));
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let rects = layout.calculate_rects(bounds);

        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, WindowId(1));
        assert!((rects[0].1.width - 800.0).abs() < 0.001);
        assert!((rects[0].1.height - 600.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_rects_vsplit() {
        let mut layout = WindowLayout::leaf(WindowId(1));
        layout.split_at(WindowId(1), SplitDirection::Vertical, WindowId(2), false);

        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let rects = layout.calculate_rects(bounds);

        assert_eq!(rects.len(), 2);
        // First window should be left half
        assert!((rects[0].1.width - 400.0).abs() < 0.001);
        assert!((rects[0].1.x - 0.0).abs() < 0.001);
        // Second window should be right half
        assert!((rects[1].1.width - 400.0).abs() < 0.001);
        assert!((rects[1].1.x - 400.0).abs() < 0.001);
    }

    // ── GroupLayout tests ──────────────────────────────────────────────────

    #[test]
    fn test_group_layout_single() {
        let layout = GroupLayout::leaf(GroupId(0));
        assert!(layout.is_single_group());
        assert_eq!(layout.group_ids(), vec![GroupId(0)]);
        assert_eq!(layout.leaf_count(), 1);
        assert_eq!(layout.nth_leaf(0), Some(GroupId(0)));
        assert_eq!(layout.nth_leaf(1), None);
    }

    #[test]
    fn test_group_layout_split() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        assert!(layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false));
        assert!(!layout.is_single_group());
        assert_eq!(layout.group_ids(), vec![GroupId(0), GroupId(1)]);
        assert_eq!(layout.leaf_count(), 2);
    }

    #[test]
    fn test_group_layout_nested_split() {
        // Start: Leaf(0)
        // Split 0 → Split(V, Leaf(0), Leaf(1))
        // Split 0 again → Split(V, Split(H, Leaf(0), Leaf(2)), Leaf(1))
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        layout.split_at(GroupId(0), SplitDirection::Horizontal, GroupId(2), false);
        assert_eq!(layout.group_ids(), vec![GroupId(0), GroupId(2), GroupId(1)]);
        assert_eq!(layout.leaf_count(), 3);
        assert_eq!(layout.nth_leaf(0), Some(GroupId(0)));
        assert_eq!(layout.nth_leaf(1), Some(GroupId(2)));
        assert_eq!(layout.nth_leaf(2), Some(GroupId(1)));
    }

    #[test]
    fn test_group_layout_next_prev_three() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        layout.split_at(GroupId(0), SplitDirection::Horizontal, GroupId(2), false);
        // Order: [0, 2, 1]
        assert_eq!(layout.next_group(GroupId(0)), Some(GroupId(2)));
        assert_eq!(layout.next_group(GroupId(2)), Some(GroupId(1)));
        assert_eq!(layout.next_group(GroupId(1)), Some(GroupId(0)));
        assert_eq!(layout.prev_group(GroupId(0)), Some(GroupId(1)));
        assert_eq!(layout.prev_group(GroupId(1)), Some(GroupId(2)));
        assert_eq!(layout.prev_group(GroupId(2)), Some(GroupId(0)));
    }

    #[test]
    fn test_group_layout_remove_nested() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        layout.split_at(GroupId(0), SplitDirection::Horizontal, GroupId(2), false);
        // Remove GroupId(2) — its sibling GroupId(0) should be promoted
        assert!(layout.remove(GroupId(2)));
        assert_eq!(layout.group_ids(), vec![GroupId(0), GroupId(1)]);
        assert_eq!(layout.leaf_count(), 2);
    }

    #[test]
    fn test_group_layout_calculate_rects() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let rects = layout.calculate_group_rects(bounds, 30.0);
        assert_eq!(rects.len(), 2);
        // Group 0: left half, y offset by tab_bar_height
        assert_eq!(rects[0].0, GroupId(0));
        assert!((rects[0].1.x - 0.0).abs() < 0.001);
        assert!((rects[0].1.y - 30.0).abs() < 0.001);
        assert!((rects[0].1.width - 400.0).abs() < 0.001);
        assert!((rects[0].1.height - 570.0).abs() < 0.001);
        // Group 1: right half
        assert_eq!(rects[1].0, GroupId(1));
        assert!((rects[1].1.x - 400.0).abs() < 0.001);
        assert!((rects[1].1.y - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_group_layout_dividers() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        layout.split_at(GroupId(0), SplitDirection::Horizontal, GroupId(2), false);
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let dividers = layout.dividers(bounds, &mut 0);
        // Two splits → two dividers
        assert_eq!(dividers.len(), 2);
        // First divider (pre-order 0) is the root vertical split
        assert_eq!(dividers[0].split_index, 0);
        assert_eq!(dividers[0].direction, SplitDirection::Vertical);
        assert!((dividers[0].position - 400.0).abs() < 0.001);
        // Second divider (pre-order 1) is the nested horizontal split
        assert_eq!(dividers[1].split_index, 1);
        assert_eq!(dividers[1].direction, SplitDirection::Horizontal);
        assert!((dividers[1].position - 300.0).abs() < 0.001); // 600*0.5
    }

    #[test]
    fn test_group_layout_set_ratio() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        assert!(layout.set_ratio_at_index(0, 0.7));
        let bounds = WindowRect::new(0.0, 0.0, 1000.0, 600.0);
        let rects = layout.calculate_group_rects(bounds, 0.0);
        assert!((rects[0].1.width - 700.0).abs() < 0.001);
        assert!((rects[1].1.width - 300.0).abs() < 0.001);
        // Clamping
        assert!(layout.set_ratio_at_index(0, 0.05));
        let rects = layout.calculate_group_rects(bounds, 0.0);
        assert!((rects[0].1.width - 100.0).abs() < 0.001); // clamped to 0.1
    }

    #[test]
    fn test_group_layout_nth_leaf() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        layout.split_at(GroupId(1), SplitDirection::Horizontal, GroupId(2), false);
        // Order: [0, 1, 2]
        assert_eq!(layout.nth_leaf(0), Some(GroupId(0)));
        assert_eq!(layout.nth_leaf(1), Some(GroupId(1)));
        assert_eq!(layout.nth_leaf(2), Some(GroupId(2)));
        assert_eq!(layout.nth_leaf(3), None);
    }

    #[test]
    fn test_group_layout_parent_split_of() {
        let mut layout = GroupLayout::leaf(GroupId(0));
        layout.split_at(GroupId(0), SplitDirection::Vertical, GroupId(1), false);
        layout.split_at(GroupId(0), SplitDirection::Horizontal, GroupId(2), false);
        // Tree: Split(V, Split(H, 0, 2), 1)
        // Parent of 0 is split_index=1 (H), first_child=true
        let p0 = layout.parent_split_of(GroupId(0)).unwrap();
        assert_eq!(p0.0, 1); // split_index
        assert_eq!(p0.1, SplitDirection::Horizontal);
        assert!(p0.2); // is_first_child
                       // Parent of 2 is split_index=1 (H), first_child=false
        let p2 = layout.parent_split_of(GroupId(2)).unwrap();
        assert_eq!(p2.0, 1);
        assert!(!p2.2);
        // Parent of 1 is split_index=0 (V), first_child=false
        let p1 = layout.parent_split_of(GroupId(1)).unwrap();
        assert_eq!(p1.0, 0);
        assert_eq!(p1.1, SplitDirection::Vertical);
        assert!(!p1.2);
    }
}
