use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LineNumberMode {
    #[default]
    None,
    Absolute,
    Relative,
    Hybrid,
}

/// User settings loaded from ~/.config/vimcode/settings.json
///
/// IMPORTANT: When adding new settings fields:
/// 1. Add the field with #[serde(default = "default_function_name")]
/// 2. Create a default function that returns a sensible default value
/// 3. Update the Default impl to include the new field
/// 4. The Settings::load() method will automatically update existing settings files
///    to include the new field with its default value, preserving all existing settings
///
/// Example:
/// ```rust
/// #[serde(default = "default_my_feature")]
/// pub my_feature: bool,
///
/// fn default_my_feature() -> bool { true }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub line_numbers: LineNumberMode,

    #[serde(default = "default_font_family")]
    pub font_family: String,

    #[serde(default = "default_font_size")]
    pub font_size: i32,

    /// Show file explorer sidebar on startup
    #[serde(default = "default_explorer_visible")]
    pub explorer_visible_on_startup: bool,

    /// Enable incremental search (search as you type)
    #[serde(default = "default_incremental_search")]
    pub incremental_search: bool,

    /// Auto-indent new lines to match current line's leading whitespace
    #[serde(default = "default_auto_indent")]
    pub auto_indent: bool,

    /// Insert spaces instead of a literal tab character on Tab key press
    #[serde(default = "default_expand_tab")]
    pub expand_tab: bool,

    /// Number of spaces a Tab key inserts (when expand_tab is true),
    /// or how wide a tab character is displayed (when expand_tab is false)
    #[serde(default = "default_tabstop")]
    pub tabstop: u8,

    /// Number of spaces added/removed by indent operators (>> / <<)
    #[serde(default = "default_shift_width")]
    pub shift_width: u8,
}

fn default_explorer_visible() -> bool {
    false // Default: hidden
}

fn default_incremental_search() -> bool {
    true // Default: enabled
}

fn default_auto_indent() -> bool {
    true // Default: enabled
}

fn default_expand_tab() -> bool {
    true // Default: on (match existing Tab key behavior — inserts spaces)
}

fn default_tabstop() -> u8 {
    4
}

fn default_shift_width() -> u8 {
    4
}

fn default_font_family() -> String {
    "Monospace".to_string()
}

fn default_font_size() -> i32 {
    14
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            line_numbers: LineNumberMode::None,
            font_family: default_font_family(),
            font_size: default_font_size(),
            explorer_visible_on_startup: default_explorer_visible(),
            incremental_search: default_incremental_search(),
            auto_indent: default_auto_indent(),
            expand_tab: default_expand_tab(),
            tabstop: default_tabstop(),
            shift_width: default_shift_width(),
        }
    }
}

impl Settings {
    /// Load settings from ~/.config/vimcode/settings.json
    /// Falls back to defaults if file doesn't exist or is invalid
    ///
    /// IMPORTANT: This method automatically updates the settings file to include any new
    /// settings with their default values, preserving all existing user settings.
    /// This ensures that when new settings are added to VimCode, they appear in the user's
    /// settings.json file with sensible defaults without requiring manual editing.
    pub fn load() -> Self {
        match Self::load_with_validation() {
            Ok(settings) => {
                // Automatically update settings file to include any new fields with defaults
                // This preserves existing settings while adding new ones
                if let Err(e) = settings.save() {
                    eprintln!("Warning: Failed to update settings file: {}", e);
                }
                settings
            }
            Err(e) => {
                eprintln!("Warning: {}. Using defaults.", e);
                let defaults = Settings::default();
                // Repair empty/corrupt file by writing defaults
                if let Err(save_err) = defaults.save() {
                    eprintln!("Warning: Failed to write default settings: {}", save_err);
                }
                defaults
            }
        }
    }

    /// Load settings from ~/.config/vimcode/settings.json with validation
    /// Returns Result with descriptive error messages for UI display
    pub fn load_with_validation() -> Result<Self, String> {
        let path = Self::settings_path();

        let contents = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read settings file at {}: {}", path.display(), e))?;

        serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse settings.json: {}. Check JSON syntax.", e))
    }

    /// Apply a single vim `:set` argument and update `self` in place.
    ///
    /// Does **not** persist to disk — call [`Self::save`] afterwards.
    ///
    /// Supported forms:
    /// - `option` — enable a boolean option (e.g. `number`, `expandtab`)
    /// - `nooption` — disable a boolean option (e.g. `nonumber`)
    /// - `option?` — query current value; returns display string, no mutation
    /// - `option=N` — set a numeric option (e.g. `tabstop=4`)
    ///
    /// Returns `Ok(display_message)` or `Err(error_message)`.
    pub fn parse_set_option(&mut self, arg: &str) -> Result<String, String> {
        // Query only — no mutation.
        if let Some(opt) = arg.strip_suffix('?') {
            return self.query_option(opt.trim());
        }

        // Disable a boolean option.
        if let Some(opt) = arg.strip_prefix("no") {
            self.set_bool_option(opt, false)?;
            return Ok(format!("no{opt}"));
        }

        // Set a value option (contains '=').
        if let Some(eq_pos) = arg.find('=') {
            let name = arg[..eq_pos].trim();
            let value = arg[eq_pos + 1..].trim();
            self.set_value_option(name, value)?;
            return Ok(format!("{name}={value}"));
        }

        // Enable a boolean option.
        self.set_bool_option(arg, true)?;
        Ok(arg.to_string())
    }

    /// Return a compact one-line summary of all current settings.
    /// Shown when the user types `:set` with no arguments.
    pub fn display_all(&self) -> String {
        let num = match self.line_numbers {
            LineNumberMode::None => "nonumber nornu",
            LineNumberMode::Absolute => "number nornu",
            LineNumberMode::Relative => "nonumber rnu",
            LineNumberMode::Hybrid => "number rnu",
        };
        let et = if self.expand_tab {
            "expandtab"
        } else {
            "noexpandtab"
        };
        let ai = if self.auto_indent {
            "autoindent"
        } else {
            "noautoindent"
        };
        let is = if self.incremental_search {
            "incsearch"
        } else {
            "noincsearch"
        };
        format!(
            "{}  {}  ts={}  sw={}  {}  {}",
            num, et, self.tabstop, self.shift_width, ai, is
        )
    }

    fn set_bool_option(&mut self, opt: &str, enable: bool) -> Result<(), String> {
        match opt {
            "number" | "nu" => {
                self.line_numbers = if enable {
                    match self.line_numbers {
                        LineNumberMode::Relative | LineNumberMode::Hybrid => LineNumberMode::Hybrid,
                        _ => LineNumberMode::Absolute,
                    }
                } else {
                    match self.line_numbers {
                        LineNumberMode::Hybrid => LineNumberMode::Relative,
                        _ => LineNumberMode::None,
                    }
                };
            }
            "relativenumber" | "rnu" => {
                self.line_numbers = if enable {
                    match self.line_numbers {
                        LineNumberMode::Absolute | LineNumberMode::Hybrid => LineNumberMode::Hybrid,
                        _ => LineNumberMode::Relative,
                    }
                } else {
                    match self.line_numbers {
                        LineNumberMode::Hybrid => LineNumberMode::Absolute,
                        _ => LineNumberMode::None,
                    }
                };
            }
            "expandtab" | "et" => self.expand_tab = enable,
            "autoindent" | "ai" => self.auto_indent = enable,
            "incsearch" | "is" => self.incremental_search = enable,
            _ => return Err(format!("Unknown option: {opt}")),
        }
        Ok(())
    }

    fn set_value_option(&mut self, name: &str, value: &str) -> Result<(), String> {
        match name {
            "tabstop" | "ts" => {
                let n: u8 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                if n == 0 {
                    return Err("tabstop must be greater than 0".to_string());
                }
                self.tabstop = n;
            }
            "shiftwidth" | "sw" => {
                let n: u8 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.shift_width = n;
            }
            _ => return Err(format!("Unknown option: {name}")),
        }
        Ok(())
    }

    fn query_option(&self, opt: &str) -> Result<String, String> {
        match opt {
            "number" | "nu" => {
                let on = matches!(
                    self.line_numbers,
                    LineNumberMode::Absolute | LineNumberMode::Hybrid
                );
                Ok(if on {
                    "number".to_string()
                } else {
                    "nonumber".to_string()
                })
            }
            "relativenumber" | "rnu" => {
                let on = matches!(
                    self.line_numbers,
                    LineNumberMode::Relative | LineNumberMode::Hybrid
                );
                Ok(if on {
                    "relativenumber".to_string()
                } else {
                    "norelativenumber".to_string()
                })
            }
            "expandtab" | "et" => Ok(if self.expand_tab {
                "expandtab".to_string()
            } else {
                "noexpandtab".to_string()
            }),
            "tabstop" | "ts" => Ok(format!("tabstop={}", self.tabstop)),
            "shiftwidth" | "sw" => Ok(format!("shiftwidth={}", self.shift_width)),
            "autoindent" | "ai" => Ok(if self.auto_indent {
                "autoindent".to_string()
            } else {
                "noautoindent".to_string()
            }),
            "incsearch" | "is" => Ok(if self.incremental_search {
                "incsearch".to_string()
            } else {
                "noincsearch".to_string()
            }),
            _ => Err(format!("Unknown option: {opt}")),
        }
    }

    /// Save settings to ~/.config/vimcode/settings.json
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::settings_path();

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents)?;

        Ok(())
    }

    fn settings_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("vimcode")
            .join("settings.json")
    }

    /// Ensure settings.json exists with default values
    /// Creates the file if missing. Note that existing files are automatically updated
    /// when Settings::load() is called - it adds new fields with defaults while preserving
    /// existing user settings.
    pub fn ensure_exists() -> Result<(), std::io::Error> {
        let path = Self::settings_path();

        // Only create if file doesn't exist
        if !path.exists() {
            // Create parent directories
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Write default settings
            Self::default().save()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_settings_path() -> PathBuf {
        PathBuf::from("/tmp/vimcode_test_settings.json")
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.line_numbers, LineNumberMode::None);
        assert_eq!(settings.font_family, "Monospace");
        assert_eq!(settings.font_size, 14);
    }

    #[test]
    fn test_settings_load_missing_file() {
        // Load should return defaults when file doesn't exist
        // Note: This test may not work if settings.json already exists
        // It's testing the fallback behavior when Settings::load() encounters
        // a missing or invalid file

        // If the file exists, this will load actual settings
        // If it doesn't exist, it will return defaults
        let settings = Settings::load();

        // Just verify that loading doesn't panic and returns valid settings
        assert!(!settings.font_family.is_empty());
        assert!(settings.font_size > 0);
    }

    #[test]
    fn test_settings_load_save() {
        let test_path = test_settings_path();

        // Clean up before test
        let _ = fs::remove_file(&test_path);

        // Create settings with custom values
        let mut settings = Settings::default();
        settings.line_numbers = LineNumberMode::Absolute;

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&settings).unwrap();
        fs::write(&test_path, json).unwrap();

        // Load and verify
        let contents = fs::read_to_string(&test_path).unwrap();
        let loaded: Settings = serde_json::from_str(&contents).unwrap();
        assert_eq!(loaded.line_numbers, LineNumberMode::Absolute);

        // Clean up
        let _ = fs::remove_file(&test_path);
    }

    #[test]
    fn test_settings_invalid_json() {
        let test_path = test_settings_path();

        // Write invalid JSON
        fs::write(&test_path, "{ invalid json }").unwrap();

        // Parse should fail gracefully and return defaults
        let contents = fs::read_to_string(&test_path).unwrap();
        let result: Result<Settings, _> = serde_json::from_str(&contents);
        assert!(result.is_err());

        // Clean up
        let _ = fs::remove_file(&test_path);
    }

    #[test]
    fn test_line_number_mode_serialization() {
        let modes = vec![
            LineNumberMode::None,
            LineNumberMode::Absolute,
            LineNumberMode::Relative,
            LineNumberMode::Hybrid,
        ];

        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: LineNumberMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_load_with_validation_success() {
        // Test the parsing directly without filesystem operations
        let json = r#"{"line_numbers":"Relative"}"#;
        let result: Result<Settings, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().line_numbers, LineNumberMode::Relative);
    }

    #[test]
    fn test_load_with_validation_invalid_json() {
        // Test that invalid JSON returns an error
        let invalid_json = "{ invalid json }";
        let result: Result<Settings, _> = serde_json::from_str(invalid_json);
        assert!(result.is_err());
    }

    // ── :set command tests ────────────────────────────────────────────────────

    #[test]
    fn test_set_number_enables_absolute() {
        let mut s = Settings::default();
        assert_eq!(s.line_numbers, LineNumberMode::None);
        let msg = s.parse_set_option("number").unwrap();
        assert_eq!(msg, "number");
        assert_eq!(s.line_numbers, LineNumberMode::Absolute);
    }

    #[test]
    fn test_set_nonumber_disables() {
        let mut s = Settings::default();
        s.line_numbers = LineNumberMode::Absolute;
        let msg = s.parse_set_option("nonumber").unwrap();
        assert_eq!(msg, "nonumber");
        assert_eq!(s.line_numbers, LineNumberMode::None);
    }

    #[test]
    fn test_set_relativenumber() {
        let mut s = Settings::default();
        s.parse_set_option("relativenumber").unwrap();
        assert_eq!(s.line_numbers, LineNumberMode::Relative);
    }

    #[test]
    fn test_set_number_plus_relativenumber_gives_hybrid() {
        let mut s = Settings::default();
        s.parse_set_option("number").unwrap();
        s.parse_set_option("relativenumber").unwrap();
        assert_eq!(s.line_numbers, LineNumberMode::Hybrid);
    }

    #[test]
    fn test_set_norelativenumber_from_hybrid_gives_absolute() {
        let mut s = Settings::default();
        s.line_numbers = LineNumberMode::Hybrid;
        s.parse_set_option("norelativenumber").unwrap();
        assert_eq!(s.line_numbers, LineNumberMode::Absolute);
    }

    #[test]
    fn test_set_expandtab() {
        let mut s = Settings::default();
        s.expand_tab = false;
        s.parse_set_option("expandtab").unwrap();
        assert!(s.expand_tab);
        s.parse_set_option("noexpandtab").unwrap();
        assert!(!s.expand_tab);
    }

    #[test]
    fn test_set_tabstop() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("tabstop=2").unwrap();
        assert_eq!(msg, "tabstop=2");
        assert_eq!(s.tabstop, 2);
    }

    #[test]
    fn test_set_tabstop_alias() {
        let mut s = Settings::default();
        s.parse_set_option("ts=8").unwrap();
        assert_eq!(s.tabstop, 8);
    }

    #[test]
    fn test_set_tabstop_zero_is_error() {
        let mut s = Settings::default();
        assert!(s.parse_set_option("tabstop=0").is_err());
    }

    #[test]
    fn test_set_shiftwidth() {
        let mut s = Settings::default();
        s.parse_set_option("shiftwidth=2").unwrap();
        assert_eq!(s.shift_width, 2);
        s.parse_set_option("sw=3").unwrap();
        assert_eq!(s.shift_width, 3);
    }

    #[test]
    fn test_set_autoindent_alias() {
        let mut s = Settings::default();
        assert!(s.auto_indent);
        s.parse_set_option("noai").unwrap();
        assert!(!s.auto_indent);
        s.parse_set_option("ai").unwrap();
        assert!(s.auto_indent);
    }

    #[test]
    fn test_set_incsearch() {
        let mut s = Settings::default();
        assert!(s.incremental_search);
        s.parse_set_option("noincsearch").unwrap();
        assert!(!s.incremental_search);
        s.parse_set_option("is").unwrap();
        assert!(s.incremental_search);
    }

    #[test]
    fn test_set_query_number() {
        let mut s = Settings::default();
        s.line_numbers = LineNumberMode::Absolute;
        let msg = s.parse_set_option("number?").unwrap();
        assert_eq!(msg, "number");
        let msg2 = s.parse_set_option("rnu?").unwrap();
        assert_eq!(msg2, "norelativenumber");
    }

    #[test]
    fn test_set_query_tabstop() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("ts?").unwrap();
        assert_eq!(msg, "tabstop=4");
    }

    #[test]
    fn test_set_unknown_option_is_error() {
        let mut s = Settings::default();
        assert!(s.parse_set_option("unknownoption").is_err());
        assert!(s.parse_set_option("nounknown").is_err());
        assert!(s.parse_set_option("foo=42").is_err());
    }

    #[test]
    fn test_display_all() {
        let s = Settings::default();
        let display = s.display_all();
        assert!(display.contains("nonumber"));
        assert!(display.contains("expandtab"));
        assert!(display.contains("ts=4"));
        assert!(display.contains("sw=4"));
        assert!(display.contains("autoindent"));
        assert!(display.contains("incsearch"));
    }
}
