use super::*;

// ─── Activity bar adapter (#133) ─────────────────────────────────────────────

/// Build a `quadraui::ActivityBar` primitive from engine state.
///
/// Both backends call this once per frame, then delegate to
/// `quadraui::{tui,gtk}::draw_activity_bar`.
///
/// * `include_hamburger` — `true` for TUI (no native menu bar so the
///   hamburger item at keyboard-index 0 provides keyboard access to the
///   menu); `false` for GTK (the menu bar is a native GTK widget).
/// * `active_ext_panel` — name of the currently-active extension panel, if
///   any. GTK passes `engine.ext_panel_active.as_deref()`; TUI passes
///   `sidebar.ext_panel_name.as_deref()`.
///
/// Icons use `icons::*.s()` which respects the thread-local `USE_NERD_FONTS`
/// flag (see `icons` module docs) set at startup — both GTK and TUI set it
/// from `settings.use_nerd_fonts` on their own (single) render thread.
pub fn build_activity_bar(
    engine: &Engine,
    theme: &Theme,
    include_hamburger: bool,
    active_ext_panel: Option<&str>,
) -> quadraui::ActivityBar {
    use crate::core::engine::sidebar::{
        ext_panel_id, HAMBURGER_PANEL_ID, PANEL_AI, PANEL_BOARD, PANEL_DEBUG, PANEL_EXPLORER,
        PANEL_EXTENSIONS, PANEL_GIT, PANEL_SEARCH, PANEL_SETTINGS,
    };

    // #536: the keyboard ring is matched by *panel id*, not by re-deriving each
    // item's numeric toolbar index at the paint site. `Engine` owns the single
    // index↔id table (`activity_bar_item_id`), and the stepping itself is
    // quadraui's `AppShell` cursor (quadraui#386) — so the painted order and
    // the navigable order cannot drift apart here.
    let kbd_sel_id = if engine.activity_bar_focused {
        engine.activity_bar_selected_item_id()
    } else {
        None
    };
    let kbd_sel = |panel_id: &str| kbd_sel_id.as_deref() == Some(panel_id);
    let sb_visible = engine.app_shell.sidebar_visible();
    let has_ext = active_ext_panel.is_some();
    let active_id = engine.app_shell.active_panel_id().map(|w| w.as_str());

    let mut top = Vec::new();

    if include_hamburger {
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(HAMBURGER_PANEL_ID),
            icon: icons::HAMBURGER.s().into(),
            tooltip: "Menu".to_string(),
            is_active: false,
            is_keyboard_selected: kbd_sel(HAMBURGER_PANEL_ID),
        });
    }

    // (panel_id, icon, tooltip, activity_id)
    let fixed: [(&str, &str, &str, &str); 7] = [
        (
            PANEL_EXPLORER,
            icons::EXPLORER.s(),
            "Explorer (Ctrl+Shift+E)",
            "activity:explorer",
        ),
        (
            PANEL_SEARCH,
            icons::SEARCH.s(),
            "Search (Ctrl+Shift+F)",
            "activity:search",
        ),
        (
            PANEL_GIT,
            icons::SOURCE_CONTROL.s(),
            "Source Control",
            "activity:git",
        ),
        (
            PANEL_DEBUG,
            icons::RUN_AND_DEBUG.s(),
            "Run and Debug",
            "activity:debug",
        ),
        (
            PANEL_EXTENSIONS,
            icons::EXTENSIONS.s(),
            "Extensions",
            "activity:extensions",
        ),
        (PANEL_AI, icons::AI_CHAT.s(), "AI Assistant", "activity:ai"),
        (PANEL_BOARD, icons::BOARD.s(), "Board", "activity:board"),
    ];

    // #635 (Stage 6b): the pre-#1434 TUI shell's own `build_shell_config` derived
    // its own middle-panel `PanelDefinition` order from
    // `sidebar::FIXED_ACTIVITY_PANEL_IDS` rather than re-reading this array (it
    // can't — `fixed` is a local, and the two arrays carry different metadata
    // shapes), so this assertion is what actually keeps them from drifting
    // apart: a reordering here without updating the shared constant now trips
    // in any test that exercises `build_activity_bar` (every one does).
    debug_assert_eq!(
        fixed.map(|(panel_id, _, _, _)| panel_id),
        crate::core::engine::sidebar::FIXED_ACTIVITY_PANEL_IDS,
        "build_activity_bar's `fixed` panel-id order must match \
         sidebar::FIXED_ACTIVITY_PANEL_IDS"
    );

    for (panel_id, icon, tooltip, activity_id) in fixed {
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(activity_id),
            icon: icon.into(),
            tooltip: tooltip.to_string(),
            is_active: sb_visible && !has_ext && active_id == Some(panel_id),
            is_keyboard_selected: kbd_sel(panel_id),
        });
    }

    // Dynamic extension panels, sorted by name — the same order
    // `Engine::ext_activity_panels` (and therefore the keyboard ring) uses.
    let mut ext_panels: Vec<_> = engine.ext_panels.values().collect();
    ext_panels.sort_by(|a, b| a.name.cmp(&b.name));
    for panel in ext_panels.iter() {
        let is_active = sb_visible && active_ext_panel == Some(panel.name.as_str());
        top.push(quadraui::ActivityItem {
            id: quadraui::WidgetId::new(format!("activity:ext:{}", panel.name)),
            icon: panel.resolved_icon().to_string().into(),
            tooltip: panel.title.clone(),
            is_active,
            is_keyboard_selected: kbd_sel(&ext_panel_id(&panel.name)),
        });
    }

    let bottom = vec![quadraui::ActivityItem {
        id: quadraui::WidgetId::new("activity:settings"),
        icon: icons::SETTINGS.s().into(),
        tooltip: "Settings".to_string(),
        is_active: sb_visible && !has_ext && active_id == Some(PANEL_SETTINGS),
        is_keyboard_selected: kbd_sel(PANEL_SETTINGS),
    }];

    quadraui::ActivityBar {
        id: quadraui::WidgetId::new("activity-bar"),
        top_items: top,
        bottom_items: bottom,
        active_accent: Some(quadraui::Color::rgb(
            theme.activity_active_accent.r,
            theme.activity_active_accent.g,
            theme.activity_active_accent.b,
        )),
        selection_bg: Some(quadraui::Color::rgb(
            theme.cursor.r,
            theme.cursor.g,
            theme.cursor.b,
        )),
        // Signals to the quadraui backend that this bar owns the keyboard so
        // it intercepts KeyPressed as ActivityBarEvent::KeyPressed (Q#368).
        is_keyboard_focused: engine.activity_bar_focused,
    }
}

/// Adapt the engine-side `ExtPanelData` (extension-provided sidebar
/// panel) into a `quadraui::TreeView` for rendering via the shared
/// `draw_tree` primitive (#476).
///
/// Tree shape:
/// - Path `[s]` — section `s` header (`Decoration::Header`, chevron
///   reflects `section.expanded`).
/// - Path `[s, i]` — visible item `i` in section `s`. `indent` mirrors
///   `ExtPanelItem.indent + 1` so children of section headers start at
///   indent 1 (matching the legacy renderer). Tree-expandable items
///   carry `is_expanded: Some(item.expanded)` so the primitive draws
///   the chevron; non-expandable items leave `is_expanded` as `None`.
///
/// `ExtPanelStyle` maps to `Decoration` as:
/// - `Header → Decoration::Header`
/// - `Dim → Decoration::Muted`
/// - `Normal → Decoration::Normal`
/// - `Accent → Decoration::Normal` with the row text wrapped in a
///   `StyledSpan` coloured by `theme.keyword` (no first-class accent
///   decoration on the primitive).
///
/// Badges and action labels are concatenated into a single
/// right-aligned `Badge`, matching the legacy `[badge] ⟨action⟩ hint`
/// hint format. Separator rows (`item.is_separator`) become a single
/// `Decoration::Muted` row with a `─` glyph; the primitive doesn't
/// have a first-class separator decoration so this is a visual
/// approximation rather than a full-width rule.
///
/// `panel.selected` is a flat row index across visible rows (section
/// headers count, items in collapsed sections do not). The matching
/// `selected_path` is computed by walking the same flat enumeration
/// while emitting rows.
///
/// Tree-item expansion state (`engine.ext_panel_tree_expanded`) is
/// expected to have already been resolved into `item.expanded` by
/// `build_ext_panel_data` before this function is called.
pub fn ext_panel_to_tree_view(panel: &ExtPanelData, theme: &Theme) -> quadraui::TreeView {
    use crate::core::plugin::ExtPanelStyle;
    use quadraui::{
        Badge, Decoration, SelectionMode, StyledSpan, StyledText, TreeRow, TreeStyle, TreeView,
        WidgetId,
    };

    let accent_color = theme.keyword;
    let mut rows: Vec<TreeRow> = Vec::new();
    let mut selected_path: Option<Vec<u16>> = None;
    let mut flat_idx = 0usize;

    for (s, section) in panel.sections.iter().enumerate() {
        if panel.has_focus && flat_idx == panel.selected {
            selected_path = Some(vec![s as u16]);
        }
        rows.push(TreeRow {
            path: vec![s as u16],
            indent: 0,
            icon: None,
            text: StyledText::plain(section.name.clone()),
            badge: None,
            is_expanded: Some(section.expanded),
            decoration: Decoration::Header,
            edit: None,
        });
        flat_idx += 1;

        if !section.expanded {
            continue;
        }

        for (i, item) in section.items.iter().enumerate() {
            let row_path = vec![s as u16, i as u16];
            if panel.has_focus && flat_idx == panel.selected {
                selected_path = Some(row_path.clone());
            }

            if item.is_separator {
                rows.push(TreeRow {
                    path: row_path,
                    indent: 0,
                    icon: None,
                    text: StyledText::plain("\u{2500}".to_string()),
                    badge: None,
                    is_expanded: None,
                    decoration: Decoration::Muted,
                    edit: None,
                });
                flat_idx += 1;
                continue;
            }

            let (decoration, text) = match item.style {
                ExtPanelStyle::Header => (Decoration::Header, StyledText::plain(item.text.clone())),
                ExtPanelStyle::Dim => (Decoration::Muted, StyledText::plain(item.text.clone())),
                ExtPanelStyle::Accent => (
                    Decoration::Normal,
                    StyledText {
                        spans: vec![StyledSpan::with_fg(item.text.clone(), accent_color)],
                    },
                ),
                ExtPanelStyle::Normal => (Decoration::Normal, StyledText::plain(item.text.clone())),
            };

            let mut parts: Vec<String> = Vec::new();
            for badge in &item.badges {
                parts.push(format!("[{}]", badge.text));
            }
            for action in &item.actions {
                parts.push(format!("\u{27e8}{}\u{27e9}", action.label));
            }
            if !item.hint.is_empty() {
                parts.push(item.hint.clone());
            }
            let badge = if parts.is_empty() {
                None
            } else {
                Some(Badge::plain(parts.join(" ")))
            };

            let icon = if item.icon.is_empty() {
                None
            } else {
                Some(quadraui::Icon::new(item.icon.clone(), item.icon.clone()))
            };

            let is_expanded = if item.expandable {
                Some(item.expanded)
            } else {
                None
            };

            rows.push(TreeRow {
                path: row_path,
                indent: item.indent as u16 + 1,
                icon,
                text,
                badge,
                is_expanded,
                decoration,
                edit: None,
            });
            flat_idx += 1;
        }
    }

    TreeView {
        id: WidgetId::new("ext-panel-tree"),
        rows,
        selection_mode: SelectionMode::Single,
        selected_path,
        scroll_offset: panel.scroll_top,
        style: TreeStyle::default(),
        has_focus: panel.has_focus,
    }
}

// ─── Sidebar-panel-body composition (quadraui#1041/#1059, vimcode#1389) ────
//
// `quadraui::compose::sidebar_panel_body::SidebarPanelBody` composes
// "background fill, optional header/search chrome, body, optional
// scrollbar gutter" — the layer order sidebar-panel renderers used to
// hand-roll per backend (`docs/TUI_AUDIT_R2.md` §2.9). Its `render()`
// convenience method takes the body as `&dyn BackendWidget`, which
// requires `Self: Send + 'static` — a bound vimcode's *stateful* sidebar
// bodies (`TreeController`/`FormController`, `Rc<RefCell<_>>`-backed on
// the `!Send` `Engine`) can't satisfy: they're mutated in place by
// `populate_*` and read back through a live `&Engine` borrow, never
// rebuilt as an owned value. quadraui#1059 added `SidebarPanelBody::
// render_with`, which takes the body as `impl FnOnce(&mut dyn Backend,
// Rect)` instead — no `Send`/`'static` bound — so those callers now pass
// their stateful body as a closure directly (`App::paint_sidebar_panel_rung`
// PANEL_EXPLORER, `panels::render_explorer_sidebar_content`), instead of the
// hand-copied `paint_sidebar_panel_chrome` background+chrome half #1242
// needed before #1059 existed (deleted by #1389). [`ExtPanelTreeBody`] is
// the one rung with a genuinely owned per-frame body
// (`ext_panel_to_tree_view`'s fresh `TreeView`), so it uses the
// `SidebarPanelBody::render` (`&dyn BackendWidget`) path directly.
pub use quadraui::compose::sidebar_panel_body::{
    SidebarPanelBody, SidebarPanelBodyLayout, SidebarPanelChrome,
};

/// Build the search-only chrome (`SidebarPanelChrome::Search`, quadraui#1061)
/// for a sidebar panel whose header the shell's own `AppShell` already owns
/// (#1343 — the #1258 items 2/3 convergence). Shared by GTK's
/// `App::paint_sidebar_panel_rung` (`PANEL_SETTINGS`/`PANEL_EXTENSIONS`
/// arms) and the shipped TUI's `tui_main::panels::render_settings_panel`/
/// `render_ext_sidebar`, so the query/placeholder/active-tint look can't
/// drift between them the way #1256 found it had (GTK painted no search row
/// at all; the TUI painted both a search row *and* its own duplicate header
/// underneath the shell's).
///
/// Before quadraui#1061, this composed a bespoke single-row `StatusBar`
/// directly (`paint_sidebar_search_row`, deleted by #1391) because
/// `Backend::draw_settings_chrome` (behind [`SidebarPanelChrome::
/// HeaderAndSearch`] above) paints its header row unconditionally — no rect
/// made it paint a search-only strip without also painting a second header,
/// exactly the double-header bug #1256 fixed. `SidebarPanelChrome::Search`
/// is quadraui's own header-less variant of that composition (a single
/// synthetic `StatusBar` segment through `Backend::
/// draw_status_bar_interactive`), so this now just resolves the
/// query/placeholder `fg` the same way the old bespoke row did — the
/// `SidebarPanelBody`/`SidebarPanelChrome` composer pair above owns the
/// actual paint.
///
/// `fg` is muted (`theme.line_number_fg`) while the placeholder is showing
/// (`query` empty, not `active`), and full-strength (`theme.foreground`)
/// otherwise — [`SidebarPanelChrome::Search`] takes one `fg` for both
/// states (it has no theme of its own to consult), so callers resolve it
/// per-frame the same way here. `bg` is always `theme.completion_bg`; the
/// active-state tint (previously a discrete swap to `theme.fuzzy_selected_
/// bg`) is now `SidebarPanelBody::render_with`'s own `bg.lighten(0.08)`
/// while `active`.
pub fn search_only_chrome(
    query: &str,
    placeholder: &str,
    active: bool,
    theme: &Theme,
) -> SidebarPanelChrome {
    let show_placeholder = query.is_empty() && !placeholder.is_empty() && !active;
    let fg = if show_placeholder {
        theme.line_number_fg
    } else {
        theme.foreground
    };
    SidebarPanelChrome::Search {
        query: query.to_string(),
        placeholder: placeholder.to_string(),
        active,
        fg,
        bg: theme.completion_bg,
    }
}

/// Owned per-frame body for the plugin extension panel (`ext:<name>`) —
/// unlike the stateful controllers `SidebarPanelBody::render_with` exists
/// for (the module note above), [`ext_panel_to_tree_view`]'s output is
/// already a fresh, owned `quadraui::TreeView`, so it satisfies
/// `BackendWidget: Send + 'static`. Public field so a caller can read the
/// `TreeView` back out after the borrow ends (e.g. to feed
/// `Backend::tree_layout`).
pub struct ExtPanelTreeBody(pub quadraui::TreeView);

impl quadraui::BackendWidget for ExtPanelTreeBody {
    fn render(&self, backend: &mut dyn quadraui::Backend, rect: quadraui::Rect) {
        backend.draw_tree(rect, &self.0);
    }
}

pub use crate::core::engine::ExplorerRow;

/// Adapt a flat explorer row list into a `quadraui::TreeView` for the
/// shared `draw_tree` primitive. Each backend drives its own flat-row
/// model (GTK via `ExplorerState`, Win-GUI via `WinSidebar`) and calls
/// this adapter on every draw.
///
/// Overlays per-row git status letters and LSP diagnostic counts via
/// `engine.explorer_indicators()` — the cached indicator map keyed by
/// canonical path. Directories get a folder glyph; files get the
/// extension-based icon from `icons::file_icon`.
/// Adapt the picker panel's `PickerPanel` render data into a generic
/// `quadraui::Palette` for rendering through the shared primitive.
///
/// Phase A.4 scope: flat-list palettes only. Returns `None` when the
/// caller should fall through to the legacy renderer:
/// - `preview.is_some()` — file / symbol picker with right-side preview pane
/// - any item has `depth > 0` or `expandable` — tree-structured picker
///
/// When `Some(Palette)` is returned, the backend can render the full
/// modal via `quadraui_tui::draw_palette` (TUI) or `quadraui_gtk::draw_palette`
/// (GTK, when A.4b ships).
pub fn picker_panel_to_palette(picker: &PickerPanel) -> quadraui::Palette {
    use quadraui::{Palette, PaletteItem, PalettePreview, StyledText, WidgetId};

    let items: Vec<PaletteItem> = picker
        .items
        .iter()
        .map(|it| PaletteItem {
            text: StyledText::plain(&it.display),
            detail: it.detail.as_deref().map(StyledText::plain),
            icon: None,
            match_positions: it.match_positions.clone(),
            depth: it.depth,
            expandable: it.expandable,
            expanded: it.expanded,
        })
        .collect();

    let preview = picker.preview.as_ref().map(|lines| {
        let highlight_line = lines.iter().position(|&(_, _, hl)| hl);
        PalettePreview {
            lines: lines
                .iter()
                .map(|(line_num, text, _)| StyledText::plain(format!("{line_num:4}: {text}")))
                .collect(),
            title: None,
            scroll_offset: picker.preview_scroll,
            highlight_line,
        }
    });

    Palette {
        id: WidgetId::new("picker"),
        title: picker.title.clone(),
        query: picker.query.clone(),
        query_cursor: picker.query.len(),
        items,
        selected_idx: picker.selected_idx,
        scroll_offset: picker.scroll_top,
        total_count: picker.total_count,
        has_focus: true,
        show_query: true,
        create_label: None,
        preview,
        mode: quadraui::PaletteMode::List,
    }
}

/// Convert `Engine`'s settings state into a generic `quadraui::Form`
/// for rendering through either `quadraui_tui::draw_form` (A.3b) or
/// `quadraui_gtk::draw_form` (A.3c). Backend-agnostic; reads only
/// engine fields.
///
/// Scope: covers the scrollable field list. Callers still handle the
/// panel header / search input / scrollbar themselves.
///
/// Field type mapping:
/// - `CoreCategory` / `ExtCategory` → `FieldKind::Label` (collapsible header)
/// - `CoreSetting` with `Bool` → `FieldKind::Toggle`
/// - `CoreSetting` currently being inline-edited (`engine.settings_editing
///   == Some(idx)`) → `FieldKind::TextInput` sourced from
///   `engine.settings_edit_buf`, with `cursor` set to the buffer's byte
///   length (edits are append/backspace-only — the cursor always sits at
///   the end, see `Engine::handle_settings_key`).
/// - `CoreSetting` with any other type, not being edited → `FieldKind::ReadOnly`
///   (enum cycling / numeric / string values still work — keys are
///   handled by `engine.handle_settings_key()`; the adapter just shows
///   the current value)
/// - `ExtSetting` mapped analogously via the manifest's declared type,
///   using `engine.ext_settings_editing` for the inline-edit check.
pub fn settings_to_form(engine: &Engine) -> quadraui::Form {
    use crate::core::engine::SettingsRow;
    use crate::core::settings::{setting_categories, SettingType, SETTING_DEFS};
    use quadraui::{FieldKind, Form, FormField, StyledText, WidgetId};

    let flat = engine.settings_flat_list();
    let cats = setting_categories();

    let mut fields: Vec<FormField> = Vec::with_capacity(flat.len());
    for row in &flat {
        let field = match row {
            SettingsRow::CoreCategory(cat_idx) => {
                let collapsed = *cat_idx < engine.settings_collapsed.len()
                    && engine.settings_collapsed[*cat_idx];
                let arrow = if collapsed { "▶ " } else { "▼ " };
                let cat_name = cats.get(*cat_idx).copied().unwrap_or("?");
                FormField {
                    id: WidgetId::new(format!("cat-{}", cat_idx)),
                    label: StyledText::plain(format!("{}{}", arrow, cat_name)),
                    kind: FieldKind::Label,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
            SettingsRow::ExtCategory(name) => {
                let collapsed = engine
                    .ext_settings_collapsed
                    .get(name)
                    .copied()
                    .unwrap_or(false);
                let arrow = if collapsed { "▶ " } else { "▼ " };
                let display = engine
                    .ext_available_manifests()
                    .into_iter()
                    .find(|m| &m.name == name)
                    .map(|m| m.display_name.clone())
                    .unwrap_or_else(|| name.clone());
                FormField {
                    id: WidgetId::new(format!("ext-cat-{}", name)),
                    label: StyledText::plain(format!("{}{}", arrow, display)),
                    kind: FieldKind::Label,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
            SettingsRow::CoreSetting(idx) => {
                let def = &SETTING_DEFS[*idx];
                let kind = if engine.settings_editing == Some(*idx) {
                    FieldKind::TextInput {
                        value: engine.settings_edit_buf.clone(),
                        placeholder: String::new(),
                        cursor: Some(engine.settings_edit_buf.len()),
                        selection_anchor: None,
                    }
                } else {
                    let value_str = engine.settings.get_value_str(def.key);
                    match def.setting_type {
                        SettingType::Bool => FieldKind::Toggle {
                            value: value_str == "true",
                        },
                        _ => FieldKind::ReadOnly {
                            value: StyledText::plain(value_str),
                        },
                    }
                };
                FormField {
                    id: WidgetId::new(format!("setting-{}", idx)),
                    label: StyledText::plain(def.label),
                    kind,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
            SettingsRow::ExtSetting(ext_name, key) => {
                let editing_this = engine
                    .ext_settings_editing
                    .as_ref()
                    .is_some_and(|(en, ek)| en == ext_name && ek == key);
                let def_opt = engine.find_ext_setting_def(ext_name, key);
                let label_str = def_opt
                    .as_ref()
                    .map(|d| {
                        if d.label.is_empty() {
                            key.clone()
                        } else {
                            d.label.clone()
                        }
                    })
                    .unwrap_or_else(|| key.clone());
                let kind = if editing_this {
                    FieldKind::TextInput {
                        value: engine.settings_edit_buf.clone(),
                        placeholder: String::new(),
                        cursor: Some(engine.settings_edit_buf.len()),
                        selection_anchor: None,
                    }
                } else {
                    let value_str = engine.get_ext_setting(ext_name, key);
                    let is_bool = def_opt.as_ref().is_some_and(|d| d.r#type == "bool");
                    if is_bool {
                        FieldKind::Toggle {
                            value: value_str == "true",
                        }
                    } else {
                        FieldKind::ReadOnly {
                            value: StyledText::plain(value_str),
                        }
                    }
                };
                FormField {
                    id: WidgetId::new(format!("ext-setting-{}-{}", ext_name, key)),
                    label: StyledText::plain(label_str),
                    kind,
                    hint: StyledText::default(),
                    disabled: false,
                    validation: None,
                }
            }
        };
        fields.push(field);
    }

    let focused_field = fields.get(engine.settings_selected).map(|f| f.id.clone());

    Form {
        id: WidgetId::new("settings"),
        fields,
        focused_field,
        scroll_offset: engine.settings_scroll_top,
        has_focus: engine.settings_has_focus,
    }
}

/// Populate the engine's `settings_form_controller` with current form
/// data and scroll state. Call before `FormController::render()` or
/// `FormController::handle()`.
pub fn populate_settings_form_controller(engine: &Engine) {
    let form = settings_to_form(engine);
    let mut fc = engine.settings_form_controller.borrow_mut();
    fc.set_form(form);
    fc.set_scroll_offset(engine.settings_scroll_top);
    fc.set_has_focus(engine.settings_has_focus);
}

/// Route a pointer event over the Settings panel through the shared
/// `quadraui::FormController` and apply the result to engine state.
///
/// `rect` is the panel's content area in the caller's own coordinate space —
/// the *same* rect the last frame passed to
/// `FormController::render_and_cache`, since `handle_cached` re-derives its
/// row layout from it. Returns `true` when the event was consumed.
///
/// This is the click twin of [`populate_settings_form_controller`], and it
/// exists so neither backend has to re-derive the panel's row geometry by
/// hand: before #544 GTK computed `row_h = line_height * 1.4`, a header/search
/// band and a scrollbar gutter from a `DrawingArea`'s own width/height, none of
/// which survive the ShellApp migration (there is no per-panel DrawingArea any
/// more, so every one of those numbers read back `0`). `FormController` already
/// owns all of it and is the only thing that painted the rows.
///
/// Activation policy matches the keyboard path and the pre-migration GTK/TUI
/// mouse paths: a `Toggle` field flips on a single click, a category header
/// expands/collapses on a single click, and a value row selects on a single
/// click but only *activates* (opens the inline editor / cycles an enum) on a
/// double click.
///
/// # Row geometry is not offset from the paint (#983 → #1028)
///
/// #983 reported (v0.11.0) that a click low in a settings row selected the
/// row *below* it, and blamed the `handle_cached` path below — the
/// `backend: None` branch of `quadraui::FormController::click_inner`, which
/// re-derives its row layout from `lh` alone instead of asking the backend.
/// #1028 measured it on a real GTK frame rather than inferring it from
/// glyph bounds, and there is no offset to fix: the `▼ LSP` category row's
/// background is *painted* over `y ∈ [389, 421)` and this function resolves
/// clicks to it over exactly `y ∈ [389, 421)`. Both paths agree because the
/// row pitch is the same pure function of `lh` on both sides —
/// `FormController`'s cached `row_height(lh)` and `GtkBackend::form_layout`'s
/// `layout_metrics::form_row_height(lh)` are both `(lh * 1.4).round()`; the
/// cached path only approximates the *horizontal* text measure, which row
/// resolution does not use. The ext-panel/`SidebarSystem` rows behave the
/// same way (`[741, 773)` painted, `[741, 773)` hit).
///
/// What #983's reporter actually clicked was the next row's own top padding
/// — a row's band is ~32px tall around a ~23px glyph, so each row owns a
/// ~4.5px strip of background above and below its label. Do **not** add a
/// vimcode-side offset here to "fix" that; it would break the agreement
/// above. `harness::row_click_in_its_painted_band_hits_its_own_row` locks
/// the two bands together, on both panels and on both sides of the glyph.
pub fn handle_settings_form_ui_event(
    engine: &mut Engine,
    event: &quadraui::UiEvent,
    rect: quadraui::Rect,
) -> bool {
    use crate::core::engine::SettingsRow;

    // `FormController` has no `DoubleClick` arm — probe with the equivalent
    // press and remember that the caller asked for activation.
    let (probe, activate_row) = match event {
        quadraui::UiEvent::DoubleClick { widget, position } => (
            quadraui::UiEvent::MouseDown {
                widget: widget.clone(),
                button: quadraui::MouseButton::Left,
                position: *position,
                modifiers: quadraui::Modifiers::default(),
            },
            true,
        ),
        other => (other.clone(), false),
    };

    populate_settings_form_controller(engine);
    let result = engine
        .settings_form_controller
        .borrow_mut()
        .handle_cached(&probe, rect);

    let sync_scroll = |engine: &mut Engine| {
        let offset = engine.settings_form_controller.borrow().scroll_offset();
        engine.settings_scroll_top = offset;
    };

    match result {
        quadraui::FormControllerEvent::Ignored => false,
        quadraui::FormControllerEvent::ScrollChanged | quadraui::FormControllerEvent::Consumed => {
            sync_scroll(engine);
            true
        }
        quadraui::FormControllerEvent::FormAction(action) => {
            let (id, activates) = match action {
                quadraui::FormEvent::ToggleChanged { id, .. } => (id, true),
                quadraui::FormEvent::ButtonClicked { id } => (id, true),
                quadraui::FormEvent::FocusChanged { id } => (id, activate_row),
                _ => return true,
            };
            // The form's fields are built 1:1 from `settings_flat_list()`
            // (see `settings_to_form`), so the field's position *is* the flat
            // index `settings_selected` indexes — read it off the controller
            // rather than re-parsing the id string, so the two can't drift.
            let idx = engine
                .settings_form_controller
                .borrow()
                .form()
                .and_then(|f| f.fields.iter().position(|field| field.id == id));
            let Some(idx) = idx else {
                return true;
            };
            engine.settings_has_focus = true;
            engine.settings_selected = idx;
            sync_scroll(engine);
            let is_category = matches!(
                engine.settings_flat_list().get(idx),
                Some(SettingsRow::CoreCategory(_)) | Some(SettingsRow::ExtCategory(_))
            );
            if activates || is_category {
                engine.handle_settings_key("Return", false, None);
            }
            true
        }
    }
}
