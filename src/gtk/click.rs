//! GTK-only click-context construction. Everything else that used to live
//! in this file (`pixel_to_click_target`, `handle_mouse_click`,
//! `handle_mouse_double_click`, `handle_mouse_drag`,
//! `resolve_tab_right_click`, the tab-bar pixel-geometry helpers, ...) moved
//! to the backend-neutral `crate::click` (#862) — none of it named a
//! `gtk4`/`pango` type. Re-exported below so this module's own tests (and
//! the rest of `crate::gtk`) keep resolving the names unchanged.
use gtk4::pango;
use pangocairo::functions as pangocairo;

// Only this file's own `#[cfg(test)]` modules below reach these through
// `use super::*` (they exercise the moved functions against real headless
// Pango layouts), so a non-test build sees the re-export as unused.
#[allow(unused_imports)]
pub(crate) use crate::click::*;

// The rest of this module is tests only — they exercise the re-exported
// `crate::click` functions against real headless Pango layouts, which is why
// this GTK-only file (rather than the neutral `crate::click`) still hosts
// them. `use super::*` inside each `mod ... tests` pulls these in, mirroring
// what `use super::*` (of `crate::gtk`) used to provide before #862 moved the
// production code out.
#[cfg(test)]
use crate::core;
#[cfg(test)]
use crate::core::engine::EngineAction;
#[cfg(test)]
use crate::core::window::GroupId;
#[cfg(test)]
use crate::core::Engine;
#[cfg(test)]
use crate::gtk::backend;
#[cfg(test)]
use crate::render;
#[cfg(test)]
use crate::render::{self as render_mod, ScreenZone, Theme};
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::rc::Rc;

/// Build the Pango context the *click* backend uses to resolve editor
/// columns, matched to the editor's **painted** font.
///
/// vimcode keeps a separate `GtkBackend` for click hit-testing than the one
/// quadraui's ShellApp runner paints with (see `App::render_content`). At
/// click time `editor_col_at_x` runs `xy_to_index` against *this* context's
/// Pango layout, so its glyph advances must reproduce the ones the painted
/// glyphs actually used — otherwise column resolution scales by the wrong
/// cell width and drifts left, the drift growing with `x` (#560 iter-3
/// smoke failure).
///
/// The runner paints the editor with a hardcoded monospace font
/// (`quadraui::gtk::run` → `"Monospace 11"`), **ignoring** `settings.font_*`;
/// the resulting painted cell advance is what `Backend::char_width()` reports
/// and what `build_screen_layout` / `editor_text_layout` positioned glyphs
/// with. The earlier fix fonted this context from `settings.font_size` (14 by
/// default) while the paint ran at 11 — a ~1.27× scale error that produced
/// exactly the reported left-growing drift on plain text, bold, italic and
/// scrolled lines alike.
///
/// So we mirror the runner's family (`Monospace`) and tune only the point
/// size: measure a probe `'0'` advance and scale until it equals the painted
/// `char_width`. Because it is the same family at the reproduced size, *all*
/// glyph advances — including emoji/CJK fallback — line up with the paint.
pub(crate) fn build_editor_click_context(paint_char_width: f64) -> Option<pango::Context> {
    let surface = gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, 1, 1).ok()?;
    let cr = gtk4::cairo::Context::new(&surface).ok()?;
    let ctx = pangocairo::create_context(&cr);

    // Mirror the runner's editor font family; only the size is tuned so the
    // measured '0' advance reproduces the painted cell width.
    let family = "Monospace";
    let mut size = 11.0_f64;
    let probe = pango::Layout::new(&ctx);
    probe.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "{family} {size}"
    ))));
    probe.set_text("0");
    let w0 = probe.pixel_size().0 as f64;
    if w0 > 0.1 && paint_char_width > 0.1 {
        size = (size * paint_char_width / w0).clamp(1.0, 400.0);
    }

    ctx.set_font_description(Some(&pango::FontDescription::from_string(&format!(
        "{family} {size}"
    ))));
    Some(ctx)
}

#[cfg(test)]
mod emoji_click_column_tests {
    //! #560 regression: a manual smoke test on the shared-quadraui-inverse
    //! fix reported clicks landing one column to the right of the intended
    //! glyph on markdown lines containing emoji (✅ 🟡 ❌ ⏭️), with the
    //! drift compounding for every wide/multi-byte glyph preceding the
    //! click point on the line. Root-cause investigation (see the vimcode
    //! issue #560 durable-findings log) reproduced the *exact* symptom
    //! shape — perfect on plain monospace text, growing drift after each
    //! emoji — only when `GtkBackend::editor_col_at_x` falls back to
    //! `EditorLayout::col_at_x`'s uniform-monospace division (the TUI path,
    //! which assumes every glyph is exactly one `cell_width` wide). That
    //! fallback fires when no Pango layout is available; the real
    //! per-glyph `quadraui::gtk::editor_col_at_x` (Pango `xy_to_index`)
    //! path was verified byte-exact for this same string (base emoji,
    //! astral-plane emoji, and a variation-selector emoji) in isolation.
    //!
    //! This test pins the production pipeline end to end — real
    //! `md_inline_spans` bold-span byte offsets via `build_screen_layout`,
    //! then `render::editor_text_layout` + `quadraui::gtk::editor_col_at_x`
    //! — against a headless Pango layout, so a future regression that
    //! silently reintroduces the naive fallback (or corrupts the
    //! span byte-offset pipeline feeding Pango's attributes) fails a
    //! `cargo test`, not just a manual click in the running app.
    use super::*;
    use crate::core::WindowRect;
    use crate::render::build_screen_layout;
    use ::pangocairo::cairo::{Context, Format, ImageSurface};

    fn headless_pango_layout() -> pango::Layout {
        let surface = ImageSurface::create(Format::ARgb32, 900, 60).expect("create ImageSurface");
        let cr = Context::new(&surface).expect("Context::new");
        let ctx = pangocairo::create_context(&cr);
        ctx.set_font_description(Some(&pango::FontDescription::from_string("Monospace 12")));
        pango::Layout::new(&ctx)
    }

    #[test]
    fn click_resolves_exact_column_on_emoji_markdown_line() {
        // Concurrent Pango/Cairo text work from two test threads segfaults
        // inside FreeType — see `src/test_paint.rs`.
        let _paint = crate::test_paint::PaintGuard::acquire();
        let text = "Total: **58 commands**  \u{b7}  \u{2705} 36  \u{b7}  \u{1f7e1} 2  \u{b7}  \u{274c} 14  \u{b7}  \u{23ed}\u{fe0f} 6";
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, text);
        let buf_id = engine.active_buffer_id();
        engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
            Some(std::path::PathBuf::from("notes.md"));

        let char_width = 9.0;
        let line_height = 18.0;
        let theme = Theme::onedark();
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, 24.0);
        let layout = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            true,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );
        let rw = &layout.windows[0];
        assert_eq!(
            rw.lines[0].raw_text, text,
            "line should not wrap in an 800px window"
        );

        let (editor, editor_layout) = render::editor_text_layout(rw, char_width, line_height);
        let line = &editor.lines[0];

        // `editor_col_at_x` unconditionally `set_text`/`set_attributes`es
        // its layout argument from `line` before hit-testing, so a single
        // throwaway call (the same one every real click makes) leaves
        // `measure_layout` holding the exact attributed text `draw_editor`
        // paints — real bold-run glyph widths included — without vimcode
        // reimplementing quadraui's private `build_pango_attrs`. Reusing
        // this one layout for both measuring and resolving throughout
        // mirrors production, which caches and reuses a single
        // `last_editor_pango_layout` across an entire click.
        let measure_layout = headless_pango_layout();
        let _ = quadraui::gtk::editor_col_at_x(&measure_layout, line, &editor_layout, 0.0);

        let char_count = text.chars().count();
        let mut prev_pos: Option<(i32, i32)> = None;
        for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
            let pos = measure_layout.index_to_pos(byte_idx as i32);
            // A zero-width combining mark (e.g. the U+FE0F variation
            // selector on "⏭️") shares its base character's glyph cluster
            // — Pango reports the *identical* (x, width) rect for both
            // byte offsets, since there is no distinct on-screen pixel
            // region for the combining mark alone. A click can only ever
            // land on the cluster as a whole, so such chars have no
            // resolvable column of their own to assert against — skip
            // them rather than asserting an unreachable identity.
            if prev_pos == Some((pos.x(), pos.width())) {
                continue;
            }
            prev_pos = Some((pos.x(), pos.width()));

            let glyph_left_x =
                editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
            let glyph_width = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
            let click_x = glyph_left_x + glyph_width * 0.25;

            let resolved = quadraui::gtk::editor_col_at_x(
                &measure_layout,
                line,
                &editor_layout,
                click_x as f32,
            );
            assert_eq!(
                resolved, char_idx,
                "clicking char {char_idx} ({ch:?}) resolved to col {resolved}, not {char_idx} \
                 — the paint↔click column inverse has drifted for this glyph"
            );
        }
        assert_eq!(char_count, text.chars().count());
    }

    /// #560 iteration 2: the test above calls `quadraui::gtk::editor_col_at_x`
    /// *directly* with a hand-built, correctly-fonted layout — it verifies the
    /// per-glyph Pango inverse but **bypasses** `GtkBackend::editor_col_at_x`'s
    /// runtime branch-selection (`current_frame_refs()` → `last_editor_pango_layout`
    /// → `pango_ctx` → naive `EditorLayout::col_at_x`). A live mouse click goes
    /// through the *trait* method, outside any frame scope, so it depends on the
    /// backend having stashed a correctly-fonted layout during paint. This test
    /// drives exactly that path: paint the editor through the trait (as
    /// `draw_window` does), then resolve clicks through
    /// `GtkBackend::editor_col_at_x` (as `pixel_to_click_target` does) — so a
    /// regression that makes live clicks fall through to the naive uniform-cell
    /// division (perfect on plain text, +1 col per preceding wide glyph on emoji
    /// lines — the exact reported symptom) fails here.
    /// #560 iteration 2 robustness: build the `Engine`/`Editor`/`EditorLayout`
    /// for the emoji markdown line and the pixel-`x` click for every glyph,
    /// then resolve each through `GtkBackend::editor_col_at_x` under the exact
    /// backend state named by `set_pango_context`. `paint_first` selects which
    /// fallback branch the trait method takes:
    ///
    /// * `true`  → an editor paint runs through the trait first, so
    ///   `last_editor_pango_layout` is stashed (the steady-state live path
    ///   after frame 1).
    /// * `false` → NO paint, but `set_pango_context` has stored a
    ///   correctly-fonted editor context (what `draw::draw_editor` now does
    ///   every frame), so the trait method resolves via the `pango_ctx`
    ///   fallback instead of the naive `EditorLayout::col_at_x` division.
    ///
    /// Both must land every click on its own glyph. Emoji here render 1.7–2.3×
    /// the cell width (see the sibling paint test's provenance), so a naive
    /// uniform-cell division would drift +1 column per preceding wide glyph and
    /// fail — this is what pins that neither branch degrades to it.
    fn assert_emoji_columns_resolve(paint_first: bool) {
        // Concurrent Pango/Cairo text work from two test threads segfaults
        // inside FreeType — see `src/test_paint.rs`.
        let _paint = crate::test_paint::PaintGuard::acquire();
        use quadraui::{Backend as _, ScreenLayout as QScreenLayout, Surface};
        use std::cell::RefCell;
        use std::rc::Rc;

        let text = "Total: **58 commands**  \u{b7}  \u{2705} 36  \u{b7}  \u{1f7e1} 2  \u{b7}  \u{274c} 14  \u{b7}  \u{23ed}\u{fe0f} 6";
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, text);
        let buf_id = engine.active_buffer_id();
        engine.buffer_manager.get_mut(buf_id).unwrap().file_path =
            Some(std::path::PathBuf::from("notes.md"));

        let surface = ImageSurface::create(Format::ARgb32, 1000, 200).expect("ImageSurface");
        let cr = Context::new(&surface).expect("Context::new");
        let pango_ctx = pangocairo::create_context(&cr);
        let font_desc = pango::FontDescription::from_string("Monospace 12");
        pango_ctx.set_font_description(Some(&font_desc));
        let layout = pango::Layout::new(&pango_ctx);
        layout.set_font_description(Some(&font_desc));
        let metrics = pango_ctx.metrics(Some(&font_desc), None);
        let line_height = (metrics.ascent() + metrics.descent()) as f64 / pango::SCALE as f64;
        layout.set_text("0");
        let char_width = layout.pixel_size().0 as f64;

        let theme = Theme::onedark();
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );
        let rw = &screen.windows[0];
        assert_eq!(rw.lines[0].raw_text, text, "line should not wrap");

        let backend = Rc::new(RefCell::new(super::backend::GtkBackend::new()));
        // Mirror `App::render_content`: hand the click backend an editor-fonted
        // PangoCairo context (built from a throwaway surface, NOT the paint
        // layout) so the click-time fallback is per-glyph accurate.
        {
            let click_surface =
                ImageSurface::create(Format::ARgb32, 1, 1).expect("click ImageSurface");
            let click_cr = Context::new(&click_surface).expect("click Context");
            let click_ctx = pangocairo::create_context(&click_cr);
            click_ctx.set_font_description(Some(&font_desc));
            backend.borrow_mut().set_pango_context(click_ctx);
        }

        if paint_first {
            let editor = render::to_q_editor(rw);
            let rect = editor.rect;
            let mut b = backend.borrow_mut();
            b.set_current_theme(render::to_quadraui_theme(&theme));
            b.set_current_line_height(line_height);
            b.set_current_char_width(char_width);
            b.enter_frame_scope(&cr, &layout, |b| {
                let mut frame = QScreenLayout::new();
                frame.push(Surface::Editor {
                    rect,
                    editor: &editor,
                });
                frame.draw(b);
            });
        }

        let (editor, editor_layout) = render::editor_text_layout(rw, char_width, line_height);
        let measure = pango::Layout::new(&pango_ctx);
        measure.set_font_description(Some(&font_desc));
        measure.set_text(text);

        let mut prev_pos: Option<(i32, i32)> = None;
        for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
            let pos = measure.index_to_pos(byte_idx as i32);
            if prev_pos == Some((pos.x(), pos.width())) {
                continue;
            }
            prev_pos = Some((pos.x(), pos.width()));

            let glyph_left_x =
                editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
            let glyph_width = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
            let click_x = glyph_left_x + glyph_width * 0.25;

            let resolved =
                backend
                    .borrow()
                    .editor_col_at_x(&editor_layout, &editor, 0, click_x as f32);
            assert_eq!(
                resolved, char_idx,
                "paint_first={paint_first}: clicking char {char_idx} ({ch:?}) resolved to \
                 col {resolved} — GtkBackend::editor_col_at_x degraded to the naive \
                 uniform-cell division instead of a per-glyph Pango layout"
            );
        }
    }

    /// Live steady-state: `last_editor_pango_layout` stashed by a real paint.
    #[test]
    fn live_trait_editor_col_at_x_resolves_exact_column_after_paint() {
        assert_emoji_columns_resolve(true);
    }

    /// #560 robustness: even with NO stashed layout, the editor-fonted
    /// `pango_ctx` (now set every frame by `draw::draw_editor`) keeps the
    /// resolution on the per-glyph Pango path instead of the naive division —
    /// so a build that hasn't painted yet, or a quadraui lacking the stash,
    /// still resolves emoji clicks exactly.
    #[test]
    fn editor_col_at_x_falls_back_to_editor_font_context_not_naive_division() {
        assert_emoji_columns_resolve(false);
    }

    /// #560 iteration 3 (the smoke failure this fix targets): plain / bold /
    /// italic / scrolled clicks landed LEFT of the target, the drift growing
    /// with `x`. Root cause: the quadraui runner paints the editor with a
    /// hardcoded "Monospace 11" (ignoring `settings.font_*`), but the previous
    /// fix fonted the click backend's Pango context from `settings.font_size`
    /// (14) — so `editor_col_at_x`'s `xy_to_index` measured against glyphs
    /// ~1.27× too wide and scaled every column down, drifting left more the
    /// further right the click. The earlier emoji tests use ONE self-consistent
    /// font for both paint and resolve, so they never caught this size split.
    ///
    /// This test reproduces the split: paint at one size, resolve through the
    /// context `App::render_content` actually builds (`build_editor_click_context`,
    /// matched to the *painted* `char_width`), and assert every column on a long
    /// plain ASCII line resolves exactly — including the far right where a
    /// size-mismatched context drifts. The `bad_drift_seen` assertion pins that
    /// a mismatched context genuinely fails, so this test can't silently pass by
    /// resolving on a too-short line.
    #[test]
    fn click_context_matches_painted_font_not_settings_size() {
        // Concurrent Pango/Cairo text work from two test threads segfaults
        // inside FreeType — see `src/test_paint.rs`.
        let _paint = crate::test_paint::PaintGuard::acquire();
        // ── The runner's painted editor font (see quadraui `gtk::run`). ──
        let paint_surface =
            ImageSurface::create(Format::ARgb32, 2000, 60).expect("paint ImageSurface");
        let pcr = Context::new(&paint_surface).expect("paint Context");
        let pctx = pangocairo::create_context(&pcr);
        let paint_font = pango::FontDescription::from_string("Monospace 11");
        pctx.set_font_description(Some(&paint_font));
        let probe = pango::Layout::new(&pctx);
        probe.set_font_description(Some(&paint_font));
        probe.set_text("0");
        let paint_cw = probe.pixel_size().0 as f64;
        let metrics = pctx.metrics(Some(&paint_font), None);
        let line_height = (metrics.ascent() + metrics.descent()) as f64 / pango::SCALE as f64;

        // ── The click context production actually builds, matched to the
        //    painted char width — NOT to any `settings.font_size`. ──
        let click_ctx = super::build_editor_click_context(paint_cw).expect("click ctx");
        let click_probe = pango::Layout::new(&click_ctx);
        click_probe.set_text("0");
        let click_cw = click_probe.pixel_size().0 as f64;
        assert!(
            (click_cw - paint_cw).abs() <= 1.0,
            "build_editor_click_context('0' adv {click_cw}) must reproduce the painted \
             char width {paint_cw}, else column resolution scales by the wrong cell width"
        );

        // ── End-to-end on a long plain ASCII line. ──
        let text = "The quick brown fox jumps over the lazy dog end AAAA BBBB CCCC DDDD EEEE";
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, text);

        let theme = Theme::onedark();
        let bounds = WindowRect::new(0.0, 0.0, 2000.0, 400.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            paint_cw,
            false,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );
        let rw = &screen.windows[0];
        assert_eq!(rw.lines[0].raw_text, text, "line should not wrap");

        let (editor, editor_layout) = render::editor_text_layout(rw, paint_cw, line_height);
        let line = &editor.lines[0];

        // Glyph geometry from the PAINT font (what draw_editor rendered with).
        let measure = pango::Layout::new(&pctx);
        measure.set_font_description(Some(&paint_font));
        measure.set_text(text);

        // The good resolver: the production click context.
        let good_layout = pango::Layout::new(&click_ctx);

        // The pre-fix bug: font the resolver from `settings.font_size` (14).
        let bad_surface = ImageSurface::create(Format::ARgb32, 1, 1).expect("bad ImageSurface");
        let bad_cr = Context::new(&bad_surface).expect("bad Context");
        let bad_ctx = pangocairo::create_context(&bad_cr);
        bad_ctx.set_font_description(Some(&pango::FontDescription::from_string("Monospace 14")));
        let bad_layout = pango::Layout::new(&bad_ctx);

        let mut bad_drift_seen = false;
        for (char_idx, (byte_idx, ch)) in text.char_indices().enumerate() {
            let pos = measure.index_to_pos(byte_idx as i32);
            let glyph_left =
                editor_layout.text_bounds.x as f64 + pos.x() as f64 / pango::SCALE as f64;
            let gw = (pos.width() as f64 / pango::SCALE as f64).max(2.0);
            let click_x = (glyph_left + gw * 0.25) as f32;

            let good = quadraui::gtk::editor_col_at_x(&good_layout, line, &editor_layout, click_x);
            assert_eq!(
                good, char_idx,
                "clicking char {char_idx} ({ch:?}) resolved to col {good} — the \
                 production click context has drifted from the painted font"
            );

            let bad = quadraui::gtk::editor_col_at_x(&bad_layout, line, &editor_layout, click_x);
            if bad != char_idx {
                bad_drift_seen = true;
            }
        }
        assert!(
            bad_drift_seen,
            "a size-mismatched click context (the pre-fix bug) must drift on this line, \
             else the test can't prove the width-match is what fixes it"
        );
    }
}

#[cfg(test)]
mod cross_split_drag_focus_tests {
    //! #568 regression: dragging a text selection in one editor group (a
    //! GTK split pane created via the tab bar's split button /
    //! `Engine::open_editor_group`, i.e. VS Code-style side-by-side panes)
    //! must not steal focus to a neighboring group's window merely because
    //! the mouse passes over it while the button is held.
    //!
    //! `Engine::mouse_drag`'s origin-window lock (`mouse_drag_origin_window`)
    //! already keeps the selection *data* pinned to the originating window
    //! — see the core-level `test_mouse_drag_locked_to_origin_window`. But
    //! GTK's `pixel_to_click_target` used to call
    //! `engine.activate_group_for_window(window_id)` unconditionally, as a
    //! side effect of resolving ANY pixel position — including drag
    //! continuation. That flipped `engine.active_group` (and therefore
    //! `active_window_id()`) to the neighboring pane just from hovering over
    //! it mid-drag, which made `render::build_selection`'s `is_active` gate
    //! light up the wrong pane's selection overlay even though the
    //! underlying selection state never actually changed. This pins that a
    //! drag-continuation hit-test (`mutate_focus: false`) leaves
    //! `active_group`/`active_window_id()` untouched, while a genuine click
    //! (`mutate_focus: true`) still focuses the pane it lands in.
    use super::*;
    use crate::render::build_screen_layout;

    fn empty_tab_pixel_hits() -> TabPixelHitMap {
        HashMap::new()
    }

    #[test]
    fn drag_continuation_does_not_steal_focus_to_neighboring_group() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid_a = engine.active_window_id();
        let group_a = engine.active_group;

        // Open a second editor group side-by-side — GTK's split-pane
        // feature (bound to the tab bar's split button). `open_editor_group`
        // makes the new group/window active.
        engine.open_editor_group(crate::core::window::SplitDirection::Vertical);
        let group_b = engine.active_group;
        let wid_b = engine.active_window_id();
        assert_ne!(group_a, group_b);
        assert_ne!(wid_a, wid_b);

        // Simulate the user clicking back into the left pane to start the drag.
        engine.mouse_click(wid_a, 0, 1);
        assert_eq!(engine.active_group, group_a);
        assert_eq!(engine.active_window_id(), wid_a);
        engine.mouse_drag(wid_a, 0, 4);
        assert!(engine.mouse_drag_active);
        assert_eq!(engine.mouse_drag_origin_window, Some(wid_a));

        // Lay out both panes side by side and locate each window's rect.
        let theme = Theme::onedark();
        let bounds = core::WindowRect::new(0.0, 0.0, 1600.0, 400.0);
        let line_height: f64 = 18.0;
        let char_width: f64 = 9.0;
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(
            &engine,
            &theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );
        let rw_b = screen
            .windows
            .iter()
            .find(|w| w.window_id == wid_b)
            .expect("window B should be laid out");
        // A pixel comfortably inside window B's text area (below its tab bar).
        let x_in_b = rw_b.rect.x + char_width * 2.0;
        let y_in_b = rw_b.rect.y + line_height * 2.0;

        let backend = Rc::new(RefCell::new(super::super::backend::GtkBackend::new()));
        let tab_pixel_hits = empty_tab_pixel_hits();

        // Drag continuation: the mouse is over window B's pixels, but this
        // must resolve as a pure hit-test — no focus/group side effects.
        let target = pixel_to_click_target(
            &mut engine,
            &*backend.borrow(),
            x_in_b,
            y_in_b,
            line_height,
            char_width,
            &screen,
            &tab_pixel_hits,
            None, // no cached FrameHitMap in this test — exercises the
            // `screen_zone_hit_test` fallback path (#449)
            &HashMap::new(),
            false, // mutate_focus: drag continuation
        );
        assert_eq!(
            engine.active_group, group_a,
            "a held drag sweeping over the neighboring group must not steal active_group"
        );
        assert_eq!(
            engine.active_window_id(),
            wid_a,
            "a held drag sweeping over the neighboring group must not steal active_window_id \
             (render::build_selection's is_active gate keys off this)"
        );
        match target {
            ClickTarget::BufferPos(wid, _, _) => {
                assert_eq!(
                    wid, wid_b,
                    "the hit-test should still resolve the real window under the cursor"
                )
            }
            other => panic!("expected a BufferPos hit in window B's text area, got {other:?}"),
        }
        // The engine-level origin lock (already covered by
        // `test_mouse_drag_locked_to_origin_window`) rejects this mismatched
        // window_id, so the selection itself stays anchored to window A.
        engine.mouse_drag(wid_b, 0, 8);
        assert_eq!(engine.mouse_drag_origin_window, Some(wid_a));

        // Contrast: a genuine click landing in window B (mutate_focus: true)
        // — as a real MouseClick/DoubleClick event would — SHOULD focus it.
        // This proves the flag actually gates behavior rather than being a
        // no-op, and that real clicks keep working as before.
        let click_target = pixel_to_click_target(
            &mut engine,
            &*backend.borrow(),
            x_in_b,
            y_in_b,
            line_height,
            char_width,
            &screen,
            &tab_pixel_hits,
            None,
            &HashMap::new(),
            true, // mutate_focus: genuine click
        );
        assert_eq!(
            engine.active_group, group_b,
            "a genuine click must still focus the pane it lands in"
        );
        assert!(matches!(click_target, ClickTarget::BufferPos(wid, _, _) if wid == wid_b));
    }
}

#[cfg(test)]
mod frame_hit_map_tests {
    //! #449 regression: `frame_zone_to_screen_zone` and the `frame_hit_map`
    //! branch of `pixel_to_click_target` are the actual mechanism this issue
    //! introduced — mapping `quadraui::FrameZone::TabBar { idx }` /
    //! `FrameZone::Editor { idx }` back to `render::ScreenZone`. These tests
    //! build a *real* `quadraui::FrameHitMap` via `ScreenLayout::hit_map()`
    //! (quadraui#425, landed as `c316f15`) from the same `Surface::Editor` /
    //! `Surface::TabBar` construction `App::render_content` uses
    //! (`src/gtk/mod.rs` ~7712-7830), instead of exercising only the
    //! pre-existing `screen_zone_hit_test` fallback (as the two tests in
    //! `cross_split_drag_focus_tests` above do by passing `None, &[]`).
    use super::*;
    use crate::core::WindowRect;
    use crate::render::build_screen_layout;
    use quadraui::{ScreenLayout as QSL, Surface};

    /// Lay out a single window / single (unsplit) tab bar and build the
    /// `FrameHitMap` + `tab_bar_zones` table the way `render_content` does
    /// (`src/gtk/mod.rs` ~7712-7839), so these tests exercise the production
    /// construction, not a hand-rolled stand-in — crucially including the
    /// same "editors pushed first, tab bars after" ordering, since
    /// `FrameZone::TabBar { idx }` carries the *global* surface index across
    /// that whole `ScreenLayout`, not a per-tab-bar position. `tab_bar_zones`
    /// must therefore be keyed by that same global index, not `0..`.
    fn build_hit_map(
        engine: &Engine,
        theme: &Theme,
        line_height: f64,
        char_width: f64,
    ) -> (
        render::ScreenLayout,
        quadraui::FrameHitMap,
        HashMap<usize, (GroupId, quadraui::Rect)>,
    ) {
        let bounds = WindowRect::new(0.0, 0.0, 800.0, 600.0);
        let (rects, _) = engine.calculate_group_window_rects(bounds, (line_height * 1.6).ceil());
        let screen = build_screen_layout(
            engine,
            theme,
            &rects,
            line_height,
            char_width,
            false,
            8.0,
            crate::render::gtk_minimap_sizing(),
        );

        let window_editors: Vec<quadraui::Editor> =
            screen.windows.iter().map(render_mod::to_q_editor).collect();
        let mut hit_frame = QSL::new();
        for editor in &window_editors {
            hit_frame.push(Surface::Editor {
                rect: editor.rect,
                editor,
            });
        }

        let tab_row_h = render_mod::tab_row_height_px(line_height);
        let tab_bar_h = render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
        let mut tab_bar_zones: HashMap<usize, (GroupId, quadraui::Rect)> = HashMap::new();
        for (next_surface_idx, target) in (window_editors.len()..).zip(
            render_mod::tab_bar_draw_targets(engine, &screen, tab_row_h, tab_bar_h),
        ) {
            hit_frame.push(Surface::TabBar {
                rect: target.rect,
                bar: target.bar,
                hovered_close: None,
            });
            tab_bar_zones.insert(next_surface_idx, (target.group_id, target.rect));
        }

        let hit_map = hit_frame.hit_map();
        (screen, hit_map, tab_bar_zones)
    }

    #[test]
    fn frame_zone_to_screen_zone_resolves_editor_point() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid = engine.active_window_id();
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hit_map, tab_bar_zones) =
            build_hit_map(&engine, &theme, line_height, char_width);
        let rw = screen
            .windows
            .first()
            .expect("single window should be laid out");
        let x = rw.rect.x + char_width * 2.0;
        let y = rw.rect.y + line_height * 2.0;

        let zone = frame_zone_to_screen_zone(&hit_map, &tab_bar_zones, &screen, x, y);
        match zone {
            ScreenZone::Window {
                window_id,
                window_idx,
                ..
            } => {
                assert_eq!(window_id, wid);
                assert_eq!(window_idx, 0);
            }
            other => panic!("expected ScreenZone::Window from the FrameHitMap path, got {other:?}"),
        }
    }

    #[test]
    fn frame_zone_to_screen_zone_resolves_tab_bar_point() {
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let group_id = engine.active_group;
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hit_map, tab_bar_zones) =
            build_hit_map(&engine, &theme, line_height, char_width);
        assert!(
            !tab_bar_zones.is_empty(),
            "single-group mode should still push one tab bar surface"
        );
        let (zone_group, rect) = *tab_bar_zones
            .values()
            .next()
            .expect("just asserted tab_bar_zones is non-empty");
        assert_eq!(zone_group, group_id);
        let x = rect.x as f64 + 2.0;
        let y = rect.y as f64 + 2.0;

        let zone = frame_zone_to_screen_zone(&hit_map, &tab_bar_zones, &screen, x, y);
        match zone {
            ScreenZone::TabBar {
                group_id: resolved, ..
            } => assert_eq!(resolved, group_id),
            other => {
                panic!("expected ScreenZone::TabBar from the FrameHitMap path, got {other:?}")
            }
        }
    }

    #[test]
    fn pixel_to_click_target_consults_the_cached_frame_hit_map_for_editor_clicks() {
        // Proves `pixel_to_click_target` actually takes the `frame_hit_map`
        // branch (not silently falling through to `screen_zone_hit_test`)
        // when a real `Some(&FrameHitMap)` is supplied — the production
        // `Some` path exercised by `render_content`'s cached hit map, as
        // opposed to the `None` fallback path already covered by
        // `cross_split_drag_focus_tests`.
        let mut engine = Engine::new();
        engine.buffer_mut().insert(0, "hello world");
        let wid = engine.active_window_id();
        let theme = Theme::onedark();
        let line_height = 18.0;
        let char_width = 9.0;

        let (screen, hit_map, tab_bar_zones) =
            build_hit_map(&engine, &theme, line_height, char_width);
        let rw = screen
            .windows
            .first()
            .expect("single window should be laid out");
        let x = rw.rect.x + char_width * 2.0;
        let y = rw.rect.y + line_height * 2.0;

        let backend = Rc::new(RefCell::new(super::super::backend::GtkBackend::new()));
        let empty_pixel_hits: TabPixelHitMap = HashMap::new();

        let target = pixel_to_click_target(
            &mut engine,
            &*backend.borrow(),
            x,
            y,
            line_height,
            char_width,
            &screen,
            &empty_pixel_hits,
            Some(&hit_map),
            &tab_bar_zones,
            true,
        );
        match target {
            ClickTarget::BufferPos(id, _, _) => assert_eq!(id, wid),
            other => panic!(
                "expected a BufferPos hit resolved via the cached FrameHitMap, got {other:?}"
            ),
        }
    }
}

#[cfg(test)]
mod single_group_tab_click_dispatch_tests {
    //! #553 regression, pinned at the GTK **click-dispatch entry point** the
    //! issue's root-cause hint names (`pixel_to_click_target`, this file):
    //! with ONE tab group, clicking a non-active tab did not activate it and
    //! clicking a tab's × did not close it, while two or more groups worked.
    //!
    //! The defect lived in `screen_zone_hit_test`'s single-group arm, which
    //! hardcoded the tab row's top at the coordinate-system origin (`y >= 0.0`)
    //! instead of deriving it from the window rects the way the split arm did.
    //! Once #552 gave GTK a persistent menu/title-bar band the content origin
    //! moved down, so the single-group band pointed at chrome pixels no tab was
    //! ever drawn on — and every single-group tab click resolved to
    //! `ScreenZone::None` → `ClickTarget::None`. `render::tab_bar_hit_bands`
    //! (this PR) is what now forces both shapes through one derivation.
    //!
    //! Two things make these tests discriminate where the black-box
    //! `gtk::testing` pair does not (see the note in that module and in the PR
    //! description):
    //!
    //! 1. `frame_hit_map: None` — forcing the `screen_zone_hit_test` fallback
    //!    branch. GTK's production routing prefers the cached
    //!    `quadraui::FrameHitMap` (#449) and only falls back here on a hit-map
    //!    miss / before the first paint, so a driver-level click never reaches
    //!    the code under test. Same technique, same rationale as
    //!    `cross_split_drag_focus_tests::drag_continuation_does_not_steal_focus_to_neighboring_group`
    //!    above.
    //! 2. A **synthetic 100px content offset**, matching
    //!    `render::tests::test_tab_bar_hit_bands_single_and_split_share_one_derivation`.
    //!    The headless harness's default title-bar chrome only shifts the
    //!    content origin ~23px, which is small enough that the painted click y
    //!    falls inside *both* the correct band and the buggy hardcoded one —
    //!    the offset has to exceed the bar height to separate them.
    //!
    //! The activate and close cases are separate `#[test]`s deliberately: with
    //! the pre-`8fbbf85` bug reinstated in `render::tab_bar_hit_bands`'s
    //! single-group arm (`y: 0.0` instead of `y: min_y - tab_bar_height`) BOTH
    //! go red independently with `ClickTarget::None`, which a single test with
    //! two sequential assertions could not show (it would panic on the first
    //! and never reach the second). That FAIL/PASS pair is reproduced in the PR
    //! description.
    use super::*;
    use crate::render::build_screen_layout;

    /// Chrome-shifted editor content origin — the #552 menu/title-bar band, at
    /// an offset large enough to separate the correct band from the buggy one.
    const CONTENT_X: f64 = 50.0;
    const CONTENT_Y: f64 = 100.0;
    const CONTENT_W: f64 = 800.0;
    const CONTENT_H: f64 = 600.0;
    /// Synthetic per-tab pixel width, bar-relative (see [`synthetic_pixel_hits`]).
    const TAB_W: f64 = 120.0;

    /// The `TabBarPixelHits` the rasteriser would have cached for a single
    /// group with `tabs` tabs: contiguous `TAB_W`-wide slots from the bar's left
    /// edge, each with a 15px close (`×`) zone inset near its right edge.
    ///
    /// Bar-relative, exactly like `tab_hits_to_pixel_hits`'s output — which is
    /// what `pixel_to_click_target` matches `ScreenZone::TabBar { local_x }`
    /// against. Synthetic rather than rasterised so the test states its own
    /// geometry instead of depending on Pango font metrics.
    fn synthetic_pixel_hits(tabs: usize) -> TabBarPixelHits {
        TabBarPixelHits {
            slots: (0..tabs)
                .map(|i| (i as f64 * TAB_W, (i + 1) as f64 * TAB_W))
                .collect(),
            close: (0..tabs)
                .map(|i| Some((i as f64 * TAB_W + 100.0, i as f64 * TAB_W + 115.0)))
                .collect(),
            segments: Vec::new(),
        }
    }

    /// Bar-relative x of a point inside tab `idx`'s body, clear of its × zone.
    fn tab_body_local_x(idx: usize) -> f64 {
        idx as f64 * TAB_W + 20.0
    }

    /// Bar-relative x of a point inside tab `idx`'s × zone.
    fn tab_close_local_x(idx: usize) -> f64 {
        idx as f64 * TAB_W + 107.0
    }

    /// Three tabs in the default SINGLE editor group — the exact shape #553
    /// reports as dead — laid out at the chrome-shifted content origin, plus
    /// the cached rasteriser geometry and the empty legacy maps
    /// `pixel_to_click_target` still takes.
    ///
    /// No buffer edits anywhere, so a close click isn't diverted into the
    /// dirty-buffer confirm dialog.
    struct Fixture {
        engine: Engine,
        group: GroupId,
        screen: render::ScreenLayout,
        tab_pixel_hits: TabPixelHitMap,
        backend: Rc<RefCell<super::super::backend::GtkBackend>>,
        line_height: f64,
        char_width: f64,
    }

    impl Fixture {
        fn new() -> Self {
            let mut engine = Engine::new();
            engine.new_tab(None);
            engine.new_tab(None);
            let group = engine.active_group;
            assert_eq!(engine.editor_groups[&group].tabs.len(), 3);
            assert_eq!(
                engine.editor_groups[&group].active_tab, 2,
                "`new_tab` activates the tab it creates"
            );

            let theme = Theme::onedark();
            let line_height: f64 = 20.0;
            let char_width: f64 = 8.0;
            let tab_bar_height =
                render_mod::tab_bar_height_px(line_height, engine.settings.breadcrumbs);
            let content = core::WindowRect::new(CONTENT_X, CONTENT_Y, CONTENT_W, CONTENT_H);
            let (rects, _) = engine.calculate_group_window_rects(content, tab_bar_height);
            let screen = build_screen_layout(
                &engine,
                &theme,
                &rects,
                line_height,
                char_width,
                false,
                8.0,
                crate::render::gtk_minimap_sizing(),
            );
            assert!(
                screen.editor_group_split.is_none(),
                "these tests must exercise the single-group arm; a split layout would \
                 take the branch that never regressed"
            );

            let mut tab_pixel_hits: TabPixelHitMap = HashMap::new();
            tab_pixel_hits.insert(group.0, synthetic_pixel_hits(3));

            Self {
                engine,
                group,
                screen,
                tab_pixel_hits,
                backend: Rc::new(RefCell::new(super::super::backend::GtkBackend::new())),
                line_height,
                char_width,
            }
        }

        /// The tab row sits immediately ABOVE the window content, i.e. in
        /// `[CONTENT_Y, CONTENT_Y + tab_bar_height)`. The pre-fix code looked
        /// for it in `[0, tab_bar_height)`, which at this offset holds no tab
        /// pixels at all.
        const CLICK_Y: f64 = CONTENT_Y + 2.0;

        /// Resolve a tab-bar click through the production GTK dispatch entry
        /// point, with `frame_hit_map: None` to force the `screen_zone_hit_test`
        /// fallback branch #553 lives in (see this module's doc comment).
        fn click(&mut self, local_x: f64) -> ClickTarget {
            pixel_to_click_target(
                &mut self.engine,
                &*self.backend.borrow(),
                CONTENT_X + local_x,
                Self::CLICK_Y,
                self.line_height,
                self.char_width,
                &self.screen,
                &self.tab_pixel_hits,
                None,
                &HashMap::new(),
                true, // a genuine click
            )
        }

        /// The same pixel driven through the real click handler —
        /// `handle_mouse_click` is the production caller that turns
        /// `ClickTarget::CloseTab` into `Engine::close_tab`.
        fn full_click(&mut self, local_x: f64) -> (Option<bool>, Option<EngineAction>) {
            handle_mouse_click(
                &mut self.engine,
                &*self.backend.borrow(),
                CONTENT_X + local_x,
                Self::CLICK_Y,
                false, // alt
                self.line_height,
                self.char_width,
                &self.screen,
                &self.tab_pixel_hits,
                None,
                &HashMap::new(),
            )
        }
    }

    /// #553, half one: clicking a non-active tab in a single-group layout must
    /// resolve as a tab-bar hit and activate that tab.
    #[test]
    fn single_group_tab_click_activates_that_tab_via_click_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;

        let target = f.click(tab_body_local_x(0));
        assert!(
            matches!(target, ClickTarget::TabBar),
            "a single-group click on tab 0's body must resolve as a tab-bar hit, got {target:?} \
             (pre-fix this was ClickTarget::None — the click missed the bar entirely)"
        );
        assert_eq!(
            f.engine.editor_groups[&group].active_tab, 0,
            "clicking tab 0 in a single-group layout must activate it (#553)"
        );
    }

    /// #553, half two: clicking a tab's × in a single-group layout must resolve
    /// to `CloseTab` for that tab, and actually close it through the production
    /// click handler.
    #[test]
    fn single_group_tab_close_click_targets_and_closes_that_tab_via_click_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;

        let target = f.click(tab_close_local_x(1));
        assert_eq!(
            target,
            ClickTarget::CloseTab(group, 1),
            "a single-group click on tab 1's × must resolve to CloseTab for tab 1 \
             (pre-fix: ClickTarget::None, so nothing ever closed)"
        );

        let before = f.engine.editor_groups[&group].tabs.len();
        let (dirty_confirm, _) = f.full_click(tab_close_local_x(1));
        assert_eq!(
            dirty_confirm, None,
            "fixture buffers are unmodified, so no dirty-buffer confirm should intercept the close"
        );
        assert_eq!(
            f.engine.editor_groups[&group].tabs.len(),
            before - 1,
            "clicking a tab's × in a single-group layout must close it (#553)"
        );
    }

    /// #814: `dispatch_tab_bar_target` used to re-implement every tab-bar
    /// button arm by hand (`engine.open_editor_group(dir)` called directly
    /// from GTK's own `ClickTarget::SplitButton` match arm) instead of
    /// routing through `Engine::handle_tab_bar_click` — the single dispatch
    /// the TUI already used. This pins the split-right button through the
    /// full production `handle_mouse_click` entry point against the shared
    /// engine call, so a future arm added to `handle_tab_bar_click` reaches
    /// GTK without anyone having to remember to hand-port it here too.
    #[test]
    fn tab_bar_split_right_button_opens_new_group_via_shared_engine_dispatch() {
        let mut f = Fixture::new();
        let group = f.group;
        let groups_before = f.engine.editor_groups.len();

        // Place a synthetic SplitRight button segment just past the three
        // tabs, mirroring the disjoint right-side button zones the
        // rasteriser lays out in production (#515).
        let seg_start = TAB_W * 3.0;
        let seg_end = seg_start + 24.0;
        f.tab_pixel_hits.get_mut(&group.0).unwrap().segments.push((
            seg_start,
            seg_end,
            crate::core::engine::TabBarClickTarget::SplitRight,
        ));

        let (click_result, engine_action) = f.full_click(seg_start + 5.0);
        assert_eq!(
            click_result, None,
            "a split-button click is not a buffer click"
        );
        assert!(engine_action.is_none());
        assert_eq!(
            f.engine.editor_groups.len(),
            groups_before + 1,
            "clicking the tab bar's split-right button must open a new editor group, \
             routed through Engine::handle_tab_bar_click (#814)"
        );
    }
}
