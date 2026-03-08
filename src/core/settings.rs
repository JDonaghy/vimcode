use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Which editing paradigm the editor uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EditorMode {
    /// Classic modal Vim key-bindings (default).
    #[default]
    Vim,
    /// VSCode-style always-insert editing (Shift+Arrow select, Ctrl-C/X/V/Z/Y/A).
    Vscode,
}

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
/// ```rust,ignore
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

    /// Enable LSP support (auto-starts language servers for supported files)
    #[serde(default = "default_lsp_enabled")]
    pub lsp_enabled: bool,

    /// Automatically format the buffer via LSP before saving (default: false).
    #[serde(default)]
    pub format_on_save: bool,

    /// Number of lines kept in the integrated terminal's scrollback history.
    /// Increase for commands that produce very long output. Default: 5000.
    #[serde(default = "default_terminal_scrollback_lines")]
    pub terminal_scrollback_lines: usize,

    /// User-configured LSP server overrides/additions
    #[serde(default)]
    pub lsp_servers: Vec<crate::core::lsp::LspServerConfig>,

    /// User-configured file extension → LSP language ID overrides.
    /// Keys are extensions without the dot (e.g. "cs", "h").
    /// Values are LSP language IDs (e.g. "csharp", "cpp").
    /// Example: { "h": "cpp", "mjs": "javascript" }
    #[serde(default)]
    pub language_map: std::collections::HashMap<String, String>,

    /// Configurable explorer key bindings (in-tree CRUD operations)
    #[serde(default)]
    pub explorer_keys: ExplorerKeys,

    /// Global panel navigation key bindings
    #[serde(default)]
    pub panel_keys: PanelKeys,

    /// Completion popup key bindings
    #[serde(default)]
    pub completion_keys: CompletionKeys,

    /// Editing mode: `vim` (modal) or `vscode` (always-insert).
    #[serde(default)]
    pub editor_mode: EditorMode,

    /// Single character used as the leader key prefix in normal mode.
    /// Default is Space (' '). Override in settings.json: { "leader": "\\" }
    #[serde(default = "default_leader")]
    pub leader: char,

    /// Wrap long lines at the viewport width instead of scrolling horizontally.
    /// When true, lines longer than the window width are wrapped to the next
    /// visual row. Corresponds to Vim's `:set wrap` / `:set nowrap`.
    #[serde(default)]
    pub wrap: bool,

    /// Whether the Lua plugin system is enabled (default true).
    #[serde(default = "default_plugins_enabled")]
    pub plugins_enabled: bool,

    /// Names of plugins that have been explicitly disabled via `:Plugin disable`.
    #[serde(default)]
    pub disabled_plugins: Vec<String>,

    /// Highlight all search matches (default true). Disable with `:set nohlsearch`.
    #[serde(default = "default_hlsearch")]
    pub hlsearch: bool,

    /// Case-insensitive search (default false). Enable with `:set ignorecase`.
    #[serde(default)]
    pub ignorecase: bool,

    /// Override `ignorecase` when the pattern has an uppercase letter (default false).
    /// Only has effect when `ignorecase` is also set.
    #[serde(default)]
    pub smartcase: bool,

    /// Number of lines to keep visible above/below the cursor (default 0).
    #[serde(default)]
    pub scrolloff: usize,

    /// Highlight the line the cursor is on (default false).
    #[serde(default)]
    pub cursorline: bool,

    /// Automatically reload files when changed externally (default true).
    /// Vim: `autoread`.
    #[serde(default = "default_autoread")]
    pub autoread: bool,

    /// Open new horizontal splits below the current window (default false).
    #[serde(default)]
    pub splitbelow: bool,

    /// Open new vertical splits to the right of the current window (default false).
    #[serde(default)]
    pub splitright: bool,

    /// Comma-separated list of columns to highlight as color columns (e.g. "80,120").
    /// Empty string means no color columns.
    #[serde(default)]
    pub colorcolumn: String,

    /// Auto-wrap inserted text at this column (0 = disabled). Corresponds to Vim's `textwidth`.
    #[serde(default)]
    pub textwidth: usize,

    /// URL of the extension registry JSON file.
    /// Default points to the official VimCode GitHub registry.
    /// Override for self-hosted or GitHub Pages custom domain setups.
    #[serde(default = "default_extension_registry_url")]
    pub extension_registry_url: String,

    /// Name of the active colour scheme. Built-in options: "onedark" (default),
    /// "gruvbox-dark", "tokyo-night", "solarized-dark".
    /// Select with `:colorscheme <name>`.
    #[serde(default = "default_colorscheme")]
    pub colorscheme: String,

    // ── AI Assistant ──────────────────────────────────────────────────────────
    /// AI provider: "anthropic" (default), "openai", or "ollama".
    #[serde(default = "default_ai_provider")]
    pub ai_provider: String,

    /// API key for Anthropic or OpenAI. Leave empty for Ollama.
    #[serde(default)]
    pub ai_api_key: String,

    /// Override model name. Empty = provider default
    /// (claude-sonnet-4-6 / gpt-4o / llama3.2).
    #[serde(default)]
    pub ai_model: String,

    /// Override base URL for the API endpoint. Empty = provider default.
    /// Useful for OpenAI-compatible local servers or proxies.
    #[serde(default)]
    pub ai_base_url: String,

    /// Enable AI inline completions (ghost text at cursor in insert mode).
    /// Requires a configured AI provider and API key.
    /// Default: false (opt-in due to API cost per keystroke).
    #[serde(default)]
    pub ai_completions: bool,

    // ── Explorer ──────────────────────────────────────────────────────────────
    /// Show hidden files (dotfiles) in the file explorer (default: false).
    #[serde(default)]
    pub show_hidden_files: bool,

    // ── Swap files ────────────────────────────────────────────────────────────
    /// Enable swap file crash recovery (default: true).
    #[serde(default = "default_swap_file")]
    pub swap_file: bool,

    /// Milliseconds between swap file writes for dirty buffers (default: 4000).
    #[serde(default = "default_updatetime")]
    pub updatetime: u32,
}

fn default_swap_file() -> bool {
    true
}

fn default_updatetime() -> u32 {
    4000
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

fn default_lsp_enabled() -> bool {
    true // Default: enabled
}

fn default_plugins_enabled() -> bool {
    true
}

fn default_hlsearch() -> bool {
    true
}

fn default_autoread() -> bool {
    true
}

fn default_terminal_scrollback_lines() -> usize {
    5000
}

fn default_leader() -> char {
    ' '
}

fn default_extension_registry_url() -> String {
    crate::core::registry::REGISTRY_URL.to_string()
}

fn default_colorscheme() -> String {
    "onedark".to_string()
}

fn default_ai_provider() -> String {
    "anthropic".to_string()
}

// ── Explorer key defaults ──────────────────────────────────────────────────

fn ek_new_file() -> String {
    "a".to_string()
}
fn ek_new_folder() -> String {
    "A".to_string()
}
fn ek_delete() -> String {
    "D".to_string()
}
fn ek_rename() -> String {
    "r".to_string()
}
fn ek_move_file() -> String {
    "M".to_string()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerAction {
    NewFile,
    NewFolder,
    Delete,
    Rename,
    MoveFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerKeys {
    #[serde(default = "ek_new_file")]
    pub new_file: String,
    #[serde(default = "ek_new_folder")]
    pub new_folder: String,
    #[serde(default = "ek_delete")]
    pub delete: String,
    #[serde(default = "ek_rename")]
    pub rename: String,
    #[serde(default = "ek_move_file")]
    pub move_file: String,
}

impl Default for ExplorerKeys {
    fn default() -> Self {
        ExplorerKeys {
            new_file: ek_new_file(),
            new_folder: ek_new_folder(),
            delete: ek_delete(),
            rename: ek_rename(),
            move_file: ek_move_file(),
        }
    }
}

impl ExplorerKeys {
    /// Resolve a typed character to an explorer action.
    /// Only single-character bindings are supported.
    pub fn resolve(&self, ch: char) -> Option<ExplorerAction> {
        let s = ch.to_string();
        if s == self.new_file {
            Some(ExplorerAction::NewFile)
        } else if s == self.new_folder {
            Some(ExplorerAction::NewFolder)
        } else if s == self.delete {
            Some(ExplorerAction::Delete)
        } else if s == self.rename {
            Some(ExplorerAction::Rename)
        } else if s == self.move_file {
            Some(ExplorerAction::MoveFile)
        } else {
            None
        }
    }
}

// ── Panel / global key defaults ────────────────────────────────────────────

fn pk_toggle_sidebar() -> String {
    "<C-b>".to_string()
}
fn pk_focus_explorer() -> String {
    "<A-e>".to_string()
}
fn pk_focus_search() -> String {
    "<A-f>".to_string()
}
fn pk_fuzzy_finder() -> String {
    "<C-p>".to_string()
}
fn pk_live_grep() -> String {
    "<C-g>".to_string()
}
fn pk_open_terminal() -> String {
    "<C-t>".to_string()
}
fn pk_add_cursor() -> String {
    "<A-d>".to_string()
}
fn pk_select_all_matches() -> String {
    "<C-S-l>".to_string()
}

/// Global keyboard shortcuts for panel navigation.
///
/// Keys are specified in Vim-style notation:
/// - `<C-x>` — Ctrl+x
/// - `<C-S-x>` — Ctrl+Shift+x
/// - `<A-x>` — Alt+x
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelKeys {
    /// Toggle sidebar visibility. Default: `<C-b>`
    #[serde(default = "pk_toggle_sidebar")]
    pub toggle_sidebar: String,
    /// Focus explorer (or return to editor if already focused). Default: `<A-e>`
    #[serde(default = "pk_focus_explorer")]
    pub focus_explorer: String,
    /// Open search panel in sidebar. Default: `<A-f>`
    #[serde(default = "pk_focus_search")]
    pub focus_search: String,
    /// Open fuzzy file finder. Default: `<C-p>`
    #[serde(default = "pk_fuzzy_finder")]
    pub fuzzy_finder: String,
    /// Open live grep modal. Default: `<C-g>`
    #[serde(default = "pk_live_grep")]
    pub live_grep: String,
    /// Toggle integrated terminal panel. Default: `<C-t>`
    #[serde(default = "pk_open_terminal")]
    pub open_terminal: String,
    /// Add cursor at next match of word under cursor. Default: `<A-d>`
    #[serde(default = "pk_add_cursor")]
    pub add_cursor: String,
    /// Select all occurrences of word under cursor. Default: `<C-S-l>`
    #[serde(default = "pk_select_all_matches")]
    pub select_all_matches: String,
    /// Split the active editor group to the right (vertical split). Default: `""` (use Ctrl+\).
    /// Example: `"<C-|>"` to bind Ctrl+|.
    #[serde(default)]
    pub split_editor_right: String,
    /// Split the active editor group downward (horizontal split). Default: `""` (unbound).
    /// Example: `"<C-_>"` to bind Ctrl+_.
    #[serde(default)]
    pub split_editor_down: String,
}

impl Default for PanelKeys {
    fn default() -> Self {
        PanelKeys {
            toggle_sidebar: pk_toggle_sidebar(),
            focus_explorer: pk_focus_explorer(),
            focus_search: pk_focus_search(),
            fuzzy_finder: pk_fuzzy_finder(),
            live_grep: pk_live_grep(),
            open_terminal: pk_open_terminal(),
            add_cursor: pk_add_cursor(),
            select_all_matches: pk_select_all_matches(),
            split_editor_right: String::new(),
            split_editor_down: String::new(),
        }
    }
}

fn default_completion_trigger() -> String {
    "<C-Space>".to_string()
}
fn default_completion_accept() -> String {
    "Tab".to_string()
}

/// Key bindings for the auto-popup completion menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionKeys {
    /// Key to manually trigger completion popup. Default: `<C-Space>`
    #[serde(default = "default_completion_trigger")]
    pub trigger: String,
    /// Key to accept the highlighted completion item. Default: `Tab`
    #[serde(default = "default_completion_accept")]
    pub accept: String,
}

impl Default for CompletionKeys {
    fn default() -> Self {
        Self {
            trigger: default_completion_trigger(),
            accept: default_completion_accept(),
        }
    }
}

/// Parse a Vim-style key binding string into `(ctrl, shift, alt, lowercase_char)`.
///
/// Supported formats: `<C-b>`, `<C-S-e>`, `<A-x>`, `<C-A-x>`.
/// Returns `None` if the format is not recognised.
pub fn parse_key_binding(s: &str) -> Option<(bool, bool, bool, char)> {
    let (ctrl, shift, alt, key_str) = parse_key_binding_named(s)?;
    // For backward compat: named keys map to sentinel chars, single chars pass through.
    let ch = match key_str.as_str() {
        "Space" | "space" => ' ',
        _ => {
            if key_str.chars().count() != 1 {
                return None;
            }
            key_str.chars().next()?
        }
    };
    Some((ctrl, shift, alt, ch.to_ascii_lowercase()))
}

/// Extended key binding parser that returns the key name as a string.
/// Supports named keys like `Tab`, `Space`, `Escape`, etc.
pub fn parse_key_binding_named(s: &str) -> Option<(bool, bool, bool, String)> {
    let s = s.trim();
    if !s.starts_with('<') || !s.ends_with('>') {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    let parts: Vec<&str> = inner.split('-').collect();
    if parts.len() < 2 {
        return None;
    }
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    for part in &parts[..parts.len() - 1] {
        match *part {
            "C" => ctrl = true,
            "S" => shift = true,
            "A" => alt = true,
            _ => return None,
        }
    }
    let key_str = parts[parts.len() - 1].to_string();
    Some((ctrl, shift, alt, key_str))
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
            lsp_enabled: default_lsp_enabled(),
            format_on_save: false,
            lsp_servers: Vec::new(),
            language_map: std::collections::HashMap::new(),
            terminal_scrollback_lines: default_terminal_scrollback_lines(),
            explorer_keys: ExplorerKeys::default(),
            panel_keys: PanelKeys::default(),
            completion_keys: CompletionKeys::default(),
            editor_mode: EditorMode::Vim,
            leader: default_leader(),
            wrap: false,
            plugins_enabled: default_plugins_enabled(),
            disabled_plugins: Vec::new(),
            hlsearch: default_hlsearch(),
            ignorecase: false,
            smartcase: false,
            scrolloff: 0,
            cursorline: false,
            autoread: default_autoread(),
            splitbelow: false,
            splitright: false,
            colorcolumn: String::new(),
            textwidth: 0,
            extension_registry_url: default_extension_registry_url(),
            colorscheme: default_colorscheme(),
            ai_provider: default_ai_provider(),
            ai_api_key: String::new(),
            ai_model: String::new(),
            ai_base_url: String::new(),
            ai_completions: false,
            show_hidden_files: false,
            swap_file: default_swap_file(),
            updatetime: default_updatetime(),
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
        // Tests must be hermetic — never read the user's settings.json.
        #[cfg(test)]
        return Self::default();

        #[cfg_attr(test, allow(unreachable_code))]
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
        let path = Self::settings_file_path();

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

        // Toggle or explicitly set a boolean option with ! suffix.
        // :set wrap!   → toggle
        // :set nowrap! → disable (no<opt>! is an explicit disable, not a toggle)
        if let Some(opt) = arg.strip_suffix('!') {
            let opt = opt.trim();
            if let Some(base) = opt.strip_prefix("no") {
                // :set nowrap! — explicit disable
                self.set_bool_option(base, false)?;
                return Ok(format!("no{base}"));
            }
            let current = self.query_option(opt)?;
            if current.contains('=') {
                return Err(format!("Option '{opt}' cannot be toggled"));
            }
            let currently_enabled = !current.starts_with("no");
            self.set_bool_option(opt, !currently_enabled)?;
            return Ok(if !currently_enabled {
                opt.to_string()
            } else {
                format!("no{opt}")
            });
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
        let lsp = if self.lsp_enabled { "lsp" } else { "nolsp" };
        let fos = if self.format_on_save {
            "formatonsave"
        } else {
            "noformatonsave"
        };
        let mode = match self.editor_mode {
            EditorMode::Vim => "mode=vim",
            EditorMode::Vscode => "mode=vscode",
        };
        let wrap = if self.wrap { "wrap" } else { "nowrap" };
        let hls = if self.hlsearch {
            "hlsearch"
        } else {
            "nohlsearch"
        };
        let ic = if self.ignorecase {
            "ignorecase"
        } else {
            "noignorecase"
        };
        let sc = if self.smartcase {
            "smartcase"
        } else {
            "nosmartcase"
        };
        format!(
            "{}  {}  ts={}  sw={}  {}  {}  {}  {}  {}  {}  {}  {}  {}  so={}  tw={}",
            num,
            et,
            self.tabstop,
            self.shift_width,
            ai,
            is,
            lsp,
            fos,
            mode,
            wrap,
            hls,
            ic,
            sc,
            self.scrolloff,
            self.textwidth
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
            "lsp" => self.lsp_enabled = enable,
            "wrap" => self.wrap = enable,
            "hlsearch" | "hls" => self.hlsearch = enable,
            "ignorecase" | "ic" => self.ignorecase = enable,
            "smartcase" | "scs" => self.smartcase = enable,
            "cursorline" | "cul" => self.cursorline = enable,
            "autoread" | "ar" => self.autoread = enable,
            "splitbelow" | "sb" => self.splitbelow = enable,
            "splitright" | "spr" => self.splitright = enable,
            "ai_completions" => self.ai_completions = enable,
            "formatonsave" | "fos" => self.format_on_save = enable,
            "showhiddenfiles" | "shf" => self.show_hidden_files = enable,
            "swapfile" => self.swap_file = enable,
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
            "mode" | "editor_mode" => match value {
                "vim" => self.editor_mode = EditorMode::Vim,
                "vscode" => self.editor_mode = EditorMode::Vscode,
                _ => return Err(format!("Unknown mode '{}' (vim|vscode)", value)),
            },
            "scrolloff" | "so" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.scrolloff = n;
            }
            "colorcolumn" | "cc" => {
                self.colorcolumn = value.to_string();
            }
            "textwidth" | "tw" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.textwidth = n;
            }
            "updatetime" | "ut" => {
                let n: u32 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.updatetime = n;
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
            "lsp" => Ok(if self.lsp_enabled {
                "lsp".to_string()
            } else {
                "nolsp".to_string()
            }),
            "mode" | "editor_mode" => Ok(format!(
                "mode={}",
                match self.editor_mode {
                    EditorMode::Vim => "vim",
                    EditorMode::Vscode => "vscode",
                }
            )),
            "wrap" => Ok(if self.wrap {
                "wrap".to_string()
            } else {
                "nowrap".to_string()
            }),
            "hlsearch" | "hls" => Ok(if self.hlsearch {
                "hlsearch".to_string()
            } else {
                "nohlsearch".to_string()
            }),
            "ignorecase" | "ic" => Ok(if self.ignorecase {
                "ignorecase".to_string()
            } else {
                "noignorecase".to_string()
            }),
            "smartcase" | "scs" => Ok(if self.smartcase {
                "smartcase".to_string()
            } else {
                "nosmartcase".to_string()
            }),
            "scrolloff" | "so" => Ok(format!("scrolloff={}", self.scrolloff)),
            "cursorline" | "cul" => Ok(if self.cursorline {
                "cursorline".to_string()
            } else {
                "nocursorline".to_string()
            }),
            "splitbelow" | "sb" => Ok(if self.splitbelow {
                "splitbelow".to_string()
            } else {
                "nosplitbelow".to_string()
            }),
            "splitright" | "spr" => Ok(if self.splitright {
                "splitright".to_string()
            } else {
                "nosplitright".to_string()
            }),
            "colorcolumn" | "cc" => Ok(format!("colorcolumn={}", self.colorcolumn)),
            "textwidth" | "tw" => Ok(format!("textwidth={}", self.textwidth)),
            "formatonsave" | "fos" => Ok(if self.format_on_save {
                "formatonsave".to_string()
            } else {
                "noformatonsave".to_string()
            }),
            "showhiddenfiles" | "shf" => Ok(if self.show_hidden_files {
                "showhiddenfiles".to_string()
            } else {
                "noshowhiddenfiles".to_string()
            }),
            "swapfile" => Ok(if self.swap_file {
                "swapfile".to_string()
            } else {
                "noswapfile".to_string()
            }),
            "updatetime" | "ut" => Ok(format!("updatetime={}", self.updatetime)),
            _ => Err(format!("Unknown option: {opt}")),
        }
    }

    /// Save settings to ~/.config/vimcode/settings.json
    pub fn save(&self) -> std::io::Result<()> {
        // Unit tests must not write to the user's real settings file.
        #[cfg(test)]
        return Ok(());

        // Integration tests compile the library without cfg(test), so the guard above
        // does not fire. Check the runtime flag instead.
        #[cfg_attr(test, allow(unreachable_code))]
        if crate::core::session::saves_suppressed() {
            return Ok(());
        }

        let path = Self::settings_file_path();

        // Create config directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(&path, contents)?;

        Ok(())
    }

    pub fn settings_file_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config")
            .join("vimcode")
            .join("settings.json")
    }

    /// Get the current value of a setting as a string.
    /// Returns an empty string for unknown keys.
    /// Used by the settings UI form to populate widget initial values.
    pub fn get_value_str(&self, key: &str) -> String {
        match key {
            "colorscheme" => self.colorscheme.clone(),
            "font_family" => self.font_family.clone(),
            "font_size" => self.font_size.to_string(),
            "line_numbers" => match self.line_numbers {
                LineNumberMode::None => "none".to_string(),
                LineNumberMode::Absolute => "absolute".to_string(),
                LineNumberMode::Relative => "relative".to_string(),
                LineNumberMode::Hybrid => "hybrid".to_string(),
            },
            "cursorline" => self.cursorline.to_string(),
            "tabstop" => self.tabstop.to_string(),
            "shift_width" => self.shift_width.to_string(),
            "expand_tab" => self.expand_tab.to_string(),
            "auto_indent" => self.auto_indent.to_string(),
            "wrap" => self.wrap.to_string(),
            "scrolloff" => self.scrolloff.to_string(),
            "colorcolumn" => self.colorcolumn.clone(),
            "textwidth" => self.textwidth.to_string(),
            "hlsearch" => self.hlsearch.to_string(),
            "ignorecase" => self.ignorecase.to_string(),
            "smartcase" => self.smartcase.to_string(),
            "incremental_search" => self.incremental_search.to_string(),
            "editor_mode" => match self.editor_mode {
                EditorMode::Vim => "vim".to_string(),
                EditorMode::Vscode => "vscode".to_string(),
            },
            "explorer_visible_on_startup" => self.explorer_visible_on_startup.to_string(),
            "autoread" => self.autoread.to_string(),
            "splitbelow" => self.splitbelow.to_string(),
            "splitright" => self.splitright.to_string(),
            "lsp_enabled" => self.lsp_enabled.to_string(),
            "format_on_save" => self.format_on_save.to_string(),
            "terminal_scrollback_lines" => self.terminal_scrollback_lines.to_string(),
            "plugins_enabled" => self.plugins_enabled.to_string(),
            "ai_provider" => self.ai_provider.clone(),
            "ai_api_key" => self.ai_api_key.clone(),
            "ai_model" => self.ai_model.clone(),
            "ai_base_url" => self.ai_base_url.clone(),
            "ai_completions" => self.ai_completions.to_string(),
            "showhiddenfiles" | "shf" | "show_hidden_files" => self.show_hidden_files.to_string(),
            "swapfile" | "swap_file" => self.swap_file.to_string(),
            "updatetime" | "ut" => self.updatetime.to_string(),
            _ => String::new(),
        }
    }

    /// Set a setting value from a string.
    /// Returns an error if the key or value is invalid.
    /// Does not persist to disk — call `save()` afterwards.
    pub fn set_value_str(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "colorscheme" => self.colorscheme = value.to_string(),
            "font_family" => self.font_family = value.to_string(),
            "font_size" => {
                self.font_size = value
                    .parse()
                    .map_err(|_| format!("Invalid font_size: {value}"))?;
            }
            "line_numbers" => {
                self.line_numbers = match value {
                    "none" => LineNumberMode::None,
                    "absolute" => LineNumberMode::Absolute,
                    "relative" => LineNumberMode::Relative,
                    "hybrid" => LineNumberMode::Hybrid,
                    _ => return Err(format!("Unknown line_numbers value: {value}")),
                };
            }
            "cursorline" => self.cursorline = value == "true",
            "tabstop" => {
                self.tabstop = value
                    .parse()
                    .map_err(|_| format!("Invalid tabstop: {value}"))?;
            }
            "shift_width" => {
                self.shift_width = value
                    .parse()
                    .map_err(|_| format!("Invalid shift_width: {value}"))?;
            }
            "expand_tab" => self.expand_tab = value == "true",
            "auto_indent" => self.auto_indent = value == "true",
            "wrap" => self.wrap = value == "true",
            "scrolloff" => {
                self.scrolloff = value
                    .parse()
                    .map_err(|_| format!("Invalid scrolloff: {value}"))?;
            }
            "colorcolumn" => self.colorcolumn = value.to_string(),
            "textwidth" => {
                self.textwidth = value
                    .parse()
                    .map_err(|_| format!("Invalid textwidth: {value}"))?;
            }
            "hlsearch" => self.hlsearch = value == "true",
            "ignorecase" => self.ignorecase = value == "true",
            "smartcase" => self.smartcase = value == "true",
            "incremental_search" => self.incremental_search = value == "true",
            "editor_mode" => {
                self.editor_mode = match value {
                    "vim" => EditorMode::Vim,
                    "vscode" => EditorMode::Vscode,
                    _ => return Err(format!("Unknown editor_mode: {value}")),
                };
            }
            "explorer_visible_on_startup" => self.explorer_visible_on_startup = value == "true",
            "autoread" => self.autoread = value == "true",
            "splitbelow" => self.splitbelow = value == "true",
            "splitright" => self.splitright = value == "true",
            "lsp_enabled" => self.lsp_enabled = value == "true",
            "format_on_save" => self.format_on_save = value == "true",
            "terminal_scrollback_lines" => {
                self.terminal_scrollback_lines = value
                    .parse()
                    .map_err(|_| format!("Invalid terminal_scrollback_lines: {value}"))?;
            }
            "plugins_enabled" => self.plugins_enabled = value == "true",
            "ai_provider" => match value {
                "anthropic" | "openai" | "ollama" => self.ai_provider = value.to_string(),
                _ => return Err(format!("Unknown ai_provider: {value}")),
            },
            "ai_api_key" => self.ai_api_key = value.to_string(),
            "ai_model" => self.ai_model = value.to_string(),
            "ai_base_url" => self.ai_base_url = value.to_string(),
            "ai_completions" => self.ai_completions = value == "true",
            "showhiddenfiles" | "shf" | "show_hidden_files" => {
                self.show_hidden_files = value == "true"
            }
            "swapfile" | "swap_file" => self.swap_file = value == "true",
            "updatetime" | "ut" => {
                self.updatetime = value
                    .parse()
                    .map_err(|_| format!("Invalid updatetime: {value}"))?;
            }
            _ => return Err(format!("Unknown setting key: {key}")),
        }
        Ok(())
    }

    /// Ensure settings.json exists with default values
    /// Creates the file if missing. Note that existing files are automatically updated
    /// when Settings::load() is called - it adds new fields with defaults while preserving
    /// existing user settings.
    pub fn ensure_exists() -> Result<(), std::io::Error> {
        let path = Self::settings_file_path();

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

    fn test_settings_path_load_save() -> PathBuf {
        std::env::temp_dir().join("vimcode_test_settings_load_save.json")
    }

    fn test_settings_path_invalid_json() -> PathBuf {
        std::env::temp_dir().join("vimcode_test_settings_invalid_json.json")
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
        let test_path = test_settings_path_load_save();

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
        let test_path = test_settings_path_invalid_json();

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

    #[test]
    fn test_settings_wrap_default_is_false() {
        let s = Settings::default();
        assert!(!s.wrap);
    }

    #[test]
    fn test_settings_set_wrap() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("wrap").unwrap();
        assert_eq!(msg, "wrap");
        assert!(s.wrap);
        let msg = s.parse_set_option("nowrap").unwrap();
        assert_eq!(msg, "nowrap");
        assert!(!s.wrap);
    }

    #[test]
    fn test_settings_query_wrap() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("wrap?").unwrap();
        assert_eq!(msg, "nowrap");
        s.wrap = true;
        let msg = s.parse_set_option("wrap?").unwrap();
        assert_eq!(msg, "wrap");
    }

    #[test]
    fn test_toggle_bang_wrap() {
        let mut s = Settings::default();
        assert!(!s.wrap);
        // :set wrap! toggles from off → on
        let msg = s.parse_set_option("wrap!").unwrap();
        assert_eq!(msg, "wrap");
        assert!(s.wrap);
        // :set wrap! toggles from on → off
        let msg = s.parse_set_option("wrap!").unwrap();
        assert_eq!(msg, "nowrap");
        assert!(!s.wrap);
    }

    #[test]
    fn test_nowrap_bang_disables() {
        let mut s = Settings::default();
        s.wrap = true;
        // :set nowrap! explicitly disables wrap
        let msg = s.parse_set_option("nowrap!").unwrap();
        assert_eq!(msg, "nowrap");
        assert!(!s.wrap);
        // idempotent: already off, stays off
        let msg = s.parse_set_option("nowrap!").unwrap();
        assert_eq!(msg, "nowrap");
        assert!(!s.wrap);
    }

    #[test]
    fn test_toggle_bang_expandtab() {
        let mut s = Settings::default();
        let initial = s.expand_tab;
        s.parse_set_option("expandtab!").unwrap();
        assert_eq!(s.expand_tab, !initial);
        s.parse_set_option("et!").unwrap();
        assert_eq!(s.expand_tab, initial);
    }

    #[test]
    fn test_toggle_bang_nonbool_is_error() {
        let mut s = Settings::default();
        assert!(s.parse_set_option("tabstop!").is_err());
        assert!(s.parse_set_option("ts!").is_err());
    }

    #[test]
    fn test_display_all_includes_wrap() {
        let mut s = Settings::default();
        assert!(s.display_all().contains("nowrap"));
        s.wrap = true;
        assert!(s.display_all().contains("wrap"));
    }

    // ── ExplorerKeys tests ──────────────────────────────────────────────────

    #[test]
    fn test_explorer_keys_default() {
        let ek = ExplorerKeys::default();
        assert_eq!(ek.new_file, "a");
        assert_eq!(ek.new_folder, "A");
        assert_eq!(ek.delete, "D");
        assert_eq!(ek.rename, "r");
        assert_eq!(ek.move_file, "M");
    }

    #[test]
    fn test_explorer_keys_resolve() {
        let ek = ExplorerKeys::default();
        assert_eq!(ek.resolve('a'), Some(ExplorerAction::NewFile));
        assert_eq!(ek.resolve('A'), Some(ExplorerAction::NewFolder));
        assert_eq!(ek.resolve('D'), Some(ExplorerAction::Delete));
        assert_eq!(ek.resolve('r'), Some(ExplorerAction::Rename));
        assert_eq!(ek.resolve('M'), Some(ExplorerAction::MoveFile));
        assert_eq!(ek.resolve('?'), None);
        assert_eq!(ek.resolve('z'), None);
    }

    #[test]
    fn test_explorer_keys_custom_override() {
        let mut ek = ExplorerKeys::default();
        ek.delete = "x".to_string();
        assert_eq!(ek.resolve('x'), Some(ExplorerAction::Delete));
        assert_eq!(ek.resolve('D'), None); // old key no longer works
                                           // Other keys still work
        assert_eq!(ek.resolve('a'), Some(ExplorerAction::NewFile));
    }

    #[test]
    fn test_explorer_keys_serde_partial() {
        let json = r#"{ "delete": "x" }"#;
        let ek: ExplorerKeys = serde_json::from_str(json).unwrap();
        assert_eq!(ek.delete, "x");
        // Unspecified fields keep defaults
        assert_eq!(ek.new_file, "a");
        assert_eq!(ek.new_folder, "A");
        assert_eq!(ek.rename, "r");
        assert_eq!(ek.move_file, "M");
    }

    #[test]
    fn test_explorer_keys_in_settings_serde() {
        let json = r#"{ "explorer_keys": { "rename": "R" } }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.explorer_keys.rename, "R");
        // Defaults preserved for the rest
        assert_eq!(s.explorer_keys.new_file, "a");
        assert_eq!(s.explorer_keys.delete, "D");
    }

    // ── parse_key_binding tests ──────────────────────────────────────────────

    #[test]
    fn test_parse_key_binding_ctrl() {
        assert_eq!(parse_key_binding("<C-b>"), Some((true, false, false, 'b')));
        assert_eq!(parse_key_binding("<C-p>"), Some((true, false, false, 'p')));
        assert_eq!(parse_key_binding("<C-g>"), Some((true, false, false, 'g')));
    }

    #[test]
    fn test_parse_key_binding_ctrl_shift() {
        assert_eq!(parse_key_binding("<C-S-e>"), Some((true, true, false, 'e')));
        assert_eq!(parse_key_binding("<C-S-f>"), Some((true, true, false, 'f')));
        // Uppercase key char is lowercased
        assert_eq!(parse_key_binding("<C-S-E>"), Some((true, true, false, 'e')));
    }

    #[test]
    fn test_parse_key_binding_alt() {
        assert_eq!(parse_key_binding("<A-x>"), Some((false, false, true, 'x')));
    }

    #[test]
    fn test_parse_key_binding_named_space() {
        assert_eq!(
            parse_key_binding("<C-Space>"),
            Some((true, false, false, ' '))
        );
        assert_eq!(
            parse_key_binding("<C-space>"),
            Some((true, false, false, ' '))
        );
    }

    #[test]
    fn test_parse_key_binding_invalid() {
        assert_eq!(parse_key_binding("ctrl+b"), None);
        assert_eq!(parse_key_binding("<C>"), None); // no key char
        assert_eq!(parse_key_binding("<X-b>"), None); // unknown modifier
        assert_eq!(parse_key_binding(""), None);
    }

    // ── PanelKeys tests ──────────────────────────────────────────────────────

    #[test]
    fn test_panel_keys_defaults() {
        let pk = PanelKeys::default();
        assert_eq!(pk.toggle_sidebar, "<C-b>");
        assert_eq!(pk.focus_explorer, "<A-e>");
        assert_eq!(pk.focus_search, "<A-f>");
        assert_eq!(pk.fuzzy_finder, "<C-p>");
        assert_eq!(pk.live_grep, "<C-g>");
        assert_eq!(pk.add_cursor, "<A-d>");
        assert_eq!(pk.select_all_matches, "<C-S-l>");
    }

    #[test]
    fn test_panel_keys_serde_partial() {
        let json = r#"{ "fuzzy_finder": "<C-A-p>" }"#;
        let pk: PanelKeys = serde_json::from_str(json).unwrap();
        assert_eq!(pk.fuzzy_finder, "<C-A-p>");
        // Unspecified keep defaults
        assert_eq!(pk.toggle_sidebar, "<C-b>");
        assert_eq!(pk.focus_explorer, "<A-e>");
    }

    #[test]
    fn test_panel_keys_in_settings_serde() {
        let json = r#"{ "panel_keys": { "live_grep": "<C-A-g>" } }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.panel_keys.live_grep, "<C-A-g>");
        assert_eq!(s.panel_keys.toggle_sidebar, "<C-b>");
    }
}
