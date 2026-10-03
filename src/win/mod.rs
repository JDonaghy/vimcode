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
//!
//! # #1562: Win-GUI chrome parity with macOS (font / title bar / status
//! # segments) — two of three items already resolved, the third is a real,
//! # substantial quadraui gap
//!
//! vimcode#1562 asks for three separate things. Investigated each against
//! the pinned quadraui rev (`9f8766d3`, unchanged from the `a58e5bec` pin
//! the issue's own real-hardware observation names — `a58e5bec` is 12
//! commits *behind* `9f8766d3` in quadraui's history, so nothing relevant
//! moved between the observation and this pin).
//!
//! **Item 1 (UI font resolves to a real Windows face, not literal
//! `"Monospace"`) — already fixed upstream, nothing to do here.**
//! `quadraui::win::backend::parse_ui_font_desc` and `WinBackend::
//! set_editor_font`/`set_ui_font` already resolve the fontconfig/Pango
//! generic alias via `GenericFamily::parse`: `Monospace` →
//! `DEFAULT_EDITOR_FONT_FAMILY` (`"Consolas"`), `SansSerif`/`SystemUi` →
//! `DEFAULT_UI_FONT_FAMILY` (`"Segoe UI"`) — confirmed present verbatim at
//! *both* `a58e5bec` and the current `9f8766d3` pin, so this was already
//! true at the moment the issue's Windows/macOS comparison was made, not
//! something this session's pin bump fixed. `App::sync_per_frame_backend_
//! state` (`src/app.rs`) calls `backend.set_ui_font(&UI_FONT())` and
//! `backend.set_editor_font(...)` unconditionally for every GUI backend —
//! no per-backend vimcode wiring gap either. The Settings sidebar showing
//! **Font Family: `Monospace`** is the intended, platform-neutral sentinel
//! value (`core::settings::default_font_family`'s own #1129/#1542 doc): it
//! displays the *raw stored setting*, not the backend-resolved face, and
//! macOS shows the identical literal string for the identical reason (its
//! own `font_family` default is the same shared `"Monospace"`) — this is
//! not Windows-specific behaviour and not a defect.
//!
//! **Item 3 (status-line segments: branch, encoding, line endings,
//! indentation, language, LSP) — already shared code, no per-backend
//! drop found.** `WinBackend::draw_status_bar_interactive` and
//! `MacBackend`'s equivalent both route through the *identical*
//! `quadraui::primitives::status_bar::native_surface_paint::paint`
//! rasteriser (the "NativeSurface Phase 4" migration folded status-bar
//! painting into one shared function for every `px()` backend), and
//! vimcode's own segment list (`render.rs`'s `StatusSegment` construction)
//! is already fully backend-neutral, with the same width-based
//! priority-drop (#164) on every backend. No source-level defect found
//! that would drop segments on Windows specifically; the narrower set
//! observed during the 2026-09-27 side-by-side is more likely a
//! window-width/DPI difference between the two compared windows than a
//! code path. Needs a real-hardware re-comparison at matched window widths
//! to confirm one way or the other, not a code change — left open pending
//! that, not closed.
//!
//! **Item 2 (custom title bar + command centre) — a real, confirmed
//! quadraui-side gap.** vimcode's own title-bar band and command centre
//! (`App::render_content`'s `FrameOp::CommandCenter` rung, `render::
//! build_command_center_view`/`paint_command_center_rung`) are fully
//! backend-neutral and gated only on the reserved band existing
//! (`RenderPresence::command_center`, `render.rs`) — not on which backend
//! is running — so the search box and its back/forward buttons *would*
//! already paint on Windows once the band is reliably live. What actually
//! blocks that:
//!
//! - `App::render_content`'s menu-bar-visibility decision is a three-way
//!   branch on `backend.backend_caps()`: `native_menu` (real OS menu bar,
//!   macOS) / `window_chrome` (drawn row pinned always-visible, doubling
//!   as the client-side titlebar — GTK today) / neither (fully toggleable,
//!   the TUI `cell`-profile posture, starting hidden outside vscode-mode).
//!   `quadraui::win::backend::WinBackend::backend_caps()` declares neither
//!   flag — confirmed by reading its struct literal at the pinned rev — so
//!   Win-GUI silently falls into the third, TUI-shaped arm, even though
//!   `App::render_content`'s own comment already names this gap ("GTK's
//!   (and any future Win-GUI's) drawn menu bar doubles as the client-side
//!   titlebar"). The practical effect: the band (and everything painted
//!   into it, menu row and command centre alike) starts hidden/toggleable
//!   rather than pinned visible, unlike GTK.
//! - `quadraui::win::run` (`win/run.rs`) creates its top-level window with
//!   plain `WS_OVERLAPPEDWINDOW` — the real Win32 caption, native
//!   min/max/close and resize border — with no `WM_NCCALCSIZE`/
//!   `WM_NCHITTEST` client-area extension the way a modern custom-caption
//!   Windows app needs to fold its own drawn band into that caption
//!   instead of stacking a second row underneath it. So even once the cap
//!   above is fixed, today's Windows chrome would still be a real native
//!   caption *plus* a separate drawn band underneath it, not the single
//!   "custom-drawn caption with native min/max/close" #1562 asks for.
//!
//! Both halves are quadraui infrastructure (`WinBackend::backend_caps`,
//! `win::run`'s window style/message handling), not a `src/win/` wrapper
//! decision — per the Platform-Neutrality Rule this is not a fix to
//! attempt here. Drafted as a pending quadraui issue in
//! `docs/PENDING_QUADRAUI_ISSUES.md` (new entry) rather than built in this
//! crate.
//!
//! **Update (#1618):** the pin then included quadraui#1199
//! (`WinBackend::backend_caps()` declares `window_chrome: true`,
//! `win::run`'s `wndproc` handles `WM_NCCALCSIZE`/`WM_NCHITTEST`) and the
//! `JDonaghy/quadraui#1213` reentrancy fix that unblocked bumping past it.
//! `App::render_content`'s three-way branch needed no vimcode-side change
//! to pick it up — but #1618 also carried quadraui#1200
//! (`native_menu: true`, for #1582), and `App::setup`'s
//! `if native_menu {...} else if window_chrome {...}` order (correct for
//! macOS, where `native_menu` means a true OS-global menu bar with zero
//! in-window footprint) always preferred the first arm — so Win-GUI never
//! actually reached `window_chrome`'s drawn-caption path. #1622's
//! real-hardware sessions on dell64 confirmed this: a stock native caption
//! plus a separate native/owner-drawn menu row underneath, the pre-#1562
//! look, with `WS_CAPTION` never cleared. Filed as the (now-struck)
//! `BackendCaps::native_menu is overloaded` entry in
//! `docs/PENDING_QUADRAUI_ISSUES.md`.
//!
//! **Resolved (#1629):** quadraui#1228 (`bc92d47`/`d292a4c`, the pin this
//! crate now carries) resolves the conflict by reverting the `native_menu`
//! half of #1200 on Win-GUI rather than adding the capability split the
//! pending-issue draft asked for: `WinBackend::backend_caps()` now
//! declares only `window_chrome: true` (matching `GtkBackend`), and
//! `install_menu_bar` is back to the trait's no-op default — a native
//! `HMENU` sits in the non-client area, which #1199's drawn caption
//! permanently covers, so #1200's native menu bar and window controls were
//! unreachable regardless. `App::setup`'s three-way branch needed no
//! vimcode-side change to pick this up: a Win backend now falls into the
//! same `window_chrome` arm GTK takes (drawn menu row pinned visible,
//! `menu_bar_visible = true`), and `capture_window_and_apply_csd`'s
//! `!native_menu` gate now lets `set_decorated(false)` run on Windows too,
//! clearing `WS_CAPTION` and handing the title strip to
//! `WM_NCCALCSIZE`/`WM_NCHITTEST`. The macOS `native_menu` arm is
//! untouched — `MacBackend` still declares it, and this bump does not
//! change that backend's caps at all. `win_driver_tests` below (this
//! module) replaces the two tests that pinned `native_menu: true` on
//! `WinBackend` with `window_chrome`-shaped ones mirroring GTK's own
//! drawn-menu-row/command-centre coverage.
//!
//! **Real-hardware verification (#1629):** on dell64, `cargo xwin build
//! --release --target x86_64-pc-windows-msvc --no-default-features
//! --features win` (`RUSTFLAGS="-C target-feature=+crt-static"`) succeeded;
//! `vimcode.exe --version` printed `VimCode 0.14.0 (quadraui d292a4c50347,
//! win)`, confirming the resolved pin. Launched directly via WSL2 interop
//! (not `PowerShell Start-Process`, which hung indefinitely for unrelated
//! reasons — likely UNC-path/Defender-scan overhead, not a vimcode/quadraui
//! defect): the process came up cleanly, a real `HWND` (title "VimCode",
//! `Responding: True`, no `%TEMP%\vimcode-crash.log`) — no repeat of
//! #1614/#1213's startup crash, since the `native_menu`/`SetMenu` path
//! that caused it no longer exists. `GetMenu(hwnd)` returned `0`/`NULL`,
//! confirming no native `HMENU` is attached, matching
//! `window_chrome_backend_paints_the_drawn_menu_row`'s `!native_menu`
//! precondition below. **`GetForegroundWindow()` returned `NULL`
//! throughout** (`Get-Process -Name logonui` running in session 1, the
//! active console session) — the identical locked-session signature
//! #1558/#1561/#1622 already document on this host. Per this issue's own
//! acceptance criteria, a locked session means pixel-level (or
//! pixel-dependent) results cannot be trusted: `GWL_STYLE` queried via
//! `GetWindowLongW` still showed `WS_CAPTION` set several seconds after
//! launch, which *could* mean `capture_window_and_apply_csd`'s
//! `set_decorated(false)` retry never got a chance to run — but that retry
//! rides the same per-frame `tick()`/paint loop #1622 already found stalls
//! under a locked session on this host, so this observation is
//! inconclusive, not a failure, and is **not** treated as a negative
//! result here. The drawn title bar, menu row, command centre, and
//! caption drag/min/max/close (#1562), the drawn row opening File/Edit/
//! View/... (#1582), and the NORMAL-mode block cursor showing the
//! character underneath it (#1559, `src/win/backend.rs`) all remain
//! **UNVERIFIED** — blocked by dell64's locked interactive session, not
//! resolved and not falsified by this session. A live, unlocked dell64
//! session is the only way to close any of the three.
//!
//! # #1582: no menu bar at startup on Win-GUI — the same `backend_caps`
//! # gap #1562 found, but the narrower `native_menu` half of it
//!
//! vimcode#1582 reports `vimcode.exe` shows **no menu bar at all** on a
//! fresh launch — nothing discoverable to reach File/Edit/View/etc. without
//! already knowing a keybinding. This is the identical root cause the
//! `#1562` section above already found and documented (`App::setup`'s
//! three-way branch on `backend.backend_caps()` falls into the fully-hidden
//! `cell`/TUI arm because `WinBackend::backend_caps()` sets neither
//! `native_menu` nor `window_chrome`), so nothing new needed re-deriving —
//! see that section for the full read-through of `App::setup`'s branch.
//!
//! What #1582 changes is *which half* of the fix to pursue. `#1562`'s own
//! ask (drafted in `docs/PENDING_QUADRAUI_ISSUES.md`) needs the
//! `window_chrome` path — a drawn row that doubles as the client-side
//! titlebar — which is gated on `win::run` first growing
//! `WM_NCCALCSIZE`/`WM_NCHITTEST` custom-caption handling (declaring the cap
//! before that lands would just stack a second drawn row under the real
//! native caption). #1582 only asks for a menu bar being *present and
//! clickable*, which the *other* named path — `native_menu`, the same one
//! macOS already uses — satisfies with no window-style change at all: a
//! real Win32 `HMENU` attached via `SetMenu` sits underneath the existing
//! native caption the ordinary way any classic Win32 app's menu does, the
//! same shape `crate::event::UiEvent::MenuActivated`'s own doc comment
//! already earmarks for it ("future Win32 `SetMenu`", `quadraui/src/
//! event.rs`). `App::setup`'s `native_menu` arm (`src/app.rs` L8493–8526)
//! already builds the `MenuBar` from the same platform-neutral `MenuDef`s
//! every backend shares and calls `Backend::install_menu_bar` unconditionally
//! for any backend declaring the cap, and `App::handle_event` already
//! matches `UiEvent::MenuActivated` with no backend-specific branch
//! (`src/app.rs` L7808) — both proven live today by macOS, so **no
//! vimcode-side change is needed** once `WinBackend` implements
//! `install_menu_bar` and declares `native_menu: true`. Both are entirely
//! inside `quadraui::win` (`win/backend.rs`'s trait impl, `win/run.rs`'s
//! `wndproc` gaining a `WM_COMMAND` arm) — per the Platform-Neutrality Rule
//! this is not a fix to attempt in this crate. Drafted as a new pending
//! quadraui issue in `docs/PENDING_QUADRAUI_ISSUES.md`, explicitly scoped
//! to the `native_menu` path so it does not duplicate or conflict with
//! `#1562`'s separate `window_chrome` entry — either can land
//! independently, and #1582 needs only this one.
//!
//! **Update (#1618):** `WinBackend::install_menu_bar`/`native_menu: true`
//! landed upstream as quadraui#1200, confirming the prediction above —
//! `App::setup`'s existing `native_menu` arm and `App::handle_event`'s
//! existing `MenuActivated` match needed no vimcode-side change. #1614
//! found #1200 itself crashed `vimcode.exe` on every startup
//! (`RefCell already borrowed`, `JDonaghy/quadraui#1213`); that fix landed
//! upstream as `ce1c763` (a `ModalPumpGuard` around the reentrant `SetMenu`
//! call) plus a regression test in `6e14d8a`, and the pin included both.
//! Real-hardware verification on dell64 confirmed `vimcode.exe` launched
//! cleanly with a real, clickable native menu bar present at startup — but
//! `#1200`'s `native_menu: true` also silently starved `#1562`'s
//! `window_chrome` drawn-caption path (see that section's now-superseded
//! "Correction (#1622)"/"Reconciliation" notes and the struck
//! `docs/PENDING_QUADRAUI_ISSUES.md` entry).
//!
//! **Resolved (#1629):** quadraui#1228 reverts the `native_menu` half of
//! #1200 on Win-GUI (see the `#1562` section's own "Resolved (#1629)" note
//! for the full mechanism) — #1582's "a menu bar is present and
//! clickable" ask is now satisfied by the *drawn* menu row instead of a
//! native `HMENU`, the same row GTK already ships, reached through
//! `App::setup`'s `window_chrome` arm with no vimcode-side change needed.
//! `win_driver_tests`' `native_menu_backend_suppresses_the_drawn_menu_row`/
//! `command_center_paints_on_a_native_menu_backend` tests (below) are
//! replaced with `window_chrome`-shaped assertions that the drawn row
//! (File/Edit/View/...) actually paints. Real-hardware re-verification
//! that the drawn row opens File/Edit/View/... on dell64 is this issue's
//! own acceptance item.
//!
//! # #1673: clicking File/Edit/.../Help on real hardware opens no
//! # dropdown, despite `WM_NCHITTEST` correctly reporting `HTCLIENT` —
//! # same root cause vimcode#1657 already found, not a new defect
//!
//! vimcode#1673 (a bugbash finding, reproduced twice independently with
//! window focus explicitly confirmed before the click) reports that a
//! real left-click at the menu row's historical `(24, 16)` position opens
//! no dropdown on real Win-GUI hardware — no paint change, no UIA
//! `MenuItem` element ever appears — even though a `WM_NCHITTEST` probe
//! at the same point correctly returns `HTCLIENT` (confirming
//! quadraui#1232, the `#1661`/`#1667` sections' pinned rev, is live).
//!
//! `(24, 16)` is not a coordinate this issue derived fresh — it is
//! *exactly* `tests/smoke-spec/win-gui.yaml`'s original
//! `click-file-menu-item-1232` step, which vimcode#1657 (see that
//! issue's own section in this file's git history and this module's
//! `win_gui_smoke_spec_title_band_coordinates_are_stale_1657` test) had
//! already found to be stale: a real-hardware `PrintWindow` remeasurement
//! found "File"'s true painted left edge sits at x≈55-88, not x=24 — the
//! app-icon slot (#720) and the Command Center (#676/#939, added to the
//! pin between the two issues) both push every menu-row item right of
//! where this smoke-spec coordinate was originally derived. x=24 is blank
//! padding left of the real label, not the label itself.
//!
//! Ruled out, by reading the shared code every backend's click routes
//! through (`crate::app::App::handle_event`'s "Menu system intercept"
//! block, `quadraui::MenuSystem::handle`) and by this module's own
//! already-passing `window_chrome_backend_paints_the_drawn_menu_row`/
//! `command_center_paints_on_a_window_chrome_backend` tests (#1629): the
//! click-to-dropdown path itself is platform-neutral and already proven
//! live on `WinBackend` up through "the menu row paints". The one
//! Win-specific link in that chain — `win::events::win_button_down`'s
//! device-pixel→DIP conversion, and `WinBackend::register_menu_bar_item_
//! zones`/`nc_hit_test` registering the *same* `MenuBarLayout` bounds
//! `MenuSystem::handle` hit-tests against — was read directly against the
//! pinned rev and found consistent (both derive from the identical
//! `layout.visible_items[i].bounds` the paint call computes, not two
//! independently-computed rects the #720 bug class would need). No
//! source-level defect distinct from #1657's already-tracked coordinate
//! staleness was found.
//!
//! Confirmed empirically (not just by source reading) on the one backend
//! this can run headlessly: `src/gtk/testing.rs::app_icon::clicking_the_
//! stale_1232_file_x_opens_nothing_but_the_real_bounds_do_1673` clicks
//! both x=24 (opens nothing, matching vimcode#1673's report exactly) and
//! the real, dynamically-located "File" bounds (opens the dropdown fine)
//! in the same test run, against today's `develop`, with no code change
//! on either side — both backends share the identical `window_chrome`
//! title-band composition and the identical `MenuBar` layout algorithm
//! (`App::render_content`'s `presence.menu_row`/`presence.command_
//! center` blocks), so GTK's result is a faithful stand-in for what the
//! same click would do on `WinBackend`, per the same reasoning #1657's
//! own Tier-1 pair already relies on.
//!
//! **No new quadraui issue is drafted for this one.** Unlike #1661/#1667,
//! source + empirical review here did not surface a quadraui-side defect
//! distinct from what #1657 already found and already tracks via its own
//! corrected `-1657` smoke-spec steps (`click-file-menu-item-1657`/
//! `file-dropdown-opens-1657`, already present in `tests/smoke-spec/
//! win-gui.yaml` at the real, remeasured x=70). vimcode#1673's real value
//! is independent, focus-safe reconfirmation that the *stale* x=24
//! coordinate specifically is dead — useful regression evidence, but not
//! evidence of a second, distinct bug on top of #1657's. Left open rather
//! than closed from here: whether the `-1657` corrected coordinate
//! genuinely opens the dropdown on *real* Win-GUI hardware (as opposed to
//! GTK's faithful-but-not-identical stand-in above) is still an
//! unconfirmed, real-hardware acceptance item — see
//! `tests/smoke-spec/win-gui.yaml`'s own `-1657` section for the exact
//! open question.
//!
//! Adds `clicking_the_stale_1232_file_x_opens_nothing_1673` to
//! `win_driver_tests` below: the Win-GUI mirror of the GTK test named
//! above, type-checked only on this Linux worktree (`#[cfg_attr(target_os
//! = "windows", test)]`, same posture as every other test in that
//! module) — see that module's own top-of-file #1558 disclaimer for why
//! it cannot yet be *executed*, here or on dell64.

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
//
// ── #1668: embedded terminal panel blank on Win-GUI — fixed outside this
// module ──────────────────────────────────────────────────────────────────
//
// vimcode#1668 ("Terminal > New Terminal shows a blank panel forever, no
// prompt, no echoed input, typing produces nothing") root-caused to a
// genuine, fixable-in-vimcode scheduling gap rather than a quadraui
// rasteriser defect like #1657/#1661/#1667 above: `quadraui::runner::
// ShellApp::tick`'s own doc says Win-GUI has no unconditional idle-poll
// fallback (unlike TUI/GTK/macOS's 250ms `IDLE_POLL_CEILING`), and nothing
// in `App::tick_dispatch` (`src/app.rs`) was re-arming a wake-up for an
// open terminal pane — so `Engine::poll_terminal` (the PTY output drain)
// never ran again past the very first frame on this backend specifically.
// Fixed in the one shared, backend-neutral call site this bug needed
// (`src/app.rs::App::tick_dispatch`'s new `terminal_poll_rearm_delay`
// re-arm, mirroring the existing `ai_streaming` one right above it) — not
// here, because there is no `WinBackend`/`win::*` decision to make: the
// gap was "nobody asked to be woken again", not "WinBackend painted the
// wrong thing". See `src/app.rs::terminal_poll_rearm_tests` for the
// RED/GREEN-verified unit coverage (a pure, `Backend`-free function test,
// the same shape `dedup_window_title`'s doc establishes as this crate's
// accepted substitute for a `Backend`-call-count driver test — sealed
// trait, no mock) and `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry for
// the follow-up testing-infrastructure ask this investigation surfaced:
// `WinDriver` (unlike `TuiDriver`/`MacDriver`) has no `.tick()` at all, and
// its `attach_headless` never sets `WinBackend::hwnd`, so even a
// `WinDriver`-based scenario here could not have observed this class of
// scheduling bug on real Windows either.
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

    // ── #1629: `WinBackend::backend_caps().window_chrome` adoption ──────
    //
    // Replaces the pair of tests #1618 added here
    // (`native_menu_backend_suppresses_the_drawn_menu_row`/
    // `command_center_paints_on_a_native_menu_backend`), which pinned
    // `WinBackend::backend_caps().native_menu: true` — quadraui#1228
    // reverted that half of #1200 (see this module's top-of-file `#1562`/
    // `#1582` "Resolved (#1629)" notes and `Cargo.toml`'s pin comment), so
    // those assertions no longer match reality: `WinBackend` now declares
    // only `window_chrome`, matching `GtkBackend`. Bodies mirror
    // `src/gtk/testing.rs`'s own `window_chrome`-shaped assertions
    // (`screen_contains("File")`/the Command Center's search label both
    // painting together, since #939 decoupled the two — neither suppresses
    // the other on this arm). Like the rest of this module, only
    // registered as an actual `#[test]` on real Windows — see the
    // `win_driver_tests` module doc above for why `cargo xwin test` can't
    // execute either one from this Linux worktree, or on dell64 (#1558's
    // DLL-load crash, unrelated to and not fixed by this pin).
    //
    // RED-verification for these two specifically (distinct from the
    // module-wide #1558 blanket disclaimer above, which explains why
    // *neither* test has ever run as an executed `#[test]` on any host):
    // read against the pin *before* this bump (`6e14d8a`, #1618's pin) —
    // `WinBackend::backend_caps()` returned `{native_menu: true,
    // window_chrome: true}` (confirmed by reading that struct literal at
    // that rev) — `App::setup`'s `if native_menu {...} else if
    // window_chrome {...}` order would have taken the `native_menu` arm,
    // so `backend_caps().window_chrome` being read as the *live* branch
    // would be false by construction (the first test's precondition would
    // fail) and the drawn menu row would have stayed suppressed
    // (`screen_contains("File")` == `false`, failing the second test).
    // Both fail by construction against the pre-#1629 pin; neither has
    // been executed as a running `#[test]` (blocked by #1558, same as
    // scenarios 1-3 above), so this is a source-level RED confirmation,
    // not an executed one — stated explicitly here rather than left to
    // the inherited blanket disclaimer.

    /// #1629/quadraui#1228: `WinBackend` declares `BackendCaps::
    /// window_chrome` (and no longer `native_menu`), so `App::setup` must
    /// take the `window_chrome` arm and paint the in-window drawn menu row
    /// — the same shape `GtkBackend` already exercises
    /// (`src/gtk/testing.rs`'s `screen_contains("File")` assertions).
    #[cfg_attr(target_os = "windows", test)]
    fn window_chrome_backend_paints_the_drawn_menu_row() {
        use quadraui::Backend;

        let h = conformance_harness(plain_engine(), 1400, 900);

        let caps = h.driver.backend().backend_caps();
        assert!(
            caps.window_chrome,
            "precondition: WinBackend must declare window_chrome on real \
             Windows now that the pin includes quadraui#1228"
        );
        assert!(
            !caps.native_menu,
            "precondition: WinBackend must no longer declare native_menu \
             — quadraui#1228 reverted that half of #1200"
        );
        assert!(
            h.driver.screen_contains("File"),
            "the in-window menu row must paint when WinBackend declares \
             window_chrome (and no native_menu); painted text was {:?}",
            h.driver.painted_texts()
        );
    }

    /// #1629/quadraui#1228: the Command Center must paint in the
    /// title-bar band on `WinBackend` alongside the now-live drawn menu
    /// row — the same #939 `title_bar_band_live`-only gate
    /// (`render::FramePresence::from_screen`) `src/gtk/testing.rs`'s
    /// `command_center_paints_between_menu_labels_and_window_controls`
    /// exercises for GTK, now newly reachable on Windows because
    /// `backend_caps().window_chrome` alone (not gated behind a competing
    /// `native_menu` arm) is what makes `menu_bar_visible`/the title-bar
    /// band live.
    #[cfg_attr(target_os = "windows", test)]
    fn command_center_paints_on_a_window_chrome_backend() {
        let mut engine = plain_engine();
        // A distinctive, non-default `cwd` so the Command Center's "🔍
        // <project>" search label is unmistakable in `painted_texts()` —
        // mirrors the GTK/macOS sibling tests' identical fixture shape.
        engine.cwd = PathBuf::from("omnibar-fixture-1629");

        let h = conformance_harness(engine, 1400, 900);

        assert!(
            h.driver.screen_contains("File"),
            "sibling assertion to window_chrome_backend_paints_the_drawn_menu_row \
             -- the drawn menu row must paint; painted text was {:?}",
            h.driver.painted_texts()
        );
        assert!(
            h.driver.screen_contains("omnibar-fixture-1629"),
            "the Command Center's search label must paint in the title-bar \
             band alongside the drawn menu row; painted text was {:?}",
            h.driver.painted_texts()
        );
    }

    // ── #1657: smoke-spec coordinate staleness, Win-GUI mirror ──────────
    //
    // `src/gtk/testing.rs`'s `win_gui_smoke_spec_title_band_coordinates_are_
    // stale_1657` is the executable half of this Tier-1 conformance pair
    // (GTK actually runs headlessly on any Linux dev box); this is the
    // Win-GUI mirror, narrower in scope because `ConformanceHarness` (used
    // here, unlike `crate::gtk::testing::Harness`) does not clone `App::
    // title_bar_rect`, so the caption-button half of that test has no
    // equivalent here. The two findings this CAN check —  the File menu
    // item's real left edge, and the Command Center search box now
    // covering the smoke-spec's old "empty band" x=620 — are both reached
    // through `WinDriver::find_bounds` and `engine.command_center_layout`,
    // exactly like the GTK twin. Only type-checked on this Linux worktree
    // (`#[cfg_attr(target_os = "windows", test)]`, same as every other test
    // in this module) — see the module-top `#1558` doc section for why it
    // cannot yet be *executed*, on this host or on dell64.
    #[cfg_attr(target_os = "windows", test)]
    fn win_gui_smoke_spec_title_band_coordinates_are_stale_1657() {
        let h = conformance_harness(plain_engine(), 1024, 768);

        let file = h
            .driver
            .find_bounds("File")
            .expect("the File menu label must paint");
        assert!(
            file.x > 24.0 + file.width,
            "the smoke-spec's stale x=24 assumption must land in blank \
             padding left of the real File label (got {file:?})"
        );

        let cc = h
            .engine
            .borrow()
            .command_center_layout
            .borrow()
            .clone()
            .expect("the Command Center must paint on a window_chrome backend");
        let search = cc
            .search_bounds
            .expect("the search box must have a painted bounds");
        assert!(
            search.x <= 620.0 && 620.0 < search.x + search.width,
            "x=620 must now land inside the real search box ({search:?}) \
             -- the smoke-spec's old \"must stay HTCAPTION\" expectation \
             at this x is therefore itself stale"
        );
    }

    // ── #1661: blank title-band strip must stay HTCAPTION ───────────────
    //
    // vimcode#1661 (same real-hardware bugbash session as #1657 above)
    // swept `WM_NCHITTEST` across the blank strip between the last menu
    // label and the caption-button region and found `HTCLIENT` almost
    // everywhere instead of `HTCAPTION` -- combined with the still-open
    // caption-button finding (#1656, `docs/PENDING_QUADRAUI_ISSUES.md`),
    // the drawn title band has no mouse-draggable region left at all. This
    // probes the identical 1024x768 geometry `win_gui_smoke_spec_title_
    // band_coordinates_are_stale_1657` already established, at a point
    // inside the Command Center's own reserved container
    // (`engine.command_center_layout().bounds`) but strictly left of its
    // leftmost painted widget (the back-navigation arrow) -- real blank
    // padding, not covered by any registered zone smaller than the
    // title-bar band per `WinBackend::nc_hit_test`'s own doc.
    //
    // By source-level reasoning alone this point *should* already read
    // `HTCAPTION`: `WinBackend::register_command_center_zones` never
    // registers `CommandCenterHit::Bar` (the Command Center's own
    // container), only `Back`/`Forward`/`SearchBox`, so a correctly-sized
    // Command Center would leave this padding unregistered and therefore
    // `HTCAPTION` by `nc_hit_test`'s own fallback -- see
    // `docs/PENDING_QUADRAUI_ISSUES.md`'s new #1661 entry for the full
    // ruling-out of that hypothesis (and of the sibling "bar-local vs
    // absolute" bug class #1656 already found) in favour of a leading,
    // unconfirmed hypothesis that real Win-GUI/DirectWrite text
    // measurement sizes the Command Center's widgets wide enough to
    // over-fill their own container. That hypothesis needs live `DWrite`
    // measurement to confirm or rule out, which has no non-Windows
    // implementation to run here -- same reason this function, like every
    // other test in this module, is only type-checked on this Linux
    // worktree (`#[cfg_attr(target_os = "windows", test)]`) and cannot yet
    // be *executed*, here or on dell64 (see the module-top `#1558` doc
    // section). RED-verification for this exact scenario is therefore the
    // real-hardware sweep transcript vimcode#1661 itself reports, not an
    // executed run of this function -- stated explicitly here rather than
    // left to the inherited blanket disclaimer.
    #[cfg_attr(target_os = "windows", test)]
    fn win_gui_blank_title_band_strip_is_caption_1661() {
        let h = conformance_harness(plain_engine(), 1024, 768);

        let cc = h
            .engine
            .borrow()
            .command_center_layout
            .borrow()
            .clone()
            .expect("the Command Center must paint on a window_chrome backend");
        let back = cc
            .back_bounds
            .expect("the back-navigation arrow must have painted bounds");

        // A point inside the Command Center's own reserved container, but
        // strictly left of its leftmost painted widget -- real blank
        // padding, the exact shape of strip vimcode#1661 reports.
        let probe_x = (cc.bounds.x + back.x) / 2.0;
        assert!(
            probe_x < back.x,
            "fixture assumption: the Command Center must leave blank \
             padding left of its back-navigation arrow (cc={cc:?}, \
             back={back:?})"
        );

        assert_eq!(
            h.driver.backend().nc_hit_test(probe_x, 16.0),
            Some(true),
            "blank title-band padding at x={probe_x} (inside the Command \
             Center's own container, left of its back arrow) must report \
             HTCAPTION so the OS treats it as the window's drag handle; \
             vimcode#1661 found this reporting HTCLIENT on real hardware \
             instead"
        );
    }

    // ── #1667: `:colorscheme` only repaints the minimap on Win-GUI ──────
    //
    // Root-caused by reading (not running — see this module's own #1558
    // disclaimer) every Win-GUI rasteriser's real source at the pinned
    // rev: `WinBackend::draw_minimap`/`draw_menu_bar`/
    // `draw_activity_bar`/`draw_status_bar_interactive`/
    // `draw_completions`/`draw_find_replace`/`draw_scrollbar`/
    // `draw_drop_overlay`/`draw_context_menu` all paint with
    // `&self.current_theme` — the live field `Backend::set_theme` writes
    // every frame (`App::sync_per_frame_backend_state`, backend-neutral,
    // `src/app.rs`) — exactly like `GtkBackend`/`MacBackend`'s equivalents.
    // But `super::editor::draw_editor` (editor.rs:68),
    // `super::tab_bar::paint_tab_bar_icons_from_layout` (tab_bar.rs:285,
    // reached with no theme argument at all from
    // `WinBackend::draw_tab_bar_icons`), `super::tree::draw_tree`
    // (tree.rs:103, the Explorer sidebar's own rasteriser),
    // `WinBackend::draw_panel` (backend.rs:4042), `WinBackend::
    // draw_sidebar_panel_interactive` (backend.rs:4381),
    // `WinBackend::draw_split` (backend.rs:3829) and `WinBackend::
    // draw_split_tree` (backend.rs:3870) each still construct a fresh
    // `Theme::default()` instead — several with an explicit "preserves
    // the pre-#8xx behaviour exactly ... has no live theme wired through
    // yet" comment admitting the gap outright. `GtkBackend`/`MacBackend`'s
    // equivalents of all seven already read `self.current_theme` (checked
    // directly against both files at the same pin). This is why the
    // bug's own screenshot evidence shows the minimap alone flipping to
    // `vscode-light` while the menu bar/activity bar/tab bar/editor/
    // status bar/Explorer sidebar stay dark: the minimap is one of the
    // nine already-wired rasterisers, the editor and Explorer tree are
    // two of the seven that are not. `docs/PENDING_QUADRAUI_ISSUES.md`'s
    // new entry drafts the upstream ask for all seven; nothing in this
    // vimcode repo can fix this directly, per the Platform-Neutrality
    // Rule — `WinBackend` is a 1-line quadraui re-export
    // (`src/win/backend.rs`), and every file named above lives in
    // quadraui, not here.
    //
    // Probes two of the seven (`draw_editor`'s `theme.background`,
    // `draw_tree`'s `theme.tab_bar_bg`) — the pair this issue's own
    // screenshot evidence calls out most directly ("editor background/
    // text" and "Explorer sidebar") — rather than all seven, to keep this
    // probe's geometry assumptions (where is a safe, glyph-free pixel to
    // sample) to the two cases this module can locate via `find_bounds`
    // with no extra fixture plumbing. `vscode-light`'s `background`
    // (`#ffffff`) and `tab_bar_bg` (`#ececec`) are both far from
    // quadraui's own dark `Theme::default()` (`rgb(20, 22, 30)` for
    // both), so either backend reading the wrong theme is unambiguous
    // pixel-exact, no tolerance needed.
    //
    // Mechanically certain to fail against the pinned rev (every
    // function named above is read directly, not inferred) and to pass
    // once quadraui wires `self.current_theme` through all seven — but,
    // per this module's own top-of-file #1558 disclaimer, could not be
    // *executed* from this Linux worktree (or on dell64) to observe that
    // RED/GREEN flip directly; this is a source-level RED confirmation,
    // stated explicitly here rather than left to the inherited blanket
    // disclaimer, exactly like `win_gui_blank_title_band_strip_is_caption_1661`
    // above.
    #[cfg_attr(target_os = "windows", test)]
    fn colorscheme_change_repaints_editor_and_explorer_sidebar_1667() {
        let mut engine = plain_engine();
        engine.settings.colorscheme = "vscode-light".to_string();
        // A leading blank line puts a glyph-free row directly above the
        // distinctive probe line below, so a pixel sampled there is
        // guaranteed pure editor background, not anti-aliased glyph
        // fringe. No digits/underscores (would risk a separate
        // syntax-highlight span splitting `find_bounds`'s match) — moot
        // here anyway (an unnamed scratch buffer has no language, so no
        // highlighting), but kept plain for robustness.
        engine
            .buffer_mut()
            .insert(0, "\nvimcodeprobelineforcolorschemerepaint\n");
        engine.app_shell.show_panel(&quadraui::WidgetId::new(
            crate::core::engine::sidebar::PANEL_EXPLORER,
        ));
        engine.session.explorer_visible = true;

        let mut h = conformance_harness(engine, 1400, 900);
        // A second render pass: `App::sync_per_frame_backend_state` pushes
        // the live theme onto the backend at the *start* of
        // `render_content`, so a widget painted *before* that call on the
        // very first frame can lag by one paint (the GTK/macOS
        // `sidebar_header_paints_...` tests document the identical
        // ordering note for the sidebar header specifically) — this keeps
        // both probes robust to that ordering rather than coupling them
        // to an unrelated, already-settled claim.
        h.driver.render();

        let theme = crate::render::Theme::vscode_light();

        // ── `win::editor::draw_editor`'s background ──────────────────
        let probe_line = h
            .driver
            .find_bounds("vimcodeprobelineforcolorschemerepaint")
            .expect("the probe line must paint inside the editor viewport");
        let editor_px = h
            .driver
            .pixel(probe_line.x as u32, (probe_line.y - 5.0).max(0.0) as u32);
        assert_eq!(
            (editor_px.r, editor_px.g, editor_px.b),
            (theme.background.r, theme.background.g, theme.background.b),
            "editor background must repaint from `theme.background` on a \
             runtime `:colorscheme` change, not stay pinned to Win-GUI's \
             hardcoded `Theme::default()` (`win::editor::draw_editor`)"
        );

        // ── `win::tree::draw_tree`'s Explorer sidebar background ──────
        let header = h
            .driver
            .find_bounds("EXPLORER")
            .expect("the Explorer sidebar header must paint its panel title");
        let tree_px = h.driver.pixel(
            header.x as u32 + 10,
            (header.y + header.height + 20.0) as u32,
        );
        assert_eq!(
            (tree_px.r, tree_px.g, tree_px.b),
            (theme.tab_bar_bg.r, theme.tab_bar_bg.g, theme.tab_bar_bg.b),
            "the Explorer sidebar tree must repaint its background from \
             `theme.tab_bar_bg` on a runtime `:colorscheme` change, not \
             stay pinned to Win-GUI's hardcoded `Theme::default()` \
             (`win::tree::draw_tree`)"
        );
    }

    // ── #1673: the stale x=24 menu-row coordinate opens nothing ────────
    //
    // Win-GUI mirror of `src/gtk/testing.rs::app_icon::clicking_the_
    // stale_1232_file_x_opens_nothing_but_the_real_bounds_do_1673` — see
    // this module's own top-of-file `#1673` doc section for the full
    // investigation and why no new quadraui issue is drafted. Both
    // backends share the identical `window_chrome` title-band
    // composition and `MenuBar` layout algorithm, so the GTK twin (which
    // *does* run headlessly here and was empirically confirmed against
    // today's `develop`) is a faithful stand-in for this one; this
    // function is, like every other test in this module, only
    // type-checked on this Linux worktree
    // (`#[cfg_attr(target_os = "windows", test)]`) and cannot yet be
    // *executed*, here or on dell64 (module-top `#1558` disclaimer).
    #[cfg_attr(target_os = "windows", test)]
    fn clicking_the_stale_1232_file_x_opens_nothing_1673() {
        let mut h = conformance_harness(plain_engine(), 1024, 768);

        assert!(
            !h.driver.screen_contains("New Tab"),
            "sanity: the File dropdown must be closed before either click"
        );

        // vimcode#1673's own reproduction coordinate — the stale,
        // pre-#1657 value `tests/smoke-spec/win-gui.yaml`'s original
        // `click-file-menu-item-1232` step still carries.
        h.driver.click(24.0, 16.0);
        h.driver.render();
        assert!(
            !h.driver.screen_contains("New Tab"),
            "x=24 is blank padding left of the real \"File\" label at \
             this window width (vimcode#1657) — it must NOT open the File \
             dropdown, reproducing vimcode#1673's report exactly; painted \
             texts were {:?}",
            h.driver.painted_texts()
        );

        let file = h
            .driver
            .find_bounds("File")
            .expect("the File menu-bar header must paint");
        assert!(
            file.x > 24.0,
            "fixture assumption: the real \"File\" label must paint \
             strictly right of x=24 at this window width, or this test's \
             own premise (x=24 misses it) doesn't hold; got {file:?}"
        );
        h.driver
            .click(file.x + file.width / 2.0, file.y + file.height / 2.0);
        h.driver.render();
        assert!(
            h.driver.screen_contains("New Tab"),
            "clicking the real, painted \"File\" label must open its \
             dropdown — proving vimcode#1673's reported symptom is the \
             stale x=24 coordinate alone, not a regression in the shared \
             click-routing path; painted texts were {:?}",
            h.driver.painted_texts()
        );
    }
}
