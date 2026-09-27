//! Native Win-GUI (Direct2D + DirectWrite) backend — a **wrapper**, not a
//! backend (#866, the Win-GUI twin of #859's `src/macos/mod.rs`, stage 3 of
//! the north star's #7 milestone).
//!
//! quadraui already ships the whole Win-GUI backend: `WinBackend` implements
//! every `Backend` trait method `AppShell` renders through, and
//! [`quadraui::win::shell_runner::run_with_shell`] composes it with the
//! shared `ShellAdapter` exactly the way `quadraui::gtk::shell_runner` /
//! `quadraui::macos::shell_runner` / `quadraui::tui::shell_runner` do
//! (quadraui#465, quadraui#707 for the Win-GUI leg). So there is nothing to
//! rasterise here and nothing to decide here: this file is the Win32
//! sibling of `src/gtk/mod.rs::run` / `src/macos/mod.rs::run`, and it is
//! deliberately the only thing in `src/win/` besides the backend
//! re-export (`src/win/backend.rs`).
//!
//! **It contains no layout, hit-test, paint or dispatch decision** — the
//! acceptance bar #859 set and #866 inherits verbatim. All of that lives in
//! `crate::app::App`, whose single `impl quadraui::ShellApp` every GUI entry
//! point runs; `grep -rn 'impl.*ShellApp for' src/` still returns one GUI
//! implementation, not three. If you find yourself about to add a decision
//! here, `CLAUDE.md`'s Platform-Neutrality Rule says stop: the gap belongs
//! in quadraui or in `crate::app`, not in a backend directory.
//!
//! # Unlike `src/macos/mod.rs`: no `PlatformWindowHandle` impl
//!
//! quadraui's `win` module (pinned rev `9eede7fd`) exposes no public
//! top-level-window handle — `WinBackend`'s `hwnd` field is private and
//! `target_os = "windows"`-gated, and nothing else in `win::run`/
//! `win::services` hands one back. Window *discovery* has no portable
//! equivalent on this backend either (same gap `src/macos/mod.rs` has for
//! `MacBackend` — see `src/app.rs`'s `PlatformWindowHandle` doc comment and
//! the `#866` note just below its GTK impl). `App::new_portable` already
//! leaves `window: None` for exactly this reason, so nothing here needs to
//! change to accommodate it.
//!
//! # Why `feature = "win"` alone, not target-gated like `macos`
//!
//! `macos` is gated `#[cfg(all(feature = "macos", target_os = "macos"))]`
//! in both quadraui's `lib.rs` and this crate's, because quadraui's own
//! `macos` module is gated the same double way — a Linux host compiles NONE
//! of it under `--features macos`. quadraui's `win` module is different by
//! design (see that module's own doc comment): it is gated on `feature =
//! "win"` alone, with every real WinAPI call *inside* it individually
//! `cfg(target_os = "windows")`-gated and falling back to a `todo!()` stub
//! everywhere else — specifically so `cargo check --features win` type-checks
//! `WinBackend` on an ordinary Linux CI runner (see quadraui's `ci.yml`
//! "Compile check (win feature)" step). `src/win/backend.rs` and this file
//! inherit that same posture: un-target-gated, so a plain Linux
//! `cargo build --no-default-features --features win` compiles this module
//! (proving the vimcode-side wrapper is well-typed) even though *running*
//! it productively needs an actual Windows target — [`run`] below only
//! becomes real once `quadraui::win::run::run_with` is invoked on
//! `target_os = "windows"`; off Windows it hits that function's own
//! `todo!()` stub instead of silently doing nothing.
//!
//! `src/main.rs`'s `COMPILED_GUI_BACKEND` selection *does* add a
//! `target_os = "windows"` check on top of `feature = "win"` before ever
//! calling [`run`] — not because this module needs it to compile, but
//! because calling it without a live Windows message loop underneath would
//! panic instead of degrading gracefully, the same reasoning that makes
//! `macos` win over `gui` in that file's precedence rule.
//!
//! # Verifying this file without cargo-xwin
//!
//! A plain `cargo check --no-default-features --features win` on any host
//! (Linux included) type-checks every line below — no cross-toolchain
//! needed, per the module doc above. Producing (and running) a real PE32+
//! binary needs `cargo xwin build --target x86_64-pc-windows-msvc
//! --no-default-features --features win` plus the `clang-cl`/`lld-link`/xwin
//! SDK toolchain #866 pins to the `windows`-capability host for — see that
//! issue's "Machine" section and `Cargo.toml`'s `win` feature comment for
//! why `mlua`'s vendored Lua C build is the thing that actually requires
//! the cross C toolchain, not anything in this file.
//!
//! # #1558: activity-bar / tab-bar Nerd Font glyphs are a quadraui gap, not
//! # a vimcode-side one
//!
//! vimcode#1558 reports activity-bar icons painting as ASCII placeholder
//! characters and tab-bar file icons painting as a generic glyph on
//! Win-GUI, despite Nerd Font Icons being on. Investigated (root cause
//! confirmed for the activity-bar half, hypothesised for the tab-bar half)
//! and found to be entirely inside `quadraui::win::activity_bar`/
//! `quadraui::win::text` — see `docs/PENDING_QUADRAUI_ISSUES.md`'s two new
//! entries for the full analysis. Nothing in this file or `backend.rs`
//! changes: `App::setup` already calls `render::register_nerd_font_fallback`
//! identically for every backend (this module has no special-cased font
//! registration to add), and per the Platform-Neutrality Rule a Win-GUI-only
//! icon-selection fix does not belong in a vimcode backend wrapper. Leave
//! #1558 open until the quadraui issues are filed and land.
//!
//! **Real-hardware verification was attempted on dell64** per #1558's own
//! "Verify on real Windows" section (which correctly notes a Windows host
//! *does* exist in this fleet). What was actually run, in order:
//! 1. `cargo xwin build --release --target x86_64-pc-windows-msvc
//!    --no-default-features --features win --bin vimcode` — succeeded, real
//!    PE32+ exe.
//! 2. The resulting `vimcode.exe` launched **directly** via WSL interop
//!    (`"$EXE" --version`, not wine) — succeeded, printed the real version
//!    banner including the quadraui rev, and (launched with a file argument)
//!    created a real `HWND` on dell64's actual Windows desktop (confirmed
//!    via `Get-Process | Select MainWindowHandle` from PowerShell and a
//!    `PrintWindow` capture that shows the real DWM-drawn titlebar chrome
//!    with the app's own taskbar icon rendering correctly).
//! 3. `cargo xwin test --release --target x86_64-pc-windows-msvc
//!    --no-default-features --features win --lib --no-run`, then the printed
//!    `vimcode_core-*.exe` run directly (the recipe #1558 gives for the
//!    `win_driver_tests` module below) — this is where verification stalled,
//!    for two independent, dell64-specific reasons, neither of which is "no
//!    Windows host":
//!    - The lib test binary as built on this branch didn't even compile at
//!      first: `poll_until_auth_choice_dialog` in
//!      `src/core/engine/acp_ops.rs` was missing the `#[cfg(unix)]` its sole
//!      callers and its own `poll_acp_until` helper already carry — a
//!      pre-existing bug unrelated to #1558, fixed alongside this commit.
//!    - Once compiling, the resulting `vimcode_core-*.exe` — unlike
//!      `vimcode.exe` above — exits immediately on dell64 with
//!      `STATUS_ENTRYPOINT_NOT_FOUND` (confirmed via
//!      `Start-Process -PassThru`'s real `ExitCode`, `-1073741511` /
//!      `0xC0000139`) before printing anything, even `--version`/`--help`.
//!      A from-scratch minimal `cargo xwin test --no-run` crate (a single
//!      `#[test] fn it_works()`) runs fine directly via the same WSL interop
//!      path on the same host, so this is not a generic
//!      "WSL can't run cross-built test binaries" limitation — it is
//!      specific to vimcode's own `--lib` test binary (44 MB, statically
//!      links `mlua`'s vendored Lua C build, tree-sitter grammars, and
//!      every `#[cfg(test)]` module in the crate) and needs its own
//!      follow-up investigation on real hardware, separate from #1558.
//!    - Pixel-level confirmation via screenshot was also attempted directly
//!      against the running `vimcode.exe` GUI window (both `BitBlt`-based
//!      `Graphics.CopyFromScreen` and `PrintWindow` with
//!      `PW_RENDERFULLCONTENT`), but dell64's interactive console session
//!      (session 1, confirmed active and holding a real `\\.\DISPLAY1`) was
//!      locked at the OS level during this session (`Get-Process -Name
//!      logonui` returned a running process, the definitive signal) —
//!      Windows blocks GDI screen/window-content capture on a locked
//!      session regardless of what's actually painted underneath, which is
//!      why `CopyFromScreen` came back solid black and `PrintWindow` only
//!      returned the DWM-drawn titlebar chrome, not the Direct2D-painted
//!      client area. This is an OS security restriction independent of
//!      vimcode/quadraui rendering correctness, not evidence either way for
//!      the reported bug.
//!
//! Net: the activity-bar root cause above is confirmed by source inspection
//! (cross-referenced against the exact pinned quadraui rev) and independently
//! corroborated by dell64 successfully building and directly launching the
//! real `vimcode.exe`; the acceptance bar's "Windows test asserts a real
//! font face" criterion remains genuinely blocked on dell64 by the two
//! dell64-local issues above (test-binary loader crash; locked interactive
//! session blocking screen capture), not by absence of a Windows host.
//!
//! # #1561: left-edge desktop strip — no quadraui source-level defect found;
//! # live pixel verification stayed blocked, and a new dell64-local wrinkle
//! # surfaced along the way
//!
//! vimcode#1561 reports a thin (~6px) strip along the window's left edge
//! showing the desktop through it. The two hypotheses the issue text names
//! ("non-client insets subtracted twice, or physical vs. logical pixels")
//! were checked directly against `quadraui::win::backend::WinBackend`
//! (pinned rev `9f8766d3`), `attach_surface` and `resize_surface`
//! specifically:
//!
//! ```text
//! let mut rect = RECT::default();
//! GetClientRect(hwnd, &mut rect)?;
//! let width = (rect.right - rect.left).max(1) as u32;
//! let height = (rect.bottom - rect.top).max(1) as u32;
//! // ... single D2D_SIZE_U { width, height } fed straight to
//! // CreateHwndRenderTarget; resize_surface's WM_SIZE-driven `Resize`
//! // call is the same single width/height pair, no per-edge split.
//! ```
//!
//! Both call sites derive the render target's pixel size from one
//! un-split `GetClientRect`/`WM_SIZE` pair — there is no separate
//! left/right/top/bottom computation anywhere in `win::backend`/`win::run`
//! for either to double-subtract, and no DIP↔physical conversion happens
//! on the render-target size itself (`dpi_scale` only scales
//! `Viewport`/hit-testing, never the `D2D_SIZE_U` passed to
//! `CreateHwndRenderTarget`/`Resize`). So neither hypothesised bug shape
//! exists in the source as pinned.
//!
//! **Real-hardware verification was attempted on dell64** (this fleet's
//! real Windows 11 host) per this issue's own "Verify on real Windows"
//! section, going one step further than #1558/#1559's attempts:
//! 1. `cargo xwin build --release --target x86_64-pc-windows-msvc
//!    --no-default-features --features win --bin vimcode` succeeded, and
//!    the resulting `vimcode.exe` was launched **directly** (WSL2 interop,
//!    not wine) against a real `HWND` on dell64's desktop, confirmed via
//!    `Get-Process -PassThru`'s real `MainWindowHandle`.
//! 2. dell64's interactive session was, again, independently confirmed
//!    locked (`Get-Process -Name logonui` running in the same session
//!    `query session` reports as the active console session;
//!    `GetForegroundWindow()` returns `NULL`) — the same blocker
//!    #1558/#1559 hit, not "no Windows host in this fleet".
//!    `Graphics.CopyFromScreen` against the live window rect came back
//!    solid black, exactly as those two issues' docs already record.
//! 3. **New this session:** `PrintWindow` with `PW_RENDERFULLCONTENT`
//!    (the workaround #1558 tried and found returned only DWM chrome) was
//!    retried and this time *did* return real client-area pixels — but
//!    cross-checking `GetWindowRect`/`GetClientRect`/
//!    `DWMWA_EXTENDED_FRAME_BOUNDS` against the captured bitmap showed:
//!    - The gap between `GetWindowRect` and the first painted (non-black)
//!      column/row was **identical (8 physical px) on the left, right,
//!      and bottom edges** of a freshly-launched, untouched window — i.e.
//!      symmetric, not left-specific — and matches Windows' own
//!      documented invisible `WS_THICKFRAME` resize-border hit-test
//!      margin present on every classic-style top-level window, which
//!      `DWMWA_EXTENDED_FRAME_BOUNDS` (the actually-visible, DWM-composited
//!      frame) already excludes: the painted content's on-screen position
//!      lined up with `DWMWA_EXTENDED_FRAME_BOUNDS`'s edges to within 1px
//!      on every side. Nothing about that measurement points at a
//!      left-specific defect, in the source or on the glass.
//!    - Repeating the capture after `SW_MAXIMIZE`, then again after an
//!      external `SetWindowPos` to a new size (`GetClientRect` itself
//!      *did* update both times, proving Windows genuinely resized the
//!      window), and again after an explicit `InvalidateRect` +
//!      `UpdateWindow`, kept returning the **same painted content pinned
//!      at its original launch size** in the corner of the new, larger
//!      capture, never stretching to fill it. That is inconsistent with
//!      live re-rendering and far more consistent with a locked/secure-
//!      desktop session suspending real composition for a fully-occluded
//!      window — `PrintWindow(PW_RENDERFULLCONTENT)` reads from DWM's own
//!      redirection surface, and Microsoft does not document that surface
//!      as guaranteed-fresh for an occluded window. In other words: this
//!      workaround's pixel data cannot be trusted as "what a user would
//!      actually see" while dell64's session stays locked, so it can
//!      neither confirm nor rule out #1561's reported strip.
//!
//! **Net:** no left-specific defect exists in `win::backend`'s sizing
//! arithmetic as pinned, and no quadraui-side fix is proposed here — filing
//! a `docs/PENDING_QUADRAUI_ISSUES.md` entry needs a concrete ask, and
//! "the render-target sizing already looks correct" isn't one. What #1561
//! actually needs next is a **live, unlocked** dell64 session (or another
//! Windows host) at both 100% and 150% scaling, so a real DWM-composited
//! screenshot — not `PrintWindow`'s possibly-stale redirection-surface
//! read — can confirm whether the strip is real, and if so, isolate which
//! of `win::run`'s window-class/style choices (not sizing arithmetic, which
//! this session already cleared) produces it. Leave #1561 open.

use std::path::PathBuf;
use std::process::ExitCode;

pub(crate) mod backend;

use crate::app::App;

/// Entry point for the native Win-GUI, mirroring `crate::gtk::run` /
/// `crate::macos::run`.
///
/// Panic hook + swap flush, choose the backend, construct the shared
/// [`App`], derive its [`quadraui::ShellConfig`] via [`build_shell_config`]
/// (#866 — no per-backend copy of the panel/title-bar/sidebar-clamp logic
/// here, see `crate::gtk::build_shell_config`'s doc comment for why that
/// lives once, in `App::shell_config`, instead), hand both to the runner.
/// Nothing else — no `gtk4::init` equivalent, because `quadraui::win::run`'s
/// Win32 bootstrap (`RegisterClassExW`/`CreateWindowExW`/the message loop)
/// does its own setup inside `run_with_shell`.
pub fn run(file_path: Option<PathBuf>) -> ExitCode {
    // The same panic hook `crate::gtk::run` / `crate::macos::run` install:
    // flush every dirty buffer to its swap file, then write a crash log.
    crate::core::swap::install_gui_crash_hook();

    // The concrete backend is chosen here, at the entry point, and handed to
    // `App` — the seam #861 opened and `src/gtk/mod.rs::run` names in its
    // own comment as the one "a future non-GTK wrapper (#859) would pass a
    // different `quadraui::Backend` impl through". This is that wrapper's
    // Win-GUI sibling.
    let concrete_backend: std::rc::Rc<std::cell::RefCell<Box<dyn quadraui::Backend>>> =
        std::rc::Rc::new(std::cell::RefCell::new(
            Box::new(backend::WinBackend::new()),
        ));

    let app = App::new_portable(
        file_path,
        concrete_backend,
        crate::render::UnitProfile::px(),
    );
    let config = build_shell_config(&app);
    quadraui::win::shell_runner::run_with_shell(app, config)
}

/// Derive the runner's [`quadraui::ShellConfig`] from an [`App`]'s engine
/// state — the Win-GUI twin of `crate::gtk::build_shell_config` /
/// `crate::macos::build_shell_config`.
///
/// Adds only [`quadraui::ShellConfig::with_app_icon`] on top of
/// `app.shell_config()`: #1531/quadraui#1142's titlebar/taskbar icon.
/// `WM_SETICON` (`ICON_BIG`/`ICON_SMALL`) needs a decodable image handed in
/// explicitly — Win-GUI has no manifest icon resource here to fall back to.
/// Split out (rather than inlined in [`run`]) so a headless test can assert
/// the bytes reach `ShellConfig` without needing a live Win32 message loop.
pub(crate) fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    app.shell_config()
        .with_app_icon(quadraui::ImageSource::Bytes(
            crate::app_support::APP_ICON_PNG.to_vec(),
        ))
}

/// #1531/quadraui#1142: same reasoning as `crate::gtk`'s
/// `shell_config_identity_tests` / `crate::macos`'s
/// `shell_config_identity_tests` — no headless taskbar to render into and
/// assert on (that's the SMOKE_TESTS item, run on real Windows hardware),
/// but a headless build *can* assert the bytes reach the `ShellConfig` the
/// real `run` hands `run_with_shell`, and that they decode as a real image.
/// Runs on an ordinary Linux host under `cargo test --features win`, per
/// this module's own "Why `feature = "win"` alone" doc — `build_shell_config`
/// touches no WinAPI, only `quadraui::ShellConfig`.
#[cfg(all(test, feature = "win"))]
mod shell_config_identity_tests {
    use super::{build_shell_config, App};
    use crate::core::Engine;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn app_icon_reaches_shell_config_as_a_decodable_image() {
        let engine = Rc::new(RefCell::new(Engine::new_for_test()));
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
            Rc::new(RefCell::new(Box::new(super::backend::WinBackend::new())));
        let app = App::new_headless_with_backend(engine, backend, crate::render::UnitProfile::px());
        let config = build_shell_config(&app);
        let quadraui::ImageSource::Bytes(bytes) = config
            .app_icon
            .expect("build_shell_config sets an app icon")
        else {
            panic!("app icon should be embedded bytes, not a path");
        };
        assert!(!bytes.is_empty());
        assert!(
            image::load_from_memory(&bytes).is_ok(),
            "app icon bytes must decode as an image"
        );
    }
}

// ── #969: `quadraui::Backend` metric-setter conformance (`WinBackend`) ─────
//
// Unlike `win_driver_tests` below, this needs no `WinDriver`/
// `quadraui::win::testing` at all — only `WinBackend::new()` plus the two
// `set_current_line_height`/`set_current_char_width` setters
// (JDonaghy/quadraui#1086) and the `quadraui::Backend` getters they feed,
// none of which are `target_os`-gated (see this module's own "Why
// `feature = "win"` alone" doc above — `current_line_height`/
// `current_char_width` and their setters/getters are plain fields, not
// WinAPI calls). So this runs on an ordinary Linux host under `cargo test
// --features win`, with no Windows target and no cross toolchain — closing
// the same gap #967 found on macOS (a metric-setter override that silently
// no-ops, disabling the #540/#819 click drift guard) for this backend too,
// and doing it without needing Windows hardware to run at all. See
// `crate::harness::assert_text_metrics_backend_applies_metrics`'s doc for
// the full mechanism.
#[cfg(all(test, feature = "win"))]
mod win_backend_conformance {
    #[test]
    fn win_backend_applies_line_height_and_char_width() {
        let mut backend = super::backend::WinBackend::new();
        crate::harness::assert_text_metrics_backend_applies_metrics(&mut backend);
    }
}

// ── #928: `crate::harness::ConformanceHarness` on `WinDriver` ──────────────
//
// #928's AC2 ("`cargo check --no-default-features --features win`
// type-checks the Win [ConformanceHarness] instantiation on an ordinary
// Linux host") is met as of quadraui#1038 (landed in the rev this crate is
// pinned to — see `Cargo.toml`). Before that, `quadraui::win::testing` (the
// module holding `WinDriver`/`driver_with_shell`) was
// `#[cfg(target_os = "windows")]`-gated *inside* quadraui regardless of
// `feature = "win"` alone, unlike `quadraui::win::backend`/`run`/
// `shell_runner` — so this module had to carry a matching double gate
// (`#[cfg(target_os = "windows")]` on top of `#[cfg(test)]`), and there was
// no `cargo check`/`cargo check --tests`/`cargo test --no-run` invocation on
// Linux that could even *see* `WinDriver`, let alone type-check code
// constructing one. See `docs/PENDING_QUADRAUI_ISSUES.md`'s (now struck)
// entry for the full history of that gap and its ask.
//
// quadraui#1038 gated `win::testing` on `feature = "win"` alone instead,
// with every real Direct2D/GDI call individually `cfg(target_os =
// "windows")`-stubbed — the same "compiles-everywhere, only *works* on
// Windows" posture `win::backend`/`run`/`shell_runner` already had (see this
// module's own "Why `feature = "win"` alone" doc above). So this module now
// only needs `#[cfg(test)]`, no `target_os` gate, to type-check on Linux —
// `cargo check --tests --no-default-features --features win` reaches every
// line below, including the `ConformanceHarness<WinDriver<...>>`
// instantiations.
//
// Type-checking is not the same as running, though: `HeadlessSurface::new`
// (the offscreen Direct2D DC render target `WinDriver::new` `.expect()`s)
// always returns `Err` off Windows — there is no non-Windows Direct2D to
// build one from — so a scenario *executed* off Windows would panic rather
// than pass or fail meaningfully. Each `#[test]` attribute below is
// therefore itself `cfg_attr(target_os = "windows", test)`-gated: the
// function body (and everything it calls) is always compiled and
// type-checked, but it is only ever registered — and run — as an actual
// test on real Windows. `#[allow(dead_code)]` on the module (off Windows
// only) is the corollary: with no `#[test]` attribute reaching them, these
// functions have no caller on that host, which is expected, not a bug to
// silence away by deleting the bodies.
//
// Bodies mirror `src/macos/mod.rs::mac_driver_tests::conformance_proof_slice`
// exactly, `MacDriver`/`MacBackend` swapped for `WinDriver`/`WinBackend` — see
// that module for the scenarios' own doc comments (RED-verification notes,
// why scenario 3 clicks outside the popup rather than a specific row, …).
// RED-verification itself could not be run against `WinDriver`, but **not**
// for "no Windows host in this fleet" (dell64 runs a real Windows 11 desktop
// under WSL2 interop and is this repo's designated Windows machine — see
// #1558's "Verify on real Windows" section, which named and corrected this
// exact stale claim). `cargo xwin build --bin vimcode` and direct (non-wine)
// execution of the result both work fine on dell64 today. The blocker is
// narrower: as of #1558's investigation, the `cargo xwin test --lib --no-run`
// product crashes at Windows DLL-load time on dell64
// (`STATUS_ENTRYPOINT_NOT_FOUND`, reproduced and isolated to vimcode's own
// test binary — see `src/win/mod.rs`'s top-of-file `#1558` doc section for
// the full repro) before any `#[test]` in this module gets to run, and
// dell64's interactive session was independently locked (blocking pixel-level
// GUI capture) during that same investigation. GTK and macOS were both
// RED-verified on real hardware (a Linux lane and an `aarch64-apple-darwin`
// Mac mini respectively) against the identical vimcode-side mutation — see
// `src/macos/mod.rs`'s copy of this scenario for that note. Win-GUI needs the
// test-binary crash above fixed first.
#[cfg(all(test, feature = "win"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod win_driver_tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use quadraui::win::testing::driver_with_shell;

    use crate::core::Engine;
    use crate::harness::ConformanceHarness;

    fn plain_engine() -> Engine {
        let mut engine = Engine::new_for_test();
        engine.settings.use_nerd_fonts = Some(false);
        engine
    }

    fn conformance_harness(
        engine: Engine,
        width: u32,
        height: u32,
    ) -> ConformanceHarness<quadraui::win::testing::WinDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
            Rc::new(RefCell::new(Box::new(super::backend::WinBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(
            Rc::clone(&engine),
            backend,
            crate::render::UnitProfile::px(),
        );
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new(driver, engine, paint, cwd)
    }

    fn conformance_harness_with_folder_picker(
        engine: Engine,
        dir: PathBuf,
        width: u32,
        height: u32,
    ) -> ConformanceHarness<quadraui::win::testing::WinDriver<impl quadraui::AppLogic>> {
        let paint = crate::test_paint::PaintGuard::acquire();
        let cwd = crate::test_cwd::CwdReadGuard::acquire();
        let engine = Rc::new(RefCell::new(engine));
        let backend: Rc<RefCell<Box<dyn quadraui::Backend>>> =
            Rc::new(RefCell::new(Box::new(super::backend::WinBackend::new())));
        let (app, config) = crate::harness::build_app_and_config(
            Rc::clone(&engine),
            backend,
            crate::render::UnitProfile::px(),
        );
        crate::harness::install_folder_picker(&app, dir);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new(driver, engine, paint, cwd)
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vimcode_test_928_win_conformance_{tag}_{:?}",
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Scenario 1 (#928): open/filter/Esc-dismiss the folder picker via
    /// `WinDriver` — the identical body `crate::gtk::testing`'s and
    /// `src/macos/mod.rs`'s own copies run against `GtkDriver`/`MacDriver`.
    #[cfg_attr(target_os = "windows", test)]
    fn folder_picker_filters_and_escape_dismisses() {
        let dir = scratch_dir("scenario1");
        std::fs::create_dir_all(dir.join("kkxxqq_distinctive_928")).unwrap();
        std::fs::create_dir_all(dir.join("another_unrelated_dir_928")).unwrap();

        let mut h = conformance_harness_with_folder_picker(plain_engine(), dir.clone(), 1400, 900);

        crate::harness::folder_picker_filters_and_escape_dismisses(
            &mut h.driver,
            "kkxxqq_distinctive_928",
            "another_unrelated_dir_928",
            "kkxxqq_distinctive_928",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Scenario 2 (#928): the command palette's open/filter/Esc cycle, via
    /// `WinDriver`.
    #[cfg_attr(target_os = "windows", test)]
    fn command_palette_filters_and_escape_dismisses() {
        let mut h = conformance_harness(plain_engine(), 1400, 900);

        crate::harness::command_palette_filters_and_escape_dismisses(&mut h.driver);
    }

    /// Scenario 3 (#928): a click outside the open folder picker's popup
    /// must dismiss it, via `WinDriver::click`'s raw pixel-coordinate
    /// dispatch.
    #[cfg_attr(target_os = "windows", test)]
    fn folder_picker_click_outside_dismisses_it() {
        let dir = scratch_dir("scenario3");
        std::fs::create_dir_all(dir.join("kkxxqq_distinctive_928")).unwrap();
        std::fs::create_dir_all(dir.join("another_unrelated_dir_928")).unwrap();

        let (width, height) = (1400.0, 900.0);
        let mut h = conformance_harness_with_folder_picker(
            plain_engine(),
            dir.clone(),
            width as u32,
            height as u32,
        );

        crate::harness::folder_picker_click_outside_dismisses_it(
            &mut h.driver,
            "kkxxqq_distinctive_928",
            "another_unrelated_dir_928",
            width - 10.0,
            height - 10.0,
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
