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
//! Rule. Leave #1559 open until the quadraui issue is filed and lands.
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
//! for this file to change. The title-bar/command-centre item is real:
//! `WinBackend::backend_caps()` declares neither `native_menu` nor
//! `window_chrome`, and `win::run` creates a plain `WS_OVERLAPPEDWINDOW`
//! with no custom-caption `WM_NCCALCSIZE`/`WM_NCHITTEST` handling. See
//! `src/win/mod.rs`'s `#1562` doc section for the full write-up and
//! `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry for the drafted ask —
//! both are quadraui-side (`WinBackend`/`win::run`), so nothing changes in
//! this 1-line re-export.
//!
//! # #1582: no menu bar at startup — the same `backend_caps` gap as #1562,
//! # narrower fix (`native_menu`, not `window_chrome`)
//!
//! vimcode#1582 reports no menu bar at all on Win-GUI startup — the same
//! root cause the `#1562` section above already names
//! (`WinBackend::backend_caps()` sets neither flag `App::setup`'s branch
//! checks), but satisfiable by the narrower of the two paths: `native_menu`
//! (a real `SetMenu`-backed `HMENU`, the shape macOS's `install_menu_bar`
//! already uses) needs no window-style change, unlike `window_chrome`
//! (gated on `#1562`'s custom-caption work landing first). See
//! `src/win/mod.rs`'s `#1582` doc section for the full write-up and
//! `docs/PENDING_QUADRAUI_ISSUES.md`'s new entry (scoped to `native_menu`
//! only, so it does not duplicate `#1562`'s `window_chrome` entry) — both
//! quadraui-side, so nothing changes in this 1-line re-export.

pub use quadraui::win::WinBackend;
