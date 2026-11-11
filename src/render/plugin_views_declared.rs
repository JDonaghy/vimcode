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

/// Resolve a plugin-declared `icon` string (already validated at Lua-parse
/// time by `plugin::lua_codicon_name`) into a real `quadraui::Icon`. A known
/// codicon name resolves to its glyph; the `unwrap_or_else` fallback paints
/// the string verbatim — the only route a validated value takes there is the
/// legacy single-character literal glyph `lua_codicon_name` accepts
/// unvalidated (e.g. `"⟳"`, `"+"`), so it is reachable, not dead.
pub(super) fn resolve_view_icon(name: &str) -> quadraui::Icon {
    quadraui::Icon::codicon(name, name).unwrap_or_else(|| quadraui::Icon::new(name, name))
}

/// Build the `quadraui::ButtonSpec` override for a button-shaped field/item
/// that declared at least one of `variant`/`icon`/`tooltip` — `None` when
/// none of the three are set, so a plugin that never opts in gets `spec:
/// None` and today's plain rendering, exactly as before this vocabulary
/// existed.
fn resolve_button_spec(
    label: &str,
    variant: Option<crate::core::plugin_ui::ViewButtonVariant>,
    icon: Option<&quadraui::Icon>,
    tooltip: &Option<String>,
    disabled: bool,
) -> Option<quadraui::ButtonSpec> {
    if variant.is_none() && icon.is_none() && tooltip.is_none() {
        return None;
    }
    let mut spec = quadraui::ButtonSpec::new(label.to_string());
    if let Some(v) = variant {
        spec.variant = v.into_quadraui();
    }
    spec.icon = icon.cloned();
    spec.tooltip = tooltip.clone();
    spec.disabled = disabled;
    Some(spec)
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
    plugin_view_to_form_with_combo_edit(
        view_name, view, selected, scroll_top, has_focus, text_edit, None,
    )
}

/// [`plugin_view_to_form`] plus a `combo_edit` parameter for the
/// `combo_box` field kind's own live interaction state (`text`/`open`/
/// `highlighted`) — see [`crate::core::plugin_ui::PluginViewComboBoxEditState`]'s
/// doc for why it needs one, unlike every other field kind here. A separate
/// function rather than a seventh parameter on [`plugin_view_to_form`]
/// itself so every pre-existing call site (test fixtures with no
/// `combo_box` field in play) keeps compiling unchanged.
pub fn plugin_view_to_form_with_combo_edit(
    view_name: &str,
    view: &crate::core::plugin_ui::PluginView,
    selected: usize,
    scroll_top: usize,
    has_focus: bool,
    text_edit: Option<&crate::core::plugin_ui::PluginViewTextEditState>,
    combo_edit: Option<&crate::core::plugin_ui::PluginViewComboBoxEditState>,
) -> quadraui::Form {
    plugin_view_to_form_with_row_focus(
        view_name, view, selected, scroll_top, has_focus, text_edit, combo_edit, 0,
    )
}

/// [`plugin_view_to_form_with_combo_edit`] plus `row_focus`: which item,
/// within `selected`'s own `buttons`/`toggles` list, the row's keyboard
/// sub-focus is on — `Engine::ext_panel_row_focus` (or its tab/dock twin).
/// Ignored unless `selected` both has keyboard focus (`has_focus`) and is a
/// `Buttons`/`Toggles` field; a separate function rather than an eighth
/// parameter on [`plugin_view_to_form_with_combo_edit`] itself, for the same
/// "pre-existing call sites keep compiling unchanged" reason that function's
/// own doc gives for not growing [`plugin_view_to_form`] directly.
///
/// quadraui 0.9's `FieldKind::ButtonRow`/`ToggleGroup` carry no per-item
/// focus field of their own — unlike `FieldKind::Toolbar`'s
/// `Toolbar::focused_index`, which a host sets and the primitive's own
/// paint path renders with a distinct highlight. Rather than adding one
/// (a quadraui-side change this file can't make), the sub-focused item's
/// own [`quadraui::ButtonRowItem::label`] / [`quadraui::ToggleGroupItem::
/// label`] is wrapped in `‹` `›` here instead: plain text every backend's
/// existing `ButtonRow`/`ToggleGroup` paint path already draws verbatim,
/// so the highlight needs no new backend-specific rendering code to show
/// up on every one of them identically. `‹…›` rather than `[…]`: a
/// `buttons` row's own `ButtonRowItem` already paints bracket chrome
/// around every item's label regardless of focus, so `[…]` here would
/// nest into an ambiguous `[[Label]]` instead of standing out.
#[allow(clippy::too_many_arguments)]
pub fn plugin_view_to_form_with_row_focus(
    view_name: &str,
    view: &crate::core::plugin_ui::PluginView,
    selected: usize,
    scroll_top: usize,
    has_focus: bool,
    text_edit: Option<&crate::core::plugin_ui::PluginViewTextEditState>,
    combo_edit: Option<&crate::core::plugin_ui::PluginViewComboBoxEditState>,
    row_focus: usize,
) -> quadraui::Form {
    use crate::core::plugin_ui::{namespaced_widget_id, ViewFieldKind};
    use quadraui::{
        ButtonRowItem, Color, FieldKind, Form, FormField, StyledText, ToggleGroupItem, Toolbar,
        ToolbarButton, ValidationState, WidgetId,
    };

    let wid = |field: &str| WidgetId::new(namespaced_widget_id(view_name, field));
    let live = |field_id: &str| text_edit.filter(|e| e.view == view_name && e.field_id == field_id);
    let live_combo =
        |field_id: &str| combo_edit.filter(|e| e.view == view_name && e.field_id == field_id);

    let visible = view.visible_field_indices();
    let fields: Vec<FormField> = visible
        .iter()
        .map(|&i| {
            let f = &view.fields[i];
            // Whether `row_focus` names an item in *this* field's own
            // `buttons`/`toggles` list that the keyboard will activate next
            // — see `plugin_view_to_form_with_row_focus`'s doc for why that
            // item's label gets marked below rather than a native per-item
            // focus flag.
            let row_focused = has_focus && i == selected;
            let mark_row_focused = |label: String, idx: usize| {
                if row_focused && idx == row_focus {
                    format!("‹{label}›")
                } else {
                    label
                }
            };
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
                // quadraui 0.9's standalone `FieldKind::Button` has no
                // `ButtonSpec` hook at all (unlike `ButtonRowItem`/
                // `ToolbarButton::Action` below) — a field that didn't
                // declare `variant`/`icon` keeps painting through it
                // unchanged. One that did is translated to a single-item
                // `FieldKind::ButtonRow` instead, the only standalone-row
                // kind that does carry a spec; the field's own label is
                // blanked below so the caption paints once, inside that one
                // item, not twice. `tooltip` alone does *not* trigger the
                // reroute: no backend paints a form-row tooltip today, so a
                // plugin that sets only `tooltip` would pay for the layout
                // change (quadraui's right-anchored `< label >` box becomes a
                // flush-left `[ label ]`) for zero visual benefit, and its
                // `tooltip` is simply dropped rather than attached to a spec.
                ViewFieldKind::Button { variant, icon, .. }
                    if variant.is_none() && icon.is_none() =>
                {
                    FieldKind::Button
                }
                ViewFieldKind::Button {
                    variant,
                    icon,
                    tooltip,
                } => {
                    let resolved_icon = icon.as_deref().map(resolve_view_icon);
                    let spec = resolve_button_spec(
                        &f.label,
                        *variant,
                        resolved_icon.as_ref(),
                        tooltip,
                        f.disabled,
                    );
                    FieldKind::ButtonRow {
                        buttons: vec![ButtonRowItem {
                            id: wid(&f.id),
                            label: f.label.clone(),
                            disabled: f.disabled,
                            icon: resolved_icon,
                            spec,
                        }],
                    }
                }
                ViewFieldKind::ReadOnly { value } => FieldKind::ReadOnly {
                    value: StyledText::plain(value.clone()),
                },
                ViewFieldKind::Dropdown { options, selected } => FieldKind::Dropdown {
                    options: options.iter().map(StyledText::plain).collect(),
                    selected_idx: (*selected).min(options.len().saturating_sub(1)),
                },
                // `placeholder` has no `quadraui::ComboBox` field to land in
                // (unlike `TextInput`/`TextArea`'s own `placeholder`) — kept
                // on the Lua-facing vocabulary for parity with every other
                // text-backed field kind, but it paints nothing yet; see
                // `ViewFieldKind::ComboBox`'s own doc. `committed` seeds from
                // the plugin's declared `value` every render; `text`/`open`/
                // `highlighted` come from `combo_edit` whenever it names this
                // field (an edit in progress), falling back to the declared
                // `value` with the dropdown closed otherwise — the same
                // "declared value is authoritative unless vimcode is actively
                // editing it" contract `text_edit`/`live` already follow for
                // `Text`/`Password`/`TextArea`.
                ViewFieldKind::ComboBox { options, value, .. } => {
                    let mut combo = quadraui::ComboBox::new(wid(&f.id), options.clone());
                    combo.committed = value.clone();
                    combo.disabled = f.disabled;
                    match live_combo(&f.id) {
                        Some(e) => {
                            combo.text = e.text.clone();
                            combo.open = e.open;
                            combo.highlighted = e.highlighted;
                        }
                        None => combo.text = value.clone(),
                    }
                    FieldKind::ComboBox(combo)
                }
                ViewFieldKind::Segmented { options, selected } => FieldKind::SegmentedControl {
                    options: options.clone(),
                    selected_idx: (*selected).min(options.len().saturating_sub(1)),
                },
                ViewFieldKind::Buttons { buttons } => FieldKind::ButtonRow {
                    buttons: buttons
                        .iter()
                        .enumerate()
                        .map(|(idx, b)| {
                            let label = mark_row_focused(b.label.clone(), idx);
                            let resolved_icon = b.icon.as_deref().map(resolve_view_icon);
                            let spec = resolve_button_spec(
                                &label,
                                b.variant,
                                resolved_icon.as_ref(),
                                &b.tooltip,
                                b.disabled,
                            );
                            ButtonRowItem {
                                id: wid(&b.id),
                                label,
                                disabled: b.disabled,
                                icon: resolved_icon,
                                spec,
                            }
                        })
                        .collect(),
                },
                ViewFieldKind::Toggles { toggles } => FieldKind::ToggleGroup {
                    toggles: toggles
                        .iter()
                        .enumerate()
                        .map(|(idx, t)| ToggleGroupItem {
                            id: wid(&t.id),
                            label: mark_row_focused(t.label.clone(), idx),
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
                // A valid hex string paints a swatch. A malformed or empty
                // one falls back to a read-only row echoing the raw string
                // rather than failing the whole form's render — the plugin
                // author sees the bad value they typed instead of a crash,
                // and can fix it. Form building runs without a theme in
                // scope, so there is no colour to borrow for a swatch
                // fallback. There is no interaction path for this field yet
                // (see `ViewFieldKind::Color`'s doc), so this is display-only.
                ViewFieldKind::Color { value } => match Color::from_hex(value) {
                    Some(color) => FieldKind::ColorPicker { value: color },
                    None => FieldKind::ReadOnly {
                        value: StyledText::plain(value),
                    },
                },
                ViewFieldKind::Toolbar { buttons } => FieldKind::Toolbar(
                    Toolbar::new(wid(&f.id)).with_buttons(
                        buttons
                            .iter()
                            .map(|b| {
                                let resolved_icon = b.icon.as_deref().map(resolve_view_icon);
                                let spec = resolve_button_spec(
                                    &b.label,
                                    b.variant,
                                    resolved_icon.as_ref(),
                                    &b.tooltip,
                                    b.disabled,
                                );
                                ToolbarButton::Action {
                                    id: wid(&b.id),
                                    label: b.label.clone(),
                                    // Legacy glyph field: painted verbatim
                                    // on every backend (see
                                    // `ToolbarButton::Action::icon`'s own
                                    // doc) — this Form-embedded toolbar
                                    // paint path has no nerd-font-aware
                                    // glyph swap of its own, so the
                                    // resolved codicon's ASCII fallback is
                                    // what actually renders, on every
                                    // backend, regardless of `spec.icon`.
                                    icon: resolved_icon.as_ref().map(|i| i.fallback.clone()),
                                    key_hint: b.key_hint.clone(),
                                    enabled: !b.disabled,
                                    is_active: false,
                                    tooltip: b.tooltip.clone().unwrap_or_default(),
                                    spec,
                                }
                            })
                            .collect(),
                    ),
                ),
            };
            // A collapsible section's label is prefixed with a disclosure
            // glyph reflecting its current state. A `Button` field that
            // declared `variant`/`icon`/`tooltip` has its caption moved
            // into the single `ButtonRow` item the `kind` match above
            // built instead (see that arm's doc) — the field's own label
            // column must stay blank or the caption would paint twice.
            // Every other field kind (including a plain `Button` and a
            // non-collapsible section) paints its label as-is.
            let label = match &f.kind {
                ViewFieldKind::Section {
                    collapsible: true,
                    collapsed,
                } => {
                    let glyph = if *collapsed { "▸" } else { "▾" };
                    StyledText::plain(format!("{glyph} {}", f.label))
                }
                ViewFieldKind::Button { variant, icon, .. }
                    if variant.is_some() || icon.is_some() =>
                {
                    StyledText::plain("")
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
                label_position: f.label_position.map(|p| p.into_quadraui()),
                label_align: f.label_align.map(|a| a.into_quadraui()),
                label_width: f.label_width,
                control_width: f.control_width.map(|w| w.into_quadraui()),
                min_control_width: f.min_control_width,
                tab_index: None,
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
        label_position: view
            .label_position
            .map(|p| p.into_quadraui())
            .unwrap_or_default(),
        label_align: view
            .label_align
            .map(|a| a.into_quadraui())
            .unwrap_or_default(),
        label_width: view.label_width,
        control_width: view
            .control_width
            .map(|w| w.into_quadraui())
            .unwrap_or_default(),
        min_control_width: view.min_control_width,
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
    let form = plugin_view_to_form_with_row_focus(
        &name,
        view,
        engine.ext_panel_selected,
        engine.ext_panel_scroll_top,
        engine.ext_panel_has_focus,
        engine.plugin_view_text_edit.as_ref(),
        engine.plugin_view_combo_edit.as_ref(),
        engine.plugin_view_row_focus(PluginViewHost::Sidebar),
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
    let form = plugin_view_to_form_with_row_focus(
        name,
        view,
        engine.plugin_view_tab_selected,
        engine.plugin_view_tab_scroll_top,
        has_focus,
        engine.plugin_view_text_edit.as_ref(),
        engine.plugin_view_combo_edit.as_ref(),
        engine.plugin_view_row_focus(PluginViewHost::Tab),
    );
    let mut fc = engine.plugin_view_tab_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.plugin_view_tab_scroll_top);
    fc.set_has_focus(has_focus);
    true
}

/// Populate `Engine::plugin_view_dock_form_controller` from the named plugin
/// view — the bottom-dock twin of [`populate_plugin_view_tab_form_controller`],
/// reading `plugin_view_dock_*` interaction state instead of `_tab_*`. The
/// caller is `paint_bottom_panel_rung`, which resolves the view name from
/// `BottomPanelKind::Plugin`.
///
/// Returns `false` when `name` isn't a registered view.
pub fn populate_plugin_view_dock_form_controller(
    engine: &Engine,
    name: &str,
    has_focus: bool,
) -> bool {
    let Some(view) = engine.plugin_views.get(name) else {
        return false;
    };
    let form = plugin_view_to_form_with_row_focus(
        name,
        view,
        engine.plugin_view_dock_selected,
        engine.plugin_view_dock_scroll_top,
        has_focus,
        engine.plugin_view_text_edit.as_ref(),
        engine.plugin_view_combo_edit.as_ref(),
        engine.plugin_view_row_focus(PluginViewHost::Dock),
    );
    let mut fc = engine.plugin_view_dock_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.plugin_view_dock_scroll_top);
    fc.set_has_focus(has_focus);
    true
}

/// Populate `Engine::plugin_view_modal_form_controller` from the named
/// plugin view — the modal twin of [`populate_plugin_view_dock_form_controller`],
/// reading `plugin_view_modal_*` interaction state. The caller is the shell's
/// `FrameOp::PluginViewModal` rung, which resolves the view name from
/// `Engine::plugin_view_modal`.
///
/// Returns `false` when `name` isn't a registered view.
pub fn populate_plugin_view_modal_form_controller(engine: &Engine, name: &str) -> bool {
    let Some(view) = engine.plugin_views.get(name) else {
        return false;
    };
    let form = plugin_view_to_form_with_row_focus(
        name,
        view,
        engine.plugin_view_modal_selected,
        engine.plugin_view_modal_scroll_top,
        true,
        engine.plugin_view_text_edit.as_ref(),
        engine.plugin_view_combo_edit.as_ref(),
        engine.plugin_view_row_focus(PluginViewHost::Modal),
    );
    let mut fc = engine.plugin_view_modal_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.plugin_view_modal_scroll_top);
    fc.set_has_focus(true);
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
    apply_combo_box_form_action(engine, &action);

    let Some((view_name, field_id, mut kind)) = plugin_view_event_from_form_event(&action) else {
        return true;
    };
    kind = remap_section_toggle(engine, &view_name, &field_id, kind);
    kind = remap_styled_button_focus(engine, &view_name, &field_id, kind);

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
    apply_combo_box_form_action(engine, &action);

    let Some((view_name, field_id, mut kind)) = plugin_view_event_from_form_event(&action) else {
        return true;
    };
    kind = remap_section_toggle(engine, &view_name, &field_id, kind);
    kind = remap_styled_button_focus(engine, &view_name, &field_id, kind);

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

/// Route a pointer event over a bottom-panel-docked plugin view — the dock
/// twin of [`handle_plugin_view_tab_ui_event`]. `name` is the view name
/// (`BottomPanelKind::Plugin`'s payload); `rect` must be the same rect
/// `paint_bottom_panel_rung`'s `Plugin` arm last painted this view's `Form`
/// into (cached in `Engine::plugin_view_dock_form_rect`).
///
/// Only called for `Form`-bodied views — `route_bottom_panel_click` routes a
/// click here only once it has confirmed `name`'s body is the field-stack
/// default, since `List`/`Tree`/`Table` have no dock-hosted layout cache and
/// `TextView`'s only interaction is the mouse-wheel scroll already wired via
/// `Engine::handle_plugin_view_dock_scroll`.
///
/// Returns `true` when the event was consumed.
pub fn handle_plugin_view_dock_ui_event(
    engine: &mut Engine,
    name: &str,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};

    if !populate_plugin_view_dock_form_controller(engine, name, true) {
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
        .plugin_view_dock_form_controller
        .borrow_mut()
        .handle_cached(&probe, rect);

    let sync_scroll = |engine: &mut Engine| {
        let offset = engine
            .plugin_view_dock_form_controller
            .borrow()
            .scroll_offset();
        engine.plugin_view_dock_scroll_top = offset;
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
    apply_combo_box_form_action(engine, &action);

    let Some((view_name, field_id, mut kind)) = plugin_view_event_from_form_event(&action) else {
        return true;
    };
    kind = remap_section_toggle(engine, &view_name, &field_id, kind);
    kind = remap_styled_button_focus(engine, &view_name, &field_id, kind);

    if let Some(view) = engine.plugin_views.get(&view_name) {
        if let Some(idx) = view.field_index(&field_id) {
            engine.plugin_view_focus_field(
                &view_name,
                crate::core::engine::PluginViewHost::Dock,
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

/// The centred rect the plugin-view modal occupies within `viewport`:
/// 60 % of its width (at least 40 columns' worth of `cw`, clamped to the
/// viewport) and 60 % of its height, centred.
pub fn plugin_view_modal_rect(viewport: quadraui::Rect, cw: f32) -> quadraui::Rect {
    let w = (viewport.width * 0.6)
        .max(40.0 * cw)
        .min(viewport.width)
        .round();
    let h = (viewport.height * 0.6).round();
    let x = viewport.x + ((viewport.width - w) / 2.0).round();
    let y = viewport.y + ((viewport.height - h) / 2.0).round();
    quadraui::Rect::new(x, y, w, h)
}

/// Paint the plugin-view modal (`Engine::plugin_view_modal`) into
/// `modal_rect`: a title bar carrying the view's title above the view's
/// body. The body rect is cached in `Engine::plugin_view_modal_form_rect`
/// for the click router. Only the field-stack `Form` default and the
/// `TextView` body kind paint anything — the other body kinds have no
/// modal-hosted layout cache, the same "accepted, not yet wired" precedent
/// the dock host follows. The caller paints the scrim first.
pub fn paint_plugin_view_modal(
    engine: &Engine,
    modal_rect: quadraui::Rect,
    theme: &Theme,
    backend: &mut dyn quadraui::Backend,
) {
    let Some(name) = engine.plugin_view_modal.as_deref() else {
        return;
    };
    engine.plugin_view_modal_rect.set(modal_rect);
    let title = engine.plugin_view_title(name);
    let panel = SidebarPanelBody {
        background: Some(theme.tab_bar_bg),
        chrome: SidebarPanelChrome::StatusBars(vec![quadraui::StatusBar {
            id: quadraui::WidgetId::new("plugin-view-modal-title"),
            left_segments: vec![quadraui::StatusBarSegment {
                text: format!(" {title}"),
                fg: theme.foreground,
                bg: theme.completion_bg.lighten(0.08),
                bold: true,
                action_id: None,
            }],
            right_segments: Vec::new(),
        }]),
        scrollbar_gutter: None,
    };
    panel.render_with(backend, modal_rect, |backend, body| {
        engine.plugin_view_modal_form_rect.set(body);
        let kind = engine
            .plugin_views
            .get(name)
            .and_then(|v| v.body.as_ref())
            .map(|b| b.kind_name());
        match kind {
            None => {
                if populate_plugin_view_modal_form_controller(engine, name) {
                    engine
                        .plugin_view_modal_form_controller
                        .borrow_mut()
                        .render_and_cache(backend, body);
                }
            }
            Some("text_view") => {
                paint_plugin_view_text(engine, name, PluginViewHost::Modal, true, body, backend);
            }
            Some(_) => {}
        }
    });
}

/// Route a left press at `position` while the plugin-view modal is open.
/// Returns `false` (nothing consumed) only when no modal is open; otherwise
/// the modal swallows the press: inside a `Form`-kind view's body it routes
/// through [`handle_plugin_view_modal_ui_event`], elsewhere inside the modal
/// it is inert, and outside the modal it dismisses it
/// (`Engine::dismiss_plugin_view_modal`), like clicking a dialog's scrim.
pub fn route_plugin_view_modal_click(engine: &mut Engine, position: quadraui::Point) -> bool {
    let Some(name) = engine.plugin_view_modal.clone() else {
        return false;
    };
    let modal_rect = engine.plugin_view_modal_rect.get();
    if !modal_rect.contains(position) {
        engine.dismiss_plugin_view_modal();
        return true;
    }
    let body = engine.plugin_view_modal_form_rect.get();
    let is_form = engine.plugin_views.get(&name).is_some_and(|v| v.is_form());
    if is_form && body.contains(position) {
        let event = quadraui::UiEvent::MouseDown {
            widget: None,
            button: quadraui::MouseButton::Left,
            position,
            modifiers: quadraui::Modifiers::default(),
        };
        handle_plugin_view_modal_ui_event(engine, &name, &event, body);
    }
    true
}

/// Scroll the open plugin-view modal's body by a wheel `delta_y` (backend
/// polarity: positive scrolls down, the same polarity
/// `Engine::handle_plugin_view_dock_scroll` takes). A `TextView` body
/// scrolls its text; a `Form` body scrolls its field stack.
pub fn scroll_plugin_view_modal(engine: &mut Engine, delta_y: f32) {
    let Some(name) = engine.plugin_view_modal.clone() else {
        return;
    };
    let kind = engine
        .plugin_views
        .get(&name)
        .and_then(|v| v.body.as_ref())
        .map(|b| b.kind_name());
    match kind {
        Some("text_view") => {
            let step = (delta_y.abs() * 3.0).ceil() as i32 * delta_y.signum() as i32;
            scroll_plugin_view_text(engine, &name, PluginViewHost::Modal, step);
        }
        None => {
            let body = engine.plugin_view_modal_form_rect.get();
            let center =
                quadraui::Point::new(body.x + body.width / 2.0, body.y + body.height / 2.0);
            let event = quadraui::UiEvent::Scroll {
                widget: None,
                position: center,
                delta: quadraui::ScrollDelta::new(0.0, -delta_y),
            };
            handle_plugin_view_modal_ui_event(engine, &name, &event, body);
        }
        Some(_) => {}
    }
}

/// Route a pointer or forwarded key event over the modal-hosted plugin view
/// — the modal twin of [`handle_plugin_view_dock_ui_event`]. `name` is
/// `Engine::plugin_view_modal`'s payload; `rect` must be the rect the
/// `FrameOp::PluginViewModal` rung last painted the view's `Form` into
/// (cached in `Engine::plugin_view_modal_form_rect`).
///
/// Returns `true` when the event was consumed.
pub fn handle_plugin_view_modal_ui_event(
    engine: &mut Engine,
    name: &str,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
) -> bool {
    use crate::core::plugin_ui::{PluginViewEvent, ViewEventKind};

    if !populate_plugin_view_modal_form_controller(engine, name) {
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
        .plugin_view_modal_form_controller
        .borrow_mut()
        .handle_cached(&probe, rect);

    let sync_scroll = |engine: &mut Engine| {
        let offset = engine
            .plugin_view_modal_form_controller
            .borrow()
            .scroll_offset();
        engine.plugin_view_modal_scroll_top = offset;
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
    apply_combo_box_form_action(engine, &action);

    let Some((view_name, field_id, mut kind)) = plugin_view_event_from_form_event(&action) else {
        return true;
    };
    kind = remap_section_toggle(engine, &view_name, &field_id, kind);
    kind = remap_styled_button_focus(engine, &view_name, &field_id, kind);

    if let Some(view) = engine.plugin_views.get(&view_name) {
        if let Some(idx) = view.field_index(&field_id) {
            engine.plugin_view_focus_field(&view_name, PluginViewHost::Modal, idx);
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

/// Route a `UiEvent` over a body-kind (`List`/`Tree`/`Table`/`TextView`/
/// `Chart`/`MessageList`/`Image`) plugin view at `host`, through the
/// matching primitive's click/scroll resolution instead of `FormController`.
/// `rect` is the rect the view was last painted into for this host — only
/// the `Tree` arm needs it live (a `TreeController` needs a real rect +
/// backend to `handle` against); `List`/`Table`/`Chart`/`MessageList`
/// resolve through their own cached layouts/rects
/// (`route_plugin_view_list_click`/`route_plugin_view_table_click`/
/// `route_plugin_view_chart_click`/`route_plugin_view_message_list_click`)
/// instead. `Image` is display-only and consumes nothing.
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
                modifiers,
                ..
            } => route_plugin_view_table_click(engine, name, host, *position, false, *modifiers),
            quadraui::UiEvent::DoubleClick { position, .. } => {
                // `DoubleClick` carries no modifiers of its own — a plain
                // `Replace` gesture (one row selected, not toggled/extended)
                // is the only sensible one for an activation click anyway.
                route_plugin_view_table_click(
                    engine,
                    name,
                    host,
                    *position,
                    true,
                    quadraui::Modifiers::default(),
                )
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
        Some("chart") => Some(match event {
            quadraui::UiEvent::MouseDown {
                position,
                button: quadraui::MouseButton::Left,
                ..
            } => route_plugin_view_chart_click(engine, name, host, *position, backend),
            _ => false,
        }),
        Some("message_list") => Some(match event {
            quadraui::UiEvent::MouseDown {
                position,
                button: quadraui::MouseButton::Left,
                ..
            } => {
                route_plugin_view_message_list_click(engine, name, host, *position, false, backend)
            }
            quadraui::UiEvent::DoubleClick { position, .. } => {
                route_plugin_view_message_list_click(engine, name, host, *position, true, backend)
            }
            quadraui::UiEvent::Scroll { delta, .. } => {
                scroll_plugin_view_message_list(engine, name, host, scroll_step(delta))
            }
            _ => false,
        }),
        // `Image` is display-only — it has no scroll/selection state and
        // emits no `ViewEventKind`, so every event falls through unconsumed.
        Some("image") => Some(false),
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

/// A styled `Button` field (one that declared `variant` and/or `icon` — see
/// [`plugin_view_to_form`]'s own doc for why `tooltip` alone does not count)
/// is rerouted to a single-item `ButtonRow` in [`plugin_view_to_form`], reusing
/// the *same* widget id as the enclosing `FormField` so the field's own label
/// column stays blank and the caption paints once, inside that item. quadraui
/// 0.9's `form_click_event` walks fields in declaration order and returns on
/// the *first* id match — this field's own `_ => FormEvent::FocusChanged` arm
/// wins before the `FieldKind::ButtonRow` arm below it is ever reached, so a
/// click on a styled button decodes as a bare focus move rather than
/// `ButtonClicked`. Re-resolve `field_id` against the live view and, when it
/// names a `Button` field that opted into styling, remap `FocusChanged` into
/// `ButtonClicked` — the event a plugin that asked for the styling vocabulary
/// actually expects. A plain (unstyled) `Button` field never hits this: it
/// keeps painting through `FieldKind::Button`, whose own id-match arm already
/// returns `FormEvent::ButtonClicked` directly.
fn remap_styled_button_focus(
    engine: &Engine,
    view_name: &str,
    field_id: &str,
    kind: crate::core::plugin_ui::ViewEventKind,
) -> crate::core::plugin_ui::ViewEventKind {
    use crate::core::plugin_ui::{ViewEventKind, ViewFieldKind};
    if !matches!(kind, ViewEventKind::FocusChanged) {
        return kind;
    }
    let Some(view) = engine.plugin_views.get(view_name) else {
        return kind;
    };
    let Some(idx) = view.field_index(field_id) else {
        return kind;
    };
    match &view.fields[idx].kind {
        ViewFieldKind::Button { variant, icon, .. } if variant.is_some() || icon.is_some() => {
            ViewEventKind::ButtonClicked
        }
        _ => kind,
    }
}

/// The combo box's currently-declared `value`, looked up by `(view,
/// field)`, or `""` when the view/field is absent or isn't a `combo_box`.
/// Seeds a freshly-created
/// [`crate::core::plugin_ui::PluginViewComboBoxEditState`]'s `text` so an
/// edit that starts with a non-text-typing action (arrow-key navigation,
/// see [`apply_combo_box_form_action`]'s `ComboBoxHighlightChanged` arm)
/// doesn't blank the row before the plugin's own declared value ever had a
/// chance to paint.
fn declared_combo_box_value(engine: &Engine, view: &str, field: &str) -> String {
    use crate::core::plugin_ui::ViewFieldKind;
    engine
        .plugin_views
        .get(view)
        .and_then(|v| v.field_index(field).map(|i| &v.fields[i]))
        .and_then(|f| match &f.kind {
            ViewFieldKind::ComboBox { value, .. } => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Apply a `quadraui::FormEvent::ComboBox*` action to
/// `Engine::plugin_view_combo_edit` — the vimcode-owned shadow state
/// `plugin_view_to_form_with_combo_edit`'s `ComboBox` arm feeds back into
/// the embedded `quadraui::ComboBox` every render, since quadraui's own
/// `FormController` keeps no such cache itself (see
/// [`crate::core::plugin_ui::PluginViewComboBoxEditState`]'s doc). A no-op
/// for any other `FormEvent` variant, or for a `ComboBox*` event whose id
/// isn't in the plugin namespace.
///
/// Called by every `handle_plugin_view_*_ui_event` before
/// [`plugin_view_event_from_form_event`], so the shadow state is updated
/// even for `ComboBoxHighlightChanged`/`ComboBoxCancelled` — the two
/// variants that function deliberately does *not* turn into a
/// `ViewEventKind` the plugin sees.
///
/// `ComboBoxCommitted`/`ComboBoxCancelled` both *clear* the state rather
/// than freeze it at the final value: the next render rebuilds the
/// `ComboBox` from the plugin's own declared `value` (which `on_event` is
/// expected to update to match, for `Committed`) with the dropdown closed —
/// the same "declared value is authoritative once vimcode stops actively
/// editing it" contract every other field kind here follows.
fn apply_combo_box_form_action(engine: &mut Engine, action: &quadraui::FormEvent) {
    use crate::core::plugin_ui::{split_widget_id, PluginViewComboBoxEditState};

    /// The combo-edit slot for `(view, field)`, resetting it first if it
    /// currently names a different field (stale from a prior edit, or
    /// never primed). A freshly-created slot seeds `text` from the
    /// field's own declared value (via [`declared_combo_box_value`]),
    /// never `String::new()` — a bare arrow key as the first keystroke
    /// must not blank a row that already shows a non-empty declared
    /// value.
    fn ensure<'a>(
        engine: &'a mut Engine,
        view: &str,
        field: &str,
    ) -> &'a mut PluginViewComboBoxEditState {
        let stale = !matches!(
            &engine.plugin_view_combo_edit,
            Some(s) if s.view == view && s.field_id == field
        );
        if stale {
            let text = declared_combo_box_value(engine, view, field);
            engine.plugin_view_combo_edit = Some(PluginViewComboBoxEditState {
                view: view.to_string(),
                field_id: field.to_string(),
                text,
                open: false,
                highlighted: None,
            });
        }
        engine
            .plugin_view_combo_edit
            .as_mut()
            .expect("just set above")
    }

    match action {
        quadraui::FormEvent::ComboBoxTextChanged { id, value } => {
            let Some((view, field)) = split_widget_id(id.as_str()) else {
                return;
            };
            let state = ensure(engine, view, field);
            state.text = value.clone();
            // Typed characters also open the dropdown — mirrors
            // `quadraui`'s own `form_combo_box` reference example.
            state.open = true;
        }
        quadraui::FormEvent::ComboBoxHighlightChanged { id, index } => {
            let Some((view, field)) = split_widget_id(id.as_str()) else {
                return;
            };
            let state = ensure(engine, view, field);
            state.highlighted = Some(*index);
            // Arrow-key navigation opens the dropdown too (quadraui's own
            // `ComboBox::handle_key` moves `highlighted` regardless of
            // `open`, but `ComboBox::commit()` only honours `highlighted`
            // when `open` is set) — without this, pressing `Down` then
            // `Enter` as the very first two keystrokes would commit the
            // typed text ("") instead of the now-highlighted option.
            state.open = true;
        }
        quadraui::FormEvent::ComboBoxCommitted { id, .. }
        | quadraui::FormEvent::ComboBoxCancelled { id } => {
            let Some((view, field)) = split_widget_id(id.as_str()) else {
                return;
            };
            if matches!(
                &engine.plugin_view_combo_edit,
                Some(s) if s.view == view && s.field_id == field
            ) {
                engine.plugin_view_combo_edit = None;
            }
        }
        _ => {}
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
        // `ComboBoxHighlightChanged`/`ComboBoxCancelled` fall to the `_`
        // arm below: they're vimcode-owned interaction detail
        // (`Engine::plugin_view_combo_edit`, applied by
        // `apply_combo_box_form_action` before this function ever runs),
        // not something the plugin needs to hear about — same convention
        // as a `Text`/`Password` field's cursor movement.
        quadraui::FormEvent::ComboBoxTextChanged { id, value } => (
            id,
            ViewEventKind::TextChanged {
                value: value.clone(),
            },
        ),
        quadraui::FormEvent::ComboBoxCommitted { id, value } => (
            id,
            ViewEventKind::ComboBoxChanged {
                value: value.clone(),
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
                ..Default::default()
            }],
            body: None,
            ..Default::default()
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
            ..Default::default()
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
            kind: ViewFieldKind::plain_button(),
            ..Default::default()
        }
    }

    fn styled_button_field(id: &str) -> ViewField {
        ViewField {
            id: id.into(),
            label: String::new(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind: ViewFieldKind::Button {
                variant: Some(crate::core::plugin_ui::ViewButtonVariant::Danger),
                icon: None,
                tooltip: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn collapsible_section_translates_to_a_button_with_a_collapsed_glyph() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![section_field("adv", "Advanced", true, true)],
            body: None,
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
            },
        );
        let kind = remap_section_toggle(&engine, "myview", "ok", ViewEventKind::ButtonClicked);
        assert_eq!(kind, ViewEventKind::ButtonClicked);
    }

    /// Reproduces the mouse-dead styled-`button` click regression:
    /// `form_click_event` resolves a click on the single-item `ButtonRow`
    /// [`plugin_view_to_form`] built for a styled `Button` field to
    /// `FormEvent::FocusChanged` (the field's own id, matched before the
    /// `ButtonRow` arm is ever reached) — `remap_styled_button_focus` must
    /// turn that back into `ButtonClicked`.
    #[test]
    fn focus_changed_on_a_styled_button_field_remaps_to_button_clicked() {
        use crate::core::plugin_ui::ViewEventKind;
        let mut engine = Engine::new();
        engine.plugin_views.insert(
            "myview".to_string(),
            PluginView {
                id: "myview".into(),
                schema_version: 1,
                fields: vec![styled_button_field("del")],
                body: None,
                ..Default::default()
            },
        );
        let kind = remap_styled_button_focus(&engine, "myview", "del", ViewEventKind::FocusChanged);
        assert_eq!(kind, ViewEventKind::ButtonClicked);
    }

    /// A plain (unstyled) `button` field never hits the id collision in the
    /// first place — `form_click_event`'s own `FieldKind::Button` arm
    /// already returns `ButtonClicked` directly — but confirms the remap is
    /// a no-op for it regardless, in case it's ever reached with
    /// `FocusChanged` some other way.
    #[test]
    fn focus_changed_on_a_plain_button_field_is_unaffected() {
        use crate::core::plugin_ui::ViewEventKind;
        let mut engine = Engine::new();
        engine.plugin_views.insert(
            "myview".to_string(),
            PluginView {
                id: "myview".into(),
                schema_version: 1,
                fields: vec![button_field("go")],
                body: None,
                ..Default::default()
            },
        );
        let kind = remap_styled_button_focus(&engine, "myview", "go", ViewEventKind::FocusChanged);
        assert_eq!(kind, ViewEventKind::FocusChanged);
    }

    /// A plain focus move on a non-button field (the ordinary case —
    /// clicking a text/dropdown/toggle row) must pass through unchanged.
    #[test]
    fn focus_changed_on_a_non_button_field_is_unaffected() {
        use crate::core::plugin_ui::ViewEventKind;
        let mut engine = Engine::new();
        engine.plugin_views.insert(
            "myview".to_string(),
            PluginView {
                id: "myview".into(),
                schema_version: 1,
                fields: vec![section_field("adv", "Advanced", true, false)],
                body: None,
                ..Default::default()
            },
        );
        let kind = remap_styled_button_focus(&engine, "myview", "adv", ViewEventKind::FocusChanged);
        assert_eq!(kind, ViewEventKind::FocusChanged);
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

#[cfg(test)]
mod color_translation_tests {
    use super::*;
    use crate::core::plugin_ui::{PluginView, ViewField, ViewFieldKind};

    fn color_field(value: &str) -> ViewField {
        ViewField {
            id: "swatch".into(),
            label: "Accent".into(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind: ViewFieldKind::Color {
                value: value.into(),
            },
            ..Default::default()
        }
    }

    fn plain_text(text: &quadraui::StyledText) -> String {
        text.spans.iter().map(|s| s.text.as_str()).collect()
    }

    fn form_for(value: &str) -> quadraui::Form {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![color_field(value)],
            body: None,
            ..Default::default()
        };
        plugin_view_to_form("myview", &view, 0, 0, false, None)
    }

    #[test]
    fn valid_hex_translates_to_a_color_picker_swatch() {
        let form = form_for("#ff8800");
        assert_eq!(
            form.fields[0].kind,
            quadraui::FieldKind::ColorPicker {
                value: Color::from_hex("#ff8800").expect("valid hex"),
            }
        );
    }

    #[test]
    fn malformed_hex_falls_back_to_a_read_only_row_echoing_the_raw_string() {
        let form = form_for("not-a-color");
        match &form.fields[0].kind {
            quadraui::FieldKind::ReadOnly { value } => {
                assert_eq!(plain_text(value), "not-a-color");
            }
            other => panic!("expected ReadOnly fallback, got {other:?}"),
        }
    }

    #[test]
    fn empty_value_falls_back_to_an_empty_read_only_row() {
        let form = form_for("");
        match &form.fields[0].kind {
            quadraui::FieldKind::ReadOnly { value } => {
                assert_eq!(plain_text(value), "");
            }
            other => panic!("expected ReadOnly fallback, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod button_style_translation_tests {
    use super::*;
    use crate::core::plugin_ui::{PluginView, ViewButtonVariant, ViewField, ViewFieldKind};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn button_field(id: &str, label: &str, kind: ViewFieldKind) -> ViewField {
        ViewField {
            id: id.into(),
            label: label.into(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind,
            ..Default::default()
        }
    }

    fn cell_char(buf: &Buffer, x: u16, y: u16) -> char {
        buf[(x, y)].symbol().chars().next().unwrap_or(' ')
    }

    fn row_string(buf: &Buffer, y: u16, width: u16) -> String {
        (0..width).map(|x| cell_char(buf, x, y)).collect()
    }

    fn cell_fg(buf: &Buffer, x: u16, y: u16) -> ratatui::style::Color {
        buf[(x, y)]
            .style()
            .fg
            .unwrap_or(ratatui::style::Color::Reset)
    }

    /// A `button` field that declares a `danger` variant paints its
    /// bracket chrome and label in `theme.error_fg` on TUI — proving
    /// `plugin_view_to_form`'s styled-`Button` arm actually reaches
    /// quadraui's intent-colour paint path (through the single-item
    /// `ButtonRow` reroute that arm's own doc explains), not just that
    /// `ButtonSpec.variant` got set somewhere in memory.
    ///
    /// A `primary` variant with an icon set alongside it paints the exact
    /// same plain foreground as no variant at all: quadraui 0.9's TUI
    /// rasteriser only tints `Danger`/`Warning` (see
    /// `tui::form::draw_field_button_row`'s own doc comment upstream) —
    /// this is "TUI degrades intent to the foreground colour on
    /// `[ Label ]`", the behaviour this issue's acceptance bar names
    /// explicitly, not a bug in this translation.
    #[test]
    fn a_danger_button_field_tints_its_bracket_chrome_on_tui() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![
                button_field(
                    "del",
                    "Delete",
                    ViewFieldKind::Button {
                        variant: Some(ViewButtonVariant::Danger),
                        icon: None,
                        tooltip: None,
                    },
                ),
                button_field(
                    "save",
                    "Save",
                    ViewFieldKind::Button {
                        variant: Some(ViewButtonVariant::Primary),
                        icon: Some("save".into()),
                        tooltip: None,
                    },
                ),
            ],
            body: None,
            ..Default::default()
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        let theme = quadraui::Theme::default();

        let width = 40u16;
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 2));
        quadraui::tui::draw_form(&mut buf, Rect::new(0, 0, width, 2), &form, &theme);

        let danger_row = row_string(&buf, 0, width);
        let danger_bracket = danger_row
            .find('[')
            .expect("danger button must paint bracket chrome");
        assert!(
            danger_row.contains("Delete"),
            "danger button's caption must still paint: {danger_row:?}"
        );
        assert_eq!(
            cell_fg(&buf, danger_bracket as u16, 0),
            quadraui::tui::ratatui_color(theme.error_fg),
            "a `danger` button field must tint its bracket in theme.error_fg; row: {danger_row:?}"
        );

        let primary_row = row_string(&buf, 1, width);
        let primary_bracket = primary_row
            .find('[')
            .expect("primary button must paint bracket chrome");
        assert!(
            primary_row.contains("Save"),
            "primary button's caption must still paint: {primary_row:?}"
        );
        assert_ne!(
            cell_fg(&buf, primary_bracket as u16, 1),
            quadraui::tui::ratatui_color(theme.error_fg),
            "a `primary` button field must not pick up the danger tint; row: {primary_row:?}"
        );
        // `resolve_view_icon`'s codicon fallback text is the icon's own name
        // (see that function's own doc) — assert the row actually paints
        // `"save"`, proving the `icon` key reached quadraui's `ButtonRowItem`
        // and not merely that the `ButtonSpec` was built with it dropped.
        assert!(
            primary_row.contains("save"),
            "an `icon` on a styled button field must paint its fallback glyph text; \
             row: {primary_row:?}"
        );
    }

    /// A plain `button` field (no `variant`/`icon`/`tooltip`) keeps
    /// painting through quadraui's standalone `FieldKind::Button` —
    /// today's `< label >` look, byte-for-byte, proving the additive
    /// vocabulary changes nothing for every plugin that doesn't opt in.
    #[test]
    fn a_plain_button_field_keeps_its_unstyled_look() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![button_field("go", "Go", ViewFieldKind::plain_button())],
            body: None,
            ..Default::default()
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(form.fields[0].kind, quadraui::FieldKind::Button);

        let width = 40u16;
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        let theme = quadraui::Theme::default();
        quadraui::tui::draw_form(&mut buf, Rect::new(0, 0, width, 1), &form, &theme);
        let row = row_string(&buf, 0, width);
        assert!(
            row.contains("< Go >"),
            "an unstyled button field must still paint quadraui's plain \
             bracket chrome: {row:?}"
        );
    }

    /// A `button` field that sets only `tooltip` (no `variant`/`icon`) must
    /// *not* reroute to `ButtonRow`: no backend paints a form-row tooltip
    /// today, so rerouting would pay for a layout change (quadraui's
    /// right-anchored `< label >` becomes a flush-left `[ label ]`) for no
    /// visible benefit, and the tooltip itself is simply dropped.
    #[test]
    fn a_tooltip_only_button_field_keeps_its_unstyled_look() {
        let view = PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![button_field(
                "go",
                "Go",
                ViewFieldKind::Button {
                    variant: None,
                    icon: None,
                    tooltip: Some("Run it".into()),
                },
            )],
            body: None,
            ..Default::default()
        };
        let form = plugin_view_to_form("myview", &view, 0, 0, false, None);
        assert_eq!(form.fields[0].kind, quadraui::FieldKind::Button);

        let width = 40u16;
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        let theme = quadraui::Theme::default();
        quadraui::tui::draw_form(&mut buf, Rect::new(0, 0, width, 1), &form, &theme);
        let row = row_string(&buf, 0, width);
        assert!(
            row.contains("< Go >"),
            "a tooltip-only button field must still paint quadraui's plain \
             bracket chrome: {row:?}"
        );
    }
}

/// Proves the Lua layout knobs (`label_position`, `label_align`,
/// `label_width`, `control_width`, `min_control_width`) declared either
/// form-wide on a view or per field reach the **painted** output of
/// [`plugin_view_to_form`], by reading the cells `quadraui::tui::draw_form`
/// wrote — not by inspecting the `quadraui::Form` fields it built.
#[cfg(test)]
mod layout_knob_translation_tests {
    use super::*;
    use crate::core::plugin_ui::{
        PluginView, ViewControlWidth, ViewField, ViewFieldKind, ViewLabelAlign, ViewLabelPosition,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn cell_char(buf: &Buffer, x: u16, y: u16) -> char {
        buf[(x, y)].symbol().chars().next().unwrap_or(' ')
    }

    /// One painted row, as text, so assertions can talk about what the user
    /// sees instead of about cell coordinates.
    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| cell_char(buf, x, y)).collect()
    }

    /// The whole painted buffer, for assertion messages.
    fn dump(buf: &Buffer) -> String {
        (0..buf.area.height)
            .map(|y| format!("{y:>2}|{}|", row_text(buf, y)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The first painted row containing `needle`.
    fn row_containing(buf: &Buffer, needle: &str) -> Option<u16> {
        (0..buf.area.height).find(|&y| row_text(buf, y).contains(needle))
    }

    fn paint(view: &PluginView, width: u16, height: u16) -> Buffer {
        let form = plugin_view_to_form("myview", view, 0, 0, false, None);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        quadraui::tui::draw_form(&mut buf, area, &form, &quadraui::Theme::default());
        buf
    }

    /// Distance between the opening and closing bracket of row `y`'s
    /// control — one less than the control's painted width, since
    /// `draw_form` closes at `control_start + control_width - 1`.
    fn bracket_span(buf: &Buffer, y: u16) -> u16 {
        let w = buf.area.width;
        let open = (0..w)
            .find(|&x| cell_char(buf, x, y) == '[')
            .unwrap_or_else(|| panic!("no opening bracket painted:\n{}", dump(buf)));
        let close = (open..w)
            .find(|&x| cell_char(buf, x, y) == ']')
            .unwrap_or_else(|| panic!("no closing bracket painted:\n{}", dump(buf)));
        close - open
    }

    fn toggle_field(id: &str, label: &str) -> ViewField {
        ViewField {
            id: id.into(),
            label: label.into(),
            kind: ViewFieldKind::Toggle { value: true },
            ..Default::default()
        }
    }

    fn text_field(id: &str, label: &str, value: &str) -> ViewField {
        ViewField {
            id: id.into(),
            label: label.into(),
            kind: ViewFieldKind::Text {
                value: value.into(),
                placeholder: String::new(),
            },
            ..Default::default()
        }
    }

    fn view_of(fields: Vec<ViewField>) -> PluginView {
        PluginView {
            id: "v".into(),
            schema_version: 1,
            fields,
            body: None,
            ..Default::default()
        }
    }

    /// A view that declares no layout knob paints a label beside its
    /// control, on the same row — the baseline every other test here
    /// contrasts against.
    #[test]
    fn a_view_with_no_layout_knobs_paints_its_label_beside_its_control() {
        let buf = paint(&view_of(vec![toggle_field("name", "Name")]), 30, 4);
        let row = row_containing(&buf, "Name").expect("the label must paint");
        assert!(
            row_text(&buf, row).contains("[x]"),
            "the default layout keeps label and control on one row:\n{}",
            dump(&buf)
        );
    }

    /// `label_position = "top"` declared form-wide lifts the label onto a
    /// row of its own, above a control row that carries no label.
    #[test]
    fn form_wide_label_position_top_lifts_the_label_off_its_control_row() {
        let mut view = view_of(vec![toggle_field("name", "Name")]);
        view.label_position = Some(ViewLabelPosition::Top);
        let buf = paint(&view, 30, 4);

        let label_row = row_containing(&buf, "Name").expect("the label must paint");
        let control_row = row_containing(&buf, "[x]").expect("the toggle must paint");
        assert_eq!(
            label_row + 1,
            control_row,
            "the label must paint on the row directly above the toggle:\n{}",
            dump(&buf)
        );
        assert!(
            !row_text(&buf, label_row).contains('['),
            "the label's own row must carry no control:\n{}",
            dump(&buf)
        );
    }

    /// A per-field `label_position` overrides the form-wide default for
    /// that row alone: the inheriting field's label sits above its control,
    /// the overriding field's label stays beside its own.
    #[test]
    fn a_field_level_label_position_overrides_the_form_wide_default() {
        let mut view = view_of(vec![
            toggle_field("name", "Name"),
            ViewField {
                label_position: Some(ViewLabelPosition::Left),
                ..toggle_field("flag", "Flag")
            },
        ]);
        view.label_position = Some(ViewLabelPosition::Top);
        let buf = paint(&view, 30, 6);

        let name_row = row_containing(&buf, "Name").expect("the first label must paint");
        assert!(
            !row_text(&buf, name_row).contains('['),
            "the inheriting field's label must sit alone above its control:\n{}",
            dump(&buf)
        );
        let flag_row = row_containing(&buf, "Flag").expect("the second label must paint");
        assert!(
            row_text(&buf, flag_row).contains("[x]"),
            "the overriding field's label must stay beside its control:\n{}",
            dump(&buf)
        );
    }

    /// `label_width` reserves a fixed label column and `label_align = "end"`
    /// pushes the label text to that column's right edge, so the label
    /// starts well right of column 1.
    #[test]
    fn form_wide_label_width_and_end_align_right_align_the_label_column() {
        let mut view = view_of(vec![toggle_field("name", "Name")]);
        view.label_width = Some(15.0);
        view.label_align = Some(ViewLabelAlign::End);
        let buf = paint(&view, 40, 3);

        let row = row_containing(&buf, "Name").expect("the label must paint");
        // A 15-cell column starting at column 1 ends at column 16; a
        // 4-cell label aligned to its end therefore starts at column 12.
        assert_eq!(
            row_text(&buf, row).find("Name"),
            Some(12),
            "`label_width = 15` + `label_align = \"end\"` must right-align \
             the label inside its column:\n{}",
            dump(&buf)
        );
    }

    /// A per-field `control_width = { fixed = n }` closes the bracketed
    /// control `n - 1` cells after it opens, however wide the row is —
    /// instead of the default `Fill`, which stretches to the right edge.
    #[test]
    fn control_width_fixed_stops_the_control_at_n_cells() {
        let view = view_of(vec![ViewField {
            control_width: Some(ViewControlWidth::Fixed(10.0)),
            ..text_field(
                "name",
                "Name",
                "a very long value that would otherwise stretch far",
            )
        }]);
        let width = 60u16;
        let buf = paint(&view, width, 1);

        assert_eq!(
            bracket_span(&buf, 0),
            9,
            "a `control_width = {{ fixed = 10 }}` field must close its \
             bracket 9 cells after it opens, not stretch to fill the row:\n{}",
            dump(&buf)
        );
    }

    /// The same knob declared form-wide applies to a field that sets none
    /// of its own.
    #[test]
    fn form_wide_control_width_fixed_applies_to_a_field_with_no_override() {
        let mut view = view_of(vec![text_field("name", "Name", "a long enough value here")]);
        view.control_width = Some(ViewControlWidth::Fixed(10.0));
        let buf = paint(&view, 60, 1);

        assert_eq!(
            bracket_span(&buf, 0),
            9,
            "the form-wide fixed control width must reach the painted row:\n{}",
            dump(&buf)
        );
    }

    /// `control_width = "content"` shrinks the control to its own text, and
    /// `min_control_width` then floors how far it may shrink.
    #[test]
    fn control_width_content_shrinks_to_its_text_and_min_control_width_floors_it() {
        let mut view = view_of(vec![text_field("name", "Name", "ab")]);
        view.control_width = Some(ViewControlWidth::Content);
        let buf = paint(&view, 60, 1);
        // `"ab"` plus its two brackets is a 4-cell control.
        assert_eq!(
            bracket_span(&buf, 0),
            3,
            "`control_width = \"content\"` must shrink the control to its \
             own text:\n{}",
            dump(&buf)
        );

        view.min_control_width = Some(20.0);
        let buf = paint(&view, 60, 1);
        assert_eq!(
            bracket_span(&buf, 0),
            19,
            "`min_control_width = 20` must floor the content-sized \
             control:\n{}",
            dump(&buf)
        );
    }
}

#[cfg(test)]
mod combo_box_translation_tests {
    use super::*;
    use crate::core::engine::Engine;
    use crate::core::plugin_ui::{
        namespaced_widget_id, PluginView, PluginViewComboBoxEditState, ViewField, ViewFieldKind,
    };

    fn combo_field(id: &str, options: &[&str], value: &str) -> ViewField {
        ViewField {
            id: id.into(),
            label: "Fruit".into(),
            hint: String::new(),
            disabled: false,
            error: None,
            warning: None,
            kind: ViewFieldKind::ComboBox {
                options: options.iter().map(|s| s.to_string()).collect(),
                value: value.into(),
                placeholder: String::new(),
            },
            ..Default::default()
        }
    }

    fn combo_view(value: &str) -> PluginView {
        PluginView {
            id: "v".into(),
            schema_version: 1,
            fields: vec![combo_field("fruit", &["Apple", "Apricot", "Banana"], value)],
            body: None,
            ..Default::default()
        }
    }

    /// `combo_edit: None` — no edit in progress — seeds both `committed`
    /// and `text` from the plugin's own declared `value`, with the
    /// dropdown closed: the same "declared value paints read-only"
    /// contract every other field kind here follows.
    #[test]
    fn no_live_edit_paints_the_declared_value_closed() {
        let view = combo_view("Banana");
        let form = plugin_view_to_form_with_combo_edit("myview", &view, 0, 0, false, None, None);
        let quadraui::FieldKind::ComboBox(combo) = &form.fields[0].kind else {
            panic!(
                "expected FieldKind::ComboBox, got {:?}",
                form.fields[0].kind
            );
        };
        assert_eq!(combo.committed, "Banana");
        assert_eq!(combo.text, "Banana");
        assert!(!combo.open);
        assert_eq!(combo.highlighted, None);
    }

    /// `combo_edit: Some(state)` naming this field — a live edit in
    /// progress — overrides `text`/`open`/`highlighted` from the shadow
    /// state, but `committed` still comes from the plugin's declared
    /// `value`: a mid-edit combo box must still know what to revert to on
    /// `Escape`.
    #[test]
    fn a_live_edit_overrides_text_open_and_highlighted_but_not_committed() {
        let view = combo_view("Banana");
        let edit = PluginViewComboBoxEditState {
            view: "myview".into(),
            field_id: "fruit".into(),
            text: "ap".into(),
            open: true,
            highlighted: Some(1),
        };
        let form =
            plugin_view_to_form_with_combo_edit("myview", &view, 0, 0, false, None, Some(&edit));
        let quadraui::FieldKind::ComboBox(combo) = &form.fields[0].kind else {
            panic!(
                "expected FieldKind::ComboBox, got {:?}",
                form.fields[0].kind
            );
        };
        assert_eq!(combo.committed, "Banana");
        assert_eq!(combo.text, "ap");
        assert!(combo.open);
        assert_eq!(combo.highlighted, Some(1));
    }

    /// Regression for the bug where pressing `Down` as the very first key
    /// on a `combo_box` whose declared `value` is non-empty blanked the
    /// row: `ComboBoxHighlightChanged` lazily creates the shadow state
    /// before any `ComboBoxTextChanged` has had a chance to prime `text`,
    /// so a fresh state's `text` must seed from the field's *declared*
    /// `value`, not `String::new()`.
    #[test]
    fn highlight_changed_as_the_first_action_does_not_blank_a_non_empty_declared_value() {
        let mut engine = Engine::new_for_test();
        engine
            .plugin_views
            .insert("myview".into(), combo_view("Banana"));
        assert!(engine.plugin_view_combo_edit.is_none());

        let id = quadraui::WidgetId::new(namespaced_widget_id("myview", "fruit"));
        apply_combo_box_form_action(
            &mut engine,
            &quadraui::FormEvent::ComboBoxHighlightChanged { id, index: 0 },
        );

        let state = engine
            .plugin_view_combo_edit
            .as_ref()
            .expect("highlight must prime the shadow state");
        assert_eq!(
            state.text, "Banana",
            "a fresh shadow state must seed text from the declared value, not blank it"
        );
        assert_eq!(state.highlighted, Some(0));
        assert!(
            state.open,
            "arrow-key highlight must open the dropdown so a following Enter commits \
             the highlighted option rather than the (irrelevant) typed text"
        );
    }

    /// `ComboBoxTextChanged` still behaves as before: it primes `text`
    /// from the typed value (not the declared one — the user is
    /// overwriting it) and opens the dropdown.
    #[test]
    fn text_changed_primes_text_from_the_typed_value_and_opens() {
        let mut engine = Engine::new_for_test();
        engine
            .plugin_views
            .insert("myview".into(), combo_view("Banana"));

        let id = quadraui::WidgetId::new(namespaced_widget_id("myview", "fruit"));
        apply_combo_box_form_action(
            &mut engine,
            &quadraui::FormEvent::ComboBoxTextChanged {
                id,
                value: "ap".into(),
            },
        );

        let state = engine
            .plugin_view_combo_edit
            .as_ref()
            .expect("typing must prime the shadow state");
        assert_eq!(state.text, "ap");
        assert!(state.open);
    }

    /// `ComboBoxCommitted`/`ComboBoxCancelled` both clear the shadow state
    /// so the next render falls back to the plugin's own declared value.
    #[test]
    fn committed_and_cancelled_both_clear_the_shadow_state() {
        let mut engine = Engine::new_for_test();
        engine
            .plugin_views
            .insert("myview".into(), combo_view("Banana"));
        let id = quadraui::WidgetId::new(namespaced_widget_id("myview", "fruit"));

        apply_combo_box_form_action(
            &mut engine,
            &quadraui::FormEvent::ComboBoxTextChanged {
                id,
                value: "Cherry".into(),
            },
        );
        assert!(engine.plugin_view_combo_edit.is_some());

        apply_combo_box_form_action(
            &mut engine,
            &quadraui::FormEvent::ComboBoxCommitted {
                id,
                value: "Cherry".into(),
            },
        );
        assert!(engine.plugin_view_combo_edit.is_none());
    }
}

/// Painted-output proof that a `kind = "row"/"column"` layout container
/// reaches `quadraui::tui::draw_form` the way
/// `core::plugin::lua_table_to_view_container`'s flattening says it should
/// — these tests build the `PluginView::fields` the flattening produces
/// directly (that flattening itself, and the Lua `kind`/`align` error
/// paths, are unit-tested in `core::plugin`'s own test module) and assert
/// on the painted cells, not on `Form`/`PluginView` state.
#[cfg(test)]
mod layout_container_painted_tests {
    use super::*;
    use crate::core::plugin_ui::{spacer_row, PluginView, ViewButton, ViewField, ViewFieldKind};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn cell_char(buf: &Buffer, x: u16, y: u16) -> char {
        buf[(x, y)].symbol().chars().next().unwrap_or(' ')
    }

    fn row_text(buf: &Buffer, y: u16, width: u16) -> String {
        (0..width).map(|x| cell_char(buf, x, y)).collect()
    }

    fn dump(buf: &Buffer, width: u16) -> String {
        (0..buf.area.height)
            .map(|y| format!("{y:>2}|{}|", row_text(buf, y, width)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn text_field(id: &str, label: &str) -> ViewField {
        ViewField {
            id: id.into(),
            label: label.into(),
            kind: ViewFieldKind::Text {
                value: String::new(),
                placeholder: String::new(),
            },
            ..Default::default()
        }
    }

    fn view_of(fields: Vec<ViewField>) -> PluginView {
        PluginView {
            id: "v".into(),
            schema_version: 1,
            fields,
            body: None,
            ..Default::default()
        }
    }

    fn paint(view: &PluginView, width: u16, height: u16) -> Buffer {
        let form = plugin_view_to_form("myview", view, 0, 0, false, None);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        quadraui::tui::draw_form(&mut buf, area, &form, &quadraui::Theme::default());
        buf
    }

    /// A column's `padding` — realised as a blank spacer row before the
    /// first child and after the last one (see `plugin_ui`'s "Layout
    /// containers" doc for the px-to-row quantization) — paints as a
    /// genuinely blank row, with the field's own label one row below it.
    #[test]
    fn column_padding_paints_a_blank_row_before_the_first_field() {
        let view = view_of(vec![spacer_row(), text_field("name", "Name")]);
        let width = 30u16;
        let buf = paint(&view, width, 2);

        assert_eq!(
            row_text(&buf, 0, width).trim(),
            "",
            "the padding spacer row must paint blank:\n{}",
            dump(&buf, width)
        );
        assert!(
            row_text(&buf, 1, width).contains("Name"),
            "the field's label must paint on the row after the padding:\n{}",
            dump(&buf, width)
        );
    }

    /// A column's `gap` — one blank spacer row between each pair of
    /// children — separates two fields' labels by exactly one blank
    /// painted row.
    #[test]
    fn column_gap_paints_one_blank_row_between_two_fields() {
        let view = view_of(vec![
            text_field("a", "A"),
            spacer_row(),
            text_field("b", "B"),
        ]);
        let width = 30u16;
        let buf = paint(&view, width, 3);

        let row_a = (0..3)
            .find(|&y| row_text(&buf, y, width).contains('A'))
            .expect("field A must paint");
        let row_b = (0..3)
            .find(|&y| row_text(&buf, y, width).contains('B'))
            .expect("field B must paint");
        assert_eq!(
            row_b,
            row_a + 2,
            "the gap spacer must separate the two fields by one blank row:\n{}",
            dump(&buf, width)
        );
        assert_eq!(
            row_text(&buf, row_a + 1, width).trim(),
            "",
            "the gap row itself must paint blank:\n{}",
            dump(&buf, width)
        );
    }

    /// A `row` of buttons flattens to one `Buttons` field (see
    /// `core::plugin::lua_table_to_view_container`'s doc) and paints both
    /// button labels on that single row — proving it is a horizontal row,
    /// not two stacked fields. `align = "end"` is accepted at parse time
    /// (`core::plugin`'s own tests cover that it is not a Lua error) but
    /// has no effect on *where* the row paints yet: right/center
    /// justification needs a `quadraui::FieldKind::ButtonRow` alignment
    /// knob that does not exist in quadraui 0.9 (filed upstream — see this
    /// issue's own doc), so both buttons still pack left, immediately
    /// after the (empty) label column, exactly like `"start"` would.
    #[test]
    fn a_nested_button_row_paints_both_buttons_on_one_row() {
        let view = view_of(vec![
            text_field("name", "Name"),
            ViewField {
                kind: ViewFieldKind::Buttons {
                    buttons: vec![
                        ViewButton {
                            id: "cancel".into(),
                            label: "Cancel".into(),
                            disabled: false,
                            variant: None,
                            icon: None,
                            tooltip: None,
                        },
                        ViewButton {
                            id: "connect".into(),
                            label: "Connect".into(),
                            disabled: false,
                            variant: None,
                            icon: None,
                            tooltip: None,
                        },
                    ],
                },
                ..Default::default()
            },
        ]);
        let width = 40u16;
        let buf = paint(&view, width, 2);

        let button_row = row_text(&buf, 1, width);
        assert!(
            button_row.contains("Cancel") && button_row.contains("Connect"),
            "both buttons must paint on the row's single line:\n{}",
            dump(&buf, width)
        );
        let cancel_at = button_row.find("Cancel").unwrap();
        let connect_at = button_row.find("Connect").unwrap();
        assert!(
            cancel_at < connect_at,
            "buttons must paint in declaration order:\n{}",
            dump(&buf, width)
        );
    }
}
