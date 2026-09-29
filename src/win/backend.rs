//! Re-export of the Win-GUI backend, lifted into `quadraui::win::backend`
//! for cross-app reuse (#866, the Win-GUI twin of #270's GTK re-export).
//!
//! Kept as a module (rather than a bare `pub use` at `src/win/mod.rs`'s top
//! level) so a future `use super::backend::WinBackend` reads exactly like
//! `src/gtk/backend.rs`'s `use super::backend::GtkBackend`. vimcode does not
//! own a backend here either — see that file's doc comment.
//!
//! # #1559: block cursor hides the glyph underneath it is a quadraui gap,
//! # not a vimcode-side one
//!
//! vimcode#1559 reports the NORMAL-mode block cursor painting over (not
//! around) the character underneath it on Win-GUI, unlike GTK/macOS which
//! keep the glyph visible against the block fill. Root-caused entirely
//! inside `quadraui::win::editor::draw_editor`'s `CursorShape::Block` arm:
//! it fills the cursor cell *after* text is painted and never redraws the
//! covered glyph on top, unlike `quadraui::macos::editor::draw_editor`'s
//! same arm, which explicitly re-paints the glyph in `theme.background` on
//! top of the block fill. See `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry
//! for the full analysis, the concrete fix, and the `HeadlessSurface`-based
//! pixel test this issue's acceptance bar asks for. Nothing in this file
//! changes: it re-exports `WinBackend` verbatim (see doc above) and has no
//! rasterising decision of its own to make, per the Platform-Neutrality
//! Rule.
//!
//! **Update (#1618):** the fix landed upstream as quadraui#1197 (the
//! `CursorShape::Block` arm now re-paints the covered glyph in
//! `theme.background`), and the pin now includes it — #1614 had found the
//! quadraui rev containing this fix also contained #1200 (native menu bar),
//! which crashed `vimcode.exe` on every startup (`JDonaghy/quadraui#1213`);
//! that regression's fix (`ce1c763`'s `ModalPumpGuard`, regression-tested by
//! `6e14d8a`) landed upstream and is also in the new pin — see the
//! `Cargo.toml` `quadraui` pin's own comment.
//!
//! **Correction (#1622):** the "Real-hardware verification on dell64
//! confirms the block cursor now shows the glyph underneath it" line above
//! overclaimed — #1618's dell64 session verified #1582's menu bar only
//! (see that issue). #1622 tried to re-drive this one and hit a genuine
//! dell64-local blocker: the interactive session was locked at the OS
//! level for the whole session, and quadraui's own `win::run` `WM_PAINT`
//! handler comment already names locked/RDP-changed sessions as a
//! `ID2D1HwndRenderTarget` device-loss trigger it retries on the next
//! paint (`ensure_surface`). In practice the editor pane never got past
//! that: a live `vimcode.exe` launched cleanly (real `HWND`,
//! `MainWindowTitle` "VimCode", `Responding: True`, no stderr/crash log)
//! but a 12-frame `PrintWindow(PW_RENDERFULLCONTENT)` burst over ~3s was
//! byte-identical on every frame and showed a fully blank editor pane (no
//! text, no cursor, no status bar) while the simpler GDI-composited
//! title/menu chrome painted fine — consistent with the content surface
//! being stuck past its first, pre-lock frame for the rest of the run.
//! `quadraui::win::editor::draw_editor`'s `CursorShape::Block` arm itself
//! is confirmed present at the pinned rev (re-read directly, see the fix
//! excerpt above) and has its own passing `HeadlessSurface` regression
//! test (`block_cursor_repaints_glyph_in_background_colour`) — the fix is
//! real and covered, but a live pixel re-check needs dell64's interactive
//! session unlocked first. Not re-attempted blind a third time.
//!
//! **Reconciliation (#1622 fix round 1):** a second dell64 session
//! reproduced the identical blocker — `GetForegroundWindow()` returned
//! `NULL` throughout, confirming the session was locked again — and added a
//! sharper signal for *why* the editor pane reads blank rather than merely
//! stale: a direct `GetWindowDC` + `BitBlt` read of the live window (as
//! opposed to `PrintWindow`'s synthetic `WM_PRINT` fallback) returned solid
//! **black** for the entire client area while the native, GDI-owned menu
//! row still painted correctly — the same "solid black" signature #1558's
//! own investigation already recorded for `Graphics.CopyFromScreen` under a
//! locked session on this host. This still is not a pixel-level VERIFIED or
//! FAILED result for the cursor glyph specifically (the whole surface is
//! unreadable, not just the cursor), so #1559 remains genuinely blocked —
//! not by lack of trying twice now, but by this host's interactive session
//! relocking on every session that has attempted this check
//! (#1558/#1561/#1622). See `docs/PENDING_QUADRAUI_ISSUES.md`'s "Correction
//! (#1622 fix round 1)" note (filed against the neighbouring #1562 entry)
//! for the full capture-method write-up, which applies identically here.
//!
//! # #1561: left-edge desktop strip — investigated, no fix here
//!
//! vimcode#1561's reported left-edge desktop strip was investigated against
//! `WinBackend::attach_surface`/`resize_surface` (the only two places a
//! render target's pixel size is derived) and, separately, against a real
//! `vimcode.exe` on dell64. See `src/win/mod.rs`'s `#1561` doc section for
//! the full write-up — nothing changes here: this file re-exports
//! `WinBackend` verbatim (see this file's own doc above) and, per the
//! sizing arithmetic already inspected, has no decision of its own that
//! could produce a left-specific gap even in principle.
//!
//! # #1562: chrome parity with macOS — font and status segments already
//! # fine, title bar/command centre is a real quadraui gap
//!
//! vimcode#1562 asked for UI-font resolution, title-bar/command-centre
//! parity, and status-line segment parity with macOS. The font and
//! status-segment items turned out to already be backend-neutral/already
//! fixed upstream (`quadraui::win::backend::parse_ui_font_desc`'s
//! `GenericFamily` resolution; the shared `primitives::status_bar::
//! native_surface_paint::paint` rasteriser both backends call) — nothing
//! for this file to change. The title-bar/command-centre item, at the time
//! of the original investigation, was a real gap: `WinBackend::
//! backend_caps()` declared neither `native_menu` nor `window_chrome`, and
//! `win::run` created a plain `WS_OVERLAPPEDWINDOW` with no custom-caption
//! `WM_NCCALCSIZE`/`WM_NCHITTEST` handling. See `src/win/mod.rs`'s `#1562`
//! doc section for the full write-up and `docs/PENDING_QUADRAUI_ISSUES.md`'s
//! entry for the drafted ask — both are quadraui-side (`WinBackend`/
//! `win::run`), so nothing changes in this 1-line re-export.
//!
//! **Update (#1618):** the fix landed upstream as quadraui#1199
//! (`WinBackend::backend_caps()` now declares `window_chrome: true`,
//! `win::run`'s `wndproc` handles `WM_NCCALCSIZE`/`WM_NCHITTEST`), and the
//! pin now includes it along with the `JDonaghy/quadraui#1213` reentrancy
//! fix that had blocked bumping past it (see the `Cargo.toml` pin comment
//! and `src/win/mod.rs`'s `#1618` update).
//!
//! **Correction (#1622):** the "Real-hardware verification on dell64
//! confirms the custom title bar and command centre now render" line above
//! was wrong — #1618's dell64 session only re-drove #1582's menu bar
//! check, not this one (see #1622's own body). Actually launching
//! `vimcode.exe` on dell64 shows a stock native caption (icon, title, real
//! min/max/close) plus a separate native/owner-drawn Win32 menu row below
//! it — not the single custom-drawn caption with an embedded command
//! centre this issue asks for, and no command-centre search box anywhere
//! in the band. Root cause: `WinBackend::backend_caps()` now declares
//! *both* `native_menu: true` (#1200/#1582) and `window_chrome: true`
//! (#1199/#1562) — the same pair `MacBackend` has declared for a while —
//! and `App::setup`'s `if native_menu {...} else if window_chrome {...}`
//! (`src/app.rs`) always takes the `native_menu` arm first, so
//! `window_chrome`'s arm (which is what sets `menu_bar_visible = true`
//! and makes the drawn CSD row/command centre live) never runs for
//! Win-GUI. `capture_window_and_apply_csd` (`src/app.rs`) also
//! early-returns whenever `native_menu` is set, so `Backend::window()
//! .set_decorated(false)` (the call that clears `WS_CAPTION` and hands
//! the title strip to quadraui's own `WM_NCCALCSIZE`/`WM_NCHITTEST`
//! handling) is never invoked either — Win-GUI keeps its stock decorated
//! window exactly as if #1199 had never landed. This is a capability-
//! modelling gap, not a vimcode branch-order bug safe to patch blind:
//! `native_menu` means two different things across backends (macOS: a
//! true OS-global menu bar with zero in-window footprint, so keeping the
//! native caption is *correct*; Windows: a per-window `SetMenu` `HMENU`
//! sitting directly under the caption, so keeping the native caption is
//! exactly what defeats #1562) and neither `BackendCaps` nor `ShellConfig`
//! expose a flag distinguishing the two — macOS's own
//! `client_side_titlebar`/`titlebar_control_inset` mechanism is a
//! separate, macOS-only hook of that shape that Win-GUI doesn't
//! participate in. See `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry for
//! the drafted ask. `src/win/mod.rs`'s `#1562` doc section has the fuller
//! write-up.
//!
//! **Reconciliation (#1622 fix round 1):** the correction above's "the
//! drawn CSD row/command centre never goes live" claim conflated two rungs
//! #939 deliberately decoupled — `render::FramePresence::from_screen`
//! (`src/render.rs`) computes `command_center` from `title_bar_band_live`
//! alone, not from `menu_bar_visible`/this branch, and both
//! `command_center_liveness_is_split_from_menu_bar_visible` (`src/
//! render.rs`) and this module's own `command_center_paints_on_a_native_
//! menu_backend` test below pin exactly that contract for `WinBackend`
//! specifically. That half of the claim is retracted; see
//! `docs/PENDING_QUADRAUI_ISSUES.md`'s "Correction (#1622 fix round 1)" for
//! the full write-up, including a second dell64 session's evidence that the
//! *entire* client area (not selectively the Command Center) was blank/
//! black under the same locked-session Direct2D suspension #1559 above
//! already discloses. The `WS_CAPTION`-not-cleared half of this correction
//! (native caption + native menu row, no CSD) is unaffected and independently
//! reproduced again this round.
//!
//! # #1582: no menu bar at startup — the same `backend_caps` gap as #1562,
//! # narrower fix (`native_menu`, not `window_chrome`)
//!
//! vimcode#1582 reports no menu bar at all on Win-GUI startup — the same
//! root cause the `#1562` section above already names (at the time of the
//! original investigation, `WinBackend::backend_caps()` set neither flag
//! `App::setup`'s branch checks), but satisfiable by the narrower of the
//! two paths: `native_menu` (a real `SetMenu`-backed `HMENU`, the shape
//! macOS's `install_menu_bar` already uses) needs no window-style change,
//! unlike `window_chrome` (gated on `#1562`'s custom-caption work landing
//! first). See `src/win/mod.rs`'s `#1582` doc section for the full
//! write-up and `docs/PENDING_QUADRAUI_ISSUES.md`'s entry (scoped to
//! `native_menu` only, so it does not duplicate `#1562`'s `window_chrome`
//! entry) — both quadraui-side, so nothing changes in this 1-line
//! re-export.
//!
//! **Update (#1618):** `WinBackend::install_menu_bar`/`native_menu: true`
//! landed upstream as quadraui#1200, confirming the analysis above. #1614
//! had found #1200 itself crashed `vimcode.exe` on every startup
//! (`RefCell already borrowed`, `JDonaghy/quadraui#1213`) — that fix
//! landed upstream as `ce1c763` (a `ModalPumpGuard` around the reentrant
//! `SetMenu` call) plus a regression test in `6e14d8a`, and the pin now
//! includes both (see the `Cargo.toml` pin comment). Real-hardware
//! verification on dell64 confirms `vimcode.exe` launches cleanly with a
//! real, clickable native menu bar present at startup.

pub use quadraui::win::WinBackend;
