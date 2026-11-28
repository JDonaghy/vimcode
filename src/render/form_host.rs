//! Hosting a quadraui `ScrolledForm` from vimcode's own event loop.
//!
//! vimcode rebuilds each form (the Settings panel, a plugin view's field
//! stack) from engine state every frame, with the engine's own selection
//! and scroll offset on the descriptor, and drives it from raw `UiEvent`s —
//! keyboard events from the engine, which has no backend. [`FormHost`]
//! keeps the widget's `FormState` beside the descriptor, plus the metrics
//! it was last painted with so an engine-side event lays the form out the
//! way it was painted.

use super::widget_paint::{drive_laid_out, PaintedMetrics};
use quadraui::{Backend, Form, FormEvent, FormState, Handled, Rect, ScrolledForm, UiEvent};

/// A form, its interaction state, and the metrics of its last paint.
#[derive(Debug)]
pub struct FormHost {
    pub form: ScrolledForm,
    pub state: FormState,
    painted: Option<PaintedMetrics>,
}

impl FormHost {
    /// An empty, unfocused form with id `id`.
    pub fn new(id: &str) -> Self {
        Self {
            form: ScrolledForm::new(Form {
                id: quadraui::WidgetId::new(id),
                fields: Vec::new(),
                focused_field: None,
                scroll_offset: 0,
                has_focus: false,
                label_position: Default::default(),
                label_align: Default::default(),
                label_width: None,
                control_width: Default::default(),
                min_control_width: None,
                spacing: Default::default(),
            }),
            state: FormState::default(),
            painted: None,
        }
    }

    /// Replace the form with this frame's, scrolled to the engine's
    /// `scroll_offset`. The engine owns the scroll position: one the
    /// widget moved itself is read back with [`Self::scroll_offset`] after
    /// each event and fed in here on the next frame.
    pub fn set_form(&mut self, mut form: Form, scroll_offset: usize, has_focus: bool) {
        form.scroll_offset = scroll_offset;
        form.has_focus = has_focus;
        self.form = ScrolledForm::new(form);
        self.state.scroll_offset = None;
    }

    /// The form as the host last built it.
    pub fn form(&self) -> &Form {
        &self.form.form
    }

    /// The scroll offset the form shows, after any scrolling it did itself.
    pub fn scroll_offset(&self) -> usize {
        self.state.live_scroll_offset(&self.form.form)
    }

    /// Paint the form into `rect`, remembering `backend`'s metrics.
    pub fn paint(&mut self, backend: &mut dyn Backend, rect: Rect) {
        self.painted = Some(PaintedMetrics::of(&*backend));
        self.form.draw(&self.state, backend, rect);
    }

    /// Resolve one raw event against the form painted at `rect`, laid out
    /// with the metrics of the last paint. Keys reach the form only while
    /// it has focus, so an unfocused panel never takes them. `Ignored`
    /// until the form has been painted once.
    pub fn handle_cached(&mut self, event: &UiEvent, rect: Rect) -> Handled<FormEvent> {
        if !self.form.form.has_focus
            && matches!(
                event,
                UiEvent::KeyPressed { .. } | UiEvent::CharTyped(_) | UiEvent::ClipboardPaste(_)
            )
        {
            return Handled::Ignored;
        }
        if let UiEvent::MouseDown { position, .. } | UiEvent::DoubleClick { position, .. } = event {
            if !rect.contains(*position) {
                self.state.slider_drag = None;
                self.state.thumb_drag = None;
                return Handled::Ignored;
            }
        }
        let Some(m) = self.painted.as_ref() else {
            return Handled::Ignored;
        };
        let ctx = m.ctx();
        let (_, layout) =
            quadraui::lend_widget_state(self.form.form.id, self.state.clone(), || {
                quadraui::primitive::Primitive::layout(&self.form, &ctx, rect)
            });
        drive_laid_out(
            &self.form,
            &mut self.state,
            event,
            &layout,
            rect,
            ctx.units(),
        )
        .map_or(Handled::Ignored, |(handled, _)| handled)
    }
}
