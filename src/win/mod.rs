//! Native Win-GUI (Direct2D + DirectWrite) backend — a **wrapper**, not a
//! backend.
//!
//! quadraui already ships the whole Win-GUI backend: `WinBackend` implements
//! every `Backend` trait method `AppShell` renders through, and
//! [`quadraui::win::shell_runner::run_with_shell`] composes it with the
//! shared `ShellAdapter` exactly the way `quadraui::gtk::shell_runner` /
//! `quadraui::macos::shell_runner` / `quadraui::tui::shell_runner` do.
//! So there is nothing to rasterise here and nothing to decide here: this
//! file is the Win32 sibling of `src/gtk/mod.rs::run` / `src/macos/mod.rs::run`,
//! and it is deliberately the only thing in `src/win/` besides the backend
//! re-export (`src/win/backend.rs`).
//!
//! **It contains no layout, hit-test, paint or dispatch decision** — every
//! GUI entry point runs a single `impl quadraui::ShellApp for crate::app::App`.
//! If you find yourself about to add a decision here, `CLAUDE.md`'s
//! Platform-Neutrality Rule says stop: the gap belongs in quadraui or in
//! `crate::app`, not in a backend directory.
//!
//! # Why `feature = "win"` alone, not target-gated like `macos`
//!
//! `quadraui::win` is gated on `feature = "win"` alone, with every real
//! WinAPI call individually `cfg(target_os = "windows")`-gated and falling
//! back to a `todo!()` stub elsewhere — specifically so `cargo check
//! --features win` type-checks `WinBackend` on an ordinary Linux CI runner.
//! `src/win/backend.rs` and this file inherit that same posture: un-target-gated,
//! so a plain Linux `cargo build --no-default-features --features win`
//! compiles this module (proving the vimcode-side wrapper is well-typed) even
//! though *running* it productively needs an actual Windows target — [`run`]
//! below only becomes real once `quadraui::win::run::run_with` is invoked on
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
//! needed. Producing (and running) a real PE32+ binary needs `cargo xwin
//! build --target x86_64-pc-windows-msvc --no-default-features --features win`
//! plus the `clang-cl`/`lld-link`/xwin SDK toolchain — see `Cargo.toml`'s `win`
//! feature comment for why `mlua`'s vendored Lua C build is the thing that
//! actually requires the cross C toolchain, not anything in this file.
//! ```text
//! let mut rect = RECT::default();
//! GetClientRect(hwnd, &mut rect)?;
//! let width = (rect.right - rect.left).max(1) as u32;
//! let height = (rect.bottom - rect.top).max(1) as u32;
//! // ... single D2D_SIZE_U { width, height } fed straight to
//! // CreateHwndRenderTarget; resize_surface's WM_SIZE-driven `Resize`
//! // call is the same single width/height pair, no per-edge split.
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

pub(crate) mod backend;

use crate::app::App;

/// Entry point for the native Win-GUI, mirroring `crate::gtk::run` /
/// `crate::macos::run`.
///
/// Panic hook + swap flush, choose the backend, construct the shared
/// [`App`], derive its [`quadraui::ShellConfig`] via [`build_shell_config`],
/// hand both to the runner. Nothing else — no `gtk4::init` equivalent,
/// because `quadraui::win::run`'s Win32 bootstrap does its own setup inside
/// `run_with_shell`.
pub fn run(file_path: Option<PathBuf>) -> ExitCode {
    // The same panic hook `crate::gtk::run` / `crate::macos::run` install:
    // flush every dirty buffer to its swap file, then write a crash log.
    crate::core::swap::install_gui_crash_hook();

    // The concrete backend is chosen here, at the entry point, and handed to
    // `App`. This is the seam a future non-GTK wrapper would use to pass a
    // different `quadraui::Backend` impl through.
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
/// `app.shell_config()`: the titlebar/taskbar icon. `WM_SETICON`
/// (`ICON_BIG`/`ICON_SMALL`) needs a decodable image handed in explicitly.
/// Split out (rather than inlined in [`run`]) so a headless test can assert
/// the bytes reach `ShellConfig` without needing a live Win32 message loop.
pub(crate) fn build_shell_config(app: &App) -> quadraui::ShellConfig {
    app.shell_config()
        .with_app_icon(quadraui::ImageSource::Bytes(
            crate::app_support::APP_ICON_PNG.to_vec(),
        ))
}

/// Same reasoning as `crate::gtk`'s
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
// This test needs only `WinBackend::new()` plus the two metric setters,
// none of which are `target_os`-gated — `current_line_height`/
// `current_char_width` and their setters/getters are plain fields. So this
// runs on an ordinary Linux host under `cargo test --features win`, closing
// the gap found on macOS (a metric-setter override that silently no-ops,
// disabling the click drift guard) for this backend too, without needing
// Windows hardware to run at all. See
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
// The Win-GUI gutter is produced by exactly three pieces of code:
//
// 1. **vimcode, backend-neutral** — `render::calculate_gutter_cols` /
//    `render::build_rendered_window` decide `gutter_char_width` and each
//    line's `gutter_text`, and `render::to_q_editor` packs them into the
//    `quadraui::Editor` that `App::paint_editor_windows_rung` pushes for
//    every GUI backend alike.
// 2. **quadraui, backend-neutral** — `Editor::layout_with_options` turns
//    that into the geometry the rasteriser paints with: `gutter_w =
//    gutter_char_width * cell_width`. This is plain arithmetic with no
//    WinAPI in it.
// 3. **quadraui, Win-only** — `win::editor::draw_editor`'s Direct2D/
//    DirectWrite `draw_text` calls.
//
// Rungs 1 and 2 are reachable from an ordinary Linux/macOS host under
// `cargo test --no-default-features --features win` — `WinBackend`'s
// metric getters are plain fields. The module below **executes** on any
// host, against the real production functions and `WinBackend`'s own
// `char_width()`/`line_height()`, rather than being type-checked-only.
// It is the same posture `win_ctrl_key_translation_tests_1674` below takes:
// drive as far up the real pipeline as a non-Windows host can reach, and
// say precisely where the reachable part stops.
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
        // `Engine::new_for_test` is hermetic, so this asserts #1543's shipped
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

    /// #1691's ask: a fresh, untouched-settings engine must reserve a real
    /// line-number gutter, put the line's own number in it, and inset the
    /// text column past it by exactly that many cells.
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

    /// The falsifiability control for the test above (CLAUDE.md rule 2):
    /// a `:set nonumber` engine pins the geometry #1691 *reports* — a 1-cell
    /// gutter carrying only the fold indicator. Every assertion in the positive
    /// test above fails against this state, which is what makes them coverage
    /// rather than tautology.
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
// Driver-tier coverage for the Win-GUI half. This module compiles and
// **runs** on an ordinary Linux host: `win` is gated on the Cargo feature
// alone, the same reason `win_gutter_contract_1691` above can.
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

        let mut backend = super::backend::WinBackend::new();
        // Mirror `App::setup`'s real production wiring
        // (`backend.set_editor_v_scrollbar_width(Some(14.0))`) rather than
        // leaving this probe's backend at the trait's own `None` default —
        // otherwise this probe would keep exercising a code path paint no
        // longer takes.
        backend.set_editor_v_scrollbar_width(Some(14.0));
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
        // The minimap defaults off on every backend — explicitly on
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
        // Honour the override just set above, the same way
        // `app_support::editor_scrollbar_layout` does for hit-testing and
        // `quadraui::win::editor::draw_editor` does for paint — the bare
        // `.layout()` call this replaced always passed `v_scrollbar_w =
        // None`, which silently reverted to `cell_width` regardless of the
        // backend's own override.
        let el = editor.layout_with_options_and_v_scrollbar_w(
            editor.rect,
            char_width as f32,
            line_height as f32,
            quadraui::EditorPaintOptions::default(),
            backend.editor_v_scrollbar_width(),
        );

        MinimapProbe {
            strip_width: mm.rect.width,
            v_scrollbar_width_px: el.v_scrollbar_bounds.map(|r| r.width as f64),
            char_width,
            pane_width: rw.rect.width,
            gutter_cells: rw.gutter_char_width,
        }
    }

    /// #1869 acceptance: the minimap width equals the VS Code formula's
    /// result, which is less than 120 at a pane narrow enough to be under
    /// the cap. Hand-computed independently of the helper functions (reverting
    /// either back to the pre-#1869 fraction formula must make this fail).
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

    /// #1869 acceptance: the minimap width is exactly 120 on a wide pane.
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

    /// #1869's formerly-open half, now closed: the vertical scrollbar
    /// gutter. VS Code's own default is a fixed 14px; quadraui 0.1.3 shipped
    /// the host-settable `Backend::set_editor_v_scrollbar_width`, and #1891
    /// wired `App::setup` to call it on GTK/macOS/Win — this pins the
    /// *fixed* width going forward.
    #[test]
    fn vertical_scrollbar_is_exactly_vs_codes_14px_1869() {
        let p = minimap_probe(1400.0);
        let got = p
            .v_scrollbar_width_px
            .expect("a 2000-line buffer in a 900px-tall window must overflow and reserve a vertical scrollbar");
        assert_eq!(
            got, 14.0,
            "WinBackend's `set_editor_v_scrollbar_width(Some(14.0))` override \
             (wired by `App::setup`, #1891) must make the vertical scrollbar \
             paint — and, via `editor_v_scrollbar_width()`, hit-test — at \
             exactly VS Code's fixed 14px rather than the editor's own font- \
             sized cell width (got {got}, char_width={})",
            p.char_width
        );
    }
}

// ── #1696: editor text not inset by the minimap strip; tab-strip
// overflow-action toolbar clipped off the window's right edge ───────────
//
// Both symptoms root-cause to code (vimcode's own `to_q_editor`, and the
// pinned `quadraui::primitives::tab_bar::TabBar::layout`) that every
// GUI/TUI backend shares equally, not anything specific to `WinBackend`.
// Nothing in `src/win/` changes for either half.

// ── #1674: Ctrl-modified shortcuts (Ctrl+`, Ctrl+B) never dispatched on
// Win-GUI ────────────────────────────────────────────────────────────────
//
// Like a backend trait method, "Ctrl+<key> reaches the engine" is not a
// `src/win/` decision: `App::setup` registers the same accelerator table
// on every GUI backend, `Engine::handle_vscode_key` is the shared handler,
// and the accelerator-matching itself is shared code every backend delegates
// to. The one layer that differs is upstream: the raw `WM_KEYDOWN`/`WM_CHAR`
// → `quadraui::UiEvent` translation (`quadraui::win::events`), which is the
// one thing genuinely different between backends.
//
// Source-level investigation (pinned rev `ca7fcc83afad01ec3422f79366566f3a263b22bf`)
// found that `events::wm_char_to_uievent` — the pure `WM_CHAR` → `UiEvent`
// translator — correctly recovers both chords when fed the payload Windows
// delivers for each:
//
// - Ctrl+B: Windows converts Ctrl+letter to its C0 control code (`0x02` for B),
//   and `ctrl_b_wm_char_recovers_the_base_letter_1674` confirms
//   `wm_char_to_uievent('\x02', {ctrl:true}, _)` correctly recovers
//   `Key::Char('b')` with `ctrl == true`.
// - Ctrl+`: backtick is not a control character, so `wm_char_to_uievent`
//   falls straight to the final arm and `ctrl_backtick_wm_char_passes_through_
//   with_ctrl_held_1674` confirms it produces exactly the `KeyPressed(Char('`'),
//   ctrl: true)` event `Engine::handle_vscode_key` needs.
//
// **The translation function itself is not at fault for either chord.**
// What remains is an OS-level question: does Windows' real `TranslateMessage`
// actually generate a `WM_CHAR` message for Ctrl+backtick? Win32 references
// describe `WM_CHAR` generation for Ctrl held with a non-letter key as
// layout/driver-dependent. Confirming this needs a live Windows message loop.
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
// #928's AC2 is met as of quadraui#1038 — `win::testing` is now gated on
// `feature = "win"` alone instead of `target_os = "windows"`, with every
// real Direct2D/GDI call individually `cfg(target_os = "windows")`-stubbed.
// So this module only needs `#[cfg(test)]` to type-check on Linux.
//
// Type-checking is not the same as running: `HeadlessSurface::new` returns
// `Err` off Windows, so each `#[test]` is itself `cfg_attr(target_os =
// "windows", test)`-gated. The function body is always compiled and
// type-checked, but only registered — and run — as an actual test on real
// Windows.
//
// Bodies mirror `src/macos/mod.rs::mac_driver_tests::conformance_proof_slice`
// exactly. RED-verification could not be run against `WinDriver` because the
// `cargo xwin test --lib` product crashes at Windows DLL-load time off real
// Windows. GTK and macOS were both RED-verified on real hardware against the
// identical vimcode-side mutation.
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
    /// ConformanceHarness::screen_layout`] handle — needed by any scenario
    /// that must locate a *window's* own painted rect (e.g. the minimap strip)
    /// rather than a text run's.
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
    /// — needed by [`activity_bar_paints_active_accent_strip_
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

    /// Scenario 1: open/filter/Esc-dismiss the folder picker via
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

    /// Scenario 2: the command palette's open/filter/Esc cycle, via
    /// `WinDriver`.
    #[cfg_attr(target_os = "windows", test)]
    fn command_palette_filters_and_escape_dismisses() {
        let mut h = conformance_harness(plain_engine(), 1400, 900);

        crate::harness::command_palette_filters_and_escape_dismisses(&mut h.driver);
    }

    /// Scenario 3: a click outside the open folder picker's popup
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
    // `WinBackend` now declares only `window_chrome`, matching `GtkBackend`.
    // Bodies mirror `src/gtk/testing.rs`'s own `window_chrome`-shaped
    // assertions. Like the rest of this module, only registered as an actual
    // `#[test]` on real Windows.

    /// `WinBackend` declares `BackendCaps::window_chrome` (and no longer
    /// `native_menu`), so `App::setup` takes the `window_chrome` arm and
    /// paints the in-window drawn menu row — the same shape `GtkBackend`
    /// already exercises.
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

    /// The Command Center paints in the title-bar band on `WinBackend`
    /// alongside the drawn menu row — the same title-bar-band gate
    /// (`render::FramePresence::from_screen`) `src/gtk/testing.rs`'s
    /// `command_center_paints_between_menu_labels_and_window_controls`
    /// exercises for GTK.
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
    // This is the Win-GUI mirror of the GTK test. The two findings this can
    // check — the File menu item's real left edge, and the Command Center
    // search box coverage — are both reached through `WinDriver::find_bounds`.
    // Only type-checked on this Linux worktree (`#[cfg_attr(target_os =
    // "windows", test)]`) — see the module-top doc for why it cannot yet be
    // *executed* on this host.
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
    // This confirms by source reading that caption buttons should answer
    // `HTCLIENT` (correct behaviour). This is a source-level assertion,
    // not an executed one, for the same DLL-load-crash reason every other
    // test in this module carries.
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
    // Probes a blank point inside the Command Center but left of its
    // leftmost widget. By source reasoning this point should read HTCAPTION,
    // but needs live `DWrite` measurement to confirm, which has no
    // non-Windows implementation. Only type-checked on this Linux worktree.
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

        // A point inside the Command Center's own container, but strictly
        // left of its leftmost painted widget — the exact blank padding this
        // issue reports.
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
    // `win::editor::draw_editor`, `win::tree::draw_tree`, and several other
    // functions construct a fresh `Theme::default()` instead of reading
    // `self.current_theme`. `GtkBackend`/`MacBackend`'s equivalents already
    // read the live theme. This is why the minimap alone flips when the
    // colorscheme changes, while the editor and Explorer stay dark.
    //
    // Probes two of these functions — `draw_editor`'s `theme.background`
    // and `draw_tree`'s `theme.tab_bar_bg` — the pair this issue's own
    // screenshot evidence calls out most directly. Only type-checked on this
    // Linux worktree.
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

        let theme = crate::render::Theme::light();

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
    // The minimap paints white when `WinBackend` has no live Direct2D
    // surface yet, because neither `draw_minimap` nor `begin_frame`'s `Clear()`
    // checks `MinimapPaintResult::painted` to retry. This test proves that
    // once a surface *is* attached, the minimap strip paints `theme.background`
    // correctly — a regression guard for whichever fix lands upstream. Only
    // type-checked on this Linux worktree.
    #[cfg_attr(target_os = "windows", test)]
    fn minimap_strip_background_matches_theme_once_a_surface_is_attached_1676() {
        let mut engine = plain_engine();
        // vscode-dark keeps `theme.background` far from white (`rgb(255,
        // 255, 255)`) — the exact colour vimcode#1676 reports — so a
        engine.settings.colorscheme = "vscode-dark".to_string();
        // Minimap is off by default on every backend now — this probe is
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
    // The full pipeline (vimcode's shared `render::calculate_gutter_cols`
    // through `render::to_q_editor` through `quadraui::win::editor::draw_editor`)
    // is Win-specific nowhere and differs from GTK nowhere. None can
    // legitimately produce `gutter_char_width == 0` for a real window.
    //
    // This function is the *pixel* half and is only type-checked on this
    // Linux worktree. The *arithmetic* half is covered by
    // `super::win_gutter_contract_1691`, which does execute on every host
    // and was RED-verified by injecting the reported defect.
    #[cfg_attr(target_os = "windows", test)]
    fn line_number_gutter_paints_and_insets_text_by_default_1691() {
        let mut engine = plain_engine();
        engine.settings.colorscheme = "vscode-dark".to_string();
        // Deliberately NOT setting `engine.settings.line_numbers` — #1543
        // made `LineNumberMode::Absolute` the untouched default.
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
        // colour: `tests/no_hardcoded_colors.rs` exempts only
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
    // Win-GUI mirror of the GTK test. Both backends share the identical
    // `window_chrome` title-band composition, so the GTK twin (which does
    // run headlessly and was confirmed) is a faithful stand-in. Only
    // type-checked on this Linux worktree.
    #[cfg_attr(target_os = "windows", test)]
    fn clicking_the_stale_1232_file_x_opens_nothing_1673() {
        let mut h = conformance_harness(plain_engine(), 1024, 768);

        assert!(
            !h.driver.screen_contains("New Tab"),
            "sanity: the File dropdown must be closed before either click"
        );

        // vimcode#1673's reproduction coordinate — the stale pre-#1657
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
    // shared dispatch pipeline ──────────────────────────────────────────────
    //
    // This deliberately dispatches Ctrl+P (`render::ACC_FUZZY_FINDER`)
    // rather than Ctrl+B, which `WinDriver` cannot reach (it has no `.tick()`
    // to apply deferred actions). Ctrl+P is one of the synchronous actions
    // reachable without `.tick()` and goes through the identical accelerator
    // path as Ctrl+B would.
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
    // `render::build_activity_bar` *does* set `active_accent`, but every real
    // `App` renders through `quadraui::AppShell::build_activity_bar` instead,
    // which this repo never overrides. `AppShell::build_activity_bar` hardcodes
    // `active_accent: None` unconditionally, with no theme in scope to source
    // a colour from. Nothing in this vimcode repo can fix this — `AppShell`
    // and its `build_activity_bar` live entirely inside quadraui.
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
    // No backend-specific code diverges between GTK and Win-GUI in the
    // entire pipeline. `render::breadcrumbs_to_quadraui_status_bar` / `render::
    // tab_bar_height_px` have no `cfg` branch. This scenario ships the missing
    // Tier-1 coverage #1694 asks for, but is a source-level assertion (not
    // executed) — dell64 needs to confirm the flip directly.
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
