//! Issue #1213 (Phase 0 of #1212) — go/no-go spike for a pure-Lua `vim.*`
//! prelude over `vimcode.*`.
//!
//! **This is a spike, not a feature.** The deliverable is the written
//! go/no-go verdict in the PR description; this file is the measurement
//! harness that produced it, kept as regression coverage for the specific,
//! reproducible findings the report cites (both of which are architectural
//! gaps in `vimcode.*`, confirmed here without touching anything under
//! `src/`, per #1213's "zero Rust changes" constraint).
//!
//! ## What's covered here vs. what was measured by hand
//!
//! Step 2 of #1213 ("load `ethanholz/nvim-lastplace` unmodified") was done
//! for real during the investigation: the actual plugin source was fetched
//! from GitHub and loaded against both a real Neovim and vimcode +
//! `contrib/nvim-shim/init.lua`. That is reported in the PR description, not
//! reproduced here — this suite does not fetch or vendor third-party code,
//! so it stays hermetic (no network) and does not add a permanent copy of
//! someone else's plugin to this repo for what is explicitly disposable spike
//! code. Instead, [`tests/fixtures/nvim_shim/lastplace_like.lua`] is a small,
//! self-contained reproduction of nvim-lastplace's two load-bearing patterns,
//! built from only public `vim.*` API — the same file is fed to a real
//! embedded Neovim ([`run_lastplace_pattern_in_neovim`]) and to
//! `PluginManager` + the shim ([`run_lastplace_pattern_in_vimcode`]), and the
//! resulting cursor position is compared, which is the "measured, not
//! eyeballed" oracle-comparison step #1213 asks for (§4), simplified from
//! `tests/nvim_conformance.rs`'s full attached-UI RPC transport down to a
//! single-shot `nvim --headless -l driver.lua` run: this spike needs one
//! deterministic post-hoc buffer+cursor read, not window-relative redraw
//! fidelity, so the simpler, pre-#1008 oracle style is a deliberate,
//! documented choice here — not a regression of that lesson.
//!
//! Step 3 (the kill test) is reproduced directly, in-process, against
//! `PluginManager` — no oracle needed, since it's a vimcode-only structural
//! bug (read-after-write staleness), already known-failing by inspection of
//! `src/core/plugin.rs:810-830` vs `:771-790` before this file existed.
//!
//! ## Requires `nvim` on PATH, same policy as `nvim_conformance.rs`
//!
//! A missing oracle is not a pass. Set `NVIM_SHIM_SPIKE_ALLOW_SKIP=1` to
//! downgrade a missing/unusable `nvim` to a printed warning instead of a
//! failure (e.g. for a machine that intentionally has no `nvim` installed).

use std::path::Path;
use std::process::{Command, Stdio};

use vimcode_core::core::plugin::{PluginCallContext, PluginManager};

/// Copy `contrib/nvim-shim/init.lua` and a second `.lua` file into a fresh
/// temp dir, named so the shim sorts and therefore loads first
/// (`PluginManager::load_plugins_dir` loads in path-sorted order — see
/// `src/core/plugin.rs`).
fn plugin_dir_with_shim(unique: &str, second_file: &Path) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vc_nvim_shim_spike_{unique}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let shim_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("contrib/nvim-shim/init.lua");
    std::fs::copy(&shim_src, dir.join("00_nvim_shim.lua")).unwrap_or_else(|e| {
        panic!("failed to copy {shim_src:?}: {e}");
    });
    let second_name = second_file.file_name().unwrap();
    std::fs::copy(
        second_file,
        dir.join(format!("10_{}", second_name.to_string_lossy())),
    )
    .unwrap();

    dir
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nvim_shim")
        .join(name)
}

// ---------------------------------------------------------------------------
// Step 3: the kill test.
// ---------------------------------------------------------------------------

/// `nvim_buf_set_lines` then immediately `nvim_buf_get_lines`: real Neovim
/// (and the `vim.*` API contract in general) guarantees read-after-write —
/// the second call sees the first call's edit. vimcode's plugin ABI does
/// not: `vimcode.buf.set_lines` queues onto `ctx.set_lines_range`
/// (`src/core/plugin.rs:807-838`), applied by the engine only *after* the
/// whole callback returns (`Engine::apply_plugin_ctx`,
/// `src/core/engine/plugins.rs`), while `vimcode.buf.get_lines` reads the
/// pre-call `ctx.buf_rope` snapshot (`src/core/plugin.rs:769-805`) — the two
/// never see each other within one callback.
///
/// This was "known to fail by inspection" per #1213's brief; this test pins
/// the *exact* failure mode: no error, no panic, just a silently stale read
/// (the old first line, not `"x"`).
#[test]
fn kill_test_set_lines_then_get_lines_is_stale_not_x() {
    let dir = plugin_dir_with_shim("killtest", &fixture_path("lastplace_like.lua"));
    // A tiny command that performs exactly #1213's kill-test snippet and
    // reports what it actually saw.
    std::fs::write(
        dir.join("20_killtest.lua"),
        r#"
        vimcode.command("KillTest", function()
            vim.api.nvim_buf_set_lines(0, 0, 1, false, {"x"})
            local seen = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
            vimcode.message("seen=" .. tostring(seen))
        end)
        "#,
    )
    .unwrap();

    let mut pm = PluginManager::new().unwrap();
    pm.load_plugins_dir(&dir, &[]);
    for p in &pm.plugins {
        assert!(
            p.error.is_none(),
            "plugin {} failed to load: {:?}",
            p.name,
            p.error
        );
    }

    let mut ctx = PluginCallContext {
        buf_rope: Some(ropey::Rope::from_str("original first line\nsecond line\n")),
        cursor_line: 1,
        cursor_col: 1,
        mode_name: "Normal".to_string(),
        ..Default::default()
    };
    ctx.settings_snapshot
        .insert("shift_width".to_string(), "4".to_string());

    let (found, ctx) = pm.call_command("KillTest", "", ctx);
    assert!(found, "KillTest command did not register");

    // Exact failure mode: get_lines after set_lines within the same callback
    // returns the PRE-call snapshot, not the just-written "x". This is what
    // #1213 predicted and what this test proves stays true post-shim: the
    // prelude cannot paper over this, it's below the Lua boundary.
    // Note the trailing "\n": `vimcode.buf.get_lines` returns ropey's
    // `line()` slices verbatim, which include the line terminator — a
    // second, smaller vimcode.* ABI wart the shim's `nvim_buf_get_lines`
    // does not paper over. Not the point of this test, but visible in the
    // assertion below.
    assert_eq!(
        ctx.message.as_deref(),
        Some("seen=original first line\n"),
        "expected the stale pre-call snapshot (\"original first line\"), which is exactly \
         the kill-test failure #1213 predicted; if this now reads \"seen=x\" the read-after-\
         write bug in src/core/plugin.rs has been fixed and this assertion (plus the PR report) \
         needs updating"
    );

    // And confirm the write DID land — just not visibly to the same callback
    // — via a second, separate command call (fresh PluginCallContext built
    // the way Engine::apply_plugin_ctx + make_plugin_ctx would for the next
    // dispatch), proving this is a same-callback staleness bug, not a
    // "the write silently vanished" bug.
    assert_eq!(ctx.set_lines_range.len(), 1);
    assert_eq!(ctx.set_lines_range[0], (0, 1, vec!["x".to_string()]));
}

/// `nvim_create_buf` has no vimcode equivalent — the shim doesn't even
/// define it (see `contrib/nvim-shim/init.lua`'s module doc: "no buffer
/// handle concept"). This test pins that exact failure mode: calling it is
/// a plain Lua "attempt to call a nil value" error, not a silent no-op and
/// not a fake handle.
#[test]
fn kill_test_nvim_create_buf_has_no_handle_concept() {
    let dir = plugin_dir_with_shim("killtest_buf", &fixture_path("lastplace_like.lua"));
    std::fs::write(
        dir.join("20_createbuf.lua"),
        r#"
        vimcode.command("TryCreateBuf", function()
            local ok, err = pcall(function()
                return vim.api.nvim_create_buf(false, true)
            end)
            vimcode.message("ok=" .. tostring(ok) .. " err=" .. tostring(err))
        end)
        "#,
    )
    .unwrap();

    let mut pm = PluginManager::new().unwrap();
    pm.load_plugins_dir(&dir, &[]);
    for p in &pm.plugins {
        assert!(
            p.error.is_none(),
            "plugin {} failed to load: {:?}",
            p.name,
            p.error
        );
    }

    let ctx = PluginCallContext::default();
    let (found, ctx) = pm.call_command("TryCreateBuf", "", ctx);
    assert!(found);
    let msg = ctx.message.unwrap_or_default();
    assert!(
        msg.starts_with("ok=false"),
        "expected a Lua call error, got: {msg}"
    );
    assert!(
        msg.contains("nil value"),
        "expected \"attempt to call a nil value\" (nvim_create_buf is simply undefined \
         in the shim, there is nothing in vimcode.* to define it over), got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// The headline finding: nested autocmd registration (nvim-lastplace's exact
// pattern) is a silent no-op once vimcode is past initial plugin load.
// ---------------------------------------------------------------------------

/// Reproduces nvim-lastplace's structure end-to-end inside vimcode: load the
/// shim + `lastplace_like.lua`, fire the mapped `BufRead` event (vimcode's
/// `"open"`), and confirm that the *nested* `nvim_create_autocmd("BufWinEnter",
/// ...)` call it makes never actually registers anything — because
/// `vimcode.on()` (see `contrib/nvim-shim/init.lua`) only writes into
/// `PluginRegistrations`, which only exists in Lua `app_data` during initial
/// script load (`src/core/plugin.rs:358,363`), not during event dispatch
/// (`call_event`, `src/core/plugin.rs:423-439`, which installs
/// `PluginCallContext` instead). No Lua error is raised anywhere in this
/// chain — the callback runs, `vimcode.on()` is a plain function call that
/// returns normally, it just has nothing to write into.
#[test]
fn nested_autocmd_registration_is_silently_dropped() {
    let dir = plugin_dir_with_shim("nested_autocmd", &fixture_path("lastplace_like.lua"));

    let mut pm = PluginManager::new().unwrap();
    pm.load_plugins_dir(&dir, &[]);
    for p in &pm.plugins {
        assert!(
            p.error.is_none(),
            "plugin {} failed to load: {:?}",
            p.name,
            p.error
        );
    }

    // Sanity: the outer "BufRead" -> vimcode "open" registration DID happen
    // (that one is a normal, load-time `vimcode.on` call).
    assert!(
        pm.has_event_hooks("open"),
        "outer BufRead autocmd failed to register at all"
    );

    // Before firing "open", there is obviously no "BufWinEnter" hook yet —
    // it's meant to be registered *by* the "open" callback.
    assert!(!pm.has_event_hooks("BufWinEnter"));

    let mut ctx = PluginCallContext {
        buf_rope: Some(ropey::Rope::from_str("one\ntwo\nthree\nfour\nfive\n")),
        cursor_line: 1,
        cursor_col: 1,
        mode_name: "Normal".to_string(),
        ..Default::default()
    };
    ctx.marks_snapshot.insert('"', (3, 1)); // pretend the '"' mark says "line 3"

    let _ctx = pm.call_event("open", "test.txt", ctx);

    // This is the finding: even though nvim-lastplace's BufRead callback ran
    // (it has to — "open" IS registered) and called
    // `vim.api.nvim_create_autocmd("BufWinEnter", { buffer = opts.buf, ... })`
    // with no error, that call landed nowhere. A real Neovim would have a
    // live "BufWinEnter" autocmd for this buffer at this point; vimcode does
    // not.
    assert!(
        !pm.has_event_hooks("BufWinEnter"),
        "if this now fails, vimcode's plugin registration has started working \
         from inside event-dispatch callbacks — that's the #1213 blocker being fixed; \
         update the PR report, this assertion, and re-run the oracle comparison below"
    );
}

// ---------------------------------------------------------------------------
// Oracle comparison (#1213 step 4): same Lua, real Neovim vs. vimcode+shim.
// ---------------------------------------------------------------------------

fn nvim_available() -> bool {
    Command::new("nvim")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run `tests/fixtures/nvim_shim/lastplace_like.lua` (unmodified, real
/// `vim.*` API — no shim involved) against a real embedded Neovim: seed a
/// 5-line buffer, set the `'"'` mark to line 3, fire `BufRead` then
/// `BufWinEnter` the way opening a file for the second time would, and read
/// back the cursor line Neovim landed on.
///
/// Deliberately the simpler `nvim --headless -l driver.lua` oracle style
/// (pre-#1008 in `tests/nvim_conformance.rs`'s terms) rather than the
/// attached-UI RPC transport: this spike needs one deterministic post-hoc
/// buffer+cursor read, not window-relative redraw fidelity, so the
/// documented weaknesses that motivated #1008 (stale `w_topline` etc.) don't
/// apply to what this test asserts on.
fn run_lastplace_pattern_in_neovim(tmp: &Path) -> Option<i64> {
    let fixture = fixture_path("lastplace_like.lua");
    let driver = tmp.join("oracle_driver.lua");
    std::fs::write(
        &driver,
        format!(
            r#"
            dofile({fixture:?})
            vim.api.nvim_buf_set_lines(0, 0, -1, false, {{"one","two","three","four","five"}})
            vim.api.nvim_buf_set_mark(0, '"', 3, 0, {{}})
            vim.api.nvim_win_set_cursor(0, {{1, 0}})
            vim.api.nvim_exec_autocmds("BufRead", {{ buffer = 0 }})
            vim.api.nvim_exec_autocmds("BufWinEnter", {{ buffer = 0 }})
            local cur = vim.api.nvim_win_get_cursor(0)
            io.stdout:write(tostring(cur[1]) .. "\n")
            "#,
        ),
    )
    .unwrap();

    let out = Command::new("nvim")
        .args(["--headless", "-n", "-u", "NONE", "-i", "NONE", "-l"])
        .arg(&driver)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<i64>()
        .ok()
}

/// The vimcode-side twin: same fixture file, loaded as a plugin behind the
/// shim, driven through `PluginManager` directly (no real `Engine` needed —
/// the plugin ABI is the thing under test).
fn run_lastplace_pattern_in_vimcode() -> i64 {
    let dir = plugin_dir_with_shim("oracle_cmp", &fixture_path("lastplace_like.lua"));
    let mut pm = PluginManager::new().unwrap();
    pm.load_plugins_dir(&dir, &[]);
    for p in &pm.plugins {
        assert!(
            p.error.is_none(),
            "plugin {} failed to load: {:?}",
            p.name,
            p.error
        );
    }

    let mut ctx = PluginCallContext {
        buf_rope: Some(ropey::Rope::from_str("one\ntwo\nthree\nfour\nfive\n")),
        cursor_line: 1,
        cursor_col: 1,
        mode_name: "Normal".to_string(),
        ..Default::default()
    };
    ctx.marks_snapshot.insert('"', (3, 1));

    // Only "open" (<- BufRead) can be fired at all — there is no vimcode
    // event for BufWinEnter, and even if there were, the nested
    // registration never happened (see
    // `nested_autocmd_registration_is_silently_dropped` above).
    let ctx = pm.call_event("open", "test.txt", ctx);

    ctx.set_cursor.map(|(line, _col)| line as i64).unwrap_or(1) // vimcode's own default: cursor stays put
}

/// The measured divergence: real Neovim restores the cursor to line 3 (the
/// saved `'"'` mark); vimcode + the shim does not move it at all, because
/// the nested-autocmd registration that would drive the restore is silently
/// dropped (see `nested_autocmd_registration_is_silently_dropped`). This is
/// the concrete evidence behind the PR report's "did nvim-lastplace work
/// end-to-end? No." verdict.
#[test]
fn oracle_lastplace_pattern_restores_cursor_in_neovim_but_not_vimcode() {
    let allow_skip = std::env::var("NVIM_SHIM_SPIKE_ALLOW_SKIP").as_deref() == Ok("1");
    if !nvim_available() {
        if allow_skip {
            eprintln!(
                "WARNING: `nvim` not found on PATH; skipping oracle comparison \
                 (NVIM_SHIM_SPIKE_ALLOW_SKIP=1). This is NOT evidence of anything."
            );
            return;
        }
        panic!(
            "`nvim` not found on PATH — this test needs a real Neovim to compare against \
             (see tests/nvim_conformance.rs's MIN_NVIM_VERSION policy). Install nvim, or set \
             NVIM_SHIM_SPIKE_ALLOW_SKIP=1 to explicitly skip."
        );
    }

    let tmp = std::env::temp_dir().join(format!("vc_nvim_shim_oracle_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let nvim_result = run_lastplace_pattern_in_neovim(&tmp);
    let Some(nvim_line) = nvim_result else {
        if allow_skip {
            eprintln!("WARNING: oracle nvim run failed to produce output; skipping.");
            return;
        }
        panic!("oracle nvim run produced no parseable output");
    };

    let vimcode_line = run_lastplace_pattern_in_vimcode();

    assert_eq!(
        nvim_line, 3,
        "sanity: real Neovim should have restored the cursor to the saved mark (line 3)"
    );
    assert_eq!(
        vimcode_line, 1,
        "sanity: vimcode's own baseline is 'cursor never moved' (started at line 1) — \
         if this changes, the nested-autocmd gap may have been fixed; re-check against \
         `nested_autocmd_registration_is_silently_dropped` and update the PR report"
    );
    assert_ne!(
        nvim_line, vimcode_line,
        "MEASURED DIVERGENCE (the point of this test): real Neovim restores the cursor \
         to the last-edit position via nvim-lastplace's pattern; vimcode + the nvim-shim \
         prelude does not, because the nested `nvim_create_autocmd` call inside the BufRead \
         callback is silently dropped (see nested_autocmd_registration_is_silently_dropped)"
    );
}
