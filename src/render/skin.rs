//! Declarative chrome proportions and visual tokens — the "skin" that
//! governs how big vimcode's own chrome is, independent of the colour
//! [`super::Theme`] paints it with.
//!
//! A [`Skin`] is the foundation later issues reskin individual surfaces
//! against: every bar height, spacing, shape and control-size token a
//! backend currently might otherwise hardcode lives here instead, with one
//! compiled-in [`Skin::default`] defining vimcode's own proportions —
//! deliberately *not* a copy of VS Code's 48/35/22px activity-bar/tab-bar/
//! status-bar metrics, so vimcode has its own visual identity from the
//! first commit, not someone else's borrowed one.
//!
//! This is **not** a replacement for VS Code theme JSON — [`super::Theme`]
//! and its `from_vscode_json` loader keep doing syntax/UI colour mapping.
//! A `Skin` only ever carries size/spacing/timing tokens, never colours.
//!
//! # File format
//!
//! A skin file is TOML with five sections, each matching one of this
//! module's sub-structs — [`BarHeights`], [`Spacing`], [`Shapes`],
//! [`Controls`], [`Transitions`] — plus a top-level `name`. Every field is
//! `#[serde(default)]`, so a user's override file only needs to name the
//! fields it actually wants to change; anything absent falls back to
//! [`Skin::default`]'s built-in value field-by-field (not section-by-section
//! — omitting `[bars]` entirely, or naming only `activity_bar` inside it,
//! both merge the same way). For example:
//!
//! ```toml
//! name = "compact"
//!
//! [bars]
//! activity_bar = 32.0
//! ```
//!
//! # Active skin
//!
//! [`active_skin`] / [`set_active_skin`] hold the process-wide skin every
//! render call reads, mirroring the `OnceLock`/`Mutex`-backed globals
//! already used for other process-wide, rarely-mutated state (e.g.
//! `render::tab_bar`'s registered font family). Nothing reads this global
//! yet from a live backend — wiring a per-frame `quadraui::Backend::set_style`
//! call is left to the backend-specific follow-up this issue's own
//! description calls out as later work.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Heights, in logical pixels, of vimcode's horizontal chrome bars.
///
/// Deliberately **not** VS Code's 48/35/22px activity-bar/tab-bar/
/// status-bar metrics — see [`Skin::default`]'s own doc for vimcode's
/// own values and why they differ.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BarHeights {
    /// Height of the vertical activity bar's icon strip, measured as a
    /// "bar height" even though the activity bar itself is drawn as a
    /// column — this is the side length of each square icon cell within
    /// it, matching how [`Controls::icon_size`] is used elsewhere.
    pub activity_bar: f32,
    /// Height of the tab row above the editor.
    pub tab_bar: f32,
    /// Height of the breadcrumb row between the tab row and the editor.
    pub breadcrumb_row: f32,
    /// Height of the status bar at the bottom of the window.
    pub status_bar: f32,
    /// Height of the title bar at the top of the window (GTK client-side
    /// decorations / Windows custom title bar only — ignored where the
    /// OS draws its own title bar).
    pub title_bar: f32,
}

impl Default for BarHeights {
    fn default() -> Self {
        Self {
            activity_bar: 40.0,
            tab_bar: 30.0,
            breadcrumb_row: 20.0,
            status_bar: 22.0,
            title_bar: 32.0,
        }
    }
}

/// Spacing tokens, in logical pixels, for chrome layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spacing {
    /// Horizontal padding inside a chrome bar's content area.
    pub chrome_padding_h: f32,
    /// Vertical padding inside a chrome bar's content area.
    pub chrome_padding_v: f32,
    /// Width of the line-number/fold gutter column.
    pub gutter: f32,
    /// Width of a split/panel divider.
    pub divider_width: f32,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            chrome_padding_h: 10.0,
            chrome_padding_v: 6.0,
            gutter: 48.0,
            divider_width: 1.0,
        }
    }
}

/// Shape tokens shared by every rounded-rect / bordered chrome surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shapes {
    /// Corner radius, in logical pixels, of rounded chrome surfaces
    /// (popups, context menus, hover cards).
    pub border_radius: f32,
    /// Drop-shadow elevation tier, `0`-`3` — `0` is no shadow, `3` is the
    /// heaviest (modal-dialog-class) tier. Values above `3` are clamped
    /// the same way [`quadraui::Style::shadow_elevation`] documents.
    pub shadow_level: u8,
    /// Border stroke width, in logical pixels, of a resting (non-focus-ring)
    /// widget border.
    pub border_width: f32,
}

impl Default for Shapes {
    fn default() -> Self {
        Self {
            border_radius: 4.0,
            shadow_level: 1,
            border_width: 1.0,
        }
    }
}

/// Size tokens for interactive controls (buttons, inputs, scroll bars, icons).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Controls {
    /// Height, in logical pixels, of a standard button.
    pub button_height: f32,
    /// Height, in logical pixels, of a single-line text input.
    pub input_height: f32,
    /// Width, in logical pixels, of a scroll bar track.
    pub scrollbar_width: f32,
    /// Side length, in logical pixels, of a square toolbar/activity-bar icon.
    pub icon_size: f32,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            button_height: 28.0,
            input_height: 24.0,
            scrollbar_width: 12.0,
            icon_size: 20.0,
        }
    }
}

/// Timing tokens for chrome animation — fades, slides, and blink rates.
/// All durations are in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transitions {
    /// Duration of a fade-in/fade-out transition (e.g. a toast or hover card).
    pub fade_ms: u32,
    /// Duration of a slide transition (e.g. a panel opening/closing).
    pub slide_ms: u32,
    /// Cursor blink half-period.
    pub blink_rate_ms: u32,
}

impl Default for Transitions {
    fn default() -> Self {
        Self {
            fade_ms: 120,
            slide_ms: 160,
            blink_rate_ms: 530,
        }
    }
}

/// A complete, named set of chrome proportions and visual tokens.
///
/// Construct with [`Skin::default`] for vimcode's own built-in proportions,
/// or [`Skin::from_toml_str`] / [`Skin::load_from_file`] to load a custom
/// skin. See the module doc for the file format and the active-skin
/// accessors ([`active_skin`], [`set_active_skin`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skin {
    /// Display name of this skin (purely informational — not used for
    /// lookup or matching).
    pub name: String,
    pub bars: BarHeights,
    pub spacing: Spacing,
    pub shapes: Shapes,
    pub controls: Controls,
    pub transitions: Transitions,
}

impl Default for Skin {
    /// vimcode's own built-in proportions. Deliberately different from
    /// VS Code's 48/35/22px activity-bar/tab-bar/status-bar metrics (see
    /// [`BarHeights::default`]) — a skin file is an *override* of this
    /// baseline, not a required file every install needs to carry.
    fn default() -> Self {
        Self {
            name: "vimcode-default".to_string(),
            bars: BarHeights::default(),
            spacing: Spacing::default(),
            shapes: Shapes::default(),
            controls: Controls::default(),
            transitions: Transitions::default(),
        }
    }
}

impl Skin {
    /// Parse a skin from a TOML string. Returns a descriptive error message
    /// on malformed TOML — the same `Result<_, String>` shape
    /// [`crate::core::settings::Settings::load_with_validation`] already
    /// uses for user-facing config-parse errors.
    pub fn from_toml_str(contents: &str) -> Result<Self, String> {
        toml::from_str(contents).map_err(|e| format!("Failed to parse skin TOML: {e}"))
    }

    /// Load a skin from a TOML file on disk. Returns a descriptive error
    /// message if the file can't be read or parsed.
    pub fn load_from_file(path: &std::path::Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read skin file at {}: {e}", path.display()))?;
        Self::from_toml_str(&contents)
    }

    /// Map this skin's [`Shapes`]/[`Controls`] tokens onto
    /// [`quadraui::Style`], the equivalent fixed token set quadraui's own
    /// pixel backends read via `quadraui::Backend::style`.
    pub fn to_quadraui_style(&self) -> quadraui::Style {
        quadraui::Style::default()
            .with_padding(self.spacing.chrome_padding_h)
            .with_corner_radius(self.shapes.border_radius)
            .with_border_width(self.shapes.border_width)
            .with_shadow_elevation(self.shapes.shadow_level)
            .with_control_height(self.controls.button_height)
    }
}

/// Process-wide active skin, read by every render call site that wants
/// skin-driven proportions instead of a hardcoded constant. Starts out
/// unset (meaning "use [`Skin::default`]") until [`set_active_skin`] is
/// called — normally once, at startup, by `app::` resolution logic.
static ACTIVE_SKIN: OnceLock<Mutex<Skin>> = OnceLock::new();

/// The currently active [`Skin`] — [`Skin::default`] until
/// [`set_active_skin`] has been called at least once.
pub fn active_skin() -> Skin {
    ACTIVE_SKIN
        .get_or_init(|| Mutex::new(Skin::default()))
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the process-wide active skin. Later calls replace earlier ones;
/// there is no "stack" — this is a single global slot, the same shape as
/// `render::tab_bar`'s registered font family.
pub fn set_active_skin(skin: Skin) {
    let lock = ACTIVE_SKIN.get_or_init(|| Mutex::new(Skin::default()));
    if let Ok(mut guard) = lock.lock() {
        *guard = skin;
    }
}

/// Serializes every test, in this file and in sibling `render::` test
/// modules, that calls [`set_active_skin`] then asserts against
/// [`active_skin`] — otherwise two such tests on different test-runner
/// threads can interleave their `set`/`assert` pairs against the one
/// process-wide [`ACTIVE_SKIN`] slot and fail nondeterministically. Hold the
/// returned guard for the test's full set-then-assert span.
#[cfg(test)]
pub(crate) static TEST_SKIN_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_skin_differs_from_vs_code_bar_heights() {
        let bars = Skin::default().bars;
        // VS Code's own activity bar / tab bar / status bar are 48 / 35 / 22px.
        // vimcode's defaults must not silently match that triple.
        assert_ne!(
            (bars.activity_bar, bars.tab_bar, bars.status_bar),
            (48.0, 35.0, 22.0)
        );
    }

    #[test]
    fn empty_toml_parses_to_full_defaults() {
        let skin = Skin::from_toml_str("").expect("empty document is valid TOML");
        assert_eq!(skin, Skin::default());
    }

    #[test]
    fn partial_override_merges_with_defaults_field_by_field() {
        let toml_src = r#"
            name = "compact"

            [bars]
            activity_bar = 32.0
        "#;
        let skin = Skin::from_toml_str(toml_src).expect("valid partial override");
        assert_eq!(skin.name, "compact");
        assert_eq!(skin.bars.activity_bar, 32.0);
        // Every other bar-height field, and every other section, falls
        // back to the built-in default untouched.
        let defaults = Skin::default();
        assert_eq!(skin.bars.tab_bar, defaults.bars.tab_bar);
        assert_eq!(skin.bars.breadcrumb_row, defaults.bars.breadcrumb_row);
        assert_eq!(skin.bars.status_bar, defaults.bars.status_bar);
        assert_eq!(skin.bars.title_bar, defaults.bars.title_bar);
        assert_eq!(skin.spacing, defaults.spacing);
        assert_eq!(skin.shapes, defaults.shapes);
        assert_eq!(skin.controls, defaults.controls);
        assert_eq!(skin.transitions, defaults.transitions);
    }

    #[test]
    fn malformed_toml_reports_a_descriptive_error() {
        let err = Skin::from_toml_str("not valid = = toml").unwrap_err();
        assert!(err.contains("Failed to parse skin TOML"));
    }

    #[test]
    fn load_from_file_reports_missing_file() {
        let err = Skin::load_from_file(std::path::Path::new(
            "/nonexistent/path/does-not-exist.toml",
        ))
        .unwrap_err();
        assert!(err.contains("Failed to read skin file"));
    }

    #[test]
    fn load_from_file_round_trips_a_real_file() {
        let dir = std::env::temp_dir().join(format!(
            "vimcode-skin-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("skin.toml");
        std::fs::write(&path, "name = \"from-disk\"\n[bars]\ntab_bar = 99.0\n").unwrap();

        let skin = Skin::load_from_file(&path).expect("file should load");
        assert_eq!(skin.name, "from-disk");
        assert_eq!(skin.bars.tab_bar, 99.0);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn to_quadraui_style_maps_shapes_and_controls() {
        let mut skin = Skin::default();
        skin.shapes.border_radius = 9.0;
        skin.shapes.border_width = 3.0;
        skin.shapes.shadow_level = 2;
        skin.controls.button_height = 31.0;
        skin.spacing.chrome_padding_h = 14.0;

        let style = skin.to_quadraui_style();
        assert_eq!(style.corner_radius, 9.0);
        assert_eq!(style.border_width, 3.0);
        assert_eq!(style.shadow_elevation, 2);
        assert_eq!(style.control_height, 31.0);
        assert_eq!(style.padding, 14.0);
    }

    #[test]
    fn active_skin_defaults_until_set() {
        let _guard = TEST_SKIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_active_skin(Skin::default());
        assert_eq!(active_skin(), Skin::default());
    }

    #[test]
    fn set_active_skin_replaces_the_process_wide_value() {
        let _guard = TEST_SKIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut custom = Skin::default();
        custom.name = "custom-for-test".to_string();
        custom.bars.status_bar = 77.0;

        set_active_skin(custom.clone());
        assert_eq!(active_skin(), custom);

        // Restore the default so other tests running in the same process
        // observe the documented starting state.
        set_active_skin(Skin::default());
        assert_eq!(active_skin(), Skin::default());
    }
}
