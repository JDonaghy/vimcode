//! The welcome panel: painted in place of the editor band when the engine
//! has no open workspace (`Engine::has_no_workspace`) — a heading, an "Open
//! Folder" button, and a `recent_workspaces` list so a fresh launch reads as
//! an intentional landing screen rather than an empty window.
//!
//! [`paint_welcome_panel`] paints and caches layout; [`welcome_panel_click_target`]
//! resolves a click against that cache into a [`ClickTarget`] — same
//! "paint caches, click reads" split `plugin_view_body.rs` uses for its own
//! lists. Applying the resolved target (opening the dialog, opening a
//! recent workspace) is `click.rs`'s job, not this file's.
//!
//! Keyboard focus (j/k to move, Enter to activate) is tracked by
//! `Engine::welcome_focus` (`core::engine::WelcomeFocus`) and handled
//! entirely in `Engine::handle_key` — this file only reads it to paint the
//! button/list focus highlight.

use super::*;

/// Paint the welcome panel into `rect` (the first window's rect, or the
/// editor band's rect if there is no window yet) and cache the button/list
/// layouts `Engine::welcome_button_layout`/`welcome_list_layout` hold so a
/// later click can be hit-tested against exactly what was last painted.
///
/// With no `recent_workspaces`, the "Welcome" heading stays put and the Open
/// Folder button is painted below it; a second, muted line explains there's
/// nothing to pick from yet (no list, and any stale cached list layout is
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
    // Whole multiples of `line_height` throughout, matching the rest of the
    // editor band's whole-cell units: a half- or 1.5-line offset is
    // meaningless on a TUI backend, whose rows are integral.
    let unit_h = backend.line_height().max(1.0);
    let pad = unit_h;

    let mut heading_lines = vec![TextDisplayLine {
        spans: vec![StyledSpan::with_fg("Welcome", theme.default_fg)],
        decoration: Decoration::default(),
        timestamp: None,
    }];
    if recents.is_empty() {
        heading_lines.push(TextDisplayLine {
            spans: vec![StyledSpan::with_fg(
                "Open a folder to get started",
                theme.annotation_fg,
            )],
            decoration: Decoration::default(),
            timestamp: None,
        });
    }
    let heading_rect = quadraui::Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.width - pad * 2.0).max(0.0),
        unit_h * 2.0,
    );
    let heading = TextDisplay {
        id: WidgetId::new("welcome.heading"),
        lines: heading_lines,
        scroll_offset: 0,
        auto_scroll: false,
        max_lines: 0,
        has_focus: false,
        title: None,
        show_scrollbar: false,
    };
    backend.draw_text_display(heading_rect, &heading);

    let button_width = (rect.width - pad * 2.0).clamp(0.0, unit_h * 16.0);
    let button_rect = quadraui::Rect::new(
        rect.x + pad,
        heading_rect.y + heading_rect.height + pad,
        button_width,
        unit_h,
    );
    // quadraui's `Button` has no color/style override (`id`, `label`,
    // `variant`, `disabled`, `focused` only, `#[non_exhaustive]`), so this
    // can only be a styled button, not the underlined "link" affordance VS
    // Code uses — the recent-folders rows below get the link color instead.
    let mut button = Button::new(WidgetId::new("welcome.open_folder"), "Open Folder");
    button.focused = engine.welcome_focus == crate::core::engine::WelcomeFocus::OpenFolderButton;
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
    let recent_focus_idx = match engine.welcome_focus {
        crate::core::engine::WelcomeFocus::RecentItem(i) => Some(i),
        crate::core::engine::WelcomeFocus::OpenFolderButton => None,
    };
    let list = ListView {
        id: WidgetId::new("welcome.recent_list"),
        title: Some(StyledText::plain("Recent Folders")),
        items,
        selected_idx: recent_focus_idx.unwrap_or(0),
        scroll_offset: 0,
        has_focus: recent_focus_idx.is_some(),
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

/// Resolve `pos` (absolute, same units the last frame painted the panel
/// into) against the welcome panel's cached button/list layouts.
///
/// Gates on `Engine::has_no_workspace` rather than cache presence alone:
/// opening a workspace doesn't clear either cache, only painting a
/// non-welcome editor band does (and that only happens on the next frame),
/// so a stale cache from the last welcome frame must not resolve a hit once
/// the panel is gone.
pub(crate) fn welcome_panel_click_target(
    engine: &Engine,
    pos: quadraui::Point,
) -> Option<ClickTarget> {
    if !engine.has_no_workspace() {
        return None;
    }

    if let Some((_, button_layout)) = engine.welcome_button_layout.borrow().as_ref() {
        if button_layout.hit_test(pos) == quadraui::ButtonHit::Body {
            return Some(ClickTarget::WelcomeOpenFolder);
        }
    }

    if let Some((list_rect, list_layout)) = engine.welcome_list_layout.borrow().as_ref() {
        if let quadraui::ListViewHit::Item(idx) =
            list_layout.hit_test(pos.x - list_rect.x, pos.y - list_rect.y)
        {
            // Most-recently-opened first, mirroring the `.rev()` the paint
            // side above built the list items from.
            let recents = &engine.session.recent_workspaces;
            if let Some(path) = recents
                .len()
                .checked_sub(1 + idx)
                .map(|i| recents[i].clone())
            {
                return Some(ClickTarget::WelcomeRecent(path));
            }
        }
    }

    None
}
