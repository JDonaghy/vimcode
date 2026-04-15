//! Lua plugin system for VimCode.
//!
//! Plugins are `.lua` files (or directories with `init.lua`) placed in
//! `~/.config/vimcode/plugins/`. They are loaded in alphabetical order at
//! startup and have access to the `vimcode.*` Lua API.
//!
//! The plugin system is intentionally unrestricted (Neovim-style): plugins have
//! full access to file I/O, OS processes, and the network. Users are
//! responsible for trusting the plugins they install.

use mlua::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A request from Lua to open a scratch buffer with content.
pub struct ScratchBufferRequest {
    pub name: String,
    pub content: String,
    pub read_only: bool,
    pub filetype: Option<String>,
    /// None = current window, "vertical" or "horizontal" split.
    pub split: Option<String>,
}

/// A request from Lua to run a shell command in a background thread.
pub struct AsyncShellRequest {
    pub command: String,
    pub callback_event: String,
    pub stdin: Option<String>,
    pub cwd: Option<PathBuf>,
}

use super::git;

// ─── Extension panel types ──────────────────────────────────────────────────

/// Registration info for an extension-provided sidebar panel.
#[derive(Debug, Clone)]
pub struct PanelRegistration {
    pub name: String,
    pub title: String,
    pub icon: char,
    /// ASCII/Unicode fallback icon for when Nerd Fonts are disabled.
    /// If `None` and nerd fonts are off, the first letter of `title` is used.
    pub fallback_icon: Option<char>,
    pub sections: Vec<String>,
}

impl PanelRegistration {
    /// Return the icon to display, respecting the global `use_nerd_fonts` flag.
    pub fn resolved_icon(&self) -> char {
        if crate::icons::nerd_fonts_enabled() {
            self.icon
        } else {
            self.fallback_icon
                .unwrap_or_else(|| self.title.chars().next().unwrap_or('?'))
        }
    }
}

/// A single item in an extension panel section.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtPanelItem {
    pub text: String,
    pub hint: String,
    pub icon: String,
    pub indent: u8,
    pub style: ExtPanelStyle,
    pub id: String,
    // ── Tree node fields ──
    /// Whether this item can be expanded/collapsed (shows chevron).
    pub expandable: bool,
    /// Current expand/collapse state (only meaningful when `expandable` is true).
    pub expanded: bool,
    /// Parent item ID (empty = top-level). Children are hidden when parent is collapsed.
    pub parent_id: String,
    // ── Rich layout fields ──
    /// Action buttons rendered as clickable badges on the right side.
    pub actions: Vec<ExtPanelAction>,
    /// Colored badge/tag pills displayed inline with the item text.
    pub badges: Vec<ExtPanelBadge>,
    /// When true, renders as a horizontal divider line instead of text.
    pub is_separator: bool,
}

/// An action button on a panel item (rendered as a clickable badge).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtPanelAction {
    /// Display label (e.g. "Stage", "Discard").
    pub label: String,
    /// Key shortcut that triggers this action when the item is selected.
    pub key: String,
}

/// A colored badge/tag pill displayed on a panel item.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExtPanelBadge {
    /// Badge text (e.g. "main", "3 ahead").
    pub text: String,
    /// CSS-style color name or hex (e.g. "green", "#4ec9b0").
    pub color: String,
}

impl Default for ExtPanelItem {
    fn default() -> Self {
        Self {
            text: String::new(),
            hint: String::new(),
            icon: String::new(),
            indent: 0,
            style: ExtPanelStyle::Normal,
            id: String::new(),
            expandable: false,
            expanded: false,
            parent_id: String::new(),
            actions: Vec::new(),
            badges: Vec::new(),
            is_separator: false,
        }
    }
}

/// Visual style for an extension panel item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtPanelStyle {
    #[default]
    Normal,
    Header,
    Dim,
    Accent,
}

// ─── Public types ─────────────────────────────────────────────────────────────

/// Manages all loaded Lua plugins and their registered callbacks.
pub struct PluginManager {
    lua: Lua,
    /// Metadata for every plugin that was discovered (including disabled/errored ones).
    pub plugins: Vec<LoadedPlugin>,
    /// Registered `:Command` → Lua function.
    commands: HashMap<String, LuaRegistryKey>,
    /// Registered `(mode, key)` → Lua function.
    keymaps: HashMap<(String, String), LuaRegistryKey>,
    /// Registered `event` → list of Lua functions.
    hooks: HashMap<String, Vec<LuaRegistryKey>>,
    /// Extension panel registrations harvested from plugin scripts.
    pub panels: HashMap<String, PanelRegistration>,
    /// Extension panel help bindings harvested from plugin scripts.
    pub help_bindings: HashMap<String, Vec<(String, String)>>,
}

/// Metadata about a single plugin file / directory.
pub struct LoadedPlugin {
    pub name: String,
    #[allow(dead_code)]
    pub path: PathBuf,
    /// False when the plugin was skipped because it appears in `disabled_plugins`.
    pub enabled: bool,
    /// Non-None when the plugin produced an error at load time.
    pub error: Option<String>,
}

/// Data passed into (and modified by) Lua callbacks during a plugin call.
///
/// `cwd`, `buf_path`, `buf_lines` are inputs; the output fields start empty.
#[derive(Default)]
pub struct PluginCallContext {
    // ── Inputs ──────────────────────────────────────────────────────────────
    pub cwd: String,
    pub buf_path: Option<String>,
    pub buf_lines: Vec<String>,
    /// Cursor line (1-indexed) in the active buffer.
    pub cursor_line: usize,
    /// Cursor column (1-indexed) in the active buffer.
    pub cursor_col: usize,
    /// Filesystem path for the current working directory (for git operations).
    pub cwd_path: Option<PathBuf>,
    /// Filesystem path for the active buffer (for git operations).
    pub buf_path_os: Option<PathBuf>,
    /// Whether the active buffer has unsaved changes.
    /// When false, `vimcode.git.blame_line()` skips piping the in-memory
    /// contents via `--contents -` and uses the faster committed-content path.
    pub buf_dirty: bool,
    /// Current mode name (e.g. "Normal", "Insert", "Visual").
    pub mode_name: String,
    /// Snapshot of registers: `char -> (content, is_linewise)`.
    pub registers_snapshot: HashMap<char, (String, bool)>,
    /// Snapshot of marks for the active buffer: `char -> (line, col)` (1-indexed).
    pub marks_snapshot: HashMap<char, (usize, usize)>,
    /// Filetype / language ID of the active buffer (e.g. "rust", "python").
    pub filetype: String,
    /// Snapshot of all settings as key-value string pairs.
    pub settings_snapshot: HashMap<String, String>,
    /// Snapshot of panel input field texts: panel_name → current text.
    pub panel_input_snapshot: HashMap<String, String>,
    // ── Outputs written by callbacks ────────────────────────────────────────
    pub message: Option<String>,
    /// `(0-based line index, new text)` — applied by the engine after the call.
    pub set_lines: Vec<(usize, String)>,
    /// VimCode commands to execute after the call (e.g. `"w"` to save).
    pub run_commands: Vec<String>,
    /// Inline annotations to set: `(1-indexed line, annotation text)`.
    pub annotate_lines: Vec<(usize, String)>,
    /// When true, all existing line annotations are cleared first.
    pub clear_annotations: bool,
    /// Requests to run shell commands in background threads.
    pub async_shell_requests: Vec<AsyncShellRequest>,
    /// Set cursor position: `(line, col)` (1-indexed). Applied with bounds clamping.
    pub set_cursor: Option<(usize, usize)>,
    /// Settings to apply: `(key, value)` pairs processed via `Settings::set_value_str()`.
    pub set_settings: Vec<(String, String)>,
    /// Lines to insert: `(1-indexed line, text)`. Inserted before the given line.
    pub insert_lines: Vec<(usize, String)>,
    /// Lines to delete: 1-indexed line numbers (processed in reverse order).
    pub delete_lines: Vec<usize>,
    /// Registers to set: `(char, content, is_linewise)`.
    pub set_registers: Vec<(char, String, bool)>,
    /// Comment style overrides: `(lang_id, line, block_open, block_close)`.
    pub comment_style_overrides: Vec<(String, String, String, String)>,
    /// Scratch buffers to open after the callback returns.
    pub scratch_buffers: Vec<ScratchBufferRequest>,
    /// Extension panel registrations collected during script init.
    pub panel_registrations: Vec<PanelRegistration>,
    /// Extension panel item updates: `(panel_name, section_name, items)`.
    pub panel_set_items: Vec<(String, String, Vec<ExtPanelItem>)>,
    /// Panel hover content registrations: `(panel_name, item_id, markdown)`.
    pub panel_hover_entries: Vec<(String, String, String)>,
    /// Panel help bindings: `(panel_name, [(key, description)])`.
    pub panel_help_entries: Vec<(String, Vec<(String, String)>)>,
    /// Panel input field text values to set: `(panel_name, text)`.
    pub panel_input_values: Vec<(String, String)>,
    /// Editor hover content registrations: `(0-indexed line, markdown)`.
    pub editor_hover_entries: Vec<(usize, String)>,
    /// Panel reveal request: `(panel_name, section_name, item_id)`.
    /// Causes the sidebar to switch to the named panel and highlight the item.
    pub panel_reveal_request: Option<(String, String, String)>,
    /// Commit file diff request: `(hash, rel_path)`.
    /// Opens a side-by-side diff of the file at `hash` vs `hash~1`.
    pub commit_file_diff: Option<(String, String)>,
    /// URLs to open in the default browser.
    pub open_urls: Vec<String>,
    /// Key sequences to feed into the engine (parsed like `send_keys`).
    pub feedkeys_sequences: Vec<String>,
    /// Range-based line replacements: `(start_0idx, end_0idx, replacement_lines)`.
    /// Replaces lines `[start, end)` with the given lines (Neovim-compatible).
    pub set_lines_range: Vec<(usize, usize, Vec<String>)>,
}

// ─── Internal registration accumulator ───────────────────────────────────────

/// Stored in Lua app_data during a plugin's top-level execution so that
/// `vimcode.command()`, `vimcode.on()`, `vimcode.keymap()` calls can
/// accumulate registrations that the engine harvests afterwards.
#[derive(Default)]
struct PluginRegistrations {
    commands: HashMap<String, LuaRegistryKey>,
    keymaps: HashMap<(String, String), LuaRegistryKey>,
    hooks: HashMap<String, Vec<LuaRegistryKey>>,
    panels: Vec<PanelRegistration>,
    help_bindings: Vec<(String, Vec<(String, String)>)>,
}

// ─── PluginManager implementation ────────────────────────────────────────────

impl PluginManager {
    /// Create a new `PluginManager` and set up the `vimcode.*` Lua API.
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        Self::setup_vimcode_api(&lua)?;
        Ok(Self {
            lua,
            plugins: Vec::new(),
            commands: HashMap::new(),
            keymaps: HashMap::new(),
            hooks: HashMap::new(),
            panels: HashMap::new(),
            help_bindings: HashMap::new(),
        })
    }

    /// Scan `dir` for `.lua` files and `*/init.lua` directories, load each one.
    /// Files whose stem appears in `disabled` are recorded but not executed.
    pub fn load_plugins_dir(&mut self, dir: &Path, disabled: &[String]) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();

        for path in paths {
            if path.extension().map(|e| e == "lua").unwrap_or(false) {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let enabled = !disabled.iter().any(|d| d == &name);
                self.load_one_plugin(&path, &name, enabled);
            } else if path.is_dir() {
                let init = path.join("init.lua");
                if init.exists() {
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let enabled = !disabled.iter().any(|d| d == &name);
                    self.load_one_plugin(&init, &name, enabled);
                }
            }
        }
    }

    /// Execute a single plugin file and harvest its registrations.
    fn load_one_plugin(&mut self, path: &Path, name: &str, enabled: bool) {
        if !enabled {
            self.plugins.push(LoadedPlugin {
                name: name.to_string(),
                path: path.to_path_buf(),
                enabled: false,
                error: None,
            });
            return;
        }

        let code = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                self.plugins.push(LoadedPlugin {
                    name: name.to_string(),
                    path: path.to_path_buf(),
                    enabled: true,
                    error: Some(format!("read error: {e}")),
                });
                return;
            }
        };

        // Install registration accumulator so vimcode.command/on/keymap can write to it.
        self.lua.set_app_data(PluginRegistrations::default());

        let result = self.lua.load(&code).set_name(name).exec();

        // Harvest whatever was registered before any error.
        if let Some(reg) = self.lua.remove_app_data::<PluginRegistrations>() {
            for (cmd_name, key) in reg.commands {
                self.commands.insert(cmd_name, key);
            }
            for (km, key) in reg.keymaps {
                self.keymaps.insert(km, key);
            }
            for (event, keys) in reg.hooks {
                self.hooks.entry(event).or_default().extend(keys);
            }
            for panel in reg.panels {
                self.panels.insert(panel.name.clone(), panel);
            }
            for (panel_name, bindings) in reg.help_bindings {
                self.help_bindings.insert(panel_name, bindings);
            }
        }

        let error = result.err().map(|e| e.to_string());
        self.plugins.push(LoadedPlugin {
            name: name.to_string(),
            path: path.to_path_buf(),
            enabled: true,
            error,
        });
    }

    // ─── Dispatch helpers ──────────────────────────────────────────────────

    /// Execute a registered `:Command`. Returns `(found, updated_context)`.
    pub fn call_command(
        &self,
        name: &str,
        args: &str,
        ctx: PluginCallContext,
    ) -> (bool, PluginCallContext) {
        let Some(key) = self.commands.get(name) else {
            return (false, ctx);
        };
        self.lua.set_app_data(ctx);
        if let Ok(f) = self.lua.registry_value::<LuaFunction>(key) {
            let _ = f.call::<String, ()>(args.to_string());
        }
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (true, ctx)
    }

    /// Return true if at least one hook is registered for `event`.
    /// Use this to skip expensive context construction when no hooks exist.
    pub fn has_event_hooks(&self, event: &str) -> bool {
        self.hooks
            .get(event)
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    }

    /// Fire all hooks registered for `event`. Returns the updated context.
    pub fn call_event(&self, event: &str, arg: &str, ctx: PluginCallContext) -> PluginCallContext {
        let Some(hooks) = self.hooks.get(event) else {
            return ctx;
        };
        if hooks.is_empty() {
            return ctx;
        }
        self.lua.set_app_data(ctx);
        for key in hooks {
            if let Ok(f) = self.lua.registry_value::<LuaFunction>(key) {
                let _ = f.call::<String, ()>(arg.to_string());
            }
        }
        self.lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default()
    }

    /// Check if any registered keymap for `mode` starts with `prefix`.
    pub fn has_keymap_prefix(&self, mode: &str, prefix: &str) -> bool {
        self.keymaps
            .keys()
            .any(|(m, k)| m == mode && k.starts_with(prefix))
    }

    /// Execute a registered keymap for `(mode, key)`. Returns `(found, updated_context)`.
    pub fn call_keymap(
        &self,
        mode: &str,
        key: &str,
        ctx: PluginCallContext,
    ) -> (bool, PluginCallContext) {
        let Some(reg_key) = self.keymaps.get(&(mode.to_string(), key.to_string())) else {
            return (false, ctx);
        };
        self.lua.set_app_data(ctx);
        if let Ok(f) = self.lua.registry_value::<LuaFunction>(reg_key) {
            let _ = f.call::<(), ()>(());
        }
        let ctx = self
            .lua
            .remove_app_data::<PluginCallContext>()
            .unwrap_or_default();
        (true, ctx)
    }

    // ─── Lua API setup ─────────────────────────────────────────────────────

    /// Install the `vimcode.*` global table into the Lua state.
    ///
    /// Registration callbacks (`vimcode.command`, `vimcode.on`, `vimcode.keymap`)
    /// write into `PluginRegistrations` stored in app_data during loading.
    ///
    /// Runtime callbacks (`vimcode.message`, `vimcode.buf.*`, etc.) read/write
    /// `PluginCallContext` stored in app_data during dispatch.
    fn setup_vimcode_api(lua: &Lua) -> LuaResult<()> {
        let vimcode = lua.create_table()?;

        // ── Registration callbacks ──────────────────────────────────────────

        // vimcode.on(event, fn)
        vimcode.set(
            "on",
            lua.create_function(|lua, (event, f): (String, LuaFunction)| {
                let key = lua.create_registry_value(f)?;
                if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.hooks.entry(event).or_default().push(key);
                }
                Ok(())
            })?,
        )?;

        // vimcode.command(name, fn)
        vimcode.set(
            "command",
            lua.create_function(|lua, (name, f): (String, LuaFunction)| {
                let key = lua.create_registry_value(f)?;
                if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.commands.insert(name, key);
                }
                Ok(())
            })?,
        )?;

        // vimcode.keymap(mode, key, fn)
        vimcode.set(
            "keymap",
            lua.create_function(|lua, (mode, k, f): (String, String, LuaFunction)| {
                let key = lua.create_registry_value(f)?;
                if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.keymaps.insert((mode, k), key);
                }
                Ok(())
            })?,
        )?;

        // ── Runtime callbacks ───────────────────────────────────────────────

        // vimcode.message(text)
        vimcode.set(
            "message",
            lua.create_function(|lua, text: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.message = Some(text);
                }
                Ok(())
            })?,
        )?;

        // vimcode.cwd()
        vimcode.set(
            "cwd",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.cwd.clone())
                    .unwrap_or_default())
            })?,
        )?;

        // vimcode.command_run(cmd)
        vimcode.set(
            "command_run",
            lua.create_function(|lua, cmd: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.run_commands.push(cmd);
                }
                Ok(())
            })?,
        )?;

        // vimcode.feedkeys(keys) — inject keystrokes into the engine.
        // Uses the same notation as Neovim: "dw", "<Esc>", "<C-a>", "<CR>".
        vimcode.set(
            "feedkeys",
            lua.create_function(|lua, keys: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.feedkeys_sequences.push(keys);
                }
                Ok(())
            })?,
        )?;

        // vimcode.eval(expr) — evaluate simple Vim-like expressions.
        // Supports: @a (register), &option (setting), line('.'), col('.').
        vimcode.set(
            "eval",
            lua.create_function(|lua, expr: String| -> LuaResult<LuaValue> {
                let ctx = lua.app_data_ref::<PluginCallContext>();
                let ctx = match ctx {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                let expr = expr.trim();
                if let Some(reg_char) = expr.strip_prefix('@') {
                    // Register contents: @a, @", @+, etc.
                    let ch = reg_char.chars().next().unwrap_or('"');
                    Ok(ctx
                        .registers_snapshot
                        .get(&ch)
                        .map(|(content, _)| LuaValue::String(lua.create_string(content).unwrap()))
                        .unwrap_or(LuaValue::Nil))
                } else if let Some(opt_name) = expr.strip_prefix('&') {
                    // Option value: &tabstop, &shiftwidth, etc.
                    Ok(ctx
                        .settings_snapshot
                        .get(opt_name)
                        .map(|v| LuaValue::String(lua.create_string(v).unwrap()))
                        .unwrap_or(LuaValue::Nil))
                } else if expr == "line('.')" {
                    Ok(LuaValue::Integer(ctx.cursor_line as i64))
                } else if expr == "col('.')" {
                    Ok(LuaValue::Integer(ctx.cursor_col as i64))
                } else if expr == "line('$')" {
                    Ok(LuaValue::Integer(ctx.buf_lines.len() as i64))
                } else if expr == "mode()" {
                    Ok(LuaValue::String(lua.create_string(&ctx.mode_name).unwrap()))
                } else {
                    Ok(LuaValue::Nil)
                }
            })?,
        )?;

        // vimcode.open_url(url)
        vimcode.set(
            "open_url",
            lua.create_function(|lua, url: String| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.open_urls.push(url);
                }
                Ok(())
            })?,
        )?;

        // vimcode.async_shell(command, callback_event [, options_table])
        // options: { stdin = "...", cwd = "..." }
        vimcode.set(
            "async_shell",
            lua.create_function(|lua, args: LuaMultiValue| {
                let command: String = args
                    .get(0)
                    .and_then(|v| match v {
                        LuaValue::String(s) => Some(s.to_str().ok()?.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let callback_event: String = args
                    .get(1)
                    .and_then(|v| match v {
                        LuaValue::String(s) => Some(s.to_str().ok()?.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                if command.is_empty() || callback_event.is_empty() {
                    return Ok(());
                }
                let mut stdin = None;
                let mut cwd = None;
                if let Some(LuaValue::Table(opts)) = args.get(2) {
                    if let Ok(s) = opts.get::<_, String>("stdin") {
                        stdin = Some(s);
                    }
                    if let Ok(s) = opts.get::<_, String>("cwd") {
                        cwd = Some(PathBuf::from(s));
                    }
                }
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.async_shell_requests.push(AsyncShellRequest {
                        command,
                        callback_event,
                        stdin,
                        cwd,
                    });
                }
                Ok(())
            })?,
        )?;

        // ── vimcode.buf subtable ────────────────────────────────────────────
        let buf = lua.create_table()?;

        // vimcode.buf.lines() → table of strings (1-indexed)
        buf.set(
            "lines",
            lua.create_function(|lua, ()| {
                let lines = lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.buf_lines.clone())
                    .unwrap_or_default();
                let t = lua.create_table()?;
                for (i, line) in lines.into_iter().enumerate() {
                    t.set(i + 1, line)?;
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buf.line(n) → string or nil (1-indexed)
        buf.set(
            "line",
            lua.create_function(|lua, n: usize| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.buf_lines.get(n.saturating_sub(1)).cloned()))
            })?,
        )?;

        // vimcode.buf.set_line(n, text) (1-indexed)
        buf.set(
            "set_line",
            lua.create_function(|lua, (n, text): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.set_lines.push((n - 1, text));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.get_lines(start, end) → table of strings (0-indexed, exclusive end)
        // Neovim-compatible: nvim_buf_get_lines(0, start, end, false)
        buf.set(
            "get_lines",
            lua.create_function(|lua, (start, end): (i64, i64)| {
                let ctx = lua.app_data_ref::<PluginCallContext>();
                let t = lua.create_table()?;
                if let Some(ctx) = ctx {
                    let len = ctx.buf_lines.len() as i64;
                    // Negative indices count from end (Neovim convention)
                    let s = if start < 0 {
                        (len + start).max(0) as usize
                    } else {
                        (start as usize).min(len as usize)
                    };
                    let e = if end < 0 {
                        (len + end).max(0) as usize
                    } else {
                        (end as usize).min(len as usize)
                    };
                    for (i, line) in ctx.buf_lines[s..e].iter().enumerate() {
                        t.set(i + 1, line.as_str())?;
                    }
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buf.set_lines(start, end, lines) (0-indexed, exclusive end)
        // Neovim-compatible: nvim_buf_set_lines(0, start, end, false, lines)
        buf.set(
            "set_lines",
            lua.create_function(|lua, (start, end, lines): (i64, i64, LuaTable)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    let len = ctx.buf_lines.len() as i64;
                    let s = if start < 0 {
                        (len + start).max(0) as usize
                    } else {
                        start as usize
                    };
                    let e = if end < 0 {
                        (len + end).max(0) as usize
                    } else {
                        end as usize
                    };
                    let mut new_lines = Vec::new();
                    for i in 1..=lines.len().unwrap_or(0) {
                        if let Ok(line) = lines.get::<_, String>(i) {
                            new_lines.push(line);
                        }
                    }
                    ctx.set_lines_range.push((s, e, new_lines));
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.path() → string or nil
        buf.set(
            "path",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.buf_path.clone()))
            })?,
        )?;

        // vimcode.buf.line_count() → integer
        buf.set(
            "line_count",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.buf_lines.len())
                    .unwrap_or(0))
            })?,
        )?;

        // vimcode.buf.cursor() → {line, col}  (1-indexed)
        buf.set(
            "cursor",
            lua.create_function(|lua, ()| {
                let t = lua.create_table()?;
                if let Some(ctx) = lua.app_data_ref::<PluginCallContext>() {
                    t.set("line", ctx.cursor_line)?;
                    t.set("col", ctx.cursor_col)?;
                } else {
                    t.set("line", 1usize)?;
                    t.set("col", 1usize)?;
                }
                Ok(t)
            })?,
        )?;

        // vimcode.buf.set_cursor(line, col)  (1-indexed)
        buf.set(
            "set_cursor",
            lua.create_function(|lua, (line, col): (usize, usize)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if line > 0 && col > 0 {
                        ctx.set_cursor = Some((line, col));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.insert_line(n, text)  (1-indexed, inserts before line n)
        buf.set(
            "insert_line",
            lua.create_function(|lua, (n, text): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.insert_lines.push((n, text));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.delete_line(n)  (1-indexed)
        buf.set(
            "delete_line",
            lua.create_function(|lua, n: usize| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.delete_lines.push(n);
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.annotate_line(n, text)  (1-indexed)
        buf.set(
            "annotate_line",
            lua.create_function(|lua, (n, text): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if n > 0 {
                        ctx.annotate_lines.push((n, text));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.clear_annotations()
        buf.set(
            "clear_annotations",
            lua.create_function(|lua, ()| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.clear_annotations = true;
                    ctx.annotate_lines.clear();
                }
                Ok(())
            })?,
        )?;

        // vimcode.buf.open_scratch(name, content, opts)
        // opts: { readonly=true, filetype="diff", split="vertical"|"horizontal"|nil }
        buf.set(
            "open_scratch",
            lua.create_function(
                |lua, (name, content, opts): (String, String, Option<LuaTable>)| {
                    let mut read_only = true;
                    let mut filetype = None;
                    let mut split = None;
                    if let Some(ref t) = opts {
                        if let Ok(ro) = t.get::<_, bool>("readonly") {
                            read_only = ro;
                        }
                        if let Ok(ft) = t.get::<_, String>("filetype") {
                            filetype = Some(ft);
                        }
                        if let Ok(s) = t.get::<_, String>("split") {
                            split = Some(s);
                        }
                    }
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.scratch_buffers.push(ScratchBufferRequest {
                            name,
                            content,
                            read_only,
                            filetype,
                            split,
                        });
                    }
                    Ok(())
                },
            )?,
        )?;

        vimcode.set("buf", buf)?;

        // ── vimcode.opt subtable ────────────────────────────────────────────
        let opt = lua.create_table()?;

        // vimcode.opt.get(key) → string
        opt.set(
            "get",
            lua.create_function(|lua, key: String| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| {
                        let v = ctx.settings_snapshot.get(&key)?;
                        if v.is_empty() {
                            None
                        } else {
                            Some(v.clone())
                        }
                    })
                    .unwrap_or_default())
            })?,
        )?;

        // vimcode.opt.set(key, value) — applied after callback returns
        opt.set(
            "set",
            lua.create_function(|lua, (key, value): (String, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.set_settings.push((key, value));
                }
                Ok(())
            })?,
        )?;

        vimcode.set("opt", opt)?;

        // ── vimcode.state subtable ──────────────────────────────────────────
        let state = lua.create_table()?;

        // vimcode.state.mode() → string (e.g. "Normal", "Insert", "Visual")
        state.set(
            "mode",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.mode_name.clone())
                    .unwrap_or_default())
            })?,
        )?;

        // vimcode.state.register(char) → {content, linewise} or nil
        state.set(
            "register",
            lua.create_function(|lua, reg: String| {
                let ch = match reg.chars().next() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                let ctx = match lua.app_data_ref::<PluginCallContext>() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                match ctx.registers_snapshot.get(&ch) {
                    Some((content, linewise)) => {
                        let t = lua.create_table()?;
                        t.set("content", content.clone())?;
                        t.set("linewise", *linewise)?;
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.state.set_register(char, content, linewise)
        state.set(
            "set_register",
            lua.create_function(|lua, (reg, content, linewise): (String, String, bool)| {
                if let Some(ch) = reg.chars().next() {
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.set_registers.push((ch, content, linewise));
                    }
                }
                Ok(())
            })?,
        )?;

        // vimcode.state.mark(char) → {line, col} (1-indexed) or nil
        state.set(
            "mark",
            lua.create_function(|lua, mark: String| {
                let ch = match mark.chars().next() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                let ctx = match lua.app_data_ref::<PluginCallContext>() {
                    Some(c) => c,
                    None => return Ok(LuaValue::Nil),
                };
                match ctx.marks_snapshot.get(&ch) {
                    Some((line, col)) => {
                        let t = lua.create_table()?;
                        t.set("line", *line)?;
                        t.set("col", *col)?;
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.state.filetype() → string (e.g. "rust", "python") or ""
        state.set(
            "filetype",
            lua.create_function(|lua, ()| {
                Ok(lua
                    .app_data_ref::<PluginCallContext>()
                    .map(|ctx| ctx.filetype.clone())
                    .unwrap_or_default())
            })?,
        )?;

        vimcode.set("state", state)?;

        // ── vimcode.git subtable ────────────────────────────────────────────
        let git_tbl = lua.create_table()?;

        // vimcode.git.blame_line(n) → {hash, author, date, relative_date, message} or nil
        git_tbl.set(
            "blame_line",
            lua.create_function(|lua, n: usize| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return Ok(LuaValue::Nil),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                // Only pipe in-memory content when the buffer has unsaved changes.
                // For a clean buffer the committed content on disk is identical,
                // so we skip the expensive `--contents -` path (which requires
                // building a full-file String and spawning git with stdin).
                // buf_lines come from Ropey's line() which includes the trailing
                // \n on each line, so join with "" not "\n".
                let buf_content: Option<String> = {
                    let ctx = lua.app_data_ref::<PluginCallContext>();
                    ctx.and_then(|c| {
                        if c.buf_dirty {
                            Some(c.buf_lines.join(""))
                        } else {
                            None
                        }
                    })
                };
                let info = match git::blame_line(&repo_root, &file, n, buf_content.as_deref()) {
                    Some(i) => i,
                    None => return Ok(LuaValue::Nil),
                };
                let t = lua.create_table()?;
                t.set("hash", info.hash)?;
                t.set("author", info.author)?;
                t.set("date", info.timestamp)?;
                t.set("relative_date", info.relative_date)?;
                t.set("message", info.message)?;
                t.set("not_committed", info.not_committed)?;
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.log_file(limit) → [{hash, message}, ...]
        git_tbl.set(
            "log_file",
            lua.create_function(|lua, limit: usize| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => {
                            return lua.create_table();
                        }
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table(),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let entries = git::log_file(&repo_root, &file, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("message", e.message)?;
                    t.set(i + 1, row)?;
                }
                Ok(t)
            })?,
        )?;

        // vimcode.git.show(hash) → string or nil
        git_tbl.set(
            "show",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::show_commit(&dir, &hash) {
                    Some(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.blame_file() → [{hash, author, date, relative_date, message, not_committed}, ...]
        git_tbl.set(
            "blame_file",
            lua.create_function(|lua, ()| {
                let (cwd_path, buf_path_os, buf_dirty) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return lua.create_table().map(LuaValue::Table),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone(), ctx.buf_dirty)
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let buf_content: Option<String> = if buf_dirty {
                    lua.app_data_ref::<PluginCallContext>()
                        .map(|c| c.buf_lines.join(""))
                } else {
                    None
                };
                let entries = git::blame_file_structured(&repo_root, &file, buf_content.as_deref());
                let t = lua.create_table()?;
                for (i, info) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", info.hash)?;
                    row.set("author", info.author)?;
                    row.set("date", info.timestamp)?;
                    row.set("relative_date", info.relative_date)?;
                    row.set("message", info.message)?;
                    row.set("not_committed", info.not_committed)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.line_log(start, end, limit) → [{hash, author, date, message}, ...]
        git_tbl.set(
            "line_log",
            lua.create_function(|lua, (start, end, limit): (usize, usize, usize)| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return lua.create_table().map(LuaValue::Table),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let entries = git::log_line_range(&repo_root, &file, start, end, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("author", e.author)?;
                    row.set("date", e.date)?;
                    row.set("message", e.message)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.diff_ref(ref) → string or nil
        git_tbl.set(
            "diff_ref",
            lua.create_function(|lua, ref_spec: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::diff_against_ref(&dir, &ref_spec) {
                    Some(s) if !s.trim().is_empty() => Ok(LuaValue::String(lua.create_string(&s)?)),
                    _ => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.file_log(limit) → [{hash, author, date, message, stat}, ...]
        // (detailed version, replaces the simple log_file for richer data)
        git_tbl.set(
            "file_log_detailed",
            lua.create_function(|lua, limit: usize| {
                let (cwd_path, buf_path_os) = {
                    let ctx = match lua.app_data_ref::<PluginCallContext>() {
                        Some(c) => c,
                        None => return lua.create_table().map(LuaValue::Table),
                    };
                    (ctx.cwd_path.clone(), ctx.buf_path_os.clone())
                };
                let file = match buf_path_os {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let repo_root = git::find_repo_root(cwd_path.as_deref().unwrap_or(&file))
                    .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
                let entries = git::file_log_detailed(&repo_root, &file, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("author", e.author)?;
                    row.set("date", e.date)?;
                    row.set("message", e.message)?;
                    row.set("stat", e.stat)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.repo_root() → string or nil
        git_tbl.set(
            "repo_root",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                match cwd_path.and_then(|p| git::find_repo_root(&p)) {
                    Some(root) => Ok(LuaValue::String(
                        lua.create_string(root.to_string_lossy().as_bytes())?,
                    )),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.branch() → string or nil
        git_tbl.set(
            "branch",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::current_branch(&dir) {
                    Some(b) => Ok(LuaValue::String(lua.create_string(&b)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.stash_list() → [{index, message, branch}, ...]
        git_tbl.set(
            "stash_list",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let entries = git::stash_list(&dir);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("index", e.index)?;
                    row.set("message", e.message)?;
                    row.set("branch", e.branch)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.stash_push(msg) → string (result message)
        git_tbl.set(
            "stash_push",
            lua.create_function(|lua, msg: Option<String>| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok("no working directory".to_string()),
                };
                match git::stash_push(&dir, msg.as_deref()) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            })?,
        )?;

        // vimcode.git.stash_pop(index) → string (result message)
        git_tbl.set(
            "stash_pop",
            lua.create_function(|lua, index: Option<usize>| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok("no working directory".to_string()),
                };
                match git::stash_pop(&dir, index.unwrap_or(0)) {
                    Ok(s) => Ok(s),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            })?,
        )?;

        // vimcode.git.stash_show(index) → string or nil
        git_tbl.set(
            "stash_show",
            lua.create_function(|lua, index: Option<usize>| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::stash_show(&dir, index.unwrap_or(0)) {
                    Some(s) => Ok(LuaValue::String(lua.create_string(&s)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.log(limit) → [{hash, message}, ...] (repo-wide log)
        git_tbl.set(
            "log",
            lua.create_function(|lua, limit: usize| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return lua.create_table().map(LuaValue::Table),
                };
                let entries = git::git_log(&dir, limit);
                let t = lua.create_table()?;
                for (i, e) in entries.into_iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("hash", e.hash)?;
                    row.set("message", e.message)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.log_commit(hash) → {hash, message} or nil
        git_tbl.set(
            "log_commit",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                match git::git_log_commit(&dir, &hash) {
                    Some(e) => {
                        let row = lua.create_table()?;
                        row.set("hash", e.hash)?;
                        row.set("message", e.message)?;
                        Ok(LuaValue::Table(row))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.branches() → [{name, is_current, upstream, ahead_behind}]
        git_tbl.set(
            "branches",
            lua.create_function(|lua, ()| {
                let cwd = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let branches = git::list_branches(&dir);
                let t = lua.create_table()?;
                for (i, b) in branches.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("name", b.name.as_str())?;
                    row.set("is_current", b.is_current)?;
                    row.set(
                        "upstream",
                        if b.upstream.is_some() {
                            LuaValue::String(
                                lua.create_string(b.upstream.as_deref().unwrap_or(""))?,
                            )
                        } else {
                            LuaValue::Nil
                        },
                    )?;
                    row.set(
                        "ahead_behind",
                        if b.ahead_behind.is_some() {
                            LuaValue::String(
                                lua.create_string(b.ahead_behind.as_deref().unwrap_or(""))?,
                            )
                        } else {
                            LuaValue::Nil
                        },
                    )?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.commit_url(hash) → string or nil (HTTPS URL to commit on hosting platform)
        git_tbl.set(
            "commit_url",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::commit_url(&repo_root, &hash) {
                    Some(url) => Ok(LuaValue::String(lua.create_string(&url)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.remote_url() → string or nil (HTTPS base URL of the origin remote)
        git_tbl.set(
            "remote_url",
            lua.create_function(|lua, ()| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::remote_url(&repo_root) {
                    Some(url) => Ok(LuaValue::String(lua.create_string(&url)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.commit_files(hash) → [{status="M", path="src/main.rs"}, ...] or nil
        git_tbl.set(
            "commit_files",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                let files = git::commit_files(&repo_root, &hash);
                if files.is_empty() {
                    return Ok(LuaValue::Nil);
                }
                let t = lua.create_table()?;
                for (i, f) in files.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("status", lua.create_string(f.status.to_string())?)?;
                    row.set("path", lua.create_string(&f.path)?)?;
                    t.set(i + 1, row)?;
                }
                Ok(LuaValue::Table(t))
            })?,
        )?;

        // vimcode.git.diff_file(hash, path) → string or nil (file content at commit)
        git_tbl.set(
            "diff_file",
            lua.create_function(|lua, (hash, path): (String, String)| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::diff_file_at_commit(&repo_root, &hash, &path) {
                    Some(content) => Ok(LuaValue::String(lua.create_string(&content)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.open_diff(hash, path) — open side-by-side diff (hash~1 vs hash)
        git_tbl.set(
            "open_diff",
            lua.create_function(|lua, (hash, path): (String, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.commit_file_diff = Some((hash, path));
                }
                Ok(())
            })?,
        )?;

        // vimcode.git.show_file(hash, path) → string or nil (diff for a file in a commit)
        git_tbl.set(
            "show_file",
            lua.create_function(|lua, (hash, path): (String, String)| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::show_commit_file(&repo_root, &hash, &path) {
                    Some(content) => Ok(LuaValue::String(lua.create_string(&content)?)),
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        // vimcode.git.commit_detail(hash) → {hash, author, date, message, stat} or nil
        git_tbl.set(
            "commit_detail",
            lua.create_function(|lua, hash: String| {
                let cwd_path = lua
                    .app_data_ref::<PluginCallContext>()
                    .and_then(|ctx| ctx.cwd_path.clone());
                let dir = match cwd_path {
                    Some(p) => p,
                    None => return Ok(LuaValue::Nil),
                };
                let repo_root = git::find_repo_root(&dir).unwrap_or(dir);
                match git::commit_detail(&repo_root, &hash) {
                    Some(detail) => {
                        let t = lua.create_table()?;
                        t.set("hash", lua.create_string(&detail.hash)?)?;
                        t.set("author", lua.create_string(&detail.author)?)?;
                        t.set("date", lua.create_string(&detail.date)?)?;
                        t.set("message", lua.create_string(&detail.message)?)?;
                        t.set("stat", lua.create_string(&detail.stat)?)?;
                        Ok(LuaValue::Table(t))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;

        vimcode.set("git", git_tbl)?;

        // ── vimcode.panel subtable ──────────────────────────────────────────
        let panel_tbl = lua.create_table()?;

        // vimcode.panel.register(name, opts) — register an extension panel
        panel_tbl.set(
            "register",
            lua.create_function(|lua, (name, opts): (String, LuaTable)| {
                let title: String = opts.get("title").unwrap_or_default();
                let icon_str: String = opts.get("icon").unwrap_or_default();
                let icon = icon_str
                    .chars()
                    .next()
                    .unwrap_or(crate::icons::PLUGIN_FALLBACK.c());
                let fb_str: String = opts.get("fallback_icon").unwrap_or_default();
                let fallback_icon = fb_str.chars().next();
                let sections_val: LuaTable = opts.get("sections")?;
                let mut sections = Vec::new();
                for (_, s) in sections_val.pairs::<usize, String>().flatten() {
                    sections.push(s);
                }
                if sections.is_empty() {
                    sections.push("Default".to_string());
                }
                let reg = PanelRegistration {
                    name: name.clone(),
                    title,
                    icon,
                    fallback_icon,
                    sections,
                };
                // During load_one_plugin: write to PluginRegistrations
                if let Some(mut regs) = lua.app_data_mut::<PluginRegistrations>() {
                    regs.panels.push(reg.clone());
                }
                // During runtime calls: write to PluginCallContext
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.panel_registrations.push(reg);
                }
                Ok(())
            })?,
        )?;

        // vimcode.panel.set_items(name, section, items)
        panel_tbl.set(
            "set_items",
            lua.create_function(
                |lua, (name, section, items_tbl): (String, String, LuaTable)| {
                    let mut items = Vec::new();
                    for (_, row) in items_tbl.pairs::<usize, LuaTable>().flatten() {
                        let text: String = row.get("text").unwrap_or_default();
                        let hint: String = row.get("hint").unwrap_or_default();
                        let icon: String = row.get("icon").unwrap_or_default();
                        let indent: u8 = row.get("indent").unwrap_or(0);
                        let style_str: String = row.get("style").unwrap_or_default();
                        let style = match style_str.as_str() {
                            "header" => ExtPanelStyle::Header,
                            "dim" => ExtPanelStyle::Dim,
                            "accent" => ExtPanelStyle::Accent,
                            _ => ExtPanelStyle::Normal,
                        };
                        let id: String = row.get("id").unwrap_or_default();
                        let expandable: bool = row.get("expandable").unwrap_or(false);
                        let expanded: bool = row.get("expanded").unwrap_or(false);
                        let parent_id: String = row.get("parent_id").unwrap_or_default();
                        let is_separator: bool = row.get("is_separator").unwrap_or(false);
                        // Parse actions: [{label="Stage", key="s"}, ...]
                        let mut actions = Vec::new();
                        if let Ok(acts_tbl) = row.get::<_, LuaTable>("actions") {
                            for (_, act) in acts_tbl.pairs::<usize, LuaTable>().flatten() {
                                actions.push(ExtPanelAction {
                                    label: act.get("label").unwrap_or_default(),
                                    key: act.get("key").unwrap_or_default(),
                                });
                            }
                        }
                        // Parse badges: [{text="main", color="green"}, ...]
                        let mut badges = Vec::new();
                        if let Ok(bdg_tbl) = row.get::<_, LuaTable>("badges") {
                            for (_, bdg) in bdg_tbl.pairs::<usize, LuaTable>().flatten() {
                                badges.push(ExtPanelBadge {
                                    text: bdg.get("text").unwrap_or_default(),
                                    color: bdg.get("color").unwrap_or_default(),
                                });
                            }
                        }
                        items.push(ExtPanelItem {
                            text,
                            hint,
                            icon,
                            indent,
                            style,
                            id,
                            expandable,
                            expanded,
                            parent_id,
                            actions,
                            badges,
                            is_separator,
                        });
                    }
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.panel_set_items.push((name, section, items));
                    }
                    Ok(())
                },
            )?,
        )?;

        // vimcode.panel.set_hover(panel_name, item_id, markdown) — register hover content
        panel_tbl.set(
            "set_hover",
            lua.create_function(
                |lua, (panel_name, item_id, markdown): (String, String, String)| {
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.panel_hover_entries
                            .push((panel_name, item_id, markdown));
                    }
                    Ok(())
                },
            )?,
        )?;

        // vimcode.panel.set_help(panel_name, bindings) — register help popup bindings
        // bindings is a table of {key, description} pairs
        panel_tbl.set(
            "set_help",
            lua.create_function(|lua, (panel_name, bindings): (String, LuaTable)| {
                let mut entries = Vec::new();
                for pair in bindings.sequence_values::<LuaTable>() {
                    let t = pair?;
                    let key: String = t.get(1)?;
                    let desc: String = t.get(2)?;
                    entries.push((key, desc));
                }
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.panel_help_entries.push((panel_name, entries));
                } else if let Some(mut reg) = lua.app_data_mut::<PluginRegistrations>() {
                    reg.help_bindings.push((panel_name, entries));
                }
                Ok(())
            })?,
        )?;

        // vimcode.panel.parse_event(arg) — split "|"-delimited event arg into table
        panel_tbl.set(
            "parse_event",
            lua.create_function(|lua, arg: String| {
                let parts: Vec<&str> = arg.splitn(5, '|').collect();
                let t = lua.create_table()?;
                t.set("panel", parts.first().copied().unwrap_or(""))?;
                t.set("section", parts.get(1).copied().unwrap_or(""))?;
                t.set("id", parts.get(2).copied().unwrap_or(""))?;
                t.set("key", parts.get(3).copied().unwrap_or(""))?;
                t.set(
                    "index",
                    parts
                        .get(4)
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0),
                )?;
                Ok(t)
            })?,
        )?;

        // vimcode.panel.get_input(panel_name) — get the current input field text
        panel_tbl.set(
            "get_input",
            lua.create_function(|lua, panel_name: String| {
                if let Some(ctx) = lua.app_data_ref::<PluginCallContext>() {
                    if let Some(text) = ctx.panel_input_snapshot.get(&panel_name) {
                        return Ok(text.clone());
                    }
                }
                Ok(String::new())
            })?,
        )?;

        // vimcode.panel.set_input(panel_name, text) — set the input field text
        panel_tbl.set(
            "set_input",
            lua.create_function(|lua, (panel_name, text): (String, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.panel_input_values.push((panel_name, text));
                }
                Ok(())
            })?,
        )?;

        // vimcode.panel.reveal(panel_name, section_name, item_id) — switch to panel and highlight item
        panel_tbl.set(
            "reveal",
            lua.create_function(
                |lua, (panel_name, section_name, item_id): (String, String, String)| {
                    if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                        ctx.panel_reveal_request = Some((panel_name, section_name, item_id));
                    }
                    Ok(())
                },
            )?,
        )?;

        vimcode.set("panel", panel_tbl)?;

        // ── vimcode.editor subtable ────────────────────────────────────────
        let editor_tbl = lua.create_table()?;

        // vimcode.editor.set_hover(line, markdown) — set hover content for a buffer line
        editor_tbl.set(
            "set_hover",
            lua.create_function(|lua, (line_1indexed, markdown): (usize, String)| {
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    if line_1indexed > 0 {
                        ctx.editor_hover_entries.push((line_1indexed - 1, markdown));
                    }
                }
                Ok(())
            })?,
        )?;

        vimcode.set("editor", editor_tbl)?;

        // ── vimcode.set_comment_style(lang_id, opts) ────────────────────────
        vimcode.set(
            "set_comment_style",
            lua.create_function(|lua, (lang_id, opts): (String, LuaTable)| {
                let line: String = opts.get("line").unwrap_or_default();
                let block_open: String = opts.get("block_open").unwrap_or_default();
                let block_close: String = opts.get("block_close").unwrap_or_default();
                if let Some(mut ctx) = lua.app_data_mut::<PluginCallContext>() {
                    ctx.comment_style_overrides
                        .push((lang_id, line, block_open, block_close));
                }
                Ok(())
            })?,
        )?;

        lua.globals().set("vimcode", vimcode)?;
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_plugin(dir: &std::path::Path, name: &str, code: &str) -> PathBuf {
        let path = dir.join(format!("{name}.lua"));
        std::fs::write(&path, code).unwrap();
        path
    }

    #[test]
    fn test_plugin_command_registered_and_callable() {
        let dir = std::env::temp_dir().join("vc_plugin_test_cmd");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "test",
            r#"
            vimcode.command("Hello", function(args)
                vimcode.message("Hello: " .. args)
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        assert_eq!(pm.plugins.len(), 1);
        assert!(pm.plugins[0].error.is_none());

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("Hello", "world", ctx);
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("Hello: world"));
    }

    #[test]
    fn test_plugin_on_save_hook_fires() {
        let dir = std::env::temp_dir().join("vc_plugin_test_save");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "savehook",
            r#"
            vimcode.on("save", function(path)
                vimcode.message("saved: " .. path)
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let ctx = pm.call_event("save", "/tmp/foo.rs", ctx);
        assert_eq!(ctx.message.as_deref(), Some("saved: /tmp/foo.rs"));
    }

    #[test]
    fn test_plugin_disabled_command_not_registered() {
        let dir = std::env::temp_dir().join("vc_plugin_test_disabled");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "myplugin",
            r#"
            vimcode.command("ShouldNotExist", function(args)
                vimcode.message("should not run")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &["myplugin".to_string()]);

        assert_eq!(pm.plugins.len(), 1);
        assert!(!pm.plugins[0].enabled);

        let ctx = PluginCallContext::default();
        let (found, _) = pm.call_command("ShouldNotExist", "", ctx);
        assert!(!found);
    }

    #[test]
    fn test_plugin_load_error_recorded() {
        let dir = std::env::temp_dir().join("vc_plugin_test_err");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(&dir, "broken", "this is not valid lua @@@");

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        assert_eq!(pm.plugins.len(), 1);
        assert!(pm.plugins[0].error.is_some());
    }

    #[test]
    fn test_plugin_keymap_registered_and_callable() {
        let dir = std::env::temp_dir().join("vc_plugin_test_km");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "keys",
            r#"
            vimcode.keymap("n", "<leader>x", function()
                vimcode.message("keymap fired")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_keymap("n", "<leader>x", ctx);
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("keymap fired"));
    }

    #[test]
    fn test_plugin_buf_api() {
        let dir = std::env::temp_dir().join("vc_plugin_test_buf");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "buftest",
            r#"
            vimcode.command("BufInfo", function(_)
                local count = vimcode.buf.line_count()
                local first = vimcode.buf.line(1) or "(nil)"
                vimcode.message("lines=" .. count .. " first=" .. first)
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext {
            buf_lines: vec!["hello".to_string(), "world".to_string()],
            ..Default::default()
        };
        let (found, ctx) = pm.call_command("BufInfo", "", ctx);
        assert!(found);
        assert_eq!(ctx.message.as_deref(), Some("lines=2 first=hello"));
    }

    #[test]
    fn test_async_shell_request_registered() {
        let dir = std::env::temp_dir().join("vc_plugin_test_async_shell");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "asynctest",
            r#"
            vimcode.command("RunAsync", function(_)
                vimcode.async_shell("echo hello", "my_callback")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("RunAsync", "", ctx);
        assert!(found);
        assert_eq!(ctx.async_shell_requests.len(), 1);
        assert_eq!(ctx.async_shell_requests[0].command, "echo hello");
        assert_eq!(ctx.async_shell_requests[0].callback_event, "my_callback");
        assert!(ctx.async_shell_requests[0].stdin.is_none());
        assert!(ctx.async_shell_requests[0].cwd.is_none());
    }

    #[test]
    fn test_async_shell_with_options() {
        let dir = std::env::temp_dir().join("vc_plugin_test_async_opts");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "asyncopts",
            r#"
            vimcode.command("RunAsyncOpts", function(_)
                vimcode.async_shell("cat", "cat_result", { stdin = "hello world", cwd = "/tmp" })
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("RunAsyncOpts", "", ctx);
        assert!(found);
        assert_eq!(ctx.async_shell_requests.len(), 1);
        assert_eq!(ctx.async_shell_requests[0].command, "cat");
        assert_eq!(ctx.async_shell_requests[0].callback_event, "cat_result");
        assert_eq!(
            ctx.async_shell_requests[0].stdin.as_deref(),
            Some("hello world")
        );
        assert_eq!(
            ctx.async_shell_requests[0].cwd.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
    }

    #[test]
    fn test_async_shell_empty_args_ignored() {
        let dir = std::env::temp_dir().join("vc_plugin_test_async_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_temp_plugin(
            &dir,
            "asyncempty",
            r#"
            vimcode.command("RunEmpty", function(_)
                vimcode.async_shell("", "my_callback")
                vimcode.async_shell("echo hi", "")
            end)
            "#,
        );

        let mut pm = PluginManager::new().unwrap();
        pm.load_plugins_dir(&dir, &[]);

        let ctx = PluginCallContext::default();
        let (found, ctx) = pm.call_command("RunEmpty", "", ctx);
        assert!(found);
        // Both calls should be silently ignored (empty command or empty event).
        assert_eq!(ctx.async_shell_requests.len(), 0);
    }
}
