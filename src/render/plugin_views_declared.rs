use super::*;
use crate::core::engine::PluginViewHost;

// ─── Plugin-declared UI views ───────────────────────────────────────────────

/// Parse a plugin-declared `date` field's ISO8601 `YYYY-MM-DD` string (an
/// optional leading `-` negates the year) into a `quadraui::CalendarDate`.
///
/// Returns `None` for an empty or malformed string — callers fall back to
/// an unset/default date rather than rejecting the whole view.
fn parse_iso_date(s: &str) -> Option<quadraui::CalendarDate> {
    let s = s.trim();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s),
    };
    // `splitn(3, '-')` yields at most three items, so there is no fourth
    // `parts.next()` to reject — a trailing `-<junk>` ends up inside the
    // `day` slice instead (e.g. `"15-99"`) and is rejected by `day`'s own
    // `parse::<u8>()` call above.
    let mut parts = rest.splitn(3, '-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let year = sign * year;
    if !(1..=quadraui::CalendarDate::days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some(quadraui::CalendarDate::new(year, month, day))
}

/// Adapt a plugin-declared [`crate::core::plugin_ui::PluginView`] into a
/// `quadraui::Form`.
///
/// This function translates between the vimcode-owned widget vocabulary
/// plugins author (see `core::plugin_ui`'s module doc) and the primitive
/// both backends paint. A breaking change to `quadraui::Form` is absorbed
/// here and nowhere else — no plugin has to be rewritten for it.
///
/// Widget ids are namespaced (`plugin:<view>:<field>`) so two plugins that
/// both name a button `"send"` cannot collide, and so
/// [`handle_plugin_view_ui_event`] can tell a plugin field from a Settings one.
///
/// `selected` / `scroll_top` / `has_focus` are vimcode-owned interaction state
/// (the existing `ext_panel_*` fields, or their editor-tab twins), *not*
/// plugin-declared: a re-render must not move the user's cursor.
///
/// `text_edit` is `Engine::plugin_view_text_edit` — when it names a field in
/// `view`, that field's *live* (possibly uncommitted) value/cursor/selection
/// paint instead of the plugin's declared value, with a real caret. Every
/// other field — including every field when `text_edit` is `None` — keeps
/// painting read-only, exactly as before: the plugin's declared value is
/// authoritative whenever vimcode isn't actively editing it.
pub fn plugin_view_to_form(
    view_name: &str,
    view: &crate::core::plugin_ui::PluginView,
    selected: usize,
    scroll_top: usize,
    has_focus: bool,
    text_edit: Option<&crate::core::plugin_ui::PluginViewTextEditState>,
) -> quadraui::Form {
    use crate::core::plugin_ui::{namespaced_widget_id, ViewFieldKind};
    use quadraui::{
        ButtonRowItem, Color, FieldKind, Form, FormField, StyledText, ToggleGroupItem, Toolbar,
        ToolbarButton, ValidationState, WidgetId,
    };

    let wid = |field: &str| WidgetId::new(namespaced_widget_id(view_name, field));
    let live = |field_id: &str| text_edit.filter(|e| e.view == view_name && e.field_id == field_id);

    let visible = view.visible_field_indices();
    let fields: Vec<FormField> = visible
        .iter()
        .map(|&i| {
            let f = &view.fields[i];
            let kind = match &f.kind {
                ViewFieldKind::Label => FieldKind::Label,
                ViewFieldKind::Text { value, placeholder } => {
                    // `cursor: None` renders read-only (no caret) — the
                    // plugin's own declared value, whenever vimcode isn't
                    // actively editing this field. `Some(live)` paints
                    // the live buffer with a real caret instead.
                    match live(&f.id) {
                        Some(e) => FieldKind::TextInput {
                            value: e.value.clone(),
                            placeholder: placeholder.clone(),
                            cursor: Some(e.cursor),
                            selection_anchor: e.selection_anchor,
                        },
                        None => FieldKind::TextInput {
                            value: value.clone(),
                            placeholder: placeholder.clone(),
                            cursor: None,
                            selection_anchor: None,
                        },
                    }
                }
                ViewFieldKind::Password { value, placeholder } => match live(&f.id) {
                    Some(e) => FieldKind::PasswordInput {
                        value: e.value.clone(),
                        placeholder: placeholder.clone(),
                        cursor: Some(e.cursor),
                        mask_char: '•',
                    },
                    None => FieldKind::PasswordInput {
                        value: value.clone(),
                        placeholder: placeholder.clone(),
                        cursor: None,
                        mask_char: '•',
                    },
                },
                ViewFieldKind::TextArea {
                    value,
                    placeholder,
                    rows,
                } => match live(&f.id) {
                    Some(e) => FieldKind::TextArea {
                        value: e.value.clone(),
                        placeholder: placeholder.clone(),
                        cursor: Some(e.cursor),
                        visible_rows: (*rows).max(1),
                    },
                    None => FieldKind::TextArea {
                        value: value.clone(),
                        placeholder: placeholder.clone(),
                        cursor: None,
                        visible_rows: (*rows).max(1),
                    },
                },
                ViewFieldKind::Toggle { value } => FieldKind::Toggle { value: *value },
                ViewFieldKind::Button => FieldKind::Button,
                ViewFieldKind::ReadOnly { value } => FieldKind::ReadOnly {
                    value: StyledText::plain(value.clone()),
                },
                ViewFieldKind::Dropdown { options, selected } => FieldKind::Dropdown {
                    options: options.iter().map(StyledText::plain).collect(),
                    selected_idx: (*selected).min(options.len().saturating_sub(1)),
                },
                ViewFieldKind::Segmented { options, selected } => FieldKind::SegmentedControl {
                    options: options.clone(),
                    selected_idx: (*selected).min(options.len().saturating_sub(1)),
                },
                ViewFieldKind::Buttons { buttons } => FieldKind::ButtonRow {
                    buttons: buttons
                        .iter()
                        .map(|b| ButtonRowItem {
                            id: wid(&b.id),
                            label: b.label.clone(),
                            disabled: b.disabled,
                            icon: None,
                        })
                        .collect(),
                },
                ViewFieldKind::Toggles { toggles } => FieldKind::ToggleGroup {
                    toggles: toggles
                        .iter()
                        .map(|t| ToggleGroupItem {
                            id: wid(&t.id),
                            label: t.label.clone(),
                            value: t.value,
                        })
                        .collect(),
                },
                ViewFieldKind::Date { value } => {
                    let parsed = parse_iso_date(value);
                    let mut picker =
                        quadraui::DatePicker::new(wid(&f.id), parsed.unwrap_or_default());
                    picker.selected = parsed;
                    // `picker.has_focus` is left at its `false` default:
                    // `quadraui::primitives::form`'s inline `DatePicker`
                    // row paint derives its focus affordance from
                    // `Form::has_focus` + `Form::focused_field` (set
                    // below), the same as every other field kind, not
                    // from the embedded `DatePicker`'s own flag.
                    FieldKind::DatePicker(picker)
                }
                ViewFieldKind::Slider {
                    value,
                    min,
                    max,
                    step,
                } => FieldKind::Slider {
                    value: *value,
                    min: *min,
                    max: *max,
                    step: *step,
                },
                // A collapsible header is a `Button` so activating it (click,
                // Enter/Space) raises `FormEvent::ButtonClicked` through
                // quadraui's existing machinery; `handle_plugin_view_ui_event`
                // and `handle_plugin_view_tab_ui_event` remap that decoded
                // event into `ViewEventKind::SectionToggled` below. A
                // non-collapsible header is inert, so it paints as a `Label`.
                ViewFieldKind::Section { collapsible, .. } => {
                    if *collapsible {
                        FieldKind::Button
                    } else {
                        FieldKind::Label
                    }
                }
                // Malformed hex falls back to black rather than failing the
                // whole form's render — the plugin author sees the wrong
                // swatch rather than a crash, and can fix the declared
                // value. There is no interaction path for this field yet
                // (see `ViewFieldKind::Color`'s doc), so this is display-only.
                ViewFieldKind::Color { value } => FieldKind::ColorPicker {
                    value: Color::from_hex(value).unwrap_or(Color::rgb(0, 0, 0)),
                },
                ViewFieldKind::Toolbar { buttons } => FieldKind::Toolbar(
                    Toolbar::new(wid(&f.id)).with_buttons(
                        buttons
                            .iter()
                            .map(|b| ToolbarButton::Action {
                                id: wid(&b.id),
                                label: b.label.clone(),
                                icon: b.icon.clone(),
                                key_hint: b.key_hint.clone(),
                                enabled: !b.disabled,
                                is_active: false,
                                tooltip: String::new(),
                            })
                            .collect(),
                    ),
                ),
            };
            // A collapsible section's label is prefixed with a disclosure
            // glyph reflecting its current state; every other field kind
            // (including non-collapsible sections) paints its label as-is.
            let label = match &f.kind {
                ViewFieldKind::Section {
                    collapsible: true,
                    collapsed,
                } => {
                    let glyph = if *collapsed { "▸" } else { "▾" };
                    StyledText::plain(format!("{glyph} {}", f.label))
                }
                _ => StyledText::plain(&f.label),
            };
            FormField {
                id: wid(&f.id),
                label,
                kind,
                hint: StyledText::plain(&f.hint),
                disabled: f.disabled,
                validation: match (&f.error, &f.warning) {
                    (Some(e), _) => Some(ValidationState::Error(e.clone())),
                    (None, Some(w)) => Some(ValidationState::Warning(w.clone())),
                    (None, None) => None,
                },
            }
        })
        .collect();

    // `selected` indexes `view.fields` (the logical/declaration order), not
    // `fields` (the painted/visible-only list) — a collapsed section's
    // members are absent from `fields` entirely. Find `selected`'s position
    // within `visible` first; `None` when `selected` itself is hidden.
    let focused_field = visible
        .iter()
        .position(|&i| i == selected)
        .and_then(|pos| fields.get(pos))
        .map(|f| f.id);

    Form {
        id: WidgetId::new(if view.id.is_empty() {
            format!("plugin-view-{view_name}")
        } else {
            crate::core::plugin_ui::namespaced_widget_id(view_name, &view.id)
        }),
        fields,
        focused_field,
        scroll_offset: scroll_top,
        has_focus,
    }
}

/// Populate `Engine::plugin_view_form_controller` from the active plugin view.
///
/// Returns `false` when the active sidebar panel is not a plugin view, so the
/// caller can fall through to the ordinary `ExtPanelItem` tree body. This is the
/// plugin-view twin of [`populate_settings_form_controller`].
pub fn populate_plugin_view_form_controller(engine: &Engine) -> bool {
    let Some(name) = engine.ext_panel_active.clone() else {
        return false;
    };
    let Some(view) = engine.plugin_views.get(&name) else {
        return false;
    };
    let form = plugin_view_to_form(
        &name,
        view,
        engine.ext_panel_selected,
        engine.ext_panel_scroll_top,
        engine.ext_panel_has_focus,
        engine.plugin_view_text_edit.as_ref(),
    );
    let mut fc = engine.plugin_view_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.ext_panel_scroll_top);
    fc.set_has_focus(engine.ext_panel_has_focus);
    true
}

/// Populate `Engine::plugin_view_tab_form_controller` from the named plugin
/// view — the editor-tab twin of [`populate_plugin_view_form_controller`],
/// which instead resolves the view name from `Engine::ext_panel_active` (the
/// sidebar's active panel) and reads `ext_panel_*` interaction state. A tab
/// has no activity-bar panel id to read the name from, so the caller
/// (`App::paint_editor_windows_rung`) passes it explicitly — the
/// `RenderedWindow::plugin_view` field set by `build_rendered_window`.
///
/// Returns `false` when `name` isn't a registered view.
pub fn populate_plugin_view_tab_form_controller(
    engine: &Engine,
    name: &str,
    has_focus: bool,
) -> bool {
    let Some(view) = engine.plugin_views.get(name) else {
        return false;
    };
    let form = plugin_view_to_form(
        name,
        view,
        engine.plugin_view_tab_selected,
        engine.plugin_view_tab_scroll_top,
        has_focus,
        engine.plugin_view_text_edit.as_ref(),
    );
    let mut fc = engine.plugin_view_tab_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.plugin_view_tab_scroll_top);
    fc.set_has_focus(has_focus);
    true
}

/// Route a pointer event over a plugin view through the shared
/// `quadraui::FormController` and dispatch whatever widget event it resolves to
/// the plugin's `on_event` callback.
///
/// `rect` must be the *same* rect the last frame passed to
/// `FormController::render_and_cache` (cached in
/// `Engine::plugin_view_form_rect`) — `handle_cached` re-derives its row layout
/// from it. Same contract, and the same reason, as
/// [`handle_settings_form_ui_event`].
///
/// Returns `true` when the event was consumed.
pub fn handle_plugin_view_ui_event(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};

    if !populate_plugin_view_form_controller(engine) {
        return false;
    }
    // `FormController` has no `DoubleClick` arm — probe with the equivalent
    // press (mirrors `handle_settings_form_ui_event`).
    let probe = match event {
        quadraui::UiEvent::DoubleClick { widget, position } => quadraui::UiEvent::MouseDown {
            widget: *widget,
            button: quadraui::MouseButton::Left,
            position: *position,
            modifiers: quadraui::Modifiers::default(),
        },
        other => other.clone(),
    };
    let result = engine
        .plugin_view_form_controller
        .borrow_mut()
        .handle_cached(&probe, rect);

    let sync_scroll = |engine: &mut Engine| {
        let offset = engine.plugin_view_form_controller.borrow().scroll_offset();
        engine.ext_panel_scroll_top = offset;
    };

    let action = match result {
        quadraui::FormControllerEvent::Ignored => return false,
        quadraui::FormControllerEvent::ScrollChanged | quadraui::FormControllerEvent::Consumed => {
            sync_scroll(engine);
            return true;
        }
        quadraui::FormControllerEvent::FormAction(action) => action,
    };
    sync_scroll(engine);

    let Some((view_name, field_id, mut kind)) = plugin_view_event_from_form_event(&action) else {
        return true;
    };
    kind = remap_section_toggle(engine, &view_name, &field_id, kind);
    // Selection follows the click, so j/k continues from where the user
    // clicked, and a click on a `Text`/`Password`/`TextArea` field primes it
    // for typing exactly as keyboard navigation does.
    if let Some(view) = engine.plugin_views.get(&view_name) {
        // `field_id` may name a sub-widget (a `buttons`/`toggles` entry) that is
        // not itself a row; only move the selection when it *is* a row.
        if let Some(idx) = view.field_index(&field_id) {
            engine.plugin_view_focus_field(
                &view_name,
                crate::core::engine::PluginViewHost::Sidebar,
                idx,
            );
        }
    }
    engine.ext_panel_has_focus = true;
    if matches!(kind, ViewEventKind::FocusChanged) {
        // A plain focus move is not something a plugin needs to hear about on
        // every click; it is reported only so a handler can track selection.
        // Keep it, but don't re-render for it beyond the selection change above.
        return true;
    }
    engine.dispatch_plugin_view_event(PluginViewEvent {
        view: view_name,
        widget_id: field_id,
        kind,
    });
    true
}

/// Route a pointer event over an editor-tab-hosted plugin view — the tab
/// twin of [`handle_plugin_view_ui_event`]. `name` is the view name
/// (`RenderedWindow::plugin_view`), since a tab has no `ext_panel_active` to
/// resolve it from; `rect` must be the same rect
/// `App::paint_editor_windows_rung` last painted this window's `Form` into
/// (cached in `Engine::plugin_view_tab_form_rect`).
///
/// Returns `true` when the event was consumed.
pub fn handle_plugin_view_tab_ui_event(
    engine: &mut Engine,
    name: &str,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};

    if !populate_plugin_view_tab_form_controller(engine, name, true) {
        return false;
    }
    let probe = match event {
        quadraui::UiEvent::DoubleClick { widget, position } => quadraui::UiEvent::MouseDown {
            widget: *widget,
            button: quadraui::MouseButton::Left,
            position: *position,
            modifiers: quadraui::Modifiers::default(),
        },
        other => other.clone(),
    };
    let result = engine
        .plugin_view_tab_form_controller
        .borrow_mut()
        .handle_cached(&probe, rect);

    let sync_scroll = |engine: &mut Engine| {
        let offset = engine
            .plugin_view_tab_form_controller
            .borrow()
            .scroll_offset();
        engine.plugin_view_tab_scroll_top = offset;
    };

    let action = match result {
        quadraui::FormControllerEvent::Ignored => return false,
        quadraui::FormControllerEvent::ScrollChanged | quadraui::FormControllerEvent::Consumed => {
            sync_scroll(engine);
            return true;
        }
        quadraui::FormControllerEvent::FormAction(action) => action,
    };
    sync_scroll(engine);

    let Some((view_name, field_id, mut kind)) = plugin_view_event_from_form_event(&action) else {
        return true;
    };
    kind = remap_section_toggle(engine, &view_name, &field_id, kind);
    if let Some(view) = engine.plugin_views.get(&view_name) {
        if let Some(idx) = view.field_index(&field_id) {
            engine.plugin_view_focus_field(
                &view_name,
                crate::core::engine::PluginViewHost::Tab,
                idx,
            );
        }
    }
    if matches!(kind, ViewEventKind::FocusChanged) {
        return true;
    }
    engine.dispatch_plugin_view_event(PluginViewEvent {
        view: view_name,
        widget_id: field_id,
        kind,
    });
    true
}

/// Route a `UiEvent` over a body-kind (`List`/`Tree`/`Table`/`TextView`)
/// plugin view at `host`, through the matching primitive's click/scroll
/// resolution instead of `FormController`. `rect` is the rect the
/// view was last painted into for this host — only the `Tree` arm needs it
/// live (a `TreeController` needs a real rect + backend to `handle` against);
/// `List`/`Table` resolve through their own cached layouts
/// (`route_plugin_view_list_click`/`route_plugin_view_table_click`) instead.
///
/// Shared by both hosts: the sidebar's `ExtPanel` click router and
/// [`handle_plugin_view_tab_ui_event`]'s caller both dispatch here first —
/// see `paint_sidebar_panel_rung`'s `ext:` arm and `paint_editor_windows_
/// rung`'s tab arm for the matching *paint*-side kind branch.
///
/// Returns `None` when `name` isn't a body-kind view at all (i.e. it's a
/// field-stack `Form`), so the caller falls back to the `Form` routing path;
/// `Some(bool)` otherwise, `true` when the event was consumed.
pub(crate) fn route_plugin_view_body_event(
    engine: &mut Engine,
    name: &str,
    host: PluginViewHost,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) -> Option<bool> {
    let kind = engine
        .plugin_views
        .get(name)
        .and_then(|v| v.body.as_ref())
        .map(|b| b.kind_name());
    // Positive `delta.y` = scroll content up (toward the top) — the same
    // convention the sidebar's fallback `Scroll` arm documents.
    let scroll_step = |delta: &quadraui::ScrollDelta| -> i32 {
        let step = (delta.y.abs() * 3.0).round().max(1.0) as i32;
        if delta.y > 0.0 {
            -step
        } else {
            step
        }
    };
    match kind {
        Some("list") => Some(match event {
            quadraui::UiEvent::MouseDown {
                position,
                button: quadraui::MouseButton::Left,
                ..
            } => route_plugin_view_list_click(engine, name, host, *position, false),
            quadraui::UiEvent::DoubleClick { position, .. } => {
                route_plugin_view_list_click(engine, name, host, *position, true)
            }
            quadraui::UiEvent::Scroll { delta, .. } => {
                scroll_plugin_view_flat_selection_list(engine, name, host, scroll_step(delta))
            }
            _ => false,
        }),
        Some("tree") => Some(route_plugin_view_tree_event(
            engine, name, host, true, event, rect, backend,
        )),
        Some("table") => Some(match event {
            quadraui::UiEvent::MouseDown {
                position,
                button: quadraui::MouseButton::Left,
                ..
            } => route_plugin_view_table_click(engine, name, host, *position, false),
            quadraui::UiEvent::DoubleClick { position, .. } => {
                route_plugin_view_table_click(engine, name, host, *position, true)
            }
            quadraui::UiEvent::Scroll { delta, .. } => {
                scroll_plugin_view_flat_selection_table(engine, name, host, scroll_step(delta))
            }
            // A header-divider drag armed by `route_plugin_view_table_click`
            // is not geometry-bound to `rect` — the pointer can stray
            // outside it mid-drag without the drag ending — so these two
            // arms consult `Engine::plugin_view_table_resize_drag` directly
            // rather than re-hit-testing `position` against `rect`.
            quadraui::UiEvent::MouseMoved { position, .. } => {
                continue_plugin_view_table_resize_drag(engine, name, host, *position, backend)
            }
            quadraui::UiEvent::MouseUp { .. } => {
                end_plugin_view_table_resize_drag(engine, name, host)
            }
            _ => false,
        }),
        Some("text_view") => Some(match event {
            quadraui::UiEvent::Scroll { delta, .. } => {
                scroll_plugin_view_text(engine, name, host, scroll_step(delta))
            }
            _ => false,
        }),
        _ => None,
    }
}

/// A collapsible `Section` header is translated to a `FieldKind::Button` (see
/// [`plugin_view_to_form`]), so activating it decodes as an ordinary
/// `ViewEventKind::ButtonClicked` through [`plugin_view_event_from_form_event`].
/// Re-resolve `field_id` against the live view and, when it names a `Section`,
/// remap the event into `ViewEventKind::SectionToggled` carrying the proposed
/// next state — the toggle plugin authors actually want to hear about.
/// Every other decoded `kind` (including `ButtonClicked` for a real button)
/// passes through unchanged.
fn remap_section_toggle(
    engine: &Engine,
    view_name: &str,
    field_id: &str,
    kind: crate::core::plugin_ui::ViewEventKind,
) -> crate::core::plugin_ui::ViewEventKind {
    use crate::core::plugin_ui::{ViewEventKind, ViewFieldKind};
    if !matches!(kind, ViewEventKind::ButtonClicked) {
        return kind;
    }
    let Some(view) = engine.plugin_views.get(view_name) else {
        return kind;
    };
    let Some(idx) = view.field_index(field_id) else {
        return kind;
    };
    match &view.fields[idx].kind {
        ViewFieldKind::Section { collapsed, .. } => ViewEventKind::SectionToggled {
            collapsed: !collapsed,
        },
        _ => kind,
    }
}

/// Decompose a `quadraui::FormEvent` into `(view, plugin-authored field id,
/// event kind)`, or `None` when the event's widget is not in the plugin
/// namespace (a Settings field, say) or carries no plugin meaning.
///
/// Kept separate from [`handle_plugin_view_ui_event`] so the mapping is unit
/// testable without a `FormController`.
pub(crate) fn plugin_view_event_from_form_event(
    action: &quadraui::FormEvent,
) -> Option<(String, String, crate::core::plugin_ui::ViewEventKind)> {
    use crate::core::plugin_ui::{split_widget_id, ViewEventKind};
    let (id, kind) = match action {
        quadraui::FormEvent::ButtonClicked { id } => (id, ViewEventKind::ButtonClicked),
        quadraui::FormEvent::ToggleChanged { id, value } => {
            (id, ViewEventKind::ToggleChanged { value: *value })
        }
        quadraui::FormEvent::DropdownChanged { id, selected_idx } => (
            id,
            ViewEventKind::DropdownChanged {
                selected: *selected_idx,
            },
        ),
        quadraui::FormEvent::SegmentedControlChanged { id, selected_idx } => (
            id,
            ViewEventKind::SegmentedChanged {
                selected: *selected_idx,
            },
        ),
        quadraui::FormEvent::TextInputChanged { id, value } => (
            id,
            ViewEventKind::TextChanged {
                value: value.clone(),
            },
        ),
        quadraui::FormEvent::TextInputCommitted { id, value } => (
            id,
            ViewEventKind::TextCommitted {
                value: value.clone(),
            },
        ),
        quadraui::FormEvent::DateChanged { id, value } => (
            id,
            ViewEventKind::DateChanged {
                value: value.format(quadraui::DateFormat::Iso8601),
            },
        ),
        quadraui::FormEvent::SliderChanged { id, value } => {
            (id, ViewEventKind::SliderChanged { value: *value })
        }
        quadraui::FormEvent::FocusChanged { id } => (id, ViewEventKind::FocusChanged),
        // The clicked button's own id is the dispatched event's widget id —
        // not the toolbar field's — matching the keyboard-activation path in
        // `Engine::handle_plugin_view_key`.
        quadraui::FormEvent::ToolbarButtonClicked {
            field_id,
            button_id,
        } => {
            let (view, _) = split_widget_id(field_id.as_str())?;
            let (_, button) = split_widget_id(button_id.as_str())?;
            return Some((
                view.to_string(),
                button.to_string(),
                ViewEventKind::ToolbarButtonClicked,
            ));
        }
        _ => return None,
    };
    let (view, field) = split_widget_id(id.as_str())?;
    Some((view.to_string(), field.to_string(), kind))
}

#[cfg(test)]
mod date_parse_tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_iso8601_date() {
        let d = parse_iso_date("2024-01-02").expect("well-formed date must parse");
        assert_eq!(d, quadraui::CalendarDate::new(2024, 1, 2));
    }

    #[test]
    fn parses_a_negative_year() {
        let d = parse_iso_date("-0044-03-15").expect("BC year must parse");
        assert_eq!(d, quadraui::CalendarDate::new(-44, 3, 15));
    }

    #[test]
    fn rejects_an_empty_string() {
        assert_eq!(parse_iso_date(""), None);
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse_iso_date("not-a-date"), None);
        assert_eq!(parse_iso_date("2024/01/02"), None);
        assert_eq!(parse_iso_date("2024-01"), None);
    }

    #[test]
    fn rejects_an_out_of_range_month() {
        assert_eq!(parse_iso_date("2024-00-01"), None);
        assert_eq!(parse_iso_date("2024-13-01"), None);
    }

    #[test]
    fn rejects_february_30th() {
        assert_eq!(parse_iso_date("2024-02-30"), None);
    }

    #[test]
    fn accepts_february_29th_in_a_leap_year() {
        let d = parse_iso_date("2024-02-29").expect("2024 is a leap year");
        assert_eq!(d, quadraui::CalendarDate::new(2024, 2, 29));
    }

    #[test]
    fn rejects_february_29th_in_a_non_leap_year() {
        assert_eq!(parse_iso_date("2023-02-29"), None);
    }

    #[test]
    fn rejects_a_zero_day() {
        assert_eq!(parse_iso_date("2024-01-00"), None);
    }

    #[test]
    fn rejects_trailing_junk_after_the_day() {
        assert_eq!(parse_iso_date("2024-01-15-99"), None);
    }
}

#[cfg(test)]
mod slider_translation_tests {
    use super::*;
    use crate::core::plugin_ui::{PluginView, ViewField, ViewFieldKind};

    #[test]
    fn slider_field_translates_to_the_quadraui_slider_field_kind() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![ViewField {
                id: "vol".into(),
                label: "Volume".into(),
                hint: String::new(),
                disabled: false,
                error: None,
                warning: None,
                kind: ViewFieldKind::Slider {
                    value: 3.0,
                    min: 0.0,
                    max: 10.0,
                    step: 0.5,
                },
            }],
            body: None,
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(
            form.fields[0].kind,
            quadraui::FieldKind::Slider {
                value: 3.0,
                min: 0.0,
                max: 10.0,
                step: 0.5,
            }
        );
    }

    #[test]
    fn form_slider_changed_event_translates_back_to_the_plugin_event() {
        let id = quadraui::WidgetId::new(crate::core::plugin_ui::namespaced_widget_id(
            "myview", "vol",
        ));
        let action = quadraui::FormEvent::SliderChanged { id, value: 7.5 };
        let (view, field, kind) =
            plugin_view_event_from_form_event(&action).expect("must map back");
        assert_eq!(view, "myview");
        assert_eq!(field, "vol");
        assert_eq!(
            kind,
            crate::core::plugin_ui::ViewEventKind::SliderChanged { value: 7.5 }
        );
    }
}

#[cfg(test)]
mod section_translation_tests {
    use super::*;
    use crate::core::plugin_ui::{PluginView, ViewField, ViewFieldKind};

    fn section_field(id: &str, label: &str, collapsible: bool, collapsed: bool) -> ViewField {
        ViewField {
            id: id.into(),
            label: label.into(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind: ViewFieldKind::Section {
                collapsible,
                collapsed,
            },
        }
    }

    fn plain_text(text: &quadraui::StyledText) -> String {
        text.spans.iter().map(|s| s.text.as_str()).collect()
    }

    fn button_field(id: &str) -> ViewField {
        ViewField {
            id: id.into(),
            label: String::new(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind: ViewFieldKind::Button,
        }
    }

    #[test]
    fn collapsible_section_translates_to_a_button_with_a_collapsed_glyph() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![section_field("adv", "Advanced", true, true)],
            body: None,
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(form.fields[0].kind, quadraui::FieldKind::Button);
        assert_eq!(plain_text(&form.fields[0].label), "▸ Advanced");
    }

    #[test]
    fn expanded_collapsible_section_paints_the_expanded_glyph() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![section_field("adv", "Advanced", true, false)],
            body: None,
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(plain_text(&form.fields[0].label), "▾ Advanced");
    }

    #[test]
    fn non_collapsible_section_translates_to_an_inert_label_with_no_glyph() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![section_field("grp", "Group", false, false)],
            body: None,
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(form.fields[0].kind, quadraui::FieldKind::Label);
        assert_eq!(plain_text(&form.fields[0].label), "Group");
    }

    #[test]
    fn collapsed_section_members_are_absent_from_the_painted_form() {
        // `hidden` is a member of the collapsed `adv` section; the next
        // `Section` header (`more`) closes that membership run, so `after`
        // (a member of `more`, which is expanded) stays visible.
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![
                section_field("adv", "Advanced", true, true),
                button_field("hidden"),
                section_field("more", "More", true, false),
                button_field("after"),
            ],
            body: None,
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(form.fields.len(), 3);
        assert_eq!(
            form.fields[2].id,
            quadraui::WidgetId::new(crate::core::plugin_ui::namespaced_widget_id(
                "myview", "after"
            ))
        );
    }

    #[test]
    fn focused_field_is_none_when_selected_points_at_a_hidden_member() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![
                section_field("adv", "Advanced", true, true),
                button_field("hidden"),
            ],
            body: None,
        };
        // `selected == 1` names the hidden member — the painted form has no
        // field to focus.
        let form = plugin_view_to_form("myview", &view, 1, 0, true, None);
        assert_eq!(form.focused_field, None);
    }

    #[test]
    fn focused_field_follows_selected_through_a_collapsed_section() {
        // `after` sits beyond the `more` header, outside `adv`'s (collapsed)
        // membership run, so `selected` pointing at it must resolve to a
        // focused field even though `adv`'s own member (`hidden`) is hidden.
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![
                section_field("adv", "Advanced", true, true),
                button_field("hidden"),
                section_field("more", "More", true, false),
                button_field("after"),
            ],
            body: None,
        };
        let form = plugin_view_to_form("myview", &view, 3, 0, true, None);
        assert_eq!(
            form.focused_field,
            Some(quadraui::WidgetId::new(
                crate::core::plugin_ui::namespaced_widget_id("myview", "after")
            ))
        );
    }

    #[test]
    fn button_clicked_on_a_collapsed_section_header_remaps_to_section_toggled_expand() {
        use crate::core::plugin_ui::ViewEventKind;
        let mut engine = Engine::new();
        engine.plugin_views.insert(
            "myview".to_string(),
            PluginView {
                id: "myview".into(),
                schema_version: 1,
                fields: vec![section_field("adv", "Advanced", true, true)],
                body: None,
            },
        );
        let kind = remap_section_toggle(&engine, "myview", "adv", ViewEventKind::ButtonClicked);
        assert_eq!(kind, ViewEventKind::SectionToggled { collapsed: false });
    }

    #[test]
    fn button_clicked_on_an_expanded_section_header_remaps_to_section_toggled_collapse() {
        use crate::core::plugin_ui::ViewEventKind;
        let mut engine = Engine::new();
        engine.plugin_views.insert(
            "myview".to_string(),
            PluginView {
                id: "myview".into(),
                schema_version: 1,
                fields: vec![section_field("adv", "Advanced", true, false)],
                body: None,
            },
        );
        let kind = remap_section_toggle(&engine, "myview", "adv", ViewEventKind::ButtonClicked);
        assert_eq!(kind, ViewEventKind::SectionToggled { collapsed: true });
    }

    #[test]
    fn button_clicked_on_a_real_button_passes_through_unchanged() {
        use crate::core::plugin_ui::ViewEventKind;
        let mut engine = Engine::new();
        engine.plugin_views.insert(
            "myview".to_string(),
            PluginView {
                id: "myview".into(),
                schema_version: 1,
                fields: vec![button_field("ok")],
                body: None,
            },
        );
        let kind = remap_section_toggle(&engine, "myview", "ok", ViewEventKind::ButtonClicked);
        assert_eq!(kind, ViewEventKind::ButtonClicked);
    }

    #[test]
    fn clicking_a_collapsible_section_header_decodes_as_button_clicked_before_remap() {
        use crate::core::plugin_ui::ViewEventKind;
        let id = quadraui::WidgetId::new(crate::core::plugin_ui::namespaced_widget_id(
            "myview", "adv",
        ));
        let action = quadraui::FormEvent::ButtonClicked { id };
        let (_view, _field, kind) =
            plugin_view_event_from_form_event(&action).expect("must decode");
        assert_eq!(kind, ViewEventKind::ButtonClicked);
    }
}
