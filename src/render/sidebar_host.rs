//! Hosting a quadraui `Sidebar` from vimcode's own event loop.
//!
//! A `Sidebar` is a controlled descriptor (its sections, active section,
//! focus, rows and rename sessions are public fields) whose interaction
//! state is a `SidebarState` the host keeps. vimcode drives its sidebars
//! from raw `UiEvent`s — some from the engine, which has no backend — so
//! [`SidebarHost`] keeps the state beside the descriptor plus the layout
//! metrics the sidebar was last painted with, and writes every
//! `SidebarAction` the widget reports back onto the descriptor.

use super::widget_paint::PaintedMetrics;
use quadraui::{
    Backend, Handled, Rect, Sidebar, SidebarAction, SidebarSectionDef, SidebarState, UiEvent,
};

/// A `Sidebar` plus the state its host keeps between frames.
#[derive(Debug)]
pub struct SidebarHost {
    /// The descriptor, which vimcode populates every frame.
    pub sidebar: Sidebar,
    state: SidebarState,
    painted: Option<PaintedMetrics>,
}

impl SidebarHost {
    /// A sidebar with one section per `defs` entry. Its trees take vim's
    /// `j`/`k` as well as the arrow keys, like every vimcode panel.
    pub fn new(id: &str, defs: &[SidebarSectionDef]) -> Self {
        let mut sidebar = Sidebar::from_defs(quadraui::WidgetId::new(id), defs);
        for section in &mut sidebar.sections {
            if let Some(tree) = section.tree_mut() {
                tree.style.vim_keys = true;
            }
        }
        Self {
            sidebar,
            state: SidebarState::default(),
            painted: None,
        }
    }

    /// Paint the sidebar into `rect`, remembering `backend`'s metrics for
    /// [`Self::handle_cached`].
    pub fn paint(&mut self, backend: &mut dyn Backend, rect: Rect) {
        self.painted = Some(PaintedMetrics::of(&*backend));
        quadraui::DrawsPrimitives::draw_primitive(backend, rect, &self.sidebar);
    }

    /// Remember `backend`'s metrics for [`Self::handle_cached`] without
    /// painting.
    pub fn record_metrics(&mut self, backend: &dyn Backend) {
        self.painted = Some(PaintedMetrics::of(backend));
    }

    /// Drive the sidebar with one raw event laid out at `rect` the way
    /// `backend` paints it, applying every action it reports.
    pub fn handle(
        &mut self,
        event: &UiEvent,
        backend: &dyn Backend,
        rect: Rect,
    ) -> Handled<Vec<SidebarAction>> {
        let handled = self
            .sidebar
            .handle_event(&mut self.state, event, backend, rect);
        self.apply_all(&handled);
        handled
    }

    /// [`Self::handle`] for an event that arrives where no backend is in
    /// reach, laid out with the metrics of the last paint. `Ignored` until
    /// the sidebar has been painted once.
    pub fn handle_cached(&mut self, event: &UiEvent, rect: Rect) -> Handled<Vec<SidebarAction>> {
        let Some(m) = self.painted.as_ref() else {
            return Handled::Ignored;
        };
        let ctx = m.ctx();
        let handled = self
            .sidebar
            .handle_event_in(&mut self.state, event, &ctx, rect);
        self.apply_all(&handled);
        handled
    }

    /// Scroll section `section`'s tree so `path` shows and select it, laid
    /// out with the metrics of the last paint.
    pub fn reveal_cached(&mut self, section: usize, path: &quadraui::TreePath, rect: Rect) {
        let Some(m) = self.painted.as_ref() else {
            return;
        };
        let ctx = m.ctx();
        self.sidebar.reveal_in(section, path, &ctx, rect);
    }

    fn apply_all(&mut self, handled: &Handled<Vec<SidebarAction>>) {
        if let Handled::Event(actions) = handled {
            for action in actions {
                self.sidebar.apply(action);
            }
        }
    }
}
