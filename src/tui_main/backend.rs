//! TUI implementation of [`quadraui::Backend`].
//!
//! Phase B.4 Stage 1: scaffold the struct + trait impl. `TuiBackend`
//! holds the persistent UI state the trait requires — modal stack,
//! drag state, accelerator registry, platform services, cached
//! viewport. It does **not** own the ratatui `Terminal` for now;
//! that stays as a local in [`super::event_loop`] and is reached via
//! `backend.terminal_mut()` once Stage 2 routes draws through trait
//! methods. Decoupling Terminal ownership from `TuiBackend` keeps
//! Stage 1's diff small and avoids 80-site mechanical renames.
//!
//! The 9 `draw_*` methods are stubbed (`unimplemented!()`) for now —
//! the existing TUI render path still calls `quadraui::tui::draw_*`
//! free functions directly via `terminal.draw(|frame| …)`. Stage 2
//! migrates the render path to call `backend.draw_*` instead.
//!
//! `poll_events` / `wait_events` are stubs (Stage 4 fills them in).

use std::collections::HashMap;
use std::time::Duration;

use quadraui::{
    Accelerator, AcceleratorId, ActivityBar, Backend, DragState, Form, ListView, ModalStack,
    Palette, PlatformServices, Rect, StatusBar, TabBar, Terminal as TerminalPrim, TextDisplay,
    TreeView, UiEvent, Viewport,
};

use super::services::TuiPlatformServices;

/// TUI backend implementing [`quadraui::Backend`].
///
/// Owns the persistent UI state the trait requires: modal stack,
/// drag state, accelerator registry, platform services, cached
/// viewport. The ratatui `Terminal` is **not** owned here — it stays
/// as a local in [`super::event_loop`] (Stage 1 simplification).
/// Stage 2 routes draws through the trait's `draw_*` methods, at
/// which point the Terminal moves into this struct or a sibling.
///
/// Construct with [`TuiBackend::new`].
pub struct TuiBackend {
    viewport: Viewport,
    modal_stack: ModalStack,
    drag_state: DragState,
    accelerators: HashMap<AcceleratorId, Accelerator>,
    services: TuiPlatformServices,
}

impl TuiBackend {
    /// Construct the backend with default viewport (80×24). The
    /// caller calls [`Backend::begin_frame`] each frame (after
    /// `terminal.size()`) to keep [`Backend::viewport`] in sync.
    pub fn new() -> Self {
        Self {
            viewport: Viewport::default(),
            modal_stack: ModalStack::new(),
            drag_state: DragState::new(),
            accelerators: HashMap::new(),
            services: TuiPlatformServices::new(),
        }
    }

    /// Mutable access to the drag-state. Inherent helper because the
    /// trait doesn't expose drag state — the existing mouse handler
    /// in `tui_main/mouse.rs` reads/writes it through this borrow.
    /// Used once Stage 5 reorganises mouse.rs around the backend.
    #[allow(dead_code)]
    pub fn drag_state_mut(&mut self) -> &mut DragState {
        &mut self.drag_state
    }

    /// Read-only access to the drag state.
    #[allow(dead_code)]
    pub fn drag_state(&self) -> &DragState {
        &self.drag_state
    }

    /// Disjoint mutable borrows of drag state and modal stack —
    /// `mouse.rs::handle_mouse` needs both at the same time.
    /// Calling `drag_state_mut()` then `modal_stack_mut()` separately
    /// would conflict; this helper splits the field borrows.
    pub fn drag_and_modal_mut(&mut self) -> (&mut DragState, &mut ModalStack) {
        (&mut self.drag_state, &mut self.modal_stack)
    }

    /// Iterate registered accelerators (Stage 4 will use this in
    /// `poll_events`).
    #[allow(dead_code)]
    pub(crate) fn accelerators(&self) -> impl Iterator<Item = (&AcceleratorId, &Accelerator)> {
        self.accelerators.iter()
    }
}

impl Default for TuiBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for TuiBackend {
    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn begin_frame(&mut self, viewport: Viewport) {
        // Stage 1 captures the viewport. Frame setup proper happens
        // inside ratatui's `terminal.draw(|frame| …)` closure, owned
        // by the event loop. Stage 2 introduces a `with_frame` inherent
        // helper and the trait's `draw_*` methods access the frame
        // through it.
        self.viewport = viewport;
    }

    fn end_frame(&mut self) {
        // Stage 1 no-op. See `begin_frame`.
    }

    fn poll_events(&mut self) -> Vec<UiEvent> {
        // Stage 4 fills this in. For Stage 1 the existing event loop
        // still drives crossterm directly.
        Vec::new()
    }

    fn wait_events(&mut self, _timeout: Duration) -> Vec<UiEvent> {
        // Stage 4 fills this in.
        Vec::new()
    }

    fn register_accelerator(&mut self, acc: &Accelerator) {
        self.accelerators.insert(acc.id.clone(), acc.clone());
    }

    fn unregister_accelerator(&mut self, id: &AcceleratorId) {
        self.accelerators.remove(id);
    }

    fn modal_stack_mut(&mut self) -> &mut ModalStack {
        &mut self.modal_stack
    }

    fn services(&self) -> &dyn PlatformServices {
        &self.services
    }

    // ─── Drawing — Stage 1 stubs, Stage 2 fills these in ────────────────
    //
    // The existing event loop calls `quadraui::tui::draw_*` free
    // functions directly via `terminal.draw(|frame| …)`. These trait
    // methods aren't reached until Stage 2 routes the render path
    // through them.

    fn draw_tree(&mut self, _rect: Rect, _tree: &TreeView) {
        unimplemented!("TuiBackend::draw_tree wired up in B.4 Stage 2")
    }

    fn draw_list(&mut self, _rect: Rect, _list: &ListView) {
        unimplemented!("TuiBackend::draw_list wired up in B.4 Stage 2")
    }

    fn draw_form(&mut self, _rect: Rect, _form: &Form) {
        unimplemented!("TuiBackend::draw_form wired up in B.4 Stage 2")
    }

    fn draw_palette(&mut self, _rect: Rect, _palette: &Palette) {
        unimplemented!("TuiBackend::draw_palette wired up in B.4 Stage 2")
    }

    fn draw_status_bar(&mut self, _rect: Rect, _bar: &StatusBar) {
        unimplemented!("TuiBackend::draw_status_bar wired up in B.4 Stage 2")
    }

    fn draw_tab_bar(&mut self, _rect: Rect, _bar: &TabBar) {
        unimplemented!("TuiBackend::draw_tab_bar wired up in B.4 Stage 2")
    }

    fn draw_activity_bar(&mut self, _rect: Rect, _bar: &ActivityBar) {
        unimplemented!("TuiBackend::draw_activity_bar wired up in B.4 Stage 2")
    }

    fn draw_terminal(&mut self, _rect: Rect, _term: &TerminalPrim) {
        unimplemented!("TuiBackend::draw_terminal wired up in B.4 Stage 2")
    }

    fn draw_text_display(&mut self, _rect: Rect, _td: &TextDisplay) {
        unimplemented!("TuiBackend::draw_text_display wired up in B.4 Stage 2")
    }
}
