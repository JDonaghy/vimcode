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
//!
//! # #1695: Explorer sidebar scrollbar is wide/always-visible with a
//! # doubled thumb — a quadraui `TreeController` bug (double-paint and
//! # no-hidden-at-rest-state shared with GTK/macOS, plus a Win-GUI-only
//! # mis-themed phantom band)
//!
//! vimcode#1695 reports the Win-GUI Explorer sidebar's vertical scrollbar
//! as a wide (~14-28px), always-visible, light-grey bar — unlike VS
//! Code's thin (~10px), hidden-at-rest overlay — with "two differently-lit
//! segments... a thumb and a second overlapping rect rather than one
//! thumb on one track".
//!
//! `WinBackend` is a 1-line quadraui re-export (this module's `backend.rs`
//! doc), and `WinBackend::tree_vscrollbar`/`draw_tree` delegate to exactly
//! the same quadraui code every other pixel backend does
//! (`TreeController::render` → `primitives::tree::native_surface_paint::
//! paint` / `primitives::scrollbar::native_surface_paint::paint`) — so
//! the double-paint and no-hidden-at-rest-state bugs below have no
//! Win-specific code for a vimcode PR to change, per the
//! Platform-Neutrality Rule. Read directly against the pinned rev
//! (`ca7fcc83afad01ec3422f79366566f3a263b22bf`) and reproduced
//! **executably, not just by source reading** — on GTK, which runs
//! headlessly on every host, including this one, and shares those two
//! bugs with Win-GUI (confirmed by reading all three `Backend` impls'
//! `tree_vscrollbar`/`draw_tree` side by side: identical delegation to
//! `TreeView::vscrollbar` / `primitives::tree::paint`).
//!
//! **One dimension does NOT match GTK: the phantom band's theme.**
//! `quadraui::win::tree::draw_tree` hardcodes `let theme =
//! Theme::default();` and never reads `WinBackend::current_theme` —
//! unlike `GtkBackend::draw_tree`/`MacBackend::draw_tree`, which both
//! thread `self.current_theme` through. Since the phantom inner
//! scrollbar (finding 1 below) paints from *inside* `draw_tree`, it
//! renders in quadraui's library-default colours on Win-GUI, not the
//! user's active theme — while the real, explicit outer scrollbar
//! (`WinBackend::draw_scrollbar`) correctly reads `self.current_theme`.
//! So on real Win-GUI hardware the two stacked bands are mismatched on
//! *theme*, not just width — a genuine, Win-GUI-specific divergence from
//! GTK/macOS (whose `draw_tree` threads the live theme through for both
//! bands). See `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry, Ask item 5,
//! for the drafted fix (the same one-line `self.current_theme` shape
//! `WinBackend::draw_scrollbar` already uses).
//!
//! Three real findings, not one:
//!
//! 1. **The "two differently-lit segments" is a genuine double-paint, not
//!    intended compositing.** `TreeController::render` narrows its `rect`
//!    into `(tree_rect, sb_rect)`, paints the real scrollbar explicitly
//!    into `sb_rect` — but first calls `backend.draw_tree(tree_rect,
//!    &tree)` with a `tree` whose `rows` are still the *full*, untruncated
//!    list. The shared rasteriser underneath `draw_tree` unconditionally
//!    re-derives and paints its *own* scrollbar whenever that untruncated
//!    row count overflows `tree_rect`'s height (which narrowing never
//!    changed) — a second, phantom scrollbar immediately left of the real
//!    one, at a *different* width (`backend.line_height()` for the real
//!    one vs. `layout_metrics::tree_row_pitch`'s `~1.4x` that for the
//!    phantom). Together they explain both the doubled-segment look and
//!    the reported ~14-28px combined width (neither alone is that wide).
//!    Shared with GTK/macOS.
//! 2. **No hidden-at-rest state exists at all.** `primitives::scrollbar::
//!    native_surface_paint::paint`'s track alpha is `0.20` even when
//!    neither hovered nor dragging — never `0.0` — and `TreeController::
//!    build_scrollbar` never sets `Scrollbar::hovered` in the first place
//!    (no state exists on `TreeController` to carry a "cursor is over the
//!    scrollbar" fact from `handle`'s `MouseMoved` arm into the next
//!    `render()` call). VS Code's fade-in-on-hover/scroll overlay needs
//!    new quadraui API surface, not a config value a caller can already
//!    reach. Shared with GTK/macOS.
//! 3. **Win-GUI-only: the phantom band is mis-themed.** As described
//!    above, `win::tree::draw_tree` paints the phantom scrollbar from
//!    finding 1 using `Theme::default()` rather than
//!    `WinBackend::current_theme` — a divergence from GTK/macOS, where
//!    both bands are same-themed. Not reproducible on this host (no
//!    Windows hardware); confirmed by reading `win/tree.rs` and
//!    `win/backend.rs` directly against the pinned rev.
//!
//! Findings 1 and 2 are proven real, not just plausible from reading
//! source, by two new GTK tests that **pass today** against the pinned
//! rev: `src/gtk/testing.rs::scrollbar_paint::
//! explorer_sidebar_scrollbar_paints_unconditionally_at_
//! rest_1695` (finding 2 — scrollbar-colored pixels appear at the panel's
//! right edge with no hover event synthesized) and `::explorer_sidebar_
//! scrollbar_double_paints_an_adjacent_phantom_band_1695` (finding 1 — a
//! second band, sized and positioned exactly as the root-cause above
//! predicts, also paints). Both drive vimcode's real Explorer paint path
//! (`App::paint_sidebar_panel_rung` → `populate_explorer_tree_controller`
//! → `TreeController::render`), not a hand-built fixture. Finding 3 has
//! no GTK characterization — GTK's `draw_tree` already threads
//! `current_theme` through correctly, so there is nothing buggy to
//! reproduce there; it remains source-verified only, pending real
//! Win-GUI hardware or a Windows-only headless harness.
//!
//! **No code changes here.** Every file implicated above
//! (`compose/tree_controller.rs`, `primitives/tree.rs`,
//! `primitives/scrollbar.rs`, `win/tree.rs`) lives in quadraui, not in
//! this repo — see `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry for the
//! full write-up and the drafted Ask (including Ask item 5, which closes
//! finding 3). vimcode#1695 stays open behind it.

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

// ── #1691: Win-GUI paints no line-number gutter, despite #1543 making
// `number` the default ──────────────────────────────────────────────────
//
// vimcode#1691 reports buffer text starting flush against the editor pane's
// own left edge on Win-GUI — no gutter column, no line numbers, no left
// inset. The issue names two suspects: `render.rs`'s `gutter_char_width`
// computation (vimcode-side) and `quadraui::win::editor::draw_editor`'s
// `if editor.gutter_char_width > 0` gate (quadraui-side).
//
// The Win-GUI gutter is produced by exactly three pieces of code, and the
// split between them decides what this repo can and cannot test:
//
// 1. **vimcode, backend-neutral** — `render::calculate_gutter_cols` /
//    `render::build_rendered_window` decide `gutter_char_width` and each
//    line's `gutter_text`, and `render::to_q_editor` packs them into the
//    `quadraui::Editor` that `App::paint_editor_windows_rung` pushes as a
//    `Surface::Editor` for *every* GUI backend alike (`src/app.rs`).
// 2. **quadraui, backend-neutral** — `Editor::layout_with_options`
//    (`quadraui/src/primitives/editor.rs`) turns that into the geometry the
//    rasteriser paints with: `gutter_w = gutter_char_width * cell_width`,
//    `gutter_bounds`, and `text_bounds.x = viewport.x + gutter_w`. This is
//    the function `win::editor::draw_editor` itself calls, and it is plain
//    arithmetic with no WinAPI in it.
// 3. **quadraui, Win-only** — `win::editor::draw_editor`'s Direct2D/
//    DirectWrite `draw_text` calls into an `ID2D1RenderTarget`.
//
// Rungs 1 and 2 are the ones the issue actually points at, and both are
// reachable from an ordinary Linux/macOS host under `cargo test
// --no-default-features --features win` — `WinBackend`'s metric getters are
// plain fields, not WinAPI calls (see this module's own "Why `feature =
// "win"` alone" doc and `win_backend_conformance` above). So the module
// below **executes**, on any host, against the real production functions
// and `WinBackend`'s own `char_width()`/`line_height()`, rather than being
// type-checked-only like `win_driver_tests`. It is the same posture
// `win_ctrl_key_translation_tests_1674` below takes for #1674: drive as far
// up the real pipeline as a non-Windows host can reach, and say precisely
// where the reachable part stops.
//
// **RED-verified** (CLAUDE.md "Testing (CRITICAL)" rule 2 — "a test that
// cannot fail is not coverage"). Two separate bugs were injected into the
// production source at the two sites #1691 itself names, each run and then
// reverted:
//
// 1. `render::calculate_gutter_cols`' `LineNumberMode::Absolute` arm made
//    to return `1 + git + bp` (i.e. collapse to `None`'s bare fold
//    column). → `default_settings_reserve_a_line_number_gutter_and_inset_
//    the_text_column_1691` **FAILED** ("must reserve more than the bare
//    one-column fold indicator … got 1").
// 2. `render::format_gutter_with_fold` made to return
//    `" ".repeat(gutter_char_width)` (gutter reserved but blank — the "no
//    line numbers" half of the report). → the same test **FAILED** ("must
//    carry its own line number, not blank padding … got \"     \"").
//
// The control test stayed **green** through both injections, so it is a
// genuine control and not a second copy of the positive assertion. Both
// injections were reverted; `git diff` touches no file but this one.
//
// The control also pins the geometry #1691 *reports* (1-cell gutter, no
// digits, text one cell from the pane edge) against a `:set nonumber`
// engine, so a dell64 reproduction can be attributed to a stray `nonumber`
// override by comparing against that exact shape.
//
// **What this does NOT cover, and why #1691 stays open.** Rung 3 — the
// actual Direct2D draw calls — needs a live Windows host; so does the
// hypothesis #1691's own "Reproduction" section names first (that the
// dell64 capture's binary predates #1543, or that session carried a stray
// `:set nonumber`/`settings.json` override), which is an *environment*
// question no source read can settle. Rungs 1 and 2 are now proven correct
// by executable tests on every host, which is new information: it means a
// reproduction on real hardware can only be rung 3 or the environment, and
// narrows what a dell64 session has to check. `win-smoke-tests.md`'s
// "Open real-hardware questions (dell64)" section carries that checklist.
// This module is therefore *not* a fix for #1691 and must not be read as
// one.
#[cfg(all(test, feature = "win"))]
mod win_gutter_contract_1691 {
    use crate::core::{Engine, WindowRect};
    use crate::render;

    /// A default-settings engine with a 20-line buffer, plus the geometry
    /// inputs Win-GUI's own backend reports.
    ///
    /// Returns `(gutter_cells, first_line_gutter_text, pane_x, text_x,
    /// gutter_bounds_is_some, cell_width)` — every value read from a
    /// production function, none recomputed by the test.
    fn probe_gutter(line_numbers: crate::core::settings::LineNumberMode) -> GutterProbe {
        use quadraui::Backend as _;

        let backend = super::backend::WinBackend::new();
        let cell_width = backend.char_width() as f64;
        let line_height = backend.line_height() as f64;
        let scrollbar_reserve = backend.scrollbar_reserve() as f64;
        assert!(
            cell_width > 0.0 && line_height > 0.0,
            "WinBackend must report real text metrics before any gutter \
             arithmetic means anything (got cell_width={cell_width}, \
             line_height={line_height})"
        );

        let mut engine = Engine::new_for_test();
        // `Engine::new_for_test` is hermetic (`Settings::default()`, no
        // ambient `settings.json` read), so this asserts #1543's shipped
        // default rather than a value the test itself installed.
        assert_eq!(
            Engine::new_for_test().settings.line_numbers,
            crate::core::settings::LineNumberMode::Absolute,
            "#1543 made `LineNumberMode::Absolute` the untouched default; \
             #1691's premise depends on it"
        );
        engine.settings.line_numbers = line_numbers;
        engine.buffer_mut().insert(
            0,
            &(1..=20)
                .map(|n| format!("vimcodeline{n:02}\n"))
                .collect::<String>(),
        );

        let theme = render::Theme::vscode_dark();
        let bounds = WindowRect::new(0.0, 0.0, 1400.0, 900.0);
        let tab_bar_h = render::tab_row_height_px(line_height);
        let (rects, _) = engine.calculate_group_window_rects(bounds, tab_bar_h);
        let layout = render::build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            cell_width,
            true,
            scrollbar_reserve,
            render::gtk_minimap_sizing(),
        );
        let rw = layout
            .windows
            .first()
            .expect("a single-group layout must paint exactly one window");

        // Rung 2: the *same* `Editor` + `Editor::layout` call
        // `quadraui::win::editor::draw_editor` makes — no second copy of
        // the arithmetic lives in this test.
        let editor = render::to_q_editor(rw);
        let el = editor.layout(editor.rect, cell_width as f32, line_height as f32);

        GutterProbe {
            gutter_cells: rw.gutter_char_width,
            first_gutter_text: rw.lines[0].gutter_text.clone(),
            pane_x: editor.rect.x,
            text_x: el.text_bounds.x,
            has_gutter_bounds: el.gutter_bounds.is_some(),
            cell_width: cell_width as f32,
        }
    }

    struct GutterProbe {
        gutter_cells: usize,
        first_gutter_text: String,
        pane_x: f32,
        text_x: f32,
        has_gutter_bounds: bool,
        cell_width: f32,
    }

    /// #1691's "Ask", for the two rungs a non-Windows host can execute: a
    /// fresh, untouched-settings engine must reserve a real line-number
    /// gutter, put the line's own number in it, and inset the text column
    /// past it by exactly that many cells.
    #[test]
    fn default_settings_reserve_a_line_number_gutter_and_inset_the_text_column_1691() {
        let p = probe_gutter(crate::core::settings::LineNumberMode::Absolute);

        // A 20-line buffer: 2 digits + 2 padding + 1 fold column = 5 cells
        // (`render::calculate_gutter_cols`). Asserted as `> 1` rather than
        // `== 5` so a future padding change doesn't false-alarm, but `> 1`
        // is the load-bearing claim: `LineNumberMode::None` is exactly 1.
        assert!(
            p.gutter_cells > 1,
            "a default-settings engine must reserve more than the bare \
             one-column fold indicator — #1543 made `number` the default, \
             so this must never read back as `LineNumberMode::None`'s \
             1-column gutter (got {})",
            p.gutter_cells
        );
        assert_eq!(
            p.first_gutter_text.trim(),
            "1",
            "the first buffer line's gutter must carry its own line \
             number, not blank padding — #1691 reports no line numbers at \
             all (got {:?})",
            p.first_gutter_text
        );
        assert!(
            p.has_gutter_bounds,
            "`Editor::layout` must hand the rasteriser a non-empty \
             `gutter_bounds` — this is the value \
             `quadraui::win::editor::draw_editor`'s `gutter_char_width > 0` \
             gate and its right-alignment arithmetic both derive from"
        );
        let expected_inset = p.gutter_cells as f32 * p.cell_width;
        assert!(
            (p.text_x - (p.pane_x + expected_inset)).abs() < 0.01,
            "the text column must begin exactly where the {}-cell gutter \
             ends (pane x={} + {expected_inset}px = {}); #1691 reports text \
             flush against the pane's own left edge instead (got text_x={})",
            p.gutter_cells,
            p.pane_x,
            p.pane_x + expected_inset,
            p.text_x
        );
        assert!(
            p.text_x > p.pane_x,
            "sanity restatement of #1691's exact symptom: text must not be \
             flush against the pane edge (pane x={}, text x={})",
            p.pane_x,
            p.text_x
        );
    }

    /// The falsifiability control for the test above (CLAUDE.md rule 2).
    ///
    /// Drives the identical probe against a `:set nonumber` engine and
    /// pins the geometry #1691 *reports*: a 1-cell gutter carrying only the
    /// fold indicator, no digits. Every assertion in the positive test
    /// above fails against this state, which is what makes them coverage
    /// rather than tautology — and it pins the one `LineNumberMode` that
    /// could legitimately produce the reported symptom, so a dell64
    /// reproduction can be attributed to a stray `nonumber` override by
    /// comparing against this exact shape.
    #[test]
    fn nonumber_collapses_the_gutter_and_leaves_text_nearly_flush_control_1691() {
        let p = probe_gutter(crate::core::settings::LineNumberMode::None);

        assert_eq!(
            p.gutter_cells, 1,
            "`nonumber` keeps only the 1-column fold indicator"
        );
        assert_eq!(
            p.first_gutter_text.trim(),
            "",
            "`nonumber` must paint no digits in the gutter (got {:?})",
            p.first_gutter_text
        );
        assert!(
            (p.text_x - (p.pane_x + p.cell_width)).abs() < 0.01,
            "`nonumber`'s text column sits one fold-indicator cell from the \
             pane edge — visually indistinguishable from #1691's \"flush\" \
             report (pane x={}, text x={}, cell={})",
            p.pane_x,
            p.text_x,
            p.cell_width
        );
    }
}

// ── #1869: GUI minimap and scrollbar widths don't match VS Code ──────────
//
// Driver-tier coverage for the Win-GUI half of #1869's acceptance bullet
// ("a black-box test drives a GUI backend ... at a fixed pane width"),
// alongside `gtk::testing::minimap`'s own two tests. Unlike `src/macos/`
// (whole-module `target_os = "macos"`-gated, so nothing in it can be typed
// or run off a Mach-O host — see that module's own "Verifying this file
// without a Mac" section), this module compiles and **runs** on an ordinary
// Linux host: `win` is gated on the Cargo feature alone (see `src/lib.rs`'s
// doc comment on `pub mod win`), the same reason
// `win_gutter_contract_1691` above can.
#[cfg(all(test, feature = "win"))]
mod win_minimap_scrollbar_1869 {
    use crate::core::{Engine, WindowRect};
    use crate::render;

    /// Geometry read back from a real `build_screen_layout` call plus the
    /// same `Editor::layout` call `quadraui::win::editor::draw_editor`
    /// makes — no arithmetic recomputed by the probe itself, only read.
    struct MinimapProbe {
        strip_width: f64,
        v_scrollbar_width_px: Option<f64>,
        char_width: f64,
        pane_width: f64,
        gutter_cells: usize,
    }

    /// `width_px` is the pane's own full width (there is no sidebar/
    /// activity-bar chrome in this single-window fixture, unlike
    /// `gtk::testing::harness`, so the `WindowRect` passed in *is* the
    /// pane's rect `build_screen_layout` sees). A 2000-line buffer and a
    /// 900px-tall window keep the vertical scrollbar reserved
    /// (`quadraui::primitives::editor::Editor::layout`'s `has_v_scrollbar`
    /// needs `total_lines > visible_lines`), so `v_scrollbar_width_px` is
    /// never vacuously `None`.
    fn minimap_probe(width_px: f64) -> MinimapProbe {
        use quadraui::Backend as _;

        let backend = super::backend::WinBackend::new();
        let char_width = backend.char_width() as f64;
        let line_height = backend.line_height() as f64;
        let scrollbar_reserve = backend.scrollbar_reserve() as f64;
        assert!(
            char_width > 0.0 && line_height > 0.0,
            "WinBackend must report real text metrics before any minimap \
             arithmetic means anything (got char_width={char_width}, \
             line_height={line_height})"
        );

        let mut engine = Engine::new_for_test();
        // #1858: the minimap defaults off on every backend — explicitly on
        // here, since reaching it at all is the whole point of this probe.
        engine.settings.minimap = true;
        let text: String = (0..2000).map(|i| format!("vimcodeline{i}\n")).collect();
        engine.buffer_mut().insert(0, &text);

        let theme = render::Theme::vscode_dark();
        let bounds = WindowRect::new(0.0, 0.0, width_px, 900.0);
        let tab_bar_h = render::tab_row_height_px(line_height);
        let (rects, _) = engine.calculate_group_window_rects(bounds, tab_bar_h);
        let layout = render::build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            true,
            scrollbar_reserve,
            render::gtk_minimap_sizing(),
        );
        let rw = layout
            .windows
            .first()
            .expect("a single-group layout must paint exactly one window");
        let mm = layout.minimap.first().expect(
            "a pane this wide with the minimap explicitly on must paint a \
             strip — if this fails, the fixture's own width is too narrow \
             for MINIMAP_MIN_TEXT_COLS's self-suppression check",
        );

        let editor = render::to_q_editor(rw);
        let el = editor.layout(editor.rect, char_width as f32, line_height as f32);

        MinimapProbe {
            strip_width: mm.rect.width,
            v_scrollbar_width_px: el.v_scrollbar_bounds.map(|r| r.width as f64),
            char_width,
            pane_width: rw.rect.width,
            gutter_cells: rw.gutter_char_width,
        }
    }

    /// #1869 acceptance: "the minimap width equals the VS Code formula's
    /// result, which is less than 120 at a pane narrow enough to be under
    /// the cap." Hand-computed independently of
    /// `minimap_reserved_width`/`vs_code_minimap_width_px` (reverting
    /// either back to the pre-#1869 fraction formula must make this fail,
    /// which calling them to compute "expected" cannot do — the same
    /// tautology the #1869 review round 1 finding flagged in
    /// `gtk::testing::minimap`).
    ///
    /// RED against the pre-#1869-round-1 shape, which fed
    /// `vs_code_minimap_width_px` the pane's raw width instead of
    /// `remainingWidth = pane width - gutter`: at this fixture's geometry
    /// (`char_width=8`, 2000 lines → a 7-cell gutter, `gutter_px=56`) that
    /// shape would have resolved `floor((700 - 14 - 2) / 9) + 8 = 84`
    /// instead of this test's own hand-computed, gutter-subtracted value.
    /// Confirmed by temporarily reverting
    /// `window_minimap_gutter_width_px`'s call site in
    /// `build_screen_layout_with_breadcrumb_row` to pass `0.0` and
    /// re-running this test: it fails with `left: 84.0, right: 77.0` (the
    /// un-gutter-subtracted production value against this test's own
    /// hand-computed, gutter-subtracted `expected`) before being reverted
    /// back.
    #[test]
    fn minimap_width_matches_vs_code_formula_on_a_narrow_pane_1869() {
        let p = minimap_probe(700.0);
        let gutter_px = p.gutter_cells as f64 * p.char_width;
        let remaining = p.pane_width - gutter_px;
        let inner = ((remaining - 14.0 - 2.0) / (p.char_width + 1.0))
            .floor()
            .max(0.0);
        let expected = (inner + 8.0).min(120.0);

        assert_eq!(
            p.strip_width, expected,
            "the real paint path must reserve exactly what VS Code's own \
             minimap formula (floor((remainingWidth - 14 - 2) / (char_width \
             + 1)) + 8, capped at 120) computes by hand (pane_width={}, \
             gutter_cells={}, char_width={})",
            p.pane_width, p.gutter_cells, p.char_width
        );
        assert!(
            expected < 120.0,
            "test setup sanity: a 700px pane must stay under the 120px \
             cap, or this isn't exercising the formula at all (got \
             {expected})"
        );
    }

    /// #1869 acceptance: "... and exactly 120 on a wide pane."
    #[test]
    fn minimap_width_caps_at_exactly_120_on_a_wide_pane_1869() {
        let p = minimap_probe(1400.0);
        let gutter_px = p.gutter_cells as f64 * p.char_width;
        let remaining = p.pane_width - gutter_px;
        let inner = ((remaining - 14.0 - 2.0) / (p.char_width + 1.0))
            .floor()
            .max(0.0);
        let expected = (inner + 8.0).min(120.0);
        assert_eq!(
            expected, 120.0,
            "test setup sanity: this fixture's own hand-computed formula \
             must actually hit the cap, or this isn't a wide-pane test"
        );

        assert_eq!(
            p.strip_width, 120.0,
            "a 1400px Win-GUI pane must cap at *exactly* VS Code's 120px \
             minimap width (got {})",
            p.strip_width
        );
    }

    /// #1869's still-open half: the vertical scrollbar gutter. VS Code's
    /// own default (`editor.scrollbar.verticalScrollbarSize`) is a fixed
    /// 14px regardless of font; quadraui's `Editor::layout` instead sizes
    /// it at `cell_width` (`quadraui-0.1.2/src/primitives/editor.rs`,
    /// `v_scrollbar_w = if has_v_scrollbar { cell_width } else { 0.0 }`),
    /// with no way for a host to override it. This is the drafted,
    /// not-yet-filed gap in `docs/PENDING_QUADRAUI_ISSUES.md` — pinning the
    /// *current*, still-wrong width here (rather than leaving it
    /// unasserted) means this test goes red the day quadraui ships a fix,
    /// which is the trigger to update this assertion and close that entry,
    /// not a silent drift.
    #[test]
    fn vertical_scrollbar_is_still_cell_width_not_vs_codes_14px_1869() {
        let p = minimap_probe(1400.0);
        let got = p
            .v_scrollbar_width_px
            .expect("a 2000-line buffer in a 900px-tall window must overflow and reserve a vertical scrollbar");
        assert_eq!(
            got, p.char_width,
            "quadraui still sizes the vertical scrollbar at the editor's \
             own cell width, not VS Code's fixed 14px — if this now fails, \
             quadraui shipped the host-settable scrollbar width \
             docs/PENDING_QUADRAUI_ISSUES.md's #1869 entry asks for; update \
             this test to assert exactly 14.0 and close that entry instead \
             of re-tuning the fixture (got {got}, char_width={})",
            p.char_width
        );
    }
}

// ── #1696: editor text not inset by the minimap strip; tab-strip
// overflow-action toolbar clipped off the window's right edge ───────────
//
// vimcode#1696 (bugbash finding, real-hardware screenshot evidence on
// dell64): with the minimap on and `'nowrap'` (vim's default), a long
// buffer line paints glyphs under the minimap strip, which then overpaints
// the last character or two; separately, the tab-strip's trailing `"⋯"`
// overflow-action control renders as two dots rather than a full ellipse,
// read as the glyph being clipped by the window's own right edge.
//
// Investigated against the shared `src/render.rs` frame-composition path
// the issue itself names first (`#764`'s converged "editor band" — see
// that file's own doc comment), **not** this module — both symptoms
// root-cause to code (vimcode's own `to_q_editor`, and the pinned
// `quadraui::primitives::tab_bar::TabBar::layout`) that every GUI/TUI
// backend shares equally, not anything specific to `WinBackend`/`win::run`.
// Nothing in `src/win/` changes for either half, matching this file's own
// "no layout, hit-test, paint or dispatch decision" posture (its own
// top-of-file doc comment).
//
// * **Minimap half:** `to_q_editor` (`src/render.rs`) builds
//   `quadraui::Editor` with `rect = rw.rect` verbatim, never consulting
//   `RenderedWindow.minimap_reserved_w` — by #1094's own design, so that
//   quadraui's drawn v/h scrollbar (`EditorLayout::v_scrollbar_bounds`/
//   `h_scrollbar_bounds`, anchored at `viewport.x + viewport.width`) lands
//   past the strip at the pane's true right edge rather than immediately
//   before it. Narrowing `to_q_editor`'s `rect` in vimcode alone would fix
//   the text overlap but pull that scrollbar in to sit flush against the
//   narrower text instead — reopening #1094 on GTK, TUI, *and* Win-GUI at
//   once (all three now share this exact code path since `#1433`/`#1434`).
//   Confirmed numerically, not just by reading the source:
//   `render::tests::to_q_editor_does_not_narrow_the_viewport_for_the_
//   minimap_strip_1696` (`src/render.rs`) is a backend-neutral test (no
//   Windows/GTK display needed, runs on any host) that builds a
//   `RenderedWindow` fixture with a non-zero `minimap_reserved_w`, runs it
//   through the real `to_q_editor` + `quadraui::Editor::layout`, and
//   passes today precisely because `EditorLayout::text_bounds` is *not*
//   narrowed — pinning the gap as a concrete regression target for the
//   missing quadraui capability (`Editor`/`EditorPaintOptions` needs a way
//   to reserve trailing content width independently of where the
//   scrollbar anchors) rather than a vimcode-side arithmetic bug.
//
// * **Tab-bar half:** `quadraui::primitives::tab_bar::TabBar::layout`
//   positions every `right_segments` entry — including the trailing
//   `tab:action_menu` `"⋯"` control `render::build_tab_bar_primitive`
//   appends — flush against whatever `bar_width` the caller passes
//   (`seg_x = bar_width - right_area_width`), with no outer-edge inset at
//   all. `primitives::status_bar::PIXEL_EDGE_INSET` already solved this
//   identical problem for the status bar (#1155); `TabBar` never grew the
//   analogue. The width vimcode hands `TabBar::layout` is the real
//   window's real content width — `#1561`'s own investigation (directly
//   above/below in this file's history) already confirmed `WinBackend::
//   attach_surface`/`resize_surface` derive the render target's pixel
//   size from one un-split `GetClientRect`/`WM_SIZE` pair, no double-
//   subtraction or DIP/physical mismatch — so this is not a `src/win/`
//   sizing bug either.
//
// Both findings, their concrete `Ask`s, and why neither is fixable from
// this repo alone (narrowing the caller-side rect/width either reopens
// #1094 or desyncs the tab bar's background fill from its own segment
// layout — the exact measure/paint-desync failure shape `#654`/`#703`
// already exist to prevent) are written up in full in
// `docs/PENDING_QUADRAUI_ISSUES.md`'s two matching #1696 entries.
// `tests/smoke-spec/win-gui.yaml` carries a matching cross-reference next
// to its existing "DELIBERATELY OMITTED" comment, since this issue's own
// acceptance bar (pixel-content assertions) hits the identical
// `win_native_driver.py` vocabulary gap that comment already names.

// ── #1674: Ctrl-modified shortcuts (Ctrl+`, Ctrl+B) never dispatched on
// Win-GUI ────────────────────────────────────────────────────────────────
//
// vimcode#1674 (bugbash finding, real-hardware-confirmed: window focus
// established via an actual mouse click, `GetForegroundWindow() == target
// hwnd` verified before each injection, reproduced via both `SendKeys` and
// raw `keybd_event(VK_CONTROL + VK_OEM_3)`) reports that neither Ctrl+`
// (open/toggle terminal) nor Ctrl+B (toggle sidebar) has any visible
// effect, while unmodified keys (typing, Enter, Escape) and mouse clicks
// work fine in the same session — isolating the break to the Ctrl
// modifier specifically.
//
// Like a backend trait method, "Ctrl+<key> reaches the engine" is not a
// `src/win/` decision: `App::setup` (`src/app.rs`) registers the same
// 15-entry panel-accelerator table (`render::register_panel_accelerators`)
// on every GUI backend identically, `Engine::handle_vscode_key`
// (`src/core/engine/vscode.rs`) is the one shared handler for the raw
// fallback path, and the accelerator-matching itself
// (`quadraui::backend_core::BackendCore::match_keypress`) is shared code
// `WinBackend::match_keypress`/`GtkBackend::match_keypress` both delegate
// to verbatim. None of that is a plausible site for a Win-GUI-only
// regression. The one layer none of that shared code can reach is
// upstream of all of it: the raw `WM_KEYDOWN`/`WM_CHAR` →
// `quadraui::UiEvent` translation (`quadraui::win::events`), which is the
// one thing genuinely different between backends — GTK's GDK and the
// TUI's crossterm each hand over an already-fully-resolved key+modifier
// pair; Win32 does not.
//
// Source-level investigation (pinned rev
// `ca7fcc83afad01ec3422f79366566f3a263b22bf`, `quadraui/src/win/events.rs`
// + `run.rs`), backed by **executing** (not just type-checked) tests
// against quadraui's own public `win::events` API below, found that
// `events::wm_char_to_uievent` — the pure `WM_CHAR` → `UiEvent` translator
// — correctly recovers *both* chords when fed the payload Windows is
// documented to deliver for each:
//
// - Ctrl+B: Windows' keyboard driver converts Ctrl+letter to its C0
//   control code (`0x02` for B) via `TranslateMessage`, and
//   `ctrl_b_wm_char_recovers_the_base_letter_1674` confirms
//   `wm_char_to_uievent('\x02', {ctrl:true}, _)` correctly recovers
//   `Key::Char('b')` with `ctrl == true` — the exact recovery GTK's/the
//   TUI's own translators perform for the same chord.
// - Ctrl+`: backtick is *not* a control character (`'`'.is_control() ==
//   false`), so `wm_char_to_uievent` never even reaches its Ctrl-recovery
//   branch for it — it falls straight to the final, unconditional
//   `Some(KeyPressed { key: Char(c), modifiers, .. })` arm.
//   `ctrl_backtick_wm_char_passes_through_with_ctrl_held_1674` confirms
//   `wm_char_to_uievent('\u{60}', {ctrl:true}, _)` already produces exactly
//   the `KeyPressed(Char('`'), ctrl: true)` event
//   `Engine::handle_vscode_key`'s `"grave" | "\`"` arm needs.
//
// So **the translation function itself is not at fault for either
// chord** — this corrects an earlier, narrower theory in this same
// investigation (that `events::vk_to_named_key` having no `VK_OEM_3`
// entry meant Ctrl+` had no delivery path at all): `vk_to_named_key`
// genuinely has no backtick entry, but that's immaterial once
// `wm_char_to_uievent` is confirmed to handle the `WM_CHAR` path
// correctly on its own.
//
// What neither this file nor any test it can run addresses is the one
// remaining, genuinely OS-level question: **does Windows' real
// `TranslateMessage` actually generate a `WM_CHAR` message at all for
// Ctrl+backtick** (and, for Ctrl+B, does `GetKeyState(VK_CONTROL)` read
// `true` at the moment `win_key_modifiers()` samples it during injected —
// not physically typed — input)? Win32 keyboard-input references
// consistently describe `WM_CHAR` generation for Ctrl held with a
// non-letter key as layout/driver-dependent, unlike the uniformly
// documented Ctrl+letter C0 conversion — but confirming (or ruling out)
// that this is the actual cause needs a live Windows message loop, which
// no test in `quadraui::win::testing` (`WinDriver` synthesizes an
// already-decoded `UiEvent` directly, bypassing `events.rs`/`run.rs`
// entirely — see `ctrl_accelerator_dispatch_reaches_engine_via_
// win_driver_1674` below) or in this repo can observe — the same "last
// link only real hardware can close" shape `docs/PENDING_QUADRAUI_
// ISSUES.md`'s #1668 entry already describes for `WinDriver`'s missing
// `.tick()`. See that doc's new entry for this issue for the concrete
// real-hardware diagnostic this needs next.
#[cfg(all(test, feature = "win"))]
mod win_ctrl_key_translation_tests_1674 {
    use quadraui::win::events::wm_char_to_uievent;
    use quadraui::{Key, Modifiers, UiEvent};

    /// Windows' keyboard driver converts Ctrl+letter to its C0 control
    /// code (`0x02` for B) via `TranslateMessage` — this confirms
    /// `wm_char_to_uievent` correctly recovers that back to
    /// `Key::Char('b')` with `ctrl` held, exactly as GTK's/the TUI's own
    /// translators already do for the issue's own confirmed-working
    /// backends. Genuinely executes on Linux: `wm_char_to_uievent` is a
    /// pure function, no WinAPI call.
    #[test]
    fn ctrl_b_wm_char_recovers_the_base_letter_1674() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        // `0x02` (STX) is the C0 control code Windows' keyboard driver
        // produces for Ctrl+B via `TranslateMessage`.
        let event = wm_char_to_uievent('\u{2}', ctrl, false);
        assert_eq!(
            event,
            Some(UiEvent::KeyPressed {
                key: Key::Char('b'),
                modifiers: ctrl,
                repeat: false,
            }),
            "wm_char_to_uievent must recover Ctrl+B's C0 control code \
             (0x02) back to Key::Char('b') with ctrl held, exactly as GTK's/ \
             the TUI's own translators already do for the issue's own \
             confirmed-working backends"
        );
    }

    /// Backtick is not a control character, so `wm_char_to_uievent` never
    /// reaches its Ctrl-recovery branch for it at all — it passes the
    /// literal `'`'` straight through with `ctrl` still set on
    /// `modifiers`, which is exactly the `KeyPressed(Char('`'), ctrl:
    /// true)` event `Engine::handle_vscode_key`'s `"grave" | "\`"` arm
    /// needs. Demonstrates that *if* Windows delivers `WM_CHAR('`', ...)`
    /// while Ctrl is held, the translation is already correct — the open
    /// question this entry's doc comment narrows to is purely whether
    /// Windows actually generates that message, not anything in this
    /// function.
    #[test]
    fn ctrl_backtick_wm_char_passes_through_with_ctrl_held_1674() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let event = wm_char_to_uievent('`', ctrl, false);
        assert_eq!(
            event,
            Some(UiEvent::KeyPressed {
                key: Key::Char('`'),
                modifiers: ctrl,
                repeat: false,
            }),
            "wm_char_to_uievent must pass a non-control character like \
             backtick straight through with ctrl still set on modifiers"
        );
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
// wrong thing". Two layers of coverage back this fix: `src/app.rs::
// terminal_poll_rearm_tests` unit-tests the decision in isolation (a
// pure, `Backend`-free function test), and `src/tui_main/
// app_on_tui_tests.rs`'s `terminal_poll_rearm_1668` module drives the
// real, shared `App::tick_dispatch` through `quadraui::tui::testing::
// TuiDriver::tick()` and asserts on `TuiBackend::frame_requests`/
// `pending_frame_delay` (quadraui#832's real `Backend`-call-count
// instrumentation) — a genuine driver-tier, black-box test of the exact
// platform-neutral decision Win-GUI's own `WM_TIMER` loop depends on,
// both RED-verified against this function always returning `None`. See
// `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry for the one thing neither
// of those two can reach: `WinBackend` itself. `WinDriver` (unlike
// `TuiDriver`/`MacDriver`) has no `.tick()` at all, and its
// `attach_headless` never sets `WinBackend::hwnd`, so a `WinDriver`-based
// scenario could not have observed whether a real Win32
// `SetTimer`/`WM_TIMER` round-trip actually re-fires `tick` on a live
// Windows message loop — only real hardware (or that quadraui-side fix)
// can close that last link.
#[cfg(all(test, feature = "win"))]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod win_driver_tests {
    use std::cell::{Cell, RefCell};
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

    /// [`conformance_harness`], plus a live [`crate::harness::
    /// ConformanceHarness::screen_layout`] handle (#987) — needed by any
    /// scenario that must locate a *window's* own painted rect (e.g. the
    /// minimap strip, #1676) rather than a text run's. Mirrors
    /// `crate::gtk::testing::conformance_harness`'s identical shape; the
    /// plain [`conformance_harness`] above leaves this field empty
    /// (`None`) because every scenario added before #1676 only ever
    /// needed `find_bounds`/`screen_contains` against painted text.
    fn conformance_harness_with_screen_layout(
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
        let screen_layout = Rc::clone(&app.cached_screen_layout);
        let driver = driver_with_shell(app, config, width, height);
        ConformanceHarness::new_with_screen_layout(driver, engine, screen_layout, paint, cwd)
    }

    /// [`conformance_harness`], plus a live handle to `App::menu_row_rect`
    /// (#552/#720) — needed by [`activity_bar_paints_active_accent_strip_
    /// on_the_open_panel_1689`] below to locate the activity bar's own
    /// y-origin (`menu_row_rect.y + menu_row_rect.height`, the bottom edge
    /// of the reserved title-bar band the activity bar sits directly below)
    /// without hardcoding chrome pixel geometry — mirrors `crate::gtk::
    /// testing::Harness`'s identical `menu_row_rect` capture (that struct's
    /// own doc: "so the headless test harness can... aim pixel probes at
    /// the row the renderer actually used, instead of hardcoding chrome
    /// coordinates"). `menu_row_rect` lives on the backend-neutral `App`
    /// itself (`src/app.rs`), not behind any GTK-specific type, so this is
    /// the same one-line `Rc::clone` capture as `conformance_harness_with_
    /// screen_layout` above, just for a different field.
    fn conformance_harness_with_menu_row_rect(
        engine: Engine,
        width: u32,
        height: u32,
    ) -> (
        ConformanceHarness<quadraui::win::testing::WinDriver<impl quadraui::AppLogic>>,
        Rc<Cell<quadraui::Rect>>,
    ) {
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
        let menu_row_rect = Rc::clone(&app.menu_row_rect);
        let driver = driver_with_shell(app, config, width, height);
        (
            ConformanceHarness::new(driver, engine, paint, cwd),
            menu_row_rect,
        )
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

    // ── #1675: caption-button real click position is still HTCAPTION ───
    //
    // vimcode#1675 is a fresh bugbash re-report of vimcode#1656's own
    // still-open finding (`docs/PENDING_QUADRAUI_ISSUES.md`'s
    // "`WinBackend::register_status_bar_segment_zones` ... registers ...
    // bar-local bounds as if they were already absolute" entry): a real
    // click on the close button at (978,16) leaves the window open after
    // 2000ms, and a real `WM_NCHITTEST` probe at all three caption-button
    // positions still answers `HTCAPTION`. Confirmed still live by reading
    // the *current* pin (`ca7fcc83afad01ec3422f79366566f3a263b22bf`) source
    // directly, not just re-citing the prior entry: `quadraui/src/win/
    // status_bar.rs::win_status_bar_layout` calls `bar.layout_padded(rect.
    // width, rect.height, ...)` with no `rect.x`/`rect.y` at all, and
    // `quadraui/src/win/backend.rs::draw_status_bar_interactive` (both the
    // DWrite-surface branch and this no-surface fallback) feeds that
    // bar-local layout straight into `register_status_bar_segment_zones`,
    // which registers each segment's `bounds` verbatim. The window-controls
    // bar paints near the window's right edge (`rect.x` on the order of
    // 850-980px in a 1024px-wide window), so the registered zone for e.g.
    // `render::WINDOW_CLOSE_ACTION` ends up roughly that many pixels left
    // of the button's real screen position -- a real click at the real
    // position finds no matching (smaller-than-band) zone, and
    // `WinBackend::nc_hit_test` falls through to `HTCAPTION`.
    //
    // This closes the specific Tier-1 coverage gap
    // `win_gui_smoke_spec_title_band_coordinates_are_stale_1657`'s own doc
    // names above ("the caption-button half of that test has no equivalent
    // here, [since] `ConformanceHarness` ... does not clone `App::
    // title_bar_rect`") by locating the close button's *real* painted
    // position via `WinDriver::find` (CLAUDE.md's "locate targets, never
    // hardcode coordinates" rule) instead of needing `title_bar_rect` at
    // all -- the same technique `src/gtk/testing.rs`'s
    // `click_titlebar_close_button` already uses for the GTK sibling.
    //
    // Asserts the *correct* expected behaviour (`HTCLIENT`, i.e.
    // `Some(false)`) rather than asserting the bug persists -- this is a
    // source-level RED confirmation, not an executed one, for the same
    // `#1558` DLL-load-crash reason every other test in this module carries
    // that disclaimer (`WinDriver::new`'s offscreen Direct2D surface
    // creation panics unconditionally off real Windows, per that
    // constructor's own doc, so this function is only type-checked here,
    // never run) -- stated explicitly here rather than left to the
    // inherited blanket disclaimer, exactly like `win_gui_blank_title_
    // band_strip_is_caption_1661` below. Do not "fix" this by weakening the
    // assertion to match the bug; this is a quadraui-side defect with no
    // vimcode-side fix available (`App::render_content`'s call to
    // `backend.draw_status_bar_interactive` for the window-controls bar is
    // already identical in shape to its calls for the menu bar/command
    // center, both of which work correctly) -- re-run once the pin moves
    // past a fix to `register_status_bar_segment_zones` to confirm the
    // flip to `Some(false)`.
    #[cfg_attr(target_os = "windows", test)]
    fn caption_button_real_click_position_is_misclassified_htcaption_1675() {
        let h = conformance_harness(plain_engine(), 1024, 768);

        let needle = format!("  {}  ", crate::icons::WINDOW_CLOSE.s());
        let (x, y) = h.driver.find(&needle).unwrap_or_else(|| {
            panic!(
                "the inline close button must have painted its {needle:?} \
                 label before a real click position can be probed; painted \
                 runs this frame: {:?}",
                h.driver.painted_texts()
            )
        });

        assert!(
            x > 512.0,
            "sanity: the close button must paint in the right half of a \
             1024-wide window, not near x=0 -- otherwise this probe isn't \
             actually testing the bar-local-vs-absolute gap (got x={x})"
        );

        assert_eq!(
            h.driver.backend().nc_hit_test(x, y),
            Some(false),
            "a real WM_NCHITTEST probe at the close button's own real \
             painted centre ({x}, {y}) must answer HTCLIENT (Some(false)) \
             so a real click reaches it as a MouseDown -- vimcode#1675/\
             #1656 found this still answering HTCAPTION (Some(true)) \
             because `WinBackend::register_status_bar_segment_zones` \
             registers the button's bar-local layout bounds, not translated \
             by the window-controls bar's real `rect.x`; see \
             docs/PENDING_QUADRAUI_ISSUES.md's entry for the full trace"
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

    // ── #1676: the minimap strip can show raw OS white instead of the
    // active theme ──────────────────────────────────────────────────────
    //
    // vimcode#1676 reports the minimap's ~96px-wide strip painting as a
    // flat, almost-blank white rectangle regardless of the active theme —
    // present immediately on launch in most runs, and reproducible after
    // a window resize in every run. Root-caused (not reproduced on real
    // hardware — no live, unlocked Windows host reachable from this
    // session) to a gap in `WinBackend::draw_minimap`'s own, already-
    // tested "no surface attached yet" fallback: `Backend::draw_minimap`
    // can legitimately return `MinimapPaintResult { painted: false, .. }`
    // whenever `WinBackend` has no live Direct2D surface yet (the
    // synchronous first `WM_SIZE` Windows fires from inside
    // `CreateWindowExW`, or an `EndDraw` failure — device loss / RDP
    // session change — dropping the surface until the next `WM_PAINT`'s
    // `ensure_surface()` call recovers it). `WinBackend::begin_frame`'s
    // own `Clear()` call (the thing that paints the *entire* render
    // target to `theme.background` every frame) is gated on that exact
    // same "surface attached" check, so a frame landing in either window
    // paints nothing at all — and the on-screen result for a client-area
    // pixel Direct2D has never actually Present-ed is the OS/DWM's own
    // default backing colour, white, not any theme's background. Neither
    // `src/render.rs::draw_minimap_strip` (this repo's own shared,
    // backend-neutral call site) nor quadraui's `win::run` ever inspects
    // `MinimapPaintResult::painted` to retry or paper over that frame.
    // Full analysis in `docs/PENDING_QUADRAUI_ISSUES.md`'s matching entry.
    //
    // No vimcode-side fix is available per the Platform-Neutrality Rule —
    // this file is a 1-line quadraui re-export (this module's own
    // top-of-file doc), and every function named above lives in
    // `quadraui::win::backend`/`quadraui::win::run`, not here.
    //
    // **What this test can and cannot prove:** the triggering state
    // itself (`WinBackend` with no surface attached, mid-session) has no
    // public or `pub(crate)`-to-vimcode entry point — `WinBackend::
    // surface`/`ensure_surface`/`resize_surface` are `pub(crate)` *to
    // quadraui*, and `quadraui::win::testing::WinDriver::new`/
    // `.attach_headless` always attaches a surface eagerly, with no
    // "drop it again" hook exposed to a downstream crate's test. So this
    // cannot be the RED-then-GREEN regression test for the actual
    // reported defect — doing that needs a quadraui-side test-harness
    // primitive this file's own `docs/PENDING_QUADRAUI_ISSUES.md` entry
    // asks for (mirroring the "`WinDriver` has no `.tick()`" entry's own
    // shape immediately above it in that file). What it proves instead:
    // the one piece of this contract reachable from here today — that
    // once a surface *is* attached (every `WinDriver`-backed scenario in
    // this module, including this one), the minimap strip paints
    // `theme.background`, matching the live theme, and is never left
    // showing raw white — a regression guard for whichever fix lands
    // upstream, RED-verifiable in principle by reverting `win::minimap::
    // draw_minimap_scaled`'s `fill_rect(target, rect, theme.background)`
    // call, though that revert could not be exercised from this Linux
    // worktree either (module-top `#1558` disclaimer: only type-checked
    // here, `#[cfg_attr(target_os = "windows", test)]`, same as every
    // other test in this module).
    #[cfg_attr(target_os = "windows", test)]
    fn minimap_strip_background_matches_theme_once_a_surface_is_attached_1676() {
        let mut engine = plain_engine();
        // vscode-dark (the default) keeps `theme.background` far from
        // white (`rgb(255, 255, 255)`) — the exact colour vimcode#1676
        // reports — so a regression that ever left this probe unpainted
        // would be pixel-exact wrong here, no tolerance needed; using
        // vscode-*light* instead would make "painted white" and "hit the
        // bug" indistinguishable, defeating the point of this probe.
        engine.settings.colorscheme = "vscode-dark".to_string();
        // #1858: minimap is off by default on every backend now — this
        // probe is about the minimap's painted background, so it must
        // turn the setting on explicitly rather than rely on a default
        // that no longer exists.
        engine.settings.minimap = true;
        // Every inserted line is blank: `win::minimap::paint_row_blocks`
        // skips whitespace columns entirely and `paint_row_glyphs` draws
        // an empty string either way, so no row in the strip ever paints
        // over the `Clear()`'d background — any probe pixel inside the
        // strip's bounds is purely `theme.background`, regardless of
        // which of the two render modes `minimap_scale` resolves to.
        engine.buffer_mut().insert(0, &"\n".repeat(200));

        let mut h = conformance_harness_with_screen_layout(engine, 1400, 900);
        // See `colorscheme_change_repaints_editor_and_explorer_sidebar_1667`'s
        // identical note above: `App::sync_per_frame_backend_state` pushes
        // the live theme onto the backend at the *start* of
        // `render_content`, so a widget painted before that call on the
        // very first frame can lag by one paint — a second render pass
        // keeps this probe robust to that ordering.
        h.driver.render();

        let win = h.engine.borrow().active_window_id();
        let strip = {
            let layout = h.screen_layout.borrow();
            let l = layout
                .as_ref()
                .expect("a frame must have painted a screen layout");
            l.minimap
                .iter()
                .find(|m| m.window_id == win)
                .expect("the minimap must be present for the active pane")
                .rect
        };
        assert!(
            strip.width > 0.0 && strip.height > 0.0,
            "fixture assumption: the minimap must actually be visible at \
             this window width, or this probe's premise doesn't hold; got \
             {strip:?}"
        );

        let theme = crate::render::Theme::vscode_dark();
        let probe_x = (strip.x + 2.0) as u32;
        let probe_y = (strip.y + strip.height / 2.0) as u32;
        let px = h.driver.pixel(probe_x, probe_y);
        assert_eq!(
            (px.r, px.g, px.b),
            (theme.background.r, theme.background.g, theme.background.b),
            "the minimap strip must paint `theme.background`, not raw OS \
             white — vimcode#1676's reported symptom (see this test's own \
             doc comment above, and `docs/PENDING_QUADRAUI_ISSUES.md`'s \
             matching entry, for why this scenario alone cannot reproduce \
             the actual reported defect); got {px:?}"
        );
        assert_ne!(
            (px.r, px.g, px.b),
            (255, 255, 255),
            "sanity: vscode-dark's own background must not itself be \
             white, or this probe can't distinguish a correct paint from \
             vimcode#1676's reported bug"
        );
    }

    // ── #1691: no line-number gutter paints on Win-GUI despite #1543
    // making `number` the default ───────────────────────────────────────
    //
    // vimcode#1691 reports buffer text starting flush against the editor
    // pane's own left edge on Win-GUI — no gutter column, no line
    // numbers, no left inset — which would contradict #1543's shipped
    // default (`Settings::default().line_numbers ==
    // LineNumberMode::Absolute`, `src/core/settings.rs:5729`) and this
    // backend's own documented acceptance bar (`win::editor`'s module
    // doc at the pinned rev: "line numbers ... implemented below").
    //
    // Traced the full pipeline the issue's own "Where it breaks" section
    // points at, end to end, at the pinned rev
    // (`ca7fcc83afad01ec3422f79366566f3a263b22bf`): vimcode's shared,
    // backend-neutral `render::calculate_gutter_cols`/
    // `render::build_rendered_window` (`src/render.rs`) through
    // `render::to_q_editor` — the exact conversion
    // `App::paint_editor_windows_rung` calls for every GUI backend alike
    // (`src/app.rs`), GTK included — to `quadraui::win::editor::
    // draw_editor`'s `if editor.gutter_char_width > 0` gate and
    // `Editor::layout_with_options`'s `gutter_w = gutter_char_width *
    // cell_width` arithmetic (`quadraui/src/primitives/editor.rs`). None
    // of that chain is Win-specific, differs from GTK's identical call
    // chain, or can legitimately produce `gutter_char_width == 0` for a
    // real (non-placeholder) window: `calculate_gutter_cols` returns at
    // least `1` (the bare fold-indicator column) even for
    // `LineNumberMode::None`, and the only `RenderedWindow` construction
    // site that hardcodes `0` (`render.rs`'s `empty()` closure, taken
    // only when the window/buffer lookup fails or the pane hosts a
    // plugin view) cannot be the frame the report's own capture shows,
    // since that frame also painted real buffer text. A `cell_width ==
    // 0` theory doesn't hold up either: `win::editor::paint_line_text`'s
    // own `visible_cols` guard (sourced from the same
    // `Editor::layout_with_options`) would make `end <= scroll_left`
    // true and skip painting the line's text entirely when `cell_width`
    // is `0.0` — contradicting the report's own readable, merely
    // unindented text. GTK already carries a passing regression test for
    // exactly this default
    // (`crate::gtk::testing::vscode_dimming::
    // fresh_engine_paints_absolute_line_numbers_by_default_on_gtk`), and
    // every line of `win::editor::draw_editor`/`Editor::
    // layout_with_options` cited above reads byte-for-byte consistent
    // with that same contract.
    //
    // A `cell_width == 0.0` startup race was also ruled out:
    // `WinBackend::new()` seeds `current_char_width: 8.0` /
    // `current_line_height: 16.0` directly (`quadraui/src/win/
    // backend.rs:768-769` at this pin) — never `0.0` — and the setters
    // only ever replace that non-zero seed with a real measured value.
    //
    // **Scope of this test, and why #1691 stays open.** This function is
    // the *pixel* half of the probe and is `#[cfg_attr(target_os =
    // "windows", test)]`, so it is type-checked here and executable only
    // on dell64 (module-top #1558 disclaimer) — it has never been run,
    // and cannot be the RED-before-fix evidence CLAUDE.md rule 2 asks
    // for. The *arithmetic* half — everything in the call chain above
    // except the Direct2D `draw_text` calls themselves — is covered by
    // `super::win_gutter_contract_1691`, which **does execute on every
    // host** (including this Linux/macOS worktree) against the same
    // production functions and `WinBackend`'s own metrics, and which was
    // RED-verified by injecting the reported defect at both sites #1691
    // names. See that module's header for the injections and their
    // observed failures.
    //
    // What remains genuinely unreachable from a non-Windows host is (a)
    // the Direct2D/DirectWrite draw calls and (b) the *environment*
    // hypothesis the issue's own "Reproduction" section names first —
    // that the dell64 capture's binary predates #1543's default flip, or
    // that session carried a stray `:set nonumber`/`settings.json`
    // override. Neither is a source question. `win-smoke-tests.md`'s
    // "Open real-hardware questions (dell64) — #1691" section carries the
    // concrete checklist for whoever next has that hardware. Until one of
    // those two comes back, #1691 is **reproduced-by-report only, not
    // fixed**, and must stay open.
    #[cfg_attr(target_os = "windows", test)]
    fn line_number_gutter_paints_and_insets_text_by_default_1691() {
        let mut engine = plain_engine();
        engine.settings.colorscheme = "vscode-dark".to_string();
        // Deliberately NOT setting `engine.settings.line_numbers` — #1543
        // made `LineNumberMode::Absolute` the untouched default, and this
        // test is exactly about what a fresh, default-settings engine
        // paints on this backend.
        engine.buffer_mut().insert(
            0,
            &(1..=20)
                .map(|n| format!("vimcodeline{n:02}\n"))
                .collect::<String>(),
        );

        let mut h = conformance_harness_with_screen_layout(engine, 1400, 900);
        h.driver.render();
        // Second pass: see `colorscheme_change_repaints_editor_and_
        // explorer_sidebar_1667`'s identical note above for why a frame
        // painted before `App::sync_per_frame_backend_state` runs can lag
        // by one.
        h.driver.render();

        let win = h.engine.borrow().active_window_id();
        let (rect, gutter_cells) = {
            let layout = h.screen_layout.borrow();
            let l = layout
                .as_ref()
                .expect("a frame must have painted a screen layout");
            let rw = l
                .windows
                .iter()
                .find(|w| w.window_id == win)
                .expect("the active pane must be in the painted layout");
            (rw.rect, rw.gutter_char_width)
        };
        assert!(
            gutter_cells > 1,
            "test setup sanity: a fresh, untouched-settings engine must \
             reserve more than the bare one-column fold indicator — #1543 \
             made `number` the default, so this should never read back as \
             `LineNumberMode::None`'s 1-column gutter (got {gutter_cells})"
        );

        use quadraui::Backend as _;
        let cell_width = h.driver.backend().char_width();
        let line_height = h.driver.backend().line_height();
        assert!(
            cell_width > 0.0,
            "test setup sanity: the backend must report a real character \
             width before this probe's gutter-width arithmetic means \
             anything (got {cell_width})"
        );
        let expected_gutter_px = gutter_cells as f32 * cell_width;

        // ── Buffer text must be inset past the gutter, not flush ───────
        let probe = h
            .driver
            .find_bounds("vimcodeline01")
            .expect("the first buffer line must paint inside the viewport");
        assert!(
            probe.x > rect.x as f32,
            "buffer text must be inset past the gutter, not flush against \
             the pane's own left edge — vimcode#1691's reported symptom; \
             pane left edge is {}, first line painted at x={}",
            rect.x,
            probe.x
        );
        assert!(
            (probe.x - (rect.x as f32 + expected_gutter_px)).abs() < cell_width,
            "buffer text must begin right where the {gutter_cells}-column \
             gutter ends (pane x={} + gutter {expected_gutter_px}px = {}), \
             not at some other, unrelated offset; painted at x={}",
            rect.x,
            rect.x as f32 + expected_gutter_px,
            probe.x
        );

        // ── Gutter glyph ink: a digit must actually be painted in the
        //    leftmost column band, not just a reserved blank one ───────
        fn luma(c: quadraui::Color) -> f64 {
            0.2126 * c.r as f64 + 0.7152 * c.g as f64 + 0.0722 * c.b as f64
        }
        let theme = crate::render::Theme::vscode_dark();
        // Seeded as `None` rather than with an all-zero black sentinel
        // colour: `tests/no_hardcoded_colors.rs` (#1575/#1576) exempts only
        // regions attributed *exactly* `#[cfg(test)]`, and this module is
        // `#[cfg(all(test, feature = "win"))]`, so even a test-only
        // sentinel colour literal here reads to that gate as shipped
        // chrome naming its own colour. The `Option` carries the "no pixel
        // seen yet" state the sentinel stood in for, naming no colour at
        // all — strictly better anyway, since a literal black seed would
        // also silently pass this probe on a theme whose background *is*
        // black.
        let mut brightest: Option<quadraui::Color> = None;
        let x0 = rect.x as i32;
        let x1 = (rect.x as f32 + expected_gutter_px).round() as i32;
        let y0 = rect.y as i32;
        let y1 = (rect.y as f32 + line_height).round() as i32;
        for x in x0..x1 {
            for y in y0..y1 {
                let p = h.driver.pixel(x.max(0) as u32, y.max(0) as u32);
                if brightest.is_none_or(|b| luma(p) > luma(b)) {
                    brightest = Some(p);
                }
            }
        }
        let brightest = brightest.expect(
            "the gutter band must span at least one pixel to probe — a \
             zero-width or zero-height band means the gutter reserved no \
             space at all, which is vimcode#1691's symptom",
        );
        assert_ne!(
            (brightest.r, brightest.g, brightest.b),
            (theme.background.r, theme.background.g, theme.background.b),
            "the gutter column's brightest pixel must differ from the \
             editor background — a line-number digit, not an empty \
             reserved column; got {brightest:?} == theme.background \
             ({:?})",
            theme.background
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

    // ── #1674: isolating half — a Ctrl-modified accelerator through the
    // shared dispatch pipeline, native Win32 translation skipped ────────
    //
    // See this module's top-level `#1674` doc section. `WinDriver::
    // ctrl_char` constructs the already-decoded `UiEvent::KeyPressed {
    // key: Char(c), modifiers: { ctrl: true, .. } }` directly and feeds it
    // through the real, shared `dispatch_event`/`preprocess_event`
    // pipeline — accelerator matching
    // (`quadraui::backend_core::BackendCore::match_keypress`,
    // `WinBackend::match_keypress` delegates to it verbatim, same as
    // `GtkBackend`) through to `App::handle`'s `UiEvent::Accelerator` arm
    // — everything downstream of the raw `WM_KEYDOWN`/`WM_CHAR` →
    // `UiEvent` translation this entry's doc section narrows the open
    // question to.
    //
    // This deliberately dispatches Ctrl+P (`render::ACC_FUZZY_FINDER`,
    // `PanelAccelerator::FuzzyFinder`), not Ctrl+B
    // (`PanelAccelerator::ToggleSidebar`, this issue's own reported
    // chord): `ToggleSidebar`/`OpenTerminal` are two of the five
    // `dispatch_panel_accelerator` actions that only *queue* a
    // `DeferredAction`, applied by `App::tick_dispatch` on the next
    // `tick()` (see `render.rs`'s own doc on that five-action split) —
    // and `WinDriver` has no `.tick()` at all (confirmed: no `pub fn
    // tick` in `quadraui/src/win/testing.rs`, unlike `TuiDriver`/
    // `MacDriver`), the exact same gap `docs/PENDING_QUADRAUI_ISSUES.md`'s
    // #1668 entry already documents. So the *specific* chords this issue
    // reports cannot be driven to a visible result through `WinDriver`
    // today regardless of this bug. `FuzzyFinder` is one of the nine
    // synchronous actions `dispatch_panel_accelerator` applies directly to
    // `Engine` with no queue involved — reachable without `.tick()` — and
    // goes through the identical accelerator-match → `App::handle` steps
    // `ToggleSidebar`/`OpenTerminal` would, so a pass here is still real
    // evidence that the shared pipeline those two chords also depend on is
    // sound on Win-GUI once an already-decoded event reaches it. Expected
    // to pass: if it ever fails, the bug has moved into shared code this
    // repo owns, and this entry's doc-section conclusion needs
    // revisiting. Only type-checked on this Linux worktree
    // (`#[cfg_attr(target_os = "windows", test)]`, same #1558 disclaimer
    // as every other test in this module).
    #[cfg_attr(target_os = "windows", test)]
    fn ctrl_accelerator_dispatch_reaches_engine_via_win_driver_1674() {
        let mut h = conformance_harness(plain_engine(), 1024, 768);
        h.driver.render();

        assert!(
            !h.driver.screen_contains("Go to File"),
            "precondition: the fuzzy-finder picker starts closed; painted \
             texts were {:?}",
            h.driver.painted_texts()
        );

        h.driver.ctrl_char('p');
        h.driver.render();

        assert_eq!(
            h.engine.borrow().picker_source,
            crate::core::engine::PickerSource::Files,
            "Ctrl+P, fed through WinDriver::ctrl_char (the real, shared \
             accelerator-match -> App::handle pipeline, native \
             WM_KEYDOWN/WM_CHAR translation skipped), must resolve to \
             render::ACC_FUZZY_FINDER and open the Files picker"
        );
        assert!(
            h.driver.screen_contains("Go to File"),
            "the fuzzy-finder picker must actually paint, not just set \
             engine state; painted texts were {:?}",
            h.driver.painted_texts()
        );
    }

    // ── #1689: activity bar paints no active-view accent line on Win-GUI ──
    //
    // Root-caused by reading the real source at the pinned rev
    // (`ca7fcc83afad01ec3422f79366566f3a263b22bf`), ruling out both halves
    // vimcode#1689 itself already names as "looking wired":
    //
    // - `render::build_activity_bar` (`src/render.rs:19440`) *does* set
    //   `active_accent: Some(theme.activity_active_accent...)`, exactly as
    //   #1547 intended — but that function has **zero production callers**
    //   (see its own test's doc, `render.rs`'s
    //   `build_activity_bar_active_accent_uses_activity_active_accent_not_
    //   cursor`): every real `App` renders its activity bar through
    //   `quadraui::compose::app_shell::AppShell::build_activity_bar`
    //   instead (`quadraui/src/compose/app_shell.rs:874`), which this
    //   repo's own `App` never overrides or post-processes — no call site
    //   in `src/app.rs` ever constructs or touches a `quadraui::
    //   ActivityBar` directly.
    // - `win::activity_bar::draw_activity_bar`/`native_surface_paint::paint`
    //   (`primitives/activity_bar.rs:656`) *does* paint the 2-DIP left-edge
    //   strip whenever `item.is_active && bar.active_accent.is_some()` —
    //   confirmed by that module's own
    //   `paint_and_hit_test_round_trip` test, which passes today with a
    //   hand-built `ActivityBar { active_accent: Some(..), .. }` fixture.
    //
    // The actual break is upstream of both: `AppShell::build_activity_bar`
    // (`compose/app_shell.rs:932`) hardcodes `active_accent: None` (and
    // `selection_bg: None`) unconditionally, with its own doc comment
    // admitting it outright — `AppShell` has no `Theme` in scope to source
    // a colour from, "that wiring is #381's job" — so every real `App`
    // render, on every backend (GTK/macOS/TUI included, not just Win-GUI;
    // this entry only probes Win-GUI per vimcode#1689's own scope), paints
    // zero accent pixels regardless of which panel is active. Nothing in
    // this vimcode repo can fix this — `AppShell` and its `build_
    // activity_bar` are entirely inside quadraui, and `src/win/backend.rs`/
    // this file are 1-line re-exports, per the Platform-Neutrality Rule.
    // `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry drafts the upstream ask
    // (thread a `&Theme` into `AppShell::build_activity_bar`, or let the
    // caller post-process the returned `ActivityBar` — quadraui#381).
    //
    // Mechanically certain to fail against the pinned rev (every function
    // named above was read directly) and to pass once quadraui wires a
    // theme-sourced `active_accent` through `AppShell::build_activity_bar`
    // — but, per this module's own top-of-file #1558 disclaimer, could not
    // be *executed* from this Linux worktree to observe that RED/GREEN
    // flip directly; this is a source-level RED confirmation, stated
    // explicitly here rather than left to the inherited blanket
    // disclaimer, exactly like `colorscheme_change_repaints_editor_and_
    // explorer_sidebar_1667` above.
    //
    // Geometry: Explorer is `Engine::new_for_test()`'s default active panel
    // (`quadraui::AppShell::new` defaults `active_panel: Some(0)`,
    // `sidebar_visible: true`, and `FIXED_ACTIVITY_PANEL_IDS[0] ==
    // PANEL_EXPLORER`), so no setup is needed to reach the exact scenario
    // the issue reports ("the Explorer is the open view"). The activity
    // bar's y-origin is read from the live `App::menu_row_rect` (via
    // `conformance_harness_with_menu_row_rect`, not hardcoded) rather than
    // assumed, since it depends on the real `title_bar_lh * line_height`
    // product and DirectWrite's own font metrics. Each top item then
    // occupies one fixed `quadraui::win::ACTIVITY_ROW_DIP`-tall band below
    // that, in `FIXED_ACTIVITY_PANEL_IDS` order (Explorer first, Search
    // second) — `ActivityBar::layout`'s own source confirms top items are
    // placed `(i as f32) * item_height` in bar-local y, so index 0 starts
    // at the bar's own top edge with no additional offset to account for.
    #[cfg_attr(target_os = "windows", test)]
    fn activity_bar_paints_active_accent_strip_on_the_open_panel_1689() {
        let mut engine = plain_engine();
        engine.settings.colorscheme = "vscode-dark".to_string();
        let (mut h, menu_row_rect) = conformance_harness_with_menu_row_rect(engine, 1400, 900);
        h.driver.render();

        assert_eq!(
            h.engine
                .borrow()
                .app_shell
                .active_panel_id()
                .map(|w| w.as_str().to_string()),
            Some(crate::core::engine::sidebar::PANEL_EXPLORER.to_string()),
            "precondition: Explorer must be the default active panel, or \
             this scenario isn't the one vimcode#1689 reports"
        );
        assert!(
            h.engine.borrow().app_shell.sidebar_visible(),
            "precondition: the sidebar (and therefore the Explorer panel) \
             must start visible"
        );

        let theme = crate::render::Theme::vscode_dark();
        let accent = theme.activity_active_accent;
        // `quadraui::win::ACTIVITY_ROW_DIP` (= `48.0`, VS-Code parity, same
        // value as `quadraui::gtk::ACTIVITY_ROW_PX`) is gated
        // `#[cfg(target_os = "windows")]` on the re-export path, unlike the
        // rest of this module's symbols — unreachable from a plain
        // `--features win` type-check on Linux, so inlined here with the
        // cross-reference instead of imported.
        let row_h: f32 = 48.0;
        let ab_top = menu_row_rect.get().y + menu_row_rect.get().height;

        // Explorer: top_items[0] -> bar-local y in [0, row_h).
        let explorer_mid_y = (ab_top + row_h / 2.0) as u32;
        // Search: top_items[1] -> bar-local y in [row_h, 2*row_h).
        let search_mid_y = (ab_top + row_h + row_h / 2.0) as u32;

        let active_px = h.driver.pixel(1, explorer_mid_y);
        assert_eq!(
            (active_px.r, active_px.g, active_px.b),
            (accent.r, accent.g, accent.b),
            "the open Explorer panel's row must paint a {:?} accent strip \
             at the activity bar's left edge (x=1, y={explorer_mid_y}) — \
             got {:?} instead (quadraui's `AppShell::build_activity_bar` \
             hardcodes `active_accent: None`, see this test's own doc)",
            accent,
            (active_px.r, active_px.g, active_px.b),
        );

        let inactive_px = h.driver.pixel(1, search_mid_y);
        assert_ne!(
            (inactive_px.r, inactive_px.g, inactive_px.b),
            (accent.r, accent.g, accent.b),
            "the inactive Search row must NOT paint the accent colour at \
             the same x=1 column (y={search_mid_y}) — got {:?}, which would \
             mean the accent painted on every row rather than just the \
             active one",
            (inactive_px.r, inactive_px.g, inactive_px.b),
        );
    }

    // ── #1694: "no breadcrumb bar under the tab strip" on Win-GUI ─────────
    //
    // Root-cause reading (this crate plus the pinned quadraui rev
    // `ca7fcc83afad01ec3422f79366566f3a263b22bf`) found **no** backend-
    // specific code anywhere in the pipeline vimcode#1694's own "where to
    // look" section names, and no divergence between GTK (already proven,
    // `crate::gtk::testing`'s `breadcrumb_segment_click_opens_the_dropdown_
    // and_selection_dispatches` / `breadcrumb_row_adds_a_fixed_22px_not_a_
    // whole_line_height`) and Win-GUI:
    //
    // - `render::breadcrumbs_to_quadraui_status_bar` / `render::
    //   build_screen_layout_with_breadcrumb_row` / `render::
    //   paint_breadcrumb_bars` (the `EditorOp::Breadcrumbs` rung's whole
    //   body, called from `render::paint_editor_band_rungs`) are plain,
    //   backend-neutral functions — no `cfg`, no "am I GTK?" branch.
    // - `render::tab_bar_height_px`'s own doc says "Used by GTK and
    //   Win-GUI backends"; it is the one function that decides whether the
    //   tab strip's reserved band grows by `BREADCRUMB_ROW_HEIGHT_PX`
    //   (22px) when `engine.settings.breadcrumbs` is on, and it has no
    //   backend branch either.
    // - `crate::win::run` — the *real*, non-test entry point, not just this
    //   test module — constructs `App::new_portable(.., UnitProfile::px())`
    //   (`src/win/mod.rs:576`), byte-identical to GTK's own `UnitProfile::
    //   px()` construction.
    // - `quadraui::win::backend::WinBackend::draw_status_bar_interactive`
    //   (`win/backend.rs:2832`) routes through the same shared
    //   `primitives::status_bar::native_surface_paint::paint` every
    //   backend's `StatusBar` (breadcrumbs included) paints through once a
    //   surface is attached — the identical call that already paints this
    //   repo's own per-window status line visibly on Win-GUI (the "status
    //   bar" row in vimcode#1694's own evidence table — present, just
    //   mis-coloured, a separate report).
    // - `Settings::breadcrumbs` defaults `true`
    //   (`core::settings::default_breadcrumbs`), so #1694's own suggested
    //   "confirm it's enabled first" alternative doesn't apply either.
    //
    // No vimcode-side or quadraui-side defect was found to fix — this
    // scenario ships the missing Tier-1 coverage #1694's own Acceptance
    // section asks for ("assert a row carrying the breadcrumbs widget id
    // is painted between the tab strip and the first buffer line"), not a
    // confirmed fix. Per this module's own top-of-file #1558 disclaimer,
    // `WinDriver` cannot be *executed* off real Windows (`HeadlessSurface::
    // new` always `Err`s there), so this is a source-level RED expectation
    // rather than an observed one: reading every call site named above is
    // what makes this mechanically certain to fail if a future change
    // dropped the `EditorOp::Breadcrumbs` rung, or the `settings.
    // breadcrumbs` gate, for this backend specifically — not an executed
    // RED/GREEN flip. dell64 (this repo's real-Windows host, #1558) needs
    // to confirm the flip directly; that confirmation is still outstanding.
    //
    // Geometry: a second, untouched scratch tab (`new_tab(None)`, never
    // given a `file_path`) stays `"[No Name]"` in the tab strip
    // (`BufferState::display_name`'s scratch-buffer branch) — a label
    // guaranteed not to collide with the active tab's breadcrumb path
    // segments or buffer text below, so its painted y anchors "the tab
    // strip" unambiguously. The active tab's buffer then gets a synthetic,
    // nonexistent nested path (`build_breadcrumbs_for_group` only ever
    // manipulates `BufferState::file_path` as a string — see its own body —
    // never touches disk, so the path need not exist, exactly like `crate::
    // gtk::testing`'s own `engine_with_breadcrumb_path` fixture) plus one
    // inserted marker line, so `find` can locate the breadcrumb's own
    // segment text, the tab strip's text and the first buffer line's text
    // as three distinct, unambiguous painted runs.
    fn engine_with_nested_breadcrumb_tab() -> crate::core::Engine {
        let mut engine = plain_engine();
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        engine.cwd = cwd.clone();

        // Tab 0: left as the default scratch buffer, never made active
        // again — its only job is to keep painting "[No Name]" in the tab
        // strip as an unambiguous anchor for that row's y.
        engine.new_tab(None);

        // Tab 1 (now active): synthetic nested path, no disk I/O (see doc
        // above) — segments "srcmarker-1694" / "nestedmarker-1694" /
        // "leafmarker-1694.rs" appear nowhere else on screen.
        let buf = engine.active_buffer_id();
        if let Some(state) = engine.buffer_manager.get_mut(buf) {
            state.file_path = Some(
                cwd.join("srcmarker-1694")
                    .join("nestedmarker-1694")
                    .join("leafmarker-1694.rs"),
            );
        }
        engine.buffer_mut().insert(0, "zzzfirstline-1694\n");
        engine
    }

    #[cfg_attr(target_os = "windows", test)]
    fn breadcrumb_bar_paints_between_tab_strip_and_first_buffer_line_1694() {
        let engine = engine_with_nested_breadcrumb_tab();
        assert!(
            engine.settings.breadcrumbs,
            "fixture assumes breadcrumbs are on by default \
             (core::settings::default_breadcrumbs) — if this ever fails, \
             #1694 is a defaults bug, not a paint bug, per the issue's own \
             \"confirm it's enabled first\" instruction"
        );

        let mut h = conformance_harness(engine, 1400, 900);
        h.driver.render();

        let (_, tab_y) = h.driver.find("No Name").expect(
            "sanity check on the fixture itself: the untouched scratch \
             tab must still paint its \"[No Name]\" label in the tab strip",
        );
        let (_, breadcrumb_y) = h.driver.find("srcmarker-1694").expect(
            "the breadcrumb bar's \"srcmarker-1694\" path segment must \
             paint — vimcode#1694 (\"no breadcrumb bar under the tab \
             strip\")",
        );
        let (_, editor_y) = h.driver.find("zzzfirstline-1694").expect(
            "sanity check on the fixture itself: the first buffer line \
             must paint",
        );

        assert!(
            tab_y < breadcrumb_y,
            "the breadcrumb row must paint below the tab strip: \
             tab_y={tab_y}, breadcrumb_y={breadcrumb_y}"
        );
        assert!(
            breadcrumb_y < editor_y,
            "the breadcrumb row must paint strictly above the first \
             buffer line, not flush against it (i.e. its row's vertical \
             space must actually be reserved): breadcrumb_y={breadcrumb_y}, \
             editor_y={editor_y}"
        );
    }
}
