//! Backend-neutral theme CSS text generation (#862).
//!
//! Moved out of `src/gtk/css.rs` (`gui`-gated): `make_theme_css`/`STATIC_CSS`
//! only build a `String` from `crate::render::Theme` — no `gtk4` type in
//! sight. `src/gtk/css.rs` keeps `load_css` (which actually constructs a
//! `gtk4::CssProvider`) and re-exports these two so the rest of `crate::gtk`
//! keeps resolving them unchanged.
//!
//! # #1103: this file used to be 501 lines styling the Relm4-era native
//! widget tree (`.activity-bar`, `.tab-bar`, custom sidebar/search/settings
//! classes, a native `treeview`, `.window-control`, `headerbar`, …). #540
//! removed Relm4 and #731 deleted the last orphaned native-widget handles;
//! the activity bar, tab bar, sidebars, search UI and settings form are all
//! painted directly by quadraui's Cairo rasterisers now (`quadraui::gtk::
//! activity_bar`/`sidebar_panel`/`tab_bar`/`scrollbar`/`dialog`, confirmed
//! by reading those modules — they only `use gtk4::cairo::Context`/
//! `gtk4::pango`, never construct a `gtk4::Widget`). None of that CSS had a
//! live consumer, so it was deleted.
//!
//! **What's still here is not dead**, and was kept only after live
//! verification. `gtk4::style_context_add_provider_for_display` registers
//! this stylesheet at the *display* level — it applies to every GTK widget
//! in the process, not just vimcode's own canvas. The one place vimcode
//! still constructs unstyled, stock GTK widgets is the native file dialog
//! (`gtk4::FileDialog`, `src/gtk/services.rs` upstream in quadraui). On a
//! desktop with no `xdg-desktop-portal` running — confirmed to be this
//! project's dev/CI baseline — `GtkFileDialog` falls back to its own
//! in-process `GtkFileChooserDialog` implementation, which *does* share
//! this display and *does* land inside the reach of a couple of these
//! selectors.
//!
//! Verified live (2026-09-18, #1103) by registering this exact provider via
//! `crate::gtk::css::load_css` and opening a real `gtk4::FileDialog` under
//! `GDK_BACKEND=x11` (this dev machine's WSLg `DISPLAY=:0`; no
//! `xdg-desktop-portal` binary is installed here, so the dialog took the
//! native fallback path), then walking `gtk4::Window::list_toplevels()` and
//! dumping every widget's `css_name()`/`css_classes()`. Findings (GTK
//! 4.14.5, `gtk4` crate 0.11.4):
//!
//! - The dialog's `GtkPlacesSidebar` (the Recent/Home/bookmarks list on the
//!   left) carries the literal class **`sidebar`** — the same class name
//!   vimcode's own (quadraui-painted) sidebar concept used, so every
//!   `.sidebar ...` rule below reaches it.
//!   - `GtkLabel` nodes are nested directly inside its rows (confirmed in
//!     the dump) → `.sidebar label` is live.
//!   - Its own `GtkScrolledWindow`/`GtkViewport` are direct children
//!     (confirmed) → `.sidebar scrolledwindow`/`.sidebar scrolledwindow >
//!     viewport` are live.
//!   - No `GtkEntry`, `GtkSearchEntry`, `GtkDropDown` or `GtkSpinButton`
//!     appeared nested under it in the (idle, freshly-opened) dump, so the
//!     old `.sidebar entry`/`.sidebar searchentry`/`.sidebar dropdown`/
//!     `.sidebar spinbutton` rules had no confirmed live target and were
//!     deleted along with the rest.
//! - Every scrollable area in the dialog (places sidebar, path bar,
//!   column view, popovers) uses a real `GtkScrollbar` (`css_name=
//!   scrollbar`) — the bare `scrollbar`/`scrollbar slider` rules below are
//!   live across the whole dialog, not just the sidebar.
//! - The dialog uses several `GtkPopover`/`GtkPopoverContent` pairs
//!   (location-bar popup, type-filter dropdown, connect-to-server popup,
//!   bookmark-rename popup) — the bare `popover contents`/`popover.menu
//!   contents` rule is live. No widget anywhere in the dump had
//!   `css_name() == "modelbutton"`, so the old `popover modelbutton` rule
//!   was dead and is gone.
//! - No `treeview` node appeared anywhere — GTK4's file chooser uses
//!   `GtkColumnView`/`GtkListView`, not the deprecated `GtkTreeView` — so
//!   the whole `treeview` block was dead and is gone.
//! - No `headerbar` node appeared — this GTK build's native file dialog
//!   uses a classic button-box action area (Cancel/Open), not a
//!   `GtkHeaderBar` — so the bare `headerbar` rule was dead and is gone.
//!
//! If `xdg-desktop-portal` is installed at some point, `GtkFileDialog`
//! prefers the portal (an out-of-process dialog this CSS provider cannot
//! reach at all) and every rule below goes quiet again without needing a
//! code change — nothing here assumes the portal's absence, it only
//! documents why the current dev/CI baseline can observe it.
use crate::render::{ColorExt, Theme};

/// Generate the full CSS string with colors taken from the active theme.
pub(crate) fn make_theme_css(theme: &Theme) -> String {
    let bar_bg = theme.tab_bar_bg.to_hex();
    // For light themes, use foreground color for active icons (status_fg is white).
    let bar_fg = if theme.is_light() {
        theme.foreground.to_hex()
    } else {
        theme.status_fg.to_hex()
    };
    let text_fg = theme.foreground.to_hex();
    let entry_bg = theme.active_background.to_hex();
    let border_col = theme.separator.to_hex();
    let sb_thumb = theme.scrollbar_thumb.to_hex();
    format!(
        r#"
        /* Sidebar — native GTK file dialog's GtkPlacesSidebar carries this
           exact class (see module docs above). */
        .sidebar {{
            background-color: {bar_bg};
            border-right: 1px solid {border_col};
        }}

        .sidebar label {{
            color: {bar_fg};
        }}

        popover.menu contents,
        popover contents {{
            background-color: {entry_bg};
            color: {text_fg};
        }}

        /* Scrollbar — theme-aware overrides.
           Trough (the track behind the slider) reads `border_col`
           (theme.separator) at low alpha so the scrollbar is
           perceptible against the editor bg without overpowering
           the editor area. */
        scrollbar {{
            background: alpha({border_col}, 0.30);
        }}
        scrollbar slider {{
            background: alpha({sb_thumb}, 0.5);
        }}
        scrollbar slider:hover {{
            background: alpha({sb_thumb}, 0.7);
        }}
        scrollbar slider:active {{
            background: alpha({sb_thumb}, 0.9);
        }}

        "#
    )
}

/// Static structural CSS that never changes with the theme.
/// Theme-specific colours live in `make_theme_css()` and are appended after this.
pub(crate) const STATIC_CSS: &str = "
        /* VSCode UI font stack — 'Segoe UI' on Windows, 'Ubuntu' on Ubuntu,
           system-ui/sans elsewhere. 13px matches VSCode default UI size.
           Also reaches the native file dialog's GtkPlacesSidebar (see
           module docs above) — deliberately: it keeps the sidebar text
           legible at the same size as the rest of the app chrome. */
        .sidebar,
        .sidebar * {
            font-family: 'Segoe UI', system-ui, -apple-system, 'Ubuntu', 'Droid Sans', sans-serif;
            font-size: 13px;
        }

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

        /* Make ScrolledWindow transparent so the sidebar background shows
           through — reaches the native file dialog's GtkPlacesSidebar,
           whose GtkScrolledWindow/GtkViewport are direct children of the
           `.sidebar`-classed widget (see module docs above). */
        .sidebar scrolledwindow,
        .sidebar scrolledwindow > viewport {
            background-color: transparent;
        }
        ";
