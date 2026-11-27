//! `vimcode --test-extension <ext-dir> <spec.lua>`: a headless harness that
//! loads a real extension directory exactly as installed (manifest +
//! scripts) and runs a Lua spec file against it through the TUI driver.
//!
//! Unlike `tests/extensions.rs` (synthetic manifests built in Rust) and
//! `crate::harness`/`tui_main::testing` (test-only, `cfg(any(test,
//! feature = "test-support"))`), this module compiles into every build —
//! `vimcode-ext`'s own CI downloads a release binary and runs each
//! extension's `tests/*_spec.lua` against it, so the harness cannot be
//! test-gated out of that binary.
//!
//! # Isolation
//!
//! The extension is installed into a scratch `$HOME` this function creates
//! and removes, never the real `~/.config/vimcode` — see [`run`]. Disk
//! saves are suppressed for the whole process via
//! `core::session::suppress_disk_saves`, so nothing a spec triggers
//! (`:w`, settings changes, …) touches the real config dir either.
//!
//! # The two Lua VMs
//!
//! The extension's own scripts run in the engine's ordinary
//! `PluginManager` Lua VM — the exact production path `Engine::startup`
//! always takes. The spec file runs in a **second, independent** `mlua::Lua`
//! that only knows the small `t.*` assertion API this module registers
//! (see [`install_assertion_api`]); it never touches `vimcode.*` directly.
//! A [`crate::core::plugin_ui::PluginView`] crossing from the engine's view
//! cache into the spec VM goes through `serde_json::Value` (see
//! [`json_to_lua`]), not a shared Lua registry key, because the two `Lua`
//! instances are unrelated interpreters.
//!
//! The full `t.*` surface a spec sees: `t.keys`, `t.command`, `t.wait_for`,
//! `t.screen`/`t.contains`/`t.cell`, `t.click`/`t.click_text`, `t.view`,
//! and `t.stub_command` — see [`install_assertion_api`] for each one's doc.
//!
//! # Process stubbing
//!
//! `t.stub_command(name, path)` writes a fake executable named `name` onto
//! a scratch directory prepended to `PATH`, so an extension's
//! `vimcode.loop`/async-shell/terminal spawn of `name` finds the fake
//! instead of (or absence of) the real tool. `path` is either a directory
//! (copied verbatim, so a hand-written shell-script fixture travels with
//! any data files it reads) or a `.lua` file, wrapped in a shim that
//! re-invokes this same binary with the hidden `--run-lua-stub` flag (see
//! [`run_lua_stub`]) — no system `lua` binary required.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::prelude::*;
use quadraui::tui::testing::{driver_with_shell, Modifier, TuiDriver};

use crate::core::engine::Engine;

/// Fixed viewport size every `--test-extension` run paints at — see this
/// module's doc for why the size is not spec-configurable: a backend-
/// neutral spec must not depend on terminal geometry any more than a
/// driver test does.
const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

/// Entry point for `vimcode --test-extension <ext-dir> <spec.lua>`.
/// Returns the process exit code: `0` on a passing spec, `1` on a read/
/// install/parse error or a failing assertion.
pub fn run(ext_dir: &Path, spec_path: &Path) -> i32 {
    // Nothing this harness does — installing the extension, loading its
    // scripts, whatever settings/session changes the spec triggers —
    // should ever touch the real `~/.config/vimcode`.
    crate::core::session::suppress_disk_saves();

    // Resolve both paths against *this* process's cwd right away, before
    // `Engine::startup`'s own directory file-argument handling below
    // `chdir`s into the scratch workspace — otherwise a relative
    // `spec_path` (and anything a spec later resolves against its
    // directory, e.g. `t.stub_command`'s fixture path) would silently
    // resolve against the wrong directory from that point on.
    let ext_dir = match std::fs::canonicalize(ext_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "vimcode --test-extension: cannot resolve extension dir {}: {e}",
                ext_dir.display()
            );
            return 1;
        }
    };
    let spec_path = match std::fs::canonicalize(spec_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "vimcode --test-extension: cannot resolve spec file {}: {e}",
                spec_path.display()
            );
            return 1;
        }
    };
    let ext_dir = ext_dir.as_path();
    let spec_path = spec_path.as_path();

    let manifest_path = ext_dir.join("manifest.toml");
    let manifest_toml = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "vimcode --test-extension: cannot read {}: {e}",
                manifest_path.display()
            );
            return 1;
        }
    };
    let manifest = match crate::core::extensions::ExtensionManifest::parse(&manifest_toml) {
        Some(m) if !m.name.is_empty() => m,
        _ => {
            eprintln!(
                "vimcode --test-extension: {} has no valid `name`",
                manifest_path.display()
            );
            return 1;
        }
    };

    let spec_src = match std::fs::read_to_string(spec_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "vimcode --test-extension: cannot read spec {}: {e}",
                spec_path.display()
            );
            return 1;
        }
    };

    let scratch = std::env::temp_dir().join(format!(
        "vimcode-test-ext-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    if let Err(e) = std::fs::create_dir_all(&scratch) {
        eprintln!(
            "vimcode --test-extension: cannot create scratch dir {}: {e}",
            scratch.display()
        );
        return 1;
    }

    let code = run_isolated(&scratch, ext_dir, &manifest, spec_path, &spec_src);
    let _ = std::fs::remove_dir_all(&scratch);
    code
}

/// The part of [`run`] that runs once the scratch directory exists —
/// split out so every early-return below still reaches [`run`]'s cleanup.
fn run_isolated(
    scratch: &Path,
    ext_dir: &Path,
    manifest: &crate::core::extensions::ExtensionManifest,
    spec_path: &Path,
    spec_src: &str,
) -> i32 {
    // Set every env var `core::paths::vimcode_config_dir()` consults
    // *before* calling it, then derive the install dir and state file
    // from that same function rather than re-deriving the per-platform
    // layout by hand — the same call every other site in the codebase
    // makes (`core::engine::lsp_ops`, `core::session`). On Windows this
    // resolves to `%APPDATA%\vimcode`, which differs from Unix's
    // `$HOME/.config/vimcode`; deriving it from the real function keeps
    // this harness honest about that instead of assuming the Unix shape.
    std::env::set_var("HOME", scratch);
    std::env::set_var("USERPROFILE", scratch);
    std::env::set_var("APPDATA", scratch);

    // Pin the terminal-capability env too, so `t.cell(...)`'s `fg`/`bg`
    // (derived from `TuiBackend::new()`'s `detect_color_depth()` /
    // `detect_underline_color_support()`, which read `COLORTERM`/`TERM`/
    // `WT_SESSION`/`TERM_PROGRAM`) are reproducible regardless of whatever
    // real terminal (or CI runner) happens to be hosting this harness.
    std::env::set_var("TERM", "xterm-256color");
    std::env::set_var("COLORTERM", "truecolor");
    std::env::remove_var("WT_SESSION");
    std::env::remove_var("TERM_PROGRAM");

    let config_dir = crate::core::paths::vimcode_config_dir();
    let ext_dest = config_dir.join("extensions").join(&manifest.name);
    if let Err(e) = copy_dir_recursive(ext_dir, &ext_dest) {
        eprintln!("vimcode --test-extension: cannot install extension: {e}");
        return 1;
    }

    let state = crate::core::session::ExtensionState {
        installed: vec![crate::core::session::InstalledExtension {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
        }],
        dismissed: Vec::new(),
    };
    let state_json = match serde_json::to_string_pretty(&state) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("vimcode --test-extension: cannot encode extension state: {e}");
            return 1;
        }
    };
    if let Err(e) = std::fs::write(config_dir.join("extensions.json"), state_json) {
        eprintln!("vimcode --test-extension: cannot write extension state: {e}");
        return 1;
    }

    let stub_bin_dir = scratch.join("stub-bin");
    if let Err(e) = std::fs::create_dir_all(&stub_bin_dir) {
        eprintln!(
            "vimcode --test-extension: cannot create stub bin dir {}: {e}",
            stub_bin_dir.display()
        );
        return 1;
    }
    prepend_path(&stub_bin_dir);

    // A dedicated, empty workspace directory — never the real process
    // cwd `Engine::startup` would otherwise adopt for a no-file-argument
    // TUI launch, and never the scratch `$HOME` itself (whose
    // `.config/vimcode/extensions/...` tree a spec has no business
    // browsing in the Explorer).
    let workspace = scratch.join("workspace");
    if let Err(e) = std::fs::create_dir_all(&workspace) {
        eprintln!(
            "vimcode --test-extension: cannot create workspace dir {}: {e}",
            workspace.display()
        );
        return 1;
    }

    let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
        Rc::new(RefCell::new(Box::new(quadraui::tui::TuiBackend::new())));
    let app =
        crate::app::App::new_portable(Some(workspace), backend, crate::render::UnitProfile::cell());
    let engine = app.engine.clone();
    let config = app.shell_config();
    let driver = driver_with_shell(app, config, WIDTH, HEIGHT);

    run_spec(driver, engine, spec_src, spec_path, stub_bin_dir)
}

/// Prepend `dir` to `PATH` so a bare command name resolves there first —
/// the mechanism [`stub_command`]-registered fakes rely on. `std::process::
/// Command::new` does an ordinary `PATH` search for a non-absolute program
/// name, so this is all `vimcode.loop`/async-shell/terminal spawns need.
fn prepend_path(dir: &Path) {
    let old = std::env::var_os("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut new_path = dir.as_os_str().to_os_string();
    new_path.push(sep);
    new_path.push(&old);
    std::env::set_var("PATH", new_path);
}

/// Copy every regular file and subdirectory under `src` into `dst`,
/// creating `dst` (and any missing parent) first. Symlinks are skipped —
/// no fixture extension needs one.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Give `path` the executable bit on Unix; a no-op everywhere else (a
/// `.cmd`/`.bat` on Windows needs no permission bit — its extension alone
/// makes it runnable).
#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path)?.permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Backs `t.stub_command(name, path)`. `path` is either a fixture
/// directory (copied verbatim into `stub_bin_dir`, so a hand-written fake
/// executable travels with any data files it reads — subdirectories are
/// skipped, matching `copy_dir_recursive`'s one-level contract for this
/// call site) or a `.lua` file (wrapped in a shim that re-invokes this
/// same binary's hidden `--run-lua-stub`).
///
/// Only spawns on Unix today: `vimcode.loop.spawn` runs
/// `std::process::Command::new(name)` directly, which on Windows cannot
/// execute either a `.cmd`/`.bat` shim (that needs `cmd.exe /C`) or an
/// extensionless `#!/bin/sh` file. A directory-form stub likewise must
/// already contain a real `.exe` for `name` to resolve on Windows.
fn stub_command(stub_bin_dir: &Path, name: &str, path: &Path) -> std::io::Result<()> {
    if name.contains(std::path::is_separator) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("stub_command: name {name:?} must not contain a path separator"),
        ));
    }

    if path.is_dir() {
        let mut found = false;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            if entry.file_name() == name {
                found = true;
            }
            let dest = stub_bin_dir.join(entry.file_name());
            std::fs::copy(entry.path(), &dest)?;
            make_executable(&dest)?;
        }
        if !found {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("stub_command: {path:?} has no file literally named {name:?}"),
            ));
        }
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let abs_lua = std::fs::canonicalize(path)?;
    let dest = stub_bin_dir.join(name);
    let script = format!(
        "#!/bin/sh\nexec \"{}\" --run-lua-stub \"{}\" \"$@\"\n",
        exe.display(),
        abs_lua.display(),
    );
    std::fs::write(&dest, script)?;
    make_executable(&dest)?;

    // `.cmd` shims cannot actually run through `vimcode.loop.spawn` (see
    // this function's doc comment); written anyway so a stub at least
    // resolves on `PATH` for any spawn mechanism that does go through a
    // real shell. Harmless on Unix — no `.cmd` in `PATHEXT` there.
    let cmd_dest = stub_bin_dir.join(format!("{name}.cmd"));
    let cmd_script = format!(
        "@echo off\r\n\"{}\" --run-lua-stub \"{}\" %*\r\n",
        exe.display(),
        abs_lua.display(),
    );
    std::fs::write(cmd_dest, cmd_script)?;
    Ok(())
}

/// The hidden `vimcode --run-lua-stub <file> [args...]` entry point —
/// never documented as a user-facing flag. Runs `file` in a fresh,
/// standalone `mlua::Lua`, with `arg` set to Lua's own CLI convention
/// (`arg[0]` the script path, `arg[1..]` the forwarded arguments), and
/// returns its exit code. A script that calls `os.exit(n)` itself never
/// returns here — the real `lua54` C library's `os.exit` terminates the
/// process directly, which is correct: this runs as a genuinely separate
/// OS process (the shell shim `stub_command` writes always `exec`s it),
/// never the harness's own.
pub fn run_lua_stub(file: &Path, args: &[String]) -> i32 {
    let lua = Lua::new();
    let arg_table = match lua.create_table() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("vimcode --run-lua-stub: {e}");
            return 1;
        }
    };
    let _ = arg_table.set(0, file.display().to_string());
    for (i, a) in args.iter().enumerate() {
        let _ = arg_table.set((i + 1) as i64, a.clone());
    }
    if let Err(e) = lua.globals().set("arg", arg_table) {
        eprintln!("vimcode --run-lua-stub: {e}");
        return 1;
    }

    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "vimcode --run-lua-stub: cannot read {}: {e}",
                file.display()
            );
            return 1;
        }
    };

    match lua.load(&src).set_name(file.display().to_string()).exec() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("vimcode --run-lua-stub: {e}");
            1
        }
    }
}

/// Shared state every `t.*` assertion-API function closes over: the
/// driver the spec drives and the engine Rc it shares with that driver's
/// `App`.
struct Harness<A: quadraui::AppLogic> {
    driver: TuiDriver<A>,
    engine: Rc<RefCell<Engine>>,
}

/// Build the spec `Lua`, install the `t.*` API, run `spec_src`, and report
/// the result. Returns `0` on a passing spec, `1` otherwise — printing the
/// screen dump alongside a failing assertion's message, per this module's
/// own doc.
fn run_spec<A: quadraui::AppLogic + 'static>(
    driver: TuiDriver<A>,
    engine: Rc<RefCell<Engine>>,
    spec_src: &str,
    spec_path: &Path,
    stub_bin_dir: PathBuf,
) -> i32 {
    print_extension_load_diagnostics(&engine);
    let harness = Rc::new(RefCell::new(Harness { driver, engine }));
    let spec_dir = spec_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let lua = Lua::new();
    if let Err(e) = install_assertion_api(&lua, harness.clone(), stub_bin_dir, spec_dir) {
        eprintln!("vimcode --test-extension: cannot install assertion API: {e}");
        return 1;
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lua.load(spec_src)
            .set_name(spec_path.display().to_string())
            .exec()
    }));

    match result {
        Ok(Ok(())) => {
            println!("PASS: {}", spec_path.display());
            0
        }
        Ok(Err(e)) => {
            let screen = harness.borrow().driver.screen();
            eprintln!(
                "FAIL: {}\n{e}\n\n--- screen dump ---\n{screen}",
                spec_path.display()
            );
            1
        }
        Err(panic) => {
            let screen = harness.borrow().driver.screen();
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            eprintln!(
                "FAIL: {}\npanic: {msg}\n\n--- screen dump ---\n{screen}",
                spec_path.display()
            );
            1
        }
    }
}

/// Print whatever `Engine::plugin_init` recorded about the extension
/// under test — a script-load error (unparsable Lua, a missing
/// `vimcode.*` API) or a `requires_vimcode` incompatibility skip — to
/// stderr before the spec runs. Without this, a load failure surfaces
/// only as an unrelated assertion failing later with a screen dump that
/// gives no hint why the extension's commands/views never registered.
fn print_extension_load_diagnostics(engine: &Rc<RefCell<Engine>>) {
    let eb = engine.borrow();
    if !eb.message.is_empty() {
        eprintln!("vimcode --test-extension: {}", eb.message);
    }
    if let Some(mgr) = &eb.plugin_manager {
        for p in &mgr.plugins {
            if let Some(err) = &p.error {
                eprintln!("vimcode --test-extension: {} failed to load: {err}", p.name);
            }
        }
    }
}

/// Convert a `serde_json::Value` into an `mlua::Value` in `lua` — the
/// bridge [`run_spec`]'s `t.view()` uses to hand a
/// [`crate::core::plugin_ui::PluginView`] (already `Serialize`, see that
/// type's own doc) across from the engine's cache into the spec's
/// independent `Lua`. See this module's top doc, "The two Lua VMs".
fn json_to_lua<'lua>(lua: &'lua Lua, value: &serde_json::Value) -> LuaResult<LuaValue<'lua>> {
    match value {
        serde_json::Value::Null => Ok(LuaValue::Nil),
        serde_json::Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(LuaValue::Integer(i))
            } else {
                Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0)))
            }
        }
        serde_json::Value::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
        serde_json::Value::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.set((i + 1) as i64, json_to_lua(lua, item)?)?;
            }
            Ok(LuaValue::Table(t))
        }
        serde_json::Value::Object(map) => {
            let t = lua.create_table()?;
            for (k, v) in map {
                t.set(k.as_str(), json_to_lua(lua, v)?)?;
            }
            Ok(LuaValue::Table(t))
        }
    }
}

/// Register the whole `t.*` assertion API described in this module's doc
/// as a global Lua table named `t`.
fn install_assertion_api<A: quadraui::AppLogic + 'static>(
    lua: &Lua,
    harness: Rc<RefCell<Harness<A>>>,
    stub_bin_dir: PathBuf,
    spec_dir: PathBuf,
) -> LuaResult<()> {
    let t = lua.create_table()?;

    // t.keys("ihello<Esc>") — Neovim-notation key injection straight into
    // the engine via `Engine::feed_keys`, the same entry point the
    // production `vimcode.feedkeys` host API uses. This bypasses
    // shell/sidebar-level routing (`TuiBackend::translate_injected` →
    // `ShellApp::handle` → `App` dispatch), so it does not exercise
    // accelerators or other shell-routed key handling a real keystroke
    // would hit first — only `Engine`-level key handling.
    {
        let h = harness.clone();
        t.set(
            "keys",
            lua.create_function(move |_, keys: String| {
                let mut hb = h.borrow_mut();
                hb.engine.borrow_mut().feed_keys(&keys);
                hb.driver.render();
                Ok(())
            })?,
        )?;
    }

    // t.command(":w") — run an ex command (a leading `:` is optional).
    {
        let h = harness.clone();
        t.set(
            "command",
            lua.create_function(move |_, cmd: String| {
                let mut hb = h.borrow_mut();
                let cmd = cmd.strip_prefix(':').unwrap_or(&cmd);
                let _ = hb.engine.borrow_mut().execute_command(cmd);
                hb.driver.render();
                Ok(())
            })?,
        )?;
    }

    // t.wait_for(function() return condition end, timeout_secs) — polls
    // the engine's idle work (so async shells/terminals/timers a stub
    // process feeds actually advance) and re-renders between calls to
    // `predicate`, until it returns truthy or `timeout_secs` elapses.
    {
        let h = harness.clone();
        t.set(
            "wait_for",
            lua.create_function(move |_, (predicate, timeout): (LuaFunction, Option<f64>)| {
                let timeout = timeout.unwrap_or(5.0);
                let start = std::time::Instant::now();
                loop {
                    let ok: bool = predicate.call(())?;
                    if ok {
                        return Ok(true);
                    }
                    if start.elapsed().as_secs_f64() > timeout {
                        return Err(LuaError::RuntimeError(format!(
                            "t.wait_for: timed out after {timeout}s"
                        )));
                    }
                    {
                        let mut hb = h.borrow_mut();
                        hb.engine.borrow_mut().poll_idle();
                        hb.driver.render();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            })?,
        )?;
    }

    // t.screen() — the full rendered frame, one line per row.
    {
        let h = harness.clone();
        t.set(
            "screen",
            lua.create_function(move |_, ()| Ok(h.borrow().driver.screen()))?,
        )?;
    }

    // t.contains(text) — true if `text` appears anywhere on screen.
    {
        let h = harness.clone();
        t.set(
            "contains",
            lua.create_function(move |_, needle: String| {
                Ok(h.borrow().driver.screen().contains(&needle))
            })?,
        )?;
    }

    // t.click(x, y) — a left click at cell (x, y), through the driver's
    // real production click/hit-test path.
    {
        let h = harness.clone();
        t.set(
            "click",
            lua.create_function(move |_, (x, y): (f32, f32)| {
                let mut hb = h.borrow_mut();
                hb.driver.click(x, y);
                hb.driver.render();
                Ok(())
            })?,
        )?;
    }

    // t.click_text(text) — locate the first painted run containing `text`
    // and click its center. Errors if `text` isn't painted anywhere —
    // never guesses coordinates, matching this repo's own driver-test
    // convention (CLAUDE.md, "Testing").
    {
        let h = harness.clone();
        t.set(
            "click_text",
            lua.create_function(move |_, needle: String| {
                let mut hb = h.borrow_mut();
                let Some((x, y)) = hb.driver.find(&needle) else {
                    return Err(LuaError::RuntimeError(format!(
                        "t.click_text: {needle:?} not painted"
                    )));
                };
                hb.driver.click(x, y);
                hb.driver.render();
                Ok(())
            })?,
        )?;
    }

    // t.cell(x, y) — the character and style painted at one cell, or nil
    // if (x, y) is outside the viewport.
    {
        let h = harness.clone();
        t.set(
            "cell",
            lua.create_function(move |lua, (x, y): (u16, u16)| {
                let hb = h.borrow();
                let row = hb.driver.styled_row(y);
                match row.get(x as usize) {
                    Some((ch, style)) => {
                        let cell = lua.create_table()?;
                        cell.set("char", ch.to_string())?;
                        cell.set("fg", format!("{:?}", style.fg))?;
                        cell.set("bg", format!("{:?}", style.bg))?;
                        cell.set("bold", style.modifiers.contains(Modifier::BOLD))?;
                        cell.set("italic", style.modifiers.contains(Modifier::ITALIC))?;
                        cell.set("underline", style.modifiers.contains(Modifier::UNDERLINED))?;
                        Ok(LuaValue::Table(cell))
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;
    }

    // t.view(id) — the widget tree a `vimcode.ui.register_view(id, ...)`
    // extension is currently showing, re-rendered fresh. nil if `id`
    // names no registered view.
    {
        let h = harness.clone();
        t.set(
            "view",
            lua.create_function(move |lua, id: String| {
                let hb = h.borrow();
                hb.engine.borrow_mut().refresh_plugin_view(&id);
                let view = hb.engine.borrow().plugin_views.get(&id).cloned();
                match view {
                    Some(view) => {
                        let json = serde_json::to_value(&view)
                            .map_err(|e| LuaError::RuntimeError(format!("t.view: {e}")))?;
                        json_to_lua(lua, &json)
                    }
                    None => Ok(LuaValue::Nil),
                }
            })?,
        )?;
    }

    // t.stub_command(name, path) — see this module's top doc,
    // "Process stubbing". A relative `path` resolves against the spec
    // file's own directory, not the harness process's cwd, so a spec
    // can name a fixture alongside itself (`t.stub_command("kubectl",
    // "fixtures/fake_kubectl.lua")`) regardless of where `vimcode
    // --test-extension` was invoked from.
    {
        let dir = stub_bin_dir.clone();
        let base = spec_dir.clone();
        t.set(
            "stub_command",
            lua.create_function(move |_, (name, path): (String, String)| {
                let path = Path::new(&path);
                let resolved = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    base.join(path)
                };
                stub_command(&dir, &name, &resolved)
                    .map_err(|e| LuaError::RuntimeError(format!("t.stub_command({name:?}): {e}")))
            })?,
        )?;
    }

    lua.globals().set("t", t)
}
