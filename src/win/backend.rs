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
//! **Note (#1629):** the pin now also includes quadraui#1228, which
//! reverts the `native_menu` half of #1200 (see the neighbouring `#1562`/
//! `#1582` sections below) — unrelated to this cursor-glyph fix itself
//! (`CursorShape::Block` re-painting the glyph is untouched by #1228), but
//! it does change what a live vimcode.exe's chrome looks like around the
//! editor pane during the next real-hardware re-check: a drawn menu row
//! and title bar (`window_chrome`) rather than the native caption + native
//! menu row this section's evidence was captured against. A fresh dell64
//! session for this bump built and launched `vimcode.exe` cleanly (real
//! `HWND`, `Responding: True`, no crash log — see `src/win/mod.rs`'s
//! `#1562` "Real-hardware verification (#1629)" note for the full
//! write-up) but found the interactive session locked again
//! (`GetForegroundWindow()` returned `NULL`), the same blocker every prior
//! attempt hit. #1559's cursor-glyph pixel check remains **UNVERIFIED** —
//! not by lack of trying a fourth time, but by this host's interactive
//! session relocking on every session that has attempted this check
//! (#1558/#1561/#1622/#1629). A live, unlocked dell64 session is the only
//! way to close it.
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
//! pin then included it along with the `JDonaghy/quadraui#1213` reentrancy
//! fix that had blocked bumping past it (see the `Cargo.toml` pin comment
//! and `src/win/mod.rs`'s `#1618` update).
//!
//! **Correction (#1622):** the "Real-hardware verification on dell64
//! confirms the custom title bar and command centre now render" line above
//! was wrong — #1618's dell64 session only re-drove #1582's menu bar
//! check, not this one (see #1622's own body). Actually launching
//! `vimcode.exe` on dell64 showed a stock native caption (icon, title, real
//! min/max/close) plus a separate native/owner-drawn Win32 menu row below
//! it — not the single custom-drawn caption with an embedded command
//! centre this issue asks for, and no command-centre search box anywhere
//! in the band. Root cause: `WinBackend::backend_caps()` declared *both*
//! `native_menu: true` (#1200/#1582) and `window_chrome: true`
//! (#1199/#1562) — the same pair `MacBackend` has declared for a while —
//! and `App::setup`'s `if native_menu {...} else if window_chrome {...}`
//! (`src/app.rs`) always took the `native_menu` arm first, so
//! `window_chrome`'s arm never ran for Win-GUI. Filed as the (now-struck)
//! `BackendCaps::native_menu is overloaded` entry in
//! `docs/PENDING_QUADRAUI_ISSUES.md`; `src/win/mod.rs`'s `#1562` doc
//! section has the fuller write-up, including the "Reconciliation (#1622
//! fix round 1)" note that retracted this correction's Command Center
//! half (that rung's liveness was never actually gated on this branch —
//! see #939).
//!
//! **Resolved (#1629):** quadraui#1228 (`bc92d47`/`d292a4c`) reverts the
//! `native_menu` half of #1200 on Win-GUI instead of adding the capability
//! split the pending-issue draft asked for: `WinBackend::backend_caps()`
//! now declares only `window_chrome: true`. `App::setup`'s three-way
//! branch needed no vimcode-side change to pick this up — a Win backend
//! now takes the same `window_chrome` arm GTK does, and
//! `capture_window_and_apply_csd`'s `!native_menu` gate now lets
//! `set_decorated(false)` run, clearing `WS_CAPTION`. See
//! `src/win/mod.rs`'s `#1562` doc section for the full mechanism and
//! `win_driver_tests`' replacement tests. Real-hardware verification of
//! the drawn title bar and command centre on dell64 is this issue's own
//! acceptance item.
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
//! `SetMenu` call) plus a regression test in `6e14d8a`, and the pin then
//! included both. Real-hardware verification on dell64 confirmed
//! `vimcode.exe` launched cleanly with a real, clickable native menu bar
//! present at startup — but `native_menu: true` also silently starved
//! `#1562`'s `window_chrome` drawn-caption path (see that section above).
//!
//! **Resolved (#1629):** quadraui#1228 reverts the `native_menu` half of
//! #1200 on Win-GUI (see the `#1562` section above for the full
//! mechanism). #1582's "a menu bar is present and clickable" ask is now
//! satisfied by the *drawn* menu row instead — the same row GTK already
//! ships — reached through `App::setup`'s `window_chrome` arm with no
//! vimcode-side change needed. Real-hardware re-verification that the
//! drawn row opens File/Edit/View/... on dell64 is this issue's own
//! acceptance item.

pub use quadraui::win::WinBackend;
