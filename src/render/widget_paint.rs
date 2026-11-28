//! Thin adapters between vimcode's paint sites and quadraui's widget
//! contract (`DrawsPrimitives::draw_dyn` / `layout_primitive`).
//!
//! Every chrome widget paints through `draw_dyn`, whose return value is a
//! type-erased `Box<dyn PrimitiveLayout>`. [`draw_widget`] downcasts it
//! back to the widget's concrete `Primitive::Layout` so a call site keeps
//! hit-testing against exactly the geometry it painted.
//!
//! A few widgets read hover/press as a sub-part of their own id (a status
//! bar's visible-segment index, for instance) rather than as the id of the
//! hovered segment itself. vimcode's interaction trackers record the
//! latter, so the per-widget helpers below re-address a tracker's state
//! onto the widget's sub-parts before painting.

use quadraui::interactive::{Interactive, SubTarget};
use quadraui::primitive::PaintState;
use quadraui::{Backend, DrawsPrimitives, InteractionState, Rect, WidgetId};

/// Paint `widget` at `rect` through the widget contract and return the
/// layout the paint resolved.
pub(crate) fn draw_widget<W>(
    backend: &mut dyn Backend,
    rect: Rect,
    widget: &W,
    state: &dyn PaintState,
) -> W::Layout
where
    W: Interactive + 'static,
    W::Layout: 'static,
{
    let painted = backend.draw_dyn(rect, widget, state);
    let painted: Box<dyn std::any::Any> = painted;
    match painted.downcast::<W::Layout>() {
        Ok(layout) => *layout,
        Err(_) => {
            debug_assert!(
                false,
                "draw_dyn returned a layout that is not the widget's own Primitive::Layout"
            );
            backend.layout_primitive(rect, widget)
        }
    }
}

/// Paint `widget` at `rect` with nothing hovered or pressed.
pub(crate) fn draw_widget_plain<W>(backend: &mut dyn Backend, rect: Rect, widget: &W) -> W::Layout
where
    W: Interactive + 'static,
    W::Layout: 'static,
{
    draw_widget(backend, rect, widget, &InteractionState::new())
}

/// Re-address an interaction state keyed by *part* ids (a segment's or a
/// button's own `WidgetId`) onto `owner`'s sub-parts. `index_of` maps a
/// part id to the sub-target index the owning widget publishes for it.
pub(crate) fn readdress_interaction(
    owner: &WidgetId,
    legacy: &InteractionState,
    index_of: impl Fn(&WidgetId) -> Option<usize>,
) -> InteractionState {
    let map = |id: Option<&WidgetId>| {
        let id = id?;
        if id == owner {
            return Some((*owner, None));
        }
        let idx = index_of(id)?;
        Some((*owner, Some(SubTarget(idx as u64))))
    };
    pressed_paints_hovered(map(legacy.hovered()), map(legacy.pressed()))
}

/// An interaction state for a hovered/pressed address pair. The widget
/// painters tint a press only while the pointer is on the pressed address,
/// but vimcode also marks a keyboard-focused or keyboard-activated button
/// as pressed wherever the pointer is. A press therefore takes the hovered
/// slot too, so it paints even while the mouse rests on another button of
/// the same widget; that other button shows no hover tint meanwhile.
// TODO(#2185): drop once quadraui's paint env carries a press that is not
// tied to the pointer (keyboard activation).
fn pressed_paints_hovered(
    hovered: Option<quadraui::focus::FocusAddress>,
    pressed: Option<quadraui::focus::FocusAddress>,
) -> InteractionState {
    let hovered = pressed.or(hovered);
    InteractionState::from_targets(hovered, pressed)
}

/// Paint a status bar and return its bar-local layout. `interaction` is
/// keyed by segment `action_id`, the way vimcode's trackers record it.
pub(crate) fn draw_status_bar(
    backend: &mut dyn Backend,
    rect: Rect,
    bar: &quadraui::StatusBar,
    interaction: &InteractionState,
) -> quadraui::StatusBarLayout {
    let state = if interaction.hovered().is_none() && interaction.pressed().is_none() {
        InteractionState::new()
    } else {
        let resolved = backend.layout_primitive(rect, bar);
        readdress_interaction(&bar.id, interaction, |id| {
            resolved.layout.visible_segments.iter().position(|vs| {
                let seg = match vs.side {
                    quadraui::StatusSegmentSide::Left => bar.left_segments.get(vs.segment_idx),
                    quadraui::StatusSegmentSide::Right => bar.right_segments.get(vs.segment_idx),
                };
                seg.and_then(|s| s.action_id.as_ref()) == Some(id)
            })
        })
    };
    draw_widget(backend, rect, bar, &state).layout
}

/// Paint a status bar whose hover and press come from the runner's ambient
/// pointer state, and return its bar-local layout.
pub(crate) fn draw_status_bar_ambient(
    backend: &mut dyn Backend,
    rect: Rect,
    bar: &quadraui::StatusBar,
) -> quadraui::StatusBarLayout {
    draw_widget(backend, rect, bar, &()).layout
}

/// A status bar's bar-local layout for `rect`, without painting.
pub(crate) fn status_bar_layout(
    backend: &dyn Backend,
    rect: Rect,
    bar: &quadraui::StatusBar,
) -> quadraui::StatusBarLayout {
    backend.layout_primitive(rect, bar).layout
}

/// Paint a tab bar and return its bar-relative layout. `hovered_close` is
/// the tab whose close glyph the pointer is over.
pub(crate) fn paint_tab_bar(
    backend: &mut dyn Backend,
    rect: Rect,
    bar: &quadraui::TabBar,
    hovered_close: Option<usize>,
) -> quadraui::TabBarLayout {
    let state = InteractionState::from_targets(
        hovered_close.map(|i| {
            (
                bar.id,
                Some(quadraui::primitives::tab_bar::TabBarSubPart::TabClose(i).sub_target()),
            )
        }),
        None,
    );
    draw_widget(backend, rect, bar, &state).layout
}

/// Paint a context menu whose items region a host has already placed.
/// `layout.bounds` is that inner items region; the box border paints in
/// the widget's own border inset around it.
pub(crate) fn draw_context_menu(
    backend: &mut dyn Backend,
    menu: &quadraui::ContextMenu,
    layout: &quadraui::ContextMenuLayout,
) {
    let inset = backend
        .layout_primitive(Rect::new(0.0, 0.0, 0.0, 0.0), menu)
        .border_inset;
    let b = layout.bounds;
    let rect = Rect::new(
        b.x - inset,
        b.y - inset,
        b.width + inset * 2.0,
        b.height + inset * 2.0,
    );
    let _ = draw_widget_plain(backend, rect, menu);
}

/// The `(item, separator)` row heights a context menu paints its rows at
/// on `backend`, so a host placing the popup sizes it to the rows the
/// widget draws.
pub(crate) fn context_menu_row_heights(backend: &dyn Backend) -> (f32, f32) {
    let probe = quadraui::ContextMenu {
        id: WidgetId::new("context_menu:probe"),
        items: Vec::new(),
        selected_idx: 0,
        bg: None,
        placement: Default::default(),
        glyphs: Default::default(),
    };
    let layout = backend.layout_primitive(Rect::new(0.0, 0.0, 0.0, 0.0), &probe);
    (layout.item_height, layout.separator_height)
}

/// Paint `dialog` inside `viewport` and return the layout it painted at,
/// in absolute coordinates. The dialog sizes its own box from its body,
/// table and input.
pub(crate) fn draw_dialog(
    backend: &mut dyn Backend,
    viewport: Rect,
    dialog: &quadraui::Dialog,
) -> quadraui::DialogLayout {
    draw_widget_plain(backend, viewport, dialog).layout
}

/// The `ToolbarSubPart::Button` index of the visible item whose
/// `action_id` is `id`.
fn toolbar_item_index(layout: &quadraui::ToolbarLayout, id: &WidgetId) -> Option<usize> {
    layout
        .visible_items
        .iter()
        .find(|vis| vis.action_id.as_ref() == Some(id))
        .map(|vis| vis.item_idx)
}

/// Re-address a tracker's button-id hover/press onto `owner`'s toolbar
/// button sub-parts.
fn toolbar_interaction(
    owner: &WidgetId,
    layout: &quadraui::ToolbarLayout,
    legacy: &InteractionState,
) -> InteractionState {
    let map = |id: Option<&WidgetId>| {
        let idx = toolbar_item_index(layout, id?)?;
        Some((
            *owner,
            Some(quadraui::primitives::toolbar::ToolbarSubPart::Button(idx).sub_target()),
        ))
    };
    pressed_paints_hovered(map(legacy.hovered()), map(legacy.pressed()))
}

/// `layout` shifted by `(dx, dy)` — a bar-local toolbar layout moved into
/// the absolute space its bar was painted in.
pub(crate) fn translate_toolbar_layout(
    mut layout: quadraui::ToolbarLayout,
    dx: f32,
    dy: f32,
) -> quadraui::ToolbarLayout {
    let shift = |r: &mut Rect| {
        r.x += dx;
        r.y += dy;
    };
    shift(&mut layout.bar_bounds);
    for vis in &mut layout.visible_items {
        shift(&mut vis.bounds);
        for child in &mut vis.group_children {
            shift(&mut child.bounds);
        }
    }
    layout
}

/// Paint a toolbar and return its layout in absolute coordinates.
/// `interaction` is keyed by button `action_id`.
pub(crate) fn draw_toolbar(
    backend: &mut dyn Backend,
    rect: Rect,
    bar: &quadraui::Toolbar,
    interaction: &InteractionState,
) -> quadraui::ToolbarLayout {
    let resolved = backend.layout_primitive(rect, bar);
    let state = toolbar_interaction(&bar.id, &resolved.layout, interaction);
    let painted = draw_widget(backend, rect, bar, &state);
    translate_toolbar_layout(painted.layout, painted.area.x, painted.area.y)
}

/// Paint a sidebar panel. `interaction` is keyed by header button
/// `action_id`.
pub(crate) fn draw_sidebar_panel(
    backend: &mut dyn Backend,
    rect: Rect,
    panel: &quadraui::SidebarPanel,
    interaction: &InteractionState,
) -> quadraui::SidebarPanelLayout {
    let resolved = backend.layout_primitive(rect, panel);
    let state = match resolved.toolbar.as_ref() {
        Some(tb) => toolbar_interaction(&panel.id, &tb.layout, interaction),
        None => InteractionState::new(),
    };
    draw_widget(backend, rect, panel, &state)
}

/// A painted sidebar panel's header toolbar layout, in absolute
/// coordinates.
#[cfg(test)]
pub(crate) fn sidebar_panel_toolbar_layout(
    layout: &quadraui::SidebarPanelLayout,
) -> Option<quadraui::ToolbarLayout> {
    let tb = layout.toolbar.as_ref()?;
    let origin = layout.toolbar_bounds?;
    Some(translate_toolbar_layout(
        tb.layout.clone(),
        origin.x,
        origin.y,
    ))
}

/// Paint `widget` through the TUI backend's widget contract into a fresh
/// `width` x `height` cell buffer, with nothing hovered or pressed.
#[cfg(test)]
pub(crate) fn paint_widget_cells<W>(
    widget: &W,
    theme: &quadraui::Theme,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer
where
    W: Interactive + 'static,
    W::Layout: 'static,
{
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
        .expect("a TestBackend terminal always builds");
    let mut tui = quadraui::tui::TuiBackend::new();
    tui.set_current_theme(theme.clone());
    terminal
        .draw(|frame| {
            tui.enter_frame_scope(frame, |b| {
                let backend: &mut dyn Backend = b;
                let rect = Rect::new(0.0, 0.0, f32::from(width), f32::from(height));
                let _ = draw_widget_plain(backend, rect, widget);
            });
        })
        .expect("drawing into a TestBackend never fails");
    terminal.backend().buffer().clone()
}

/// Fill `rect` with one solid colour — a quadraui `Canvas` holding a single
/// `DrawOp::Rect` over its whole area, registered as a `chrome:solid-fill`
/// zone. An opaque colour covers what is under it; a translucent one blends
/// over it (on the cell grid, over each cell's background, keeping its
/// glyph), which is what a modal scrim needs.
pub(crate) fn fill_solid(backend: &mut dyn Backend, rect: Rect, color: quadraui::Color) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let canvas = quadraui::Canvas {
        id: WidgetId::new("chrome:solid-fill"),
        ops: vec![quadraui::DrawOp::Rect {
            rect: Rect::new(0.0, 0.0, rect.width, rect.height),
            color,
        }],
        regions: Vec::new(),
        focusable: false,
    };
    backend.draw_primitive(rect, &canvas);
}

/// The layout metrics a widget was last painted with, for routing an event
/// to it where no backend is in reach (engine-side keyboard dispatch). The
/// same metrics `DrawsPrimitives::layout_primitive` lays a widget out with,
/// minus the backend's text measurer.
#[derive(Debug, Clone)]
pub(crate) struct PaintedMetrics {
    line_height: f32,
    char_width: f32,
    nerd_fonts_enabled: bool,
    editor_line_height: f32,
    chrome_line_height: f32,
    chrome_text_line_height: f32,
    scales_text_rows: bool,
    style: quadraui::Style,
}

impl PaintedMetrics {
    pub(crate) fn of(backend: &dyn Backend) -> Self {
        let style = backend.style();
        Self {
            line_height: quadraui::primitive::LayoutCtx::resolve_row_height(
                backend.chrome_line_height(),
                &style,
            ),
            char_width: backend.char_width(),
            nerd_fonts_enabled: backend.nerd_fonts_enabled(),
            editor_line_height: backend.line_height(),
            chrome_line_height: backend.chrome_line_height(),
            chrome_text_line_height: backend.measure_text("M", quadraui::FontRole::Chrome).1,
            scales_text_rows: backend.scales_text_rows(),
            style,
        }
    }

    pub(crate) fn ctx(&self) -> quadraui::primitive::LayoutCtx<'_> {
        quadraui::primitive::LayoutCtx::new(
            self.line_height,
            self.char_width,
            self.nerd_fonts_enabled,
        )
        .with_editor_line_height(self.editor_line_height)
        .with_chrome_line_height(self.chrome_line_height)
        .with_chrome_text_line_height(self.chrome_text_line_height)
        .with_scaled_text_rows(self.scales_text_rows)
        .with_style(&self.style)
    }
}

/// The `EventHost` a widget driven outside the runtime handles an event
/// against: the widget is the host's whole world, so it holds focus on every
/// part; capture, cursor and redraw requests have nowhere further to go.
struct DetachedHost {
    units: quadraui::primitive::Units,
    emitted: Vec<quadraui::UiEvent>,
}

impl quadraui::interactive::EventHost for DetachedHost {
    fn is_focused(&self, _id: &WidgetId, _sub: Option<SubTarget>) -> bool {
        true
    }
    fn request_focus(&mut self, _id: &WidgetId, _sub: Option<SubTarget>) {}
    fn capture_pointer(&mut self, _id: &WidgetId) {}
    fn release_pointer(&mut self) {}
    fn set_cursor(&mut self, _cursor: quadraui::PointerShape) {}
    fn request_redraw(&mut self) {}
    fn emit(&mut self, event: quadraui::UiEvent) {
        self.emitted.push(event);
    }
    fn units(&self) -> quadraui::primitive::Units {
        self.units
    }
}

/// Drive `widget` with one raw event against a `layout` it was laid out
/// into at `rect` — what `quadraui::drive_ui_event` does, for a layout
/// resolved from [`PaintedMetrics`] rather than a live backend. Answers the
/// widget's own result plus every event it emitted, or `None` for an event
/// no widget handles.
pub(crate) fn drive_laid_out<W: Interactive>(
    widget: &W,
    state: &mut W::State,
    event: &quadraui::UiEvent,
    layout: &W::Layout,
    rect: Rect,
    units: quadraui::primitive::Units,
) -> Option<(quadraui::Handled<W::Action>, Vec<quadraui::UiEvent>)> {
    let subs = Interactive::sub_regions(widget, layout);
    let routed = quadraui::compose::routed_event(event, rect, &subs)?;
    let id = quadraui::primitive::Primitive::id(widget)
        .copied()
        .unwrap_or_else(|| WidgetId::new("widget"));
    let mut host = DetachedHost {
        units,
        emitted: Vec::new(),
    };
    let handled = {
        let mut cx = quadraui::interactive::EventCx::new(id, &mut host);
        widget.handle(state, &routed, layout, &mut cx)
    };
    Some((handled, host.emitted))
}
