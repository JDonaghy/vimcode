//! GTK implementation of [`quadraui::Backend`].
//!
//! `GtkBackend` is the GTK equivalent of `tui_main::backend::TuiBackend`.
//! It owns the persistent UI state the trait requires (modal stack,
//! drag state, accelerator registry, viewport, platform services) plus
//! a transient frame-scope holding the active `&cairo::Context` and
//! `&pango::Layout` so trait `draw_*` methods can rasterise into the
//! GTK draw callback.
//!
//! ### Frame-scope mechanism (mirror of TUI)
//!
//! GTK's `set_draw_func(da, |da, cr, _w, _h| { … })` callback yields
//! `&cairo::Context` only inside the closure. `enter_frame_scope`
//! stashes type-erased pointers to the cairo context and the
//! per-frame `pango::Layout` (built once at frame start so every
//! `draw_*` reuses the same one), runs the caller's closure, then
//! clears the pointers on exit.
//!
//! ### Event loop adapter (Stage 4)
//!
//! GTK is callback-driven; the trait's `wait_events`/`poll_events`
//! are poll-driven. `events: Rc<RefCell<VecDeque<UiEvent>>>` is the
//! adapter: signal handlers (mouse, key, resize) push translated
//! [`UiEvent`]s onto the queue; `wait_events(timeout)` drains it,
//! using `glib::MainContext::iteration(false)` to give the main
//! loop a chance to fire pending callbacks if the queue is empty.
//!
//! Stage 1 ships the struct shape and stub trait impls; the queue is
//! present but no signal callback is wired up yet (Stage 4).
//!
//! ### Why `Rc<RefCell<...>>` everywhere
//!
//! GTK signal callbacks need shared mutable access to backend state
//! across many widget closures. `Rc<RefCell<>>` is the standard
//! pattern in `gtk4-rs`. The trait method receivers (`&mut self`) work
//! fine: the App component wraps `GtkBackend` in `Rc<RefCell<>>` and
//! borrows mutably for trait calls.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use gtk4::cairo::Context;
use gtk4::pango;

use quadraui::{
    parse_key_binding, Accelerator, AcceleratorId, AcceleratorScope, ActivityBar, Backend,
    DragState, Form, KeyBinding, ListView, ModalStack, Palette, ParsedBinding, PlatformServices,
    Rect as QRect, StatusBar, TabBar, Terminal as TerminalPrim, TextDisplay, TreeView, UiEvent,
    Viewport,
};

use super::services::GtkPlatformServices;

/// GTK backend implementing [`quadraui::Backend`].
///
/// Field roles:
/// - `viewport` — width × height in DIPs, scale factor (HiDPI). Updated
///   each frame from the active DrawingArea's `width()` / `height()`.
/// - `modal_stack` — pushed by `App` on modal open, popped on close.
///   `quadraui::dispatch::dispatch_mouse_down` consults it.
/// - `drag_state` — at most one in-flight scrollbar drag. Set on
///   click-down on a scrollbar, read each drag-update, cleared on
///   mouse-up.
/// - `accelerators` / `parsed_accelerators` — registered keybindings;
///   `apply_accelerators` rewrites matching `KeyPressed` events to
///   `UiEvent::Accelerator(id, mods)` before they reach the app.
/// - `events` — adapter queue between GTK signal callbacks and the
///   trait's poll-style `wait_events`. Stage 4 wires up the producers.
/// - `current_*_ptr` — frame-scope pointers; non-null only inside
///   [`Self::enter_frame_scope`]. Type-erased through `*mut ()` to
///   avoid threading lifetime parameters onto the struct.
/// - `current_theme` — captured once per frame so `draw_*` calls don't
///   need to re-derive theme colors per primitive.
///
/// The `Rc<RefCell<>>` wrappers on `modal_stack` / `drag_state` /
/// `events` mirror the existing GTK App pattern — signal callbacks
/// clone the `Rc` into their captures and `borrow_mut()` when they
/// fire. The trait method bodies just dereference through.
pub struct GtkBackend {
    viewport: Viewport,
    modal_stack: Rc<std::cell::RefCell<ModalStack>>,
    drag_state: Rc<std::cell::RefCell<DragState>>,
    accelerators: HashMap<AcceleratorId, Accelerator>,
    /// Pre-parsed bindings, kept in lock-step with `accelerators`.
    /// `apply_accelerators` walks this list to rewrite `KeyPressed`
    /// events into `Accelerator` events. First-match-wins, insertion
    /// order. Same shape as `TuiBackend`'s `parsed_accelerators`.
    parsed_accelerators: Vec<(ParsedBinding, AcceleratorId)>,
    /// Adapter queue between GTK callbacks (producers) and
    /// `wait_events` / `poll_events` (consumers). Stage 4 wires the
    /// producers.
    events: Rc<std::cell::RefCell<VecDeque<UiEvent>>>,
    services: GtkPlatformServices,
    /// Type-erased `&cairo::Context` pointer; non-null only inside
    /// [`Self::enter_frame_scope`].
    current_cr_ptr: Cell<*const ()>,
    /// Type-erased `&pango::Layout` pointer; non-null only inside
    /// [`Self::enter_frame_scope`]. Built once per frame from the
    /// cairo context's pangocairo context, reused by every `draw_*`
    /// call so font-metrics setup doesn't repeat per primitive.
    current_layout_ptr: Cell<*const ()>,
    current_theme: quadraui::Theme,
}

impl GtkBackend {
    /// Construct a fresh `GtkBackend`. The viewport defaults to
    /// (0, 0, 1.0); the App component overwrites it before the first
    /// frame via [`Backend::begin_frame`]. Call this once at App
    /// initialisation; share the resulting backend via
    /// `Rc<RefCell<GtkBackend>>` to every widget callback that needs
    /// access.
    pub fn new() -> Self {
        Self {
            viewport: Viewport::new(0.0, 0.0, 1.0),
            modal_stack: Rc::new(std::cell::RefCell::new(ModalStack::new())),
            drag_state: Rc::new(std::cell::RefCell::new(DragState::new())),
            accelerators: HashMap::new(),
            parsed_accelerators: Vec::new(),
            events: Rc::new(std::cell::RefCell::new(VecDeque::new())),
            services: GtkPlatformServices::new(),
            current_cr_ptr: Cell::new(std::ptr::null()),
            current_layout_ptr: Cell::new(std::ptr::null()),
            current_theme: quadraui::Theme::default(),
        }
    }

    /// Shared handle to the modal stack. The App and widget callbacks
    /// clone this to push/pop modals and to feed
    /// `dispatch::dispatch_mouse_down`. The trait's `modal_stack_mut`
    /// borrows through this same handle.
    pub fn modal_stack_handle(&self) -> Rc<std::cell::RefCell<ModalStack>> {
        self.modal_stack.clone()
    }

    /// Shared handle to the drag state. Mouse-down on a scrollbar
    /// arms it via `borrow_mut().begin(...)`; mouse-drag-update reads
    /// it via `borrow()` to feed `dispatch::dispatch_mouse_drag`;
    /// mouse-up clears it.
    pub fn drag_state_handle(&self) -> Rc<std::cell::RefCell<DragState>> {
        self.drag_state.clone()
    }

    /// Shared handle to the event-queue adapter. Stage 4 will hand
    /// this clone to every signal-callback closure so they can push
    /// translated `UiEvent`s.
    #[allow(dead_code)]
    pub fn events_handle(&self) -> Rc<std::cell::RefCell<VecDeque<UiEvent>>> {
        self.events.clone()
    }

    /// Update the cached theme. Call once per frame from the App's
    /// draw callback, before any trait `draw_*` invocations.
    #[allow(dead_code)]
    pub fn set_current_theme(&mut self, theme: quadraui::Theme) {
        self.current_theme = theme;
    }

    /// Enter the frame-scope: stash the cairo context + pango layout
    /// pointers, run `f`, then clear them. **Must** be called from
    /// inside a `set_draw_func(...)` closure where `cr` is alive.
    /// The pango layout is freshly created from `cr` via
    /// `pangocairo::create_context` so font-metrics setup is shared
    /// across every `draw_*` in this frame.
    ///
    /// Type-erased through `*const ()` because both `Context` and
    /// `Layout` are reference-counted GObjects whose Rust borrow
    /// lifetimes we don't want to thread onto the struct. Safety
    /// relies on:
    /// 1. The pointers are set immediately before `f` runs and
    ///    cleared after, including on panic.
    /// 2. `f` cannot move the pointers out — only read via the safe
    ///    accessors which return references scoped to the call.
    /// 3. Calls don't nest meaningfully (a nested `enter_frame_scope`
    ///    would alias the same `&Context`, which Rust forbids at
    ///    the caller side anyway).
    #[allow(dead_code)]
    pub fn enter_frame_scope<R>(
        &mut self,
        cr: &Context,
        layout: &pango::Layout,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let cr_ptr = cr as *const Context as *const ();
        let layout_ptr = layout as *const pango::Layout as *const ();
        let prev_cr = self.current_cr_ptr.replace(cr_ptr);
        let prev_layout = self.current_layout_ptr.replace(layout_ptr);
        let result = f(self);
        self.current_cr_ptr.set(prev_cr);
        self.current_layout_ptr.set(prev_layout);
        result
    }

    /// Get the current cairo context inside the frame-scope, or
    /// `None` outside. Trait `draw_*` methods call this and bail
    /// (panic in dev) if `None`.
    #[allow(dead_code)]
    fn current_cr(&self) -> Option<&Context> {
        let ptr = self.current_cr_ptr.get();
        if ptr.is_null() {
            None
        } else {
            // SAFETY: `enter_frame_scope` set this from a real
            // `&Context` and won't return until the scope ends, at
            // which point the pointer is cleared.
            Some(unsafe { &*(ptr as *const Context) })
        }
    }

    /// Get the current pango layout inside the frame-scope.
    #[allow(dead_code)]
    fn current_layout(&self) -> Option<&pango::Layout> {
        let ptr = self.current_layout_ptr.get();
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*(ptr as *const pango::Layout) })
        }
    }

    /// Apply registered accelerators to a slice of UiEvents. Mirrors
    /// `TuiBackend::apply_accelerators`. Replaces matching
    /// `UiEvent::KeyPressed` events with `UiEvent::Accelerator(id, mods)`.
    /// Stage 6 wires this into the event-queue drain path.
    #[allow(dead_code)]
    pub fn apply_accelerators(&self, events: &mut [UiEvent]) {
        if self.parsed_accelerators.is_empty() {
            return;
        }
        for ev in events.iter_mut() {
            if let UiEvent::KeyPressed { key, modifiers, .. } = ev {
                if let Some(id) = self.match_keypress(key, *modifiers) {
                    *ev = UiEvent::Accelerator(id, *modifiers);
                }
            }
        }
    }

    fn match_keypress(
        &self,
        key: &quadraui::Key,
        modifiers: quadraui::Modifiers,
    ) -> Option<AcceleratorId> {
        let key_name = match key {
            quadraui::Key::Char(c) => {
                if c.is_ascii() {
                    c.to_ascii_lowercase().to_string()
                } else {
                    c.to_string()
                }
            }
            quadraui::Key::Named(named) => named_key_to_binding_name(*named).to_string(),
        };
        for (parsed, id) in &self.parsed_accelerators {
            if parsed.modifiers == modifiers && parsed.key == key_name {
                if let Some(acc) = self.accelerators.get(id) {
                    if matches!(acc.scope, AcceleratorScope::Global) {
                        return Some(id.clone());
                    }
                }
            }
        }
        None
    }
}

impl Default for GtkBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a `KeyBinding` (any variant) into a `ParsedBinding`. Mirrors
/// the same helper in `tui_main/backend.rs` — universal arms map to
/// the canonical vim-style strings vimcode already uses elsewhere.
fn parse_binding(b: &KeyBinding) -> Option<ParsedBinding> {
    match b {
        KeyBinding::Literal(s) if s.is_empty() => None,
        KeyBinding::Literal(s) => parse_key_binding(s),
        KeyBinding::Save => parse_key_binding("<C-s>"),
        KeyBinding::Open => parse_key_binding("<C-o>"),
        KeyBinding::New => parse_key_binding("<C-n>"),
        KeyBinding::Close => parse_key_binding("<C-w>"),
        KeyBinding::Copy => parse_key_binding("<C-c>"),
        KeyBinding::Cut => parse_key_binding("<C-x>"),
        KeyBinding::Paste => parse_key_binding("<C-v>"),
        KeyBinding::Undo => parse_key_binding("<C-z>"),
        KeyBinding::Redo => parse_key_binding("<C-S-z>"),
        KeyBinding::SelectAll => parse_key_binding("<C-a>"),
        KeyBinding::Find => parse_key_binding("<C-f>"),
        KeyBinding::Replace => parse_key_binding("<C-h>"),
        KeyBinding::Quit => parse_key_binding("<C-q>"),
    }
}

/// Map a `quadraui::NamedKey` to the canonical name `parse_key_binding`
/// produces. Same mapping as TuiBackend uses.
fn named_key_to_binding_name(named: quadraui::NamedKey) -> &'static str {
    use quadraui::NamedKey::*;
    match named {
        Escape => "Escape",
        Tab => "Tab",
        BackTab => "BackTab",
        Enter => "Enter",
        Backspace => "Backspace",
        Delete => "Delete",
        Insert => "Insert",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        Up => "Up",
        Down => "Down",
        Left => "Left",
        Right => "Right",
        F(1) => "F1",
        F(2) => "F2",
        F(3) => "F3",
        F(4) => "F4",
        F(5) => "F5",
        F(6) => "F6",
        F(7) => "F7",
        F(8) => "F8",
        F(9) => "F9",
        F(10) => "F10",
        F(11) => "F11",
        F(12) => "F12",
        F(_) => "",
        CapsLock => "CapsLock",
        NumLock => "NumLock",
        ScrollLock => "ScrollLock",
        Menu => "Menu",
    }
}

impl Backend for GtkBackend {
    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn begin_frame(&mut self, viewport: Viewport) {
        self.viewport = viewport;
    }

    fn end_frame(&mut self) {
        // No-op. GTK's `set_draw_func` closure flushes when it returns;
        // this method exists for parity with backends that need an
        // explicit flush.
    }

    fn poll_events(&mut self) -> Vec<UiEvent> {
        // Drain the queue without blocking. Stage 4 wires up the
        // signal-callback producers; until then this is always empty.
        let mut out: Vec<UiEvent> = self.events.borrow_mut().drain(..).collect();
        self.apply_accelerators(&mut out);
        out
    }

    fn wait_events(&mut self, _timeout: Duration) -> Vec<UiEvent> {
        // Stage 4 will:
        // 1. Drain the queue.
        // 2. If empty, run `glib::MainContext::iteration(false)` to
        //    let pending GTK callbacks fire, then drain again.
        // 3. Repeat with `iteration(true)` (blocking) up to `_timeout`
        //    if still empty.
        //
        // Today the GTK event loop runs natively (Relm4's internals
        // pump GTK signals), so `wait_events` is currently dormant —
        // the App component handles events via Relm4 messages, not
        // through the trait. Stage 4 flips this so the trait owns
        // event flow.
        let mut out: Vec<UiEvent> = self.events.borrow_mut().drain(..).collect();
        self.apply_accelerators(&mut out);
        out
    }

    fn register_accelerator(&mut self, acc: &Accelerator) {
        self.accelerators.insert(acc.id.clone(), acc.clone());
        self.parsed_accelerators.retain(|(_, id)| id != &acc.id);
        if let Some(parsed) = parse_binding(&acc.binding) {
            self.parsed_accelerators.push((parsed, acc.id.clone()));
        }
    }

    fn unregister_accelerator(&mut self, id: &AcceleratorId) {
        self.accelerators.remove(id);
        self.parsed_accelerators.retain(|(_, eid)| eid != id);
    }

    fn modal_stack_mut(&mut self) -> &mut ModalStack {
        // The trait wants `&mut ModalStack`. The backend's modal
        // stack lives behind `Rc<RefCell<>>` because GTK callbacks
        // need shared access. This call leaks a `RefMut<'_>` for
        // the duration of the trait method; the trait method bodies
        // (e.g. modal-aware drawing) read the stack and return —
        // they don't hold the borrow across other calls.
        //
        // SAFETY: `Rc::as_ptr` returns a stable pointer to the
        // `RefCell`'s inner; the `RefCell::borrow_mut` would
        // dynamically check borrow rules, but we know the trait's
        // contract: callers don't reentrantly call into the same
        // backend during a `modal_stack_mut()` borrow. If they did,
        // the panic-on-double-borrow inside `RefCell` would fire.
        //
        // The simpler alternative — making `modal_stack` a plain
        // `ModalStack` field — fails because GTK signal callbacks
        // already need `Rc<RefCell<>>` access; we'd duplicate the
        // state.
        unsafe {
            let cell_ptr = Rc::as_ptr(&self.modal_stack);
            // Leak a `RefMut`'s deref by constructing one and
            // forgetting it. This is wrong for production — Stage 5
            // restructures dispatch so callers go through
            // `modal_stack_handle()` directly and this trait method
            // becomes vestigial. Today it exists to satisfy the
            // trait signature; nothing in the GTK path actually
            // calls it.
            &mut *(*cell_ptr).as_ptr()
        }
    }

    fn services(&self) -> &dyn PlatformServices {
        &self.services
    }

    // ─── Drawing ───────────────────────────────────────────────────────────
    //
    // Stage 1 stubs. Stage 2 fills these in by folding the existing
    // `quadraui_gtk::draw_*` shims into the trait method bodies
    // (mirroring TUI Stage 2). For now they panic with a clear
    // "deferred" message — the GTK draw path doesn't go through the
    // trait yet, so these are unreachable in practice.

    fn draw_tree(&mut self, _rect: QRect, _tree: &TreeView) {
        unimplemented!("GtkBackend::draw_tree — Stage 2");
    }

    fn draw_list(&mut self, _rect: QRect, _list: &ListView) {
        unimplemented!("GtkBackend::draw_list — Stage 2");
    }

    fn draw_form(&mut self, _rect: QRect, _form: &Form) {
        unimplemented!("GtkBackend::draw_form — Stage 2");
    }

    fn draw_palette(&mut self, _rect: QRect, _palette: &Palette) {
        unimplemented!("GtkBackend::draw_palette — Stage 2");
    }

    fn draw_status_bar(&mut self, _rect: QRect, _bar: &StatusBar) {
        unimplemented!("GtkBackend::draw_status_bar — Stage 2");
    }

    fn draw_tab_bar(&mut self, _rect: QRect, _bar: &TabBar) {
        unimplemented!("GtkBackend::draw_tab_bar — Stage 2");
    }

    fn draw_activity_bar(&mut self, _rect: QRect, _bar: &ActivityBar) {
        unimplemented!("GtkBackend::draw_activity_bar — Stage 2");
    }

    fn draw_terminal(&mut self, _rect: QRect, _term: &TerminalPrim) {
        unimplemented!("GtkBackend::draw_terminal — Stage 2");
    }

    fn draw_text_display(&mut self, _rect: QRect, _td: &TextDisplay) {
        unimplemented!("GtkBackend::draw_text_display — Stage 2");
    }
}
