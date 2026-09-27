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

pub use quadraui::win::WinBackend;
