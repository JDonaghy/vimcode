use super::window::{WindowId, WindowLayout};

/// Unique identifier for a tab within the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub usize);

/// A tab page contains a window layout and tracks the active window.
#[derive(Debug, Clone)]
pub struct Tab {
    #[allow(dead_code)]
    pub id: TabId,
    /// The layout tree of windows in this tab.
    pub layout: WindowLayout,
    /// The currently focused window in this tab.
    pub active_window: WindowId,
}

impl Tab {
    pub fn new(id: TabId, initial_window: WindowId) -> Self {
        Self {
            id,
            layout: WindowLayout::leaf(initial_window),
            active_window: initial_window,
        }
    }

    /// Get all window IDs in this tab.
    pub fn window_ids(&self) -> Vec<WindowId> {
        self.layout.window_ids()
    }

    /// Check if this tab contains a specific window.
    #[allow(dead_code)]
    pub fn contains_window(&self, window_id: WindowId) -> bool {
        self.layout.window_ids().contains(&window_id)
    }

    /// Cycle to the next window in this tab.
    pub fn cycle_next_window(&mut self) {
        if let Some(next) = self.layout.next_window(self.active_window) {
            self.active_window = next;
        }
    }

    /// Cycle to the previous window in this tab.
    pub fn cycle_prev_window(&mut self) {
        if let Some(prev) = self.layout.prev_window(self.active_window) {
            self.active_window = prev;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::window::SplitDirection;

    #[test]
    fn test_tab_new() {
        let tab = Tab::new(TabId(1), WindowId(1));
        assert_eq!(tab.id, TabId(1));
        assert_eq!(tab.active_window, WindowId(1));
        assert_eq!(tab.window_ids(), vec![WindowId(1)]);
    }

    #[test]
    fn test_tab_cycle_windows() {
        let mut tab = Tab::new(TabId(1), WindowId(1));
        tab.layout
            .split_at(WindowId(1), SplitDirection::Vertical, WindowId(2), false);

        assert_eq!(tab.active_window, WindowId(1));
        tab.cycle_next_window();
        assert_eq!(tab.active_window, WindowId(2));
        tab.cycle_next_window();
        assert_eq!(tab.active_window, WindowId(1));
    }
}
