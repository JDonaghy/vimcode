use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Bumped every time `Settings::save` writes to disk. Lets the TUI's
/// settings.json mtime watcher distinguish self-saves (silent reload) from
/// external edits (show "Settings reloaded" + apply).
static SAVE_REVISION: AtomicU64 = AtomicU64::new(0);

/// Read the current save revision. Watchers cache this and compare on
/// each poll; an unchanged revision means any mtime bump came from outside.
pub fn save_revision() -> u64 {
    SAVE_REVISION.load(Ordering::Acquire)
}

/// Test-only seam (#949 review) — see [`Settings::settings_file_path`]'s
/// doc for why this is a thread-local rather than a `$HOME` mutation.
#[cfg(test)]
thread_local! {
    static TEST_SETTINGS_PATH_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII installer for the [`Settings::settings_file_path`] test override —
/// mirrors `crate::test_cwd::CwdReadGuard`/`crate::test_paint::PaintGuard`'s
/// "acquire on construct, restore on `Drop`" shape, so a test that panics
/// mid-assertion still clears the thread-local instead of leaking the
/// override into whatever other `#[test]` fn Rust's runner schedules next
/// on the same pooled thread.
#[cfg(test)]
pub(crate) struct TestSettingsPathGuard {
    _private: (),
}

#[cfg(test)]
impl TestSettingsPathGuard {
    /// Point `settings_file_path()` at `path` for the calling test thread
    /// only until the returned guard drops.
    pub(crate) fn install(path: PathBuf) -> Self {
        TEST_SETTINGS_PATH_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(path));
        Self { _private: () }
    }
}

#[cfg(test)]
impl Drop for TestSettingsPathGuard {
    fn drop(&mut self) {
        TEST_SETTINGS_PATH_OVERRIDE.with(|cell| *cell.borrow_mut() = None);
    }
}

/// Which editing paradigm the editor uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EditorMode {
    /// Classic modal Vim key-bindings (default).
    #[default]
    Vim,
    /// VSCode-style always-insert editing (Shift+Arrow select, Ctrl-C/X/V/Z/Y/A).
    Vscode,
}

/// How right-click context menus are presented — platform look-and-feel,
/// **not** a keybinding paradigm (that's [`EditorMode`]; the two are
/// orthogonal and must not be folded together).
///
/// Mirrors VS Code's `window.menuStyle` (v1.101), which the release notes
/// describe as controlling "the menu style ... for context menus on
/// macOS" specifically — the macOS menu *bar* is always native and has no
/// such setting (see `native_menu`/`install_menu_bar`, vimcode#901); this
/// setting only ever changes anything on a backend that advertises
/// `quadraui::BackendCaps::native_menu` (macOS's `MacBackend` today). GTK
/// and TUI report `native_menu: false`, so every variant here resolves to
/// the same in-window `paint_context_menu_rung` path on those backends —
/// see `render::context_menu_should_be_native`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MenuStyle {
    /// Always use the backend's native context menu when it has one
    /// (`BackendCaps::native_menu`); fall back to the in-window rasteriser
    /// on a backend that doesn't (GTK, TUI never draw nothing).
    Native,
    /// Always paint the in-window `ContextMenuPanel`, even on a backend
    /// that could show a native one.
    Custom,
    /// Follow the window's title-bar style, matching VS Code. vimcode has
    /// no `titleBarStyle` setting yet, so until it does this resolves the
    /// same as `Native` (capability-gated) — revisit this arm once
    /// `titleBarStyle` exists.
    #[default]
    Inherit,
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

    /// Font size for UI chrome (menus, sidebars, dialogs, hover popup body).
    /// Independent of `font_size` (which controls editor text). Lower bound
    /// is enforced at use to avoid layout breakage.
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: u8,

    /// Show file explorer sidebar on startup
    #[serde(default = "default_explorer_visible")]
    pub explorer_visible_on_startup: bool,

    /// Enable incremental search (search as you type)
    #[serde(default = "default_incremental_search")]
    pub incremental_search: bool,

    /// Auto-indent new lines to match current line's leading whitespace
    #[serde(default = "default_auto_indent")]
    pub auto_indent: bool,

    /// C-ish auto-indent on newline, layered on top of `'autoindent'`:
    /// indent one `'shiftwidth'` further after a line ending in `{`, dedent
    /// a line whose first non-blank character is `}`, and put a `#`
    /// preprocessor line at column 0 unconditionally. Corresponds to Vim's
    /// `'smartindent'` / `'si'`. Superseded by `'cindent'` when both are set
    /// (`:h 'cindent'`: "'cindent' ... overrules 'smartindent'"). Default
    /// off, matching Vim. #1207.
    #[serde(default)]
    pub smartindent: bool,

    /// Stricter C-aware indenting; when set, takes precedence over
    /// `'smartindent'` (both may be on at once — `'cindent'` wins). This
    /// implementation is a simplified subset: indent after `{`, dedent a
    /// line whose first non-blank character is `}`, `#` to column 0,
    /// otherwise copy the previous non-blank line's indent. Corresponds to
    /// Vim's `'cindent'` / `'cin'`. Default off, matching Vim. #1207.
    #[serde(default)]
    pub cindent: bool,

    /// In Insert mode, briefly move the cursor to the matching opening
    /// bracket when a closing `)`, `]` or `}` is typed, then move it back.
    /// Corresponds to Vim's `'showmatch'` / `'sm'`. `'matchtime'` (how long
    /// the cursor stays on the match) is a value option and out of scope —
    /// see [`crate::core::engine::Engine::showmatch_flash`]. Default off,
    /// matching Vim. #1207.
    #[serde(default)]
    pub showmatch: bool,

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

    /// Opt-in freshness (#523): periodically run the Board panel's
    /// provider-declared `tick_command` (`BoardProviderConfig`,
    /// `crate::core::extensions`) so a daemon-less provider's pipeline
    /// doesn't stall just because vimcode is the only client with the
    /// board open. No vim precedent, so no `:set` abbreviation — toggle
    /// via the Settings sidebar or `:set board_tick_enabled=true`.
    /// **Default off** — a passive viewer must not silently dispatch
    /// metered work.
    #[serde(default)]
    pub board_tick_enabled: bool,

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

    /// How right-click context menus are presented (native OS popup vs.
    /// the in-window rasteriser). See [`MenuStyle`] — orthogonal to
    /// `editor_mode`, only observable on a backend with
    /// `BackendCaps::native_menu` (macOS today).
    #[serde(default)]
    pub menu_style: MenuStyle,

    /// Single character used as the leader key prefix in normal mode.
    /// Default is Space (' '). Override in settings.json: { "leader": "\\" }
    #[serde(default = "default_leader")]
    pub leader: char,

    /// Wrap long lines at the viewport width instead of scrolling horizontally.
    /// When true, lines longer than the window width are wrapped to the next
    /// visual row. Corresponds to Vim's `:set wrap` / `:set nowrap`.
    #[serde(default)]
    pub wrap: bool,

    /// When `'wrap'` is also on, break a soft-wrapped line at a word
    /// boundary (whitespace) at or before the wrap column instead of
    /// splitting mid-word. Purely a display-time choice of wrap point —
    /// never touches what's stored in the buffer. No-op when `'wrap'` is
    /// off. Corresponds to Vim's `'linebreak'` / `'lbr'`. Default off,
    /// matching Vim. #1207.
    #[serde(default)]
    pub linebreak: bool,

    /// When true, highlights misspelled words with underlines.
    /// Corresponds to Vim's `:set spell` / `:set nospell`.
    #[serde(default)]
    pub spell: bool,

    /// Spell-check language code (Hunspell dictionary name).
    #[serde(default = "default_spelllang")]
    pub spelllang: String,

    /// Whether the Lua plugin system is enabled (default true).
    #[serde(default = "default_plugins_enabled")]
    pub plugins_enabled: bool,

    /// Names of plugins that have been explicitly disabled via `:Plugin disable`.
    #[serde(default)]
    pub disabled_plugins: Vec<String>,

    /// User-defined key mappings, persisted in vimcode's internal storage
    /// format: `"mode[!] keys rhs"`.
    /// Mode: `n v x o i c s` (vim's `:map-modes` letters); a trailing `!`
    /// on the mode marks the entry `noremap` (no recursive expansion).
    /// Keys (lhs): single char (`x`), modifier (`<C-/>`, `<A-c>`), or
    /// sequence (`gcc`, `gc`), in vim key notation.
    /// Rhs: either an ex command prefixed with `:` (`":Commentary"`), or a
    /// raw key sequence fed back through the normal key path (`"<Esc>"`,
    /// `"<C-w>h"`) — vim's key-to-keys remapping (#1151).
    ///
    /// This array is populated by the `:map` family of ex commands
    /// (`:nnoremap`, `:imap`, `:vnoremap`, …), which parse vim's own
    /// `:{cmd} {lhs} {rhs}` syntax and translate it into this storage
    /// format — the format itself, and pre-#1151 entries written in it
    /// (`"n keys :command"`, always non-`noremap`, always an ex-command
    /// rhs), keep parsing and working unmigrated.
    /// Example: `["i! jk <Esc>", "n <C-/> :Commentary"]`
    #[serde(default)]
    pub keymaps: Vec<String>,

    /// User-defined Vim-style abbreviations (`:h abbreviations`), persisted
    /// alongside `keymaps`. Each entry is `"mode lhs rhs"`: mode is `i`
    /// (Insert-only, `:iabbrev`), `c` (Command-line-only, `:cabbrev`), or `a`
    /// (both, `:abbreviate`/`:noreabbrev`). `rhs` may itself contain spaces.
    /// Example: `["i teh the", "a @@ me@example.com"]`
    #[serde(default)]
    pub abbreviations: Vec<String>,

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

    /// Which regex metacharacters need backslash-escaping to be special vs.
    /// literal, in search patterns / `:s` / `*`/`#`. Corresponds to Vim's
    /// `'magic'` (`:h 'magic'`). Default **on**, matching Vim: `.`, `*`,
    /// `[`, `~`, `^`, `$` are special unescaped ([`crate::core::vim_regex::Magic::Magic`]).
    /// With `nomagic`, only `^`/`$` stay special unescaped — `.`, `*`, `[`,
    /// `~` become literal unless backslash-escaped, at which point they
    /// regain their special meaning ([`crate::core::vim_regex::Magic::NoMagic`]).
    /// A pattern's own inline `\v`/`\V`/`\m`/`\M` override always wins over
    /// this setting, exactly as it wins over a literal `:h /magic` line in
    /// real Vim. #1207.
    #[serde(default = "default_true")]
    pub magic: bool,

    /// Number of lines to keep visible above/below the cursor (default 0).
    #[serde(default)]
    pub scrolloff: usize,

    /// When true, commands that move the cursor to a different line
    /// (`<C-d>`, `<C-u>`, `<C-b>`, `<C-f>`, `G`, `gg`, `H`, `M`, `L`) park the
    /// cursor on the first non-blank column of the destination line instead
    /// of keeping the current column. Corresponds to Vim's `'startofline'` /
    /// `'sol'`. Default **false**, matching Neovim (real Vim defaults this
    /// **on** — see `:h 'startofline'`); vimcode's existing hardcoded
    /// column-preserving behavior for these commands already matched
    /// Neovim's default before this option existed, so flipping the default
    /// would silently change behavior for every user who never touches this
    /// setting.
    #[serde(default)]
    pub startofline: bool,

    /// When true, `J` (join) inserts two spaces instead of one after a line
    /// ending in `.`, `!` or `?`. Corresponds to Vim's `'joinspaces'` /
    /// `'js'`. Default **false**, matching Neovim (real Vim defaults this
    /// **on** — see `:h 'joinspaces'`). `gJ` is unaffected regardless of this
    /// setting: it never inserts a space.
    #[serde(default)]
    pub joinspaces: bool,

    /// When true, `<Tab>` at or before the first non-blank column of a line
    /// advances by `'shiftwidth'` (rounded to its next stop) instead of
    /// `'tabstop'`, and `<BS>` over leading whitespace deletes a whole
    /// `'shiftwidth'` worth of blanks instead of one character. Corresponds
    /// to Vim's `'smarttab'` / `'sta'`. Default **true**, matching Neovim
    /// (real Vim defaults this **off** — see `:h 'smarttab'`); vimcode's
    /// existing hardcoded Insert-mode Tab/BS behavior already matched
    /// Neovim's default before this option existed, so flipping the default
    /// would silently change behavior for every user who never touches this
    /// setting.
    #[serde(default = "default_smarttab")]
    pub smarttab: bool,

    /// Which numeral formats `<C-a>`/`<C-x>` (and Visual-mode `g<C-a>`)
    /// recognize besides plain decimal: any of `"bin"`, `"octal"`, `"hex"`,
    /// `"alpha"`. Corresponds to Vim's `'nrformats'` / `'nf'`. Default
    /// `["bin", "hex"]`, matching Neovim (real Vim's default additionally
    /// includes `"octal"` — see `:h 'nrformats'`).
    #[serde(default = "default_nrformats")]
    pub nrformats: Vec<String>,

    /// Characters `w`/`b`/`e`/`ge`, `*`/`#`/`g*`/`g#`, the `iw`/`aw` text
    /// objects, and the `\k`/`\K` regex classes treat as part of a "word".
    /// Corresponds to Vim's `'iskeyword'` / `'isk'`. Comma-separated list of
    /// single characters, `c1-c2` character ranges, decimal character codes,
    /// decimal code ranges, or `@` (every Unicode alphabetic character —
    /// vim's "`@` means alphabetic for the current encoding", and vimcode is
    /// always UTF-8); a leading `^` on an item excludes it instead of
    /// including it. Default `"@,48-57,_,192-255"`, matching Neovim's UTF-8
    /// default (`:h 'iskeyword'`) — every Unicode letter, digit, underscore,
    /// and the Latin-1 supplement block. `w`/`b`/`e`/etc. are ASCII-only
    /// before this option is consulted (#1191); this also fixes that,
    /// because the default already includes `@`.
    #[serde(default = "default_iskeyword")]
    pub iskeyword: String,

    /// How folds are found: `"manual"` (only `zf`-created folds — nothing is
    /// closeable until the user explicitly folds a range), `"indent"`
    /// (folds are derived from indentation and recomputed on demand), or
    /// `"marker"` (folds are derived from the literal `'foldmarker'` pair,
    /// #1159). Corresponds to Vim's `'foldmethod'` / `'fdm'`. Default
    /// `"manual"`, matching Vim (`:h 'foldmethod'`) — a fresh buffer has no
    /// folds at all until one is created. `"syntax"`/`"expr"`/`"diff"` are
    /// not implemented (#1159 scoped them out — see the issue).
    #[serde(default = "default_foldmethod")]
    pub foldmethod: String,

    /// When `'foldmethod'` is `"indent"` or `"marker"`, folds nested deeper
    /// than this level start closed; folds at or above it start open.
    /// Corresponds to Vim's `'foldlevel'` / `'fdl'`. Default `0`, matching
    /// Vim: every computed fold starts closed until raised (`:h
    /// 'foldlevel'`).
    #[serde(default)]
    pub foldlevel: usize,

    /// The open/close marker pair used when `'foldmethod'` is `"marker"`: a
    /// literal-text scan for these two strings, not a regex (`:h
    /// 'foldmarker'`). Format is `"{open},{close}"`; default `"{{{,}}}"`,
    /// matching Vim. A following digit on a marker in the text (Vim's
    /// explicit-fold-level refinement, e.g. `{{{2`) is not parsed specially
    /// here — the marker is still found as a literal-prefix match, but the
    /// digit doesn't set an explicit level (#1159 scoped that out: marker
    /// folding's core value is "a scan for a literal pair").
    #[serde(default = "default_foldmarker")]
    pub foldmarker: String,

    /// Maximum fold nesting depth for `'foldmethod'` `"indent"` (Vim also
    /// documents `"syntax"`, which vimcode doesn't implement — `:h
    /// 'foldnestmax'`). Deeper levels are absorbed into their `foldnestmax`
    /// ancestor instead of becoming their own closeable fold. Does **not**
    /// apply to `"marker"` folds, matching Vim (marker nesting is either the
    /// literal pair depth or an explicit numbered level, neither of which
    /// `'foldnestmax'` caps — verified against `nvim --headless`). Default
    /// `20`, matching Vim.
    #[serde(default = "default_foldnestmax")]
    pub foldnestmax: usize,

    /// Whether `/` and `?` search wrap around the end/start of the buffer
    /// when no more matches are found in the current direction. Corresponds
    /// to Vim's `'wrapscan'` / `'ws'`. Default **true**, matching Vim
    /// (`:h 'wrapscan'`).
    #[serde(default = "default_true")]
    pub wrapscan: bool,

    /// When true, `>>`/`<<` (and their operator/count forms) round the
    /// resulting indent to a multiple of `'shiftwidth'` instead of adding or
    /// removing exactly one `'shiftwidth'`. Corresponds to Vim's
    /// `'shiftround'` / `'sr'`. Default **false**, matching Vim (`:h
    /// 'shiftround'`).
    #[serde(default)]
    pub shiftround: bool,

    /// When true, inverts the meaning of the `:substitute` command's `g`
    /// flag: every match on a line is replaced by default, and a `g` flag
    /// toggles that off (replace only the first match per line). Corresponds
    /// to Vim's `'gdefault'` / `'gd'`. Default **false**, matching Vim (`:h
    /// 'gdefault'`).
    #[serde(default)]
    pub gdefault: bool,

    /// Number of columns a `<Tab>`/`<BS>` "feels like" in Insert mode,
    /// independent of `'tabstop'`. `0` (the default) disables this — Tab/BS
    /// use `'tabstop'` as usual. A negative value is a documented Vim idiom
    /// meaning "use `'shiftwidth'` instead" (`:h 'softtabstop'`). Only takes
    /// effect when `'expandtab'` is on; mixed tab/space soft-tabs
    /// (`noexpandtab` + `softtabstop`) are not modeled. Corresponds to Vim's
    /// `'softtabstop'` / `'sts'`.
    #[serde(default)]
    pub softtabstop: i32,

    /// Where the cursor may go past the end of a line, in Normal/Visual
    /// mode. Corresponds to Vim's `'virtualedit'` / `'ve'`. Only the
    /// documented "one column past the last character" effect of `"all"` /
    /// `"onemore"` is implemented (applied to `l`/`<Right>`/`$`) — full
    /// virtual-column placement anywhere in blank space (`"all"`'s complete
    /// behavior) is not modeled. Default `""` (off), matching Vim.
    #[serde(default)]
    pub virtualedit: String,

    /// Highlight the line the cursor is on (default true).
    #[serde(default = "default_cursorline")]
    pub cursorline: bool,

    /// Per-window status lines instead of a single global status bar (default true).
    #[serde(default = "default_window_status_line")]
    pub window_status_line: bool,

    /// When per-window status lines are enabled and the terminal panel is open,
    /// show the active window's status line as a dedicated row above the terminal
    /// instead of inside each window (default true).
    #[serde(default = "default_status_line_above_terminal")]
    pub status_line_above_terminal: bool,

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

    /// URLs of extension registry JSON files (fetched in order; later entries override earlier
    /// on name collision). Default: the official VimCode GitHub registry.
    #[serde(default = "default_extension_registries")]
    pub extension_registries: Vec<String>,

    /// Legacy field — migrated into `extension_registries` on load.
    #[serde(default, skip_serializing)]
    extension_registry_url: String,

    /// Name of the active colour scheme. Built-in options: "onedark" (default),
    /// "gruvbox-dark", "tokyo-night", "solarized-dark", "vscode-dark", "vscode-light".
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

    /// Attach the active buffer's path as a `resource_link` content block
    /// on every ACP `session/prompt` (#1449) — a baseline ACP v1 content
    /// kind every agent must accept, no `promptCapabilities` check needed.
    /// Skipped for an unnamed/scratch buffer (nothing on disk to link) and
    /// only relevant to the ACP transport (`ai_send_message_via_acp`); the
    /// direct-provider `curl` transport has no content-block concept.
    /// Default: true — the whole point of #1449 is that the agent already
    /// knows the workspace and can read files itself (#954), it just
    /// doesn't know *which* file the user is looking at without this.
    #[serde(default = "default_true")]
    pub ai_attach_current_buffer: bool,

    /// ACP (Agent Client Protocol) agent command line, e.g.
    /// `"claude-code-acp"` — parsed into argv via
    /// `crate::core::acp::parse_agent_command`. The agent is spawned with
    /// the workspace root (falling back to the CWD vimcode was started in)
    /// as its `session/new` `cwd` (#952, ACP-1).
    ///
    /// Empty (the default) means "no live agent configured": the AI panel
    /// falls back to `ai_provider`/`ai_api_key`'s direct-provider `curl`
    /// transport (`crate::core::ai`), kept as a no-agent-binary-required
    /// escape hatch per #952's "Decide in this slice" — through ACP-7, at
    /// which point a follow-up issue retires it.
    ///
    /// Superseded (never read) once `acp_agents` below is non-empty — see
    /// that field's doc for the multi-agent registry this single-string
    /// setting predates.
    #[serde(default)]
    pub acp_agent_command: String,

    /// The multi-agent registry (#958, ACP-7): zero or more named ACP
    /// agent profiles, selectable at runtime with `:AiAgent <name>`
    /// without restarting vimcode. Each profile is spawned through the
    /// exact same `AcpClient::spawn_with_env` the single-agent
    /// `acp_agent_command` path always used — bringing up a *second* agent
    /// is meant to be entirely a config fact (a second entry in this list
    /// with a different `command`/`env`), never new Rust, per the issue's
    /// own acceptance bar.
    ///
    /// Empty (the default) keeps the pre-#958 behaviour exactly:
    /// `acp_agent_command` alone decides the (single) live agent. Once
    /// non-empty, `acp_agent_command` is ignored — `acp_active_agent`
    /// picks which entry here is live instead.
    ///
    /// Example `settings.json` fragment registering two native (no
    /// adapter) ACP agents side by side:
    /// ```json
    /// "acp_agents": [
    ///   { "name": "claude", "command": "claude-code-acp" },
    ///   { "name": "gemini", "command": "gemini --acp" }
    /// ],
    /// "acp_active_agent": "claude"
    /// ```
    #[serde(default)]
    pub acp_agents: Vec<crate::core::acp::AcpAgentProfile>,

    /// Name of the currently active entry in `acp_agents`, matched
    /// case-insensitively. Empty, or naming a profile no longer present,
    /// falls back to `acp_agents[0]`. Changed only by `:AiAgent <name>`
    /// (`Engine::acp_switch_agent`) — never optimistically elsewhere —
    /// which also ends whatever session is currently live (a different
    /// agent process shares no context with the old one, so continuing to
    /// show its transcript next to a new agent's replies would be
    /// actively misleading; same reasoning `:AiClear` already documents).
    #[serde(default)]
    pub acp_active_agent: String,

    // ── Explorer ──────────────────────────────────────────────────────────────
    /// Show hidden files (dotfiles) in the file explorer (default: false).
    #[serde(default)]
    pub show_hidden_files: bool,
    /// Sort explorer entries case-insensitively (default: true).
    #[serde(default = "default_true")]
    pub explorer_sort_case_insensitive: bool,

    // ── Swap files ────────────────────────────────────────────────────────────
    /// Enable swap file crash recovery (default: true).
    #[serde(default = "default_swap_file")]
    pub swap_file: bool,

    /// Milliseconds between swap file writes for dirty buffers (default: 4000).
    #[serde(default = "default_updatetime")]
    pub updatetime: u32,

    // ── Undo persistence (#1156) ─────────────────────────────────────────────
    /// Maximum number of live undo-tree states kept per buffer, across every
    /// branch. Corresponds to Vim's `'undolevels'` / `'ul'`. Once exceeded,
    /// the globally-oldest branch tip not on the buffer's active path is
    /// pruned first (see `buffer_manager::UndoTree::enforce_undolevels`).
    /// Default `1000`, matching Vim/Neovim.
    #[serde(default = "default_undolevels")]
    pub undolevels: usize,

    /// Persist each buffer's undo tree to a file under `'undodir'` on save,
    /// and reload it the next time that file is opened — undo history then
    /// survives quitting and reopening. Corresponds to Vim's `'undofile'`.
    /// Default off, matching Vim/Neovim.
    #[serde(default)]
    pub undofile: bool,

    /// Directory undofiles are written to when `'undofile'` is on. Empty
    /// string (the default) means `~/.config/vimcode/undo/` (see
    /// `undofile::default_undo_dir`) — unlike Vim's `'undodir'`, this is a
    /// single directory rather than a priority list, since vimcode has no
    /// per-directory-unwritable fallback logic to drive a list with.
    #[serde(default)]
    pub undodir: String,

    /// Show breadcrumbs bar (file path + symbol hierarchy) below the tab bar.
    #[serde(default = "default_breadcrumbs")]
    pub breadcrumbs: bool,

    /// Hide the tab bar when an editor group has only one tab.
    /// Reclaims the row for editor content. Tab bar reappears when a second tab is opened.
    #[serde(default)]
    pub hide_single_tab: bool,

    /// Hide toolbar and sidebar panels at startup (TUI only).
    /// When true, panels appear on demand via Ctrl-W l and hide again when unfocused.
    #[serde(default)]
    pub autohide_panels: bool,

    /// Show indent guide lines at each indentation level.
    #[serde(default = "default_indent_guides")]
    pub indent_guides: bool,

    /// Show the code-overview minimap on the right edge of each editor pane.
    #[serde(default = "default_minimap")]
    pub minimap: bool,

    /// Highlight matching brackets when cursor is on one.
    #[serde(default = "default_match_brackets")]
    pub match_brackets: bool,

    /// Auto-close brackets and quotes in Insert mode.
    ///
    /// Mode-derived (see `EditorMode`): `None` means "inherit from
    /// `editor_mode`" and is resolved live by the [`Settings::auto_pairs`]
    /// accessor method — Vim mode is strict-off, Vscode mode is on. `Some(_)`
    /// is an explicit user override that always wins, including across a
    /// later `:set mode=...` switch. Never read this field directly; call
    /// the accessor method (`self.settings.auto_pairs()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_pairs: Option<bool>,

    /// Mouse dwell delay (ms) before auto-showing hover popups. 0 = disabled.
    #[serde(default = "default_hover_delay")]
    pub hover_delay: u32,

    /// Use Nerd Font icons in the UI (activity bar, file explorer, panels).
    ///
    /// Backend-derived (issue #999): `None` means "inherit from the running
    /// backend" and is resolved live by the [`Settings::use_nerd_fonts`]
    /// accessor method — GTK and macOS bundle Symbols Nerd Font 3.5.1 and
    /// register it in-process at startup (`render::register_nerd_font_
    /// fallback`, #1130), so the glyphs are guaranteed
    /// available regardless of what the user has installed, on every OS —
    /// those two backends therefore inherit `true` unconditionally. Win-GUI
    /// shares the same bundled font in principle but has two open,
    /// unverified bugs (vimcode#178, vimcode#161) suggesting its font
    /// resolution path may not actually work yet, and there is no Windows
    /// host in this project's fleet to check — so it keeps the conservative
    /// guess until those are confirmed fixed. The TUI renders through the
    /// user's terminal emulator, which uses its own configured font; there
    /// is no reliable way to detect that font's glyph coverage from inside
    /// the terminal (a CSI-6n width probe measures advance, not whether a
    /// real glyph painted — see the issue), so the TUI also keeps the
    /// previous conservative `target_os`-based guess and offers
    /// `:CheckNerdFonts` for the user to check by eye instead. `Some(_)` is
    /// an explicit user override that always wins, including across a
    /// later backend change, and persists across restarts. Never read this
    /// field directly; call the accessor method
    /// (`self.settings.use_nerd_fonts()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_nerd_fonts: Option<bool>,

    /// What Ctrl+F does: "find" opens the find/replace overlay, "page_down"
    /// is traditional Vim Ctrl+F page-down behavior.
    ///
    /// Mode-derived (see `EditorMode`): `None` means "inherit from
    /// `editor_mode`" and is resolved live by the
    /// [`Settings::ctrl_f_action`] accessor method — Vim mode is
    /// `page_down`, Vscode mode is `find`. `Some(_)` is an explicit user
    /// override that always wins, including across a later `:set
    /// mode=...` switch. Never read this field directly; call the
    /// accessor method (`self.settings.ctrl_f_action()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctrl_f_action: Option<String>,

    /// Maximum buffer line count for tree-sitter syntax highlighting.
    /// Files with more lines than this render as plain text. Default 20_000
    /// matches VSCode's tokenization cutoff and prevents the initial parse
    /// from blocking the main thread for seconds on generated files
    /// (Cargo.lock, logs). Set to a very large value to re-enable
    /// highlighting for huge files.
    #[serde(default = "default_syntax_max_lines")]
    pub syntax_max_lines: usize,

    /// Allow switching away from a modified buffer (`:edit`, `:bnext`,
    /// `:bprevious`, `:bfirst`, `:blast`, `:buffer`, `:enew`, ...) without
    /// saving or forcing with `!` — the abandoned buffer stays loaded,
    /// just not shown in any window. Without it those commands refuse
    /// with "No write since last change (add ! to override)" unless
    /// another window still shows the buffer (`:h 'hidden'`, `:h E37`).
    ///
    /// Default **on** — historical Vim defaults this off, but Neovim (this
    /// repo's oracle, per `tests/nvim_conformance.rs`) defaults it on;
    /// confirmed by hand with `nvim --headless -u NONE -c 'set hidden?'`.
    /// #1190.
    #[serde(default = "default_true")]
    pub hidden: bool,

    /// Show a partially-typed Normal-mode command (count/register/operator
    /// prefix, e.g. `"a2d`) in the last line while it's being typed.
    /// Corresponds to Vim's `'showcmd'` / `'sc'`. Default on, matching
    /// Neovim. #1190.
    #[serde(default = "default_true")]
    pub showcmd: bool,

    /// Show the cursor's line/column (and file percentage) in the status
    /// line. Corresponds to Vim's `'ruler'` / `'ru'`. Default on, matching
    /// Neovim. #1190.
    #[serde(default = "default_true")]
    pub ruler: bool,

    /// Render unprintable characters as glyphs instead of their normal
    /// whitespace effect: a tab displays as literal `^I` instead of
    /// expanding to `'tabstop'` width, and the true end of each line gets
    /// a trailing `$`. Corresponds to Vim's `'list'`. This is the on/off
    /// switch only — the exact glyphs are hardcoded to Vim's own
    /// no-`'listchars'`-item fallback (`:h 'listchars'`) until `'listchars'`
    /// itself lands (sibling value-option tranche). Default off, matching
    /// Vim. #1190.
    #[serde(default)]
    pub list: bool,

    /// Which characters `'list'` mode uses for otherwise-invisible glyphs.
    /// Corresponds to Vim's `'listchars'` / `'lcs'`. Comma-separated
    /// `item:chars` pairs. Recognised-and-wired items: `eol`, `tab`
    /// (two or three characters), `trail`, `nbsp`, `space` (each a single
    /// character). Recognised-and-*validated*-but-not-painted:
    /// `extends`/`precedes` (`'wrap'`-off horizontal-scroll clipping isn't
    /// glyph-annotated here) and `multispace`/`lead`/`leadmultispace`/
    /// `leadtab`/`conceal` (real Vim items with no vimcode rendering path
    /// yet — accepted so a pasted vimrc line doesn't read as a typo, exactly
    /// like `UNIMPLEMENTED_VALUE_OPTIONS`, but scoped per-item rather than
    /// per-option since the *option* itself is otherwise fully implemented).
    /// Default `"tab:> ,trail:-,nbsp:+"`, matching Neovim (`:h 'listchars'`)
    /// — note this has no `eol` item, so `'list'` does **not** show a
    /// trailing `$` out of the box (#1190's hardcoded always-`$`/`^I`
    /// fallback only matched classic Vim's *empty*-`'listchars'` behavior,
    /// not Neovim's real default).
    #[serde(default = "default_listchars")]
    pub listchars: String,

    /// Comma-separated list of Vim's per-key wrap tokens governing which
    /// motions may cross a line boundary instead of stopping at column 0 /
    /// the last column: `b` (`<BS>`), `s` (`<Space>`), `h`, `l`, `<`, `>`
    /// (Left/Right arrows, Normal and Visual mode), `[`, `]` (Left/Right
    /// arrows, Insert/Replace mode), `~` (the `~` command — accepted but not
    /// wired; `~` never advances past end-of-line here regardless of this
    /// setting). Corresponds to Vim's `'whichwrap'` / `'ww'`. Default
    /// `"b,s"`, matching Neovim (`:h 'whichwrap'`).
    #[serde(default = "default_whichwrap")]
    pub whichwrap: String,

    /// Comma-separated list of what Insert-mode `<BS>` may delete across:
    /// `"indent"` (autoindent — accepted but not distinctly wired; the
    /// existing autoindent-aware BackSpace behavior doesn't yet gate on
    /// this token), `"eol"` (the start of a line, joining with the previous
    /// line), `"start"` (the position where the current Insert session
    /// began). `"nostop"` is accepted (real Vim item, an `"eol"` variant)
    /// but not distinctly wired. A legacy numeric value (`0`-`3`, pre-7.4
    /// Vim) is also accepted and expanded to the equivalent list on write.
    /// Corresponds to Vim's `'backspace'` / `'bs'`. Default
    /// `"indent,eol,start"`, matching Neovim (`:h 'backspace'`) — this also
    /// matches vimcode's own pre-existing (hardcoded, unconditional)
    /// BackSpace behavior, so the default changes nothing out of the box.
    #[serde(default = "default_backspace")]
    pub backspace: String,

    /// Milliseconds to wait, after typing a keystroke that's an ambiguous
    /// prefix of a key-to-keys mapping (#1151), for a further keystroke that
    /// resolves it before giving up and replaying the buffered keys as
    /// typed. `0` disables the wait — the buffered keys are replayed as soon
    /// as nothing else could still match, matching vimcode's pre-existing
    /// (untimed) behavior. Corresponds to Vim's `'timeoutlen'` / `'tm'`.
    /// Default `1000`, matching Neovim (`:h 'timeoutlen'`).
    #[serde(default = "default_timeoutlen")]
    pub timeoutlen: u32,

    /// Command-line completion behavior for repeated `<Tab>`. Corresponds
    /// to Vim's `'wildmode'` / `'wim'`. Validated against Vim's documented
    /// comma/colon grammar (`full`/`longest`/`list`/`longest:full`/etc, `:h
    /// 'wildmode'`) and — as of #1206 — drives real, observably different
    /// Tab-completion behavior per [`WildmodeStage`]/`wildmode_stage_at`:
    /// `longest` fills only the common prefix without selecting an item,
    /// `list`-only leaves the command line untouched (the item list is
    /// shown either way — vimcode's wildmenu is unconditionally on, see
    /// `'wildmenu'` in `set_bool_option`), and stages advance one per
    /// `<Tab>` press the way `:h 'wildmode'` describes. The bare default
    /// value `"full"` is the one exception: it keeps vimcode's
    /// pre-existing UX (common-prefix on the first press, full-match
    /// cycling from the second press on) rather than switching to Vim's
    /// literal "select the first full match immediately" `full` semantics,
    /// so the already-covered `tests/wildmenu.rs` suite keeps passing
    /// unchanged — see `Settings::wildmode_is_plain_full`'s doc comment.
    /// `noselect`/`lastused` parse but are not distinctly wired (see
    /// `WildmodeStage`). Default `"full"`, matching Neovim.
    #[serde(default = "default_wildmode")]
    pub wildmode: String,

    /// When the per-window status line is shown: `0` never, `1` only when
    /// there are 2+ windows, `2` always. `3` (one global status line instead
    /// of per-window) is accepted but not modeled — falls back to `2`'s
    /// behavior (`render::effective_window_status_line`), since vimcode's
    /// status line is architecturally per-window
    /// (`Settings::window_status_line`). Corresponds to Vim's `'laststatus'`
    /// / `'ls'`. Default `2`, matching Neovim (`:h 'laststatus'`).
    #[serde(default = "default_laststatus")]
    pub laststatus: u8,

    /// Horizontal counterpart to `'scrolloff'`: minimum number of screen
    /// columns to keep to the left/right of the cursor when `'wrap'` is
    /// off. Corresponds to Vim's `'sidescrolloff'` / `'siso'`. Default `0`,
    /// matching Neovim (`:h 'sidescrolloff'`).
    #[serde(default)]
    pub sidescrolloff: usize,

    /// Minimum number of lines to scroll when the cursor moves off the top
    /// or bottom of the window. Corresponds to Vim's `'scrolljump'` /
    /// `'sj'`. Default `1`, matching Neovim (`:h 'scrolljump'`) — the
    /// minimum needed to bring the cursor back into view, i.e. vimcode's
    /// pre-existing behavior. Vim's documented "negative value is a
    /// percentage of the window height" is not modeled.
    #[serde(default = "default_scrolljump")]
    pub scrolljump: usize,
}

/// Mode-derived default for `ctrl_f_action` — see the field doc comment on
/// [`Settings::ctrl_f_action`]. Vim mode pages down (traditional Vim
/// behavior); Vscode mode opens find/replace (today's IDE default).
fn default_ctrl_f_action(mode: EditorMode) -> String {
    match mode {
        EditorMode::Vim => "page_down".to_string(),
        EditorMode::Vscode => "find".to_string(),
    }
}

fn default_syntax_max_lines() -> usize {
    20_000
}

fn default_indent_guides() -> bool {
    true
}

fn default_minimap() -> bool {
    true
}

fn default_match_brackets() -> bool {
    true
}

/// Mode-derived default for `auto_pairs` — see the field doc comment on
/// [`Settings::auto_pairs`]. Vim mode is strict (no auto-pairing); Vscode
/// mode auto-closes brackets/quotes (today's IDE default).
fn default_auto_pairs(mode: EditorMode) -> bool {
    match mode {
        EditorMode::Vim => false,
        EditorMode::Vscode => true,
    }
}

fn default_hover_delay() -> u32 {
    300
}

/// Backend-derived default for `use_nerd_fonts` — see the field doc on
/// [`Settings::use_nerd_fonts`].
///
/// `gui` is `crate::icons::is_gui_backend()`, set once at startup by every
/// GUI entry point (`App::new`, `App::new_portable`,
/// `App::new_headless_with_backend`) right where they already call
/// `icons::set_nerd_fonts(...)`; it defaults to `false` (the conservative,
/// TUI assumption) so a caller that forgets to opt in gets today's
/// behavior rather than a false "glyphs available".
///
/// GUI is uniform across GTK/macOS/Win-GUI *except* on Windows itself:
/// `App`'s `impl quadraui::ShellApp` is the one shared implementation all
/// three GUI backends run (`src/app.rs`), so there is no per-backend hook
/// to special-case just Win-GUI without adding per-backend code — this
/// `cfg!(target_os = "windows")` check lives here in core, mirroring the
/// TUI branch below, rather than in `src/gtk/`/`src/tui_main/`, so it does
/// not run afoul of this repo's Platform-Neutrality Rule. The reason it
/// exists at all: vimcode#178 (diff toolbar arrows render as `?`) and
/// vimcode#161 (tree-sized icon font) are open evidence that Win-GUI's
/// DirectWrite fallback path may not fully resolve the bundled font yet,
/// and there is no Windows host anywhere in this project's fleet to verify
/// it either way. Per the issue: "if Win-GUI genuinely can't resolve it
/// yet, leave it off and say so" — so Win-GUI keeps the pre-#999
/// conservative guess until #178/#161 confirm the font path actually
/// works, while GTK/macOS (verified to bundle and resolve the font) get
/// the new `true` default on every OS they run on.
fn default_use_nerd_fonts(gui: bool) -> bool {
    if gui && cfg!(target_os = "windows") {
        // Win-GUI: see the doc comment above — #178/#161 are open,
        // unverified evidence that the bundled font may not resolve here,
        // so don't claim it's available until they're confirmed fixed.
        false
    } else if gui {
        // GTK/macOS bundle the font (`app_support::ICON_FONT_BYTES`) and
        // install/register it at startup — always available.
        true
    } else {
        // TUI: on Windows, terminal fonts (Consolas, Cascadia Mono) don't
        // include Nerd Font glyphs by default. Use ASCII fallback icons
        // instead. Users who install a Nerd Font can enable via
        // `:set nerdfonts` or `:CheckNerdFonts`. On Linux/macOS, TUI
        // terminals commonly have Nerd Font support.
        !cfg!(target_os = "windows")
    }
}

fn default_swap_file() -> bool {
    true
}

fn default_breadcrumbs() -> bool {
    true
}

fn default_updatetime() -> u32 {
    4000
}

fn default_undolevels() -> usize {
    1000
}

fn default_explorer_visible() -> bool {
    false // Default: hidden
}

fn default_incremental_search() -> bool {
    true // Default: enabled
}

fn default_true() -> bool {
    true
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

fn default_spelllang() -> String {
    "en_US".to_string()
}

fn default_plugins_enabled() -> bool {
    true
}

fn default_hlsearch() -> bool {
    true
}

fn default_window_status_line() -> bool {
    true
}

fn default_status_line_above_terminal() -> bool {
    true
}

fn default_cursorline() -> bool {
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

fn default_extension_registries() -> Vec<String> {
    vec![crate::core::registry::DEFAULT_REGISTRY_URL.to_string()]
}

fn default_smarttab() -> bool {
    true
}

fn default_nrformats() -> Vec<String> {
    vec!["bin".to_string(), "hex".to_string()]
}

fn default_foldmethod() -> String {
    "manual".to_string()
}

fn default_foldmarker() -> String {
    "{{{,}}}".to_string()
}

fn default_foldnestmax() -> usize {
    20
}

fn default_iskeyword() -> String {
    "@,48-57,_,192-255".to_string()
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

impl ExplorerAction {
    /// Resolve an explorer context-menu/keyboard-shortcut action id string
    /// to the `ExplorerAction` both backends dispatch through
    /// `Engine::dispatch_explorer_crud`.
    ///
    /// #823 item 6: GTK's `App::explorer_action` (`app.rs`) and TUI's
    /// `handle_explorer_context_action` (`tui_main/mod.rs`) map this same
    /// 5-string table, but they are no longer safe to fully collapse — they
    /// were re-verified against current `HEAD` per this issue's own
    /// instruction, not just the line numbers recorded when the issue was
    /// filed, and the two functions have drifted past what a mechanical
    /// merge could do without changing behavior:
    ///
    /// * TUI's "delete" arm calls `Engine::confirm_delete_file` directly
    ///   with the context menu's explicit target path; GTK's routes
    ///   through `dispatch_explorer_crud(Delete)`, which acts on
    ///   `explorer_tree`'s *selected row* instead — forcing GTK onto TUI's
    ///   explicit-path behavior (or vice versa) is a real behavior change,
    ///   not a refactor, and picks the wrong file if a context-menu click
    ///   and the tree's selection ever disagree.
    /// * TUI's match has no `"move_file"` arm at all — it silently no-ops
    ///   today. Wiring it up via this table would be a new capability, not
    ///   a dedup, and needs its own test/issue.
    ///
    /// What both sides still agree on byte-for-byte is the `new_file` /
    /// `new_folder` / `rename` subset, both feeding the identical
    /// `dispatch_explorer_crud` call — that's what this function shares.
    /// `delete` / `move_file` are included for GTK's benefit (its own
    /// `explorer_action` handles all five through one call to
    /// `dispatch_explorer_crud`) but TUI's caller never reaches this
    /// function for those two strings — its own arms handle `"delete"`
    /// first and it has no `"move_file"` arm to reach here at all.
    pub fn from_action_str(s: &str) -> Option<Self> {
        Some(match s {
            "new_file" => Self::NewFile,
            "new_folder" => Self::NewFolder,
            "rename" => Self::Rename,
            "delete" => Self::Delete,
            "move_file" => Self::MoveFile,
            _ => return None,
        })
    }
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
    // <A-e> moved off in #318 — clashed with menu bar Alt+E (Edit menu).
    "<C-S-e>".to_string()
}
fn pk_focus_search() -> String {
    // <A-f> moved off in #318 — clashed with menu bar Alt+F (File menu).
    "<C-S-f>".to_string()
}
fn pk_fuzzy_finder() -> String {
    "<C-p>".to_string()
}
fn pk_live_grep() -> String {
    // Moved off <C-S-f> in #318 to free that combo for focus_search.
    "<C-S-g>".to_string()
}
fn pk_command_palette() -> String {
    "<C-S-p>".to_string()
}
fn pk_open_terminal() -> String {
    "<C-t>".to_string()
}
fn pk_toggle_terminal_maximize() -> String {
    "<C-S-t>".to_string()
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
    /// Focus explorer (or return to editor if already focused). Default: `<C-S-e>`
    #[serde(default = "pk_focus_explorer")]
    pub focus_explorer: String,
    /// Open search panel in sidebar. Default: `<C-S-f>`
    #[serde(default = "pk_focus_search")]
    pub focus_search: String,
    /// Open fuzzy file finder. Default: `<C-p>`
    #[serde(default = "pk_fuzzy_finder")]
    pub fuzzy_finder: String,
    /// Open live grep modal. Default: `<C-S-g>`
    #[serde(default = "pk_live_grep")]
    pub live_grep: String,
    /// Open command palette. Default: `<C-S-p>`
    #[serde(default = "pk_command_palette")]
    pub command_palette: String,
    /// Toggle integrated terminal panel. Default: `<C-t>`
    #[serde(default = "pk_open_terminal")]
    pub open_terminal: String,
    /// Toggle terminal panel maximize (fill editor area). Default: `<C-S-t>`
    #[serde(default = "pk_toggle_terminal_maximize")]
    pub toggle_terminal_maximize: String,
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
    /// Navigate back in tab history. Default: `<C-A-Left>`
    #[serde(default = "pk_nav_back")]
    pub nav_back: String,
    /// Navigate forward in tab history. Default: `<C-A-Right>`
    #[serde(default = "pk_nav_forward")]
    pub nav_forward: String,
}

fn pk_nav_back() -> String {
    "<C-A-Left>".to_string()
}
fn pk_nav_forward() -> String {
    "<C-A-Right>".to_string()
}

impl Default for PanelKeys {
    fn default() -> Self {
        PanelKeys {
            toggle_sidebar: pk_toggle_sidebar(),
            focus_explorer: pk_focus_explorer(),
            focus_search: pk_focus_search(),
            fuzzy_finder: pk_fuzzy_finder(),
            live_grep: pk_live_grep(),
            command_palette: pk_command_palette(),
            open_terminal: pk_open_terminal(),
            toggle_terminal_maximize: pk_toggle_terminal_maximize(),
            add_cursor: pk_add_cursor(),
            select_all_matches: pk_select_all_matches(),
            split_editor_right: String::new(),
            split_editor_down: String::new(),
            nav_back: pk_nav_back(),
            nav_forward: pk_nav_forward(),
        }
    }
}

fn default_completion_trigger() -> String {
    "<C-Space>".to_string()
}

/// Mode-derived default for `completion_keys.accept` — see the field doc
/// comment on [`CompletionKeys::accept`]. Vim mode leaves `<Tab>` alone
/// (`<C-y>` accepts, matching Vim's native completion menu); Vscode mode
/// uses `Tab` (today's IDE default).
fn default_completion_accept(mode: EditorMode) -> String {
    match mode {
        EditorMode::Vim => "<C-y>".to_string(),
        EditorMode::Vscode => "Tab".to_string(),
    }
}

/// Key bindings for the auto-popup completion menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionKeys {
    /// Key to manually trigger completion popup. Default: `<C-Space>`
    #[serde(default = "default_completion_trigger")]
    pub trigger: String,
    /// Key to accept the highlighted completion item.
    ///
    /// Mode-derived (see `EditorMode`): `None` means "inherit from
    /// `editor_mode`" and is resolved live by the
    /// [`CompletionKeys::accept`] accessor method — Vim mode is `<C-y>`,
    /// Vscode mode is `Tab`. `Some(_)` is an explicit user override that
    /// always wins, including across a later `:set mode=...` switch. Never
    /// read this field directly; call the accessor method
    /// (`self.settings.completion_keys.accept(self.settings.editor_mode)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
}

impl CompletionKeys {
    /// Resolve the effective accept key: an explicit override if set,
    /// otherwise the mode-derived default for `mode`.
    pub fn accept(&self, mode: EditorMode) -> String {
        self.accept
            .clone()
            .unwrap_or_else(|| default_completion_accept(mode))
    }
}

impl Default for CompletionKeys {
    fn default() -> Self {
        Self {
            trigger: default_completion_trigger(),
            accept: None,
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

/// #1069 branched this on `target_os == "macos"` via `cfg!` ("Menlo" on
/// macOS, "Monospace" everywhere else) because `"Monospace"` is a fontconfig
/// *generic alias* — GTK/Pango on Linux resolves it, and Win-GUI's
/// DirectWrite has an equivalent, but at the time CoreText had no such
/// alias and `MacBackend::set_editor_font` -> `make_font_exact` rejected
/// it outright, leaving `current_font` at `None` and the editor painting
/// at quadraui's placeholder metrics (`current_char_width: 8.0pt`,
/// `current_line_height: 16.0pt`) forever on macOS.
///
/// #1129: removed once quadraui#1023 added [`quadraui::GenericFamily`]
/// and taught `MacBackend::set_editor_font` to resolve the Pango alias
/// `"Monospace"` straight to `system_monospace_font` (CoreText's
/// `kCTFontUserFixedPitchFontType`) instead of routing it through
/// `make_font_exact`'s installed-family lookup — so one shared value
/// now resolves a real face on every backend and the `cfg!` branch is
/// no longer needed (Platform-Neutrality Rule; matches the `UI_FONT_FAMILY`
/// fix in `src/app_support.rs`).
fn default_font_family() -> String {
    "Monospace".to_string()
}

fn default_font_size() -> i32 {
    14
}

fn default_ui_font_size() -> u8 {
    10
}

fn default_listchars() -> String {
    "tab:> ,trail:-,nbsp:+".to_string()
}

fn default_whichwrap() -> String {
    "b,s".to_string()
}

fn default_backspace() -> String {
    "indent,eol,start".to_string()
}

fn default_timeoutlen() -> u32 {
    1000
}

fn default_wildmode() -> String {
    "full".to_string()
}

fn default_laststatus() -> u8 {
    2
}

fn default_scrolljump() -> usize {
    1
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            line_numbers: LineNumberMode::None,
            font_family: default_font_family(),
            font_size: default_font_size(),
            ui_font_size: default_ui_font_size(),
            explorer_visible_on_startup: default_explorer_visible(),
            incremental_search: default_incremental_search(),
            auto_indent: default_auto_indent(),
            smartindent: false,
            cindent: false,
            showmatch: false,
            expand_tab: default_expand_tab(),
            tabstop: default_tabstop(),
            shift_width: default_shift_width(),
            lsp_enabled: default_lsp_enabled(),
            format_on_save: false,
            board_tick_enabled: false,
            lsp_servers: Vec::new(),
            language_map: std::collections::HashMap::new(),
            terminal_scrollback_lines: default_terminal_scrollback_lines(),
            explorer_keys: ExplorerKeys::default(),
            panel_keys: PanelKeys::default(),
            completion_keys: CompletionKeys::default(),
            editor_mode: EditorMode::Vim,
            menu_style: MenuStyle::Inherit,
            leader: default_leader(),
            wrap: false,
            linebreak: false,
            spell: false,
            spelllang: default_spelllang(),
            plugins_enabled: default_plugins_enabled(),
            disabled_plugins: Vec::new(),
            keymaps: Vec::new(),
            abbreviations: Vec::new(),
            hlsearch: default_hlsearch(),
            ignorecase: false,
            smartcase: false,
            magic: default_true(),
            scrolloff: 0,
            startofline: false,
            joinspaces: false,
            smarttab: default_smarttab(),
            nrformats: default_nrformats(),
            iskeyword: default_iskeyword(),
            foldmethod: default_foldmethod(),
            foldlevel: 0,
            foldmarker: default_foldmarker(),
            foldnestmax: default_foldnestmax(),
            wrapscan: default_true(),
            shiftround: false,
            gdefault: false,
            softtabstop: 0,
            virtualedit: String::new(),
            cursorline: default_cursorline(),
            window_status_line: default_window_status_line(),
            status_line_above_terminal: default_status_line_above_terminal(),
            autoread: default_autoread(),
            splitbelow: false,
            splitright: false,
            colorcolumn: String::new(),
            textwidth: 0,
            extension_registries: default_extension_registries(),
            extension_registry_url: String::new(),
            colorscheme: default_colorscheme(),
            ai_provider: default_ai_provider(),
            ai_api_key: String::new(),
            ai_model: String::new(),
            ai_base_url: String::new(),
            ai_completions: false,
            ai_attach_current_buffer: default_true(),
            acp_agent_command: String::new(),
            acp_agents: Vec::new(),
            acp_active_agent: String::new(),
            show_hidden_files: false,
            explorer_sort_case_insensitive: true,
            swap_file: default_swap_file(),
            updatetime: default_updatetime(),
            undolevels: default_undolevels(),
            undofile: false,
            undodir: String::new(),
            breadcrumbs: default_breadcrumbs(),
            hide_single_tab: false,
            autohide_panels: false,
            indent_guides: default_indent_guides(),
            minimap: default_minimap(),
            match_brackets: default_match_brackets(),
            auto_pairs: None, // mode-derived — see Settings::auto_pairs()
            hover_delay: default_hover_delay(),
            use_nerd_fonts: None, // backend-derived — see Settings::use_nerd_fonts()
            ctrl_f_action: None,  // mode-derived — see Settings::ctrl_f_action()
            syntax_max_lines: default_syntax_max_lines(),
            hidden: default_true(),
            showcmd: default_true(),
            ruler: default_true(),
            list: false,
            listchars: default_listchars(),
            whichwrap: default_whichwrap(),
            backspace: default_backspace(),
            timeoutlen: default_timeoutlen(),
            wildmode: default_wildmode(),
            laststatus: default_laststatus(),
            sidescrolloff: 0,
            scrolljump: default_scrolljump(),
        }
    }
}

/// Real vim **boolean** options `:set` recognises by name (so a vimrc line
/// naming one doesn't read as an unrecognised typo) but does not yet wire to
/// any behaviour. `(long_name, short_name)`. #1153 — extend this table (and
/// implement) as each is picked up; see the issue for the full missing-option
/// audit and rough priority order.
///
/// Empty as of #1207, which implemented the last five entries (`magic`,
/// `showmatch`, `linebreak`, `smartindent`, `cindent` — #1190's tranche
/// before it cleared `hidden`, `list`, `showcmd`, `ruler`). Kept as `&[]`
/// rather than removed, per #1207's own note: the "recognised but not
/// implemented" mechanism (#1153) is meant to be reused by the next Vim
/// option that lands here recognised-but-unwired.
const UNIMPLEMENTED_BOOL_OPTIONS: &[(&str, &str)] = &[];

/// Real vim **value** options `:set` recognises by name but does not yet
/// wire to any behaviour. See [`UNIMPLEMENTED_BOOL_OPTIONS`]'s doc — same
/// rationale, same table shape.
const UNIMPLEMENTED_VALUE_OPTIONS: &[(&str, &str)] = &[("clipboard", "cb")];

/// Shared "recognised, not implemented" message for both option tables
/// (#1153) — deliberately distinct wording from `"Unknown option: {opt}"` so
/// a user (or a vimrc author) can tell "this is a typo" from "this is a real
/// vim option vimcode hasn't wired up yet" at a glance.
fn not_implemented_message(opt: &str) -> String {
    format!("Option '{opt}' is recognised but not implemented yet")
}

// ── 'iskeyword' (#1191) ──────────────────────────────────────────────────
//
// Vim's char-list grammar, shared (per `:h 'isfname'`) by 'iskeyword',
// 'isident', 'isprint' and 'isfname': a comma-separated list of items, each
// either a single character, a `c1-c2` character range, a decimal character
// code, a decimal `n1-n2` code range, or `@` ("every alphabetic character
// for the current encoding" — vimcode is always UTF-8, so this means every
// Unicode alphabetic `char`). A leading `^` on an item excludes it from the
// set built so far instead of adding it. Later items win over earlier ones
// for the same character, exactly like Vim evaluates the list in order.

/// One `'iskeyword'`-list item's character-set half (include/exclude is
/// tracked separately in [`IskeywordEntry`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IskeywordItem {
    /// Bare `@` — every Unicode alphabetic character.
    AllAlpha,
    /// A single literal character.
    Char(char),
    /// A `c1-c2` character range (order-independent).
    Range(char, char),
    /// A single decimal character code.
    Code(u32),
    /// A decimal `n1-n2` code range (order-independent).
    CodeRange(u32, u32),
}

#[derive(Debug, Clone, Copy)]
struct IskeywordEntry {
    item: IskeywordItem,
    include: bool,
}

/// Parse one comma-separated token (after an optional leading `^`, already
/// stripped by the caller) into an [`IskeywordItem`].
fn parse_iskeyword_token(tok: &str) -> Result<IskeywordItem, String> {
    if tok == "@" {
        return Ok(IskeywordItem::AllAlpha);
    }
    // A range is `left-right` with a `-` that isn't the whole token (so a
    // lone "-" is still the literal dash character, and "@-@" is the
    // literal '@' via the char-range branch, not the bare-`@` branch above).
    if tok.len() > 1 {
        if let Some(dash) = tok.char_indices().skip(1).find(|&(_, c)| c == '-') {
            let (left, right) = (&tok[..dash.0], &tok[dash.0 + 1..]);
            if !left.is_empty() && !right.is_empty() {
                if let (Ok(n1), Ok(n2)) = (left.parse::<u32>(), right.parse::<u32>()) {
                    return Ok(IskeywordItem::CodeRange(n1, n2));
                }
                let (lchars, rchars): (Vec<char>, Vec<char>) =
                    (left.chars().collect(), right.chars().collect());
                if lchars.len() == 1 && rchars.len() == 1 {
                    return Ok(IskeywordItem::Range(lchars[0], rchars[0]));
                }
                return Err(format!("bad range '{tok}'"));
            }
        }
    }
    if let Ok(n) = tok.parse::<u32>() {
        return Ok(IskeywordItem::Code(n));
    }
    let chars: Vec<char> = tok.chars().collect();
    if chars.len() == 1 {
        return Ok(IskeywordItem::Char(chars[0]));
    }
    Err(format!("bad token '{tok}'"))
}

/// Parse a full `'iskeyword'` value into its ordered entry list. Empty
/// tokens (e.g. a trailing comma) are skipped, matching Vim.
fn parse_iskeyword(spec: &str) -> Result<Vec<IskeywordEntry>, String> {
    let mut entries = Vec::new();
    for raw in spec.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (include, tok) = match raw.strip_prefix('^') {
            Some(rest) if !rest.is_empty() => (false, rest),
            _ => (true, raw),
        };
        let item = parse_iskeyword_token(tok)?;
        entries.push(IskeywordEntry { item, include });
    }
    Ok(entries)
}

/// Does `c` belong to the character class described by `entries`? Order-
/// sensitive: later entries override earlier ones for the same character,
/// exactly like Vim evaluates the 'iskeyword' list.
fn iskeyword_matches(entries: &[IskeywordEntry], c: char) -> bool {
    let cp = c as u32;
    let mut result = false;
    for e in entries {
        let hit = match e.item {
            IskeywordItem::AllAlpha => c.is_alphabetic(),
            IskeywordItem::Char(ch) => c == ch,
            IskeywordItem::Range(a, b) => {
                let (lo, hi) = (a.min(b) as u32, a.max(b) as u32);
                cp >= lo && cp <= hi
            }
            IskeywordItem::Code(n) => cp == n,
            IskeywordItem::CodeRange(n1, n2) => {
                let (lo, hi) = (n1.min(n2), n1.max(n2));
                cp >= lo && cp <= hi
            }
        };
        if hit {
            result = e.include;
        }
    }
    result
}

/// Escape `c` so it's safe as a literal inside a `[...]` Rust-regex class
/// body (used by [`iskeyword_regex_class_body`]).
fn push_class_char(out: &mut String, c: char) {
    if matches!(c, '\\' | ']' | '^' | '-' | '&') {
        out.push('\\');
    }
    out.push(c);
}

/// Build the `[...]`-body fragment for `\k` (`'iskeyword'`-driven, `:h
/// /\k`) from a parsed 'iskeyword' spec.
///
/// Vim's real semantics are order-sensitive include/exclude
/// ([`iskeyword_matches`]), which a single regex character class can't
/// losslessly represent — a `-=`/`^=`-based *exclusion* is dropped here
/// (only additive items are represented). That's exact for the default
/// spec and every `+=`-only customization — the common case — and only
/// under-covers `\k` relative to real word motions for an explicit
/// exclusion. Falls back to the historical ASCII-only class if the spec is
/// somehow unparsable (defensive — `set_value_option` already validates on
/// write).
fn iskeyword_regex_class_body(entries: &[IskeywordEntry]) -> String {
    let mut body = String::new();
    for e in entries {
        if !e.include {
            continue;
        }
        match e.item {
            IskeywordItem::AllAlpha => body.push_str("\\p{Alphabetic}"),
            IskeywordItem::Char(c) => push_class_char(&mut body, c),
            IskeywordItem::Range(a, b) => {
                push_class_char(&mut body, a);
                body.push('-');
                push_class_char(&mut body, b);
            }
            IskeywordItem::Code(n) => {
                if let Some(c) = char::from_u32(n) {
                    push_class_char(&mut body, c);
                }
            }
            IskeywordItem::CodeRange(n1, n2) => {
                if let (Some(a), Some(b)) = (char::from_u32(n1), char::from_u32(n2)) {
                    push_class_char(&mut body, a);
                    body.push('-');
                    push_class_char(&mut body, b);
                }
            }
        }
    }
    if body.is_empty() {
        body.push_str("0-9A-Za-z_");
    }
    body
}

/// `+=`/`-=`/`^=` combine mode for either shape of value option `:set`
/// accepts an operator on: a comma-separated list-style option (`:h :set`,
/// "List of items" — `Append`/`Remove`/`Prepend` act on whole comma-
/// separated tokens) or a numeric option (`:h :set`, "For number options" —
/// `Append`/`Remove`/`Prepend` mean add/subtract/multiply). #1191 added this
/// for `'iskeyword'` alone; #1206 generalised it to every list- and number-
/// shaped option (`parse_set_option`'s combine loop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListOp {
    Append,
    Remove,
    Prepend,
}

/// Apply `op` with `value` (a comma-separated list of items) to `current`
/// (also comma-separated), returning the new combined string. Used for
/// `:set iskeyword+=X` / `-=X` / `^=X`, and (#1206) every other list-style
/// value option (`'whichwrap'`, `'backspace'`, `'wildmode'`, `'listchars'`).
fn combine_csv_list(current: &str, value: &str, op: ListOp) -> String {
    match op {
        ListOp::Append => {
            if current.is_empty() {
                value.to_string()
            } else {
                format!("{current},{value}")
            }
        }
        ListOp::Prepend => {
            if current.is_empty() {
                value.to_string()
            } else {
                format!("{value},{current}")
            }
        }
        ListOp::Remove => {
            let removed: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
            current
                .split(',')
                .filter(|tok| !removed.contains(&tok.trim()))
                .collect::<Vec<_>>()
                .join(",")
        }
    }
}

/// `:h 'whichwrap'`'s real per-key wrap-token characters. Anything else in a
/// `'whichwrap'` value is a malformed token.
const WHICHWRAP_TOKENS: &[char] = &['b', 's', 'h', 'l', '<', '>', '~', '[', ']'];

/// Validate a `'whichwrap'` value: comma-separated, each token one of
/// [`WHICHWRAP_TOKENS`].
fn parse_whichwrap(spec: &str) -> Result<(), String> {
    for tok in spec.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        let chars: Vec<char> = tok.chars().collect();
        if chars.len() != 1 || !WHICHWRAP_TOKENS.contains(&chars[0]) {
            return Err(format!("bad token '{tok}'"));
        }
    }
    Ok(())
}

/// `:h 'backspace'`'s real list-form tokens (`"nostop"` is a real Vim 8.2+
/// item — an `"eol"` variant that also disables `'start'`-style stopping
/// when crossing into the previous line — accepted here but not distinctly
/// wired; see the field doc comment).
const BACKSPACE_TOKENS: &[&str] = &["indent", "eol", "start", "nostop"];

/// Validate and normalise a `'backspace'` value. Accepts both the modern
/// comma-list form and Vim's legacy pre-7.4 numeric shorthand (`0`-`3`),
/// expanding the latter to its equivalent list form so every other call
/// site only ever has to deal with one shape (`:h 'backspace'`).
fn parse_backspace(spec: &str) -> Result<String, String> {
    match spec {
        "0" => return Ok(String::new()),
        "1" => return Ok("indent,eol".to_string()),
        "2" => return Ok("indent,eol,start".to_string()),
        "3" => return Ok("indent,eol,nostop".to_string()),
        _ => {}
    }
    for tok in spec.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        if !BACKSPACE_TOKENS.contains(&tok) {
            return Err(format!("bad token '{tok}'"));
        }
    }
    Ok(spec.to_string())
}

/// `:h 'wildmode'`'s real per-stage keywords, each comma-separated stage
/// optionally a colon-separated sequence of these. `"noselect"` and
/// `"lastused"` parse as valid (real Vim tokens, `:h 'wildmode'`) but are
/// not distinctly wired by [`WildmodeStage`] — see its doc comment.
const WILDMODE_TOKENS: &[&str] = &[
    "full", "longest", "list", "lastused", "noselect",
    "", // "" — an empty stage, e.g. leading `,`
];

/// Validate a `'wildmode'` value: comma-separated stages, each stage a
/// colon-separated sequence of [`WILDMODE_TOKENS`].
fn parse_wildmode(spec: &str) -> Result<(), String> {
    for stage in spec.split(',') {
        for tok in stage.split(':') {
            if !WILDMODE_TOKENS.contains(&tok) {
                return Err(format!("bad token '{tok}'"));
            }
        }
    }
    Ok(())
}

/// One comma-separated stage of `'wildmode'` — the behavior flags active on
/// a single `<Tab>` press (`:h 'wildmode'`, #1206). Multiple colon-joined
/// flags in the same stage combine (`"longest:full"` sets both); per the
/// docs, when `longest` and `full` are both set on the *same* stage,
/// `longest` wins and the stage does not cycle full matches — callers
/// implement that by checking `longest` before `full`.
///
/// `lastused` (sort buffer-name matches by recency) and `noselect` (show
/// the menu without preselecting the first item) parse as valid stage
/// tokens but have no vimcode-side hook to attach to: this codebase's
/// wildmenu list is unconditionally shown regardless of `'wildmode'` (see
/// `'wildmenu'`'s `set_bool_option` arm), so `noselect`'s distinction from
/// plain `full` has nothing to change, and there is no buffer-name
/// completion sort order to key off `lastused`. Both are accepted rather
/// than rejected (matching real Vim's grammar) but produce the same
/// behavior as the stage's other flags alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WildmodeStage {
    /// `""` — an empty stage: complete the first match once, then stop
    /// (never cycle or list on repeat presses of *this* stage).
    pub only_first: bool,
    pub full: bool,
    pub longest: bool,
    #[allow(dead_code)] // parsed for grammar completeness; see struct doc
    pub list: bool,
}

/// Parse `'wildmode'` into its ordered stages. `:h 'wildmode'`: "a comma
/// separated list of up to four parts, corresponding to the first, second,
/// third, and fourth presses of 'wildchar'" — Vim holds on the last
/// configured stage for every press beyond that, so callers should clamp
/// a press index to `stages.len() - 1` (see [`Settings::wildmode_stage_at`])
/// rather than treating a missing stage as "no more completion".
fn wildmode_stages(spec: &str) -> Vec<WildmodeStage> {
    spec.split(',')
        .map(|stage| {
            if stage.is_empty() {
                return WildmodeStage {
                    only_first: true,
                    ..Default::default()
                };
            }
            let mut s = WildmodeStage::default();
            for tok in stage.split(':') {
                match tok {
                    "full" => s.full = true,
                    "longest" => s.longest = true,
                    "list" => s.list = true,
                    _ => {} // lastused / noselect — not distinctly wired
                }
            }
            s
        })
        .collect()
}

/// `:h 'listchars'`'s real item keys. `Fixed(n)` items take exactly `n`
/// characters; `Tab` takes two or three; `Cyclic` (`multispace`,
/// `leadmultispace`) takes one or more.
enum ListcharsItemShape {
    Fixed(usize),
    Tab,
    Cyclic,
}

/// `(key, shape, is_wired)` — `is_wired` items are the ones
/// `render::apply_list_glyphs` actually paints; the rest are accepted (real
/// Vim items) but not rendered specially — see the `listchars` field doc.
const LISTCHARS_ITEMS: &[(&str, ListcharsItemShape, bool)] = &[
    ("eol", ListcharsItemShape::Fixed(1), true),
    ("tab", ListcharsItemShape::Tab, true),
    ("trail", ListcharsItemShape::Fixed(1), true),
    ("nbsp", ListcharsItemShape::Fixed(1), true),
    ("space", ListcharsItemShape::Fixed(1), true),
    ("extends", ListcharsItemShape::Fixed(1), false),
    ("precedes", ListcharsItemShape::Fixed(1), false),
    ("multispace", ListcharsItemShape::Cyclic, false),
    ("lead", ListcharsItemShape::Fixed(1), false),
    ("leadmultispace", ListcharsItemShape::Cyclic, false),
    ("leadtab", ListcharsItemShape::Tab, false),
    ("conceal", ListcharsItemShape::Fixed(1), false),
];

/// Validate a `'listchars'` value: comma-separated `item:chars` pairs, each
/// `item` one of [`LISTCHARS_ITEMS`] and `chars` matching that item's shape.
fn parse_listchars(spec: &str) -> Result<(), String> {
    // Deliberately does NOT trim each comma-split token (unlike every other
    // list option in this file): a trailing space can be a meaningful part
    // of an item's `chars` — Neovim's own default value is `"tab:> ,..."`,
    // where the tab glyph's second character *is* a space — so trimming it
    // away here would silently corrupt the parse.
    for tok in spec.split(',') {
        if tok.is_empty() {
            continue;
        }
        let Some((key, chars)) = tok.split_once(':') else {
            return Err(format!("bad item '{tok}' (expected 'item:chars')"));
        };
        let Some((_, shape, _)) = LISTCHARS_ITEMS.iter().find(|(k, ..)| *k == key) else {
            return Err(format!("unknown listchars item '{key}'"));
        };
        let n = chars.chars().count();
        let ok = match shape {
            ListcharsItemShape::Fixed(want) => n == *want,
            ListcharsItemShape::Tab => n == 2 || n == 3,
            ListcharsItemShape::Cyclic => n >= 1,
        };
        if !ok {
            return Err(format!("bad value '{chars}' for listchars item '{key}'"));
        }
    }
    Ok(())
}

/// Look up the single-character glyph configured for `item` in `listchars`
/// (`self.settings.listchars`), falling back to `default_char` if the item
/// isn't present. Used by `render::apply_list_glyphs` for `'list'`'s
/// `eol`/`trail`/`nbsp`/`space` glyphs.
///
/// No `.trim()` on each comma-split token — see [`parse_listchars`]'s doc
/// comment; a trailing space can be the configured glyph itself.
pub(crate) fn listchars_char(
    listchars: &str,
    item: &str,
    default_char: Option<char>,
) -> Option<char> {
    for tok in listchars.split(',') {
        if let Some(chars) = tok.strip_prefix(&format!("{item}:")) {
            return chars.chars().next();
        }
    }
    default_char
}

/// A parsed `'listchars'` `tab:xy` or `tab:xyz` item (`:h lcs-tab`). The two
/// forms fill a tabstop-width gap differently — see [`Self::render`] — so
/// this keeps them distinct rather than collapsing `xy` into `(x, y, y)`,
/// which would get the width-1 case wrong (the 2-char form always shows
/// `x` for a single-column gap; the 3-char form always shows `z`).
pub(crate) enum TabGlyph {
    /// `tab:xy` — `x` is always used first, then `y` fills the rest.
    Two(char, char),
    /// `tab:xyz` — `z` is always used last, `x` first, `y` fills the middle.
    Three(char, char, char),
}

impl TabGlyph {
    /// Render this glyph to fill a `width`-column gap (`width >= 1`).
    pub(crate) fn render(&self, width: usize) -> String {
        let width = width.max(1);
        match *self {
            TabGlyph::Two(x, y) => {
                let mut s = String::new();
                s.push(x);
                for _ in 1..width {
                    s.push(y);
                }
                s
            }
            TabGlyph::Three(x, y, z) => {
                if width == 1 {
                    return z.to_string();
                }
                let mut s = String::new();
                s.push(x);
                for _ in 0..width.saturating_sub(2) {
                    s.push(y);
                }
                s.push(z);
                s
            }
        }
    }
}

/// Look up the `'listchars'` `tab:xy[z]` item, if present. No `.trim()` on
/// each comma-split token — see [`parse_listchars`]'s doc comment.
pub(crate) fn listchars_tab(listchars: &str) -> Option<TabGlyph> {
    for tok in listchars.split(',') {
        if let Some(spec) = tok.strip_prefix("tab:") {
            let chars: Vec<char> = spec.chars().collect();
            return match chars.len() {
                2 => Some(TabGlyph::Two(chars[0], chars[1])),
                3 => Some(TabGlyph::Three(chars[0], chars[1], chars[2])),
                _ => None,
            };
        }
    }
    None
}

impl Settings {
    // ── Mode-derived contested defaults ─────────────────────────────────────
    //
    // These three settings (`ctrl_f_action`, `auto_pairs`,
    // `completion_keys.accept`) are stored as `Option<T>`: `None` means
    // "not explicitly set, inherit from `editor_mode`"; `Some(_)` is an
    // explicit user override. Resolution is intentionally *lazy* — done on
    // every read via these accessor methods rather than baked into the
    // field at load time — so that:
    //   1. An explicit override always wins, even after a later
    //      `:set mode=...` switch (there is no stale "filled" state to
    //      accidentally clobber).
    //   2. `:set mode=vim` / `:set mode=vscode` re-resolves every unset
    //      field immediately, with no extra step and no restart, because
    //      the next read naturally uses the new `self.editor_mode`.
    //   3. `Settings::save()` never persists a resolved value for a field
    //      the user never touched (`skip_serializing_if = "Option::is_none"`),
    //      so a fresh settings.json stays mode-reactive across restarts too.
    //
    // See `docs/PATTERNS.md` ("Mode-derived contested defaults") for the
    // full table and the recipe for adding a fourth one.

    /// Resolve the effective `ctrl_f_action`: an explicit override if set,
    /// otherwise the default for the current `editor_mode`.
    pub fn ctrl_f_action(&self) -> String {
        self.ctrl_f_action
            .clone()
            .unwrap_or_else(|| default_ctrl_f_action(self.editor_mode))
    }

    /// Resolve the effective `auto_pairs`: an explicit override if set,
    /// otherwise the default for the current `editor_mode`.
    pub fn auto_pairs(&self) -> bool {
        self.auto_pairs
            .unwrap_or_else(|| default_auto_pairs(self.editor_mode))
    }

    /// Resolve the effective `use_nerd_fonts`: an explicit override if set,
    /// otherwise the backend-derived default (issue #999) — see the field
    /// doc on [`Settings::use_nerd_fonts`]. Unlike `ctrl_f_action`/
    /// `auto_pairs`, which are derived from `self.editor_mode` (a stored,
    /// user-configurable field), the dimension here — GUI vs TUI — isn't a
    /// `Settings` field at all: it's a fact about which binary is running,
    /// recorded via `crate::icons::set_gui_backend`/`is_gui_backend` the
    /// same way `icons::set_nerd_fonts` already threads the resolved
    /// glyph-vs-fallback flag through this module (see that thread-local's
    /// doc for why thread-local, not process-global).
    pub fn use_nerd_fonts(&self) -> bool {
        self.use_nerd_fonts
            .unwrap_or_else(|| default_use_nerd_fonts(crate::icons::is_gui_backend()))
    }

    /// Does `'virtualedit'` include the "one column past the last character"
    /// effect (`"all"` or `"onemore"`)? The only subset of `'virtualedit'`
    /// vimcode implements — see the field doc comment on
    /// [`Settings::virtualedit`] (#1153).
    pub(crate) fn virtualedit_allows_onemore(&self) -> bool {
        self.virtualedit
            .split(',')
            .any(|t| t == "all" || t == "onemore")
    }

    /// The `'wildmode'` flags active on the `press`'th `<Tab>` press of the
    /// current command-line completion round (0-indexed). Clamps to the
    /// last configured stage once `press` runs past the configured list,
    /// matching Vim holding on the last stage for every press beyond it
    /// (#1206). See [`WildmodeStage`] for which flags are distinctly wired.
    pub(crate) fn wildmode_stage_at(&self, press: usize) -> WildmodeStage {
        let stages = wildmode_stages(&self.wildmode);
        match stages.len() {
            0 => WildmodeStage {
                full: true,
                ..Default::default()
            },
            n => stages[press.min(n - 1)],
        }
    }

    /// True when `'wildmode'` is (equivalent to) the bare default
    /// `"full"` — a single stage whose only flag is `full`. vimcode's
    /// pre-existing Tab-completion UX (common-prefix on the first press,
    /// then full-match cycling from the second press on) predates this
    /// option being wired and is kept exactly as-is for this one
    /// configuration, rather than switched to Vim's literal "select the
    /// first full match immediately" reading of `'full'` — see the
    /// `wildmode` field doc comment for why, and `handle_command_key`'s
    /// `"Tab"` arm for where this carve-out is consulted. Every other
    /// configuration (`longest`, `list`, multiple stages, …) drives Tab
    /// completion through [`wildmode_stage_at`](Self::wildmode_stage_at)
    /// instead, so changing `'wildmode'` away from the default now
    /// produces genuinely different, observable completion behavior.
    pub(crate) fn wildmode_is_plain_full(&self) -> bool {
        let stages = wildmode_stages(&self.wildmode);
        stages.len() == 1
            && stages[0]
                == WildmodeStage {
                    full: true,
                    ..Default::default()
                }
    }

    /// Does `'backspace'` include `token` (`"indent"`, `"eol"`, `"start"`,
    /// or `"nostop"`)? #1206 — see the field doc comment on
    /// [`Settings::backspace`] for which tokens are distinctly wired.
    pub(crate) fn backspace_allows(&self, token: &str) -> bool {
        self.backspace.split(',').any(|t| t.trim() == token)
    }

    /// Is `c` a "word" character per `'iskeyword'` (#1191)? Drives
    /// `w`/`b`/`e`/`ge`, `*`/`#`/`g*`/`g#`, and the `iw`/`aw` text objects.
    ///
    /// Reparses `self.iskeyword` on every call rather than caching a parsed
    /// form — deliberately: the spec is short (a handful of comma-separated
    /// items) and per-call cost is dominated by matching against those few
    /// items, not by string splitting, so a cache would trade a real
    /// invalidation-correctness risk (stale entries after `:set
    /// iskeyword+=...`) for a speedup on a path that isn't hot (word
    /// motions touch tens of characters, not the whole buffer, per
    /// keystroke). Falls back to the old ASCII+Unicode-alphanumeric default
    /// if the stored spec is somehow unparsable (defensive only —
    /// `set_value_option` validates on write).
    pub(crate) fn is_keyword_char(&self, c: char) -> bool {
        match parse_iskeyword(&self.iskeyword) {
            Ok(entries) => iskeyword_matches(&entries, c),
            Err(_) => c.is_alphanumeric() || c == '_',
        }
    }

    /// The `[...]`-body fragments for `\k` / `\K` (`:h /\k`), derived from
    /// the current `'iskeyword'`. See [`iskeyword_regex_class_body`] for the
    /// include/exclude caveat.
    pub(crate) fn iskeyword_regex_class_bodies(&self) -> (String, String) {
        let entries = parse_iskeyword(&self.iskeyword).unwrap_or_default();
        let k = iskeyword_regex_class_body(&entries);
        // `\K` is `\k` excluding (ASCII) digits — `:h /\K`.
        let big_k = format!("{k}&&[^0-9]");
        (k, big_k)
    }

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
                let _ = settings.save();
                settings
            }
            Err(_) => {
                let defaults = Settings::default();
                let _ = defaults.save();
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

        let mut settings: Self = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse settings.json: {}. Check JSON syntax.", e))?;
        settings.migrate_legacy_fields();
        Ok(settings)
    }

    /// Migrate deprecated fields into their replacements.
    fn migrate_legacy_fields(&mut self) {
        // extension_registry_url (String) → extension_registries (Vec<String>)
        if !self.extension_registry_url.is_empty() {
            let default_regs = default_extension_registries();
            // If registries is still the default (single official URL) and the old field
            // is set to something different, replace with the old value.
            if self.extension_registries == default_regs
                && self.extension_registries.first().map(|s| s.as_str())
                    != Some(&self.extension_registry_url)
            {
                self.extension_registries = vec![self.extension_registry_url.clone()];
            }
            self.extension_registry_url.clear();
        }
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
            let raw_name = arg[..eq_pos].trim();
            let value = arg[eq_pos + 1..].trim();

            // `+=`/`-=`/`^=` (#1191, generalised #1206): Vim's option-modify
            // syntax. For a comma-separated list-style option, append,
            // remove, or prepend items rather than replacing the whole
            // value (`:h :set`, "List of items"). For a number option, add,
            // subtract, or multiply (same section, "For number options").
            //
            // #1191 handled only 'iskeyword'; every other base fell through
            // to the plain `=` path *with the operator still attached to its
            // name* (`raw_name` was e.g. `"whichwrap+"`), so
            // `set_value_option` did an exact-name match against that and
            // reported "Unknown option: whichwrap+" instead of recognising
            // the real option. Stripping the suffix unconditionally here —
            // for every base, not just the ones with a combine helper below
            // — fixes that: an option with no list/numeric handling still
            // falls through to `set_value_option(base, value)`, which now
            // sees the real name and reports its real status (implemented,
            // "recognised but not implemented" for 'clipboard', or a genuine
            // "Unknown option" for an actual typo).
            for (suffix, combine) in [
                ('+', ListOp::Append),
                ('-', ListOp::Remove),
                ('^', ListOp::Prepend),
            ] {
                if let Some(base) = raw_name.strip_suffix(suffix) {
                    let base = base.trim();
                    if let Some(current) = self.current_numeric_value(base) {
                        let delta: i64 = value
                            .parse()
                            .map_err(|_| format!("Invalid value for {base}: '{value}'"))?;
                        let new_val = match combine {
                            ListOp::Append => current + delta,
                            ListOp::Remove => current - delta,
                            ListOp::Prepend => current * delta,
                        };
                        self.set_value_option(base, &new_val.to_string())?;
                        // Re-read rather than trust `new_val` verbatim: some
                        // numeric options clamp on write (e.g. `font_size`),
                        // so the displayed message must match what's
                        // actually stored, not the raw arithmetic result.
                        return self.query_option(base);
                    }
                    if let Some(current) = self.current_list_value(base) {
                        let combined = combine_csv_list(&current, value, combine);
                        self.set_value_option(base, &combined)?;
                        return self.query_option(base);
                    }
                    self.set_value_option(base, value)?;
                    return Ok(format!("{base}={value}"));
                }
            }

            self.set_value_option(raw_name, value)?;
            return Ok(format!("{raw_name}={value}"));
        }

        // Enable a boolean option.
        self.set_bool_option(arg, true)?;
        Ok(arg.to_string())
    }

    /// Parse the `colorcolumn` string into a sorted, deduplicated list of column numbers.
    /// Supports: `"80"`, `"80,120"`, `"+1"` (textwidth + 1), `"-2"` (textwidth - 2).
    pub fn colorcolumn_positions(&self) -> Vec<usize> {
        if self.colorcolumn.is_empty() {
            return Vec::new();
        }
        let mut cols: Vec<usize> = Vec::new();
        for part in self.colorcolumn.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some(offset_str) = part.strip_prefix('+') {
                if let Ok(offset) = offset_str.parse::<usize>() {
                    if self.textwidth > 0 {
                        cols.push(self.textwidth + offset);
                    }
                }
            } else if let Some(offset_str) = part.strip_prefix('-') {
                if let Ok(offset) = offset_str.parse::<usize>() {
                    if self.textwidth > offset {
                        cols.push(self.textwidth - offset);
                    }
                }
            } else if let Ok(col) = part.parse::<usize>() {
                if col > 0 {
                    cols.push(col);
                }
            }
        }
        cols.sort_unstable();
        cols.dedup();
        cols
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
        let spell = if self.spell { "spell" } else { "nospell" };
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
        let nf = if self.use_nerd_fonts() {
            "nerdfonts"
        } else {
            "nonerdfonts"
        };
        let sol = if self.startofline {
            "startofline"
        } else {
            "nostartofline"
        };
        format!(
            "{}  {}  ts={}  sw={}  {}  {}  {}  {}  {}  {}  {}  {}  {}  {}  so={}  tw={}  {}  {}",
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
            spell,
            hls,
            ic,
            sc,
            self.scrolloff,
            self.textwidth,
            nf,
            sol
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
            "spell" => self.spell = enable,
            "hlsearch" | "hls" => self.hlsearch = enable,
            "ignorecase" | "ic" => self.ignorecase = enable,
            "smartcase" | "scs" => self.smartcase = enable,
            "startofline" | "sol" => self.startofline = enable,
            "joinspaces" | "js" => self.joinspaces = enable,
            "smarttab" | "sta" => self.smarttab = enable,
            "cursorline" | "cul" => self.cursorline = enable,
            "windowstatusline" | "wsl" => self.window_status_line = enable,
            "statuslineaboveterminal" | "slat" => self.status_line_above_terminal = enable,
            "autoread" | "ar" => self.autoread = enable,
            "splitbelow" | "sb" => self.splitbelow = enable,
            "splitright" | "spr" => self.splitright = enable,
            "ai_completions" => self.ai_completions = enable,
            "ai_attach_current_buffer" => self.ai_attach_current_buffer = enable,
            "formatonsave" | "fos" => self.format_on_save = enable,
            "showhiddenfiles" | "shf" => self.show_hidden_files = enable,
            "explorersortcaseinsensitive" | "esci" => self.explorer_sort_case_insensitive = enable,
            "swapfile" => self.swap_file = enable,
            "undofile" | "udf" => {
                self.undofile = enable;
                crate::core::undofile::set_enabled(enable);
            }
            "wrapscan" | "ws" => self.wrapscan = enable,
            "shiftround" | "sr" => self.shiftround = enable,
            "gdefault" | "gd" => self.gdefault = enable,
            "breadcrumbs" => self.breadcrumbs = enable,
            "hidesingletab" | "hst" => self.hide_single_tab = enable,
            "autohidepanels" => self.autohide_panels = enable,
            "indentguides" => self.indent_guides = enable,
            "minimap" => self.minimap = enable,
            "matchbrackets" => self.match_brackets = enable,
            "autopairs" => self.auto_pairs = Some(enable),
            "hidden" | "hid" => self.hidden = enable,
            "showcmd" | "sc" => self.showcmd = enable,
            "ruler" | "ru" => self.ruler = enable,
            "list" => self.list = enable,
            "magic" => self.magic = enable,
            "showmatch" | "sm" => self.showmatch = enable,
            "linebreak" | "lbr" => self.linebreak = enable,
            "smartindent" | "si" => self.smartindent = enable,
            "cindent" | "cin" => self.cindent = enable,
            // `"nf"` is Vim's real abbreviation for `'nrformats'` (a
            // value-option, handled in `set_value_option` below) — nerdfonts
            // (a vimcode-only setting with no real-Vim counterpart) keeps
            // only its full name here to avoid claiming that abbreviation.
            "nerdfonts" => {
                self.use_nerd_fonts = Some(enable);
                crate::icons::set_nerd_fonts(enable);
            }
            // `:h 'wildmenu'` describes an enhanced command-line completion
            // menu — vimcode already has one, unconditionally, for every
            // `:` command line (`wildmenu_items`/`wildmenu_selected` in
            // `src/core/engine/keys.rs`, painted by both backends). It isn't
            // gated by any setting, so unlike the genuinely-missing options
            // below, rejecting `set wildmenu` as "not implemented" would be
            // the wrong lie: the requested behavior is already there. Accept
            // both spellings as a no-op (#1153 review).
            "wildmenu" | "wmnu" => {}
            // #1153: real vim boolean options vimcode recognises but does not
            // yet implement. Accepted (never "Unknown option") so a pasted
            // vimrc line doesn't read as a typo, but rejected rather than
            // silently no-op'd, per the issue's "reject with a 'recognised
            // but not implemented' message" deliverable — a silent accept
            // would be a worse lie than a loud one (the user would believe
            // the behavior changed).
            _ if UNIMPLEMENTED_BOOL_OPTIONS
                .iter()
                .any(|(n, a)| *n == opt || *a == opt) =>
            {
                return Err(not_implemented_message(opt));
            }
            _ => {
                // Settings panel shows snake_case keys (e.g. `window_status_line`)
                // but `:set` historically uses vim-style packed names
                // (`windowstatusline` / `wsl`). If the input contains an
                // underscore, retry once with all underscores stripped so
                // the user can paste the panel key directly.
                if opt.contains('_') {
                    let normalized = opt.replace('_', "");
                    if normalized != opt {
                        return self.set_bool_option(&normalized, enable);
                    }
                }
                return Err(format!("Unknown option: {opt}"));
            }
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
            "extension_registries" => {
                self.extension_registries = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "nrformats" | "nf" => {
                self.nrformats = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "hover_delay" | "hd" => {
                let n: u32 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.hover_delay = n;
            }
            "font_size" => {
                let n: i32 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.font_size = n.clamp(6, 72);
            }
            "ui_font_size" => {
                let n: u32 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.ui_font_size = n.clamp(6, 32) as u8;
            }
            "font_family" => {
                self.font_family = value.to_string();
            }
            "syntax_max_lines" | "syntaxmaxlines" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.syntax_max_lines = n;
                crate::core::buffer_manager::set_syntax_max_lines(n);
            }
            "undolevels" | "ul" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.undolevels = n;
                crate::core::buffer_manager::set_undo_levels(n);
            }
            "undodir" | "udir" => {
                self.undodir = value.to_string();
                crate::core::undofile::set_dir(value);
            }
            "softtabstop" | "sts" => {
                let n: i32 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.softtabstop = n;
            }
            "virtualedit" | "ve" => {
                // `:h 'virtualedit'`: a comma-separated list of `all`,
                // `block`, `insert`, `onemore`, or the empty string (off).
                // Only `all`/`onemore`'s "one column past end of line"
                // effect is implemented — see the field doc comment — but
                // every real token is still accepted so `:set ve=all`
                // doesn't read as invalid input.
                if !value.is_empty() {
                    for tok in value.split(',') {
                        if !matches!(tok, "all" | "block" | "insert" | "onemore") {
                            return Err(format!(
                                "Invalid value for {name}: '{value}' (expected a comma-separated \
                                 list of all/block/insert/onemore, or empty)"
                            ));
                        }
                    }
                }
                self.virtualedit = value.to_string();
            }
            "foldmethod" | "fdm" => {
                if !matches!(value, "manual" | "indent" | "marker") {
                    return Err(format!(
                        "Invalid value for {name}: '{value}' (only 'manual'/'indent'/'marker' \
                         are implemented)"
                    ));
                }
                self.foldmethod = value.to_string();
            }
            "foldlevel" | "fdl" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.foldlevel = n;
            }
            // `:h 'foldmarker'`: exactly two non-empty, comma-separated
            // strings — "the two markers must be different, in order to
            // avoid ambiguity" is Vim's own wording, but Vim doesn't
            // actually enforce that (a same-string pair just never closes a
            // fold, since every occurrence looks like an open), so this
            // doesn't either.
            "foldmarker" | "fmr" => {
                let Some((open, close)) = value.split_once(',') else {
                    return Err(format!(
                        "Invalid value for {name}: '{value}' (expected \
                         'open,close', e.g. '{{{{{{,}}}}}}')"
                    ));
                };
                if open.is_empty() || close.is_empty() || close.contains(',') {
                    return Err(format!(
                        "Invalid value for {name}: '{value}' (expected 'open,close', e.g. \
                         '{{{{{{,}}}}}}')"
                    ));
                }
                self.foldmarker = value.to_string();
            }
            "foldnestmax" | "fdn" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                if n == 0 {
                    return Err(format!(
                        "Invalid value for {name}: '{value}' (must be at least 1)"
                    ));
                }
                self.foldnestmax = n;
            }
            // #1191: validate eagerly (rather than storing an unparsable
            // spec and only failing later, per-character, in
            // `is_keyword_char`) so a typo'd vimrc line is rejected the way
            // Vim rejects it, not silently downgraded to the ASCII default.
            "iskeyword" | "isk" => {
                parse_iskeyword(value).map_err(|e| {
                    format!(
                        "Invalid value for {name}: '{value}' ({e}; expected a comma-separated \
                         list of characters, 'c1-c2' ranges, decimal codes, decimal 'n1-n2' \
                         ranges, or '@', each optionally prefixed with '^' to exclude)"
                    )
                })?;
                self.iskeyword = value.to_string();
            }
            // #1206
            "whichwrap" | "ww" => {
                parse_whichwrap(value).map_err(|e| {
                    format!(
                        "Invalid value for {name}: '{value}' ({e}; expected a comma-separated \
                         list of b/s/h/l/</>/~/[/])"
                    )
                })?;
                self.whichwrap = value.to_string();
            }
            "backspace" | "bs" => {
                let normalized = parse_backspace(value).map_err(|e| {
                    format!(
                        "Invalid value for {name}: '{value}' ({e}; expected a comma-separated \
                         list of indent/eol/start/nostop, or the legacy 0-3)"
                    )
                })?;
                self.backspace = normalized;
            }
            "timeoutlen" | "tm" => {
                let n: u32 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.timeoutlen = n;
            }
            "wildmode" | "wim" => {
                parse_wildmode(value).map_err(|e| {
                    format!(
                        "Invalid value for {name}: '{value}' ({e}; expected comma-separated \
                         stages of colon-separated full/longest/list/lastused)"
                    )
                })?;
                self.wildmode = value.to_string();
            }
            "laststatus" | "ls" => {
                let n: u8 = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                if n > 3 {
                    return Err(format!(
                        "Invalid value for {name}: '{value}' (expected 0-3)"
                    ));
                }
                self.laststatus = n;
            }
            "sidescrolloff" | "siso" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.sidescrolloff = n;
            }
            "scrolljump" | "sj" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid value for {name}: '{value}'"))?;
                self.scrolljump = n;
            }
            "listchars" | "lcs" => {
                parse_listchars(value).map_err(|e| {
                    format!(
                        "Invalid value for {name}: '{value}' ({e}; expected comma-separated \
                         item:chars pairs, e.g. 'tab:> ,trail:-')"
                    )
                })?;
                self.listchars = value.to_string();
            }
            // #1153: see `UNIMPLEMENTED_BOOL_OPTIONS`'s doc comment — same
            // rationale, value-option side.
            _ if UNIMPLEMENTED_VALUE_OPTIONS
                .iter()
                .any(|(n, a)| *n == name || *a == name) =>
            {
                return Err(not_implemented_message(name));
            }
            _ => {
                // Snake_case → packed-name fallback (see `set_bool_option`).
                if name.contains('_') {
                    let normalized = name.replace('_', "");
                    if normalized != name {
                        return self.set_value_option(&normalized, value);
                    }
                }
                return Err(format!("Unknown option: {name}"));
            }
        }
        Ok(())
    }

    /// The current value of `base` as an `i64`, if `base` names a numeric
    /// value option — used by `parse_set_option`'s `+=`/`-=`/`^=` handling
    /// (#1206) to compute add/subtract/multiply without a second, parallel
    /// name-to-field match. Deliberately narrow: only options whose
    /// `set_value_option` arm does a plain integer parse belong here — a
    /// list-style option (even one that happens to look numeric, like the
    /// legacy `'backspace'` shorthand) must go through
    /// [`Self::current_list_value`] instead, or it would silently reinterpret
    /// `:set bs+=1` as arithmetic instead of the list append Vim performs.
    fn current_numeric_value(&self, base: &str) -> Option<i64> {
        match base {
            "tabstop" | "ts" => Some(self.tabstop as i64),
            "shiftwidth" | "sw" => Some(self.shift_width as i64),
            "scrolloff" | "so" => Some(self.scrolloff as i64),
            "textwidth" | "tw" => Some(self.textwidth as i64),
            "updatetime" | "ut" => Some(self.updatetime as i64),
            "hover_delay" | "hd" => Some(self.hover_delay as i64),
            "font_size" => Some(self.font_size as i64),
            "ui_font_size" => Some(self.ui_font_size as i64),
            "syntax_max_lines" | "syntaxmaxlines" => Some(self.syntax_max_lines as i64),
            "undolevels" | "ul" => Some(self.undolevels as i64),
            "softtabstop" | "sts" => Some(self.softtabstop as i64),
            "foldlevel" | "fdl" => Some(self.foldlevel as i64),
            "foldnestmax" | "fdn" => Some(self.foldnestmax as i64),
            "timeoutlen" | "tm" => Some(self.timeoutlen as i64),
            "laststatus" | "ls" => Some(self.laststatus as i64),
            "sidescrolloff" | "siso" => Some(self.sidescrolloff as i64),
            "scrolljump" | "sj" => Some(self.scrolljump as i64),
            _ => None,
        }
    }

    /// The current value of `base` as a raw comma-separated string, if
    /// `base` names a list-style value option — used by
    /// `parse_set_option`'s `+=`/`-=`/`^=` handling (#1206) the same way
    /// [`Self::current_numeric_value`] is. `'clipboard'` deliberately has no
    /// arm: it's list-shaped in real Vim, but vimcode has no stored value to
    /// combine against (it's in `UNIMPLEMENTED_VALUE_OPTIONS`) — leaving it
    /// out here means its `+=`/`-=`/`^=` case falls through to the plain
    /// `set_value_option(base, value)` call, which is what reports the
    /// "recognised but not implemented" message.
    fn current_list_value(&self, base: &str) -> Option<String> {
        match base {
            "iskeyword" | "isk" => Some(self.iskeyword.clone()),
            "whichwrap" | "ww" => Some(self.whichwrap.clone()),
            "wildmode" | "wim" => Some(self.wildmode.clone()),
            "listchars" | "lcs" => Some(self.listchars.clone()),
            // Pre-existing list-style options #1206 didn't add but whose
            // `+=`/`-=`/`^=` this same generalisation now has to get right
            // too — without an arm here, the generic strip above would
            // silently reinterpret e.g. `:set nrformats+=octal` as `:set
            // nrformats=octal` (replacing, not appending), which is a worse
            // outcome than #1191's old "Unknown option: nrformats+" error.
            "nrformats" | "nf" => Some(self.nrformats.join(",")),
            "colorcolumn" | "cc" => Some(self.colorcolumn.clone()),
            "virtualedit" | "ve" => Some(self.virtualedit.clone()),
            // 'backspace' also accepts a legacy *numeric* shorthand
            // (0-3, expanded by `parse_backspace`), which would collide with
            // `current_numeric_value`'s arithmetic if it were listed there
            // too — it belongs here, as a list option, because `:h :set`
            // classifies `'backspace'` itself as a list-of-items string
            // option, and Vim's own `+=`/`-=`/`^=` on it does list
            // append/remove/prepend, not arithmetic on the legacy digit.
            "backspace" | "bs" => Some(self.backspace.clone()),
            _ => None,
        }
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
            "spell" => Ok(if self.spell {
                "spell".to_string()
            } else {
                "nospell".to_string()
            }),
            "spelllang" => Ok(format!("spelllang={}", self.spelllang)),
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
            "startofline" | "sol" => Ok(if self.startofline {
                "startofline".to_string()
            } else {
                "nostartofline".to_string()
            }),
            "joinspaces" | "js" => Ok(if self.joinspaces {
                "joinspaces".to_string()
            } else {
                "nojoinspaces".to_string()
            }),
            "smarttab" | "sta" => Ok(if self.smarttab {
                "smarttab".to_string()
            } else {
                "nosmarttab".to_string()
            }),
            "nrformats" | "nf" => Ok(format!("nrformats={}", self.nrformats.join(","))),
            "cursorline" | "cul" => Ok(if self.cursorline {
                "cursorline".to_string()
            } else {
                "nocursorline".to_string()
            }),
            "windowstatusline" | "wsl" => Ok(if self.window_status_line {
                "windowstatusline".to_string()
            } else {
                "nowindowstatusline".to_string()
            }),
            "statuslineaboveterminal" | "slat" => Ok(if self.status_line_above_terminal {
                "statuslineaboveterminal".to_string()
            } else {
                "nostatuslineaboveterminal".to_string()
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
            "explorersortcaseinsensitive" | "esci" => Ok(if self.explorer_sort_case_insensitive {
                "explorersortcaseinsensitive".to_string()
            } else {
                "noexplorersortcaseinsensitive".to_string()
            }),
            "swapfile" => Ok(if self.swap_file {
                "swapfile".to_string()
            } else {
                "noswapfile".to_string()
            }),
            "updatetime" | "ut" => Ok(format!("updatetime={}", self.updatetime)),
            "breadcrumbs" => Ok(if self.breadcrumbs {
                "breadcrumbs".to_string()
            } else {
                "nobreadcrumbs".to_string()
            }),
            "hidesingletab" | "hst" => Ok(if self.hide_single_tab {
                "hidesingletab".to_string()
            } else {
                "nohidesingletab".to_string()
            }),
            "autohidepanels" => Ok(if self.autohide_panels {
                "autohidepanels".to_string()
            } else {
                "noautohidepanels".to_string()
            }),
            "indentguides" => Ok(if self.indent_guides {
                "indentguides".to_string()
            } else {
                "noindentguides".to_string()
            }),
            "minimap" => Ok(if self.minimap {
                "minimap".to_string()
            } else {
                "nominimap".to_string()
            }),
            "matchbrackets" => Ok(if self.match_brackets {
                "matchbrackets".to_string()
            } else {
                "nomatchbrackets".to_string()
            }),
            "autopairs" => Ok(if self.auto_pairs() {
                "autopairs".to_string()
            } else {
                "noautopairs".to_string()
            }),
            "hidden" | "hid" => Ok(if self.hidden {
                "hidden".to_string()
            } else {
                "nohidden".to_string()
            }),
            "showcmd" | "sc" => Ok(if self.showcmd {
                "showcmd".to_string()
            } else {
                "noshowcmd".to_string()
            }),
            "ruler" | "ru" => Ok(if self.ruler {
                "ruler".to_string()
            } else {
                "noruler".to_string()
            }),
            "list" => Ok(if self.list {
                "list".to_string()
            } else {
                "nolist".to_string()
            }),
            "magic" => Ok(if self.magic {
                "magic".to_string()
            } else {
                "nomagic".to_string()
            }),
            "showmatch" | "sm" => Ok(if self.showmatch {
                "showmatch".to_string()
            } else {
                "noshowmatch".to_string()
            }),
            "linebreak" | "lbr" => Ok(if self.linebreak {
                "linebreak".to_string()
            } else {
                "nolinebreak".to_string()
            }),
            "smartindent" | "si" => Ok(if self.smartindent {
                "smartindent".to_string()
            } else {
                "nosmartindent".to_string()
            }),
            "cindent" | "cin" => Ok(if self.cindent {
                "cindent".to_string()
            } else {
                "nocindent".to_string()
            }),
            "extension_registries" => Ok(format!(
                "extension_registries={}",
                self.extension_registries.join(",")
            )),
            "hover_delay" | "hd" => Ok(format!("hover_delay={}", self.hover_delay)),
            "nerdfonts" => Ok(if self.use_nerd_fonts() {
                "nerdfonts".to_string()
            } else {
                "nonerdfonts".to_string()
            }),
            "syntax_max_lines" | "syntaxmaxlines" => {
                Ok(format!("syntax_max_lines={}", self.syntax_max_lines))
            }
            "undolevels" | "ul" => Ok(format!("undolevels={}", self.undolevels)),
            "undofile" | "udf" => Ok(if self.undofile {
                "undofile".to_string()
            } else {
                "noundofile".to_string()
            }),
            "undodir" | "udir" => Ok(format!("undodir={}", self.undodir)),
            "wrapscan" | "ws" => Ok(if self.wrapscan {
                "wrapscan".to_string()
            } else {
                "nowrapscan".to_string()
            }),
            "shiftround" | "sr" => Ok(if self.shiftround {
                "shiftround".to_string()
            } else {
                "noshiftround".to_string()
            }),
            "gdefault" | "gd" => Ok(if self.gdefault {
                "gdefault".to_string()
            } else {
                "nogdefault".to_string()
            }),
            "softtabstop" | "sts" => Ok(format!("softtabstop={}", self.softtabstop)),
            "virtualedit" | "ve" => Ok(format!("virtualedit={}", self.virtualedit)),
            "foldmethod" | "fdm" => Ok(format!("foldmethod={}", self.foldmethod)),
            "foldlevel" | "fdl" => Ok(format!("foldlevel={}", self.foldlevel)),
            "foldmarker" | "fmr" => Ok(format!("foldmarker={}", self.foldmarker)),
            "foldnestmax" | "fdn" => Ok(format!("foldnestmax={}", self.foldnestmax)),
            "iskeyword" | "isk" => Ok(format!("iskeyword={}", self.iskeyword)),
            // Always on — see the `set_bool_option` "wildmenu" | "wmnu" arm.
            "wildmenu" | "wmnu" => Ok("wildmenu".to_string()),
            // #1206
            "whichwrap" | "ww" => Ok(format!("whichwrap={}", self.whichwrap)),
            "backspace" | "bs" => Ok(format!("backspace={}", self.backspace)),
            "timeoutlen" | "tm" => Ok(format!("timeoutlen={}", self.timeoutlen)),
            "wildmode" | "wim" => Ok(format!("wildmode={}", self.wildmode)),
            "laststatus" | "ls" => Ok(format!("laststatus={}", self.laststatus)),
            "sidescrolloff" | "siso" => Ok(format!("sidescrolloff={}", self.sidescrolloff)),
            "scrolljump" | "sj" => Ok(format!("scrolljump={}", self.scrolljump)),
            "listchars" | "lcs" => Ok(format!("listchars={}", self.listchars)),
            _ if UNIMPLEMENTED_BOOL_OPTIONS
                .iter()
                .any(|(n, a)| *n == opt || *a == opt)
                || UNIMPLEMENTED_VALUE_OPTIONS
                    .iter()
                    .any(|(n, a)| *n == opt || *a == opt) =>
            {
                Err(not_implemented_message(opt))
            }
            _ => {
                // Snake_case → packed-name fallback (see `set_bool_option`).
                if opt.contains('_') {
                    let normalized = opt.replace('_', "");
                    if normalized != opt {
                        return self.query_option(&normalized);
                    }
                }
                Err(format!("Unknown option: {opt}"))
            }
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

        // Bump the save revision so the TUI mtime watcher knows this write
        // came from us and can suppress its "Settings reloaded" message.
        SAVE_REVISION.fetch_add(1, Ordering::AcqRel);

        Ok(())
    }

    /// Where `settings.json` lives — `~/.config/vimcode/settings.json`
    /// (or the platform equivalent, see [`super::paths::vimcode_config_dir`]).
    ///
    /// Under `#[cfg(test)]`, a per-thread override installed via
    /// [`TestSettingsPathGuard::install`] takes priority when set (#949
    /// review).
    /// This is a `thread_local`, not a `$HOME` env-var mutation, precisely
    /// because `std::env::set_var` is process-global: Rust's default test
    /// runner executes tests in parallel on multiple threads within the
    /// same process, so mutating `$HOME` from one test would race every
    /// other concurrently-running test that (transitively, via
    /// `Engine::new`/`check_settings_reload`) also resolves this path.
    /// A thread-local override carries no such risk — each test thread
    /// gets its own slot — and needs no `serial_test`/lock discipline this
    /// codebase doesn't otherwise have.
    pub fn settings_file_path() -> PathBuf {
        #[cfg(test)]
        {
            if let Some(p) = TEST_SETTINGS_PATH_OVERRIDE.with(|cell| cell.borrow().clone()) {
                return p;
            }
        }
        super::paths::vimcode_config_dir().join("settings.json")
    }

    /// Get the current value of a setting as a string.
    /// Returns an empty string for unknown keys.
    /// Used by the settings UI form to populate widget initial values.
    pub fn get_value_str(&self, key: &str) -> String {
        match key {
            "colorscheme" => self.colorscheme.clone(),
            "font_family" => self.font_family.clone(),
            "font_size" => self.font_size.to_string(),
            "ui_font_size" => self.ui_font_size.to_string(),
            "line_numbers" => match self.line_numbers {
                LineNumberMode::None => "none".to_string(),
                LineNumberMode::Absolute => "absolute".to_string(),
                LineNumberMode::Relative => "relative".to_string(),
                LineNumberMode::Hybrid => "hybrid".to_string(),
            },
            "cursorline" => self.cursorline.to_string(),
            "window_status_line" => self.window_status_line.to_string(),
            "status_line_above_terminal" => self.status_line_above_terminal.to_string(),
            "tabstop" => self.tabstop.to_string(),
            "shift_width" => self.shift_width.to_string(),
            "expand_tab" => self.expand_tab.to_string(),
            "auto_indent" => self.auto_indent.to_string(),
            "wrap" => self.wrap.to_string(),
            "spell" => self.spell.to_string(),
            "spelllang" => self.spelllang.clone(),
            "scrolloff" => self.scrolloff.to_string(),
            "startofline" | "sol" => self.startofline.to_string(),
            "joinspaces" | "js" => self.joinspaces.to_string(),
            "hidden" | "hid" => self.hidden.to_string(),
            "showcmd" | "sc" => self.showcmd.to_string(),
            "ruler" | "ru" => self.ruler.to_string(),
            "list" => self.list.to_string(),
            "listchars" | "lcs" => self.listchars.clone(),
            "whichwrap" | "ww" => self.whichwrap.clone(),
            "backspace" | "bs" => self.backspace.clone(),
            "timeoutlen" | "tm" => self.timeoutlen.to_string(),
            "wildmode" | "wim" => self.wildmode.clone(),
            "laststatus" | "ls" => self.laststatus.to_string(),
            "sidescrolloff" | "siso" => self.sidescrolloff.to_string(),
            "scrolljump" | "sj" => self.scrolljump.to_string(),
            "smarttab" | "sta" => self.smarttab.to_string(),
            "nrformats" | "nf" => self.nrformats.join(","),
            "iskeyword" | "isk" => self.iskeyword.clone(),
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
            "menu_style" => match self.menu_style {
                MenuStyle::Native => "native".to_string(),
                MenuStyle::Custom => "custom".to_string(),
                MenuStyle::Inherit => "inherit".to_string(),
            },
            "explorer_visible_on_startup" => self.explorer_visible_on_startup.to_string(),
            "autoread" => self.autoread.to_string(),
            "splitbelow" => self.splitbelow.to_string(),
            "splitright" => self.splitright.to_string(),
            "lsp_enabled" => self.lsp_enabled.to_string(),
            "format_on_save" => self.format_on_save.to_string(),
            "board_tick_enabled" => self.board_tick_enabled.to_string(),
            "terminal_scrollback_lines" => self.terminal_scrollback_lines.to_string(),
            "plugins_enabled" => self.plugins_enabled.to_string(),
            "ai_provider" => self.ai_provider.clone(),
            "ai_api_key" => self.ai_api_key.clone(),
            "ai_model" => self.ai_model.clone(),
            "ai_base_url" => self.ai_base_url.clone(),
            "ai_completions" => self.ai_completions.to_string(),
            "ai_attach_current_buffer" => self.ai_attach_current_buffer.to_string(),
            "acp_agent_command" => self.acp_agent_command.clone(),
            "showhiddenfiles" | "shf" | "show_hidden_files" => self.show_hidden_files.to_string(),
            "explorersortcaseinsensitive" | "esci" | "explorer_sort_case_insensitive" => {
                self.explorer_sort_case_insensitive.to_string()
            }
            "swapfile" | "swap_file" => self.swap_file.to_string(),
            "updatetime" | "ut" => self.updatetime.to_string(),
            "undolevels" | "ul" => self.undolevels.to_string(),
            "undofile" | "udf" => self.undofile.to_string(),
            "undodir" | "udir" => self.undodir.clone(),
            "breadcrumbs" => self.breadcrumbs.to_string(),
            "hide_single_tab" | "hidesingletab" | "hst" => self.hide_single_tab.to_string(),
            "autohide_panels" | "autohidepanels" => self.autohide_panels.to_string(),
            "indent_guides" | "indentguides" => self.indent_guides.to_string(),
            "minimap" => self.minimap.to_string(),
            "match_brackets" | "matchbrackets" => self.match_brackets.to_string(),
            "auto_pairs" | "autopairs" => self.auto_pairs().to_string(),
            "hover_delay" => self.hover_delay.to_string(),
            "use_nerd_fonts" | "nerdfonts" => self.use_nerd_fonts().to_string(),
            "ctrl_f_action" => self.ctrl_f_action(),
            "extension_registries" => self.extension_registries.join(", "),
            "syntax_max_lines" | "syntaxmaxlines" => self.syntax_max_lines.to_string(),
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
            "ui_font_size" => {
                let n: u8 = value
                    .parse()
                    .map_err(|_| format!("Invalid ui_font_size: {value}"))?;
                self.ui_font_size = n;
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
            "window_status_line" => self.window_status_line = value == "true",
            "status_line_above_terminal" => self.status_line_above_terminal = value == "true",
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
            "spell" => self.spell = value == "true",
            "spelllang" => self.spelllang = value.to_string(),
            "scrolloff" => {
                self.scrolloff = value
                    .parse()
                    .map_err(|_| format!("Invalid scrolloff: {value}"))?;
            }
            "startofline" | "sol" => self.startofline = value == "true",
            "joinspaces" | "js" => self.joinspaces = value == "true",
            "smarttab" | "sta" => self.smarttab = value == "true",
            "nrformats" | "nf" => {
                self.nrformats = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "iskeyword" | "isk" => {
                parse_iskeyword(value)
                    .map_err(|e| format!("Invalid iskeyword: '{value}' ({e})"))?;
                self.iskeyword = value.to_string();
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
            "menu_style" => {
                self.menu_style = match value {
                    "native" => MenuStyle::Native,
                    "custom" => MenuStyle::Custom,
                    "inherit" => MenuStyle::Inherit,
                    _ => return Err(format!("Unknown menu_style: {value}")),
                };
            }
            "explorer_visible_on_startup" => self.explorer_visible_on_startup = value == "true",
            "autoread" => self.autoread = value == "true",
            "splitbelow" => self.splitbelow = value == "true",
            "splitright" => self.splitright = value == "true",
            "lsp_enabled" => self.lsp_enabled = value == "true",
            "format_on_save" => self.format_on_save = value == "true",
            "board_tick_enabled" => self.board_tick_enabled = value == "true",
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
            "ai_attach_current_buffer" => self.ai_attach_current_buffer = value == "true",
            "acp_agent_command" => self.acp_agent_command = value.to_string(),
            "showhiddenfiles" | "shf" | "show_hidden_files" => {
                self.show_hidden_files = value == "true"
            }
            "explorersortcaseinsensitive" | "esci" | "explorer_sort_case_insensitive" => {
                self.explorer_sort_case_insensitive = value == "true"
            }
            "swapfile" | "swap_file" => self.swap_file = value == "true",
            "updatetime" | "ut" => {
                self.updatetime = value
                    .parse()
                    .map_err(|_| format!("Invalid updatetime: {value}"))?;
            }
            "undolevels" | "ul" => {
                self.undolevels = value
                    .parse()
                    .map_err(|_| format!("Invalid undolevels: {value}"))?;
                crate::core::buffer_manager::set_undo_levels(self.undolevels);
            }
            "undofile" | "udf" => {
                self.undofile = value == "true";
                crate::core::undofile::set_enabled(self.undofile);
            }
            "undodir" | "udir" => {
                self.undodir = value.to_string();
                crate::core::undofile::set_dir(&self.undodir);
            }
            "breadcrumbs" => self.breadcrumbs = value == "true",
            "hide_single_tab" | "hidesingletab" | "hst" => self.hide_single_tab = value == "true",
            "autohide_panels" | "autohidepanels" => self.autohide_panels = value == "true",
            "indent_guides" | "indentguides" => self.indent_guides = value == "true",
            "minimap" => self.minimap = value == "true",
            "match_brackets" | "matchbrackets" => self.match_brackets = value == "true",
            "auto_pairs" | "autopairs" => self.auto_pairs = Some(value == "true"),
            "hover_delay" => {
                self.hover_delay = value
                    .parse()
                    .map_err(|_| format!("Invalid hover_delay: {value}"))?;
            }
            "use_nerd_fonts" | "nerdfonts" => {
                self.use_nerd_fonts = Some(value == "true");
                crate::icons::set_nerd_fonts(self.use_nerd_fonts());
            }
            "ctrl_f_action" => match value {
                "find" | "page_down" => self.ctrl_f_action = Some(value.to_string()),
                _ => {
                    return Err(format!(
                        "Invalid ctrl_f_action: {value} (expected 'find' or 'page_down')"
                    ))
                }
            },
            "extension_registries" => {
                self.extension_registries = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "syntax_max_lines" | "syntaxmaxlines" => {
                self.syntax_max_lines = value
                    .parse()
                    .map_err(|_| format!("Invalid syntax_max_lines: {value}"))?;
                crate::core::buffer_manager::set_syntax_max_lines(self.syntax_max_lines);
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

// ─── Setting definitions (UI metadata) ───────────────────────────────────────

/// The type of a user-configurable setting, used to generate appropriate form widgets.
#[derive(Debug, Clone)]
pub enum SettingType {
    Bool,
    /// `min`/`max` encode the valid range — kept for future range-aware
    /// Form widgets (Slider, etc., per issue #143). Currently unread by
    /// any backend now that GTK no longer renders a `SpinButton`.
    #[allow(dead_code)]
    Integer {
        min: i32,
        max: i32,
    },
    StringVal,
    Enum(&'static [&'static str]),
    /// Like Enum but options are computed at runtime (e.g. colorscheme includes custom themes).
    DynamicEnum(fn() -> Vec<String>),
    /// Opens a scratch buffer editor (e.g. keymaps editor). Enter to open.
    BufferEditor,
}

/// Returns available colorscheme names: built-in + custom themes from ~/.config/vimcode/themes/.
pub fn available_colorschemes() -> Vec<String> {
    let mut names: Vec<String> = vec![
        "onedark".into(),
        "gruvbox-dark".into(),
        "tokyo-night".into(),
        "solarized-dark".into(),
        "vscode-dark".into(),
        "vscode-light".into(),
    ];
    {
        let dir = super::paths::vimcode_config_dir().join("themes");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
    }
    names
}

/// Metadata for a single user-configurable setting.
pub struct SettingDef {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub setting_type: SettingType,
}

/// All user-configurable settings exposed in the Settings sidebar form.
///
/// When adding a new field to `Settings` struct, add a corresponding entry here
/// so it appears in the UI form.
pub static SETTING_DEFS: &[SettingDef] = &[
    // ── Appearance ───────────────────────────────────────────────────────────
    SettingDef {
        key: "colorscheme",
        label: "Color Scheme",
        description: "Editor color theme",
        category: "Appearance",
        setting_type: SettingType::DynamicEnum(available_colorschemes),
    },
    SettingDef {
        key: "font_family",
        label: "Font Family",
        description: "Editor font family (e.g. \"JetBrains Mono\")",
        category: "Appearance",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "font_size",
        label: "Font Size",
        description: "Editor font size in points",
        category: "Appearance",
        setting_type: SettingType::Integer { min: 6, max: 48 },
    },
    SettingDef {
        key: "ui_font_size",
        label: "UI Font Size",
        description: "Font size for menus, sidebars, dialogs, and hover popups",
        category: "Appearance",
        setting_type: SettingType::Integer { min: 6, max: 32 },
    },
    SettingDef {
        key: "use_nerd_fonts",
        label: "Nerd Font Icons",
        description: "Use Nerd Font glyphs for UI icons (disable for ASCII fallbacks)",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "line_numbers",
        label: "Line Numbers",
        description: "How line numbers are displayed in the gutter",
        category: "Appearance",
        setting_type: SettingType::Enum(&["none", "absolute", "relative", "hybrid"]),
    },
    SettingDef {
        key: "cursorline",
        label: "Cursor Line",
        description: "Highlight the line containing the cursor",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "window_status_line",
        label: "Per-Window Status Line",
        description: "Show a status line at the bottom of each window instead of a single global bar",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "status_line_above_terminal",
        label: "Status Line Inside Window",
        description: "Keep each window's status line inside that window. When off and the terminal panel is open, a separated status row is shown above the terminal instead.",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "breadcrumbs",
        label: "Breadcrumbs",
        description: "Show file path and symbol hierarchy below the tab bar",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "hide_single_tab",
        label: "Hide Single Tab",
        description: "Hide the tab bar when an editor group has only one tab",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
    // ── Editor ───────────────────────────────────────────────────────────────
    SettingDef {
        key: "tabstop",
        label: "Tab Size",
        description: "Number of spaces a Tab key inserts (or tab display width)",
        category: "Editor",
        setting_type: SettingType::Integer { min: 1, max: 16 },
    },
    SettingDef {
        key: "shift_width",
        label: "Indent Width",
        description: "Spaces added/removed by indent operators (<< / >>)",
        category: "Editor",
        setting_type: SettingType::Integer { min: 1, max: 16 },
    },
    SettingDef {
        key: "expand_tab",
        label: "Expand Tabs",
        description: "Insert spaces instead of a literal tab character",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "auto_indent",
        label: "Auto Indent",
        description: "Automatically indent new lines to match current indent",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "wrap",
        label: "Word Wrap",
        description: "Wrap long lines at the viewport width instead of scrolling",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "scrolloff",
        label: "Scroll Offset",
        description: "Minimum lines to keep visible above and below the cursor",
        category: "Editor",
        setting_type: SettingType::Integer { min: 0, max: 30 },
    },
    SettingDef {
        key: "startofline",
        label: "Start Of Line",
        description: "Land on the first non-blank column after G, gg, H, M, L, <C-d>, <C-u>, <C-b>, <C-f> (Vim's default; Neovim's is off)",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "joinspaces",
        label: "Join Spaces",
        description: "Insert two spaces instead of one when J joins a line ending in '.', '!' or '?' (Vim's default; Neovim's is off)",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "smarttab",
        label: "Smart Tab",
        description: "Tab/Backspace at the start of a line use 'shift_width' instead of 'tabstop' (Vim's default is off; Neovim's is on)",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "nrformats",
        label: "Number Formats",
        description: "Extra numeral formats <C-a>/<C-x> recognize besides decimal: bin, octal, hex, alpha (comma-separated; Neovim's default is \"bin,hex\")",
        category: "Editor",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "colorcolumn",
        label: "Color Column",
        description: "Columns to highlight as rulers (e.g. \"80,120\")",
        category: "Editor",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "iskeyword",
        label: "Keyword Characters",
        description: "Characters word motions (w/b/e, */#, iw/aw) treat as part of a word",
        category: "Editor",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "textwidth",
        label: "Text Width",
        description: "Auto-wrap inserted text at this column (0 = disabled)",
        category: "Editor",
        setting_type: SettingType::Integer { min: 0, max: 200 },
    },
    SettingDef {
        key: "swap_file",
        label: "Swap Files",
        description: "Write swap files for crash recovery (like Vim's swapfile option)",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "updatetime",
        label: "Update Time",
        description: "Milliseconds between swap file writes for dirty buffers",
        category: "Editor",
        setting_type: SettingType::Integer {
            min: 100,
            max: 60000,
        },
    },
    SettingDef {
        key: "undolevels",
        label: "Undo Levels",
        description: "Maximum number of undo states kept per buffer, across every branch (like Vim's undolevels option)",
        category: "Editor",
        setting_type: SettingType::Integer {
            min: 1,
            max: 1_000_000,
        },
    },
    SettingDef {
        key: "undofile",
        label: "Persistent Undo",
        description: "Save undo history to disk so it survives closing and reopening a file (like Vim's undofile option)",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "undodir",
        label: "Undo Directory",
        description: "Directory undo history is saved to when Persistent Undo is on (default: ~/.config/vimcode/undo/)",
        category: "Editor",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "syntax_max_lines",
        label: "Syntax Highlighting Line Limit",
        description: "Skip tree-sitter highlighting for buffers over this many lines (plain text for huge generated files)",
        category: "Editor",
        setting_type: SettingType::Integer {
            min: 0,
            max: 1_000_000,
        },
    },
    SettingDef {
        key: "spell",
        label: "Spell Check",
        description: "Highlight misspelled words (comments/strings in code files)",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "spelllang",
        label: "Spell Language",
        description: "Spell-check language code (e.g. en_US)",
        category: "Editor",
        setting_type: SettingType::StringVal,
    },
    // ── Search ───────────────────────────────────────────────────────────────
    SettingDef {
        key: "hlsearch",
        label: "Highlight Search",
        description: "Highlight all matches of the last search pattern",
        category: "Search",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "ignorecase",
        label: "Ignore Case",
        description: "Case-insensitive search by default",
        category: "Search",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "smartcase",
        label: "Smart Case",
        description: "Override Ignore Case when the pattern has an uppercase letter",
        category: "Search",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "incremental_search",
        label: "Incremental Search",
        description: "Move the cursor as you type the search pattern",
        category: "Search",
        setting_type: SettingType::Bool,
    },
    // ── Workspace ────────────────────────────────────────────────────────────
    SettingDef {
        key: "editor_mode",
        label: "Editor Mode",
        description: "Vim (modal) or VSCode (always-insert) key bindings",
        category: "Workspace",
        setting_type: SettingType::Enum(&["vim", "vscode"]),
    },
    SettingDef {
        key: "menu_style",
        label: "Context Menu Style",
        description: "Native OS context menu, the in-window one, or inherit \
                       from the window style (only observable on a backend \
                       with a native context menu, e.g. macOS)",
        category: "Workspace",
        setting_type: SettingType::Enum(&["native", "custom", "inherit"]),
    },
    SettingDef {
        key: "explorer_visible_on_startup",
        label: "Show Explorer on Start",
        description: "Open the file explorer sidebar automatically on startup",
        category: "Workspace",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "autoread",
        label: "Auto Read",
        description: "Automatically reload files when changed externally",
        category: "Workspace",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "splitbelow",
        label: "Split Below",
        description: "Open new horizontal splits below the current window",
        category: "Workspace",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "splitright",
        label: "Split Right",
        description: "Open new vertical splits to the right of the current window",
        category: "Workspace",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "show_hidden_files",
        label: "Show Hidden Files",
        description: "Display dotfiles and hidden directories in the file explorer",
        category: "Workspace",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "board_tick_enabled",
        label: "Board Auto-Tick",
        description: "Periodically run the Board panel's provider-declared \
                       tick command so its pipeline advances even when \
                       vimcode is the only client with the board open \
                       (off by default — this can dispatch metered work)",
        category: "Workspace",
        setting_type: SettingType::Bool,
    },
    // ── LSP ──────────────────────────────────────────────────────────────────
    SettingDef {
        key: "lsp_enabled",
        label: "Enable LSP",
        description: "Enable Language Server Protocol support",
        category: "LSP",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "format_on_save",
        label: "Format on Save",
        description: "Automatically format the buffer via LSP before saving",
        category: "LSP",
        setting_type: SettingType::Bool,
    },
    // ── Terminal ─────────────────────────────────────────────────────────────
    SettingDef {
        key: "terminal_scrollback_lines",
        label: "Terminal Scrollback",
        description: "Maximum scrollback history lines in the integrated terminal",
        category: "Terminal",
        setting_type: SettingType::Integer {
            min: 100,
            max: 100000,
        },
    },
    // ── Plugins ──────────────────────────────────────────────────────────────
    SettingDef {
        key: "plugins_enabled",
        label: "Enable Plugins",
        description: "Enable the Lua plugin system (requires restart)",
        category: "Plugins",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "keymaps",
        label: "User Keymaps",
        description: "Press Enter to edit keymaps (one per line: mode keys :command)",
        category: "Plugins",
        setting_type: SettingType::BufferEditor,
    },
    // ── AI ────────────────────────────────────────────────────────────────────
    SettingDef {
        key: "ai_provider",
        label: "AI Provider",
        description: "AI backend: anthropic, openai (or compatible), or ollama (local)",
        category: "AI",
        setting_type: SettingType::Enum(&["anthropic", "openai", "ollama"]),
    },
    SettingDef {
        key: "ai_api_key",
        label: "API Key",
        description:
            "API key (or leave empty and set ANTHROPIC_API_KEY / OPENAI_API_KEY env var)",
        category: "AI",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "ai_model",
        label: "Model",
        description: "Model name override (leave empty to use the provider default)",
        category: "AI",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "ai_base_url",
        label: "Base URL",
        description: "Custom API endpoint URL (leave empty for provider default)",
        category: "AI",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "ai_completions",
        label: "Inline Completions",
        description: "Show AI ghost-text completions at the cursor in insert mode (Tab to accept, Alt+]/Alt+[ to cycle alternatives)",
        category: "AI",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "ai_attach_current_buffer",
        label: "Attach Current Buffer",
        description: "Attach the active buffer's path to every ACP prompt so the agent knows which file the user is looking at",
        category: "AI",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "acp_agent_command",
        label: "ACP Agent Command",
        description: "Command line of a live ACP agent to launch for the AI panel (e.g. \"claude-code-acp\"); empty falls back to the direct ai_provider/ai_api_key transport",
        category: "AI",
        setting_type: SettingType::StringVal,
    },
    SettingDef {
        key: "indent_guides",
        label: "Indent Guides",
        description: "Show vertical lines at each indentation level",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "minimap",
        label: "Minimap",
        description: "Show the code-overview minimap on the right edge of each editor pane",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "match_brackets",
        label: "Match Brackets",
        description: "Highlight matching bracket when cursor is on a bracket character",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "auto_pairs",
        label: "Auto Pairs",
        description: "Auto-close brackets and quotes in Insert mode",
        category: "Editor",
        setting_type: SettingType::Bool,
    },
    SettingDef {
        key: "hover_delay",
        label: "Hover Delay",
        description: "Mouse dwell delay (ms) before showing hover popups (0 = disabled)",
        category: "Editor",
        setting_type: SettingType::Integer { min: 0, max: 5000 },
    },
    // ── Extensions ────────────────────────────────────────────────────────────
    SettingDef {
        key: "extension_registries",
        label: "Registries",
        description: "Press Enter to edit registry URLs (one per line, # comments)",
        category: "Extensions",
        setting_type: SettingType::BufferEditor,
    },
    // ── Keybindings ────────────────────────────────────────────────────────
    SettingDef {
        key: "ctrl_f_action",
        label: "Ctrl+F Action",
        description: "What Ctrl+F does: 'find' (find/replace overlay) or 'page_down' (Vim default)",
        category: "Editor",
        setting_type: SettingType::Enum(&["find", "page_down"]),
    },
    // ── TUI ─────────────────────────────────────────────────────────────────
    SettingDef {
        key: "autohide_panels",
        label: "Auto-Hide Panels",
        description: "Hide toolbar and sidebar at startup; they appear on Ctrl-W l (TUI only)",
        category: "Appearance",
        setting_type: SettingType::Bool,
    },
];

/// Returns the unique ordered list of category names used in `SETTING_DEFS`.
pub fn setting_categories() -> Vec<&'static str> {
    let mut cats = Vec::new();
    for def in SETTING_DEFS {
        if !cats.contains(&def.category) {
            cats.push(def.category);
        }
    }
    cats
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
        // #1129: one value on every platform now that quadraui#1023
        // resolves the `"Monospace"` generic alias per-backend (see
        // `default_font_family`'s doc comment) — no more macOS special case.
        assert_eq!(settings.font_family, "Monospace");
        assert_eq!(settings.font_size, 14);
        // #700 item 6: VS Code draws indent guides by default; nothing
        // previously pinned this, so a future edit to
        // `default_indent_guides()` could silently flip it back off with no
        // test catching it.
        assert!(
            settings.indent_guides,
            "indent guides must default on, matching VS Code"
        );
    }

    // ── `minimap` option (#35) ───────────────────────────────────────────
    // The option is plumbed through eight separate sites; miss one and it
    // works from `:set` but not the settings UI (or vice versa). One test
    // per site so a regression names the site it broke.

    #[test]
    fn minimap_defaults_on() {
        assert!(
            Settings::default().minimap,
            "minimap must default on, matching VS Code"
        );
    }

    #[test]
    fn set_minimap_and_nominimap_both_parse() {
        let mut s = Settings::default();
        s.parse_set_option("nominimap").expect("nominimap");
        assert!(!s.minimap, "`:set nominimap` must turn the minimap off");
        s.parse_set_option("minimap").expect("minimap");
        assert!(s.minimap, "`:set minimap` must turn it back on");
    }

    #[test]
    fn set_minimap_query_form_reports_both_states() {
        let mut s = Settings::default();
        assert_eq!(s.parse_set_option("minimap?").unwrap(), "minimap");
        s.minimap = false;
        assert_eq!(s.parse_set_option("minimap?").unwrap(), "nominimap");
    }

    #[test]
    fn minimap_round_trips_through_get_set_by_key() {
        // The settings UI reads/writes by key string, not by field.
        let mut s = Settings::default();
        assert_eq!(s.get_value_str("minimap"), "true");
        s.set_value_str("minimap", "false").expect("set");
        assert!(!s.minimap);
        assert_eq!(s.get_value_str("minimap"), "false");
        s.set_value_str("minimap", "true").expect("set");
        assert!(s.minimap);
    }

    #[test]
    fn minimap_appears_in_the_settings_registry() {
        let def = SETTING_DEFS
            .iter()
            .find(|d| d.key == "minimap")
            .expect("`minimap` must appear in SETTING_DEFS so the settings UI lists it");
        assert_eq!(def.category, "Editor");
        assert!(matches!(def.setting_type, SettingType::Bool));
        assert!(!def.label.is_empty());
        assert!(!def.description.is_empty());
    }

    #[test]
    fn minimap_round_trips_through_the_settings_file() {
        let mut s = Settings::default();
        s.minimap = false;
        let json = serde_json::to_string(&s).expect("serialize");
        let back: Settings = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.minimap, "`minimap: false` must survive a save/load");

        // …and an older settings file with no `minimap` key at all must come
        // back with the default (on), not `false` from `bool::default()`.
        let legacy: Settings = serde_json::from_str("{}").expect("deserialize legacy");
        assert!(
            legacy.minimap,
            "a settings file predating #35 must default the minimap on"
        );
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

    // ── #1156: 'undolevels' / 'undofile' / 'undodir' ────────────────────────

    #[test]
    fn test_set_undolevels_and_alias() {
        let mut s = Settings::default();
        assert_eq!(s.undolevels, 1000);
        let msg = s.parse_set_option("undolevels=50").unwrap();
        assert_eq!(msg, "undolevels=50");
        assert_eq!(s.undolevels, 50);
        s.parse_set_option("ul=10").unwrap();
        assert_eq!(s.undolevels, 10);
        assert_eq!(s.parse_set_option("ul?").unwrap(), "undolevels=10");
    }

    #[test]
    fn test_set_undofile_and_alias_toggle_and_query() {
        let mut s = Settings::default();
        assert!(!s.undofile);
        s.parse_set_option("undofile").unwrap();
        assert!(s.undofile);
        assert_eq!(s.parse_set_option("udf?").unwrap(), "undofile");
        s.parse_set_option("noundofile").unwrap();
        assert!(!s.undofile);
        assert_eq!(s.parse_set_option("undofile?").unwrap(), "noundofile");
    }

    #[test]
    fn test_set_undodir_round_trips() {
        let mut s = Settings::default();
        assert_eq!(s.undodir, "");
        s.parse_set_option("undodir=/tmp/myundo").unwrap();
        assert_eq!(s.undodir, "/tmp/myundo");
        assert_eq!(s.parse_set_option("udir?").unwrap(), "undodir=/tmp/myundo");
    }

    #[test]
    fn undolevels_undofile_undodir_round_trip_through_get_set_by_key() {
        let mut s = Settings::default();
        s.set_value_str("undolevels", "42").unwrap();
        assert_eq!(s.get_value_str("undolevels"), "42");
        s.set_value_str("undofile", "true").unwrap();
        assert_eq!(s.get_value_str("undofile"), "true");
        s.set_value_str("undodir", "/tmp/u").unwrap();
        assert_eq!(s.get_value_str("undodir"), "/tmp/u");
    }

    #[test]
    fn undo_settings_appear_in_the_settings_registry() {
        assert!(SETTING_DEFS.iter().any(|d| d.key == "undolevels"));
        assert!(SETTING_DEFS.iter().any(|d| d.key == "undofile"));
        assert!(SETTING_DEFS.iter().any(|d| d.key == "undodir"));
    }

    /// #523's opt-in freshness setting: default off, round-trips through
    /// `get_value_str`/`set_value_str` (the Settings sidebar's contract,
    /// `explorer_visible_on_startup`'s sibling — no vim abbreviation to
    /// exercise here since there's no vim precedent for it), and appears in
    /// `SETTING_DEFS` so the sidebar actually lists it.
    #[test]
    fn board_tick_enabled_defaults_off_and_round_trips_via_settings_ui() {
        let mut s = Settings::default();
        assert!(!s.board_tick_enabled);
        assert_eq!(s.get_value_str("board_tick_enabled"), "false");

        s.set_value_str("board_tick_enabled", "true").unwrap();
        assert!(s.board_tick_enabled);
        assert_eq!(s.get_value_str("board_tick_enabled"), "true");

        assert!(SETTING_DEFS.iter().any(|d| d.key == "board_tick_enabled"));
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
    fn test_set_smarttab() {
        let mut s = Settings::default();
        assert!(s.smarttab);
        s.parse_set_option("nosmarttab").unwrap();
        assert!(!s.smarttab);
        s.parse_set_option("sta").unwrap();
        assert!(s.smarttab);
    }

    #[test]
    fn test_set_nrformats_default_and_nf_alias() {
        let mut s = Settings::default();
        assert_eq!(s.nrformats, vec!["bin".to_string(), "hex".to_string()]);
        let msg = s.parse_set_option("nf=bin,octal,hex").unwrap();
        assert_eq!(msg, "nf=bin,octal,hex");
        assert_eq!(
            s.nrformats,
            vec!["bin".to_string(), "octal".to_string(), "hex".to_string()]
        );
        let query = s.parse_set_option("nrformats?").unwrap();
        assert_eq!(query, "nrformats=bin,octal,hex");
    }

    #[test]
    fn test_nf_abbreviation_is_nrformats_not_nerdfonts() {
        // `"nf"` is Vim's real 'nrformats' abbreviation. vimcode's own
        // nerdfonts setting predates this option and had claimed "nf" for
        // itself; that alias was dropped in favor of the real-Vim meaning —
        // `:set nf=...` must go to `nrformats`, not toggle `nerdfonts`.
        let mut s = Settings::default();
        s.parse_set_option("nf=alpha").unwrap();
        assert_eq!(s.nrformats, vec!["alpha".to_string()]);
        assert!(s.use_nerd_fonts()); // untouched
    }

    #[test]
    fn test_set_unknown_option_is_error() {
        let mut s = Settings::default();
        assert!(s.parse_set_option("unknownoption").is_err());
        assert!(s.parse_set_option("nounknown").is_err());
        assert!(s.parse_set_option("foo=42").is_err());
    }

    // ── 'iskeyword' (#1191) ──────────────────────────────────────────────────

    #[test]
    fn test_iskeyword_default_and_isk_alias_readback() {
        let mut s = Settings::default();
        assert_eq!(s.iskeyword, "@,48-57,_,192-255");
        let query = s.parse_set_option("isk?").unwrap();
        assert_eq!(query, "iskeyword=@,48-57,_,192-255");
    }

    #[test]
    fn test_iskeyword_plain_assignment_replaces_value() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("iskeyword=@,_").unwrap();
        assert_eq!(msg, "iskeyword=@,_");
        assert_eq!(s.iskeyword, "@,_");
    }

    #[test]
    fn test_iskeyword_rejects_malformed_value() {
        // Before #1191, `iskeyword` was in `UNIMPLEMENTED_VALUE_OPTIONS`, so
        // *every* value (valid or not) was rejected with the same
        // "recognised but not implemented" message. Now a well-formed value
        // is accepted (see the other tests in this section) and only a
        // genuinely malformed one is rejected — with a different message.
        let mut s = Settings::default();
        let err = s.parse_set_option("iskeyword=abc-").unwrap_err();
        assert!(err.contains("Invalid value for iskeyword"), "{err}");
        // The stored value must be untouched by the rejected attempt.
        assert_eq!(s.iskeyword, "@,48-57,_,192-255");
    }

    #[test]
    fn test_iskeyword_plus_equals_appends() {
        // Before #1191 this failed outright — `iskeyword` (in any spelling,
        // any operator) was unconditionally "recognised but not
        // implemented", so `+=` could never even be attempted.
        let mut s = Settings::default();
        let msg = s.parse_set_option("iskeyword+=-").unwrap();
        assert_eq!(msg, "iskeyword=@,48-57,_,192-255,-");
        assert_eq!(s.iskeyword, "@,48-57,_,192-255,-");
    }

    #[test]
    fn test_iskeyword_caret_equals_prepends() {
        let mut s = Settings::default();
        s.parse_set_option("isk^=$").unwrap();
        assert_eq!(s.iskeyword, "$,@,48-57,_,192-255");
    }

    #[test]
    fn test_iskeyword_minus_equals_removes() {
        let mut s = Settings::default();
        s.parse_set_option("iskeyword-=_").unwrap();
        assert_eq!(s.iskeyword, "@,48-57,192-255");
    }

    #[test]
    fn test_iskeyword_is_keyword_char_default_is_unicode_aware() {
        // #1191: the underlying motion/regex bug this issue exists to fix —
        // before it, word-char classification for anything beyond the
        // hardcoded ASCII set was wrong for `\k`/`\K` (see vim_regex.rs's
        // `keyword_class_is_unicode_aware_with_the_default_iskeyword`).
        // `Settings::is_keyword_char` is the motion-side half of the same
        // fix: it must treat every Unicode letter as a keyword char under
        // the default spec, exactly like real Vim's `@` token promises.
        let s = Settings::default();
        assert!(s.is_keyword_char('a'));
        assert!(s.is_keyword_char('_'));
        assert!(s.is_keyword_char('5'));
        assert!(s.is_keyword_char('é'));
        assert!(s.is_keyword_char('Я'));
        assert!(s.is_keyword_char('北'));
        assert!(!s.is_keyword_char(' '));
        assert!(!s.is_keyword_char('.'));
        assert!(!s.is_keyword_char('-'));
    }

    #[test]
    fn test_iskeyword_custom_spec_add_hyphen_as_keyword() {
        let mut s = Settings::default();
        s.parse_set_option("iskeyword+=-").unwrap();
        assert!(s.is_keyword_char('-'));
        assert!(s.is_keyword_char('a'));
    }

    #[test]
    fn test_iskeyword_exclusion_removes_a_default_class() {
        // `^_` excludes underscore from the (still-present) `@` alphabetic
        // class — order-sensitive, matching real Vim.
        let mut s = Settings::default();
        s.parse_set_option("iskeyword=@,^_").unwrap();
        assert!(s.is_keyword_char('a'));
        assert!(!s.is_keyword_char('_'));
    }

    #[test]
    fn test_iskeyword_code_range_and_single_char_and_code() {
        let mut s = Settings::default();
        s.parse_set_option("iskeyword=65-90,35,36").unwrap();
        assert!(s.is_keyword_char('A')); // 65
        assert!(s.is_keyword_char('Z')); // 90
        assert!(!s.is_keyword_char('a')); // outside 65-90, lowercase not included
        assert!(s.is_keyword_char('#')); // 35
        assert!(s.is_keyword_char('$')); // 36
    }

    #[test]
    fn test_set_accepts_snake_case_aliases() {
        // The Settings panel displays snake_case keys, and the `:set` command
        // historically used vim-style packed names. Underscored aliases must
        // work so users can paste the panel key directly.
        let mut s = Settings::default();

        // Bool setting by its snake_case name.
        s.parse_set_option("window_status_line").unwrap();
        assert!(s.window_status_line);
        s.parse_set_option("nowindow_status_line").unwrap();
        assert!(!s.window_status_line);

        // Query form with snake_case.
        let msg = s.parse_set_option("window_status_line?").unwrap();
        assert_eq!(msg, "nowindowstatusline");

        // Toggle form with snake_case.
        s.window_status_line = false;
        s.parse_set_option("window_status_line!").unwrap();
        assert!(s.window_status_line);

        // #174 reproducer: :set status_line_above_terminal.
        s.parse_set_option("status_line_above_terminal").unwrap();
        assert!(s.status_line_above_terminal);
        s.parse_set_option("nostatus_line_above_terminal").unwrap();
        assert!(!s.status_line_above_terminal);

        // Settings whose canonical arm is already underscored still work as
        // exact matches (the fallback should not shadow them).
        s.parse_set_option("font_size=18").unwrap();
        assert_eq!(s.font_size, 18);

        // Unknown keys with underscores still error.
        assert!(s.parse_set_option("not_a_real_setting").is_err());
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

    // ── #1207: the last five `UNIMPLEMENTED_BOOL_OPTIONS` (#1190 tranche 2) ──
    //
    // RED against unfixed `develop`: before #1207, every one of `magic`,
    // `showmatch`, `linebreak`, `smartindent`, `cindent` was listed in
    // `UNIMPLEMENTED_BOOL_OPTIONS`, so `parse_set_option` rejected all of
    // them with "recognised but not implemented yet" instead of setting the
    // field — every assertion below that a field flips would have failed
    // (the `unwrap()` on `parse_set_option` would have panicked on the
    // `Err`).

    #[test]
    fn test_settings_magic_defaults_on_and_round_trips() {
        let mut s = Settings::default();
        assert!(s.magic, "'magic' defaults on, matching real Vim");
        let msg = s.parse_set_option("nomagic").unwrap();
        assert_eq!(msg, "nomagic");
        assert!(!s.magic);
        let msg = s.parse_set_option("magic").unwrap();
        assert_eq!(msg, "magic");
        assert!(s.magic);
        assert_eq!(s.parse_set_option("magic?").unwrap(), "magic");
    }

    #[test]
    fn test_settings_showmatch_defaults_off_and_round_trips_via_abbrev() {
        let mut s = Settings::default();
        assert!(!s.showmatch);
        let msg = s.parse_set_option("sm").unwrap();
        assert_eq!(msg, "sm");
        assert!(s.showmatch);
        assert_eq!(s.parse_set_option("showmatch?").unwrap(), "showmatch");
        let msg = s.parse_set_option("nosm").unwrap();
        assert_eq!(msg, "nosm");
        assert!(!s.showmatch);
    }

    #[test]
    fn test_settings_linebreak_defaults_off_and_round_trips_via_abbrev() {
        let mut s = Settings::default();
        assert!(!s.linebreak);
        let msg = s.parse_set_option("lbr").unwrap();
        assert_eq!(msg, "lbr");
        assert!(s.linebreak);
        assert_eq!(s.parse_set_option("linebreak?").unwrap(), "linebreak");
        let msg = s.parse_set_option("nolbr").unwrap();
        assert_eq!(msg, "nolbr");
        assert!(!s.linebreak);
    }

    #[test]
    fn test_settings_smartindent_defaults_off_and_round_trips_via_abbrev() {
        let mut s = Settings::default();
        assert!(!s.smartindent);
        let msg = s.parse_set_option("si").unwrap();
        assert_eq!(msg, "si");
        assert!(s.smartindent);
        assert_eq!(s.parse_set_option("smartindent?").unwrap(), "smartindent");
        let msg = s.parse_set_option("nosi").unwrap();
        assert_eq!(msg, "nosi");
        assert!(!s.smartindent);
    }

    #[test]
    fn test_settings_cindent_defaults_off_and_round_trips_via_abbrev() {
        let mut s = Settings::default();
        assert!(!s.cindent);
        let msg = s.parse_set_option("cin").unwrap();
        assert_eq!(msg, "cin");
        assert!(s.cindent);
        assert_eq!(s.parse_set_option("cindent?").unwrap(), "cindent");
        let msg = s.parse_set_option("nocin").unwrap();
        assert_eq!(msg, "nocin");
        assert!(!s.cindent);
    }

    #[test]
    fn test_unimplemented_bool_options_is_now_empty() {
        // #1207 implemented the last five entries #1190 left behind
        // (`magic`, `showmatch`, `linebreak`, `smartindent`, `cindent`).
        // Kept as `&[]` rather than removed — see the constant's own doc.
        assert!(UNIMPLEMENTED_BOOL_OPTIONS.is_empty());
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
        assert_eq!(pk.focus_explorer, "<C-S-e>");
        assert_eq!(pk.focus_search, "<C-S-f>");
        assert_eq!(pk.fuzzy_finder, "<C-p>");
        assert_eq!(pk.live_grep, "<C-S-g>");
        assert_eq!(pk.command_palette, "<C-S-p>");
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
        assert_eq!(pk.focus_explorer, "<C-S-e>");
    }

    #[test]
    fn test_panel_keys_in_settings_serde() {
        let json = r#"{ "panel_keys": { "live_grep": "<C-A-g>" } }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.panel_keys.live_grep, "<C-A-g>");
        assert_eq!(s.panel_keys.toggle_sidebar, "<C-b>");
    }

    // ── Mode-derived contested defaults (#800) ──────────────────────────────
    // `ctrl_f_action`, `auto_pairs`, `completion_keys.accept` derive their
    // default from `editor_mode` when unset (`None`), but an explicit
    // `Some(_)` override always wins — including across a later mode
    // switch. Each field gets: unset×Vim, unset×Vscode, explicit-survives-
    // mode-switch, plus the two acceptance scenarios (no config file /
    // `mode=vscode` with nothing else set).

    #[test]
    fn ctrl_f_action_unset_is_page_down_in_vim_mode() {
        let mut s = Settings::default();
        s.editor_mode = EditorMode::Vim;
        assert!(s.ctrl_f_action.is_none(), "must start unset");
        assert_eq!(s.ctrl_f_action(), "page_down");
    }

    #[test]
    fn ctrl_f_action_unset_is_find_in_vscode_mode() {
        let mut s = Settings::default();
        s.editor_mode = EditorMode::Vscode;
        assert!(s.ctrl_f_action.is_none(), "must start unset");
        assert_eq!(s.ctrl_f_action(), "find");
    }

    #[test]
    fn ctrl_f_action_explicit_override_survives_mode_switch() {
        let mut s = Settings::default();
        s.editor_mode = EditorMode::Vim;
        s.ctrl_f_action = Some("find".to_string());
        assert_eq!(s.ctrl_f_action(), "find");
        // A later mode switch must not clobber the explicit override.
        s.editor_mode = EditorMode::Vscode;
        assert_eq!(
            s.ctrl_f_action(),
            "find",
            "explicit override must survive a mode switch"
        );
    }

    #[test]
    fn auto_pairs_unset_is_false_in_vim_mode() {
        let mut s = Settings::default();
        s.editor_mode = EditorMode::Vim;
        assert!(s.auto_pairs.is_none(), "must start unset");
        assert!(!s.auto_pairs());
    }

    #[test]
    fn auto_pairs_unset_is_true_in_vscode_mode() {
        let mut s = Settings::default();
        s.editor_mode = EditorMode::Vscode;
        assert!(s.auto_pairs.is_none(), "must start unset");
        assert!(s.auto_pairs());
    }

    #[test]
    fn auto_pairs_explicit_override_survives_mode_switch() {
        // The scenario called out by #800: `:set mode=vim` + `:set
        // auto_pairs=true` keeps autopairs on; a later `:set mode=vscode`
        // must not clobber that explicit `true` (nor, symmetrically, an
        // explicit `false`).
        let mut s = Settings::default();
        s.editor_mode = EditorMode::Vim;
        s.auto_pairs = Some(true);
        assert!(s.auto_pairs());
        s.editor_mode = EditorMode::Vscode;
        assert!(
            s.auto_pairs(),
            "explicit auto_pairs=true must survive mode=vscode"
        );

        let mut s2 = Settings::default();
        s2.editor_mode = EditorMode::Vscode;
        s2.auto_pairs = Some(false);
        assert!(!s2.auto_pairs());
        s2.editor_mode = EditorMode::Vim;
        assert!(
            !s2.auto_pairs(),
            "explicit auto_pairs=false must survive mode=vim"
        );
    }

    #[test]
    fn completion_accept_unset_is_c_y_in_vim_mode() {
        let ck = CompletionKeys::default();
        assert!(ck.accept.is_none(), "must start unset");
        assert_eq!(ck.accept(EditorMode::Vim), "<C-y>");
        assert_ne!(
            ck.accept(EditorMode::Vim),
            "Tab",
            "Vim mode must not capture <Tab> for completion accept"
        );
    }

    #[test]
    fn completion_accept_unset_is_tab_in_vscode_mode() {
        let ck = CompletionKeys::default();
        assert!(ck.accept.is_none(), "must start unset");
        assert_eq!(ck.accept(EditorMode::Vscode), "Tab");
    }

    #[test]
    fn completion_accept_explicit_override_survives_mode_switch() {
        let mut ck = CompletionKeys::default();
        ck.accept = Some("<C-Space>".to_string());
        assert_eq!(ck.accept(EditorMode::Vim), "<C-Space>");
        assert_eq!(
            ck.accept(EditorMode::Vscode),
            "<C-Space>",
            "explicit override must survive a mode switch"
        );
    }

    #[test]
    fn no_config_file_defaults_are_strict_vim() {
        // Acceptance: with no config file (Settings::default(), nothing
        // ever set), mode=vim and all three contested defaults are the
        // strict Vim values.
        let s = Settings::default();
        assert_eq!(s.editor_mode, EditorMode::Vim);
        assert_eq!(s.ctrl_f_action(), "page_down");
        assert!(!s.auto_pairs());
        assert_ne!(
            s.completion_keys.accept(s.editor_mode),
            "Tab",
            "the completion popup must not eat <Tab> in default (Vim) mode"
        );
    }

    #[test]
    fn mode_vscode_with_nothing_else_set_reverts_to_ide_defaults() {
        // Acceptance: `mode=vscode` with nothing else set reverts all
        // three contested defaults to today's IDE values.
        let mut s = Settings::default();
        s.parse_set_option("mode=vscode").unwrap();
        assert_eq!(s.editor_mode, EditorMode::Vscode);
        assert_eq!(s.ctrl_f_action(), "find");
        assert!(s.auto_pairs());
        assert_eq!(s.completion_keys.accept(s.editor_mode), "Tab");
    }

    #[test]
    fn contested_fields_omitted_from_json_when_unset() {
        // Unset contested fields must not be baked into a fresh
        // settings.json — otherwise a fresh install would stop being
        // mode-reactive after the very first save/load round trip.
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("\"ctrl_f_action\""),
            "unset ctrl_f_action must be omitted from serialized settings"
        );
        assert!(
            !json.contains("\"auto_pairs\""),
            "unset auto_pairs must be omitted from serialized settings"
        );
        assert!(
            !json.contains("\"accept\""),
            "unset completion_keys.accept must be omitted from serialized settings"
        );
        assert!(
            !json.contains("\"use_nerd_fonts\""),
            "unset use_nerd_fonts must be omitted from serialized settings"
        );

        // Round-trip: deserializing that JSON must still resolve unset.
        let s2: Settings = serde_json::from_str(&json).unwrap();
        assert!(s2.ctrl_f_action.is_none());
        assert!(s2.auto_pairs.is_none());
        assert!(s2.completion_keys.accept.is_none());
        assert!(s2.use_nerd_fonts.is_none());
    }

    #[test]
    fn contested_fields_round_trip_when_explicitly_set() {
        let mut s = Settings::default();
        s.ctrl_f_action = Some("find".to_string());
        s.auto_pairs = Some(true);
        s.completion_keys.accept = Some("<C-y>".to_string());
        s.use_nerd_fonts = Some(false);

        let json = serde_json::to_string(&s).unwrap();
        let s2: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s2.ctrl_f_action, Some("find".to_string()));
        assert_eq!(s2.auto_pairs, Some(true));
        assert_eq!(s2.completion_keys.accept, Some("<C-y>".to_string()));
        assert_eq!(s2.use_nerd_fonts, Some(false));
    }

    // ── Backend-derived contested default: `use_nerd_fonts` (#999) ──────────
    // Unlike the mode-derived trio above, this one derives from which
    // *backend* is running (GUI vs TUI) rather than `editor_mode` — see the
    // field doc on `Settings::use_nerd_fonts`. `crate::icons::
    // is_gui_backend`/`set_gui_backend` is thread-local (it sits next to
    // `nerd_fonts_enabled`/`set_nerd_fonts`, for the same #618 reason), so
    // each test here saves and restores the ambient value to avoid leaking
    // into whatever other test Rust's runner schedules next on the same
    // worker thread.

    #[test]
    fn use_nerd_fonts_unset_is_true_on_gui_backend() {
        let prev = crate::icons::is_gui_backend();
        crate::icons::set_gui_backend(true);
        let s = Settings::default();
        assert!(s.use_nerd_fonts.is_none(), "must start unset");
        assert!(
            s.use_nerd_fonts(),
            "GUI bundles the icon font, so an unset setting must resolve true on every OS"
        );
        crate::icons::set_gui_backend(prev);
    }

    #[test]
    fn use_nerd_fonts_unset_is_conservative_guess_on_tui_backend() {
        let prev = crate::icons::is_gui_backend();
        crate::icons::set_gui_backend(false);
        let s = Settings::default();
        assert!(s.use_nerd_fonts.is_none(), "must start unset");
        assert_eq!(
            s.use_nerd_fonts(),
            !cfg!(target_os = "windows"),
            "TUI keeps the previous target_os-based guess"
        );
        crate::icons::set_gui_backend(prev);
    }

    #[test]
    fn use_nerd_fonts_explicit_override_survives_backend_change() {
        let prev = crate::icons::is_gui_backend();

        let mut s = Settings::default();
        s.use_nerd_fonts = Some(false);
        crate::icons::set_gui_backend(true);
        assert!(
            !s.use_nerd_fonts(),
            "explicit false must win even on a GUI backend that would default true"
        );

        let mut s2 = Settings::default();
        s2.use_nerd_fonts = Some(true);
        crate::icons::set_gui_backend(false);
        assert!(
            s2.use_nerd_fonts(),
            "explicit true must win even on a TUI backend that might default false"
        );

        crate::icons::set_gui_backend(prev);
    }

    /// #902: `menu_style` defaults to `Inherit`, matching VS Code's
    /// `window.menuStyle` default.
    #[test]
    fn menu_style_defaults_to_inherit() {
        assert_eq!(Settings::default().menu_style, MenuStyle::Inherit);
    }

    /// #902: `get_value_str`/`set_value_str` round-trip every `MenuStyle`
    /// variant, the same contract every other `SETTING_DEFS` `Enum` entry
    /// (e.g. `editor_mode`, `line_numbers`) already has to hold for the
    /// Settings sidebar UI to read/write it.
    #[test]
    fn menu_style_round_trips_through_value_str() {
        let mut s = Settings::default();
        for (text, variant) in [
            ("native", MenuStyle::Native),
            ("custom", MenuStyle::Custom),
            ("inherit", MenuStyle::Inherit),
        ] {
            s.set_value_str("menu_style", text).unwrap();
            assert_eq!(s.menu_style, variant);
            assert_eq!(s.get_value_str("menu_style"), text);
        }

        assert!(s.set_value_str("menu_style", "bogus").is_err());
    }

    // ── #1206: generic `+=`/`-=`/`^=` strip ──────────────────────────────

    /// RED against unfixed `develop`: before #1206, only `'iskeyword'` had
    /// its `+=`/`-=`/`^=` suffix stripped before the name lookup — every
    /// other option's raw name still carried the operator into
    /// `set_value_option`'s exact-name match, so `whichwrap+` (not
    /// `whichwrap`) is what got looked up and reported as unknown.
    #[test]
    fn plus_equals_on_a_list_option_other_than_iskeyword_no_longer_names_the_option_wrong() {
        let mut s = Settings::default();
        let err = s.parse_set_option("clipboard+=unnamed").unwrap_err();
        assert_eq!(
            err, "Option 'clipboard' is recognised but not implemented yet",
            "must name the real option 'clipboard', not 'clipboard+'"
        );
    }

    #[test]
    fn plus_equals_on_whichwrap_appends_to_the_list() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("whichwrap+=h,l").unwrap();
        assert_eq!(msg, "whichwrap=b,s,h,l");
        assert_eq!(s.whichwrap, "b,s,h,l");
    }

    #[test]
    fn minus_equals_on_whichwrap_removes_from_the_list() {
        let mut s = Settings::default();
        s.parse_set_option("ww-=b").unwrap();
        assert_eq!(s.whichwrap, "s");
    }

    #[test]
    fn caret_equals_on_backspace_prepends_to_the_list() {
        let mut s = Settings::default();
        s.parse_set_option("bs^=nostop").unwrap();
        assert_eq!(s.backspace, "nostop,indent,eol,start");
    }

    /// Numeric options also take `+=`/`-=`/`^=` in real Vim (add/subtract/
    /// multiply) — #1206 generalised the same strip loop to cover them too,
    /// not just list options.
    #[test]
    fn plus_equals_on_a_numeric_option_adds() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("scrolloff+=3").unwrap();
        assert_eq!(msg, "scrolloff=3");
        assert_eq!(s.scrolloff, 3);
        s.parse_set_option("so+=2").unwrap();
        assert_eq!(s.scrolloff, 5);
    }

    #[test]
    fn minus_equals_on_a_numeric_option_subtracts() {
        let mut s = Settings::default();
        s.scrolljump = 5;
        s.parse_set_option("scrolljump-=2").unwrap();
        assert_eq!(s.scrolljump, 3);
    }

    #[test]
    fn caret_equals_on_a_numeric_option_multiplies() {
        let mut s = Settings::default();
        s.parse_set_option("timeoutlen=100").unwrap();
        s.parse_set_option("tm^=3").unwrap();
        assert_eq!(s.timeoutlen, 300);
    }

    /// A pre-existing list option #1206 didn't add (`'nrformats'`) must
    /// also get real list append/remove, not silent overwrite — see the
    /// `current_list_value` doc comment for why this would otherwise be a
    /// worse regression than the bug #1206 fixes.
    #[test]
    fn plus_equals_on_a_pre_existing_list_option_appends_not_overwrites() {
        let mut s = Settings::default();
        assert_eq!(s.nrformats, vec!["bin", "hex"]);
        s.parse_set_option("nrformats+=octal").unwrap();
        assert_eq!(s.nrformats, vec!["bin", "hex", "octal"]);
    }

    // ── #1206: 'whichwrap' ───────────────────────────────────────────────

    #[test]
    fn whichwrap_default_matches_neovim() {
        assert_eq!(Settings::default().whichwrap, "b,s");
    }

    #[test]
    fn whichwrap_rejects_bad_token() {
        let mut s = Settings::default();
        let err = s.parse_set_option("whichwrap=x").unwrap_err();
        assert!(err.contains("Invalid value for whichwrap"), "{err}");
    }

    #[test]
    fn whichwrap_accepts_every_real_token() {
        let mut s = Settings::default();
        s.parse_set_option("ww=b,s,h,l,<,>,~,[,]").unwrap();
        assert_eq!(s.whichwrap, "b,s,h,l,<,>,~,[,]");
    }

    // ── #1206: 'backspace' ───────────────────────────────────────────────

    #[test]
    fn backspace_default_matches_neovim() {
        assert_eq!(Settings::default().backspace, "indent,eol,start");
    }

    #[test]
    fn backspace_rejects_bad_token() {
        let mut s = Settings::default();
        let err = s.parse_set_option("backspace=bogus").unwrap_err();
        assert!(err.contains("Invalid value for backspace"), "{err}");
    }

    #[test]
    fn backspace_legacy_numeric_values_expand_to_the_list_form() {
        let mut s = Settings::default();
        s.parse_set_option("bs=0").unwrap();
        assert_eq!(s.backspace, "");
        s.parse_set_option("bs=1").unwrap();
        assert_eq!(s.backspace, "indent,eol");
        s.parse_set_option("bs=2").unwrap();
        assert_eq!(s.backspace, "indent,eol,start");
        s.parse_set_option("bs=3").unwrap();
        assert_eq!(s.backspace, "indent,eol,nostop");
    }

    // ── #1206: 'timeoutlen' ──────────────────────────────────────────────

    #[test]
    fn timeoutlen_default_matches_neovim() {
        assert_eq!(Settings::default().timeoutlen, 1000);
    }

    #[test]
    fn timeoutlen_set_and_query() {
        let mut s = Settings::default();
        let msg = s.parse_set_option("timeoutlen=250").unwrap();
        assert_eq!(msg, "timeoutlen=250");
        assert_eq!(s.parse_set_option("tm?").unwrap(), "timeoutlen=250");
    }

    // ── #1206: 'wildmode' ────────────────────────────────────────────────

    #[test]
    fn wildmode_default_matches_neovim() {
        assert_eq!(Settings::default().wildmode, "full");
    }

    #[test]
    fn wildmode_accepts_documented_stage_grammar() {
        let mut s = Settings::default();
        s.parse_set_option("wildmode=longest:full,full").unwrap();
        assert_eq!(s.wildmode, "longest:full,full");
    }

    #[test]
    fn wildmode_rejects_bad_token() {
        let mut s = Settings::default();
        let err = s.parse_set_option("wildmode=bogus").unwrap_err();
        assert!(err.contains("Invalid value for wildmode"), "{err}");
    }

    // ── #1206: 'laststatus' ──────────────────────────────────────────────

    #[test]
    fn laststatus_default_matches_neovim() {
        assert_eq!(Settings::default().laststatus, 2);
    }

    #[test]
    fn laststatus_rejects_out_of_range() {
        let mut s = Settings::default();
        let err = s.parse_set_option("laststatus=4").unwrap_err();
        assert!(err.contains("Invalid value for laststatus"), "{err}");
    }

    #[test]
    fn laststatus_accepts_0_through_3() {
        let mut s = Settings::default();
        for n in 0..=3 {
            s.parse_set_option(&format!("ls={n}")).unwrap();
            assert_eq!(s.laststatus, n);
        }
    }

    // ── #1206: 'sidescrolloff' / 'scrolljump' ────────────────────────────

    #[test]
    fn sidescrolloff_and_scrolljump_defaults_match_neovim() {
        let s = Settings::default();
        assert_eq!(s.sidescrolloff, 0);
        assert_eq!(s.scrolljump, 1);
    }

    #[test]
    fn sidescrolloff_set_and_query() {
        let mut s = Settings::default();
        s.parse_set_option("siso=8").unwrap();
        assert_eq!(s.sidescrolloff, 8);
        assert_eq!(
            s.parse_set_option("sidescrolloff?").unwrap(),
            "sidescrolloff=8"
        );
    }

    #[test]
    fn scrolljump_accepts_zero() {
        // Neovim itself accepts `scrolljump=0` (verified against
        // `nvim --headless`) — must not be rejected here.
        let mut s = Settings::default();
        s.parse_set_option("sj=0").unwrap();
        assert_eq!(s.scrolljump, 0);
    }

    // ── #1206: 'listchars' ───────────────────────────────────────────────

    #[test]
    fn listchars_default_matches_neovim() {
        assert_eq!(Settings::default().listchars, "tab:> ,trail:-,nbsp:+");
    }

    #[test]
    fn listchars_rejects_unknown_item() {
        let mut s = Settings::default();
        let err = s.parse_set_option("listchars=bogus:x").unwrap_err();
        assert!(err.contains("Invalid value for listchars"), "{err}");
    }

    #[test]
    fn listchars_rejects_wrong_length_value() {
        let mut s = Settings::default();
        assert!(s.parse_set_option("lcs=eol:$$").is_err());
        assert!(s.parse_set_option("lcs=tab:x").is_err());
        assert!(s.parse_set_option("lcs=tab:wxyz").is_err());
    }

    #[test]
    fn listchars_accepts_three_char_tab_and_unwired_real_items() {
        let mut s = Settings::default();
        // 'multispace'/'lead'/etc are real Vim items with no vimcode glyph
        // yet — must be accepted, not rejected as a typo.
        s.parse_set_option("lcs=tab:<->,multispace:-+,lead:.,conceal:x")
            .unwrap();
        assert_eq!(s.listchars, "tab:<->,multispace:-+,lead:.,conceal:x");
    }
}
