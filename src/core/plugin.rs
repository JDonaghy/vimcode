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
    // ── Outputs written by callbacks ────────────────────────────────────────
    pub message: Option<String>,
    /// `(0-based line index, new text)` — applied by the engine after the call.
    pub set_lines: Vec<(usize, String)>,
    /// VimCode commands to execute after the call (e.g. `"w"` to save).
    pub run_commands: Vec<String>,
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

        vimcode.set("buf", buf)?;
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
}
