//! The welcome panel: painted in place of the editor band when the engine
//! has no open workspace (`Engine::has_no_workspace`) — a heading, an "Open
//! Folder" button, and a `recent_workspaces` list so a fresh launch reads as
//! an intentional landing screen rather than an empty window.
//!
//! Render + layout-cache only. Routing a click against the cached button/list
//! layout (`Engine::welcome_button_layout`, `Engine::welcome_list_layout`)
//! back into `Engine::open_folder`/`open_workspace` is a separate layer's job.

use super::*;

/// Paint the welcome panel into `rect` (the first window's rect, or the
/// editor band's rect if there is no window yet) and cache the button/list
/// layouts `Engine::welcome_button_layout`/`welcome_list_layout` hold so a
/// later click can be hit-tested against exactly what was last painted.
///
/// With no `recent_workspaces`, only the empty-state heading and the Open
/// Folder button are painted (no list, and any stale cached list layout is
/// cleared so a leftover rect from a previous frame can't be hit).
pub(crate) fn paint_welcome_panel(
    engine: &Engine,
    theme: &Theme,
    rect: quadraui::Rect,
    backend: &mut dyn quadraui::Backend,
) {
    use quadraui::{
        Button, Decoration, ListItem, ListView, StyledSpan, StyledText, TextDisplay,
        TextDisplayLine, WidgetId,
    };

    if rect.width <= 0.0 || rect.height <= 0.0 {
        engine.welcome_button_layout.replace(None);
        engine.welcome_list_layout.replace(None);
        return;
    }

    let recents = &engine.session.recent_workspaces;
    let unit_h = backend.line_height().max(1.0);
    let pad = unit_h * 0.5;

    let heading_text = if recents.is_empty() {
        "Open a folder to get started"
    } else {
        "Welcome"
    };
    let heading_rect = quadraui::Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.width - pad * 2.0).max(0.0),
        unit_h * 2.0,
    );
    let heading = TextDisplay {
        id: WidgetId::new("welcome.heading"),
        lines: vec![TextDisplayLine {
            spans: vec![StyledSpan::plain(heading_text)],
            decoration: Decoration::default(),
            timestamp: None,
        }],
        scroll_offset: 0,
        auto_scroll: false,
        max_lines: 0,
        has_focus: false,
        title: None,
        show_scrollbar: false,
    };
    backend.draw_text_display(heading_rect, &heading);

    let button_width = (rect.width - pad * 2.0).max(0.0).min(unit_h * 16.0);
    let button_rect = quadraui::Rect::new(
        rect.x + pad,
        heading_rect.y + heading_rect.height + pad,
        button_width,
        unit_h * 1.5,
    );
    // quadraui's `Button` has no color/style override (`id`, `label`,
    // `variant`, `disabled`, `focused` only, `#[non_exhaustive]`), so this
    // can only be a styled button, not the underlined "link" affordance VS
    // Code uses — the recent-folders rows below get the link color instead.
    let button = Button::new(WidgetId::new("welcome.open_folder"), "Open Folder");
    backend.draw_button(button_rect, &button);
    let button_layout = backend.button_layout(button_rect, &button);
    engine
        .welcome_button_layout
        .replace(Some((button_rect, button_layout)));

    if recents.is_empty() {
        engine.welcome_list_layout.replace(None);
        return;
    }

    let list_rect_y = button_rect.y + button_rect.height + pad;
    let list_rect = quadraui::Rect::new(
        rect.x + pad,
        list_rect_y,
        (rect.width - pad * 2.0).max(0.0),
        (rect.y + rect.height - list_rect_y).max(0.0),
    );
    // Most-recently-opened first; `recent_workspaces` is pushed-to-end/
    // deduped/capped-at-10 by `SessionState`'s own mutator.
    let items: Vec<ListItem> = recents
        .iter()
        .rev()
        .map(|path| ListItem {
            text: StyledText::colored(path.display().to_string(), theme.md_link),
            icon: None,
            detail: None,
            decoration: Decoration::default(),
        })
        .collect();
    let list = ListView {
        id: WidgetId::new("welcome.recent_list"),
        title: Some(StyledText::plain("Recent Folders")),
        items,
        selected_idx: 0,
        scroll_offset: 0,
        has_focus: false,
        bordered: false,
        h_scroll: 0,
        max_content_width: None,
        show_v_scrollbar: true,
    };
    backend.draw_list(list_rect, &list);
    let list_layout = backend.list_layout(list_rect, &list);
    engine
        .welcome_list_layout
        .replace(Some((list_rect, list_layout)));
}
