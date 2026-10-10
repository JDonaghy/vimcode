//! Platform-agnostic rendering abstraction layer.
//!
//! This module defines the data types and builder function that convert engine
//! state into a `ScreenLayout` — the shared contract between the GTK/Cairo
//! backend and any future TUI backend.
//!
//! **Critical:** No GTK, Cairo, Pango, or Relm4 dependencies are allowed here.
//! All types must be plain Rust structs with no platform coupling.

// #937's mandatory quadraui pin bump (dbb3023 -> 68f0ef9, needed for
// `register_font_from_memory`/`set_nerd_font_fallback`) newly deprecated
// `Backend::draw_status_bar`/`draw_toolbar`/`draw_sidebar_panel` (quadraui#819)
// and the tab-bar hit struct plus a syntax-highlight span shape (quadraui#822/
// #823) that this file used. #1490 migrated every `draw_status_bar` call site
// to `draw_status_bar_interactive`; #1491 migrated every tab-bar paint/measure
// call site off the now-removed deprecated struct onto `TabBarLayout`. #1652
// (quadraui#1108/#1109) finished the set: `draw_toolbar`/`draw_sidebar_panel`
// now call their `_interactive` twins directly, and `raw_syntax_spans` is
// `Vec<quadraui::MinimapSpan>` — no `quadraui`-only `#[allow(deprecated)]`
// remains in this file.
//
// `dead_code` dropped by #1489: the ~600 lines it was silently covering
// (two whole dead data pipelines plus five dead helpers) are gone; any new
// dead code in this file is a real warning again.

use crate::core::buffer::{Buffer, DecorMark, VirtTextPos};
use crate::core::engine::sidebar::{
    HAMBURGER_PANEL_ID, PANEL_AI, PANEL_BOARD, PANEL_DEBUG, PANEL_EXTENSIONS, PANEL_GIT,
    PANEL_SEARCH, PANEL_SETTINGS,
};
use crate::core::engine::{
    compute_word_wrap_segments, AlignedDiffEntry, DiffLine, Engine, PanelChromeDesc,
    SearchDirection,
};
pub use crate::core::engine::{BottomPanelKind, DebugSidebarSection};
use crate::core::lsp::SignatureHelpData;
use crate::core::project_search::QuickfixList;
use crate::core::settings::{FoldControlsMode, LineNumberMode, Settings};
use crate::core::view::View;
use crate::core::window::{GroupDivider, GroupId, SplitDirection, WindowDivider};
use crate::core::{Cursor, GitLineStatus, Mode, WindowId, WindowRect};
use crate::icons;

mod activity_bar_and_sidebar_composition;
mod ai_chat_and_misc_builders;
mod build_screen_layout;
mod chrome_mouse_rung;
mod decoration_and_markdown;
mod editor_hover_rung;
mod frame_chrome_band;
mod frame_editor_band;
mod hit_test_geometry;
mod key_decode;
mod keyboard_routing;
mod keyboard_routing_closing;
mod menu_and_chrome_routing;
mod menu_bar_structure;
mod minimap;
mod picker_and_modal_router;
mod plugin_view_body;
mod plugin_views_declared;
mod primitives;
mod quadraui_adapters;
mod screen_layout_type;
#[cfg(test)]
mod scrollbar_reserve_tests;
mod sidebar_data_types;
mod sidebar_dispatch;
mod sidebar_tree_population;
mod source_control_sidebar;
mod static_menu_structure;
mod tab_bar;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_behavioral;
mod theme;
mod window_and_popups;
mod window_builder;

pub use activity_bar_and_sidebar_composition::*;
pub use ai_chat_and_misc_builders::*;
pub use build_screen_layout::*;
pub use chrome_mouse_rung::*;
pub use decoration_and_markdown::*;
pub use editor_hover_rung::*;
pub use frame_chrome_band::*;
pub use frame_editor_band::*;
pub use hit_test_geometry::*;
pub use key_decode::*;
pub use keyboard_routing::*;
pub use keyboard_routing_closing::*;
pub use menu_and_chrome_routing::*;
pub use menu_bar_structure::*;
pub use minimap::*;
pub use picker_and_modal_router::*;
pub use plugin_views_declared::*;
pub use primitives::*;
pub use screen_layout_type::*;
pub use sidebar_data_types::*;
pub use sidebar_dispatch::*;
pub use sidebar_tree_population::*;
pub use source_control_sidebar::*;
pub use static_menu_structure::*;
pub use tab_bar::*;
pub use theme::*;
pub use window_and_popups::*;
// `plugin_view_body` and `window_builder` currently expose only
// `pub(crate)` items (same as before the split, when they were reachable
// straight off `crate::render::`) — the glob re-export below still makes
// them reachable at `render::<name>` for same-crate callers like `app.rs`;
// it just isn't *publicly* public, hence the glob-reexport lint below.
#[allow(unused_imports)]
pub use plugin_view_body::*;
pub use quadraui_adapters::*;
#[cfg(test)]
pub use scrollbar_reserve_tests::*;
#[cfg(test)]
pub use test_support::*;
#[cfg(test)]
pub use tests_behavioral::*;
#[allow(unused_imports)]
pub use window_builder::*;

// Cross-file test fixtures: a handful of helper fns/consts used by more than
// one split-off test module still live inside their "home" file's own
// `#[cfg(test)] mod tests { ... }` (moved there verbatim with the rest of
// that file's test block) rather than in `test_support.rs` — widened to
// `pub(crate)` and re-exported here so every other test module's existing
// `use super::*;` picks them up transitively, same as it did when they were
// all one flat `mod tests` in the original file.
#[cfg(test)]
pub(crate) use frame_editor_band::tests::*;
#[cfg(test)]
pub(crate) use hit_test_geometry::tests::*;
#[cfg(test)]
pub(crate) use minimap::tests::*;
#[cfg(test)]
pub(crate) use screen_layout_type::tests::*;
#[cfg(test)]
pub(crate) use theme::tests::*;
