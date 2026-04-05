use super::*;

/// Generate the full CSS string with colors taken from the active theme.
pub(super) fn make_theme_css(theme: &Theme) -> String {
    let bar_bg = theme.tab_bar_bg.to_hex();
    // For light themes, use foreground color for active icons (status_fg is white).
    let bar_fg = if theme.is_light() {
        theme.foreground.to_hex()
    } else {
        theme.status_fg.to_hex()
    };
    let editor_bg = theme.background.to_hex();
    let text_fg = theme.foreground.to_hex();
    let accent = theme.function.to_hex();
    let sel_bg = theme.fuzzy_selected_bg.to_hex();
    // Selected item text: white on dark selection bg for both light/dark themes.
    let sel_fg = if theme.is_light() {
        theme.foreground.darken(0.9).to_hex()
    } else {
        bar_fg.clone()
    };
    let hover_bg = theme.tab_active_bg.to_hex();
    let dim_fg = theme.line_number_fg.to_hex();
    let entry_bg = theme.active_background.to_hex();
    let border_col = theme.separator.to_hex();
    let sb_thumb = theme.scrollbar_thumb.to_hex();
    let comment_fg = theme.comment.to_hex();
    format!(
        r#"
        /* Activity Bar */
        .activity-bar {{
            background-color: {bar_bg};
            border-right: 1px solid {border_col};
        }}

        .activity-button {{
            background: transparent;
            border: none;
            border-radius: 0;
            font-family: 'Symbols Nerd Font', monospace;
            font-size: 24px;
            color: {dim_fg};
            padding: 0;
        }}

        .activity-button:hover {{
            background-color: {hover_bg};
            color: {bar_fg};
        }}

        .activity-button.active {{
            color: {bar_fg};
            border-left: 2px solid {accent};
        }}

        .custom-titlebar {{
            background-color: {bar_bg};
        }}

        /* Window control buttons (min/max/close) */
        .window-control {{
            color: {dim_fg};
        }}
        .window-control:hover {{
            background-color: {hover_bg};
            color: {bar_fg};
        }}
        .window-control:active {{
            background-color: {hover_bg};
        }}

        /* Sidebar */
        .sidebar-container {{
            background-color: {bar_bg};
        }}

        .sidebar {{
            background-color: {bar_bg};
            border-right: 1px solid {border_col};
        }}

        .sidebar label {{
            color: {bar_fg};
        }}

        /* Tree View */
        treeview {{
            background-color: {bar_bg};
            color: {bar_fg};
            border: none;
            outline: none;
        }}

        treeview:selected {{
            background-color: {sel_bg};
            color: {sel_fg};
            border-left: 3px solid {accent};
        }}

        treeview:selected:focus {{
            background-color: {sel_bg};
            color: {sel_fg};
        }}

        treeview row:hover {{
            background-color: {hover_bg};
        }}

        treeview row {{
            padding: 4px 8px;
            min-height: 22px;
        }}

        treeview expander {{
            min-width: 16px;
            min-height: 16px;
        }}

        treeview expander:checked {{
            color: {bar_fg};
        }}

        treeview expander:not(:checked) {{
            color: {dim_fg};
        }}

        /* Inline editing entry (rename / new file/folder) */
        treeview entry {{
            border: 1px solid {accent};
            border-radius: 2px;
            padding: 2px 4px;
            background-color: {editor_bg};
            color: {text_fg};
            min-height: 20px;
        }}

        /* Search results */
        .search-results-list {{
            background-color: {bar_bg};
            color: {bar_fg};
        }}

        .search-results-list > row {{
            background-color: {bar_bg};
            color: {bar_fg};
            padding: 2px 4px;
        }}

        .search-results-list > row:selected,
        .search-results-list > row:selected:focus {{
            background-color: {sel_bg};
        }}

        .search-results-list > row:selected label,
        .search-results-list > row:selected:focus label {{
            color: {sel_fg};
        }}

        .search-results-scroll {{
            background-color: {bar_bg};
        }}

        /* Search file header */
        .search-file-header {{
            color: {accent};
            font-weight: bold;
            font-size: 12px;
        }}

        /* Search input entry inside sidebar */
        .sidebar entry {{
            background-color: {entry_bg};
            color: {text_fg};
            border: 1px solid {border_col};
            border-radius: 2px;
            padding: 4px;
            min-width: 0;
        }}

        .sidebar entry > text {{
            min-width: 0;
        }}

        .sidebar searchentry {{
            min-width: 0;
        }}

        .sidebar searchentry > text {{
            min-width: 0;
        }}

        .sidebar entry:focus {{
            border-color: {accent};
        }}

        /* Search toggle buttons */
        .search-toggle-btn {{
            background: transparent;
            color: {dim_fg};
            border: 1px solid {border_col};
            border-radius: 2px;
            padding: 2px 6px;
            min-width: 0;
            min-height: 0;
            font-size: 12px;
        }}
        .search-toggle-btn:hover {{
            background-color: {hover_bg};
        }}
        .search-toggle-btn:checked {{
            background-color: {accent};
            color: {editor_bg};
            border-color: {accent};
        }}

        /* Settings form widgets — theme-aware */
        .settings-category-header {{
            color: {dim_fg};
        }}
        .sidebar spinbutton {{
            background-color: {entry_bg};
            color: {text_fg};
            border: 1px solid {border_col};
        }}
        .sidebar spinbutton entry {{
            color: {text_fg};
        }}
        .sidebar spinbutton button {{
            background-color: {entry_bg};
            color: {text_fg};
        }}
        .sidebar spinbutton button:hover {{
            background-color: {hover_bg};
        }}
        .sidebar dropdown {{
            background-color: {entry_bg};
            color: {text_fg};
            border: 1px solid {border_col};
        }}
        .sidebar dropdown button {{
            color: {text_fg};
        }}
        .sidebar dropdown button:hover {{
            background-color: {hover_bg};
        }}
        popover.menu contents,
        popover contents {{
            background-color: {entry_bg};
            color: {text_fg};
        }}
        popover.menu modelbutton,
        popover modelbutton {{
            color: {text_fg};
        }}
        popover.menu modelbutton:hover,
        popover modelbutton:hover {{
            background-color: {sel_bg};
            color: {sel_fg};
        }}
        .sidebar entry {{
            background-color: {entry_bg};
            color: {text_fg};
            border: 1px solid {border_col};
        }}

        /* Scrollbar — theme-aware overrides */
        scrollbar slider {{
            background: alpha({sb_thumb}, 0.5);
        }}
        scrollbar slider:hover {{
            background: alpha({sb_thumb}, 0.7);
        }}
        scrollbar slider:active {{
            background: alpha({sb_thumb}, 0.9);
        }}

        /* Horizontal editor scrollbar — theme-aware */
        .h-editor-scrollbar slider {{
            background: alpha({sb_thumb}, 0.45);
        }}
        .h-editor-scrollbar slider:hover {{
            background: alpha({sb_thumb}, 0.7);
        }}

        /* Find/Replace dialog — theme-aware */
        .find-dialog {{
            background-color: {editor_bg};
            border: 1px solid {border_col};
        }}
        .find-dialog entry {{
            background-color: {entry_bg};
            color: {text_fg};
            border: 1px solid {border_col};
        }}
        .find-dialog button {{
            border: 1px solid {border_col};
            color: {text_fg};
        }}
        .find-dialog button:hover {{
            background-color: {hover_bg};
        }}
        .find-match-count {{
            color: {comment_fg};
        }}
        "#
    )
}

/// Static structural CSS that never changes with the theme.
/// Theme-specific colours live in `make_theme_css()` and are appended after this.
pub(super) const STATIC_CSS: &str = "
        /* Custom titlebar — matches status bar color.
           CSD provides edge resize handles; WindowHandle enables drag-to-move. */
        .custom-titlebar {
            background-color: transparent;
            min-height: 0;
            padding: 0;
            margin: 0;
            border: none;
            box-shadow: none;
        }
        headerbar {
            min-height: 0;
            padding: 0;
            margin: 0;
            border: none;
            box-shadow: none;
            background: transparent;
        }

        /* VSCode UI font stack — 'Segoe UI' on Windows, 'Ubuntu' on Ubuntu,
           system-ui/sans elsewhere.  13px matches VSCode default UI size. */
        .sidebar,
        .sidebar *,
        .sidebar-header,
        .sidebar-title,
        .search-results-list,
        .search-results-list *,
        .search-file-header {
            font-family: 'Segoe UI', system-ui, -apple-system, 'Ubuntu', 'Droid Sans', sans-serif;
            font-size: 13px;
        }

        /* Window control buttons — VSCode style (transparent bg, subtle hover) */
        .window-control {
            background: transparent;
            border: none;
            border-radius: 0;
            font-size: 13px;
            padding: 0;
            min-width: 46px;
            min-height: 30px;
        }
        /* Close button: red on hover, matching Windows/VSCode */
        .window-control:last-child:hover {
            background-color: #e81123;
            color: #ffffff;
        }
        .window-control:last-child:active {
            background-color: #f1707a;
            color: #ffffff;
        }

        /* Activity bar, sidebar, treeview: see make_theme_css() — applied dynamically */
        
        /* treeview: see make_theme_css() — applied dynamically */

        /* Thin overlay scrollbars */
        scrollbar {
            background: transparent;
            transition: opacity 200ms ease-out;
        }

        scrollbar.vertical {
            min-width: 4px;
            padding: 0;
            margin: 0;
        }

        scrollbar.horizontal {
            min-height: 4px;
        }

        scrollbar.horizontal slider {
            min-height: 4px;
        }

        scrollbar slider {
            min-width: 4px;
            min-height: 40px;
            padding: 0;
            margin: 0;
            background: rgba(255, 255, 255, 0.3);
            border-radius: 2px;
        }

        scrollbar slider:hover {
            background: rgba(255, 255, 255, 0.5);
        }

        scrollbar slider:active {
            background: rgba(255, 255, 255, 0.7);
        }

        /* Scrollbars — subtle but always visible */
        scrollbar:not(:hover):not(:active) {
            opacity: 0.4;
        }

        /* search-results-list, sidebar entry, search-toggle-btn: see make_theme_css() */
        /* Search toggle buttons (Aa / Ab| / .*) — base layout only, colors via make_theme_css */
        .search-toggle-btn {
            background: transparent;
            color: #808080;
            border: 1px solid #3e3e42;
            border-radius: 2px;
            padding: 2px 6px;
            min-width: 0;
            min-height: 0;
            font-size: 12px;
        }
        .search-toggle-btn:hover {
            background-color: #2a2d2e;
        }
        .search-toggle-btn:checked {
            background-color: #0e639c;
            color: #ffffff;
            border-color: #0e639c;
        }

        /* Horizontal editor scrollbar — overlays the bottom of editor content.
           Semi-transparent like VSCode so text beneath is still visible.
           min-height/min-width: 0 prevents the GTK theme from forcing the
           widget taller than our height_request(10), which would push it
           into the status line. */
        .h-editor-scrollbar {
            background: transparent;
            border: none;
            padding: 0;
            min-height: 0;
            min-width: 0;
        }
        .h-editor-scrollbar trough {
            background: transparent;
            border: none;
            min-height: 0;
            min-width: 0;
            padding: 0;
        }
        .h-editor-scrollbar slider {
            background: rgba(100, 100, 100, 0.45);
            border-radius: 2px;
            min-height: 0;
            min-width: 20px;
            margin: 1px 0;
        }
        .h-editor-scrollbar slider:hover {
            background: rgba(150, 150, 150, 0.7);
        }

        /* Find/Replace Dialog */
        .find-dialog {
            background-color: #2d2d30;
            border: 1px solid #3e3e42;
            border-radius: 4px;
            padding: 12px;
        }

        .find-dialog entry {
            background-color: #3c3c3c;
            color: #cccccc;
            padding: 6px;
            border: 1px solid #3e3e42;
            border-radius: 2px;
        }

        .find-dialog button {
            background: transparent;
            border: 1px solid #3e3e42;
            color: #cccccc;
            padding: 6px 12px;
            border-radius: 2px;
        }

        .find-dialog button:hover {
            background-color: #2a2d2e;
        }

        .find-match-count {
            color: #858585;
            font-size: 11px;
        }

        /* Settings sidebar form — color-dependent rules in make_theme_css() */
        .settings-category-header {
            font-size: 11px;
            font-weight: bold;
            letter-spacing: 1px;
        }

        /* Make ScrolledWindow transparent so sidebar background shows through */
        .sidebar scrolledwindow,
        .sidebar scrolledwindow > viewport {
            background-color: transparent;
        }

        /* SpinButton layout */
        .sidebar spinbutton {
            border-radius: 2px;
        }
        .sidebar spinbutton entry {
            background-color: transparent;
            min-width: 44px;
            padding: 2px 4px;
        }
        .sidebar spinbutton button {
            border: none;
            min-width: 20px;
            padding: 0 2px;
        }

        /* DropDown layout */
        .sidebar dropdown {
            min-height: 24px;
            padding: 0 4px;
            border-radius: 2px;
        }
        .sidebar dropdown button {
            background-color: transparent;
            border: none;
            padding: 2px 4px;
        }

        /* Entry layout */
        .sidebar entry {
            min-height: 24px;
            padding: 2px 6px;
            border-radius: 2px;
        }
        ";

pub(super) fn load_css(theme: &Theme) -> gtk4::CssProvider {
    let provider = gtk4::CssProvider::new();
    let combined = format!("{STATIC_CSS}\n{}", make_theme_css(theme));
    provider.load_from_data(&combined);

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    provider
}
