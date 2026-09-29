use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// When set to `true`, all `save()` methods in this module become no-ops.
/// Integration tests call [`suppress_disk_saves`] once so `cargo test` never
/// clobbers real user config files under `~/.config/vimcode/`.
static SUPPRESS_SAVES: AtomicBool = AtomicBool::new(false);

/// Permanently suppress all disk writes from `save()` in this process.
/// Thread-safe: uses `AtomicBool`; safe to call from multiple threads.
/// Intended for integration tests only.
#[allow(dead_code)]
pub fn suppress_disk_saves() {
    SUPPRESS_SAVES.store(true, Ordering::Relaxed);
}

/// Returns `true` if disk saves have been suppressed (i.e. we are running in
/// an integration-test process). Used by `Settings::save()` to avoid writing
/// to the user's real config when called from integration test code that is
/// compiled without `#[cfg(test)]`.
pub fn saves_suppressed() -> bool {
    SUPPRESS_SAVES.load(Ordering::Relaxed)
}

/// When set to `true`, `load()` methods that opt in (currently just
/// [`HistoryState::load`]) return `Default` instead of reading the real
/// `~/.config/vimcode/` files. Companion to `SUPPRESS_SAVES` — that flag
/// only stops *writes*, so `Engine::new()` in a test process still picked up
/// whatever command history happened to be sitting in the developer
/// machine's real config directory (#1304).
static SUPPRESS_LOADS: AtomicBool = AtomicBool::new(false);

/// Permanently suppress opted-in disk loads for the remaining lifetime of
/// this process. Thread-safe: uses `AtomicBool`; safe to call from multiple
/// threads. Intended for integration tests only.
#[allow(dead_code)]
pub fn suppress_disk_loads() {
    SUPPRESS_LOADS.store(true, Ordering::Relaxed);
}

/// Returns `true` if disk loads have been suppressed (i.e. we are running in
/// an integration-test process). Used by `HistoryState::load()` to avoid
/// reading the user's real history when called from integration test code
/// that is compiled without `#[cfg(test)]`.
pub fn loads_suppressed() -> bool {
    SUPPRESS_LOADS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// HistoryState — command/search history in its own file
// ---------------------------------------------------------------------------

/// Command and search history, persisted to ~/.config/vimcode/history.json
/// (separate from session.json so it is never overwritten by workspace sessions).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryState {
    /// Command-mode history (most recent last, max 100 entries)
    #[serde(default)]
    pub command_history: Vec<String>,

    /// Search history (most recent last, max 100 entries)
    #[serde(default)]
    pub search_history: Vec<String>,
}

/// Minimal view of legacy session.json used only during one-time migration.
#[derive(Debug, Deserialize, Default)]
struct LegacySession {
    #[serde(default)]
    command_history: Vec<String>,
    #[serde(default)]
    search_history: Vec<String>,
}

impl HistoryState {
    fn history_path() -> PathBuf {
        super::paths::vimcode_config_dir().join("history.json")
    }

    fn legacy_session_path() -> PathBuf {
        super::paths::vimcode_config_dir().join("session.json")
    }

    /// Load history from history.json.
    /// If history.json is absent, attempts a one-time migration from session.json.
    pub fn load() -> Self {
        // In-crate unit tests (`cargo test --lib`) never call
        // `suppress_disk_loads()` — that runtime flag exists for *integration*
        // tests, which are compiled without `#[cfg(test)]`. So the `#1304`
        // hazard `save()` already guards against (below) still applied in the
        // read direction here: `Engine::new()` inside a lib unit test seeded
        // `engine.history` from whatever was sitting in the developer
        // machine's real `~/.config/vimcode/history.json`. On a machine whose
        // history had reached the 100-entry cap that made
        // `q_colon_opens_a_split_not_a_new_tab_via_shell_app` fail: the marker
        // command the test appends landed at line 101 of the `[Command
        // History]` buffer and scrolled off the rendered cmdline window.
        // Mirror `save()`'s `#[cfg(test)]` no-op so the in-crate lane is
        // hermetic too, regardless of the host's config dir.
        #[cfg(test)]
        return Self::default();

        #[cfg_attr(test, allow(unreachable_code))]
        {
            if loads_suppressed() {
                return Self::default();
            }
            let path = Self::history_path();
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(state) = serde_json::from_str(&contents) {
                    return state;
                }
            }
            // history.json not found — try migrating from legacy session.json
            let session_path = Self::legacy_session_path();
            if let Ok(contents) = std::fs::read_to_string(&session_path) {
                if let Ok(legacy) = serde_json::from_str::<LegacySession>(&contents) {
                    if !legacy.command_history.is_empty() || !legacy.search_history.is_empty() {
                        return Self {
                            command_history: legacy.command_history,
                            search_history: legacy.search_history,
                        };
                    }
                }
            }
            Self::default()
        }
    }

    /// Save history to history.json using an atomic write.
    pub fn save(&self) -> std::io::Result<()> {
        // Unlike `SessionState::save()`/`ExtensionState::save()`, this used
        // to have no `SUPPRESS_SAVES` guard at all (#1304): every `:command`
        // or search that hit `handle_key`'s Enter path called this
        // unconditionally, so the *entire* test suite was silently
        // appending every test's typed commands to the real
        // `~/.config/vimcode/history.json` on whatever machine ran
        // `cargo test` — confirmed: this repo's own dev machine's real
        // history.json had accumulated stray `echo hello`/`echo one`/
        // `echo two` entries from earlier test runs before this fix.
        #[cfg(test)]
        return Ok(());

        #[cfg_attr(test, allow(unreachable_code))]
        if SUPPRESS_SAVES.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg_attr(test, allow(unreachable_code))]
        {
            let path = Self::history_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string_pretty(self)?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, &json)?;
            std::fs::rename(&tmp, &path)?;
            Ok(())
        }
    }

    /// Add a command to history (max 100, removes duplicates, moves to end).
    pub fn add_command(&mut self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }
        self.command_history.retain(|c| c != cmd);
        self.command_history.push(cmd.to_string());
        if self.command_history.len() > 100 {
            self.command_history.remove(0);
        }
    }

    /// Add a search query to history (max 100, removes duplicates, moves to end).
    pub fn add_search(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|q| q != query);
        self.search_history.push(query.to_string());
        if self.search_history.len() > 100 {
            self.search_history.remove(0);
        }
    }
}

// ---------------------------------------------------------------------------
// ExtensionState — installed/dismissed extension tracking
// ---------------------------------------------------------------------------

/// A single installed extension with its version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledExtension {
    pub name: String,
    #[serde(default)]
    pub version: String,
}

/// Custom deserializer that accepts both the old format (plain string list)
/// and the new format (list of `InstalledExtension` objects).
fn deserialize_installed<'de, D>(deserializer: D) -> Result<Vec<InstalledExtension>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    use serde_json::Value;

    let values: Vec<Value> = Vec::deserialize(deserializer)?;
    let mut result = Vec::with_capacity(values.len());
    for v in values {
        match v {
            Value::String(name) => {
                // Old format: plain string → version unknown
                result.push(InstalledExtension {
                    name,
                    version: String::new(),
                });
            }
            Value::Object(_) => {
                // New format: object with name+version
                let ext: InstalledExtension =
                    serde_json::from_value(v).map_err(de::Error::custom)?;
                result.push(ext);
            }
            _ => {
                return Err(de::Error::custom(
                    "expected string or object in installed list",
                ));
            }
        }
    }
    Ok(result)
}

/// Which extensions the user has installed or dismissed, persisted to
/// `~/.config/vimcode/extensions.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionState {
    /// Extensions the user has installed, with version tracking.
    #[serde(default, deserialize_with = "deserialize_installed")]
    pub installed: Vec<InstalledExtension>,
    /// Names of extensions the user dismissed the install prompt for.
    #[serde(default)]
    pub dismissed: Vec<String>,
}

impl ExtensionState {
    fn extensions_path() -> std::path::PathBuf {
        super::paths::vimcode_config_dir().join("extensions.json")
    }

    /// Load extension state from disk.  Returns `Default` when the file is
    /// absent or malformed.
    pub fn load() -> Self {
        let path = Self::extensions_path();
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str(&contents) {
                return state;
            }
        }
        Self::default()
    }

    /// Persist extension state (atomic write).
    pub fn save(&self) -> std::io::Result<()> {
        #[cfg(test)]
        return Ok(());

        #[cfg_attr(test, allow(unreachable_code))]
        if SUPPRESS_SAVES.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg_attr(test, allow(unreachable_code))]
        {
            let path = Self::extensions_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string_pretty(self)?;
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, &json)?;
            std::fs::rename(&tmp, &path)?;
            Ok(())
        }
    }

    /// Mark an extension as installed (with version), removing any dismissed entry.
    #[allow(dead_code)]
    pub fn mark_installed(&mut self, name: &str) {
        self.mark_installed_version(name, "");
    }

    /// Mark an extension as installed with a specific version.
    pub fn mark_installed_version(&mut self, name: &str, version: &str) {
        self.dismissed.retain(|n| n != name);
        if let Some(existing) = self.installed.iter_mut().find(|e| e.name == name) {
            // Update version of already-installed extension
            if !version.is_empty() {
                existing.version = version.to_string();
            }
        } else {
            self.installed.push(InstalledExtension {
                name: name.to_string(),
                version: version.to_string(),
            });
        }
    }

    /// Mark an extension as dismissed (suppress future install prompts).
    pub fn mark_dismissed(&mut self, name: &str) {
        if !self.dismissed.contains(&name.to_string()) {
            self.dismissed.push(name.to_string());
        }
    }

    pub fn is_installed(&self, name: &str) -> bool {
        self.installed.iter().any(|e| e.name == name)
    }

    /// Return the installed version of an extension, or empty string if not tracked.
    pub fn installed_version(&self, name: &str) -> &str {
        self.installed
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.version.as_str())
            .unwrap_or("")
    }

    pub fn is_dismissed(&self, name: &str) -> bool {
        self.dismissed.iter().any(|n| n == name)
    }
}

/// Recursive group layout for session persistence.
/// Each leaf stores the files open in that group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionGroupLayout {
    /// A single editor group with its open files.
    Leaf { files: Vec<PathBuf> },
    /// A split containing two sub-layouts.
    Split {
        /// 0 = Vertical (side-by-side), 1 = Horizontal (stacked).
        direction: u8,
        /// Split ratio (0.1..0.9).
        ratio: f64,
        first: Box<SessionGroupLayout>,
        second: Box<SessionGroupLayout>,
    },
}

/// Saved cursor and scroll position for a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePosition {
    pub line: usize,
    pub col: usize,
    pub scroll_top: usize,
}

/// Session state persisted across restarts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Window geometry
    pub window: WindowGeometry,

    /// Explorer sidebar visible on startup
    #[serde(default)]
    pub explorer_visible: bool,

    /// Sidebar panel width in pixels (GTK only, default 300)
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: i32,

    /// Last opened files (future: cursor positions)
    #[serde(default)]
    pub recent_files: Vec<PathBuf>,

    /// Saved cursor/scroll positions per file (keyed by canonical path)
    #[serde(default)]
    pub file_positions: HashMap<PathBuf, FilePosition>,

    /// All files that were open when the session was last saved (for restore on startup)
    #[serde(default)]
    pub open_files: Vec<PathBuf>,

    /// The active (focused) file when the session was last saved
    #[serde(default)]
    pub active_file: Option<PathBuf>,

    /// Terminal panel content rows (default 12; does not include the header row)
    #[serde(default = "default_terminal_rows")]
    pub terminal_panel_rows: u16,

    /// Recently opened workspace root paths (last 10, stored in global session only).
    #[serde(default)]
    pub recent_workspaces: Vec<PathBuf>,

    /// Files open in the second editor group (empty = single-group mode).
    #[serde(default)]
    pub open_files_group1: Vec<PathBuf>,

    /// Which editor group was active (0 or 1).
    #[serde(default)]
    pub active_group: usize,

    /// Split direction: 0 = Vertical (side-by-side), 1 = Horizontal (stacked).
    #[serde(default)]
    pub group_split_direction: u8,

    /// Editor group split ratio (0.2..0.8, default 0.5).
    #[serde(default = "default_group_split_ratio")]
    pub group_split_ratio: f64,

    /// Recursive group layout tree (new format).
    /// When present, takes priority over the flat open_files_group1/active_group fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_layout: Option<SessionGroupLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub width: i32,
    pub height: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowGeometry {
    /// Clamp a saved `x`/`y`/`width`/`height` so a monitor that was
    /// unplugged (or swapped for a smaller one) since the geometry was
    /// saved can never strand the restored window off-screen or oversized
    /// (#1529).
    ///
    /// Position: if `x`/`y` fall inside some connected display's `bounds`,
    /// that display becomes the clamp target and they pass through
    /// unchanged; otherwise (including when either is `None` — e.g. GTK,
    /// which never saves a real position at all — or when `displays` is
    /// empty, meaning `PlatformServices::displays()` reported
    /// `Unsupported` or genuinely found none) both are cleared to `None`
    /// so the caller falls back to whatever default placement the window
    /// manager/OS picks, rather than applying coordinates that used to be
    /// on a monitor that no longer exists.
    ///
    /// Size: clamped to fit the clamp target display — the one the saved
    /// position resolved to above, or (when position didn't resolve to
    /// one, including the "no position saved at all" GTK case) the
    /// largest connected display, so a window saved on a big external
    /// monitor doesn't reopen larger than a much smaller built-in panel
    /// left behind after that monitor is unplugged. Never grown — only
    /// ever shrunk to fit. Left unchanged if `displays` is empty.
    pub fn clamp_to_displays(&self, displays: &[quadraui::Display]) -> Self {
        let mut clamped = self.clone();

        let position_target = self.x.zip(self.y).and_then(|(x, y)| {
            let (xf, yf) = (x as f32, y as f32);
            displays.iter().find(|d| {
                let b = d.bounds;
                xf >= b.x && xf < b.x + b.width && yf >= b.y && yf < b.y + b.height
            })
        });
        if position_target.is_none() {
            clamped.x = None;
            clamped.y = None;
        }

        let size_target = position_target.or_else(|| {
            displays.iter().max_by(|a, b| {
                (a.bounds.width * a.bounds.height).total_cmp(&(b.bounds.width * b.bounds.height))
            })
        });
        if let Some(d) = size_target {
            clamped.width = clamped.width.min(d.bounds.width.round() as i32);
            clamped.height = clamped.height.min(d.bounds.height.round() as i32);
        }

        clamped
    }
}

fn default_sidebar_width() -> i32 {
    260
}

fn default_terminal_rows() -> u16 {
    12
}

fn default_group_split_ratio() -> f64 {
    0.5
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            window: WindowGeometry {
                width: 800,
                height: 600,
                x: None,
                y: None,
                maximized: false,
            },
            explorer_visible: false,
            sidebar_width: default_sidebar_width(),
            recent_files: Vec::new(),
            file_positions: HashMap::new(),
            open_files: Vec::new(),
            active_file: None,
            terminal_panel_rows: default_terminal_rows(),
            recent_workspaces: Vec::new(),
            open_files_group1: Vec::new(),
            active_group: 0,
            group_split_direction: 0,
            group_split_ratio: default_group_split_ratio(),
            group_layout: None,
        }
    }
}

impl SessionState {
    /// Load session state from ~/.config/vimcode/session.json
    pub fn load() -> Self {
        let path = Self::session_path();
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str(&contents) {
                return state;
            }
        }
        Self::default()
    }

    /// Save session state to ~/.config/vimcode/session.json
    ///
    /// Uses an atomic write: serialise → write to `.tmp` → rename.
    /// A rename is atomic on Linux/macOS (same filesystem), so a crash
    /// mid-write cannot corrupt the existing session file.
    pub fn save(&self) -> std::io::Result<()> {
        // Prevent races on ~/.config/vimcode/session.json during parallel tests
        #[cfg(test)]
        return Ok(());

        #[cfg_attr(test, allow(unreachable_code))]
        if SUPPRESS_SAVES.load(Ordering::Relaxed) {
            return Ok(());
        }

        #[cfg_attr(test, allow(unreachable_code))]
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn session_path() -> PathBuf {
        super::paths::vimcode_config_dir().join("session.json")
    }

    /// Compute a stable per-workspace session path based on the workspace root.
    /// Uses a simple FNV-1a 64-bit hash of the canonical path string.
    pub fn session_path_for_workspace(root: &Path) -> PathBuf {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let path_str = canonical.to_string_lossy();
        // FNV-1a 64-bit hash (deterministic, no external crates needed)
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in path_str.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x00000100000001b3);
        }
        super::paths::vimcode_config_dir()
            .join("sessions")
            .join(format!("{:016x}.json", hash))
    }

    /// Load per-workspace session state (open files, positions, etc.).
    /// Falls back to an empty session if the file does not exist.
    pub fn load_for_workspace(root: &Path) -> Self {
        let path = Self::session_path_for_workspace(root);
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str(&contents) {
                return state;
            }
        }
        Self::default()
    }

    /// Save per-workspace session state to the per-project file.
    pub fn save_for_workspace(&self, root: &Path) -> std::io::Result<()> {
        #[cfg(test)]
        {
            let _ = root;
            return Ok(());
        }

        #[cfg_attr(test, allow(unreachable_code))]
        if SUPPRESS_SAVES.load(Ordering::Relaxed) {
            let _ = root;
            return Ok(());
        }

        #[cfg_attr(test, allow(unreachable_code))]
        let path = Self::session_path_for_workspace(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Add a workspace root to `recent_workspaces` (max 10, removes duplicates).
    pub fn add_recent_workspace(&mut self, root: &Path) {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.recent_workspaces.retain(|p| p != &canonical);
        self.recent_workspaces.push(canonical);
        // Keep last 10
        while self.recent_workspaces.len() > 10 {
            self.recent_workspaces.remove(0);
        }
    }

    /// Save cursor and scroll position for a file path
    pub fn save_file_position(&mut self, path: &Path, line: usize, col: usize, scroll_top: usize) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.file_positions.insert(
            canonical,
            FilePosition {
                line,
                col,
                scroll_top,
            },
        );
    }

    /// Get saved cursor/scroll position for a file path, if any
    pub fn get_file_position(&self, path: &Path) -> Option<&FilePosition> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.file_positions.get(&canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_load_default() {
        let session = SessionState::default();
        assert_eq!(session.window.width, 800);
        assert_eq!(session.window.height, 600);
        assert!(!session.explorer_visible);
    }

    /// #1529: `WindowGeometry`'s `x`/`y`/`maximized` must round-trip through
    /// JSON, not just `width`/`height` (the fields `Default` already
    /// covered). Before this fix nothing ever wrote `x`/`y`/`maximized`, so
    /// a round-trip test against the un-fixed save path would have found
    /// them silently absent from the saved JSON.
    #[test]
    fn test_window_geometry_round_trip() {
        let geo = WindowGeometry {
            width: 1000,
            height: 700,
            x: Some(50),
            y: Some(75),
            maximized: true,
        };
        let json = serde_json::to_string(&geo).unwrap();
        let restored: WindowGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.width, 1000);
        assert_eq!(restored.height, 700);
        assert_eq!(restored.x, Some(50));
        assert_eq!(restored.y, Some(75));
        assert!(restored.maximized);
    }

    /// #1529: a saved position that falls inside a connected display's
    /// bounds passes through `clamp_to_displays` unchanged.
    #[test]
    fn test_window_geometry_clamp_keeps_position_on_known_display() {
        let geo = WindowGeometry {
            width: 1000,
            height: 700,
            x: Some(100),
            y: Some(100),
            maximized: false,
        };
        let displays = [quadraui::Display {
            bounds: quadraui::Rect::new(0.0, 0.0, 1920.0, 1080.0),
            work_area: quadraui::Rect::new(0.0, 0.0, 1920.0, 1080.0),
            scale: 1.0,
            primary: true,
        }];
        let clamped = geo.clamp_to_displays(&displays);
        assert_eq!(clamped.x, Some(100));
        assert_eq!(clamped.y, Some(100));
        // Size/maximized pass through untouched.
        assert_eq!(clamped.width, 1000);
        assert_eq!(clamped.height, 700);
        assert!(!clamped.maximized);
    }

    /// #1529: a saved position that no longer sits on *any* connected
    /// display (the monitor it was saved on was unplugged, or moved) is
    /// cleared to `None` rather than stranding the restored window
    /// off-screen.
    #[test]
    fn test_window_geometry_clamp_drops_offscreen_position() {
        let geo = WindowGeometry {
            width: 1000,
            height: 700,
            // Saved while a second monitor sat to the right of the
            // primary; that monitor is gone now.
            x: Some(2500),
            y: Some(200),
            maximized: false,
        };
        let displays = [quadraui::Display {
            bounds: quadraui::Rect::new(0.0, 0.0, 1920.0, 1080.0),
            work_area: quadraui::Rect::new(0.0, 0.0, 1920.0, 1080.0),
            scale: 1.0,
            primary: true,
        }];
        let clamped = geo.clamp_to_displays(&displays);
        assert_eq!(clamped.x, None);
        assert_eq!(clamped.y, None);
    }

    /// #1529: no connected displays at all (backend reports `Unsupported`,
    /// surfaced here as an empty slice) also clears the position — there is
    /// nothing to validate it against, so trusting a stale saved
    /// coordinate would be no safer than trusting an actually-offscreen
    /// one.
    #[test]
    fn test_window_geometry_clamp_no_displays_drops_position() {
        let geo = WindowGeometry {
            width: 1000,
            height: 700,
            x: Some(100),
            y: Some(100),
            maximized: false,
        };
        let clamped = geo.clamp_to_displays(&[]);
        assert_eq!(clamped.x, None);
        assert_eq!(clamped.y, None);
    }

    /// #1529 review (non-blocking finding): a window saved at 1000x700 on
    /// a display too small to hold it (a laptop's 1024x768 built-in panel,
    /// left behind after the bigger external monitor it was saved on was
    /// unplugged) must have its *size* shrunk to fit, not just its
    /// position cleared — an unclamped size could otherwise reopen larger
    /// than the only display left.
    #[test]
    fn test_window_geometry_clamp_shrinks_size_to_fit_the_target_display() {
        let geo = WindowGeometry {
            width: 1000,
            height: 700,
            x: Some(100),
            y: Some(100),
            maximized: false,
        };
        let displays = [quadraui::Display {
            bounds: quadraui::Rect::new(0.0, 0.0, 800.0, 600.0),
            work_area: quadraui::Rect::new(0.0, 0.0, 800.0, 600.0),
            scale: 1.0,
            primary: true,
        }];
        let clamped = geo.clamp_to_displays(&displays);
        // The saved position (100, 100) is still on this display, so it
        // survives — only the oversized dimensions are shrunk.
        assert_eq!(clamped.x, Some(100));
        assert_eq!(clamped.y, Some(100));
        assert_eq!(clamped.width, 800);
        assert_eq!(clamped.height, 600);
    }

    /// #1529 review: when the saved position no longer resolves to a
    /// display (or there was never one to begin with — GTK, structurally),
    /// size still gets clamped, against the *largest* connected display
    /// rather than left unbounded — an unplugged-monitor window shouldn't
    /// reopen bigger than anything the user has left connected.
    #[test]
    fn test_window_geometry_clamp_with_no_position_shrinks_size_to_largest_display() {
        let geo = WindowGeometry {
            width: 3000,
            height: 2000,
            x: None,
            y: None,
            maximized: false,
        };
        let displays = [
            quadraui::Display {
                bounds: quadraui::Rect::new(0.0, 0.0, 1024.0, 768.0),
                work_area: quadraui::Rect::new(0.0, 0.0, 1024.0, 768.0),
                scale: 1.0,
                primary: true,
            },
            quadraui::Display {
                bounds: quadraui::Rect::new(1024.0, 0.0, 1920.0, 1080.0),
                work_area: quadraui::Rect::new(1024.0, 0.0, 1920.0, 1080.0),
                scale: 1.0,
                primary: false,
            },
        ];
        let clamped = geo.clamp_to_displays(&displays);
        assert_eq!(clamped.width, 1920);
        assert_eq!(clamped.height, 1080);
    }

    #[test]
    fn test_history_state_add_command() {
        let mut h = HistoryState::default();
        h.add_command("w");
        h.add_command("q");
        assert_eq!(h.command_history, vec!["w", "q"]);

        // Duplicate: moved to end
        h.add_command("w");
        assert_eq!(h.command_history, vec!["q", "w"]);
    }

    #[test]
    fn test_history_state_add_search() {
        let mut h = HistoryState::default();
        h.add_search("hello");
        h.add_search("world");
        assert_eq!(h.search_history, vec!["hello", "world"]);

        // Duplicate: moved to end
        h.add_search("hello");
        assert_eq!(h.search_history, vec!["world", "hello"]);
    }

    #[test]
    fn test_history_limit() {
        let mut h = HistoryState::default();
        for i in 0..150 {
            h.add_command(&format!("cmd{}", i));
        }
        assert_eq!(h.command_history.len(), 100);
        // Should have kept the last 100
        assert_eq!(h.command_history[0], "cmd50");
        assert_eq!(h.command_history[99], "cmd149");
    }

    #[test]
    fn test_history_empty_strings_ignored() {
        let mut h = HistoryState::default();
        h.add_command("");
        h.add_search("");
        assert_eq!(h.command_history.len(), 0);
        assert_eq!(h.search_history.len(), 0);
    }

    #[test]
    fn test_save_and_get_file_position() {
        let mut session = SessionState::default();
        let path = Path::new("/tmp/test_vimcode_position.rs");

        // Nothing saved yet
        assert!(session.get_file_position(path).is_none());

        // Save a position
        session.save_file_position(path, 42, 7, 30);
        let pos = session.get_file_position(path).unwrap();
        assert_eq!(pos.line, 42);
        assert_eq!(pos.col, 7);
        assert_eq!(pos.scroll_top, 30);
    }

    #[test]
    fn test_file_position_overwrite() {
        let mut session = SessionState::default();
        let path = Path::new("/tmp/test_vimcode_position.rs");

        session.save_file_position(path, 10, 5, 0);
        session.save_file_position(path, 20, 3, 15);

        let pos = session.get_file_position(path).unwrap();
        assert_eq!(pos.line, 20);
        assert_eq!(pos.col, 3);
        assert_eq!(pos.scroll_top, 15);
    }

    #[test]
    fn test_file_positions_serialization() {
        let mut session = SessionState::default();
        let path = Path::new("/tmp/test_vimcode_serialize.py");
        session.save_file_position(path, 5, 2, 0);

        // Round-trip through JSON
        let json = serde_json::to_string(&session).unwrap();
        let restored: SessionState = serde_json::from_str(&json).unwrap();

        // Position should survive serialization (using canonical path)
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let pos = restored.file_positions.get(&canonical).unwrap();
        assert_eq!(pos.line, 5);
        assert_eq!(pos.col, 2);
    }

    #[test]
    fn test_workspace_session_path_is_stable() {
        let root = Path::new("/tmp");
        let path1 = SessionState::session_path_for_workspace(root);
        let path2 = SessionState::session_path_for_workspace(root);
        // Same input must always produce the same path
        assert_eq!(path1, path2);
        // Path must live under ~/.config/vimcode/sessions/
        let path_str = path1.to_string_lossy();
        assert!(
            path_str.contains("vimcode/sessions/") || path_str.contains("vimcode\\sessions\\"),
            "session path should contain vimcode sessions dir: {path_str}"
        );
        assert!(path_str.ends_with(".json"));
    }

    #[test]
    fn test_workspace_session_path_differs_per_root() {
        let root_a = Path::new("/tmp/proj_a");
        let root_b = Path::new("/tmp/proj_b");
        let path_a = SessionState::session_path_for_workspace(root_a);
        let path_b = SessionState::session_path_for_workspace(root_b);
        // Different roots must hash to different files
        assert_ne!(path_a, path_b);
    }

    #[test]
    fn test_add_recent_workspace() {
        let mut session = SessionState::default();
        let root = Path::new("/tmp/my_project");
        session.add_recent_workspace(root);
        assert_eq!(session.recent_workspaces.len(), 1);

        // Adding same path twice should not duplicate
        session.add_recent_workspace(root);
        assert_eq!(session.recent_workspaces.len(), 1);

        // Adding more than 10 paths keeps only the last 10
        for i in 0..12 {
            session.add_recent_workspace(&PathBuf::from(format!("/tmp/proj_{}", i)));
        }
        assert_eq!(session.recent_workspaces.len(), 10);
    }

    #[test]
    fn test_session_group_layout_serialization() {
        let layout = SessionGroupLayout::Split {
            direction: 0,
            ratio: 0.5,
            first: Box::new(SessionGroupLayout::Leaf {
                files: vec![PathBuf::from("/tmp/a.rs")],
            }),
            second: Box::new(SessionGroupLayout::Split {
                direction: 1,
                ratio: 0.6,
                first: Box::new(SessionGroupLayout::Leaf {
                    files: vec![PathBuf::from("/tmp/b.rs"), PathBuf::from("/tmp/c.rs")],
                }),
                second: Box::new(SessionGroupLayout::Leaf { files: vec![] }),
            }),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: SessionGroupLayout = serde_json::from_str(&json).unwrap();
        // Verify round-trip: top split is vertical (0).
        if let SessionGroupLayout::Split {
            direction, ratio, ..
        } = &restored
        {
            assert_eq!(*direction, 0);
            assert!((ratio - 0.5).abs() < f64::EPSILON);
        } else {
            panic!("expected Split");
        }
    }

    #[test]
    fn test_extension_state_mark_installed() {
        let mut es = ExtensionState::default();
        assert!(!es.is_installed("csharp"));
        es.mark_installed("csharp");
        assert!(es.is_installed("csharp"));
        // idempotent
        es.mark_installed("csharp");
        assert_eq!(
            es.installed.iter().filter(|e| e.name == "csharp").count(),
            1
        );
    }

    #[test]
    fn test_extension_state_mark_dismissed() {
        let mut es = ExtensionState::default();
        es.mark_dismissed("java");
        assert!(es.is_dismissed("java"));
        // idempotent
        es.mark_dismissed("java");
        assert_eq!(es.dismissed.iter().filter(|n| *n == "java").count(), 1);
    }

    #[test]
    fn test_extension_state_install_clears_dismissed() {
        let mut es = ExtensionState::default();
        es.mark_dismissed("python");
        assert!(es.is_dismissed("python"));
        es.mark_installed("python");
        assert!(es.is_installed("python"));
        assert!(!es.is_dismissed("python"));
    }

    #[test]
    fn test_extension_state_serialization() {
        let mut es = ExtensionState::default();
        es.mark_installed("csharp");
        es.mark_dismissed("java");
        let json = serde_json::to_string(&es).unwrap();
        let restored: ExtensionState = serde_json::from_str(&json).unwrap();
        assert!(restored.is_installed("csharp"));
        assert!(restored.is_dismissed("java"));
    }

    #[test]
    fn test_session_group_layout_backward_compat() {
        // Old session JSON (no group_layout field) should deserialize with group_layout = None.
        let json = r#"{
            "window": {"width": 800, "height": 600, "maximized": false},
            "explorer_visible": false,
            "sidebar_width": 260,
            "recent_files": [],
            "file_positions": {},
            "open_files": ["/tmp/x.rs"],
            "terminal_panel_rows": 12,
            "recent_workspaces": [],
            "open_files_group1": [],
            "active_group": 0,
            "group_split_direction": 0,
            "group_split_ratio": 0.5
        }"#;
        let session: SessionState = serde_json::from_str(json).unwrap();
        assert!(session.group_layout.is_none());
        assert_eq!(session.open_files, vec![PathBuf::from("/tmp/x.rs")]);
    }
}
