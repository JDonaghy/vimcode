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
                    *first = Box::new(new_first);
                    return Some(self.clone());
                }
                if let Some(new_second) = second.remove(target) {
                    *second = Box::new(new_second);
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
}
