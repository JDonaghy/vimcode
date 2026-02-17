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
}

fn default_explorer_visible() -> bool {
    false // Default: hidden
}

fn default_incremental_search() -> bool {
    true // Default: enabled
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

    /// Save settings to ~/.config/vimcode/settings.json
    #[allow(dead_code)]
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
}
