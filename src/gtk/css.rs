//! GTK-only CSS provider construction. The text-generation half
//! (`make_theme_css`/`STATIC_CSS`) moved to the backend-neutral
//! `crate::css` (#862) — re-exported here so the rest of `crate::gtk`
//! (and this module's own callers) keep resolving the names unchanged.
use crate::render::Theme;

pub(crate) use crate::css::{make_theme_css, STATIC_CSS};

pub(crate) fn load_css(theme: &Theme) -> gtk4::CssProvider {
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
