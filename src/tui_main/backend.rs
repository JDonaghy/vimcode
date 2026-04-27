//! TUI implementation of [`quadraui::Backend`].
//!
//! Phase B.4 Stage 1 added the struct + trait shape. Stage 2 wires up
//! the frame-scope mechanism so trait `draw_*` methods can reach
//! `&mut Frame` (only valid inside `terminal.draw(|frame| …)`'s
//! closure), plus implementations for `draw_palette`, `draw_list`,
//! `draw_tree`, `draw_form` — the four primitives whose existing
//! `quadraui_tui::draw_*` shims already match the `(buf, area, prim,
//! theme)` shape and migrate cleanly.
//!
//! The other 5 trait `draw_*` methods (status_bar, tab_bar,
//! activity_bar, terminal, text_display) stay stubbed for now because
//! their existing shims take a pre-computed `*Layout` parameter that
//! the trait doesn't pass through. Migrating them needs either the
//! trait gaining `&Layout` parameters (per
//! `BACKEND_TRAIT_PROPOSAL.md` §6.2), or the impl recomputing layout
//! from `&Primitive`. Stage 3 (`paint<B: Backend>` extraction) is the
//! natural place to resolve that — it has to confront cross-call-site
//! layout reuse anyway.
//!
//! `poll_events` / `wait_events` are stubs (Stage 4 fills them in).

use std::cell::Cell;
use std::collections::HashMap;
use std::time::Duration;

use quadraui::{
    Accelerator, AcceleratorId, ActivityBar, Backend, DragState, Form, ListView, ModalStack,
    Palette, PlatformServices, Rect as QRect, StatusBar, TabBar, Terminal as TerminalPrim,
    TextDisplay, TreeView, UiEvent, Viewport,
};
use ratatui::layout::Rect;
use ratatui::Frame;

use super::services::TuiPlatformServices;

/// TUI backend implementing [`quadraui::Backend`].
///
/// Owns the persistent UI state the trait requires plus a transient
/// "current frame" pointer + theme set inside
/// [`Self::enter_frame_scope`]. The pointer is type-erased
/// (`*mut ()`) and cleared on scope exit; safe accessors deref it
/// only while the scope is active.
///
/// The ratatui `Terminal` is **not** owned here — it stays as a local
/// in [`super::event_loop`]. See `BACKEND_TRAIT_PROPOSAL.md` §11 for
/// rationale and the eventual migration plan.
pub struct TuiBackend {
    viewport: Viewport,
    modal_stack: ModalStack,
    drag_state: DragState,
    accelerators: HashMap<AcceleratorId, Accelerator>,
    services: TuiPlatformServices,
    /// Type-erased `&mut Frame<'_>` pointer; non-null only inside
    /// [`Self::enter_frame_scope`]. `Cell` (not `RefCell`) because
    /// trait methods borrow `&mut self` already; we only need
    /// shared-cell semantics for `Copy`-able pointer values.
    current_frame_ptr: Cell<*mut ()>,
    /// Theme captured by the most recent
    /// [`Self::set_current_theme`] call. Defaults to
    /// `quadraui::Theme::default()` until set.
    current_theme: quadraui::Theme,
}

impl TuiBackend {
    /// Construct the backend with default viewport (80×24) and
    /// default quadraui theme. The caller calls [`Backend::begin_frame`]
    /// each frame (after `terminal.size()`) to keep
    /// [`Backend::viewport`] in sync, and [`Self::set_current_theme`]
    /// before drawing so the trait `draw_*` methods see the right
    /// palette.
    pub fn new() -> Self {
        Self {
            viewport: Viewport::default(),
            modal_stack: ModalStack::new(),
            drag_state: DragState::new(),
            accelerators: HashMap::new(),
            services: TuiPlatformServices::new(),
            current_frame_ptr: Cell::new(std::ptr::null_mut()),
            current_theme: quadraui::Theme::default(),
        }
    }

    /// Enter the frame-scope: stash the `&mut Frame<'_>` pointer for
    /// trait `draw_*` methods to access, run `f`, then clear the
    /// pointer. **Must** be called from inside a
    /// `terminal.draw(|frame| …)` closure.
    ///
    /// Type-erased through `*mut ()` because `Frame<'a>` carries a
    /// lifetime parameter we don't want to thread onto `TuiBackend`.
    /// Safety relies on three invariants enforced by this function's
    /// shape:
    ///   1. The pointer is set immediately before running `f` and
    ///      cleared immediately after, including on panic (via
    ///      [`scopeguard`]-style restore).
    ///   2. `f` cannot move the pointer out — it only sees it via
    ///      [`Self::current_frame_mut`] which returns a fresh
    ///      `&mut Frame<'_>` borrow scoped to the call.
    ///   3. `enter_frame_scope` calls don't nest meaningfully —
    ///      the inner call would overwrite the pointer with the
    ///      same `&mut` (already aliased) which Rust's borrow-checker
    ///      forbids at the caller side.
    pub fn enter_frame_scope<R>(
        &mut self,
        frame: &mut Frame<'_>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let ptr = frame as *mut Frame<'_> as *mut ();
        let prev = self.current_frame_ptr.replace(ptr);
        let result = f(self);
        self.current_frame_ptr.set(prev);
        result
    }

    /// Get the current frame inside [`Self::enter_frame_scope`], or
    /// `None` outside it. Trait `draw_*` methods call this and bail
    /// (panic in dev, silent return otherwise) if `None`.
    fn current_frame_mut(&mut self) -> Option<&mut Frame<'static>> {
        let ptr = self.current_frame_ptr.get();
        if ptr.is_null() {
            None
        } else {
            // SAFETY: `enter_frame_scope` set this from a real
            // `&mut Frame<'_>` and won't return until the scope
            // ends, at which point the pointer is cleared. Outside
            // the scope `ptr` is null and we return `None`.
            // The `'static` lifetime here is a fiction — the borrow
            // is actually scoped to the enclosing
            // `enter_frame_scope` call. Methods using this never let
            // the borrow escape past their own return.
            Some(unsafe { &mut *(ptr as *mut Frame<'static>) })
        }
    }

    /// Update the cached quadraui theme. Call once per frame from
    /// `paint`, before any `backend.draw_*` calls. Subsequent
    /// `draw_*` invocations consume the stored theme.
    pub fn set_current_theme(&mut self, theme: quadraui::Theme) {
        self.current_theme = theme;
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

/// Convert a [`quadraui::Rect`] (f32 coordinates) to a
/// [`ratatui::layout::Rect`] (u16). Any negative values clamp to 0;
/// fractional widths/heights round to nearest. Used by every trait
/// `draw_*` method to translate the trait's `Rect` argument.
fn q_rect_to_ratatui(r: QRect) -> Rect {
    let x = r.x.max(0.0).round() as u16;
    let y = r.y.max(0.0).round() as u16;
    let w = r.width.max(0.0).round() as u16;
    let h = r.height.max(0.0).round() as u16;
    Rect::new(x, y, w, h)
}

impl Backend for TuiBackend {
    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn begin_frame(&mut self, viewport: Viewport) {
        self.viewport = viewport;
    }

    fn end_frame(&mut self) {
        // No-op. The frame's actual flush happens when ratatui's
        // `terminal.draw(|frame| …)` closure returns; this method
        // exists for parity with backends that need explicit flush.
    }

    fn poll_events(&mut self) -> Vec<UiEvent> {
        Vec::new() // Stage 4
    }

    fn wait_events(&mut self, _timeout: Duration) -> Vec<UiEvent> {
        Vec::new() // Stage 4
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

    // ─── Drawing ───────────────────────────────────────────────────────────
    //
    // Implementations call into the public `quadraui::tui::draw_*` free
    // functions; this trait impl is the thin wrapper. The frame is
    // stashed by `enter_frame_scope`; the theme by `set_current_theme`.
    // Calling these outside `enter_frame_scope` is a programmer error
    // and panics in dev (the `expect` makes the boundary loud).

    fn draw_tree(&mut self, rect: QRect, tree: &TreeView) {
        let area = q_rect_to_ratatui(rect);
        let theme = self.current_theme;
        let frame = self
            .current_frame_mut()
            .expect("TuiBackend::draw_tree called outside enter_frame_scope");
        quadraui::tui::draw_tree(
            frame.buffer_mut(),
            area,
            tree,
            &theme,
            crate::icons::nerd_fonts_enabled(),
        );
    }

    fn draw_list(&mut self, rect: QRect, list: &ListView) {
        let area = q_rect_to_ratatui(rect);
        let theme = self.current_theme;
        let frame = self
            .current_frame_mut()
            .expect("TuiBackend::draw_list called outside enter_frame_scope");
        quadraui::tui::draw_list(
            frame.buffer_mut(),
            area,
            list,
            &theme,
            crate::icons::nerd_fonts_enabled(),
        );
    }

    fn draw_form(&mut self, rect: QRect, form: &Form) {
        let area = q_rect_to_ratatui(rect);
        let theme = self.current_theme;
        let frame = self
            .current_frame_mut()
            .expect("TuiBackend::draw_form called outside enter_frame_scope");
        quadraui::tui::draw_form(frame.buffer_mut(), area, form, &theme);
    }

    fn draw_palette(&mut self, rect: QRect, palette: &Palette) {
        let area = q_rect_to_ratatui(rect);
        let theme = self.current_theme;
        let frame = self
            .current_frame_mut()
            .expect("TuiBackend::draw_palette called outside enter_frame_scope");
        quadraui::tui::draw_palette(
            frame.buffer_mut(),
            area,
            palette,
            &theme,
            crate::icons::nerd_fonts_enabled(),
        );
    }

    // ─── Layout-passthrough primitives — Stage 3 / trait migration ──────
    //
    // These take a pre-computed `*Layout` in their existing TUI
    // shims. Migrating them through the trait needs either the
    // trait to take `&Layout` (per `BACKEND_TRAIT_PROPOSAL.md` §6.2)
    // or a per-method recompute. Deferred until Stage 3.

    fn draw_status_bar(&mut self, _rect: QRect, _bar: &StatusBar) {
        unimplemented!("TuiBackend::draw_status_bar — see Stage 3")
    }

    fn draw_tab_bar(&mut self, _rect: QRect, _bar: &TabBar) {
        unimplemented!("TuiBackend::draw_tab_bar — see Stage 3")
    }

    fn draw_activity_bar(&mut self, _rect: QRect, _bar: &ActivityBar) {
        unimplemented!("TuiBackend::draw_activity_bar — see Stage 3")
    }

    fn draw_terminal(&mut self, _rect: QRect, _term: &TerminalPrim) {
        unimplemented!("TuiBackend::draw_terminal — see Stage 3")
    }

    fn draw_text_display(&mut self, _rect: QRect, _td: &TextDisplay) {
        unimplemented!("TuiBackend::draw_text_display — see Stage 3")
    }
}
